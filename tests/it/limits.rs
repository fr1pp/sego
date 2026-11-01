//! sego's default limits. Their responses hold hundreds of entries, so these
//! tests build them in a loop, and check the count and the edges of the answer.

use std::fmt::Write as _;

use serde_json::{Value, json};

use crate::common::recording::ServerResponse;
use crate::common::{Ran, answered};

const WIDGET: &str = "example.com/acme/widget";
const BASE: &str = "0123456789abcdef0123456789abcdef01234567";
const HEAD: &str = "89abcdef0123456789abcdef0123456789abcdef";
const AUTHOR: &str = "Jonathan Davies";

/// Holds sego's text to its first line, its number of lines and its last line.
fn printed(outcome: &Ran, first: &str, lines: usize, last: &str) {
    let got: Vec<&str> = outcome.stdout.lines().collect();
    assert_eq!(got.first().copied(), Some(first));
    assert_eq!(got.len(), lines);
    assert_eq!(got.last().copied(), Some(last));
    assert_eq!(outcome.stderr, "");
}

fn json_answer(outcome: &Ran) -> Value {
    assert_eq!(outcome.stderr, "");
    serde_json::from_str(&outcome.stdout).unwrap_or_else(|e| panic!("{e}"))
}

fn commit_args(json: bool) -> Vec<&'static str> {
    let mut args = vec!["show", "example.com/acme/widget@v1"];
    if json {
        args.insert(1, "--json");
    }
    args
}

/// A commit's two responses: `files` files in the page of `total`.
fn commit(files: usize, total: usize, more: bool) -> [Value; 2] {
    let nodes: Vec<Value> = (0..files)
        .map(|n| {
            let path = format!("f{n:03}.go");
            json!({"oldPath": path, "newPath": path, "stat": {"added": 1, "deleted": 0}})
        })
        .collect();
    let added = if more || files > 200 { 13366 } else { total };
    [
        json!({"data": {"r0": {
            "name": WIDGET,
            "defaultBranch": {"displayName": "main"},
            "mirrorInfo": null,
            "commit": {
                "oid": "affe2b9b7c608f4e354d7e83d0583ed35e845650",
                "subject": "big",
                "message": "big",
                "author": {"date": "2026-09-09T20:14:26Z", "person": {"displayName": AUTHOR}},
                "committer": {"date": "2026-09-10T01:02:03Z", "person": {"displayName": "GitHub"}},
                "parents": [{"oid": "6b668aa526afdc623c1a9ed1d6ae920e04a717ad"}],
                "w": {"commit": {"diff": {
                    "all": {"diffStat": {"added": added, "deleted": 0}, "totalCount": total},
                    "page": {"nodes": nodes, "pageInfo": {"hasNextPage": more}},
                }}},
            },
        }}}),
        json!({"data": {"repository": {"commit": {"tags": {"nodes": []}}}}}),
    ]
}

#[test]
fn commit_files_at_the_default_limit() {
    let outcome = answered(&commit_args(false), commit(200, 200, false));
    let first = format!(
        "{WIDGET}@affe2b9b7c60  2026-09-09  {AUTHOR}  parents 6b668aa526af  200 files  +200 -0  \
         no tags  rev v1"
    );
    printed(&outcome, &first, 203, "M  +1 -0  f199.go");
}

#[test]
fn commit_files_past_the_default_limit() {
    let outcome = answered(&commit_args(false), commit(200, 13366, true));
    let first = format!(
        "{WIDGET}@affe2b9b7c60  2026-09-09  {AUTHOR}  parents 6b668aa526af  200 of 13366 files \
         shown  +13366 -0  no tags  rev v1"
    );
    printed(&outcome, &first, 203, "M  +1 -0  f199.go");
}

/// In JSON the commit carries every file the server has, past the 200 text prints.
#[test]
fn commit_files_past_the_default_limit_json() {
    let answer = json_answer(&answered(&commit_args(true), commit(250, 250, false)));
    assert_eq!(answer["files"].as_array().map(Vec::len), Some(250));
    assert_eq!(answer["files"][249]["new_path"], "f249.go");
    assert_eq!(answer["files_total"], 250);
    assert_eq!(answer["files_more"], false);
}

const RANGE: &str = "example.com/acme/widget@v1...v2";

/// A range's two responses: `files` files in the page of `total`.
fn range(files: usize, total: usize, more: bool) -> [Value; 2] {
    let nodes: Vec<Value> = (0..files)
        .map(|n| {
            let path = format!("f{n:03}.go");
            json!({
                "oldPath": path,
                "newPath": path,
                "oldFile": {"commit": {"oid": BASE}},
                "stat": {"added": 1, "deleted": 0},
            })
        })
        .collect();
    [
        json!({"data": {"r0": {
            "name": WIDGET,
            "defaultBranch": {"displayName": "main"},
            "mirrorInfo": null,
            "a": {"oid": BASE},
            "b": {"oid": HEAD},
        }}}),
        json!({"data": {
            "a": {"commit": {"oid": BASE}},
            "b": {"commit": {"oid": HEAD}},
            "c": {"comparison": {
                "range": {"mergeBase": null},
                "all": {"diffStat": {"added": total, "deleted": 0}, "totalCount": total},
                "page": {"nodes": nodes, "pageInfo": {"hasNextPage": more}},
            }},
        }}),
    ]
}

