# A search reports the files the index skipped

The index skips files over 1 MiB and files with too many distinct trigrams, and no field of the
response says so. In about 800 live probes against `git grep` ground truth, such files were missing
from 10 of 26 answers that looked complete. Sourcegraph's MCP server, grep.app's MCP server and
`src` stay silent about them too.

So when a query is eligible (scoped by `repo:`, at `HEAD`, with nothing the side query can't
restate), sego adds a second alias to the same request. It asks the index for its `NOT-INDEXED`
placeholders in the query's scope, under the same deadline. The answer names those files. When the
fact is unknown, a zero or complete answer carries a fixed note saying which files are never
searched.

An empty listing is unknown too, and JSON `unindexed` is `null`. A side query that failed under load
comes back empty, just as one in a repository with nothing skipped does: in 9 of 16 kubernetes runs
under load on 2026-10-26 it came back empty while 14 files were skipped. So the fixed note also
appears on scoped searches of repositories that skip nothing.

The side query has a cost. On golang/go under load its median went from 1.3 s to 3.2 s for the
whole request.
