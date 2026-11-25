//! The fetch job of a media resource: it runs on a thread of the pool,
//! opens `Range` requests, reads their bodies in chunks into the block
//! cache, and ends when the cache holds enough, the resource is cancelled
//! or the reader needs another place (ADR 0047 section 5).
//!
//! A resource has at most one current job (its *epoch*). When the reader
//! seeks away, the resource starts a job with the next epoch; the old job,
//! which may wait for the network, ends at its next check without touching
//! the state.

use std::io::{self, Read};
use std::sync::Arc;
use std::time::Duration;

use log::{debug, warn};
use url::Url;

use super::cache::{BLOCK, BLOCK_U64};
use super::pool::Slot;
use super::{FAR_BLOCKS, FirstResponse, Inner, State, Status};
use crate::cors_fetch::fetch_request_with;
use crate::error::NetError;
use crate::range::{ContentRange, parse_content_range};
use crate::response::Response;

/// How often a read of a block fails without completing a block before the
/// resource fails (a server that closes the connection again and again).
const MAX_FAILURES: u32 = 3;

/// How long a job that waits for room or for the reader sleeps at most
/// before it looks again (the page budget changes without a signal).
const WAIT: Duration = Duration::from_millis(100);

/// The work of one epoch of a resource.
pub(crate) struct Job {
    inner: Arc<Inner>,
    epoch: u64,
    /// Opens the resource (the request for the first bytes decides whether
    /// it can seek).
    first: bool,
    /// False until the pool runs it: a job that is dropped before fails the
    /// resource (the pool has no thread).
    ran: bool,
}

/// An open response being read.
struct Conn {
    body: Box<dyn Read + Send>,
    /// The next block that the body gives.
    next: u64,
}

/// An answer to a range request that passed the checks.
struct Opened {
    body: Box<dyn Read + Send>,
    /// The `Content-Range` of a 206.
    range: Option<ContentRange>,
    url: Url,
}

/// How reading a block ended.
enum Filled {
    /// `BLOCK` bytes.
    Full,
    /// The body ended after this many bytes (fewer than `BLOCK`).
    End,
    /// The read failed after this many bytes.
    Failed(io::Error),
}

impl Job {
    pub(crate) fn new(inner: Arc<Inner>, epoch: u64, first: bool) -> Job {
        Job {
            inner,
            epoch,
            first,
            ran: false,
        }
    }

    /// Runs the job on a pool thread.
    pub(crate) fn run(mut self, slot: &Arc<Slot>) {
        self.ran = true;
        {
            let mut st = self.inner.lock();
            if !self.current(&st) {
                return;
            }
            if let Some(job) = st.job.as_mut() {
                job.slot = Some(Arc::clone(slot));
            }
        }
        let result = self.work();
        self.finish(result);
    }

    /// True while this job is the job of the resource.
    fn current(&self, st: &State) -> bool {
        !st.cancelled && st.epoch == self.epoch
    }

    fn finish(&self, result: Result<(), NetError>) {
        if let Err(error) = result {
            let mut st = self.inner.lock();
            if self.current(&st) {
                warn!("media {}: {error}", st.final_url);
                st.job = None;
                st.status = Status::Failed(Arc::new(error));
            }
        }
        self.inner.reader_cv.notify_all();
    }

