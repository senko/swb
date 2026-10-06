//! Grid layout tests on whole documents. Geometry against Chromium is in
//! `tests/layout/grid-*.html`; these tests cover behaviour that the
//! layout tests do not show: caching, limits and paint order.

use std::time::{Duration, Instant};

use crate::FragmentRef;
use crate::test_support::{body, layout_html};

#[test]
fn wikipedia_page_skeleton() {
    // `.mw-page-container-inner` at 1120px..1679px.
    let l = layout_html(&body(
        "<style>#g { display:grid; column-gap:24px; \
         grid-template: min-content 1fr min-content / 196px minmax(0,1fr); \
         grid-template-areas: 'siteNotice siteNotice' 'columnStart pageContent' \
         'footer footer' } #n { grid-area: siteNotice } #s { grid-area: columnStart } \
         #c { grid-area: pageContent } #f { grid-area: footer }</style>\
         <div id=g><div id=n>notice</div><div id=s style='height:300px'>toc</div>\
         <div id=c style='height:500px'>article</div><div id=f>footer</div></div>",
    ));
    let rect = |id: &str| {
        let r = l.rect(id);
        (r.x, r.y, r.width, r.height)
    };
    assert_eq!(rect("n"), (0.0, 0.0, 800.0, 20.0));
    assert_eq!(rect("s"), (0.0, 20.0, 196.0, 300.0));
    assert_eq!(rect("c"), (220.0, 20.0, 580.0, 500.0));
    assert_eq!(rect("f"), (0.0, 520.0, 800.0, 20.0));
    assert_eq!(l.rect("g").height, 540.0);
}

#[test]
fn flexible_tracks_share_the_space_after_gaps() {
    let l = layout_html(&body(
        "<div style='display:grid; width:430px; grid-template-columns: 1fr 2fr 100px; \
         column-gap:15px'><div id=a>a</div><div id=b>b</div><div id=c>c</div></div>",
    ));
    assert_eq!((l.rect("a").x, l.rect("a").width), (0.0, 100.0));
    assert_eq!((l.rect("b").x, l.rect("b").width), (115.0, 200.0));
    assert_eq!((l.rect("c").x, l.rect("c").width), (330.0, 100.0));
}

#[test]
fn auto_placement_creates_implicit_rows() {
    let l = layout_html(&body(
        "<div style='display:grid; grid-template-columns: 50px 50px; grid-auto-rows: 10px 30px'>\
         <div id=a></div><div id=b></div><div id=c></div><div id=d style='grid-column: span 2'>\
         </div></div>",
    ));
    assert_eq!(
        (l.rect("c").x, l.rect("c").y, l.rect("c").height),
        (0.0, 10.0, 30.0)
    );
    assert_eq!(
        (l.rect("d").y, l.rect("d").width, l.rect("d").height),
        (40.0, 100.0, 10.0)
    );
}

#[test]
fn item_alignment_and_auto_margins() {
    let l = layout_html(&body(
        "<div style='display:grid; grid-template-columns: 200px; grid-template-rows: 100px; \
         justify-items:center'><div id=a style='grid-area: 1 / 1; width:50px; height:20px; \
         align-self:end'></div>\
         <div id=b style='grid-area: 1 / 1; width:50px; height:20px; margin-left:auto'></div>\
         <div id=c style='grid-area: 1 / 1; justify-self:stretch'>c</div></div>",
    ));
    assert_eq!((l.rect("a").x, l.rect("a").y), (75.0, 80.0));
    assert_eq!((l.rect("b").x, l.rect("b").y), (150.0, 0.0));
    let c = l.rect("c");
    assert_eq!((c.width, c.height), (200.0, 100.0));
}

#[test]
fn intrinsic_sizes_of_a_grid() {
    let l = layout_html(&body(
        "<div><div id=g style='display:inline-grid; grid-template-columns: auto 30px; \
         column-gap:5px'><div style='width:40px'></div><div></div></div></div>\
         <div id=f style='float:left; display:grid; grid-template-columns: repeat(2, 1fr)'>\
         <div style='width:70px'></div><div style='width:10px'></div></div>",
    ));
    assert_eq!(l.rect("g").width, 75.0);
    // Two equal columns as wide as the widest item's max-content.
    assert_eq!(l.rect("f").width, 140.0);
}

