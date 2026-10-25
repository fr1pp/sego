use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use sego::internal::search::query::prepare;
use sego::internal::search::response::{Listing, UnindexedFile};
use sego::internal::upstream::{
    DEADLINE, Endpoint, Envelope, GRAPHQL_PATH, Reply, Transport, Ureq, Var,
    exchange, top_level,
};

use crate::support::fixture::{Fixture, load_all};

const PAUSE: Duration = Duration::from_secs(1);
const TRANSIENT: [&str; 2] = ["elapsedMilliseconds", "requestId"];

type Shape = BTreeMap<String, BTreeSet<&'static str>>;

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// The key paths of `value` with the JSON types found at each; list elements
/// share the path `...[]`.
fn shape_of(value: &Value, path: &str, shape: &mut Shape) {
    shape.entry(path.to_owned()).or_default().insert(type_name(value));
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let at = join(path, key);
                shape_of(child, &at, shape);
            }
        }
        Value::Array(items) => {
            for child in items {
                shape_of(child, &format!("{path}[]"), shape);
            }
        }
        _ => {}
    }
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() { key.to_owned() } else { format!("{path}.{key}") }
}

fn non_null(types: &BTreeSet<&'static str>) -> BTreeSet<&'static str> {
    types.iter().copied().filter(|t| *t != "null").collect()
}

fn is_under(path: &str, ancestor: &str) -> bool {
    path.len() > ancestor.len()
        && path.starts_with(ancestor)
        && matches!(path.as_bytes()[ancestor.len()], b'.' | b'[')
}

/// The first way `live` differs in shape from `recorded`. A null in the
/// recording matches anything, because a recorded null is often an absent
/// value; so does whatever sits below it.
fn shape_difference(recorded: &Value, live: &Value) -> Option<String> {
    let (mut want, mut got) = (Shape::new(), Shape::new());
    shape_of(recorded, "", &mut want);
    shape_of(live, "", &mut got);
    for (path, types) in &want {
        let types = non_null(types);
        if types.is_empty() {
            continue;
        }
        let shown = if path.is_empty() { "(root)" } else { path };
        match got.get(path).map(non_null) {
            None => return Some(format!("{shown} is missing")),
            Some(live) if live.is_empty() => {
                return Some(format!("{shown} is null, was {types:?}"));
            }
            Some(live) if live != types => {
                return Some(format!("{shown} is {live:?}, was {types:?}"));
            }
            Some(_) => {}
        }
    }
    for path in got.keys() {
        if want.contains_key(path) {
            continue;
        }
        let open = want.keys().any(|ancestor| {
            is_under(path, ancestor)
                && !want.keys().any(|below| is_under(below, ancestor))
        });
        if !open {
            return Some(format!("{path} is new"));
        }
    }
    None
}

fn without_transient(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for key in TRANSIENT {
                map.remove(key);
            }
            map.remove("extensions");
            map.values_mut().for_each(without_transient);
        }
        Value::Array(items) => items.iter_mut().for_each(without_transient),
        _ => {}
    }
}

/// The server resolves the aliases concurrently, so it lists their errors
/// in no fixed order.
fn sort_errors(value: &mut Value) {
    if let Some(Value::Array(errors)) = value.get_mut("errors") {
        errors.sort_by_key(Value::to_string);
    }
}

fn value_difference(
    path: &str,
    recorded: &Value,
    live: &Value,
) -> Option<String> {
    let shown = if path.is_empty() { "(root)" } else { path };
    match (recorded, live) {
        (Value::Object(a), Value::Object(b)) => {
            for (key, value) in a {
                let at = join(path, key);
                match b.get(key) {
                    None => return Some(format!("{at} is missing")),
                    Some(other) => {
                        if let Some(found) =
                            value_difference(&at, value, other)
                        {
                            return Some(found);
                        }
                    }
                }
            }
            b.keys()
                .find(|key| !a.contains_key(*key))
                .map(|key| format!("{} is new", join(path, key)))
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                return Some(format!(
                    "{shown} has {} items, had {}",
                    b.len(),
                    a.len()
                ));
            }
            a.iter().zip(b).enumerate().find_map(|(i, (x, y))| {
                value_difference(&format!("{path}[{i}]"), x, y)
            })
        }
        (a, b) if a == b => None,
        (a, b) => Some(format!(
            "{shown} changed ({} {}, was {} {})",
            type_name(b),
            digest(b.to_string().as_bytes()),
            type_name(a),
            digest(a.to_string().as_bytes()),
        )),
    }
}

