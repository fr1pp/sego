mod table;

use std::borrow::Cow;
use std::fmt::Write as _;

use unicode_width::UnicodeWidthChar as _;

pub fn is_unsafe(c: char) -> bool {
    let i = table::UNSAFE.partition_point(|&(_, last)| last < c);
    table::UNSAFE.get(i).is_some_and(|&(first, _)| first <= c)
}

/// `raw` with each unsafe character written as `\xHH` below U+0100 and as
/// `\u{h…}` above. One trailing `\r`, a CRLF line's ending, is dropped.
pub fn sanitise(raw: &str) -> String {
    sanitise_marking(raw, None).0
}

/// `sanitise(raw)` and the byte offset in it of the character at codepoint
/// index `focus` in `raw`, or of the end when `focus` is past the last one.
fn sanitise_marking(
    raw: &str,
    focus: Option<usize>,
) -> (String, Option<usize>) {
    let raw = raw.strip_suffix('\r').unwrap_or(raw);
    let mut text = String::with_capacity(raw.len());
    let mut marked = None;
    for (i, c) in raw.chars().enumerate() {
        if Some(i) == focus {
            marked = Some(text.len());
        }
        if !is_unsafe(c) {
            text.push(c);
        } else if let Ok(byte) = u8::try_from(c) {
            let _ = write!(text, "\\x{byte:02x}");
        } else {
            text.extend(c.escape_unicode());
        }
    }
    let marked = focus.map(|_| marked.unwrap_or(text.len()));
    (text, marked)
}

/// The widest content line, in display columns, printed whole.
pub const CLIP_COLUMNS: usize = 500;

/// `raw` sanitised, then, if wider than `CLIP_COLUMNS`, cut to a window that
/// wide: centred on the character at codepoint index `focus` in `raw` but
/// kept inside the line, or from the start without a focus. `…` marks each
/// cut, and a closing note counts the columns dropped.
pub fn content_line(raw: &str, focus: Option<usize>) -> String {
    // The only control character `sanitise` keeps is tab, which
    // `UnicodeWidthStr` also counts as one column.
    fn width(c: char) -> usize {
        c.width().unwrap_or(1)
    }
    let (text, focus) = sanitise_marking(raw, focus);
    let total: usize = text.chars().map(width).sum();
    if total <= CLIP_COLUMNS {
        return text;
    }
    let start = focus.map_or(0, |at| {
        let column: usize = text[..at].chars().map(width).sum();
        column.saturating_sub(CLIP_COLUMNS / 2).min(total - CLIP_COLUMNS)
    });
    let end = start + CLIP_COLUMNS;
    let (mut first, mut last, mut kept, mut column) = (None, 0, 0, 0);
    for (i, c) in text.char_indices() {
        let w = width(c);
        if column >= start && column + w <= end {
            first.get_or_insert(i);
            last = i + c.len_utf8();
            kept += w;
        }
        column += w;
    }
    let first = first.unwrap_or(last);
    let lead = if first > 0 { "…" } else { "" };
    format!(
        "{lead}{}… (+{} more columns; --json has the full line)",
        &text[first..last],
        total - kept
    )
}

/// `s` as one POSIX shell word that delivers its exact bytes: bare when
/// that is safe, `'…'` when it has no control or unsafe character, and
/// `$'…'` otherwise. A leading `-` is quoted, so a bare word never reads as
/// an option, and so is a leading `=`, which zsh expands to a command's
/// path.
pub fn shell_word(s: &str) -> Cow<'_, str> {
    if s.starts_with('-') { Cow::Owned(quoted(s)) } else { word(s) }
}

/// `head`, the program and its options, which must already be shell words,
/// then each operand as a shell word. If any operand starts with `--`, a
/// `--` goes before the operands, and after it a leading `-` needs no
/// quoting; a leading `=` still does.
pub fn command_line(head: &[&str], operands: &[&str]) -> String {
    let dashes = operands.iter().any(|operand| operand.starts_with("--"));
    let mut line = head.join(" ");
    if dashes {
        line.push_str(" --");
    }
    for &operand in operands {
        let operand = if dashes { word(operand) } else { shell_word(operand) };
        line.push(' ');
        line.push_str(&operand);
    }
    line
}

fn word(s: &str) -> Cow<'_, str> {
    let bare = |c: char| c.is_ascii_alphanumeric() || "@%+=:,./_-".contains(c);
    if !s.is_empty() && !s.starts_with('=') && s.chars().all(bare) {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(quoted(s))
    }
}

fn quoted(s: &str) -> String {
    let escaped = |c: char| c.is_control() || is_unsafe(c);
    if !s.chars().any(escaped) {
        return format!("'{}'", s.replace('\'', r"'\''"));
    }
    // POSIX.1-2024 `$'…'` has no `\u`, and its `\x` may swallow a following
    // hex digit, but `\ddd` stops at three digits.
    let mut text = String::from("$'");
    for c in s.chars() {
        match c {
            '\\' => text.push_str(r"\\"),
            '\'' => text.push_str(r"\'"),
            '\n' => text.push_str(r"\n"),
            '\t' => text.push_str(r"\t"),
            '\r' => text.push_str(r"\r"),
            c if escaped(c) => {
                for byte in c.encode_utf8(&mut [0; 4]).bytes() {
                    let _ = write!(text, "\\{byte:03o}");
                }
            }
            c => text.push(c),
        }
    }
    text.push('\'');
    text
}

