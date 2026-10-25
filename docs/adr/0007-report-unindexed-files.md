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
