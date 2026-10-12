use std::time::Duration;

use super::lint::{Filter, Op, Token, check, tokenize};

const MAX_DEADLINE_MS: u64 = 60_000;
const DEFAULT_DEADLINE_MS: u64 = 20_000;
const DEFAULT_CAP: u64 = 30;
const SCOPE_FIELDS: [&str; 12] = [
    "repo",
    "r",
    "file",
    "f",
    "path",
    "lang",
    "l",
    "language",
    "fork",
    "archived",
    "visibility",
    "context",
];
pub const UNINDEXED_PATTERN: &str = r"^NOT-INDEXED:\ (exceeds\ the\ maximum\ size\ limit|contains\ too\ many\ trigrams)$";
const PATTERN_TYPES: [&str; 8] = [
    "standard",
    "regex",
    "regexp",
    "literal",
    "structural",
    "codycontext",
    "keyword",
    "lucky",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Matches,
    Symbols,
    Files,
    Repos,
}

impl Unit {
    pub fn plural(self) -> &'static str {
        match self {
            Unit::Matches => "matches",
            Unit::Symbols => "symbols",
            Unit::Files => "files",
            Unit::Repos => "repos",
        }
    }

    pub fn singular(self) -> &'static str {
        match self {
            Unit::Matches => "match",
            Unit::Symbols => "symbol",
            Unit::Files => "file",
            Unit::Repos => "repo",
        }
    }
}

pub struct Prepared {
    pub query: String,
    pub pattern_type: String,
    pub cap: Option<u64>,
    pub unit: Unit,
    pub lower_bound: bool,
    pub deadline_ms: u64,
    pub unindexed: Option<String>,
    pub notes: Vec<String>,
}

pub fn prepare(query: &str) -> Prepared {
    let tokens = tokenize(query);
    let deadline_ms = deadline_ms(&tokens);
    Prepared {
        query: query.to_owned(),
        pattern_type: pattern_type(&tokens),
        notes: check(&tokens),
        cap: cap(&tokens),
        unit: unit(&tokens),
        lower_bound: lower_bound(&tokens),
        unindexed: unindexed_query(&tokens, deadline_ms),
        deadline_ms,
    }
}

fn field(token: &Token, name: &str) -> bool {
    matches!(token, Token::Filter(f) if f.field.eq_ignore_ascii_case(name))
}

fn values<'a>(
    tokens: &'a [Token],
    name: &'a str,
) -> impl Iterator<Item = &'a str> {
    tokens.iter().filter_map(move |token| match token {
        Token::Filter(f) if f.field.eq_ignore_ascii_case(name) => {
            Some(f.value.as_str())
        }
        _ => None,
    })
}

