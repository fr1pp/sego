use std::cell::Cell;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use serde_json::json;

use sego::internal::upstream::DEADLINE;
use sego::{Endpoint, Reply, Transport, TransportError};
use sego::{Env, run};

use crate::support::exec::{Ran, args, exec, exec_os};
use crate::support::fake::Fake;
use crate::support::fixture::load_all;
use tempfile::TempDir;

fn env(endpoint: Option<&str>) -> Env {
    Env {
        endpoint: endpoint.map(OsString::from),
        cache_dir: Some(OsString::from("off")),
    }
}

fn never(_: Endpoint) -> Box<dyn Transport> {
    panic!("connect was called");
}

#[test]
fn usage_errors_never_connect() {
    let cases: [(&[&str], Option<&str>); 3] = [
        (&["shwo"], None),
        (&["show", "github.com/x/y:a"], Some("ftp://x")),
        (&["show", "github.com/x/y"], None),
    ];
    for (words, endpoint) in cases {
        let ran = exec(words, endpoint, &never);
        assert_eq!(ran.code, ExitCode::from(2), "{words:?}");
        assert!(ran.out.is_empty(), "{words:?}");
        let err = String::from_utf8_lossy(&ran.err);
        assert!(err.starts_with("sego: error: "), "{err}");
        assert!(err.contains("usage: sego "), "{err}");
    }
}

#[test]
fn the_endpoint_error_names_the_variable() {
    let ran = exec(&["show", "github.com/x/y:a"], Some("ftp://x"), &never);
    assert_eq!(
        String::from_utf8_lossy(&ran.err),
        "sego: error: SEGO_ENDPOINT must be an http:// or https:// URL \
         with a host and no query or fragment: ftp://x\n\
         usage: sego COMMAND [--json] ...\n"
    );
}

#[test]
fn help_and_version_never_connect() {
    let ran = exec(&["--help"], Some("ftp://x"), &never);
    assert_eq!(ran.code, ExitCode::SUCCESS);
    assert!(
        ran.out.starts_with(b"sego searches and reads public source code")
    );
    assert_eq!(ran.err, b"");
    let ran = exec(&["--version"], None, &never);
    assert_eq!(ran.code, ExitCode::SUCCESS);
    assert_eq!(ran.out, b"sego 0.2.0\n");
}

#[test]
fn a_valid_command_connects_once_to_the_environment_endpoint() {
    let fake = Fake::scripted(Vec::new());
    let connects = Cell::new(0);
    let urls = Cell::new(String::new());
    let handle = fake.clone();
    let connect = |endpoint: Endpoint| -> Box<dyn Transport> {
        connects.set(connects.get() + 1);
        urls.set(endpoint.url("/x"));
        Box::new(handle.clone())
    };
    let ran = exec(
        &["show", "github.com/x/y:a"],
        Some("http://127.0.0.1:9/"),
        &connect,
    );
    assert_eq!(ran.code, ExitCode::from(1));
    assert_eq!(connects.get(), 1);
    assert_eq!(urls.take(), "http://127.0.0.1:9/x");
}

struct Broken;

impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::ErrorKind::BrokenPipe.into())
    }
}

#[test]
fn broken_pipe_is_quiet() {
    let mut err = Vec::new();
    let code =
        run(args(&["--help"]), &env(None), &never, &mut Broken, &mut err);
    assert_eq!(code, ExitCode::SUCCESS);
    assert_eq!(err, b"");

    let mut out = Vec::new();
    let code = run(args(&["shwo"]), &env(None), &never, &mut out, &mut Broken);
    assert_eq!(code, ExitCode::SUCCESS);
    assert_eq!(out, b"");
}

#[test]
fn the_fake_records_every_call_and_replays_fixtures() {
    let fixtures = load_all(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/selftest"),
    );
    let fake = Fake::fixtures(fixtures);
    let deadline = Instant::now() + Duration::from_secs(5);
    let reply = fake.get("/r/x", deadline).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(reply.body, b"raw body\n");
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!((calls[0].method, calls[0].path.as_str()), ("GET", "/r/x"));
    assert_eq!(calls[0].deadline, deadline);
}

