use std::path::Path;

use super::{every_case, exchanges_used, requests_match};
use crate::case::{Case, Request};
use crate::run::Outcome;

const QUERY: &str = "query Refs($repo0: String!) {\n  r0: repository(name: \
                     $repo0) { name }\n}\n";

fn case(response: &str) -> Case {
    let text = format!(
        "args: refs example.com/a/b\nexit: 0\n--- request Refs \
         {{\"repo0\":\"example.com/a/b\"}}\n--- query\n{QUERY}--- response \
         {response}\n{{}}\n"
    );
    Case::parse(&text, Path::new("refs/x.case"))
        .unwrap_or_else(|e| panic!("{e}"))
}

fn refs(repo: &str, query: &str) -> Request {
    Request::Graphql {
        operation: "Refs".to_owned(),
        variables: serde_json::json!({"repo0": repo}),
        query: query.to_owned(),
    }
}

fn outcome(sent: Vec<Request>) -> Outcome {
    Outcome {
        exit: 0,
        stdout: String::new(),
        stderr: String::new(),
        sent,
        problems: Vec::new(),
    }
}

#[test]
fn a_request_that_differs_from_its_exchange_fails_with_both_texts() {
    let case = case("200 application/json");
    let changed = "query Refs($repo0: String!) { r0: repository(name: \
                   $repo0) { name defaultBranch { displayName } } }";
    let failures = requests_match(
        &case,
        &outcome(vec![refs("example.com/a/b", changed)]),
    );
    let [failure] = &failures[..] else { panic!("{failures:?}") };
    assert!(failure.contains("refs/x.case"), "{failure}");
    assert!(failure.contains("--- written\nRefs"), "{failure}");
    assert!(failure.contains("--- sent\nRefs"), "{failure}");
    assert!(failure.contains("defaultBranch"), "{failure}");
    assert!(!failure.contains("just record"), "{failure}");
}

#[test]
fn a_recorded_response_for_a_changed_request_points_to_just_record() {
    let case = case("200 application/json recorded 2026-10-07 https://x");
    let failures =
        requests_match(&case, &outcome(vec![refs("example.com/c/d", QUERY)]));
    let [failure] = &failures[..] else { panic!("{failures:?}") };
    assert!(failure.contains("just record"), "{failure}");
}

#[test]
fn a_request_that_differs_only_in_whitespace_is_the_one_written() {
    let case = case("200 application/json");
    let spaced = "query Refs($repo0: String!)\n\n{ r0:   repository(name: \
                  $repo0) { name }\n}";
    assert_eq!(
        requests_match(&case, &outcome(vec![refs("example.com/a/b", spaced)])),
        Vec::<String>::new()
    );
}

#[test]
fn a_request_with_no_exchange_left_fails_naming_the_case_and_the_request() {
    let case = case("200 application/json");
    let sent =
        vec![refs("example.com/a/b", QUERY), refs("example.com/e/f", QUERY)];
    let failures = exchanges_used(&case, &outcome(sent));
    let [failure] = &failures[..] else { panic!("{failures:?}") };
    assert!(failure.contains("refs/x.case"), "{failure}");
    assert!(failure.contains("request 2"), "{failure}");
    assert!(failure.contains("example.com/e/f"), "{failure}");
}

#[test]
fn an_exchange_that_goes_unused_fails_the_case() {
    let case = case("200 application/json");
    let failures = exchanges_used(&case, &outcome(Vec::new()));
    let [failure] = &failures[..] else { panic!("{failures:?}") };
    assert!(failure.contains("1 of 1 exchanges went unused"), "{failure}");
}

fn raw() -> Request {
    Request::Raw { method: "GET".to_owned(), path: "/r/x/-/raw/f".to_owned() }
}

fn stopped_with(exit: i32, stderr: &str, sent: Vec<Request>) -> Vec<String> {
    every_case(&Outcome { exit, stderr: stderr.to_owned(), ..outcome(sent) })
}

fn noted(stderr: &str) -> Vec<String> {
    stopped_with(0, stderr, Vec::new())
}

#[test]
fn one_graphql_request_and_one_raw_read_are_allowed() {
    let sent = vec![refs("example.com/a/b", QUERY), raw()];
    assert_eq!(stopped_with(0, "", sent), Vec::<String>::new());
}

#[test]
fn a_second_graphql_request_fails_the_case() {
    let sent =
        vec![refs("example.com/a/b", QUERY), refs("example.com/c/d", QUERY)];
    let failures = stopped_with(0, "", sent);
    let [failure] = &failures[..] else { panic!("{failures:?}") };
    assert!(
        failure.contains("2 GraphQL requests and 0 raw reads"),
        "{failure}"
    );
}

#[test]
fn a_second_raw_read_fails_the_case() {
    let failures = stopped_with(0, "", vec![raw(), raw()]);
    let [failure] = &failures[..] else { panic!("{failures:?}") };
    assert!(
        failure.contains("0 GraphQL requests and 2 raw reads"),
        "{failure}"
    );
}

#[test]
fn a_request_on_a_usage_error_fails_the_case() {
    let failures = stopped_with(2, "", vec![refs("example.com/a/b", QUERY)]);
    let [failure] = &failures[..] else { panic!("{failures:?}") };
    assert!(
        failure.contains("reached the server on a usage error"),
        "{failure}"
    );
    assert_eq!(stopped_with(2, "", Vec::new()), Vec::<String>::new());
    assert_eq!(
        stopped_with(1, "", vec![refs("example.com/a/b", QUERY)]),
        Vec::<String>::new()
    );
}

#[test]
fn a_note_or_error_line_ending_in_a_period_fails_the_case() {
    for stderr in [
        "sego: note: tags marked from the 1000 newest.\n",
        "sego: error: no such file.\n",
        "  repo:, lang: or file: narrows what the search reads.\n",
    ] {
        let failures = noted(stderr);
        let [failure] = &failures[..] else {
            panic!("{stderr:?}: {failures:?}")
        };
        assert!(failure.contains("ends with a period"), "{failure}");
    }
    assert_eq!(noted("sego: note: a\nsego: error: b.\n  c\n").len(), 1);
}

#[test]
fn a_line_that_ends_in_an_ellipsis_or_starts_in_uppercase_is_not_checked() {
    assert_eq!(
        noted(
            "sego: note: the rest follows...\nsego: error: Sourcegraph says \
             no.\n  Not found.\n"
        ),
        Vec::<String>::new()
    );
}
