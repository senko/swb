//! `document.cookie`, `localStorage` and `sessionStorage` with the page
//! (M8 feature 8b, ADR 0028 sections 12 and 16). The probes
//! `tools/probes/web-cookies.json` and `web-storage.json` have the values
//! of Chromium for the script side; these tests check what needs a page and
//! a jar: a cookie that a response set with `HttpOnly`, the cookies in the
//! requests that follow, storage across the documents of one page, and the
//! jar of a replayed fixture. Pages come from in-memory fetchers; scripts
//! run on a thread with a 16 MiB stack, as in swb.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use swb_engine::{Evaluation, JsonForm, Page, Size, Url};
use swb_net::{CookieJar, Fetcher, Headers, NetError, ReplayFetcher, Request, Response};

mod common;
use common::{Site, finish_loading, new_page};

/// Runs `f` on a thread with the stack of swb's page thread.
fn on_page_thread(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

/// A site with a cookie jar, as the network fetcher has one: it sends the
/// jar's `Cookie` header, stores the `Set-Cookie` headers of the responses,
/// and records the `Cookie` header of each request.
#[derive(Default)]
struct JarSite {
    jar: CookieJar,
    /// Page bodies and the `Set-Cookie` lines of their responses.
    pages: HashMap<String, (String, Vec<&'static str>)>,
    /// The URL and the `Cookie` header of each request.
    seen: Mutex<Vec<(String, Option<String>)>>,
}

impl Fetcher for JarSite {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let cookie = self.jar.cookie_header(request);
        self.seen
            .lock()
            .unwrap()
            .push((request.url.to_string(), cookie));
        let mut url = request.url.clone();
        url.set_fragment(None);
        let (body, set_cookies) = self.pages.get(url.as_str()).cloned().unwrap_or_default();
        let mut headers: Headers = [("content-type", "text/html; charset=utf-8")]
            .into_iter()
            .collect();
        for line in set_cookies {
            headers.append("set-cookie", line);
        }
        self.jar.store_response_cookies(request, &headers);
        Ok(Response {
            url: request.url.clone(),
            status: 200,
            headers,
            body: body.into_bytes(),
            redirected: false,
        })
    }

    fn cookie_jar(&self) -> Option<&CookieJar> {
        Some(&self.jar)
    }
}

impl JarSite {
    fn cookie_headers_of(&self, url: &str) -> Vec<Option<String>> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(seen, _)| seen == url)
            .map(|(_, cookie)| cookie.clone())
            .collect()
    }
}

fn open(fetcher: Arc<dyn Fetcher>, url: &str) -> Page {
    let mut page = new_page(fetcher, 2, Size::new(600.0, 400.0));
    page.set_javascript(true);
    page.navigate(Url::parse(url).unwrap());
    finish_loading(&mut page, Duration::from_secs(20));
    page
}

fn settle(page: &mut Page) {
    assert!(
        page.wait_until_idle(Duration::from_millis(30), Duration::from_secs(20)),
        "the page did not settle"
    );
    page.update_layout();
}

fn value(page: &mut Page, source: &str) -> String {
    match page.evaluate(source, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => text,
        other => panic!("{source}: {other:?}"),
    }
}

const A: &str = "https://site.test/dir/a.html";
const B: &str = "https://site.test/dir/b.html";
const OTHER: &str = "https://other.test/c.html";
const ECHO: &str = "https://site.test/echo.html";

const SCRIPTED: &str = "<!DOCTYPE html><title>x</title><script>window.LOG = [];</script>";

fn site() -> Arc<JarSite> {
    let mut site = JarSite::default();
    site.pages.insert(
        A.to_owned(),
        (
            SCRIPTED.to_owned(),
            vec!["sid=secret; Path=/; HttpOnly", "pub=1; Path=/"],
        ),
    );
    site.pages
        .insert(B.to_owned(), (SCRIPTED.to_owned(), Vec::new()));
    site.pages
        .insert(OTHER.to_owned(), (SCRIPTED.to_owned(), Vec::new()));
    site.pages
        .insert(ECHO.to_owned(), (SCRIPTED.to_owned(), Vec::new()));
    Arc::new(site)
}

