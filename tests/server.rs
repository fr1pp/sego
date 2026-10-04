//! A mock upstream on `std::net`: each step answers one connection, in
//! order, and every request is checked.

use std::fmt::Write as _;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const USER_AGENT: &str = concat!("sego/", env!("CARGO_PKG_VERSION"));

/// How long the server waits between polls for a connection.
const POLL: Duration = Duration::from_millis(5);

/// The longest a connected client may take to send its request.
const READ_TIMEOUT: Duration = Duration::from_secs(10);

pub enum Step {
    /// Checks the request against the capture's `request`, a path under
    /// `tests/`, and answers with its `response`.
    Capture(&'static str),
    /// Checks the request against the capture's `request` with its query
    /// variable replaced by `query`, and answers with its `response`.
    Search { capture: &'static str, query: &'static str },
    /// Answers with this status, headers and body.
    Status { status: u16, headers: Vec<(&'static str, String)>, body: String },
    /// Reads the request, then closes the connection without answering.
    Drop,
    /// Waits this long before taking the next step's connection, so that
    /// step answers late. Takes no connection itself.
    Stall(Duration),
}

pub struct Server {
    url: String,
    finished: Arc<AtomicBool>,
    thread: JoinHandle<Vec<String>>,
}

impl Server {
    /// The endpoint: `http://127.0.0.1:<port>`.
    pub fn url(&self) -> String {
        self.url.clone()
    }

    /// Waits for the steps to be taken, once the client has exited, and
    /// panics if a request didn't match, a step went unused or a request
    /// came after the last step.
    pub fn finish(self) {
        self.finished.store(true, Ordering::SeqCst);
        let problems = self.thread.join().expect("the server thread ends");
        assert!(problems.is_empty(), "mock server:\n{}", problems.join("\n"));
    }
}

/// Serves `steps` on a random local port.
pub fn start(steps: Vec<Step>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a port");
    listener.set_nonblocking(true).expect("a nonblocking listener");
    let url = format!("http://{}", listener.local_addr().unwrap());
    let finished = Arc::new(AtomicBool::new(false));
    let done = Arc::clone(&finished);
    let thread = thread::spawn(move || serve(&listener, steps, &done));
    Server { url, finished, thread }
}

/// Where `unreachable` looks for a free port: below the default ephemeral
/// ranges (Linux 32768-60999, macOS 49152-65535) that a bind to port 0, as
/// every mock server makes, draws from.
const UNREACHABLE_PORTS: Range<u16> = 20_000..32_768;

/// The URL of a local port that nothing listens on: the first port in
/// `UNREACHABLE_PORTS` that binds, found once per test process. That one
/// bind ends before any caller knows the port, and no mock server can be
/// given it unless the host's ephemeral range reaches below 32768; an
/// unrelated process can still bind it.
pub fn unreachable() -> String {
    static URL: OnceLock<String> = OnceLock::new();
    URL.get_or_init(|| {
        let listener = UNREACHABLE_PORTS
            .clone()
            .find_map(|port| TcpListener::bind(("127.0.0.1", port)).ok())
            .expect("bind a port below the ephemeral range");
        format!("http://{}", listener.local_addr().unwrap())
    })
    .clone()
}

/// Takes each step in turn, then refuses any further request. Returns the
/// problems found.
fn serve(
    listener: &TcpListener,
    steps: Vec<Step>,
    finished: &AtomicBool,
) -> Vec<String> {
    let mut problems = Vec::new();
    let count = steps.len();
    for (i, step) in steps.into_iter().enumerate() {
        if let Step::Stall(delay) = step {
            thread::sleep(delay);
            continue;
        }
        let Some(stream) = next_connection(listener, finished) else {
            problems.push(format!("steps {}-{count} went unused", i + 1));
            return problems;
        };
        if let Err(problem) = answer(stream, step) {
            problems.push(format!("step {}: {problem}", i + 1));
        }
    }
    while let Some(stream) = next_connection(listener, finished) {
        let line = Request::read(stream).map_or_else(|e| e, |r| r.line);
        problems.push(format!("a request after the last step: {line}"));
    }
    problems
}

/// The next connection, or `None` once the client has finished and none is
/// waiting. The client has exited before `finished` is set, so every
/// connection it made is already queued.
fn next_connection(
    listener: &TcpListener,
    finished: &AtomicBool,
) -> Option<TcpStream> {
    loop {
        let finished = finished.load(Ordering::SeqCst);
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).expect("a blocking stream");
                stream.set_read_timeout(Some(READ_TIMEOUT)).ok();
                return Some(stream);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if finished {
                    return None;
                }
                thread::sleep(POLL);
            }
            Err(error) => panic!("accept: {error}"),
        }
    }
}

