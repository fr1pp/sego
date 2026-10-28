# The cache of raw file reads is sego's only state

sego keeps no configuration files and no state between runs, except a cache of the files it reads
through the raw route. Agents read the same large file again and again, a range at a time: one
evaluation read one file over a hundred times, and sourcegraph.com answered with HTTP 429. An entry
is keyed by its instance, repository, full commit and path, so it can never go stale. The cache
never adds a request, and never fails a read.
