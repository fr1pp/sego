//! Reading a file: its metadata, line count and a range of its lines.

use serde::Deserialize;

use crate::address::{Address, CommitId, Pinned, RepoName, RepoPath};
use crate::diagnostic::Diagnostic;
use crate::domain::{File, NotAFile, Unavailable};

use super::envelope::Envelope;
use super::{FetchError, Plan};

/// `node` gives the path's type and metadata, `lines` its line count and
/// `body` the range. Each is its own alias, so the read cap's error at
/// `body` leaves the others intact.
const QUERY: &str = r"query Read(
  $repo: String!, $rev: String!, $path: String!, $start: Int!, $end: Int
) {
  repository(name: $repo) {
    name
    commit(rev: $rev) {
      oid
      node: path(path: $path) {
        __typename
        ... on GitBlob {
          path byteSize binary contentType submodule { url commit }
        }
        ... on GitTree { path }
      }
      lines: path(path: $path) { ... on GitBlob { totalLines } }
      body: path(path: $path) {
        ... on GitBlob {
          content(startLine: $start, endLine: $end, textOnly: true)
        }
      }
    }
  }
}
";

/// Lines `start` to `end` of `path`, 1-based and inclusive; no `end` runs
/// to the last line.
#[derive(Debug, Clone)]
pub struct ReadRequest {
    pub address: Address,
    pub path: RepoPath,
    pub start: u32,
    pub end: Option<u32>,
    /// No range was asked for, so the range is the line cap's.
    pub capped: bool,
}

pub fn plan(request: &ReadRequest) -> Plan {
    Plan {
        query: QUERY,
        variables: serde_json::json!({
            "repo": request.address.repo.as_str(),
            "rev": request.address.rev_or_head(),
            "path": request.path.upstream(),
            "start": request.start,
            "end": request.end,
        }),
    }
}

/// The path of the read cap's error.
const CONTENT: &[&str] = &["repository", "commit", "body", "content"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    repository: Option<Repository>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Repository {
    name: String,
    commit: Option<Commit>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Commit {
    oid: String,
    node: Option<Node>,
    lines: Option<Lines>,
    body: Option<Body>,
}

#[derive(Deserialize)]
#[serde(tag = "__typename", deny_unknown_fields)]
enum Node {
    #[serde(rename_all = "camelCase")]
    GitBlob {
        path: String,
        byte_size: u64,
        binary: bool,
        content_type: String,
        submodule: Option<Submodule>,
    },
    GitTree {
        path: String,
    },
}

/// Only its presence matters here.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Submodule {
    #[serde(rename = "url")]
    _url: String,
    #[serde(rename = "commit")]
    _commit: String,
}

/// `totalLines` is absent unless the path is a blob, and never null.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Lines {
    total_lines: Option<u32>,
}

/// `content` is absent unless the path is a blob, and never null: over the
/// read cap, the error nulls the whole `body` alias instead.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Body {
    content: Option<String>,
}

