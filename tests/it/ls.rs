//! `sego ls`.

use insta::assert_snapshot;
use serde_json::json;

use crate::common::{answered, sego};

/// A clone URL is not an address.
#[test]
fn clone_url_is_not_an_address() {
    assert_snapshot!(sego(&["ls", "ssh://git@github.com/gorilla/mux.git"]));
}

/// With no revision the JSON rev is null.
#[test]
fn default_branch_json() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "meta": {
                        "__typename": "GitTree",
                        "entries": [
                            {"isDirectory": true, "path": "docs", "submodule": null},
                            {"isDirectory": false, "path": "mux.go", "submodule": null},
                        ],
                        "path": "",
                    },
                    "oid": "98cb6bf42e086f6af920b965c38cacc07402d51b",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
        },
    });
    assert_snapshot!(answered(&["ls", "--json", "example.com/acme/widget"], [response]));
}

/// A directory with no entries says so.
#[test]
fn empty_directory() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "meta": {"__typename": "GitTree", "entries": [], "path": "empty"},
                    "oid": "98cb6bf42e086f6af920b965c38cacc07402d51b",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
        },
    });
    assert_snapshot!(answered(&["ls", "example.com/acme/widget@v1.0.0:empty"], [response]));
}

/// Text prints 200 rows and says how many were fetched.
#[test]
fn entries_past_the_default_limit() {
    assert_snapshot!(sego(&[
        "ls",
        "github.com/golang/go@e3c3d2f9e7ca88e6f240fe00faa67efb03d851a1:src/cmd/go/testdata/script"
    ]));
}

/// A directory holding a file with a space lists it shell-quoted.
#[test]
fn entry_with_space_quoted_in_row() {
    assert_snapshot!(sego(&[
        "ls",
        "github.com/nodejs/node@aa1bf2cc37539bfeeef8faf80c9d31a64cb78026:test/fixtures"
    ]));
}

/// A listing with an error beneath the tree is not used.
/// A located error from `GqlError::kind`'s table: git exit 128.
#[test]
fn error_beneath_tree_entries_hides_server_text() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "meta": {
                        "__typename": "GitTree",
                        "entries": [{"isDirectory": false, "path": "docs/guide.md", "submodule": null}],
                        "path": "docs",
                    },
                    "oid": "1a2b3c4d5e6f708192a3b4c5d6e7f80910111213",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/partial",
            },
        },
        "errors": [
            {
                "message": "rpc error: code = Internal desc = git command failed on host gitserver-5.internal",
                "path": ["r0", "commit", "meta", "entries", 0, "submodule"],
            },
        ],
    });
    assert_snapshot!(answered(&["ls", "example.com/acme/partial@v3.0.0:docs"], [response]));
}

/// A file where a directory was asked for names the path, escaped, in the error.
#[test]
fn file_at_path_with_control_characters() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "meta": {
                        "__typename": "GitBlob",
                        "binary": false,
                        "byteSize": 40,
                        "lfs": null,
                        "path": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                        "rawURL": "/r/example.com/acme/widget@0123456789abcdef0123456789abcdef01234567/-/raw/a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                        "submodule": null,
                        "totalLines": 3,
                    },
                    "oid": "0123456789abcdef0123456789abcdef01234567",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
        },
    });
    assert_snapshot!(answered(
        &["ls", "example.com/acme/widget:a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"],
        [response]
    ));
}

/// A path that names a file lists the file as its one row.
#[test]
fn file_path_answers_with_its_row() {
    assert_snapshot!(sego(&["ls", "github.com/gorilla/mux@v1.8.0:mux.go"]));
}

/// In JSON a path that names a file is its one entry, kind file, with null submodule keys.
#[test]
fn file_path_answers_with_its_row_json() {
    assert_snapshot!(sego(&["ls", "--json", "github.com/gorilla/mux@v1.8.0:mux.go"]));
}

/// A host followed by a path names no repository.
#[test]
fn host_alone_is_not_a_repository() {
    assert_snapshot!(sego(&["ls", "github.com:gorilla/mux"]));
}

