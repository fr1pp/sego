# Behaviour is tested by cases that own their exchanges, with a live drift check

Most behaviour is tested by cases. A case is one file under `tests/cases/<command>/`: the command
line and environment, the exit code, stdout and stderr, and each exchange with the server, that is
the request sego sent (operation, variables and query text) and the response, as JSON. A raw read's
body sits beside the case as `<case>.body`. A runner calls `run` in-process, as one test per case,
with a transport that serves the case's exchanges in order. A sent request that differs from the one
written fails the case, and so does an exchange left unused, so a case pins the requests a command
sends as well as its answer. The runner also checks every case for one request per invocation (ADR
0002), no request on a usage error, and no note or error line ending in a period. One test runs the
built binary, for what `main` adds.

A recorded response names the endpoint and date it was recorded. Any other response was made for
its case, for an answer sourcegraph.com can't be made to give on demand, such as a stopped search. A
search is recorded only when it ran to completion, because cut results vary from run to run. Bless
rewrites a case's expectations, and the request of an exchange made for the case; a recorded
exchange whose request changed must be recorded again.

`just live` re-sends every recorded request to sourcegraph.com and fails when the response's shape
changes or new errors appear, or, for a response marked `live: values`, when its values change.
Because those requests are the operations sego sends, this is also the schema check, and sego needs
no GraphQL validator.