fn answer(stream: TcpStream, step: Step) -> Result<(), String> {
    let request = Request::read(stream)?;
    request.check()?;
    match step {
        Step::Capture(path) => respond_with(request, path, None),
        Step::Search { capture, query } => {
            respond_with(request, capture, Some(query))
        }
        Step::Status { status, headers, body } => {
            request.respond(status, &headers, &body)
        }
        Step::Drop => Ok(()),
        Step::Stall(_) => unreachable!("a stall takes no connection"),
    }
}

/// Answers with the capture at `path`, checking the request against its
/// own, with the query variable replaced when `query` is given.
fn respond_with(
    request: Request,
    path: &str,
    query: Option<&str>,
) -> Result<(), String> {
    let capture = capture(path);
    let mut expected = capture["request"].clone();
    if let Some(query) = query {
        expected["variables"]["query"] = query.into();
    }
    let mismatch = (request.json() != expected)
        .then(|| format!("the request doesn't match {path}"));
    let status = capture["response"]["status"]
        .as_u64()
        .and_then(|status| u16::try_from(status).ok())
        .expect("a capture's status");
    let body = capture["response"]["body"].to_string();
    request.respond(status, &[], &body)?;
    mismatch.map_or(Ok(()), Err)
}

/// The capture at `path`, under `tests/`.
pub fn capture(path: &str) -> serde_json::Value {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join(path);
    let text = fs::read_to_string(&file)
        .unwrap_or_else(|error| panic!("{}: {error}", file.display()));
    serde_json::from_str(&text).expect("a capture is JSON")
}

struct Request {
    line: String,
    agent: Option<String>,
    body: String,
    stream: TcpStream,
}

impl Request {
    /// Reads the request line, the headers and a `Content-Length` body.
    fn read(stream: TcpStream) -> Result<Request, String> {
        let mut reader = BufReader::new(stream);
        let mut read_line = || {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(_) => Ok(line.trim_end().to_owned()),
                Err(error) => Err(error.to_string()),
            }
        };
        let line = read_line()?;
        let (mut length, mut agent) = (None, None);
        loop {
            let header = read_line()?;
            if header.is_empty() {
                break;
            }
            let (name, value) =
                header.split_once(':').unwrap_or((&header, ""));
            let value = value.trim();
            match name.to_ascii_lowercase().as_str() {
                "content-length" => length = value.parse::<usize>().ok(),
                "user-agent" => agent = Some(value.to_owned()),
                _ => {}
            }
        }
        let mut body = vec![0; length.ok_or("no Content-Length")?];
        reader.read_exact(&mut body).map_err(|error| error.to_string())?;
        let body = String::from_utf8(body).map_err(|e| e.to_string())?;
        Ok(Request { line, agent, body, stream: reader.into_inner() })
    }

    /// Whether this is sego's POST to the GraphQL path.
    fn check(&self) -> Result<(), String> {
        if self.line != "POST /.api/graphql HTTP/1.1" {
            return Err(format!("unexpected request line {:?}", self.line));
        }
        if self.agent.as_deref() != Some(USER_AGENT) {
            return Err(format!("unexpected user agent {:?}", self.agent));
        }
        Ok(())
    }

    fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap_or_default()
    }

    /// Answers and closes the connection. `x-trace` is `test-trace` unless
    /// `headers` has one.
    fn respond(
        mut self,
        status: u16,
        headers: &[(&str, String)],
        body: &str,
    ) -> Result<(), String> {
        let mut head = format!(
            "HTTP/1.1 {status} mock\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n",
            body.len()
        );
        if !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("x-trace"))
        {
            head.push_str("x-trace: test-trace\r\n");
        }
        for (name, value) in headers {
            let _ = write!(head, "{name}: {value}\r\n");
        }
        head.push_str("\r\n");
        head.push_str(body);
        self.stream.write_all(head.as_bytes()).map_err(|e| e.to_string())
    }
}
