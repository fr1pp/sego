//! The integration tests: one test per case in `tests/cases`, the built
//! binary, and the cache across two runs.

mod binary;
mod cache;
mod cases;
mod support;

use std::fs;
use std::path::Path;

#[test]
fn the_case_list_matches_tests_cases() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/it/cases.rs");
    let list = support::case_list(&support::cases_dir()).unwrap_or_else(|e| panic!("{e}"));
    if support::blessing() {
        fs::write(&path, &list).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    } else {
        let written = fs::read_to_string(&path).unwrap_or_default();
        assert!(written == list, "tests/it/cases.rs is out of date; run with SEGO_BLESS=1");
    }
}

#[test]
fn every_document_is_named_by_a_case() {
    let unnamed = support::unnamed_documents().unwrap_or_else(|e| panic!("{e}"));
    assert!(
        unnamed.is_empty() || support::blessing(),
        "no case names {}; run with SEGO_BLESS=1",
        unnamed.join(", ")
    );
}

#[test]
#[ignore = "sends requests to sourcegraph.com; run with `just record`"]
fn missing_and_stale_responses_are_recorded() {
    let failures = support::record::every_case();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "sends requests to sourcegraph.com; run with `just live`"]
fn recorded_responses_still_match_sourcegraph_com() {
    let failures = support::live::every_case();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "runs sego against sourcegraph.com; run with `just live e2e`"]
fn answers_still_match_sourcegraph_com() {
    let commands: Vec<String> = std::env::var("SEGO_E2E_COMMANDS")
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let failures = support::live::every_answer(&commands);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
