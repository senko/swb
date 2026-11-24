//! The end-to-end oracle of the damage of frames (ADR 0044 section 18):
//! sequences of frames where each frame computes its damage against the
//! last one and rasterizes only that into the kept pixmap
//! (`Page::render_update`), compared bit for bit with a full render of the
//! same frame (`common::damage::DamageOracle`). A difference fails with the
//! frame, the first differing pixel and the rectangles; it is a bug of the
//! diff, of an input of the render that gives no damage, or of the region
//! raster. The sessions: the videojs fixture with its animations through
//! many ticks (JavaScript off, and on when `fixtures/js` exists), hover and
//! press sequences on the page fixtures, scrolls of scroll containers, a
//! frame whose document changes, nested frames, an image that arrives after
//! the first frames, the caret and the selection, a new content generation
//! of an image, and elements that paint outside their boxes at scales 1 and
//! 1.5. `frame_oracle.rs` checks the damage of its sessions too.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use swb_engine::{
    ClockMode, JsonForm, Key, Modifiers, MouseButton, Page, PageClock, Point, Size, Url,
};
use swb_net::{Fetcher, NetError, Request, Response};

mod common;
use common::damage::DamageOracle;
use common::{
    CASE_PAGE, OnePage, SHIFT, html_response, load_fixture, new_page, new_page_at_scale,
    on_page_thread, on_page_threads, tiny_png,
};

const VIEW: Size = Size::new(1280.0, 800.0);

/// The videojs fixture with its animations on: `ticks` frames of 33 ms of
/// virtual time with painted frames, scrolled at the given ticks to the
/// marquee and the bars and back.
fn videojs(javascript: bool, ticks: usize) -> Option<String> {
    let mut page = load_fixture("videojs", javascript, VIEW, |page| {
        page.set_animations(true);
        page.set_clock(PageClock::new(ClockMode::Virtual, None));
        page.set_paints_frames(true);
        page.set_scroll_indicators(true);
    })?;
    let mut oracle = DamageOracle::default();
    oracle.step(&mut page, "videojs: first");
    let scrolls = [(30, 600.0), (60, 1900.0), (90, 2600.0), (110, 0.0)];
    for tick in 0..ticks {
        if let Some(&(_, y)) = scrolls.iter().find(|&&(at, _)| at == tick) {
            page.scroll_to(Point::new(0.0, y));
        }
        page.run_event_loop(Duration::from_millis(33), Duration::from_secs(30));
        oracle.step(
            &mut page,
            &format!("videojs (js {javascript}): tick {tick}"),
        );
    }
    Some(format!("videojs (js {javascript}): {}", oracle.summary()))
}

#[test]
fn videojs_animations_without_scripts() {
    let summary = on_page_thread(|| videojs(false, 130)).expect("the page fixture is here");
    eprintln!("{summary}");
}

#[test]
fn videojs_animations_with_scripts() {
    match on_page_thread(|| videojs(true, 130)) {
        Some(summary) => eprintln!("{summary}"),
        None => eprintln!("skipped: no fixtures/js/videojs"),
    }
}

/// The hover grid of `--bench` (8 x 6 points) for two rounds with presses
/// at some points, then Tab and Shift+Tab, each followed by a frame.
fn hover_session(page: &mut Page, oracle: &mut DamageOracle, name: &str) {
    page.mouse_leave();
    oracle.step(page, &format!("{name}: start"));
    let (columns, rows) = (8, 6);
    for round in 0..2 {
        for row in 0..rows {
            for column in 0..columns {
                let x = VIEW.width * (column as f32 + 0.5) / columns as f32;
                let y = VIEW.height * (row as f32 + 0.5) / rows as f32;
                page.mouse_move(x, y);
                oracle.step(page, &format!("{name}: move to ({x}, {y})"));
                if (row * columns + column + round) % 7 == 0 {
                    page.mouse_down(x, y, MouseButton::Primary, Modifiers::NONE, 1);
                    oracle.step(page, &format!("{name}: press at ({x}, {y})"));
                    page.mouse_up(x, y, MouseButton::Primary);
                    oracle.step(page, &format!("{name}: release at ({x}, {y})"));
                }
            }
        }
    }
    for i in 0..8 {
        let modifiers = if i % 4 == 3 { SHIFT } else { Modifiers::NONE };
        page.key_down(&Key::Tab, modifiers);
        page.key_up(&Key::Tab, modifiers);
        oracle.step(page, &format!("{name}: tab {i}"));
    }
}

