use std::slice;

use serde::Deserialize;
use serde_json::{Value, json};

use super::graphql::{Envelope, Request, View, repeated};
use super::repository::{
    locate_repository, lookup_failed, missing_revisions, missing_sides, no_commits,
    repository_variables, revision_lookup_failed,
};
use crate::address::{Repo, is_plain_path, short};
use crate::error::Error;

const COMMIT: &str = "\
query ShowCommit($repo0: String!, $repo1: String!, $gitlab: Boolean!, $rev: String!, \
$base: String!) {
  m0: repository(name: $repo0) { ...Commit }
  d0: repository(name: $repo0) { ...Changes }
  m1: repository(name: $repo1) @include(if: $gitlab) { ...Commit }
  d1: repository(name: $repo1) @include(if: $gitlab) { ...Changes }
}

fragment Commit on Repository {
  name
  defaultBranch { displayName }
  commit(rev: $rev) {
    oid subject message
    author { date person { displayName } }
    committer { date person { displayName } }
    parents { oid }
  }
}

fragment Changes on Repository {
  name
  defaultBranch { displayName }
  comparison(base: $base, head: $rev) {
    all: fileDiffs(first: 0) { totalCount diffStat { added deleted } }
    page: fileDiffs(first: 200) { nodes { oldPath newPath stat { added deleted } } }
  }
}
";

/// A commit, with the files it changed against its first parent.
#[derive(Debug, PartialEq, Eq)]
pub struct Commit {
    /// The repository's name as the server spells it.
    pub repo: String,
    pub oid: String,
    pub subject: String,
    pub message: String,
    pub author: String,
    /// The author's timestamp.
    pub author_date: String,
    pub committer: Option<String>,
    /// The committer's timestamp.
    pub committer_date: Option<String>,
    pub parents: Vec<String>,
    /// `None` for a root commit, which has no parent to compare with.
    pub changes: Option<Changes>,
}

/// The files a comparison changed: the totals, and up to the first 200
/// files.
#[derive(Debug, PartialEq, Eq)]
pub struct Changes {
    pub total: u64,
    pub added: u64,
    pub deleted: u64,
    pub files: Vec<Change>,
}

/// One changed file. A file added has no old path, and one deleted no new
/// path.
#[derive(Debug, PartialEq, Eq)]
pub struct Change {
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub added: u64,
    pub deleted: u64,
}

