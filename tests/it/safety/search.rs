use std::ffi::OsString;
use std::process::ExitCode;

use serde_json::Value;

use super::{REPO, Ran, assert_safe, hostile, hostile_name, json, sego};

fn search(tag: &str, json: bool) -> Ran {
    let mut args: Vec<OsString> = vec!["search".into()];
    if json {
        args.push("--json".into());
    }
    args.push(format!("{} {tag}", hostile()).into());
    sego(args)
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
    assert!(lines[0].contains(" matches  "), "{:?}", lines[0]);
    assert!(lines[0].ends_with(" s"), "{:?}", lines[0]);
    assert!(lines[1].starts_with("query  "), "{:?}", lines[1]);
    let (mut headers, mut binary, mut code, mut symbols, mut commit) =
        (0, 0, 0, 0, 0);
    for (i, line) in lines.iter().enumerate().skip(2) {
        if line.is_empty() || *line == "--" {
            continue;
        }
        if search_code_line(line) {
            if line
                .trim_start_matches(|c: char| c == ' ' || c.is_ascii_digit())
                .starts_with(":function  ")
            {
                symbols += 1;
            } else {
                code += 1;
            }
        } else if *line == "binary file; lines not shown" {
            binary += 1;
        } else if line.starts_with("    ") {
            commit += 1;
        } else if line.starts_with(REPO) || line.starts_with("$'") {
            headers += 1;
        } else {
            panic!("line {i} is not part of the layout: {line:?}");
        }
    }
    assert_eq!(headers, 4, "{out:?}");
    assert_eq!(binary, 1, "{out:?}");
    assert_eq!(code, content_text().len(), "{out:?}");
    assert_eq!(symbols, 1, "{out:?}");
    assert_eq!(commit, content_text().len(), "{out:?}");
    assert!(lines[2].is_empty(), "{out:?}");
    for (i, line) in lines.iter().enumerate().skip(2) {
        if line.is_empty() {
            let next = lines.get(i + 1).copied().unwrap_or_default();
            assert!(
                next.starts_with(REPO) || next.starts_with("$'"),
                "a blank line at {i} is not followed by a header: {out:?}"
            );
        }
    }
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
    assert_eq!(ran.err.lines().count(), 3, "{:?}", ran.err);
}

#[test]
fn search_json_values_equal_the_hostile_originals() {
    let ran = search("ok", true);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.err);
    assert_search_stderr(&ran);
    let h = hostile();
    let value = json(&ran);
    assert_eq!(value["query"], format!("{h} ok"));
    let name = hostile_name();
    assert_eq!(value["server"]["timedout"][0], h);
    assert_eq!(value["server"]["approximate_count"], h);
    assert_eq!(value["alert"]["title"], h);
    assert_eq!(value["alert"]["description"], h);
    assert_eq!(value["alert"]["proposed"][0]["description"], h);
    assert_eq!(value["alert"]["proposed"][0]["query"], h);
    assert_eq!(value["widen"][0]["filter"], h);

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
                "unable" | "stopped" => 1,
                _ => 3,
            };
            assert_eq!(lines, expected, "{what}: {:?}", ran.err);
        }
    }
}
