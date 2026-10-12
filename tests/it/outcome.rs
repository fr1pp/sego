use serde_json::{Value, json};

use sego::internal::error::report;
use sego::internal::output::Notes;
use sego::internal::search::model::Searched;
use sego::internal::search::outcome::{binary_size, judge, unindexed_note};
use sego::internal::search::query::prepare;
use sego::internal::search::response::{Results, UnindexedFile};

fn results(overrides: &Value) -> Results {
    let mut base = json!({
        "matchCount": 0,
        "approximateResultCount": "0",
        "limitHit": false,
        "repositoriesCount": 0,
        "timedout": [],
        "cloning": [],
        "missing": [],
        "alert": null,
        "elapsedMilliseconds": 100,
        "dynamicFilters": [],
        "results": [],
    });
    if let (Some(map), Some(target)) =
        (overrides.as_object(), base.as_object_mut())
    {
        target.extend(map.clone());
    }
    match serde_json::from_value(base) {
        Ok(results) => results,
        Err(e) => panic!("bad fixture: {e}"),
    }
}

fn named(list: &[&str]) -> Value {
    list.iter().map(|name| json!({"name": name})).collect()
}

fn repo(name: &str) -> Value {
    json!({"__typename": "Repository", "name": name})
}

fn alert(title: &str, description: impl Into<Value>) -> Value {
    json!({"title": title, "description": description.into(),
        "kind": null, "proposedQueries": null})
}

fn run(results: Results, deadline_ms: u64) -> (String, String) {
    let mut prepared = prepare("x");
    prepared.deadline_ms = deadline_ms;
    let searched = Searched::new(results, &prepared);
    let mut notes = Notes::new();
    let outcome = judge(&searched, &mut notes);
    let mut printed = Vec::new();
    let mut error = Vec::new();
    assert!(notes.write(&mut printed).is_ok());
    if let Err(e) = outcome {
        assert!(report(&e, &mut error).is_ok());
    }
    (
        String::from_utf8_lossy(&printed).into_owned(),
        String::from_utf8_lossy(&error).into_owned(),
    )
}

fn revisions(count: usize) -> Value {
    let lines: Vec<String> =
        (1..=count).map(|i| format!("* r{i}@v{i}")).collect();
    json!(format!(
        "These repository revisions do not exist:\n{}",
        lines.join("\n")
    ))
}

const REVISIONS: &str = "Some revisions could not be searched";

fn failure(results: Results, deadline_ms: u64) -> String {
    let (notes, error) = run(results, deadline_ms);
    assert_eq!(notes, "");
    error
}

#[test]
fn unable_to_process_uses_description() {
    let r = results(&json!({"alert":
        alert("Unable To Process Query", json!("bad thing"))}));
    assert_eq!(failure(r, 60000), "sego: error: bad thing\n");
}

#[test]
fn unable_to_process_falls_back_to_title() {
    for description in [json!(null), json!(""), json!("  \n ")] {
        let r = results(&json!({"alert":
            alert("Unable To Process Query", description)}));
        assert_eq!(
            failure(r, 60000),
            "sego: error: Unable To Process Query\n"
        );
    }
}

#[test]
fn unable_to_process_wins_even_with_results() {
    let r = results(&json!({
        "results": [repo("a")],
        "alert": alert("Unable To Process Query", json!("bad")),
    }));
    assert_eq!(failure(r, 60000), "sego: error: bad\n");
}

#[test]
fn one_missing_revision() {
    let r = results(&json!({"alert": alert(REVISIONS, revisions(1))}));
    assert_eq!(
        failure(r, 60000),
        "sego: error: revision v1 not found in r1\n"
    );
}

#[test]
fn three_missing_revisions() {
    let r = results(&json!({"alert": alert(REVISIONS, revisions(3))}));
    assert_eq!(
        failure(r, 60000),
        "sego: error: revision v1 not found in r1\n  \
         revision v2 not found in r2\n  \
         revision v3 not found in r3\n"
    );
}

#[test]
fn five_missing_revisions() {
    let r = results(&json!({"alert": alert(REVISIONS, revisions(5))}));
    assert_eq!(
        failure(r, 60000),
        "sego: error: revision v1 not found in r1\n  \
         revision v2 not found in r2\n  \
         and 3 more revisions not found\n"
    );
}

