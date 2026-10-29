//! `location` and `history` with the page (M8 feature 8a1, ADR 0028
//! section 13): navigations that scripts ask for, the session history that
//! `history.length`, `history.state`, `pushState()` and `back()` show, and
//! the events of navigations within a document. The probe
//! `tools/probes/web-location-history.json` has the values of Chromium for
//! the script side; these tests check what needs a page: loads, entries,
//! scrolling and the order of events. Pages come from an in-memory
//! fetcher; scripts run on a thread with a 16 MiB stack, as in swb.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{Evaluation, JsonForm, Page, Size, Url};
use swb_net::Fetcher;

mod common;
use common::{TestSite, finish_loading, new_page};

const A: &str = "https://site.test/a.html";
const B: &str = "https://site.test/b.html";

/// The page of most tests: listeners that log, and a tall body with an
/// anchor far down.
const PAGE: &str = "<!DOCTYPE html><title>A</title><body style='margin:0'><div style='height:2000px'>top</div><a id=far name=far>far</a><div style='height:2000px'></div><script>\
window.LOG = [];\
addEventListener('popstate', (e) => LOG.push('pop:' + JSON.stringify(e.state) + ':' + location.hash));\
addEventListener('hashchange', (e) => LOG.push('hash:' + e.oldURL.slice(-7) + '>' + e.newURL.slice(-7)));\
</script>";

