set shell := ["bash", "-euo", "pipefail", "-c"]

# Install what the checks need and turn on the pre-commit hook.
setup:
    cargo insta --version 2>/dev/null | grep -qx 'cargo-insta 1.49.0' || cargo install --force cargo-insta --version 1.49.0 --locked
    git config core.hooksPath .githooks

# Format check, lints with warnings as errors, and the offline tests, never blessing and failing on a snapshot no test uses.
check:
    cargo fmt --all --check
    cargo clippy --locked --all-targets -- -D warnings
    cargo insta test --check --unreferenced=reject

# Write the snapshots of the tests matching FILTER from what sego does now, and print the diff.
[positional-arguments]
bless *filter:
    INSTA_UPDATE=force INSTA_OUTPUT=none cargo test -q --test it -- "$@"
    git add -N tests/it/snapshots
    git diff --stat --patch -- tests/it/snapshots

# Record the responses the tests lack, two requests a second, and remove the recordings they don't use; with a FILTER, record the matching tests' responses again. Bless afterwards.
[positional-arguments]
record *filter:
    export INSTA_UPDATE=no INSTA_FORCE_PASS=1 INSTA_OUTPUT=none; if [[ $# -gt 0 ]]; then SEGO_RECORD=again cargo test --test it -- --test-threads=1 "$@"; else rm -f target/recordings-used; SEGO_RECORD=1 cargo test --test it -- --test-threads=1; SEGO_RECORD=prune cargo test --test it -- --exact prune_recordings; fi

# Send the request of each recording whose name holds FILTER again; with no FILTER, also check search's live rules. Fail on drift.
[positional-arguments]
live filter="":
    SEGO_LIVE="$1" cargo test --no-fail-fast -- --ignored --test-threads=1 ${1:+--skip live_rules}

# Regenerate src/escape/table.rs from the pinned Unicode data.
escape-table:
    scripts/escape-table

# Fetch the full introspected schema into target/schema.json, for reading.
schema:
    scripts/schema
