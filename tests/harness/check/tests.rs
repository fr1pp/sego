use std::path::Path;

use super::{exchanges_used, requests_match};
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
