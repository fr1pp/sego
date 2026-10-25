# Output and time limits are sized to Claude Code's Bash tool

sego's main users are agents in Claude Code. Its limits come from that tool:

- Claude Code inlines up to 30,000 characters of Bash output, and over that shows only the first
  2,000. So each default limit keeps a typical answer near 20,000 characters. For example, `show`
  defaults to 500 lines, which measured a median of 20,050 characters over 2,068 files. Notes and the
  header come first, so a preview of the head still shows the counts.
- Claude Code times Bash out at 120 s, and the server allows searches up to 60 s. sego's client
  deadline of 90 s sits between the two.
- A line wider than 500 columns is cut in text, with a pointer to `--json`, which always carries the
  full line.

An explicit request is fetched in full, with no size bound. There is no tool-wide output budget.
