# Sourcegraph code-search query language

## 1. How a query is read

A query is a search pattern plus filters. Filters are `name:value` tokens such as `repo:`, `file:` and `lang:`; everything else is the pattern. The pattern is optional when filters alone say what to list:

- a query of only `repo:` filters lists repositories;
- a query with `file:` or `lang:` and no pattern lists files;
- `type:commit` or `type:diff` with no pattern lists commits.

### Terms, phrases and regexes

- **Keyword is the default pattern type.** Each whitespace-separated term is matched literally, as a case-insensitive substring. `NewRouter` matches `newRouteRegexp`, `testNewRouter` and `NEWROUTER`. Regex metacharacters are literal: `SkipClea.` matches only the text `SkipClea.`.
- **Terms combine with AND at file level.** `foo bar` returns files that contain both terms anywhere, not necessarily on the same line.
- **Terms can match the file path as well as the content.** `route_test BenchmarkNewRouter` finds `route_test.go` because its path contains `route_test` and its content contains `BenchmarkNewRouter`. A file that matches only by its path comes back with no line matches. Content terms never produce repository results.
- **`"..."` is an exact phrase.** Spaces and punctuation inside are literal: `"func NewRouter"` and `"e.g. POST"`. A phrase does not cross a line break.
  - Single quotes work the same way: `'router or one'`.
  - Inside quotes, write `\"` for a quote and `\\` for a backslash. `"\"POST\""` matches `"POST"` together with its quotes.
  - Quoting also turns reserved words into text: `"router or one"` is a phrase, not an `or`.
- **`/.../` is an RE2 regular expression** inside a keyword query, as in `/func New\w+\(\)/`.
  - Spaces inside the slashes are part of the regex.
  - A regex can span lines with `\n` or `\s+`: `/NewRouter\(\)\n\s*r\.Host/`.
  - Anchors and wildcards only work inside slashes. In keyword mode, `^NewRouter$` and `foo.*bar` written bare are literal text.
  - Flags go inside the slashes, as in `/(?s)func \(r \*Router\) Match\(.*?\n\}/`. Letters after the closing slash, as in `/cloudflare/i`, make the whole term literal text, which usually matches nothing.
- **`content:"..."` sets the pattern explicitly.** Use it when the text looks like query syntax, for example `content:"repo:foo"`, `content:"SkipClean("` or `content:"a or b"`.
- **Bare punctuation is literal.** A term with an unbalanced parenthesis, such as `(value` or `SkipClean(`, is searched as literal text.
- **A leading `-` on a bare term is not negation.** `-race` searches for the text `-race`. Use `not term` to exclude files that contain a term. A `-` prefix negates filters only: `-repo:`, `-file:`, `-lang:`, `-content:`, `-author:` and `-message:`.
- **A token shaped like an unknown filter is searched as text.** `nolint:ineffassign` and `http://example.com` are pattern terms.

### Case

- **Matching is case-insensitive by default.** This applies to terms, phrases and regexes, and to the regexes in `file:`, `repo:` and the predicates.
- **`case:yes` makes the pattern case-sensitive.** It also makes `file:` case-sensitive, but `repo:` stays case-insensitive. `case:` accepts boolean-style values such as `yes`, `no` and `YES`; `case:maybe` returns the alert "Invalid boolean".
- **Inline case flags have no effect.** `(?i)` and `(?-i)` inside `/.../` are ignored; only `case:` controls case. Other flags work: `(?s)` lets `.` cross lines.
- **Field names are case-insensitive.** `REPO:`, `File:` and `patternType:` all work.

### Limits and timeouts

- **Default limit.** With no `count:`, a search stops after 30 matches and sets `limitHit`.
- **What counts as a match.** A match is a line range, a path hit, a symbol, a commit or a repository, not a file. 30 matches can come from just two files.
- **`count:`.** `count:N` raises the limit to N, and `count:all` removes it.
- **Default timeout.** A search has a 20 s deadline by default. A large indexed search can still take longer than that to respond.
- **Maximum timeout.** The server maximum is 60 s.
  - Setting `count:` without `timeout:` raises the deadline to the 60 s maximum.
  - `timeout:` sets the deadline directly. Values above 60 s, such as `timeout:2m`, are accepted without error but capped at 60 s.
- **Timed-out searches.** When the deadline passes, the search returns what it has; `limitHit` can be false and the result set can be empty. Usually the `timedout` list is non-empty, but not always: an empty result after about 20 s with an empty `timedout` list can still be a timeout.

### Errors

- **Most invalid queries return an alert.** The result is zero results and an `alert` titled "Unable To Process Query" with a description. Causes include:
  - a bad `count:`, `timeout:` or date value;
  - an unknown language;
  - an invalid `select:` path;
  - a repeated singleton filter;
  - `rev:` without `repo:`;
  - a missing revision;
  - an unknown context;
  - a commit-only filter used without `type:commit` or `type:diff`.
- **A few return a GraphQL `errors` entry instead.** Two examples are negation in a symbol search and a bad date inside `repo:has.commit.after()`.
- **Some bad values are silently ignored or return nothing.** `type:bogus` returns nothing with no alert. `visibility:bogus` and `patterntype:bogus` are ignored.

### What is searched by default

