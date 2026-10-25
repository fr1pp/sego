use super::plan::{self, Filter as Planned, PatternType, Plan};
use super::query::{chosen, has_pattern, search_type};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    Term(String),
    Quoted(String),
    Pattern(String),
    Filter(Filter),
    Open,
    Close,
    Op(Op),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    pub negated: bool,
    pub field: String,
    pub value: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    And,
    Or,
    Not,
}

impl Filter {
    pub fn raw(&self) -> String {
        format!(
            "{}{}:{}",
            if self.negated { "-" } else { "" },
            self.field,
            self.value
        )
    }
}

/// Splits a query the way the server's parser does. A query whose group
/// parentheses don't balance is read again with every parenthesis as text.
pub fn tokenize(query: &str) -> Vec<Token> {
    let chars: Vec<char> = query.chars().collect();
    let closes = closes(&chars);
    scan(&chars, &closes, true)
        .or_else(|| scan(&chars, &closes, false))
        .unwrap_or_default()
}

/// For each `(`, the index after the `)` that balances it when the text is
/// read from that `(` with `\` escaping the next character, or `None`.
/// One pass from the start gives every answer: read from any `(`, the text
/// after it escapes the same way, so its `)` is the first one after it
/// where the depth falls one below the depth just after it.
fn closes(chars: &[char]) -> Vec<Option<usize>> {
    let mut after = vec![0i64; chars.len()];
    let mut active = vec![false; chars.len()];
    let (mut depth, mut i) = (0i64, 0);
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                after[i] = depth;
                if let Some(next) = after.get_mut(i + 1) {
                    *next = depth;
                }
                i += 2;
                continue;
            }
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        after[i] = depth;
        active[i] = true;
        i += 1;
    }
    let mut out = vec![None; chars.len()];
    let mut next_close: std::collections::HashMap<i64, usize> =
        std::collections::HashMap::new();
    for i in (0..chars.len()).rev() {
        match chars[i] {
            '(' => out[i] = next_close.get(&(after[i] - 1)).map(|at| at + 1),
            ')' if active[i] => {
                next_close.insert(after[i], i);
            }
            _ => {}
        }
    }
    out
}

struct Scanner<'a> {
    chars: &'a [char],
    closes: &'a [Option<usize>],
    pos: usize,
    groups: bool,
    depth: usize,
}

fn scan(
    chars: &[char],
    closes: &[Option<usize>],
    groups: bool,
) -> Option<Vec<Token>> {
    let mut scanner = Scanner { chars, closes, pos: 0, groups, depth: 0 };
    let mut tokens = Vec::new();
    while scanner.skip_space() {
        scanner.next(&mut tokens)?;
    }
    (scanner.depth == 0).then_some(tokens)
}

