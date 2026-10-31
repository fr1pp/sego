//! What a path names at a commit: a file, a directory or a submodule, read
//! through the `path` union under the one revision lookup.

use serde::Deserialize;
use serde_json::{Map, json};

use super::Page;
use super::graphql::{Envelope, GqlErrorKind, Request, Segment, View};
use super::resolve::{self, Repository, Resolved, repository_variables};
use crate::address::{Repo, Target, pinned};
use crate::error::Error;
use crate::pointer;

/// How many lines an open-ended window asks for, at most.
const OPEN_END_SPAN: i64 = 500_000;

/// What a commit holds at `$path`: `meta` says what the path names, and `body`
/// holds a file's lines apart from it, since the content limit's error nulls
/// its parent.
const READ: &str = "\
    oid
    meta: path(path: $path) {
      __typename
      ... on GitBlob { path byteSize totalLines binary rawURL lfs { byteSize } submodule { commit } }
      ... on GitTree { path entries(recursive: $recursive) { path isDirectory submodule { commit } } }
    }
    body: path(path: $path) @include(if: $content) {
      __typename
      ... on GitBlob { content(startLine: $start, endLine: $end, textOnly: true) }
    }";

/// What a path names.
#[derive(Debug, PartialEq, Eq)]
pub enum File {
    Blob(Blob),
    Directory(Directory),
    Submodule(Submodule),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Blob {
    pub byte_size: u64,
    /// Sourcegraph's classification, not git's.
    pub binary: bool,
    /// 0 for an empty file.
    pub total_lines: u32,
    /// The size of the object an LFS pointer names.
    pub lfs_size: Option<u64>,
    /// The lines asked for; `None` when there are none to read: the file is
    /// binary, or has fewer lines than the first asked for.
    pub content: Option<Content>,
}

/// The lines of a window of a file.
#[derive(Debug, PartialEq, Eq)]
pub struct Content {
    pub lines: Vec<Line>,
    /// Whether at least one line shown ends a line and every one that does
    /// ends in CRLF.
    pub crlf: bool,
    /// Whether the lines are valid UTF-8; `Err` when Sourcegraph's text held
    /// U+FFFD and the raw read for the exact bytes failed.
    pub utf8: Result<bool, Error>,
    /// Why the cache couldn't be used, when a raw read went without it.
    pub cache_unavailable: Option<String>,
}

/// A line of a file: its bytes without the line feed, and whether one ended
/// it.
#[derive(Debug, PartialEq, Eq)]
pub struct Line {
    pub bytes: Vec<u8>,
    pub terminated: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Directory {
    /// Every entry, in the server's order.
    pub entries: Page<Entry>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub is_directory: bool,
    pub submodule: Option<Submodule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submodule {
    pub commit: String,
}

/// The lines asked of a file: from `start`, to `end` or the last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub start: u32,
    pub end: Option<u32>,
}

/// What the response gave for a path, before the raw read.
pub struct Read {
    pub resolved: Resolved,
    pub file: File,
    pub body: Body,
    /// The site path of a file's raw read.
    pub raw: Option<String>,
}

/// Where a file's lines come from.
pub enum Body {
    /// No lines are read.
    None,
    /// Sourcegraph's text of the window.
    Text(String),
    /// The file is over the content limit: only a raw read has its lines.
    OverLimit,
}

/// The lines of `text`, split after each `\n` as git numbers them.
pub fn lines(text: &[u8]) -> Vec<Line> {
    let mut lines: Vec<Line> = text
        .split(|&b| b == b'\n')
        .map(|bytes| Line { bytes: bytes.to_vec(), terminated: true })
        .collect();
    if let Some(last) = lines.pop()
        && !last.bytes.is_empty()
    {
        lines.push(Line { terminated: false, ..last });
    }
    lines
}

/// Whether `lines` hold at least one terminated line and every terminated
/// line ends in CRLF.
pub fn crlf(lines: &[Line]) -> bool {
    let mut terminated = lines.iter().filter(|line| line.terminated).peekable();
    terminated.peek().is_some() && terminated.all(|line| line.bytes.ends_with(b"\r"))
}

/// The request for what `target` names in `repo`: the lines of `window`
/// when given, and every entry beneath a directory when `recursive`.
pub fn request(repo: &Repo, target: &Target, window: Option<Window>, recursive: bool) -> Request {
    let mut variables = Map::new();
    variables.insert("recursive".to_owned(), json!(recursive));
    variables.insert("content".to_owned(), json!(window.is_some()));
    if let Some(Window { start, end }) = window {
        let start = i64::from(start);
        let end = end.map_or_else(|| (start + OPEN_END_SPAN).min(i64::from(i32::MAX)), i64::from);
        variables.insert("start".to_owned(), json!(start));
        variables.insert("end".to_owned(), json!(end));
    }
    variables.extend(repository_variables(repo));
    variables.insert("rev".to_owned(), json!(target.rev.as_deref().unwrap_or("HEAD")));
    variables.insert("path".to_owned(), json!(target.path.as_deref().unwrap_or_default()));
    let document = format!(
        "query File($repo0: String!, $repo1: String!, $gitlab: Boolean!, $rev: String!, \
         $path: String!, $recursive: Boolean!, $content: Boolean!, $start: Int, $end: Int) {{
  r0: repository(name: $repo0) {{ ...File }}
  r1: repository(name: $repo1) @include(if: $gitlab) {{ ...File }}
}}

fragment File on Repository {{
  name
  defaultBranch {{ displayName }}
  mirrorInfo {{ updatedAt }}
  commit(rev: $rev) {{
{READ}
  }}
}}
"
    );
    Request::new("File", document, variables)
}

