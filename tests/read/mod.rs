use std::time::Duration;

use expect_test::expect;

use crate::command::Run;
use crate::server::{self, Step};
use crate::{sego, sego_at};

const MUX: &str = "github.com/gorilla/mux@v1.7.0";

/// Runs sego with `args` against a server taking `steps`, and checks that
/// the steps were taken as scripted.
fn exchange(steps: Vec<Step>, args: &[&str]) -> Run {
    let server = server::start(steps);
    let run = sego_at(&server).arg("read").args(args).run();
    server.finish();
    run
}

/// The transcript of `sego read <args>` answered by `capture`.
fn read(capture: &'static str, args: &[&str]) -> String {
    exchange(vec![Step::Capture(capture)], args).transcript()
}

fn status(status: u16, body: &str) -> Step {
    Step::Status { status, headers: Vec::new(), body: body.to_owned() }
}

#[test]
fn range() {
    expect![[r#"
        exit 0
        --- stdout
        github.com/gorilla/mux@a7962380ca08 mux.go 213-219 • 601 lines
         213: }
         214:
         215: // Get returns a route registered with the given name.
         216: func (r *Router) Get(name string) *Route {
         217: 	return r.namedRoutes[name]
         218: }
         219:
        [EOF]
        --- stderr
        [EOF]
    "#]]
    .assert_eq(&read("read/range.json", &[MUX, "mux.go", "213-219"]));
}

#[test]
fn range_json() {
    expect![[r#"
        exit 0
        --- stdout
        {"version":1,"kind":"file","repo":"github.com/gorilla/mux","commit":"a7962380ca08b5a188038c69871b8d3fbdf31e89","path":"mux.go","byte_size":17526,"total_lines":601,"content_type":"text/plain; charset=utf-8","binary":false,"start":213,"end":219,"lines":[{"n":213,"text":"}"},{"n":214,"text":""},{"n":215,"text":"// Get returns a route registered with the given name."},{"n":216,"text":"func (r *Router) Get(name string) *Route {"},{"n":217,"text":"\treturn r.namedRoutes[name]"},{"n":218,"text":"}"},{"n":219,"text":""}]}
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&read(
        "read/range.json",
        &[MUX, "mux.go", "213-219", "--json"],
    ));
}

#[test]
fn whole_file_is_capped() {
    let run =
        exchange(vec![Step::Capture("read/capped.json")], &[MUX, "mux.go"]);
    assert_eq!((run.code, run.stderr.as_str()), (0, ""));
    let lines: Vec<&str> = run.stdout.lines().collect();
    assert_eq!(lines.len(), 1 + 500 + 2);
    let shown = [&lines[..3], &["…"], &lines[lines.len() - 4..]].concat();
    expect![[r#"
        github.com/gorilla/mux@a7962380ca08 mux.go 1-500 • 601 lines
           1: // Copyright 2012 The Gorilla Authors. All rights reserved.
           2: // Use of this source code is governed by a BSD-style
        …
         499: func mapFromPairsToString(pairs ...string) (map[string]string, error) {
         500: 	length, err := checkPairs(pairs...)
        notice: lines 501-601 not shown
        hint: sego read github.com/gorilla/mux@a7962380ca08 mux.go 501-
    "#]].assert_eq(&(shown.join("\n") + "\n"));
}

#[test]
fn whole_file_is_capped_json() {
    let run = exchange(
        vec![Step::Capture("read/capped.json")],
        &[MUX, "mux.go", "--json"],
    );
    assert_eq!((run.code, run.stderr.as_str()), (0, ""));
    let document: serde_json::Value =
        serde_json::from_str(&run.stdout).unwrap();
    assert_eq!(document["lines"].as_array().unwrap().len(), 500);
    let lines = run.stdout.find(r#""lines":["#).unwrap();
    let partial = run.stdout.find(r#""partial":"#).unwrap();
    let elided = [&run.stdout[..lines], "…,", &run.stdout[partial..]];
    expect![[r#"
        {"version":1,"kind":"file","repo":"github.com/gorilla/mux","commit":"a7962380ca08b5a188038c69871b8d3fbdf31e89","path":"mux.go","byte_size":17526,"total_lines":601,"content_type":"text/plain; charset=utf-8","binary":false,"start":1,"end":500,…,"partial":{"reason":"lines 501-601 not shown","hints":["sego read github.com/gorilla/mux@a7962380ca08 mux.go 501-"]}}
    "#]].assert_eq(&elided.concat());
}

#[test]
fn directory_is_not_a_file() {
    expect![[r#"
        exit 1
        --- stdout
        [EOF]
        --- stderr
        .github is a directory in github.com/gorilla/mux@a7962380ca08
        hint: sego list github.com/gorilla/mux@a7962380ca08 .github/
        [EOF]
    "#]]
    .assert_eq(&read("read/directory.json", &[MUX, ".github"]));
}

#[test]
fn missing_repo() {
    expect![[r#"
        exit 1
        --- stdout
        [EOF]
        --- stderr
        repository github.com/gorilla/doesnotexist not found
        hint: sego search 'type:repo doesnotexist'
        [EOF]
    "#]]
    .assert_eq(&read(
        "read/missing-repo.json",
        &["github.com/gorilla/doesnotexist", "x"],
    ));
}

#[test]
fn missing_revision() {
    expect![[r#"
        exit 1
        --- stdout
        [EOF]
        --- stderr
        revision v9.9.9 not found in github.com/gorilla/mux
        hint: sego list github.com/gorilla/mux
        [EOF]
    "#]]
    .assert_eq(&read(
        "read/missing-revision.json",
        &["github.com/gorilla/mux@v9.9.9", "mux.go"],
    ));
}

#[test]
fn missing_path() {
    expect![[r#"
        exit 1
        --- stdout
        [EOF]
        --- stderr
        nope.go not found in github.com/gorilla/mux@a7962380ca08
        hint: sego list github.com/gorilla/mux@a7962380ca08
        [EOF]
    "#]]
    .assert_eq(&read("read/missing-path.json", &[MUX, "nope.go"]));
}

#[test]
fn past_end() {
    expect![[r#"
        exit 1
        --- stdout
        [EOF]
        --- stderr
        mux.go in github.com/gorilla/mux@a7962380ca08 has 601 lines, so line 700 is past its end
        hint: sego read github.com/gorilla/mux@a7962380ca08 mux.go 1-601
        [EOF]
    "#]]
        .assert_eq(&read("read/past-end.json", &[MUX, "mux.go", "700-710"]));
}

#[test]
fn too_large() {
    expect![[r#"
        exit 1
        --- stdout
        [EOF]
        --- stderr
        src/compiler/checker.ts in github.com/microsoft/TypeScript@a7e3374f1332 is 3057771 bytes, over the upstream's 1.5 MiB read cap
        [EOF]
    "#]].assert_eq(&read(
        "read/too-large.json",
        &[
            "github.com/microsoft/TypeScript@v5.6.2",
            "src/compiler/checker.ts",
            "1-3",
        ],
    ));
}

#[test]
fn submodule_is_not_a_file() {
    expect![[r#"
        exit 1
        --- stdout
        [EOF]
        --- stderr
        src/tools/cargo is a submodule in github.com/rust-lang/rust@051478957371
        hint: sego list github.com/rust-lang/rust@051478957371 src/tools/cargo
        [EOF]
    "#]].assert_eq(&read(
        "read/submodule.json",
        &["github.com/rust-lang/rust@1.80.0", "src/tools/cargo"],
    ));
}

#[test]
fn binary() {
    expect![[r#"
        exit 0
        --- stdout
        github.com/golang/go@6885bad7dd86 src/archive/zip/testdata/gophercolor16x16.png • binary • 785 bytes
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&read(
        "read/binary.json",
        &[
            "github.com/golang/go@go1.23.0",
            "src/archive/zip/testdata/gophercolor16x16.png",
        ],
    ));
}

#[test]
fn unavailable_json_goes_to_both_streams() {
    expect![[r#"
        exit 1
        --- stdout
        {"version":1,"kind":"unavailable","code":"path_not_found","message":"nope.go not found in github.com/gorilla/mux@a7962380ca08","hints":["sego list github.com/gorilla/mux@a7962380ca08"]}
        [EOF]
        --- stderr
        nope.go not found in github.com/gorilla/mux@a7962380ca08
        hint: sego list github.com/gorilla/mux@a7962380ca08
        [EOF]
    "#]].assert_eq(&read(
        "read/missing-path.json",
        &[MUX, "nope.go", "--json"],
    ));
}

#[test]
fn invalid_ranges_send_nothing() {
    let server = server::start(Vec::new());
    let runs = [
        [MUX, "mux.go", "0"],
        [MUX, "mux.go", "5-3"],
        ["github.com/gorilla/mux@v1..v2", "mux.go", "1"],
    ]
    .map(|args| sego_at(&server).arg("read").args(args).run().transcript());
    server.finish();
    expect![[r#"
        exit 2
        --- stdout
        [EOF]
        --- stderr
        error: invalid line range 0; lines are numbered from 1
        hint: sego read --help
        [EOF]
        exit 2
        --- stdout
        [EOF]
        --- stderr
        error: invalid line range 5-3; the end is before the start
        hint: sego read --help
        [EOF]
        exit 2
        --- stdout
        [EOF]
        --- stderr
        error: invalid revision v1..v2; a revision cannot contain ".."
        hint: sego read --help
        [EOF]
    "#]]
    .assert_eq(&runs.concat());
}

#[test]
fn retries_once_after_503() {
    let steps = vec![status(503, "busy"), Step::Capture("read/range.json")];
    let run = exchange(steps, &[MUX, "mux.go", "213-219"]);
    assert_eq!((run.code, run.stderr.as_str()), (0, ""));
}

#[test]
fn a_second_failure_is_unanswered() {
    let steps = vec![status(503, "busy"), status(503, "still busy")];
    let run = exchange(steps, &[MUX, "mux.go", "213-219", "--json"]);
    expect![[r#"
        exit 3
        --- stdout
        {"version":1,"kind":"error","code":"bad_response","message":"the upstream answered HTTP 503: still busy","hints":[],"detail":{"trace":"test-trace"}}
        [EOF]
        --- stderr
        error: the upstream answered HTTP 503: still busy
        [EOF]
    "#]].assert_eq(&run.transcript());
}

#[test]
fn dropped_connections_are_retried() {
    let steps = vec![Step::Drop, Step::Capture("read/range.json")];
    let run = exchange(steps, &[MUX, "mux.go", "213-219"]);
    assert_eq!((run.code, run.stderr.as_str()), (0, ""), "{}", run.stderr);
}

#[test]
fn rate_limits_with_retry_after_are_retried() {
    let limited = Step::Status {
        status: 429,
        headers: vec![("Retry-After", "1".to_owned())],
        body: "slow down".to_owned(),
    };
    let steps = vec![limited, Step::Capture("read/range.json")];
    let run = exchange(steps, &[MUX, "mux.go", "213-219"]);
    assert_eq!((run.code, run.stderr.as_str()), (0, ""), "{}", run.stderr);
}

#[test]
fn a_slow_answer_is_awaited() {
    let steps = vec![
        Step::Stall(Duration::from_millis(500)),
        Step::Capture("read/range.json"),
    ];
    let run = exchange(steps, &[MUX, "mux.go", "213-219"]);
    assert_eq!((run.code, run.stderr.as_str()), (0, ""), "{}", run.stderr);
}

/// `text` with each `os error <n>` as `os error N`: errno values differ
/// between platforms.
fn without_errno(text: &str) -> String {
    let mut parts = text.split("os error ");
    let mut out = parts.next().unwrap_or_default().to_owned();
    for part in parts {
        out.push_str("os error N");
        out.push_str(part.trim_start_matches(|c: char| c.is_ascii_digit()));
    }
    out
}

#[test]
fn refused_connections_are_transport_errors() {
    let endpoint = server::unreachable();
    let run = sego()
        .env("SEGO_ENDPOINT", &endpoint)
        .args(["read", MUX, "mux.go", "--json"])
        .run();
    let transcript = run.transcript().replace(&endpoint, "$SEGO_ENDPOINT");
    expect![[r#"
        exit 3
        --- stdout
        {"version":1,"kind":"error","code":"transport","message":"the request to $SEGO_ENDPOINT/.api/graphql failed: Connection refused (os error N)","hints":[]}
        [EOF]
        --- stderr
        error: the request to $SEGO_ENDPOINT/.api/graphql failed
        caused by: Connection refused (os error N)
        [EOF]
    "#]].assert_eq(&without_errno(&transcript));
}

#[test]
fn redirects_name_their_location() {
    let moved = Step::Status {
        status: 301,
        headers: vec![(
            "Location",
            "https://example.com/.api/graphql".to_owned(),
        )],
        body: "moved".to_owned(),
    };
    let run = exchange(vec![moved], &[MUX, "mux.go"]);
    expect![[r#"
        exit 3
        --- stdout
        [EOF]
        --- stderr
        error: the upstream answered HTTP 301 with Location https://example.com/.api/graphql: moved
        [EOF]
    "#]].assert_eq(&run.transcript());
}

#[test]
fn the_trace_reaches_json_detail() {
    let failed = Step::Status {
        status: 500,
        headers: vec![("x-trace", "t1".to_owned())],
        body: "boom".to_owned(),
    };
    let run = exchange(vec![failed], &[MUX, "mux.go", "--json"]);
    expect![[r#"
        exit 3
        --- stdout
        {"version":1,"kind":"error","code":"bad_response","message":"the upstream answered HTTP 500: boom","hints":[],"detail":{"trace":"t1"}}
        [EOF]
        --- stderr
        error: the upstream answered HTTP 500: boom
        [EOF]
    "#]].assert_eq(&run.transcript());
}

#[test]
fn an_endpoint_without_a_scheme_is_invalid() {
    let run = sego()
        .env("SEGO_ENDPOINT", "sourcegraph.com")
        .args(["read", MUX, "mux.go"])
        .run();
    expect![[r#"
        exit 2
        --- stdout
        [EOF]
        --- stderr
        error: SEGO_ENDPOINT must start with http:// or https://: sourcegraph.com
        hint: sego --help
        [EOF]
    "#]].assert_eq(&run.transcript());
}

#[test]
fn a_trailing_slash_endpoint_works() {
    let server = server::start(vec![Step::Capture("read/range.json")]);
    let run = sego()
        .env("SEGO_ENDPOINT", format!("{}/", server.url()))
        .args(["read", MUX, "mux.go", "213-219"])
        .run();
    server.finish();
    assert_eq!((run.code, run.stderr.as_str()), (0, ""), "{}", run.stderr);
}

#[test]
fn closed_stdout_is_quiet() {
    let server = server::start(vec![Step::Capture("read/capped.json")]);
    let run = sego_at(&server)
        .args(["read", MUX, "mux.go"])
        .run_with_closed_stdout();
    server.finish();
    assert_eq!((run.code, run.stderr.as_str()), (0, ""), "{}", run.stderr);
}

#[test]
fn help() {
    check!(
        sego().args(["read", "--help"]),
        expect![[r#"
            exit 0
            --- stdout
            sego read prints the numbered lines of a file at a revision.

            Usage:
              sego read [--json] <repo>[@<rev>] <path> [<lines>]

            Operands:
              <repo>    a repository, such as github.com/gorilla/mux; a URL, a trailing
                        .git and a trailing slash are accepted. Matching is
                        case-insensitive.
              <rev>     a branch, tag or commit-ish, such as main, v1.7.0 or
                        a7962380~1; it cannot contain "..". Without it, the default
                        branch.
              <path>    the file's path from the repository root
              <lines>   N, N-M, N- (to the last line) or N,M (the same as N-M); 1-based
                        and inclusive. Without it, the file up to line 500.

            Output:
              An address line: the repository at the commit the revision resolved to,
              the path and the lines shown, then the file's line count. Then the
              numbered lines:

                github.com/gorilla/mux@a7962380ca08 mux.go 23-24 • 601 lines
                  23: // NewRouter returns a new router instance.
                  24: func NewRouter() *Router {

              A file longer than 500 lines, read without <lines>, stops at line 500,
              followed by a notice: line and a hint: line with the command for the
              rest. A binary file has its size in the address line, and no lines. A
              line wider than 500 columns is cut, and says so; --json has it whole.

              With --json, one document of kind "file": repo, commit (40 hex digits),
              path, byte_size, total_lines, content_type, binary, start, end (null
              without lines), lines [{n, text}], and partial {reason, hints} when the
              file stopped at line 500. Anything else is reported on stderr, and its
              document, of kind "unavailable" or "error", also goes to stdout.

            Exit codes:
              0  answered      the lines
              1  answered no   no such repository, revision or path; a directory or a
                               submodule; a file over the upstream's 1.5 MiB read cap,
                               whatever the range; a range that starts past the end
              2  invalid       bad operands, such as line 0, a range that ends before
                               it starts or a revision containing ".."; an invalid
                               SEGO_ENDPOINT
              3  unanswered    no usable answer from the upstream; retry

            Examples:
              sego read github.com/gorilla/mux mux.go
              sego read github.com/gorilla/mux@v1.7.0 mux.go 30
              sego read github.com/gorilla/mux@v1.7.0 mux.go 30-60
              sego read github.com/gorilla/mux@v1.7.0 mux.go 30-
              sego read github.com/gorilla/mux@v1.7.0 mux.go 30,60
            [EOF]
            --- stderr
            [EOF]
        "#]]
    );
}

#[test]
fn help_fits_79_columns() {
    let run = sego().args(["read", "--help"]).run();
    assert_eq!(run.code, 0);
    for line in run.stdout.lines() {
        assert!(line.chars().count() <= 79, "too wide: {line}");
    }
}
