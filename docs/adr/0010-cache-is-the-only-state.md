# The cache is sego's only state between runs

sego keeps no configuration and no state between runs, except the bytes of files it reads through
the raw route. In evaluations, agents read the same large file many times. One run made 103 range
reads of a 2.9 MB file, and sourcegraph.com answered HTTP 429. Two others downloaded one file 12 and
18 times.

An entry is keyed by endpoint, repository, full commit and path, so it never goes stale. Sourcegraph
exposes no content hash. The cache never adds a request, and a failed write never fails the read.
Entries unused for 5 days are trimmed, following Go's build cache. `SEGO_CACHE_DIR=off` turns the
cache off.
