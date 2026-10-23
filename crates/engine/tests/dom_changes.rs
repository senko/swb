//! Changes of the DOM through the engine's change API (`Page::change_dom`,
//! and a document state lent out and given back): inserted and removed
//! stylesheets, attribute and text changes, removed elements and changed
//! image sources, checked in the computed style, the boxes and the loads
//! (ADR 0028 sections 2, 3 and 9). Also the parse in steps with
//! JavaScript on, which must render and load as the parse without pauses.
//! Pages are files in a temporary directory.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use swb_dom::{Document, DomError, NodeId, QualName, ns};
use swb_engine::{LoadState, Page, Pixmap, Size, Url};
use swb_net::{Fetcher, NetError, NetworkFetcher, Request, Response};
use swb_style::Rgba;

mod common;
use common::Site;

/// Fetches local files and records the file names of the requests.
struct Recorder {
    inner: NetworkFetcher,
    fetched: Mutex<Vec<String>>,
}

impl Recorder {
    fn fetched(&self) -> Vec<String> {
        self.fetched.lock().unwrap().clone()
    }

    fn count(&self, name: &str) -> usize {
        self.fetched().iter().filter(|n| *n == name).count()
    }
}

impl Fetcher for Recorder {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let name = request.url.path().rsplit('/').next().unwrap_or("");
        self.fetched.lock().unwrap().push(name.to_owned());
        self.inner.fetch(request)
    }
}

fn load_with(url: &Url, javascript: bool) -> (Page, Arc<Recorder>) {
    let recorder = Arc::new(Recorder {
        inner: NetworkFetcher::new(),
        fetched: Mutex::new(Vec::new()),
    });
    let mut page = common::new_page(recorder.clone(), 1, Size::new(800.0, 600.0));
    page.set_javascript(javascript);
    page.navigate(url.clone());
    common::finish_loading(&mut page, Duration::from_secs(30));
    (page, recorder)
}

fn load(url: &Url) -> (Page, Arc<Recorder>) {
    load_with(url, false)
}

fn png(width: u32, height: u32) -> Vec<u8> {
    Pixmap::new(width, height).unwrap().encode_png().unwrap()
}

/// Creates an HTML element with attributes.
fn create(dom: &mut Document, local: &str, attrs: &[(&str, &str)]) -> Result<NodeId, DomError> {
    let name = QualName::new(None, ns!(html), local.into());
    let element = dom.create_element(name, Vec::new())?;
    for (name, value) in attrs {
        dom.set_attr(element, name, value)?;
    }
    Ok(element)
}

/// Appends a new element with attributes and text to `parent`.
fn append(
    dom: &mut Document,
    parent: NodeId,
    local: &str,
    attrs: &[(&str, &str)],
    text: &str,
) -> Result<NodeId, DomError> {
    let element = create(dom, local, attrs)?;
    if !text.is_empty() {
        let text = dom.create_text(text)?;
        dom.append_child(element, text)?;
    }
    dom.append_child(parent, element)?;
    Ok(element)
}

fn head(page: &Page) -> NodeId {
    page.document().unwrap().head().unwrap()
}

fn body(page: &Page) -> NodeId {
    page.document().unwrap().body().unwrap()
}

/// The computed color of the element with the ID `id`, after a flush.
fn color(page: &mut Page, id: &str) -> Rgba {
    page.update_layout();
    let node = common::node(page, id);
    page.styles().expect("styles").get(node).unwrap().color
}

const RED: Rgba = Rgba::new(255, 0, 0, 255);
const BLUE: Rgba = Rgba::new(0, 0, 255, 255);

#[test]
fn inserted_and_removed_stylesheets() {
    let site = Site::new("dom-sheets");
    site.file("blue.css", b"#p { color: rgb(0, 0, 255) }");
    let url = site.page("page.html", "<!DOCTYPE html><p id=p>text</p>");
    let (mut page, recorder) = load(&url);
    assert_eq!(color(&mut page, "p"), Rgba::BLACK);

    let head = head(&page);
    let style = page
        .change_dom(|dom| append(dom, head, "style", &[], "#p { color: rgb(255, 0, 0) }"))
        .unwrap()
        .unwrap();
    assert_eq!(color(&mut page, "p"), RED);

    // The sheet of an inserted link loads; the page waits for it.
    let attrs = [("rel", "stylesheet"), ("href", "blue.css")];
    let link = page
        .change_dom(|dom| append(dom, head, "link", &attrs, ""))
        .unwrap()
        .unwrap();
    assert_eq!(page.load_state(), LoadState::LoadingResources);
    common::finish_loading(&mut page, Duration::from_secs(30));
    assert_eq!(color(&mut page, "p"), BLUE);
    assert_eq!(recorder.count("blue.css"), 1);

    // A link moved before the style element keeps its sheet, which now
    // comes first in the cascade.
    page.change_dom(|dom| dom.insert_before(head, link, Some(style)))
        .unwrap()
        .unwrap();
    assert!(!page.is_loading());
    assert_eq!(color(&mut page, "p"), RED);
    assert_eq!(recorder.count("blue.css"), 1);

    // Removed elements take their sheets with them.
    page.change_dom(|dom| dom.remove(style)).unwrap();
    assert_eq!(color(&mut page, "p"), BLUE);
    page.change_dom(|dom| dom.remove(link)).unwrap();
    assert_eq!(color(&mut page, "p"), Rgba::BLACK);
}

