//! What a query writes, read as the server reads its text: groups, `and`,
//! `or` and `not`, filters, and quoted and `/`-delimited patterns.

/// The fields the server reads as filters, in any case.
pub const KNOWN: &[&str] = &[
    "repo",
    "r",
    "file",
    "f",
    "path",
    "lang",
    "l",
    "language",
    "content",
    "type",
    "case",
    "fork",
    "archived",
    "visibility",
    "rev",
    "revision",
    "context",
    "g",
    "patterntype",
    "select",
    "count",
    "timeout",
    "index",
    "before",
    "after",
    "since",
    "until",
    "author",
    "committer",
    "message",
    "m",
    "msg",
    "repohasfile",
    "repohascommitafter",
    "rule",
];

const TYPES: [(&str, PatternType); 8] = [
    ("keyword", PatternType::Keyword),
    ("standard", PatternType::Standard),
    ("lucky", PatternType::Standard),
    ("regexp", PatternType::Regexp),
    ("regex", PatternType::Regexp),
    ("literal", PatternType::Literal),
    ("structural", PatternType::Structural),
    ("codycontext", PatternType::CodyContext),
];

/// The fields whose value can be a predicate, such as `has.file(...)`, and
/// the predicates each takes.
const PREDICATES: [(&str, &str); 3] = [
    (
        "repo r",
        "contains.file has.file contains.path has.path contains.content \
         has.content contains.commit.after has.commit.after has.description \
         has.meta has.topic has.tag has has.key contains",
    ),
    ("file f path", "contains.content has.content has.owner has.contributor"),
    ("rev revision", "at.time"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatternType {
    Keyword,
    Standard,
    Regexp,
    Literal,
    Structural,
    CodyContext,
}

impl PatternType {
    pub fn named(value: &str) -> Option<Self> {
        TYPES.iter().find(|t| t.0 == value).map(|t| t.1)
    }

    pub fn name(self) -> &'static str {
        TYPES.iter().find(|t| t.1 == self).map_or("", |t| t.0)
    }
}

/// What a query writes, in the order written: each filter of a known field
/// and each pattern, as the server reads them apart, and each `or` it reads
/// as an operator. A filter with an empty value, which the server drops, is
/// left out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Written {
    Filter(WrittenFilter),
    /// A pattern's text, without a `not` before it, which negates it.
    /// `delimited` is a quoted or `/`-delimited pattern.
    Pattern {
        text: String,
        negated: bool,
        delimited: bool,
    },
    Or,
}

/// A filter as written: the field as spelled and the value with any quotes.
/// `negated` is a `-` before it or a `not`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WrittenFilter {
    pub negated: bool,
    pub field: String,
    pub value: String,
}

impl WrittenFilter {
    /// The filter as the query could restate it.
    pub fn raw(&self) -> String {
        format!("{}{}:{}", if self.negated { "-" } else { "" }, self.field, self.value)
    }
}

/// The values of the non-negated `name:` filters; a negated `type:` or
/// `select:` doesn't choose what the results are.
pub fn chosen<'a>(written: &'a [Written], name: &'a str) -> impl Iterator<Item = &'a str> {
    written.iter().filter_map(move |item| match item {
        Written::Filter(f) if !f.negated && f.field.eq_ignore_ascii_case(name) => {
            Some(f.value.as_str())
        }
        _ => None,
    })
}

/// Whether the query writes a pattern.
pub fn has_pattern(written: &[Written]) -> bool {
    written.iter().any(|item| matches!(item, Written::Pattern { .. }))
}

/// A quoted value without its quotes.
pub fn unquoted(value: &str) -> &str {
    ['"', '\''].iter().find_map(|q| value.strip_prefix(*q)?.strip_suffix(*q)).unwrap_or(value)
}

/// A query as read: its pattern type, and what it writes. A query the
/// server rejects while reading it writes nothing, in keyword mode.
pub struct Reading {
    pub pattern_type: PatternType,
    pub written: Vec<Written>,
}