#[test]
fn http_only_cookies_are_invisible_to_scripts_and_still_sent() {
    on_page_thread(|| {
        let site = site();
        let mut page = open(Arc::clone(&site) as Arc<dyn Fetcher>, A);
        // The response set `sid` with `HttpOnly`; the script sees `pub`.
        assert_eq!(value(&mut page, "document.cookie"), r#""pub=1""#);
        // A script cannot set `HttpOnly`, cannot replace or delete `sid`,
        // and sets its own cookie.
        assert_eq!(
            value(
                &mut page,
                "document.cookie = 'mine=1; HttpOnly'; document.cookie = 'sid=forged; Path=/'; \
                 document.cookie = 'sid=; Path=/; Max-Age=0'; document.cookie = 'x=2; Path=/'; \
                 document.cookie"
            ),
            r#""pub=1; x=2""#
        );
        // The jar still has the server's cookie, which the next request
        // sends with the cookies of the script.
        let names: Vec<(String, String)> = page
            .cookie_jar()
            .unwrap()
            .cookies()
            .into_iter()
            .map(|c| (c.name, c.value))
            .collect();
        assert_eq!(
            names,
            [
                ("sid".to_owned(), "secret".to_owned()),
                ("pub".to_owned(), "1".to_owned()),
                ("x".to_owned(), "2".to_owned())
            ]
        );
        value(&mut page, &format!("location.href = '{ECHO}'; 0"));
        settle(&mut page);
        assert_eq!(
            site.cookie_headers_of(ECHO),
            [Some("sid=secret; pub=1; x=2".to_owned())]
        );
        // The new document sees the same cookies, without `sid`.
        assert_eq!(value(&mut page, "document.cookie"), r#""pub=1; x=2""#);
    });
}

#[test]
fn cookies_of_other_hosts_and_paths_are_invisible() {
    on_page_thread(|| {
        let site = site();
        let mut page = open(Arc::clone(&site) as Arc<dyn Fetcher>, A);
        value(
            &mut page,
            "document.cookie = 'deep=1; Path=/dir/sub'; document.cookie = 'here=1'; \
             document.cookie = 'dom=1; Domain=other.test'; 0",
        );
        // `here` has the default path `/dir`.
        assert_eq!(value(&mut page, "document.cookie"), r#""here=1; pub=1""#);
        value(&mut page, &format!("location.href = '{OTHER}'; 0"));
        settle(&mut page);
        assert_eq!(value(&mut page, "location.host"), r#""other.test""#);
        assert_eq!(value(&mut page, "document.cookie"), r#""""#);
    });
}

#[test]
fn storage_survives_navigations_and_is_separate_for_other_origins() {
    on_page_thread(|| {
        let site = site();
        let mut page = open(Arc::clone(&site) as Arc<dyn Fetcher>, A);
        value(
            &mut page,
            "localStorage.setItem('k', 'v'); sessionStorage.setItem('s', '1'); \
             localStorage.n = 5; 0",
        );
        let read = "[localStorage.getItem('k'), sessionStorage.getItem('s'), localStorage.n, \
                    localStorage.length, sessionStorage.length]";
        assert_eq!(value(&mut page, read), r#"["v","1","5",2,1]"#);
        // Another document of the same origin.
        value(&mut page, &format!("location.href = '{B}'; 0"));
        settle(&mut page);
        assert_eq!(value(&mut page, "location.pathname"), r#""/dir/b.html""#);
        assert_eq!(value(&mut page, read), r#"["v","1","5",2,1]"#);
        value(&mut page, "localStorage.setItem('k2', 'w'); 0");
        // Another origin sees nothing, and writes its own.
        value(&mut page, &format!("location.href = '{OTHER}'; 0"));
        settle(&mut page);
        assert_eq!(
            value(&mut page, "[localStorage.length, sessionStorage.length]"),
            "[0,0]"
        );
        value(&mut page, "localStorage.setItem('k', 'other'); 0");
        // Back at the first origin, in a new document.
        value(&mut page, &format!("location.href = '{A}'; 0"));
        settle(&mut page);
        assert_eq!(
            value(
                &mut page,
                "[localStorage.getItem('k'), localStorage.getItem('k2'), sessionStorage.length]"
            ),
            r#"["v","w",1]"#
        );
        // `clear` empties only this origin's area.
        value(&mut page, "localStorage.clear(); 0");
        value(&mut page, &format!("location.href = '{OTHER}'; 0"));
        settle(&mut page);
        assert_eq!(value(&mut page, "localStorage.getItem('k')"), r#""other""#);
    });
}

#[test]
fn storage_is_empty_in_a_new_page_and_cookie_averse_documents_are_quiet() {
    on_page_thread(|| {
        // A new page has no storage: nothing lasts beyond the process.
        let site = site();
        let mut page = open(Arc::clone(&site) as Arc<dyn Fetcher>, B);
        assert_eq!(value(&mut page, "localStorage.length"), "0");
        // A `file:` page: cookie-averse, but with a storage area.
        let dir = Site::new("script-storage-file");
        let url = dir.page("page.html", SCRIPTED);
        let mut page = open(Arc::new(swb_net::NetworkFetcher::new()), url.as_str());
        assert_eq!(
            value(
                &mut page,
                "document.cookie = 'a=1'; localStorage.setItem('f', '1'); \
                 [document.cookie, localStorage.getItem('f'), navigator.cookieEnabled]"
            ),
            r#"["","1",true]"#
        );
        assert_eq!(page.cookie_jar().unwrap().cookies().len(), 0);
        // A `data:` document: both throw `SecurityError`.
        let mut page = open(
            Arc::new(swb_net::NetworkFetcher::new()),
            "data:text/html,<script>window.R=1</script>",
        );
        assert_eq!(
            value(
                &mut page,
                "(() => { const r = []; for (const f of [() => document.cookie, () => localStorage, \
                 () => sessionStorage, () => { document.cookie = 'a=1'; }]) { try { f(); r.push('ok'); } \
                 catch (e) { r.push(e.name + ': ' + e.message); } } return r; })()"
            ),
            r#"["SecurityError: Failed to read the 'cookie' property from 'Document': Cookies are disabled inside 'data:' URLs.","SecurityError: Failed to read the 'localStorage' property from 'Window': Storage is disabled inside 'data:' URLs.","SecurityError: Failed to read the 'sessionStorage' property from 'Window': Storage is disabled inside 'data:' URLs.","SecurityError: Failed to set the 'cookie' property on 'Document': Cookies are disabled inside 'data:' URLs."]"#
        );
    });
}

#[test]
fn document_cookie_works_in_a_replayed_fixture() {
    on_page_thread(|| {
        let dir = Site::new("script-storage-replay");
        let root = dir.page("manifest.json", "").to_file_path().unwrap();
        let root = root.parent().unwrap().to_path_buf();
        std::fs::create_dir_all(root.join("files")).unwrap();
        std::fs::write(root.join("files/a.html"), SCRIPTED).unwrap();
        let manifest = r#"{"version": 1, "entries": [
            {"method": "GET", "url": "https://site.test/dir/a.html", "status": 200,
             "headers": [["content-type", "text/html; charset=utf-8"]], "body": "files/a.html"},
            {"method": "GET", "url": "https://site.test/dir/b.html", "status": 200,
             "headers": [["content-type", "text/html; charset=utf-8"]], "body": "files/a.html"}]}"#;
        std::fs::write(root.join("manifest.json"), manifest).unwrap();
        let fetcher = ReplayFetcher::load(&root).unwrap();
        let mut page = open(Arc::new(fetcher), A);
        // JavaScript off leaves the jar empty; with scripts, the page stores
        // and reads cookies and keeps them across documents.
        assert_eq!(
            value(
                &mut page,
                "document.cookie = 'a=1; Path=/'; document.cookie = 'b=2'; document.cookie"
            ),
            r#""b=2; a=1""#
        );
        assert_eq!(page.cookie_jar().unwrap().cookies().len(), 2);
        value(&mut page, &format!("location.href = '{B}'; 0"));
        settle(&mut page);
        assert_eq!(value(&mut page, "document.cookie"), r#""b=2; a=1""#);
        // The automation view of the jar sees the script's cookies, and
        // clearing it empties `document.cookie`.
        page.cookie_jar().unwrap().clear();
        assert_eq!(value(&mut page, "document.cookie"), r#""""#);
    });
}

#[test]
fn a_replayed_fixture_without_scripts_has_an_empty_jar() {
    let dir = Site::new("script-storage-replay-off");
    let root = dir.page("manifest.json", "").to_file_path().unwrap();
    let root = root.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(root.join("files")).unwrap();
    std::fs::write(root.join("files/a.html"), SCRIPTED).unwrap();
    let manifest = r#"{"version": 1, "entries": [
        {"method": "GET", "url": "https://site.test/dir/a.html", "status": 200,
         "headers": [["content-type", "text/html; charset=utf-8"]], "body": "files/a.html"}]}"#;
    std::fs::write(root.join("manifest.json"), manifest).unwrap();
    let fetcher = ReplayFetcher::load(&root).unwrap();
    let mut page = new_page(Arc::new(fetcher), 2, Size::new(600.0, 400.0));
    page.navigate(Url::parse(A).unwrap());
    finish_loading(&mut page, Duration::from_secs(20));
    assert_eq!(page.cookie_jar().unwrap().cookies().len(), 0);
}
