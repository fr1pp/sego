# Each question has a declared request budget

Each invocation asks Sourcegraph one question, and each question sends a fixed sequence of
requests, with no retries, backoff or concurrency beyond the one fallback below. All of them run
under one 90 s client deadline (ADR 0004), and nothing starts after it passes.

**Each revision name is looked up once.** `commit(rev:)` fetches a name the mirror lacks, so two
lookups of one missing name in a request race each other's fetch, and the one that waits gets
"revision not found after waiting for a fetch already in progress". A fetch can also move a branch
between two lookups of one answer: on 2026-10-26 a diff of `develop...COMMIT` took its merge base
from a `develop` its own head lookup had just moved, and printed `0 files`. So:

- **One revision:** every part of the answer nests under the one lookup. A GraphQL error nulls its
  nearest nullable parent, and `diff`, `behindAhead` and `comparison` are non-null, so a part that
  can fail on its own sits in a nullable wrapper, `tree(path: "") { commit { … } }`, whose error
  nulls only the wrapper.
- **A range:** one request resolves both ends, and a second reads the comparison by commit ID. One
  request can't bind a comparison to two lookups.
- **`owner/name`** asks for both `github.com/owner/name` and `gitlab.com/owner/name` in the one
  request. A missing gitlab.com repository costs a null and no fetch.
- **A web URL with a slashed branch** asks for each way of splitting it into a revision and a path,
  each guess as `comparison(base: G, head: G, fetchMissing: false)`, so a wrong guess costs no
  fetch. Only when no guess resolves with its path does the same document go again with fetching:
  the one retry, started only with time left.

sego keeps fetching a missing revision everywhere else: a commit pushed minutes ago is otherwise
falsely not found.

**A raw read** of a file's bytes is one more request, sent only for what GraphQL can't carry: a file
over the content limit, or a window whose text holds U+FFFD (ADR 0010). Binaries are never
downloaded.

| Question | Commands | GraphQL | Raw read |
|---|---|---|---|
| a file or directory | show, ls | 1 | ≤ 1 |
| a web URL with a slashed branch | show, ls | 1; 2 when no guess resolves without fetching | ≤ 1 |
| a commit | show REV, diff REV | 1 | 0 |
| a range's comparison | diff A...B | 2 | 0 |
| a revision's history | log REV | 1 | 0 |
| a range's history | log A..B | 2 | 0 |
| refs | refs | 1 | 0 |
| a search | search | 1 | 0 |

**`mirrored`** is the mirror's `updatedAt`, read in the request that resolves the address. In a
request that fetched, as a range or the split fallback can, a name may have resolved just before
that update, so `mirrored` says when the mirror was last updated, not that the name was read after
it.

When an answer would need a request beyond its budget, sego goes without it. The indexed commit
beside a pinned search added a median of 0.50 s, and up to 14.68 s, measured on 2026-10-18, so sego
prints a cautious `+` instead.
