//! The user's input as DOM events (ADR 0028 section 11 and "Feature 6d2
//! notes"). With scripts on, each input of the user is a task of the user
//! interaction source: the page hit-tests, fires the events at the target,
//! and runs the default action only if no listener canceled the event. This
//! file has the events of the pointer, the mouse, the wheel, the keys and
//! the edits, and the frame that fires `scroll`; `input.rs` has the flow
//! of each input and calls these functions when
//! [`Page::script_input`] is true.
//!
//! Specifications: UI Events (<https://www.w3.org/TR/uievents/>: mouse,
//! wheel and keyboard events), Pointer Events
//! (<https://www.w3.org/TR/pointerevents/>), Input Events
//! (<https://www.w3.org/TR/input-events-2/>), the HTML focus steps
//! (<https://html.spec.whatwg.org/multipage/interaction.html#focus-update-steps>)
//! and "update the rendering"
//! (<https://html.spec.whatwg.org/multipage/webappapis.html#update-the-rendering>)
//! for `scroll`.
//!
//! The event orders and attributes are Chromium 148's, measured with
//! `tools/probes/web-input-real.json` (Playwright's mouse and keyboard
//! against the same steps through the automation API in swb).

use std::collections::HashMap;

use swb_dom::NodeId;
use swb_layout::Point;
use swb_web::{DispatchTarget, EventInterface, TrustedEvent, UserEvent, UserKind, modifiers};

use super::Page;
use crate::input::{KeyInfo, Modifiers, MouseButton};

/// The length of a frame in milliseconds of page time (60 Hz).
const FRAME_MS: f64 = 1000.0 / 60.0;

/// The pointer id of the mouse in Chromium's pointer events.
const MOUSE_POINTER_ID: i32 = 1;

/// The scroll offsets that `scroll` events were fired for, to find what
/// scrolled since the last frame.
#[derive(Clone, Debug, Default)]
pub(super) struct ScrollSeen {
    document: Point,
    elements: HashMap<NodeId, Point>,
}

/// The boxes of the elements for the layout and the scroll offsets they
/// were collected for.
#[derive(Clone, Debug, Default)]
pub(super) struct BoxCache {
    serial: u64,
    scroll: Point,
    offsets: HashMap<NodeId, Point>,
    boxes: HashMap<NodeId, swb_layout::Rect>,
}

/// The interface and flags of an event type of the user's input.
struct Spec {
    kind: UserKind,
    bubbles: bool,
    cancelable: bool,
    composed: bool,
}

/// The interface, `bubbles`, `cancelable` and `composed` of each event type
/// (measured; `wheel` is cancelable only with a blocking listener, which
/// the bindings decide).
fn spec(name: &str) -> Spec {
    let (kind, bubbles, cancelable, composed) = match name {
        "pointerenter" | "pointerleave" => (UserKind::Pointer, false, false, false),
        "mouseenter" | "mouseleave" => (UserKind::Mouse, false, false, false),
        "pointerover" | "pointerout" | "pointermove" | "pointerdown" | "pointerup" | "click"
        | "auxclick" | "contextmenu" => (UserKind::Pointer, true, true, true),
        "wheel" => (UserKind::Wheel, true, true, true),
        "keydown" | "keyup" | "keypress" => (UserKind::Keyboard, true, true, true),
        "beforeinput" => (UserKind::Input, true, true, true),
        "input" => (UserKind::Input, true, false, true),
        _ => (UserKind::Mouse, true, true, true),
    };
    Spec {
        kind,
        bubbles,
        cancelable,
        composed,
    }
}

/// `button` of a mouse button.
fn button_index(button: MouseButton) -> i16 {
    match button {
        MouseButton::Primary => 0,
        MouseButton::Middle => 1,
        MouseButton::Secondary => 2,
        MouseButton::Back => 3,
        MouseButton::Forward => 4,
    }
}

