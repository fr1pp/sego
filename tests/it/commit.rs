//! `sego show` of a commit.

use insta::assert_snapshot;
use serde_json::json;

use crate::common::{answered, sego};

/// A commit body with CRLF line ends.
#[test]
fn commit_with_crlf_message() {
    assert_snapshot!(sego(&[
        "show",
        "github.com/gorilla/mux@948bec34b5168796bf3fc4dbb09215baa970351a"
    ]));
}

/// A commit shows its message and changed files.
#[test]
fn commit_with_message_and_files() {
    assert_snapshot!(sego(&[
        "show",
        "github.com/ast-grep/agent-skill@affe2b9b7c608f4e354d7e83d0583ed35e845650"
    ]));
}

/// A comparison without totals fails the commit.
#[test]
fn comparison_without_totals() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "author": {
                        "date": "2026-09-09T20:14:26Z",
                        "person": {"displayName": "Jonathan Davies"},
                    },
                    "committer": {"date": "2026-09-10T01:02:03Z", "person": {"displayName": "GitHub"}},
                    "message": "m",
                    "oid": "affe2b9b7c608f4e354d7e83d0583ed35e845650",
                    "parents": [{"oid": "6b668aa526afdc623c1a9ed1d6ae920e04a717ad"}],
                    "subject": "m",
                    "w": {
                        "commit": {
                            "diff": {
                                "all": null,
                                "page": {"nodes": [], "pageInfo": {"hasNextPage": false}},
                            },
                        },
                    },
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    assert_snapshot!(answered(&["show", "example.com/acme/widget@v1"], [response]));
}

/// A failed file list fails the commit, and the server's text is not printed.
/// A located error from `GqlError::kind`'s table: git exit 128.
#[test]
fn file_list_error_hides_server_text() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "author": {
                        "date": "2021-02-03T04:05:06Z",
                        "person": {"displayName": "Grace Hopper"},
                    },
                    "committer": {
                        "date": "2021-02-03T04:05:06Z",
                        "person": {"displayName": "Grace Hopper"},
                    },
                    "message": "Add the parser",
                    "oid": "1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f708192a3",
                    "parents": [{"oid": "0f1e2d3c4b5a69788796a5b4c3d2e1f001122334"}],
                    "subject": "Add the parser",
                    "w": {
                        "commit": {
                            "diff": {
                                "all": {"diffStat": {"added": 30, "deleted": 4}, "totalCount": 2},
                                "page": {"nodes": [], "pageInfo": {"hasNextPage": false}},
                            },
                        },
                    },
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
        "errors": [
            {
                "message": "git command [git diff --raw -z --secret] failed",
                "path": ["r0", "commit", "w", "commit", "diff", "page", "nodes"],
            },
        ],
    });
    assert_snapshot!(answered(&["show", "example.com/acme/widget@v2.0.0"], [response]));
}

