# Contributing to sego

## Building and checking

sego builds with the stable toolchain named by `rust-version` in `Cargo.toml`.

```sh
cargo build --release   # target/release/sego
just check              # rustfmt, clippy with warnings denied, and every offline test
just ci                 # check, then fail if it left a tracked file changed
```

## Cases

Behaviour an agent can see is tested by cases
([ADR 0011](docs/adr/0011-tests-replay-recorded-responses.md)). A case is one TOML file in
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
query = '''
…
'''
status = 200
content-type = "application/json"
recorded = { date = 2026-10-07, endpoint = "https://sourcegraph.com" }
response = '''
{ … }
'''
```

- `env` sets `SEGO_ENDPOINT` or `SEGO_CACHE_DIR`.
- `stdout.closed = true` or `stderr.closed = true` runs the case with that pipe's reader gone.
- A raw read has `get = "<path>"` in place of the GraphQL request, and names its body with
  `body = "<file>.body"`, relative to the case's directory. Cases may share a body file.
- `live = "values"` in `recorded` makes `just live` compare the response's values as well as its
  shape.

Each case is a test in `tests/it/cases.rs`, so `cargo test --test it show::empty_file` runs one.
To add a case, write its `args` and `status.code`, then:

- **Record its responses** with `just record`. It sends sourcegraph.com each request that no
  exchange answers, and each whose recorded request changed, one a second.
- **Bless it** with `SEGO_BLESS=1 cargo test --test it`. Bless writes what the case expects from
  what sego printed, the request of each exchange that isn't recorded, and the list of cases. A new
  case needs two runs: the first adds it to the list.

Read the blessed diff before committing it.

A response sourcegraph.com can't be made to give on demand, such as a stopped search, is written by
hand: an `[[exchange]]` with its `status`, `content-type` and `response`, whose request bless fills
in.

## Live checks

`just live` re-sends every recorded request to sourcegraph.com and fails when a response's shape
has changed. A response marked `live = "values"` also fails when its values change. Run it after any
change to a request, and before a release.

## Generated files

- `just escape-table` regenerates `src/escape/table.rs` from the pinned Unicode data.
- `just schema` fetches the live GraphQL schema into `target/schema.json`, for reading.

## Decisions and terms

- **An ADR in `docs/adr/`** is needed for a decision that is hard to reverse, surprising without
  its context, and the result of a real trade-off. A change that goes against an ADR rewrites the
  ADR in the same change.
- **A GLOSSARY.md entry** is needed for a new domain term, in the same change that names it.
- The rules for code are in [CODING_STANDARDS.md](CODING_STANDARDS.md).

## Commits and pull requests

- A commit message is one imperative sentence that says what the change does.
- Each commit passes `just ci`.
- A pull request says what changed and why, and links its issue.
