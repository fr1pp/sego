# Answers state facts; they never advise

What sego prints states what the server returned and what sego did. It never states a guess at what
the caller meant, never prints a corrected command and never suggests a workflow. The one addition
it allows is a pointer: a short line naming the sego command, option or filter that gets what an
answer left out (`--json has the full list`), or that finds a name an error left the caller without.
The aim is that nothing sego prints is false, and that nothing it prints invites a search the agent
can't win.

Every answer is **pinned**, so each location names its commit as a pinned address. Every answer is
also **honest about size**: when sego prints less than it found, it says how much less, and when the
server stops early, sego passes on the server's signal. Agents take what sego prints at face value.
In the 2026-10-09 evaluation, `widen  archived:yes 68896` read as 68,896 more results, `0+` read as
doubt about the string itself, and `deadline 60 s` read as a bound the server keeps. So raw server
fields are passed on only where they mean what they appear to.

