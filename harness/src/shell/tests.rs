use super::split;

fn words(line: &str) -> Vec<String> {
    split(line)
        .unwrap_or_else(|e| panic!("{e}"))
        .into_iter()
        .map(|word| String::from_utf8(word).unwrap_or_else(|e| panic!("{e}")))
        .collect()
}

#[test]
fn words_split_like_a_shell() {
    assert_eq!(words("show  a/b:c 3-4"), ["show", "a/b:c", "3-4"]);
    assert_eq!(words("'a b' \"c d\" e\\ f"), ["a b", "c d", "e f"]);
    assert_eq!(words(r#""a \"q\" \\ \$""#), [r#"a "q" \ \$"#]);
    assert_eq!(words(r"'a\b' x''y ''"), [r"a\b", "xy", ""]);
    assert_eq!(words(""), Vec::<String>::new());
    assert_eq!(words("$HOME a$b $"), ["$HOME", "a$b", "$"]);
}

#[test]
fn an_unterminated_quote_or_a_trailing_backslash_is_refused() {
    for bad in ["'open", "\"open", "trailing\\", "$'open", "$'x\\"] {
        assert!(split(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn ansi_c_quoting_decodes_escapes() {
    assert_eq!(words(r"$'a\nb\tc\rd'"), ["a\nb\tc\rd"]);
    assert_eq!(words(r"$'\e[0m'"), ["\u{1b}[0m"]);
    assert_eq!(words(r"$'\0'"), ["\0"]);
    assert_eq!(words(r"$'\x41\x7f'"), ["A\u{7f}"]);
    assert_eq!(words(r"$'é\u{202e}\u{1F600}'"), ["é\u{202e}\u{1F600}"]);
    assert_eq!(words(r#"$'it\'s \\ \"'"#), ["it's \\ \""]);
    assert_eq!(words(r"x$'a b'y $'c'"), ["xa by", "c"]);
}

#[test]
fn a_hex_escape_is_a_byte_not_a_character() {
    assert_eq!(split(r"$'\xff\x00'"), Ok(vec![vec![0xff, 0]]));
}

#[test]
fn a_bad_escape_is_refused() {
    for bad in [
        r"$'\q'",
        r"$'\x'",
        r"$'\xg0'",
        r"$'\u12'",
        r"$'\u{}'",
        r"$'\u{110000}'",
        r"$'\u{d800}'",
        r"$'\u{41'",
    ] {
        assert!(split(bad).is_err(), "{bad:?}");
    }
}
