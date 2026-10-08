//! Tests of inline SVG: box construction, content, drawing commands and
//! limits.

use std::sync::Arc;

use swb_dom::parse_html;
use swb_style::{FillRule, Rgba, StrokeLinecap};

use super::*;
use crate::geom::{Matrix, Rect, Size};
use crate::test_support::{body, layout_html, styles_for};
use crate::{BoxContent, FragmentRef};

/// The content of the first `<svg>` box of a laid-out document and its
/// content box size.
fn svg_content(html: &str) -> (Arc<SvgContent>, Size) {
    let layout = layout_html(&body(html));
    let mut found = None;
    layout.tree.walk(|f, _| {
        if let FragmentRef::Box(b) = f
            && let BoxContent::Svg(svg) = &b.content
            && found.is_none()
        {
            let c = b.content_rect();
            found = Some((Arc::clone(svg), Size::new(c.width, c.height)));
        }
    });
    found.expect("an svg box")
}

/// The drawing commands of the first `<svg>` of a document.
fn draw(html: &str) -> Vec<SvgDrawItem> {
    let (svg, size) = svg_content(html);
    let mut out = Vec::new();
    svg.draw(size, &mut out);
    out
}

#[test]
fn svg_is_a_replaced_box_and_its_descendants_have_no_boxes() {
    let layout = layout_html(&body(
        "<svg id=s width=40 height=20><title>Title</title><g id=g><path id=p d='M0 0h10v10z'/>\
         </g>loose text<foreignObject><div id=d>html</div></foreignObject></svg>after",
    ));
    assert_eq!(layout.rect("s"), Rect::new(0.0, 0.0, 40.0, 20.0));
    let boxes = layout.tree.element_boxes();
    for id in ["g", "p", "d"] {
        assert!(!boxes.contains_key(&layout.node(id)), "#{id} has a box");
    }
    let texts: Vec<String> = layout.texts().into_iter().map(|(_, t)| t).collect();
    assert_eq!(texts, vec!["after".to_owned()]);
}

#[test]
fn paths_fill_black_with_the_view_box_transform() {
    let items = draw("<svg width=100 height=50 viewBox='0 0 10 10'><path d='M0 0h10v10z'/></svg>");
    let [
        SvgDrawItem::Fill {
            transform,
            color,
            rule,
            path,
        },
    ] = &items[..]
    else {
        panic!("one fill: {items:?}");
    };
    assert_eq!(*transform, Matrix::new(5.0, 0.0, 0.0, 5.0, 25.0, 0.0));
    assert_eq!(*color, Rgba::BLACK);
    assert_eq!(*rule, FillRule::NonZero);
    assert_eq!(path.segments().len(), 4);
}

#[test]
fn presentation_attributes_cascade_and_inherit() {
    let items = draw(
        "<style>.blue { fill: blue }</style>\
         <svg width=10 height=10 fill=red stroke=green stroke-width=2 style='color: #010203'>\
         <g fill-rule=evenodd><rect class=blue width=5 height=5 /><rect width=5 height=5 fill=currentColor stroke=none /></g></svg>",
    );
    let fills: Vec<(Rgba, FillRule)> = items
        .iter()
        .filter_map(|i| match i {
            SvgDrawItem::Fill { color, rule, .. } => Some((*color, *rule)),
            _ => None,
        })
        .collect();
    assert_eq!(
        fills,
        vec![
            (Rgba::rgb(0, 0, 255), FillRule::EvenOdd),
            (Rgba::rgb(1, 2, 3), FillRule::EvenOdd)
        ]
    );
    let strokes: Vec<f32> = items
        .iter()
        .filter_map(|i| match i {
            SvgDrawItem::Stroke { stroke, color, .. } => {
                assert_eq!(*color, Rgba::rgb(0, 128, 0));
                Some(stroke.width)
            }
            _ => None,
        })
        .collect();
    assert_eq!(strokes, vec![2.0]);
}

#[test]
fn opacity_folds_into_one_paint_and_groups_two() {
    let items = draw(
        "<svg width=10 height=10><rect width=5 height=5 opacity=0.5 />\
         <rect width=5 height=5 opacity=0.5 stroke=black fill-opacity=0.5 />\
         <g opacity=0.25><rect width=5 height=5 /></g>\
         <g opacity=0.25><rect width=5 height=5 /><rect width=5 height=5 /></g></svg>",
    );
    let kinds: Vec<String> = items
        .iter()
        .map(|i| match i {
            SvgDrawItem::PushOpacity(o) => format!("push {o}"),
            SvgDrawItem::PopOpacity => "pop".to_owned(),
            SvgDrawItem::Fill { color, .. } => format!("fill {}", color.a),
            SvgDrawItem::Stroke { color, .. } => format!("stroke {}", color.a),
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            "fill 128",
            "push 0.5",
            "fill 128",
            "stroke 255",
            "pop",
            // A group with one shape with one paint needs no layer.
            "fill 64",
            "push 0.25",
            "fill 255",
            "fill 255",
            "pop"
        ]
    );
}

