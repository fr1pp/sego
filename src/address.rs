use std::fmt;

use crate::quote::shell_word;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Repo {
    Exact(String),
    /// Holds `owner/name`, with no host.
    OwnerName(String),
}

impl Repo {
    pub fn candidates(&self) -> Vec<String> {
        match self {
            Repo::Exact(name) => vec![name.clone()],
            Repo::OwnerName(name) => {
                vec![
                    format!("github.com/{name}"),
                    format!("gitlab.com/{name}"),
                ]
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dots {
    Two,
    Three,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rev {
    Default,
    One(String),
    Range {
        base: String,
        head: String,
        dots: Dots,
    },
    /// URL segments after blob/tree/raw/blame, where the ref and the path
    /// are not yet told apart.
    Split(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlKind {
    Plain,
    Blob,
    Tree,
    Commit,
    Compare,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    pub start: u32,
    pub end: End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    Line(u32),
    Open,
}

impl LineRange {
    pub fn display(&self) -> String {
        match self.end {
            End::Line(end) if end == self.start => end.to_string(),
            End::Line(end) => format!("{}-{end}", self.start),
            End::Open => format!("{}-", self.start),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub repo: Repo,
    pub rev: Rev,
    /// `None` when the address has no `:`; `Some("")` is the root.
    pub path: Option<String>,
    pub lines: Option<LineRange>,
    pub kind: UrlKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressError {
    EndsInAt(String),
    NoRepository(String),
    EmptyRangeSide(String),
    InvalidRange(String),
    InvalidRevision(String),
    InvalidRepository(String),
    LineColumn(String),
    LineZero(String),
    LineEndBeforeStart(String),
    LineForm(String),
}

impl fmt::Display for AddressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EndsInAt(t) => {
                write!(f, "the address ends in @ with no revision: {t}")
            }
            Self::NoRepository(t) => {
                write!(f, "the address has no repository: {t}")
            }
            Self::EmptyRangeSide(r) => {
                write!(f, "the range {r} has an empty side")
            }
            Self::InvalidRange(r) => {
                write!(f, "invalid range {r}; a range is A..B or A...B")
            }
            Self::InvalidRevision(r) => write!(
                f,
                "invalid revision '{r}'; a revision cannot start with - or \
                 contain whitespace or control characters"
            ),
            Self::InvalidRepository(n) => {
                write!(f, "invalid repository name '{n}'")
            }
            Self::LineColumn(s) => write!(
                f,
                "invalid line range {s}; N:M reads as line and column, a \
                 range is N-M"
            ),
            Self::LineZero(s) => {
                write!(f, "invalid line range {s}; lines are numbered from 1")
            }
            Self::LineEndBeforeStart(s) => write!(
                f,
                "invalid line range {s}; the end is before the start"
            ),
            Self::LineForm(s) => write!(
                f,
                "invalid line range {s}; lines are N, N-M, N,M or N-"
            ),
        }
    }
}

impl std::error::Error for AddressError {}

pub fn pinned(repo: &str, commit: &str, path: Option<&str>) -> String {
    let short: String = commit.chars().take(12).collect();
    let mut address = format!("{repo}@{short}");
    if let Some(path) = path {
        address.push(':');
        address.push_str(path);
    }
    shell_word(&address).into_owned()
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn line_number(s: &str) -> Option<u32> {
    if !is_digits(s) {
        return None;
    }
    s.parse::<u32>().ok().filter(|&n| i32::try_from(n).is_ok())
}

pub fn parse_lines(s: &str) -> Result<LineRange, AddressError> {
    let form = || AddressError::LineForm(s.to_owned());
    if let Some((a, b)) = s.split_once(':')
        && is_digits(a)
        && is_digits(b)
    {
        return Err(AddressError::LineColumn(s.to_owned()));
    }
    let (start, end) = match s.find(['-', ',']) {
        None => {
            let n = line_number(s).ok_or_else(form)?;
            (n, End::Line(n))
        }
        Some(i) => {
            let start = line_number(&s[..i]).ok_or_else(form)?;
            let tail = &s[i + 1..];
            if tail.is_empty() && s.as_bytes()[i] == b'-' {
                (start, End::Open)
            } else {
                (start, End::Line(line_number(tail).ok_or_else(form)?))
            }
        }
    };
    if start == 0 || end == End::Line(0) {
        return Err(AddressError::LineZero(s.to_owned()));
    }
    if let End::Line(end) = end
        && end < start
    {
        return Err(AddressError::LineEndBeforeStart(s.to_owned()));
    }
    Ok(LineRange { start, end })
}

fn is_bad_char(c: char) -> bool {
    c.is_whitespace() || c.is_control()
}

fn check_revision(rev: &str) -> Result<(), AddressError> {
    if rev.starts_with('-') || rev.chars().any(is_bad_char) {
        return Err(AddressError::InvalidRevision(rev.to_owned()));
    }
    Ok(())
}

fn parse_rev(rev: &str) -> Result<Rev, AddressError> {
    let split = rev
        .split_once("...")
        .map(|(a, b)| (a, b, Dots::Three))
        .or_else(|| rev.split_once("..").map(|(a, b)| (a, b, Dots::Two)));
    let Some((base, head, dots)) = split else {
        check_revision(rev)?;
        return Ok(Rev::One(rev.to_owned()));
    };
    if base.is_empty() || head.is_empty() {
        return Err(AddressError::EmptyRangeSide(rev.to_owned()));
    }
    if base.contains("..") || head.contains("..") {
        return Err(AddressError::InvalidRange(rev.to_owned()));
    }
    check_revision(base)?;
    check_revision(head)?;
    Ok(Rev::Range { base: base.to_owned(), head: head.to_owned(), dots })
}

fn all_digits_then_slash_or_end(s: &str) -> bool {
    let end = s.find('/').unwrap_or(s.len());
    is_digits(&s[..end])
}

/// Removes the clone-URL parts (scheme, userinfo, port, scp `host:`) from
/// the front of the token, leaving `host/path...` or the token unchanged.
fn strip_clone_url(token: &str) -> String {
    let (rest, has_scheme) = match token.split_once("://") {
        Some((scheme, rest))
            if !scheme.is_empty()
                && scheme.chars().all(|c| {
                    c.is_ascii_alphanumeric() || "+-.".contains(c)
                }) =>
        {
            (rest, true)
        }
        _ => (token, false),
    };
    let first_end = rest.find('/').unwrap_or(rest.len());
    let first = &rest[..first_end];
    let mut host_part = rest;
    if let Some(at) = first.find('@') {
        let user = &first[..at];
        let after = &first[at + 1..];
        let host_end = after.find([':', '@']).unwrap_or(after.len());
        let followed = after[host_end..].starts_with(':')
            || host_end == after.len() && first_end < rest.len();
        let userinfo_ok = if has_scheme {
            true
        } else {
            !user.is_empty()
                && !user.contains('.')
                && after[..host_end].contains('.')
                && followed
        };
        if userinfo_ok {
            host_part = &rest[at + 1..];
        }
    }
    let host_end = host_part.find(['/', ':', '@']).unwrap_or(host_part.len());
    let host = &host_part[..host_end];
    if !host.contains('.') {
        return token.to_owned();
    }
    let after = &host_part[host_end..];
    match after.strip_prefix(':') {
        Some(tail) if all_digits_then_slash_or_end(tail) => {
            let slash = tail.find('/').unwrap_or(tail.len());
            format!("{host}{}", &tail[slash..])
        }
        Some(tail) => format!("{host}/{}", tail.trim_start_matches('/')),
        None => format!("{host}{after}"),
    }
}

fn parse_repo(repo: &str, token: &str) -> Result<Repo, AddressError> {
    let repo = repo.trim_end_matches('/');
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    let repo = repo.trim_end_matches('/');
    if repo.is_empty() {
        return Err(AddressError::NoRepository(token.to_owned()));
    }
    if repo.chars().any(is_bad_char) {
        return Err(AddressError::InvalidRepository(repo.to_owned()));
    }
    let segments: Vec<&str> = repo.split('/').collect();
    if segments.iter().any(|s| s.is_empty()) {
        return Err(AddressError::InvalidRepository(repo.to_owned()));
    }
    if segments[0].contains('.') {
        return Ok(Repo::Exact(repo.to_owned()));
    }
    match segments.len() {
        1 => Err(AddressError::InvalidRepository(repo.to_owned())),
        2 => Ok(Repo::OwnerName(repo.to_owned())),
        _ => Ok(Repo::Exact(format!("gitlab.com/{repo}"))),
    }
}

fn parse_path(path: &str) -> String {
    let path = path.trim_matches('/');
    if path == "." { String::new() } else { path.to_owned() }
}

pub fn parse(token: &str) -> Result<Address, AddressError> {
    let stripped = strip_clone_url(token);
    let repo_end = stripped.find(['@', ':']).unwrap_or(stripped.len());
    let repo = parse_repo(&stripped[..repo_end], token);
    let rest = &stripped[repo_end..];
    let rev_and_path = if let Some(after_at) = rest.strip_prefix('@') {
        let (rev, path) = match after_at.split_once(':') {
            Some((rev, path)) => (rev, Some(path)),
            None => (after_at, None),
        };
        if rev.is_empty() {
            Err(AddressError::EndsInAt(token.to_owned()))
        } else {
            parse_rev(rev).map(|rev| (rev, path))
        }
    } else {
        Ok((Rev::Default, rest.strip_prefix(':')))
    };
    let (repo, (rev, path)) = match (repo, rev_and_path) {
        (Ok(repo), Ok(rev_and_path)) => (repo, rev_and_path),
        (Err(AddressError::NoRepository(_)), Err(e))
            if matches!(
                e,
                AddressError::EmptyRangeSide(_)
                    | AddressError::InvalidRange(_)
            ) =>
        {
            return Err(e);
        }
        (Err(e), _) | (_, Err(e)) => return Err(e),
    };
    Ok(Address {
        repo,
        rev,
        path: path.map(parse_path),
        lines: None,
        kind: UrlKind::Plain,
    })
}