#[test]
fn diff_default_limit_cut() {
    let outcome = answered(&["diff", RANGE], range(200, 13366, true));
    let first = format!(
        "{WIDGET}@0123456789ab...89abcdef0123  200 of 13366 files shown  +13366 -0  base \
         0123456789ab  rev v1...v2"
    );
    printed(&outcome, &first, 201, "M  +1 -0  f199.go");
}

#[test]
fn diff_default_limit_cut_json() {
    let answer = json_answer(&answered(&["diff", "--json", RANGE], range(201, 201, false)));
    assert_eq!(answer["files"].as_array().map(Vec::len), Some(201));
    assert_eq!(answer["files"][200]["new_path"], "f200.go");
    assert_eq!(answer["files_total"], 201);
    assert_eq!(answer["files_more"], false);
}

#[test]
fn diff_exactly_the_default_limit() {
    let outcome = answered(&["diff", RANGE], range(200, 200, false));
    let first = format!(
        "{WIDGET}@0123456789ab...89abcdef0123  200 files  +200 -0  base 0123456789ab  rev v1...v2"
    );
    printed(&outcome, &first, 201, "M  +1 -0  f199.go");
}

const TREE: &str = "98cb6bf42e086f6af920b965c38cacc07402d51b";

fn tree_address() -> String {
    format!("{WIDGET}@{TREE}")
}

fn tree(entries: usize) -> [Value; 1] {
    let entries: Vec<Value> = (0..entries)
        .map(|n| json!({"path": format!("f{n:03}"), "isDirectory": false, "submodule": null}))
        .collect();
    [json!({"data": {"r0": {
        "name": WIDGET,
        "defaultBranch": {"displayName": "main"},
        "commit": {
            "oid": TREE,
            "meta": {"__typename": "GitTree", "path": "", "entries": entries},
        },
    }}})]
}

#[test]
fn ls_entries_at_the_default_limit() {
    let outcome = answered(&["ls", &tree_address()], tree(200));
    printed(
        &outcome,
        &format!("{WIDGET}@98cb6bf42e08  200 entries  200 files"),
        201,
        "file       f199",
    );
}

#[test]
fn ls_entries_past_the_default_limit_json() {
    let answer = json_answer(&answered(&["ls", "--json", &tree_address()], tree(201)));
    assert_eq!(answer["entries"].as_array().map(Vec::len), Some(201));
    assert_eq!(answer["entries"][200]["path"], "f200");
    assert_eq!(answer["entries_total"], 201);
    assert_eq!(answer["entries_more"], false);
}

#[test]
fn ls_entries_past_the_default_limit_recursive() {
    let outcome = answered(&["ls", "--recursive", &tree_address()], tree(201));
    let first = format!("{WIDGET}@98cb6bf42e08  200 of 201 entries shown  201 files");
    printed(&outcome, &first, 201, "file       f199");
}

fn ref_nodes(names: impl Iterator<Item = (String, String)>) -> Vec<Value> {
    names.map(|(name, oid)| json!({"displayName": name, "target": {"oid": oid}})).collect()
}

#[test]
fn refs_branches_and_tags_at_default_limit() {
    let branches = ref_nodes((0..100).map(|n| (format!("b{n}"), format!("{:040x}", n + 1))));
    let tags = ref_nodes((0..100).map(|n| (format!("t{n}"), format!("{:040x}", n + 1000))));
    let response = json!({"data": {"r0": {
        "name": WIDGET,
        "defaultBranch": {"displayName": "b0"},
        "mirrorInfo": null,
        "branches": {"nodes": branches, "totalCount": 100},
        "tags": {"nodes": tags, "totalCount": 100},
    }}});
    let outcome = answered(&["refs", WIDGET], [response]);
    let first = format!("{WIDGET}  100 branches  100 tags  default b0");
    printed(&outcome, &first, 201, "tag     000000000000  t99");
}

/// A response to refs filtered by a text.
fn matching(branches: (&[Value], usize, usize), tags: (&[Value], usize, usize)) -> Value {
    json!({"data": {"r0": {
        "name": WIDGET,
        "defaultBranch": {"displayName": "main"},
        "mirrorInfo": null,
        "branches": {"nodes": branches.0, "totalCount": branches.1},
        "tags": {"nodes": tags.0, "totalCount": tags.1},
        "all_branches": {"totalCount": branches.2},
        "all_tags": {"totalCount": tags.2},
    }}})
}

