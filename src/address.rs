use std::fmt;

use crate::escape::{is_unsafe, shell_word};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoName(String);

impl RepoName {
    /// `s` as a repository name, with a URL's scheme, user, port, scp-style
    /// `:`, trailing `/`s and one trailing `.git` dropped.
    pub fn parse(s: &str) -> Result<RepoName, AddressError> {
        RepoName::checked(normalise(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The repository a submodule's `url` names: as `parse`, but with a
    /// relative URL (`./` or `../`) resolved against `parent`, and `None`
    /// for anything that can't be a repository name, such as a local path.
    pub fn from_submodule_url(
        url: &str,
        parent: &RepoName,
    ) -> Option<RepoName> {
        let name = if url.starts_with("./") || url.starts_with("../") {
            tidy(&resolve(parent.as_str(), url)?, false)
        } else {
            normalise(url)
        };
        if name.starts_with('/') {
            return None;
        }
        RepoName::checked(name).ok()
    }

    fn checked(name: String) -> Result<RepoName, AddressError> {
        if name.is_empty() {
            Err(AddressError::EmptyRepo)
        } else if name.chars().any(forbidden) {
            Err(AddressError::BadRepo(name))
        } else {
            Ok(RepoName(name))
        }
    }
}

/// `url` as a repository name: see `RepoName::parse`.
fn normalise(url: &str) -> String {
    let (rest, scp) = host_onwards(url);
    tidy(&rest, scp)
}

/// `url` with any scheme, user and port dropped, and whether it is
/// scp-style (`host:owner/name`), which is only recognised without a scheme.
/// Only a URL with a scheme has a port, and no repository name has one.
fn host_onwards(url: &str) -> (String, bool) {
    let (rest, scheme) = match after_scheme(url) {
        Some(rest) => (rest, true),
        None => (url, false),
    };
    let slash = rest.find('/');
    let host_end = if scheme {
        Some(slash.unwrap_or(rest.len()))
    } else {
        rest.find(':').filter(|&colon| slash.is_none_or(|s| colon < s))
    };
    let Some(end) = host_end else {
        return (rest.to_owned(), false);
    };
    let host = &rest[..end];
    let host = host.rfind('@').map_or(host, |at| &host[at + 1..]);
    let host = if scheme { without_port(host) } else { host };
    (format!("{host}{}", &rest[end..]), !scheme)
}

/// `url` after its `scheme://`, if the text before the first `://` is a
/// scheme: an ASCII letter, then letters, digits, `+`, `-` or `.`.
fn after_scheme(url: &str) -> Option<&str> {
    let (scheme, rest) = url.split_once("://")?;
    let mut chars = scheme.chars();
    let is_scheme = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    is_scheme.then_some(rest)
}

/// `host` without a trailing `:port`.
fn without_port(host: &str) -> &str {
    match host.rsplit_once(':') {
        Some((name, port)) if port.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => host,
    }
}

/// `relative` resolved against `base` as git resolves a submodule's URL
/// against its superproject's: `base` is the directory `./` names, and `..`
/// can't climb past its host.
fn resolve(base: &str, relative: &str) -> Option<String> {
    let mut segments: Vec<&str> = base.split('/').collect();
    for segment in relative.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
                if segments.is_empty() {
                    return None;
                }
            }
            _ => segments.push(segment),
        }
    }
    Some(segments.join("/"))
}

/// The repository name in `rest`, the output of `host_onwards`.
fn tidy(rest: &str, scp: bool) -> String {
    let name = if scp { rest.replacen(':', "/", 1) } else { rest.to_owned() };
    let name = name.trim_end_matches('/');
    name.strip_suffix(".git").unwrap_or(name).to_owned()
}

