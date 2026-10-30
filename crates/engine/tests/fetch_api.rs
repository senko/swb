//! `fetch()` in the page (M8 feature 10a, ADR 0028 section 12): CORS,
//! credentials modes, preflights, redirects, abort, the limits, with the
//! values of the Chromium probe `tools/probes/web-fetch-cors.json` (the
//! server of the probe is simulated by `Server`, which answers as the
//! probe server does). The pages run on a thread with a 16 MiB stack, as in
//! swb.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use swb_engine::{Evaluation, JsonForm, Page, Size, Url};
use swb_net::{Fetcher, Headers, Method, NetError, Request, Response};

mod common;
use common::{finish_loading, new_page};

const APP: &str = "https://app.test";
const API: &str = "https://api.test";

/// What a route answers.
#[derive(Clone, Default)]
struct Route {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
    /// The answer to `OPTIONS`: `None` is a 404 without headers.
    options: Option<(u16, Vec<(String, String)>)>,
    /// A delay in milliseconds before the answer.
    delay: u64,
}

fn route(status: u16, headers: &[(&str, &str)], body: &str) -> Route {
    Route {
        status,
        headers: pairs(headers),
        body: body.to_owned(),
        ..Route::default()
    }
}

fn pairs(headers: &[(&str, &str)]) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
        .collect()
}

/// A request as the server saw it.
#[derive(Clone, Debug)]
struct Seen {
    method: String,
    url: String,
    origin: Option<String>,
    credentials: bool,
    headers: Vec<(String, String)>,
    body: String,
}

/// Routes by URL (without the query), and the log of the requests.
#[derive(Default)]
struct Server {
    routes: HashMap<String, Route>,
    log: Mutex<Vec<Seen>>,
}

impl Server {
    fn route(mut self, url: &str, route: Route) -> Server {
        self.routes.insert(url.to_owned(), route);
        self
    }

    fn seen(&self) -> Vec<Seen> {
        self.log.lock().unwrap().clone()
    }
}

impl Fetcher for Server {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        if let Some(result) = fixture_builtin(request) {
            return result;
        }
        let mut url = request.url.clone();
        url.set_fragment(None);
        url.set_query(None);
        let found = self.routes.get(url.as_str());
        if request.url.path() != "/page.html" {
            self.log.lock().unwrap().push(Seen {
                method: request.method.to_string(),
                url: request.url.to_string(),
                origin: request.origin_header(),
                credentials: request.sends_credentials(),
                headers: request
                    .headers
                    .iter()
                    .map(|(n, v)| (n.to_owned(), v.to_owned()))
                    .collect(),
                body: String::from_utf8_lossy(request.body.as_deref().unwrap_or_default())
                    .into_owned(),
            });
        }
        let page = request.url.path() == "/page.html";
        let route = if page {
            Some(route(
                200,
                &[("content-type", "text/html")],
                "<!DOCTYPE html><p>x</p>",
            ))
        } else {
            found.cloned()
        };
        let Some(route) = route else {
            return Err(NetError::NotInFixture(request.url.clone()));
        };
        if route.delay > 0 {
            std::thread::sleep(Duration::from_millis(route.delay));
        }
        let (status, headers) = if request.method == Method::Options {
            match &route.options {
                Some((status, headers)) => (*status, headers.clone()),
                None => (404, Vec::new()),
            }
        } else {
            (route.status, route.headers.clone())
        };
        let body = if request.method == Method::Options || request.method == Method::Head {
            Vec::new()
        } else {
            route.body.into_bytes()
        };
        Ok(Response {
            url: request.url.clone(),
            status,
            headers: headers.into_iter().collect::<Headers>(),
            body,
            redirected: false,
        })
    }
}

