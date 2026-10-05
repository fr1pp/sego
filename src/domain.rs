use crate::address::{CommitId, Pinned, RepoName};

/// Lines of a file, from line `start`. A binary file has no lines.
#[derive(Debug)]
pub struct File {
    pub at: Pinned,
    pub path: String,
    pub byte_size: u64,
    pub total_lines: u32,
    pub content_type: String,
    pub binary: bool,
    pub start: u32,
    pub lines: Vec<String>,
}

/// The commits that last changed a file's lines, from line `start` to
/// `end`, the last line shown. A binary or empty file has no hunks; over the
/// read cap there are no `lines`.
#[derive(Debug)]
pub struct Blame {
    pub at: Pinned,
    pub path: String,
    pub byte_size: u64,
    pub total_lines: u32,
    pub binary: bool,
    pub start: u32,
    pub end: u32,
    pub hunks: Vec<Hunk>,
    pub lines: Option<Vec<String>>,
}

/// Lines `start` to `end`, inclusive, as `commit` left them, at `path` as
/// it was in that commit. `date` is the author date as the upstream gives
/// it; `subject` is the commit's summary line.
#[derive(Debug)]
pub struct Hunk {
    pub start: u32,
    pub end: u32,
    pub commit: CommitId,
    pub path: String,
    pub date: String,
    pub author: Author,
    pub subject: String,
}

#[derive(Debug)]
pub struct Author {
    pub name: String,
    pub email: String,
}

/// A directory's entries, in path order. A truncated listing is missing
/// entries from anywhere in it: the upstream cuts it at its limit after
/// sending every directory before any file.
#[derive(Debug)]
pub struct Listing {
    pub at: Pinned,
    pub path: String,
    pub entries: Vec<Entry>,
    pub truncated: bool,
}

/// A path in a listing, from the repository's root.
#[derive(Debug)]
pub struct Entry {
    pub path: String,
    pub kind: EntryKind,
}

#[derive(Debug)]
pub enum EntryKind {
    Directory,
    File,
    /// The submodule's URL as `.gitmodules` gives it, and the repository
    /// and commit it pins, if the URL names a repository.
    Submodule {
        url: String,
        target: Option<Pinned>,
    },
}

/// A submodule asked for as a directory: its URL as `.gitmodules` gives it,
/// and the repository and commit it pins, if the URL names a repository.
#[derive(Debug)]
pub struct Submodule {
    pub at: Pinned,
    pub path: String,
    pub url: String,
    pub target: Option<Pinned>,
}

/// What a directory path answers with.
#[derive(Debug)]
pub enum Tree {
    Listing(Listing),
    Submodule(Submodule),
}

/// A match within a line, as codepoint offsets: `start` inclusive, `end`
/// exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

/// A numbered line of a search hit. A line with no spans is context.
#[derive(Debug)]
pub struct Line {
    pub number: u32,
    pub text: String,
    pub spans: Vec<Span>,
}

/// An upstream value in sego's vocabulary. An unknown value is `other`,
/// with the upstream's value in `raw`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Term {
    pub value: &'static str,
    pub raw: Option<String>,
}

/// A symbol a search matched, on its 1-based line.
#[derive(Debug)]
pub struct Symbol {
    pub line: u32,
    pub kind: &'static str,
    pub name: String,
    pub container: Option<String>,
}

/// A file a search matched: by its path, its lines or its symbols.
#[derive(Debug)]
pub struct FileHit {
    pub at: Pinned,
    pub path: String,
    pub path_match: bool,
    /// True when any of its matched content has a NUL byte; a binary hit
    /// has no lines. Search never asks the upstream, whose answer reads every
    /// hit's whole content.
    pub binary: bool,
    pub lines: Vec<Line>,
    pub symbols: Vec<Symbol>,
}

/// A commit a search matched, with its diff preview for a diff search.
/// `date` is the author date as the upstream gives it.
#[derive(Debug)]
pub struct CommitHit {
    pub at: Pinned,
    pub date: String,
    pub author: String,
    pub subject: String,
    pub diff: Option<String>,
}

