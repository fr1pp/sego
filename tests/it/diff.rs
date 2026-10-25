use std::path::Path;
use std::process::ExitCode;

use serde_json::{Value, json};

use sego::{Endpoint, Reply, Transport};

use crate::support::exec::{Ran, exec};
use crate::support::fake::Fake;
use crate::support::fixture::load_all;

const REPO: &str = "example.com/selftest/diff";
const OID_A: &str = "a7962380ca08b5a188038c69871b8d3fbdf31e89";
const OID_B: &str = "c5c6c98bc25355028a63748a498942a6398ccd22";
const A12: &str = "a7962380ca08";
const B12: &str = "c5c6c98bc253";

fn fixtures() -> Fake {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/Diff");
    Fake::fixtures(load_all(&root))
}

fn run_on(fake: &Fake, words: &[&str]) -> Ran {
    let handle = fake.clone();
    let connect =
        move |_: Endpoint| -> Box<dyn Transport> { Box::new(handle.clone()) };
    let mut argv = vec!["diff"];
    argv.extend_from_slice(words);
    exec(&argv, None, &connect)
}

fn diff(words: &[&str]) -> (Ran, Fake) {
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

fn root(name: &str, key: &str, value: &Value) -> Value {
    let mut root = json!({
        "name": name,
        "defaultBranch": {"displayName": "main"},
    });
    root[key] = value.clone();
    root
}

fn commit(oid: &str, parents: Option<&[&str]>) -> Value {
    let mut commit = json!({"oid": oid});
    if let Some(parents) = parents {
        let parents: Vec<Value> =
            parents.iter().map(|oid| json!({"oid": oid})).collect();
        commit["parents"] = Value::Array(parents);
    }
    commit
}

fn with_paths(mut commit: Value, found: &[bool]) -> Value {
    for (n, found) in found.iter().enumerate() {
        commit[format!("p{n}")] = if *found {
            json!({"__typename": "GitBlob"})
        } else {
            Value::Null
        };
    }
    commit
}

fn node(
    old: Option<&str>,
    new: Option<&str>,
    stat: (u64, u64),
    base: Option<&str>,
) -> Value {
    json!({
        "oldPath": old,
        "newPath": new,
        "stat": {"added": stat.0, "deleted": stat.1},
        "oldFile": base.map(|oid| json!({"commit": {"oid": oid}})),
    })
}

fn modified(path: &str, stat: (u64, u64)) -> Value {
    node(Some(path), Some(path), stat, Some(OID_A))
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

fn document_of(a: &Value, b: &Value, c: &Value) -> Value {
    json!({"data": {
        "a0": root(REPO, "commit", a),
        "b0": root(REPO, "commit", b),
        "c0": root(REPO, "comparison", c),
    }})
}

fn answer(nodes: &[Value], stat: (u64, u64)) -> Value {
    document_of(
        &commit(OID_A, None),
        &commit(OID_B, Some(&["15a353a636720571d19e37b34a14499c3afa9991"])),
        &comparison(nodes.len() as u64, stat, nodes),
    )
}

#[test]
fn diff_text_and_json() {
    let (ran, _) = diff(&[&format!("{REPO}@v1.7.0...v1.7.1")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(ran.err, b"");
    assert_eq!(
        out(&ran),
        format!(
            "{REPO}@{A12}...{B12}  4 files  +13 -6  base {A12}  \
             rev v1.7.0...v1.7.1\n\
             M  +2 -2  .travis.yml\n\
             M  +6 -0  mux.go\n\
             M  +4 -3  mux_test.go\n\
             M  +1 -1  route.go\n"
        )
    );

    let (ran, _) = diff(&["--json", &format!("{REPO}@v1.7.0...v1.7.1")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(ran.err, b"");
    let file = |path: &str, added: u32, deleted: u32| {
        format!(r#"{{"status":"M","old_path":"{path}","new_path":"{path}","#)
            + &format!(r#""added":{added},"deleted":{deleted}}}"#)
    };
    assert_eq!(
        out(&ran),
        format!(r#"{{"repo":"{REPO}","rev_a":"v1.7.0","rev_b":"v1.7.1","#)
            + &format!(
                r#""commit_a":"{OID_A}","commit_b":"{OID_B}","base":"{OID_A}","#
            )
            + &format!(
                r#""files_total":4,"added":13,"deleted":6,"files":[{},{},{},{}],"#,
                file(".travis.yml", 2, 2),
                file("mux.go", 6, 0),
                file("mux_test.go", 4, 3),
                file("route.go", 1, 1),
            )
            + "\"patch\":null}\n"
    );
}

#[test]
fn patch_text_and_json() {
    let words = [&*format!("{REPO}@v1.7.0...v1.7.1"), "mux.go"];
    let (ran, fake) = diff(&words);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    let vars = variables(&fake);
    assert_eq!(vars["paths"], json!(["mux.go"]));
    assert_eq!(vars["path0"], "mux.go");
    let text = out(&ran);
    let (head, patch) = text.split_once("\n\n").unwrap_or_default();
    assert_eq!(
        head,
        format!(
            "{REPO}@{A12}...{B12}:mux.go  1 file  +6 -0  base {A12}  \
             rev v1.7.0...v1.7.1\n\
             M  +6 -0  mux.go"
        )
    );
    assert!(
        patch.starts_with(
            "    diff --git mux.go mux.go\n    index 8aca972d2f61"
        )
    );
    assert!(patch.contains("\n    --- mux.go\n    +++ mux.go\n"));
    assert!(patch.contains("\n    @@ -283,6 +283,12 @@ func (r *Router)"));
    assert!(patch.contains("\n     \treturn route\n"));
    assert!(patch.ends_with("URL path.\n"));
    assert!(patch.lines().all(|line| line.starts_with("    ")));

    let (ran, _) = diff(&["--json", words[0], words[1]]);
    let json = json_out(&ran);
    let rawdiff = json["patch"].as_str().unwrap_or_default();
    assert!(rawdiff.starts_with("diff --git mux.go mux.go\nindex 8aca"));
    assert!(rawdiff.ends_with("URL path.\n"));
    assert!(rawdiff.contains("\n \treturn route\n"));
}

#[test]
fn a_path_in_the_address_is_an_operand() {
    let (ran, fake) = diff(&[&format!("{REPO}@v1.7.0...v1.7.1:mux.go")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(variables(&fake)["paths"], json!(["mux.go"]));
    assert!(
        out(&ran).starts_with(&format!("{REPO}@{A12}...{B12}:mux.go  1 file")),
        "{}",
        out(&ran)
    );
}

#[test]
fn the_header_names_the_paths() {
    let mut body = answer(&[modified("x.go", (1, 1))], (1, 1));
    body["data"]["a0"]["commit"] =
        with_paths(commit(OID_A, None), &[true, true]);
    body["data"]["b0"]["commit"] =
        with_paths(commit(OID_B, Some(&[OID_A])), &[true, true]);
    body["data"]["c0"]["comparison"]["patch"] = json!({"rawDiff": ""});
    let header = |words: &[&str]| {
        let ran = run_on(&scripted(&body), words);
        assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
        out(&ran).lines().next().unwrap_or_default().to_owned()
    };
    let counts = "1 file  +1 -1";
    let tail = format!("base {A12}  rev v1");
    assert_eq!(
        header(&[&format!("{REPO}@v1"), "x.go"]),
        format!("{REPO}@{A12}...{B12}:x.go  {counts}  {tail}")
    );
    assert_eq!(
        header(&[&format!("{REPO}@v1:x.go"), "x.go", "x.go"]),
        format!("{REPO}@{A12}...{B12}:x.go  {counts}  {tail}")
    );
    assert_eq!(
        header(&[&format!("{REPO}@v1"), "*.md"]),
        format!("'{REPO}@{A12}...{B12}:*.md'  {counts}  {tail}")
    );
    assert_eq!(
        header(&[&format!("{REPO}@v1:x.go"), "dir/a b.go"]),
        format!(
            "{REPO}@{A12}...{B12}  {counts}  paths x.go 'dir/a b.go'  {tail}"
        )
    );
    assert_eq!(
        header(&[&format!("{REPO}@v1...v2"), "x.go", "*.md", "y/z.go"]),
        format!(
            "{REPO}@{A12}...{B12}  {counts}  paths x.go '*.md' y/z.go  \
             {tail}...v2"
        )
    );
}

#[test]
fn status_letters() {
    let nodes = [
        node(None, Some("new.go"), (5, 0), None),
        node(Some("old.go"), None, (0, 7), Some(OID_A)),
        node(Some("a.go"), Some("b.go"), (1, 1), Some(OID_A)),
        modified("c.go", (3, 4)),
    ];
    let fake = scripted(&answer(&nodes, (9, 12)));
    let ran = run_on(&fake, &[&format!("{REPO}@v1")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(
        out(&ran),
        format!(
            "{REPO}@{A12}...{B12}  4 files  +9 -12  base {A12}  rev v1\n\
             A  +5 -0  new.go\n\
             D  +0 -7  old.go\n\
             R  +1 -1  a.go -> b.go\n\
             M  +3 -4  c.go\n"
        )
    );
    let vars = variables(&fake);
    assert_eq!(vars["a"], "v1^1");
    assert_eq!(vars["b"], "v1");

    let fake = scripted(&answer(&nodes, (9, 12)));
    let ran = run_on(&fake, &["--json", &format!("{REPO}@v1")]);
    let json = json_out(&ran);
    assert_eq!(json["rev_a"], Value::Null);
    assert_eq!(json["rev_b"], "v1");
    let statuses: Vec<&str> = json["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|file| file["status"].as_str())
        .collect();
    assert_eq!(statuses, ["A", "D", "R", "M"]);
    assert_eq!(json["files"][0]["old_path"], Value::Null);
    assert_eq!(json["files"][1]["new_path"], Value::Null);
    assert_eq!(json["base"], OID_A);
}

#[test]
fn the_rev_field_follows_the_revision_as_written() {
    let body = answer(&[modified("c.go", (1, 1))], (1, 1));
    let header = |word: &str| {
        let ran = run_on(&scripted(&body), &[word]);
        assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
        out(&ran).lines().next().unwrap_or_default().to_owned()
    };
    let base = format!("{REPO}@{A12}...{B12}  1 file  +1 -1  base {A12}");
    assert_eq!(header(&format!("{REPO}@v1")), format!("{base}  rev v1"));
    assert_eq!(header(&format!("{REPO}@c5c6c98")), base);
    assert_eq!(
        header(&format!("{REPO}@abc1234...v2")),
        format!("{base}  rev abc1234...v2")
    );
    assert_eq!(header(&format!("{REPO}@abc1234...def5678")), base);
}

#[test]
fn a_rename() {
    let nodes = [node(Some("old.go"), Some("new.go"), (1, 1), Some(OID_A))];
    let raw = "diff --git old.go new.go\nsimilarity index 98%\n\
               rename from old.go\nrename to new.go\nindex 193c5ce..2875a0f \
               100644\n--- old.go\n+++ new.go\n@@ -1,4 +1,4 @@\n-package a\n\
               +package b\n";
    let mut body = answer(&nodes, (1, 1));
    body["data"]["a0"]["commit"] =
        with_paths(commit(OID_A, None), &[true, false]);
    body["data"]["b0"]["commit"] =
        with_paths(commit(OID_B, Some(&[OID_A])), &[false, true]);
    body["data"]["c0"]["comparison"]["patch"] = json!({"rawDiff": raw});
    let fake = scripted(&body);
    let ran = run_on(&fake, &[&format!("{REPO}@v1...v2"), "old.go", "new.go"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    let vars = variables(&fake);
    assert_eq!(vars["paths"], json!(["old.go", "new.go"]));
    assert_eq!(
        (&vars["path0"], &vars["path1"]),
        (&json!("old.go"), &json!("new.go"))
    );
    let doc = document(&fake);
    assert!(doc.contains("p0: path(path: $path0) { __typename }"), "{doc}");
    assert!(doc.contains("p1: path(path: $path1) { __typename }"), "{doc}");
    let text = out(&ran);
    assert_eq!(
        text.lines().take(2).collect::<Vec<_>>(),
        [
            format!(
                "{REPO}@{A12}...{B12}  1 file  +1 -1  paths old.go new.go  \
                 base {A12}  rev v1...v2"
            ),
            "R  +1 -1  old.go -> new.go".to_owned(),
        ]
    );
    assert!(text.contains("\n    rename from old.go\n    rename to new.go\n"));
}

#[test]
fn quoted_paths_and_the_patch_as_served() {
    let raw = "diff --git dir/a b.go dir/a b.go\n\
               index 1111111..2222222 100644\n\
               --- dir/a b.go\n+++ dir/a b.go\n@@ -1,2 +1,2 @@\n\
               --- foo\n+-- bar\n ctx\n+red\u{1b}[31m\n\
               \\ No newline at end of file\n";
    let mut body = answer(&[modified("dir/a b.go", (2, 1))], (2, 1));
    body["data"]["a0"]["commit"] = with_paths(commit(OID_A, None), &[true]);
    body["data"]["b0"]["commit"] =
        with_paths(commit(OID_B, Some(&[OID_A])), &[true]);
    body["data"]["c0"]["comparison"]["patch"] = json!({"rawDiff": raw});

    let ran =
        run_on(&scripted(&body), &[&format!("{REPO}@v1...v2"), "dir/a b.go"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(
        out(&ran),
        format!(
            "'{REPO}@{A12}...{B12}:dir/a b.go'  1 file  +2 -1  base {A12}  \
             rev v1...v2\n\
             M  +2 -1  'dir/a b.go'\n\
             \n\
             \x20   diff --git dir/a b.go dir/a b.go\n\
             \x20   index 1111111..2222222 100644\n\
             \x20   --- dir/a b.go\n\
             \x20   +++ dir/a b.go\n\
             \x20   @@ -1,2 +1,2 @@\n\
             \x20   --- foo\n\
             \x20   +-- bar\n\
             \x20    ctx\n\
             \x20   +red\\u{{1b}}[31m\n\
             \x20   \\ No newline at end of file\n"
        )
    );

    let ran = run_on(
        &scripted(&body),
        &["--json", &format!("{REPO}@v1...v2"), "dir/a b.go"],
    );
    let json = json_out(&ran);
    assert_eq!(json["patch"], raw);
    assert_eq!(json["files"][0]["new_path"], "dir/a b.go");
    assert!(!String::from_utf8_lossy(&ran.out).contains('\u{1b}'));
}

#[test]
fn limit_at_200() {
    let nodes: Vec<Value> =
        (0..201).map(|n| modified(&format!("f{n:03}.go"), (1, 0))).collect();
    let body = document_of(
        &commit(OID_A, None),
        &commit(OID_B, Some(&[OID_A])),
        &comparison(13366, (13366, 0), &nodes),
    );
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v1...v2")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    let text = out(&ran);
    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some(
            format!(
                "{REPO}@{A12}...{B12}  200 of 13366 files shown  +13366 -0  \
                 base {A12}  rev v1...v2"
            )
            .as_str()
        )
    );
    let rows: Vec<&str> = lines.collect();
    assert_eq!(rows.len(), 200);
    assert_eq!(rows[199], "M  +1 -0  f199.go");

    let ran =
        run_on(&scripted(&body), &["--json", &format!("{REPO}@v1...v2")]);
    let json = json_out(&ran);
    assert_eq!(json["files"].as_array().map(Vec::len), Some(201));
    assert_eq!(json["files_total"], 13366);

    let exact = &nodes[..200];
    let body = document_of(
        &commit(OID_A, None),
        &commit(OID_B, Some(&[OID_A])),
        &comparison(200, (200, 0), exact),
    );
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v1...v2")]);
    let text = out(&ran);
    assert!(text.contains("  200 files  +200 -0  "), "{text}");
    assert_eq!(text.lines().count(), 201);
}

#[test]
fn units_follow_the_total() {
    let body = answer(&[modified("a.go", (1, 1))], (1, 1));
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v1...v2")]);
    assert!(out(&ran).contains("  1 file  +1 -1  "));
}

#[test]
fn base_unknown() {
    let nodes = [
        node(None, Some("a.go"), (3, 0), None),
        node(None, Some("b.go"), (4, 0), None),
    ];
    let body = answer(&nodes, (7, 0));
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v1...v2")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(
        out(&ran),
        format!(
            "{REPO}@{A12}...{B12}  2 files  +7 -0  rev v1...v2\n\
             A  +3 -0  a.go\n\
             A  +4 -0  b.go\n"
        )
    );
    let ran =
        run_on(&scripted(&body), &["--json", &format!("{REPO}@v1...v2")]);
    assert_eq!(json_out(&ran)["base"], Value::Null);

    let body = answer(&[], (0, 0));
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v1...v2")]);
    assert_eq!(
        out(&ran),
        format!("{REPO}@{A12}...{B12}  0 files  +0 -0  rev v1...v2\n")
    );
    let ran =
        run_on(&scripted(&body), &["--json", &format!("{REPO}@v1...v2")]);
    let json = json_out(&ran);
    assert_eq!(json["base"], Value::Null);
    assert_eq!(json["files"], json!([]));
}

#[test]
fn the_base_is_the_first_node_that_has_one() {
    let other = "0123456789abcdef0123456789abcdef01234567";
    let nodes = [
        node(None, Some("a.go"), (3, 0), None),
        node(Some("b.go"), Some("b.go"), (1, 1), Some(other)),
        modified("c.go", (1, 1)),
    ];
    let ran = run_on(
        &scripted(&answer(&nodes, (5, 2))),
        &[&format!("{REPO}@v1...v2")],
    );
    assert!(out(&ran).contains("  base 0123456789ab  "), "{}", out(&ran));
}

#[test]
fn diff_of_a_root_commit() {
    let (ran, fake) = diff(&[&format!("{REPO}@v0.1.0")]);
    failed(&ran, "v0.1.0 is a root commit: Sourcegraph has no diff for it");
    assert_eq!(variables(&fake)["a"], "v0.1.0^1");

    let (ran, _) = diff(&["--json", &format!("{REPO}@v0.1.0")]);
    failed(&ran, "v0.1.0 is a root commit: Sourcegraph has no diff for it");
}

#[test]
fn a_range_is_never_a_root_commit() {
    let body =
        document_of(&Value::Null, &commit(OID_B, Some(&[])), &Value::Null);
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v0...v1")]);
    failed(&ran, &format!("base revision v0 not found in {REPO}"));
}

#[test]
fn missing_sides() {
    let one = |a: &Value, b: &Value| {
        let body = document_of(a, b, &Value::Null);
        run_on(&scripted(&body), &[&format!("{REPO}@v9.9")])
    };
    let missing = format!("revision v9.9 not found in {REPO}");
    failed(&one(&Value::Null, &Value::Null), &missing);
    failed(&one(&commit(OID_A, None), &Value::Null), &missing);

    let range = |a: &Value, b: &Value| {
        let body = document_of(a, b, &Value::Null);
        run_on(&scripted(&body), &[&format!("{REPO}@v9.9.9...v1.9")])
    };
    let (a, b) = (commit(OID_A, None), commit(OID_B, Some(&[OID_A])));
    failed(
        &range(&Value::Null, &b),
        &format!("base revision v9.9.9 not found in {REPO}"),
    );
    failed(
        &range(&a, &Value::Null),
        &format!("head revision v1.9 not found in {REPO}"),
    );
    failed(
        &range(&Value::Null, &Value::Null),
        &format!("revisions v9.9.9 and v1.9 not found in {REPO}"),
    );
}

#[test]
fn an_empty_repository() {
    let mut body = document_of(&Value::Null, &Value::Null, &Value::Null);
    for alias in ["a0", "b0", "c0"] {
        body["data"][alias]["defaultBranch"] = Value::Null;
    }
    body["errors"] = json!([{
        "message": "revision not found: repo 714773 @HEAD",
        "path": ["c0", "comparison"],
    }]);
    for word in [format!("{REPO}@v1"), format!("{REPO}@v1...v2")] {
        let ran = run_on(&scripted(&body), &[&word]);
        failed(&ran, &format!("{REPO} has no commits"));
    }
}

#[test]
fn revision_root_errors() {
    let mut body = answer(&[modified("a.go", (1, 1))], (1, 1));
    body["errors"] = json!([{
        "message": "secret internal text",
        "path": ["a0", "commit"],
    }]);
    body["data"]["a0"]["commit"] = Value::Null;
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v1...v2")]);
    failed(&ran, "Sourcegraph failed to answer the revision lookup");
    assert!(!err(&ran).contains("secret"));

    let mut body = answer(&[modified("a.go", (1, 1))], (1, 1));
    body["errors"] = json!([
        {"message": "x", "path": ["c0", "comparison"]},
        {"message": "y", "path": ["b0", "commit", "p0"]},
    ]);
    body["data"]["b0"]["commit"]["p0"] = Value::Null;
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v1...v2"), "a.go"]);
    failed(&ran, "Sourcegraph failed to answer the revision lookup");
}

#[test]
fn missing_path() {
    let found = |at_a: bool, at_b: bool| {
        let mut body = answer(&[], (0, 0));
        body["data"]["a0"]["commit"] =
            with_paths(commit(OID_A, None), &[at_a]);
        body["data"]["b0"]["commit"] =
            with_paths(commit(OID_B, Some(&[OID_A])), &[at_b]);
        body["data"]["c0"]["comparison"]["patch"] = json!({"rawDiff": ""});
        run_on(&scripted(&body), &[&format!("{REPO}@v1...v2"), "nope.go"])
    };
    failed(
        &found(false, false),
        &format!("nope.go not found in {REPO}@{A12} or @{B12}"),
    );
    assert_eq!(found(true, false).code, ExitCode::SUCCESS);
    assert_eq!(found(false, true).code, ExitCode::SUCCESS);
}

#[test]
fn the_second_plain_path_is_checked_too() {
    let mut body = answer(&[], (0, 0));
    body["data"]["a0"]["commit"] =
        with_paths(commit(OID_A, None), &[true, false]);
    body["data"]["b0"]["commit"] =
        with_paths(commit(OID_B, Some(&[OID_A])), &[true, false]);
    body["data"]["c0"]["comparison"]["patch"] = json!({"rawDiff": ""});
    let ran = run_on(
        &scripted(&body),
        &[&format!("{REPO}@v1...v2"), "here.go", "gone.go"],
    );
    failed(&ran, &format!("gone.go not found in {REPO}@{A12} or @{B12}"));
}

#[test]
fn a_missing_path_outranks_a_comparison_error() {
    let mut body = answer(&[], (0, 0));
    body["data"]["a0"]["commit"] = with_paths(commit(OID_A, None), &[false]);
    body["data"]["b0"]["commit"] =
        with_paths(commit(OID_B, Some(&[OID_A])), &[false]);
    body["data"]["c0"] = Value::Null;
    body["errors"] = json!([{"message": "x", "path": ["c0", "comparison"]}]);
    let ran =
        run_on(&scripted(&body), &[&format!("{REPO}@v1...v2"), "nope.go"]);
    failed(&ran, &format!("nope.go not found in {REPO}@{A12} or @{B12}"));
}

#[test]
fn unchanged_path() {
    let mut body = answer(&[], (0, 0));
    body["data"]["a0"]["commit"] = with_paths(commit(OID_A, None), &[true]);
    body["data"]["b0"]["commit"] =
        with_paths(commit(OID_B, Some(&[OID_A])), &[true]);
    body["data"]["c0"]["comparison"]["patch"] = json!({"rawDiff": ""});
    let ran =
        run_on(&scripted(&body), &[&format!("{REPO}@v1...v2"), "same.go"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(ran.err, b"");
    assert_eq!(
        out(&ran),
        format!("{REPO}@{A12}...{B12}:same.go  0 files  +0 -0  rev v1...v2\n")
    );
    let ran = run_on(
        &scripted(&body),
        &["--json", &format!("{REPO}@v1...v2"), "same.go"],
    );
    let json = json_out(&ran);
    assert_eq!(json["files_total"], 0);
    assert_eq!(json["patch"], "");
}

#[test]
fn globs_and_magic_skip_existence() {
    let body = answer(&[modified("mux.go", (1, 1))], (1, 1));
    let words =
        ["*.go", ":!mux_test.go", "a?.go", "src/[ab]/x", ":(glob)**/*.go"];
    let mut argv = vec![format!("{REPO}@v1...v2")];
    argv.extend(words.iter().map(|word| (*word).to_owned()));
    let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
    let mut body = body;
    body["data"]["c0"]["comparison"]["patch"] = json!({"rawDiff": ""});
    let fake = scripted(&body);
    let ran = run_on(&fake, &argv);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    let vars = variables(&fake);
    assert_eq!(vars["paths"], json!(words));
    let names: Vec<&String> =
        vars.as_object().map(|vars| vars.keys().collect()).unwrap_or_default();
    assert_eq!(names, ["a", "b", "paths", "repo0"]);
    let doc = document(&fake);
    assert!(!doc.contains("path(path:"), "{doc}");
    assert!(!doc.contains("$path0"), "{doc}");
}

#[test]
fn plain_paths_are_numbered_among_themselves() {
    let mut body = answer(&[], (0, 0));
    body["data"]["a0"]["commit"] =
        with_paths(commit(OID_A, None), &[true, true]);
    body["data"]["b0"]["commit"] =
        with_paths(commit(OID_B, Some(&[OID_A])), &[true, true]);
    body["data"]["c0"]["comparison"]["patch"] = json!({"rawDiff": ""});
    let fake = scripted(&body);
    let ran =
        run_on(&fake, &[&format!("{REPO}@v1...v2"), "x.go", "*.md", "y/z.go"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    let vars = variables(&fake);
    assert_eq!(vars["paths"], json!(["x.go", "*.md", "y/z.go"]));
    assert_eq!(vars["path0"], "x.go");
    assert_eq!(vars["path1"], "y/z.go");
    assert!(vars.get("path2").is_none());
    let doc = document(&fake);
    assert!(doc.contains("$path0: String!"), "{doc}");
    assert!(doc.contains("$path1: String!"), "{doc}");
    assert!(doc.contains("$paths: [String!]"), "{doc}");
}

#[test]
fn first_by_mode() {
    let body = answer(&[modified("mux.go", (1, 1))], (1, 1));
    let text = scripted(&body);
    run_on(&text, &[&format!("{REPO}@v1...v2")]);
    let doc = document(&text);
    assert!(doc.contains("page: fileDiffs(first: 201, paths: $paths)"));
    assert!(doc.contains("all: fileDiffs(first: 0, paths: $paths)"));
    assert!(!doc.contains("patch:"), "{doc}");
    assert_eq!(variables(&text)["paths"], Value::Null);
    assert_eq!(variables(&text)["a"], "v1");
    assert_eq!(variables(&text)["b"], "v2");
    assert_eq!(variables(&text)["repo0"], REPO);

    let json = scripted(&body);
    run_on(&json, &["--json", &format!("{REPO}@v1...v2")]);
    let doc = document(&json);
    assert!(doc.contains("page: fileDiffs(paths: $paths)"), "{doc}");
    assert!(!doc.contains("first: 201"), "{doc}");
    assert!(doc.contains("all: fileDiffs(first: 0, paths: $paths)"));
}

#[test]
fn the_patch_is_requested_only_with_paths() {
    let mut body = answer(&[modified("mux.go", (1, 1))], (1, 1));
    body["data"]["a0"]["commit"] = with_paths(commit(OID_A, None), &[true]);
    body["data"]["b0"]["commit"] =
        with_paths(commit(OID_B, Some(&[OID_A])), &[true]);
    body["data"]["c0"]["comparison"]["patch"] = json!({"rawDiff": "x\n"});
    for flag in [None, Some("--json")] {
        let fake = scripted(&body);
        let mut words = vec![];
        words.extend(flag);
        let address = format!("{REPO}@v1...v2");
        words.extend([address.as_str(), "mux.go"]);
        run_on(&fake, &words);
        let doc = document(&fake);
        assert!(doc.contains("patch: fileDiffs(paths: $paths) { rawDiff }"));
        assert!(doc.contains("a0: repository(name: $repo0)"));
        assert!(doc.contains("b0: repository(name: $repo0)"));
        assert!(doc.contains("c0: repository(name: $repo0)"));
        assert!(doc.contains("comparison(base: $a, head: $b)"));
        assert!(doc.contains("parents { oid }"));
    }
}

#[test]
fn comparison_errors() {
    let secret = "git command [git diff --secret-flag] failed";
    let cases = [
        ("c0 root", json!(["c0"]), true),
        ("comparison", json!(["c0", "comparison"]), true),
        ("page nodes", json!(["c0", "comparison", "page", "nodes"]), false),
        ("total", json!(["c0", "comparison", "all", "totalCount"]), false),
    ];
    for (name, path, nulls) in cases {
        let mut body = answer(&[modified("a.go", (1, 1))], (1, 1));
        body["errors"] = json!([{"message": secret, "path": path}]);
        if nulls {
            body["data"]["c0"] = Value::Null;
        }
        let ran = run_on(&scripted(&body), &[&format!("{REPO}@v1...v2")]);
        failed(&ran, "Sourcegraph failed to answer the comparison");
        assert!(!err(&ran).contains("secret"), "{name}");
    }
}

#[test]
fn a_null_comparison_root_is_a_failure() {
    let mut body = answer(&[], (0, 0));
    body["data"]["c0"] = Value::Null;
    let ran = run_on(&scripted(&body), &[&format!("{REPO}@v1...v2")]);
    failed(&ran, "Sourcegraph failed to answer the comparison");
}

#[test]
fn the_repository_must_exist() {
    let body = json!({"data": {"a0": null, "b0": null, "c0": null}});
    let ran = run_on(&scripted(&body), &["example.com/x/y@v1...v2"]);
    assert_eq!(ran.code, ExitCode::from(1));
    assert!(err(&ran).starts_with(
        "sego: error: example.com/x/y is not on sourcegraph.com\n"
    ));
}

#[test]
fn owner_name_candidates() {
    let gitlab = "gitlab.com/o/n";
    let mut body = json!({"data": {"a0": null, "b0": null, "c0": null}});
    let live = answer(&[modified("a.go", (1, 1))], (1, 1));
    for (alias, canonical) in [("a1", "a0"), ("b1", "b0"), ("c1", "c0")] {
        let mut root = live["data"][canonical].clone();
        root["name"] = json!(gitlab);
        body["data"][alias] = root;
    }
    let fake = scripted(&body);
    let ran = run_on(&fake, &["o/n@v1...v2"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert!(out(&ran).starts_with(&format!("{gitlab}@{A12}...{B12}  1 file")));
    let doc = document(&fake);
    assert!(doc.contains("a1: repository(name: $repo1)"));
    assert_eq!(variables(&fake)["repo1"], "gitlab.com/o/n");
}

#[test]
fn paths_are_escaped_in_rows() {
    let nodes = [node(
        Some("a\u{1b}[31m\nb.go"),
        Some("a\u{1b}[31m\nb.go"),
        (1, 1),
        Some(OID_A),
    )];
    let ran = run_on(
        &scripted(&answer(&nodes, (1, 1))),
        &[&format!("{REPO}@v1...v2")],
    );
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert!(!out(&ran).contains('\u{1b}'));
    assert_eq!(out(&ran).lines().count(), 2);
    assert!(out(&ran).contains("$'a\\033[31m\\nb.go'"), "{}", out(&ran));
}
