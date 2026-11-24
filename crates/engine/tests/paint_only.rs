//! The paint-only path (ADR 0044 section 9, M13 feature 4a): a restyle that
//! changes paint properties only keeps the layout; the fragments get the
//! new styles and the display list is built again. With the check mode on
//! (debug builds), every such frame also runs a full layout and display
//! list and panics at the first difference, so these tests drive many
//! kinds of boxes through state changes, script changes and animation
//! ticks and count the frames that took the path.

#![cfg(debug_assertions)]
// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{ClockMode, FlushCounters, Page, PageClock, Size, Url};

mod common;
use common::{CASE_PAGE, OnePage, new_page, on_page_thread, render_viewport, value};

const VIEW: Size = Size::new(400.0, 300.0);

/// What a case does between renders.
#[derive(Clone, Copy)]
enum Step {
    /// The pointer moves to (x, y).
    Move(f32, f32),
    /// The pointer leaves the page.
    Leave,
    /// A script runs.
    Js(&'static str),
    /// The event loop runs for this many milliseconds of virtual time.
    Wait(u64),
}

/// What a case did, from the counters.
struct Outcome {
    /// Paint-only refreshes of the fragments.
    refreshes: u32,
    /// Refreshes that could not keep the layout.
    fallbacks: u32,
    /// Layout runs.
    layouts: u32,
    /// Paint-only frames that the check mode compared.
    checks: u64,
}

fn counters(page: &Page) -> FlushCounters {
    page.flush_counters()
}

/// Loads `html` in virtual time with animations on, runs `steps` with a
/// render after each, in check mode.
fn run(html: &'static str, steps: &[Step]) -> Outcome {
    swb_engine::set_restyle_check(true);
    let mut page = new_page(Arc::new(OnePage::new(html)), 1, VIEW);
    page.set_javascript(true);
    page.set_animations(true);
    page.set_clock(PageClock::new(ClockMode::Virtual, None));
    page.set_paints_frames(true);
    page.navigate(Url::parse(CASE_PAGE).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(10)));
    render_viewport(&mut page);
    let before = counters(&page);
    let checks = swb_engine::paint_checks();
    for (i, step) in steps.iter().enumerate() {
        if std::env::var_os("PAINT_ONLY_TRACE").is_some() {
            eprintln!("step {i}: {:?}", counters(&page).paint_refreshes);
        }
        match step {
            Step::Move(x, y) => {
                page.mouse_move(*x, *y);
            }
            Step::Leave => {
                page.mouse_leave();
            }
            Step::Js(source) => {
                value(&mut page, source);
            }
            Step::Wait(ms) => {
                page.run_event_loop(Duration::from_millis(*ms), Duration::from_secs(30));
            }
        }
        render_viewport(&mut page);
    }
    let after = counters(&page);
    if std::env::var_os("PAINT_ONLY_TRACE").is_some() {
        eprintln!(
            "refreshes {} fallbacks {} layouts {} checks {}",
            after.paint_refreshes - before.paint_refreshes,
            after.paint_fallbacks - before.paint_fallbacks,
            after.layout_runs - before.layout_runs,
            swb_engine::paint_checks() - checks
        );
    }
    Outcome {
        refreshes: after.paint_refreshes - before.paint_refreshes,
        fallbacks: after.paint_fallbacks - before.paint_fallbacks,
        layouts: after.layout_runs - before.layout_runs,
        checks: swb_engine::paint_checks() - checks,
    }
}

/// Moves onto the element at (20, 20) and away, three times.
fn hover_steps() -> Vec<Step> {
    let mut steps = Vec::new();
    for _ in 0..3 {
        steps.push(Step::Move(20.0, 20.0));
        steps.push(Step::Move(380.0, 280.0));
    }
    steps.push(Step::Leave);
    steps
}

/// Asserts that the hover sequence takes the paint-only path every time,
/// with no layout.
fn assert_paint_only(html: &'static str) -> Outcome {
    let outcome = on_page_thread(move || run(html, &hover_steps()));
    assert_eq!(outcome.layouts, 0, "a layout ran");
    assert_eq!(outcome.fallbacks, 0, "a refresh fell back");
    assert!(outcome.refreshes >= 6, "{} refreshes", outcome.refreshes);
    assert!(outcome.checks >= 1, "the check mode compared nothing");
    outcome
}

