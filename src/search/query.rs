use std::time::Duration;

use super::lint::{Token, check, tokenize};

const DEADLINE_MS: u64 = 60_000;
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

pub struct Prepared {
    pub sent: String,
    pub added: Vec<String>,
    pub pattern_type: String,
    pub deadline_ms: u64,
    pub tokens: Vec<Token>,
    pub notes: Vec<String>,
}

pub fn prepare(query: &str) -> Prepared {
    let has_count = tokenize(query).iter().any(|token| field(token, "count"));
    let (sent, added) = if has_count {
        (query.to_owned(), Vec::new())
    } else {
        (format!("{query} count:100"), vec!["count:100".to_owned()])
    };
    let tokens = tokenize(&sent);
    Prepared {
        pattern_type: pattern_type(&tokens),
        notes: check(&tokens),
        deadline_ms: deadline_ms(&tokens),
        sent,
        added,
        tokens,
    }
}

fn field(token: &Token, name: &str) -> bool {
    matches!(token, Token::Filter(f) if f.field.eq_ignore_ascii_case(name))
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

fn deadline_ms(tokens: &[Token]) -> u64 {
    let Some(Token::Filter(timeout)) =
        tokens.iter().find(|token| field(token, "timeout"))
    else {
        return DEADLINE_MS;
    };
    match parse(unquoted(&timeout.value)) {
        Some((true, _)) => 0,
        Some((false, d)) => u64::try_from(d.as_nanos().div_ceil(1_000_000))
            .map_or(DEADLINE_MS, |ms| ms.min(DEADLINE_MS)),
        None => DEADLINE_MS,
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

pub fn seconds(ms: u64) -> String {
    let (whole, part) = (ms / 1000, ms % 1000);
    if part == 0 {
        return whole.to_string();
    }
    let part = format!("{part:03}");
    format!("{whole}.{}", part.trim_end_matches('0'))
}
