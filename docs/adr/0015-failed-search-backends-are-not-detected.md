# sego doesn't detect a search backend that fails to answer

Under load, sourcegraph.com's index rejects searches it can't start in time, and the server answers
with whatever the other backends found. GraphQL reports nothing about it, so a search can come back
short, often as a plain zero, with no sign. Only the streaming API reports it, and sourcegraph.com
challenges streaming at the rates agents use. Routing searches around the index with `index:no`, or
checking every zero again, would cost ranking, latency or extra requests for a problem that comes
and goes. So sego accepts that a search answer can be silently short, and the help says so.
