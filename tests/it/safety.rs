use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

use serde_json::Value;

use sego::internal::escape::is_unsafe;
use sego::internal::upstream::{Endpoint, Transport};
use sego::{Env, run};

use crate::support::fake::Fake;
use crate::support::fixture::load_all;

const CORPUS: [&str; 8] = [
    "esc\u{1b}[0m",
    "rtl\u{202e}x",
    "zw\u{200b}x",
    "new\nline",
    "cr\r",
    "nul\u{0}x",
    "ls\u{2028}x",
    "del\u{7f}x",
];
const MID_CR: &str = "mid\rcr";
const TAB: &str = "tab\tok";
const ADDRESS: &str = "github.com/hostile/repo";

struct Ran {
    code: ExitCode,
    out: String,
    err: String,
}

fn exec(fake: &Fake, words: &[&str]) -> Ran {
    let handle = fake.clone();
    let connect =
        move |_: Endpoint| -> Box<dyn Transport> { Box::new(handle.clone()) };
    let args: Vec<OsString> = words.iter().map(OsString::from).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code =
        run(args, &Env { endpoint: None }, &connect, &mut out, &mut err);
    Ran {
        code,
        out: String::from_utf8(out).unwrap_or_else(|e| panic!("{e}")),
        err: String::from_utf8(err).unwrap_or_else(|e| panic!("{e}")),
    }
}

fn hostile() -> Fake {
    Fake::fixtures(load_all(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hostile"),
    ))
}

/// The line break is the one unsafe character sego writes itself; the
/// line-count tests check where.
fn assert_safe(what: &str, text: &str) {
    if let Some(c) = text.chars().find(|&c| c != '\n' && is_unsafe(c)) {
        panic!("{what} holds U+{:04X}: {text:?}", u32::from(c));
    }
}

/// The lines the file's content holds, as `show` reports them.
fn content_lines() -> Vec<String> {
    let mut all: Vec<&str> = CORPUS.to_vec();
    all.extend([MID_CR, TAB]);
    all.join("\n").split('\n').map(str::to_owned).collect()
}

fn address(path: &str) -> String {
    format!("{ADDRESS}:{path}")
}

fn show(path: &str, json: bool) -> Ran {
    let target = address(path);
    let mut words = vec!["show"];
    if json {
        words.push("--json");
    }
    words.push(&target);
    exec(&hostile(), &words)
}

#[test]
fn the_fixtures_carry_the_corpus() {
    let fixtures = load_all(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hostile"),
    );
    let file = fixtures
        .iter()
        .find(|f| f.file.ends_with("file.yaml"))
        .unwrap_or_else(|| panic!("no file.yaml"));
    let response = file.response.as_ref().unwrap_or(&Value::Null);
    let content = response["data"]["r0"]["c0"]["body"]["content"]
        .as_str()
        .unwrap_or_default();
    let mut expected = CORPUS.to_vec();
    expected.extend([MID_CR, TAB]);
    assert_eq!(content, format!("{}\n", expected.join("\n")));
    let name = response["data"]["r0"]["name"].as_str().unwrap_or_default();
    assert!(name.contains('\u{0}') && name.contains('\u{1b}'));
}

#[test]
fn no_unsafe_character_reaches_any_output() {
    let paths =
        ["f.txt", "sub", "d", "gone.txt", "broken.txt", "rejected.txt"];
    for path in paths {
        for json in [false, true] {
            let ran = show(path, json);
            let what = format!("{path} json={json}");
            assert_safe(&format!("stdout of {what}"), &ran.out);
            assert_safe(&format!("stderr of {what}"), &ran.err);
        }
    }
}

#[test]
fn text_has_only_the_line_breaks_sego_writes() {
    let ran = show("f.txt", false);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.err);
    let lines = content_lines();
    let body = ran.out.strip_suffix('\n').unwrap_or(&ran.out);
    assert_eq!(body.split('\n').count(), 1 + lines.len(), "{:?}", ran.out);
    assert!(ran.out.ends_with('\n'));
    assert!(ran.err.is_empty(), "{:?}", ran.err);
    assert!(ran.out.contains("CRLF"), "{:?}", ran.out);

    let ran = show("sub", false);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.err);
    let body = ran.out.strip_suffix('\n').unwrap_or(&ran.out);
    assert_eq!(body.split('\n').count(), 1, "{:?}", ran.out);
}

#[test]
fn json_values_equal_the_hostile_originals() {
    let ran = show("f.txt", true);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.err);
    let json: Value = serde_json::from_str(&ran.out)
        .unwrap_or_else(|e| panic!("{e}: {:?}", ran.out));
    let expected_repo = format!(
        "github.com/hostile/repo{}",
        "\u{1b}[0m\u{202e}x\u{200b}x\nline\r\u{0}x\u{2028}x\u{7f}x"
    );
    assert_eq!(json["repo"], expected_repo);

    let mut lines = content_lines();
    let last = lines.len() - 1;
    let mut crlf = false;
    for (i, line) in lines.iter_mut().enumerate() {
        if i == last && line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_suffix('\r') {
            *line = rest.to_owned();
            crlf = true;
        }
    }
    let shown: Vec<&str> = json["lines"]
        .as_array()
        .unwrap_or_else(|| panic!("{:?}", ran.out))
        .iter()
        .map(|l| l["text"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(shown, lines);
    assert_eq!(json["crlf"], crlf);

    let ran = show("sub", true);
    let json: Value = serde_json::from_str(&ran.out)
        .unwrap_or_else(|e| panic!("{e}: {:?}", ran.out));
    assert_eq!(
        json["submodule"]["url"],
        "https://example.com/esc\u{1b}[0m\u{202e}x\u{200b}x\nline\r\u{0}x\
         \u{2028}x\u{7f}x"
    );
}

#[test]
fn error_paths_fail_with_a_clean_message_and_no_server_text() {
    for (path, code) in
        [("d", 1), ("gone.txt", 1), ("broken.txt", 1), ("rejected.txt", 1)]
    {
        let ran = show(path, false);
        assert_eq!(ran.code, ExitCode::from(code), "{path}: {}", ran.err);
        assert!(ran.out.is_empty(), "{path}: {:?}", ran.out);
        assert!(ran.err.starts_with("sego: error: "), "{:?}", ran.err);
        assert_eq!(ran.err.matches('\n').count(), 1, "{:?}", ran.err);
        if path != "rejected.txt" {
            assert!(!ran.err.contains("LEAK"), "{path}: {:?}", ran.err);
        }
    }
}
