//! Form controls: typing and editing, the caret and the selection in text
//! fields, checkboxes, radio buttons, selects, labels, reset, validation
//! and submission (the requests that a submission sends, and the history
//! of `POST` results). Pages come from an in-memory fetcher that records
//! the requests; nothing uses the network.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::fmt::Write;
use std::sync::Arc;
use std::time::Duration;

use swb_engine::{Key, LoadState, Modifiers, MouseButton, Page, Rect, Size, Url};
use swb_layout::{BoxContent, FragmentRef};
use swb_net::{Fetcher, Method, Request};
use swb_paint::{DisplayItem, SELECTION_BACKGROUND};

mod common;
use common::{CTRL, SHIFT, TestSite, click, key, node, rect};

const ORIGIN: &str = "https://site.test/";

const TIMEOUT: Duration = Duration::from_secs(20);

/// Loads `html` as `https://site.test/` into an 800×600 page.
fn open(html: &str) -> (Page, Arc<TestSite>) {
    open_pages(&[("", html)])
}

/// Loads pages (paths relative to `https://site.test/`); the first one is
/// opened.
fn open_pages(pages: &[(&str, &str)]) -> (Page, Arc<TestSite>) {
    open_site(TestSite::default(), pages)
}

/// Adds `pages` to `site` and opens the first one. A path with a `:` is a
/// whole URL.
fn open_site(mut site: TestSite, pages: &[(&str, &str)]) -> (Page, Arc<TestSite>) {
    let url = |path: &str| {
        if path.contains(':') {
            path.to_owned()
        } else {
            format!("{ORIGIN}{path}")
        }
    };
    for (path, html) in pages {
        site.pages.insert(url(path), (*html).to_owned());
    }
    let site = Arc::new(site);
    let fetcher: Arc<dyn Fetcher> = Arc::clone(&site) as Arc<dyn Fetcher>;
    let mut page = common::new_page(fetcher, 2, Size::new(800.0, 600.0));
    page.navigate(Url::parse(&url(pages[0].0)).unwrap());
    common::finish_loading(&mut page, TIMEOUT);
    (page, site)
}

fn wait(page: &mut Page) {
    common::finish_loading(page, TIMEOUT);
}

/// The horizontal scroll offset of the first form control.
fn control_scroll(page: &mut Page) -> f32 {
    page.update_layout();
    let mut scroll = None;
    page.fragments().unwrap().walk(|f, _| {
        if let FragmentRef::Box(b) = f
            && let BoxContent::Control(c) = &b.content
            && scroll.is_none()
        {
            scroll = Some(c.scroll.x);
        }
    });
    scroll.unwrap()
}

/// Types text as key presses.
fn type_keys(page: &mut Page, text: &str) {
    for c in text.chars() {
        page.key_down(&Key::Character(c.to_string()), Modifiers::NONE);
    }
}

fn value(page: &Page, id: &str) -> String {
    page.control_value(node(page, id)).unwrap()
}

fn checked(page: &Page, id: &str) -> bool {
    page.control_checked(node(page, id)).unwrap()
}

/// The text of all text fragments of the page, in tree order.
fn shown_text(page: &mut Page) -> String {
    page.update_layout();
    let mut shown = String::new();
    page.fragments().unwrap().walk(|f, _| {
        if let FragmentRef::Text(t) = f {
            shown.push_str(&t.text);
        }
    });
    shown
}

const BODY: &str = "<!DOCTYPE html><body style='margin:0; font: 16px/20px sans-serif'>";

#[test]
fn typing_and_editing_a_text_field() {
    let (mut page, _) = open(&format!("{BODY}<input id=t value=abc>"));
    assert!(!page.has_editable_focus());
    assert!(!page.insert_text("x"));
    click(&mut page, "t");
    assert_eq!(page.focused_element(), Some(node(&page, "t")));
    assert!(page.has_editable_focus());
    // A click at the center of the empty part puts the caret at the end.
    type_keys(&mut page, "de");
    assert_eq!(value(&page, "t"), "abcde");
    key(&mut page, &Key::Backspace);
    key(&mut page, &Key::Home);
    key(&mut page, &Key::Delete);
    assert_eq!(value(&page, "t"), "bcd");
    page.key_down(&Key::End, SHIFT);
    assert_eq!(page.selected_text(), "bcd");
    assert!(page.insert_text("x y"));
    assert_eq!(value(&page, "t"), "x y");
    // Ctrl+A selects the field, not the page; a cut removes the text.
    page.key_down(&Key::Character("a".to_owned()), CTRL);
    assert_eq!(page.selected_text(), "x y");
    assert_eq!(page.cut_selection(), "x y");
    assert_eq!(value(&page, "t"), "");
    // Pasted line breaks become spaces in text fields (as in Chromium).
    page.insert_text("one\ntwo");
    assert_eq!(value(&page, "t"), "one two");
}

/// The caret rectangle of the control `id` (document coordinates), if it
/// has one.
fn caret_of(page: &mut Page, id: &str) -> Option<Rect> {
    let target = node(page, id);
    page.update_layout();
    let mut caret = None;
    page.fragments().unwrap().walk(|f, origin| {
        if let FragmentRef::Box(b) = f
            && b.node == Some(target)
            && let BoxContent::Control(c) = &b.content
        {
            let border = b.border_rect.translate(origin);
            caret = c.caret.map(|r| r.translate(border.origin()));
        }
    });
    caret
}