/// `data:` and `about:` as the real fetchers do.
fn fixture_builtin(request: &Request) -> Option<Result<Response, NetError>> {
    matches!(request.url.scheme(), "data" | "about")
        .then(|| swb_net::NetworkFetcher::new().fetch(request))
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

/// A page at `APP` with JavaScript on, served by `server`.
fn load(server: Arc<Server>) -> Page {
    let mut page = new_page(server as Arc<dyn Fetcher>, 3, Size::new(600.0, 400.0));
    page.set_javascript(true);
    page.navigate(Url::parse(&format!("{APP}/page.html")).unwrap());
    finish_loading(&mut page, Duration::from_secs(20));
    page
}

fn value(page: &mut Page, source: &str) -> String {
    match page.evaluate(source, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => text,
        other => panic!("{source}: {other:?}"),
    }
}

/// The helpers of the probe pages: `T(promise)` is the type, the status and
/// the text of a response, or the error.
const HELPERS: &str = "window.E = e => e.name + ': ' + e.message; window.T = p => p.then(r => r.text().then(t => r.type + '|' + r.status + '|' + t), E);";

fn with_helpers(page: &mut Page) {
    value(page, HELPERS);
}

#[test]
fn simple_cors_requests_need_the_allowed_origin() {
    on_page_thread(|| {
        let allow = ("access-control-allow-origin", APP);
        let server = Arc::new(
            Server::default()
                .route(&format!("{API}/ok.txt"), route(200, &[allow], "ok"))
                .route(
                    &format!("{API}/star.txt"),
                    route(200, &[("access-control-allow-origin", "*")], "star"),
                )
                .route(&format!("{API}/none.txt"), route(200, &[], "none"))
                .route(
                    &format!("{API}/wrong.txt"),
                    route(
                        200,
                        &[("access-control-allow-origin", "https://else.test")],
                        "w",
                    ),
                )
                .route(
                    &format!("{API}/slash.txt"),
                    route(
                        200,
                        &[("access-control-allow-origin", "https://app.test/")],
                        "s",
                    ),
                )
                .route(
                    &format!("{API}/err.txt"),
                    route(404, &[("access-control-allow-origin", "*")], "err"),
                ),
        );
        let mut page = load(Arc::clone(&server));
        with_helpers(&mut page);
        // The values of Chromium 148 (cors-simple).
        assert_eq!(
            value(
                &mut page,
                &format!(
                    "Promise.all(['ok', 'star', 'none', 'wrong', 'slash', 'err'].map(f => T(fetch('{API}/' + f + '.txt'))))"
                )
            ),
            r#"["cors|200|ok","cors|200|star","TypeError: Failed to fetch","TypeError: Failed to fetch","TypeError: Failed to fetch","cors|404|err"]"#
        );
        // Every request reached the server, with the Origin header.
        let seen = server.seen();
        assert_eq!(seen.len(), 6);
        assert!(seen.iter().all(|s| s.origin.as_deref() == Some(APP)));
        // A `no-cors` request gets an opaque response and no Origin.
        assert_eq!(
            value(
                &mut page,
                &format!(
                    "(async () => {{ const r = await fetch('{API}/none.txt', {{mode: 'no-cors'}}); return [r.type, r.status, r.ok, r.statusText, r.url, [...r.headers], await r.text()]; }})()"
                )
            ),
            r#"["opaque",0,false,"","",[],""]"#
        );
        assert_eq!(server.seen().last().unwrap().origin, None);
        // Mode `same-origin` to another origin sends nothing.
        let count = server.seen().len();
        assert_eq!(
            value(
                &mut page,
                &format!("T(fetch('{API}/ok.txt', {{mode: 'same-origin'}}))")
            ),
            r#""TypeError: Failed to fetch""#
        );
        assert_eq!(server.seen().len(), count);
    });
}

#[test]
fn response_headers_are_filtered_and_exposed() {
    on_page_thread(|| {
        let server = Arc::new(Server::default().route(
            &format!("{API}/h.txt"),
            route(
                200,
                &[
                    ("access-control-allow-origin", "*"),
                    ("access-control-expose-headers", "X-Secret, x-other"),
                    ("content-type", "text/plain"),
                    ("x-secret", "s"),
                    ("x-other", "o"),
                    ("x-third", "t"),
                    ("set-cookie", "a=1"),
                ],
                "h",
            ),
        ));
        let mut page = load(server);
        assert_eq!(
            value(
                &mut page,
                &format!(
                    "fetch('{API}/h.txt').then(r => [r.type, [...r.headers].map(h => h.join('=')).join('&'), r.headers.get('set-cookie'), r.headers.getSetCookie()])"
                )
            ),
            r#"["cors","content-type=text/plain&x-other=o&x-secret=s",null,[]]"#
        );
    });
}

#[test]
fn credentials_modes_decide_about_cookies() {
    on_page_thread(|| {
        let cred = &[
            ("access-control-allow-origin", APP),
            ("access-control-allow-credentials", "true"),
        ];
        let server = Arc::new(
            Server::default()
                .route(&format!("{API}/cred.txt"), route(200, cred, "c"))
                .route(
                    &format!("{API}/star.txt"),
                    route(
                        200,
                        &[
                            ("access-control-allow-origin", "*"),
                            ("access-control-allow-credentials", "true"),
                        ],
                        "s",
                    ),
                )
                .route(
                    &format!("{API}/nocred.txt"),
                    route(200, &[("access-control-allow-origin", APP)], "n"),
                )
                .route(&format!("{APP}/same.txt"), route(200, &[], "same")),
        );
        let mut page = load(Arc::clone(&server));
        with_helpers(&mut page);
        // The values of Chromium 148 (cors-credentials).
        assert_eq!(
            value(
                &mut page,
                &format!(
                    "Promise.all([fetch('{API}/cred.txt'), fetch('{API}/cred.txt', {{credentials: 'omit'}}), fetch('{API}/cred.txt', {{credentials: 'include'}}), fetch('{API}/star.txt', {{credentials: 'include'}}), fetch('{API}/nocred.txt', {{credentials: 'include'}}), fetch('{API}/star.txt', {{credentials: 'omit'}})].map(p => T(p)))"
                )
            ),
            r#"["cors|200|c","cors|200|c","cors|200|c","TypeError: Failed to fetch","TypeError: Failed to fetch","cors|200|s"]"#
        );
        let mut sent: Vec<(String, bool)> = server
            .seen()
            .iter()
            .map(|s| (s.url.clone(), s.credentials))
            .collect();
        sent.sort();
        // Cookies go only with `include` to another origin.
        let with_cookies: Vec<&str> = sent
            .iter()
            .filter(|(_, c)| *c)
            .map(|(u, _)| u.as_str())
            .collect();
        assert_eq!(
            with_cookies,
            [
                "https://api.test/cred.txt",
                "https://api.test/nocred.txt",
                "https://api.test/star.txt"
            ]
        );
        // Same origin: `same-origin` and `include` send, `omit` does not.
        for (mode, want) in [("omit", false), ("same-origin", true), ("include", true)] {
            value(
                &mut page,
                &format!(
                    "fetch('{APP}/same.txt?{mode}', {{credentials: '{mode}'}}).then(r => r.status)"
                ),
            );
            let last = server.seen().last().cloned().unwrap();
            assert_eq!((mode, last.credentials), (mode, want));
        }
    });
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one page, a table of Chromium's values"
)]
fn preflights_for_methods_and_headers() {
    on_page_thread(|| {
        let open = Route {
            status: 200,
            headers: pairs(&[("access-control-allow-origin", APP)]),
            body: "open".to_owned(),
            options: Some((
                204,
                pairs(&[
                    ("access-control-allow-origin", APP),
                    ("access-control-allow-methods", "PUT, DELETE, POST"),
                    ("access-control-allow-headers", "x-custom, content-type"),
                    ("access-control-max-age", "600"),
                ]),
            )),
            delay: 0,
        };
        let server = Arc::new(
            Server::default()
                .route(&format!("{API}/open.txt"), open.clone())
                .route(
                    &format!("{API}/denied.txt"),
                    Route {
                        options: Some((200, Vec::new())),
                        ..open.clone()
                    },
                )
                .route(
                    &format!("{API}/absent.txt"),
                    Route {
                        options: None,
                        ..open.clone()
                    },
                ),
        );
        let mut page = load(Arc::clone(&server));
        with_helpers(&mut page);
        assert_eq!(
            value(
                &mut page,
                &format!(
                    "(async () => {{ const out = []; for (const o of [{{method: 'PUT', body: 'b'}}, {{method: 'DELETE'}}, {{headers: {{'X-Custom': '1'}}}}, {{method: 'POST', headers: {{'Content-Type': 'application/json'}}, body: '{{}}'}}, {{method: 'POST', body: 'plain'}}, {{method: 'PATCH'}}]) out.push(await T(fetch('{API}/open.txt', o))); return out; }})()"
                )
            ),
            // PATCH is not in the allowed methods.
            r#"["cors|200|open","cors|200|open","cors|200|open","cors|200|open","cors|200|open","TypeError: Failed to fetch"]"#
        );
        let log: Vec<String> = server
            .seen()
            .iter()
            .map(|s| {
                let ask = |name: &str| {
                    s.headers
                        .iter()
                        .find(|(n, _)| n == name)
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default()
                };
                format!(
                    "{} {} {}",
                    s.method,
                    ask("access-control-request-method"),
                    ask("access-control-request-headers")
                )
            })
            .collect();
        // The preflight of the first `PUT` (max-age 600) allows `DELETE`,
        // `POST`, `x-custom` and `content-type` too, so the cache answers
        // the next requests; a `POST` of `text/plain` is simple; `PATCH` is
        // not allowed and needs its own preflight.
        assert_eq!(
            log,
            [
                "OPTIONS PUT ",
                "PUT  ",
                "DELETE  ",
                "GET  ",
                "POST  ",
                "POST  ",
                "OPTIONS PATCH ",
            ]
        );
        // A second request of a cached kind sends no preflight.
        let options = |server: &Server| {
            server
                .seen()
                .iter()
                .filter(|s| s.method == "OPTIONS")
                .count()
        };
        let before = options(&server);
        value(
            &mut page,
            &format!("fetch('{API}/open.txt', {{method: 'PUT'}}).then(r => r.status)"),
        );
        assert_eq!(server.seen().last().unwrap().method, "PUT");
        assert_eq!(options(&server), before);
        // Denied, absent: only the preflight is sent.
        let before = server.seen().len();
        assert_eq!(
            value(
                &mut page,
                &format!(
                    "Promise.all(['denied', 'absent'].map(f => T(fetch('{API}/' + f + '.txt', {{method: 'PUT'}}))))"
                )
            ),
            r#"["TypeError: Failed to fetch","TypeError: Failed to fetch"]"#
        );
        let new: Vec<String> = server.seen()[before..]
            .iter()
            .map(|s| s.method.clone())
            .collect();
        assert_eq!(new, ["OPTIONS", "OPTIONS"]);
    });
}

