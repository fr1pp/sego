use std::fmt::Write as _;

use serde::Deserialize;
use serde_json::{Value, json};

use super::Page;
use super::graphql::{Envelope, GqlErrorKind, Request, Segment, View, repeated};
use super::resolve::repository_variables;
use super::resolve::{self, Ends, Lookup, Resolved};
use crate::address::{Repo, is_plain_path, pinned, short};
use crate::error::Error;

/// A revision's commit, the changes against its first parent in a wrapper
/// whose error nulls only the wrapper, and whether each path `#` exists in
/// the commit and its parents. The changes' count has its own `first: 0`
/// alias, since `fileDiffs(first: N)` gives a null `totalCount`.
const COMMIT: &str = "\
query Commit($repo0: String!, $repo1: String!, $gitlab: Boolean!, $rev: String!, \
$paths: [String!], $first: Int, $patch: Boolean!{declared}) {
  r0: repository(name: $repo0) { ...Commit }
  r1: repository(name: $repo1) @include(if: $gitlab) { ...Commit }
}

fragment Commit on Repository {
  name
  defaultBranch { displayName }
  mirrorInfo { updatedAt }
  commit(rev: $rev) {
    oid subject message
    author { date person { displayName } }
    committer { date person { displayName } }
    parents { oid{parent_probes} }
{probes}
    w: tree(path: \"\") { commit { diff { ...Changes } } }
  }
}

fragment Changes on RepositoryComparison {
  all: fileDiffs(first: 0, paths: $paths) { totalCount diffStat { added deleted } }
  page: fileDiffs(first: $first, paths: $paths) {
    nodes { oldPath newPath stat { added deleted } oldFile { ... on GitBlob { commit { oid } } } }
    pageInfo { hasNextPage }
  }
  patch: fileDiffs(paths: $paths) @include(if: $patch) { rawDiff }
}
";

/// The tags that hold a commit named by its full ID, which never fetches.
const COMMIT_TAGS: &str = "\
query CommitTags($repo: String!, $commit: String!) {
  repository(name: $repo) { commit(rev: $commit) { tags { nodes { displayName } } } }
}
";

/// The diff of two commits named by their full IDs, which never
/// fetch, with the merge base, and whether each path `#` exists at each.
/// The count has its own `first: 0` alias, as in `COMMIT`.
const DIFF: &str = "\
query Diff($repo: String!, $a: String!, $b: String!, $paths: [String!], $first: Int, \
$patch: Boolean!{declared}) {
  c: repository(name: $repo) {
    comparison(base: $a, head: $b, fetchMissing: false) {
      range { mergeBase { oid } }
      ...Changes
    }
  }
  a: repository(name: $repo) { commit(rev: $a) { oid{probes} } }
  b: repository(name: $repo) { commit(rev: $b) { oid{probes} } }
}

fragment Changes on RepositoryComparison {
  all: fileDiffs(first: 0, paths: $paths) { totalCount diffStat { added deleted } }
  page: fileDiffs(first: $first, paths: $paths) {
    nodes { oldPath newPath stat { added deleted } oldFile { ... on GitBlob { commit { oid } } } }
    pageInfo { hasNextPage }
  }
  patch: fileDiffs(paths: $paths) @include(if: $patch) { rawDiff }
}
";

/// A commit as `show` prints it.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Commit {
    pub subject: String,
    pub message: String,
    pub author: String,
    /// The author's timestamp.
    pub author_date: String,
    pub committer: Option<String>,
    /// The committer's timestamp.
    pub committer_date: Option<String>,
    pub parents: Vec<String>,
    /// The tags that hold the commit, newest created first, when asked for;
    /// the error when they couldn't be read.
    pub tags: Option<Result<Vec<String>, Error>>,
}