#[test]
fn the_fake_replays_a_script_in_order() {
    let ok = |status| {
        Ok(Reply {
            status,
            content_type: None,
            location: None,
            retry_after: None,
            cf_mitigated: None,
            body: Vec::new(),
        })
    };
    let fake = Fake::scripted(vec![ok(200), Err(TransportError::Deadline)]);
    let deadline = Instant::now();
    assert_eq!(fake.post("/a", b"{}", deadline).map(|r| r.status), Ok(200));
    assert_eq!(
        fake.get("/b", deadline).map(|r| r.status),
        Err(TransportError::Deadline)
    );
    assert_eq!(
        fake.post("/c", b"", deadline).map(|r| r.status),
        Err(TransportError::Network("script exhausted"))
    );
    let paths: Vec<_> = fake.calls().into_iter().map(|c| c.path).collect();
    assert_eq!(paths, ["/a", "/b", "/c"]);
}

fn json_reply(value: &serde_json::Value) -> Reply {
    Reply {
        status: 200,
        content_type: Some("application/json".to_owned()),
        location: None,
        retry_after: None,
        cf_mitigated: None,
        body: value.to_string().into_bytes(),
    }
}

fn raw_reply(status: u16, content_type: &str, body: &[u8]) -> Reply {
    Reply {
        status,
        content_type: Some(content_type.to_owned()),
        location: None,
        retry_after: None,
        cf_mitigated: None,
        body: body.to_vec(),
    }
}

const SHA: &str = "d81fd54f1a2b3c4d5e6f708192a3b4c5d6e7f809";

fn file_envelope() -> serde_json::Value {
    json!({"data": {"r0": {
        "name": "github.com/x/y",
        "defaultBranch": {"displayName": "main"},
        "c0": {
            "oid": SHA,
            "meta": {
                "byteSize": 6, "totalLines": 3, "binary": false,
                "rawURL": "/github.com/x/y@d81fd54f1a2b/-/raw/a.go",
                "lfs": null
            },
            "body": {"content": "a\nb\nc\n"},
            "dir": null,
            "parent": {"entries": [{"path": "a.go", "submodule": null}]}
        }
    }}, "errors": [{"message": "not a directory", "path": ["r0", "c0", "dir"]}]})
}

fn over_cap_envelope() -> serde_json::Value {
    json!({"data": {"r0": {
        "name": "github.com/x/y",
        "defaultBranch": {"displayName": "main"},
        "c0": {
            "oid": SHA,
            "meta": {
                "byteSize": 2_000_000, "totalLines": 5, "binary": false,
                "rawURL": "/github.com/x/y@d81fd54f1a2b/-/raw/big.c",
                "lfs": null
            },
            "body": null,
            "dir": null,
            "parent": {"entries": [{"path": "big.c", "submodule": null}]}
        }
    }}, "errors": [
        {"message": "file exceeded size limit", "path": ["r0", "c0", "body", "content"]},
        {"message": "file exceeded size limit", "path": ["r0", "c0", "meta", "lfs"]},
        {"message": "not a directory", "path": ["r0", "c0", "dir"]}
    ]})
}

fn submodule_envelope(url: &str) -> serde_json::Value {
    json!({"data": {"r0": {
        "name": "github.com/akinomyoga/ble.sh",
        "defaultBranch": {"displayName": "master"},
        "c0": {
            "oid": SHA,
            "meta": null, "body": null, "dir": null,
            "parent": {"entries": [
                {"path": "contrib", "submodule": {
                    "url": url,
                    "commit": "d2109203480a1b2c3d4e5f60718293a4b5c6d7e8"
                }},
                {"path": "make", "submodule": null}
            ]}
        }
    }}, "errors": [
        {"message": "x", "path": ["r0", "c0", "meta"]},
        {"message": "x", "path": ["r0", "c0", "body"]},
        {"message": "x", "path": ["r0", "c0", "dir"]}
    ]})
}

fn show_via(
    words: &[&str],
    script: Vec<Result<Reply, TransportError>>,
) -> (Ran, Vec<crate::support::fake::Call>) {
    show_cached(words, OsStr::new("off"), script)
}

