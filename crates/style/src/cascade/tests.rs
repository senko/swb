//! The unit tests of the cascade that exercise the whole style pass on
//! small documents: selectors, inheritance, `var()`, layers, pseudo-elements.

use std::fmt::Write as _;

use swb_css::MediaEnvironment;
use swb_dom::{Document, parse_html};

use super::*;
use crate::content::content_text;
use crate::values::*;

fn base() -> Url {
    Url::parse("https://example.com/dir/page.html").expect("valid URL")
}

/// Styles `html` (a standards-mode document unless it says otherwise)
/// with one author stylesheet.
fn render_with(
    html: &str,
    css: &str,
    env: &MediaEnvironment,
    states: &ElementStates,
) -> (Document, StyleMap) {
    let doc = parse_html(html);
    let mut stylist = Stylist::new(doc.quirks_mode);
    stylist.add_author_sheet(&swb_css::parse_stylesheet(css), &base());
    let map = compute_styles(&doc, &stylist, env, states, &base());
    (doc, map)
}

fn render(html: &str, css: &str) -> (Document, StyleMap) {
    let html = format!("<!DOCTYPE html>{html}");
    render_with(
        &html,
        css,
        &MediaEnvironment::default(),
        &ElementStates::default(),
    )
}

fn node(doc: &Document, id: &str) -> NodeId {
    doc.element_by_id(id)
        .unwrap_or_else(|| panic!("no element #{id}"))
}

fn get<'a>(doc: &Document, map: &'a StyleMap, id: &str) -> &'a ComputedStyle {
    map.get(node(doc, id))
        .unwrap_or_else(|| panic!("#{id} has no style"))
}

fn rgb(r: u8, g: u8, b: u8) -> Rgba {
    Rgba::rgb(r, g, b)
}

const RED: Rgba = Rgba::rgb(255, 0, 0);
const GREEN: Rgba = Rgba::rgb(0, 128, 0);
const BLUE: Rgba = Rgba::rgb(0, 0, 255);

#[test]
fn specificity_order_and_importance() {
    let (doc, map) = render(
        "<p id=a class=c>x</p><p id=b class=c style='color: blue'>y</p>\
         <p id=d class=c style='color: blue !important'>z</p><p id=e>w</p>",
        "#a { color: red } .c { color: blue } p { color: green }\
         .c { color: green } #b { color: red }\
         #d { color: red !important }\
         p { color: green !important } #e { color: red }",
    );
    // An ID beats classes and types; `!important` beats everything
    // normal; `style` beats rules; `style !important` beats rule
    // `!important`.
    assert_eq!(get(&doc, &map, "a").color, GREEN);
    assert_eq!(get(&doc, &map, "b").color, GREEN);
    assert_eq!(get(&doc, &map, "d").color, BLUE);
    assert_eq!(get(&doc, &map, "e").color, GREEN);
    let (doc, map) = render(
        "<p id=a class='x y'>x</p>",
        ".x { color: red } .y { color: blue }",
    );
    assert_eq!(
        get(&doc, &map, "a").color,
        BLUE,
        "later rule wins at equal specificity"
    );
    let (doc, map) = render("<p id=a>x</p>", "p { color: red; color: blue }");
    assert_eq!(get(&doc, &map, "a").color, BLUE);
}

/// The HTML Standard rule for images with `sizes="auto"`: `contain:
/// size` is `!important`, the intrinsic size is not.
#[test]
fn auto_sized_images_have_size_containment() {
    let (doc, map) = render(
        "<img id=a sizes=auto><img id=b sizes='AUTO, 10px'><img id=c sizes=' auto'>\
         <img id=d sizes='auto 10px'><img id=e sizes=auto><img id=f>",
        "#e { contain: none; contain-intrinsic-size: 50px 25px }",
    );
    let cis = |id| {
        let s = get(&doc, &map, id);
        (
            s.contain,
            s.contain_intrinsic_width.size(),
            s.contain_intrinsic_height.size(),
        )
    };
    let size = Contain {
        size: true,
        ..Contain::NONE
    };
    assert_eq!(cis("a"), (size, 300.0, 150.0));
    assert_eq!(cis("b"), (size, 300.0, 150.0));
    // The attribute selector does not trim white space.
    assert_eq!(cis("c"), (Contain::NONE, 0.0, 0.0));
    assert_eq!(cis("d"), (Contain::NONE, 0.0, 0.0));
    // Author `contain` loses to the `!important`; the size does not.
    assert_eq!(cis("e"), (size, 50.0, 25.0));
    assert_eq!(cis("f"), (Contain::NONE, 0.0, 0.0));
}

#[test]
fn origins() {
    // Author rules beat the UA sheet regardless of specificity, and
    // UA !important beats author !important.
    let (doc, map) = render(
        "<a id=l href=x>link</a><input id=h type=hidden>",
        "* { color: green; text-decoration: none } input { display: block !important }",
    );
    let link = get(&doc, &map, "l");
    assert_eq!(link.color, GREEN);
    assert!(link.text_decoration_line.is_empty());
    assert!(
        map.get(node(&doc, "h"))
            .is_some_and(|s| s.display == Display::None)
    );
    // Presentational hints lose to any author rule.
    let (doc, map) = render(
        "<table id=t bgcolor=red width=50 align=right><tr><td id=c bgcolor=blue>x</td></tr></table>",
        "* { background-color: green } table { float: none }",
    );
    let t = get(&doc, &map, "t");
    assert_eq!(t.background_color, Color::Rgba(GREEN));
    assert_eq!(t.width, Size::LengthPercentage(LengthPercentage::Px(50.0)));
    assert_eq!(t.float, Float::None);
    assert_eq!(get(&doc, &map, "c").background_color, Color::Rgba(GREEN));
    // ...but beat the UA sheet.
    let (doc, map) = render("<p id=p align=center>x</p>", "");
    assert_eq!(get(&doc, &map, "p").text_align, TextAlign::WebkitCenter);
}

#[test]
fn inheritance_and_css_wide_keywords() {
    let (doc, map) = render(
        "<div id=p><span id=c>x</span><span id=i>y</span><span id=u>z</span><b id=n>q</b></div>",
        "#p { color: red; border: 2px solid; padding: 3px; font-size: 20px }\
         #i { border-top-width: inherit; border-top-style: solid; padding-left: inherit;\
         color: initial }\
         #u { color: unset; padding-top: unset; font-size: 50% }\
         #n { font-size: inherit; font-weight: inherit }",
    );
    let c = get(&doc, &map, "c");
    assert_eq!(c.color, RED);
    assert_eq!(c.border_top_width, 0.0);
    assert_eq!(c.padding_left, LengthPercentage::ZERO);
    let i = get(&doc, &map, "i");
    assert_eq!(i.border_top_width, 2.0);
    assert_eq!(i.padding_left, LengthPercentage::Px(3.0));
    assert_eq!(i.color, Rgba::BLACK);
    let u = get(&doc, &map, "u");
    assert_eq!(u.color, RED);
    assert_eq!(u.padding_top, LengthPercentage::ZERO);
    assert_eq!(u.font_size, 10.0);
    let n = get(&doc, &map, "n");
    assert_eq!(n.font_size, 20.0);
    assert_eq!(n.font_weight, 400.0, "inherit beats the UA `b` rule");
}

