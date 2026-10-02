//! The WebSocket server: accepts connections on 127.0.0.1 and passes each
//! request to the page thread.
//!
//! Each connection has its own thread. It reads a request, sends it to the
//! page thread with a reply channel, waits for the response and writes it.
//! Requests of one connection are therefore handled in order.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use tungstenite::error::CapacityError;
use tungstenite::handshake::server::{
    ErrorResponse, Request as HttpRequest, Response as HttpResponse,
};
use tungstenite::http::StatusCode;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::protocol::frame::CloseFrame;
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::{Message, WebSocket};

use crate::protocol::{Request, Response, RpcError};

/// The largest request message (and frame) that the server accepts.
const MAX_REQUEST_SIZE: usize = 1 << 20;

/// The most connections at the same time.
const MAX_CONNECTIONS: usize = 16;

/// How long a client may take for the WebSocket handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// After closing a connection for an oversized request: how long and how
/// much input to read and discard, so that the close frame reaches the
/// client instead of a TCP reset.
const DRAIN_TIMEOUT: Duration = Duration::from_millis(500);
const DRAIN_LIMIT: usize = 8 << 20;

/// A request and the channel for its response.
pub(crate) struct Command {
    pub(crate) request: Request,
    pub(crate) reply: Sender<Reply>,
}

/// A response, and optionally a channel on which the connection thread
/// reports that it wrote the response (so that swb can exit after
/// `browser.close` without losing the answer).
pub(crate) struct Reply {
    pub(crate) response: Response,
    pub(crate) written: Option<Sender<()>>,
}

impl Reply {
    pub(crate) fn new(response: Response) -> Self {
        Reply {
            response,
            written: None,
        }
    }
}

/// Called from server threads when a command arrives, to wake the page
/// thread.
pub(crate) type Wake = Arc<dyn Fn() + Send + Sync>;

/// A listening server. Dropping it stops accepting connections.
pub(crate) struct Listener {
    pub(crate) port: u16,
    pub(crate) commands: Receiver<Command>,
    shutdown: Arc<AtomicBool>,
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // Wake the accepting thread, which then sees the flag. If the
        // connection fails, the thread ends with the process.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

/// Starts listening on 127.0.0.1:`port` (0 picks a free port).
pub(crate) fn listen(port: u16, wake: Wake) -> io::Result<Listener> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let port = listener.local_addr()?.port();
    let (commands, receiver) = mpsc::channel();
    let shutdown = Arc::new(AtomicBool::new(false));
    let stop = Arc::clone(&shutdown);
    thread::Builder::new()
        .name("automation".to_owned())
        .spawn(move || accept(&listener, &commands, &wake, &stop))?;
    Ok(Listener {
        port,
        commands: receiver,
        shutdown,
    })
}

fn accept(listener: &TcpListener, commands: &Sender<Command>, wake: &Wake, stop: &AtomicBool) {
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                log::warn!("automation: cannot accept a connection: {e}");
                continue;
            }
        };
        if active.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
            log::warn!("automation: more than {MAX_CONNECTIONS} connections; refused");
            continue;
        }
        active.fetch_add(1, Ordering::SeqCst);
        let (commands, wake, done) = (commands.clone(), Arc::clone(wake), Arc::clone(&active));
        let spawned = thread::Builder::new()
            .name("automation-connection".to_owned())
            .spawn(move || {
                serve(stream, &commands, &wake);
                done.fetch_sub(1, Ordering::SeqCst);
            });
        if let Err(e) = spawned {
            active.fetch_sub(1, Ordering::SeqCst);
            log::warn!("automation: cannot start a connection thread: {e}");
        }
    }
}

/// Rejects WebSocket handshakes from web pages. Browsers always send an
/// `Origin` header; the swb clients do not. Without this check, any page
/// open in a browser on this machine could drive swb.
#[allow(clippy::result_large_err)] // The signature is tungstenite's.
fn check_origin(
    request: &HttpRequest,
    response: HttpResponse,
) -> Result<HttpResponse, ErrorResponse> {
    if request.headers().contains_key("origin") {
        let mut error =
            ErrorResponse::new(Some("connections from web pages are not allowed".into()));
        *error.status_mut() = StatusCode::FORBIDDEN;
        return Err(error);
    }
    Ok(response)
}

