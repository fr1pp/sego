use std::io::{self, Read};
use std::time::{Duration, Instant};

use super::{Endpoint, Response, Transport, TransportError};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const BODY_LIMIT: u64 = 64 * 1024 * 1024;

pub struct Ureq {
    agent: ureq::Agent,
    endpoint: Endpoint,
}

impl Ureq {
    pub fn new(endpoint: Endpoint) -> Ureq {
        Ureq::build(endpoint, true)
    }

    /// Ignores `HTTP_PROXY` and friends, so tests that talk to 127.0.0.1
    /// don't depend on the environment of the process running them.
    #[cfg(test)]
    fn without_proxy(endpoint: Endpoint) -> Ureq {
        Ureq::build(endpoint, false)
    }

    fn build(endpoint: Endpoint, env_proxy: bool) -> Ureq {
        let builder = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .max_redirects_will_error(false)
            .user_agent(format!("sego/{VERSION}"));
        let builder = if env_proxy { builder } else { builder.proxy(None) };
        Ureq { agent: builder.build().into(), endpoint }
    }
}

fn identify<B>(request: ureq::RequestBuilder<B>) -> ureq::RequestBuilder<B> {
    request
        .header("X-Requested-With", format!("sego/{VERSION}"))
        .header("X-Sourcegraph-API-Client-Name", "sego")
        .header("X-Sourcegraph-API-Client-Version", VERSION)
}

fn remaining(deadline: Instant) -> Result<Duration, TransportError> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() { Err(TransportError::Deadline) } else { Ok(left) }
}

impl Transport for Ureq {
    fn post(&self, path: &str, body: &[u8], deadline: Instant) -> Result<Response, TransportError> {
        let timeout = remaining(deadline)?;
        let request = identify(self.agent.post(self.endpoint.url(path)))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .config()
            .timeout_global(Some(timeout))
            .build();
        read(request.send(body))
    }

    fn get(&self, path: &str, deadline: Instant) -> Result<Response, TransportError> {
        let timeout = remaining(deadline)?;
        let request = identify(self.agent.get(self.endpoint.url(path)))
            .config()
            .timeout_global(Some(timeout))
            .build();
        read(request.call())
    }
}

fn header(response: &ureq::http::Response<ureq::Body>, name: &str) -> Option<String> {
    response.headers().get(name).map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
}

fn read(
    result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<Response, TransportError> {
    let mut sent = result.map_err(classify)?;
    let mut response = Response {
        status: sent.status().as_u16(),
        content_type: header(&sent, "content-type"),
        location: header(&sent, "location"),
        retry_after: header(&sent, "retry-after"),
        cf_mitigated: header(&sent, "cf-mitigated"),
        body: Vec::new(),
    };
    sent.body_mut()
        .as_reader()
        .take(BODY_LIMIT + 1)
        .read_to_end(&mut response.body)
        .map_err(classify_io)?;
    if response.body.len() as u64 > BODY_LIMIT {
        return Err(TransportError::TooLarge);
    }
    Ok(response)
}

fn classify(error: ureq::Error) -> TransportError {
    match error {
        ureq::Error::Timeout(_) => TransportError::Deadline,
        ureq::Error::HostNotFound => TransportError::Network("DNS lookup failed"),
        ureq::Error::Io(error) => classify_io(error),
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) | ureq::Error::Pem(_) => {
            TransportError::Network("TLS failed")
        }
        _ => TransportError::Network("network error"),
    }
}

