//! Web URLs of GitHub, GitLab and Sourcegraph, and raw.githubusercontent.com,
//! read as addresses.

use super::{
    Address, AddressError, FULL_OID, LineRange, Repo, Rev, UrlKind, commit_rev, is_bad_char,
    is_digits, is_oid_like, parse_lines, parse_path, parse_repo, parse_rev, strip_clone_url,
};

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
            && let (Some(h), Some(l)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2]))
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

fn decode_all(segments: &[&str]) -> Vec<String> {
    segments.iter().map(|s| percent_decode(s)).collect()
}

fn github_repo(owner: &str, name: &str) -> String {
    format!("github.com/{}/{}", percent_decode(owner), percent_decode(name))
}

fn is_sha(s: &str) -> bool {
    s.len() == FULL_OID && is_oid_like(s)
}

/// `10`, `10-20`, `10C5-12C8` and the same with `L` before each number.
/// Anything else is not a line selection and yields `None`.
fn selection_lines(selection: &str) -> Result<Option<LineRange>, AddressError> {
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
            part.strip_prefix('L').is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()))
        })
    });
    selection.map(selection_lines).transpose().map(Option::flatten)
}

struct Web {
    rev: Rev,
    path: Option<String>,
    kind: UrlKind,
}

fn split_rev(token: &str, rest: &[&str], kind: UrlKind) -> Result<Web, AddressError> {
    let unsupported = || AddressError::UnsupportedUrl(token.to_owned());
    let segments = decode_all(rest);
    if segments.is_empty() || segments.iter().any(String::is_empty) {
        return Err(unsupported());
    }
    if is_sha(&segments[0]) {
        let path = segments[1..].join("/");
        if kind == UrlKind::Blob && path.is_empty() {
            return Err(unsupported());
        }
        return Ok(Web { rev: Rev::One(segments[0].clone()), path: Some(path), kind });
    }
    if kind == UrlKind::Blob && segments.len() < 2 {
        return Err(unsupported());
    }
    Ok(Web { rev: Rev::Split(segments), path: None, kind })
}

