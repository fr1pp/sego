use std::path::Path;
use std::process::ExitCode;

use serde_json::{Value, json};

use sego::internal::address::{self, Repo, Rev, pinned};
use sego::internal::upstream::{Endpoint, GRAPHQL_PATH, Reply, Transport};

use crate::support::case::shell_words;
use crate::support::exec::{Ran, exec};
use crate::support::fake::Fake;
use crate::support::fixture::load_all;

const MUX: &str = "example.com/selftest/mux";
const MUX_OID: &str = "98cb6bf42e086f6af920b965c38cacc07402d51b";
const BLE: &str = "example.com/selftest/ble";
const BLE_OID: &str = "d81fd54feb0d996fdff20dca27eaf0201f7015cc";
const MUX_AT: &str = "example.com/selftest/mux@98cb6bf42e08";

fn fixtures() -> Fake {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ListTree");
    Fake::fixtures(load_all(&root))
}

fn run_on(fake: &Fake, words: &[&str]) -> Ran {
    let handle = fake.clone();
    let connect =
        move |_: Endpoint| -> Box<dyn Transport> { Box::new(handle.clone()) };
    let mut argv = vec!["ls"];
    argv.extend_from_slice(words);
    exec(&argv, None, &connect)
}

fn ls(words: &[&str]) -> (Ran, Fake) {
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

fn rows(ran: &Ran) -> Vec<String> {
    out(ran).lines().skip(1).map(str::to_owned).collect()
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

fn root_of(entries: &[Value]) -> Value {
    json!({"data": {"r0": {
        "name": MUX,
        "defaultBranch": {"displayName": "main"},
        "c0": {"oid": MUX_OID, "tree": {"entries": entries}}
    }}})
}

fn dir(path: &str) -> Value {
    json!({"path": path, "isDirectory": true, "submodule": null})
}

fn file(path: &str) -> Value {
    json!({"path": path, "isDirectory": false, "submodule": null})
}

fn module(path: &str, url: &str) -> Value {
    json!({"path": path, "isDirectory": false,
           "submodule": {"url": url, "commit": BLE_OID}})
}

fn failed(ran: &Ran, message: &str) {
    assert_eq!(ran.code, ExitCode::from(1), "{}", err(ran));
    assert_eq!(ran.out, b"");
    assert_eq!(err(ran), format!("sego: error: {message}\n"));
}

#[test]
fn listing_orders_rows_and_serialises_submodules() {
    let (ran, _) = ls(&["example.com/selftest/ble@d81fd54feb0d"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(ran.err, b"");
    let text = out(&ran);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[0],
        "example.com/selftest/ble@d81fd54feb0d  41 entries  12 dirs  \
         28 files  1 submodule"
    );
    assert_eq!(lines.len(), 42);
    let at = |row: &str| {
        lines
            .iter()
            .position(|line| *line == row)
            .unwrap_or_else(|| panic!("no row {row:?}"))
    };
    let archive = at("dir        archive/");
    let ble_pp = at("file       ble.pp");
    let contrib = at(
        "submodule  contrib  example.com/selftest/blesh-contrib@d2109203480a",
    );
    assert!(archive < ble_pp && ble_pp < contrib);

    let (ran, _) = ls(&["--json", "example.com/selftest/ble@d81fd54feb0d"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    let json = json_out(&ran);
    assert_eq!(json["repo"], BLE);
    assert_eq!(json["rev"], "d81fd54feb0d");
    assert_eq!(json["commit"], BLE_OID);
    assert_eq!(json["path"], Value::Null);
    assert_eq!(json["recursive"], false);
    assert_eq!(json["entries"].as_array().map(Vec::len), Some(41));
    let json_text = out(&ran);
    assert!(json_text.contains(
        r#"{"path":"archive","kind":"dir","submodule_url":null,"submodule_target":null,"submodule_commit":null}"#
    ));
    assert!(json_text.contains(
        r#"{"path":"contrib","kind":"submodule","submodule_url":"../blesh-contrib","submodule_target":"example.com/selftest/blesh-contrib@d2109203480a","submodule_commit":"d2109203480a7dfe1dead5f5f8f9f15a9146c90d"}"#
    ));
    assert!(json_text.contains(
        r#"{"path":"ble.pp","kind":"file","submodule_url":null,"submodule_target":null,"submodule_commit":null}"#
    ));
}

#[test]
fn a_tree_url_at_the_root() {
    let mut body = root_of(&[dir("docs"), file("mux.go")]);
    body["data"]["r0"]["c0"]["x0"] = json!({"__typename": "GitTree"});
    let url = "https://github.com/x/y/tree/v1.8.0";

    let fake = scripted(&body);
    let ran = run_on(&fake, &[url]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(ran.err, b"");
    assert_eq!(
        out(&ran),
        "example.com/selftest/mux@98cb6bf42e08  2 entries  1 dir  1 file  \
         rev v1.8.0\n\
         dir        docs/\n\
         file       mux.go\n"
    );
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    let sent: Value = serde_json::from_slice(&calls[0].body)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(sent["variables"]["rev0"], "v1.8.0");
    assert_eq!(sent["variables"]["path_0"], "");
    assert!(sent["query"].as_str().is_some_and(|q| q.contains("x0: path")));

    let ran = run_on(&scripted(&body), &["--json", url]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    let json = json_out(&ran);
    assert_eq!(json["path"], Value::Null);
    assert_eq!(json["rev"], "v1.8.0");
}

#[test]
fn entries_sort_by_segment() {
    let expected = [
        "pkg/foo",
        "pkg/foo/bar.go",
        "pkg/foo/sub",
        "pkg/foo/sub/x.go",
        "pkg/foo-bar.go",
        "pkg/foo.go",
        "pkg/foo_x.go",
    ];
    let (ran, fake) =
        ls(&["--recursive", "example.com/selftest/mux@v1.8.0:pkg"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    let text = out(&ran);
    assert_eq!(
        text.lines().next(),
        Some(
            "example.com/selftest/mux@98cb6bf42e08:pkg  7 entries  2 dirs  \
             5 files  rev v1.8.0"
        )
    );
    let paths: Vec<String> = rows(&ran)
        .iter()
        .map(|row| {
            let words = shell_words(row).unwrap_or_else(|e| panic!("{e}"));
            words[1].trim_end_matches('/').to_owned()
        })
        .collect();
    assert_eq!(paths, expected);

    let (ran, _) =
        ls(&["--json", "--recursive", "example.com/selftest/mux@v1.8.0:pkg"]);
    let json = json_out(&ran);
    let listed: Vec<&str> = json["entries"]
        .as_array()
        .unwrap_or_else(|| panic!("{json}"))
        .iter()
        .filter_map(|entry| entry["path"].as_str())
        .collect();
    assert_eq!(listed, expected);
    assert_eq!(json["recursive"], true);
    assert_eq!(fake.calls().len(), 1);
}

#[test]
fn rows_join_to_the_header() {
    let (ran, _) = ls(&["example.com/selftest/mux@v1.8.0:a b"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(
        out(&ran),
        "'example.com/selftest/mux@98cb6bf42e08:a b'  2 entries  1 dir  \
         1 file  rev v1.8.0\n\
         file       'a b/c.go'\n\
         dir        'a b/d e'/\n"
    );

    let row = &rows(&ran)[0];
    let words = shell_words(row).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(words, ["file", "a b/c.go"]);
    let joined = pinned(MUX, MUX_OID, Some(&words[1])).to_string();
    assert_eq!(joined, "'example.com/selftest/mux@98cb6bf42e08:a b/c.go'");
    let token = shell_words(&joined).unwrap_or_else(|e| panic!("{e}"));
    let parsed = address::parse(&token[0]).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(parsed.repo, Repo::Exact(MUX.to_owned()));
    assert_eq!(parsed.rev, Rev::One("98cb6bf42e08".to_owned()));
    assert_eq!(parsed.path.as_deref(), Some("a b/c.go"));
}

#[test]
fn the_root_three_ways() {
    let mut texts = Vec::new();
    for token in [
        "example.com/selftest/ble@d81fd54feb0d",
        "example.com/selftest/ble@d81fd54feb0d:",
        "example.com/selftest/ble@d81fd54feb0d:.",
    ] {
        let (ran, fake) = ls(&[token]);
        assert_eq!(ran.code, ExitCode::SUCCESS, "{token}: {}", err(&ran));
        let calls = fake.calls();
        assert_eq!(calls.len(), 1, "{token}");
        let sent: Value = serde_json::from_slice(&calls[0].body)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(sent["variables"]["path_0"], "", "{token}");
        assert_eq!(sent["variables"]["parent_0"], "", "{token}");
        assert_eq!(sent["variables"]["recursive_0"], false, "{token}");
        texts.push(out(&ran));

        let (ran, _) = ls(&["--json", token]);
        assert_eq!(json_out(&ran)["path"], Value::Null, "{token}");
    }
    assert_eq!(texts[0], texts[1]);
    assert_eq!(texts[0], texts[2]);
    assert!(
        texts[0].starts_with(
            "example.com/selftest/ble@d81fd54feb0d  41 entries  "
        )
    );
}

#[test]
fn limit_at_200() {
    let files = |n: usize| -> Vec<Value> {
        (0..n).map(|i| file(&format!("f{i:03}"))).collect()
    };
    let address = format!("{MUX}@{MUX_OID}");
    let header = |ran: &Ran| out(ran).lines().next().map(str::to_owned);

    let fake = scripted(&root_of(&files(200)));
    let ran = run_on(&fake, &[&address]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(
        header(&ran).as_deref(),
        Some("example.com/selftest/mux@98cb6bf42e08  200 entries  200 files")
    );
    assert_eq!(rows(&ran).len(), 200);

    for flags in [&[][..], &["--recursive"][..]] {
        let fake = scripted(&root_of(&files(201)));
        let mut words = flags.to_vec();
        words.push(&address);
        let ran = run_on(&fake, &words);
        assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
        assert_eq!(
            header(&ran).as_deref(),
            Some(
                "example.com/selftest/mux@98cb6bf42e08  200 of 201 entries \
                 shown  201 files"
            ),
            "{flags:?}"
        );
        let shown = rows(&ran);
        assert_eq!(shown.len(), 200);
        assert_eq!(shown[199], "file       f199");

        let fake = scripted(&root_of(&files(201)));
        let mut words = vec!["--json"];
        words.extend_from_slice(flags);
        words.push(&address);
        let ran = run_on(&fake, &words);
        assert_eq!(
            json_out(&ran)["entries"].as_array().map(Vec::len),
            Some(201)
        );
    }
}

#[test]
fn counts_are_singular() {
    let entries = [dir("a"), file("b"), module("c", "../c")];
    let fake = scripted(&root_of(&entries));
    let ran = run_on(&fake, &[&format!("{MUX}@{MUX_OID}")]);
    assert_eq!(
        out(&ran).lines().next(),
        Some(
            "example.com/selftest/mux@98cb6bf42e08  3 entries  1 dir  1 file  \
             1 submodule"
        )
    );

    let fake = scripted(&root_of(&[dir("a")]));
    let ran = run_on(&fake, &[&format!("{MUX}@{MUX_OID}")]);
    assert_eq!(
        out(&ran),
        "example.com/selftest/mux@98cb6bf42e08  1 entry  1 dir\n\
         dir        a/\n"
    );
}

#[test]
fn an_empty_directory() {
    let (ran, _) = ls(&["example.com/selftest/mux@v1.8.0:empty"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(
        out(&ran),
        "example.com/selftest/mux@98cb6bf42e08:empty  0 entries  rev v1.8.0\n"
    );
    let (ran, _) = ls(&["--json", "example.com/selftest/mux@v1.8.0:empty"]);
    assert_eq!(json_out(&ran)["entries"], json!([]));
}

#[test]
fn a_submodule_path() {
    let (ran, _) = ls(&["example.com/selftest/ble@d81fd54feb0d:contrib"]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    assert_eq!(ran.err, b"");
    assert_eq!(
        out(&ran),
        "example.com/selftest/ble@d81fd54feb0d:contrib  1 entry  \
         1 submodule\n\
         submodule  contrib  example.com/selftest/blesh-contrib@d2109203480a\n"
    );
    let (ran, _) =
        ls(&["--json", "example.com/selftest/ble@d81fd54feb0d:contrib"]);
    assert_eq!(
        out(&ran),
        concat!(
            r#"{"repo":"example.com/selftest/ble","rev":"d81fd54feb0d","#,
            r#""commit":"d81fd54feb0d996fdff20dca27eaf0201f7015cc","#,
            r#""path":"contrib","recursive":false,"entries":["#,
            r#"{"path":"contrib","kind":"submodule","#,
            r#""submodule_url":"../blesh-contrib","#,
            r#""submodule_target":"example.com/selftest/blesh-contrib@d2109203480a","#,
            r#""submodule_commit":"d2109203480a7dfe1dead5f5f8f9f15a9146c90d"}]}"#,
            "\n"
        )
    );
}

#[test]
fn a_file_path() {
    for (path, message) in [
        ("mux.go", "(606 lines)"),
        ("one.txt", "(1 line)"),
        ("empty.txt", "(0 lines)"),
        ("logo.png", "(binary, 4096 bytes)"),
    ] {
        let (ran, _) = ls(&[&format!("{MUX}@v1.8.0:{path}")]);
        failed(&ran, &format!("{path} is a file at {MUX_AT} {message}"));
    }
}

#[test]
fn a_missing_path() {
    let (ran, _) = ls(&["example.com/selftest/mux@v1.8.0:nope"]);
    failed(&ran, &format!("nope not found in {MUX_AT}"));
}

#[test]
fn a_path_under_a_file() {
    let (ran, _) = ls(&["example.com/selftest/mux@v1.8.0:mux.go/x"]);
    failed(&ran, &format!("mux.go/x not found in {MUX_AT}"));
}

#[test]
fn listing_errors() {
    for path in ["broken", "ghost", "ghost/x"] {
        let (ran, _) = ls(&[&format!("{MUX}@v1.8.0:{path}")]);
        failed(&ran, "Sourcegraph failed to answer the directory listing");
        let text = err(&ran);
        assert!(!text.contains("gitserver"), "{path}: {text}");
        assert!(!text.contains("git command"), "{path}: {text}");
    }
}

#[test]
fn the_rev_field() {
    let (ran, _) = ls(&["example.com/selftest/mux@v1.8.0"]);
    assert_eq!(
        out(&ran).lines().next(),
        Some(
            "example.com/selftest/mux@98cb6bf42e08  3 entries  1 dir  2 files  \
             rev v1.8.0"
        )
    );
    let (ran, _) = ls(&["--json", "example.com/selftest/mux@v1.8.0"]);
    assert_eq!(json_out(&ran)["rev"], "v1.8.0");

    let (ran, _) = ls(&[&format!("{MUX}@{MUX_OID}")]);
    assert_eq!(
        out(&ran).lines().next(),
        Some(
            "example.com/selftest/mux@98cb6bf42e08  3 entries  1 dir  2 files"
        )
    );
    let (ran, _) = ls(&["--json", &format!("{MUX}@{MUX_OID}")]);
    assert_eq!(json_out(&ran)["rev"], MUX_OID);

    let (ran, _) = ls(&["example.com/selftest/mux"]);
    assert_eq!(
        out(&ran).lines().next(),
        Some(
            "example.com/selftest/mux@98cb6bf42e08  3 entries  1 dir  2 files"
        )
    );
    let (ran, _) = ls(&["--json", "example.com/selftest/mux"]);
    assert_eq!(json_out(&ran)["rev"], Value::Null);
}

#[test]
fn server_paths_are_escaped() {
    let entries = [
        file("a\u{1b}[31mb"),
        file("line\nbreak"),
        module("m\u{202e}x", "https://example.com/a\nb\u{1b}[0m"),
    ];
    let fake = scripted(&root_of(&entries));
    let ran = run_on(&fake, &[&format!("{MUX}@{MUX_OID}")]);
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", err(&ran));
    let text = out(&ran);
    assert!(!text.contains('\u{1b}'), "{text:?}");
    assert!(!text.contains('\u{202e}'), "{text:?}");
    assert_eq!(text.lines().count(), 4, "{text:?}");
    assert!(text.contains(r"$'a\033[31mb'"), "{text}");
    assert!(text.contains(r"$'line\nbreak'"), "{text}");
    assert!(text.contains(r"https://example.com/a b\u{1b}[0m"), "{text}");
}

#[test]
fn one_request() {
    for token in [
        "example.com/selftest/ble@d81fd54feb0d",
        "example.com/selftest/ble@d81fd54feb0d:contrib",
        "example.com/selftest/mux@v1.8.0:mux.go",
        "example.com/selftest/mux@v1.8.0:nope",
        "example.com/selftest/mux@v1.8.0:mux.go/x",
        "example.com/selftest/mux@v1.8.0:broken",
    ] {
        let (_, fake) = ls(&[token]);
        let calls = fake.calls();
        assert_eq!(calls.len(), 1, "{token}");
        assert_eq!(calls[0].method, "POST");
        assert_eq!(calls[0].path, GRAPHQL_PATH);
    }
}