#[test]
fn the_caret_on_empty_lines_and_after_clusters() {
    let (mut page, _) = open(&format!(
        "{BODY}<textarea id=t rows=6>a\n\nb\nc</textarea><input id=f value='a\u{1F44D}\u{1F3FD}b'>"
    ));
    let t = rect(&mut page, "t");
    page.focus(Some(node(&page, "t")), false);
    // Text areas start with the caret at the start.
    let first = caret_of(&mut page, "t").unwrap();
    key(&mut page, &Key::ArrowDown);
    let second = caret_of(&mut page, "t").unwrap();
    key(&mut page, &Key::ArrowDown);
    let third = caret_of(&mut page, "t").unwrap();
    // The empty second line is between the first and the third.
    let line = third.y - second.y;
    assert!(line > 10.0, "{first:?} {second:?} {third:?}");
    assert!((second.y - first.y - line).abs() < 0.5);
    assert!(third.bottom() < t.bottom());
    // After a final line break, the caret is on a new line.
    page.key_down(&Key::End, CTRL);
    page.insert_text("\n");
    let last = caret_of(&mut page, "t").unwrap();
    assert!((last.y - (third.y + 2.0 * line)).abs() < 0.5, "{last:?}");
    // In a text field, the caret steps over a whole emoji cluster and
    // stays on the line.
    let f = rect(&mut page, "f");
    page.focus(Some(node(&page, "f")), false);
    key(&mut page, &Key::End);
    key(&mut page, &Key::ArrowLeft);
    key(&mut page, &Key::ArrowLeft);
    let caret = caret_of(&mut page, "f").unwrap();
    assert!(
        caret.y >= f.y && caret.bottom() <= f.bottom(),
        "{caret:?} {f:?}"
    );
    key(&mut page, &Key::Delete);
    assert_eq!(value(&page, "f"), "ab");
}

#[test]
fn keyboard_focus_selects_text_fields_but_not_text_areas() {
    let (mut page, _) = open(&format!(
        "{BODY}<input id=a value=field><textarea id=t>area</textarea>\
         <label id=l>Name <input id=n value=x></label><input id=p type=password value='ab cd'>"
    ));
    key(&mut page, &Key::Tab);
    assert_eq!(page.selected_text(), "field");
    key(&mut page, &Key::Tab);
    assert_eq!(page.focused_element(), Some(node(&page, "t")));
    assert_eq!(page.selected_text(), "");
    type_keys(&mut page, "x");
    assert_eq!(value(&page, "t"), "xarea");
    // A click on the text of a label focuses its field.
    let l = rect(&mut page, "l");
    page.click(l.x + 5.0, l.y + l.height / 2.0);
    assert_eq!(page.focused_element(), Some(node(&page, "n")));
    // A double click in a password field selects all of it.
    let p = rect(&mut page, "p");
    let (x, y) = (p.x + 8.0, p.y + p.height / 2.0);
    page.mouse_down(x, y, MouseButton::Primary, Modifiers::NONE, 1);
    page.mouse_up(x, y, MouseButton::Primary);
    page.mouse_down(x, y, MouseButton::Primary, Modifiers::NONE, 2);
    page.mouse_up(x, y, MouseButton::Primary);
    key(&mut page, &Key::Backspace);
    assert_eq!(value(&page, "p"), "");
}

#[test]
fn a_click_places_the_caret_and_a_drag_selects() {
    let (mut page, _) = open(&format!("{BODY}<input id=t value='aaaa bbbb' size=30>"));
    let r = rect(&mut page, "t");
    let y = r.y + r.height / 2.0;
    // At the left edge of the text: offset 0.
    page.click(r.x + 3.0, y);
    type_keys(&mut page, "_");
    assert_eq!(value(&page, "t"), "_aaaa bbbb");
    // Drag from the start to the far right selects everything.
    page.mouse_down(r.x + 3.0, y, MouseButton::Primary, Modifiers::NONE, 1);
    page.mouse_move(r.x + r.width - 5.0, y);
    page.mouse_up(r.x + r.width - 5.0, y, MouseButton::Primary);
    assert_eq!(page.selected_text(), "_aaaa bbbb");
    // A double click selects a word.
    page.mouse_down(r.x + 10.0, y, MouseButton::Primary, Modifiers::NONE, 1);
    page.mouse_up(r.x + 10.0, y, MouseButton::Primary);
    page.mouse_down(r.x + 10.0, y, MouseButton::Primary, Modifiers::NONE, 2);
    page.mouse_up(r.x + 10.0, y, MouseButton::Primary);
    assert_eq!(page.selected_text(), "_aaaa");
}

#[test]
fn the_caret_and_the_selection_are_painted() {
    let (mut page, _) = open(&format!("{BODY}<input id=t value=abc>"));
    let carets = |page: &mut Page| -> Vec<Rect> {
        common::display_list(page);
        let tree = page.fragments().unwrap();
        let mut out = Vec::new();
        tree.walk(|f, _| {
            if let FragmentRef::Box(b) = f
                && let BoxContent::Control(c) = &b.content
                && let Some(caret) = c.caret
            {
                out.push(caret);
            }
        });
        out
    };
    assert_eq!(carets(&mut page), Vec::<Rect>::new());
    click(&mut page, "t");
    let caret = carets(&mut page);
    assert_eq!(caret.len(), 1);
    assert_eq!(caret[0].width, 1.0);
    // Tab focus selects the text; the selection hides the caret and is
    // highlighted.
    page.focus(None, false);
    key(&mut page, &Key::Tab);
    assert_eq!(page.selected_text(), "abc");
    assert_eq!(carets(&mut page), Vec::<Rect>::new());
    let r = rect(&mut page, "t");
    let pixmap = page.screenshot(false).unwrap();
    let pixel = pixmap
        .pixel((r.x + 6.0) as u32, (r.y + r.height / 2.0) as u32)
        .unwrap();
    let blue = SELECTION_BACKGROUND;
    assert!(
        (i32::from(pixel.blue()) - i32::from(blue.b)).abs() < 40 && pixel.red() < 150,
        "{pixel:?}"
    );
}

