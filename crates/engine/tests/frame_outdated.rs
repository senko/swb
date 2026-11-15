//! `Page::frame_outdated` (ADR 0040 section 2): after every kind of change
//! that alters pixels it is true, after a render it is false, and a task
//! that changes nothing leaves it false. The pages come from an in-memory
//! fetcher; resources arrive after a delay so that they come after a render.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use swb_engine::{Key, Modifiers, MouseButton, Page, Pixmap, Point, Size, Url};
use swb_net::{Fetcher, Headers, NetError, Request, Response};

mod common;
use common::{center, finish_loading, new_page, node, on_page_thread, rect, value};

const URL: &str = "https://site.test/page.html";
const VIEW: Size = Size::new(400.0, 300.0);

/// Serves pages at once and other resources after `delay`.
struct Site {
    files: HashMap<String, (&'static str, Vec<u8>)>,
    delay: Duration,
}

impl Fetcher for Site {
    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let url = request.url.as_str();
        let Some((content_type, body)) = self.files.get(url) else {
            return Ok(Response {
                url: request.url.clone(),
                status: 404,
                headers: Headers::default(),
                body: b"not found".to_vec(),
                redirected: false,
            });
        };
        if !content_type.starts_with("text/html") {
            std::thread::sleep(self.delay);
        }
        let headers: Headers = [("content-type", *content_type)].into_iter().collect();
        Ok(Response {
            url: request.url.clone(),
            status: 200,
            headers,
            body: body.clone(),
            redirected: false,
        })
    }
}

fn site(html: &str) -> Site {
    let mut files = HashMap::new();
    files.insert(
        URL.to_owned(),
        ("text/html; charset=utf-8", html.as_bytes().to_vec()),
    );
    Site {
        files,
        delay: Duration::from_millis(150),
    }
}

fn load_site(site: Site, javascript: bool) -> Page {
    let mut page = new_page(Arc::new(site) as Arc<dyn Fetcher>, 2, VIEW);
    page.set_javascript(javascript);
    page.set_scroll_indicators(true);
    page.navigate(Url::parse(URL).unwrap());
    finish_loading(&mut page, Duration::from_secs(20));
    page
}

/// A page for `html` with JavaScript off; it has been rendered.
fn load(html: &str) -> Page {
    let mut page = load_site(site(html), false);
    draw(&mut page);
    page
}

/// A page for `html` with JavaScript on; it has been rendered.
fn load_js(html: &str) -> Page {
    let mut page = load_site(site(html), true);
    draw(&mut page);
    page
}

/// Renders the viewport.
fn draw(page: &mut Page) -> Pixmap {
    let size = page.viewport();
    let mut pixmap = Pixmap::new(size.width as u32, size.height as u32).unwrap();
    page.render(&mut pixmap);
    pixmap
}

/// `change` makes the frame outdated; a render makes it current again.
fn changes(page: &mut Page, what: &str, change: impl FnOnce(&mut Page)) {
    assert!(!page.frame_outdated(), "{what}: outdated before the change");
    change(page);
    assert!(page.frame_outdated(), "{what}: the change did not show");
    draw(page);
    assert!(
        !page.frame_outdated(),
        "{what}: still outdated after a render"
    );
}

/// Runs the page's event loop until the frame is outdated.
fn wait_outdated(page: &mut Page, what: &str) {
    let started = Instant::now();
    while !page.frame_outdated() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{what}: nothing arrived"
        );
        page.process_network();
        page.pump();
        std::thread::sleep(Duration::from_millis(5));
    }
    draw(page);
    assert!(!page.frame_outdated(), "{what}: outdated after a render");
}

