//! A style sheet that a script adds after the load, and that is still
//! loading (M10 9c). Chromium keeps rendering with the sheets that it has:
//! geometry, `getComputedStyle`, the focus order and the observers work, and
//! the pending sheet applies when it arrives (HTML "render-blocking": only
//! the parser's `link` elements in the head block rendering). swb used to
//! drop its layout until the sheet settled: every `getBoundingClientRect()`
//! gave zeros, an `IntersectionObserver` saw nothing, and a Tab press found
//! no element. GitHub's lazy chunks add a `link` whose file the replay lacks;
//! its page then showed the focus order running into a hidden "More items"
//! button. The values are Chromium 148's
//! (`tools/probes/web-pending-sheets.json`, cases
//! `script-link-pending-geometry` and
//! `script-link-pending-focus-and-observer`, three equal runs each).

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use swb_engine::{Key, Size, Url};
use swb_net::{Fetcher, Headers, NetError, Request, Response};

mod common;
use common::{finish_loading, key, new_page, on_page_thread, settle, value};

const PAGE: &str = "https://site.test/page.html";

const HTML: &str = "<!DOCTYPE html><html><head></head><body style=margin:0>\
<div id=a style=height:50px>x</div><button id=b>b</button><button id=c>c</button>\
</body></html>";

/// Serves the page at once and `slow.css` when the gate opens.
struct Gated {
    open: Arc<(Mutex<bool>, Condvar)>,
}

impl Fetcher for Gated {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let (body, kind) = if request.url.path().ends_with("slow.css") {
            let (lock, wake) = &*self.open;
            let mut open = lock.lock().unwrap();
            while !*open {
                let (next, timeout) = wake.wait_timeout(open, Duration::from_secs(30)).unwrap();
                open = next;
                if timeout.timed_out() {
                    break;
                }
            }
            ("#a { color: rgb(255, 0, 0) }", "text/css")
        } else {
            (HTML, "text/html")
        };
        let headers: Headers = [("content-type", kind)].into_iter().collect();
        Ok(Response {
            url: request.url.clone(),
            status: 200,
            headers,
            body: body.as_bytes().to_vec(),
            redirected: false,
        })
    }
}

#[test]
fn a_pending_script_sheet_keeps_the_layout() {
    let open = Arc::new((Mutex::new(false), Condvar::new()));
    let gate = Arc::clone(&open);
    on_page_thread(move || {
        let mut page = new_page(Arc::new(Gated { open: gate }), 2, Size::new(400.0, 200.0));
        page.set_javascript(true);
        page.navigate(Url::parse(PAGE).unwrap());
        finish_loading(&mut page, Duration::from_secs(20));

        // The page adds the sheet; its file is held back.
        value(
            &mut page,
            "var l = document.createElement('link'); l.rel = 'stylesheet'; l.href = 'slow.css'; \
             document.head.appendChild(l); 1",
        );
        // Chromium: {"h1":50,"h2":80,"p":true,"color":"rgb(0, 0, 0)","rects":1,"h3":80,"sheet":0}.
        let read = "var a = document.getElementById('a'); var R = {}; \
             R.h1 = a.getBoundingClientRect().height; a.style.height = '80px'; \
             R.h2 = a.getBoundingClientRect().height; \
             var d = document.createElement('p'); d.textContent = 'y'; d.style.margin = '0'; \
             document.body.appendChild(d); R.p = d.getBoundingClientRect().height > 0; \
             R.color = getComputedStyle(a).color; R.rects = a.getClientRects().length; \
             R.h3 = a.getBoundingClientRect().height; R.sheet = document.styleSheets.length; \
             JSON.stringify(R)";
        let got = value(&mut page, read);
        assert_eq!(
            got,
            r#""{\"h1\":50,\"h2\":80,\"p\":true,\"color\":\"rgb(0, 0, 0)\",\"rects\":1,\"h3\":80,\"sheet\":0}""#
        );
        // The focus order has its elements: Tab, Tab.
        assert!(key(&mut page, &Key::Tab));
        assert_eq!(value(&mut page, "document.activeElement.id"), r#""b""#);
        assert!(key(&mut page, &Key::Tab));
        assert_eq!(value(&mut page, "document.activeElement.id"), r#""c""#);

        // A script's focus and an observer (case
        // `script-link-pending-focus-and-observer`). Chromium:
        // {"focus":"b","io":["true:21"]}; swb's button is 18 px high (the
        // test fonts), so the height only has to be above 0.
        value(
            &mut page,
            "document.getElementById('b').focus(); \
             window.R2 = { focus: document.activeElement.id }; \
             new IntersectionObserver(function (es) { R2.io = es.map(function (e) { \
             return e.isIntersecting + ':' + (e.boundingClientRect.height > 0) }) }) \
             .observe(document.getElementById('c')); 1",
        );
        page.run_event_loop(Duration::from_millis(200), Duration::from_secs(10));
        assert_eq!(
            value(&mut page, "JSON.stringify(R2)"),
            r#""{\"focus\":\"b\",\"io\":[\"true:true\"]}""#
        );

        // The sheet arrives: it applies.
        let (lock, wake) = &*open;
        *lock.lock().unwrap() = true;
        wake.notify_all();
        settle(&mut page);
        assert_eq!(
            value(
                &mut page,
                "getComputedStyle(document.getElementById('a')).color"
            ),
            r#""rgb(255, 0, 0)""#
        );
        assert_eq!(value(&mut page, "document.styleSheets.length"), "1");
    });
}