- **Revision.** Each repository is searched at the head of its default branch. Results from the default branch carry no revision.
- **Scope.** Without `repo:` or `context:`, a query searches every repository on sourcegraph.com (`context:global`).
- **Forks and archived repos.** Both are excluded by default. sourcegraph.com holds a few hundred forks (282 when checked) and tens of thousands of archived repositories (69,060 when checked).
- **The exact-repository exemption.** Forks and archived repos are included automatically when the query has exactly one `repo:` filter whose value is anchored `^...$` around a literal name, such as `repo:^github\.com/gorilla/muxy$`.
  - The exemption also holds with a revision attached: `repo:^github\.com/gorilla/muxy$@HEAD`.
  - Inside an `or`, it applies per branch, so `(repo:^github\.com/a/b$ or repo:^github\.com/c/d$)` includes both even if one is archived.
  - It does not apply when the anchors are missing, as in `repo:gorilla/muxy$`.
  - It does not apply when the name is not a plain literal: `repo:^github.com/gorilla/muxy$` (unescaped dots) or `repo:^(github\.com/gorilla/muxy)$` (a group).
  - It does not apply when there is a second `repo:` filter.
  - An explicit `fork:no` or `archived:no` overrides it.

### Files whose content is never searched

The index replaces the content of some files with a one-line placeholder, `NOT-INDEXED: <reason>`. The placeholder is all that content search sees for these files. They still match by path, through `file:` filters and path terms, and `byteSize` still reports their real size.

| Reason in the placeholder | Rule | Typical files |
|---|---|---|
| `exceeds the maximum size limit` | Larger than 1 MiB (1,048,576 bytes). A 1,021,211-byte file is searched; files from 1,048,984 bytes up are not. | Single-header libraries such as `json.hpp`, generated code, wordlists, large images |
| `contains binary content` | The file contains a NUL byte. | Images and other binaries. UTF-16 text counts too, because it contains NUL bytes, and so does any text file with a stray `\0`. |
| `contains too many trigrams` | More than 20,000 distinct 3-character sequences. Only files larger than about 20 KB can hit this. | Minified JS and CSS, wordlists, logs full of ANSI escapes, dense data files |
| `contains too few trigrams` | Non-empty but shorter than 3 bytes. | One- or two-byte files. Empty files are not marked. |

How these files behave:

- **Listing skipped files.** `repo:^github\.com/owner/name$ NOT-INDEXED count:all` lists them, each with a line-0 match showing its reason. A precise form is `repo:... type:file select:file index:only patterntype:regexp ^NOT-INDEXED:`.
- **The placeholder can be matched by ordinary searches.** A search for `binary content` or `exceeds` can return skipped files as line-0 hits on the placeholder.
- **Unindexed search reads some of them.** `index:no` searches the files directly instead of through the index, and so does a search at a revision that is not indexed, such as most tags. Unindexed search finds content in files skipped for too many trigrams. It still does not search files over 1 MiB or files with binary content.
- **Comparing with `git grep`.** On a clone, `git grep` finds text that the server's default search cannot. In sharkdp/bat, `git grep syntax_request.md` lists `tests/benchmarks/highlighting-speed-src/grep-output-ansi-sequences.txt` (skipped for too many trigrams). The default server search omits that file, while the same search with `index:no` includes it.

## 2. Pattern types

The pattern type is set by the API's `patternType` argument, or in the query with `patterntype:` (alias `patternType:`), which overrides the argument.

### keyword (default)

Literal terms are ANDed at file level, `"..."` gives phrases, `/.../` gives regexes, and `and`, `or` and `not` are operators. This is the right mode for nearly all searches.

### regexp

The whole pattern is RE2.

- **Spaces become a lazy wildcard within one line.** A space between terms becomes `.*?`, so `func Router` matches `func NewRouter() *Router`. `StrictSlash SkipClean` matches nothing in a file where the two words sit on different lines.
- **Literal spaces.** Write a space as `\ ` (`skipClean\ bool`) or wrap the regex in `/.../`.
- **Phrases.** `"..."` is still an exact literal, for example `"func (r"`.
- **Alternation and line breaks.** `skipClean|strictSlash` is alternation. `\n` crosses lines: `NewRouter\(\)\n\s*r\.Host`.
- **Syntax errors.** An invalid regex returns an alert. For example, `(skipClean` gives "missing closing )".
- **Operators.** `and`, `or` and `not` remain operators.

### structural

Comby-style matching that respects balanced `()`, `[]`, `{}` and string delimiters. It is available on sourcegraph.com and also works at non-default revisions.

- **One file at a time.** The server runs a structural search only on the candidate files its index offers first, and reports no cut. Over many files it can miss matches without a `+`: in kubernetes `pkg/kubelet/`, `if err != nil { return nil, err }` found 127 of 192 blocks. Its count and its zero hold when `file:^PATH$` names one file; across more files, find and count with a regex.
- **The language.** The first `lang:` picks the matcher; with no `lang:`, the first `file:` path's extension does. With neither, or with a value the matcher doesn't know (`lang:js`, `file:\.tsx$`), a generic matcher parses the files and can miss matches.

The pattern describes code with holes:

