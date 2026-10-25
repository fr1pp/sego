# Nothing from a repository can pass for sego's own output

Every string that comes from the server, on stdout or stderr, has its control characters, line and
paragraph separators and default-ignorable code points written as `\u{h…}` escapes. JSON writes
them as `\u` escapes, so the parsed value is unchanged. Code always follows a gutter. Column 0 then
belongs to sego, and no file, path, commit message or server alert can forge a header, a note or
terminal control sequences in an agent's context. Every case served a GraphQL response runs a
second time with a hostile corpus in the strings of its response, and fails if an unsafe character
reaches the output as itself, a line starts with the corpus (an unescaped line break let server
text reach column 0), or a JSON value carries only part of the corpus. Cases made for the purpose
cover what that can't reach, such as alert titles, file contents and raw reads.
