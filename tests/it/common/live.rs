use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use sego::{Endpoint, Response, Transport};

use super::recording::{Recording, Request, Store};
use super::{Paced, REFUSED};

const GRAPHQL_PATH: &str = "/.api/graphql";
const DEADLINE: Duration = Duration::from_secs(90);

/// How long the second answer waits, for a search shed under load to clear.
const SECOND_ANSWER: Duration = Duration::from_secs(5);

/// Sends the request of each recording whose name holds `filter` again, and
/// says how each answer differs. Once the server has refused a request, the
/// rest would only be refused too.
pub fn drift(filter: &str) -> Result<Vec<String>, String> {
    let store = Store::default();
    let transport = Paced::new(sego::connect(Endpoint::default()));
    let mut drifted = Vec::new();
    for name in store.names()?.iter().filter(|name| name.contains(filter)) {
        let recording = store.load(name)?;
        if let Err(found) = check(&recording, &transport, SECOND_ANSWER) {
            drifted.push(format!("{name} {}: {found}", describe(&recording.request)));
            if found.contains(REFUSED) {
                break;
            }
        }
    }
    Ok(drifted)
}

/// What differs between the recording and what the server answers to its
/// request now. A difference counts only if a second answer, `wait` later,
/// shows one too: sourcegraph.com now and then answers a search without parts
/// it usually sends, such as its filters.
fn check(recording: &Recording, transport: &dyn Transport, wait: Duration) -> Result<(), String> {
    let answer = || {
        let response = send(transport, &recording.request)?;
        compare(recording, &response)
    };
    answer().or_else(|first| {
        if first.contains(REFUSED) {
            return Err(first);
        }
        thread::sleep(wait);
        answer().map_err(|_| first)
    })
}

fn describe(request: &Request) -> String {
    match request {
        Request::Graphql { operation, .. } => operation.clone(),
        Request::Raw { path } => format!("GET {path}"),
    }
}

fn send(transport: &dyn Transport, request: &Request) -> Result<Response, String> {
    let deadline = Instant::now() + DEADLINE;
    let response = match request {
        Request::Graphql { operation, variables, query } => {
            let body = json!({"query": query, "variables": variables, "operationName": operation});
            transport.post(GRAPHQL_PATH, body.to_string().as_bytes(), deadline)
        }
        Request::Raw { path } => transport.get(path, deadline),
    };
    response.map_err(|e| format!("transport: {e:?}"))
}

fn compare(recording: &Recording, response: &Response) -> Result<(), String> {
    if response.status != recording.response.status {
        return Err(format!("status {}, was {}", response.status, recording.response.status));
    }
    let recorded = matches!(recording.request, Request::Graphql { .. })
        .then(|| serde_json::from_slice::<Value>(&recording.response.body).ok())
        .flatten();
    let Some(recorded) = recorded else {
        let (live, recorded) = (digest(&response.body), digest(&recording.response.body));
        return if live == recorded { Ok(()) } else { Err(format!("body {live}, was {recorded}")) };
    };
    let live: Value = serde_json::from_slice(&response.body)
        .map_err(|_| format!("body is not JSON ({})", digest(&response.body)))?;
    let values = !grows(&recording.request)
        && !capped(&recorded)
        && !capped(&live)
        && commit_ids(&recorded) == commit_ids(&live);
    difference(&recorded, &live, values).map_or(Ok(()), Err)
}

/// Whether the answer lists refs, which grow without any commit moving.
fn grows(request: &Request) -> bool {
    matches!(request, Request::Graphql { operation, .. } if ["Refs", "CommitTags"].contains(&operation.as_str()))
}

/// Whether the search reached its cap, since which results fill it varies.
fn capped(answer: &Value) -> bool {
    answer.pointer("/data/search/results/limitHit") == Some(&Value::Bool(true))
}

/// Every commit ID in `value`. While these match the recording's, the answer
/// is about the same commits, so its values should match too; once they move,
/// only its shape can.
fn commit_ids(value: &Value) -> BTreeSet<&str> {
    let mut ids = BTreeSet::new();
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::Object(map) => {
                ids.extend(map.get("oid").and_then(Value::as_str));
                pending.extend(map.values());
            }
            Value::Array(items) => pending.extend(items),
            _ => {}
        }
    }
    ids
}

