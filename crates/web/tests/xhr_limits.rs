//! The memory bounds of `XMLHttpRequest`, `FileReader`, `Blob` and Blob
//! URLs (M8 feature 10b and its review fixes, ADR 0028 section 16): bytes
//! that live outside the script heap count against it as soon as they
//! arrive, a request holds a bounded number of header bytes, a request of a
//! Blob URL copies the blob when its task runs, and the finished requests
//! leave nothing in the task queue. The runtime has a test document that
//! accepts the requests of scripts; the tests hand it the responses.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

mod common;

use std::time::Duration;

use common::{TestDocument, answer, load_with, on_big_stack};
use swb_web::{ClockMode, ScriptRuntime};

fn page() -> ScriptRuntime {
    load_with(
        TestDocument::accepting_fetches("<!DOCTYPE html><p>x"),
        false,
        ClockMode::Virtual,
    )
}

/// Runs the tasks until none is left, moving the virtual clock on.
fn settle(rt: &mut ScriptRuntime) {
    for _ in 0..1000 {
        let ran = rt.run_due_tasks(Duration::from_secs(5));
        match rt.next_task_time() {
            Some(time) if ran == 0 => rt.clock().advance_to(time),
            Some(_) => {}
            None => break,
        }
    }
}

/// Hands the response `body` of request `id` to the runtime.
fn respond(rt: &mut ScriptRuntime, id: u64, body: Vec<u8>) {
    let response = swb_net::Response {
        url: swb_net::Url::parse("http://site.test/x").unwrap(),
        status: 200,
        headers: swb_net::Headers::default(),
        body,
        redirected: false,
    };
    rt.api_fetch_completed(id, Ok(response), swb_net::ResponseKind::Basic);
}

/// The requests that the page started, in order.
fn started(rt: &mut ScriptRuntime) -> Vec<swb_net::Request> {
    let host = rt.take_back().unwrap();
    let doc = host.into_any().downcast::<TestDocument>().unwrap();
    let requests = doc.started.clone();
    rt.lend(doc);
    requests
}

#[test]
fn response_bytes_are_charged_when_they_arrive() {
    on_big_stack(|| {
        let mut rt = page();
        answer(
            &mut rt,
            "window.keep = []; for (let i = 0; i < 3; i++) { const x = new XMLHttpRequest(); \
             x.open('GET', 'http://site.test/big' + i); x.responseType = 'arraybuffer'; \
             keep.push(x); x.send(); } keep.length",
        );
        rt.collect();
        let before = rt.heap_size();
        for id in 1..=3 {
            respond(&mut rt, id, vec![0; 4 << 20]);
        }
        // No script ran and nothing collected after the responses; the
        // bytes count already.
        assert!(
            rt.heap_size() >= before + (12 << 20),
            "{} bytes before, {} after",
            before,
            rt.heap_size()
        );
        assert_eq!(
            answer(&mut rt, "keep.map(x => x.readyState + ':' + x.status)"),
            serde_json::json!(["4:200", "4:200", "4:200"])
        );
        // A request that is opened again gives its bytes back.
        answer(
            &mut rt,
            "keep.forEach(x => x.open('GET', 'http://site.test/n'))",
        );
        assert!(rt.heap_size() < before + (4 << 20));
    });
}

#[test]
fn retained_responses_reach_the_heap_limit() {
    on_big_stack(|| {
        let mut rt = page();
        rt.set_heap_limit(24 << 20);
        answer(
            &mut rt,
            "window.done = 0; window.keep = []; for (let i = 0; i < 10; i++) { \
             const x = new XMLHttpRequest(); x.open('GET', 'http://site.test/big' + i); \
             x.responseType = 'arraybuffer'; x.onloadend = () => { done++; }; \
             keep.push(x); x.send(); } keep.length",
        );
        for id in 1..=10 {
            respond(&mut rt, id, vec![0; 4 << 20]);
            if rt.failed() {
                break;
            }
        }
        // 10 responses of 4 MiB do not fit in 24 MiB: the usual heap-limit
        // behaviour, the runtime fails.
        assert!(rt.failed());
    });
}

#[test]
fn request_headers_are_bounded() {
    on_big_stack(|| {
        let mut rt = page();
        answer(
            &mut rt,
            "const x = new XMLHttpRequest(); x.open('GET', 'http://site.test/h'); \
             const big = 'a'.repeat(1 << 20); \
             for (let i = 0; i < 50; i++) x.setRequestHeader('x-big', big); \
             const part = 'b'.repeat(40000); \
             x.setRequestHeader('x-part', part); x.setRequestHeader('x-part', part); \
             for (let i = 0; i < 100000; i++) x.setRequestHeader('x-n' + i, 'v'); \
             x.send(); 1",
        );
        let requests = started(&mut rt);
        assert_eq!(requests.len(), 1);
        let headers: Vec<(String, String)> = requests[0]
            .headers
            .iter()
            .map(|(n, v)| (n.to_owned(), v.to_owned()))
            .collect();
        assert!(
            headers.iter().all(|(n, _)| n != "x-big"),
            "a 1 MiB value fits"
        );
        let part = headers.iter().find(|(n, _)| n == "x-part").unwrap();
        // The second value would pass 64 KiB: it is dropped.
        assert_eq!(part.1.len(), 40000);
        let bytes: usize = headers.iter().map(|(n, v)| n.len() + v.len() + 2).sum();
        assert!(bytes <= 64 << 10, "{bytes} bytes of headers");
        assert!(headers.len() <= 256, "{} headers", headers.len());
        assert!(headers.len() >= 200, "{} headers", headers.len());
    });
}

