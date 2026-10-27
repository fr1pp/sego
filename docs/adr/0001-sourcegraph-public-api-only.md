# sego answers only through sourcegraph.com's public GraphQL API

sego reads code through Sourcegraph's public GraphQL API at sourcegraph.com, with no token and no
setup, so an agent can use it the moment it is installed. It has no other upstream: no GitHub or
GitLab API, no clone, no Sourcegraph streaming search API. Blame, code navigation and package-name
resolution are out of scope.

## Consequences

- Only repositories sourcegraph.com indexes can be read: those on github.com, gitlab.com and a few
  other hosts. A renamed repository appears only under its new name.
- sego never requests a field that needs a token (`Repository.externalServices`,
  `recordedCommands`, `codeIntelSummary`) or `Usage.dataSource`: requesting one nulls the whole
  repository.
- `SEGO_ENDPOINT` names another endpoint. It exists for the tests, not to support other servers.
- GraphQL reports no search backend that fails to answer, so a search answering 0 carries a note
  saying a 0 can come from one. The streaming API reports it, but on 2026-10-27, after about 110
  stream requests, sourcegraph.com answered it with a Cloudflare challenge (HTTP 429,
  `cf-mitigated: challenge`), which no CLI can pass, while GraphQL kept answering.
- The raw route was challenged the same way minutes later. So sego reads it only for what GraphQL
  can't carry, and where GraphQL's text is already in hand, a failed raw read degrades to a note
  (ADR 0014).
