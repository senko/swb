//! The requests of a script's `fetch()` calls (M8 feature 10a, ADR 0028
//! section 12).
//!
//! A call comes from the script runtime
//! ([`swb_web::EngineHost::start_api_fetch`]) with a request that has no
//! initiator yet; the document gives it its origin and a [`Reservation`]
//! of the document's [`ByteBudget`], and the loader runs it (CORS,
//! credentials and redirects are in `net`).
//!
//! - **Memory.** The request (URL, headers, body) and the response body
//!   that is read but not yet in the script heap count against one budget
//!   of [`API_BUDGET`] bytes (ADR 0028 section 16). A request that does not
//!   fit fails at once; a response that grows past it fails while it is
//!   read. The bytes return when the response reaches the heap or is
//!   dropped (abort, navigation, stop).
//! - **Concurrency.** The requests run on their own pool of workers
//!   ([`swb_net::Loader::start_api`]), so the page's loads never wait for
//!   them. At most [`MAX_API_FETCHES`] run at once and at most
//!   [`MAX_API_PER_HOST`] to one host (Chromium: six connections per host),
//!   so a server that never answers does not take all places. The others
//!   wait in a FIFO queue of at most [`MAX_QUEUED_API_FETCHES`] requests;
//!   the first [`QUEUE_SCAN`] of them are looked at for a request to a host
//!   with a free place.
//! - **Abort.** A cancelled call that waits is dropped from the queue. One
//!   that runs cannot be interrupted in its blocking read; the loader puts
//!   another thread in its place, the call gives up its place for the next
//!   request, and the bytes that the old thread still reads stay counted
//!   until it lets go. If the loader cannot replace the thread, the call
//!   keeps its place until the completion arrives.
//!
//! The results wait in [`ApiFetches::arrived`] for the page, which hands
//! them to the script runtime as tasks. These requests do not delay the
//! `load` event: they are not in [`crate::resources::Requests`].

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use swb_net::{
    API_WORKERS, ByteBudget, NetError, Request, RequestId, Reservation, Response, ResponseKind,
};

use super::DocumentState;

/// The most `fetch()` requests of a document that run at once (the size
/// of the loader's pool for them).
pub(crate) const MAX_API_FETCHES: usize = API_WORKERS;

/// The most requests to one host that run at once.
pub(crate) const MAX_API_PER_HOST: usize = 6;

/// The most requests of a document that wait for a place.
pub(crate) const MAX_QUEUED_API_FETCHES: usize = 16_384;

/// The bytes outside the script heap that the requests and the responses
/// of a document's `fetch()` calls may hold (queued, running, and read but
/// not delivered).
pub(crate) const API_BUDGET: usize = 256 << 20;

/// How many of the waiting requests are looked at for one with a free
/// place at its host.
const QUEUE_SCAN: usize = 256;

/// The result of a call: the id, the response or the error, the type of
/// the response, and the memory that the response holds until it is in the
/// script heap.
pub(crate) type ApiResult = (
    u64,
    Result<Response, NetError>,
    ResponseKind,
    Option<Arc<Reservation>>,
);

/// A request in the loader.
#[derive(Debug)]
struct Running {
    /// The call, `None` once it was cancelled.
    call: Option<u64>,
    host: String,
    /// True while the request counts against the limits.
    has_place: bool,
}

/// A request that waits.
#[derive(Debug)]
struct Queued {
    request: Request,
    host: String,
}

/// The `fetch()` calls of a document.
#[derive(Debug)]
pub(crate) struct ApiFetches {
    /// The id of the last call.
    last_id: u64,
    budget: Arc<ByteBudget>,
    /// The requests in the loader.
    running: HashMap<RequestId, Running>,
    /// The number of requests with a place, and per host.
    places: usize,
    places_by_host: HashMap<String, usize>,
    /// The requests that wait, in order of their ids.
    queued: BTreeMap<u64, Queued>,
    /// The results that wait for the script runtime.
    arrived: Vec<ApiResult>,
}

