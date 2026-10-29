use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use sego::{Endpoint, Response, Transport};

use super::case::{self, Body, Case, Exchange, Request};
use super::{Paced, case_paths, cases_dir, documents_dir};

const GRAPHQL_PATH: &str = "/.api/graphql";
const DEADLINE: Duration = Duration::from_secs(90);

/// How `check` reaches the server a response was recorded from, given the
/// endpoint it was recorded from.
type Connect<'a> = &'a dyn Fn(&str) -> Result<Rc<dyn Transport>, String>;

/// Sends every recorded request of each case under the paths in `under`, or
/// of every case given none, again, one a second, and returns the ways the
/// responses have drifted.
pub fn check_cases(under: &[String]) -> Vec<String> {
    let servers: RefCell<HashMap<String, Rc<dyn Transport>>> = RefCell::default();
    let connect = |endpoint: &str| -> Result<Rc<dyn Transport>, String> {
        if let Some(server) = servers.borrow().get(endpoint) {
            return Ok(server.clone());
        }
        let parsed: Endpoint = endpoint.parse()?;
        let server: Rc<dyn Transport> = Rc::new(Paced::new(sego::connect(parsed)));
        servers.borrow_mut().insert(endpoint.to_owned(), server.clone());
        Ok(server)
    };
    let paths = match case_paths() {
        Ok(paths) => paths,
        Err(e) => return vec![e],
    };
    let (paths, mut failures) = cases_under(&cases_dir(), paths, under);
    for path in paths {
        match Case::load(&path, &documents_dir()) {
            Ok(case) => failures.extend(check(&case, &connect)),
            Err(e) => failures.push(e),
        }
    }
    failures
}

/// The cases of `cases` under each path in `under`, a case or a directory
/// relative to `root`, or every case given no path; and a failure for each
/// path that names no case, so a mistyped one can't pass unchecked.
fn cases_under(root: &Path, cases: Vec<PathBuf>, under: &[String]) -> (Vec<PathBuf>, Vec<String>) {
    if under.is_empty() {
        return (cases, Vec::new());
    }
    let is_under = |case: &Path, path: &str| {
        case.strip_prefix(root).is_ok_and(|c| c.with_extension("").starts_with(path))
    };
    let failures = under
        .iter()
        .filter(|path| !cases.iter().any(|case| is_under(case, path)))
        .map(|path| format!("{path} names no case in tests/cases"))
        .collect();
    let cases =
        cases.into_iter().filter(|case| under.iter().any(|path| is_under(case, path))).collect();
    (cases, failures)
}