/// The bit of a mouse button in `buttons`.
fn button_bit(button: MouseButton) -> u16 {
    match button {
        MouseButton::Primary => 1,
        MouseButton::Secondary => 2,
        MouseButton::Middle => 4,
        MouseButton::Back => 8,
        MouseButton::Forward => 16,
    }
}

/// The modifier bits of an event.
pub(super) fn modifier_bits(m: Modifiers) -> u16 {
    let bit = |on: bool, bit: u16| if on { bit } else { 0 };
    bit(m.ctrl, modifiers::CONTROL)
        | bit(m.shift, modifiers::SHIFT)
        | bit(m.alt, modifiers::ALT)
        | bit(m.meta, modifiers::META)
}

/// The press of a mouse button that is in progress, for the events that
/// follow it.
#[derive(Clone, Copy, Debug)]
pub(super) struct Down {
    pub(super) button: MouseButton,
    /// The event target of the press.
    pub(super) target: NodeId,
    /// The click count (`detail`).
    pub(super) count: u32,
    /// False if the `pointerdown` was canceled: the compatibility mouse
    /// events are not fired for this press.
    pub(super) compat: bool,
}

impl Page {
    /// True if the user's input becomes DOM events: the document has a
    /// script runtime that is not busy and not stopped. After the time
    /// limits stopped the scripts, the input works as with JavaScript off.
    pub(super) fn script_input(&self) -> bool {
        self.script.as_ref().is_some_and(|s| !s.stopped()) && !self.lent
    }

    /// True if `node` is in the document.
    pub(super) fn is_connected(&self, node: NodeId) -> bool {
        self.document().is_some_and(|d| d.is_connected(node))
    }

    /// The event target at a point in viewport coordinates: the element of
    /// the hit node, else the root element (a point outside every box is on
    /// the canvas of the root element, as in Chromium).
    pub(super) fn target_at(&mut self, point: Point) -> Option<NodeId> {
        let hit = self.hit_test(point.x, point.y);
        let doc = self.doc.as_ref()?;
        hit.and_then(|h| doc.element_of(h.node))
            .or_else(|| doc.dom().element_children(NodeId::DOCUMENT).next())
    }

    /// The target of the key events: the focused element, else the body
    /// (the root element without one).
    pub(super) fn key_target(&self) -> Option<NodeId> {
        let doc = self.doc.as_ref()?;
        if let Some(focus) = doc.states.focus.filter(|&n| doc.dom().is_connected(n)) {
            return Some(focus);
        }
        let dom = doc.dom();
        let root = dom.element_children(NodeId::DOCUMENT).next()?;
        let body = dom.element_children(root).find(|&c| {
            dom.is_html_element(c, &swb_dom::local_name!("body"))
                || dom.is_html_element(c, &swb_dom::local_name!("frameset"))
        });
        Some(body.unwrap_or(root))
    }

    /// Fires `event` of type `kind` at `target` as a task. Returns false
    /// if a listener canceled it. A target that left the document gets
    /// nothing (true).
    fn fire(&mut self, kind: &str, target: NodeId, mut event: UserEvent) -> bool {
        if !self.is_connected(target) {
            return true;
        }
        let spec = spec(kind);
        event.kind = spec.kind;
        let trusted = TrustedEvent {
            kind: kind.to_owned(),
            bubbles: spec.bubbles,
            cancelable: spec.cancelable,
            composed: spec.composed,
            interface: EventInterface::User(Box::new(event)),
            legacy_target_override: false,
        };
        self.dispatch_event(DispatchTarget::Node(target), &trusted)
            .unwrap_or(true)
    }

    // ----- Mouse and pointer events -----

