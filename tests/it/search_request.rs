use std::process::ExitCode;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use sego::internal::error::{Error, report};
use sego::internal::search::query::prepare;
use sego::internal::search::response::{Item, Results};
use sego::internal::search::{OPERATION, fetch, request};
use sego::internal::upstream::{
    Endpoint, GRAPHQL_PATH, Reply, Request, Transport, Var, top_level,
};

use crate::support::exec::{Ran, exec};
use crate::support::fake::Fake;
use crate::support::fixture::reply;

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

fn fetched(name: &str, query: &str) -> Result<Results, Error> {
    let fake = Fake::scripted(vec![Ok(reply(&format!("Search/{name}")))]);
    fetch(&prepare(query), &fake, deadline())
}

fn rendered(error: &Error) -> String {
    let mut out = Vec::new();
    report(error, &mut out).unwrap_or_else(|e| panic!("{e}"));
    String::from_utf8(out).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn top_level_document() {
    let request: Request = top_level(
        "Probe",
        vec![Var { name: "q", ty: "String!", value: json!("a b") }],
        "search(query: $q) { x }",
    );
    assert_eq!(
        request.document,
        "query Probe($q: String!) {\n  search(query: $q) { x }\n}\n"
    );
    let body: Value = serde_json::from_slice(&request.body())
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        body,
        json!({
            "query": request.document,
            "variables": { "q": "a b" },
            "operationName": "Probe",
        })
    );
}

#[test]
fn request_carries_the_query_as_written() {
    let written = r#"repo:x "a b" (foo or bar)"#;
    assert_eq!(request(&prepare(written)).operation, OPERATION);
    let fake = Fake::scripted(vec![Ok(reply("Search/selftest-kinds"))]);
    let handle = fake.clone();
    let connect =
        move |_: Endpoint| -> Box<dyn Transport> { Box::new(handle.clone()) };
    let ran = exec(&["search", written], None, &connect);
    assert_eq!(ran.code, ExitCode::SUCCESS);
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].path, GRAPHQL_PATH);
    let body: Value = serde_json::from_slice(&calls[0].body)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        body["variables"],
        json!({ "query": r#"repo:x "a b" (foo or bar) count:100"# })
    );
    assert_eq!(body["operationName"], "Search");
}

fn envelope(results: &Value) -> Reply {
    let body = json!({ "data": { "search": { "results": results } } });
    Reply {
        status: 200,
        content_type: Some("application/json".to_owned()),
        location: None,
        retry_after: None,
        cf_mitigated: None,
        body: body.to_string().into_bytes(),
    }
}

fn results(alert: &Value, items: &Value) -> Value {
    json!({
        "matchCount": items.as_array().map_or(0, Vec::len),
        "approximateResultCount": "0",
        "limitHit": false,
        "repositoriesCount": 1,
        "timedout": [],
        "cloning": [],
        "missing": [],
        "alert": alert,
        "elapsedMilliseconds": 5,
        "dynamicFilters": [],
        "results": items,
    })
}

fn ran_with(reply: Reply, query: &str) -> (Ran, Fake) {
    let fake = Fake::scripted(vec![Ok(reply)]);
    let handle = fake.clone();
    let connect =
        move |_: Endpoint| -> Box<dyn Transport> { Box::new(handle.clone()) };
    (exec(&["search", query], None, &connect), fake)
}

const LINT: &str = "sego: note: lang: filters intersect: a file must \
                    match both\n";

#[test]
fn run_order() {
    let alert = json!({
        "title": "Heads up",
        "description": "be careful",
        "kind": null,
        "proposedQueries": null,
    });
    let items =
        json!([{ "__typename": "Repository", "name": "github.com/a/b" }]);
    let (ran, fake) =
        ran_with(envelope(&results(&alert, &items)), "lang:go lang:rust foo");
    assert_eq!(ran.code, ExitCode::SUCCESS);
    assert_eq!(
        String::from_utf8_lossy(&ran.err),
        format!("{LINT}sego: note: Heads up: be careful\n")
    );
    assert!(String::from_utf8_lossy(&ran.out).contains("github.com/a/b"));
    assert_eq!(fake.calls().len(), 1);

    let alert = json!({
        "title": "Unable To Process Query",
        "description": "bad query",
        "kind": null,
        "proposedQueries": null,
    });
    let (ran, fake) = ran_with(
        envelope(&results(&alert, &json!([]))),
        "lang:go lang:rust foo",
    );
    assert_eq!(ran.code, ExitCode::from(1));
    assert_eq!(ran.out, b"");
    assert_eq!(
        String::from_utf8_lossy(&ran.err),
        format!("{LINT}sego: error: bad query\n")
    );
    assert_eq!(fake.calls().len(), 1);
}

