//! Sub-feature 6d (ADR 0037 sections 1, 5, 6 and 10): `show()`,
//! `showModal()`, `close()`, `requestClose()`, the close watchers of
//! dialogs and their `cancel` rule, the `open` removal rule, the dialog
//! focusing steps and the form method `dialog`, with the recording host of
//! the parent tests.

use super::*;

/// Two dialogs with buttons, one with `closedby=any`, one with
/// `closedby=none`, a popover and a button outside.
const DIALOGS: &str = "<button id=ob>o</button><dialog id=d><button id=db>x</button></dialog>\
    <dialog id=d2><p id=t2>t</p></dialog><dialog id=da closedby=any>a</dialog>\
    <dialog id=dn closedby=none>n</dialog><dialog id=dc closedby=closerequest>c</dialog>\
    <div id=pp popover>p</div>";

fn show(h: &mut Rec, id: &str) -> Result<(), PopoverError> {
    let el = h.id(id);
    show_dialog(h, el)
}

fn modal(h: &mut Rec, id: &str) -> Result<(), PopoverError> {
    let el = h.id(id);
    show_modal(h, el, None)
}

fn close(h: &mut Rec, id: &str, result: Option<&str>) {
    let el = h.id(id);
    close_dialog(h, el, result.map(str::to_owned)).unwrap();
}

fn escape(h: &mut Rec) -> bool {
    process_close_watchers(h).unwrap()
}

fn is_open(h: &Rec, id: &str) -> bool {
    h.doc.element(h.id(id)).unwrap().has_attr("open")
}

fn is_modal(h: &Rec, id: &str) -> bool {
    h.doc.element(h.id(id)).unwrap().is_modal()
}

#[test]
fn show_and_close_fire_their_events_and_tasks() {
    let mut h = Rec::new(DIALOGS);
    show(&mut h, "d").unwrap();
    assert!(is_open(&h, "d") && !is_modal(&h, "d"));
    // A non-modal dialog is not in the top layer.
    assert_eq!(h.doc.top_layer().unwrap().layer(), []);
    assert_eq!(h.doc.open_dialogs(), [h.id("d")]);
    // The focusing steps gave the focus to the first focusable element.
    assert_eq!(
        h.take_log(),
        "bt:d(closed>open)! focus:db dtoggle:d(closed>open)"
    );
    // A second show does nothing.
    show(&mut h, "d").unwrap();
    assert_eq!(h.take_log(), "");
    close(&mut h, "d", Some("r1"));
    assert!(!is_open(&h, "d"));
    assert_eq!(h.doc.dialog_return_value(h.id("d")), "r1");
    assert_eq!(h.doc.open_dialogs(), []);
    assert_eq!(
        h.take_log(),
        "bt:d(open>closed) dtoggle:d(open>closed) close:d"
    );
    // A close of a closed dialog does nothing and keeps the value.
    close(&mut h, "d", Some("again"));
    assert_eq!(h.doc.dialog_return_value(h.id("d")), "r1");
    assert_eq!(h.take_log(), "");
    invariants(&h.doc);
}

#[test]
fn toggle_tasks_of_a_dialog_coalesce() {
    let mut h = Rec::new(DIALOGS);
    modal(&mut h, "d").unwrap();
    close(&mut h, "d", Some("x"));
    // One task: the old state of the first change, the new of the last.
    assert_eq!(
        h.take_log(),
        "bt:d(closed>open)! focus:db bt:d(open>closed) dtoggle:d(closed>closed) close:d"
    );
}