impl Scanner<'_> {
    fn skip_space(&mut self) -> bool {
        while self.chars.get(self.pos).is_some_and(|c| c.is_whitespace()) {
            self.pos += 1;
        }
        self.pos < self.chars.len()
    }

    fn text(&self, start: usize, end: usize) -> String {
        self.chars[start..end].iter().collect()
    }

    /// Reads one token or one group parenthesis. `None` means a `)` with no
    /// group to close.
    fn next(&mut self, tokens: &mut Vec<Token>) -> Option<()> {
        let start = self.pos;
        let c = self.chars[start];
        if self.groups
            && c == '('
            && let Some(end) = self.empty_parens_end(start)
        {
            self.pos = end;
            tokens.push(Token::Term("()".to_owned()));
            return Some(());
        }
        if self.groups && c == '(' {
            self.pos += 1;
            self.depth += 1;
            tokens.push(Token::Open);
            return Some(());
        }
        if self.groups && c == ')' {
            self.depth = self.depth.checked_sub(1)?;
            self.pos += 1;
            tokens.push(Token::Close);
            return Some(());
        }
        if let Some(end) = self.quoted_end(start) {
            self.pos = end;
            tokens.push(Token::Quoted(self.text(start, end)));
            return Some(());
        }
        if c == '/'
            && let Some(end) = self.pattern_end(start)
        {
            self.pos = end;
            tokens.push(Token::Pattern(self.text(start, end)));
            return Some(());
        }
        let (end, floor) = match self.filter_quote_end(start) {
            Some(end) => (end, end),
            None => self.plain_end(start),
        };
        let mut closes = 0;
        if self.groups {
            let mut trailing = 0;
            while end - trailing > floor
                && self.chars[end - 1 - trailing] == ')'
            {
                trailing += 1;
            }
            if trailing > self.depth {
                return None;
            }
            closes = trailing;
        }
        self.pos = end;
        tokens.push(classify(self.text(start, end - closes)));
        self.depth -= closes;
        tokens.extend(std::iter::repeat_n(Token::Close, closes));
        Some(())
    }

    /// The end of a plain token, and where the last balanced run ended: a
    /// `)` before that point belongs to the run, not to a group.
    fn plain_end(&self, start: usize) -> (usize, usize) {
        let mut i = start;
        let mut floor = start;
        while i < self.chars.len() && !self.chars[i].is_whitespace() {
            if self.chars[i] == '('
                && i > start
                && let Some(end) = self.balanced_end(i)
            {
                i = end;
                floor = end;
            } else {
                i += 1;
            }
        }
        (i, floor)
    }

    /// From the `(` at `open`, the index after its matching `)`.
    fn balanced_end(&self, open: usize) -> Option<usize> {
        self.closes[open]
    }

    /// `()` or `( )` standing alone is the text `()`, not an empty group.
    fn empty_parens_end(&self, open: usize) -> Option<usize> {
        let mut i = open + 1;
        while self.chars.get(i).is_some_and(|c| c.is_whitespace()) {
            i += 1;
        }
        (self.chars.get(i) == Some(&')') && self.terminal(i + 1))
            .then_some(i + 1)
    }

    fn terminal(&self, i: usize) -> bool {
        self.chars.get(i).is_none_or(|c| c.is_whitespace() || *c == ')')
    }

    /// A quote opens at `start` only when its first unescaped twin is
    /// followed by whitespace, `)` or the end.
    fn quoted_end(&self, start: usize) -> Option<usize> {
        self.delimited_end(start, true)
    }

    /// The end of the quoted run at `start`: the first unescaped twin of the
    /// opening quote. With `terminated`, that twin must be followed by
    /// whitespace, `)` or the end, or there is no run; without it, whatever
    /// follows the twin is accepted.
    fn delimited_end(&self, start: usize, terminated: bool) -> Option<usize> {
        let quote = *self.chars.get(start)?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        let mut i = start + 1;
        while i < self.chars.len() {
            if self.chars[i] == '\\' {
                i += 1;
            } else if self.chars[i] == quote {
                return (!terminated || self.terminal(i + 1)).then_some(i + 1);
            }
            i += 1;
        }
        None
    }

    fn pattern_end(&self, start: usize) -> Option<usize> {
        let mut i = start + 1;
        while i < self.chars.len() {
            if self.chars[i] == '\\' {
                i += 1;
            } else if self.chars[i] == '/' {
                return (i > start + 1 && self.terminal(i + 1))
                    .then_some(i + 1);
            }
            i += 1;
        }
        None
    }

    /// For a token that starts `-?word:` and then a quote that opens, the
    /// end of that quote.
    fn filter_quote_end(&self, start: usize) -> Option<usize> {
        let mut i = start;
        if self.chars.get(i) == Some(&'-') {
            i += 1;
        }
        let letters = i;
        while self.chars.get(i).is_some_and(char::is_ascii_alphabetic) {
            i += 1;
        }
        if i == letters || self.chars.get(i) != Some(&':') {
            return None;
        }
        self.delimited_end(i + 1, false)
    }
}

fn classify(text: String) -> Token {
    for (name, op) in [("and", Op::And), ("or", Op::Or), ("not", Op::Not)] {
        if text.eq_ignore_ascii_case(name) {
            return Token::Op(op);
        }
    }
    let (negated, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.as_str()),
    };
    if let Some((field, value)) = rest.split_once(':')
        && !field.is_empty()
        && field.chars().all(|c| c.is_ascii_alphabetic())
        && !value.is_empty()
        && !value.starts_with(':')
    {
        return Token::Filter(Filter {
            negated,
            field: field.to_owned(),
            value: value.to_owned(),
        });
    }
    Token::Term(text)
}

