# A test sits inline in the module it checks, or goes through `run`

A module's own tests sit inline, in a `#[cfg(test)] mod tests` at the end of its file. A module test
is kept only where a case can't pin the behaviour well:

- a pure function with a large input space: query reading, plans, argument parsing, addresses,
  escaping
- the reading of server responses, with its combinations of errors
- behaviour the command line can't reach: the cache's file handling, the HTTP transport

Anything an agent can see on the command line is pinned by a case (ADR 0011). A help text is pinned
only by its case; only the help's layout rules are module tests. A module test that repeats a case
is deleted.

The library exports `run`, what it takes (`Env`, `Endpoint`, `Transport`, `Response`,
`TransportError`) and `connect`, the transport `main` hands it, and nothing else. Tests under
`tests/` go through `run`. Re-exporting private modules for tests once put two thirds of the suite
past the interface, in one flat file per module, where a refactor broke tests far from the code it
changed.

A case's exchanges pin the shape of each request: a request whose operation, variables or query
differ from the next exchange fails the case. A test that checks only the request a command sends is
needed only where no case pins it.

The case runner lives in `tests/it/support`, inside the one test binary `tests/it`, with its own
tests inline. Besides the cases, only two tests go through `run`: the built binary's test, and the
cache's test across two runs.
