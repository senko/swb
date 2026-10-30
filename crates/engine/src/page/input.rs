//! Input handling of a page: the pointer (hover, `:active`, clicks, text
//! selection), the keyboard (focus navigation, scrolling, activation), and
//! the element states that depend on them. The element states and the
//! selection belong to the document (`crate::document`); the pointer and
//! the press are the page's.

use swb_dom::NodeId;
use swb_layout::{Point, Size};
use swb_style::{Cursor, ElementStates, UserSelect};

use super::Page;
use super::events::Down;
use super::frame::{ObservedState, ScrollSeen};
use crate::document::is_rendered_in;
use crate::focus;
use crate::hit_test::{self, HitResult};
use crate::input::{Key, Modifiers, MouseButton};
use crate::selection::{self, Selection, Snap, TextPosition};

/// How far (in CSS px) the pointer must move with the button held before a
/// press becomes a drag that selects text. Chromium uses 4 px.
const DRAG_THRESHOLD: f32 = 4.0;

/// Pixels scrolled by an arrow key (as in Chromium).
const ARROW_SCROLL: f32 = 40.0;

/// A press of the primary button.
#[derive(Clone, Debug)]
struct Press {
    /// Where the press started, in viewport coordinates.
    origin: Point,
    /// The element whose activation behavior a click would run (a link, a
    /// button, a label; see `Page::activation_target`).
    target: Option<NodeId>,
    /// Where a drag selection starts. `None` after a double or triple
    /// click, whose selection a drag does not change.
    anchor: Option<TextPosition>,
    /// The text control in which a drag selects text.
    control: Option<NodeId>,
    /// True once the pointer moved beyond the drag threshold.
    dragging: bool,
}

/// The pointer and keyboard state of a page. Reset for each document,
/// except the pointer position.
#[derive(Clone, Debug, Default)]
pub(super) struct InputState {
    /// The pointer position in viewport coordinates, while it is over the
    /// page.
    pub(super) pointer: Option<Point>,
    press: Option<Press>,
    hovered_link: Option<swb_net::Url>,
    cursor: Cursor,
    /// Where sequential focus navigation starts when no element is
    /// focused: the node of the last click or the target of the last
    /// fragment navigation.
    focus_start: Option<NodeId>,
    /// The node of the last press of the primary button. Without a focused
    /// element, keyboard scrolling starts at its scroll container.
    last_press: Option<NodeId>,
    // ----- Events of the user's input (scripts on; `events.rs`) -----
    /// The element that the pointer is over, as the events saw it last.
    pub(super) over: Option<NodeId>,
    /// The mouse buttons held (`buttons`).
    pub(super) buttons: u16,
    /// The modifier keys of the last input.
    pub(super) modifiers: Modifiers,
    /// Where the last pointer event was (for `movementX`).
    pub(super) last_point: Option<Point>,
    /// The press in progress, for the events that follow it.
    pub(super) down: Option<Down>,
    /// True after Tab left the page: the window lost the focus.
    pub(super) window_blurred: bool,
    /// The element on which Space went down; it is clicked on release.
    pub(super) space_down: Option<NodeId>,
    /// The page time of the next frame that has work other than animation
    /// frame callbacks (`scroll` and `resize` events, media queries).
    pub(super) frame_due: Option<f64>,
    /// The page time of the last frame.
    pub(super) last_frame: Option<f64>,
    /// The page time at which the next idle period can start (the end of
    /// the last one; `idle.rs`).
    pub(super) idle_next: f64,
    /// The idle periods since the last task or network completion.
    pub(super) idle_streak: u32,
    /// The viewport changed its size since the last frame: the frame fires
    /// `resize`.
    pub(super) resized: bool,
    /// The scroll offsets that `scroll` events were last fired for.
    pub(super) scroll_seen: ScrollSeen,
    /// What the intersection and resize observers saw at the last frame.
    pub(super) observed: Option<ObservedState>,
    /// The scroll containers in the order they scrolled since the last
    /// frame: the `scroll` events follow this order (CSSOM View "run the
    /// scroll steps": the order the targets were added; measured).
    pub(super) scroll_order: Vec<NodeId>,
    /// The boxes of the elements for `offsetX` and `offsetY`, kept while
    /// the layout and the scroll offsets stay the same (`events.rs`).
    pub(super) boxes: Option<super::events::BoxCache>,
}