/// A failed directory listing names the part, not the server's text.
/// A located error from `GqlError::kind`'s table: git exit 128.
#[test]
fn listing_error_hides_server_text() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {"meta": null, "oid": "0a1b2c3d4e5f60718293a4b5c6d7e8f901234567"},
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/listing",
            },
        },
        "errors": [
            {
                "message": "rpc error: code = Internal desc = git command failed on host gitserver-3.internal",
                "path": ["r0", "commit", "meta", "entries"],
            },
        ],
    });
    assert_snapshot!(answered(&["ls", "example.com/acme/listing@v2.0.0:docs"], [response]));
}

/// A missing path is an error.
#[test]
fn missing_path() {
    assert_snapshot!(sego(&["ls", "github.com/gorilla/mux@v1.8.0:nope"]));
}

/// A missing path with control characters is escaped in the error.
#[test]
fn missing_path_with_control_characters() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {"meta": null, "oid": "0123456789abcdef0123456789abcdef01234567"},
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
        },
    });
    assert_snapshot!(answered(
        &["ls", "example.com/acme/widget:a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i-gone"],
        [response]
    ));
}

/// A path beneath a file is missing, not a listing failure.
#[test]
fn path_under_a_file() {
    assert_snapshot!(sego(&["ls", "github.com/gorilla/mux@v1.8.0:mux.go/x"]));
}

/// A path with control characters, given as the argument, is escaped in the header and the rows.
#[test]
fn path_with_control_characters() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "meta": {
                        "__typename": "GitTree",
                        "entries": [
                            {
                                "isDirectory": false,
                                "path": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i/a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                "submodule": null,
                            },
                            {
                                "isDirectory": true,
                                "path": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i/a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i-dir",
                                "submodule": null,
                            },
                            {
                                "isDirectory": false,
                                "path": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i/sub",
                                "submodule": {"commit": "0123456789abcdef0123456789abcdef01234567"},
                            },
                        ],
                        "path": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                    },
                    "oid": "0123456789abcdef0123456789abcdef01234567",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
        },
    });
    assert_snapshot!(answered(
        &["ls", "example.com/acme/widget:a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"],
        [response]
    ));
}

/// In JSON the path and the names hold the characters themselves.
#[test]
fn path_with_control_characters_json() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "meta": {
                        "__typename": "GitTree",
                        "entries": [
                            {
                                "isDirectory": false,
                                "path": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i/a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                "submodule": null,
                            },
                            {
                                "isDirectory": true,
                                "path": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i/a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i-dir",
                                "submodule": null,
                            },
                            {
                                "isDirectory": false,
                                "path": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i/sub",
                                "submodule": {"commit": "0123456789abcdef0123456789abcdef01234567"},
                            },
                        ],
                        "path": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                    },
                    "oid": "0123456789abcdef0123456789abcdef01234567",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
        },
    });
    assert_snapshot!(answered(
        &[
            "ls",
            "--json",
            "example.com/acme/widget:a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"
        ],
        [response]
    ));
}

/// A path with a space is shell-quoted in the header and the rows, so each pastes into the next command.
#[test]
fn path_with_space_quoted_in_header_and_rows() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "meta": {
                        "__typename": "GitTree",
                        "entries": [
                            {"isDirectory": false, "path": "a b/c.go", "submodule": null},
                            {"isDirectory": true, "path": "a b/d e", "submodule": null},
                        ],
                        "path": "a b",
                    },
                    "oid": "98cb6bf42e086f6af920b965c38cacc07402d51b",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
        },
    });
    assert_snapshot!(answered(&["ls", "example.com/acme/widget@v1.0.0:a b"], [response]));
}

/// A recursive listing sorts segment by segment, so a directory's entries follow
/// it.
#[test]
fn recursive_listing_sorted_by_segment() {
    assert_snapshot!(sego(&["ls", "--recursive", "github.com/gorilla/websocket@v1.5.3:examples"]));
}

