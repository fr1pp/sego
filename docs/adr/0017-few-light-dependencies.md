# sego keeps its dependencies few and light

sego is a small command-line tool that agents run constantly, so it takes few dependencies and
prefers light ones. That keeps it quick to build and install, with little code it doesn't own. It
parses its command line with `lexopt` instead of clap, and talks HTTP with blocking `ureq` instead
of an async stack. clap would also have replaced sego's hand-written usage errors with its own.
sego's usage errors state facts, carry pointers (ADR 0003) and escape what the caller typed
(ADR 0006).
