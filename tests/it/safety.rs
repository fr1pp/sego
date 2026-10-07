use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;
use std::sync::OnceLock;

use serde_json::Value;

use sego::internal::escape::is_unsafe;
use sego::internal::upstream::{Endpoint, Transport};

use crate::support::exec::exec_os;
use crate::support::fake::Fake;
use crate::support::fixture::{Fixture, load_all};

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
const REPO: &str = "github.com/hostile/repo";

struct Ran {
    code: ExitCode,
    out: String,
    err: String,
}

fn fixtures() -> Vec<Fixture> {
    static LOADED: OnceLock<Vec<Fixture>> = OnceLock::new();
    LOADED
        .get_or_init(|| {
            load_all(
                &Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/hostile"),
            )
        })
        .clone()
}

fn hostile() -> String {
    CORPUS.concat()
}

fn hostile_name() -> String {
    format!("{REPO}{}", hostile())
}

/// `show` of `REPO:path`, with the path passed as the one argument it is.
fn show(path: &str, json: bool) -> Ran {
    let fake = Fake::fixtures(fixtures());
    let connect =
        move |_: Endpoint| -> Box<dyn Transport> { Box::new(fake.clone()) };
    let mut args: Vec<OsString> = vec!["show".into()];
    if json {
        args.push("--json".into());
    }
    args.push(format!("{REPO}:{path}").into());
    let ran = exec_os(args, None, &connect);
    Ran {
        code: ran.code,
        out: String::from_utf8(ran.out).unwrap_or_else(|e| panic!("{e}")),
        err: String::from_utf8(ran.err).unwrap_or_else(|e| panic!("{e}")),
    }
}

/// The line break is the one unsafe character sego writes itself; the
/// line tests check where.
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

fn gutter_line(line: &str) -> bool {
    line.split_once(':').is_some_and(|(gutter, _)| {
        let digits = gutter.trim_start_matches(' ');
        !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
    })
}

/// Header, then one gutter line per code line, nothing else.
fn assert_text_layout(ran: &Ran, code_lines: usize) {
    let body = ran.out.strip_suffix('\n').unwrap_or(&ran.out);
    let lines: Vec<&str> = body.split('\n').collect();
    assert!(ran.out.ends_with('\n'), "{:?}", ran.out);
    assert_eq!(lines.len(), 1 + code_lines, "{:?}", ran.out);
    for (i, line) in lines[1..].iter().enumerate() {
        assert!(gutter_line(line), "line {}: {line:?}", i + 1);
    }
}

fn assert_clean_error(what: &str, ran: &Ran, code: u8) {
    assert_eq!(ran.code, ExitCode::from(code), "{what}: {}", ran.err);
    assert!(ran.out.is_empty(), "{what}: {:?}", ran.out);
    assert!(ran.err.starts_with("sego: error: "), "{what}: {:?}", ran.err);
    assert!(ran.err.ends_with('\n'), "{what}: {:?}", ran.err);
    assert_eq!(ran.err.matches('\n').count(), 1, "{what}: {:?}", ran.err);
}

fn json(ran: &Ran) -> Value {
    serde_json::from_str(&ran.out)
        .unwrap_or_else(|e| panic!("{e}: {:?}", ran.out))
}

#[test]
fn the_fixtures_carry_the_corpus() {
    let fixtures = fixtures();
    let response = |name: &str| {
        let fixture = fixtures
            .iter()
            .find(|f| f.file.ends_with(name))
            .unwrap_or_else(|| panic!("no {name}"));
        fixture.response.clone().unwrap_or(Value::Null)
    };
    let file = response("file.yaml");
    let c0 = &file["data"]["r0"]["c0"];
    let mut all = CORPUS.to_vec();
    all.extend([MID_CR, TAB]);
    assert_eq!(
        c0["body"]["content"].as_str(),
        Some(format!("{}\n", all.join("\n")).as_str())
    );
    assert_eq!(
        file["data"]["r0"]["name"].as_str(),
        Some(hostile_name().as_str())
    );
    let sub = response("submodule.yaml");
    assert_eq!(
        sub["data"]["r0"]["c0"]["parent"]["entries"][0]["submodule"]["url"]
            .as_str(),
        Some(format!("https://example.com/{}", hostile()).as_str())
    );
    let hostile_path = |name: &str| {
        fixtures
            .iter()
            .find(|f| f.file.ends_with(name))
            .and_then(|f| f.variables.as_ref())
            .and_then(|v| v["path_0"].as_str().map(str::to_owned))
    };
    assert_eq!(hostile_path("path-found.yaml"), Some(hostile()));
    assert_eq!(
        hostile_path("path-missing.yaml"),
        Some(format!("{}-gone", hostile()))
    );
}

