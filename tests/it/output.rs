use serde::{Deserialize, Serialize};

use sego::internal::output::json;
use sego::internal::output::text::{code_line, gutter_width, header};
use sego::internal::output::{Marker, Notes};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Record {
    zeta: String,
    alpha: u32,
    mid: Option<String>,
}

#[test]
fn notes_are_prefixed_and_one_line() {
    let mut notes = Notes::new();
    notes.push("a\nb");
    let mut out = Vec::new();
    notes.write(&mut out).unwrap();
    assert_eq!(String::from_utf8(out).unwrap(), "sego: note: a b\n");
}

#[test]
fn no_notes_write_nothing() {
    let mut out = Vec::new();
    Notes::new().write(&mut out).unwrap();
    assert_eq!(out, Vec::<u8>::new());
}

#[test]
fn header_joins_with_two_spaces() {
    let line = header(
        "github.com/x/y@98cb6bf42e08:a.go",
        &["lines 1-3 of 3".into(), "rev v1".into()],
    );
    assert_eq!(
        line,
        "github.com/x/y@98cb6bf42e08:a.go  lines 1-3 of 3  rev v1\n"
    );
    let line = header("a", &["x\u{1b}y".into()]);
    assert_eq!(line, "a  x\\u{1b}y\n");
}

#[test]
fn gutter() {
    assert_eq!(gutter_width(606), 4);
    assert_eq!(gutter_width(10000), 5);
    assert_eq!(code_line(5, 4, Marker::Match, "x", None), "   5:x\n");
    assert_eq!(code_line(463, 4, Marker::Context, "", None), " 463-\n");
    let long = "x".repeat(600);
    let line = code_line(1, 4, Marker::Match, &long, None);
    assert!(line.contains("more columns"), "{line}");
    assert!(line.len() < 600);
}

#[test]
fn json_escapes_unsafe_but_keeps_values() {
    let record = Record {
        zeta: "a\u{202e}b\u{1b}\"\u{1f600}".into(),
        alpha: 1,
        mid: None,
    };
    let mut out = Vec::new();
    json::write(&record, &mut out).unwrap();
    let raw = String::from_utf8(out).unwrap();
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
fn json_escapes_astral_unsafe_as_surrogate_pairs() {
    let mut out = Vec::new();
    json::write(&"\u{e0041}", &mut out).unwrap();
    let raw = String::from_utf8(out).unwrap();
    assert_eq!(raw, "\"\\udb40\\udc41\"\n");
    let back: String = serde_json::from_str(&raw).unwrap();
    assert_eq!(back, "\u{e0041}");
}