fn classify_io(error: io::Error) -> TransportError {
    let kind = error.kind();
    if let Some(inner) = error.into_inner()
        && let Ok(inner) = inner.downcast::<ureq::Error>()
    {
        return classify(*inner);
    }
    match kind {
        io::ErrorKind::ConnectionRefused => TransportError::Network("connection refused"),
        io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted => {
            TransportError::Network("connection reset")
        }
        io::ErrorKind::UnexpectedEof | io::ErrorKind::BrokenPipe | io::ErrorKind::NotConnected => {
            TransportError::Network("connection closed")
        }
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => TransportError::Deadline,
        _ => TransportError::Network("network error"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ffi::OsStr;
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    use serde_json::Map;

    use crate::sourcegraph::{Endpoint, GRAPHQL_PATH, Transport, TransportError};

    use crate::sourcegraph::{Client, Request};

    mod server {
        use std::fmt::Write as _;
        use std::io::{Read, Write};
        use std::net::{Shutdown, TcpListener, TcpStream};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
        use std::thread;
        use std::time::Duration;

        #[derive(Clone, Debug)]
        pub struct Request {
            pub method: String,
            pub path: String,
            pub headers: Vec<(String, String)>,
            pub body: Vec<u8>,
        }

        impl Request {
            pub fn header(&self, name: &str) -> Option<&str> {
                self.headers.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
            }
        }

        pub enum Body {
            Bytes(Vec<u8>),
            Filler { byte: u8, len: u64 },
        }

        pub struct Response {
            pub status: u16,
            pub headers: Vec<(String, String)>,
            pub body: Body,
            pub delay: Option<Duration>,
            pub drop_connection: bool,
        }

        impl Response {
            pub fn new(status: u16) -> Response {
                Response {
                    status,
                    headers: Vec::new(),
                    body: Body::Bytes(Vec::new()),
                    delay: None,
                    drop_connection: false,
                }
            }

            pub fn header(mut self, name: &str, value: &str) -> Response {
                self.headers.push((name.to_owned(), value.to_owned()));
                self
            }

            pub fn body(mut self, body: impl Into<Vec<u8>>) -> Response {
                self.body = Body::Bytes(body.into());
                self
            }

            pub fn filler(mut self, byte: u8, len: u64) -> Response {
                self.body = Body::Filler { byte, len };
                self
            }

            pub fn delay(mut self, delay: Duration) -> Response {
                self.delay = Some(delay);
                self
            }

            pub fn drop_connection() -> Response {
                Response { drop_connection: true, ..Response::new(0) }
            }
        }

        pub struct Server {
            address: std::net::SocketAddr,
            requests: Arc<Mutex<Vec<Request>>>,
            stop: Arc<AtomicBool>,
        }

        impl Server {
            pub fn start(handler: impl Fn(Request) -> Response + Send + Sync + 'static) -> Server {
                let listener =
                    TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| panic!("bind: {e}"));
                let address =
                    listener.local_addr().unwrap_or_else(|e| panic!("local address: {e}"));
                let requests = Arc::new(Mutex::new(Vec::new()));
                let stop = Arc::new(AtomicBool::new(false));
                let handler = Arc::new(handler);
                {
                    let requests = Arc::clone(&requests);
                    let stop = Arc::clone(&stop);
                    thread::spawn(move || {
                        for stream in listener.incoming() {
                            if stop.load(Ordering::SeqCst) {
                                break;
                            }
                            let Ok(stream) = stream else { continue };
                            let requests = Arc::clone(&requests);
                            let handler = Arc::clone(&handler);
                            thread::spawn(move || {
                                let _ = serve(stream, &requests, &*handler);
                            });
                        }
                    });
                }
                Server { address, requests, stop }
            }

            pub fn url(&self) -> String {
                format!("http://{}", self.address)
            }

            pub fn requests(&self) -> Vec<Request> {
                lock(&self.requests).clone()
            }
        }

        impl Drop for Server {
            fn drop(&mut self) {
                self.stop.store(true, Ordering::SeqCst);
                let _ = TcpStream::connect(self.address);
            }
        }

        fn lock(requests: &Mutex<Vec<Request>>) -> MutexGuard<'_, Vec<Request>> {
            requests.lock().unwrap_or_else(PoisonError::into_inner)
        }

        fn serve(
            mut stream: TcpStream,
            requests: &Mutex<Vec<Request>>,
            handler: &dyn Fn(Request) -> Response,
        ) -> std::io::Result<()> {
            let request = read_request(&mut stream)?;
            lock(requests).push(request.clone());
            let response = handler(request);
            if let Some(delay) = response.delay {
                thread::sleep(delay);
            }
            if response.drop_connection {
                return stream.shutdown(Shutdown::Both);
            }
            let len = match &response.body {
                Body::Bytes(bytes) => bytes.len() as u64,
                Body::Filler { len, .. } => *len,
            };
            let mut head = format!(
                "HTTP/1.1 {} X\r\nContent-Length: {len}\r\nConnection: close\r\n",
                response.status
            );
            for (name, value) in &response.headers {
                let _ = write!(head, "{name}: {value}\r\n");
            }
            head.push_str("\r\n");
            stream.write_all(head.as_bytes())?;
            match response.body {
                Body::Bytes(bytes) => stream.write_all(&bytes)?,
                Body::Filler { byte, len } => {
                    let chunk = vec![byte; 64 * 1024];
                    let mut left = len;
                    while left > 0 {
                        let n =
                            usize::try_from(left).map_or(chunk.len(), |left| left.min(chunk.len()));
                        stream.write_all(&chunk[..n])?;
                        left -= n as u64;
                    }
                }
            }
            stream.flush()?;
            stream.shutdown(Shutdown::Both)
        }

        fn read_request(stream: &mut TcpStream) -> std::io::Result<Request> {
            let mut buf = Vec::new();
            let mut byte = [0u8; 1];
            while !buf.ends_with(b"\r\n\r\n") {
                if stream.read(&mut byte)? == 0 {
                    return Err(std::io::ErrorKind::UnexpectedEof.into());
                }
                buf.push(byte[0]);
            }
            let text = String::from_utf8_lossy(&buf).into_owned();
            let mut lines = text.split("\r\n");
            let mut first = lines.next().unwrap_or_default().split(' ');
            let method = first.next().unwrap_or_default().to_owned();
            let path = first.next().unwrap_or_default().to_owned();
            let headers: Vec<(String, String)> = lines
                .filter_map(|line| line.split_once(':'))
                .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
                .collect();
            let len = headers
                .iter()
                .find(|(name, _)| name == "content-length")
                .and_then(|(_, value)| value.parse::<usize>().ok())
                .unwrap_or(0);
            let mut body = vec![0u8; len];
            stream.read_exact(&mut body)?;
            Ok(Request { method, path, headers, body })
        }
    }

    use server::{Response, Server};

    fn client(server: &Server) -> Ureq {
        let value = server.url();
        let endpoint = value.parse::<Endpoint>().unwrap_or_else(|e| panic!("{e}"));
        Ureq::without_proxy(endpoint)
    }

    fn soon() -> Instant {
        Instant::now() + Duration::from_secs(30)
    }

    #[test]
    fn an_endpoint_is_an_http_or_https_url_with_a_host() {
        let url = |s: &str| s.parse::<Endpoint>().map(|e| e.url(GRAPHQL_PATH));
        assert_eq!(Endpoint::default().url(GRAPHQL_PATH), "https://sourcegraph.com/.api/graphql");
        assert_eq!(url("https://host/sg/"), Ok("https://host/sg/.api/graphql".to_owned()));
        assert_eq!(url("http://127.0.0.1:9"), Ok("http://127.0.0.1:9/.api/graphql".to_owned()));
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
                url(bad),
                Err("must be an http:// or https:// URL with a host and no query or fragment"
                    .to_owned()),
                "{bad}"
            );
        }
    }

    #[test]
    fn every_request_identifies_sego_in_its_headers() {
        let server = Server::start(|_| Response::new(200).body("{}"));
        client(&server).post(GRAPHQL_PATH, b"{}", soon()).expect("post");
        client(&server).get("/r/x", soon()).expect("get");
        let requests = server.requests();
        assert_eq!(requests.len(), 2);
        for request in &requests {
            let client = format!("sego/{VERSION}");
            assert_eq!(request.header("user-agent"), Some(client.as_str()));
            assert_eq!(request.header("x-requested-with"), Some(client.as_str()));
            assert_eq!(request.header("x-sourcegraph-api-client-name"), Some("sego"));
            assert_eq!(request.header("x-sourcegraph-api-client-version"), Some(VERSION));
            assert!(request.header("accept-encoding").is_some_and(|v| { v.contains("gzip") }));
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
    fn a_response_with_an_error_status_returns_its_body_and_retry_after() {
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
        let server =
            Server::start(|_| Response::new(302).header("Location", "/next").body("moved"));
        for response in [
            client(&server).get("/r/a", soon()).expect("get"),
            client(&server).post(GRAPHQL_PATH, b"{}", soon()).expect("post"),
        ] {
            assert_eq!(response.status, 302);
            assert_eq!(response.location.as_deref(), Some("/next"));
        }
        assert_eq!(server.requests().len(), 2);
    }

    #[test]
    fn a_body_over_the_limit_is_too_large_and_one_at_the_limit_is_read() {
        let server = Server::start(|request| {
            let len = if request.path == "/over" { BODY_LIMIT + 1 } else { BODY_LIMIT };
            Response::new(200).filler(b'x', len)
        });
        let over = client(&server).get("/over", soon());
        assert!(matches!(over, Err(TransportError::TooLarge)), "{over:?}");
        let exact = client(&server).get("/exact", soon()).expect("exact");
        assert_eq!(exact.body.len() as u64, BODY_LIMIT);
    }

    #[test]
    fn a_slow_response_fails_at_the_deadline_and_a_past_deadline_sends_nothing() {
        let server =
            Server::start(|_| Response::new(200).delay(Duration::from_secs(3)).body("late"));
        let started = Instant::now();
        let slow = client(&server).post(GRAPHQL_PATH, b"{}", started + Duration::from_millis(100));
        assert!(matches!(slow, Err(TransportError::Deadline)), "{slow:?}");
        assert!(started.elapsed() < Duration::from_secs(1));

        let before = server.requests().len();
        let past = Instant::now();
        let gone = client(&server).get("/r/x", past);
        assert!(matches!(gone, Err(TransportError::Deadline)), "{gone:?}");
        assert_eq!(server.requests().len(), before);
    }

    #[test]
    fn a_refused_or_dropped_connection_is_a_network_error_naming_its_cause() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| panic!("{e}"));
        let port = listener.local_addr().unwrap_or_else(|e| panic!("{e}")).port();
        drop(listener);
        let value = format!("http://127.0.0.1:{port}");
        let closed =
            Ureq::without_proxy(value.parse::<Endpoint>().unwrap_or_else(|e| panic!("{e}")));
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
                Err(TransportError::Network("connection closed" | "connection reset"))
            ),
            "{dropped:?}"
        );
    }

    #[test]
    fn endpoint_host_is_host_and_port_without_credentials() {
        let host = |s: &str| s.parse::<Endpoint>().unwrap_or_else(|e| panic!("{e:?}")).host();
        assert_eq!(Endpoint::default().host(), "sourcegraph.com");
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
        let endpoint = server.url().parse::<Endpoint>().unwrap_or_else(|e| panic!("{e}"));
        let cache = endpoint.cache(Some(OsStr::new("off")));
        let connect = || Client::new(Box::new(client(&server)), endpoint.clone(), &cache, soon());

        let request = Request {
            operation: "Op".to_owned(),
            document: "query Op { x }".to_owned(),
            variables: Map::new(),
        };
        let Err(error) = connect().exchange(&request) else {
            panic!("a 500 was accepted");
        };
        assert_eq!(error.to_string(), format!("{host} answered HTTP 500"));
        let Err(error) = connect().raw("/raw") else {
            panic!("a 500 was read");
        };
        assert_eq!(error.to_string(), format!("{host} answered HTTP 500"));
    }

    #[test]
    fn a_refused_connection_names_the_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| panic!("bind: {e}"));
        let address = listener.local_addr().unwrap_or_else(|e| panic!("{e}"));
        drop(listener);
        let url = format!("http://{address}");
        let endpoint = url.parse::<Endpoint>().unwrap_or_else(|e| panic!("{e}"));
        let cache = endpoint.cache(Some(OsStr::new("off")));
        let client =
            Client::new(Box::new(Ureq::without_proxy(endpoint.clone())), endpoint, &cache, soon());
        let request = Request {
            operation: "Op".to_owned(),
            document: "query Op { x }".to_owned(),
            variables: Map::new(),
        };
        let Err(error) = client.exchange(&request) else {
            panic!("a closed port answered");
        };
        assert_eq!(error.to_string(), format!("could not reach {address}: connection refused"));
    }

    #[test]
    fn transport_errors_name_the_host() {
        assert_eq!(
            TransportError::Deadline.into_error("127.0.0.1:1").to_string(),
            "no answer from 127.0.0.1:1 within 90 s"
        );
        assert_eq!(
            TransportError::Network("connection refused").into_error("127.0.0.1:1").to_string(),
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
        let response =
            client(&server).post(GRAPHQL_PATH, b"{}", soon()).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(response.status, 200);
        assert_eq!(response.body, json);
    }
}
