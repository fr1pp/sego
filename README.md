# sego

Code search for agents, built on Sourcegraph.

## Install

### Standalone installer

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/fr1pp/sego/releases/latest/download/sego-installer.sh | sh
```

### Cargo

```sh
cargo install --locked sego
```

## Commands

| Command | Answers with |
|---|---|
| `sego search QUERY...` | code matching a Sourcegraph query |
| `sego show REPO[@REV]:PATH [LINES]` | lines of a file |
| `sego show REPO@REV` | a commit: its message and changed files |
| `sego ls REPO[@REV][:PATH]` | entries of a directory |
| `sego refs REPO [TEXT]` | branches and tags |
| `sego log REPO[@REV][:PATH] [PATH]` | commits, in git log order |
| `sego log REPO@A..B [PATH]` | commits in B and not in A |
| `sego diff REPO@A...B [PATH...]` | changed files, and the patch of named paths |
| `sego diff REPO@REV [PATH...]` | the change one commit made |

`REPO` is `host/owner/name` or `owner/name`, `@REV` is a branch, tag or commit, and every command
takes `--json`. `sego --help` and `sego COMMAND --help` describe every command, option and field.

## Affiliation

sego is an independent project, not affiliated with or endorsed by Sourcegraph.

## License

sego is distributed under the terms of both the MIT license and the Apache License (Version 2.0).
See [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT) for details. Opening a pull
request is assumed to signal agreement with these licensing terms.
