# Contributing to sego

## Building and checking

sego builds with the stable toolchain named by `rust-version` in `Cargo.toml`.

```sh
cargo build --release   # target/release/sego
just check              # rustfmt, clippy with warnings denied, and every offline test
```

## Cases

Behaviour an agent can see is tested by cases
([ADR 0010](docs/adr/0010-tests-replay-recorded-responses.md)). A case is one TOML file in
`tests/cases/<command>/`:

```toml
# What the case shows, in one sentence.
args = ["refs", "github.com/gorilla/mux", "v1.8"]
status.code = 0
stdout = '''
…
'''

[[exchange]]
operation = "Refs"
variables = '{"repo0":"github.com/gorilla/mux"}'
document = "Refs-fed5d5bc"
status = 200
content-type = "application/json"
recorded = { date = 2026-10-07, endpoint = "https://sourcegraph.com" }
response = '''
{ … }
'''
```

- `document` names the query's text, stored once in `tests/documents/` as
  `<operation>-<hash>.graphql`. Bless and record write new documents; bless deletes those no case
  names.
- `env` sets `SEGO_CACHE_DIR`.
- `stdout.closed = true` or `stderr.closed = true` runs the case with that pipe's reader gone.
- A raw read has `get = "<path>"` in place of the GraphQL request, and names its body with
  `body = "<file>.body"`, relative to the case's directory. Cases may share a body file.
- `live = "values"` in `recorded` makes `just live` compare the response's values as well as its
  shape.

Each case is a test in `tests/it/cases.rs`, so `cargo test --test it show::empty_file` runs one.
To add a case, write its `args` and `status.code`, then:

- **Record its responses** with `just record`. It sends sourcegraph.com each request that no
  exchange answers, and each whose recorded request changed, two a second.
- **Bless it** with `just bless`. Bless writes the list of cases, then what each case expects from
  what sego printed and the request of each exchange that isn't recorded, with its document, and
  deletes the documents no case names. It prints the diff of what it wrote, new files included.

Read that diff before committing it.

`just record` refuses a search that sego reports as stopped, because cut results vary from run to
run: run it again. A search can also come back silently short when a search backend fails to answer
([ADR 0015](docs/adr/0015-failed-search-backends-are-not-detected.md)), and only the blessed diff
shows that. A response sourcegraph.com can't be made to give on demand, such as a stopped search, is
written by hand: an `[[exchange]]` with its `status`, `content-type` and `response`, whose request
bless fills in. A hand-written response with a located GraphQL error names its `evidence`: a
recorded case with the same kind of error at the same kind of path
(`evidence = "show/binary-file-over-content-limit"`), `evidence = "GqlError::kind table: …"`, or
`evidence = "none: REASON"`.

Every case is also checked for:

- the exact sequence of requests its exchanges hold, with none left unused
- no request on a usage error, except the three found from a response
- no note or error line that ends in a period
- every request under the one client deadline
- `connect` given sourcegraph.com, at most once
- a cache directory left untouched unless a raw read was sent

Every case served a GraphQL response runs a second time with a hostile corpus in the strings of its
response ([ADR 0006](docs/adr/0006-escape-server-text.md)). It fails if an unsafe character reaches
the output as itself, a line starts with the corpus, or a JSON value carries only part of it. Cases
made for the purpose cover what that can't reach, such as alert titles, file contents and raw reads.

## Other tests

The case runner lives in `tests/it/support`, inside the one test binary `tests/it`, with its own
tests inline. Faults below HTTP, such as a dropped connection or a passed deadline, aren't cases:
they are module tests of the transport and of the `sourcegraph` module. Besides the cases, two tests
go through `run`: one runs the built binary, for what `main` adds, and one runs the cache across two
runs.

## Live checks

`just live` re-sends every recorded request to sourcegraph.com and fails when a response's shape
has changed. A response marked `live = "values"` also fails when its values change. Then it checks
the rules search relies on. `just live PATH…` re-sends only the requests of the cases under each
path, a case or a directory under `tests/cases` such as `show` or `show/lfs-pointer`, and skips the
rules. Run it whole before a release.

Before changing a command's requests, run `just live <command>`, or `just live` whole for search,
whose rules only the whole run checks. Once it passes, the diff that `just record` and `just bless`
then write is your change, not server drift.

A difference counts only when a second answer shows it too, since sourcegraph.com now and then
leaves out part of a search's answer. Three differences never count:

- a "revision not found after waiting for a fetch" error at a revision lookup that one answer has
  and the other doesn't, which another client's fetch can cause
- which of two missing names waited for the fetch they share
- the mirror's `updatedAt`, which moves on its own

## Generated files

- `just bless` regenerates `tests/it/cases.rs` from `tests/cases`, and what each case expects and
  the stored documents from what sego does now; see [Cases](#cases).
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

- A commit message is one imperative sentence that says what the change does.
- Each commit passes `just check`.
- A pull request says what changed and why, and links its issue.