fn forbidden(c: char) -> bool {
    c.is_whitespace() || c.is_control() || is_unsafe(c)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rev(String);

impl Rev {
    pub fn parse(s: &str) -> Result<Rev, AddressError> {
        if s.is_empty() {
            Err(AddressError::EmptyRev)
        } else if s.contains("..") {
            Err(AddressError::TwoDots(s.to_owned()))
        } else if s.chars().any(forbidden) {
            Err(AddressError::BadRev(s.to_owned()))
        } else {
            Ok(Rev(s.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// `repo[@rev]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub repo: RepoName,
    pub rev: Option<Rev>,
}

impl Address {
    /// `s` split at the first `@` after the host, so a URL's user is not
    /// read as the separator.
    pub fn parse(s: &str) -> Result<Address, AddressError> {
        let (rest, scp) = host_onwards(s);
        let (repo, rev) = match rest.split_once('@') {
            Some((repo, rev)) => (repo, Some(rev)),
            None => (rest.as_str(), None),
        };
        Ok(Address {
            repo: RepoName::checked(tidy(repo, scp))?,
            rev: rev.map(Rev::parse).transpose()?,
        })
    }

    pub fn rev_or_head(&self) -> &str {
        self.rev.as_ref().map_or("HEAD", Rev::as_str)
    }
}

/// A repo-relative path; empty is the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoPath(String);

impl RepoPath {
    pub fn parse(s: &str) -> RepoPath {
        RepoPath(s.trim_matches('/').to_owned())
    }

    /// The path as the upstream takes it: no leading or trailing `/`.
    pub fn upstream(&self) -> &str {
        &self.0
    }

    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }
}

/// 1-based, inclusive lines; no `end` runs to the last line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    pub start: u32,
    pub end: Option<u32>,
}

impl LineRange {
    /// `N`, `N-M`, `N-` or `N,M`.
    pub fn parse(s: &str) -> Result<LineRange, AddressError> {
        let bad = || AddressError::BadLines(s.to_owned());
        let number = |n: &str| {
            if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
                return Err(bad());
            }
            n.parse::<u32>().map_err(|_| bad())
        };
        let (start, end) = match s.split_once(['-', ',']) {
            Some((start, "")) if s.ends_with('-') => (number(start)?, None),
            Some((start, end)) => (number(start)?, Some(number(end)?)),
            None => {
                let n = number(s)?;
                (n, Some(n))
            }
        };
        if start == 0 || end == Some(0) {
            return Err(AddressError::LineZero(s.to_owned()));
        }
        match end {
            Some(end) if end < start => {
                Err(AddressError::Inverted(s.to_owned()))
            }
            _ => Ok(LineRange { start, end }),
        }
    }
}

impl fmt::Display for LineRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.end {
            Some(end) if end == self.start => write!(f, "{end}"),
            Some(end) => write!(f, "{}-{end}", self.start),
            None => write!(f, "{}-", self.start),
        }
    }
}

/// A full commit SHA, as the upstream returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitId(pub String);

impl CommitId {
    /// The first 12 hex digits, as text output shows them.
    pub fn short(&self) -> &str {
        self.0.get(..12).unwrap_or(&self.0)
    }
}

/// A repository at a resolved commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pinned {
    pub repo: RepoName,
    pub commit: CommitId,
}

impl Pinned {
    /// `repo@short` as a shell word, valid input to any command.
    pub fn word(&self) -> String {
        shell_word(&self.to_string()).into_owned()
    }
}

/// `repo@short`, unquoted.
impl fmt::Display for Pinned {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.repo.as_str(), self.commit.short())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddressError {
    EmptyRepo,
    EmptyRev,
    TwoDots(String),
    LineZero(String),
    Inverted(String),
    BadLines(String),
    BadRepo(String),
    BadRev(String),
}

