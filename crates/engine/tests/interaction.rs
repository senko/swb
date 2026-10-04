//! User interaction: hover and `:active` restyles, the cursor, clicks,
//! focus and the Tab key, text selection and the selected text. The pages
//! are files in a temporary directory; nothing uses the network.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{Cursor, Key, Modifiers, MouseButton, Page, Point, Rect, Size, Url};
use swb_net::NetworkFetcher;
use swb_paint::SELECTION_BACKGROUND;
use swb_style::TextDecorationLine;

mod common;
use common::{CTRL, SHIFT, Site, node, rect};

/// Loads `html` into an 800×600 page with the test fonts.
fn open(site: &Site, html: &str) -> (Page, Url) {
    let url = site.page("page.html", html);
    (open_url(url.clone()), url)
}

/// Loads `url` into an 800×600 page with the test fonts.
fn open_url(url: Url) -> Page {
    let fetcher = Arc::new(NetworkFetcher::new());
    let mut page = common::new_page(fetcher, 2, Size::new(800.0, 600.0));
    page.navigate(url);
    common::finish_loading(&mut page, Duration::from_secs(20));
    page
}

fn center(r: Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

fn press(page: &mut Page, x: f32, y: f32, clicks: u32) {
    page.mouse_down(x, y, MouseButton::Primary, Modifiers::NONE, clicks);
}

fn key(page: &mut Page, key: &Key, modifiers: Modifiers) -> bool {
    page.key_down(key, modifiers)
}

/// The address of the root fragment's children: it changes when the page
/// is laid out again.
fn layout_identity(page: &mut Page) -> *const () {
    page.update_layout();
    let root = page.fragments().unwrap().root.as_ref().unwrap();
    Arc::as_ptr(&root.children).cast()
}

const LINKS: &str = "<!DOCTYPE html><style>body { margin: 0; font: 20px sans-serif }\
    a { text-decoration: none } a:hover { text-decoration: underline }</style>\
    <p id=p style='margin:0'>Some text <a id=a href='target.html'>a link</a> more</p>\
    <p style='margin:0'><a id=b href='other.html'>other</a></p>\
    <div id=empty style='height:100px'></div>";

#[test]
fn hover_restyles_links() {
    let site = Site::new("hover");
    let (mut page, _) = open(&site, LINKS);
    let a = node(&page, "a");
    let decoration = |page: &Page| page.styles().unwrap().get(a).unwrap().text_decoration_line;
    assert_eq!(decoration(&page), TextDecorationLine::empty());
    let (x, y) = center(rect(&mut page, "a"));
    assert!(page.mouse_move(x, y));
    assert_eq!(decoration(&page), TextDecorationLine::UNDERLINE);
    assert!(page.hovered_link().is_some());
    assert!(page.mouse_leave());
    assert_eq!(decoration(&page), TextDecorationLine::empty());
}

#[test]
fn hover_without_style_changes_keeps_the_layout() {
    let site = Site::new("hover-layout");
    let (mut page, _) = open(&site, LINKS);
    let before = layout_identity(&mut page);
    // Moving over the paragraph text: `a:hover` does not match anything.
    page.mouse_move(5.0, 10.0);
    assert_eq!(layout_identity(&mut page), before);
    let (x, y) = center(rect(&mut page, "a"));
    page.mouse_move(x, y);
    assert_ne!(layout_identity(&mut page), before);
}

#[test]
fn cursor_shapes() {
    let site = Site::new("cursor");
    let (mut page, _) = open(
        &site,
        "<!DOCTYPE html><body style='margin:0'><p id=t style='margin:0'>text</p>\
         <a id=a href=x>link</a><div id=h style='cursor:help;height:50px'></div>\
         <p id=n style='user-select:none;margin:0'>none</p>",
    );
    let mut cursor_at = |id: &str| {
        let r = rect(&mut page, id);
        page.mouse_move(r.x + 2.0, r.y + r.height / 2.0);
        page.cursor()
    };
    assert_eq!(cursor_at("t"), Cursor::Text);
    assert_eq!(cursor_at("a"), Cursor::Pointer);
    assert_eq!(cursor_at("h"), Cursor::Help);
    assert_eq!(cursor_at("n"), Cursor::Default);
    page.mouse_move(700.0, 500.0);
    assert_eq!(page.cursor(), Cursor::Default);
}

#[test]
fn clicks_follow_links_but_drags_do_not() {
    let site = Site::new("click");
    let (mut page, url) = open(&site, LINKS);
    let (x, y) = center(rect(&mut page, "a"));
    // A drag that starts on the link selects text instead.
    press(&mut page, x, y, 1);
    page.mouse_move(x + 30.0, y);
    page.mouse_up(x, y, MouseButton::Primary);
    assert_eq!(page.url(), Some(&url));
    assert!(page.selection().is_some());
    // Pressing on one link and releasing on another does nothing.
    let (bx, by) = center(rect(&mut page, "b"));
    press(&mut page, x, y, 1);
    page.mouse_up(bx, by, MouseButton::Primary);
    assert_eq!(page.url(), Some(&url));
    // A click (with a small movement) follows the link.
    press(&mut page, x, y, 1);
    page.mouse_move(x + 2.0, y + 1.0);
    assert!(page.mouse_up(x + 2.0, y + 1.0, MouseButton::Primary));
    assert!(page.url().unwrap().as_str().ends_with("target.html"));
}

#[test]
fn active_state_while_pressed() {
    let site = Site::new("active");
    let (mut page, _) = open(&site, "<!DOCTYPE html><a id=a href=x>link</a>");
    let a = node(&page, "a");
    let color = |page: &Page| page.styles().unwrap().get(a).unwrap().color;
    let normal = color(&page);
    let (x, y) = center(rect(&mut page, "a"));
    press(&mut page, x, y, 1);
    // `a:any-link:active` has the color ActiveText (red).
    assert_eq!(color(&page), swb_style::Rgba::rgb(255, 0, 0));
    page.mouse_up(x + 50.0, y + 50.0, MouseButton::Primary);
    assert_eq!(color(&page), normal);
}

#[test]
fn tab_moves_the_focus_in_order() {
    let site = Site::new("tab");
    let (mut page, _) = open(
        &site,
        "<!DOCTYPE html><a id=a href=x>a</a> <a id=nohref>n</a> <a id=b href=y>b</a>\
         <div id=t tabindex=0>t</div><a id=hidden href=z style='display:none'>h</a>",
    );
    let ids = ["a", "b", "t"].map(|id| Some(node(&page, id)));
    for expected in ids {
        assert!(key(&mut page, &Key::Tab, Modifiers::NONE));
        assert_eq!(page.focused_element(), expected);
    }
    // After the last element, the focus leaves the page, then starts over.
    key(&mut page, &Key::Tab, Modifiers::NONE);
    assert_eq!(page.focused_element(), None);
    key(&mut page, &Key::Tab, Modifiers::NONE);
    assert_eq!(page.focused_element(), ids[0]);
    key(&mut page, &Key::Tab, SHIFT);
    assert_eq!(page.focused_element(), None);
    key(&mut page, &Key::Tab, SHIFT);
    assert_eq!(page.focused_element(), ids[2]);
}

#[test]
fn keyboard_focus_shows_a_focus_ring() {
    let site = Site::new("focus-ring");
    let (mut page, _) = open(&site, "<!DOCTYPE html><a id=a href=x>link</a>");
    let a = node(&page, "a");
    let outline = |page: &Page| page.styles().unwrap().get(a).unwrap().outline_style;
    // A click focuses the link, but `:focus-visible` does not match.
    let (x, y) = center(rect(&mut page, "a"));
    press(&mut page, x, y, 1);
    page.mouse_up(x + 50.0, y + 50.0, MouseButton::Primary);
    assert_eq!(page.focused_element(), Some(a));
    assert_eq!(outline(&page), swb_style::OutlineStyle::None);
    // Clicking elsewhere removes the focus.
    page.click(700.0, 500.0);
    assert_eq!(page.focused_element(), None);
    key(&mut page, &Key::Tab, Modifiers::NONE);
    assert_eq!(outline(&page), swb_style::OutlineStyle::Auto);
}

#[test]
fn tab_starts_after_the_last_click() {
    let site = Site::new("tab-start");
    let (mut page, _) = open(
        &site,
        "<!DOCTYPE html><body style='margin:0'><a id=a href=x>a</a>\
         <p id=p style='margin:0'>text</p><a id=b href=y>b</a>",
    );
    let (x, y) = center(rect(&mut page, "p"));
    page.click(x, y);
    key(&mut page, &Key::Tab, Modifiers::NONE);
    assert_eq!(page.focused_element(), Some(node(&page, "b")));
}

#[test]
fn enter_follows_the_focused_link() {
    let site = Site::new("enter");
    let (mut page, _) = open(&site, LINKS);
    assert!(!key(&mut page, &Key::Enter, Modifiers::NONE));
    key(&mut page, &Key::Tab, Modifiers::NONE);
    assert!(key(&mut page, &Key::Enter, Modifiers::NONE));
    assert!(page.url().unwrap().as_str().ends_with("target.html"));
}

#[test]
fn focus_scrolls_into_view() {
    let site = Site::new("focus-scroll");
    let (mut page, _) = open(
        &site,
        "<!DOCTYPE html><body style='margin:0'><div style='height:2000px'></div>\
         <a id=a href=x>far</a><div style='height:2000px'></div>",
    );
    key(&mut page, &Key::Tab, Modifiers::NONE);
    let r = rect(&mut page, "a");
    let scroll = page.scroll_position();
    assert!(
        r.y >= scroll.y && r.y + r.height <= scroll.y + 600.0,
        "{r:?} {scroll:?}"
    );
}

#[test]
fn scrolling_keys() {
    let site = Site::new("scroll-keys");
    let (mut page, _) = open(&site, "<!DOCTYPE html><div style='height:5000px'></div>");
    assert!(key(&mut page, &Key::ArrowDown, Modifiers::NONE));
    assert_eq!(page.scroll_position(), Point::new(0.0, 40.0));
    key(&mut page, &Key::Character(" ".to_owned()), Modifiers::NONE);
    assert_eq!(page.scroll_position().y, 40.0 + 525.0);
    key(&mut page, &Key::End, Modifiers::NONE);
    assert_eq!(page.scroll_position().y, page.content_size().height - 600.0);
    key(&mut page, &Key::Home, Modifiers::NONE);
    assert_eq!(page.scroll_position().y, 0.0);
    assert!(!key(
        &mut page,
        &Key::Character("x".to_owned()),
        Modifiers::NONE
    ));
}

const TEXT: &str = "<!DOCTYPE html><body style='margin:0;font:20px sans-serif'>\
    <p id=one style='margin:0 0 20px'>First   paragraph with words.</p>\
    <p id=two style='margin:0'>Second <b>bold</b> one.</p>";

#[test]
fn dragging_selects_text() {
    let site = Site::new("drag-select");
    let (mut page, _) = open(&site, TEXT);
    let one = rect(&mut page, "one");
    let two = rect(&mut page, "two");
    press(&mut page, one.x + 1.0, one.y + 10.0, 1);
    assert!(page.selection().is_none());
    // Beyond the end of the second paragraph's line.
    page.mouse_move(700.0, two.y + 10.0);
    assert_eq!(page.cursor(), Cursor::Text);
    page.mouse_up(700.0, two.y + 10.0, MouseButton::Primary);
    assert_eq!(
        page.selected_text(),
        "First paragraph with words.\n\nSecond bold one."
    );
    // A click elsewhere clears the selection.
    page.click(one.x + 1.0, one.y + 10.0);
    assert!(page.selection().is_none());
    assert_eq!(page.selected_text(), "");
}

#[test]
fn selection_is_highlighted() {
    let site = Site::new("highlight");
    let (mut page, _) = open(&site, TEXT);
    let highlighted = |page: &mut Page| {
        page.hit_test(0.0, 0.0);
        let mut pixmap = swb_engine::Pixmap::new(800, 600).unwrap();
        page.render(&mut pixmap);
        // The selection color appears in the rendering.
        let (pixels, _) = pixmap.data().as_chunks::<4>();
        let c = SELECTION_BACKGROUND;
        pixels.iter().any(|p| p[..3] == [c.r, c.g, c.b])
    };
    assert!(!highlighted(&mut page));
    key(&mut page, &Key::Character("a".to_owned()), CTRL);
    assert!(highlighted(&mut page));
}

#[test]
fn double_and_triple_clicks() {
    let site = Site::new("multi-click");
    let (mut page, _) = open(&site, TEXT);
    let one = rect(&mut page, "one");
    // Somewhere in "paragraph" (the second word).
    let (x, y) = (one.x + 100.0, one.y + 10.0);
    press(&mut page, x, y, 1);
    page.mouse_up(x, y, MouseButton::Primary);
    press(&mut page, x, y, 2);
    page.mouse_up(x, y, MouseButton::Primary);
    assert_eq!(page.selected_text(), "paragraph");
    press(&mut page, x, y, 3);
    page.mouse_up(x, y, MouseButton::Primary);
    assert_eq!(page.selected_text(), "First paragraph with words.");
}

#[test]
fn shift_click_extends_the_selection() {
    let site = Site::new("shift-click");
    let (mut page, _) = open(&site, TEXT);
    let one = rect(&mut page, "one");
    let two = rect(&mut page, "two");
    page.click(one.x + 1.0, one.y + 10.0);
    page.mouse_down(700.0, two.y + 10.0, MouseButton::Primary, SHIFT, 1);
    page.mouse_up(700.0, two.y + 10.0, MouseButton::Primary);
    assert!(page.selected_text().ends_with("Second bold one."));
}

#[test]
fn select_all_and_copy() {
    let site = Site::new("select-all");
    let (mut page, _) = open(
        &site,
        "<!DOCTYPE html><title>t</title><style>.x { display: none }</style>\
         <h1>Title</h1><div>one<br>two</div><ul><li>item <i>a</i></li><li>item b</li></ul>\
         <p class=x>hidden</p><pre>  pre\n  kept</pre><p>a\n  b</p>",
    );
    assert!(key(&mut page, &Key::Character("A".to_owned()), CTRL));
    assert_eq!(
        page.selected_text(),
        "Title\none\ntwo\nitem a\nitem b\n  pre\n  kept\n\na b"
    );
}

#[test]
fn selection_survives_a_relayout() {
    let site = Site::new("select-relayout");
    let (mut page, _) = open(&site, TEXT);
    key(&mut page, &Key::Character("a".to_owned()), CTRL);
    let text = page.selected_text();
    page.set_viewport(Size::new(100.0, 600.0), 1.0);
    page.update_layout();
    assert_eq!(page.selected_text(), text);
}

#[test]
fn fragment_targets_match_target() {
    let site = Site::new("target");
    let (mut page, url) = open(
        &site,
        "<!DOCTYPE html><style>:target { color: red }</style><p id=t>x</p><a id=a href=#t>go</a>",
    );
    let t = node(&page, "t");
    let color = |page: &Page| page.styles().unwrap().get(t).unwrap().color;
    let normal = color(&page);
    let mut target = url.clone();
    target.set_fragment(Some("t"));
    assert!(page.follow_link(target));
    assert_eq!(color(&page), swb_style::Rgba::rgb(255, 0, 0));
    assert!(page.go_back());
    assert_eq!(color(&page), normal);
}

#[test]
fn scrolling_does_not_restyle_hover() {
    let site = Site::new("scroll-hover");
    let (mut page, _) = open(
        &site,
        "<!DOCTYPE html><style>body { margin: 0 } div:hover { color: red }\
         div { height: 300px }</style><div id=a>a</div><div id=b>b</div><div>c</div>\
         <div style='height:2000px'></div>",
    );
    page.mouse_move(10.0, 10.0);
    let a = node(&page, "a");
    assert_eq!(
        page.styles().unwrap().get(a).unwrap().color,
        swb_style::Rgba::rgb(255, 0, 0)
    );
    let before = layout_identity(&mut page);
    page.scroll_by(0.0, 300.0);
    // The pointer is over #b now, but `:hover` waits for a mouse movement.
    assert_eq!(layout_identity(&mut page), before);
    page.mouse_move(10.0, 10.0);
    let b = node(&page, "b");
    assert_eq!(
        page.styles().unwrap().get(b).unwrap().color,
        swb_style::Rgba::rgb(255, 0, 0)
    );
}

#[test]
fn target_matches_on_the_first_load() {
    let site = Site::new("target-load");
    let mut url = site.page(
        "page.html",
        "<!DOCTYPE html><style>:target { color: red }</style><p id=t>x</p>",
    );
    url.set_fragment(Some("t"));
    let page = open_url(url);
    let t = node(&page, "t");
    assert_eq!(
        page.styles().unwrap().get(t).unwrap().color,
        swb_style::Rgba::rgb(255, 0, 0)
    );
}

#[test]
fn unselectable_text_is_not_copied() {
    let site = Site::new("user-select");
    let (mut page, _) = open(
        &site,
        "<!DOCTYPE html><p>alpha</p><p style='user-select:none'>beta <b>bold</b></p><p>gamma</p>",
    );
    key(&mut page, &Key::Character("a".to_owned()), CTRL);
    assert_eq!(page.selected_text(), "alpha\n\ngamma");
}

#[test]
fn table_cells_are_separated_by_tabs() {
    let site = Site::new("table-copy");
    let (mut page, _) = open(
        &site,
        "<!DOCTYPE html><table><tr><td>\n a\n</td><td>\n b\n</td></tr>\
         <tr><td style='display:none'>x</td><td>c</td><td>d</td></tr></table>",
    );
    key(&mut page, &Key::Character("a".to_owned()), CTRL);
    assert_eq!(page.selected_text(), "a\tb\nc\td");
}

#[test]
fn hover_rules_change_the_cursor_at_once() {
    let site = Site::new("hover-cursor");
    let (mut page, _) = open(
        &site,
        "<!DOCTYPE html><style>#d:hover { cursor: pointer }</style>\
         <div id=d style='height:50px'></div>",
    );
    let (x, y) = center(rect(&mut page, "d"));
    page.mouse_move(x, y);
    assert_eq!(page.cursor(), Cursor::Pointer);
}

#[test]
fn hidden_elements_cannot_have_the_focus() {
    let site = Site::new("hidden-focus");
    let (mut page, url) = open(
        &site,
        "<!DOCTYPE html><style>#menu a { display: none } #menu:hover a { display: inline }\
         </style><div id=menu style='height:40px'>menu <a id=a href='x.html'>item</a></div>\
         <a id=h href='y.html' style='display:none'>hidden</a>",
    );
    assert!(!page.focus(Some(node(&page, "h")), true));
    // A link in a hover menu: focused while visible, then hidden.
    let (x, y) = center(rect(&mut page, "menu"));
    page.mouse_move(x, y);
    key(&mut page, &Key::Tab, Modifiers::NONE);
    assert_eq!(page.focused_element(), Some(node(&page, "a")));
    page.mouse_leave();
    assert!(!key(&mut page, &Key::Enter, Modifiers::NONE));
    assert_eq!(page.url(), Some(&url));
}

#[test]
fn skip_links_move_the_focus_start() {
    let site = Site::new("skip-link");
    let (mut page, _) = open(
        &site,
        "<!DOCTYPE html><a id=skip href='#main'>skip</a><nav><a id=n href='x.html'>nav</a></nav>\
         <div id=main><a id=m href='y.html'>main</a></div><div id=t tabindex=-1>t</div>\
         <a id=to-t href='#t'>t</a>",
    );
    key(&mut page, &Key::Tab, Modifiers::NONE);
    assert_eq!(page.focused_element(), Some(node(&page, "skip")));
    key(&mut page, &Key::Enter, Modifiers::NONE);
    // #main is not focusable: the focus goes away, and Tab continues there.
    assert_eq!(page.focused_element(), None);
    key(&mut page, &Key::Tab, Modifiers::NONE);
    assert_eq!(page.focused_element(), Some(node(&page, "m")));
    // A focusable target gets the focus.
    let to_t = rect(&mut page, "to-t");
    let scroll = page.scroll_position();
    page.click(to_t.x + 2.0 - scroll.x, to_t.y + 2.0 - scroll.y);
    assert_eq!(page.focused_element(), Some(node(&page, "t")));
}

#[test]
fn triple_click_selects_a_paragraph() {
    let site = Site::new("paragraph");
    let (mut page, _) = open(
        &site,
        "<!DOCTYPE html><body style='margin:0;font:20px sans-serif'>\
         <div id=d>First paragraph<p>Second</p></div><p id=br>one<br>two</p>",
    );
    let d = rect(&mut page, "d");
    page.click(5.0, d.y + 10.0);
    press(&mut page, 5.0, d.y + 10.0, 3);
    page.mouse_up(5.0, d.y + 10.0, MouseButton::Primary);
    assert_eq!(page.selected_text(), "First paragraph");
    let br = rect(&mut page, "br");
    press(&mut page, 5.0, br.y + 10.0, 3);
    page.mouse_up(5.0, br.y + 10.0, MouseButton::Primary);
    assert_eq!(page.selected_text(), "one");
}

#[test]
fn a_click_needs_one_link_element() {
    let site = Site::new("same-href");
    let (mut page, url) = open(
        &site,
        "<!DOCTYPE html><p><a id=a href='x.html'>first</a></p><p><a id=b href='x.html'>second</a></p>",
    );
    let (ax, ay) = center(rect(&mut page, "a"));
    let (bx, by) = center(rect(&mut page, "b"));
    // A press on one link and a release on another link with the same
    // target (without movement events in between, so no drag) is not a
    // click.
    press(&mut page, ax, ay, 1);
    page.mouse_up(bx, by, MouseButton::Primary);
    assert_eq!(page.url(), Some(&url));
}

/// The pixels of a rendering of the page.
fn pixels(page: &mut Page) -> Vec<[u8; 4]> {
    let mut pixmap = swb_engine::Pixmap::new(800, 600).unwrap();
    page.render(&mut pixmap);
    pixmap.data().as_chunks::<4>().0.to_vec()
}

#[test]
fn a_partial_selection_highlights_only_its_part() {
    let site = Site::new("partial-highlight");
    let (mut page, _) = open(
        &site,
        "<!DOCTYPE html><body style='margin:0;font:20px sans-serif'>\
         <p id=p style='margin:0;line-height:40px'>abcdefghijkl</p>",
    );
    let p = rect(&mut page, "p");
    // Drag from about the third letter to about the eighth.
    press(&mut page, 25.0, p.y + 20.0, 1);
    page.mouse_move(80.0, p.y + 20.0);
    page.mouse_up(80.0, p.y + 20.0, MouseButton::Primary);
    let selected = page.selected_text();
    assert!(selected.len() > 2 && selected.len() < 10, "{selected}");
    let pixels = pixels(&mut page);
    let c = SELECTION_BACKGROUND;
    let blue = |x: usize, y: usize| pixels[y * 800 + x][..3] == [c.r, c.g, c.b];
    // The highlight fills the 40 px line box, between the drag ends.
    assert!(blue(50, 2) && blue(50, 37));
    assert!(!blue(10, 20) && !blue(110, 20));
}

/// The rings that a display list paints with `Border` items: rectangle,
/// width and color.
fn rings(page: &mut Page) -> Vec<(Rect, f32, swb_style::Rgba)> {
    common::display_list(page)
        .items
        .iter()
        .filter_map(|item| match item {
            swb_paint::DisplayItem::Border {
                rect,
                widths,
                colors,
                ..
            } => Some((*rect, widths[0], colors[0])),
            _ => None,
        })
        .collect()
}

#[test]
fn focus_ring_geometry() {
    let site = Site::new("focus-ring-geometry");
    let (mut page, _) = open(
        &site,
        "<!DOCTYPE html><body style='margin:0'>\
         <div id=d tabindex=0 style='margin:20px;width:40px;height:20px'></div>\
         <div id=b tabindex=0 style='margin:20px;width:40px;height:20px;border:2px solid #888'></div>\
         <div id=e tabindex=0 style='width:0;height:0'></div>",
    );
    let white = swb_style::Rgba::WHITE;
    let dark = swb_style::Rgba::rgb(0x10, 0x10, 0x10);
    // Chromium 148: the white ring at x = 18, the dark ring at x = 19..21.
    page.focus(Some(node(&page, "d")), true);
    let r: Vec<_> = rings(&mut page)
        .into_iter()
        .filter(|r| r.2 == white || r.2 == dark)
        .collect();
    assert_eq!(r.len(), 2, "{r:?}");
    assert_eq!((r[0].0.x, r[0].1, r[0].2), (18.0, 1.0, white));
    assert_eq!((r[1].0.x, r[1].1, r[1].2), (19.0, 2.0, dark));
    // With a border on every side, 1 px further in.
    page.focus(Some(node(&page, "b")), true);
    let dark_ring = rings(&mut page).into_iter().find(|r| r.2 == dark).unwrap();
    assert_eq!(dark_ring.0.x, 20.0);
    // An empty box gets no ring.
    page.focus(Some(node(&page, "e")), true);
    assert!(rings(&mut page).iter().all(|r| r.2 != dark));
}

#[test]
fn focus_rings_enclose_images_in_links() {
    let site = Site::new("focus-ring-image");
    let png = site.file("i.png", &common::tiny_png());
    let (mut page, _) = open(
        &site,
        &format!(
            "<!DOCTYPE html><body style='margin:20px'><a id=a href=x>\
             <img id=i src='{png}' style='width:40px;height:40px'></a>"
        ),
    );
    page.focus(Some(node(&page, "a")), true);
    let image = rect(&mut page, "i");
    let dark = swb_style::Rgba::rgb(0x10, 0x10, 0x10);
    let ring = rings(&mut page)
        .into_iter()
        .find(|r| r.2 == dark)
        .unwrap()
        .0;
    assert!(
        ring.y <= image.y && ring.y + ring.height >= image.y + image.height,
        "{ring:?} {image:?}"
    );
}

#[test]
fn a_selection_in_one_cell_has_no_tab() {
    let site = Site::new("cell-word");
    let (mut page, _) = open(
        &site,
        "<!DOCTYPE html><body style='margin:0;font:20px sans-serif'>\
         <table><tr><td id=a>alpha</td><td>beta</td></tr></table>",
    );
    let a = rect(&mut page, "a");
    let (x, y) = (a.x + 10.0, a.y + a.height / 2.0);
    page.click(x, y);
    press(&mut page, x, y, 2);
    page.mouse_up(x, y, MouseButton::Primary);
    assert_eq!(page.selected_text(), "alpha");
}

#[test]
fn focus_rings_do_not_enclose_clipped_overflow() {
    let site = Site::new("focus-ring-overflow");
    let (mut page, _) = open(
        &site,
        "<!DOCTYPE html><body style='margin:20px'>\
         <div id=d tabindex=0 style='width:100px;height:50px;overflow:hidden'>\
         <div style='width:300px;height:100px'></div></div>",
    );
    page.focus(Some(node(&page, "d")), true);
    let dark = swb_style::Rgba::rgb(0x10, 0x10, 0x10);
    let ring = rings(&mut page)
        .into_iter()
        .find(|r| r.2 == dark)
        .unwrap()
        .0;
    assert_eq!((ring.width, ring.height), (102.0, 52.0));
}
