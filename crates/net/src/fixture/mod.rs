//! Fixture directories: recorded responses for offline tests.
//!
//! [`ReplayFetcher`] answers requests from a fixture directory.
//! [`RecordingFetcher`] writes one. [`ExtendingFetcher`] adds the responses
//! that a fixture does not have. Python tools (fixture capture with
//! Playwright) read and write the same format, so this documentation is the
//! format specification. See also ADR 0005 (testing strategy).
//!
//! # Directory layout
//!
//! - `manifest.json`: the list of recorded responses.
//! - `files/`: the response bodies, one file per distinct body.
//!
//! # Manifest format, version 1
//!
//! ```json
//! {
//!   "version": 1,
//!   "entries": [
//!     {
//!       "method": "GET",
//!       "url": "https://senko.net/",
//!       "status": 200,
//!       "headers": [
//!         [
//!           "content-type",
//!           "text/html; charset=utf-8"
//!         ]
//!       ],
//!       "body": "files/3f2a9c0d1e4b5a6c.html"
//!     }
//!   ]
//! }
//! ```
//!
//! The top-level object has two fields:
//!
//! - `version` (integer): `1`. Readers reject other versions.
//! - `entries` (array): one object per recorded request.
//!
//! Each entry has these fields. All are required.
//!
//! - `method` (string): the request method in uppercase: `GET` or `POST`.
//! - `url` (string): the request URL, serialized by the WHATWG URL
//!   serializer, without the fragment (no `#` part).
//! - `status` (integer): the HTTP status code.
//! - `headers` (array of two-element string arrays): response headers as
//!   `[name, value]` pairs, in order. Names are in lowercase. Writers store
//!   only `content-type` and `location`. Readers accept any header.
//! - `body` (string): the path of the body file, relative to the fixture
//!   directory, with `/` as the separator. It must not be absolute and must
//!   not contain `..`. An empty body also has a file.
//!
//! Readers ignore unknown fields. [`RecordingFetcher`] drops them when it
//! rewrites the manifest.
//!
//! # Matching
//!
//! A request matches an entry if the serialized request URL without its
//! fragment is equal to `url` (exact string comparison) and the request
//! method is equal to `method`. The request body is not compared. Replayed
//! responses keep the fragment of the request URL in
//! [`Response::url`](crate::Response::url).
//!
//! # Body files
//!
//! The file name is the first 16 hexadecimal digits (lowercase) of the
//! SHA-256 hash of the body, a dot, and an extension. The extension comes
//! from the essence of the response content type (see
//! [`Response::content_type`](crate::Response::content_type)):
//!
//! | Content type essence                                        | Extension |
//! |-------------------------------------------------------------|-----------|
//! | `text/html`                                                 | `html`    |
//! | `application/xhtml+xml`                                     | `xhtml`   |
//! | `text/css`                                                  | `css`     |
//! | `text/javascript`, `application/javascript`, `application/x-javascript` | `js` |
//! | `application/json`                                          | `json`    |
//! | `text/plain`                                                | `txt`     |
//! | `image/png`                                                 | `png`     |
//! | `image/jpeg`                                                | `jpg`     |
//! | `image/gif`                                                 | `gif`     |
//! | `image/webp`                                                | `webp`    |
//! | `image/svg+xml`                                             | `svg`     |
//! | `image/x-icon`, `image/vnd.microsoft.icon`                  | `ico`     |
//! | `image/bmp`                                                 | `bmp`     |
//! | `image/avif`                                                | `avif`    |
//! | `font/woff`, `application/font-woff`, `application/x-font-woff` | `woff` |
//! | `font/woff2`, `application/font-woff2`                      | `woff2`   |
//! | `font/ttf`, `application/x-font-ttf`                        | `ttf`     |
//! | `font/otf`, `application/x-font-otf`                        | `otf`     |
//! | anything else, or no content type                           | `bin`     |
//!
//! Entries with identical bodies and the same extension share one file.
//! Readers do not depend on the naming scheme; they only follow `body`.
//!
//! # Writing rules
//!
//! These rules keep diffs small and stable:
//!
//! - At most one entry per (`url`, `method`). A new response replaces the
//!   old entry.
//! - Entries are sorted by `url`, then by `method`, comparing strings by
//!   Unicode code points (byte-wise comparison of UTF-8 gives the same
//!   order).
//! - Entry fields appear in the order shown above.
//! - The JSON is pretty-printed with an indent of 2 spaces, one array
//!   element per line, `": "` between key and value, and non-ASCII
//!   characters written as UTF-8 (not escaped). The file ends with a
//!   newline. In Python:
//!   `json.dumps(manifest, indent=2, ensure_ascii=False) + "\n"`.
//! - Files are written atomically: write a temporary file in the same
//!   directory, then rename it to the final name.

mod extend;
mod record;
mod replay;

use std::fmt::{self, Write as _};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

