//! A simulated media server for the media tests (M14 feature 6a): the page
//! of a probe case and its files under `http://main.test/case-N/`, as the
//! probe's case server answers (`tools/swbtools/probe_server.py`): `Range`
//! requests get 206 with `Content-Range`, `Accept-Ranges: bytes`; a case
//! can ignore `Range` with or without claiming ranges; `__log` answers the
//! requests since the page loaded (path, `Range` header, status and bytes
//! sent). The fetcher answers whole responses (a body in memory); the media
//! byte source reads them as streams.

// Test helpers unwrap too.
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use swb_engine::{ClockMode, Page, PageClock, Size, Url};
use swb_net::{Fetcher, Headers, NetError, Request, Response, StreamResponse};

use super::{new_page, repo_root};

/// The folder of the page of a case (`http://main.test/case-1/`).
pub(crate) const FOLDER: &str = "case-1";
/// 320 x 240, 25 fps, 2 s, H.264 and AAC.
pub(crate) const MP4: &str = "fixtures/media/mp4-h264-high-aac.mp4";
/// The viewport of the media pages.
pub(crate) const VIEW: Size = Size::new(800.0, 600.0);
/// Script helpers of the media pages: `sleep(ms)` and `once(m, name)`, which
/// gives the event's name or `timeout` after 8 s.
pub(crate) const JS_HELPERS: &str = "<script>\
    const sleep = ms => new Promise(r => setTimeout(r, ms));\
    const once = (m, n, ms = 8000) => new Promise(r => { const to = setTimeout(() => r('timeout'), ms); m.addEventListener(n, () => { clearTimeout(to); r(n); }, {once: true}); });\
    </script>";

/// A page of `site` (the folder [`FOLDER`]) in the viewport `view` with the
/// clock `mode`, loaded (30 s). `configure` runs before the navigation
/// (JavaScript, `isolate_media`, `set_paints_frames`, ...).
pub(crate) fn open_page(
    site: MediaSite,
    view: Size,
    mode: ClockMode,
    configure: impl FnOnce(&mut Page),
) -> (Page, Arc<MediaSite>) {
    let site = Arc::new(site);
    let mut page = new_page(Arc::clone(&site) as Arc<dyn Fetcher>, 2, view);
    configure(&mut page);
    page.set_clock(PageClock::new(mode, None));
    page.navigate(Url::parse(&MediaSite::page_url(FOLDER)).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(30)));
    (page, site)
}

/// A page in real time with JavaScript that paints frames, as the GUI's
/// page does.
pub(crate) fn real_page(site: MediaSite) -> Page {
    open_page(site, VIEW, ClockMode::Real, |page| {
        page.set_javascript(true);
        page.set_paints_frames(true);
    })
    .0
}

/// How a file answers `Range`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RangeMode {
    /// 206 and `Accept-Ranges: bytes`.
    Honor,
    /// 200, no `Accept-Ranges` (a server that cannot seek).
    Ignore,
    /// 200 with `Accept-Ranges: bytes` (a server that claims ranges).
    ClaimButIgnore,
}

/// The text of a `swbtools probe --json` output with the `NaN` of a number
/// (Python writes it) made `null`, so that it is JSON. Text in strings stays.
pub(crate) fn chromium_json(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let (mut in_string, mut escaped) = (false, false);
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
        } else if let Some(after) = rest.strip_prefix("NaN") {
            out.push_str("null");
            rest = after;
            continue;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// One file of a case.
#[derive(Clone)]
pub(crate) struct File {
    pub(crate) body: Arc<Vec<u8>>,
    pub(crate) content_type: String,
    pub(crate) status: u16,
    pub(crate) headers: Vec<(String, String)>,
    /// A `resources` entry of a probe case: the probe's server answers it
    /// as a plain file (200 with the whole body, no `Range` logic).
    pub(crate) plain: bool,
}

/// An entry of `__log`.
#[derive(Clone, Debug)]
pub(crate) struct Logged {
    pub(crate) path: String,
    pub(crate) range: Option<String>,
    pub(crate) status: u16,
    pub(crate) sent: usize,
}

/// The server of one case.
pub(crate) struct MediaSite {
    pub(crate) folder: String,
    pub(crate) html: String,
    pub(crate) files: HashMap<String, File>,
    pub(crate) mode: RangeMode,
    pub(crate) log: Mutex<Vec<Logged>>,
    /// A slow link for the media byte source (`fetch_stream`): each read of
    /// a body gives at most `.0` bytes, and every read but the first waits
    /// `.1` before it.
    pub(crate) slow: Option<(usize, Duration)>,
}

/// A body that arrives in pieces (see [`MediaSite::slow`]).
struct SlowBody {
    data: std::io::Cursor<Vec<u8>>,
    chunk: usize,
    pause: Duration,
    first: bool,
}

impl Read for SlowBody {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if !std::mem::replace(&mut self.first, false) {
            std::thread::sleep(self.pause);
        }
        let n = buf.len().min(self.chunk);
        self.data.read(&mut buf[..n])
    }
}