/// The server's message for a rejected request is escaped in the error.
#[test]
fn rejected_request_with_control_characters() {
    let response = json!({
        "data": {},
        "errors": [{"message": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"}],
    });
    assert_snapshot!(answered(&["ls", "example.com/acme/widget"], [response]));
}

/// A repository name with control characters is escaped in the usage error.
#[test]
fn repository_name_with_control_characters() {
    assert_snapshot!(sego(&[
        "ls",
        "example.com/acme/widgeta\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"
    ]));
}

/// A revision written as a commit is the JSON rev as written, though the header omits it.
#[test]
fn revision_written_as_commit_json() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "meta": {
                        "__typename": "GitTree",
                        "entries": [
                            {"isDirectory": true, "path": "docs", "submodule": null},
                            {"isDirectory": false, "path": "mux.go", "submodule": null},
                        ],
                        "path": "",
                    },
                    "oid": "98cb6bf42e086f6af920b965c38cacc07402d51b",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
        },
    });
    assert_snapshot!(answered(
        &["ls", "--json", "example.com/acme/widget@98cb6bf42e086f6af920b965c38cacc07402d51b"],
        [response]
    ));
}

/// The root of a tag lists its entries sorted by path, with the rev named in the
/// header.
#[test]
fn root_at_tag() {
    assert_snapshot!(sego(&["ls", "github.com/gorilla/mux@v1.8.0"]));
}

/// JSON carries every entry with its kind.
#[test]
fn root_at_tag_json() {
    assert_snapshot!(sego(&["ls", "--json", "github.com/gorilla/mux@v1.8.0"]));
}

/// Names and a submodule's commit with control characters are escaped.
#[test]
fn root_with_control_characters() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "meta": {
                        "__typename": "GitTree",
                        "entries": [
                            {
                                "isDirectory": false,
                                "path": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                "submodule": null,
                            },
                            {
                                "isDirectory": true,
                                "path": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i-dir",
                                "submodule": null,
                            },
                            {
                                "isDirectory": false,
                                "path": "sub",
                                "submodule": {"commit": "0123456789abcdef0123456789abcdef01234567"},
                            },
                            {
                                "isDirectory": false,
                                "path": "mod",
                                "submodule": {
                                    "commit": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                },
                            },
                        ],
                        "path": "",
                    },
                    "oid": "0123456789abcdef0123456789abcdef01234567",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
        },
    });
    assert_snapshot!(answered(&["ls", "example.com/acme/widget"], [response]));
}

/// In JSON the names and the submodule's commit hold the characters themselves.
#[test]
fn root_with_control_characters_json() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "meta": {
                        "__typename": "GitTree",
                        "entries": [
                            {
                                "isDirectory": false,
                                "path": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                "submodule": null,
                            },
                            {
                                "isDirectory": true,
                                "path": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i-dir",
                                "submodule": null,
                            },
                            {
                                "isDirectory": false,
                                "path": "sub",
                                "submodule": {"commit": "0123456789abcdef0123456789abcdef01234567"},
                            },
                            {
                                "isDirectory": false,
                                "path": "mod",
                                "submodule": {
                                    "commit": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                                },
                            },
                        ],
                        "path": "",
                    },
                    "oid": "0123456789abcdef0123456789abcdef01234567",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
        },
    });
    let ran = answered(&["ls", "--json", "example.com/acme/widget"], [response]);
    assert_eq!(
        ran.stdout,
        "{\"repo\":\"example.com/acme/widget\",\"rev\":null,\"commit\":\"0123456789abcdef0123456789abcdef01234567\",\"path\":null,\"kind\":\"directory\",\"recursive\":false,\"entries\":[{\"path\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"kind\":\"file\",\"submodule_commit\":null},{\"path\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi-dir\",\"kind\":\"dir\",\"submodule_commit\":null},{\"path\":\"mod\",\"kind\":\"submodule\",\"submodule_commit\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\"},{\"path\":\"sub\",\"kind\":\"submodule\",\"submodule_commit\":\"0123456789abcdef0123456789abcdef01234567\"}],\"entries_total\":4,\"entries_more\":false,\"mirrored_at\":null,\"notes\":[]}\n"
    );
    assert_snapshot!(ran);
}