/// How many results the query asks for: the last `count:` value, 30 when
/// there is none. `all`, or a value that isn't a number, is no cap.
pub fn cap(tokens: &[Token]) -> Option<u64> {
    let Some(value) = values(tokens, "count").last() else {
        return Some(DEFAULT_CAP);
    };
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

/// What the results count, from the query's `select:` and `type:` values.
pub fn unit(tokens: &[Token]) -> Unit {
    let selects = || values(tokens, "select");
    let types = || values(tokens, "type");
    let has_or = tokens.iter().any(|t| matches!(t, Token::Op(Op::Or)));
    if selects().any(|v| v.starts_with("repo")) || types().any(|v| v == "repo")
    {
        Unit::Repos
    } else if selects().any(|v| v.starts_with("file"))
        || types().any(|v| v == "path")
    {
        Unit::Files
    } else if types().any(|v| v == "symbol")
        && types().all(|v| v == "symbol")
        && !has_or
    {
        Unit::Symbols
    } else if types().any(|v| v == "commit" || v == "diff")
        || tokens.iter().any(|token| {
            matches!(
                token,
                Token::Term(_) | Token::Quoted(_) | Token::Pattern(_)
            )
        })
    {
        Unit::Matches
    } else {
        Unit::Files
    }
}

/// Whether the count can fall short of the answer without `limitHit`: a
/// diff's count is of highlights in truncated text, and the server cuts a
/// `select:file.directory` answer at the default cap without saying so.
pub fn lower_bound(tokens: &[Token]) -> bool {
    values(tokens, "type").any(|value| value == "diff")
        || values(tokens, "select").any(|value| value == "file.directory")
}

pub fn duration_text(ms: u64) -> String {
    if ms.is_multiple_of(1000) {
        format!("{}s", ms / 1000)
    } else {
        format!("{ms}ms")
    }
}

/// `ms` in seconds, rounded to one decimal place.
pub fn tenths(ms: u64) -> String {
    let tenths = (ms + 50) / 100;
    format!("{}.{}", tenths / 10, tenths % 10)
}

/// A second search for the files the index skipped, within the query's
/// scope. `None` when the query's scope can't be restated that way.
pub fn unindexed_query(tokens: &[Token], deadline_ms: u64) -> Option<String> {
    let filters = || {
        tokens.iter().filter_map(|token| match token {
            Token::Filter(f) => Some(f),
            _ => None,
        })
    };
    let is = |f: &Filter, names: &[&str]| {
        names.iter().any(|name| f.field.eq_ignore_ascii_case(name))
    };
    let repo = |f: &Filter| is(f, &["repo", "r"]);
    let eligible = filters().any(|f| repo(f) && !f.negated)
        && !tokens.iter().any(|t| matches!(t, Token::Op(Op::Or | Op::Not)))
        && !filters().any(|f| repo(f) && f.value.contains('@'))
        && !filters().any(|f| is(f, &["rev", "revision"]))
        && !values(tokens, "type")
            .any(|v| matches!(v, "commit" | "diff" | "repo"));
    let scope: Vec<String> =
        filters().filter(|f| is(f, &SCOPE_FIELDS)).map(Filter::raw).collect();
    if !eligible || scope.iter().any(|raw| raw.contains(char::is_whitespace)) {
        return None;
    }
    Some(format!(
        "{} type:file select:file index:only patterntype:regexp {UNINDEXED_PATTERN} count:all timeout:{}",
        scope.join(" "),
        duration_text(deadline_ms),
    ))
}

/// The last `patterntype:` value the server accepts, as written. `regex`
/// is reported as `regexp`; with none, the server searches by keyword.
pub fn pattern_type(tokens: &[Token]) -> String {
    let value = tokens
        .iter()
        .rev()
        .find_map(|token| match token {
            Token::Filter(f)
                if !f.negated
                    && f.field.eq_ignore_ascii_case("patterntype")
                    && PATTERN_TYPES.contains(&f.value.as_str()) =>
            {
                Some(f.value.as_str())
            }
            _ => None,
        })
        .unwrap_or("keyword");
    if value == "regex" { "regexp" } else { value }.to_owned()
}

/// How long the search may run: the first `timeout:`, at most a minute;
/// without one, a minute when the query has a `count:`, else 20 seconds.
pub fn deadline_ms(tokens: &[Token]) -> u64 {
    let Some(Token::Filter(timeout)) =
        tokens.iter().find(|token| field(token, "timeout"))
    else {
        return if tokens.iter().any(|token| field(token, "count")) {
            MAX_DEADLINE_MS
        } else {
            DEFAULT_DEADLINE_MS
        };
    };
    match parse(unquoted(&timeout.value)) {
        Some((true, _)) => 0,
        Some((false, d)) => u64::try_from(d.as_nanos().div_ceil(1_000_000))
            .map_or(MAX_DEADLINE_MS, |ms| ms.min(MAX_DEADLINE_MS)),
        None => MAX_DEADLINE_MS,
    }
}

fn unquoted(value: &str) -> &str {
    ['"', '\'']
        .iter()
        .find_map(|q| value.strip_prefix(*q)?.strip_suffix(*q))
        .unwrap_or(value)
}

/// Go's `time.ParseDuration`, for a duration that isn't negative. A
/// negative one, or text Go rejects, is `None`. `-0` is zero.
pub fn go_duration(text: &str) -> Option<Duration> {
    match parse(text)? {
        (true, d) if !d.is_zero() => None,
        (_, d) => Some(d),
    }
}

/// The sign (true for negative) and magnitude. Go keeps durations in an
/// `i64` of nanoseconds, so a magnitude past that range is rejected.
fn parse(text: &str) -> Option<(bool, Duration)> {
    let (negative, mut rest) = match text.as_bytes().first()? {
        b'-' => (true, &text[1..]),
        b'+' => (false, &text[1..]),
        _ => (false, text),
    };
    if rest == "0" {
        return Some((negative, Duration::ZERO));
    }
    if rest.is_empty() {
        return None;
    }
    let mut total: u128 = 0;
    while !rest.is_empty() {
        let (whole, after) = digits(rest);
        let (fraction, after) = match after.strip_prefix('.') {
            Some(tail) => digits(tail),
            None => ("", after),
        };
        if whole.is_empty() && fraction.is_empty() {
            return None;
        }
        let unit_len = after
            .find(|c: char| c == '.' || c.is_ascii_digit())
            .unwrap_or(after.len());
        let unit: u128 = match &after[..unit_len] {
            "ns" => 1,
            "us" | "\u{b5}s" | "\u{3bc}s" => 1_000,
            "ms" => 1_000_000,
            "s" => 1_000_000_000,
            "m" => 60_000_000_000,
            "h" => 3_600_000_000_000,
            _ => return None,
        };
        rest = &after[unit_len..];
        let mut value: u128 = 0;
        for digit in whole.bytes() {
            value = value
                .checked_mul(10)?
                .checked_add(u128::from(digit - b'0'))?;
            if value > 1 << 63 {
                return None;
            }
        }
        total = total.checked_add(value.checked_mul(unit)?)?;
        let mut scaled: u128 = 0;
        let mut scale: u128 = 1;
        for digit in fraction.bytes().take(18) {
            scaled = scaled * 10 + u128::from(digit - b'0');
            scale *= 10;
        }
        total = total.checked_add(scaled * unit / scale)?;
        if total > 1 << 63 {
            return None;
        }
    }
    if !negative && total == 1 << 63 {
        return None;
    }
    let nanos = u64::try_from(total).ok()?;
    Some((negative, Duration::from_nanos(nanos)))
}

fn digits(text: &str) -> (&str, &str) {
    let end = text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len());
    text.split_at(end)
}