pub fn read(query: &str) -> Reading {
    let rejected = Reading { pattern_type: PatternType::Keyword, written: Vec::new() };
    // Narrower indices keep the reading small; no command line comes near
    // 4 Gi characters.
    let Some(text) = Text::new(query) else { return rejected };
    let mut ty = PatternType::Keyword;
    if let Ok(literal) = groups_or_flat(&text, PatternType::Literal) {
        for item in &literal {
            steps(1);
            if let Written::Filter(f) = item
                && f.field.eq_ignore_ascii_case("patterntype")
            {
                ty = PatternType::named(unquoted(&f.value)).unwrap_or(ty);
            }
        }
    }
    match groups_or_flat(&text, ty) {
        Ok(written) => Reading { pattern_type: ty, written },
        Err(Halt::Reject | Halt::Missing) => rejected,
    }
}

#[cfg(test)]
thread_local! {
    static STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Counts `n` steps of reading, for the test that reading is linear: one for
/// each pass of a loop through the query or through the items read, and one
/// for each character copied out of it.
#[cfg(test)]
fn steps(n: usize) {
    STEPS.with(|steps| steps.set(steps.get() + n));
}

#[cfg(not(test))]
fn steps(_: usize) {}

fn ascii_space(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\x0B' | '\x0C' | '\r' | ' ')
}

fn known(letters: &[char]) -> bool {
    letters.len() <= 18 && {
        let field: String = letters.iter().map(char::to_ascii_lowercase).collect();
        KNOWN.contains(&field.as_str())
    }
}

fn starts(c: &[char], i: usize, word: &str) -> bool {
    word.chars().enumerate().all(|(k, w)| c.get(i + k).is_some_and(|c| c.eq_ignore_ascii_case(&w)))
}

/// The query's characters, with what a balanced run needs worked out once,
/// so that reading takes time linear in the query's length. A run starts
/// after a character that isn't `\`, so its escapes pair up as they do read
/// from the start of the query.
struct Text {
    c: Vec<char>,
    /// The end of the run of ASCII letters from each index.
    letters: Vec<u32>,
    /// The parentheses open before each index, counted up from the fewest
    /// any index has, so that the counts index an array from 0.
    depth: Vec<u32>,
    stop: Vec<bool>,
    /// The first unescaped whitespace or `)` at or after each index whose
    /// depth equals the index's.
    level_end: Vec<u32>,
    /// The first unescaped `(` or whitespace at or after each index that a
    /// stop follows.
    stopped: Vec<u32>,
    /// For an unescaped `(`, the index of its `)`, or `NONE`.
    close: Vec<u32>,
    first: usize,
}

const NONE: u32 = u32::MAX;

/// An index into a `Text`, which holds fewer than `NONE` characters.
fn ix(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(NONE)
}

impl Text {
    #[expect(clippy::many_single_char_names, reason = "indices into `c`")]
    fn new(query: &str) -> Option<Text> {
        let c: Vec<char> = query.chars().collect();
        let n = c.len();
        if ix(n) == NONE {
            return None;
        }
        let mut escaped = vec![false; n];
        let mut i = 0;
        while i < n {
            steps(1);
            if c[i] == '\\' && i + 1 < n {
                escaped[i + 1] = true;
                i += 1;
            }
            i += 1;
        }
        let change = |i: usize| -> i32 {
            match c[i] {
                '(' if !escaped[i] => 1,
                ')' if !escaped[i] => -1,
                _ => 0,
            }
        };
        let (mut d, mut low, mut high) = (0i64, 0i64, 0i64);
        for i in 0..n {
            steps(1);
            d += i64::from(change(i));
            (low, high) = (low.min(d), high.max(d));
        }
        let mut depth = vec![u32::try_from(-low).ok()?; n + 1];
        for i in 0..n {
            steps(1);
            depth[i + 1] = depth[i].checked_add_signed(change(i))?;
        }
        // The counts run from 0 to `high - low`, one entry each; a `(`
        // looks one level up, to a count some later index has.
        let levels = usize::try_from(high - low).ok()? + 1;
        let mut letters = vec![ix(n); n + 1];
        let mut stop = vec![false; n + 1];
        let mut level_end = vec![ix(n); n];
        let mut stopped = vec![ix(n); n + 1];
        let mut close = vec![NONE; n];
        let (mut near, mut near_close) = (vec![ix(n); levels], vec![NONE; levels]);
        for i in (0..n).rev() {
            steps(1);
            letters[i] = if c[i].is_ascii_alphabetic() { letters[i + 1] } else { ix(i) };
            let j = i + usize::from(c[i] == '-');
            let end = letters[j] as usize;
            stop[i] = (end > j && c.get(end) == Some(&':') && known(&c[j..end]))
                || ["and ", "or ", "not "].iter().any(|w| starts(&c, i, w));
            let (d, plain) = (depth[i] as usize, !escaped[i]);
            if plain && (c[i].is_whitespace() || c[i] == ')') {
                near[d] = ix(i);
            }
            if plain && c[i] == ')' {
                near_close[d] = ix(i);
            }
            if plain && c[i] == '(' {
                close[i] = near_close[d + 1];
            }
            level_end[i] = near[d];
            let opens = plain && (c[i] == '(' || c[i].is_whitespace());
            stopped[i] = if opens && stop[i + 1] { ix(i) } else { stopped[i + 1] };
        }
        let first = c.iter().position(|c| !c.is_whitespace()).unwrap_or(n);
        Some(Text { c, letters, depth, stop, level_end, stopped, close, first })
    }

    /// The end of the balanced run from `s`, or `None` when it fails.
    fn run(&self, s: usize) -> Option<usize> {
        let n = self.c.len();
        if s == n {
            return Some(n);
        }
        let end = self.level_end[s] as usize;
        let unbalanced = end == n && self.depth[n] != self.depth[s];
        let stopped = self.stopped[s] as usize;
        (!self.stop[s] && stopped >= end && !unbalanced).then_some(end)
    }

    /// The end of a plain run from `s`: to whitespace, or a parenthesis
    /// too with `parens`, a `\` keeping the next character with `escapes`.
    fn plain(&self, s: usize, parens: bool, escapes: bool) -> usize {
        let mut i = s;
        while let Some(&c) = self.c.get(i) {
            steps(1);
            if escapes && c == '\\' {
                i += 2;
                continue;
            }
            if c.is_whitespace() || (parens && (c == '(' || c == ')')) {
                break;
            }
            i += 1;
        }
        i.min(self.c.len())
    }

    /// The index of the unescaped character that closes the quote or slash
    /// at `open`.
    fn closing(&self, open: usize) -> Option<usize> {
        let mut i = open + 1;
        while let Some(&c) = self.c.get(i) {
            steps(1);
            if c == '\\' {
                i += 2;
            } else if c == self.c[open] {
                return Some(i);
            } else {
                i += 1;
            }
        }
        None
    }

    fn terminal(&self, i: usize) -> bool {
        self.c.get(i).is_none_or(|&c| ascii_space(c) || c == ')')
    }

    /// The end of a predicate value starting at `s`, for the field as
    /// written.
    fn predicate(&self, field: &str, s: usize) -> Option<usize> {
        let (_, names) =
            PREDICATES.iter().find(|(fields, _)| fields.split(' ').any(|f| f == field))?;
        let open = (s..self.c.len()).find(|&i| {
            steps(1);
            !(self.c[i].is_ascii_alphabetic() || self.c[i] == '.')
        })?;
        let name = self.text(s, open);
        let close = self.close[open];
        (close != NONE && names.split_whitespace().any(|n| n == name)).then(|| close as usize + 1)
    }

    fn text(&self, start: usize, end: usize) -> String {
        steps(end - start);
        self.c[start..end].iter().collect()
    }
}

enum Halt {
    Reject,
    Missing,
}

/// Reads the query in `ty` with groups, or, when a group is left open or
/// an operand is missing, with every parenthesis as text.
fn groups_or_flat(text: &Text, ty: PatternType) -> Result<Vec<Written>, Halt> {
    let mut groups = Reader::new(text, ty, false);
    match groups.read() {
        Ok(()) if groups.open == 0 => {
            // Standard and literal mode reject a sequence of patterns that
            // holds a group read as `and` or `or`.
            let strict = matches!(ty, PatternType::Standard | PatternType::Literal);
            return if strict && groups.mixed { Err(Halt::Reject) } else { Ok(groups.written) };
        }
        Ok(()) | Err(Halt::Missing) => {}
        Err(halt) => return Err(halt),
    }
    let mut flat = Reader::new(text, ty, true);
    match flat.read() {
        Ok(()) => Ok(flat.written),
        Err(_) => Err(Halt::Reject),
    }
}

/// What reading needs of an item: whether it holds a filter, and whether
/// it is a group of several items, read as `and` or `or`.
#[derive(Clone, Copy)]
struct Item {
    filters: bool,
    operator: bool,
}

const PATTERN: Item = Item { filters: false, operator: false };

/// Several items read as one, `and` or `or` when `operator`; one item is
/// itself.
fn combine(items: &[Item], operator: bool) -> Option<Item> {
    steps(items.len());
    match items {
        [] => None,
        [one] => Some(*one),
        many => Some(Item { filters: many.iter().any(|i| i.filters), operator }),
    }
}

/// An or-expression being read: its operands, the levels of the operand
/// being read, and the items of the level being read.
#[derive(Default)]
struct Frame {
    operands: Vec<Item>,
    levels: Vec<Item>,
    items: Vec<Item>,
}

impl Frame {
    /// Ends the level being read, noting in `mixed` a sequence of patterns
    /// that holds a group read as `and` or `or`.
    fn end_level(&mut self, mixed: &mut bool) -> Result<(), Halt> {
        if self.items.is_empty() {
            return Err(Halt::Missing);
        }
        steps(self.items.len());
        let (mut level, patterns): (Vec<Item>, Vec<Item>) =
            self.items.drain(..).partition(|i| i.filters);
        if patterns.len() > 1 {
            *mixed |= patterns.iter().any(|i| i.operator);
            level.push(PATTERN);
        } else {
            level.extend(patterns);
        }
        self.levels.extend(combine(&level, true));
        Ok(())
    }

    fn end_operand(&mut self) {
        let levels = std::mem::take(&mut self.levels);
        self.operands.extend(combine(&levels, true));
    }

    fn end(mut self, mixed: &mut bool) -> Result<Item, Halt> {
        self.end_level(mixed)?;
        self.end_operand();
        combine(&self.operands, true).ok_or(Halt::Missing)
    }
}

struct Reader<'t> {
    t: &'t Text,
    ty: PatternType,
    flat: bool,
    pos: usize,
    open: usize,
    mixed: bool,
    written: Vec<Written>,
}