    /// The border box of `target` in document coordinates, for `offsetX`
    /// and `offsetY`. The boxes of the whole page are collected once for a
    /// layout and a scroll position, so a pointer that moves over a page of
    /// 100,000 elements does not walk it for each event.
    fn input_box(&mut self, target: NodeId) -> Option<swb_layout::Rect> {
        let doc = self.doc.as_deref_mut()?;
        doc.update_layout(&mut self.fonts);
        let current = doc.layout_serial;
        let fresh = self.input.boxes.as_ref().is_some_and(|c| {
            c.serial == current && c.scroll == doc.scroll && &c.offsets == doc.scrollers.offsets()
        });
        if !fresh {
            let boxes = doc
                .fragments
                .as_ref()?
                .element_boxes_scrolled(doc.scrollers.offsets(), doc.scroll);
            self.input.boxes = Some(BoxCache {
                serial: current,
                scroll: doc.scroll,
                offsets: doc.scrollers.offsets().clone(),
                boxes,
            });
        }
        self.input.boxes.as_ref()?.boxes.get(&target).copied()
    }

    /// The attributes of a mouse or pointer event of type `kind` at
    /// `target`, for the pointer at `point` (viewport coordinates).
    #[allow(clippy::too_many_arguments, reason = "the attributes of one event")]
    fn mouse_event(
        &mut self,
        kind: &str,
        target: NodeId,
        point: Point,
        related: Option<NodeId>,
        button: i16,
        detail: u32,
        movement: (f32, f32),
    ) -> UserEvent {
        let scroll = self.scroll_position();
        let offset = match self.input_box(target) {
            Some(r) => (
                f64::from(point.x + scroll.x - r.x),
                f64::from(point.y + scroll.y - r.y),
            ),
            None => (f64::from(point.x + scroll.x), f64::from(point.y + scroll.y)),
        };
        let pointer = kind.starts_with("pointer");
        let moving = matches!(
            kind,
            "pointerover"
                | "pointerout"
                | "pointerenter"
                | "pointerleave"
                | "pointermove"
                | "mouseover"
                | "mouseout"
                | "mouseenter"
                | "mouseleave"
                | "mousemove"
        );
        let buttons = self.input.buttons;
        let button = match (moving, pointer) {
            (true, true) => -1,
            (true, false) => 0,
            (false, _) => button,
        };
        // `isPrimary` is false for the click family; the pressure is 0.5
        // while a button is held, except for the events that end it.
        let click_family = matches!(kind, "click" | "auxclick" | "contextmenu");
        let pressure = if buttons != 0 && (moving || kind == "pointerdown") {
            0.5
        } else {
            0.0
        };
        UserEvent {
            client: (f64::from(point.x), f64::from(point.y)),
            offset,
            movement: (f64::from(movement.0), f64::from(movement.1)),
            button,
            buttons,
            detail: i32::try_from(detail).unwrap_or(i32::MAX),
            related_target: related,
            modifiers: modifier_bits(self.input.modifiers),
            pointer_id: MOUSE_POINTER_ID,
            primary: !click_family,
            pressure,
            ..UserEvent::default()
        }
    }

    /// Fires the mouse or pointer event `kind` at `target` for the pointer
    /// at `point`. Returns false if it was canceled.
    #[allow(clippy::too_many_arguments, reason = "the attributes of one event")]
    pub(super) fn fire_mouse(
        &mut self,
        kind: &str,
        target: NodeId,
        point: Point,
        related: Option<NodeId>,
        button: i16,
        detail: u32,
        movement: (f32, f32),
    ) -> bool {
        if !self.is_connected(target) {
            return true;
        }
        let event = self.mouse_event(kind, target, point, related, button, detail, movement);
        self.fire(kind, target, event)
    }