#[test]
fn long_text_scrolls_to_keep_the_caret_visible() {
    let (mut page, _) = open(&format!("{BODY}<input id=t size=5>"));
    click(&mut page, "t");
    type_keys(&mut page, "a long text that does not fit");
    let at_end = control_scroll(&mut page);
    assert!(at_end > 50.0, "{at_end}");
    key(&mut page, &Key::Home);
    assert_eq!(control_scroll(&mut page), 0.0);
    // A selection scrolls to its moving end.
    page.key_down(&Key::End, SHIFT);
    assert_eq!(control_scroll(&mut page), at_end);
    page.key_down(&Key::Home, SHIFT);
    assert_eq!(control_scroll(&mut page), 0.0);
    // Without the focus, the text shows its start.
    key(&mut page, &Key::End);
    page.focus(None, false);
    assert_eq!(control_scroll(&mut page), 0.0);
}

#[test]
fn maxlength_readonly_and_disabled() {
    let (mut page, _) = open(&format!(
        "{BODY}<input id=m maxlength=3><input id=r readonly value=ro><input id=d disabled value=dis>\
         <input id=n type=number>"
    ));
    click(&mut page, "m");
    type_keys(&mut page, "abcdef");
    assert_eq!(value(&page, "m"), "abc");
    click(&mut page, "r");
    assert_eq!(page.focused_element(), Some(node(&page, "r")));
    assert!(!page.has_editable_focus());
    type_keys(&mut page, "x");
    key(&mut page, &Key::Backspace);
    assert_eq!(value(&page, "r"), "ro");
    // A disabled field cannot get the focus.
    click(&mut page, "d");
    assert_ne!(page.focused_element(), Some(node(&page, "d")));
    click(&mut page, "n");
    type_keys(&mut page, "1a2.5");
    assert_eq!(value(&page, "n"), "12.5");
}

#[test]
fn text_areas_keep_line_breaks() {
    let (mut page, site) = open(&format!(
        "{BODY}<form action=save method=post><textarea id=t name=t>first</textarea>\
         <input type=submit id=s></form>"
    ));
    click(&mut page, "t");
    page.key_down(&Key::End, CTRL);
    key(&mut page, &Key::Enter);
    type_keys(&mut page, "second");
    assert_eq!(value(&page, "t"), "first\nsecond");
    key(&mut page, &Key::ArrowUp);
    key(&mut page, &Key::Home);
    page.key_down(&Key::End, SHIFT);
    assert_eq!(page.selected_text(), "first");
    click(&mut page, "s");
    wait(&mut page);
    let request = site.last_request();
    assert_eq!(request.method, Method::Post);
    assert_eq!(request.body.as_deref(), Some(&b"t=first%0D%0Asecond"[..]));
}

#[test]
fn passwords_show_bullets_and_cannot_be_copied() {
    let (mut page, _) = open(&format!("{BODY}<input id=p type=password value=secret>"));
    assert_eq!(shown_text(&mut page), "\u{2022}".repeat(6));
    click(&mut page, "p");
    page.key_down(&Key::Character("a".to_owned()), CTRL);
    assert_eq!(page.selected_text(), "");
    key(&mut page, &Key::Backspace);
    assert_eq!(value(&page, "p"), "");
}

#[test]
fn checkboxes_radio_buttons_and_labels() {
    let (mut page, _) = open(
        "<!DOCTYPE html><style>body { margin: 0; font: 16px/20px sans-serif } \
         input:checked + span { color: red }</style>\
         <input id=c type=checkbox><span id=cs>c</span>\
         <label id=l for=c>label</label>\
         <label id=l2><input id=r1 type=radio name=g> one</label>\
         <label><input id=r2 type=radio name=g checked> two</label>\
         <input id=dis type=checkbox disabled>",
    );
    let color = |page: &Page| page.styles().unwrap().get(node(page, "cs")).unwrap().color;
    let black = color(&page);
    assert!(click(&mut page, "c"));
    assert!(checked(&page, "c"));
    assert_eq!(color(&page), swb_style::Rgba::rgb(255, 0, 0));
    // A click on the label toggles its control.
    click(&mut page, "l");
    assert!(!checked(&page, "c"));
    assert_eq!(color(&page), black);
    // Space toggles the focused checkbox.
    page.focus(Some(node(&page, "c")), true);
    type_keys(&mut page, " ");
    assert!(checked(&page, "c"));
    // Radio buttons: one per group; the label text checks its button.
    assert!(checked(&page, "r2"));
    click(&mut page, "l2");
    assert!(checked(&page, "r1") && !checked(&page, "r2"));
    // Arrows move to the next radio button of the group.
    key(&mut page, &Key::ArrowDown);
    assert!(checked(&page, "r2") && !checked(&page, "r1"));
    assert_eq!(page.focused_element(), Some(node(&page, "r2")));
    // Disabled checkboxes do not change.
    click(&mut page, "dis");
    assert!(!checked(&page, "dis"));
}

#[test]
fn selects_change_with_the_keyboard() {
    let (mut page, site) = open(&format!(
        "{BODY}<form action=go><select id=s name=s><option>alpha<option disabled>beta\
         <option value=g>gamma<option>delta</select><input type=submit id=b></form>"
    ));
    assert_eq!(value(&page, "s"), "alpha");
    click(&mut page, "s");
    key(&mut page, &Key::ArrowDown);
    assert_eq!(value(&page, "s"), "g");
    key(&mut page, &Key::End);
    assert_eq!(value(&page, "s"), "delta");
    type_keys(&mut page, "a");
    assert_eq!(value(&page, "s"), "alpha");
    key(&mut page, &Key::ArrowUp);
    assert_eq!(value(&page, "s"), "alpha");
    let label = shown_text(&mut page);
    assert!(label.starts_with("alpha"), "{label}");
    click(&mut page, "b");
    wait(&mut page);
    assert_eq!(
        site.last_request().url.as_str(),
        "https://site.test/go?s=alpha"
    );
}