#[test]
fn show_modal_puts_the_dialog_in_the_top_layer() {
    let mut h = Rec::new(DIALOGS);
    modal(&mut h, "d").unwrap();
    modal(&mut h, "d2").unwrap();
    assert!(is_modal(&h, "d") && is_modal(&h, "d2"));
    let layer = h.doc.top_layer().unwrap().layer().to_vec();
    assert_eq!(layer, [h.id("d"), h.id("d2")]);
    let blocking = h.doc.top_layer().unwrap().blocking();
    assert_eq!(blocking, Some(h.id("d2")));
    // The first element that the host can focus (the recording host can
    // focus any element).
    assert!(h.take_log().contains("focus:t2"));
    // `showModal()` of an open modal dialog does nothing.
    modal(&mut h, "d").unwrap();
    assert_eq!(h.take_log(), "");
    close(&mut h, "d2", None);
    assert!(!is_modal(&h, "d2"));
    let layer = h.doc.top_layer().unwrap().layer().to_vec();
    assert_eq!(layer, [h.id("d")]);
    invariants(&h.doc);
}

#[test]
fn the_blocking_dialog_is_kept_in_step_with_the_top_layer() {
    // `blocking_dialog` is O(1): the dialog is noted at each show, close,
    // removal and adoption. `invariants` compares it with a walk of the
    // layer.
    let mut h = Rec::new(
        "<dialog id=d1>1</dialog><dialog id=d2>2</dialog><dialog id=d3>3</dialog>\
         <div id=pp popover=manual>p</div><div id=w><dialog id=d4>4</dialog></div>",
    );
    let (d1, d2, d3, d4) = (h.id("d1"), h.id("d2"), h.id("d3"), h.id("d4"));
    assert_eq!(h.doc.blocking_dialog(), None);
    modal(&mut h, "d1").unwrap();
    modal(&mut h, "d2").unwrap();
    // A popover above the modal dialogs does not block.
    h.show("pp").unwrap();
    modal(&mut h, "d3").unwrap();
    assert_eq!(h.doc.blocking_dialog(), Some(d3));
    invariants(&h.doc);
    // The close of a dialog below the top one keeps the top one; the close
    // of the top one gives the blocking to the next.
    close(&mut h, "d2", None);
    assert_eq!(h.doc.blocking_dialog(), Some(d3));
    invariants(&h.doc);
    close(&mut h, "d3", None);
    assert_eq!(h.doc.blocking_dialog(), Some(d1));
    invariants(&h.doc);
    // A removal of the blocking dialog, with its subtree.
    modal(&mut h, "d4").unwrap();
    assert_eq!(h.doc.blocking_dialog(), Some(d4));
    let w = h.id("w");
    h.doc.remove(w);
    assert_eq!(h.doc.blocking_dialog(), Some(d1));
    invariants(&h.doc);
    // An adoption into another document of the arena.
    let other = h.doc.create_document().unwrap();
    h.doc.adopt(d1, other).unwrap();
    assert_eq!(h.doc.blocking_dialog(), None);
    invariants(&h.doc);
    // A dialog that shows again gets the blocking once more.
    modal(&mut h, "d2").unwrap();
    assert_eq!(h.doc.blocking_dialog(), Some(d2));
    invariants(&h.doc);
}

#[test]
fn a_dialog_is_shown_when_it_is_open_or_a_popover_that_shows() {
    // The click focus rule (`x-popover-dialog`, `e-click-*`).
    let mut h = Rec::new(
        "<dialog id=d>d</dialog><dialog id=dp popover=manual>p</dialog>\
         <div id=dv popover=manual>v</div><div id=nn>n</div>",
    );
    let (d, dp, dv, nn) = (h.id("d"), h.id("dp"), h.id("dv"), h.id("nn"));
    for node in [d, dp, dv, nn] {
        assert!(!h.doc.is_shown_dialog(node));
    }
    show(&mut h, "d").unwrap();
    h.show("dp").unwrap();
    h.show("dv").unwrap();
    assert!(h.doc.is_shown_dialog(d) && h.doc.is_open_dialog(d));
    assert!(h.doc.is_shown_dialog(dp) && !h.doc.is_open_dialog(dp));
    assert!(!h.doc.is_shown_dialog(dv) && !h.doc.is_shown_dialog(nn));
}