    /// The chains of nodes that the pointer leaves and enters when it moves
    /// from `old` to `new`: the leave chain innermost first, the enter
    /// chain outermost first. Both exclude the common inclusive ancestor.
    fn boundary_chains(&self, old: Option<NodeId>, new: NodeId) -> (Vec<NodeId>, Vec<NodeId>) {
        let Some(doc) = self.document() else {
            return (Vec::new(), Vec::new());
        };
        let path =
            |n: NodeId| -> Vec<NodeId> { std::iter::once(n).chain(doc.ancestors(n)).collect() };
        let new_path = path(new);
        let old_path = old.map(path).unwrap_or_default();
        let common = old_path.iter().find(|n| new_path.contains(n)).copied();
        let until = |p: &[NodeId]| -> Vec<NodeId> {
            p.iter()
                .copied()
                .take_while(|n| Some(*n) != common)
                .collect()
        };
        let leave = until(&old_path);
        let mut enter = until(&new_path);
        enter.reverse();
        (leave, enter)
    }

    /// The events of a pointer that moved from `old` to `new` (UI Events,
    /// "mouseover", "mouseout", "mouseenter", "mouseleave"
    /// <https://www.w3.org/TR/uievents/#events-mouse-types>; Pointer Events
    /// <https://www.w3.org/TR/pointerevents/#the-pointerover-event>): in
    /// Chromium all pointer events first, then the mouse
    /// events; each as `out`, `leave` of the old chain, `over`, `enter`
    /// of the new chain.
    fn boundary_events(&mut self, old: Option<NodeId>, new: NodeId, point: Point) {
        let (leave, enter) = self.boundary_chains(old, new);
        for prefix in ["pointer", "mouse"] {
            if let Some(old) = old {
                self.fire_mouse(
                    &format!("{prefix}out"),
                    old,
                    point,
                    Some(new),
                    0,
                    0,
                    (0.0, 0.0),
                );
                for &node in &leave {
                    let kind = format!("{prefix}leave");
                    self.fire_mouse(&kind, node, point, Some(new), 0, 0, (0.0, 0.0));
                }
            }
            self.fire_mouse(&format!("{prefix}over"), new, point, old, 0, 0, (0.0, 0.0));
            for &node in &enter {
                let kind = format!("{prefix}enter");
                self.fire_mouse(&kind, node, point, old, 0, 0, (0.0, 0.0));
            }
        }
    }

    /// The pointer moved to `point` (or pressed or wheeled there): the
    /// boundary events if the element under it changed, then `pointermove`
    /// and `mousemove`.
    pub(super) fn scripted_move(&mut self, point: Point) {
        let Some(new) = self.target_at(point) else {
            return;
        };
        let old = self.input.over.filter(|&n| self.is_connected(n));
        let previous = self.input.last_point.replace(point);
        self.input.over = Some(new);
        if old != Some(new) {
            self.boundary_events(old, new, point);
        }
        let movement = previous.map_or((0.0, 0.0), |p| (point.x - p.x, point.y - p.y));
        self.fire_mouse("pointermove", new, point, None, 0, 0, movement);
        self.fire_mouse("mousemove", new, point, None, 0, 0, movement);
    }

    /// The pointer left the page: `out` and `leave` for the element it was
    /// over and its ancestors, with no related target.
    pub(super) fn scripted_leave(&mut self) {
        let Some(old) = self.input.over.take() else {
            return;
        };
        let point = self.input.last_point.take().unwrap_or_default();
        if !self.is_connected(old) {
            return;
        }
        let Some(doc) = self.document() else {
            return;
        };
        let chain: Vec<NodeId> = std::iter::once(old).chain(doc.ancestors(old)).collect();
        for prefix in ["pointer", "mouse"] {
            self.fire_mouse(&format!("{prefix}out"), old, point, None, 0, 0, (0.0, 0.0));
            for &node in &chain {
                let kind = format!("{prefix}leave");
                self.fire_mouse(&kind, node, point, None, 0, 0, (0.0, 0.0));
            }
        }
    }

