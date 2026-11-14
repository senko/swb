#![allow(clippy::unwrap_used)]

use super::*;
use crate::{Document, parse_html};

/// What a hook does in a `beforetoggle` event.
type Hook = fn(&mut Rec, NodeId, ToggleState);

/// A host that records the events and the toggle tasks; a hook can run
/// re-entrant steps in each `beforetoggle` event.
struct Rec {
    doc: Document,
    log: Vec<String>,
    tasks: Vec<(NodeId, u64)>,
    cancel_shows: bool,
    hook: Option<Hook>,
    /// Events left before the host stops (a termination); `None`: never.
    stop_after: Option<usize>,
    /// The state of the pseudo-random hook.
    rng: u64,
    /// The elements that the hook picks from.
    nodes: Vec<NodeId>,
    /// The focused element, and the elements that cannot take the focus.
    focus: Option<NodeId>,
    unfocusable: Vec<NodeId>,
    /// A `cancel` listener cancels the event (when it is cancelable).
    prevent_cancel: bool,
    /// What a `cancel` listener does besides (it runs before the verdict).
    cancel_hook: Option<fn(&mut Rec, NodeId)>,
    /// The queued dialog `toggle` tasks and `close` tasks.
    dialog_tasks: Vec<(NodeId, u64)>,
    close_tasks: Vec<NodeId>,
}

impl Rec {
    fn new(html: &str) -> Rec {
        Rec {
            doc: parse_html(html),
            log: Vec::new(),
            tasks: Vec::new(),
            cancel_shows: false,
            hook: None,
            stop_after: None,
            rng: 0,
            nodes: Vec::new(),
            focus: None,
            unfocusable: Vec::new(),
            prevent_cancel: false,
            cancel_hook: None,
            dialog_tasks: Vec::new(),
            close_tasks: Vec::new(),
        }
    }

    fn id(&self, id: &str) -> NodeId {
        self.doc.element_by_id(id).unwrap()
    }

    fn name(&self, node: NodeId) -> String {
        self.doc
            .element(node)
            .and_then(crate::ElementData::id)
            .unwrap_or("?")
            .to_owned()
    }

    /// Runs the queued toggle tasks, in order, then the dialog toggle tasks
    /// and the `close` tasks.
    fn flush(&mut self) {
        for (element, serial) in std::mem::take(&mut self.tasks) {
            if let Some(t) = self.doc.take_toggle(element, serial) {
                let name = self.name(element);
                self.log.push(format!(
                    "toggle:{name}({}>{})",
                    t.old.as_str(),
                    t.new.as_str()
                ));
            }
        }
        for (element, serial) in std::mem::take(&mut self.dialog_tasks) {
            if let Some(t) = self.doc.take_dialog_toggle(element, serial) {
                let name = self.name(element);
                self.log.push(format!(
                    "dtoggle:{name}({}>{})",
                    t.old.as_str(),
                    t.new.as_str()
                ));
            }
        }
        for element in std::mem::take(&mut self.close_tasks) {
            let name = self.name(element);
            self.log.push(format!("close:{name}"));
        }
    }

    fn take_log(&mut self) -> String {
        self.flush();
        std::mem::take(&mut self.log).join(" ")
    }

    fn show(&mut self, id: &str) -> Result<(), PopoverError> {
        let el = self.id(id);
        show_popover(self, el, true, None)
    }

    fn hide(&mut self, id: &str) -> Result<(), PopoverError> {
        let el = self.id(id);
        hide_popover(self, el, HIDE)
    }

    /// The showing popovers in tree order.
    fn open(&self) -> String {
        let mut names: Vec<String> = self
            .doc
            .shadow_including_descendants(NodeId::DOCUMENT)
            .filter(|&n| self.doc.is_popover_showing(n))
            .map(|n| self.name(n))
            .collect();
        names.sort();
        names.join(",")
    }
}

const HIDE: HideOptions = HideOptions {
    focus_previous: true,
    fire_events: true,
    throw: true,
    ignore_dom: false,
    source: None,
};

impl TopLayerHost for Rec {
    fn doc(&mut self) -> &mut Document {
        &mut self.doc
    }

    fn fire_beforetoggle(
        &mut self,
        element: NodeId,
        old: ToggleState,
        new: ToggleState,
        cancelable: bool,
        _source: Option<NodeId>,
    ) -> Option<bool> {
        if let Some(left) = &mut self.stop_after {
            if *left == 0 {
                return None;
            }
            *left -= 1;
        }
        let name = self.name(element);
        let mark = if cancelable { "!" } else { "" };
        self.log.push(format!(
            "bt:{name}({}>{}){mark}",
            old.as_str(),
            new.as_str()
        ));
        if let Some(hook) = self.hook {
            hook(self, element, new);
        }
        Some(!(cancelable && self.cancel_shows))
    }