/// Strings that need every form of shell word and the escape set.
#[cfg(test)]
pub const CORPUS: &[&str] = &[
    "",
    "plain",
    "two words",
    "it's",
    "$HOME",
    "`id`",
    "a\\b",
    "*?[]{}",
    "!#~",
    "new\nline",
    "tab\t",
    "cr\r",
    "esc\u{1b}[0m",
    "rtl\u{202e}",
    "zw\u{200b}",
    "zw\u{200b}7b",
    "汉语",
    "🚀",
    "-dash",
    "--long",
    "=foo",
];

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    #[test]
    fn table_is_sorted_and_disjoint() {
        for &(first, last) in table::UNSAFE {
            assert!(first <= last, "{first:?}..={last:?}");
        }
        for [(_, last), (next, _)] in table::UNSAFE.array_windows() {
            assert!(last < next, "{last:?} then {next:?}");
        }
    }

    #[test]
    fn sanitise_escapes_unsafe_characters() {
        assert_eq!(sanitise("a\u{202e}b"), r"a\u{202e}b");
        assert_eq!(sanitise("\u{1b}[31m"), r"\x1b[31m");
        assert_eq!(sanitise("\u{7f}\u{9b}"), r"\x7f\x9b");
        assert_eq!(sanitise("zero\u{200b}width"), r"zero\u{200b}width");
        assert_eq!(sanitise("line\u{2028}sep"), r"line\u{2028}sep");
    }

    #[test]
    fn sanitise_keeps_safe_text() {
        assert_eq!(sanitise("\tindented"), "\tindented");
        assert_eq!(sanitise("汉语 🚀"), "汉语 🚀");
        assert_eq!(
            sanitise("👨\u{200d}👩\u{200d}👧"),
            "👨\u{200d}👩\u{200d}👧"
        );
        assert_eq!(sanitise("❤\u{fe0f}"), "❤\u{fe0f}");
    }

    #[test]
    fn sanitise_strips_one_trailing_carriage_return() {
        assert_eq!(sanitise("crlf\r"), "crlf");
        assert_eq!(sanitise("mid\rline"), r"mid\x0dline");
    }

    #[test]
    fn short_lines_are_untouched() {
        assert_eq!(content_line("let x = 1;", Some(4)), "let x = 1;");
    }

    #[test]
    fn long_lines_keep_a_window_and_say_so() {
        let line = "x".repeat(600);
        assert_eq!(
            content_line(&line, None),
            format!(
                "{}… (+100 more columns; --json has the full line)",
                "x".repeat(500)
            )
        );
        let clipped = content_line(
            &format!("{}NEEDLE{}", "a".repeat(550), "b".repeat(300)),
            Some(550),
        );
        assert!(clipped.starts_with('…') && clipped.contains("NEEDLE"));
    }

    #[test]
    fn focus_centres_the_window() {
        assert_eq!(
            content_line(&"x".repeat(1000), Some(500)),
            format!(
                "…{}… (+500 more columns; --json has the full line)",
                "x".repeat(500)
            )
        );
    }

    #[test]
    fn wide_characters_count_two_columns() {
        let clipped = content_line(&"汉".repeat(300), None);
        assert!(clipped.starts_with(&format!("{}…", "汉".repeat(250))));
    }

    #[test]
    fn focus_survives_escapes_before_it() {
        let line = format!("{}NEEDLE", "\u{202e}".repeat(400));
        let clipped = content_line(&line, Some(400));
        assert!(clipped.starts_with('…') && clipped.contains("NEEDLE"));
    }

    #[test]
    fn shell_words() {
        assert_eq!(shell_word("mux.go"), "mux.go");
        assert_eq!(
            shell_word("github.com/gorilla/mux@a7962380ca08"),
            "github.com/gorilla/mux@a7962380ca08"
        );
        assert_eq!(shell_word(""), "''");
        assert_eq!(shell_word("src/my file.go"), "'src/my file.go'");
        assert_eq!(shell_word("it's"), r"'it'\''s'");
        assert_eq!(shell_word("a7962380~1"), "'a7962380~1'");
        assert_eq!(shell_word("a\u{200b}b"), r"$'a\342\200\213b'");
        assert_eq!(shell_word("tab\there"), r"$'tab\there'");
        assert_eq!(shell_word("back\\slash\u{1b}"), r"$'back\\slash\033'");
        assert_eq!(shell_word("=foo"), "'=foo'");
        assert_eq!(shell_word("a=b"), "a=b");
    }

    #[test]
    fn command_lines_put_dashes_before_dashed_operands() {
        assert_eq!(
            command_line(&["sego", "read"], &["r", "--x"]),
            "sego read -- r --x"
        );
        assert_eq!(
            command_line(&["sego", "read"], &["r", "--x", "=y"]),
            "sego read -- r --x '=y'"
        );
    }

    #[test]
    fn shell_words_round_trip_through_bash() {
        for &word in CORPUS {
            let script = format!("printf %s {}", shell_word(word));
            let output =
                Command::new("bash").args(["-c", &script]).output().unwrap();
            assert!(output.status.success(), "{script}: {output:?}");
            assert_eq!(output.stdout, word.as_bytes(), "{script}");
        }
    }
}
