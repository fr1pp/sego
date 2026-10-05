use std::fmt::Write as _;

use expect_test::expect;

use crate::command::Run;
use crate::server::{self, Step};
use crate::{sego, sego_at};

const RELEASE: &str = "github.com/gorilla/mux@v1.7.0...v1.8.0";
const MUX: &str = "github.com/gorilla/mux";
const GO: &str = "github.com/golang/go";

/// Runs sego compare with `args` against a server taking `steps`, and
/// checks that the steps were taken as scripted.
fn exchange(steps: Vec<Step>, args: &[&str]) -> Run {
    let server = server::start(steps);
    let run = sego_at(&server).arg("compare").args(args).run();
    server.finish();
    run
}

/// `text` with each line of spaces alone, such as a hunk's empty context
/// line, written as `␠`s: a snapshot reads such a line as empty.
fn visible(text: &str) -> String {
    text.split_inclusive('\n')
        .map(|line| match line.trim_end_matches('\n') {
            spaces if !spaces.is_empty() && spaces.trim().is_empty() => {
                spaces.replace(' ', "␠") + "\n"
            }
            _ => line.to_owned(),
        })
        .collect()
}

/// The transcript of `sego compare <args>` answered by `capture`.
fn compare(capture: &'static str, args: &[&str]) -> String {
    visible(&exchange(vec![Step::Capture(capture)], args).transcript())
}

/// The answer to `sego compare <args>` from `capture`, which must succeed
/// with nothing on stderr.
fn answer(capture: &'static str, args: &[&str]) -> String {
    let run = exchange(vec![Step::Capture(capture)], args);
    assert_eq!((run.code, run.stderr.as_str()), (0, ""), "{}", run.stdout);
    run.stdout
}

/// The JSON document `sego compare --json <args>` answers with.
fn document(capture: &'static str, args: &[&str]) -> serde_json::Value {
    let args = [args, &["--json"]].concat();
    serde_json::from_str(&answer(capture, &args)).unwrap()
}

