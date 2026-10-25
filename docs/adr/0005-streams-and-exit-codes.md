# The answer on stdout, notes and errors on stderr, and three exit codes

The answer goes to stdout. Notes go to stderr, written before the answer, so that Claude Code, which
merges the streams in write order, shows a note above the data it qualifies, and so that notes stay
visible when stdout goes through `head` or `jq`. Errors go to stderr with stdout empty, under `--json`
too, because a JSON error document on stdout would vanish into `| jq .field`.

Exit 0 means answered, including an empty or partial answer. Exit 1 means not answered. Exit 2 means
the command line is wrong, and sego sends nothing.
