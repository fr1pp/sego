use std::path::Path;

use super::run_case;
use crate::case::Case;

#[test]
fn each_run_of_a_case_gets_a_cache_directory_of_its_own() {
    let case = Case::load(Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/cases/show/raw-read-over-content-limit.case"
    )))
    .unwrap_or_else(|e| panic!("{e}"));
    for run in 1..=2 {
        let outcome = run_case(&case).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            outcome.sent.len(),
            2,
            "run {run} sent {} requests; a raw read cached by an earlier run \
             skips the second",
            outcome.sent.len()
        );
    }
}
