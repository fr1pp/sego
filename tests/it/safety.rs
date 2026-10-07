use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;
use std::sync::OnceLock;

use serde_json::{Value, json};

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
/// The server's hostile message as an error line prints it.
const ESCAPED_MESSAGE: &str = "LEAK esc\\u{1b}[0m rtl\\u{202e}x zw\\u{200b}x \
                               new line cr\\u{d} nul\\u{0}x ls\\u{2028}x \
                               del\\u{7f}x";
/// `hostile()` as `one_line` prints it.
const ESCAPED: &str = "esc\\u{1b}[0mrtl\\u{202e}xzw\\u{200b}xnew \
                       linecr\\u{d}nul\\u{0}xls\\u{2028}xdel\\u{7f}x";
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

fn sego(args: Vec<OsString>) -> Ran {
    let fake = Fake::fixtures(fixtures());
    let connect =
        move |_: Endpoint| -> Box<dyn Transport> { Box::new(fake.clone()) };
    let ran = exec_os(args, None, &connect);
    Ran {
        code: ran.code,
        out: String::from_utf8(ran.out).unwrap_or_else(|e| panic!("{e}")),
        err: String::from_utf8(ran.err).unwrap_or_else(|e| panic!("{e}")),
    }
}

/// `show` of `REPO:path`, with the path passed as the one argument it is.
fn show(path: &str, json: bool) -> Ran {
    let mut args: Vec<OsString> = vec!["show".into()];
    if json {
        args.push("--json".into());
    }
    args.push(format!("{REPO}:{path}").into());
    sego(args)
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
    assert!(ran.err.contains(ESCAPED_MESSAGE), "{:?}", ran.err);
}

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
    assert!(lines[0].contains(" s, deadline "), "{:?}", lines[0]);
    assert!(lines[1].starts_with("query  "), "{:?}", lines[1]);
    let (mut headers, mut binary, mut code, mut symbols, mut commit) =
        (0, 0, 0, 0, 0);
    for (i, line) in lines.iter().enumerate().skip(2) {
        let filter_line = i == 2
            && (line.starts_with("narrow  ") || line.starts_with("widen  "));
        if filter_line || line.is_empty() || *line == "--" {
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
    assert_eq!(value["approximate_count"], h);
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

/// The two-line error for a repository Sourcegraph doesn't have.
fn assert_not_served(what: &str, ran: &Ran) {
    assert_eq!(ran.code, ExitCode::from(1), "{what}: {}", ran.err);
    assert!(ran.out.is_empty(), "{what}: {:?}", ran.out);
    assert_eq!(
        ran.err,
        format!(
            "sego: error: {REPO} is not on sourcegraph.com\n  \
             sourcegraph.com serves indexed repositories from github.com, \
             gitlab.com and a few other hosts; a renamed repository appears \
             only under its new name\n"
        ),
        "{what}"
    );
}

fn ls(path: &str, json: bool) -> Ran {
    let mut args: Vec<OsString> = vec!["ls".into()];
    if json {
        args.push("--json".into());
    }
    let address = if path.is_empty() {
        REPO.to_owned()
    } else {
        format!("{REPO}:{path}")
    };
    args.push(address.into());
    sego(args)
}

const LS_PATHS: [&str; 9] = [
    "",
    "HOSTILE",
    "sub",
    "f.txt",
    "bin.dat",
    "HOSTILE-gone",
    "absent",
    "broken.txt",
    "rejected.txt",
];
const LS_ERRORS: [&str; 5] =
    ["f.txt", "bin.dat", "HOSTILE-gone", "broken.txt", "rejected.txt"];
const LS_KINDS: [&str; 3] = ["file", "dir", "submodule"];
/// The hostile repository at `COMMIT`, as an address prints it: `$'…'`.
const QUOTED_ADDRESS: &str = "$'github.com/hostile/repoesc\\033[0mrtl\\342\\200\\256xzw\\342\\200\\213xnew\\nlinecr\\015nul\\000xls\\342\\200\\250xdel\\177x@0123456789ab'";
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

/// Header, then one `kind  path` row per entry, nothing else. `kinds` counts
/// the rows of each of `LS_KINDS`.
fn assert_ls_layout(ran: &Ran, kinds: [usize; 3]) {
    assert!(ran.out.ends_with('\n'), "{:?}", ran.out);
    let body = ran.out.strip_suffix('\n').unwrap_or(&ran.out);
    let lines: Vec<&str> = body.split('\n').collect();
    let header = lines[0];
    assert!(header.starts_with("$'github.com/hostile/repo"), "{header:?}");
    assert!(header.contains("@0123456789ab"), "{header:?}");
    let mut seen = [0; 3];
    for (i, line) in lines.iter().enumerate().skip(1) {
        let kind = LS_KINDS.iter().position(|kind| {
            line.strip_prefix(&format!("{kind:<9}  "))
                .is_some_and(|rest| !rest.is_empty() && !rest.starts_with(' '))
        });
        match kind {
            Some(kind) => seen[kind] += 1,
            None => panic!("line {i} is not a row: {line:?}"),
        }
    }
    assert_eq!(seen, kinds, "{:?}", ran.out);
    assert_eq!(lines.len(), 1 + kinds.iter().sum::<usize>(), "{:?}", ran.out);
}

#[test]
fn the_ls_and_refs_fixtures_carry_the_corpus() {
    let fixtures = fixtures();
    let response = |name: &str| {
        let fixture = fixtures
            .iter()
            .find(|f| f.file.ends_with(name))
            .unwrap_or_else(|| panic!("no {name}"));
        fixture.response.clone().unwrap_or(Value::Null)
    };
    let root = response("ls-root.yaml");
    let c0 = &root["data"]["r0"]["c0"];
    assert_eq!(
        root["data"]["r0"]["name"].as_str(),
        Some(hostile_name().as_str())
    );
    let entries = &c0["tree"]["entries"];
    assert_eq!(entries[0]["path"].as_str(), Some(hostile().as_str()));
    assert_eq!(
        entries[1]["path"].as_str(),
        Some(format!("{}-dir", hostile()).as_str())
    );
    assert_eq!(
        entries[2]["submodule"]["url"].as_str(),
        Some(format!("https://example.com/{}", hostile()).as_str())
    );
    assert_eq!(
        entries[3]["submodule"]["commit"].as_str(),
        Some(hostile().as_str())
    );
    let listed = response("ls-path.yaml");
    let entries = &listed["data"]["r0"]["c0"]["tree"]["entries"];
    assert_eq!(
        entries[0]["path"].as_str(),
        Some(format!("{}/{}", hostile(), hostile()).as_str())
    );

    let refs = response("refs-text.yaml");
    let r0 = &refs["data"]["r0"];
    assert_eq!(r0["name"].as_str(), Some(hostile_name().as_str()));
    assert_eq!(
        r0["defaultBranch"]["displayName"].as_str(),
        Some(hostile().as_str())
    );
    assert_eq!(
        r0["branches"]["nodes"][1]["displayName"].as_str(),
        Some(format!("b-{}", hostile()).as_str())
    );
    assert_eq!(
        r0["branches"]["nodes"][1]["target"]["oid"].as_str(),
        Some(hostile().as_str())
    );
    assert_eq!(
        r0["tags"]["nodes"][0]["target"]["commit"]["committer"]["date"]
            .as_str(),
        Some(hostile().as_str())
    );
    assert_eq!(
        r0["tags"]["nodes"][2]["displayName"].as_str(),
        Some(format!("u-{}", hostile()).as_str())
    );
    let matched = fixtures
        .iter()
        .find(|f| f.file.ends_with("refs-match.yaml"))
        .and_then(|f| f.variables.as_ref())
        .and_then(|v| v["query"].as_str().map(str::to_owned));
    assert_eq!(matched, Some(hostile()));
}

#[test]
fn the_hostile_fixture_files_hold_no_raw_unsafe_character() {
    let dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hostile");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{e}")) {
        let path = entry.unwrap_or_else(|e| panic!("{e}")).path();
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_safe(&path.display().to_string(), &text);
        checked += 1;
    }
    assert!(checked >= 20, "{checked} fixture files");
}

