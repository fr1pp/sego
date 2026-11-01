# Contributing to sego

## Building and checking

`rust-toolchain.toml` names the toolchain sego is developed with, and rustup fetches it.
`rust-version` in `Cargo.toml` is the oldest compiler that builds sego; bump the two together.

```sh
just setup              # once: install cargo-insta and turn on the pre-commit hook
cargo build --release   # target/release/sego
just check              # rustfmt, clippy with warnings denied, and every offline test
```

## Tests

Behaviour an agent can see is tested by ordinary `#[test]` functions in `tests/it/<command>.rs`
([ADR 0010](docs/adr/0010-tests-replay-recorded-responses.md)), each pinning what sego prints with an
[insta](https://insta.rs) snapshot:

```rust
/// A single line.
#[test]
fn single_line() {
    assert_snapshot!(sego(&["show", "github.com/gorilla/mux@v1.8.0:mux.go", "10"]));
}
```

Keep `assert_snapshot!` in the test itself: inside a helper function, snapshots are named after the
helper. `cargo test --test it show::single_line` runs one test.

A snapshot in `tests/it/snapshots/` holds the exit code, stdout and stderr, written by `just bless`.
A JSON answer is laid out there with its keys sorted, so that a diff shows the field that changed, so
each command also pins the exact text of one `--json` answer with `assert_eq!`. Read the snapshot's
diff before committing it.

`sego()` answers from `tests/recordings/`, which `just record` generates and nobody edits: one JSON
file per distinct request, named for a hash of the request. Tests that send the same request share
its recording, and a request with no recording fails the test. So to add a test:

1. Write it.
2. **Record its responses** with `just record`. It sends sourcegraph.com each request that has no
   recording, two a second, writes only recordings, and removes those no test uses, which nothing
   else notices. With a filter, it records the matching tests' responses again.
3. **Bless it** with `just bless`, which takes the same filter and prints the diff.

`just record` refuses a search that sego reports as stopped, because cut results vary from run to
run: run it again. A search can also come back silently short when a search backend fails to answer
([ADR 0015](docs/adr/0015-failed-search-backends-are-not-detected.md)), and only the blessed diff
shows that.

A response sourcegraph.com can't be made to give on demand, such as a stopped search, goes in the
test. `answered()` serves its responses in order, whatever is asked, and any left unused fails the
test:

```rust
#[test]
fn server_failure() {
    let response = ServerResponse::new(502, "text/html", "<html>…</html>");
    assert_snapshot!(answered(&["search", "foo"], [response]));
}
```

A `serde_json::Value` is a successful JSON response, so `json!` covers most. A response of hundreds
of entries is built in a loop, and its test checks the count and the edges of the answer, as in
`tests/it/limits.rs`. `Case` runs what those two can't, such as a closed pipe:
`Case::new(&args, responses).stdout_closed().ran()`.

These all run through the rules every test keeps, and a test that breaks one panics before its
snapshot is compared or written, so bless can't save a broken run:

- no request on a usage error, except the three found from a response
- no note or error line that ends in a period
- every request under the one client deadline
- `connect` given sourcegraph.com, at most once
- a cache directory left untouched unless a raw read was sent
- no response left unused, and no request without one
- output that's UTF-8 with no carriage return, the two things a snapshot can't pin
- a JSON answer on one line

Every test served a GraphQL response runs a second time with a corpus of characters that must never
print as themselves in the strings of its response
([ADR 0006](docs/adr/0006-escape-server-text.md)). It fails if an unescaped character reaches the
output, a line starts with the corpus, or a JSON value carries only part of it. Tests made for the
purpose cover what that can't reach, such as alert titles, file contents and raw reads.

`just check` fails on a snapshot no test uses, as a renamed test leaves behind. Never pass
`--unreferenced=delete` with a test filter: it deletes the snapshots of every test that didn't run.

## Other tests

What the tests run sego through lives in `tests/it/common`, inside the one test binary `tests/it`,
with its own tests inline. Faults below HTTP, such as a dropped connection or a passed deadline, are
module tests of the transport and of the `sourcegraph` module. One more test runs the built binary,
for what `main` adds, and `tests/it/cache.rs` runs the cache across two runs.

## Live checks

`just live` re-sends the request of every recording to sourcegraph.com, and fails at the end, listing
each recording whose response's shape has changed. Its values must match too, unless the commit IDs in the
answer have moved, since answers about the same commits should be the same. Capped searches and lists
of refs compare only their shape: which results fill a cap varies, and refs grow without a commit
moving. It also checks the rules search relies on, each its own test, one at a time. `just live
FILTER` sends only the recordings whose names hold FILTER, such as `File-` for the `File` requests,
and skips those rules, so a filter that matches nothing passes. Run it whole before a release. Every
recording goes through one gate, two requests a second, and stops once the server
refuses one. Nothing it runs writes a file.

Before changing a command's requests, run `just live` for those requests. Once it passes, the diff
that `just record` and `just bless` then write is your change, not server drift. When a recording has
really drifted, `just record FILTER` records the matching tests' responses again.

A difference counts only when a second answer, a few seconds later, shows it too, since
sourcegraph.com now and then leaves out part of a search's answer. Four differences never count:

- a "revision not found after waiting for a fetch" error at a revision lookup that one answer has
  and the other doesn't, which another client's fetch can cause
- which of two missing names waited for the fetch they share
- the mirror's `updatedAt`, which moves on its own
- the order of a search's results, which is whatever order the files were read in

## Generated files

- `just bless` writes `tests/it/snapshots/` from what sego does now; see [Tests](#tests).
- `just record` regenerates `tests/recordings/`.
- `just escape-table` regenerates `src/escape/table.rs` from the pinned Unicode data.
- `just schema` fetches the live GraphQL schema into `target/schema.json`, for reading.

## Research

`docs/research/` holds what has been established about how sourcegraph.com behaves, one file per
topic.

- A new finding goes into its topic's file, or starts a new topic; a file is never dated or
  duplicated.
- Each claim carries its own evidence: a citation to source code at a commit
  (`repo@commit:path:line`), a docs path, or a live check written out as its query, its UTC date
  and the values that came back.
- Nothing but the Markdown files lives there: no scripts, logs or raw responses. A claim checked
  again gets its new date and values in place.

What the research means for how to search well goes in
[docs/search-strategy.md](docs/search-strategy.md).

## Terms and standards

- **A GLOSSARY.md entry** is needed for a new domain term, in the same change that names it.
- The rules for code are in [CODING_STANDARDS.md](CODING_STANDARDS.md).

## Commits and pull requests

- `just setup` makes each commit run `just check` first.
- A commit message is one imperative sentence that says what the change does.
- Each commit passes `just check`.
- A pull request says what changed and why, and links its issue.

## Releases

`dist` builds a release from a tag: the archives, their checksums, the installer and the GitHub
Release. It then publishes the crate, through trusted publishing, so a release needs no token; if
that job fails, the GitHub Release is already live, so re-run the job.
`.github/workflows/release.yml` is generated from `dist-workspace.toml`, so change that file
and run `dist generate`; `dist` comes from its GitHub releases, since crates.io is behind.

1. Run `just live`.
2. In a pull request, set `version` in `Cargo.toml`, run `cargo check` to update `Cargo.lock`, add the
   version's section to `CHANGELOG.md`, and run `just bless cli::version`.
3. Once it's merged, run `git fetch && git tag vX.Y.Z origin/main && git push origin vX.Y.Z`.