fn digest(bytes: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{} bytes, hash {:016x}", bytes.len(), hasher.finish())
}

fn error_paths(value: &Value) -> Vec<String> {
    value
        .get("errors")
        .and_then(Value::as_array)
        .map(|errors| errors.iter().map(|e| e["path"].to_string()).collect())
        .unwrap_or_default()
}

/// The first live error at a path the recording has no error at. Paths are
/// stable where messages and their order are not.
fn new_error_path(recorded: &Value, live: &Value) -> Option<String> {
    let known = error_paths(recorded);
    error_paths(live).into_iter().find(|path| !known.contains(path))
}

fn compare_graphql(fixture: &Fixture, reply: &Reply) -> Option<String> {
    if reply.status != fixture.status {
        return Some(format!(
            "status {}, was {}",
            reply.status, fixture.status
        ));
    }
    let Ok(mut live) = serde_json::from_slice::<Value>(&reply.body) else {
        return Some(format!("body is not JSON ({})", digest(&reply.body)));
    };
    let mut recorded: Value = match serde_json::from_slice(&fixture.body()) {
        Ok(recorded) => recorded,
        Err(e) => return Some(format!("the recorded body is not JSON: {e}")),
    };
    for value in [&mut live, &mut recorded] {
        without_transient(value);
        sort_errors(value);
    }
    if let Some(path) = new_error_path(&recorded, &live) {
        return Some(format!("new response error at {path}"));
    }
    if let Some(found) = shape_difference(&recorded, &live) {
        return Some(format!("shape: {found}"));
    }
    if fixture.pinned {
        return value_difference("", &recorded, &live)
            .map(|found| format!("value: {found}"));
    }
    None
}

fn compare_raw(fixture: &Fixture, reply: &Reply) -> Option<String> {
    if reply.status != fixture.status {
        return Some(format!(
            "status {}, was {}",
            reply.status, fixture.status
        ));
    }
    if fixture.pinned {
        let (live, recorded) = (digest(&reply.body), digest(&fixture.body()));
        if live != recorded {
            return Some(format!("body {live}, was {recorded}"));
        }
    }
    None
}

fn send(upstream: &Ureq, fixture: &Fixture) -> Result<Reply, String> {
    let deadline = Instant::now() + DEADLINE;
    let reply = match (&fixture.operation, &fixture.request) {
        (Some(operation), _) => {
            let body = json!({
                "query": fixture.query,
                "variables": fixture.variables.clone().unwrap_or(Value::Null),
                "operationName": operation,
            });
            upstream.post(GRAPHQL_PATH, body.to_string().as_bytes(), deadline)
        }
        (None, Some(request)) => upstream.get(&request.path, deadline),
        (None, None) => return Err("neither operation nor request".to_owned()),
    };
    reply.map_err(|e| format!("transport: {e:?}"))
}

