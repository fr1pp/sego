use serde_json::json;

use sego::internal::address::{Address, Dots, Repo, Rev, UrlKind, parse};
use sego::internal::error::{Error, report};
use sego::internal::upstream::{
    Candidate, Envelope, Located, Plan, RepoAt, RepoPlan, Root, Var, build,
    build_repository, candidates, classify, locate, locate_repository,
    missing_revisions,
};

use crate::support::fixture::reply;

const SHA: &str = "98cb6bf42e086f6af920b965c38cacc07402d51b";

fn toy_vars(candidate: &Candidate) -> Vec<Var> {
    vec![Var {
        name: "path",
        ty: "String!",
        value: json!(candidate.path.clone().unwrap_or_default()),
    }]
}

fn toy() -> Plan {
    Plan {
        operation: "Toy",
        commit_template: "meta: blob(path: $path) { byteSize }",
        vars: Box::new(toy_vars),
    }
}

fn address(token: &str) -> Address {
    parse(token).unwrap_or_else(|e| panic!("{token}: {e}"))
}

fn pretty(map: &serde_json::Map<String, serde_json::Value>) -> String {
    serde_json::to_string_pretty(map).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn document_shapes() {
    let request =
        build(&toy(), &address("github.com/gorilla/mux@v1.8.0:mux.go"));
    assert_eq!(request.operation, "Toy");
    assert_eq!(
        request.document,
        "\
query Toy($repo0: String!, $rev0: String!, $path_0: String!) {
  r0: repository(name: $repo0) {
    name
    defaultBranch { displayName }
    c0: commit(rev: $rev0) {
      oid
      meta: blob(path: $path_0) { byteSize }
    }
  }
}
"
    );
    assert_eq!(
        pretty(&request.variables),
        r#"{
  "path_0": "mux.go",
  "repo0": "github.com/gorilla/mux",
  "rev0": "v1.8.0"
}"#
    );

    let request = build(&toy(), &address("gorilla/mux:mux.go"));
    assert_eq!(
        request.document,
        "\
query Toy($repo0: String!, $repo1: String!, $rev0: String!, $path_0: String!) {
  r0: repository(name: $repo0) {
    name
    defaultBranch { displayName }
    c0: commit(rev: $rev0) {
      oid
      meta: blob(path: $path_0) { byteSize }
    }
  }
  r1: repository(name: $repo1) {
    name
    defaultBranch { displayName }
    c0: commit(rev: $rev0) {
      oid
      meta: blob(path: $path_0) { byteSize }
    }
  }
}
"
    );
    assert_eq!(
        pretty(&request.variables),
        r#"{
  "path_0": "mux.go",
  "repo0": "github.com/gorilla/mux",
  "repo1": "gitlab.com/gorilla/mux",
  "rev0": "HEAD"
}"#
    );
}

#[test]
fn document_shapes_for_splits() {
    let request = build(
        &toy(),
        &address(
            "https://github.com/bminor/glibc/blob/release/2.34/master/NEWS",
        ),
    );
    assert_eq!(
        request.document,
        "\
query Toy($repo0: String!, $rev0: String!, $rev1: String!, $rev2: String!, $rev3: String!, $path_0: String!, $path_1: String!, $path_2: String!, $path_3: String!) {
  r0: repository(name: $repo0) {
    name
    defaultBranch { displayName }
    c0: commit(rev: $rev0) {
      oid
      meta: blob(path: $path_0) { byteSize }
      x0: path(path: $path_0) { __typename }
    }
    c1: commit(rev: $rev1) {
      oid
      meta: blob(path: $path_1) { byteSize }
      x1: path(path: $path_1) { __typename }
    }
    c2: commit(rev: $rev2) {
      oid
      meta: blob(path: $path_2) { byteSize }
      x2: path(path: $path_2) { __typename }
    }
    c3: commit(rev: $rev3) {
      oid
      meta: blob(path: $path_3) { byteSize }
      x3: path(path: $path_3) { __typename }
    }
  }
}
"
    );
    assert_eq!(
        pretty(&request.variables),
        r#"{
  "path_0": "2.34/master/NEWS",
  "path_1": "master/NEWS",
  "path_2": "NEWS",
  "path_3": "",
  "repo0": "github.com/bminor/glibc",
  "rev0": "release",
  "rev1": "release/2.34",
  "rev2": "release/2.34/master",
  "rev3": "release/2.34/master/NEWS"
}"#
    );
}

