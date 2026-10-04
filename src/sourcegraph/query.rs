//! Search query syntax: tokens, and the `timeout:` field.

use std::ops::Range;
use std::time::Duration;

use super::GLOBAL_TIMEOUT;

/// `query`'s first `timeout:` token outside double quotes: the byte range
/// of its value and the duration it gives. `None` if there is none or its
/// value isn't a Go duration.
pub fn timeout_token(query: &str) -> Option<(Range<usize>, Duration)> {
    const FIELD: &str = "timeout:";
    let token = tokens(query).into_iter().find(|token| {
        query[token.clone()]
            .get(..FIELD.len())
            .is_some_and(|field| field.eq_ignore_ascii_case(FIELD))
    })?;
    let value = token.start + FIELD.len()..token.end;
    let duration = go_duration(&query[value.clone()])?;
    Some((value, duration))
}

/// The byte ranges of `query`'s tokens: runs of anything but whitespace,
/// where whitespace between double quotes, as in `"a b"` or
/// `content:"a b"`, doesn't end a token.
fn tokens(query: &str) -> Vec<Range<usize>> {
    let mut tokens = Vec::new();
    let mut start = None;
    let (mut quoted, mut escaped) = (false, false);
    for (i, c) in query.char_indices() {
        if quoted {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => quoted = false,
                _ => {}
            }
        } else if c.is_whitespace() {
            if let Some(start) = start.take() {
                tokens.push(start..i);
            }
        } else {
            start.get_or_insert(i);
            quoted = c == '"';
        }
    }
    if let Some(start) = start {
        tokens.push(start..query.len());
    }
    tokens
}

/// Whether `token` is one of `query`'s tokens, as `tokens` splits them.
pub fn has_token(query: &str, token: &str) -> bool {
    tokens(query).into_iter().any(|range| query[range] == *token)
}

/// The nanoseconds in each of Go's duration units.
const GO_UNITS: &[(&str, u128)] = &[
    ("h", 3_600_000_000_000),
    ("m", 60_000_000_000),
    ("s", 1_000_000_000),
    ("ms", 1_000_000),
    ("us", 1_000),
    ("\u{b5}s", 1_000),
    ("\u{3bc}s", 1_000),
    ("ns", 1),
];

/// `text` as Go's `time.ParseDuration` reads it, such as `1m30s` or
/// `1.5s`, but with no sign; `None` if it isn't one.
fn go_duration(text: &str) -> Option<Duration> {
    let number = |c: char| c.is_ascii_digit() || c == '.';
    let mut rest = text;
    let mut nanos: u128 = 0;
    if rest.is_empty() {
        return None;
    }
    while !rest.is_empty() {
        let (amount, tail) =
            rest.split_at(rest.find(|c| !number(c)).unwrap_or(rest.len()));
        let (unit, tail) =
            tail.split_at(tail.find(number).unwrap_or(tail.len()));
        let &(_, scale) = GO_UNITS.iter().find(|(name, _)| *name == unit)?;
        let (whole, fraction) = amount.split_once('.').unwrap_or((amount, ""));
        if whole.is_empty() && fraction.is_empty() {
            return None;
        }
        let whole: u128 =
            if whole.is_empty() { 0 } else { whole.parse().ok()? };
        let mut part = whole.checked_mul(scale)?;
        let mut divisor: u128 = 1;
        // Digits past the 18th are below a nanosecond in every unit.
        for digit in fraction.chars() {
            let digit = digit.to_digit(10)?;
            if divisor < 10u128.pow(18) {
                divisor = divisor.checked_mul(10)?;
                let nanos = u128::from(digit).checked_mul(scale)?;
                part = part.checked_add(nanos.checked_div(divisor)?)?;
            }
        }
        nanos = nanos.checked_add(part)?;
        rest = tail;
    }
    u64::try_from(nanos).ok().map(Duration::from_nanos)
}