#[test]
fn fetch_reads_every_kind() {
    let results = fetched("selftest-kinds", "selftest-kinds")
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(results.match_count, 4);
    assert_eq!(results.approximate_result_count, "4");
    assert!(!results.limit_hit);
    assert_eq!(results.repositories_count, 1);
    assert!(results.timedout.is_empty());
    assert!(results.cloning.is_empty());
    assert!(results.missing.is_empty());
    assert!(results.alert.is_none());
    assert_eq!(results.elapsed_milliseconds, 12);
    let filter = &results.dynamic_filters[0];
    assert_eq!(filter.value, "lang:go");
    assert_eq!(filter.label, "Go");
    assert_eq!(filter.count, 3);
    assert!(!filter.limit_hit);
    assert_eq!(filter.kind, "lang");
    assert_eq!(results.results.len(), 4);

    let Item::FileMatch(file) = &results.results[0] else {
        panic!("not a file match");
    };
    assert_eq!(file.repository.name, "github.com/a/b");
    assert_eq!(file.file.path, "src/main.go");
    assert_eq!(
        file.file.commit.oid,
        "0123456789abcdef0123456789abcdef01234567"
    );
    let expr = file.rev_spec.as_ref().and_then(|r| r.expr.as_deref());
    assert_eq!(expr, Some("v1.0"));
    let chunk = &file.chunk_matches[0];
    assert_eq!(chunk.content, "func main() {\n}");
    assert_eq!(chunk.content_start.line, 9);
    let range = &chunk.ranges[0];
    assert_eq!((range.start.line, range.start.character), (9, 5));
    assert_eq!((range.end.line, range.end.character), (9, 9));
    let symbol = &file.symbols[0];
    assert_eq!(symbol.name, "main");
    assert_eq!(symbol.kind, "FUNCTION");
    assert_eq!(symbol.container_name, None);
    let line = symbol
        .location
        .as_ref()
        .and_then(|l| l.range.as_ref())
        .and_then(|r| r.start.as_ref())
        .map(|s| s.line);
    assert_eq!(line, Some(9));

    let Item::CommitSearchResult(commit) = &results.results[1] else {
        panic!("not a commit");
    };
    assert_eq!(commit.commit.subject, "Fix the thing");
    assert_eq!(commit.commit.author.date, "2024-01-02T03:04:05Z");
    let person = commit.commit.author.person.as_ref();
    assert_eq!(person.and_then(|p| p.display_name.as_deref()), None);
    assert_eq!(commit.commit.repository.name, "github.com/a/b");
    let found = &commit.matches[0];
    assert_eq!(found.body.text, "Fix the thing");
    let h = &found.highlights[0];
    assert_eq!((h.line, h.character, h.length), (0, 4, 3));

    let Item::Repository(repo) = &results.results[2] else {
        panic!("not a repository");
    };
    assert_eq!(repo.name, "github.com/c/d");
    assert!(matches!(&results.results[3], Item::Other(t) if t == "NewKind"));
}

#[test]
fn fetch_errors() {
    for name in ["selftest-located-error", "selftest-null-search"] {
        let error = fetched(name, name)
            .err()
            .unwrap_or_else(|| panic!("{name}: accepted"));
        let text = rendered(&error);
        assert!(
            text.contains("Sourcegraph failed to answer the search"),
            "{name}"
        );
        assert!(!text.contains("boom"), "{name}");
        assert!(!text.contains("search failed"), "{name}");
    }
}

#[test]
fn an_item_without_a_typename_does_not_deserialise() {
    let item = serde_json::from_value::<Item>(json!({ "name": "x" }));
    assert!(item.is_err());
}

#[test]
fn top_level_indents_every_selection_line() {
    let request = top_level("Probe", Vec::new(), "a {\n  b\n}");
    assert_eq!(request.document, "query Probe {\n  a {\n    b\n  }\n}\n");
}