#[test]
fn redirect_modes_and_cross_origin_redirects() {
    on_page_thread(|| {
        let server = Arc::new(
            Server::default()
                .route(&format!("{APP}/plain.txt"), route(200, &[], "plain"))
                .route(
                    &format!("{APP}/same.txt"),
                    route(302, &[("location", "/plain.txt")], ""),
                )
                .route(
                    &format!("{APP}/to-api.txt"),
                    route(302, &[("location", &format!("{API}/final.txt"))], ""),
                )
                .route(
                    &format!("{API}/final.txt"),
                    route(200, &[("access-control-allow-origin", APP)], "final"),
                ),
        );
        let mut page = load(Arc::clone(&server));
        with_helpers(&mut page);
        // The values of Chromium 148 (cors-redirect).
        assert_eq!(
            value(
                &mut page,
                "(async () => { const f = (u, o) => fetch(u, o).then(r => r.text().then(t => [r.type, r.status, r.redirected, r.url.slice(-9), t].join('|')), E); return [await f('/same.txt'), await f('/same.txt', {redirect: 'error'}), await f('/to-api.txt'), await f('/to-api.txt', {mode: 'no-cors'}), await f('/to-api.txt', {mode: 'same-origin'})]; })()"
            ),
            r#"["basic|200|true|plain.txt|plain","TypeError: Failed to fetch","cors|200|true|final.txt|final","opaque|0|false||","TypeError: Failed to fetch"]"#
        );
        assert_eq!(
            value(
                &mut page,
                "(async () => { const r = await fetch('/same.txt', {redirect: 'manual'}); return [r.type, r.status, r.ok, r.url.slice(-8), r.redirected, [...r.headers].length, r.statusText, await r.text()]; })()"
            ),
            r#"["opaqueredirect",0,false,"same.txt",false,0,"",""]"#
        );
        // The Origin header of the hops: none for a same-origin GET, the
        // origin for the cross-origin hop of a CORS request.
        let hops: Vec<(String, Option<String>)> = server
            .seen()
            .iter()
            .filter(|s| s.url.ends_with("final.txt"))
            .map(|s| (s.method.clone(), s.origin.clone()))
            .collect();
        assert_eq!(hops[0], ("GET".to_owned(), Some(APP.to_owned())));
    });
}