/// What differs between the case's recorded responses and what the server
/// answers to the same requests now. A response that isn't recorded isn't
/// sent for. A difference counts only if a second answer shows one too:
/// sourcegraph.com now and then answers a search without parts it usually
/// sends, such as its filters.
fn check(case: &Case, connect: Connect) -> Vec<String> {
    let mut failures = Vec::new();
    for (i, exchange) in case.exchanges.iter().enumerate() {
        let Some(recorded) = &exchange.recorded else { continue };
        let name = format!("{}: exchange {}", case.path.display(), i + 1);
        let Some(request) = &exchange.request else {
            failures.push(format!("{name} is recorded but has no request to send again"));
            continue;
        };
        let result = connect(&recorded.endpoint).and_then(|transport| {
            let answer = || {
                let response = send(transport.as_ref(), request)?;
                compare(case, exchange, &response)
            };
            answer().or_else(|first| answer().map_err(|_| first))
        });
        if let Err(found) = result {
            failures.push(format!("{name} {}: {found}", describe(request)));
        }
    }
    failures
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

fn compare(case: &Case, exchange: &Exchange, response: &Response) -> Result<(), String> {
    if response.status != exchange.status {
        return Err(format!("status {}, was {}", response.status, exchange.status));
    }
    let values = exchange.recorded.as_ref().is_some_and(|r| r.live_values);
    match &exchange.body {
        Body::File(name) => {
            if !values {
                return Ok(());
            }
            let path = case.dir().join(name);
            let recorded = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let (live, recorded) = (digest(&response.body), digest(&recorded));
            if live == recorded { Ok(()) } else { Err(format!("body {live}, was {recorded}")) }
        }
        Body::Text(text) => {
            let live: Value = serde_json::from_slice(&response.body)
                .map_err(|_| format!("body is not JSON ({})", digest(&response.body)))?;
            let recorded: Value = serde_json::from_str(text)
                .map_err(|e| format!("the recorded body is not JSON: {e}"))?;
            drift(&recorded, &live, values).map_or(Ok(()), Err)
        }
    }
}

const TRANSIENT: [&str; 3] = ["elapsedMilliseconds", "requestId", "extensions"];

/// Where a fetch another client started can make a revision lookup wait and
/// fail; the lookup then answers as if the revision were missing.
const AFTER_A_FETCH: &str = "revision not found after waiting for a fetch";

type Shape = BTreeMap<String, BTreeSet<&'static str>>;

/// The first way `live` differs from `recorded`, or `None`. Reworded errors
/// are not drift and an error at a path the recording has none at is; list
/// elements share one shape. With `values`, the values must also be equal,
/// whatever the order of the errors.
fn drift(recorded: &Value, live: &Value, values: bool) -> Option<String> {
    let (mut recorded, mut live) = (recorded.clone(), live.clone());
    let (in_recorded, in_live) = (error_paths(&recorded), error_paths(&live));
    without_lookups_after_a_fetch(&mut live, &in_recorded);
    without_lookups_after_a_fetch(&mut recorded, &in_live);
    for value in [&mut recorded, &mut live] {
        without_transient(value);
        missing_revisions_alike(value);
        sort_errors(value);
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
/// `a` or `b`, or a split guess's `comparison`.
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
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::path::Path;

    use sego::TransportError;
    use tempfile::TempDir;

    use super::*;
    use crate::support::replay::response_of;

    fn shape(recorded: &Value, live: &Value) -> Option<String> {
        drift(recorded, live, false)
    }

    fn values(recorded: &Value, live: &Value) -> Option<String> {
        drift(recorded, live, true)
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
    fn reworded_errors_are_not_drift() {
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
    fn an_error_at_a_new_path_is_drift() {
        let recorded = with_errors(&[&["r0", "c2", "dir"]]);
        let live = with_errors(&[&["r0", "c2", "dir"], &["r0", "c1", "dir"]]);
        let found = shape(&recorded, &live);
        assert_eq!(found.as_deref(), Some(r#"new response error at ["r0","c1","dir"]"#));
        let clean = json!({"data": {}});
        assert!(shape(&clean, &live).is_some());
    }

    #[test]
    fn what_varies_from_one_response_to_the_next_is_not_drift() {
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
    fn only_a_lookup_that_waited_for_a_fetch_is_not_drift() {
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
    fn which_of_two_missing_names_waited_for_the_fetch_is_not_drift() {
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
    fn the_mirrors_update_time_is_not_drift() {
        let at = |time: &str| json!({"data": {"r0": {"mirrorInfo": {"updatedAt": time}}}});
        assert_eq!(values(&at("2026-10-27T04:17:00Z"), &at("2026-10-27T05:01:12Z")), None);
    }

    fn case_path(name: &str) -> PathBuf {
        Path::new("/c").join(format!("{name}.toml"))
    }

    fn under(paths: &[&str]) -> (Vec<PathBuf>, Vec<String>) {
        let cases = ["show/empty-file", "show/raw-read", "refs/tags", "refs/tags-missing"];
        let paths: Vec<String> = paths.iter().map(|&p| p.to_owned()).collect();
        cases_under(Path::new("/c"), cases.map(case_path).to_vec(), &paths)
    }

    #[test]
    fn no_path_chooses_every_case() {
        assert_eq!(under(&[]).0.len(), 4);
    }

    #[test]
    fn a_path_names_a_directory_or_a_case_by_whole_components() {
        assert_eq!(under(&["show"]).0, ["show/empty-file", "show/raw-read"].map(case_path));
        assert_eq!(under(&["refs/tags"]).0, [case_path("refs/tags")]);
    }

    #[test]
    fn a_path_that_names_no_case_fails() {
        let (chosen, failures) = under(&["refs", "show/empty"]);
        assert_eq!(chosen, ["refs/tags", "refs/tags-missing"].map(case_path));
        assert_eq!(failures, ["show/empty names no case in tests/cases"]);
    }

    /// A server that answers each request with the next response and keeps what
    /// it was asked.
    #[derive(Clone, Default)]
    struct Server {
        responses: Rc<RefCell<VecDeque<Response>>>,
        asked: Rc<RefCell<Vec<String>>>,
        endpoints: Rc<RefCell<Vec<String>>>,
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

        fn check(&self, case: &Case) -> Vec<String> {
            let server = self.clone();
            check(case, &move |endpoint| {
                server.endpoints.borrow_mut().push(endpoint.to_owned());
                Ok(Rc::new(server.clone()))
            })
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

    const RECORDED: &str =
        r#"recorded = { date = 2026-10-07, endpoint = "https://sourcegraph.com" }"#;
    const MARKED: &str = r#"recorded = { date = 2026-10-07, endpoint = "https://sourcegraph.com", live = "values" }"#;

    fn graphql(recorded: &str, body: &str) -> String {
        let document = case::scratch("Refs", "query Refs($repo0: String!) { r0 { name } }\n");
        format!(
            r#"[[exchange]]
operation = "Refs"
variables = '{{"repo0":"example.com/a"}}'
document = "{document}"
status = 200
{recorded}
response = '{body}'
"#
        )
    }

    fn raw(recorded: &str) -> String {
        format!("[[exchange]]\nget = \"/raw\"\nstatus = 200\n{recorded}\nbody = \"raw.body\"\n")
    }

    fn parse_at(path: &Path, exchanges: &[String]) -> Case {
        let text = format!("args = [\"x\"]\nstatus.code = 0\n\n{}", exchanges.join("\n"));
        Case::parse(&text, path, case::SCRATCH.path()).unwrap_or_else(|e| panic!("{e}\n{text}"))
    }

    fn parse(exchanges: &[String]) -> Case {
        parse_at(Path::new("x.toml"), exchanges)
    }

    fn json(body: &str) -> Response {
        response_of(200, None, body)
    }

    #[test]
    fn a_recorded_request_is_sent_again_as_written() {
        let case = parse(&[graphql(RECORDED, r#"{"data":{}}"#)]);
        let server = Server::answering(vec![json(r#"{"data":{}}"#)]);
        assert_eq!(server.check(&case), Vec::<String>::new());
        let sent = server.asked();
        assert_eq!(sent.len(), 1);
        let body = sent[0].strip_prefix("POST /.api/graphql ").unwrap_or_default();
        let body: Value = serde_json::from_str(body).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(body["operationName"], "Refs");
        assert_eq!(body["variables"]["repo0"], "example.com/a");
        assert_eq!(body["query"], "query Refs($repo0: String!) { r0 { name } }\n");
        assert_eq!(*server.endpoints.borrow(), ["https://sourcegraph.com"]);
    }

    #[test]
    fn each_recorded_exchange_is_sent_and_the_rest_are_not() {
        let case = parse(&[
            graphql("", r#"{"data":{}}"#),
            graphql(RECORDED, r#"{"data":{}}"#),
            raw(RECORDED),
        ]);
        let server = Server::answering(vec![json(r#"{"data":{}}"#), response_of(200, None, "raw")]);
        assert_eq!(server.check(&case), Vec::<String>::new());
        assert_eq!(server.asked().len(), 2);
        assert_eq!(server.asked()[1], "GET /raw");
    }

    #[test]
    fn a_changed_shape_fails_naming_the_exchange() {
        let case = parse(&[graphql(RECORDED, r#"{"data":{"r0":{"name":"a"}}}"#)]);
        let server = Server::answering(vec![json(r#"{"data":{"r0":null}}"#)]);
        let failures = server.check(&case);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].starts_with("x.toml: exchange 1 Refs: shape:"), "{failures:?}");
    }

    #[test]
    fn a_difference_the_second_answer_does_not_repeat_is_not_drift() {
        let case = parse(&[graphql(RECORDED, r#"{"data":{"r0":{"name":"a"}}}"#)]);
        let server = Server::answering(vec![
            json(r#"{"data":{"r0":null}}"#),
            json(r#"{"data":{"r0":{"name":"a"}}}"#),
        ]);
        assert_eq!(server.check(&case), Vec::<String>::new());
        assert_eq!(server.asked().len(), 2);
    }

    #[test]
    fn a_difference_the_second_answer_repeats_fails_once() {
        let case = parse(&[graphql(RECORDED, r#"{"data":{"r0":{"name":"a"}}}"#)]);
        let server = Server::answering(vec![
            json(r#"{"data":{"r0":null}}"#),
            json(r#"{"data":{"r0":null}}"#),
        ]);
        assert_eq!(server.check(&case).len(), 1);
        assert_eq!(server.asked().len(), 2);
    }

    #[test]
    fn values_are_compared_only_when_the_response_is_marked() {
        let recorded = r#"{"data":{"r0":{"name":"a"}}}"#;
        let moved = || vec![json(r#"{"data":{"r0":{"name":"b"}}}"#)];
        let server = Server::answering(moved());
        assert_eq!(server.check(&parse(&[graphql(RECORDED, recorded)])), Vec::<String>::new());

        let server = Server::answering(moved());
        let failures = server.check(&parse(&[graphql(MARKED, recorded)]));
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("value: data.r0.name changed"), "{failures:?}");
    }

    #[test]
    fn a_different_status_fails() {
        let case = parse(&[graphql(RECORDED, "{}")]);
        let server = Server::answering(vec![response_of(400, None, "{}")]);
        let failures = server.check(&case);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].ends_with("status 400, was 200"), "{failures:?}");
    }

    #[test]
    fn a_body_that_is_not_json_fails() {
        let case = parse(&[graphql(RECORDED, "{}")]);
        let failures = Server::answering(vec![json("<html>")]).check(&case);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("body is not JSON"), "{failures:?}");
    }

    #[test]
    fn a_transport_error_fails() {
        let case = parse(&[graphql(RECORDED, "{}")]);
        let failures = Server::answering(Vec::new()).check(&case);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("transport: Network"), "{failures:?}");
    }

    #[test]
    fn a_recorded_response_with_no_request_fails_without_sending() {
        let case = parse(&[format!("[[exchange]]\nstatus = 200\n{RECORDED}\nresponse = '{{}}'\n")]);
        let server = Server::answering(Vec::new());
        let failures = server.check(&case);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("has no request to send again"));
        assert_eq!(server.asked(), Vec::<String>::new());
    }

    #[test]
    fn a_raw_read_compares_its_body_only_when_marked() {
        let dir = TempDir::new().unwrap_or_else(|e| panic!("{e}"));
        fs::write(dir.path().join("raw.body"), b"recorded").unwrap_or_else(|e| panic!("{e}"));
        let path = dir.path().join("raw.toml");

        let server = Server::answering(vec![response_of(200, None, "moved")]);
        assert_eq!(server.check(&parse_at(&path, &[raw(RECORDED)])), Vec::<String>::new());

        let server = Server::answering(vec![response_of(200, None, "recorded")]);
        assert_eq!(server.check(&parse_at(&path, &[raw(MARKED)])), Vec::<String>::new());

        let server = Server::answering(vec![response_of(200, None, "moved")]);
        let failures = server.check(&parse_at(&path, &[raw(MARKED)]));
        assert_eq!(failures.len(), 1);
        assert!(
            failures[0].contains("raw.toml: exchange 1 GET /raw: body 5 bytes"),
            "{failures:?}"
        );
    }
}