    fn queue_toggle_task(&mut self, element: NodeId, serial: u64) {
        self.tasks.push((element, serial));
    }

    fn focused_element(&mut self) -> Option<NodeId> {
        self.focus
    }

    fn focus(&mut self, element: NodeId) -> bool {
        if self.unfocusable.contains(&element) {
            return false;
        }
        self.focus = Some(element);
        let name = self.name(element);
        self.log.push(format!("focus:{name}"));
        true
    }

    fn focus_dialog(&mut self, dialog: NodeId) -> bool {
        self.focus = Some(dialog);
        let name = self.name(dialog);
        self.log.push(format!("focus:{name}"));
        true
    }

    fn fire_cancel(&mut self, dialog: NodeId, cancelable: bool) -> Option<bool> {
        let name = self.name(dialog);
        let mark = if cancelable { "!" } else { "" };
        self.log.push(format!("cancel:{name}{mark}"));
        if let Some(hook) = self.cancel_hook {
            hook(self, dialog);
        }
        Some(!(cancelable && self.prevent_cancel))
    }

    fn queue_dialog_toggle_task(&mut self, dialog: NodeId, serial: u64) {
        self.dialog_tasks.push((dialog, serial));
    }

    fn queue_close_task(&mut self, dialog: NodeId) {
        self.close_tasks.push(dialog);
    }
}

/// Checks that the lists, the top layer and the flags agree.
fn invariants(doc: &Document) {
    let Some(state) = doc.top_layer() else {
        return;
    };
    for (list, items) in [(List::Auto, &state.auto), (List::Hint, &state.hint)] {
        for (index, &n) in items.iter().enumerate() {
            assert_eq!(state.index_in(n, list), Some(index), "{n:?} in {list:?}");
            assert!(doc.is_popover_showing(n), "{n:?} in a list shows");
            assert!(state.layer.contains(&n), "{n:?} in a list is in the layer");
        }
    }
    // The kept blocking dialog is the one that a walk of the layer finds.
    assert_eq!(state.blocking(), state.blocking_scan(doc), "blocking");
    for &n in &state.layer {
        assert!(doc.element(n).unwrap().in_top_layer(), "{n:?} has its bit");
        assert_eq!(state.layer.iter().filter(|&&m| m == n).count(), 1);
    }
    // Each popover of a list has a close watcher, no other has one, and a
    // manual popover that shows has none.
    let listed = state.auto.len() + state.hint.len();
    assert_eq!(
        doc.close_watcher_counts().1,
        listed + state.open_dialogs.len(),
        "one watcher each"
    );
    for &n in &state.open_dialogs {
        assert!(
            doc.element(n).unwrap().is_dialog_open(),
            "{n:?} has its bit"
        );
        assert!(doc.has_close_watcher(n, WatcherKind::Dialog), "{n:?}");
    }
    for &n in state.auto.iter().chain(&state.hint) {
        assert!(doc.has_close_watcher(n, WatcherKind::Popover), "{n:?}");
    }
    for n in doc.shadow_including_descendants(NodeId::DOCUMENT) {
        if doc.is_popover_showing(n) {
            assert!(
                state.layer.contains(&n),
                "a showing popover is in the layer"
            );
        }
    }
}

const EVENTS: &str =
    "<div id=pa popover>a</div><div id=pb popover=manual>b</div><div id=nn>n</div>";

#[test]
fn show_and_hide_fire_beforetoggle_and_queue_toggle() {
    let mut h = Rec::new(EVENTS);
    h.show("pa").unwrap();
    assert!(h.doc.is_popover_showing(h.id("pa")));
    assert_eq!(h.doc.top_layer().unwrap().layer(), [h.id("pa")]);
    assert_eq!(h.take_log(), "bt:pa(closed>open)! toggle:pa(closed>open)");
    // A second show and a second hide do nothing.
    h.show("pa").unwrap();
    assert_eq!(h.take_log(), "");
    h.hide("pa").unwrap();
    assert!(!h.doc.is_popover_showing(h.id("pa")));
    assert_eq!(h.take_log(), "bt:pa(open>closed) toggle:pa(open>closed)");
    h.hide("pa").unwrap();
    assert_eq!(h.take_log(), "");
    invariants(&h.doc);
    // Nothing is kept for a hidden popover without a task.
    assert!(h.doc.top_layer().unwrap().popovers.is_empty());
}

