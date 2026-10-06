//! Input handling of a page: the pointer (hover, `:active`, clicks, text
//! selection), the keyboard (focus navigation, scrolling, activation), and
//! the element states that depend on them.

use swb_dom::NodeId;
use swb_layout::{Point, Size};
use swb_style::{Cursor, ElementStates, UserSelect, Visibility};

use super::Page;
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

/// The pointer, keyboard and focus state of a page. Reset for each
/// document, except the pointer position.
#[derive(Clone, Debug, Default)]
pub(super) struct InputState {
    pub(super) states: ElementStates,
    /// The pointer position in viewport coordinates, while it is over the
    /// page.
    pub(super) pointer: Option<Point>,
    press: Option<Press>,
    hovered_link: Option<swb_net::Url>,
    cursor: Cursor,
    pub(super) selection: Option<Selection>,
    /// Where sequential focus navigation starts when no element is
    /// focused: the node of the last click or the target of the last
    /// fragment navigation.
    focus_start: Option<NodeId>,
    /// The node of the last press of the primary button. Without a focused
    /// element, keyboard scrolling starts at its scroll container.
    last_press: Option<NodeId>,
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
        self.update_display_list();
        let doc = self.document.as_ref()?;
        let list = self.display_list.as_ref()?;
        let point = Point::new(x + self.scroll.x, y + self.scroll.y);
        hit_test::hit_test(doc, list, point, self.base_url.as_ref())
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
        self.input.states.focus
    }

    // ----- Selection -----

    /// The text selection, if there is one and it is not empty.
    pub fn selection(&self) -> Option<Selection> {
        self.input.selection.filter(|s| !s.is_collapsed())
    }

    /// The selected text, with white space collapsed and line breaks
    /// between blocks (as `innerText`). Empty without a selection. While a
    /// text control has the focus, the text selected in it (nothing for a
    /// password field).
    pub fn selected_text(&mut self) -> String {
        if let Some(text) = self.control_selected_text() {
            return text;
        }
        self.update_layout();
        let (Some(selection), Some(doc), Some(styles)) =
            (self.selection(), &self.document, &self.styles)
        else {
            return String::new();
        };
        let (start, end) = selection.ordered(&self.tree_order);
        selection::selected_text(doc, styles, start, end, &self.tree_order)
    }

    /// The rendered text of an element (or text node), as `innerText`:
    /// collapsed white space, line breaks between blocks. Empty if the
    /// node has no rendered text.
    pub fn element_text(&mut self, node: NodeId) -> String {
        self.update_layout();
        let (Some(doc), Some(tree), Some(styles)) = (&self.document, &self.fragments, &self.styles)
        else {
            return String::new();
        };
        if doc.get(node).is_none() {
            return String::new();
        }
        let extents = selection::text_extents(tree);
        let Some(range) = selection::subtree_range(doc, node, &extents) else {
            return String::new();
        };
        let (start, end) = range.ordered(&self.tree_order);
        selection::selected_text(doc, styles, start, end, &self.tree_order)
    }

    /// Selects all text of the document. Returns true if the selection
    /// changed.
    pub fn select_all(&mut self) -> bool {
        self.update_layout();
        let (Some(doc), Some(tree)) = (&self.document, &self.fragments) else {
            return false;
        };
        let range = selection::subtree_range(doc, NodeId::DOCUMENT, &selection::text_extents(tree));
        self.set_selection(range)
    }

    /// Removes the selection. Returns true if there was one.
    pub fn clear_selection(&mut self) -> bool {
        self.set_selection(None)
    }

    /// Replaces the selection. Returns true if it changed.
    fn set_selection(&mut self, selection: Option<Selection>) -> bool {
        if selection == self.input.selection {
            return false;
        }
        let visible_before = self.selection().is_some();
        self.input.selection = selection;
        if visible_before || self.selection().is_some() {
            // The highlight is part of the display list.
            self.display_list = None;
        }
        true
    }

    // ----- Pointer -----

    /// Handles mouse movement over the page (viewport coordinates, CSS
    /// px): hover, the cursor, and drag selection. Returns true if a
    /// repaint is needed.
    pub fn mouse_move(&mut self, x: f32, y: f32) -> bool {
        let point = Point::new(x, y);
        self.input.pointer = Some(point);
        let mut changed = false;
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
        self.input.pointer = None;
        self.input.cursor = Cursor::Auto;
        let changed = self.input.hovered_link.take().is_some();
        changed | self.update_states(|s| s.hover = None)
    }

    /// Handles a mouse button press at a point in viewport coordinates.
    /// The primary button focuses, activates (`:active`) and starts a
    /// selection; `click_count` 2 selects a word, 3 a paragraph, and Shift
    /// extends the selection. In a text field, the press places the caret
    /// (and selects in the field). Returns true if a repaint is needed.
    pub fn mouse_down(
        &mut self,
        x: f32,
        y: f32,
        button: MouseButton,
        modifiers: Modifiers,
        click_count: u32,
    ) -> bool {
        if button != MouseButton::Primary {
            return false;
        }
        let point = Point::new(x, y);
        self.input.pointer = Some(point);
        let hit = self.hit_test(x, y);
        let element = hit.as_ref().and_then(|h| self.element_of(h.node));
        let focus = hit.as_ref().and_then(|h| self.click_focus_target(h.node));
        if focus.is_none() {
            self.input.focus_start = hit.as_ref().map(|h| h.node);
        }
        self.input.last_press = hit.as_ref().map(|h| h.node);
        // A text control gets the caret; the page selection goes away.
        let control = focus.filter(|&n| self.forms.is_text_control(n));
        let (selection, anchor) = match control {
            Some(_) => (None, None),
            None => self.press_selection(point, modifiers, click_count),
        };
        // The offset in the text as shown before the focus changes (which
        // can scroll the text).
        let offset = control.map(|c| self.control_offset_at(c, point));
        self.set_selection(selection);
        self.input.press = Some(Press {
            origin: point,
            target: hit.as_ref().and_then(|h| self.activation_target(h.node)),
            anchor,
            control,
            dragging: false,
        });
        self.update_states(|s| {
            s.active = element;
            s.focus = focus;
            s.focus_visible = false;
        });
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
                let word = position.and_then(|p| selection::word_at(self.document.as_ref()?, p));
                (word, None)
            }
            n if n >= 3 => (self.paragraph_selection(point), None),
            _ => {
                let position = self.position_at(point, Snap::Nearest);
                match (self.input.selection, position) {
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
        if button != MouseButton::Primary {
            return (false, false);
        }
        let press = self.input.press.take();
        let changed = self.update_states(|s| s.active = None);
        let Some(press) = press.filter(|p| !p.dragging) else {
            return (changed, false);
        };
        let hit = self.hit_test(x, y);
        let target = hit.and_then(|h| self.activation_target(h.node));
        let activated = match target {
            Some(target) if press.target == Some(target) => self.activate(target, Point::new(x, y)),
            _ => false,
        };
        (changed, activated)
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
    pub fn key_down(&mut self, key: &Key, modifiers: Modifiers) -> bool {
        if let Some(handled) = self.control_key(key, modifiers) {
            return handled;
        }
        let ctrl = modifiers.ctrl || modifiers.meta;
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
                .document
                .as_ref()
                .is_some_and(|doc| doc.get(node).is_some() && focus::is_focusable(doc, node));
            if !focusable || !self.is_rendered(node) {
                return false;
            }
        }
        let before = (self.input.states.focus, self.input.states.focus_visible);
        self.update_states(|s| {
            s.focus = node;
            s.focus_visible = visible && node.is_some();
        });
        if visible && let Some(node) = node {
            self.scroll_into_view(node);
        }
        before != (self.input.states.focus, self.input.states.focus_visible)
    }

    /// Moves the focus for a navigation to a fragment of this document:
    /// to the target if it is focusable; otherwise the focus is removed
    /// and the next Tab starts at the target (the HTML "scroll to the
    /// fragment" steps).
    pub(super) fn focus_fragment_target(&mut self, target: Option<NodeId>) {
        let Some(target) = target else {
            return;
        };
        if !self.focus(Some(target), false) && self.input.states.focus != Some(target) {
            self.focus(None, false);
            self.input.focus_start = Some(target);
        }
    }

    /// Moves the focus to the next (or previous) element in sequential
    /// focus order. After the last element, the focus leaves the page; the
    /// next Tab starts at the first element again.
    fn focus_next(&mut self, backward: bool) -> bool {
        self.update_layout();
        let (Some(doc), Some(tree), Some(styles)) = (&self.document, &self.fragments, &self.styles)
        else {
            return false;
        };
        let boxes = tree.element_boxes();
        let rendered = |n: NodeId| {
            boxes.contains_key(&n)
                && styles
                    .get(n)
                    .is_some_and(|s| s.visibility == Visibility::Visible)
        };
        let from = self.input.states.focus.or(self.input.focus_start);
        let next = focus::next_in_order(doc, from, backward, rendered);
        self.input.focus_start = None;
        self.focus(next, true);
        // Keyboard focus selects the text of a text field.
        if let Some(next) = next {
            self.select_field_text(next);
        }
        true
    }

    /// Follows the focused link, if there is one and it is rendered.
    fn activate_focused(&mut self) -> bool {
        let Some(focus) = self.input.states.focus.filter(|&n| self.is_rendered(n)) else {
            return false;
        };
        let link = match (&self.document, &self.base_url) {
            (Some(doc), Some(base)) => hit_test::link_target(doc, focus, base),
            _ => None,
        };
        link.is_some_and(|link| self.follow_link(link))
    }

    /// True if the element has a box and is visible.
    fn is_rendered(&mut self, node: NodeId) -> bool {
        self.update_layout();
        let (Some(tree), Some(styles)) = (&self.fragments, &self.styles) else {
            return false;
        };
        styles
            .get(node)
            .is_some_and(|s| s.visibility == Visibility::Visible)
            && tree.element_boxes().contains_key(&node)
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
        let Some(viewport) = delta(self.viewport) else {
            return false;
        };
        let start = self.input.states.focus.or(self.input.last_press);
        self.user_scroll(start, |port| delta(port).unwrap_or_default(), viewport);
        true
    }

    // ----- Helpers -----

    /// The text position at a point in viewport coordinates.
    fn position_at(&mut self, point: Point, snap: Snap) -> Option<TextPosition> {
        self.update_layout();
        let tree = self.fragments.as_ref()?;
        let document_point = Point::new(point.x + self.scroll.x, point.y + self.scroll.y);
        selection::position_at(tree, self.scroll_offsets(), document_point, snap)
    }

    /// The selection of a triple click: the paragraph at a point.
    fn paragraph_selection(&mut self, point: Point) -> Option<Selection> {
        let position = self.position_at(point, Snap::Character)?;
        let (doc, styles, tree) = (
            self.document.as_ref()?,
            self.styles.as_ref()?,
            self.fragments.as_ref()?,
        );
        selection::paragraph_at(doc, styles, &selection::text_extents(tree), position)
    }

    /// The element that a click on `node` focuses.
    fn click_focus_target(&self, node: NodeId) -> Option<NodeId> {
        let (doc, tree) = (self.document.as_ref()?, self.fragments.as_ref()?);
        let boxes = tree.element_boxes();
        focus::click_target(doc, node, |n| boxes.contains_key(&n))
    }

    /// The element of a hit node: the node itself, or the parent of a text
    /// node.
    fn element_of(&self, node: NodeId) -> Option<NodeId> {
        let doc = self.document.as_ref()?;
        if doc.get(node)?.is_element() {
            Some(node)
        } else {
            doc.parent_element(node)
        }
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
        let (Some(hit), Some(doc), Some(styles)) = (hit, &self.document, &self.styles) else {
            return Cursor::Auto;
        };
        let is_text = doc.get(hit.node).is_some_and(|n| n.as_text().is_some());
        let Some(style) = self.element_of(hit.node).and_then(|e| styles.get(e)) else {
            return Cursor::Auto;
        };
        match style.cursor {
            Cursor::Auto if is_text && style.user_select != UserSelect::None => Cursor::Text,
            cursor => cursor,
        }
    }

    /// Changes the element states with `change`. See
    /// [`Page::set_states`].
    pub(super) fn update_states(&mut self, change: impl FnOnce(&mut ElementStates)) -> bool {
        let mut states = self.input.states.clone();
        change(&mut states);
        self.set_states(states)
    }

    /// Changes the element states. Restyles if a selector depends on a
    /// state that changed. Returns true if a repaint is needed.
    fn set_states(&mut self, states: ElementStates) -> bool {
        let changed = self.input.states.changes(&states);
        let old_focus = self.input.states.focus;
        self.input.states = states;
        let focus_moved = old_focus != self.input.states.focus && self.focus_changed(old_focus);
        if changed.is_empty() {
            return focus_moved;
        }
        let affected = self
            .stylist
            .as_ref()
            .is_none_or(|s| s.state_dependencies().intersects(changed));
        if affected {
            self.restyle();
        }
        affected || focus_moved
    }
}
