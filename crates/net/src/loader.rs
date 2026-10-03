//! [`Loader`]: runs fetches on worker threads.
//!
//! Requests go to the workers through a channel. Each worker takes one
//! request at a time, fetches it with [`fetch_following_redirects`], and
//! sends the result back through a second channel. The owner of the
//! [`Loader`] (the engine) polls that channel.
//!
//! [`Loader::cancel_all`] cancels the requests that are queued: each job
//! carries the loader's generation number at the time it was queued, and
//! workers drop jobs from an older generation without fetching them.

use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use log::{error, warn};

use crate::cookies::CookieJar;
use crate::error::NetError;
use crate::fetch::{Fetcher, fetch_following_redirects};
use crate::request::Request;
use crate::response::Response;

/// Identifies a request started with [`Loader::start`]. Ids start at 1 and
/// are unique for one loader.
pub type RequestId = u64;

/// The result of a request started with [`Loader::start`].
#[derive(Debug)]
pub struct Completion {
    /// The id that [`Loader::start`] returned.
    pub id: RequestId,
    /// The final response after redirects, or the error.
    pub result: Result<Response, NetError>,
}

/// A callback that a worker calls after it queues a completion.
type Notify = Arc<dyn Fn() + Send + Sync>;

struct Job {
    id: RequestId,
    /// The value of [`Loader::generation`] when the job was queued.
    generation: u64,
    request: Request,
}

/// Loads resources on a pool of worker threads.
///
/// Dropping the loader stops the workers. Idle workers stop at once. A
/// worker that is in a fetch stops when the fetch ends; the loader does not
/// wait for it. Requests that have not started are dropped.
pub struct Loader {
    /// The fetcher of the workers.
    fetcher: Arc<dyn Fetcher>,
    /// `None` only during drop.
    jobs: Option<Sender<Job>>,
    completions: Receiver<Completion>,
    /// Used when no worker can take a request.
    completion_sender: Sender<Completion>,
    notify: Notify,
    next_id: AtomicU64,
    /// Incremented by [`Loader::cancel_all`]. Shared with the workers.
    generation: Arc<AtomicU64>,
    shutdown: Arc<AtomicBool>,
}

impl Loader {
    /// Starts `workers` threads (at least one) that fetch with `fetcher`.
    ///
    /// `notify` is called from a worker thread after each completion is
    /// queued. The GUI uses it to wake its event loop.
    pub fn new(
        fetcher: Arc<dyn Fetcher>,
        workers: usize,
        notify: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let (job_sender, job_receiver) = mpsc::channel::<Job>();
        let (completion_sender, completions) = mpsc::channel();
        let job_receiver = Arc::new(Mutex::new(job_receiver));
        let shutdown = Arc::new(AtomicBool::new(false));
        let generation = Arc::new(AtomicU64::new(0));
        for index in 0..workers.max(1) {
            let worker = Worker {
                fetcher: Arc::clone(&fetcher),
                jobs: Arc::clone(&job_receiver),
                completions: completion_sender.clone(),
                notify: Arc::clone(&notify),
                generation: Arc::clone(&generation),
                shutdown: Arc::clone(&shutdown),
            };
            let spawned = thread::Builder::new()
                .name(format!("swb-net-{index}"))
                .spawn(move || worker.run());
            if let Err(error) = spawned {
                error!("cannot start network worker thread: {error}");
            }
        }
        Loader {
            fetcher,
            jobs: Some(job_sender),
            completions,
            completion_sender,
            notify,
            next_id: AtomicU64::new(1),
            generation,
            shutdown,
        }
    }

    /// Queues `request` and returns its id. A worker fetches it and follows
    /// redirects. The result arrives as a [`Completion`] with this id.
    pub fn start(&self, request: Request) -> RequestId {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let job = Job {
            id,
            generation: self.generation.load(Ordering::Relaxed),
            request,
        };
        let sent = self
            .jobs
            .as_ref()
            .is_some_and(|jobs| jobs.send(job).is_ok());
        if !sent {
            // All workers have stopped (or none could start).
            warn!("no network worker thread is running");
            let completion = Completion {
                id,
                result: Err(NetError::Internal(
                    "no network worker thread is running".to_string(),
                )),
            };
            if self.completion_sender.send(completion).is_ok() {
                (self.notify)();
            }
        }
        id
    }