/// Asserts that the hover sequence lays out again (the change is not a
/// paint change, or the refresh gives it up) and that the page is right.
fn assert_layout(html: &'static str) -> Outcome {
    let outcome = on_page_thread(move || run(html, &hover_steps()));
    assert!(outcome.layouts >= 1, "no layout ran");
    outcome
}

macro_rules! page {
    ($css:literal, $body:literal) => {
        concat!(
            "<!doctype html><style>body{margin:0;font:16px/20px sans-serif} ",
            $css,
            "</style>",
            $body
        )
    };
}

#[test]
fn colors_and_effects_of_a_box() {
    assert_paint_only(page!(
        "#a{width:100px;height:60px;background:#ccc;color:#333;opacity:1} \
         #a:hover{background:#c00;color:#fff;opacity:.5;outline-color:red;\
         box-shadow:2px 2px 3px #000}",
        "<div id=a>text</div>"
    ));
}

/// A hover that starts transitions of colors, `opacity` and a shadow
/// (ADR 0044, feature 5): every frame of the transitions is paint-only,
/// also the frames that reverse them.
#[test]
fn transitions_of_colors_stay_paint_only() {
    let html = page!(
        "#a{width:100px;height:60px;background:#ccc;color:#333;opacity:1;\
         transition:background-color .2s,color .2s,opacity .2s,box-shadow .2s} \
         #a:hover{background:#c00;color:#fff;opacity:.5;box-shadow:2px 2px 3px #000}",
        "<div id=a>text <span>inherited</span></div>"
    );
    let steps = [
        Step::Move(20.0, 20.0),
        Step::Wait(50),
        Step::Wait(50),
        Step::Move(380.0, 280.0),
        Step::Wait(30),
        Step::Move(20.0, 20.0),
        Step::Wait(300),
        Step::Leave,
        Step::Wait(300),
    ];
    let outcome = on_page_thread(move || run(html, &steps));
    assert_eq!(outcome.layouts, 0, "a layout ran");
    assert_eq!(outcome.fallbacks, 0, "a refresh fell back");
    assert!(outcome.refreshes >= 6, "{} refreshes", outcome.refreshes);
    assert!(outcome.checks >= 1, "the check mode compared nothing");
}

#[test]
fn inherited_color_reaches_anonymous_boxes_and_text() {
    assert_paint_only(page!(
        "#a{width:200px;height:100px;color:#333} #a:hover{color:#c00} .i{color:#060}",
        "<div id=a>before<div>block</div>after <span>span</span> <span class=i>green</span>\
         <table><tr><td>cell</td></tr></table></div>"
    ));
}

#[test]
fn transform_values_move_the_overflow() {
    // A scroll container whose content a hovered child moves further right.
    let outcome = assert_paint_only(page!(
        "#s{overflow:auto;width:100px;height:100px;position:absolute;top:0;left:0} \
         #t{width:20px;height:20px;background:red;transform:translateX(0)} \
         #t:hover{transform:translateX(300px)} #s:hover{background:#eee}",
        "<div id=s><div id=t></div></div>"
    ));
    assert!(outcome.checks >= 1);
}

#[test]
fn transform_between_none_and_a_value_lays_out() {
    assert_layout(page!(
        "#a{width:100px;height:60px;background:#ccc} #a:hover{transform:translateX(5px)}",
        "<div id=a>text</div>"
    ));
}

#[test]
fn filter_between_none_and_a_value_lays_out() {
    assert_layout(page!(
        "#a{width:100px;height:60px;background:#ccc} #a:hover{filter:blur(2px)}",
        "<div id=a>text</div>"
    ));
}

#[test]
fn layout_properties_lay_out() {
    assert_layout(page!(
        "#a{width:100px;height:60px;background:#ccc} #a:hover{width:150px}",
        "<div id=a>text</div>"
    ));
}