/// What `target` names in `repo`, as the response to `request` gives it.
pub fn read(envelope: &Envelope, repo: &Repo, target: &Target) -> Result<Read, Error> {
    let found = resolve::repository(repo, envelope, resolve::roots(repo))?;
    let at = [found.root.as_str(), "commit"];
    let rev = target.rev.as_deref().unwrap_or("HEAD");
    let commit = resolve::commit(envelope, &found, &at, rev)?;
    let path = target.path.clone().unwrap_or_default();
    read_at(envelope, &found, &at, commit, target.rev.clone(), &path)
}

#[derive(Deserialize)]
#[serde(tag = "__typename")]
enum Meta {
    GitBlob(BlobNode),
    GitTree(TreeNode),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlobNode {
    path: String,
    byte_size: u64,
    total_lines: u64,
    binary: bool,
    #[serde(rename = "rawURL")]
    raw_url: Option<String>,
    lfs: Option<Lfs>,
    submodule: Option<SubmoduleNode>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Lfs {
    byte_size: String,
}

#[derive(Deserialize)]
struct TreeNode {
    path: String,
    entries: Vec<EntryNode>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EntryNode {
    path: String,
    is_directory: bool,
    submodule: Option<SubmoduleNode>,
}

#[derive(Deserialize)]
struct SubmoduleNode {
    commit: String,
}

/// What the commit at `at`, resolved to `commit`, holds at `path`.
fn read_at(
    envelope: &Envelope,
    found: &Repository,
    at: &[&str],
    commit: String,
    rev: Option<String>,
    path: &str,
) -> Result<Read, Error> {
    let at: Vec<String> = at.iter().map(|key| (*key).to_owned()).collect();
    let view = View::new(envelope, &at);
    let Some(meta) = view.get(&["meta"]) else {
        let errors = view.errors_beneath(&["meta"]);
        if errors.iter().any(|e| e.kind() != GqlErrorKind::RevisionNotFound) {
            let entries = Segment::Key("entries".to_owned());
            let listing = errors.iter().any(|e| e.path.contains(&entries));
            let part = if listing { "the directory listing" } else { "the file's details" };
            return Err(Error::failed(part));
        }
        let at = pinned(&found.name, &commit, None);
        if path.is_empty() {
            return Err(Error::not_found(format!("./ not found in {at}")));
        }
        let missing =
            pointer::missing_path(pointer::At { repo: &found.name, commit: &commit }, path);
        return Err(Error::not_found(format!("{path} not found in {at}")).facts(missing));
    };
    let lfs_errors = view.errors_beneath(&["meta", "lfs"]);
    let lfs_failed = lfs_errors.iter().any(|e| e.kind() != GqlErrorKind::OverLimit);
    let other_failed = view.errors_beneath(&["meta"]).len() > lfs_errors.len();
    let mut raw = None;
    let (file, path, body) = match Meta::deserialize(meta) {
        Ok(Meta::GitTree(tree)) if !other_failed => {
            let items: Vec<Entry> = tree
                .entries
                .into_iter()
                .map(|node| Entry {
                    submodule: node.submodule.map(|module| Submodule { commit: module.commit }),
                    path: node.path,
                    is_directory: node.is_directory,
                })
                .collect();
            let total = Some(items.len() as u64);
            let entries = Page { items, total, more: false };
            (File::Directory(Directory { entries }), tree.path, Body::None)
        }
        Ok(Meta::GitBlob(blob)) if !other_failed && !lfs_failed => {
            if let Some(module) = blob.submodule {
                let module = Submodule { commit: module.commit };
                (File::Submodule(module), blob.path, Body::None)
            } else {
                let details_failed = || Error::failed("the file's details");
                let lfs_size = blob
                    .lfs
                    .map(|lfs| lfs.byte_size.parse::<u64>().map_err(|_| details_failed()))
                    .transpose()?;
                let total_lines = if blob.byte_size == 0 {
                    0
                } else {
                    u32::try_from(blob.total_lines).map_err(|_| details_failed())?
                };
                raw = blob.raw_url.filter(|raw| raw.starts_with('/') && !raw.starts_with("//"));
                let body = body(&view, blob.binary)?;
                let file = File::Blob(Blob {
                    byte_size: blob.byte_size,
                    binary: blob.binary,
                    total_lines,
                    lfs_size,
                    content: None,
                });
                (file, blob.path, body)
            }
        }
        Ok(Meta::GitTree(_)) => return Err(Error::failed("the directory listing")),
        Ok(Meta::GitBlob(_)) | Err(_) => return Err(Error::failed("the file's details")),
    };
    let resolved = Resolved::new(found, commit, rev, Some(path));
    Ok(Read { resolved, file, body, raw })
}

/// Where a text file's lines come from: Sourcegraph's text, or for a file
/// over the content limit, a raw read.
fn body(view: &View<'_>, binary: bool) -> Result<Body, Error> {
    if binary {
        return Ok(Body::None);
    }
    if let Some(text) = view.read::<String>(&["body", "content"]) {
        return Ok(Body::Text(text));
    }
    let errors = view.errors_beneath(&["body"]);
    if errors.is_empty() {
        return Ok(Body::None);
    }
    if errors.iter().all(|e| e.kind() == GqlErrorKind::OverLimit) {
        return Ok(Body::OverLimit);
    }
    Err(Error::failed("the file content"))
}

/// The lines of `window` in a file of `total` lines. `raw` reads the file's
/// exact bytes, with why the cache was unavailable: it is needed for a file
/// over the content limit, and for text holding U+FFFD, where it only makes
/// Sourcegraph's text exact, so its failure leaves the bytes unknown.
pub fn content(
    body: Body,
    window: Window,
    total: u32,
    raw: impl FnOnce() -> Result<(Vec<u8>, Option<String>), Error>,
) -> Result<Option<Content>, Error> {
    let start = window.start;
    if start > total {
        return Ok(None);
    }
    let last = window.end.unwrap_or(total).min(total);
    let wanted = (last - start + 1) as usize;
    let whole = |bytes: &[u8]| -> Vec<Line> {
        lines(bytes).into_iter().skip((start - 1) as usize).take(wanted).collect()
    };
    let (lines, utf8, cache_unavailable) = match body {
        Body::None => return Ok(None),
        Body::Text(text) if !text.contains('\u{fffd}') => (lines(text.as_bytes()), Ok(true), None),
        Body::Text(text) => match raw() {
            Ok((bytes, unavailable)) => {
                let lines = whole(&bytes);
                let utf8 = lines.iter().all(|line| std::str::from_utf8(&line.bytes).is_ok());
                (lines, Ok(utf8), unavailable)
            }
            Err(_) => (lines(text.as_bytes()), Err(Error::failed("the raw read")), None),
        },
        Body::OverLimit => {
            let (bytes, unavailable) = raw()?;
            let lines = whole(&bytes);
            let utf8 = lines.iter().all(|line| std::str::from_utf8(&line.bytes).is_ok());
            (lines, Ok(utf8), unavailable)
        }
    };
    if lines.len() != wanted {
        return Err(Error::failed("the file content"));
    }
    let crlf = crlf(&lines);
    Ok(Some(Content { lines, crlf, utf8, cache_unavailable }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(text: &[u8]) -> (Vec<(String, bool)>, bool) {
        let lines = lines(text);
        let crlf = crlf(&lines);
        let lines = lines
            .into_iter()
            .map(|line| (String::from_utf8_lossy(&line.bytes).into(), line.terminated));
        (lines.collect(), crlf)
    }

    fn line(text: &str, terminated: bool) -> (String, bool) {
        (text.to_owned(), terminated)
    }

    #[test]
    fn lines_split_after_each_line_feed_and_the_last_may_have_none() {
        assert_eq!(split(b""), (vec![], false));
        assert_eq!(split(b"a\nb"), (vec![line("a", true), line("b", false)], false));
        assert_eq!(split(b"a\n\n"), (vec![line("a", true), line("", true)], false));
    }

    #[test]
    fn a_window_is_crlf_when_every_terminated_line_ends_in_cr() {
        assert!(split(b"a\r\nb\r\n").1);
        assert!(split(b"a\r\nb").1);
        assert!(!split(b"a\nb\r\r\nc\n").1);
        assert_eq!(split(b"a\rb\r"), (vec![line("a\rb\r", false)], false));
        assert!(!split(b"x\r").1);
    }
}