impl Default for ApiFetches {
    fn default() -> Self {
        ApiFetches {
            last_id: 0,
            budget: ByteBudget::new(API_BUDGET),
            running: HashMap::new(),
            places: 0,
            places_by_host: HashMap::new(),
            queued: BTreeMap::new(),
            arrived: Vec::new(),
        }
    }
}

impl ApiFetches {
    /// True if a call waits for its result (running or queued, not
    /// cancelled).
    pub(crate) fn is_pending(&self) -> bool {
        !self.queued.is_empty() || self.running.values().any(|r| r.call.is_some())
    }

    /// The number of requests with a place (running and not given up).
    #[cfg(test)]
    pub(crate) fn places(&self) -> usize {
        self.places
    }

    /// The number of requests that wait.
    #[cfg(test)]
    pub(crate) fn queued(&self) -> usize {
        self.queued.len()
    }

    /// The bytes that the calls hold now.
    #[cfg(test)]
    pub(crate) fn used_bytes(&self) -> usize {
        self.budget.used()
    }

    fn host_places(&self, host: &str) -> usize {
        self.places_by_host.get(host).copied().unwrap_or(0)
    }

    fn can_run(&self, host: &str) -> bool {
        self.places < MAX_API_FETCHES && self.host_places(host) < MAX_API_PER_HOST
    }

    fn take_place(&mut self, host: &str) {
        self.places += 1;
        *self.places_by_host.entry(host.to_owned()).or_insert(0) += 1;
    }

    fn give_up_place(&mut self, host: &str) {
        self.places = self.places.saturating_sub(1);
        if let Some(count) = self.places_by_host.get_mut(host) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.places_by_host.remove(host);
            }
        }
    }
}

/// The bytes that a request holds: URL, headers and body.
fn request_bytes(request: &Request) -> usize {
    request.url.as_str().len()
        + request.body.as_ref().map_or(0, Vec::len)
        + request
            .headers
            .iter()
            .map(|(n, v)| n.len() + v.len() + 32)
            .sum::<usize>()
        + 256
}

/// The host and port of the request, the key of the per-host limit.
fn host_key(request: &Request) -> String {
    format!(
        "{}:{}",
        request.url.host_str().unwrap_or_default(),
        request.url.port_or_known_default().unwrap_or_default()
    )
}

impl DocumentState {
    /// Starts the call of `request` or queues it; its id, or `None` if the
    /// document may not load the URL, the queue is full or the request does
    /// not fit in the memory budget.
    pub(crate) fn start_api_fetch(&mut self, mut request: Request) -> Option<u64> {
        if !self.may_load(&request.url) {
            log::warn!("not allowed to load {} from a script", request.url);
            return None;
        }
        request.initiator = Some(self.origin());
        let bytes = request_bytes(&request);
        let fetches = &mut self.api_fetches;
        let Some(reservation) = Reservation::new(&fetches.budget, bytes) else {
            log::warn!(
                "{}: the fetch() requests hold {} of {} bytes, a request of {bytes} bytes does not fit",
                request.url,
                fetches.budget.used(),
                fetches.budget.limit()
            );
            return None;
        };
        request.reservation = Some(reservation);
        let host = host_key(&request);
        if fetches.queued.is_empty() && fetches.can_run(&host) {
            fetches.last_id += 1;
            let id = fetches.last_id;
            self.run_api_fetch(id, request, host);
            return Some(id);
        }
        if fetches.queued.len() >= MAX_QUEUED_API_FETCHES {
            log::warn!(
                "{}: too many fetch() requests wait ({} requests)",
                request.url,
                fetches.queued.len()
            );
            return None;
        }
        fetches.last_id += 1;
        let id = fetches.last_id;
        fetches.queued.insert(id, Queued { request, host });
        // A request to a host with a free place may pass the ones before it.
        self.start_queued_api_fetches();
        Some(id)
    }

