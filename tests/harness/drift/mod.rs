use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};

use serde_json::Value;

module_tests!();

const TRANSIENT: [&str; 3] =
    ["elapsedMilliseconds", "requestId", "extensions"];

type Shape = BTreeMap<String, BTreeSet<&'static str>>;

/// The first way `live` differs from `recorded`, or `None`. Reworded errors
/// are not drift and an error at a path the recording has none at is; list
/// elements share one shape. With `values`, the values must also be equal,
/// whatever the order of the errors.
pub fn drift(recorded: &Value, live: &Value, values: bool) -> Option<String> {
    let (mut recorded, mut live) = (recorded.clone(), live.clone());
    for value in [&mut recorded, &mut live] {
        without_transient(value);
        sort_errors(value);
    }
    if let Some(path) = new_error_path(&recorded, &live) {
        return Some(format!("new response error at {path}"));
    }
    if let Some(found) = shape_difference(&recorded, &live) {
        return Some(format!("shape: {found}"));
    }
    if values {
        return value_difference("", &recorded, &live)
            .map(|found| format!("value: {found}"));
    }
    None
}

/// A short stand-in for a body too large to show.
pub fn digest(bytes: &[u8]) -> String {
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
