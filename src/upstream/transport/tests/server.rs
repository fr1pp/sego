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
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
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
    pub fn start(
        handler: impl Fn(Request) -> Response + Send + Sync + 'static,
    ) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0")
            .unwrap_or_else(|e| panic!("bind: {e}"));
        let address = listener
            .local_addr()
            .unwrap_or_else(|e| panic!("local address: {e}"));
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
                let n = usize::try_from(left)
                    .map_or(chunk.len(), |left| left.min(chunk.len()));
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
        .map(|(name, value)| {
            (name.trim().to_ascii_lowercase(), value.trim().to_owned())
        })
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