fn show_cached(
    words: &[&str],
    cache_dir: &OsStr,
    script: Vec<Result<Reply, TransportError>>,
) -> (Ran, Vec<crate::support::fake::Call>) {
    let fake = Fake::scripted(script);
    let handle = fake.clone();
    let connect =
        move |_: Endpoint| -> Box<dyn Transport> { Box::new(handle.clone()) };
    let ran = exec_os(args(words), None, cache_dir, &connect);
    (ran, fake.calls())
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn deadline_starts_once() {
    let before = Instant::now();
    let (ran, calls) = show_via(
        &["show", "github.com/x/y@main:a.go"],
        vec![Ok(json_reply(&file_envelope()))],
    );
    assert_eq!(text(&ran.err), "", "{}", text(&ran.err));
    assert_eq!(ran.code, ExitCode::SUCCESS);
    assert_eq!(calls.len(), 1, "a show without the raw route is one call");
    assert_eq!(calls[0].method, "POST");

    let (ran, calls) = show_via(
        &["show", "github.com/x/y@main:big.c", "1-5"],
        vec![
            Ok(json_reply(&over_cap_envelope())),
            Ok(raw_reply(200, "text/plain", b"1\n2\n3\n4\n5\n")),
        ],
    );
    assert_eq!(text(&ran.err), "", "{}", text(&ran.err));
    assert_eq!(calls.len(), 2);
    for call in &calls {
        assert!(call.deadline >= before + DEADLINE);
        assert!(call.deadline <= Instant::now() + DEADLINE);
    }
}

#[test]
fn deadline_is_shared_by_the_raw_read() {
    let (ran, calls) = show_via(
        &["show", "github.com/x/y@main:big.c", "1-5"],
        vec![
            Ok(json_reply(&over_cap_envelope())),
            Ok(raw_reply(200, "text/plain", b"1\n2\n3\n4\n5\n")),
        ],
    );
    assert_eq!(ran.code, ExitCode::SUCCESS);
    let methods: Vec<_> = calls.iter().map(|call| call.method).collect();
    assert_eq!(methods, ["POST", "GET"]);
    assert_eq!(calls[1].deadline, calls[0].deadline);

    let (ran, calls) = show_via(
        &["show", "github.com/x/y@main:big.c", "1-5"],
        vec![
            Ok(json_reply(&over_cap_envelope())),
            Err(TransportError::Deadline),
        ],
    );
    assert_eq!(ran.code, ExitCode::from(1));
    assert_eq!(ran.out, b"");
    assert!(
        text(&ran.err).ends_with(
            "sego: error: no answer from sourcegraph.com within 90 s\n"
        ),
        "{}",
        text(&ran.err)
    );
    assert_eq!(calls[1].deadline, calls[0].deadline);
}

#[test]
fn a_recorded_binary_file_over_the_limit_takes_one_request() {
    let fake = Fake::fixtures(load_all(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures"),
    ));
    let handle = fake.clone();
    let connect =
        move |_: Endpoint| -> Box<dyn Transport> { Box::new(handle.clone()) };
    let ran = exec(
        &[
            "show",
            "github.com/golang/go@6885bad7dd86:src/crypto/internal/boring/syso/goboringcrypto_linux_amd64.syso",
        ],
        None,
        &connect,
    );
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", text(&ran.err));
    assert_eq!(text(&ran.err), "");
    assert!(text(&ran.out).contains("binary  2555664 bytes"));
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "POST");
}

#[test]
fn a_file_prints_its_header_and_lines() {
    let mut envelope = file_envelope();
    envelope["data"]["r0"]["c0"]["body"]["content"] = json!("b\nc\n");
    let (ran, _) = show_via(
        &["show", "github.com/x/y@main:a.go", "2-9"],
        vec![Ok(json_reply(&envelope))],
    );
    assert_eq!(ran.code, ExitCode::SUCCESS);
    assert_eq!(
        text(&ran.err),
        "sego: note: lines 2-9 clamped to 2-3; the file has 3 lines\n"
    );
    assert_eq!(
        text(&ran.out),
        "github.com/x/y@d81fd54f1a2b:a.go  lines 2-3 of 3  rev main\n\
2:b\n3:c\n"
    );
}

#[test]
fn the_default_window_says_how_to_read_a_longer_file_whole() {
    let header = |total: u32| {
        let mut envelope = file_envelope();
        let content = "x\n".repeat(total.min(500) as usize);
        envelope["data"]["r0"]["c0"]["meta"]["totalLines"] = json!(total);
        envelope["data"]["r0"]["c0"]["body"]["content"] = json!(content);
        let (ran, _) = show_via(
            &["show", "github.com/x/y@main:a.go"],
            vec![Ok(json_reply(&envelope))],
        );
        assert_eq!(ran.code, ExitCode::SUCCESS, "{}", text(&ran.err));
        text(&ran.out).lines().next().unwrap_or_default().to_owned()
    };
    assert_eq!(
        header(500),
        "github.com/x/y@d81fd54f1a2b:a.go  lines 1-500 of 500  rev main"
    );
    assert_eq!(
        header(501),
        "github.com/x/y@d81fd54f1a2b:a.go  lines 1-500 of 501 (1- for all)  \
         rev main"
    );
}

