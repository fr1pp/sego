# sego

Code search for agents, built on Sourcegraph.

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
