//! DOM changes from scripts reach style, layout and the loads (ADR 0028
//! section 9, feature 6a): an inserted, moved or removed element shows up
//! in the boxes, an attribute change restyles, a changed text relays out,
//! and an inserted style sheet, `link` and `img` apply or load. The
//! scripts run through `Page::evaluate`, as tasks. Pages are files in a
//! temporary directory; the page thread has the 16 MiB stack of swb's.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use swb_engine::{Evaluation, JsonForm, Page, Pixmap, Size, Url};
use swb_net::{Fetcher, NetError, NetworkFetcher, Request, Response};
use swb_style::Rgba;

mod common;
use common::{Site, finish_loading, new_page, node, rect};

/// Fetches local files and records the file names of the requests.
struct Recorder {
    inner: NetworkFetcher,
    fetched: Mutex<Vec<String>>,
}

impl Recorder {
    fn count(&self, name: &str) -> usize {
        self.fetched
            .lock()
            .unwrap()
            .iter()
            .filter(|n| *n == name)
            .count()
    }
}

impl Fetcher for Recorder {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let name = request.url.path().rsplit('/').next().unwrap_or("");
        self.fetched.lock().unwrap().push(name.to_owned());
        self.inner.fetch(request)
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

/// A page with JavaScript on that shows `url`.
fn load(url: &Url) -> (Page, Arc<Recorder>) {
    let recorder = Arc::new(Recorder {
        inner: NetworkFetcher::new(),
        fetched: Mutex::new(Vec::new()),
    });
    let mut page = new_page(recorder.clone(), 2, Size::new(600.0, 400.0));
    page.set_javascript(true);
    page.navigate(url.clone());
    finish_loading(&mut page, Duration::from_secs(30));
    (page, recorder)
}

/// Runs a script as a task and lays the page out.
fn run(page: &mut Page, source: &str) -> String {
    let value = match page.evaluate(source, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => text,
        other => panic!("{source}: {other:?}"),
    };
    page.update_layout();
    value
}

/// Waits for the requests that a script started (with a script runtime,
/// `wait_until_loaded` waits for the `load` event only).
fn settle(page: &mut Page) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        page.process_network();
        page.update_layout();
        if !page.is_loading() {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "requests did not finish"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn color(page: &mut Page, id: &str) -> Rgba {
    page.update_layout();
    let n = node(page, id);
    page.styles().expect("styles").get(n).unwrap().color
}

const RED: Rgba = Rgba::new(255, 0, 0, 255);
const BLUE: Rgba = Rgba::new(0, 0, 255, 255);

const BOXES: &str = "<!DOCTYPE html><style>body { margin: 0 } div { height: 20px } #w { display: inline-block } .red { color: red } #x { height: 50px }</style>\
    <div id=a>a</div><div id=b>b</div><span id=w>ab</span>";

#[test]
fn inserted_moved_and_removed_elements_change_the_boxes() {
    on_page_thread(|| {
        let site = Site::new("script-dom-boxes");
        let url = site.page("page.html", BOXES);
        let (mut page, _) = load(&url);
        assert_eq!(rect(&mut page, "b").y, 20.0);

        // An appended element has a box after the others.
        run(
            &mut page,
            "var n = document.createElement('div'); n.id = 'n'; n.setAttribute('style', 'height: 30px'); document.body.insertBefore(n, document.getElementById('w'));",
        );
        assert_eq!(
            (rect(&mut page, "n").y, rect(&mut page, "n").height),
            (40.0, 30.0)
        );

        // Moving it before the first element moves the others down.
        run(
            &mut page,
            "document.body.insertBefore(n, document.getElementById('a'))",
        );
        assert_eq!(rect(&mut page, "n").y, 0.0);
        assert_eq!(rect(&mut page, "a").y, 30.0);

        // Removing an element removes its box and closes the gap.
        let a = node(&page, "a");
        run(&mut page, "document.getElementById('a').remove()");
        assert!(page.element_box(a).is_none());
        assert_eq!(rect(&mut page, "b").y, 30.0);

        // replaceChild and the other mixin methods.
        run(
            &mut page,
            "var r = document.createElement('div'); r.id = 'r'; document.body.replaceChild(r, document.getElementById('b')); r.after('t', document.createElement('div'));",
        );
        assert_eq!(rect(&mut page, "r").y, 30.0);
        assert!(page.document().unwrap().element_by_id("b").is_none());
    });
}

#[test]
fn attribute_and_text_changes_restyle_and_lay_out() {
    on_page_thread(|| {
        let site = Site::new("script-dom-attributes");
        let url = site.page("page.html", BOXES);
        let (mut page, _) = load(&url);
        assert_eq!(color(&mut page, "a"), Rgba::BLACK);

        run(
            &mut page,
            "document.getElementById('a').classList.add('red')",
        );
        assert_eq!(color(&mut page, "a"), RED);
        run(&mut page, "document.getElementById('a').className = ''");
        assert_eq!(color(&mut page, "a"), Rgba::BLACK);

        // An id change selects `#x`.
        assert_eq!(rect(&mut page, "b").height, 20.0);
        run(&mut page, "document.getElementById('b').id = 'x'");
        assert_eq!(rect(&mut page, "x").height, 50.0);

        // The style attribute.
        run(
            &mut page,
            "document.getElementById('a').setAttribute('style', 'height: 70px')",
        );
        assert_eq!(rect(&mut page, "a").height, 70.0);
        run(
            &mut page,
            "document.getElementById('a').removeAttribute('style')",
        );
        assert_eq!(rect(&mut page, "a").height, 20.0);

        // Text: the inline block grows with its text.
        let before = rect(&mut page, "w").width;
        run(
            &mut page,
            "document.getElementById('w').firstChild.data = 'abcdefghij'",
        );
        assert!(rect(&mut page, "w").width > before);
        run(&mut page, "document.getElementById('w').textContent = 'a'");
        assert!(rect(&mut page, "w").width < before);

        // dataset and attributes reach the DOM the engine reads.
        run(
            &mut page,
            "document.getElementById('a').dataset.fooBar = '1'",
        );
        let a = node(&page, "a");
        assert_eq!(
            page.document()
                .unwrap()
                .element(a)
                .unwrap()
                .attr("data-foo-bar"),
            Some("1")
        );
    });
}

#[test]
fn inserted_style_sheets_and_images_apply_and_load() {
    on_page_thread(|| {
        let site = Site::new("script-dom-loads");
        site.file("blue.css", b"#a { color: rgb(0, 0, 255) }");
        let png = Pixmap::new(2, 3).unwrap().encode_png().unwrap();
        site.file("pic.png", &png);
        let url = site.page(
            "page.html",
            "<!DOCTYPE html><style>body { margin: 0 }</style><div id=a>a</div>",
        );
        let (mut page, recorder) = load(&url);

        // A style element.
        run(
            &mut page,
            "var s = document.createElement('style'); s.textContent = '#a { color: red }'; document.head.appendChild(s);",
        );
        assert_eq!(color(&mut page, "a"), RED);

        // A link to a style sheet loads; the sheet applies after it.
        run(
            &mut page,
            "var l = document.createElement('link'); l.setAttribute('rel', 'stylesheet'); l.setAttribute('href', 'blue.css'); document.head.appendChild(l);",
        );
        settle(&mut page);
        assert_eq!(color(&mut page, "a"), BLUE);
        assert_eq!(recorder.count("blue.css"), 1);

        // Removing the link takes the sheet away.
        run(&mut page, "l.remove()");
        assert_eq!(color(&mut page, "a"), RED);

        // An img loads and gets its size.
        run(
            &mut page,
            "var i = document.createElement('img'); i.id = 'i'; i.setAttribute('src', 'pic.png'); document.body.appendChild(i);",
        );
        settle(&mut page);
        assert_eq!(recorder.count("pic.png"), 1);
        let r = rect(&mut page, "i");
        assert_eq!((r.width, r.height), (2.0, 3.0));

        // Changing the src loads the other image; the old one stays cached.
        run(&mut page, "i.setAttribute('src', 'pic.png?2')");
        settle(&mut page);
        assert_eq!(recorder.count("pic.png"), 2);
    });
}

#[test]
fn a_cloned_fragment_with_a_style_sheet_applies() {
    on_page_thread(|| {
        let site = Site::new("script-dom-clone");
        let url = site.page("page.html", "<!DOCTYPE html><div id=a>a</div>");
        let (mut page, _) = load(&url);
        // A style in a fragment does not apply; a clone of the fragment
        // that goes into the head does.
        run(
            &mut page,
            "var f = document.createDocumentFragment(); var s = document.createElement('style'); s.textContent = '#a { color: red }'; f.appendChild(s);",
        );
        assert_eq!(color(&mut page, "a"), Rgba::BLACK);
        run(&mut page, "document.head.appendChild(f.cloneNode(true));");
        assert_eq!(color(&mut page, "a"), RED);
    });
}

#[test]
fn inserted_markup_reaches_style_layout_and_the_loads() {
    on_page_thread(|| {
        let site = Site::new("script-dom-markup");
        site.file("blue.css", b"#a { color: rgb(0, 0, 255) }");
        let png = Pixmap::new(2, 3).unwrap().encode_png().unwrap();
        site.file("pic.png", &png);
        let url = site.page(
            "page.html",
            "<!DOCTYPE html><style>body { margin: 0 } div { height: 20px }</style><div id=a>a</div><div id=host></div>",
        );
        let (mut page, recorder) = load(&url);

        // A style element in `insertAdjacentHTML` applies.
        run(
            &mut page,
            "document.head.insertAdjacentHTML('beforeend', '<style>#a { color: red }</style>')",
        );
        assert_eq!(color(&mut page, "a"), RED);

        // A link in `innerHTML` loads its sheet, which applies afterwards.
        run(
            &mut page,
            "document.getElementById('host').innerHTML = '<link rel=stylesheet href=blue.css><i id=m>m</i>'",
        );
        settle(&mut page);
        assert_eq!(recorder.count("blue.css"), 1);
        assert_eq!(color(&mut page, "a"), BLUE);

        // An image loads and has its size; a box in the markup is laid out.
        run(
            &mut page,
            "document.getElementById('host').innerHTML = '<img id=i src=pic.png><div id=n style=\"height: 30px\">n</div>'",
        );
        settle(&mut page);
        assert_eq!(recorder.count("pic.png"), 1);
        let r = rect(&mut page, "i");
        assert_eq!((r.width, r.height), (2.0, 3.0));
        assert_eq!(rect(&mut page, "n").height, 30.0);

        // The old nodes are gone with their sheet: the link left the page.
        assert_eq!(color(&mut page, "a"), RED);

        // `outerHTML` replaces the element and its box.
        run(
            &mut page,
            "document.getElementById('n').outerHTML = '<div id=o style=\"height: 40px\"></div>'",
        );
        assert_eq!(rect(&mut page, "o").height, 40.0);
        assert!(page.document().unwrap().element_by_id("n").is_none());

        // Scripts in markup do not run, with any of the entry points.
        let ran = run(
            &mut page,
            "var h = document.getElementById('host'); h.innerHTML = '<script>window.ran1 = 1</script>'; h.insertAdjacentHTML('beforeend', '<script>window.ran2 = 1</script>'); h.firstChild.outerHTML = '<script>window.ran3 = 1</script>'; typeof window.ran1 + typeof window.ran2 + typeof window.ran3",
        );
        assert_eq!(ran, "\"undefinedundefinedundefined\"");
    });
}

#[test]
fn documents_that_scripts_parse_are_not_rendered_or_loaded() {
    on_page_thread(|| {
        let site = Site::new("script-dom-documents");
        site.file("other.css", b"#a { color: rgb(0, 0, 255) }");
        let png = Pixmap::new(2, 3).unwrap().encode_png().unwrap();
        site.file("pic.png", &png);
        let url = site.page(
            "page.html",
            "<!DOCTYPE html><style>body { margin: 0 } div { height: 20px }</style><div id=a>a</div>",
        );
        let (mut page, recorder) = load(&url);
        let before = rect(&mut page, "a");
        let value = run(
            &mut page,
            "var d = new DOMParser().parseFromString('<style>#a { color: red }</style><link rel=stylesheet href=other.css><img src=pic.png><div id=a style=\"height: 90px\">x</div>', 'text/html'); var h = document.implementation.createHTMLDocument('t'); h.body.innerHTML = '<style>#a { color: red }</style><img src=pic.png>'; [d.querySelectorAll('div').length, h.querySelectorAll('img').length, document.getElementById('a') === d.getElementById('a')]",
        );
        assert_eq!(value, "[1,1,false]");
        settle(&mut page);
        assert_eq!(recorder.count("other.css"), 0);
        assert_eq!(recorder.count("pic.png"), 0);
        assert_eq!(color(&mut page, "a"), Rgba::BLACK);
        assert_eq!(rect(&mut page, "a"), before);
        // A node that moves into the page is rendered and loads.
        run(
            &mut page,
            "document.body.appendChild(d.querySelector('img')); document.head.appendChild(d.querySelector('style'));",
        );
        settle(&mut page);
        assert_eq!(recorder.count("pic.png"), 1);
        assert_eq!(color(&mut page, "a"), RED);
    });
}