#[test]
fn a_submodule_prints_its_target() {
    let cases = [
        (
            "../blesh-contrib",
            "github.com/akinomyoga/blesh-contrib@d2109203480a",
        ),
        ("file:///srv/y", "file:///srv/y"),
        ("", "?"),
    ];
    for (url, shown) in cases {
        let (ran, calls) = show_via(
            &["show", "github.com/akinomyoga/ble.sh@d81fd54f1a2b:contrib"],
            vec![Ok(json_reply(&submodule_envelope(url)))],
        );
        assert_eq!(text(&ran.err), "", "{url}");
        assert_eq!(ran.code, ExitCode::SUCCESS, "{url}");
        assert_eq!(calls.len(), 1);
        let out = text(&ran.out);
        assert_eq!(
            out,
            format!(
                "github.com/akinomyoga/ble.sh@d81fd54f1a2b:contrib  \
                 submodule  {shown}\n"
            ),
            "{url}"
        );
    }
}

#[test]
fn a_submodule_in_json() {
    let (ran, _) = show_via(
        &[
            "show",
            "--json",
            "github.com/akinomyoga/ble.sh@d81fd54f1a2b:contrib",
        ],
        vec![Ok(json_reply(&submodule_envelope("../blesh-contrib")))],
    );
    let value: serde_json::Value =
        serde_json::from_slice(&ran.out).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        value["submodule"],
        json!({
            "url": "../blesh-contrib",
            "target": "github.com/akinomyoga/blesh-contrib@d2109203480a",
            "commit": "d2109203480a1b2c3d4e5f60718293a4b5c6d7e8"
        })
    );
    assert_eq!(value["lines"], json!([]));
    assert_eq!(value["start"], json!(null));
    assert_eq!(value["via_raw"], json!(false));
}

#[test]
fn a_raw_read_says_how_to_read_a_longer_file_whole() {
    let mut envelope = over_cap_envelope();
    envelope["data"]["r0"]["c0"]["meta"]["totalLines"] = json!(501);
    let (ran, calls) = show_via(
        &["show", "github.com/x/y@main:big.c"],
        vec![
            Ok(json_reply(&envelope)),
            Ok(raw_reply(200, "text/plain", "x\n".repeat(501).as_bytes())),
        ],
    );
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", text(&ran.err));
    assert_eq!(calls.len(), 2);
    assert_eq!(
        text(&ran.out).lines().next(),
        Some(
            "github.com/x/y@d81fd54f1a2b:big.c  lines 1-500 of 501 \
             (1- for all)  rev main"
        )
    );
}

#[test]
fn over_the_limit_reads_the_raw_route() {
    let (ran, calls) = show_via(
        &["show", "github.com/x/y@main:big.c", "2-3"],
        vec![
            Ok(json_reply(&over_cap_envelope())),
            Ok(raw_reply(200, "text/plain", b"1\r\n2\r\n3\r\n4\r\n5\r\n")),
        ],
    );
    assert_eq!(ran.code, ExitCode::SUCCESS);
    assert_eq!(text(&ran.err), "");
    assert_eq!(
        text(&ran.out),
        "github.com/x/y@d81fd54f1a2b:big.c  lines 2-3 of 5  CRLF  rev main\n\
2:2\n3:3\n"
    );
    assert_eq!(calls.len(), 2);
    assert_eq!(
        (calls[1].method, calls[1].path.as_str()),
        ("GET", "/github.com/x/y@d81fd54f1a2b/-/raw/big.c")
    );
}

#[test]
fn the_raw_read_in_json() {
    let (ran, _) = show_via(
        &["show", "--json", "github.com/x/y@main:big.c", "5-"],
        vec![
            Ok(json_reply(&over_cap_envelope())),
            Ok(raw_reply(200, "text/plain", b"1\n2\n3\n4\n5")),
        ],
    );
    let value: serde_json::Value =
        serde_json::from_slice(&ran.out).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(value["via_raw"], json!(true));
    assert_eq!(value["lines"], json!([{"line": 5, "text": "5"}]));
    assert_eq!(
        (value["start"].as_u64(), value["end"].as_u64()),
        (Some(5), Some(5))
    );
}