const TRANSIENT: [&str; 3] = ["elapsedMilliseconds", "requestId", "extensions"];

/// Where a fetch another client started can make a revision lookup wait and
/// fail; the lookup then answers as if the revision were missing.
const AFTER_A_FETCH: &str = "revision not found after waiting for a fetch";

type Shape = BTreeMap<String, BTreeSet<&'static str>>;

/// The first way `live` differs from `recorded`, or `None`. Reworded errors
/// don't count and an error at a path the recording has none at does; list
/// elements share one shape. With `values`, the values must also be equal,
/// whatever the order of the errors.
fn difference(recorded: &Value, live: &Value, values: bool) -> Option<String> {
    let (mut recorded, mut live) = (recorded.clone(), live.clone());
    let (in_recorded, in_live) = (error_paths(&recorded), error_paths(&live));
    without_lookups_after_a_fetch(&mut live, &in_recorded);
    without_lookups_after_a_fetch(&mut recorded, &in_live);
    for value in [&mut recorded, &mut live] {
        without_transient(value);
        missing_revisions_alike(value);
        sort_errors(value);
        sort_search_results(value);
    }
    if let Some(path) = new_error_path(&recorded, &live) {
        return Some(format!("new response error at {path}"));
    }
    if let Some(found) = shape_difference(&recorded, &live) {
        return Some(format!("shape: {found}"));
    }
    if values {
        return value_difference("", &recorded, &live).map(|found| format!("value: {found}"));
    }
    None
}

/// A short stand-in for a body too large to show.
fn digest(bytes: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{} bytes, hash {:016x}", bytes.len(), hasher.finish())
}

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
            is_under(path, ancestor) && !want.keys().any(|below| is_under(below, ancestor))
        });
        if !open {
            return Some(format!("{path} is new"));
        }
    }
    None
}

/// Drops the transient keys, and the mirror's `updatedAt`, which moves
/// whenever Sourcegraph updates the mirror.
fn without_transient(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for key in TRANSIENT {
                map.remove(key);
            }
            if let Some(Value::Object(mirror)) = map.get_mut("mirrorInfo") {
                mirror.remove("updatedAt");
            }
            map.values_mut().for_each(without_transient);
        }
        Value::Array(items) => items.iter_mut().for_each(without_transient),
        _ => {}
    }
}

/// Whether the error sits at a revision lookup: a `commit`, a range's end
/// `a` or `b`, or a `comparison`.
fn at_a_lookup(error: &Value) -> bool {
    let last = error["path"].as_array().and_then(|path| path.last()).and_then(Value::as_str);
    matches!(last, Some("commit" | "a" | "b" | "comparison"))
}

/// Drops the errors of revision lookups in `answer` that waited for a fetch
/// another client started, where the other answer has no error at `other`:
/// no request can avoid them.
fn without_lookups_after_a_fetch(answer: &mut Value, other: &[String]) {
    let Value::Object(map) = answer else { return };
    if let Some(Value::Array(errors)) = map.get_mut("errors") {
        errors.retain(|e| {
            let waited = e["message"].as_str().is_some_and(|m| m.starts_with(AFTER_A_FETCH));
            !(waited && at_a_lookup(e) && !other.contains(&e["path"].to_string()))
        });
        if errors.is_empty() {
            map.remove("errors");
        }
    }
}

/// Reads every revision-not-found error at a lookup alike: when two
/// missing names share one fetch, which of them waited for it varies from
/// one answer to the next.
fn missing_revisions_alike(value: &mut Value) {
    if let Some(Value::Array(errors)) = value.get_mut("errors") {
        for error in errors.iter_mut().filter(|e| at_a_lookup(e)) {
            if error["message"].as_str().is_some_and(|m| m.starts_with("revision not found")) {
                error["message"] = Value::from("revision not found");
            }
        }
    }
}

/// A search's results arrive in whatever order the files were read, at any
/// revision, so their order means nothing.
fn sort_search_results(value: &mut Value) {
    let results = value.pointer_mut("/data/search/results/results").and_then(Value::as_array_mut);
    if let Some(results) = results {
        results.sort_by_cached_key(Value::to_string);
    }
}

/// The server resolves the aliases concurrently, so it lists their errors
/// in no fixed order.
fn sort_errors(value: &mut Value) {
    if let Some(Value::Array(errors)) = value.get_mut("errors") {
        errors.sort_by_key(Value::to_string);
    }
}

