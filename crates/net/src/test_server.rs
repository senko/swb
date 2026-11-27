//! A local HTTP/1.1 server for the tests of the media byte source (M14
//! feature 4): it serves bodies with or without `Range` (206, 416,
//! `Accept-Ranges`), and misbehaves on demand (a body that ends early, a
//! server that stalls, redirects, an endless body). It is only built for
//! tests (`cfg(test)` or the feature `test-server`); the tests never use
//! the network.
//!
//! One thread per connection, one request per connection
//! (`Connection: close`). Dropping the server stops it: handlers that
//! stall end, and the listener closes.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use url::Url;

use crate::range::{Resolved, parse_range, resolve};

/// The bytes of a route.
#[derive(Clone, Debug)]
pub enum Body {
    /// Bytes in memory.
    Memory(Arc<Vec<u8>>),
    /// A file, read at the offset of each request.
    File(PathBuf),
    /// `len` bytes that the server makes up ([`pattern_byte`]): a large body
    /// without memory.
    Pattern(u64),
    /// Bytes without an end (no `Content-Length`).
    Endless,
}

/// The byte at `index` of a [`Body::Pattern`] and [`Body::Endless`] body.
pub fn pattern_byte(index: u64) -> u8 {
    (index.wrapping_mul(2_654_435_761) >> 13) as u8
}

/// How a route breaks the rules of range requests. Each lie applies to the
/// requests after the first one, so that a resource opens and the lie shows
/// at a reopen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lie {
    /// A 206 whose `Content-Range` starts one byte after the requested one.
    ShiftedStart,
    /// A 206 whose `Content-Range` total is one more than before.
    ChangingTotal,
    /// A 200 with the whole body for a range that starts after byte 0.
    WholeForLaterRanges,
}

/// How the server answers one path.
#[derive(Clone, Debug)]
#[must_use]
pub struct Route {
    /// The body.
    pub body: Body,
    /// The `Content-Type`.
    pub content_type: String,
    /// True: answers `Range` (206, 416) and sends `Accept-Ranges: bytes`.
    /// False: ignores `Range` and answers 200 with the whole body.
    pub ranges: bool,
    /// The status of a route that is not a body (404, 500).
    pub status: u16,
    /// A 302 to this path.
    pub redirect: Option<String>,
    /// Closes the connection after this many body bytes of a response, for
    /// the next `close_times` responses.
    pub close_after: Option<u64>,
    /// How many responses `close_after` applies to.
    pub close_times: Arc<AtomicUsize>,
    /// Stops after this many body bytes of a response and waits (until the
    /// server drops) without closing.
    pub stall_after: Option<u64>,
    /// Waits this long before the status line (a server that does not
    /// answer).
    pub hold_headers: Option<Duration>,
    /// More response headers.
    pub headers: Vec<(String, String)>,
    /// The number of requests left that get the body; then every request
    /// gets the status.
    pub ok_requests: Option<(Arc<AtomicUsize>, u16)>,
    /// Waits this long after this many body bytes of a response, then goes
    /// on (a paused source).
    pub pause_after: Option<(u64, Duration)>,
    /// A broken range answer.
    pub lie: Option<Lie>,
    /// The number of body responses so far (for [`Lie::ChangingTotal`]).
    pub answered: Arc<AtomicUsize>,
}