    fn work(&self) -> Result<(), NetError> {
        let inner = &*self.inner;
        let mut conn: Option<Conn> = None;
        let mut failures = 0;
        if self.first {
            let opened = self.open(0, true)?;
            let mut st = inner.lock();
            if !self.current(&st) {
                return Ok(());
            }
            st.apply_first(&opened.url, first_response(&opened));
            inner.reader_cv.notify_all();
            conn = Some(Conn {
                body: opened.body,
                next: 0,
            });
        }
        loop {
            let (want, seekable) = {
                let mut st = inner.lock();
                if !self.current(&st) {
                    return Ok(());
                }
                match st.desired(true) {
                    Some(block) => (block, st.seekable),
                    // The cache holds enough, or everything.
                    None if st.seekable || st.end.is_some() => {
                        st.job = None;
                        inner.reader_cv.notify_all();
                        return Ok(());
                    }
                    // A stream cannot reopen: the connection waits.
                    None => {
                        let _ = inner.job_cv.wait_timeout(st, WAIT);
                        continue;
                    }
                }
            };
            // A stream reads on from where it is; a seekable resource
            // reopens when the wanted block is behind the body or more than
            // `FAR_BLOCKS` ahead of it. `Inner::plan` (mod.rs) makes the same
            // check from the reader's side; either alone is enough for a
            // right result, both exist because the position changes between
            // the two checks.
            let fits = conn
                .as_ref()
                .is_some_and(|c| !seekable || (c.next <= want && want - c.next <= FAR_BLOCKS));
            if !fits {
                if failures >= MAX_FAILURES {
                    return Err(NetError::Range(
                        "the connection failed again and again".into(),
                    ));
                }
                // Close the old connection before the new one opens.
                drop(conn.take());
                let opened = self.open(want, false)?;
                let mut st = inner.lock();
                if !self.current(&st) {
                    return Ok(());
                }
                if let Some(job) = st.job.as_mut() {
                    job.cur = want;
                }
                conn = Some(Conn {
                    body: opened.body,
                    next: want,
                });
            }
            let Some(open) = conn.as_mut() else {
                continue;
            };
            match self.read_block(open)? {
                Step::Block => failures = 0,
                Step::Retry => {
                    failures += 1;
                    conn = None;
                }
                Step::Wait => {}
                Step::Gone => return Ok(()),
            }
        }
    }

    /// Reads the block `conn.next` into the cache (or skips it when it is
    /// there already).
    fn read_block(&self, conn: &mut Conn) -> Result<Step, NetError> {
        let inner = &*self.inner;
        let index = conn.next;
        let skip = {
            let mut st = inner.lock();
            if !self.current(&st) {
                return Ok(Step::Gone);
            }
            let skip = st.cache.contains(index);
            if !skip {
                let read_block = st.pos / BLOCK_U64;
                let room = st.cache.reserve(read_block, BLOCK);
                st.stalled = !room;
                if !room {
                    // No room: wait for the reader to consume, or for the
                    // other resources of the page to free memory.
                    let _ = inner.job_cv.wait_timeout(st, WAIT);
                    return Ok(Step::Wait);
                }
            }
            skip
        };
        let mut data = Vec::with_capacity(if skip { 0 } else { BLOCK });
        let (filled, read) = fill(conn.body.as_mut(), &mut data, skip);
        let mut st = inner.lock();
        if !self.current(&st) {
            if !skip {
                st.cache.unreserve(BLOCK);
            }
            return Ok(Step::Gone);
        }
        let step = match filled {
            Filled::Full => Step::Block,
            Filled::End => {
                let total = index * BLOCK_U64 + read as u64;
                match st.size {
                    Some(size) if total < size => Step::Retry,
                    Some(_) => Step::Block,
                    None => {
                        st.end = Some(total);
                        Step::Block
                    }
                }
            }
            Filled::Failed(error) => {
                debug!(
                    "media {}: read failed at block {index}: {error}",
                    st.final_url
                );
                if !st.seekable {
                    if !skip {
                        st.cache.unreserve(BLOCK);
                    }
                    return Err(NetError::Io(error));
                }
                Step::Retry
            }
        };
        if !skip {
            if matches!(step, Step::Block) && !data.is_empty() {
                st.cache.insert(index, data);
            } else {
                st.cache.unreserve(BLOCK);
            }
        }
        if matches!(step, Step::Block) {
            conn.next += 1;
            if let Some(job) = st.job.as_mut() {
                job.cur = conn.next;
            }
        }
        drop(st);
        inner.reader_cv.notify_all();
        Ok(step)
    }

