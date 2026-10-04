//! The layout primitives every command's text output is built from.

use std::fmt::Write as _;

use crate::escape::{sanitise, shell_word};

/// `words` as shell words joined by spaces, then ` • ` and each item of
/// `meta`, sanitised, joined by ` • `; then a newline.
pub fn address_line(words: &[&str], meta: &[String]) -> String {
    let mut line = words
        .iter()
        .map(|&word| shell_word(word))
        .collect::<Vec<_>>()
        .join(" ");
    for item in meta {
        line.push_str(" • ");
        line.push_str(&sanitise(item));
    }
    line.push('\n');
    line
}

/// How a numbered line relates to the question: grep's `:` and `-`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    Match,
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "search shows context in a later task")
    )]
    Context,
}

/// Each line as its number, right-aligned in a column as wide as the
/// largest number and at least 4, then its marker and its text, which must
/// already be a content line. `--` separates numbers that aren't adjacent.
pub fn numbered(lines: &[(u32, Marker, String)]) -> String {
    let largest = lines.iter().map(|&(n, ..)| n).max().unwrap_or(0);
    let width = largest.to_string().len().max(4);
    let mut text = String::new();
    let mut previous = None;
    for (n, marker, line) in lines {
        if previous
            .is_some_and(|previous: u32| previous.checked_add(1) != Some(*n))
        {
            text.push_str("--\n");
        }
        previous = Some(*n);
        let marker = match marker {
            Marker::Match => ':',
            Marker::Context => '-',
        };
        // An empty line ends at its marker, with no trailing space.
        let _ = match line.as_str() {
            "" => writeln!(text, "{n:>width$}{marker}"),
            line => writeln!(text, "{n:>width$}{marker} {line}"),
        };
    }
    text
}

/// `notice: <text>`, sanitised, on a line of its own.
pub fn notice(text: &str) -> String {
    format!("notice: {}\n", sanitise(text))
}

/// `hint: <command>` on a line of its own; `command` is already shell
/// words.
pub fn hint(command: &str) -> String {
    format!("hint: {command}\n")
}

#[cfg(test)]
mod tests {
    use expect_test::expect;

    use super::*;

    #[test]
    fn address_lines_quote_words_and_join_meta() {
        let meta = ["601 lines".to_owned(), "binary\u{1b}".to_owned()];
        let line = address_line(&["x/y@a7962380ca08", "a b.go", "1-5"], &meta);
        expect![[r#"
            x/y@a7962380ca08 'a b.go' 1-5 • 601 lines • binary\x1b
        "#]]
        .assert_eq(&line);
        assert_eq!(address_line(&["x/y"], &[]), "x/y\n");
    }

    #[test]
    fn numbered_lines_align_and_separate_runs() {
        let line = |n, marker, text: &str| (n, marker, text.to_owned());
        let text = numbered(&[
            line(9, Marker::Match, "a"),
            line(10, Marker::Context, ""),
            line(12, Marker::Match, "b"),
        ]);
        expect![[r#"
               9: a
              10-
            --
              12: b
        "#]]
        .assert_eq(&text);
        let wide = numbered(&[line(99_999, Marker::Match, "x")]);
        assert_eq!(wide, "99999: x\n");
        assert_eq!(numbered(&[]), "");
    }

    #[test]
    fn notices_and_hints_are_lines_of_their_own() {
        expect![[r#"
            notice: lines 501-601 not shown\u{202e}
            hint: sego read x/y@a7962380ca08 mux.go 501-
        "#]]
        .assert_eq(
            &(notice("lines 501-601 not shown\u{202e}")
                + &hint("sego read x/y@a7962380ca08 mux.go 501-")),
        );
    }
}
