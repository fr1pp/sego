# Behaviour is tested against recorded server responses, with a live drift check

Sourcegraph's GraphQL API makes no compatibility promise, and sego depends on how the server
behaves, not just on its schema. So sego is tested against real responses rather than mocks. A test
is an ordinary `#[test]` that runs sego on a command line and pins what it prints with an insta
snapshot. What a person writes and what a machine writes are kept apart, as `Cargo.toml` is from
`Cargo.lock`. A test holds only what a person decides: the command line and any response the server
can't be made to give. A recording is one generated file per distinct request, found by a hash of the
request, and a test holds nothing about the requests it causes. A snapshot is generated too. A
request with no recording fails, so a recorded test proves what sego sends. A response in a test
proves what sego prints but not what it asked, so a changed query leaves those tests alone. A snapshot
ignores what ends a text and treats `\r\n` as `\n`, so it closes with a line of its own and sego's
output may hold no carriage return. `just live` sends each recording's request again and fails when the answer's shape has
changed, or its values while its commit IDs haven't, since answers about the same commits should
match. Capped searches and lists of refs compare only their shape: which results fill a cap varies,
and refs grow without a commit moving.
