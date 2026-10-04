//! Listing a directory: its entries, to a depth, or where a submodule
//! points.

use serde::Deserialize;

use crate::address::{Address, CommitId, Pinned, RepoName, RepoPath};
use crate::diagnostic::Diagnostic;
use crate::domain::{Entry, EntryKind, Listing, Submodule, Tree, Unavailable};

use super::envelope::Envelope;
use super::{FetchError, Plan};

/// Recursive `entries` has no depth limit, so a recursive listing is
/// filtered to the depth asked for.
const QUERY: &str = r"query List(
  $repo: String!, $rev: String!, $path: String!, $recursive: Boolean!,
  $first: Int!
) {
  repository(name: $repo) {
    name
    commit(rev: $rev) {
      oid
      node: path(path: $path) {
        __typename
        ... on GitBlob { path submodule { url commit } }
        ... on GitTree {
          path
          entries(first: $first, recursive: $recursive) {
            path isDirectory submodule { url commit }
          }
        }
      }
    }
  }
}
";

/// The most entries one listing asks for. The upstream cuts a listing at
/// this many without saying so.
pub const LIST_ENTRY_CAP: u32 = 1000;

/// The entries of `path` down to `depth` levels below it; 1 is its own
/// entries.
#[derive(Debug, Clone)]
pub struct ListRequest {
    pub address: Address,
    pub path: RepoPath,
    pub depth: u32,
}

pub fn plan(request: &ListRequest) -> Plan {
    Plan {
        query: QUERY,
        variables: serde_json::json!({
            "repo": request.address.repo.as_str(),
            "rev": request.address.rev_or_head(),
            "path": request.path.upstream(),
            "recursive": request.depth > 1,
            "first": LIST_ENTRY_CAP,
        }),
    }
}

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
}

