//! Tests of inline SVG: box construction, content, drawing commands and
//! limits.

use std::fmt::Write as _;
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

/// The drawing commands of the first `<svg>` of a document, without the
/// hit commands.
fn draw(html: &str) -> Vec<SvgDrawItem> {
    let mut items = draw_all(html);
    items.retain(|i| !matches!(i, SvgDrawItem::Hit(_)));
    items
}

/// All drawing commands of the first `<svg>` of a document.
fn draw_all(html: &str) -> Vec<SvgDrawItem> {
    let (svg, size) = svg_content(html);
    let mut out = Vec::new();
    svg.draw(size, &mut out);
    out
}

#[test]
fn svg_is_a_replaced_box_and_its_descendants_have_boxes_only_in_the_svg_namespace() {
    let layout = layout_html(&body(
        "<svg id=s width=40 height=20><title>Title</title><g id=g><path id=p d='M0 0h10v10z'/>\
         </g>loose text<foreignObject><div id=d>html</div></foreignObject></svg>after",
    ));
    assert_eq!(layout.rect("s"), Rect::new(0.0, 0.0, 40.0, 20.0));
    let boxes = layout.tree.element_boxes();
    // `g` and `path` have their bounding boxes; the HTML content of a
    // `foreignObject` is not laid out.
    assert_eq!(boxes[&layout.node("g")], Rect::new(0.0, 0.0, 10.0, 10.0));
    assert_eq!(boxes[&layout.node("p")], Rect::new(0.0, 0.0, 10.0, 10.0));
    assert!(!boxes.contains_key(&layout.node("d")), "#d has a box");
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
            ..
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
            SvgDrawItem::PushClip(_) => "clip".to_owned(),
            SvgDrawItem::PopClip => "unclip".to_owned(),
            SvgDrawItem::Hit(_) => "hit".to_owned(),
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
        shapes: Counter::new(3),
        ..SvgBudget::default()
    };
    let html = format!("<svg>{}</svg>", "<rect width=1 height=1 />".repeat(5));
    assert_eq!(shapes(&build_with(&html, &mut budget)), 3);
    assert!(budget.shapes.warned);
    // The budget is shared by the next `<svg>` of the same box tree.
    assert_eq!(shapes(&build_with(&html, &mut budget)), 0);
}

#[test]
fn segments_are_limited_per_document() {
    let mut budget = SvgBudget {
        segments: Counter::new(12),
        ..SvgBudget::default()
    };
    let html = "<svg><path d='M0 0 L1 1 L2 2 L3 3 L4 4' /><rect width=1 height=1 />\
                <polyline points='0,0 1,1 2,2 3,3 4,4 5,5' /></svg>";
    let content = build_with(html, &mut budget);
    // The path takes 5 segments, the rectangle 10 of the 7 left: it is not
    // drawn, and nothing is left for the polyline (a shape without a path,
    // which has only a box).
    let drawn = content
        .nodes
        .iter()
        .filter(|n| matches!(n, Node::Shape(s) if matches!(s.geometry, Geometry::Path(_))))
        .count();
    assert_eq!(drawn, 1);
    assert_eq!(shapes(&content), 2);
    assert!(budget.segments.warned);
    assert_eq!(budget.segments.left, 0);
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
    assert!(!budget.depth.warned);
    assert_eq!(shapes(&build_with(&deep(MAX_DEPTH + 1), &mut budget)), 0);
    assert!(budget.depth.warned);
}