#[test]
fn data_urls_and_other_schemes() {
    on_page_thread(|| {
        let mut page = load(Arc::new(Server::default()));
        // The values of Chromium 148 (fetch-schemes).
        assert_eq!(
            value(
                &mut page,
                "(async () => { const F = async u => { try { const r = await fetch(u); return [r.type, r.status, await r.text()].join('|'); } catch (e) { return e.name + ': ' + e.message; } }; return Promise.all(['data:text/plain,hello', 'data:text/plain;base64,aGk=', 'data:,%E2%82%AC', 'data:bad', 'about:blank', 'file:///etc/hostname', 'ftp://example.test/', 'javascript:1', 'blob:https://app.test/x'].map(F)); })()"
            ),
            r#"["basic|200|hello","basic|200|hi","basic|200|€","TypeError: Failed to fetch","TypeError: Failed to fetch","TypeError: Failed to fetch","TypeError: Failed to fetch","TypeError: Failed to fetch","TypeError: Failed to fetch"]"#
        );
        assert_eq!(
            value(
                &mut page,
                "(async () => { const r = await fetch('data:text/plain;charset=utf-8,x'); return [r.headers.get('content-type'), r.status, r.statusText, r.ok, r.redirected, [...r.headers].length]; })()"
            ),
            r#"["text/plain;charset=utf-8",200,"OK",true,false,1]"#
        );
    });
}