#[test]
#[ignore = "sends requests to sourcegraph.com"]
fn fixtures_match_sourcegraph() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let endpoint =
        Endpoint::from_env(None::<&OsStr>).unwrap_or_else(|e| panic!("{e}"));
    let upstream = Ureq::new(endpoint);
    let mut failures = Vec::new();
    let mut sent = 0;
    for fixture in load_all(&root).iter().filter(|f| !f.handwritten) {
        if sent > 0 {
            sleep(PAUSE);
        }
        sent += 1;
        let name = fixture
            .file
            .strip_prefix(&root)
            .unwrap_or(&fixture.file)
            .display()
            .to_string();
        let reply = match send(&upstream, fixture) {
            Ok(reply) => reply,
            Err(message) => {
                failures.push(format!("{name}: {message}"));
                continue;
            }
        };
        if reply.status == 429 || reply.cf_mitigated.is_some() {
            failures.push(format!(
                "{name}: BLOCKED, sourcegraph.com answered HTTP {} \
                 (cf-mitigated: {:?}); stopped",
                reply.status, reply.cf_mitigated
            ));
            break;
        }
        let difference = if fixture.operation.is_some() {
            compare_graphql(fixture, &reply)
        } else {
            compare_raw(fixture, &reply)
        };
        if let Some(found) = difference {
            failures.push(format!("{name}: {found}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {sent} fixtures drifted:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

type Rule = fn(&Envelope) -> Result<(), String>;

const SQLITE_FILE: &str = "c_src/sqlite3.c";
const X509_FILE: &str = "src/crypto/x509/x509_test.go";
const TRIGRAM_LIMIT: u64 = 1_048_576;

const LISTED: &str = "results { limitHit elapsedMilliseconds timedout { name } \
    alert { title } results { ... on FileMatch { repository { name } \
    file { path byteSize commit { oid } } } } }";
const COUNTED: &str = "results { matchCount limitHit \
    dynamicFilters { value count kind } }";

fn unindexed_of(query: &str) -> String {
    prepare(query)
        .unindexed
        .unwrap_or_else(|| panic!("`{query}` has no unindexed side query"))
}

fn listed(
    envelope: &Envelope,
    alias: &str,
) -> Result<Vec<UnindexedFile>, String> {
    let value = envelope
        .get(&[alias, "results"])
        .ok_or_else(|| format!("{alias} is missing or errored"))?;
    let listing: Listing = serde_json::from_value(value.clone())
        .map_err(|e| format!("{alias} has an unexpected shape: {e}"))?;
    Ok(listing.files())
}

fn counted(
    envelope: &Envelope,
    alias: &str,
) -> Result<(u64, bool, Value), String> {
    let value = envelope
        .get(&[alias, "results"])
        .ok_or_else(|| format!("{alias} is missing or errored"))?;
    let count = value["matchCount"].as_u64();
    let hit = value["limitHit"].as_bool();
    match (count, hit) {
        (Some(count), Some(hit)) => {
            Ok((count, hit, value["dynamicFilters"].clone()))
        }
        _ => Err(format!("{alias} lacks matchCount or limitHit")),
    }
}

fn file_bytes(files: &[UnindexedFile], path: &str) -> Result<u64, String> {
    files.iter().find(|f| f.path == path).map(|f| f.bytes).ok_or_else(|| {
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        format!("{path} is not listed (listed: {paths:?})")
    })
}

fn placeholder_listing(envelope: &Envelope) -> Result<(), String> {
    let bytes = file_bytes(&listed(envelope, "q0")?, SQLITE_FILE)?;
    if bytes > TRIGRAM_LIMIT {
        Ok(())
    } else {
        Err(format!(
            "{SQLITE_FILE} is {bytes} bytes, not over {TRIGRAM_LIMIT}"
        ))
    }
}

fn trigram_reason(envelope: &Envelope) -> Result<(), String> {
    let bytes = file_bytes(&listed(envelope, "q1")?, X509_FILE)?;
    if bytes <= TRIGRAM_LIMIT {
        Ok(())
    } else {
        Err(format!("{X509_FILE} is {bytes} bytes, over {TRIGRAM_LIMIT}"))
    }
}

fn cap_and_plus(envelope: &Envelope) -> Result<(), String> {
    let (count, hit, _) = counted(envelope, "q2")?;
    if count == 30 && hit {
        Ok(())
    } else {
        Err(format!("matchCount {count}, limitHit {hit}; want 30 and true"))
    }
}

fn complete_answer(envelope: &Envelope) -> Result<(), String> {
    let (count, hit, _) = counted(envelope, "q3")?;
    if (1..30).contains(&count) && !hit {
        Ok(())
    } else {
        Err(format!(
            "matchCount {count}, limitHit {hit}; want 1 to 29 and false"
        ))
    }
}

fn utility_counts(envelope: &Envelope) -> Result<(), String> {
    let (_, _, filters) = counted(envelope, "q4")?;
    let found = filters.as_array().into_iter().flatten().find(|f| {
        f["value"] == "archived:yes"
            && f["kind"]
                .as_str()
                .is_some_and(|k| k.eq_ignore_ascii_case("utility"))
    });
    match found.and_then(|f| f["count"].as_u64()) {
        Some(count) if count >= 1 => Ok(()),
        _ => Err(format!(
            "no utility filter archived:yes with count >= 1 ({filters})"
        )),
    }
}

#[test]
#[ignore = "sends requests to sourcegraph.com"]
fn drift_rules() {
    let queries = [
        unindexed_of(r"repo:^github\.com/elixir-sqlite/exqlite$ x"),
        unindexed_of(r"repo:^github\.com/golang/go$ file:^src/crypto/x509/ x"),
        r"repo:^github\.com/gorilla/mux$ func".to_owned(),
        r"repo:^github\.com/gorilla/mux$ catchallhandler".to_owned(),
        r"repo:^github\.com/gorilla/ zqxjkvbnmwqtyplkh".to_owned(),
    ];
    let selections = [LISTED, LISTED, COUNTED, COUNTED, COUNTED];
    let names = ["q0", "q1", "q2", "q3", "q4"];
    let vars = names
        .iter()
        .zip(&queries)
        .map(|(name, query)| Var {
            name,
            ty: "String!",
            value: Value::String(query.clone()),
        })
        .collect();
    let selection = names
        .iter()
        .zip(selections)
        .map(|(name, fields)| {
            format!(
                "{name}: search(query: ${name}, version: V3, \
                 patternType: keyword) {{ {fields} }}"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let request = top_level("DriftRules", vars, &selection);
    let endpoint =
        Endpoint::from_env(None::<&OsStr>).unwrap_or_else(|e| panic!("{e}"));
    let upstream = Ureq::new(endpoint);
    let envelope = exchange(&upstream, &request, Instant::now() + DEADLINE)
        .unwrap_or_else(|e| panic!("{e}"));

    let rules: [(&str, Rule); 5] = [
        ("placeholder listing", placeholder_listing),
        ("trigram reason", trigram_reason),
        ("cap and plus", cap_and_plus),
        ("complete answer", complete_answer),
        ("utility counts", utility_counts),
    ];
    let failures: Vec<String> = rules
        .iter()
        .filter_map(|(name, check)| {
            check(&envelope).err().map(|why| format!("{name}: {why}"))
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} rules no longer hold:\n{}",
        failures.len(),
        rules.len(),
        failures.join("\n")
    );
}

fn shape(recorded: &Value, live: &Value) -> Option<String> {
    shape_difference(recorded, live)
}

#[test]
fn shape_names_a_missing_key() {
    let found = shape(&json!({"a": 1, "b": "x"}), &json!({"a": 1}));
    assert_eq!(found.as_deref(), Some("b is missing"));
}

#[test]
fn shape_names_a_type_change() {
    let found = shape(&json!({"a": 1}), &json!({"a": "1"}));
    assert_eq!(found.as_deref(), Some(r#"a is {"string"}, was {"number"}"#));
}

#[test]
fn shape_names_a_new_key() {
    let found = shape(&json!({"a": 1}), &json!({"a": 1, "z": true}));
    assert_eq!(found.as_deref(), Some("z is new"));
}

#[test]
fn shape_names_a_live_null_where_the_recording_has_a_value() {
    let found = shape(&json!({"a": {"b": 1}}), &json!({"a": null}));
    assert_eq!(found.as_deref(), Some(r#"a is null, was {"object"}"#));
}

#[test]
fn a_recorded_null_matches_a_live_subtree() {
    assert_eq!(shape(&json!({"a": null}), &json!({"a": {"b": [1, 2]}})), None);
    assert_eq!(shape(&json!({"a": []}), &json!({"a": [{"b": 1}]})), None);
}

#[test]
fn list_elements_share_one_shape() {
    let recorded = json!({"a": [{"b": 1}, {"b": null}]});
    let live = json!({"a": [{"b": null}, {"b": 2}, {"b": 3}]});
    assert_eq!(shape(&recorded, &live), None);
}

#[test]
fn values_differ_at_the_first_path() {
    let found =
        value_difference("", &json!({"a": [1, 2]}), &json!({"a": [1, 3]}));
    assert!(found.is_some_and(|f| f.starts_with("a[1] changed")));
    let found =
        value_difference("", &json!({"a": 1}), &json!({"a": 1, "k": 2}));
    assert_eq!(found.as_deref(), Some("k is new"));
    assert_eq!(value_difference("", &json!({"a": 1}), &json!({"a": 1})), None);
}

fn with_errors(paths: &[&[&str]]) -> Value {
    let errors: Vec<Value> = paths
        .iter()
        .map(|path| json!({"message": format!("m {path:?}"), "path": path}))
        .collect();
    json!({"data": {}, "errors": errors})
}

#[test]
fn reworded_errors_are_not_drift() {
    let recorded = with_errors(&[&["r0", "meta"], &["r0", "body"]]);
    let mut live = recorded.clone();
    live["errors"][0]["message"] = json!("another wording");
    assert_eq!(new_error_path(&recorded, &live), None);
}

#[test]
fn a_pinned_fixture_ignores_the_order_of_errors() {
    let recorded = with_errors(&[&["r0", "meta"], &["r0", "body"]]);
    let live = with_errors(&[&["r0", "body"], &["r0", "meta"]]);
    let fixture = Fixture {
        operation: Some("ShowFile".to_owned()),
        pinned: true,
        response: Some(recorded),
        ..Fixture::default()
    };
    let reply = Reply {
        status: 200,
        content_type: None,
        location: None,
        retry_after: None,
        cf_mitigated: None,
        body: live.to_string().into_bytes(),
    };
    assert_eq!(compare_graphql(&fixture, &reply), None);
}

#[test]
fn an_error_at_a_new_path_is_drift() {
    let recorded = with_errors(&[&["r0", "c2", "dir"]]);
    let live = with_errors(&[&["r0", "c2", "dir"], &["r0", "c1", "dir"]]);
    let found = new_error_path(&recorded, &live);
    assert_eq!(found.as_deref(), Some(r#"["r0","c1","dir"]"#));
    let clean = json!({"data": {}});
    assert!(new_error_path(&clean, &live).is_some());
    assert_eq!(new_error_path(&recorded, &clean), None);
}

/// Queries from `tests/fixtures/plan/examples.jsonl`, across the plan's
/// rules, in the three modes sourcegraph.com can be asked to read a query
/// in. Keyword mode can't be checked this way: `parseSearchQuery` reads a
/// query sent as keyword as literal.
const PLANNED: [&str; 13] = [
    "repo:x patterntype:literal (a or b) or c",
    "repo:x patterntype:literal (a -file:x) or c",
    "repo:x patterntype:literal (\"a or b\") or baz",
    "repo:x patterntype:regexp (\"b or c\") or d",
    "a (file:y b) or c \"q (z\" patterntype:regexp",
    "patterntype:regexp (\"x(\" b) (c or repo:d e) \"q (z\"",
    "repo:x patterntype:regexp (foo and bar) or baz",
    "patterntype:structural foo(...) or (lang:go bar(...))",
    r"repo:x patterntype:structural (file:^a\.go$ foo(...) or file:^b\.go$ bar(...))",
    r"foo(...) or bar(...) file:^a\.go$ patterntype:structural",
    r"repo:x not file:y file:^a\.go$ patterntype:structural foo(...) or bar(...)",
    r"repo:x file:has.content(x) file:\.go$ patterntype:structural foo(...)",
    r"repo:x l:go file:^Makefile$ patterntype:structural foo(...)",
];

/// The searches a parse tree from `parseSearchQuery` implies, each filter
/// written `-field:value`, with ` [p]` after a predicate: an `or` with a
/// filter inside splits, and an `and` multiplies its items' searches.
fn tree_searches(node: &Value) -> Vec<Vec<String>> {
    fn holds_filter(node: &Value) -> bool {
        match node {
            Value::Array(items) => items.iter().any(holds_filter),
            Value::Object(map) if map.contains_key("field") => true,
            Value::Object(map) => map.values().any(holds_filter),
            _ => false,
        }
    }
    let and = |items: &[Value]| {
        let mut searches = vec![Vec::new()];
        for item in items {
            let mut next = Vec::new();
            for own in tree_searches(item) {
                for sofar in &searches {
                    next.push([sofar.clone(), own.clone()].concat());
                }
            }
            searches = next;
        }
        searches
    };
    match node {
        Value::Array(items) => and(items),
        Value::Object(map) => {
            if let Some(Value::Array(items)) = map.get("and") {
                and(items)
            } else if let Some(Value::Array(items)) = map.get("or") {
                if holds_filter(node) {
                    items.iter().flat_map(tree_searches).collect()
                } else {
                    vec![Vec::new()]
                }
            } else if let Some(field) =
                map.get("field").and_then(Value::as_str)
            {
                let dash = if map["negated"] == true { "-" } else { "" };
                let value = map["value"].as_str().unwrap_or_default();
                let labels = map["labels"].as_array().into_iter().flatten();
                let predicate = labels.into_iter().any(|l| l == "IsPredicate");
                let mark = if predicate { " [p]" } else { "" };
                vec![vec![format!("{dash}{field}:{value}{mark}")]]
            } else {
                vec![Vec::new()]
            }
        }
        _ => vec![Vec::new()],
    }
}

/// sourcegraph.com's parse tree for each query of `PLANNED`, asked for one
/// at a time in the query's mode, implies the searches and filters the plan
/// module gives it.
#[test]
#[ignore = "sends requests to sourcegraph.com"]
fn drift_plan() {
    use sego::internal::search::plan::{Plan, plan};
    let endpoint =
        Endpoint::from_env(None::<&OsStr>).unwrap_or_else(|e| panic!("{e}"));
    let upstream = Ureq::new(endpoint);
    let mut failures = Vec::new();
    for query in PLANNED {
        let Plan::Read(searches) = plan(query) else { panic!("{query}") };
        let mode = searches.pattern_type.name();
        let mine: Vec<Vec<String>> = searches
            .list(4096)
            .unwrap_or_default()
            .into_iter()
            .map(|filters| {
                filters
                    .into_iter()
                    .map(|f| {
                        let dash = if f.negated { "-" } else { "" };
                        let mark = if f.predicate { " [p]" } else { "" };
                        format!("{dash}{}:{}{mark}", f.field, f.value)
                    })
                    .collect()
            })
            .collect();
        let var = Var { name: "q", ty: "String!", value: json!(query) };
        let selection = format!(
            "parseSearchQuery(query: $q, patternType: {mode}, \
             outputPhase: PARSE_TREE, outputFormat: JSON)"
        );
        let request = top_level("DriftPlan", vec![var], &selection);
        let envelope =
            exchange(&upstream, &request, Instant::now() + DEADLINE)
                .unwrap_or_else(|e| panic!("{query}: {e}"));
        let tree =
            envelope.data["parseSearchQuery"].as_str().unwrap_or("null");
        let tree: Value =
            serde_json::from_str(tree).unwrap_or_else(|e| panic!("{e}"));
        let theirs = tree_searches(&tree);
        if mine != theirs {
            failures.push(format!(
                "{query}\n  sego:   {mine:?}\n  server: {theirs:?}"
            ));
        }
        sleep(PAUSE);
    }
    assert!(
        failures.is_empty(),
        "{} of {} plans differ from the server's parse tree:\n{}",
        failures.len(),
        PLANNED.len(),
        failures.join("\n")
    );
}
