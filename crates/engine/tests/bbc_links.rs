//! Links and `:hover` rules in the forms that the BBC page uses (target 5):
//! card links that wrap block content, the logo link with an inline SVG,
//! section title links with an SVG chevron, the links in the open menu of
//! `details`, and the hover rules of the styled-components style sheet. The
//! markup is condensed from the fixture `bbc`; the pages are files in a
//! temporary directory and nothing uses the network.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use swb_engine::{Cursor, Page};
use swb_style::{Color, Rgba, TextDecorationLine};

mod common;
use common::{Site, center, node, open_url, rect};

/// Loads `html` into an 800×600 page with the test fonts.
fn open(site: &Site, html: &str) -> Page {
    open_url(site.page("page.html", html))
}

fn hover(page: &mut Page, id: &str) {
    let (x, y) = center(rect(page, id));
    page.mouse_move(x, y);
}

fn underlined(page: &Page, id: &str) -> bool {
    let style = page.styles().unwrap().get(node(page, id)).unwrap();
    style.text_decoration_line == TextDecorationLine::UNDERLINE
}

fn opacity(page: &Page, id: &str) -> f32 {
    page.styles().unwrap().get(node(page, id)).unwrap().opacity
}

fn background(page: &Page, id: &str) -> Color {
    page.styles()
        .unwrap()
        .get(node(page, id))
        .unwrap()
        .background_color
}

fn url_ends_with(page: &Page, suffix: &str) -> bool {
    page.url().unwrap().as_str().ends_with(suffix)
}

const BASE: &str = "<!DOCTYPE html><style>\
    body { margin: 0; font: 16px/20px sans-serif }\
    a { color: #202224; text-decoration: none }\
    h2 { margin: 0; font-size: 18px; line-height: 22px }\
    p { margin: 0 }\
    ul { margin: 0; padding: 0; list-style: none }";

/// A card as on the BBC front page: a grid cell with a link around block
/// content, `:hover` rules inside `@media (hover: hover)` for the cell and
/// a rule on the inner card.
const CARD: &str = "\
    @media (hover: hover) {\
      .cell:hover { cursor: pointer }\
      .cell:hover h2 { text-decoration: underline }\
      .cell:hover img { opacity: 0.8 }\
    }\
    .card:hover h2 { text-decoration: underline }\
    </style>\
    <div class=cell id=cell style='display:grid;grid-template-columns:repeat(24,1fr);column-gap:16px'>\
     <div style='grid-column: span 12'><div><a id=a href='/news/articles/c1' class=anchor>\
      <div class=card id=card style='display:flex;flex-direction:column'>\
       <img id=img width=200 height=100 style='display:block'>\
       <div><h2 id=h>Heading of the card</h2></div>\
       <p id=p>Description of the card.</p>\
       <div><span id=meta>3 hrs ago</span></div>\
      </div></a></div></div>\
     <div style='grid-column: span 12'><a id=b href='/news/articles/c2'><h2 id=h2>Other</h2></a></div>\
    </div>\
    <div style='height:300px'></div>";

#[test]
fn a_card_link_wraps_block_content_and_follows_from_any_part() {
    let site = Site::new("bbc-card-click");
    let mut page = open(&site, &format!("{BASE}{CARD}"));
    for id in ["h", "p", "img", "meta"] {
        let (x, y) = center(rect(&mut page, id));
        let hit = page.hit_test(x, y).unwrap();
        let link = hit.link.unwrap_or_else(|| panic!("no link at {id}"));
        assert!(link.path().ends_with("/news/articles/c1"), "{id}: {link}");
    }
    let (x, y) = center(rect(&mut page, "p"));
    assert!(page.click(x, y));
    assert!(url_ends_with(&page, "/news/articles/c1"));
}

#[test]
fn card_hover_rules_apply_to_descendants_and_the_cursor() {
    let site = Site::new("bbc-card-hover");
    let mut page = open(&site, &format!("{BASE}{CARD}"));
    assert!(!underlined(&page, "h"));
    assert_eq!(opacity(&page, "img"), 1.0);
    assert_eq!(page.cursor(), Cursor::Default);
    // Hovering the description of the card: `.card:hover h2` and the cell
    // rules apply to the heading and the image.
    hover(&mut page, "p");
    assert!(underlined(&page, "h"));
    assert!((opacity(&page, "img") - 0.8).abs() < 1e-6);
    assert_eq!(page.cursor(), Cursor::Pointer);
    // The other card in the same cell is hovered too (the cell matches),
    // so its heading is underlined by the cell rule.
    assert!(underlined(&page, "h2"));
    page.mouse_leave();
    assert!(!underlined(&page, "h"));
    assert_eq!(opacity(&page, "img"), 1.0);
}