#[test]
fn class_and_text_changes_restyle_and_lay_out() {
    let site = Site::new("dom-class");
    let url = site.page(
        "page.html",
        "<!DOCTYPE html><style>.wide { width: 300px } .red { color: rgb(255, 0, 0) }</style>\
         <div id=d style='width: 100px'>x</div>\
         <span id=s style='display: inline-block; font: 16px DejaVu Sans'>ab</span>",
    );
    let (mut page, _) = load(&url);
    assert_eq!(common::rect(&mut page, "d").width, 100.0);
    let d = common::node(&page, "d");
    page.change_dom(|dom| {
        dom.set_attr(d, "class", "red wide")?;
        dom.remove_attr(d, "style");
        Ok::<_, DomError>(())
    })
    .unwrap()
    .unwrap();
    assert_eq!(common::rect(&mut page, "d").width, 300.0);
    assert_eq!(color(&mut page, "d"), RED);

    // A text change lays out again.
    let s = common::node(&page, "s");
    let before = common::rect(&mut page, "s").width;
    let text = page.document().unwrap().children(s).next().unwrap();
    page.change_dom(|dom| dom.set_data(text, "abcdef"))
        .unwrap()
        .unwrap();
    assert!(common::rect(&mut page, "s").width > before * 2.0);
    assert_eq!(page.element_text(s), "abcdef");
}

#[test]
fn a_removed_element_loses_its_box() {
    let site = Site::new("dom-remove");
    let url = site.page(
        "page.html",
        "<!DOCTYPE html><body style='margin: 0'>\
         <div id=a style='height: 50px'></div><div id=b style='height: 20px'></div>",
    );
    let (mut page, _) = load(&url);
    assert_eq!(common::rect(&mut page, "b").y, 50.0);
    let a = common::node(&page, "a");
    page.change_dom(|dom| dom.remove(a)).unwrap();
    assert_eq!(common::rect(&mut page, "b").y, 0.0);
    assert_eq!(page.element_box(a), None);
}

#[test]
fn a_changed_image_source_loads_the_new_image() {
    let site = Site::new("dom-image");
    site.file("a.png", &png(2, 3));
    site.file("b.png", &png(40, 30));
    let url = site.page("page.html", "<!DOCTYPE html><img id=i src=a.png>");
    let (mut page, recorder) = load(&url);
    let size = |page: &mut Page| {
        let r = common::rect(page, "i");
        (r.width, r.height)
    };
    assert_eq!(size(&mut page), (2.0, 3.0));
    let img = common::node(&page, "i");
    page.change_dom(|dom| dom.set_attr(img, "src", "b.png"))
        .unwrap()
        .unwrap();
    assert_eq!(page.load_state(), LoadState::LoadingResources);
    common::finish_loading(&mut page, Duration::from_secs(30));
    assert_eq!(size(&mut page), (40.0, 30.0));
    // An inserted image loads too.
    let body = body(&page);
    let attrs = [("id", "j"), ("src", "a.png?2")];
    page.change_dom(|dom| append(dom, body, "img", &attrs, ""))
        .unwrap()
        .unwrap();
    common::finish_loading(&mut page, Duration::from_secs(30));
    assert_eq!(common::rect(&mut page, "j").width, 2.0);
    assert_eq!(
        recorder.fetched(),
        ["page.html", "a.png", "b.png", "a.png"],
        "the document, the first image, the new source, the inserted image"
    );
}

#[test]
fn a_lent_document_state_comes_back_with_its_changes() {
    let site = Site::new("dom-lend");
    site.file("a.png", &png(5, 6));
    let url = site.page("page.html", "<!DOCTYPE html><p>x</p>");
    let (mut page, _) = load(&url);
    let mut state = page.take_document_state().unwrap();
    assert!(page.document().is_none());
    // A task changes the DOM without draining the journal.
    let body = state.dom().body().unwrap();
    append(
        state.dom_mut(),
        body,
        "img",
        &[("id", "i"), ("src", "a.png")],
        "",
    )
    .unwrap();
    assert!(!state.is_loading());
    page.return_document_state(state);
    assert_eq!(page.load_state(), LoadState::LoadingResources);
    common::finish_loading(&mut page, Duration::from_secs(30));
    assert_eq!(common::rect(&mut page, "i").width, 5.0);
}