#[test]
fn toggle_tasks_coalesce_and_move_to_the_end() {
    let mut h = Rec::new(EVENTS);
    h.show("pa").unwrap();
    h.hide("pa").unwrap();
    assert_eq!(
        h.take_log(),
        "bt:pa(closed>open)! bt:pa(open>closed) toggle:pa(closed>closed)"
    );
    h.show("pb").unwrap();
    h.hide("pb").unwrap();
    h.show("pb").unwrap();
    assert!(h.take_log().ends_with("toggle:pb(closed>open)"));
    h.hide("pb").unwrap();
    h.show("pb").unwrap();
    assert!(h.take_log().ends_with("toggle:pb(open>open)"));
    // `pb` (open) hides after `pa` shows; `pa` hides last: its task moves
    // behind the one of `pb`.
    h.show("pa").unwrap();
    h.hide("pb").unwrap();
    h.hide("pa").unwrap();
    assert!(
        h.take_log()
            .ends_with("toggle:pb(open>closed) toggle:pa(closed>closed)")
    );
    // A task whose serial was replaced does nothing.
    h.show("pa").unwrap();
    let (el, old_serial) = h.tasks[0];
    h.hide("pa").unwrap();
    assert_eq!(h.doc.take_toggle(el, old_serial), None);
}

#[test]
fn a_canceled_show_stays_closed() {
    let mut h = Rec::new(EVENTS);
    h.cancel_shows = true;
    h.show("pa").unwrap();
    assert!(!h.doc.is_popover_showing(h.id("pa")));
    assert_eq!(h.take_log(), "bt:pa(closed>open)!");
    let pa = h.id("pa");
    assert_eq!(toggle_popover(&mut h, pa, None, None), Ok(false));
    // The hide event cannot be canceled.
    h.cancel_shows = false;
    h.show("pa").unwrap();
    h.cancel_shows = true;
    h.hide("pa").unwrap();
    assert!(!h.doc.is_popover_showing(pa));
}

#[test]
fn errors_have_chromiums_texts() {
    let mut h = Rec::new(EVENTS);
    let not_popover = h.show("nn").unwrap_err();
    assert_eq!(not_popover.name(), "NotSupportedError");
    assert_eq!(
        not_popover.message(),
        "Not supported on elements that are not popovers."
    );
    let hide = h.hide("nn").unwrap_err();
    assert!(
        hide.message()
            .ends_with("changing the state of this popover.")
    );
    let pa = h.id("pa");
    h.doc.remove(pa);
    let disconnected = show_popover(&mut h, pa, true, None).unwrap_err();
    assert_eq!(disconnected.kind, PopoverErrorKind::Disconnected);
    assert_eq!(
        disconnected.message(),
        "Invalid on disconnected popover elements."
    );
    // A hide of a disconnected popover does nothing.
    assert_eq!(hide_popover(&mut h, pa, HIDE), Ok(()));
    // Template contents and a document without a browsing context.
    let mut t = Rec::new("<template id=t><div id=in popover>x</div></template>");
    let contents = t.doc.element(t.id("t")).unwrap().template_contents.unwrap();
    let inner = t.doc.first_element_child(contents).unwrap();
    let error = show_popover(&mut t, inner, true, None).unwrap_err();
    assert_eq!(error.kind, PopoverErrorKind::NotFullyActive);
    let other = t.doc.create_document().unwrap();
    let name = crate::QualName::new(None, crate::ns!(html), crate::LocalName::from("div"));
    let div = t
        .doc
        .create_node_in(
            other,
            crate::NodeData::Element(crate::ElementData::new(name, Vec::new())),
        )
        .unwrap();
    t.doc.set_attr(div, "popover", "").unwrap();
    t.doc.append_child(other, div).unwrap();
    assert_eq!(
        show_popover(&mut t, div, true, None).unwrap_err().kind,
        PopoverErrorKind::NotFullyActive
    );
    // Without `throw` the errors end the algorithm silently.
    assert_eq!(show_popover(&mut t, div, false, None), Ok(()));
    assert_eq!(t.take_log(), "");
}

