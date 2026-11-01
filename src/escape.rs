//! Makes text safe to print, or to paste into a shell.

use std::borrow::Cow;
use std::fmt::Write as _;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

mod table;

pub const CUT_COLUMNS: usize = 500;

pub fn needs_escape(c: char) -> bool {
    table::UNSAFE
        .binary_search_by(|&(first, last)| {
            if last < c {
                std::cmp::Ordering::Less
            } else if first > c {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

fn push_escaped(out: &mut String, c: char) {
    if needs_escape(c) {
        let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
    } else {
        out.push(c);
    }
}

pub fn escape(s: &str) -> Cow<'_, str> {
    if !s.chars().any(needs_escape) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        push_escaped(&mut out, c);
    }
    Cow::Owned(out)
}

pub fn one_line(s: &str) -> String {
    escape(&s.replace("\r\n", " ").replace('\n', " ")).into_owned()
}

/// `bytes` as text, each byte that isn't part of valid UTF-8 written as
/// `\x{hh}`.
pub fn utf8_escaped(bytes: &[u8]) -> Cow<'_, str> {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(bytes.len() + 16);
    for chunk in bytes.utf8_chunks() {
        out.push_str(chunk.valid());
        for byte in chunk.invalid() {
            let _ = write!(out, "\\x{{{byte:02x}}}");
        }
    }
    Cow::Owned(out)
}

pub fn strip_cr(line: &str) -> (&str, bool) {
    match line.strip_suffix('\r') {
        Some(rest) => (rest, true),
        None => (line, false),
    }
}

/// A run of escaped text that the cut never splits: a base character with
/// the zero-width characters, emoji modifiers and ZWJ-joined characters that
/// follow it, or a pair of regional indicators (a flag).
struct Unit {
    text: String,
    width: usize,
    holds_focus: bool,
}

fn units(line: &str, focus: Option<usize>) -> Vec<Unit> {
    let mut units: Vec<Unit> = Vec::new();
    let mut after_joiner = false;
    let mut indicators = 0;
    for (index, c) in line.chars().enumerate() {
        let mut piece = String::new();
        push_escaped(&mut piece, c);
        let indicator = ('\u{1f1e6}'..='\u{1f1ff}').contains(&c);
        let modifier = ('\u{1f3fb}'..='\u{1f3ff}').contains(&c);
        let attaches = !needs_escape(c)
            && (after_joiner
                || modifier
                || UnicodeWidthChar::width(c) == Some(0)
                || (indicator && indicators % 2 == 1));
        after_joiner = c == '\u{200d}';
        indicators = if !indicator {
            0
        } else if attaches {
            indicators + 1
        } else {
            1
        };
        let holds_focus = focus == Some(index);
        match units.last_mut() {
            Some(last) if attaches => {
                last.text.push_str(&piece);
                last.holds_focus |= holds_focus;
            }
            _ => units.push(Unit { text: piece, width: 0, holds_focus }),
        }
    }
    for unit in &mut units {
        unit.width = unit.text.width();
    }
    units
}

/// Escapes `line` and, past `CUT_COLUMNS` display columns, keeps a window of
/// at most that many: centred on the character at `focus` (a code-point
/// index, clamped to the line), or from column 0 without one.
pub fn render_line(line: &str, focus: Option<usize>) -> String {
    let escaped = escape(line);
    let total = escaped.width();
    if total <= CUT_COLUMNS {
        return escaped.into_owned();
    }

    let focus = focus.map(|f| f.min(line.chars().count().saturating_sub(1)));
    let units = units(line, focus);
    let focus_unit = units.iter().position(|u| u.holds_focus);

    let mut start = 0;
    if let Some(focus_unit) = focus_unit {
        let mut before = 0;
        let mut centre = 0;
        for (i, unit) in units.iter().enumerate() {
            if i == focus_unit {
                centre = before + unit.width / 2;
                break;
            }
            before += unit.width;
        }
        let target = centre.saturating_sub(CUT_COLUMNS / 2);
        let mut column = 0;
        start = units
            .iter()
            .position(|unit| {
                let at = column;
                column += unit.width;
                at >= target
            })
            .unwrap_or(units.len())
            .min(focus_unit);
    }

    let mut end = start;
    let mut kept = 0;
    while end < units.len() && kept + units[end].width <= CUT_COLUMNS {
        kept += units[end].width;
        end += 1;
    }
    while end == units.len() && start > 0 {
        let width = units[start - 1].width;
        if kept + width > CUT_COLUMNS {
            break;
        }
        start -= 1;
        kept += width;
    }

    let window: String = units[start..end].iter().map(|unit| unit.text.as_str()).collect();
    let mut out = String::with_capacity(window.len() + 64);
    if start > 0 {
        out.push('…');
    }
    out.push_str(&window);
    if start > 0 || end < units.len() {
        let hidden = total.saturating_sub(window.width());
        let _ = write!(out, "… (+{hidden} more columns; --json has the full line)");
    }
    out
}

fn is_bare(c: char) -> bool {
    c.is_ascii_alphanumeric() || "@%+=:,./_-".contains(c)
}

fn needs_ansi_c(c: char) -> bool {
    c.is_control() || needs_escape(c)
}

pub fn shell_word(s: &str) -> Cow<'_, str> {
    if !s.is_empty() && !s.starts_with(['-', '=']) && s.chars().all(is_bare) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 3);
    if s.chars().any(needs_ansi_c) {
        out.push_str("$'");
        for c in s.chars() {
            match c {
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                '\\' => out.push_str("\\\\"),
                '\'' => out.push_str("\\'"),
                c if needs_ansi_c(c) => {
                    let mut buf = [0; 4];
                    for byte in c.encode_utf8(&mut buf).bytes() {
                        let _ = write!(out, "\\{byte:03o}");
                    }
                }
                c => out.push(c),
            }
        }
        out.push('\'');
    } else {
        out.push('\'');
        for c in s.chars() {
            if c == '\'' {
                out.push_str("'\\''");
            } else {
                out.push(c);
            }
        }
        out.push('\'');
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::borrow::Cow;
    use std::process::Command;

    const FAMILY: &str = "👨\u{200d}👩\u{200d}👧";

    #[test]
    fn bytes_that_arent_utf8_are_written_as_hex_escapes() {
        assert_eq!(utf8_escaped(b"plain"), "plain");
        assert_eq!(utf8_escaped("é".as_bytes()), "é");
        assert_eq!(
            utf8_escaped(b"\"\xf8\x88\x80\x80\x80\""),
            "\"\\x{f8}\\x{88}\\x{80}\\x{80}\\x{80}\""
        );
        assert_eq!(utf8_escaped(b"\xff\xfeh\x00"), "\\x{ff}\\x{fe}h\u{0}");
    }

    fn tail(hidden: usize) -> String {
        format!("… (+{hidden} more columns; --json has the full line)")
    }

    #[test]
    fn controls_separators_and_invisible_characters_need_escaping() {
        let cases = [
            ('\u{1b}', true),
            ('\u{7f}', true),
            ('\u{85}', true),
            ('\u{202e}', true),
            ('\u{200b}', true),
            ('\u{2028}', true),
            ('\r', true),
            ('\0', true),
            ('\t', false),
            ('\u{200d}', false),
            ('\u{fe0f}', false),
            ('é', false),
            ('日', false),
        ];
        for (c, expected) in cases {
            assert_eq!(needs_escape(c), expected, "{c:?}");
        }
    }

    #[test]
    fn a_character_that_needs_escaping_is_written_as_a_unicode_escape() {
        let cases = [
            ("a\u{1b}[0m", "a\\u{1b}[0m"),
            ("\u{202e}x", "\\u{202e}x"),
            ("\0\u{85}", "\\u{0}\\u{85}"),
        ];
        for (input, expected) in cases {
            assert_eq!(escape(input), expected, "{input:?}");
        }
    }

    #[test]
    fn text_with_nothing_to_escape_is_returned_as_it_is() {
        assert!(matches!(escape("plain"), Cow::Borrowed("plain")));
    }

    #[test]
    fn a_line_break_becomes_a_space_and_a_lone_carriage_return_is_escaped() {
        let cases = [
            ("a\r\nb", "a b"),
            ("a\nb", "a b"),
            ("a\rb", "a\\u{d}b"),
            ("a\u{1b}\nb", "a\\u{1b} b"),
        ];
        for (input, expected) in cases {
            assert_eq!(one_line(input), expected, "{input:?}");
        }
    }

    #[test]
    fn one_trailing_carriage_return_is_split_off() {
        let cases = [("a\r", ("a", true)), ("a", ("a", false)), ("a\r\r", ("a\r", true))];
        for (input, expected) in cases {
            assert_eq!(strip_cr(input), expected, "{input:?}");
        }
    }

    #[test]
    fn a_line_is_cut_past_500_columns_around_the_focus() {
        assert_eq!(CUT_COLUMNS, 500);

        let focus_550 = format!("{}Y{}", "x".repeat(550), "x".repeat(49));
        let focus_300 = format!("{}Y{}", "x".repeat(300), "x".repeat(299));
        let families = FAMILY.repeat(100);
        let exact = "x".repeat(500);
        let cases: Vec<(&str, String, Option<usize>, String)> = vec![
            ("ascii, no focus", "x".repeat(600), None, format!("{}{}", "x".repeat(500), tail(100))),
            (
                "focus near the end",
                focus_550,
                Some(550),
                format!("…{}Y{}{}", "x".repeat(450), "x".repeat(49), tail(100)),
            ),
            (
                "focus in the middle",
                focus_300,
                Some(300),
                format!("…{}Y{}{}", "x".repeat(250), "x".repeat(249), tail(100)),
            ),
            (
                "wide characters",
                "日".repeat(300),
                None,
                format!("{}{}", "日".repeat(250), tail(100)),
            ),
            ("tabs count 1", "\t".repeat(600), None, format!("{}{}", "\t".repeat(500), tail(100))),
            ("zwj families under the limit", families.clone(), None, families),
            ("exactly 500 columns", exact.clone(), None, exact),
        ];
        for (label, line, focus, expected) in cases {
            assert_eq!(render_line(&line, focus), expected, "{label}");
        }
    }

    #[test]
    fn a_cut_keeps_emoji_modifiers_and_flags_whole() {
        let wave = "👋🏽";
        let flag = "🇫🇷";
        let cases = [
            ("modifier", wave.repeat(300), format!("{}{}", wave.repeat(250), tail(100))),
            (
                "modifier after ascii",
                format!("x{}", wave.repeat(300)),
                format!("x{}{}", wave.repeat(249), tail(102)),
            ),
            (
                "flag after ascii",
                format!("x{}", flag.repeat(300)),
                format!("x{}{}", flag.repeat(249), tail(102)),
            ),
        ];
        for (label, line, expected) in cases {
            assert_eq!(render_line(&line, None), expected, "{label}");
        }
    }

    #[test]
    fn a_cut_never_splits_a_zero_width_joiner_sequence() {
        let families = FAMILY.repeat(300);
        let out = render_line(&families, None);
        let kept = out.strip_suffix("… (+100 more columns; --json has the full line)");
        assert_eq!(kept, Some(FAMILY.repeat(250).as_str()));
    }

    #[test]
    fn a_cut_measures_the_escaped_text() {
        let line = "\u{1b}".repeat(100);
        let out = render_line(&line, None);
        let expected = format!(
            "{}… (+{} more columns; --json has the full line)",
            "\\u{1b}".repeat(83),
            100 * 6 - 83 * 6
        );
        assert_eq!(out, expected);
    }

    #[test]
    fn a_token_that_needs_quoting_is_single_quoted() {
        let cases = [
            ("a b.txt", "'a b.txt'"),
            ("it's", "'it'\\''s'"),
            ("-x", "'-x'"),
            ("=x", "'=x'"),
            ("", "''"),
        ];
        for (token, expected) in cases {
            assert_eq!(shell_word(token), expected, "{token:?}");
        }
    }

    #[test]
    fn a_token_with_a_control_character_is_ansi_c_quoted() {
        let cases = [
            ("a\nb", "$'a\\nb'"),
            ("a\tb", "$'a\\tb'"),
            ("a\u{1b}b", "$'a\\033b'"),
            ("a\\\nb", "$'a\\\\\\nb'"),
            ("it's\n", "$'it\\'s\\n'"),
            ("日本 \u{202e}\n", "$'日本 \\342\\200\\256\\n'"),
        ];
        for (token, expected) in cases {
            assert_eq!(shell_word(token), expected, "{token:?}");
        }
    }

    #[test]
    fn bash_reads_a_quoted_token_back_as_the_token() {
        if Command::new("bash").arg("--version").output().is_err() {
            eprintln!("skipping: bash is not installed");
            return;
        }
        let corpus = [
            "github.com/gorilla/mux@98cb6bf42e08:mux.go",
            "a b.txt",
            "it's",
            "-x",
            "=x",
            "",
            "a\nb",
            "a\u{1b}b",
            "日本 語",
            "$HOME",
            "`",
            "!x",
            "*",
            "[a]",
            "a\u{202e}b",
        ];
        for token in corpus {
            let script = format!("shopt -s failglob; printf %s {}", shell_word(token));
            let output = Command::new("bash").arg("-c").arg(&script).output().unwrap();
            assert!(output.status.success(), "{script}");
            assert_eq!(output.stdout, token.as_bytes(), "{script}");
        }
    }
}
