set shell := ["bash", "-euo", "pipefail", "-c"]

# Format check, lints with warnings as errors, and the offline tests, never blessing.
check:
    cargo fmt --all --check
    cargo clippy --locked --all-targets -- -D warnings
    env -u SEGO_BLESS cargo test --locked

# Rewrite the case list, what each case expects and the stored documents from what sego does now, and print the diff.
bless:
    scripts/bless

# Record the responses that cases lack, and those whose request changed, two requests a second.
record:
    cargo test --test it -- --ignored --exact missing_and_stale_responses_are_recorded

# Re-send the recorded requests of the cases under each PATH in tests/cases, or of every case and then search's live rules; fail on drift.
[positional-arguments]
live *paths:
    SEGO_LIVE_PATHS="$*" cargo test --test it -- --ignored --exact recorded_responses_still_match_sourcegraph_com
    if [[ $# -eq 0 ]]; then cargo test --lib -- --ignored --test-threads=1; fi

# Regenerate src/escape/table.rs from the pinned Unicode data.
escape-table:
    scripts/escape-table

# Fetch the full introspected schema into target/schema.json, for reading.
schema:
    scripts/schema
