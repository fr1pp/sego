use serde_json::{Value, json};

use sego::internal::output::json::write;
use sego::internal::output::vocab::{filter_kind, symbol_kind};
use sego::internal::search::model::{Chunk, Group, Searched};
use sego::internal::search::query::prepare;
use sego::internal::search::response::Results;

fn searched(items: &Value) -> Searched {
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
    Searched::new(results, &prepare("foo"))
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
fn vocabularies() {
    let kinds = [
        "UNKNOWN",
        "FILE",
        "MODULE",
        "NAMESPACE",
        "PACKAGE",
        "CLASS",
        "METHOD",
        "PROPERTY",
        "FIELD",
        "CONSTRUCTOR",
        "ENUM",
        "INTERFACE",
        "FUNCTION",
        "VARIABLE",
        "CONSTANT",
        "STRING",
        "NUMBER",
        "BOOLEAN",
        "ARRAY",
        "OBJECT",
        "KEY",
        "NULL",
        "STRUCT",
        "EVENT",
        "OPERATOR",
    ];
    for kind in kinds {
        assert_eq!(symbol_kind(kind), kind.to_lowercase());
    }
    assert_eq!(symbol_kind("ENUMMEMBER"), "enum_member");
    assert_eq!(symbol_kind("TYPEPARAMETER"), "type_parameter");
    assert_eq!(symbol_kind("NEWKIND"), "newkind");

    for kind in ["repo", "lang", "file", "type", "utility", "author"] {
        assert_eq!(filter_kind(kind), kind);
    }
    assert_eq!(filter_kind("commit date"), "commit_date");
    assert_eq!(filter_kind("symbol type"), "symbol_type");
    assert_eq!(filter_kind("New Kind"), "new kind");
}

#[test]
fn chunk_lines_and_markers() {
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
fn ranges_count_code_points() {
    let s = searched(&json!([file(
        "a.go",
        &[chunk("🚀 foo", 0, &json!([range(0, 2, 0, 5)]))],
        &[]
    )]));
    let line = &only_chunks(&s)[0].lines[0];
    assert_eq!(line.ranges, [[2, 5]]);
    let s = searched(&json!([file(
        "a.go",
        &[chunk("🚀 foo", 0, &json!([range(0, 2, 0, 9)]))],
        &[]
    )]));
    assert_eq!(only_chunks(&s)[0].lines[0].ranges, [[2, 5]]);
    assert!(json_of(&s).contains(r#""text":"🚀 foo""#));
}

#[test]
fn range_offsets_on_multi_line_ranges() {
    let s = searched(&json!([file(
        "a.go",
        &[chunk("abcd\nefgh\nij", 0, &json!([range(0, 1, 2, 9)]))],
        &[]
    )]));
    let lines = &only_chunks(&s)[0].lines;
    assert_eq!(lines[0].ranges, [[1, 4]]);
    assert_eq!(lines[1].ranges, [[0, 4]]);
    assert_eq!(lines[2].ranges, [[0, 2]]);
}

#[test]
fn crlf_lines_drop_the_cr() {
    let s = searched(&json!([
        file("a.go", &[chunk("a\r\nb\r\n", 0, &json!([]))], &[]),
        commit("```\r\nx\r\n+y\r\n```", &json!([])),
    ]));
    let texts: Vec<_> =
        only_chunks(&s)[0].lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(texts, ["a", "b"]);
    let Group::Commit { lines, .. } = &s.groups[1] else {
        panic!("not a commit")
    };
    assert_eq!(lines, &["x", "+y"]);
}

#[test]
fn one_path_two_items() {
    let s = searched(&json!([
        file("a.go", &[], &[symbol("F", Some(9))]),
        file("b.go", &[], &[]),
        file(
            "a.go",
            &[chunk("x", 2, &json!([range(2, 0, 2, 1)]))],
            &[symbol("E", Some(1))]
        ),
    ]));
    assert_eq!(s.groups.len(), 2);
    let Group::File { path, chunks, symbols, .. } = &s.groups[0] else {
        panic!("not a file")
    };
    assert_eq!(path, "a.go");
    assert_eq!(chunks.len(), 1);
    let names: Vec<_> = symbols.iter().map(|s| (s.line, &*s.name)).collect();
    assert_eq!(names, [(Some(2), "E"), (Some(10), "F")]);
    assert!(
        matches!(&s.groups[1], Group::File { path, .. } if path == "b.go")
    );
}

#[test]
fn overlapping_chunks_merge() {
    let s = searched(&json!([file(
        "a.go",
        &[
            chunk("c\nd", 2, &json!([range(3, 0, 3, 1)])),
            chunk("a\nb\nc", 0, &json!([range(1, 0, 1, 1)])),
            chunk("x", 9, &json!([range(9, 0, 9, 1)])),
        ],
        &[]
    )]));
    let chunks = only_chunks(&s);
    assert_eq!(chunks.len(), 2);
    let texts: Vec<_> =
        chunks[0].lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(texts, ["a", "b", "c", "d"]);
    assert_eq!(
        marked(&chunks[0]),
        [(1, false), (2, true), (3, false), (4, true)]
    );
    assert_eq!(chunks[0].units, 2);
    assert_eq!(chunks[1].lines[0].number, 10);
}

#[test]
fn binary_chunk() {
    let s = searched(&json!([file(
        "a.bin",
        &[chunk("a\0b", 0, &json!([range(0, 0, 0, 1)]))],
        &[]
    )]));
    let Group::File { binary, chunks, .. } = &s.groups[0] else {
        panic!("not a file")
    };
    assert!(binary);
    assert!(chunks.is_empty());
    let out = json_of(&s);
    assert!(out.contains(r#""kind":"binary""#));
    assert!(!out.contains(r#""kind":"line""#));
}

#[test]
fn commit_body() {
    let body =
        "```diff\nold_test.go old_test.go\n@@ -263,0 +263,3 @@ x\n+a\n\n```";
    let s = searched(&json!([commit(
        body,
        &json!([
            {"line": 3, "character": 1, "length": 2},
            {"line": 4, "character": 1, "length": 2},
            {"line": 0, "character": 1, "length": 2},
            {"line": 40, "character": 1, "length": 2},
        ])
    )]));
    let Group::Commit { lines, highlights, date, author, .. } = &s.groups[0]
    else {
        panic!("not a commit")
    };
    assert_eq!(
        lines,
        &["old_test.go old_test.go", "@@ -263,0 +263,3 @@ x", "+a"]
    );
    assert_eq!(highlights, &[[2, 1, 3]]);
    assert_eq!(date, "2024-05-06");
    assert_eq!(author, "Ann");
}

#[test]
fn commit_body_crlf_blank_line() {
    let s = searched(&json!([commit(
        "```diff\r\nx\r\n+y\r\n\r\n```",
        &json!([{"line": 3, "character": 0, "length": 1}])
    )]));
    let Group::Commit { lines, highlights, .. } = &s.groups[0] else {
        panic!("not a commit")
    };
    assert_eq!(lines, &["x", "+y"]);
    assert!(highlights.is_empty());
}

#[test]
fn huge_range_end_is_clamped_to_the_chunk() {
    let s = searched(&json!([file(
        "a.go",
        &[chunk("ab\ncd", 3, &json!([range(3, 1, 4_000_000_000_u32, 7)]))],
        &[]
    )]));
    let lines = &only_chunks(&s)[0].lines;
    assert_eq!(lines[0].ranges, [[1, 2]]);
    assert_eq!(lines[1].ranges, [[0, 2]]);
}

#[test]
fn json_merges_lines_and_symbols_by_line() {
    let s = searched(&json!([file(
        "a.go",
        &[chunk("a\nb\nc", 0, &json!([])), chunk("z", 20, &json!([]))],
        &[symbol("N", None), symbol("S", Some(8)), symbol("T", Some(1))]
    )]));
    let out = json_of(&s);
    let Ok(doc) = serde_json::from_str::<Value>(&out) else {
        panic!("not json")
    };
    let order: Vec<String> = doc["results"]
        .as_array()
        .map(|rs| {
            rs.iter()
                .map(|r| {
                    let what = if r["kind"] == "line" {
                        r["line"].to_string()
                    } else {
                        r["name"].to_string()
                    };
                    format!("{}:{what}", r["kind"])
                })
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        order,
        [
            r#""line":1"#,
            r#""line":2"#,
            r#""symbol":"T""#,
            r#""line":3"#,
            r#""symbol":"S""#,
            r#""line":21"#,
            r#""symbol":"N""#,
        ]
    );
}

#[test]
fn json_order_and_kinds() {
    let mut no_rev = file("b.go", &[], &[]);
    no_rev["revSpec"] = Value::Null;
    let results: Results = serde_json::from_value(json!({
        "matchCount": 5,
        "approximateResultCount": "5",
        "limitHit": false,
        "repositoriesCount": 3,
        "timedout": [{"name": "t/x"}],
        "cloning": [],
        "missing": [],
        "alert": {"title": "T", "description": null, "kind": "k",
                  "proposedQueries": [{"description": null, "query": "q"}]},
        "elapsedMilliseconds": 12,
        "dynamicFilters": [{"value": "go", "label": "Go", "count": 3,
                            "limitHit": false, "kind": "lang"}],
        "results": [
            file("a.go", &[chunk("x\nfoo", 4, &json!([range(5, 0, 5, 3)]))],
                 &[symbol("Foo", Some(4))]),
            no_rev,
            file("bin", &[chunk("a\0b", 0, &json!([]))], &[]),
            commit("```diff\nx\n+y\n```",
                   &json!([{"line": 2, "character": 1, "length": 2}])),
            {"__typename": "Repository", "name": "r3"},
            {"__typename": "Unknown"},
        ],
    }))
    .unwrap();
    let s = Searched::new(results, &prepare("foo"));
    let expected = concat!(
        r#"{"query":"foo count:100","added":["count:100"],"#,
        r#""pattern_type":"keyword","match_count":5,"#,
        r#""approximate_count":"5","limit_hit":false,"repo_count":3,"#,
        r#""elapsed_ms":12,"deadline_ms":60000,"#,
        r#""alert":{"title":"T","description":null,"kind":"k","#,
        r#""proposed":[{"description":null,"query":"q"}]},"#,
        r#""timedout":["t/x"],"cloning":[],"missing":[],"#,
        r#""filters":[{"kind":"lang","value":"go","label":"Go","#,
        r#""count":3,"limit_hit":false}],"results":["#,
        r#"{"kind":"line","repo":"r/a","commit":"c1","rev":"main","#,
        r#""path":"a.go","line":5,"text":"x","match":false,"ranges":[],"#,
        r#""chunk":1},"#,
        r#"{"kind":"symbol","repo":"r/a","commit":"c1","rev":"main","#,
        r#""path":"a.go","line":5,"name":"Foo","symbol_kind":"function","#,
        r#""container":null},"#,
        r#"{"kind":"line","repo":"r/a","commit":"c1","rev":"main","#,
        r#""path":"a.go","line":6,"text":"foo","match":true,"#,
        r#""ranges":[[0,3]],"chunk":1},"#,
        r#"{"kind":"path","repo":"r/a","commit":"c1","rev":null,"#,
        r#""path":"b.go"},"#,
        r#"{"kind":"binary","repo":"r/a","commit":"c1","rev":"main","#,
        r#""path":"bin"},"#,
        r#"{"kind":"commit","repo":"r2","commit":"c2","#,
        r#""date":"2024-05-06","author":"Ann","subject":"fix","#,
        r#""lines":["x","+y"],"highlights":[[1,1,3]]},"#,
        r#"{"kind":"repo","repo":"r3"}]}"#,
        "\n"
    );
    assert_eq!(json_of(&s), expected);
}

#[test]
fn units() {
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