/// A TCP stream whose reads fail after a deadline (for the handshake).
struct Connection {
    stream: TcpStream,
    deadline: Option<Instant>,
}

impl Read for Connection {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(deadline) = self.deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::ErrorKind::TimedOut.into());
            }
            self.stream.set_read_timeout(Some(remaining))?;
        }
        self.stream.read(buf)
    }
}

impl Write for Connection {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

fn serve(stream: TcpStream, commands: &Sender<Command>, wake: &Wake) {
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_REQUEST_SIZE))
        .max_frame_size(Some(MAX_REQUEST_SIZE));
    // A client that does not complete the handshake in time must not keep
    // the thread.
    let connection = Connection {
        stream,
        deadline: Some(Instant::now() + HANDSHAKE_TIMEOUT),
    };
    let mut socket =
        match tungstenite::accept_hdr_with_config(connection, check_origin, Some(config)) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("automation: handshake failed: {e}");
                return;
            }
        };
    let connection = socket.get_mut();
    connection.deadline = None;
    if let Err(e) = connection.stream.set_read_timeout(None) {
        log::warn!("automation: {e}");
        return;
    }
    while let Some(reply) = next_reply(&mut socket, commands, wake) {
        let sent = send(&mut socket, &reply.response);
        if let Some(written) = reply.written {
            let _ = written.send(());
        }
        if sent.is_err() {
            return;
        }
    }
}

/// Reads the next request and returns its reply: from the page thread, or
/// an error for a message that is not a request. `None` when the
/// connection ends.
fn next_reply(
    socket: &mut WebSocket<Connection>,
    commands: &Sender<Command>,
    wake: &Wake,
) -> Option<Reply> {
    let error = |code, message: String| {
        Some(Reply::new(Response::new(
            Value::Null,
            Err(RpcError::new(code, message)),
        )))
    };
    let text = loop {
        match socket.read() {
            Ok(Message::Text(text)) => break text,
            Ok(Message::Binary(_)) => {
                return error(
                    RpcError::INVALID_REQUEST,
                    "binary messages are not supported".to_owned(),
                );
            }
            // Pings are answered by tungstenite.
            Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => {}
            Err(tungstenite::Error::Capacity(CapacityError::MessageTooLong { .. })) => {
                let frame = CloseFrame {
                    code: CloseCode::Size,
                    reason: "the request is larger than 1 MiB".into(),
                };
                let _ = socket.close(Some(frame));
                let _ = socket.flush();
                drain(&mut socket.get_mut().stream);
                return None;
            }
            Ok(Message::Close(_)) | Err(_) => return None,
        }
    };
    match serde_json::from_str::<Request>(&text) {
        Ok(request) => {
            let (reply, response) = mpsc::channel();
            // Fails if the page is gone.
            commands.send(Command { request, reply }).ok()?;
            wake();
            response.recv().ok()
        }
        Err(e) => {
            let code = if serde_json::from_str::<Value>(&text).is_ok() {
                RpcError::INVALID_REQUEST
            } else {
                RpcError::PARSE_ERROR
            };
            error(code, e.to_string())
        }
    }
}

/// Stops writing and reads and discards what the client still sends (up
/// to [`DRAIN_LIMIT`] bytes or [`DRAIN_TIMEOUT`]). Closing a socket with
/// unread input sends a TCP reset, which can destroy the close frame
/// before the client reads it.
fn drain(stream: &mut TcpStream) {
    let _ = stream.shutdown(Shutdown::Write);
    let deadline = Instant::now() + DRAIN_TIMEOUT;
    let mut buf = [0; 16 * 1024];
    let mut total = 0;
    while total < DRAIN_LIMIT {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || stream.set_read_timeout(Some(remaining)).is_err() {
            return;
        }
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => total += n,
        }
    }
}

fn send(socket: &mut WebSocket<Connection>, response: &Response) -> Result<(), tungstenite::Error> {
    // Serializing JSON values does not fail; the fallback is only for
    // completeness.
    let text = serde_json::to_string(response).unwrap_or_else(|_| {
        r#"{"id":null,"error":{"code":-32603,"message":"cannot serialize the response"}}"#
            .to_owned()
    });
    socket.send(Message::text(text))
}
