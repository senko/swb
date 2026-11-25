//! [`ReplayFetcher`]: answers requests from a fixture directory.

use std::collections::HashMap;
use std::collections::hash_map;
use std::fmt;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use log::{debug, warn};

use super::{
    EntryKey, body_path, entry_url, fixture_error, natural_cmp, pattern_matches, read_manifest,
    read_replay_patterns, request_hash,
};
use crate::builtin;
use crate::cookies::CookieJar;
use crate::error::NetError;
use crate::fetch::{Fetcher, StreamResponse};
use crate::headers::Headers;
use crate::range::{Resolved, parse_range, resolve};
use crate::request::{Method, Request};
use crate::response::Response;

/// Answers requests from a fixture directory, without network access.
///
/// [`ReplayFetcher::load`] reads the manifest and all body files. A request
/// that has no entry in the manifest returns [`NetError::NotInFixture`] and
/// logs a warning. `data:` and `about:` URLs work as in
/// [`NetworkFetcher`](crate::NetworkFetcher). The format is described in the
/// [module documentation](super).
pub struct ReplayFetcher {
    dir: PathBuf,
    /// Keyed by (url, method, request body hash).
    entries: HashMap<EntryKey, ReplayEntry>,
    /// The jar for scripts (`document.cookie`, ADR 0028 section 12). A
    /// replay sends no `Cookie` header and the fixtures have no
    /// `Set-Cookie` header, so only scripts use it.
    cookies: CookieJar,
    /// The `replay_patterns` of `fixture.json`.
    patterns: Vec<String>,
}

struct ReplayEntry {
    status: u16,
    headers: Headers,
    body: ReplayBody,
}

/// Bodies larger than this stay in their file and are read at an offset
/// when a request needs them (a media file: the 4K test file is 161 MB).
const LAZY_BODY_BYTES: u64 = 8 << 20;

/// A recorded body: in memory, or a file that is read on demand.
#[derive(Clone)]
enum ReplayBody {
    Memory(Arc<Vec<u8>>),
    File { path: Arc<PathBuf>, len: u64 },
}

impl ReplayBody {
    fn len(&self) -> u64 {
        match self {
            ReplayBody::Memory(data) => data.len() as u64,
            ReplayBody::File { len, .. } => *len,
        }
    }

    /// The bytes `start..start + len` as a reader. The range is inside the
    /// body (the callers resolved it against [`ReplayBody::len`]).
    fn reader(&self, start: u64, len: u64) -> Result<Box<dyn Read + Send>, NetError> {
        match self {
            ReplayBody::Memory(data) => Ok(Box::new(MemoryRange {
                data: Arc::clone(data),
                pos: usize::try_from(start).unwrap_or(usize::MAX),
                end: usize::try_from(start.saturating_add(len)).unwrap_or(usize::MAX),
            })),
            ReplayBody::File { path, .. } => {
                let mut file =
                    fs::File::open(path.as_path()).map_err(|error| fixture_error(path, &error))?;
                file.seek(SeekFrom::Start(start))
                    .map_err(|error| fixture_error(path, &error))?;
                Ok(Box::new(file.take(len)))
            }
        }
    }

    /// The bytes `start..start + len` in memory.
    fn read(&self, start: u64, len: u64) -> Result<Vec<u8>, NetError> {
        if let ReplayBody::Memory(data) = self
            && start == 0
            && len == data.len() as u64
        {
            return Ok(data.as_ref().clone());
        }
        let mut bytes = Vec::new();
        let path = match self {
            ReplayBody::File { path, .. } => path.as_path(),
            ReplayBody::Memory(_) => Path::new("(memory)"),
        };
        self.reader(start, len)?
            .read_to_end(&mut bytes)
            .map_err(|error| fixture_error(path, &error))?;
        Ok(bytes)
    }
}

/// A reader over a part of a body in memory.
struct MemoryRange {
    data: Arc<Vec<u8>>,
    pos: usize,
    end: usize,
}

impl Read for MemoryRange {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let end = self.end.min(self.data.len());
        let rest = self.data.get(self.pos..end).unwrap_or_default();
        let n = rest.len().min(buf.len());
        buf[..n].copy_from_slice(&rest[..n]);
        self.pos += n;
        Ok(n)
    }
}

