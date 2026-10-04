mod table;

use std::borrow::Cow;
use std::fmt::Write as _;

pub fn is_unsafe(c: char) -> bool {
    let i = table::UNSAFE.partition_point(|&(_, last)| last < c);
    table::UNSAFE.get(i).is_some_and(|&(first, _)| first <= c)
}

/// `raw` with each unsafe character written as `\xHH` below U+0100 and as
/// `\u{h…}` above. One trailing `\r`, a CRLF line's ending, is dropped.
pub fn sanitise(raw: &str) -> String {
    let raw = raw.strip_suffix('\r').unwrap_or(raw);
    let mut text = String::with_capacity(raw.len());
    for c in raw.chars() {
        if !is_unsafe(c) {
            text.push(c);
        } else if let Ok(byte) = u8::try_from(c) {
            let _ = write!(text, "\\x{byte:02x}");
        } else {
            text.extend(c.escape_unicode());
        }
    }
    text
}

/// `s` as one POSIX shell word that delivers its exact bytes: bare when
/// that is safe, `'…'` when it has no control or unsafe character, and
/// `$'…'` otherwise. A leading `-` is quoted, so a bare word never reads as
/// an option.
pub fn shell_word(s: &str) -> Cow<'_, str> {
    if s.starts_with('-') { Cow::Owned(quoted(s)) } else { word(s) }
}

/// `head`, the program and its options, which must already be shell words,
/// then each operand as a shell word. If any operand starts with `--`, a
/// `--` goes before the operands, and after it a leading `-` needs no
/// quoting.
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
    if !s.is_empty() && s.chars().all(bare) {
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
    }

    #[test]
    fn command_lines_put_dashes_before_dashed_operands() {
        assert_eq!(
            command_line(&["sego", "read"], &["r", "--x"]),
            "sego read -- r --x"
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