/// A root with submodules counts them and prints their commits.
#[test]
fn root_with_submodules() {
    assert_snapshot!(sego(&[
        "ls",
        "github.com/akinomyoga/ble.sh@d81fd54feb0d996fdff20dca27eaf0201f7015cc"
    ]));
}

/// JSON carries each submodule's commit beside the dirs and files.
#[test]
fn root_with_submodules_json() {
    assert_snapshot!(sego(&[
        "ls",
        "--json",
        "github.com/akinomyoga/ble.sh@d81fd54feb0d996fdff20dca27eaf0201f7015cc"
    ]));
}

/// A root written as `:.` asks for the empty path and is null in JSON.
#[test]
fn root_written_as_dot_json() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "meta": {
                        "__typename": "GitTree",
                        "entries": [
                            {"isDirectory": true, "path": "docs", "submodule": null},
                            {"isDirectory": false, "path": "mux.go", "submodule": null},
                        ],
                        "path": "",
                    },
                    "oid": "98cb6bf42e086f6af920b965c38cacc07402d51b",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
        },
    });
    assert_snapshot!(answered(&["ls", "--json", "example.com/acme/widget@v1.0.0:."], [response]));
}

/// A clone URL in scp form is not an address.
#[test]
fn scp_clone_url_is_not_an_address() {
    assert_snapshot!(sego(&["ls", "git@github.com:gorilla/mux.git"]));
}

/// A subdirectory lists its entries with paths from the repository root.
#[test]
fn subdirectory_paths_from_repository_root() {
    assert_snapshot!(sego(&["ls", "github.com/gorilla/websocket@v1.5.3:examples"]));
}

/// A submodule path answers with its submodule row.
#[test]
fn submodule_path_answers_with_its_row() {
    assert_snapshot!(sego(&[
        "ls",
        "github.com/akinomyoga/ble.sh@d81fd54feb0d996fdff20dca27eaf0201f7015cc:contrib"
    ]));
}

/// JSON carries one entry for the submodule path, with its commit.
#[test]
fn submodule_path_json() {
    assert_snapshot!(sego(&[
        "ls",
        "--json",
        "github.com/akinomyoga/ble.sh@d81fd54feb0d996fdff20dca27eaf0201f7015cc:contrib"
    ]));
}

/// A submodule with no commit prints ? in its place.
#[test]
fn submodule_without_commit() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "meta": {
                        "__typename": "GitBlob",
                        "binary": false,
                        "byteSize": 0,
                        "lfs": null,
                        "path": "contrib",
                        "rawURL": null,
                        "submodule": {"commit": ""},
                        "totalLines": 1,
                    },
                    "oid": "d81fd54feb0d996fdff20dca27eaf0201f7015cc",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/shell",
            },
        },
    });
    assert_snapshot!(answered(
        &["ls", "example.com/acme/shell@d81fd54feb0d996fdff20dca27eaf0201f7015cc:contrib"],
        [response]
    ));
}

/// A submodule's unknown commit is null in JSON.
#[test]
fn submodule_without_commit_json() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "meta": {
                        "__typename": "GitBlob",
                        "binary": false,
                        "byteSize": 0,
                        "lfs": null,
                        "path": "contrib",
                        "rawURL": null,
                        "submodule": {"commit": ""},
                        "totalLines": 1,
                    },
                    "oid": "d81fd54feb0d996fdff20dca27eaf0201f7015cc",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/shell",
            },
        },
    });
    assert_snapshot!(answered(
        &[
            "ls",
            "--json",
            "example.com/acme/shell@d81fd54feb0d996fdff20dca27eaf0201f7015cc:contrib"
        ],
        [response]
    ));
}

/// A web URL is not an address: the error names the forms that are.
#[test]
fn tree_url_is_not_an_address() {
    assert_snapshot!(sego(&["ls", "https://github.com/gorilla/mux/tree/v1.8.0/.github"]));
}
