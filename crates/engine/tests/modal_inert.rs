//! Modal inertness (M10 sub-feature 6e, ADR 0037 section 7): while a modal
//! dialog blocks the document, the rest of the document gets no hit, no
//! focus, no `:hover` and no selection. The pages are the ones of the
//! probe cases `inert-*` of `tools/probes/web-top-layer.json` and the
//! `e-*` cases that 6e added; the expected values are Chromium's (the
//! tables in `crates/automation/tests/data/web-top-layer-steps.json` and
//! `crates/engine/tests/probe_net/top_layer.rs`). The tests here check the
//! engine's own state: the hit tests, the focus, the Tab order, the
//! selection.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use swb_engine::{Evaluation, JsonForm, Key, Modifiers, MouseButton, Page, Size};
use swb_net::NetworkFetcher;

mod common;
use common::{Site, center, finish_loading, new_page, node, on_page_thread, rect};

/// A page with JavaScript on that shows `html` in 800 x 600.
fn open(site: &Site, html: &str) -> Page {
    let url = site.page("page.html", html);
    let mut page = new_page(Arc::new(NetworkFetcher::new()), 2, Size::new(800.0, 600.0));
    page.set_javascript(true);
    page.navigate(url);
    finish_loading(&mut page, Duration::from_secs(30));
    page
}

/// The value of the script `source` as text.
fn js(page: &mut Page, source: &str) -> String {
    let value = match page.evaluate(source, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => text,
        other => panic!("{source}: {other:?}"),
    };
    page.update_layout();
    value.trim_matches('"').to_owned()
}

fn press(page: &mut Page, x: f32, y: f32, clicks: u32) {
    page.mouse_down(x, y, MouseButton::Primary, Modifiers::NONE, clicks);
}

fn click_at(page: &mut Page, x: f32, y: f32) {
    press(page, x, y, 1);
    page.mouse_up(x, y, MouseButton::Primary);
}

fn tab(page: &mut Page, backward: bool) {
    let modifiers = Modifiers {
        shift: backward,
        ..Modifiers::NONE
    };
    page.key_down(&Key::Tab, modifiers);
    page.key_up(&Key::Tab, modifiers);
}

/// The page of the `inert-*` probe cases, with a log of the pointer and
/// focus events (the event targets, as `k()` writes them).
const PAGE: &str = "<!DOCTYPE html><body style='margin:0'>\
    <style>button,input,p{display:block;margin:4px} iframe{width:120px;height:60px;border:0}\
    dialog{width:200px;padding:30px}</style>\
    <button id=ob>outside</button><input id=oi><p id=tx>some outside text</p>\
    <iframe id=fr srcdoc='<button id=fb>fb</button>'></iframe>\
    <dialog id=d><button id=db>in</button><input id=di></dialog>\
    <script>var L=[];function k(e){return e.localName+(e.id?'#'+e.id:'')}\
    ['pointerdown','mousedown','click','focusin'].forEach(function(t){\
    document.addEventListener(t,function(e){L.push(t+':'+k(e.target))},true)});\
    addEventListener('load',function(){\
    fr.contentDocument.addEventListener('click',function(e){L.push('frame-click:'+k(e.target))},true);\
    fr.contentDocument.addEventListener('focusin',function(e){L.push('frame-focus:'+k(e.target))},true);\
    d.showModal();L.length=0})</script>";

fn log(page: &mut Page) -> String {
    let log = js(page, "L.join(' ')");
    js(page, "L.length=0;0");
    log
}