#[test]
fn a_raw_read_that_fails_is_an_error() {
    let challenge = Ok(Reply {
        status: 429,
        content_type: Some("text/html".to_owned()),
        location: None,
        retry_after: None,
        cf_mitigated: Some("challenge".to_owned()),
        body: b"<html></html>".to_vec(),
    });
    let cases: Vec<(Result<Reply, TransportError>, &str)> = vec![
        (
            challenge,
            "sego: error: sourcegraph.com's edge refused the request (HTTP \
             429, Cloudflare challenge)\n",
        ),
        (
            Ok(raw_reply(404, "text/html", b"<html>nope</html>")),
            "sego: error: big.c not found in github.com/x/y@d81fd54f1a2b\n",
        ),
        (
            Ok(raw_reply(502, "text/plain", b"bad")),
            "sego: error: sourcegraph.com answered HTTP 502\n",
        ),
        (
            Err(TransportError::Deadline),
            "sego: error: no answer from sourcegraph.com within 90 s\n",
        ),
    ];
    for (raw, expected) in cases {
        let (ran, calls) = show_via(
            &["show", "github.com/x/y@main:big.c", "1-2"],
            vec![Ok(json_reply(&over_cap_envelope())), raw],
        );
        assert_eq!(ran.code, ExitCode::from(1), "{expected}");
        assert_eq!(ran.out, b"");
        assert!(
            text(&ran.err).ends_with(expected),
            "{}\n{expected}",
            text(&ran.err)
        );
        assert_eq!(calls.len(), 2);
    }
}

#[test]
fn a_binary_file_over_the_limit_is_not_read_raw() {
    let mut envelope = over_cap_envelope();
    envelope["data"]["r0"]["c0"]["meta"]["binary"] = json!(true);
    let (ran, calls) = show_via(
        &["show", "github.com/x/y@main:big.c"],
        vec![Ok(json_reply(&envelope))],
    );
    assert_eq!(ran.code, ExitCode::SUCCESS);
    assert_eq!(calls.len(), 1);
    assert_eq!(
        text(&ran.out),
        "github.com/x/y@d81fd54f1a2b:big.c  binary  2000000 bytes  rev main\n"
    );
}

fn with_errors(
    mut envelope: serde_json::Value,
    errors: &[(&[&str], &str)],
) -> serde_json::Value {
    let list: Vec<_> = errors
        .iter()
        .map(|(path, message)| json!({"message": message, "path": path}))
        .collect();
    envelope["errors"] = json!(list);
    envelope
}

fn fails_with(envelope: &serde_json::Value, expected: &str, secret: &str) {
    let (ran, calls) = show_via(
        &["show", "github.com/x/y@main:a.go"],
        vec![Ok(json_reply(envelope))],
    );
    assert_eq!(ran.code, ExitCode::from(1), "{expected}");
    assert_eq!(ran.out, b"");
    assert_eq!(text(&ran.err), format!("sego: error: {expected}\n"));
    assert!(!text(&ran.err).contains(secret));
    assert_eq!(calls.len(), 1);
}

#[test]
fn upstream_errors_name_the_part_and_hide_the_server_text() {
    let secret = "git exec: /srv/secret-host failed";
    let mut body = file_envelope();
    body["data"]["r0"]["c0"]["body"] = json!(null);
    fails_with(
        &with_errors(body, &[(&["r0", "c0", "body"], secret)]),
        "Sourcegraph failed to answer the file content",
        secret,
    );

    let mut meta = file_envelope();
    meta["data"]["r0"]["c0"]["meta"] = json!(null);
    fails_with(
        &with_errors(meta, &[(&["r0", "c0", "meta"], secret)]),
        "Sourcegraph failed to answer the file's details",
        secret,
    );

    let mut parent = file_envelope();
    let c0 = &mut parent["data"]["r0"]["c0"];
    c0["meta"] = json!(null);
    c0["parent"] = json!(null);
    fails_with(
        &with_errors(
            parent,
            &[
                (&["r0", "c0", "meta"], secret),
                (&["r0", "c0", "parent"], secret),
            ],
        ),
        "Sourcegraph failed to answer the directory listing",
        secret,
    );
}

#[test]
fn a_parent_listing_error_does_not_fail_a_file_that_read() {
    let mut envelope = file_envelope();
    envelope["data"]["r0"]["c0"]["parent"] = json!(null);
    let envelope = with_errors(
        envelope,
        &[
            (&["r0", "c0", "dir"], "not a directory"),
            (&["r0", "c0", "parent"], "listing failed"),
        ],
    );
    let (ran, _) = show_via(
        &["show", "github.com/x/y@main:a.go"],
        vec![Ok(json_reply(&envelope))],
    );
    assert_eq!(ran.code, ExitCode::SUCCESS, "{}", text(&ran.err));
    assert_eq!(
        text(&ran.out),
        "github.com/x/y@d81fd54f1a2b:a.go  lines 1-3 of 3  rev main\n\
1:a\n2:b\n3:c\n"
    );
}