#[test]
fn no_unsafe_character_reaches_any_ls_output() {
    for path in LS_PATHS {
        for json in [false, true] {
            let ran = ls(&requested(path), json);
            let what = format!("ls {path:?} json={json}");
            assert_safe(&format!("stdout of {what}"), &ran.out);
            assert_safe(&format!("stderr of {what}"), &ran.err);
        }
    }
}

#[test]
fn ls_text_has_only_the_layout_lines() {
    let ran = ls("", false);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.err);
    assert!(ran.err.is_empty(), "{:?}", ran.err);
    assert_ls_layout(&ran, [1, 1, 2]);
    assert!(
        ran.out.lines().next().is_some_and(|header| header
            .ends_with("  4 entries  1 dir  1 file  2 submodules")),
        "{:?}",
        ran.out
    );
    assert!(
        ran.out.contains("\nsubmodule  mod  example.com/hostile/lib@esc\\u{1b}[0mrtl\\u{202e}x\n"),
        "{:?}",
        ran.out
    );
    assert!(
        ran.out.contains(&format!("https://example.com/{ESCAPED}\n")),
        "{:?}",
        ran.out
    );

    let ran = ls(&hostile(), false);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.err);
    assert_ls_layout(&ran, [1, 1, 1]);

    let ran = ls("sub", false);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.err);
    assert_ls_layout(&ran, [0, 0, 1]);
}

