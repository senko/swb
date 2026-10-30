//! The Chromium probes of feature 10b that need a network, run in the
//! engine against a simulated server (M8 feature 10b, ADR 0028 section 12):
//! `tools/probes/web-xhr.json` (`XMLHttpRequest`: states, event order,
//! `upload`, `abort`, `timeout`, response types, header lists, CORS),
//! `web-image-events.json` (`load` and `error` of images and style sheets,
//! `complete`, natural sizes, `decode()`) and `web-blob-formdata.json`
//! (`FormData` and multipart bodies, `FileReader`, Blob URLs, `sendBeacon`).
//!
//! `Sim` answers as the probe server does (`swbtools/probe.py`): the
//! `resources` of a case under `/case-N/`, the headers `Cache-Control:
//! no-store` and `Content-Length`, the `OPTIONS` answers, a delay per
//! resource, both origins serving the same files, and `__log` (the requests
//! since the page loaded). The values are Chromium's; `DEPARTURES` lists
//! the queries where swb differs on purpose, with the reason.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use swb_engine::{Evaluation, JsonForm, Page, Size, Url};
use swb_net::{Fetcher, Headers, Method, NetError, Request, Response};

mod common;
use common::{finish_loading, new_page};

#[path = "probe_net/blob.rs"]
mod blob_cases;
#[path = "probe_net/images.rs"]
mod image_cases;
#[path = "probe_net/xhr.rs"]
mod xhr_cases;

const MAIN: &str = "http://main.test";
const OTHER: &str = "http://other.test";

/// The answer to `OPTIONS` (`options` of a resource of the probe).
#[derive(Clone, Copy)]
pub(crate) enum Options {
    None,
    #[allow(dead_code, reason = "no 10b case denies a preflight")]
    Deny,
    Permissive,
}

/// A resource of a case.
pub(crate) struct Res {
    pub(crate) path: &'static str,
    pub(crate) text: &'static str,
    pub(crate) status: u16,
    pub(crate) delay_ms: u64,
    pub(crate) content_type: Option<&'static str>,
    pub(crate) headers: &'static [(&'static str, &'static str)],
    pub(crate) options: Options,
}

/// A case of a probe file with the values of Chromium.
pub(crate) struct Case {
    pub(crate) name: &'static str,
    pub(crate) html: &'static str,
    pub(crate) resources: &'static [Res],
    pub(crate) queries: &'static [(&'static str, &'static str)],
}