/// Cases of tables: the page, and whether the hover steps take the
/// paint-only path (the layout never runs again) or the refresh gives it up.
fn table_cases() -> Vec<(&'static str, bool)> {
    vec![
        (
            // A background appears on the hovered row: added to its cells.
            page!(
                "table{border-collapse:collapse} td{border:1px solid #888;padding:4px} \
                 tr:hover{background:#cde}",
                "<table id=a><tr><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr></table>"
            ),
            true,
        ),
        (
            page!(
                "table{border-spacing:2px} td{padding:4px} tr:hover{background:#cde}",
                "<table id=a><tr><td>a</td><td rowspan=2>b</td></tr><tr><td>c</td></tr>\
                 <tr><td colspan=2>d</td></tr></table>"
            ),
            true,
        ),
        (
            // The color of a cell of a collapsed table: its borders hold it.
            page!(
                "table{border-collapse:collapse} td{border:1px solid #888;padding:4px} \
                 td:hover{border-color:#c00;color:#c00}",
                "<table id=a><tr><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr></table>"
            ),
            false,
        ),
        (
            // The background of a row changes its color.
            page!(
                "table{border-spacing:2px} td{border:1px solid #888;padding:4px} \
                 tr{background:#eee} tr:hover{background:#cde} td:hover{border-color:#c00}",
                "<table id=a><tr><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr></table>"
            ),
            true,
        ),
        (
            // A column group and a column: the cells hold the parts; the
            // hovered cell changes its own background.
            page!(
                "table{border-spacing:2px} colgroup{background:#eee} col:hover{background:#cde} \
                 td:hover{background:#fcc}",
                "<table id=a><colgroup><col><col></colgroup>\
                 <tr><td>a</td><td>b</td></tr></table>"
            ),
            true,
        ),
        (
            // The background of a row group appears: not handled.
            page!(
                "table{border-spacing:2px} tbody:hover{background:#cde} td{padding:4px}",
                "<table id=a><tr><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr></table>"
            ),
            false,
        ),
        (
            // `empty-cells: hide` and a nested table.
            page!(
                "table{border-spacing:2px;empty-cells:hide} td{padding:4px} \
                 tr:hover{background:#cde;color:#c00}",
                "<table id=a><tr><td></td><td><table><tr><td>in</td></tr></table></td></tr></table>"
            ),
            true,
        ),
    ]
}

#[test]
fn table_rows_and_collapsed_borders() {
    on_page_thread(|| {
        for (html, paint_only) in table_cases() {
            let outcome = run(
                html,
                &[
                    Step::Move(10.0, 10.0),
                    Step::Move(60.0, 10.0),
                    Step::Move(10.0, 40.0),
                    Step::Move(380.0, 280.0),
                    Step::Move(10.0, 10.0),
                    Step::Leave,
                ],
            );
            if paint_only {
                assert_eq!(outcome.layouts, 0, "{html}: a layout ran");
                assert!(
                    outcome.refreshes >= 4,
                    "{html}: {} refreshes",
                    outcome.refreshes
                );
            } else {
                assert!(outcome.layouts >= 1, "{html}: no layout ran");
            }
        }
    });
}

#[test]
fn form_controls() {
    assert_paint_only(page!(
        "input,button,select,textarea{display:block;margin:0 0 4px;width:120px} \
         input:hover,button:hover,select:hover,textarea:hover{color:#c00;background:#fee;\
         border-color:#c00}",
        "<input id=a value='text'><button>button</button><select><option>one</option></select>\
         <textarea>area</textarea><input type=checkbox><input placeholder=hint>"
    ));
}

#[test]
fn placeholder_text_lays_out() {
    // The text of a `::placeholder` derives from the pseudo-element: the
    // refresh gives the change up and the page lays out.
    let outcome = on_page_thread(|| {
        run(
            page!(
                "input{display:block;width:120px;color:#333} input:hover{color:#c00} \
                 input::placeholder{color:inherit}",
                "<input placeholder='hint'><input value='v'>"
            ),
            &hover_steps(),
        )
    });
    assert!(outcome.fallbacks >= 1, "the refresh did not give up");
    assert!(outcome.layouts >= 1, "no layout ran");
}

#[test]
fn svg_icons_follow_the_color() {
    assert_paint_only(page!(
        "button{display:block;width:60px;height:40px;color:#333} button:hover{color:#c00} \
         svg{fill:currentColor;stroke:none}",
        "<button><svg width=20 height=20 viewBox='0 0 20 20'>\
         <g><path d='M0 0H20V10H0z'/><circle cx=10 cy=14 r=4 /></g></svg></button>"
    ));
}