const PATHS: [&str; 8] = [
    "f.txt",
    "sub",
    "d",
    "gone.txt",
    "broken.txt",
    "rejected.txt",
    "HOSTILE",
    "HOSTILE-gone",
];

fn requested(path: &str) -> String {
    path.replace("HOSTILE", &hostile())
}

#[test]
fn no_unsafe_character_reaches_any_output() {
    for path in PATHS {
        for json in [false, true] {
            let ran = show(&requested(path), json);
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
    assert_text_layout(&ran, content_lines().len());
    assert!(ran.err.is_empty(), "{:?}", ran.err);
    assert!(ran.out.contains("CRLF"), "{:?}", ran.out);

    let ran = show("sub", false);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.err);
    assert_text_layout(&ran, 0);

    let ran = show(&hostile(), false);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.err);
    assert_text_layout(&ran, 1);
}

#[test]
fn json_values_equal_the_hostile_originals() {
    let ran = show("f.txt", true);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.err);
    let value = json(&ran);
    assert_eq!(value["repo"], hostile_name());

    let mut lines = content_lines();
    let mut crlf = false;
    for line in &mut lines {
        if let Some(rest) = line.strip_suffix('\r') {
            *line = rest.to_owned();
            crlf = true;
        }
    }
    let shown: Vec<&str> = value["lines"]
        .as_array()
        .unwrap_or_else(|| panic!("{:?}", ran.out))
        .iter()
        .map(|l| l["text"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(shown, lines);
    assert_eq!(value["crlf"], crlf);

    let value = json(&show("sub", true));
    assert_eq!(
        value["submodule"]["url"],
        format!("https://example.com/{}", hostile())
    );

    let value = json(&show(&hostile(), true));
    assert_eq!(value["path"], hostile());
}

#[test]
fn error_paths_fail_with_a_clean_message_and_no_server_text() {
    let cases = [
        ("d", 1),
        ("gone.txt", 1),
        ("broken.txt", 1),
        ("rejected.txt", 1),
        ("HOSTILE-gone", 1),
    ];
    for (path, code) in cases {
        for as_json in [false, true] {
            let ran = show(&requested(path), as_json);
            let what = format!("{path} json={as_json}");
            assert_clean_error(&what, &ran, code);
            if path != "rejected.txt" {
                assert!(!ran.err.contains("LEAK"), "{what}: {:?}", ran.err);
            }
        }
    }
}

#[test]
fn a_rejected_request_prints_the_server_message_escaped() {
    let ran = show("rejected.txt", false);
    let escaped = "LEAK esc\\u{1b}[0m rtl\\u{202e}x zw\\u{200b}x new line \
                   cr\\u{d} nul\\u{0}x ls\\u{2028}x del\\u{7f}x";
    assert!(ran.err.contains(escaped), "{:?}", ran.err);
}

fn search(tag: &str, json: bool) -> Ran {
    let fake = Fake::fixtures(fixtures());
    let connect =
        move |_: Endpoint| -> Box<dyn Transport> { Box::new(fake.clone()) };
    let mut args: Vec<OsString> = vec!["search".into()];
    if json {
        args.push("--json".into());
    }
    args.push(format!("{} {tag}", hostile()).into());
    let ran = exec_os(args, None, &connect);
    Ran {
        code: ran.code,
        out: String::from_utf8(ran.out).unwrap_or_else(|e| panic!("{e}")),
        err: String::from_utf8(ran.err).unwrap_or_else(|e| panic!("{e}")),
    }
}

const SEARCH_TAGS: [&str; 5] =
    ["ok", "stopped", "norepos", "revisions", "unable"];
const SEARCH_ERRORS: [&str; 4] = ["stopped", "norepos", "revisions", "unable"];

fn content_text() -> Vec<&'static str> {
    vec![
        "esc\u{1b}[0m",
        "rtl\u{202e}x",
        "zw\u{200b}x",
        "new",
        "line",
        "cr",
        "ls\u{2028}x",
        "del\u{7f}x",
        "mid\rcr",
        "tab\tok",
    ]
}

/// A code line is a gutter, then `:` for a match or `-` for context.
fn search_code_line(line: &str) -> bool {
    let rest = line.trim_start_matches(' ');
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0 && matches!(rest.as_bytes().get(digits), Some(b':' | b'-'))
}