#[test]
fn a_rawurl_that_is_not_a_site_path_is_refused() {
    for raw in
        ["@evil.example/x", "//evil.example/x", "https://evil.example/x", ""]
    {
        let mut envelope = over_cap_envelope();
        envelope["data"]["r0"]["c0"]["meta"]["rawURL"] = json!(raw);
        let (ran, calls) = show_via(
            &["show", "github.com/x/y@main:big.c", "1-2"],
            vec![Ok(json_reply(&envelope))],
        );
        assert_eq!(ran.code, ExitCode::from(1), "{raw}");
        assert_eq!(
            text(&ran.err),
            "sego: error: Sourcegraph failed to answer the file's details\n",
            "{raw}"
        );
        assert_eq!(calls.len(), 1, "{raw}");
    }
}

#[test]
fn a_tree_url_of_the_root_names_it() {
    let mut envelope = file_envelope();
    let c0 = &mut envelope["data"]["r0"]["c0"];
    c0["meta"] = json!(null);
    c0["body"] = json!(null);
    c0["dir"] = json!({"entries": [{"path": "a"}, {"path": "b"}]});
    c0["x0"] = json!({"__typename": "GitTree"});
    let envelope = with_errors(
        envelope,
        &[(&["r0", "c0", "meta"], "x"), (&["r0", "c0", "body"], "x")],
    );
    let (ran, _) = show_via(
        &["show", "https://github.com/x/y/tree/main"],
        vec![Ok(json_reply(&envelope))],
    );
    assert_eq!(ran.code, ExitCode::from(1));
    assert_eq!(
        text(&ran.err),
        "sego: error: ./ is a directory at github.com/x/y@d81fd54f1a2b \
         (2 entries)\n"
    );
}

#[test]
fn only_the_limit_error_at_meta_lfs_means_not_lfs() {
    let secret = "lfs backend exploded";
    let mut envelope = file_envelope();
    envelope["data"]["r0"]["c0"]["meta"]["lfs"] = json!(null);
    fails_with(
        &with_errors(envelope, &[(&["r0", "c0", "meta", "lfs"], secret)]),
        "Sourcegraph failed to answer the file's details",
        secret,
    );
}

#[test]
fn short_content_is_not_printed_under_a_longer_header() {
    let mut envelope = file_envelope();
    envelope["data"]["r0"]["c0"]["body"]["content"] = json!("a\n");
    fails_with(
        &envelope,
        "Sourcegraph failed to answer the file content",
        "zzz",
    );
}

#[test]
fn an_empty_file_with_lines_has_a_past_end_note() {
    let mut envelope = file_envelope();
    let meta = &mut envelope["data"]["r0"]["c0"]["meta"];
    meta["byteSize"] = json!(0);
    meta["totalLines"] = json!(1);
    envelope["data"]["r0"]["c0"]["body"]["content"] = json!("");
    let (ran, _) = show_via(
        &["show", "github.com/x/y@main:a.go", "1-5"],
        vec![Ok(json_reply(&envelope))],
    );
    assert_eq!(ran.code, ExitCode::SUCCESS);
    assert_eq!(
        text(&ran.out),
        "github.com/x/y@d81fd54f1a2b:a.go  lines 0 of 0  rev main\n"
    );
    assert_eq!(text(&ran.err), "sego: note: no lines 1-5; the file has 0\n");
}

#[test]
fn a_directory_counts_its_entries() {
    for (entries, unit) in [(1, "1 entry"), (2, "2 entries")] {
        let listing: Vec<_> = (0..entries)
            .map(|i| json!({"path": format!("a.go/{i}")}))
            .collect();
        let mut envelope = file_envelope();
        let c0 = &mut envelope["data"]["r0"]["c0"];
        c0["meta"] = json!(null);
        c0["body"] = json!(null);
        c0["dir"] = json!({"entries": listing});
        let envelope = with_errors(
            envelope,
            &[(&["r0", "c0", "meta"], "x"), (&["r0", "c0", "body"], "x")],
        );
        fails_with(
            &envelope,
            &format!(
                "a.go is a directory at github.com/x/y@d81fd54f1a2b ({unit})"
            ),
            "zzz",
        );
    }
}

