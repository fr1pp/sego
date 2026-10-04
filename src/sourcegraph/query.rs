//! Search query syntax: tokens, and the timeout a search runs for.

use std::ops::Range;
use std::time::Duration;

/// How long the upstream lets a search run when its query sets neither
/// `timeout:` nor `count:`.
const UPSTREAM_DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);

/// The longest the upstream lets a search run, and how long it runs when
/// its query sets `count:` but not `timeout:`. A longer `timeout:` is cut
/// to it silently. It is the upstream's default
/// `search.limits.maxTimeoutSeconds`, and sourcegraph.com's.
pub const UPSTREAM_MAX_TIMEOUT: Duration = Duration::from_secs(60);

/// The timeout a search runs for, and where it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timeout {
    pub duration: Duration,
    pub source: Source,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The query's `timeout:`, holding its value as typed.
    Query(String),
    /// `UPSTREAM_DEFAULT_TIMEOUT`.
    Default,
    /// `UPSTREAM_MAX_TIMEOUT`, from a longer `timeout:` or from `count:`.
    Ceiling,
}

/// The timeout the upstream gives a search for `query`: its `timeout:`, up
/// to `UPSTREAM_MAX_TIMEOUT`; else `UPSTREAM_MAX_TIMEOUT` if it has a
/// `count:`; else `UPSTREAM_DEFAULT_TIMEOUT`.
pub fn effective_timeout(query: &str) -> Timeout {
    let ceiling =
        Timeout { duration: UPSTREAM_MAX_TIMEOUT, source: Source::Ceiling };
    match timeout_token(query) {
        Some((_, duration)) if duration > UPSTREAM_MAX_TIMEOUT => ceiling,
        Some((value, duration)) => Timeout {
            duration,
            source: Source::Query(query[value].to_owned()),
        },
        None if has_field(query, "count:") => ceiling,
        None => Timeout {
            duration: UPSTREAM_DEFAULT_TIMEOUT,
            source: Source::Default,
        },
    }
}

/// `query`'s first token outside quotes that starts with `field`, such as
/// `count:`, ignoring ASCII case.
fn field_token(query: &str, field: &str) -> Option<Range<usize>> {
    tokens(query).into_iter().find(|token| {
        query[token.clone()]
            .get(..field.len())
            .is_some_and(|name| name.eq_ignore_ascii_case(field))
    })
}

/// Whether `query` has a token outside quotes that starts with `field`,
/// such as `count:`, ignoring ASCII case.
fn has_field(query: &str, field: &str) -> bool {
    field_token(query, field).is_some()
}