    /// A press of `button` at `point` (UI Events "mousedown", Pointer Events
    /// "pointerdown"; the compatibility mouse events,
    /// <https://www.w3.org/TR/pointerevents/#compatibility-mapping-with-mouse-events>):
    /// `pointerdown`, `mousedown` and, for
    /// the secondary button, `contextmenu`. Returns whether the default
    /// actions of the press (the focus, the selection) may run: no
    /// listener canceled `pointerdown` or `mousedown`.
    pub(super) fn scripted_down(&mut self, point: Point, button: MouseButton, count: u32) -> bool {
        if self.input.last_point != Some(point) {
            self.scripted_move(point);
        }
        let Some(target) = self.target_at(point) else {
            return true;
        };
        self.input.buttons |= button_bit(button);
        let index = button_index(button);
        let disabled = self.is_disabled_control(target);
        let mut allowed = self.fire_mouse("pointerdown", target, point, None, index, 0, (0.0, 0.0));
        let compat = allowed;
        self.input.down = Some(Down {
            button,
            target,
            count,
            compat,
        });
        if compat && !disabled {
            allowed &= self.fire_mouse("mousedown", target, point, None, index, count, (0.0, 0.0));
        }

        if button == MouseButton::Secondary && !disabled {
            // Chromium on Linux fires `contextmenu` when the button goes
            // down.
            self.fire_mouse("contextmenu", target, point, None, index, 0, (0.0, 0.0));
        }
        allowed
    }

    /// `selectstart` (Selection API) at the node under `point`, for a press
    /// that starts a selection: it focuses nothing, and the text is
    /// selectable. Returns false if a listener canceled it (no selection
    /// starts).
    pub(super) fn fire_selectstart(&mut self, point: Point) -> bool {
        let Some(hit) = self.hit_test(point.x, point.y) else {
            return true;
        };
        let Some(doc) = self.doc.as_ref() else {
            return true;
        };
        let selectable = doc
            .element_of(hit.node)
            .and_then(|e| doc.styles()?.get(e))
            .is_none_or(|style| style.user_select != swb_style::UserSelect::None);
        if doc.click_focus_target(hit.node).is_some() || !selectable {
            return true;
        }
        let mut event = TrustedEvent::new("selectstart");
        event.bubbles = true;
        event.cancelable = true;
        self.dispatch_event(DispatchTarget::Node(hit.node), &event)
            .unwrap_or(true)
    }

    /// The release of `button` at `point` (UI Events "mouseup", "click"
    /// <https://www.w3.org/TR/uievents/#event-type-click>; Pointer Events
    /// "pointerup"): `pointerup`, `mouseup`, and the
    /// click: `click` (primary) or `auxclick` at the nearest common
    /// ancestor of the targets of the press and the release, then
    /// `dblclick` for the second click. Returns the click target and
    /// whether the click event was not canceled; `None` if no click was
    /// fired.
    pub(super) fn scripted_up(
        &mut self,
        point: Point,
        button: MouseButton,
        drag: bool,
    ) -> Option<(NodeId, bool)> {
        if self.input.last_point != Some(point) {
            self.scripted_move(point);
        }
        let down = self.input.down.take().filter(|d| d.button == button);
        self.input.buttons &= !button_bit(button);
        let target = self.target_at(point)?;
        let index = button_index(button);
        let disabled = self.is_disabled_control(target);
        self.fire_mouse("pointerup", target, point, None, index, 0, (0.0, 0.0));
        let count = down.map_or(1, |d| d.count);
        if down.is_none_or(|d| d.compat) && !disabled {
            self.fire_mouse("mouseup", target, point, None, index, count, (0.0, 0.0));
        }
        let down = down?;
        if disabled || drag {
            return None;
        }
        // The click goes to the nearest common inclusive ancestor of the
        // two targets, if both are still in the document.
        if !self.is_connected(down.target) || !self.is_connected(target) {
            return None;
        }
        let click_target = self.common_ancestor(down.target, target)?;
        let kind = if button == MouseButton::Primary {
            "click"
        } else {
            "auxclick"
        };
        let proceed = self.fire_mouse(kind, click_target, point, None, index, count, (0.0, 0.0));
        if button == MouseButton::Primary && count == 2 {
            self.fire_mouse("dblclick", click_target, point, None, 0, count, (0.0, 0.0));
        }
        Some((click_target, proceed))
    }

