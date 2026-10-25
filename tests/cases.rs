//! One test per case file in `tests/cases`; see the `harness` crate.

datatest_stable::harness! {
    { test = harness::run_case, root = "tests/cases", pattern = r"\.case$" },
}
