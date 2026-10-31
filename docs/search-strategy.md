# Search strategy

How to search well with sego, and why. This is what has been learned about getting right answers
fast; sego itself prints facts, not advice ([ADR 0003](adr/0003-facts-not-advice.md)). Each point
rests on facts in [research/](research/) or the [query reference](code-search-reference.md).

## When something isn't found

- **A revision.** `sego refs REPO TEXT` lists the refs whose names contain TEXT, and the release
  number alone finds more than a full tag name: `1.28` rather than `release-1.28`.
- **A path.** `sego search 'repo:^REPO$@COMMIT file:(^|/)NAME(/|$) count:all'` lists the files named
  NAME at a commit, or under a directory so named. `sego log REPO@COMMIT:PATH` names the commit that
  moved or deleted it.
- **A root commit's files.** Sourcegraph has no diff for a root commit; `sego ls --recursive --json
  REPO@COMMIT` lists what it adds.

## Case

From [research/case.md](research/case.md).

- **Write `case:yes` when counting a name.** A search ignores case unless the query says otherwise,
  so `func Test` also counts `func test`, and nothing in the answer shows it.
- **Check the filters beside `case:yes`.** It also makes `file:`, `author:`, `committer:`, `message:`
  and the text of predicates match case. `file:readme` stops matching `README.md`. `(?i)` keeps a
  filter caseless, but on a `file:` value write `(?i)readme.*`: with plain text after it, the
  index searches in uppercase and finds almost nothing.

## The cap and the deadline

From [research/count-and-timeout.md](research/count-and-timeout.md).

- **Look before you count.** The default cap of 30 ends a search early: the server cancels it as
  soon as it has 30 matches, so even a common pattern across sourcegraph.com answers in a second
  or two. Thirty matches show whether the pattern matches what was meant and whether the scope is
  right.
- **Narrow a search that stopped.** `repo:`, `lang:` or `file:` reach further than running it again.
  An answer below the cap with `N+` and no `stopped` may still be cut: `count:all` gets every result.
- **Narrow, then count.** `count:all` cancels nothing: the search waits for every index replica to
  search everything it holds, and returns all it found.
  - On one repository at its default branch it is cheap: hundreds of matches in under a second.
  - For a pattern too rare to fill the cap, it costs no more than the default.
  - For a broad pattern across sourcegraph.com it runs to the deadline and returns megabytes, and
    the count is still at least, not exact.

  So scope with `repo:`, `file:` or `lang:` first, then count.
- **A count above the cap isn't a cheaper `count:all`.** `count:1000` keeps the early exit, but at
  1,000 instead of 30. Use it to see more of a broad result, not to count one.
- **Leave the deadline alone.** sego gives every search 60 s
  ([ADR 0017](adr/0017-every-search-gets-the-longest-deadline.md)), and a search that finishes
  early answers early. A shorter `timeout:` makes nothing faster that would finish anyway, and
  under load it makes wrong answers likelier: at 5 s, every search of the 2026-10-28 run came back
  wrong.
- **Distrust a slow zero.** A zero that arrives at half the deadline or later, with nothing to say
  why, may be a search the index shed under load. Run it again, or narrow it.
- **Count commits with `sego log`, not search.** A commit or diff search counts highlighted ranges
  toward its cap, so `count:3` can return one commit. `sego log REPO@A..B` lists the commits
  themselves.
- **Know what `or` costs.** An `or` whose branches carry their own filters runs as separate
  searches, each with its own cap, so `count:20` can return 40. An `or` of patterns alone is one
  search with one cap.
