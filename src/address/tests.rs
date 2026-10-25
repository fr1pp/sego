use crate::address::{
    Address, AddressError, Dots, End, LineRange, Repo, Rev, UrlKind,
    is_oid_like, parse, parse_lines, submodule_target,
};

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

fn split(parts: &[&str]) -> Rev {
    Rev::Split(parts.iter().map(ToString::to_string).collect())
}

fn span(start: u32, end: u32) -> LineRange {
    LineRange { start, end: End::Line(end) }
}

#[test]
fn repos() {
    let cases = [
        (
            "github.com/gorilla/mux",
            exact("github.com/gorilla/mux"),
            Rev::Default,
        ),
        (
            "GitHub.com/Gorilla/Mux",
            exact("GitHub.com/Gorilla/Mux"),
            Rev::Default,
        ),
        ("gorilla/mux", Repo::OwnerName("gorilla/mux".into()), Rev::Default),
        (
            "gitlab-org/charts/gitlab",
            exact("gitlab.com/gitlab-org/charts/gitlab"),
            Rev::Default,
        ),
        (
            "src.fedoraproject.org/rpms/x",
            exact("src.fedoraproject.org/rpms/x"),
            Rev::Default,
        ),
        (
            "git@github.com:gorilla/mux.git@v1.7.0",
            exact("github.com/gorilla/mux"),
            one("v1.7.0"),
        ),
        (
            "https://u:pw@github.com:443/x/y.git",
            exact("github.com/x/y"),
            Rev::Default,
        ),
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
fn revisions_and_paths() {
    let cases = [
        (
            "github.com/x/y@feature/x:src/a.go",
            one("feature/x"),
            Some("src/a.go"),
        ),
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
fn ranges() {
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
fn rejected() {
    let cases = [
        (
            "github.com/x/y@",
            "the address has an empty revision: github.com/x/y@",
        ),
        (
            "github.com/x/y@:src",
            "the address has an empty revision: github.com/x/y@:src",
        ),
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
        (
            "github.com/x/a\u{1b}b",
            "invalid repository name 'github.com/x/a\u{1b}b'",
        ),
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
fn lines() {
    let range = |start, end| LineRange { start, end };
    let cases = [
        ("10", range(10, End::Line(10)), "10"),
        ("10-20", range(10, End::Line(20)), "10-20"),
        ("10,20", range(10, End::Line(20)), "10-20"),
        ("10-", range(10, End::Open), "10-"),
        (
            "2147483647",
            range(2_147_483_647, End::Line(2_147_483_647)),
            "2147483647",
        ),
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

#[test]
fn github_forms() {
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
        Rev::Range {
            base: "v1.7.0".into(),
            head: "v1.7.1".into(),
            dots: Dots::Three
        }
    );
    let a = ok("github.com/gorilla/mux/compare/v1.7.0..v1.7.1");
    assert!(matches!(a.rev, Rev::Range { dots: Dots::Two, .. }));
    let a = ok("github.com/gorilla/mux/compare/feature/x...main");
    assert!(
        matches!(a.rev, Rev::Range { ref base, .. } if base == "feature/x")
    );
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
fn raw_githubusercontent() {
    let a = ok("https://raw.githubusercontent.com/gorilla/mux/v1.8.0/mux.go");
    assert_eq!(a.repo, exact("github.com/gorilla/mux"));
    assert_eq!(a.rev, split(&["v1.8.0", "mux.go"]));
    assert_eq!(a.kind, UrlKind::Blob);
}

#[test]
fn gitlab_forms() {
    let a = ok(
        "https://gitlab.com/gitlab-org/api/client-go/-/blob/main/README.md#L5",
    );
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
fn sourcegraph_forms() {
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
    let a = ok(&format!(
        "sourcegraph.com/github.com/gorilla/mux@v1.8.0/-/commit/{SHA}"
    ));
    assert_eq!(a.kind, UrlKind::Commit);
    assert_eq!(a.rev, Rev::One(SHA.into()));
    let a = ok("sourcegraph.com/github.com/o/r@feature/x/-/blob/a.go");
    assert_eq!(a.rev, Rev::One("feature/x".into()));
}

#[test]
fn fragments() {
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
fn percent_decoding() {
    let a = ok("https://github.com/o/r/blob/main/a%20b.go");
    assert_eq!(a.rev, split(&["main", "a b.go"]));
    let a = ok("https://github.com/o/r/blob/main/a%zzb.go");
    assert_eq!(a.rev, split(&["main", "a%zzb.go"]));
    let a = ok("https://github.com/o/r/blob/main/100%");
    assert_eq!(a.rev, split(&["main", "100%"]));
}

#[test]
fn rejected_urls() {
    let cases = [
        (
            "https://github.com/o/r/pull/12",
            "pull request URLs name no revision sego can read",
        ),
        (
            "https://gitlab.com/g/r/-/merge_requests/3",
            "pull request URLs name no revision sego can read",
        ),
        (
            "https://github.com/o/r/releases/tag/v1",
            "release URLs name no revision sego can read",
        ),
        (
            "https://github.com/o/r/wiki",
            "unsupported URL form: https://github.com/o/r/wiki",
        ),
        (
            "https://gitlab.com/g/r/-/issues/1",
            "unsupported URL form: https://gitlab.com/g/r/-/issues/1",
        ),
        (
            "github.com/o/r/blob/main",
            "unsupported URL form: github.com/o/r/blob/main",
        ),
    ];
    for (token, message) in cases {
        let e = parse(token).expect_err(token);
        assert_eq!(e.to_string(), message, "{token}");
    }
}

#[test]
fn submodule_targets() {
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
        (
            "ssh://git@github.com/x/y.git",
            "github.com/x/a",
            Some("github.com/x/y"),
        ),
        ("", "github.com/x/a", None),
        ("../../../b", "github.com/x/a", None),
        ("file:///srv/b.git", "github.com/x/a", None),
        ("https://github.com/x", "github.com/x/a", None),
        ("/srv/b", "github.com/x/a", None),
    ];
    for (url, parent, want) in cases {
        assert_eq!(
            submodule_target(url, parent).as_deref(),
            want,
            "{url} from {parent}"
        );
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
fn sourcegraph_without_revision() {
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
fn repository_urls() {
    let cases: [(&str, &str, Rev); _] = [
        ("https://www.github.com/o/r", "github.com/o/r", Rev::Default),
        ("www.github.com/o/r", "github.com/o/r", Rev::Default),
        ("https://github.com/o/r/", "github.com/o/r", Rev::Default),
        ("github.com/o/r.git", "github.com/o/r", Rev::Default),
        ("https://github.com/o/r.git/", "github.com/o/r", Rev::Default),
        (
            "https://github.com/o/r?tab=readme#L5",
            "github.com/o/r",
            Rev::Default,
        ),
        ("https://gitlab.com/g/s/r", "gitlab.com/g/s/r", Rev::Default),
        ("https://www.gitlab.com/g/s/r/", "gitlab.com/g/s/r", Rev::Default),
        ("gitlab.com/g/r?a=b#top", "gitlab.com/g/r", Rev::Default),
        (
            "https://sourcegraph.com/github.com/o/r",
            "github.com/o/r",
            Rev::Default,
        ),
        ("sourcegraph.com/r/github.com/o/r", "github.com/o/r", Rev::Default),
        (
            "https://www.sourcegraph.com/github.com/o/r/",
            "github.com/o/r",
            Rev::Default,
        ),
        (
            "sourcegraph.com/github.com/o/r?L3#L4",
            "github.com/o/r",
            Rev::Default,
        ),
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
            Rev::Range {
                base: "v1".into(),
                head: "v2".into(),
                dots: Dots::Two,
            },
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
    for token in [
        "sourcegraph.com/github.com/o/r@-x",
        "sourcegraph.com/github.com/o/r@a%20b",
    ] {
        assert!(
            matches!(parse(token), Err(AddressError::InvalidRevision(_))),
            "{token}: {:?}",
            parse(token)
        );
    }
    assert_eq!(
        parse("sourcegraph.com/github.com/o/r@"),
        Err(AddressError::EmptyRevision(
            "sourcegraph.com/github.com/o/r@".to_owned()
        ))
    );
}
