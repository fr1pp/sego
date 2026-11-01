//! `sego refs`.

use insta::assert_snapshot;
use serde_json::json;

use crate::common::{answered, sego};

/// TEXT filters the rows, and the header still names the default branch.
#[test]
fn branch_and_tag_matching_text() {
    let response = json!({
        "data": {
            "r0": {
                "all_branches": {"totalCount": 1},
                "all_tags": {"totalCount": 1},
                "branches": {
                    "nodes": [
                        {
                            "displayName": "release",
                            "target": {"oid": "0000000000000000000000000000000000000001"},
                        },
                    ],
                    "totalCount": 1,
                },
                "defaultBranch": {"displayName": "release"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
                "tags": {"nodes": [], "totalCount": 0},
            },
        },
    });
    assert_snapshot!(answered(&["refs", "example.com/acme/widget", "release"], [response]));
}

/// A located branch list error names the part, not the server's text.
/// No recording shows these located errors: a server failure sourcegraph.com doesn't give on demand.
#[test]
fn branch_list_error_hides_server_text() {
    let response = json!({
        "data": {"r0": null},
        "errors": [
            {
                "message": "panic occurred: runtime error: slice bounds out of range [:-1]",
                "path": ["r0", "branches", "nodes"],
            },
        ],
    });
    assert_snapshot!(answered(&["refs", "example.com/acme/widget"], [response]));
}

/// Branches and tags of a repository, newest first.
#[test]
fn branches_and_tags() {
    assert_snapshot!(sego(&["refs", "github.com/gorilla/mux"]));
}

/// JSON carries every ref.
#[test]
fn branches_and_tags_json() {
    assert_snapshot!(sego(&["refs", "--json", "github.com/gorilla/mux"]));
}

/// JSON carries the unfiltered totals beside the matching ones.
#[test]
fn branches_and_tags_matching_text_json() {
    let response = json!({
        "data": {
            "r0": {
                "all_branches": {"totalCount": 16},
                "all_tags": {"totalCount": 40},
                "branches": {
                    "nodes": [
                        {
                            "displayName": "release-1.1",
                            "target": {"oid": "0000000000000000000000000000000000000001"},
                        },
                        {
                            "displayName": "release-1.0",
                            "target": {"oid": "0000000000000000000000000000000000000002"},
                        },
                    ],
                    "totalCount": 2,
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
                "tags": {
                    "nodes": [
                        {
                            "displayName": "v1.2-release",
                            "target": {"oid": "0000000000000000000000000000000000000003"},
                        },
                        {
                            "displayName": "v1.1-release",
                            "target": {"oid": "0000000000000000000000000000000000000004"},
                        },
                        {
                            "displayName": "v1.0-release",
                            "target": {"oid": "0000000000000000000000000000000000000005"},
                        },
                    ],
                    "totalCount": 3,
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["refs", "--json", "example.com/acme/widget", "release"],
        [response]
    ));
}

/// A repository with many branches prints 100 and says so.
#[test]
fn branches_and_tags_past_default_limit() {
    assert_snapshot!(sego(&["refs", "github.com/microsoft/vscode"]));
}

/// The default branch is marked by name, wherever it is listed.
#[test]
fn default_branch_not_listed_first() {
    let response = json!({
        "data": {
            "r0": {
                "branches": {
                    "nodes": [
                        {
                            "displayName": "b1",
                            "target": {"oid": "0000000000000000000000000000000000000001"},
                        },
                        {
                            "displayName": "b0",
                            "target": {"oid": "0000000000000000000000000000000000000002"},
                        },
                        {
                            "displayName": "b2",
                            "target": {"oid": "0000000000000000000000000000000000000003"},
                        },
                    ],
                    "totalCount": 3,
                },
                "defaultBranch": {"displayName": "b0"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
                "tags": {"nodes": [], "totalCount": 0},
            },
        },
    });
    assert_snapshot!(answered(&["refs", "example.com/acme/widget"], [response]));
}

/// In JSON `default_branch` is the name, wherever the branch is listed.
#[test]
fn default_branch_not_listed_first_json() {
    let response = json!({
        "data": {
            "r0": {
                "branches": {
                    "nodes": [
                        {
                            "displayName": "b1",
                            "target": {"oid": "0000000000000000000000000000000000000001"},
                        },
                        {
                            "displayName": "b0",
                            "target": {"oid": "0000000000000000000000000000000000000002"},
                        },
                        {
                            "displayName": "b2",
                            "target": {"oid": "0000000000000000000000000000000000000003"},
                        },
                    ],
                    "totalCount": 3,
                },
                "defaultBranch": {"displayName": "b0"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
                "tags": {"nodes": [], "totalCount": 0},
            },
        },
    });
    assert_snapshot!(answered(&["refs", "--json", "example.com/acme/widget"], [response]));
}

/// A web URL is not an address: the error names the forms that are.
#[test]
fn github_url_is_not_an_address() {
    assert_snapshot!(sego(&["refs", "https://github.com/gorilla/mux"]));
}

/// Branch and tag names and targets with control characters are escaped.
#[test]
fn names_with_control_characters() {
    let response = json!({
        "data": {
            "r0": {
                "branches": {
                    "nodes": [
                        {
                            "displayName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                        },
                        {
                            "displayName": "b-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"},
                        },
                    ],
                    "totalCount": 2,
                },
                "defaultBranch": {"displayName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
                "tags": {
                    "nodes": [
                        {
                            "displayName": "t-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                        },
                        {
                            "displayName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                        },
                        {
                            "displayName": "u-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                        },
                    ],
                    "totalCount": 3,
                },
            },
        },
    });
    assert_snapshot!(answered(&["refs", "example.com/acme/widget"], [response]));
}

/// In JSON the names and targets hold the characters themselves.
#[test]
fn names_with_control_characters_json() {
    let response = json!({
        "data": {
            "r0": {
                "branches": {
                    "nodes": [
                        {
                            "displayName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                        },
                        {
                            "displayName": "b-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"},
                        },
                    ],
                    "totalCount": 2,
                },
                "defaultBranch": {"displayName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
                "tags": {
                    "nodes": [
                        {
                            "displayName": "t-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                        },
                        {
                            "displayName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                        },
                        {
                            "displayName": "u-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                        },
                    ],
                    "totalCount": 3,
                },
            },
        },
    });
    let ran = answered(&["refs", "--json", "example.com/acme/widget"], [response]);
    assert_eq!(
        ran.stdout,
        "{\"repo\":\"example.com/acme/widget\",\"query\":null,\"default_branch\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"branches_total\":2,\"tags_total\":3,\"branches_all\":null,\"tags_all\":null,\"branches_more\":false,\"tags_more\":false,\"refs\":[{\"kind\":\"branch\",\"name\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"target\":\"0123456789abcdef0123456789abcdef01234567\"},{\"kind\":\"branch\",\"name\":\"b-a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"target\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\"},{\"kind\":\"tag\",\"name\":\"t-a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"target\":\"0123456789abcdef0123456789abcdef01234567\"},{\"kind\":\"tag\",\"name\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"target\":\"0123456789abcdef0123456789abcdef01234567\"},{\"kind\":\"tag\",\"name\":\"u-a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"target\":\"0123456789abcdef0123456789abcdef01234567\"}],\"mirrored_at\":null,\"notes\":[]}\n"
    );
    assert_snapshot!(ran);
}

/// A failed lookup of owner/name's other host is a note beside the refs from the host found.
/// No recording shows these located errors: a failed lookup of one host sourcegraph.com doesn't give on demand.
#[test]
fn other_host_lookup_failed() {
    let response = json!({
        "data": {
            "r0": {
                "branches": {
                    "nodes": [
                        {
                            "displayName": "main",
                            "target": {"oid": "db9d1d0073d27a0a2d9a8c1bc52aa0af4374d265"},
                        },
                        {
                            "displayName": "coreydaley-patch-2",
                            "target": {"oid": "ac856fa44dac3d5bdac2160d53f6bbec042fa3a3"},
                        },
                        {
                            "displayName": "release-1.8",
                            "target": {"oid": "b4617d0b9670ad14039b2739167fd35a60f557c5"},
                        },
                        {
                            "displayName": "coreydaley-patch-1",
                            "target": {"oid": "d033abe4a66f2728c35886278148b0e38e7dbe4f"},
                        },
                    ],
                    "totalCount": 4,
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": {"updatedAt": "2026-10-27T07:22:54Z"},
                "name": "github.com/gorilla/mux",
                "tags": {
                    "nodes": [
                        {
                            "displayName": "v1.8.1",
                            "target": {"oid": "b4617d0b9670ad14039b2739167fd35a60f557c5"},
                        },
                        {
                            "displayName": "v1.8.0",
                            "target": {"oid": "98cb6bf42e086f6af920b965c38cacc07402d51b"},
                        },
                        {
                            "displayName": "v1.7.4",
                            "target": {"oid": "75dcda0896e109a2a22c9315bca3bb21b87b2ba5"},
                        },
                        {
                            "displayName": "v1.7.3",
                            "target": {"oid": "00bdffe0f3c77e27d2cf6f5c70232a2d3e4d9c15"},
                        },
                        {
                            "displayName": "v1.7.2",
                            "target": {"oid": "ed099d42384823742bba0bf9a72b53b55c9e2e38"},
                        },
                        {
                            "displayName": "v1.7.1",
                            "target": {"oid": "c5c6c98bc25355028a63748a498942a6398ccd22"},
                        },
                        {
                            "displayName": "v1.7.0",
                            "target": {"oid": "a7962380ca08b5a188038c69871b8d3fbdf31e89"},
                        },
                        {
                            "displayName": "v1.6.2",
                            "target": {"oid": "e3702bed27f0d39777b0b37b664b6280e8ef8fbf"},
                        },
                        {
                            "displayName": "v1.6.1",
                            "target": {"oid": "53c1911da2b537f792e7cafcb446b05ffe33b996"},
                        },
                        {
                            "displayName": "v1.6.0",
                            "target": {"oid": "7f08801859139f86dfafd1c296e2cba9a80d292e"},
                        },
                        {
                            "displayName": "v1.5.0",
                            "target": {"oid": "24fca303ac6da784b9e8269f724ddeb0b2eea5e7"},
                        },
                        {
                            "displayName": "v1.4.0",
                            "target": {"oid": "bcd8bc72b08df0f70df986b97f95590779502d31"},
                        },
                        {
                            "displayName": "v1.3.0",
                            "target": {"oid": "392c28fe23e1c45ddba891b0320b3b5df220beea"},
                        },
                        {
                            "displayName": "v1.2.0",
                            "target": {"oid": "b12896167c61cb7a17ee5f15c2ba0729d78793db"},
                        },
                        {
                            "displayName": "v1.1",
                            "target": {"oid": "0eeaf8392f5b04950925b8a69fe70f110fa7cbfc"},
                        },
                    ],
                    "totalCount": 15,
                },
            },
            "r1": null,
        },
        "errors": [
            {
                "message": "rpc error: code = Unavailable desc = connection refused",
                "path": ["r1"],
            },
        ],
    });
    assert_snapshot!(answered(&["refs", "gorilla/mux"], [response]));
}

/// owner/name on both hosts is ambiguous and names both.
#[test]
fn owner_name_on_both_hosts() {
    let response = json!({
        "data": {
            "r0": {
                "branches": {"nodes": [], "totalCount": 0},
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "github.com/acme/widget",
                "tags": {"nodes": [], "totalCount": 0},
            },
            "r1": {
                "branches": {"nodes": [], "totalCount": 0},
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "gitlab.com/acme/widget",
                "tags": {"nodes": [], "totalCount": 0},
            },
        },
    });
    assert_snapshot!(answered(&["refs", "acme/widget"], [response]));
}

/// owner/name resolves to the one host that has it.
#[test]
fn owner_name_on_github() {
    assert_snapshot!(sego(&["refs", "gorilla/mux"]));
}

/// Two roots fit the complexity limit with the JSON page size.
#[test]
fn owner_name_on_github_json() {
    assert_snapshot!(sego(&["refs", "--json", "gorilla/mux"]));
}

/// owner/name resolves to the one host that has it.
#[test]
fn owner_name_on_gitlab() {
    let response = json!({
        "data": {
            "r0": null,
            "r1": {
                "branches": {
                    "nodes": [
                        {
                            "displayName": "main",
                            "target": {"oid": "0000000000000000000000000000000000000003"},
                        },
                    ],
                    "totalCount": 1,
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "gitlab.com/acme/widget",
                "tags": {"nodes": [], "totalCount": 0},
            },
        },
    });
    assert_snapshot!(answered(&["refs", "acme/widget"], [response]));
}

/// The server's message for a rejected request is escaped in the error.
#[test]
fn rejected_request_with_control_characters() {
    let response = json!({
        "data": null,
        "errors": [{"message": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"}],
    });
    assert_snapshot!(answered(&["refs", "example.com/acme/widget"], [response]));
}

/// A repository name with control characters is escaped in the usage error.
#[test]
fn repository_name_with_control_characters() {
    assert_snapshot!(sego(&[
        "refs",
        "example.com/acme/widgeta\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"
    ]));
}

/// A repository that is not on sourcegraph.com is an error.
#[test]
fn repository_not_on_sourcegraph() {
    assert_snapshot!(sego(&["refs", "github.com/facebook/react"]));
}

/// A repository without refs prints zero of each.
#[test]
fn repository_without_refs() {
    assert_snapshot!(sego(&["refs", "github.com/ascoders/blog"]));
}

/// JSON of a repository without refs has no default branch and no refs.
#[test]
fn repository_without_refs_json() {
    assert_snapshot!(sego(&["refs", "--json", "github.com/ascoders/blog"]));
}

/// A located list error names the part, not the server's text.
/// No recording shows these located errors: a server failure sourcegraph.com doesn't give on demand.
#[test]
fn tag_list_error_hides_server_text() {
    let response = json!({
        "data": {"r0": null},
        "errors": [
            {
                "message": "panic occurred: runtime error: slice bounds out of range [:-1]",
                "path": ["r0", "tags", "nodes"],
            },
        ],
    });
    assert_snapshot!(answered(&["refs", "example.com/acme/widget"], [response]));
}

/// A tag that names a tree prints the tree it names.
#[test]
fn tag_naming_a_tree() {
    assert_snapshot!(sego(&["refs", "github.com/torvalds/linux", "2.6.11"]));
}

/// A tag naming an object Sourcegraph reads as no commit prints the object it names.
#[test]
fn tag_without_commit() {
    let response = json!({
        "data": {
            "r0": {
                "branches": {
                    "nodes": [
                        {
                            "displayName": "main",
                            "target": {"oid": "a1b2c3d4e5f6071829304a5b6c7d8e9f0a1b2c3d"},
                        },
                    ],
                    "totalCount": 1,
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
                "tags": {
                    "nodes": [
                        {
                            "displayName": "v2",
                            "target": {"oid": "0f1e2d3c4b5a69788796a5b4c3d2e1f0a9b8c7d6"},
                        },
                        {
                            "displayName": "v1-tree",
                            "target": {"oid": "9a8b7c6d5e4f30211203f4e5d6c7b8a990817263"},
                        },
                    ],
                    "totalCount": 2,
                },
            },
        },
    });
    assert_snapshot!(answered(&["refs", "example.com/acme/widget"], [response]));
}

/// In JSON a tag naming an object that is not a commit carries the object as its target.
#[test]
fn tag_without_commit_json() {
    let response = json!({
        "data": {
            "r0": {
                "branches": {
                    "nodes": [
                        {
                            "displayName": "main",
                            "target": {"oid": "0000000000000000000000000000000000000001"},
                        },
                    ],
                    "totalCount": 1,
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
                "tags": {
                    "nodes": [
                        {
                            "displayName": "v2",
                            "target": {"oid": "0000000000000000000000000000000000000002"},
                        },
                        {
                            "displayName": "v1-tree",
                            "target": {"oid": "0000000000000000000000000000000000000003"},
                        },
                    ],
                    "totalCount": 2,
                },
            },
        },
    });
    assert_snapshot!(answered(&["refs", "--json", "example.com/acme/widget"], [response]));
}

/// A text filter counts the matches against the unfiltered totals.
#[test]
fn tags_matching_text() {
    assert_snapshot!(sego(&["refs", "github.com/BurntSushi/ripgrep", "14.1"]));
}

/// A filter with control characters, given as the argument, is escaped in the header.
#[test]
fn text_with_control_characters() {
    let response = json!({
        "data": {
            "r0": {
                "all_branches": {"totalCount": 9},
                "all_tags": {"totalCount": 12},
                "branches": {
                    "nodes": [
                        {
                            "displayName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                        },
                        {
                            "displayName": "b-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"},
                        },
                    ],
                    "totalCount": 2,
                },
                "defaultBranch": {"displayName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
                "tags": {
                    "nodes": [
                        {
                            "displayName": "t-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                        },
                        {
                            "displayName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                        },
                        {
                            "displayName": "u-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                        },
                    ],
                    "totalCount": 3,
                },
            },
        },
    });
    assert_snapshot!(answered(
        &["refs", "example.com/acme/widget", "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"],
        [response]
    ));
}

/// In JSON the filter holds the characters themselves.
#[test]
fn text_with_control_characters_json() {
    let response = json!({
        "data": {
            "r0": {
                "all_branches": {"totalCount": 9},
                "all_tags": {"totalCount": 12},
                "branches": {
                    "nodes": [
                        {
                            "displayName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                        },
                        {
                            "displayName": "b-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"},
                        },
                    ],
                    "totalCount": 2,
                },
                "defaultBranch": {"displayName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
                "tags": {
                    "nodes": [
                        {
                            "displayName": "t-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                        },
                        {
                            "displayName": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                        },
                        {
                            "displayName": "u-a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                            "target": {"oid": "0123456789abcdef0123456789abcdef01234567"},
                        },
                    ],
                    "totalCount": 3,
                },
            },
        },
    });
    assert_snapshot!(answered(
        &[
            "refs",
            "--json",
            "example.com/acme/widget",
            "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"
        ],
        [response]
    ));
}

/// TEXT is a substring, so glob characters match themselves.
#[test]
fn text_with_glob_characters() {
    let response = json!({
        "data": {
            "r0": {
                "all_branches": {"totalCount": 4},
                "all_tags": {"totalCount": 15},
                "branches": {"nodes": [], "totalCount": 0},
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
                "tags": {"nodes": [], "totalCount": 0},
            },
        },
    });
    assert_snapshot!(answered(&["refs", "example.com/acme/widget", "v1.*"], [response]));
}

/// JSON lists every ref, so `branches_more` and `tags_more` are false.
#[test]
fn totals_past_the_listed_refs_json() {
    let response = json!({
        "data": {
            "r0": {
                "branches": {
                    "nodes": [
                        {
                            "displayName": "b0",
                            "target": {"oid": "0000000000000000000000000000000000000001"},
                        },
                        {
                            "displayName": "b1",
                            "target": {"oid": "0000000000000000000000000000000000000002"},
                        },
                        {
                            "displayName": "b2",
                            "target": {"oid": "0000000000000000000000000000000000000003"},
                        },
                    ],
                    "totalCount": 3,
                },
                "defaultBranch": {"displayName": "b0"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
                "tags": {
                    "nodes": [
                        {
                            "displayName": "t0",
                            "target": {"oid": "0000000000000000000000000000000000000001"},
                        },
                        {
                            "displayName": "t1",
                            "target": {"oid": "0000000000000000000000000000000000000002"},
                        },
                    ],
                    "totalCount": 2,
                },
            },
        },
    });
    assert_snapshot!(answered(&["refs", "--json", "example.com/acme/widget"], [response]));
}