#[test]
fn a_finished_request_leaves_no_timeout_task() {
    on_big_stack(|| {
        let mut rt = page();
        answer(
            &mut rt,
            "window.a = new XMLHttpRequest(); a.open('GET', 'http://site.test/a'); \
             a.timeout = 4e9; a.send(); \
             window.b = new XMLHttpRequest(); b.open('GET', 'http://site.test/b'); \
             b.timeout = 4e9; b.send(); 1",
        );
        assert!(rt.next_task_time().is_some(), "the timeouts are queued");
        respond(&mut rt, 1, b"ok".to_vec());
        answer(&mut rt, "b.abort()");
        assert_eq!(rt.next_task_time(), None);
        assert_eq!(
            answer(&mut rt, "[a.readyState, a.responseText, b.readyState]"),
            serde_json::json!([4, "ok", 0])
        );
        // The timeout still fires for a request that hangs.
        answer(
            &mut rt,
            "window.log = []; window.c = new XMLHttpRequest(); c.open('GET', 'http://site.test/c'); \
             c.timeout = 50; c.ontimeout = () => log.push('timeout'); c.send(); 1",
        );
        settle(&mut rt);
        assert_eq!(answer(&mut rt, "log"), serde_json::json!(["timeout"]));
    });
}

#[test]
fn stopping_the_page_drops_the_requests_of_xhr() {
    on_big_stack(|| {
        let mut rt = page();
        answer(
            &mut rt,
            "window.log = []; window.x = new XMLHttpRequest(); x.open('GET', 'http://site.test/a'); \
             x.timeout = 4e9; x.onload = () => log.push('load'); x.onerror = () => log.push('error'); \
             x.send(); 1",
        );
        rt.abort_document();
        assert_eq!(rt.next_task_time(), None);
        // A late result is ignored: the object stays at `readyState` 1.
        respond(&mut rt, 1, b"late".to_vec());
        assert_eq!(
            answer(&mut rt, "[x.readyState, log]"),
            serde_json::json!([1, []])
        );
    });
}

#[test]
fn a_failed_read_ends_the_reader() {
    on_big_stack(|| {
        let mut rt = page();
        rt.set_heap_limit(20 << 20);
        answer(
            &mut rt,
            "window.log = []; window.r = new FileReader(); \
             ['loadstart', 'load', 'error', 'loadend'].forEach(t => r.addEventListener(t, () => log.push(t))); \
             r.readAsArrayBuffer(new Blob([new Uint8Array(7 << 20)])); 1",
        );
        settle(&mut rt);
        // The result (a copy of 7 MiB, twice for the check) does not fit in
        // the heap: the reader ends with an `error` and can be used again.
        assert_eq!(
            answer(&mut rt, "[r.readyState, r.error && r.error.name, log]"),
            serde_json::json!([2, "NotReadableError", ["loadstart", "error", "loadend"]])
        );
        assert_eq!(
            answer(&mut rt, "r.readAsText(new Blob(['x'])); r.readyState"),
            serde_json::json!(1)
        );
        settle(&mut rt);
        assert_eq!(
            answer(&mut rt, "[r.readyState, r.result]"),
            serde_json::json!([2, "x"])
        );
    });
}

#[test]
fn requests_of_blob_urls_copy_the_blob_when_their_task_runs() {
    on_big_stack(|| {
        let mut rt = page();
        answer(
            &mut rt,
            "window.ok = 0; window.bad = 0; \
             window.url = URL.createObjectURL(new Blob([new Uint8Array(100000)])); \
             for (let i = 0; i < 300; i++) { const x = new XMLHttpRequest(); \
             x.open('GET', url); x.onload = () => { ok++; }; x.onerror = () => { bad++; }; \
             x.send(); } 1",
        );
        settle(&mut rt);
        // 256 requests of Blob URLs may wait for their task; the rest fail
        // like a refused request.
        assert_eq!(answer(&mut rt, "[ok, bad]"), serde_json::json!([256, 44]));
        // Once they ran, the next ones work.
        answer(
            &mut rt,
            "const y = new XMLHttpRequest(); y.open('GET', url); y.onload = () => { ok++; }; y.send(); 1",
        );
        settle(&mut rt);
        assert_eq!(answer(&mut rt, "ok"), serde_json::json!(257));
    });
}

