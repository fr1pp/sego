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