#[test]
fn relative_units() {
    let env = MediaEnvironment {
        viewport_width: 1000.0,
        viewport_height: 500.0,
        ..MediaEnvironment::default()
    };
    let (doc, map) = render_with(
        "<!DOCTYPE html><html id=h><div id=d><p id=p>x</p></div></html>",
        "html { font-size: 20px } #d { font-size: 2em; width: 10vw; height: 10vh }\
         #p { font-size: 1.5rem; margin-left: 2em; padding-left: 1rem; width: 50vmin; line-height: 1.5em }",
        &env,
        &ElementStates::default(),
    );
    let d = get(&doc, &map, "d");
    assert_eq!(d.font_size, 40.0);
    assert_eq!(d.width, Size::LengthPercentage(LengthPercentage::Px(100.0)));
    assert_eq!(d.height, Size::LengthPercentage(LengthPercentage::Px(50.0)));
    let p = get(&doc, &map, "p");
    assert_eq!(p.font_size, 30.0);
    assert_eq!(p.margin_left, LengthPercentageOrAuto::px(60.0));
    assert_eq!(p.padding_left, LengthPercentage::Px(20.0));
    assert_eq!(p.width, Size::LengthPercentage(LengthPercentage::Px(250.0)));
    assert_eq!(p.line_height, LineHeight::Px(45.0));
    // `rem` on the root refers to the initial font size.
    let (doc, map) = render(
        "<html id=h></html>",
        "html { font-size: 2rem; width: 1rem }",
    );
    let h = get(&doc, &map, "h");
    assert_eq!(h.font_size, 32.0);
    assert_eq!(h.width, Size::LengthPercentage(LengthPercentage::Px(32.0)));
}

#[test]
fn display_fixups() {
    let (doc, map) = render(
        "<html id=root><span id=f>a</span><span id=a>b</span>\
         <div id=flex><span id=item>c</span><span id=cont><em id=deep>d</em></span></div>\
         <div id=grid><span id=g>e</span></div><span id=none><b id=hidden>x</b></span></html>",
        "html { display: inline } #f { float: left } #a { position: absolute; float: right }\
         #flex { display: flex } #cont { display: contents } #grid { display: inline-grid }\
         #none { display: none } #item { display: table-cell }",
    );
    assert_eq!(get(&doc, &map, "root").display, Display::Block);
    assert_eq!(get(&doc, &map, "f").display, Display::Block);
    let a = get(&doc, &map, "a");
    assert_eq!(a.display, Display::Block);
    assert_eq!(a.original_display, Display::Inline);
    assert_eq!(a.float, Float::None);
    assert_eq!(get(&doc, &map, "item").display, Display::Block);
    assert_eq!(get(&doc, &map, "cont").display, Display::Contents);
    assert_eq!(
        get(&doc, &map, "deep").display,
        Display::Block,
        "flex item through contents"
    );
    assert_eq!(get(&doc, &map, "g").display, Display::Block);
    assert!(map.get(node(&doc, "hidden")).is_none());
    // Border widths are zero without a border style; the overflow axes
    // pair up.
    let (doc, map) = render(
        "<div id=d>x</div>",
        "#d { border-width: 5px; border-left-style: solid; outline-width: 4px; overflow-x: hidden }",
    );
    let d = get(&doc, &map, "d");
    assert_eq!(d.border_widths(), [0.0, 0.0, 0.0, 5.0]);
    assert_eq!(d.outline_width, 4.0);
    assert_eq!(d.overflow_y, Overflow::Auto);
    let (doc, map) = render(
        "<div id=n><span id=c>x</span><b id=t>y</b></div>",
        "#n { user-select: none } #t { user-select: text }",
    );
    assert_eq!(get(&doc, &map, "c").user_select, UserSelect::None);
    assert_eq!(get(&doc, &map, "t").user_select, UserSelect::Text);
}

#[test]
fn custom_properties_and_var() {
    let (doc, map) = render(
        "<div id=p><p id=c>x</p><p id=bad>y</p><p id=sh>z</p><p id=cyc>w</p></div>",
        ":root { --main: rgb(255 0 0); --gap: 4px; --w: 2px }\
         #p { --main: blue; color: var(--main); padding: var(--gap) calc(var(--gap) * 2) }\
         #c { background-color: var(--missing, var(--main)); margin: var(--gap, 1px) }\
         #bad { color: var(--missing); border-top-width: var(--nope); border-top-style: solid; width: var(--main) }\
         #sh { border: var(--w) solid var(--main); --main: green; font: var(--font, bold 10px serif) }\
         #cyc { --a: var(--b); --b: var(--a); color: var(--a, red); --x: env(nope, 7px); margin-top: var(--x) }",
    );
    let p = get(&doc, &map, "p");
    assert_eq!(p.color, BLUE);
    assert_eq!(p.padding_top, LengthPercentage::Px(4.0));
    assert_eq!(p.padding_right, LengthPercentage::Px(8.0));
    let c = get(&doc, &map, "c");
    assert_eq!(c.background_color, Color::Rgba(BLUE));
    assert_eq!(c.margin_top, LengthPercentageOrAuto::px(4.0));
    // Invalid at computed-value time: `unset` (inherit for `color`,
    // initial for the others).
    let bad = get(&doc, &map, "bad");
    assert_eq!(bad.color, BLUE);
    assert_eq!(bad.border_top_width, 3.0);
    assert_eq!(bad.width, Size::Auto);
    let sh = get(&doc, &map, "sh");
    assert_eq!(sh.border_left_width, 2.0);
    assert_eq!(sh.border_left_style, BorderStyle::Solid);
    assert_eq!(sh.border_left_color, Color::Rgba(GREEN));
    assert_eq!(sh.font_weight, 700.0);
    assert_eq!(sh.font_size, 10.0);
    let cyc = get(&doc, &map, "cyc");
    assert_eq!(cyc.color, RED);
    assert_eq!(cyc.margin_top, LengthPercentageOrAuto::px(7.0));
    let custom = cyc.custom_properties.as_ref().expect("custom properties");
    assert!(custom.get("--a").is_none());
    assert!(custom.get("--gap").is_some());
    // Children share the parent's map when they declare nothing.
    let parent = map.get(node(&doc, "p")).expect("style");
    let child = map.get(node(&doc, "c")).expect("style");
    assert!(Arc::ptr_eq(
        parent.custom_properties.as_ref().expect("map"),
        child.custom_properties.as_ref().expect("map")
    ));
}

#[test]
fn media_and_supports() {
    let css = "p { color: red } @media (min-width: 600px) { p { color: green } }\
               @media print { p { color: blue } }\
               @supports (display: flex) { p { background-color: green } }\
               @supports (display: nonsense) { p { background-color: red } }";
    let html = "<!DOCTYPE html><p id=p>x</p>";
    let wide = MediaEnvironment::default();
    let narrow = MediaEnvironment {
        viewport_width: 400.0,
        ..MediaEnvironment::default()
    };
    let (doc, map) = render_with(html, css, &wide, &ElementStates::default());
    assert_eq!(get(&doc, &map, "p").color, GREEN);
    assert_eq!(get(&doc, &map, "p").background_color, Color::Rgba(GREEN));
    let (doc, map) = render_with(html, css, &narrow, &ElementStates::default());
    assert_eq!(get(&doc, &map, "p").color, RED);
}