#[test]
fn hits_outside_the_dialog_fall_to_its_backdrop() {
    // `inert-click`, `top-layer-hit-order` rows 5 and 6.
    on_page_thread(|| {
        let site = Site::new("modal-inert-hit");
        let mut page = open(&site, PAGE);
        let ob = center(rect(&mut page, "ob"));
        let d = node(&page, "d");
        let hit = page.hit_test(ob.0, ob.1).unwrap();
        assert_eq!(hit.node, d, "a hit on the outside button gives the dialog");
        let text = center(rect(&mut page, "tx"));
        assert_eq!(page.hit_test(text.0, text.1).unwrap().node, d);
        // Inside the dialog the hit is normal (the button's text).
        let db = center(rect(&mut page, "db"));
        let inside = page.hit_test(db.0, db.1).unwrap().node;
        assert_eq!(
            page.document().unwrap().flat_parent_element(inside),
            Some(node(&page, "db"))
        );
        // `elementFromPoint` and `elementsFromPoint`: the dialog, then the
        // root.
        let script = format!(
            "[k(document.elementFromPoint({0},{1})),\
             document.elementsFromPoint({0},{1}).map(k).join(',')].join(' ')",
            ob.0, ob.1
        );
        assert_eq!(js(&mut page, &script), "dialog#d dialog#d,html");
        let script = format!(
            "document.elementsFromPoint({},{}).map(k).join(',')",
            db.0, db.1
        );
        assert_eq!(js(&mut page, &script), "button#db,dialog#d,html");
    });
}

#[test]
fn a_popover_outside_the_dialog_is_drawn_but_inert() {
    // `top-layer-hit-order` row 7, and a popover inside the dialog.
    on_page_thread(|| {
        let site = Site::new("modal-inert-popover");
        let html = "<!DOCTYPE html><body style='margin:0'><style>\
            [popover]{margin:0;padding:0;border:0;width:100px;height:100px;inset:auto;left:0;top:0}\
            dialog{padding:0;border:0;width:60px;height:60px}</style>\
            <div id=out popover=manual>o</div>\
            <dialog id=d>d<div id=inn popover=manual style='left:300px;top:200px;width:50px;height:50px'>i</div></dialog>\
            <script>function k(e){return e.localName+(e.id?'#'+e.id:'')}\
            d.showModal();out.showPopover();inn.showPopover()</script>";
        let mut page = open(&site, html);
        let at = |page: &mut Page, x: u32, y: u32| {
            js(
                page,
                &format!("document.elementsFromPoint({x},{y}).map(k).join(',')"),
            )
        };
        assert_eq!(at(&mut page, 50, 50), "dialog#d,html");
        assert_eq!(at(&mut page, 320, 220), "div#inn,dialog#d,html");
    });
}

#[test]
fn clicks_on_inert_content_go_to_the_dialog_and_focus_it() {
    // `inert-click`: pointerdown, mousedown, click go to the dialog; the
    // first click focuses it; the iframe gets nothing.
    on_page_thread(|| {
        let site = Site::new("modal-inert-click");
        let mut page = open(&site, PAGE);
        let d = node(&page, "d");
        assert_eq!(page.focused_element(), Some(node(&page, "db")));
        let ob = center(rect(&mut page, "ob"));
        click_at(&mut page, ob.0, ob.1);
        assert_eq!(
            log(&mut page),
            "pointerdown:dialog#d mousedown:dialog#d focusin:dialog#d click:dialog#d"
        );
        assert_eq!(page.focused_element(), Some(d));
        let oi = center(rect(&mut page, "oi"));
        click_at(&mut page, oi.0, oi.1);
        let fr = center(rect(&mut page, "fr"));
        click_at(&mut page, fr.0, fr.1);
        let text = center(rect(&mut page, "tx"));
        click_at(&mut page, text.0, text.1);
        assert_eq!(
            log(&mut page),
            "pointerdown:dialog#d mousedown:dialog#d click:dialog#d \
             pointerdown:dialog#d mousedown:dialog#d click:dialog#d \
             pointerdown:dialog#d mousedown:dialog#d click:dialog#d"
        );
        assert_eq!(page.focused_element(), Some(d));
        // The frame's own document is not blocked.
        let frame = page
            .frame_of(swb_engine::FrameId::TOP, node(&page, "fr"))
            .unwrap();
        let fb = page
            .with_frame(frame, |p| {
                let b = node(p, "fb");
                p.element_box(b).is_some()
            })
            .unwrap();
        assert!(fb);
    });
}