/// The file `envelope` answers `request` with, or why there is none.
pub fn answer(
    request: &ReadRequest,
    mut envelope: Envelope,
) -> Result<File, Box<dyn Diagnostic>> {
    let too_large = match envelope.take_error(CONTENT) {
        Some(error) if error.message.contains("exceeded size limit") => true,
        Some(error) => {
            envelope.errors.push(error);
            false
        }
        None => false,
    };
    let trace = envelope.trace.clone();
    // A blob's `lines` or `body` alias is null only with an error at its
    // path: the read cap's, handled before either alias is read, or another,
    // which `into_data` reports. So only drift reaches this.
    let missing = |field: &str| FetchError::Field {
        cause: format!("a blob has no {field}"),
        trace: trace.clone(),
    };
    let Data { repository } = envelope.into_data()?;
    let Some(Repository { name, commit }) = repository else {
        let repo = request.address.repo.as_str().to_owned();
        return Err(Unavailable::RepoNotFound { repo }.into());
    };
    let Some(Commit { oid, node, lines, body }) = commit else {
        let rev = request.address.rev_or_head().to_owned();
        return Err(Unavailable::RevisionNotFound { repo: name, rev }.into());
    };
    let repo = RepoName::parse(&name).map_err(|error| FetchError::Field {
        cause: error.to_string(),
        trace: trace.clone(),
    })?;
    let at = Pinned { repo, commit: CommitId(oid) };
    let (path, byte_size, binary, content_type) = match node {
        None => {
            let path = request.path.upstream().to_owned();
            return Err(Unavailable::PathNotFound { at, path }.into());
        }
        Some(Node::GitTree { path }) => {
            let what = NotAFile::Directory;
            return Err(Unavailable::NotAFile { at, path, what }.into());
        }
        Some(Node::GitBlob { path, submodule: Some(_), .. }) => {
            let what = NotAFile::Submodule;
            return Err(Unavailable::NotAFile { at, path, what }.into());
        }
        Some(Node::GitBlob {
            path,
            byte_size,
            binary,
            content_type,
            submodule: None,
        }) => (path, byte_size, binary, content_type),
    };
    // A binary file answers with its metadata alone, so the read cap,
    // which only withholds the content, doesn't stop it.
    if too_large && !binary {
        return Err(Unavailable::TooLarge { at, path, byte_size }.into());
    }
    let total_lines = lines
        .and_then(|lines| lines.total_lines)
        .ok_or_else(|| missing("totalLines"))?;
    let start = request.start;
    // A binary file's `totalLines` counts its raw bytes' newlines, so no
    // range applies to it. An empty file answers line 1 with no lines.
    let lines = if binary {
        Vec::new()
    } else if start > total_lines.max(1) {
        return Err(
            Unavailable::OutOfRange { at, path, start, total_lines }.into()
        );
    } else {
        let content = body
            .and_then(|body| body.content)
            .ok_or_else(|| missing("content"))?;
        let mut lines: Vec<String> =
            content.split('\n').map(str::to_owned).collect();
        if lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        lines
    };
    Ok(File {
        at,
        path,
        byte_size,
        total_lines,
        content_type,
        binary,
        start,
        lines,
    })
}

#[cfg(test)]
mod tests {
    use expect_test::expect;

    use super::*;
    use crate::diagnostic::report;
    use crate::sourcegraph::testing::{self, decoded, failure};

    /// The capture cases, by name: each is `tests/read/<name>.json`.
    fn cases() -> Vec<(&'static str, ReadRequest)> {
        let read = |address: &str, path: &str, start, end| ReadRequest {
            address: Address::parse(address).unwrap(),
            path: RepoPath::parse(path),
            start,
            end: Some(end),
            capped: false,
        };
        let mux = "github.com/gorilla/mux@v1.7.0";
        vec![
            ("range", read(mux, "mux.go", 213, 219)),
            ("capped", read(mux, "mux.go", 1, 500)),
            ("directory", read(mux, ".github", 1, 500)),
            ("missing-path", read(mux, "nope.go", 1, 500)),
            (
                "missing-repo",
                read("github.com/gorilla/doesnotexist", "x", 1, 500),
            ),
            (
                "missing-revision",
                read("github.com/gorilla/mux@v9.9.9", "mux.go", 1, 500),
            ),
            ("past-end", read(mux, "mux.go", 700, 710)),
            (
                "too-large",
                read(
                    "github.com/microsoft/TypeScript@v5.6.2",
                    "src/compiler/checker.ts",
                    1,
                    3,
                ),
            ),
            (
                "submodule",
                read(
                    "github.com/rust-lang/rust@1.80.0",
                    "src/tools/cargo",
                    1,
                    500,
                ),
            ),
            (
                "symlink",
                read(
                    "github.com/torvalds/linux@v6.10",
                    "scripts/dtc/include-prefixes/arm",
                    1,
                    500,
                ),
            ),
            (
                "binary",
                read(
                    "github.com/golang/go@go1.23.0",
                    "src/archive/zip/testdata/gophercolor16x16.png",
                    1,
                    500,
                ),
            ),
            (
                "binary-over-cap",
                read(
                    "github.com/golang/go@go1.23.0",
                    "src/crypto/internal/boring/syso/\
                     goboringcrypto_linux_amd64.syso",
                    1,
                    500,
                ),
            ),
            (
                "canonical",
                read("GitHub.com/Gorilla/Mux@v1.7.0", "mux.go", 1, 1),
            ),
        ]
    }