/// The commit `rev` names in `repo`, and its changes against its first
/// parent.
pub fn commit_request(repo: &Repo, rev: &str) -> Request {
    let mut variables = repository_variables(repo);
    variables.insert("rev".to_owned(), json!(rev));
    variables.insert("base".to_owned(), json!(format!("{rev}^1")));
    Request::new("ShowCommit", COMMIT.to_owned(), variables)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommitNode {
    oid: String,
    subject: String,
    message: String,
    author: Author,
    committer: Option<Committer>,
    parents: Vec<Oid>,
}

#[derive(Deserialize)]
struct Author {
    date: String,
    person: Person,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Person {
    display_name: String,
}

#[derive(Deserialize)]
struct Committer {
    date: Option<String>,
    person: Option<CommitterPerson>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommitterPerson {
    display_name: Option<String>,
}

#[derive(Deserialize)]
struct Oid {
    oid: String,
}

#[derive(Deserialize)]
struct CommitComparison {
    all: FileDiffTotals,
    page: FileDiffPage,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileDiffTotals {
    total_count: u64,
    diff_stat: Stat,
}

#[derive(Deserialize)]
struct Stat {
    added: u64,
    deleted: u64,
}

#[derive(Deserialize)]
struct FileDiffPage {
    nodes: Vec<FileDiff>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileDiff {
    old_path: Option<String>,
    new_path: Option<String>,
    stat: Option<NodeStat>,
    old_file: Option<OldFile>,
}

/// The file before the change, which names its commit when it is a blob.
#[derive(Deserialize)]
struct OldFile {
    commit: Option<Oid>,
}

/// A file's counts, read as 0 when absent.
#[derive(Deserialize)]
struct NodeStat {
    added: Option<u64>,
    deleted: Option<u64>,
}

impl FileDiff {
    fn change(self) -> Change {
        let (added, deleted) =
            self.stat.map_or((0, 0), |s| (s.added.unwrap_or(0), s.deleted.unwrap_or(0)));
        Change { old_path: self.old_path, new_path: self.new_path, added, deleted }
    }
}

pub fn read_commit(envelope: &Envelope, repo: &Repo, rev: &str) -> Result<Commit, Error> {
    let found = locate_repository(repo, envelope, "m", &[])?;
    let (at_m, at_d) = (found.at("m"), found.at("d"));
    let m = View::new(envelope, slice::from_ref(&at_m));
    let d = View::new(envelope, slice::from_ref(&at_d));
    if lookup_failed(&m.errors_beneath(&["commit"])) {
        return Err(revision_lookup_failed());
    }
    if m.get(&["commit"]).is_none() {
        let repo = &found.repo;
        return Err(no_commits(repo, &m).unwrap_or_else(|| missing_revisions(repo, &[("", rev)])));
    }
    let node: CommitNode = m.read(&["commit"]).ok_or_else(revision_lookup_failed)?;
    let (committer, committer_date) = match node.committer {
        Some(c) => (c.person.and_then(|p| p.display_name), c.date),
        None => (None, None),
    };
    let parents: Vec<String> = node.parents.into_iter().map(|p| p.oid).collect();
    let changes = if parents.is_empty() {
        None
    } else {
        let failed = || Error::failed("the file list");
        if d.errored_beneath(&[]) {
            return Err(failed());
        }
        let comparison: CommitComparison = d.read(&["comparison"]).ok_or_else(failed)?;
        Some(Changes {
            total: comparison.all.total_count,
            added: comparison.all.diff_stat.added,
            deleted: comparison.all.diff_stat.deleted,
            files: comparison.page.nodes.into_iter().map(FileDiff::change).collect(),
        })
    };
    Ok(Commit {
        repo: found.repo,
        oid: node.oid,
        subject: node.subject,
        message: node.message,
        author: node.author.person.display_name,
        author_date: node.author.date,
        committer,
        committer_date,
        parents,
        changes,
    })
}

/// Two commits and the files that changed between them.
#[derive(Debug, PartialEq, Eq)]
pub struct Comparison {
    /// The repository's name as the server spells it.
    pub repo: String,
    /// The commit the base revision names.
    pub base: String,
    /// The commit the head revision names.
    pub head: String,
    /// The commit the comparison started from: the merge base, read from the
    /// first file that existed before.
    pub merge_base: Option<String>,
    pub changes: Changes,
    /// The patch of the paths given, when there were any.
    pub patch: Option<String>,
}

/// Two commits, each with whether each path exists in it, `#` numbering the
/// paths, and the files that changed between them, with the patch of the
/// paths when `$patch`.
const COMPARISON: &str = "\
query Diff($repo0: String!, $repo1: String!, $gitlab: Boolean!, $a: String!, $b: String!, \
$paths: [String!], $first: Int, $patch: Boolean!{declared}) {
  a0: repository(name: $repo0) { ...Base }
  b0: repository(name: $repo0) { ...Head }
  c0: repository(name: $repo0) { ...Comparison }
  a1: repository(name: $repo1) @include(if: $gitlab) { ...Base }
  b1: repository(name: $repo1) @include(if: $gitlab) { ...Head }
  c1: repository(name: $repo1) @include(if: $gitlab) { ...Comparison }
}

fragment Base on Repository {
  name
  defaultBranch { displayName }
  commit(rev: $a) {
    oid
{probes}
  }
}

fragment Head on Repository {
  name
  defaultBranch { displayName }
  commit(rev: $b) {
    oid parents { oid }
{probes}
  }
}

fragment Comparison on Repository {
  name
  defaultBranch { displayName }
  comparison(base: $a, head: $b) {
    all: fileDiffs(first: 0, paths: $paths) { totalCount diffStat { added deleted } }
    page: fileDiffs(first: $first, paths: $paths) {
      nodes { oldPath newPath stat { added deleted } oldFile { ... on GitBlob { commit { oid } } } }
    }
    patch: fileDiffs(paths: $paths) @include(if: $patch) { rawDiff }
  }
}
";

/// The comparison of `head` with `base`, or with its first parent for
/// `None`, over `paths`, listing at most `first` files or all for `None`.
pub fn comparison_request(
    repo: &Repo,
    base: Option<&str>,
    head: &str,
    paths: &[String],
    first: Option<u32>,
) -> Request {
    let base = base.map_or_else(|| format!("{head}^1"), str::to_owned);
    let checked: Vec<&String> = paths.iter().filter(|p| is_plain_path(p)).collect();
    let mut variables = repository_variables(repo);
    variables.insert("a".to_owned(), json!(base));
    variables.insert("b".to_owned(), json!(head));
    variables.insert("paths".to_owned(), if paths.is_empty() { Value::Null } else { json!(paths) });
    variables.insert("first".to_owned(), json!(first));
    variables.insert("patch".to_owned(), json!(!paths.is_empty()));
    for (n, path) in checked.iter().enumerate() {
        variables.insert(format!("path{n}"), json!(path));
    }
    let probes = repeated("    p#: path(path: $path#) { __typename }", checked.len());
    let declared = repeated(", $path#: String!", checked.len());
    let document = COMPARISON.replace("{declared}", &declared).replace("{probes}", &probes);
    Request::new("Diff", document, variables)
}

#[derive(Deserialize)]
struct Patch {
    #[serde(rename = "rawDiff")]
    raw_diff: String,
}

#[derive(Deserialize)]
struct DiffComparison {
    all: FileDiffTotals,
    page: FileDiffPage,
    patch: Option<Patch>,
}

/// The commit a side of the comparison names: `None` when it names none,
/// an error when the server didn't say which.
fn side(view: &View<'_>) -> Result<Option<String>, Error> {
    if view.get(&["commit"]).is_none() {
        return Ok(None);
    }
    view.read(&["commit", "oid"]).map(Some).ok_or_else(revision_lookup_failed)
}

pub fn read_comparison(
    envelope: &Envelope,
    repo: &Repo,
    base: Option<&str>,
    head: &str,
    paths: &[String],
) -> Result<Comparison, Error> {
    let found = locate_repository(repo, envelope, "a", &[])?;
    let (at_a, at_b, at_c) = (found.at("a"), found.at("b"), found.at("c"));
    let a = View::new(envelope, slice::from_ref(&at_a));
    let b = View::new(envelope, slice::from_ref(&at_b));
    let c = View::new(envelope, slice::from_ref(&at_c));
    let name = &found.repo;
    if lookup_failed(&a.errors_beneath(&["commit"]))
        || lookup_failed(&b.errors_beneath(&["commit"]))
    {
        return Err(revision_lookup_failed());
    }
    let (commit_a, commit_b) = (side(&a)?, side(&b)?);
    if commit_a.is_none()
        && commit_b.is_none()
        && let Some(error) = no_commits(name, &a)
    {
        return Err(error);
    }
    let root =
        b.read::<Vec<Value>>(&["commit", "parents"]).is_some_and(|parents| parents.is_empty());
    if base.is_none() && commit_a.is_none() && commit_b.is_some() && root {
        return Err(Error::missing(format!(
            "{head} is a root commit: Sourcegraph has no diff for it"
        )));
    }
    let (Some(commit_a), Some(commit_b)) = (commit_a, commit_b) else {
        return Err(match base {
            None => missing_revisions(name, &[("", head)]),
            Some(base) => missing_sides(name, (base, &a), (head, &b)),
        });
    };
    for (n, path) in paths.iter().filter(|p| is_plain_path(p)).enumerate() {
        let probe = format!("p{n}");
        let keys = ["commit", probe.as_str()];
        if a.get(&keys).is_none() && b.get(&keys).is_none() {
            return Err(Error::missing(format!(
                "{path} not found in {name}@{} or @{}",
                short(&commit_a),
                short(&commit_b)
            )));
        }
    }
    let failed = || Error::failed("the comparison");
    if c.errored_beneath(&[]) {
        return Err(failed());
    }
    let comparison: DiffComparison = c.read(&["comparison"]).ok_or_else(failed)?;
    let patch =
        if paths.is_empty() { None } else { Some(comparison.patch.ok_or_else(failed)?.raw_diff) };
    let merge_base = comparison
        .page
        .nodes
        .iter()
        .find_map(|node| node.old_file.as_ref()?.commit.as_ref().map(|c| c.oid.clone()));
    Ok(Comparison {
        repo: found.repo,
        base: commit_a,
        head: commit_b,
        merge_base,
        changes: Changes {
            total: comparison.all.total_count,
            added: comparison.all.diff_stat.added,
            deleted: comparison.all.diff_stat.deleted,
            files: comparison.page.nodes.into_iter().map(FileDiff::change).collect(),
        },
        patch,
    })
}

/// The most tags a log reads to mark its commits with.
pub const NEWEST_TAGS: u64 = 1000;

/// The history of a revision, its first commits, and the newest tags.
const HISTORY: &str = "\
query Log($repo0: String!, $repo1: String!, $gitlab: Boolean!, $rev: String!, $path: String, \
$first: Int) {
  r0: repository(name: $repo0) { ...History }
  t0: repository(name: $repo0) { ...Tags }
  r1: repository(name: $repo1) @include(if: $gitlab) { ...History }
  t1: repository(name: $repo1) @include(if: $gitlab) { ...Tags }
}

fragment History on Repository {
  name
  defaultBranch { displayName }
  commit(rev: $rev) { oid ancestors(first: $first, path: $path) { ...Page } }
}
fragment Page on GitCommitConnection {
  nodes { oid subject author { date person { displayName } } parents { oid } }
  pageInfo { hasNextPage }
}

fragment Tags on Repository {
  name
  defaultBranch { displayName }
  gitRefs(type: GIT_TAG, first: 1000) { totalCount nodes { displayName target { oid } } }
}
";

/// The commits in a range's head and not in its base, the count of them
/// when `$count`, whether the path exists at each end when `$probe`, and
/// the newest tags.
const RANGE: &str = "\
query Log($repo0: String!, $repo1: String!, $gitlab: Boolean!, $a: String!, $b: String!, \
$path: String, $first: Int, $count: Boolean!, $probe: Boolean!) {
  a0: repository(name: $repo0) { ...Base }
  b0: repository(name: $repo0) { ...Head }
  n0: repository(name: $repo0) { ...Ahead }
  c0: repository(name: $repo0) { ...Commits }
  t0: repository(name: $repo0) { ...Tags }
  a1: repository(name: $repo1) @include(if: $gitlab) { ...Base }
  b1: repository(name: $repo1) @include(if: $gitlab) { ...Head }
  n1: repository(name: $repo1) @include(if: $gitlab) { ...Ahead }
  c1: repository(name: $repo1) @include(if: $gitlab) { ...Commits }
  t1: repository(name: $repo1) @include(if: $gitlab) { ...Tags }
}

fragment Base on Repository {
  name
  defaultBranch { displayName }
  commit(rev: $a) { oid x: path(path: $path) @include(if: $probe) { __typename } }
}

fragment Head on Repository {
  name
  defaultBranch { displayName }
  commit(rev: $b) { oid x: path(path: $path) @include(if: $probe) { __typename } }
}

fragment Ahead on Repository {
  name
  defaultBranch { displayName }
  commit(rev: $b) @include(if: $count) { behindAhead(revspec: $a) { ahead } }
}

fragment Commits on Repository {
  name
  defaultBranch { displayName }
  comparison(base: $a, head: $b) { commits(first: $first, path: $path) { ...Page } }
}
fragment Page on GitCommitConnection {
  nodes { oid subject author { date person { displayName } } parents { oid } }
  pageInfo { hasNextPage }
}

fragment Tags on Repository {
  name
  defaultBranch { displayName }
  gitRefs(type: GIT_TAG, first: 1000) { totalCount nodes { displayName target { oid } } }
}
";

/// The history of a revision: its commit and the commits before it.
#[derive(Debug, PartialEq, Eq)]
pub struct History {
    /// The repository's name as the server spells it.
    pub repo: String,
    /// The commit the revision names.
    pub commit: String,
    pub log: Log,
}

/// The commits in a range's head and not in its base.
#[derive(Debug, PartialEq, Eq)]
pub struct RangeHistory {
    /// The repository's name as the server spells it.
    pub repo: String,
    /// The commit the base revision names.
    pub base: String,
    /// The commit the head revision names.
    pub head: String,
    /// How many commits the head has that the base doesn't, when no path was
    /// given.
    pub ahead: Option<u64>,
    pub log: Log,
}

/// A page of commits, newest first, and the repository's newest tags.
#[derive(Debug, PartialEq, Eq)]
pub struct Log {
    pub commits: Vec<CommitSummary>,
    /// Whether the server holds more commits after the page.
    pub more: bool,
    /// The newest tags, at most `NEWEST_TAGS`, with the commit each names.
    pub tags: Vec<(String, String)>,
    /// How many tags the repository has.
    pub tags_total: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CommitSummary {
    pub oid: String,
    pub subject: String,
    pub author: String,
    /// The author's timestamp.
    pub author_date: String,
    pub parents: Vec<String>,
}

/// The history of `rev`, or of `HEAD` for `None`, touching `path`, the
/// first `first` commits.
pub fn history_request(repo: &Repo, rev: Option<&str>, path: Option<&str>, first: u32) -> Request {
    let mut variables = repository_variables(repo);
    variables.insert("rev".to_owned(), json!(rev.unwrap_or("HEAD")));
    variables.insert("path".to_owned(), json!(path));
    variables.insert("first".to_owned(), json!(first));
    Request::new("Log", HISTORY.to_owned(), variables)
}

/// The commits in `head` and not in `base` touching `path`, the first
/// `first`, or all for `None`.
pub fn range_request(
    repo: &Repo,
    (base, head): (&str, &str),
    path: Option<&str>,
    first: Option<u32>,
) -> Request {
    let mut variables = repository_variables(repo);
    variables.insert("a".to_owned(), json!(base));
    variables.insert("b".to_owned(), json!(head));
    variables.insert("path".to_owned(), json!(path));
    variables.insert("first".to_owned(), json!(first));
    variables.insert("count".to_owned(), json!(path.is_none()));
    variables.insert("probe".to_owned(), json!(path.is_some_and(is_plain_path)));
    Request::new("Log", RANGE.to_owned(), variables)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommitPage {
    nodes: Vec<SummaryNode>,
    page_info: PageInfo,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
}

#[derive(Deserialize)]
struct SummaryNode {
    oid: String,
    subject: String,
    author: Author,
    parents: Vec<Oid>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TagList {
    total_count: u64,
    nodes: Vec<TagNode>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TagNode {
    display_name: Option<String>,
    target: Option<TagTarget>,
}

#[derive(Deserialize)]
struct TagTarget {
    oid: Option<String>,
}

/// The commits of a page, newest first, and whether more follow.
fn page(view: &View<'_>, tail: &[&str]) -> Option<(Vec<CommitSummary>, bool)> {
    let page: CommitPage = view.read(tail)?;
    let commits = page
        .nodes
        .into_iter()
        .map(|node| CommitSummary {
            oid: node.oid,
            subject: node.subject,
            author: node.author.person.display_name,
            author_date: node.author.date,
            parents: node.parents.into_iter().map(|p| p.oid).collect(),
        })
        .collect();
    Some((commits, page.page_info.has_next_page))
}

/// The newest tags and how many there are, from the `t` root.
fn tags(t: &View<'_>) -> Result<(Vec<(String, String)>, u64), Error> {
    let failed = || Error::failed("the tag list");
    if t.errored_beneath(&[]) {
        return Err(failed());
    }
    let list: TagList = t.read(&["gitRefs"]).ok_or_else(failed)?;
    let tags = list
        .nodes
        .into_iter()
        .filter_map(|node| Some((node.display_name?, node.target?.oid?)))
        .collect();
    Ok((tags, list.total_count))
}

pub fn read_history(
    envelope: &Envelope,
    repo: &Repo,
    rev: Option<&str>,
    path: Option<&str>,
) -> Result<History, Error> {
    let found = locate_repository(repo, envelope, "r", &[])?;
    let (at_r, at_t) = (found.at("r"), found.at("t"));
    let r = View::new(envelope, slice::from_ref(&at_r));
    let t = View::new(envelope, slice::from_ref(&at_t));
    let name = &found.repo;
    let list_failed = || Error::failed("the commit list");
    if r.errored_beneath(&["commit", "ancestors"]) {
        return Err(list_failed());
    }
    if lookup_failed(&r.errors_beneath(&["commit"])) {
        return Err(revision_lookup_failed());
    }
    if r.get(&["commit"]).is_none() {
        let sent = rev.unwrap_or("HEAD");
        return Err(no_commits(name, &r).unwrap_or_else(|| missing_revisions(name, &[("", sent)])));
    }
    let commit: String = r.read(&["commit", "oid"]).ok_or_else(revision_lookup_failed)?;
    let (commits, more) = page(&r, &["commit", "ancestors"]).ok_or_else(list_failed)?;
    if let Some(path) = path.filter(|p| is_plain_path(p))
        && commits.is_empty()
    {
        return Err(Error::missing(format!("{path} not found in {name}@{}", short(&commit))));
    }
    let (tags, tags_total) = tags(&t)?;
    Ok(History { repo: found.repo, commit, log: Log { commits, more, tags, tags_total } })
}

pub fn read_range(
    envelope: &Envelope,
    repo: &Repo,
    (base, head): (&str, &str),
    path: Option<&str>,
) -> Result<RangeHistory, Error> {
    let found = locate_repository(repo, envelope, "a", &[])?;
    let (at_a, at_b) = (found.at("a"), found.at("b"));
    let (at_n, at_c, at_t) = (found.at("n"), found.at("c"), found.at("t"));
    let a = View::new(envelope, slice::from_ref(&at_a));
    let b = View::new(envelope, slice::from_ref(&at_b));
    let count = View::new(envelope, slice::from_ref(&at_n));
    let compared = View::new(envelope, slice::from_ref(&at_c));
    let t = View::new(envelope, slice::from_ref(&at_t));
    let name = &found.repo;
    if lookup_failed(&a.errors_beneath(&["commit"]))
        || lookup_failed(&b.errors_beneath(&["commit"]))
    {
        return Err(revision_lookup_failed());
    }
    let (commit_a, commit_b) = (side(&a)?, side(&b)?);
    if commit_a.is_none()
        && commit_b.is_none()
        && let Some(error) = no_commits(name, &a)
    {
        return Err(error);
    }
    let (Some(commit_a), Some(commit_b)) = (commit_a, commit_b) else {
        return Err(missing_sides(name, (base, &a), (head, &b)));
    };
    let listed = !compared.errored_beneath(&[]);
    let empty = listed
        && compared
            .read::<Vec<Value>>(&["comparison", "commits", "nodes"])
            .is_some_and(|nodes| nodes.is_empty());
    if let Some(path) = path.filter(|p| is_plain_path(p))
        && a.get(&["commit", "x"]).is_none()
        && b.get(&["commit", "x"]).is_none()
        && empty
    {
        return Err(Error::missing(format!(
            "{path} not found in {name}@{} or @{}",
            short(&commit_a),
            short(&commit_b)
        )));
    }
    let comparison_failed = || Error::failed("the comparison");
    let (commits, more) = listed
        .then(|| page(&compared, &["comparison", "commits"]))
        .flatten()
        .ok_or_else(comparison_failed)?;
    let ahead = if path.is_none() {
        let count_failed = || Error::failed("the commit count");
        if count.errored_beneath(&[]) {
            return Err(count_failed());
        }
        Some(count.read(&["commit", "behindAhead", "ahead"]).ok_or_else(count_failed)?)
    } else {
        None
    };
    let (tags, tags_total) = tags(&t)?;
    Ok(RangeHistory {
        repo: found.repo,
        base: commit_a,
        head: commit_b,
        ahead,
        log: Log { commits, more, tags, tags_total },
    })
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::error::report;
    use crate::sourcegraph::parsed;

    const REPO: &str = "example.com/acme/widget";
    const OID: &str = "affe2b9b7c608f4e354d7e83d0583ed35e845650";
    const PARENT: &str = "6b668aa526afdc623c1a9ed1d6ae920e04a717ad";
    const SECRET: &str = "git command [git show --secret] failed";

    fn commit() -> Value {
        json!({
            "oid": OID,
            "subject": "m",
            "message": "m",
            "author": {
                "date": "2026-09-09T20:14:26Z",
                "person": {"displayName": "Jonathan Davies"},
            },
            "committer": {
                "date": "2026-09-10T01:02:03Z",
                "person": {"displayName": "GitHub"},
            },
            "parents": [{"oid": PARENT}],
        })
    }

    fn comparison() -> Value {
        json!({
            "all": {"totalCount": 1, "diffStat": {"added": 1, "deleted": 1}},
            "page": {"nodes": [{
                "oldPath": "a.go",
                "newPath": "a.go",
                "stat": {"added": 1, "deleted": 1},
            }]},
        })
    }

    fn body() -> Value {
        json!({"data": {
            "m0": {"name": REPO, "defaultBranch": {"displayName": "main"}, "commit": commit()},
            "d0": {"name": REPO, "defaultBranch": {"displayName": "main"}, "comparison": comparison()},
        }})
    }

    fn errors(path: &Value) -> Value {
        json!([{"message": SECRET, "path": path}])
    }

    fn commit_mut(body: &mut Value) -> &mut Value {
        &mut body["data"]["m0"]["commit"]
    }

    fn read(body: &Value) -> Result<Commit, String> {
        read_commit(&parsed(body), &Repo::Exact(REPO.to_owned()), "v1").map_err(|e| report(&e))
    }

    fn failed(body: &Value, part: &str) {
        let written = read(body).expect_err("the commit fails");
        assert_eq!(written, format!("sego: error: Sourcegraph failed to answer {part}\n"));
    }

    #[test]
    fn a_commit_is_read_with_its_changes() {
        let commit = read(&body()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(commit.repo, REPO);
        assert_eq!(commit.oid, OID);
        assert_eq!(
            (commit.author.as_str(), commit.author_date.as_str()),
            ("Jonathan Davies", "2026-09-09T20:14:26Z")
        );
        assert_eq!(commit.committer.as_deref(), Some("GitHub"));
        assert_eq!(commit.parents, [PARENT]);
        let change = Change {
            old_path: Some("a.go".to_owned()),
            new_path: Some("a.go".to_owned()),
            added: 1,
            deleted: 1,
        };
        assert_eq!(
            commit.changes,
            Some(Changes { total: 1, added: 1, deleted: 1, files: vec![change] })
        );
    }

    #[test]
    fn a_root_commit_has_no_changes_and_its_comparison_is_not_read() {
        let mut body = body();
        commit_mut(&mut body)["parents"] = json!([]);
        body["data"]["d0"] = Value::Null;
        body["errors"] = errors(&json!(["d0", "comparison"]));
        assert_eq!(read(&body).unwrap_or_else(|e| panic!("{e}")).changes, None);
    }

    #[test]
    fn an_error_at_the_revision_hides_the_server_text() {
        for path in [
            json!(["m0", "commit"]),
            json!(["m0", "commit", "message"]),
            json!(["m0", "commit", "author", "date"]),
            json!(["m0", "commit", "parents"]),
        ] {
            let mut body = body();
            body["errors"] = errors(&path);
            if path.as_array().map(Vec::len) == Some(2) {
                *commit_mut(&mut body) = Value::Null;
            }
            failed(&body, "the revision lookup");
        }
    }

    #[test]
    fn an_error_at_the_revision_outranks_one_at_the_file_list() {
        let mut body = body();
        *commit_mut(&mut body) = Value::Null;
        body["data"]["d0"] = Value::Null;
        body["errors"] = json!([
            {"message": "x", "path": ["d0", "comparison"]},
            {"message": SECRET, "path": ["m0", "commit"]},
        ]);
        failed(&body, "the revision lookup");
    }

    #[test]
    fn an_error_in_the_file_list_hides_the_server_text() {
        for (path, nulls) in [
            (json!(["d0"]), true),
            (json!(["d0", "comparison"]), true),
            (json!(["d0", "comparison", "page", "nodes"]), false),
            (json!(["d0", "comparison", "all", "totalCount"]), false),
        ] {
            let mut body = body();
            body["errors"] = errors(&path);
            if nulls {
                body["data"]["d0"] = Value::Null;
            }
            failed(&body, "the file list");
        }
    }

    #[test]
    fn a_missing_author_fails_the_revision_lookup() {
        for path in [
            &["author"][..],
            &["author", "date"],
            &["author", "person"],
            &["author", "person", "displayName"],
        ] {
            let mut body = body();
            let mut at = commit_mut(&mut body);
            for key in path {
                at = &mut at[*key];
            }
            *at = Value::Null;
            failed(&body, "the revision lookup");
        }
    }

    #[test]
    fn a_missing_committer_or_file_count_is_read_as_none_or_zero() {
        let mut body = body();
        commit_mut(&mut body)["committer"] = Value::Null;
        body["data"]["d0"]["comparison"]["page"]["nodes"][0]["stat"] = Value::Null;
        let commit = read(&body).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((commit.committer, commit.committer_date), (None, None));
        let files = commit.changes.map(|c| c.files).unwrap_or_default();
        assert_eq!((files[0].added, files[0].deleted), (0, 0));
    }

    const WAITED: &str = "revision not found after waiting for a fetch already in progress: repo 1";

    /// `body` with the value at `path` null, and the same with the server's
    /// not-found error at `path` as well.
    fn not_found_both_ways(body: &Value, path: &[&str]) -> (Value, Value) {
        let mut null = body.clone();
        let mut at = &mut null["data"];
        for key in path {
            at = &mut at[*key];
        }
        *at = Value::Null;
        let mut errored = null.clone();
        errored["errors"] = json!([{"message": WAITED, "path": path}]);
        (null, errored)
    }

    #[test]
    fn a_revision_the_server_says_is_not_found_reads_as_one_whose_commit_is_null() {
        let not_found = |result: Option<String>| {
            assert!(result.as_deref().is_some_and(|e| e.contains("not found in")), "{result:?}");
            result
        };
        let (null, errored) = not_found_both_ways(&body(), &["m0", "commit"]);
        assert_eq!(read(&errored).err(), not_found(read(&null).err()));
        for side in ["a0", "b0"] {
            let (null, errored) = not_found_both_ways(&compared(), &[side, "commit"]);
            let compared = |body| compare(body, Some("v1"), &[]).err();
            assert_eq!(compared(&errored), not_found(compared(&null)));
        }
        let (null, errored) = not_found_both_ways(&rev_body(0), &["r0", "commit"]);
        let history = |body| history(body, Some("v9"), None).err();
        assert_eq!(history(&errored), not_found(history(&null)));
        for side in ["a0", "b0"] {
            let (null, errored) = not_found_both_ways(&range_body(Some(0), 0), &[side, "commit"]);
            assert_eq!(range(&errored, None).err(), not_found(range(&null, None).err()));
        }
    }

    #[test]
    fn a_missing_revision_or_an_empty_repository_is_named() {
        let mut missing = body();
        *commit_mut(&mut missing) = Value::Null;
        assert_eq!(
            read(&missing).err().as_deref(),
            Some("sego: error: revision v1 not found in example.com/acme/widget\n")
        );
        let mut empty = missing.clone();
        empty["data"]["m0"]["defaultBranch"] = Value::Null;
        assert_eq!(
            read(&empty).err().as_deref(),
            Some("sego: error: example.com/acme/widget has no commits\n")
        );
    }

    const OID_A: &str = "0123456789abcdef0123456789abcdef01234567";
    const OID_B: &str = "89abcdef0123456789abcdef0123456789abcdef";

    fn root(key: &str, value: Value) -> Value {
        let mut root = json!({"name": REPO, "defaultBranch": {"displayName": "main"}});
        root[key] = value;
        root
    }

    fn compared() -> Value {
        json!({"data": {
            "a0": root("commit", json!({"oid": OID_A})),
            "b0": root("commit", json!({
                "oid": OID_B,
                "parents": [{"oid": OID_A}],
                "p0": {"__typename": "GitBlob"},
            })),
            "c0": root("comparison", json!({
                "all": {"totalCount": 1, "diffStat": {"added": 1, "deleted": 1}},
                "page": {"nodes": [{
                    "oldPath": "a.go",
                    "newPath": "a.go",
                    "stat": {"added": 1, "deleted": 1},
                    "oldFile": {"commit": {"oid": OID_A}},
                }]},
                "patch": {"rawDiff": "diff --git a/a.go b/a.go\n"},
            })),
        }})
    }

    fn compare(body: &Value, base: Option<&str>, paths: &[&str]) -> Result<Comparison, String> {
        let paths: Vec<String> = paths.iter().map(|p| (*p).to_owned()).collect();
        read_comparison(&parsed(body), &Repo::Exact(REPO.to_owned()), base, "v2", &paths)
            .map_err(|e| report(&e))
    }

    fn compare_failure(body: &Value, paths: &[&str]) -> String {
        compare(body, Some("v1"), paths).expect_err("the comparison fails")
    }

    #[test]
    fn a_comparison_is_read_with_its_merge_base_and_patch() {
        let comparison =
            compare(&compared(), Some("v1"), &["a.go"]).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((comparison.base.as_str(), comparison.head.as_str()), (OID_A, OID_B));
        assert_eq!(comparison.merge_base.as_deref(), Some(OID_A));
        assert_eq!(comparison.changes.total, 1);
        assert_eq!(comparison.patch.as_deref(), Some("diff --git a/a.go b/a.go\n"));
        let without = compare(&compared(), Some("v1"), &[]).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(without.patch, None);
    }

    #[test]
    fn a_failed_revision_lookup_hides_the_server_text() {
        let mut commit = compared();
        commit["data"]["a0"]["commit"] = Value::Null;
        commit["errors"] = json!([{"message": SECRET, "path": ["a0", "commit"]}]);

        let mut probe = compared();
        probe["data"]["b0"]["commit"]["p0"] = Value::Null;
        probe["errors"] = json!([
            {"message": SECRET, "path": ["c0", "comparison"]},
            {"message": SECRET, "path": ["b0", "commit", "p0"]},
        ]);

        for (body, paths) in [(commit, &[][..]), (probe, &["a.go"][..])] {
            assert_eq!(
                compare_failure(&body, paths),
                "sego: error: Sourcegraph failed to answer the revision lookup\n"
            );
        }
    }

    #[test]
    fn a_failed_comparison_hides_the_server_text() {
        for (path, null_root) in [
            (json!(["c0"]), true),
            (json!(["c0", "comparison"]), true),
            (json!(["c0", "comparison", "page", "nodes"]), false),
            (json!(["c0", "comparison", "all", "totalCount"]), false),
        ] {
            let mut body = compared();
            body["errors"] = json!([{"message": SECRET, "path": path}]);
            if null_root {
                body["data"]["c0"] = Value::Null;
            }
            assert_eq!(
                compare_failure(&body, &[]),
                "sego: error: Sourcegraph failed to answer the comparison\n",
                "{path}"
            );
        }
        let mut no_patch = compared();
        no_patch["data"]["c0"]["comparison"]["patch"] = Value::Null;
        assert_eq!(
            compare_failure(&no_patch, &["a.go"]),
            "sego: error: Sourcegraph failed to answer the comparison\n"
        );
    }

    #[test]
    fn a_root_commit_has_no_comparison_with_its_parent() {
        let mut body = compared();
        body["data"]["a0"]["commit"] = Value::Null;
        body["data"]["b0"]["commit"]["parents"] = json!([]);
        assert_eq!(
            compare(&body, None, &[]).err().as_deref(),
            Some("sego: error: v2 is a root commit: Sourcegraph has no diff for it\n")
        );
    }

    #[test]
    fn a_path_in_neither_commit_is_named() {
        let mut body = compared();
        body["data"]["b0"]["commit"]["p0"] = Value::Null;
        assert_eq!(
            compare_failure(&body, &["gone.go"]),
            "sego: error: gone.go not found in example.com/acme/widget@0123456789ab or \
             @89abcdef0123\n"
        );
    }

    fn node(n: usize) -> Value {
        json!({
            "oid": format!("{n:040x}"),
            "subject": format!("change {n}"),
            "author": {
                "date": "2026-09-14T10:19:56Z",
                "person": {"displayName": "Ada Example"},
            },
            "parents": [{"oid": format!("{:040x}", n + 1)}],
        })
    }

    fn page_of(count: usize) -> Value {
        let nodes: Vec<Value> = (1..=count).map(node).collect();
        json!({"nodes": nodes, "pageInfo": {"hasNextPage": false}})
    }

    fn with_tags(mut body: Value) -> Value {
        body["data"]["t0"] = root(
            "gitRefs",
            json!({"totalCount": 1001, "nodes": [{"displayName": "v1", "target": {"oid": OID_A}}]}),
        );
        body
    }

    fn rev_body(count: usize) -> Value {
        with_tags(json!({"data": {
            "r0": root("commit", json!({"oid": format!("{:040x}", 1), "ancestors": page_of(count)})),
        }}))
    }

    fn range_body(ahead: Option<u64>, count: usize) -> Value {
        let mut body = json!({"data": {
            "a0": root("commit", json!({"oid": OID_A})),
            "b0": root("commit", json!({"oid": OID_B})),
            "c0": root("comparison", json!({"commits": page_of(count)})),
        }});
        if let Some(ahead) = ahead {
            body["data"]["n0"] = root("commit", json!({"behindAhead": {"ahead": ahead}}));
        }
        with_tags(body)
    }

    fn history(body: &Value, rev: Option<&str>, path: Option<&str>) -> Result<History, String> {
        read_history(&parsed(body), &Repo::Exact(REPO.to_owned()), rev, path)
            .map_err(|e| report(&e))
    }

    fn range(body: &Value, path: Option<&str>) -> Result<RangeHistory, String> {
        read_range(&parsed(body), &Repo::Exact(REPO.to_owned()), ("v1", "v2"), path)
            .map_err(|e| report(&e))
    }

    fn error_line(message: &str) -> String {
        format!("sego: error: {message}\n")
    }

    #[test]
    fn a_history_is_read_with_its_tags() {
        let history = history(&rev_body(2), Some("v1"), None).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(history.commit, format!("{:040x}", 1));
        assert_eq!(history.log.commits.len(), 2);
        assert_eq!(history.log.commits[0].author, "Ada Example");
        assert_eq!(history.log.commits[0].parents, [format!("{:040x}", 2)]);
        assert_eq!(history.log.tags, [("v1".to_owned(), OID_A.to_owned())]);
        assert_eq!(history.log.tags_total, 1001);
        let range = range(&range_body(Some(7), 1), None).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            (range.base.as_str(), range.head.as_str(), range.ahead),
            (OID_A, OID_B, Some(7))
        );
    }

    #[test]
    fn a_range_with_a_missing_side_names_the_revisions_not_found() {
        let both_gone = |a: bool, b: bool| {
            let mut body = range_body(Some(0), 0);
            if !a {
                body["data"]["a0"]["commit"] = Value::Null;
            }
            if !b {
                body["data"]["b0"]["commit"] = Value::Null;
            }
            body["data"]["c0"] = Value::Null;
            body["data"]["n0"]["commit"] = Value::Null;
            let named = if b { "v1" } else { "v2" };
            let mut errors = vec![json!({
                "message": format!("revision not found: repo 1700 @{named}^0"),
                "path": ["c0", "comparison"],
            })];
            if !a && b {
                errors.push(json!({
                    "message": "revision not found",
                    "path": ["n0", "commit", "behindAhead"],
                }));
            }
            body["errors"] = Value::Array(errors);
            range(&body, None).expect_err("the range fails")
        };
        assert_eq!(
            both_gone(false, true),
            error_line(&format!("base revision v1 not found in {REPO}"))
        );
        assert_eq!(
            both_gone(true, false),
            error_line(&format!("head revision v2 not found in {REPO}"))
        );
        assert_eq!(
            both_gone(false, false),
            error_line(&format!("revisions v1 and v2 not found in {REPO}"))
        );
    }

    #[test]
    fn a_missing_revision_is_named_and_a_missing_head_is_named_head() {
        let mut body = rev_body(0);
        body["data"]["r0"]["commit"] = Value::Null;
        assert_eq!(
            history(&body, Some("v9"), None).err(),
            Some(error_line(&format!("revision v9 not found in {REPO}")))
        );
        assert_eq!(
            history(&body, None, None).err(),
            Some(error_line(&format!("revision HEAD not found in {REPO}")))
        );
    }

    #[test]
    fn a_plain_path_with_no_commits_at_a_revision_is_not_found_and_a_glob_is_empty() {
        assert_eq!(
            history(&rev_body(0), Some("v1"), Some("nope.go")).err(),
            Some(error_line(&format!(
                "nope.go not found in {REPO}@{}",
                &format!("{:040x}", 1)[..12]
            )))
        );
        for path in ["*.nope", ":!nope"] {
            let empty =
                history(&rev_body(0), Some("v1"), Some(path)).unwrap_or_else(|e| panic!("{e}"));
            assert!(empty.log.commits.is_empty(), "{path}");
        }
    }

    #[test]
    fn a_plain_path_in_a_range_is_not_found_only_where_it_exists_at_neither_end() {
        let found = |at_a: bool, at_b: bool| {
            let mut body = range_body(None, 0);
            let blob = json!({"__typename": "GitBlob"});
            body["data"]["a0"]["commit"]["x"] = if at_a { blob.clone() } else { Value::Null };
            body["data"]["b0"]["commit"]["x"] = if at_b { blob } else { Value::Null };
            range(&body, Some("nope.go"))
        };
        assert_eq!(
            found(false, false).err(),
            Some(error_line(&format!("nope.go not found in {REPO}@0123456789ab or @89abcdef0123")))
        );
        for (at_a, at_b) in [(true, false), (false, true), (true, true)] {
            assert!(found(at_a, at_b).is_ok(), "{at_a} {at_b}");
        }
    }

    #[test]
    fn a_failed_comparison_beats_a_path_not_found_and_that_beats_a_failed_tag_list() {
        let mut body = range_body(None, 0);
        for alias in ["a0", "b0"] {
            body["data"][alias]["commit"]["x"] = Value::Null;
        }
        let mut tags_gone = body.clone();
        tags_gone["errors"] = json!([{"message": "x", "path": ["t0", "gitRefs"]}]);
        tags_gone["data"]["t0"]["gitRefs"] = Value::Null;
        assert_eq!(
            range(&tags_gone, Some("nope.go")).err(),
            Some(error_line(&format!("nope.go not found in {REPO}@0123456789ab or @89abcdef0123")))
        );
        body["data"]["c0"] = Value::Null;
        body["errors"] = json!([{"message": "x", "path": ["c0", "comparison"]}]);
        assert_eq!(
            range(&body, Some("nope.go")).err(),
            Some(error_line("Sourcegraph failed to answer the comparison"))
        );
    }

    #[test]
    fn a_failed_commit_list_or_revision_lookup_hides_the_server_text() {
        let mut list = rev_body(1);
        list["errors"] = json!([{
            "message": "git command [git log --secret] failed",
            "path": ["r0", "commit", "ancestors", "nodes"],
        }]);
        list["data"]["r0"]["commit"] = Value::Null;
        assert_eq!(
            history(&list, Some("v1"), Some("src")).err(),
            Some(error_line("Sourcegraph failed to answer the commit list"))
        );

        let mut commit = rev_body(1);
        commit["errors"] = json!([{"message": "x", "path": ["r0", "commit"]}]);
        commit["data"]["r0"]["commit"] = Value::Null;
        assert_eq!(
            history(&commit, Some("v1"), None).err(),
            Some(error_line("Sourcegraph failed to answer the revision lookup"))
        );

        for (path, side) in [(json!(["a0", "commit"]), "a0"), (json!(["b0", "commit", "x"]), "b0")]
        {
            let mut body = range_body(None, 1);
            body["errors"] = json!([{"message": "x", "path": path}]);
            if side == "a0" {
                body["data"]["a0"]["commit"] = Value::Null;
            } else {
                body["data"]["b0"]["commit"]["x"] = Value::Null;
            }
            assert_eq!(
                range(&body, Some("a.go")).err(),
                Some(error_line("Sourcegraph failed to answer the revision lookup")),
                "{side}"
            );
        }
    }

    #[test]
    fn a_failed_part_of_a_range_is_named_and_hides_the_server_text() {
        for (name, path, alias, part) in [
            ("c0 root", json!(["c0"]), "c0", "the comparison"),
            ("commits", json!(["c0", "comparison", "commits", "nodes"]), "c0", "the comparison"),
            ("ahead", json!(["n0", "commit", "behindAhead"]), "n0", "the commit count"),
            ("n0 root", json!(["n0"]), "n0", "the commit count"),
            ("tags", json!(["t0", "gitRefs"]), "t0", "the tag list"),
            ("t0 root", json!(["t0"]), "t0", "the tag list"),
        ] {
            let mut body = range_body(Some(1), 1);
            body["errors"] = json!([{"message": "secret internal", "path": path}]);
            if path.as_array().map(Vec::len) == Some(1) {
                body["data"][alias] = Value::Null;
            }
            assert_eq!(
                range(&body, None).err(),
                Some(error_line(&format!("Sourcegraph failed to answer {part}"))),
                "{name}"
            );
        }
    }

    #[test]
    fn a_failed_tag_list_fails_a_revision() {
        let mut body = rev_body(1);
        body["errors"] = json!([{"message": "secret", "path": ["t0", "gitRefs"]}]);
        body["data"]["t0"]["gitRefs"] = Value::Null;
        assert_eq!(
            history(&body, Some("v1"), None).err(),
            Some(error_line("Sourcegraph failed to answer the tag list"))
        );
    }

    #[test]
    fn a_range_without_a_commit_count_fails() {
        let mut body = range_body(None, 1);
        body["data"]["n0"] = Value::Null;
        assert_eq!(
            range(&body, None).err(),
            Some(error_line("Sourcegraph failed to answer the commit count"))
        );
    }
}