/// `duration` in Go's syntax, in its largest whole unit.
fn go_text(duration: Duration) -> String {
    let nanos = duration.as_nanos();
    GO_UNITS
        .iter()
        .find(|&&(_, scale)| nanos.is_multiple_of(scale))
        .map_or_else(
            || format!("{nanos}ns"),
            |&(unit, scale)| format!("{}{unit}", nanos / scale),
        )
}

/// The longest `timeout:` a hint proposes, short enough that a search
/// running for all of it can still answer within `GLOBAL_TIMEOUT`.
const LONGEST_TIMEOUT: Duration =
    GLOBAL_TIMEOUT.saturating_sub(Duration::from_secs(5));

/// `query` with its `timeout:` doubled, up to `LONGEST_TIMEOUT`; `None` if
/// it has none or that is no longer.
pub fn with_longer_timeout(query: &str) -> Option<String> {
    let (value, duration) = timeout_token(query)?;
    let longer = duration.saturating_mul(2).min(LONGEST_TIMEOUT);
    if longer <= duration {
        return None;
    }
    let mut retry = query.to_owned();
    retry.replace_range(value, &go_text(longer));
    Some(retry)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The timeout `cut_short` waits for: `query`'s, unless it is zero.
    fn query_timeout(query: &str) -> Option<Duration> {
        timeout_token(query)
            .map(|(_, duration)| duration)
            .filter(|duration| !duration.is_zero())
    }

    #[test]
    fn query_timeouts() {
        let ms = Duration::from_millis;
        assert_eq!(query_timeout("x timeout:1ms"), Some(ms(1)));
        assert_eq!(query_timeout("timeout:1m30s x"), Some(ms(90_000)));
        assert_eq!(query_timeout("x"), None);
        assert_eq!(query_timeout("\"timeout:5s\""), None);
        assert_eq!(query_timeout("\"a timeout:5s\" x"), None);
        assert_eq!(query_timeout("content:\"a timeout:5s\""), None);
        assert_eq!(query_timeout("x timeout:2h"), Some(ms(7_200_000)));
        assert_eq!(query_timeout("x timeout:1.5s"), Some(ms(1500)));
        assert_eq!(query_timeout("x timeout:soon"), None);
        assert_eq!(query_timeout("x timeout:5"), None);
        assert_eq!(query_timeout("x timeout:0s"), None);
        assert_eq!(query_timeout("x mytimeout:5s"), None);
        let overflow = "x timeout:340282366920938463463374607431.9s";
        assert_eq!(query_timeout(overflow), None);
    }

    #[test]
    fn tokens_keep_quoted_spaces() {
        let query = r#"repo:x before:"2 months ago" "a b" c"#;
        assert!(has_token(query, r#"before:"2 months ago""#));
        assert!(has_token(query, r#""a b""#));
        assert!(has_token(query, "repo:x"));
        assert!(!has_token(query, "months"));
        assert!(!has_token(query, "repo"));
    }

    #[test]
    fn longer_timeouts_double_up_to_the_cap() {
        let longer = |query| with_longer_timeout(query);
        assert_eq!(longer("x timeout:1ms").as_deref(), Some("x timeout:2ms"));
        assert_eq!(longer("timeout:20s x").as_deref(), Some("timeout:40s x"));
        assert_eq!(longer("x timeout:45s").as_deref(), Some("x timeout:90s"));
        assert_eq!(longer("x timeout:0.5s").as_deref(), Some("x timeout:1s"));
        assert_eq!(longer("x timeout:1m").as_deref(), Some("x timeout:90s"));
        assert_eq!(longer("x timeout:89s").as_deref(), Some("x timeout:90s"));
        assert_eq!(longer("x timeout:1m30s"), None);
        assert_eq!(longer("x timeout:2m"), None);
        assert_eq!(longer("x timeout:0s"), None);
        assert_eq!(longer("x"), None);
    }

    #[test]
    fn longer_timeouts_end_before_the_request_does() {
        for seconds in 1..200 {
            let query = format!("x timeout:{seconds}s");
            let Some(longer) = with_longer_timeout(&query) else { continue };
            let timeout = query_timeout(&longer).unwrap();
            assert!(timeout < GLOBAL_TIMEOUT, "{query} -> {longer}");
        }
    }
}
