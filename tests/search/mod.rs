use expect_test::expect;
use serde_json::{Value, json};

use crate::command::Run;
use crate::server::{self, Step};
use crate::{sego, sego_at};

const MUX: &str = r"repo:^github\.com/gorilla/mux$";
const MUX_TAG: &str = r"repo:^github\.com/gorilla/mux$@v1.7.0";

/// Runs `sego search <args>` against a server taking `steps`, and checks
/// that the steps were taken as scripted.
fn exchange(steps: Vec<Step>, args: &[&str]) -> Run {
    let server = server::start(steps);
    let run = sego_at(&server).arg("search").args(args).run();
    server.finish();
    run
}

/// The transcript of `sego search <args>` answered by `capture`.
fn search(capture: &'static str, args: &[&str]) -> String {
    exchange(vec![Step::Capture(capture)], args).transcript()
}

/// The response body of `capture`, with its search results changed by
/// `edit`.
fn edited(capture: &str, edit: impl FnOnce(&mut Value)) -> String {
    let mut body = server::capture(capture)["response"]["body"].clone();
    edit(&mut body["data"]["search"]["results"]);
    body.to_string()
}

/// A 200 answer with `body`.
fn ok(body: &str) -> Step {
    Step::Status { status: 200, headers: Vec::new(), body: body.to_owned() }
}

/// `text` with `$` after each line that ends in a space, which a snapshot
/// would lose.
fn ends_shown(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line.ends_with(' ') {
                format!("{line}$\n")
            } else {
                format!("{line}\n")
            }
        })
        .collect()
}