impl<'t> Reader<'t> {
    fn new(t: &'t Text, ty: PatternType, flat: bool) -> Self {
        Reader { t, ty, flat, pos: 0, open: 0, mixed: false, written: Vec::new() }
    }

    fn write_pattern(&mut self, start: usize, end: usize, negated: bool, delimited: bool) {
        let text = self.t.text(start, end);
        self.written.push(Written::Pattern { text, negated, delimited });
    }

    /// A `)` ends the innermost or-expression being read, unless `or`
    /// follows it: then that or-expression carries on, though its group is
    /// closed.
    fn read(&mut self) -> Result<(), Halt> {
        let t = self.t;
        let mut frames = vec![Frame::default()];
        loop {
            steps(1);
            while t.c.get(self.pos).is_some_and(|c| c.is_whitespace()) {
                steps(1);
                self.pos += 1;
            }
            let Some(frame) = frames.last_mut() else { unreachable!() };
            let Some(&c) = t.c.get(self.pos) else {
                let mut child = None;
                while let Some(mut frame) = frames.pop() {
                    steps(1);
                    frame.items.extend(child);
                    child = Some(frame.end(&mut self.mixed)?);
                }
                return Ok(());
            };
            if !self.flat && c == '(' {
                if self.ty != PatternType::Keyword
                    && let Some(end) = t.run(self.pos)
                {
                    frame.items.push(PATTERN);
                    self.write_pattern(self.pos, end, false, false);
                    self.pos = end;
                    continue;
                }
                self.open += 1;
                self.pos += 1;
                frames.push(Frame::default());
            } else if !self.flat && c == ')' {
                self.open = self.open.checked_sub(1).ok_or(Halt::Reject)?;
                if frame.items.is_empty() {
                    frame.items.push(PATTERN);
                    // The server reads an empty group as the pattern `()`.
                    let empty = Written::Pattern {
                        text: "()".to_owned(),
                        negated: false,
                        delimited: false,
                    };
                    self.written.push(empty);
                }
                self.pos += 1;
                if starts(&t.c, self.pos, "or") {
                    self.written.push(Written::Or);
                    self.pos += 2;
                    frame.end_level(&mut self.mixed)?;
                    frame.end_operand();
                } else if let Some(frame) = frames.pop() {
                    let group = frame.end(&mut self.mixed)?;
                    let Some(outer) = frames.last_mut() else { unreachable!() };
                    outer.items.push(group);
                }
            } else if let Some((or, len)) = self.operator() {
                frame.end_level(&mut self.mixed)?;
                if or {
                    self.written.push(Written::Or);
                    frame.end_operand();
                }
                self.pos += len;
            } else {
                let item = self.item()?;
                let Some(frame) = frames.last_mut() else { unreachable!() };
                frame.items.push(item);
            }
        }
    }

