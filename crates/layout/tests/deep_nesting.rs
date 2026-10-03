//! Deeply nested documents must not overflow the stack: box construction
//! limits the box depth, so that styles, layout, the fragment walk and
//! dropping the trees work on a 2 MiB stack (the default for Rust threads).
//! The documents are built without the HTML parser, which has its own
//! depth limit.

use swb_css::MediaEnvironment;
use swb_dom::{Document, NodeId, QualName, local_name, ns, parse_html};
use swb_layout::{FragmentRef, LayoutInput, NoReplacedSizes, Size, layout};
use swb_style::{ElementStates, Stylist, compute_styles};
use swb_text::FontContext;

const DEPTH: usize = 10_000;
const STACK_SIZE: usize = 2 * 1024 * 1024;

/// A document whose body contains `DEPTH` nested elements: the element
/// names cycle through `names`, and the innermost holds some text.
fn nested_document(names: &[&str], style: &str) -> Document {
    let mut doc = parse_html(&format!(
        "<!DOCTYPE html><style>{style}</style><body><div id=start></div>"
    ));
    let mut parent: NodeId = doc.element_by_id("start").expect("#start exists");
    for i in 0..DEPTH {
        let local = match names[i % names.len()] {
            "span" => local_name!("span"),
            "ul" => local_name!("ul"),
            "li" => local_name!("li"),
            "table" => local_name!("table"),
            "tr" => local_name!("tr"),
            "td" => local_name!("td"),
            _ => local_name!("div"),
        };
        let child = doc.create_element(QualName::new(None, ns!(html), local), Vec::new());
        doc.append_child(parent, child);
        parent = child;
    }
    let text = doc.create_text("innermost text");
    doc.append_child(parent, text);
    doc
}

/// Styles and lays out the document on a thread with a 2 MiB stack, walks
/// the fragment tree, and returns the number of fragments.
fn layout_on_small_stack(names: &'static [&'static str], style: &'static str) -> usize {
    std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(move || {
            let doc = nested_document(names, style);
            let stylist = {
                let mut stylist = Stylist::new(doc.quirks_mode);
                let base = url::Url::parse("about:blank").expect("valid URL");
                stylist.add_author_sheet(&swb_css::parse_stylesheet(style), &base);
                stylist
            };
            let base = url::Url::parse("about:blank").expect("valid URL");
            let styles = compute_styles(
                &doc,
                &stylist,
                &MediaEnvironment::default(),
                &ElementStates::default(),
                &base,
            );
            let mut fonts = FontContext::for_tests();
            let input = LayoutInput {
                document: &doc,
                styles: &styles,
                viewport: Size::new(800.0, 600.0),
                replaced: &NoReplacedSizes,
            };
            let tree = layout(&input, &mut fonts);
            let mut count = 0;
            let mut text_found = false;
            tree.walk(|f, _| {
                count += 1;
                if let FragmentRef::Text(t) = f {
                    text_found |= t.text.contains("innermost");
                }
            });
            assert!(text_found, "the innermost text is laid out");
            assert!(tree.scroll_size.height.is_finite());
            count
        })
        .expect("thread starts")
        .join()
        .expect("layout does not overflow the stack")
}

#[test]
fn nested_blocks() {
    assert!(layout_on_small_stack(&["div"], "div { padding-left: 1px }") > 100);
}

#[test]
fn nested_inline_boxes() {
    assert!(layout_on_small_stack(&["span"], "span { border-left: 1px solid }") > 100);
}

#[test]
fn nested_inline_blocks_and_lists() {
    let style = "span { display: inline-block } li { margin-left: 1px }";
    assert!(layout_on_small_stack(&["div", "span", "ul", "li"], style) > 100);
}

#[test]
fn nested_flex_containers() {
    let style = "div { display: flex; flex-direction: column } span { display: flex }";
    assert!(layout_on_small_stack(&["div", "span"], style) > 100);
}

#[test]
fn nested_tables() {
    // `tr` directly in `table`: every level also gets an anonymous row
    // group.
    let style = "table { display: table } tr { display: table-row } \
                 td { display: table-cell; padding: 1px }";
    assert!(layout_on_small_stack(&["table", "tr", "td"], style) > 100);
}

#[test]
fn nested_misparented_table_cells() {
    // Every cell gets an anonymous table, row group and row.
    let style = "div { display: table-cell; border: 1px solid } span { display: table-row }";
    assert!(layout_on_small_stack(&["div", "span"], style) > 100);
    let style = "div { display: table; border-collapse: collapse; border: 1px solid }";
    assert!(layout_on_small_stack(&["div"], style) > 100);
}

#[test]
fn nested_display_contents() {
    assert!(layout_on_small_stack(&["div", "span"], "div { display: contents }") > 1);
}