#[test]
fn a_modal_dialog_makes_the_other_nodes_of_the_document_inert() {
    // ADR 0037 section 7: the dialog and its flat descendants are not inert;
    // the rest of the document, a modal dialog below the topmost one, and
    // the document itself are; a detached node is not in the document.
    let mut h = Rec::new(DIALOGS);
    let (ob, d, db, d2, t2) = (h.id("ob"), h.id("d"), h.id("db"), h.id("d2"), h.id("t2"));
    assert_eq!(h.doc.blocking_dialog(), None);
    assert!(!h.doc.is_modal_inert(ob));
    modal(&mut h, "d").unwrap();
    assert_eq!(h.doc.blocking_dialog(), Some(d));
    assert!(h.doc.is_modal_inert(ob) && h.doc.is_modal_inert(d2));
    assert!(!h.doc.is_modal_inert(d) && !h.doc.is_modal_inert(db));
    assert!(h.doc.is_modal_inert(NodeId::DOCUMENT));
    modal(&mut h, "d2").unwrap();
    assert_eq!(h.doc.blocking_dialog(), Some(d2));
    assert!(h.doc.is_modal_inert(d) && h.doc.is_modal_inert(db));
    assert!(!h.doc.is_modal_inert(d2) && !h.doc.is_modal_inert(t2));
    let name = crate::QualName::new(None, crate::ns!(html), "div".into());
    let detached = h.doc.create_element(name, Vec::new()).unwrap();
    assert!(!h.doc.is_modal_inert(detached));
    // The close of the topmost one gives the blocking back to the first.
    close(&mut h, "d2", None);
    assert_eq!(h.doc.blocking_dialog(), Some(d));
    close(&mut h, "d", None);
    assert!(!h.doc.is_modal_inert(ob));
}

#[test]
fn dialog_errors_have_chromiums_texts() {
    let mut h = Rec::new(DIALOGS);
    show(&mut h, "d").unwrap();
    let error = modal(&mut h, "d").unwrap_err();
    assert_eq!(error.name(), "InvalidStateError");
    assert_eq!(
        error.message(),
        "The dialog is already open as a non-modal dialog, and therefore cannot be opened as a \
         modal dialog."
    );
    modal(&mut h, "d2").unwrap();
    assert_eq!(
        show(&mut h, "d2").unwrap_err().message(),
        "The dialog is already open as a modal dialog, and therefore cannot be opened as a \
         non-modal dialog."
    );
    // A detached dialog: `showModal()` throws, `show()` opens it.
    let name = crate::QualName::new(None, crate::ns!(html), crate::LocalName::from("dialog"));
    let detached = h.doc.create_element(name, Vec::new()).unwrap();
    assert_eq!(
        show_modal(&mut h, detached, None).unwrap_err().message(),
        "The element is not in a Document."
    );
    show_dialog(&mut h, detached).unwrap();
    assert!(h.doc.element(detached).unwrap().has_attr("open"));
    // Not connected: no close watcher, not in the open dialogs list.
    assert!(!h.doc.element(detached).unwrap().is_dialog_open());
    // A dialog that shows as a popover.
    let dp = h.id("dc");
    h.doc.set_attr(dp, "popover", "manual").unwrap();
    show_popover(&mut h, dp, true, None).unwrap();
    assert_eq!(
        show_modal(&mut h, dp, None).unwrap_err().message(),
        "The dialog is already open as a Popover, and therefore cannot be opened as a modal \
         dialog."
    );
    // `show()` opens it as a dialog too (measured, `exceptions` #3).
    show_dialog(&mut h, dp).unwrap();
    assert!(h.doc.element(dp).unwrap().has_attr("open"));
    invariants(&h.doc);
}

#[test]
fn a_canceled_beforetoggle_keeps_the_dialog_closed() {
    let mut h = Rec::new(DIALOGS);
    h.cancel_shows = true;
    modal(&mut h, "d").unwrap();
    show(&mut h, "d2").unwrap();
    assert!(!is_open(&h, "d") && !is_open(&h, "d2"));
    assert_eq!(h.take_log(), "bt:d(closed>open)! bt:d2(closed>open)!");
    invariants(&h.doc);
}

