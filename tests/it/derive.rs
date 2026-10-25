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
    let got = counted(&results, &prepare(query), placeholder_ranges, None);
    (got.count, got.at_least, got.stopped)
}

/// An answer of `commits` commit results, `highlights` the server's
/// `matchCount`.
fn history(
    commits: u64,
    highlights: u64,
    limit_hit: bool,
    elapsed_ms: u64,
    query: &str,
) -> (u64, bool, bool) {
    let results: Results = serde_json::from_value(json!({
        "matchCount": highlights,
        "approximateResultCount": highlights.to_string(),
        "limitHit": limit_hit,
        "repositoriesCount": 1,
        "timedout": [],
        "cloning": [],
        "missing": [],
        "alert": null,
        "elapsedMilliseconds": elapsed_ms,
        "dynamicFilters": [],
        "results": [],
    }))
    .unwrap_or_else(|e| panic!("bad fixture: {e}"));
    let got = counted(&results, &prepare(query), 0, Some(commits));
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
        c(2, true, &[], 21_300, "repo:x foo timeout:20s count:30", 0),
        (2, true, true)
    );
}

#[test]
fn limit_hit_below_the_cap_in_time_is_not_a_cut() {
    let query = "repo:x select:file.directory foo count:30";
    assert_eq!(c(3, true, &[], 400, query, 0), (3, true, false));
    assert_eq!(c(0, true, &[], 400, query, 0), (0, false, false));
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
fn a_capped_answer_of_only_placeholders_is_at_least_even_at_zero() {
    let query = "repo:x binary count:30";
    assert_eq!(c(30, true, &[], 2000, query, 30), (0, true, false));
    assert_eq!(c(30, false, &[], 2000, query, 30), (0, false, false));
    assert_eq!(c(29, true, &[], 2000, query, 29), (0, false, false));
    assert_eq!(
        c(30, true, &[], 2000, "repo:x binary count:all", 30),
        (0, false, false)
    );
    assert_eq!(
        c(0, true, &[], 2000, "repo:x binary count:0", 0),
        (0, false, false)
    );
}

#[test]
fn a_mixed_answer_with_diffs_is_a_lower_bound() {
    assert_eq!(
        c(4, false, &[], 400, "(repo:x type:diff foo) or bar count:30", 0),
        (4, true, false)
    );
    assert_eq!(
        c(0, false, &[], 400, "(repo:x type:diff foo) or bar count:30", 0),
        (0, false, false)
    );
}

#[test]
fn a_complete_repo_answer_to_a_diff_search_is_exact() {
    assert_eq!(
        c(
            4,
            false,
            &[],
            400,
            r"repo:^github\.com/x/y$ type:diff select:repo foo count:30",
            0
        ),
        (4, false, false)
    );
}

#[test]
fn a_directory_count_is_a_lower_bound_unless_count_all() {
    let rows = [
        ("repo:x select:file.directory foo", 4, true),
        ("repo:x select:file.directory foo count:30", 30, true),
        ("repo:x select:file.directory foo count:100", 19, true),
        ("repo:x select:file.directory foo count:all", 4, false),
    ];
    for (query, count, at_least) in rows {
        assert_eq!(
            c(count, false, &[], 400, query, 0),
            (count, at_least, false),
            "{query}"
        );
    }
}

#[test]
fn a_repo_select_wider_than_one_repository_is_a_lower_bound() {
    let rows = [
        ("lang:python foo select:repo count:all timeout:60s", true),
        ("foo select:repo", true),
        ("-repo:x foo select:repo", true),
        ("foo or bar select:repo", true),
        ("foo select:repo.topics", true),
        ("repo:x foo select:repo", true),
        (r"repo:^github\.com/vuejs/ foo select:repo", true),
        (r"repo:^github\.com/gorilla/(mux|handlers)$ foo select:repo", true),
        (r"repo:^github\.com/gorilla/mux.*$ foo select:repo", true),
        (r"repo:^github\.com/gorilla/ repo:mux foo select:repo", true),
        (r"repo:has.file(path:go\.mod) foo select:repo", true),
        (
            r"(repo:^github\.com/gorilla/mux$ foo) or (repo:^github\.com/gorilla/handlers$ foo) select:repo",
            true,
        ),
        (r"(repo:^github\.com/gorilla/mux$ foo) or (bar) select:repo", true),
        (r"repo:^github\.com/gorilla/mux$ foo select:repo", false),
        (r"r:^github\.com/gorilla/mux$ foo select:repo", false),
        (r#"repo:"^github\\.com/gorilla/mux$" foo select:repo"#, false),
        (r"repo:^github\.com/gorilla/mux$@v1.8.0 foo select:repo", false),
        (r"repo:^github\.com/gorilla/mux$ foo select:repo.topics", false),
        (
            r"repo:^github\.com/gorilla/ repo:^github\.com/gorilla/mux$ foo select:repo",
            false,
        ),
        (
            r"(repo:^github\.com/gorilla/mux$ foo) or (repo:^github\.com/gorilla/mux$ bar) select:repo",
            false,
        ),
        ("foo -select:repo", false),
        ("foo select:file", false),
        (r"-repo:^github\.com/gorilla/mux$ foo select:repo", true),
        (r"repo:^github\.com/gorilla/mux$@v1:v2 foo select:repo", false),
        ("repo:^mux$ foo select:repo", false),
        ("repo:^github.com/gorilla/mux$ foo select:repo", false),
        (r"repo:^github\.com/gorilla/my\_mux$ foo select:repo", false),
        (
            r"repo:^github\.com/gorilla/mux$ repo:has.file(path:go\.mod) foo select:repo",
            false,
        ),
        (r"repo:^a/b$ repo:^c/d$ foo select:repo", false),
        (r"(repo:^a/b$ foo) or (repo:^c/d$ foo) select:repo", true),
        (r"foo or repo:^a/b$ bar select:repo", true),
        ("repo:^/$ foo select:repo", true),
        (r"repo:^github\.com/$ foo select:repo", true),
        ("repo:^./.$ foo select:repo", true),
        ("repo:^a//b$ foo select:repo", true),
        ("type:repo foo", false),
    ];
    for (query, at_least) in rows {
        assert_eq!(
            c(247, false, &[], 400, query, 0),
            (247, at_least, false),
            "{query}"
        );
    }
    assert_eq!(c(0, false, &[], 400, "foo select:repo", 0), (0, false, false));
    assert_eq!(
        c(0, false, &[], 400, r"repo:^a/b$ foo select:repo", 0),
        (0, false, false)
    );
}

#[test]
fn a_repo_select_with_more_branches_than_read_is_a_lower_bound() {
    let branch = r"(repo:^a/b$ x)";
    let mut query = [branch; 65].join(" or ");
    query.push_str(" select:repo");
    assert_eq!(c(2, false, &[], 400, &query, 0), (2, true, false));
    let mut query = [branch; 3].join(" or ");
    query.push_str(" select:repo");
    assert_eq!(c(2, false, &[], 400, &query, 0), (2, false, false));
}

#[test]
fn a_pinned_select_answer_is_a_lower_bound_unless_count_all() {
    let rows = [
        ("repo:x@v1 Router select:file", true),
        ("repo:x@v1 type:symbol /./ select:symbol.method count:50", true),
        ("repo:x@v1 Router select:file count:all", false),
        ("repo:x Router select:file count:30", false),
        ("repo:x -rev:v1 Router select:file", false),
        (r"repo:^github\.com/x/y$@v1 Router select:repo", false),
        (r"repo:^github\.com/x/y$ rev:v1 Router select:repo.topics", false),
        ("repo:x@HEAD Router select:file", false),
        ("repo:x rev:HEAD Router select:file", false),
        ("repo:x@main Router select:file", true),
    ];
    for (query, at_least) in rows {
        assert_eq!(
            c(2, false, &[], 400, query, 0),
            (2, at_least, false),
            "{query}"
        );
    }
}

#[test]
fn a_pinned_owners_answer_is_not_a_lower_bound() {
    let query = "repo:x@v1 NewRouter select:file.owners";
    assert_eq!(c(2, false, &[], 400, query, 0), (2, false, false));
    assert_eq!(c(0, false, &[], 400, query, 0), (0, false, false));
}

#[test]
fn a_pinned_zero_is_a_lower_bound_only_when_the_select_drops_results() {
    let rows = [
        ("repo:x@v1 type:symbol /./ select:symbol.method", true),
        ("repo:x@v1 type:symbol /./ select:symbol", true),
        ("repo:x@v1 Router select:content", true),
        ("repo:x@v1 Router select:file", false),
        ("repo:x@v1 Router select:file.directory", false),
        (r"repo:^github\.com/x/y$@v1 Router select:repo", false),
        ("repo:x@v1 Router select:content count:all", false),
    ];
    for (query, at_least) in rows {
        assert_eq!(
            c(0, false, &[], 400, query, 0),
            (0, at_least, false),
            "{query}"
        );
    }
}

#[test]
fn a_diff_answer_under_the_limit_is_exact_in_commits() {
    assert_eq!(
        history(8, 27, false, 400, "repo:x type:diff skipClean count:all"),
        (8, false, false)
    );
    assert_eq!(
        history(2, 5, false, 400, "repo:x type:commit fix count:30"),
        (2, false, false)
    );
}

#[test]
fn a_diff_answer_at_the_limit_is_at_least_in_commits() {
    assert_eq!(
        history(12, 30, true, 400, "repo:x type:diff Vars count:30"),
        (12, true, false)
    );
}

#[test]
fn a_diff_answer_cut_by_the_clock_is_at_least_in_commits() {
    assert_eq!(
        history(3, 9, false, 19_500, "repo:x type:diff count:all timeout:20s"),
        (3, true, true)
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