#[test]
fn pseudo_elements() {
    let (doc, map) = render(
        "<p id=a title=T>x</p><p id=b>y</p><p id=n>z</p><ul><li id=li>i</li></ul><q id=q>q</q>\
         <div id=flex><span id=fi>s</span></div><img id=img src=x.png>",
        "#a::before { content: '[' attr(title) ']'; color: red } #a::after { content: 'end' }\
         #b::before { color: red } #n::before { content: 'x'; display: none }\
         li::marker { color: blue } #flex { display: flex } #flex::before { content: 'f' }\
         img::before { content: 'no' }",
    );
    let a = node(&doc, "a");
    let before = map.pseudo(a, PseudoKind::Before).expect("::before");
    assert_eq!(content_text(before).as_deref(), Some("[T]"));
    assert_eq!(before.color, RED);
    assert_eq!(before.display, Display::Inline);
    assert!(map.pseudo(a, PseudoKind::After).is_some());
    assert!(map.pseudo(node(&doc, "b"), PseudoKind::Before).is_none());
    assert!(map.pseudo(node(&doc, "n"), PseudoKind::Before).is_none());
    let marker = map
        .pseudo(node(&doc, "li"), PseudoKind::Marker)
        .expect("::marker");
    assert_eq!(marker.color, BLUE);
    assert_eq!(marker.white_space, WhiteSpace::Pre);
    assert_eq!(content_text(marker), None);
    let q = node(&doc, "q");
    let open = map.pseudo(q, PseudoKind::Before).expect("q::before");
    assert_eq!(content_text(open).as_deref(), Some("\u{201C}"));
    let flex_before = map
        .pseudo(node(&doc, "flex"), PseudoKind::Before)
        .expect("::before");
    assert_eq!(flex_before.display, Display::Block, "blockified flex item");
    assert!(map.pseudo(node(&doc, "img"), PseudoKind::Before).is_none());
}

#[test]
fn placeholder_styles() {
    let (doc, map) = render(
        "<input id=a placeholder=x><textarea id=b placeholder=y></textarea><input id=c>",
        "#b::placeholder { color: red }",
    );
    let a = map
        .pseudo(node(&doc, "a"), PseudoKind::Placeholder)
        .expect("input::placeholder");
    assert_eq!(a.color, Rgba::rgb(0x75, 0x75, 0x75));
    assert_eq!(
        a.font_size,
        map.get(node(&doc, "a")).expect("style").font_size
    );
    let b = map
        .pseudo(node(&doc, "b"), PseudoKind::Placeholder)
        .expect("textarea::placeholder");
    assert_eq!(b.color, RED);
    // Chromium 148 (measured): an input's placeholder does not wrap; a
    // text area's inherits `white-space` from the text area.
    assert_eq!(a.white_space, WhiteSpace::Pre);
    assert_eq!(b.white_space, WhiteSpace::PreWrap);
    assert!(
        map.pseudo(node(&doc, "c"), PseudoKind::Placeholder)
            .is_none()
    );
}

#[test]
fn disabled_inputs() {
    // Chromium 148 (`swbtools measure ua-styles`): a disabled image input
    // has the default cursor; disabled image inputs and checkboxes keep
    // a transparent background; disabled text fields get a gray one.
    let (doc, map) = render(
        "<input id=i type=image disabled><input id=c type=checkbox disabled>\
         <input id=t disabled>",
        "",
    );
    let image = get(&doc, &map, "i");
    assert_eq!(image.cursor, Cursor::Default);
    assert_eq!(image.background_color, Color::TRANSPARENT);
    assert_eq!(get(&doc, &map, "c").background_color, Color::TRANSPARENT);
    assert_eq!(
        get(&doc, &map, "t").background_color,
        Color::Rgba(Rgba::new(239, 239, 239, 77))
    );
}

#[test]
fn details_pseudo_elements() {
    let (doc, map) = render(
        "<details id=closed><summary id=s>s</summary><p id=p>x</p></details>\
         <details id=open open><summary>s</summary></details>\
         <details id=nosum><p>x</p></details>",
        "",
    );
    let content = |id: &str| map.pseudo(node(&doc, id), PseudoKind::DetailsContent);
    assert!(
        content("closed")
            .expect("::details-content")
            .contents_hidden
    );
    assert!(!content("open").expect("::details-content").contents_hidden);
    assert_eq!(content("closed").map(|s| s.display), Some(Display::Block));
    assert!(map.has_hidden_contents());
    // The children keep their styles (they are laid out).
    assert_eq!(get(&doc, &map, "p").display, Display::Block);
    // Only a `details` without a summary has the default summary and
    // its marker.
    let summary = |id: &str| map.pseudo(node(&doc, id), PseudoKind::DetailsSummary);
    assert!(summary("closed").is_none() && summary("open").is_none());
    let default = summary("nosum").expect("default summary");
    assert_eq!(default.display, Display::ListItem);
    assert_eq!(default.list_style_type, ListStyleType::DisclosureClosed);
    assert_eq!(default.list_style_position, ListStylePosition::Inside);
    assert_eq!(
        map.list_marker_text(node(&doc, "nosum"), default),
        Some("\u{25B8} ".to_owned())
    );
    let (doc, map) = render("<details id=d open><p>x</p></details>", "");
    assert_eq!(
        map.pseudo(node(&doc, "d"), PseudoKind::DetailsSummary)
            .map(|s| s.list_style_type),
        Some(ListStyleType::DisclosureOpen)
    );
    assert!(!map.has_hidden_contents());
}

#[test]
fn user_agent_defaults() {
    let (doc, map) = render(
        "<body id=body><h1 id=h1>t</h1><p id=p>x</p><ul id=ul><li><ul id=ul2></ul></li></ul>\
         <ol id=ol></ol><table id=t><tr id=tr><th id=th>h</th><td id=td>d</td></tr></table>\
         <pre id=pre>x</pre><a id=a href=x>l</a><b id=b>b</b><small id=small>s</small>\
         <center id=center>c</center><input id=input><button id=button>b</button>\
         <details id=det><summary id=sum>s</summary><p id=hidden>h</p></details>\
         <hr id=hr><sub id=sub>s</sub><script id=script></script><noscript id=ns>n</noscript></body>",
        "",
    );
    assert_eq!(
        get(&doc, &map, "body").margin_top,
        LengthPercentageOrAuto::px(8.0)
    );
    let h1 = get(&doc, &map, "h1");
    assert_eq!((h1.font_size, h1.font_weight), (32.0, 700.0));
    assert_eq!(h1.margin_top, LengthPercentageOrAuto::px(32.0 * 0.67));
    assert_eq!(
        get(&doc, &map, "p").margin_bottom,
        LengthPercentageOrAuto::px(16.0)
    );
    let ul = get(&doc, &map, "ul");
    assert_eq!(ul.list_style_type, ListStyleType::Disc);
    assert_eq!(ul.padding_left, LengthPercentage::Px(40.0));
    let ul2 = get(&doc, &map, "ul2");
    assert_eq!(ul2.list_style_type, ListStyleType::Circle);
    assert_eq!(ul2.margin_top, LengthPercentageOrAuto::px(0.0));
    assert_eq!(
        get(&doc, &map, "ol").list_style_type,
        ListStyleType::Decimal
    );
    let t = get(&doc, &map, "t");
    assert_eq!(t.display, Display::Table);
    assert_eq!(t.border_spacing_horizontal, 2.0);
    assert_eq!(t.box_sizing, BoxSizing::BorderBox);
    assert_eq!(
        get(&doc, &map, "tr").vertical_align,
        VerticalAlign::Keyword(VerticalAlignKeyword::Middle)
    );
    let th = get(&doc, &map, "th");
    assert_eq!(th.text_align, TextAlign::Center);
    assert_eq!(th.font_weight, 700.0);
    assert_eq!(
        th.vertical_align,
        VerticalAlign::Keyword(VerticalAlignKeyword::Middle)
    );
    assert_eq!(get(&doc, &map, "td").padding_top, LengthPercentage::Px(1.0));
    let pre = get(&doc, &map, "pre");
    assert_eq!(pre.white_space, WhiteSpace::Pre);
    assert_eq!(pre.font_size, 13.0);
    let a = get(&doc, &map, "a");
    assert_eq!(a.color, rgb(0, 0, 0xEE));
    assert_eq!(a.text_decoration_line, TextDecorationLine::UNDERLINE);
    assert_eq!(a.cursor, Cursor::Pointer);
    assert_eq!(get(&doc, &map, "b").font_weight, 700.0);
    assert!((get(&doc, &map, "small").font_size - 16.0 / 1.2).abs() < 1e-4);
    assert_eq!(
        get(&doc, &map, "center").text_align,
        TextAlign::WebkitCenter
    );
    let input = get(&doc, &map, "input");
    assert_eq!(input.display, Display::InlineBlock);
    assert_eq!(input.border_top_width, 2.0);
    assert_eq!(input.border_top_style, BorderStyle::Inset);
    assert!((input.font_size - 13.333_333).abs() < 1e-4);
    assert_eq!(
        get(&doc, &map, "button").border_top_style,
        BorderStyle::Outset
    );
    assert_eq!(get(&doc, &map, "sum").display, Display::ListItem);
    assert_eq!(
        get(&doc, &map, "sum").list_style_type,
        ListStyleType::DisclosureClosed
    );
    // The children of `details` are laid out; the content box hides them.
    assert_eq!(get(&doc, &map, "hidden").display, Display::Block);
    let hr = get(&doc, &map, "hr");
    assert_eq!(hr.border_widths(), [1.0; 4]);
    assert_eq!(hr.margin_left, LengthPercentageOrAuto::Auto);
    assert_eq!(
        get(&doc, &map, "sub").vertical_align,
        VerticalAlign::Keyword(VerticalAlignKeyword::Sub)
    );
    assert_eq!(get(&doc, &map, "script").display, Display::None);
    assert_eq!(get(&doc, &map, "ns").display, Display::Inline);
}