| Syntax | Aliases | Matches |
|---|---|---|
| `...` | `:[hole]`, `:[_]` | Zero or more characters, lazily. Inside delimiters, as in `(...)` or `{:[a], :[b]}`, it matches within that group and can span lines. |
| `:[~regexp]` | `:[hole~regexp]` | Text matching an RE2 regex, such as `fmt.:[fn~Errorf\|Sprintf](...)`. Avoid regexes that match delimiters such as `)`, or `.*`. |
| `:[[hole]]` | `:[~\w+]`, `:[hole~\w+]` | One or more word characters (letters, digits, `_`). |
| `:[hole\n]` | `:[~.*\n]`, `:[hole~.*\n]` | Everything up to and including the end of the line. |
| `:[ hole]` | `:[hole~[ \t]+]` | Spaces and tabs only, not newlines. The name is needed: `Headers(pairs:[ h]...)` matches, while the anonymous `:[ ]` form failed with an error. |
| `:[hole.]` | | An identifier-like run of word characters and punctuation such as `.`, `;` and `-` that does not affect balancing. |

How holes behave:

- **Delimiters.** Delimiters in the pattern must balance. A pattern such as `func (r *Router) :[[name]](` with an unclosed `(` returns nothing, with no alert.
- **Quotes around a hole.** They limit it to one string: `fmt.Errorf("...", ...)` matches only calls whose first argument is a single string literal.
- **Laziness.** A hole stops at the first text that matches what follows it. `fmt.Errorf(:[first], :[second], ...)` matches calls with three or more arguments.
- **Repeated names.** A name used twice must match identical text both times: `return :[v.], :[v.]` matches `return nil, nil`.
- **Structured data.** Structural patterns work on data files: `"engines": {...}` in `package.json`, or `"files": [...]`.
- **Overmatching.** Holes are lazy, but `...` can still cover more than intended when the following text appears later than expected. Anchor holes with surrounding syntax and narrow the search with `file:` or `lang:`.
- **Literal `...`.** To match three dots literally, use `:[~\.\.\.]`.
- **Rules.** The experimental `rule:` parameter filters matches by hole value, for example `fmt.Errorf(:[a], :[b]) rule:'where :[b] == "v2"'`.

## 3. Filters

Ways to negate a filter:

- Put a leading `-` on it: `-repo:x`.
- Put `not` before it: `NOT repo:x` is the same as `-repo:x`.

How repeated filters combine:

- Repeated `repo:` filters intersect, and so do repeated `file:` filters. `repo:gorilla repo:mux` needs both.
- For "either", use regex alternation, as in `repo:^github\.com/gorilla/(mux|handlers)$` or `file:^(regexp|middleware)\.go$`, or use an `or` group: `(file:a or file:b)`.

Singleton filters may appear only once per `or` branch: `rev:`, `count:`, `case:`, `timeout:` and the like. A repeat returns the alert "may not be used more than once". Repeating `type:` returns nothing.

### Scope

| Filter | Aliases | Value | What it does |
|---|---|---|---|
| `repo:` | `r:` | RE2 regex on the repo name, optional `@rev` | Search only repos whose name (`github.com/owner/name`) matches. The regex is unanchored, so anchor it: `repo:^github\.com/gorilla/mux$`. With no pattern, lists matching repos. `repo:@rev` with no name applies the revision to the other `repo:` filters. |
| `-repo:` | `-r:` | regex | Exclude matching repos. |
| `rev:` | `revision:` | revision spec (§5) | Search a revision instead of the default branch. Requires a `repo:` filter. Cannot be combined with `repo:...@rev`. |
| `file:` | `f:`, `path:` | RE2 regex on the full path from the repo root | Keep files whose path matches. Unanchored: `file:^mux\.go$` is the root-level file only, while `file:route\.go` also matches `vendor/.../route.go`. With no pattern, lists matching files. |
| `-file:` | `-f:`, `-path:` | regex | Exclude matching paths. |
| `lang:` | `l:`, `language:` | language name | Keep files of that language. Names are case-insensitive and common aliases work (`go`, `Go`, `golang`, `markdown`). An unknown name returns the alert "Unknown language". |
| `-lang:` | `-l:`, `-language:` | language name | Exclude that language. |
| `content:` | | quoted string | Sets the pattern explicitly (see §1). |
| `-content:` | | quoted string | Exclude files whose content matches. |
| `fork:` | | `yes`, `only`, `no` | Forks are excluded by default; `yes` includes them, `only` searches only forks and `no` excludes them even under the exact-repository exemption. sourcegraph.com holds a few hundred forks, so unscoped and pattern-scoped searches silently leave them out. Other values return an alert. |
| `archived:` | | `yes`, `only`, `no` | The same, for archived repos. sourcegraph.com holds tens of thousands of archived repos, all excluded unless `archived:yes`, `archived:only` or the exact-repository exemption applies. |
| `visibility:` | | `any`, `public`, `private` | Filter by repo visibility. `visibility:private` returns nothing on sourcegraph.com. Unknown values are ignored. |
| `context:` | | context name | Search a named set of repos (see below). |
| `index:` | | `yes`, `only`, `no` | `yes` (default) uses the index where it exists and unindexed search elsewhere. `only` searches only indexed revisions, so a tag usually returns nothing. `no` searches files directly, which also reads files the index skipped for too many trigrams. The values are case-sensitive, and the server's yes/no/only reading also takes its boolean spellings: `y`, `Y`, `yes`, `YES`, `Yes` and the true ones (`1`, `t`, `true`, …) are yes; `n`, `N`, `no`, `NO`, `No` and `0`, `f`, `F`, `false`, `FALSE`, `False` are no; `o`, `only`, `ONLY`, `Only` are only. Other values return an alert. |

