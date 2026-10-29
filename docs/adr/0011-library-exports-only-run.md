# The library exports only `run`, and tests outside it go through it

sego's library exports `run`, the inputs it takes and the HTTP transport `main` hands it, and
nothing else, so every test outside the library tests sego the way an agent uses it. Re-exporting
private modules for tests once put two thirds of the suite past that interface, where a refactor
broke tests far from the code it changed.
