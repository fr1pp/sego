//! `sego show` of a file.

use insta::assert_snapshot;
use serde_json::json;

use crate::common::case::Case;
use crate::common::recording::ServerResponse;
use crate::common::{answered, sego};

/// An abbreviation two commits share names no single commit, and the message says so.
#[test]
fn ambiguous_abbreviation() {
    assert_snapshot!(sego(&["show", "github.com/gorilla/mux@49c0:mux.go"]));
}

/// A binary file is answered from its details.
#[test]
fn binary_file() {
    assert_snapshot!(sego(&[
        "show",
        "github.com/BurntSushi/ripgrep@14.1.0:tests/data/sherlock.gz"
    ]));
}

/// A binary file over the limit is answered from its details, with one request.
#[test]
fn binary_file_over_content_limit() {
    assert_snapshot!(sego(&[
        "show",
        "github.com/golang/go@6885bad7dd86:src/crypto/internal/boring/syso/goboringcrypto_linux_amd64.syso"
    ]));
}

/// Sourcegraph's text holds U+FFFD for bytes that aren't UTF-8, so they are read raw and printed as hex escapes.
#[test]
fn bytes_that_arent_utf8_are_read_raw() {
    let first = json!({
        "data": {
            "r0": {
                "commit": {
                    "body": {
                        "__typename": "GitBlob",
                        "content": "        perimeter(&rect1)\n    );\n    println!(r#\"This line contains invalid utf8:  \"�����\"#;\n}\n",
                    },
                    "meta": {
                        "__typename": "GitBlob",
                        "binary": false,
                        "byteSize": 533,
                        "lfs": null,
                        "path": "tests/snapshots/sample.modified.rs",
                        "rawURL": "/r/github.com/sharkdp/bat@a9aa73d215455a479f072da75dd39f19826acfea/-/raw/tests/snapshots/sample.modified.rs",
                        "submodule": null,
                        "totalLines": 23,
                    },
                    "oid": "a9aa73d215455a479f072da75dd39f19826acfea",
                },
                "defaultBranch": {"displayName": "master"},
                "mirrorInfo": {"updatedAt": "2026-10-31T00:17:18Z"},
                "name": "github.com/sharkdp/bat",
            },
        },
    });
    let second = ServerResponse::new(200, "text/plain; charset=utf-8", b"\x2f\x2f\x2f\x20\x41\x20\x72\x65\x63\x74\x61\x6e\x67\x6c\x65\x2e\x20\x46\x69\x72\x73\x74\x20\x6c\x69\x6e\x65\x20\x69\x73\x20\x63\x68\x61\x6e\x67\x65\x64\x20\x74\x6f\x20\x70\x72\x65\x76\x65\x6e\x74\x20\x61\x20\x72\x65\x67\x72\x65\x73\x73\x69\x6f\x6e\x20\x6f\x66\x20\x23\x31\x38\x36\x39\x0a\x73\x74\x72\x75\x63\x74\x20\x52\x65\x63\x74\x61\x6e\x67\x6c\x65\x20\x7b\x0a\x20\x20\x20\x20\x77\x69\x64\x74\x68\x3a\x20\x75\x33\x32\x2c\x0a\x20\x20\x20\x20\x68\x65\x69\x67\x68\x74\x3a\x20\x75\x33\x32\x2c\x0a\x7d\x0a\x0a\x66\x6e\x20\x6d\x61\x69\x6e\x28\x29\x20\x7b\x0a\x20\x20\x20\x20\x6c\x65\x74\x20\x72\x65\x63\x74\x31\x20\x3d\x20\x52\x65\x63\x74\x61\x6e\x67\x6c\x65\x20\x7b\x20\x77\x69\x64\x74\x68\x3a\x20\x33\x30\x2c\x20\x68\x65\x69\x67\x68\x74\x3a\x20\x35\x30\x20\x7d\x3b\x0a\x0a\x20\x20\x20\x20\x70\x72\x69\x6e\x74\x6c\x6e\x21\x28\x0a\x20\x20\x20\x20\x20\x20\x20\x20\x22\x54\x68\x65\x20\x70\x65\x72\x69\x6d\x65\x74\x65\x72\x20\x6f\x66\x20\x74\x68\x65\x20\x72\x65\x63\x74\x61\x6e\x67\x6c\x65\x20\x69\x73\x20\x7b\x7d\x20\x70\x69\x78\x65\x6c\x73\x2e\x22\x2c\x0a\x20\x20\x20\x20\x20\x20\x20\x20\x70\x65\x72\x69\x6d\x65\x74\x65\x72\x28\x26\x72\x65\x63\x74\x31\x29\x0a\x20\x20\x20\x20\x29\x3b\x0a\x20\x20\x20\x20\x70\x72\x69\x6e\x74\x6c\x6e\x21\x28\x72\x23\x22\x54\x68\x69\x73\x20\x6c\x69\x6e\x65\x20\x63\x6f\x6e\x74\x61\x69\x6e\x73\x20\x69\x6e\x76\x61\x6c\x69\x64\x20\x75\x74\x66\x38\x3a\x20\x20\x22\xf8\x88\x80\x80\x80\x22\x23\x3b\x0a\x7d\x0a\x0a\x66\x6e\x20\x61\x72\x65\x61\x28\x72\x65\x63\x74\x61\x6e\x67\x6c\x65\x3a\x20\x26\x52\x65\x63\x74\x61\x6e\x67\x6c\x65\x29\x20\x2d\x3e\x20\x75\x33\x32\x20\x7b\x0a\x20\x20\x20\x20\x72\x65\x63\x74\x61\x6e\x67\x6c\x65\x2e\x77\x69\x64\x74\x68\x20\x2a\x20\x72\x65\x63\x74\x61\x6e\x67\x6c\x65\x2e\x68\x65\x69\x67\x68\x74\x0a\x7d\x0a\x0a\x66\x6e\x20\x70\x65\x72\x69\x6d\x65\x74\x65\x72\x28\x72\x65\x63\x74\x61\x6e\x67\x6c\x65\x3a\x20\x26\x52\x65\x63\x74\x61\x6e\x67\x6c\x65\x29\x20\x2d\x3e\x20\x75\x33\x32\x20\x7b\x0a\x20\x20\x20\x20\x28\x72\x65\x63\x74\x61\x6e\x67\x6c\x65\x2e\x77\x69\x64\x74\x68\x20\x2b\x20\x72\x65\x63\x74\x61\x6e\x67\x6c\x65\x2e\x68\x65\x69\x67\x68\x74\x29\x20\x2a\x20\x32\x0a\x7d\x0a");
    assert_snapshot!(answered(
        &["show", "github.com/sharkdp/bat@a9aa73d2:tests/snapshots/sample.modified.rs", "12-15"],
        [first.into(), second]
    ));
}