#[test]
fn a_hover_media_query_that_does_not_match_does_not_apply() {
    // Chromium's headless shell has `hover: hover`; a page that asks for
    // `hover: none` gets no rule.
    let site = Site::new("bbc-hover-none");
    let mut page = open(
        &site,
        &format!(
            "{BASE}@media (hover: none) {{ a:hover {{ text-decoration: underline }} }}\
             @media (hover: hover) {{ a:hover {{ color: #f00 }} }}</style>\
             <a id=a href='/x'>link</a>"
        ),
    );
    hover(&mut page, "a");
    assert!(!underlined(&page, "a"));
    let color = page.styles().unwrap().get(node(&page, "a")).unwrap().color;
    assert_eq!(color, Rgba::rgb(255, 0, 0));
}

#[test]
fn the_logo_link_with_an_inline_svg_follows_and_has_the_pointer() {
    let site = Site::new("bbc-logo");
    let mut page = open(
        &site,
        &format!(
            "{BASE}</style><header><a id=logo href='/' class=anchor>\
             <svg id=svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 112 32' \
              role=img aria-labelledby=t style='display:block;width:112px;height:32px'>\
             <title id=t>BBC</title>\
             <path d='M0 0h30v32H0z' fill='#202224'/><path d='M40 0h30v32H40z'/></svg></a></header>"
        ),
    );
    // Over a shape and over the empty part of the SVG box.
    for (x, y) in [(15.0, 16.0), (100.0, 16.0)] {
        let hit = page.hit_test(x, y).unwrap();
        let link = hit.link.unwrap();
        assert!(link.path().ends_with('/'), "{link}");
        page.mouse_move(x, y);
        assert_eq!(page.cursor(), Cursor::Pointer);
        assert!(page.hovered_link().is_some());
    }
    // The link is in the Tab order.
    page.key_down(&swb_engine::Key::Tab, swb_engine::Modifiers::NONE);
    assert_eq!(page.focused_element(), Some(node(&page, "logo")));
    let before = page.url().unwrap().clone();
    assert!(page.click(15.0, 16.0));
    assert_ne!(page.url().unwrap(), &before);
}

#[test]
fn a_section_title_link_with_a_chevron_svg_underlines_on_hover() {
    let site = Site::new("bbc-section-title");
    let mut page = open(
        &site,
        &format!(
            "{BASE}.title:hover {{ text-decoration: underline }}</style>\
             <a id=a href='/news'><h2 id=h class=title>More news\
             <svg id=chevron viewBox='0 0 32 32' width='1em' height='1em'>\
             <path d='M21.6 14.3 5 3l-.1 26 16.7-11.7z'/></svg></h2></a>"
        ),
    );
    assert!(!underlined(&page, "h"));
    // Hovering the chevron is hovering its title.
    hover(&mut page, "chevron");
    assert!(underlined(&page, "h"));
    assert!(page.hovered_link().is_some());
    let (x, y) = center(rect(&mut page, "chevron"));
    assert!(page.click(x, y));
    assert!(url_ends_with(&page, "/news"));
}

/// The no-script menu: a closed `details` whose summary has an SVG icon
/// and whose content is nested lists of links.
const MENU: &str = "\
    .link:hover { background-color: #e6e8ea }\
    details { display: block }\
    summary { display: block; list-style: none; cursor: pointer; width: 40px; height: 40px }\
    summary::-webkit-details-marker { display: none }\
    li { display: block }\
    .link { display: block; padding: 4px 8px }\
    </style>\
    <details id=d><summary id=s>\
     <svg viewBox='0 0 32 32' width=20 height=20><path d='M1 7.5h30V1.9H1z'/></svg></summary>\
     <ul><li><a id=home class=link href='/home'>Home</a></li><ul></ul>\
      <li><a id=news class=link href='/news'>News</a></li>\
      <ul><li><a id=uk class=link href='/news/uk'>UK</a></li><ul></ul></ul></ul></details>\
    <p id=after>After the menu</p>";