    /// The nearest common inclusive ancestor of two nodes.
    fn common_ancestor(&self, a: NodeId, b: NodeId) -> Option<NodeId> {
        let doc = self.document()?;
        let b_path: Vec<NodeId> = std::iter::once(b).chain(doc.ancestors(b)).collect();
        std::iter::once(a)
            .chain(doc.ancestors(a))
            .find(|n| b_path.contains(n))
    }

    /// True if `node` is a form control that is disabled: it gets no mouse
    /// events but the pointer events (measured).
    fn is_disabled_control(&self, node: NodeId) -> bool {
        self.document().is_some_and(|d| {
            d.element(node).is_some_and(|e| {
                e.is_html()
                    && matches!(
                        &**e.local_name(),
                        "button" | "input" | "select" | "textarea"
                    )
            }) && swb_style::is_actually_disabled(d, node)
        })
    }

    /// `wheel` (UI Events <https://www.w3.org/TR/uievents/#event-type-wheel>)
    /// at `point` with the deltas in CSS px. Returns false if a
    /// listener canceled it (the page does not scroll).
    pub(super) fn scripted_wheel(&mut self, point: Point, dx: f32, dy: f32) -> bool {
        let Some(target) = self.target_at(point) else {
            return true;
        };
        let mut event = self.mouse_event("wheel", target, point, None, 0, 0, (0.0, 0.0));
        event.delta = (f64::from(dx), f64::from(dy));
        event.pointer_id = 0;
        self.fire("wheel", target, event)
    }

    // ----- Keyboard events -----

    /// Fires the keyboard event `kind` of `key` at `target` (UI Events
    /// <https://www.w3.org/TR/uievents/#keys>). Returns false
    /// if a listener canceled it.
    pub(super) fn fire_key(
        &mut self,
        kind: &str,
        target: NodeId,
        info: &KeyInfo,
        mods: Modifiers,
    ) -> bool {
        let keypress = kind == "keypress";
        let event = UserEvent {
            modifiers: modifier_bits(mods),
            key: info.key.clone(),
            code: info.code.clone(),
            key_code: if keypress {
                info.char_code
            } else {
                info.key_code
            },
            char_code: if keypress { info.char_code } else { 0 },
            ..UserEvent::default()
        };
        self.fire(kind, target, event)
    }

    /// Fires `beforeinput` (`before`) or `input` (Input Events
    /// <https://www.w3.org/TR/input-events-2/#x5-1-1-events-summary>) at the text control
    /// `target`. Returns false if a listener canceled `beforeinput`.
    pub(super) fn fire_input(
        &mut self,
        before: bool,
        target: NodeId,
        input_type: &str,
        data: Option<&str>,
    ) -> bool {
        let event = UserEvent {
            input_type: input_type.to_owned(),
            data: data.map(str::to_owned),
            modifiers: 0,
            ..UserEvent::default()
        };
        self.fire(if before { "beforeinput" } else { "input" }, target, event)
    }

    // ----- Focus -----

    /// The user's focus change with the focus events (a click, Tab; HTML
    /// "focus update steps"
    /// <https://html.spec.whatwg.org/multipage/interaction.html#focus-update-steps>): when
    /// the window gave up the focus (Tab left the page), it gets `focus`
    /// first. Returns true if the focus changed.
    pub(super) fn scripted_focus(&mut self, node: Option<NodeId>, visible: bool) -> bool {
        let old = self.focused_element();
        if node == old && !(node.is_some() && self.input.window_blurred) {
            return false;
        }
        if node.is_some() {
            self.regain_window_focus();
        }
        self.run_task(|s| s.user_focus(node, visible));
        let new = self.focused_element();
        if old != new {
            // A `keyup` of Space must not click what the focus left.
            self.input.space_down = None;
            self.focus_changed(old);
        }
        old != new
    }

