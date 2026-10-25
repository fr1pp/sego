# Behaviour is tested against recorded responses, with a live drift check

Most behaviour is tested by case files (`tests/cases/<command>/*.case`): the command line, exit
code, stdout and stderr, with no Rust. A runner drives the built binary against a replay server of
recorded responses (`tests/fixtures/`), reached through `SEGO_ENDPOINT`. A fixture whose recorded
query differs from the one sent fails as stale. Fixtures come only from pinned revisions and from
searches that ran to completion, because cut results vary from run to run.

`just live` re-sends every fixture's request to sourcegraph.com and fails when the response's shape
changes or new errors appear. Because those requests are the operations sego sends, this is also the
schema check, and sego needs no GraphQL validator.
