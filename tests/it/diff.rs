//! `sego diff`.

use insta::assert_snapshot;
use serde_json::json;

use crate::common::{answered, sego};

/// A range with a tag on one side keeps its rev field.
#[test]
fn abbreviated_commit_base() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@a796238...v1.7.1"]));
}

/// A range between abbreviated commits has no rev field.
#[test]
fn abbreviated_commit_range() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@a796238...c5c6c98"]));
}

/// A revision written as an abbreviated commit has no rev field.
#[test]
fn abbreviated_commit_revision() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@c5c6c98"]));
}

/// The base is the commit of the first file that existed before.
#[test]
fn base_from_first_existing_file() {
    let first = json!({
        "data": {
            "r0": {
                "a": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                "b": {"oid": "89abcdef0123456789abcdef0123456789abcdef"},
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    let second = json!({
        "data": {
            "a": {"commit": {"oid": "0123456789abcdef0123456789abcdef01234567"}},
            "b": {"commit": {"oid": "89abcdef0123456789abcdef0123456789abcdef"}},
            "c": {
                "comparison": {
                    "all": {"diffStat": {"added": 5, "deleted": 2}, "totalCount": 3},
                    "page": {
                        "nodes": [
                            {
                                "newPath": "a.go",
                                "oldFile": null,
                                "oldPath": null,
                                "stat": {"added": 3, "deleted": 0},
                            },
                            {
                                "newPath": "b.go",
                                "oldFile": {
                                    "commit": {"oid": "fedcba9876543210fedcba9876543210fedcba98"},
                                },
                                "oldPath": "b.go",
                                "stat": {"added": 1, "deleted": 1},
                            },
                            {
                                "newPath": "c.go",
                                "oldFile": {
                                    "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                },
                                "oldPath": "c.go",
                                "stat": {"added": 1, "deleted": 1},
                            },
                        ],
                        "pageInfo": {"hasNextPage": false},
                    },
                    "range": {"mergeBase": null},
                },
            },
        },
    });
    assert_snapshot!(answered(&["diff", "example.com/acme/widget@v1...v2"], [first, second]));
}

/// A binary file shows in the list and in its patch.
#[test]
fn binary_file() {
    assert_snapshot!(sego(&[
        "diff",
        "github.com/BurntSushi/cargo-benchcmp@cf84e27337f5c671f622330ed2ee1e36584d5c2e",
        "coloured_output_example.png"
    ]));
}

/// A comparison between two commit ids has no rev field in its header.
#[test]
fn commit_ids_have_no_rev_field() {
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
                    "all": {"diffStat": {"added": 3, "deleted": 1}, "totalCount": 1},
                    "page": {
                        "nodes": [
                            {
                                "newPath": "widget.go",
                                "oldFile": {
                                    "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                },
                                "oldPath": "widget.go",
                                "stat": {"added": 3, "deleted": 1},
                            },
                        ],
                        "pageInfo": {"hasNextPage": false},
                    },
                    "range": {"mergeBase": null},
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["diff", "example.com/acme/widget@0123456789ab...456789abcdef"],
        [first, second]
    ));
}

/// A web URL is not an address: the error names the forms that are.
#[test]
fn compare_url_is_not_an_address() {
    assert_snapshot!(sego(&["diff", "https://github.com/gorilla/mux/compare/v1.7.0...v1.7.1"]));
}

/// A comparison the server leaves null, without an error, is a failure.
#[test]
fn comparison_without_answer() {
    let first = json!({
        "data": {
            "r0": {
                "a": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                "b": {"oid": "89abcdef0123456789abcdef0123456789abcdef"},
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    let second = json!({
        "data": {
            "a": {"commit": {"oid": "0123456789abcdef0123456789abcdef01234567"}},
            "b": {"commit": {"oid": "89abcdef0123456789abcdef0123456789abcdef"}},
            "c": null,
        },
    });
    assert_snapshot!(answered(&["diff", "example.com/acme/widget@v1...v2"], [first, second]));
}

/// A range between two release lines is compared from their merge base, which is neither end.
#[test]
fn divergent_range_base_is_the_merge_base() {
    assert_snapshot!(sego(&["diff", "github.com/golang/go@go1.21.1...go1.22.0", "VERSION"]));
}

/// A comparison of a revision with itself lists no files.
#[test]
fn empty_comparison() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@v1.7.0...v1.7.0"]));
}

/// In JSON an empty comparison has no files and no base.
#[test]
fn empty_comparison_json() {
    assert_snapshot!(sego(&["diff", "--json", "github.com/gorilla/mux@v1.7.0...v1.7.0"]));
}

/// When every file was added, the base is the merge base all the same.
#[test]
fn every_file_added_takes_the_merge_base() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@v1.6.2...v1.7.0", ".github"]));
}

/// In JSON the base is the merge base when every file was added.
#[test]
fn every_file_added_takes_the_merge_base_json() {
    assert_snapshot!(sego(&[
        "diff",
        "--json",
        "github.com/gorilla/mux@v1.6.2...v1.7.0",
        ".github"
    ]));
}

/// A failed comparison is named, and the server's text is not printed.
/// A located error from `GqlError::kind`'s table: git exit 128.
#[test]
fn failed_comparison() {
    let first = json!({
        "data": {
            "r0": {
                "a": {"oid": "0a1b2c3d4e5f60718293a4b5c6d7e8f901234567"},
                "b": {"oid": "76543210fedcba98765432100123456789abcdef"},
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    let second = json!({
        "data": {
            "a": {"commit": {"oid": "0a1b2c3d4e5f60718293a4b5c6d7e8f901234567"}},
            "b": {"commit": {"oid": "76543210fedcba98765432100123456789abcdef"}},
            "c": null,
        },
        "errors": [
            {
                "message": "git command [git diff --raw -z --find-renames --secret] failed",
                "path": ["c", "comparison", "page", "nodes"],
            },
            {
                "message": "git command [git diff --numstat -z --secret] failed",
                "path": ["c", "comparison", "all", "diffStat"],
            },
            {
                "message": "git command [git diff --numstat -z --secret] failed",
                "path": ["c", "comparison", "all", "totalCount"],
            },
        ],
    });
    assert_snapshot!(answered(&["diff", "example.com/acme/widget@v1...v2"], [first, second]));
}

/// A path lookup that fails is a failure, not a path that is missing.
/// A located error from `GqlError::kind`'s table: git exit 128.
#[test]
fn failed_path_lookup_is_not_a_missing_path() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "author": {
                        "date": "2017-05-17T12:08:25Z",
                        "person": {"displayName": "Guillaume Fraux"},
                    },
                    "committer": {
                        "date": "2017-05-17T12:39:43Z",
                        "person": {"displayName": "Andrew Gallant"},
                    },
                    "message": "Update README examples",
                    "oid": "cf84e27337f5c671f622330ed2ee1e36584d5c2e",
                    "p0": null,
                    "parents": [{"oid": "23d04701228d6b4aed46bf8d48e58dfecc338e25", "p0": null}],
                    "subject": "Update README examples",
                    "w": {
                        "commit": {
                            "diff": {
                                "all": {"diffStat": {"added": 0, "deleted": 0}, "totalCount": 1},
                                "page": {
                                    "nodes": [
                                        {
                                            "newPath": "coloured_output_example.png",
                                            "oldFile": {
                                                "commit": {
                                                    "oid": "23d04701228d6b4aed46bf8d48e58dfecc338e25",
                                                },
                                            },
                                            "oldPath": "coloured_output_example.png",
                                            "stat": {"added": 0, "deleted": 0},
                                        },
                                    ],
                                    "pageInfo": {"hasNextPage": false},
                                },
                                "patch": {
                                    "rawDiff": "diff --git coloured_output_example.png coloured_output_example.png\nindex a224ed001017471d230d80849a38941277c192a3..600d1940bcf9ff652b8a5615f9deba46c57170ad 100644\nBinary files coloured_output_example.png and coloured_output_example.png differ\n",
                                },
                            },
                        },
                    },
                },
                "defaultBranch": {"displayName": "master"},
                "mirrorInfo": {"updatedAt": "2026-10-27T01:05:32Z"},
                "name": "github.com/BurntSushi/cargo-benchcmp",
            },
        },
        "errors": [
            {
                "message": "rpc error: code = Internal desc = git command failed",
                "path": ["r0", "commit", "p0"],
            },
            {
                "message": "rpc error: code = Internal desc = git command failed",
                "path": ["r0", "commit", "parents", 0, "p0"],
            },
        ],
    });
    assert_snapshot!(answered(
        &[
            "diff",
            "github.com/BurntSushi/cargo-benchcmp@cf84e27337f5c671f622330ed2ee1e36584d5c2e",
            "coloured_output_example.png"
        ],
        [response]
    ));
}

/// A file added, deleted or changed has its status letter.
#[test]
fn file_statuses() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@v1.7.2...v1.7.3"]));
}

/// A glob is quoted in the header.
#[test]
fn glob_path() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@v1.7.0...v1.7.1", "*.go"]));
}

/// Globs and pathspec magic are sent as given, with no check that they exist.
#[test]
fn magic_paths() {
    assert_snapshot!(sego(&[
        "diff",
        "github.com/gorilla/mux@v1.7.0...v1.7.1",
        "*.go",
        ":!mux_test.go",
        "a?.go",
        "src/[ab]/x",
        ":(glob)**/*.go"
    ]));
}

/// A missing base is named.
#[test]
fn missing_base_revision() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@v9.9.9...v1.7.1"]));
}

/// Both ends missing, each of 4 to 39 hex digits, says what neither can be.
#[test]
fn missing_both_hex_revisions() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@dead...beef"]));
}

