//! The media byte source (ADR 0047 section 5, M14 feature 4): a resource
//! that a demux thread reads with blocking calls, over HTTP(S) with `Range`
//! requests, a local file, or bytes in memory.
//!
//! [`Loader::open_media`](crate::Loader::open_media) returns a
//! [`MediaResource`]: `read`, `seek` and `size` block the calling thread;
//! [`MediaHandle::cancel`] (any thread) makes a blocked call return an
//! error at once. The owner (the element) keeps a [`MediaHandle`] to set the
//! read-ahead ([`ReadAhead`]) and to see the state ([`MediaStats`]).
//!
//! **HTTP(S).** The first request is `GET` with `Range: bytes=0-` and
//! `Accept-Encoding: identity`. A 206 with `Content-Range` gives the size
//! and makes the resource seekable; a 200 makes it a stream: [`size`] is
//! `None` and seeks fail. Other statuses fail the resource with
//! [`NetError::Status`]. Redirects, CORS and cookies are those of the
//! request (`fetch_request_with`); the later requests go to the final URL.
//!
//! **Cache.** A job on a thread of the media pool ([`pool`]) reads the
//! body in chunks into 64 KiB blocks ([`cache`]). A read that needs a block
//! that is not there and is more than 1 MiB ahead of the running request,
//! or behind it, cancels that request's job and opens `bytes=N-` at the
//! block boundary. When the cache holds the read-ahead target ahead of the
//! reader, the job ends and drops the response (as Chromium cancels it);
//! when the reader comes within half of the target a new job opens
//! `bytes=N-`. The memory of the blocks is within a limit per resource
//! (64 MiB, 8 MiB of it behind the read position) and the [`ByteBudget`] of
//! the page (512 MiB); a job that cannot reserve a block waits (`stalled`
//! in [`MediaStats`]).
//!
//! **Failures.** A body that ends or fails before the end of the file is
//! requested again at the block that was not complete, up to three times
//! in a row without a completed block; then the resource fails. A failed
//! resource still serves the blocks that it has. A stream (no `Range`)
//! cannot reopen: its failure ends it at that point.
//!
//! [`size`]: MediaResource::size

mod cache;
mod job;
mod pool;
#[cfg(test)]
mod tests;

use std::fs::File;
use std::io;
use std::os::unix::fs::FileExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use log::debug;
use url::Url;

pub use self::cache::BLOCK;
use self::cache::{BLOCK_U64, Cache};
use self::job::Job;
pub(crate) use self::pool::MediaPool;
use self::pool::Slot;
use crate::budget::ByteBudget;
use crate::cors::PreflightCache;
use crate::error::NetError;
use crate::fetch::Fetcher;
use crate::request::{Destination, Request};

/// The most bytes of cache for one resource.
pub const ELEMENT_CACHE_BYTES: usize = 64 << 20;

/// The bytes behind the read position that eviction keeps (for short seeks
/// back).
pub const KEEP_BEHIND_BYTES: usize = 8 << 20;

/// The default size of the [`ByteBudget`] that all media resources of a
/// page share.
pub const PAGE_BUDGET_BYTES: usize = 512 << 20;

/// A request that is more than this many blocks (1 MiB) ahead of the
/// running request does not wait for it: it opens a new one.
pub(crate) const FAR_BLOCKS: u64 = (1 << 20) / BLOCK_U64;

/// The least read-ahead target: a smaller one would open a request for each
/// block.
const MIN_READ_AHEAD: u64 = 1 << 20;

/// The bit rate that turns seconds of read-ahead into bytes when the size
/// or the duration is not known (ADR 0047 section 5).
const DEFAULT_BYTES_PER_SECOND: f64 = (1 << 20) as f64;

/// How much to read ahead of the playback position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReadAhead {
    /// This many bytes.
    Bytes(u64),
    /// This many seconds of media: bytes by the average bit rate (the size
    /// over the duration, [`MediaHandle::set_duration`]), else 1 MiB per
    /// second.
    Seconds(f64),
}