#[test]
fn a_click_on_the_dialog_itself_focuses_it_unless_a_control_is_nearer() {
    // `e-click-padding`, `e-click-text` (Chromium: the dialog), and the
    // button of the dialog (focused).
    on_page_thread(|| {
        let site = Site::new("modal-inert-click-dialog");
        let html = "<!DOCTYPE html><body style='margin:0'><style>button,p{display:block;margin:4px}\
            dialog{width:200px;padding:30px}</style><button id=ob>outside</button>\
            <dialog id=d><p id=pt>text in</p><button id=db>in</button></dialog>\
            <script>d.showModal()</script>";
        let mut page = open(&site, html);
        let (d, db) = (node(&page, "d"), node(&page, "db"));
        assert_eq!(page.focused_element(), Some(db));
        let pt = center(rect(&mut page, "pt"));
        click_at(&mut page, pt.0, pt.1);
        assert_eq!(page.focused_element(), Some(d), "a click on text");
        let button = center(rect(&mut page, "db"));
        click_at(&mut page, button.0, button.1);
        assert_eq!(page.focused_element(), Some(db), "a click on a control");
        // The padding of the dialog.
        let bounds = rect(&mut page, "d");
        click_at(&mut page, bounds.x + 10.0, bounds.y + 10.0);
        assert_eq!(page.focused_element(), Some(d), "a click on the padding");
        // The backdrop, while the focus is elsewhere.
        click_at(&mut page, button.0, button.1);
        click_at(&mut page, 5.0, 590.0);
        assert_eq!(page.focused_element(), Some(d), "a click on the backdrop");
    });
}

#[test]
fn inert_elements_cannot_be_focused_and_the_tab_order_is_the_dialogs() {
    // `inert-tab`, `e-script`: Tab 1 `di`, leaves, `db`, `di`, leaves, then
    // Shift+Tab `di`.
    on_page_thread(|| {
        let site = Site::new("modal-inert-tab");
        let mut page = open(&site, PAGE);
        let at = |page: &mut Page| {
            js(
                page,
                "document.activeElement ? k(document.activeElement) : 'none'",
            )
        };
        assert_eq!(at(&mut page), "button#db");
        assert_eq!(
            js(&mut page, "ob.focus();k(document.activeElement)"),
            "button#db"
        );
        let mut seen = Vec::new();
        for _ in 0..5 {
            tab(&mut page, false);
            seen.push(at(&mut page));
        }
        assert_eq!(
            seen,
            ["input#di", "body", "button#db", "input#di", "body"],
            "after the last element the focus leaves the document"
        );
        tab(&mut page, true);
        assert_eq!(at(&mut page), "input#di");
    });
}

#[test]
fn the_tab_order_under_a_modal_dialog_follows_popovers_inside_it() {
    // `e-tab-popover-in-dialog`: an invoker's popover in the dialog follows
    // its invoker; a popover outside the dialog is not visited.
    on_page_thread(|| {
        let site = Site::new("modal-inert-tab-popover");
        let html = "<!DOCTYPE html><body><button id=ob>outside</button>\
            <div popover=manual id=op><button id=opb>opb</button></div>\
            <dialog id=d><button id=di1 popovertarget=ip>di1</button>\
            <div popover id=ip><button id=ipb>ipb</button></div>\
            <button id=di2>di2</button></dialog>\
            <script>function k(e){return e.localName+(e.id?'#'+e.id:'')}\
            op.showPopover();d.showModal()</script>";
        let mut page = open(&site, html);
        let at = |page: &mut Page| js(page, "k(document.activeElement)");
        assert_eq!(at(&mut page), "button#di1");
        page.key_down(&Key::Enter, Modifiers::NONE);
        page.key_up(&Key::Enter, Modifiers::NONE);
        assert_eq!(js(&mut page, "ip.matches(':popover-open')"), "true");
        let mut seen = Vec::new();
        for _ in 0..4 {
            tab(&mut page, false);
            seen.push(at(&mut page));
        }
        assert_eq!(seen, ["button#ipb", "button#di2", "body", "button#di1"]);
    });
}

