//! [`ReplayFetcher`]: answers requests from a fixture directory.

use std::collections::HashMap;
use std::collections::hash_map;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use log::warn;

use super::{
    body_path, entry_url, fixture_error, natural_cmp, pattern_matches, read_manifest,
    read_replay_patterns,
};
use crate::builtin;
use crate::cookies::CookieJar;
use crate::error::NetError;
use crate::fetch::Fetcher;
use crate::headers::Headers;
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
    /// Keyed by (url, method).
    entries: HashMap<(String, String), ReplayEntry>,
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
    body: Arc<Vec<u8>>,
}

impl ReplayFetcher {
    /// Loads the fixture in `dir`. Fails if the manifest is missing or
    /// invalid, or if a body file cannot be read. If the manifest has
    /// several entries for the same URL and method, the first one is used.
    pub fn load(dir: impl AsRef<Path>) -> Result<Self, NetError> {
        let dir = dir.as_ref();
        let mut bodies: HashMap<String, Arc<Vec<u8>>> = HashMap::new();
        let mut entries = HashMap::new();
        for entry in read_manifest(dir)? {
            let body = if let Some(body) = bodies.get(&entry.body) {
                Arc::clone(body)
            } else {
                let path = body_path(dir, &entry.body)?;
                let body = fs::read(&path).map_err(|error| fixture_error(&path, &error))?;
                let body = Arc::new(body);
                bodies.insert(entry.body.clone(), Arc::clone(&body));
                body
            };
            match entries.entry((entry.url, entry.method)) {
                hash_map::Entry::Occupied(occupied) => {
                    let (url, method) = occupied.key();
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
    /// the same pattern as the request URL, for the same method.
    fn lookup(&self, key: &(String, String)) -> Option<&ReplayEntry> {
        if let Some(entry) = self.entries.get(key) {
            return Some(entry);
        }
        let (url, method) = key;
        let pattern = self.patterns.iter().find(|p| pattern_matches(p, url))?;
        self.entries
            .iter()
            .filter(|((u, m), _)| m == method && pattern_matches(pattern, u))
            .max_by(|((a, _), _), ((b, _), _)| natural_cmp(a, b))
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

impl Fetcher for ReplayFetcher {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        if let Some(result) = builtin::fetch(request) {
            return result;
        }
        if is_preflight(request) {
            return Ok(preflight_response(request));
        }
        let key = (entry_url(&request.url), request.method.as_str().to_string());
        let Some(entry) = self.lookup(&key) else {
            warn!(
                "fixture {}: no entry for {} {}",
                self.dir.display(),
                request.method,
                key.0
            );
            return Err(NetError::NotInFixture(request.url.clone()));
        };
        Ok(Response {
            url: request.url.clone(),
            status: entry.status,
            headers: served_headers(request, entry),
            body: entry.body.as_ref().clone(),
            redirected: false,
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