    fn case(name: &str) -> ReadRequest {
        cases()
            .into_iter()
            .find_map(|(case, request)| (case == name).then_some(request))
            .unwrap_or_else(|| panic!("no case {name}"))
    }

    #[test]
    #[ignore = "network: run by just capture"]
    fn capture() {
        for (case, request) in cases() {
            testing::capture("read", case, &plan(&request));
        }
    }

    /// The answer to case `name`, whose capture is `capture`.
    fn read(name: &str, capture: &str) -> Result<File, Box<dyn Diagnostic>> {
        answer(&case(name), testing::envelope(capture))
    }

    /// The file a result answers with.
    fn answered(result: Result<File, Box<dyn Diagnostic>>) -> File {
        result.unwrap_or_else(|diagnostic| panic!("{}", report(&*diagnostic)))
    }

    #[test]
    fn range() {
        let file = answered(read(
            "range",
            include_str!("../../tests/read/range.json"),
        ));
        assert_eq!((file.start, file.lines.len()), (213, 7));
        expect![[r#"
            File {
                at: Pinned {
                    repo: RepoName(
                        "github.com/gorilla/mux",
                    ),
                    commit: CommitId(
                        "a7962380ca08b5a188038c69871b8d3fbdf31e89",
                    ),
                },
                path: "mux.go",
                byte_size: 17526,
                total_lines: 601,
                content_type: "text/plain; charset=utf-8",
                binary: false,
                start: 213,
                lines: [
                    "}",
                    "",
                    "// Get returns a route registered with the given name.",
                    "func (r *Router) Get(name string) *Route {",
                    "\treturn r.namedRoutes[name]",
                    "}",
                    "",
                ],
            }
        "#]]
        .assert_debug_eq(&file);
    }

    #[test]
    fn capped() {
        let file = answered(read(
            "capped",
            include_str!("../../tests/read/capped.json"),
        ));
        assert_eq!(
            (file.start, file.lines.len(), file.total_lines),
            (1, 500, 601)
        );
        expect![[r#"
            (
                "// Copyright 2012 The Gorilla Authors. All rights reserved.",
                "\tlength, err := checkPairs(pairs...)",
            )
        "#]]
        .assert_debug_eq(&(&file.lines[0], &file.lines[499]));
    }

    #[test]
    fn directory() {
        let result =
            read("directory", include_str!("../../tests/read/directory.json"));
        expect![[r#"
            [not_a_file AnsweredNo]
            .github is a directory in github.com/gorilla/mux@a7962380ca08
            hint: sego list github.com/gorilla/mux@a7962380ca08 .github/
        "#]]
        .assert_eq(&failure(result));
    }

    #[test]
    fn missing_path() {
        let result = read(
            "missing-path",
            include_str!("../../tests/read/missing-path.json"),
        );
        expect![[r#"
            [path_not_found AnsweredNo]
            nope.go not found in github.com/gorilla/mux@a7962380ca08
            hint: sego list github.com/gorilla/mux@a7962380ca08
        "#]]
        .assert_eq(&failure(result));
    }

    #[test]
    fn missing_repo() {
        let result = read(
            "missing-repo",
            include_str!("../../tests/read/missing-repo.json"),
        );
        expect![[r#"
            [repo_not_found AnsweredNo]
            repository github.com/gorilla/doesnotexist not found
            hint: sego search 'type:repo doesnotexist'
        "#]]
        .assert_eq(&failure(result));
    }

    #[test]
    fn missing_revision() {
        let result = read(
            "missing-revision",
            include_str!("../../tests/read/missing-revision.json"),
        );
        expect![[r#"
            [revision_not_found AnsweredNo]
            revision v9.9.9 not found in github.com/gorilla/mux
            hint: sego list github.com/gorilla/mux
        "#]]
        .assert_eq(&failure(result));
    }

    #[test]
    fn past_end() {
        let result =
            read("past-end", include_str!("../../tests/read/past-end.json"));
        expect![[r#"
            [out_of_range AnsweredNo]
            mux.go in github.com/gorilla/mux@a7962380ca08 has 601 lines, so line 700 is past its end
            hint: sego read github.com/gorilla/mux@a7962380ca08 mux.go 1-601
        "#]].assert_eq(&failure(result));
    }

    #[test]
    fn too_large() {
        let result =
            read("too-large", include_str!("../../tests/read/too-large.json"));
        expect![[r#"
            [too_large AnsweredNo]
            src/compiler/checker.ts in github.com/microsoft/TypeScript@a7e3374f1332 is 3057771 bytes, over the upstream's 1.5 MiB read cap
        "#]].assert_eq(&failure(result));
    }

    #[test]
    fn submodule() {
        let result =
            read("submodule", include_str!("../../tests/read/submodule.json"));
        expect![[r#"
            [not_a_file AnsweredNo]
            src/tools/cargo is a submodule in github.com/rust-lang/rust@051478957371
            hint: sego list github.com/rust-lang/rust@051478957371 src/tools/cargo
        "#]].assert_eq(&failure(result));
    }

    #[test]
    fn symlink() {
        let file = answered(read(
            "symlink",
            include_str!("../../tests/read/symlink.json"),
        ));
        assert_eq!(file.path, "scripts/dtc/include-prefixes/arm");
        assert_eq!(file.lines, ["../../../arch/arm/boot/dts"]);
        expect![[r#"
            File {
                at: Pinned {
                    repo: RepoName(
                        "github.com/torvalds/linux",
                    ),
                    commit: CommitId(
                        "0c3836482481200ead7b416ca80c68a29cfdaabd",
                    ),
                },
                path: "scripts/dtc/include-prefixes/arm",
                byte_size: 26,
                total_lines: 1,
                content_type: "application/octet-stream",
                binary: false,
                start: 1,
                lines: [
                    "../../../arch/arm/boot/dts",
                ],
            }
        "#]]
        .assert_debug_eq(&file);
    }

    #[test]
    fn binary() {
        let file = answered(read(
            "binary",
            include_str!("../../tests/read/binary.json"),
        ));
        assert!(file.binary && file.lines.is_empty());
        assert_eq!(
            (file.content_type.as_str(), file.byte_size),
            ("image/png", 785)
        );
        expect![[r#"
            File {
                at: Pinned {
                    repo: RepoName(
                        "github.com/golang/go",
                    ),
                    commit: CommitId(
                        "6885bad7dd86880be6929c02085e5c7a67ff2887",
                    ),
                },
                path: "src/archive/zip/testdata/gophercolor16x16.png",
                byte_size: 785,
                total_lines: 8,
                content_type: "image/png",
                binary: true,
                start: 1,
                lines: [],
            }
        "#]]
        .assert_debug_eq(&file);
    }

    #[test]
    fn binary_over_cap() {
        let file = answered(read(
            "binary-over-cap",
            include_str!("../../tests/read/binary-over-cap.json"),
        ));
        expect![[r#"
            File {
                at: Pinned {
                    repo: RepoName(
                        "github.com/golang/go",
                    ),
                    commit: CommitId(
                        "6885bad7dd86880be6929c02085e5c7a67ff2887",
                    ),
                },
                path: "src/crypto/internal/boring/syso/goboringcrypto_linux_amd64.syso",
                byte_size: 2555664,
                total_lines: 3790,
                content_type: "application/octet-stream",
                binary: true,
                start: 1,
                lines: [],
            }
        "#]].assert_debug_eq(&file);
    }

    #[test]
    fn canonical() {
        let file = answered(read(
            "canonical",
            include_str!("../../tests/read/canonical.json"),
        ));
        assert_eq!(file.at.repo.as_str(), "github.com/gorilla/mux");
        expect![[r#"
            File {
                at: Pinned {
                    repo: RepoName(
                        "github.com/gorilla/mux",
                    ),
                    commit: CommitId(
                        "a7962380ca08b5a188038c69871b8d3fbdf31e89",
                    ),
                },
                path: "mux.go",
                byte_size: 17526,
                total_lines: 601,
                content_type: "text/plain; charset=utf-8",
                binary: false,
                start: 1,
                lines: [
                    "// Copyright 2012 The Gorilla Authors. All rights reserved.",
                ],
            }
        "#]].assert_debug_eq(&file);
    }

    #[test]
    fn binary_files_answer_whatever_the_range() {
        let request =
            ReadRequest { start: 20, end: Some(30), ..case("binary") };
        let capture = include_str!("../../tests/read/binary.json");
        let file = answered(answer(&request, testing::envelope(capture)));
        assert!(file.start > file.total_lines);
        assert!(file.binary && file.lines.is_empty());
    }

    /// A reply for a blob with `total_lines` lines and `content`.
    fn blob(total_lines: u32, content: &str) -> Envelope {
        let body = serde_json::json!({"data": {"repository": {
            "name": "github.com/gorilla/mux",
            "commit": {
                "oid": "a7962380ca08b5a188038c69871b8d3fbdf31e89",
                "node": {"__typename": "GitBlob", "path": "empty.go",
                    "byteSize": content.len(), "binary": false,
                    "contentType": "text/plain", "submodule": null},
                "lines": {"totalLines": total_lines},
                "body": {"content": content}
            }
        }}});
        decoded(body.to_string())
    }

    #[test]
    fn an_empty_file_has_no_lines_and_no_line_2() {
        let read = |start| ReadRequest {
            path: RepoPath::parse("empty.go"),
            start,
            end: None,
            ..case("range")
        };
        let file = answered(answer(&read(1), blob(0, "")));
        assert_eq!((file.total_lines, file.lines.len()), (0, 0));
        let result = answer(&read(2), blob(0, ""));
        expect![[r#"
            [out_of_range AnsweredNo]
            empty.go in github.com/gorilla/mux@a7962380ca08 has 0 lines, so line 2 is past its end
        "#]].assert_eq(&failure(result));
    }

    #[test]
    fn missing_oid_is_bad_response() {
        let body = br#"{"data":{"repository":{
            "name":"github.com/gorilla/mux",
            "commit":{"oid":null,"node":null,"lines":null,"body":null}
        }}}"#;
        let result = answer(&case("range"), decoded(body));
        expect![[r#"
            [bad_response Unanswered]
            error: the upstream's answer has missing or unknown fields
            caused by: invalid type: null, expected a string
        "#]]
        .assert_eq(&failure(result));
    }

    #[test]
    fn a_submodule_without_its_url_and_commit_is_bad_response() {
        let body = br#"{"data":{"repository":{
            "name":"github.com/rust-lang/rust",
            "commit":{
                "oid":"a7962380ca08b5a188038c69871b8d3fbdf31e89",
                "node":{"__typename":"GitBlob","path":"src/tools/cargo",
                    "byteSize":0,"binary":false,
                    "contentType":"text/plain","submodule":{}},
                "lines":{"totalLines":0},
                "body":{"content":""}
            }
        }}}"#;
        let result = answer(&case("submodule"), decoded(body));
        expect![[r#"
            [bad_response Unanswered]
            error: the upstream's answer has missing or unknown fields
            caused by: missing field `url`
        "#]]
        .assert_eq(&failure(result));
    }

    #[test]
    fn other_errors_at_the_content_are_server_errors() {
        let body = br#"{"data":{"repository":{
            "name":"github.com/gorilla/mux",
            "commit":{
                "oid":"a7962380ca08b5a188038c69871b8d3fbdf31e89",
                "node":{"__typename":"GitBlob","path":"mux.go",
                    "byteSize":17526,"binary":false,
                    "contentType":"text/plain","submodule":null},
                "lines":{"totalLines":601},
                "body":null
            }
        }},"errors":[{"message":"boom",
            "path":["repository","commit","body","content"]}]}"#;
        let result = answer(&case("range"), decoded(body));
        expect![[r#"
            [server_error Unanswered]
            error: the upstream answered with an error
            caused by: boom
        "#]]
        .assert_eq(&failure(result));
    }
}
