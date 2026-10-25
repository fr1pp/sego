use std::cell::Cell;
use std::ffi::OsStr;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::address;
use crate::cache::Cache;
use crate::error::report;
use crate::output::Notes;
use crate::upstream::{Reply, Transport, TransportError};

use super::run;

const SHA: &str = "d81fd54f1a2b3c4d5e6f708192a3b4c5d6e7f809";

/// Answers the one GraphQL request with `reply` and counts the raw reads.
struct Server {
    reply: Cell<Option<Reply>>,
    raw_reads: Cell<usize>,
}

impl Transport for Server {
    fn post(
        &self,
        _: &str,
        _: &[u8],
        _: Instant,
    ) -> Result<Reply, TransportError> {
        Ok(self.reply.take().expect("one GraphQL request"))
    }

    fn get(&self, _: &str, _: Instant) -> Result<Reply, TransportError> {
        self.raw_reads.set(self.raw_reads.get() + 1);
        Err(TransportError::Network("no raw route"))
    }
}

fn file() -> Value {
    json!({"data": {"r0": {
        "name": "example.com/x/y",
        "defaultBranch": {"displayName": "main"},
        "c0": {
            "oid": SHA,
            "meta": {
                "byteSize": 6, "totalLines": 3, "binary": false,
                "rawURL": format!("/r/example.com/x/y@{SHA}/-/raw/a.go"),
                "lfs": null
            },
            "body": {"content": "a\nb\nc\n"},
            "dir": null,
            "parent": {"entries": [{"path": "a.go", "submodule": null}]}
        }
    }}, "errors": [{"message": "not a directory", "path": ["r0", "c0", "dir"]}]})
}

fn over_the_content_limit() -> Value {
    let mut envelope = file();
    let c0 = &mut envelope["data"]["r0"]["c0"];
    c0["meta"]["byteSize"] = json!(2_000_000);
    c0["body"] = json!(null);
    envelope["errors"] = errors(&[
        (&["r0", "c0", "dir"], "not a directory"),
        (&["r0", "c0", "body", "content"], "file exceeded size limit"),
        (&["r0", "c0", "meta", "lfs"], "file exceeded size limit"),
    ]);
    envelope
}

fn errors(list: &[(&[&str], &str)]) -> Value {
    let list: Vec<_> = list
        .iter()
        .map(|(path, message)| json!({"message": message, "path": path}))
        .collect();
    json!(list)
}

/// What `sego show example.com/x/y@main:a.go` writes to stderr as an error
/// on `envelope`, and how many raw reads it made.
fn failure(envelope: &Value) -> (String, usize) {
    let server = Server {
        reply: Cell::new(Some(Reply {
            status: 200,
            content_type: Some("application/json".to_owned()),
            location: None,
            retry_after: None,
            cf_mitigated: None,
            body: envelope.to_string().into_bytes(),
        })),
        raw_reads: Cell::new(0),
    };
    let address = address::parse("example.com/x/y@main:a.go")
        .expect("a well-formed address");
    let cache = Cache::new(Some(OsStr::new("off")), String::new());
    let deadline = Instant::now() + Duration::from_secs(90);
    let error =
        run(&address, None, &server, deadline, &cache, &mut Notes::new())
            .err()
            .expect("the show fails");
    let mut written = Vec::new();
    report(&error, &mut written).expect("a vector takes a write");
    (String::from_utf8_lossy(&written).into_owned(), server.raw_reads.get())
}

#[test]
fn upstream_errors_name_the_part_and_hide_the_server_text() {
    let secret = "git exec: /srv/secret-host failed";
    let mut body = file();
    body["data"]["r0"]["c0"]["body"] = json!(null);
    body["errors"] = errors(&[(&["r0", "c0", "body"], secret)]);

    let mut meta = file();
    meta["data"]["r0"]["c0"]["meta"] = json!(null);
    meta["errors"] = errors(&[(&["r0", "c0", "meta"], secret)]);

    let mut parent = file();
    let c0 = &mut parent["data"]["r0"]["c0"];
    c0["meta"] = json!(null);
    c0["parent"] = json!(null);
    parent["errors"] = errors(&[
        (&["r0", "c0", "meta"], secret),
        (&["r0", "c0", "parent"], secret),
    ]);

    for (envelope, part) in [
        (body, "the file content"),
        (meta, "the file's details"),
        (parent, "the directory listing"),
    ] {
        let (written, _) = failure(&envelope);
        assert_eq!(
            written,
            format!("sego: error: Sourcegraph failed to answer {part}\n")
        );
    }
}

#[test]
fn only_the_limit_error_at_meta_lfs_means_not_lfs() {
    let secret = "lfs backend exploded";
    let mut envelope = file();
    envelope["data"]["r0"]["c0"]["meta"]["lfs"] = json!(null);
    envelope["errors"] = errors(&[(&["r0", "c0", "meta", "lfs"], secret)]);
    let (written, _) = failure(&envelope);
    assert_eq!(
        written,
        "sego: error: Sourcegraph failed to answer the file's details\n"
    );
}

#[test]
fn a_raw_url_that_is_not_a_site_path_is_not_read() {
    for raw_url in
        ["@evil.example/x", "//evil.example/x", "https://evil.example/x", ""]
    {
        let mut envelope = over_the_content_limit();
        envelope["data"]["r0"]["c0"]["meta"]["rawURL"] = json!(raw_url);
        let (written, raw_reads) = failure(&envelope);
        assert_eq!(
            written,
            "sego: error: Sourcegraph failed to answer the file's details\n",
            "{raw_url}"
        );
        assert_eq!(raw_reads, 0, "{raw_url}");
    }
}