#[test]
fn the_raw_route_shares_the_refusal_rules() {
    let reply =
        |status, content_type: Option<&str>, f: &dyn Fn(&mut Reply)| {
            let mut reply = Reply {
                status,
                content_type: content_type.map(str::to_owned),
                location: None,
                retry_after: None,
                cf_mitigated: None,
                body: b"x".to_vec(),
            };
            f(&mut reply);
            Ok(reply)
        };
    let cases = vec![
        (
            reply(302, None, &|r| r.location = Some("/login".to_owned())),
            "sourcegraph.com answered HTTP 302 with Location /login",
        ),
        (
            reply(302, None, &|_| {}),
            "sourcegraph.com answered HTTP 302 with no Location",
        ),
        (
            reply(503, Some("text/plain"), &|r| {
                r.retry_after = Some("30".to_owned());
            }),
            "sourcegraph.com answered HTTP 503\n  Retry-After: 30",
        ),
        (
            reply(200, Some("text/html; charset=utf-8"), &|_| {}),
            "sourcegraph.com answered HTTP 200 text/html",
        ),
        (
            reply(403, Some("text/html"), &|_| {}),
            "sourcegraph.com's edge refused the request (HTTP 403, \
             Cloudflare challenge)",
        ),
    ];
    for (raw, expected) in cases {
        let (ran, _) = show_via(
            &["show", "github.com/x/y@main:big.c", "1-2"],
            vec![Ok(json_reply(&over_cap_envelope())), raw],
        );
        assert_eq!(ran.code, ExitCode::from(1), "{expected}");
        assert!(
            text(&ran.err).ends_with(&format!("sego: error: {expected}\n")),
            "{}\n{expected}",
            text(&ran.err)
        );
    }
}

#[test]
fn a_failed_raw_read_writes_only_its_error() {
    let cases: Vec<(Result<Reply, TransportError>, &str)> = vec![
        (
            Ok(raw_reply(404, "text/html", b"<html>nope</html>")),
            "sego: error: big.c not found in github.com/x/y@d81fd54f1a2b\n",
        ),
        (
            Err(TransportError::Deadline),
            "sego: error: no answer from sourcegraph.com within 90 s\n",
        ),
    ];
    for (raw, error) in cases {
        let (ran, _) = show_via(
            &["show", "github.com/x/y@main:big.c", "1-2"],
            vec![Ok(json_reply(&over_cap_envelope())), raw],
        );
        assert_eq!(ran.code, ExitCode::from(1), "{error}");
        assert_eq!(text(&ran.err), error);
    }
}

const BODY: &[u8] = b"1\n2\n3\n4\n5\n";

/// An over-cap envelope whose `byteSize` is `BODY`'s length, so the raw
/// read is cacheable.
fn fitting_envelope() -> serde_json::Value {
    let mut envelope = over_cap_envelope();
    envelope["data"]["r0"]["c0"]["meta"]["byteSize"] = json!(BODY.len());
    envelope
}

fn fitting_script() -> Vec<Result<Reply, TransportError>> {
    vec![
        Ok(json_reply(&fitting_envelope())),
        Ok(raw_reply(200, "text/plain", BODY)),
    ]
}

const BIG: [&str; 3] = ["show", "github.com/x/y@main:big.c", "1-5"];

fn methods(calls: &[crate::support::fake::Call]) -> Vec<&'static str> {
    calls.iter().map(|call| call.method).collect()
}

fn files_under(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(files_under(&path));
        } else {
            files.push(path);
        }
    }
    files
}

#[test]
fn a_second_read_uses_the_cache() {
    let dir = TempDir::new().unwrap();
    let (first, calls) =
        show_cached(&BIG, dir.path().as_os_str(), fitting_script());
    assert_eq!(text(&first.err), "");
    assert_eq!(first.code, ExitCode::SUCCESS);
    assert_eq!(methods(&calls), ["POST", "GET"]);
    assert!(dir.path().join("trim.txt").exists());

    let (second, calls) = show_cached(
        &BIG,
        dir.path().as_os_str(),
        vec![Ok(json_reply(&fitting_envelope()))],
    );
    assert_eq!(text(&second.err), "");
    assert_eq!(second.code, ExitCode::SUCCESS);
    assert_eq!(methods(&calls), ["POST"]);
    assert_eq!(text(&second.out), text(&first.out));
    assert!(text(&second.out).ends_with("5:5\n"), "{}", text(&second.out));
}

