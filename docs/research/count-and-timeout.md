# `count:` and `timeout:` on sourcegraph.com

How the two search fields behave, how they interact with each other and with a search's scope,
and what each costs in latency. Written 2026-10-30.

## How this was built

- **Sources, at these commits:**
  - `sourcegraph-public-snapshot@c864f15af264` (2024-08-22). sourcegraph.com runs newer, closed
    code, so the snapshot gives leads. Every claim that rests on it alone is labelled
    **source (snapshot)**.
  - `zoekt@c11c1cb5` (main, 2026-10-29), including #1156, #1184 and #1191.
  - `docs@4b49940b` (main, 2026-10-29).
  - The sourcegraph.com changelog, for dated changes.
- **Live checks:** anonymous POSTs to `https://sourcegraph.com/.api/graphql`, never the streaming
  API, sending sego's own search document (`patternType: keyword`). Each claim gives its query, its
  UTC time and the values that came back.
  - **2026-10-30, 06:13–06:26 UTC, low load:** 146 requests, one at a time, 1.6 s apart. Each round
    sent every variant once in a fresh random order, so the variants were interleaved; there were 3
    rounds. Every response was HTTP 200, with no errors, no alerts and no silent zeros.
    Repository-scoped searches mostly answered in under 0.5 s. Latency fell from round to round
    (median over all variants 674 ms, then 327 ms, then 45 ms), and warm caches and falling load
    can't be told apart: a cache-busting control (a unique `-file:` per request) gave global
    latencies in the same range. Comparisons *between* variants hold; absolute numbers drift.
  - **2026-10-28, 15:29–15:55 UTC, under load:** 1,140 searches across 13 queries with known
    counts, with the deadline varied, plus 208 searches of a term that occurs nowhere as a
    true-zero control; 8–12 requests in flight. Silent zeros and 10x latency variance throughout.
    From 16:00 to 16:30 UTC, the same kind of run compared searches through the index with
    `index:no`. Used for every claim about behaviour under load.
- **Labels:**
  - **live-verified:** seen in one of the live checks above.
  - **source (snapshot):** read in `sourcegraph-public-snapshot@c864f15af264`.
  - **source (current zoekt):** read in `zoekt@c11c1cb5`.
  - **docs:** stated in `sourcegraph/docs`.
  - **inferred:** reasoned from the above, not seen directly.

Citations use short names: `snap` is `sourcegraph-public-snapshot@c864f15af264`, `zoekt` is
`zoekt@c11c1cb5`, and `docs` is `docs@4b49940b`. A live check is cited by its date and UTC time.

## 1. `count:`

### 1.1 Parsing and the default

- **Values.** `count:all`, in any case, is rewritten to `count:99999999` before anything else
  reads the query. **source (snapshot)**
  - `snap:internal/search/query/transformer.go:42`, `:47`
- **Validation.** Any other value must parse as a positive int32 (`strconv.ParseInt(v, 10, 32)`,
  `> 0`). The field is singular and can't be negated. **source (snapshot)**
  - `snap:internal/search/query/validate.go:179-191`, `:288`
- **The default depends on the protocol.**
  - GraphQL `search()` plans with `search.Batch`, whose default limit is
    `DefaultMaxSearchResults = 30`.
  - The streaming API and the web UI use `DefaultMaxSearchResultsStreaming = 10000`. The docs'
    "only 10k first results" is the streaming figure.
  - Every default-count answer in the probe had `matchCount 30` and `limitHit true`.

  **source (snapshot)**, **docs**, **live-verified**
  - `snap:cmd/frontend/graphqlbackend/search.go:41`
  - `snap:internal/search/types.go:51-63`
  - `snap:internal/search/limits/limits.go:12-13`
  - `docs:docs/code-search/working/search-filters.mdx:39`
  - live, 2026-10-30: every default-count search

### 1.2 What is counted

A search builds one `LimitJob(count)` that sits above the `SelectJob`. It counts
`ResultCount()` over the results *after* `select:` has converted and deduplicated them.
**source (snapshot)**
- `snap:internal/search/job/jobutil/job.go:242`, `:255`
- `snap:internal/search/job/jobutil/limit.go:85-121`
- `snap:internal/search/job/jobutil/select.go:62`

| Result | What one unit of `count:` is | Source |
|---|---|---|
| Content match (`FileMatch`) | One matched range, plus one per symbol. A file with neither (a path match) counts 1. | `snap:internal/search/result/file.go:113-119` |
| `type:symbol` | One symbol | same |
| `type:path`, `select:file` | One file: the match carries no ranges after select | `file.go:113-119`, `:136-142` |
| `type:commit`, `type:diff` | One **highlighted range** in the message or diff. A commit with no highlights counts 1. | `snap:internal/search/result/commit.go:68-82`, `:88-107` |
| `type:repo`, `select:repo` | One repository | `snap:internal/search/result/repo.go:34` |

- **`type:diff` counts highlights, not commits.** **live-verified**
  - `repo:^github\.com/gorilla/mux$@v1.8.0:^v1.7.4 type:diff count:3 route` returned **one**
    commit with 3 highlights, `matchCount 3`, `limitHit true`.
  - live, 2026-10-27
- **The cap is enforced by truncation.** When a result batch crosses the cap, `Limit` cuts the
  ranges inside the file that crosses it and sets that file's `FileMatch.LimitHit`. A commit's
  highlights are cut the same way. **source (snapshot)**
  - `snap:internal/search/result/file.go:187-212`
  - `commit.go:88-107`