/// Runs `f` on a thread with the stack of swb's page thread.
fn on_page_thread(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

/// A site with the pages `A` and `B` and a page with JavaScript on that
/// shows `A`.
fn open(b: &str) -> (Page, Arc<TestSite>) {
    let mut site = TestSite::default();
    site.pages.insert(A.to_owned(), PAGE.to_owned());
    site.pages.insert(B.to_owned(), b.to_owned());
    let site = Arc::new(site);
    let mut page = new_page(
        Arc::clone(&site) as Arc<dyn Fetcher>,
        2,
        Size::new(600.0, 400.0),
    );
    page.set_javascript(true);
    page.navigate(Url::parse(A).unwrap());
    finish_loading(&mut page, Duration::from_secs(20));
    (page, site)
}

const B_PAGE: &str = "<!DOCTYPE html><title>B</title><script>window.LOG = [];\
addEventListener('popstate', (e) => LOG.push('pop:' + JSON.stringify(e.state)));</script>";

/// Runs the tasks and loads until nothing is left.
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

/// The string value of an expression.
fn text(page: &mut Page, source: &str) -> String {
    let json = value(page, source);
    serde_json::from_str::<String>(&json).unwrap_or_else(|_| panic!("{source}: {json}"))
}

fn url(page: &Page) -> String {
    page.url().unwrap().to_string()
}

#[test]
fn a_hash_change_scrolls_adds_an_entry_and_fires_events() {
    on_page_thread(|| {
        let (mut page, _) = open(B_PAGE);
        assert_eq!(value(&mut page, "history.length"), "1");
        // `popstate` runs inside the call, `hashchange` later; the URL and
        // the length change at once.
        assert_eq!(
            value(
                &mut page,
                "location.hash = 'far'; [location.hash, history.length, LOG.join()]"
            ),
            r##"["#far",2,"pop:null:#far"]"##
        );
        settle(&mut page);
        assert_eq!(url(&page), format!("{A}#far"));
        assert!(
            page.scroll_position().y > 1500.0,
            "{:?}",
            page.scroll_position()
        );
        assert_eq!(
            text(&mut page, "LOG.join()"),
            "pop:null:#far,hash:/a.html>tml#far"
        );
        // The same hash is no navigation.
        value(&mut page, "location.hash = 'far'");
        settle(&mut page);
        assert_eq!(value(&mut page, "history.length"), "2");
        // `replace` adds no entry.
        value(&mut page, "location.replace('#top2')");
        settle(&mut page);
        assert_eq!(value(&mut page, "history.length"), "2");
        assert_eq!(url(&page), format!("{A}#top2"));
        value(&mut page, "location.assign('#x')");
        settle(&mut page);
        assert_eq!(value(&mut page, "history.length"), "3");
    });
}

#[test]
fn a_link_to_a_fragment_fires_the_events_too() {
    on_page_thread(|| {
        let (mut page, _) = open(B_PAGE);
        page.navigate(Url::parse(&format!("{A}#far")).unwrap());
        settle(&mut page);
        assert_eq!(
            text(&mut page, "LOG.join()"),
            "pop:null:#far,hash:/a.html>tml#far"
        );
    });
}

#[test]
fn location_href_loads_another_document_and_history_returns() {
    on_page_thread(|| {
        let (mut page, site) = open(B_PAGE);
        value(&mut page, "location.href = '/b.html'");
        // The load starts after the task.
        settle(&mut page);
        assert_eq!(url(&page), B);
        assert_eq!(page.title(), "B");
        assert_eq!(value(&mut page, "history.length"), "2");
        value(&mut page, "history.back()");
        settle(&mut page);
        assert_eq!(url(&page), A);
        assert_eq!(page.title(), "A");
        // A traversal to another document loads it and fires no `popstate`.
        assert_eq!(value(&mut page, "LOG.length"), "0");
        assert_eq!(value(&mut page, "history.length"), "2");
        value(&mut page, "history.forward()");
        settle(&mut page);
        assert_eq!(url(&page), B);
        let loads = site.requests().len();
        assert_eq!(loads, 4, "{:?}", site.requests());
    });
}

#[test]
fn location_replace_does_not_add_an_entry() {
    on_page_thread(|| {
        let (mut page, _) = open(B_PAGE);
        value(&mut page, "location.replace('/b.html')");
        settle(&mut page);
        assert_eq!(url(&page), B);
        assert_eq!(value(&mut page, "history.length"), "1");
        assert!(!page.can_go_back());
    });
}

#[test]
fn reload_loads_the_document_again_and_keeps_the_state() {
    on_page_thread(|| {
        let (mut page, site) = open(B_PAGE);
        value(
            &mut page,
            "history.replaceState({kept: [1, 2]}, '', '?q=1')",
        );
        value(&mut page, "location.reload()");
        settle(&mut page);
        assert_eq!(site.requests().len(), 2);
        assert_eq!(site.last_request().url.as_str(), format!("{A}?q=1"));
        assert_eq!(
            value(&mut page, "JSON.stringify(history.state)"),
            r#""{\"kept\":[1,2]}""#
        );
        assert_eq!(value(&mut page, "history.length"), "1");
        value(&mut page, "history.go(0)");
        settle(&mut page);
        assert_eq!(site.requests().len(), 3);
    });
}

#[test]
fn push_state_changes_the_url_and_history_back_fires_popstate() {
    on_page_thread(|| {
        let (mut page, site) = open(B_PAGE);
        assert_eq!(
            value(
                &mut page,
                "history.pushState('a', '', '/p/a'); history.pushState({b: 1}, '', '/p/b?x#h'); [location.pathname, location.search, location.hash, history.length, JSON.stringify(history.state), document.baseURI]"
            ),
            r##"["/p/b","?x","#h",3,"{\"b\":1}","https://site.test/p/b?x#h"]"##
        );
        settle(&mut page);
        assert_eq!(url(&page), "https://site.test/p/b?x#h");
        assert_eq!(site.requests().len(), 1, "no load");
        // The engine's back button: `popstate` with the state.
        assert!(page.go_back());
        settle(&mut page);
        assert_eq!(url(&page), "https://site.test/p/a");
        assert_eq!(text(&mut page, "LOG.join('|')"), "pop:\"a\":");
        // `back()` from a script is a task of its own.
        value(&mut page, "history.back(); LOG.push('after')");
        settle(&mut page);
        assert_eq!(url(&page), A);
        assert_eq!(
            text(&mut page, "LOG.join('|')"),
            "pop:\"a\":|after|pop:null:"
        );
        value(&mut page, "history.go(2)");
        settle(&mut page);
        assert_eq!(url(&page), "https://site.test/p/b?x#h");
        assert_eq!(
            value(&mut page, "JSON.stringify(history.state)"),
            r#""{\"b\":1}""#
        );
        // The traversal from `/p/a` to `/p/b?x#h` is not a fragment change.
        assert!(!text(&mut page, "LOG.join('|')").contains("hash:"));
    });
}

#[test]
fn the_navigation_entry_keeps_the_url_of_the_navigation() {
    // Measured in Chromium (feature 8a2 review): `replaceState` and a
    // fragment change before the first read do not change the `name` of
    // the navigation entry.
    on_page_thread(|| {
        let (mut page, _site) = open(B_PAGE);
        assert_eq!(
            text(
                &mut page,
                "history.replaceState(null, '', '/p/a?x#h'); location.hash = 'z'; performance.getEntriesByType('navigation')[0].name"
            ),
            A
        );
        settle(&mut page);
        assert_eq!(url(&page), "https://site.test/p/a?x#z");
    });
}

#[test]
fn the_state_of_an_entry_survives_a_load_of_its_document_again() {
    on_page_thread(|| {
        let (mut page, _) = open(B_PAGE);
        value(&mut page, "history.pushState({n: 7}, '', '/p');");
        settle(&mut page);
        value(&mut page, "location.href = '/b.html'");
        settle(&mut page);
        assert_eq!(url(&page), B);
        assert_eq!(value(&mut page, "history.state"), "null");
        // Back to `/p`: another document is loaded (the default page of the
        // test site), and the entry's state comes with it.
        value(&mut page, "history.back()");
        settle(&mut page);
        assert_eq!(url(&page), "https://site.test/p");
        assert_eq!(value(&mut page, "history.state.n"), "7");
        assert_eq!(value(&mut page, "history.length"), "3");
    });
}

#[test]
fn the_history_keeps_fifty_entries_and_a_loop_of_pushes_ends() {
    on_page_thread(|| {
        let (mut page, _) = open(B_PAGE);
        let started = std::time::Instant::now();
        assert_eq!(
            value(
                &mut page,
                "for (let i = 0; i < 100000; i++) history.pushState(i, '', '?i=' + (i % 7)); [history.length, history.state, location.search]"
            ),
            r#"[50,99999,"?i=4"]"#
        );
        settle(&mut page);
        assert!(started.elapsed() < Duration::from_secs(20));
        assert_eq!(value(&mut page, "history.length"), "50");
        assert!(page.can_go_back());
        assert!(!page.can_go_forward());
    });
}

#[test]
fn a_hashchange_handler_that_changes_the_hash_runs_per_task() {
    on_page_thread(|| {
        let (mut page, _) = open(B_PAGE);
        value(
            &mut page,
            "window.N = 0; addEventListener('hashchange', () => { if (N++ < 20) location.hash = 'n' + N; }); location.hash = 'n0';",
        );
        settle(&mut page);
        assert_eq!(value(&mut page, "N"), "21");
        assert_eq!(text(&mut page, "location.hash"), "#n20");
        assert_eq!(value(&mut page, "history.length"), "22");
    });
}

#[test]
fn a_javascript_url_runs_as_a_task() {
    on_page_thread(|| {
        let (mut page, site) = open(B_PAGE);
        value(
            &mut page,
            "location.href = 'javascript:window.RAN = (window.RAN || 0) + 1'; window.RAN",
        );
        settle(&mut page);
        assert_eq!(value(&mut page, "window.RAN"), "1");
        assert_eq!(site.requests().len(), 1);
        assert_eq!(url(&page), A);
    });
}

#[test]
fn scroll_restoration_manual_keeps_the_scroll_of_the_page() {
    on_page_thread(|| {
        let (mut page, _) = open(B_PAGE);
        value(
            &mut page,
            "window.scrollTo(0, 500); history.pushState(1, '', '/s'); window.scrollTo(0, 900);",
        );
        settle(&mut page);
        assert!(page.go_back());
        settle(&mut page);
        // The entry that was left keeps the position of the `pushState`
        // call.
        assert_eq!(page.scroll_position().y, 500.0);
        // The entry that the page is at is `manual` from now on.
        value(
            &mut page,
            "history.scrollRestoration = 'manual'; window.scrollTo(0, 700)",
        );
        settle(&mut page);
        assert!(page.go_forward());
        settle(&mut page);
        assert_eq!(page.scroll_position().y, 900.0, "an `auto` entry restores");
        value(&mut page, "window.scrollTo(0, 100)");
        settle(&mut page);
        assert!(page.go_back());
        settle(&mut page);
        // The `manual` entry keeps the position of the page.
        assert_eq!(page.scroll_position().y, 100.0);
    });
}

#[test]
fn location_replace_gives_the_new_document_no_state() {
    on_page_thread(|| {
        let (mut page, _) = open(B_PAGE);
        // The new document, also of another origin, never reads the state
        // of the document that it replaces.
        value(&mut page, "history.replaceState({secret: 'A'}, '')");
        value(&mut page, "location.replace('https://other.test/x.html')");
        settle(&mut page);
        assert_eq!(url(&page), "https://other.test/x.html");
        assert_eq!(value(&mut page, "history.state"), "null");
        assert_eq!(value(&mut page, "history.length"), "1");
        // `assign()` to another document starts an entry with no state.
        value(&mut page, "history.replaceState({secret: 'B'}, '')");
        value(&mut page, "location.assign('/b.html')");
        settle(&mut page);
        assert_eq!(value(&mut page, "history.state"), "null");
        // Back to the entry that `assign()` left: its state is its own.
        value(&mut page, "history.back()");
        settle(&mut page);
        assert_eq!(text(&mut page, "history.state.secret"), "B");
    });
}

#[test]
fn a_navigation_to_the_url_of_the_document_replaces_and_keeps_the_state() {
    on_page_thread(|| {
        let (mut page, site) = open(B_PAGE);
        value(&mut page, "location.hash = 'far'");
        settle(&mut page);
        for call in [
            "location.href = location.href",
            "location.assign(location.href)",
            "location.replace(location.href)",
            "location.port = 'abc'",
        ] {
            value(
                &mut page,
                "history.replaceState({m: 2}, ''); LOG.length = 0",
            );
            value(&mut page, call);
            settle(&mut page);
            // A navigation to a fragment: no entry, the state stays,
            // `popstate` has no state, `hashchange` does not fire.
            assert_eq!(value(&mut page, "history.length"), "2", "{call}");
            assert_eq!(
                text(&mut page, "JSON.stringify(history.state)"),
                r#"{"m":2}"#,
                "{call}"
            );
            assert_eq!(text(&mut page, "LOG.join()"), "pop:null:#far", "{call}");
        }
        assert_eq!(site.requests().len(), 1, "no load");
        // The URL of a document without a fragment is a reload.
        value(&mut page, "history.replaceState({k: 1}, '', '/p?q')");
        value(&mut page, "location.href = location.href");
        settle(&mut page);
        assert_eq!(site.requests().len(), 2);
        assert_eq!(value(&mut page, "history.length"), "2");
        assert_eq!(value(&mut page, "history.state.k"), "1");
    });
}

#[test]
fn an_empty_host_throws_as_chromium_does() {
    on_page_thread(|| {
        let (mut page, _) = open(B_PAGE);
        assert_eq!(
            text(
                &mut page,
                "try { location.hostname = ''; 'no' } catch (e) { e.name + ': ' + e.message }"
            ),
            "SyntaxError: Failed to set the 'hostname' property on 'Location': 'https:///a.html' is not a valid URL."
        );
    });
}

#[test]
fn at_most_sixty_four_traversals_wait_and_reloads_are_one() {
    on_page_thread(|| {
        let (mut page, site) = open(B_PAGE);
        value(
            &mut page,
            "for (let i = 0; i < 3; i++) history.pushState(i, '', '?p=' + i); LOG.length = 0",
        );
        settle(&mut page);
        let started = std::time::Instant::now();
        value(
            &mut page,
            "for (let i = 0; i < 200000; i++) { history.back(); history.go(0); history.forward(); }",
        );
        settle(&mut page);
        assert!(started.elapsed() < Duration::from_secs(20));
        // Each traversal within the document fires one `popstate`; the
        // calls over the bound were dropped. One reload was made.
        let pops = value(&mut page, "window.LOG ? LOG.length : 0")
            .parse::<usize>()
            .unwrap();
        assert!(pops <= 64, "{pops}");
        assert!(site.requests().len() <= 3, "{:?}", site.requests());
    });
}

#[test]
fn a_navigation_before_the_load_event_has_finished_replaces_the_entry() {
    // An inline script, then a `load` handler.
    let pages = [
        (
            "<script>if (!location.search) location.search = '?a';</script>",
            "?a",
        ),
        (
            "<script>addEventListener('load', () => { if (!location.search) location.search = '?b'; });</script>",
            "?b",
        ),
    ];
    for (html, search) in pages {
        on_page_thread(move || {
            let (mut page, _) = open(&format!("<!DOCTYPE html>{html}"));
            value(&mut page, "location.href = '/b.html'");
            settle(&mut page);
            assert_eq!(url(&page), format!("{B}{search}"));
            // A and B, the redirect of B added no entry.
            assert_eq!(value(&mut page, "history.length"), "2", "{search}");
            // After the `load` event, a navigation adds an entry.
            value(
                &mut page,
                "setTimeout(() => { location.search = '?c'; }, 0)",
            );
            settle(&mut page);
            assert_eq!(value(&mut page, "history.length"), "3", "{search}");
        });
    }
}
