# One GraphQL request per invocation

Each invocation sends one GraphQL request, with no retries, backoff or concurrency. What a command
needs at run time goes into that one document as aliases: `owner/name` asks for both
`github.com/owner/name` and `gitlab.com/owner/name`; a web URL with a slashed branch asks for one
`commit(rev:)` per split point; a search asks for the files the index skipped (ADR 0007). Parts that
can fail independently sit under separate top-level aliases, since a GraphQL error nulls its nearest
nullable parent.

The one exception is a raw read of a file over the content limit, which needs a second request.

The request runs under a 90 s client deadline (ADR 0004). When an answer would need a second
request, sego goes without it. For example, the indexed commit beside a pinned search added a median
of 0.50 s, and up to 14.68 s, when measured on 2026-10-18. So sego prints a cautious `+` instead.
