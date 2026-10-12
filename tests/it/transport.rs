use std::ffi::OsStr;
use std::net::TcpListener;
use std::time::{Duration, Instant};

use sego::internal::error::Error;
use serde_json::Map;

use sego::internal::upstream::{
    BODY_LIMIT, Endpoint, GRAPHQL_PATH, RawError, Request, Transport,
    TransportError, Ureq, exchange, read,
};

use crate::support::http::{Response, Server};

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
    Endpoint::from_env(value.map(OsStr::new)).map(|e| e.url(GRAPHQL_PATH))
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
        "https://host:abc",
        "https://host:",
        "http://[::1]:x",
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
    let Err(error) =
        Endpoint::from_env(Some(OsStr::from_bytes(b"http://\xff")))
    else {
        panic!("non-UTF-8 was accepted");
    };
    let mut out = Vec::new();
    sego::internal::error::report(&error, &mut out)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        String::from_utf8_lossy(&out),
        "sego: error: SEGO_ENDPOINT is not valid UTF-8\n\
         usage: sego COMMAND [--json] ...\n"
    );
}

#[test]
fn sends_identification() {
    let server = Server::start(|_| Response::new(200).body("{}"));
    client(&server).post(GRAPHQL_PATH, b"{}", soon()).expect("post");
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
    assert_eq!(post.path, GRAPHQL_PATH);
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
    let slow = client(&server).post(GRAPHQL_PATH, b"{}", soon()).expect("429");
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
        client(&server).post(GRAPHQL_PATH, b"{}", soon()).expect("post"),
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
    let slow = client(&server).post(
        GRAPHQL_PATH,
        b"{}",
        started + Duration::from_secs(1),
    );
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
    let listener =
        TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| panic!("{e}"));
    let port = listener.local_addr().unwrap_or_else(|e| panic!("{e}")).port();
    drop(listener);
    let value = format!("http://127.0.0.1:{port}");
    let closed = Ureq::without_proxy(
        Endpoint::from_env(Some(OsStr::new(&value)))
            .unwrap_or_else(|e| panic!("{e}")),
    );
    let refused = closed.get("/", soon());
    assert!(
        matches!(refused, Err(TransportError::Network("connection refused"))),
        "{refused:?}"
    );

    let server = Server::start(|_| Response::drop_connection());
    let dropped = client(&server).post(GRAPHQL_PATH, b"{}", soon());
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
fn endpoint_host_is_host_and_port_without_credentials() {
    let host = |s: &str| {
        Endpoint::from_env(Some(OsStr::new(s)))
            .unwrap_or_else(|e| panic!("{e:?}"))
            .host()
    };
    assert_eq!(
        Endpoint::from_env(None).unwrap_or_else(|e| panic!("{e:?}")).host(),
        "sourcegraph.com"
    );
    assert_eq!(host("http://127.0.0.1:1"), "127.0.0.1:1");
    assert_eq!(host("https://u:p@example.com:8443/base/"), "example.com:8443");
    assert_eq!(host("http://[::1]:8080"), "[::1]:8080");
}

fn endpoint_host_of(url: &str) -> String {
    url.strip_prefix("http://").unwrap_or(url).to_owned()
}

#[test]
fn errors_name_the_endpoint_a_client_talks_to() {
    let server = Server::start(|_| Response::new(500).body("down"));
    let host = endpoint_host_of(&server.url());
    let client = client(&server);
    assert_eq!(client.host(), host);

    let request = Request {
        operation: "Op".to_owned(),
        document: "query Op { x }".to_owned(),
        variables: Map::new(),
    };
    let Err(error) = exchange(&client, &request, soon()) else {
        panic!("a 500 was accepted");
    };
    assert_eq!(error.to_string(), format!("{host} answered HTTP 500"));
    let Err(RawError::Failed(error)) = read(&client, "/raw", soon()) else {
        panic!("a 500 was read");
    };
    assert_eq!(error.to_string(), format!("{host} answered HTTP 500"));
}

#[test]
fn a_refused_connection_names_the_endpoint() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .unwrap_or_else(|e| panic!("bind: {e}"));
    let address = listener.local_addr().unwrap_or_else(|e| panic!("{e}"));
    drop(listener);
    let url = format!("http://{address}");
    let endpoint = Endpoint::from_env(Some(OsStr::new(&url)))
        .unwrap_or_else(|e| panic!("{e}"));
    let client = Ureq::without_proxy(endpoint);
    let request = Request {
        operation: "Op".to_owned(),
        document: "query Op { x }".to_owned(),
        variables: Map::new(),
    };
    let Err(error) = exchange(&client, &request, soon()) else {
        panic!("a closed port answered");
    };
    assert_eq!(
        error.to_string(),
        format!("could not reach {address}: connection refused")
    );
}

#[test]
fn transport_errors_name_the_host() {
    assert_eq!(
        TransportError::Deadline.into_error("127.0.0.1:1").to_string(),
        "no answer from 127.0.0.1:1 within 90 s"
    );
    assert_eq!(
        TransportError::Network("connection refused")
            .into_error("127.0.0.1:1")
            .to_string(),
        "could not reach 127.0.0.1:1: connection refused"
    );
    assert_eq!(
        TransportError::TooLarge.into_error("h:9").to_string(),
        "h:9's answer is over 64 MiB"
    );
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

/// A gzip stream holding `data` in one stored deflate block.
fn gzip(data: &[u8]) -> Vec<u8> {
    let len = u16::try_from(data.len()).unwrap_or_else(|e| panic!("{e}"));
    let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff, 1];
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&(!len).to_le_bytes());
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(data).to_le_bytes());
    out.extend_from_slice(&u32::from(len).to_le_bytes());
    out
}

#[test]
fn a_gzip_encoded_body_is_decoded() {
    let json = br#"{"data":{"r0":null}}"#;
    let compressed = gzip(json);
    let server = Server::start(move |_| {
        Response::new(200)
            .header("Content-Type", "application/json")
            .header("Content-Encoding", "gzip")
            .body(compressed.clone())
    });
    let reply = client(&server)
        .post(GRAPHQL_PATH, b"{}", soon())
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, json);
}