pub use extend::ExtendingFetcher;
pub use record::RecordingFetcher;
pub use replay::ReplayFetcher;

use crate::error::NetError;
use crate::file_types::extension_for_mime_type;
use crate::response::ContentType;

/// The name of the manifest file in a fixture directory.
pub(crate) const MANIFEST_FILE: &str = "manifest.json";

/// The name of the directory for body files in a fixture directory.
pub(crate) const FILES_DIR: &str = "files";

/// The manifest format version that this crate reads and writes.
pub(crate) const MANIFEST_VERSION: u32 = 1;

/// The response headers that [`RecordingFetcher`] stores.
pub(crate) const STORED_HEADERS: [&str; 2] = ["content-type", "location"];

/// One manifest entry, as stored in the JSON file. The field order is the
/// order in the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub(crate) method: String,
    pub(crate) url: String,
    pub(crate) status: u16,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: String,
}

impl Entry {
    /// The key that identifies an entry. Sorting by it gives the file order.
    pub(crate) fn key(&self) -> (String, String) {
        (self.url.clone(), self.method.clone())
    }
}

#[derive(Deserialize)]
struct ManifestVersion {
    version: u32,
}

#[derive(Deserialize)]
struct ManifestIn {
    entries: Vec<Entry>,
}

#[derive(Serialize)]
struct ManifestOut<'a> {
    version: u32,
    entries: Vec<&'a Entry>,
}

/// Returns the `url` value of a manifest entry for a request URL.
pub(crate) fn entry_url(url: &Url) -> String {
    let mut url = url.clone();
    url.set_fragment(None);
    url.into()
}

/// Reads and validates `manifest.json` in `dir`.
pub(crate) fn read_manifest(dir: &Path) -> Result<Vec<Entry>, NetError> {
    let path = dir.join(MANIFEST_FILE);
    let text = fs::read_to_string(&path).map_err(|error| fixture_error(&path, &error))?;
    let version: ManifestVersion =
        serde_json::from_str(&text).map_err(|error| fixture_error(&path, &error))?;
    if version.version != MANIFEST_VERSION {
        return Err(fixture_error(
            &path,
            &format_args!(
                "manifest version {} is not supported (expected {MANIFEST_VERSION})",
                version.version
            ),
        ));
    }
    let manifest: ManifestIn =
        serde_json::from_str(&text).map_err(|error| fixture_error(&path, &error))?;
    Ok(manifest.entries)
}

/// Writes `manifest.json` in `dir` atomically, with entries sorted by
/// (url, method).
pub(crate) fn write_manifest<'a>(
    dir: &Path,
    entries: impl IntoIterator<Item = &'a Entry>,
) -> Result<(), NetError> {
    let mut entries: Vec<&Entry> = entries.into_iter().collect();
    entries.sort_by(|a, b| (&a.url, &a.method).cmp(&(&b.url, &b.method)));
    let manifest = ManifestOut {
        version: MANIFEST_VERSION,
        entries,
    };
    let path = dir.join(MANIFEST_FILE);
    let mut json =
        serde_json::to_string_pretty(&manifest).map_err(|error| fixture_error(&path, &error))?;
    json.push('\n');
    write_atomic(&path, json.as_bytes()).map_err(|error| fixture_error(&path, &error))
}

/// Returns the body file name: 16 hex digits of the SHA-256 hash and an
/// extension from the content type.
pub(crate) fn body_file_name(body: &[u8], content_type: Option<&ContentType>) -> String {
    let digest = Sha256::digest(body);
    let mut name = String::new();
    for byte in digest.iter().take(8) {
        let _ = write!(name, "{byte:02x}");
    }
    let extension = content_type
        .and_then(|content_type| extension_for_mime_type(&content_type.essence))
        .unwrap_or("bin");
    let _ = write!(name, ".{extension}");
    name
}

/// Resolves the `body` value of an entry against the fixture directory.
/// Rejects absolute paths and paths with `..`.
pub(crate) fn body_path(dir: &Path, body: &str) -> Result<PathBuf, NetError> {
    let relative = Path::new(body);
    let valid = !body.is_empty()
        && relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
    if !valid {
        return Err(fixture_error(
            &dir.join(MANIFEST_FILE),
            &format_args!("invalid body path {body:?}"),
        ));
    }
    Ok(dir.join(relative))
}

/// Writes `data` to a temporary file next to `path`, then renames it to
/// `path`. Readers see either the old or the new file, never a partial one.
pub(crate) fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    let temporary = path.with_file_name(format!(
        ".{}.{}-{}.tmp",
        file_name.to_string_lossy(),
        process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let result = fs::write(&temporary, data).and_then(|()| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Returns a [`NetError::Fixture`] for `path` with `message`.
pub(crate) fn fixture_error(path: &Path, message: &dyn fmt::Display) -> NetError {
    NetError::Fixture {
        path: path.to_path_buf(),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests;
