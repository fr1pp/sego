# Facts asked for fail the answer; facts sego adds beside them degrade

An answer holds the facts the command line asked for, and facts sego adds beside them so an agent
can judge the answer. When a fact asked for can't be read, the answer fails: an agent can't use a
file listing without its files. When an added fact can't be read, the answer is still right
without it, so it prints with a note naming what is unknown, and the fact's JSON field is `null`.

The facts sego adds are:

- the tags marked in a log;
- a search's unindexed listing (ADR 0007);
- a submodule's URL, read from `.gitmodules`;
- whether the other host also has an `owner/name` repository;
- the cache (ADR 0010);
- the exact bytes of a window whose text Sourcegraph already gave.

In code each is a `Result<T, Error>` field of the answer the `sourcegraph` module returns, and the
error's message is the note's reason. There are two exceptions:
- The cache is the reason it was unavailable, an `Option<String>`, since no `Error` kind describes a
  local directory.
- The unindexed listing is an `Option`, `None` when it wasn't sent or can't be read. `search`
  treats a listing that is incomplete or empty as unknown too. A side query that fails under load
  usually comes back empty rather than with an error (ADR 0007), so the note is fixed and gives no
  reason.

A note says what is unknown and why, such as "tags not marked: Sourcegraph failed to answer the tag
list", never only that something went wrong.
