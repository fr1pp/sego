use serde_json::{Value, json};

use sego::internal::search::derive::{counted, is_placeholder};
use sego::internal::search::query::prepare;
use sego::internal::search::response::{Item, Results};

fn named(list: &[&str]) -> Value {
    list.iter().map(|name| json!({"name": name})).collect()
}

fn c(
    match_count: u64,
    limit_hit: bool,
    timedout: &[&str],
    elapsed_ms: u64,
    query: &str,
    placeholder_ranges: u64,
) -> (u64, bool, bool) {
    let results: Results = serde_json::from_value(json!({
        "matchCount": match_count,
        "approximateResultCount": match_count.to_string(),
        "limitHit": limit_hit,
        "repositoriesCount": 0,
        "timedout": named(timedout),
        "cloning": [],
        "missing": [],
        "alert": null,
        "elapsedMilliseconds": elapsed_ms,
        "dynamicFilters": [],
        "results": [],
    }))
    .unwrap_or_else(|e| panic!("bad fixture: {e}"));
    let got = counted(&results, &prepare(query), placeholder_ranges);
    (got.count, got.at_least, got.stopped)
}

#[test]
fn complete_answer() {
    assert_eq!(
        c(3, false, &[], 1200, "repo:x foo count:30", 0),
        (3, false, false)
    );
}

#[test]
fn cap_reached() {
    assert_eq!(
        c(30, true, &[], 2000, "repo:x foo count:30", 0),
        (30, true, false)
    );
}

#[test]
fn cut_below_the_cap() {
    assert_eq!(
        c(2, true, &[], 21_300, "repo:x foo count:30", 0),
        (2, true, true)
    );
}

#[test]
fn timedout_is_stopped() {
    assert_eq!(
        c(5, false, &["r"], 20_004, "repo:x type:commit foo count:30", 0),
        (5, true, true)
    );
}

#[test]
fn clean_answer_past_its_deadline_is_stopped() {
    assert_eq!(c(1, false, &[], 59_500, "foo count:1000", 0), (1, true, true));
}

#[test]
fn expired_timeout_zero_is_stopped() {
    assert_eq!(
        c(0, false, &[], 15, "repo:x func timeout:1ms", 0),
        (0, false, true)
    );
}

#[test]
fn count_all_cut_by_the_clock() {
    assert_eq!(
        c(3472, true, &[], 62_190, "foo count:all", 0),
        (3472, true, true)
    );
}

#[test]
fn placeholders_use_the_cap_but_not_the_count() {
    assert_eq!(
        c(30, true, &[], 2000, "repo:x binary count:30", 5),
        (25, true, false)
    );
}

#[test]
fn a_diff_count_is_a_lower_bound() {
    assert_eq!(
        c(4, false, &[], 400, "repo:x type:diff Vars count:30", 0),
        (4, true, false)
    );
    assert_eq!(
        c(0, false, &[], 400, "repo:x type:diff Vars count:30", 0),
        (0, false, false)
    );
}

#[test]
fn placeholders_cannot_push_the_count_below_zero() {
    assert_eq!(c(2, false, &[], 100, "foo count:30", 9), (0, false, false));
}

fn file(chunks: &Value, symbols: &Value) -> Value {
    json!({
        "__typename": "FileMatch",
        "repository": {"name": "r/a"},
        "file": {"path": "big.bin", "commit": {"oid": "c1"}},
        "revSpec": null,
        "chunkMatches": chunks,
        "symbols": symbols,
    })
}

fn chunk(content: &str, line: u32) -> Value {
    json!({"content": content, "contentStart": {"line": line},
           "ranges": []})
}

fn is(value: &Value) -> bool {
    let item: Item = serde_json::from_value(value.clone())
        .unwrap_or_else(|e| panic!("bad fixture: {e}"));
    match item {
        Item::FileMatch(file) => is_placeholder(&file),
        _ => panic!("not a file match"),
    }
}

#[test]
fn placeholder_shape() {
    let text = "NOT-INDEXED: exceeds the maximum size limit";
    assert!(is(&file(&json!([chunk(text, 0)]), &json!([]))));
    assert!(!is(&file(&json!([chunk(text, 3)]), &json!([]))));
    assert!(!is(&file(&json!([chunk("not-indexed: x", 0)]), &json!([]))));
    assert!(!is(&file(&json!([]), &json!([]))));
    assert!(!is(&file(&json!([chunk(text, 0), chunk(text, 0)]), &json!([]))));
    let symbol = json!({"name": "S", "kind": "FUNCTION",
        "containerName": null, "location": null});
    assert!(!is(&file(&json!([chunk(text, 0)]), &json!([symbol]))));
}