fn value_difference(path: &str, recorded: &Value, live: &Value) -> Option<String> {
    let shown = if path.is_empty() { "(root)" } else { path };
    match (recorded, live) {
        (Value::Object(a), Value::Object(b)) => {
            for (key, value) in a {
                let at = join(path, key);
                match b.get(key) {
                    None => return Some(format!("{at} is missing")),
                    Some(other) => {
                        if let Some(found) = value_difference(&at, value, other) {
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
                return Some(format!("{shown} has {} items, had {}", b.len(), a.len()));
            }
            a.iter()
                .zip(b)
                .enumerate()
                .find_map(|(i, (x, y))| value_difference(&format!("{path}[{i}]"), x, y))
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

#[cfg(test)]
mod tests {
    use sego::TransportError;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    use super::*;
    use crate::common::recording::ServerResponse;
    use crate::common::replay::response_of;

    fn shape(recorded: &Value, live: &Value) -> Option<String> {
        difference(recorded, live, false)
    }

    fn values(recorded: &Value, live: &Value) -> Option<String> {
        difference(recorded, live, true)
    }

    #[test]
    fn shape_names_a_missing_key() {
        let found = shape(&json!({"a": 1, "b": "x"}), &json!({"a": 1}));
        assert_eq!(found.as_deref(), Some("shape: b is missing"));
    }

    #[test]
    fn shape_names_a_type_change() {
        let found = shape(&json!({"a": 1}), &json!({"a": "1"}));
        assert_eq!(found.as_deref(), Some(r#"shape: a is {"string"}, was {"number"}"#));
    }

    #[test]
    fn shape_names_a_new_key() {
        let found = shape(&json!({"a": 1}), &json!({"a": 1, "z": true}));
        assert_eq!(found.as_deref(), Some("shape: z is new"));
    }

    #[test]
    fn shape_names_a_live_null_where_the_recording_has_a_value() {
        let found = shape(&json!({"a": {"b": 1}}), &json!({"a": null}));
        assert_eq!(found.as_deref(), Some(r#"shape: a is null, was {"object"}"#));
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
    fn values_may_differ_when_only_the_shape_is_compared() {
        assert_eq!(shape(&json!({"a": [1, 2]}), &json!({"a": [3]})), None);
    }

    #[test]
    fn values_differ_at_the_first_path() {
        let found = values(&json!({"a": [1, 2]}), &json!({"a": [1, 3]}));
        assert!(found.is_some_and(|f| f.starts_with("value: a[1] changed")));
        let found = values(&json!({"a": null}), &json!({"a": 1}));
        assert!(found.is_some_and(|f| f.starts_with("value: a changed")));
        let found = values(&json!({"a": [1]}), &json!({"a": [1, 2]}));
        assert_eq!(found.as_deref(), Some("value: a has 2 items, had 1"));
        assert_eq!(values(&json!({"a": 1}), &json!({"a": 1})), None);
    }

    fn with_errors(paths: &[&[&str]]) -> Value {
        let errors: Vec<Value> = paths
            .iter()
            .map(|path| json!({"message": format!("m {path:?}"), "path": path}))
            .collect();
        json!({"data": {}, "errors": errors})
    }

    #[test]
    fn reworded_errors_do_not_count() {
        let recorded = with_errors(&[&["r0", "meta"], &["r0", "body"]]);
        let mut live = recorded.clone();
        live["errors"][0]["message"] = json!("another wording");
        assert_eq!(shape(&recorded, &live), None);
    }

    #[test]
    fn a_values_comparison_ignores_the_order_of_errors() {
        let recorded = with_errors(&[&["r0", "meta"], &["r0", "body"]]);
        let live = with_errors(&[&["r0", "body"], &["r0", "meta"]]);
        assert_eq!(values(&recorded, &live), None);
    }

    #[test]
    fn an_error_at_a_new_path_counts() {
        let recorded = with_errors(&[&["r0", "c2", "dir"]]);
        let live = with_errors(&[&["r0", "c2", "dir"], &["r0", "c1", "dir"]]);
        let found = shape(&recorded, &live);
        assert_eq!(found.as_deref(), Some(r#"new response error at ["r0","c1","dir"]"#));
        let clean = json!({"data": {}});
        assert!(shape(&clean, &live).is_some());
    }

    #[test]
    fn what_varies_from_one_response_to_the_next_does_not_count() {
        let recorded = json!({
            "data": {"a": 1, "elapsedMilliseconds": 5},
            "extensions": {"trace": "x"},
            "requestId": "one",
        });
        let live = json!({
            "data": {"a": 1, "elapsedMilliseconds": 9},
            "extensions": {"trace": "y", "more": true},
            "requestId": "two",
        });
        assert_eq!(values(&recorded, &live), None);
    }

    #[test]
    fn only_a_lookup_that_waited_for_a_fetch_does_not_count() {
        let not_found = |path: &[&str], message: &str| json!({"message": message, "path": path});
        let recorded = json!({
            "data": {"m0": {"commit": null}, "d0": null},
            "errors": [not_found(&["d0", "comparison"], "revision not found: repo 1 @v1^0")],
        });
        let live = json!({
            "data": {"m0": {"commit": null}, "d0": null},
            "errors": [
                not_found(&["m0", "commit"], "revision not found after waiting for a fetch: repo 1"),
                not_found(&["d0", "comparison"], "revision not found: repo 1 @v1^0"),
            ],
        });
        assert_eq!(values(&recorded, &live), None);
        let at_a_path = json!({
            "data": {"m0": {"commit": null}, "d0": null},
            "errors": [
                not_found(&["m0", "p0"], "revision not found after waiting for a fetch: repo 1"),
                not_found(&["d0", "comparison"], "revision not found: repo 1 @v1^0"),
            ],
        });
        assert!(shape(&recorded, &at_a_path).is_some());
        let failed = json!({
            "data": {"m0": {"commit": null}, "d0": null},
            "errors": [not_found(&["m0", "commit"], "internal git failure")],
        });
        assert!(shape(&recorded, &failed).is_some());
        let missing = json!({
            "data": {"m0": {"commit": null}, "d0": null},
            "errors": [
                not_found(&["m0", "commit"], "revision not found: repo 1 @v1"),
                not_found(&["d0", "comparison"], "revision not found: repo 1 @v1^0"),
            ],
        });
        assert!(shape(&recorded, &missing).is_some());
    }

    #[test]
    fn which_of_two_missing_names_waited_for_the_fetch_does_not_count() {
        let error =
            |path: &str, message: &str| json!({"message": message, "path": [path, "comparison"]});
        let recorded = json!({"data": {"g0": null, "g1": null}, "errors": [
            error("g0", "revision not found: repo 1 @x^0"),
            error("g1", "revision not found after waiting for a fetch already in progress: repo 1"),
        ]});
        let live = json!({"data": {"g0": null, "g1": null}, "errors": [
            error("g0", "revision not found after waiting for a fetch already in progress: repo 1"),
            error("g1", "revision not found: repo 1 @x/y^0"),
        ]});
        assert_eq!(values(&recorded, &live), None);
        let quiet = json!({"data": {"g0": null, "g1": null}, "errors": [
            error("g0", "revision not found: repo 1 @x^0"),
        ]});
        assert_eq!(values(&recorded, &quiet), None);
    }

    #[test]
    fn the_mirrors_update_time_does_not_count() {
        let at = |time: &str| json!({"data": {"r0": {"mirrorInfo": {"updatedAt": time}}}});
        assert_eq!(values(&at("2026-10-27T04:17:00Z"), &at("2026-10-27T05:01:12Z")), None);
    }

    /// A server that answers each request with the next response and keeps what
    /// it was asked.
    #[derive(Clone, Default)]
    struct Server {
        responses: Rc<RefCell<VecDeque<Response>>>,
        asked: Rc<RefCell<Vec<String>>>,
    }

    impl Server {
        fn answering(responses: Vec<Response>) -> Server {
            let server = Server::default();
            *server.responses.borrow_mut() = responses.into();
            server
        }

        fn asked(&self) -> Vec<String> {
            self.asked.borrow().clone()
        }

        fn check(&self, recording: &Recording) -> Result<(), String> {
            check(recording, self, Duration::ZERO)
        }

        fn serve(&self, what: String) -> Result<Response, TransportError> {
            self.asked.borrow_mut().push(what);
            self.responses
                .borrow_mut()
                .pop_front()
                .ok_or(TransportError::Network("no response left"))
        }
    }

    impl Transport for Server {
        fn post(&self, path: &str, body: &[u8], _: Instant) -> Result<Response, TransportError> {
            self.serve(format!("POST {path} {}", String::from_utf8_lossy(body)))
        }

        fn get(&self, path: &str, _: Instant) -> Result<Response, TransportError> {
            self.serve(format!("GET {path}"))
        }
    }

    fn graphql(body: &str) -> Recording {
        Recording {
            request: Request::Graphql {
                operation: "Refs".to_owned(),
                variables: json!({"repo0": "example.com/a"}),
                query: "query Refs($repo0: String!) { r0 { name } }\n".to_owned(),
            },
            response: ServerResponse {
                status: 200,
                content_type: Some("application/json".to_owned()),
                body: body.as_bytes().to_vec(),
            },
            date: "2026-10-07".to_owned(),
        }
    }

    fn file(body: &str) -> Recording {
        let mut recording = graphql(body);
        let Request::Graphql { operation, .. } = &mut recording.request else {
            panic!("a GraphQL request")
        };
        "File".clone_into(operation);
        recording
    }

    fn json(body: &str) -> Response {
        response_of(200, Some("application/json"), body)
    }

    #[test]
    fn a_recorded_request_is_sent_again_as_written() {
        let server = Server::answering(vec![json(r#"{"data":{}}"#)]);
        assert_eq!(server.check(&graphql(r#"{"data":{}}"#)), Ok(()));
        let sent = server.asked();
        assert_eq!(sent.len(), 1);
        let body = sent[0].strip_prefix("POST /.api/graphql ").unwrap_or_default();
        let body: Value = serde_json::from_str(body).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(body["operationName"], "Refs");
        assert_eq!(body["variables"]["repo0"], "example.com/a");
        assert_eq!(body["query"], "query Refs($repo0: String!) { r0 { name } }\n");
    }

    #[test]
    fn a_changed_shape_fails() {
        let server = Server::answering(vec![
            json(r#"{"data":{"r0":null}}"#),
            json(r#"{"data":{"r0":null}}"#),
        ]);
        let failure = server.check(&graphql(r#"{"data":{"r0":{"name":"a"}}}"#));
        assert!(failure.is_err_and(|f| f.starts_with("shape:")));
    }

    #[test]
    fn the_order_of_a_searchs_results_does_not_count() {
        let search = |results: &str| {
            format!(r#"{{"data":{{"search":{{"results":{{"results":[{results}]}}}}}}}}"#)
        };
        let recording = file(&search(r#"{"path":"a"},{"path":"b"}"#));
        let reordered = json(&search(r#"{"path":"b"},{"path":"a"}"#));
        assert_eq!(Server::answering(vec![reordered]).check(&recording), Ok(()));
        let changed = || json(&search(r#"{"path":"b"},{"path":"c"}"#));
        assert!(Server::answering(vec![changed(), changed()]).check(&recording).is_err());
    }

    #[test]
    fn a_capped_search_compares_only_its_shape() {
        let search = |path: &str| {
            format!(
                r#"{{"data":{{"search":{{"results":{{"limitHit":true,"results":[{{"path":"{path}"}}]}}}}}}}}"#
            )
        };
        let moved = json(&search("b"));
        assert_eq!(Server::answering(vec![moved]).check(&graphql(&search("a"))), Ok(()));
    }

    #[test]
    fn refs_compare_only_their_shape_since_they_grow_without_a_commit_moving() {
        for operation in ["Refs", "CommitTags"] {
            let mut recording = graphql(r#"{"data":{"tags":[{"name":"a"}],"total":1}}"#);
            let Request::Graphql { operation: named, .. } = &mut recording.request else {
                panic!("a GraphQL request")
            };
            *named = operation.to_owned();
            let grown = json(r#"{"data":{"tags":[{"name":"a"},{"name":"b"}],"total":2}}"#);
            assert_eq!(Server::answering(vec![grown]).check(&recording), Ok(()), "{operation}");
        }
    }

    #[test]
    fn a_refused_request_is_not_tried_again() {
        let recording = graphql("{}");
        let refused = Refused::default();
        let failure = check(&recording, &refused, Duration::from_secs(60));
        assert!(failure.is_err_and(|f| f.contains(REFUSED)));
        assert_eq!(*refused.sent.borrow(), 1);
    }

    /// A transport behind a gate that has closed.
    #[derive(Default)]
    struct Refused {
        sent: RefCell<usize>,
    }

    impl Transport for Refused {
        fn post(&self, _: &str, _: &[u8], _: Instant) -> Result<Response, TransportError> {
            *self.sent.borrow_mut() += 1;
            Err(TransportError::Network(REFUSED))
        }

        fn get(&self, _: &str, _: Instant) -> Result<Response, TransportError> {
            Err(TransportError::Network(REFUSED))
        }
    }

    #[test]
    fn a_difference_the_second_answer_does_not_repeat_does_not_count() {
        let server = Server::answering(vec![
            json(r#"{"data":{"r0":null}}"#),
            json(r#"{"data":{"r0":{"name":"a"}}}"#),
        ]);
        assert_eq!(server.check(&graphql(r#"{"data":{"r0":{"name":"a"}}}"#)), Ok(()));
        assert_eq!(server.asked().len(), 2);
    }

    #[test]
    fn a_difference_the_second_answer_repeats_fails_once() {
        let server = Server::answering(vec![
            json(r#"{"data":{"r0":null}}"#),
            json(r#"{"data":{"r0":null}}"#),
        ]);
        assert!(server.check(&graphql(r#"{"data":{"r0":{"name":"a"}}}"#)).is_err());
        assert_eq!(server.asked().len(), 2);
    }

    #[test]
    fn values_are_compared_while_the_commit_ids_match() {
        let recording = file(r#"{"data":{"commit":{"oid":"a1"},"name":"a"}}"#);
        let same = || json(r#"{"data":{"commit":{"oid":"a1"},"name":"a"}}"#);
        assert_eq!(Server::answering(vec![same()]).check(&recording), Ok(()));
        let changed = || json(r#"{"data":{"commit":{"oid":"a1"},"name":"b"}}"#);
        let failure = Server::answering(vec![changed(), changed()]).check(&recording);
        assert!(failure.is_err_and(|f| f.contains("value: data.name changed")));
    }

    #[test]
    fn only_the_shape_is_compared_once_the_commit_ids_move() {
        let recording = file(r#"{"data":{"commit":{"oid":"a1"},"name":"a"}}"#);
        let moved = json(r#"{"data":{"commit":{"oid":"b2"},"name":"b"}}"#);
        assert_eq!(Server::answering(vec![moved]).check(&recording), Ok(()));
    }

    #[test]
    fn an_answer_with_no_commit_ids_compares_its_values() {
        let recording = file(r#"{"data":{"name":"a"}}"#);
        let changed = || json(r#"{"data":{"name":"b"}}"#);
        let failure = Server::answering(vec![changed(), changed()]).check(&recording);
        assert!(failure.is_err_and(|f| f.contains("value: data.name changed")));
    }

    #[test]
    fn a_different_status_fails() {
        let bad = || response_of(400, None, "{}");
        let failure = Server::answering(vec![bad(), bad()]).check(&graphql("{}"));
        assert_eq!(failure, Err("status 400, was 200".to_owned()));
    }

    #[test]
    fn a_body_that_is_not_json_fails() {
        let failure = Server::answering(vec![json("<html>"), json("<html>")]).check(&graphql("{}"));
        assert!(failure.is_err_and(|f| f.contains("body is not JSON")));
    }

    #[test]
    fn a_transport_error_fails() {
        let failure = Server::answering(Vec::new()).check(&graphql("{}"));
        assert!(failure.is_err_and(|f| f.contains("transport: Network")));
    }

    #[test]
    fn a_raw_read_compares_its_bytes() {
        let recording = Recording {
            request: Request::Raw { path: "/raw".to_owned() },
            response: ServerResponse {
                status: 200,
                content_type: None,
                body: b"recorded".to_vec(),
            },
            date: "d".to_owned(),
        };
        let same = vec![response_of(200, None, "recorded")];
        assert_eq!(Server::answering(same).check(&recording), Ok(()));
        let moved = || response_of(200, None, "moved");
        let failure = Server::answering(vec![moved(), moved()]).check(&recording);
        assert!(failure.is_err_and(|f| f.starts_with("body 5 bytes")));
        assert_eq!(describe(&recording.request), "GET /raw");
    }
}