/// `query`'s first `timeout:` token outside quotes: the byte range of its
/// value and the duration it gives. `None` if there is none or its value
/// isn't a Go duration.
fn timeout_token(query: &str) -> Option<(Range<usize>, Duration)> {
    const FIELD: &str = "timeout:";
    let token = field_token(query, FIELD)?;
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

/// The longest `timeout:` a hint proposes: the upstream runs no search
/// longer.
const LONGEST_TIMEOUT: Duration = UPSTREAM_MAX_TIMEOUT;

/// `query` with `timeout:` set to twice its effective timeout, up to
/// `LONGEST_TIMEOUT`, replacing its value or appended; `None` if the
/// effective timeout is already that long. A zero timeout becomes
/// `UPSTREAM_DEFAULT_TIMEOUT`.
pub fn with_longer_timeout(query: &str) -> Option<String> {
    let current = effective_timeout(query).duration;
    if current >= LONGEST_TIMEOUT {
        return None;
    }
    let longer = if current.is_zero() {
        UPSTREAM_DEFAULT_TIMEOUT
    } else {
        current.saturating_mul(2).min(LONGEST_TIMEOUT)
    };
    let longer = go_text(longer);
    let mut retry = query.to_owned();
    if let Some((value, _)) = timeout_token(query) {
        retry.replace_range(value, &longer);
    } else {
        retry.push_str(" timeout:");
        retry.push_str(&longer);
    }
    Some(retry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sourcegraph::GLOBAL_TIMEOUT;

    fn timeout(duration: Duration, source: Source) -> Timeout {
        Timeout { duration, source }
    }

    fn typed(value: &str) -> Source {
        Source::Query(value.to_owned())
    }

    #[test]
    fn query_timeouts() {
        let ms = Duration::from_millis;
        let default = timeout(ms(20_000), Source::Default);
        let ceiling = timeout(ms(60_000), Source::Ceiling);
        let overflow = "x timeout:340282366920938463463374607431.9s";
        for (query, expected) in [
            ("x timeout:1ms", timeout(ms(1), typed("1ms"))),
            ("x TIMEOUT:5s", timeout(ms(5000), typed("5s"))),
            ("timeout:1m x", timeout(ms(60_000), typed("1m"))),
            ("timeout:1m30s x", ceiling.clone()),
            ("x", default.clone()),
            ("\"timeout:5s\"", default.clone()),
            ("\"a timeout:5s\" x", default.clone()),
            ("content:\"a timeout:5s\"", default.clone()),
            ("x timeout:2h", ceiling.clone()),
            ("x timeout:1.5s", timeout(ms(1500), typed("1.5s"))),
            ("x timeout:soon", default.clone()),
            ("x timeout:5", default.clone()),
            ("x timeout:0s", timeout(ms(0), typed("0s"))),
            ("x mytimeout:5s", default.clone()),
            (overflow, default.clone()),
        ] {
            assert_eq!(effective_timeout(query), expected, "{query}");
        }
    }

    #[test]
    fn effective_timeouts() {
        let s = Duration::from_secs;
        for (query, expected) in [
            ("x", timeout(s(20), Source::Default)),
            ("x count:5", timeout(s(60), Source::Ceiling)),
            ("x count:all", timeout(s(60), Source::Ceiling)),
            ("x timeout:30s count:all", timeout(s(30), typed("30s"))),
            ("x timeout:90s", timeout(s(60), Source::Ceiling)),
            ("x \"count:5\"", timeout(s(20), Source::Default)),
            ("x xcount:5", timeout(s(20), Source::Default)),
            ("x COUNT:5", timeout(s(60), Source::Ceiling)),
        ] {
            assert_eq!(effective_timeout(query), expected, "{query}");
        }
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
    fn longer_timeouts() {
        let longer = |query| with_longer_timeout(query);
        assert_eq!(longer("x").as_deref(), Some("x timeout:40s"));
        assert_eq!(longer("x timeout:20s").as_deref(), Some("x timeout:40s"));
        assert_eq!(longer("timeout:20s x").as_deref(), Some("timeout:40s x"));
        assert_eq!(longer("x timeout:40s").as_deref(), Some("x timeout:1m"));
        assert_eq!(longer("x timeout:45s").as_deref(), Some("x timeout:1m"));
        assert_eq!(longer("x timeout:1m"), None);
        assert_eq!(longer("x timeout:2m"), None);
        assert_eq!(longer("x count:5"), None);
        assert_eq!(
            longer("x count:5 timeout:10s").as_deref(),
            Some("x count:5 timeout:20s")
        );
        assert_eq!(longer("x timeout:1ms").as_deref(), Some("x timeout:2ms"));
        assert_eq!(longer("x timeout:0.5s").as_deref(), Some("x timeout:1s"));
        assert_eq!(longer("x timeout:0s").as_deref(), Some("x timeout:20s"));
    }

    #[test]
    fn longer_timeouts_end_before_the_request_does() {
        for query in ["x", "x count:5"]
            .map(str::to_owned)
            .into_iter()
            .chain((0..200).map(|seconds| format!("x timeout:{seconds}s")))
        {
            let Some(longer) = with_longer_timeout(&query) else { continue };
            let timeout = effective_timeout(&longer).duration;
            assert!(timeout < GLOBAL_TIMEOUT, "{query} -> {longer}");
        }
    }
}