#[derive(Debug)]
pub struct RepoHit {
    pub repo: RepoName,
    pub description: String,
    pub stars: u64,
}

/// One search result, in the upstream's ranked order. A directory's path
/// has no trailing `/`, and the root's is empty.
#[derive(Debug)]
pub enum Hit {
    File(FileHit),
    Directory { at: Pinned, path: String },
    Commit(CommitHit),
    Repository(RepoHit),
}

/// A filter the upstream suggests, with how many matches it would keep.
#[derive(Debug)]
pub struct Filter {
    pub kind: Term,
    pub value: String,
    pub count: u64,
    pub limit_hit: bool,
}

/// The upstream's message about a query, and the queries it proposes
/// instead. `title` and `description` are the upstream's text, untrusted.
#[derive(Debug)]
pub struct Alert {
    pub title: String,
    pub description: Option<String>,
    pub proposed: Vec<String>,
}

impl Alert {
    /// The proposed queries sego can run: those that aren't blank.
    pub fn queries(&self) -> impl Iterator<Item = &str> {
        self.proposed
            .iter()
            .map(String::as_str)
            .filter(|query| !query.trim().is_empty())
    }
}

/// What a search left out: repositories that timed out, are still cloning
/// or are missing.
#[derive(Debug, Default)]
pub struct Skipped {
    pub timed_out: Vec<String>,
    pub cloning: Vec<String>,
    pub missing: Vec<String>,
}

/// Why a search answer, one with hits or a no-repositories alert, may be
/// missing results, and `retry`, its query with a longer `timeout:` when
/// that could bring them in.
#[derive(Debug)]
pub struct CutShort {
    pub reasons: Vec<String>,
    pub retry: Option<String>,
}

/// A search's hits and summary. `approximate` is the upstream's rounded
/// count, such as `5+`, when it differs from `match_count`.
#[derive(Debug)]
pub struct SearchReport {
    pub query: String,
    pub match_count: u64,
    pub approximate: Option<String>,
    pub limit_hit: bool,
    pub skipped: Skipped,
    pub cut_short: Option<CutShort>,
    pub filters: Vec<Filter>,
    pub alert: Option<Alert>,
    pub hits: Vec<Hit>,
}

/// What a path is when a file was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotAFile {
    Directory,
    Submodule,
}

/// A command that takes a file's line range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinesCommand {
    Read,
    Blame,
}

impl LinesCommand {
    pub fn name(self) -> &'static str {
        match self {
            LinesCommand::Read => "read",
            LinesCommand::Blame => "blame",
        }
    }
}

/// The revision of a range a `RevisionNotFound` is about.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used by compare in a later task")
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Base,
    Head,
    Both,
}

/// A definitive answer that the thing asked for isn't there. Names and
/// paths are as the upstream returned them, or as the caller sent them.
/// A `RevisionNotFound` has a side only when its revision is from a range,
/// and for `Both`, `rev` is `base...head`. An `OutOfRange` carries the
/// command whose range it was, for its hint. A `NoParent` is a root commit
/// asked to be compared with its parent.
#[derive(Debug)]
pub enum Unavailable {
    RepoNotFound {
        repo: String,
    },
    RevisionNotFound {
        repo: String,
        rev: String,
        side: Option<Side>,
    },
    PathNotFound {
        at: Pinned,
        path: String,
    },
    NotAFile {
        at: Pinned,
        path: String,
        what: NotAFile,
    },
    NotADirectory {
        at: Pinned,
        path: String,
    },
    TooLarge {
        at: Pinned,
        path: String,
        byte_size: u64,
    },
    OutOfRange {
        at: Pinned,
        path: String,
        start: u32,
        total_lines: u32,
        command: LinesCommand,
    },
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "used by compare in a later task")
    )]
    NoParent {
        at: Pinned,
    },
}
