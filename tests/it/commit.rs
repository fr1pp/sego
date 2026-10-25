use std::path::Path;
use std::process::ExitCode;

use serde_json::{Value, json};

use sego::{Endpoint, Reply, Transport};

use crate::support::exec::{Ran, exec};
use crate::support::fake::Fake;
use crate::support::fixture::load_all;

const REPO: &str = "example.com/selftest/commit";
const OID: &str = "affe2b9b7c608f4e354d7e83d0583ed35e845650";
const O12: &str = "affe2b9b7c60";
const PARENT: &str = "6b668aa526afdc623c1a9ed1d6ae920e04a717ad";
const P12: &str = "6b668aa526af";
const SECOND: &str = "7456b4e9ae7b67e27ffaa2e497941e77d39bc884";
const ROOT: &str = "4e8d10dc715feb79b43f1d607b45148537546a0a";

fn fixtures() -> Fake {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ShowCommit");
    Fake::fixtures(load_all(&root))
}

fn run_on(fake: &Fake, words: &[&str]) -> Ran {
    let handle = fake.clone();
    let connect =
        move |_: Endpoint| -> Box<dyn Transport> { Box::new(handle.clone()) };
    let mut argv = vec!["show"];
    argv.extend_from_slice(words);
    exec(&argv, None, &connect)
}

fn show(words: &[&str]) -> (Ran, Fake) {
    let fake = fixtures();
    let ran = run_on(&fake, words);
    (ran, fake)
}

fn out(ran: &Ran) -> String {
    String::from_utf8_lossy(&ran.out).into_owned()
}

fn err(ran: &Ran) -> String {
    String::from_utf8_lossy(&ran.err).into_owned()
}

fn json_out(ran: &Ran) -> Value {
    serde_json::from_slice(&ran.out).unwrap_or_else(|e| panic!("{e}"))
}

fn scripted(body: &Value) -> Fake {
    Fake::scripted(vec![Ok(Reply {
        status: 200,
        content_type: Some("application/json".to_owned()),
        location: None,
        retry_after: None,
        cf_mitigated: None,
        body: body.to_string().into_bytes(),
    })])
}

fn sent(fake: &Fake) -> Value {
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    serde_json::from_slice(&calls[0].body).unwrap_or_else(|e| panic!("{e}"))
}

fn failed(ran: &Ran, message: &str) {
    assert_eq!(ran.code, ExitCode::from(1), "{}", err(ran));
    assert_eq!(ran.out, b"");
    assert_eq!(err(ran), format!("sego: error: {message}\n"));
}

fn root(key: &str, value: &Value) -> Value {
    let mut root = json!({
        "name": REPO,
        "defaultBranch": {"displayName": "main"},
    });
    root[key] = value.clone();
    root
}

fn commit(message: &str, parents: &[&str]) -> Value {
    let subject = message.split(['\r', '\n']).next().unwrap_or_default();
    json!({
        "oid": OID,
        "subject": subject,
        "message": message,
        "author": {
            "date": "2026-09-09T20:14:26Z",
            "person": {"displayName": "Jonathan Davies"},
        },
        "committer": {
            "date": "2026-09-10T01:02:03Z",
            "person": {"displayName": "GitHub"},
        },
        "parents": parents.iter().map(|oid| json!({"oid": oid})).collect::<Vec<_>>(),
    })
}

fn modified(path: &str, stat: (u64, u64)) -> Value {
    json!({
        "oldPath": path,
        "newPath": path,
        "stat": {"added": stat.0, "deleted": stat.1},
    })
}

fn comparison(total: u64, stat: (u64, u64), nodes: &[Value]) -> Value {
    json!({
        "all": {
            "totalCount": total,
            "diffStat": {"added": stat.0, "deleted": stat.1},
        },
        "page": {"nodes": nodes},
    })
}

fn document_of(m: &Value, d: &Value) -> Value {
    json!({"data": {
        "m0": root("commit", m),
        "d0": root("comparison", d),
    }})
}

fn answer(message: &str, parents: &[&str], nodes: &[Value]) -> Value {
    let added = nodes.iter().map(|n| n["stat"]["added"].as_u64().unwrap_or(0));
    let deleted =
        nodes.iter().map(|n| n["stat"]["deleted"].as_u64().unwrap_or(0));
    document_of(
        &commit(message, parents),
        &comparison(nodes.len() as u64, (added.sum(), deleted.sum()), nodes),
    )
}

fn address() -> String {
    format!("{REPO}@v1")
}

