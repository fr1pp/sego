set shell := ["bash", "-euo", "pipefail", "-c"]

# Format check, lints with warnings as errors, and the offline tests, never blessing.
check:
    cargo fmt --all --check
    cargo clippy --locked --all-targets -- -D warnings
    env -u SEGO_BLESS cargo test --locked

# Rewrite the case list, what each case expects and the stored documents from what sego does now, and print the diff.
bless:
    #!/usr/bin/env bash
    set -euo pipefail
    # The working tree, untracked files included, as a tree. It's built in an
    # index of its own, so what's staged is left alone.
    snapshot() { GIT_INDEX_FILE=target/bless.index git add -A && GIT_INDEX_FILE=target/bless.index git write-tree; }
    mkdir -p target
    if [[ -e target/bless.index.lock ]]; then
        echo "error: target/bless.index.lock exists: a bless is running, or one was stopped; if none is running, delete it" >&2
        exit 1
    fi
    before=$(snapshot)
    export SEGO_BLESS=1
    # A new case is a test only once the list names it, so the list is blessed first.
    status=0
    cargo test -q --test it -- --exact the_case_list_matches_tests_cases || status=$?
    if [[ $status -eq 0 ]]; then cargo test -q --test it || status=$?; fi
    after=$(snapshot)
    if git diff --quiet "$before" "$after"; then echo "bless changed nothing"; else git diff --stat --patch "$before" "$after"; fi
    exit "$status"

# Record the responses that cases lack, and those whose request changed, one request a second.
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
