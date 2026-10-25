use sego::internal::address::{
    Address, AddressError, Dots, End, LineRange, Repo, Rev, UrlKind,
    is_oid_like, parse, parse_lines, pinned, range_rev_field, short,
};

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
fn repo_candidates() {
    assert_eq!(
        Repo::OwnerName("x/y".into()).candidates(),
        ["github.com/x/y", "gitlab.com/x/y"]
    );
    assert_eq!(exact("github.com/x/y").candidates(), ["github.com/x/y"]);
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
fn pinned_addresses() {
    let commit = "98cb6bf42e086f6af920b965c38cacc07402d51b";
    let quoted = pinned("github.com/x/y", commit, Some("a b.go"));
    assert_eq!(quoted.as_str(), "'github.com/x/y@98cb6bf42e08:a b.go'");
    assert_eq!(quoted.to_string(), quoted.as_str());
    assert_eq!(
        pinned("github.com/x/y", commit, None).as_str(),
        "github.com/x/y@98cb6bf42e08"
    );
}

#[test]
fn a_commit_is_shortened_to_twelve_characters() {
    let commit = "98cb6bf42e086f6af920b965c38cacc07402d51b";
    assert_eq!(short(commit), "98cb6bf42e08");
    assert_eq!(short("abc"), "abc");
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
fn a_range_has_a_rev_field_unless_both_sides_are_commit_ids() {
    let (a, b) = ("0123abcd", "4567ef01");
    assert_eq!(range_rev_field(a, b, Dots::Two), None);
    assert_eq!(range_rev_field(a, b, Dots::Three), None);
    assert_eq!(
        range_rev_field("v1", b, Dots::Two).as_deref(),
        Some("rev v1..4567ef01")
    );
    assert_eq!(
        range_rev_field(a, "main", Dots::Three).as_deref(),
        Some("rev 0123abcd...main")
    );
}