impl ReadAhead {
    /// `preload=auto` and playback: 15 s.
    pub const AUTO: ReadAhead = ReadAhead::Seconds(15.0);
    /// `preload=metadata` after `loadeddata`: 3 s.
    pub const METADATA: ReadAhead = ReadAhead::Seconds(3.0);
}

/// The limits of one resource.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MediaOptions {
    /// The most bytes of cache ([`ELEMENT_CACHE_BYTES`]).
    pub cache_bytes: usize,
    /// The bytes behind the read position that eviction keeps
    /// ([`KEEP_BEHIND_BYTES`]).
    pub keep_behind_bytes: usize,
    /// The read-ahead target at the start ([`ReadAhead::AUTO`]).
    pub read_ahead: ReadAhead,
}

impl Default for MediaOptions {
    fn default() -> Self {
        MediaOptions {
            cache_bytes: ELEMENT_CACHE_BYTES,
            keep_behind_bytes: KEEP_BEHIND_BYTES,
            read_ahead: ReadAhead::AUTO,
        }
    }
}

/// The state of a resource, for the owner and for tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaStats {
    /// The bytes that the cache holds now (blocks and the block being
    /// read).
    pub cached_bytes: usize,
    /// The most bytes that the cache held at one time.
    pub peak_cached_bytes: usize,
    /// The requests sent (HTTP) so far.
    pub requests: u64,
    /// True while a job waits for room in the cache or the page budget.
    pub stalled: bool,
    /// True when the resource can seek (known size).
    pub seekable: bool,
    /// The size in bytes, when known and seekable.
    pub size: Option<u64>,
}

/// Where the resource is.
#[derive(Debug, Clone)]
pub(crate) enum Status {
    /// The first request is on its way.
    Opening,
    Ready,
    Failed(Arc<NetError>),
}

/// How the first response opened the resource.
#[derive(Debug, Clone, Copy)]
pub(crate) enum FirstResponse {
    Seekable { size: u64 },
    Stream,
}

/// The place of the running job in the state.
pub(crate) struct JobInfo {
    /// The block that the job reads next (for the check of the reader's
    /// position against the job's).
    pub(crate) cur: u64,
    /// Its place in the pool, when it has started.
    pub(crate) slot: Option<Arc<Slot>>,
}

/// What the resource shares between the reader and the fetch job.
pub(crate) struct State {
    pub(crate) status: Status,
    pub(crate) seekable: bool,
    /// The size, for a seekable resource.
    pub(crate) size: Option<u64>,
    /// Where the bytes end, when known (the size; for a stream, where the
    /// body ended).
    pub(crate) end: Option<u64>,
    pub(crate) cache: Cache,
    /// The read position of the reader that moved last.
    pub(crate) pos: u64,
    pub(crate) cancelled: bool,
    pub(crate) job: Option<JobInfo>,
    /// The number of the current job; a job of another number is stale.
    pub(crate) epoch: u64,
    read_ahead: ReadAhead,
    duration: Option<f64>,
    /// The URL of the later requests: the final URL of the first one.
    pub(crate) final_url: Url,
    pub(crate) requests: u64,
    pub(crate) stalled: bool,
}

impl State {
    /// The first response arrived.
    pub(crate) fn apply_first(&mut self, url: &Url, response: FirstResponse) {
        self.final_url = url.clone();
        match response {
            FirstResponse::Seekable { size } => {
                self.seekable = true;
                self.size = Some(size);
                self.end = Some(size);
            }
            FirstResponse::Stream => self.seekable = false,
        }
        self.status = Status::Ready;
    }

    /// The read-ahead target in bytes.
    fn target_bytes(&self) -> u64 {
        let wanted = match self.read_ahead {
            ReadAhead::Bytes(bytes) => bytes,
            ReadAhead::Seconds(seconds) => {
                let rate = match (self.size, self.duration) {
                    (Some(size), Some(duration)) if duration > 0.0 => size as f64 / duration,
                    _ => DEFAULT_BYTES_PER_SECOND,
                };
                let bytes = seconds.max(0.0) * rate;
                if bytes.is_finite() {
                    bytes as u64
                } else {
                    u64::MAX
                }
            }
        };
        let most = self.cache.limit().saturating_sub(self.cache.keep_behind()) as u64;
        wanted.max(MIN_READ_AHEAD).min(most.max(BLOCK_U64))
    }