#[test]
fn refs_branches_matching_text_past_default_limit() {
    let branches = ref_nodes((0..100).map(|n| (format!("feature-{n}"), format!("{:040x}", n + 1))));
    let tags = ref_nodes(
        ["a", "b", "c"]
            .iter()
            .zip(500..)
            .map(|(name, oid)| (format!("feature-{name}"), format!("{oid:040x}"))),
    );
    let outcome =
        answered(&["refs", WIDGET, "feature"], [matching((&branches, 120, 5456), (&tags, 3, 40))]);
    let first = format!(
        "{WIDGET}  120 of 5456 branches match \"feature\"  100 shown  3 of 40 tags match \
         \"feature\"  default main"
    );
    printed(&outcome, &first, 104, "tag     000000000000  feature-c");
}

#[test]
fn refs_tags_matching_text_past_default_limit() {
    let tags = ref_nodes((1..=100).map(|n| (format!("rel-{n:03}"), format!("abc{n:09x}{n:028x}"))));
    let outcome = answered(&["refs", WIDGET, "rel"], [matching((&[], 0, 12), (&tags, 130, 400))]);
    let first = format!(
        "{WIDGET}  0 of 12 branches match \"rel\"  130 of 400 tags match \"rel\"  100 shown  \
         default main"
    );
    printed(&outcome, &first, 101, "tag     abc000000064  rel-100");
}

/// A search's response, its count both exact and approximate.
fn search(results: &[Value], count: usize, ms: u64, limit_hit: bool, timedout: &Value) -> Value {
    json!({"data": {"search": {"results": {
        "alert": null,
        "approximateResultCount": count.to_string(),
        "dynamicFilters": [],
        "elapsedMilliseconds": ms,
        "limitHit": limit_hit,
        "matchCount": count,
        "results": results,
        "timedout": timedout,
    }}}})
}

fn file_match(path: &str, repo: &str, path_matches: &Value, chunks: &Value) -> Value {
    json!({
        "__typename": "FileMatch",
        "pathMatches": path_matches,
        "chunkMatches": chunks,
        "file": {"commit": {"oid": BASE}, "path": path},
        "repository": {"name": repo},
        "revSpec": null,
        "symbols": [],
    })
}

/// Four commits of 38 highlights each are past the limit after the third.
#[test]
fn search_commit_answer_past_the_default_limit_prints_3_of_4_commits() {
    let added: Vec<String> = (1..=38).map(|n| format!("+line {n}")).collect();
    let text = format!("```diff\na.go a.go\n@@ -1,0 +1,38 @@\n{}\n```", added.join("\n"));
    let highlights: Vec<Value> =
        (3..=40).map(|line| json!({"line": line, "character": 0, "length": 1})).collect();
    let results: Vec<Value> = ('a'..='d')
        .zip(1..)
        .map(|(letter, n)| {
            json!({
                "__typename": "CommitSearchResult",
                "commit": {
                    "oid": letter.to_string().repeat(40),
                    "subject": format!("Add line {n}"),
                    "author": {"date": "2024-05-06T07:08:09Z", "person": {"displayName": "Ann"}},
                    "repository": {"name": WIDGET},
                },
                "matches": [{"body": {"text": text}, "highlights": highlights}],
            })
        })
        .collect();
    let query = "repo:example.com/acme/widget type:diff line count:4";
    let outcome = answered(&["search", query], [search(&results, 152, 700, true, &json!([]))]);
    printed(&outcome, "3 of 4+ commits  1 repo  0.7 s", 128, "    +line 38");
    assert!(outcome.stdout.contains("Add line 3\n"));
    assert!(!outcome.stdout.contains("Add line 4\n"));
}

#[test]
fn search_default_limit_cuts_a_stopped_answer_of_directories() {
    let results: Vec<Value> = (1..=101)
        .map(|n| file_match(&format!("src/dir{n:03}/"), WIDGET, &json!([]), &json!([])))
        .collect();
    let response = search(&results, 836, 21300, false, &json!([{"name": WIDGET}]));
    let query = "repo:example.com/acme/widget select:file.directory foo count:all timeout:20s";
    let outcome = answered(&["search", query], [response]);
    let last = format!("{WIDGET}@0123456789ab:src/dir100/");
    printed(&outcome, "100 of 836+ directories  1 repo  stopped at 21.3 s", 103, &last);
}

fn path_matches(files: usize) -> Vec<Value> {
    (1..=files)
        .map(|n| {
            let path = format!("src/file{n:03}.go");
            file_match(&path, WIDGET, &json!([{"start": {"line": 0}}]), &json!([]))
        })
        .collect()
}

const PATH_QUERY: &str = r"repo:example.com/acme/widget type:path file:\.go$ count:all";

