# sego

A code search CLI for agents, built on Sourcegraph.

## Before changing code

Read [GLOSSARY.md](GLOSSARY.md) for the domain's terms, the ADRs in `docs/adr/` that touch the
area, and [CODING_STANDARDS.md](CODING_STANDARDS.md). [CONTRIBUTING.md](CONTRIBUTING.md) covers
cases, bless, recording and live checks.

## Commands

- `just check`: rustfmt, clippy and the offline tests. Run it before every commit.
- `just ci`: `just check` on a clean tree, failing if a tracked file changed.
- `SEGO_BLESS=1 cargo test --test it`: rewrite what the cases expect and the list of cases, then
  read the diff.
- `just record` and `just live` talk to sourcegraph.com, one request a second. Run one at a time;
  sourcegraph.com blocks bursts.

## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues on `fr1pp/sego`, via the `gh` CLI. See
`docs/agents/issue-tracker.md`.

### Triage labels

The default label strings: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`,
`wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `GLOSSARY.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.
