set shell := ["bash", "-euo", "pipefail", "-c"]

# Format check, lints with warnings as errors, and the offline tests.
check:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo test

# `check` with snapshot updates refused, then fail if anything tracked changed.
ci:
    if [[ -n "${UPDATE_EXPECT:-}" ]]; then echo "error: unset UPDATE_EXPECT for ci" >&2; exit 1; fi
    just check
    git diff --exit-code