fn hover_fixture(name: &str, javascript: bool) -> Option<String> {
    let mut page = load_fixture(name, javascript, VIEW, |page| {
        page.set_scroll_indicators(true);
    })?;
    let mut oracle = DamageOracle::default();
    let label = format!("{name} (js {javascript})");
    hover_session(&mut page, &mut oracle, &label);
    Some(format!("{label}: {}", oracle.summary()))
}

const PAGE_FIXTURES: [&str; 8] = [
    "ars-technica",
    "bbc",
    "craigslist-posting",
    "github-swb",
    "hacker-news",
    "senko-net",
    "videojs",
    "wikipedia-web-browser",
];

#[test]
fn hover_sequences_on_page_fixtures() {
    let runs = on_page_threads(PAGE_FIXTURES.map(|name| move || hover_fixture(name, false)));
    assert!(
        runs.iter().all(Option::is_some),
        "a page fixture is missing"
    );
    for summary in runs.into_iter().flatten() {
        eprintln!("{summary}");
    }
}

#[test]
fn hover_sequences_on_js_fixtures() {
    let runs = on_page_threads(PAGE_FIXTURES.map(|name| move || hover_fixture(name, true)));
    for summary in runs.into_iter().flatten() {
        eprintln!("{summary}");
    }
}

/// Loads `html` (JavaScript as given, animations on, virtual time) at
/// 800 x 600 and returns the page.
fn open(html: &str, javascript: bool) -> Page {
    open_at(html, javascript, 1.0)
}

/// Like [`open`] at the scale factor `scale`.
fn open_at(html: &str, javascript: bool, scale: f32) -> Page {
    let mut page = new_page_at_scale(
        Arc::new(OnePage::with_data_urls(html)),
        2,
        Size::new(800.0, 600.0),
        scale,
    );
    page.set_javascript(javascript);
    page.set_animations(true);
    page.set_clock(PageClock::new(ClockMode::Virtual, None));
    page.set_paints_frames(true);
    page.set_scroll_indicators(true);
    page.navigate(Url::parse(CASE_PAGE).unwrap());
    assert!(page.wait_until_loaded(Duration::from_secs(20)));
    page
}

/// Scroll containers: nested, with text, a rounded image, a sticky header,
/// a transformed box and a filter inside; wheel steps over each, then back.
#[test]
fn scrolls_of_scroll_containers() {
    on_page_thread(|| {
        let lines = "<p>a line of the text in the box</p>".repeat(60);
        let html = format!(
            "<!DOCTYPE html><style>body{{margin:0;font:14px sans-serif}}\
             .box{{position:absolute;overflow:auto;border:3px solid #444;border-radius:12px}}\
             #a{{left:10px;top:10px;width:300px;height:250px}}\
             #b{{left:340px;top:10px;width:400px;height:300px;background:#eef}}\
             #c{{left:20px;top:40px;width:200px;height:150px;overflow:scroll}}\
             h3{{position:sticky;top:0;background:#fd8;margin:0}}\
             .t{{transform:rotate(7deg);background:#8c8;width:120px}}\
             .f{{filter:blur(2px);color:#c00}}\
             img{{border-radius:20px;width:100px;height:60px}}</style>\
             <div class=box id=a><h3>sticky</h3>{lines}</div>\
             <div class=box id=b><div class=t>rotated</div><p class=f>blurred</p>\
             <img src='data:image/svg+xml,<svg xmlns=\"http://www.w3.org/2000/svg\" \
             width=\"10\" height=\"10\"><rect width=\"10\" height=\"5\" fill=\"red\"/></svg>'>\
             <div class=box id=c>{lines}</div><div style='height:900px'></div>{lines}</div>"
        );
        let mut page = open(&html, false);
        let mut oracle = DamageOracle::default();
        oracle.step(&mut page, "scroll: first");
        for (i, (x, y, dy)) in [
            (100.0, 100.0, 120.0),
            (100.0, 100.0, 37.0),
            (500.0, 100.0, 90.0),
            (420.0, 200.0, 45.0),
            (420.0, 200.0, 300.0),
            (500.0, 100.0, 400.0),
            (100.0, 100.0, -80.0),
            (420.0, 200.0, -1000.0),
            (500.0, 100.0, -2000.0),
        ]
        .into_iter()
        .enumerate()
        {
            page.wheel(x, y, 0.0, dy);
            oracle.step(&mut page, &format!("scroll: wheel {i}"));
            page.run_event_loop(Duration::from_millis(50), Duration::from_secs(10));
            oracle.step(&mut page, &format!("scroll: after wheel {i}"));
        }
        assert!(oracle.region_frames > 5, "{}", oracle.summary());
        eprintln!("scroll containers: {}", oracle.summary());
    });
}

