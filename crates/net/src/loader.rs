//! [`Loader`]: runs fetches on worker threads.
//!
//! Requests go to the workers through a channel. Each worker takes one
//! request at a time, fetches it with [`fetch_request`], and sends the
//! result back through a second channel. The owner of the [`Loader`] (the
//! engine) polls that channel.
//!
//! There are two pools of workers with a queue each. [`Loader::start`] runs
//! the page's own loads (documents, images, scripts, style sheets, fonts)
//! on the first pool. [`Loader::start_api`] runs the requests of a script's
//! API calls (`fetch()`) on the second, which has [`API_WORKERS`] threads
//! and starts when the first such request arrives: a server that never
//! answers can hold the threads of the second pool for the length of the
//! request timeout, and the page's loads must not wait for them (M8 feature
//! 10a).
//!
//! [`Loader::cancel_all`] cancels the requests that are queued: each job
//! carries the loader's generation number at the time it was queued, and
//! workers drop jobs from an older generation without fetching them.
//!
//! The loader keeps one entry per request that it started and whose
//! worker has not finished it (`JobState`); the entry is removed when the
//! worker takes the job and drops it, or sends the completion, so the table
//! is as large as the number of requests in progress.

use std::collections::HashMap;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use log::{error, warn};

use crate::budget::Reservation;
use crate::cookies::CookieJar;
use crate::cors::{PreflightCache, ResponseKind};
use crate::cors_fetch::fetch_request;
use crate::error::NetError;
use crate::fetch::Fetcher;
use crate::request::Request;
use crate::response::Response;

/// Identifies a request started with [`Loader::start`]. Ids start at 1 and
/// are unique for one loader.
pub type RequestId = u64;

/// The number of worker threads for the requests of [`Loader::start_api`].
pub const API_WORKERS: usize = 8;

/// The most threads that run a cancelled request to its end while a new
/// thread takes their place ([`Loader::cancel`]). Each ends when its
/// request ends, after the request timeout at the latest.
const MAX_DETACHED_WORKERS: usize = 16;

/// The result of a request started with [`Loader::start`].
#[derive(Debug)]
pub struct Completion {
    /// The id that [`Loader::start`] returned.
    pub id: RequestId,
    /// The final response after redirects, or the error. For a request with
    /// [`Request::exposed_to_script`] it is the filtered response of
    /// `kind`.
    pub result: Result<Response, NetError>,
    /// The kind of the response (`Response.type` of `fetch()`). `Basic`
    /// for an error.
    pub kind: ResponseKind,
    /// The memory that the response body of a request with a
    /// [`Request::reservation`] holds; the receiver drops it when the body
    /// has moved elsewhere. `None` for an error.
    pub reservation: Option<Arc<Reservation>>,
}

impl Completion {
    fn failed(id: RequestId, error: NetError) -> Self {
        Completion {
            id,
            result: Err(error),
            kind: ResponseKind::Basic,
            reservation: None,
        }
    }
}

/// A callback that a worker calls after it queues a completion.
type Notify = Arc<dyn Fn() + Send + Sync>;

/// The pool that runs a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PoolKind {
    Page,
    Api,
}

struct Job {
    id: RequestId,
    /// The value of [`Shared::generation`] when the job was queued.
    generation: u64,
    request: Request,
}

/// Where a started request is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JobState {
    /// In the queue of a pool.
    Queued,
    /// In the queue; [`Loader::cancel`] was called.
    Cancelled,
    /// A worker of the pool fetches it.
    Running(PoolKind),
    /// A worker fetches it; [`Loader::cancel`] was called and another
    /// thread took the worker's place. The worker ends after this request.
    Detached,
}

/// What the loader and its workers share.
struct Shared {
    /// The fetcher of the workers.
    fetcher: Arc<dyn Fetcher>,
    completions: Sender<Completion>,
    notify: Notify,
    /// Incremented by [`Loader::cancel_all`].
    generation: AtomicU64,
    shutdown: AtomicBool,
    states: Mutex<HashMap<RequestId, JobState>>,
    preflights: PreflightCache,
    /// The workers in state [`JobState::Detached`].
    detached: AtomicUsize,
}

