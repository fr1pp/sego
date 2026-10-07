use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use sego::internal::upstream::{
    DEADLINE, Endpoint, GRAPHQL_PATH, Reply, Transport, Ureq,
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
    assert!(found.is_some_and(|f| f.starts_with("a is")));
}

#[test]
fn shape_names_a_new_key() {
    let found = shape(&json!({"a": 1}), &json!({"a": 1, "z": true}));
    assert_eq!(found.as_deref(), Some("z is new"));
}

#[test]
fn shape_names_a_live_null_where_the_recording_has_a_value() {
    let found = shape(&json!({"a": {"b": 1}}), &json!({"a": null}));
    assert!(found.is_some());
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
fn reordered_or_reworded_errors_are_not_drift() {
    let recorded = with_errors(&[&["r0", "meta"], &["r0", "body"]]);
    let mut live = with_errors(&[&["r0", "body"], &["r0", "meta"]]);
    live["errors"][0]["message"] = json!("another wording");
    assert_eq!(new_error_path(&recorded, &live), None);
    sort_errors(&mut live);
    assert_eq!(live["errors"].as_array().map(Vec::len), Some(2));
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