/// `width` and `height` of `video` map to `width`, `height` and, if
/// both are lengths, to `aspect-ratio: auto w / h` (Chromium 148:
/// computed `auto 250 / 159`).
#[test]
fn video_dimension_hints() {
    let (doc, map) = render(
        "<video id=v width=250 height=159></video>\
         <video id=p width=50% height=100></video>\
         <video id=w width=250></video>\
         <video id=z width=0 height=0></video>\
         <img id=img width=250 height=159>",
        "",
    );
    let v = get(&doc, &map, "v");
    assert_eq!(v.width, Size::LengthPercentage(LengthPercentage::Px(250.0)));
    assert_eq!(
        v.height,
        Size::LengthPercentage(LengthPercentage::Px(159.0))
    );
    assert_eq!(
        v.aspect_ratio,
        AspectRatio {
            auto: true,
            ratio: Some(250.0 / 159.0)
        }
    );
    assert_eq!(get(&doc, &map, "p").aspect_ratio, AspectRatio::AUTO);
    assert_eq!(get(&doc, &map, "w").aspect_ratio, AspectRatio::AUTO);
    assert_eq!(
        get(&doc, &map, "z").aspect_ratio,
        AspectRatio {
            auto: true,
            ratio: None
        }
    );
    // An `img` has the hint too (a broken or contained image has no
    // natural ratio).
    assert_eq!(
        get(&doc, &map, "img").aspect_ratio,
        AspectRatio {
            auto: true,
            ratio: Some(250.0 / 159.0)
        }
    );
}

#[test]
fn presentational_hints() {
    let (doc, map) = render(
        "<body id=body text=red bgcolor=#ffffee link=green>\
         <a id=a href=x>l</a>\
         <table id=t border=2 cellpadding=5 cellspacing=3 width=80% height=20>\
         <tbody><tr id=tr valign=top align=center height=30><td id=td nowrap width=10 background=bg.png>x</td>\
         <th id=th align=left bgcolor=blue>y</th></tr></tbody></table>\
         <table id=t0 border=0><tr><td id=td0>z</td></tr></table>\
         <font id=f color=blue face='Courier New, monospace' size=+2>f</font>\
         <font id=f1 size=1>g</font>\
         <img id=img width=50 height=40% hspace=4 vspace=2 border=1 align=left>\
         <hr id=hr size=1 width=50%><hr id=hr2 noshade size=4>\
         <br id=br clear=all><ol id=ol type=a></ol><ul id=ul type=square></ul>\
         <div id=div align=right>d</div><pre id=pre wrap>p</pre>\
         <iframe id=ifr frameborder=0></iframe></body>",
        "",
    );
    let body = get(&doc, &map, "body");
    assert_eq!(body.color, RED);
    assert_eq!(body.background_color, Color::Rgba(rgb(0xff, 0xff, 0xee)));
    assert_eq!(get(&doc, &map, "a").color, GREEN);
    let t = get(&doc, &map, "t");
    assert_eq!(t.border_widths(), [2.0; 4]);
    assert_eq!(t.border_top_style, BorderStyle::Outset);
    assert_eq!(t.border_spacing_vertical, 3.0);
    assert_eq!(
        t.width,
        Size::LengthPercentage(LengthPercentage::Percent(0.8))
    );
    assert_eq!(t.height, Size::LengthPercentage(LengthPercentage::Px(20.0)));
    let tr = get(&doc, &map, "tr");
    assert_eq!(
        tr.vertical_align,
        VerticalAlign::Keyword(VerticalAlignKeyword::Top)
    );
    assert_eq!(tr.text_align, TextAlign::WebkitCenter);
    assert_eq!(
        tr.height,
        Size::LengthPercentage(LengthPercentage::Px(30.0))
    );
    let td = get(&doc, &map, "td");
    assert_eq!(td.padding_left, LengthPercentage::Px(5.0));
    assert_eq!(td.border_widths(), [1.0; 4]);
    assert_eq!(td.border_top_style, BorderStyle::Inset);
    assert_eq!(td.white_space, WhiteSpace::Nowrap);
    assert_eq!(td.width, Size::LengthPercentage(LengthPercentage::Px(10.0)));
    assert_eq!(
        td.vertical_align,
        VerticalAlign::Keyword(VerticalAlignKeyword::Top)
    );
    assert_eq!(
        td.text_align,
        TextAlign::WebkitCenter,
        "inherited from the row"
    );
    assert_eq!(
        &*td.background_image,
        &[Some(Image::Url("https://example.com/dir/bg.png".into()))]
    );
    let th = get(&doc, &map, "th");
    assert_eq!(th.text_align, TextAlign::WebkitLeft);
    assert_eq!(th.background_color, Color::Rgba(BLUE));
    let td0 = get(&doc, &map, "td0");
    assert_eq!(td0.border_widths(), [0.0; 4]);
    assert_eq!(get(&doc, &map, "t0").border_widths(), [0.0; 4]);
    let f = get(&doc, &map, "f");
    assert_eq!(f.color, BLUE);
    assert_eq!(f.font_family[0], FontFamily::Named("Courier New".into()));
    assert_eq!(f.font_size, 24.0);
    assert_eq!(get(&doc, &map, "f1").font_size, 10.0);
    let img = get(&doc, &map, "img");
    assert_eq!(
        img.width,
        Size::LengthPercentage(LengthPercentage::Px(50.0))
    );
    assert_eq!(
        img.height,
        Size::LengthPercentage(LengthPercentage::Percent(0.4))
    );
    assert_eq!(img.margin_left, LengthPercentageOrAuto::px(4.0));
    assert_eq!(img.margin_top, LengthPercentageOrAuto::px(2.0));
    assert_eq!(img.border_top_style, BorderStyle::Solid);
    assert_eq!(img.float, Float::Left);
    let hr = get(&doc, &map, "hr");
    assert_eq!(hr.border_bottom_width, 0.0);
    assert_eq!(
        hr.width,
        Size::LengthPercentage(LengthPercentage::Percent(0.5))
    );
    let hr2 = get(&doc, &map, "hr2");
    assert_eq!(hr2.border_top_style, BorderStyle::Solid);
    assert_eq!(hr2.border_top_width, 2.0);
    assert_eq!(get(&doc, &map, "br").clear, Clear::Both);
    assert_eq!(
        get(&doc, &map, "ol").list_style_type,
        ListStyleType::LowerAlpha
    );
    assert_eq!(get(&doc, &map, "ul").list_style_type, ListStyleType::Square);
    assert_eq!(get(&doc, &map, "div").text_align, TextAlign::WebkitRight);
    assert_eq!(get(&doc, &map, "pre").white_space, WhiteSpace::PreWrap);
    assert_eq!(get(&doc, &map, "ifr").border_widths(), [0.0; 4]);
}

