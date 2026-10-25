use serde::{Deserialize, Serialize};

use super::write;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Record {
    zeta: String,
    alpha: u32,
    mid: Option<String>,
}

fn written<T: Serialize>(value: &T) -> String {
    let mut out = Vec::new();
    write(value, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn unsafe_characters_are_escaped_and_the_parsed_value_is_unchanged() {
    let record = Record {
        zeta: "a\u{202e}b\u{1b}\"\u{1f600}".into(),
        alpha: 1,
        mid: None,
    };
    let raw = written(&record);
    assert!(raw.contains("\\u202e"), "{raw}");
    assert!(raw.contains(r"\u001b"), "{raw}");
    assert!(raw.contains(r#"\""#), "{raw}");
    assert!(raw.contains('\u{1f600}'), "{raw}");
    assert!(raw.ends_with("}\n"), "{raw}");
    assert!(raw.starts_with(r#"{"zeta":"#), "{raw}");
    assert!(raw.find("zeta") < raw.find("alpha"));
    assert!(raw.find("alpha") < raw.find("mid"));
    let back: Record = serde_json::from_str(&raw).unwrap();
    assert_eq!(back, record);
}

#[test]
fn an_unsafe_character_beyond_the_basic_plane_is_a_surrogate_pair() {
    let raw = written(&"\u{e0041}");
    assert_eq!(raw, "\"\\udb40\\udc41\"\n");
    let back: String = serde_json::from_str(&raw).unwrap();
    assert_eq!(back, "\u{e0041}");
}
