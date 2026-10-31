# `case:` on sourcegraph.com

How the `case:` filter reaches beyond the pattern, and how it binds across `or`. Written
2026-10-30.

Counts are for gorilla/mux at its default branch, at `count:all` unless the query gives a `count:`
of its own or a count ends in `+`.

## 1. What `case:yes` reaches

- **The pattern, in every result type.** **live-verified**
  - `type:symbol newrouter case:yes` returned 0 symbols; `type:symbol NewRouter case:yes`
    returned 5.
- **`file:` matches case too, in content, symbol, diff and commit searches.** **live-verified**
  for all but commit searches
  - `file:readme Router` returned 30+ matches; `file:readme Router case:yes` returned 0, because
    the file is `README.md`.
  - `type:diff file:readme Router count:20` returned 4+ diffs with `case:no` and 0 with `case:yes`.
  - `type:symbol file:MUX Route` returned 50 with `case:no` and 0 with `case:yes`.
  - For diffs and commits, `sourcegraph-public-snapshot@c864f15af264:internal/search/commit/commit.go:311`
    passes the query's case to the file filter.
- **`author:`, `committer:` and `message:` match case too.** **live-verified**
  - `type:commit author:silverlock`, for an author named Matt Silverlock: 54 commits with `case:no`,
    0 with `case:yes`.
  - `type:commit committer:NOREPLY`: 64 with `case:no`, 0 with `case:yes`.
  - `type:commit message:merge`: 64 with `case:no`, 11 with `case:yes`.
  - `sourcegraph-public-snapshot@c864f15af264:internal/search/commit/commit.go:293-308`.
- **So does the text of a predicate, and of `repohasfile:`.** **live-verified**
  - `file:has.content(newrouter)`: 15 matches with `case:no`, 0 with `case:yes`.
  - With `Router case:yes count:5`, `repo:has.content(newrouter)`, `repo:has.file(readme)` and
    `repo:has.path(readme)` each returned 0. `repo:has.content(NewRouter) Router case:yes count:5`
    returned 5+.
  - `repohasfile:readme NewRouter`: 129 with `case:no`, 0 with `case:yes`.
    `repohasfile:README NewRouter case:yes` returned 122.
- **The date of a predicate isn't reached.** **live-verified**
  - `rev:at.time(2022-01-01) NewRouter` returned 105 with `case:no` and 100 with `case:yes`.
  - `repo:has.commit.after(20 years ago) NewRouter case:yes` returned 122, as `NewRouter case:yes`
    does.
    `repo:contains.commit.after(20 years ago) NewRouter case:yes` returned 122.
- **`lang:` and `repo:` aren't reached.** **live-verified**
  - `lang:markdown Router case:yes` returned 30+ matches.
  - `repo:GORILLA/mux NewRouter case:yes` returned 122, as `repo:^github\.com/gorilla/mux$` does.
  - `type:repo repo:^GITHUB\.COM/GORILLA/MUX$ case:yes` returned the repository, as with `case:no`.

## 2. Keeping a filter caseless under `case:yes`

- **`(?i)` at the start of a value keeps `author:`, `committer:` and `message:` caseless.**
  **live-verified**
  - With `case:yes`: `type:commit author:(?i)silverlock` returned 54, `committer:(?i)NOREPLY` 64,
    `message:(?i)merge` 64, the same as `case:no`.
- **On a `file:` value, `(?i)` alone only works when the value is a regular expression.** The value
  must not reduce to plain text. **live-verified**
  - The index looks for the uppercase of the text, honouring `case:yes`, and finds a file only
    if its name is in uppercase. `file:(?i)readme Router case:yes` matches `README.md`, while
    `file:(?i)mux_test Router case:yes` returned 0, against 95 for `file:mux_test Router case:yes`.
    `-file:(?i)mux_test Router case:yes` excluded nothing: 274, the same as no filter, against 179 for
    `-file:mux_test`.
  - In sourcegraph-public-snapshot, with `fuzzyRegexp case:yes`, `file:(?i)transformer` returned 0
    against 3 without `(?i)`, and `file:(?i)internal/search/query/` 0 against 4.
  - Go parses `(?i)mux_test` to the literal `MUX_TEST`, checked with `regexp/syntax`, and a value that
    parses to a literal becomes a substring search with the query's case:
    `sourcegraph-public-snapshot@c864f15af264:internal/search/zoektquery/query.go:29`.
  - Pinned to a past revision, the search isn't indexed and a real regular expression runs:
    at `@eb99d7a67714`, `file:(?i)mux_test Router case:yes` returned 71, as `file:mux_test` does.
- **A trailing `.*` makes any such value a regular expression and changes no match.** **live-verified**
  - With `case:yes`, written against the same filter without `(?i)`:
    `file:(?i)mux_test.* Router` 95 against 95, `-file:(?i)mux_test.* Router` 179 against 179,
    `file:(?i)mux_test\.go.* Router` 95 against 95, `file:(?i)MUX_test.* Router` 95,
    `file:"(?i)mux_test.*" Router` 95. `file:(?i)\.go$ Router` needs no `.*` and returned 244.
  - `type:symbol file:(?i)route.* Route`: 10 against 10, where `file:(?i)route` returned 0.
    `type:diff file:(?i)mux_test.* Router`: 30 against 30. `type:diff -file:(?i)mux_test.* Router`:
    59 against 59. `type:commit file:(?i)mux_test.* Router`: 4 against 4.
  - Pinned at `@eb99d7a67714`: `file:(?i)mux_test.* Router` 71 against 71,
    `-file:(?i)mux_test.* Router` 145 against 145.
  - It also frees predicates: `file:has.content((?i)newrouter.*) case:yes` returned 15, the same as
    `case:no`, where `file:has.content((?i)newrouter)` returned 0.
- **Inside a pattern, `(?i)` has no effect:** see the query reference, §1 Case.

## 3. How `case:` binds across `or`

- **A `case:yes` in front of an `or` of filtered branches binds only the first branch.**
  **live-verified**
  - `case:yes (repo:^github\.com/gorilla/mux$ NEWROUTER) or (repo:^github\.com/gorilla/mux$
    STRICTSLASH)` returned 30+ matches, though each pattern alone with `case:yes` returned 0.
  - This is the binding the query reference describes for every filter written before an `or`
    (§4).
- **Around the whole query, it binds every branch.** **live-verified**
  - `case:yes ((repo:… NEWROUTER) or (repo:… STRICTSLASH))` returned 0.
  - `case:yes ((repo:… NEWROUTER) or (repo:… StrictSlash))` returned 16.
  - `case:yes (repo:… NEWROUTER or STRICTSLASH)` returned 0.
- **An `or` of patterns alone is one search, so a `case:yes` anywhere in it binds it.**
  **live-verified**
  - `repo:… NEWROUTER or STRICTSLASH case:yes` returned 0.
- **`timeout:` binds differently.** A `timeout:` inside one branch also cut the other branch short
  ([count-and-timeout.md](count-and-timeout.md), §1.5).