#[test]
fn order_changes_the_paint_order() {
    let l = layout_html(&body(
        "<div style='display:grid'><div id=a style='order:2'>a</div><div id=b>b</div></div>",
    ));
    assert!(l.rect("b").y < l.rect("a").y);
    let (a, b) = (l.node("a"), l.node("b"));
    let mut order = Vec::new();
    l.tree.walk(|f, _| {
        if let FragmentRef::Box(f) = f
            && let Some(n) = f.node
            && (n == a || n == b)
        {
            order.push(n);
        }
    });
    assert_eq!(order, [b, a]);
}

#[test]
fn empty_auto_fit_tracks_collapse() {
    let l = layout_html(&body(
        "<div style='display:grid; width:500px; grid-template-columns: repeat(auto-fit, 100px); \
         justify-content:center; column-gap:10px'><div id=a>a</div><div id=b>b</div></div>",
    ));
    // Four repetitions fit; the two empty ones collapse with their gaps.
    assert_eq!(l.rect("a").x, 145.0);
    assert_eq!(l.rect("b").x, 255.0);
}

#[test]
fn absolutely_positioned_children_get_a_box() {
    let l = layout_html(&body(
        "<div id=g style='display:grid; position:relative; grid-template-columns: 100px; \
         padding: 10px; margin-top: 5px'><div>x</div><span id=a style='position:absolute'>abs\
         </span></div>",
    ));
    let a = l.rect("a");
    assert!(a.width > 0.0 && a.height > 0.0);
    // The static position is the start of the padding box (§10.2).
    let g = l.rect("g");
    assert_eq!((a.x, a.y), (g.x, g.y));
}

#[test]
fn nested_grids_lay_out_items_a_bounded_number_of_times() {
    const DEPTH: usize = 30;
    let open = "<div style='display:grid; grid-template-columns: auto 1fr'>".repeat(DEPTH);
    let l = layout_html(&body(&format!("{open}x")));
    assert!(
        l.uncached_layouts <= 8 * DEPTH,
        "{} layouts",
        l.uncached_layouts
    );
    // Stretched rows: every level has a sibling that sets the row height.
    let mut html = String::from("x");
    for i in 0..DEPTH {
        html = format!(
            "<div style='display:grid; grid-template-columns: auto auto'><div>{html}</div>\
             <div style='height:{}px'>b</div></div>",
            100 + 10 * i
        );
    }
    let l = layout_html(&body(&html));
    assert!(
        l.uncached_layouts <= 8 * DEPTH,
        "{} layouts",
        l.uncached_layouts
    );
}

/// Lays out `html` and fails if it takes longer than `limit` (generous,
/// for slow debug builds) or produces non-finite geometry.
fn layout_bounded(html: &str, limit: Duration) -> crate::test_support::TestLayout {
    let start = Instant::now();
    let l = layout_html(html);
    let elapsed = start.elapsed();
    assert!(elapsed < limit, "layout took {elapsed:?}");
    let mut finite = true;
    l.tree.walk(|f, origin| {
        let r = match f {
            FragmentRef::Box(b) => b.border_rect,
            FragmentRef::Text(t) => t.rect,
        };
        finite &= [origin.x, origin.y, r.x, r.y, r.width, r.height]
            .iter()
            .all(|v| v.is_finite());
    });
    assert!(finite);
    l
}

#[test]
fn hostile_templates_and_lines_are_bounded() {
    let grid = "<div class=g><div style='grid-row: 9999999 / -9999999'>a</div>\
                <div style='grid-column: span 2147483647'>b</div>\
                <div style='grid-area: x 99999 / y -99999 / span z 99999'>c</div></div>";
    let html = format!(
        "<!DOCTYPE html><style>.g {{ display:grid; \
         grid-template-columns: repeat(100000, [x y] 1px 2fr); \
         grid-template-rows: repeat(auto-fill, minmax(1px, 1e9px)); \
         grid-auto-rows: 1px 2px 3px; gap: 1e30px }}</style>{}",
        grid.repeat(200)
    );
    layout_bounded(&html, Duration::from_secs(20));
}

