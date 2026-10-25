use std::fmt::Write;
use std::path::{Path, PathBuf};

use ring::digest::{SHA256, digest};

#[derive(Clone, Copy, Debug)]
pub struct Key<'a> {
    pub endpoint: &'a str,
    pub repo: &'a str,
    pub commit: &'a str,
    pub path: &'a str,
}

impl Key<'_> {
    /// 64 lowercase hex digits. Each part is hashed after its byte length,
    /// so that no two keys share a byte stream.
    pub fn name(&self) -> String {
        let mut bytes = Vec::new();
        for part in [self.endpoint, self.repo, self.commit, self.path] {
            bytes.extend_from_slice(&(part.len() as u64).to_be_bytes());
            bytes.extend_from_slice(part.as_bytes());
        }
        digest(&SHA256, &bytes).as_ref().iter().fold(
            String::with_capacity(64),
            |mut hex, byte| {
                let _ = write!(hex, "{byte:02x}");
                hex
            },
        )
    }
}

pub fn entry_path(root: &Path, name: &str) -> PathBuf {
    let (fan, rest) = name.split_at(2);
    root.join("raw").join(fan).join(rest)
}