#[test]
fn svg_fills_strokes_and_transforms_follow() {
    assert_paint_only(page!(
        "svg{display:block} svg:hover path{fill:#c00;stroke:#00c} \
         svg:hover circle{fill:#060} circle{transform:translateX(0)} \
         svg:hover circle{transform:translateX(3px)}",
        "<svg width=100 height=100><g><path d='M0 0H50V50z' fill='#0a0' stroke='#000'/>\
         <circle cx=60 cy=60 r=30 fill='#0a0' stroke='#000'/></g></svg>"
    ));
}

#[test]
fn svg_with_structure_changes_lays_out() {
    // A stroke width, `use` and `symbol`, and a `clipPath`: not paint
    // changes, so the page lays out.
    on_page_thread(|| {
        for html in [
            page!(
                "svg{display:block} svg:hover circle{opacity:.5;fill:#c00} \
                 svg:hover path{stroke:#00c;stroke-width:2px}",
                "<svg width=100 height=100><path d='M0 0H50V50z' fill='#0a0' stroke='none'/>\
                 <circle cx=60 cy=60 r=30 fill='#0a0' stroke='#000'/></svg>"
            ),
            page!(
                "svg{display:block} svg:hover use{fill:#c00} svg:hover symbol path{fill:#00c}",
                "<svg width=100 height=100><defs><symbol id=s viewBox='0 0 10 10'>\
                 <path d='M0 0H10V10z'/></symbol></defs><use href='#s' width=50 height=50 /></svg>"
            ),
            page!(
                "svg{display:block} svg:hover clipPath path{transform:translateX(10px)} \
                 svg:hover rect{clip-path:url(#c)}",
                "<svg width=100 height=100><clipPath id=c><path d='M0 0H50V50z'/></clipPath>\
                 <rect width=80 height=80 fill='#0a0' clip-path='url(#c)'/></svg>"
            ),
        ] {
            let outcome = run(html, &hover_steps());
            assert_eq!(outcome.refreshes, 0, "{html}: a refresh ran");
            assert!(outcome.layouts >= 1, "{html}: no layout ran");
        }
    });
}

#[test]
fn canvas_and_body_backgrounds() {
    on_page_thread(|| {
        for html in [
            page!(
                "html{background:#eee} html:hover{background:#cde}",
                "<div id=a>a</div>"
            ),
            page!(
                "body{height:300px} body:hover{background:#cde}",
                "<div id=a>a</div>"
            ),
            page!(
                "html{overflow:auto} body{overflow:auto;height:400px;background:#eee} \
                 body:hover{background:#cde}",
                "<div id=a>a</div>"
            ),
            page!(
                "body{height:300px;background:#eee} body:hover{background:transparent}",
                "<div id=a>a</div>"
            ),
        ] {
            // The root keeps `:hover` from the first move to the leave.
            let outcome = run(html, &hover_steps());
            assert_eq!(outcome.layouts, 0, "{html}: a layout ran");
            assert!(
                outcome.refreshes >= 2,
                "{html}: {} refreshes",
                outcome.refreshes
            );
            assert!(
                outcome.checks >= 1,
                "{html}: the check mode compared nothing"
            );
        }
    });
}

#[test]
fn pseudo_elements_and_markers() {
    assert_paint_only(page!(
        "#a{width:200px;color:#333} #a::before{content:'before ';color:#060} \
         #a:hover::before{color:#c00} #a:hover{color:#00c} li::marker{color:#900} \
         li:hover::marker{color:#009}",
        "<div id=a>text <ul><li>item</li></ul></div>"
    ));
}

#[test]
fn details_and_top_layer() {
    assert_paint_only(page!(
        "details{width:200px;color:#333} details:hover{color:#c00} \
         dialog{color:#333} dialog:hover{color:#c00} dialog::backdrop{background:#0002}",
        "<details><summary>sum</summary>content</details><details open><p>x</p></details>\
         <dialog id=d open>dialog</dialog>"
    ));
}

#[test]
fn display_contents_and_inline_boxes() {
    assert_paint_only(page!(
        "#a{display:contents;color:#333} #a:hover{color:#c00} p{margin:0;height:40px} \
         span{background:#eee} .w{position:absolute;top:0;left:0;width:200px;height:100px}",
        "<div class=w><div id=a><p>text <span>inline <b>bold</b></span></p></div></div>"
    ));
}

