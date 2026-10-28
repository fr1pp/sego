# Answers state facts; they never advise

What sego prints states what the server returned and what sego did. It never states a guess at what
the caller meant, never prints a corrected command and never suggests a workflow. The one addition
it allows is a pointer: a short line naming the sego command, option or filter that gets what an
answer left out (`--json has the full list`), or that finds a name an error left the caller without.
The aim is that nothing sego prints is false, and that nothing it prints invites a search the agent
can't win.

A pointer is worked out from the answer: it gives the command or filter itself, ready to run, and
appears only where it holds for that answer. One pointer guesses at meaning. A missing revision that
ends in a release number points to the refs that contain the number (`release/1.28` gives
`sego refs REPO 1.28`), since tags and branches spell one release with different prefixes and
separators: on 2026-10-27, `refs` found nothing for `release/1.28` in kubernetes or `release-8.0` in
dotnet/runtime, and found both branches by the number alone. A list of refs can't mislead.

Every answer is **pinned**, so each location names its commit as a pinned address. Every answer is
also **honest about size**: when sego prints less than it found, it says how much less, and when the
server stops early, sego passes on the server's signal. Agents take what sego prints at face value.
In the 2026-10-09 evaluation, `widen  archived:yes 68896` read as 68,896 more results, `0+` read as
doubt about the string itself, and `deadline 60 s` read as a bound the server keeps. So raw server
fields are passed on only where they mean what they appear to.