impl Route {
    /// A route that serves `body` with range support.
    pub fn new(body: Body) -> Route {
        Route {
            body,
            content_type: "video/mp4".to_owned(),
            ranges: true,
            status: 200,
            redirect: None,
            close_after: None,
            close_times: Arc::new(AtomicUsize::new(0)),
            stall_after: None,
            hold_headers: None,
            headers: Vec::new(),
            ok_requests: None,
            pause_after: None,
            lie: None,
            answered: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Breaks the rules of range requests after the first answer.
    pub fn lying(mut self, lie: Lie) -> Route {
        self.lie = Some(lie);
        self
    }

    /// Answers the first `n` requests with the body and the later ones with
    /// `status`.
    pub fn failing_after(mut self, n: usize, status: u16) -> Route {
        self.ok_requests = Some((Arc::new(AtomicUsize::new(n)), status));
        self
    }

    /// Adds a response header.
    pub fn with_header(mut self, name: &str, value: &str) -> Route {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    /// Waits `time` before it answers.
    pub fn holding_headers(mut self, time: Duration) -> Route {
        self.hold_headers = Some(time);
        self
    }

    /// A route that serves bytes in memory.
    pub fn bytes(data: impl Into<Vec<u8>>) -> Route {
        Route::new(Body::Memory(Arc::new(data.into())))
    }

    /// A route that serves a file.
    pub fn file(path: impl Into<PathBuf>) -> Route {
        Route::new(Body::File(path.into()))
    }

    /// A route that ignores `Range`.
    pub fn without_ranges(mut self) -> Route {
        self.ranges = false;
        self
    }

    /// A route with this status and a short text body.
    pub fn status(code: u16) -> Route {
        let mut route = Route::bytes(b"status".to_vec());
        route.status = code;
        route
    }

    /// A route that redirects to `path`.
    pub fn redirect(path: &str) -> Route {
        let mut route = Route::status(302);
        route.redirect = Some(path.to_owned());
        route
    }

    /// Closes the connection after `bytes` body bytes, `times` times.
    pub fn closing_after(mut self, bytes: u64, times: usize) -> Route {
        self.close_after = Some(bytes);
        self.close_times = Arc::new(AtomicUsize::new(times));
        self
    }

    /// Stops sending after `bytes` body bytes and waits.
    pub fn stalling_after(mut self, bytes: u64) -> Route {
        self.stall_after = Some(bytes);
        self
    }
}

/// A request that the server received.
#[derive(Clone, Debug)]
pub struct Logged {
    /// The request target.
    pub path: String,
    /// The `Range` header.
    pub range: Option<String>,
    /// All headers, names in lowercase.
    pub headers: HashMap<String, String>,
}

#[derive(Default)]
struct Shared {
    routes: Mutex<HashMap<String, Route>>,
    log: Mutex<Vec<Logged>>,
    stop: AtomicBool,
    /// The body bytes written to clients.
    sent: AtomicU64,
    /// The handlers that run.
    live: AtomicUsize,
}

/// The server. Dropping it stops it.
pub struct TestServer {
    addr: SocketAddr,
    shared: Arc<Shared>,
}

impl TestServer {
    /// Starts a server on 127.0.0.1 with the given routes.
    pub fn start(routes: Vec<(&str, Route)>) -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
        let addr = listener
            .local_addr()
            .expect("a bound listener has an address");
        let shared = Arc::new(Shared {
            routes: Mutex::new(
                routes
                    .into_iter()
                    .map(|(path, route)| (path.to_owned(), route))
                    .collect(),
            ),
            ..Shared::default()
        });
        let accepting = Arc::clone(&shared);
        thread::spawn(move || {
            for stream in listener.incoming() {
                if accepting.stop.load(Ordering::Relaxed) {
                    return;
                }
                let Ok(stream) = stream else { continue };
                let shared = Arc::clone(&accepting);
                shared.live.fetch_add(1, Ordering::Relaxed);
                thread::spawn(move || {
                    handle(&shared, &stream);
                    shared.live.fetch_sub(1, Ordering::Relaxed);
                });
            }
        });
        TestServer { addr, shared }
    }

    /// The URL of `path` on this server.
    pub fn url(&self, path: &str) -> Url {
        Url::parse(&format!("http://{}{path}", self.addr)).expect("a loopback URL is valid")
    }

    /// Adds or replaces a route.
    pub fn set(&self, path: &str, route: Route) {
        self.shared
            .routes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path.to_owned(), route);
    }

    /// The requests received so far.
    pub fn requests(&self) -> Vec<Logged> {
        self.shared
            .log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The body bytes written to clients so far.
    pub fn bytes_sent(&self) -> u64 {
        self.shared.sent.load(Ordering::Relaxed)
    }

    /// The handlers that run (connections that are open or stalled).
    pub fn live_connections(&self) -> usize {
        self.shared.live.load(Ordering::Relaxed)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        // Wake the accept loop.
        let _ = TcpStream::connect(self.addr);
    }
}

fn push_header(out: &mut String, name: &str, value: &str) {
    out.push_str(name);
    out.push_str(": ");
    out.push_str(value);
    out.push_str("\r\n");
}

fn handle(shared: &Shared, stream: &TcpStream) {
    let Some((path, headers)) = read_request(stream) else {
        return;
    };
    let range = headers.get("range").cloned();
    shared
        .log
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(Logged {
            path: path.clone(),
            range: range.clone(),
            headers,
        });
    let route = shared
        .routes
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&path)
        .cloned();
    let Some(route) = route else {
        let _ = respond_text(stream, 404, &[]);
        return;
    };
    if let Some(hold) = route.hold_headers {
        wait(shared, hold);
    }
    if let Some(location) = &route.redirect {
        let _ = respond_text(stream, 302, &[("Location", location)]);
        return;
    }
    if route.status != 200 {
        let _ = respond_text(stream, route.status, &[]);
        return;
    }
    if let Some((left, status)) = &route.ok_requests
        && left
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1))
            .is_err()
    {
        let _ = respond_text(stream, *status, &[]);
        return;
    }
    let _ = respond_body(shared, stream, &route, range.as_deref());
}

/// Waits up to `time`, less when the server stops.
fn wait(shared: &Shared, time: Duration) {
    let step = Duration::from_millis(10);
    let mut waited = Duration::ZERO;
    while waited < time && !shared.stop.load(Ordering::Relaxed) {
        thread::sleep(step);
        waited += step;
    }
}

fn read_request(stream: &TcpStream) -> Option<(String, HashMap<String, String>)> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let path = line.split(' ').nth(1)?.to_owned();
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let line = line.trim_end();
        if line.is_empty() {
            return Some((path, headers));
        }
        let (name, value) = line.split_once(':')?;
        headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        206 => "Partial Content",
        302 => "Found",
        404 => "Not Found",
        416 => "Range Not Satisfiable",
        500 => "Internal Server Error",
        _ => "Status",
    }
}