#[test]
fn search_default_limit_cuts_an_answer_of_101_files_to_100() {
    let response = search(&path_matches(101), 101, 700, false, &json!([]));
    let outcome = answered(&["search", PATH_QUERY], [response]);
    let last = format!("{WIDGET}@0123456789ab:src/file100.go  path match");
    printed(&outcome, "100 of 101 files  1 repo  0.7 s", 103, &last);
}

#[test]
fn search_default_limit_keeps_an_answer_of_100_files_whole() {
    let response = search(&path_matches(100), 100, 700, false, &json!([]));
    let outcome = answered(&["search", PATH_QUERY], [response]);
    let last = format!("{WIDGET}@0123456789ab:src/file100.go  path match");
    printed(&outcome, "100 files  1 repo  0.7 s", 103, &last);
}

/// The placeholders count against the cap but not the count: 30 matches with 5
/// of them placeholders is 25 of a capped answer.
#[test]
fn search_placeholders_use_cap() {
    let range = |line: usize, end: usize| json!({"start": {"line": line, "character": 0}, "end": {"line": line, "character": end}});
    let chunk = |content: String, ranges: Vec<Value>| json!([{"content": content, "contentStart": {"line": 0}, "ranges": ranges}]);
    let lines: Vec<String> = (0..25).map(|n| format!("binary {n}")).collect();
    let mut results = vec![file_match(
        "main.go",
        "example.com/app",
        &json!([]),
        &chunk(lines.join("\n"), (0..25).map(|line| range(line, 6)).collect()),
    )];
    results.extend((0..5).map(|n| {
        let placeholder = "NOT-INDEXED: exceeds the maximum size limit".to_owned();
        let chunks = chunk(placeholder, vec![range(0, 43)]);
        file_match(&format!("big{n}.bin"), "example.com/app", &json!([]), &chunks)
    }));
    let response = search(&results, 30, 2000, true, &json!([]));
    let outcome = answered(&["search", "repo:example.com/app binary count:30"], [response]);
    printed(&outcome, "25+ matches  1 repo  2.0 s", 29, "25:binary 24");
}

const BLOB_OID: &str = "d81fd54f1a2b3c4d5e6f708192a3b4c5d6e7f809";

fn numbered_lines(lines: usize) -> String {
    (1..=lines).fold(String::new(), |mut text, n| {
        let _ = writeln!(text, "line {n}");
        text
    })
}

/// A file of exactly 500 lines needs no pointer to read the rest.
#[test]
fn show_default_window_covers_whole_file() {
    let content = numbered_lines(500);
    let response = json!({"data": {"r0": {
        "name": "example.com/acme/ledger",
        "defaultBranch": {"displayName": "main"},
        "commit": {
            "oid": BLOB_OID,
            "body": {"__typename": "GitBlob", "content": content},
            "meta": {
                "__typename": "GitBlob",
                "path": "a.go",
                "byteSize": content.len(),
                "totalLines": 500,
                "binary": false,
                "rawURL": format!("/r/example.com/acme/ledger@{BLOB_OID}/-/raw/a.go"),
                "lfs": null,
                "submodule": null,
            },
        },
    }}});
    let outcome = answered(&["show", "example.com/acme/ledger@main:a.go"], [response]);
    let first = "example.com/acme/ledger@d81fd54f1a2b:a.go  lines 1-500 of 500  rev main";
    printed(&outcome, first, 501, "500:line 500");
}

/// A raw read of more than 500 lines says how to read the file whole.
#[test]
fn show_raw_read_default_window_has_pointer() {
    let too_big = "file exceeded size limit: BytesRead=1572865 (may be lower than file size)";
    let response = json!({
        "data": {"r0": {
            "name": WIDGET,
            "defaultBranch": {"displayName": "main"},
            "commit": {
                "oid": BLOB_OID,
                "body": null,
                "meta": {
                    "__typename": "GitBlob",
                    "path": "big.c",
                    "byteSize": 2_000_000,
                    "totalLines": 501,
                    "binary": false,
                    "rawURL": format!("/r/{WIDGET}@{BLOB_OID}/-/raw/big.c"),
                    "lfs": null,
                    "submodule": null,
                },
            },
        }},
        "errors": [
            {"message": too_big, "path": ["r0", "commit", "body", "content"]},
            {"message": too_big, "path": ["r0", "commit", "meta", "lfs"]},
        ],
    });
    let raw = ServerResponse::new(200, "text/plain; charset=utf-8", numbered_lines(501));
    let outcome = answered(&["show", "example.com/acme/widget@main:big.c"], [response.into(), raw]);
    let first = format!("{WIDGET}@d81fd54f1a2b:big.c  lines 1-500 of 501 (1- for all)  rev main");
    printed(&outcome, &first, 501, "500:line 500");
}