    fn run_api_fetch(&mut self, id: u64, request: Request, host: String) {
        #[cfg(test)]
        self.load_log.push(format!("fetch {}", request.url));
        log::debug!("fetch() {} {}", request.method, request.url);
        let loader_id = self.loader.start_api(request);
        let fetches = &mut self.api_fetches;
        fetches.take_place(&host);
        fetches.running.insert(
            loader_id,
            Running {
                call: Some(id),
                host,
                has_place: true,
            },
        );
    }

    /// Starts waiting requests while places are free.
    fn start_queued_api_fetches(&mut self) {
        loop {
            let fetches = &self.api_fetches;
            if fetches.places >= MAX_API_FETCHES {
                return;
            }
            let next = fetches
                .queued
                .iter()
                .take(QUEUE_SCAN)
                .find(|(_, queued)| fetches.can_run(&queued.host))
                .map(|(id, _)| *id);
            let Some(id) = next else {
                return;
            };
            if let Some(Queued { request, host }) = self.api_fetches.queued.remove(&id) {
                self.run_api_fetch(id, request, host);
            }
        }
    }

    /// Drops the call `id`.
    pub(crate) fn cancel_api_fetch(&mut self, id: u64) {
        let fetches = &mut self.api_fetches;
        if fetches.queued.remove(&id).is_some() {
            // The request and its reservation are gone.
            return;
        }
        let running = fetches
            .running
            .iter_mut()
            .find(|(_, running)| running.call == Some(id));
        let Some((loader_id, running)) = running else {
            return;
        };
        running.call = None;
        let loader_id = *loader_id;
        // A request that is in a worker's blocking read gives up its place
        // if the loader puts another thread in the worker's place.
        if self.loader.cancel(loader_id) {
            let fetches = &mut self.api_fetches;
            let host = fetches.running.get_mut(&loader_id).and_then(|running| {
                let had_place = std::mem::take(&mut running.has_place);
                had_place.then(|| running.host.clone())
            });
            if let Some(host) = host {
                fetches.give_up_place(&host);
                self.start_queued_api_fetches();
            }
        }
    }

    /// True if the loader request `loader_id` is a call of `fetch()`.
    pub(crate) fn is_api_fetch(&self, loader_id: RequestId) -> bool {
        self.api_fetches.running.contains_key(&loader_id)
    }

    /// The loader's completion of the request `loader_id` of a call. The
    /// result of a cancelled call is dropped (with its memory); the next
    /// queued request starts.
    pub(crate) fn api_fetch_completed(
        &mut self,
        loader_id: RequestId,
        result: Result<Response, NetError>,
        kind: ResponseKind,
        reservation: Option<Arc<Reservation>>,
    ) {
        let fetches = &mut self.api_fetches;
        let Some(running) = fetches.running.remove(&loader_id) else {
            return;
        };
        if running.has_place {
            fetches.give_up_place(&running.host);
        }
        if let Some(id) = running.call {
            fetches.arrived.push((id, result, kind, reservation));
        }
        self.start_queued_api_fetches();
    }

    /// The results that wait for the runtime, in order of arrival.
    pub(crate) fn take_arrived_api_fetches(&mut self) -> Vec<ApiResult> {
        std::mem::take(&mut self.api_fetches.arrived)
    }

    /// Drops all calls (the document's scripts stop): the queue goes, the
    /// loader skips the requests that did not start, and the results of
    /// those in flight are ignored. The budget stays: the threads that still
    /// read a response hold their bytes until they let go.
    pub(crate) fn abandon_api_fetches(&mut self) {
        for loader_id in self.api_fetches.running.keys() {
            self.loader.cancel(*loader_id);
        }
        let fetches = &mut self.api_fetches;
        fetches.queued.clear();
        fetches.arrived.clear();
        fetches.places = 0;
        fetches.places_by_host.clear();
        // Completions of the old requests still arrive; they find no entry.
        fetches.running.clear();
    }

    /// True if a call waits for its result.
    pub(crate) fn has_pending_api_fetches(&self) -> bool {
        self.api_fetches.is_pending()
    }
}
