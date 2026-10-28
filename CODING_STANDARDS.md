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
  ([ADR 0011](docs/adr/0011-tests-replay-recorded-responses.md)). A module test is kept only where
  [ADR 0012](docs/adr/0012-tests-sit-inline-or-go-through-run.md) allows one.
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
- Every string from the server is escaped before it is printed
  ([ADR 0006](docs/adr/0006-escape-server-text.md)).

## Errors

- Every error is an `Error`, and its kind decides the exit code
  ([ADR 0005](docs/adr/0005-streams-and-exit-codes.md)).

## Visibility

- An item is private, or `pub` inside a private module. The crate's public items are the ones
  [ADR 0012](docs/adr/0012-tests-sit-inline-or-go-through-run.md) lists.
- `pub(crate)` and `pub(super)` aren't used, and no item is made public for a test.

## Comments

- Code carries no comment by default.
- A comment is written for a why, an invariant, a landmine, a ticket link or a toolchain pragma.
- A comment describes the code as it is now. A comment that only records how the code changed is
  deleted.

## Formatting and lints

- `cargo fmt` with rustfmt's defaults plus `use_small_heuristics = "Max"`.
- The lints are the `[lints]` table in `Cargo.toml`, and `just check` denies every warning.