    fn operator(&self) -> Option<(bool, usize)> {
        let (c, s) = (&self.t.c, self.pos);
        if s <= self.t.first || !(ascii_space(c[s - 1]) || c[s - 1] == ')') {
            return None;
        }
        [("and", false), ("or", true)].into_iter().find_map(|(word, or)| {
            let after = c.get(s + word.len());
            (starts(c, s, word) && after.is_some_and(|&a| ascii_space(a) || a == '('))
                .then_some((or, word.len()))
        })
    }

    fn item(&mut self) -> Result<Item, Halt> {
        let (c, start) = (&self.t.c, self.pos);
        let after_space = start == 0 || {
            let before = c[start - 1];
            ascii_space(before) || before == '(' || before == ')'
        };
        let not = after_space
            && starts(c, start, "not")
            && c.get(start + 3).is_some_and(|&a| ascii_space(a));
        if not {
            self.pos += 3;
            while c.get(self.pos).is_some_and(|c| c.is_whitespace()) {
                steps(1);
                self.pos += 1;
            }
            if c.get(self.pos) == Some(&'(') {
                return Err(Halt::Reject);
            }
        }
        if self.filter(not)? {
            return Ok(Item { filters: true, operator: false });
        }
        self.pattern(not);
        Ok(PATTERN)
    }