/// Chromium (`popover-validity-6a`): `hidePopover()` and `togglePopover()`
/// of a popover in template contents or in a document without a browsing
/// context throw `InvalidStateError`, also when it is hidden; a hidden
/// popover of the page that is detached does not throw.
#[test]
fn a_hide_in_a_document_that_is_not_fully_active_throws() {
    let mut t =
        Rec::new("<template id=t><div id=in popover>x</div></template><div id=pa popover>a</div>");
    let contents = t.doc.element(t.id("t")).unwrap().template_contents.unwrap();
    let inner = t.doc.first_element_child(contents).unwrap();
    let error = hide_popover(&mut t, inner, HIDE).unwrap_err();
    assert_eq!(error.kind, PopoverErrorKind::NotFullyActive);
    assert_eq!(
        error.message(),
        "Invalid for popovers within documents that are not fully active. This might have been \
         the result of the \"beforetoggle\" event handler changing the state of this popover."
    );
    let error = toggle_popover(&mut t, inner, None, None).unwrap_err();
    assert_eq!(error.kind, PopoverErrorKind::NotFullyActive);
    assert!(!error.after_event);
    let error = toggle_popover(&mut t, inner, Some(false), None).unwrap_err();
    assert_eq!(error.kind, PopoverErrorKind::NotFullyActive);
    // Without `throw` (the steps of a removal) the hide ends silently.
    let silent = HideOptions {
        throw: false,
        ..HIDE
    };
    assert_eq!(hide_popover(&mut t, inner, silent), Ok(()));
    // A detached element of the page's document: a hidden popover is not
    // hidden again, without an error.
    let pa = t.id("pa");
    t.doc.remove(pa);
    assert_eq!(hide_popover(&mut t, pa, HIDE), Ok(()));
    assert_eq!(t.take_log(), "");
}

/// An open non-modal dialog shows as a popover (Chromium, measured
/// `popover-validity-6a` #1); the `open` attribute alone does not stop it.
#[test]
fn an_open_dialog_shows_as_a_popover() {
    let mut h = Rec::new("<dialog id=d open popover>x</dialog>");
    h.show("d").unwrap();
    assert_eq!(h.open(), "d");
    assert!(h.doc.element(h.id("d")).unwrap().has_attr("open"));
}

#[test]
fn removal_in_beforetoggle_throws_with_the_event_sentence() {
    let mut h = Rec::new(EVENTS);
    h.hook = Some(|h, el, new| {
        if new == ToggleState::Open {
            h.doc.remove(el);
        }
    });
    let error = h.show("pa").unwrap_err();
    assert_eq!(error.kind, PopoverErrorKind::Disconnected);
    assert!(error.after_event);
    assert!(error.message().starts_with(
        "Invalid on disconnected popover elements. This might have been the result of the \
         \"beforetoggle\" event handler"
    ));
    invariants(&h.doc);
}

#[test]
fn a_type_change_in_beforetoggle_opens_as_the_new_type() {
    let mut h = Rec::new("<div id=a popover>a</div><div id=b popover>b</div>");
    h.show("b").unwrap();
    h.hook = Some(|h, el, new| {
        if new == ToggleState::Open {
            h.doc.set_attr(el, "popover", "manual").unwrap();
        }
    });
    h.show("a").unwrap();
    // `a` opened as manual: `b` stays open.
    assert_eq!(h.open(), "a,b");
    assert_eq!(h.doc.showing_popover_lists().0, [h.id("b")]);
}

#[test]
fn a_hide_in_the_show_event_does_nothing() {
    let mut h = Rec::new(EVENTS);
    h.hook = Some(|h, el, new| {
        if new == ToggleState::Open {
            assert_eq!(hide_popover(h, el, HIDE), Ok(()));
        }
    });
    h.show("pa").unwrap();
    assert_eq!(h.open(), "pa");
}

#[test]
fn a_show_inside_its_own_event_opens_once() {
    let mut h = Rec::new(EVENTS);
    h.hook = Some(|h, el, _| {
        if h.log.len() < 4 {
            assert_eq!(show_popover(h, el, true, None), Ok(()));
        }
    });
    h.show("pa").unwrap();
    assert_eq!(
        h.take_log(),
        "bt:pa(closed>open)! bt:pa(closed>open)! bt:pa(closed>open)! bt:pa(closed>open)! \
         toggle:pa(closed>open)"
    );
    assert_eq!(h.doc.showing_popover_lists().0, [h.id("pa")]);
    invariants(&h.doc);
}

