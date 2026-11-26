//! The oracle for `Page::frame_outdated` (ADR 0040 section 2): scripted
//! input and tasks on page fixtures; whenever `frame_outdated()` is false, a
//! render gives the same pixels as the last render. A missed invalidation
//! shows as a difference here. The JavaScript runs use the fixtures in
//! `fixtures/js/` when they exist (they are not committed).
//!
//! The animation cases (ADR 0044 section 16) run in virtual time with
//! painted frames, as the GUI does: at each step the clock advances, the
//! page's own frames run, and the oracle compares "a frame is needed" with
//! "the pixels changed". The visibility test of the frame loop can drop a
//! frame silently; a changed pixel without a frame fails here.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{ClockMode, Key, Modifiers, MouseButton, Page, PageClock, Size, Url};
use swb_net::Fetcher;

mod common;
use common::damage::DamageOracle;
use common::{SHIFT, TestSite, finish_loading, load_fixture, new_page, on_page_thread, pump_for};

const VIEW: Size = Size::new(1280.0, 800.0);

/// Renders when the frame is outdated, else renders again and compares
/// with the last pixels. Returns the number of frames that were outdated.
#[derive(Default)]
struct Oracle {
    last: Option<Vec<u8>>,
    frames: usize,
    checks: usize,
    /// Frames that were outdated but drew the same pixels (allowed).
    wasted: usize,
    /// Every frame is also drawn with its damage into a kept pixmap, which
    /// must equal the full render (ADR 0044 section 18).
    damage: DamageOracle,
}

impl Oracle {
    fn step(&mut self, page: &mut Page, label: &str) {
        let outdated = page.frame_outdated();
        let data = self.damage.step(page, label).data().to_vec();
        if outdated {
            self.frames += 1;
            if self.last.as_ref() == Some(&data) {
                self.wasted += 1;
            }
        } else {
            self.checks += 1;
            if let Some(last) = &self.last {
                assert!(
                    *last == data,
                    "{label}: the frame was not outdated, but the pixels changed"
                );
            }
        }
        self.last = Some(data);
    }
}

/// The scripted session: moves, wheel, drags, clicks, keys, resize, each
/// followed by the event loop and an oracle step.
fn session(page: &mut Page, oracle: &mut Oracle) {
    let points = [
        (100.0, 100.0),
        (640.0, 200.0),
        (300.0, 400.0),
        (900.0, 300.0),
        (50.0, 700.0),
        (1200.0, 50.0),
    ];
    oracle.step(page, "first");
    pump_for(page, Duration::from_millis(150));
    oracle.step(page, "after the first tasks");
    for (i, &(x, y)) in points.iter().enumerate() {
        page.mouse_move(x, y);
        oracle.step(page, &format!("move {i}"));
        page.mouse_move(x + 1.0, y + 1.0);
        oracle.step(page, &format!("small move {i}"));
        pump_for(page, Duration::from_millis(20));
        oracle.step(page, &format!("tasks after move {i}"));
    }
    for (i, &(x, y)) in points.iter().enumerate() {
        page.wheel(x, y, 0.0, 250.0);
        oracle.step(page, &format!("wheel {i}"));
        pump_for(page, Duration::from_millis(20));
        oracle.step(page, &format!("tasks after wheel {i}"));
    }
    page.wheel(640.0, 400.0, 0.0, -10_000.0);
    oracle.step(page, "wheel to the top");
    // Drags and double clicks (selection), at points that are probably text.
    for (i, &(x, y)) in points.iter().enumerate().take(4) {
        page.mouse_down(x, y, MouseButton::Primary, Modifiers::NONE, 2);
        oracle.step(page, &format!("double press {i}"));
        page.mouse_up(x, y, MouseButton::Primary);
        oracle.step(page, &format!("double release {i}"));
        page.mouse_down(x, y, MouseButton::Primary, Modifiers::NONE, 1);
        page.mouse_move(x + 150.0, y + 60.0);
        oracle.step(page, &format!("drag {i}"));
        page.mouse_up(x + 150.0, y + 60.0, MouseButton::Primary);
        oracle.step(page, &format!("drag end {i}"));
    }
    page.key_down(&Key::Character("a".into()), common::CTRL);
    oracle.step(page, "ctrl-a");
    page.clear_selection();
    oracle.step(page, "clear selection");
    for (i, (key, mods)) in [
        (Key::Tab, Modifiers::NONE),
        (Key::Tab, Modifiers::NONE),
        (Key::Tab, Modifiers::NONE),
        (Key::Tab, SHIFT),
        (Key::ArrowDown, Modifiers::NONE),
        (Key::PageDown, Modifiers::NONE),
        (Key::End, Modifiers::NONE),
        (Key::Home, Modifiers::NONE),
        (Key::Escape, Modifiers::NONE),
    ]
    .iter()
    .enumerate()
    {
        page.key_down(key, *mods);
        page.key_up(key, *mods);
        oracle.step(page, &format!("key {i}"));
        pump_for(page, Duration::from_millis(10));
        oracle.step(page, &format!("tasks after key {i}"));
    }
    page.set_viewport(Size::new(900.0, 700.0), 1.0);
    oracle.step(page, "resize");
    page.set_viewport(Size::new(900.0, 700.0), 2.0);
    oracle.step(page, "scale");
    pump_for(page, Duration::from_millis(100));
    oracle.step(page, "tasks at the end");
}

