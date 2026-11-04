# Answers state facts and never advise

What sego prints states what the server returned and what sego did: no guess at what the caller
meant, no corrected command and no suggested workflow, because agents take output at face value. In
evaluations, `widen archived:yes 68896` (68,896 archived repositories the search left out) read as
68,896 more results, and `0+` (a search cut before it found anything) read as doubt about the string
itself. The one addition is a pointer: a short line, worked out from the answer and ready to run,
naming the sego command that gets the rest of an answer sego itself cut, such as a log past its
default limit. A search the server cut, a path or revision that doesn't exist and a mistyped option
get the fact alone, since what to do next is a guess at what the caller meant. One fact can stand
beside a repository a search found missing: the names the server's own repository search finds for
its last part (ADR 0018). It is the server's answer to a lookup, not a guess, and it names no
command. A note earns its place by one test: does it tell an agent something the answer doesn't
already show, and would that change what it does next? A note that restates the answer only costs
context (ADR 0004). The one exception is `cache unavailable`: the answer is whole without the cache
(ADR 0009), but the raw route is rate limited separately, so a cache that stops working must not do
so silently, since every read of a large file then spends that limit.