#[test]
fn content() {
    let args = [MUX_TAG, "NewRouter", "case:yes", "count:all"];
    let run = exchange(vec![Step::Capture("search/content.json")], &args);
    assert_eq!((run.code, run.stderr.as_str()), (0, ""));
    let lines: Vec<&str> = run.stdout.lines().collect();
    let mux = lines.iter().position(|&line| line == "mux.go").unwrap();
    let shown =
        [&lines[..16], &["…"], &lines[mux - 1..mux + 9], &["…"]].concat();
    expect![[r#"
        91 matches • 10 results

        github.com/gorilla/mux@a7962380ca08
        example_route_test.go
          13- func ExampleRoute_HeadersRegexp() {
          14: 	r := mux.NewRouter()
          15- 	route := r.NewRoute().HeadersRegexp("Accept", "html")
        --
          35- func ExampleRoute_HeadersRegexp_exactMatch() {
          36: 	r := mux.NewRouter()
          37- 	route := r.NewRoute().HeadersRegexp("Origin", "^https://example.co$")

        example_authentication_middleware_test.go
          38- func Example_authenticationMiddleware() {
          39: 	r := mux.NewRouter()
          40- 	r.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
        …

        mux.go
          22-
          23: // NewRouter returns a new router instance.
          24: func NewRouter() *Router {
          25- 	return &Router{namedRoutes: make(map[string]*Route)}
        --
          32- //
          33: //     var router = mux.NewRouter()
          34- //
        …
    "#]].assert_eq(&(shown.join("\n") + "\n"));
}

#[test]
fn content_json() {
    let args = [MUX_TAG, "NewRouter", "case:yes", "count:all", "--json"];
    let run = exchange(vec![Step::Capture("search/content.json")], &args);
    assert_eq!((run.code, run.stderr.as_str()), (0, ""));
    let document: Value = serde_json::from_str(&run.stdout).unwrap();
    assert_eq!(document["results"].as_array().unwrap().len(), 10);
    let results = run.stdout.find(r#""results":["#).unwrap();
    let second = run.stdout.find(r#",{"type":"file""#).unwrap();
    let shown = [&run.stdout[..second], ",…]}\n"].concat();
    assert!(results < second);
    expect![[r#"
        {"version":1,"kind":"search","query":"repo:^github\\.com/gorilla/mux$@v1.7.0 NewRouter case:yes count:all","match_count":91,"approximate":null,"limit_hit":false,"skipped":{"timed_out":[],"cloning":[],"missing":[]},"alert":null,"results":[{"type":"file","repo":"github.com/gorilla/mux","commit":"a7962380ca08b5a188038c69871b8d3fbdf31e89","path":"example_route_test.go","path_match":false,"binary":false,"lines":[{"n":13,"text":"func ExampleRoute_HeadersRegexp() {","matches":[]},{"n":14,"text":"\tr := mux.NewRouter()","matches":[{"start":10,"end":19}]},{"n":15,"text":"\troute := r.NewRoute().HeadersRegexp(\"Accept\", \"html\")","matches":[]},{"n":35,"text":"func ExampleRoute_HeadersRegexp_exactMatch() {","matches":[]},{"n":36,"text":"\tr := mux.NewRouter()","matches":[{"start":10,"end":19}]},{"n":37,"text":"\troute := r.NewRoute().HeadersRegexp(\"Origin\", \"^https://example.co$\")","matches":[]}],"symbols":[]},…]}
    "#]].assert_eq(&shown);
}

#[test]
fn symbols() {
    let args = [MUX_TAG, "type:symbol", "/^Use/"];
    expect![[r#"
        exit 0
        --- stdout
        5 matches • 3 results

        github.com/gorilla/mux@a7962380ca08
        middleware.go
          24: method Use (mux.Router)
        --
          31: method useInterface (mux.Router)

        mux.go
          75: field useEncodedPath (mux.routeConf)
        --
         269: method UseEncodedPath (mux.Router)

        regexp.go
          19: field useEncodedPath (mux.routeRegexpOptions)
        [EOF]
        --- stderr
        [EOF]
    "#]]
    .assert_eq(&search("search/symbols.json", &args));
}

#[test]
fn symbols_json() {
    let args = [MUX_TAG, "type:symbol", "/^Use/", "--json"];
    let run = exchange(vec![Step::Capture("search/symbols.json")], &args);
    let results = run.stdout.find(r#""results":["#).unwrap();
    let second = run.stdout.find(r#",{"type":"file""#).unwrap();
    expect![[r#"
        "results":[{"type":"file","repo":"github.com/gorilla/mux","commit":"a7962380ca08b5a188038c69871b8d3fbdf31e89","path":"middleware.go","path_match":false,"binary":false,"lines":[],"symbols":[{"line":24,"kind":"method","name":"Use","container":"mux.Router"},{"line":31,"kind":"method","name":"useInterface","container":"mux.Router"}]}
    "#]].assert_eq(&format!("{}\n", &run.stdout[results..second]));
}

#[test]
fn paths() {
    let args = [MUX_TAG, "file:.", "type:path", "count:all"];
    expect![[r#"
        exit 0
        --- stdout
        22 matches • 22 results

        github.com/gorilla/mux@a7962380ca08
        example_authentication_middleware_test.go
        example_route_test.go
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
    .assert_eq(&search("search/paths.json", &args));
}

#[test]
fn directories() {
    let args = [MUX_TAG, "file:.", "select:file.directory", "count:all"];
    expect![[r#"
        exit 0
        --- stdout
        2 matches • 2 results

        github.com/gorilla/mux@a7962380ca08
        /
        .github/
        [EOF]
        --- stderr
        [EOF]
    "#]]
    .assert_eq(&search("search/directories.json", &args));
}

#[test]
fn directories_json() {
    let args =
        [MUX_TAG, "file:.", "select:file.directory", "count:all", "--json"];
    let run = exchange(vec![Step::Capture("search/directories.json")], &args);
    let results = run.stdout.find(r#""results":["#).unwrap();
    expect![[r#"
        "results":[{"type":"directory","repo":"github.com/gorilla/mux","commit":"a7962380ca08b5a188038c69871b8d3fbdf31e89","path":""},{"type":"directory","repo":"github.com/gorilla/mux","commit":"a7962380ca08b5a188038c69871b8d3fbdf31e89","path":".github"}]}
    "#]].assert_eq(&run.stdout[results..]);
}

#[test]
fn paths_starting_with_a_dash_print_with_a_leading_slash() {
    let directory =
        server::capture("search/directories.json")["response"]["body"]["data"]
            ["search"]["results"]["results"][1]
            .clone();
    let answer = edited("search/content.json", |results| {
        let hits = results["results"].as_array_mut().unwrap();
        hits.truncate(1);
        hits[0]["file"]["path"] = "--weird.go".into();
        hits[0]["chunkMatches"].as_array_mut().unwrap().truncate(1);
        hits.push(directory);
        hits[1]["file"]["path"] = "-d/".into();
    });
    let run = exchange(vec![ok(&answer)], &["NewRouter"]);
    expect![[r#"
        exit 0
        --- stdout
        91 matches • 2 results

        github.com/gorilla/mux@a7962380ca08
        /--weird.go
          13- func ExampleRoute_HeadersRegexp() {
          14: 	r := mux.NewRouter()
          15- 	route := r.NewRoute().HeadersRegexp("Accept", "html")

        /-d/
        [EOF]
        --- stderr
        [EOF]
    "#]]
    .assert_eq(&run.transcript());
}

#[test]
fn commits() {
    let args = [MUX, "type:commit", "Walk", "count:3"];
    expect![[r#"
        exit 0
        --- stdout
        3+ matches • 3 results
        notice: results may be missing: it hit a limit
        narrow: before:"2 months ago" (3) • author:kamil@kamilkisiel\.net (1) • author:nick@nickhudkins\.com (1) • author:nmiyake@palantir\.com (1)

        github.com/gorilla/mux
        dc835075986d • 2018-01-20T04:47:48Z • Kamil Kisiel • [docs] README.md: Improve "walking routes" example. (#337) (#323)
        043ee6597c29 • 2017-05-24T00:31:38Z • Nick Hudkins • Update Walking Routes Section
        456bcfa82d67 • 2017-05-20T00:30:49Z • Nick Miyake • Update README with example for Router.Walk
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&search("search/commits.json", &args));
}

#[test]
fn commits_json() {
    let args = [MUX, "type:commit", "Walk", "count:3", "--json"];
    let run = exchange(vec![Step::Capture("search/commits.json")], &args);
    let results = run.stdout.find(r#""results":["#).unwrap();
    let second = run.stdout.find(r#",{"type":"commit""#).unwrap();
    expect![[r#"
        "results":[{"type":"commit","repo":"github.com/gorilla/mux","commit":"dc835075986d1932ee5495f32e3631770715fd6c","date":"2018-01-20T04:47:48Z","author":"Kamil Kisiel","subject":"[docs] README.md: Improve \"walking routes\" example. (#337) (#323)","diff":null}
    "#]].assert_eq(&format!("{}\n", &run.stdout[results..second]));
}

#[test]
fn diffs() {
    let args = [MUX, "type:diff", "Walk", "count:3"];
    expect![[r#"
        exit 0
        --- stdout
        3+ matches • 2 results
        notice: results may be missing: it hit a limit
        narrow: before:"2 months ago" (2) • author:32021905\+fharding1@users\.noreply\.github\.com (1) • author:franklinharding0\.0@gmail\.com (1)

        github.com/gorilla/mux
        946b6237eb8d • 2019-11-14T20:19:09Z • Franklin Harding • Fix the CORSMethodMiddleware bug with subrouters
          middleware.go middleware.go
          @@ -60,3 +60,1 @@ func CORSMethodMiddleware(r *Router) MiddlewareFunc {
           $
          -	err := r.Walk(func(route *Route, _ *Router, _ []*Route) error {
          -		for _, m := range route.matchers {

        05347690167e • 2019-06-29T20:52:29Z • Franklin Harding • Improve CORS Method Middleware (#477)
          middleware.go middleware.go
          @@ -43,3 +42,0 @@ func (r *Router) useInterface(mw middleware) {
          -
          -			err := r.Walk(func(route *Route, _ *Router, _ []*Route) error {
          -				for _, m := range route.matchers {
          @@ -73,0 +60,3 @@ func CORSMethodMiddleware(r *Router) MiddlewareFunc {
          +
          +	err := r.Walk(func(route *Route, _ *Router, _ []*Route) error {
          +		for _, m := range route.matchers {
        [EOF]
        --- stderr
        [EOF]
    "#]]
    .assert_eq(&ends_shown(&search("search/diffs.json", &args)));
}

#[test]
fn repos() {
    let args = [r"repo:^github\.com/gorilla/", "type:repo", "mux"];
    expect![[r#"
        exit 0
        --- stdout
        1 match • 1 result

        github.com/gorilla/mux • 21839 stars
          Package gorilla/mux is a powerful HTTP router and URL matcher for building Go web servers with 🦍
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&search("search/repos.json", &args));
}

#[test]
fn repos_json() {
    let args = [r"repo:^github\.com/gorilla/", "type:repo", "mux", "--json"];
    expect![[r#"
        exit 0
        --- stdout
        {"version":1,"kind":"search","query":"repo:^github\\.com/gorilla/ type:repo mux","match_count":1,"approximate":null,"limit_hit":false,"skipped":{"timed_out":[],"cloning":[],"missing":[]},"alert":null,"results":[{"type":"repository","repo":"github.com/gorilla/mux","description":"Package gorilla/mux is a powerful HTTP router and URL matcher for building Go web servers with 🦍","stars":21839}]}
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&search("search/repos.json", &args));
}

#[test]
fn no_hits_exits_1() {
    let args = [MUX_TAG, "zzzzunlikely"];
    expect![[r#"
        exit 1
        --- stdout
        0 matches • 0 results
        [EOF]
        --- stderr
        [EOF]
    "#]]
    .assert_eq(&search("search/no-hits.json", &args));
}

#[test]
fn no_hits_suggests_widening() {
    let filters = json!([
        {"value": "archived:yes", "count": 12, "limitHit": false,
         "kind": "utility"},
        {"value": "fork:yes", "count": 3, "limitHit": false,
         "kind": "utility"},
        {"value": "lang:go", "count": 0, "limitHit": false, "kind": "lang"},
    ]);
    let answer = edited("search/no-hits.json", |results| {
        results["dynamicFilters"] = filters;
    });
    let runs = [&[MUX_TAG, "zzzzunlikely"][..], &[MUX_TAG, "x", "--json"]]
        .map(|args| exchange(vec![ok(&answer)], args).transcript());
    expect![[r#"
        exit 1
        --- stdout
        0 matches • 0 results
        widen: archived:yes (12) • fork:yes (3)
        [EOF]
        --- stderr
        [EOF]
        exit 1
        --- stdout
        {"version":1,"kind":"search","query":"repo:^github\\.com/gorilla/mux$@v1.7.0 x","match_count":0,"approximate":null,"limit_hit":false,"skipped":{"timed_out":[],"cloning":[],"missing":[]},"filters":[{"kind":"utility","value":"archived:yes","count":12,"limit_hit":false},{"kind":"utility","value":"fork:yes","count":3,"limit_hit":false}],"alert":null,"results":[]}
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&runs.concat());
}

#[test]
fn no_repos_shows_the_alert() {
    let args = [r"repo:^github\.com/nope/nope$", "x"];
    expect![[r#"
        exit 1
        --- stdout
        0 matches • 0 results
        alert: No repositories found: Try using a different `repo:<regexp>` filter to see results
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&search("search/no-repos.json", &args));
}

#[test]
fn limit_hit_suggests_narrowing() {
    let args = [
        r"repo:^github\.com/golang/go$@go1.23.0",
        "context.WithTimeout",
        "count:5",
    ];
    expect![[r#"
        exit 0
        --- stdout
        5+ matches • 5 results
        notice: results may be missing: it hit a limit
        narrow: lang:go (5) • -file:_test\.\w+$ (3) • -file:(^|/)vendor/ (1)

        github.com/golang/go@6885bad7dd86
        src/cmd/cgo/internal/testplugin/plugin_test.go
         242-
         243: 	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
         244- 	defer cancel()

        src/cmd/go/internal/test/test.go
        1549- 	// but add a last-ditch deadline to detect and stop wedged binaries.
        1550: 	ctx, cancel := context.WithTimeout(ctx, testKillTimeout)
        1551- 	defer cancel()

        src/cmd/go/script_test.go
          82- 		var cancel context.CancelFunc
          83: 		ctx, cancel = context.WithTimeout(ctx, timeout)
          84- 		t.Cleanup(cancel)

        src/cmd/vendor/golang.org/x/tools/go/analysis/passes/unusedresult/unusedresult.go
          64- 		"context.WithDeadline": true,
          65: 		"context.WithTimeout":  true,
          66- 		"context.WithValue":    true,

        src/context/afterfunc_test.go
          89- 	ctx0 := &afterFuncContext{}
          90: 	ctx1, cancel := context.WithTimeout(ctx0, veryLongDuration)
          91- 	defer cancel()
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&search("search/limit-hit.json", &args));
}

#[test]
fn filters_in_the_query_are_not_suggested_whatever_their_case() {
    let args = [
        r"repo:^github\.com/golang/go$@go1.23.0",
        "context.WithTimeout",
        "count:5",
        "lang:Go",
    ];
    let answer = edited("search/limit-hit.json", |_| {});
    let run = exchange(vec![ok(&answer)], &args);
    let narrow = run.stdout.lines().find(|line| line.starts_with("narrow:"));
    expect![[r#"
        Some(
            "narrow: -file:_test\\.\\w+$ (3) • -file:(^|/)vendor/ (1)",
        )
    "#]]
    .assert_debug_eq(&narrow);
}

#[test]
fn a_file_hits_lines_and_symbols_are_one_numbered_block() {
    let answer = edited("search/content.json", |results| {
        let hits = results["results"].as_array_mut().unwrap();
        hits.truncate(1);
        hits[0]["chunkMatches"] = json!([{
            "content": "a := 1\nb := 2",
            "contentStart": {"line": 2},
            "ranges": [{"start": {"line": 2, "character": 0},
                "end": {"line": 2, "character": 1}}],
        }]);
        hits[0]["symbols"] = json!([{
            "name": "c", "kind": "FUNCTION", "containerName": null,
            "location": {"range": {"start": {"line": 8, "character": 5}}},
        }]);
    });
    let run = exchange(vec![ok(&answer)], &["x"]);
    expect![[r#"
        exit 0
        --- stdout
        91 matches • 1 result

        github.com/gorilla/mux@a7962380ca08
        example_route_test.go
           3: a := 1
           4- b := 2
        --
           9: function c
        [EOF]
        --- stderr
        [EOF]
    "#]]
    .assert_eq(&run.transcript());
}

#[test]
fn partial_answers_say_what_is_missing() {
    let answer = edited("search/repos.json", |results| {
        results["limitHit"] = true.into();
        results["timedout"] = json!([{"name": "github.com/x/slow"}]);
        results["cloning"] = json!([{"name": "github.com/x/new"}, {"name": "github.com/x/a b"}]);
        results["missing"] = json!([{"name": "github.com/x/gone"}]);
        results["alert"] = json!({
            "title": "Some results are hidden",
            "description": "Try a\nnarrower query",
            "proposedQueries": [
                {"description": "narrower", "query": "type:repo mux lang:go"},
            ],
        });
    });
    let runs = [&["x"][..], &["x", "--json"]]
        .map(|args| exchange(vec![ok(&answer)], args).transcript());
    expect![[r#"
        exit 0
        --- stdout
        1 match • 1 result
        alert: Some results are hidden: Try a narrower query
        hint: sego search 'type:repo mux lang:go'
        notice: results may be missing: it hit a limit, 1 repository timed out, 2 repositories are still cloning, 1 repository is missing
        hint: sego search 'x timeout:40s'

        github.com/gorilla/mux • 21839 stars
          Package gorilla/mux is a powerful HTTP router and URL matcher for building Go web servers with 🦍
        [EOF]
        --- stderr
        [EOF]
        exit 0
        --- stdout
        {"version":1,"kind":"search","query":"x","match_count":1,"approximate":null,"limit_hit":true,"skipped":{"timed_out":["github.com/x/slow"],"cloning":["github.com/x/new","github.com/x/a b"],"missing":["github.com/x/gone"]},"alert":{"title":"Some results are hidden","description":"Try a\nnarrower query","hints":["sego search 'type:repo mux lang:go'"]},"partial":{"reason":"results may be missing: it hit a limit, 1 repository timed out, 2 repositories are still cloning, 1 repository is missing","hints":["sego search 'x timeout:40s'"]},"results":[{"type":"repository","repo":"github.com/gorilla/mux","description":"Package gorilla/mux is a powerful HTTP router and URL matcher for building Go web servers with 🦍","stars":21839}]}
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&runs.concat());
}

#[test]
fn binary_hits_show_no_lines() {
    let answer = edited("search/content.json", |results| {
        let hits = results["results"].as_array_mut().unwrap();
        hits.truncate(1);
        hits[0]["chunkMatches"][0]["content"] =
            "PK\u{3}\u{4}\0NewRouter".into();
        hits[0]["chunkMatches"].as_array_mut().unwrap().truncate(1);
    });
    let runs = [&["NewRouter"][..], &["NewRouter", "--json"]]
        .map(|args| exchange(vec![ok(&answer)], args).transcript());
    expect![[r#"
        exit 0
        --- stdout
        91 matches • 1 result

        github.com/gorilla/mux@a7962380ca08
        example_route_test.go
          binary file; lines not shown
        [EOF]
        --- stderr
        [EOF]
        exit 0
        --- stdout
        {"version":1,"kind":"search","query":"NewRouter","match_count":91,"approximate":null,"limit_hit":false,"skipped":{"timed_out":[],"cloning":[],"missing":[]},"alert":null,"results":[{"type":"file","repo":"github.com/gorilla/mux","commit":"a7962380ca08b5a188038c69871b8d3fbdf31e89","path":"example_route_test.go","path_match":false,"binary":true,"lines":[],"symbols":[]}]}
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&runs.concat());
}

#[test]
fn bad_query_is_invalid() {
    let args = [MUX_TAG, "/(/"];
    let proposed = edited("search/bad-query.json", |results| {
        results["alert"]["proposedQueries"] = json!([
            {"description": "literal", "query": format!("{MUX_TAG} \\(")},
            {"description": "quoted", "query": "content:\"(\""},
        ]);
    });
    let runs = [
        search("search/bad-query.json", &args),
        exchange(vec![ok(&proposed)], &args).transcript(),
    ];
    expect![[r#"
        exit 2
        --- stdout
        [EOF]
        --- stderr
        error: the upstream can't run this query
        caused by: Error parsing regexp: missing closing ): `(`
        [EOF]
        exit 2
        --- stdout
        [EOF]
        --- stderr
        error: the upstream can't run this query
        caused by: Error parsing regexp: missing closing ): `(`
        hint: sego search 'repo:^github\.com/gorilla/mux$@v1.7.0 \('
        hint: sego search 'content:"("'
        [EOF]
    "#]]
    .assert_eq(&runs.concat());
}

#[test]
fn missing_revision() {
    let args = [r"repo:^github\.com/gorilla/mux$@v9.9.9", "NewRouter"];
    expect![[r#"
        exit 1
        --- stdout
        [EOF]
        --- stderr
        revision v9.9.9 not found in github.com/gorilla/mux
        hint: sego list github.com/gorilla/mux
        [EOF]
    "#]]
    .assert_eq(&search("search/missing-revision.json", &args));
}

#[test]
fn timed_out_is_unanswered() {
    let args = [MUX_TAG, "NewRouter", "timeout:1ms"];
    expect![[r#"
        exit 3
        --- stdout
        [EOF]
        --- stderr
        error: the search was cut short with no hits: it ran for its whole timeout:1ms
        hint: sego search 'repo:^github\.com/gorilla/mux$@v1.7.0 NewRouter timeout:2ms'
        [EOF]
    "#]].assert_eq(&search("search/timed-out.json", &args));
}

#[test]
fn timed_out_carries_the_trace() {
    let args = [MUX_TAG, "NewRouter", "timeout:1ms", "--json"];
    expect![[r#"
        exit 3
        --- stdout
        {"version":1,"kind":"error","code":"incomplete","message":"the search was cut short with no hits: it ran for its whole timeout:1ms","hints":["sego search 'repo:^github\\.com/gorilla/mux$@v1.7.0 NewRouter timeout:2ms'"],"detail":{"trace":"test-trace"}}
        [EOF]
        --- stderr
        error: the search was cut short with no hits: it ran for its whole timeout:1ms
        hint: sego search 'repo:^github\.com/gorilla/mux$@v1.7.0 NewRouter timeout:2ms'
        [EOF]
    "#]].assert_eq(&search("search/timed-out.json", &args));
}

#[test]
fn negations_need_no_escaping() {
    let step = Step::Search {
        capture: "search/no-hits.json",
        query: "-file:_test NewRouter",
    };
    let run = exchange(vec![step], &["-file:_test", "NewRouter"]);
    expect![[r#"
        exit 1
        --- stdout
        0 matches • 0 results
        [EOF]
        --- stderr
        [EOF]
    "#]]
    .assert_eq(&run.transcript());
}

#[test]
fn a_missing_query_sends_nothing() {
    let server = server::start(Vec::new());
    let runs = [&[][..], &["", " "]].map(|args: &[&str]| {
        sego_at(&server).arg("search").args(args).run().transcript()
    });
    server.finish();
    expect![[r#"
        exit 2
        --- stdout
        [EOF]
        --- stderr
        error: missing query; expected <query>...
        usage: sego search [--json] <query>...
        [EOF]
        exit 2
        --- stdout
        [EOF]
        --- stderr
        error: missing query; expected <query>...
        usage: sego search [--json] <query>...
        [EOF]
    "#]]
    .assert_eq(&runs.concat());
}

#[test]
fn help() {
    check!(
        sego().args(["search", "--help"]),
        expect![[r#"
            exit 0
            --- stdout
            sego search prints the code, paths, symbols, commits and repositories that
            match a Sourcegraph query.

            Usage:
              sego search [--json] <query>...

            Operands:
              <query>   a Sourcegraph search query. The operands are joined with spaces
                        and sent as they are. A token that starts with a single dash,
                        such as -file:_test, is part of the query.

            Output:
              A summary line: the number of matches, with + when the upstream counted
              only some, and the number of results. Then, when there is one, a line
              each:

                alert:      the upstream's message about the query, then its proposed
                            queries as hint: lines
                notice:     that results may be missing, and why: the search hit a
                            limit, such as its count:, repositories timed out, are
                            still cloning or are missing, or it ran for its whole
                            timeout; then the search with a longer timeout: as a hint:
                            line, when that could help
                narrow:     when a limit was hit, filters to add and their match counts
                widen:      when nothing matched, filters that bring in repositories the
                            search left out, such as archived:yes

              Then the results, in the upstream's order, under a line naming their
              repository and commit, repeated whenever it changes:

                github.com/gorilla/mux@a7962380ca08
                mux.go
                  22-
                  23: // NewRouter returns a new router instance.
                  24: func NewRouter() *Router {

              A file's lines are N: for a line that matches, N- for context, and --
              between runs that aren't adjacent; its symbols are N: kind name
              (container); a binary file shows none. A path match is the path alone,
              and a directory ends in /. A commit is its commit, date, author and
              subject, then any diff below it. A repository is its name and stars,
              then its description. A line wider than 500 columns is cut, and says
              so; --json has it whole.

              With --json, one document of kind "search": query, match_count,
              approximate (null when exact), limit_hit, skipped {timed_out, cloning,
              missing}, filters [{kind, value, count, limit_hit}] when shown, alert
              {title, description, hints} or null, partial {reason, hints} when results
              may be missing, and results. Each result has a type: file {repo, commit,
              path, path_match, binary, lines [{n, text, matches [{start, end}]}],
              symbols [{line, kind, name, container}]}, directory {repo, commit, path},
              commit {repo, commit, date, author, subject, diff} or repository {repo,
              description, stars}. A filter or a symbol whose kind is other has detail
              {kind}, with the upstream's kind. Commits have 40 hex digits; match
              offsets count codepoints. Anything else is reported on stderr, and its
              document, of kind "unavailable" or "error", also goes to stdout.

            Matching:
              Matching is case-insensitive unless the query says case:yes. A misspelt
              filter or an unbalanced quote becomes search text and matches nothing,
              silently. A search runs for its timeout:, up to 60s; without one, for
              20s, or 60s with count:. A search with no results under a short timeout:
              may have been cut short; when it ran for its whole timeout, it is
              unanswered.

            Exit codes:
              0  answered      the results
              1  answered no   no results from a complete search; no repository
                               matches; a revision that doesn't exist
              2  invalid       no query; a query the upstream can't run; an invalid
                               SEGO_ENDPOINT
              3  unanswered    a search cut short with no results, or no usable answer
                               from the upstream; retry, or narrow and retry

            Examples:
              sego search NewRouter
              sego search 'func NewRouter'
              sego search 'repo:^github\.com/gorilla/mux$' NewRouter
              sego search -file:_test NewRouter
            [EOF]
            --- stderr
            [EOF]
        "#]]
    );
}

#[test]
fn help_fits_79_columns() {
    let run = sego().args(["search", "--help"]).run();
    assert_eq!(run.code, 0);
    for line in run.stdout.lines() {
        assert!(line.chars().count() <= 79, "too wide: {line}");
    }
}

#[test]
fn help_states_the_known_limits() {
    let run = sego().args(["search", "--help"]).run();
    let help = run.stdout.split_whitespace().collect::<Vec<_>>().join(" ");
    for fact in [
        "case-insensitive unless the query says case:yes",
        "A misspelt filter or an unbalanced quote",
        "matches nothing, silently",
        "under a short timeout: may have been cut short",
    ] {
        assert!(help.contains(fact), "the help doesn't say: {fact}");
    }
    assert!(!help.contains("type:path"));
    assert!(!help.contains("select:"));
}