#[test]
fn body_is_the_graphql_envelope() {
    let request =
        build(&toy(), &address("github.com/gorilla/mux@v1.8.0:mux.go"));
    let body: serde_json::Value = serde_json::from_slice(&request.body())
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        body,
        json!({
            "query": request.document,
            "variables": {
                "repo0": "github.com/gorilla/mux",
                "rev0": "v1.8.0",
                "path_0": "mux.go",
            },
            "operationName": "Toy",
        })
    );
}

#[test]
fn placeholders_are_renamed_whole_word() {
    fn vars(_: &Candidate) -> Vec<Var> {
        vec![
            Var { name: "path", ty: "String!", value: json!("a") },
            Var { name: "pathx", ty: "Int", value: json!(1) },
        ]
    }
    let plan = Plan {
        operation: "Words",
        commit_template: "a: f(p: $path, q: $pathx)",
        vars: Box::new(vars),
    };
    let request = build(&plan, &address("github.com/a/b:x"));
    assert!(
        request.document.contains("a: f(p: $path_0, q: $pathx_0)"),
        "{}",
        request.document
    );
    assert!(request.document.contains("$path_0: String!, $pathx_0: Int)"));
    assert_eq!(request.variables.len(), 4);
}

#[test]
fn candidates_for_splits() {
    let a = address(
        "https://github.com/bminor/glibc/blob/release/2.34/master/NEWS",
    );
    let got: Vec<(String, Option<String>)> =
        candidates(&a).into_iter().map(|c| (c.rev, c.path)).collect();
    let want = [
        ("release", Some("2.34/master/NEWS")),
        ("release/2.34", Some("master/NEWS")),
        ("release/2.34/master", Some("NEWS")),
        ("release/2.34/master/NEWS", None),
    ]
    .map(|(r, p)| (r.to_owned(), p.map(str::to_owned)));
    assert_eq!(got, want);
}

#[test]
fn candidates_for_other_revs() {
    let default = candidates(&address("github.com/a/b:x"));
    assert_eq!(default.len(), 1);
    assert_eq!(default[0].rev, "HEAD");
    assert_eq!(default[0].path.as_deref(), Some("x"));

    let one = candidates(&address("github.com/a/b@v1"));
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].rev, "v1");
    assert_eq!(one[0].path, None);

    let range = Address {
        repo: Repo::Exact("github.com/a/b".to_owned()),
        rev: Rev::Range {
            base: "a".to_owned(),
            head: "b".to_owned(),
            dots: Dots::Two,
        },
        path: None,
        lines: None,
        kind: UrlKind::Compare,
    };
    assert!(candidates(&range).is_empty());
}

fn split_address(segs: &[&str]) -> Address {
    Address {
        repo: Repo::Exact("github.com/a/b".to_owned()),
        rev: Rev::Split(segs.iter().map(ToString::to_string).collect()),
        path: None,
        lines: None,
        kind: UrlKind::Blob,
    }
}

#[test]
fn invalid_split_prefixes_are_skipped() {
    let a = split_address(&["-x", "a", "b"]);
    assert!(candidates(&a).is_empty());

    let a = split_address(&["ok", "bad name", "f"]);
    let got: Vec<String> = candidates(&a).into_iter().map(|c| c.rev).collect();
    assert_eq!(got, ["ok"]);

    let a = split_address(&["-x"]);
    assert!(candidates(&a).is_empty());
    let request = build(&toy(), &a);
    assert!(!request.document.contains("c0"), "{}", request.document);
    assert!(!request.document.contains("$rev"), "{}", request.document);
}

#[test]
#[should_panic(expected = "range")]
fn building_a_range_panics() {
    let a = Address {
        repo: Repo::Exact("github.com/a/b".to_owned()),
        rev: Rev::Range {
            base: "a".to_owned(),
            head: "b".to_owned(),
            dots: Dots::Three,
        },
        path: None,
        lines: None,
        kind: UrlKind::Compare,
    };
    build(&toy(), &a);
}