/// In JSON a line whose bytes aren't UTF-8 carries them in base64 beside its text.
#[test]
fn bytes_that_arent_utf8_are_read_raw_json() {
    let first = json!({
        "data": {
            "r0": {
                "commit": {
                    "body": {
                        "__typename": "GitBlob",
                        "content": "    println!(r#\"This line contains invalid utf8:  \"�����\"#;\n",
                    },
                    "meta": {
                        "__typename": "GitBlob",
                        "binary": false,
                        "byteSize": 533,
                        "lfs": null,
                        "path": "tests/snapshots/sample.modified.rs",
                        "rawURL": "/r/github.com/sharkdp/bat@a9aa73d215455a479f072da75dd39f19826acfea/-/raw/tests/snapshots/sample.modified.rs",
                        "submodule": null,
                        "totalLines": 23,
                    },
                    "oid": "a9aa73d215455a479f072da75dd39f19826acfea",
                },
                "defaultBranch": {"displayName": "master"},
                "mirrorInfo": {"updatedAt": "2026-10-31T00:17:18Z"},
                "name": "github.com/sharkdp/bat",
            },
        },
    });
    let second = ServerResponse::new(200, "text/plain; charset=utf-8", b"\x2f\x2f\x2f\x20\x41\x20\x72\x65\x63\x74\x61\x6e\x67\x6c\x65\x2e\x20\x46\x69\x72\x73\x74\x20\x6c\x69\x6e\x65\x20\x69\x73\x20\x63\x68\x61\x6e\x67\x65\x64\x20\x74\x6f\x20\x70\x72\x65\x76\x65\x6e\x74\x20\x61\x20\x72\x65\x67\x72\x65\x73\x73\x69\x6f\x6e\x20\x6f\x66\x20\x23\x31\x38\x36\x39\x0a\x73\x74\x72\x75\x63\x74\x20\x52\x65\x63\x74\x61\x6e\x67\x6c\x65\x20\x7b\x0a\x20\x20\x20\x20\x77\x69\x64\x74\x68\x3a\x20\x75\x33\x32\x2c\x0a\x20\x20\x20\x20\x68\x65\x69\x67\x68\x74\x3a\x20\x75\x33\x32\x2c\x0a\x7d\x0a\x0a\x66\x6e\x20\x6d\x61\x69\x6e\x28\x29\x20\x7b\x0a\x20\x20\x20\x20\x6c\x65\x74\x20\x72\x65\x63\x74\x31\x20\x3d\x20\x52\x65\x63\x74\x61\x6e\x67\x6c\x65\x20\x7b\x20\x77\x69\x64\x74\x68\x3a\x20\x33\x30\x2c\x20\x68\x65\x69\x67\x68\x74\x3a\x20\x35\x30\x20\x7d\x3b\x0a\x0a\x20\x20\x20\x20\x70\x72\x69\x6e\x74\x6c\x6e\x21\x28\x0a\x20\x20\x20\x20\x20\x20\x20\x20\x22\x54\x68\x65\x20\x70\x65\x72\x69\x6d\x65\x74\x65\x72\x20\x6f\x66\x20\x74\x68\x65\x20\x72\x65\x63\x74\x61\x6e\x67\x6c\x65\x20\x69\x73\x20\x7b\x7d\x20\x70\x69\x78\x65\x6c\x73\x2e\x22\x2c\x0a\x20\x20\x20\x20\x20\x20\x20\x20\x70\x65\x72\x69\x6d\x65\x74\x65\x72\x28\x26\x72\x65\x63\x74\x31\x29\x0a\x20\x20\x20\x20\x29\x3b\x0a\x20\x20\x20\x20\x70\x72\x69\x6e\x74\x6c\x6e\x21\x28\x72\x23\x22\x54\x68\x69\x73\x20\x6c\x69\x6e\x65\x20\x63\x6f\x6e\x74\x61\x69\x6e\x73\x20\x69\x6e\x76\x61\x6c\x69\x64\x20\x75\x74\x66\x38\x3a\x20\x20\x22\xf8\x88\x80\x80\x80\x22\x23\x3b\x0a\x7d\x0a\x0a\x66\x6e\x20\x61\x72\x65\x61\x28\x72\x65\x63\x74\x61\x6e\x67\x6c\x65\x3a\x20\x26\x52\x65\x63\x74\x61\x6e\x67\x6c\x65\x29\x20\x2d\x3e\x20\x75\x33\x32\x20\x7b\x0a\x20\x20\x20\x20\x72\x65\x63\x74\x61\x6e\x67\x6c\x65\x2e\x77\x69\x64\x74\x68\x20\x2a\x20\x72\x65\x63\x74\x61\x6e\x67\x6c\x65\x2e\x68\x65\x69\x67\x68\x74\x0a\x7d\x0a\x0a\x66\x6e\x20\x70\x65\x72\x69\x6d\x65\x74\x65\x72\x28\x72\x65\x63\x74\x61\x6e\x67\x6c\x65\x3a\x20\x26\x52\x65\x63\x74\x61\x6e\x67\x6c\x65\x29\x20\x2d\x3e\x20\x75\x33\x32\x20\x7b\x0a\x20\x20\x20\x20\x28\x72\x65\x63\x74\x61\x6e\x67\x6c\x65\x2e\x77\x69\x64\x74\x68\x20\x2b\x20\x72\x65\x63\x74\x61\x6e\x67\x6c\x65\x2e\x68\x65\x69\x67\x68\x74\x29\x20\x2a\x20\x32\x0a\x7d\x0a");
    assert_snapshot!(answered(
        &[
            "show",
            "--json",
            "github.com/sharkdp/bat@a9aa73d2:tests/snapshots/sample.modified.rs",
            "14"
        ],
        [first.into(), second]
    ));
}

