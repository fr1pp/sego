# Facts asked for fail the answer; facts sego adds beside them degrade

An answer holds the facts its command line asked for, and facts sego adds so an agent can judge
them, such as the tags marked in a log or the tags that hold a commit. When a fact asked for can't be read,
the answer fails, since an agent can't use a file listing without its files. When an added fact
can't be read, the answer is still right without it. sego prints it with a note naming what is
unknown and why, and the fact's JSON field is `null`, never an empty value that would read as a
fact.