#[test]
fn hex_and_tilde_prefixes_get_ref_lookups() {
    let lookup = |j: usize| {
        format!(
            "g{j}: gitRefs(query: $rev{j}, first: 5) {{ nodes {{ \
             displayName }} }}"
        )
    };
    let request = build(&toy(), &split_address(&["abcd1234", "y", "z"]));
    assert!(request.document.contains(&lookup(0)), "{}", request.document);
    assert!(!request.document.contains("g1:"), "{}", request.document);
    assert!(!request.document.contains("g2:"), "{}", request.document);

    let request = build(&toy(), &split_address(&["main", "x~1"]));
    assert!(!request.document.contains("g0:"), "{}", request.document);
    assert!(request.document.contains(&lookup(1)), "{}", request.document);

    let request = build(&toy(), &split_address(&["abc", "f"]));
    assert!(!request.document.contains("g0:"), "{}", request.document);
}

fn envelope(name: &str) -> Envelope {
    classify(reply(&format!("resolve/{name}")))
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn rendered(error: &Error) -> String {
    let mut out = Vec::new();
    report(error, &mut out).unwrap_or_else(|e| panic!("{e}"));
    String::from_utf8(out).unwrap_or_else(|e| panic!("{e}"))
}

fn located(fixture: &str, token: &str) -> Located {
    locate(&address(token), &envelope(fixture))
        .unwrap_or_else(|e| panic!("{fixture}: {}", rendered(&e)))
}

fn refused(fixture: &str, token: &str) -> String {
    match locate(&address(token), &envelope(fixture)) {
        Ok(found) => panic!("{fixture}: located {found:?}"),
        Err(error) => rendered(&error),
    }
}

fn at(parts: &[&str]) -> Vec<String> {
    parts.iter().map(ToString::to_string).collect()
}

#[test]
fn locate_cases() {
    assert_eq!(
        located("found", "gorilla/mux@v1.8.0:mux.go"),
        Located {
            repo: "github.com/gorilla/mux".to_owned(),
            commit: SHA.to_owned(),
            rev: Some("v1.8.0".to_owned()),
            path: Some("mux.go".to_owned()),
            at: at(&["r0", "c0"]),
        }
    );
    assert_eq!(
        located("found", "gorilla/mux:mux.go").rev,
        None,
        "the default branch carries no rev"
    );

    let blocked = refused("no-repo", "github.com/facebook/react");
    assert!(
        blocked
            .contains("github.com/facebook/react is not on sourcegraph.com"),
        "{blocked}"
    );
    assert!(
        blocked.contains(
            "sourcegraph.com serves indexed repositories from github.com, \
             gitlab.com and a few other hosts; a renamed repository appears \
             only under its new name"
        ),
        "{blocked}"
    );
    assert!(
        refused("no-repo", "gorilla/mux")
            .contains("gorilla/mux is not on sourcegraph.com")
    );

    assert!(refused("both-hosts", "gorilla/mux").contains(
        "gorilla/mux exists as github.com/gorilla/mux and \
         gitlab.com/gorilla/mux"
    ));
    assert!(
        refused("no-rev", "gorilla/mux@v9.9")
            .contains("revision v9.9 not found in github.com/gorilla/mux")
    );
    assert!(
        refused("empty-repo", "github.com/ascoders/blog")
            .contains("github.com/ascoders/blog has no commits")
    );
}

#[test]
fn locate_splits() {
    let glibc =
        "https://github.com/bminor/glibc/blob/release/2.34/master/NEWS";
    assert_eq!(
        located("split", glibc),
        Located {
            repo: "github.com/bminor/glibc".to_owned(),
            commit: SHA.to_owned(),
            rev: Some("release/2.34/master".to_owned()),
            path: Some("NEWS".to_owned()),
            at: at(&["r0", "c2"]),
        }
    );
    let found = located("split-error", glibc);
    assert_eq!(found.rev.as_deref(), Some("release/2.34/master"));
    assert_eq!(found.path.as_deref(), Some("NEWS"));

    let none = refused("split-none", glibc);
    assert!(
        none.contains(
            "no revision and path in release/2.34/master/NEWS resolve in \
             github.com/bminor/glibc"
        ),
        "{none}"
    );
}

#[test]
fn locate_split_hex() {
    let found = located("split-hex", "github.com/o/r/blob/main/v~1");
    assert_eq!(found.rev.as_deref(), Some("main"));
    assert_eq!(found.path.as_deref(), Some("v~1"));
    assert_eq!(found.at, at(&["r0", "c0"]));

    let found =
        located("split-hex-alone", "github.com/o/r/blob/abcd1234/f.go");
    assert_eq!(found.rev.as_deref(), Some("abcd1234"));
    assert_eq!(found.path.as_deref(), Some("f.go"));
}

#[test]
fn locate_with_no_valid_candidates() {
    let a = split_address(&["-x"]);
    let e = locate(&a, &envelope("found"));
    match e {
        Ok(found) => panic!("{found:?}"),
        Err(error) => assert!(
            rendered(&error).contains("no revision and path in -x resolve in"),
            "{}",
            rendered(&error)
        ),
    }
}

#[test]
fn errors_are_not_missing_things() {
    let root = refused("root-error", "github.com/gorilla/mux");
    assert!(
        root.contains("Sourcegraph failed to answer the repository lookup"),
        "{root}"
    );
    assert!(!root.contains("internal git failure"), "{root}");
    let commit = refused("commit-error", "github.com/gorilla/mux@v1");
    assert!(
        commit.contains("Sourcegraph failed to answer the revision lookup"),
        "{commit}"
    );
    assert!(!commit.contains("internal git failure"), "{commit}");

    let glibc =
        "https://github.com/bminor/glibc/blob/release/2.34/master/NEWS";
    let split = refused("split-commit-error", glibc);
    assert!(
        split.contains("Sourcegraph failed to answer the revision lookup"),
        "{split}"
    );
    let found = located("split-commit-error-wins", glibc);
    assert_eq!(found.rev.as_deref(), Some("release/2.34/master"));
}

#[test]
fn empty_repository_through_a_split() {
    let blocked = refused(
        "split-empty",
        "https://github.com/bminor/glibc/blob/release/2.34/master/NEWS",
    );
    assert!(blocked.contains("github.com/bminor/glibc has no commits"));
}

#[test]
fn canonical_name_falls_back_to_the_candidate() {
    assert_eq!(
        located("no-name", "github.com/gorilla/mux@v1").repo,
        "github.com/gorilla/mux"
    );
    assert!(refused("no-name", "gorilla/mux").contains(
        "gorilla/mux exists as github.com/gorilla/mux and \
         gitlab.com/gorilla/mux"
    ));
}

#[test]
fn an_errored_name_falls_back() {
    assert_eq!(
        located("errored-name", "github.com/x/y@v1").repo,
        "github.com/x/y"
    );
}

#[test]
fn an_errored_oid_is_unresolved() {
    let lookup = "Sourcegraph failed to answer the revision lookup";
    let blocked = refused("errored-oid", "github.com/gorilla/mux@v1");
    assert!(blocked.contains(lookup), "{blocked}");
    assert!(!blocked.contains("not found"), "{blocked}");
}

#[test]
fn an_error_inside_a_null_commit_failed() {
    let lookup = "Sourcegraph failed to answer the revision lookup";
    let blocked =
        refused("null-commit-oid-error", "github.com/gorilla/mux@v1");
    assert!(blocked.contains(lookup), "{blocked}");
    assert!(!blocked.contains("not found"), "{blocked}");
}

#[test]
fn a_resolved_candidate_keeps_its_expected_errors() {
    let none = refused(
        "split-resolved-expected-errors",
        "https://github.com/bminor/glibc/blob/release/2.34/master/NEWS",
    );
    assert!(
        none.contains(
            "no revision and path in release/2.34/master/NEWS resolve in \
             github.com/bminor/glibc"
        ),
        "{none}"
    );
}

#[test]
fn only_splits_look_up_refs() {
    for token in ["github.com/a/b@abcd1234:f", "github.com/a/b@main~1:f"] {
        let request = build(&toy(), &address(token));
        assert!(!request.document.contains("g0:"), "{}", request.document);
    }
}

#[test]
fn one_host_failing_is_not_one_host_existing() {
    let lookup = "Sourcegraph failed to answer the repository lookup";
    let blocked = refused("one-host-error", "gorilla/mux");
    assert!(blocked.contains(lookup), "{blocked}");
    assert!(!blocked.contains("internal git failure"), "{blocked}");
}

#[test]
fn failed_default_branch_is_not_an_empty_repository() {
    let lookup = "Sourcegraph failed to answer the repository lookup";
    let single = refused("branch-error", "github.com/ascoders/blog");
    assert!(single.contains(lookup), "{single}");
    let split = refused(
        "split-branch-error",
        "https://github.com/bminor/glibc/blob/release/2.34/master/NEWS",
    );
    assert!(split.contains(lookup), "{split}");
}

const REPO_PARTS: &[(&str, &str)] =
    &[("tags", "the tag list"), ("all_branches", "the branch list")];

fn repo_plan() -> RepoPlan {
    RepoPlan {
        operation: "Toy",
        vars: vec![Var { name: "first", ty: "Int!", value: json!(20) }],
        roots: vec![Root {
            alias: "r",
            template: "tags(first: $first) {\n  totalCount\n}".to_owned(),
        }],
        parts: REPO_PARTS,
    }
}

fn named_plan() -> RepoPlan {
    RepoPlan {
        operation: "Named",
        vars: vec![Var { name: "rev", ty: "String!", value: json!("v1") }],
        roots: vec![
            Root {
                alias: "m",
                template: "commit(rev: $rev) {\n  oid\n}".to_owned(),
            },
            Root {
                alias: "d",
                template: "commit(rev: $rev) {\n  oid\n}".to_owned(),
            },
        ],
        parts: &[],
    }
}

fn repo_of(token: &str) -> Repo {
    address(token).repo
}

fn repo_refused(fixture: &str, token: &str) -> String {
    match locate_repository(&repo_of(token), &envelope(fixture), &repo_plan())
    {
        Ok(found) => panic!("{fixture}: located {}", found.repo),
        Err(error) => rendered(&error),
    }
}

#[test]
fn repository_roots() {
    let request =
        build_repository(&repo_plan(), &repo_of("github.com/gorilla/mux"));
    assert_eq!(request.operation, "Toy");
    assert_eq!(
        request.document,
        "\
query Toy($repo0: String!, $first: Int!) {
  r0: repository(name: $repo0) {
    name
    defaultBranch { displayName }
    tags(first: $first) {
      totalCount
    }
  }
}
"
    );
    assert_eq!(
        pretty(&request.variables),
        r#"{
  "first": 20,
  "repo0": "github.com/gorilla/mux"
}"#
    );

    let request = build_repository(&repo_plan(), &repo_of("gorilla/mux"));
    assert_eq!(
        request.document,
        "\
query Toy($repo0: String!, $repo1: String!, $first: Int!) {
  r0: repository(name: $repo0) {
    name
    defaultBranch { displayName }
    tags(first: $first) {
      totalCount
    }
  }
  r1: repository(name: $repo1) {
    name
    defaultBranch { displayName }
    tags(first: $first) {
      totalCount
    }
  }
}
"
    );
    assert_eq!(
        pretty(&request.variables),
        r#"{
  "first": 20,
  "repo0": "github.com/gorilla/mux",
  "repo1": "gitlab.com/gorilla/mux"
}"#
    );

    let found = locate_repository(
        &repo_of("gorilla/mux"),
        &envelope("repo-found"),
        &repo_plan(),
    )
    .unwrap_or_else(|e| panic!("{}", rendered(&e)));
    assert_eq!(found.repo, "github.com/gorilla/mux");
    assert_eq!(found.index, 0);
    assert_eq!(found.at("r"), "r0");

    let empty = locate_repository(
        &repo_of("github.com/ascoders/blog"),
        &envelope("repo-empty"),
        &repo_plan(),
    )
    .unwrap_or_else(|e| panic!("{}", rendered(&e)));
    assert_eq!(empty.repo, "github.com/ascoders/blog");
    assert_eq!(empty.index, 0);
    assert_eq!(empty.at("r"), "r0");
}

