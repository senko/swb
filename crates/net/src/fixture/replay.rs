//! [`ReplayFetcher`]: answers requests from a fixture directory.

use std::collections::HashMap;
use std::collections::hash_map;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use log::warn;

use super::{body_path, entry_url, fixture_error, read_manifest};
use crate::builtin;
use crate::error::NetError;
use crate::fetch::Fetcher;
use crate::headers::Headers;
use crate::request::Request;
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

impl Fetcher for ReplayFetcher {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        if let Some(result) = builtin::fetch(request) {
            return result;
        }
        let key = (entry_url(&request.url), request.method.as_str().to_string());
        let Some(entry) = self.entries.get(&key) else {
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
            headers: entry.headers.clone(),
            body: entry.body.as_ref().clone(),
        })
    }
}

impl fmt::Debug for ReplayFetcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReplayFetcher")
            .field("dir", &self.dir)
            .field("entries", &self.entries.len())
            .finish()
    }
}