    /// The block where a request should run now, or `None` when the cache
    /// holds enough: the first block from the reader's position that is not
    /// cached, when the reader waits for it or the cache ahead is below the
    /// threshold (the target while a job runs, half of it before one
    /// starts).
    pub(crate) fn desired(&self, running: bool) -> Option<u64> {
        let pos = self.pos;
        if self.end.is_some_and(|end| pos >= end) {
            return None;
        }
        let first_missing = self.cache.first_missing(pos / BLOCK_U64);
        let missing_at = first_missing * BLOCK_U64;
        if self.end.is_some_and(|end| missing_at >= end) {
            return None;
        }
        let ahead = missing_at.saturating_sub(pos);
        let target = self.target_bytes();
        let threshold = if running { target } else { target / 2 };
        (missing_at <= pos || ahead < threshold).then_some(first_missing)
    }

    /// Decides whether a job must start or restart for the reader's
    /// position. The work is returned, not done: the caller runs it after it
    /// has released the lock.
    fn plan(&mut self, inner: &Arc<Inner>) -> Action {
        let mut action = Action::default();
        if self.cancelled || !matches!(self.status, Status::Ready) {
            return action;
        }
        let Some(want) = self.desired(self.job.is_some()) else {
            return action;
        };
        match &self.job {
            // A stream cannot reopen: its job reads on, and none starts.
            Some(_) | None if !self.seekable => return action,
            // A far block (behind the job, or more than `FAR_BLOCKS` ahead)
            // replaces the job at once. The job repeats the check on its
            // own body (`job.rs`, `work`) because the reader's position
            // changes between two `plan` calls. Each check alone keeps the
            // result right (a mutation test shows it); both stay, so a job
            // never reads a far block through a body that cannot reach it
            // cheaply, whichever side sees the move first.
            Some(job) if job.cur <= want && want - job.cur <= FAR_BLOCKS => return action,
            Some(job) => action.release.clone_from(&job.slot),
            None => {}
        }
        self.epoch += 1;
        self.job = Some(JobInfo {
            cur: want,
            slot: None,
        });
        action.spawn = Some(Job::new(Arc::clone(inner), self.epoch, false));
        action
    }
}

/// Work that the lock holder hands to its caller.
#[derive(Default)]
struct Action {
    spawn: Option<Job>,
    release: Option<Arc<Slot>>,
}

/// The core of a network resource.
pub(crate) struct Inner {
    state: Mutex<State>,
    /// Signals the reader: a block, the end, a failure, the first response.
    pub(crate) reader_cv: Condvar,
    /// Signals the job: the reader consumed, a new target, a cancel.
    pub(crate) job_cv: Condvar,
    pub(crate) fetcher: Arc<dyn Fetcher>,
    pub(crate) preflights: PreflightCache,
    /// The request of the resource; the jobs add `Range`.
    pub(crate) template: Request,
    pool: Arc<MediaPool>,
}

impl Inner {
    pub(crate) fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts the job that sends the first request.
    fn start(self: &Arc<Self>) {
        let job = Self::first_job(self, &mut self.lock());
        self.pool.submit(Box::new(move |slot| job.run(slot)));
    }

    /// The job that sends the first request, as the current job of `st`.
    fn first_job(this: &Arc<Self>, st: &mut State) -> Job {
        st.epoch += 1;
        st.job = Some(JobInfo { cur: 0, slot: None });
        Job::new(Arc::clone(this), st.epoch, true)
    }

