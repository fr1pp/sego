# The query goes to the server exactly as written

sego never rewrites a query. It adds no `count:`, sentinel, `timeout:` or `patterntype:`, so the
server's defaults (a cap of 30 and a 20 s deadline) and the agent's own filters apply as Sourcegraph
documents them, and the query on an answer's second line is the one the server ran. Earlier versions
appended `count:100` and a sentinel. Those were dropped with the search-answers work of 2026-10-12.

Where the server answers a query silently and wrongly, with zero results, results from the wrong
place or too few, sego runs local checks that print a note naming the cause and never block the
query. They are tuned to fire only on a query that can't be a legitimate one. The exceptions are the
structural-search checks, which fire on legitimate queries the server can cut without saying so.
