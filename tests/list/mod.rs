use expect_test::expect;

use crate::command::Run;
use crate::server::{self, Step};
use crate::{sego, sego_at};

const MUX: &str = "github.com/gorilla/mux@v1.7.0";
const RUST: &str = "github.com/rust-lang/rust@1.80.0";

/// Runs `sego list <args>` against a server answering with `capture`.
fn run(capture: &'static str, args: &[&str]) -> Run {
    let server = server::start(vec![Step::Capture(capture)]);
    let run = sego_at(&server).arg("list").args(args).run();
    server.finish();
    run
}

/// The transcript of `sego list <args>` answered by `capture`.
fn list(capture: &'static str, args: &[&str]) -> String {
    run(capture, args).transcript()
}

#[test]
fn root() {
    expect![[r#"
        exit 0
        --- stdout
        github.com/gorilla/mux@a7962380ca08 • 21 entries
        .github/
        .travis.yml
        AUTHORS
        ISSUE_TEMPLATE.md
        LICENSE
        README.md
        bench_test.go
        context.go
        context_test.go
        doc.go
        example_authentication_middleware_test.go
        example_route_test.go
        go.mod
        middleware.go
        middleware_test.go
        mux.go
        mux_test.go
        old_test.go
        regexp.go
        route.go
        test_helpers.go
        [EOF]
        --- stderr
        [EOF]
    "#]]
    .assert_eq(&list("list/root.json", &[MUX]));
}

#[test]
fn root_json() {
    expect![[r#"
        exit 0
        --- stdout
        {"version":1,"kind":"listing","repo":"github.com/gorilla/mux","commit":"a7962380ca08b5a188038c69871b8d3fbdf31e89","path":"","entries":[{"path":".github","type":"directory"},{"path":".travis.yml","type":"file"},{"path":"AUTHORS","type":"file"},{"path":"ISSUE_TEMPLATE.md","type":"file"},{"path":"LICENSE","type":"file"},{"path":"README.md","type":"file"},{"path":"bench_test.go","type":"file"},{"path":"context.go","type":"file"},{"path":"context_test.go","type":"file"},{"path":"doc.go","type":"file"},{"path":"example_authentication_middleware_test.go","type":"file"},{"path":"example_route_test.go","type":"file"},{"path":"go.mod","type":"file"},{"path":"middleware.go","type":"file"},{"path":"middleware_test.go","type":"file"},{"path":"mux.go","type":"file"},{"path":"mux_test.go","type":"file"},{"path":"old_test.go","type":"file"},{"path":"regexp.go","type":"file"},{"path":"route.go","type":"file"},{"path":"test_helpers.go","type":"file"}]}
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&list("list/root.json", &[MUX, "--json"]));
}

#[test]
fn paths_starting_with_a_dash_print_with_a_leading_slash() {
    let mut body =
        server::capture("list/root.json")["response"]["body"].clone();
    body["data"]["repository"]["commit"]["node"] = serde_json::json!({
        "__typename": "GitTree",
        "path": "--d",
        "entries": [
            {"isDirectory": true, "path": "--d/-x", "submodule": null},
            {"isDirectory": false, "path": "--d/--weird.go", "submodule": null},
            {
                "isDirectory": false,
                "path": "--d/-s",
                "submodule": {
                    "commit": "37629051518c3df9ac2c1744589362a02ecafa99",
                    "url": "https://github.com/rust-lang/cargo.git",
                },
            },
        ],
    });
    let answer = Step::Status {
        status: 200,
        headers: Vec::new(),
        body: body.to_string(),
    };
    let server = server::start(vec![answer]);
    let run = sego_at(&server).args(["list", MUX, "/--d"]).run();
    server.finish();
    expect![[r#"
        exit 0
        --- stdout
        github.com/gorilla/mux@a7962380ca08 /--d/ • 3 entries
        /--d/--weird.go
        /--d/-s • submodule github.com/rust-lang/cargo@37629051518c
        /--d/-x/
        [EOF]
        --- stderr
        [EOF]
    "#]]
    .assert_eq(&run.transcript());
}

#[test]
fn depth_2() {
    expect![[r#"
        exit 0
        --- stdout
        github.com/gorilla/mux@a7962380ca08 • 23 entries
        .github/
        .github/release-drafter.yml
        .github/stale.yml
        .travis.yml
        AUTHORS
        ISSUE_TEMPLATE.md
        LICENSE
        README.md
        bench_test.go
        context.go
        context_test.go
        doc.go
        example_authentication_middleware_test.go
        example_route_test.go
        go.mod
        middleware.go
        middleware_test.go
        mux.go
        mux_test.go
        old_test.go
        regexp.go
        route.go
        test_helpers.go
        [EOF]
        --- stderr
        [EOF]
    "#]]
    .assert_eq(&list("list/depth-2.json", &[MUX, "--depth", "2"]));
}

#[test]
fn subdirectory() {
    expect![[r#"
        exit 0
        --- stdout
        github.com/gorilla/mux@a7962380ca08 .github/ • 2 entries
        .github/release-drafter.yml
        .github/stale.yml
        [EOF]
        --- stderr
        [EOF]
    "#]]
    .assert_eq(&list("list/subdirectory.json", &[MUX, ".github"]));
}

#[test]
fn truncated() {
    let run = run(
        "list/truncated.json",
        &["github.com/golang/go@go1.23.0", "--depth", "3"],
    );
    assert_eq!((run.code, run.stderr.as_str()), (0, ""));
    let lines: Vec<&str> = run.stdout.lines().collect();
    assert_eq!(lines.len(), 1 + 216 + 2);
    let shown = [&lines[..3], &["…"], &lines[lines.len() - 4..]].concat();
    expect![[r#"
        github.com/golang/go@6885bad7dd86 • 216 entries
        .github/
        .github/ISSUE_TEMPLATE/
        …
        src/math/bits/
        src/math/cmplx/
        notice: listing stopped at 1000 entries; some entries are not shown
        hint: sego list github.com/golang/go@6885bad7dd86 --depth 2
    "#]]
    .assert_eq(&(shown.join("\n") + "\n"));
}

#[test]
fn truncated_json() {
    let run = run(
        "list/truncated.json",
        &["github.com/golang/go@go1.23.0", "--depth", "3", "--json"],
    );
    assert_eq!((run.code, run.stderr.as_str()), (0, ""));
    let document: serde_json::Value =
        serde_json::from_str(&run.stdout).unwrap();
    assert_eq!(document["entries"].as_array().unwrap().len(), 216);
    let entries = run.stdout.find(r#""entries":["#).unwrap();
    let partial = run.stdout.find(r#""partial":"#).unwrap();
    let elided = [&run.stdout[..entries], "…,", &run.stdout[partial..]];
    expect![[r#"
        {"version":1,"kind":"listing","repo":"github.com/golang/go","commit":"6885bad7dd86880be6929c02085e5c7a67ff2887","path":"",…,"partial":{"reason":"listing stopped at 1000 entries; some entries are not shown","hints":["sego list github.com/golang/go@6885bad7dd86 --depth 2"]}}
    "#]].assert_eq(&elided.concat());
}

#[test]
fn submodules() {
    expect![[r#"
        exit 0
        --- stdout
        github.com/rust-lang/rust@051478957371 src/tools/ • 47 entries
        src/tools/build-manifest/
        src/tools/build_helper/
        src/tools/bump-stage0/
        src/tools/cargo • submodule github.com/rust-lang/cargo@37629051518c
        src/tools/cargotest/
        src/tools/cherry-pick.sh
        src/tools/clippy/
        src/tools/collect-license-metadata/
        src/tools/compiletest/
        src/tools/coverage-dump/
        src/tools/error_index_generator/
        src/tools/generate-copyright/
        src/tools/generate-windows-sys/
        src/tools/html-checker/
        src/tools/jsondocck/
        src/tools/jsondoclint/
        src/tools/libcxx-version/
        src/tools/linkchecker/
        src/tools/lint-docs/
        src/tools/lld-wrapper/
        src/tools/llvm-bitcode-linker/
        src/tools/miri/
        src/tools/miropt-test-tools/
        src/tools/opt-dist/
        src/tools/publish_toolstate.py
        src/tools/remote-test-client/
        src/tools/remote-test-server/
        src/tools/replace-version-placeholder/
        src/tools/rls/
        src/tools/run-make-support/
        src/tools/rust-analyzer/
        src/tools/rust-demangler/
        src/tools/rust-installer/
        src/tools/rustbook/
        src/tools/rustc-perf • submodule github.com/rust-lang/rustc-perf@72daa50ce235
        src/tools/rustdoc/
        src/tools/rustdoc-gui/
        src/tools/rustdoc-gui-test/
        src/tools/rustdoc-js/
        src/tools/rustdoc-themes/
        src/tools/rustfmt/
        src/tools/suggest-tests/
        src/tools/tidy/
        src/tools/tier-check/
        src/tools/unicode-table-generator/
        src/tools/unstable-book-gen/
        src/tools/x/
        [EOF]
        --- stderr
        [EOF]
    "#]]
        .assert_eq(&list("list/submodules.json", &[RUST, "src/tools/"]));
}

#[test]
fn submodules_json() {
    let run = run("list/submodules.json", &[RUST, "src/tools", "--json"]);
    assert_eq!((run.code, run.stderr.as_str()), (0, ""));
    let document: serde_json::Value =
        serde_json::from_str(&run.stdout).unwrap();
    assert_eq!(document["entries"].as_array().unwrap().len(), 47);
    let start = run.stdout.find(r#"{"path":"src/tools/bump-stage0""#).unwrap();
    let end =
        run.stdout.find(r#"{"path":"src/tools/cherry-pick.sh""#).unwrap();
    expect![[r#"
        {"path":"src/tools/bump-stage0","type":"directory"},{"path":"src/tools/cargo","type":"submodule","submodule":{"url":"https://github.com/rust-lang/cargo.git","repo":"github.com/rust-lang/cargo","commit":"37629051518c3df9ac2c1744589362a02ecafa99"}},{"path":"src/tools/cargotest","type":"directory"},
    "#]].assert_eq(&format!("{}\n", &run.stdout[start..end]));
}

#[test]
fn submodule_answers_with_its_address() {
    expect![[r#"
        exit 0
        --- stdout
        github.com/rust-lang/rust@051478957371 src/tools/cargo • submodule
        github.com/rust-lang/cargo@37629051518c
        [EOF]
        --- stderr
        [EOF]
    "#]]
    .assert_eq(&list("list/submodule.json", &[RUST, "src/tools/cargo"]));
}

#[test]
fn submodule_answers_with_its_address_json() {
    expect![[r#"
        exit 0
        --- stdout
        {"version":1,"kind":"submodule","repo":"github.com/rust-lang/rust","commit":"051478957371ee0084a7c0913941d2a8c4757bb9","path":"src/tools/cargo","submodule":{"url":"https://github.com/rust-lang/cargo.git","repo":"github.com/rust-lang/cargo","commit":"37629051518c3df9ac2c1744589362a02ecafa99"}}
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&list(
        "list/submodule.json",
        &[RUST, "src/tools/cargo", "--json"],
    ));
}

#[test]
fn a_file_is_not_a_directory() {
    expect![[r#"
        exit 1
        --- stdout
        [EOF]
        --- stderr
        mux.go is a file in github.com/gorilla/mux@a7962380ca08
        hint: sego read github.com/gorilla/mux@a7962380ca08 mux.go
        [EOF]
    "#]]
    .assert_eq(&list("list/file.json", &[MUX, "mux.go"]));
}

#[test]
fn missing_path() {
    expect![[r#"
        exit 1
        --- stdout
        [EOF]
        --- stderr
        nope not found in github.com/gorilla/mux@a7962380ca08
        hint: sego list github.com/gorilla/mux@a7962380ca08
        [EOF]
    "#]]
    .assert_eq(&list("list/missing-path.json", &[MUX, "nope/"]));
}

#[test]
fn depth_zero_is_invalid() {
    let server = server::start(Vec::new());
    let runs = [&[MUX, "--depth", "0"][..], &[MUX, "a", "b"]].map(|args| {
        sego_at(&server).arg("list").args(args).run().transcript()
    });
    server.finish();
    expect![[r#"
        exit 2
        --- stdout
        [EOF]
        --- stderr
        error: invalid --depth 0; expected a number from 1 to 4294967295
        usage: sego list [--json] [--depth N] <repo>[@<rev>] [<path>]
        [EOF]
        exit 2
        --- stdout
        [EOF]
        --- stderr
        error: too many operands; expected <repo>[@<rev>] [<path>]
        usage: sego list [--json] [--depth N] <repo>[@<rev>] [<path>]
        [EOF]
    "#]]
    .assert_eq(&runs.concat());
}

#[test]
fn help() {
    check!(
        sego().args(["list", "--help"]),
        expect![[r#"
        exit 0
        --- stdout
        sego list prints the entries of a directory at a revision.

        Usage:
          sego list [--json] [--depth N] <repo>[@<rev>] [<path>]

        Operands:
          <repo>    a repository, such as github.com/gorilla/mux; a URL, a trailing
                    .git and a trailing slash are accepted. Matching is
                    case-insensitive.
          <rev>     a branch, tag or commit-ish, such as main, v1.7.0 or
                    a7962380~1; it cannot contain "..". Without it, the default
                    branch.
          <path>    the directory's path from the repository root, with or without
                    a trailing /. Without it, the root.

        Options:
          --depth N   entries down to N levels below the directory, at least 1
                      (default 1)

        Output:
          An address line: the repository at the commit the revision resolved to,
          the directory unless it is the root, then the number of entries. Then
          one entry per line, in path order, each a path from the repository root.
          A directory ends in /; a submodule is followed by the repository and
          commit it pins, or by its URL when that names no repository:

            github.com/rust-lang/rust@051478957371 src/tools/ • 47 entries
            src/tools/build_helper/
            src/tools/bump-stage0/
            src/tools/cargo • submodule github.com/rust-lang/cargo@37629051518c

          The upstream stops a listing at 1000 entries, which may leave out
          entries from anywhere in it. Such a listing ends with a notice: line,
          and with --depth above 1 a hint: line for one level less.

          A submodule's path answers with its address line, then, on a line of its
          own, the repository and commit it pins, or its URL.

          With --json, one document of kind "listing": repo, commit (40 hex
          digits), path, entries [{path, type}] with type directory, file or
          submodule, and partial {reason, hints} when the listing stopped at 1000
          entries. A submodule entry has submodule {url, repo, commit}, without
          repo and commit when its URL names no repository. A submodule's path
          gives a document of kind "submodule": repo, commit, path and
          submodule. Anything else is reported on stderr, and its document, of
          kind "unavailable" or "error", also goes to stdout.

        Exit codes:
          0  answered      the entries, or the submodule's address
          1  answered no   no such repository, revision or path; a file
          2  invalid       bad operands, such as --depth 0 or a revision
                           containing ".."; an invalid SEGO_ENDPOINT
          3  unanswered    no usable answer from the upstream; retry

        Examples:
          sego list github.com/gorilla/mux
          sego list github.com/gorilla/mux@v1.7.0
          sego list github.com/gorilla/mux@v1.7.0 .github/
          sego list --depth 2 github.com/gorilla/mux@v1.7.0
        [EOF]
        --- stderr
        [EOF]
    "#]]
    );
}

#[test]
fn help_fits_79_columns() {
    let run = sego().args(["list", "--help"]).run();
    assert_eq!(run.code, 0);
    for line in run.stdout.lines() {
        assert!(line.chars().count() <= 79, "too wide: {line}");
    }
}