    /// Undoes a cancel. A resource that was cancelled while the first
    /// request was on its way is still `Opening` and has no job (the cancel
    /// made that job stale): the first request starts again. Otherwise the
    /// next read starts the job that its position needs.
    fn resume(self: &Arc<Self>) {
        let job = {
            let mut st = self.lock();
            st.cancelled = false;
            (matches!(st.status, Status::Opening) && st.job.is_none())
                .then(|| Self::first_job(self, &mut st))
        };
        if let Some(job) = job {
            self.pool.submit(Box::new(move |slot| job.run(slot)));
        }
    }

    /// The last owner is gone: frees the cache at once, also when a job
    /// thread that waits for the network still holds `Inner` (the page's
    /// budget gets the bytes back now, not when that thread ends).
    fn discard(&self) {
        self.lock().cache.close();
    }

    /// Starts or restarts the job that the reader's position needs; returns
    /// the lock again.
    fn kick<'a>(self: &'a Arc<Self>, mut st: MutexGuard<'a, State>) -> MutexGuard<'a, State> {
        let action = st.plan(self);
        if action.spawn.is_none() && action.release.is_none() {
            return st;
        }
        drop(st);
        if let Some(slot) = action.release {
            self.pool.release(&slot);
        }
        if let Some(job) = action.spawn {
            self.pool.submit(Box::new(move |slot| job.run(slot)));
        }
        self.lock()
    }

    fn cancel(&self) {
        let slot = {
            let mut st = self.lock();
            st.cancelled = true;
            // The running job is stale for good: a `resume` must not revive
            // a job that sits in a network call (it starts a new one).
            st.epoch += 1;
            st.job.take().and_then(|job| job.slot)
        };
        self.reader_cv.notify_all();
        self.job_cv.notify_all();
        if let Some(slot) = slot {
            self.pool.release(&slot);
        }
    }

    fn wait_opened<'a>(&'a self, mut st: MutexGuard<'a, State>) -> MutexGuard<'a, State> {
        while matches!(st.status, Status::Opening) && !st.cancelled {
            st = self
                .reader_cv
                .wait(st)
                .unwrap_or_else(PoisonError::into_inner);
        }
        st
    }

    fn read(self: &Arc<Self>, pos: &mut u64, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut st = self.lock();
        loop {
            if st.cancelled {
                return Err(cancelled());
            }
            if matches!(st.status, Status::Opening) {
                st = self.wait_opened(st);
                continue;
            }
            if st.end.is_some_and(|end| *pos >= end) {
                return Ok(0);
            }
            let (index, offset) = (*pos / BLOCK_U64, (*pos % BLOCK_U64) as usize);
            if let Some(block) = st.cache.get(index) {
                let Some(rest) = block.get(offset..).filter(|rest| !rest.is_empty()) else {
                    // A block shorter than `BLOCK` is the last one.
                    let end = index * BLOCK_U64 + block.len() as u64;
                    st.end.get_or_insert(end);
                    return Ok(0);
                };
                let n = rest.len().min(buf.len());
                buf[..n].copy_from_slice(&rest[..n]);
                *pos += n as u64;
                st.pos = *pos;
                let st = self.kick(st);
                drop(st);
                self.job_cv.notify_all();
                return Ok(n);
            }
            if let Status::Failed(error) = &st.status {
                return Err(io::Error::other(error.to_string()));
            }
            st.pos = *pos;
            st = self.kick(st);
            st = self
                .reader_cv
                .wait(st)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn seek(self: &Arc<Self>, pos: u64) -> io::Result<u64> {
        let st = self.wait_opened(self.lock());
        if st.cancelled {
            return Err(cancelled());
        }
        if let Status::Failed(error) = &st.status {
            return Err(io::Error::other(error.to_string()));
        }
        if !st.seekable {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "the server does not support range requests: the resource cannot seek",
            ));
        }
        let mut st = st;
        st.pos = pos;
        drop(self.kick(st));
        Ok(pos)
    }

    fn size(&self) -> Option<u64> {
        let st = self.wait_opened(self.lock());
        if st.cancelled || !matches!(st.status, Status::Ready) {
            return None;
        }
        st.seekable.then_some(st.size).flatten()
    }

