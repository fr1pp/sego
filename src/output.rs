use std::fmt::Display;
use std::io::{self, Write};

use serde::Serialize;
use serde_json::Serializer;
use serde_json::ser::{CompactFormatter, Formatter};

use crate::address::{self, Pinned, short};
use crate::escape;

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

/// Writes `answer` to `out`: one JSON document with every unsafe character
/// escaped (ADR 0006), or its text.
pub fn print<A: Display + Serialize>(
    answer: &A,
    json: bool,
    out: &mut dyn Write,
) -> io::Result<()> {
    if json {
        let mut serializer = Serializer::with_formatter(&mut *out, EscapingFormatter);
        answer.serialize(&mut serializer).map_err(io::Error::from)?;
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

/// Where a submodule points, as an address with 12 hex digits of its commit
/// when the commit is known; `None` when its URL names no repository.
pub fn submodule_target(url: &str, commit: &str, repo: &str) -> Option<String> {
    let target = address::submodule_target(url, repo)?;
    Some(if commit.is_empty() { target } else { format!("{target}@{}", short(commit)) })
}

/// A submodule as text: its target, else its stored URL, else `?`.
pub fn submodule_text(target: Option<&str>, url: &str) -> String {
    match target {
        Some(target) => target.to_owned(),
        None if url.is_empty() => "?".to_owned(),
        None => url.to_owned(),
    }
}

pub fn code_line(number: u32, marker: Marker, text: &str, focus: Option<usize>) -> String {
    let marker = match marker {
        Marker::Match => ':',
        Marker::Context => '-',
    };
    format!("{number}{marker}{}\n", escape::render_line(text, focus))
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

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