fn respond_text(
    mut stream: &TcpStream,
    status: u16,
    extra: &[(&str, &str)],
) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Length: 0\r\nConnection: close\r\n",
        reason(status)
    );
    for (name, value) in extra {
        push_header(&mut head, name, value);
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())
}

/// The total length of a body, `None` for an endless one.
fn length(body: &Body) -> Option<u64> {
    match body {
        Body::Memory(data) => Some(data.len() as u64),
        Body::File(path) => std::fs::metadata(path).ok().map(|m| m.len()),
        Body::Pattern(len) => Some(*len),
        Body::Endless => None,
    }
}

fn respond_body(
    shared: &Shared,
    mut stream: &TcpStream,
    route: &Route,
    range: Option<&str>,
) -> std::io::Result<()> {
    let total = length(&route.body);
    let mut status = 200;
    let (mut start, mut count) = (0, total);
    let mut head = String::new();
    let earlier = route.answered.fetch_add(1, Ordering::Relaxed);
    if let (true, Some(total), Some(spec)) = (route.ranges, total, range.and_then(parse_range)) {
        match resolve(spec, total) {
            Resolved::Partial { start: first, end } => {
                // The lies apply to the requests after the first.
                let lie = route.lie.filter(|_| earlier > 0);
                if lie == Some(Lie::WholeForLaterRanges) && first > 0 {
                    // Answered below as 200 with the whole body.
                } else {
                    status = 206;
                    (start, count) = (first, Some(end - first + 1));
                    let shown_start = first + u64::from(lie == Some(Lie::ShiftedStart));
                    let shown_total = total + u64::from(lie == Some(Lie::ChangingTotal));
                    push_header(
                        &mut head,
                        "Content-Range",
                        &format!("bytes {shown_start}-{end}/{shown_total}"),
                    );
                }
            }
            Resolved::Unsatisfiable => {
                let text = format!(
                    "HTTP/1.1 416 {}\r\nContent-Range: bytes */{total}\r\nAccept-Ranges: bytes\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    reason(416)
                );
                return stream.write_all(text.as_bytes());
            }
            Resolved::Whole => {}
        }
    }
    let mut text = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {}\r\nConnection: close\r\n",
        reason(status),
        route.content_type
    );
    if route.ranges {
        text.push_str("Accept-Ranges: bytes\r\n");
    }
    for (name, value) in &route.headers {
        push_header(&mut text, name, value);
    }
    if let Some(count) = count {
        push_header(&mut text, "Content-Length", &count.to_string());
    }
    text.push_str(&head);
    text.push_str("\r\n");
    stream.write_all(text.as_bytes())?;
    stream_body(shared, stream, route, start, count)
}

/// Writes `count` bytes (all, for `None`) from `start`, with the faults of
/// the route.
fn stream_body(
    shared: &Shared,
    mut stream: &TcpStream,
    route: &Route,
    start: u64,
    count: Option<u64>,
) -> std::io::Result<()> {
    let close_after = route.close_after.filter(|_| {
        route
            .close_times
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1))
            .is_ok()
    });
    let mut file = match &route.body {
        Body::File(path) => {
            let mut file = File::open(path)?;
            file.seek(SeekFrom::Start(start))?;
            Some(file)
        }
        _ => None,
    };
    let mut sent = 0u64;
    let mut chunk = vec![0u8; 16 * 1024];
    while count.is_none_or(|count| sent < count) {
        let mut n = count.map_or(chunk.len() as u64, |count| {
            (count - sent).min(chunk.len() as u64)
        });
        for limit in [close_after, route.stall_after].into_iter().flatten() {
            n = n.min(limit.saturating_sub(sent));
        }
        if let Some((at, time)) = route.pause_after {
            if sent == at {
                wait(shared, time);
            }
            if sent < at {
                n = n.min(at - sent);
            }
        }
        if n == 0 {
            break;
        }
        let buffer = &mut chunk[..n as usize];
        fill_chunk(route, file.as_mut(), start + sent, buffer)?;
        stream.write_all(buffer)?;
        sent += n;
        shared.sent.fetch_add(n, Ordering::Relaxed);
    }
    if route.stall_after.is_some_and(|limit| sent >= limit) {
        // Hold the connection open until the server drops.
        while !shared.stop.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(10));
        }
    }
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

fn fill_chunk(
    route: &Route,
    file: Option<&mut File>,
    offset: u64,
    buffer: &mut [u8],
) -> std::io::Result<()> {
    match &route.body {
        Body::Memory(data) => {
            let from = usize::try_from(offset).unwrap_or(usize::MAX);
            let source = data.get(from..from + buffer.len()).unwrap_or_default();
            buffer[..source.len()].copy_from_slice(source);
        }
        Body::File(_) => {
            if let Some(file) = file {
                file.read_exact(buffer)?;
            }
        }
        Body::Pattern(_) | Body::Endless => {
            for (i, byte) in buffer.iter_mut().enumerate() {
                *byte = pattern_byte(offset + i as u64);
            }
        }
    }
    Ok(())
}
