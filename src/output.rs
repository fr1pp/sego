use std::fmt::Display;
use std::io::{self, Write};

use serde::Serialize;
use serde_json::Serializer;
use serde_json::ser::{CompactFormatter, Formatter};

use crate::address::{Pinned, short};
use crate::escape::{self, shell_word};
use crate::sourcegraph::Change;

const DATE_LEN: usize = 10;

/// The day of a timestamp: its first ten characters, `YYYY-MM-DD`.
pub fn date(timestamp: &str) -> String {
    timestamp.chars().take(DATE_LEN).collect()
}

/// `n` and the noun for that many: `1 file`, `2 files`.
pub fn count<N: Copy + Display + PartialEq + From<u8>>(n: N, one: &str, many: &str) -> String {
    let noun = if n == N::from(1) { one } else { many };
    format!("{n} {noun}")
}

/// `bytes` in base64 with padding (RFC 4648), as JSON carries bytes that
/// aren't UTF-8.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The fields a header ends with: `rev_field`, the `rev` field that
/// `address::rev_field` or `address::range_rev_field` gives, then when
/// Sourcegraph last updated its mirror, `mirrored 2026-10-27T04:17Z`.
pub fn closing_fields(
    rev_field: Option<String>,
    mirrored_at: Option<&str>,
) -> impl Iterator<Item = String> {
    rev_field.into_iter().chain(mirrored_at.map(|at| format!("mirrored {at}")))
}

/// An answer as JSON prints it: its fields, then the notes stderr carries
/// too (ADR 0014).
#[derive(Serialize)]
struct Document<'a, A> {
    #[serde(flatten)]
    answer: &'a A,
    notes: Vec<String>,
}

/// Writes `answer` to `out`: one JSON document with `notes` and every unsafe
/// character escaped (ADR 0006), or its text.
pub fn print<A: Display + Serialize>(
    answer: &A,
    json: bool,
    notes: &Notes,
    out: &mut dyn Write,
) -> io::Result<()> {
    if json {
        let document = Document { answer, notes: notes.texts() };
        let mut serializer = Serializer::with_formatter(&mut *out, EscapingFormatter);
        document.serialize(&mut serializer).map_err(io::Error::from)?;
        out.write_all(b"\n")
    } else {
        write!(out, "{answer}")
    }
}

#[derive(Debug, Default)]
pub struct Notes(Vec<Note>);

#[derive(Debug)]
struct Note {
    first: String,
    rest: Vec<String>,
}

impl Notes {
    pub fn new() -> Notes {
        Notes::default()
    }

    pub fn push(&mut self, note: impl Into<String>) {
        self.push_lines(note, Vec::new());
    }

    /// A note whose `rest` lines print under the first, each as given.
    pub fn push_lines(&mut self, first: impl Into<String>, rest: Vec<String>) {
        self.0.push(Note { first: first.into(), rest });
    }

    /// Each note's text, its lines joined by line feeds.
    pub fn texts(&self) -> Vec<String> {
        self.0
            .iter()
            .map(|note| {
                let mut text = note.first.clone();
                for line in &note.rest {
                    text.push('\n');
                    text.push_str(line);
                }
                text
            })
            .collect()
    }