#[test]
fn range() {
    expect![[r#"
        exit 0
        --- stdout
        github.com/gorilla/mux@a7962380ca08...98cb6bf42e08 • 30 commits • 20 files changed, +1033 -378

        github.com/gorilla/mux@98cb6bf42e08 • 2020-07-11 • Harshavardhana • fix: regression in vars extract for wildcard host (#579)
        github.com/gorilla/mux@948bec34b516 • 2020-05-17 • Eric Skoglund • docs: Remove stale text from comment. (#568)
        github.com/gorilla/mux@75dcda0896e1 • 2020-01-12 • Klaus Post • perf: reduce allocations in (*routeRegexp).getURLQuery (#544)
        github.com/gorilla/mux@49c01487a141 • 2019-11-21 • Veselkov Konstantin • lint: Remove golint warning (#526)
        github.com/gorilla/mux@4de8a5a4d283 • 2019-11-19 • icattlecoder • fix headers regexp test (#536)
        github.com/gorilla/mux@2854a05fd3e2 • 2019-11-17 • Matt Silverlock • Merge pull request #535 from fharding1/cors-subrouter-bug
        github.com/gorilla/mux@946b6237eb8d • 2019-11-14 • Franklin Harding • Fix the CORSMethodMiddleware bug with subrouters
        github.com/gorilla/mux@f395758b854c • 2019-10-24 • Franklin Harding • Remove/cleanup request context helpers (#525)
        github.com/gorilla/mux@ff4e71f14416 • 2019-10-18 • Euan Kemp • Guess the scheme if r.URL.Scheme is unset (#474)
        github.com/gorilla/mux@884b5ffcbd3a • 2019-08-30 • Vivek V • Added capacity to slice creation, when capacity is known (#516)
        github.com/gorilla/mux@e1863a64f3c6 • 2019-08-27 • Vivek V • Modified http status codes, using constants provided by the http package (#514)
        github.com/gorilla/mux@9536e4053d76 • 2019-08-26 • Jonas De Beukelaer • bugfix: subrouter custom methodNotAllowed handler returning 404 (#509) (#510)
        github.com/gorilla/mux@e0cdff45b7de • 2019-08-24 • Matt Silverlock • Update README (self-host logo) (#513)
        github.com/gorilla/mux@e67b3c02c719 • 2019-07-20 • Matt Silverlock • Remove TravisCI badge (#503)
        github.com/gorilla/mux@7a1bf406d6f5 • 2019-07-20 • Franklin Harding • [docs] Add documentation for using mux to serve a SPA (#493)
        github.com/gorilla/mux@eab9c4f3d22d • 2019-07-20 • Christian Muehlhaeuser • Simplify code (#501)
        github.com/gorilla/mux@50fbc3e7fbfc • 2019-07-20 • Christian Muehlhaeuser • Avoid unnecessary conversion (#502)
        github.com/gorilla/mux@d83b6ffe499a • 2019-07-01 • Matt Silverlock • Update config.yml (#495)
        github.com/gorilla/mux@00bdffe0f3c7 • 2019-06-30 • Matt Silverlock • Update stale.yml (#494)
        github.com/gorilla/mux@05347690167e • 2019-06-29 • Franklin Harding • Improve CORS Method Middleware (#477)
        github.com/gorilla/mux@d70f7b4baacb • 2019-06-29 • Matt Silverlock • Delete ISSUE_TEMPLATE.md (#492)
        github.com/gorilla/mux@48f941fa9994 • 2019-06-29 • Franklin Harding • Use subtests for middleware tests (#478)
        github.com/gorilla/mux@64954673e972 • 2019-06-28 • Matt Silverlock • Delete .travis.yml (#490)
        github.com/gorilla/mux@4248f5cd8717 • 2019-06-28 • Franklin Harding • Fix nil panic in authentication middleware example (#489)
        github.com/gorilla/mux@212aa90d7cec • 2019-06-24 • Matt Silverlock • [WIP] Create CircleCI config (#484)
        github.com/gorilla/mux@ed099d423848 • 2019-05-17 • M@ • host:port matching does not require a :port to be specified.
        github.com/gorilla/mux@c5c6c98bc253 • 2019-03-16 • sekky0905 • [build] Remove sudo setting from travis.yml (#462)
        github.com/gorilla/mux@15a353a63672 • 2019-02-28 • seriousben • adding Router.Name to create new Route (#457)
        github.com/gorilla/mux@8eaa9f130911 • 2019-02-28 • seriousben • fix go1.12 go vet usage (#458)
        github.com/gorilla/mux@8559a4f775fc • 2019-02-17 • Souvik Haldar • [docs] typo (#454)

        github.com/gorilla/mux@98cb6bf42e08
        .circleci/config.yml • added • +87
        .github/stale.yml • modified • +5 -5
        .travis.yml • deleted at a7962380ca08 • -24
        ISSUE_TEMPLATE.md • deleted at a7962380ca08 • -11
        README.md • modified • +158 -2
        context.go • deleted at a7962380ca08 • -18
        context_test.go • deleted at a7962380ca08 • -30
        doc.go • modified • +1 -1
        example_cors_method_middleware_test.go • added • +37
        go.mod • modified • +2 -0
        middleware.go • modified • +29 -27
        middleware_test.go • modified • +317 -189
        mux.go • modified • +21 -16
        mux_httpserver_test.go • added • +49
        mux_test.go • modified • +94 -13
        old_test.go • modified • +27 -13
        regexp.go • modified • +81 -21
        regexp_test.go • added • +91
        route.go • modified • +33 -7
        test_helpers.go • modified • +1 -1
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&compare("compare/range.json", &[RELEASE]));
}

#[test]
fn range_json() {
    expect![[r#"
        exit 0
        --- stdout
        {"version":1,"kind":"comparison","repo":"github.com/gorilla/mux","base":"a7962380ca08b5a188038c69871b8d3fbdf31e89","head":"98cb6bf42e086f6af920b965c38cacc07402d51b","path":null,"commits":[{"commit":"98cb6bf42e086f6af920b965c38cacc07402d51b","date":"2020-07-11T20:05:21Z","author":"Harshavardhana","subject":"fix: regression in vars extract for wildcard host (#579)"},{"commit":"948bec34b5168796bf3fc4dbb09215baa970351a","date":"2020-05-17T04:02:54Z","author":"Eric Skoglund","subject":"docs: Remove stale text from comment. (#568)"},{"commit":"75dcda0896e109a2a22c9315bca3bb21b87b2ba5","date":"2020-01-12T19:17:43Z","author":"Klaus Post","subject":"perf: reduce allocations in (*routeRegexp).getURLQuery (#544)"},{"commit":"49c01487a141b49f8ffe06277f3dca3ee80a55fa","date":"2019-11-21T17:05:00Z","author":"Veselkov Konstantin","subject":"lint: Remove golint warning (#526)"},{"commit":"4de8a5a4d283677c69afa1a86a044c8451633a18","date":"2019-11-19T13:02:14Z","author":"icattlecoder","subject":"fix headers regexp test (#536)"},{"commit":"2854a05fd3e2a9ecec0802bcb6eda09bbc0eea9f","date":"2019-11-17T01:33:21Z","author":"Matt Silverlock","subject":"Merge pull request #535 from fharding1/cors-subrouter-bug"},{"commit":"946b6237eb8d0ce3225f502b7fd4208d0b60ce5f","date":"2019-11-14T20:19:09Z","author":"Franklin Harding","subject":"Fix the CORSMethodMiddleware bug with subrouters"},{"commit":"f395758b854c4efa789b8c1e9b73479704c548cb","date":"2019-10-24T12:12:56Z","author":"Franklin Harding","subject":"Remove/cleanup request context helpers (#525)"},{"commit":"ff4e71f144166b1dfe3017a146f8ed32a82e688b","date":"2019-10-18T00:48:19Z","author":"Euan Kemp","subject":"Guess the scheme if r.URL.Scheme is unset (#474)"},{"commit":"884b5ffcbd3a11b730f0b75f5c86ac408753c34d","date":"2019-08-30T12:11:56Z","author":"Vivek V","subject":"Added capacity to slice creation, when capacity is known (#516)"},{"commit":"e1863a64f3c620ef604fb7905a0351e69f6a51e4","date":"2019-08-27T12:58:13Z","author":"Vivek V","subject":"Modified http status codes, using constants provided by the http package (#514)"},{"commit":"9536e4053d763b54d935f1ce731a315cfb42b979","date":"2019-08-26T00:11:59Z","author":"Jonas De Beukelaer","subject":"bugfix: subrouter custom methodNotAllowed handler returning 404 (#509) (#510)"},{"commit":"e0cdff45b7de56d3ad24ef9871c14fe4238475aa","date":"2019-08-24T00:44:47Z","author":"Matt Silverlock","subject":"Update README (self-host logo) (#513)"},{"commit":"e67b3c02c7195c052acff13261f0c9fd1ba53011","date":"2019-07-20T20:14:35Z","author":"Matt Silverlock","subject":"Remove TravisCI badge (#503)"},{"commit":"7a1bf406d6f5f8cfef4802eb0d949606c6ee7aea","date":"2019-07-20T14:53:35Z","author":"Franklin Harding","subject":"[docs] Add documentation for using mux to serve a SPA (#493)"},{"commit":"eab9c4f3d22d907d728aa0f5918934357866249e","date":"2019-07-20T14:49:38Z","author":"Christian Muehlhaeuser","subject":"Simplify code (#501)"},{"commit":"50fbc3e7fbfcdb4fb850686588071e5f0bdd4a0a","date":"2019-07-20T14:48:32Z","author":"Christian Muehlhaeuser","subject":"Avoid unnecessary conversion (#502)"},{"commit":"d83b6ffe499a29cc05fc977988d0392851779620","date":"2019-07-01T20:26:33Z","author":"Matt Silverlock","subject":"Update config.yml (#495)"},{"commit":"00bdffe0f3c77e27d2cf6f5c70232a2d3e4d9c15","date":"2019-06-30T04:17:52Z","author":"Matt Silverlock","subject":"Update stale.yml (#494)"},{"commit":"05347690167e152b85ece29fdfa9cc12a7b8c385","date":"2019-06-29T20:52:29Z","author":"Franklin Harding","subject":"Improve CORS Method Middleware (#477)"},{"commit":"d70f7b4baacbd8115844925606cf68ee63d438cc","date":"2019-06-29T18:01:59Z","author":"Matt Silverlock","subject":"Delete ISSUE_TEMPLATE.md (#492)"},{"commit":"48f941fa99947e50375f2b0dd4b3b060d15b2fe8","date":"2019-06-29T17:24:12Z","author":"Franklin Harding","subject":"Use subtests for middleware tests (#478)"},{"commit":"64954673e972292a409b5c8d13b16ed797b905d0","date":"2019-06-28T23:07:30Z","author":"Matt Silverlock","subject":"Delete .travis.yml (#490)"},{"commit":"4248f5cd8717eaea35eded08100714b2b2bac756","date":"2019-06-28T15:33:07Z","author":"Franklin Harding","subject":"Fix nil panic in authentication middleware example (#489)"},{"commit":"212aa90d7cec051ab29930d5c56f758f6f69a789","date":"2019-06-24T16:05:39Z","author":"Matt Silverlock","subject":"[WIP] Create CircleCI config (#484)"},{"commit":"ed099d42384823742bba0bf9a72b53b55c9e2e38","date":"2019-05-17T00:20:44Z","author":"M@","subject":"host:port matching does not require a :port to be specified."},{"commit":"c5c6c98bc25355028a63748a498942a6398ccd22","date":"2019-03-16T13:32:43Z","author":"sekky0905","subject":"[build] Remove sudo setting from travis.yml (#462)"},{"commit":"15a353a636720571d19e37b34a14499c3afa9991","date":"2019-02-28T18:12:03Z","author":"seriousben","subject":"adding Router.Name to create new Route (#457)"},{"commit":"8eaa9f13091105874ef3e20c65922e382cef3c64","date":"2019-02-28T17:36:07Z","author":"seriousben","subject":"fix go1.12 go vet usage (#458)"},{"commit":"8559a4f775fc329165fe32bd4c2543de8ada8fce","date":"2019-02-17T15:38:49Z","author":"Souvik Haldar","subject":"[docs] typo (#454)"}],"commit_count":30,"commits_truncated":false,"files_changed":20,"added":1033,"deleted":378,"files":[{"path":".circleci/config.yml","old_path":null,"status":"added","binary":false,"added":87,"deleted":0,"readable_at":"98cb6bf42e086f6af920b965c38cacc07402d51b"},{"path":".github/stale.yml","old_path":null,"status":"modified","binary":false,"added":5,"deleted":5,"readable_at":"98cb6bf42e086f6af920b965c38cacc07402d51b"},{"path":".travis.yml","old_path":null,"status":"deleted","binary":false,"added":0,"deleted":24,"readable_at":"a7962380ca08b5a188038c69871b8d3fbdf31e89"},{"path":"ISSUE_TEMPLATE.md","old_path":null,"status":"deleted","binary":false,"added":0,"deleted":11,"readable_at":"a7962380ca08b5a188038c69871b8d3fbdf31e89"},{"path":"README.md","old_path":null,"status":"modified","binary":false,"added":158,"deleted":2,"readable_at":"98cb6bf42e086f6af920b965c38cacc07402d51b"},{"path":"context.go","old_path":null,"status":"deleted","binary":false,"added":0,"deleted":18,"readable_at":"a7962380ca08b5a188038c69871b8d3fbdf31e89"},{"path":"context_test.go","old_path":null,"status":"deleted","binary":false,"added":0,"deleted":30,"readable_at":"a7962380ca08b5a188038c69871b8d3fbdf31e89"},{"path":"doc.go","old_path":null,"status":"modified","binary":false,"added":1,"deleted":1,"readable_at":"98cb6bf42e086f6af920b965c38cacc07402d51b"},{"path":"example_cors_method_middleware_test.go","old_path":null,"status":"added","binary":false,"added":37,"deleted":0,"readable_at":"98cb6bf42e086f6af920b965c38cacc07402d51b"},{"path":"go.mod","old_path":null,"status":"modified","binary":false,"added":2,"deleted":0,"readable_at":"98cb6bf42e086f6af920b965c38cacc07402d51b"},{"path":"middleware.go","old_path":null,"status":"modified","binary":false,"added":29,"deleted":27,"readable_at":"98cb6bf42e086f6af920b965c38cacc07402d51b"},{"path":"middleware_test.go","old_path":null,"status":"modified","binary":false,"added":317,"deleted":189,"readable_at":"98cb6bf42e086f6af920b965c38cacc07402d51b"},{"path":"mux.go","old_path":null,"status":"modified","binary":false,"added":21,"deleted":16,"readable_at":"98cb6bf42e086f6af920b965c38cacc07402d51b"},{"path":"mux_httpserver_test.go","old_path":null,"status":"added","binary":false,"added":49,"deleted":0,"readable_at":"98cb6bf42e086f6af920b965c38cacc07402d51b"},{"path":"mux_test.go","old_path":null,"status":"modified","binary":false,"added":94,"deleted":13,"readable_at":"98cb6bf42e086f6af920b965c38cacc07402d51b"},{"path":"old_test.go","old_path":null,"status":"modified","binary":false,"added":27,"deleted":13,"readable_at":"98cb6bf42e086f6af920b965c38cacc07402d51b"},{"path":"regexp.go","old_path":null,"status":"modified","binary":false,"added":81,"deleted":21,"readable_at":"98cb6bf42e086f6af920b965c38cacc07402d51b"},{"path":"regexp_test.go","old_path":null,"status":"added","binary":false,"added":91,"deleted":0,"readable_at":"98cb6bf42e086f6af920b965c38cacc07402d51b"},{"path":"route.go","old_path":null,"status":"modified","binary":false,"added":33,"deleted":7,"readable_at":"98cb6bf42e086f6af920b965c38cacc07402d51b"},{"path":"test_helpers.go","old_path":null,"status":"modified","binary":false,"added":1,"deleted":1,"readable_at":"98cb6bf42e086f6af920b965c38cacc07402d51b"}],"hints":[]}
        [EOF]
        --- stderr
        [EOF]
    "#]]
        .assert_eq(&compare("compare/range.json", &[RELEASE, "--json"]));
}

#[test]
fn directory() {
    expect![[r#"
        exit 0
        --- stdout
        github.com/gorilla/mux@a7962380ca08...98cb6bf42e08 .github • 1 commit • 1 file changed, +5 -5

        github.com/gorilla/mux@00bdffe0f3c7 • 2019-06-30 • Matt Silverlock • Update stale.yml (#494)

        github.com/gorilla/mux@98cb6bf42e08
        .github/stale.yml • modified • +5 -5
        [EOF]
        --- stderr
        [EOF]
    "#]]
        .assert_eq(&compare("compare/directory.json", &[RELEASE, ".github"]));
}

#[test]
fn file() {
    expect![[r#"
        exit 0
        --- stdout
        github.com/gorilla/mux@a7962380ca08...98cb6bf42e08 mux.go • 4 commits • 1 file changed, +21 -16

        github.com/gorilla/mux@948bec34b516 • 2020-05-17 • Eric Skoglund • docs: Remove stale text from comment. (#568)
        github.com/gorilla/mux@f395758b854c • 2019-10-24 • Franklin Harding • Remove/cleanup request context helpers (#525)
        github.com/gorilla/mux@eab9c4f3d22d • 2019-07-20 • Christian Muehlhaeuser • Simplify code (#501)
        github.com/gorilla/mux@15a353a63672 • 2019-02-28 • seriousben • adding Router.Name to create new Route (#457)

        github.com/gorilla/mux@98cb6bf42e08
        mux.go • modified • +21 -16
        @@ -5,6 +5,7 @@
         package mux
        ␠
         import (
        +	"context"
         	"errors"
         	"fmt"
         	"net/http"
        @@ -58,8 +59,7 @@ type Router struct {
        ␠
         	// If true, do not clear the request context after handling the request.
         	//
        -	// Deprecated: No effect when go1.7+ is used, since the context is stored
        -	// on the request itself.
        +	// Deprecated: No effect, since the context is stored on the request itself.
         	KeepContext bool
        ␠
         	// Slice of middlewares to be called after a match is found
        @@ -111,10 +111,8 @@ func copyRouteConf(r routeConf) routeConf {
         		c.regexp.queries = append(c.regexp.queries, copyRouteRegexp(q))
         	}
        ␠
        -	c.matchers = make([]matcher, 0, len(r.matchers))
        -	for _, m := range r.matchers {
        -		c.matchers = append(c.matchers, m)
        -	}
        +	c.matchers = make([]matcher, len(r.matchers))
        +	copy(c.matchers, r.matchers)
        ␠
         	return c
         }
        @@ -197,8 +195,8 @@ func (r *Router) ServeHTTP(w http.ResponseWriter, req *http.Request) {
         	var handler http.Handler
         	if r.Match(req, &match) {
         		handler = match.Handler
        -		req = setVars(req, match.Vars)
        -		req = setCurrentRoute(req, match.Route)
        +		req = requestWithVars(req, match.Vars)
        +		req = requestWithRoute(req, match.Route)
         	}
        ␠
         	if handler == nil && match.MatchErr == ErrMethodMismatch {
        @@ -283,6 +281,12 @@ func (r *Router) NewRoute() *Route {
         	return route
         }
        ␠
        +// Name registers a new route with a name.
        +// See Route.Name().
        +func (r *Router) Name(name string) *Route {
        +	return r.NewRoute().Name(name)
        +}
        +
         // Handle registers a new route with a matcher for the URL path.
         // See Route.Path() and Route.Handler().
         func (r *Router) Handle(path string, handler http.Handler) *Route {
        @@ -422,30 +426,31 @@ const (
        ␠
         // Vars returns the route variables for the current request, if any.
         func Vars(r *http.Request) map[string]string {
        -	if rv := contextGet(r, varsKey); rv != nil {
        +	if rv := r.Context().Value(varsKey); rv != nil {
         		return rv.(map[string]string)
         	}
         	return nil
         }
        ␠
         // CurrentRoute returns the matched route for the current request, if any.
         // This only works when called inside the handler of the matched route
         // because the matched route is stored in the request context which is cleared
        -// after the handler returns, unless the KeepContext option is set on the
        -// Router.
        +// after the handler returns.
         func CurrentRoute(r *http.Request) *Route {
        -	if rv := contextGet(r, routeKey); rv != nil {
        +	if rv := r.Context().Value(routeKey); rv != nil {
         		return rv.(*Route)
         	}
         	return nil
         }
        ␠
        -func setVars(r *http.Request, val interface{}) *http.Request {
        -	return contextSet(r, varsKey, val)
        +func requestWithVars(r *http.Request, vars map[string]string) *http.Request {
        +	ctx := context.WithValue(r.Context(), varsKey, vars)
        +	return r.WithContext(ctx)
         }
        ␠
        -func setCurrentRoute(r *http.Request, val interface{}) *http.Request {
        -	return contextSet(r, routeKey, val)
        +func requestWithRoute(r *http.Request, route *Route) *http.Request {
        +	ctx := context.WithValue(r.Context(), routeKey, route)
        +	return r.WithContext(ctx)
         }
        ␠
         // ----------------------------------------------------------------------------
        [EOF]
        --- stderr
        [EOF]
    "#]]
        .assert_eq(&compare("compare/file.json", &[RELEASE, "mux.go"]));
}

#[test]
fn file_json() {
    let document = document("compare/file.json", &[RELEASE, "mux.go"]);
    let file = &document["files"][0];
    expect![[r#"
        "mux.go" "mux.go" "modified"
        {"lines":[" package mux"," "," import (","+\t\"context\""," \t\"errors\""," \t\"fmt\""," \t\"net/http\""],"new_lines":7,"new_start":5,"no_newline_new":false,"no_newline_old":false,"old_lines":6,"old_start":5,"section":null}
    "#]].assert_eq(&format!(
        "{} {} {}\n{}\n",
        document["path"], file["path"], file["status"], file["hunks"][0]
    ));
}

#[test]
fn file_deleted() {
    expect![[r#"
        exit 0
        --- stdout
        github.com/gorilla/mux@a7962380ca08...98cb6bf42e08 context.go • 1 commit • 1 file changed, +0 -18

        github.com/gorilla/mux@f395758b854c • 2019-10-24 • Franklin Harding • Remove/cleanup request context helpers (#525)

        github.com/gorilla/mux@98cb6bf42e08
        context.go • deleted at a7962380ca08 • -18
        @@ -1,18 +0,0 @@
        -package mux
        -
        -import (
        -	"context"
        -	"net/http"
        -)
        -
        -func contextGet(r *http.Request, key interface{}) interface{} {
        -	return r.Context().Value(key)
        -}
        -
        -func contextSet(r *http.Request, key, val interface{}) *http.Request {
        -	if val == nil {
        -		return r
        -	}
        -
        -	return r.WithContext(context.WithValue(r.Context(), key, val))
        -}
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&compare(
        "compare/file-deleted.json",
        &[RELEASE, "context.go"],
    ));
}

#[test]
fn renamed() {
    let range = format!("{MUX}@v1.6.0...v1.7.0");
    let stdout = answer("compare/renamed.json", &[&range]);
    let blocks: Vec<&str> = stdout.split("\n\n").collect();
    let [first, commits, files] = blocks[..] else {
        panic!("{stdout}");
    };
    assert_eq!(commits.lines().count(), 50);
    expect![[r#"
        github.com/gorilla/mux@7f0880185913...a7962380ca08 • 50 commits • 23 files changed, +2173 -489

        github.com/gorilla/mux@a7962380ca08
        .github/release-drafter.yml • added • +8
        .github/stale.yml • added • +12
        .travis.yml • modified • +8 -6
        AUTHORS • added • +8
        ISSUE_TEMPLATE.md • added • +11
        LICENSE • modified • +1 -1
        README.md • modified • +360 -126
        context.go • renamed from context_native.go • +0 -6
        context_gorilla.go • deleted at 7f0880185913 • -26
        context_gorilla_test.go • deleted at 7f0880185913 • -40
        context_test.go • renamed from context_native_test.go • +0 -2
        doc.go • modified • +64 -0
        example_authentication_middleware_test.go • added • +46
        example_route_test.go • added • +51
        go.mod • added • +1
        middleware.go • added • +72
        middleware_test.go • added • +437
        mux.go • modified • +87 -61
        mux_test.go • modified • +877 -67
        old_test.go • modified • +1 -1
        regexp.go • modified • +45 -43
        route.go • modified • +65 -110
        test_helpers.go • added • +19
    "#]].assert_eq(&format!("{first}\n\n{files}"));
}

#[test]
fn divergent() {
    let range = format!(
        "{MUX}@f395758b854c4efa789b8c1e9b73479704c548cb...\
         946b6237eb8d0ce3225f502b7fd4208d0b60ce5f"
    );
    expect![[r#"
        exit 0
        --- stdout
        github.com/gorilla/mux@f395758b854c...946b6237eb8d • 1 commit • 2 files changed, +30 -15

        github.com/gorilla/mux@946b6237eb8d • 2019-11-14 • Franklin Harding • Fix the CORSMethodMiddleware bug with subrouters

        github.com/gorilla/mux@946b6237eb8d
        middleware.go • modified • +10 -15
        middleware_test.go • modified • +20 -0
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&compare("compare/divergent.json", &[&range]));
}

#[test]
fn reversed() {
    let range = format!("{MUX}@v1.8.0...v1.7.0");
    expect![[r#"
        exit 0
        --- stdout
        github.com/gorilla/mux@98cb6bf42e08...a7962380ca08 • no commits
        hint: sego compare github.com/gorilla/mux@v1.7.0...v1.8.0
        [EOF]
        --- stderr
        [EOF]
    "#]]
    .assert_eq(&compare("compare/reversed.json", &[&range]));
    let json = compare("compare/reversed.json", &[&range, "--json"]);
    expect![[r#"
        exit 0
        --- stdout
        {"version":1,"kind":"comparison","repo":"github.com/gorilla/mux","base":"98cb6bf42e086f6af920b965c38cacc07402d51b","head":"a7962380ca08b5a188038c69871b8d3fbdf31e89","path":null,"commits":[],"commit_count":0,"commits_truncated":false,"files_changed":0,"added":0,"deleted":0,"files":[],"hints":["sego compare github.com/gorilla/mux@v1.7.0...v1.8.0"]}
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&json);
}

#[test]
fn equal() {
    let range = format!("{MUX}@v1.8.0...v1.8.0");
    expect![[r#"
        exit 0
        --- stdout
        github.com/gorilla/mux@98cb6bf42e08...98cb6bf42e08 • no commits
        [EOF]
        --- stderr
        [EOF]
    "#]]
    .assert_eq(&compare("compare/equal.json", &[&range]));
}

#[test]
fn capped() {
    let range = format!("{GO}@go1.22.0...go1.23.0");
    let args = [range.as_str(), "src/runtime"];
    let stdout = answer("compare/capped.json", &args);
    assert!(stdout.lines().any(|line| line.contains(" • deleted • -")));
    let mut shown = String::new();
    for block in stdout.split("\n\n") {
        let lines: Vec<&str> = block.lines().collect();
        let (first, rest) = lines.split_first().unwrap();
        let notices = rest.iter().filter(|line| line.starts_with("notice:"));
        let _ = writeln!(shown, "{first} (+{} lines)", rest.len());
        for notice in notices {
            let _ = writeln!(shown, "{notice}");
        }
    }
    expect![[r#"
        github.com/golang/go@a10e42f219ab...6885bad7dd86 src/runtime • more than 50 commits • 320 files changed, +11534 -21469 (+0 lines)
        github.com/golang/go@3959d54c0bd5 • 2024-07-19 • Cuong Manh Le • runtime: mark lockWithRankMayAcquire as nosplit (+50 lines)
        notice: more than 50 commits; the newest 50 shown
        github.com/golang/go@6885bad7dd86 (+201 lines)
        notice: 320 files changed; 200 shown
    "#]].assert_eq(&shown);
    let document = document("compare/capped.json", &args);
    expect![[r#"
        null true 50 320 200
        {"hints":[],"reason":"more than 50 commits; the newest 50 shown. 320 files changed; 200 shown"}
    "#]].assert_eq(&format!(
        "{} {} {} {} {}\n{}\n",
        document["commit_count"],
        document["commits_truncated"],
        document["commits"].as_array().unwrap().len(),
        document["files_changed"],
        document["files"].as_array().unwrap().len(),
        document["partial"],
    ));
}

#[test]
fn commit() {
    expect![[r#"
        exit 0
        --- stdout
        github.com/gorilla/mux@98cb6bf42e08 • 2020-07-11 • Harshavardhana • 2 files changed, +28 -1
        parents: github.com/gorilla/mux@948bec34b516 • diffed against

            fix: regression in vars extract for wildcard host (#579)

            Continuing from PR #447 we have to add extra
            check to ignore the port as well

            add tests to cover this case

        github.com/gorilla/mux@98cb6bf42e08
        old_test.go • modified • +22 -1
        regexp.go • modified • +6 -0
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&compare(
        "compare/commit.json",
        &[&format!("{MUX}@98cb6bf42e08")],
    ));
    expect![[r#"
        exit 0
        --- stdout
        {"version":1,"kind":"comparison","repo":"github.com/gorilla/mux","base":null,"head":"98cb6bf42e086f6af920b965c38cacc07402d51b","date":"2020-07-11T20:05:21Z","author":"Harshavardhana","message":"fix: regression in vars extract for wildcard host (#579)\n\nContinuing from PR #447 we have to add extra\ncheck to ignore the port as well\n\nadd tests to cover this case","parents":["948bec34b5168796bf3fc4dbb09215baa970351a"],"path":null,"commits":[],"commit_count":1,"commits_truncated":false,"files_changed":2,"added":28,"deleted":1,"files":[{"path":"old_test.go","old_path":null,"status":"modified","binary":false,"added":22,"deleted":1,"readable_at":"98cb6bf42e086f6af920b965c38cacc07402d51b"},{"path":"regexp.go","old_path":null,"status":"modified","binary":false,"added":6,"deleted":0,"readable_at":"98cb6bf42e086f6af920b965c38cacc07402d51b"}],"hints":[]}
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&compare(
        "compare/commit.json",
        &[&format!("{MUX}@98cb6bf42e08"), "--json"],
    ));
}

#[test]
fn commit_file() {
    expect![[r#"
        exit 0
        --- stdout
        github.com/gorilla/mux@98cb6bf42e08 regexp.go • 2020-07-11 • Harshavardhana • 1 file changed, +6 -0
        parents: github.com/gorilla/mux@948bec34b516 • diffed against

            fix: regression in vars extract for wildcard host (#579)

            Continuing from PR #447 we have to add extra
            check to ignore the port as well

            add tests to cover this case

        github.com/gorilla/mux@98cb6bf42e08
        regexp.go • modified • +6 -0
        @@ -325,6 +325,12 @@ func (v routeRegexpGroup) setMatch(req *http.Request, m *RouteMatch, r *Route) {
         	// Store host variables.
         	if v.host != nil {
         		host := getHost(req)
        +		if v.host.wildcardHostPort {
        +			// Don't be strict on the port match
        +			if i := strings.Index(host, ":"); i != -1 {
        +				host = host[:i]
        +			}
        +		}
         		matches := v.host.regexp.FindStringSubmatchIndex(host)
         		if len(matches) > 0 {
         			extractVars(host, matches, v.host.varsN, m.Vars)
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&compare(
        "compare/commit-file.json",
        &[&format!("{MUX}@98cb6bf42e08"), "regexp.go"],
    ));
}

#[test]
fn merge() {
    expect![[r#"
        exit 0
        --- stdout
        github.com/gorilla/mux@2854a05fd3e2 • 2019-11-17 • Matt Silverlock • 2 files changed, +30 -15
        parents: github.com/gorilla/mux@f395758b854c github.com/gorilla/mux@946b6237eb8d • diffed against the first

            Merge pull request #535 from fharding1/cors-subrouter-bug

            Fix the CORSMethodMiddleware bug with subrouters

        github.com/gorilla/mux@2854a05fd3e2
        middleware.go • modified • +10 -15
        middleware_test.go • modified • +20 -0
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&compare(
        "compare/merge.json",
        &[&format!("{MUX}@2854a05fd3e2")],
    ));
}

#[test]
fn crlf_message() {
    expect![[r#"
        exit 0
        --- stdout
        github.com/gorilla/mux@ed099d423848 • 2019-05-17 • M@ • 1 file changed, +27 -10
        parents: github.com/gorilla/mux@c5c6c98bc253 • diffed against

            host:port matching does not require a :port to be specified.

            In lieu of checking the template pattern on every Match request, a bool is added to the routeRegexp, and set
            if the routeRegexp is a host AND there is no ":" in the template. I dislike extending the type, but I'd dislike
            doing a string match on every single Match, even more.

        github.com/gorilla/mux@ed099d423848
        regexp.go • modified • +27 -10
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&compare(
        "compare/crlf-message.json",
        &[&format!("{MUX}@ed099d423848")],
    ));
}

#[test]
fn no_newline_old() {
    expect![[r#"
        exit 0
        --- stdout
        github.com/golang/go@48469a2c866c src/cmd/ld/lib.h • 2015-02-21 • David du Colombier • 1 file changed, +1 -1
        parents: github.com/golang/go@57cefa657d6e • diffed against

            cmd/ld: fix warning on Plan 9

            cpp: src/cmd/ld/lib.h:349 No newline at end of file

            Change-Id: Id21851963f7778364ba9337da3bacd312443f51f
            Reviewed-on: https://go-review.googlesource.com/5520
            Reviewed-by: Minux Ma <minux@golang.org>

        github.com/golang/go@48469a2c866c
        src/cmd/ld/lib.h • modified • +1 -1
        @@ -346,4 +346,4 @@ void	ldmain(int, char**);
        ␠
         #pragma	varargck	argpos	diag	1
        ␠
        -#define	SYMDEF	"__.GOSYMDEF"
        \ No newline at end of file
        +#define	SYMDEF	"__.GOSYMDEF"
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&compare(
        "compare/no-newline-old.json",
        &[&format!("{GO}@48469a2c866c"), "src/cmd/ld/lib.h"],
    ));
}

#[test]
fn no_newline_new() {
    let stdout = answer(
        "compare/no-newline-new.json",
        &[&format!("{MUX}@1fa2ee87bf61"), ".editorconfig"],
    );
    let lines: Vec<&str> = stdout.lines().collect();
    expect![[r#"
        +
        +eclint_indent_style = unset
        \ No newline at end of file
    "#]]
    .assert_eq(&(lines[lines.len() - 3..].join("\n") + "\n"));
}

#[test]
fn binary() {
    expect![[r#"
        exit 0
        --- stdout
        github.com/golang/go@620058f867b2 • 2026-08-25 • Ian Lance Taylor • 3 files changed, +25 -1
        parents: github.com/golang/go@f30a9b3490f8 • diffed against

            debug/elf: use addend when applying ppc relocations

            CL 705075 accidentally dropped the addend value when applying
            ppc relocations. This CL restores it.

            For #75516
            Fixes #81112

            Change-Id: I4313325647f9315ce77cae6548aff3c38e66815f
            Reviewed-on: https://go-review.googlesource.com/c/go/+/821720
            Auto-Submit: Ian Lance Taylor <iant@golang.org>
            Reviewed-by: Tobias Klauser <tobias.klauser@gmail.com>
            Reviewed-by: Cherry Mui <cherryyz@google.com>
            Reviewed-by: David Chase <drchase@google.com>
            LUCI-TryBot-Result: golang-scoped@luci-project-accounts.iam.gserviceaccount.com <golang-scoped@luci-project-accounts.iam.gserviceaccount.com>
            SLSA-Policy-Verified: SLSA Policy Verification Service <devtools-gerritcodereview-exitgate@google.com>

        github.com/golang/go@620058f867b2
        src/debug/elf/file.go • modified • +1 -1
        src/debug/elf/file_test.go • modified • +24 -0
        src/debug/elf/testdata/go-relocation-test-gcc133-ppc.obj • added • binary
        [EOF]
        --- stderr
        [EOF]
    "#]].assert_eq(&compare(
        "compare/binary.json",
        &[&format!("{GO}@620058f867b2")],
    ));
}

#[test]
fn unchanged_path() {
    expect![[r#"
        exit 0
        --- stdout
        github.com/gorilla/mux@a7962380ca08...98cb6bf42e08 LICENSE • no changes
        [EOF]
        --- stderr
        [EOF]
    "#]]
    .assert_eq(&compare("compare/unchanged-path.json", &[RELEASE, "LICENSE"]));
}

#[test]
fn missing_base() {
    expect![[r#"
        exit 1
        --- stdout
        [EOF]
        --- stderr
        base revision v9.9.9 not found in github.com/gorilla/mux
        hint: sego list github.com/gorilla/mux
        [EOF]
    "#]]
    .assert_eq(&compare(
        "compare/missing-base.json",
        &[&format!("{MUX}@v9.9.9...v1.8.0")],
    ));
}

#[test]
fn missing_head() {
    expect![[r#"
        exit 1
        --- stdout
        [EOF]
        --- stderr
        head revision nope-branch not found in github.com/gorilla/mux
        hint: sego list github.com/gorilla/mux
        [EOF]
    "#]]
    .assert_eq(&compare(
        "compare/missing-head.json",
        &[&format!("{MUX}@v1.7.0...nope-branch")],
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
    .assert_eq(&compare(
        "compare/missing-repo.json",
        &["github.com/gorilla/doesnotexist@v1.7.0...v1.8.0"],
    ));
}

#[test]
fn missing_path() {
    expect![[r#"
        exit 1
        --- stdout
        [EOF]
        --- stderr
        nope.go not found in github.com/gorilla/mux@98cb6bf42e08
        hint: sego list github.com/gorilla/mux@98cb6bf42e08
        [EOF]
    "#]]
    .assert_eq(&compare("compare/missing-path.json", &[RELEASE, "nope.go"]));
}

#[test]
fn root() {
    let commit = format!("{MUX}@4e8d10dc715f");
    expect![[r#"
        exit 1
        --- stdout
        [EOF]
        --- stderr
        github.com/gorilla/mux@4e8d10dc715f is a root commit, with no parent to compare with
        hint: sego list github.com/gorilla/mux@4e8d10dc715f
        [EOF]
    "#]].assert_eq(&compare("compare/root.json", &[&commit]));
    expect![[r#"
        exit 1
        --- stdout
        {"version":1,"kind":"unavailable","code":"no_parent","message":"github.com/gorilla/mux@4e8d10dc715f is a root commit, with no parent to compare with","hints":["sego list github.com/gorilla/mux@4e8d10dc715f"]}
        [EOF]
        --- stderr
        github.com/gorilla/mux@4e8d10dc715f is a root commit, with no parent to compare with
        hint: sego list github.com/gorilla/mux@4e8d10dc715f
        [EOF]
    "#]]
        .assert_eq(&compare("compare/root.json", &[&commit, "--json"]));
}

#[test]
fn usage() {
    check!(
        sego().args(["compare", &format!("{MUX}@v1.7.0..v1.8.0")]),
        expect![[r#"
            exit 2
            --- stdout
            [EOF]
            --- stderr
            error: invalid revision v1.7.0..v1.8.0; a revision cannot contain ".."
            usage: sego compare [--json] <repo>@[<base>...]<head> [<path>]
            [EOF]
        "#]]
    );
    check!(
        sego().args(["compare", MUX]),
        expect![[r#"
        exit 2
        --- stdout
        [EOF]
        --- stderr
        error: missing revision; expected <repo>@[<base>...]<head>
        usage: sego compare [--json] <repo>@[<base>...]<head> [<path>]
        [EOF]
    "#]]
    );
    check!(
        sego().args(["compare"]),
        expect![[r#"
        exit 2
        --- stdout
        [EOF]
        --- stderr
        error: missing operands; expected <repo>@[<base>...]<head> [<path>]
        usage: sego compare [--json] <repo>@[<base>...]<head> [<path>]
        [EOF]
    "#]]
    );
}

#[test]
fn help() {
    check!(
        sego().args(["compare", "--help"]),
        expect![[r#"
        exit 0
        --- stdout
        sego compare prints what changed between two revisions, or in one commit:
        the commits, the files changed and, for a file, its hunks.

        Usage:
          sego compare [--json] <repo>@[<base>...]<head> [<path>]

        Operands:
          <repo>    a repository, such as github.com/gorilla/mux; a URL, a trailing
                    .git and a trailing slash are accepted. Matching is
                    case-insensitive.
          <base>    a branch, tag or commit-ish, such as v1.7.0 or a7962380~1; it
                    cannot contain "..". The files are compared from the merge
                    base of <base> and <head>, as git diff <base>...<head> does,
                    and the commits are those on <head> and not on <base>.
          <head>    a branch, tag or commit-ish. Without <base>, the commit it
                    names is compared with its first parent.
          <path>    a file or a directory, from the repository root. A directory
                    narrows the commits and the files to those below it; a file
                    also shows its hunks. Without it, the whole repository.

        Output:
          A first line: the range, pinned to the commits the revisions resolved
          to, the path, the number of commits, and the files changed with the
          lines added and deleted. Then, after a blank line, the commits, newest
          first: each an address line with the day of its author date (UTC), its
          author and its subject. Then, after a blank line, the head's address
          line, and a line for each file changed, in path order: its path, then
          added, deleted, renamed from its old path, or modified, then the lines
          added and deleted, or binary:

            github.com/gorilla/mux@a7962380ca08...98cb6bf42e08 • 30 commits • …

            github.com/gorilla/mux@98cb6bf42e08 • 2020-07-11 • Harshavardhana • …
            …

            github.com/gorilla/mux@98cb6bf42e08
            .circleci/config.yml • added • +87
            context.go • deleted at a7962380ca08 • -18
            mux.go • modified • +21 -16

          A deleted file names the commit that holds it, the base, when the base
          is an ancestor of the head; otherwise it has no address. A file renamed
          in the range, compared by its new path, shows as added; the comparison
          of the whole repository, or of a directory holding both paths, shows
          the rename.

          The list of commits stops at the newest 50, and the list of files at the
          first 200, each then followed by a notice: line with the total. With a
          <path>, a list of commits that stopped says more than 50, as there is no
          exact count.

          A file's <path> shows its hunks after its line, as a unified diff. A line
          wider than 500 columns is cut, and says so; --json has it whole.

          A single commit's first line is its address line, with the day, the
          author and the files changed. Then its parents, the first being the one
          it is diffed against; then, after a blank line, its whole message,
          indented; then its files.

          A comparison with no commits says so: <head> has none that <base> lacks.
          When <base> has commits that <head> lacks, a hint: line follows with the
          command for the two swapped. A <path> that didn't change says no changes.

          With --json, one document of kind "comparison": repo, base (null for a
          single commit) and head (40 hex digits); for a single commit, date,
          author, message and parents; path; commits [{commit, date, author,
          subject}], commit_count (null when there is no exact count) and
          commits_truncated; files_changed, added, deleted; files [{path,
          old_path, status, binary, added, deleted, readable_at}], with hunks
          [{old_start, old_lines, new_start, new_lines, section, lines,
          no_newline_old, no_newline_new}] for a file's <path>; hints; and
          partial {reason, hints} when a list stopped. Anything else is reported
          on stderr, and its document, of kind "unavailable" or "error", also
          goes to stdout.

        Exit codes:
          0  answered      the comparison, including one with no commits
          1  answered no   no such repository, revision or path; a commit without
                           <base> that has no parent
          2  invalid       bad operands, such as no @<head>, a two-dot range or a
                           revision containing ".."; an invalid SEGO_ENDPOINT
          3  unanswered    no usable answer from the upstream; retry

        Examples:
          sego compare github.com/gorilla/mux@v1.7.0...v1.8.0
          sego compare github.com/gorilla/mux@v1.7.0...v1.8.0 mux.go
          sego compare github.com/gorilla/mux@98cb6bf42e08
        [EOF]
        --- stderr
        [EOF]
    "#]]
    );
}

#[test]
fn help_fits_79_columns() {
    let run = sego().args(["compare", "--help"]).run();
    assert_eq!(run.code, 0);
    for line in run.stdout.lines() {
        assert!(line.chars().count() <= 79, "too wide: {line}");
    }
}
