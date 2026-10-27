# The cache is sego's only state between runs

sego keeps no configuration and no state between runs, except the bytes of files it reads through
the raw route: files over the content limit, and files whose window of text holds U+FFFD, where
only the raw bytes are exact (ADR 0002). In evaluations, agents read the same large file many times. One run made 103 range
reads of a 2.9 MB file, and sourcegraph.com answered HTTP 429. Two others downloaded one file 12 and
18 times.

An entry is keyed by endpoint, repository, full commit and path, so it never goes stale. Sourcegraph
exposes no content hash. The cache never adds a request, and a failed write never fails the read. A raw read that fails
where it only made Sourcegraph's text exact degrades to a note (ADR 0014); one for a file over the
content limit fails the answer, since there is no other source.
Entries unused for 5 days are trimmed, following Go's build cache. `SEGO_CACHE_DIR=off` turns the
cache off.