#[test]
fn opacity_layers_are_limited_per_document() {
    let mut budget = SvgBudget {
        layers: Counter::new(2),
        ..SvgBudget::default()
    };
    // Three groups with two shapes, then a shape with a fill and a stroke.
    let group = "<g opacity=0.5><rect width=5 height=5 /><rect width=5 height=5 /></g>";
    let html = format!(
        "<svg width=10 height=10>{}<rect width=5 height=5 stroke=black opacity=0.5 /></svg>",
        group.repeat(3)
    );
    let content = build_with(&html, &mut budget);
    assert!(budget.layers.warned);
    assert_eq!(budget.layers.left, 0);
    let layers: Vec<bool> = content
        .nodes
        .iter()
        .filter_map(|n| match n {
            Node::BeginGroup(group) => Some(group.layer),
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
fn empty_groups_draw_nothing_but_keep_their_box() {
    let (svg, size) =
        svg_content("<svg width=10 height=10><g><g opacity=0.5></g><title>x</title></g></svg>");
    let mut out = Vec::new();
    svg.draw(size, &mut out);
    assert_eq!(out, []);
    assert_eq!(svg.element_boxes(size).len(), 2);
}

/// The clip commands of `items`: `rect x y w h` for a clip rectangle,
/// `path n` for a clip path of `n` shapes, and `unclip`.
fn clip_commands(items: &[SvgDrawItem]) -> Vec<String> {
    items
        .iter()
        .filter_map(|i| match i {
            SvgDrawItem::PushClip(ClipRegion::Rect(r)) => {
                Some(format!("rect {} {} {} {}", r.x, r.y, r.width, r.height))
            }
            SvgDrawItem::PushClip(ClipRegion::Path(p)) => Some(format!("path {}", p.shapes.len())),
            SvgDrawItem::PopClip => Some("unclip".to_owned()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_rectangular_clip_path_is_a_clip_rectangle() {
    // The Ars icon: a clip path of the size of the view box on a group.
    let items = draw(
        "<svg width=40 height=40 viewBox='0 0 20 20'><defs><clipPath id=a>\
         <path d='M0 0h20v20H0z'/></clipPath></defs>\
         <g clip-path='url(#a)'><rect width=5 height=5 /></g></svg>",
    );
    assert_eq!(clip_commands(&items), ["rect 0 0 40 40", "unclip"]);
    // The clip follows the transform of the clipped element; the CSS
    // property is the same as the attribute.
    let items = draw(
        "<svg width=100 height=100><clipPath id=a><rect width=10 height=20 /></clipPath>\
         <rect width=50 height=50 transform='translate(5 6) scale(2)' style='clip-path: url(#a)' />\
         </svg>",
    );
    assert_eq!(clip_commands(&items), ["rect 5 6 20 40", "unclip"]);
    // A rotation makes it a path (and takes a layer from the budget).
    let items = draw(
        "<svg width=100 height=100><clipPath id=a><rect width=10 height=20 /></clipPath>\
         <rect width=50 height=50 transform='rotate(10)' clip-path='url(#a)' /></svg>",
    );
    assert_eq!(clip_commands(&items), ["path 1", "unclip"]);
}

#[test]
fn clip_paths_follow_chromium() {
    let commands = |html: &str| clip_commands(&draw(html));
    let shapes = "<clipPath id=a><circle cx=5 cy=5 r=5 /><rect width=2 height=2 />\
                  <g><rect width=9 height=9 /></g><rect width=3 height=3 style='display:none' />\
                  <rect width=4 height=4 visibility=hidden /><text>x</text></clipPath>";
    // Two shapes count; `g`, `display: none`, hidden and text do not.
    assert_eq!(
        commands(&format!(
            "<svg width=10 height=10>{shapes}<rect width=9 height=9 clip-path='url(#a)' /></svg>"
        )),
        ["path 2", "unclip"]
    );
    // An empty clip path clips everything.
    assert_eq!(
        commands(
            "<svg width=10 height=10><clipPath id=a></clipPath>\
             <rect width=9 height=9 clip-path='url(#a)' /></svg>"
        ),
        ["rect 0 0 0 0", "unclip"]
    );
    // A missing reference, another element, `url(a)`, two urls, `none`
    // and a clip path with `display: none` leave the element unclipped.
    let none = Vec::<String>::new();
    for reference in [
        "url(#missing)",
        "url(#r)",
        "url(a)",
        "url(#a) url(#a)",
        "#a",
        "none",
    ] {
        assert_eq!(
            commands(&format!(
                "<svg width=10 height=10><rect id=r width=1 height=1 />\
                 <clipPath id=a><rect width=2 height=2 /></clipPath>\
                 <rect width=9 height=9 clip-path='{reference}' /></svg>"
            )),
            none,
            "{reference}"
        );
    }
    assert_eq!(
        commands(
            "<svg width=10 height=10><clipPath id=a style='display:none'><rect width=2 height=2 />\
             </clipPath><rect width=9 height=9 clip-path='url(#a)' /></svg>"
        ),
        none
    );
    // A clip path in a `display: none` subtree is not valid either.
    assert_eq!(
        commands(
            "<svg width=10 height=10 style='display:none'><clipPath id=a><rect width=2 height=2 />\
             </clipPath></svg><svg width=10 height=10>\
             <rect width=9 height=9 clip-path='url(#a)' /></svg>"
        ),
        none
    );
}

#[test]
fn clip_paths_use_units_transforms_and_nesting() {
    // `objectBoundingBox`: the unit square is the fill bounding box (a
    // group's is the union of its children's).
    let items = draw(
        "<svg width=100 height=100><clipPath id=a clipPathUnits=objectBoundingBox>\
         <rect width=0.5 height=0.5 /></clipPath>\
         <g clip-path='url(#a)'><rect x=10 y=10 width=20 height=20 />\
         <rect x=50 y=50 width=40 height=40 /></g></svg>",
    );
    assert_eq!(clip_commands(&items)[0], "rect 10 10 40 40");
    // The `transform` of the clip path is outside the bounding box.
    let items = draw(
        "<svg width=100 height=100><clipPath id=a clipPathUnits=objectBoundingBox \
         transform='translate(1 2)'><rect width=0.5 height=0.5 /></clipPath>\
         <rect x=10 y=10 width=20 height=20 clip-path='url(#a)' /></svg>",
    );
    assert_eq!(clip_commands(&items)[0], "rect 11 12 10 10");
    // An empty bounding box clips everything.
    let items = draw(
        "<svg width=100 height=100><clipPath id=a clipPathUnits=objectBoundingBox>\
         <rect width=1 height=1 /></clipPath><line x2=10 stroke=red clip-path='url(#a)' /></svg>",
    );
    assert_eq!(clip_commands(&items)[0], "rect 0 0 0 0");
    // `clip-path` on a clip path intersects; a cycle is cut.
    let items = draw(
        "<svg width=100 height=100><clipPath id=a><rect width=60 height=60 /></clipPath>\
         <clipPath id=b clip-path='url(#a)'><rect x=30 y=30 width=60 height=60 /></clipPath>\
         <rect width=100 height=100 clip-path='url(#b)' /></svg>",
    );
    assert_eq!(clip_commands(&items)[0], "rect 30 30 30 30");
    let items = draw(
        "<svg width=100 height=100><clipPath id=a clip-path='url(#b)'><rect width=60 height=60 /></clipPath>\
         <clipPath id=b clip-path='url(#a)'><rect width=60 height=60 /></clipPath>\
         <rect width=100 height=100 clip-path='url(#a)' /></svg>",
    );
    assert_eq!(clip_commands(&items)[0], "rect 0 0 60 60");
}

#[test]
fn clip_rule_is_inherited_from_the_clip_path() {
    let rule = |html: &str| {
        draw(html).into_iter().find_map(|i| match i {
            SvgDrawItem::PushClip(ClipRegion::Path(p)) => p.shapes.first().map(|s| s.rule),
            _ => None,
        })
    };
    let ring = "<path d='M0 0H40V40H0Z M10 10H30V30H10Z' ";
    assert_eq!(
        rule(&format!(
            "<svg width=40 height=40><clipPath id=a clip-rule=evenodd>{ring}/></clipPath>\
             <rect width=40 height=40 clip-path='url(#a)' /></svg>"
        )),
        Some(FillRule::EvenOdd)
    );
    // The `clip-rule` of the clipped element does not matter, and a
    // `fill-rule` does not either.
    assert_eq!(
        rule(&format!(
            "<svg width=40 height=40><clipPath id=a>{ring} fill-rule=evenodd /></clipPath>\
             <rect width=40 height=40 clip-rule=evenodd clip-path='url(#a)' /></svg>"
        )),
        Some(FillRule::NonZero)
    );
    // It is inherited from the clip path's ancestors.
    assert_eq!(
        rule(&format!(
            "<svg width=40 height=40 clip-rule=evenodd><clipPath id=a>{ring}/></clipPath>\
             <rect width=40 height=40 clip-path='url(#a)' /></svg>"
        )),
        Some(FillRule::EvenOdd)
    );
}

#[test]
fn clip_paths_that_need_a_layer_are_limited_per_document() {
    let mut budget = SvgBudget {
        clip_layers: Counter::new(2),
        ..SvgBudget::default()
    };
    let circles = "<rect width=9 height=9 clip-path='url(#c)' />".repeat(4);
    // A circle is never a rectangle; a rectangle clip needs no layer.
    let html = format!(
        "<svg width=10 height=10><clipPath id=c><circle r=5 /></clipPath>\
         <clipPath id=r><rect width=5 height=5 /></clipPath>{circles}\
         <rect width=9 height=9 clip-path='url(#r)' /></svg>"
    );
    let content = build_with(&html, &mut budget);
    assert!(budget.clip_layers.warned);
    let clips: Vec<bool> = content
        .nodes
        .iter()
        .filter_map(|n| match n {
            Node::Shape(s) => Some(s.clip.is_some()),
            _ => None,
        })
        .collect();
    // Two circles clip, two do not, the rectangle clips.
    assert_eq!(clips, [true, true, false, false, true]);
}

#[test]
fn clip_path_references_are_limited_in_depth() {
    let chain = |n: usize| {
        let mut paths = String::new();
        for i in 0..n {
            let _ = write!(
                paths,
                "<clipPath id=k{i} clip-path='url(#k{})'><rect width=50 height=50 /></clipPath>",
                i + 1
            );
        }
        format!(
            "<svg width=100 height=100>{paths}<clipPath id=k{n}><rect width=20 height=20 /></clipPath>\
             <rect width=100 height=100 clip-path='url(#k0)' /></svg>"
        )
    };
    // Within the limit, the last clip path of the chain counts: 20 x 20.
    assert_eq!(
        clip_commands(&draw(&chain(MAX_CLIP_DEPTH - 1)))[0],
        "rect 0 0 20 20"
    );
    // Past it, the chain ends early (the rest of it is dropped).
    assert_eq!(clip_commands(&draw(&chain(100)))[0], "rect 0 0 50 50");
}

#[test]
fn use_draws_a_translated_copy_with_inherited_styles() {
    let items = draw(
        "<svg width=100 height=100 fill=red><defs><rect id=r width=5 height=5 />\
         <g id=g fill=blue><use href='#r' x=1 /></g></defs>\
         <use href='#r' x=10 y=20 fill=green /><use xlink:href='#g' x=50 /><use href='#none' /></svg>",
    );
    let fills: Vec<(Rgba, Matrix)> = items
        .iter()
        .filter_map(|i| match i {
            SvgDrawItem::Fill {
                color, transform, ..
            } => Some((*color, *transform)),
            _ => None,
        })
        .collect();
    // The first `use`: green, from the `use` element, translated. The
    // second: the `use` inside the copy of the group inherits blue from
    // that copy, and both translations add up.
    assert_eq!(
        fills,
        vec![
            (Rgba::rgb(0, 128, 0), Matrix::translate(10.0, 20.0)),
            (Rgba::rgb(0, 0, 255), Matrix::translate(51.0, 0.0)),
        ]
    );
}

#[test]
fn the_box_dump_follows_chromium() {
    let (svg, size) = svg_content(
        "<svg id=s width=200 height=100 viewBox='0 0 100 50'><g id=g1><rect id=a x=10 y=10 \
         width=20 height=10 stroke=red stroke-width=4 /><circle id=b cx=50 cy=20 r=10 /></g>\
         <g id=g2 transform='rotate(45)'><rect id=c width=10 height=10 /><rect id=d x=90 y=90 \
         width=10 height=10 /></g><g id=e><rect width=0 height=10 x=50 y=1 /></g>\
         <path id=f d='M80 80' /><path id=h /><use id=u href='#none' x=7 y=9 /></svg>",
    );
    let boxes = svg.element_boxes(size);
    let rects: Vec<Rect> = boxes.iter().map(|(_, r)| *r).collect();
    let near = |a: &Rect, b: Rect| {
        (a.x - b.x).abs() < 0.01
            && (a.y - b.y).abs() < 0.01
            && (a.width - b.width).abs() < 0.01
            && (a.height - b.height).abs() < 0.01
    };
    // g1: fill bounding boxes, strokes ignored, under the 2x view box.
    assert!(
        near(&rects[0], Rect::new(20.0, 20.0, 100.0, 40.0)),
        "{rects:?}"
    );
    assert!(near(&rects[1], Rect::new(20.0, 20.0, 40.0, 20.0)));
    assert!(near(&rects[2], Rect::new(80.0, 20.0, 40.0, 40.0)));
    // g2: the matrix applied to the union (not the union of the
    // children's boxes).
    assert!(
        near(&rects[3], Rect::new(-141.42, 0.0, 282.84, 282.84)),
        "{:?}",
        rects[3]
    );
    // A rect of zero width has a box; the group of only that one is empty,
    // at the origin.
    assert!(
        near(&rects[6], Rect::new(0.0, 0.0, 0.0, 0.0)),
        "{:?}",
        rects[6]
    );
    assert!(
        near(&rects[7], Rect::new(100.0, 2.0, 0.0, 20.0)),
        "{:?}",
        rects[7]
    );
    // A path of one point; a path without data; a `use` without target.
    assert!(
        near(&rects[8], Rect::new(160.0, 160.0, 0.0, 0.0)),
        "{:?}",
        rects[8]
    );
    assert!(near(&rects[9], Rect::new(0.0, 0.0, 0.0, 0.0)));
    assert!(
        near(&rects[10], Rect::new(14.0, 18.0, 0.0, 0.0)),
        "{:?}",
        rects[10]
    );
}

#[test]
fn pointer_events_decide_what_a_shape_reports_for_hit_testing() {
    type Hits = Vec<(Option<FillRule>, Option<f32>)>;
    let hits = |html: &str| -> Hits {
        draw_all(html)
            .into_iter()
            .filter_map(|i| match i {
                SvgDrawItem::Hit(h) => Some((h.fill, h.stroke_width)),
                _ => None,
            })
            .collect()
    };
    let rule = Some(FillRule::NonZero);
    assert_eq!(
        hits("<svg width=10 height=10><rect width=5 height=5 /></svg>"),
        [(rule, None)]
    );
    // visiblePainted: the stroke takes part with a painted stroke only.
    assert_eq!(
        hits("<svg width=10 height=10><rect width=5 height=5 stroke=red stroke-width=2 /></svg>"),
        [(rule, Some(2.0))]
    );
    let nothing = Hits::new();
    assert_eq!(
        hits("<svg width=10 height=10><rect width=5 height=5 fill=none /></svg>"),
        nothing
    );
    assert_eq!(
        hits(
            "<svg width=10 height=10><rect width=5 height=5 fill=none pointer-events=all /></svg>"
        ),
        [(rule, Some(1.0))]
    );
    assert_eq!(
        hits("<svg width=10 height=10><rect width=5 height=5 pointer-events=none /></svg>"),
        nothing
    );
    assert_eq!(
        hits("<svg width=10 height=10><rect width=5 height=5 visibility=hidden /></svg>"),
        nothing
    );
    assert_eq!(
        hits(
            "<svg width=10 height=10><rect width=5 height=5 visibility=hidden \
             pointer-events=painted /></svg>"
        ),
        [(rule, None)]
    );
    // A copy that a `use` draws is not an element of the page: it reports
    // the `use` element (a hit command exists; see the test of instances).
    assert_eq!(
        hits(
            "<svg width=10 height=10><defs><rect id=r width=5 height=5 /></defs>\
             <use href='#r' /></svg>"
        ),
        [(rule, None)]
    );
}

#[test]
fn shape_rendering_turns_anti_aliasing_off_for_the_two_speed_values() {
    let flags = |value: &str| {
        let html = format!(
            "<svg width=10 height=10 shape-rendering='{value}'><g><rect width=5 height=5 /></g></svg>"
        );
        draw(&html).into_iter().find_map(|i| match i {
            SvgDrawItem::Fill { anti_alias, .. } => Some(anti_alias),
            _ => None,
        })
    };
    assert_eq!(flags("auto"), Some(true));
    assert_eq!(flags("geometricPrecision"), Some(true));
    assert_eq!(flags("optimizeSpeed"), Some(false));
    assert_eq!(flags("crispEdges"), Some(false));
    assert_eq!(flags("CRISPEDGES"), Some(false));
    assert_eq!(flags("bogus"), Some(true));
}

#[test]
fn a_use_in_a_clip_path_keeps_the_clip_of_its_target() {
    // Chromium: ink 304.74 (the circle clips the 50 x 50 rect).
    let items = draw(
        "<svg width=100 height=100><defs><clipPath id=c1><circle cx=20 cy=20 r=10 /></clipPath>\
         <rect id=r width=50 height=50 clip-path='url(#c1)'/>\
         <clipPath id=a><use href='#r' /></clipPath></defs>\
         <rect width=100 height=100 clip-path='url(#a)'/></svg>",
    );
    assert_eq!(clip_commands(&items), ["path 1", "unclip"]);
    let Some(SvgDrawItem::PushClip(ClipRegion::Path(p))) =
        items.iter().find(|i| matches!(i, SvgDrawItem::PushClip(_)))
    else {
        panic!("a clip path")
    };
    assert!(p.shapes[0].clip.is_some());
}

#[test]
fn use_takes_both_groups_or_none() {
    let mut budget = SvgBudget {
        groups: Counter::new(1),
        ..SvgBudget::default()
    };
    let html = "<svg><rect id=r width=1 height=1 /><use href='#r' /></svg>";
    let _ = build_with(html, &mut budget);
    assert_eq!(budget.groups.left, 1);
    assert!(budget.groups.warned);
}

#[test]
fn groups_are_limited_per_document() {
    let mut budget = SvgBudget {
        groups: Counter::new(3),
        ..SvgBudget::default()
    };
    let html = format!(
        "<svg>{}</svg>",
        "<g><rect width=1 height=1 /></g>".repeat(5)
    );
    let content = build_with(&html, &mut budget);
    assert_eq!(shapes(&content), 3);
    assert!(budget.groups.warned);
}

/// The fill commands and the clip commands in order, as text.
fn fills_and_clips(items: &[SvgDrawItem]) -> Vec<String> {
    items
        .iter()
        .filter_map(|i| match i {
            SvgDrawItem::Fill { transform, .. } => Some(format!("fill {transform:?}")),
            SvgDrawItem::PushClip(ClipRegion::Rect(r)) => {
                Some(format!("clip {} {} {} {}", r.x, r.y, r.width, r.height))
            }
            SvgDrawItem::PushClip(ClipRegion::Path(_)) => Some("clip path".to_owned()),
            SvgDrawItem::PopClip => Some("pop".to_owned()),
            _ => None,
        })
        .collect()
}

fn count(items: &[SvgDrawItem], prefix: &str) -> usize {
    fills_and_clips(items)
        .iter()
        .filter(|c| c.starts_with(prefix))
        .count()
}

const SYMBOL: &str =
    "<symbol id=s viewBox='0 0 10 10'><rect width=10 height=10 fill=red /></symbol>";

#[test]
fn a_symbol_draws_only_through_use_as_a_viewport() {
    // Unused: nothing. Used: the viewBox transform into the viewport of
    // the `use` (meet, centered), clipped to it, inside the translation.
    let unused = draw(&format!("<svg width=100 height=100>{SYMBOL}</svg>"));
    assert_eq!(fills_and_clips(&unused), Vec::<String>::new());
    let items = draw(&format!(
        "<svg width=100 height=100>{SYMBOL}<use href='#s' x=20 y=10 width=40 height=20 /></svg>"
    ));
    let got = fills_and_clips(&items);
    assert_eq!(got.len(), 3, "{got:?}");
    assert_eq!(got[0], "clip 20 10 40 20");
    // Scale 2, the 20 x 20 content centered in 40 x 20: x 20 + 10 = 30.
    assert_eq!(
        got[1],
        format!(
            "fill {:?}",
            Matrix::translate(20.0, 10.0).multiply(&Matrix::new(2.0, 0.0, 0.0, 2.0, 10.0, 0.0))
        )
    );
    assert_eq!(got[2], "pop");
}

#[test]
fn a_symbol_instance_takes_its_size_from_the_use_the_symbol_or_the_viewport() {
    let clip = |symbol: &str, usage: &str| {
        let items = draw(&format!(
            "<svg width=100 height=100><symbol id=s {symbol}><rect width=1 height=1 /></symbol>\
             <use href='#s' {usage} /></svg>"
        ));
        fills_and_clips(&items)
            .into_iter()
            .next()
            .unwrap_or_default()
    };
    assert_eq!(clip("", ""), "clip 0 0 100 100");
    assert_eq!(clip("width=40 height=30", ""), "clip 0 0 40 30");
    assert_eq!(clip("width=40", ""), "clip 0 0 40 100");
    assert_eq!(clip("width=40 height=30", "width=20"), "clip 0 0 20 30");
    assert_eq!(clip("", "width=50% height=10%"), "clip 0 0 50 10");
    // A size of zero disables the instance, whatever its `overflow`.
    assert_eq!(clip("overflow=visible", "width=0"), "clip 0 0 0 0");
}

/// The commands of the symbol `s` (with `symbol` as its attributes) used by
/// `usage`, in a 100 x 100 `<svg>`.
fn symbol_items(symbol: &str, usage: &str) -> Vec<String> {
    fills_and_clips(&draw(&format!(
        "<svg width=100 height=100><symbol id=s viewBox='0 0 10 10' {symbol}>\
         <rect width=1 height=1 /></symbol><use href='#s' {usage} /></svg>"
    )))
}

#[test]
fn a_symbol_instance_is_placed_by_its_x_y_and_transform() {
    // Measured in Chromium 148 (`tools/probes/svg-symbol.json`, cases
    // `sym-xy*` and `sym-transform*`): `x` and `y` shift the viewport and
    // add to those of the `use`; the `transform` of the symbol applies
    // outside the shift.
    let clip = |symbol: &str, usage: &str| symbol_items(symbol, usage)[0].clone();
    assert_eq!(clip("x=30 y=20", "width=40 height=40"), "clip 30 20 40 40");
    assert_eq!(
        clip("x=30 y=5", "width=40 height=40 x=10 y=5"),
        "clip 40 10 40 40"
    );
    assert_eq!(
        clip("x=10% y=20%", "width=40 height=40"),
        "clip 10 20 40 40"
    );
    assert_eq!(clip("x=auto y=foo", "width=40 height=40"), "clip 0 0 40 40");
    assert_eq!(
        clip("transform='translate(20 20)'", "width=40 height=40"),
        "clip 20 20 40 40"
    );
    assert_eq!(
        clip("transform='scale(0.5)' x=20 y=20", "width=40 height=40"),
        "clip 10 10 20 20"
    );
}

#[test]
fn a_symbol_instance_with_a_bad_size_or_an_empty_view_box_draws_nothing() {
    // Measured in Chromium 148 (cases `use-width-*`, `sym-width-*`, `vb-zero-*`).
    // A zero-size viewport clips everything away: no fill is drawn.
    let nothing = |symbol: &str, usage: &str| {
        let items = symbol_items(symbol, usage);
        !items.iter().any(|i| i.starts_with("fill")) || items.iter().any(|i| i == "clip 0 0 0 0")
    };
    for bad in ["auto", "-5", "foo", ""] {
        assert!(
            nothing("", &format!("width='{bad}' height=40")),
            "use width={bad}"
        );
        assert!(
            nothing("", &format!("width=40 height='{bad}'")),
            "use height={bad}"
        );
    }
    assert!(nothing("width=30 height=30", "width=-5"));
    assert!(nothing("width=-5 height=-5", ""));
    // An invalid or `auto` size of the symbol counts as missing.
    assert_eq!(
        symbol_items("width=auto height=foo", "")[0],
        "clip 0 0 100 100"
    );
    assert_eq!(
        symbol_items("width=foo height=foo", "width=30 height=30")[0],
        "clip 0 0 30 30"
    );
    // A `viewBox` with a zero width or height: no instance at all.
    for vb in ["0 0 0 10", "0 0 10 0", "0,0,0,0"] {
        let items = fills_and_clips(&draw(&format!(
            "<svg width=100 height=100><symbol id=s viewBox='{vb}'><rect width=1 height=1 />\
             </symbol><use href='#s' width=40 height=40 /></svg>"
        )));
        assert!(items.is_empty(), "viewBox {vb}: {items:?}");
    }
}

#[test]
fn a_symbol_clips_unless_its_overflow_is_visible_or_auto() {
    let clips = |overflow: &str| {
        let items = draw(&format!(
            "<svg width=100 height=100><symbol id=s {overflow}><rect width=1 height=1 /></symbol>\
             <use href='#s' width=10 height=10 /></svg>"
        ));
        count(&items, "clip")
    };
    assert_eq!(clips(""), 1);
    assert_eq!(clips("overflow=hidden"), 1);
    assert_eq!(clips("overflow=scroll"), 1);
    assert_eq!(clips("overflow=visible"), 0);
    assert_eq!(clips("overflow=auto"), 0);
    assert_eq!(clips("style='overflow:visible'"), 0);
}

#[test]
fn symbol_content_inherits_from_the_use_and_the_symbol() {
    let items = draw(
        "<svg width=100 height=100 fill=red><symbol id=s viewBox='0 0 10 10'>\
         <rect width=1 height=1 /></symbol>\
         <symbol id=t fill=blue><rect width=1 height=1 /></symbol>\
         <use href='#s' fill=green width=10 height=10 />\
         <use href='#t' fill=green width=10 height=10 /></svg>",
    );
    let colors: Vec<Rgba> = items
        .iter()
        .filter_map(|i| match i {
            SvgDrawItem::Fill { color, .. } => Some(*color),
            _ => None,
        })
        .collect();
    assert_eq!(colors, vec![Rgba::rgb(0, 128, 0), Rgba::rgb(0, 0, 255)]);
}

#[test]
fn percentages_in_a_symbol_refer_to_its_view_box_or_viewport() {
    let width_of = |symbol: &str| {
        let items = draw(&format!(
            "<svg width=100 height=100><symbol id=s {symbol}><rect width=50% height=50% />\
             </symbol><use href='#s' width=40 height=40 /></svg>"
        ));
        items
            .iter()
            .find_map(|i| match i {
                SvgDrawItem::Fill {
                    path, transform, ..
                } => Some(transform.map_rect(&path.bounds()).width),
                _ => None,
            })
            .unwrap_or(-1.0)
    };
    // 50% of the viewBox (10 units) at the scale 2; 50% of the viewport.
    assert_eq!(width_of("viewBox='0 0 20 20'"), 20.0);
    assert_eq!(width_of(""), 20.0);
}

#[test]
fn symbols_nest_and_cycles_and_chains_are_bounded() {
    // A symbol that uses itself, and a cycle of two: no copy of the cycle.
    let items = draw(
        "<svg width=100 height=100><symbol id=a><rect width=1 height=1 /><use href='#a' /></symbol>\
         <symbol id=b><use href='#c' /></symbol><symbol id=c><use href='#b' /></symbol>\
         <use href='#a' /><use href='#b' /></svg>",
    );
    assert_eq!(count(&items, "fill"), 1);
    // Symbols in symbols are nested viewports.
    let items = draw(
        "<svg width=100 height=100><symbol id=i viewBox='0 0 10 10'><rect width=10 height=10 />\
         </symbol><symbol id=o viewBox='0 0 20 20'><use href='#i' x=10 y=10 width=10 height=10 />\
         </symbol><use href='#o' width=40 height=40 /></svg>",
    );
    assert_eq!(count(&items, "clip"), 2);
    // A chain longer than the limit of the style crate ends.
    let mut html = String::from("<svg width=100 height=100>");
    for i in 0..40 {
        write!(html, "<symbol id=s{i}><use href='#s{}' /></symbol>", i + 1).expect("write");
    }
    html.push_str("<symbol id=s40><rect width=1 height=1 /></symbol><use href='#s0' /></svg>");
    let items = draw(&html);
    assert!(count(&items, "fill") <= 1);
}

#[test]
fn the_shapes_of_an_instance_report_the_use_element_for_hit_testing() {
    let hits = |html: &str| -> Vec<NodeId> {
        draw_all(html)
            .iter()
            .filter_map(|i| match i {
                SvgDrawItem::Hit(h) => Some(h.node),
                _ => None,
            })
            .collect()
    };
    let one = hits(&format!(
        "<svg width=100 height=100>{SYMBOL}<use id=u href='#s' width=40 height=40 /></svg>"
    ));
    assert_eq!(one.len(), 1);
    // The `use` in the group draws its own copy (reported by `inner`); the
    // copy of the group has another copy in it, reported by `outer`.
    let nested = hits(
        "<svg width=100 height=100><symbol id=i><rect width=1 height=1 /></symbol>\
         <g id=g><use id=inner href='#i' /></g><use id=outer href='#g' /></svg>",
    );
    assert_eq!(nested.len(), 2);
    assert_ne!(nested[0], nested[1]);
}