#[test]
fn enter_submits_a_search_form() {
    // The Hacker News search form: one field, no submit button.
    let (mut page, site) = open(&format!(
        "{BODY}<form method=get action='//hn.algolia.com/'>Search: \
         <input id=q type=text name=q size=17></form>"
    ));
    click(&mut page, "q");
    type_keys(&mut page, "rust browser");
    assert!(key(&mut page, &Key::Enter));
    assert!(page.is_loading());
    assert_eq!(
        page.url().unwrap().as_str(),
        "https://hn.algolia.com/?q=rust+browser"
    );
    wait(&mut page);
    assert_eq!(page.title(), "result");
    let request = site.last_request();
    assert_eq!(request.method, Method::Get);
    assert!(request.body.is_none());
    assert!(page.go_back());
    wait(&mut page);
    assert_eq!(page.url().unwrap().as_str(), ORIGIN);
}

#[test]
fn implicit_submission_rules() {
    // Two fields and no button: Enter does nothing.
    let (mut page, site) = open(&format!(
        "{BODY}<form action=a><input id=a name=a><input id=b name=b></form>"
    ));
    click(&mut page, "a");
    type_keys(&mut page, "x");
    key(&mut page, &Key::Enter);
    assert!(!page.is_loading());
    assert_eq!(site.requests().len(), 1);
    // With a submit button, Enter clicks it (its name and value are sent).
    let (mut page, site) = open(&format!(
        "{BODY}<form action=a><input id=a name=a><input id=b name=b>\
         <input type=submit name=go value=Go></form>"
    ));
    click(&mut page, "a");
    type_keys(&mut page, "x");
    key(&mut page, &Key::Enter);
    wait(&mut page);
    assert_eq!(
        site.last_request().url.as_str(),
        "https://site.test/a?a=x&b=&go=Go"
    );
}

#[test]
fn login_form_posts_and_its_history_entry_asks_before_resubmitting() {
    let (mut page, site) = open_pages(&[
        (
            "login",
            "<html><body><form action=\"login\" method=\"post\">\
             <input type=\"hidden\" name=\"goto\" value=\"news\">\
             <table><tr><td>username:</td><td><input id=acct type=text name=acct size=20></td></tr>\
             <tr><td>password:</td><td><input id=pw type=password name=pw size=20></td></tr></table>\
             <input id=go type=submit value=login></form>",
        ),
        ("other", "<title>other</title>"),
    ]);
    click(&mut page, "acct");
    type_keys(&mut page, "senko");
    key(&mut page, &Key::Tab);
    assert_eq!(page.focused_element(), Some(node(&page, "pw")));
    type_keys(&mut page, "p@ss w");
    click(&mut page, "go");
    wait(&mut page);
    let request = site.last_request();
    assert_eq!(request.method, Method::Post);
    assert_eq!(request.url.as_str(), "https://site.test/login");
    assert_eq!(
        request.headers.get("content-type"),
        Some("application/x-www-form-urlencoded")
    );
    assert_eq!(
        request.body.as_deref(),
        Some(&b"goto=news&acct=senko&pw=p%40ss+w"[..])
    );
    // Leave the result and come back: no request is sent; the page asks
    // for a reload.
    page.navigate(Url::parse("https://site.test/other").unwrap());
    wait(&mut page);
    let count = site.requests().len();
    assert!(page.go_back());
    wait(&mut page);
    assert_eq!(site.requests().len(), count);
    assert_eq!(page.load_state(), LoadState::Failed);
    assert_eq!(page.url().unwrap().as_str(), "https://site.test/login");
    // A reload sends the form data again.
    page.reload();
    wait(&mut page);
    let resent = site.last_request();
    assert_eq!(resent.method, Method::Post);
    assert_eq!(resent.body, request.body);
    assert_eq!(page.load_state(), LoadState::Complete);
    assert!(page.can_go_forward());
}

#[test]
fn required_fields_block_submission() {
    let (mut page, site) = open(&format!(
        "{BODY}<style>input:invalid {{ color: red }}</style>\
         <form action=a><input id=a name=a required><input id=b name=b>\
         <input id=s type=submit></form>\
         <form action=n novalidate><input name=c required><input id=s2 type=submit></form>"
    ));
    click(&mut page, "s");
    assert!(!page.is_loading());
    assert_eq!(site.requests().len(), 1);
    // The first invalid field gets the focus.
    assert_eq!(page.focused_element(), Some(node(&page, "a")));
    let a = node(&page, "a");
    assert_eq!(
        page.styles().unwrap().get(a).unwrap().color,
        swb_style::Rgba::rgb(255, 0, 0)
    );
    type_keys(&mut page, "v");
    assert_ne!(
        page.styles().unwrap().get(a).unwrap().color,
        swb_style::Rgba::rgb(255, 0, 0)
    );
    click(&mut page, "s");
    wait(&mut page);
    assert_eq!(site.last_request().url.query(), Some("a=v&b="));
    // `novalidate` submits anyway.
    let (mut page, site) = open(&format!(
        "{BODY}<form action=n novalidate><input name=c required><input id=s type=submit></form>"
    ));
    click(&mut page, "s");
    wait(&mut page);
    assert_eq!(site.last_request().url.query(), Some("c="));
}

#[test]
fn reset_buttons_restore_the_defaults() {
    let (mut page, _) = open(&format!(
        "{BODY}<form><input id=t value=default><input id=c type=checkbox checked>\
         <button id=r type=reset>reset</button></form>"
    ));
    click(&mut page, "t");
    type_keys(&mut page, "x");
    click(&mut page, "c");
    assert_eq!(value(&page, "t"), "defaultx");
    assert!(!checked(&page, "c"));
    click(&mut page, "r");
    assert_eq!(value(&page, "t"), "default");
    assert!(checked(&page, "c"));
}