#[derive(Deserialize)]
#[serde(tag = "__typename", deny_unknown_fields)]
enum Node {
    GitBlob {
        path: String,
        submodule: Option<Gitlink>,
    },
    GitTree {
        path: String,
        /// Every directory, then every file and submodule, each group in
        /// path order, whatever the depth.
        entries: Vec<TreeEntry>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct TreeEntry {
    path: String,
    is_directory: bool,
    submodule: Option<Gitlink>,
}

/// A submodule: its URL as `.gitmodules` gives it, and the commit it pins.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Gitlink {
    url: String,
    commit: String,
}

/// The listing or submodule `envelope` answers `request` with, or why there
/// is none.
pub fn answer(
    request: &ListRequest,
    envelope: Envelope,
) -> Result<Tree, Box<dyn Diagnostic>> {
    let trace = envelope.trace.clone();
    let Data { repository } = envelope.into_data()?;
    let Some(Repository { name, commit }) = repository else {
        let repo = request.address.repo.as_str().to_owned();
        return Err(Unavailable::RepoNotFound { repo }.into());
    };
    let Some(Commit { oid, node }) = commit else {
        let rev = request.address.rev_or_head().to_owned();
        return Err(Unavailable::RevisionNotFound { repo: name, rev }.into());
    };
    let repo = RepoName::parse(&name).map_err(|error| FetchError::Field {
        cause: error.to_string(),
        trace: trace.clone(),
    })?;
    let at = Pinned { repo, commit: CommitId(oid) };
    let target = |link: Gitlink| -> Result<Pinned, FetchError> {
        let repo = RepoName::from_submodule_url(&link.url, &at.repo)
            .ok_or_else(|| FetchError::Field {
                cause: format!(
                    "a submodule's URL isn't a repository: {}",
                    link.url
                ),
                trace: trace.clone(),
            })?;
        Ok(Pinned { repo, commit: CommitId(link.commit) })
    };
    let (path, upstream) = match node {
        None => {
            let path = request.path.upstream().to_owned();
            return Err(Unavailable::PathNotFound { at, path }.into());
        }
        Some(Node::GitBlob { path, submodule: Some(link) }) => {
            let target = target(link)?;
            return Ok(Tree::Submodule(Submodule { at, path, target }));
        }
        Some(Node::GitBlob { path, submodule: None }) => {
            return Err(Unavailable::NotADirectory { at, path }.into());
        }
        Some(Node::GitTree { path, entries }) => (path, entries),
    };
    let truncated_after = upstream
        .last()
        .filter(|_| upstream.len() >= LIST_ENTRY_CAP as usize)
        .map(|entry| entry.path.clone());
    let depth = request.depth as usize + segments(&path);
    let mut entries = Vec::new();
    for TreeEntry { path: entry, is_directory, submodule } in upstream {
        if segments(&entry) > depth {
            continue;
        }
        let kind = match submodule {
            Some(link) => EntryKind::Submodule(target(link)?),
            None if is_directory => EntryKind::Directory,
            None => EntryKind::File,
        };
        entries.push(Entry { path: entry, kind });
    }
    Ok(Tree::Listing(Listing { at, path, entries, truncated_after }))
}

/// How many segments `path` has; the root has none.
fn segments(path: &str) -> usize {
    if path.is_empty() { 0 } else { path.split('/').count() }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use expect_test::expect;

    use super::*;
    use crate::diagnostic::report;
    use crate::sourcegraph::Reply;
    use crate::sourcegraph::envelope::decode;
    use crate::sourcegraph::testing;

    /// The capture cases, by name: each is `tests/list/<name>.json`.
    fn cases() -> Vec<(&'static str, ListRequest)> {
        let list = |address: &str, path: &str, depth| ListRequest {
            address: Address::parse(address).unwrap(),
            path: RepoPath::parse(path),
            depth,
        };
        let mux = "github.com/gorilla/mux@v1.7.0";
        let rust = "github.com/rust-lang/rust@1.80.0";
        vec![
            ("root", list(mux, "", 1)),
            ("depth-2", list(mux, "", 2)),
            ("subdirectory", list(mux, ".github", 1)),
            ("file", list(mux, "mux.go", 1)),
            ("missing-path", list(mux, "nope/", 1)),
            ("submodules", list(rust, "src/tools", 1)),
            ("submodule", list(rust, "src/tools/cargo", 1)),
            ("truncated", list("github.com/golang/go@go1.23.0", "", 3)),
        ]
    }

    #[test]
    #[ignore = "network: run by just capture"]
    fn capture() {
        for (case, request) in cases() {
            testing::capture("list", case, &plan(&request));
        }
    }

    fn case(name: &str) -> ListRequest {
        cases()
            .into_iter()
            .find_map(|(case, request)| (case == name).then_some(request))
            .unwrap_or_else(|| panic!("no case {name}"))
    }

    /// The answer to case `name`, whose capture is `capture`.
    fn list(name: &str, capture: &str) -> Result<Tree, Box<dyn Diagnostic>> {
        answer(&case(name), testing::envelope(capture))
    }

    /// The listing a result answers with.
    fn listing(result: Result<Tree, Box<dyn Diagnostic>>) -> Listing {
        match result {
            Ok(Tree::Listing(listing)) => listing,
            Ok(tree) => panic!("not a listing: {tree:?}"),
            Err(diagnostic) => panic!("{}", report(&*diagnostic)),
        }
    }

    /// A failure's code, outcome and report.
    fn failure(result: Result<Tree, Box<dyn Diagnostic>>) -> String {
        let diagnostic = match result {
            Ok(tree) => panic!("an answer: {tree:?}"),
            Err(diagnostic) => diagnostic,
        };
        format!(
            "[{} {:?}]\n{}",
            diagnostic.code(),
            diagnostic.outcome(),
            report(&*diagnostic)
        )
    }

    /// The entry at `path`.
    fn entry<'a>(listing: &'a Listing, path: &str) -> &'a EntryKind {
        listing
            .entries
            .iter()
            .find_map(|entry| (entry.path == path).then_some(&entry.kind))
            .unwrap_or_else(|| panic!("no entry {path}"))
    }

    /// Each entry's `Debug` on a line of its own.
    fn one_per_line(entries: &[Entry]) -> String {
        let mut text = String::new();
        for entry in entries {
            let _ = writeln!(text, "{entry:?}");
        }
        text
    }

    fn depth(path: &str) -> usize {
        path.split('/').count()
    }

    #[test]
    fn root() {
        let listing =
            listing(list("root", include_str!("../../tests/list/root.json")));
        assert_eq!(listing.at.word(), "github.com/gorilla/mux@a7962380ca08");
        assert_eq!((listing.path.as_str(), listing.entries.len()), ("", 21));
        assert!(matches!(entry(&listing, ".github"), EntryKind::Directory));
        assert!(matches!(entry(&listing, "mux.go"), EntryKind::File));
        assert_eq!(listing.truncated_after, None);
        expect![[r#"
            Entry { path: ".github", kind: Directory }
            Entry { path: ".travis.yml", kind: File }
            Entry { path: "AUTHORS", kind: File }
            Entry { path: "ISSUE_TEMPLATE.md", kind: File }
            Entry { path: "LICENSE", kind: File }
            Entry { path: "README.md", kind: File }
            Entry { path: "bench_test.go", kind: File }
            Entry { path: "context.go", kind: File }
            Entry { path: "context_test.go", kind: File }
            Entry { path: "doc.go", kind: File }
            Entry { path: "example_authentication_middleware_test.go", kind: File }
            Entry { path: "example_route_test.go", kind: File }
            Entry { path: "go.mod", kind: File }
            Entry { path: "middleware.go", kind: File }
            Entry { path: "middleware_test.go", kind: File }
            Entry { path: "mux.go", kind: File }
            Entry { path: "mux_test.go", kind: File }
            Entry { path: "old_test.go", kind: File }
            Entry { path: "regexp.go", kind: File }
            Entry { path: "route.go", kind: File }
            Entry { path: "test_helpers.go", kind: File }
        "#]].assert_eq(&one_per_line(&listing.entries));
    }

    #[test]
    fn depth_2() {
        let listing = listing(list(
            "depth-2",
            include_str!("../../tests/list/depth-2.json"),
        ));
        assert_eq!(listing.entries.len(), 23);
        assert!(matches!(entry(&listing, ".github"), EntryKind::Directory));
        assert!(matches!(
            entry(&listing, ".github/stale.yml"),
            EntryKind::File
        ));
        assert!(listing.entries.iter().all(|entry| depth(&entry.path) <= 2));
        assert_eq!(listing.truncated_after, None);
        expect![[r#"
            Entry { path: ".github", kind: Directory }
            Entry { path: ".github/release-drafter.yml", kind: File }
            Entry { path: ".github/stale.yml", kind: File }
            Entry { path: ".travis.yml", kind: File }
            Entry { path: "AUTHORS", kind: File }
            Entry { path: "ISSUE_TEMPLATE.md", kind: File }
            Entry { path: "LICENSE", kind: File }
            Entry { path: "README.md", kind: File }
            Entry { path: "bench_test.go", kind: File }
            Entry { path: "context.go", kind: File }
            Entry { path: "context_test.go", kind: File }
            Entry { path: "doc.go", kind: File }
            Entry { path: "example_authentication_middleware_test.go", kind: File }
            Entry { path: "example_route_test.go", kind: File }
            Entry { path: "go.mod", kind: File }
            Entry { path: "middleware.go", kind: File }
            Entry { path: "middleware_test.go", kind: File }
            Entry { path: "mux.go", kind: File }
            Entry { path: "mux_test.go", kind: File }
            Entry { path: "old_test.go", kind: File }
            Entry { path: "regexp.go", kind: File }
            Entry { path: "route.go", kind: File }
            Entry { path: "test_helpers.go", kind: File }
        "#]].assert_eq(&one_per_line(&listing.entries));
    }

    #[test]
    fn subdirectory() {
        let listing = listing(list(
            "subdirectory",
            include_str!("../../tests/list/subdirectory.json"),
        ));
        assert_eq!(listing.path, ".github");
        expect![[r#"
            Listing {
                at: Pinned {
                    repo: RepoName(
                        "github.com/gorilla/mux",
                    ),
                    commit: CommitId(
                        "a7962380ca08b5a188038c69871b8d3fbdf31e89",
                    ),
                },
                path: ".github",
                entries: [
                    Entry {
                        path: ".github/release-drafter.yml",
                        kind: File,
                    },
                    Entry {
                        path: ".github/stale.yml",
                        kind: File,
                    },
                ],
                truncated_after: None,
            }
        "#]]
        .assert_debug_eq(&listing);
    }

    #[test]
    fn file() {
        let result = list("file", include_str!("../../tests/list/file.json"));
        expect![[r#"
            [not_a_directory AnsweredNo]
            mux.go is a file in github.com/gorilla/mux@a7962380ca08
            hint: sego read github.com/gorilla/mux@a7962380ca08 mux.go
        "#]]
        .assert_eq(&failure(result));
    }

    #[test]
    fn missing_path() {
        let result = list(
            "missing-path",
            include_str!("../../tests/list/missing-path.json"),
        );
        expect![[r#"
            [path_not_found AnsweredNo]
            nope not found in github.com/gorilla/mux@a7962380ca08
            hint: sego list github.com/gorilla/mux@a7962380ca08
        "#]]
        .assert_eq(&failure(result));
    }

    #[test]
    fn submodules() {
        let listing = listing(list(
            "submodules",
            include_str!("../../tests/list/submodules.json"),
        ));
        assert_eq!(listing.entries.len(), 47);
        let EntryKind::Submodule(cargo) = entry(&listing, "src/tools/cargo")
        else {
            panic!("src/tools/cargo is not a submodule");
        };
        assert_eq!(
            cargo.to_string(),
            "github.com/rust-lang/cargo@37629051518c"
        );
        assert!(matches!(
            entry(&listing, "src/tools/miri"),
            EntryKind::Directory
        ));
        assert!(matches!(
            entry(&listing, "src/tools/cherry-pick.sh"),
            EntryKind::File
        ));
        let last: Vec<_> = listing.entries.iter().skip(42).collect();
        expect![[r#"
            [
                Entry {
                    path: "src/tools/x",
                    kind: Directory,
                },
                Entry {
                    path: "src/tools/cargo",
                    kind: Submodule(
                        Pinned {
                            repo: RepoName(
                                "github.com/rust-lang/cargo",
                            ),
                            commit: CommitId(
                                "37629051518c3df9ac2c1744589362a02ecafa99",
                            ),
                        },
                    ),
                },
                Entry {
                    path: "src/tools/cherry-pick.sh",
                    kind: File,
                },
                Entry {
                    path: "src/tools/publish_toolstate.py",
                    kind: File,
                },
                Entry {
                    path: "src/tools/rustc-perf",
                    kind: Submodule(
                        Pinned {
                            repo: RepoName(
                                "github.com/rust-lang/rustc-perf",
                            ),
                            commit: CommitId(
                                "72daa50ce2350f5a9b5ae6dc3ad6babccd14ec0a",
                            ),
                        },
                    ),
                },
            ]
        "#]]
        .assert_debug_eq(&last);
    }

    #[test]
    fn submodule() {
        let result =
            list("submodule", include_str!("../../tests/list/submodule.json"));
        let submodule = match result {
            Ok(Tree::Submodule(submodule)) => submodule,
            Ok(tree) => panic!("not a submodule: {tree:?}"),
            Err(diagnostic) => panic!("{}", report(&*diagnostic)),
        };
        assert_eq!(
            (submodule.at.word(), submodule.path.as_str()),
            (
                "github.com/rust-lang/rust@051478957371".to_owned(),
                "src/tools/cargo"
            )
        );
        assert_eq!(
            submodule.target.to_string(),
            "github.com/rust-lang/cargo@37629051518c"
        );
        expect![[r#"
            Submodule {
                at: Pinned {
                    repo: RepoName(
                        "github.com/rust-lang/rust",
                    ),
                    commit: CommitId(
                        "051478957371ee0084a7c0913941d2a8c4757bb9",
                    ),
                },
                path: "src/tools/cargo",
                target: Pinned {
                    repo: RepoName(
                        "github.com/rust-lang/cargo",
                    ),
                    commit: CommitId(
                        "37629051518c3df9ac2c1744589362a02ecafa99",
                    ),
                },
            }
        "#]]
        .assert_debug_eq(&submodule);
    }

    #[test]
    fn truncated() {
        let listing = listing(list(
            "truncated",
            include_str!("../../tests/list/truncated.json"),
        ));
        assert!(listing.entries.len() <= LIST_ENTRY_CAP as usize);
        assert!(listing.entries.iter().all(|entry| depth(&entry.path) <= 3));
        assert_eq!(listing.truncated_after.as_deref(), Some("src/math/cmplx"));
        // Every directory comes before any file, so a cut listing can hold
        // no files at all.
        assert!(
            listing
                .entries
                .iter()
                .all(|entry| matches!(entry.kind, EntryKind::Directory))
        );
        expect![[r#"
            (
                216,
                [
                    Entry {
                        path: ".github",
                        kind: Directory,
                    },
                    Entry {
                        path: ".github/ISSUE_TEMPLATE",
                        kind: Directory,
                    },
                    Entry {
                        path: "api",
                        kind: Directory,
                    },
                ],
                Some(
                    "src/math/cmplx",
                ),
            )
        "#]]
        .assert_debug_eq(&(
            listing.entries.len(),
            &listing.entries[..3],
            &listing.truncated_after,
        ));
    }

    /// A reply listing `path` with `entries`, as recursive `entries` gives
    /// them.
    fn tree(path: &str, entries: &serde_json::Value) -> Envelope {
        let body = serde_json::json!({"data": {"repository": {
            "name": "github.com/x/a",
            "commit": {
                "oid": "a7962380ca08b5a188038c69871b8d3fbdf31e89",
                "node": {"__typename": "GitTree", "path": path,
                    "entries": entries}
            }
        }}});
        let reply = Reply {
            status: 200,
            body: body.to_string().into_bytes(),
            trace: None,
            location: None,
        };
        decode(reply).unwrap()
    }

    #[test]
    fn depth_counts_below_the_listed_path() {
        let request = ListRequest {
            address: Address::parse("github.com/x/a").unwrap(),
            path: RepoPath::parse("src/"),
            depth: 2,
        };
        let entry = |path: &str, is_directory| {
            serde_json::json!({"path": path, "isDirectory": is_directory,
                "submodule": null})
        };
        let entries = serde_json::json!([
            entry("src/a", true),
            entry("src/a/b", true),
            entry("src/a/b/c.go", false),
            entry("src/a/d.go", false),
        ]);
        let listing = listing(answer(&request, tree("src", &entries)));
        let paths: Vec<_> =
            listing.entries.iter().map(|entry| entry.path.as_str()).collect();
        assert_eq!(paths, ["src/a", "src/a/b", "src/a/d.go"]);
    }

    #[test]
    fn relative_submodule_urls_resolve_against_the_repository() {
        let request = ListRequest {
            address: Address::parse("github.com/x/a").unwrap(),
            path: RepoPath::parse(""),
            depth: 1,
        };
        let link = |url: &str| {
            serde_json::json!([{"path": "b", "isDirectory": false,
                "submodule": {"url": url,
                    "commit": "f3865b2a4d1a6c5bf0aa7ab0b4b4b2a1c1b1d1e1"}}])
        };
        let listing = listing(answer(&request, tree("", &link("../b.git"))));
        let EntryKind::Submodule(b) = &listing.entries[0].kind else {
            panic!("b is not a submodule");
        };
        assert_eq!(b.to_string(), "github.com/x/b@f3865b2a4d1a");
        let result = answer(&request, tree("", &link("/srv/git/b.git")));
        expect![[r#"
            [bad_response Unanswered]
            error: the upstream's answer has missing or unknown fields
            caused by: a submodule's URL isn't a repository: /srv/git/b.git
        "#]]
        .assert_eq(&failure(result));
    }

    #[test]
    fn missing_repository_and_commit() {
        let answer = |data: &str| {
            let reply = Reply {
                status: 200,
                body: data.as_bytes().to_vec(),
                trace: None,
                location: None,
            };
            answer(&case("root"), decode(reply).unwrap())
        };
        let repo = answer(r#"{"data":{"repository":null}}"#);
        let commit = answer(
            r#"{"data":{"repository":{
                "name":"github.com/gorilla/mux","commit":null}}}"#,
        );
        expect![[r#"
            [repo_not_found AnsweredNo]
            repository github.com/gorilla/mux not found
            hint: sego search 'type:repo mux'
            [revision_not_found AnsweredNo]
            revision v1.7.0 not found in github.com/gorilla/mux
            hint: sego list github.com/gorilla/mux
        "#]]
        .assert_eq(&(failure(repo) + &failure(commit)));
    }
}