**`context:`.** A search context is a named set of repositories, possibly at specific revisions, or a saved query such as `repo:^github\.com/celo-org/`.

- **The default.** `context:global` is every repository and is used when the query has none.
- **Other contexts.** User-owned contexts are written `context:@user/name`. Public ones are usable from the API; sourcegraph.com lists hundreds through the GraphQL `searchContexts` field.
- **Combining contexts.** Use `or`: `(context:@a/x or context:@b/y) term`.
- **Unknown names.** An unknown context returns the alert "Search context not found".

### Predicates

A predicate goes in a filter value and selects repos or files by something other than their name. Predicates combine with each other and with plain filters by intersection.

| Predicate | Argument | What it does |
|---|---|---|
| `repo:has.path(re)` | regex on the full file path | Keep repos that contain a matching path. Negate with `-repo:has.path(...)`. |
| `repo:has.file(path:re content:re)` | `path:` and/or `content:` regex | Keep repos with one file matching both. Either part can be left out. |
| `repo:has.content(re)` | regex | Keep repos with any file whose content matches. Negate with `-repo:has.content(...)`. |
| `repo:has.description(re)` | regex | Keep repos whose description matches. |
| `repo:has.topic(name)` | GitHub or GitLab topic | Keep repos tagged with the topic. |
| `repo:has.commit.after(time)` | time (§5) | Keep repos with a commit after that time. A bad date is a GraphQL error. |
| `repo:has.meta(key)`, `repo:has.meta(key:value)` | key, optional value; the value may be a `/regex/` | Keep repos with that metadata key, or key and value: `repo:has.meta(team:/sourcegraph/)`. sourcegraph.com has almost no metadata. |
| `file:has.content(re)` | regex | Keep files whose content matches, while the pattern does the matching. It can appear more than once (intersection) and works with `type:diff`. It cannot be negated: `-file:has.content()` returns an alert. |
| `file:has.contributor(re)` | regex on contributor name or email | Keep files with a matching contributor. |
| `file:has.owner(x)` | owner, or empty for any | Code-ownership filter. sourcegraph.com has no ownership data, so it returns nothing. |
| `rev:at.time(time[, rev])` | time (§5), optional starting rev | Search each repo at its last commit before that time, on the default branch or on `rev`. Results report the resolved commit SHA. Requires `repo:`. A time before the first commit returns nothing. |

### Shaping

| Filter | Aliases | Value | What it does |
|---|---|---|---|
| `type:` | | `file`, `path`, `symbol`, `repo`, `commit`, `diff` | Choose what is searched (§6). Unknown values return nothing. |
| `select:` | | a select path (§6) | Convert results to that type and deduplicate them. An invalid path returns an alert. |
| `count:` | | integer or `all` | Maximum number of matches, 30 by default. Also raises the deadline to 60 s. |
| `timeout:` | | Go duration (`500ms`, `15s`, `1m`) | Overrides the 20 s default, up to the 60 s cap. A bad value returns an alert. |
| `case:` | | `yes`, `no` | Case-sensitive pattern and `file:` matching. |
| `patterntype:` | `patternType:` | `keyword`, `regexp`, `structural` | Sets the pattern type (§2). |
| `rule:` | | quoted Comby rule | Structural search only (§2). |

### Commit and diff only

These filters need `type:commit` or `type:diff`. Without one, they return the alert "requires type:commit or type:diff".

| Filter | Aliases | Value | What it does |
|---|---|---|---|
| `author:` | | regex | Keep commits whose author name or email matches. The regex is tested against the name and the email separately: `author:silverlock`, `author:^Matt` and `author:gmail\.com$` work, but `author:gmail\.com>$` and `author:"Silverlock <"` match nothing. `-author:` excludes. |
| `committer:` | | regex | The same, for the committer. |
| `before:` | `until:` | time (§5) | Keep commits dated before that time. |
| `after:` | `since:` | time (§5) | Keep commits dated after that time. |
| `message:` | `m:`, `msg:` | regex, quoted if it has spaces | Keep commits whose message matches. `-message:` excludes. Works with `type:diff` too. |

## 4. Operators and filter binding

- **`and` / `AND`**: both sides must match in the same file. Two terms side by side are an implicit `and`.
- **`or` / `OR`**: the union of both sides. Counts add up.
- **`not` / `NOT`**: excludes files that contain the next term, or negates the next filter. `a not b` is the same as `a and not b`. A query of only `not X` lists the files that lack `X`.
- **Precedence.** `and` binds tighter than `or`: `a or b and not c` means `a or (b and not c)`. Use parentheses to group: `(a or b) and c`.
- **Reserved words.** To search for the words `and`, `or` or `not`, quote them or use `content:"..."`.
- **Symbol and regexp searches.** Operators also work in `type:symbol` searches (except `not`) and in regexp mode.

### How filters bind across `or`

An `or` splits the query into branches, and each branch runs as its own search.

- **The query starts with filters, then has only patterns, then optional trailing filters.** All those filters apply to every branch.
  - `repo:X StrictSlash or SkipClean` searches only repo X on both sides.
  - `repo:X StrictSlash or SkipClean file:^mux\.go$` also restricts both sides to `mux.go`.
- **Any other shape.** Each filter belongs only to the branch it sits in.
  - `repo:X a or file:Y b`: the `b` branch gets `file:Y` but not repo X, so it searches every repo.
  - `a or b repo:X`: only `b` is scoped to repo X; `a` searches everything.
  - `repo:X a or (b and c)`: the parenthesised branch is unscoped.