#[test]
fn another_popover_shown_in_the_event_is_hidden_by_the_outer_show() {
    let mut h = Rec::new("<div id=x popover>x</div><div id=y popover>y</div>");
    h.hook = Some(|h, el, new| {
        if new == ToggleState::Open && h.name(el) == "x" {
            h.show("y").unwrap();
        }
    });
    h.show("x").unwrap();
    assert_eq!(
        h.take_log(),
        "bt:x(closed>open)! bt:y(closed>open)! bt:y(open>closed) toggle:y(closed>closed) \
         toggle:x(closed>open)"
    );
    assert_eq!(h.open(), "x");
}

#[test]
fn a_show_in_the_hide_event_lets_the_hide_go_on() {
    let mut h = Rec::new(EVENTS);
    h.show("pa").unwrap();
    h.hook = Some(|h, el, new| {
        if new == ToggleState::Closed {
            assert_eq!(show_popover(h, el, true, None), Ok(()));
        }
    });
    h.hide("pa").unwrap();
    assert_eq!(h.open(), "");
    assert_eq!(
        h.take_log(),
        "bt:pa(closed>open)! bt:pa(open>closed) toggle:pa(closed>closed)"
    );
}

const STACK: &str = "<div id=p1 popover>1<div id=p2 popover>2<div id=p3 popover>3</div></div></div>\
    <div id=q1 popover>q</div><div id=m1 popover=manual>m</div>";

#[test]
fn nested_auto_popovers_stay_open_and_close_from_the_top() {
    let mut h = Rec::new(STACK);
    for id in ["p1", "p2", "p3", "m1"] {
        h.show(id).unwrap();
    }
    assert_eq!(h.open(), "m1,p1,p2,p3");
    let _ = h.take_log();
    h.show("q1").unwrap();
    assert_eq!(
        h.take_log(),
        "bt:q1(closed>open)! bt:p3(open>closed) bt:p2(open>closed) bt:p1(open>closed) \
         toggle:p3(open>closed) toggle:p2(open>closed) toggle:p1(open>closed) \
         toggle:q1(closed>open)"
    );
    assert_eq!(h.open(), "m1,q1");
    invariants(&h.doc);
    h.hide("q1").unwrap();
    for id in ["p1", "p2", "p3"] {
        h.show(id).unwrap();
    }
    let _ = h.take_log();
    h.hide("p2").unwrap();
    assert_eq!(
        h.take_log(),
        "bt:p3(open>closed) bt:p2(open>closed) toggle:p3(open>closed) toggle:p2(open>closed)"
    );
    assert_eq!(h.open(), "m1,p1");
    // A nested popover whose parent is closed closes the others.
    h.hide("p1").unwrap();
    h.show("p3").unwrap();
    h.show("p2").unwrap();
    assert_eq!(h.open(), "m1,p2");
    // A popover inside an open manual popover is not nested in it.
    let (m1, q1) = (h.id("m1"), h.id("q1"));
    h.hide("p2").unwrap();
    h.doc.append_child(m1, q1).unwrap();
    h.show("q1").unwrap();
    h.show("p1").unwrap();
    assert_eq!(h.open(), "m1,p1");
    invariants(&h.doc);
}

#[test]
fn a_source_nests_like_an_invoker() {
    let mut h = Rec::new(
        "<div id=p1 popover>1<button id=b2>b2</button></div><div id=p2 popover>2</div>\
         <div id=p3 popover>3</div>",
    );
    let (p2, b2) = (h.id("p2"), h.id("b2"));
    h.show("p1").unwrap();
    show_popover(&mut h, p2, true, Some(b2)).unwrap();
    assert_eq!(h.open(), "p1,p2");
    assert_eq!(h.doc.popover_invoker(p2), Some(b2));
    // `b2` is inside `p1`: a show with it as source nests on `p1`.
    assert_eq!(
        topmost_popover_ancestor(&h.doc, h.id("p3"), Some(b2)),
        Some(h.id("p1"))
    );
    h.show("p3").unwrap();
    assert_eq!(h.open(), "p3");
    // Without a source the first one closes.
    h.show("p1").unwrap();
    h.show("p2").unwrap();
    assert_eq!(h.open(), "p2");
    // `togglePopover` with a source and `force`.
    h.hide("p2").unwrap();
    h.show("p1").unwrap();
    assert_eq!(toggle_popover(&mut h, p2, Some(true), Some(b2)), Ok(true));
    assert_eq!(h.open(), "p1,p2");
    invariants(&h.doc);
}

const HINTS: &str = "<div id=a1 popover>a1<div id=h2 popover=hint>h2</div></div>\
    <div id=h1 popover=hint>h1</div><div id=h3 popover=hint>h3</div><div id=m1 popover=manual>m</div>";

