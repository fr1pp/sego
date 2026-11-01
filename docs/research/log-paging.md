# Paging a commit list

How `ancestors` and `comparison.commits` page, what `first` may be, and why a followed history
can't page. Written 2026-11-01.

Live values are from sourcegraph.com on 2026-11-01, anonymous, 222 requests between 12:24 and 12:45
UTC, one at a time. Each row is one sample. Snapshot citations are to
`sourcegraph-public-snapshot@c864f15af264` and give leads, not proof. Sizes are decoded bytes.

## 1. Without `follow`, `after:` is an exact offset

- **`after:` skips that many commits of the list.** **live-verified**
  - gorilla/mux@v1.8.0: `first: 6` matched `first: 3` then `after: "3"`, with no path and with
    `path: mux.go`. `endCursor` read `"3"`, then `"6"`.
  - kubernetes@v1.28.0: pages of 20,000 + 10,000 matched one `first: 30000`, with no path and with
    `path: pkg`.
  - Whole lists of 287 commits, and of 50 for a path, matched a single request, every commit unique.
- **The cursor reaches `git log --skip`.** **source (snapshot)**
  - `cmd/frontend/graphqlbackend/git_commits.go:109` passes `Skip: uint(afterCursor)`, and
    `cmd/gitserver/internal/git/gitcli/commitlog.go` writes `--skip=N`.
  - The live schema of 2026-10-28 calls this argument `after`, an "opaque cursor". The snapshot's
    `afterCursor` is that argument, and its date filters are now `afterDate` and `beforeDate`.
  - Sourcegraph doesn't promise the number, so a live rule pins it.
- **The live code still does this arithmetic.** **live-verified**
  - `first: 3, after: "-1"` failed with `git log -n 4 --skip=4294967295`: one extra commit, and `-1`
    wrapped to an unsigned number.
- **The end of a list is plain.** **live-verified**
  - A short or exact last page gives `endCursor: null`, `hasNextPage: false`.
  - `after` past the end gives no nodes, `null`, `false`, and no error.
  - `"0"` and `""` act as no cursor. `"abc"` is a parse error.
- **A range pages the same way.** **live-verified**
  - `comparison.commits(after:)` acts as an offset. The snapshot's resolver has no such argument, so
    this rests on the live schema and live checks alone.

## 2. With `follow`, `after:` gives wrong answers

- **`after:` skips commits of the revision walk, not the file's.** **live-verified**
  - `src/cmd/compile/internal/ssagen/ssa.go` at golang/go@go1.21.0 has 1,119 followed commits.
  - `first: 150, after: "150"` returned positions 2-151, so 148 were repeats, and `endCursor` still
    read `"300"`.
- **Past a rename it returns nothing, and that reads as the end.** **live-verified**
  - The rename sits at position 10889 of the walk. `after: "10889"` returned positions 227-231, and
    `after: "10890"` returned no nodes and `hasNextPage: false`, leaving 891 commits out of reach.
  - `proc.go` did the same: positions 586-590 at `after: "36807"`, nothing at `"36808"`.
- **Skip and follow go into one `git log`.** **source (snapshot)**
  - `git_commits.go:109-119` passes both to one command. With renames followed, git prunes nothing
    and `--skip` runs in the walk (`git/git@50fe5cc6:revision.c:3203`, `revision.c:4604`).
- **A larger `first:` works.** **live-verified**
  - `first: 5000` returned all 1,119 with `hasNextPage: false`, and `first:` of 150, 151, 300 and
    1000 each returned an exact prefix of it. `hasNextPage` was right at 1118, 1119 and 1120.
  - The longest followed list tried was 8,786 commits: linux `MAINTAINERS`, `first: 20000`, 18.8 s.

## 3. The cost check limits `first`, not the commit count

- **Over the limit, the server refuses before it starts.** **live-verified**
  - HTTP 400 in about 0.25 s, `ErrQueryComplexityLimitExceeded`, `type: "field count"`,
    `limit: 500000` and `actual` in `extensions`. No value tried ran out of time.
- **A field under the one carrying `first` costs `first`.** **live-verified**
  - `nodes` costs nothing, and `pageInfo` and its children count. The rule is in
    `cmd/frontend/graphqlbackend/rate_limit.go`.
  - `@include` isn't considered, so `HISTORY`'s second root always counts.

| Document | Cost | Highest `first` |
|---|---|---|
| `HISTORY` | 20·first + 32 | 24,998 (24,999: 400, `actual 500012`) |
| `HISTORY` + `endCursor` | 22·first + 32 | 22,725 (22,726: 400, `actual 500004`) |
| One root, sego's fields + `endCursor` | 11·first + 4 | 45,454 (45,455: 400, `actual 500009`) |
| `LogRange` | 10·first + 21 | 49,997 passed |
| `nodes { oid }` only | first + 3 | 499,997 (499,998: 400, `actual 500001`) |

- **`first: null` or omitted counts as `first: 1`.** **source (snapshot)**
  - `rate_limit.go` falls back to 1, so the request passes and returns everything. This is a gap in
    the check, and sego doesn't send it.
  - Kubernetes with `first: null`: 117,725 commits, 37.7 s, 33.4 MB. `LogRange` v1.0..v1.28 with
    `first: null`: 103,354 commits, 26.6 s, 29.5 MB. **live-verified**

## 4. Values of `first` that act oddly

- **`first: 1` always reports `hasNextPage: true`.** **live-verified**
  - On the last commit, `first: 1, after: "286"` of 287 gave `endCursor: "287"`, `true`, and the next
    page was empty. `first: 2, after: "285"` was right.
  - It is deliberate: the resolver asks git for N + 1 commits only when N is above 1, so a file with
    one commit needs no full walk (`git_commits.go:59-72`). **source (snapshot)**
- **`first: 0` runs the whole log and returns nothing.** **live-verified**
  - No nodes, `hasNextPage: true`, `endCursor: "0"`; 2.43 s on kubernetes against 0.30 s for
    `first: 1`. `N: 0` means no limit to git (`internal/gitserver/commands.go:951`), and the
    resolver cuts the result to nothing.
- **`totalCount` is null when `first:` is set.** **source (snapshot)**
  - `git_commits.go:146`, to spare a second git command. A total takes a full listing.

## 5. Timing and size

With sego's fields, single samples from one quiet window:

| `first` | kubernetes@v1.28.0 | linux@v6.1 | linux `path: drivers` |
|---|---|---|---|
| 150 | 0.46 s, 44 KB | 0.36 s, 43 KB | 0.69 s, 43 KB |
| 1000 | 0.83 s, 288 KB | 1.07 s, 283 KB | 1.02 s, 281 KB |
| 20000 | 6.55 s, 5.71 MB | 5.32 s, 5.43 MB | 12.3 s, 5.40 MB |
| 45454 | 11.6 s, 13.0 MB | 11.2 s, 12.3 MB | not sent |

- A deep offset is cheap with no path (linux `after: 1000000`, 1.24 s) and slow with one (`drivers`,
  `after: 300000`, 23.6 s). **live-verified**
- With `follow`, the cost is the walk, not `first`: linux `README` took 18.6 s to list 47 commits,
  against 1.34 s for 46 without. **live-verified**
- The anonymous limiter charges 1 per request whatever the cost (`rate_limit.go:482`).
  **source (snapshot)** None of the 222 requests got a 429. **live-verified**

Not tested: behaviour under load, and a followed file long enough to reach the cost limit.