#[test]
fn request_close_fires_a_cancelable_cancel() {
    let mut h = Rec::new(DIALOGS);
    let d = h.id("d");
    // No activation: `requestClose()` does not need one.
    modal(&mut h, "d").unwrap();
    let _ = h.take_log();
    h.prevent_cancel = true;
    request_close_dialog(&mut h, d, Some("rq".to_owned())).unwrap();
    assert!(is_open(&h, "d"));
    assert_eq!(h.doc.dialog_return_value(d), "");
    h.prevent_cancel = false;
    request_close_dialog(&mut h, d, Some("rq2".to_owned())).unwrap();
    assert!(!is_open(&h, "d"));
    assert_eq!(h.doc.dialog_return_value(d), "rq2");
    assert_eq!(
        h.take_log(),
        "cancel:d! cancel:d! bt:d(open>closed) dtoggle:d(open>closed) close:d"
    );
    // `closedby=none` does not stop `requestClose()`.
    let dn = h.id("dn");
    modal(&mut h, "dn").unwrap();
    request_close_dialog(&mut h, dn, None).unwrap();
    assert!(!is_open(&h, "dn"));
    invariants(&h.doc);
}

#[test]
fn escape_closes_dialogs_and_popovers_of_one_group() {
    let mut h = Rec::new(DIALOGS);
    // Made before any activation: one group.
    modal(&mut h, "d").unwrap();
    h.show("pp").unwrap();
    assert_eq!(h.doc.close_watcher_counts(), (1, 2));
    let _ = h.take_log();
    assert!(escape(&mut h));
    // The newest first; without an activation `cancel` is not cancelable.
    assert_eq!(
        h.take_log(),
        "bt:pp(open>closed) cancel:d bt:d(open>closed) toggle:pp(open>closed) \
         dtoggle:d(open>closed) close:d"
    );
    assert!(!is_open(&h, "d"));
    assert_eq!(h.doc.close_watcher_counts(), (0, 0));
    invariants(&h.doc);
}

#[test]
fn closedby_decides_which_dialogs_escape_closes() {
    let mut h = Rec::new(DIALOGS);
    // A plain non-modal dialog has a watcher that is not enabled.
    show(&mut h, "d").unwrap();
    show(&mut h, "dc").unwrap();
    modal(&mut h, "dn").unwrap();
    assert_eq!(h.doc.close_watcher_counts(), (1, 3));
    let _ = h.take_log();
    assert!(escape(&mut h));
    assert!(is_open(&h, "d") && is_open(&h, "dn") && !is_open(&h, "dc"));
    assert_eq!(
        h.take_log(),
        "cancel:dc bt:dc(open>closed) dtoggle:dc(open>closed) close:dc"
    );
    // Nothing that a request closes is left.
    assert!(!escape(&mut h));
    // `closedBy`: missing and invalid give the Auto state.
    let (d, dn, da) = (h.id("d"), h.id("dn"), h.id("da"));
    assert_eq!(ClosedBy::of(&h.doc, d), ClosedBy::None);
    assert_eq!(ClosedBy::of(&h.doc, dn), ClosedBy::None);
    assert_eq!(ClosedBy::of(&h.doc, da), ClosedBy::Any);
    h.doc.set_attr(d, "closedby", "BOGUS").unwrap();
    assert_eq!(ClosedBy::of(&h.doc, d), ClosedBy::None);
    close(&mut h, "d", None);
    modal(&mut h, "d").unwrap();
    assert_eq!(ClosedBy::of(&h.doc, d), ClosedBy::CloseRequest);
    invariants(&h.doc);
}

