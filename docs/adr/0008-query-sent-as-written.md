# A query goes to the server exactly as written

sego sends the agent's query unchanged. It adds no `count:`, `timeout:` or `patterntype:`, so
Sourcegraph's documented defaults and the agent's own filters apply as Sourcegraph describes them,
and the query an answer prints is the query the server ran.