/// The files that changed between two commits: a revision and its first
/// parent, or a range's base and head.
#[derive(Debug, PartialEq, Eq)]
pub struct Diff {
    /// The revision, or the range's head.
    pub head: Resolved,
    /// The range's base; `None` for a revision.
    pub base: Option<Resolved>,
    /// The head commit, read for a revision.
    pub commit: Option<Commit>,
    /// The commit the files are compared from: a revision's first parent, or
    /// a range's merge base, else the commit of the first changed file that
    /// existed before.
    pub from: Option<String>,
    /// `None` for a root commit, which Sourcegraph has no diff for.
    pub changes: Option<Changes>,
    /// `None` when no paths were given: only named paths get a patch.
    pub patch: Option<String>,
}

impl Diff {
    /// The lookup of the repository, as either end read it.
    pub fn lookup(&self) -> Lookup {
        self.head.lookup_with(self.base.as_ref())
    }
}

/// The files a diff changed, and their totals.
#[derive(Debug, PartialEq, Eq)]
pub struct Changes {
    pub added: u64,
    pub deleted: u64,
    pub files: Page<Change>,
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

/// The variables and declarations of the plain `paths`, which are probed:
/// a glob or pathspec magic names no single path.
fn probed(paths: &[String], variables: &mut serde_json::Map<String, Value>) -> (usize, String) {
    let checked: Vec<&String> = paths.iter().filter(|p| is_plain_path(p)).collect();
    let mut declared = String::new();
    for (n, path) in checked.iter().enumerate() {
        variables.insert(format!("path{n}"), json!(path));
        let _ = write!(declared, ", $path{n}: String!");
    }
    variables.insert("paths".to_owned(), if paths.is_empty() { Value::Null } else { json!(paths) });
    variables.insert("patch".to_owned(), json!(!paths.is_empty()));
    (checked.len(), declared)
}

/// The commit `rev` names in `repo` and its changes against its first parent
/// over `paths`, listing at most `first` files or all for `None`.
pub fn commit_request(repo: &Repo, rev: &str, paths: &[String], first: Option<u32>) -> Request {
    let mut variables = repository_variables(repo);
    variables.insert("rev".to_owned(), json!(rev));
    variables.insert("first".to_owned(), json!(first));
    let (probes, declared) = probed(paths, &mut variables);
    let document = COMMIT
        .replace("{declared}", &declared)
        .replace(
            "{parent_probes}",
            &repeated(" p#: path(path: $path#) { __typename }", probes).replace('\n', ""),
        )
        .replace("{probes}", &repeated("    p#: path(path: $path#) { __typename }", probes));
    Request::new("Commit", document, variables)
}

/// The diff of a range's resolved ends, by their full IDs, over
/// `paths`, listing at most `first` files or all for `None`.
pub fn diff_request(ends: &Ends, paths: &[String], first: Option<u32>) -> Request {
    let mut variables = ends_variables(ends);
    variables.insert("first".to_owned(), json!(first));
    let (probes, declared) = probed(paths, &mut variables);
    let document = DIFF.replace("{declared}", &declared).replace(
        "{probes}",
        &repeated(" p#: path(path: $path#) { __typename }", probes).replace('\n', ""),
    );
    Request::new("Diff", document, variables)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommitNode {
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
struct ChangesNode {
    all: FileDiffTotals,
    page: FileDiffPage,
    patch: Option<Patch>,
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
#[serde(rename_all = "camelCase")]
struct FileDiffPage {
    nodes: Vec<FileDiff>,
    page_info: PageInfo,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
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

/// A file's counts, read as 0 when missing.
#[derive(Deserialize)]
struct NodeStat {
    added: Option<u64>,
    deleted: Option<u64>,
}

#[derive(Deserialize)]
struct Patch {
    #[serde(rename = "rawDiff")]
    raw_diff: String,
}

impl FileDiff {
    fn change(self) -> Change {
        let (added, deleted) =
            self.stat.map_or((0, 0), |s| (s.added.unwrap_or(0), s.deleted.unwrap_or(0)));
        Change { old_path: self.old_path, new_path: self.new_path, added, deleted }
    }
}

impl ChangesNode {
    /// The commit of the first changed file that existed before.
    fn first_old_commit(&self) -> Option<String> {
        self.page
            .nodes
            .iter()
            .find_map(|node| node.old_file.as_ref()?.commit.as_ref().map(|c| c.oid.clone()))
    }

    fn changes(self) -> (Changes, Option<String>) {
        let total = self.all.total_count;
        let items: Vec<Change> = self.page.nodes.into_iter().map(FileDiff::change).collect();
        let more = self.page.page_info.has_next_page || (items.len() as u64) < total;
        let files = Page { items, total: Some(total), more };
        let changes =
            Changes { added: self.all.diff_stat.added, deleted: self.all.diff_stat.deleted, files };
        (changes, self.patch.map(|patch| patch.raw_diff))
    }
}

/// The first of the plain `paths` that neither commit holds, as the probes
/// `p#` under each say.
fn missing_path<'p>(
    paths: &'p [String],
    (a, b): (Option<&Value>, Option<&Value>),
) -> Option<&'p String> {
    let holds = |commit: Option<&Value>, probe: &str| {
        commit.and_then(|commit| commit.get(probe)).is_some_and(|value| !value.is_null())
    };
    paths.iter().filter(|p| is_plain_path(p)).enumerate().find_map(|(n, path)| {
        let probe = format!("p{n}");
        (!holds(a, &probe) && !holds(b, &probe)).then_some(path)
    })
}

/// Whether a path probe (`p#`, or `x` in a range's history) failed: a
/// probe that errors is a failure, not a missing path, unless the server
/// said the revision isn't there.
fn probe_failed(envelope: &Envelope) -> bool {
    let probe = |key: &str| {
        key == "x"
            || key
                .strip_prefix('p')
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    };
    envelope.errors.iter().any(|e| {
        e.kind() != GqlErrorKind::RevisionNotFound
            && e.path.iter().any(|s| matches!(s, Segment::Key(k) if probe(k)))
    })
}

/// That `path` is in neither commit `a` nor `b`.
fn in_neither(path: &str, repo: &str, a: &str, b: &str) -> String {
    format!("{path} not found in {} or @{}", pinned(repo, a, None), short(b))
}

/// The error for a path in neither commit of a diff.
fn path_not_found(path: &str, repo: &str, a: &str, b: &str) -> Error {
    Error::not_found(in_neither(path, repo, a, b))
}

/// The revision `rev`'s commit and its changes, from the response to
/// `commit_request`.
pub fn read_commit(
    envelope: &Envelope,
    repo: &Repo,
    rev: &str,
    paths: &[String],
) -> Result<Diff, Error> {
    let found = resolve::repository(repo, envelope, resolve::roots(repo))?;
    let root = found.root.clone();
    let oid = resolve::commit(envelope, &found, &[&root, "commit"], rev)?;
    let at = [root.clone(), "commit".to_owned()];
    let view = View::new(envelope, &at);
    let node: CommitNode = view.read(&[]).ok_or_else(|| Error::failed("the revision lookup"))?;
    let (committer, committer_date) = match node.committer {
        Some(c) => (c.person.and_then(|p| p.display_name), c.date),
        None => (None, None),
    };
    let parents: Vec<String> = node.parents.into_iter().map(|p| p.oid).collect();
    let head = Resolved::new(&found, oid, Some(rev.to_owned()), None);
    let commit = Commit {
        subject: node.subject,
        message: node.message,
        author: node.author.person.display_name,
        author_date: node.author.date,
        committer,
        committer_date,
        parents,
        tags: None,
    };
    let Some(parent) = commit.parents.first().cloned() else {
        return Ok(Diff {
            head,
            base: None,
            commit: Some(commit),
            from: None,
            changes: None,
            patch: None,
        });
    };
    let at_head = envelope.get(&[&at[0], "commit"]);
    let at_parent = envelope.get(&[&at[0], "commit", "parents"]).and_then(|p| p.get(0));
    if probe_failed(envelope) {
        return Err(Error::failed("the path lookup"));
    }
    if let Some(path) = missing_path(paths, (at_parent, at_head)) {
        return Err(path_not_found(path, &head.repo, &parent, &head.commit));
    }
    let failed = || Error::failed("the file list");
    if view.errored_beneath(&["w"]) {
        return Err(failed());
    }
    let node: ChangesNode = view.read(&["w", "commit", "diff"]).ok_or_else(failed)?;
    let (changes, patch) = node.changes();
    if !paths.is_empty() && patch.is_none() {
        return Err(failed());
    }
    Ok(Diff {
        head,
        base: None,
        commit: Some(commit),
        from: Some(parent),
        changes: Some(changes),
        patch,
    })
}

/// The diff of a range's ends, from the response to
/// `diff_request`.
pub fn read_diff(
    envelope: &Envelope,
    Ends { base, head }: Ends,
    paths: &[String],
) -> Result<Diff, Error> {
    let (at_a, at_b) = (envelope.get(&["a", "commit"]), envelope.get(&["b", "commit"]));
    if at_a.is_none() || at_b.is_none() {
        return Err(Error::failed("the comparison"));
    }
    if probe_failed(envelope) {
        return Err(Error::failed("the path lookup"));
    }
    if let Some(path) = missing_path(paths, (at_a, at_b)) {
        return Err(path_not_found(path, &head.repo, &base.commit, &head.commit));
    }
    let failed = || Error::failed("the comparison");
    if envelope.error_beneath(&["c"]).is_some() {
        return Err(failed());
    }
    let node: ChangesNode = envelope
        .get(&["c", "comparison"])
        .and_then(|value| ChangesNode::deserialize(value).ok())
        .ok_or_else(failed)?;
    let merge_base = envelope
        .get(&["c", "comparison", "range", "mergeBase", "oid"])
        .and_then(Value::as_str)
        .map(str::to_owned);
    let from = merge_base.or_else(|| node.first_old_commit());
    let (changes, patch) = node.changes();
    if !paths.is_empty() && patch.is_none() {
        return Err(failed());
    }
    Ok(Diff { head, base: Some(base), commit: None, from, changes: Some(changes), patch })
}

/// The tags that hold the resolved commit `at`.
pub fn commit_tags_request(at: &Resolved) -> Request {
    let mut variables = serde_json::Map::new();
    variables.insert("repo".to_owned(), json!(at.repo));
    variables.insert("commit".to_owned(), json!(at.commit));
    Request::new("CommitTags", COMMIT_TAGS.to_owned(), variables)
}

#[derive(Deserialize)]
struct Holding {
    nodes: Vec<HoldingTag>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HoldingTag {
    display_name: Option<String>,
}

/// The tags that hold the commit, from the response to
/// `commit_tags_request`.
pub fn read_commit_tags(envelope: &Envelope) -> Result<Vec<String>, Error> {
    let failed = || Error::failed("the tag list");
    if envelope.error_beneath(&["repository"]).is_some() {
        return Err(failed());
    }
    let list = envelope
        .get(&["repository", "commit", "tags"])
        .and_then(|value| Holding::deserialize(value).ok())
        .ok_or_else(failed)?;
    Ok(list.nodes.into_iter().filter_map(|node| node.display_name).collect())
}

/// The roots of `HISTORY`'s tag lists, one per candidate of the address.
const TAG_ROOTS: [&str; 2] = ["t0", "t1"];

/// A revision's history, its first `$first` commits, in a wrapper whose
/// error nulls only the wrapper, and every tag under its own root.
const HISTORY: &str = "\
query Log($repo0: String!, $repo1: String!, $gitlab: Boolean!, $rev: String!, $path: String, \
$follow: Boolean!, $first: Int) {
  r0: repository(name: $repo0) { ...History }
  t0: repository(name: $repo0) { ...Tags }
  r1: repository(name: $repo1) @include(if: $gitlab) { ...History }
  t1: repository(name: $repo1) @include(if: $gitlab) { ...Tags }
}

fragment History on Repository {
  name
  defaultBranch { displayName }
  mirrorInfo { updatedAt }
  commit(rev: $rev) {
    oid
    w: tree(path: \"\") { commit { ancestors(first: $first, path: $path, follow: $follow) { ...Page } } }
  }
}

fragment Page on GitCommitConnection {
  nodes { oid subject author { date person { displayName } } parents { oid } }
  pageInfo { hasNextPage }
}

fragment Tags on Repository {
  gitRefs(type: GIT_TAG) { nodes { displayName target { oid } } }
}
";

/// The commits in a range's head and not in its base, by their full IDs,
/// with how far each is ahead of the other when `$count`, whether the path
/// exists at each end when `$probe`, and every tag.
const LOG_RANGE: &str = "\
query LogRange($repo: String!, $a: String!, $b: String!, $path: String, $first: Int, \
$count: Boolean!, $probe: Boolean!) {
  c: repository(name: $repo) {
    comparison(base: $a, head: $b, fetchMissing: false) { commits(first: $first, path: $path) { ...Page } }
  }
  n: repository(name: $repo) {
    commit(rev: $b) @include(if: $count) { behindAhead(revspec: $a) { ahead behind } }
  }
  a: repository(name: $repo) { commit(rev: $a) { oid x: path(path: $path) @include(if: $probe) { __typename } } }
  b: repository(name: $repo) { commit(rev: $b) { oid x: path(path: $path) @include(if: $probe) { __typename } } }
  t: repository(name: $repo) { gitRefs(type: GIT_TAG) { nodes { displayName target { oid } } } }
}

fragment Page on GitCommitConnection {
  nodes { oid subject author { date person { displayName } } parents { oid } }
  pageInfo { hasNextPage }
}
";

/// The commits of a revision, or of a range's head and not its base.
#[derive(Debug, PartialEq, Eq)]
pub struct History {
    /// The revision, or the range's head.
    pub head: Resolved,
    /// The range's base; `None` for a revision.
    pub base: Option<Resolved>,
    pub commits: Page<CommitSummary>,
    /// Every tag, with the commit it names.
    pub tags: Result<Vec<Tag>, Error>,
    /// How many commits the base has that the head doesn't, for a range with
    /// no path.
    pub behind: Option<u64>,
}

impl History {
    /// The lookup of the repository, as either end read it.
    pub fn lookup(&self) -> Lookup {
        self.head.lookup_with(self.base.as_ref())
    }
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

#[derive(Debug, PartialEq, Eq)]
pub struct Tag {
    pub name: String,
    pub commit: String,
}

/// The history of `rev`, or the default branch for `None`, touching `path`,
/// across renames when `follow`: the first `first` commits.
pub fn history_request(
    repo: &Repo,
    rev: Option<&str>,
    path: Option<&str>,
    follow: bool,
    first: Option<u32>,
) -> Request {
    let mut variables = repository_variables(repo);
    variables.insert("rev".to_owned(), json!(rev.unwrap_or("HEAD")));
    variables.insert("path".to_owned(), json!(path));
    variables.insert("follow".to_owned(), json!(follow));
    variables.insert("first".to_owned(), json!(first));
    Request::new("Log", HISTORY.to_owned(), variables)
}

/// The commits in a range's head and not its base, both resolved, by their
/// full IDs, touching `path`: the first `first`, or all for `None`.
pub fn range_history_request(ends: &Ends, path: Option<&str>, first: Option<u32>) -> Request {
    let mut variables = ends_variables(ends);
    variables.insert("path".to_owned(), json!(path));
    variables.insert("first".to_owned(), json!(first));
    variables.insert("count".to_owned(), json!(path.is_none()));
    variables.insert("probe".to_owned(), json!(path.is_some_and(is_plain_path)));
    Request::new("LogRange", LOG_RANGE.to_owned(), variables)
}

/// `$repo`, `$a` and `$b`: a range's repository and its ends' full IDs.
fn ends_variables(ends: &Ends) -> serde_json::Map<String, Value> {
    let mut variables = serde_json::Map::new();
    variables.insert("repo".to_owned(), json!(ends.head.repo));
    variables.insert("a".to_owned(), json!(ends.base.commit));
    variables.insert("b".to_owned(), json!(ends.head.commit));
    variables
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommitPage {
    nodes: Vec<SummaryNode>,
    page_info: PageInfo,
}

#[derive(Deserialize)]
struct SummaryNode {
    oid: String,
    subject: String,
    author: Author,
    parents: Vec<Oid>,
}

#[derive(Deserialize)]
struct TagList {
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

/// The commits of the page at `at`, and whether more follow.
fn page(envelope: &Envelope, at: &[&str], total: Option<u64>) -> Option<Page<CommitSummary>> {
    let page = CommitPage::deserialize(envelope.get(at)?).ok()?;
    let items = page
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
    Some(Page { items, total, more: page.page_info.has_next_page })
}

/// Every tag listed at `root`, or the failure that leaves log's tags
/// unmarked.
fn tags(envelope: &Envelope, root: &str) -> Result<Vec<Tag>, Error> {
    let failed = || Error::failed("the tag list");
    if envelope.error_beneath(&[root]).is_some() {
        return Err(failed());
    }
    let list = envelope
        .get(&[root, "gitRefs"])
        .and_then(|value| TagList::deserialize(value).ok())
        .ok_or_else(failed)?;
    Ok(list
        .nodes
        .into_iter()
        .filter_map(|node| Some(Tag { name: node.display_name?, commit: node.target?.oid? }))
        .collect())
}

/// The history of `rev`, from the response to `history_request`.
pub fn read_history(
    envelope: &Envelope,
    repo: &Repo,
    rev: Option<&str>,
    path: Option<&str>,
) -> Result<History, Error> {
    let found = resolve::repository(repo, envelope, resolve::roots(repo))?;
    let root = found.root.clone();
    let oid = resolve::commit(envelope, &found, &[&root, "commit"], rev.unwrap_or("HEAD"))?;
    let list_failed = || Error::failed("the commit list");
    if envelope.error_beneath(&[&root, "commit", "w"]).is_some() {
        return Err(list_failed());
    }
    let commits = page(envelope, &[&root, "commit", "w", "commit", "ancestors"], None)
        .ok_or_else(list_failed)?;
    let head = Resolved::new(&found, oid, rev.map(str::to_owned), None);
    if let Some(path) = path.filter(|p| is_plain_path(p))
        && commits.items.is_empty()
    {
        let at = pinned(&head.repo, &head.commit, None);
        return Err(Error::not_found(format!("{path} not found in {at}")));
    }
    let tags = tags(envelope, found.root_among(TAG_ROOTS));
    Ok(History { head, base: None, commits, tags, behind: None })
}

/// The commits in a range, from the response to `range_history_request`.
pub fn read_range_history(
    envelope: &Envelope,
    Ends { base, head }: Ends,
    path: Option<&str>,
) -> Result<History, Error> {
    let comparison_failed = || Error::failed("the comparison");
    let listed = envelope.error_beneath(&["c"]).is_none();
    let commits = listed.then(|| page(envelope, &["c", "comparison", "commits"], None)).flatten();
    if probe_failed(envelope) {
        return Err(Error::failed("the path lookup"));
    }
    if let Some(path) = path.filter(|p| is_plain_path(p))
        && envelope.get(&["a", "commit", "x"]).is_none()
        && envelope.get(&["b", "commit", "x"]).is_none()
        && commits.as_ref().is_some_and(|commits| commits.items.is_empty())
    {
        return Err(path_not_found(path, &head.repo, &base.commit, &head.commit));
    }
    let mut commits = commits.ok_or_else(comparison_failed)?;
    let mut behind = None;
    if path.is_none() {
        let count_failed = || Error::failed("the commit count");
        if envelope.error_beneath(&["n"]).is_some() {
            return Err(count_failed());
        }
        let counts = envelope.get(&["n", "commit", "behindAhead"]).ok_or_else(count_failed)?;
        let count = |key: &str| counts.get(key).and_then(Value::as_u64).ok_or_else(count_failed);
        commits.total = Some(count("ahead")?);
        behind = Some(count("behind")?);
    }
    let tags = tags(envelope, "t");
    Ok(History { head, base: Some(base), commits, tags, behind })
}
