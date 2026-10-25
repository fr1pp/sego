# Nothing from a repository can pass for sego's own output

Every string that comes from the server, on stdout or stderr, has its control characters, line and
paragraph separators and default-ignorable code points written as `\u{h…}` escapes. JSON writes
them as `\u` escapes, so the parsed value is unchanged. Code always follows a gutter. Column 0 then
belongs to sego, and no file, path, commit message or server alert can forge a header, a note or
terminal control sequences in an agent's context. A property test runs a hostile corpus through
every answer and error to hold this.