#[test]
fn missing_revisions_without_lines_print_description() {
    let r = results(&json!({"alert": alert(REVISIONS, json!("odd text"))}));
    assert_eq!(failure(r, 60000), "sego: error: odd text\n");
}

#[test]
fn revision_splits_at_first_at() {
    let r = results(&json!({"alert": alert(
        REVISIONS, json!("x:\n  * a/b@v@2"))}));
    assert_eq!(
        failure(r, 60000),
        "sego: error: revision v@2 not found in a/b\n"
    );
}

#[test]
fn revisions_with_results_are_a_note() {
    let r = results(&json!({
        "results": [repo("a")],
        "alert": alert(REVISIONS, json!("Some of them.")),
    }));
    let (notes, error) = run(r, 60000);
    assert_eq!(error, "");
    assert_eq!(
        notes,
        "sego: note: Some revisions could not be searched: Some of them.\n"
    );
}

#[test]
fn no_repositories_without_proposals() {
    let r = results(&json!({"alert": alert(
        "No repositories found", json!("Try using a different filter"))}));
    assert_eq!(failure(r, 60000), "sego: error: No repositories found\n");
}

#[test]
fn no_repositories_with_proposals() {
    let mut a = alert("No repositories found", json!("advice"));
    a["proposedQueries"] = json!([
        {"description": "d1", "query": "q1"},
        {"description": null, "query": "q2"},
        {"description": null, "query": "q3"},
    ]);
    let r = results(&json!({"alert": a}));
    assert_eq!(
        failure(r, 60000),
        "sego: error: No repositories found\n  \
         the server proposes: q1\n  \
         the server proposes: q2\n"
    );
}

#[test]
fn no_repositories_with_results_is_a_note() {
    let mut a = alert("No repositories found", json!("advice"));
    a["proposedQueries"] = json!([{"description": null, "query": "q1"}]);
    let r = results(&json!({"results": [repo("a")], "alert": a}));
    let (notes, error) = run(r, 60000);
    assert_eq!(error, "");
    assert_eq!(
        notes,
        "sego: note: No repositories found: advice\n\
         sego: note: the server proposes: q1\n"
    );
}

#[test]
fn a_stopped_zero_is_an_error_without_facts() {
    for overrides in [
        json!({"limitHit": true, "elapsedMilliseconds": 21_300}),
        json!({"timedout": named(&["a", "b"]), "elapsedMilliseconds": 21_300}),
        json!({"elapsedMilliseconds": 21_300}),
    ] {
        let r = results(&overrides);
        assert_eq!(
            failure(r, 20_000),
            "sego: error: the search stopped at 21.3 s with no results\n"
        );
    }
}

#[test]
fn a_zero_stopped_past_a_zero_deadline() {
    let r = results(&json!({"elapsedMilliseconds": 0}));
    assert_eq!(
        failure(r, 0),
        "sego: error: the search stopped at 0.0 s with no results\n"
    );
}

#[test]
fn a_zero_in_time_exits_zero_with_no_note() {
    let r = results(&json!({"elapsedMilliseconds": 700}));
    assert_eq!(run(r, 60000), (String::new(), String::new()));
}

#[test]
fn a_real_result_left_after_the_placeholders_is_not_a_zero() {
    let file = |path: &str, chunks: Value| {
        json!({
            "__typename": "FileMatch",
            "repository": {"name": "r/a"},
            "file": {"path": path, "commit": {"oid": "c1"}},
            "revSpec": null,
            "chunkMatches": chunks,
            "symbols": [],
        })
    };
    let placeholder = json!([{
        "content": "NOT-INDEXED: exceeds the maximum size limit",
        "contentStart": {"line": 0},
        "ranges": [{"start": {"line": 0, "character": 0},
                    "end": {"line": 0, "character": 3}}],
    }]);
    let r = results(&json!({
        "matchCount": 1,
        "results": [file("big", placeholder), file("a.go", json!([]))],
        "timedout": named(&["t"]),
        "elapsedMilliseconds": 21_300,
    }));
    assert_eq!(run(r, 20_000), (String::new(), String::new()));
}