/// A closed stdout is not an error.
#[test]
fn closed_stdout_is_not_an_error() {
    assert_snapshot!(
        Case::new(&["show", "github.com/gorilla/mux@v1.8.0:mux.go", "1-3"], Vec::new())
            .stdout_closed()
            .ran()
    );
}

/// Content shorter than the lines the details report is a failure.
#[test]
fn content_shorter_than_total_lines() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "body": {"__typename": "GitBlob", "content": "a\n"},
                    "meta": {
                        "__typename": "GitBlob",
                        "binary": false,
                        "byteSize": 2,
                        "lfs": null,
                        "path": "a.go",
                        "rawURL": "/r/example.com/acme/gadget@d81fd54f1a2b3c4d5e6f708192a3b4c5d6e7f809/-/raw/a.go",
                        "submodule": null,
                        "totalLines": 3,
                    },
                    "oid": "d81fd54f1a2b3c4d5e6f708192a3b4c5d6e7f809",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/gadget",
            },
        },
    });
    assert_snapshot!(answered(&["show", "example.com/acme/gadget@main:a.go"], [response]));
}

/// CRLF is dropped from each line and named in the header.
#[test]
fn crlf_line_endings() {
    assert_snapshot!(sego(&[
        "show",
        "github.com/golang/go@e3c3d2f9e7ca88e6f240fe00faa67efb03d851a1:src/cmd/go/testdata/script/go_version.txt",
        "1-5"
    ]));
}

/// The default window is lines 1-500.
#[test]
fn default_window_of_500_lines() {
    assert_snapshot!(sego(&["show", "github.com/gorilla/mux@v1.8.0:mux.go"]));
}

/// A path that names a directory prints its listing (#25).
#[test]
fn directory_answers_with_its_listing() {
    assert_snapshot!(sego(&["show", "github.com/gorilla/mux@v1.8.0:.github"]));
}

/// LINES given for a directory is a usage error found from the response.
#[test]
fn directory_with_lines_is_a_usage_error() {
    assert_snapshot!(sego(&["show", "github.com/gorilla/mux@v1.8.0:.github", "1-5"]));
}

/// A directory with one entry is listed, saying entry in the singular.
#[test]
fn directory_with_one_entry() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "body": {"__typename": "GitTree"},
                    "meta": {
                        "__typename": "GitTree",
                        "entries": [{"isDirectory": false, "path": "docs/0.md", "submodule": null}],
                        "path": "docs",
                    },
                    "oid": "d81fd54f1a2b3c4d5e6f708192a3b4c5d6e7f809",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/docs",
            },
        },
    });
    assert_snapshot!(answered(&["show", "example.com/acme/docs@main:docs"], [response]));
}

/// An empty file has no lines, although the server reports 1.
#[test]
fn empty_file() {
    assert_snapshot!(sego(&[
        "show",
        "github.com/django/django@8c34887b1ad8bb3ec61c6e713442dab3c643d557:tests/str/__init__.py"
    ]));
}

/// A range on an empty file prints no lines and exits 0.
#[test]
fn empty_file_with_range() {
    assert_snapshot!(sego(&[
        "show",
        "github.com/django/django@8c34887b1ad8bb3ec61c6e713442dab3c643d557:tests/str/__init__.py",
        "1-5"
    ]));
}

/// Control characters in a file's lines are escaped, and so is a CR, unless every line shown ends in CRLF.
#[test]
fn file_content_with_control_characters() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "body": {
                        "__typename": "GitBlob",
                        "content": "a\u{1b}b\nc\u{202e}d\ne\u{200b}f\ng\0h\ni\u{2028}j\nk\u{7f}l\nmid\rcr\ntab\tok\ncrlf\r\n",
                    },
                    "meta": {
                        "__typename": "GitBlob",
                        "binary": false,
                        "byteSize": 50,
                        "lfs": null,
                        "path": "f.txt",
                        "rawURL": "/r/example.com/acme/widget@0123456789abcdef0123456789abcdef01234567/-/raw/f.txt",
                        "submodule": null,
                        "totalLines": 9,
                    },
                    "oid": "0123456789abcdef0123456789abcdef01234567",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
        },
    });
    assert_snapshot!(answered(
        &["show", "example.com/acme/widget@0123456789abcdef0123456789abcdef01234567:f.txt"],
        [response]
    ));
}

