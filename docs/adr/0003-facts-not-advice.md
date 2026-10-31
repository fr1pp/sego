# Answers state facts and never advise

What sego prints states what the server returned and what sego did: no guess at what the caller
meant, no corrected command and no suggested workflow, because agents take output at face value. In
evaluations, `widen archived:yes 68896` (68,896 archived repositories the search left out) read as
68,896 more results, and `0+` (a search cut before it found anything) read as doubt about the
string itself. The one addition is a pointer: a short line, worked out from the answer and ready to
run, naming the sego command that gets the rest of an answer sego itself cut, such as a log past its
default limit. A search the server cut, a path or revision that doesn't exist and a mistyped option
get the fact alone, since what to do next is a guess at what the caller meant.
