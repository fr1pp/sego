use expect_test::expect;

use crate::command::Run;
use crate::server::{self, Step};
use crate::{sego, sego_at};

const MUX: &str = "github.com/gorilla/mux@v1.7.0";
const GO: &str = "github.com/golang/go@go1.23.0";

/// Runs sego blame with `args` against a server taking `steps`, and checks
/// that the steps were taken as scripted.
fn exchange(steps: Vec<Step>, args: &[&str]) -> Run {
    let server = server::start(steps);
    let run = sego_at(&server).arg("blame").args(args).run();
    server.finish();
    run
}

/// The transcript of `sego blame <args>` answered by `capture`.
fn blame(capture: &'static str, args: &[&str]) -> String {
    exchange(vec![Step::Capture(capture)], args).transcript()
}

#[test]
fn range() {
    expect![[r#"
        exit 0
        --- stdout
        github.com/gorilla/mux@a7962380ca08 mux.go 214-221 • 601 lines

        github.com/gorilla/mux@eac83ba2c004 mux.go • lines 214-216 • 2012-10-03 • moraes • Initial files.
         214:
         215: // Get returns a route registered with the given name.
         216: func (r *Router) Get(name string) *Route {

        github.com/gorilla/mux@758eb64354aa mux.go • lines 217 • 2018-12-07 • Joe Wilner • Improve subroute configuration propagation #422
         217: 	return r.namedRoutes[name]

        github.com/gorilla/mux@eac83ba2c004 mux.go • lines 218-221 • 2012-10-03 • moraes • Initial files.
         218: }
         219:
         220: // GetRoute returns a route registered with the given name. This method
         221: // was renamed to Get() and remains here for backwards compatibility.
        [EOF]
        --- stderr
        [EOF]
    "#]]
        .assert_eq(&blame("blame/range.json", &[MUX, "mux.go", "214-221"]));
}

#[test]
fn range_json() {
    expect![[r#"
        exit 0
        --- stdout
        {"version":1,"kind":"blame","repo":"github.com/gorilla/mux","commit":"a7962380ca08b5a188038c69871b8d3fbdf31e89","path":"mux.go","start":214,"end":221,"total_lines":601,"byte_size":17526,"binary":false,"hunks":[{"start":214,"end":216,"commit":"eac83ba2c004bb759a2875b1f1dbb032adf8bb4a","path":"mux.go","date":"2012-10-03T04:48:17Z","author":{"name":"moraes","email":"moraes@noreply.sourcegraph.com"},"subject":"Initial files.","lines":[{"n":214,"text":""},{"n":215,"text":"// Get returns a route registered with the given name."},{"n":216,"text":"func (r *Router) Get(name string) *Route {"}]},{"start":217,"end":217,"commit":"758eb64354aa27cda6c1b026822c788cc755f06f","path":"mux.go","date":"2018-12-07T15:48:26Z","author":{"name":"Joe Wilner","email":"joewilner@noreply.sourcegraph.com"},"subject":"Improve subroute configuration propagation #422","lines":[{"n":217,"text":"\treturn r.namedRoutes[name]"}]},{"start":218,"end":221,"commit":"eac83ba2c004bb759a2875b1f1dbb032adf8bb4a","path":"mux.go","date":"2012-10-03T04:48:17Z","author":{"name":"moraes","email":"moraes@noreply.sourcegraph.com"},"subject":"Initial files.","lines":[{"n":218,"text":"}"},{"n":219,"text":""},{"n":220,"text":"// GetRoute returns a route registered with the given name. This method"},{"n":221,"text":"// was renamed to Get() and remains here for backwards compatibility."}]}]}
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&blame(
        "blame/range.json",
        &[MUX, "mux.go", "214-221", "--json"],
    ));
}

#[test]
fn whole() {
    let run = exchange(
        vec![Step::Capture("blame/whole.json")],
        &[MUX, "mux.go", "1-"],
    );
    assert_eq!((run.code, run.stderr.as_str()), (0, ""));
    let numbered = run.stdout.lines().filter(|line| {
        let number = line.trim_start().split_once(':').map(|(n, _)| n);
        number.is_some_and(|n| n.parse::<u32>().is_ok())
    });
    let numbered = numbered.count();
    let lines: Vec<&str> = run.stdout.lines().collect();
    let shown = [&lines[..6], &["…"], &lines[lines.len() - 3..]].concat();
    expect![[r#"
        github.com/gorilla/mux@a7962380ca08 mux.go 1-601 • 601 lines

        github.com/gorilla/mux@eac83ba2c004 mux.go • lines 1-7 • 2012-10-03 • moraes • Initial files.
           1: // Copyright 2012 The Gorilla Authors. All rights reserved.
           2: // Use of this source code is governed by a BSD-style
           3: // license that can be found in the LICENSE file.
        …
         599: // methodNotAllowedHandler returns a simple request handler
         600: // that replies to each request with a status code 405.
         601: func methodNotAllowedHandler() http.Handler { return http.HandlerFunc(methodNotAllowed) }
    "#]].assert_eq(&(shown.join("\n") + "\n"));
    assert_eq!(numbered, 601);
}

#[test]
fn end_clamped() {
    expect![[r#"
        exit 0
        --- stdout
        github.com/gorilla/mux@a7962380ca08 mux.go 595-601 • 601 lines

        github.com/gorilla/mux@a659b61323b0 mux.go • lines 595-601 • 2017-08-30 • Mayank Patel • Fix #271:  Return 405 instead of 404 when request method doesn't match the route
         595: func methodNotAllowed(w http.ResponseWriter, r *http.Request) {
         596: 	w.WriteHeader(http.StatusMethodNotAllowed)
         597: }
         598:
         599: // methodNotAllowedHandler returns a simple request handler
         600: // that replies to each request with a status code 405.
         601: func methodNotAllowedHandler() http.Handler { return http.HandlerFunc(methodNotAllowed) }
        [EOF]
        --- stderr
        [EOF]
    "#]]
        .assert_eq(&blame("blame/end-clamped.json", &[MUX, "mux.go", "595-"]));
}

#[test]
fn end_past() {
    expect![[r#"
        exit 0
        --- stdout
        github.com/gorilla/mux@a7962380ca08 mux.go 595-601 • 601 lines

        github.com/gorilla/mux@a659b61323b0 mux.go • lines 595-601 • 2017-08-30 • Mayank Patel • Fix #271:  Return 405 instead of 404 when request method doesn't match the route
         595: func methodNotAllowed(w http.ResponseWriter, r *http.Request) {
         596: 	w.WriteHeader(http.StatusMethodNotAllowed)
         597: }
         598:
         599: // methodNotAllowedHandler returns a simple request handler
         600: // that replies to each request with a status code 405.
         601: func methodNotAllowedHandler() http.Handler { return http.HandlerFunc(methodNotAllowed) }
        [EOF]
        --- stderr
        [EOF]
    "#]]
        .assert_eq(&blame("blame/end-past.json", &[MUX, "mux.go", "595-700"]));
}

#[test]
fn too_large() {
    let args = [GO, "api/go1.txt", "1-5"];
    expect![[r#"
        exit 0
        --- stdout
        github.com/golang/go@6885bad7dd86 api/go1.txt 1-5 • 30871 lines

        github.com/golang/go@f69132d7ad89 api/go1.txt • lines 1-5 • 2012-03-17 • Brad Fitzpatrick • build: catch API changes during build
        notice: the file is over the upstream's 1.5 MiB read cap, so its lines aren't shown
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&blame("blame/too-large.json", &args));
    let json = [&args[..], &["--json"]].concat();
    expect![[r#"
        exit 0
        --- stdout
        {"version":1,"kind":"blame","repo":"github.com/golang/go","commit":"6885bad7dd86880be6929c02085e5c7a67ff2887","path":"api/go1.txt","start":1,"end":5,"total_lines":30871,"byte_size":1759838,"binary":false,"hunks":[{"start":1,"end":5,"commit":"f69132d7ad89c6507af6079bd75d2fe50f7f7fe8","path":"api/go1.txt","date":"2012-03-17T18:20:46Z","author":{"name":"Brad Fitzpatrick","email":"bradfitzpatrick@noreply.sourcegraph.com"},"subject":"build: catch API changes during build"}],"partial":{"reason":"the file is over the upstream's 1.5 MiB read cap, so its lines aren't shown","hints":[]}}
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&blame("blame/too-large.json", &json));
}

#[test]
fn binary() {
    let args = [GO, "src/archive/zip/testdata/gophercolor16x16.png", "1-8"];
    expect![[r#"
        exit 0
        --- stdout
        github.com/golang/go@6885bad7dd86 src/archive/zip/testdata/gophercolor16x16.png • binary • 785 bytes
        notice: a binary file has no lines to blame
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&blame("blame/binary.json", &args));
    let json = [&args[..], &["--json"]].concat();
    expect![[r#"
        exit 0
        --- stdout
        {"version":1,"kind":"blame","repo":"github.com/golang/go","commit":"6885bad7dd86880be6929c02085e5c7a67ff2887","path":"src/archive/zip/testdata/gophercolor16x16.png","start":1,"end":null,"total_lines":8,"byte_size":785,"binary":true,"hunks":[]}
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&blame("blame/binary.json", &json));
}

#[test]
fn binary_over_cap() {
    let path =
        "src/crypto/internal/boring/syso/goboringcrypto_linux_amd64.syso";
    let args = [GO, path, "1-10"];
    expect![[r#"
        exit 0
        --- stdout
        github.com/golang/go@6885bad7dd86 src/crypto/internal/boring/syso/goboringcrypto_linux_amd64.syso • binary • 2555664 bytes
        notice: a binary file has no lines to blame
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&blame("blame/binary-over-cap.json", &args));
    let json = [&args[..], &["--json"]].concat();
    expect![[r#"
        exit 0
        --- stdout
        {"version":1,"kind":"blame","repo":"github.com/golang/go","commit":"6885bad7dd86880be6929c02085e5c7a67ff2887","path":"src/crypto/internal/boring/syso/goboringcrypto_linux_amd64.syso","start":1,"end":null,"total_lines":3790,"byte_size":2555664,"binary":true,"hunks":[]}
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&blame("blame/binary-over-cap.json", &json));
}

#[test]
fn empty() {
    expect![[r#"
        exit 0
        --- stdout
        github.com/golang/go@6885bad7dd86 test/typeparam/issue47740.out • 0 lines
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&blame(
        "blame/empty.json",
        &[GO, "test/typeparam/issue47740.out", "1-"],
    ));
}

#[test]
#[expect(
    clippy::unicode_not_nfc,
    reason = "an author's name prints as the upstream sends it, decomposed"
)]
fn renamed() {
    expect![[r#"
        exit 0
        --- stdout
        github.com/golang/go@6885bad7dd86 src/fmt/print.go 1-20 • 1223 lines

        github.com/golang/go@418b97c67013 src/lib/fmt/print.go • lines 1-7 • 2008-10-24 • Rob Pike • add printf to fmt.
           1: // Copyright 2009 The Go Authors. All rights reserved.
           2: // Use of this source code is governed by a BSD-style
           3: // license that can be found in the LICENSE file.
           4:
           5: package fmt
           6:
           7: import (

        github.com/golang/go@a440cc0d702e src/fmt/print.go • lines 8 • 2018-10-16 • Rob Pike • fmt: print maps in key-sorted order
           8: 	"internal/fmtsort"

        github.com/golang/go@1c729599996c src/pkg/fmt/print.go • lines 9-11 • 2009-12-15 • Robert Griesemer • 1) Change default gofmt default settings for
           9: 	"io"
          10: 	"os"
          11: 	"reflect"

        github.com/golang/go@d75e186e2c01 src/fmt/print.go • lines 12 • 2022-04-19 • Rob Pike • fmt: add a function to recover the original format string given a State
          12: 	"strconv"

        github.com/golang/go@12ad9b4315cc src/pkg/fmt/print.go • lines 13 • 2011-09-29 • Rob Pike • fmt: replace channel cache with slice.
          13: 	"sync"

        github.com/golang/go@30aa701fec9f src/pkg/fmt/print.go • lines 14 • 2011-11-08 • Rob Pike • renaming_2: gofix -r go1pkgrename src/pkg/[a-l]*
          14: 	"unicode/utf8"

        github.com/golang/go@418b97c67013 src/lib/fmt/print.go • lines 15-16 • 2008-10-24 • Rob Pike • add printf to fmt.
          15: )
          16:

        github.com/golang/go@7da4ceddd020 src/fmt/print.go • lines 17-20 • 2016-02-21 • Martin Möhrmann • fmt: simplify buffer write methods and adjust calls to them
          17: // Strings for use with buffer.WriteString.
          18: // This is less overhead than using buffer.Write with byte arrays.
          19: const (
          20: 	commaSpaceString  = ", "
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&blame(
        "blame/renamed.json",
        &[GO, "src/fmt/print.go", "1-20"],
    ));
}

#[test]
fn submodule() {
    expect![[r#"
        exit 1
        --- stdout
        [EOF]
        --- stderr
        src/tools/cargo is a submodule in github.com/rust-lang/rust@051478957371
        hint: sego list github.com/rust-lang/rust@051478957371 src/tools/cargo
        [EOF]
    "#]].assert_eq(&blame(
        "blame/submodule.json",
        &["github.com/rust-lang/rust@1.80.0", "src/tools/cargo", "1"],
    ));
}

#[test]
fn directory() {
    expect![[r#"
        exit 1
        --- stdout
        [EOF]
        --- stderr
        .github is a directory in github.com/gorilla/mux@a7962380ca08
        hint: sego list github.com/gorilla/mux@a7962380ca08 .github/
        [EOF]
    "#]]
    .assert_eq(&blame("blame/directory.json", &[MUX, ".github", "1"]));
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
    .assert_eq(&blame("blame/missing-path.json", &[MUX, "nope.go", "1"]));
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
    .assert_eq(&blame(
        "blame/missing-revision.json",
        &["github.com/gorilla/mux@v9.9.9", "mux.go", "1"],
    ));
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
    .assert_eq(&blame(
        "blame/missing-repo.json",
        &["github.com/gorilla/doesnotexist", "x", "1"],
    ));
}

#[test]
fn past_end() {
    expect![[r#"
        exit 1
        --- stdout
        [EOF]
        --- stderr
        mux.go in github.com/gorilla/mux@a7962380ca08 has 601 lines, so line 700 is past its end
        hint: sego blame github.com/gorilla/mux@a7962380ca08 mux.go 1-601
        [EOF]
    "#]]
        .assert_eq(&blame("blame/past-end.json", &[MUX, "mux.go", "700-710"]));
}

#[test]
fn usage() {
    check!(
        sego().args(["blame", MUX, "mux.go"]),
        expect![[r#"
            exit 2
            --- stdout
            [EOF]
            --- stderr
            error: missing <lines>; expected <repo>[@<rev>] <path> <lines>
            usage: sego blame [--json] <repo>[@<rev>] <path> <lines>
            [EOF]
        "#]]
    );
}

#[test]
fn help() {
    check!(
        sego().args(["blame", "--help"]),
        expect![[r#"
            exit 0
            --- stdout
            sego blame prints the commit that last changed each of a file's lines at
            a revision, with the lines.

            Usage:
              sego blame [--json] <repo>[@<rev>] <path> <lines>

            Operands:
              <repo>    a repository, such as github.com/gorilla/mux; a URL, a trailing
                        .git and a trailing slash are accepted. Matching is
                        case-insensitive.
              <rev>     a branch, tag or commit-ish, such as main, v1.7.0 or
                        a7962380~1; it cannot contain "..". Without it, the default
                        branch.
              <path>    the file's path from the repository root
              <lines>   N, N-M, N- (to the last line) or N,M (the same as N-M); 1-based
                        and inclusive. 1- is the whole file.

            Output:
              An address line: the repository at the commit the revision resolved to,
              the path and the lines blamed, then the file's line count. Then each
              hunk, after a blank line: an address line with the commit that last
              changed its lines and the path as it was in that commit, then the
              hunk's lines, the day of the author date (UTC), the author's name and
              the commit's subject; then the hunk's numbered lines. A hunk's line
              numbers are the blamed revision's, not its commit's:

                github.com/gorilla/mux@a7962380ca08 mux.go 214-221 • 601 lines

                github.com/gorilla/mux@eac83ba2c004 mux.go • lines 214-216 • 2012-10-03 • …
                 214:
                 215: // Get returns a route registered with the given name.
                 216: func (r *Router) Get(name string) *Route {

              The blame follows the repository's .git-blame-ignore-revs, as
              git blame --ignore-revs-file .git-blame-ignore-revs does. Its cost
              follows the history of the lines, not the width of the range.

              A text file over the upstream's 1.5 MiB read cap has its hunks without
              their lines, followed by a notice: line. A binary file has its size in
              the address line, then a notice: line, as a binary file has no lines to
              blame. A line wider than 500 columns is cut, and says so; --json has it
              whole.

              With --json, one document of kind "blame": repo, commit (40 hex digits),
              path, start, end (null without hunks), total_lines, byte_size, binary,
              and hunks [{start, end, commit, path, date, author {name, email},
              subject, lines [{n, text}]}]; a hunk's start and end number the blamed
              revision's lines. Over the read cap, the hunks have no lines and
              partial {reason, hints} says why. Anything else is reported on stderr,
              and its document, of kind "unavailable" or "error", also goes to
              stdout.

            Exit codes:
              0  answered      the hunks; a binary file's size
              1  answered no   no such repository, revision or path; a directory or a
                               submodule; a range that starts past the end
              2  invalid       bad operands, such as no <lines>, line 0, a range that
                               ends before it starts or a revision containing "..";
                               an invalid SEGO_ENDPOINT
              3  unanswered    no usable answer from the upstream; retry. A blame with
                               no answer within 95 s will take as long again.

            Examples:
              sego blame github.com/gorilla/mux@v1.7.0 mux.go 214-221
              sego blame github.com/gorilla/mux@v1.7.0 mux.go 30
              sego blame github.com/gorilla/mux@v1.7.0 mux.go 1-
            [EOF]
            --- stderr
            [EOF]
        "#]]
    );
}

#[test]
fn help_fits_79_columns() {
    let run = sego().args(["blame", "--help"]).run();
    assert_eq!(run.code, 0);
    for line in run.stdout.lines() {
        assert!(line.chars().count() <= 79, "too wide: {line}");
    }
}