#[test]
fn hint_popovers_follow_chromium() {
    let mut h = Rec::new(HINTS);
    // A new hint closes an unrelated open hint.
    h.show("h1").unwrap();
    h.show("h3").unwrap();
    assert_eq!(h.open(), "h3");
    // An auto popover closes the open hints.
    h.hide("h3").unwrap();
    h.show("h1").unwrap();
    h.show("a1").unwrap();
    assert_eq!(h.open(), "a1");
    // A hint inside an open auto popover stays open, also when another hint
    // opens after it.
    h.show("h2").unwrap();
    h.show("h1").unwrap();
    assert_eq!(h.open(), "a1,h1,h2");
    invariants(&h.doc);
    // Hiding an auto popover closes the open hints first.
    h.hide("h1").unwrap();
    h.hide("h2").unwrap();
    h.show("h1").unwrap();
    h.show("m1").unwrap();
    let _ = h.take_log();
    h.hide("a1").unwrap();
    assert_eq!(
        h.take_log(),
        "bt:h1(open>closed) bt:a1(open>closed) toggle:h1(open>closed) toggle:a1(open>closed)"
    );
    assert_eq!(h.open(), "m1");
    invariants(&h.doc);
}

#[test]
fn hide_all_popovers_until_keeps_the_endpoint_and_below() {
    let mut h = Rec::new(STACK);
    for id in ["p1", "p2", "p3"] {
        h.show(id).unwrap();
    }
    let p1 = h.id("p1");
    hide_all_popovers_until(&mut h, Some(p1), true).unwrap();
    assert_eq!(h.open(), "p1");
    hide_all_popovers_until(&mut h, None, true).unwrap();
    assert_eq!(h.open(), "");
    invariants(&h.doc);
}

#[test]
fn removal_hides_without_events_and_keeps_the_pending_task() {
    let mut h = Rec::new(
        "<section id=w><div id=r1 popover>r<div id=r2 popover>r2</div></div></section>\
         <div id=nn>n</div>",
    );
    h.show("r1").unwrap();
    h.show("r2").unwrap();
    h.log.clear();
    let (w, nn, r1) = (h.id("w"), h.id("nn"), h.id("r1"));
    // A move (an insertion elsewhere) removes first.
    h.doc.append_child(nn, w).unwrap();
    assert_eq!(h.open(), "");
    // No `beforetoggle`; the toggle tasks of the shows still fire.
    assert_eq!(
        h.take_log(),
        "toggle:r1(closed>open) toggle:r2(closed>open)"
    );
    invariants(&h.doc);
    assert_eq!(h.doc.top_layer().unwrap().layer(), &[] as &[NodeId]);
    // Adoption into another document of the arena.
    h.show("r1").unwrap();
    let other = h.doc.create_document().unwrap();
    h.doc.adopt(r1, other).unwrap();
    assert!(!h.doc.is_popover_showing(r1));
    invariants(&h.doc);
}

#[test]
fn removal_inside_a_shadow_tree() {
    let mut h = Rec::new("<div id=host></div>");
    let host = h.id("host");
    let init = crate::ShadowRootInit {
        mode: crate::ShadowRootMode::Open,
        slot_assignment: crate::SlotAssignment::Named,
        delegates_focus: false,
        clonable: false,
        serializable: false,
    };
    let root = h.doc.attach_shadow(host, init, false).unwrap();
    let name = crate::QualName::new(None, crate::ns!(html), crate::LocalName::from("div"));
    let p = h.doc.create_element(name, Vec::new()).unwrap();
    h.doc.set_attr(p, "popover", "manual").unwrap();
    h.doc.append_child(root, p).unwrap();
    show_popover(&mut h, p, true, None).unwrap();
    assert!(h.doc.is_popover_showing(p));
    h.doc.remove(host);
    assert!(!h.doc.is_popover_showing(p));
    invariants(&h.doc);
}

#[test]
fn attribute_changes_between_states_hide_with_events() {
    let mut h = Rec::new(EVENTS);
    let pa = h.id("pa");
    h.show("pa").unwrap();
    h.flush();
    h.log.clear();
    // The same state in another spelling keeps it open.
    h.doc.set_attr(pa, "popover", "AUTO").unwrap();
    h.doc.set_attr(pa, "popover", "").unwrap();
    assert!(!h.doc.has_popover_attribute_hides());
    h.doc.set_attr(pa, "popover", "bogus").unwrap();
    assert!(h.doc.has_popover_attribute_hides());
    // The flag stays until the steps run.
    assert!(h.doc.is_popover_showing(pa));
    run_popover_attribute_steps(&mut h).unwrap();
    assert!(!h.doc.is_popover_showing(pa));
    assert_eq!(h.take_log(), "bt:pa(open>closed) toggle:pa(open>closed)");
    // A removed attribute: the hide runs although the element is no popover.
    h.doc.set_attr(pa, "popover", "manual").unwrap();
    h.show("pa").unwrap();
    assert!(h.doc.remove_attr(pa, "popover"));
    run_popover_attribute_steps(&mut h).unwrap();
    assert!(!h.doc.is_popover_showing(pa));
    invariants(&h.doc);
}