#[test]
fn bodies_are_sent_and_read_once() {
    on_page_thread(|| {
        let server = Arc::new(
            Server::default()
                .route(
                    &format!("{APP}/data.json"),
                    route(
                        200,
                        &[("content-type", "application/json")],
                        "{\"a\": [1, 2]}",
                    ),
                )
                .route(&format!("{APP}/echo.txt"), route(201, &[], "echoed"))
                .route(&format!("{APP}/gone.txt"), route(404, &[], "missing"))
                .route(&format!("{APP}/empty.txt"), route(204, &[], "")),
        );
        let mut page = load(Arc::clone(&server));
        assert_eq!(
            value(
                &mut page,
                "(async () => { const r = await fetch('/data.json'); const c = r.clone(); const j = await r.json(); let again; try { await r.json(); } catch (e) { again = e.name + ': ' + e.message; } return [j, again, await c.text(), (await (await fetch('/data.json')).arrayBuffer()).byteLength, r.bodyUsed, r.headers.get('content-type')]; })()"
            ),
            r#"[{"a":[1,2]},"TypeError: Failed to execute 'json' on 'Response': body stream already read","{\"a\": [1, 2]}",13,true,"application/json"]"#
        );
        assert_eq!(
            value(
                &mut page,
                "(async () => { const out = []; for (const u of ['/gone.txt', '/empty.txt']) { const r = await fetch(u); out.push([r.status, r.ok, r.statusText, await r.text()]); } return out; })()"
            ),
            r#"[[404,false,"Not Found","missing"],[204,true,"No Content",""]]"#
        );
        value(
            &mut page,
            "(async () => { for (const [m, b] of [['POST', 'a=1'], ['PUT', new Uint8Array([1, 2, 3])], ['DELETE'], ['HEAD']]) await (await fetch('/echo.txt', {method: m, body: b, headers: {'X-Test': m}})).text(); })()",
        );
        let sent: Vec<(String, String, Option<String>)> = server
            .seen()
            .iter()
            .filter(|s| s.url.ends_with("echo.txt"))
            .map(|s| (s.method.clone(), s.body.clone(), s.origin.clone()))
            .collect();
        // HEAD sends no Origin header; the others send it.
        assert_eq!(
            sent,
            [
                ("POST".to_owned(), "a=1".to_owned(), Some(APP.to_owned())),
                (
                    "PUT".to_owned(),
                    "\u{1}\u{2}\u{3}".to_owned(),
                    Some(APP.to_owned())
                ),
                ("DELETE".to_owned(), String::new(), Some(APP.to_owned())),
                ("HEAD".to_owned(), String::new(), None),
            ]
        );
    });
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one page, a table of Chromium's values"
)]
fn abort_before_during_and_after_the_response() {
    on_page_thread(|| {
        let server = Arc::new(
            Server::default()
                .route(
                    &format!("{APP}/slow.txt"),
                    Route {
                        delay: 400,
                        ..route(200, &[], "slow")
                    },
                )
                .route(&format!("{APP}/fast.txt"), route(200, &[], "fast")),
        );
        let mut page = load(Arc::clone(&server));
        // The values of Chromium 148 (fetch-abort).
        assert_eq!(
            value(
                &mut page,
                "(async () => { const c = new AbortController(); c.abort(); try { await fetch('/slow.txt', {signal: c.signal}); return 'resolved'; } catch (e) { return [e.name + ': ' + e.message, e === c.signal.reason, e.code]; } })()"
            ),
            r#"["AbortError: signal is aborted without reason",true,20]"#
        );
        assert_eq!(server.seen().len(), 0, "an aborted signal sends nothing");
        assert_eq!(
            value(
                &mut page,
                "(async () => { const c = new AbortController(); const p = fetch('/slow.txt', {signal: c.signal}); setTimeout(() => c.abort(), 50); try { await p; return 'resolved'; } catch (e) { return [e.name + ': ' + e.message, e.code, e === c.signal.reason]; } })()"
            ),
            r#"["AbortError: signal is aborted without reason",20,true]"#
        );
        assert_eq!(
            value(
                &mut page,
                "(async () => { try { await fetch('/slow.txt', {signal: AbortSignal.timeout(80)}); return 'resolved'; } catch (e) { return [e.name + ': ' + e.message, e.code]; } })()"
            ),
            r#"["TimeoutError: signal timed out",23]"#
        );
        assert_eq!(
            value(
                &mut page,
                "(async () => { const c = new AbortController(); const d = new AbortController(); const any = AbortSignal.any([c.signal, d.signal]); const p = fetch('/slow.txt', {signal: any}); d.abort('D'); try { await p; } catch (e) { return [e, c.signal.aborted, any.aborted]; } })()"
            ),
            r#"["D",false,true]"#
        );
        // After the response: a body that was read is not affected; a body
        // that was not read errors (Chromium's stream).
        assert_eq!(
            value(
                &mut page,
                "(async () => { const c = new AbortController(); const r = await fetch('/fast.txt', {signal: c.signal}); const t = await r.text(); c.abort(); return [r.status, t, c.signal.aborted, r.bodyUsed]; })()"
            ),
            r#"[200,"fast",true,true]"#
        );
        assert_eq!(
            value(
                &mut page,
                "(async () => { const c = new AbortController(); const r = await fetch('/fast.txt', {signal: c.signal}); c.abort(); try { return [r.status, await r.text()]; } catch (e) { return ['text rejected', e.name + ': ' + e.message, e.code]; } })()"
            ),
            r#"["text rejected","AbortError: The user aborted a request.",20]"#
        );
        assert_eq!(
            value(
                &mut page,
                "(async () => { const c = new AbortController(); const ps = []; for (let i = 0; i < 5; i++) ps.push(fetch('/slow.txt?' + i, {signal: c.signal}).then(() => 'ok', e => e.name)); c.abort(); return await Promise.all(ps); })()"
            ),
            r#"["AbortError","AbortError","AbortError","AbortError","AbortError"]"#
        );
    });
}