fn forge_tail(token: &str, keyword: &str, rest: &[&str]) -> Result<Web, AddressError> {
    let unsupported = || AddressError::UnsupportedUrl(token.to_owned());
    match keyword {
        "blob" | "raw" | "blame" => split_rev(token, rest, UrlKind::Blob),
        "tree" => split_rev(token, rest, UrlKind::Tree),
        "commit" => match rest {
            [sha] if !sha.is_empty() => Ok(Web {
                rev: commit_rev(token, &percent_decode(sha))?,
                path: None,
                kind: UrlKind::Commit,
            }),
            _ => Err(unsupported()),
        },
        "compare" => {
            let segments = decode_all(rest);
            match parse_rev(&segments.join("/"))? {
                rev @ Rev::Range { .. } => Ok(Web { rev, path: None, kind: UrlKind::Compare }),
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
        path: web.path.as_deref().map(parse_path),
        lines: lines?,
        kind: web.kind,
    })
}

fn has_scp_marks(segment: &str) -> bool {
    segment.contains(['@', ':'])
}

/// A repository named by a URL with nothing after it. The query string and
/// fragment are dropped, and the name is read as a plain address would be.
fn repository(token: &str, host: &str, segs: &[&str]) -> Result<Address, AddressError> {
    let name = format!("{host}/{}", decode_all(segs).join("/"));
    Ok(Address {
        repo: parse_repo(&name, token)?,
        rev: Rev::Default,
        path: None,
        lines: None,
        kind: UrlKind::Plain,
    })
}

fn github_url(
    token: &str,
    host: &str,
    segs: &[&str],
    lines: Result<Option<LineRange>, AddressError>,
) -> Option<Result<Address, AddressError>> {
    if segs.len() < 2 || segs[..2].iter().any(|s| s.is_empty() || has_scp_marks(s)) {
        return None;
    }
    if segs.len() == 2 {
        return Some(repository(token, host, segs));
    }
    let repo = github_repo(segs[0], segs[1]);
    if GITHUB_KEYWORDS.contains(&segs[2]) {
        return Some(finish(repo, forge_tail(token, segs[2], &segs[3..]), lines));
    }
    if segs.iter().any(|s| has_scp_marks(s)) {
        return None;
    }
    Some(Err(AddressError::UnsupportedUrl(token.to_owned())))
}

fn gitlab_url(
    token: &str,
    host: &str,
    segs: &[&str],
    lines: Result<Option<LineRange>, AddressError>,
) -> Option<Result<Address, AddressError>> {
    let Some(dash) = segs.iter().position(|s| *s == "-") else {
        if segs.len() < 2 || segs.iter().any(|s| s.is_empty() || has_scp_marks(s)) {
            return None;
        }
        return Some(repository(token, host, segs));
    };
    let unsupported = || AddressError::UnsupportedUrl(token.to_owned());
    let repo_segs = &segs[..dash];
    if repo_segs.iter().any(|s| has_scp_marks(s)) {
        return None;
    }
    if repo_segs.len() < 2 || repo_segs.iter().any(|s| s.is_empty()) {
        return Some(Err(unsupported()));
    }
    let repo = format!("gitlab.com/{}", decode_all(repo_segs).join("/"));
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
    if segs.is_empty() {
        return None;
    }
    let dash = segs.iter().position(|s| *s == "-");
    let unsupported = || AddressError::UnsupportedUrl(token.to_owned());
    if dash.is_none() && segs.iter().any(|s| s.contains(':')) {
        return Some(Err(unsupported()));
    }
    let lines = if dash.is_some() { lines } else { Ok(None) };
    let dash = dash.unwrap_or(segs.len());
    let named = if segs[0] == "r" { &segs[1..dash] } else { &segs[..dash] };
    let joined = named.join("/");
    let (repo, rev) = match joined.split_once('@') {
        Some((repo, rev)) => (repo, Some(rev)),
        None => (joined.as_str(), None),
    };
    let repo = percent_decode(repo);
    let repo_segments: Vec<&str> = repo.split('/').collect();
    if repo_segments.len() < 3
        || !repo_segments[0].contains('.')
        || repo_segments.iter().any(|s| s.is_empty() || s.contains(is_bad_char))
    {
        return Some(Err(unsupported()));
    }
    let rev = match rev {
        None => Ok(Rev::Default),
        Some("") => Err(AddressError::EmptyRevision(token.to_owned())),
        Some(rev) => parse_rev(&percent_decode(rev)),
    };
    let rest = decode_all(segs.get(dash + 1..).unwrap_or_default());
    let tail = rev.and_then(|rev| match rest.split_first() {
        None if dash == segs.len() => Ok(Web { rev, path: None, kind: UrlKind::Plain }),
        Some((keyword, path)) if keyword == "blob" || keyword == "tree" => {
            let path = path.join("/");
            let kind = if keyword == "blob" { UrlKind::Blob } else { UrlKind::Tree };
            if kind == UrlKind::Blob && path.is_empty() {
                Err(unsupported())
            } else {
                Ok(Web { rev, path: Some(path), kind })
            }
        }
        Some((keyword, [sha])) if keyword == "commit" => {
            let rev = commit_rev(token, sha)?;
            Ok(Web { rev, path: None, kind: UrlKind::Commit })
        }
        _ => Err(unsupported()),
    });
    Some(finish(repo, tail, lines))
}

fn raw_githubusercontent_url(
    token: &str,
    segs: &[&str],
    lines: Result<Option<LineRange>, AddressError>,
) -> Result<Address, AddressError> {
    if segs.len() < 4 || segs[..2].iter().any(|s| s.is_empty()) {
        return Err(AddressError::UnsupportedUrl(token.to_owned()));
    }
    let repo = github_repo(segs[0], segs[1]);
    finish(repo, split_rev(token, &segs[2..], UrlKind::Blob), lines)
}

/// `None` when the token is not a web URL and the plain grammar applies.
pub fn parse_web(token: &str) -> Option<Result<Address, AddressError>> {
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
    let segs: Vec<&str> = if path.is_empty() { Vec::new() } else { path.split('/').collect() };
    let lines = url_lines(fragment, query);
    match host.to_ascii_lowercase().as_str() {
        "github.com" => github_url(token, host, &segs, lines),
        "gitlab.com" => gitlab_url(token, host, &segs, lines),
        "sourcegraph.com" => sourcegraph_url(token, &segs, lines),
        "raw.githubusercontent.com" => Some(raw_githubusercontent_url(token, &segs, lines)),
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

#[cfg(test)]
mod tests {
    use super::super::{Address, AddressError, Dots, End, LineRange, Repo, Rev, UrlKind, parse};
    use super::*;

    const SHA: &str = "98cb6bf42e086f6af920b965c38cacc07402d51b";

    fn exact(s: &str) -> Repo {
        Repo::Exact(s.to_owned())
    }

    fn ok(token: &str) -> Address {
        parse(token).unwrap_or_else(|e| panic!("{token}: {e}"))
    }

    fn split(parts: &[&str]) -> Rev {
        Rev::Split(parts.iter().map(ToString::to_string).collect())
    }

    fn span(start: u32, end: u32) -> LineRange {
        LineRange { start, end: End::Line(end) }
    }

    #[test]
    fn github_blob_raw_blame_tree_commit_and_compare_urls_parse() {
        let base = "github.com/gorilla/mux/blob/v1.8.0/mux.go";
        for token in [
            format!("https://{base}#L10-L20"),
            format!("https://www.{base}?plain=1#L10-L20"),
            format!("{base}#L10-L20"),
        ] {
            let a = ok(&token);
            assert_eq!(a.repo, exact("github.com/gorilla/mux"), "{token}");
            assert_eq!(a.rev, split(&["v1.8.0", "mux.go"]), "{token}");
            assert_eq!(a.lines, Some(span(10, 20)), "{token}");
            assert_eq!(a.kind, UrlKind::Blob, "{token}");
            assert_eq!(a.path, None, "{token}");
        }
        for kw in ["raw", "blame"] {
            let a = ok(&format!("github.com/gorilla/mux/{kw}/main/mux.go"));
            assert_eq!(a.kind, UrlKind::Blob);
            assert_eq!(a.rev, split(&["main", "mux.go"]));
        }
        let a = ok("https://github.com/gorilla/mux/tree/main/src");
        assert_eq!(a.kind, UrlKind::Tree);
        assert_eq!(a.rev, split(&["main", "src"]));
        let a = ok(&format!("https://github.com/gorilla/mux/commit/{SHA}"));
        assert_eq!(a.kind, UrlKind::Commit);
        assert_eq!(a.rev, Rev::One(SHA.into()));
        let a = ok("github.com/gorilla/mux/compare/v1.7.0...v1.7.1");
        assert_eq!(a.kind, UrlKind::Compare);
        assert_eq!(
            a.rev,
            Rev::Range { base: "v1.7.0".into(), head: "v1.7.1".into(), dots: Dots::Three }
        );
        let a = ok("github.com/gorilla/mux/compare/v1.7.0..v1.7.1");
        assert!(matches!(a.rev, Rev::Range { dots: Dots::Two, .. }));
        let a = ok("github.com/gorilla/mux/compare/feature/x...main");
        assert!(matches!(a.rev, Rev::Range { ref base, .. } if base == "feature/x"));
    }

    #[test]
    fn plain_forms_stay_plain() {
        for token in [
            "github.com/gorilla/mux",
            "https://github.com/gorilla/mux/",
            "github.com/gorilla/mux@v1:mux.go",
            "github.com/gorilla/mux@v1:dir/mux.go",
            "gitlab.com/g/r@main:docs/-/x.md",
        ] {
            assert_eq!(ok(token).kind, UrlKind::Plain, "{token}");
        }
    }

    #[test]
    fn a_blob_url_at_a_commit_names_the_commit() {
        let a = ok(&format!("https://github.com/gorilla/mux/blob/{SHA}/mux.go"));
        assert_eq!(a.rev, Rev::One(SHA.into()));
        assert_eq!(a.path.as_deref(), Some("mux.go"));
        assert_eq!(a.kind, UrlKind::Blob);
        let a = ok(&format!("https://github.com/gorilla/mux/tree/{SHA}"));
        assert_eq!(a.path.as_deref(), Some(""));
        assert_eq!(a.kind, UrlKind::Tree);
    }

    #[test]
    fn a_raw_githubusercontent_url_parses_as_a_blob() {
        let a = ok("https://raw.githubusercontent.com/gorilla/mux/v1.8.0/mux.go");
        assert_eq!(a.repo, exact("github.com/gorilla/mux"));
        assert_eq!(a.rev, split(&["v1.8.0", "mux.go"]));
        assert_eq!(a.kind, UrlKind::Blob);
    }

    #[test]
    fn gitlab_blob_tree_and_commit_urls_parse() {
        let a = ok("https://gitlab.com/gitlab-org/api/client-go/-/blob/main/README.md#L5");
        assert_eq!(a.repo, exact("gitlab.com/gitlab-org/api/client-go"));
        assert_eq!(a.rev, split(&["main", "README.md"]));
        assert_eq!(a.lines, Some(span(5, 5)));
        assert_eq!(a.kind, UrlKind::Blob);
        let a = ok("gitlab.com/g/r/-/tree/main/src");
        assert_eq!(a.kind, UrlKind::Tree);
        let a = ok(&format!("gitlab.com/g/r/-/commit/{SHA}"));
        assert_eq!(a.kind, UrlKind::Commit);
        assert_eq!(a.repo, exact("gitlab.com/g/r"));
    }

    #[test]
    fn sourcegraph_blob_and_tree_urls_parse() {
        for token in [
            "https://sourcegraph.com/github.com/gorilla/mux@v1.8.0/-/blob/mux.go?L3-5",
            "sourcegraph.com/r/github.com/gorilla/mux@v1.8.0/-/blob/mux.go?L3-5",
        ] {
            let a = ok(token);
            assert_eq!(a.repo, exact("github.com/gorilla/mux"), "{token}");
            assert_eq!(a.rev, Rev::One("v1.8.0".into()), "{token}");
            assert_eq!(a.path.as_deref(), Some("mux.go"), "{token}");
            assert_eq!(a.lines, Some(span(3, 5)), "{token}");
            assert_eq!(a.kind, UrlKind::Blob, "{token}");
        }
        let a = ok("sourcegraph.com/github.com/gorilla/mux@v1.8.0/-/tree/docs");
        assert_eq!(a.kind, UrlKind::Tree);
        assert_eq!(a.path.as_deref(), Some("docs"));
        let a = ok(&format!("sourcegraph.com/github.com/gorilla/mux@v1.8.0/-/commit/{SHA}"));
        assert_eq!(a.kind, UrlKind::Commit);
        assert_eq!(a.rev, Rev::One(SHA.into()));
        let a = ok("sourcegraph.com/github.com/o/r@feature/x/-/blob/a.go");
        assert_eq!(a.rev, Rev::One("feature/x".into()));
    }

    #[test]
    fn line_fragments_and_queries_give_the_lines() {
        let cases = [
            ("#L10", Some(span(10, 10))),
            ("#L10-L20", Some(span(10, 20))),
            ("#L10-20", Some(span(10, 20))),
            ("#L10C5-L12C8", Some(span(10, 12))),
            ("?L10-20", Some(span(10, 20))),
            ("?plain=1&L7", Some(span(7, 7))),
            ("?plain=1", None),
            ("#readme", None),
        ];
        for (suffix, want) in cases {
            let token = format!("github.com/o/r/blob/main/a.go{suffix}");
            assert_eq!(ok(&token).lines, want, "{token}");
        }
        assert!(matches!(
            parse("github.com/o/r/blob/main/a.go#L20-L10"),
            Err(AddressError::LineEndBeforeStart(_))
        ));
    }

    #[test]
    fn percent_escapes_in_a_url_path_are_decoded_where_valid() {
        let a = ok("https://github.com/o/r/blob/main/a%20b.go");
        assert_eq!(a.rev, split(&["main", "a b.go"]));
        let a = ok("https://github.com/o/r/blob/main/a%zzb.go");
        assert_eq!(a.rev, split(&["main", "a%zzb.go"]));
        let a = ok("https://github.com/o/r/blob/main/100%");
        assert_eq!(a.rev, split(&["main", "100%"]));
    }

    #[test]
    fn urls_that_name_no_readable_revision_are_rejected() {
        let cases = [
            ("https://github.com/o/r/pull/12", "pull request URLs name no revision sego can read"),
            (
                "https://gitlab.com/g/r/-/merge_requests/3",
                "pull request URLs name no revision sego can read",
            ),
            (
                "https://github.com/o/r/releases/tag/v1",
                "release URLs name no revision sego can read",
            ),
            ("https://github.com/o/r/wiki", "unsupported URL form: https://github.com/o/r/wiki"),
            (
                "https://gitlab.com/g/r/-/issues/1",
                "unsupported URL form: https://gitlab.com/g/r/-/issues/1",
            ),
            ("github.com/o/r/blob/main", "unsupported URL form: github.com/o/r/blob/main"),
        ];
        for (token, message) in cases {
            let e = parse(token).expect_err(token);
            assert_eq!(e.to_string(), message, "{token}");
        }
    }

    #[test]
    fn a_submodule_url_resolves_against_its_parent_repository() {
        let cases = [
            (
                "../blesh-contrib",
                "github.com/akinomyoga/ble.sh",
                Some("github.com/akinomyoga/blesh-contrib"),
            ),
            (
                "https://github.com/rust-lang/cargo.git",
                "github.com/x/a",
                Some("github.com/rust-lang/cargo"),
            ),
            ("git@github.com:x/y.git", "github.com/x/a", Some("github.com/x/y")),
            ("ssh://git@github.com/x/y.git", "github.com/x/a", Some("github.com/x/y")),
            ("", "github.com/x/a", None),
            ("../../../b", "github.com/x/a", None),
            ("file:///srv/b.git", "github.com/x/a", None),
            ("https://github.com/x", "github.com/x/a", None),
            ("/srv/b", "github.com/x/a", None),
        ];
        for (url, parent, want) in cases {
            assert_eq!(submodule_target(url, parent).as_deref(), want, "{url} from {parent}");
        }
    }

    #[test]
    fn a_commit_url_names_one_commit() {
        for token in [
            "github.com/o/r/commit/a..b",
            "github.com/o/r/commit/a...b",
            "github.com/o/r/commit/a%2E%2Eb",
            "github.com/o/r/commit/a..",
            "gitlab.com/g/r/-/commit/a..b",
            "sourcegraph.com/github.com/o/r@v1/-/commit/a..b",
        ] {
            assert!(
                matches!(parse(token), Err(AddressError::UnsupportedUrl(_))),
                "{token}: {:?}",
                parse(token)
            );
        }
    }

    #[test]
    fn web_revisions_are_validated() {
        let a = ok("sourcegraph.com/github.com/o/r@a..b/-/blob/x");
        assert!(matches!(a.rev, Rev::Range { dots: Dots::Two, .. }));
        for token in [
            "sourcegraph.com/github.com/o/r@-x/-/blob/x",
            "sourcegraph.com/github.com/o/r@a%20b/-/blob/x",
            "sourcegraph.com/github.com/o/r@v1/-/commit/%0A",
            "github.com/o/r/commit/%0A",
            "gitlab.com/g/r/-/commit/-x",
        ] {
            assert!(
                matches!(parse(token), Err(AddressError::InvalidRevision(_))),
                "{token}: {:?}",
                parse(token)
            );
        }
    }

    #[test]
    fn a_sourcegraph_url_without_a_revision_has_the_default_revision() {
        for token in [
            "sourcegraph.com/github.com/o/r/-/blob/a.go",
            "sourcegraph.com/r/github.com/o/r/-/blob/a.go",
        ] {
            let a = ok(token);
            assert_eq!(a.repo, exact("github.com/o/r"), "{token}");
            assert_eq!(a.rev, Rev::Default, "{token}");
            assert_eq!(a.path.as_deref(), Some("a.go"), "{token}");
        }
        let a = ok("sourcegraph.com/github.com/o/r/-/tree/docs");
        assert_eq!((a.kind, a.rev), (UrlKind::Tree, Rev::Default));
    }

    #[test]
    fn sourcegraph_repository_names_are_validated() {
        for token in [
            "sourcegraph.com/foo@v1/-/blob/a",
            "sourcegraph.com/github.com/o@v1/-/blob/a",
            "sourcegraph.com/@v1/-/blob/a",
        ] {
            assert_eq!(
                parse(token),
                Err(AddressError::UnsupportedUrl(token.to_owned())),
                "{token}"
            );
        }
    }

    #[test]
    fn sourcegraph_paths_that_name_no_repository() {
        for token in [
            "sourcegraph.com/search",
            "https://sourcegraph.com/search",
            "sourcegraph.com/r",
            "sourcegraph.com/github.com/o/r:a.go",
            "sourcegraph.com/github.com/o/r@v1:a.go",
            "sourcegraph.com/r/github.com/o:r",
        ] {
            assert_eq!(
                parse(token),
                Err(AddressError::UnsupportedUrl(token.to_owned())),
                "{token}"
            );
        }
    }

    #[test]
    fn sourcegraph_splits_before_decoding() {
        let a = ok("sourcegraph.com/github.com/o/r@v1/-/blob/a%40b.go");
        assert_eq!(a.path.as_deref(), Some("a@b.go"));
        let a = ok("sourcegraph.com/github.com/o/r@feature%40x/-/blob/a.go");
        assert_eq!(a.rev, Rev::One("feature@x".into()));
    }

    #[test]
    fn repository_urls_resolve_to_a_repository_with_an_optional_revision() {
        let cases: [(&str, &str, Rev); _] = [
            ("https://www.github.com/o/r", "github.com/o/r", Rev::Default),
            ("www.github.com/o/r", "github.com/o/r", Rev::Default),
            ("https://github.com/o/r/", "github.com/o/r", Rev::Default),
            ("github.com/o/r.git", "github.com/o/r", Rev::Default),
            ("https://github.com/o/r.git/", "github.com/o/r", Rev::Default),
            ("https://github.com/o/r?tab=readme#L5", "github.com/o/r", Rev::Default),
            ("https://gitlab.com/g/s/r", "gitlab.com/g/s/r", Rev::Default),
            ("https://www.gitlab.com/g/s/r/", "gitlab.com/g/s/r", Rev::Default),
            ("gitlab.com/g/r?a=b#top", "gitlab.com/g/r", Rev::Default),
            ("https://sourcegraph.com/github.com/o/r", "github.com/o/r", Rev::Default),
            ("sourcegraph.com/r/github.com/o/r", "github.com/o/r", Rev::Default),
            ("https://www.sourcegraph.com/github.com/o/r/", "github.com/o/r", Rev::Default),
            ("sourcegraph.com/github.com/o/r?L3#L4", "github.com/o/r", Rev::Default),
            (
                "https://sourcegraph.com/github.com/o/r@v1.2",
                "github.com/o/r",
                Rev::One("v1.2".into()),
            ),
            (
                "sourcegraph.com/r/github.com/o/r@feature/x",
                "github.com/o/r",
                Rev::One("feature/x".into()),
            ),
            (
                "sourcegraph.com/github.com/o/r@v1..v2",
                "github.com/o/r",
                Rev::Range { base: "v1".into(), head: "v2".into(), dots: Dots::Two },
            ),
        ];
        for (token, name, rev) in cases {
            let a = ok(token);
            assert_eq!(a.repo, exact(name), "{token}");
            assert_eq!(a.rev, rev, "{token}");
            assert_eq!(a.path, None, "{token}");
            assert_eq!(a.lines, None, "{token}");
            assert_eq!(a.kind, UrlKind::Plain, "{token}");
        }
    }

    #[test]
    fn repository_urls_validate_the_revision() {
        for token in ["sourcegraph.com/github.com/o/r@-x", "sourcegraph.com/github.com/o/r@a%20b"] {
            assert!(
                matches!(parse(token), Err(AddressError::InvalidRevision(_))),
                "{token}: {:?}",
                parse(token)
            );
        }
        assert_eq!(
            parse("sourcegraph.com/github.com/o/r@"),
            Err(AddressError::EmptyRevision("sourcegraph.com/github.com/o/r@".to_owned()))
        );
    }
}