/// `white-space` is the shorthand of `white-space-collapse` and
/// `text-wrap-mode`; each longhand wins on its own (measured in
/// Chromium 148, `css-videojs-2a`: the cases `ws-*`).
#[test]
fn white_space_longhands_cascade_apart() {
    let (doc, map) = render(
        "<!DOCTYPE html><p id=a style='text-wrap:nowrap;white-space:normal'>x</p>\
         <p id=b style='white-space:normal;text-wrap:nowrap'>x</p>\
         <p id=c class=c>x</p>\
         <p id=d style='white-space:pre-line;text-wrap:nowrap'>x</p>\
         <div style='white-space:pre-line'><p id=e style='text-wrap-mode:nowrap'>x</p></div>\
         <div style='text-wrap:nowrap'><p id=f style='white-space:pre'>x</p></div>\
         <p id=g style='white-space-collapse:break-spaces;text-wrap-mode:nowrap'>x</p>",
        "p.c{text-wrap:nowrap} .c{white-space:normal}",
    );
    assert_eq!(get(&doc, &map, "a").white_space, WhiteSpace::Normal);
    assert_eq!(get(&doc, &map, "b").white_space, WhiteSpace::Nowrap);
    // Chromium: the more specific `.c` sets `white-space`; `p.c` sets only the mode.
    assert_eq!(get(&doc, &map, "c").white_space, WhiteSpace::Nowrap);
    assert_eq!(get(&doc, &map, "d").white_space, WhiteSpace::PreLineNowrap);
    assert_eq!(get(&doc, &map, "e").white_space, WhiteSpace::PreLineNowrap);
    assert_eq!(get(&doc, &map, "f").white_space, WhiteSpace::Pre);
    assert_eq!(
        get(&doc, &map, "g").white_space,
        WhiteSpace::BreakSpacesNowrap
    );
}

#[test]
fn tables_reset_webkit_text_align() {
    let (doc, map) = render(
        "<!DOCTYPE html><center><table id=t><tr><td id=td>x</td></tr></table>\
         <div id=d style='display:table'>y</div></center>",
        "",
    );
    assert_eq!(get(&doc, &map, "t").text_align, TextAlign::Start);
    assert_eq!(get(&doc, &map, "td").text_align, TextAlign::Start);
    // Only table elements, as in Chromium.
    assert_eq!(get(&doc, &map, "d").text_align, TextAlign::WebkitCenter);
}

#[test]
fn quirks_mode() {
    let quirks_html = "<html><body style='font-size: 20px; color: red; text-align: center'>\
         <table id=t><tr><td id=td>x</td></tr></table>\
         <div id=d class=Foo>y</div><ul><li id=li>z</li></ul><div><li id=li2>w</li></div>\
         </body></html>";
    let css = "#d { width: 100; height: 50px } .foo { color: green } #td { padding: 3 }";
    let (doc, map) = render_with(
        quirks_html,
        css,
        &MediaEnvironment::default(),
        &ElementStates::default(),
    );
    assert_eq!(doc.quirks_mode, swb_dom::QuirksMode::Quirks);
    let t = get(&doc, &map, "t");
    assert_eq!(t.font_size, 16.0);
    assert_eq!(t.color, RED, "Chromium: tables inherit the color");
    assert_eq!(t.text_align, TextAlign::Start);
    let d = get(&doc, &map, "d");
    assert_eq!(d.width, Size::LengthPercentage(LengthPercentage::Px(100.0)));
    assert_eq!(d.color, GREEN, "class selectors match case-insensitively");
    assert_eq!(get(&doc, &map, "td").padding_top, LengthPercentage::Px(3.0));
    // `li` is `inside`, except in lists (Chromium's quirks.css).
    assert_eq!(
        get(&doc, &map, "li").list_style_position,
        ListStylePosition::Outside
    );
    assert_eq!(
        get(&doc, &map, "li2").list_style_position,
        ListStylePosition::Inside
    );
    // The same document in standards mode.
    let html = format!("<!DOCTYPE html>{quirks_html}");
    let (doc, map) = render_with(
        &html,
        css,
        &MediaEnvironment::default(),
        &ElementStates::default(),
    );
    assert_eq!(get(&doc, &map, "t").font_size, 20.0);
    let d = get(&doc, &map, "d");
    assert_eq!(d.width, Size::Auto);
    assert_eq!(d.color, RED);
    assert_eq!(
        get(&doc, &map, "li2").list_style_position,
        ListStylePosition::Outside
    );
}

#[test]
fn monospace_font_size_quirk() {
    let (doc, map) = render(
        "<div id=body><pre id=pre>p<code id=nested>c</code><span id=sans>s</span></pre>\
         <code id=code>c</code><code id=fixed>f</code><pre id=pct>p</pre>\
         <code id=two>t</code><code id=small_c><small id=small>s</small></code>\
         <span id=big style='font-size: x-large'><code id=xl>x</code></span>\
         <span id=px style='font-size: 20px'><code id=px_code>y</code></span></div>",
        "#fixed { font-size: 12px } #pct { font-size: 120% } #two { font-family: monospace, monospace }\
         #sans { font-family: sans-serif }",
    );
    let size = |id: &str| get(&doc, &map, id).font_size;
    assert_eq!(size("body"), 16.0);
    assert_eq!(size("pre"), 13.0);
    assert_eq!(size("nested"), 13.0);
    assert_eq!(size("sans"), 16.0);
    assert_eq!(size("code"), 13.0);
    assert_eq!(size("fixed"), 12.0);
    assert!((size("pct") - 15.6).abs() < 1e-4, "{}", size("pct"));
    assert_eq!(size("two"), 16.0);
    assert!((size("small") - 13.0 / 1.2).abs() < 1e-4);
    assert_eq!(size("xl"), 20.0, "the monospace x-large size");
    assert_eq!(size("px_code"), 20.0, "absolute sizes do not change");
}

#[test]
fn dynamic_states() {
    let html = "<!DOCTYPE html><div id=outer><a id=link href=x>l</a></div><p id=other>o</p>";
    let css = "div:hover { color: red } a:hover { color: green } :focus { outline: 1px solid } p:hover { color: blue }";
    let doc = parse_html(html);
    let states = ElementStates {
        hover: doc.element_by_id("link"),
        focus: doc.element_by_id("link"),
        ..ElementStates::default()
    };
    let (doc, map) = render_with(html, css, &MediaEnvironment::default(), &states);
    assert_eq!(get(&doc, &map, "outer").color, RED);
    assert_eq!(get(&doc, &map, "link").color, GREEN);
    assert_eq!(get(&doc, &map, "link").outline_width, 1.0);
    assert_eq!(get(&doc, &map, "other").color, Rgba::BLACK);
    // Only the states that some selector uses can change styles.
    let mut stylist = Stylist::new(doc.quirks_mode);
    stylist.add_author_sheet(&swb_css::parse_stylesheet(css), &base());
    let deps = stylist.state_dependencies();
    assert!(deps.contains(swb_css::ElementState::HOVER | swb_css::ElementState::FOCUS));
    assert!(!deps.contains(swb_css::ElementState::TARGET));
}

