//! Tests of the document state: resource discovery from the journal
//! against the walk of a complete document, invalidation by generation,
//! the parsed-stylesheet cache and the derived data.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use swb_dom::{Document, HtmlParser, NodeId, QualName, local_name, ns};
use swb_layout::Size;
use swb_net::{Fetcher, Loader, NetError, Request, Response, Url};

use super::{DocumentInit, DocumentState, View};
use crate::resources::{Images, SheetSource};
use crate::selection::{Selection, TextPosition};

/// A fetcher that fails every request: the tests look at the loads that
/// start, not at their results.
struct NoNetwork;

impl Fetcher for NoNetwork {
    fn fetch(&self, _request: &Request) -> Result<Response, NetError> {
        Err(NetError::UnsupportedScheme("test".to_owned()))
    }
}

fn url() -> Url {
    Url::parse("https://a.test/dir/page.html").unwrap()
}

/// The state of a document: `dom` parsed to the end, or parsed in steps
/// by `parser`.
fn state(dom: Document, parser: Option<HtmlParser>, url: &Url) -> DocumentState {
    let mut doc = DocumentState::new(DocumentInit {
        content_type: "text/html".to_owned(),
        dom,
        parser,
        url: url.clone(),
        encoding: encoding_rs::UTF_8,
        images: Images::default(),
        loader: Rc::new(Loader::new(Arc::new(NoNetwork), 1, Arc::new(|| {}))),
        view: View {
            viewport: Size::new(1280.0, 800.0),
            scale: 1.0,
            indicators: false,
        },
        scroll_target: None,
        parse: Duration::ZERO,
    });
    doc.run_parser();
    doc
}

/// The state of `html` parsed without pauses (the walk).
fn walked(html: &str, url: &Url) -> DocumentState {
    state(swb_dom::parse_html(html), None, url)
}

/// The state of `html` parsed in steps with scripting off (the journal).
fn journaled(html: &str, url: &Url) -> DocumentState {
    let mut dom = Document::new();
    let parser = HtmlParser::new(&mut dom, html, false);
    state(dom, Some(parser), url)
}

/// The loads of one kind (`style` or `image`), in order.
fn loads(doc: &DocumentState, kind: &str) -> Vec<String> {
    doc.load_log
        .iter()
        .filter(|l| l.starts_with(kind))
        .cloned()
        .collect()
}

/// What discovery found: the loads, the sheets and the images.
fn discovered(doc: &DocumentState) -> String {
    let sheets: Vec<_> = doc
        .sheets
        .iter()
        .map(|s| (s.owner, &s.source, &s.css, &s.media, s.base_url.as_str()))
        .collect();
    let mut images: Vec<_> = doc
        .images
        .by_node
        .iter()
        .map(|(node, image)| (*node, image.url.as_str(), image.density))
        .collect();
    images.sort_by_key(|(node, _, _)| *node);
    let mut sources: Vec<_> = doc.states.dimension_sources.iter().collect();
    sources.sort();
    let auto: Vec<_> = doc.images.auto.iter().map(|(n, _)| *n).collect();
    format!(
        "styles {:?}\nimages {:?}\nsheets {sheets:?}\nby node {images:?}\n\
         dimension sources {sources:?}\nauto {auto:?}\ntitle {:?}\nbase {}",
        loads(doc, "style"),
        loads(doc, "image"),
        doc.title(),
        doc.base_url(),
    )
}