#[test]
fn hidden_and_undisplayed_content_draws_nothing() {
    let items = draw(
        "<svg width=10 height=10><g visibility=hidden><rect width=1 height=1 />\
         <rect width=2 height=2 visibility=visible /></g><g display=none><rect width=3 height=3 /></g>\
         <defs><rect width=4 height=4 /></defs><clipPath><rect width=5 height=5 /></clipPath>\
         <svg><rect width=6 height=6 /></svg><foo><rect width=7 height=7 /></foo>\
         <a><rect width=8 height=8 /></a><rect width=0 height=9 /><circle r=-1 />\
         <rect width=9 height=9 fill=none /></svg>",
    );
    let widths: Vec<f32> = items
        .iter()
        .filter_map(|i| match i {
            SvgDrawItem::Fill { path, .. } => Some(path.bounds().width),
            _ => None,
        })
        .collect();
    assert_eq!(widths, vec![2.0, 8.0]);
}

#[test]
fn percentages_use_the_view_box_or_the_content_box() {
    let bounds = |html: &str| {
        draw(html)
            .iter()
            .find_map(|i| match i {
                SvgDrawItem::Fill { path, .. } => Some(path.bounds()),
                _ => None,
            })
            .expect("a fill")
    };
    assert_eq!(
        bounds("<svg width=200 height=100><rect width='50%' height='10%' /></svg>"),
        Rect::new(0.0, 0.0, 100.0, 10.0)
    );
    assert_eq!(
        bounds(
            "<svg width=200 height=100 viewBox='0 0 20 40'><rect x='50%' width='50%' height='10%' /></svg>"
        ),
        Rect::new(10.0, 0.0, 10.0, 4.0)
    );
    // `r` refers to the normalized diagonal: sqrt((200² + 100²) / 2).
    let r = bounds("<svg width=200 height=100><circle r='10%' /></svg>").width / 2.0;
    assert!((r - 15.811).abs() < 0.01, "{r}");
    // `em` uses the element's font size.
    assert_eq!(
        bounds(
            "<svg width=200 height=100 style='font-size: 10px'><rect x='1em' width='2em' height='1ex' /></svg>"
        ),
        Rect::new(10.0, 0.0, 20.0, 5.0)
    );
}

#[test]
fn transforms_compose() {
    let items = draw(
        "<svg width=100 height=100><g transform='translate(10 20)'>\
         <rect width=5 height=5 style='transform: scale(2)' /></g></svg>",
    );
    let Some(SvgDrawItem::Fill { transform, .. }) = items.first() else {
        panic!("a fill: {items:?}");
    };
    assert_eq!(*transform, Matrix::new(2.0, 0.0, 0.0, 2.0, 10.0, 20.0));
    // The view box is the reference box of percentages in `transform`.
    let items = draw(
        "<svg width=100 height=100 viewBox='0 0 200 200'><rect width=5 height=5 \
         style='transform: translate(50%, 0)' /></svg>",
    );
    let Some(SvgDrawItem::Fill { transform, .. }) = items.first() else {
        panic!("a fill: {items:?}");
    };
    assert_eq!(*transform, Matrix::new(0.5, 0.0, 0.0, 0.5, 50.0, 0.0));
}