#[test]
fn ls_json_values_equal_the_hostile_originals() {
    let ran = ls("", true);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.err);
    let value = json(&ran);
    assert_eq!(value["repo"], hostile_name());
    assert_eq!(value["path"], Value::Null);
    let entries = value["entries"].as_array().expect("entries");
    let paths: Vec<&str> =
        entries.iter().map(|e| e["path"].as_str().unwrap_or("")).collect();
    let dir = format!("{}-dir", hostile());
    let file = hostile();
    let expected = [file.as_str(), dir.as_str(), "mod", "sub"];
    for path in expected {
        assert!(paths.contains(&path), "{path:?} not in {paths:?}");
    }
    assert_eq!(paths.len(), expected.len(), "{paths:?}");
    let entry = |path: &str| {
        entries
            .iter()
            .find(|e| e["path"] == path)
            .unwrap_or_else(|| panic!("no {path:?}"))
    };
    assert_eq!(entry("sub")["kind"], "submodule");
    assert_eq!(
        entry("sub")["submodule_url"],
        format!("https://example.com/{}", hostile())
    );
    assert_eq!(entry("sub")["submodule_target"], Value::Null);
    assert_eq!(entry("mod")["submodule_commit"], hostile());
    assert_eq!(
        entry("mod")["submodule_target"],
        format!(
            "example.com/hostile/lib@{}",
            hostile().chars().take(12).collect::<String>()
        )
    );
    assert_eq!(entry(&dir)["kind"], "dir");
    assert_eq!(entry(&hostile())["kind"], "file");

    let value = json(&ls(&hostile(), true));
    assert_eq!(value["path"], hostile());
    let paths: Vec<&str> = value["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|e| e["path"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(paths.len(), 3, "{paths:?}");
    assert!(paths.contains(&format!("{0}/{0}", hostile()).as_str()));
    assert!(paths.contains(&format!("{0}/{0}-dir", hostile()).as_str()));
    assert!(paths.contains(&format!("{}/sub", hostile()).as_str()));

    let value = json(&ls("sub", true));
    assert_eq!(
        value["entries"][0]["submodule_url"],
        format!("https://example.com/{}", hostile())
    );
    assert_eq!(value["entries"][0]["submodule_commit"], COMMIT);
}

