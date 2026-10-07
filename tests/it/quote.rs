use std::process::Command;

use sego::internal::quote::shell_word;

#[test]
fn bare_tokens() {
    let token = "github.com/gorilla/mux@98cb6bf42e08:mux.go";
    assert_eq!(shell_word(token), token);
}

#[test]
fn quoted_tokens() {
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
fn control_tokens() {
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
fn round_trips_through_bash() {
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
        let script =
            format!("shopt -s failglob; printf %s {}", shell_word(token));
        let output =
            Command::new("bash").arg("-c").arg(&script).output().unwrap();
        assert!(output.status.success(), "{script}");
        assert_eq!(output.stdout, token.as_bytes(), "{script}");
    }
}
