# The answer goes to stdout, everything else to stderr, and the exit code says what to do next

The answer goes to stdout and nothing else does, so it can be piped into `head` or `jq` whole.
Notes go to stderr, written before the answer, so a harness that merges the two streams shows each
note above what it qualifies, and a pipe on stdout doesn't swallow it. An error goes to stderr with
stdout empty, even under `--json`, because an error document on stdout would vanish into
`| jq .field`. The exit code tells an agent its next move:

- **0:** it has an answer, even an empty one or a lower bound.
- **1:** what it named doesn't exist, so asking again won't help, but a different address might.
- **2:** the command line itself is wrong and must change.
- **3:** Sourcegraph or the network failed to answer, so asking again later might help.

sego says not found only when the server says a name doesn't exist. Any other server error is a
failure, so an agent is never told something is missing when sego merely couldn't look.
