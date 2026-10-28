# Nothing from a repository can pass for sego's own output

Everything sego prints from the server is text someone else controls: file contents, paths, commit
messages and server alerts. An agent treats sego's output as trustworthy. So sego escapes every
character that could hide or forge text, such as control characters, line breaks inside a value and
invisible code points. It prints code only after a gutter, so the first column always belongs to
sego, and it quotes any word printed for pasting, so it pastes back as itself. A repository can't
forge a header, a note or a terminal control sequence in an agent's context, and `--json` still
carries the exact text.
