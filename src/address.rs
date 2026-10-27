mod url;

use std::fmt;

use crate::escape::shell_word;

use url::parse_web;
pub use url::submodule_repository;

const MIN_ABBREVIATION: usize = 4;
pub const FULL_OID: usize = 40;
const SHORT_OID: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Repo {
    Exact(String),
    /// Holds `owner/name`, with no host.
    OwnerName(String),
}

impl Repo {
    /// The name as the address wrote it.
    pub fn as_written(&self) -> &str {
        match self {
            Repo::Exact(name) | Repo::OwnerName(name) => name,
        }
    }

    pub fn candidates(&self) -> Vec<String> {
        match self {
            Repo::Exact(name) => vec![name.clone()],
            Repo::OwnerName(name) => {
                vec![format!("github.com/{name}"), format!("gitlab.com/{name}")]
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dots {
    Two,
    Three,
}

impl Dots {
    pub fn as_str(self) -> &'static str {
        match self {
            Dots::Two => "..",
            Dots::Three => "...",
        }
    }
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

/// What `show` and `ls` read in a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A revision, or the default branch for `None`, and a path.
    At { rev: Option<String>, path: Option<String> },
    /// The segments after `blob/` or `tree/` in a web URL, where the ref and
    /// the path are not yet told apart.
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
    LeavesRoot(String),
    Reflog,
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
            Self::LineEndBeforeStart(s) => {
                write!(f, "invalid line range {s}; the end is before the start")
            }
            Self::LineForm(s) => write!(f, "invalid line range {s}; lines are N, N-M, N,M, or N-"),
            Self::PullRequestUrl => {
                write!(f, "pull request URLs name no revision sego can read")
            }
            Self::ReleaseUrl => {
                write!(f, "release URLs name no revision sego can read")
            }
            Self::UnsupportedUrl(t) => {
                write!(f, "unsupported URL form: {t}")
            }
            Self::LeavesRoot(p) => write!(f, "the path {p} leaves the repository's root"),
            Self::Reflog => write!(
                f,
                "sego can't read a reflog or upstream revision: Sourcegraph's mirror has none"
            ),
        }
    }
}

impl std::error::Error for AddressError {}

/// A repository at a commit, with an optional path, escaped and quoted to be
/// printed as an address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pinned(String);

impl Pinned {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Pinned {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The first 12 characters of a commit id, as addresses print it.
pub fn short(commit: &str) -> String {
    commit.chars().take(SHORT_OID).collect()
}

/// Git reads a leading `:` as pathspec magic, and `*`, `?` and `[` as globs;
/// `path(path:)` takes none of them literally.
pub fn is_plain_path(path: &str) -> bool {
    !path.starts_with(':') && !path.contains(['*', '?', '['])
}

/// Whether `rev` is 4 to 40 hex digits, so it may be an abbreviated or a
/// full commit id.
pub fn is_oid_like(rev: &str) -> bool {
    (MIN_ABBREVIATION..=FULL_OID).contains(&rev.len()) && rev.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Whether `rev`, as written, was a name rather than `commit` written in
/// full or in part: whether `commit` doesn't start with it, ignoring case.
pub fn is_name(rev: &str, commit: &str) -> bool {
    commit.get(..rev.len()).is_none_or(|start| !start.eq_ignore_ascii_case(rev))
}

/// The header field for a revision that was a name, not `commit` written in
/// full or in part.
pub fn rev_field(rev: Option<&String>, commit: &str) -> Option<String> {
    let rev = rev?;
    is_name(rev, commit).then(|| format!("rev {rev}"))
}

/// The header field for a range of which a side was a name, each side given
/// as written and with its commit, with the range's `dots`.
pub fn range_rev_field(
    (base, base_commit): (&str, &str),
    dots: &str,
    (head, head_commit): (&str, &str),
) -> Option<String> {
    (is_name(base, base_commit) || is_name(head, head_commit))
        .then(|| format!("rev {base}{dots}{head}"))
}

pub fn pinned(repo: &str, commit: &str, path: Option<&str>) -> Pinned {
    let short = short(commit);
    let mut address = format!("{repo}@{short}");
    if let Some(path) = path {
        address.push(':');
        address.push_str(path);
    }
    Pinned(shell_word(&address).into_owned())
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

pub fn valid_revision(rev: &str) -> bool {
    check_revision(rev).is_ok()
}

fn check_revision(rev: &str) -> Result<(), AddressError> {
    if rev.starts_with('-') || rev.chars().any(is_bad_char) {
        return Err(AddressError::InvalidRevision(rev.to_owned()));
    }
    if rev.contains("@{") {
        return Err(AddressError::Reflog);
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
    let stray_dot =
        |side: &str| side.contains("..") || side.starts_with('.') || side.ends_with('.');
    if stray_dot(base) || stray_dot(head) {
        return Err(AddressError::InvalidRange(rev.to_owned()));
    }
    check_revision(base)?;
    check_revision(head)?;
    Ok(Rev::Range { base: base.to_owned(), head: head.to_owned(), dots })
}

/// A commit URL names one commit, so a range in its place is unsupported.
fn commit_rev(token: &str, sha: &str) -> Result<Rev, AddressError> {
    if sha.contains("..") {
        return Err(AddressError::UnsupportedUrl(token.to_owned()));
    }
    parse_rev(sha)
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
                && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) =>
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
        let followed =
            after[host_end..].starts_with(':') || host_end == after.len() && first_end < rest.len();
        let userinfo_ok = if has_scheme {
            true
        } else {
            !user.is_empty() && !user.contains('.') && after[..host_end].contains('.') && followed
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

/// The path from the repository's root: `.` and empty segments dropped and
/// each `x/..` resolved, so `""` is the root. A path that leaves the root
/// names nothing in a mirror.
pub fn parse_path(path: &str) -> Result<String, AddressError> {
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop().ok_or_else(|| AddressError::LeavesRoot(path.to_owned()))?;
            }
            _ => segments.push(segment),
        }
    }
    Ok(segments.join("/"))
}

/// A path operand of `log` or `diff`: a plain path as `parse_path` reads it,
/// keeping a trailing `/`, which limits it to a directory; a glob or pathspec
/// magic as written, since git reads it.
pub fn parse_operand(path: &str) -> Result<String, AddressError> {
    if !is_plain_path(path) {
        return Ok(path.to_owned());
    }
    let normalised = parse_path(path)?;
    if path.ends_with('/') && !normalised.is_empty() {
        Ok(format!("{normalised}/"))
    } else {
        Ok(normalised)
    }
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
            if matches!(e, AddressError::EmptyRangeSide(_) | AddressError::InvalidRange(_)) =>
        {
            return Err(e);
        }
        (Err(e), _) | (_, Err(e)) => return Err(e),
    };
    let path = path.map(parse_path).transpose()?;
    Ok(Address { repo, rev, path, lines: None, kind: UrlKind::Plain })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "98cb6bf42e086f6af920b965c38cacc07402d51b";

    fn exact(s: &str) -> Repo {
        Repo::Exact(s.to_owned())
    }

    fn one(s: &str) -> Rev {
        Rev::One(s.to_owned())
    }

    fn ok(token: &str) -> Address {
        parse(token).unwrap_or_else(|e| panic!("{token}: {e}"))
    }

    #[test]
    fn the_rev_field_names_a_revision_unless_the_commit_starts_with_it() {
        assert_eq!(rev_field(Some(&"main".to_owned()), SHA), Some("rev main".to_owned()));
        assert_eq!(rev_field(Some(&"2024".to_owned()), SHA), Some("rev 2024".to_owned()));
        assert_eq!(rev_field(Some(&"98CB6BF4".to_owned()), SHA), None);
        assert_eq!(rev_field(None, SHA), None);
        let other = "a".repeat(40);
        assert_eq!(range_rev_field(("98cb6bf4", SHA), "..", ("aaaa", &other)), None,);
        assert_eq!(
            range_rev_field(("v1", SHA), "..", ("aaaa", &other)),
            Some("rev v1..aaaa".to_owned())
        );
    }

    #[test]
    fn a_revision_is_a_name_unless_the_commit_starts_with_it() {
        for rev in ["main", "v1.8.0", "2024", "HEAD", "98cb6bf4~1"] {
            assert!(is_name(rev, SHA), "{rev}");
        }
        for rev in ["98cb6bf4", "98CB6BF42E08", SHA] {
            assert!(!is_name(rev, SHA), "{rev}");
        }
    }

    #[test]
    fn a_path_is_normalised_and_may_not_leave_the_root() {
        for (written, normalised) in [
            ("a/./b", "a/b"),
            ("a//b/", "a/b"),
            ("pkg/x/../brew", "pkg/brew"),
            (".", ""),
            ("", ""),
            ("/a/", "a"),
            ("a/..", ""),
            ("*.go", "*.go"),
        ] {
            assert_eq!(parse_path(written), Ok(normalised.to_owned()), "{written}");
        }
        for written in ["..", "../mux.go", "a/../../b"] {
            assert_eq!(parse_path(written), Err(AddressError::LeavesRoot(written.to_owned())));
        }
        assert_eq!(ok("github.com/x/y@v1:pkg/x/../brew").path.as_deref(), Some("pkg/brew"));
        assert_eq!(ok("github.com/x/y@v1:.").path.as_deref(), Some(""));
        assert_eq!(ok("github.com/x/y@v1:").path.as_deref(), Some(""));
    }

    #[test]
    fn a_path_operand_keeps_its_trailing_slash_and_a_glob_or_pathspec_is_sent_as_given() {
        for (written, normalised) in [
            ("./pkg/", "pkg/"),
            ("pkg/x/../brew", "pkg/brew"),
            ("a//b", "a/b"),
            ("./*.go", "./*.go"),
            (":(glob)**/x", ":(glob)**/x"),
            (".", ""),
        ] {
            assert_eq!(parse_operand(written), Ok(normalised.to_owned()), "{written}");
        }
        assert_eq!(
            parse_operand("../mux.go"),
            Err(AddressError::LeavesRoot("../mux.go".to_owned()))
        );
    }

    #[test]
    fn a_reflog_or_upstream_revision_is_refused() {
        for token in [
            "github.com/gorilla/mux@@{upstream}:mux.go",
            "github.com/gorilla/mux@main@{1}",
            "github.com/gorilla/mux@v1@{u}...v2",
        ] {
            assert_eq!(parse(token), Err(AddressError::Reflog), "{token}");
        }
        assert_eq!(
            AddressError::Reflog.to_string(),
            "sego can't read a reflog or upstream revision: Sourcegraph's mirror has none"
        );
        assert!(!valid_revision("main@{1}"));
    }

    #[test]
    fn plain_and_ssh_repositories_resolve_to_a_host_qualified_name() {
        let cases = [
            ("github.com/gorilla/mux", exact("github.com/gorilla/mux"), Rev::Default),
            ("GitHub.com/Gorilla/Mux", exact("GitHub.com/Gorilla/Mux"), Rev::Default),
            ("gorilla/mux", Repo::OwnerName("gorilla/mux".into()), Rev::Default),
            (
                "gitlab-org/charts/gitlab",
                exact("gitlab.com/gitlab-org/charts/gitlab"),
                Rev::Default,
            ),
            ("src.fedoraproject.org/rpms/x", exact("src.fedoraproject.org/rpms/x"), Rev::Default),
            (
                "git@github.com:gorilla/mux.git@v1.7.0",
                exact("github.com/gorilla/mux"),
                one("v1.7.0"),
            ),
            ("https://u:pw@github.com:443/x/y.git", exact("github.com/x/y"), Rev::Default),
            ("ssh://git@github.com:22/x/y", exact("github.com/x/y"), Rev::Default),
            ("https://github.com/x/y/", exact("github.com/x/y"), Rev::Default),
            ("github.com/x/y.git@v1", exact("github.com/x/y"), one("v1")),
        ];
        for (token, repo, rev) in cases {
            let address = ok(token);
            assert_eq!(address.repo, repo, "{token}");
            assert_eq!(address.rev, rev, "{token}");
            assert_eq!(address.lines, None, "{token}");
            assert_eq!(address.kind, UrlKind::Plain, "{token}");
        }
    }

    #[test]
    fn a_revision_and_a_path_follow_the_repository() {
        let cases = [
            ("github.com/x/y@feature/x:src/a.go", one("feature/x"), Some("src/a.go")),
            ("github.com/x/y:dir/a@b:c.txt", Rev::Default, Some("dir/a@b:c.txt")),
            ("github.com/x/y@v1:/src/", one("v1"), Some("src")),
            ("github.com/x/y:.", Rev::Default, Some("")),
            ("github.com/x/y:", Rev::Default, Some("")),
            ("github.com/x/y@v1:", one("v1"), Some("")),
            ("github.com/x/y@HEAD~3:a", one("HEAD~3"), Some("a")),
            ("github.com/x/y", Rev::Default, None),
        ];
        for (token, rev, path) in cases {
            let address = ok(token);
            assert_eq!(address.rev, rev, "{token}");
            assert_eq!(address.path.as_deref(), path, "{token}");
        }
    }

    #[test]
    fn the_root_is_written_three_ways() {
        for token in ["github.com/x/y@v1", "github.com/x/y@v1:", "github.com/x/y@v1:."] {
            let address = ok(token);
            assert_eq!(address.rev, one("v1"), "{token}");
            assert_eq!(address.path.unwrap_or_default(), "", "{token}");
        }
    }

    #[test]
    fn a_pinned_address_with_a_spaced_path_parses_back() {
        let printed = pinned("github.com/x/y", SHA, Some("a b/c.go")).to_string();
        assert_eq!(printed, "'github.com/x/y@98cb6bf42e08:a b/c.go'");
        let address = ok(printed.trim_matches('\''));
        assert_eq!(address.repo, exact("github.com/x/y"));
        assert_eq!(address.rev, one("98cb6bf42e08"));
        assert_eq!(address.path.as_deref(), Some("a b/c.go"));
    }

    #[test]
    fn two_and_three_dot_ranges_parse_and_malformed_ranges_are_rejected() {
        let range = |base: &str, head: &str, dots| Rev::Range {
            base: base.into(),
            head: head.into(),
            dots,
        };
        let cases = [
            ("github.com/x/y@v1..v2", range("v1", "v2", Dots::Two)),
            ("github.com/x/y@v1...v2", range("v1", "v2", Dots::Three)),
        ];
        for (token, rev) in cases {
            assert_eq!(ok(token).rev, rev, "{token}");
        }
        let errors = [
            ("@...v2", "the range ...v2 has an empty side"),
            ("@v1..", "the range v1.. has an empty side"),
            ("@...", "the range ... has an empty side"),
            ("@a...b..c", "invalid range a...b..c; a range is A..B or A...B"),
            ("@a....b", "invalid range a....b; a range is A..B or A...B"),
            ("@a....", "invalid range a....; a range is A..B or A...B"),
        ];
        for (token, message) in errors {
            assert_eq!(parse(token).unwrap_err().to_string(), message, "{token}");
        }
    }

    #[test]
    fn addresses_that_cannot_name_a_repository_or_revision_are_rejected() {
        let cases = [
            ("github.com/x/y@", "the address has an empty revision: github.com/x/y@"),
            ("github.com/x/y@:src", "the address has an empty revision: github.com/x/y@:src"),
            ("@v1", "the address has no repository: @v1"),
            (
                "github.com/x/y@-x",
                "invalid revision '-x'; a revision cannot start with - or \
             contain whitespace or control characters",
            ),
            (
                "github.com/x/y@a b",
                "invalid revision 'a b'; a revision cannot start with - or \
             contain whitespace or control characters",
            ),
            (
                "github.com/x/y@a\u{1}b",
                "invalid revision 'a\u{1}b'; a revision cannot start with - or \
             contain whitespace or control characters",
            ),
            ("github.com/x/a\u{1b}b", "invalid repository name 'github.com/x/a\u{1b}b'"),
            ("github.com/x /y", "invalid repository name 'github.com/x /y'"),
        ];
        for (token, message) in cases {
            assert_eq!(parse(token).unwrap_err().to_string(), message, "{token}");
        }
    }

    #[test]
    fn empty_side_wins_over_no_repository() {
        assert_eq!(
            parse("github.com/x/y@...").unwrap_err(),
            AddressError::EmptyRangeSide("...".into())
        );
    }

    #[test]
    fn lines_parse_and_malformed_lines_are_rejected() {
        let range = |start, end| LineRange { start, end };
        let cases = [
            ("10", range(10, End::Line(10)), "10"),
            ("10-20", range(10, End::Line(20)), "10-20"),
            ("10,20", range(10, End::Line(20)), "10-20"),
            ("10-", range(10, End::Open), "10-"),
            ("2147483647", range(2_147_483_647, End::Line(2_147_483_647)), "2147483647"),
        ];
        for (s, expected, shown) in cases {
            let lines = parse_lines(s).unwrap();
            assert_eq!(lines, expected, "{s}");
            assert_eq!(lines.display(), shown, "{s}");
        }
        let form = "lines are N, N-M, N,M, or N-";
        let errors = [
            ("3:4", "N:M reads as line and column, a range is N-M"),
            ("0", "lines are numbered from 1"),
            ("0-3", "lines are numbered from 1"),
            ("3-0", "lines are numbered from 1"),
            ("5-3", "the end is before the start"),
            ("2147483648", form),
            ("+3", form),
            ("-3", form),
            ("3,", form),
            ("3-4-5", form),
            ("a-b", form),
            ("", form),
        ];
        for (s, reason) in errors {
            assert_eq!(
                parse_lines(s).unwrap_err().to_string(),
                format!("invalid line range {s}; {reason}"),
                "{s:?}"
            );
        }
    }

    #[test]
    fn four_to_forty_hex_digits_look_like_an_oid() {
        for yes in ["abcd", "ABCD", "0123456789", &"a".repeat(40)] {
            assert!(is_oid_like(yes), "{yes}");
        }
        for no in ["abc", "abcg", "", "v1.8.0", &"a".repeat(41)] {
            assert!(!is_oid_like(no), "{no}");
        }
    }
}
