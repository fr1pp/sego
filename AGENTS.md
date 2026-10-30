# sego

A code search CLI for agents, built on Sourcegraph.

## Before changing code

Read [GLOSSARY.md](GLOSSARY.md) for the domain's terms, the ADRs in `docs/adr/` that touch the
area, and [CODING_STANDARDS.md](CODING_STANDARDS.md). [CONTRIBUTING.md](CONTRIBUTING.md) covers
cases, bless, recording and live checks. Before relying on how sourcegraph.com behaves, check
`docs/research/`, which holds what has been established, with its sources.

## Commands

- `just check`: rustfmt, clippy and the offline tests. Run it before every commit.
- `just bless`: rewrite the case list, what cases expect and the documents; read the diff it prints.
- `just record`: record from sourcegraph.com the responses cases lack or whose request changed.
- `just live [PATH…]`: re-send recorded requests and fail on drift; PATH as in `show/lfs-pointer`.
- `just escape-table`: regenerate `src/escape/table.rs` from the pinned Unicode data.
- `just schema`: fetch sourcegraph.com's full GraphQL schema into `target/schema.json`.

## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues on `fr1pp/sego`, via the `gh` CLI. See
`docs/agents/issue-tracker.md`.

### Triage labels

The default label strings: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`,
`wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `GLOSSARY.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.