impl InputState {
    /// The state for a new document: only the pointer position stays.
    pub(super) fn for_new_document(&self) -> InputState {
        InputState {
            pointer: self.pointer,
            ..InputState::default()
        }
    }
}

impl Page {
    /// Hit-tests a point in viewport coordinates (CSS px).
    pub fn hit_test(&mut self, x: f32, y: f32) -> Option<HitResult> {
        let (doc, fonts) = self.parts()?;
        doc.hit_test(x, y, fonts)
    }

    /// The generation of the DOM of the current document (0 without one):
    /// it grows with every change, so a caller can tell that input events
    /// changed the page.
    pub(super) fn dom_generation(&self) -> u64 {
        self.doc.as_ref().map_or(0, |d| d.dom().generation())
    }

    /// The position of the mouse pointer in viewport coordinates, while it
    /// is over the page.
    pub fn pointer_position(&self) -> Option<Point> {
        self.input.pointer
    }

    /// The URL of the link under the mouse pointer.
    pub fn hovered_link(&self) -> Option<&swb_net::Url> {
        self.input.hovered_link.as_ref()
    }

    /// The mouse cursor for the pointer position (never `auto`).
    pub fn cursor(&self) -> Cursor {
        match self.input.cursor {
            Cursor::Auto => Cursor::Default,
            cursor => cursor,
        }
    }

    /// The focused element.
    pub fn focused_element(&self) -> Option<NodeId> {
        self.doc.as_ref()?.states.focus
    }

    // ----- Selection -----

    /// The text selection, if there is one and it is not empty.
    pub fn selection(&self) -> Option<Selection> {
        self.doc.as_ref()?.visible_selection()
    }

    /// The selected text, with white space collapsed and line breaks
    /// between blocks (as `innerText`). Empty without a selection. While a
    /// text control has the focus, the text selected in it (nothing for a
    /// password field).
    pub fn selected_text(&mut self) -> String {
        self.parts()
            .map(|(doc, fonts)| doc.selected_text(fonts))
            .unwrap_or_default()
    }

    /// The rendered text of an element (or text node), as `innerText`:
    /// collapsed white space, line breaks between blocks. Empty if the
    /// node has no rendered text.
    pub fn element_text(&mut self, node: NodeId) -> String {
        self.parts()
            .map(|(doc, fonts)| doc.element_text(node, fonts))
            .unwrap_or_default()
    }

    /// Selects all text of the document. Returns true if the selection
    /// changed.
    pub fn select_all(&mut self) -> bool {
        self.parts()
            .is_some_and(|(doc, fonts)| doc.select_all(fonts))
    }

    /// Removes the selection. Returns true if there was one.
    pub fn clear_selection(&mut self) -> bool {
        self.set_selection(None)
    }

    /// Replaces the selection. Returns true if it changed.
    fn set_selection(&mut self, selection: Option<Selection>) -> bool {
        self.doc
            .as_deref_mut()
            .is_some_and(|doc| doc.set_selection(selection))
    }

    // ----- Pointer -----

    /// Handles mouse movement over the page (viewport coordinates, CSS
    /// px): hover, the cursor, and drag selection. Returns true if a
    /// repaint is needed.
    pub fn mouse_move(&mut self, x: f32, y: f32) -> bool {
        let point = Point::new(x, y);
        self.input.pointer = Some(point);
        let generation = self.dom_generation();
        if self.script_input() {
            self.scripted_move(point);
        }
        // Listeners may have changed the page.
        let mut changed = generation != self.dom_generation();
        if let Some(press) = &mut self.input.press {
            let (dx, dy) = (point.x - press.origin.x, point.y - press.origin.y);
            press.dragging |= dx.hypot(dy) > DRAG_THRESHOLD;
            let (dragging, anchor, control) = (press.dragging, press.anchor, press.control);
            if dragging && let Some(control) = control {
                changed |= self.drag_in_control(control, point);
            } else if dragging && let Some(anchor) = anchor {
                let focus = self.position_at(point, Snap::Nearest);
                if let Some(focus) = focus {
                    changed |= self.set_selection(Some(Selection::new(anchor, focus)));
                }
            }
        }
        changed | self.update_hover(true)
    }