    /// Cancels all requests started so far. Requests that no worker has
    /// started yet are dropped without a completion. Requests that are in
    /// progress run to the end and their completions still arrive; the
    /// caller must ignore their ids. Requests started after this call are
    /// not affected.
    pub fn cancel_all(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns the next completion if one is ready. Does not block.
    pub fn try_recv(&self) -> Option<Completion> {
        self.completions.try_recv().ok()
    }

    /// Waits up to `timeout` for the next completion.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<Completion> {
        self.completions.recv_timeout(timeout).ok()
    }

    /// Returns the cookie jar of the fetcher, if it has one.
    pub fn cookie_jar(&self) -> Option<&CookieJar> {
        self.fetcher.cookie_jar()
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        // Workers that wait for a job get an error from `recv` and stop.
        self.jobs = None;
    }
}

struct Worker {
    fetcher: Arc<dyn Fetcher>,
    jobs: Arc<Mutex<Receiver<Job>>>,
    completions: Sender<Completion>,
    notify: Notify,
    generation: Arc<AtomicU64>,
    shutdown: Arc<AtomicBool>,
}

impl Worker {
    fn run(self) {
        loop {
            let job = self
                .jobs
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .recv();
            let Ok(Job {
                id,
                generation,
                request,
            }) = job
            else {
                return;
            };
            if self.shutdown.load(Ordering::Relaxed) {
                return;
            }
            if generation != self.generation.load(Ordering::Relaxed) {
                // Cancelled by `Loader::cancel_all` before it started.
                continue;
            }
            // A panic in a fetcher is a bug, but it must not leave the
            // request without a completion or kill the worker.
            let result = panic::catch_unwind(AssertUnwindSafe(|| {
                fetch_following_redirects(self.fetcher.as_ref(), request)
            }))
            .unwrap_or_else(|_| Err(NetError::Internal("fetcher panicked".to_string())));
            if self.completions.send(Completion { id, result }).is_err() {
                // The loader is gone.
                return;
            }
            (self.notify)();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::time::Instant;

    use url::Url;

    use super::*;
    use crate::headers::Headers;
    use crate::request::Destination;

    /// Answers every request with status 200 and the URL path as the body.
    /// `/panic` panics, `/block` waits until `release` gets a message.
    struct EchoFetcher {
        release: Mutex<Receiver<()>>,
    }

    impl Fetcher for EchoFetcher {
        fn fetch(&self, request: &Request) -> Result<Response, NetError> {
            match request.url.path() {
                "/panic" => panic!("test panic"),
                "/block" => {
                    let _ = self.release.lock().unwrap().recv();
                }
                _ => {}
            }
            Ok(Response {
                url: request.url.clone(),
                status: 200,
                headers: Headers::new(),
                body: request.url.path().as_bytes().to_vec(),
                redirected: false,
            })
        }
    }

    /// Returns a fetcher and the sender that releases `/block` requests.
    fn echo() -> (Arc<EchoFetcher>, Sender<()>) {
        let (sender, receiver) = mpsc::channel();
        let fetcher = EchoFetcher {
            release: Mutex::new(receiver),
        };
        (Arc::new(fetcher), sender)
    }

    fn request(path: &str) -> Request {
        Request::get(
            Url::parse("https://a.test/").unwrap().join(path).unwrap(),
            Destination::Other,
        )
    }

    fn no_notify() -> Notify {
        Arc::new(|| {})
    }

    #[test]
    fn several_requests_complete() {
        let (fetcher, _release) = echo();
        let (notified, notifications) = mpsc::channel();
        let notify: Notify = Arc::new(move || {
            let _ = notified.send(());
        });
        let loader = Loader::new(fetcher, 3, notify);
        let mut expected = HashSet::new();
        for i in 0..10 {
            let id = loader.start(request(&format!("/{i}")));
            expected.insert((id, format!("/{i}")));
        }
        let mut seen = HashSet::new();
        for _ in 0..10 {
            let completion = loader.recv_timeout(Duration::from_secs(10)).unwrap();
            let body = String::from_utf8(completion.result.unwrap().body).unwrap();
            seen.insert((completion.id, body));
        }
        assert_eq!(seen, expected);
        let ids: HashSet<RequestId> = seen.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, (1..=10).collect());
        // One notification per completion.
        for _ in 0..10 {
            notifications.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        assert!(loader.try_recv().is_none());
    }

    #[test]
    fn try_recv_and_recv_timeout_without_completions() {
        let (fetcher, release) = echo();
        let loader = Loader::new(fetcher, 1, no_notify());
        assert!(loader.try_recv().is_none());
        let id = loader.start(request("/block"));
        let start = Instant::now();
        assert!(loader.recv_timeout(Duration::from_millis(50)).is_none());
        assert!(start.elapsed() >= Duration::from_millis(50));
        assert!(loader.try_recv().is_none());
        release.send(()).unwrap();
        let completion = loader.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(completion.id, id);
    }

    #[test]
    fn try_recv_returns_ready_completion() {
        let (fetcher, _release) = echo();
        let loader = Loader::new(fetcher, 1, no_notify());
        let id = loader.start(request("/x"));
        let deadline = Instant::now() + Duration::from_secs(10);
        let completion = loop {
            if let Some(completion) = loader.try_recv() {
                break completion;
            }
            assert!(Instant::now() < deadline, "no completion");
            thread::yield_now();
        };
        assert_eq!(completion.id, id);
    }

    #[test]
    fn panic_in_fetcher_becomes_an_error() {
        let (fetcher, _release) = echo();
        let loader = Loader::new(fetcher, 1, no_notify());
        let id = loader.start(request("/panic"));
        let completion = loader.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(completion.id, id);
        assert!(matches!(completion.result, Err(NetError::Internal(_))));
        // The worker still runs.
        loader.start(request("/after"));
        let completion = loader.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(completion.result.unwrap().body, b"/after");
    }

    #[test]
    fn drop_does_not_wait_for_running_fetches() {
        let (fetcher, release) = echo();
        let loader = Loader::new(fetcher.clone(), 2, no_notify());
        loader.start(request("/block"));
        loader.start(request("/block"));
        loader.start(request("/queued"));
        let start = Instant::now();
        drop(loader);
        assert!(start.elapsed() < Duration::from_secs(1));
        // Release the blocked fetches; the workers then stop.
        release.send(()).unwrap();
        release.send(()).unwrap();
        // The workers hold the last references to the fetcher besides this
        // one; wait until they have stopped.
        let deadline = Instant::now() + Duration::from_secs(10);
        while Arc::strong_count(&fetcher) > 1 {
            assert!(Instant::now() < deadline, "workers did not stop");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn cancel_all_drops_queued_requests() {
        let (fetcher, release) = echo();
        let loader = Loader::new(fetcher, 1, no_notify());
        // The only worker is either in this fetch or about to start it.
        loader.start(request("/block"));
        let queued = [loader.start(request("/q1")), loader.start(request("/q2"))];
        loader.cancel_all();
        let after = loader.start(request("/after"));
        release.send(()).unwrap();
        // Requests started after the cancellation run; queued requests
        // never complete.
        let mut seen = Vec::new();
        while !seen.contains(&after) {
            let completion = loader.recv_timeout(Duration::from_secs(10)).unwrap();
            seen.push(completion.id);
        }
        assert!(loader.recv_timeout(Duration::from_millis(50)).is_none());
        assert!(seen.iter().all(|id| !queued.contains(id)), "{seen:?}");
    }

    #[test]
    fn zero_workers_means_one() {
        let (fetcher, _release) = echo();
        let loader = Loader::new(fetcher, 0, no_notify());
        loader.start(request("/x"));
        assert!(loader.recv_timeout(Duration::from_secs(10)).is_some());
    }
}
