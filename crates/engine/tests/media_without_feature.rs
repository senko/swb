//! M14 feature 6a (ADR 0047 section 3, "Without the feature"): a build
//! without the Cargo feature `media` behaves like a Chromium build without
//! codecs, with JavaScript on and off. Nothing is fetched; `canPlayType()`
//! answers `""` for every type, so a `<source type>` is skipped with an
//! `error` event on the source; a resource without a type (`src`, or a
//! `<source>` without `type`) fails as a failed request does: `loadstart`,
//! then `error` with code 4 and `MEDIA_ELEMENT_ERROR: Format error`,
//! `networkState` 3 and `readyState` 0. Run by `just test` (they need the
//! build without the feature; `just test-media` skips them).

#![cfg(not(feature = "media"))]
// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::media_site::{MediaSite, RangeMode};
use common::{new_page, node, on_page_thread};
use serde_json::{Value, json};
use swb_engine::{ClockMode, Evaluation, JsonForm, Page, PageClock, Size, Url};
use swb_net::Fetcher;

/// The events and state script reads of the element `v` (and sources `s1`,
/// `s2`), the media requests of the server.
const PROBE_SCRIPT: &str = "<script>
const EVO = [];
function watch(m, tag, names) { names.forEach(n => m.addEventListener(n, () => EVO.push(n + (tag ? '@' + tag : '') + (m.readyState === undefined ? '' : ' rs=' + m.readyState + ' ns=' + m.networkState)))); }
const names = ['loadstart', 'progress', 'suspend', 'abort', 'error', 'emptied', 'loadedmetadata', 'loadeddata', 'canplay', 'canplaythrough', 'durationchange'];
</script>";

fn page_for(html: &str, javascript: bool) -> (Page, Arc<MediaSite>) {
    let mut site = MediaSite::new("case-1", html);
    site.add_file("a.mp4", "fixtures/media/mp4-h264-high-aac.mp4");
    site.add_file("b.webm", "fixtures/media/webm-vp8-vorbis.webm");
    site.mode = RangeMode::Honor;
    let site = Arc::new(site);
    let mut page = new_page(
        Arc::clone(&site) as Arc<dyn Fetcher>,
        2,
        Size::new(800.0, 600.0),
    );
    page.set_javascript(javascript);
    page.set_clock(PageClock::new(ClockMode::Virtual, None));
    page.navigate(Url::parse(&MediaSite::page_url("case-1")).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(20)));
    page.run_event_loop(Duration::from_secs(3), Duration::from_secs(20));
    (page, site)
}

fn evaluate(page: &mut Page, js: &str) -> Value {
    match page.evaluate(js, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap(),
        other => panic!("{js}: {other:?}"),
    }
}

#[test]
fn a_src_fails_with_code_4_and_no_request() {
    on_page_thread(|| {
        let html = format!(
            "<!DOCTYPE html>{PROBE_SCRIPT}<video id=v src=a.mp4></video><script>watch(v, '', names)</script>"
        );
        let (mut page, site) = page_for(&html, true);
        let got = evaluate(
            &mut page,
            "({ev: EVO, ns: v.networkState, rs: v.readyState, code: v.error && v.error.code, message: v.error && v.error.message, src: v.currentSrc.slice(-5), type: v.canPlayType('video/mp4; codecs=\"avc1.42E01E\"')})",
        );
        assert_eq!(
            got,
            json!({
                "ev": ["loadstart rs=0 ns=2", "error rs=0 ns=3"],
                "ns": 3,
                "rs": 0,
                "code": 4,
                "message": "MEDIA_ELEMENT_ERROR: Format error",
                "src": "a.mp4",
                "type": ""
            })
        );
        assert!(site.log_lines().is_empty(), "{:?}", site.log_lines());
    });
}

#[test]
fn a_source_with_a_type_is_skipped_and_one_without_fails_as_a_request_does() {
    on_page_thread(|| {
        let html = format!(
            "<!DOCTYPE html>{PROBE_SCRIPT}<video id=v><source id=s1 src=a.mp4 type='video/mp4'><source id=s2 src=b.webm></video><script>watch(v, '', names); watch(s1, 's1', ['error']); watch(s2, 's2', ['error'])</script>"
        );
        let (mut page, site) = page_for(&html, true);
        let got = evaluate(
            &mut page,
            "({ev: EVO, ns: v.networkState, rs: v.readyState, error: v.error})",
        );
        assert_eq!(
            got,
            json!({
                "ev": [
                    "loadstart rs=0 ns=2",
                    "error@s1",
                    "error@s2",
                ],
                "ns": 3,
                "rs": 0,
                "error": null
            })
        );
        assert!(site.log_lines().is_empty(), "no request");
    });
}

#[test]
fn without_scripts_the_element_ends_in_the_same_state() {
    on_page_thread(|| {
        let html = "<!DOCTYPE html><video id=v src=a.mp4></video><audio id=a><source src=b.webm type='audio/webm'></audio><video id=n src=a.mp4 preload=none></video>";
        let (mut page, site) = page_for(html, false);
        let v = page.media_info(node(&page, "v")).unwrap();
        assert_eq!((v.network_state, v.ready_state), (3, 0));
        let error = v.error.unwrap();
        assert_eq!(
            (error.code, error.message.as_str()),
            (4, "MEDIA_ELEMENT_ERROR: Format error")
        );
        // A typed source is skipped: no error on the element.
        let a = page.media_info(node(&page, "a")).unwrap();
        assert_eq!((a.network_state, a.ready_state, a.error), (3, 0, None));
        // `preload=none` waits for a load that nobody asks for: idle.
        let n = page.media_info(node(&page, "n")).unwrap();
        assert_eq!((n.network_state, n.ready_state), (1, 0));
        assert!(site.log_lines().is_empty(), "no request");
    });
}

#[test]
fn load_without_the_feature_fails_again() {
    on_page_thread(|| {
        let html = format!(
            "<!DOCTYPE html>{PROBE_SCRIPT}<video id=v src=a.mp4 preload=none></video><script>watch(v, '', names)</script>"
        );
        let (mut page, site) = page_for(&html, true);
        let before = evaluate(&mut page, "({ev: EVO.slice(), ns: v.networkState})");
        assert_eq!(
            before,
            json!({"ev": ["loadstart rs=0 ns=1", "suspend rs=0 ns=1"], "ns": 1})
        );
        // `load()` starts the load that `preload=none` held back; it fails.
        let after = evaluate(
            &mut page,
            "v.load(); ({ns: v.networkState, rs: v.readyState})",
        );
        assert_eq!(after, json!({"ns": 3, "rs": 0}));
        page.run_event_loop(Duration::from_secs(2), Duration::from_secs(20));
        let ev = evaluate(&mut page, "EVO.slice(2)");
        assert_eq!(
            ev,
            json!([
                "abort rs=0 ns=3",
                "emptied rs=0 ns=3",
                "loadstart rs=0 ns=2",
                "error rs=0 ns=3"
            ]),
        );
        assert_eq!(evaluate(&mut page, "v.error.code"), json!(4));
        assert!(site.log_lines().is_empty(), "no request");
    });
}