/// In JSON the lines hold the characters themselves, every CR included.
#[test]
fn file_content_with_control_characters_json() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "body": {
                        "__typename": "GitBlob",
                        "content": "a\u{1b}b\nc\u{202e}d\ne\u{200b}f\ng\0h\ni\u{2028}j\nk\u{7f}l\nmid\rcr\ntab\tok\ncrlf\r\n",
                    },
                    "meta": {
                        "__typename": "GitBlob",
                        "binary": false,
                        "byteSize": 50,
                        "lfs": null,
                        "path": "f.txt",
                        "rawURL": "/r/example.com/acme/widget@0123456789abcdef0123456789abcdef01234567/-/raw/f.txt",
                        "submodule": null,
                        "totalLines": 9,
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
            "show",
            "--json",
            "example.com/acme/widget@0123456789abcdef0123456789abcdef01234567:f.txt"
        ],
        [response]
    ));
}

/// A file whose lines end in CR alone is one line, its CRs escaped, and not CRLF.
#[test]
fn file_using_cr_alone() {
    assert_snapshot!(sego(&["show", "github.com/uliwitness/UliKit@41a918fa:LoginItemAPI.h", "1"]));
}

/// A file without a final newline has no line past its last.
#[test]
fn file_without_final_newline() {
    assert_snapshot!(sego(&[
        "show",
        "github.com/evilmartians/lefthook@cc9969c2e2198aafff8accc63ee1aee30e9a2aa4:.typos.toml",
        "5-6"
    ]));
}

/// The header names the canonical spelling.
#[test]
fn header_uses_canonical_spelling() {
    assert_snapshot!(sego(&["show", "GitHub.com/Gorilla/Mux@v1.8.0:mux.go", "1-3"]));
}

/// A revision of hex digits that the commit doesn't start with, such as a tag named 2024, is a name the header shows.
#[test]
fn hex_named_tag_shows_its_rev() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "body": {"__typename": "GitBlob", "content": "package a\n"},
                    "meta": {
                        "__typename": "GitBlob",
                        "binary": false,
                        "byteSize": 9,
                        "lfs": null,
                        "path": "a.go",
                        "rawURL": "/r/example.com/acme/widget@d81fd54f1a2b3c4d5e6f708192a3b4c5d6e7f809/-/raw/a.go",
                        "submodule": null,
                        "totalLines": 1,
                    },
                    "oid": "d81fd54f1a2b3c4d5e6f708192a3b4c5d6e7f809",
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": null,
                "name": "example.com/acme/widget",
            },
        },
    });
    assert_snapshot!(answered(&["show", "example.com/acme/widget@2024:a.go"], [response]));
}

/// JSON carries every key.
#[test]
fn json_answer() {
    assert_snapshot!(sego(&["show", "--json", "github.com/gorilla/mux@v1.8.0:mux.go", "1-3"]));
}

/// An LFS pointer prints its text.
#[test]
fn lfs_pointer() {
    assert_snapshot!(sego(&[
        "show",
        "github.com/openai/mle-bench@507f92e1138bb6e40dac5c6ee7a6758e6424bf97:experiments/competition_categories.csv"
    ]));
}

/// A range running past the end is clamped.
#[test]
fn lines_past_end_are_clamped() {
    assert_snapshot!(sego(&["show", "github.com/gorilla/mux@v1.8.0:mux.go", "600-700"]));
}

/// A range starting past the end prints no lines and exits 0.
#[test]
fn lines_start_past_end() {
    assert_snapshot!(sego(&["show", "github.com/gorilla/mux@v1.8.0:mux.go", "700-710"]));
}

/// N,M is the same range as N-M.
#[test]
fn lines_with_comma() {
    assert_snapshot!(sego(&["show", "github.com/gorilla/mux@v1.8.0:mux.go", "10,20"]));
}

/// A first line of 169000 columns is cut in text.
#[test]
fn minified_line_cut_in_text() {
    assert_snapshot!(sego(&[
        "show",
        "github.com/sbwml/luci-app-mosdns@bd40245303cd0ba56804d49eed5dbc4be2a082ca:luci-app-mosdns/htdocs/luci-static/resources/codemirror5/codemirror.min.js"
    ]));
}

/// JSON carries the minified line whole.
#[test]
fn minified_line_in_json() {
    assert_snapshot!(sego(&[
        "show",
        "--json",
        "github.com/sbwml/luci-app-mosdns@bd40245303cd0ba56804d49eed5dbc4be2a082ca:luci-app-mosdns/htdocs/luci-static/resources/codemirror5/codemirror.min.js"
    ]));
}

/// A missing path is an error.
#[test]
fn missing_path() {
    assert_snapshot!(sego(&["show", "github.com/gorilla/mux@v1.8.0:nope.go"]));
}

/// A missing path with control characters is escaped in the error.
#[test]
fn missing_path_with_control_characters() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "body": null,
                    "meta": null,
                    "oid": "0123456789abcdef0123456789abcdef01234567",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
        },
    });
    assert_snapshot!(answered(
        &[
            "show",
            "example.com/acme/widget@0123456789abcdef0123456789abcdef01234567:a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i-gone"
        ],
        [response]
    ));
}

/// A missing repository is an error.
#[test]
fn missing_repository() {
    assert_snapshot!(sego(&["show", "github.com/facebook/react:README.md"]));
}

/// A missing revision is an error.
#[test]
fn missing_revision() {
    assert_snapshot!(sego(&["show", "github.com/gorilla/mux@v9.9:mux.go"]));
}

/// A missing revision states the default branch.
#[test]
fn missing_revision_states_the_default_branch() {
    assert_snapshot!(sego(&["show", "github.com/gorilla/mux@release/1.8:mux.go"]));
}

/// A non-ASCII path prints shell-quoted in the header.
#[test]
fn non_ascii_path_quoted_in_header() {
    assert_snapshot!(sego(&[
        "show",
        "github.com/nodejs/node@aa1bf2cc37539bfeeef8faf80c9d31a64cb78026:test/fixtures/copy/utf/新建文件夹/index.js"
    ]));
}