#[test]
fn ls_error_paths_fail_with_a_clean_message_and_no_server_text() {
    for path in LS_ERRORS {
        for as_json in [false, true] {
            let ran = ls(&requested(path), as_json);
            let what = format!("ls {path} json={as_json}");
            assert_clean_error(&what, &ran, 1);
            if path != "rejected.txt" {
                assert!(!ran.err.contains("LEAK"), "{what}: {:?}", ran.err);
            }
        }
    }
    let ran = ls("f.txt", false);
    let at = QUOTED_ADDRESS;
    assert_eq!(
        ran.err,
        format!("sego: error: f.txt is a file at {at} (3 lines)\n")
    );
    let ran = ls("bin.dat", false);
    assert_eq!(
        ran.err,
        format!(
            "sego: error: bin.dat is a file at {at} (binary, 2048 bytes)\n"
        )
    );
    for as_json in [false, true] {
        assert_not_served("ls absent", &ls("absent", as_json));
    }
    let ran = ls(&requested("HOSTILE-gone"), false);
    assert_eq!(
        ran.err,
        format!("sego: error: {ESCAPED}-gone not found in {at}\n")
    );
    let ran = ls("broken.txt", false);
    assert_eq!(
        ran.err,
        "sego: error: Sourcegraph failed to answer the directory listing\n"
    );
    let ran = ls("rejected.txt", false);
    assert_eq!(
        ran.err,
        format!(
            "sego: error: Sourcegraph rejected sego's request: \
             {ESCAPED_MESSAGE}\n"
        )
    );
}

fn refs(text: Option<&str>, json: bool) -> Ran {
    let mut args: Vec<OsString> = vec!["refs".into()];
    if json {
        args.push("--json".into());
    }
    args.push(REPO.into());
    if let Some(text) = text {
        args.push(text.into());
    }
    sego(args)
}

const REF_QUERIES: [Option<&str>; 6] = [
    None,
    Some("HOSTILE"),
    Some("broken"),
    Some("nobranches"),
    Some("absent"),
    Some("refused"),
];

fn refs_query(query: Option<&str>) -> Option<String> {
    query.map(requested)
}

/// Header, `branch` rows, then `tag` rows. `ESCAPED` stands for each hostile
/// name; the hostile commit and date are written out.
fn refs_text(counts: &str) -> String {
    let short = "esc\\u{1b}[0mrtl\\u{202e}x";
    let date = "esc\\u{1b}[0mrtl";
    format!(
        "{REPO}{ESCAPED}  {counts}\n\
         branch  0123456789ab  2026-03-01  {ESCAPED}  (default)\n\
         branch  {short}  {date}  b-{ESCAPED}\n\
         tag     0123456789ab  {date}  t-{ESCAPED}\n\
         tag     0123456789ab  2026-02-01  {ESCAPED}\n\
         tag     -             -           u-{ESCAPED}\n"
    )
}

/// Every stdout line of `refs` is the header or a `branch` or `tag` row, in
/// that order.
fn assert_refs_layout(out: &str, branches: usize, tags: usize) {
    assert!(out.ends_with('\n'), "{out:?}");
    let lines: Vec<&str> =
        out.strip_suffix('\n').unwrap_or(out).split('\n').collect();
    assert!(lines[0].starts_with(REPO), "{:?}", lines[0]);
    assert_eq!(lines.len(), 1 + branches + tags, "{out:?}");
    for (i, line) in lines.iter().enumerate().skip(1) {
        let kind = if i <= branches { "branch  " } else { "tag     " };
        let rest = line.strip_prefix(kind);
        assert!(
            rest.is_some_and(|rest| !rest.is_empty() && !rest.starts_with(' ')),
            "line {i}: {line:?}"
        );
    }
}

