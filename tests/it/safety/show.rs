use std::ffi::OsString;
use std::process::ExitCode;

use serde_json::Value;

use super::{
    CORPUS, ESCAPED_MESSAGE, REPO, Ran, assert_clean_error, assert_safe,
    fixtures, hostile, hostile_name, json, requested, sego,
};

const MID_CR: &str = "mid\rcr";
const TAB: &str = "tab\tok";

/// `show` of `REPO:path`, with the path passed as the one argument it is.
fn show(path: &str, json: bool) -> Ran {
    let mut args: Vec<OsString> = vec!["show".into()];
    if json {
        args.push("--json".into());
    }
    args.push(format!("{REPO}:{path}").into());
    sego(args)
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
    assert!(ran.err.contains(ESCAPED_MESSAGE), "{:?}", ran.err);
}