/// A window of LF lines with one CR CR LF line is not CRLF, and that line's CRs show.
#[test]
fn one_crlf_line_among_lf_lines() {
    assert_snapshot!(sego(&[
        "show",
        "github.com/go-gitea/gitea@965d7522:options/gitignore/macOS",
        "5-9"
    ]));
}

/// An open range runs to the end.
#[test]
fn open_lines_range() {
    assert_snapshot!(sego(&["show", "github.com/gorilla/mux@v1.8.0:mux.go", "600-"]));
}

/// A failed lookup of owner/name's other host is a note beside the answer from the host found.
/// No recording shows these located errors: a failed lookup of one host sourcegraph.com doesn't give on demand.
#[test]
fn other_host_lookup_failed() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "body": {"__typename": "GitBlob", "content": "# gorilla/mux\n\n"},
                    "meta": {
                        "__typename": "GitBlob",
                        "binary": false,
                        "byteSize": 25673,
                        "lfs": null,
                        "path": "README.md",
                        "rawURL": "/r/github.com/gorilla/mux@db9d1d0073d27a0a2d9a8c1bc52aa0af4374d265/-/raw/README.md",
                        "submodule": null,
                        "totalLines": 812,
                    },
                    "oid": "db9d1d0073d27a0a2d9a8c1bc52aa0af4374d265",
                },
                "defaultBranch": {"displayName": "main"},
                "mirrorInfo": {"updatedAt": "2026-10-27T07:04:31Z"},
                "name": "github.com/gorilla/mux",
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
    assert_snapshot!(answered(&["show", "gorilla/mux:README.md", "1-2"], [response]));
}

/// owner/name that exists on both hosts is a usage error naming both.
#[test]
fn owner_name_on_both_hosts() {
    assert_snapshot!(sego(&["show", "wireshark/wireshark:README.md", "1-3"]));
}

/// owner/name found on one host reads from it.
#[test]
fn owner_name_on_one_host() {
    assert_snapshot!(sego(&["show", "gorilla/mux:README.md", "1-2"]));
}

/// A path with control characters, given as the argument, is escaped in the header.
#[test]
fn path_with_control_characters() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "body": {"__typename": "GitBlob", "content": "x\n"},
                    "meta": {
                        "__typename": "GitBlob",
                        "binary": false,
                        "byteSize": 2,
                        "lfs": null,
                        "path": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                        "rawURL": "/r/example.com/acme/widget@0123456789abcdef0123456789abcdef01234567/-/raw/f.txt",
                        "submodule": null,
                        "totalLines": 1,
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
            "show",
            "example.com/acme/widget@0123456789abcdef0123456789abcdef01234567:a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"
        ],
        [response]
    ));
}