- **To scope a whole expression, put the filters first and parenthesise the patterns.** `repo:X file:Y (a or (b and c))`.
- **Filters distribute into parenthesised groups.** `(repo:A or repo:B) file:\.go$ term` runs as `(repo:A file:\.go$ term) or (repo:B file:\.go$ term)`.
- **Each branch gets its own `count:`.** `count:20` across a two-branch `or` can return 40 matches.
- **Each branch is checked on its own.** `case:yes (a or (case:no b))` gives `case` twice in one branch and is rejected. `((a case:yes) or (b case:no))` is valid and mixes case rules per branch.
- **The exact-repository exemption applies per branch.**

## 5. Revisions and times

### Revisions

- **Two equivalent forms.** `repo:R@rev` is the same as `repo:R rev:rev`.
  - Using both on one repo returns an alert.
  - `rev:` without `repo:` returns an alert.
  - `rev:` may appear only once.
- **Default.** Without a revision, a repo is searched at the head of its default branch. `HEAD` names the default branch explicitly.
- **What a rev can be.** A branch, a tag or a commit SHA, full or abbreviated: `@main`, `@v1.6.0`, `@ef912dd`.
- **Several revs at once.** Separate them with colons: `@HEAD:v1.6.0` or `@v1.6.0:v1.8.0`. Each result reports the rev it came from.
- **Tags or branches only.** By default a name can match a tag, a branch or a commit. `refs/tags/` or `refs/heads/` restricts it to one kind: `@refs/tags/v1.6.0` or `@refs/heads/main`.
- **Globs.** A leading `*` makes the rev a glob over ref names.
  - `@*refs/heads/*` searches every branch, and `@*refs/tags/v1.8*` every matching tag.
  - A glob with no wildcard gets `/*` appended. `@*refs/heads/` means every branch, but `@*refs/tags/v1.8` means tags under `refs/tags/v1.8/` and usually matches nothing.
  - Glob results report the short ref name, such as `main` or `release-1.8`.
- **Excluding refs.** `*!` excludes a glob: `@*refs/tags/v1.8*:*!refs/tags/v1.8.0`, or `@*refs/heads/*:*!refs/heads/release*`.
- **Set difference (commit and diff searches).** A commit search covers every commit reachable from the given revs. A leading `^` subtracts the commits reachable from a rev. `repo:R@main:^v1.8.1 type:commit` lists commits on `main` since `v1.8.1`.
- **A revision shared across filters.** `repo:@rev` with no name applies the rev to the other `repo:` filters: `repo:^github\.com/gorilla/mux$ repo:@v1.6.0`.
- **A point in time.** `rev:at.time(2019-01-01)` or `rev:at.time(3 years ago, main)` (§3).
- **Missing revisions.** A rev that does not exist returns an alert naming `repo@rev`.
- **Only the default branch, unnamed, uses the index.** A search with no revision, or with `@HEAD`, reads the index. Any other revision uses unindexed search, and `index:only` finds nothing there: a tag, a commit, and the default branch named as a revision, such as `@main`. Unindexed search still supports symbol and structural search.

### Time values

The same date parser serves `before:`, `after:`, `rev:at.time()` and `repo:has.commit.after()`. Times are UTC unless an offset is given. Quote any value that contains spaces: `after:"6 weeks ago"`. Inside predicate parentheses, never quote: `rev:at.time(2 years ago)` works, while `repo:has.commit.after("2 years ago")` is rejected as an invalid date.

**Absolute forms:**

| Form | Example |
|---|---|
| ISO date | `2019-11-01` |
| ISO date and time | `2005-04-07T22:13:13` |
| ISO with offset | `2005-04-07T22:13:13+07:00` |
| RFC 2822 | `Thu, 07 Apr 2005 22:13:13 +0200` |
| Month name | `november 1 2019`, `1 november 2019`, `Nov 1, 2019`, `june 25 2017` |
| US slashes | `11/1/2019` |
| Dotted, day first | `01.11.2019` |
| Dotted, year first | `2019.11.01` (two-digit day and month only) |
| Unix seconds | `1632782809` |
| Unix seconds with offset | `1632782809 -0600` |

**Relative forms:** `yesterday`, `today`, `last thursday`, `last week`, `5 days ago`, `20 minutes ago`, `2 weeks ago`, `6 weeks ago`, `1 month ago`, `3 years ago`, `3:00` and `3pm`.

**Rejected:** these return an alert, or a GraphQL error inside `repo:has.commit.after()`.

- `2019.11.1`: a single-digit day in the dotted year-first form
- `2019/11/01`
- `2019-11`
- `now`
- free text

## 6. What each `type:` and `select:` returns

| `type:` | Searches | Returns |
|---|---|---|
| none (default) | file content and paths | File results: line matches, plus path-only hits with no lines. |
| `type:file` | file content only | File results with line matches. |
| `type:path` | file paths only | File results with no lines. Several terms are ANDed against the path, and `or` works. |
| `type:symbol` | ctags symbol definitions | File results carrying symbols (see below). |
| `type:repo` | repository names | Repositories. |
| `type:commit` | commit messages | Commits (oid, subject, author, date, message), newest first. Works with filters only. |
| `type:diff` | diff hunks | Commits with the matching hunks, newest first. `file:`, `lang:` and `file:has.content()` restrict which changed files count. Works with filters only. |