impl Shared {
    fn states(&self) -> MutexGuard<'_, HashMap<RequestId, JobState>> {
        self.states.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Removes the entry of `id`; true if its worker was detached.
    fn finish(&self, id: RequestId) -> bool {
        self.states().remove(&id) == Some(JobState::Detached)
    }

    /// Sends a completion to the loader's owner.
    fn complete(&self, completion: Completion) -> bool {
        if self.completions.send(completion).is_err() {
            return false;
        }
        (self.notify)();
        true
    }
}

/// The queue of one pool.
struct Pool {
    kind: PoolKind,
    /// `None` only during drop.
    jobs: Option<Sender<Job>>,
    receiver: Arc<Mutex<Receiver<Job>>>,
    /// The threads that started. The pool keeps the receiver, so a send
    /// succeeds without workers; a pool where no thread could start fails
    /// its requests instead (workers stop only at shutdown or after a
    /// replacement took their place).
    started: AtomicUsize,
}

impl Pool {
    fn new(kind: PoolKind) -> Self {
        let (sender, receiver) = mpsc::channel();
        Pool {
            kind,
            jobs: Some(sender),
            receiver: Arc::new(Mutex::new(receiver)),
            started: AtomicUsize::new(0),
        }
    }

    /// Starts one worker thread.
    fn spawn_worker(&self, shared: &Arc<Shared>, index: usize) -> bool {
        let worker = Worker {
            shared: Arc::clone(shared),
            jobs: Arc::clone(&self.receiver),
            kind: self.kind,
        };
        let name = match self.kind {
            PoolKind::Page => format!("swb-net-{index}"),
            PoolKind::Api => format!("swb-net-api-{index}"),
        };
        match thread::Builder::new()
            .name(name)
            .spawn(move || worker.run())
        {
            Ok(_) => {
                self.started.fetch_add(1, Ordering::Relaxed);
                true
            }
            Err(error) => {
                error!("cannot start network worker thread: {error}");
                false
            }
        }
    }
}