#[test]
fn a_list_member_without_its_attribute_is_still_hidden() {
    let mut h = Rec::new(STACK);
    h.show("p1").unwrap();
    h.show("p2").unwrap();
    let p2 = h.id("p2");
    // The attribute change steps have not run yet.
    assert!(h.doc.remove_attr(p2, "popover"));
    h.show("q1").unwrap();
    assert_eq!(h.open(), "q1");
    invariants(&h.doc);
}

#[test]
fn toggle_popover_returns_the_new_state() {
    let mut h = Rec::new(EVENTS);
    let pa = h.id("pa");
    let results: Vec<bool> = [None, None, Some(true), Some(true), Some(false), Some(false)]
        .into_iter()
        .map(|force| toggle_popover(&mut h, pa, force, None).unwrap())
        .collect();
    assert_eq!(results, [true, false, true, true, false, false]);
    let nn = h.id("nn");
    let error = toggle_popover(&mut h, nn, Some(false), None).unwrap_err();
    assert_eq!(error.kind, PopoverErrorKind::NotPopover);
    assert!(!error.after_event);
}

#[test]
fn invoker_actions() {
    let mut h = Rec::new(
        "<button id=b popovertarget=t>b</button><div id=t popover=manual>t<button id=in>i</button></div>",
    );
    let (b, t, inner) = (h.id("b"), h.id("t"), h.id("in"));
    assert_eq!(popover_target_by_id(&h.doc, b), Some(t));
    let act = |h: &mut Rec, action, target| {
        popover_target_activation(h, b, t, (action, target)).unwrap();
        h.doc.is_popover_showing(t)
    };
    assert!(act(&mut h, TargetAction::Toggle, b));
    assert!(act(&mut h, TargetAction::Show, b));
    assert!(!act(&mut h, TargetAction::Toggle, b));
    assert!(!act(&mut h, TargetAction::Hide, b));
    // A popover inside its invoker: a click inside the popover does nothing.
    h.doc.append_child(b, t).unwrap();
    assert!(!act(&mut h, TargetAction::Toggle, inner));
    assert!(act(&mut h, TargetAction::Toggle, b));
    assert_eq!(TargetAction::of(Some("SHOW")), TargetAction::Show);
    assert_eq!(TargetAction::of(Some("bogus")), TargetAction::Toggle);
    assert_eq!(TargetAction::of(None), TargetAction::Toggle);
}

#[test]
fn attribute_states() {
    use PopoverState::{Auto, Hint, Manual};
    let states: Vec<Option<PopoverState>> = [
        None,
        Some(""),
        Some("AUTO"),
        Some("Manual"),
        Some("hint"),
        Some("bogus"),
        Some(" auto"),
    ]
    .into_iter()
    .map(PopoverState::of)
    .collect();
    assert_eq!(
        states,
        [
            None,
            Some(Auto),
            Some(Auto),
            Some(Manual),
            Some(Hint),
            Some(Manual),
            Some(Manual)
        ]
    );
}

/// A chain of nested auto popovers shown in order: each show finds its
/// ancestor in one step (ADR 0037 section 2), so the chain is linear.
#[test]
fn a_long_chain_of_nested_popovers_shows_in_linear_time() {
    let mut h = Rec::new("<body>");
    let mut parent = h.doc.body().unwrap();
    let mut chain = Vec::new();
    for _ in 0..10_000 {
        let name = crate::QualName::new(None, crate::ns!(html), crate::LocalName::from("div"));
        let p = h.doc.create_element(name, Vec::new()).unwrap();
        h.doc.set_attr(p, "popover", "").unwrap();
        h.doc.append_child(parent, p).unwrap();
        chain.push(p);
        parent = p;
    }
    let started = std::time::Instant::now();
    for &p in &chain {
        show_popover(&mut h, p, true, None).unwrap();
    }
    assert_eq!(h.doc.showing_popover_lists().0.len(), 10_000);
    assert!(started.elapsed().as_secs() < 5, "{:?}", started.elapsed());
    h.tasks.clear();
    // The bottom one hides them all, top first.
    hide_popover(&mut h, chain[0], HIDE).unwrap();
    assert_eq!(h.doc.showing_popover_lists().0.len(), 0);
    assert!(h.log.last().unwrap().starts_with("bt:?(open>closed)"));
}