    fn stats(&self) -> MediaStats {
        let st = self.lock();
        MediaStats {
            cached_bytes: st.cache.reserved(),
            peak_cached_bytes: st.cache.peak(),
            requests: st.requests,
            stalled: st.stalled,
            seekable: st.seekable,
            size: st.size,
        }
    }
}

fn cancelled() -> io::Error {
    // Not `Interrupted`: callers retry that.
    io::Error::new(
        io::ErrorKind::ConnectionAborted,
        "the media load was cancelled",
    )
}

enum Kind {
    Net(Arc<Inner>),
    File { file: File, size: u64 },
    Memory(Arc<[u8]>),
    Failed(Arc<NetError>),
}

/// What a resource and its handles share.
struct Control {
    kind: Kind,
    /// Cancelled (for the kinds without a state of their own).
    cancelled: AtomicBool,
}

impl Control {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Kind::Net(inner) = &self.kind {
            inner.cancel();
        }
    }
}

impl Drop for Control {
    fn drop(&mut self) {
        self.cancel();
        // `cancel` keeps the cache (a `resume` uses it); the last owner is
        // gone, so free it.
        if let Kind::Net(inner) = &self.kind {
            inner.discard();
        }
    }
}

/// A media resource for one reader: blocking `read`, `seek` and `size`
/// (the calls of `swb_media::ByteSource`). Create it with
/// [`Loader::open_media`](crate::Loader::open_media). It is `Send`: the
/// demux thread owns it.
pub struct MediaResource {
    control: Arc<Control>,
    pos: u64,
}

/// The owner's handle to a resource: cancel from any thread, the read-ahead,
/// the state, and more readers. Dropping the last handle and reader
/// cancels the resource.
#[derive(Clone)]
pub struct MediaHandle {
    control: Arc<Control>,
}

impl MediaResource {
    fn new(kind: Kind) -> MediaResource {
        MediaResource {
            control: Arc::new(Control {
                kind,
                cancelled: AtomicBool::new(false),
            }),
            pos: 0,
        }
    }

    /// A resource over bytes in memory (`blob:` URLs, tests): seekable, with
    /// a size.
    pub fn from_bytes(data: impl Into<Arc<[u8]>>) -> MediaResource {
        MediaResource::new(Kind::Memory(data.into()))
    }

    /// A resource that failed to open: every call fails with `error`.
    pub(crate) fn failed(error: NetError) -> MediaResource {
        MediaResource::new(Kind::Failed(Arc::new(error)))
    }

    /// A seekable resource over the file at `path`.
    pub(crate) fn from_file(file: File, size: u64) -> MediaResource {
        MediaResource::new(Kind::File { file, size })
    }

    /// A network resource; sends the first request on the pool.
    pub(crate) fn network(
        request: Request,
        fetcher: Arc<dyn Fetcher>,
        pool: Arc<MediaPool>,
        budget: Arc<ByteBudget>,
        options: MediaOptions,
    ) -> MediaResource {
        let inner = Arc::new(Inner {
            state: Mutex::new(State {
                status: Status::Opening,
                seekable: false,
                size: None,
                end: None,
                cache: Cache::new(options.cache_bytes, options.keep_behind_bytes, budget),
                pos: 0,
                cancelled: false,
                job: None,
                epoch: 0,
                read_ahead: options.read_ahead,
                duration: None,
                final_url: request.url.clone(),
                requests: 0,
                stalled: false,
            }),
            reader_cv: Condvar::new(),
            job_cv: Condvar::new(),
            fetcher,
            preflights: PreflightCache::new(),
            template: request,
            pool,
        });
        inner.start();
        MediaResource::new(Kind::Net(inner))
    }

    /// The handle for the owner.
    pub fn handle(&self) -> MediaHandle {
        MediaHandle {
            control: Arc::clone(&self.control),
        }
    }

