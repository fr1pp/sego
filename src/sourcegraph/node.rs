//! A path at a commit, as the `__typename`-tagged node the upstream gives
//! for it, and the file it resolves to when a file was asked for.

use serde::Deserialize;

use crate::address::{Pinned, RepoPath};
use crate::domain::{NotAFile, Unavailable};

#[derive(Deserialize)]
#[serde(tag = "__typename", deny_unknown_fields)]
pub enum Node {
    #[serde(rename_all = "camelCase")]
    GitBlob {
        path: String,
        byte_size: u64,
        binary: bool,
        content_type: String,
        submodule: Option<Submodule>,
    },
    GitTree {
        path: String,
    },
}

/// Only its presence matters here.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Submodule {
    #[serde(rename = "url")]
    _url: String,
    #[serde(rename = "commit")]
    _commit: String,
}

/// A file's metadata, with `path` as the upstream returned it.
pub struct Blob {
    pub path: String,
    pub byte_size: u64,
    pub binary: bool,
    pub content_type: String,
}

/// The file `node` is at `at`, or why there is none: no node means no such
/// path as `requested`; a directory or a submodule isn't a file.
pub fn blob(
    at: &Pinned,
    requested: &RepoPath,
    node: Option<Node>,
) -> Result<Blob, Unavailable> {
    let at = || at.clone();
    match node {
        None => {
            let path = requested.upstream().to_owned();
            Err(Unavailable::PathNotFound { at: at(), path })
        }
        Some(Node::GitTree { path }) => {
            let what = NotAFile::Directory;
            Err(Unavailable::NotAFile { at: at(), path, what })
        }
        Some(Node::GitBlob { path, submodule: Some(_), .. }) => {
            let what = NotAFile::Submodule;
            Err(Unavailable::NotAFile { at: at(), path, what })
        }
        Some(Node::GitBlob {
            path,
            byte_size,
            binary,
            content_type,
            submodule: None,
        }) => Ok(Blob { path, byte_size, binary, content_type }),
    }
}