#[test]
fn no_unsafe_character_reaches_any_refs_output() {
    for query in REF_QUERIES {
        for json in [false, true] {
            let ran = refs(refs_query(query).as_deref(), json);
            let what = format!("refs {query:?} json={json}");
            assert_safe(&format!("stdout of {what}"), &ran.out);
            assert_safe(&format!("stderr of {what}"), &ran.err);
        }
    }
}

#[test]
fn refs_text_has_only_the_layout_lines() {
    let ran = refs(None, false);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.err);
    assert!(ran.err.is_empty(), "{:?}", ran.err);
    assert_refs_layout(&ran.out, 2, 3);
    assert_eq!(ran.out, refs_text("2 branches  3 tags"));

    let ran = refs(Some(&hostile()), false);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.err);
    assert!(ran.err.is_empty(), "{:?}", ran.err);
    assert_refs_layout(&ran.out, 2, 3);
    assert_eq!(
        ran.out,
        refs_text(&format!(
            "2 of 9 branches match \"{ESCAPED}\"  \
             3 of 12 tags match \"{ESCAPED}\""
        ))
    );
}

#[test]
fn refs_json_values_equal_the_hostile_originals() {
    for query in [None, Some(hostile())] {
        let ran = refs(query.as_deref(), true);
        assert_eq!(ran.code, ExitCode::SUCCESS, "{}", ran.err);
        assert!(ran.err.is_empty(), "{:?}", ran.err);
        let value = json(&ran);
        let h = hostile();
        assert_eq!(value["repo"], hostile_name());
        assert_eq!(value["query"], json!(query));
        assert_eq!(value["default_branch"], h);
        let (branches_all, tags_all) = match query {
            Some(_) => (json!(9), json!(12)),
            None => (Value::Null, Value::Null),
        };
        assert_eq!(value["branches_all"], branches_all);
        assert_eq!(value["tags_all"], tags_all);
        let kinds: Vec<&Value> =
            value["refs"].as_array().expect("refs").iter().collect();
        let field = |name: &str| -> Vec<Value> {
            kinds.iter().map(|r| r[name].clone()).collect()
        };
        assert_eq!(
            field("kind"),
            ["branch", "branch", "tag", "tag", "tag"].map(Value::from)
        );
        assert_eq!(
            field("name"),
            [
                h.clone(),
                format!("b-{h}"),
                format!("t-{h}"),
                h.clone(),
                format!("u-{h}")
            ]
            .map(Value::from)
        );
        assert_eq!(
            field("commit"),
            [
                json!(COMMIT),
                json!(h),
                json!(COMMIT),
                json!(COMMIT),
                Value::Null
            ]
        );
        let date: String = h.chars().take(10).collect();
        assert_eq!(
            field("date"),
            [
                json!("2026-03-01"),
                json!(date),
                json!(date),
                json!("2026-02-01"),
                Value::Null
            ]
        );
    }
}

#[test]
fn refs_error_paths_fail_with_a_clean_message() {
    let rejected = format!(
        "sego: error: Sourcegraph rejected sego's request: \
         {ESCAPED_MESSAGE}\n"
    );
    let cases = [
        ("broken", "sego: error: Sourcegraph failed to answer the tag list\n"),
        (
            "nobranches",
            "sego: error: Sourcegraph failed to answer the branch list\n",
        ),
        ("refused", rejected.as_str()),
    ];
    for (query, message) in cases {
        for as_json in [false, true] {
            let ran = refs(Some(query), as_json);
            let what = format!("refs {query} json={as_json}");
            assert_clean_error(&what, &ran, 1);
            assert_eq!(ran.err, message, "{what}");
        }
    }
    for as_json in [false, true] {
        assert_not_served("refs absent", &refs(Some("absent"), as_json));
    }
}
