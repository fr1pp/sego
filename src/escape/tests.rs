use std::borrow::Cow;

use super::{CUT_COLUMNS, escape, is_unsafe, one_line, render_line, strip_cr};

const FAMILY: &str = "👨\u{200d}👩\u{200d}👧";

fn tail(hidden: usize) -> String {
    format!("… (+{hidden} more columns; --json has the full line)")
}

#[test]
fn controls_separators_and_invisible_characters_are_unsafe() {
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
        assert_eq!(is_unsafe(c), expected, "{c:?}");
    }
}

#[test]
fn an_unsafe_character_is_written_as_a_unicode_escape() {
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
fn text_with_nothing_unsafe_is_returned_as_it_is() {
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
    let cases =
        [("a\r", ("a", true)), ("a", ("a", false)), ("a\r\r", ("a\r", true))];
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
        (
            "ascii, no focus",
            "x".repeat(600),
            None,
            format!("{}{}", "x".repeat(500), tail(100)),
        ),
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
        (
            "tabs count 1",
            "\t".repeat(600),
            None,
            format!("{}{}", "\t".repeat(500), tail(100)),
        ),
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
        (
            "modifier",
            wave.repeat(300),
            format!("{}{}", wave.repeat(250), tail(100)),
        ),
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
    let kept =
        out.strip_suffix("… (+100 more columns; --json has the full line)");
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