    /// Sends `bytes=START-` for the block `block` and checks the answer.
    /// `first_open` is true for the request that opens the resource.
    fn open(&self, block: u64, first_open: bool) -> Result<Opened, NetError> {
        let inner = &*self.inner;
        let mut request = inner.template.clone();
        let (url, size) = {
            let mut st = inner.lock();
            st.requests += 1;
            (st.final_url.clone(), st.size)
        };
        request.url = url;
        request
            .headers
            .set("range", format!("bytes={}-", block * BLOCK_U64));
        debug!("media request {} bytes={}-", request.url, block * BLOCK_U64);
        let (fetched, body) =
            fetch_request_with(&*inner.fetcher, request, &inner.preflights, &|request| {
                let streamed = inner.fetcher.fetch_stream(request)?;
                Ok((streamed.head, streamed.body))
            })?;
        let head = fetched.response;
        let range = check_response(&head, block, size, first_open)?;
        Ok(Opened {
            body,
            range,
            url: head.url,
        })
    }
}

/// What to do after a block.
enum Step {
    /// The block is in the cache (or was skipped).
    Block,
    /// The body failed or ended early: open a new one.
    Retry,
    /// No room: look again.
    Wait,
    /// The job is not current any more.
    Gone,
}

impl Drop for Job {
    fn drop(&mut self) {
        if self.ran {
            return;
        }
        let mut st = self.inner.lock();
        if self.current(&st) {
            st.job = None;
            st.status = Status::Failed(Arc::new(NetError::Internal(
                "no thread for the media fetch".into(),
            )));
        }
        drop(st);
        self.inner.reader_cv.notify_all();
    }
}

/// Reads up to `BLOCK` bytes into `data` (or discards them when `skip`).
/// Returns how it ended and the bytes read.
fn fill(body: &mut dyn Read, data: &mut Vec<u8>, skip: bool) -> (Filled, usize) {
    let mut chunk = vec![0u8; if skip { 8192 } else { 0 }];
    if !skip {
        data.resize(BLOCK, 0);
    }
    let mut total = 0;
    while total < BLOCK {
        let buffer = if skip {
            let n = chunk.len().min(BLOCK - total);
            &mut chunk[..n]
        } else {
            &mut data[total..]
        };
        match body.read(buffer) {
            Ok(0) => {
                data.truncate(total);
                return (Filled::End, total);
            }
            Ok(n) => total += n,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                data.truncate(total);
                return (Filled::Failed(error), total);
            }
        }
    }
    (Filled::Full, total)
}

/// Checks the status and the `Content-Range` of the answer to the request
/// for `block`.
fn check_response(
    head: &Response,
    block: u64,
    size: Option<u64>,
    first: bool,
) -> Result<Option<ContentRange>, NetError> {
    match head.status {
        206 => {
            let range = head
                .headers
                .get("content-range")
                .and_then(parse_content_range)
                .ok_or_else(|| NetError::Range("no valid Content-Range in a 206".into()))?;
            if range.start != block * BLOCK_U64 {
                return Err(NetError::Range(format!(
                    "asked for byte {}, got {}",
                    block * BLOCK_U64,
                    range.start
                )));
            }
            if let (Some(total), Some(known)) = (range.total, size)
                && total != known
            {
                return Err(NetError::Range(format!(
                    "the size changed from {known} to {total}"
                )));
            }
            Ok(Some(range))
        }
        // The whole body from byte 0: a server that ignores `Range`. For a
        // later request it is an answer only when it starts at byte 0.
        200 if first || block == 0 => Ok(None),
        200 => Err(NetError::Range(
            "the server answered 200 to a request for a later range".into(),
        )),
        status => Err(NetError::Status(status)),
    }
}

fn first_response(opened: &Opened) -> FirstResponse {
    match opened.range {
        Some(ContentRange {
            total: Some(size), ..
        }) => FirstResponse::Seekable { size },
        _ => FirstResponse::Stream,
    }
}
