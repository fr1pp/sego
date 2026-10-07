use serde_json::{Value, json};

use sego::internal::error::report;
use sego::internal::output::Notes;
use sego::internal::search::outcome::{judge, names};
use sego::internal::search::query::prepare;
use sego::internal::search::response::{Named, Results};

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

fn run(results: &Results, deadline_ms: u64) -> (String, String) {
    let mut prepared = prepare("x");
    prepared.deadline_ms = deadline_ms;
    let mut notes = Notes::new();
    let outcome = judge(results, &prepared, &mut notes);
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

fn failure(results: &Results, deadline_ms: u64) -> String {
    let (notes, error) = run(results, deadline_ms);
    assert_eq!(notes, "");
    error
}

#[test]
fn unable_to_process_uses_description() {
    let r = results(&json!({"alert":
        alert("Unable To Process Query", json!("bad thing"))}));
    assert_eq!(failure(&r, 60000), "sego: error: bad thing\n");
}

#[test]
fn unable_to_process_falls_back_to_title() {
    for description in [json!(null), json!(""), json!("  \n ")] {
        let r = results(&json!({"alert":
            alert("Unable To Process Query", description)}));
        assert_eq!(
            failure(&r, 60000),
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
    assert_eq!(failure(&r, 60000), "sego: error: bad\n");
}

#[test]
fn one_missing_revision() {
    let r = results(&json!({"alert": alert(REVISIONS, revisions(1))}));
    assert_eq!(
        failure(&r, 60000),
        "sego: error: revision v1 not found in r1\n"
    );
}

#[test]
fn three_missing_revisions() {
    let r = results(&json!({"alert": alert(REVISIONS, revisions(3))}));
    assert_eq!(
        failure(&r, 60000),
        "sego: error: revision v1 not found in r1\n  \
         revision v2 not found in r2\n  \
         revision v3 not found in r3\n"
    );
}

#[test]
fn five_missing_revisions() {
    let r = results(&json!({"alert": alert(REVISIONS, revisions(5))}));
    assert_eq!(
        failure(&r, 60000),
        "sego: error: revision v1 not found in r1\n  \
         revision v2 not found in r2\n  \
         and 3 more revisions not found\n"
    );
}

#[test]
fn missing_revisions_without_lines_print_description() {
    let r = results(&json!({"alert": alert(REVISIONS, json!("odd text"))}));
    assert_eq!(failure(&r, 60000), "sego: error: odd text\n");
}

#[test]
fn revision_splits_at_first_at() {
    let r = results(&json!({"alert": alert(
        REVISIONS, json!("x:\n  * a/b@v@2"))}));
    assert_eq!(
        failure(&r, 60000),
        "sego: error: revision v@2 not found in a/b\n"
    );
}

#[test]
fn revisions_with_results_are_a_note() {
    let r = results(&json!({
        "results": [repo("a")],
        "alert": alert(REVISIONS, json!("Some of them.")),
    }));
    let (notes, error) = run(&r, 60000);
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
    assert_eq!(failure(&r, 60000), "sego: error: No repositories found\n");
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
        failure(&r, 60000),
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
    let (notes, error) = run(&r, 60000);
    assert_eq!(error, "");
    assert_eq!(
        notes,
        "sego: note: No repositories found: advice\n\
         sego: note: the server proposes: q1\n"
    );
}

#[test]
fn stopped_by_timedout_alone() {
    let r = results(&json!({"timedout": named(&["a", "b"])}));
    assert_eq!(
        failure(&r, 60000),
        "sego: error: the search stopped at its 60 s deadline with no \
         results\n  timed out: a, b\n"
    );
}

#[test]
fn stopped_by_elapsed_alone() {
    let r = results(&json!({"elapsedMilliseconds": 60000}));
    assert_eq!(
        failure(&r, 60000),
        "sego: error: the search stopped at its 60 s deadline with no \
         results\n"
    );
}

#[test]
fn stopped_with_zero_deadline() {
    let r = results(&json!({"elapsedMilliseconds": 0}));
    assert_eq!(
        failure(&r, 0),
        "sego: error: the search stopped at its 0 s deadline with no \
         results\n"
    );
}

#[test]
fn quick_zero_is_not_stopped() {
    let r = results(&json!({"approximateResultCount": "0+"}));
    assert_eq!(run(&r, 60000), (String::new(), String::new()));
}

#[test]
fn no_owners_alert_with_zero_results_exits_zero() {
    let r = results(&json!({
        "approximateResultCount": "0+",
        "alert": alert("Some results have no owners", json!("See docs.")),
    }));
    let (notes, error) = run(&r, 60000);
    assert_eq!(error, "");
    assert_eq!(notes, "sego: note: Some results have no owners: See docs.\n");
}

#[test]
fn alert_without_description_is_the_title() {
    let r = results(&json!({
        "results": [repo("a")],
        "alert": alert("Heads up", json!(null)),
    }));
    assert_eq!(run(&r, 60000).0, "sego: note: Heads up\n");
}

#[test]
fn partial_lists_and_deadline_notes() {
    let r = results(&json!({
        "results": [repo("a")],
        "timedout": named(&["t"]),
        "cloning": named(&["c1", "c2"]),
        "missing": named(&["m"]),
    }));
    let (notes, error) = run(&r, 60000);
    assert_eq!(error, "");
    assert_eq!(
        notes,
        "sego: note: timed out: t\n\
         sego: note: cloning: c1, c2\n\
         sego: note: missing: m\n\
         sego: note: the search stopped at its 60 s deadline\n"
    );
}

#[test]
fn elapsed_alone_notes_the_deadline() {
    let r = results(&json!({
        "results": [repo("a")],
        "elapsedMilliseconds": 1500,
    }));
    assert_eq!(
        run(&r, 1500).0,
        "sego: note: the search stopped at its 1.5 s deadline\n"
    );
}

#[test]
fn note_order_is_alert_proposals_lists_deadline_kinds() {
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
        "timedout": named(&["t"]),
    }));
    assert_eq!(
        run(&r, 60000).0,
        "sego: note: Heads up: text\n\
         sego: note: the server proposes: q\n\
         sego: note: timed out: t\n\
         sego: note: the search stopped at its 60 s deadline\n\
         sego: note: 2 results of kind NewKind not shown\n\
         sego: note: 1 result of kind Other not shown\n"
    );
}

#[test]
fn names_lists_up_to_five() {
    let list = |n: usize| -> Vec<Named> {
        (1..=n).map(|i| Named { name: format!("n{i}") }).collect()
    };
    assert_eq!(names(&list(1)), "n1");
    assert_eq!(names(&list(5)), "n1, n2, n3, n4, n5");
    assert_eq!(names(&list(8)), "n1, n2, n3, n4, n5, and 3 more");
}
