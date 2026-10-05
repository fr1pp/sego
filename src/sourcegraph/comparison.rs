//! Comparing two revisions, or one commit with its first parent: the
//! commits between them, the files changed and, for a file, its hunks.

use serde::Deserialize;

use crate::address::{CommitId, Pinned, Range, RepoName, RepoPath};
use crate::diagnostic::Diagnostic;
use crate::domain::{
    CommitCount, CommitDetail, CommitSummary, Comparison, DiffHunk,
    FileChange, FileStatus, Side, Unavailable,
};

use super::envelope::Envelope;
use super::node::{self, Node};
use super::{FetchError, Plan};

/// The most commits a comparison lists.
pub const COMMIT_CAP: u32 = 50;

/// The most files a comparison lists.
pub const FILE_CAP: u32 = 200;

/// A path's type and metadata at a commit, as `node::Node` takes it, in a
/// commit or, indented further, in its `parents`. `path()` takes the path
/// itself, not a pathspec.
macro_rules! node {
    () => {
        "
      node: path(path: $plainPath) {
        __typename
        ... on GitBlob {
          path byteSize binary contentType submodule { url commit }
        }
        ... on GitTree { path }
      }"
    };
    (parents) => {
        "
        node: path(path: $plainPath) {
          __typename
          ... on GitBlob {
            path byteSize binary contentType submodule { url commit }
          }
          ... on GitTree { path }
        }"
    };
}

/// The hunks of the first file: with a file's path, the file's.
macro_rules! hunks {
    () => {
        "
      hunks: fileDiffs(first: 1, paths: $paths) {
        nodes {
          hunks {
            oldRange { startLine lines } newRange { startLine lines }
            oldNoNewlineAt section body
          }
        }
      }"
    };
}

/// The totals, the list without hunks, and `$hunks`. Any unfiltered
/// `fileDiffs` field costs the whole patch, whatever its `first`, so every
/// one carries `$paths`.
macro_rules! files {
    ($hunks:expr) => {
        concat!(
            "
      totals: fileDiffs(first: 0, paths: $paths) {
        totalCount diffStat { added deleted }
      }
      fileDiffs(first: $files, paths: $paths) {
        nodes {
          oldPath newPath stat { added deleted }
          oldFile { binary } newFile { binary }
        }
      }",
            $hunks
        )
    };
}

/// A range. `refs` resolves each side alone, so a missing one can be named,
/// and counts the commits each way; `behindAhead` has an alias of its own,
/// as its error for an unknown base nulls its commit. `cmp` is the
/// comparison, which an unknown side nulls whole.
macro_rules! range {
    ($variables:expr, $node:expr, $hunks:expr) => {
        concat!(
            "query CompareRange(
  $repo: String!, $base: String!, $head: String!, $path: String,
  $paths: [String!], $commits: Int!, $files: Int!",
            $variables,
            "
) {
  refs: repository(name: $repo) {
    name
    base: commit(rev: $base) {
      oid",
            $node,
            "
    }
    head: commit(rev: $head) {
      oid",
            $node,
            "
    }
    count: commit(rev: $head) {
      behindAhead(revspec: $base) { behind ahead }
    }
  }
  cmp: repository(name: $repo) {
    comparison(base: $base, head: $head) {
      range {
        base { ... on GitRevSpecExpr { object { oid } } }
        head { ... on GitRevSpecExpr { object { oid } } }
      }
      commits(first: $commits, path: $path) {
        pageInfo { hasNextPage }
        nodes { oid subject author { person { name } date } }
      }",
            files!($hunks),
            "
    }
  }
}
"
        )
    };
}

/// A single commit. `commit` has its message and parents; `diff`, which a
/// root commit's error nulls whole, is apart.
macro_rules! commit {
    ($variables:expr, $node:expr, $parent:expr, $hunks:expr) => {
        concat!(
            "query CompareCommit(
  $repo: String!, $rev: String!, $paths: [String!], $files: Int!",
            $variables,
            "
) {
  commit: repository(name: $repo) {
    name
    commit(rev: $rev) {
      oid message author { person { name } date }",
            $node,
            "
      parents {
        oid",
            $parent,
            "
      }
    }
  }
  diff: repository(name: $repo) {
    commit(rev: $rev) { diff {",
            files!($hunks),
            "
    } }
  }
}
"
        )
    };
}

const RANGE: &str = range!("", "", "");
const RANGE_AT_PATH: &str = range!(", $plainPath: String!", node!(), hunks!());
const COMMIT: &str = commit!("", "", "", "");
const COMMIT_AT_PATH: &str =
    commit!(", $plainPath: String!", node!(), node!(parents), hunks!());

/// What changed in `range`, narrowed to `path` if there is one.
#[derive(Debug, Clone)]
pub struct CompareRequest {
    pub range: Range,
    pub path: Option<RepoPath>,
}

impl CompareRequest {
    /// The path, unless it is the root, which narrows nothing.
    fn path(&self) -> Option<&RepoPath> {
        self.path.as_ref().filter(|path| !path.is_root())
    }
}

/// A path is sent as a literal pathspec, so its `*` and `?` match only
/// themselves, and `path()` gets it as it is.
pub fn plan(request: &CompareRequest) -> Plan {
    let Range { repo, base, head } = &request.range;
    let path = request.path();
    let spec = path.map(|path| format!(":(literal){}", path.upstream()));
    let mut variables = serde_json::json!({
        "repo": repo.as_str(),
        "paths": spec.as_ref().map(|spec| [spec]),
        "files": FILE_CAP,
    });
    if let Some(path) = path {
        variables["plainPath"] = path.upstream().into();
    }
    let narrowed = path.is_some();
    let query = if let Some(base) = base {
        variables["base"] = base.as_str().into();
        variables["head"] = head.as_str().into();
        variables["path"] = spec.into();
        variables["commits"] = COMMIT_CAP.into();
        if narrowed { RANGE_AT_PATH } else { RANGE }
    } else {
        variables["rev"] = head.as_str().into();
        if narrowed { COMMIT_AT_PATH } else { COMMIT }
    };
    Plan { query, variables }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RangeData {
    refs: Option<Refs>,
    cmp: Option<Cmp>,
}

/// `base` and `head` are null for a revision that doesn't resolve; `count`
/// is null with them, or with its own error.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Refs {
    name: String,
    base: Option<Ref>,
    head: Option<Ref>,
    count: Option<Count>,
}

/// `node` is absent without a path, and null where the path isn't.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ref {
    #[serde(rename = "oid")]
    _oid: String,
    node: Option<Node>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Count {
    behind_ahead: BehindAhead,
}