    /// Updates the hovered link and the cursor for the pointer position,
    /// and with `restyle` also the hovered element (`:hover`). After a
    /// scroll, `:hover` waits for the next mouse movement, as in Chromium:
    /// otherwise each scroll step could restyle the page. Returns true if a
    /// repaint is needed.
    pub(super) fn update_hover(&mut self, restyle: bool) -> bool {
        let Some(pointer) = self.input.pointer else {
            return false;
        };
        let hit = self.hit_test(pointer.x, pointer.y);
        let link = hit.as_ref().and_then(|h| h.link.clone());
        let mut changed = link != self.input.hovered_link;
        self.input.hovered_link = link;
        if restyle {
            let hover = hit.as_ref().and_then(|h| self.element_of(h.node));
            changed |= self.update_states(|s| s.hover = hover);
        }
        // After the restyle: a `:hover` rule can change the cursor.
        self.input.cursor = self.cursor_for(hit.as_ref());
        changed
    }

    /// Handles the mouse pointer leaving the page area. Returns true if a
    /// repaint is needed.
    pub fn mouse_leave(&mut self) -> bool {
        if self.script_input() {
            self.scripted_leave();
        }
        self.input.pointer = None;
        self.input.cursor = Cursor::Auto;
        let changed = self.input.hovered_link.take().is_some();
        changed | self.update_states(|s| s.hover = None)
    }

    /// Handles a mouse button press at a point in viewport coordinates.
    /// The primary button focuses, activates (`:active`) and starts a
    /// selection; `click_count` 2 selects a word, 3 a paragraph, and Shift
    /// extends the selection. In a text field, the press places the caret
    /// (and selects in the field). With scripts, `pointerdown`,
    /// `mousedown` and, for the secondary button, `contextmenu` come first;
    /// a canceled `pointerdown` or `mousedown` leaves the focus and the
    /// selection alone. Returns true if a repaint is needed.
    pub fn mouse_down(
        &mut self,
        x: f32,
        y: f32,
        button: MouseButton,
        modifiers: Modifiers,
        click_count: u32,
    ) -> bool {
        self.input.modifiers = modifiers;
        let point = Point::new(x, y);
        let scripted = self.script_input();
        let mut allowed = true;
        if scripted {
            self.input.pointer = Some(point);
            allowed = self.scripted_down(point, button, click_count);
        }
        if button != MouseButton::Primary {
            return scripted;
        }
        self.input.pointer = Some(point);
        let hit = self.hit_test(x, y);
        let element = hit.as_ref().and_then(|h| self.element_of(h.node));
        let focus = hit
            .as_ref()
            .filter(|_| allowed)
            .and_then(|h| self.doc.as_ref()?.click_focus_target(h.node));
        if focus.is_none() && allowed {
            self.input.focus_start = hit.as_ref().map(|h| h.node);
        }
        self.input.last_press = hit.as_ref().map(|h| h.node);
        // A text control gets the caret; the page selection goes away.
        let control = focus.filter(|&n| self.is_text_control(n));
        let (selection, anchor) = match control {
            Some(_) => (None, None),
            None if !allowed => (None, None),
            None => self.press_selection(point, modifiers, click_count),
        };
        // The offset in the text as shown before the focus changes (which
        // can scroll the text).
        let offset = control.map(|c| self.control_offset_at(c, point));
        if allowed && !scripted {
            self.set_selection(selection);
        }
        self.input.press = Some(Press {
            origin: point,
            target: hit
                .as_ref()
                .and_then(|h| self.activation_target(h.node, point)),
            anchor,
            control,
            dragging: false,
        });
        if scripted {
            // The focus change fires the focus events: a task of its own.
            self.update_states(|s| s.active = element);
            if allowed {
                if focus.is_none() {
                    // A click anywhere in the page brings the window focus
                    // back after Tab left it.
                    self.regain_window_focus();
                }
                self.scripted_focus(focus, false);
                // A press that focuses nothing starts a selection, if no
                // listener cancels `selectstart`.
                if focus.is_none() && !self.fire_selectstart(point) {
                    if let Some(press) = &mut self.input.press {
                        press.anchor = None;
                    }
                } else {
                    self.set_selection(selection);
                }
            }
        } else {
            self.update_states(|s| {
                s.active = element;
                s.focus = focus;
                s.focus_visible = false;
            });
        }
        if let (Some(control), Some(offset)) = (control, offset) {
            self.press_in_control(control, offset, modifiers.shift, click_count);
        }
        self.update_hover(true);
        true
    }

