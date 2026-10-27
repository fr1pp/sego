# Behaviour is tested by cases that own their exchanges, with a live drift check

Most behaviour is tested by cases. A case is one TOML file under `tests/cases/<command>/`. It holds:

- the arguments and environment
- the exit status
- stdout and stderr, or that either is closed
- each exchange with the server: the request sego sent (operation, variables and query text) and
  the response (status, content type and body)

A raw read's body is a `.body` file that the case names, and cases may share one. A query's text
is stored once under `tests/documents/`, as `<operation>-<hash>.graphql`, the hash being the first
8 hex digits of the text's SHA-256, and an exchange names its document. A changed document is a
new file, so a recorded exchange still names the text it was recorded with. Bless writes new
documents and deletes those no case names. Each case is one
`#[test]` in the single test binary `tests/it`, from a list that bless keeps current. The test calls
`run` in-process with a transport that serves the case's exchanges in order. A request that differs
from the next exchange fails the case, and so does an exchange left unused. A case therefore pins
the requests a command sends as well as its answer.

Every case is also checked for:

- at most two GraphQL requests and one raw read (ADR 0002); each case's exchanges pin the exact
  sequence
- no request on a usage error, including the three found from a response (ADR 0005)
- no note or error line that ends in a period
- every request under the one client deadline (ADR 0004)
- the endpoint `connect` is given: the case's `SEGO_ENDPOINT`, or the default
- a cache directory left untouched unless a raw read was sent

Faults below HTTP, such as a dropped connection or a passed deadline, are not cases. They are module
tests of the transport and of the Sourcegraph module. One test runs the built binary, for what
`main` adds.

A response is recorded unless sourcegraph.com can't be made to give it on demand, such as a stopped
search. A hand-written exchange whose response carries a located error names its evidence:
`evidence = "<recorded case>"`, a recorded response with the same kind of error at the same kind of
path; `evidence = "GqlError::kind table: …"`, a message in that table, which holds only messages
from recorded responses; or `evidence = "none: REASON"`. A recorded response names the endpoint and date it was recorded. A search is recorded only
when it ran to completion, because cut results vary from run to run. Bless rewrites a case's
expectations, and the request of any exchange that isn't recorded. A recorded exchange whose request
changed must be recorded again.

`just live` re-sends every recorded request to sourcegraph.com. It fails when the response's shape
changes or new errors appear. For a response marked `live = "values"`, it also fails when the values
change. A difference counts only if a second answer shows it too, since sourcegraph.com now and then
leaves out part of a search's answer. Three differences never count: a "revision not found after
waiting for a fetch" error at a revision lookup that one answer has and the other doesn't, which
another client's fetch can cause; which of two missing names waited for the fetch they share; and
the mirror's `updatedAt`, which moves on its own.

`just live e2e [COMMAND…]` runs on demand, never in `just ci`. It runs sego against sourcegraph.com
for each case whose exchanges are all value-checked, and compares stdout, stderr and exit code with
the case, masking `mirrored`. Before a change re-records a command's cases, it runs the changed
binary this way, and each difference must be a change the work intends or server drift that a
second run confirms. Because those requests are the operations sego sends, this is also the schema check,
and sego needs no GraphQL validator.
