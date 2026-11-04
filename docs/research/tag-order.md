# The order of the tags that hold a commit

`sego show REPO@COMMIT` lists the tags that hold a commit in the server's order, and names the last
as the earliest tag. This is what that order is, and where it can mislead. Written 2026-11-04.

## How this was built

- `sourcegraph-public-snapshot@c864f15af264` (2024-08-22). sourcegraph.com runs newer, closed code,
  so claims that rest on it alone are labelled **source (snapshot)**.
- git's documentation for `git for-each-ref`.
- Live GraphQL requests to sourcegraph.com on 2026-11-04, labelled **live-verified**.

## 1. The server sorts by creation date, newest first

- **gitserver lists refs newest created first.** **source (snapshot)**
  - `cmd/gitserver/internal/git/gitcli/refs.go` (`buildListRefsArgs`) runs `git for-each-ref`
    with `--sort -refname --sort -creatordate --sort -HEAD`.
  - git applies the last `--sort` as the primary key, so the order is `-HEAD`, then
    `-creatordate`, then `-refname`.
- **The `GitCommit.tags` resolver isn't in the snapshot.** The snapshot's `GitCommit` type has no
  `tags` field; only `Repository.tags` is there (`repository_git_refs.go`). That `GitCommit.tags`
  uses the same `ListRefs` with `Contains` is an inference from the live order below.
- **Live, the order matches creation date and not commit date.** **live-verified**
  - kubernetes `1dc05009fe7f` is held by 150 tags (`totalCount` 150, `hasNextPage` false), and the
    last is `v1.30.0-rc.0`.
  - Sorting the same 150 by the tagged commit's committer date would put `v1.31.0-alpha.0`
    (2024-03-25) first, before `v1.30.0-rc.0` (2024-03-26). The server lists `v1.31.0-alpha.0`
    second to last.
  - kubernetes `cb5dc46edf70`, from the trials, is held by 175 tags, and the last is
    `v1.29.0-alpha.3`.

## 2. What `creatordate` is

- **An annotated tag's date is its tagger date; a lightweight tag's is its commit's committer
  date.** git's `for-each-ref` documentation: "For commit and tag objects, the special
  `creatordate` and `creator` fields will correspond to the appropriate date or name-email-date
  tuple from the `committer` or `tagger` fields depending on the object type."
- **Numeric fields sort numerically**, `creatordate` among them (same page).

## 3. Where the earliest tag can mislead

- **A lightweight tag added later to an old commit sorts by that commit's date**, so it can come
  last, and be named the earliest tag, though it was created after others. Inferred from §2; not
  seen live.
- **Tags with one creation date are ordered by name, descending**, so of two the last is the one
  whose name sorts first. Inferred from the `-refname` key in §1.
- **The earliest tag can be a pre-release**, such as `v1.29.0-alpha.3`. sego names it as listed
  and doesn't guess which names are releases.
- **A list the server cuts has no earliest tag.** sego asks for `totalCount` and
  `pageInfo { hasNextPage }`, and gives no earliest tag when the list is incomplete. Neither cut was
  seen live: `tags` with no `first:` returned all 150 and 175.

## 4. How this is pinned

- The recorded case `commit::tags_past_the_default_limit` (kubernetes `1dc05009`) prints
  `earliest tag  v1.30.0-rc.0`.
- `just live` compares the last tag of every `CommitTags` recording exactly. New tags join the
  newest end, so the last stays put unless the order changes.
