# sego

A code search CLI for agents, built on Sourcegraph.

## Before changing code

Read [GLOSSARY.md](GLOSSARY.md) for the domain's terms, the ADRs in `docs/adr/` that touch the
area, and [CODING_STANDARDS.md](CODING_STANDARDS.md). [CONTRIBUTING.md](CONTRIBUTING.md) covers
tests, bless, recording and live checks. Before relying on how sourcegraph.com behaves, check
`docs/research/`, which holds what has been established, with its sources.

## Commands

- `just setup`: install `cargo-insta` and turn on the pre-commit hook, once per clone.
- `just check`: rustfmt, clippy and the offline tests, failing on a snapshot no test uses. Run it
  before every commit.
- `just bless [FILTER]`: write the snapshots; read the diff it prints.
- `just record [FILTER]`: record the requests that have no recording and remove those no case uses;
  with FILTER, record the matching tests' responses again.
- `just live [FILTER]`: re-send each recording's request, and with no FILTER check the live rules;
  fail on drift.
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
