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
