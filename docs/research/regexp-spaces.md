# Spaces in `patterntype:regexp`

How the server reads a space between terms in regexp mode, what ends that reading, and when
`patterntype:regexp` applies at all. Written 2026-10-30.

Counts are for gorilla/mux at its default branch, at `count:all`, with `case:yes`.

## 1. A space joins terms with `.*?`

- **Consecutive patterns become one regular expression.** **live-verified**
  - `sourcegraph-public-snapshot@c864f15af264:internal/search/query/transformer.go:299`
    (`fuzzyRegexp`) writes `^func Test\w*\(` as `(?:^func).*?(?:Test\w*\()`.
  - `^func Test\w*\(` returned 75 matches; `^func\ Test\w*\(`, `/^func Test\w*\(/` and
    `^func\sTest\w*\(` returned 74. The extra line is `func getPopulateContextTestCases() []struct {`
    (mux_test.go:3018).
- **Any run of spaces is one join.** **live-verified**
  - `^func  Test\w*\(`, with two spaces, returned 75.
- **Quoted and `/.../` terms join too.** **source (snapshot)**
  - `"func" Test`, `/^func/ /Test/` and `"a b" c` all come out as `(?:…).*?(?:…)`.
- **A filter between terms doesn't stop it.** **live-verified**
  - `sourcegraph-public-snapshot@c864f15af264:internal/search/query/parser.go:944`
    (`partitionParameters`) reads filters apart before patterns are joined.
  - `^func lang:go Test\w*\(` returned 73, near the join's 75 and far from the 206 of `and` (§2).

## 2. What ends a join

- **`and`, `or` and `not` do.** **live-verified** for `and` and `not`
  - `^func and Test\w*\(` returned 206: the terms are two searches, anded at file level.
  - `^func Test or ^func Bench` reads as `(or (?:^func).*?(?:Test) (?:^func).*?(?:Bench))`.
- **After `not`, a join starts again.** **source (snapshot)**
  - `a not b c d` reads as `(and "a" (not "b") (?:c).*?(?:d))`.
- **A sequence that opens with `not` joins nothing.** **live-verified**
  - `sourcegraph-public-snapshot@c864f15af264:internal/search/query/transformer.go:394`
    (`substituteConcat`) tracks the previous operand from the first, and a negated first operand
    never lets it advance: `not a b c d` reads as
    `(and (not "a") "b" "c" "d")`.
  - `^func Bench` returned 9. `not zzqqxx ^func Bench` and `^func not zzqqxx Bench` both returned 91.

## 3. A space that isn't a join

- **Inside balanced parentheses, a space is a space.** **live-verified**
  - `^(func Test\w*)\(` returned 74.
- **Between groups, it is a join.** **source (snapshot)**
  - `(func) (Test\w*\()` reads as `(?:(func)).*?(?:(Test\w*\())`.
- **Inside brackets, it splits the term.** **live-verified**
  - `^func[ ]Test\w*\(` reads as `(?:^func[).*?(?:]Test\w*\()`, a valid regular expression whose
    `[…]` now spans the join. It returned 0. `^func[\ ]Test\w*\(` returned 74.
  - `a{1, 2}` splits the same way.

## 4. When `patterntype:` doesn't apply

- **A group of `or` or of a filter and a pattern, beside another pattern, drops it, for every
  mode.** **live-verified** for `regexp` and `literal`, **source (snapshot)** for the others
  - `sourcegraph-public-snapshot@c864f15af264:internal/search/client/client.go:281` finds
    `patterntype:` with a literal parse, and a parse that fails leaves the default.
  - `sourcegraph-public-snapshot@c864f15af264:internal/search/query/validate.go:432` fails a literal
    parse when a sequence of patterns holds an `and` or `or` group. On sourcegraph.com the default
    is keyword, so the regular expression is searched as text.
  - `patterntype:regexp`: `rout.r` returned 836; `rout.r (mux or Mux)` and
    `(case:yes ^func) (Test\w*\()` returned 0.
  - `patterntype:literal "func" (Router or router)`, with `case:no`, returned 1625, reading the
    quotes as a phrase, as keyword mode does. In literal mode they are text: with `and` between the
    two, it returned 0.
- **`and` between them keeps the mode.** **live-verified** for `regexp`, **source (snapshot)** for
  the others
  - `patterntype:regexp rout.r and (mux or Mux)`, which leaves no such sequence, returned 635.
- **A group of filters alone doesn't.** **source (snapshot)**
  - `(repo:x lang:go) a b` stays in regexp mode and joins `a` and `b`.