### 1.3 How the cap stops work

This mechanism drives latency.

1. **The frontend cancels on reaching the cap.** `limitStream.Send` subtracts each event's count.
   Once the remainder reaches zero it sends `IsLimitHit: true` and cancels the whole child
   search. The condition is `after <= 0 && before > 0`, so landing exactly on the cap also sets
   `limitHit`. **source (snapshot)**
   - `snap:internal/search/job/jobutil/limit.go:116-118`
2. **The frontend asks zoekt for at most `count` files per replica.** `ToSearchOptions` sets
   `MaxDocDisplayCount = FileMatchLimit = count`. **source (snapshot)**
   - `snap:internal/search/types.go:207-209`
   - `snap:internal/search/job/jobutil/job.go:715`
   - Zoekt's display truncator counts **files**, and cancels that replica's search once it has
     sent `MaxDocDisplayCount` files. **source (current zoekt)**
     - `zoekt:index/limit.go:50`
     - `zoekt:search/aggregate.go:165-174`
     - `zoekt:search/shards.go:996-1000`
3. **Zoekt has per-shard and per-replica work limits that `count:` only raises.** **source
   (snapshot)**
   - The defaults are `ShardMaxMatchCount = 10_000` and `TotalMaxMatchCount = 100_000`. Both are
     multiplied by 10 for keyword patterns, which is what sego sends, giving 100,000 and 1,000,000.
     - `snap:internal/search/types.go:197-205`
   - A `count` above those numbers raises them to `count`. So only `count:all` (99,999,999) or a
     count above 100,000 changes them.
     - `snap:internal/search/types.go:213-218`
   - `count:100` and `count:1000` leave them unchanged: they differ from the default only in how
     many files a replica sends before it stops and in when the frontend cancels.
   - Zoekt skips the rest of a shard once it has `ShardMaxMatchCount` matches, and stops starting
     new shards past `TotalMaxMatchCount`. **source (current zoekt)**
     - `zoekt:index/eval.go:263-270`
     - `zoekt:search/shards.go:1168`
4. **Ranking collects for a fixed window before streaming.**
   - Each zoekt replica collects and ranks results for `FlushWallTime` (500 ms by default) before
     it streams them. **source (current zoekt)**, **source (snapshot)**
     - `snap:internal/search/types.go:232`
     - `snap:internal/conf/computed.go:586-598`
     - `zoekt:search/aggregate.go:94-162`
   - The frontend then collects across replicas until each has sent its first non-empty batch or
     said done, or 2 × `FlushWallTime` (1 s) has passed. **source (snapshot)**
     - `snap:internal/search/backend/aggregate.go:150-161`, `:166-205`
   - Neither window depends on `count:` or `timeout:`.

**Consequence: with a small cap, a common pattern ends early.** The frontend cancels as soon as
the ranked first batches hold `count` matches. With a pattern rarer than the cap, or with
`count:all`, nothing cancels. The search ends only when every replica has finished every shard, a
work limit is hit, or the deadline passes. **inferred** from 1 to 4. The latency data in §5 fits it.

### 1.4 `limitHit`

GraphQL `limitHit` is `Stats.IsLimitHit`. **source (snapshot)**
- `snap:cmd/frontend/graphqlbackend/search_results.go:50`

It is set by any of these:

- **The frontend cap is reached,** including exactly. **source (snapshot)**
  - `limit.go:116`
- **Zoekt skipped files or shards:** `FilesSkipped + ShardsSkipped > 0`.
  - A shard skips files when it reaches `ShardMaxMatchCount` *or* its context is cancelled, and
    the deadline cancels it. **source (snapshot)**, **source (current zoekt)**
    - `snap:internal/search/zoekt/indexed_search.go:364`
    - `zoekt:api.go:398-412`
    - `zoekt:index/eval.go:149-153`, `:263-270`
  - So **an indexed search cut by its deadline reports `limitHit true`.** **live-verified**: all
    six indexed searches cut by the clock in this probe had `limitHit true`, `timedout` empty and
    no alert:
    - `/q.z.x.j.k/` at 20 s: 24,355 ms
    - `/q.z.x.j.k/ count:1000`: 62,608 ms
    - `/q.z.x.j.k/ timeout:60s`: 65,722 ms
    - `lang:go /q.z.x.j.k/ timeout:10s`: 10,245 ms
    - `lang:go mux.NewRouter count:all timeout:1s`: 8,805 of 9,549 matches
    - `lang:go withAdmissionDeadline timeout:1s`: all 5 matches present, yet `limitHit true`

    Source: live, 2026-10-30, 06:21:33–06:25:50 UTC.
- **Searcher or gitserver reports a per-repository limit:** the searcher hit its per-request cap,
  or commit search hit its per-repository limit. **source (snapshot)**
  - `snap:internal/search/searcher/search.go:124-128`
  - `snap:internal/search/commit/commit.go:82-89`
- **Exception for `select:repo`:** zoekt's skip flags are forced to `false`. **source (snapshot)**
  - `indexed_search.go:366-378`

So `limitHit true` doesn't by itself mean more matches exist. It can mean:

- the cap was reached exactly;
- the clock cut the search, even with every match already found, as with
  `withAdmissionDeadline timeout:1s` above;
