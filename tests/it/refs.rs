use std::path::Path;
use std::process::ExitCode;

use serde_json::{Value, json};

use sego::internal::upstream::{Endpoint, Reply, Transport};

use crate::support::exec::{Ran, exec};
use crate::support::fake::Fake;
use crate::support::fixture::load_all;

const REFS: &str = "example.com/selftest/refs";
const MASTER: &str = "3fce3b5bb023a41c5d8e9f0a1b2c3d4e5f6a7b8c";

fn fixtures() -> Fake {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/Refs");
    Fake::fixtures(load_all(&root))
}

fn run_on(fake: &Fake, words: &[&str]) -> Ran {
    let handle = fake.clone();
    let connect =
        move |_: Endpoint| -> Box<dyn Transport> { Box::new(handle.clone()) };
    let mut argv = vec!["refs"];
    argv.extend_from_slice(words);
    exec(&argv, None, &connect)
}

fn refs(words: &[&str]) -> (Ran, Fake) {
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

fn variables(fake: &Fake) -> Value {
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    let body: Value = serde_json::from_slice(&calls[0].body)
        .unwrap_or_else(|e| panic!("{e}"));
    body["variables"].clone()
}

fn document(fake: &Fake) -> String {
    let calls = fake.calls();
    let body: Value = serde_json::from_slice(&calls[0].body)
        .unwrap_or_else(|e| panic!("{e}"));
    body["query"].as_str().unwrap_or_default().to_owned()
}

fn node(name: &str, n: usize) -> Value {
    let oid = format!("{n:040x}");
    json!({"displayName": name, "target": {"oid": oid, "commit": {
        "committer": {"date": "2026-01-02T03:04:05Z"}}}})
}

fn many(prefix: &str, count: usize) -> Vec<Value> {
    (0..count).map(|n| node(&format!("{prefix}{n}"), n + 1)).collect()
}

fn root(branches: &Value, tags: &Value) -> Value {
    json!({"data": {"r0": {
        "name": REFS,
        "defaultBranch": {"displayName": "b0"},
        "branches": branches,
        "tags": tags,
    }}})
}

fn list(total: usize, nodes: &[Value]) -> Value {
    json!({"totalCount": total, "nodes": nodes})
}

fn failed(ran: &Ran, message: &str) {
    assert_eq!(ran.code, ExitCode::from(1), "{}", err(ran));
    assert_eq!(ran.out, b"");
    assert_eq!(err(ran), format!("sego: error: {message}\n"));
}

#[test]
fn refs_text_and_json() {
    let (ran, _) = refs(&[REFS]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(ran.err, b"");
    assert_eq!(
        out(&ran),
        "example.com/selftest/refs  16 branches  100 of 272 tags shown\n\
         branch  3fce3b5bb023  2026-08-04  master  (default)\n\
         branch  9b4d6c1aa772  2026-07-23  ag/index-design-wip\n\
         tag     e3d7b2c11f0a  2026-08-04  ignore-0.4.33\n"
    );

    let (ran, _) = refs(&["--json", REFS]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(ran.err, b"");
    assert_eq!(
        out(&ran),
        concat!(
            r#"{"repo":"example.com/selftest/refs","query":null,"#,
            r#""default_branch":"master","branches_total":16,"#,
            r#""tags_total":272,"branches_all":null,"tags_all":null,"#,
            r#""branches_more":true,"tags_more":true,"refs":["#,
            r#"{"kind":"branch","name":"master","#,
            r#""commit":"3fce3b5bb023a41c5d8e9f0a1b2c3d4e5f6a7b8c","#,
            r#""date":"2026-08-04"},"#,
            r#"{"kind":"branch","name":"ag/index-design-wip","#,
            r#""commit":"9b4d6c1aa772e0f1a2b3c4d5e6f708192a3b4c5d","#,
            r#""date":"2026-07-23"},"#,
            r#"{"kind":"tag","name":"ignore-0.4.33","#,
            r#""commit":"e3d7b2c11f0ab9c8d7e6f5a4b3c2d1e0f9a8b7c6","#,
            r#""date":"2026-08-04"}]}"#,
            "\n"
        )
    );
}

#[test]
fn filtered() {
    let (ran, fake) = refs(&[REFS, "14.1"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(
        out(&ran),
        "example.com/selftest/refs  0 of 16 branches match \"14.1\"  \
         2 of 272 tags match \"14.1\"\n\
         tag     aaaaaaaaaaaa  2025-03-02  14.1.1\n\
         tag     bbbbbbbbbbbb  2025-01-20  14.1.0\n"
    );
    assert_eq!(variables(&fake)["query"], "14.1");

    let (ran, _) = refs(&["--json", REFS, "14.1"]);
    let json = json_out(&ran);
    assert_eq!(json["query"], "14.1");
    assert_eq!(json["branches_total"], 0);
    assert_eq!(json["tags_total"], 2);
    assert_eq!(json["branches_more"], false);
    assert_eq!(json["tags_more"], false);
}

fn counted(rows: &[String], kind: &str) -> usize {
    rows.iter().filter(|row| row.starts_with(kind)).count()
}

fn text_rows(ran: &Ran) -> Vec<String> {
    out(ran).lines().skip(1).map(str::to_owned).collect()
}

#[test]
fn limits() {
    let exact = root(&list(100, &many("b", 100)), &list(100, &many("t", 100)));
    let fake = scripted(&exact);
    let ran = run_on(&fake, &[REFS]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert!(
        out(&ran).starts_with(&format!("{REFS}  100 branches  100 tags\n"))
    );
    let rows = text_rows(&ran);
    assert_eq!((counted(&rows, "branch"), counted(&rows, "tag")), (100, 100));

    let over = root(&list(101, &many("b", 101)), &list(5456, &many("t", 101)));
    let ran = run_on(&scripted(&over), &[REFS]);
    assert!(out(&ran).starts_with(&format!(
        "{REFS}  100 of 101 branches shown  100 of 5456 tags shown\n"
    )));
    let rows = text_rows(&ran);
    assert_eq!((counted(&rows, "branch"), counted(&rows, "tag")), (100, 100));
    assert!(rows.iter().any(|row| row.ends_with("  b99")));
    assert!(!rows.iter().any(|row| row.ends_with("  b100")));
    assert!(rows.iter().any(|row| row.ends_with("  t99")));
    assert!(!rows.iter().any(|row| row.ends_with("  t100")));
}

#[test]
fn limits_when_filtered() {
    let mut body = root(&list(101, &many("b", 101)), &list(3, &many("t", 3)));
    body["data"]["r0"]["all_branches"] = json!({"totalCount": 5456});
    body["data"]["r0"]["all_tags"] = json!({"totalCount": 1});
    let ran = run_on(&scripted(&body), &[REFS, "x"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(
        out(&ran).lines().next(),
        Some(
            "example.com/selftest/refs  101 of 5456 branches match \"x\"  \
             100 shown  3 of 1 tag match \"x\""
        )
    );
    assert_eq!(counted(&text_rows(&ran), "branch"), 100);
}

#[test]
fn units_follow_the_total() {
    let mut body = root(&list(1, &many("b", 1)), &list(0, &[]));
    let ran = run_on(&scripted(&body), &[REFS]);
    assert_eq!(
        out(&ran).lines().next(),
        Some("example.com/selftest/refs  1 branch  0 tags")
    );
    body["data"]["r0"]["all_branches"] = json!({"totalCount": 1});
    body["data"]["r0"]["all_tags"] = json!({"totalCount": 1});
    let ran = run_on(&scripted(&body), &[REFS, "x"]);
    assert_eq!(
        out(&ran).lines().next(),
        Some(
            "example.com/selftest/refs  1 of 1 branch match \"x\"  \
             0 of 1 tag match \"x\""
        )
    );
}

#[test]
fn annotated_and_undereferenced_tags() {
    let (ran, _) = refs(&["example.com/selftest/tags"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(
        out(&ran),
        "example.com/selftest/tags  1 branch  3 tags\n\
         branch  db9d1d0073d2  2024-06-19  main  (default)\n\
         tag     c0ffee00c0ff  2024-05-01  v2.0.0\n\
         tag     -             -           v2.6.11\n\
         tag     0123456789ab  2020-02-29  v0.1\n"
    );

    let (ran, _) = refs(&["--json", "example.com/selftest/tags"]);
    let json = json_out(&ran);
    let refs = json["refs"].as_array().unwrap_or_else(|| panic!("refs"));
    assert_eq!(refs.len(), 4);
    assert_eq!(refs[1]["commit"], "c0ffee00c0ffee00c0ffee00c0ffee00c0ffee00");
    assert_eq!(
        refs[2],
        json!({"kind": "tag", "name": "v2.6.11", "commit": null,
               "date": null})
    );
}

#[test]
fn a_missing_committer_has_no_date() {
    let mut tag = node("v1", 7);
    tag["target"]["commit"] = json!({"committer": null});
    let body = root(&list(0, &[]), &list(1, &[tag]));
    let ran = run_on(&scripted(&body), &["--json", REFS]);
    let json = json_out(&ran);
    assert_eq!(json["refs"][0]["commit"], format!("{:040x}", 7));
    assert_eq!(json["refs"][0]["date"], Value::Null);
}

#[test]
fn empty_repository() {
    let (ran, _) = refs(&["example.com/selftest/empty"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(ran.err, b"");
    assert_eq!(out(&ran), "example.com/selftest/empty  0 branches  0 tags\n");

    let (ran, _) = refs(&["--json", "example.com/selftest/empty"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(
        out(&ran),
        concat!(
            r#"{"repo":"example.com/selftest/empty","query":null,"#,
            r#""default_branch":null,"branches_total":0,"tags_total":0,"#,
            r#""branches_all":null,"tags_all":null,"branches_more":false,"#,
            r#""tags_more":false,"refs":[]}"#,
            "\n"
        )
    );
}

#[test]
fn text_is_a_substring() {
    let (ran, fake) = refs(&[REFS, "v1.*"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(variables(&fake)["query"], "v1.*");
    assert_eq!(
        out(&ran),
        "example.com/selftest/refs  0 of 4 branches match \"v1.*\"  \
         0 of 15 tags match \"v1.*\"\n"
    );
}

#[test]
fn text_is_escaped_in_the_header() {
    let mut body = root(&list(0, &[]), &list(0, &[]));
    body["data"]["r0"]["all_branches"] = json!({"totalCount": 1});
    body["data"]["r0"]["all_tags"] = json!({"totalCount": 1});
    let ran = run_on(&scripted(&body), &[REFS, "a\u{1b}[31mb"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert!(!out(&ran).contains('\u{1b}'));
    assert_eq!(out(&ran).lines().count(), 1);
}

#[test]
fn names_are_escaped_in_rows() {
    let nodes = vec![node("a\u{1b}[31mb\nc", 1)];
    let body = root(&list(1, &nodes), &list(0, &[]));
    let ran = run_on(&scripted(&body), &[REFS]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert!(!out(&ran).contains('\u{1b}'));
    assert_eq!(out(&ran).lines().count(), 2);
}

#[test]
fn first_by_mode() {
    let (_, fake) = refs(&[REFS]);
    let vars = variables(&fake);
    assert_eq!(vars["first"], 101);
    assert_eq!(vars["query"], Value::Null);
    assert_eq!(vars["repo0"], REFS);

    let (_, fake) = refs(&["--json", REFS]);
    assert_eq!(variables(&fake)["first"], 17000);

    let (_, fake) = refs(&[REFS, "14.1"]);
    assert_eq!(variables(&fake)["first"], 101);
    let (_, fake) = refs(&["--json", REFS, "14.1"]);
    assert_eq!(variables(&fake)["first"], 17000);
}

#[test]
fn all_totals() {
    let (ran, fake) = refs(&["--json", REFS, "14.1"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    let json = json_out(&ran);
    assert_eq!(json["branches_all"], 16);
    assert_eq!(json["tags_all"], 272);
    assert_eq!(json["branches_total"], 0);
    assert_eq!(json["tags_total"], 2);
    let sent = document(&fake);
    assert!(sent.contains("all_branches: branches(first: 1) { totalCount }"));
    assert!(sent.contains("all_tags: tags(first: 1) { totalCount }"));

    let (ran, fake) = refs(&["--json", REFS]);
    let json = json_out(&ran);
    assert_eq!(json["branches_all"], Value::Null);
    assert_eq!(json["tags_all"], Value::Null);
    let sent = document(&fake);
    assert!(!sent.contains("all_branches"));
    assert!(!sent.contains("all_tags"));
}

#[test]
fn the_default_branch_is_marked_by_name() {
    let (ran, _) = refs(&[REFS]);
    let text = out(&ran);
    let defaults: Vec<&str> =
        text.lines().filter(|line| line.ends_with("(default)")).collect();
    assert_eq!(defaults.len(), 1);
    assert!(defaults[0].contains(&MASTER[..12]));
}

#[test]
fn the_default_branch_marker_follows_the_name_not_the_position() {
    let nodes = vec![node("b1", 1), node("b0", 2), node("b2", 3)];
    let body = root(&list(3, &nodes), &list(0, &[]));
    let ran = run_on(&scripted(&body), &[REFS]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(
        out(&ran),
        "example.com/selftest/refs  3 branches  0 tags\n\
         branch  000000000000  2026-01-02  b1\n\
         branch  000000000000  2026-01-02  b0  (default)\n\
         branch  000000000000  2026-01-02  b2\n"
    );

    let ran = run_on(&scripted(&body), &["--json", REFS]);
    let json = json_out(&ran);
    assert_eq!(json["default_branch"], "b0");
    assert_eq!(json["refs"][1]["name"], "b0");
}

#[test]
fn refs_errors() {
    let run = |body: &Value, words: &[&str]| run_on(&scripted(body), words);

    let ran = run(&json!({"data": {"r0": null}}), &[REFS]);
    assert_eq!(ran.code, ExitCode::from(1));
    assert!(err(&ran).starts_with(&format!(
        "sego: error: {REFS} is not on sourcegraph.com\n"
    )));

    let live = |part: &str| {
        json!({"errors": [{
            "message": "panic occurred: runtime error: slice bounds out of \
                        range [:-1]",
            "path": ["r0", part, "nodes"]
        }], "data": {"r0": null}})
    };
    let ran = run(&live("tags"), &[REFS]);
    failed(&ran, "Sourcegraph failed to answer the tag list");
    let ran = run(&live("branches"), &[REFS]);
    failed(&ran, "Sourcegraph failed to answer the branch list");
    let ran = run(&live("all_tags"), &[REFS, "x"]);
    failed(&ran, "Sourcegraph failed to answer the tag list");
    let ran = run(&live("all_branches"), &[REFS, "x"]);
    failed(&ran, "Sourcegraph failed to answer the branch list");
    let ran = run(&live("tags"), &["--json", REFS]);
    failed(&ran, "Sourcegraph failed to answer the tag list");

    let mut partial = root(&list(1, &many("b", 1)), &Value::Null);
    partial["errors"] = json!([{
        "message": "boom", "path": ["r0", "tags"]
    }]);
    let ran = run(&partial, &[REFS]);
    failed(&ran, "Sourcegraph failed to answer the tag list");

    let mut partial = root(&Value::Null, &list(0, &[]));
    partial["errors"] = json!([{
        "message": "boom", "path": ["r0", "branches", "nodes"]
    }]);
    let ran = run(&partial, &[REFS]);
    failed(&ran, "Sourcegraph failed to answer the branch list");

    let mut partial = root(&list(0, &[]), &list(0, &[]));
    partial["data"]["r0"]["defaultBranch"] = Value::Null;
    partial["errors"] = json!([{
        "message": "boom", "path": ["r0", "defaultBranch"]
    }]);
    let ran = run(&partial, &[REFS]);
    failed(&ran, "Sourcegraph failed to answer the repository lookup");

    let ran = run(&root(&Value::Null, &Value::Null), &[REFS]);
    failed(&ran, "Sourcegraph failed to answer the branch list");

    let both = json!({"data": {
        "r0": {"name": "github.com/o/n", "defaultBranch": null,
               "branches": list(0, &[]),
               "tags": list(0, &[])},
        "r1": {"name": "gitlab.com/o/n", "defaultBranch": null,
               "branches": list(0, &[]),
               "tags": list(0, &[])}
    }});
    let ran = run(&both, &["o/n"]);
    failed(&ran, "o/n exists as github.com/o/n and gitlab.com/o/n");
}

#[test]
fn owner_name_reads_the_root_that_exists() {
    let body = json!({"data": {
        "r0": null,
        "r1": {"name": "gitlab.com/o/n",
               "defaultBranch": {"displayName": "main"},
               "branches": list(1, &[node("main", 3)]),
               "tags": list(0, &[])}
    }});
    let fake = scripted(&body);
    let ran = run_on(&fake, &["o/n"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(
        out(&ran),
        "gitlab.com/o/n  1 branch  0 tags\n\
         branch  000000000000  2026-01-02  main  (default)\n"
    );
    let vars = variables(&fake);
    assert_eq!(vars["repo0"], "github.com/o/n");
    assert_eq!(vars["repo1"], "gitlab.com/o/n");
}
