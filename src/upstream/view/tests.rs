use serde_json::json;

use super::text_at;

#[test]
fn text_at_reads_a_string_below_a_value() {
    let value = json!({"a": {"b": "x", "n": 1, "z": null}});
    assert_eq!(text_at(&value, &["a", "b"]), Some("x"));
    assert_eq!(text_at(&value, &["a", "n"]), None);
    assert_eq!(text_at(&value, &["a", "z"]), None);
    assert_eq!(text_at(&value, &["a", "missing"]), None);
    assert_eq!(text_at(&value["a"]["b"], &[]), Some("x"));
}
