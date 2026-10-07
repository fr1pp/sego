use std::cell::Cell;
use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use sego::internal::upstream::{Endpoint, Reply, Transport, TransportError};
use sego::{Env, run};

use crate::support::fake::Fake;
use crate::support::fixture::load_all;

struct Ran {
    code: ExitCode,
    out: Vec<u8>,
    err: Vec<u8>,
}

fn args(words: &[&str]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

fn env(endpoint: Option<&str>) -> Env {
    Env { endpoint: endpoint.map(OsString::from) }
}

fn exec(
    words: &[&str],
    endpoint: Option<&str>,
    connect: &dyn Fn(Endpoint) -> Box<dyn Transport>,
) -> Ran {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run(args(words), &env(endpoint), connect, &mut out, &mut err);
    Ran { code, out, err }
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
    assert!(ran.out.starts_with(b"sego reads public source code"));
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