### Symbol search

- **What is indexed.** Symbols come from ctags. Each symbol has a name, a kind (`FUNCTION`, `METHOD`, `STRUCT`, `FIELD`, `VARIABLE`, `CONSTANT`, `INTERFACE`, `PACKAGE`, `CLASS`, `UNKNOWN` and so on) and a container (`mux.Router`, a package name).
- **Markdown headings.** They are indexed too: `##` headings as `CLASS` symbols, and `#` and `###` headings as `UNKNOWN`.
- **How terms match.** A term matches symbol names as a case-insensitive substring. `/^NewRouter$/` matches the exact name; bare `^NewRouter$` is literal text and matches nothing.
- **Several terms.** Every term must match some symbol in the same file; each returned symbol matches at least one term. `SkipClean or newRouteRegexp` returns either.
- **Restrictions.** `not` is rejected with a GraphQL error. `"phrases"` with spaces match nothing, because symbol names have no spaces. `case:yes` works.
- **Where it works.** Symbol search works at tags and other revisions as well as the default branch, and across all of sourcegraph.com (`type:symbol /^NewRouter$/ lang:go`).
- **With no pattern.** `type:symbol` alone lists symbols, but with `select:symbol` or `select:symbol.<kind>` it returns nothing. Add `/./` to list every symbol of a kind.

### `select:`

`select:` converts each result to the selected type and deduplicates. A result that cannot be converted is dropped, so a mismatched `select:` returns nothing without an alert.

| `select:` | Returns | Works on |
|---|---|---|
| `repo` | Distinct repositories. The server never flags a cut `select:repo` answer, so over more than one repository its count is a lower bound. | file, symbol, commit and diff results |
| `file`, `file.path` | Distinct files with no line matches. | file and symbol results; nothing for diffs |
| `file.directory` | Distinct directories that hold matching files, such as `./` and `vendor/x/y/`. | file results |
| `file.owners` | File owners. | Unavailable on sourcegraph.com; returns an alert. |
| `content` | Line matches only. Path-only hits are dropped, so `middleware_test select:content` returns nothing. | file results; nothing for symbols or diffs |
| `symbol` | Symbols. | `type:symbol` only; without it, nothing |
| `symbol.<kind>` | Symbols of one kind. | `type:symbol` only |
| `commit` | The commits. | diff results |
| `commit.diff.added` | Diffs where the pattern matched an added line. The whole diff is still returned. | `type:diff` only; nothing for `type:commit` |
| `commit.diff.removed` | The same, for removed lines. | `type:diff` only |

The symbol kinds are:

- `file`, `module`, `namespace`, `package`;
- `class`, `struct`, `interface`, `enum`, `enum-member`, `type-parameter`;
- `method`, `function`, `constructor`, `property`, `field`, `event`, `operator`;
- `variable`, `constant`;
- `string`, `number`, `boolean`, `array`, `object`, `key`, `null`.

Any other kind returns an alert. `UNKNOWN` symbols cannot be selected.

## 7. Pitfalls

- **Default scope.** With no `repo:` filter, a query searches every repo on sourcegraph.com and stops at 30 matches.
- **Unanchored repo regexes.** `repo:gorilla/mux` also matches `gorilla/muxy` and any `.../gorilla-mux`. Anchor with `^...$` and escape the dots.
- **Unanchored file regexes.** `file:route\.go` matches vendored copies too. Use `^` to start at the repo root.
- **The limit counts matches, not files.** `limitHit: true` with two files means more matches exist; raise `count:`.
- **Pattern negation.** `-term` is a literal search for `-term`. Use `not term`.
- **Anchors and wildcards need a regex.** In keyword mode, `^NewRouter$` and `foo.*bar` are literal text, in symbol search too. Wrap them in `/.../`.
- **Inline case flags.** `(?i)` and `(?-i)` are ignored. Use `case:yes`.
- **Flags after the slash.** `/foo/i` is literal text, not a regex with a flag, and usually matches nothing. Search ignores case by default; other flags go inside the slashes, as `(?s)`.
- **`case:yes` and filters.** It makes `file:` case-sensitive (`file:^MUX\.go$` stops matching `mux.go`) but leaves `repo:` case-insensitive.
- **Phrases stay on one line.** A `"phrase"` never matches across a line break; use `/a\s+b/`.
- **Filter binding across `or`.** A filter does not automatically cover the whole query. Put shared filters first and parenthesise the `or` (§4). A filter written after the patterns, as in `a or b repo:X`, scopes only the last branch.
- **Repeated filters intersect.** `repo:a repo:b` means both must match, not either.
- **Singleton filters.** `case:`, `rev:` and `count:` may appear only once per `or` branch, and `repo:R@x` cannot be mixed with `rev:y`.
- **Silent unknown values.** `type:bogus` returns nothing, and `visibility:bogus` and `patterntype:bogus` are ignored, all without an alert. Check spelling.
- **Commit-only filters.** `author:`, `committer:`, `before:`, `after:` and `message:` need `type:commit` or `type:diff`; otherwise the query returns an alert.
- **Author regexes.** They test the name and the email separately, never `Name <email>` as one string. `author:example\.com>$` matches nothing; use `author:example\.com$`.
- **Unquoted relative times.** `before:3 years ago` reads as `before:3` plus the terms `years` and `ago`, and quietly returns nothing. Quote it: `before:"3 years ago"`.
- **Picky date forms.** `2019.11.1`, `2019/11/01`, `2019-11` and `now` are rejected. Use ISO `2019-11-01`.
- **No quotes inside predicate parentheses.** `repo:has.commit.after("2 years ago")` is rejected; write `repo:has.commit.after(2 years ago)`.
- **Timeouts.**
  - A diff or commit search over many repos with a rare term can hit the 20 s deadline and return zero results with `limitHit: false`. The only sign is a non-empty `timedout` list. Narrow `repo:`, add `after:`, or set `count:` or `timeout:` to get the full 60 s.
  - Nothing runs longer than 60 s, however long the `timeout:` value.
  - A tiny `timeout:` such as `1ms` can return zero results with no `timedout` entry.
  - A broad search can occasionally return nothing after about 20 s with no `timedout` entry, then succeed on a retry. Treat an empty result that took about 20 s as suspect: retry it, or narrow it.
