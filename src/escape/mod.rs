mod table;

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

#[cfg(test)]
mod tests {
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
}
