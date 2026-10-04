use crate::address::Pinned;

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

/// A directory's entries in the upstream's order: every directory, then
/// every file and submodule, each group in path order. A listing cut at the
/// upstream's limit is complete only up to `truncated_after`, the path of
/// the last entry the upstream returned, in that order.
#[derive(Debug)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "list answers with it in a later task")
)]
pub struct Listing {
    pub at: Pinned,
    pub path: String,
    pub entries: Vec<Entry>,
    pub truncated_after: Option<String>,
}

/// A path in a listing, from the repository's root.
#[derive(Debug)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "list answers with it in a later task")
)]
pub struct Entry {
    pub path: String,
    pub kind: EntryKind,
}

#[derive(Debug)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "list answers with it in a later task")
)]
pub enum EntryKind {
    Directory,
    File,
    /// The repository and commit the submodule pins.
    Submodule(Pinned),
}

/// A submodule asked for as a directory: where it points.
#[derive(Debug)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "list answers with it in a later task")
)]
pub struct Submodule {
    pub at: Pinned,
    pub path: String,
    pub target: Pinned,
}

/// What a directory path answers with.
#[derive(Debug)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "list answers with it in a later task")
)]
pub enum Tree {
    Listing(Listing),
    Submodule(Submodule),
}

/// What a path is when a file was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotAFile {
    Directory,
    Submodule,
}

/// A definitive answer that the thing asked for isn't there. Names and
/// paths are as the upstream returned them, or as the caller sent them.
#[derive(Debug)]
pub enum Unavailable {
    RepoNotFound { repo: String },
    RevisionNotFound { repo: String, rev: String },
    PathNotFound { at: Pinned, path: String },
    NotAFile { at: Pinned, path: String, what: NotAFile },
    NotADirectory { at: Pinned, path: String },
    TooLarge { at: Pinned, path: String, byte_size: u64 },
    OutOfRange { at: Pinned, path: String, start: u32, total_lines: u32 },
}