#[test]
fn many_spanning_items_are_bounded() {
    let items = "<div style='grid-column: 1 / -1; grid-row: span 10000'>x</div>".repeat(2000);
    let html = format!(
        "<!DOCTYPE html><div style='display:grid; grid-template-columns: repeat(10000, auto); \
         grid-auto-flow: dense'>{items}<div style='grid-column: span 3'>y</div></div>"
    );
    layout_bounded(&html, Duration::from_secs(20));
}

#[test]
fn many_auto_placed_items() {
    let items = "<div>x</div><div style='grid-column: span 2'>y</div>".repeat(3000);
    let html = format!(
        "<!DOCTYPE html><div style='display:grid; grid-template-columns: repeat(3, 1fr); \
         grid-auto-flow: row dense'>{items}</div>"
    );
    layout_bounded(&html, Duration::from_secs(20));
}

/// `100px repeat(auto-fill, 50px)`.
fn auto_fill_list() -> swb_style::TrackList {
    use std::sync::Arc;
    use swb_style::{
        LengthPercentage, RepeatCount, TrackBreadth, TrackList, TrackListEntry, TrackListValue,
        TrackRepeat, TrackSize,
    };
    let px = |v: f32| TrackSize::Breadth(TrackBreadth::Length(LengthPercentage::Px(v)));
    TrackList::new(
        Arc::from([
            TrackListEntry {
                names: Arc::from([]),
                value: TrackListValue::Track(px(100.0)),
            },
            TrackListEntry {
                names: Arc::from([]),
                value: TrackListValue::Repeat(TrackRepeat {
                    count: RepeatCount::AutoFill,
                    tracks: Arc::from([px(50.0)]),
                    names: Arc::from([Arc::from([]), Arc::from([])]),
                }),
            },
        ]),
        Arc::from([]),
    )
}

#[test]
fn many_names_shared_by_many_grids_are_cheap() {
    // Each grid looks up only the names that its items use, so the cost
    // does not grow with the number of names times the number of grids.
    let names: Vec<String> = (0..4000).map(|i| format!("a{i}")).collect();
    let line_names: Vec<String> = (0..4000).map(|i| format!("n{i}")).collect();
    let grids = "<div class=g><div style='grid-area: a5'>x</div>\
                 <div style='grid-column: n7'>y</div></div>"
        .repeat(1000);
    let html = format!(
        "<!DOCTYPE html><style>.g {{ display:grid; grid-template-areas: '{}'; \
         grid-template-columns: [{}] 10px; justify-content: start }} \
         .g > * {{ width: 5px }}</style>\
         <div class=g><div id=a style='grid-area: a3'></div>\
         <div id=n style='grid-column: n7'></div></div>{grids}",
        names.join(" "),
        line_names.join(" ")
    );
    let l = layout_bounded(&html, Duration::from_secs(5));
    // Every line name is on line 1, so `n` is in the first column; area
    // `a3` is in column 4, after the 10px column and two empty ones.
    assert_eq!(l.rect("a").x - l.rect("n").x, 10.0);
}

#[test]
fn automatic_repetitions() {
    use super::auto_repetitions as reps;
    let list = auto_fill_list();
    // 100 + n * (50 + 10) - 10 <= 400: n = 5.
    assert_eq!(reps(&list, 10.0, Some(400.0), 0.0, None), 5);
    // At least one, even if nothing fits.
    assert_eq!(reps(&list, 0.0, Some(20.0), 0.0, None), 1);
    // Indefinite: enough to reach the minimum (ceil), or as many as fit
    // into the maximum (floor).
    assert_eq!(reps(&list, 0.0, None, 230.0, None), 3);
    assert_eq!(reps(&list, 0.0, None, 230.0, Some(290.0)), 3);
    assert_eq!(reps(&list, 0.0, None, 0.0, None), 1);
    // Hostile sizes stay within the limited grid.
    assert_eq!(reps(&list, 0.0, Some(f32::NAN), 0.0, None), 1);
    let huge = reps(&list, 0.0, Some(f32::INFINITY), 0.0, None);
    assert!(huge >= 1 && huge <= super::MAX_LINE as u32);
    assert_eq!(
        reps(
            &swb_style::TrackList::default(),
            0.0,
            Some(400.0),
            0.0,
            None
        ),
        0
    );
}