impl ReplayFetcher {
    /// Loads the fixture in `dir`. Fails if the manifest is missing or
    /// invalid, or if a body file cannot be read. If the manifest has
    /// several entries for the same URL and method, the first one is used.
    /// A body file over [`LAZY_BODY_BYTES`] is not read here: it is read
    /// at an offset when a request needs it.
    pub fn load(dir: impl AsRef<Path>) -> Result<Self, NetError> {
        let dir = dir.as_ref();
        let mut bodies: HashMap<String, ReplayBody> = HashMap::new();
        let mut entries = HashMap::new();
        for entry in read_manifest(dir)? {
            let body = if let Some(body) = bodies.get(&entry.body) {
                body.clone()
            } else {
                let path = body_path(dir, &entry.body)?;
                let len = fs::metadata(&path)
                    .map_err(|error| fixture_error(&path, &error))?
                    .len();
                let body = if len > LAZY_BODY_BYTES {
                    ReplayBody::File {
                        path: Arc::new(path),
                        len,
                    }
                } else {
                    let data = fs::read(&path).map_err(|error| fixture_error(&path, &error))?;
                    ReplayBody::Memory(Arc::new(data))
                };
                bodies.insert(entry.body.clone(), body.clone());
                body
            };
            match entries.entry((entry.url, entry.method, entry.request_body)) {
                hash_map::Entry::Occupied(occupied) => {
                    let (url, method, _) = occupied.key();
                    warn!("fixture {}: duplicate entry {method} {url}", dir.display());
                }
                hash_map::Entry::Vacant(vacant) => {
                    vacant.insert(ReplayEntry {
                        status: entry.status,
                        headers: entry.headers.into_iter().collect(),
                        body,
                    });
                }
            }
        }
        Ok(ReplayFetcher {
            dir: dir.to_path_buf(),
            entries,
            cookies: CookieJar::new(),
            patterns: read_replay_patterns(dir)?,
        })
    }

    /// Returns the fixture directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Returns the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the fixture has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl ReplayFetcher {
    /// The entry for a request: the entry of its URL, else (when the
    /// fixture has `replay_patterns`) the entry of the recorded URL with the
    /// greatest URL (numbers compared as numbers) among those that match
    /// the same pattern as the request URL, for the same method. An entry
    /// with a `request_body` hash matches only a request with that body hash
    /// and comes first, also among pattern entries of the same URL; the
    /// entries without it match any body.
    fn lookup(&self, url: &str, method: &str, body: Option<&str>) -> Option<&ReplayEntry> {
        let key = |hash: Option<&str>| (url.to_owned(), method.to_owned(), hash.map(str::to_owned));
        if body.is_some()
            && let Some(entry) = self.entries.get(&key(body))
        {
            return Some(entry);
        }
        if let Some(entry) = self.entries.get(&key(None)) {
            return Some(entry);
        }
        let pattern = self.patterns.iter().find(|p| pattern_matches(p, url))?;
        self.entries
            .iter()
            .filter(|((u, m, h), _)| {
                m == method && pattern_matches(pattern, u) && (h.is_none() || h.as_deref() == body)
            })
            .max_by(|((a, _, a_hash), _), ((b, _, b_hash), _)| {
                natural_cmp(a, b).then_with(|| a_hash.is_some().cmp(&b_hash.is_some()))
            })
            .map(|(_, entry)| entry)
    }
}

/// The answer to a CORS preflight: the request is allowed as far as it
/// asks (ADR 0028 section 12: both browsers see the same permissive
/// server).
fn preflight_response(request: &Request) -> Response {
    let mut headers = Headers::new();
    headers.append(
        "access-control-allow-origin",
        request.origin_header().unwrap_or_else(|| "*".to_owned()),
    );
    headers.append("access-control-allow-credentials", "true");
    for (name, answer) in [
        (
            "access-control-request-method",
            "access-control-allow-methods",
        ),
        (
            "access-control-request-headers",
            "access-control-allow-headers",
        ),
    ] {
        if let Some(value) = request.headers.get(name) {
            headers.append(answer, value);
        }
    }
    headers.append("vary", "Origin");
    Response {
        url: request.url.clone(),
        status: 204,
        headers,
        body: Vec::new(),
        redirected: false,
    }
}

/// True for a CORS preflight request: `OPTIONS` with
/// `Access-Control-Request-Method`.
fn is_preflight(request: &Request) -> bool {
    request.method == Method::Options && request.headers.contains("access-control-request-method")
}