#[test]
fn hung_fetches_do_not_hold_the_loads_of_the_page() {
    // Review fix 3: six `fetch()` calls to a server that never answers
    // took the six workers; images and further fetches waited. The server
    // here answers after 1.5 s; the page has three workers.
    on_page_thread(|| {
        let server = Arc::new(
            Server::default()
                .route(
                    &format!("{API}/hang.txt"),
                    Route {
                        delay: 1500,
                        ..route(200, &[], "late")
                    },
                )
                .route(&format!("{APP}/ok.txt"), route(200, &[], "ok"))
                .route(&format!("{APP}/p.png"), route(200, &[], "not an image")),
        );
        let mut page = load(Arc::clone(&server));
        let start = std::time::Instant::now();
        value(
            &mut page,
            "window.hung = Array.from({length: 12}, (_, i) => fetch('https://api.test/hang.txt?' + i, {mode: 'no-cors'}).then(() => 1, () => 0)); 1",
        );
        // A healthy fetch (another host) and an image load do not wait.
        assert_eq!(
            value(&mut page, "fetch('/ok.txt').then(r => r.text())"),
            "\"ok\""
        );
        value(
            &mut page,
            "const i = new Image(); i.src = '/p.png'; document.body.append(i); new Promise(r => setTimeout(r, 200)).then(() => 1)",
        );
        assert!(
            server.seen().iter().any(|s| s.url.ends_with("/p.png")),
            "the image was not requested"
        );
        assert!(
            start.elapsed() < Duration::from_millis(1400),
            "waited {:?}",
            start.elapsed()
        );
        // The hung calls finish later; all of them.
        assert_eq!(
            value(
                &mut page,
                "Promise.all(window.hung).then(a => a.reduce((x, y) => x + y, 0))"
            ),
            "12"
        );
    });
}