/// `:defined` follows the custom element state (ADR 0035 section 1):
/// an HTML element with a custom element name is not defined before its
/// upgrade, also with JavaScript off (probe case `defined-js-off`,
/// Chromium 148); other names, reserved names and other namespaces are.
#[test]
fn defined_follows_the_custom_element_state() {
    let html = "<x-a id=a>a</x-a><x-u id=u>u</x-u><foo id=f>f</foo><div id=d>d</div><font-face id=ff>ff</font-face><svg><x-a id=s></x-a></svg><annotation-xml id=ax>ax</annotation-xml>";
    let css = ":not(:defined) { color: rgb(255, 0, 0) } :defined { color: rgb(0, 128, 0) }";
    let (doc, map) = render(html, css);
    for id in ["a", "u"] {
        assert_eq!(get(&doc, &map, id).color, RED, "#{id}");
    }
    for id in ["f", "d", "ff", "s", "ax"] {
        assert_eq!(get(&doc, &map, id).color, rgb(0, 128, 0), "#{id}");
    }
}

#[test]
fn same_styles_compares_values() {
    let html = "<p id=p>x</p><ul><li>i</li></ul>";
    let (_, a) = render(html, "p { color: red }");
    let (_, b) = render(html, "p { color: red }");
    let (_, c) = render(html, "p { color: blue }");
    let (_, d) = render(html, "p { color: red } li::marker { color: blue }");
    assert!(a.same_styles(&b));
    assert!(!a.same_styles(&c));
    assert!(!a.same_styles(&d), "pseudo-element styles count");
}

#[test]
fn image_urls_are_absolute_and_unique() {
    let (_, map) = render(
        "<div class=a></div><div class=a></div><ul class=b><li>x</li></ul><p class=c></p>",
        ".a { background: url(img/a.png) } .b { list-style-image: url('/b.png') }\
         .c { background-image: url(img/a.png), linear-gradient(red, blue) }",
    );
    let urls: Vec<String> = map.image_urls().iter().map(ToString::to_string).collect();
    assert_eq!(
        urls,
        vec![
            "https://example.com/dir/img/a.png".to_owned(),
            "https://example.com/b.png".to_owned()
        ]
    );
}

#[test]
fn revert_rolls_back_to_the_user_agent_origin() {
    let (doc, map) = render(
        "<div id=d>x</div><p id=p>y</p><h1 id=h style='font-size: revert'>z</h1>\
         <span id=s style='color: revert'>s</span><a id=a href=x>l</a>",
        "* { all: unset } div { display: revert } p { all: revert }\
         h1 { font-size: 10px !important } span { color: red } #s { color: blue }\
         a { color: revert-layer; text-decoration: revert }",
    );
    assert_eq!(get(&doc, &map, "d").display, Display::Block);
    let p = get(&doc, &map, "p");
    assert_eq!(p.display, Display::Block);
    assert_eq!(p.margin_top, LengthPercentageOrAuto::px(16.0));
    // An `!important` author declaration still wins over a normal
    // `revert` in the style attribute.
    assert_eq!(get(&doc, &map, "h").font_size, 10.0);
    // `revert` in the style attribute skips all author rules: the
    // span has no UA color, so it inherits.
    assert_eq!(get(&doc, &map, "s").color, Rgba::BLACK);
    let a = get(&doc, &map, "a");
    assert_eq!(a.color, rgb(0, 0, 0xEE));
    assert_eq!(a.text_decoration_line, TextDecorationLine::UNDERLINE);
}

#[test]
fn replaced_elements_and_edge_cases() {
    let (doc, map) = render(
        "<img id=img src=x style='display: contents'><div id=div style='display: contents'></div>\
         <canvas id=canvas width=300 height=150></canvas><img id=mid align=middle>\
         <table id=t border=1><tr><td>x</td></tr></table><p id=q>q</p>",
        "#q { --x: 1; -swb-border-spacing-horizontal: 9px; content: no-open-quote }",
    );
    assert_eq!(get(&doc, &map, "img").display, Display::None);
    assert_eq!(get(&doc, &map, "div").display, Display::Contents);
    assert_eq!(get(&doc, &map, "canvas").width, Size::Auto);
    assert_eq!(
        get(&doc, &map, "mid").vertical_align,
        VerticalAlign::Keyword(VerticalAlignKeyword::Middle)
    );
    assert_eq!(get(&doc, &map, "t").border_top_color, Color::CurrentColor);
    let q = get(&doc, &map, "q");
    assert_eq!(
        q.border_spacing_horizontal, 0.0,
        "internal names are not valid"
    );
    assert_eq!(q.content, Content::Items(Arc::from([])));
}

#[test]
fn deep_nesting_through_custom_properties() {
    // Each element wraps the value of its parent in 60 more levels of
    // parentheses. The values must stop growing (and not overflow the
    // stack in recursive code).
    let html = "<div class=e><div class=o>".repeat(1500);
    let css = format!(
        ":root {{ --b: x }} .e {{ --a: {0}var(--b){1} }} .o {{ --b: {0}var(--a){1} }}\
         div {{ width: var(--a, 1px) }}",
        "(".repeat(60),
        ")".repeat(60)
    );
    let (doc, map) = render(&html, &css);
    let styled = doc
        .descendants(NodeId::DOCUMENT)
        .filter(|&n| map.get(n).is_some())
        .count();
    assert!(styled > 3000);
}

#[test]
fn many_custom_properties() {
    let mut css = String::from(":root {");
    for i in 0..20_000 {
        let _ = write!(css, "--v{i}: {i}px;");
    }
    css.push_str("} p { width: var(--v19999) }");
    let (doc, map) = render("<p id=p>x</p>", &css);
    assert_eq!(
        get(&doc, &map, "p").width,
        Size::LengthPercentage(LengthPercentage::Px(19999.0))
    );
}

#[test]
fn styles_are_shared_when_inputs_match() {
    let (doc, map) = render(
        "<p><a id=a href=x>1</a><a id=b href=y>2</a><a id=c href=z style='color: red'>3</a>\
         <span id=d title=x>4</span><span id=e title=y>5</span></p>",
        "span::before { content: attr(title) } p span { content: attr(title) }",
    );
    let style = |id: &str| map.get(node(&doc, id)).expect("style");
    assert!(Arc::ptr_eq(style("a"), style("b")));
    assert!(!Arc::ptr_eq(style("a"), style("c")));
    // Rules with `content` (which can use `attr()`) prevent sharing.
    assert!(!Arc::ptr_eq(style("d"), style("e")));
    let before = |id: &str| {
        map.pseudo(node(&doc, id), PseudoKind::Before)
            .expect("::before")
    };
    assert_eq!(content_text(before("d")).as_deref(), Some("x"));
    assert_eq!(content_text(before("e")).as_deref(), Some("y"));
}

/// Loads a fixture page with its stylesheets (by file name).
fn fixture(name: &str, html: &str, sheets: &[&str]) -> (Document, Stylist) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pages")
        .join(name)
        .join("files");
    let bytes = std::fs::read(dir.join(html)).expect("fixture HTML");
    let (doc, _) = swb_dom::parse_html_bytes(&bytes, Some("utf-8"));
    let mut stylist = Stylist::new(doc.quirks_mode);
    for sheet in sheets {
        let css = std::fs::read_to_string(dir.join(sheet)).expect("fixture CSS");
        stylist.add_author_sheet(&swb_css::parse_stylesheet(&css), &base());
    }
    for node in doc.descendants(NodeId::DOCUMENT) {
        if doc.is_html_element(node, &swb_dom::local_name!("style")) {
            let css = doc.text_content(node);
            stylist.add_author_sheet(&swb_css::parse_stylesheet(&css), &base());
        }
    }
    (doc, stylist)
}