/// A frame whose document changes: its own animation, a hover inside it,
/// and a scroll of it, while the parent stays.
#[test]
fn a_frame_whose_document_changes() {
    on_page_thread(|| {
        let inner = "<style>@keyframes k{to{background:#08f;transform:translateX(30px)}}\
            div{width:60px;height:40px;background:#f80;animation:k 1s linear infinite}\
            p:hover{color:red}body{height:2000px}</style><div></div><p>hover me</p>";
        let html = format!(
            "<!DOCTYPE html><style>body{{margin:0}}iframe{{position:absolute;left:100px;\
             top:80px;width:300px;height:200px;border:2px solid black}}\
             .x{{position:absolute;left:500px;top:80px;width:100px;height:100px;\
             background:linear-gradient(red,blue)}}</style>\
             <div class=x></div><iframe srcdoc=\"{inner}\"></iframe>"
        );
        let mut page = open(&html, false);
        let mut oracle = DamageOracle::default();
        oracle.step(&mut page, "frame: first");
        for i in 0..12 {
            page.run_event_loop(Duration::from_millis(70), Duration::from_secs(10));
            oracle.step(&mut page, &format!("frame: tick {i}"));
        }
        page.mouse_move(130.0, 145.0);
        oracle.step(&mut page, "frame: hover");
        page.wheel(200.0, 150.0, 0.0, 100.0);
        oracle.step(&mut page, "frame: wheel");
        page.run_event_loop(Duration::from_millis(70), Duration::from_secs(10));
        oracle.step(&mut page, "frame: after the wheel");
        assert!(oracle.region_frames > 5, "{}", oracle.summary());
        eprintln!("frame: {}", oracle.summary());
    });
}

/// Serves the page; `/i.png` waits until the gate opens.
struct Gated {
    html: String,
    gate: Arc<(Mutex<bool>, Condvar)>,
}

impl Fetcher for Gated {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        if request.url.path() == "/i.png" {
            let (open, opened) = &*self.gate;
            let mut ready = open.lock().unwrap();
            while !*ready {
                ready = opened.wait(ready).unwrap();
            }
            return Ok(html_response(request, 200, tiny_png()));
        }
        Ok(html_response(request, 200, self.html.as_bytes().to_vec()))
    }
}

/// An image with its size in attributes arrives after the first frames:
/// the display list can stay the same, the image's media key changes.
#[test]
fn an_image_that_arrives_later() {
    on_page_thread(|| {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let html = "<!DOCTYPE html><style>body{margin:0}img{position:absolute;left:50px;\
            top:60px;width:200px;height:120px;border-radius:16px}</style>\
            <p>text</p><img src=i.png width=200 height=120>";
        let fetcher = Gated {
            html: html.to_owned(),
            gate: Arc::clone(&gate),
        };
        let mut page = new_page(Arc::new(fetcher), 2, Size::new(400.0, 300.0));
        page.navigate(Url::parse("http://site.test/page.html").unwrap());
        let started = Instant::now();
        while page.fragments().is_none() && started.elapsed() < Duration::from_secs(10) {
            page.process_network();
            page.pump();
            page.update_layout();
        }
        let mut oracle = DamageOracle::default();
        oracle.step(&mut page, "image: before");
        oracle.step(&mut page, "image: before, again");
        {
            let (open, opened) = &*gate;
            *open.lock().unwrap() = true;
            opened.notify_all();
        }
        assert!(page.wait_until_loaded(Duration::from_secs(10)));
        oracle.step(&mut page, "image: arrived");
        let damage = oracle.last.clone().unwrap();
        assert!(
            damage.full || !damage.rects.is_empty(),
            "the image gave no damage"
        );
        eprintln!("image: {}", oracle.summary());
    });
}