/// A query where swb's value differs on purpose: (case, query index,
/// swb's value, why).
type Departure = (&'static str, usize, &'static str, &'static str);

/// A request as `__log` lists it.
#[derive(Clone)]
struct Logged {
    method: String,
    path: String,
    server: &'static str,
    origin: Option<String>,
    headers: Vec<(String, String)>,
    body: String,
}

/// The simulated probe server of one case.
struct Sim {
    folder: String,
    html: String,
    resources: &'static [Res],
    log: Mutex<Vec<Logged>>,
    /// The URLs requested (with the query), for the tests of form submissions.
    urls: Mutex<Vec<String>>,
}

impl Sim {
    fn substitute(&self, text: &str) -> String {
        text.replace("$ORIGIN", MAIN)
            .replace("$OTHER", OTHER)
            .replace("$FOLDER", &self.folder)
    }

    fn log_json(&self) -> String {
        let entries: Vec<serde_json::Value> = self
            .log
            .lock()
            .unwrap()
            .iter()
            .map(|e| {
                let mut headers = serde_json::Map::new();
                let mut sorted = e.headers.clone();
                sorted.sort();
                for (name, value) in sorted {
                    headers.insert(name, serde_json::Value::String(value));
                }
                serde_json::json!({
                    "method": e.method,
                    "path": e.path,
                    "server": e.server,
                    "origin": e.origin.clone().unwrap_or_default(),
                    "cookie": "",
                    "headers": headers,
                    "body": e.body,
                })
            })
            .collect();
        serde_json::Value::Array(entries).to_string()
    }
}

/// The request headers that `__log` keeps.
fn logged_header(name: &str) -> bool {
    matches!(name, "accept" | "authorization" | "content-type" | "range")
        || name.starts_with("access-control-request-")
        || name.starts_with("x-")
}

fn reply(url: &Url, status: u16, headers: Vec<(String, String)>, body: Vec<u8>) -> Response {
    Response {
        url: url.clone(),
        status,
        headers: headers.into_iter().collect::<Headers>(),
        body,
        redirected: false,
    }
}

impl Fetcher for Sim {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        if matches!(request.url.scheme(), "data" | "about") {
            return swb_net::NetworkFetcher::new().fetch(request);
        }
        let host = request.url.host_str().unwrap_or("");
        let server = if host == "other.test" {
            "other"
        } else {
            "main"
        };
        let prefix = format!("/{}/", self.folder);
        let path = request.url.path().to_owned();
        let Some(name) = path.strip_prefix(&prefix) else {
            return Err(NetError::NotInFixture(request.url.clone()));
        };
        if name == format!("{}.html", self.folder) {
            let headers = vec![("content-type".to_owned(), "text/html".to_owned())];
            return Ok(reply(
                &request.url,
                200,
                headers,
                self.html.clone().into_bytes(),
            ));
        }
        if name == "__log" {
            let headers = vec![("content-type".to_owned(), "application/json".to_owned())];
            return Ok(reply(
                &request.url,
                200,
                headers,
                self.log_json().into_bytes(),
            ));
        }
        let mut headers: Vec<(String, String)> = request
            .headers
            .iter()
            .filter(|(n, _)| logged_header(&n.to_ascii_lowercase()))
            .map(|(n, v)| (n.to_ascii_lowercase(), v.to_owned()))
            .collect();
        if !headers.iter().any(|(n, _)| n == "accept") {
            headers.push(("accept".to_owned(), "*/*".to_owned()));
        }
        self.urls.lock().unwrap().push(request.url.to_string());
        self.log.lock().unwrap().push(Logged {
            method: request.method.to_string(),
            path: path.clone(),
            server,
            origin: request.origin_header(),
            headers,
            body: String::from_utf8_lossy(
                &request
                    .body
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .copied()
                    .take(200)
                    .collect::<Vec<u8>>(),
            )
            .into_owned(),
        });
        let resource = self.resources.iter().find(|r| r.path == name);
        if request.method == Method::Options {
            return Ok(Self::options_answer(request, resource));
        }
        let Some(resource) = resource else {
            return Ok(reply(
                &request.url,
                404,
                vec![("content-type".to_owned(), "text/html".to_owned())],
                b"<!DOCTYPE HTML><title>Error response</title>".to_vec(),
            ));
        };
        // A real server needs some time even without a delay: a timer of
        // 0 ms runs before the response arrives (Chromium, measured).
        std::thread::sleep(Duration::from_millis(resource.delay_ms.max(30)));
        let mut out: Vec<(String, String)> = resource
            .headers
            .iter()
            .map(|(n, v)| ((*n).to_owned(), self.substitute(v)))
            .collect();
        let has = |out: &[(String, String)], name: &str| {
            out.iter().any(|(n, _)| n.eq_ignore_ascii_case(name))
        };
        if !has(&out, "content-type") {
            let by_extension = match name.rsplit('.').next() {
                Some("txt") => "text/plain",
                Some("json") => "application/json",
                Some("html") => "text/html",
                Some("svg") => "image/svg+xml",
                Some("css") => "text/css",
                Some("png") => "image/png",
                _ => "application/octet-stream",
            };
            out.push((
                "content-type".to_owned(),
                resource.content_type.unwrap_or(by_extension).to_owned(),
            ));
        }
        out.push(("cache-control".to_owned(), "no-store".to_owned()));
        let body = self.substitute(resource.text).into_bytes();
        out.push(("content-length".to_owned(), body.len().to_string()));
        let body = if request.method == Method::Head {
            Vec::new()
        } else {
            body
        };
        Ok(reply(&request.url, resource.status, out, body))
    }
}