#[test]
fn bloom_filter_does_not_change_results() {
    let pages: [(&str, &str, &[&str]); 2] = [
        (
            "wikipedia-web-browser",
            "e21b21467da0d04c.html",
            &["54eaa11ea56e20f7.css", "3f439934c51c220c.css"],
        ),
        (
            "hacker-news",
            "8e8eec10c3f22bf2.html",
            &["de4722818cb6e785.css"],
        ),
    ];
    for (name, html, sheets) in pages {
        let (doc, stylist) = fixture(name, html, sheets);
        let env = MediaEnvironment::default();
        let states = ElementStates::default();
        let with = compute_styles_with_options(
            &doc,
            &stylist,
            &env,
            &states,
            &base(),
            true,
            &ContainerSizes::new(),
            None,
        );
        let without = compute_styles_with_options(
            &doc,
            &stylist,
            &env,
            &states,
            &base(),
            false,
            &ContainerSizes::new(),
            None,
        );
        let mut compared = 0;
        for node in doc.descendants(NodeId::DOCUMENT) {
            assert_eq!(with.get(node), without.get(node), "{name}: {node:?}");
            for kind in [
                PseudoKind::Before,
                PseudoKind::After,
                PseudoKind::Marker,
                PseudoKind::Placeholder,
            ] {
                assert_eq!(with.pseudo(node, kind), without.pseudo(node, kind));
            }
            compared += usize::from(with.get(node).is_some());
        }
        assert!(compared > 100, "{name}: {compared}");
    }
}

#[test]
fn hostile_input_does_not_panic() {
    // Deep nesting is handled iteratively.
    let deep = "<div>".repeat(5000);
    let (doc, map) = render(&deep, "div div { margin: 1px } div { --x: var(--x) }");
    let deepest = doc
        .descendants(NodeId::DOCUMENT)
        .filter(|&n| doc.element(n).is_some())
        .last()
        .expect("an element");
    assert!(map.get(deepest).is_some());
    let mut css = String::from(":root { --v0: x x x x x x x x }");
    for i in 1..30 {
        let _ = write!(css, ":root {{ --v{i}: var(--v{0}) var(--v{0}) }}", i - 1);
    }
    css.push_str("p { width: var(--v29) }");
    let (doc, map) = render("<p id=p>x</p>", &css);
    assert_eq!(get(&doc, &map, "p").width, Size::Auto);
}

/// The color of `#p` (`r`ed, `g`reen, `b`lue or blac`k`) with the
/// sheets `css`, in document order, and the media environment `env`.
fn layer_color(css: &[&str], body: &str, env: &MediaEnvironment) -> char {
    let doc = parse_html(&format!("<!DOCTYPE html>{body}"));
    let mut stylist = Stylist::new(doc.quirks_mode);
    for sheet in css {
        stylist.add_author_sheet(&swb_css::parse_stylesheet(sheet), &base());
    }
    let map = compute_styles(&doc, &stylist, env, &ElementStates::default(), &base());
    match get(&doc, &map, "p").color {
        c if c == RED => 'r',
        c if c == GREEN => 'g',
        c if c == BLUE => 'b',
        c if c == Rgba::BLACK => 'k',
        other => panic!("unexpected color {other:?}"),
    }
}

/// Cases of `tools/probes/cascade-layers.json` with Chromium 148's
/// results (`engine/tests/cascade_layers.rs` runs all of them): the
/// layer order.
#[test]
fn layers_order_normal_declarations() {
    let p = "<p id=p>x</p>";
    let ord = "@layer b{p{color:red}} @layer a{p{color:blue}}";
    let rows: &[(&str, String, char)] = &[
        ("no statement", ord.to_owned(), 'b'),
        ("statement first", format!("@layer a, b;{ord}"), 'r'),
        ("statement last", format!("{ord}@layer a, b;"), 'b'),
        ("empty block counts", format!("@layer a{{}}{ord}"), 'r'),
        (
            "same layer, later wins",
            "@layer a{p{color:red}} @layer b{p{color:green}} @layer a{p{color:blue}}".into(),
            'g',
        ),
        (
            "each anonymous layer is its own",
            "@layer{p{color:red}} @layer{p{color:blue}} @layer x{p{color:green}}".into(),
            'g',
        ),
        (
            "unlayered beats layered, whatever the specificity",
            "@layer a{#p{color:blue}} p{color:red}".into(),
            'r',
        ),
        (
            "unlayered first still wins",
            "p{color:red} @layer a{p{color:blue}}".into(),
            'r',
        ),
        (
            "dotted and nested are one layer",
            "@layer a.b{p{color:red}} @layer a{@layer b{p{color:blue}}}".into(),
            'b',
        ),
        (
            "a parent's own rules come after its sublayers",
            "@layer a{p{color:blue} @layer b{#p{color:red}}}".into(),
            'b',
        ),
        (
            "a late sublayer stays inside its parent",
            "@layer a{@layer x{p{color:red}}} @layer b{p{color:green}} @layer a.y{p{color:blue}}"
                .into(),
            'g',
        ),
        (
            "layer names are case-sensitive",
            "@layer A{p{color:red}} @layer a{p{color:blue}} @layer A{p{color:green}}".into(),
            'b',
        ),
        (
            "an @import with layer() declares its layer",
            format!("@import url(missing.css) layer(a);{ord}"),
            'r',
        ),
        (
            "an @import with a false media does not",
            format!("@import url(missing.css) layer(a) (min-width: 5000px);{ord}"),
            'b',
        ),
        (
            "an @import with a false supports() does not",
            format!("@import url(missing.css) layer(a) supports(foo: bar);{ord}"),
            'b',
        ),
    ];
    for (name, css, want) in rows {
        let got = layer_color(&[css], p, &MediaEnvironment::default());
        assert_eq!(got, *want, "{name}");
    }
}

/// The layers of one document across sheets, and the declarations in
/// conditional rules that do not apply.
#[test]
fn layers_order_across_sheets_and_conditions() {
    let p = "<p id=p>x</p>";
    let env = MediaEnvironment::default();
    let ord = "@layer b{p{color:red}} @layer a{p{color:blue}}";
    let sheets: &[(&str, &[&str], char)] = &[
        (
            "b then a",
            &["@layer b{p{color:red}}", "@layer a{p{color:blue}}"],
            'b',
        ),
        (
            "one name in two sheets",
            &[
                "@layer a{p{color:red}}",
                "@layer b{p{color:green}} @layer a{p{color:blue}}",
            ],
            'g',
        ),
        ("statement in the first", &["@layer a, b;", ord], 'r'),
        (
            "statement in the second",
            &[
                "@layer b{p{color:red}}",
                "@layer a, b; @layer a{p{color:blue}}",
            ],
            'b',
        ),
        (
            "unlayered in a later sheet",
            &["@layer a{p{color:red}}", "p{color:blue}"],
            'b',
        ),
        (
            "unlayered in an earlier sheet",
            &["p{color:blue}", "@layer a{p{color:red}}"],
            'b',
        ),
    ];
    for (name, css, want) in sheets {
        assert_eq!(layer_color(css, p, &env), *want, "{name}");
    }
    // A declaration inside an `@media` that does not match, or a false
    // `@supports`, does not count for the order.
    for (name, prefix, want) in [
        (
            "media false statement",
            "@media (min-width:5000px){@layer a,b;}",
            'b',
        ),
        (
            "media false block",
            "@media (min-width:5000px){@layer a{p{color:green}}}",
            'b',
        ),
        ("media true statement", "@media all{@layer a,b;}", 'r'),
        (
            "supports false statement",
            "@supports (foo:bar){@layer a,b;}",
            'b',
        ),
        (
            "supports true statement",
            "@supports (display:block){@layer a,b;}",
            'r',
        ),
        ("media print statement", "@media print{@layer a,b;}", 'b'),
    ] {
        assert_eq!(
            layer_color(&[&format!("{prefix}{ord}")], p, &env),
            want,
            "{name}"
        );
    }
    // The order follows the viewport: the same sheet, two environments.
    let css = format!("@media (min-width:500px){{@layer a,b;}}{ord}");
    let wide = MediaEnvironment {
        viewport_width: 800.0,
        ..MediaEnvironment::default()
    };
    let narrow = MediaEnvironment {
        viewport_width: 300.0,
        ..MediaEnvironment::default()
    };
    assert_eq!(layer_color(&[&css], p, &wide), 'r');
    assert_eq!(layer_color(&[&css], p, &narrow), 'b');
}

