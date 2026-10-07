use std::ffi::OsStr;
use std::time::{Duration, Instant};

use sego::internal::error::Error;
use sego::internal::upstream::{
    BODY_LIMIT, Endpoint, Transport, TransportError, Ureq,
};

use crate::support::http::{Response, Server};

const GRAPHQL: &str = "/.api/graphql";

fn client(server: &Server) -> Ureq {
    let value = server.url();
    let endpoint = Endpoint::from_env(Some(OsStr::new(&value)))
        .unwrap_or_else(|e| panic!("{e}"));
    Ureq::without_proxy(endpoint)
}

fn soon() -> Instant {
    Instant::now() + Duration::from_secs(30)
}

fn graphql_url(value: Option<&str>) -> Result<String, Error> {
    Endpoint::from_env(value.map(OsStr::new)).map(|e| e.url(GRAPHQL))
}

fn usage_message(value: &str) -> String {
    let Err(error) = graphql_url(Some(value)) else {
        panic!("{value} was accepted");
    };
    let mut out = Vec::new();
    sego::internal::error::report(&error, &mut out)
        .unwrap_or_else(|e| panic!("{e}"));
    String::from_utf8_lossy(&out).into_owned()
}

#[test]
fn endpoint() {
    assert_eq!(
        graphql_url(None).expect("unset"),
        "https://sourcegraph.com/.api/graphql"
    );
    assert_eq!(
        graphql_url(Some("")).expect("empty"),
        "https://sourcegraph.com/.api/graphql"
    );
    assert_eq!(
        graphql_url(Some("https://host/sg/")).expect("prefix"),
        "https://host/sg/.api/graphql"
    );
    assert_eq!(
        graphql_url(Some("http://127.0.0.1:9")).expect("local"),
        "http://127.0.0.1:9/.api/graphql"
    );
    for bad in [
        "ftp://x",
        "sourcegraph.com",
        "https://host?x=1",
        "https://host#x",
        "http://",
    ] {
        assert_eq!(
            usage_message(bad),
            format!(
                "sego: error: SEGO_ENDPOINT must be an http:// or https:// \
                 URL with a host and no query or fragment: {bad}\n\
                 usage: sego COMMAND [--json] ...\n"
            ),
            "{bad}"
        );
    }
}

#[cfg(unix)]
#[test]
fn endpoint_must_be_utf8() {
    use std::os::unix::ffi::OsStrExt;
    let error = Endpoint::from_env(Some(OsStr::from_bytes(b"http://\xff")))
        .expect_err("non-utf8");
    assert_eq!(error.to_string(), "SEGO_ENDPOINT is not valid UTF-8");
}

#[test]
fn sends_identification() {
    let server = Server::start(|_| Response::new(200).body("{}"));
    client(&server).post(GRAPHQL, b"{}", soon()).expect("post");
    client(&server).get("/r/x", soon()).expect("get");
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_eq!(request.header("user-agent"), Some("sego/0.2.0"));
        assert_eq!(request.header("x-requested-with"), Some("sego/0.2.0"));
        assert_eq!(
            request.header("x-sourcegraph-api-client-name"),
            Some("sego")
        );
        assert_eq!(
            request.header("x-sourcegraph-api-client-version"),
            Some("0.2.0")
        );
        assert!(
            request
                .header("accept-encoding")
                .is_some_and(|v| { v.contains("gzip") })
        );
        for (name, value) in &request.headers {
            assert!(!name.contains('@') && !value.contains('@'), "{name}");
        }
    }
    let post = &requests[0];
    assert_eq!(post.method, "POST");
    assert_eq!(post.path, GRAPHQL);
    assert_eq!(post.header("content-type"), Some("application/json"));
    assert_eq!(post.header("accept"), Some("application/json"));
    assert_eq!(post.body, b"{}");
    assert_eq!(requests[1].method, "GET");
    assert_eq!(requests[1].path, "/r/x");
}

#[test]
fn status_bodies_are_returned() {
    let server = Server::start(|request| {
        if request.path == "/400" {
            Response::new(400).body("bad")
        } else {
            Response::new(429).header("Retry-After", "30").body("slow")
        }
    });
    let bad = client(&server).get("/400", soon()).expect("400");
    assert_eq!((bad.status, bad.body.as_slice()), (400, &b"bad"[..]));
    let slow = client(&server).post(GRAPHQL, b"{}", soon()).expect("429");
    assert_eq!(slow.status, 429);
    assert_eq!(slow.retry_after.as_deref(), Some("30"));
    assert_eq!(slow.body, b"slow");
}

#[test]
fn redirects_are_not_followed() {
    let server = Server::start(|_| {
        Response::new(302).header("Location", "/next").body("moved")
    });
    for reply in [
        client(&server).get("/r/a", soon()).expect("get"),
        client(&server).post(GRAPHQL, b"{}", soon()).expect("post"),
    ] {
        assert_eq!(reply.status, 302);
        assert_eq!(reply.location.as_deref(), Some("/next"));
    }
    assert_eq!(server.requests().len(), 2);
}

#[test]
fn body_limit() {
    let server = Server::start(|request| {
        let len =
            if request.path == "/over" { BODY_LIMIT + 1 } else { BODY_LIMIT };
        Response::new(200).filler(b'x', len)
    });
    let over = client(&server).get("/over", soon());
    assert!(matches!(over, Err(TransportError::TooLarge)), "{over:?}");
    let exact = client(&server).get("/exact", soon()).expect("exact");
    assert_eq!(exact.body.len() as u64, BODY_LIMIT);
}

#[test]
fn deadline_is_shared() {
    let server = Server::start(|_| {
        Response::new(200).delay(Duration::from_secs(3)).body("late")
    });
    let started = Instant::now();
    let slow =
        client(&server).post(GRAPHQL, b"{}", started + Duration::from_secs(1));
    assert!(matches!(slow, Err(TransportError::Deadline)), "{slow:?}");
    assert!(started.elapsed() < Duration::from_secs(2));

    let before = server.requests().len();
    let past = Instant::now();
    let gone = client(&server).get("/r/x", past);
    assert!(matches!(gone, Err(TransportError::Deadline)), "{gone:?}");
    assert_eq!(server.requests().len(), before);
}

#[test]
fn network_causes() {
    let closed = {
        let server = Server::start(|_| Response::new(200));
        client(&server)
    };
    let refused = closed.get("/", soon());
    assert!(
        matches!(refused, Err(TransportError::Network("connection refused"))),
        "{refused:?}"
    );

    let server = Server::start(|_| Response::drop_connection());
    let dropped = client(&server).post(GRAPHQL, b"{}", soon());
    assert!(
        matches!(
            dropped,
            Err(TransportError::Network(
                "connection closed" | "connection reset"
            ))
        ),
        "{dropped:?}"
    );
}

#[test]
fn errors_convert() {
    let network: Error = TransportError::Network("connection reset").into();
    assert_eq!(
        network.to_string(),
        "could not reach sourcegraph.com: connection reset"
    );
    let deadline: Error = TransportError::Deadline.into();
    assert_eq!(
        deadline.to_string(),
        "no answer from sourcegraph.com within 90 s"
    );
    let large: Error = TransportError::TooLarge.into();
    assert_eq!(large.to_string(), "sourcegraph.com's answer is over 64 MiB");
}