#[test]
fn the_cancel_rule_follows_the_activations() {
    // `xo-escape-cancel-after-click`: an activation makes the first `cancel`
    // cancelable; a canceled one consumes it.
    let mut h = Rec::new(DIALOGS);
    modal(&mut h, "d").unwrap();
    h.prevent_cancel = true;
    h.doc.note_user_activation();
    let _ = h.take_log();
    assert!(escape(&mut h));
    assert!(is_open(&h, "d"));
    assert!(!h.doc.has_history_action_activation());
    // `xo-escape-cancel-click-between`: a second activation sets the
    // history-action activation again, but the close request took the group
    // that the first one allowed: not cancelable.
    h.doc.note_user_activation();
    assert!(h.doc.has_history_action_activation());
    assert!(escape(&mut h));
    assert!(!is_open(&h, "d"));
    assert_eq!(
        h.take_log().split(' ').take(2).collect::<Vec<_>>(),
        ["cancel:d!", "cancel:d"]
    );
}

#[test]
fn an_escape_with_nothing_open_keeps_the_allowed_groups() {
    // `esc0`: Chromium does not lower the allowed number of groups at an
    // Escape that finds no group, so the `cancel` of the next dialog is
    // cancelable (the standard's step 3 would lower it).
    let mut h = Rec::new(DIALOGS);
    h.doc.note_user_activation();
    assert!(!escape(&mut h));
    assert!(!escape(&mut h));
    modal(&mut h, "d").unwrap();
    h.prevent_cancel = true;
    let _ = h.take_log();
    assert!(escape(&mut h));
    assert!(is_open(&h, "d"));
    assert_eq!(h.take_log(), "cancel:d!");
}

/// The `cancel` listener of a request: `r-nested-requestclose`.
fn nested_request(h: &mut Rec, dialog: NodeId) {
    request_close_dialog(h, dialog, Some("B".to_owned())).unwrap();
    // The nested request returned before any close.
    assert!(h.doc.element(dialog).unwrap().has_attr("open"));
}

#[test]
fn a_request_inside_the_cancel_action_does_not_close() {
    let mut h = Rec::new(DIALOGS);
    modal(&mut h, "da").unwrap();
    let da = h.id("da");
    h.cancel_hook = Some(nested_request);
    let _ = h.take_log();
    request_close_dialog(&mut h, da, Some("A".to_owned())).unwrap();
    // One `cancel`; the outer request closes with the nested value.
    assert_eq!(h.take_log().matches("cancel:").count(), 1);
    assert!(!is_open(&h, "da"));
    assert_eq!(h.doc.dialog_return_value(da), "B");
    invariants(&h.doc);
}

#[test]
fn a_listener_that_removes_the_dialog_stops_the_request() {
    // `r-remove-in-cancel`: `open` stays, no `close`, no return value.
    let mut h = Rec::new(DIALOGS);
    modal(&mut h, "da").unwrap();
    let da = h.id("da");
    h.cancel_hook = Some(|h, dialog| h.doc.remove(dialog));
    let _ = h.take_log();
    request_close_dialog(&mut h, da, Some("A".to_owned())).unwrap();
    assert!(h.doc.element(da).unwrap().has_attr("open"));
    assert_eq!(h.doc.dialog_return_value(da), "");
    assert!(!h.take_log().contains("close:da"));
}

#[test]
fn a_listener_that_reshows_the_dialog_keeps_it_open() {
    // `q-reshow-in-cancel`: the new watcher is not the one that the request
    // closes; each Escape fires `cancel` again.
    let mut h = Rec::new(DIALOGS);
    modal(&mut h, "d2").unwrap();
    h.cancel_hook = Some(|h, dialog| {
        close_dialog(h, dialog, None).unwrap();
        let _ = show_modal(h, dialog, None);
    });
    let _ = h.take_log();
    assert!(escape(&mut h));
    assert!(is_open(&h, "d2") && is_modal(&h, "d2"));
    assert!(escape(&mut h));
    assert!(is_open(&h, "d2") && is_modal(&h, "d2"));
    assert_eq!(h.take_log().matches("cancel:d2").count(), 2);
    invariants(&h.doc);
}