#[test]
fn repository_lookup_errors() {
    let blocked = repo_refused("repo-none", "gorilla/mux");
    assert!(
        blocked.contains("gorilla/mux is not on sourcegraph.com"),
        "{blocked}"
    );
    assert!(
        blocked.contains(
            "sourcegraph.com serves indexed repositories from github.com, \
             gitlab.com and a few other hosts; a renamed repository appears \
             only under its new name"
        ),
        "{blocked}"
    );

    let both = repo_refused("repo-both-hosts", "gorilla/mux");
    assert!(
        both.contains(
            "gorilla/mux exists as github.com/gorilla/mux and \
             gitlab.com/gorilla/mux"
        ),
        "{both}"
    );

    let one = repo_refused("repo-one-host-error", "gorilla/mux");
    assert!(
        one.contains("Sourcegraph failed to answer the repository lookup"),
        "{one}"
    );
}

#[test]
fn a_null_root_with_errors_beneath_failed() {
    let tags = repo_refused("repo-tags-error", "github.com/gorilla/mux");
    assert!(
        tags.contains("Sourcegraph failed to answer the tag list"),
        "{tags}"
    );
    assert!(!tags.contains("not on sourcegraph.com"), "{tags}");

    let branches =
        repo_refused("repo-branches-error", "github.com/gorilla/mux");
    assert!(
        branches.contains("Sourcegraph failed to answer the branch list"),
        "{branches}"
    );

    for fixture in ["repo-root-error", "repo-unknown-part-error"] {
        let lookup = repo_refused(fixture, "github.com/gorilla/mux");
        assert!(
            lookup.contains(
                "Sourcegraph failed to answer the repository lookup"
            ),
            "{fixture}: {lookup}"
        );
    }

    let one = repo_refused("repo-one-host-tags-error", "gorilla/mux");
    assert!(
        one.contains("Sourcegraph failed to answer the tag list"),
        "{one}"
    );

    let both = repo_refused("repo-both-null-errors", "gorilla/mux");
    assert!(
        both.contains("Sourcegraph failed to answer the repository lookup"),
        "all_tags is not in this table: {both}"
    );
}

