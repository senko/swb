//! A page without a window, driven only by the automation server.

use std::io;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use swb_engine::{FontContext, Page, PageConfig, Size};
use swb_net::Fetcher;

use crate::Automation;

/// How long the loop sleeps when nothing is pending.
const IDLE_WAIT: Duration = Duration::from_secs(3600);

/// A headless browser: one page and the automation server. The page lives
/// on the thread that calls [`HeadlessBrowser::run`].
pub struct HeadlessBrowser {
    page: Page,
    automation: Automation,
    /// Receives a message for each network completion and each request.
    wake: Receiver<()>,
}

impl HeadlessBrowser {
    /// Creates the page and starts the server on 127.0.0.1:`port` (0
    /// picks a free port). `scripts_allowed` states that the calling
    /// thread, which runs the page, has a stack large enough for scripts
    /// (`PageConfig::scripts_allowed`).
    pub fn new(
        fetcher: Arc<dyn Fetcher>,
        fonts: FontContext,
        viewport: Size,
        scale: f32,
        port: u16,
        scripts_allowed: bool,
    ) -> io::Result<Self> {
        let (sender, wake) = mpsc::channel();
        let notify: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            // The loop may have ended; then nobody needs the wake-up.
            let _ = sender.send(());
        });
        let config = PageConfig {
            fetcher,
            notify: Arc::clone(&notify),
            network_threads: PageConfig::DEFAULT_NETWORK_THREADS,
            scripts_allowed,
        };
        let page = Page::new(config, fonts, viewport, scale);
        let automation = Automation::start(port, notify)?;
        Ok(HeadlessBrowser {
            page,
            automation,
            wake,
        })
    }

    /// The port the server listens on.
    pub fn port(&self) -> u16 {
        self.automation.port()
    }

    /// The page, for example to start the first navigation.
    pub fn page(&mut self) -> &mut Page {
        &mut self.page
    }

    /// Runs the page's event loop (network completions and due tasks) and
    /// the requests until a client sends `browser.close`.
    pub fn run(mut self) {
        loop {
            self.page.pump();
            self.automation.process(&mut self.page);
            if self.automation.close_requested() {
                self.automation.finish_close();
                return;
            }
            // Wake up for the next task of the page, a waiting request's
            // deadline, a network completion or a request.
            let next = [
                self.automation.next_deadline(),
                self.page.next_task_instant(),
            ]
            .into_iter()
            .flatten()
            .min();
            let timeout = next.map_or(IDLE_WAIT, |d| d.saturating_duration_since(Instant::now()));
            if self.wake.recv_timeout(timeout).is_ok() {
                // One pass handles everything that arrived.
                while self.wake.try_recv().is_ok() {}
            }
        }
    }
}
