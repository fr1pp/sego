# One module owns everything sego asks Sourcegraph, and types it by hand

The `sourcegraph` module owns every request sego sends and every response it reads: the GraphQL
documents, resolving repositories and revisions, the raw read and its cache, and sorting the
server's errors by where they sit and what kind they are. Commands get typed answers and errors
from it, never a document, an alias, a JSON path or a raw GraphQL error. Sourcegraph's quirks stay
in one place, and commands own only sego's own rules: windows, notes, headers and wording. Its
response types are written by hand with serde rather than generated from the schema. That keeps
dependencies light (ADR 0017), and handles selections that repeat at run time, such as one per path
given to `diff`, which code generators handle poorly.
