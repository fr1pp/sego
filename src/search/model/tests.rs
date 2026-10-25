use std::fmt::Write as _;

use serde_json::{Value, json};

use super::{Chunk, Group, Searched};
use crate::output::json::write;
use crate::search::query::{Unit, prepare};
use crate::search::response::Results;

fn searched(items: &Value) -> Searched {
    searched_for("foo", items)
}

fn searched_for(query: &str, items: &Value) -> Searched {
    let Ok(results) = serde_json::from_value::<Results>(json!({
        "matchCount": 5,
        "approximateResultCount": "5",
        "limitHit": false,
        "repositoriesCount": 3,
        "timedout": [{"name": "t/x"}],
        "cloning": [],
        "missing": [],
        "alert": null,
        "elapsedMilliseconds": 12,
        "dynamicFilters": [],
        "results": items,
    })) else {
        panic!("results do not parse")
    };
    Searched::new(results, &prepare(query))
}

fn chunk(content: &str, start: u32, ranges: &Value) -> Value {
    json!({"content": content, "contentStart": {"line": start},
           "ranges": ranges})
}

fn range(sl: u32, sc: u32, el: u32, ec: u32) -> Value {
    json!({"start": {"line": sl, "character": sc},
           "end": {"line": el, "character": ec}})
}

fn file(path: &str, chunks: &[Value], symbols: &[Value]) -> Value {
    json!({
        "__typename": "FileMatch",
        "repository": {"name": "r/a"},
        "file": {"path": path, "commit": {"oid": "c1"}},
        "revSpec": {"expr": "main"},
        "chunkMatches": chunks,
        "symbols": symbols,
    })
}

fn symbol(name: &str, line: Option<u32>) -> Value {
    let location = line.map(|l| json!({"range": {"start": {"line": l}}}));
    json!({"name": name, "kind": "FUNCTION", "containerName": null,
           "location": location})
}

fn commit(body: &str, highlights: &Value) -> Value {
    json!({
        "__typename": "CommitSearchResult",
        "commit": {"oid": "c2", "subject": "fix", "repository": {"name": "r2"},
                   "author": {"date": "2024-05-06T07:08:09Z",
                              "person": {"displayName": "Ann"}}},
        "matches": [{"body": {"text": body}, "highlights": highlights}],
    })
}

fn only_chunks(searched: &Searched) -> &[Chunk] {
    match &searched.groups[0] {
        Group::File { chunks, .. } => chunks,
        _ => panic!("not a file"),
    }
}

fn json_of(searched: &Searched) -> String {
    let mut out = Vec::new();
    assert!(write(&searched.json(), &mut out).is_ok(), "write failed");
    String::from_utf8_lossy(&out).into_owned()
}

fn marked(chunk: &Chunk) -> Vec<(u32, bool)> {
    chunk.lines.iter().map(|l| (l.number, l.matched)).collect()
}

#[test]
fn a_chunk_marks_the_lines_its_ranges_touch() {
    let s = searched(&json!([file(
        "a.go",
        &[chunk("a\nb\nc", 41, &json!([range(42, 0, 42, 1)]))],
        &[]
    )]));
    let chunks = only_chunks(&s);
    assert_eq!(chunks.len(), 1);
    assert_eq!(marked(&chunks[0]), [(42, false), (43, true), (44, false)]);
    assert_eq!(chunks[0].units, 1);

    let s = searched(&json!([file(
        "a.go",
        &[chunk("a\nb\nc", 41, &json!([range(41, 0, 42, 1)]))],
        &[]
    )]));
    assert_eq!(
        marked(&only_chunks(&s)[0]),
        [(42, true), (43, true), (44, false)]
    );

    let s = searched(&json!([file(
        "a.go",
        &[chunk("a\nb\nc", 41, &json!([range(41, 0, 42, 0)]))],
        &[]
    )]));
    assert_eq!(
        marked(&only_chunks(&s)[0]),
        [(42, true), (43, false), (44, false)]
    );

    let s = searched(&json!([file(
        "a.go",
        &[chunk("a\nb\nc\nd\n", 41, &json!([range(44, 0, 44, 1)]))],
        &[]
    )]));
    assert_eq!(
        marked(&only_chunks(&s)[0]),
        [(42, false), (43, false), (44, false), (45, true)]
    );

    let s = searched(&json!([file(
        "a.go",
        &[chunk("a\nb\n", 41, &json!([range(43, 0, 43, 0)]))],
        &[]
    )]));
    let chunk = &only_chunks(&s)[0];
    assert_eq!(marked(chunk), [(42, false), (43, false)]);
    assert_eq!(chunk.units, 0);
}