#[test]
fn a_stopped_answer_with_results_is_not_an_error() {
    let r = results(&json!({
        "matchCount": 1,
        "results": [repo("a")],
        "timedout": named(&["t"]),
        "elapsedMilliseconds": 21_300,
    }));
    assert_eq!(run(r, 20_000), (String::new(), String::new()));
}

#[test]
fn no_owners_alert_with_zero_results_exits_zero() {
    let r = results(&json!({
        "approximateResultCount": "0+",
        "alert": alert("Some results have no owners", json!("See docs.")),
    }));
    let (notes, error) = run(r, 60000);
    assert_eq!(error, "");
    assert_eq!(notes, "sego: note: Some results have no owners: See docs.\n");
}

#[test]
fn alert_without_description_is_the_title() {
    let r = results(&json!({
        "results": [repo("a")],
        "alert": alert("Heads up", json!(null)),
    }));
    assert_eq!(run(r, 60000).0, "sego: note: Heads up\n");
}

#[test]
fn the_server_lists_are_not_notes() {
    let r = results(&json!({
        "matchCount": 1,
        "results": [repo("a")],
        "timedout": named(&["t"]),
        "cloning": named(&["c1", "c2"]),
        "missing": named(&["m"]),
        "elapsedMilliseconds": 60_000,
    }));
    assert_eq!(run(r, 60000), (String::new(), String::new()));
}

#[test]
fn note_order_is_alert_proposals_kinds() {
    let mut a = alert("Heads up", json!("text"));
    a["proposedQueries"] = json!([{"description": null, "query": "q"}]);
    let r = results(&json!({
        "results": [
            {"__typename": "NewKind"},
            repo("a"),
            {"__typename": "Other"},
            {"__typename": "NewKind"},
        ],
        "alert": a,
    }));
    assert_eq!(
        run(r, 60000).0,
        "sego: note: Heads up: text\n\
         sego: note: the server proposes: q\n\
         sego: note: 2 results of kind NewKind not shown\n\
         sego: note: 1 result of kind Other not shown\n"
    );
}

const MIB: u64 = 1_048_576;
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

fn file(repo: &str, path: &str, bytes: u64) -> UnindexedFile {
    UnindexedFile {
        repo: repo.into(),
        commit: COMMIT.into(),
        path: path.into(),
        bytes,
    }
}

fn answer(
    overrides: &Value,
    unindexed: Option<Vec<UnindexedFile>>,
) -> Searched {
    let mut searched = Searched::new(results(overrides), &prepare("x"));
    searched.unindexed = unindexed;
    searched
}

fn zero(unindexed: Vec<UnindexedFile>) -> Searched {
    answer(&json!({}), Some(unindexed))
}

fn lines(rest: &[&str]) -> Vec<String> {
    rest.iter().map(|line| (*line).to_string()).collect()
}

#[test]
fn binary_sizes() {
    assert_eq!(binary_size(4_273_688), "4.1 MiB");
    assert_eq!(binary_size(263_448), "257.3 KiB");
    assert_eq!(binary_size(12), "12 B");
    assert_eq!(binary_size(1023), "1023 B");
    assert_eq!(binary_size(1024), "1.0 KiB");
    assert_eq!(binary_size(MIB), "1.0 MiB");
}

#[test]
fn one_unindexed_file() {
    let searched = zero(vec![file("r", "c_src/sqlite3.c", 9_542_041)]);
    assert_eq!(
        unindexed_note(&searched),
        Some((
            "the contents of 1 file in scope aren't in the search index \
             (1 over 1 MiB):"
                .to_string(),
            lines(&["  c_src/sqlite3.c  9.1 MiB"])
        ))
    );
}

#[test]
fn three_unindexed_files_have_no_tail() {
    let searched = zero(vec![
        file("r", "small.txt", 5000),
        file("r", "big.txt", 2 * MIB),
        file("r", "bigger.txt", 3 * MIB),
    ]);
    assert_eq!(
        unindexed_note(&searched),
        Some((
            "the contents of 3 files in scope aren't in the search index \
             (2 over 1 MiB, 1 too many distinct trigrams):"
                .to_string(),
            lines(&[
                "  bigger.txt  3.0 MiB",
                "  big.txt  2.0 MiB",
                "  small.txt  4.9 KiB",
            ])
        ))
    );
}

