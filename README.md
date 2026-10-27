# sego

A code search CLI for agents, built on Sourcegraph.

sego searches and reads code in the repositories Sourcegraph serves, at any branch, tag or commit,
with no clone. It talks only to Sourcegraph's public GraphQL API, so it needs no account or token.

## Install

```sh
cargo install --path .
```

sego needs the Rust toolchain named by `rust-version` in `Cargo.toml`.

## Examples

```sh
sego search 'repo:^github\.com/gorilla/mux$@v1.8.0 NewRouter'
sego show github.com/gorilla/mux@v1.8.0:mux.go 120-180
sego ls github.com/gorilla/mux@v1.8.0
sego log github.com/gorilla/mux:mux.go
sego diff github.com/gorilla/mux@v1.7.4...v1.8.0 mux.go
```

`sego --help` and `sego COMMAND --help` describe every command, option and field.

## Documentation

- [GLOSSARY.md](GLOSSARY.md): the terms sego and its docs use
- [docs/adr/](docs/adr/): the decisions behind sego's design
- [docs/code-search-reference.md](docs/code-search-reference.md): Sourcegraph's query language
- [CONTRIBUTING.md](CONTRIBUTING.md) and [CODING_STANDARDS.md](CODING_STANDARDS.md)
