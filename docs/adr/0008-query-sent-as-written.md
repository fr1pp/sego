# A query goes to the server as written

sego sends the agent's query unchanged but for one addition: the longest deadline, when the query
sets none ([ADR 0017](0017-every-search-gets-the-longest-deadline.md)). It adds no `count:` or
`patterntype:`, so Sourcegraph's documented defaults and the agent's own filters apply as
Sourcegraph describes them, and the query an answer prints is the query the server ran, the added
`timeout:` included.
