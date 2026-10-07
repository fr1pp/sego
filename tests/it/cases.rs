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
        if failure.is_none() && case.exit == 2 && outcome.requests > 0 {
            failure = Some(format!(
                "{} request(s) reached the server on a usage error\n",
                outcome.requests
            ));
        }
        if let Some(failure) = failure {
            if blessing() {
                case.bless(&outcome);
            }
            let name = file.strip_prefix(&root).unwrap_or(file);
            failures.push(format!("--- {}\n{failure}", name.display()));
        }
    }
    assert!(
        failures.is_empty() || blessing(),
        "{} of {} cases failed\n\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
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