#[test]
fn a_stop_ends_the_algorithms() {
    let mut h = Rec::new(STACK);
    for id in ["p1", "p2", "p3"] {
        h.show(id).unwrap();
    }
    h.stop_after = Some(1);
    let error = h.show("q1").unwrap_err();
    assert_eq!(error.kind, PopoverErrorKind::Stopped);
    invariants(&h.doc);
    // Without `throw` a stop still ends the caller.
    let q1 = h.id("q1");
    h.stop_after = Some(0);
    assert_eq!(
        show_popover(&mut h, q1, false, None).unwrap_err().kind,
        PopoverErrorKind::Stopped
    );
}

/// A hook that changes the DOM in each event: removals, moves, attribute
/// changes, shows and hides of other popovers, from a fixed pseudo-random
/// sequence. The state stays consistent and every call ends.
#[test]
fn re_entrant_hosts_keep_the_state_consistent() {
    fn chaos(h: &mut Rec, el: NodeId, _new: ToggleState) {
        if h.log.len() > 4_000 {
            return;
        }
        h.rng = h
            .rng
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let roll = (h.rng >> 33) % 97;
        let other = h.nodes[(roll % 8) as usize];
        let body = h.doc.body().unwrap();
        match roll % 9 {
            0 => h.doc.remove(el),
            1 => {
                let _ = h.doc.append_child(other, el);
            }
            2 => {
                let _ = h.doc.set_attr(el, "popover", "manual");
            }
            3 => {
                let _ = h.doc.set_attr(other, "popover", "hint");
            }
            4 => {
                let _ = show_popover(h, other, false, None);
            }
            5 => {
                let _ = hide_popover(h, other, HIDE);
            }
            6 => {
                let _ = h.doc.append_child(body, other);
            }
            7 => {
                let _ = run_popover_attribute_steps(h);
            }
            _ => {
                let _ = h.doc.set_attr(other, "popover", "");
            }
        }
        invariants(&h.doc);
    }
    for seed in 0..40_usize {
        let mut h = Rec::new(
            "<div id=c0 popover>0<div id=c1 popover>1<div id=c2 popover=hint>2</div></div></div>\
             <div id=c3 popover=hint>3<div id=c4 popover>4</div></div><div id=c5 popover=manual>5\
             <div id=c6 popover>6</div></div><div id=c7 popover>7</div>",
        );
        h.rng = seed as u64;
        h.nodes = ["c0", "c1", "c2", "c3", "c4", "c5", "c6", "c7"]
            .iter()
            .map(|id| h.id(id))
            .collect();
        h.hook = Some(chaos);
        for round in 0..30 {
            let el = h.nodes[(round * 3 + seed) % 8];
            let _ = match round % 3 {
                0 => show_popover(&mut h, el, true, None),
                1 => hide_popover(&mut h, el, HIDE),
                _ => toggle_popover(&mut h, el, None, None).map(|_| ()),
            };
            let _ = run_popover_attribute_steps(&mut h);
            invariants(&h.doc);
            h.flush();
        }
    }
}

#[test]
fn flag_bits_are_distinct_and_the_element_keeps_its_size() {
    let bits = [
        flags::HOST,
        flags::SLOT,
        flags::POPOVER_SHOWING,
        flags::MODAL,
        flags::TOP_LAYER,
        flags::DIALOG_OPEN,
    ];
    for (i, a) in bits.iter().enumerate() {
        assert_eq!(a.count_ones(), 1);
        for b in &bits[i + 1..] {
            assert_eq!(a & b, 0);
        }
    }
    #[cfg(target_pointer_width = "64")]
    assert_eq!(size_of::<crate::tree::ElementData>(), 64);
}

#[test]
fn the_journal_notes_top_layer_changes() {
    let mut h = Rec::new(EVENTS);
    let _ = h.doc.take_journal();
    h.show("pa").unwrap();
    let pa = h.id("pa");
    let changes: Vec<Change<'_>> = h.doc.journal().iter().collect();
    assert_eq!(changes, [Change::TopLayer { element: pa }]);
}

mod dialog;
mod interaction;