impl Sim {
    fn options_answer(request: &Request, resource: Option<&Res>) -> Response {
        let mode = resource.map_or(Options::None, |r| r.options);
        match mode {
            Options::None => reply(&request.url, 404, Vec::new(), Vec::new()),
            Options::Deny => reply(&request.url, 200, Vec::new(), Vec::new()),
            Options::Permissive => {
                let mut headers = vec![
                    (
                        "access-control-allow-origin".to_owned(),
                        request.origin_header().unwrap_or_default(),
                    ),
                    (
                        "access-control-allow-credentials".to_owned(),
                        "true".to_owned(),
                    ),
                ];
                if let Some(method) = request.headers.get("access-control-request-method") {
                    headers.push(("access-control-allow-methods".to_owned(), method.to_owned()));
                }
                if let Some(names) = request.headers.get("access-control-request-headers") {
                    headers.push(("access-control-allow-headers".to_owned(), names.to_owned()));
                }
                reply(&request.url, 204, headers, Vec::new())
            }
        }
    }
}

/// Runs `f` on a thread with the stack of swb's page thread.
fn on_page_thread(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

/// The failures of the cases `cases`: a query whose value differs from
/// Chromium's and is not a departure.
fn failures(cases: &'static [Case], departures: &'static [Departure]) -> Vec<String> {
    let mut out = Vec::new();
    for case in cases {
        let folder = format!("case-{}", case.name);
        let sim = Arc::new(Sim {
            folder: folder.clone(),
            html: case
                .html
                .replace("$ORIGIN", MAIN)
                .replace("$OTHER", OTHER)
                .replace("$FOLDER", &folder),
            resources: case.resources,
            log: Mutex::new(Vec::new()),
            urls: Mutex::new(Vec::new()),
        });
        let mut page = new_page(
            Arc::clone(&sim) as Arc<dyn Fetcher>,
            3,
            Size::new(400.0, 300.0),
        );
        page.set_javascript(true);
        page.navigate(Url::parse(&format!("{MAIN}/{folder}/{folder}.html")).unwrap());
        finish_loading(&mut page, Duration::from_secs(20));
        for (index, (query, expected)) in case.queries.iter().enumerate() {
            let want = departures
                .iter()
                .find(|(c, i, _, _)| *c == case.name && *i == index)
                .map_or(*expected, |d| d.2);
            let got = evaluate(&mut page, &sim, query);
            let want: serde_json::Value = serde_json::from_str(want).unwrap();
            let got: serde_json::Value =
                serde_json::from_str(&got).unwrap_or(serde_json::Value::String(got));
            if got != want {
                out.push(format!(
                    "{} #{index}: {query}\n  expected: {want}\n  actual:   {got}",
                    case.name
                ));
            }
        }
    }
    out
}

fn evaluate(page: &mut Page, _sim: &Sim, query: &str) -> String {
    match page.evaluate(query, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => text,
        Some(Evaluation::Error(text)) => format!("{:?}", format!("error: {text}")),
        None => "\"no runtime\"".to_owned(),
    }
}

/// The departures of the XHR probe.
const XHR_DEPARTURES: &[Departure] = &[
    (
        "response-types",
        5,
        r#"["readystatechange:1:0:undefined/undefined!Event","loadstart:1:0:0/0","readystatechange:2:200:undefined/undefined!Event","readystatechange:3:200:undefined/undefined!Event","progress:3:200:45/45c","readystatechange:4:200:undefined/undefined!Event","load:4:200:45/45c",["[object HTMLDocument]","t",true,"text/html","UTF-8","case-response-types.html"],"loadend:4:200:45/45c"]"#,
        "the document of `responseType` `document` has the URL of the page (swb's parser makes it without a URL)",
    ),
    (
        "response-types",
        6,
        r#"["readystatechange:1:0:undefined/undefined!Event","loadstart:1:0:0/0","readystatechange:2:200:undefined/undefined!Event","readystatechange:3:200:undefined/undefined!Event","progress:3:200:21/21c","readystatechange:4:200:undefined/undefined!Event","load:4:200:21/21c",["[object Null]",null,null],"loadend:4:200:21/21c"]"#,
        "swb has no XML parser: `responseXML` of an XML response is `null`",
    ),
    (
        "response-types",
        10,
        r#"["readystatechange:1:0:undefined/undefined!Event","loadstart:1:0:0/0","readystatechange:2:200:undefined/undefined!Event","readystatechange:3:200:undefined/undefined!Event","progress:3:200:5/5c","readystatechange:4:200:undefined/undefined!Event","load:4:200:5/5c",[3],"loadend:4:200:5/5c"]"#,
        "an odd trailing byte of a UTF-16 text is a replacement character in swb (`encoding_rs`); Chromium drops it",
    ),
    (
        "errors",
        0,
        r#"["InvalidStateError: Failed to execute 'send' on 'XMLHttpRequest': The object's state must be OPENED.","TypeError: Failed to execute 'open' on 'XMLHttpRequest': 2 arguments required, but only 1 present.","SyntaxError: Failed to execute 'open' on 'XMLHttpRequest': 'GE T' is not a valid HTTP method.","SecurityError: Failed to execute 'open' on 'XMLHttpRequest': 'CONNECT' HTTP method is unsupported.","SecurityError: Failed to execute 'open' on 'XMLHttpRequest': 'TRACE' HTTP method is unsupported.","SyntaxError: Failed to execute 'open' on 'XMLHttpRequest': Invalid URL","InvalidAccessError: Failed to execute 'open' on 'XMLHttpRequest': Synchronous requests are not supported.","1","InvalidStateError: Failed to execute 'send' on 'XMLHttpRequest': The object's state must be OPENED.","","undefined","undefined","InvalidStateError: Failed to set the 'withCredentials' property on 'XMLHttpRequest': The value may only be set if the object's state is UNSENT or OPENED.","ok","undefined","","0,0,,,,,,0,false,,true,0,4,1,2,3"]"#,
        "a synchronous `open` throws `InvalidAccessError` (ADR 0028 section 12)",
    ),
    (
        "errors",
        1,
        r#"["onreadystatechange,readyState,timeout,withCredentials,upload,responseURL,status,statusText,responseType,response,responseText,responseXML,UNSENT,OPENED,HEADERS_RECEIVED,LOADING,DONE,abort,getAllResponseHeaders,getResponseHeader,open,overrideMimeType,send,setRequestHeader,constructor","onloadstart,onprogress,onabort,onerror,onload,ontimeout,onloadend,constructor","constructor","lengthComputable,loaded,total,constructor",true,true,true,"[object XMLHttpRequest]","[object XMLHttpRequestUpload]",2,"undefined"]"#,
        "the generator writes the members in IDL order: `responseXML` comes after `responseText`; Chromium lists it last and has `setAttributionReporting` and `setPrivateToken`",
    ),
];

/// The departures of the image probe.
const IMAGE_DEPARTURES: &[Departure] = &[
    (
        "new-image",
        10,
        r#"["alt,src,srcset,sizes,crossOrigin,useMap,isMap,width,height,naturalWidth,naturalHeight,complete,currentSrc,referrerPolicy,decoding,loading,fetchPriority,decode,constructor"]"#,
        "the generator sorts the members of the IDL and lacks the non-standard members of Chromium",
    ),
    (
        "stylesheets",
        0,
        r#"["appended:false","l.load:,,,:Event:false:false:true","sheet:false"]"#,
        "`link.sheet` is `null` in swb for a loaded style sheet (backlog)",
    ),
    (
        "stylesheets",
        1,
        r#"["l.error:,,,:Event:false:false:true","sheet:false"]"#,
        "`link.sheet` is `null` in swb for a loaded style sheet (backlog)",
    ),
    (
        "stylesheets",
        2,
        r#"["l.load:,,,:Event:false:false:true","sheet:false"]"#,
        "`link.sheet` is `null` in swb for a loaded style sheet (backlog)",
    ),
    (
        "stylesheets",
        3,
        r#"["l.load:,,,:Event:false:false:true","sheet:undefined"]"#,
        "`link.sheet` is `null` in swb for a loaded style sheet (backlog)",
    ),
    (
        "stylesheets",
        5,
        r#"["no-rel","l.load:,,,:Event:false:false:true","sheet:false"]"#,
        "`link.sheet` is `null` in swb for a loaded style sheet (backlog)",
    ),
    (
        "stylesheets",
        7,
        r#"["x"]"#,
        "swb does not support `rel=preload`: no `load` event",
    ),
    (
        "parser-sheets",
        0,
        r#"["s.load","bad.error","script","dcl","window.load"]"#,
        "a style sheet that blocks a script fires `load` before the script that waited for it runs (Chromium: after)",
    ),
];

/// The departures of the `Blob` and `FormData` probe.
const BLOB_DEPARTURES: &[Departure] = &[
    (
        "blob",
        1,
        r#"["size,type,arrayBuffer,bytes,slice,text,constructor","name,lastModified,webkitRelativePath,constructor",true,"readyState,result,error,onloadstart,onprogress,onload,onabort,onerror,onloadend,EMPTY,LOADING,DONE,abort,readAsArrayBuffer,readAsBinaryString,readAsDataURL,readAsText,constructor","append,delete,get,getAll,has,set,entries,forEach,keys,values,constructor","length,name,prototype,canParse,createObjectURL,parse,revokeObjectURL"]"#,
        "the generator sorts the operations; swb has no `Blob.stream()` (no streams) and no `File.lastModifiedDate`",
    ),
    (
        "blob-urls",
        0,
        r#"["blob:O/UUID",true,true,200,true,"text/x","8","url-body",null,"TypeError: Failed to fetch","TypeError: Failed to execute 'createObjectURL' on 'URL': parameter 1 is not of type 'Blob'.","TypeError: Failed to execute 'createObjectURL' on 'URL': parameter 1 is not of type 'Blob'."]"#,
        "Chromium's `createObjectURL` is an overload with `MediaSource`; swb's has only `Blob` and words the error of the conversion",
    ),
    (
        "blob-urls",
        2,
        r#"["error","error"]"#,
        "the image loader of the engine has no `blob:` scheme: an `img` with a Blob URL fails (backlog)",
    ),
];

#[test]
fn xhr_probe_cases_match_chromium() {
    on_page_thread(|| {
        let failures = failures(xhr_cases::CASES, XHR_DEPARTURES);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

#[test]
fn image_probe_cases_match_chromium() {
    on_page_thread(|| {
        let failures = failures(image_cases::CASES, IMAGE_DEPARTURES);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

#[test]
fn blob_probe_cases_match_chromium() {
    on_page_thread(|| {
        let failures = failures(blob_cases::CASES, BLOB_DEPARTURES);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

/// A page with a form and a `formdata` listener that adds an entry.
#[test]
fn a_formdata_listener_changes_the_submission() {
    on_page_thread(|| {
        let sim = Arc::new(Sim {
            folder: "case-form".to_owned(),
            html: concat!(
                "<!DOCTYPE html><form id=f action=echo.txt><input name=a value=1><input name=b value=2></form>",
                "<script>document.getElementById('f').addEventListener('formdata', e => { e.formData.append('added', 'yes'); e.formData.delete('b'); });",
                "document.getElementById('f').requestSubmit();</script>"
            )
            .to_owned(),
            resources: &[Res {
                path: "echo.txt",
                text: "<!DOCTYPE html><p>echo</p>",
                status: 200,
                delay_ms: 0,
                content_type: Some("text/html"),
                headers: &[],
                options: Options::None,
            }],
            log: Mutex::new(Vec::new()),
            urls: Mutex::new(Vec::new()),
        });
        let mut page = new_page(
            Arc::clone(&sim) as Arc<dyn Fetcher>,
            3,
            Size::new(400.0, 300.0),
        );
        page.set_javascript(true);
        page.navigate(Url::parse(&format!("{MAIN}/case-form/case-form.html")).unwrap());
        finish_loading(&mut page, Duration::from_secs(20));
        let urls = sim.urls.lock().unwrap().clone();
        assert_eq!(
            urls,
            vec![format!("{MAIN}/case-form/echo.txt?a=1&added=yes")]
        );
    });
}