#[test]
fn script_changes_keep_the_layout() {
    on_page_thread(|| {
        let outcome = run(
            "<!doctype html><style>body{margin:0} #a{width:100px;height:60px;background:#ccc} \
             #s{overflow:auto;width:100px;height:60px}</style>\
             <div id=a>text</div><div id=s><div id=t style='width:10px;height:10px;\
             transform:translateX(0)'></div></div>",
            &[
                Step::Js("document.getElementById('a').style.backgroundColor = 'red'; 1"),
                Step::Js("document.getElementById('a').style.opacity = '.5'; 1"),
                Step::Js(
                    "document.getElementById('t').style.transform = 'translateX(200px)'; \
                     document.getElementById('s').scrollWidth",
                ),
                Step::Js(
                    "document.getElementById('t').style.transform = 'translateX(50px)'; \
                     JSON.stringify(document.getElementById('t').getBoundingClientRect())",
                ),
                Step::Js("document.getElementById('a').className = 'x'; 1"),
                Step::Js("document.getElementById('a').style.color = 'blue'; 1"),
            ],
        );
        assert!(outcome.refreshes >= 4, "{} refreshes", outcome.refreshes);
        assert_eq!(outcome.layouts, 0);
    });
}

#[test]
fn decorations_of_ancestors_reach_nested_boxes() {
    // Decorations propagate to the text of descendants at paint time
    // (not by inheritance): inline, block, float, atomic inline and
    // absolute children, and decorations of both an ancestor and a span.
    assert_paint_only(page!(
        "#a{width:200px;height:100px;color:#333} #a:hover{text-decoration:underline} \
         .f{float:left;width:50px} .p{position:absolute;left:100px;top:50px} \
         .ib{display:inline-block}",
        "<div id=a>top <span>span <b>bold <i>it</i></b></span><p>para <span>in</span></p>\
         <div class=f>float</div><span class=ib>atomic <u>u</u></span><div class=p>abs</div></div>"
    ));
    assert_paint_only(page!(
        "#a{width:200px;height:60px} #a span{text-decoration:underline} \
         #a:hover span{text-decoration:line-through} #a:hover{text-decoration:overline}",
        "<div id=a>x <span>y <em>z</em></span></div>"
    ));
}

/// Scripts read the scroll size, the clamped scroll offset and hit tests
/// after paint-only changes of `transform`, `clip-path` and `opacity`.
#[test]
fn script_reads_after_paint_changes() {
    on_page_thread(|| {
        swb_engine::set_restyle_check(true);
        let mut page = new_page(
            Arc::new(OnePage::new(
                "<!doctype html><style>body{margin:0} \
                 #s{overflow:auto;width:100px;height:100px;position:absolute;top:0;left:0} \
                 #t{width:20px;height:20px;transform:translateX(0)} \
                 #c{position:absolute;top:150px;left:0;width:100px;height:100px;\
                 background:red;clip-path:circle(50%)} \
                 #o{position:absolute;top:150px;left:200px;width:50px;height:50px;opacity:1}\
                 </style><div id=s><div id=t></div></div><div id=c></div><div id=o></div>",
            )),
            1,
            VIEW,
        );
        page.set_javascript(true);
        page.navigate(Url::parse(CASE_PAGE).unwrap());
        assert!(page.wait_until_loaded(Duration::from_secs(10)));
        render_viewport(&mut page);
        value(
            &mut page,
            "function at(x, y) { var e = document.elementFromPoint(x, y); \
             return e ? e.id || e.localName : 'none'; } 1",
        );
        let before = counters(&page);
        let checks = swb_engine::paint_checks();
        for (source, expected) in [
            // The overflow grows, and the offset stays in range.
            (
                "var s = document.getElementById('s'); \
                 document.getElementById('t').style.transform = 'translateX(600px)'; \
                 s.scrollLeft = 500; s.scrollLeft + ',' + s.scrollWidth",
                "\"500,620\"",
            ),
            // The overflow shrinks: the offset is clamped.
            (
                "document.getElementById('t').style.transform = 'translateX(10px)'; \
                 s.scrollLeft + ',' + s.scrollWidth",
                "\"0,100\"",
            ),
            ("at(50, 170)", "\"c\""),
            // The clip shrinks: the point outside it misses the element.
            (
                "document.getElementById('c').style.clipPath = 'circle(5%)'; \
                 at(50, 170) + ',' + at(50, 200)",
                "\"html,c\"",
            ),
            // An element with opacity 0 still takes hits.
            (
                "document.getElementById('o').style.opacity = '0'; at(220, 170)",
                "\"o\"",
            ),
        ] {
            assert_eq!(value(&mut page, source), expected, "{source}");
            render_viewport(&mut page);
        }
        let after = counters(&page);
        assert_eq!(after.layout_runs, before.layout_runs, "a layout ran");
        assert!(after.paint_refreshes - before.paint_refreshes >= 4);
        assert!(swb_engine::paint_checks() > checks, "nothing was compared");
    });
}

