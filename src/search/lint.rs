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

impl Token {
    /// The token as written. An operator's case is not kept, so it comes
    /// back lower-case.
    pub fn raw(&self) -> String {
        match self {
            Token::Term(text) | Token::Quoted(text) | Token::Pattern(text) => {
                text.clone()
            }
            Token::Filter(filter) => format!(
                "{}{}:{}",
                if filter.negated { "-" } else { "" },
                filter.field,
                filter.value
            ),
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

    fn terminal(&self, i: usize) -> bool {
        self.chars.get(i).is_none_or(|c| c.is_whitespace() || *c == ')')
    }

    /// A quote opens at `start` only when its first unescaped twin is
    /// followed by whitespace, `)` or the end.
    fn quoted_end(&self, start: usize) -> Option<usize> {
        let quote = *self.chars.get(start)?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        let mut i = start + 1;
        while i < self.chars.len() {
            if self.chars[i] == '\\' {
                i += 1;
            } else if self.chars[i] == quote {
                return self.terminal(i + 1).then_some(i + 1);
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
            } else if self.chars[i] == '/' && self.terminal(i + 1) {
                return Some(i + 1);
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
        self.quoted_end(i + 1)
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
