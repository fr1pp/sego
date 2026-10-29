# `--json` carries every fact the text prints

`--json` exists for programs, and a program should never have to read the text to learn something.
So every fact in a header, a row or a note has a JSON field, and JSON differs from text only in
carrying the whole answer and every line in full (ADR 0004). Notes stay on stderr too, so an agent
piping the JSON into `jq` still sees them.
