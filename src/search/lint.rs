use super::query::pattern_type;

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

impl Token {
    /// The token as written. An operator's case is not kept, so it comes
    /// back lower-case.
    pub fn raw(&self) -> String {
        match self {
            Token::Term(text) | Token::Quoted(text) | Token::Pattern(text) => {
                text.clone()
            }
            Token::Filter(filter) => filter.raw(),
            Token::Open => "(".to_owned(),
            Token::Close => ")".to_owned(),
            Token::Op(Op::And) => "and".to_owned(),
            Token::Op(Op::Or) => "or".to_owned(),
            Token::Op(Op::Not) => "not".to_owned(),
        }
    }
}

/// Splits a query the way the server's parser does. A query whose group
/// parentheses don't balance is read again with every parenthesis as text.
pub fn tokenize(query: &str) -> Vec<Token> {
    let chars: Vec<char> = query.chars().collect();
    scan(&chars, true).or_else(|| scan(&chars, false)).unwrap_or_default()
}

struct Scanner<'a> {
    chars: &'a [char],
    pos: usize,
    groups: bool,
    depth: usize,
}

fn scan(chars: &[char], groups: bool) -> Option<Vec<Token>> {
    let mut scanner = Scanner { chars, pos: 0, groups, depth: 0 };
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
        let mut depth = 0;
        let mut i = open;
        while i < self.chars.len() {
            match self.chars[i] {
                '\\' => i += 1,
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i + 1);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        None
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

const KNOWN: &[&str] = &[
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

/// The notes for a query, in check order. Each check fires only on a query
/// that can't be meant as written.
pub fn check(tokens: &[Token]) -> Vec<String> {
    let operands = Operands::of(tokens);
    let mut notes = Vec::new();
    unknown_filters(tokens, &mut notes);
    different_languages(&operands, &mut notes);
    regex_in_keyword_mode(tokens, &mut notes);
    escaped_or(tokens, &operands, &mut notes);
    unknown_types(tokens, &mut notes);
    path_without_pattern(tokens, &mut notes);
    revision_ranges(tokens, &mut notes);
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

/// One operand of OR: its own filters, and whether a group inside it holds
/// a `repo:` filter.
#[derive(Default)]
struct Operand<'a> {
    filters: Vec<&'a Filter>,
    nested_repo: bool,
}

impl Operand<'_> {
    fn repo(&self) -> bool {
        self.nested_repo || self.filters.iter().any(|f| is_repo(f))
    }
}

fn is_repo(filter: &Filter) -> bool {
    is_field(filter, &["repo", "r"])
}

/// The operands of OR at every depth. A filter inside a group belongs to the
/// group's operands, not to the enclosing one.
struct Operands<'a> {
    all: Vec<Operand<'a>>,
    top: Vec<Operand<'a>>,
}

impl<'a> Operands<'a> {
    fn of(tokens: &'a [Token]) -> Self {
        let mut done: Vec<Operand<'a>> = Vec::new();
        let mut stack: Vec<Vec<Operand<'a>>> = vec![vec![Operand::default()]];
        for token in tokens {
            match token {
                Token::Open => stack.push(vec![Operand::default()]),
                Token::Close if stack.len() > 1 => {
                    let level = stack.pop().unwrap_or_default();
                    let repo = level.iter().any(Operand::repo);
                    if let Some(operand) =
                        stack.last_mut().and_then(|l| l.last_mut())
                    {
                        operand.nested_repo |= repo;
                    }
                    done.extend(level);
                }
                Token::Op(Op::Or) => {
                    if let Some(level) = stack.last_mut() {
                        level.push(Operand::default());
                    }
                }
                Token::Filter(filter) => {
                    if let Some(operand) =
                        stack.last_mut().and_then(|l| l.last_mut())
                    {
                        operand.filters.push(filter);
                    }
                }
                _ => {}
            }
        }
        let top = stack.into_iter().next().unwrap_or_default();
        done.extend(top.iter().map(|o| Operand {
            filters: o.filters.clone(),
            nested_repo: o.nested_repo,
        }));
        Operands { all: done, top }
    }
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

fn different_languages(operands: &Operands, notes: &mut Vec<String>) {
    let differ = operands.all.iter().any(|operand| {
        let mut langs = operand
            .filters
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

fn regex_in_keyword_mode(tokens: &[Token], notes: &mut Vec<String>) {
    if pattern_type(tokens) != "keyword" {
        return;
    }
    let first = tokens.iter().find_map(|token| match token {
        Token::Term(term) if looks_like_regex(term) => Some(term),
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

/// The server hoists the filters around a flat run of operands of OR to
/// apply to all of them.
fn hoisted(tokens: &[Token]) -> bool {
    let is_filter = |t: &Token| matches!(t, Token::Filter(_));
    let lead = tokens.iter().take_while(|t| is_filter(t)).count();
    let run_end = lead
        + tokens[lead..]
            .iter()
            .take_while(|t| {
                !is_filter(t) && !matches!(t, Token::Open | Token::Close)
            })
            .count();
    let run = &tokens[lead..run_end];
    let operand = |t: Option<&Token>| {
        matches!(
            t,
            Some(Token::Term(_) | Token::Quoted(_) | Token::Pattern(_))
        )
    };
    lead > 0
        && operand(run.first())
        && operand(run.last())
        && tokens[run_end..].iter().all(is_filter)
}

fn escaped_or(tokens: &[Token], operands: &Operands, notes: &mut Vec<String>) {
    let has_or = tokens
        .iter()
        .scan(0usize, |depth, t| {
            match t {
                Token::Open => *depth += 1,
                Token::Close => *depth = depth.saturating_sub(1),
                _ => {}
            }
            Some((*depth, t))
        })
        .any(|(depth, t)| depth == 0 && matches!(t, Token::Op(Op::Or)));
    let positive = |operand: &Operand| {
        operand.filters.iter().any(|f| !f.negated && is_repo(f))
    };
    if has_or
        && operands.top.iter().any(positive)
        && !operands.top.iter().all(Operand::repo)
        && !hoisted(tokens)
    {
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
        let bare = ['"', '\'']
            .iter()
            .find_map(|q| value.strip_prefix(*q)?.strip_suffix(*q))
            .unwrap_or(value);
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