- a zoekt work limit was reached.

`FileMatch.limitHit` is set where `Limit` truncates a file's ranges or symbols. **source
(snapshot)**
- `file.go:196-208`
- The searcher's own per-request truncation also cuts inside a file: `snap:cmd/searcher/internal/search/sender.go:43-68`

### 1.5 Behaviour by query shape

- **`or`.** **source (snapshot)**, **live-verified**
  - The planner distributes an `or` into separate basic queries only when its operands carry
    filters. An `or` made only of patterns stays inside one basic query.
    - `snap:internal/search/query/transformer.go:209-240`, the `isPatternExpression` branch at
      `:215`
  - Each basic query gets its **own** `LimitJob` and its **own** `TimeoutJob`, and the branches
    run in parallel under an `OrJob`.
    - `snap:internal/search/job/jobutil/job.go:36-45`, `:253-261`
  - Live:
    - `repo:^github\.com/gorilla/mux$ count:5 (func or return)` returned **5** matches (one count).
    - `(repo:… func count:5) or (repo:… return count:5)` returned **10**.
    - Source: live, 2026-10-30, 06:22:55 and 06:22:57 UTC.
  - **A `timeout:` written before the first branch binds every branch.** **live-verified**, one
    search each, 2026-10-30:
    - `timeout:3s (lang:python /q.z.x.j.k/) or (-lang:python /q.z.x.j.k/)` stopped at 11.2 s.
    - The same without `timeout:` ran to 25.3 s, the second branch's 20 s default.
    - `(lang:python /q.z.x.j.k/ timeout:3s) or (-lang:python /q.z.x.j.k/)`, with the `timeout:`
      inside the first branch only, also stopped early, at 9.7 s. How a branch's own `timeout:`
      scopes is open (§6).
- **`select:`.** The cap counts results after select and deduplication (§1.2). **source
  (snapshot)**
  - **`select:repo` on the index:** `ShardRepoMaxMatchCount = 1` (one file per repository *per
    shard*), and each replica still stops at `count` **files**. A repository spread over several
    shards can use up several of a replica's `count` slots, and the skip flags are dropped.
    - `snap:internal/search/types.go:220-223`
    - `indexed_search.go:366-378`
  - **Pinned revisions through the searcher:** the searcher caps each repository and revision
    request at `count` matches *before* the frontend's select runs.
    - `snap:internal/search/job/jobutil/job.go:701-715` (`FileMatchLimit` into `TextPatternInfo`)
    - `snap:cmd/searcher/internal/search/sender.go:31-68`
- **`type:symbol`.** The cap counts symbols. At HEAD, zoekt's `MaxDocDisplayCount` still counts
  files. At a pinned revision, `SymbolSearchJob{Limit: maxResults}` applies. **source (snapshot)**
  - `job.go:380-391`
- **`type:commit` and `type:diff`.**
  - Every repository's gitserver search gets `Limit: count`, and the frontend cap counts
    highlights (§1.2). **source (snapshot)**
    - `snap:internal/search/commit/commit.go:61-67`
  - **live-verified:** `diff-search-over-a-range.toml`.
- **`type:path`.** One unit per file. **source (snapshot)**
- **Pinned (`repo:R@rev`, `rev:`) as against HEAD.** **source (snapshot)**
  - HEAD of an indexed repository goes to zoekt with the options in §1.3.
  - Other revisions go to the searcher. It tries hybrid search first (zoekt for the paths that
    match the indexed commit, with `MaxWallTime = deadline − 100 ms` and
    `FileMatchLimit = count`), then searches the rest unindexed.
    - `snap:cmd/searcher/internal/search/hybrid.go:202-218`
    - `snap:cmd/searcher/internal/search/search.go:148-177`
  - The searcher's cap is per repository and revision request (§1.5 `select:`).

## 2. `timeout:`

### 2.1 Parsing, the default, the maximum

- **Parsing.** The value is Go's `time.ParseDuration`; anything that fails returns the alert
  "invalid value for field 'timeout'". The field is singular and can't be negated. Negative
  values parse. **source (snapshot)**
  - `snap:internal/search/query/validate.go:193-199`, `:294`
  - `snap:internal/search/query/types.go:556-566`
- **The deadline** comes from `timeoutDuration`. **source (snapshot)**
  - `snap:internal/search/job/jobutil/job.go:610-635`
  - With `timeout:`, the deadline is that value.
  - Without `timeout:` but with `count:`, it is `MaxTimeoutSeconds`.
  - Otherwise it is `DefaultTimeout`.
  - The result is capped at `MaxTimeoutSeconds`.
  - `DefaultTimeout = 20 s` (`snap:internal/search/limits/limits.go:16`).
  - `MaxTimeoutSeconds` defaults to 60 (`limits.go:45`) and is site-configurable
    (`docs:docs/admin/search.mdx:11-19`).
- **Live:** the default is 20 s and the maximum 60 s. **live-verified**
  - `/q.z.x.j.k/`, which has no trigrams, so zoekt must scan everything:
    - no `count:` and no `timeout:`: cut at 24.4 s
    - `timeout:60s`: cut at 65.7 s
  - Source: live, 2026-10-30, 06:25:50 and 06:23:38 UTC.
  - Under load on 2026-10-28, zeros shed at the 20 s default clustered between 0.5 and 1.0 of the
    deadline.
