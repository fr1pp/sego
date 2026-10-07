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
    EmptyRevision(String),
    NoRepository(String),
    EmptyRangeSide(String),
    InvalidRange(String),
    InvalidRevision(String),
    InvalidRepository(String),
    LineColumn(String),
    LineZero(String),
    LineEndBeforeStart(String),
    LineForm(String),
    PullRequestUrl,
    ReleaseUrl,
    UnsupportedUrl(String),
}

impl fmt::Display for AddressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRevision(t) => {
                write!(f, "the address has an empty revision: {t}")
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
            Self::PullRequestUrl => {
                write!(f, "pull request URLs name no revision sego can read")
            }
            Self::ReleaseUrl => {
                write!(f, "release URLs name no revision sego can read")
            }
            Self::UnsupportedUrl(t) => {
                write!(f, "unsupported URL form: {t}")
            }
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
    let stray_dot = |side: &str| {
        side.contains("..") || side.starts_with('.') || side.ends_with('.')
    };
    if stray_dot(base) || stray_dot(head) {
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
    if let Some(web) = parse_web(token) {
        return web;
    }
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
            Err(AddressError::EmptyRevision(token.to_owned()))
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

fn hex_value(b: u8) -> Option<u8> {
    char::from(b).to_digit(16).and_then(|d| u8::try_from(d).ok())
}

/// An invalid escape such as `%zz` is kept literally.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) =
                (hex_value(bytes[i + 1]), hex_value(bytes[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn strip_prefix_ignore_case<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then(|| &s[prefix.len()..])
}

fn is_sha(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `10`, `10-20`, `10C5-12C8` and the same with `L` before each number.
/// Anything else is not a line selection and yields `None`.
fn selection_lines(
    selection: &str,
) -> Result<Option<LineRange>, AddressError> {
    let Some(body) = selection.strip_prefix('L') else {
        return Ok(None);
    };
    let number = |part: &str| -> Option<String> {
        let (line, column) = part.split_once('C').unwrap_or((part, "1"));
        (is_digits(line) && is_digits(column)).then(|| line.to_owned())
    };
    let normalised = match body.split_once('-') {
        None => number(body),
        Some((a, b)) => {
            let b = b.strip_prefix('L').unwrap_or(b);
            number(a).zip(number(b)).map(|(a, b)| format!("{a}-{b}"))
        }
    };
    match normalised {
        Some(range) => parse_lines(&range).map(Some),
        None => Ok(None),
    }
}

fn url_lines(
    fragment: Option<&str>,
    query: Option<&str>,
) -> Result<Option<LineRange>, AddressError> {
    if let Some(lines) = fragment.map(selection_lines).transpose()?.flatten() {
        return Ok(Some(lines));
    }
    let selection = query.and_then(|q| {
        q.split('&').find(|part| {
            part.strip_prefix('L')
                .is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()))
        })
    });
    selection.map(selection_lines).transpose().map(Option::flatten)
}

struct Web {
    rev: Rev,
    path: Option<String>,
    kind: UrlKind,
}

fn split_rev(
    token: &str,
    rest: &[&str],
    kind: UrlKind,
) -> Result<Web, AddressError> {
    let unsupported = || AddressError::UnsupportedUrl(token.to_owned());
    let segments: Vec<String> =
        rest.iter().map(|s| percent_decode(s)).collect();
    if segments.is_empty() || segments.iter().any(String::is_empty) {
        return Err(unsupported());
    }
    if is_sha(&segments[0]) {
        let path = segments[1..].join("/");
        if kind == UrlKind::Blob && path.is_empty() {
            return Err(unsupported());
        }
        return Ok(Web {
            rev: Rev::One(segments[0].clone()),
            path: Some(path),
            kind,
        });
    }
    if kind == UrlKind::Blob && segments.len() < 2 {
        return Err(unsupported());
    }
    Ok(Web { rev: Rev::Split(segments), path: None, kind })
}

fn forge_tail(
    token: &str,
    keyword: &str,
    rest: &[&str],
) -> Result<Web, AddressError> {
    let unsupported = || AddressError::UnsupportedUrl(token.to_owned());
    match keyword {
        "blob" | "raw" | "blame" => split_rev(token, rest, UrlKind::Blob),
        "tree" => split_rev(token, rest, UrlKind::Tree),
        "commit" => match rest {
            [sha] if !sha.is_empty() => Ok(Web {
                rev: Rev::One(percent_decode(sha)),
                path: None,
                kind: UrlKind::Commit,
            }),
            _ => Err(unsupported()),
        },
        "compare" => {
            let spec: Vec<String> =
                rest.iter().map(|s| percent_decode(s)).collect();
            match parse_rev(&spec.join("/"))? {
                rev @ Rev::Range { .. } => {
                    Ok(Web { rev, path: None, kind: UrlKind::Compare })
                }
                _ => Err(unsupported()),
            }
        }
        "pull" | "merge_requests" => Err(AddressError::PullRequestUrl),
        "releases" => Err(AddressError::ReleaseUrl),
        _ => Err(unsupported()),
    }
}

const GITHUB_KEYWORDS: [&str; 8] =
    ["blob", "tree", "raw", "blame", "commit", "compare", "pull", "releases"];

fn finish(
    repo: String,
    web: Result<Web, AddressError>,
    lines: Result<Option<LineRange>, AddressError>,
) -> Result<Address, AddressError> {
    let web = web?;
    Ok(Address {
        repo: Repo::Exact(repo),
        rev: web.rev,
        path: web.path,
        lines: lines?,
        kind: web.kind,
    })
}

fn has_scp_marks(segment: &str) -> bool {
    segment.contains(['@', ':'])
}

fn github_url(
    token: &str,
    segs: &[&str],
    lines: Result<Option<LineRange>, AddressError>,
) -> Option<Result<Address, AddressError>> {
    if segs.len() < 3
        || segs[..2].iter().any(|s| s.is_empty() || has_scp_marks(s))
    {
        return None;
    }
    let repo = format!(
        "github.com/{}/{}",
        percent_decode(segs[0]),
        percent_decode(segs[1])
    );
    if GITHUB_KEYWORDS.contains(&segs[2]) {
        return Some(finish(
            repo,
            forge_tail(token, segs[2], &segs[3..]),
            lines,
        ));
    }
    if segs.iter().any(|s| has_scp_marks(s)) {
        return None;
    }
    Some(Err(AddressError::UnsupportedUrl(token.to_owned())))
}

fn gitlab_url(
    token: &str,
    segs: &[&str],
    lines: Result<Option<LineRange>, AddressError>,
) -> Option<Result<Address, AddressError>> {
    let dash = segs.iter().position(|s| *s == "-")?;
    let unsupported = || AddressError::UnsupportedUrl(token.to_owned());
    let repo_segs = &segs[..dash];
    if repo_segs.len() < 2
        || repo_segs.iter().any(|s| s.is_empty() || has_scp_marks(s))
    {
        return Some(Err(unsupported()));
    }
    let names: Vec<String> =
        repo_segs.iter().map(|s| percent_decode(s)).collect();
    let repo = format!("gitlab.com/{}", names.join("/"));
    let tail = match segs.get(dash + 1) {
        Some(keyword) => forge_tail(token, keyword, &segs[dash + 2..]),
        None => Err(unsupported()),
    };
    Some(finish(repo, tail, lines))
}

fn sourcegraph_url(
    token: &str,
    segs: &[&str],
    lines: Result<Option<LineRange>, AddressError>,
) -> Option<Result<Address, AddressError>> {
    let dash = segs.iter().position(|s| *s == "-")?;
    if !segs[..dash].iter().any(|s| s.contains('@')) {
        return None;
    }
    let unsupported = || AddressError::UnsupportedUrl(token.to_owned());
    let named = if segs[0] == "r" { &segs[1..dash] } else { &segs[..dash] };
    let joined = percent_decode(&named.join("/"));
    let (repo, rev) = joined.split_once('@')?;
    if repo.is_empty() {
        return Some(Err(unsupported()));
    }
    if rev.is_empty() {
        return Some(Err(AddressError::EmptyRevision(token.to_owned())));
    }
    let rest: Vec<String> =
        segs[dash + 1..].iter().map(|s| percent_decode(s)).collect();
    let tail = match rest.split_first() {
        Some((keyword, path)) if keyword == "blob" || keyword == "tree" => {
            let path = path.join("/");
            let kind =
                if keyword == "blob" { UrlKind::Blob } else { UrlKind::Tree };
            if kind == UrlKind::Blob && path.is_empty() {
                Err(unsupported())
            } else {
                Ok(Web {
                    rev: Rev::One(rev.to_owned()),
                    path: Some(path),
                    kind,
                })
            }
        }
        Some((keyword, [sha])) if keyword == "commit" => Ok(Web {
            rev: Rev::One(sha.clone()),
            path: None,
            kind: UrlKind::Commit,
        }),
        _ => Err(unsupported()),
    };
    Some(finish(repo.to_owned(), tail, lines))
}

fn raw_githubusercontent_url(
    token: &str,
    segs: &[&str],
    lines: Result<Option<LineRange>, AddressError>,
) -> Result<Address, AddressError> {
    if segs.len() < 4 || segs[..2].iter().any(|s| s.is_empty()) {
        return Err(AddressError::UnsupportedUrl(token.to_owned()));
    }
    let repo = format!(
        "github.com/{}/{}",
        percent_decode(segs[0]),
        percent_decode(segs[1])
    );
    finish(repo, split_rev(token, &segs[2..], UrlKind::Blob), lines)
}

/// `None` when the token is not a web URL and the plain grammar applies.
fn parse_web(token: &str) -> Option<Result<Address, AddressError>> {
    let rest = strip_prefix_ignore_case(token, "https://")
        .or_else(|| strip_prefix_ignore_case(token, "http://"))
        .unwrap_or(token);
    let rest = strip_prefix_ignore_case(rest, "www.").unwrap_or(rest);
    let (rest, fragment) = match rest.split_once('#') {
        Some((a, b)) => (a, Some(b)),
        None => (rest, None),
    };
    let (rest, query) = match rest.split_once('?') {
        Some((a, b)) => (a, Some(b)),
        None => (rest, None),
    };
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let path = path.trim_end_matches('/');
    let segs: Vec<&str> =
        if path.is_empty() { Vec::new() } else { path.split('/').collect() };
    let lines = url_lines(fragment, query);
    match host.to_ascii_lowercase().as_str() {
        "github.com" => github_url(token, &segs, lines),
        "gitlab.com" => gitlab_url(token, &segs, lines),
        "sourcegraph.com" => sourcegraph_url(token, &segs, lines),
        "raw.githubusercontent.com" => {
            Some(raw_githubusercontent_url(token, &segs, lines))
        }
        _ => None,
    }
}

/// Resolves a submodule's stored URL to `host/owner/name`.
pub fn submodule_target(url: &str, parent_repo: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() || strip_prefix_ignore_case(url, "file:").is_some() {
        return None;
    }
    let location = if url.starts_with("./") || url.starts_with("../") {
        let mut segments: Vec<&str> = parent_repo.split('/').collect();
        for part in url.split('/') {
            match part {
                "." | "" => {}
                ".." => {
                    segments.pop();
                    if segments.is_empty() {
                        return None;
                    }
                }
                name => segments.push(name),
            }
        }
        segments.join("/")
    } else {
        strip_clone_url(url)
    };
    let location = location.trim_end_matches('/');
    let location = location.strip_suffix(".git").unwrap_or(location);
    let segments: Vec<&str> = location.split('/').collect();
    let valid = segments.len() >= 3
        && segments[0].contains('.')
        && segments.iter().all(|s| !s.is_empty() && !s.contains(is_bad_char));
    valid.then(|| location.to_owned())
}
