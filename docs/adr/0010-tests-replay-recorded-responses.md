# Behaviour is tested against recorded server responses, with a live drift check

Sourcegraph's GraphQL API makes no compatibility promise, and sego depends on how the server
behaves, not just on its schema. So sego is tested against real responses rather than mocks. Each
case runs sego on a command line and serves it responses recorded from sourcegraph.com. It fails if
sego sends any request other than the recorded ones, or prints anything other than what the case
expects. `just live` sends the recorded requests again and fails when the server's answers have
changed.