- **Forks and archived repos.** Both are silently missing from every search unless you add `fork:yes` or `archived:yes`, or name exactly one repo as `repo:^literal$`. With 69,000 archived repos on sourcegraph.com, an org-wide `repo:^github\.com/org/` search can quietly skip some of the org's repos.
- **The exact-repository exemption is fragile.** It stops applying with an unescaped dot, a regex group, a missing anchor or a second `repo:` filter.
- **Unindexed files.**
  - Files over 1 MiB, binary files (including UTF-16 text), files with over 20,000 distinct trigrams and one- or two-byte files are invisible to content search on the default branch. A search can come back empty, or incomplete, while the text is in the repo.
  - Check for such files with `repo:... NOT-INDEXED`.
  - For the too-many-trigrams case, retry with `index:no`. Nothing finds content in files over 1 MiB or with binary content.
- **Placeholder hits.** Skipped files can appear as line-0 matches for words in `NOT-INDEXED: contains binary content` and similar placeholders.
- **Glob revisions without a wildcard.** `@*refs/tags/v1.8` matches nothing, because `/*` is appended; write `@*refs/tags/v1.8*`.
- **Structural search over many files.** It can miss matches without a `+`, because the server matches only the candidate files its index offers first. Name one file with `file:^PATH$`, or find and count with a regex.
- **Structural search with no language.** With no `lang:` and no `file:` extension the matcher knows, a generic parser can miss matches. Add `lang:`.
- **`select:repo` counts.** The server never flags a cut `select:repo` answer, so a count over more than one repository can fall short with no sign. Treat it as a lower bound.
- **Structural delimiters.** A structural pattern with unbalanced delimiters returns nothing, with no alert.
- **Anonymous whitespace holes.** The anonymous structural hole `:[ ]` fails; name it, as in `:[ h]`.
- **`file:has.content()` cannot be negated.** Negating it returns an alert. Use `not` on a pattern instead, or `-content:`.
- **Owners and metadata are unavailable on sourcegraph.com.** `file:has.owner()` and `select:file.owners` return nothing (the second with an alert), and `repo:has.meta()` matches almost nothing.
- **`not` in symbol search.** It is a GraphQL error, not an alert.
- **Quoting reserved words.** `"a or b"` searches for the phrase; `a or b` is a union. Quote, or use `content:`, to search for the words themselves.
- **`select:` mismatches.** An impossible conversion returns nothing without an alert: `select:symbol` without `type:symbol`, `select:file` on diffs, or `select:commit.diff.added` without `type:diff`.

## 8. Example queries

```
repo:^github\.com/gorilla/mux$ SkipClean
```
Files in gorilla/mux that mention `SkipClean`, case-insensitively.

```
repo:^github\.com/gorilla/mux$ "func NewRouter"
```
The exact phrase `func NewRouter`.

```
repo:^github\.com/gorilla/mux$ /func New\w+\(\)/
```
Zero-argument constructor declarations, by regex.

```
repo:^github\.com/gorilla/mux$ /NewRouter\(\)\n\s*r\.Host/
```
A call followed on the next line by `r.Host`.

```
repo:^github\.com/gorilla/mux$ StrictSlash not SkipClean
```
Files that mention `StrictSlash` but never `SkipClean`.

```
repo:^github\.com/gorilla/mux$ lang:go -file:_test\.go$ SkipClean
```
Non-test Go files that use `SkipClean`.

```
repo:^github\.com/gorilla/mux$ file:^mux\.go$ (StrictSlash or SkipClean)
```
Either term, in the root `mux.go` only.

```
repo:^github\.com/gorilla/mux$ NewRouter case:yes count:all
```
Every case-exact `NewRouter` match, with no 30-match limit.

```
repo:^github\.com/gorilla/mux$ content:"SkipClean("
```
A literal string that would otherwise look like syntax.

```
r:^github\.com/hashicorp/ lang:yaml terraform
```
YAML files mentioning `terraform` across the hashicorp org.

```
(-----BEGIN [A-Z ]*PRIVATE KEY-----)|(("gh|'gh)[pousr]_[A-Za-z0-9_]{16,}) patterntype:regexp case:yes
```
Checked-in private keys and GitHub tokens across all of sourcegraph.com.

```
-file:\.(json|md|txt)$ hack|todo|kludge|fixme patterntype:regexp
```
Admitted hacks and TODOs, skipping data and docs files.