/// The type of a file by its extension, as the probe's case server guesses it.
fn content_type_of(name: &str) -> &'static str {
    match name.rsplit('.').next() {
        Some("mp4" | "m4a") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mp3") => "audio/mpeg",
        Some("ogg" | "oga" | "opus") => "audio/ogg",
        Some("wav") => "audio/x-wav",
        Some("flac") => "audio/flac",
        Some("aac") => "audio/aac",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    }
}

impl MediaSite {
    /// The URL of the page of a case.
    pub(crate) fn page_url(folder: &str) -> String {
        format!("http://main.test/{folder}/{folder}.html")
    }

    /// A server for `html` with no files.
    pub(crate) fn new(folder: &str, html: &str) -> MediaSite {
        MediaSite {
            folder: folder.to_owned(),
            html: html.to_owned(),
            files: HashMap::new(),
            mode: RangeMode::Honor,
            log: Mutex::new(Vec::new()),
            slow: None,
        }
    }

    /// Serves the file at `path` (relative to the repository) as `name`.
    pub(crate) fn add_file(&mut self, name: &str, path: &str) {
        let body = std::fs::read(Path::new(&repo_root()).join(path))
            .unwrap_or_else(|e| panic!("{path}: {e}"));
        self.add_bytes(name, body);
    }

    /// Serves `body` as `name`.
    pub(crate) fn add_bytes(&mut self, name: &str, body: Vec<u8>) {
        self.files.insert(
            name.to_owned(),
            File {
                body: Arc::new(body),
                content_type: content_type_of(name).to_owned(),
                status: 200,
                headers: Vec::new(),
                plain: false,
            },
        );
    }

    /// Serves a `resources` entry of a probe case (`text`, `content_type`,
    /// `status`, `headers`).
    pub(crate) fn add_resource(&mut self, name: &str, resource: &Value) {
        let text = resource["text"].as_str().unwrap_or_default();
        let headers = resource["headers"]
            .as_object()
            .map(|h| {
                h.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
                    .collect()
            })
            .unwrap_or_default();
        self.files.insert(
            name.to_owned(),
            File {
                body: Arc::new(text.as_bytes().to_vec()),
                content_type: resource["content_type"]
                    .as_str()
                    .unwrap_or_else(|| content_type_of(name))
                    .to_owned(),
                status: resource["status"].as_u64().map_or(200, |s| s as u16),
                headers,
                plain: true,
            },
        );
    }