#[test]
fn show_names_no_parts_under_a_failed_root() {
    let lookup = refused("root-error", "github.com/gorilla/mux");
    assert!(
        lookup.contains("Sourcegraph failed to answer the repository lookup"),
        "{lookup}"
    );
}

#[test]
fn named_roots() {
    let request =
        build_repository(&named_plan(), &repo_of("github.com/gorilla/mux"));
    assert_eq!(request.operation, "Named");
    assert_eq!(
        request.document,
        "\
query Named($repo0: String!, $rev: String!) {
  m0: repository(name: $repo0) {
    name
    defaultBranch { displayName }
    commit(rev: $rev) {
      oid
    }
  }
  d0: repository(name: $repo0) {
    name
    defaultBranch { displayName }
    commit(rev: $rev) {
      oid
    }
  }
}
"
    );
    assert_eq!(
        pretty(&request.variables),
        r#"{
  "repo0": "github.com/gorilla/mux",
  "rev": "v1"
}"#
    );

    let request = build_repository(&named_plan(), &repo_of("gorilla/mux"));
    let order: Vec<&str> = request
        .document
        .lines()
        .filter(|line| line.contains(": repository("))
        .map(str::trim)
        .collect();
    assert_eq!(
        order,
        [
            "m0: repository(name: $repo0) {",
            "d0: repository(name: $repo0) {",
            "m1: repository(name: $repo1) {",
            "d1: repository(name: $repo1) {",
        ]
    );
    assert!(
        request.document.starts_with(
            "query Named($repo0: String!, $repo1: String!, $rev: String!) {"
        ),
        "{}",
        request.document
    );
}