#[test]
fn a_body_of_the_wrong_length_answers_and_is_not_cached() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("c");
    let (ran, calls) = show_cached(
        &BIG,
        root.as_os_str(),
        vec![
            Ok(json_reply(&over_cap_envelope())),
            Ok(raw_reply(200, "text/plain", BODY)),
        ],
    );
    assert_eq!(ran.code, ExitCode::SUCCESS);
    assert_eq!(text(&ran.err), "");
    assert_eq!(methods(&calls), ["POST", "GET"]);
    assert!(root.join("CACHEDIR.TAG").exists());
    assert_eq!(
        files_under(&root.join("raw")),
        Vec::<std::path::PathBuf>::new()
    );
}

#[test]
fn an_existing_cache_dir_is_used_untagged() {
    let dir = TempDir::new().unwrap();
    let (first, calls) =
        show_cached(&BIG, dir.path().as_os_str(), fitting_script());
    assert_eq!(text(&first.err), "");
    assert_eq!(methods(&calls), ["POST", "GET"]);

    let (second, calls) = show_cached(
        &BIG,
        dir.path().as_os_str(),
        vec![Ok(json_reply(&fitting_envelope()))],
    );
    assert_eq!(text(&second.err), "");
    assert_eq!(methods(&calls), ["POST"]);
    assert!(!dir.path().join("CACHEDIR.TAG").exists());
}

#[test]
fn an_entry_of_the_wrong_length_is_replaced() {
    let dir = TempDir::new().unwrap();
    let (_, calls) =
        show_cached(&BIG, dir.path().as_os_str(), fitting_script());
    assert_eq!(methods(&calls), ["POST", "GET"]);
    let entries = files_under(&dir.path().join("raw"));
    assert_eq!(entries.len(), 1);
    fs::write(&entries[0], b"short").unwrap();

    let (ran, calls) =
        show_cached(&BIG, dir.path().as_os_str(), fitting_script());
    assert_eq!(ran.code, ExitCode::SUCCESS);
    assert_eq!(text(&ran.err), "");
    assert_eq!(methods(&calls), ["POST", "GET"]);
    assert_eq!(files_under(&dir.path().join("raw")), entries);
    assert_eq!(fs::read(&entries[0]).unwrap(), BODY);
}

#[test]
fn a_cache_dir_that_is_a_file_is_noted() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("file");
    fs::write(&file, b"").unwrap();
    let (ran, calls) = show_cached(&BIG, file.as_os_str(), fitting_script());
    assert_eq!(ran.code, ExitCode::SUCCESS);
    assert_eq!(
        text(&ran.err),
        "sego: note: cache unavailable: not a directory\n"
    );
    assert_eq!(methods(&calls), ["POST", "GET"]);
}

#[test]
fn a_show_under_the_limit_leaves_the_cache_untouched() {
    let dir = TempDir::new().unwrap();
    let (ran, calls) = show_cached(
        &["show", "github.com/x/y@main:a.go"],
        dir.path().as_os_str(),
        vec![Ok(json_reply(&file_envelope()))],
    );
    assert_eq!(ran.code, ExitCode::SUCCESS);
    assert_eq!(text(&ran.err), "");
    assert_eq!(methods(&calls), ["POST"]);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn an_unwritable_root_notes_once_and_answers() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let dir = TempDir::new().unwrap();
    if fs::metadata(dir.path()).unwrap().uid() == 0 {
        return;
    }
    let root = dir.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o500)).unwrap();
    let (ran, calls) = show_cached(&BIG, root.as_os_str(), fitting_script());
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(ran.code, ExitCode::SUCCESS);
    assert_eq!(
        text(&ran.err),
        "sego: note: cache unavailable: permission denied\n"
    );
    assert!(text(&ran.out).ends_with("5:5\n"), "{}", text(&ran.out));
    assert_eq!(methods(&calls), ["POST", "GET"]);
}

#[test]
fn off_reads_through_every_time() {
    for _ in 0..2 {
        let (ran, calls) =
            show_cached(&BIG, OsStr::new("off"), fitting_script());
        assert_eq!(ran.code, ExitCode::SUCCESS);
        assert_eq!(text(&ran.err), "");
        assert_eq!(methods(&calls), ["POST", "GET"]);
    }
}

#[test]
fn a_relative_cache_dir_is_noted() {
    let (ran, calls) = show_cached(&BIG, OsStr::new("rel"), fitting_script());
    assert_eq!(ran.code, ExitCode::SUCCESS);
    assert_eq!(
        text(&ran.err),
        "sego: note: cache unavailable: SEGO_CACHE_DIR is not an absolute \
         path\n"
    );
    assert_eq!(methods(&calls), ["POST", "GET"]);
}