    /// The selection that a press makes, and the anchor of a drag that
    /// follows it.
    fn press_selection(
        &mut self,
        point: Point,
        modifiers: Modifiers,
        click_count: u32,
    ) -> (Option<Selection>, Option<TextPosition>) {
        match click_count {
            2 => {
                let position = self.position_at(point, Snap::Character);
                let word = position.and_then(|p| selection::word_at(self.document()?, p));
                (word, None)
            }
            n if n >= 3 => {
                let selection = self
                    .parts()
                    .and_then(|(doc, fonts)| doc.paragraph_selection(point, fonts));
                (selection, None)
            }
            _ => {
                let position = self.position_at(point, Snap::Nearest);
                let current = self.doc.as_ref().and_then(|d| d.selection);
                match (current, position) {
                    (Some(current), Some(position)) if modifiers.shift => (
                        Some(Selection::new(current.anchor, position)),
                        Some(current.anchor),
                    ),
                    _ => (position.map(|p| Selection::new(p, p)), position),
                }
            }
        }
    }

    /// Handles a mouse button release. A primary-button click (press and
    /// release on the same link, button, checkbox, radio button or label,
    /// without a drag) runs its activation behavior: it follows the link,
    /// submits or resets the form, or changes the control. Returns true if
    /// a repaint is needed.
    pub fn mouse_up(&mut self, x: f32, y: f32, button: MouseButton) -> bool {
        let (changed, activated) = self.release(x, y, button);
        changed || activated
    }

    /// Returns whether a repaint is needed and whether a click activated
    /// something.
    fn release(&mut self, x: f32, y: f32, button: MouseButton) -> (bool, bool) {
        if self.script_input() {
            return self.scripted_release(Point::new(x, y), button);
        }
        if button != MouseButton::Primary {
            return (false, false);
        }
        let press = self.input.press.take();
        let changed = self.update_states(|s| s.active = None);
        let Some(press) = press.filter(|p| !p.dragging) else {
            return (changed, false);
        };
        let hit = self.hit_test(x, y);
        let target = hit.and_then(|h| self.activation_target(h.node, Point::new(x, y)));
        let activated = match target {
            Some(target) if press.target == Some(target) => self.activate(target, Point::new(x, y)),
            _ => false,
        };
        (changed, activated)
    }

    /// The release with scripts: `pointerup`, `mouseup` and the click
    /// events. The activation behavior of the click runs in the dispatch
    /// of `click` (the bindings do it: links, checkboxes, radio buttons,
    /// buttons, labels, summaries), so the engine's own action does not
    /// run. Only the default summary of a `details` element, which has no
    /// element, toggles here. Returns whether a repaint is needed and
    /// whether the click activated something.
    fn scripted_release(&mut self, point: Point, button: MouseButton) -> (bool, bool) {
        let generation = self.dom_generation();
        let press = if button == MouseButton::Primary {
            self.input.press.take()
        } else {
            None
        };
        let mut changed = if button == MouseButton::Primary {
            self.update_states(|s| s.active = None)
        } else {
            false
        };
        let dragging = press.as_ref().is_some_and(|p| p.dragging);
        let up = self.scripted_up(point, button, dragging);
        changed |= generation != self.dom_generation();
        let Some((target, proceed)) = up else {
            return (changed, false);
        };
        let mut activated = false;
        if proceed && button == MouseButton::Primary {
            let activation = self.activation_target(target, point);
            activated = activation.is_some();
            if let Some(node) = activation
                && self.document().is_some_and(|d| {
                    d.element(node)
                        .is_some_and(|e| e.is_html_named(&swb_dom::local_name!("details")))
                })
            {
                changed |= self.toggle_details(node);
            }
        }
        (changed || activated, activated)
    }