/// In JSON the path holds the characters themselves.
#[test]
fn path_with_control_characters_json() {
    let response = json!({
        "data": {
            "r0": {
                "commit": {
                    "body": {"__typename": "GitBlob", "content": "x\n"},
                    "meta": {
                        "__typename": "GitBlob",
                        "binary": false,
                        "byteSize": 2,
                        "lfs": null,
                        "path": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
                        "rawURL": "/r/example.com/acme/widget@0123456789abcdef0123456789abcdef01234567/-/raw/f.txt",
                        "submodule": null,
                        "totalLines": 1,
                    },
                    "oid": "0123456789abcdef0123456789abcdef01234567",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
        },
    });
    let ran = answered(
        &[
            "show",
            "--json",
            "example.com/acme/widget@0123456789abcdef0123456789abcdef01234567:a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i",
        ],
        [response],
    );
    assert_eq!(
        ran.stdout,
        "{\"repo\":\"example.com/acme/widget\",\"rev\":\"0123456789abcdef0123456789abcdef01234567\",\"commit\":\"0123456789abcdef0123456789abcdef01234567\",\"path\":\"a\\u001bb\\u202ec\\u200bd\\ne\\rf\\u0000g\\u2028h\\u007fi\",\"kind\":\"file\",\"total_lines\":1,\"byte_size\":2,\"binary\":false,\"crlf\":false,\"utf8\":true,\"lfs_size\":null,\"submodule\":null,\"start\":1,\"end\":1,\"lines\":[{\"line\":1,\"text\":\"x\"}],\"mirrored_at\":null,\"notes\":[]}\n"
    );
    assert_snapshot!(ran);
}

/// A path with `.`, `..` and empty segments is read, and printed, as the path it resolves to.
#[test]
fn path_with_dot_segments_is_normalised() {
    assert_snapshot!(sego(&["show", "github.com/gorilla/mux@v1.8.0:./x/..//mux.go", "1-3"]));
}

/// A path with a space prints shell-quoted in the header.
#[test]
fn path_with_space_quoted_in_header() {
    assert_snapshot!(sego(&[
        "show",
        "github.com/nodejs/node@aa1bf2cc37539bfeeef8faf80c9d31a64cb78026:test/fixtures/print A.js"
    ]));
}

/// A Cloudflare challenge on the raw read is reported as the edge's refusal.
/// Located errors as in `show::binary_file_over_content_limit`.
#[test]
fn raw_read_challenge() {
    let first = json!({
        "data": {
            "r0": {
                "commit": {
                    "body": null,
                    "meta": {
                        "__typename": "GitBlob",
                        "binary": false,
                        "byteSize": 2_000_000,
                        "lfs": null,
                        "path": "big.c",
                        "rawURL": "/r/example.com/acme/widget@d81fd54f1a2b3c4d5e6f708192a3b4c5d6e7f809/-/raw/big.c",
                        "submodule": null,
                        "totalLines": 5,
                    },
                    "oid": "d81fd54f1a2b3c4d5e6f708192a3b4c5d6e7f809",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
        },
        "errors": [
            {
                "message": "file exceeded size limit: BytesRead=1572865 (may be lower than file size)",
                "path": ["r0", "commit", "body", "content"],
            },
            {
                "message": "file exceeded size limit: BytesRead=1572865 (may be lower than file size)",
                "path": ["r0", "commit", "meta", "lfs"],
            },
        ],
    });
    let second = ServerResponse::new(429, "text/html", "<html><body>challenge</body></html>\n");
    assert_snapshot!(answered(
        &["show", "example.com/acme/widget@main:big.c", "1-4"],
        [first.into(), second]
    ));
}

/// A raw read that fails where it only made Sourcegraph's text exact leaves the text, with U+FFFD, and a note.
#[test]
fn raw_read_failure_leaves_bytes_unknown() {
    let first = json!({
        "data": {
            "r0": {
                "commit": {
                    "body": {
                        "__typename": "GitBlob",
                        "content": "        perimeter(&rect1)\n    );\n    println!(r#\"This line contains invalid utf8:  \"�����\"#;\n}\n",
                    },
                    "meta": {
                        "__typename": "GitBlob",
                        "binary": false,
                        "byteSize": 533,
                        "lfs": null,
                        "path": "tests/snapshots/sample.modified.rs",
                        "rawURL": "/r/github.com/sharkdp/bat@a9aa73d215455a479f072da75dd39f19826acfea/-/raw/tests/snapshots/sample.modified.rs",
                        "submodule": null,
                        "totalLines": 23,
                    },
                    "oid": "a9aa73d215455a479f072da75dd39f19826acfea",
                },
                "defaultBranch": {"displayName": "master"},
                "mirrorInfo": {"updatedAt": "2026-10-27T04:50:20Z"},
                "name": "github.com/sharkdp/bat",
            },
        },
    });
    let second = ServerResponse::new(503, "text/html", "");
    assert_snapshot!(answered(
        &["show", "github.com/sharkdp/bat@a9aa73d2:tests/snapshots/sample.modified.rs", "12-15"],
        [first.into(), second]
    ));
}

/// In JSON a raw read that fails where it only made the text exact leaves utf8 null.
#[test]
fn raw_read_failure_leaves_bytes_unknown_json() {
    let first = json!({
        "data": {
            "r0": {
                "commit": {
                    "body": {
                        "__typename": "GitBlob",
                        "content": "    println!(r#\"This line contains invalid utf8:  \"�����\"#;\n",
                    },
                    "meta": {
                        "__typename": "GitBlob",
                        "binary": false,
                        "byteSize": 533,
                        "lfs": null,
                        "path": "tests/snapshots/sample.modified.rs",
                        "rawURL": "/r/github.com/sharkdp/bat@a9aa73d215455a479f072da75dd39f19826acfea/-/raw/tests/snapshots/sample.modified.rs",
                        "submodule": null,
                        "totalLines": 23,
                    },
                    "oid": "a9aa73d215455a479f072da75dd39f19826acfea",
                },
                "defaultBranch": {"displayName": "master"},
                "mirrorInfo": {"updatedAt": "2026-10-27T04:50:20Z"},
                "name": "github.com/sharkdp/bat",
            },
        },
    });
    let second = ServerResponse::new(503, "text/html", "");
    assert_snapshot!(answered(
        &[
            "show",
            "--json",
            "github.com/sharkdp/bat@a9aa73d2:tests/snapshots/sample.modified.rs",
            "14"
        ],
        [first.into(), second]
    ));
}

/// The raw read is reported in JSON.
#[test]
fn raw_read_in_json() {
    assert_snapshot!(sego(&[
        "show",
        "--json",
        "github.com/torvalds/linux@c339f4010dea8b0d754f84ace70cd1b089878ef8:drivers/gpu/drm/amd/include/asic_reg/nbif/nbif_7_10_0_offset.h",
        "1-5"
    ]));
}

/// A raw read the server cannot find is a failure.
/// Located errors as in `show::binary_file_over_content_limit`.
#[test]
fn raw_read_not_found() {
    let first = json!({
        "data": {
            "r0": {
                "commit": {
                    "body": null,
                    "meta": {
                        "__typename": "GitBlob",
                        "binary": false,
                        "byteSize": 2_000_000,
                        "lfs": null,
                        "path": "big.c",
                        "rawURL": "/r/example.com/acme/widget@d81fd54f1a2b3c4d5e6f708192a3b4c5d6e7f809/-/raw/big.c",
                        "submodule": null,
                        "totalLines": 5,
                    },
                    "oid": "d81fd54f1a2b3c4d5e6f708192a3b4c5d6e7f809",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
        },
        "errors": [
            {
                "message": "file exceeded size limit: BytesRead=1572865 (may be lower than file size)",
                "path": ["r0", "commit", "body", "content"],
            },
            {
                "message": "file exceeded size limit: BytesRead=1572865 (may be lower than file size)",
                "path": ["r0", "commit", "meta", "lfs"],
            },
        ],
    });
    let second = ServerResponse::new(404, "text/html", "<html>not found</html>\n");
    assert_snapshot!(answered(
        &["show", "example.com/acme/widget@main:big.c", "1-2"],
        [first.into(), second]
    ));
}

/// An open range of a raw read ends at a last line with no newline.
/// Located errors as in `show::binary_file_over_content_limit`.
#[test]
fn raw_read_open_range_in_json() {
    let first = json!({
        "data": {
            "r0": {
                "commit": {
                    "body": null,
                    "meta": {
                        "__typename": "GitBlob",
                        "binary": false,
                        "byteSize": 2_000_000,
                        "lfs": null,
                        "path": "big.c",
                        "rawURL": "/r/example.com/acme/widget@d81fd54f1a2b3c4d5e6f708192a3b4c5d6e7f809/-/raw/big.c",
                        "submodule": null,
                        "totalLines": 5,
                    },
                    "oid": "d81fd54f1a2b3c4d5e6f708192a3b4c5d6e7f809",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
        },
        "errors": [
            {
                "message": "file exceeded size limit: BytesRead=1572865 (may be lower than file size)",
                "path": ["r0", "commit", "body", "content"],
            },
            {
                "message": "file exceeded size limit: BytesRead=1572865 (may be lower than file size)",
                "path": ["r0", "commit", "meta", "lfs"],
            },
        ],
    });
    let second = ServerResponse::new(200, "text/plain; charset=utf-8", "1\n2\n3\n4\n5");
    assert_snapshot!(answered(
        &["show", "--json", "example.com/acme/widget@main:big.c", "5-"],
        [first.into(), second]
    ));
}

/// A file over the content limit is read from the raw route.
#[test]
fn raw_read_over_content_limit() {
    assert_snapshot!(sego(&[
        "show",
        "github.com/torvalds/linux@c339f4010dea8b0d754f84ace70cd1b089878ef8:drivers/gpu/drm/amd/include/asic_reg/nbif/nbif_7_10_0_offset.h",
        "1-5"
    ]));
}

/// A raw read the server fails is a failure.
/// Located errors as in `show::binary_file_over_content_limit`.
#[test]
fn raw_read_server_error() {
    let first = json!({
        "data": {
            "r0": {
                "commit": {
                    "body": null,
                    "meta": {
                        "__typename": "GitBlob",
                        "binary": false,
                        "byteSize": 2_000_000,
                        "lfs": null,
                        "path": "big.c",
                        "rawURL": "/r/example.com/acme/widget@d81fd54f1a2b3c4d5e6f708192a3b4c5d6e7f809/-/raw/big.c",
                        "submodule": null,
                        "totalLines": 5,
                    },
                    "oid": "d81fd54f1a2b3c4d5e6f708192a3b4c5d6e7f809",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
        },
        "errors": [
            {
                "message": "file exceeded size limit: BytesRead=1572865 (may be lower than file size)",
                "path": ["r0", "commit", "body", "content"],
            },
            {
                "message": "file exceeded size limit: BytesRead=1572865 (may be lower than file size)",
                "path": ["r0", "commit", "meta", "lfs"],
            },
        ],
    });
    let second = ServerResponse::new(502, "text/plain", "bad gateway\n");
    assert_snapshot!(answered(
        &["show", "example.com/acme/widget@main:big.c", "1-2"],
        [first.into(), second]
    ));
}

/// The lines of a raw read of a large file are escaped.
/// Located errors as in `show::binary_file_over_content_limit`.
#[test]
fn raw_read_with_control_characters() {
    let first = json!({
        "data": {
            "r0": {
                "commit": {
                    "body": null,
                    "meta": {
                        "__typename": "GitBlob",
                        "binary": false,
                        "byteSize": 2_000_000,
                        "lfs": null,
                        "path": "big.txt",
                        "rawURL": "/r/example.com/acme/widget@0123456789abcdef0123456789abcdef01234567/-/raw/big.txt",
                        "submodule": null,
                        "totalLines": 8,
                    },
                    "oid": "0123456789abcdef0123456789abcdef01234567",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
        },
        "errors": [
            {
                "message": "file exceeded size limit: BytesRead=1572865 (may be lower than file size)",
                "path": ["r0", "commit", "body", "content"],
            },
            {
                "message": "file exceeded size limit: BytesRead=1572865 (may be lower than file size)",
                "path": ["r0", "commit", "meta", "lfs"],
            },
        ],
    });
    let second = ServerResponse::new(
        200,
        "text/plain; charset=utf-8",
        "a\u{1b}b\nc\u{202e}d\ne\u{200b}f\ng\0h\ni\u{2028}j\nk\u{7f}l\r\nmid\rcr\ntab\tok\n",
    );
    assert_snapshot!(answered(
        &[
            "show",
            "example.com/acme/widget@0123456789abcdef0123456789abcdef01234567:big.txt",
            "1-7"
        ],
        [first.into(), second]
    ));
}

/// CRLF is dropped from the lines of a raw read and named in the header.
/// Located errors as in `show::binary_file_over_content_limit`.
#[test]
fn raw_read_with_crlf() {
    let first = json!({
        "data": {
            "r0": {
                "commit": {
                    "body": null,
                    "meta": {
                        "__typename": "GitBlob",
                        "binary": false,
                        "byteSize": 2_000_000,
                        "lfs": null,
                        "path": "big.c",
                        "rawURL": "/r/example.com/acme/widget@d81fd54f1a2b3c4d5e6f708192a3b4c5d6e7f809/-/raw/big.c",
                        "submodule": null,
                        "totalLines": 5,
                    },
                    "oid": "d81fd54f1a2b3c4d5e6f708192a3b4c5d6e7f809",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
        },
        "errors": [
            {
                "message": "file exceeded size limit: BytesRead=1572865 (may be lower than file size)",
                "path": ["r0", "commit", "body", "content"],
            },
            {
                "message": "file exceeded size limit: BytesRead=1572865 (may be lower than file size)",
                "path": ["r0", "commit", "meta", "lfs"],
            },
        ],
    });
    let second = ServerResponse::new(200, "text/plain; charset=utf-8", "1\r\n2\r\n3\r\n4\r\n5\r\n");
    assert_snapshot!(answered(
        &["show", "example.com/acme/widget@main:big.c", "2-3"],
        [first.into(), second]
    ));
}

/// The server's message for a rejected request is escaped in the error.
#[test]
fn rejected_request_with_control_characters() {
    let response = json!({
        "data": {},
        "errors": [{"message": "a\u{1b}b\u{202e}c\u{200b}d\ne\rf\0g\u{2028}h\u{7f}i"}],
    });
    assert_snapshot!(answered(&["show", "example.com/acme/widget:f.txt"], [response]));
}

/// A relative `SEGO_CACHE_DIR` is noted and the raw read answers.
/// Located errors as in `show::binary_file_over_content_limit`.
#[test]
fn relative_cache_dir_is_noted() {
    let first = json!({
        "data": {
            "r0": {
                "commit": {
                    "body": null,
                    "meta": {
                        "__typename": "GitBlob",
                        "binary": false,
                        "byteSize": 2_000_000,
                        "lfs": null,
                        "path": "big.c",
                        "rawURL": "/r/example.com/acme/widget@d81fd54f1a2b3c4d5e6f708192a3b4c5d6e7f809/-/raw/big.c",
                        "submodule": null,
                        "totalLines": 5,
                    },
                    "oid": "d81fd54f1a2b3c4d5e6f708192a3b4c5d6e7f809",
                },
                "defaultBranch": {"displayName": "main"},
                "name": "example.com/acme/widget",
            },
        },
        "errors": [
            {
                "message": "file exceeded size limit: BytesRead=1572865 (may be lower than file size)",
                "path": ["r0", "commit", "body", "content"],
            },
            {
                "message": "file exceeded size limit: BytesRead=1572865 (may be lower than file size)",
                "path": ["r0", "commit", "meta", "lfs"],
            },
        ],
    });
    let second = ServerResponse::new(200, "text/plain; charset=utf-8", "1\n2\n3\n4\n5\n");
    assert_snapshot!(
        Case::new(
            &["show", "example.com/acme/widget@main:big.c", "1-5"],
            vec![first.into(), second]
        )
        .cache_dir("rel")
        .ran()
    );
}

/// A repository with no commits is an error.
#[test]
fn repository_with_no_commits() {
    assert_snapshot!(sego(&["show", "github.com/ascoders/blog:README.md"]));
}

/// The root, written as `:.`, prints the listing of the repository.
#[test]
fn root_lists_the_repository() {
    assert_snapshot!(sego(&["show", "github.com/gorilla/mux@v1.8.0:."]));
}

/// The root, written as a bare `:`, prints the listing of the repository.
#[test]
fn root_written_as_a_bare_colon() {
    assert_snapshot!(sego(&["show", "github.com/gorilla/mux@v1.8.0:"]));
}

/// A single line.
#[test]
fn single_line() {
    assert_snapshot!(sego(&["show", "github.com/gorilla/mux@v1.8.0:mux.go", "10"]));
}

/// A branch name with a slash, in an address.
#[test]
fn slashed_branch_in_an_address() {
    assert_snapshot!(sego(&["show", "github.com/bminor/glibc@release/2.34/master:NEWS", "1-3"]));
}

/// A submodule gives its commit.
#[test]
fn submodule_gives_its_commit() {
    assert_snapshot!(sego(&[
        "show",
        "github.com/sharkdp/bat@a9aa73d2:assets/syntaxes/01_Packages"
    ]));
}

/// A submodule in JSON has its commit.
#[test]
fn submodule_in_json() {
    assert_snapshot!(sego(&[
        "show",
        "--json",
        "github.com/akinomyoga/ble.sh@d81fd54feb0d:contrib"
    ]));
}

/// A symlink is a one-line file.
#[test]
fn symlink() {
    assert_snapshot!(sego(&["show", "github.com/BurntSushi/ripgrep@14.1.0:HomebrewFormula"]));
}

/// A reflog or upstream revision names nothing in a mirror.
#[test]
fn upstream_revision() {
    assert_snapshot!(sego(&["show", "github.com/gorilla/mux@@{upstream}:mux.go"]));
}

/// UTF-16 text is printed byte for byte, its bytes that aren't UTF-8 as hex escapes.
#[test]
fn utf_16_text_is_read_raw() {
    let first = json!({
        "data": {
            "r0": {
                "commit": {
                    "body": {
                        "__typename": "GitBlob",
                        "content": "��h\0e\0l\0l\0o\0 \0w\0o\0r\0l\0d\0\n\0",
                    },
                    "meta": {
                        "__typename": "GitBlob",
                        "binary": false,
                        "byteSize": 26,
                        "lfs": null,
                        "path": "tests/examples/test_UTF-16LE.txt",
                        "rawURL": "/r/github.com/sharkdp/bat@a9aa73d215455a479f072da75dd39f19826acfea/-/raw/tests/examples/test_UTF-16LE.txt",
                        "submodule": null,
                        "totalLines": 2,
                    },
                    "oid": "a9aa73d215455a479f072da75dd39f19826acfea",
                },
                "defaultBranch": {"displayName": "master"},
                "mirrorInfo": {"updatedAt": "2026-10-31T00:17:18Z"},
                "name": "github.com/sharkdp/bat",
            },
        },
    });
    let second = ServerResponse::new(200, "text/plain; charset=utf-8", b"\xff\xfe\x68\x00\x65\x00\x6c\x00\x6c\x00\x6f\x00\x20\x00\x77\x00\x6f\x00\x72\x00\x6c\x00\x64\x00\x0a\x00");
    assert_snapshot!(answered(
        &["show", "github.com/sharkdp/bat@a9aa73d2:tests/examples/test_UTF-16LE.txt"],
        [first.into(), second]
    ));
}

/// A web URL is not an address: the error names the forms that are.
#[test]
fn web_url_is_not_an_address() {
    assert_snapshot!(sego(&["show", "https://github.com/gorilla/mux/blob/v1.8.0/mux.go#L10-L20"]));
}