/// Every stdout line of a search is one of the shapes the layout allows.
fn assert_search_layout(out: &str) {
    assert!(out.ends_with('\n'), "{out:?}");
    let lines: Vec<&str> =
        out.strip_suffix('\n').unwrap_or(out).split('\n').collect();
    assert!(lines[0].contains(" s, deadline "), "{:?}", lines[0]);
    assert!(lines[1].starts_with("query  "), "{:?}", lines[1]);
    let mut headers = 0;
    let mut binary = 0;
    for (i, line) in lines.iter().enumerate().skip(2) {
        if i == 2
            && (line.starts_with("narrow  ") || line.starts_with("widen  "))
        {
            continue;
        }
        if line.is_empty() || *line == "--" || search_code_line(line) {
            continue;
        }
        if *line == "binary file; lines not shown" {
            binary += 1;
        } else if line.starts_with("    ") {
        } else if line.starts_with(REPO) || line.starts_with("$'") {
            headers += 1;
        } else {
            panic!("line {i} is not part of the layout: {line:?}");
        }
    }
    assert_eq!(headers, 4, "{out:?}");
    assert_eq!(binary, 1, "{out:?}");
    assert_eq!(lines.iter().filter(|l| l.is_empty()).count(), 1, "{out:?}");
}

fn assert_search_stderr(ran: &Ran) {
    assert!(ran.err.ends_with('\n'), "{:?}", ran.err);
    for line in ran.err.lines() {
        assert!(
            line.starts_with("sego: note: ")
                || line.starts_with("sego: error: ")
                || line.starts_with("  "),
            "{line:?}"
        );
    }
}

#[test]
fn no_unsafe_character_reaches_any_search_output() {
    for tag in SEARCH_TAGS {
        for json in [false, true] {
            let ran = search(tag, json);
            let what = format!("{tag} json={json}");
            assert_safe(&format!("stdout of {what}"), &ran.out);
            assert_safe(&format!("stderr of {what}"), &ran.err);
        }
    }
}

#[test]
fn search_text_has_only_the_layout_lines() {
    let ran = search("ok", false);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.err);
    assert_search_layout(&ran.out);
    assert_search_stderr(&ran);
    assert_eq!(ran.err.lines().count(), 7, "{:?}", ran.err);
}

#[test]
fn search_json_values_equal_the_hostile_originals() {
    let ran = search("ok", true);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.err);
    assert_search_stderr(&ran);
    let h = hostile();
    let value = json(&ran);
    assert_eq!(value["query"], format!("{h} ok count:100"));
    let name = hostile_name();
    assert_eq!(value["timedout"][0], h);
    assert_eq!(value["cloning"][0], h);
    assert_eq!(value["missing"][0], h);
    assert_eq!(value["alert"]["title"], h);
    assert_eq!(value["alert"]["description"], h);
    assert_eq!(value["alert"]["proposed"][0]["description"], h);
    assert_eq!(value["alert"]["proposed"][0]["query"], h);
    assert_eq!(value["filters"][0]["value"], h);
    assert_eq!(value["filters"][0]["label"], h);
    assert_eq!(value["filters"][1]["value"], h);

    let results = value["results"].as_array().expect("results array");
    let of = |kind: &str| -> Vec<&Value> {
        results.iter().filter(|r| r["kind"] == kind).collect()
    };
    let lines = of("line");
    let text: Vec<&str> =
        lines.iter().map(|l| l["text"].as_str().unwrap_or_default()).collect();
    assert_eq!(text, content_text());
    for line in &lines {
        assert_eq!(line["repo"], name);
        assert_eq!(line["path"], h);
        assert_eq!(line["rev"], h);
    }
    let symbols = of("symbol");
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0]["name"], h);
    assert_eq!(symbols[0]["container"], h);
    assert_eq!(symbols[0]["path"], h);
    let binary = of("binary");
    assert_eq!(binary.len(), 1);
    assert_eq!(binary[0]["path"], format!("{h}.bin"));
    let commits = of("commit");
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0]["subject"], h);
    assert_eq!(commits[0]["author"], h);
    assert_eq!(commits[0]["repo"], name);
    let commit_lines: Vec<&str> = commits[0]["lines"]
        .as_array()
        .expect("lines")
        .iter()
        .map(|l| l.as_str().unwrap_or_default())
        .collect();
    assert_eq!(commit_lines, content_text());
    let repos = of("repo");
    assert_eq!(repos.len(), 1);
    assert_eq!(repos[0]["repo"], name);
}

#[test]
fn search_error_paths_fail_with_a_clean_message() {
    for tag in SEARCH_ERRORS {
        for as_json in [false, true] {
            let ran = search(tag, as_json);
            let what = format!("{tag} json={as_json}");
            assert_eq!(ran.code, ExitCode::from(1), "{what}: {}", ran.err);
            assert!(ran.out.is_empty(), "{what}: {:?}", ran.out);
            assert!(
                ran.err.starts_with("sego: error: "),
                "{what}: {:?}",
                ran.err
            );
            assert_search_stderr(&ran);
            let lines = ran.err.lines().count();
            let expected = match tag {
                "unable" => 1,
                "stopped" => 2,
                _ => 3,
            };
            assert_eq!(lines, expected, "{what}: {:?}", ran.err);
        }
    }
}
