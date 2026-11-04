# sego reports the server's answer and doesn't predict its parser

sego can't see how the server will read a query: the parser is closed code that keeps changing, and
the only source available is a snapshot from 2024. Modelling it had no end, since each case found
in evaluations led to a deeper copy, and the copy did harm. Adding `case:yes` for a pattern with an
uppercase letter turned good searches into silent zeros, because `case:yes` also reaches `file:`,
`author:`, `committer:`, `message:` and the text of predicates. So sego follows one rule: it never
makes an answer worse than the server's own, and it doesn't predict the server's parser. Every fact
in an answer comes from the response or from what sego itself did. A query goes as written but for
the deadline, which can only give a search more time, and `N+` means the server reported a cut or
the search stopped. An answer can then mislead the way the server's can: a search ignores case
unless told otherwise, a misspelt filter returns a plain zero, and a capped `select:repo` can fall
short with no sign. That knowledge belongs in the query reference and the search strategy. An answer
that misstates what the server did is a bug; a poor query isn't.

One reading of a query is allowed, because its facts don't depend on how the server read the query.
When the server finds no repository, the query has no `or`, and its only `repo:` filter is `^NAME$`
or `^NAME$@REV` with NAME a literal, host-qualified name, sego looks NAME up. It states that NAME
isn't on the server, and the names the server's repository search finds for NAME's last part, most
stars first, after any proposals of the server's own. Each is a lookup's answer, true whatever the
server made of the query. The lookup ignores case, as `repo:` does, so a name the server holds in
any case is never called missing. Only a pattern that matches exactly one repository name
qualifies: `^owner/name$` names none the server holds, though `owner/name` names one as an address,
and a shell that ate `$@` leaves `^github\.com/o/rv1.2`, which qualifies for nothing.

## Considered options

- **Keep the local checks that look certain.** Each is cheap alone, but together they are the
  model, and a check's truth still depends on the server's parser.
- **Keep smart case, with each filter it reaches kept caseless.** It closes a real gap, but it stays
  safe only while sego keeps up with everything `case:yes` reaches, and when it falls behind the
  failure is a silent zero.
- **Work out cuts the server doesn't report.** A missing `+` is a real loss, but the rules behind
  it describe the server's internals, and the official CLI reports only what the server does.