#[test]
fn four_unindexed_files_order_by_reason_then_size() {
    let searched = zero(vec![
        file("r", "t1", 100),
        file("r", "o1", 2 * MIB),
        file("r", "t2", 900),
        file("r", "o2", 5 * MIB),
    ]);
    assert_eq!(
        unindexed_note(&searched),
        Some((
            "the contents of 4 files in scope aren't in the search index \
             (2 over 1 MiB, 2 too many distinct trigrams):"
                .to_string(),
            lines(&[
                "  o2  5.0 MiB",
                "  o1  2.0 MiB",
                "  t2  900 B",
                "  … (+1 more file; --json has the full list)",
            ])
        ))
    );
}

#[test]
fn one_reason_is_left_out_when_it_has_no_files() {
    let searched = zero(vec![file("r", "t", 100), file("r", "u", 200)]);
    let Some((first, _)) = unindexed_note(&searched) else {
        panic!("no note");
    };
    assert_eq!(
        first,
        "the contents of 2 files in scope aren't in the search index \
         (2 too many distinct trigrams):"
    );
}

#[test]
fn files_of_two_repositories_print_full_addresses() {
    let searched = zero(vec![
        file("github.com/a/x", "p", 2 * MIB),
        file("github.com/a/x", "q", 3 * MIB),
        file("github.com/a/y", "r", 4 * MIB),
        file("github.com/a/y", "s", 5 * MIB),
    ]);
    let Some((_, rest)) = unindexed_note(&searched) else {
        panic!("no note");
    };
    assert_eq!(
        rest,
        lines(&[
            "  github.com/a/y@0123456789ab:s  5.0 MiB",
            "  github.com/a/y@0123456789ab:r  4.0 MiB",
            "  github.com/a/x@0123456789ab:q  3.0 MiB",
            "  … (+1 more file; --json has the full list)",
        ])
    );
}

#[test]
fn a_path_with_a_space_is_quoted() {
    let searched = zero(vec![file("r", "my file.txt", 2 * MIB)]);
    let Some((_, rest)) = unindexed_note(&searched) else {
        panic!("no note");
    };
    assert_eq!(rest, lines(&["  'my file.txt'  2.0 MiB"]));
}

#[test]
fn a_lower_bound_has_no_unindexed_note_unless_zero() {
    let listed = || Some(vec![file("r", "p", 2 * MIB)]);
    let bounded = answer(
        &json!({"matchCount": 5, "limitHit": true, "results": [repo("a")]}),
        listed(),
    );
    assert!(bounded.at_least);
    assert!(unindexed_note(&bounded).is_none());
    let complete =
        answer(&json!({"matchCount": 1, "results": [repo("a")]}), listed());
    assert!(!complete.at_least);
    assert!(unindexed_note(&complete).is_some());
}

#[test]
fn a_stopped_answer_has_no_unindexed_note() {
    let mut searched = answer(
        &json!({"matchCount": 1, "results": [repo("a")]}),
        Some(vec![file("r", "p", 2 * MIB)]),
    );
    searched.stopped = true;
    assert!(unindexed_note(&searched).is_none());
}

#[test]
fn an_unknown_or_empty_list_has_no_unindexed_note() {
    assert!(unindexed_note(&answer(&json!({}), None)).is_none());
    assert!(unindexed_note(&answer(&json!({}), Some(vec![]))).is_none());
}

#[test]
fn judge_prints_the_unindexed_note_after_the_others() {
    let mut searched = zero(vec![file("r", "p", 2 * MIB)]);
    searched.unshown = vec!["NewKind".into()];
    let mut notes = Notes::new();
    assert!(judge(&searched, &mut notes).is_ok());
    let mut printed = Vec::new();
    assert!(notes.write(&mut printed).is_ok());
    assert_eq!(
        String::from_utf8_lossy(&printed),
        "sego: note: 1 result of kind NewKind not shown\n\
         sego: note: the contents of 1 file in scope aren't in the search \
         index (1 over 1 MiB):\n  p  2.0 MiB\n"
    );
}