    /// Gives the window the `focus` event after Tab left the page: the
    /// next focus change or click in the page brings the focus back.
    pub(super) fn regain_window_focus(&mut self) {
        if self.input.window_blurred {
            self.input.window_blurred = false;
            self.run_task(|s| s.window_focus(true));
        }
    }

    // ----- Scroll events -----

    /// Notes that something may have scrolled (`element`: the scroll
    /// container that the caller scrolled, if it is one): the frame fires
    /// `scroll` events for what did, in the order it scrolled (CSSOM View
    /// "pending scroll event targets"; the document recorded the scrolls of
    /// the viewport and of its scroll containers, also those of a script
    /// task, in `DocumentState::scrolled`).
    pub(super) fn note_scroll(&mut self, element: Option<NodeId>) {
        let scrolled = self
            .doc
            .as_deref_mut()
            .map(|d| std::mem::take(&mut d.scrolled))
            .unwrap_or_default();
        if self.script.is_none() {
            return;
        }
        for node in scrolled.into_iter().chain(element) {
            if !self.input.scroll_order.contains(&node) {
                self.input.scroll_order.push(node);
            }
        }
        if self.input.frame_due.is_none() {
            self.input.frame_due = Some(self.clock.now() + FRAME_MS);
        }
    }

    /// The page time of the next frame that has work, if any.
    pub(super) fn frame_time(&self) -> Option<f64> {
        self.input.frame_due
    }

    /// The rendering update of a frame that is due (HTML "update the
    /// rendering", <https://drafts.csswg.org/cssom-view/#run-the-scroll-steps>):
    /// fires `scroll` once at each target that scrolled since the last
    /// frame (the document for the viewport), in the order they scrolled
    /// (measured in Chromium 148: a container that a script scrolled before
    /// the viewport gets its event first; a scroll back to the old position
    /// still fires). Targets whose offsets changed without a note (a layout
    /// clamped them) follow, the document first, then in tree order.
    /// Returns true if an event was fired.
    pub(super) fn run_frame(&mut self) -> bool {
        let Some(due) = self.input.frame_due else {
            return false;
        };
        if due > self.clock.now() || !self.script_input() {
            return false;
        }
        self.input.frame_due = None;
        let Some(doc) = self.doc.as_deref() else {
            return false;
        };
        let seen = std::mem::take(&mut self.input.scroll_seen);
        let current = ScrollSeen {
            document: doc.scroll,
            elements: doc.scrollers.offsets().clone(),
        };
        let mut targets = std::mem::take(&mut self.input.scroll_order);
        if current.document != seen.document && !targets.contains(&NodeId::DOCUMENT) {
            targets.push(NodeId::DOCUMENT);
        }
        let unnoted: std::collections::HashSet<NodeId> = current
            .elements
            .iter()
            .filter(|(node, offset)| seen.elements.get(node) != Some(offset))
            .map(|(node, _)| *node)
            .chain(
                seen.elements
                    .keys()
                    .filter(|node| !current.elements.contains_key(node))
                    .copied(),
            )
            .filter(|node| !targets.contains(node))
            .collect();
        if !unnoted.is_empty() {
            let doc = self.document();
            targets.extend(doc.into_iter().flat_map(|d| {
                d.descendants(NodeId::DOCUMENT)
                    .filter(|n| unnoted.contains(n))
                    .collect::<Vec<_>>()
            }));
        }
        self.input.scroll_seen = current;
        let fired = !targets.is_empty();
        for node in targets {
            self.fire_scroll(node, node == NodeId::DOCUMENT);
        }
        fired
    }

    /// Fires `scroll` at `node` (it bubbles for the document only).
    fn fire_scroll(&mut self, node: NodeId, bubbles: bool) {
        if !self.is_connected(node) {
            return;
        }
        let mut event = TrustedEvent::new("scroll");
        event.bubbles = bubbles;
        self.dispatch_event(DispatchTarget::Node(node), &event);
    }
}
