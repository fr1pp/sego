use std::borrow::Cow;

use sego::internal::escape::{
    CUT_COLUMNS, escape, is_unsafe, one_line, render_line, strip_cr,
};

const FAMILY: &str = "👨\u{200d}👩\u{200d}👧";
const TAIL: &str = "… (+100 more columns; --json has the full line)";

#[test]
fn unsafe_set() {
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
fn escape_forms() {
    let cases = [
        ("a\u{1b}[0m", "a\\u{1b}[0m"),
        ("\u{202e}x", "\\u{202e}x"),
        ("\0\u{85}", "\\u{0}\\u{85}"),
    ];
    for (input, expected) in cases {
        assert_eq!(escape(input), expected, "{input:?}");
    }
    assert!(matches!(escape("plain"), Cow::Borrowed("plain")));
}

#[test]
fn one_line_forms() {
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
fn strip_cr_forms() {
    let cases =
        [("a\r", ("a", true)), ("a", ("a", false)), ("a\r\r", ("a\r", true))];
    for (input, expected) in cases {
        assert_eq!(strip_cr(input), expected, "{input:?}");
    }
}

#[test]
fn cut() {
    assert_eq!(CUT_COLUMNS, 500);

    let ascii = "x".repeat(600);
    assert_eq!(
        render_line(&ascii, None),
        format!("{}{TAIL}", "x".repeat(500))
    );

    let focused = format!("{}Y{}", "x".repeat(550), "x".repeat(49));
    let out = render_line(&focused, Some(550));
    assert!(out.starts_with('…'), "{out}");
    assert!(out.contains('Y'), "{out}");
    assert!(!out.contains("more columns"), "{out}");
    assert_eq!(out.chars().count(), 1 + 500);

    let middle = format!("{}Y{}", "x".repeat(300), "x".repeat(299));
    let out = render_line(&middle, Some(300));
    assert!(out.starts_with('…') && out.contains('Y'), "{out}");
    assert!(out.ends_with("more columns; --json has the full line)"));

    let wide = "日".repeat(300);
    assert_eq!(
        render_line(&wide, None),
        format!(
            "{}… (+100 more columns; --json has the full line)",
            "日".repeat(250)
        )
    );

    let tabs = "\t".repeat(600);
    assert_eq!(
        render_line(&tabs, None),
        format!("{}{TAIL}", "\t".repeat(500))
    );

    let families = FAMILY.repeat(100);
    assert_eq!(render_line(&families, None), families);

    let exact = "x".repeat(500);
    assert_eq!(render_line(&exact, None), exact);
}

#[test]
fn cut_never_splits_a_zwj_sequence() {
    let families = FAMILY.repeat(300);
    let out = render_line(&families, None);
    let kept =
        out.strip_suffix("… (+100 more columns; --json has the full line)");
    assert_eq!(kept, Some(FAMILY.repeat(250).as_str()));
}

#[test]
fn cut_measures_escaped_text() {
    let line = "\u{1b}".repeat(100);
    let out = render_line(&line, None);
    let expected = format!(
        "{}… (+{} more columns; --json has the full line)",
        "\\u{1b}".repeat(83),
        100 * 6 - 83 * 6
    );
    assert_eq!(out, expected);
}