/// Pumps the event loop for `duration`.
fn run(page: &mut Page, duration: Duration) {
    let started = Instant::now();
    while started.elapsed() < duration {
        page.pump();
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn the_first_frame_is_outdated_and_a_render_makes_it_current() {
    let mut page = load_site(site("<p>text</p>"), false);
    assert!(page.frame_outdated());
    draw(&mut page);
    assert!(!page.frame_outdated());
    // Nothing changes by itself.
    page.pump();
    assert!(!page.frame_outdated());
}

#[test]
fn a_screenshot_is_not_a_frame() {
    let mut page = load("<p id=p>text</p><script>1</script>");
    page.scroll_to(Point::new(0.0, 0.0));
    let _ = page.screenshot(false).unwrap();
    assert!(!page.frame_outdated());
    page.set_viewport(Size::new(300.0, 300.0), 1.0);
    let _ = page.screenshot(false).unwrap();
    assert!(page.frame_outdated());
}

const LINKS: &str = "<!DOCTYPE html><style>body { margin: 0; font: 20px sans-serif }\
    #a:hover { text-decoration: none } #b:active { background: red }\
    #plain { height: 50px }</style>\
    <p style='margin:0'><a id=a href='t.html'>a link</a> and <span id=b>an active</span></p>\
    <a id=n href='n.html' style='display:block;height:40px'></a>\
    <div id=plain></div><p id=text>Some text to select</p>";

#[test]
fn hover_and_the_status_link() {
    on_page_thread(|| {
        let mut page = load(LINKS);
        let (x, y) = center(rect(&mut page, "a"));
        changes(&mut page, "hover a link", |p| {
            p.mouse_move(x, y);
        });
        changes(&mut page, "leave a link with a style", |p| {
            let (x, y) = center(rect(p, "plain"));
            p.mouse_move(x, y);
        });
        // The link changes and the style does not (no `:hover` rule on
        // `#n`): the page's pixels stay, only the shell's status bubble
        // changes (ADR 0040 section 5).
        let (x, y) = center(rect(&mut page, "n"));
        page.mouse_move(x, y);
        assert!(page.hovered_link().is_some());
        assert!(!page.frame_outdated(), "a link without a style");
        let (x, y) = center(rect(&mut page, "plain"));
        page.mouse_move(x, y);
        assert!(page.hovered_link().is_none());
        // Moves that change nothing leave the frame current.
        assert!(!page.frame_outdated());
        page.mouse_move(x + 1.0, y);
        assert!(!page.frame_outdated());
        let (x, y) = center(rect(&mut page, "a"));
        changes(&mut page, "hover again", |p| {
            p.mouse_move(x, y);
        });
        changes(&mut page, "pointer leaves", |p| {
            p.mouse_leave();
        });
    });
}

#[test]
fn active_state_and_selection() {
    on_page_thread(|| {
        let mut page = load(LINKS);
        let (x, y) = center(rect(&mut page, "b"));
        changes(&mut page, "press", |p| {
            p.mouse_down(x, y, MouseButton::Primary, Modifiers::NONE, 1);
        });
        changes(&mut page, "release", |p| {
            p.mouse_up(x, y, MouseButton::Primary);
        });
        let r = rect(&mut page, "text");
        changes(&mut page, "double click selects a word", |p| {
            p.mouse_down(
                r.x + 5.0,
                r.y + 5.0,
                MouseButton::Primary,
                Modifiers::NONE,
                2,
            );
            p.mouse_up(r.x + 5.0, r.y + 5.0, MouseButton::Primary);
        });
        changes(&mut page, "drag extends the selection", |p| {
            p.mouse_down(
                r.x + 5.0,
                r.y + 5.0,
                MouseButton::Primary,
                Modifiers::NONE,
                1,
            );
            p.mouse_move(r.x + 100.0, r.y + 5.0);
            p.mouse_up(r.x + 100.0, r.y + 5.0, MouseButton::Primary);
        });
        changes(&mut page, "select all", |p| {
            p.select_all();
        });
        changes(&mut page, "clear the selection", |p| {
            p.clear_selection();
        });
    });
}

const FORM: &str = "<!DOCTYPE html><style>body { margin: 0; font: 16px sans-serif }\
    input { width: 60px }</style>\
    <input id=i value='some long text in a field'><textarea id=t rows=2 cols=10>a\nb\nc\nd\ne\nf</textarea>\
    <button id=btn>go</button><input id=c type=checkbox>\
    <a id=l href='x.html'>focusable link</a>";

#[test]
fn focus_ring_caret_selection_and_values() {
    on_page_thread(|| {
        let mut page = load(FORM);
        let input = node(&page, "i");
        changes(&mut page, "focus a field (ring and caret)", |p| {
            p.focus(Some(input), false);
        });
        changes(&mut page, "type", |p| {
            p.insert_text("xy");
        });
        changes(&mut page, "move the caret", |p| {
            p.key_down(&Key::ArrowLeft, Modifiers::NONE);
        });
        changes(&mut page, "select with the keyboard", |p| {
            p.key_down(
                &Key::ArrowLeft,
                Modifiers {
                    shift: true,
                    ..Modifiers::NONE
                },
            );
        });
        changes(&mut page, "select all in the field", |p| {
            p.key_down(&Key::Character("a".into()), common::CTRL);
        });
        changes(&mut page, "delete", |p| {
            p.key_down(&Key::Backspace, Modifiers::NONE);
        });
        // The text scrolls inside the field as the caret moves to the end.
        changes(&mut page, "type a long text", |p| {
            p.insert_text("a very long text that does not fit in the field");
        });
        changes(&mut page, "home scrolls the text", |p| {
            p.key_down(&Key::Home, Modifiers::NONE);
        });
        let area = node(&page, "t");
        changes(&mut page, "move the focus to the text area", |p| {
            p.focus(Some(area), true);
        });
        changes(&mut page, "text area end", |p| {
            p.key_down(
                &Key::End,
                Modifiers {
                    ctrl: true,
                    ..Modifiers::NONE
                },
            );
        });
        let check = node(&page, "c");
        changes(&mut page, "focus a checkbox", |p| {
            p.focus(Some(check), true);
        });
        changes(&mut page, "toggle the checkbox", |p| {
            p.key_down(&Key::Character(" ".into()), Modifiers::NONE);
        });
        changes(&mut page, "tab to the next control", |p| {
            p.key_down(&Key::Tab, Modifiers::NONE);
        });
        changes(&mut page, "blur", |p| {
            p.focus(None, false);
        });
    });
}

#[test]
fn a_click_into_a_field_places_the_caret() {
    on_page_thread(|| {
        let mut page = load(FORM);
        let (x, y) = center(rect(&mut page, "i"));
        changes(&mut page, "click in the field", |p| {
            p.mouse_down(x, y, MouseButton::Primary, Modifiers::NONE, 1);
            p.mouse_up(x, y, MouseButton::Primary);
        });
        let (x, y) = center(rect(&mut page, "btn"));
        changes(&mut page, "click a button", |p| {
            p.mouse_down(x, y, MouseButton::Primary, Modifiers::NONE, 1);
            p.mouse_up(x, y, MouseButton::Primary);
        });
    });
}

const TALL: &str = "<!DOCTYPE html><style>body { margin: 0 } \
    #box { width: 200px; height: 100px; overflow: auto } #inner { height: 600px }\
    #tall { height: 1500px; background: linear-gradient(white, blue) }</style>\
    <div id=box><div id=inner>inner</div></div><div id=tall></div>";

#[test]
fn scrolling_the_page_and_a_scroll_container() {
    on_page_thread(|| {
        let mut page = load(TALL);
        changes(&mut page, "wheel over the page", |p| {
            p.wheel(300.0, 200.0, 0.0, 100.0);
        });
        changes(&mut page, "scroll_by", |p| p.scroll_by(0.0, 50.0));
        changes(&mut page, "scroll_to", |p| {
            p.scroll_to(Point::new(0.0, 0.0));
        });
        changes(&mut page, "wheel over the container", |p| {
            p.wheel(50.0, 50.0, 0.0, 40.0);
        });
        let target = node(&page, "box");
        changes(&mut page, "scroll the container", |p| {
            p.scroll_element_to(target, Point::new(0.0, 200.0));
        });
        // A scroll that changes nothing leaves the frame current.
        assert!(!page.frame_outdated());
        page.scroll_element_to(target, Point::new(0.0, 200.0));
        assert!(!page.frame_outdated());
        changes(&mut page, "scroll into view", |p| {
            let n = node(p, "tall");
            p.scroll_into_view(n);
        });
        changes(&mut page, "keyboard scrolling", |p| {
            p.key_down(&Key::PageDown, Modifiers::NONE);
        });
    });
}

#[test]
fn viewport_and_scale() {
    on_page_thread(|| {
        let mut page = load(TALL);
        changes(&mut page, "resize", |p| {
            p.set_viewport(Size::new(300.0, 200.0), 1.0);
        });
        changes(&mut page, "scale", |p| {
            p.set_viewport(Size::new(300.0, 200.0), 2.0);
        });
        changes(&mut page, "color scheme", |p| p.set_color_scheme(true));
        changes(&mut page, "indicators off", |p| {
            p.set_scroll_indicators(false);
        });
    });
}

#[test]
fn script_changes_and_tasks_that_change_nothing() {
    on_page_thread(|| {
        let mut page = load_js(
            "<!DOCTYPE html><style>.x { color: red }</style><p id=p>a</p><div style='height:2000px'></div>\
             <script>var n = 0; function tick() { n++; setTimeout(tick, 10); } tick();\
             function raf() { n++; requestAnimationFrame(raf); } raf();\
             var chan = new MessageChannel(); chan.port1.onmessage = () => { n++; };</script>",
        );
        // Timers and animation frames that do not change the page.
        run(&mut page, Duration::from_millis(300));
        assert!(!page.frame_outdated(), "idle timers and rAF");
        // Reads (layout queries and styles) do not change pixels either.
        value(
            &mut page,
            "document.getElementById('p').offsetHeight + getComputedStyle(document.body).color",
        );
        assert!(!page.frame_outdated(), "queries");
        changes(&mut page, "a DOM change by a script", |p| {
            value(p, "document.getElementById('p').textContent = 'b'; 1");
            run(p, Duration::from_millis(5));
        });
        changes(&mut page, "a class change", |p| {
            value(p, "document.getElementById('p').className = 'x'; 1");
        });
        changes(&mut page, "an inline style", |p| {
            value(p, "document.getElementById('p').style.color = 'green'; 1");
        });
        changes(&mut page, "a style sheet rule", |p| {
            value(
                p,
                "document.styleSheets[0].insertRule('#p { background: yellow }', 0); 1",
            );
        });
        changes(&mut page, "a new style element", |p| {
            value(
                p,
                "var s = document.createElement('style'); s.textContent = '#p{color:blue}'; \
                 document.head.appendChild(s); 1",
            );
        });
        changes(&mut page, "a script's selection", |p| {
            value(
                p,
                "getSelection().selectAllChildren(document.getElementById('p')); 1",
            );
        });
        changes(&mut page, "a script's focus", |p| {
            value(
                p,
                "var b = document.createElement('button'); b.textContent = 'b'; \
                 document.body.appendChild(b); 1",
            );
        });
        changes(&mut page, "a script's control value", |p| {
            value(
                p,
                "var i = document.createElement('input'); i.id = 'in'; document.body.appendChild(i); \
                 i.focus(); i.value = 'typed'; 1",
            );
        });
        changes(&mut page, "a script sets a control value later", |p| {
            value(p, "document.getElementById('in').value = 'other'; 1");
        });
        changes(&mut page, "a script's caret", |p| {
            value(
                p,
                "document.getElementById('in').setSelectionRange(1, 3); 1",
            );
        });
        changes(&mut page, "a script's scroll", |p| {
            value(p, "window.scrollTo(0, 5); 1");
        });
        changes(&mut page, "a message", |p| {
            value(p, "chan.port2.postMessage(1); document.title = 'x'; 1");
            run(p, Duration::from_millis(5));
            value(
                p,
                "document.getElementById('p').setAttribute('hidden', ''); 1",
            );
        });
        // Back to idle.
        run(&mut page, Duration::from_millis(100));
        draw(&mut page);
        run(&mut page, Duration::from_millis(100));
        assert!(!page.frame_outdated(), "idle again");
    });
}

#[test]
fn a_timer_that_changes_the_dom_draws_each_time() {
    on_page_thread(|| {
        let mut page = load_js(
            "<!DOCTYPE html><p id=p>0</p><script>var n = 0; setInterval(() => { \
             document.getElementById('p').textContent = ++n; }, 20);</script>",
        );
        wait_outdated(&mut page, "interval");
        wait_outdated(&mut page, "interval again");
    });
}

#[test]
fn details_and_the_top_layer() {
    on_page_thread(|| {
        let mut page = load_js(
            "<!DOCTYPE html><style>body{margin:0}</style>\
             <details id=d><summary id=s>sum</summary><p>content</p></details>\
             <dialog id=dlg>dialog</dialog><div id=pop popover>popover</div>\
             <button id=b popovertarget=pop>show</button>",
        );
        let (x, y) = center(rect(&mut page, "s"));
        changes(&mut page, "toggle details", |p| {
            p.click(x, y);
        });
        changes(&mut page, "show a modal dialog", |p| {
            value(p, "document.getElementById('dlg').showModal(); 1");
        });
        changes(&mut page, "close the dialog", |p| {
            value(p, "document.getElementById('dlg').close(); 1");
        });
        changes(&mut page, "show a popover", |p| {
            value(p, "document.getElementById('pop').showPopover(); 1");
        });
        changes(&mut page, "escape closes the popover", |p| {
            p.key_down(&Key::Escape, Modifiers::NONE);
        });
    });
}

fn png() -> Vec<u8> {
    common::tiny_png()
}

#[test]
fn images_and_lazy_images_arrive_later() {
    on_page_thread(|| {
        let mut site = site(
            "<!DOCTYPE html><style>body{margin:0} #sp{height:800px}</style>\
             <img id=a src='a.png' width=20 height=20>\
             <div id=sp></div><img id=z loading=lazy src='z.png' width=20 height=20>",
        );
        for name in ["a.png", "z.png"] {
            site.files
                .insert(format!("https://site.test/{name}"), ("image/png", png()));
        }
        let mut page = new_page(Arc::new(site) as Arc<dyn Fetcher>, 2, VIEW);
        page.navigate(Url::parse(URL).unwrap());
        // The image has a fixed size: layout is complete before it arrives.
        page.update_layout();
        draw(&mut page);
        assert!(!page.frame_outdated());
        wait_outdated(&mut page, "the image arrives");
        // The lazy image loads when it nears the viewport.
        changes(&mut page, "scroll to the lazy image", |p| {
            p.scroll_to(Point::new(0.0, 600.0));
        });
        wait_outdated(&mut page, "the lazy image arrives");
        run(&mut page, Duration::from_millis(50));
        assert!(!page.frame_outdated());
    });
}

#[test]
fn a_style_sheet_and_a_web_font_arrive_later() {
    on_page_thread(|| {
        let mut site = site(
            "<!DOCTYPE html><link rel=stylesheet href='s.css'>\
             <p id=p style='font: 24px W'>Hamburgefonstiv</p>",
        );
        let font =
            std::fs::read(common::repo_root().join("fixtures/fonts/DejaVuSans.ttf")).unwrap();
        site.files.insert(
            "https://site.test/s.css".to_owned(),
            (
                "text/css",
                b"@font-face{font-family:W;src:url(w.ttf)} body{background:#eef}".to_vec(),
            ),
        );
        site.files
            .insert("https://site.test/w.ttf".to_owned(), ("font/ttf", font));
        let mut page = new_page(Arc::new(site) as Arc<dyn Fetcher>, 2, VIEW);
        page.navigate(Url::parse(URL).unwrap());
        draw(&mut page);
        // Waiting for the style sheet: nothing to draw until it arrives.
        let before = page.frame_outdated();
        let _ = before;
        wait_outdated(&mut page, "the style sheet arrives");
        wait_outdated(&mut page, "the font arrives");
        run(&mut page, Duration::from_millis(50));
        assert!(!page.frame_outdated());
    });
}

#[test]
fn frame_documents() {
    on_page_thread(|| {
        let inner = "<!DOCTYPE html><style>body{margin:0}</style><div id=in style='height:800px'>\
                     inner</div><script>setTimeout(function () { \
                     document.getElementById('in').style.background = 'green'; }, 400);</script>";
        let mut site = site(
            "<!DOCTYPE html><style>body{margin:0} iframe{border:0;width:200px;height:100px}</style>\
             <iframe id=f src='inner.html'></iframe><p id=p>after</p>",
        );
        site.files.insert(
            "https://site.test/inner.html".to_owned(),
            ("text/html", inner.as_bytes().to_vec()),
        );
        let mut page = load_site(site, true);
        draw(&mut page);
        run(&mut page, Duration::from_millis(100));
        draw(&mut page);
        // The frame's own scroll.
        changes(&mut page, "wheel in the frame", |p| {
            p.wheel(50.0, 50.0, 0.0, 60.0);
        });
        // The frame's script changes its document.
        wait_outdated(&mut page, "the frame's timer");
        changes(&mut page, "the parent changes", |p| {
            value(p, "document.getElementById('p').textContent = 'x'; 1");
        });
        changes(&mut page, "the frame is removed", |p| {
            value(p, "document.getElementById('f').remove(); 1");
        });
    });
}
