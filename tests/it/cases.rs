use std::fs;
use std::path::{Path, PathBuf};

use crate::support::case::{Case, shell_words};
use crate::support::fixture::load_all;
use crate::support::replay::{Mode, Replay};

fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for entry in entries {
        let path = entry.unwrap_or_else(|e| panic!("{e}")).path();
        if path.is_dir() {
            collect(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "case") {
            files.push(path);
        }
    }
}

fn blessing() -> bool {
    std::env::var("SEGO_BLESS").is_ok_and(|v| !v.is_empty() && v != "0")
}

#[test]
fn cases() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut files = Vec::new();
    collect(&root.join("cases"), &mut files);
    files.sort();
    assert!(!files.is_empty(), "no case files under tests/cases");

    let replay =
        Replay::start(load_all(&root.join("fixtures")), Mode::from_env());
    let mut failures = Vec::new();
    for file in &files {
        let case = Case::load(file);
        let outcome = case.run(&replay);
        let mut failure = case.failure(&outcome);
        if failure.is_some() && blessing() {
            case.bless(&outcome);
        }
        if case.exit == 2 && outcome.requests > 0 {
            failure = Some(format!(
                "{} request(s) reached the server on a usage error\n",
                outcome.requests
            ));
        }
        if let Some(failure) = failure {
            let name = file.strip_prefix(&root).unwrap_or(file);
            failures.push(format!("--- {}\n{failure}", name.display()));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases failed\n\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}

/// A note, an error or a fact line ends without a period, as in rustc, cargo
/// and git. A message that starts in uppercase is Sourcegraph's own text,
/// quoted as sent.
#[test]
fn notes_and_facts_end_without_a_period() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cases");
    let mut files = Vec::new();
    collect(&root, &mut files);
    let mut found = Vec::new();
    for file in &files {
        for line in Case::load(file).stderr.lines() {
            let text = line
                .strip_prefix("sego: note: ")
                .or_else(|| line.strip_prefix("sego: error: "))
                .unwrap_or(line.trim_start());
            if text.starts_with(|c: char| c.is_ascii_uppercase()) {
                continue;
            }
            if text.ends_with('.') && !text.ends_with("...") {
                found.push(format!("{}: {line}", file.display()));
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}

/// Every note, error and fact sego writes comes from string literals in
/// src/, so no literal ends with a period, before any trailing newlines. The
/// exceptions are code tokens and the cache tag's file text.
#[test]
fn no_text_in_the_source_ends_with_a_period() {
    const CODE: &[&str] =
        &[".", "..", "+-.", "_-.", "www.", "has.", "contains."];
    let mut files = Vec::new();
    rust_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
    assert!(files.len() > 10, "{files:?}");
    let mut found = Vec::new();
    for file in &files {
        let source = fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        for literal in string_literals(&source) {
            let text = literal.trim_end_matches('\n');
            if text.ends_with('.')
                && !text.ends_with("...")
                && !CODE.contains(&text)
                && !text.starts_with("Signature: ")
            {
                found.push(format!("{}: {literal:?}", file.display()));
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}

#[test]
fn string_literals_are_found_in_source() {
    let source = r##"
        // "a comment."
        /* "a block comment." */
        let c = '"'; let e = '\''; let u = '\u{22}'; fn f<'a>(x: &'a str) {}
        let s = "one \"two\". \
                 three.";
        let r = r#"raw "quoted"."#;
        let b = br"bytes.";
        let n = b"\n";
    "##;
    assert_eq!(
        string_literals(source),
        ["one \"two\". three.", "raw \"quoted\".", "bytes.", "\n"]
    );
}

fn rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for entry in entries {
        let path = entry.unwrap_or_else(|e| panic!("{e}")).path();
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

/// The string literals in Rust source, with a `\` line continuation and
/// the common escapes decoded; comments, char literals and lifetimes are
/// skipped.
fn string_literals(source: &str) -> Vec<String> {
    let c: Vec<char> = source.chars().collect();
    let ident = |i: usize| c[i].is_alphanumeric() || c[i] == '_';
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let next = c.get(i + 1).copied();
        if c[i] == '/' && next == Some('/') {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
        } else if c[i] == '/' && next == Some('*') {
            i += 2;
            while i + 1 < c.len() && !(c[i] == '*' && c[i + 1] == '/') {
                i += 1;
            }
            i += 2;
        } else if c[i] == '\'' {
            if next == Some('\\') {
                i += 3;
                while i < c.len() && c[i] != '\'' {
                    i += 1;
                }
                i += 1;
            } else if c.get(i + 2) == Some(&'\'') {
                i += 3;
            } else {
                i += 1;
            }
        } else if c[i] == 'r'
            && (i == 0
                || !ident(i - 1)
                || (c[i - 1] == 'b' && (i == 1 || !ident(i - 2))))
            && matches!(next, Some('"' | '#'))
        {
            let hashes = c[i + 1..].iter().take_while(|&&h| h == '#').count();
            let open = i + 1 + hashes;
            if c.get(open) != Some(&'"') {
                i += 1;
                continue;
            }
            let mut end = open + 1;
            while end < c.len()
                && !(c[end] == '"'
                    && c[end + 1..]
                        .iter()
                        .take(hashes)
                        .filter(|&&h| h == '#')
                        .count()
                        == hashes)
            {
                end += 1;
            }
            out.push(c[open + 1..end].iter().collect());
            i = end + 1 + hashes;
        } else if c[i] == '"' {
            let mut text = String::new();
            i += 1;
            while i < c.len() && c[i] != '"' {
                if c[i] == '\\' {
                    i += 1;
                    match c.get(i) {
                        Some('\n') => {
                            while c
                                .get(i + 1)
                                .is_some_and(|w| w.is_whitespace())
                            {
                                i += 1;
                            }
                        }
                        Some('n') => text.push('\n'),
                        Some('t') => text.push('\t'),
                        Some(&other) => text.push(other),
                        None => {}
                    }
                } else {
                    text.push(c[i]);
                }
                i += 1;
            }
            out.push(text);
            i += 1;
        } else {
            i += 1;
        }
    }
    out
}

#[test]
fn shell_words_split_like_a_shell() {
    let words = |line| shell_words(line).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(words("show  a/b:c 3-4"), ["show", "a/b:c", "3-4"]);
    assert_eq!(words("'a b' \"c d\" e\\ f"), ["a b", "c d", "e f"]);
    assert_eq!(words(r#""a \"q\" \\ \$""#), [r#"a "q" \ \$"#]);
    assert_eq!(words(r"'a\b' x''y ''"), [r"a\b", "xy", ""]);
    assert_eq!(words(""), Vec::<String>::new());
    assert!(shell_words("'open").is_err());
    assert!(shell_words("\"open").is_err());
    assert!(shell_words("trailing\\").is_err());
}

#[test]
fn a_case_file_parses() {
    let text = "\
# a comment
args: show 'a b'
env: SEGO_ENDPOINT=http://x
env: K=V=W
exit: 2
stdout: closed
--- stdout
one
two
--- stderr
err
";
    let case = Case::parse(text, Path::new("x.case"))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(case.args, ["show", "a b"]);
    assert_eq!(
        case.env,
        [
            ("SEGO_ENDPOINT".to_owned(), "http://x".to_owned()),
            ("K".to_owned(), "V=W".to_owned())
        ]
    );
    assert_eq!(case.exit, 2);
    assert!(case.stdout_closed);
    assert_eq!(case.stdout, "one\ntwo\n");
    assert_eq!(case.stderr, "err\n");
}

#[test]
fn missing_sections_are_empty_and_bad_cases_are_refused() {
    let case = Case::parse("args: --version\nexit: 0\n", Path::new("x"))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((case.stdout.as_str(), case.stderr.as_str()), ("", ""));
    assert!(!case.stdout_closed);
    for bad in [
        "exit: 0\n",
        "args: x\n",
        "args: x\nexit: nope\n",
        "args: x\nexit: 0\nwhat: 1\n",
        "args: x\nexit: 0\n--- stdin\n",
        "args: x\nexit: 0\nenv: NOEQUALS\n",
        "args: x\nexit: 0\nstdout: open\n",
    ] {
        assert!(Case::parse(bad, Path::new("x")).is_err(), "{bad:?}");
    }
}

#[test]
fn only_exact_markers_start_a_section() {
    let text = "\
args: diff
exit: 0
--- stdout
--- a/x
+++ b/x
--- stderr
--- stdoutish
";
    let case = Case::parse(text, Path::new("x.case"))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(case.stdout, "--- a/x\n+++ b/x\n");
    assert_eq!(case.stderr, "--- stdoutish\n");
}

#[test]
fn the_case_runner_isolates_the_cache() {
    let case = Case::parse("args: --version\nexit: 0\n", Path::new("x"))
        .unwrap_or_else(|e| panic!("{e}"));
    let dir = Path::new("/tmp/sego-cache");
    let command = case.command("http://127.0.0.1:1", dir);
    let cache = command
        .get_envs()
        .find(|(name, _)| *name == "SEGO_CACHE_DIR")
        .and_then(|(_, value)| value);
    assert_eq!(cache, Some(dir.as_os_str()));
}
