//! Helpers for unit tests: lay out a document with the bundled test fonts
//! in an 800×600 viewport.

use swb_css::MediaEnvironment;
use swb_dom::{Document, NodeId, local_name, parse_html};
use swb_style::{ElementStates, StyleMap, Stylist, compute_styles};
use swb_text::FontContext;

use crate::{
    BoxFragment, Control, ControlKind, FormControls, FragmentRef, FragmentTree, LayoutContext,
    LayoutInput, NoReplacedSizes, Point, Rect, Size, layout_with,
};

/// The result of a test layout.
pub(crate) struct TestLayout {
    pub(crate) doc: Document,
    pub(crate) tree: FragmentTree,
    /// [`LayoutContext::uncached_layouts`] after the layout.
    pub(crate) uncached_layouts: usize,
}

impl TestLayout {
    /// The element with `id`.
    pub(crate) fn node(&self, id: &str) -> NodeId {
        self.doc
            .element_by_id(id)
            .unwrap_or_else(|| panic!("no element #{id}"))
    }

    /// The union of the border boxes of the element with `id`.
    pub(crate) fn rect(&self, id: &str) -> Rect {
        let node = self.node(id);
        self.tree
            .element_boxes()
            .get(&node)
            .copied()
            .unwrap_or_else(|| panic!("#{id} has no box"))
    }

    /// All text fragments with their absolute positions, in tree order.
    pub(crate) fn texts(&self) -> Vec<(Rect, String)> {
        let mut out = Vec::new();
        self.tree.walk(|f, origin| {
            if let FragmentRef::Text(t) = f {
                out.push((t.rect.translate(origin), t.text.to_string()));
            }
        });
        out
    }

    /// The root box fragment.
    pub(crate) fn root(&self) -> &BoxFragment {
        self.tree
            .root
            .as_ref()
            .expect("the document has a root box")
    }
}

/// Lays out an HTML document.
pub(crate) fn layout_html(html: &str) -> TestLayout {
    layout_document(parse_html(html))
}

/// Lays out a document.
pub(crate) fn layout_document(doc: Document) -> TestLayout {
    let styles = styles_for(&doc);
    let mut fonts = FontContext::for_tests();
    let mut ctx = LayoutContext::new(&mut fonts);
    let input = LayoutInput {
        document: &doc,
        styles: &styles,
        viewport: Size::new(800.0, 600.0),
        replaced: &NoReplacedSizes,
        controls: &TestControls(&doc),
    };
    let tree = layout_with(&input, &mut ctx);
    let uncached_layouts = ctx.uncached_layouts;
    TestLayout {
        doc,
        tree,
        uncached_layouts,
    }
}

/// Form controls from the attributes of `input`, `textarea`, `select`
/// and `button` elements (the value is the `value` attribute), for tests.
/// The engine keeps the real state of controls.
pub(crate) struct TestControls<'a>(pub(crate) &'a Document);

impl FormControls for TestControls<'_> {
    fn is_control(&self, node: NodeId) -> bool {
        self.control(node).is_some()
    }

    fn control(&self, node: NodeId) -> Option<Control> {
        let doc = self.0;
        let e = doc.element(node).filter(|e| e.is_html())?;
        let number = |name: &str, default: u32| {
            e.attr(name)
                .and_then(|v| v.trim().parse::<u32>().ok())
                .filter(|v| *v > 0)
                .unwrap_or(default)
        };
        let value = |default: &str| e.attr("value").unwrap_or(default).to_owned();
        let mut options = Vec::new();
        let (kind, text) = match &**e.local_name() {
            "input" => match e
                .attr("type")
                .unwrap_or("text")
                .to_ascii_lowercase()
                .as_str()
            {
                "hidden" => return None,
                "checkbox" => (ControlKind::Checkbox, String::new()),
                "radio" => (ControlKind::Radio, String::new()),
                "submit" => (ControlKind::Button, value("Submit")),
                "reset" => (ControlKind::Button, value("Reset")),
                "button" => (ControlKind::Button, value("")),
                _ => (
                    ControlKind::TextField {
                        size: number("size", 20),
                    },
                    value(""),
                ),
            },
            "textarea" => (
                ControlKind::TextArea {
                    cols: number("cols", 20),
                    rows: number("rows", 2),
                },
                doc.text_content(node),
            ),
            "select" => {
                options = doc
                    .descendants(node)
                    .filter(|&n| doc.is_html_element(n, &local_name!("option")))
                    .map(|n| doc.text_content(n))
                    .collect();
                (
                    ControlKind::Select,
                    options.first().cloned().unwrap_or_default(),
                )
            }
            "button" => (ControlKind::ButtonElement, String::new()),
            _ => return None,
        };
        Some(Control {
            kind,
            text,
            placeholder: false,
            options,
            caret: None,
            focus: None,
            scroll: Point::default(),
            checked: e.has_attr("checked"),
            disabled: e.has_attr("disabled"),
        })
    }
}

/// Computed styles with the user-agent stylesheet and the document's
/// `<style>` elements.
pub(crate) fn styles_for(doc: &Document) -> StyleMap {
    let mut stylist = Stylist::new(doc.quirks_mode);
    let base = url::Url::parse("about:blank").expect("valid URL");
    let root = doc.document_element().expect("the document has a root");
    for node in doc.descendants(root) {
        if doc.is_html_element(node, &local_name!("style")) {
            let css = doc.text_content(node);
            stylist.add_author_sheet(&swb_css::parse_stylesheet(&css), &base);
        }
    }
    compute_styles(
        doc,
        &stylist,
        &MediaEnvironment::default(),
        &ElementStates::default(),
        &base,
    )
}

/// The rectangles of the text fragments whose text is `text`.
pub(crate) fn rects_of_text(texts: &[(Rect, String)], text: &str) -> Vec<Rect> {
    texts
        .iter()
        .filter(|(_, t)| t == text)
        .map(|(r, _)| *r)
        .collect()
}