/// Files print with their status letter and a path with a space quoted.
#[test]
fn files_added_deleted_and_renamed() {
    let first = json!({
        "data": {
            "r0": {
                "commit": {
                    "author": {
                        "date": "2026-09-09T20:14:26Z",
                        "person": {"displayName": "Jonathan Davies"},
                    },
                    "committer": {"date": "2026-09-10T01:02:03Z", "person": {"displayName": "GitHub"}},
                    "message": "m",
                    "oid": "affe2b9b7c608f4e354d7e83d0583ed35e845650",
                    "parents": [{"oid": "6b668aa526afdc623c1a9ed1d6ae920e04a717ad"}],
                    "subject": "m",
                    "w": {
                        "commit": {
                            "diff": {
                                "all": {"diffStat": {"added": 6, "deleted": 8}, "totalCount": 3},
                                "page": {
                                    "nodes": [
                                        {
                                            "newPath": "new.go",
                                            "oldPath": null,
                                            "stat": {"added": 5, "deleted": 0},
                                        },
                                        {
                                            "newPath": null,
                                            "oldPath": "old.go",
                                            "stat": {"added": 0, "deleted": 7},
                                        },
                                        {
                                            "newPath": "b c.go",
                                            "oldPath": "a.go",
                                            "stat": {"added": 1, "deleted": 1},
                                        },
                                    ],
                                    "pageInfo": {"hasNextPage": false},
                                },
                            },
                        },
                    },
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    let second = json!({"data": {"repository": {"commit": {"tags": {"nodes": []}}}}});
    assert_snapshot!(answered(&["show", "example.com/acme/widget@v1"], [first, second]));
}

/// JSON carries the commit and its files.
#[test]
fn json_answer() {
    assert_snapshot!(sego(&["show", "--json", "github.com/gorilla/mux@v1.8.0"]));
}

/// A commit with a merge subject and one parent is not a merge.
#[test]
fn merge_subject_with_one_parent() {
    let first = json!({
        "data": {
            "r0": {
                "commit": {
                    "author": {
                        "date": "2026-09-09T20:14:26Z",
                        "person": {"displayName": "Jonathan Davies"},
                    },
                    "committer": {"date": "2026-09-10T01:02:03Z", "person": {"displayName": "GitHub"}},
                    "message": "Merge branch 'x'",
                    "oid": "affe2b9b7c608f4e354d7e83d0583ed35e845650",
                    "parents": [{"oid": "6b668aa526afdc623c1a9ed1d6ae920e04a717ad"}],
                    "subject": "Merge branch 'x'",
                    "w": {
                        "commit": {
                            "diff": {
                                "all": {"diffStat": {"added": 1, "deleted": 1}, "totalCount": 1},
                                "page": {
                                    "nodes": [
                                        {
                                            "newPath": "a.go",
                                            "oldPath": "a.go",
                                            "stat": {"added": 1, "deleted": 1},
                                        },
                                    ],
                                    "pageInfo": {"hasNextPage": false},
                                },
                            },
                        },
                    },
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    let second = json!({"data": {"repository": {"commit": {"tags": {"nodes": []}}}}});
    assert_snapshot!(answered(&["show", "example.com/acme/widget@v1"], [first, second]));
}

/// A merge commit that changed nothing has 0 files.
#[test]
fn merge_with_no_files() {
    assert_snapshot!(sego(&[
        "show",
        "github.com/gorilla/mux@306509329bce2152a92040e805ef24fd8e3f877d"
    ]));
}

/// A merge commit is diffed against its first parent.
#[test]
fn merge_with_two_parents() {
    assert_snapshot!(sego(&[
        "show",
        "github.com/gorilla/mux@abccd7e4d7c117370b7908f51e48decb38b6835b"
    ]));
}

/// A merge commit's JSON lists both parents.
#[test]
fn merge_with_two_parents_json() {
    assert_snapshot!(sego(&[
        "show",
        "--json",
        "github.com/gorilla/mux@abccd7e4d7c117370b7908f51e48decb38b6835b"
    ]));
}

/// A commit's author, message and file paths with control characters are escaped.
#[test]
fn message_and_names_with_control_characters() {
    let first = json!({
        "data": {
            "r0": {
                "commit": {
                    "author": {
                        "date": "a\u{1b}b\u{202e}c\u{200b}d\ne\rT00:00:00Z",
                        "person": {"displayName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"},
                    },
                    "committer": {
                        "date": "2026-03-01T12:00:00Z",
                        "person": {
                            "displayName": "c-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                        },
                    },
                    "message": "a\u{1b}b\nc\u{202e}d\ne\u{200b}f\ng\0h\ni\u{2028}j\nk\u{7f}l\nmid\rcr\ntab\tok\n",
                    "oid": "89abcdef0123456789abcdef0123456789abcdef",
                    "parents": [{"oid": "fedcba9876543210fedcba9876543210fedcba98"}],
                    "subject": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                    "w": {
                        "commit": {
                            "diff": {
                                "all": {"diffStat": {"added": 7, "deleted": 7}, "totalCount": 4},
                                "page": {
                                    "nodes": [
                                        {
                                            "newPath": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                            "oldPath": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                            "stat": {"added": 1, "deleted": 1},
                                        },
                                        {
                                            "newPath": "n-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                            "oldPath": "o-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                            "stat": {"added": 2, "deleted": 2},
                                        },
                                        {
                                            "newPath": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i-added",
                                            "oldPath": null,
                                            "stat": {"added": 4, "deleted": 0},
                                        },
                                        {
                                            "newPath": null,
                                            "oldPath": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i-deleted",
                                            "stat": {"added": 0, "deleted": 4},
                                        },
                                    ],
                                    "pageInfo": {"hasNextPage": false},
                                },
                            },
                        },
                    },
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    let second = json!({"data": {"repository": {"commit": {"tags": {"nodes": []}}}}});
    assert_snapshot!(answered(&["show", "example.com/acme/widget@v2.0.0"], [first, second]));
}

/// In JSON the author, message and paths hold the characters themselves.
#[test]
fn message_and_names_with_control_characters_json() {
    let first = json!({
        "data": {
            "r0": {
                "commit": {
                    "author": {
                        "date": "a\u{1b}b\u{202e}c\u{200b}d\ne\rT00:00:00Z",
                        "person": {"displayName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"},
                    },
                    "committer": {
                        "date": "2026-03-01T12:00:00Z",
                        "person": {
                            "displayName": "c-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                        },
                    },
                    "message": "a\u{1b}b\nc\u{202e}d\ne\u{200b}f\ng\0h\ni\u{2028}j\nk\u{7f}l\nmid\rcr\ntab\tok\n",
                    "oid": "89abcdef0123456789abcdef0123456789abcdef",
                    "parents": [{"oid": "fedcba9876543210fedcba9876543210fedcba98"}],
                    "subject": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                    "w": {
                        "commit": {
                            "diff": {
                                "all": {"diffStat": {"added": 7, "deleted": 7}, "totalCount": 4},
                                "page": {
                                    "nodes": [
                                        {
                                            "newPath": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                            "oldPath": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                            "stat": {"added": 1, "deleted": 1},
                                        },
                                        {
                                            "newPath": "n-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                            "oldPath": "o-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                            "stat": {"added": 2, "deleted": 2},
                                        },
                                        {
                                            "newPath": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i-added",
                                            "oldPath": null,
                                            "stat": {"added": 4, "deleted": 0},
                                        },
                                        {
                                            "newPath": null,
                                            "oldPath": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i-deleted",
                                            "stat": {"added": 0, "deleted": 4},
                                        },
                                    ],
                                    "pageInfo": {"hasNextPage": false},
                                },
                            },
                        },
                    },
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    let second = json!({"data": {"repository": {"commit": {"tags": {"nodes": []}}}}});
    let ran = answered(&["show", "--json", "example.com/acme/widget@v2.0.0"], [first, second]);
    assert_eq!(
        ran.stdout,
        "{\"repo\":\"example.com/acme/widget\",\"rev\":\"v2.0.0\",\"commit\":\"89abcdef0123456789abcdef0123456789abcdef\",\"kind\":\"commit\",\"author\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"author_date\":\"a\\u001bb\\u202ec\\u200bd\\ne\\r\",\"committer\":\"c-a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"committer_date\":\"2026-03-01\",\"parents\":[\"fedcba9876543210fedcba9876543210fedcba98\"],\"subject\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"message\":\"a\\u001bb\\nc\\u202ed\\ne\\u200bf\\ng\\u0000h\\ni\\u2028j\\nk\\u007fl\\nmid\\rcr\\ntab\\tok\\n\",\"files\":[{\"status\":\"M\",\"old_path\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"new_path\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"added\":1,\"deleted\":1},{\"status\":\"R\",\"old_path\":\"o-a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"new_path\":\"n-a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"added\":2,\"deleted\":2},{\"status\":\"A\",\"old_path\":null,\"new_path\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi-added\",\"added\":4,\"deleted\":0},{\"status\":\"D\",\"old_path\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi-deleted\",\"new_path\":null,\"added\":0,\"deleted\":4}],\"files_total\":4,\"files_more\":false,\"added\":7,\"deleted\":7,\"tags\":[],\"tags_total\":0,\"earliest_tag\":null,\"mirrored_at\":null,\"notes\":[]}\n"
    );
    assert_snapshot!(ran);
}

/// A message line is cut at 500 columns; CRLF ends and whitespace-only lines print as blank lines.
#[test]
fn message_with_long_line_and_blank_lines() {
    let first = json!({
        "data": {
            "r0": {
                "commit": {
                    "author": {
                        "date": "2026-09-09T20:14:26Z",
                        "person": {"displayName": "Jonathan Davies"},
                    },
                    "committer": {"date": "2026-09-10T01:02:03Z", "person": {"displayName": "GitHub"}},
                    "message": "Subject\r\n\r\nfirst body line\r\n  \t\r\n\r\nxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\r\nlast\n",
                    "oid": "affe2b9b7c608f4e354d7e83d0583ed35e845650",
                    "parents": [{"oid": "6b668aa526afdc623c1a9ed1d6ae920e04a717ad"}],
                    "subject": "Subject",
                    "w": {
                        "commit": {
                            "diff": {
                                "all": {"diffStat": {"added": 1, "deleted": 0}, "totalCount": 1},
                                "page": {
                                    "nodes": [
                                        {
                                            "newPath": "a.go",
                                            "oldPath": "a.go",
                                            "stat": {"added": 1, "deleted": 0},
                                        },
                                    ],
                                    "pageInfo": {"hasNextPage": false},
                                },
                            },
                        },
                    },
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    let second = json!({"data": {"repository": {"commit": {"tags": {"nodes": []}}}}});
    assert_snapshot!(answered(&["show", "example.com/acme/widget@v1"], [first, second]));
}

/// In JSON the message keeps its full lines, with LF ends.
#[test]
fn message_with_long_line_and_blank_lines_json() {
    let first = json!({
        "data": {
            "r0": {
                "commit": {
                    "author": {
                        "date": "2026-09-09T20:14:26Z",
                        "person": {"displayName": "Jonathan Davies"},
                    },
                    "committer": {"date": "2026-09-10T01:02:03Z", "person": {"displayName": "GitHub"}},
                    "message": "Subject\r\n\r\nfirst body line\r\n  \t\r\n\r\nxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\r\nlast\n",
                    "oid": "affe2b9b7c608f4e354d7e83d0583ed35e845650",
                    "parents": [{"oid": "6b668aa526afdc623c1a9ed1d6ae920e04a717ad"}],
                    "subject": "Subject",
                    "w": {
                        "commit": {
                            "diff": {
                                "all": {"diffStat": {"added": 1, "deleted": 0}, "totalCount": 1},
                                "page": {
                                    "nodes": [
                                        {
                                            "newPath": "a.go",
                                            "oldPath": "a.go",
                                            "stat": {"added": 1, "deleted": 0},
                                        },
                                    ],
                                    "pageInfo": {"hasNextPage": false},
                                },
                            },
                        },
                    },
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    let second = json!({"data": {"repository": {"commit": {"tags": {"nodes": []}}}}});
    assert_snapshot!(answered(&["show", "--json", "example.com/acme/widget@v1"], [first, second]));
}

/// Trailing blank lines of a message print as one empty line, without trailing spaces.
#[test]
fn message_with_trailing_blank_lines() {
    let first = json!({
        "data": {
            "r0": {
                "commit": {
                    "author": {
                        "date": "2026-09-09T20:14:26Z",
                        "person": {"displayName": "Jonathan Davies"},
                    },
                    "committer": {"date": "2026-09-10T01:02:03Z", "person": {"displayName": "GitHub"}},
                    "message": "Subject\n\n\nbody\n\n",
                    "oid": "affe2b9b7c608f4e354d7e83d0583ed35e845650",
                    "parents": [{"oid": "6b668aa526afdc623c1a9ed1d6ae920e04a717ad"}],
                    "subject": "Subject",
                    "w": {
                        "commit": {
                            "diff": {
                                "all": {"diffStat": {"added": 0, "deleted": 0}, "totalCount": 0},
                                "page": {"nodes": [], "pageInfo": {"hasNextPage": false}},
                            },
                        },
                    },
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    let second = json!({"data": {"repository": {"commit": {"tags": {"nodes": []}}}}});
    assert_snapshot!(answered(&["show", "example.com/acme/widget@v1"], [first, second]));
}

/// A whitespace-only line of a message is blank; a line of U+2028 is escaped.
#[test]
fn message_with_whitespace_only_lines() {
    let first = json!({
        "data": {
            "r0": {
                "commit": {
                    "author": {
                        "date": "2026-09-09T20:14:26Z",
                        "person": {"displayName": "Jonathan Davies"},
                    },
                    "committer": {"date": "2026-09-10T01:02:03Z", "person": {"displayName": "GitHub"}},
                    "message": "A\n \t \n\u{2028}\nB",
                    "oid": "affe2b9b7c608f4e354d7e83d0583ed35e845650",
                    "parents": [{"oid": "6b668aa526afdc623c1a9ed1d6ae920e04a717ad"}],
                    "subject": "A",
                    "w": {
                        "commit": {
                            "diff": {
                                "all": {"diffStat": {"added": 0, "deleted": 0}, "totalCount": 0},
                                "page": {"nodes": [], "pageInfo": {"hasNextPage": false}},
                            },
                        },
                    },
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    let second = json!({"data": {"repository": {"commit": {"tags": {"nodes": []}}}}});
    assert_snapshot!(answered(&["show", "example.com/acme/widget@v1"], [first, second]));
}

/// A missing revision is an error.
#[test]
fn missing_revision() {
    assert_snapshot!(sego(&["show", "github.com/gorilla/mux@deadbeefdeadbeef"]));
}

/// A missing revision is an error in JSON too.
#[test]
fn missing_revision_json() {
    assert_snapshot!(sego(&["show", "--json", "github.com/gorilla/mux@deadbeefdeadbeef"]));
}

/// A commit that no tag holds says so, though a tag was created after its author date.
#[test]
fn no_tag_holds_the_commit() {
    assert_snapshot!(sego(&["show", "github.com/gorilla/mux@374bd703c990"]));
}

/// The server's message for a rejected request is escaped in the error.
#[test]
fn rejected_request_with_control_characters() {
    let response = json!({
        "data": {},
        "errors": [{"message": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"}],
    });
    assert_snapshot!(answered(&["show", "example.com/acme/widget@v2.0.0"], [response]));
}

/// A failed repository lookup fails the commit, and the server's text is not printed.
/// No recording shows these located errors: a server failure sourcegraph.com doesn't give on demand.
#[test]
fn repository_lookup_error() {
    let response = json!({
        "data": {
            "r0": {
                "commit": null,
                "defaultBranch": null,
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
        "errors": [{"message": "secret", "path": ["r0", "defaultBranch"]}],
    });
    assert_snapshot!(answered(&["show", "example.com/acme/widget@v1"], [response]));
}

/// A repository Sourcegraph does not hold is an error.
#[test]
fn repository_not_on_sourcegraph() {
    assert_snapshot!(sego(&["show", "github.com/facebook/react@main"]));
}

/// A repository with no commits is an error.
#[test]
fn repository_with_no_commits() {
    assert_snapshot!(sego(&["show", "github.com/ascoders/blog@main"]));
}

/// A revision the server says is not found after waiting for a fetch, which the request's other part started, is named as missing.
/// Located errors as in `diff::missing_both_revisions`.
#[test]
fn revision_not_found_after_waiting_for_a_fetch() {
    let response = json!({
        "data": {
            "r0": {
                "commit": null,
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
        "errors": [
            {
                "message": "revision not found after waiting for a fetch already in progress: repo 1",
                "path": ["r0", "commit"],
            },
        ],
    });
    assert_snapshot!(answered(&["show", "example.com/acme/widget@v1"], [response]));
}

/// A revision with control characters is escaped in the usage error.
#[test]
fn revision_with_control_characters() {
    assert_snapshot!(sego(&[
        "show",
        "example.com/acme/widget@a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"
    ]));
}

/// An abbreviated commit has no rev field in the header.
#[test]
fn revision_written_as_abbreviated_commit() {
    assert_snapshot!(sego(&["show", "github.com/gorilla/mux@948bec3"]));
}

/// A branch is echoed in the header's rev field.
#[test]
fn revision_written_as_branch() {
    assert_snapshot!(sego(&["show", "github.com/gorilla/mux@main"]));
}

/// A root commit has no diff.
#[test]
fn root_commit() {
    assert_snapshot!(sego(&[
        "show",
        "github.com/gorilla/mux@4e8d10dc715feb79b43f1d607b45148537546a0a"
    ]));
}

/// A root commit's JSON has no parents, files or totals.
#[test]
fn root_commit_json() {
    assert_snapshot!(sego(&[
        "show",
        "--json",
        "github.com/gorilla/mux@4e8d10dc715feb79b43f1d607b45148537546a0a"
    ]));
}

/// A root commit whose file list errors prints without it, with a note.
/// A located error from `GqlError::kind`'s table: git exit 128.
#[test]
fn root_commit_with_file_list_error() {
    let first = json!({
        "data": {
            "r0": {
                "commit": {
                    "author": {
                        "date": "2020-05-04T10:00:00Z",
                        "person": {"displayName": "Ada Lovelace"},
                    },
                    "committer": {
                        "date": "2020-05-04T10:00:00Z",
                        "person": {"displayName": "Ada Lovelace"},
                    },
                    "message": "Start the project\n\nThe first commit.\n",
                    "oid": "9f2c1d3e5a7b4c6d8e0f1a2b3c4d5e6f70819a2b",
                    "parents": [],
                    "subject": "Start the project",
                    "w": null,
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
        "errors": [
            {
                "message": "revision not found: repo 1 @9f2c1d3e5a7b4c6d8e0f1a2b3c4d5e6f70819a2b~^0",
                "path": ["r0", "commit", "w", "commit", "diff"],
            },
        ],
    });
    let second = json!({"data": {"repository": {"commit": {"tags": {"nodes": []}}}}});
    assert_snapshot!(answered(&["show", "example.com/acme/widget@v1.0.0"], [first, second]));
}

/// A commit of example.com/acme/widget with one changed file, as the commit
/// lookup answers it.
fn widget_commit() -> serde_json::Value {
    json!({
        "data": {
            "r0": {
                "commit": {
                    "author": {
                        "date": "2026-09-09T20:14:26Z",
                        "person": {"displayName": "Jonathan Davies"},
                    },
                    "committer": {"date": "2026-09-10T01:02:03Z", "person": {"displayName": "GitHub"}},
                    "message": "Add the parser",
                    "oid": "affe2b9b7c608f4e354d7e83d0583ed35e845650",
                    "parents": [{"oid": "6b668aa526afdc623c1a9ed1d6ae920e04a717ad"}],
                    "subject": "Add the parser",
                    "w": {
                        "commit": {
                            "diff": {
                                "all": {"diffStat": {"added": 1, "deleted": 1}, "totalCount": 1},
                                "page": {
                                    "nodes": [
                                        {
                                            "newPath": "a.go",
                                            "oldPath": "a.go",
                                            "stat": {"added": 1, "deleted": 1},
                                        },
                                    ],
                                    "pageInfo": {"hasNextPage": false},
                                },
                            },
                        },
                    },
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    })
}

/// The tag list's answer when the server fails to read it.
fn failed_tag_list() -> serde_json::Value {
    json!({
        "data": {"repository": {"commit": null}},
        "errors": [
            {
                "message": "git command [git tag --list --contains] failed",
                "path": ["repository", "commit", "tags"],
            },
        ],
    })
}

/// A failed tag list leaves the tags out, with a note, and the commit and its files still print.
/// No recording shows these located errors: a server failure sourcegraph.com doesn't give on demand.
#[test]
fn tag_list_error_leaves_the_tags_out() {
    let responses = [widget_commit(), failed_tag_list()];
    assert_snapshot!(answered(&["show", "example.com/acme/widget@v1"], responses));
}

/// JSON for a failed tag list holds null tags, count and earliest tag.
#[test]
fn tag_list_error_leaves_the_tags_out_json() {
    let responses = [widget_commit(), failed_tag_list()];
    assert_snapshot!(answered(&["show", "--json", "example.com/acme/widget@v1"], responses));
}

/// A tag list the server cut leaves the earliest tag unknown, never a wrong one.
#[test]
fn a_tag_list_the_server_cut_leaves_the_earliest_tag_unknown() {
    let nodes: Vec<serde_json::Value> =
        (0..120).rev().map(|n| json!({"displayName": format!("v1.{n}.0")})).collect();
    let tags = json!({
        "data": {
            "repository": {
                "commit": {
                    "tags": {"totalCount": 150, "pageInfo": {"hasNextPage": true}, "nodes": nodes},
                },
            },
        },
    });
    let text = answered(&["show", "example.com/acme/widget@v1"], [widget_commit(), tags.clone()]);
    assert_snapshot!(text);
    let json = answered(&["show", "--json", "example.com/acme/widget@v1"], [widget_commit(), tags]);
    assert!(json.to_string().contains("\"earliest_tag\": null"), "{json}");
}

/// Text prints the first 100 tags that hold a commit, says how many there are, and
/// names the tag created first.
#[test]
fn tags_past_the_default_limit() {
    assert_snapshot!(sego(&["show", "github.com/kubernetes/kubernetes@1dc05009"]));
}

/// A commit lists the tags that hold it, newest created first.
#[test]
fn tags_that_hold_the_commit() {
    assert_snapshot!(sego(&["show", "github.com/gorilla/mux@946b6237eb8d"]));
}

/// JSON carries the tags that hold the commit and their count.
#[test]
fn tags_that_hold_the_commit_json() {
    assert_snapshot!(sego(&["show", "--json", "github.com/gorilla/mux@946b6237eb8d"]));
}