#[test]
fn commit_text_and_json() {
    let word = format!("{REPO}@{O12}");
    let (ran, fake) = show(&[&word]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(ran.err, b"");
    assert_eq!(
        out(&ran),
        format!(
            "{REPO}@{O12}  2026-09-09  Jonathan Davies  parents {P12}  \
             1 file  +33 -0\n\
             \x20   docs(ast-grep): document run --json output and \
             zero-match debugging (#11)\n\
             \n\
             \x20   * docs: document run --json output schema and jq \
             extraction in ast-grep skill\n\
             \n\
             \x20   * docs: add node-granularity and zero-match debugging \
             tips to ast-grep skill\n\
             \n\
             M  +33 -0  ast-grep/skills/ast-grep/SKILL.md\n"
        )
    );
    let request = sent(&fake);
    assert_eq!(request["operationName"], "ShowCommit");
    assert_eq!(request["variables"]["rev"], O12);
    assert_eq!(request["variables"]["base"], format!("{O12}^1"));

    let (ran, _) = show(&["--json", &word]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(ran.err, b"");
    let subject = "docs(ast-grep): document run --json output and \
                   zero-match debugging (#11)";
    let message = format!(
        "{subject}\\n\\n* docs: document run --json output schema and jq \
         extraction in ast-grep skill\\n\\n* docs: add node-granularity \
         and zero-match debugging tips to ast-grep skill"
    );
    assert_eq!(
        out(&ran),
        format!(r#"{{"repo":"{REPO}","rev":"{O12}","commit":"{OID}","#)
            + r#""author":"Jonathan Davies","author_date":"2026-09-09","#
            + r#""committer":"GitHub","committer_date":"2026-09-09","#
            + &format!(r#""parents":["{PARENT}"],"subject":"{subject}","#)
            + &format!(r#""message":"{message}","#)
            + r#""files":[{"status":"M","#
            + r#""old_path":"ast-grep/skills/ast-grep/SKILL.md","#
            + r#""new_path":"ast-grep/skills/ast-grep/SKILL.md","#
            + r#""added":33,"deleted":0}],"#
            + "\"files_total\":1,\"added\":33,\"deleted\":0}\n"
    );
}

#[test]
fn the_plan_is_as_specified() {
    let (_, fake) = show(&[&format!("{REPO}@{O12}")]);
    let request = sent(&fake);
    let document = request["query"]
        .as_str()
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for part in [
        "query ShowCommit($repo0: String!, $rev: String!, $base: String!)",
        "m0: repository(name: $repo0)",
        "d0: repository(name: $repo0)",
        "commit(rev: $rev) { oid subject message author { date person { \
         displayName } } committer { date person { displayName } } parents \
         { oid } }",
        "comparison(base: $base, head: $rev) { all: fileDiffs(first: 0) { \
         totalCount diffStat { added deleted } } page: fileDiffs(first: \
         200) { nodes { oldPath newPath stat { added deleted } } } }",
    ] {
        assert!(document.contains(part), "{part}\n{document}");
    }
}

#[test]
fn a_merge_commit() {
    let oid = "abccd7e4d7c117370b7908f51e48decb38b6835b";
    let (ran, fake) = show(&[&format!("{REPO}@{oid}")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(ran.err, b"");
    assert_eq!(
        out(&ran),
        format!(
            "{REPO}@abccd7e4d7c1  2023-07-15  Corey Daley  \
             parents eb99d7a67714 {}  1 file  +0 -6  \
             diff against first parent\n\
             \x20   Merge pull request #713 from gorilla/coreydaley-patch-1\n\
             \n\
             \x20   Update README.md\n\
             \n\
             M  +0 -6  README.md\n",
            &SECOND[..12]
        )
    );
    assert_eq!(sent(&fake)["variables"]["base"], format!("{oid}^1"));

    let (ran, _) = show(&["--json", &format!("{REPO}@{oid}")]);
    let json = json_out(&ran);
    assert_eq!(
        json["parents"],
        json!(["eb99d7a67714bbab6db27f896a8c8c947b51b610", SECOND])
    );
    assert_eq!(json["files_total"], 1);
    assert_eq!(json["deleted"], 6);
}

#[test]
fn a_merge_is_counted_not_guessed() {
    let nodes = [modified("a.go", (1, 1))];
    let two = answer("fix a bug", &[PARENT, SECOND], &nodes);
    let ran = run_on(&scripted(&two), &[&address()]);
    let text = out(&ran);
    let header = text.lines().next().unwrap_or_default();
    assert!(
        header.contains("  diff against first parent  rev v1"),
        "{header}"
    );

    let one = answer("Merge branch 'x'", &[PARENT], &nodes);
    let ran = run_on(&scripted(&one), &[&address()]);
    let text = out(&ran);
    let header = text.lines().next().unwrap_or_default();
    assert!(!header.contains("first parent"), "{header}");
}

#[test]
fn a_root_commit() {
    let (ran, fake) = show(&[&format!("{REPO}@v0.0.1")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(
        err(&ran),
        "sego: note: root commit: Sourcegraph has no diff for it\n"
    );
    assert_eq!(
        out(&ran),
        format!(
            "{REPO}@{}  2012-10-02  rodrigo moraes  parents none  \
             rev v0.0.1\n\
             \x20   Initial commit\n",
            &ROOT[..12]
        )
    );
    assert_eq!(sent(&fake)["variables"]["base"], "v0.0.1^1");

    let (ran, _) = show(&["--json", &format!("{REPO}@v0.0.1")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(
        err(&ran),
        "sego: note: root commit: Sourcegraph has no diff for it\n"
    );
    let json = json_out(&ran);
    assert_eq!(json["commit"], ROOT);
    assert_eq!(json["rev"], "v0.0.1");
    assert_eq!(json["parents"], json!([]));
    assert_eq!(json["files"], json!([]));
    for key in ["files_total", "added", "deleted"] {
        assert_eq!(json[key], Value::Null, "{key}");
    }
}

#[test]
fn a_root_commit_outranks_a_file_list_error() {
    let mut body = answer("first", &[], &[]);
    body["data"]["d0"] = Value::Null;
    body["errors"] = json!([{"message": "x", "path": ["d0", "comparison"]}]);
    let ran = run_on(&scripted(&body), &[&address()]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(
        err(&ran),
        "sego: note: root commit: Sourcegraph has no diff for it\n"
    );
}

#[test]
fn message_layout() {
    let long = "x".repeat(600);
    let message = format!(
        "Subject\r\n\r\nfirst body line\r\n  \t\r\n\r\n{long}\r\nlast\n"
    );
    let body = answer(&message, &[PARENT], &[modified("a.go", (1, 0))]);
    let ran = run_on(&scripted(&body), &[&address()]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    let cut = "… (+100 more columns; --json has the full line)";
    assert_eq!(
        out(&ran),
        format!(
            "{REPO}@{O12}  2026-09-09  Jonathan Davies  parents {P12}  \
             1 file  +1 -0  rev v1\n\
             \x20   Subject\n\
             \n\
             \x20   first body line\n\
             \n\
             \n\
             \x20   {}{cut}\n\
             \x20   last\n\
             \n\
             M  +1 -0  a.go\n",
            "x".repeat(500)
        )
    );

    let ran = run_on(&scripted(&body), &["--json", &address()]);
    let json = json_out(&ran);
    assert_eq!(
        json["message"],
        format!("Subject\n\nfirst body line\n  \t\n\n{long}\nlast\n")
    );
    assert_eq!(json["subject"], "Subject");
}

#[test]
fn message_lines_are_escaped() {
    let body = answer("a\u{1b}[31mred\n\nb\rc", &[PARENT], &[]);
    let ran = run_on(&scripted(&body), &[&address()]);
    let text = out(&ran);
    assert!(!text.contains('\u{1b}'), "{text:?}");
    assert!(!text.contains('\r'), "{text:?}");
}

#[test]
fn no_trailing_spaces_and_no_final_blank_line() {
    let body = answer("Subject\n\n\nbody\n\n", &[PARENT], &[]);
    let ran = run_on(&scripted(&body), &[&address()]);
    let text = out(&ran);
    assert!(text.lines().all(|line| line == line.trim_end()), "{text:?}");
    assert!(
        text.ends_with("    body\n\n"),
        "one empty message line survives: {text:?}"
    );
}

#[test]
fn no_files_no_row_block() {
    let body = answer("Subject", &[PARENT], &[]);
    let ran = run_on(&scripted(&body), &[&address()]);
    assert_eq!(
        out(&ran),
        format!(
            "{REPO}@{O12}  2026-09-09  Jonathan Davies  parents {P12}  \
             0 files  +0 -0  rev v1\n    Subject\n"
        )
    );
}

#[test]
fn the_rev_field_follows_the_revision_as_written() {
    let body = answer("m", &[PARENT], &[]);
    let header = |word: &str| {
        let ran = run_on(&scripted(&body), &[&format!("{REPO}@{word}")]);
        assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
        out(&ran).lines().next().unwrap_or_default().to_owned()
    };
    let base = format!(
        "{REPO}@{O12}  2026-09-09  Jonathan Davies  parents {P12}  \
         0 files  +0 -0"
    );
    assert_eq!(header("v1"), format!("{base}  rev v1"));
    assert_eq!(header("main"), format!("{base}  rev main"));
    assert_eq!(header("affe2b9"), base);
    assert_eq!(header(OID), base);
}

#[test]
fn limit_at_200() {
    let nodes: Vec<Value> =
        (0..200).map(|n| modified(&format!("f{n:03}.go"), (1, 0))).collect();
    let body = document_of(
        &commit("big", &[PARENT]),
        &comparison(13366, (13366, 0), &nodes),
    );
    let ran = run_on(&scripted(&body), &[&address()]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    let text = out(&ran);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[0],
        format!(
            "{REPO}@{O12}  2026-09-09  Jonathan Davies  parents {P12}  \
             200 of 13366 files shown  +13366 -0  rev v1"
        )
    );
    assert_eq!(lines[1], "    big");
    assert_eq!(lines[2], "");
    assert_eq!(lines.len(), 3 + 200);
    assert_eq!(lines[202], "M  +1 -0  f199.go");

    let ran = run_on(&scripted(&body), &["--json", &address()]);
    let json = json_out(&ran);
    assert_eq!(json["files"].as_array().map(Vec::len), Some(200));
    assert_eq!(json["files_total"], 13366);
    assert_eq!(json["added"], 13366);

    let exact = &nodes[..];
    let body = document_of(
        &commit("big", &[PARENT]),
        &comparison(200, (200, 0), exact),
    );
    let ran = run_on(&scripted(&body), &[&address()]);
    let text = out(&ran);
    assert!(text.contains("  200 files  +200 -0  "), "{text}");
}

#[test]
fn units_follow_the_total() {
    let body = answer("m", &[PARENT], &[modified("a.go", (1, 1))]);
    let ran = run_on(&scripted(&body), &[&address()]);
    assert!(out(&ran).contains("  1 file  +1 -1  "), "{}", out(&ran));
}

#[test]
fn status_letters_and_quoted_paths() {
    let nodes = [
        json!({"oldPath": null, "newPath": "new.go",
               "stat": {"added": 5, "deleted": 0}}),
        json!({"oldPath": "old.go", "newPath": null,
               "stat": {"added": 0, "deleted": 7}}),
        json!({"oldPath": "a.go", "newPath": "b c.go",
               "stat": {"added": 1, "deleted": 1}}),
    ];
    let body = answer("m", &[PARENT], &nodes);
    let ran = run_on(&scripted(&body), &[&address()]);
    assert!(
        out(&ran).ends_with(
            "\nA  +5 -0  new.go\nD  +0 -7  old.go\nR  +1 -1  a.go -> 'b c.go'\n"
        ),
        "{}",
        out(&ran)
    );
}

#[test]
fn missing_revision() {
    let body = json!({
        "errors": [{"message": "revision not found: repo 1 @v9^0",
                    "path": ["d0", "comparison"]}],
        "data": {
            "m0": root("commit", &Value::Null),
            "d0": null,
        },
    });
    for json in [false, true] {
        let word = format!("{REPO}@v9");
        let words: Vec<&str> =
            if json { vec!["--json", &word] } else { vec![&word] };
        let ran = run_on(&scripted(&body), &words);
        failed(&ran, &format!("revision v9 not found in {REPO}"));
    }
}

#[test]
fn empty_repository() {
    let mut body = json!({
        "errors": [{"message": "revision not found: repo 714773 @HEAD",
                    "path": ["d0", "comparison"]}],
        "data": {
            "m0": root("commit", &Value::Null),
            "d0": null,
        },
    });
    body["data"]["m0"]["defaultBranch"] = Value::Null;
    let ran = run_on(&scripted(&body), &[&address()]);
    failed(&ran, &format!("{REPO} has no commits"));

    body["errors"] = json!([{
        "message": "secret",
        "path": ["m0", "defaultBranch"],
    }]);
    let ran = run_on(&scripted(&body), &[&address()]);
    failed(&ran, "Sourcegraph failed to answer the repository lookup");
    assert!(!err(&ran).contains("secret"));
}

#[test]
fn revision_root_errors() {
    let secret = "git command [git show --secret] failed";
    for path in [
        json!(["m0", "commit"]),
        json!(["m0", "commit", "message"]),
        json!(["m0", "commit", "author", "date"]),
        json!(["m0", "commit", "parents"]),
    ] {
        let mut body = answer("m", &[PARENT], &[modified("a.go", (1, 1))]);
        body["errors"] = json!([{"message": secret, "path": path}]);
        let at = path.as_array().map_or(0, Vec::len);
        if at == 2 {
            body["data"]["m0"]["commit"] = Value::Null;
        }
        let ran = run_on(&scripted(&body), &[&address()]);
        failed(&ran, "Sourcegraph failed to answer the revision lookup");
        assert!(!err(&ran).contains("secret"), "{path}");
    }

    let mut body = document_of(&Value::Null, &Value::Null);
    body["errors"] = json!([
        {"message": "x", "path": ["d0", "comparison"]},
        {"message": "y", "path": ["m0", "commit"]},
    ]);
    body["data"]["d0"] = Value::Null;
    let ran = run_on(&scripted(&body), &[&address()]);
    failed(&ran, "Sourcegraph failed to answer the revision lookup");
}

#[test]
fn file_list_errors() {
    let secret = "git command [git diff --secret-flag] failed";
    let cases = [
        ("d0 root", json!(["d0"]), true),
        ("comparison", json!(["d0", "comparison"]), true),
        ("page nodes", json!(["d0", "comparison", "page", "nodes"]), false),
        ("total", json!(["d0", "comparison", "all", "totalCount"]), false),
    ];
    for (name, path, nulls) in cases {
        let mut body = answer("m", &[PARENT], &[modified("a.go", (1, 1))]);
        body["errors"] = json!([{"message": secret, "path": path}]);
        if nulls {
            body["data"]["d0"] = Value::Null;
        }
        let ran = run_on(&scripted(&body), &[&address()]);
        failed(&ran, "Sourcegraph failed to answer the file list");
        assert!(!err(&ran).contains("secret"), "{name}");
    }
}

#[test]
fn a_comparison_without_totals_is_a_failure() {
    let mut body = answer("m", &[PARENT], &[]);
    body["data"]["d0"]["comparison"] = json!({"page": {"nodes": []}});
    let ran = run_on(&scripted(&body), &[&address()]);
    failed(&ran, "Sourcegraph failed to answer the file list");
}

#[test]
fn the_repository_must_exist() {
    let body = json!({"data": {"m0": null, "d0": null}});
    let ran = run_on(&scripted(&body), &["example.com/x/y@v1"]);
    assert_eq!(ran.code, ExitCode::from(1));
    assert!(err(&ran).starts_with(
        "sego: error: example.com/x/y is not on sourcegraph.com\n"
    ));
}

#[test]
fn a_range_is_not_a_commit() {
    let fake = scripted(&json!({"data": {}}));
    let ran = run_on(&fake, &[&format!("{REPO}@v1...v2")]);
    assert_eq!(ran.code, ExitCode::from(2), "{}", err(&ran));
    assert!(fake.calls().is_empty());
}

#[test]
fn names_are_escaped() {
    let mut body = answer("m", &[PARENT], &[]);
    body["data"]["m0"]["commit"]["author"]["person"]["displayName"] =
        json!("Eve\u{1b}[31m\nX");
    let ran = run_on(&scripted(&body), &[&address()]);
    let text = out(&ran);
    assert!(!text.contains('\u{1b}'), "{text:?}");
    assert_eq!(text.matches('\n').count(), 2, "{text:?}");
}

#[test]
fn a_whitespace_only_line_is_blank() {
    let body = answer("A\n \t \n\u{2028}\nB", &[PARENT], &[]);
    let ran = run_on(&scripted(&body), &[&address()]);
    let text = out(&ran);
    let lines: Vec<&str> = text.lines().skip(1).collect();
    assert_eq!(lines.len(), 4, "{text:?}");
    assert_eq!(lines[0], "    A");
    assert_eq!(lines[1], "");
    assert!(lines[2].starts_with("    ") && lines[2].len() > 4, "{text:?}");
    assert!(!lines[2].contains('\u{2028}'), "{text:?}");
    assert_eq!(lines[3], "    B");
}

#[test]
fn the_author_is_required() {
    for path in [
        &["author"][..],
        &["author", "date"],
        &["author", "person"],
        &["author", "person", "displayName"],
    ] {
        let mut body = answer("m", &[PARENT], &[]);
        let mut at = &mut body["data"]["m0"]["commit"];
        for key in path {
            at = &mut at[*key];
        }
        *at = Value::Null;
        let ran = run_on(&scripted(&body), &[&address()]);
        failed(&ran, "Sourcegraph failed to answer the revision lookup");
    }

    let mut body = answer("m", &[PARENT], &[]);
    body["data"]["m0"]["commit"]["committer"] = Value::Null;
    let ran = run_on(&scripted(&body), &[&address()]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    let json = json_out(&run_on(&scripted(&body), &["--json", &address()]));
    assert_eq!(json["committer"], Value::Null);
    assert_eq!(json["committer_date"], Value::Null);
}
