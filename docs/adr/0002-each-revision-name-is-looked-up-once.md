# Each revision name is looked up once per answer

Looking up a revision the mirror lacks makes Sourcegraph fetch it, and a fetch can move a branch,
so two lookups of one name in an answer can disagree. On 2026-10-26 a diff of `develop...COMMIT`
took its merge base from a `develop` its own head lookup had just moved, and printed `0 files`.
Two lookups racing one fetch also fail with "revision not found after waiting for a fetch already
in progress". So every part of an answer reads the commit its one lookup resolved, and sego still
fetches a missing revision, since otherwise a commit pushed minutes ago is falsely not found.