#[test]
fn a_completion_that_arrives_while_the_state_is_lent_is_handled_after_the_return() {
    let site = Site::new("dom-lend-net");
    site.file("blue.css", b"#p { color: rgb(0, 0, 255) }");
    let url = site.page("page.html", "<!DOCTYPE html><p id=p>text</p>");
    let (mut page, recorder) = load(&url);
    let mut state = page.take_document_state().unwrap();
    let head = state.dom().head().unwrap();
    let attrs = [("rel", "stylesheet"), ("href", "blue.css")];
    state
        .change_dom(|dom| append(dom, head, "link", &attrs, ""))
        .unwrap();
    assert!(state.is_loading());
    // Let the response arrive in the loader.
    let started = std::time::Instant::now();
    while recorder.count("blue.css") == 0 && started.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(200));
    // Nothing is consumed or waited for while the state is out.
    assert!(!page.process_network());
    assert!(!page.wait_until_loaded(Duration::from_secs(30)));
    page.return_document_state(state);
    assert_eq!(page.load_state(), LoadState::LoadingResources);
    common::finish_loading(&mut page, Duration::from_secs(30));
    assert!(!page.is_loading());
    assert_eq!(color(&mut page, "p"), BLUE);
}

#[test]
fn a_removed_subtree_with_a_style_loses_its_sheet() {
    let site = Site::new("dom-subtree");
    let url = site.page(
        "page.html",
        "<!DOCTYPE html><p id=p>text</p><div id=d><i><style>#p { color: rgb(255, 0, 0) }</style></i></div><span id=s></span>",
    );
    let (mut page, _) = load(&url);
    assert_eq!(color(&mut page, "p"), RED);
    // A removal without a sheet keeps it.
    let span = common::node(&page, "s");
    page.change_dom(|dom| dom.remove(span)).unwrap();
    assert_eq!(color(&mut page, "p"), RED);
    let div = common::node(&page, "d");
    page.change_dom(|dom| dom.remove(div)).unwrap();
    assert_eq!(color(&mut page, "p"), Rgba::BLACK);
}

/// A page whose stylesheets and images come before, between and after
/// scripts, so that the parse in steps pauses between them.
const STEPPED_PAGE: &str = "<!DOCTYPE html><title>Steps</title>\
    <link rel=stylesheet href=one.css><script>var a = 1;</script>\
    <style>#b { width: 50px }</style><script>var b = 2;</script>\
    <link rel=stylesheet href=two.css>\
    <body><img id=i src=a.png><script>var c = 3;</script>\
    <div id=b>b</div><video id=v poster=p.png></video>\
    <picture><source srcset=s.png><img id=j src=x.png></picture>\
    <script>var d = 4;</script><p id=p class=two>p</p>";

#[test]
fn the_parse_in_steps_loads_and_renders_as_the_parse_to_the_end() {
    let site = Site::new("dom-steps");
    site.file("one.css", b"p { margin: 3px } #b { color: rgb(255, 0, 0) }");
    site.file("two.css", b".two { width: 70px; color: rgb(0, 0, 255) }");
    for (name, w, h) in [
        ("a.png", 3, 4),
        ("p.png", 5, 6),
        ("s.png", 7, 8),
        ("x.png", 9, 9),
    ] {
        site.file(name, &png(w, h));
    }
    let url = site.page("page.html", STEPPED_PAGE);
    let (off, off_loads) = load_with(&url, false);
    let (mut on, on_loads) = load_with(&url, true);
    assert!(on.document_has_scripting() && !off.document_has_scripting());
    assert_eq!(on.title(), "Steps");
    assert_eq!(
        swb_engine::element_boxes(&on),
        swb_engine::element_boxes(&off)
    );
    assert_eq!(color(&mut on, "b"), RED);
    assert_eq!(color(&mut on, "p"), BLUE);
    assert_eq!(common::rect(&mut on, "j").width, 7.0);
    // The same loads (one network thread fetches in the order of the
    // requests). The parse in steps starts each load after the step that
    // inserted the element; the walk starts the stylesheets, then the
    // posters, then the images.
    let kind = |loads: &[String], ext: &str| -> Vec<String> {
        loads.iter().filter(|n| n.ends_with(ext)).cloned().collect()
    };
    let (on_loads, off_loads) = (on_loads.fetched(), off_loads.fetched());
    assert_eq!(kind(&on_loads, ".css"), ["one.css", "two.css"]);
    assert_eq!(kind(&on_loads, ".css"), kind(&off_loads, ".css"));
    assert_eq!(kind(&on_loads, ".png"), ["a.png", "p.png", "s.png"]);
    assert_eq!(kind(&off_loads, ".png"), ["p.png", "a.png", "s.png"]);
    assert!(on.is_fully_loaded());
}
