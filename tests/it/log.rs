//! `sego log`.

use insta::assert_snapshot;
use serde_json::json;

use crate::common::{answered, sego};

/// A failed commit count fails the range, and the server's text is not printed.
/// A located error from `GqlError::kind`'s table: git exit 128.
#[test]
fn commit_count_error() {
    let first = json!({
        "data": {
            "r0": {
                "a": {"oid": "aaaa111111111111111111111111111111111111"},
                "b": {"oid": "bbbb222222222222222222222222222222222222"},
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    let second = json!({
        "data": {
            "a": {"commit": {"oid": "aaaa111111111111111111111111111111111111"}},
            "b": {"commit": {"oid": "bbbb222222222222222222222222222222222222"}},
            "c": {
                "comparison": {
                    "commits": {
                        "nodes": [
                            {
                                "author": {
                                    "date": "2026-07-30T17:30:00Z",
                                    "person": {"displayName": "Grace Hopper"},
                                },
                                "oid": "bbbb222222222222222222222222222222222222",
                                "parents": [{"oid": "aaaa111111111111111111111111111111111111"}],
                                "subject": "Fix the parser",
                            },
                        ],
                        "pageInfo": {"hasNextPage": false},
                    },
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
            "n": {"commit": null},
            "t": {"gitRefs": {"nodes": []}},
        },
        "errors": [
            {
                "message": "git command [git rev-list --count --secret] failed",
                "path": ["n", "commit", "behindAhead"],
            },
        ],
    });
    assert_snapshot!(answered(&["log", "example.com/acme/widget@v1..v2"], [first, second]));
}

/// A range between two commit ids has no rev field in its header.
#[test]
fn commit_id_range() {
    let first = json!({
        "data": {
            "r0": {
                "a": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                "b": {"oid": "456789abcdef0123456789abcdef0123456789ab"},
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    let second = json!({
        "data": {
            "a": {"commit": {"oid": "0123456789abcdef0123456789abcdef01234567"}},
            "b": {"commit": {"oid": "456789abcdef0123456789abcdef0123456789ab"}},
            "c": {
                "comparison": {
                    "commits": {
                        "nodes": [
                            {
                                "author": {
                                    "date": "2026-09-14T10:00:00Z",
                                    "person": {"displayName": "Ada Lovelace"},
                                },
                                "oid": "456789abcdef0123456789abcdef0123456789ab",
                                "parents": [{"oid": "0123456789abcdef0123456789abcdef01234567"}],
                                "subject": "Add the widget",
                            },
                        ],
                        "pageInfo": {"hasNextPage": false},
                    },
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
            "n": {"commit": {"behindAhead": {"ahead": 1, "behind": 0}}},
            "t": {"gitRefs": {"nodes": []}},
        },
    });
    assert_snapshot!(answered(
        &["log", "example.com/acme/widget@0123456789ab..456789abcdef"],
        [first, second]
    ));
}

/// A commit with two tags is marked with both.
#[test]
fn commit_with_two_tags() {
    assert_snapshot!(sego(&["log", "github.com/gorilla/context@v1.1.1..v1.1.2"]));
}

/// A path with control characters, given as the argument, is escaped in the header, and so are the authors, subjects and tags of the rows.
#[test]
fn commits_with_control_characters() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "oid": "0123456789abcdef0123456789abcdef01234567",
                    "w": {
                        "commit": {
                            "ancestors": {
                                "nodes": [
                                    {
                                        "author": {
                                            "date": "2026-08-01T09:00:00Z",
                                            "person": {
                                                "displayName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                            },
                                        },
                                        "oid": "0123456789abcdef0123456789abcdef01234567",
                                        "parents": [{"oid": "89abcdef0123456789abcdef0123456789abcdef"}],
                                        "subject": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                    },
                                    {
                                        "author": {
                                            "date": "a\u{1b}b\u{202e}c\u{200b}d\ne\rT00:00:00Z",
                                            "person": {
                                                "displayName": "b-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                            },
                                        },
                                        "oid": "89abcdef0123456789abcdef0123456789abcdef",
                                        "parents": [
                                            {
                                                "oid": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                            },
                                        ],
                                        "subject": "b-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                    },
                                    {
                                        "author": {
                                            "date": "2026-07-01T00:00:00Z",
                                            "person": {
                                                "displayName": "c-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                            },
                                        },
                                        "oid": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                        "parents": [],
                                        "subject": "c-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                    },
                                ],
                                "pageInfo": {"hasNextPage": false},
                            },
                        },
                    },
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
            "t0": {
                "gitRefs": {
                    "nodes": [
                        {
                            "displayName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                        },
                        {
                            "displayName": "t-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                        },
                        {
                            "displayName": "u-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"},
                        },
                    ],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &[
            "log",
            "example.com/acme/widget@v2.0.0:a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"
        ],
        [response]
    ));
}

/// In JSON the path, authors, subjects and tags hold the characters themselves.
#[test]
fn commits_with_control_characters_json() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "oid": "0123456789abcdef0123456789abcdef01234567",
                    "w": {
                        "commit": {
                            "ancestors": {
                                "nodes": [
                                    {
                                        "author": {
                                            "date": "2026-08-01T09:00:00Z",
                                            "person": {
                                                "displayName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                            },
                                        },
                                        "oid": "0123456789abcdef0123456789abcdef01234567",
                                        "parents": [{"oid": "89abcdef0123456789abcdef0123456789abcdef"}],
                                        "subject": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                    },
                                    {
                                        "author": {
                                            "date": "a\u{1b}b\u{202e}c\u{200b}d\ne\rT00:00:00Z",
                                            "person": {
                                                "displayName": "b-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                            },
                                        },
                                        "oid": "89abcdef0123456789abcdef0123456789abcdef",
                                        "parents": [
                                            {
                                                "oid": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                            },
                                        ],
                                        "subject": "b-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                    },
                                    {
                                        "author": {
                                            "date": "2026-07-01T00:00:00Z",
                                            "person": {
                                                "displayName": "c-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                            },
                                        },
                                        "oid": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                        "parents": [],
                                        "subject": "c-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                    },
                                ],
                                "pageInfo": {"hasNextPage": false},
                            },
                        },
                    },
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
            "t0": {
                "gitRefs": {
                    "nodes": [
                        {
                            "displayName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                        },
                        {
                            "displayName": "t-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                        },
                        {
                            "displayName": "u-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"},
                        },
                    ],
                },
            },
        },
    });
    let ran = answered(
        &[
            "log",
            "--json",
            "example.com/acme/widget@v2.0.0:a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
        ],
        [response],
    );
    assert_eq!(
        ran.stdout,
        "{\"repo\":\"example.com/acme/widget\",\"rev\":\"v2.0.0\",\"commit\":\"0123456789abcdef0123456789abcdef01234567\",\"rev_a\":null,\"rev_b\":null,\"commit_a\":null,\"commit_b\":null,\"path\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"follow\":false,\"behind\":null,\"commits_total\":null,\"commits_more\":false,\"commits\":[{\"commit\":\"0123456789abcdef0123456789abcdef01234567\",\"date\":\"2026-08-01\",\"author\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"subject\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"parents\":[\"89abcdef0123456789abcdef0123456789abcdef\"],\"tags\":[\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"t-a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\"]},{\"commit\":\"89abcdef0123456789abcdef0123456789abcdef\",\"date\":\"a\\u001bb\\u202ec\\u200bd\\ne\\r\",\"author\":\"b-a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"subject\":\"b-a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"parents\":[\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\"],\"tags\":[]},{\"commit\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"date\":\"2026-07-01\",\"author\":\"c-a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"subject\":\"c-a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"parents\":[],\"tags\":[\"u-a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\"]}],\"mirrored_at\":null,\"notes\":[]}\n"
    );
    assert_snapshot!(ran);
}

/// A row without a subject ends after its author or its tag marks.
#[test]
fn commits_without_subjects() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "oid": "0001000100010001000100010001000100010001",
                    "w": {
                        "commit": {
                            "ancestors": {
                                "nodes": [
                                    {
                                        "author": {
                                            "date": "2026-08-01T09:00:00Z",
                                            "person": {"displayName": "Ada Lovelace"},
                                        },
                                        "oid": "0001000100010001000100010001000100010001",
                                        "parents": [{"oid": "0002000200020002000200020002000200020002"}],
                                        "subject": "",
                                    },
                                    {
                                        "author": {
                                            "date": "2026-08-01T09:00:00Z",
                                            "person": {"displayName": "Ada Lovelace"},
                                        },
                                        "oid": "0002000200020002000200020002000200020002",
                                        "parents": [{"oid": "0003000300030003000300030003000300030003"}],
                                        "subject": "",
                                    },
                                    {
                                        "author": {
                                            "date": "2026-08-01T09:00:00Z",
                                            "person": {"displayName": "Ada Lovelace"},
                                        },
                                        "oid": "0003000300030003000300030003000300030003",
                                        "parents": [{"oid": "0004000400040004000400040004000400040004"}],
                                        "subject": "  ",
                                    },
                                ],
                                "pageInfo": {"hasNextPage": false},
                            },
                        },
                    },
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
            "t0": {
                "gitRefs": {
                    "nodes": [
                        {
                            "displayName": "v2",
                            "target": {"oid": "0002000200020002000200020002000200020002"},
                        },
                    ],
                },
            },
        },
    });
    assert_snapshot!(answered(&["log", "example.com/acme/widget@v2"], [response]));
}

/// A web URL is not an address: the error names the forms that are.
#[test]
fn compare_url_is_not_an_address() {
    assert_snapshot!(sego(&["log", "https://github.com/gorilla/mux/compare/v1.7.0...v1.7.1"]));
}

/// Without a revision the log starts at HEAD.
#[test]
fn default_revision() {
    assert_snapshot!(sego(&["log", "github.com/gorilla/context"]));
}

/// In JSON a log without a revision has a commit and no rev, range or total.
#[test]
fn default_revision_json() {
    assert_snapshot!(sego(&["log", "--json", "github.com/gorilla/context"]));
}

/// A deleted file's history is still listed.
#[test]
fn deleted_path() {
    assert_snapshot!(sego(&["log", "github.com/gorilla/mux@v1.8.0", "context_gorilla.go"]));
}

/// A path deleted within a range is still listed.
#[test]
fn deleted_path_in_range() {
    assert_snapshot!(sego(&["log", "github.com/gorilla/mux@v1.1..v1.7.0", "context_gorilla.go"]));
}

/// An empty range prints 0 commits.
#[test]
fn empty_range() {
    assert_snapshot!(sego(&["log", "github.com/gorilla/mux@v1.8.0..v1.7.0"]));
}

/// In JSON an empty range has a total of 0.
#[test]
fn empty_range_json() {
    assert_snapshot!(sego(&["log", "--json", "github.com/gorilla/mux@v1.8.0..v1.7.0"]));
}

/// A repository with no commits is an error.
#[test]
fn empty_repository() {
    assert_snapshot!(sego(&["log", "github.com/ascoders/blog"]));
}

/// Every tag marks the commit it names, however old the tag.
#[test]
fn every_tag_marks_its_commit() {
    assert_snapshot!(sego(&["log", "github.com/siyuan-note/siyuan@v0.1.0"]));
}

/// --follow follows a path across a rename, as git log --follow.
#[test]
fn follow_crosses_a_rename() {
    assert_snapshot!(sego(&["log", "--follow", "github.com/gorilla/mux@v1.8.0", "context.go"]));
}

/// A cut log that follows renames has no pointer: the last commit shown can hold the path under an
/// older name.
#[test]
fn follow_past_150_commits_points_nowhere() {
    assert_snapshot!(sego(&[
        "log",
        "--follow",
        "github.com/sharkdp/bat@fc9546854f5a:src/bin/bat/main.rs"
    ]));
}

/// A base revision that does not exist is an error.
#[test]
fn missing_base_revision() {
    assert_snapshot!(sego(&["log", "github.com/gorilla/mux@v9.9.9..v1.8.0"]));
}

/// A path that exists nowhere is an error.
#[test]
fn missing_path() {
    assert_snapshot!(sego(&["log", "github.com/gorilla/mux@v1.8.0", "nope.go"]));
}

/// A missing path with control characters, given as the argument, is escaped in the error.
#[test]
fn missing_path_with_control_characters() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "oid": "0123456789abcdef0123456789abcdef01234567",
                    "w": {
                        "commit": {"ancestors": {"nodes": [], "pageInfo": {"hasNextPage": false}}},
                    },
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
            "t0": {"gitRefs": {"nodes": []}},
        },
    });
    assert_snapshot!(answered(
        &[
            "log",
            "example.com/acme/widget@v2.0.0:a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i-gone"
        ],
        [response]
    ));
}

/// A revision that does not exist is an error.
#[test]
fn missing_revision() {
    assert_snapshot!(sego(&["log", "github.com/gorilla/mux@v9.9.9"]));
}

/// One commit says commit in the singular.
#[test]
fn one_commit() {
    assert_snapshot!(sego(&["log", "github.com/gorilla/context@652e24b8c857"]));
}

/// A path in the address limits a revision to the commits touching it.
#[test]
fn path_in_address() {
    assert_snapshot!(sego(&["log", "github.com/gorilla/mux@v1.8.0:go.mod"]));
}

/// A path operand that leaves the repository's root names nothing.
#[test]
fn path_that_leaves_the_root() {
    assert_snapshot!(sego(&["log", "github.com/gorilla/mux@v1.8.0", "../mux.go"]));
}

/// A range lists its commits with tag marks.
#[test]
fn range() {
    assert_snapshot!(sego(&["log", "github.com/gorilla/mux@v1.7.1..v1.7.4"]));
}

/// A range of exactly 150 commits is not cut.
#[test]
fn range_at_default_limit() {
    assert_snapshot!(sego(&["log", "github.com/gorilla/mux@v1.1..v1.8.0"]));
}

/// A range from a commit id to a tag names the range in its rev field.
#[test]
fn range_from_commit_id_to_tag() {
    assert_snapshot!(sego(&["log", "github.com/gorilla/context@08b5f424b927..v1.1.2"]));
}

/// A range from a tag to a commit id names the range in its rev field.
#[test]
fn range_from_tag_to_commit_id() {
    assert_snapshot!(sego(&["log", "github.com/gorilla/context@v1.1.1..6a490e2f9d52"]));
}

/// A glob path limits a range without checking that the path exists.
#[test]
fn range_glob_path() {
    assert_snapshot!(sego(&["log", "github.com/gorilla/mux@v1.7.0..v1.7.1", "*.go"]));
}

/// A range with a path past 150 commits has no total.
#[test]
fn range_glob_path_past_default_limit() {
    assert_snapshot!(sego(&["log", "github.com/gorilla/mux@v1.1..v1.8.1", "*"]));
}

/// In JSON a range with a path past 150 commits says more exist and has no total.
#[test]
fn range_glob_path_past_default_limit_json() {
    assert_snapshot!(sego(&["log", "--json", "github.com/gorilla/mux@v1.1..v1.8.1", "*"]));
}

/// A range in a repository with no commits is an error.
#[test]
fn range_in_empty_repository() {
    assert_snapshot!(sego(&["log", "github.com/ascoders/blog@v1..v2"]));
}

/// JSON carries the range, the commits and their tags.
#[test]
fn range_json() {
    assert_snapshot!(sego(&["log", "--json", "github.com/gorilla/mux@v1.6.2..v1.7.0"]));
}

/// A range of one commit says commit in the singular.
#[test]
fn range_of_one_commit() {
    assert_snapshot!(sego(&["log", "github.com/gorilla/context@a45ea70b19f4..6a490e2f9d52"]));
}

/// A range past 150 commits says how many exist.
#[test]
fn range_past_default_limit() {
    assert_snapshot!(sego(&["log", "github.com/gorilla/mux@v1.1..v1.8.1"]));
}

/// In JSON a range past 150 commits lists all of them with their total.
#[test]
fn range_past_default_limit_json() {
    assert_snapshot!(sego(&["log", "--json", "github.com/gorilla/mux@v1.1..v1.8.1"]));
}

/// A path limits a range to the commits touching it.
#[test]
fn range_path() {
    assert_snapshot!(sego(&["log", "github.com/gorilla/mux@v1.7.0..v1.8.0", "mux.go"]));
}

/// A path with control characters in a range is escaped in the header.
#[test]
fn range_path_with_control_characters() {
    let first = json!({
        "data": {
            "r0": {
                "a": {"oid": "fedcba9876543210fedcba9876543210fedcba98"},
                "b": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    let second = json!({
        "data": {
            "a": {"commit": {"oid": "fedcba9876543210fedcba9876543210fedcba98"}},
            "b": {"commit": {"oid": "0123456789abcdef0123456789abcdef01234567"}},
            "c": {
                "comparison": {
                    "commits": {
                        "nodes": [
                            {
                                "author": {
                                    "date": "2026-08-01T09:00:00Z",
                                    "person": {
                                        "displayName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                    },
                                },
                                "oid": "0123456789abcdef0123456789abcdef01234567",
                                "parents": [{"oid": "89abcdef0123456789abcdef0123456789abcdef"}],
                                "subject": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            },
                            {
                                "author": {
                                    "date": "a\u{1b}b\u{202e}c\u{200b}d\ne\rT00:00:00Z",
                                    "person": {
                                        "displayName": "b-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                    },
                                },
                                "oid": "89abcdef0123456789abcdef0123456789abcdef",
                                "parents": [
                                    {
                                        "oid": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                    },
                                ],
                                "subject": "b-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            },
                            {
                                "author": {
                                    "date": "2026-07-01T00:00:00Z",
                                    "person": {
                                        "displayName": "c-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                    },
                                },
                                "oid": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                "parents": [],
                                "subject": "c-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            },
                        ],
                        "pageInfo": {"hasNextPage": false},
                    },
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
            "n": {"commit": {"behindAhead": {"ahead": 3, "behind": 0}}},
            "t": {
                "gitRefs": {
                    "nodes": [
                        {
                            "displayName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                        },
                        {
                            "displayName": "t-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                        },
                        {
                            "displayName": "u-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"},
                        },
                    ],
                },
            },
        },
    });
    assert_snapshot!(answered(
        &[
            "log",
            "example.com/acme/widget@v1.0.0..v2.0.0:a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"
        ],
        [first, second]
    ));
}

/// The server's message for a rejected request is escaped in the error.
#[test]
fn rejected_request_with_control_characters() {
    let response = json!({
        "data": null,
        "errors": [{"message": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"}],
    });
    assert_snapshot!(answered(&["log", "example.com/acme/widget@v2.0.0"], [response]));
}

/// A revision with exactly 150 commits is not cut.
#[test]
fn revision_at_default_limit() {
    assert_snapshot!(sego(&["log", "github.com/gorilla/mux@9fa818a44c2b"]));
}

/// A revision with exactly 151 commits lists 150 and says more exist, though the server has no next page.
#[test]
fn revision_one_past_default_limit() {
    assert_snapshot!(sego(&["log", "github.com/gorilla/mux@198f95d82475"]));
}

/// A revision lists its newest 150 commits.
#[test]
fn revision_past_default_limit() {
    assert_snapshot!(sego(&["log", "github.com/gorilla/mux@v1.8.0"]));
}

/// In JSON a revision past 150 commits is cut at 150 and says more exist.
#[test]
fn revision_past_default_limit_json() {
    assert_snapshot!(sego(&["log", "--json", "github.com/gorilla/mux@v1.8.0"]));
}

/// A cut log of a path points to the two logs that together list what it left out.
#[test]
fn revision_with_a_path_past_150_commits_points_to_the_rest() {
    assert_snapshot!(sego(&["log", "github.com/sharkdp/bat@fc9546854f5a:Cargo.toml"]));
}

/// A subject past 500 columns is cut in text, with a pointer to JSON.
#[test]
fn subject_past_500_columns() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "oid": "0001000100010001000100010001000100010001",
                    "w": {
                        "commit": {
                            "ancestors": {
                                "nodes": [
                                    {
                                        "author": {
                                            "date": "2026-08-01T09:00:00Z",
                                            "person": {"displayName": "Ada Lovelace"},
                                        },
                                        "oid": "0001000100010001000100010001000100010001",
                                        "parents": [{"oid": "0002000200020002000200020002000200020002"}],
                                        "subject": "sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss",
                                    },
                                ],
                                "pageInfo": {"hasNextPage": false},
                            },
                        },
                    },
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
            "t0": {"gitRefs": {"nodes": []}},
        },
    });
    assert_snapshot!(answered(&["log", "example.com/acme/widget@v2"], [response]));
}

/// In JSON a subject past 500 columns is whole.
#[test]
fn subject_past_500_columns_json() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "oid": "0001000100010001000100010001000100010001",
                    "w": {
                        "commit": {
                            "ancestors": {
                                "nodes": [
                                    {
                                        "author": {
                                            "date": "2026-08-01T09:00:00Z",
                                            "person": {"displayName": "Ada Lovelace"},
                                        },
                                        "oid": "0001000100010001000100010001000100010001",
                                        "parents": [{"oid": "0002000200020002000200020002000200020002"}],
                                        "subject": "sssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssssss",
                                    },
                                ],
                                "pageInfo": {"hasNextPage": false},
                            },
                        },
                    },
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
            "t0": {"gitRefs": {"nodes": []}},
        },
    });
    assert_snapshot!(answered(&["log", "--json", "example.com/acme/widget@v2"], [response]));
}

/// A tag list that fails leaves the log unmarked, with a note.
/// No recording shows these located errors: a server failure sourcegraph.com doesn't give on demand.
#[test]
fn tags_not_marked_when_the_tag_list_fails() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "oid": "98cb6bf42e086f6af920b965c38cacc07402d51b",
                    "w": {
                        "commit": {
                            "ancestors": {
                                "nodes": [
                                    {
                                        "author": {
                                            "date": "2019-07-20T14:53:35Z",
                                            "person": {"displayName": "Franklin Harding"},
                                        },
                                        "oid": "7a1bf406d6f5f8cfef4802eb0d949606c6ee7aea",
                                        "parents": [{"oid": "eab9c4f3d22d907d728aa0f5918934357866249e"}],
                                        "subject": "[docs] Add documentation for using mux to serve a SPA (#493)",
                                    },
                                    {
                                        "author": {
                                            "date": "2018-05-17T17:36:23Z",
                                            "person": {"displayName": "Matt Silverlock"},
                                        },
                                        "oid": "c85619274f5d55a67491010f883a91f568b3a9b0",
                                        "parents": [{"oid": "e3702bed27f0d39777b0b37b664b6280e8ef8fbf"}],
                                        "subject": "[deps] Add go.mod for versioned Go (#376)",
                                    },
                                ],
                                "pageInfo": {"hasNextPage": false},
                            },
                        },
                    },
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": {"updatedAt": "2026-10-27T07:22:54Z"},
                "name": "github.com/gorilla/mux",
            },
            "t0": null,
        },
        "errors": [
            {
                "message": "rpc error: code = Unavailable desc = gitserver unavailable",
                "path": ["t0", "gitRefs"],
            },
        ],
    });
    assert_snapshot!(answered(&["log", "github.com/gorilla/mux@v1.8.0:go.mod"], [response]));
}

/// In JSON a tag list that fails leaves each commit's tags null.
/// No recording shows these located errors: a server failure sourcegraph.com doesn't give on demand.
#[test]
fn tags_not_marked_when_the_tag_list_fails_json() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "oid": "98cb6bf42e086f6af920b965c38cacc07402d51b",
                    "w": {
                        "commit": {
                            "ancestors": {
                                "nodes": [
                                    {
                                        "author": {
                                            "date": "2019-07-20T14:53:35Z",
                                            "person": {"displayName": "Franklin Harding"},
                                        },
                                        "oid": "7a1bf406d6f5f8cfef4802eb0d949606c6ee7aea",
                                        "parents": [{"oid": "eab9c4f3d22d907d728aa0f5918934357866249e"}],
                                        "subject": "[docs] Add documentation for using mux to serve a SPA (#493)",
                                    },
                                    {
                                        "author": {
                                            "date": "2018-05-17T17:36:23Z",
                                            "person": {"displayName": "Matt Silverlock"},
                                        },
                                        "oid": "c85619274f5d55a67491010f883a91f568b3a9b0",
                                        "parents": [{"oid": "e3702bed27f0d39777b0b37b664b6280e8ef8fbf"}],
                                        "subject": "[deps] Add go.mod for versioned Go (#376)",
                                    },
                                ],
                                "pageInfo": {"hasNextPage": false},
                            },
                        },
                    },
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": {"updatedAt": "2026-10-27T07:22:54Z"},
                "name": "github.com/gorilla/mux",
            },
            "t0": null,
        },
        "errors": [
            {
                "message": "rpc error: code = Unavailable desc = gitserver unavailable",
                "path": ["t0", "gitRefs"],
            },
        ],
    });
    assert_snapshot!(answered(
        &["log", "--json", "github.com/gorilla/mux@v1.8.0:go.mod"],
        [response]
    ));
}
