use serde_json::{Value, json};

use super::{
    Candidate, Located, Plan, RepoAt, RepoPlan, Root, Var, build,
    build_repository, candidates, locate, locate_repository,
};
use crate::address::{Address, Dots, Repo, Rev, UrlKind, parse};
use crate::error::{Error, report};
use crate::upstream::envelope::{Envelope, classify};
use crate::upstream::script::reply;

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
fn a_plain_address_builds_one_candidate_document() {
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
fn a_slashed_url_revision_builds_a_document_with_a_candidate_per_split() {
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
fn a_slashed_url_revision_has_a_candidate_per_split() {
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
fn an_address_without_a_split_has_one_candidate_and_a_range_has_none() {
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

const GLIBC: &str =
    "https://github.com/bminor/glibc/blob/release/2.34/master/NEWS";

const LOOKUP: &str = "Sourcegraph failed to answer the repository lookup";

const REVISION_LOOKUP: &str =
    "Sourcegraph failed to answer the revision lookup";

fn envelope_on(host: &str, data: &Value, errors: &[Value]) -> Envelope {
    let body = json!({"data": data, "errors": errors});
    classify(reply(200, Some("application/json"), &body.to_string()), host)
        .unwrap_or_else(|e| panic!("{e}"))
}

fn envelope(data: &Value, errors: &[Value]) -> Envelope {
    envelope_on("sourcegraph.com", data, errors)
}

fn failing(path: &Value) -> Value {
    json!({"message": "internal git failure", "path": path})
}

/// A repository root with its name and default branch, plus the fields of
/// `rest`.
fn repository(name: &str, branch: Option<&str>, rest: &Value) -> Value {
    let mut root = json!({
        "name": name,
        "defaultBranch": branch.map(|b| json!({"displayName": b})),
    });
    if let (Some(root), Some(rest)) = (root.as_object_mut(), rest.as_object())
    {
        root.extend(rest.clone());
    }
    root
}

fn mux(rest: &Value) -> Value {
    repository("github.com/gorilla/mux", Some("main"), rest)
}

fn glibc(rest: &Value) -> Value {
    repository("github.com/bminor/glibc", Some("master"), rest)
}

/// A split candidate that resolves, with the kind of path at `x`.
fn resolved(x: &str, kind: Option<&str>) -> Value {
    json!({"oid": SHA, x: kind.map(|k| json!({"__typename": k}))})
}

fn found() -> Envelope {
    envelope(
        &json!({"r0": mux(&json!({"c0": {"oid": SHA}})), "r1": null}),
        &[],
    )
}

fn root_error() -> Envelope {
    envelope(&json!({"r0": null}), &[failing(&json!(["r0"]))])
}

fn rendered(error: &Error) -> String {
    let mut out = Vec::new();
    report(error, &mut out).unwrap_or_else(|e| panic!("{e}"));
    String::from_utf8(out).unwrap_or_else(|e| panic!("{e}"))
}

fn located(envelope: &Envelope, token: &str) -> Located {
    locate(&address(token), envelope)
        .unwrap_or_else(|e| panic!("{}", rendered(&e)))
}

fn refused(envelope: &Envelope, token: &str) -> String {
    match locate(&address(token), envelope) {
        Ok(found) => panic!("located {found:?}"),
        Err(error) => rendered(&error),
    }
}

fn at(parts: &[&str]) -> Vec<String> {
    parts.iter().map(ToString::to_string).collect()
}

#[test]
fn located_reads_the_repository_commit_revision_and_path_or_names_what_is_missing()
 {
    assert_eq!(
        located(&found(), "gorilla/mux@v1.8.0:mux.go"),
        Located {
            repo: "github.com/gorilla/mux".to_owned(),
            commit: SHA.to_owned(),
            rev: Some("v1.8.0".to_owned()),
            path: Some("mux.go".to_owned()),
            at: at(&["r0", "c0"]),
        }
    );
    assert_eq!(
        located(&found(), "gorilla/mux:mux.go").rev,
        None,
        "the default branch carries no rev"
    );

    let no_repo = envelope(&json!({"r0": null}), &[]);
    let blocked = refused(&no_repo, "github.com/facebook/react");
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
        refused(&no_repo, "gorilla/mux")
            .contains("gorilla/mux is not on sourcegraph.com")
    );

    let elsewhere = envelope_on("h:1", &json!({"r0": null}), &[]);
    let elsewhere = refused(&elsewhere, "github.com/x/y");
    assert!(elsewhere.contains("github.com/x/y is not on h:1"), "{elsewhere}");
    assert!(
        elsewhere.contains("sourcegraph.com serves indexed repositories from"),
        "{elsewhere}"
    );

    let both_hosts = envelope(
        &json!({
            "r0": mux(&json!({"c0": {"oid": SHA}})),
            "r1": repository(
                "gitlab.com/gorilla/mux",
                Some("master"),
                &json!({"c0": {"oid": SHA}}),
            ),
        }),
        &[],
    );
    assert!(refused(&both_hosts, "gorilla/mux").contains(
        "gorilla/mux exists as github.com/gorilla/mux and \
         gitlab.com/gorilla/mux"
    ));
    let no_rev =
        envelope(&json!({"r0": mux(&json!({"c0": null})), "r1": null}), &[]);
    assert!(
        refused(&no_rev, "gorilla/mux@v9.9")
            .contains("revision v9.9 not found in github.com/gorilla/mux")
    );
    let empty_repo = envelope(
        &json!({"r0": repository(
            "github.com/ascoders/blog",
            None,
            &json!({"c0": null}),
        )}),
        &[],
    );
    assert!(
        refused(&empty_repo, "github.com/ascoders/blog")
            .contains("github.com/ascoders/blog has no commits")
    );
}

#[test]
fn a_slashed_url_revision_is_located_at_the_split_that_resolves() {
    let split = envelope(
        &json!({"r0": glibc(&json!({
            "c0": resolved("x0", None),
            "g0": null,
            "c1": resolved("x1", None),
            "c2": resolved("x2", Some("GitBlob")),
            "c3": null,
        }))}),
        &[],
    );
    assert_eq!(
        located(&split, GLIBC),
        Located {
            repo: "github.com/bminor/glibc".to_owned(),
            commit: SHA.to_owned(),
            rev: Some("release/2.34/master".to_owned()),
            path: Some("NEWS".to_owned()),
            at: at(&["r0", "c2"]),
        }
    );

    let errored = envelope(
        &json!({"r0": glibc(&json!({
            "c0": resolved("x0", None),
            "c1": null,
            "c2": resolved("x2", None),
            "c3": null,
        }))}),
        &[json!({"message": "internal", "path": ["r0", "c2", "x2"]})],
    );
    let found = located(&errored, GLIBC);
    assert_eq!(found.rev.as_deref(), Some("release/2.34/master"));
    assert_eq!(found.path.as_deref(), Some("NEWS"));

    let none = envelope(
        &json!({"r0": glibc(&json!({
            "c0": resolved("x0", None),
            "c1": null,
            "c2": null,
            "c3": null,
        }))}),
        &[],
    );
    let none = refused(&none, GLIBC);
    assert!(
        none.contains(
            "no revision and path in release/2.34/master/NEWS resolve in \
             github.com/bminor/glibc"
        ),
        "{none}"
    );
}

#[test]
fn hex_and_tilde_split_prefixes_are_located_by_their_ref_lookups() {
    let prefix = envelope(
        &json!({"r0": repository("github.com/o/r", Some("main"), &json!({
            "c0": resolved("x0", Some("GitBlob")),
            "c1": resolved("x1", Some("GitTree")),
            "g1": {"nodes": []},
        }))}),
        &[],
    );
    let found = located(&prefix, "github.com/o/r/blob/main/v~1");
    assert_eq!(found.rev.as_deref(), Some("main"));
    assert_eq!(found.path.as_deref(), Some("v~1"));
    assert_eq!(found.at, at(&["r0", "c0"]));

    let alone = envelope(
        &json!({"r0": repository("github.com/o/r", Some("main"), &json!({
            "c0": resolved("x0", Some("GitBlob")),
            "g0": {"nodes": []},
            "c1": null,
        }))}),
        &[],
    );
    let found = located(&alone, "github.com/o/r/blob/abcd1234/f.go");
    assert_eq!(found.rev.as_deref(), Some("abcd1234"));
    assert_eq!(found.path.as_deref(), Some("f.go"));
}

#[test]
fn an_error_is_a_failed_lookup_not_a_missing_repository_or_revision() {
    let root = refused(&root_error(), "github.com/gorilla/mux");
    assert!(root.contains(LOOKUP), "{root}");
    assert!(!root.contains("internal git failure"), "{root}");

    let commit_error = envelope(
        &json!({"r0": mux(&json!({"c0": null}))}),
        &[failing(&json!(["r0", "c0"]))],
    );
    let commit = refused(&commit_error, "github.com/gorilla/mux@v1");
    assert!(commit.contains(REVISION_LOOKUP), "{commit}");
    assert!(!commit.contains("internal git failure"), "{commit}");

    let split_error = envelope(
        &json!({"r0": glibc(&json!({
            "c0": null, "c1": null, "c2": null, "c3": null,
        }))}),
        &[failing(&json!(["r0", "c1"]))],
    );
    let split = refused(&split_error, GLIBC);
    assert!(split.contains(REVISION_LOOKUP), "{split}");

    let resolved_beside_error = envelope(
        &json!({"r0": glibc(&json!({
            "c0": resolved("x0", None),
            "c1": null,
            "c2": resolved("x2", Some("GitBlob")),
            "c3": null,
        }))}),
        &[failing(&json!(["r0", "c1"]))],
    );
    let found = located(&resolved_beside_error, GLIBC);
    assert_eq!(found.rev.as_deref(), Some("release/2.34/master"));
}

#[test]
fn a_repository_without_commits_is_named_through_a_split_address() {
    let empty = envelope(
        &json!({"r0": repository(
            "github.com/bminor/glibc",
            None,
            &json!({"c0": null, "c1": null, "c2": null, "c3": null}),
        )}),
        &[],
    );
    let blocked = refused(&empty, GLIBC);
    assert!(blocked.contains("github.com/bminor/glibc has no commits"));
}

#[test]
fn canonical_name_falls_back_to_the_candidate() {
    let unnamed = || {
        json!({
            "defaultBranch": {"displayName": "main"},
            "c0": {"oid": SHA},
        })
    };
    let no_name = envelope(&json!({"r0": unnamed(), "r1": unnamed()}), &[]);
    assert_eq!(
        located(&no_name, "github.com/gorilla/mux@v1").repo,
        "github.com/gorilla/mux"
    );
    assert!(refused(&no_name, "gorilla/mux").contains(
        "gorilla/mux exists as github.com/gorilla/mux and \
         gitlab.com/gorilla/mux"
    ));
}

#[test]
fn an_errored_name_falls_back() {
    let errored = envelope(
        &json!({"r0": repository(
            "github.com/X/Y",
            Some("main"),
            &json!({"c0": {"oid": SHA}}),
        )}),
        &[failing(&json!(["r0", "name"]))],
    );
    assert_eq!(located(&errored, "github.com/x/y@v1").repo, "github.com/x/y");
}

#[test]
fn an_errored_oid_is_unresolved() {
    let errored = envelope(
        &json!({"r0": mux(&json!({"c0": {"oid": SHA}}))}),
        &[failing(&json!(["r0", "c0", "oid"]))],
    );
    let blocked = refused(&errored, "github.com/gorilla/mux@v1");
    assert!(blocked.contains(REVISION_LOOKUP), "{blocked}");
    assert!(!blocked.contains("not found"), "{blocked}");
}

#[test]
fn an_error_inside_a_null_commit_failed() {
    let errored = envelope(
        &json!({"r0": mux(&json!({"c0": null}))}),
        &[failing(&json!(["r0", "c0", "oid"]))],
    );
    let blocked = refused(&errored, "github.com/gorilla/mux@v1");
    assert!(blocked.contains(REVISION_LOOKUP), "{blocked}");
    assert!(!blocked.contains("not found"), "{blocked}");
}

#[test]
fn a_resolved_candidate_keeps_its_expected_errors() {
    let expected = envelope(
        &json!({"r0": glibc(&json!({
            "c0": {"oid": SHA, "meta": null, "x0": null},
            "c1": null,
            "c2": null,
            "c3": null,
        }))}),
        &[failing(&json!(["r0", "c0", "meta"]))],
    );
    let none = refused(&expected, GLIBC);
    assert!(
        none.contains(
            "no revision and path in release/2.34/master/NEWS resolve in \
             github.com/bminor/glibc"
        ),
        "{none}"
    );
}

#[test]
fn one_host_failing_is_not_one_host_existing() {
    let one_host = envelope(
        &json!({"r0": mux(&json!({"c0": {"oid": SHA}})), "r1": null}),
        &[failing(&json!(["r1"]))],
    );
    let blocked = refused(&one_host, "gorilla/mux");
    assert!(blocked.contains(LOOKUP), "{blocked}");
    assert!(!blocked.contains("internal git failure"), "{blocked}");
}

#[test]
fn failed_default_branch_is_not_an_empty_repository() {
    let single = envelope(
        &json!({"r0": repository(
            "github.com/ascoders/blog",
            None,
            &json!({"c0": null}),
        )}),
        &[failing(&json!(["r0", "defaultBranch"]))],
    );
    let single = refused(&single, "github.com/ascoders/blog");
    assert!(single.contains(LOOKUP), "{single}");

    let split = envelope(
        &json!({"r0": repository(
            "github.com/bminor/glibc",
            None,
            &json!({"c0": null, "c1": null, "c2": null, "c3": null}),
        )}),
        &[failing(&json!(["r0", "defaultBranch"]))],
    );
    let split = refused(&split, GLIBC);
    assert!(split.contains(LOOKUP), "{split}");
}

fn repo_refused(envelope: &Envelope, token: &str) -> String {
    match locate_repository(&repo_of(token), envelope, &repo_plan()) {
        Ok(found) => panic!("located {}", found.repo),
        Err(error) => rendered(&error),
    }
}

#[test]
fn repository_lookups_name_what_is_missing_or_failed() {
    let none = envelope(&json!({"r0": null, "r1": null}), &[]);
    let blocked = repo_refused(&none, "gorilla/mux");
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

    let both_hosts = envelope(
        &json!({
            "r0": mux(&json!({})),
            "r1": repository("gitlab.com/gorilla/mux", Some("master"), &json!({})),
        }),
        &[],
    );
    let both = repo_refused(&both_hosts, "gorilla/mux");
    assert!(
        both.contains(
            "gorilla/mux exists as github.com/gorilla/mux and \
             gitlab.com/gorilla/mux"
        ),
        "{both}"
    );

    let one_host = envelope(
        &json!({"r0": mux(&json!({})), "r1": null}),
        &[failing(&json!(["r1"]))],
    );
    let one = repo_refused(&one_host, "gorilla/mux");
    assert!(one.contains(LOOKUP), "{one}");
}

#[test]
fn a_null_root_with_errors_beneath_failed() {
    let null_root =
        |path: &Value| envelope(&json!({"r0": null}), &[failing(path)]);

    let tags = repo_refused(
        &null_root(&json!(["r0", "tags", "nodes"])),
        "github.com/gorilla/mux",
    );
    assert!(
        tags.contains("Sourcegraph failed to answer the tag list"),
        "{tags}"
    );
    assert!(!tags.contains("not on sourcegraph.com"), "{tags}");

    let branches = repo_refused(
        &null_root(&json!(["r0", "all_branches", "nodes", 3, "target"])),
        "github.com/gorilla/mux",
    );
    assert!(
        branches.contains("Sourcegraph failed to answer the branch list"),
        "{branches}"
    );

    for path in [json!(["r0"]), json!(["r0", "contributors"])] {
        let lookup = repo_refused(&null_root(&path), "github.com/gorilla/mux");
        assert!(lookup.contains(LOOKUP), "{path}: {lookup}");
    }

    let one_host_tags = envelope(
        &json!({"r0": mux(&json!({})), "r1": null}),
        &[failing(&json!(["r1", "tags", "nodes"]))],
    );
    let one = repo_refused(&one_host_tags, "gorilla/mux");
    assert!(
        one.contains("Sourcegraph failed to answer the tag list"),
        "{one}"
    );

    let both_null = envelope(
        &json!({"r0": null, "r1": null}),
        &[failing(&json!(["r1", "all_tags", "nodes"]))],
    );
    let both = repo_refused(&both_null, "gorilla/mux");
    assert!(both.contains(LOOKUP), "all_tags is not in this table: {both}");
}

#[test]
fn only_splits_look_up_refs() {
    for token in ["github.com/a/b@abcd1234:f", "github.com/a/b@main~1:f"] {
        let request = build(&toy(), &address(token));
        assert!(!request.document.contains("g0:"), "{}", request.document);
    }
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

#[test]
fn a_repository_request_builds_its_document_from_the_plan() {
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

    let with_tags = envelope(
        &json!({
            "r0": mux(&json!({"tags": {"totalCount": 3}})),
            "r1": null,
        }),
        &[],
    );
    let found =
        locate_repository(&repo_of("gorilla/mux"), &with_tags, &repo_plan())
            .unwrap_or_else(|e| panic!("{}", rendered(&e)));
    assert_eq!(found.repo, "github.com/gorilla/mux");
    assert_eq!(found.index, 0);
    assert_eq!(found.at("r"), "r0");

    let no_commits = envelope(
        &json!({"r0": repository(
            "github.com/ascoders/blog",
            None,
            &json!({"tags": {"totalCount": 0}}),
        )}),
        &[],
    );
    let empty = locate_repository(
        &repo_of("github.com/ascoders/blog"),
        &no_commits,
        &repo_plan(),
    )
    .unwrap_or_else(|e| panic!("{}", rendered(&e)));
    assert_eq!(empty.repo, "github.com/ascoders/blog");
    assert_eq!(empty.index, 0);
    assert_eq!(empty.at("r"), "r0");
}

#[test]
fn a_plan_with_named_roots_builds_a_document_with_a_root_per_name() {
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
    let second_root_error = envelope(
        &json!({
            "m0": mux(&json!({"commit": {"oid": SHA}})),
            "d0": null,
        }),
        &[failing(&json!(["d0", "comparison"]))],
    );
    let found = locate_repository(
        &repo_of("github.com/gorilla/mux"),
        &second_root_error,
        &named_plan(),
    )
    .unwrap_or_else(|e| panic!("{}", rendered(&e)));
    assert_eq!(
        found,
        RepoAt { repo: "github.com/gorilla/mux".to_owned(), index: 0 }
    );
    assert_eq!(found.at("m"), "m0");
    assert_eq!(found.at("d"), "d0");

    let first_root_error = envelope(
        &json!({
            "m0": null,
            "d0": mux(&json!({"commit": {"oid": SHA}})),
        }),
        &[failing(&json!(["m0", "commit"]))],
    );
    let blocked = match locate_repository(
        &repo_of("github.com/gorilla/mux"),
        &first_root_error,
        &named_plan(),
    ) {
        Ok(found) => panic!("located {}", found.repo),
        Err(error) => rendered(&error),
    };
    assert!(blocked.contains(LOOKUP), "{blocked}");
}