/// The caret and the selection: a text field gets the focus, keys move the
/// caret and select, a paragraph is selected by a drag and a double click.
#[test]
fn the_caret_and_the_selection() {
    on_page_thread(|| {
        let html = "<!DOCTYPE html><style>body{margin:0;font:16px sans-serif}\
            input{position:absolute;left:20px;top:20px;width:300px}\
            textarea{position:absolute;left:20px;top:70px;width:300px;height:80px}\
            p{position:absolute;left:20px;top:180px;width:500px}</style>\
            <input id=i value='hello world'><textarea>first line\nsecond line</textarea>\
            <p>Some text to select with the mouse, over two lines of the paragraph when \
            it wraps at the width of the box.</p>";
        let mut page = open(html, true);
        let mut oracle = DamageOracle::default();
        oracle.step(&mut page, "caret: first");
        page.mouse_down(100.0, 30.0, MouseButton::Primary, Modifiers::NONE, 1);
        page.mouse_up(100.0, 30.0, MouseButton::Primary);
        oracle.step(&mut page, "caret: click into the field");
        let keys = [
            (Key::Character("x".into()), Modifiers::NONE),
            (Key::ArrowLeft, Modifiers::NONE),
            (Key::ArrowLeft, Modifiers::NONE),
            (Key::ArrowLeft, SHIFT),
            (Key::ArrowLeft, SHIFT),
            (Key::Home, SHIFT),
            (Key::End, Modifiers::NONE),
            (Key::Backspace, Modifiers::NONE),
            (Key::Tab, Modifiers::NONE),
            (Key::Character("y".into()), Modifiers::NONE),
            (Key::Enter, Modifiers::NONE),
            (Key::ArrowUp, SHIFT),
        ];
        for (i, (key, modifiers)) in keys.iter().enumerate() {
            page.key_down(key, *modifiers);
            page.key_up(key, *modifiers);
            oracle.step(&mut page, &format!("caret: key {i}"));
        }
        page.mouse_down(30.0, 190.0, MouseButton::Primary, Modifiers::NONE, 1);
        oracle.step(&mut page, "selection: press");
        for (i, x) in [80.0, 200.0, 420.0].into_iter().enumerate() {
            page.mouse_move(x, 205.0);
            oracle.step(&mut page, &format!("selection: drag {i}"));
        }
        page.mouse_up(420.0, 205.0, MouseButton::Primary);
        oracle.step(&mut page, "selection: release");
        page.mouse_down(150.0, 190.0, MouseButton::Primary, Modifiers::NONE, 2);
        page.mouse_up(150.0, 190.0, MouseButton::Primary);
        oracle.step(&mut page, "selection: double click");
        page.clear_selection();
        oracle.step(&mut page, "selection: cleared");
        assert!(oracle.region_frames > 5, "{}", oracle.summary());
        eprintln!("caret and selection: {}", oracle.summary());
    });
}

/// A new content generation of an image (a video frame in M14) makes the
/// frame outdated, and its damage covers the image.
#[test]
fn a_new_content_generation_of_an_image() {
    on_page_thread(|| {
        let html = "<!DOCTYPE html><style>body{margin:0}img{position:absolute;left:40px;\
            top:30px;width:100px;height:80px}</style><p>text</p><img id=v src='data:image/svg+xml,\
            <svg xmlns=\"http://www.w3.org/2000/svg\" width=\"4\" height=\"4\"><rect width=\"4\" \
            height=\"2\" fill=\"red\"/></svg>'>";
        let mut page = open(html, false);
        let mut oracle = DamageOracle::default();
        oracle.step(&mut page, "generation: first");
        oracle.step(&mut page, "generation: again");
        assert!(!page.frame_outdated());
        let node = common::node(&page, "v");
        page.bump_image_generation(node);
        assert!(page.frame_outdated(), "a new generation needs a frame");
        oracle.step(&mut page, "generation: bumped");
        let damage = oracle.last.clone().unwrap();
        let covered = damage
            .rects
            .iter()
            .any(|r| r.x <= 40 && r.y <= 30 && r.right() >= 140 && r.bottom() >= 110);
        assert!(covered, "the damage {damage:?} does not cover the image");
        assert!(!page.frame_outdated());
    });
}