impl fmt::Display for AddressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AddressError::EmptyRepo => write!(f, "empty repository name"),
            AddressError::EmptyRev => write!(f, "empty revision after @"),
            AddressError::TwoDots(rev) => write!(
                f,
                r#"invalid revision {}; a revision cannot contain "..""#,
                shell_word(rev)
            ),
            AddressError::LineZero(lines) => write!(
                f,
                "invalid line range {}; lines are numbered from 1",
                shell_word(lines)
            ),
            AddressError::Inverted(lines) => write!(
                f,
                "invalid line range {}; the end is before the start",
                shell_word(lines)
            ),
            AddressError::BadLines(lines) => write!(
                f,
                "invalid line range {}; expected N, N-M, N- or N,M",
                shell_word(lines)
            ),
            AddressError::BadRepo(name) => write!(
                f,
                "invalid repository name {}; a repository name cannot \
                 contain whitespace, control or invisible characters",
                shell_word(name)
            ),
            AddressError::BadRev(rev) => write!(
                f,
                "invalid revision {}; a revision cannot contain whitespace, \
                 control or invisible characters",
                shell_word(rev)
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use expect_test::expect;

    use super::*;

    #[test]
    fn repo_names_are_normalised() {
        for (input, expected) in [
            ("github.com/gorilla/mux", "github.com/gorilla/mux"),
            ("https://github.com/gorilla/mux.git", "github.com/gorilla/mux"),
            ("https://github.com/gorilla/mux/", "github.com/gorilla/mux"),
            ("git@github.com:gorilla/mux.git", "github.com/gorilla/mux"),
            ("ssh://git@github.com/gorilla/mux", "github.com/gorilla/mux"),
            ("GitHub.com/Gorilla/Mux", "GitHub.com/Gorilla/Mux"),
            ("https://github.com:443/gorilla/mux", "github.com/gorilla/mux"),
            ("ssh://git@github.com:22/x/y.git", "github.com/x/y"),
            ("https://u:pw@github.com:443/x/y", "github.com/x/y"),
        ] {
            assert_eq!(RepoName::parse(input).unwrap().as_str(), expected);
        }
        assert!(matches!(RepoName::parse(""), Err(AddressError::EmptyRepo)));
        assert!(matches!(
            RepoName::parse("https://"),
            Err(AddressError::EmptyRepo)
        ));
        assert!(matches!(
            RepoName::parse("github.com/x/y z"),
            Err(AddressError::BadRepo(_))
        ));
    }

    #[test]
    fn submodule_urls_are_repo_names() {
        let parent = RepoName::parse("github.com/x/a").unwrap();
        for (url, expected) in [
            (
                "https://github.com/rust-lang/cargo.git",
                Some("github.com/rust-lang/cargo"),
            ),
            ("git@gitlab.com:a/b.git", Some("gitlab.com/a/b")),
            ("ssh://git@github.com:22/x/y.git", Some("github.com/x/y")),
            ("../b.git", Some("github.com/x/b")),
            ("./b", Some("github.com/x/a/b")),
            ("../../y/b.git/", Some("github.com/y/b")),
            ("../../../b", None),
            ("", None),
            ("file:///srv/git/b.git", None),
            ("https://github.com/x/b c", None),
        ] {
            assert_eq!(
                RepoName::from_submodule_url(url, &parent)
                    .as_ref()
                    .map(RepoName::as_str),
                expected,
                "{url}"
            );
        }
    }

    #[test]
    fn addresses_split_at_the_first_at_sign() {
        let a = Address::parse("github.com/gorilla/mux@feature/x").unwrap();
        assert_eq!(a.rev_or_head(), "feature/x");
        assert_eq!(a.rev.unwrap().as_str(), "feature/x");
        let scp =
            Address::parse("git@github.com:gorilla/mux.git@v1.7.0").unwrap();
        assert_eq!(
            (scp.repo.as_str(), scp.rev.unwrap().as_str()),
            ("github.com/gorilla/mux", "v1.7.0")
        );
        let url =
            Address::parse("https://git@github.com:443/x/y.git@a~1").unwrap();
        assert_eq!(
            (url.repo.as_str(), url.rev.unwrap().as_str()),
            ("github.com/x/y", "a~1")
        );
        expect![[r#"
            Ok(
                Address {
                    repo: RepoName(
                        "github.com/x/y",
                    ),
                    rev: Some(
                        Rev(
                            "a://b",
                        ),
                    ),
                },
            )
        "#]]
        .assert_debug_eq(&Address::parse("github.com/x/y@a://b"));
        let bare = Address::parse("github.com/x/y").unwrap();
        assert_eq!((bare.rev.as_ref(), bare.rev_or_head()), (None, "HEAD"));
        assert!(matches!(
            Address::parse("github.com/x/y@"),
            Err(AddressError::EmptyRev)
        ));
        assert!(matches!(
            Address::parse("github.com/x/y@v1..v2"),
            Err(AddressError::TwoDots(_))
        ));
        assert!(matches!(Address::parse("@v1"), Err(AddressError::EmptyRepo)));
        for bad in ["github.com/x/y@a b", "github.com/x/y@a\u{1b}"] {
            assert!(matches!(
                Address::parse(bad),
                Err(AddressError::BadRev(_))
            ));
        }
    }

    #[test]
    fn name_errors_say_what_they_reject() {
        let messages: String = ["a b", "a\u{1b}", "a\u{200b}"]
            .into_iter()
            .flat_map(|bad| {
                [
                    format!("github.com/x/{bad}"),
                    format!("github.com/x/y@{bad}"),
                ]
            })
            .map(|input| {
                Address::parse(&input).unwrap_err().to_string() + "\n"
            })
            .collect();
        expect![[r#"
            invalid repository name 'github.com/x/a b'; a repository name cannot contain whitespace, control or invisible characters
            invalid revision 'a b'; a revision cannot contain whitespace, control or invisible characters
            invalid repository name $'github.com/x/a\033'; a repository name cannot contain whitespace, control or invisible characters
            invalid revision $'a\033'; a revision cannot contain whitespace, control or invisible characters
            invalid repository name $'github.com/x/a\342\200\213'; a repository name cannot contain whitespace, control or invisible characters
            invalid revision $'a\342\200\213'; a revision cannot contain whitespace, control or invisible characters
        "#]].assert_eq(&messages);
    }

    #[test]
    fn revs() {
        assert_eq!(Rev::parse("a7962380~1").unwrap().as_str(), "a7962380~1");
        assert!(matches!(Rev::parse(""), Err(AddressError::EmptyRev)));
        assert!(matches!(Rev::parse("a..b"), Err(AddressError::TwoDots(_))));
        assert!(matches!(Rev::parse("a\tb"), Err(AddressError::BadRev(_))));
    }

    #[test]
    fn repo_paths() {
        for (input, upstream, root) in [
            ("", "", true),
            ("/", "", true),
            ("mux.go", "mux.go", false),
            ("/src/tools/", "src/tools", false),
        ] {
            let path = RepoPath::parse(input);
            assert_eq!((path.upstream(), path.is_root()), (upstream, root));
        }
    }

    #[test]
    fn line_ranges() {
        for (input, start, end, shown) in [
            ("30", 30, Some(30), "30"),
            ("30-33", 30, Some(33), "30-33"),
            ("30-", 30, None, "30-"),
            ("30,33", 30, Some(33), "30-33"),
        ] {
            let range = LineRange::parse(input).unwrap();
            assert_eq!((range.start, range.end), (start, end), "{input}");
            assert_eq!(range.to_string(), shown);
        }
        assert!(matches!(
            LineRange::parse("0"),
            Err(AddressError::LineZero(_))
        ));
        assert!(matches!(
            LineRange::parse("3-0"),
            Err(AddressError::LineZero(_))
        ));
        assert!(matches!(
            LineRange::parse("33-30"),
            Err(AddressError::Inverted(_))
        ));
        assert_eq!(
            LineRange::parse("33,30").unwrap_err().to_string(),
            "invalid line range 33,30; the end is before the start"
        );
        for bad in ["99999999999", "a-b", "", "-3", "+3", "3,", "3-4-5"] {
            assert!(
                matches!(
                    LineRange::parse(bad),
                    Err(AddressError::BadLines(_))
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn pinned_prints_a_shell_word_with_a_short_commit() {
        let pinned = Pinned {
            repo: RepoName::parse("x/y").unwrap(),
            commit: CommitId(
                "a7962380ca08c6a9a5ba0c0a2e5d6b35a8f2b9f1".to_owned(),
            ),
        };
        assert_eq!(pinned.commit.short(), "a7962380ca08");
        assert_eq!(pinned.word(), "x/y@a7962380ca08");
    }

    #[test]
    fn errors_read_as_one_line_and_quote_input() {
        for (error, shown) in [
            (AddressError::EmptyRepo, "empty repository name"),
            (AddressError::EmptyRev, "empty revision after @"),
            (
                AddressError::TwoDots("a..b".to_owned()),
                r#"invalid revision a..b; a revision cannot contain "..""#,
            ),
            (
                AddressError::LineZero("0".to_owned()),
                "invalid line range 0; lines are numbered from 1",
            ),
            (
                AddressError::Inverted("33,30".to_owned()),
                "invalid line range 33,30; the end is before the start",
            ),
            (
                AddressError::BadLines("a b".to_owned()),
                "invalid line range 'a b'; expected N, N-M, N- or N,M",
            ),
            (
                AddressError::BadRepo("a\u{1b}".to_owned()),
                "invalid repository name $'a\\033'; a repository name \
                 cannot contain whitespace, control or invisible characters",
            ),
            (
                AddressError::BadRev("a b".to_owned()),
                "invalid revision 'a b'; a revision \
                 cannot contain whitespace, control or invisible characters",
            ),
        ] {
            assert_eq!(error.to_string(), shown);
        }
    }
}