    /// Reads a filter at the current position, negated after `not`. `false`
    /// when there is none, or when a quoted value after `not` is malformed,
    /// which leaves the text after the `:` to read.
    fn filter(&mut self, not: bool) -> Result<bool, Halt> {
        let (t, s) = (self.t, self.pos);
        let Some(&first) = t.c.get(s) else { return Ok(false) };
        let dash = first == '-';
        let letters = s + usize::from(dash);
        let colon = t.letters[letters] as usize;
        if colon == letters || t.c.get(colon) != Some(&':') || !known(&t.c[letters..colon]) {
            return Ok(false);
        }
        let field = t.text(letters, colon);
        let v = colon + 1;
        let (end, empty) = if matches!(t.c.get(v), Some('"' | '\'')) {
            match t.closing(v) {
                Some(close) => (close + 1, close == v + 1),
                None if not => {
                    self.pos = v;
                    return Ok(false);
                }
                None => return Err(Halt::Reject),
            }
        } else {
            let end = t
                .predicate(&field, v)
                .unwrap_or_else(|| t.run(v).unwrap_or_else(|| t.plain(v, true, true)));
            (end, end == v)
        };
        if dash && not {
            return Err(Halt::Reject);
        }
        self.pos = end;
        if !empty {
            let written = WrittenFilter { negated: dash || not, field, value: t.text(v, end) };
            self.written.push(Written::Filter(written));
        }
        Ok(true)
    }

