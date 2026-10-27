# sego plans each query as the server does

Several of sego's rules depend on the basic queries the server runs for a query, and on their
filters. Examples are a filter written beside one operand of `or` (one such query matched 1,553
repositories instead of 1), structural searches the server cuts without saying so, and `index:no`.
sego computes that plan locally, written from a behavioural description of the server, without its
source in view.

`src/commands/search/plan/examples.jsonl` holds 1,825 queries with the plans a Sourcegraph server
(2024-08-22) gives them, and sego's plan must match every one. Where sourcegraph.com has since
changed, its rows are corrected against the live server. On 2026-10-25 every rejected row was sent
again, and one rule had changed: an unknown escape in a quoted value is kept as written, not
rejected. `just live` also checks a sample
against sourcegraph.com's `parseSearchQuery`.

The plan is computed locally because a search sends one request (ADR 0002). Even as a
check, `parseSearchQuery` can't give keyword mode's plan: it reads a query sent as keyword as
literal. Keyword is the mode sego sends in.
