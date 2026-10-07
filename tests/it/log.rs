use std::path::Path;
use std::process::ExitCode;

use serde_json::{Value, json};

use sego::internal::upstream::{Endpoint, Reply, Transport};

use crate::support::exec::{Ran, exec};
use crate::support::fake::Fake;
use crate::support::fixture::load_all;

const REPO: &str = "example.com/selftest/log";
const OID_A: &str = "4ad40c3a28bd2acabbb50eb3ff63b08c2c55f96c";
const OID_B: &str = "1e23553eec2392753c5420d348e63a63f517cd45";
const OID_C: &str = "a2c9d37aab87e75e6b10698f2dad72e20432aa69";
const A12: &str = "4ad40c3a28bd";
const B12: &str = "1e23553eec23";

fn fixtures() -> Fake {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/Log");
    Fake::fixtures(load_all(&root))
}

fn run_on(fake: &Fake, words: &[&str]) -> Ran {
    let handle = fake.clone();
    let connect =
        move |_: Endpoint| -> Box<dyn Transport> { Box::new(handle.clone()) };
    let mut argv = vec!["log"];
    argv.extend_from_slice(words);
    exec(&argv, None, &connect)
}

fn log(words: &[&str]) -> (Ran, Fake) {
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

fn variables(fake: &Fake) -> Value {
    sent(fake)["variables"].clone()
}

fn document(fake: &Fake) -> String {
    sent(fake)["query"].as_str().unwrap_or_default().to_owned()
}

fn failed(ran: &Ran, message: &str) {
    assert_eq!(ran.code, ExitCode::from(1), "{}", err(ran));
    assert_eq!(ran.out, b"");
    assert_eq!(err(ran), format!("sego: error: {message}\n"));
}

fn root(fields: &[(&str, Value)]) -> Value {
    let mut root = json!({
        "name": REPO,
        "defaultBranch": {"displayName": "main"},
    });
    for (key, value) in fields {
        root[*key] = value.clone();
    }
    root
}

fn oid(n: usize) -> String {
    format!("{n:040x}")
}

fn node(n: usize, subject: &str) -> Value {
    json!({
        "oid": oid(n),
        "subject": subject,
        "author": {
            "date": "2026-09-14T10:19:56Z",
            "person": {"displayName": "Ada Example"},
        },
        "parents": [{"oid": oid(n + 1)}],
    })
}

fn nodes(count: usize) -> Vec<Value> {
    (1..=count).map(|n| node(n, &format!("change {n}"))).collect()
}

fn page(nodes: &[Value], more: bool) -> Value {
    json!({"nodes": nodes, "pageInfo": {"hasNextPage": more}})
}

fn tags(total: u64, tags: &[(&str, &str)]) -> Value {
    let nodes: Vec<Value> = tags
        .iter()
        .map(
            |(name, oid)| json!({"displayName": name, "target": {"oid": oid}}),
        )
        .collect();
    root(&[("gitRefs", json!({"totalCount": total, "nodes": nodes}))])
}

fn rev_body(nodes: &[Value], more: bool, tag_list: &Value) -> Value {
    json!({"data": {
        "r0": root(&[("commit", json!({
            "oid": oid(1),
            "ancestors": page(nodes, more),
        }))]),
        "t0": tag_list,
    }})
}

fn range_body(
    ahead: Option<u64>,
    nodes: &[Value],
    more: bool,
    tag_list: &Value,
) -> Value {
    let mut data = json!({
        "a0": root(&[("commit", json!({"oid": OID_A}))]),
        "b0": root(&[("commit", json!({"oid": OID_B}))]),
        "c0": root(&[(
            "comparison",
            json!({"commits": page(nodes, more)}),
        )]),
        "t0": tag_list,
    });
    if let Some(ahead) = ahead {
        data["n0"] =
            root(&[("commit", json!({"behindAhead": {"ahead": ahead}}))]);
    }
    json!({"data": data})
}

fn no_tags() -> Value {
    tags(0, &[])
}

#[test]
fn log_text_and_json() {
    let (ran, fake) = log(&[&format!("{REPO}@v2.1.12..v2.1.14")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(ran.err, b"");
    let text = out(&ran);
    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some(
            format!("{REPO}@{A12}..{B12}  10 commits  rev v2.1.12..v2.1.14")
                .as_str()
        )
    );
    assert_eq!(
        lines.next(),
        Some(
            format!(
                "{B12}  2026-09-14  Valentin Kiselev  (tag: v2.1.14) \
                 2.1.14: small fixes and spinner improvements"
            )
            .as_str()
        )
    );
    assert_eq!(
        lines.next(),
        Some(
            "a2c9d37aab87  2026-09-14  Valentin Kiselev  (tag: v2.1.13) \
             2.1.13: small fixes and spinner improvements"
        )
    );
    assert_eq!(
        lines.next(),
        Some(
            "c7b498349ffa  2026-09-14  Nityanand Gupta  \
             fix(run): error when --job/--command matches nothing (#1512)"
        )
    );
    assert_eq!(lines.count(), 7);
    let vars = variables(&fake);
    assert_eq!(vars["a"], "v2.1.12");
    assert_eq!(vars["b"], "v2.1.14");
    assert_eq!(vars["path"], Value::Null);

    let (ran, _) = log(&["--json", &format!("{REPO}@v2.1.12..v2.1.14")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    let json = json_out(&ran);
    let text = out(&ran);
    assert!(text.starts_with(&format!(
        r#"{{"repo":"{REPO}","rev":null,"commit":null,"rev_a":"v2.1.12","#
    )));
    assert!(text.contains(
        r#""path":null,"total":10,"more":false,"commits":[{"commit":""#
    ));
    assert_eq!(json["repo"], REPO);
    assert_eq!(json["rev"], Value::Null);
    assert_eq!(json["commit"], Value::Null);
    assert_eq!(json["rev_a"], "v2.1.12");
    assert_eq!(json["rev_b"], "v2.1.14");
    assert_eq!(json["commit_a"], OID_A);
    assert_eq!(json["commit_b"], OID_B);
    assert_eq!(json["path"], Value::Null);
    assert_eq!(json["total"], 10);
    assert_eq!(json["more"], false);
    assert_eq!(json["commits"].as_array().map(Vec::len), Some(10));
    assert_eq!(
        json["commits"][1],
        json!({
            "commit": OID_C,
            "date": "2026-09-14",
            "author": "Valentin Kiselev",
            "subject": "2.1.13: small fixes and spinner improvements",
            "parents": ["c7b498349fface33a7b6d6e613455568815c77e2"],
            "tags": ["v2.1.13"],
        })
    );
    assert_eq!(json["commits"][2]["tags"], json!([]));
}

#[test]
fn default_branch_sends_head() {
    let (ran, fake) = log(&["example.com/selftest/head"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    let vars = variables(&fake);
    assert_eq!(vars["rev"], "HEAD");
    assert_eq!(vars["path"], Value::Null);
    assert_eq!(
        out(&ran),
        "example.com/selftest/head@98cb6bf42e08  2 commits\n\
         98cb6bf42e08  2022-11-01  Ada Example  (tag: v1.8.0) Release v1.8.0\n\
         a7962380ca08  2019-01-02  Ada Example  \
         (tag: v1.7.0, tag: v1.7.0-rc1) Release v1.7.0\n"
    );

    let (ran, _) = log(&["--json", "example.com/selftest/head"]);
    let json = json_out(&ran);
    assert_eq!(json["rev"], Value::Null);
    assert_eq!(json["commit"], "98cb6bf42e086f6af920b965c38cacc07402d51b");
    assert_eq!(json["rev_a"], Value::Null);
    assert_eq!(json["total"], Value::Null);
    assert_eq!(json["commits"][1]["parents"], json!([]));
}

#[test]
fn rev_limit() {
    let body = rev_body(&nodes(151), true, &no_tags());
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v1")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    let text = out(&ran);
    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some(
            format!(
                "{REPO}@{}  150 commits shown; more exist  rev v1",
                &oid(1)[..12]
            )
            .as_str()
        )
    );
    assert_eq!(lines.clone().count(), 150);
    assert_eq!(
        lines.last(),
        Some(
            format!(
                "{}  2026-09-14  Ada Example  change 150",
                &oid(150)[..12]
            )
            .as_str()
        )
    );

    let ran = run_on(&scripted(&body), &["--json", &format!("{REPO}@v1")]);
    let json = json_out(&ran);
    assert_eq!(json["more"], true);
    assert_eq!(json["total"], Value::Null);
    assert_eq!(json["rev"], "v1");
    assert_eq!(json["commits"].as_array().map(Vec::len), Some(150));

    let body = rev_body(&nodes(150), false, &no_tags());
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v1")]);
    assert!(out(&ran).starts_with(&format!(
        "{REPO}@{}  150 commits  rev v1\n",
        &oid(1)[..12]
    )));
}

#[test]
fn one_commit_is_singular() {
    let body = rev_body(&nodes(1), false, &no_tags());
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@{OID_A}")]);
    assert_eq!(
        out(&ran).lines().next(),
        Some(format!("{REPO}@{}  1 commit", &oid(1)[..12]).as_str())
    );
}

#[test]
fn range_totals() {
    let body = range_body(Some(15109), &nodes(151), true, &no_tags());
    let fake = scripted(&body);
    let ran = run_on(&fake, &[&format!("{REPO}@v1..v2")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    let text = out(&ran);
    assert_eq!(
        text.lines().next(),
        Some(
            format!(
                "{REPO}@{A12}..{B12}  150 of 15109 commits shown  rev v1..v2"
            )
            .as_str()
        )
    );
    assert_eq!(text.lines().count(), 151);

    let fake = scripted(&range_body(Some(1), &nodes(1), false, &no_tags()));
    let ran = run_on(&fake, &[&format!("{REPO}@{A12}..{B12}")]);
    assert_eq!(
        out(&ran).lines().next(),
        Some(format!("{REPO}@{A12}..{B12}  1 commit").as_str())
    );

    let body = range_body(Some(15109), &nodes(200), false, &no_tags());
    let ran = run_on(&scripted(&body), &["--json", &format!("{REPO}@v1..v2")]);
    let json = json_out(&ran);
    assert_eq!(json["total"], 15109);
    assert_eq!(json["more"], false);
    assert_eq!(json["commits"].as_array().map(Vec::len), Some(200));
}

#[test]
fn the_rev_field_follows_the_revisions_as_written() {
    let body = range_body(Some(1), &nodes(1), false, &no_tags());
    let header = |word: &str| {
        let ran = run_on(&scripted(&body), &[word]);
        assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
        out(&ran).lines().next().unwrap_or_default().to_owned()
    };
    let base = format!("{REPO}@{A12}..{B12}  1 commit");
    assert_eq!(header(&format!("{REPO}@abc1234..def5678")), base);
    assert_eq!(
        header(&format!("{REPO}@abc1234..v2")),
        format!("{base}  rev abc1234..v2")
    );
    assert_eq!(
        header(&format!("{REPO}@v1..def5678")),
        format!("{base}  rev v1..def5678")
    );
}

#[test]
fn range_with_a_path() {
    let body = range_body(None, &nodes(2), false, &no_tags());
    let fake = scripted(&body);
    let ran = run_on(&fake, &[&format!("{REPO}@v1..v2"), "src/a.go"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(
        out(&ran).lines().next(),
        Some(
            format!("{REPO}@{A12}..{B12}:src/a.go  2 commits  rev v1..v2")
                .as_str()
        )
    );
    assert_eq!(variables(&fake)["path"], "src/a.go");
    let doc = document(&fake);
    assert!(!doc.contains("n0:"), "{doc}");
    assert!(!doc.contains("behindAhead"), "{doc}");
    assert!(doc.contains("a0: repository(name: $repo0)"));
    assert!(doc.contains("b0: repository(name: $repo0)"));
    assert!(doc.contains("c0: repository(name: $repo0)"));
    assert!(doc.contains("t0: repository(name: $repo0)"));

    let body = range_body(None, &nodes(151), true, &no_tags());
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v1..v2:src/a.go")]);
    assert_eq!(
        out(&ran).lines().next(),
        Some(
            format!(
                "{REPO}@{A12}..{B12}:src/a.go  150 commits shown; more exist  \
                 rev v1..v2"
            )
            .as_str()
        )
    );

    let ran = run_on(
        &scripted(&body),
        &["--json", &format!("{REPO}@v1..v2:src/a.go")],
    );
    let json = json_out(&ran);
    assert_eq!(json["path"], "src/a.go");
    assert_eq!(json["total"], Value::Null);
    assert_eq!(json["more"], true);
}

#[test]
fn a_rev_with_a_path() {
    let body = rev_body(&nodes(2), false, &no_tags());
    let fake = scripted(&body);
    let ran = run_on(&fake, &[&format!("{REPO}@v1:src/a.go")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(
        out(&ran).lines().next(),
        Some(
            format!("{REPO}@{}:src/a.go  2 commits  rev v1", &oid(1)[..12])
                .as_str()
        )
    );
    assert_eq!(variables(&fake)["path"], "src/a.go");
}

#[test]
fn an_empty_range() {
    let body = range_body(Some(0), &[], false, &no_tags());
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v2..v1")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(ran.err, b"");
    assert_eq!(
        out(&ran),
        format!("{REPO}@{A12}..{B12}  0 commits  rev v2..v1\n")
    );

    let ran = run_on(&scripted(&body), &["--json", &format!("{REPO}@v2..v1")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    let json = json_out(&ran);
    assert_eq!(json["total"], 0);
    assert_eq!(json["commits"], json!([]));
}

#[test]
fn tag_marks() {
    let list =
        tags(3, &[("v2", &oid(1)), ("v1", &oid(1)), ("other", &oid(99))]);
    let body = rev_body(&nodes(2), false, &list);
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v2")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(ran.err, b"");
    let text = out(&ran);
    let rows: Vec<&str> = text.lines().skip(1).collect();
    assert_eq!(
        rows,
        [
            format!(
                "{}  2026-09-14  Ada Example  (tag: v2, tag: v1) change 1",
                &oid(1)[..12]
            ),
            format!("{}  2026-09-14  Ada Example  change 2", &oid(2)[..12]),
        ]
    );

    let ran = run_on(&scripted(&body), &["--json", &format!("{REPO}@v2")]);
    assert_eq!(json_out(&ran)["commits"][0]["tags"], json!(["v2", "v1"]));

    let list = tags(1250, &[("v2", &oid(1))]);
    let body = rev_body(&nodes(1), false, &list);
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v2")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(
        err(&ran),
        "sego: note: tags marked from the 1000 newest of 1250\n"
    );

    let list = tags(1000, &[("v2", &oid(1))]);
    let body = rev_body(&nodes(1), false, &list);
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v2")]);
    assert_eq!(ran.err, b"");
}

#[test]
fn a_row_never_ends_in_whitespace() {
    let list = tags(1, &[("v2", &oid(2))]);
    let blank = [node(1, ""), node(2, ""), node(3, "  ")];
    let body = rev_body(&blank, false, &list);
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v2")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    let text = out(&ran);
    let rows: Vec<&str> = text.lines().skip(1).collect();
    assert_eq!(
        rows,
        [
            format!("{}  2026-09-14  Ada Example", &oid(1)[..12]),
            format!("{}  2026-09-14  Ada Example  (tag: v2)", &oid(2)[..12]),
            format!("{}  2026-09-14  Ada Example", &oid(3)[..12]),
        ]
    );
}

#[test]
fn hostile_names_are_escaped() {
    let mut node = node(1, "red\u{1b}[31m subject");
    node["author"]["person"]["displayName"] = json!("Eve\u{1b}[2J\nMallory");
    node["oid"] = json!("ab\u{1b}[2Jc\nd0123456789");
    node["author"]["date"] = json!("2026-0\u{1b}[2J\n9-14T10:19:56Z");
    let hostile = "ab\u{1b}[2Jc\nd0123456789";
    let list = tags(1, &[("v\u{1b}1", hostile)]);
    let body = rev_body(&[node], false, &list);
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v2")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert!(!out(&ran).contains('\u{1b}'));
    assert_eq!(out(&ran).lines().count(), 2);
    let row = out(&ran).lines().nth(1).unwrap_or_default().to_owned();
    assert!(
        row.starts_with("ab\\u{1b}[2Jc d012  2026-0\\u{1b}[2J  Eve"),
        "{row}"
    );
    assert!(
        row.ends_with("  (tag: v\\u{1b}1) red\\u{1b}[31m subject"),
        "{row}"
    );
    let ran = run_on(&scripted(&body), &["--json", &format!("{REPO}@v2")]);
    assert!(!out(&ran).contains('\u{1b}'));
}

#[test]
fn a_deleted_path() {
    let body = rev_body(&nodes(2), false, &no_tags());
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v1:gone.go")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(out(&ran).lines().count(), 3);

    let mut body = range_body(None, &nodes(3), false, &no_tags());
    body["data"]["a0"]["commit"]["x"] = Value::Null;
    body["data"]["b0"]["commit"]["x"] = Value::Null;
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v1..v2:gone.go")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(out(&ran).lines().count(), 4);
}

#[test]
fn x_is_sent_for_a_plain_path_in_a_range_only() {
    let body = range_body(None, &nodes(1), false, &no_tags());
    let counted = range_body(Some(1), &nodes(1), false, &no_tags());
    let fake = scripted(&body);
    let ran = run_on(&fake, &[&format!("{REPO}@v1..v2"), "a.go"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    let doc = document(&fake);
    assert_eq!(doc.matches("x: path(path: $path) { __typename }").count(), 2);

    for path in ["*.go", ":!a.go", "src/[ab].go"] {
        let fake = scripted(&body);
        let ran = run_on(&fake, &[&format!("{REPO}@v1..v2"), path]);
        assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
        assert!(!document(&fake).contains("x:"), "{path}");
        assert_eq!(variables(&fake)["path"], path);
    }

    let fake = scripted(&counted);
    let ran = run_on(&fake, &[&format!("{REPO}@v1..v2")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert!(!document(&fake).contains("x:"));

    let fake = scripted(&rev_body(&nodes(1), false, &no_tags()));
    let ran = run_on(&fake, &[&format!("{REPO}@v1"), "a.go"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert!(!document(&fake).contains("x:"));
}

#[test]
fn missing_sides() {
    let both_gone = |a: bool, b: bool| {
        let mut body = range_body(Some(0), &[], false, &no_tags());
        if !a {
            body["data"]["a0"]["commit"] = Value::Null;
        }
        if !b {
            body["data"]["b0"]["commit"] = Value::Null;
        }
        body["data"]["c0"] = Value::Null;
        body["data"]["n0"]["commit"] = Value::Null;
        let named = if b { "v1" } else { "v2" };
        let mut errors = vec![json!({
            "message": format!("revision not found: repo 1700 @{named}^0"),
            "path": ["c0", "comparison"],
        })];
        if !a && b {
            errors.push(json!({
                "message": "revision not found",
                "path": ["n0", "commit", "behindAhead"],
            }));
        }
        body["errors"] = Value::Array(errors);
        run_on(&scripted(&body), &[&format!("{REPO}@v1..v2")])
    };
    failed(
        &both_gone(false, true),
        &format!("base revision v1 not found in {REPO}"),
    );
    failed(
        &both_gone(true, false),
        &format!("head revision v2 not found in {REPO}"),
    );
    failed(
        &both_gone(false, false),
        &format!("revisions v1 and v2 not found in {REPO}"),
    );

    let mut body = rev_body(&[], false, &no_tags());
    body["data"]["r0"]["commit"] = Value::Null;
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v9")]);
    failed(&ran, &format!("revision v9 not found in {REPO}"));

    let ran = run_on(&scripted(&body), &[REPO]);
    failed(&ran, &format!("revision HEAD not found in {REPO}"));
}

#[test]
fn an_empty_repository() {
    let mut body = rev_body(&[], false, &no_tags());
    body["data"]["r0"]["commit"] = Value::Null;
    body["data"]["r0"]["defaultBranch"] = Value::Null;
    failed(
        &run_on(&scripted(&body), &[REPO]),
        &format!("{REPO} has no commits"),
    );

    let mut body = range_body(Some(0), &[], false, &no_tags());
    for alias in ["a0", "b0"] {
        body["data"][alias]["commit"] = Value::Null;
        body["data"][alias]["defaultBranch"] = Value::Null;
    }
    body["data"]["c0"] = Value::Null;
    body["errors"] = json!([{
        "message": "revision not found: repo 714773 @v2^0",
        "path": ["c0", "comparison"],
    }]);
    failed(
        &run_on(&scripted(&body), &[&format!("{REPO}@v1..v2")]),
        &format!("{REPO} has no commits"),
    );
}

#[test]
fn missing_path() {
    let body = rev_body(&[], false, &no_tags());
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v1:nope.go")]);
    failed(&ran, &format!("nope.go not found in {REPO}@{}", &oid(1)[..12]));

    let nothing = |path: &str| {
        let ran = run_on(&scripted(&body), &[&format!("{REPO}@v1"), path]);
        assert_eq!(ran.code, ExitCode::SUCCESS, "{path}: {}", err(&ran));
        out(&ran)
    };
    assert!(nothing("*.nope").ends_with("0 commits  rev v1\n"));
    assert!(nothing(":!nope").ends_with("0 commits  rev v1\n"));

    let found = |at_a: bool, at_b: bool, commits: usize| {
        let mut body = range_body(None, &nodes(commits), false, &no_tags());
        let x = json!({"__typename": "GitBlob"});
        body["data"]["a0"]["commit"]["x"] =
            if at_a { x.clone() } else { Value::Null };
        body["data"]["b0"]["commit"]["x"] = if at_b { x } else { Value::Null };
        run_on(&scripted(&body), &[&format!("{REPO}@v1..v2"), "nope.go"])
    };
    failed(
        &found(false, false, 0),
        &format!("nope.go not found in {REPO}@{A12} or @{B12}"),
    );
    for (at_a, at_b) in [(true, false), (false, true), (true, true)] {
        let ran = found(at_a, at_b, 0);
        assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
        assert!(out(&ran).contains("  0 commits  "));
    }

    let mut body = range_body(None, &[], false, &no_tags());
    body["data"]["c0"] = Value::Null;
    body["errors"] = json!([{"message": "x", "path": ["c0", "comparison"]}]);
    for alias in ["a0", "b0"] {
        body["data"][alias]["commit"]["x"] = Value::Null;
    }
    let ran =
        run_on(&scripted(&body), &[&format!("{REPO}@v1..v2"), "nope.go"]);
    failed(&ran, "Sourcegraph failed to answer the comparison");

    let mut body = range_body(None, &[], false, &no_tags());
    body["errors"] = json!([{"message": "x", "path": ["t0", "gitRefs"]}]);
    body["data"]["t0"]["gitRefs"] = Value::Null;
    for alias in ["a0", "b0"] {
        body["data"][alias]["commit"]["x"] = Value::Null;
    }
    let ran =
        run_on(&scripted(&body), &[&format!("{REPO}@v1..v2"), "nope.go"]);
    failed(&ran, &format!("nope.go not found in {REPO}@{A12} or @{B12}"));
}

#[test]
fn revision_root_errors() {
    let mut body = rev_body(&nodes(1), false, &no_tags());
    body["errors"] = json!([{
        "message": "git command [git log --secret] failed",
        "path": ["r0", "commit", "ancestors", "nodes"],
    }]);
    body["data"]["r0"]["commit"] = Value::Null;
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v1:src")]);
    failed(&ran, "Sourcegraph failed to answer the commit list");
    assert!(!err(&ran).contains("secret"));

    let mut body = rev_body(&nodes(1), false, &no_tags());
    body["errors"] = json!([{"message": "x", "path": ["r0", "commit"]}]);
    body["data"]["r0"]["commit"] = Value::Null;
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v1")]);
    failed(&ran, "Sourcegraph failed to answer the revision lookup");

    for (alias, path) in
        [("a0", json!(["a0", "commit"])), ("b0", json!(["b0", "commit", "x"]))]
    {
        let mut body = range_body(None, &nodes(1), false, &no_tags());
        body["errors"] = json!([{"message": "x", "path": path}]);
        if alias == "a0" {
            body["data"]["a0"]["commit"] = Value::Null;
        } else {
            body["data"]["b0"]["commit"]["x"] = Value::Null;
        }
        let ran =
            run_on(&scripted(&body), &[&format!("{REPO}@v1..v2"), "a.go"]);
        failed(&ran, "Sourcegraph failed to answer the revision lookup");
    }
}

#[test]
fn part_errors() {
    let cases = [
        ("c0 root", json!(["c0"]), "c0", "the comparison"),
        (
            "commits",
            json!(["c0", "comparison", "commits", "nodes"]),
            "c0",
            "the comparison",
        ),
        (
            "ahead",
            json!(["n0", "commit", "behindAhead"]),
            "n0",
            "the commit count",
        ),
        ("n0 root", json!(["n0"]), "n0", "the commit count"),
        ("tags", json!(["t0", "gitRefs"]), "t0", "the tag list"),
        ("t0 root", json!(["t0"]), "t0", "the tag list"),
    ];
    for (name, path, alias, part) in cases {
        let mut body = range_body(Some(1), &nodes(1), false, &no_tags());
        body["errors"] = json!([{"message": "secret internal", "path": path}]);
        if path.as_array().map(Vec::len) == Some(1) {
            body["data"][alias] = Value::Null;
        }
        let ran = run_on(&scripted(&body), &[&format!("{REPO}@v1..v2")]);
        failed(&ran, &format!("Sourcegraph failed to answer {part}"));
        assert!(!err(&ran).contains("secret"), "{name}");
    }

    let mut body = rev_body(&nodes(1), false, &no_tags());
    body["errors"] = json!([{"message": "secret", "path": ["t0", "gitRefs"]}]);
    body["data"]["t0"]["gitRefs"] = Value::Null;
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v1")]);
    failed(&ran, "Sourcegraph failed to answer the tag list");

    let mut body = range_body(None, &nodes(1), false, &no_tags());
    body["data"]["n0"] = Value::Null;
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v1..v2")]);
    failed(&ran, "Sourcegraph failed to answer the commit count");
}

#[test]
fn first_by_mode() {
    let body = range_body(Some(1), &nodes(1), false, &no_tags());
    let fake = scripted(&body);
    let ran = run_on(&fake, &[&format!("{REPO}@v1..v2")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    let doc = document(&fake);
    assert!(doc.contains("commits(first: 151, path: $path)"), "{doc}");
    assert!(doc.contains("gitRefs(type: GIT_TAG, first: 1000)"), "{doc}");
    assert!(!doc.contains("x:"));

    let fake = scripted(&body);
    let ran = run_on(&fake, &["--json", &format!("{REPO}@v1..v2")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    let doc = document(&fake);
    assert!(doc.contains("commits(path: $path)"), "{doc}");
    assert!(!doc.contains("first: 151"), "{doc}");

    for json in [false, true] {
        let body = rev_body(&nodes(1), false, &no_tags());
        let fake = scripted(&body);
        let mut words = vec![];
        if json {
            words.push("--json");
        }
        let at = format!("{REPO}@v1");
        words.push(&at);
        let ran = run_on(&fake, &words);
        assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
        let doc = document(&fake);
        assert!(doc.contains("ancestors(first: 151, path: $path)"), "{doc}");
        assert!(!doc.contains("x:"));
    }
}

#[test]
fn long_subjects_are_cut() {
    let subject = "s".repeat(533);
    let body = rev_body(&[node(1, &subject)], false, &no_tags());
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v1")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    let text = out(&ran);
    let line = text.lines().nth(1).unwrap_or_default();
    let prefix = format!("{}  2026-09-14  Ada Example  ", &oid(1)[..12]);
    let shown = line.strip_prefix(&prefix).unwrap_or_default();
    let kept = format!("{}…", "s".repeat(500));
    assert!(shown.starts_with(&kept), "{shown}");
    assert!(
        shown.ends_with("(+33 more columns; --json has the full line)"),
        "{shown}"
    );
    assert_eq!(text.lines().count(), 2);

    let ran = run_on(&scripted(&body), &["--json", &format!("{REPO}@v1")]);
    assert_eq!(json_out(&ran)["commits"][0]["subject"], subject);
}