    /// Clicks the primary button at a point in viewport coordinates.
    /// Returns true if the click activated something: a navigation
    /// started, the page scrolled to a fragment, or a control changed.
    pub fn click(&mut self, x: f32, y: f32) -> bool {
        self.mouse_down(x, y, MouseButton::Primary, Modifiers::NONE, 1);
        self.release(x, y, MouseButton::Primary).1
    }

    // ----- Keyboard and focus -----

    /// Handles a key press: a focused form control gets it first (typing,
    /// editing and caret keys in text fields, Space and Enter on buttons
    /// and checkboxes, the arrows in selects and radio groups, Enter for
    /// implicit submission). Then Tab and Shift+Tab move the focus, Enter
    /// follows the focused link, Ctrl+A selects all, and the arrows, Page
    /// Up/Down, Space, Home and End scroll. Returns true if the page
    /// handled the key (and needs a repaint).
    ///
    /// With scripts, `keydown` goes first and, if no listener canceled it,
    /// `keypress` for printable keys and Enter; a canceled event leaves the
    /// default action out (no text, no focus move, no scroll, no
    /// activation). An Enter on a link clicks it after `keydown`, without
    /// `keypress` (measured). The edits of text fields fire `beforeinput`
    /// and `input`; Space on a button, a checkbox, a radio button or a
    /// summary clicks it when the key goes up ([`Page::key_up`]).
    pub fn key_down(&mut self, key: &Key, modifiers: Modifiers) -> bool {
        self.input.modifiers = modifiers;
        if !self.script_input() {
            return self.key_default(key, modifiers);
        }
        // A new key going down: a Space whose release was lost (a host that
        // sends no `key_up`) must not click later.
        self.input.space_down = None;
        let info = key.info();
        let Some(target) = self.key_target() else {
            return self.key_default(key, modifiers);
        };
        if !self.fire_key("keydown", target, &info, modifiers) {
            return true;
        }
        let ctrl = modifiers.ctrl || modifiers.meta;
        if key == &Key::Enter
            && !ctrl
            && !modifiers.alt
            && let Some(link) = self.focused_link()
        {
            return self.simulate_click(link);
        }
        if info.keypress && !ctrl {
            let target = self.key_target().unwrap_or(target);
            if !self.fire_key("keypress", target, &info, modifiers) {
                return true;
            }
        }
        self.key_default(key, modifiers)
    }

    /// Handles a key release. With scripts, `keyup` goes to the focused
    /// element (or the body), and Space that went down on a button, a
    /// checkbox, a radio button or a summary clicks it now. Returns true
    /// if the page handled the key.
    pub fn key_up(&mut self, key: &Key, modifiers: Modifiers) -> bool {
        if !self.script_input() {
            return false;
        }
        self.input.modifiers = modifiers;
        let Some(target) = self.key_target() else {
            return false;
        };
        let info = key.info();
        self.fire_key("keyup", target, &info, modifiers);
        if key.is_char(' ')
            && let Some(node) = self.input.space_down.take()
            && self.focused_element() == Some(node)
        {
            self.simulate_click(node);
        }
        true
    }

    /// The default action of a key (after its events, if scripts run).
    fn key_default(&mut self, key: &Key, modifiers: Modifiers) -> bool {
        if let Some(handled) = self.control_key(key, modifiers) {
            return handled;
        }
        let ctrl = modifiers.ctrl || modifiers.meta;
        if !ctrl
            && !modifiers.alt
            && (key == &Key::Enter || key.is_char(' '))
            && let Some(details) = self.focused_summary()
        {
            if self.script_input() {
                return self.scripted_activation_key(key);
            }
            return self.toggle_details(details);
        }
        match key {
            Key::Tab if !ctrl && !modifiers.alt => self.focus_next(modifiers.shift),
            Key::Enter if !ctrl && !modifiers.alt => self.activate_focused(),
            key if ctrl && !modifiers.alt && key.is_char('a') => {
                self.select_all();
                true
            }
            _ if ctrl || modifiers.alt => false,
            key => self.scroll_key(key, modifiers.shift),
        }
    }