- **The docs disagree.** They say "By default, the timeout is set to 10 seconds". The code and
  live behaviour say 20 s. **docs**
  - `docs:docs/code-search/queries/language.mdx:249`
  - `docs:docs/code-search/queries/index.mdx:91`
- **The deadline is one context timeout over each basic query.** `TimeoutJob` is
  `context.WithTimeout`. **source (snapshot)**
  - `snap:internal/search/job/jobutil/combinators.go:190-198`
- **Zoekt** receives the time left as `MaxWallTime`. The frontend then detaches its own context
  from the deadline, so zoekt can return what it found. **source (snapshot)**
  - `snap:internal/search/zoekt/indexed_search.go:275-290`, `:317-331`

### 2.2 What happens at the deadline

| Backend | What GraphQL returns | Evidence |
|---|---|---|
| Zoekt (HEAD, indexed), results found | The partial results, `limitHit true`, `timedout` empty, no alert | **live-verified**, §1.4 (6 of 6) |
| Zoekt, nothing found, ran the full `MaxWallTime` | Every repository searched is marked timed out, so `timedout` is non-empty | **source (snapshot)**: `indexed_search.go:352-354`. Not seen live: every zoekt zero seen here or on 10-28 had `timedout` empty, including a 55.7 s global zero on 10-28 |
| Searcher (pinned) | `timedout` names the repository, `limitHit false`, usually 0 results | **live-verified**: `repo:^github\.com/kubernetes/kubernetes$@v1.27.0 klog.Errorf count:all timeout:1s` returned 0 matches, `timedout` 1, 1,006 ms (live, 2026-10-30, 06:22:49 UTC); and the pinned searches of 2026-10-28 |
| `DeadlineExceeded` reaches the alert job with no repository marked timed out | Alert "Timed out while searching", proposing a doubled `timeout:` | **source (snapshot)**: `snap:internal/search/job/jobutil/alert.go:63-71`. Never seen live, here or on 10-28 |
| A zoekt replica sheds the search (`ErrOverloaded`, zoekt #1191) | The other replicas' results, often a plain zero, `timedout` empty, no alert, answered at or after half the deadline | **live-verified** on 2026-10-28 (§2.4); **source (current zoekt)**: `zoekt:search/sched.go:56-76` |

- **The answer arrives after the deadline.** The `elapsedMilliseconds` of cut searches:
  - 10 s deadline: 10,245 ms
  - 20 s: 24,355 ms
  - 60 s: 62,608 and 65,722 ms
  - 1 s: 2,390 and 3,549 ms (zoekt), 1,006 ms (searcher)

  The 10-28 data reaches 70.2 s for a 60 s deadline. **live-verified**
  - live, 2026-10-30, 06:21–06:26 UTC, and 2026-10-28
- **The 2026-10-26 changelog** says "Report searches stopped at their deadline as timeouts". Over
  GraphQL, an indexed search cut by the clock is still reported only through `limitHit` and its
  elapsed time, not through `timedout` or an alert. What the entry changed is in closed code; it
  may concern streaming or MCP only. **live-verified** (the GraphQL side) and **inferred**

### 2.3 Does a longer deadline slow down a search that would finish fast?

**No, for the work itself.** Every mechanism found treats the deadline as an upper bound:

- `TimeoutJob` is a plain context timeout. **source (snapshot)**
  - `combinators.go:190-198`
- Zoekt uses `MaxWallTime` only for:
  - a context timeout that starts when the request arrives (`zoekt:search/shards.go:950`, `:1014-1021`)
  - the admission limit of half the remaining time (`zoekt:search/sched.go:62-76`)

  **source (current zoekt)**
- `FlushWallTime` is fixed at 500 ms, and the frontend's cross-replica collection window is fixed
  at 1 s. Neither scales with the deadline. **source (snapshot)**, **source (current zoekt)**
  - §1.3 item 4
- The interactive-to-batch demotion is driven by run time (after 5 s), not by the deadline.
  **source (current zoekt)**
  - `zoekt:search/sched.go:145-152`, `:280-300`
- The snapshot has no "wait for more results until the deadline" logic. Searches end when the cap
  cancels them or when every backend is done. **source (snapshot)**
  - `limit.go`
  - `aggregate.go:197-205`

**Live:** fast searches stay fast with `timeout:60s`. The values below are elapsed-ms medians over
3 interleaved runs (§5.1). **live-verified**

| Search | none | `timeout:5s` | `timeout:20s` | `timeout:60s` |
|---|---|---|---|---|
| mux `func` | 65 | 26 | 18 | 37 |
| kubernetes `klog.Errorf` | 24 | 27 | 90 | 25 |
| kubernetes `NewSharedInformerFactoryWithOptions` | 150 | 23 | 307 | 24 |
| `lang:go mux.NewRouter` | 359 | 1,286 | 302 | 1,062 |
| `lang:go withAdmissionDeadline` (5 matches, global) | 3,291 | | | 2,786 |
| mux `@v1.8.0 func` (pinned) | 86 | | | 153 |
| kubernetes `@v1.28.0 klog.Errorf` (pinned) | 367 | | | 378 |

The spread within any one variant is larger than the difference between variants.

**What a longer deadline does change:**

1. **A search that needs more than the shorter deadline runs longer.** The no-trigram scan
   stopped at 24 s by default and at 63–66 s with a 60 s deadline. **live-verified**
2. **Under load, there is more queue headroom before shedding.** The admission limit is half the
   time left: 10 s at the default 20 s, 30 s at 60 s. So a search that would have been shed (a
   silent zero at about 10 s) instead waits up to 30 s for a slot and then runs.
   - **source (current zoekt)**: `zoekt:search/sched.go:62-76`
   - **live-verified** on 2026-10-28: single-repository median about 11–13 s at every deadline;
     wrong answers 44–56% at 20 s against 10–11% at 60 s (§2.4).
   - Whether the search waits longer depends on the queue, not the deadline: a search admitted at
     once is unaffected.
3. **Multi-repository searcher searches wait for archives.** Any `timeout:`, of any length, or
   any `count:` sets `UseFullDeadline`. The searcher then waits up to the whole deadline to fetch
   each repository's archive, instead of 500 ms per repository. A single-repository searcher job
   always gets the whole deadline.
   - **source (snapshot)**:
     - `snap:internal/search/job/jobutil/job.go:283-284`, `:365-366`
     - `snap:internal/search/searcher/search.go:61-75`
     - `snap:internal/search/types.go:248-255`
   - **docs:** "When provided, the search is given the full timeout to complete"
     (`docs:docs/code-search/queries/index.mdx:91`).
   - **Not observed live:** four `rev:at.time(…)` searches over six gorilla repositories, with and
     without `timeout:20s`, all answered 0 in 1.2–5.7 s with `timedout` empty. Source:
     live, 2026-10-30, 06:22:03–06:23:08 UTC.
   - **Not observed live with cold archives either.** Live, 2026-10-30, 07:42:48–07:44:10 UTC,
     each search over a different organisation so that no archive was warm from an earlier one,
     alternating with and without `timeout:60s`. No search had a repository in `timedout`.
     - `repo:^github\.com/ORG/ rev:at.time(2018-06-01) TODO` over 10 organisations (2–12
       repositories each): with `timeout:60s` 0.5–5.1 s, median 1.3 s (spf13, fatih, junegunn,
       sirupsen, go-chi); without 0.4–4.9 s, median 2.5 s (urfave, BurntSushi, mattn, tj,
       labstack). Most filled the cap of 30.
     - `repo:^github\.com/ORG/ rev:at.time(2017-03-01) zqxjvkwq`, which matches nothing, so every
       archive is read: with `timeout:60s` 2.6, 4.2 and 3.7 s (hashicorp, prometheus,
       cockroachdb); without 2.4, 3.2 and 1.4 s (grafana, elastic, etcd-io).

### 2.4 Under load: wrong answers by deadline

Live, 2026-10-28, 15:29–15:55 UTC (see How this was built). A wrong answer is a count other than
the query's known count. **live-verified**

| Shape | 5 s | 10 s | 20 s (default) | 30 s | 60 s |
|---|---|---|---|---|---|
| One repository, default branch (8 repositories) | 100% | 99% | 44–56% | 20% | 10–11% |
| Global | 100% | 100% | 58–71% | 44% | 19–38% |
| Pinned revision (searcher) | 100% | 92% | 25–42% | 12% | 0–3% |

- **One repository, default branch:** every wrong answer was a total zero with `timedout` empty
  and no alert, the mark of a search zoekt shed (§2.2). The median latency was about 11–13 s at
  every deadline, for terms with 2–106 matches.
- **Global:** wrong answers were zeros at 20 s or less, and partial counts at 30 s or more, where
  some replicas shed while others answered.
- **Pinned:** a wrong answer was a real timeout, running to the deadline, with `timedout` naming
  the repository in most cases.
- **A shed answer never landed before half its deadline,** as zoekt's admission limit requires
  (§2.3 item 2). Answers from the true-zero control also landed at or past half the deadline: 29
  of 64 at 20 s, 21 of 64 at 30 s and 4 of 64 at 60 s, because nearly every search waited for a
  slot.

## 3. How `count:` and `timeout:` interact

- **`count:` without `timeout:` raises the deadline to the maximum,** 60 s on sourcegraph.com.
  - **source (snapshot)**: `job.go:627-630`, `b.Count() != nil`
  - **live-verified:**
    - `/q.z.x.j.k/ count:1000` ran to 62.6 s; the same query without it was cut at 24.4 s
      (live, 2026-10-30, 06:24:46 and 06:25:50 UTC).
    - Under load on 2026-10-28, many `count:1000` global searches ran 55–70 s.
- **It doesn't depend on the count's size.** The test is only whether a `count:` is present, so
  `count:1` and `count:30` (the default cap) also give 60 s. **source (snapshot)**
  - `job.go:627`
  - Live, `count:1000` was tested, not `count:30`.
- **`count:` also sets `UseFullDeadline`** (§2.3 item 3), as `timeout:` does. **source (snapshot)**
- **An explicit `timeout:` wins over `count:`.** For example, `count:all timeout:20s` runs with
  20 s. **source (snapshot)**
  - `job.go:624-627`
- **Scheduling class and the open-source code.**
  - **The snapshot has no scheduling class.** It predates zoekt #1156 (2026-09-02), which added a
    `SchedulingClass` that *callers* choose. Zoekt never infers it from counts or deadlines; it
    defaults to interactive. **source (current zoekt)**
    - `zoekt:api.go:966-975`, `:1016-1018`
    - `zoekt:search/sched.go:162-176`
  - Whether sourcegraph.com's frontend marks any searches as batch (for example `count:all`) is
    in closed code (§7).
  - **live:** global `count:all` answered as fast as the default (§5). There's no sign of batch
    admission at low load. That is consistent with interactive admission, but it isn't proof.
- **The timed demotion applies to every search.** Any search still running after 5 s moves to the
  batch semaphore, which has a quarter of the capacity. #1191 doesn't shed that move, so the
  search waits for a batch slot until its context expires. **source (current zoekt)**
  - `zoekt:search/sched.go:131-152`, `:178-213`
  - The move releases the interactive slot before it waits for a batch one (`yieldFunc`,
    `zoekt:search/sched.go:197-212`). So a long search holds interactive capacity only for its
    first 5 s, the same at a 20 s or a 60 s deadline, and after that competes only with other
    long searches.
  - Commit message of `7751f4d9` (#1191): "neither is an interactive search moving to the batch
    queue after its timeslice"
- **Neither field changes ranking or the flush window.** **source (snapshot)**
  - `FlushWallTime` and `UseDocumentRanks` are set without regard to either: `snap:internal/search/types.go:230-237`.
  - With `count:all`, results still arrive ranked in the first flush, but nothing truncates them.

## 4. Scope

- **Every indexed search goes to every zoekt replica.**
  - The snapshot's `HorizontalSearcher.StreamSearch` sends the query to every endpoint,
    repository-scoped or not. **source (snapshot)**
    - `snap:internal/search/backend/horizontal.go:45-90`
  - In current zoekt, a replica **acquires a scheduler slot before** it narrows to the shards
    that hold the scoped repositories (`selectRepoSet`). **source (current zoekt)**
    - `zoekt:search/shards.go:950-955` (acquire), `:1054-1063` (selectRepoSet)
  - So under load, even a one-repository search waits in, and can be shed by, every replica's
    queue. **inferred.** It fits the 10-28 medians: about 11 s for single-repository searches
    that answer in tens of milliseconds at low load.
- **One repository at HEAD.**
  - The work is that repository's shards only, so `count:all` adds little work: mux 790 matches
    in 154–576 ms, kubernetes `klog.Errorf` 430 matches in 15–1,067 ms. **live-verified** (§5)
  - The cap, the deadline and every replica's queue still apply.
- **Many repositories, or global.**
  - Every replica scans all candidate shards. With a small cap and a common pattern, the
    frontend cancels after the first ranked batches. A rare pattern or `count:all` waits for the
    slowest replica (§1.3).
  - **live:** the rare global search (5 matches) was the slowest family at 0.5–5.7 s whatever its
    count or deadline. The common global search with the default cap took 0.1–2 s.
  - On dotcom, global search runs zoekt only, with no searcher for unindexed repositories.
    **source (snapshot)**
    - `job.go:980-988`
  - On dotcom, a searcher job runs only after the zoekt job completes (`orderRacingJobs`).
    **source (snapshot)**
    - `job.go:262-277`
- **Pinned revisions (searcher).**
  - Each repository and revision is one request with its own `count` cap. The archive fetch
    timeout is the whole deadline for a single repository, or with `count:`/`timeout:`, and
    500 ms otherwise. **source (snapshot)** (§2.3 item 3)
  - **live:** pinned `count:all` was fast once archives were warm: mux@v1.8.0 689 matches in
    34–763 ms, kubernetes@v1.28.0 974 in 300–467 ms. A cold or busy archive fails as a timeout
    with `timedout` set (kubernetes@v1.27.0 at 1 s). Under load on 2026-10-28, linux@v6.1 at
    20 s and a cold chromium/chromium at 60 s did the same.
- **`index:no` and `index:only`.**
  - `index:only` drops unindexed repositories (`snap:internal/search/zoekt/indexed_search.go:266-268`).
  - `index:no` sends every repository to the searcher, so the searcher rules above apply: one
    request per repository, a per-request cap, and fetch timeouts. **source (snapshot)**
  - **live**, under load on 2026-10-28, 16:00–16:30 UTC:
    - `index:no` on 8 large repositories at HEAD answered in a median of 37–41 ms and was never
      shed, where the same searches through the index were 50% shed zeros at 20 s, with a median of
      11.4 s.
    - Over `context:global` it returned 22–27 matches with 12–64 repositories timed out at 60 s,
      against 447–465 matches through the index.
    - Its results under a cap come back unranked, in an order that changed between identical
      requests.

### Why unscoped `count:all` is slow or harmful

The causes below are ordered from most to least certain. The deadline jump is only one of them.

1. **The early exit disappears.** With a cap, the frontend cancels as soon as ranked batches hold
   `count` matches (§1.3). With `count:all` it waits until every replica has searched every
   candidate shard. **source (snapshot)**, **source (current zoekt)**
2. **The zoekt work limits disappear.** For keyword patterns, `count:all` raises
   `ShardMaxMatchCount` from 100,000 and `TotalMaxMatchCount` from 1,000,000 to 99,999,999, so
   nothing stops a broad pattern from matching everywhere. **source (snapshot)**
   - `snap:internal/search/types.go:197-218`
3. **The payload scales with the corpus.**
   - `lang:go mux.NewRouter count:all` returned 9,549 matches in 4,301 files from 1,153
     repositories: 3.26 MB against 11 KB for the default. The download added about 0.6–1 s of
     wall time over `elapsedMilliseconds`. **live-verified** (2026-10-30)
   - A common token such as `func` would be orders of magnitude larger, and would stop at the
     60 s deadline with a partial answer. **inferred**
4. **The deadline jumps from 20 s to 60 s.** A search that can't finish runs three times longer
   before it answers: 24 s against 63 s on the no-trigram scan. **live-verified**
5. **It depends on the slowest replica.** Under load, it waits in every replica's queue and is
   demoted to the quarter-size batch queue after 5 s of running.
   - Admission headroom is 30 s at a 60 s deadline. A shed replica silently drops its share,
     which the 10-28 probe saw as partial global counts at 30 s or more.
   - **source (current zoekt)**, **live-verified** (10-28)
6. **Its cost spills onto other searches.** While it runs it holds a slot on every replica:
   first interactive, then batch. That is the overload zoekt #1190 and #1191 describe.
   **inferred**

When the pattern is selective, unscoped `count:all` is not slow at low load:
`lang:go mux.NewRouter count:all` took 0.1–1.9 s, about the same as the default cap. The cost is
in causes 1 to 3, which grow with how much the pattern matches, and in 4 to 6, which grow with
load. **live-verified**

## 5. Measured latency (2026-10-30, 06:13–06:26 UTC, low load)

Each cell gives `elapsedMilliseconds` as the median over 3 runs, the min–max range in
parentheses, then `matchCount`, with `L` where `limitHit` was true. `timedout` was empty and
`alert` null in every cell. Wall time was elapsed plus about 0.25 s, plus the download for large
answers.

### 5.1 Deadline, with the default cap

| Search | none | `timeout:5s` | `timeout:20s` | `timeout:60s` |
|---|---|---|---|---|
| `repo:^github\.com/gorilla/mux$ func` | 65 (31–806) 30L | 26 (21–518) 30L | 18 (15–282) 30L | 37 (31–490) 30L |
| `repo:^github\.com/kubernetes/kubernetes$ klog.Errorf` | 24 (18–349) 30L | 27 (15–76) 30L | 90 (30–280) 30L | 25 (17–47) 30L |
| same, `NewSharedInformerFactoryWithOptions` | 150 (131–1611) 30L | 23 (19–41) 30L | 307 (16–939) 30L | 24 (20–97) 30L |
| `lang:go mux.NewRouter` | 359 (231–1273) 30L | 1286 (123–1476) 30L | 302 (109–1070) 30L | 1062 (888–1865) 30L |
| `lang:go withAdmissionDeadline` (rare, 5 matches) | 3291 (712–3513) 5 | | | 2786 (535–4705) 5 |
| `repo:^github\.com/gorilla/mux$@v1.8.0 func` | 86 (55–138) 30L | | | 153 (100–1375) 30L |
| `repo:^github\.com/kubernetes/kubernetes$@v1.28.0 klog.Errorf` | 367 (268–3580) 30L | | | 378 (309–1147) 30L |

### 5.2 Cap, with no `timeout:` (so the deadline is 60 s whenever `count:` is present)

| Search | none (20 s) | `count:30` | `count:100` | `count:1000` | `count:all` | `count:all timeout:20s` |
|---|---|---|---|---|---|---|
| mux `func` | 65 (31–806) 30L | | 165 (14–2430) 100L | 18 (17–246) 790 | 318 (154–576) 790 | |
| kubernetes `klog.Errorf` | 24 (18–349) 30L | 54 (20–335) 30L | 315 (17–970) 100L | 18 (17–316) 430 | 26 (15–1067) 430 | 669 (18–753) 430 |
| `lang:go mux.NewRouter` | 359 (231–1273) 30L | 831 (731–2008) 30L | 853 (679–963) 100L | 1157 (105–1265) 1000L | 694 (101–1860) 9549 | 1011 (385–1012) 9549 |
| `lang:go withAdmissionDeadline` | 3291 (712–3513) 5 | | | | 2477 (642–5667) 5 | |
| mux `@v1.8.0 func` | 86 (55–138) 30L | | | | 415 (34–763) 689 | |
| kubernetes `@v1.28.0 klog.Errorf` | 367 (268–3580) 30L | | | | 426 (300–467) 974 | |

Response sizes:

- 11 KB at the default cap
- 34 KB at `count:100`
- 346 KB for global `count:1000`
- 3.26 MB for global `count:all`
- 155 KB for mux `count:all`
- 327 KB for pinned kubernetes `count:all`

### 5.3 Searches that can't finish

| Search | elapsed ms | matchCount | limitHit | timedout |
|---|---|---|---|---|
| `/q.z.x.j.k/` (no trigrams, all languages) | 24,355 | 9 | true | 0 |
| `/q.z.x.j.k/ count:1000` | 62,608 | 12 | true | 0 |
| `/q.z.x.j.k/ timeout:60s` | 65,722 | 13 | true | 0 |
| `lang:go /q.z.x.j.k/` | 18,456; 20,090 (busted) | 0 | false | 0 |
| `lang:go /q.z.x.j.k/ count:1000` | 10,417; 15,681 (busted) | 0 | false | 0 |
| `lang:go /q.z.x.j.k/ timeout:10s` | 10,245 | 0 | true | 0 |

The `lang:go` scan fits in 10–20 s, so it doesn't separate the deadlines. The scan over all
languages does.

### 5.4 Other runs

- **The first run of a query was slow:**
  - `lang:go withAdmissionDeadline count:all` took 20,560 ms.
  - `lang:go mux.NewRouter count:all` took 20,066 ms.
  - Within the next 2 minutes the same queries took 2.5 s and 0.7 s.
  - The rare query's six deadline and cap variants, run back to back, took 1.4–15.3 s in no
    order of deadline (live, 2026-10-30, 06:13:41–06:15:31 UTC).
  - These are the only samples near 20 s outside the deadline tests. Their cause is unknown.
- **Cache-busting:** a unique `-file:zqbustN\.txt` per request.
  - Global default cap: 727, 979 and 2,007 ms.
  - Global `count:all`: 557, 1,390 and 2,077 ms.
  - Both are in the range of the unbusted repeats, so a result cache wasn't shown.
- **Not measured:**
  - a rare pattern at pinned revisions
  - `timeout:5s` and `timeout:20s` on the rare global search and on pinned searches
  - `count:100` and `count:1000` on pinned searches
  - anything under load with the cap varied (§2.4 varies the deadline)

## 6. Open questions

- **Does sourcegraph.com's closed frontend set zoekt's `SchedulingClassBatch` for some searches,**
  such as `count:all`, large counts or long deadlines? Zoekt #1156 added the option for "API
  users", and the 10-26 changelog has "Improve indexed-searcher behavior under load". At low load,
  global `count:all` showed no admission penalty.
- **Does the current frontend still fan out every search to every replica?** It did in the
  snapshot. The answer decides whether a single-repository search can avoid other replicas'
  queues.
- **What explains the round-to-round speed-up** (674 → 327 → 45 ms) and the two 20 s first runs?
  Possibilities: zoekt page cache, a frontend cache (repository resolution or results), or falling
  load.
- **Is `ZOEKTSCHED=queuewaitseconds` set on sourcegraph.com?** It would shed sooner than half the
  deadline, whatever the deadline.
- **Does `UseFullDeadline` change anything live** for multi-repository unindexed searches? Twenty
  searches, 16 of them over cold organisations, showed no difference (§2.3 item 3); a scope large
  enough to make archive fetches queue wasn't tried.
- **What did the 10-26 "Report searches stopped at their deadline as timeouts" change?** GraphQL
  still reports indexed deadline cuts only through `limitHit` and elapsed time.
- **How do these latencies look under load,** with the deadline fixed and the cap varied? The
  10-28 data varied the deadline, not the cap.
- **Does a `timeout:` inside one branch of an `or` bind the other branches?** One search stopped
  early as if it did (§1.5).

## Summary

- **A longer deadline doesn't slow a search that would finish within the shorter one.** At low
  load, `timeout:60s` matched no timeout, `timeout:5s` and `timeout:20s` within noise at every
  scope.
- **A longer deadline costs time in three cases:**
  - **A search that needs more than 20 s** answers at about 63–66 s instead of about 24 s, with
    more results and `limitHit true`.
  - **Under load**, a search waits up to half its deadline for a zoekt slot before it is shed: 30 s
    at 60 s against 10 s at 20 s. On 10-28 this cut wrong single-repository answers from about
    half to about a tenth, at a median of about 11–13 s either way.
  - **Multi-repository searcher searches** (pinned revisions or `index:no` over several
    repositories) wait for archive fetches instead of skipping a repository after 500 ms. In 16
    searches over cold organisations this cost no measurable time (§2.3 item 3).
- **A long deadline doesn't take interactive capacity from other searches.** A search still
  running after 5 s gives up its interactive slot and waits for a batch one, at any deadline.
- **Any `count:` sets the deadline to 60 s,** whatever its value, unless `timeout:` is present.
  `count:30` and the default cap return the same matches with different deadlines.
- **A small cap makes common patterns fast.** The frontend cancels at the cap, and zoekt replicas
  stop after `count` files. Patterns rarer than the cap, and `count:all`, wait for every replica.
- **`count:N` up to 100,000 changes only the files per replica and the frontend cut.** `count:all`
  also lifts zoekt's per-shard and total match limits, and returns everything it found: 3.3 MB
  for 9.5k matches.
- **Scoped to one repository at HEAD, `count:all` cost little:** sub-second for 430–790 matches.
  Global `count:all` of a selective pattern cost about the same as the default cap. Global
  `count:all` of a broad pattern is bounded only by the 60 s deadline and by the response size.
- **Under load, every indexed search queues on every zoekt replica,** single-repository ones
  included (snapshot fan-out plus admission before shard selection). Load, not the fields,
  dominated latency on 10-28.
- **A search cut by its deadline answers after the deadline:** 0.2–5.7 s late here, up to 10 s
  late on 10-28. Over GraphQL an indexed cut shows as `limitHit true` with `timedout` empty, and a
  pinned cut as `timedout` set.
- **`limitHit true` doesn't by itself mean more results exist.** It is also set at exactly the cap,
  and on a deadline cut that already had every match.
- **The cap counts highlighted ranges in `type:diff` and `type:commit`,** results after `select:`
  in a select, and files per zoekt replica before select.
- **An `or` of patterns shares one cap and one deadline.** An `or` whose branches carry filters
  runs each branch as its own search with its own cap and deadline, in parallel.