#[test]
fn only_the_establishing_root_decides() {
    let found = locate_repository(
        &repo_of("github.com/gorilla/mux"),
        &envelope("repo-second-root-error"),
        &named_plan(),
    )
    .unwrap_or_else(|e| panic!("{}", rendered(&e)));
    assert_eq!(
        found,
        RepoAt { repo: "github.com/gorilla/mux".to_owned(), index: 0 }
    );
    assert_eq!(found.at("m"), "m0");
    assert_eq!(found.at("d"), "d0");

    let blocked = match locate_repository(
        &repo_of("github.com/gorilla/mux"),
        &envelope("repo-first-root-error"),
        &named_plan(),
    ) {
        Ok(found) => panic!("located {}", found.repo),
        Err(error) => rendered(&error),
    };
    assert!(
        blocked.contains("Sourcegraph failed to answer the repository lookup"),
        "{blocked}"
    );
}

#[test]
fn missing_revision_messages() {
    let message = |missing: &[(&str, &str)]| {
        rendered(&missing_revisions("github.com/a/b", missing))
    };
    assert!(
        message(&[("", "v9")])
            .contains("revision v9 not found in github.com/a/b")
    );
    assert!(
        message(&[("base", "v1")])
            .contains("base revision v1 not found in github.com/a/b")
    );
    assert!(
        message(&[("head", "v2")])
            .contains("head revision v2 not found in github.com/a/b")
    );
    assert!(
        message(&[("base", "v1"), ("head", "v2")])
            .contains("revisions v1 and v2 not found in github.com/a/b")
    );
}