#[test]
fn a_listener_that_closes_another_dialog_does_not_stop_the_request() {
    // `q-close-other`: the watcher of the request is still its own.
    let mut h = Rec::new(DIALOGS);
    modal(&mut h, "d").unwrap();
    modal(&mut h, "d2").unwrap();
    h.cancel_hook = Some(|h, dialog| {
        if h.name(dialog) == "d2" {
            let d = h.id("d");
            close_dialog(h, d, None).unwrap();
        }
    });
    assert!(escape(&mut h));
    assert!(!is_open(&h, "d2"));
    assert!(!is_open(&h, "d"));
    invariants(&h.doc);
}

#[test]
fn a_canceled_cancel_stops_the_group() {
    let mut h = Rec::new(DIALOGS);
    modal(&mut h, "d").unwrap();
    modal(&mut h, "d2").unwrap();
    h.doc.note_user_activation();
    h.prevent_cancel = true;
    let _ = h.take_log();
    assert!(escape(&mut h));
    // `d2` stays, and the request does not go on to `d`.
    assert_eq!(h.take_log(), "cancel:d2!");
    assert!(is_open(&h, "d") && is_open(&h, "d2"));
}

#[test]
fn removing_open_keeps_a_modal_dialog_in_the_top_layer() {
    let mut h = Rec::new(DIALOGS);
    let d = h.id("d");
    modal(&mut h, "d").unwrap();
    let _ = h.take_log();
    h.doc.remove_attr(d, "open");
    // Chromium: still modal and in the top layer; the watcher is gone.
    assert!(is_modal(&h, "d"));
    assert_eq!(h.doc.top_layer().unwrap().layer(), [d]);
    assert!(!h.doc.has_close_watcher(d, WatcherKind::Dialog));
    assert_eq!(h.doc.open_dialogs(), []);
    // `close()` then does nothing.
    close(&mut h, "d", Some("x"));
    assert_eq!(h.take_log(), "");
    assert!(is_modal(&h, "d"));
    // Removing a non-modal dialog's attribute runs the cleanup steps.
    show(&mut h, "d2").unwrap();
    let d2 = h.id("d2");
    h.doc.remove_attr(d2, "open");
    assert!(!h.doc.element(d2).unwrap().is_dialog_open());
    invariants(&h.doc);
}

#[test]
fn removal_takes_a_dialog_out_of_the_top_layer() {
    let mut h = Rec::new(DIALOGS);
    let d = h.id("d");
    modal(&mut h, "d").unwrap();
    show(&mut h, "d2").unwrap();
    let d2 = h.id("d2");
    h.doc.remove(d);
    h.doc.remove(d2);
    assert!(!h.doc.element(d).unwrap().is_modal());
    assert_eq!(h.doc.top_layer().unwrap().layer(), []);
    assert_eq!(h.doc.open_dialogs(), []);
    assert_eq!(h.doc.close_watcher_counts(), (0, 0));
    // The `open` attribute stays (no events).
    assert!(h.doc.element(d).unwrap().has_attr("open"));
    invariants(&h.doc);
}

#[test]
fn the_dialog_focusing_steps() {
    let html = "<button id=ob>o</button><p id=px>x</p>\
        <dialog id=a1><input id=i1><button id=b1 autofocus>b</button></dialog>\
        <dialog id=a2 autofocus><input id=i2></dialog>\
        <dialog id=a3><div id=t3 tabindex=-1>t</div></dialog>\
        <dialog id=a4><p>no</p></dialog>";
    let mut h = Rec::new(html);
    // `tabindex` and the controls: the host decides; here every element of
    // the list below cannot take the focus.
    for id in ["a1", "a2", "a3", "a4"] {
        let el = h.id(id);
        h.unfocusable.push(el);
    }
    let a4 = h.id("a4");
    let para = h.doc.children(a4).next().unwrap();
    h.unfocusable.push(para);
    let ob = h.id("ob");
    h.focus = Some(ob);
    for (id, want) in [("a1", "b1"), ("a2", "i2"), ("a3", "t3"), ("a4", "a4")] {
        modal(&mut h, id).unwrap();
        assert_eq!(h.focus.map(|f| h.name(f)).as_deref(), Some(want), "{id}");
        // A modal dialog gives the focus back at its close, also when the
        // focus is outside it.
        let px = h.id("px");
        h.focus = Some(px);
        close(&mut h, id, None);
        assert_eq!(h.focus, Some(ob), "{id}");
    }
    // A non-modal one gives it back only if the focus is inside.
    show(&mut h, "a1").unwrap();
    h.focus = Some(ob);
    let _ = h.take_log();
    close(&mut h, "a1", None);
    assert!(!h.take_log().contains("focus:"));
}