#[test]
fn strokes_resolve_dashes_and_caps() {
    let items = draw(
        "<svg width=100 height=100><path d='M0 0h10' stroke=red stroke-dasharray='1 2 3' \
         stroke-dashoffset=1 stroke-linecap=round stroke-miterlimit=0.5 /></svg>",
    );
    let Some(SvgDrawItem::Stroke { stroke, .. }) = items.get(1) else {
        panic!("a stroke: {items:?}");
    };
    assert_eq!(
        stroke.dashes.as_deref(),
        Some(&[1.0, 2.0, 3.0, 1.0, 2.0, 3.0][..])
    );
    assert_eq!(stroke.dash_offset, 1.0);
    assert_eq!(stroke.cap, StrokeLinecap::Round);
    assert_eq!(stroke.miter_limit, 1.0);
    let items = draw(
        "<svg width=100 height=100><path d='M0 0h10' stroke=red stroke-dasharray='0 0' />\
         <path d='M0 0h10' stroke=red stroke-width=0 /></svg>",
    );
    let strokes: Vec<_> = items
        .iter()
        .filter_map(|i| match i {
            SvgDrawItem::Stroke { stroke, .. } => Some(stroke.dashes.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(strokes, vec![None]);
}

/// Builds the content of the first `<svg>` of `html` with `budget`.
fn build_with(html: &str, budget: &mut SvgBudget) -> SvgContent {
    let doc = parse_html(html);
    let styles = styles_for(&doc);
    let svg = doc
        .descendants(doc.document_element().expect("a root"))
        .find(|&n| doc.element(n).is_some_and(|e| &**e.local_name() == "svg"))
        .expect("an svg element");
    build(&doc, &styles, svg, budget)
}

fn shapes(content: &SvgContent) -> usize {
    content
        .nodes
        .iter()
        .filter(|n| matches!(n, Node::Shape(_)))
        .count()
}

#[test]
fn shapes_are_limited_per_document() {
    let mut budget = SvgBudget {
        shapes: 3,
        ..SvgBudget::default()
    };
    let html = format!("<svg>{}</svg>", "<rect width=1 height=1 />".repeat(5));
    assert_eq!(shapes(&build_with(&html, &mut budget)), 3);
    assert!(budget.warned_shapes);
    // The budget is shared by the next `<svg>` of the same box tree.
    assert_eq!(shapes(&build_with(&html, &mut budget)), 0);
}

#[test]
fn segments_are_limited_per_document() {
    let mut budget = SvgBudget {
        segments: 12,
        ..SvgBudget::default()
    };
    let html = "<svg><path d='M0 0 L1 1 L2 2 L3 3 L4 4' /><rect width=1 height=1 />\
                <polyline points='0,0 1,1 2,2 3,3 4,4 5,5' /></svg>";
    let content = build_with(html, &mut budget);
    // The path takes 5 segments, the rectangle 10 of the 7 left: it is not
    // drawn, and nothing is left for the polyline.
    assert_eq!(shapes(&content), 1);
    assert!(budget.warned_segments);
    assert_eq!(budget.segments, 0);
}

#[test]
fn groups_are_limited_in_depth() {
    let mut budget = SvgBudget::default();
    let deep = |n: usize| {
        format!(
            "<svg>{}<rect width=1 height=1 />{}</svg>",
            "<g>".repeat(n),
            "</g>".repeat(n)
        )
    };
    assert_eq!(shapes(&build_with(&deep(MAX_DEPTH), &mut budget)), 1);
    assert!(!budget.warned_depth);
    assert_eq!(shapes(&build_with(&deep(MAX_DEPTH + 1), &mut budget)), 0);
    assert!(budget.warned_depth);
}

#[test]
fn opacity_layers_are_limited_per_document() {
    let mut budget = SvgBudget {
        layers: 2,
        ..SvgBudget::default()
    };
    // Three groups with two shapes, then a shape with a fill and a stroke.
    let group = "<g opacity=0.5><rect width=5 height=5 /><rect width=5 height=5 /></g>";
    let html = format!(
        "<svg width=10 height=10>{}<rect width=5 height=5 stroke=black opacity=0.5 /></svg>",
        group.repeat(3)
    );
    let content = build_with(&html, &mut budget);
    assert!(budget.warned_layers);
    assert_eq!(budget.layers, 0);
    let layers: Vec<bool> = content
        .nodes
        .iter()
        .filter_map(|n| match n {
            Node::BeginGroup { layer, .. } => Some(*layer),
            _ => None,
        })
        .collect();
    assert_eq!(layers, [true, true, false]);
    let mut out = Vec::new();
    content.draw(Size::new(10.0, 10.0), &mut out);
    let pushes = out
        .iter()
        .filter(|i| matches!(i, SvgDrawItem::PushOpacity(_)))
        .count();
    let pops = out
        .iter()
        .filter(|i| matches!(i, SvgDrawItem::PopOpacity))
        .count();
    assert_eq!((pushes, pops), (2, 2));
    // Past the limit, the opacity multiplies the alpha of each paint: the
    // third group's two fills and the last shape's fill and stroke.
    let alphas: Vec<u8> = out
        .iter()
        .rev()
        .filter_map(|i| match i {
            SvgDrawItem::Fill { color, .. } | SvgDrawItem::Stroke { color, .. } => Some(color.a),
            _ => None,
        })
        .take(4)
        .collect();
    assert_eq!(alphas, [128, 128, 128, 128]);
}

#[test]
fn empty_groups_are_dropped() {
    let content = build_with(
        "<svg><g><g opacity=0.5></g><title>x</title></g></svg>",
        &mut SvgBudget::default(),
    );
    assert_eq!(content.nodes, []);
}