/// The HTML of a fixture page and its URL.
fn fixture_page(dir: &Path) -> Option<(Url, String)> {
    let fixture: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("fixture.json")).ok()?).ok()?;
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).ok()?).ok()?;
    let url = fixture["url"].as_str()?;
    let entry = manifest["entries"]
        .as_array()?
        .iter()
        .find(|e| e["url"].as_str() == Some(url))?;
    let charset = entry["headers"].as_array()?.iter().find_map(|h| {
        let (name, value) = (h[0].as_str()?, h[1].as_str()?);
        if !name.eq_ignore_ascii_case("content-type") {
            return None;
        }
        let (_, charset) = value.split_once("charset=")?;
        Some(charset.trim().to_owned())
    });
    let bytes = std::fs::read(dir.join(entry["body"].as_str()?)).ok()?;
    let (text, _) = swb_dom::decode_html_bytes(&bytes, charset.as_deref());
    Some((Url::parse(url).ok()?, text))
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The pages of the fixtures and the layout tests.
fn test_pages() -> Vec<(String, Url, String)> {
    let root = repo_root();
    let mut pages = Vec::new();
    let mut dirs: Vec<_> = std::fs::read_dir(root.join("fixtures/pages"))
        .expect("the fixtures directory")
        .filter_map(|e| Some(e.ok()?.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let (url, text) = fixture_page(&dir)
            .unwrap_or_else(|| panic!("cannot read the fixture {}", dir.display()));
        pages.push((dir.display().to_string(), url, text));
    }
    let mut files: Vec<_> = std::fs::read_dir(root.join("tests/layout"))
        .expect("the layout tests")
        .filter_map(|e| Some(e.ok()?.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "html"))
        .collect();
    files.sort();
    for file in files {
        let text = std::fs::read_to_string(&file).expect("a layout test");
        let url = Url::from_file_path(file.canonicalize().unwrap()).unwrap();
        pages.push((file.display().to_string(), url, text));
    }
    pages
}

#[test]
fn the_journal_finds_the_loads_of_the_walk() {
    let pages = test_pages();
    assert!(pages.len() > 6, "the fixtures are missing");
    for (name, url, text) in pages {
        let walk = walked(&text, &url);
        let journal = journaled(&text, &url);
        assert_eq!((walk.walks, journal.walks), (1, 0), "{name}");
        assert_eq!(discovered(&walk), discovered(&journal), "{name}");
        assert_eq!(walk.dom.len(), journal.dom.len(), "{name}");
    }
}

#[test]
fn the_document_state_is_static() {
    fn is_static<T: 'static>() {}
    is_static::<DocumentState>();
    is_static::<Box<DocumentState>>();
}

fn html_name(local: swb_dom::LocalName) -> QualName {
    QualName::new(None, ns!(html), local)
}

fn element(doc: &DocumentState, id: &str) -> NodeId {
    doc.dom().element_by_id(id).expect("the element exists")
}

#[test]
fn inserted_and_removed_sheets() {
    let mut doc = walked(
        "<head><style id=s>p{}</style></head><body><p id=p>x</p>",
        &url(),
    );
    doc.update_style();
    assert!(doc.stylist.is_some() && doc.styles.is_some());
    let head = doc.dom().head().unwrap();
    // A link before the style element: its sheet comes first.
    let link = doc.change_dom(|dom| {
        let link = dom.create_element(html_name(local_name!("link")), Vec::new())?;
        dom.set_attr(link, "rel", "stylesheet")?;
        dom.set_attr(link, "href", "a.css")?;
        let first = dom.children(head).next();
        dom.insert_before(head, link, first)?;
        Ok::<_, swb_dom::DomError>(link)
    });
    let link = link.unwrap();
    assert_eq!(loads(&doc, "style"), ["style https://a.test/dir/a.css"]);
    assert_eq!(doc.sheets.len(), 2);
    assert_eq!(doc.sheets.get(0).unwrap().owner, link);
    assert!(doc.stylist.is_none(), "a new sheet marks the stylist dirty");
    // A moved link keeps its sheet and does not load again.
    doc.change_dom(|dom| dom.append_child(head, link)).unwrap();
    assert_eq!(doc.sheets.get(1).unwrap().owner, link);
    assert_eq!(loads(&doc, "style").len(), 1);
    // A changed `href` loads the new sheet.
    doc.change_dom(|dom| dom.set_attr(link, "href", "b.css"))
        .unwrap();
    assert_eq!(loads(&doc, "style").len(), 2);
    assert_eq!(
        doc.sheets.get(1).unwrap().source,
        SheetSource::Link(Url::parse("https://a.test/dir/b.css").unwrap())
    );
    // Removed elements take their sheets away.
    let style = element(&doc, "s");
    doc.change_dom(|dom| {
        dom.remove(style);
        dom.remove(link);
    });
    assert_eq!(doc.sheets.len(), 0);
}

#[test]
fn style_text_changes_reparse_only_that_sheet() {
    let mut doc = walked(
        "<style id=a>p{color:red}</style><style id=b>p{margin:0}</style><p id=p>x</p>",
        &url(),
    );
    doc.update_style();
    let rules = |doc: &DocumentState, i: usize| {
        doc.sheets
            .get(i)
            .unwrap()
            .parsed
            .as_ref()
            .map(|s| s.rules.as_ptr())
    };
    let (a, b) = (rules(&doc, 0), rules(&doc, 1));
    assert!(a.is_some() && b.is_some());
    let text = doc.dom().children(element(&doc, "b")).next().unwrap();
    doc.change_dom(|dom| dom.set_data(text, "p{margin:1px}"))
        .unwrap();
    assert!(doc.stylist.is_none());
    assert_eq!(
        doc.sheets.get(1).unwrap().css.as_deref(),
        Some("p{margin:1px}")
    );
    doc.update_style();
    // The stylist was built again from the kept parse of the first sheet.
    assert_eq!(rules(&doc, 0), a);
    assert!(rules(&doc, 1).is_some());
}

#[test]
fn a_flush_after_no_change_does_nothing() {
    let mut doc = walked("<style>.on{color:red}</style><p id=p>x</p>", &url());
    doc.update_style();
    let generation = doc.dom().generation();
    doc.process_changes();
    doc.update_style();
    assert!(doc.styles.is_some() && doc.stylist.is_some());
    assert_eq!(doc.dom().generation(), generation);
    // A class change marks the styles dirty, not the stylist.
    let p = element(&doc, "p");
    doc.change_dom(|dom| dom.set_attr(p, "class", "on"))
        .unwrap();
    assert!(doc.styles.is_none() && doc.stylist.is_some());
    // A change through `dom_mut` waits for the next drain or flush.
    doc.update_style();
    doc.dom_mut().remove_attr(p, "class");
    assert!(doc.styles.is_some());
    doc.update_style();
    let color = doc.styles().unwrap().get(p).unwrap().color;
    assert_eq!(color, swb_style::Rgba::BLACK);
}

#[test]
fn images_follow_their_attributes() {
    let mut doc = walked(
        "<img id=i src=a.png><video id=v poster=p.png></video>",
        &url(),
    );
    let (img, video) = (element(&doc, "i"), element(&doc, "v"));
    assert_eq!(
        loads(&doc, "image"),
        [
            "image https://a.test/dir/p.png",
            "image https://a.test/dir/a.png"
        ]
    );
    // A new `src` loads; the current image stays until it arrives (the
    // HTML pending request).
    doc.change_dom(|dom| dom.set_attr(img, "src", "b.png"))
        .unwrap();
    assert_eq!(doc.images.by_node[&img].url.path(), "/dir/a.png");
    assert_eq!(loads(&doc, "image").len(), 3);
    doc.change_dom(|dom| dom.remove_attr(video, "poster"));
    assert!(!doc.images.by_node.contains_key(&video));
    // An inserted picture selects its image with its sources.
    doc.change_dom(|dom| {
        let picture = dom.create_element(html_name(local_name!("picture")), Vec::new())?;
        let source = dom.create_element(html_name(local_name!("source")), Vec::new())?;
        dom.set_attr(source, "srcset", "s.png")?;
        let image = dom.create_element(html_name(local_name!("img")), Vec::new())?;
        dom.set_attr(image, "id", "j")?;
        dom.set_attr(image, "src", "c.png")?;
        dom.append_child(picture, source)?;
        dom.append_child(picture, image)?;
        let body = dom.body().expect("a body");
        dom.append_child(body, picture)
    })
    .unwrap();
    let j = element(&doc, "j");
    assert_eq!(doc.images.by_node[&j].url.path(), "/dir/s.png");
    // A changed source selects again (and loads the new image).
    let source = doc.dom().prev_sibling_element(j).unwrap();
    doc.change_dom(|dom| dom.set_attr(source, "srcset", "t.png"))
        .unwrap();
    // A removed image is forgotten.
    doc.change_dom(|dom| dom.remove(img));
    assert!(!doc.images.by_node.contains_key(&img));
    assert_eq!(
        loads(&doc, "image"),
        [
            "image https://a.test/dir/p.png",
            "image https://a.test/dir/a.png",
            "image https://a.test/dir/b.png",
            "image https://a.test/dir/s.png",
            "image https://a.test/dir/t.png",
        ]
    );
}

#[test]
fn title_and_base_url_are_derived_by_generation() {
    let mut doc = walked("<title>One</title><a id=a href=x>x</a>", &url());
    assert_eq!(doc.title(), "One");
    let title = doc
        .dom()
        .find_element(NodeId::DOCUMENT, |e| e.is_html_named(&local_name!("title")))
        .unwrap();
    let text = doc.dom().children(title).next().unwrap();
    doc.change_dom(|dom| dom.set_data(text, " Two  words "))
        .unwrap();
    assert_eq!(doc.title(), "Two words");
    assert!(doc.title.is_current(doc.dom().generation()));
    // A change elsewhere confirms the title without a rebuild.
    let a = element(&doc, "a");
    doc.change_dom(|dom| dom.set_attr(a, "class", "c")).unwrap();
    assert!(doc.title.is_current(doc.dom().generation()));
    assert_eq!(doc.base_url().as_str(), url().as_str());
    doc.change_dom(|dom| {
        let base = dom.create_element(html_name(local_name!("base")), Vec::new())?;
        dom.set_attr(base, "href", "https://b.test/")?;
        let head = dom.head().expect("a head");
        dom.append_child(head, base)
    })
    .unwrap();
    assert_eq!(doc.base_url().as_str(), "https://b.test/");
}

#[test]
fn tree_order_is_rebuilt_after_a_change() {
    let mut doc = walked("<p id=a>a</p><p id=b>b</p>", &url());
    let text = |doc: &DocumentState, id: &str| doc.dom().children(element(doc, id)).next().unwrap();
    let (a, b) = (text(&doc, "a"), text(&doc, "b"));
    let at = |node| TextPosition { node, offset: 0 };
    let selection = Selection::new(at(b), at(a));
    assert_eq!(selection.ordered(doc.tree_order()).0.node, a);
    // Move the first paragraph after the second.
    let (pa, body) = (element(&doc, "a"), doc.dom().body().unwrap());
    doc.change_dom(|dom| dom.append_child(body, pa)).unwrap();
    assert_eq!(selection.ordered(doc.tree_order()).0.node, b);
}

#[test]
fn removed_nodes_leave_the_element_states() {
    let mut doc = walked("<input id=i><p id=p>text</p>", &url());
    let (input, p) = (element(&doc, "i"), element(&doc, "p"));
    doc.update_states(|s| {
        s.focus = Some(input);
        s.hover = Some(p);
    });
    doc.change_dom(|dom| {
        dom.remove(input);
        dom.remove(p);
    });
    assert_eq!((doc.states.focus, doc.states.hover), (None, None));
    // Form controls are rebuilt for the new generation.
    assert!(doc.forms.value().get(input).is_none());
}

#[test]
fn a_selection_of_some_images_does_not_scan_all_images() {
    // Every parser step with an image selects it alone; the cost must not
    // grow with the images of the document.
    use std::fmt::Write as _;
    let mut html = String::from("<!DOCTYPE html><body>");
    for k in 0..50 {
        write!(html, "<img src=i{k}.png><script></script>").unwrap();
    }
    let mut dom = Document::new();
    let parser = HtmlParser::new(&mut dom, &html, true);
    let mut doc = state(dom, Some(parser), &url());
    assert_eq!(doc.images.by_url.len(), 50);
    assert_eq!(doc.images.forget_calls, 0);
    // A stop forgets once; the next selection requests the images again.
    doc.stop_loading(&mut swb_text::FontContext::for_tests());
    assert_eq!(doc.images.forget_calls, 1);
    assert!(doc.images.by_url.is_empty());
}

#[test]
fn changes_outside_the_document_keep_the_styles() {
    let mut doc = walked(
        "<style>.on{color:red}</style><p id=p>x</p><template id=t><b>y</b></template>",
        &url(),
    );
    doc.update_style();
    let runs = doc.flushes.style_runs;
    // A detached subtree: built, its attributes and text changed.
    let div = doc
        .change_dom(|dom| {
            let div = dom.create_element(html_name(local_name!("div")), Vec::new())?;
            let span = dom.create_element(html_name(local_name!("span")), Vec::new())?;
            dom.append_child(div, span)?;
            dom.set_attr(span, "class", "on")?;
            let text = dom.create_text("t")?;
            dom.append_child(span, text)?;
            dom.set_data(text, "u")?;
            Ok::<_, swb_dom::DomError>(div)
        })
        .unwrap();
    assert!(doc.styles.is_some(), "a detached change keeps the styles");
    // Template contents are not in the document either.
    let template = element(&doc, "t");
    let contents = doc
        .dom()
        .element(template)
        .and_then(|e| e.template_contents)
        .unwrap();
    let b = doc.dom().children(contents).next().unwrap();
    doc.change_dom(|dom| dom.set_attr(b, "class", "on"))
        .unwrap();
    assert!(
        doc.styles.is_some(),
        "a change in template contents keeps the styles"
    );
    doc.update_style();
    assert_eq!(doc.flushes.style_runs, runs);
    // The insertion of the subtree changes the styles.
    let body = doc.dom().body().unwrap();
    doc.change_dom(|dom| dom.append_child(body, div)).unwrap();
    assert!(doc.styles.is_none());
    // A change in a subtree after its removal in the same drain: the
    // removal was in the document.
    doc.update_style();
    let p = element(&doc, "p");
    doc.change_dom(|dom| {
        dom.remove(p);
        dom.set_attr(p, "class", "on")
    })
    .unwrap();
    assert!(doc.styles.is_none());
}

#[test]
fn a_deep_detached_chain_keeps_the_styles() {
    let mut doc = walked("<p id=p>x</p>", &url());
    doc.update_style();
    // 50,000 levels built top-down: each entry's parent is deeper. The
    // check walks each ancestor once per drain.
    doc.change_dom(|dom| {
        let mut parent = dom.create_element(html_name(local_name!("div")), Vec::new())?;
        for _ in 0..50_000 {
            let child = dom.create_element(html_name(local_name!("div")), Vec::new())?;
            dom.append_child(parent, child)?;
            parent = child;
        }
        Ok::<_, swb_dom::DomError>(())
    })
    .unwrap();
    assert!(doc.styles.is_some());
}

#[test]
fn element_styles_equal_the_full_styles() {
    let html = "<style>body{color:blue} .x{display:none} .x .y{visibility:hidden} \
                p+p{font-size:20px} :root{font-size:10px} b{line-height:2rem}</style>\
                <div class=x><span class=y id=s>a</span></div><p>1</p><p id=q>2<b id=b>3</b></p>";
    let mut doc = walked(html, &url());
    let b = element(&doc, "b");
    let s = element(&doc, "s");
    let q = element(&doc, "q");
    assert!(doc.styles.is_none());
    let one = doc.query_element_style(b).unwrap();
    assert_eq!(doc.flushes.element_style_runs, 1);
    assert_eq!(doc.flushes.style_runs, 0);
    // The ancestors are kept: no second run for `q`.
    let q_style = doc.query_element_style(q).unwrap();
    assert_eq!(doc.flushes.element_style_runs, 1);
    // A descendant of `display: none` gets a style too.
    let hidden = doc.query_element_style(s).unwrap();
    assert_eq!(hidden.visibility, swb_style::Visibility::Hidden);
    doc.update_style();
    let full = doc.styles().unwrap();
    assert_eq!(*one, **full.get(b).unwrap());
    assert_eq!(*q_style, **full.get(q).unwrap());
    assert_eq!(one.font_size, 20.0);
    assert!(full.get(s).is_none());
    let full_b = Arc::clone(full.get(b).unwrap());
    // With current styles the query reads them.
    let again = doc.query_element_style(b).unwrap();
    assert!(Arc::ptr_eq(&again, &full_b));
}

#[test]
fn layout_queries_count_as_forced() {
    let mut doc = walked("<p id=p>x</p>", &url());
    let mut fonts = swb_text::FontContext::for_tests();
    doc.update_layout(&mut fonts);
    assert_eq!((doc.flushes.style_runs, doc.flushes.layout_runs), (1, 1));
    assert_eq!(doc.flushes.forced_layout_runs, 0);
    let p = element(&doc, "p");
    doc.change_dom(|dom| dom.set_attr(p, "class", "a")).unwrap();
    doc.query_layout(&mut fonts);
    doc.query_layout(&mut fonts);
    let f = doc.flushes;
    assert_eq!((f.style_runs, f.layout_runs), (2, 2));
    assert_eq!((f.forced_style_runs, f.forced_layout_runs), (1, 1));
    assert!(f.forced_layout_time <= f.layout_time);
}