```
repo:^github\.com/gorilla/mux$ type:symbol /^NewRouter$/
```
The definition of the symbol named exactly `NewRouter`.

```
repo:^github\.com/gorilla/mux$ type:symbol Router select:symbol.struct
```
Struct types whose names contain `Router`.

```
repo:^github\.com/gorilla/mux$ type:symbol /./ select:symbol.interface
```
Every interface in the repo.

```
repo:^github\.com/gorilla/mux$ type:path middleware
```
Files whose path contains `middleware`.

```
repo:^github\.com/gorilla/ securecookie select:repo
```
Gorilla repos that mention `securecookie` anywhere.

```
licensed to the apache software foundation select:file
```
Files carrying the Apache license header, as a deduplicated file list.

```
repo:^github\.com/gorilla/ repo:has.path(^context\.go$)
```
Gorilla repos with a `context.go` at the root.

```
repo:^github\.com/sourcegraph/ repo:has.file(path:CHANGELOG content:fix)
```
Repos whose CHANGELOG mentions `fix`.

```
repo:^github\.com/gorilla/ repo:has.commit.after(2 years ago)
```
Gorilla repos with a commit in the last two years.

```
repo:has.topic(code-search) count:20
```
Repos tagged with the GitHub topic `code-search`.

```
repo:^github\.com/gorilla/ file:\.go$ securecookie select:file.directory
```
Directories that hold Go files mentioning `securecookie`.

```
repo:^github\.com/gorilla/mux$ file:has.content(SkipClean) StrictSlash
```
`StrictSlash` matches, only in files that also mention `SkipClean`.

```
repo:^github\.com/gorilla/mux$@v1.6.0 file:^mux\.go$ SkipClean
```
The same search at tag `v1.6.0`.

```
repo:^github\.com/gorilla/mux$@*refs/tags/v1.8* file:^mux\.go$ SkipClean
```
The search across every `v1.8*` tag.

```
repo:^github\.com/gorilla/mux$@*refs/heads/* file:^mux\.go$ SkipClean
```
The search across every branch.

```
repo:^github\.com/gorilla/mux$ rev:at.time(2019-01-01) file:^mux\.go$ SkipClean
```
The file as it stood on 2019-01-01.

```
repo:^github\.com/gorilla/muxy$ func
```
An archived repo, searched without `archived:yes` thanks to the exact-repository exemption.

```
repo:^github\.com/sharkdp/bat$ NOT-INDEXED count:all
```
Files whose content the index skipped, each with its reason.

```
repo:^github\.com/sharkdp/bat$ index:no syntax_request.md
```
An unindexed search that also reads files skipped for too many trigrams.

```
repo:^github\.com/gorilla/mux$ type:commit SkipClean
```
Commits whose message mentions `SkipClean`.

```
repo:^github\.com/gorilla/mux$ type:commit author:silverlock after:2020-01-01 before:2021-01-01
```
One author's commits in 2020.

```
repo:^github\.com/gorilla/mux$@main:^v1.8.1 type:commit
```
Commits on `main` since tag `v1.8.1`.

```
repo:^github\.com/gorilla/mux$ type:diff select:commit.diff.removed skipClean
```
Commits whose diffs removed a line containing `skipClean`.

```
repo:^github\.com/gorilla/mux$ type:diff file:^regexp\.go$ after:2023-01-01
```
Changes to `regexp.go` since 2023.

```
repo:^github\.com/hashicorp/ type:diff after:"2 months ago" fix
```
Recent diffs mentioning `fix` across an org. The time filter keeps a many-repo diff search inside the deadline.

```
repo:^github\.com/gorilla/mux$ type:diff file:has.content(SkipClean) StrictSlash
```
Diffs touching `StrictSlash` in files that mention `SkipClean`.

```
repo:^github\.com/sourcegraph/ file:package\.json type:diff after:"1 month ago" select:repo
```
Sourcegraph repos with dependency changes in the last month. The same query without `repo:` runs out of time across all of sourcegraph.com.

```
repo:^github\.com/gorilla/mux$ ((SkipClean case:yes) or (strictslash case:no))
```
Case-exact `SkipClean`, together with `strictslash` in any case.

```
repo:^github\.com/gorilla/mux$ -file:_test\.go$ (file:router or newRouter)
```
Files whose path contains `router`, or whose content mentions `newRouter`, excluding tests.

```
(repo:^github\.com/gorilla/mux$ or repo:^github\.com/gorilla/csrf$) file:\.go$ http.Request
```
The same search in two repos, with each branch counted separately.

```
repo:^github\.com/gorilla/mux$ file:^regexp\.go$ patterntype:structural fmt.Errorf("...", ...)
```
`fmt.Errorf` calls in `regexp.go` whose first argument is a single string literal.

```
repo:^github\.com/gorilla/websocket$ file:^client_proxy_server_test\.go$ patterntype:structural return :[v.], :[v.]
```
Returns of two identical values, such as `return nil, nil`, in one file.

```
repo:^github\.com/vuejs/core$ file:^package\.json$ patterntype:structural "engines": {...}
```
The `engines` object in `package.json`, across lines.

```
repo:^github\.com/gorilla/mux$ file:^regexp\.go$ patterntype:structural fmt.Errorf(:[a], :[b]) rule:'where :[b] == "tag"'
```
Two-argument `fmt.Errorf` calls in `regexp.go` whose second argument is exactly `tag`.