#[test]
fn the_form_method_dialog() {
    let html = "<dialog id=d open><form id=f method=dialog><button id=s1 value=v1>a</button>\
        <button id=s2>b</button><input type=image id=s3><button id=s4 formmethod=get>g</button>\
        </form></dialog><form id=g method=DIALOG><button id=u value=u>x</button></form>";
    let h = Rec::new(html);
    let (d, f, g) = (h.id("d"), h.id("f"), h.id("g"));
    let at = |id: &str, coordinate| dialog_form_target(&h.doc, f, Some(h.id(id)), coordinate);
    assert_eq!(at("s1", None), Some(Some((d, Some("v1".to_owned())))));
    assert_eq!(at("s2", None), Some(Some((d, None))));
    assert_eq!(
        at("s3", Some((3, 4))),
        Some(Some((d, Some("3,4".to_owned()))))
    );
    assert_eq!(at("s3", None), Some(Some((d, Some("0,0".to_owned())))));
    assert_eq!(at("s4", None), None);
    // Without a submitter: the empty string (Chromium).
    assert_eq!(
        dialog_form_target(&h.doc, f, None, None),
        Some(Some((d, Some(String::new()))))
    );
    // Outside a dialog: nothing happens.
    assert_eq!(
        dialog_form_target(&h.doc, g, Some(h.id("u")), None),
        Some(None)
    );
}

#[test]
fn closedby_any_light_dismiss() {
    let mut h = Rec::new(DIALOGS);
    let (da, ob) = (h.id("da"), h.id("ob"));
    modal(&mut h, "da").unwrap();
    let _ = h.take_log();
    // A click inside the dialog keeps it.
    light_dismiss_dialogs_down(&mut h.doc, da, false);
    light_dismiss_dialogs_up(&mut h, da, false).unwrap();
    assert!(is_open(&h, "da"));
    // Press on the backdrop, release inside: nothing.
    light_dismiss_dialogs_down(&mut h.doc, da, true);
    light_dismiss_dialogs_up(&mut h, da, false).unwrap();
    assert!(is_open(&h, "da"));
    // A click on the backdrop requests the close (`cancel` cancelable).
    light_dismiss_dialogs_down(&mut h.doc, da, true);
    light_dismiss_dialogs_up(&mut h, da, true).unwrap();
    assert!(!is_open(&h, "da"));
    assert_eq!(h.doc.dialog_return_value(da), "");
    assert!(h.take_log().starts_with("cancel:da! bt:da(open>closed)"));
    // A non-modal dialog with `closedby=any`: a click outside it.
    show(&mut h, "da").unwrap();
    light_dismiss_dialogs_down(&mut h.doc, ob, false);
    light_dismiss_dialogs_up(&mut h, ob, false).unwrap();
    assert!(!is_open(&h, "da"));
    // `closedby=closerequest` stays.
    show(&mut h, "dc").unwrap();
    light_dismiss_dialogs_down(&mut h.doc, ob, false);
    light_dismiss_dialogs_up(&mut h, ob, false).unwrap();
    assert!(is_open(&h, "dc"));
    invariants(&h.doc);
}