#[test]
fn handlers_do_not_keep_finished_requests_alive() {
    on_big_stack(|| {
        let mut rt = page();
        answer(
            &mut rt,
            "window.done = 0; for (let i = 0; i < 4; i++) { const x = new XMLHttpRequest(); \
             x.open('GET', 'http://site.test/big' + i); x.onload = () => { done += x.status; }; \
             x.send(); } 1",
        );
        for id in 1..=4 {
            respond(&mut rt, id, vec![0; 4 << 20]);
        }
        assert_eq!(answer(&mut rt, "done"), serde_json::json!(800));
        // Each handler refers to its request, and nothing else does: the
        // requests and their 16 MiB of responses are garbage.
        rt.collect();
        assert!(rt.heap_size() < 8 << 20, "{} bytes", rt.heap_size());
    });
}

#[test]
fn targets_held_only_by_their_handlers_live_until_their_events() {
    on_big_stack(|| {
        // The stress mode collects at every allocation: a timeout signal and
        // a reader leave their tables before their events allocate.
        let mut rt = load_with(
            TestDocument::accepting_fetches("<!DOCTYPE html><p>x"),
            true,
            ClockMode::Virtual,
        );
        answer(
            &mut rt,
            "window.log = []; \
             AbortSignal.timeout(50).onabort = (e) => log.push(e.target.reason.name); \
             const r = new FileReader(); r.onload = () => log.push(r.result); \
             r.readAsText(new Blob(['read'])); \
             fetch(URL.createObjectURL(new Blob(['fetched']))).then(x => x.text()).then(t => log.push(t)); 1",
        );
        settle(&mut rt);
        // (The order of the three is not measured.)
        assert_eq!(
            answer(&mut rt, "log.slice().sort()"),
            serde_json::json!(["TimeoutError", "fetched", "read"])
        );
    });
}

#[test]
fn waiting_requests_and_reads_bound_the_blobs_that_they_keep_alive() {
    on_big_stack(|| {
        let mut rt = page();
        // Each turn drops its blob while the request or the read waits for
        // its task; the waiting operations keep at most 256 MiB alive (8 of
        // the 30 MiB blobs).
        assert_eq!(
            answer(
                &mut rt,
                "window.ok = 0; window.bad = 0; \
                 for (let i = 0; i < 9; i++) { let b = new Blob([new Uint8Array(30 << 20)]); \
                 const u = URL.createObjectURL(b); const x = new XMLHttpRequest(); x.open('GET', u); \
                 x.onload = () => { ok++; }; x.onerror = () => { bad++; }; x.send(); \
                 URL.revokeObjectURL(u); b = null; } 1",
            ),
            serde_json::json!(1)
        );
        settle(&mut rt);
        assert_eq!(answer(&mut rt, "[ok, bad]"), serde_json::json!([8, 1]));
        // The slice shares the 30 MiB vector (it is more than half of it).
        // The page holds the readers only through their handlers: a reader
        // must live until its last event (a collection before `load` freed
        // the eighth one).
        answer(
            &mut rt,
            "window.read = 0; window.failed = 0; window.ends = 0; \
             window.readAll = () => { for (let i = 0; i < 9; i++) { \
             let b = new Blob([new Uint8Array(30 << 20)]); const r = new FileReader(); \
             r.onload = () => { read++; }; r.onerror = () => { failed++; }; \
             r.onloadend = () => { ends++; }; \
             r.readAsArrayBuffer(b.slice(0, 16 << 20)); b = null; } }; readAll(); 1",
        );
        settle(&mut rt);
        assert_eq!(
            answer(&mut rt, "[read, failed, ends]"),
            serde_json::json!([8, 1, 9])
        );
        // The finished reads gave their bytes back.
        answer(&mut rt, "readAll(); 1");
        settle(&mut rt);
        assert_eq!(
            answer(&mut rt, "[read, failed]"),
            serde_json::json!([16, 2])
        );
    });
}

#[test]
fn blob_urls_count_the_vectors_that_their_blobs_keep_alive() {
    on_big_stack(|| {
        let mut rt = page();
        // A small slice of a large blob is a copy: 300 URLs fit.
        assert_eq!(
            answer(
                &mut rt,
                "const b = new Blob([new Uint8Array(1 << 20)]); let n = 0; \
                 for (let i = 0; i < 300; i++) { URL.createObjectURL(b.slice(0, 1)); n++; } n"
            ),
            serde_json::json!(300)
        );
        // A slice of most of the blob shares its vector (1 MiB each): 256
        // URLs fit in 256 MiB.
        assert_eq!(
            answer(
                &mut rt,
                "let m = 0; try { for (let i = 0; i < 300; i++) { \
                 URL.createObjectURL(b.slice(0, 900000)); m++; } } catch (e) { window.name = e.name; } \
                 [m >= 250 && m <= 256, window.name]"
            ),
            serde_json::json!([true, "QuotaExceededError"])
        );
    });
}