pub(super) const KNOWN: &[&str] = &[
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
const GUESSED: &[&str] =
    &["repogroup", "ext", "extension", "filename", "branch", "org"];
const RESULT_TYPES: &[&str] =
    &["file", "path", "repo", "symbol", "commit", "diff", "structural"];
const PATTERN_FIELDS: &[&str] =
    &["file", "f", "path", "lang", "l", "language", "content"];

/// The notes for a query, in check order. Checks 1-7 and 10 fire only on a
/// query that can't be meant as written; checks 8 and 9 fire on a
/// structural search whose answer the server can cut without saying so.
/// Checks 4, 8 and 9 read the query's plan, and skip a query the server
/// rejects while reading it.
pub fn check(tokens: &[Token], plan: &Plan) -> Vec<String> {
    let mut notes = Vec::new();
    unknown_filters(tokens, &mut notes);
    different_languages(&operands(tokens), &mut notes);
    let ty = search_type(tokens, plan);
    regex_in_keyword_mode(tokens, ty, &mut notes);
    if let Plan::Read(searches) = plan {
        escaped_or(searches, &mut notes);
    }
    unknown_types(tokens, &mut notes);
    path_without_pattern(tokens, &mut notes);
    revision_ranges(tokens, &mut notes);
    if let Plan::Read(searches) = plan {
        structural_scope(searches, &mut notes);
    }
    symbol_select_without_pattern(tokens, ty, &mut notes);
    notes
}

fn filters(tokens: &[Token]) -> impl Iterator<Item = &Filter> {
    tokens.iter().filter_map(|token| match token {
        Token::Filter(filter) => Some(filter),
        _ => None,
    })
}

fn is_field(filter: &Filter, names: &[&str]) -> bool {
    names.iter().any(|name| filter.field.eq_ignore_ascii_case(name))
}

/// The filters of each operand of OR, at every depth. A filter inside a
/// group belongs to the group's operands, not to the enclosing one.
fn operands(tokens: &[Token]) -> Vec<Vec<&Filter>> {
    let mut done = Vec::new();
    let mut stack: Vec<Vec<Vec<&Filter>>> = vec![vec![Vec::new()]];
    for token in tokens {
        match token {
            Token::Open => stack.push(vec![Vec::new()]),
            Token::Close if stack.len() > 1 => {
                done.extend(stack.pop().unwrap_or_default());
            }
            Token::Op(Op::Or) => {
                if let Some(level) = stack.last_mut() {
                    level.push(Vec::new());
                }
            }
            Token::Filter(filter) => {
                if let Some(operand) =
                    stack.last_mut().and_then(|l| l.last_mut())
                {
                    operand.push(filter);
                }
            }
            _ => {}
        }
    }
    done.extend(stack.into_iter().next().unwrap_or_default());
    done
}

fn unknown_filters(tokens: &[Token], notes: &mut Vec<String>) {
    let mut seen = Vec::new();
    for filter in filters(tokens) {
        let field = filter.field.to_ascii_lowercase();
        if KNOWN.contains(&field.as_str()) {
            continue;
        }
        if !GUESSED.contains(&field.as_str()) && !near_known(&field) {
            continue;
        }
        let raw = filter.raw();
        if seen.contains(&raw) {
            continue;
        }
        notes.push(format!(
            "'{}:' is not a filter; the server searches `{raw}` as text",
            filter.field
        ));
        seen.push(raw);
    }
}

/// One adjacent transposition, insertion or deletion from a known field of
/// four or more letters. A known field with a trailing `s` and `patch` are
/// common keys in code, so they are not near.
fn near_known(field: &str) -> bool {
    let plural =
        field.strip_suffix('s').is_some_and(|stem| KNOWN.contains(&stem));
    if plural || field == "patch" {
        return false;
    }
    let f: Vec<char> = field.chars().collect();
    KNOWN.iter().filter(|known| known.len() >= 4).any(|known| {
        let k: Vec<char> = known.chars().collect();
        match f.len().cmp(&k.len()) {
            std::cmp::Ordering::Equal => {
                (0..f.len().saturating_sub(1)).any(|i| {
                    let mut swapped = f.clone();
                    swapped.swap(i, i + 1);
                    swapped == k
                })
            }
            std::cmp::Ordering::Greater => {
                f.len() == k.len() + 1 && drops_one(&f, &k)
            }
            std::cmp::Ordering::Less => {
                k.len() == f.len() + 1 && drops_one(&k, &f)
            }
        }
    })
}

/// Whether removing one character from `long` gives `short`.
fn drops_one(long: &[char], short: &[char]) -> bool {
    (0..long.len())
        .any(|i| long[..i].iter().chain(&long[i + 1..]).eq(short.iter()))
}

fn different_languages(operands: &[Vec<&Filter>], notes: &mut Vec<String>) {
    let differ = operands.iter().any(|operand| {
        let mut langs = operand
            .iter()
            .filter(|f| !f.negated && is_field(f, &["lang", "l", "language"]))
            .map(|f| f.value.to_ascii_lowercase());
        langs.next().is_some_and(|first| langs.any(|v| v != first))
    });
    if differ {
        notes.push(
            "lang: filters intersect: a file must match both".to_owned(),
        );
    }
}

fn regex_in_keyword_mode(
    tokens: &[Token],
    ty: PatternType,
    notes: &mut Vec<String>,
) {
    if ty != PatternType::Keyword {
        return;
    }
    let first = tokens.iter().find_map(|token| match token {
        Token::Term(term)
            if looks_like_regex(term) && flags(term).is_none() =>
        {
            Some(term)
        }
        _ => None,
    });
    if let Some(term) = first {
        notes.push(format!(
            "keyword mode matches `{term}` literally; a regex goes inside /.../"
        ));
    }
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn looks_like_regex(term: &str) -> bool {
    let c: Vec<char> = term.chars().collect();
    let at = |i: usize| c.get(i).copied();
    if matches!(c.first(), Some('^'))
        && at(1)
            .is_some_and(|n| n.is_ascii_alphabetic() || n == '\\' || n == '[')
    {
        return true;
    }
    (0..c.len()).any(|i| match c[i] {
        '\\' => at(i + 1).is_some_and(|n| {
            (i > 0 && is_word(c[i - 1]) && ".()[]{}|*+?^/".contains(n))
                || ("wWsSdDbB".contains(n)
                    && !at(i + 2).is_some_and(|a| a.is_ascii_alphabetic()))
        }),
        '.' => {
            matches!(at(i + 1), Some('*' | '+'))
                && at(i + 2).is_some_and(|n| n != ';' && n != '.')
        }
        '[' => class_of_ranges(&c[i + 1..]),
        _ => false,
    })
}

/// Whether the bracket body starting `rest` holds two ranges, or one range
/// followed by a quantifier.
fn class_of_ranges(rest: &[char]) -> bool {
    let Some(end) = rest.iter().position(|&x| x == ']') else {
        return false;
    };
    let body = &rest[..end];
    let (mut ranges, mut i) = (0, 0);
    while i < body.len() {
        let range = match body[i..] {
            [x, '-', y, ..] => {
                x < y
                    && ((x.is_ascii_lowercase() && y.is_ascii_lowercase())
                        || (x.is_ascii_uppercase() && y.is_ascii_uppercase())
                        || (x.is_ascii_digit() && y.is_ascii_digit()))
            }
            _ => false,
        };
        if range {
            ranges += 1;
            i += 3;
        } else {
            i += 1;
        }
    }
    ranges >= 2
        || (ranges == 1
            && matches!(rest.get(end + 1), Some('+' | '*' | '?' | '{')))
}

/// Check 4: some search is scoped by a positive `repo:` filter and another
/// has no `repo:` filter at all, as when a filter sits beside one operand
/// of OR.
fn escaped_or(searches: &plan::Searches, notes: &mut Vec<String>) {
    let ends = searches.reach((false, false), |(positive, any), f| {
        let repo = f.field == "repo";
        (positive || (repo && !f.negated), any || repo)
    });
    if ends.iter().any(|end| end.0) && ends.iter().any(|end| !end.1) {
        notes.push(
            "filters outside parentheses apply only to the operand of OR \
             they sit in; the other operands search every repository"
                .to_owned(),
        );
    }
}

fn unknown_types(tokens: &[Token], notes: &mut Vec<String>) {
    let mut seen: Vec<&str> = Vec::new();
    for filter in
        filters(tokens).filter(|f| !f.negated && is_field(f, &["type"]))
    {
        let value = filter.value.as_str();
        let bare = unquoted(value);
        if RESULT_TYPES.contains(&bare) || seen.contains(&value) {
            continue;
        }
        seen.push(value);
        notes.push(format!(
            "type:{value} is not a result type; the server returns nothing"
        ));
    }
}

fn path_without_pattern(tokens: &[Token], notes: &mut Vec<String>) {
    let typed = filters(tokens)
        .any(|f| !f.negated && is_field(f, &["type"]) && f.value == "path");
    let only_filters = tokens.iter().all(|t| {
        matches!(
            t,
            Token::Filter(_) | Token::Open | Token::Close | Token::Op(_)
        )
    });
    if typed
        && only_filters
        && !filters(tokens).any(|f| is_field(f, PATTERN_FIELDS))
    {
        notes.push("type:path with only filters returns nothing".to_owned());
    }
}

fn revision_ranges(tokens: &[Token], notes: &mut Vec<String>) {
    for filter in filters(tokens).filter(|f| is_field(f, &["repo", "r"])) {
        let Some((_, revisions)) = filter.value.split_once('@') else {
            continue;
        };
        let end = revisions
            .split(':')
            .find(|rev| rev.contains(".."))
            .and_then(|rev| rev.rsplit_once("..").map(|(_, end)| end));
        if let Some(end) = end.filter(|end| !end.is_empty()) {
            let note = format!("{} searches {end} only", filter.raw());
            if !notes.contains(&note) {
                notes.push(note);
            }
        }
    }
}

const STRUCTURAL_SCOPE: &str = "a structural search over more than one file \
can miss matches without a +; file:^PATH$ makes it complete, and a regex \
search marks a cut with +";
const STRUCTURAL_LANGUAGE: &str = "with no lang: and no file: extension, a \
structural search can parse files as the wrong language and miss matches; \
lang: names the language";

/// The server runs a structural search only on the candidate files its
/// literals select, up to a cap, and reports no cut; it picks the matcher
/// from the first `lang:`, or with no `lang:` from the first `file:`
/// path's extension, and a generic one when Comby has none for either.
/// Each search of the plan is checked with its own filters. The server
/// rejects a structural search with `type:`, so that query gets neither
/// note.
fn structural_scope(searches: &plan::Searches, notes: &mut Vec<String>) {
    if searches.pattern_type != PatternType::Structural
        || searches.reach(false, |t, f| t || f.field == "type").contains(&true)
    {
        return;
    }
    let path = |f: &Planned| f.field == "file" && !f.negated && !f.predicate;
    let one_file = searches
        .reach(false, |one, f| one || (path(f) && one_literal_path(&f.value)));
    let language = searches.reach((false, None), |(lang, ext), f| {
        let named = lang || (f.field == "lang" && !f.negated);
        (named, ext.or_else(|| path(f).then(|| ends_in_extension(&f.value))))
    });
    if one_file.contains(&false) {
        notes.push(STRUCTURAL_SCOPE.to_owned());
    }
    if language.iter().any(|&(lang, ext)| !lang && ext != Some(true)) {
        notes.push(STRUCTURAL_LANGUAGE.to_owned());
    }
}

/// Whether the query can search only one repository: each search of its
/// plan has a non-negated `repo:` whose value is one exact name, the same in
/// every search. Two `repo:` filters in a search intersect, so one exact
/// name among them is enough. A plan of 64 or more searches isn't read.
pub fn one_repository(plan: &Plan) -> bool {
    let Some(searches) = plan.each() else {
        return false;
    };
    let mut names = searches.iter().map(|search| {
        search
            .iter()
            .filter(|f| f.field == "repo" && !f.negated)
            .find_map(|f| exact_repository(&f.value))
    });
    let Some(Some(first)) = names.next() else {
        return false;
    };
    names.all(|name| name == Some(first))
}

/// The name in a `repo:` value that matches one repository: `^NAME$`, with
/// an optional `@` revision, where NAME is `/`-separated segments that each
/// hold a letter or digit, made of letters, digits, `_`, `-` and `.`, each
/// punctuation mark (and `/`) escaped or not.
fn exact_repository(value: &str) -> Option<&str> {
    let name = value.split_once('@').map_or(value, |(name, _)| name);
    let body = name.strip_prefix('^')?.strip_suffix('$')?;
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        let literal = if c == '\\' {
            matches!(chars.next(), Some('.' | '-' | '_' | '/'))
        } else {
            c.is_ascii_alphanumeric() || "_-./".contains(c)
        };
        if !literal {
            return None;
        }
    }
    body.split('/')
        .all(|segment| segment.chars().any(|c| c.is_ascii_alphanumeric()))
        .then_some(body)
}

/// A `content:` filter or a path's content predicate supplies the pattern:
/// the server searches the predicate's argument as one. Negated,
/// the server rejects either, so neither gets the note. A field the server
/// doesn't know is searched as text. In literal and structural mode `/./`
/// is text, and the server rejects a second `patterntype:`, so the note
/// names the one to replace.
fn symbol_select_without_pattern(
    tokens: &[Token],
    ty: PatternType,
    notes: &mut Vec<String>,
) {
    let symbols = chosen(tokens, "type").any(|v| unquoted(v) == "symbol");
    let selected =
        chosen(tokens, "select").any(|v| unquoted(v).starts_with("symbol"));
    let supplies = |f: &Filter| {
        is_field(f, &["content"])
            || !is_field(f, KNOWN)
            || (is_field(f, &["file", "f", "path"])
                && (f.value.starts_with("has.content(")
                    || f.value.starts_with("contains.content(")))
    };
    if !symbols
        || !selected
        || has_pattern(tokens)
        || filters(tokens).any(supplies)
    {
        return;
    }
    let remedy =
        if matches!(ty, PatternType::Literal | PatternType::Structural) {
            let mode = ty.name();
            format!(
                "with patterntype:regexp in place of patterntype:{mode}, /./"
            )
        } else {
            "/./".to_owned()
        };
    notes.push(format!(
        "type:symbol with select:symbol and no pattern returns nothing; \
         {remedy} matches every symbol"
    ));
}

const FLAGS: &str = "gims";
const ROOTS: &[&str] = &[
    "bin", "boot", "dev", "etc", "home", "lib", "media", "mnt", "opt", "proc",
    "root", "run", "sbin", "srv", "sys", "tmp", "usr", "var",
];

/// The flags of `/RE/FLAGS`: a `/`, text up to the first unescaped `/`,
/// then one or more of `FLAGS`, each at most once, in any order. The server
/// searches the whole term as text. Text that is a top-level Unix directory
/// makes it a path, such as `/usr/i`.
fn flags(term: &str) -> Option<&str> {
    let rest = term.strip_prefix('/')?;
    let mut chars = rest.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '/' => {
                let (body, flags) = (&rest[..i], &rest[i + 1..]);
                let set = !flags.is_empty()
                    && flags.char_indices().all(|(at, flag)| {
                        FLAGS.contains(flag) && !flags[..at].contains(flag)
                    });
                return (!body.is_empty() && set && !ROOTS.contains(&body))
                    .then_some(flags);
            }
            _ => {}
        }
    }
    None
}