/// Both missing revisions are named together.
#[test]
fn missing_both_revisions() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@v9.9.8...v9.9.9"]));
}

/// Both ends missing, one of 4 to 39 hex digits, says what that one can't be.
#[test]
fn missing_both_revisions_one_hex() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@dead...v9.9.9"]));
}

/// A missing path written with a trailing slash is not found.
#[test]
fn missing_directory_with_a_trailing_slash() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@v1.7.4...v1.8.0", "nope/"]));
}

/// A missing head is named.
#[test]
fn missing_head_revision() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@v1.7.0...v9.9.9"]));
}

/// A missing path is reported before a failed comparison.
/// No recording shows these located errors: a server failure sourcegraph.com doesn't give on demand.
#[test]
fn missing_path_with_comparison_error() {
    let first = json!({
        "data": {
            "r0": {
                "a": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                "b": {"oid": "89abcdef0123456789abcdef0123456789abcdef"},
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    let second = json!({
        "data": {
            "a": {"commit": {"oid": "0123456789abcdef0123456789abcdef01234567", "p0": null}},
            "b": {"commit": {"oid": "89abcdef0123456789abcdef0123456789abcdef", "p0": null}},
            "c": null,
        },
        "errors": [{"message": "x", "path": ["c", "comparison"]}],
    });
    assert_snapshot!(answered(
        &["diff", "example.com/acme/widget@v1...v2", "nope.go"],
        [first, second]
    ));
}

/// A missing path with control characters, given as the argument, is escaped in the error.
#[test]
fn missing_path_with_control_characters() {
    let first = json!({
        "data": {
            "r0": {
                "a": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                "b": {"oid": "89abcdef0123456789abcdef0123456789abcdef"},
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    let second = json!({
        "data": {
            "a": {"commit": {"oid": "0123456789abcdef0123456789abcdef01234567", "p0": null}},
            "b": {"commit": {"oid": "89abcdef0123456789abcdef0123456789abcdef", "p0": null}},
            "c": {
                "comparison": {
                    "all": {"diffStat": {"added": 0, "deleted": 0}, "totalCount": 0},
                    "page": {"nodes": [], "pageInfo": {"hasNextPage": false}},
                    "patch": {"rawDiff": ""},
                    "range": {"mergeBase": null},
                },
            },
        },
    });
    assert_snapshot!(answered(
        &[
            "diff",
            "example.com/acme/widget@v1.0.0...v2.0.0",
            "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i-gone"
        ],
        [first, second]
    ));
}

/// A repository that is not on the server is named.
#[test]
fn missing_repository() {
    assert_snapshot!(sego(&["diff", "example.com/acme/gadget@v1...v2"]));
}

/// A revision that names no commit is named.
#[test]
fn missing_revision() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@v9.9"]));
}

/// Plain paths and a glob are listed in the header, and each plain path is checked.
#[test]
fn mixed_paths() {
    assert_snapshot!(sego(&[
        "diff",
        "github.com/gorilla/mux@v1.7.0...v1.7.1",
        "mux.go",
        "*.md",
        "route.go"
    ]));
}

/// OWNER/NAME looks for the repository on every host.
#[test]
fn owner_name() {
    assert_snapshot!(sego(&["diff", "gorilla/mux@v1.7.0...v1.7.1"]));
}

/// A patch is printed as served, indented, whatever its lines look like.
#[test]
fn patch_lines_that_look_like_headers() {
    let first = json!({
        "data": {
            "r0": {
                "a": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                "b": {"oid": "89abcdef0123456789abcdef0123456789abcdef"},
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    let second = json!({
        "data": {
            "a": {
                "commit": {
                    "oid": "0123456789abcdef0123456789abcdef01234567",
                    "p0": {"__typename": "GitBlob"},
                },
            },
            "b": {
                "commit": {
                    "oid": "89abcdef0123456789abcdef0123456789abcdef",
                    "p0": {"__typename": "GitBlob"},
                },
            },
            "c": {
                "comparison": {
                    "all": {"diffStat": {"added": 2, "deleted": 1}, "totalCount": 1},
                    "page": {
                        "nodes": [
                            {
                                "newPath": "dir/a b.go",
                                "oldFile": {
                                    "commit": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                                },
                                "oldPath": "dir/a b.go",
                                "stat": {"added": 2, "deleted": 1},
                            },
                        ],
                        "pageInfo": {"hasNextPage": false},
                    },
                    "patch": {
                        "rawDiff": "diff --git dir/a b.go dir/a b.go\nindex 1111111..2222222 100644\n--- dir/a b.go\n+++ dir/a b.go\n@@ -1,2 +1,2 @@\n--- foo\n+-- bar\n ctx\n+red\u{1b}[31m\n\\ No newline at end of file\n",
                    },
                    "range": {"mergeBase": null},
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["diff", "example.com/acme/widget@v1...v2", "dir/a b.go"],
        [first, second]
    ));
}

/// A path with control characters, given as the argument, is escaped in the header, the row and the patch.
#[test]
fn patch_with_control_characters() {
    let first = json!({
        "data": {
            "r0": {
                "a": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                "b": {"oid": "89abcdef0123456789abcdef0123456789abcdef"},
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    let second = json!({
        "data": {
            "a": {
                "commit": {
                    "oid": "0123456789abcdef0123456789abcdef01234567",
                    "p0": {"__typename": "GitBlob"},
                },
            },
            "b": {
                "commit": {
                    "oid": "89abcdef0123456789abcdef0123456789abcdef",
                    "p0": {"__typename": "GitBlob"},
                },
            },
            "c": {
                "comparison": {
                    "all": {"diffStat": {"added": 1, "deleted": 1}, "totalCount": 1},
                    "page": {
                        "nodes": [
                            {
                                "newPath": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                "oldFile": {
                                    "commit": {"oid": "fedcba9876543210fedcba9876543210fedcba98"},
                                },
                                "oldPath": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                "stat": {"added": 1, "deleted": 1},
                            },
                        ],
                        "pageInfo": {"hasNextPage": false},
                    },
                    "patch": {
                        "rawDiff": "diff --git a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i\n--- a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i\n+++ a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i\n@@ -1 +1 @@\n-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i\n+a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i\n",
                    },
                    "range": {"mergeBase": null},
                },
            },
        },
    });
    assert_snapshot!(answered(
        &[
            "diff",
            "example.com/acme/widget@v1.0.0...v2.0.0",
            "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"
        ],
        [first, second]
    ));
}

/// In JSON the patch and the path hold the characters themselves.
#[test]
fn patch_with_control_characters_json() {
    let first = json!({
        "data": {
            "r0": {
                "a": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                "b": {"oid": "89abcdef0123456789abcdef0123456789abcdef"},
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    let second = json!({
        "data": {
            "a": {
                "commit": {
                    "oid": "0123456789abcdef0123456789abcdef01234567",
                    "p0": {"__typename": "GitBlob"},
                },
            },
            "b": {
                "commit": {
                    "oid": "89abcdef0123456789abcdef0123456789abcdef",
                    "p0": {"__typename": "GitBlob"},
                },
            },
            "c": {
                "comparison": {
                    "all": {"diffStat": {"added": 1, "deleted": 1}, "totalCount": 1},
                    "page": {
                        "nodes": [
                            {
                                "newPath": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                "oldFile": {
                                    "commit": {"oid": "fedcba9876543210fedcba9876543210fedcba98"},
                                },
                                "oldPath": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                "stat": {"added": 1, "deleted": 1},
                            },
                        ],
                        "pageInfo": {"hasNextPage": false},
                    },
                    "patch": {
                        "rawDiff": "diff --git a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i\n--- a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i\n+++ a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i\n@@ -1 +1 @@\n-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i\n+a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i\n",
                    },
                    "range": {"mergeBase": null},
                },
            },
        },
    });
    let ran = answered(
        &[
            "diff",
            "--json",
            "example.com/acme/widget@v1.0.0...v2.0.0",
            "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
        ],
        [first, second],
    );
    assert_eq!(
        ran.stdout,
        "{\"repo\":\"example.com/acme/widget\",\"rev_a\":\"v1.0.0\",\"rev_b\":\"v2.0.0\",\"commit_a\":\"0123456789abcdef0123456789abcdef01234567\",\"commit_b\":\"89abcdef0123456789abcdef0123456789abcdef\",\"base\":\"fedcba9876543210fedcba9876543210fedcba98\",\"files_total\":1,\"files_more\":false,\"added\":1,\"deleted\":1,\"files\":[{\"status\":\"M\",\"old_path\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"new_path\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"added\":1,\"deleted\":1}],\"paths\":[\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\"],\"patch\":\"diff --git a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\\n--- a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\\n+++ a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\\n@@ -1 +1 @@\\n-a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\\n+a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\\n\",\"mirrored_at\":null,\"notes\":[]}\n"
    );
    assert_snapshot!(ran);
}

/// A path at the head only is compared.
#[test]
fn path_added_in_range() {
    assert_snapshot!(sego(&[
        "diff",
        "github.com/gorilla/mux@v1.7.2...v1.7.3",
        ".circleci/config.yml"
    ]));
}

/// A path at the base only is compared.
#[test]
fn path_deleted_in_range() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@v1.7.2...v1.7.3", ".travis.yml"]));
}

/// A path in the address is a path to compare.
#[test]
fn path_in_address() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@v1.7.0...v1.7.1:mux.go"]));
}

/// The same path in the address and as an operand is named once.
#[test]
fn path_in_address_and_operand() {
    assert_snapshot!(sego(&[
        "diff",
        "github.com/gorilla/mux@v1.7.0...v1.7.1:mux.go",
        "mux.go",
        "mux.go"
    ]));
}

/// A path at neither end is an error.
#[test]
fn path_missing_at_both_ends() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@v1.7.0...v1.7.1", "nope.go"]));
}

/// A path operand that leaves the repository's root names nothing.
#[test]
fn path_that_leaves_the_root() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@v1.8.0...v1.8.1", "../mux.go"]));
}

/// A path with spaces is quoted in the output.
#[test]
fn path_with_spaces() {
    assert_snapshot!(sego(&[
        "diff",
        "github.com/torvalds/linux@0debb20c5c812f8750c20c3406bc94a1e8ea4742",
        "tools/testing/selftests/devices/probe/boards/Dell Inc.,XPS 13 9300.yaml"
    ]));
}

/// A path in the address and an operand are listed in the header.
#[test]
fn paths_in_address_and_operand() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@v1.7.0...v1.7.1:mux.go", "route.go"]));
}

/// File paths with control characters are escaped in the rows.
#[test]
fn paths_with_control_characters() {
    let first = json!({
        "data": {
            "r0": {
                "a": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                "b": {"oid": "89abcdef0123456789abcdef0123456789abcdef"},
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    let second = json!({
        "data": {
            "a": {"commit": {"oid": "0123456789abcdef0123456789abcdef01234567"}},
            "b": {"commit": {"oid": "89abcdef0123456789abcdef0123456789abcdef"}},
            "c": {
                "comparison": {
                    "all": {"diffStat": {"added": 7, "deleted": 7}, "totalCount": 4},
                    "page": {
                        "nodes": [
                            {
                                "newPath": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                "oldFile": {
                                    "commit": {"oid": "fedcba9876543210fedcba9876543210fedcba98"},
                                },
                                "oldPath": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                "stat": {"added": 1, "deleted": 1},
                            },
                            {
                                "newPath": "n-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                "oldFile": {
                                    "commit": {"oid": "fedcba9876543210fedcba9876543210fedcba98"},
                                },
                                "oldPath": "o-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                "stat": {"added": 2, "deleted": 2},
                            },
                            {
                                "newPath": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i-added",
                                "oldFile": null,
                                "oldPath": null,
                                "stat": {"added": 4, "deleted": 0},
                            },
                            {
                                "newPath": null,
                                "oldFile": {
                                    "commit": {"oid": "fedcba9876543210fedcba9876543210fedcba98"},
                                },
                                "oldPath": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i-deleted",
                                "stat": {"added": 0, "deleted": 4},
                            },
                        ],
                        "pageInfo": {"hasNextPage": false},
                    },
                    "range": {"mergeBase": null},
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["diff", "example.com/acme/widget@v1.0.0...v2.0.0"],
        [first, second]
    ));
}

/// In JSON the file paths hold the characters themselves.
#[test]
fn paths_with_control_characters_json() {
    let first = json!({
        "data": {
            "r0": {
                "a": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                "b": {"oid": "89abcdef0123456789abcdef0123456789abcdef"},
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    let second = json!({
        "data": {
            "a": {"commit": {"oid": "0123456789abcdef0123456789abcdef01234567"}},
            "b": {"commit": {"oid": "89abcdef0123456789abcdef0123456789abcdef"}},
            "c": {
                "comparison": {
                    "all": {"diffStat": {"added": 7, "deleted": 7}, "totalCount": 4},
                    "page": {
                        "nodes": [
                            {
                                "newPath": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                "oldFile": {
                                    "commit": {"oid": "fedcba9876543210fedcba9876543210fedcba98"},
                                },
                                "oldPath": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                "stat": {"added": 1, "deleted": 1},
                            },
                            {
                                "newPath": "n-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                "oldFile": {
                                    "commit": {"oid": "fedcba9876543210fedcba9876543210fedcba98"},
                                },
                                "oldPath": "o-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                "stat": {"added": 2, "deleted": 2},
                            },
                            {
                                "newPath": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i-added",
                                "oldFile": null,
                                "oldPath": null,
                                "stat": {"added": 4, "deleted": 0},
                            },
                            {
                                "newPath": null,
                                "oldFile": {
                                    "commit": {"oid": "fedcba9876543210fedcba9876543210fedcba98"},
                                },
                                "oldPath": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i-deleted",
                                "stat": {"added": 0, "deleted": 4},
                            },
                        ],
                        "pageInfo": {"hasNextPage": false},
                    },
                    "range": {"mergeBase": null},
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["diff", "--json", "example.com/acme/widget@v1.0.0...v2.0.0"],
        [first, second]
    ));
}

/// A range whose base is missing is not read as a root commit.
#[test]
fn range_head_is_a_root_commit() {
    let response = json!({
        "data": {
            "r0": {
                "a": null,
                "b": {"oid": "89abcdef0123456789abcdef0123456789abcdef"},
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    assert_snapshot!(answered(&["diff", "example.com/acme/widget@v0...v1"], [response]));
}

/// A range with no shared history, whose files were all added, prints no base.
#[test]
fn range_with_no_shared_history_prints_no_base() {
    assert_snapshot!(sego(&["diff", "github.com/select2/select2@gh-pages...master", ".github"]));
}

/// In JSON the base is null for a range with no shared history whose files were all added.
#[test]
fn range_with_no_shared_history_prints_no_base_json() {
    assert_snapshot!(sego(&[
        "diff",
        "--json",
        "github.com/select2/select2@gh-pages...master",
        ".github"
    ]));
}

/// JSON carries the files and the patch of a named path.
#[test]
fn range_with_path_json() {
    assert_snapshot!(sego(&[
        "diff",
        "--json",
        "github.com/gorilla/mux@v1.6.2...v1.7.0",
        "route.go"
    ]));
}

/// The server's message for a rejected request is escaped in the error.
#[test]
fn rejected_request_with_control_characters() {
    let response = json!({
        "data": {},
        "errors": [{"message": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"}],
    });
    assert_snapshot!(answered(&["diff", "example.com/acme/widget@v1.0.0...v2.0.0"], [response]));
}

/// Both paths together recognise a rename.
#[test]
fn rename_from_two_paths() {
    assert_snapshot!(sego(&[
        "diff",
        "github.com/evilmartians/lefthook@v2.0.0...v2.1.0",
        "internal/run/controller/filters/detect_text.go",
        "internal/run/controller/filter/detect_text.go"
    ]));
}

/// A repository with no commits is named.
#[test]
fn repository_without_commits() {
    let response = json!({
        "data": {
            "r0": {
                "commit": null,
                "defaultBranch": null,
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    assert_snapshot!(answered(&["diff", "example.com/acme/widget@v1"], [response]));
}

/// A root commit has no parent to diff against.
/// Located errors as in `commit::root_commit`.
#[test]
fn root_commit_has_no_parent() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "author": {"date": "2020-01-01T00:00:00Z", "person": {"displayName": ""}},
                    "committer": null,
                    "message": "",
                    "oid": "4e8d10dc715feb79b43f1d607b45148537546a0a",
                    "parents": [],
                    "subject": "",
                    "w": null,
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
        "errors": [
            {
                "message": "revision not found: repo 1 @4e8d10dc715feb79b43f1d607b45148537546a0a~^0",
                "path": ["r0", "commit", "w", "commit", "diff"],
            },
        ],
    });
    assert_snapshot!(answered(&["diff", "example.com/acme/widget@v0.1.0"], [response]));
}

/// A root commit has no diff on Sourcegraph, so diff of it is not found.
#[test]
fn root_commit_is_not_found() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@4e8d10dc715f"]));
}

/// Every plain path is checked, not only the first.
#[test]
fn second_path_missing() {
    let first = json!({
        "data": {
            "r0": {
                "a": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                "b": {"oid": "89abcdef0123456789abcdef0123456789abcdef"},
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    let second = json!({
        "data": {
            "a": {
                "commit": {
                    "oid": "0123456789abcdef0123456789abcdef01234567",
                    "p0": {"__typename": "GitBlob"},
                    "p1": null,
                },
            },
            "b": {
                "commit": {
                    "oid": "89abcdef0123456789abcdef0123456789abcdef",
                    "p0": {"__typename": "GitBlob"},
                    "p1": null,
                },
            },
            "c": {
                "comparison": {
                    "all": {"diffStat": {"added": 0, "deleted": 0}, "totalCount": 0},
                    "page": {"nodes": [], "pageInfo": {"hasNextPage": false}},
                    "patch": {"rawDiff": ""},
                    "range": {"mergeBase": null},
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["diff", "example.com/acme/widget@v1...v2", "here.go", "gone.go"],
        [first, second]
    ));
}

/// A revision alone is the change that commit made.
#[test]
fn single_revision() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@v1.8.0"]));
}

/// A revision alone has no `rev_a` in JSON.
#[test]
fn single_revision_json() {
    assert_snapshot!(sego(&["diff", "--json", "github.com/gorilla/mux@v1.8.0"]));
}

/// A comparison lists the changed files with totals and the merge base.
#[test]
fn tag_range() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@v1.7.0...v1.7.1"]));
}

/// A JSON comparison lists the files and has no patch.
#[test]
fn tag_range_json() {
    assert_snapshot!(sego(&["diff", "--json", "github.com/gorilla/mux@v1.7.0...v1.7.1"]));
}

/// A named path brings its patch, indented four spaces.
#[test]
fn tag_range_with_path() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@v1.7.0...v1.7.1", "mux.go"]));
}

/// A path that exists but did not change gives 0 files.
#[test]
fn unchanged_path() {
    assert_snapshot!(sego(&["diff", "github.com/gorilla/mux@v1.7.0...v1.7.1", "README.md"]));
}

/// In JSON an unchanged path has no files and an empty patch.
#[test]
fn unchanged_path_json() {
    assert_snapshot!(sego(&[
        "diff",
        "--json",
        "github.com/gorilla/mux@v1.7.0...v1.7.1",
        "README.md"
    ]));
}

/// With no shared history the base is A itself.
#[test]
fn unrelated_histories() {
    let first = json!({
        "data": {
            "r0": {
                "a": {"oid": "e7df16ff0ce8bbc7139db0368ac23853be65b1b8"},
                "b": {"oid": "cc9969c2e2198aafff8accc63ee1aee30e9a2aa4"},
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    let second = json!({
        "data": {
            "a": {"commit": {"oid": "e7df16ff0ce8bbc7139db0368ac23853be65b1b8"}},
            "b": {"commit": {"oid": "cc9969c2e2198aafff8accc63ee1aee30e9a2aa4"}},
            "c": {
                "comparison": {
                    "all": {"diffStat": {"added": 65, "deleted": 0}, "totalCount": 4},
                    "page": {
                        "nodes": [
                            {
                                "newPath": ".agents/rules/.gitkeep",
                                "oldFile": {
                                    "commit": {"oid": "e7df16ff0ce8bbc7139db0368ac23853be65b1b8"},
                                },
                                "oldPath": ".nojekyll",
                                "stat": {"added": 0, "deleted": 0},
                            },
                            {
                                "newPath": ".agents/rules/unit_tests.md",
                                "oldFile": null,
                                "oldPath": null,
                                "stat": {"added": 64, "deleted": 0},
                            },
                            {
                                "newPath": ".agents/skills/.gitkeep",
                                "oldFile": null,
                                "oldPath": null,
                                "stat": {"added": 0, "deleted": 0},
                            },
                            {
                                "newPath": ".claude/rules",
                                "oldFile": null,
                                "oldPath": null,
                                "stat": {"added": 1, "deleted": 0},
                            },
                        ],
                        "pageInfo": {"hasNextPage": false},
                    },
                    "range": {"mergeBase": null},
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["diff", "example.com/acme/widget@gh-pages...main"],
        [first, second]
    ));
}