    pub fn write(&self, err: &mut dyn Write) -> io::Result<()> {
        for note in &self.0 {
            writeln!(err, "sego: note: {}", escape::one_line(&note.first))?;
            for line in &note.rest {
                writeln!(err, "{}", escape::one_line(line))?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    Match,
    Context,
}

/// Writes every unsafe character as `\u` escapes of its UTF-16 units; the
/// serializer itself escapes quotes, backslashes and C0 controls.
struct EscapingFormatter;

impl Formatter for EscapingFormatter {
    fn write_string_fragment<W>(&mut self, writer: &mut W, fragment: &str) -> io::Result<()>
    where
        W: ?Sized + Write,
    {
        let mut rest = fragment;
        while let Some(at) = rest.find(escape::is_unsafe) {
            CompactFormatter.write_string_fragment(writer, &rest[..at])?;
            let mut chars = rest[at..].chars();
            let c = chars.next().unwrap_or_default();
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                write!(writer, "\\u{unit:04x}")?;
            }
            rest = chars.as_str();
        }
        CompactFormatter.write_string_fragment(writer, rest)
    }
}

pub fn header(address: &Pinned, fields: &[String]) -> String {
    header_line(&address.to_string(), fields)
}

/// `lead` is written as it is: the caller has already made it safe.
pub fn header_line(lead: &str, fields: &[String]) -> String {
    let mut line = lead.to_owned();
    for field in fields {
        line.push_str("  ");
        line.push_str(&escape::one_line(field));
    }
    line.push('\n');
    line
}

/// A submodule's commit as text: 12 hex digits, or `?` when it isn't known.
pub fn submodule_text(commit: Option<&str>) -> String {
    commit.map_or_else(|| "?".to_owned(), short)
}

pub fn code_line(number: u32, marker: Marker, text: &str, focus: Option<usize>) -> String {
    let marker = match marker {
        Marker::Match => ':',
        Marker::Context => '-',
    };
    format!("{number}{marker}{}\n", escape::render_line(text, focus))
}

#[derive(Serialize)]
pub struct FileRow {
    /// `A`, `D`, `R` or `M`. A type, mode or submodule change is an `M`.
    pub status: char,
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub added: u64,
    pub deleted: u64,
}

fn status(old: Option<&String>, new: Option<&String>) -> char {
    match (old, new) {
        (None, _) => 'A',
        (_, None) => 'D',
        (Some(old), Some(new)) if old != new => 'R',
        _ => 'M',
    }
}

pub fn file_row(change: Change) -> FileRow {
    FileRow {
        status: status(change.old_path.as_ref(), change.new_path.as_ref()),
        old_path: change.old_path,
        new_path: change.new_path,
        added: change.added,
        deleted: change.deleted,
    }
}

/// `M  +2 -2  path`, or `R  +1 -1  old -> new`, without a line break.
pub fn row_text(row: &FileRow) -> String {
    let word = |path: &Option<String>| shell_word(path.as_deref().unwrap_or_default()).into_owned();
    let path = match row.status {
        'R' => format!("{} -> {}", word(&row.old_path), word(&row.new_path)),
        'D' => word(&row.old_path),
        _ => word(&row.new_path),
    };
    format!("{}  +{} -{}  {path}", row.status, row.added, row.deleted)
}

/// `shown` rows are printed out of `total` files.
pub fn files_field(shown: usize, total: u64) -> String {
    if (shown as u64) < total {
        return format!("{shown} of {total} files shown");
    }
    count(total, "file", "files")
}

/// The rows to ask the server for: as many as text prints, or for JSON all.
pub fn rows_asked(json: bool, rows: usize) -> Option<u32> {
    (!json).then(|| u32::try_from(rows).unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Serialize)]
    struct Shown {
        repo: &'static str,
    }

    impl Display for Shown {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            writeln!(f, "{}", self.repo)
        }
    }

    #[test]
    fn json_carries_the_notes_after_the_answer_and_text_leaves_them_to_stderr() {
        let mut notes = Notes::new();
        notes.push("one");
        notes.push_lines("two:", vec!["  a".to_owned(), "  b".to_owned()]);
        let printed = |json: bool| {
            let mut out = Vec::new();
            assert!(print(&Shown { repo: "x" }, json, &notes, &mut out).is_ok());
            String::from_utf8_lossy(&out).into_owned()
        };
        assert_eq!(printed(true), "{\"repo\":\"x\",\"notes\":[\"one\",\"two:\\n  a\\n  b\"]}\n");
        assert_eq!(printed(false), "x\n");
    }

    #[test]
    fn base64_matches_rfc_4648s_test_vectors() {
        for (bytes, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(bytes.as_bytes()), encoded);
        }
        assert_eq!(base64(&[0xfb, 0xff, 0xbf]), "+/+/");
    }

    #[test]
    fn a_date_is_the_first_ten_characters_of_a_timestamp() {
        assert_eq!(date("2026-09-14T10:19:56Z"), "2026-09-14");
        assert_eq!(date("2026-09"), "2026-09");
        assert_eq!(
            date("\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}"),
            "\u{e9}".repeat(10)
        );
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Record {
        zeta: String,
        alpha: u32,
        mid: Option<String>,
    }

    fn written<T: Serialize>(value: &T) -> String {
        let mut out = Vec::new();
        value.serialize(&mut Serializer::with_formatter(&mut out, EscapingFormatter)).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn unsafe_characters_are_escaped_and_the_parsed_value_is_unchanged() {
        let record = Record { zeta: "a\u{202e}b\u{1b}\"\u{1f600}".into(), alpha: 1, mid: None };
        let raw = written(&record);
        assert!(raw.contains("\\u202e"), "{raw}");
        assert!(raw.contains(r"\u001b"), "{raw}");
        assert!(raw.contains(r#"\""#), "{raw}");
        assert!(raw.contains('\u{1f600}'), "{raw}");
        assert!(raw.ends_with('}'), "{raw}");
        assert!(raw.starts_with(r#"{"zeta":"#), "{raw}");
        assert!(raw.find("zeta") < raw.find("alpha"));
        assert!(raw.find("alpha") < raw.find("mid"));
        let back: Record = serde_json::from_str(&raw).unwrap();
        assert_eq!(back, record);
    }

    #[test]
    fn an_unsafe_character_beyond_the_basic_plane_is_a_surrogate_pair() {
        let raw = written(&"\u{e0041}");
        assert_eq!(raw, "\"\\udb40\\udc41\"");
        let back: String = serde_json::from_str(&raw).unwrap();
        assert_eq!(back, "\u{e0041}");
    }
}
