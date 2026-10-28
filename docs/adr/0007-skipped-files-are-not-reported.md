# A search doesn't report the files the index skipped

The index skips files over 1 MiB and files with too many distinct trigrams, and nothing in a
search's response says so, so an answer can miss a match that only such a file holds. sego used to
list them with a second search in the same request. That search went through the same index as the
first, so it held every answer back until it finished, and under load it came back empty as often
as not. So a search doesn't list them; a query that names `NOT-INDEXED` lists them when asked.
The help says which files no search reads, and how to list them.