#[test]
fn a_navigation_frees_the_places_of_the_old_documents_fetches() {
    // Delta review fix: the old document's hung calls kept all API
    // workers after a navigation, so the new document's calls waited for
    // them. Twelve calls to two hosts fill the eight workers.
    on_page_thread(|| {
        let hang = || Route {
            delay: 3000,
            ..route(200, &[], "late")
        };
        let server = Arc::new(
            Server::default()
                .route(&format!("{API}/hang.txt"), hang())
                .route("https://cdn.test/hang.txt", hang())
                .route(&format!("{APP}/ok.txt"), route(200, &[], "ok")),
        );
        let mut page = load(Arc::clone(&server));
        value(
            &mut page,
            "for (let i = 0; i < 6; i++) { fetch('https://api.test/hang.txt?' + i, {mode: 'no-cors'}); fetch('https://cdn.test/hang.txt?' + i, {mode: 'no-cors'}); } new Promise(r => setTimeout(r, 100)).then(() => 1)",
        );
        assert_eq!(server.seen().len(), 8, "the workers are not all busy");
        let start = std::time::Instant::now();
        page.navigate(Url::parse(&format!("{APP}/page.html?next")).unwrap());
        finish_loading(&mut page, Duration::from_secs(20));
        assert_eq!(
            value(&mut page, "fetch('/ok.txt').then(r => r.text())"),
            "\"ok\""
        );
        assert!(
            start.elapsed() < Duration::from_millis(2500),
            "waited {:?}",
            start.elapsed()
        );
    });
}

#[test]
fn a_page_with_many_pending_fetches_stays_alive() {
    on_page_thread(|| {
        let server = Arc::new(Server::default().route(
            &format!("{APP}/slow.txt"),
            Route {
                delay: 20,
                ..route(200, &[], "slow")
            },
        ));
        let mut page = load(Arc::clone(&server));
        // 300 requests: 64 run at a time, the rest wait in the queue; all
        // finish.
        assert_eq!(
            value(
                &mut page,
                "Promise.all(Array.from({length: 300}, (_, i) => fetch('/slow.txt?' + i).then(r => r.status))).then(a => a.filter(s => s === 200).length)"
            ),
            "300"
        );
        assert_eq!(server.seen().len(), 300);
    });
}
