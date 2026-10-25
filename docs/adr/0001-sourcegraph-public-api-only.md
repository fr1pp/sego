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
