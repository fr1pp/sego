# Every search gets the longest deadline

A search's deadline only bounds it: a search that finishes early answers early, whatever its
deadline. A longer deadline costs time only when a search would otherwise be cut, and then buys a
fuller answer, or, under load, when the index would otherwise shed it, and then buys a right answer
in place of a silent zero. On 2026-10-28, searches of one repository came back wrong 44–56% of the
time at the server's default of 20 s, and 10–11% at 60 s. Nor does it take capacity from fast
searches: after 5 s a search moves to the index's batch queue. One cost comes from any `timeout:`,
whatever its length: a search of several repositories at past revisions waits for each one's archive
instead of skipping it after 500 ms. In 16 such searches on 2026-10-30, that cost no measurable time
and skipped nothing either way. So when a query writes no `timeout:`, sego puts `timeout:60s` at its
front, where it binds every branch of an `or`, and the query an answer prints shows it. A query that
writes `timeout:` anywhere is sent as written, since a second one is an error. sego leaves the cap
alone: a small cap is what ends a broad search early, and choosing one is strategy
([search-strategy.md](../search-strategy.md)). The evidence is in
[research/count-and-timeout.md](../research/count-and-timeout.md).

## Considered options

- **The server's default, 20 s.** Searches shed under load come back as plain zeros, and a search
  cut at 20 s points the agent at a longer deadline, so it runs again at 60 s anyway.
- **60 s only for searches scoped to repositories,** to spare the server long broad searches. A
  `repo:` doesn't bound the work (`repo:^github\.com/` covers most of the corpus), a stopped broad
  search is run again at 60 s anyway, and the batch queue already keeps long searches off fast
  ones.
- **30 s.** A search that can't finish answers at about 33 s rather than 63 s, but searches of one
  repository came back wrong about 20% of the time under load, twice as often as at 60 s.