/// Loads resources on a pool of worker threads.
///
/// Dropping the loader stops the workers. Idle workers stop at once. A
/// worker that is in a fetch stops when the fetch ends; the loader does not
/// wait for it. Requests that have not started are dropped.
pub struct Loader {
    shared: Arc<Shared>,
    page: Pool,
    api: Pool,
    api_started: AtomicBool,
    completions: Receiver<Completion>,
    next_id: AtomicU64,
    /// The numbers of the threads that were started after the first ones.
    spawned: AtomicUsize,
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
        let (completion_sender, completions) = mpsc::channel();
        let shared = Arc::new(Shared {
            fetcher,
            completions: completion_sender,
            notify,
            generation: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
            states: Mutex::new(HashMap::new()),
            preflights: PreflightCache::new(),
            detached: AtomicUsize::new(0),
        });
        let page = Pool::new(PoolKind::Page);
        for index in 0..workers.max(1) {
            page.spawn_worker(&shared, index);
        }
        Loader {
            shared,
            page,
            api: Pool::new(PoolKind::Api),
            api_started: AtomicBool::new(false),
            completions,
            next_id: AtomicU64::new(1),
            spawned: AtomicUsize::new(0),
        }
    }

    /// Queues `request` and returns its id. A worker fetches it and follows
    /// redirects. The result arrives as a [`Completion`] with this id.
    pub fn start(&self, request: Request) -> RequestId {
        self.enqueue(&self.page, request)
    }

    /// Like [`Loader::start`], on the pool of the script's API calls
    /// ([`API_WORKERS`] threads, apart from the page's loads).
    pub fn start_api(&self, request: Request) -> RequestId {
        if !self.api_started.swap(true, Ordering::Relaxed) {
            for index in 0..API_WORKERS {
                self.api.spawn_worker(&self.shared, index);
            }
        }
        self.enqueue(&self.api, request)
    }

    fn enqueue(&self, pool: &Pool, request: Request) -> RequestId {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let job = Job {
            id,
            generation: self.shared.generation.load(Ordering::Relaxed),
            request,
        };
        self.shared.states().insert(id, JobState::Queued);
        let sent = pool.started.load(Ordering::Relaxed) > 0
            && pool
                .jobs
                .as_ref()
                .is_some_and(|jobs| jobs.send(job).is_ok());
        if !sent {
            // No worker could start (or the loader is being dropped).
            warn!("no network worker thread is running");
            self.shared.finish(id);
            self.shared.complete(Completion::failed(
                id,
                NetError::Internal("no network worker thread is running".to_string()),
            ));
        }
        id
    }

    /// Cancels all requests started so far. Requests that no worker has
    /// started yet are dropped without a completion. Requests that are in
    /// progress run to the end and their completions still arrive; the
    /// caller must ignore their ids. Requests started after this call are
    /// not affected.
    pub fn cancel_all(&self) {
        self.shared.generation.fetch_add(1, Ordering::Relaxed);
    }

    /// Cancels the request `id`. A request that no worker has started
    /// completes with [`NetError::Aborted`] without a fetch. One that is in
    /// progress runs to the end (the worker cannot interrupt the blocking
    /// read) and its completion arrives as usual: the caller ignores it.
    ///
    /// Returns true if the request is in progress and another thread took
    /// its worker's place, so that the caller may count the request as gone
    /// (at most [`MAX_DETACHED_WORKERS`] such threads exist at a time).
    /// Cancelling a request that has finished does nothing. The table of
    /// requests holds at most one entry per request that a worker has not
    /// finished.
    pub fn cancel(&self, id: RequestId) -> bool {
        let pool = {
            let mut states = self.shared.states();
            match states.get(&id).copied() {
                Some(JobState::Queued) => {
                    states.insert(id, JobState::Cancelled);
                    return false;
                }
                Some(JobState::Running(kind)) => {
                    if self.shared.detached.load(Ordering::Relaxed) >= MAX_DETACHED_WORKERS {
                        return false;
                    }
                    states.insert(id, JobState::Detached);
                    self.shared.detached.fetch_add(1, Ordering::Relaxed);
                    kind
                }
                _ => return false,
            }
        };
        let pool = match pool {
            PoolKind::Page => &self.page,
            PoolKind::Api => &self.api,
        };
        let index = self.spawned.fetch_add(1, Ordering::Relaxed);
        if pool.spawn_worker(&self.shared, 1000 + index) {
            return true;
        }
        // No thread for the replacement: the worker stays.
        let mut states = self.shared.states();
        if states.get(&id) == Some(&JobState::Detached) {
            states.insert(id, JobState::Running(pool.kind));
            self.shared.detached.fetch_sub(1, Ordering::Relaxed);
        }
        false
    }

    /// The number of requests that the loader keeps an entry for (started
    /// and not finished by a worker).
    pub fn tracked_requests(&self) -> usize {
        self.shared.states().len()
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
        self.shared.fetcher.cookie_jar()
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, Ordering::Relaxed);
        // Workers that wait for a job get an error from `recv` and stop.
        self.page.jobs = None;
        self.api.jobs = None;
    }
}

struct Worker {
    shared: Arc<Shared>,
    jobs: Arc<Mutex<Receiver<Job>>>,
    kind: PoolKind,
}

impl Worker {
    /// Marks the start of the job `id`; false if it was cancelled before.
    fn begin(&self, id: RequestId) -> bool {
        let mut states = self.shared.states();
        if states.get(&id) == Some(&JobState::Cancelled) {
            return false;
        }
        states.insert(id, JobState::Running(self.kind));
        true
    }