#[test]
fn script_geometry_follows_a_transform() {
    on_page_thread(|| {
        swb_engine::set_restyle_check(true);
        let mut page = new_page(
            Arc::new(OnePage::new(
                "<!doctype html><style>body{margin:0} #s{overflow:auto;width:100px;height:60px}\
                 #t{width:10px;height:10px;transform:translateX(0)}</style>\
                 <div id=s><div id=t></div></div>",
            )),
            1,
            VIEW,
        );
        page.set_javascript(true);
        page.navigate(Url::parse(CASE_PAGE).unwrap());
        assert!(page.wait_until_loaded(Duration::from_secs(10)));
        render_viewport(&mut page);
        let x = |page: &mut Page| {
            value(
                page,
                "document.getElementById('t').getBoundingClientRect().x",
            )
        };
        assert_eq!(x(&mut page), "0");
        assert_eq!(
            value(&mut page, "document.getElementById('s').scrollWidth"),
            "100"
        );
        value(
            &mut page,
            "document.getElementById('t').style.transform = 'translateX(200px)'; 1",
        );
        assert_eq!(x(&mut page), "200");
        assert_eq!(
            value(&mut page, "document.getElementById('s').scrollWidth"),
            "210"
        );
        let flushes = page.flush_counters();
        assert!(flushes.paint_refreshes >= 1);
    });
}

#[test]
fn animations_run_on_paint_properties() {
    on_page_thread(|| {
        let outcome = run(
            page!(
                "@keyframes c { from { color: #c00; background: #eee } to { color: #00c; \
                 background: #cde } } \
                 @keyframes o { from { opacity: 1 } to { opacity: .2 } } \
                 @keyframes m { from { transform: translateX(0) } to { transform: translateX(80px) } } \
                 #a{width:100px;height:40px;animation:c 1s linear infinite} \
                 #b{width:100px;height:40px;animation:o 1s linear infinite} \
                 #s{overflow:auto;width:150px;height:50px} \
                 #m{width:20px;height:20px;background:red;animation:m 1s linear infinite}",
                "<div id=a>color<div>inner</div>text</div><div id=b>opacity</div>\
                 <div id=s><div id=m></div></div>"
            ),
            &[Step::Wait(100); 14],
        );
        assert!(outcome.refreshes >= 10, "{} refreshes", outcome.refreshes);
        assert_eq!(outcome.layouts, 0, "an animation tick laid out");
        assert!(outcome.checks >= 5);
    });
}

#[test]
fn animations_of_layout_properties_lay_out() {
    on_page_thread(|| {
        let outcome = run(
            page!(
                "@keyframes w { from { width: 10px } to { width: 100px } } \
                 #a{height:40px;background:#c00;animation:w 1s linear infinite}",
                "<div id=a></div>"
            ),
            &[Step::Wait(100); 5],
        );
        assert_eq!(outcome.refreshes, 0);
        assert!(outcome.layouts >= 4);
    });
}

#[test]
fn a_dom_change_that_keeps_the_layout_refreshes_too() {
    on_page_thread(|| {
        // Attribute changes that only styles read (ADR 0042 section 6).
        let outcome = run(
            "<!doctype html><style>body{margin:0} #a{width:100px;height:60px;background:#ccc} \
             #a[data-on]{background:#c00;color:#fff} #a[data-on=two]{opacity:.5}</style>\
             <div id=a>text</div>",
            &[
                Step::Js("document.getElementById('a').setAttribute('data-on', ''); 1"),
                Step::Js("document.getElementById('a').setAttribute('data-on', 'two'); 1"),
                Step::Js("document.getElementById('a').removeAttribute('data-on'); 1"),
            ],
        );
        assert!(outcome.refreshes >= 2, "{} refreshes", outcome.refreshes);
        assert_eq!(outcome.layouts, 0);
    });
}