    /// Reads up to `buf.len()` bytes at the position; `Ok(0)` at the end.
    /// Blocks until a block is there, the end, a failure or a cancel.
    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let control = Arc::clone(&self.control);
        match &control.kind {
            Kind::Net(inner) => inner.read(&mut self.pos, buf),
            Kind::File { file, .. } => {
                if control.cancelled.load(Ordering::Relaxed) {
                    return Err(cancelled());
                }
                let n = file.read_at(buf, self.pos)?;
                self.pos += n as u64;
                Ok(n)
            }
            Kind::Memory(data) => {
                if control.cancelled.load(Ordering::Relaxed) {
                    return Err(cancelled());
                }
                let start = usize::try_from(self.pos).unwrap_or(usize::MAX);
                let rest = data.get(start..).unwrap_or_default();
                let n = rest.len().min(buf.len());
                buf[..n].copy_from_slice(&rest[..n]);
                self.pos += n as u64;
                Ok(n)
            }
            Kind::Failed(error) => Err(io::Error::other(error.to_string())),
        }
    }

    /// Moves to the byte `pos` and returns it. Fails for a stream (a server
    /// without `Range`). A position past the end reads nothing.
    pub fn seek(&mut self, pos: u64) -> io::Result<u64> {
        let control = Arc::clone(&self.control);
        match &control.kind {
            Kind::Net(inner) => {
                inner.seek(pos)?;
            }
            Kind::File { .. } | Kind::Memory(_) => {
                if control.cancelled.load(Ordering::Relaxed) {
                    return Err(cancelled());
                }
            }
            Kind::Failed(error) => return Err(io::Error::other(error.to_string())),
        }
        self.pos = pos;
        Ok(pos)
    }

    /// The size in bytes, when known and the resource can seek. Waits for
    /// the first response of a network resource. `None` makes the demuxer
    /// read the resource as a stream.
    pub fn size(&mut self) -> Option<u64> {
        match &self.control.kind {
            Kind::Net(inner) => inner.size(),
            Kind::File { size, .. } => Some(*size),
            Kind::Memory(data) => Some(data.len() as u64),
            Kind::Failed(_) => None,
        }
    }

    /// Cancels the resource (see [`MediaHandle::cancel`]).
    pub fn cancel(&self) {
        self.control.cancel();
    }
}

impl MediaHandle {
    /// Cancels the resource from any thread: the running request is dropped,
    /// a blocked `read` or `seek` returns an error at once, and later calls
    /// fail. The cache stays until the last handle and reader are dropped.
    /// A request that waits for the server ends when the server answers or
    /// the time limit of the request passes; it no longer holds a place in
    /// the media pool.
    pub fn cancel(&self) {
        debug!("media resource cancelled");
        self.control.cancel();
    }

    /// Undoes [`MediaHandle::cancel`] for a resource that was cancelled and
    /// is read again (a suspended pipeline that opens again): new readers
    /// get the cache and the next request opens where they read.
    pub fn resume(&self) {
        self.control.cancelled.store(false, Ordering::Relaxed);
        if let Kind::Net(inner) = &self.control.kind {
            inner.resume();
        }
    }

    /// A new reader at position 0 over the same cache.
    pub fn reader(&self) -> MediaResource {
        MediaResource {
            control: Arc::clone(&self.control),
            pos: 0,
        }
    }

    /// Sets the read-ahead target.
    pub fn set_read_ahead(&self, read_ahead: ReadAhead) {
        if let Kind::Net(inner) = &self.control.kind {
            let mut st = inner.lock();
            st.read_ahead = read_ahead;
            let st = inner.kick(st);
            drop(st);
            inner.job_cv.notify_all();
        }
    }

    /// Tells the duration in seconds, for [`ReadAhead::Seconds`].
    pub fn set_duration(&self, seconds: Option<f64>) {
        if let Kind::Net(inner) = &self.control.kind {
            inner.lock().duration = seconds.filter(|s| s.is_finite() && *s > 0.0);
        }
    }

