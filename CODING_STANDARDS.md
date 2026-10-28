# Coding standards

The rules every change to sego follows. Where an ADR holds the reason, the rule links to it.

## Layout

- A module with children is `foo.rs` beside a `foo/` directory, never `foo/mod.rs`. A directory
  exists only for a module that has child modules. `clippy::mod_module_files` enforces this.
- Each subcommand is one module under `src/commands/`.
- Everything sego asks Sourcegraph goes through `src/sourcegraph.rs` and its children
  ([ADR 0013](docs/adr/0013-sourcegraph-module-owns-every-graphql-document.md)).

## Tests

- A module's unit tests sit inline, as a `#[cfg(test)] mod tests { use super::*; … }` at the end of
  its file, whatever the file's size.
- What an agent sees on the command line is pinned by a case
  ([ADR 0011](docs/adr/0011-tests-replay-recorded-responses.md)). A help text is pinned only by its
  case.
- A module test is kept only where a case can't pin the behaviour well: a pure function with a large
  input space (query reading, argument parsing, addresses, escaping), the reading of server
  responses with their combinations of errors, and what the command line can't reach (the cache's
  file handling, the HTTP transport). A module test that repeats a case is deleted.
- A test's name is a sentence that states the behaviour: `a_hit_needs_the_exact_size`.

## Names

- Domain names are the terms in [GLOSSARY.md](GLOSSARY.md). A word on a term's _Avoid_ list is used
  for nothing, in code or text.
- Response types, and the `server` object of search's JSON, keep Sourcegraph's own field names.

## Messages

- A note or error is lowercase and has no trailing period.
- It states a fact or a pointer
  ([ADR 0003](docs/adr/0003-facts-not-advice.md)).
- A note says something about its answer that the answer's fields don't, or points to what gets
  what the answer left out. A fact that holds whatever the answer, such as which files no search
  reads, goes in the help.
- A pointer gives the command or filter itself, worked out from the answer, and appears only where
  it holds for that answer.
- The help states the action to take, never the wrong form to avoid.
- Every string from the server is escaped before it is printed
  ([ADR 0006](docs/adr/0006-escape-server-text.md)).

## JSON

- Every fact a note states is also in the document's `notes` array
  ([ADR 0015](docs/adr/0015-json-carries-every-fact-text-prints.md)).
- A list that can be cut carries `X_total`, `null` when the server doesn't give it, and `X_more`,
  whether more exists than the list holds. `X_more` reports what the server sent, so where JSON
  asks for the whole list it is `false` unless the server sent less than its total. A search
  keeps `count`, `at_least` and `stopped`, its own form of the rule.
- A field name means one thing in every command's document. An `ls` entry's `kind` is the word its
  text row prints (`dir`, `file` or `submodule`), while a document's `kind` names what the address
  named (`directory`, `file`, `submodule` or `commit`).

## Errors

- Every error is an `Error`, and its kind decides the exit code
  ([ADR 0005](docs/adr/0005-streams-and-exit-codes.md)).

## Visibility

- An item is private, or `pub` inside a private module. The crate's public items are `run`, what it
  takes (`Env`, `Endpoint`, `Transport`, `Response`, `TransportError`) and `connect`
  ([ADR 0012](docs/adr/0012-library-exports-only-run.md)).
- `pub(crate)` and `pub(super)` aren't used, and no item is made public for a test.

## Comments

- Code carries no comment by default.
- A comment is written for a why, an invariant, a landmine, a ticket link or a toolchain pragma.
- A comment describes the code as it is now. A comment that only records how the code changed is
  deleted.

## Formatting and lints

- `cargo fmt` with rustfmt's defaults plus `use_small_heuristics = "Max"`.
- The lints are the `[lints]` table in `Cargo.toml`, and `just check` denies every warning.