/// `!important` reverses the order of the layers; the unlayered rules
/// are then the weakest, and the `style` attribute stays on top.
#[test]
fn layers_important_reverses() {
    let p = "<p id=p>x</p>";
    let env = MediaEnvironment::default();
    let attr = |style: &str| format!("<p id=p style='{style}'>x</p>");
    let rows: &[(&str, &str, String, char)] = &[
        (
            "first layer wins",
            "@layer a{p{color:red!important}} @layer b{p{color:blue!important}}",
            p.into(),
            'r',
        ),
        (
            "unlayered important is weaker than layered",
            "@layer a{p{color:red!important}} p{color:blue!important}",
            p.into(),
            'r',
        ),
        (
            "layered important beats unlayered normal",
            "@layer a{p{color:red!important}} p{color:blue}",
            p.into(),
            'r',
        ),
        (
            "sublayers: the first sublayer wins, the parent's own rules lose",
            "@layer a{@layer x{p{color:red!important}} @layer y{p{color:green!important}} p{color:blue!important}}",
            p.into(),
            'r',
        ),
        (
            "statement order sets the reversed order too",
            "@layer b, a; @layer a{p{color:red!important}} @layer b{p{color:blue!important}}",
            p.into(),
            'b',
        ),
        (
            "specificity inside one layer",
            "@layer a{p{color:red!important} #p{color:blue!important}}",
            p.into(),
            'b',
        ),
        (
            "important style attribute wins",
            "@layer a{p{color:red!important}}",
            attr("color:blue!important"),
            'b',
        ),
        (
            "normal style attribute loses to a layered important",
            "@layer a{p{color:red!important}}",
            attr("color:blue"),
            'r',
        ),
        (
            "normal style attribute beats a layered normal",
            "@layer a{p{color:red}}",
            attr("color:blue"),
            'b',
        ),
        (
            "normal style attribute loses to an unlayered important",
            "p{color:red!important}",
            attr("color:blue"),
            'r',
        ),
    ];
    for (name, css, html, want) in rows {
        assert_eq!(layer_color(&[css], html, &env), *want, "{name}");
    }
}

/// `revert-layer` rolls back to the previous layer of the origin; the
/// rows are Chromium 148's results (the `rl-*` probe cases).
#[test]
#[allow(clippy::too_many_lines, reason = "a table of cases")]
fn revert_layer_rolls_back_one_layer() {
    let env = MediaEnvironment::default();
    let p = "<p id=p>x</p>";
    let attr = |style: &str| format!("<p id=p style='{style}'>x</p>");
    let hint = "<font id=p color=red>x</font>";
    let rows: &[(&str, &str, String, char)] = &[
        (
            "style attribute rolls back to the unlayered rule",
            "@layer a{p{color:green}} p{color:blue}",
            attr("color:revert-layer"),
            'b',
        ),
        (
            "style attribute, important, rolls back to the first layer",
            "@layer a{p{color:green!important}} @layer b{p{color:red!important}}",
            attr("color:revert-layer!important"),
            'g',
        ),
        (
            "unlayered rule rolls back to the last layer",
            "@layer a{p{color:green}} p{color:revert-layer}",
            p.into(),
            'g',
        ),
        (
            "and skips the other unlayered rules",
            "@layer a{p{color:green}} p{color:red} #p{color:revert-layer}",
            p.into(),
            'g',
        ),
        (
            "without layers it reverts: no user-agent color here",
            "p{color:red} #p{color:revert-layer}",
            p.into(),
            'k',
        ),
        (
            "second layer rolls back to the first",
            "@layer a{p{color:green}} @layer b{p{color:red} #p{color:revert-layer}}",
            p.into(),
            'g',
        ),
        (
            "it skips the layers in between",
            "@layer a{p{color:green}} @layer b{p{color:red}} @layer c{p{color:revert-layer}}",
            p.into(),
            'r',
        ),
        (
            "the parent's own rules roll back to a sublayer",
            "@layer a{@layer x{p{color:green}} p{color:revert-layer}}",
            p.into(),
            'g',
        ),
        (
            "an empty layer in between",
            "@layer a{p{color:green}} @layer b{} @layer c{p{color:revert-layer}}",
            p.into(),
            'g',
        ),
        (
            "important in the first layer: the layers above it are dropped",
            "@layer a{p{color:revert-layer!important}} @layer b{p{color:green!important}}",
            p.into(),
            'k',
        ),
        (
            "important sees the normal rules of the layers below",
            "@layer a{p{color:green}} @layer b{p{color:red}} @layer c{p{color:revert-layer!important}}",
            p.into(),
            'r',
        ),
        (
            "important drops the unlayered rules",
            "@layer a{p{color:revert-layer!important}} p{color:green}",
            p.into(),
            'k',
        ),
        (
            "important drops the normal rules of its own layer",
            "@layer a{p{color:red} #p{color:revert-layer!important}}",
            p.into(),
            'k',
        ),
        (
            "the hint is below every layer",
            "@layer a{font{color:revert-layer}}",
            hint.into(),
            'r',
        ),
        (
            "revert drops the hint",
            "font{color:revert}",
            hint.into(),
            'k',
        ),
        (
            "a layer below wins over the hint",
            "@layer a{font{color:green}} font{color:revert-layer}",
            hint.into(),
            'g',
        ),
        (
            "an empty layer below leaves the hint",
            "@layer a{} font{color:revert-layer}",
            hint.into(),
            'r',
        ),
        (
            "a shorthand rolls back",
            "@layer a{p{color:green}} @layer b{p{all:revert-layer}}",
            p.into(),
            'g',
        ),
        (
            "a custom property rolls back",
            "@layer a{p{--c:green;color:var(--c)}} @layer b{p{--c:revert-layer}}",
            p.into(),
            'g',
        ),
    ];
    for (name, css, html, want) in rows {
        assert_eq!(layer_color(&[css], html, &env), *want, "{name}");
    }
}

/// The margin of `#p` (UA 16px; `revert-layer` in the first layer falls
/// back to the user-agent origin).
#[test]
fn revert_layer_in_the_first_layer_reaches_the_user_agent_sheet() {
    for (css, want) in [
        ("@layer a{p{margin-top:revert-layer}}", 16.0),
        (
            "@layer a{p{margin-top:7px} #p{margin-top:revert-layer}}",
            16.0,
        ),
        ("p{margin-top:7px} #p{margin-top:revert-layer}", 16.0),
        ("@layer a{p{margin-top:revert-layer!important}}", 16.0),
        (
            "@layer a{p{margin-top:3px}} @layer b{p{margin-top:revert-layer}}",
            3.0,
        ),
    ] {
        let (doc, map) = render("<p id=p>x</p>", css);
        assert_eq!(
            get(&doc, &map, "p").margin_top,
            LengthPercentageOrAuto::px(want),
            "{css}"
        );
    }
}

#[test]
fn layers_keep_the_pseudo_element_order() {
    let (doc, map) = render(
        "<p id=p>x</p>",
        "@layer a{#p::before{content:'a';color:red}} @layer b{p::before{content:'b';color:blue}}",
    );
    let pseudo = map
        .pseudo(node(&doc, "p"), PseudoKind::Before)
        .expect("a ::before");
    assert_eq!(pseudo.color, BLUE);
}
