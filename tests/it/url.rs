use sego::internal::address::{
    Address, AddressError, Dots, End, LineRange, Repo, Rev, UrlKind, parse,
    submodule_target,
};

const SHA: &str = "98cb6bf42e086f6af920b965c38cacc07402d51b";

fn ok(token: &str) -> Address {
    parse(token).unwrap_or_else(|e| panic!("{token}: {e}"))
}

fn split(parts: &[&str]) -> Rev {
    Rev::Split(parts.iter().map(ToString::to_string).collect())
}

fn range(start: u32, end: u32) -> LineRange {
    LineRange { start, end: End::Line(end) }
}

fn repo(name: &str) -> Repo {
    Repo::Exact(name.to_owned())
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
        assert_eq!(a.repo, repo("github.com/gorilla/mux"), "{token}");
        assert_eq!(a.rev, split(&["v1.8.0", "mux.go"]), "{token}");
        assert_eq!(a.lines, Some(range(10, 20)), "{token}");
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
    ] {
        assert_eq!(ok(token).kind, UrlKind::Plain, "{token}");
    }
}

#[test]
fn permalink_is_pinned() {
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
    assert_eq!(a.repo, repo("github.com/gorilla/mux"));
    assert_eq!(a.rev, split(&["v1.8.0", "mux.go"]));
    assert_eq!(a.kind, UrlKind::Blob);
}

#[test]
fn gitlab_forms() {
    let a = ok(
        "https://gitlab.com/gitlab-org/api/client-go/-/blob/main/README.md#L5",
    );
    assert_eq!(a.repo, repo("gitlab.com/gitlab-org/api/client-go"));
    assert_eq!(a.rev, split(&["main", "README.md"]));
    assert_eq!(a.lines, Some(range(5, 5)));
    assert_eq!(a.kind, UrlKind::Blob);
    let a = ok("gitlab.com/g/r/-/tree/main/src");
    assert_eq!(a.kind, UrlKind::Tree);
    let a = ok(&format!("gitlab.com/g/r/-/commit/{SHA}"));
    assert_eq!(a.kind, UrlKind::Commit);
    assert_eq!(a.repo, repo("gitlab.com/g/r"));
}

#[test]
fn sourcegraph_forms() {
    for token in [
        "https://sourcegraph.com/github.com/gorilla/mux@v1.8.0/-/blob/mux.go?L3-5",
        "sourcegraph.com/r/github.com/gorilla/mux@v1.8.0/-/blob/mux.go?L3-5",
    ] {
        let a = ok(token);
        assert_eq!(a.repo, repo("github.com/gorilla/mux"), "{token}");
        assert_eq!(a.rev, Rev::One("v1.8.0".into()), "{token}");
        assert_eq!(a.path.as_deref(), Some("mux.go"), "{token}");
        assert_eq!(a.lines, Some(range(3, 5)), "{token}");
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
        ("#L10", Some(range(10, 10))),
        ("#L10-L20", Some(range(10, 20))),
        ("#L10-20", Some(range(10, 20))),
        ("#L10C5-L12C8", Some(range(10, 12))),
        ("?L10-20", Some(range(10, 20))),
        ("?plain=1&L7", Some(range(7, 7))),
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
