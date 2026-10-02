//! [`RecordingFetcher`]: stores responses in a fixture directory.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use log::debug;

use super::{
    Entry, FILES_DIR, MANIFEST_FILE, STORED_HEADERS, body_file_name, entry_url, fixture_error,
    read_manifest, write_atomic, write_manifest,
};
use crate::builtin;
use crate::error::NetError;
use crate::fetch::Fetcher;
use crate::request::Request;
use crate::response::Response;

/// Forwards requests to another fetcher and records each response in a
/// fixture directory.
///
/// Redirect responses are recorded with their `Location` header, so a
/// replay follows the same redirects. Errors from the inner fetcher are not
/// recorded. `data:` and `about:` URLs are not recorded.
///
/// After each response, the manifest is rewritten (atomically), so the
/// directory is a valid fixture at any time. Several threads can use one
/// recorder. Several recorders (or processes) must not write to the same
/// directory at the same time. Body files of replaced entries stay in
/// `files/`.
///
/// The format is described in the [module documentation](super).
pub struct RecordingFetcher<F: Fetcher> {
    inner: F,
    dir: PathBuf,
    /// Keyed by (url, method), so iteration gives the file order.
    entries: Mutex<BTreeMap<(String, String), Entry>>,
}

impl<F: Fetcher> RecordingFetcher<F> {
    /// Creates a recorder that writes to `dir`. Creates the directory if
    /// necessary. If the directory has a manifest, new entries are merged
    /// into it; otherwise an empty manifest is written.
    pub fn new(inner: F, dir: impl Into<PathBuf>) -> Result<Self, NetError> {
        let dir = dir.into();
        let files = dir.join(FILES_DIR);
        fs::create_dir_all(&files).map_err(|error| fixture_error(&files, &error))?;
        let mut entries = BTreeMap::new();
        if dir.join(MANIFEST_FILE).exists() {
            for entry in read_manifest(&dir)? {
                entries.entry(entry.key()).or_insert(entry);
            }
        } else {
            write_manifest(&dir, [])?;
        }
        Ok(RecordingFetcher {
            inner,
            dir,
            entries: Mutex::new(entries),
        })
    }

    /// Returns the inner fetcher.
    pub fn inner(&self) -> &F {
        &self.inner
    }

    /// Returns the fixture directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn record(&self, request: &Request, response: &Response) -> Result<(), NetError> {
        let file_name = body_file_name(&response.body, response.content_type().as_ref());
        let path = self.dir.join(FILES_DIR).join(&file_name);
        if !path.exists() {
            write_atomic(&path, &response.body).map_err(|error| fixture_error(&path, &error))?;
        }
        let headers = STORED_HEADERS
            .iter()
            .flat_map(|name| {
                response
                    .headers
                    .get_all(name)
                    .map(|value| ((*name).to_string(), value.to_string()))
            })
            .collect();
        let entry = Entry {
            method: request.method.as_str().to_string(),
            url: entry_url(&request.url),
            status: response.status,
            headers,
            body: format!("{FILES_DIR}/{file_name}"),
        };
        debug!("record {} {} -> {}", entry.method, entry.url, entry.body);
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries.insert(entry.key(), entry);
        // Written under the lock, so that an older state never replaces a
        // newer one.
        write_manifest(&self.dir, entries.values())
    }
}

impl<F: Fetcher> Fetcher for RecordingFetcher<F> {
    /// Fetches through the inner fetcher and records the response. A failure
    /// to write the fixture is returned as an error.
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let response = self.inner.fetch(request)?;
        if !builtin::handles(&request.url) {
            self.record(request, &response)?;
        }
        Ok(response)
    }
}

impl<F: Fetcher + fmt::Debug> fmt::Debug for RecordingFetcher<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecordingFetcher")
            .field("inner", &self.inner)
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}