#[test]
fn shift_tab_enters_the_dialog_at_its_end() {
    // `e-tab-backward`.
    on_page_thread(|| {
        let site = Site::new("modal-inert-tab-back");
        let html = "<!DOCTYPE html><body><button id=ob>outside</button>\
            <dialog id=d><button id=db>db</button><input id=di></dialog>\
            <button id=ob2>outside2</button>\
            <script>function k(e){return e.localName+(e.id?'#'+e.id:'')}d.showModal()</script>";
        let mut page = open(&site, html);
        let at = |page: &mut Page| js(page, "k(document.activeElement)");
        let mut seen = Vec::new();
        for _ in 0..4 {
            tab(&mut page, true);
            seen.push(at(&mut page));
        }
        assert_eq!(seen, ["body", "input#di", "button#db", "body"]);
    });
}

#[test]
fn hover_over_inert_content_matches_the_dialog_chain() {
    // `inert-hover`: `html`, `body` and the dialog match; the button does not.
    on_page_thread(|| {
        let site = Site::new("modal-inert-hover");
        let mut page = open(&site, PAGE);
        let ob = center(rect(&mut page, "ob"));
        page.mouse_move(ob.0, ob.1);
        assert_eq!(
            js(
                &mut page,
                "[ob.matches(':hover'),document.body.matches(':hover'),\
                 document.documentElement.matches(':hover'),d.matches(':hover'),\
                 Array.prototype.map.call(document.querySelectorAll(':hover'),k).join('/')].join()"
            ),
            "false,true,true,true,html/body/dialog#d"
        );
        // Pressing on inert content does not make it `:active`.
        press(&mut page, ob.0, ob.1, 1);
        assert_eq!(js(&mut page, "ob.matches(':active')"), "false");
        assert_eq!(js(&mut page, "d.matches(':active')"), "true");
    });
}

#[test]
fn the_selection_does_not_start_in_inert_text() {
    // `inert-selection` row 1: swb does not select the word (Chromium
    // selects a word of the dialog's button; ADR 0037 section 11).
    on_page_thread(|| {
        let site = Site::new("modal-inert-selection");
        let mut page = open(&site, PAGE);
        let text = rect(&mut page, "tx");
        press(&mut page, text.x + 10.0, text.y + 5.0, 2);
        page.mouse_up(text.x + 10.0, text.y + 5.0, MouseButton::Primary);
        assert_eq!(page.selected_text(), "");
        // A drag from inside the dialog does not extend into the outside
        // text either.
        let db = center(rect(&mut page, "db"));
        press(&mut page, db.0, db.1, 1);
        page.mouse_move(text.x + 40.0, text.y + 5.0);
        page.mouse_up(text.x + 40.0, text.y + 5.0, MouseButton::Primary);
        assert!(!page.selected_text().contains("outside"));
        // Select all stays in the dialog.
        page.key_down(&Key::Character("a".into()), common::CTRL);
        assert!(!page.selected_text().contains("outside"));
    });
}

