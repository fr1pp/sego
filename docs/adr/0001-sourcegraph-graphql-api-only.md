# sego reads code only through Sourcegraph's GraphQL API

sego reads code through sourcegraph.com's GraphQL API, with no token or setup, so an agent can use
it the moment it is installed. It uses no clone, no code host API, no package registry and no
streaming search API, which sourcegraph.com challenges at the rates agents use. Its one exception
is the raw file route, which it reads only for what GraphQL can't carry, because sourcegraph.com
challenges that route too. Blame is left out because it is too slow: the API always applies the
repository's `.git-blame-ignore-revs`, and lines under an ignored reformat took minutes.
