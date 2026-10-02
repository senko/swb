//! Remote control of swb: a JSON protocol over WebSocket (see
//! `docs/automation.md` and `docs/adr/0008-automation-protocol.md`).
//!
//! - [`Automation`]: the server side. It listens on 127.0.0.1, receives
//!   requests on its own threads, and executes them on the page thread
//!   when the host calls [`Automation::process`].
//! - [`HeadlessBrowser`]: a page without a window, driven only by the
//!   server (`swb --headless --remote-port PORT`).
//! - [`Client`]: a blocking client, for tests and tools.

mod client;
mod headless;
mod methods;
mod protocol;
mod server;

use std::io;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use swb_engine::Page;

pub use client::{Client, ClientError};
pub use headless::HeadlessBrowser;
pub use protocol::{Request, Response, RpcError};

use methods::Outcome;
use server::{Command, Listener, Reply};

/// How long [`Automation::finish_close`] waits for the reply to be written.
const CLOSE_REPLY_TIMEOUT: Duration = Duration::from_secs(1);

/// A request that waits until the page is loaded.
struct Waiter {
    id: Value,
    reply: Sender<Reply>,
    deadline: Instant,
}

/// The automation server, as seen from the page thread.
pub struct Automation {
    listener: Listener,
    waiters: Vec<Waiter>,
    /// True after a client sent `browser.close`.
    close_requested: bool,
    /// After `browser.close`: reports when the reply was written.
    close_written: Option<Receiver<()>>,
}

impl Automation {
    /// Starts the server on 127.0.0.1:`port` (0 picks a free port). `wake`
    /// is called from a server thread when a request arrives; the host
    /// then calls [`Automation::process`] on the page thread.
    pub fn start(port: u16, wake: Arc<dyn Fn() + Send + Sync>) -> io::Result<Self> {
        Ok(Automation {
            listener: server::listen(port, wake)?,
            waiters: Vec::new(),
            close_requested: false,
            close_written: None,
        })
    }

    /// The port the server listens on.
    pub fn port(&self) -> u16 {
        self.listener.port
    }

    /// Executes the requests that arrived, and answers the waiting
    /// requests whose condition holds or whose time ran out. Call it when
    /// woken, after network progress, and at [`Automation::next_deadline`].
    /// Returns true if a request was executed (the page may need a
    /// repaint).
    pub fn process(&mut self, page: &mut Page) -> bool {
        let mut executed = false;
        while let Ok(Command { request, reply }) = self.listener.commands.try_recv() {
            executed = true;
            log::debug!("automation: {} {}", request.method, request.params);
            match methods::execute(page, &request.method, request.params) {
                Outcome::Done(result) => {
                    // The client may be gone; then nobody needs the answer.
                    let _ = reply.send(Reply::new(Response::new(request.id, result)));
                }
                Outcome::WaitForLoad(deadline) => self.waiters.push(Waiter {
                    id: request.id,
                    reply,
                    deadline,
                }),
                Outcome::Close => {
                    let (written, close_written) = mpsc::channel();
                    let _ = reply.send(Reply {
                        response: Response::new(request.id, Ok(json!({}))),
                        written: Some(written),
                    });
                    self.close_requested = true;
                    self.close_written = Some(close_written);
                }
            }
        }
        self.answer_waiters(page);
        executed
    }

    fn answer_waiters(&mut self, page: &mut Page) {
        if self.waiters.is_empty() {
            return;
        }
        let loaded = page.is_fully_loaded();
        let now = Instant::now();
        self.waiters.retain(|w| {
            if !loaded && now < w.deadline {
                return true;
            }
            let response = Response::new(w.id.clone(), Ok(json!({ "loaded": loaded })));
            let _ = w.reply.send(Reply::new(response));
            false
        });
    }

    /// When the next waiting request times out, if any waits.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.waiters.iter().map(|w| w.deadline).min()
    }

    /// True after a client sent `browser.close`.
    pub fn close_requested(&self) -> bool {
        self.close_requested
    }

    /// After `browser.close`: waits until the reply is written (at most one
    /// second), so that the client gets it before the process exits.
    pub fn finish_close(&mut self) {
        if let Some(written) = self.close_written.take() {
            let _ = written.recv_timeout(CLOSE_REPLY_TIMEOUT);
        }
    }
}

/// The element box dump of a page (format: `docs/testing.md`, "Box
/// dump"): every element in tree order with the union of its border boxes
/// in document coordinates, rounded to 2 decimals, or null if it has no
/// box. `url` is the URL to record. The page must be laid out.
pub fn box_dump(page: &Page, url: &str) -> Value {
    let round = |v: f32| (f64::from(v) * 100.0).round() / 100.0;
    let elements: Vec<Value> = swb_engine::element_boxes(page)
        .into_iter()
        .map(|b| {
            let rect = b
                .rect
                .map(|r| json!([round(r.x), round(r.y), round(r.width), round(r.height)]));
            json!({ "tag": b.tag, "rect": rect, "parent": b.parent })
        })
        .collect();
    let viewport = page.viewport();
    json!({
        "url": url,
        "viewport": [viewport.width, viewport.height],
        "elements": elements,
    })
}