    /// The state of the cache and the requests.
    pub fn stats(&self) -> MediaStats {
        match &self.control.kind {
            Kind::Net(inner) => inner.stats(),
            Kind::File { size, .. } => MediaStats {
                cached_bytes: 0,
                peak_cached_bytes: 0,
                requests: 0,
                stalled: false,
                seekable: true,
                size: Some(*size),
            },
            Kind::Memory(data) => MediaStats {
                cached_bytes: 0,
                peak_cached_bytes: 0,
                requests: 0,
                stalled: false,
                seekable: true,
                size: Some(data.len() as u64),
            },
            Kind::Failed(_) => MediaStats {
                cached_bytes: 0,
                peak_cached_bytes: 0,
                requests: 0,
                stalled: false,
                seekable: false,
                size: None,
            },
        }
    }

    /// The error that failed the resource, if one did: the status of the
    /// first response (`Status(404)`), a failed CORS check, a connection
    /// error.
    pub fn error(&self) -> Option<Arc<NetError>> {
        match &self.control.kind {
            Kind::Net(inner) => match &inner.lock().status {
                Status::Failed(error) => Some(Arc::clone(error)),
                _ => None,
            },
            Kind::Failed(error) => Some(Arc::clone(error)),
            Kind::File { .. } | Kind::Memory(_) => None,
        }
    }

    /// Waits until the first response arrived (or the resource failed or
    /// was cancelled), at most `timeout`. True when the resource is open.
    pub fn wait_open(&self, timeout: Duration) -> bool {
        let Kind::Net(inner) = &self.control.kind else {
            return self.error().is_none();
        };
        let deadline = std::time::Instant::now() + timeout;
        let mut st = inner.lock();
        while matches!(st.status, Status::Opening) && !st.cancelled {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return false;
            }
            st = inner
                .reader_cv
                .wait_timeout(st, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        matches!(st.status, Status::Ready) && !st.cancelled
    }
}

impl std::fmt::Debug for MediaResource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaResource")
            .field("pos", &self.pos)
            .finish_non_exhaustive()
    }
}

/// Opens `request` for the media byte source.
pub(crate) fn open(
    mut request: Request,
    fetcher: &Arc<dyn Fetcher>,
    pool: &Arc<MediaPool>,
    budget: &Arc<ByteBudget>,
    options: MediaOptions,
) -> MediaResource {
    // The destination makes the request send `Accept-Encoding: identity`.
    if !matches!(request.destination, Destination::Video | Destination::Audio) {
        request.destination = Destination::Video;
    }
    // The jobs add their own range.
    request.headers.remove("range");
    match request.url.scheme() {
        "file" => open_file(&request.url),
        "data" => match crate::builtin::fetch(&request) {
            Some(Ok(response)) => MediaResource::from_bytes(response.body),
            Some(Err(error)) => MediaResource::failed(error),
            None => MediaResource::failed(NetError::Internal("data: URL not handled".into())),
        },
        "http" | "https" => MediaResource::network(
            request,
            Arc::clone(fetcher),
            Arc::clone(pool),
            Arc::clone(budget),
            options,
        ),
        scheme => MediaResource::failed(NetError::UnsupportedScheme(scheme.to_owned())),
    }
}

/// A `file:` URL: read directly (no cache and no read-ahead: the operating
/// system's cache does that). A missing file is status 404, as for images; a
/// directory or another kind of file is an error.
fn open_file(url: &Url) -> MediaResource {
    let Ok(path) = url.to_file_path() else {
        return MediaResource::failed(NetError::InvalidUrl {
            url: url.clone(),
            reason: "not a local file path".to_owned(),
        });
    };
    let opened = std::fs::metadata(&path).and_then(|metadata| {
        if metadata.is_file() {
            File::open(&path).map(|file| (file, metadata.len()))
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} is not a regular file", path.display()),
            ))
        }
    });
    match opened {
        Ok((file, size)) => MediaResource::from_file(file, size),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            MediaResource::failed(NetError::Status(404))
        }
        Err(error) => MediaResource::failed(NetError::Io(error)),
    }
}