#[test]
fn placeholder_shown_follows_the_value() {
    let (mut page, _) = open(&format!(
        "{BODY}<style>input:placeholder-shown {{ color: red }}</style><input id=t placeholder=hint>"
    ));
    let t = node(&page, "t");
    let red = swb_style::Rgba::rgb(255, 0, 0);
    let color = |page: &Page| page.styles().unwrap().get(t).unwrap().color;
    assert_eq!(color(&page), red);
    // The placeholder text is drawn in the placeholder color.
    let list = common::display_list(&mut page);
    assert!(list.items.iter().any(|item| matches!(
        item,
        DisplayItem::Text { color, .. } if *color == swb_style::Rgba::rgb(0x75, 0x75, 0x75)
    )));
    click(&mut page, "t");
    type_keys(&mut page, "a");
    assert_ne!(color(&page), red);
}

#[test]
fn image_buttons_send_the_click_coordinate() {
    let (mut page, site) = open(&format!(
        "{BODY}<form action=img><input id=i type=image name=pos alt=Go></form>"
    ));
    let r = rect(&mut page, "i");
    page.click(r.x + 3.5, r.y + 2.5);
    wait(&mut page);
    assert_eq!(site.last_request().url.query(), Some("pos.x=3&pos.y=2"));
}

#[test]
fn many_controls_and_hostile_attributes_do_not_hang() {
    let mut html = String::from(BODY);
    html.push_str("<form id=f action=x>");
    html.push_str("<input id=first name=first><input type=submit name=go>");
    for i in 0..4000 {
        write!(
            html,
            "<input name=n{i} value={i} size=99999999999 maxlength=-5 form=f>\
             <input type=radio name=r checked required form=f>"
        )
        .unwrap();
    }
    html.push_str("<select name=s>");
    for i in 0..2000 {
        write!(html, "<option>{i}</option>").unwrap();
    }
    html.push_str("</select><textarea cols=0 rows=99999999999></textarea>");
    for _ in 0..200 {
        html.push_str("<form><fieldset disabled><legend>");
    }
    html.push_str("<input id=deep>");
    let started = std::time::Instant::now();
    let (mut page, site) = open(&html);
    page.update_layout();
    assert!(page.content_size().height.is_finite());
    let checked_radios = page
        .query_selector_all("input[type=radio]:checked")
        .unwrap()
        .len();
    assert_eq!(checked_radios, 1);
    // Typing and submitting do not depend on the square of the number of
    // controls.
    page.focus(Some(node(&page, "first")), false);
    type_keys(&mut page, "abc");
    key(&mut page, &Key::Enter);
    assert!(page.is_loading());
    wait(&mut page);
    let query = site.last_request().url.query().unwrap().to_owned();
    assert!(query.starts_with("first=abc&go=&n0=0&n1=1"), "{query}");
    assert!(query.ends_with("&n3999=3999&r=on&s=0"), "{query}");
    // About 0.1 s in a test build; the limit leaves room for slow machines.
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn a_post_result_and_its_fragments_share_a_document() {
    // A form that posts to its own URL.
    let (mut page, site) = open(&format!(
        "{BODY}<form method=post><input id=q name=q value=x>\
         <input type=submit id=s></form><a id=frag href='#part'>part</a><p id=part>p</p>"
    ));
    click(&mut page, "s");
    wait(&mut page);
    assert_eq!(site.last_request().method, Method::Post);
    // A fragment of the result: no request, and back goes to the result.
    let count = site.requests().len();
    click(&mut page, "frag");
    assert!(page.url().unwrap().as_str().ends_with("#part"));
    assert!(page.go_back());
    assert_eq!(site.requests().len(), count);
    assert_eq!(page.load_state(), LoadState::Complete);
    // Reloading the fragment entry sends the form again.
    assert!(page.go_forward());
    page.reload();
    wait(&mut page);
    assert_eq!(site.last_request().method, Method::Post);
    // Back to the form (same URL, a GET entry) loads it again.
    assert!(page.go_back());
    assert!(page.go_back());
    wait(&mut page);
    let last = site.last_request();
    assert_eq!((last.method, last.url.as_str()), (Method::Get, ORIGIN));
    assert_eq!(page.load_state(), LoadState::Complete);
    // Forward to the result asks before sending again; back from that page
    // loads the form.
    assert!(page.go_forward());
    wait(&mut page);
    assert_eq!(page.load_state(), LoadState::Failed);
    let count = site.requests().len();
    assert!(page.go_back());
    wait(&mut page);
    assert_eq!(site.requests().len(), count + 1);
    assert_eq!(page.load_state(), LoadState::Complete);
}

#[test]
fn a_disabled_default_button_blocks_implicit_submission() {
    let (mut page, site) = open(&format!(
        "{BODY}<form action=a><input id=q name=q><input type=submit disabled>\
         <input type=submit name=other></form>"
    ));
    click(&mut page, "q");
    key(&mut page, &Key::Enter);
    assert!(!page.is_loading());
    assert_eq!(site.requests().len(), 1);
}

#[test]
fn navigate_with_a_post_request() {
    let (mut page, site) = open(BODY);
    let mut request = Request {
        method: Method::Post,
        body: Some(b"x=1".to_vec()),
        ..Request::get(
            Url::parse("https://site.test/post").unwrap(),
            swb_net::Destination::Document,
        )
    };
    request.headers.set("content-type", "text/plain");
    page.navigate_with(request);
    wait(&mut page);
    let sent = site.last_request();
    assert_eq!(sent.method, Method::Post);
    assert_eq!(sent.headers.get("content-type"), Some("text/plain"));
    assert_eq!(page.title(), "result");
}

/// The initiator (the origin that `SameSite` cookies and the `Origin`
/// header use) of a submission stays with its history entry: a reload and
/// back and forward send it again (ADR 0012).
#[test]
fn a_submission_its_reload_and_history_keep_the_initiator() {
    let (mut page, site) = open(&format!(
        "{BODY}<form action=login method=post><input name=q value=x><input id=s type=submit></form>"
    ));
    let site_test = Some(Url::parse(ORIGIN).unwrap().origin());
    let other_test = Some(Url::parse("https://other.test/").unwrap().origin());
    let last = |site: &TestSite| {
        let r = site.last_request();
        (r.method, r.url.to_string(), r.initiator)
    };
    assert_eq!(last(&site).2, None);
    click(&mut page, "s");
    wait(&mut page);
    let login = (Method::Post, format!("{ORIGIN}login"), site_test.clone());
    assert_eq!(last(&site), login);
    page.reload();
    wait(&mut page);
    assert_eq!(last(&site), login);
    // A cross-site link, and a link from there back to site.test.
    assert!(page.follow_link(Url::parse("https://other.test/").unwrap()));
    wait(&mut page);
    let other = (Method::Get, "https://other.test/".to_owned(), site_test);
    assert_eq!(last(&site), other);
    assert!(page.follow_link(Url::parse(&format!("{ORIGIN}x")).unwrap()));
    wait(&mut page);
    let x = (Method::Get, format!("{ORIGIN}x"), other_test);
    assert_eq!(last(&site), x);
    assert!(page.go_back());
    wait(&mut page);
    assert_eq!(last(&site), other);
    assert!(page.go_forward());
    wait(&mut page);
    assert_eq!(last(&site), x);
    page.reload();
    wait(&mut page);
    assert_eq!(last(&site), x);
}

/// A reload while a `POST` is pending does not send the form again: it
/// reloads the page that has the form (as Chromium does).
#[test]
fn a_reload_during_a_post_reloads_the_current_page() {
    let (mut page, site) = open(&format!(
        "{BODY}<form action=login method=post><input name=q value=x><input id=s type=submit></form>"
    ));
    click(&mut page, "s");
    assert!(page.is_loading());
    page.reload();
    wait(&mut page);
    let requests = site.requests();
    // The POST can reach the server before the reload cancels it, but it is
    // not sent a second time.
    let posts = requests.iter().filter(|r| r.method == Method::Post).count();
    assert!(posts <= 1, "{requests:?}");
    let last = requests.last().unwrap();
    assert_eq!((last.method, last.url.as_str()), (Method::Get, ORIGIN));
    assert_eq!(page.url().unwrap().as_str(), ORIGIN);
    assert!(!page.can_go_back());
}

/// "POST/redirect/GET" to the same URL: the history entry keeps no body,
/// so a reload sends a `GET`.
#[test]
fn a_post_that_redirects_to_its_own_url_is_not_sent_again() {
    let site = TestSite {
        redirect_posts: true,
        ..TestSite::default()
    };
    let html =
        format!("{BODY}<form method=post><input name=q value=x><input id=s type=submit></form>");
    let (mut page, site) = open_site(site, &[("", &html)]);
    click(&mut page, "s");
    wait(&mut page);
    let methods =
        |site: &TestSite| -> Vec<Method> { site.requests().iter().map(|r| r.method).collect() };
    assert_eq!(methods(&site), [Method::Get, Method::Post, Method::Get]);
    assert!(site.last_request().body.is_none());
    page.reload();
    wait(&mut page);
    assert_eq!(methods(&site).last(), Some(&Method::Get));
    assert_eq!(page.load_state(), LoadState::Complete);
    // Back to the form page and forward again: no resubmission page.
    assert!(page.go_back());
    wait(&mut page);
    assert!(page.go_forward());
    wait(&mut page);
    assert_eq!(page.load_state(), LoadState::Complete);
    let posts = methods(&site).into_iter().filter(|&m| m == Method::Post);
    assert_eq!(posts.count(), 1);
}

/// `formaction`, `formenctype`, `formmethod` and `formnovalidate` on a
/// submit button override the form.
#[test]
fn submit_button_attributes_override_the_form() {
    let html = format!(
        "{BODY}<form action=a method=post enctype=text/plain><input name=q required>\
         <input id=s type=submit>\
         <button id=m formaction=b formenctype=multipart/form-data formnovalidate>m</button>\
         <button id=t formnovalidate>t</button>\
         <button id=g formmethod=get formnovalidate>g</button></form>"
    );
    let (mut page, site) = open(&html);
    // The required field blocks the plain submit button.
    click(&mut page, "s");
    assert!(!page.is_loading());
    assert_eq!(site.requests().len(), 1);
    click(&mut page, "m");
    wait(&mut page);
    let request = site.last_request();
    assert_eq!(
        (request.method, request.url.as_str()),
        (Method::Post, "https://site.test/b")
    );
    let content_type = request.headers.get("content-type").unwrap();
    assert!(
        content_type.starts_with("multipart/form-data; boundary="),
        "{content_type}"
    );
    let body = String::from_utf8(request.body.unwrap()).unwrap();
    assert!(
        body.contains("Content-Disposition: form-data; name=\"q\"\r\n\r\n\r\n"),
        "{body}"
    );

    let (mut page, site) = open(&html);
    click(&mut page, "t");
    wait(&mut page);
    let request = site.last_request();
    assert_eq!(request.headers.get("content-type"), Some("text/plain"));
    assert_eq!(request.body.as_deref(), Some(&b"q=\r\n"[..]));

    let (mut page, site) = open(&html);
    click(&mut page, "g");
    wait(&mut page);
    let request = site.last_request();
    assert_eq!(
        (request.method, request.url.as_str()),
        (Method::Get, "https://site.test/a?q=")
    );
}

/// A page in a legacy encoding submits in that encoding; characters that
/// the encoding does not have become numeric character references.
#[test]
fn a_legacy_encoding_page_submits_in_its_encoding() {
    let site = TestSite {
        content_type: Some("text/html"),
        ..TestSite::default()
    };
    let html = "<!DOCTYPE html><meta charset=windows-1252><body style='margin:0'>\
                <form action=go><input id=q name=q></form>";
    let (mut page, site) = open_site(site, &[("", html)]);
    click(&mut page, "q");
    page.insert_text("\u{e9}\u{20ac}\u{10d}");
    key(&mut page, &Key::Enter);
    wait(&mut page);
    assert_eq!(
        site.last_request().url.query(),
        Some("q=%E9%80%26%23269%3B")
    );
}

/// A web page cannot submit a form to a `file:` URL (as for links).
#[test]
fn a_form_cannot_submit_to_a_local_file() {
    let (mut page, site) = open(&format!(
        "{BODY}<form action='file:///etc/hostname'><input name=q><input type=submit id=s></form>"
    ));
    click(&mut page, "s");
    assert!(!page.is_loading());
    assert_eq!(site.requests().len(), 1);
    assert_eq!(page.url().unwrap().as_str(), ORIGIN);
}

/// A click on a control without an activation behavior (a `type=button`
/// button, a submit button without a form) or on a label without a control
/// follows the link around it, as in Chromium.
#[test]
fn controls_and_labels_inside_links_follow_the_link() {
    for inner in [
        "<button id=x type=button>Go</button>",
        "<button id=x>Go</button>",
        "<label id=x>Go</label>",
    ] {
        let (mut page, site) = open(&format!("{BODY}<a href=next>{inner}</a>"));
        assert!(click(&mut page, "x"), "{inner}");
        wait(&mut page);
        assert_eq!(
            site.last_request().url.as_str(),
            "https://site.test/next",
            "{inner}"
        );
    }
    // A checkbox inside a link toggles; the link is not followed.
    let (mut page, site) = open(&format!(
        "{BODY}<a href=next><input id=c type=checkbox></a>"
    ));
    click(&mut page, "c");
    assert!(checked(&page, "c"));
    assert!(!page.is_loading());
    assert_eq!(site.requests().len(), 1);
    // A label that clicks a control without an activation behavior focuses
    // the control, and the click goes on to the link around the control.
    for control in ["<input id=c>", "<button id=c type=button>b</button>"] {
        let (mut page, site) = open(&format!(
            "{BODY}<a href=next><label><span id=x>Name</span> {control}</label></a>"
        ));
        assert!(click(&mut page, "x"), "{control}");
        assert_eq!(page.focused_element(), Some(node(&page, "c")), "{control}");
        wait(&mut page);
        assert_eq!(
            site.last_request().url.as_str(),
            "https://site.test/next",
            "{control}"
        );
    }
    // A control outside the link: only the focus moves.
    let (mut page, site) = open(&format!(
        "{BODY}<a href=next><label for=c><span id=x>Name</span></label></a><input id=c>"
    ));
    click(&mut page, "x");
    assert_eq!(page.focused_element(), Some(node(&page, "c")));
    assert!(!page.is_loading());
    assert_eq!(site.requests().len(), 1);
}

/// A disabled control gets no click events, so a link around it is not
/// followed; a label with a disabled control does nothing (as in
/// Chromium).
#[test]
fn disabled_controls_inside_links_do_not_follow_the_link() {
    for inner in [
        "<input id=x disabled>",
        "<select id=x disabled><option>a</option></select>",
        "<button disabled><span id=x>b</span></button>",
        "<label><span id=x>text</span> <input type=checkbox id=c disabled></label>",
        "<fieldset disabled><label><span id=x>text</span> <input type=checkbox id=c></label></fieldset>",
    ] {
        let (mut page, site) = open(&format!("{BODY}<a href=next>{inner}</a>"));
        assert!(!click(&mut page, "x"), "{inner}");
        assert!(!page.is_loading(), "{inner}");
        assert_eq!(site.requests().len(), 1, "{inner}");
        if inner.contains("checkbox") {
            assert!(!checked(&page, "c"), "{inner}");
        }
    }
    // A disabled fieldset around plain text does not stop the link.
    let (mut page, site) = open(&format!(
        "{BODY}<a href=next><fieldset disabled><span id=x>t</span></fieldset></a>"
    ));
    assert!(click(&mut page, "x"));
    wait(&mut page);
    assert_eq!(site.last_request().url.as_str(), "https://site.test/next");
}

/// A click on interactive content inside a label goes to that content, not
/// to the labeled control.
#[test]
fn a_click_on_a_field_inside_a_label_for_another_field() {
    let (mut page, _) = open(&format!(
        "{BODY}<label id=l for=b>Label <input id=a></label><input id=b>"
    ));
    click(&mut page, "a");
    assert_eq!(page.focused_element(), Some(node(&page, "a")));
    // A click on the label text focuses the control that `for` names.
    let l = rect(&mut page, "l");
    page.click(l.x + 5.0, l.y + l.height / 2.0);
    assert_eq!(page.focused_element(), Some(node(&page, "b")));
    // `details` and images with `usemap` are interactive; a `summary`
    // outside `details` is not (measured in Chromium).
    for (inner, activates) in [
        (
            "<details open><summary>s</summary><span id=x>body</span></details>",
            false,
        ),
        (
            "<img id=x usemap=#m width=20 height=20 style='background: red'>",
            false,
        ),
        ("<summary id=x>sum</summary>", true),
    ] {
        let (mut page, _) = open(&format!(
            "{BODY}<label for=b>{inner}</label><map name=m></map><input id=b>"
        ));
        click(&mut page, "x");
        let focused_b = page.focused_element() == Some(node(&page, "b"));
        assert_eq!(focused_b, activates, "{inner}");
    }
}

/// Many controls in a disabled fieldset: loading, Tab and typing take
/// linear time.
#[test]
fn many_controls_in_a_disabled_fieldset() {
    let mut html = String::from(BODY);
    html.push_str("<fieldset disabled><legend><input id=l></legend>");
    for i in 0..20_000 {
        write!(html, "<input name=n{i}>").unwrap();
    }
    html.push_str("</fieldset><input id=t>");
    let started = std::time::Instant::now();
    let (mut page, _) = open(&html);
    // The field in the first legend is not disabled; Tab skips the others.
    key(&mut page, &Key::Tab);
    assert_eq!(page.focused_element(), Some(node(&page, "l")));
    key(&mut page, &Key::Tab);
    assert_eq!(page.focused_element(), Some(node(&page, "t")));
    type_keys(&mut page, "a");
    page.update_layout();
    assert_eq!(value(&page, "t"), "a");
    // About 0.3 s in a test build; a check that is quadratic in the number
    // of controls makes it much slower.
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
}

/// Out-of-range dates and huge years are sanitized to an empty value
/// without overflowing.
#[test]
fn hostile_date_values_become_empty() {
    let (page, _) = open(&format!(
        "{BODY}<input id=a type=week value=0000-W01><input id=b type=week value=4000000000-W01>\
         <input id=c type=date value=275760-09-14><input id=d type=month value=99999999999-01>\
         <input id=e type=datetime-local value='275760-09-13T00:00'>"
    ));
    for id in ["a", "b", "c", "d"] {
        assert_eq!(value(&page, id), "", "{id}");
    }
    assert_eq!(value(&page, "e"), "275760-09-13T00:00");
}

/// While the placeholder is shown, the field shows its start, also after
/// a long value scrolled it.
#[test]
fn the_placeholder_is_not_scrolled() {
    let (mut page, _) = open(&format!(
        "{BODY}<input id=t size=5 placeholder='a long placeholder that does not fit'>"
    ));
    click(&mut page, "t");
    type_keys(&mut page, "a long text that does not fit");
    assert!(control_scroll(&mut page) > 50.0);
    page.key_down(&Key::Character("a".to_owned()), CTRL);
    key(&mut page, &Key::Backspace);
    assert_eq!(control_scroll(&mut page), 0.0);
    let t = rect(&mut page, "t");
    let caret = caret_of(&mut page, "t").unwrap();
    assert!(
        caret.x >= t.x && caret.right() <= t.right(),
        "{caret:?} {t:?}"
    );
}

/// Where typing goes when a label focuses a field: before the value after
/// loading; after a reset, at the end of a text field and at the start of
/// a text area (measured in Chromium).
#[test]
fn the_caret_after_loading_and_after_a_reset() {
    let html = format!(
        "{BODY}<form><label id=li for=i>LI</label> <input id=i value=abc> \
         <label id=lt for=t>LT</label> <textarea id=t>abc</textarea> \
         <input type=reset id=r></form>"
    );
    let (mut page, _) = open(&html);
    click(&mut page, "li");
    type_keys(&mut page, "x");
    click(&mut page, "lt");
    type_keys(&mut page, "y");
    assert_eq!(
        (value(&page, "i"), value(&page, "t")),
        ("xabc".into(), "yabc".into())
    );
    click(&mut page, "r");
    click(&mut page, "li");
    type_keys(&mut page, "x");
    click(&mut page, "lt");
    type_keys(&mut page, "y");
    assert_eq!(
        (value(&page, "i"), value(&page, "t")),
        ("abcx".into(), "yabc".into())
    );
}

/// The initiator of entries for fragments of a `POST` result, of a
/// document with an opaque origin, and of a reload from the resubmission
/// page.
#[test]
fn initiators_of_fragments_opaque_origins_and_resubmission() {
    let form = "<form action='https://site.test/login' method=post><input name=q value=x>\
                <input id=s type=submit></form><a id=frag href='#part'>part</a><p id=part>p</p>";
    let site_test = Some(Url::parse(ORIGIN).unwrap().origin());
    // A fragment of a POST result: a reload sends the form with the
    // initiator of the result's entry.
    let (mut page, site) = open(&format!("{BODY}{form}"));
    click(&mut page, "s");
    wait(&mut page);
    assert!(page.follow_link(Url::parse(&format!("{ORIGIN}login#part")).unwrap()));
    page.reload();
    wait(&mut page);
    let last = site.last_request();
    assert_eq!(
        (last.method, last.initiator),
        (Method::Post, site_test.clone())
    );
    // From the resubmission page, a reload sends the initiator too.
    page.navigate(Url::parse(&format!("{ORIGIN}other")).unwrap());
    wait(&mut page);
    assert!(page.go_back());
    wait(&mut page);
    assert_eq!(page.load_state(), LoadState::Failed);
    page.reload();
    wait(&mut page);
    let last = site.last_request();
    assert_eq!((last.method, last.initiator), (Method::Post, site_test));
    // A `file:` document has an opaque origin; the entry keeps that origin.
    let (mut page, site) = open_pages(&[("file:///form.html", &format!("{BODY}{form}"))]);
    click(&mut page, "s");
    wait(&mut page);
    let submitted = site.last_request();
    let initiator = submitted.initiator.clone().unwrap();
    assert!(!initiator.is_tuple());
    page.reload();
    wait(&mut page);
    assert_eq!(site.last_request().initiator, Some(initiator));
}