fn run_fixture(name: &str, javascript: bool) -> Option<(usize, usize)> {
    let mut page = load_fixture(name, javascript, VIEW, |page| {
        page.set_scroll_indicators(true);
    })?;
    let mut oracle = Oracle::default();
    session(&mut page, &mut oracle);
    eprintln!(
        "{name} (js {javascript}): {} outdated frames, {} frames checked as current; {}",
        oracle.frames,
        oracle.checks,
        oracle.damage.summary()
    );
    Some((oracle.frames, oracle.checks))
}

#[test]
fn no_stale_frames_on_fixtures_without_javascript() {
    let runs = on_page_thread(|| {
        ["hacker-news", "wikipedia-web-browser", "github-swb"]
            .map(|name| run_fixture(name, false))
            .to_vec()
    });
    for (frames, checks) in runs.into_iter().flatten() {
        assert!(frames > 5 && checks > 5, "the session must do both");
    }
}

#[test]
fn no_stale_frames_on_fixtures_with_javascript() {
    let runs = on_page_thread(|| {
        ["hacker-news", "wikipedia-web-browser", "github-swb"]
            .map(|name| run_fixture(name, true))
            .to_vec()
    });
    for (frames, checks) in runs.into_iter().flatten() {
        assert!(frames > 5 && checks > 5, "the session must do both");
    }
}

// ----- Animations -----

/// What happens before an oracle step of an animation case.
#[derive(Clone, Copy)]
enum Act {
    /// The page's event loop runs for this many milliseconds of virtual time.
    Wait(u64),
    /// The viewport scrolls to this offset.
    Scroll(f32),
    /// The wheel turns by `dy` at (x, y).
    Wheel(f32, f32, f32),
}

/// The counts of one animation case.
struct Counts {
    frames: usize,
    wasted: usize,
    checks: usize,
}

/// Runs `actions` on `html` (virtual time, painted frames, 400x300), with
/// an oracle step after each. Panics if pixels changed without a frame.
fn run_animation_case(name: &str, html: &str, javascript: bool, actions: &[Act]) -> Counts {
    const URL: &str = "https://site.test/page.html";
    let mut site = TestSite::default();
    site.pages.insert(URL.to_owned(), html.to_owned());
    let mut page = new_page(
        Arc::new(site) as Arc<dyn Fetcher>,
        2,
        Size::new(400.0, 300.0),
    );
    page.set_javascript(javascript);
    page.set_animations(true);
    page.set_clock(PageClock::new(ClockMode::Virtual, None));
    page.set_paints_frames(true);
    page.navigate(Url::parse(URL).unwrap());
    finish_loading(&mut page, Duration::from_secs(20));
    let mut oracle = Oracle::default();
    oracle.step(&mut page, &format!("{name}: first"));
    for (i, act) in actions.iter().enumerate() {
        match *act {
            Act::Wait(ms) => {
                page.run_event_loop(Duration::from_millis(ms), Duration::from_secs(30));
            }
            Act::Scroll(y) => page.scroll_to(swb_engine::Point::new(0.0, y)),
            Act::Wheel(x, y, dy) => {
                page.wheel(x, y, 0.0, dy);
            }
        }
        oracle.step(&mut page, &format!("{name} (js {javascript}): step {i}"));
    }
    eprintln!(
        "{name} (js {javascript}): {} frames ({} without a pixel change), {} steps without a \
         frame; {}",
        oracle.frames,
        oracle.wasted,
        oracle.checks,
        oracle.damage.summary()
    );
    Counts {
        frames: oracle.frames,
        wasted: oracle.wasted,
        checks: oracle.checks,
    }
}

/// Twelve waits of 130 ms.
fn waits() -> Vec<Act> {
    vec![Act::Wait(130); 12]
}