    /// The requests that the media files got, in the form of the probe's
    /// `log()`: `path range -> status sent`.
    pub(crate) fn log_lines(&self) -> Vec<String> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .map(|e| {
                format!(
                    "{} {} -> {} {}",
                    e.path,
                    e.range.as_deref().unwrap_or("-"),
                    e.status,
                    e.sent
                )
            })
            .collect()
    }

    fn reply(
        request: &Request,
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    ) -> Response {
        let mut all = Headers::default();
        for (name, value) in headers {
            all.set(&name, value);
        }
        all.set("content-length", body.len().to_string());
        Response {
            url: request.url.clone(),
            status,
            headers: all,
            body,
            redirected: false,
        }
    }

    fn serve_file(&self, request: &Request, name: &str, file: &File) -> Response {
        let range = request.headers.get("range").map(str::to_owned);
        let mut headers = vec![("content-type".to_owned(), file.content_type.clone())];
        headers.extend(file.headers.iter().cloned());
        let body = &file.body;
        let size = body.len();
        let (status, out) = if file.status != 200 || file.plain {
            (file.status, file.body.to_vec())
        } else if self.mode == RangeMode::Honor {
            headers.push(("accept-ranges".to_owned(), "bytes".to_owned()));
            match range
                .as_deref()
                .map_or(Spec::Ignored, |r| parse_range(r, size))
            {
                Spec::Bytes(start, end) => {
                    headers.push((
                        "content-range".to_owned(),
                        format!("bytes {start}-{end}/{size}"),
                    ));
                    (206, body[start..=end].to_vec())
                }
                Spec::Unsatisfiable => {
                    headers.push(("content-range".to_owned(), format!("bytes */{size}")));
                    (416, Vec::new())
                }
                Spec::Ignored => (200, body.to_vec()),
            }
        } else {
            if self.mode == RangeMode::ClaimButIgnore {
                headers.push(("accept-ranges".to_owned(), "bytes".to_owned()));
            }
            (200, body.to_vec())
        };
        self.log.lock().unwrap().push(Logged {
            path: name.to_owned(),
            range,
            status,
            sent: out.len(),
        });
        MediaSite::reply(request, status, headers, out)
    }
}

/// What a `Range` header asks for.
enum Spec {
    /// A satisfiable single `bytes` range: the first and the last byte.
    Bytes(usize, usize),
    /// A range past the end.
    Unsatisfiable,
    /// No header, or one that a server ignores (RFC 9110 section 14).
    Ignored,
}

fn parse_range(header: &str, size: usize) -> Spec {
    parse_bytes(header, size).unwrap_or(Spec::Ignored)
}

fn parse_bytes(header: &str, size: usize) -> Option<Spec> {
    let spec = header.strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (first, last) = spec.trim().split_once('-')?;
    if first.is_empty() {
        let count: usize = last.parse().ok()?;
        if count == 0 || size == 0 {
            return Some(Spec::Unsatisfiable);
        }
        return Some(Spec::Bytes(size.saturating_sub(count), size - 1));
    }
    let start: usize = first.parse().ok()?;
    let end = if last.is_empty() {
        size.saturating_sub(1)
    } else {
        let end: usize = last.parse().ok()?;
        if end < start {
            return None;
        }
        end.min(size.saturating_sub(1))
    };
    if start >= size {
        return Some(Spec::Unsatisfiable);
    }
    Some(Spec::Bytes(start, end))
}

impl Fetcher for MediaSite {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let prefix = format!("/{}/", self.folder);
        let path = request.url.path().to_owned();
        let Some(name) = path.strip_prefix(&prefix) else {
            return Err(NetError::NotInFixture(request.url.clone()));
        };
        if name == format!("{}.html", self.folder) {
            return Ok(MediaSite::reply(
                request,
                200,
                vec![("content-type".to_owned(), "text/html".to_owned())],
                self.html.as_bytes().to_vec(),
            ));
        }
        if name == "__log" {
            let entries: Vec<Value> = self
                .log
                .lock()
                .unwrap()
                .iter()
                .map(|e| {
                    json!({
                        "path": e.path,
                        "headers": { "range": e.range },
                        "status": e.status,
                        "sent": e.sent,
                    })
                })
                .collect();
            return Ok(MediaSite::reply(
                request,
                200,
                vec![("content-type".to_owned(), "application/json".to_owned())],
                Value::Array(entries).to_string().into_bytes(),
            ));
        }
        if let Some(file) = self.files.get(name) {
            return Ok(self.serve_file(request, name, file));
        }
        self.log.lock().unwrap().push(Logged {
            path: name.to_owned(),
            range: request.headers.get("range").map(str::to_owned),
            status: 404,
            sent: 0,
        });
        Ok(MediaSite::reply(request, 404, Vec::new(), Vec::new()))
    }

    fn fetch_stream(&self, request: &Request) -> Result<StreamResponse, NetError> {
        let mut response = self.fetch(request)?;
        Ok(match self.slow {
            Some((chunk, pause)) => {
                let body = std::mem::take(&mut response.body);
                StreamResponse {
                    head: response,
                    body: Box::new(SlowBody {
                        data: std::io::Cursor::new(body),
                        chunk: chunk.max(1),
                        pause,
                        first: true,
                    }),
                }
            }
            None => StreamResponse::buffered(response),
        })
    }
}
