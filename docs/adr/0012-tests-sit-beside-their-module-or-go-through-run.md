# A test sits beside the module it checks, or goes through `run`

A module's own tests sit beside it, in a sibling `tests.rs`: `src/search/lint.rs` declares
`#[cfg(test)] mod tests;` and the tests are in `src/search/lint/tests.rs`. A module test is kept
only where a case can't pin the behaviour well: a pure function with a large input space (query
preparation, plans, argument parsing, addresses, escaping), or behaviour the command line can't
reach (cache trimming, the transport, combinations of error classification). Anything an agent can
see on the command line is pinned by a case (ADR 0011). A help text is pinned only by its case;
only the help's layout rules are module tests. A module test that repeats a case is deleted.

The library exports `run`, what it takes (`Env`, `Endpoint`, `Transport`, `Reply`,
`TransportError`) and `connect`, the transport `main` hands it, and nothing else. Tests under
`tests/` go through `run`. Re-exporting private modules for tests put two thirds of the suite past
the interface, in one flat file per module, where a refactor broke tests far from the code it
changed.

A case's exchanges pin the shape of each request: the runner fails a request whose operation,
variables or query differ from the next exchange. A test that checks only the request a command
sends is needed only where no case pins it.

The case runner and the code it is built from are the `harness` crate, a member of the workspace
beside sego, and its module tests follow the same layout.
