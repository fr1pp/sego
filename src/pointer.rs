//! Pointers (ADR 0003): each names the sego command or filter that gets
//! what an answer left out, or finds a name an error left the caller
//! without, worked out from what the answer holds.

use crate::address::{is_oid_like, pinned, short};
use crate::escape::shell_word;

const MIB: u64 = 1 << 20;

pub const NARROWS: &str = "repo:, lang: or file: narrows what the search reads";

/// A commit of a repository, as a pointer names it.
#[derive(Clone, Copy)]
pub struct At<'a> {
    pub repo: &'a str,
    pub commit: &'a str,
}

/// The pointers for `path`, missing at `at`: a search for the files named
/// like its last segment, and its log, which names the commit that moved
/// or deleted it.
pub fn missing_path(at: At<'_>, path: &str) -> [String; 2] {
    let log = pinned(at.repo, at.commit, Some(path));
    [files_named(at, path), format!("sego log {log} lists any commits that changed {path}")]
}

/// The search for the files named like `path`'s last segment, or under a
/// directory so named. `count:all` makes its count exact, and raises its
/// deadline to the most, which a pinned search can need.
pub fn files_named(at: At<'_>, path: &str) -> String {
    let path = path.trim_end_matches('/');
    let name = path.rsplit('/').next().unwrap_or(path);
    let commit = short(at.commit);
    let query =
        format!("repo:^{}$@{commit} file:(^|/){}(/|$) count:all", literal(at.repo), literal(name));
    format!(
        "sego search {} lists the files named {name}, or under a directory named {name}, at {commit}",
        shell_word(&query)
    )
}

/// The refs that could be the one `rev` meant, found by the release number
/// it ends in, or by `rev` itself; `None` for a revision `sego refs` can't
/// match: one that could be a commit ID, or an expression on another.
pub fn refs(repo: &str, rev: &str) -> Option<String> {
    if is_oid_like(rev) || rev.contains(['~', '^', ':']) {
        return None;
    }
    let text = release_number(rev).unwrap_or(rev);
    Some(format!(
        "sego refs {} {} lists the refs whose names contain {text}",
        shell_word(repo),
        shell_word(text)
    ))
}

/// The tree a root commit adds, which no diff gives.
pub fn root_tree(at: At<'_>) -> String {
    let tree = pinned(at.repo, at.commit, None);
    format!("sego ls --recursive --json {tree} lists every file it adds")
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

/// What reaches further for a search that stopped: a longer deadline while
/// `deadline_ms` is under `most_ms`, the most the server allows; otherwise,
/// or when the deadline is unknown, a narrower scope. `timeout` is the
/// query's own `timeout:` filter, which the server takes only once.
pub fn stopped(deadline_ms: Option<u64>, most_ms: u64, timeout: Option<&str>) -> String {
    let most = most_ms / 1000;
    match (deadline_ms, timeout) {
        (Some(ms), _) if ms >= most_ms => NARROWS.to_owned(),
        (None, _) => NARROWS.to_owned(),
        (Some(_), Some(timeout)) => {
            format!("{timeout} sets its deadline; sourcegraph.com allows up to timeout:{most}s")
        }
        (Some(ms), None) => format!(
            "timeout:{most}s raises its {} s deadline to {most} s, the most sourcegraph.com allows",
            ms / 1000
        ),
    }
}

/// What reaches the files a search skipped: `index:no` the `dense` ones
/// with too many trigrams, unless the query has an `index:` already, and
/// `sego show` those of the sizes `over`, over 1 MiB, that a response of at
/// most `largest` bytes can hold.
pub fn unindexed(dense: usize, over: &[u64], index_written: bool, largest: u64) -> Vec<String> {
    let mut pointers = Vec::new();
    if dense > 0 && !index_written {
        pointers.push(format!(
            "index:no searches the {} with too many distinct trigrams",
            files(dense)
        ));
    }
    let readable = over.iter().filter(|&&bytes| bytes < largest).count();
    if readable > 0 {
        let under = if readable == over.len() {
            String::new()
        } else {
            format!(" and under {} MiB", largest / MIB)
        };
        pointers.push(format!(
            "sego show reads the {} over 1 MiB{under}, which no search reaches",
            files(readable)
        ));
    }
    pointers
}

fn files(n: usize) -> String {
    if n == 1 { "1 file".to_owned() } else { format!("{n} files") }
}

/// The release number `rev` ends in, such as `1.28` in `release-1.28`.
fn release_number(rev: &str) -> Option<&str> {
    let prefix = rev.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    let tail = rev[prefix.len()..].trim_start_matches('.');
    let parts: Vec<&str> = tail.split('.').collect();
    (parts.len() > 1 && parts.iter().all(|part| !part.is_empty())).then_some(tail)
}

/// `text` as an RE2 pattern that matches it literally, a space escaped as
/// the query language reads it.
fn literal(text: &str) -> String {
    let mut pattern = String::with_capacity(text.len());
    for c in text.chars() {
        if r"\.+*?()|[]{}^$ ".contains(c) {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: At<'static> = At { repo: "github.com/a/b", commit: "0123456789abcdef" };

    #[test]
    fn a_revision_ending_in_a_release_number_is_found_by_it() {
        let text = |rev| refs("r", rev).map(|line| line.split(' ').nth(3).unwrap_or("").to_owned());
        assert_eq!(text("release/1.28").as_deref(), Some("1.28"));
        assert_eq!(text("release-8.0").as_deref(), Some("8.0"));
        assert_eq!(text("v1.8").as_deref(), Some("1.8"));
        assert_eq!(text("1.8.0").as_deref(), Some("1.8.0"));
        assert_eq!(text("go1.22.3").as_deref(), Some("1.22.3"));
        assert_eq!(text("master").as_deref(), Some("master"));
        assert_eq!(text("v1").as_deref(), Some("v1"));
        assert_eq!(text("v1.").as_deref(), Some("v1."));
        assert_eq!(text("deadbeef"), None);
        assert_eq!(text("main~3"), None);
        assert_eq!(text("v1^"), None);
    }

    #[test]
    fn a_name_is_matched_literally() {
        assert_eq!(literal(r"[id].tsx a+b c\d"), r"\[id\]\.tsx\ a\+b\ c\\d");
        let line = files_named(AT, "src/[id].tsx");
        let want = r"sego search 'repo:^github\.com/a/b$@0123456789ab file:(^|/)\[id\]\.tsx(/|$) count:all'";
        assert!(line.starts_with(want), "{line}");
    }

    #[test]
    fn a_directory_written_with_a_trailing_slash_is_found_by_its_name() {
        let line = files_named(AT, "src/nope/");
        assert!(line.contains(r"file:(^|/)nope(/|$)"), "{line}");
        assert!(line.contains("the files named nope,"), "{line}");
    }

    #[test]
    fn a_stopped_search_is_pointed_to_a_longer_deadline_until_the_most() {
        assert_eq!(
            stopped(Some(20_000), 60_000, None),
            "timeout:60s raises its 20 s deadline to 60 s, the most sourcegraph.com allows"
        );
        assert_eq!(
            stopped(Some(5000), 60_000, Some("timeout:5s")),
            "timeout:5s sets its deadline; sourcegraph.com allows up to timeout:60s"
        );
        assert_eq!(stopped(Some(60_000), 60_000, None), NARROWS);
        assert_eq!(stopped(None, 60_000, None), NARROWS);
    }
}
