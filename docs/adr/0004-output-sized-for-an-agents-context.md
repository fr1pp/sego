# Output is sized for an agent's context

sego is written for coding agents, and an agent pays for everything a tool prints: each line costs
tokens, competes for a finite context window with the work the agent is doing, and stays there for
the rest of the session. Harnesses also cut long tool output, keeping only its start. So by default
sego prints a bounded answer, enough to act on and small enough to read whole, with the header and
notes first so a cut answer still says what it holds, and with any line too wide to be code worth
reading shortened. An agent that needs more asks for it, and an explicit request or `--json` gets
the whole of what it asked for. Time is bounded the same way: sego gives up before a typical
harness kills the command, and after the longest search the server allows.
