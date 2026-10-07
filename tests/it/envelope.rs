use sego::internal::error::{Error, Kind, report};
use sego::internal::upstream::{Envelope, Seg, classify};

use crate::support::fixture::reply;

const NOT_GRAPHQL: &str = "sourcegraph.com's answer isn't a GraphQL response";

fn rendered(error: &Error) -> String {
    let mut out = Vec::new();
    report(error, &mut out).unwrap_or_else(|e| panic!("{e}"));
    String::from_utf8(out).unwrap_or_else(|e| panic!("{e}"))
}

fn refused(name: &str) -> Error {
    match classify(reply(&format!("envelope/{name}"))) {
        Ok(envelope) => panic!("{name}: accepted {envelope:?}"),
        Err(error) => error,
    }
}

fn partial() -> Envelope {
    classify(reply("envelope/partial")).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn classification() {
    let cases: [(&str, Kind, String); 19] = [
        (
            "challenge",
            Kind::Edge,
            "sourcegraph.com's edge refused the request (HTTP 403, \
             Cloudflare challenge)"
                .into(),
        ),
        (
            "redirect",
            Kind::Upstream,
            "sourcegraph.com answered HTTP 301 with Location \
             https://sourcegraph.com/.api/graphql/"
                .into(),
        ),
        (
            "complexity",
            Kind::Upstream,
            "Sourcegraph rejected the request: query complexity limit \
             exceeded (cost 5001, limit 5000)"
                .into(),
        ),
        ("rate", Kind::Upstream, "sourcegraph.com answered HTTP 429".into()),
        (
            "bad-gateway",
            Kind::Upstream,
            "sourcegraph.com answered HTTP 502".into(),
        ),
        (
            "html-404",
            Kind::Upstream,
            "sourcegraph.com answered HTTP 404 text/html".into(),
        ),
        (
            "text-plain-envelope",
            Kind::Upstream,
            format!("{NOT_GRAPHQL}: not JSON"),
        ),
        (
            "no-content-type",
            Kind::Upstream,
            format!("{NOT_GRAPHQL}: not JSON"),
        ),
        (
            "redirect-no-location",
            Kind::Upstream,
            "sourcegraph.com answered HTTP 302 with no Location".into(),
        ),
        (
            "html-403",
            Kind::Edge,
            "sourcegraph.com's edge refused the request (HTTP 403, \
             Cloudflare challenge)"
                .into(),
        ),
        (
            "challenge-text",
            Kind::Edge,
            "sourcegraph.com's edge refused the request (HTTP 403, \
             Cloudflare challenge)"
                .into(),
        ),
        ("null-no-errors", Kind::Upstream, format!("{NOT_GRAPHQL}: empty")),
        ("neither", Kind::Upstream, format!("{NOT_GRAPHQL}: empty")),
        (
            "bad-request",
            Kind::Upstream,
            "Sourcegraph rejected sego's request: Syntax Error".into(),
        ),
        (
            "no-message",
            Kind::Upstream,
            "Sourcegraph rejected sego's request".into(),
        ),
        ("not-json", Kind::Upstream, format!("{NOT_GRAPHQL}: not JSON")),
        ("truncated", Kind::Upstream, format!("{NOT_GRAPHQL}: truncated")),
        ("empty", Kind::Upstream, format!("{NOT_GRAPHQL}: empty")),
        (
            "pathless",
            Kind::Upstream,
            "Sourcegraph rejected sego's request: Unknown field \"nope\" on \
             type \"Query\""
                .into(),
        ),
    ];
    for (name, kind, message) in cases {
        let error = refused(name);
        assert_eq!(error.kind(), kind, "{name}");
        assert_eq!(error.to_string(), message, "{name}");
    }
    assert!(classify(reply("envelope/partial")).is_ok());
}

#[test]
fn rate_limit_carries_retry_after() {
    assert_eq!(
        rendered(&refused("rate")),
        "sego: error: sourcegraph.com answered HTTP 429\n  Retry-After: 30\n"
    );
    assert!(!rendered(&refused("bad-gateway")).contains("Retry-After"));
}

#[test]
fn error_coverage() {
    let envelope = partial();
    assert!(envelope.get(&["r0", "c0", "meta"]).is_some());
    assert!(envelope.get(&["r0", "c0", "meta", "lfs"]).is_none());
    assert!(envelope.get(&["r0", "c0", "meta", "size"]).is_some());
    assert!(
        envelope.error_at(&["r0", "c0", "meta", "lfs", "byteSize"]).is_some()
    );
    assert!(envelope.error_at(&["r0", "c0", "meta"]).is_none());
    assert!(envelope.error_at(&["r0", "c0", "meta", "lfs"]).is_some());
    let messages: Vec<&str> =
        envelope.errors.iter().map(|e| e.message.as_str()).collect();
    assert_eq!(messages, ["git blob failed", "other"]);
    let path: Vec<Seg> = ["r0", "c0", "meta", "lfs"]
        .into_iter()
        .map(|k| Seg::Key(k.to_owned()))
        .collect();
    assert_eq!(envelope.errors[0].path, path);
}

#[test]
fn odd_paths_stay_located() {
    let envelope =
        classify(reply("envelope/odd-path")).unwrap_or_else(|e| panic!("{e}"));
    let key = |k: &str| Seg::Key(k.to_owned());
    assert_eq!(envelope.errors.len(), 2);
    assert_eq!(envelope.errors[0].path, [Seg::Invalid]);
    assert_eq!(envelope.errors[1].path, [key("r0"), Seg::Invalid, key("x")]);
    assert!(envelope.error_at(&["r0", "x"]).is_none());
    assert!(envelope.get(&["r0", "name"]).is_some());
}