    fn pattern(&mut self, negated: bool) {
        let (t, s, ty) = (self.t, self.pos, self.ty);
        let slashes = !matches!(ty, PatternType::Literal | PatternType::Structural);
        let quotes = matches!(ty, PatternType::Keyword | PatternType::Regexp);
        let close = match t.c.get(s) {
            Some('/') if slashes => t.closing(s),
            Some('"' | '\'') if quotes => t.closing(s),
            _ => None,
        };
        if let Some(c) = close.filter(|&c| t.terminal(c + 1)) {
            self.pos = c + 1;
            self.write_pattern(s, self.pos, negated, true);
            return;
        }
        self.pos = match (!self.flat).then(|| t.run(s)).flatten() {
            Some(end) => end,
            None if ty == PatternType::Regexp => t.plain(s, !self.flat, true),
            None => t.plain(s, false, false),
        };
        self.write_pattern(s, self.pos, negated, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The steps `read` takes through `head`, then `units` repeats of `unit`,
    /// then `tail` as often, and the query's length in characters.
    fn steps_reading(head: &str, unit: &str, tail: &str, units: usize) -> (usize, usize) {
        let query = format!("{head}{}{}", unit.repeat(units), tail.repeat(units));
        STEPS.with(|steps| steps.set(0));
        read(&query);
        (STEPS.with(std::cell::Cell::get), query.chars().count())
    }

    /// A query sixteen times as long takes at most sixteen times the steps:
    /// steps that grow with the square of the length would number some 256
    /// times as many. Every shape takes at least a step a character, so a
    /// count that stopped counting couldn't pass.
    #[test]
    fn groups_are_read_at_any_depth() {
        let nested = |depth| format!("{}repo:x foo{}", "(".repeat(depth), ")".repeat(depth));
        for depth in [64, 65, 100_000] {
            assert_eq!(read(&nested(depth)).written.len(), 2, "{depth}");
        }
    }

    #[test]
    fn unbalanced_parentheses_are_read_in_linear_time() {
        let started = std::time::Instant::now();
        read(&format!("{}foo", "\\(".repeat(60_000)));
        read(&format!("{}foo", "(".repeat(120_000)));
        let elapsed = started.elapsed();
        assert!(elapsed < std::time::Duration::from_secs(5), "{elapsed:?}");
    }

    #[test]
    fn a_query_is_read_in_linear_time() {
        let nests = ["(f:x a ", "(a f:x ", "(f:x a b ", "(-f:x a ", "(not f:x a ", "(content:x a "];
        let mut shapes: Vec<(&str, &str, &str)> = vec![
            ("", "f:a ", ""),
            ("", "\"a ", ""),
            ("", "a(b ", ""),
            ("", "foo or ", ""),
            ("", "repo:x foo or bar ", ""),
            ("", "(repo:a or repo:b) ", ""),
            ("patterntype:standard ", "( a", ""),
            ("", "(a)or x ", ""),
            ("", "(a b ", ""),
            ("", "a or (", ""),
            ("", "x and (a ", ""),
            ("", "(", ""),
        ];
        for unit in nests {
            shapes.push(("", unit, ")"));
        }
        for head in [
            "patterntype:regexp ",
            "patterntype:literal ",
            "patterntype:standard ",
            "patterntype:structural ",
        ] {
            shapes.push((head, "(f:x a ", ")"));
        }
        for (head, unit, tail) in shapes {
            let (small, chars) = steps_reading(head, unit, tail, 64);
            let (large, _) = steps_reading(head, unit, tail, 16 * 64);
            assert!(small >= chars, "{head}{unit:?}: {small} steps for {chars} characters");
            assert!(
                large <= 16 * small,
                "{head}{unit:?}: {large} steps, against {small} for a sixteenth"
            );
        }
    }
}