/// `ahead` counts the commits on head and not on base; `behind`, those on
/// base and not on head.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BehindAhead {
    behind: u32,
    ahead: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cmp {
    comparison: WireComparison,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireComparison {
    range: WireRange,
    commits: Commits,
    #[serde(flatten)]
    diff: Diff,
}

/// Each side is a `GitRevSpecExpr` whose object is the commit, peeled; any
/// other member of the union is `{}`, with no object.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRange {
    base: RevSpec,
    head: RevSpec,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevSpec {
    object: Option<Object>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Object {
    oid: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Commits {
    page_info: PageInfo,
    nodes: Vec<WireCommit>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
}

/// `subject` is the message's first line.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireCommit {
    oid: String,
    subject: String,
    author: Signature,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Signature {
    person: Person,
    date: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Person {
    name: String,
}

/// The three `fileDiffs` aliases; `hunks` is absent without a path.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Diff {
    totals: Totals,
    file_diffs: FileDiffs,
    hunks: Option<HunkDiffs>,
}

/// `totalCount` is null only for a partial page, which `first: 0` isn't.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Totals {
    total_count: Option<u32>,
    diff_stat: Stat,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Stat {
    added: u32,
    deleted: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileDiffs {
    nodes: Vec<FileDiff>,
}

/// No `oldPath` is an addition, no `newPath` a deletion. A rename's `stat`
/// counts its content changes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct FileDiff {
    old_path: Option<String>,
    new_path: Option<String>,
    stat: Stat,
    old_file: Option<File>,
    new_file: Option<File>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    binary: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HunkDiffs {
    nodes: Vec<HunkDiff>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HunkDiff {
    hunks: Vec<WireHunk>,
}

/// `body` is the hunk's lines, each ending `\n` but a last line without a
/// newline on the new side; the old side's has `oldNoNewlineAt` instead.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WireHunk {
    old_range: Lines,
    new_range: Lines,
    old_no_newline_at: bool,
    section: Option<String>,
    body: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Lines {
    start_line: u32,
    lines: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommitData {
    commit: Option<CommitRepository>,
    diff: Option<DiffRepository>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommitRepository {
    name: String,
    commit: Option<WireDetail>,
}

/// `message` is the whole message, without its final newline.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireDetail {
    oid: String,
    message: String,
    author: Signature,
    node: Option<Node>,
    parents: Vec<Parent>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Parent {
    oid: String,
    node: Option<Node>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DiffRepository {
    commit: Option<DiffCommit>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DiffCommit {
    diff: Diff,
}

/// Removes every error at `prefix` or below it from `envelope`, and says
/// whether there was one.
fn take_errors(envelope: &mut Envelope, prefix: &[&str]) -> bool {
    let before = envelope.errors.len();
    envelope.errors.retain(|error| {
        !error.path.as_deref().is_some_and(|path| {
            path.len() >= prefix.len()
                && path
                    .iter()
                    .zip(prefix)
                    .all(|(segment, name)| segment.as_str() == Some(name))
        })
    });
    envelope.errors.len() < before
}

/// An error the upstream's text explains only with internal ids and
/// command lines.
fn unexplained(cause: &str, trace: Option<String>) -> Box<dyn Diagnostic> {
    let errors = vec![cause.to_owned()];
    FetchError::Server { errors, trace }.into()
}

/// The comparison `envelope` answers `request` with, or why there is none.
/// Errors under the comparison are classified from the other aliases, never
/// from their text, which leaks internal ids and git command lines.
pub fn answer(
    request: &CompareRequest,
    envelope: Envelope,
) -> Result<Comparison, Box<dyn Diagnostic>> {
    match &request.range.base {
        Some(base) => compare_range(request, base.as_str(), envelope),
        None => compare_commit(request, envelope),
    }
}

fn compare_range(
    request: &CompareRequest,
    base_rev: &str,
    mut envelope: Envelope,
) -> Result<Comparison, Box<dyn Diagnostic>> {
    let compared = take_errors(&mut envelope, &["cmp"]);
    let counted = take_errors(&mut envelope, &["refs", "count"]);
    let trace = envelope.trace.clone();
    let drift = |cause: &str| FetchError::Field {
        cause: cause.to_owned(),
        trace: trace.clone(),
    };
    let RangeData { refs, cmp } = envelope.into_data()?;
    let Some(Refs { name, base, head, count }) = refs else {
        let repo = request.range.repo.as_str().to_owned();
        return Err(Unavailable::RepoNotFound { repo }.into());
    };
    let head_rev = request.range.head.as_str();
    let missing = |rev: String, side| -> Box<dyn Diagnostic> {
        let (repo, side) = (name.clone(), Some(side));
        Unavailable::RevisionNotFound { repo, rev, side }.into()
    };
    let (base, head) = match (base, head) {
        (Some(base), Some(head)) => (base, head),
        (None, None) => {
            let rev = format!("{base_rev}...{head_rev}");
            return Err(missing(rev, Side::Both));
        }
        (None, Some(_)) => return Err(missing(base_rev.into(), Side::Base)),
        (Some(_), None) => return Err(missing(head_rev.into(), Side::Head)),
    };
    if compared || counted {
        let cause = "the upstream couldn't compare these revisions";
        return Err(unexplained(cause, trace));
    }
    let repo =
        RepoName::parse(&name).map_err(|error| drift(&error.to_string()))?;
    let BehindAhead { behind, ahead } = count
        .ok_or_else(|| drift("the head has no behindAhead"))?
        .behind_ahead;
    let WireComparison { range: pins, commits, diff } = cmp
        .ok_or_else(|| drift("the comparison is null with no error"))?
        .comparison;
    let pin = |side: RevSpec, which: &str| {
        side.object.map(|object| CommitId(object.oid)).ok_or_else(|| {
            drift(&format!("the range's {which} has no object"))
        })
    };
    let base_commit = pin(pins.base, "base")?;
    let head_commit = pin(pins.head, "head")?;
    let narrowed = request.path();
    let path_is_file =
        classify(&repo, &head_commit, narrowed, head.node, base.node)?;
    let commit_count = match narrowed {
        None => CommitCount::Exact(ahead),
        Some(_) if commits.page_info.has_next_page => {
            CommitCount::MoreThan(COMMIT_CAP)
        }
        Some(_) => CommitCount::Exact(listed(commits.nodes.len())),
    };
    let commits = commits
        .nodes
        .into_iter()
        .map(|WireCommit { oid, subject, author }| CommitSummary {
            commit: CommitId(oid),
            date: author.date,
            author: author.person.name,
            subject,
        })
        .collect();
    // The diff is from the merge base, which is the base when the base has
    // nothing the head lacks.
    let deleted_at = (behind == 0).then(|| base_commit.clone());
    let changes =
        changes(diff, &head_commit, deleted_at.as_ref(), path_is_file)
            .map_err(|cause| drift(&cause))?;
    Ok(Comparison {
        repo,
        base: Some(base_commit),
        head: head_commit,
        commit: None,
        ahead,
        behind,
        commits,
        commit_count,
        files_changed: changes.total,
        added: changes.added,
        deleted: changes.deleted,
        files_truncated: listed(changes.files.len()) < changes.total,
        files: changes.files,
        path: narrowed.map(|path| path.upstream().to_owned()),
        path_is_file,
        hunks: changes.hunks,
    })
}

fn compare_commit(
    request: &CompareRequest,
    mut envelope: Envelope,
) -> Result<Comparison, Box<dyn Diagnostic>> {
    let failed = take_errors(&mut envelope, &["diff"]);
    let trace = envelope.trace.clone();
    let drift = |cause: &str| FetchError::Field {
        cause: cause.to_owned(),
        trace: trace.clone(),
    };
    let CommitData { commit, diff } = envelope.into_data()?;
    let Some(CommitRepository { name, commit }) = commit else {
        let repo = request.range.repo.as_str().to_owned();
        return Err(Unavailable::RepoNotFound { repo }.into());
    };
    let Some(WireDetail { oid, message, author, node, parents }) = commit
    else {
        let rev = request.range.head.as_str().to_owned();
        return Err(Unavailable::RevisionNotFound {
            repo: name,
            rev,
            side: None,
        }
        .into());
    };
    let repo =
        RepoName::parse(&name).map_err(|error| drift(&error.to_string()))?;
    let head = CommitId(oid);
    let mut parents = parents.into_iter();
    let Some(Parent { oid: first, node: first_node }) = parents.next() else {
        let at = Pinned { repo, commit: head };
        return Err(Unavailable::NoParent { at }.into());
    };
    if failed {
        let cause = "the upstream couldn't diff this commit";
        return Err(unexplained(cause, trace));
    }
    let diff = diff
        .and_then(|diff| diff.commit)
        .ok_or_else(|| drift("the commit's diff is null with no error"))?
        .diff;
    let narrowed = request.path();
    let path_is_file = classify(&repo, &head, narrowed, node, first_node)?;
    let parents: Vec<CommitId> = std::iter::once(first)
        .chain(parents.map(|parent| parent.oid))
        .map(CommitId)
        .collect();
    // A single commit's diff is from its first parent, which holds a file
    // the commit deletes.
    let changes = changes(diff, &head, parents.first(), path_is_file)
        .map_err(|cause| drift(&cause))?;
    Ok(Comparison {
        repo,
        base: None,
        head,
        commit: Some(CommitDetail {
            date: author.date,
            author: author.person.name,
            message: message.replace("\r\n", "\n"),
            parents,
        }),
        ahead: 1,
        behind: 0,
        commits: Vec::new(),
        commit_count: CommitCount::Exact(1),
        files_changed: changes.total,
        added: changes.added,
        deleted: changes.deleted,
        files_truncated: listed(changes.files.len()) < changes.total,
        files: changes.files,
        path: narrowed.map(|path| path.upstream().to_owned()),
        path_is_file,
        hunks: changes.hunks,
    })
}

/// Whether `path` is a file, from its node at head, or at base if it isn't
/// at head; or why there is no answer: it is at neither.
fn classify(
    repo: &RepoName,
    head: &CommitId,
    path: Option<&RepoPath>,
    at_head: Option<Node>,
    at_base: Option<Node>,
) -> Result<bool, Unavailable> {
    let Some(path) = path else {
        return Ok(false);
    };
    let at = Pinned { repo: repo.clone(), commit: head.clone() };
    match at_head.or(at_base) {
        None => {
            let path = path.upstream().to_owned();
            Err(Unavailable::PathNotFound { at, path })
        }
        node @ Some(_) => Ok(node::blob(&at, path, node).is_ok()),
    }
}

/// A list's length as a count; no list is near `u32::MAX` long.
pub fn listed(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// A diff's totals, its listed files and, for a file, its hunks.
struct Changes {
    total: u32,
    added: u32,
    deleted: u32,
    files: Vec<FileChange>,
    hunks: Vec<DiffHunk>,
}

/// `diff` as files whose new content is at `head`, and whose old content,
/// for a deletion, is at `deleted_at` if the diff's base is known to hold
/// it; or why the diff can't be one.
fn changes(
    diff: Diff,
    head: &CommitId,
    deleted_at: Option<&CommitId>,
    path_is_file: bool,
) -> Result<Changes, String> {
    let Diff { totals, file_diffs, hunks } = diff;
    let total = totals.total_count.ok_or("the totals have no totalCount")?;
    let mut files = Vec::with_capacity(file_diffs.nodes.len());
    for FileDiff { old_path, new_path, stat, old_file, new_file } in
        file_diffs.nodes
    {
        let (path, old_path, status, readable_at) = match (old_path, new_path)
        {
            (None, Some(new)) => {
                (new, None, FileStatus::Added, Some(head.clone()))
            }
            (Some(old), None) => {
                (old, None, FileStatus::Deleted, deleted_at.cloned())
            }
            (Some(old), Some(new)) if old == new => {
                (new, None, FileStatus::Modified, Some(head.clone()))
            }
            (Some(old), Some(new)) => {
                (new, Some(old), FileStatus::Renamed, Some(head.clone()))
            }
            (None, None) => return Err("a file diff has no path".into()),
        };
        let binary =
            [old_file, new_file].into_iter().flatten().any(|file| file.binary);
        files.push(FileChange {
            path,
            old_path,
            status,
            binary,
            added: stat.added,
            deleted: stat.deleted,
            readable_at,
        });
    }
    if listed(files.len()) > total {
        return Err(format!(
            "the diff lists {} files of {total}",
            files.len()
        ));
    }
    let hunks = match hunks {
        Some(HunkDiffs { nodes }) if path_is_file => nodes
            .into_iter()
            .next()
            .map_or_else(|| Ok(Vec::new()), |node| diff_hunks(node.hunks))?,
        Some(_) | None => Vec::new(),
    };
    Ok(Changes {
        total,
        added: totals.diff_stat.added,
        deleted: totals.diff_stat.deleted,
        files,
        hunks,
    })
}

/// `wire` with each body split into its lines; or why a body doesn't match
/// its ranges.
fn diff_hunks(wire: Vec<WireHunk>) -> Result<Vec<DiffHunk>, String> {
    let mut hunks = Vec::with_capacity(wire.len());
    for WireHunk { old_range, new_range, old_no_newline_at, section, body } in
        wire
    {
        // Split at `\n` alone: a line's `\r` is the file's.
        let lines: Vec<String> = body
            .split_inclusive('\n')
            .map(|line| line.strip_suffix('\n').unwrap_or(line).to_owned())
            .collect();
        let (mut old, mut new) = (0, 0);
        for line in &lines {
            match line.as_bytes().first() {
                Some(b' ') => (old, new) = (old + 1, new + 1),
                Some(b'-') => old += 1,
                Some(b'+') => new += 1,
                _ => return Err("a hunk line has no prefix".to_owned()),
            }
        }
        if (old, new) != (old_range.lines, new_range.lines) {
            return Err(format!(
                "a hunk of {} and {} lines has {old} and {new}",
                old_range.lines, new_range.lines
            ));
        }
        hunks.push(DiffHunk {
            old_start: old_range.start_line,
            old_lines: old_range.lines,
            new_start: new_range.start_line,
            new_lines: new_range.lines,
            section,
            lines,
            no_newline_old: old_no_newline_at,
            no_newline_new: !body.is_empty() && !body.ends_with('\n'),
        });
    }
    Ok(hunks)
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use expect_test::expect;

    use super::*;
    use crate::diagnostic::report;
    use crate::sourcegraph::testing::{self, decoded, failure};

    /// The capture cases, by name: each is `tests/compare/<name>.json`.
    fn cases() -> Vec<(&'static str, CompareRequest)> {
        let compare = |range: &str, path: Option<&str>| CompareRequest {
            range: Range::parse(range).unwrap(),
            path: path.map(RepoPath::parse),
        };
        let mux = |rev: &str, path| {
            compare(&format!("github.com/gorilla/mux@{rev}"), path)
        };
        let go = |rev: &str, path| {
            compare(&format!("github.com/golang/go@{rev}"), path)
        };
        let release = "v1.7.0...v1.8.0";
        vec![
            ("range", mux(release, None)),
            ("directory", mux(release, Some(".github"))),
            ("file", mux(release, Some("mux.go"))),
            ("file-added", mux(release, Some(".circleci/config.yml"))),
            ("file-deleted", mux(release, Some("context.go"))),
            ("unchanged-path", mux(release, Some("LICENSE"))),
            ("missing-path", mux(release, Some("nope.go"))),
            ("renamed", mux("v1.6.0...v1.7.0", None)),
            ("renamed-file", mux("v1.6.0...v1.7.0", Some("context.go"))),
            (
                "divergent",
                mux(
                    "f395758b854c4efa789b8c1e9b73479704c548cb...\
                     946b6237eb8d0ce3225f502b7fd4208d0b60ce5f",
                    None,
                ),
            ),
            ("reversed", mux("v1.8.0...v1.7.0", None)),
            ("equal", mux("v1.8.0...v1.8.0", None)),
            ("missing-base", mux("v9.9.9...v1.8.0", None)),
            ("missing-head", mux("v1.7.0...nope-branch", None)),
            (
                "missing-repo",
                compare(
                    "github.com/gorilla/doesnotexist@v1.7.0...v1.8.0",
                    None,
                ),
            ),
            ("short-revs", mux("a7962380...98cb6bf4", Some(".github"))),
            ("commit", mux("98cb6bf42e08", None)),
            ("commit-file", mux("98cb6bf42e08", Some("regexp.go"))),
            ("merge", mux("2854a05fd3e2", None)),
            ("root", mux("4e8d10dc715f", None)),
            ("multi-line-subject", mux("e48e440e4c92", None)),
            ("crlf-message", mux("ed099d423848", None)),
            ("no-newline-old", go("48469a2c866c", Some("src/cmd/ld/lib.h"))),
            ("no-newline-new", mux("1fa2ee87bf61", Some(".editorconfig"))),
            ("binary", go("620058f867b2", None)),
            ("mode", go("b8efc006f2cb", None)),
            (
                "submodule",
                compare("github.com/rust-lang/rust@4b0bbb5bd087", None),
            ),
            ("capped", go("go1.22.0...go1.23.0", Some("src/runtime"))),
            (
                "annotated-tag",
                compare(
                    "github.com/torvalds/linux@v6.9...v6.10",
                    Some("kernel/sched"),
                ),
            ),
        ]
    }

    #[test]
    #[ignore = "network: run by just capture"]
    fn capture() {
        for (case, request) in cases() {
            testing::capture("compare", case, &plan(&request));
        }
    }

    fn case(name: &str) -> CompareRequest {
        cases()
            .into_iter()
            .find_map(|(case, request)| (case == name).then_some(request))
            .unwrap_or_else(|| panic!("no case {name}"))
    }

    /// The envelope of `tests/compare/<name>.json`.
    macro_rules! envelope {
        ($name:literal) => {
            testing::envelope(include_str!(concat!(
                "../../tests/compare/",
                $name,
                ".json"
            )))
        };
    }

    /// The answer to case `name`, from its capture.
    macro_rules! compare {
        ($name:literal) => {
            answer(&case($name), envelope!($name))
        };
    }

    /// The comparison a result answers with.
    fn answered(
        result: Result<Comparison, Box<dyn Diagnostic>>,
    ) -> Comparison {
        result.unwrap_or_else(|diagnostic| panic!("{}", report(&*diagnostic)))
    }

    /// The comparison's header, then its commits, files and hunks, one to
    /// a line, with short commits.
    fn summary(comparison: &Comparison) -> String {
        let short = |commit: &CommitId| commit.short().to_owned();
        let mut text = String::new();
        let _ = writeln!(
            text,
            "{}@{}{} | ahead {} behind {} | {:?} | {} files{} +{} -{}",
            comparison.repo.as_str(),
            comparison
                .base
                .as_ref()
                .map(|base| short(base) + "...")
                .unwrap_or_default(),
            short(&comparison.head),
            comparison.ahead,
            comparison.behind,
            comparison.commit_count,
            comparison.files_changed,
            if comparison.files_truncated { " (cut)" } else { "" },
            comparison.added,
            comparison.deleted,
        );
        if let Some(path) = &comparison.path {
            let _ = writeln!(
                text,
                "path {path} | file: {}",
                comparison.path_is_file
            );
        }
        if let Some(commit) = &comparison.commit {
            let parents: Vec<String> =
                commit.parents.iter().map(short).collect();
            let _ = writeln!(
                text,
                "{} | {} | parents {}",
                commit.date,
                commit.author,
                parents.join(" ")
            );
            for line in commit.message.lines() {
                let _ = writeln!(text, "    {line:?}");
            }
        }
        for commit in &comparison.commits {
            let _ = writeln!(
                text,
                "{} {} {} | {}",
                short(&commit.commit),
                commit.date,
                commit.author,
                commit.subject
            );
        }
        for file in &comparison.files {
            let _ = write!(text, "{:?} {}", file.status, file.path);
            if let Some(old_path) = &file.old_path {
                let _ = write!(text, " from {old_path}");
            }
            let _ = write!(text, " +{} -{}", file.added, file.deleted);
            if file.binary {
                text.push_str(" binary");
            }
            match &file.readable_at {
                Some(at) => {
                    let _ = writeln!(text, " @{}", short(at));
                }
                None => text.push('\n'),
            }
        }
        for hunk in &comparison.hunks {
            let _ = writeln!(
                text,
                "@@ -{},{} +{},{} @@{}{}{}",
                hunk.old_start,
                hunk.old_lines,
                hunk.new_start,
                hunk.new_lines,
                hunk.section
                    .as_ref()
                    .map(|section| format!(" {section}"))
                    .unwrap_or_default(),
                if hunk.no_newline_old { " [old: no newline]" } else { "" },
                if hunk.no_newline_new { " [new: no newline]" } else { "" },
            );
            for line in &hunk.lines {
                let _ = writeln!(text, "{line:?}");
            }
        }
        text
    }

    /// `envelope`'s data, to edit into a synthetic body.
    fn data(envelope: &mut Envelope) -> &mut serde_json::Value {
        envelope.data.as_mut().expect("the capture has data")
    }

    #[test]
    fn plans_send_literal_paths() {
        let variables = |request: &CompareRequest| plan(request).variables;
        let narrowed = variables(&case("file"));
        assert_eq!(narrowed["paths"], serde_json::json!([":(literal)mux.go"]));
        assert_eq!(narrowed["path"], ":(literal)mux.go");
        assert_eq!(narrowed["plainPath"], "mux.go");
        assert_eq!(
            (&narrowed["base"], &narrowed["head"]),
            (&"v1.7.0".into(), &"v1.8.0".into())
        );
        assert_eq!(
            (&narrowed["commits"], &narrowed["files"]),
            (&50.into(), &200.into())
        );
        assert!(plan(&case("file")).query.contains("hunks:"));
        let commit = variables(&case("commit-file"));
        assert_eq!(
            commit["paths"],
            serde_json::json!([":(literal)regexp.go"])
        );
        assert_eq!(commit["rev"], "98cb6bf42e08");
        assert!(plan(&case("commit-file")).query.contains("hunks:"));
        let root = CompareRequest {
            path: Some(RepoPath::parse("/")),
            ..case("range")
        };
        for request in [case("range"), case("commit"), root] {
            let plan = plan(&request);
            assert!(plan.variables["paths"].is_null(), "{}", plan.variables);
            assert!(plan.variables["path"].is_null(), "{}", plan.variables);
            assert!(!plan.query.contains("hunks:"), "{}", plan.query);
            assert!(!plan.query.contains("plainPath"), "{}", plan.query);
        }
    }

    #[test]
    fn ranges_pin_both_sides_and_count() {
        let range = answered(compare!("range"));
        assert_eq!(
            range.base,
            Some(CommitId("a7962380ca08b5a188038c69871b8d3fbdf31e89".into()))
        );
        assert_eq!(
            range.head,
            CommitId("98cb6bf42e086f6af920b965c38cacc07402d51b".into())
        );
        assert_eq!(range.commit_count, CommitCount::Exact(30));
        assert_eq!((range.ahead, range.behind), (30, 0));
        assert!(range.commit.is_none() && range.path.is_none());
        let mut more = envelope!("range");
        data(&mut more)["refs"]["count"]["behindAhead"]["ahead"] = 31.into();
        let more = answered(answer(&case("range"), more));
        assert_eq!(more.commits.len(), 30);
        assert_eq!(
            (more.ahead, more.commit_count),
            (31, CommitCount::Exact(31))
        );
        assert_eq!(range.commits.len(), 30);
        let commits = range.commits.into_iter().take(3).collect();
        expect![[r#"
            github.com/gorilla/mux@a7962380ca08...98cb6bf42e08 | ahead 30 behind 0 | Exact(30) | 20 files +1033 -378
            98cb6bf42e08 2020-07-11T20:05:21Z Harshavardhana | fix: regression in vars extract for wildcard host (#579)
            948bec34b516 2020-05-17T04:02:54Z Eric Skoglund | docs: Remove stale text from comment. (#568)
            75dcda0896e1 2020-01-12T19:17:43Z Klaus Post | perf: reduce allocations in (*routeRegexp).getURLQuery (#544)
            Added .circleci/config.yml +87 -0 @98cb6bf42e08
            Modified .github/stale.yml +5 -5 @98cb6bf42e08
            Deleted .travis.yml +0 -24 @a7962380ca08
            Deleted ISSUE_TEMPLATE.md +0 -11 @a7962380ca08
            Modified README.md +158 -2 @98cb6bf42e08
            Deleted context.go +0 -18 @a7962380ca08
            Deleted context_test.go +0 -30 @a7962380ca08
            Modified doc.go +1 -1 @98cb6bf42e08
            Added example_cors_method_middleware_test.go +37 -0 @98cb6bf42e08
            Modified go.mod +2 -0 @98cb6bf42e08
            Modified middleware.go +29 -27 @98cb6bf42e08
            Modified middleware_test.go +317 -189 @98cb6bf42e08
            Modified mux.go +21 -16 @98cb6bf42e08
            Added mux_httpserver_test.go +49 -0 @98cb6bf42e08
            Modified mux_test.go +94 -13 @98cb6bf42e08
            Modified old_test.go +27 -13 @98cb6bf42e08
            Modified regexp.go +81 -21 @98cb6bf42e08
            Added regexp_test.go +91 -0 @98cb6bf42e08
            Modified route.go +33 -7 @98cb6bf42e08
            Modified test_helpers.go +1 -1 @98cb6bf42e08
        "#]]
        .assert_eq(&summary(&Comparison { commits, ..range }));
        for (name, result, base, head) in [
            (
                "short-revs",
                compare!("short-revs"),
                "a7962380ca08b5a188038c69871b8d3fbdf31e89",
                "98cb6bf42e086f6af920b965c38cacc07402d51b",
            ),
            (
                "annotated-tag",
                compare!("annotated-tag"),
                "a38297e3fb012ddfa7ce0321a7e5a8daeb1872b6",
                "0c3836482481200ead7b416ca80c68a29cfdaabd",
            ),
        ] {
            let comparison = answered(result);
            assert_eq!(
                (comparison.base.as_ref(), &comparison.head),
                (Some(&CommitId(base.into())), &CommitId(head.into())),
                "{name}"
            );
        }
    }

    #[test]
    fn statuses_are_derived() {
        let mut text = String::new();
        for result in [
            compare!("file-added"),
            compare!("file-deleted"),
            compare!("renamed"),
            compare!("binary"),
            compare!("mode"),
            compare!("submodule"),
        ] {
            let comparison = answered(result);
            for file in &comparison.files {
                if file.binary {
                    assert_eq!((file.added, file.deleted), (0, 0));
                }
            }
            if comparison.head.0.starts_with("b8efc006") {
                assert!(comparison.files.iter().all(|file| {
                    file.status == FileStatus::Modified
                        && (file.added, file.deleted) == (0, 0)
                }));
            }
            // A release's other files and a mode-only commit's other binary
            // files add nothing here.
            let shown = [
                "doc/progs/run.go",
                "src/debug/dwarf/testdata/line-clang.elf",
            ];
            let files = comparison
                .files
                .into_iter()
                .filter(|file| {
                    comparison.files_changed < 10
                        || shown.contains(&file.path.as_str())
                        || !matches!(
                            file.status,
                            FileStatus::Added | FileStatus::Modified
                        )
                })
                .collect();
            text.push_str(&summary(&Comparison {
                commit: None,
                commits: Vec::new(),
                files,
                hunks: Vec::new(),
                ..comparison
            }));
        }
        expect![[r#"
            github.com/gorilla/mux@a7962380ca08...98cb6bf42e08 | ahead 30 behind 0 | Exact(3) | 1 files +87 -0
            path .circleci/config.yml | file: true
            Added .circleci/config.yml +87 -0 @98cb6bf42e08
            github.com/gorilla/mux@a7962380ca08...98cb6bf42e08 | ahead 30 behind 0 | Exact(1) | 1 files +0 -18
            path context.go | file: true
            Deleted context.go +0 -18 @a7962380ca08
            github.com/gorilla/mux@7f0880185913...a7962380ca08 | ahead 50 behind 0 | Exact(50) | 23 files +2173 -489
            Renamed context.go from context_native.go +0 -6 @a7962380ca08
            Deleted context_gorilla.go +0 -26 @7f0880185913
            Deleted context_gorilla_test.go +0 -40 @7f0880185913
            Renamed context_test.go from context_native_test.go +0 -2 @a7962380ca08
            github.com/golang/go@620058f867b2 | ahead 1 behind 0 | Exact(1) | 3 files +25 -1
            Modified src/debug/elf/file.go +1 -1 @620058f867b2
            Modified src/debug/elf/file_test.go +24 -0 @620058f867b2
            Added src/debug/elf/testdata/go-relocation-test-gcc133-ppc.obj +0 -0 binary @620058f867b2
            github.com/golang/go@b8efc006f2cb | ahead 1 behind 0 | Exact(1) | 10 files +0 -0
            Modified doc/progs/run.go +0 -0 @b8efc006f2cb
            Modified src/debug/dwarf/testdata/line-clang.elf +0 -0 binary @b8efc006f2cb
            github.com/rust-lang/rust@4b0bbb5bd087 | ahead 1 behind 0 | Exact(1) | 1 files +1 -1
            Modified src/tools/cargo +1 -1 @4b0bbb5bd087
        "#]].assert_eq(&text);
    }

    #[test]
    fn deleted_files_are_readable_at_the_base_only_when_it_holds_them() {
        let base = |comparison: &Comparison| comparison.base.clone();
        let deleted = answered(compare!("file-deleted"));
        assert_eq!(deleted.behind, 0);
        assert_eq!(deleted.files[0].status, FileStatus::Deleted);
        assert_eq!(deleted.files[0].readable_at, base(&deleted));
        let mut envelope = envelope!("file-deleted");
        data(&mut envelope)["refs"]["count"]["behindAhead"]["behind"] =
            1.into();
        let divergent = answered(answer(&case("file-deleted"), envelope));
        assert_eq!(divergent.behind, 1);
        assert_eq!(divergent.files[0].status, FileStatus::Deleted);
        assert_eq!(divergent.files[0].readable_at, None);
    }

    #[test]
    fn caps_cut_the_lists() {
        let capped = answered(compare!("capped"));
        assert_eq!(capped.commits.len(), 50);
        assert_eq!(capped.commit_count, CommitCount::MoreThan(50));
        assert_eq!(capped.files.len(), 200);
        assert_eq!(capped.files_changed, 320);
        assert!(capped.files_truncated);
        assert_eq!((capped.added, capped.deleted), (11534, 21469));
        assert_eq!(capped.behind, 24);
        let deleted: Vec<&FileChange> = capped
            .files
            .iter()
            .filter(|file| file.status == FileStatus::Deleted)
            .collect();
        assert!(!deleted.is_empty());
        assert!(deleted.iter().all(|file| file.readable_at.is_none()));
        let renamed = answered(compare!("renamed"));
        assert_eq!(renamed.commits.len(), 50);
        assert_eq!(renamed.commit_count, CommitCount::Exact(50));
        assert!(!renamed.files_truncated);
        let narrowed = answered(compare!("annotated-tag"));
        assert_eq!(narrowed.commit_count, CommitCount::MoreThan(50));
        assert_eq!(narrowed.files.len(), 15);
        assert!(!narrowed.files_truncated);
    }

    #[test]
    fn empty_and_reversed() {
        let equal = answered(compare!("equal"));
        assert!(equal.commits.is_empty() && equal.files.is_empty());
        assert_eq!(equal.behind, 0);
        assert_eq!(equal.commit_count, CommitCount::Exact(0));
        let reversed = answered(compare!("reversed"));
        assert!(reversed.commits.is_empty() && reversed.files.is_empty());
        assert_eq!(reversed.commit_count, CommitCount::Exact(0));
        assert_eq!((reversed.ahead, reversed.behind), (0, 30));
        // Narrowed to a path the range didn't touch, a comparison lists no
        // more than a reversed one; `ahead` tells them apart.
        let untouched = answered(compare!("unchanged-path"));
        assert!(untouched.commits.is_empty() && untouched.files.is_empty());
        assert_eq!(untouched.commit_count, CommitCount::Exact(0));
        assert_eq!(untouched.ahead, 30);
        let mut reversed_at_path = envelope!("unchanged-path");
        data(&mut reversed_at_path)["refs"]["count"]["behindAhead"] =
            serde_json::json!({"behind": 30, "ahead": 0});
        let reversed_at_path =
            answered(answer(&case("unchanged-path"), reversed_at_path));
        assert_eq!(reversed_at_path.commit_count, CommitCount::Exact(0));
        assert_eq!((reversed_at_path.ahead, reversed_at_path.behind), (0, 30));
        expect![[r#"
            github.com/gorilla/mux@98cb6bf42e08...98cb6bf42e08 | ahead 0 behind 0 | Exact(0) | 0 files +0 -0
            github.com/gorilla/mux@98cb6bf42e08...a7962380ca08 | ahead 0 behind 30 | Exact(0) | 0 files +0 -0
        "#]].assert_eq(&(summary(&equal) + &summary(&reversed)));
    }

    #[test]
    fn paths() {
        let file = answered(compare!("file"));
        assert!(file.path_is_file && !file.hunks.is_empty());
        assert!(file.hunks.iter().all(|hunk| !hunk.lines.is_empty()));
        let headers = file
            .hunks
            .into_iter()
            .map(|hunk| DiffHunk { lines: Vec::new(), ..hunk })
            .collect();
        expect![[r#"
            github.com/gorilla/mux@a7962380ca08...98cb6bf42e08 | ahead 30 behind 0 | Exact(4) | 1 files +21 -16
            path mux.go | file: true
            948bec34b516 2020-05-17T04:02:54Z Eric Skoglund | docs: Remove stale text from comment. (#568)
            f395758b854c 2019-10-24T12:12:56Z Franklin Harding | Remove/cleanup request context helpers (#525)
            eab9c4f3d22d 2019-07-20T14:49:38Z Christian Muehlhaeuser | Simplify code (#501)
            15a353a63672 2019-02-28T18:12:03Z seriousben | adding Router.Name to create new Route (#457)
            Modified mux.go +21 -16 @98cb6bf42e08
            @@ -5,6 +5,7 @@
            @@ -58,8 +59,7 @@ type Router struct {
            @@ -111,10 +111,8 @@ func copyRouteConf(r routeConf) routeConf {
            @@ -197,8 +195,8 @@ func (r *Router) ServeHTTP(w http.ResponseWriter, req *http.Request) {
            @@ -283,6 +281,12 @@ func (r *Router) NewRoute() *Route {
            @@ -422,30 +426,31 @@ const (
        "#]]
            .assert_eq(&summary(&Comparison { hunks: headers, ..file }));
        let directory = answered(compare!("directory"));
        assert!(!directory.path_is_file && directory.hunks.is_empty());
        expect![[r#"
            github.com/gorilla/mux@a7962380ca08...98cb6bf42e08 | ahead 30 behind 0 | Exact(1) | 1 files +5 -5
            path .github | file: false
            00bdffe0f3c7 2019-06-30T04:17:52Z Matt Silverlock | Update stale.yml (#494)
            Modified .github/stale.yml +5 -5 @98cb6bf42e08
        "#]].assert_eq(&summary(&directory));
        let unchanged = answered(compare!("unchanged-path"));
        assert!(unchanged.path_is_file && unchanged.files.is_empty());
        assert!(unchanged.commits.is_empty() && unchanged.hunks.is_empty());
        assert_eq!(unchanged.commit_count, CommitCount::Exact(0));
        expect![[r#"
            [path_not_found AnsweredNo]
            nope.go not found in github.com/gorilla/mux@98cb6bf42e08
            hint: sego list github.com/gorilla/mux@98cb6bf42e08
        "#]]
        .assert_eq(&failure(compare!("missing-path")));
        let renamed = answered(compare!("renamed-file"));
        assert!(renamed.path_is_file);
        expect![[r#"
            github.com/gorilla/mux@7f0880185913...a7962380ca08 | ahead 50 behind 0 | Exact(2) | 1 files +18 -0
            path context.go | file: true
            6137e193cdcb 2018-12-17T14:42:43Z alimy | remove redundant code that remove support gorilla/context (#427)
            8771f97498c6 2018-09-02T22:22:40Z Franklin Harding | Drop support for Go < 1.7: remove gorilla/context (#391)
            Added context.go +18 -0 @a7962380ca08
        "#]]
            .assert_eq(&summary(&Comparison { hunks: Vec::new(), ..renamed }));
    }

    #[test]
    fn single_commits() {
        let commit = answered(compare!("commit"));
        assert_eq!(commit.base, None);
        assert_eq!(
            (commit.behind, commit.commit_count),
            (0, CommitCount::Exact(1))
        );
        assert!(commit.commits.is_empty());
        expect![[r#"
            github.com/gorilla/mux@98cb6bf42e08 | ahead 1 behind 0 | Exact(1) | 2 files +28 -1
            2020-07-11T20:05:21Z | Harshavardhana | parents 948bec34b516
                "fix: regression in vars extract for wildcard host (#579)"
                ""
                "Continuing from PR #447 we have to add extra"
                "check to ignore the port as well"
                ""
                "add tests to cover this case"
            Modified old_test.go +22 -1 @98cb6bf42e08
            Modified regexp.go +6 -0 @98cb6bf42e08
        "#]].assert_eq(&summary(&commit));
        let merge = answered(compare!("merge"));
        expect![[r#"
            github.com/gorilla/mux@2854a05fd3e2 | ahead 1 behind 0 | Exact(1) | 2 files +30 -15
            2019-11-17T01:33:21Z | Matt Silverlock | parents f395758b854c 946b6237eb8d
                "Merge pull request #535 from fharding1/cors-subrouter-bug"
                ""
                "Fix the CORSMethodMiddleware bug with subrouters"
            Modified middleware.go +10 -15 @2854a05fd3e2
            Modified middleware_test.go +20 -0 @2854a05fd3e2
        "#]].assert_eq(&summary(&merge));
        let mut text = String::new();
        for result in
            [compare!("multi-line-subject"), compare!("crlf-message")]
        {
            let comparison = answered(result);
            let message = &comparison.commit.as_ref().unwrap().message;
            assert!(!message.contains('\r'), "{message:?}");
            text.push_str(&summary(&comparison));
        }
        expect![[r#"
            github.com/gorilla/mux@e48e440e4c92 | ahead 1 behind 0 | Exact(1) | 1 files +9 -0
            2018-08-07T07:52:56Z | Kamil Kisiel | parents 815b8c6a2641
                "Add test for multiple calls to Name()."
                "Fixes #394"
            Modified mux_test.go +9 -0 @e48e440e4c92
            github.com/gorilla/mux@ed099d423848 | ahead 1 behind 0 | Exact(1) | 1 files +27 -10
            2019-05-17T00:20:44Z | M@ | parents c5c6c98bc253
                "host:port matching does not require a :port to be specified."
                ""
                "In lieu of checking the template pattern on every Match request, a bool is added to the routeRegexp, and set"
                "if the routeRegexp is a host AND there is no \":\" in the template. I dislike extending the type, but I'd dislike"
                "doing a string match on every single Match, even more."
            Modified regexp.go +27 -10 @ed099d423848
        "#]].assert_eq(&text);
        expect![[r#"
            github.com/gorilla/mux@98cb6bf42e08 | ahead 1 behind 0 | Exact(1) | 1 files +6 -0
            path regexp.go | file: true
            Modified regexp.go +6 -0 @98cb6bf42e08
            @@ -325,6 +325,12 @@ func (v routeRegexpGroup) setMatch(req *http.Request, m *RouteMatch, r *Route) {
            " \t// Store host variables."
            " \tif v.host != nil {"
            " \t\thost := getHost(req)"
            "+\t\tif v.host.wildcardHostPort {"
            "+\t\t\t// Don't be strict on the port match"
            "+\t\t\tif i := strings.Index(host, \":\"); i != -1 {"
            "+\t\t\t\thost = host[:i]"
            "+\t\t\t}"
            "+\t\t}"
            " \t\tmatches := v.host.regexp.FindStringSubmatchIndex(host)"
            " \t\tif len(matches) > 0 {"
            " \t\t\textractVars(host, matches, v.host.varsN, m.Vars)"
        "#]].assert_eq(&summary(&Comparison {
            commit: None,
            ..answered(compare!("commit-file"))
        }));
        expect![[r#"
            [no_parent AnsweredNo]
            github.com/gorilla/mux@4e8d10dc715f is a root commit, with no parent to compare with
            hint: sego list github.com/gorilla/mux@4e8d10dc715f
        "#]].assert_eq(&failure(compare!("root")));
        let mut untouched = envelope!("commit-file");
        let diff = &mut data(&mut untouched)["diff"]["commit"]["diff"];
        diff["totals"] = serde_json::json!({
            "totalCount": 0, "diffStat": {"added": 0, "deleted": 0}
        });
        diff["fileDiffs"]["nodes"] = serde_json::json!([]);
        diff["hunks"]["nodes"] = serde_json::json!([]);
        let untouched = answered(answer(&case("commit-file"), untouched));
        assert!(untouched.path_is_file);
        assert!(untouched.files.is_empty() && untouched.hunks.is_empty());
        assert_eq!(untouched.files_changed, 0);
    }

    #[test]
    fn no_newline_markers() {
        let old = answered(compare!("no-newline-old"));
        let flags = |comparison: &Comparison| {
            let hunk = comparison.hunks.last().unwrap();
            (hunk.no_newline_old, hunk.no_newline_new)
        };
        assert_eq!(flags(&old), (true, false));
        let mut new = answered(compare!("no-newline-new"));
        assert_eq!(flags(&new), (false, true));
        assert_eq!(new.hunks[0].lines.len(), 20);
        let mut crlf = envelope!("no-newline-new");
        data(&mut crlf)["diff"]["commit"]["diff"]["hunks"]["nodes"][0]["hunks"]
            [0]["body"] = "+a\r\n".repeat(20).into();
        let crlf = answered(answer(&case("no-newline-new"), crlf));
        assert_eq!(crlf.hunks[0].lines[0], "+a\r");
        assert_eq!(crlf.hunks[0].lines.len(), 20);
        new.hunks[0].lines.drain(..18);
        expect![[r#"
            github.com/golang/go@48469a2c866c | ahead 1 behind 0 | Exact(1) | 1 files +1 -1
            path src/cmd/ld/lib.h | file: true
            Modified src/cmd/ld/lib.h +1 -1 @48469a2c866c
            @@ -346,4 +346,4 @@ void	ldmain(int, char**); [old: no newline]
            " "
            " #pragma\tvarargck\targpos\tdiag\t1"
            " "
            "-#define\tSYMDEF\t\"__.GOSYMDEF\""
            "+#define\tSYMDEF\t\"__.GOSYMDEF\""
            github.com/gorilla/mux@1fa2ee87bf61 | ahead 1 behind 0 | Exact(1) | 1 files +20 -0
            path .editorconfig | file: true
            Added .editorconfig +20 -0 @1fa2ee87bf61
            @@ -0,0 +1,20 @@ [new: no newline]
            "+"
            "+eclint_indent_style = unset"
        "#]].assert_eq(
            &(summary(&Comparison { commit: None, ..old })
                + &summary(&Comparison { commit: None, ..new })),
        );
    }

    #[test]
    fn missing_sides() {
        let mut both = envelope!("missing-base");
        data(&mut both)["refs"]["head"] = serde_json::Value::Null;
        let mut text = String::new();
        for result in [
            compare!("missing-base"),
            compare!("missing-head"),
            answer(&case("missing-base"), both),
            compare!("missing-repo"),
        ] {
            text.push_str(&failure(result));
        }
        assert!(!text.contains("1700"), "{text}");
        expect![[r#"
            [revision_not_found AnsweredNo]
            base revision v9.9.9 not found in github.com/gorilla/mux
            hint: sego list github.com/gorilla/mux
            [revision_not_found AnsweredNo]
            head revision nope-branch not found in github.com/gorilla/mux
            hint: sego list github.com/gorilla/mux
            [revision_not_found AnsweredNo]
            revisions v9.9.9 and v1.8.0 not found in github.com/gorilla/mux
            hint: sego list github.com/gorilla/mux
            [repo_not_found AnsweredNo]
            repository github.com/gorilla/doesnotexist not found
            hint: sego search 'type:repo doesnotexist'
        "#]]
        .assert_eq(&text);
    }

    #[test]
    fn other_errors_are_server_errors() {
        let secret = "revision not found: repo 1700 @v1.7.0^0";
        let cmp = serde_json::json!({
            "message": secret, "path": ["cmp", "comparison"]
        });
        let mut range = envelope!("range");
        data(&mut range)["cmp"] = serde_json::Value::Null;
        let range = decoded(
            serde_json::json!({"data": range.data, "errors": [cmp]})
                .to_string(),
        );
        let diff = serde_json::json!({
            "message": secret, "path": ["diff", "commit", "diff"]
        });
        let mut commit = envelope!("commit");
        data(&mut commit)["diff"]["commit"] = serde_json::Value::Null;
        let commit = decoded(
            serde_json::json!({"data": commit.data, "errors": [diff]})
                .to_string(),
        );
        let mut text = String::new();
        for (request, envelope) in
            [(case("range"), range), (case("commit"), commit)]
        {
            text.push_str(&failure(answer(&request, envelope)));
        }
        assert!(!text.contains("1700"), "{text}");
        expect![[r#"
            [server_error Unanswered]
            error: the upstream answered with an error
            caused by: the upstream couldn't compare these revisions
            [server_error Unanswered]
            error: the upstream answered with an error
            caused by: the upstream couldn't diff this commit
        "#]]
        .assert_eq(&text);
    }
}