/// The note for the first `/RE/FLAGS` term in `keyword` or `standard`
/// mode. It is for a zero answer only: such a term can be meant as written,
/// as a regex literal in code or a path prefix.
pub fn flag_note(tokens: &[Token], ty: PatternType) -> Option<String> {
    if !matches!(ty, PatternType::Keyword | PatternType::Standard) {
        return None;
    }
    let (term, flags) = tokens.iter().find_map(|token| match token {
        Token::Term(term) => Some((term, flags(term)?)),
        _ => None,
    })?;
    let inline: String =
        ['s', 'm'].into_iter().filter(|&f| flags.contains(f)).collect();
    let mut clauses = Vec::new();
    if flags.contains('i') || inline.is_empty() {
        clauses.push(
            if chosen(tokens, "case").any(|v| unquoted(v) == "yes") {
                "case:yes matches case even with (?i)"
            } else {
                "it ignores case by default"
            },
        );
    }
    let example = format!("flags go inside: /(?{inline}).../");
    if !inline.is_empty() {
        clauses.push(&example);
    }
    Some(format!(
        "the server searches `{term}` as text; {}",
        clauses.join(", and ")
    ))
}

/// A quoted value without its quotes.
pub fn unquoted(value: &str) -> &str {
    ['"', '\'']
        .iter()
        .find_map(|q| value.strip_prefix(*q)?.strip_suffix(*q))
        .unwrap_or(value)
}

/// `^PATH$`, where PATH is literal text that names a file, not a
/// directory: `.` and escaped punctuation are allowed, and an escaped
/// letter or digit (`\d`, `\w`, `\b`) is a pattern.
fn one_literal_path(value: &str) -> bool {
    let Some(body) = value.strip_prefix('^').and_then(|v| v.strip_suffix('$'))
    else {
        return false;
    };
    if body.is_empty() || body.ends_with('/') {
        return false;
    }
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if chars.next().is_none_or(|n| n.is_ascii_alphanumeric()) {
                    return false;
                }
            }
            '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|'
            | '^' | '$' => return false,
            _ => {}
        }
    }
    true
}

/// Whether the value, less a trailing `$`, ends in `.` or `\.` and an
/// extension of lower-case letters and digits: the server matches the
/// extensions it knows, all lower-case, case-sensitively.
fn ends_in_extension(value: &str) -> bool {
    let value = value.strip_suffix('$').unwrap_or(value);
    let stem = value.trim_end_matches(|c: char| {
        c.is_ascii_lowercase() || c.is_ascii_digit()
    });
    stem.len() < value.len() && stem.ends_with('.')
}

#[cfg(test)]
mod tests;
