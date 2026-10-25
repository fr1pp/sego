use serde_json::{Value, json};

use super::drift;

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
    assert_eq!(
        found.as_deref(),
        Some(r#"shape: a is {"string"}, was {"number"}"#)
    );
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
    assert_eq!(
        found.as_deref(),
        Some(r#"new response error at ["r0","c1","dir"]"#)
    );
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