/// The headers of a replayed response: the stored ones without
/// `Access-Control-*`, and, for a request with an `Origin` header, the
/// request's origin as allowed origin, credentials allowed, `Vary: Origin`
/// and the names of the stored headers as exposed headers. The fixtures
/// store no CORS headers; the Chromium routing of the test tools adds the
/// same (`tools/swbtools/routing.py`, `served_headers`).
fn served_headers(request: &Request, entry: &ReplayEntry) -> Headers {
    let mut headers: Headers = entry
        .headers
        .iter()
        .filter(|(name, _)| !name.starts_with("access-control-"))
        .collect();
    if let Some(origin) = request.origin_header() {
        headers.append("access-control-allow-origin", origin);
        headers.append("access-control-allow-credentials", "true");
        headers.append("vary", "Origin");
        let names: Vec<&str> = entry.headers.iter().map(|(name, _)| name).collect();
        if !names.is_empty() {
            headers.append("access-control-expose-headers", names.join(", "));
        }
    }
    headers
}

/// What a replayed request gets: the head and the part of the body.
struct Answer {
    head: Response,
    body: ReplayBody,
    start: u64,
    len: u64,
}

impl ReplayFetcher {
    /// The answer to `request`. A `GET` with a single `Range` of `bytes`
    /// to an entry with status 200 gets the range of the recorded body, as
    /// a server with range support answers: 206 with `Content-Range`, 416
    /// (`Content-Range: bytes */LEN`) outside the body, 200 for a range
    /// that is not valid; all with `Accept-Ranges: bytes` (M14 feature 4).
    fn answer(&self, request: &Request) -> Result<Answer, NetError> {
        let url = entry_url(&request.url);
        let body = request_hash(request);
        let Some(entry) = self.lookup(&url, request.method.as_str(), body.as_deref()) else {
            warn!(
                "fixture {}: no entry for {} {}",
                self.dir.display(),
                request.method,
                url
            );
            return Err(NetError::NotInFixture(request.url.clone()));
        };
        debug!("served {} {}", request.method, url);
        let mut headers = served_headers(request, entry);
        let total = entry.body.len();
        let (mut status, mut start, mut len) = (entry.status, 0, total);
        if request.method == Method::Get
            && entry.status == 200
            && let Some(value) = request.headers.get("range")
        {
            // The stored `Content-Length` is that of the whole body, and a
            // stored `Accept-Ranges` or `Content-Range` would repeat ours.
            let partial_headers = |headers: &mut Headers| {
                for name in ["content-length", "accept-ranges", "content-range"] {
                    headers.remove(name);
                }
                headers.append("accept-ranges", "bytes");
            };
            headers.set("accept-ranges", "bytes");
            match parse_range(value).map(|spec| resolve(spec, total)) {
                Some(Resolved::Partial { start: first, end }) => {
                    status = 206;
                    (start, len) = (first, end - first + 1);
                    partial_headers(&mut headers);
                    headers.append("content-range", format!("bytes {first}-{end}/{total}"));
                }
                Some(Resolved::Unsatisfiable) => {
                    status = 416;
                    len = 0;
                    partial_headers(&mut headers);
                    headers.append("content-range", format!("bytes */{total}"));
                }
                Some(Resolved::Whole) | None => {}
            }
        }
        Ok(Answer {
            head: Response {
                url: request.url.clone(),
                status,
                headers,
                body: Vec::new(),
                redirected: false,
            },
            body: entry.body.clone(),
            start,
            len,
        })
    }
}

impl Fetcher for ReplayFetcher {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        if let Some(result) = builtin::fetch(request) {
            return result;
        }
        if is_preflight(request) {
            return Ok(preflight_response(request));
        }
        let Answer {
            mut head,
            body,
            start,
            len,
        } = self.answer(request)?;
        head.body = body.read(start, len)?;
        Ok(head)
    }

    /// Like [`ReplayFetcher::fetch`], but a body that is in a file is read
    /// at the offset of the range when the caller reads it, not loaded.
    fn fetch_stream(&self, request: &Request) -> Result<StreamResponse, NetError> {
        if builtin::handles(&request.url) || is_preflight(request) {
            return self.fetch(request).map(StreamResponse::buffered);
        }
        let Answer {
            head,
            body,
            start,
            len,
        } = self.answer(request)?;
        Ok(StreamResponse {
            head,
            body: body.reader(start, len)?,
        })
    }

    /// The in-memory jar of the replay, empty until a script stores a
    /// cookie.
    fn cookie_jar(&self) -> Option<&CookieJar> {
        Some(&self.cookies)
    }
}

impl fmt::Debug for ReplayFetcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReplayFetcher")
            .field("dir", &self.dir)
            .field("entries", &self.entries.len())
            .field("patterns", &self.patterns)
            .field("cookies", &self.cookies)
            .finish()
    }
}
