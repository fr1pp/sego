//! Pointers (ADR 0003): each names the sego command that gets the rest of
//! an answer sego cut.

use crate::address::{pinned, short};
use crate::escape::shell_word;

/// A commit of a repository, as a pointer names it.
#[derive(Clone, Copy)]
pub struct At<'a> {
    pub repo: &'a str,
    pub commit: &'a str,
}

/// The log from `oldest`, the last commit shown, when it holds exactly the
/// commits a cut log left out.
pub fn log_continues(oldest: At<'_>) -> String {
    format!(
        "sego log {} continues from the last commit shown",
        pinned(oldest.repo, oldest.commit, None)
    )
}

/// The two logs that together hold what a log of `head` cut at `oldest`
/// left out: the one from `oldest`, and the range from it to `head`.
pub fn log_rest(head: At<'_>, oldest: &str, path: Option<&str>) -> String {
    let path = path.map(|path| path.trim_end_matches('/'));
    let from = pinned(head.repo, oldest, path);
    let range = format!("{}@{}..{}", head.repo, short(oldest), short(head.commit));
    let mut range = shell_word(&range).into_owned();
    if let Some(path) = path {
        range = format!("{range} {}", shell_word(path));
    }
    format!(
        "sego log {from} and sego log --json {range} together list every commit this log left out"
    )
}