/// Elements that paint outside their boxes or under unusual transforms
/// (from the review of feature 4b): glyph overhang, shadows, decorations,
/// outlines with offset, SVG strokes with markers, a border wider than its
/// box at a fractional scale, filters, masks, backdrop filters, perspective,
/// strong skews and scales, a blend mode, a clip path.
const OVERPAINT: &str = "<!DOCTYPE html><style>
body{margin:0;font:16px sans-serif;background:#fff}
.a{position:absolute}
#t1{left:30px;top:20px;font:italic 120px serif;text-shadow:20px 20px 30px red;\
text-decoration:underline wavy blue;text-decoration-thickness:12px}
#t2{left:400px;top:30px;font:70px serif}
#t3{left:30px;top:200px;font:bold 90px serif;-webkit-text-stroke:14px green;color:#fc0}
#b1{left:420px;top:200px;width:80px;height:60px;background:#eee;\
box-shadow:0 0 40px 20px blue,inset 0 0 30px 10px black}
#o1{left:600px;top:60px;width:60px;height:60px;outline:10px solid orange;outline-offset:30px}
#s1{left:30px;top:400px}
#r1{left:300px;top:400px;width:50px;height:50px;transform:rotate(37deg) skew(20deg) scale(3);\
border:3px solid red;background:#cfc}
#r2{left:560px;top:400px;width:0;height:0;border:0.4px solid #000;border-radius:10px;padding:0.3px}
#li{left:650px;top:300px;font-size:70px}
#f1{left:30px;top:520px;filter:blur(10px) drop-shadow(30px 30px 10px #000);font-size:40px}
#m1{left:300px;top:520px;width:100px;height:60px;background:#f0f;\
mask-image:linear-gradient(black,transparent)}
#bf{left:500px;top:520px;width:100px;height:60px;backdrop-filter:blur(8px);background:#ffffff40}
#sc{left:650px;top:450px;width:20px;height:20px;transform:scale(8);transform-origin:0 0;\
background:#0a0}
#p1{left:100px;top:300px;width:120px;height:80px;transform:perspective(200px) rotateY(60deg);\
background:#fa0;border:4px solid #00f;font-size:30px}
#k1{left:250px;top:300px;width:60px;height:60px;transform:skewX(70deg) scale(4,0.3);\
background:#0af;border:2px dashed red;border-radius:20px}
#ro{left:360px;top:300px;width:200px;height:20px;transform:rotate(89.9deg) scaleX(0.002);\
background:#000}
#mb{left:420px;top:100px;width:100px;height:100px;background:linear-gradient(90deg,#f00,#00f);\
mix-blend-mode:difference;border-radius:30px;opacity:.8}
#cp{left:560px;top:170px;width:100px;height:100px;background:#c0c;\
clip-path:circle(40% at 30% 60%)}
#sh{left:20px;top:340px;width:60px;height:40px;transform:scale(0.2);transform-origin:0 0;\
border:20px solid #333;border-radius:50%;background:radial-gradient(#f00,#00f)}
</style>
<div id=t1 class=a>Aff</div>
<div id=t2 class=a>a&#768;&#769;&#770;&#771;&#772;&#773; e&#8419;&#803;</div>
<div id=t3 class=a>Sk</div>
<div id=b1 class=a></div>
<div id=o1 class=a></div>
<svg id=s1 class=a width=240 height=150><defs><marker id=mk markerWidth=20 markerHeight=20 \
refX=10 refY=10><circle cx=10 cy=10 r=10 fill=red /></marker></defs><path \
d='M20 120 L110 20 L200 120' stroke=black stroke-width=30 stroke-linejoin=miter \
stroke-miterlimit=20 fill=none marker-mid=url(#mk) /></svg>
<div id=r1 class=a>rot</div>
<div id=r2 class=a></div>
<ul class=a id=li><li>item</li></ul>
<div id=f1 class=a>Blur me</div>
<div id=m1 class=a></div>
<div id=bf class=a></div>
<div id=p1 class=a>persp</div><div id=k1 class=a>skew</div><div id=ro class=a></div>
<div id=mb class=a></div><div id=cp class=a></div><div id=sh class=a></div>
<div id=sc class=a><div id=in style='width:3px;height:3px;background:red'></div></div>
";

const OVERPAINT_IDS: [&str; 19] = [
    "t1", "t2", "t3", "b1", "o1", "s1", "r1", "r2", "li", "f1", "m1", "bf", "in", "p1", "k1", "ro",
    "mb", "cp", "sh",
];

/// Hides each element of [`OVERPAINT`] (with `display` and with
/// `visibility`) and shows it again, each step a frame.
fn overpaint(scale: f32) {
    let mut page = open_at(OVERPAINT, true, scale);
    let mut oracle = DamageOracle::default();
    oracle.step(&mut page, "overpaint: first");
    for id in OVERPAINT_IDS {
        for (property, hidden) in [("display", "none"), ("visibility", "hidden")] {
            for (step, value) in [("hide", hidden), ("show", "")] {
                let script =
                    format!("document.getElementById('{id}').style.{property} = '{value}'");
                page.evaluate(&script, JsonForm::Strict)
                    .expect("scripts run");
                oracle.step(
                    &mut page,
                    &format!("overpaint (scale {scale}): {step} {id} by {property}"),
                );
            }
        }
    }
    eprintln!("overpaint (scale {scale}): {}", oracle.summary());
}

#[test]
fn elements_that_paint_outside_their_boxes() {
    on_page_thread(|| {
        overpaint(1.0);
        overpaint(1.5);
    });
}

/// An animating frame inside a frame inside the page; wheel steps over the
/// inner frame and the middle frame.
#[test]
fn nested_frames_with_animation_and_scrolls() {
    on_page_thread(|| {
        let leaf = "<style>@keyframes k{to{background:#08f;transform:translateX(30px)}}\
            div{width:60px;height:40px;background:#f80;animation:k 1s linear infinite}\
            body{height:2000px;margin:0}</style><div></div><p>leaf</p>";
        let mid = format!(
            "<style>body{{margin:0;height:900px}}iframe{{width:200px;height:120px;\
             border:1px solid red}}</style><iframe srcdoc=\"{}\"></iframe><p>mid</p>",
            leaf.replace('"', "&quot;")
        );
        let html = format!(
            "<!DOCTYPE html><style>body{{margin:0}}iframe{{position:absolute;left:100px;\
             top:80px;width:300px;height:250px;border:2px solid black}}</style>\
             <iframe srcdoc=\"{}\"></iframe>",
            mid.replace('"', "&quot;")
        );
        let mut page = open(&html, true);
        let mut oracle = DamageOracle::default();
        oracle.step(&mut page, "nested frames: first");
        for tick in 0..12 {
            page.run_event_loop(Duration::from_millis(70), Duration::from_secs(10));
            oracle.step(&mut page, &format!("nested frames: tick {tick}"));
        }
        page.wheel(150.0, 140.0, 0.0, 60.0);
        oracle.step(&mut page, "nested frames: wheel over the inner frame");
        page.wheel(350.0, 300.0, 0.0, 60.0);
        oracle.step(&mut page, "nested frames: wheel over the middle frame");
        for tick in 0..6 {
            page.run_event_loop(Duration::from_millis(70), Duration::from_secs(10));
            oracle.step(&mut page, &format!("nested frames: tick {tick} after"));
        }
        let summary = oracle.summary();
        eprintln!("nested frames: {summary}");
    });
}

/// Composited groups (ADR 0044 section 19): a reveal of a rounded box with
/// a gradient, text, an SVG image, a `backdrop-filter` bar and a spinning
/// box inside (the spinner's own composited group in the revealed one), a
/// scale animation that grows past 2 (a layer at twice the device scale),
/// an animation that ends on a held step, a paused animation; a hover in
/// the revealed box and a wheel step while they run. Each frame: the damage
/// over the kept pixels, a full render, and a render without the raster
/// caches are equal.
fn composited(scale: f32) {
    let svg = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='40' \
               height='40'%3E%3Ccircle cx='20' cy='20' r='17' fill='%23fd0'/%3E%3C/svg%3E";
    let html = format!(
        "<!DOCTYPE html><style>body{{margin:0;font:16px sans-serif;height:1600px}}\
         @keyframes reveal{{from{{opacity:0;transform:translateY(12px) scale(.99)}}\
         to{{opacity:1;transform:none}}}}\
         @keyframes spin{{to{{transform:rotate(360deg)}}}}\
         @keyframes pop{{from{{transform:scale(.2)}}to{{transform:scale(2.5)}}}}\
         @keyframes hold{{to{{transform:rotate(30deg)}}}}\
         #hero{{position:absolute;left:40px;top:30px;width:500px;height:300px;\
         border-radius:24px;overflow:hidden;background:linear-gradient(45deg,#c33,#36c);\
         animation:reveal 1s cubic-bezier(.2,.8,.2,1) .2s both}}\
         #bar{{position:absolute;left:0;right:0;bottom:0;height:50px;\
         backdrop-filter:blur(6px);background:rgba(255,255,255,.3)}}\
         #hero a{{display:block;margin:20px;color:#fff}}\
         #hero a:hover{{color:#ff0;background:#0004}}\
         #spin{{position:absolute;left:300px;top:40px;width:60px;height:60px;\
         border-radius:12px;background:#0c8;animation:spin 2s linear infinite}}\
         #pop{{position:absolute;left:600px;top:80px;width:80px;height:40px;\
         background:#fa0;border-radius:50%;animation:pop .8s ease-in both}}\
         #hold{{position:absolute;left:600px;top:300px;width:100px;height:30px;\
         background:#38f;animation:hold .6s steps(2,jump-start) forwards}}\
         #paused{{position:absolute;left:40px;top:400px;width:100px;height:30px;\
         background:#93f;animation:spin 1s linear infinite paused}}</style>\
         <div id=hero><a href=#x>a link in the hero</a><img src=\"{svg}\">\
         <div id=spin></div><div id=bar>bar</div></div>\
         <div id=pop>pop</div><div id=hold>hold</div><div id=paused></div>"
    );
    let mut page = open_at(&html, false, scale);
    let mut oracle = DamageOracle::default();
    let label = format!("composited (scale {scale})");
    oracle.step(&mut page, &format!("{label}: first"));
    for tick in 0..32 {
        match tick {
            10 => {
                page.mouse_move(100.0, 60.0);
            }
            14 => {
                page.mouse_move(100.0, 500.0);
            }
            20 => {
                page.wheel(100.0, 500.0, 0.0, 40.0);
            }
            _ => {}
        }
        page.run_event_loop(Duration::from_millis(50), Duration::from_secs(10));
        oracle.step(&mut page, &format!("{label}: tick {tick}"));
    }
    let stats = page.raster_cache_stats();
    assert!(stats.hits > 0, "{label}: {stats:?}");
    eprintln!("{label}: {}; cache {stats:?}", oracle.summary());
}

#[test]
fn composited_groups_of_running_animations() {
    on_page_thread(|| {
        composited(1.0);
        composited(1.5);
    });
}

/// An animation that ends on the value it held for its last step: the
/// style does not change at the end, but the group stops being composited
/// (ADR 0044 section 19), and the frame after the end equals the render of
/// the same page at rest.
#[test]
fn the_frame_after_a_composited_animation_is_the_static_render() {
    on_page_thread(|| {
        let style = "body{margin:0}div{position:absolute;left:50px;top:40px;width:120px;\
                     height:50px;background:#38f;border-radius:9px}";
        let animated = format!(
            "<!DOCTYPE html><style>{style}@keyframes hold{{to{{transform:rotate(30deg);\
             opacity:.5}}}}div{{animation:hold .6s steps(2,jump-start) forwards}}</style>\
             <div></div>"
        );
        let still = format!(
            "<!DOCTYPE html><style>{style}div{{transform:rotate(30deg);opacity:.5}}</style>\
             <div></div>"
        );
        let mut page = open(&animated, false);
        let mut oracle = DamageOracle::default();
        oracle.step(&mut page, "hold: first");
        page.run_event_loop(Duration::from_millis(400), Duration::from_secs(10));
        let held = oracle.step(&mut page, "hold: the last step");
        page.run_event_loop(Duration::from_millis(400), Duration::from_secs(10));
        assert!(!page.animations_running());
        let after = oracle.step(&mut page, "hold: after the end");
        let mut rest = open(&still, false);
        let static_render = rest.screenshot(false).expect("a screenshot");
        assert!(
            common::damage::first_difference(&after, &static_render).is_none(),
            "the frame after the end is the static render"
        );
        // While it ran, the group was composited: other pixels.
        assert!(common::damage::first_difference(&held, &static_render).is_some());
    });
}

/// A sticky box inside a rotating (composited) group while the page
/// scrolls: the layer's key holds the viewport scroll offset (found in the
/// review of feature 4c: no other test covered that part of the key).
#[test]
fn a_sticky_box_inside_a_composited_group() {
    on_page_thread(|| {
        for scale in [1.0, 1.5] {
            let html = "<!DOCTYPE html><style>body{margin:0;height:4000px;\
                font:16px sans-serif}#rot{position:absolute;left:100px;top:100px;\
                width:300px;height:2400px;background:linear-gradient(#cde,#edc);\
                animation:spin 20s linear infinite}@keyframes spin{to{transform:rotate(3deg)}}\
                #s{position:sticky;top:10px;height:30px;margin:0 20px;background:#c33;\
                color:#fff}</style><div id=rot><p>top</p><div id=s>sticky</div>\
                <p>after</p><p style='margin-top:1500px'>far</p></div>";
            let mut page = open_at(html, false, scale);
            let mut oracle = DamageOracle::default();
            oracle.step(&mut page, "sticky: first");
            for tick in 0..24 {
                if tick % 3 == 0 {
                    page.scroll_to(Point::new(0.0, 40.0 * tick as f32));
                }
                page.run_event_loop(Duration::from_millis(50), Duration::from_secs(10));
                oracle.step(&mut page, &format!("sticky (scale {scale}): tick {tick}"));
            }
            eprintln!("sticky (scale {scale}): {}", oracle.summary());
        }
    });
}

/// In the delay of an animation of `transform` (no fill), the element
/// renders as without the animation: the composited path is only for
/// running animations.
#[test]
fn the_delay_of_an_animation_is_the_static_render() {
    on_page_thread(|| {
        let style = "body{margin:0}div{position:absolute;left:50px;top:40px;width:120px;\
                     height:50px;background:#38f;border-radius:9px;transform:rotate(7deg)}";
        let delayed = format!(
            "<!DOCTYPE html><style>{style}@keyframes a{{to{{transform:rotate(90deg)}}}}\
             div{{animation:a 1s linear 100s}}</style><div></div>"
        );
        let still = format!("<!DOCTYPE html><style>{style}</style><div></div>");
        let mut page = open_at(&delayed, false, 1.0);
        let delay = page.screenshot(false).unwrap();
        let mut rest = open_at(&still, false, 1.0);
        let static_render = rest.screenshot(false).unwrap();
        let difference = common::damage::first_difference(&delay, &static_render);
        assert!(difference.is_none(), "{difference:?}");
    });
}

/// CSS transitions (ADR 0044, feature 5): hovers that start transitions of
/// colors (paint-only frames), of `transform` and `opacity` (composited
/// groups), and a script that toggles a class with transitions of
/// `height` (layout) and a color, reversed while they run; every frame is
/// drawn with its damage and compared with a full render.
#[test]
fn transitions_of_hovers_and_class_changes() {
    on_page_thread(|| {
        let html = "<!DOCTYPE html><style>body{margin:0;font:16px sans-serif}\
            a{display:inline-block;margin:10px;padding:8px;color:#123;background:#eee;\
            border-radius:6px;transition:color .3s,background-color .3s ease}\
            a:hover{color:#fff;background:#c00}\
            #b{width:120px;height:60px;margin:20px;background:#38f;border-radius:9px;\
            transition:transform .4s cubic-bezier(.2,.8,.2,1),opacity .4s,scale .4s}\
            #b:hover{transform:translateX(40px) rotate(10deg);opacity:.5;scale:1.1}\
            #c{height:20px;margin:10px;background:#0a5;overflow:hidden;\
            transition:height .3s linear,background-color .3s}\
            body.on #c{height:80px;background:#a05}</style>\
            <a href=#x>one</a><a href=#y>two</a><div id=b></div><div id=c>text</div>\
            <p>after</p><script>setInterval(()=>document.body.classList.toggle('on'),170)\
            </script>";
        for scale in [1.0, 1.5] {
            let mut page = open_at(html, true, scale);
            let mut oracle = DamageOracle::default();
            let label = format!("transitions (scale {scale})");
            oracle.step(&mut page, &format!("{label}: first"));
            for tick in 0..40 {
                match tick {
                    3 => {
                        page.mouse_move(30.0, 25.0);
                    }
                    7 => {
                        page.mouse_move(90.0, 25.0);
                    }
                    12 | 26 => {
                        page.mouse_move(80.0, 100.0);
                    }
                    22 => {
                        page.mouse_move(700.0, 500.0);
                    }
                    _ => {}
                }
                page.run_event_loop(Duration::from_millis(33), Duration::from_secs(10));
                oracle.step(&mut page, &format!("{label}: tick {tick}"));
            }
            assert!(oracle.region_frames > 0, "{label}: {}", oracle.summary());
            eprintln!("{label}: {}", oracle.summary());
        }
    });
}