#[test]
fn an_answer_counts_the_units_of_each_kind_of_group() {
    let s = searched(&json!([
        file(
            "a.go",
            &[chunk(
                "a\nb",
                0,
                &json!([range(0, 0, 0, 1), range(1, 0, 1, 1)])
            )],
            &[symbol("F", Some(0)), symbol("G", Some(1)), symbol("H", None)]
        ),
        file("b.go", &[], &[]),
        file("c.bin", &[chunk("\0", 0, &json!([]))], &[]),
        json!({"__typename": "Repository", "name": "r3"}),
        commit(
            "```\nx\ny\n```",
            &json!([
                {"line": 1, "character": 0, "length": 1},
                {"line": 2, "character": 0, "length": 1},
            ])
        ),
        commit("```\nx\n```", &json!([])),
        json!({"__typename": "Unknown"}),
    ]));
    assert_eq!(s.units(), 2 + 2 + 1 + 1 + 1 + 2 + 1);
}

#[test]
fn a_patternless_query_of_repository_results_counts_repos() {
    let repos = json!([
        {"__typename": "Repository", "name": "r/a"},
        {"__typename": "Repository", "name": "r/b"},
    ]);
    let query = r"repo:^github\.com/gorilla/mux$";
    assert_eq!(searched_for(query, &repos).unit, Unit::Repos);
    assert_eq!(searched_for("foo", &repos).unit, Unit::Matches);
    assert_eq!(searched_for("repo:x select:file", &repos).unit, Unit::Files);

    let files = json!([file("a.go", &[], &[])]);
    assert_eq!(searched_for(query, &files).unit, Unit::Files);

    let mixed = json!([repos[0], file("a.go", &[], &[])]);
    assert_eq!(searched_for(query, &mixed).unit, Unit::Files);

    assert_eq!(searched_for(query, &json!([])).unit, Unit::Files);

    let directories = json!([file("a/", &[], &[]), file("b/", &[], &[])]);
    let s = searched_for("repo:x select:file.directory", &directories);
    assert_eq!((s.unit, s.count), (Unit::Directories, 5));
    assert!(json_of(&s).contains(r#""unit":"directories","count":5,"#));
    let Ok(none) = serde_json::from_value::<Results>(json!({
        "matchCount": 0, "approximateResultCount": "0", "limitHit": false,
        "timedout": [], "alert": null, "elapsedMilliseconds": 12,
        "dynamicFilters": [], "results": [],
    })) else {
        panic!("results do not parse")
    };
    let s = Searched::new(none, &prepare("repo:x select:file.directory"));
    assert_eq!((s.unit, s.count), (Unit::Directories, 0));
    assert!(json_of(&s).contains(r#""unit":"directories","count":0,"#));
}

#[test]
fn an_answer_of_commit_results_counts_commits() {
    let commits = json!([
        commit(
            "```diff\nx\n+y\n```",
            &json!([{"line": 1, "character": 0, "length": 1}]),
        ),
        commit("```\nx\n```", &json!([])),
    ]);
    for query in [
        "repo:x type:diff y",
        "repo:x type:commit",
        "repo:x type:diff select:commit.diff.added y",
        "repo:x select:commit",
    ] {
        let s = searched_for(query, &commits);
        assert_eq!((s.unit, s.count), (Unit::Commits, 2), "{query}");
        assert_eq!(s.server.match_count, 5, "{query}");
        let out = json_of(&s);
        assert!(out.contains(r#""unit":"commits","count":2,"#), "{out}");
    }
    let s = searched_for("repo:x type:diff select:repo y", &commits);
    assert_eq!((s.unit, s.count), (Unit::Repos, 5));

    let s = searched_for("(repo:x type:diff y) or z", &commits);
    assert_eq!((s.unit, s.count), (Unit::Commits, 2));

    let mixed = json!([commits[0], file("a.go", &[], &[])]);
    let s = searched_for("(repo:x type:diff y) or z", &mixed);
    assert_eq!((s.unit, s.count), (Unit::Matches, 5));
    let s = searched_for("repo:x type:diff y", &mixed);
    assert_eq!((s.unit, s.count), (Unit::Commits, 1));

    let s = searched_for("repo:x type:diff y", &json!([]));
    assert_eq!((s.unit, s.count), (Unit::Commits, 0));
    assert!(json_of(&s).contains(r#""unit":"commits","count":0,"#));
}

/// A diff body of `hunks.len()` files, file `i` with `hunks[i]` hunks.
fn diff_body(hunks: &[usize]) -> String {
    let hunk = "@@ -1,2 +1,2 @@ x\n ctx\n-old\n+new\n";
    let mut body = "```diff\n".to_owned();
    for (i, &n) in hunks.iter().enumerate() {
        let _ = writeln!(body, "f{i}.go f{i}.go");
        body.push_str(&hunk.repeat(n));
    }
    body.push_str("```");
    body
}

fn limited(body: &str) -> bool {
    let s = searched_for("type:diff x", &json!([commit(body, &json!([]))]));
    match &s.groups[0] {
        Group::Commit { limited, .. } => *limited,
        _ => panic!("not a commit"),
    }
}

#[test]
fn a_diff_of_five_files_is_limited() {
    assert!(limited(&diff_body(&[1, 1, 1, 1, 1])));
}

#[test]
fn a_diff_of_four_files_of_two_hunks_is_not_limited() {
    assert!(!limited(&diff_body(&[2, 2, 1, 2])));
}

#[test]
fn files_are_told_from_hunk_lines_by_the_hunk_counts() {
    let hunk = "@@ -1,1 +1,1 @@\n-o\n+n\n";
    let body = format!("```diff\na a\n{hunk}+b +b\n{hunk}-c -c\n{hunk}```");
    assert!(!limited(&body));
    let body = format!(
        "```diff\na a\n{hunk}+b +b\n{hunk}-c -c\n{hunk}d d\n{hunk}+e +e\n{hunk}```"
    );
    assert!(limited(&body));
}

#[test]
fn a_hunk_counting_more_lines_than_it_shows_ends_at_the_next_header() {
    let short = "@@ -1,9 +1,9 @@\n ctx\n-o\n";
    let hunk = "@@ -1,1 +1,1 @@\n-o\n+n\n";
    assert!(limited(&format!("```diff\na a\n{short}{hunk}{hunk}```")));
    let files = format!("b b\n{hunk}c c\n{hunk}d d\n{hunk}e e\n{hunk}");
    assert!(limited(&format!("```diff\na a\n{short}{files}```")));
    assert!(!limited(&format!("```diff\na a\n{short}b b\n{hunk}```")));
}

#[test]
fn a_hunk_header_has_both_counts() {
    let lines = "@@ -1 +1 @@\n".repeat(3);
    assert!(!limited(&format!("```diff\na a\n{lines}```")));
}

#[test]
fn a_commit_message_is_not_limited() {
    let lines = "@@ a\n@@ b\n@@ c\n".repeat(2);
    assert!(!limited(&format!("```\na\n{lines}b\nc\nd\ne\n```")));
}
