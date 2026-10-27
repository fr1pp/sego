# The answer on stdout, notes and errors on stderr, and four exit codes

The answer goes to stdout. Notes go to stderr, written before the answer, so that Claude Code, which
merges the streams in write order, shows a note above the data it qualifies, and so that notes stay
visible when stdout goes through `head` or `jq`. Errors go to stderr with stdout empty, under `--json`
too, because a JSON error document on stdout would vanish into `| jq .field`.

The exit code tells an agent what to do next:

- **0, answered**, including an empty answer or one that is a lower bound.
- **1, not found:** the command line is well formed and unambiguous, but what it names doesn't
  exist on Sourcegraph: a repository, a revision, a path, any commit in an empty repository, or a
  root commit's diff. Asking again won't help; changing the address may.
- **2, usage error:** the command line itself must change. It is malformed, names something no
  mirror can hold (a reflog or upstream revision, a path that leaves the root), or is ambiguous.
  Most usage errors are found before sending anything. Three are found from the response, and carry
  their command's synopsis like the rest: LINES given for a directory, a regular expression the
  server can't compile, and `owner/name` that exists on both github.com and gitlab.com.
- **3, failure:** Sourcegraph or the network didn't answer: the server failed or refused, the
  connection failed, or the client deadline passed. Asking again later may help. A write to stdout
  or stderr that fails, other than to a closed pipe, exits 3 too: the answer didn't reach its
  reader.

Exit 1 once covered both "doesn't exist" and "the server failed", so an agent couldn't tell when
asking again might help.

Server errors are sorted once, by where they sit and what kind they are. A revision-not-found error
is not found only at a revision lookup. Any other server error is a failure, never not found:
gitserver already maps its own not-found exits to revision-not-found, and what is left includes
real server failures.
