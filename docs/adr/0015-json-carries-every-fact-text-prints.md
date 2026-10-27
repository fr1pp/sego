# JSON carries every fact text prints

`--json` exists for a program to read, so it must not hold less than text does. Text and JSON
differ only in how many rows they carry and how wide a line may be (ADR 0004).

- Every fact in a header, a row or a note has a JSON field. Notes go in a `notes` array, and stderr
  still carries them.
- Every list that can be cut carries `X_total`, `null` when the server doesn't give it, and
  `X_more`, whether more exists than the list holds. `X_more` reports what the server sent, so
  where JSON asks for the whole list it is `false` unless the server sent less than its total.
  A search keeps `count`, `at_least` and `stopped`, its own form of the rule.
- A field name means one thing in every command's document. An `ls` entry's `kind` is the word
  its text row prints (`dir`, `file` or `submodule`), while a document's `kind` names what the
  address named (`directory`, `file`, `submodule` or `commit`).
- A fact sego added and couldn't read is `null` (ADR 0014), never an empty value that reads as a
  fact, such as `[]` for tags or `""` for a URL.