/// A page with a 5000 px tall body, the style `css` and `body`.
fn animation_page(css: &str, body: &str) -> String {
    format!("<!DOCTYPE html><style>body {{ margin: 0; height: 5000px }} {css}</style>{body}")
}

/// One case of the animation oracle: its name, page, steps and the least
/// and most frames that the design allows.
type AnimationCase = (&'static str, String, Vec<Act>, usize, usize);

/// A fading box at the top left (`.a`).
const FADE: &str = "@keyframes k { to { opacity: 0.2 } } \
    .a { position: absolute; top: 20px; left: 20px; width: 50px; height: 50px; \
    background: blue; animation: k 1s linear infinite }";

/// A strip (`.s`) that clips a transform animation (`.t`) out of itself.
const CLIPPED: &str = "@keyframes k { from { transform: translateX(-400px) } \
    to { transform: translateX(-300px) } } \
    .s { position: absolute; top: 20px; left: 20px; width: 100px; height: 20px; \
    overflow: hidden } \
    .t { width: 50px; height: 20px; background: red; animation: k 1s linear infinite }";

/// The animation cases. The visible ones must draw; what the visibility
/// test hides may draw at most at its event times (a frame that changes no
/// pixel is allowed, and counted).
fn animation_cases() -> Vec<AnimationCase> {
    vec![
        (
            "visible",
            animation_page(FADE, "<div class=a></div>"),
            waits(),
            6,
            usize::MAX,
        ),
        (
            // A transform that keeps the element clipped out of a strip
            // that is in view. The test bounds the animation by the strip
            // (what a transform can reach), so every frame is asked for and
            // none changes a pixel: allowed, counted as wasted.
            "clipped out",
            animation_page(CLIPPED, "<div class=s><div class=t></div></div>"),
            waits(),
            0,
            usize::MAX,
        ),
        (
            // The same strip out of the viewport: no frames.
            "clipped out, strip offscreen",
            animation_page(
                &format!("{CLIPPED} .s {{ top: 2000px }}"),
                "<div class=s><div class=t></div></div>",
            ),
            waits(),
            0,
            3,
        ),
        (
            // An inner scroll container: the animation at its top is in
            // view, the one at 600 px is not; then the wheel swaps them.
            "inner scroll",
            animation_page(
                "@keyframes k { to { opacity: 0.2 } } \
                 .box { position: absolute; top: 20px; left: 20px; width: 150px; height: 100px; \
                 overflow: auto } \
                 .in { height: 700px; position: relative } \
                 .m { position: absolute; width: 40px; height: 40px; background: green; \
                 animation: k 1s linear infinite } \
                 .m1 { top: 10px } .m2 { top: 600px }",
                "<div class=box><div class=in><div class='m m1'></div>\
                 <div class='m m2'></div></div></div>",
            ),
            [
                waits(),
                vec![Act::Wheel(50.0, 60.0, 590.0)],
                waits(),
                vec![Act::Wheel(50.0, 60.0, -590.0)],
                waits(),
            ]
            .concat(),
            12,
            usize::MAX,
        ),
        (
            "svg owner",
            animation_page(
                "@keyframes k { from { fill: red } to { fill: blue } } \
                 circle { animation: k 1s linear infinite }",
                "<svg width=100 height=100><circle cx=50 cy=50 r=40></circle></svg>",
            ),
            waits(),
            6,
            usize::MAX,
        ),
        (
            "svg owner offscreen",
            animation_page(
                "@keyframes k { from { fill: red } to { fill: blue } } \
                 circle { animation: k 1s linear infinite } \
                 svg { position: absolute; top: 2000px }",
                "<svg width=100 height=100><circle cx=50 cy=50 r=40></circle></svg>",
            ),
            waits(),
            0,
            3,
        ),
    ]
}

/// The cases of fixed and sticky boxes, `visibility`, `display: contents` and
/// a transform that moves a box into the viewport.
fn moving_cases() -> Vec<AnimationCase> {
    vec![
        (
            "fixed",
            animation_page(
                &format!("{FADE} .a {{ position: fixed }}"),
                "<div class=a></div>",
            ),
            [vec![Act::Scroll(1000.0)], waits()].concat(),
            6,
            usize::MAX,
        ),
        (
            // At 1500 px in the flow; sticky at the top of the viewport.
            "sticky",
            animation_page(
                &format!(
                    "{FADE} .c {{ position: absolute; top: 1000px; height: 2000px; width: 100px }} \
                     .a {{ position: sticky; top: 10px }}"
                ),
                "<div class=c><div style='height: 500px'></div><div class=a></div></div>",
            ),
            [vec![Act::Scroll(1400.0)], waits()].concat(),
            6,
            usize::MAX,
        ),
        (
            "visibility",
            animation_page(
                "@keyframes k { 0%, 50% { visibility: hidden } 100% { visibility: visible } } \
                 .a { position: absolute; top: 20px; left: 20px; width: 50px; height: 50px; \
                 background: blue; animation: k 1s step-end infinite }",
                "<div class=a></div>",
            ),
            waits(),
            1,
            usize::MAX,
        ),
        (
            // `display: contents` has no box: the paragraph inherits the
            // animated color.
            "display contents",
            animation_page(
                "@keyframes k { to { color: red } } \
                 .d { display: contents; animation: k 1s linear infinite } \
                 p { margin: 20px; font-size: 40px }",
                "<div class=d><p>Text</p></div>",
            ),
            waits(),
            6,
            usize::MAX,
        ),
        (
            // Starts at x = -100 (off to the left) and moves in.
            "transform into view",
            animation_page(
                "@keyframes k { from { transform: translateX(0) } \
                 to { transform: translateX(400px) } } \
                 .a { position: absolute; top: 20px; left: -100px; width: 50px; height: 50px; \
                 background: blue; animation: k 2s linear infinite }",
                "<div class=a></div>",
            ),
            waits(),
            4,
            usize::MAX,
        ),
    ]
}

#[test]
fn animation_frames_are_asked_for_when_pixels_change() {
    for javascript in [false, true] {
        let results = on_page_thread(move || {
            animation_cases()
                .into_iter()
                .chain(moving_cases())
                .map(|(name, html, actions, least, most)| {
                    let c = run_animation_case(name, &html, javascript, &actions);
                    (name, c, least, most)
                })
                .collect::<Vec<_>>()
        });
        for (name, c, least, most) in results {
            assert!(
                (least..=most).contains(&c.frames),
                "{name} (js {javascript}): {} frames, expected {least}..={most} \
                 ({} wasted, {} steps without a frame)",
                c.frames,
                c.wasted,
                c.checks
            );
        }
    }
}

/// A playing video in real time with painted frames (M14 feature 7b): the
/// frame decision and the pixels agree. At each step the loop pumps, asks
/// `frame_outdated()` and renders; a render whose pixels differ from the
/// last one after a decision of "not outdated" fails, unless the next
/// due time of the page came after the decision was asked for (a frame that
/// became due during the render is not a miss).
#[cfg(feature = "media")]
#[test]
fn a_playing_video_asks_for_frames_when_its_pixels_change() {
    use common::media_site::MediaSite;
    use std::time::Instant;
    on_page_thread(|| {
        let mut site = MediaSite::new(
            "case-1",
            "<!DOCTYPE html><style>body{margin:0;background:#ccc}\
            video{display:block;margin:100px 0 0 200px}</style>\
            <video id=v src=a.mp4 preload=auto muted loop autoplay></video>",
        );
        site.add_file("a.mp4", "fixtures/media/mp4-h264-high-aac.mp4");
        let mut page = new_page(
            Arc::new(site) as Arc<dyn Fetcher>,
            2,
            Size::new(800.0, 600.0),
        );
        page.set_javascript(true);
        page.set_paints_frames(true);
        page.set_clock(PageClock::new(ClockMode::Real, None));
        page.navigate(Url::parse(&MediaSite::page_url("case-1")).unwrap());
        assert!(page.wait_until_loaded(Duration::from_secs(30)));
        let mut target = swb_engine::Pixmap::new(800, 600).unwrap();
        let (mut last, mut frames, mut checks, mut raced) = (None::<Vec<u8>>, 0, 0, 0);
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(2500) {
            page.process_network();
            page.pump();
            let due = page.next_task_instant();
            let asked = Instant::now();
            let outdated = page.frame_outdated();
            page.render(&mut target);
            let data = target.data().to_vec();
            if outdated {
                frames += 1;
            } else {
                checks += 1;
                if last.as_ref().is_some_and(|last| *last != data) {
                    assert!(
                        due.is_some_and(|t| t > asked),
                        "the frame was not outdated, but the pixels changed (due {due:?})"
                    );
                    raced += 1;
                }
            }
            last = Some(data);
            std::thread::sleep(Duration::from_millis(3));
        }
        // 25 fps for the part of the 2.5 s after the first frame.
        assert!(frames >= 25, "{frames} outdated frames, {checks} checks");
        assert!(raced <= 3, "{raced} frames came during a render");
    });
}
