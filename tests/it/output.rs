use serde::{Deserialize, Serialize};

use sego::internal::address::pinned;
use sego::internal::output::json;
use sego::internal::output::text::{
    code_line, gutter_width, header, header_line,
};
use sego::internal::output::{Marker, Notes, date};

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
fn notes_print_continuation_lines() {
    let mut notes = Notes::new();
    notes.push_lines(
        "head:",
        vec![
            "  a  1 B".into(),
            "  … (+1 more file; --json has the full list)".into(),
        ],
    );
    let mut out = Vec::new();
    notes.write(&mut out).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "sego: note: head:\n  a  1 B\n  … (+1 more file; --json has the full list)\n"
    );
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
        &pinned(
            "github.com/x/y",
            "98cb6bf42e086f6af920b965c38cacc07402d51b",
            Some("a.go"),
        ),
        &["lines 1-3 of 3".into(), "rev v1".into()],
    );
    assert_eq!(
        line,
        "github.com/x/y@98cb6bf42e08:a.go  lines 1-3 of 3  rev v1\n"
    );
    let line = header(&pinned("a", "abc", None), &["x\u{1b}y".into()]);
    assert_eq!(line, "a@abc  x\\u{1b}y\n");
}

#[test]
fn header_line_escapes_fields_but_not_the_lead() {
    let line = header_line("a/b", &["1 tag".into(), "x\u{1b}y\nz".into()]);
    assert_eq!(line, "a/b  1 tag  x\\u{1b}y z\n");
    assert_eq!(header_line("a/b", &[]), "a/b\n");
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

#[test]
fn a_date_is_the_first_ten_characters_of_a_timestamp() {
    assert_eq!(date("2026-09-14T10:19:56Z"), "2026-09-14");
    assert_eq!(date("2026-09"), "2026-09");
    assert_eq!(
        date(
            "\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}"
        ),
        "\u{e9}".repeat(10)
    );
}