    /// Focuses `node` (or removes the focus). `visible` makes the element
    /// match `:focus-visible` and scrolls it into view, as for keyboard
    /// focus. Elements that cannot be focused or are not rendered are
    /// ignored. Returns true if the focus changed.
    pub fn focus(&mut self, node: Option<NodeId>, visible: bool) -> bool {
        if let Some(node) = node {
            let focusable = self
                .document()
                .is_some_and(|doc| doc.get(node).is_some() && focus::is_focusable(doc, node));
            if !focusable || !self.is_rendered(node) {
                return false;
            }
        }
        let before = self.focus_state();
        if self.script_input() {
            // The focus events (`change`, `blur`, `focus`, ...) fire.
            self.scripted_focus(node, visible && node.is_some());
        } else {
            self.update_states(|s| {
                s.focus = node;
                s.focus_visible = visible && node.is_some();
            });
        }
        if visible && let Some(node) = node {
            self.scroll_into_view(node);
        }
        before != self.focus_state()
    }

    /// The focused element and whether it matches `:focus-visible`.
    fn focus_state(&self) -> (Option<NodeId>, bool) {
        self.doc
            .as_ref()
            .map_or((None, false), |d| (d.states.focus, d.states.focus_visible))
    }

    /// Moves the focus for a navigation to a fragment of this document:
    /// to the target if it is focusable; otherwise the focus is removed
    /// and the next Tab starts at the target (the HTML "scroll to the
    /// fragment" steps).
    pub(super) fn focus_fragment_target(&mut self, target: Option<NodeId>) {
        let Some(target) = target else {
            return;
        };
        if !self.focus(Some(target), false) && self.focused_element() != Some(target) {
            self.focus(None, false);
            self.input.focus_start = Some(target);
        }
    }

    /// Moves the focus to the next (or previous) element in sequential
    /// focus order. After the last element, the focus leaves the page; the
    /// next Tab starts at the first element again.
    fn focus_next(&mut self, backward: bool) -> bool {
        let from_start = self.input.focus_start;
        let Some((doc, fonts)) = self.parts() else {
            return false;
        };
        doc.update_layout(fonts);
        let (Some(tree), Some(styles)) = (&doc.fragments, &doc.styles) else {
            return false;
        };
        let dom = &doc.dom;
        let rendered = |n: NodeId| is_rendered_in(dom, tree, styles, n);
        let from = doc.states.focus.or(from_start);
        let next = focus::next_in_order(dom, from, backward, rendered);
        self.input.focus_start = None;
        let leaving = next.is_none() && self.focused_element().is_some();
        self.focus(next, true);
        if leaving && self.script_input() {
            // Tab left the page: the window loses the focus.
            self.input.window_blurred = true;
            self.run_task(|s| s.window_focus(false));
        }
        // Keyboard focus selects the text of a text field.
        if let Some(next) = next {
            self.select_field_text(next);
        }
        true
    }

    /// The `details` element whose summary has the focus.
    fn focused_summary(&self) -> Option<NodeId> {
        let doc = self.doc.as_ref()?;
        doc.dom().details_of_summary(doc.states.focus?)
    }

    /// The focused element if it is a link (an `a` or `area` with `href`)
    /// that is rendered.
    fn focused_link(&mut self) -> Option<NodeId> {
        let focus = self.focused_element().filter(|&n| self.is_rendered(n))?;
        let doc = self.doc.as_ref()?;
        hit_test::link_target(doc.dom(), focus, doc.base_url()).map(|_| focus)
    }

    /// Clicks `node` as a key does (Chromium's simulated click, a trusted
    /// `click` event whose activation behavior follows). Returns true.
    pub(super) fn simulate_click(&mut self, node: NodeId) -> bool {
        self.run_task(|s| s.simulated_click(node));
        true
    }

    /// Enter or Space on a focused summary with scripts: Enter clicks it
    /// at once, Space when the key goes up.
    fn scripted_activation_key(&mut self, key: &Key) -> bool {
        let Some(node) = self.focused_element() else {
            return false;
        };
        if key == &Key::Enter {
            return self.simulate_click(node);
        }
        self.input.space_down = Some(node);
        true
    }

