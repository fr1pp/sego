# sego

Code search for agents, built on Sourcegraph.

sego searches and reads code on Sourcegraph: millions of public repositories, at any branch, tag or
commit, with no clone and no account or token. It gives an agent the actual code where it would
otherwise recall or guess.

It answers the questions an agent can name but can't answer from what it has: where something is
defined, who calls an API, where an error message comes from, what a dependency does at the version
in use, which commit changed a behaviour and why, what differs between two releases. Answers are
exact, say when they're cut short, and come as plain text or JSON that the agent's own tools take
straight in.

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

## License

sego is distributed under the terms of both the MIT license and the Apache License (Version 2.0).
See [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT) for details. Opening a pull
request is assumed to signal agreement with these licensing terms.