#[test]
fn the_menu_links_work_only_when_the_menu_is_open() {
    let site = Site::new("bbc-menu");
    let mut page = open(&site, &format!("{BASE}{MENU}"));
    let uk = rect(&mut page, "uk");
    // Closed: the links have boxes but cannot be hit.
    let hit = page.hit_test(uk.x + 4.0, uk.y + 4.0);
    assert!(hit.is_none_or(|h| h.link.is_none()));
    // Clicking the SVG icon of the summary opens the menu.
    let (x, y) = center(rect(&mut page, "s"));
    page.click(x, y);
    let details = page.document().unwrap().element(node(&page, "d")).unwrap();
    assert!(details.has_attr("open"));
    // The nested links are reachable and follow their hrefs.
    let uk = rect(&mut page, "uk");
    let hit = page.hit_test(uk.x + 4.0, uk.y + 4.0).unwrap();
    assert!(hit.link.unwrap().path().ends_with("/news/uk"));
    // `.link:hover` applies to the link under the pointer only.
    assert_eq!(background(&page, "uk"), Color::TRANSPARENT);
    hover(&mut page, "uk");
    assert_eq!(
        background(&page, "uk"),
        Color::Rgba(Rgba::rgb(230, 232, 234))
    );
    assert_eq!(background(&page, "news"), Color::TRANSPARENT);
    assert!(page.click(uk.x + 4.0, uk.y + 4.0));
    assert!(url_ends_with(&page, "/news/uk"));
}

#[test]
fn the_menu_links_are_reached_with_tab_only_when_open() {
    let site = Site::new("bbc-menu-tab");
    let mut page = open(&site, &format!("{BASE}{MENU}"));
    let tab = |page: &mut Page| {
        page.key_down(&swb_engine::Key::Tab, swb_engine::Modifiers::NONE);
        page.focused_element()
    };
    // The summary is the only stop of the closed menu.
    assert_eq!(tab(&mut page), Some(node(&page, "s")));
    assert_eq!(tab(&mut page), None);
    // Open (Enter on the summary): the three links follow.
    page.key_down(&swb_engine::Key::Tab, swb_engine::Modifiers::NONE);
    assert_eq!(page.focused_element(), Some(node(&page, "s")));
    page.key_down(&swb_engine::Key::Enter, swb_engine::Modifiers::NONE);
    assert_eq!(tab(&mut page), Some(node(&page, "home")));
    assert_eq!(tab(&mut page), Some(node(&page, "news")));
    assert_eq!(tab(&mut page), Some(node(&page, "uk")));
}

/// Navigation, button and search box rules of the page: a plain `:hover`
/// background on links and buttons, and a wrapper whose `:hover` and
/// `:focus-within` change the border and the margin and style a descendant
/// button.
const CONTROLS: &str = "\
    .nav:hover { background-color: #e6e8ea }\
    .menu-button:hover { background-color: #d2d4d6 }\
    .search:focus-within, .search:hover { border: 2px solid #000; margin: -1px }\
    .search { border: 1px solid #ccc; margin: 0; width: 200px; height: 30px }\
    .search:hover .go { background-color: #3a3c3e; color: #fff }\
    .go { background-color: #fff; color: #000; border: 0; width: 30px; height: 30px }\
    </style>\
    <ul><li><a id=nav class=nav href='/news'>News</a></li></ul>\
    <button id=menu class=menu-button>Menu</button>\
    <div id=search class=search><input id=input style='width:100px'>\
     <button id=go class=go>Go</button></div>";

#[test]
fn navigation_link_button_and_search_hover_rules_apply() {
    let site = Site::new("bbc-controls");
    let mut page = open(&site, &format!("{BASE}{CONTROLS}"));
    hover(&mut page, "nav");
    assert_eq!(
        background(&page, "nav"),
        Color::Rgba(Rgba::rgb(230, 232, 234))
    );
    hover(&mut page, "menu");
    assert_eq!(background(&page, "nav"), Color::TRANSPARENT);
    assert_eq!(
        background(&page, "menu"),
        Color::Rgba(Rgba::rgb(210, 212, 214))
    );
    // The search wrapper: hover changes its border (2 px, margin -1 px,
    // so the box keeps its position) and the descendant button.
    let before = rect(&mut page, "search");
    hover(&mut page, "input");
    let during = rect(&mut page, "search");
    assert_eq!((during.x, during.y), (before.x - 1.0, before.y - 1.0));
    assert_eq!(background(&page, "go"), Color::Rgba(Rgba::rgb(58, 60, 62)));
    page.mouse_leave();
    assert_eq!(rect(&mut page, "search"), before);
    assert_eq!(
        background(&page, "go"),
        Color::Rgba(Rgba::rgb(255, 255, 255))
    );
    // Focus inside the wrapper has the border too (`:focus-within`).
    let (x, y) = center(rect(&mut page, "input"));
    page.click(x, y);
    page.mouse_leave();
    let focused = rect(&mut page, "search");
    assert_eq!((focused.x, focused.y), (before.x - 1.0, before.y - 1.0));
}