    /// Follows the focused link, if there is one and it is rendered.
    fn activate_focused(&mut self) -> bool {
        if self.script_input() {
            // The click of the key did it (`key_down`).
            return false;
        }
        let Some(focus) = self.focused_element().filter(|&n| self.is_rendered(n)) else {
            return false;
        };
        let link = self
            .doc
            .as_ref()
            .and_then(|d| hit_test::link_target(d.dom(), focus, d.base_url()));
        link.is_some_and(|link| self.follow_link(link))
    }

    /// True if the element has a box and is visible.
    fn is_rendered(&mut self, node: NodeId) -> bool {
        self.parts()
            .is_some_and(|(doc, fonts)| doc.is_rendered(node, fonts))
    }

    /// Scrolls for a scrolling key. Returns true if the key is one. As in
    /// Chromium, the key scrolls the first scroll container that can
    /// scroll in its direction in the scroll chain of the focused element
    /// (or, without one, of the node of the last click); else the
    /// viewport. Page Up, Page Down and Space scroll by 87.5 % of the
    /// scrollport in whole pixels (`scrollers::page_step`).
    fn scroll_key(&mut self, key: &Key, shift: bool) -> bool {
        // The delta for a scrollport of `port` px.
        let delta = |port: Size| -> Option<(f32, f32)> {
            let page_step = crate::scrollers::page_step(port.height);
            Some(match key {
                Key::ArrowDown => (0.0, ARROW_SCROLL),
                Key::ArrowUp => (0.0, -ARROW_SCROLL),
                Key::ArrowRight => (ARROW_SCROLL, 0.0),
                Key::ArrowLeft => (-ARROW_SCROLL, 0.0),
                Key::PageDown => (0.0, page_step),
                Key::PageUp => (0.0, -page_step),
                key if key.is_char(' ') => (0.0, if shift { -page_step } else { page_step }),
                // Scrolling clamps to the content.
                Key::Home => (0.0, -swb_style::Length::MAX_PX),
                Key::End => (0.0, swb_style::Length::MAX_PX),
                _ => return None,
            })
        };
        let Some(viewport) = delta(self.view.viewport) else {
            return false;
        };
        let start = self.focused_element().or(self.input.last_press);
        self.user_scroll(start, |port| delta(port).unwrap_or_default(), viewport);
        true
    }

    // ----- Helpers -----

    /// The text position at a point in viewport coordinates.
    fn position_at(&mut self, point: Point, snap: Snap) -> Option<TextPosition> {
        let (doc, fonts) = self.parts()?;
        doc.position_at(point, snap, fonts)
    }

    /// The element of a hit node: the node itself, or the parent of a text
    /// node.
    fn element_of(&self, node: NodeId) -> Option<NodeId> {
        self.doc.as_ref()?.element_of(node)
    }

    /// The cursor over a hit node: the computed `cursor`, where `auto` is
    /// the text cursor over selectable text and the default cursor
    /// elsewhere. While a drag selects text, the text cursor.
    fn cursor_for(&self, hit: Option<&HitResult>) -> Cursor {
        if self
            .input
            .press
            .as_ref()
            .is_some_and(|p| p.dragging && p.anchor.is_some())
        {
            return Cursor::Text;
        }
        let (Some(hit), Some(doc)) = (hit, self.doc.as_ref()) else {
            return Cursor::Auto;
        };
        let Some(styles) = doc.styles() else {
            return Cursor::Auto;
        };
        let is_text = doc
            .dom()
            .get(hit.node)
            .is_some_and(|n| n.as_text().is_some());
        let Some(style) = doc.element_of(hit.node).and_then(|e| styles.get(e)) else {
            return Cursor::Auto;
        };
        match style.cursor {
            Cursor::Auto if is_text && style.user_select != UserSelect::None => Cursor::Text,
            cursor => cursor,
        }
    }

    /// Changes the element states with `change`. Restyles if a selector
    /// depends on a state that changed; a focus that moves between form
    /// controls lays out again. Returns true if a repaint is needed.
    pub(super) fn update_states(&mut self, change: impl FnOnce(&mut ElementStates)) -> bool {
        let Some(doc) = self.doc.as_deref_mut() else {
            return false;
        };
        let (affected, old_focus) = doc.update_states(change);
        let focus_moved = old_focus != doc.states.focus && self.focus_changed(old_focus);
        affected || focus_moved
    }
}