    /// Fetches `request`; the response keeps its [`Reservation`].
    fn fetch(&self, id: RequestId, request: Request) -> Completion {
        let reservation = request.reservation.clone();
        // A panic in a fetcher is a bug, but it must not leave the request
        // without a completion or kill the worker.
        let fetched = panic::catch_unwind(AssertUnwindSafe(|| {
            fetch_request(
                self.shared.fetcher.as_ref(),
                request,
                &self.shared.preflights,
            )
        }))
        .unwrap_or_else(|_| Err(NetError::Internal("fetcher panicked".to_string())));
        let fetched = fetched.and_then(|fetched| {
            // The request is gone; the response is what the request holds
            // now (a fetcher that does not count while it reads, like the
            // replay, is checked here).
            if let Some(reservation) = &reservation {
                reservation.release_request();
                if !reservation.set_response(fetched.response.body.len()) {
                    return Err(NetError::OverBudget {
                        limit: reservation.limit(),
                    });
                }
            }
            Ok(fetched)
        });
        match fetched {
            Ok(fetched) => Completion {
                id,
                result: Ok(fetched.response),
                kind: fetched.kind,
                reservation,
            },
            Err(error) => Completion::failed(id, error),
        }
    }

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
            if self.shared.shutdown.load(Ordering::Relaxed) {
                return;
            }
            if generation != self.shared.generation.load(Ordering::Relaxed) {
                // Cancelled by `Loader::cancel_all` before it started.
                self.shared.finish(id);
                continue;
            }
            let completion = if self.begin(id) {
                self.fetch(id, request)
            } else {
                // Cancelled by `Loader::cancel` before it started.
                Completion::failed(id, NetError::Aborted)
            };
            let detached = self.shared.finish(id);
            if !self.shared.complete(completion) {
                // The loader is gone.
                return;
            }
            if detached {
                // `Loader::cancel` started another thread for this one.
                self.shared.detached.fetch_sub(1, Ordering::Relaxed);
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
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
    fn cancel_aborts_a_queued_request_with_a_completion() {
        let (fetcher, release) = echo();
        let loader = Loader::new(fetcher, 1, no_notify());
        let blocked = loader.start(request("/block"));
        let queued = loader.start(request("/q1"));
        let kept = loader.start(request("/q2"));
        loader.cancel(queued);
        release.send(()).unwrap();
        let mut results = HashMap::new();
        while results.len() < 3 {
            let completion = loader.recv_timeout(Duration::from_secs(10)).unwrap();
            results.insert(completion.id, completion.result);
        }
        assert!(results[&blocked].is_ok());
        assert!(matches!(results[&queued], Err(NetError::Aborted)));
        assert!(results[&kept].is_ok());
    }

    /// Waits until `condition` holds.
    fn wait_for(what: &str, condition: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !condition() {
            assert!(Instant::now() < deadline, "timeout: {what}");
            thread::sleep(Duration::from_millis(2));
        }
    }

    fn running(loader: &Loader, id: RequestId) -> bool {
        matches!(
            loader.shared.states().get(&id),
            Some(JobState::Running(_) | JobState::Detached)
        )
    }

    #[test]
    fn cancel_after_the_completion_keeps_no_entry() {
        // Review fix 5: the worker had passed the point where it looked at
        // the cancelled set, so the id stayed in it.
        let (fetcher, _release) = echo();
        let loader = Loader::new(fetcher, 1, no_notify());
        let id = loader.start(request("/x"));
        let completion = loader.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(completion.id, id);
        assert!(!loader.cancel(id));
        assert_eq!(loader.tracked_requests(), 0);
    }

    #[test]
    fn cancel_after_cancel_all_keeps_no_entry() {
        // Review fix 5: `abandon_api_fetches` cancels the ids of jobs that
        // `cancel_all` dropped already or will drop.
        let (fetcher, release) = echo();
        let loader = Loader::new(fetcher, 1, no_notify());
        let blocked = loader.start(request("/block"));
        wait_for("running", || running(&loader, blocked));
        let queued: Vec<RequestId> = (0..5)
            .map(|i| loader.start(request(&format!("/q{i}"))))
            .collect();
        loader.cancel_all();
        for id in &queued {
            loader.cancel(*id);
        }
        release.send(()).unwrap();
        wait_for("the dropped jobs", || loader.tracked_requests() == 0);
        // Their ids are gone, so a late cancel adds nothing.
        for id in &queued {
            loader.cancel(*id);
        }
        assert_eq!(loader.tracked_requests(), 0);
    }

    #[test]
    fn a_cancelled_running_request_gets_a_new_thread() {
        let (fetcher, release) = echo();
        let loader = Loader::new(fetcher, 1, no_notify());
        let hung = loader.start_api(request("/block"));
        wait_for("running", || running(&loader, hung));
        assert!(loader.cancel(hung));
        // The only worker of the pool is blocked, a new one answers.
        let after = loader.start_api(request("/after"));
        let completion = loader.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(completion.id, after);
        release.send(()).unwrap();
        let completion = loader.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(completion.id, hung);
        wait_for("the detached worker ended", || {
            loader.shared.detached.load(Ordering::Relaxed) == 0
        });
        assert_eq!(loader.tracked_requests(), 0);
    }

    #[test]
    fn hung_api_requests_do_not_hold_the_page_loads() {
        // Review fix 3: six requests that never answer took the six
        // workers of the page.
        let (fetcher, release) = echo();
        let loader = Loader::new(fetcher, 2, no_notify());
        for _ in 0..API_WORKERS {
            loader.start_api(request("/block"));
        }
        let page = loader.start(request("/page"));
        let completion = loader.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(completion.id, page);
        for _ in 0..API_WORKERS {
            release.send(()).unwrap();
        }
    }

    #[test]
    fn hung_page_loads_do_not_hold_the_api_requests() {
        let (fetcher, release) = echo();
        let loader = Loader::new(fetcher, 1, no_notify());
        loader.start(request("/block"));
        let api = loader.start_api(request("/api"));
        let completion = loader.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(completion.id, api);
        release.send(()).unwrap();
    }

    #[test]
    fn a_response_over_the_budget_fails_and_the_bytes_return() {
        use crate::budget::ByteBudget;
        let (fetcher, _release) = echo();
        let loader = Loader::new(fetcher, 1, no_notify());
        let budget = ByteBudget::new(10);
        let mut fits = request("/abcdefgh");
        fits.reservation = Reservation::new(&budget, 5);
        assert!(fits.reservation.is_some());
        let id = loader.start_api(fits);
        let completion = loader.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(completion.id, id);
        assert!(completion.result.is_ok());
        // The request's bytes went back, the response's stay.
        assert_eq!(budget.used(), 9);
        drop(completion);
        assert_eq!(budget.used(), 0);
        let mut over = request("/abcdefghijk");
        over.reservation = Reservation::new(&budget, 5);
        loader.start_api(over);
        let completion = loader.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(matches!(
            completion.result,
            Err(NetError::OverBudget { limit: 10 })
        ));
        drop(completion);
        wait_for("the bytes went back", || budget.used() == 0);
    }

    #[test]
    fn zero_workers_means_one() {
        let (fetcher, _release) = echo();
        let loader = Loader::new(fetcher, 0, no_notify());
        loader.start(request("/x"));
        assert!(loader.recv_timeout(Duration::from_secs(10)).is_some());
    }

    #[test]
    fn a_pool_without_threads_fails_its_requests() {
        // As when no thread could start: the request does not wait for ever.
        let (fetcher, _release) = echo();
        let loader = Loader::new(fetcher, 1, no_notify());
        let empty = Pool::new(PoolKind::Api);
        let id = loader.enqueue(&empty, request("/x"));
        let completion = loader.try_recv().expect("the failure is queued at once");
        assert_eq!(completion.id, id);
        assert!(matches!(completion.result, Err(NetError::Internal(_))));
        assert_eq!(loader.tracked_requests(), 0);
    }
}
