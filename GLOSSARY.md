# sego

sego is a code search CLI for agents, built on Sourcegraph's API.

## Naming code

**Address**:
One token naming a repository, and optionally a revision and a path: `REPO[@REV][:PATH]`.
_Avoid_: locator, spec, URL

**Pinned address**:
An address whose revision is a commit, written as 12 hex digits, as sego prints it in every header.
It pastes unchanged into the next command.
_Avoid_: permalink

**Revision**:
Anything that names a commit: a branch, a tag, a full or abbreviated commit, `HEAD`, `~N` or `^`.
_Avoid_: version, ref

**Ref**:
A branch or a tag, as `sego refs` lists them.

**Range**:
Two revisions in one address: `A..B` for `log`, `A...B` for `diff`.

**Lines**:
The `LINES` operand of `show`, the 1-based, inclusive lines of a file to print.

## Answers

**Output**:
Everything sego prints: the answer on stdout, and notes and errors on stderr.

**Answer**:
What a command prints on stdout when it succeeds, which may be empty.
_Avoid_: response

**Response**:
What Sourcegraph sends back to sego's request.
_Avoid_: answer, reply

**Header**:
The first line of an answer: a pinned address, then facts.

**Note**:
A fact about an answer, written to stderr before the answer as `sego: note: …`.
_Avoid_: warning, hint

**Pointer**:
A note or error line naming the sego command, option or filter that gets what an answer left out.
_Avoid_: suggestion, hint, advice

**Usage error**:
A command line sego rejects before sending anything. It exits 2.

**Answer error**:
A question sego couldn't answer: something named doesn't exist, the server refused or failed, the
network failed. It exits 1.

**Default limit**:
How much of an open-ended request sego fetches, or how many rows a list prints in text, when the
caller asks for no more.

**Explicit request**:
A request that names its own extent (a `LINES` range, a `count:`, `--recursive`, a range, a path
given to `diff`), so no default limit cuts what is fetched.

## Search

**Query**:
The text an agent passes to `sego search`, sent to the server exactly as written.

**Plan**:
The searches the server runs for a query, each with its filters, as Sourcegraph reads the query.

**Search**:
One of the searches in a query's plan. An `or` with a filter beside an operand splits a query into
several.

**Pattern type**:
How the server reads a query's patterns: `keyword`, `standard`, `regexp`, `literal`, `structural`
and the rest.

**Result**:
One item of a search answer: a content match, a path match, a symbol, a commit or a repository.
_Avoid_: hit, match (a match is one matched range)

**Unit**:
What a search answer counts: matches, commits, symbols, files, directories or repositories.

**Cap**:
The most units a search returns: the query's `count:`, or 30.
_Avoid_: limit

**Deadline**:
How long the server searches before it stops: the query's `timeout:`, or the server's default.
sego's own deadline for the whole invocation is the **client deadline**.
_Avoid_: timeout

**At least**:
An answer whose count is a lower bound, printed `N+`: the cap was reached, the search stopped, or
the server cut the results without saying so.
_Avoid_: partial, approximate, limit hit

**Stopped**:
A search the server's deadline cut short. A stopped search with no results is an answer error.
_Avoid_: timed out

**Local check**:
A rule sego applies to a query before sending it, for a query the server would answer silently and
wrongly. It prints a note and never blocks.
_Avoid_: lint, validation

**Index**:
Sourcegraph's search index, which holds each repository's default branch at `HEAD`.

**Searcher**:
The part of Sourcegraph that reads a revision the index doesn't hold, file by file.

**Unindexed file**:
A file in a search's scope whose contents the index skipped, because it is over 1 MiB or holds too
many distinct trigrams. No search of the index reaches its contents.

**Pinned search**:
A search at a revision other than `HEAD`, which the searcher reads rather than the index.
_Avoid_: pinned query (a pinned address is a different thing)

**Content search**:
A query with a pattern that searches file contents or symbols, rather than paths, repositories or
commits.

**Widen line**:
The line of a search answer that names the filters (`archived:yes`, `fork:yes`) under which the
server holds more repositories than the query searched.

**Preview limited**:
A commit or diff result whose diff preview the server cut to 5 files or 3 hunks a file.

## Files

**Content limit**:
The size, 1,572,864 bytes, above which Sourcegraph's API returns no content for a file.

**Raw read**:
A read of a file over the content limit through Sourcegraph's raw route, which serves the whole
file.

**Cache**:
The files sego keeps from raw reads, so a large file is downloaded once. It is sego's only state
between runs.
_Avoid_: raw-file cache
