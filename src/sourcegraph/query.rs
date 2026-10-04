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
    /// `UPSTREAM_MAX_TIMEOUT`, from a `timeout:` at least that long or from
    /// `count:`.
    Ceiling,
}

/// The timeout the upstream gives a search for `query`: its `timeout:`, up
/// to `UPSTREAM_MAX_TIMEOUT`, which is `Source::Ceiling` when reached; else
/// `UPSTREAM_MAX_TIMEOUT` if it has a `count:`; else
/// `UPSTREAM_DEFAULT_TIMEOUT`.
pub fn effective_timeout(query: &str) -> Timeout {
    let ceiling =
        Timeout { duration: UPSTREAM_MAX_TIMEOUT, source: Source::Ceiling };
    match timeout_token(query) {
        Some((_, duration)) if duration >= UPSTREAM_MAX_TIMEOUT => ceiling,
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

/// The byte range of the value of `query`'s first `field`, such as
/// `count:`, ignoring ASCII case: a token outside quotes that starts with
/// it. The `(` that open a group before it and the `)` that close one
/// after it are not part of the token, as upstream.
fn field_value(query: &str, field: &str) -> Option<Range<usize>> {
    tokens(query).into_iter().find_map(|token| {
        let text = &query[token.clone()];
        let inner = text.trim_start_matches('(');
        let start = token.start + text.len() - inner.len();
        let end = token.start + text.trim_end_matches(')').len();
        let named = inner
            .get(..field.len())
            .is_some_and(|name| name.eq_ignore_ascii_case(field));
        let value = start + field.len();
        named.then(|| value..end.max(value))
    })
}

/// Whether `query` has a `field`, such as `count:`, as `field_value` finds
/// it.
fn has_field(query: &str, field: &str) -> bool {
    field_value(query, field).is_some()
}

/// `query`'s first `timeout:`, as `field_value` finds it: the byte range of
/// its value and the duration it gives. `None` if there is none or its
/// value isn't a Go duration.
fn timeout_token(query: &str) -> Option<(Range<usize>, Duration)> {
    let value = field_value(query, "timeout:")?;
    let duration = go_duration(&query[value.clone()])?;
    Some((value, duration))
}

/// The byte ranges of `query`'s tokens: runs of anything but whitespace,
/// where whitespace between quotes, as in `"a b"`, `'a b'` or
/// `content:"a b"`, doesn't end a token. As upstream, a quote opens only at
/// the start of a token, of a field's value or after the `(` that start a
/// token, which open groups; so `don't` and `foo("a b")` quote nothing.
fn tokens(query: &str) -> Vec<Range<usize>> {
    let mut tokens = Vec::new();
    let mut start = None;
    let mut quote = None;
    let mut escaped = false;
    let mut previous = None;
    let mut grouping = false;
    for (i, c) in query.char_indices() {
        if let Some(delimiter) = quote {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                _ if c == delimiter => quote = None,
                _ => {}
            }
        } else if c.is_whitespace() {
            if let Some(start) = start.take() {
                tokens.push(start..i);
            }
        } else {
            let opens = start.is_none() || grouping || previous == Some(':');
            grouping = (start.is_none() || grouping) && c == '(';
            start.get_or_insert(i);
            if opens && matches!(c, '"' | '\'') {
                quote = Some(c);
            }
        }
        previous = Some(c);
    }
    if let Some(start) = start {
        tokens.push(start..query.len());
    }
    tokens
}

/// Whether `token` is one of `query`'s tokens, as `tokens` splits them,
/// ignoring ASCII case.
pub fn has_token(query: &str, token: &str) -> bool {
    tokens(query)
        .into_iter()
        .any(|range| query[range].eq_ignore_ascii_case(token))
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
            ("timeout:1m x", ceiling.clone()),
            ("x timeout:60s", ceiling.clone()),
            ("x timeout:59.999s", timeout(ms(59_999), typed("59.999s"))),
            ("timeout:1m30s x", ceiling.clone()),
            ("x", default.clone()),
            ("\"timeout:5s\"", default.clone()),
            ("\"a timeout:5s\" x", default.clone()),
            ("content:\"a timeout:5s\"", default.clone()),
            ("'timeout:5s'", default.clone()),
            ("'a timeout:5s b'", default.clone()),
            ("x 'a timeout:5s b' y", default.clone()),
            ("content:'a timeout:5s b'", default.clone()),
            ("(\"a timeout:5s b\")", default.clone()),
            ("x ('a timeout:5s b' or y)", default.clone()),
            ("((\"a timeout:5s b\"))", default.clone()),
            ("foo(\"a timeout:5s b\")", timeout(ms(5000), typed("5s"))),
            ("x foo('a timeout:5s b')", timeout(ms(5000), typed("5s"))),
            ("(timeout:5s x)", timeout(ms(5000), typed("5s"))),
            ("((timeout:5s) x)", timeout(ms(5000), typed("5s"))),
            ("(y x timeout:5s)", timeout(ms(5000), typed("5s"))),
            ("x timeout:5s)", timeout(ms(5000), typed("5s"))),
            ("(x (y timeout:5s))", timeout(ms(5000), typed("5s"))),
            ("foo(timeout:5s", default.clone()),
            (r"'a \' timeout:5s' x", default.clone()),
            ("don't timeout:5s", timeout(ms(5000), typed("5s"))),
            ("a\"b timeout:5s", timeout(ms(5000), typed("5s"))),
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
            ("x timeout:1m", timeout(s(60), Source::Ceiling)),
            ("x timeout:60s", timeout(s(60), Source::Ceiling)),
            ("x \"count:5\"", timeout(s(20), Source::Default)),
            ("x xcount:5", timeout(s(20), Source::Default)),
            ("x COUNT:5", timeout(s(60), Source::Ceiling)),
            ("(count:5 x)", timeout(s(60), Source::Ceiling)),
            ("(x count:5)", timeout(s(60), Source::Ceiling)),
            ("x foo(count:5", timeout(s(20), Source::Default)),
        ] {
            assert_eq!(effective_timeout(query), expected, "{query}");
        }
    }

    #[test]
    fn tokens_keep_quoted_spaces() {
        let query = r#"repo:x before:"2 months ago" "a b" c 'd e' f:'g h'"#;
        assert!(has_token(query, r#"before:"2 months ago""#));
        assert!(has_token(query, r#""a b""#));
        assert!(has_token(query, "'d e'"));
        assert!(has_token(query, "f:'g h'"));
        assert!(has_token(r#"x ("a b")"#, r#"("a b")"#));
        assert!(has_token(query, "repo:x"));
        assert!(!has_token(query, "months"));
        assert!(!has_token(query, "repo"));
    }

    #[test]
    fn tokens_match_ignoring_ascii_case() {
        assert!(has_token("x lang:Go", "lang:go"));
        assert!(has_token("x LANG:GO", "lang:go"));
        assert!(!has_token("x lang:golang", "lang:go"));
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
        assert_eq!(
            longer("(timeout:5s x)").as_deref(),
            Some("(timeout:10s x)")
        );
        assert_eq!(
            longer("(y timeout:5s)").as_deref(),
            Some("(y timeout:10s)")
        );
        assert_eq!(longer("x timeout:5s)").as_deref(), Some("x timeout:10s)"));
        assert_eq!(longer("(x count:5)"), None);
        assert_eq!(
            longer("foo(\"a timeout:5s b\")").as_deref(),
            Some("foo(\"a timeout:10s b\")")
        );
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