#[test]
fn selectstart_fires_for_a_press_on_dialog_text_and_on_the_backdrop() {
    // `x-selectstart-modal`, `-nonmodal`, `-backdrop-text`, `-backdrop-blank`
    // and `-plain` (Chromium: `selectstart` at the text node, at the dialog
    // for a press on its backdrop; none for a press on a control).
    on_page_thread(|| {
        let site = Site::new("modal-inert-selectstart");
        let html = "<!DOCTYPE html><body style='margin:0'><style>button{display:block}\
            dialog{position:absolute;margin:0;left:100px;top:100px;width:200px;padding:0;border:0}\
            p{margin:0}</style><p id=op>outside text here</p><button id=ob>outside</button>\
            <dialog id=d><p id=pt>text in the dialog</p><button id=db>in</button></dialog>\
            <dialog id=n open style='top:300px'><p id=nt>text in a plain dialog</p></dialog>\
            <script>var L=[];function k(e){return e.nodeType==3?'#text':e.localName+(e.id?'#'+e.id:'')}\
            document.addEventListener('selectstart',function(e){L.push('selectstart:'+k(e.target))});\
            d.showModal();L.length=0</script>";
        let mut page = open(&site, html);
        let pt = rect(&mut page, "pt");
        click_at(&mut page, pt.x + 5.0, pt.y + 5.0);
        assert_eq!(
            log(&mut page),
            "selectstart:#text",
            "text in a modal dialog"
        );
        click_at(&mut page, 5.0, 8.0);
        assert_eq!(log(&mut page), "selectstart:dialog#d", "backdrop, text");
        click_at(&mut page, 5.0, 590.0);
        assert_eq!(log(&mut page), "selectstart:dialog#d", "backdrop, blank");
        let db = center(rect(&mut page, "db"));
        click_at(&mut page, db.0, db.1);
        assert_eq!(log(&mut page), "", "a control that has the focus");
        js(&mut page, "d.close();0");
        let nt = rect(&mut page, "nt");
        click_at(&mut page, nt.x + 5.0, nt.y + 5.0);
        assert_eq!(log(&mut page), "selectstart:#text", "a dialog that is open");
        let ob = center(rect(&mut page, "ob"));
        click_at(&mut page, ob.0, ob.1);
        assert_eq!(log(&mut page), "", "a button");
        let op = rect(&mut page, "op");
        click_at(&mut page, op.x + 5.0, op.y + 5.0);
        assert_eq!(log(&mut page), "selectstart:#text", "plain text");
    });
}

#[test]
fn a_click_on_a_dialog_shown_as_a_popover_focuses_it() {
    // `x-popover-dialog` (Chromium: `focusin:dialog#d`), with scripts (the
    // `showPopover` call) and without (an invoker).
    on_page_thread(|| {
        for scripts in [true, false] {
            let site = Site::new("modal-inert-popover-dialog");
            let html = "<!DOCTYPE html><body style='margin:0'><style>button,p{display:block;margin:4px}\
                dialog{width:200px;padding:30px}</style><button id=ob popovertarget=d>open</button>\
                <dialog id=d popover><p id=pt>text in</p></dialog>";
            let url = site.page("page.html", html);
            let mut page = new_page(Arc::new(NetworkFetcher::new()), 2, Size::new(800.0, 600.0));
            page.set_javascript(scripts);
            page.navigate(url);
            finish_loading(&mut page, Duration::from_secs(30));
            let ob = center(rect(&mut page, "ob"));
            click_at(&mut page, ob.0, ob.1);
            page.update_layout();
            let d = node(&page, "d");
            let pt = center(rect(&mut page, "pt"));
            click_at(&mut page, pt.0, pt.1);
            assert_eq!(page.focused_element(), Some(d), "scripts {scripts}");
        }
    });
}

#[test]
fn without_a_modal_dialog_everything_is_hit_and_focusable() {
    // The same page, the dialog closed.
    on_page_thread(|| {
        let site = Site::new("modal-inert-closed");
        let mut page = open(&site, PAGE);
        js(&mut page, "d.close();0");
        let ob = center(rect(&mut page, "ob"));
        let hit = page.hit_test(ob.0, ob.1).unwrap().node;
        assert_eq!(
            page.document().unwrap().flat_parent_element(hit),
            Some(node(&page, "ob"))
        );
        assert_eq!(
            js(&mut page, "ob.focus();k(document.activeElement)"),
            "button#ob"
        );
        log(&mut page);
        click_at(&mut page, ob.0, ob.1);
        assert_eq!(
            log(&mut page),
            "pointerdown:button#ob mousedown:button#ob click:button#ob"
        );
    });
}
