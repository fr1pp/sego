# Nothing from a repository can pass for sego's own output

Every string that comes from the server, on stdout or stderr, has its control characters, line and
paragraph separators and default-ignorable code points written as `\u{h…}` escapes. JSON writes them
as `\u` escapes, so the parsed value is unchanged. Bytes of a file that aren't UTF-8 print as
`\x{hh}`, and JSON carries such a line twice: `text`, with U+FFFD in their place, and `bytes`, the
line in base64.

A CR in a file shown by `show` prints as `\u{d}`, except the CRs of CRLF terminators when the
header says `CRLF`. It says so only when the window shows at least one terminated line and every
terminated line shown ends in CRLF, so a header can't hide a CR that a line really holds. JSON line
text keeps every CR. A word printed for pasting into a shell, such as
an address or a path in a pointer, is quoted for bash instead, with each unsafe character as a
`$'…'` octal escape, so it pastes back as itself. Code always follows a gutter. Column 0 then
belongs to sego, and no file, path, commit message or server alert can forge a header, a note or
terminal control sequences in an agent's context. Every case served a GraphQL response runs a second
time with a hostile corpus in the strings of its response, and fails if an unsafe character reaches
the output as itself, a line starts with the corpus (an unescaped line break let server text reach
column 0), or a JSON value carries only part of the corpus. Cases made for the purpose cover what
that can't reach, such as alert titles, file contents and raw reads.
