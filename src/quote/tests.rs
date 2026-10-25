use std::process::Command;

use super::shell_word;

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
        let script =
            format!("shopt -s failglob; printf %s {}", shell_word(token));
        let output =
            Command::new("bash").arg("-c").arg(&script).output().unwrap();
        assert!(output.status.success(), "{script}");
        assert_eq!(output.stdout, token.as_bytes(), "{script}");
    }
}
