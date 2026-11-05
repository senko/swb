//! Focus and keys in frames (ADR 0031 section 8, ADR 0009; M9 feature 11).
//!
//! Each document has a focused area (`states.focus`). When a frame holds
//! the page's focus, the documents above it have no focused element of
//! their own: their `frame_focus` is the `iframe` that holds the focus
//! (`document.activeElement` is the `iframe`, measured; `:focus` does not
//! match it), and [`InputState::focus_frame`](super::input::InputState) names
//! the innermost frame. A key goes to the document of that frame.
//!
//! Events of the page's scripts (measured in Chromium 148): the focus
//! moving into a frame gives the page's element a `blur` and the window a
//! `blur`, with no `focus` event on the `iframe`; the focus returning to
//! the page gives the window a `focus` and the next element a `focus`. The
//! frames' own events wait for scripts in frames (feature 12).
//!
//! *Tab* treats a frame as its content: sequential navigation reaches the
//! `iframe` element at its place in the order, then enters the frame at its
//! first (last, backward) focusable element, recursively; after the frame's
//! last element it goes on after the `iframe` in the parent. A frame without
//! a focusable element is a stop of its own (its document is focused with no
//! element; measured), and a hidden frame or one with `tabindex=-1` is
//! skipped (measured).

use swb_dom::NodeId;

use super::Page;
use crate::document::rendered_test;
use crate::focus;
use crate::frames::{FrameId, MAX_DEPTH};
use crate::input::{Key, Modifiers};

/// What the active document decides for a key.
pub(super) enum KeyOutcome {
    /// The document handled the key (or ignores it); true if it did
    /// something.
    Handled(bool),
    /// Tab: the focus moves, perhaps out of the document.
    Tab,
    /// A scrolling key that nothing in the document used: the scroll chain
    /// goes on.
    Scroll,
}

/// Where sequential navigation starts in a document.
#[derive(Clone, Copy)]
enum TabStart {
    /// At the focused element, else at the last click.
    Current,
    /// After (before, backward) an element: the `iframe` that was left.
    After(NodeId),
    /// At the first (last, backward) element: the frame was entered.
    Edge,
}

/// The result of a step of sequential navigation in one document.
enum TabStep {
    /// The next element.
    Node(NodeId),
    /// There is none.
    End,
}

impl Page {
    /// The frame (not the page's own document) that holds the keyboard
    /// focus. Checks the state: the frame is gone, or an element of the
    /// page's own document got the focus since (a script's `focus()`).
    pub(super) fn focused_frame(&mut self) -> Option<FrameId> {
        let frame = self.input.focus_frame?;
        let page_focus = self.doc.as_ref().is_some_and(|d| d.states.focus.is_some());
        if self.doc_of(frame).is_some() && !page_focus {
            return Some(frame);
        }
        self.set_focus_frame(FrameId::TOP);
        None
    }

    /// Runs `run` in the focused document: the frame that holds the focus,
    /// else the page's own. Inside [`Page::in_frame`] it runs in the active
    /// document.
    pub(super) fn in_focused_document<R: Default>(
        &mut self,
        run: impl FnOnce(&mut Page) -> R,
    ) -> R {
        if self.active_frame != FrameId::TOP {
            return run(self);
        }
        let frame = self.focused_frame().unwrap_or(FrameId::TOP);
        self.in_frame(frame, run).unwrap_or_default()
    }

    /// The frame that holds the focus, as a test or a tool needs it
    /// ([`FrameId::TOP`] if the page's own document does).
    pub fn focused_frame_id(&mut self) -> FrameId {
        self.focused_frame().unwrap_or(FrameId::TOP)
    }

    /// Gives the keyboard focus to the document of `new` ([`FrameId::TOP`]:
    /// the page's own): the documents above it are focused through their
    /// `iframe`, the documents that lose it lose their focused element.
    /// Moving the focus from the page's own element into a frame gives that
    /// element a `blur` and the window a `blur` (scripts on). Leaving a
    /// frame gives no events here: the caller focuses an element of the
    /// page's own document, which gives the window its `focus` first.
    /// Not inside [`Page::in_frame`].
    pub(super) fn set_focus_frame(&mut self, new: FrameId) {
        if self.active_frame != FrameId::TOP {
            return;
        }
        let old = self.input.focus_frame.unwrap_or(FrameId::TOP);
        if old == new {
            return;
        }
        let old_chain = self.frame_chain(old);
        let new_chain = self.frame_chain(new);
        let common = old_chain
            .iter()
            .zip(&new_chain)
            .take_while(|(a, b)| a == b)
            .count();
        // The frames that lose the focus, innermost first.
        for &frame in old_chain[common.min(old_chain.len())..].iter().rev() {
            self.in_frame(frame, Page::drop_document_focus);
        }
        // A document that the focus enters a frame of loses its selection;
        // one that the focus leaves keeps it (measured, cases
        // `input-select-parent-click-frame`, `input-select-frame-cleared`,
        // `input-select-frame-to-frame`).
        for (i, &frame) in new_chain.iter().enumerate() {
            let container = new_chain
                .get(i + 1)
                .and_then(|&next| self.frames.get(next))
                .and_then(|f| f.container);
            if frame == FrameId::TOP {
                self.set_top_frame_focus(container);
            } else {
                self.in_frame(frame, |p| {
                    if container.is_some() {
                        p.drop_document_focus();
                        p.set_selection(None);
                    }
                    if let Some(doc) = p.doc.as_deref_mut() {
                        doc.frame_focus = container;
                    }
                });
            }
        }
        if new != FrameId::TOP && self.doc_of(new).is_none() {
            // A `blur` listener removed the frame: the focus is nowhere, and
            // the window has none until the next focus (measured, cases
            // `input-focus-tab-remove`, `input-focus-click-remove`).
            for &frame in &new_chain {
                self.in_frame(frame, Page::drop_document_focus);
            }
            self.input.focus_frame = None;
            self.sync_window_focus();
            return;
        }
        self.input.focus_frame = (new != FrameId::TOP).then_some(new);
        self.sync_window_focus();
    }

    /// The page's own document is focused through `container` (an
    /// `iframe` that holds the focus), or not any more (`None`).
    fn set_top_frame_focus(&mut self, container: Option<NodeId>) {
        if container.is_some() {
            self.set_selection(None);
            if self.focused_element().is_some() {
                if self.script_input() {
                    self.scripted_focus(None, false);
                } else {
                    self.update_states(|s| {
                        s.focus = None;
                        s.focus_visible = false;
                    });
                }
            }
            self.blur_window();
        }
        if let Some(doc) = self.doc.as_deref_mut() {
            doc.frame_focus = container;
        }
    }

    /// The window loses the focus (scripts: `blur` at the window): the
    /// focus is in a frame, or left the page.
    pub(super) fn blur_window(&mut self) {
        if std::mem::replace(&mut self.input.window_blurred, true) {
            return;
        }
        self.sync_window_focus();
        if self.script_input() {
            self.run_task(|s| s.window_focus(false));
        }
    }

    /// Tells the document whether the window has the focus
    /// (`document.hasFocus()`): it has it unless it gave it up, and a frame
    /// that holds the focus counts as the window's focus.
    pub(super) fn sync_window_focus(&mut self) {
        let blurred = self.input.window_blurred && self.input.focus_frame.is_none();
        if let Some(doc) = self.doc.as_deref_mut() {
            doc.window_blurred = blurred;
        }
    }

    /// The active document is not focused any more: no focused element, no
    /// frame that holds the focus.
    fn drop_document_focus(&mut self) {
        self.update_states(|s| {
            s.focus = None;
            s.focus_visible = false;
        });
        if let Some(doc) = self.doc.as_deref_mut() {
            doc.frame_focus = None;
        }
    }

    // ----- Keys -----

    /// A key that goes to the focused frame's document: the document's own
    /// default action, then what depends on other documents (Tab, the
    /// scroll chain). Returns true if the key was handled.
    pub(super) fn key_down_in_frame(
        &mut self,
        frame: FrameId,
        key: &Key,
        modifiers: Modifiers,
    ) -> bool {
        let outcome = self.in_frame(frame, |p| {
            p.input.modifiers = modifiers;
            p.set_keyboard_modality(true);
            if p.focused_element().is_some() {
                p.update_states(|s| s.focus_visible = true);
            }
            p.key_in_document(key, modifiers)
        });
        match outcome {
            Some(outcome) => self.finish_key(frame, &outcome, key, modifiers),
            None => false,
        }
    }

    // ----- Sequential navigation -----

    /// Moves the focus to the next (or previous) element in sequential
    /// focus order, starting in `frame` (the module documentation tells how
    /// frames take part). After the last element, the focus leaves the page;
    /// the next Tab starts at the first element again. Returns true.
    pub(super) fn tab_from(&mut self, frame: FrameId, backward: bool) -> bool {
        let (mut frame, mut start) = (frame, TabStart::Current);
        // Each step enters a frame or leaves one.
        for _ in 0..=(2 * MAX_DEPTH + 2) {
            let Some(step) = self.in_frame(frame, |p| p.tab_step(backward, start)) else {
                return false;
            };
            match step {
                TabStep::Node(node) => {
                    let child = self
                        .frames
                        .by_container(frame, node)
                        .filter(|&c| self.doc_of(c).is_some());
                    match child {
                        Some(child) => (frame, start) = (child, TabStart::Edge),
                        None => return self.finish_tab(frame, Some(node)),
                    }
                }
                // A frame that was entered and has no focusable element is
                // a stop of its own.
                TabStep::End if matches!(start, TabStart::Edge) => {
                    return self.finish_tab(frame, None);
                }
                TabStep::End => {
                    let up = self
                        .frames
                        .get(frame)
                        .and_then(|f| Some((f.parent?, f.container?)));
                    match up {
                        Some((parent, container)) => {
                            (frame, start) = (parent, TabStart::After(container));
                        }
                        // The page's own document: the focus leaves the page.
                        None => return self.finish_tab(FrameId::TOP, None),
                    }
                }
            }
        }
        false
    }

    /// The next element of the active document in sequential order.
    fn tab_step(&mut self, backward: bool, start: TabStart) -> TabStep {
        let from_start = self.input.focus_start;
        let Some((doc, fonts)) = self.parts() else {
            return TabStep::End;
        };
        doc.update_layout(fonts);
        let (Some(tree), Some(styles)) = (&doc.fragments, &doc.styles) else {
            return TabStep::End;
        };
        let dom = &doc.dom;
        let rendered = rendered_test(dom, tree, styles);
        let from = match start {
            TabStart::Current => doc.states.focus.or(from_start),
            TabStart::After(node) => Some(node),
            TabStart::Edge => None,
        };
        let next = focus::next_in_order(dom, from, backward, rendered);
        self.input.focus_start = None;
        next.map_or(TabStep::End, TabStep::Node)
    }

    /// Focuses `node` of the document of `frame` for a Tab (`None`: the
    /// frame itself, or, for the page's own document, nothing: the focus
    /// leaves the page).
    fn finish_tab(&mut self, frame: FrameId, node: Option<NodeId>) -> bool {
        if frame == FrameId::TOP {
            let leaving = node.is_none()
                && (self.focused_element().is_some() || self.input.focus_frame.is_some());
            self.focus(node, true);
            if leaving {
                // Tab left the page: the window loses the focus (once).
                self.blur_window();
            }
            if let Some(node) = node {
                self.select_field_text(node);
            }
            return true;
        }
        self.set_focus_frame(frame);
        self.in_frame(frame, |p| {
            p.focus(node, true);
            if let Some(node) = node {
                p.select_field_text(node);
            }
        });
        self.scroll_frame_into_view(frame);
        true
    }

    /// Scrolls the documents above `frame` so that its `iframe` is visible
    /// (the focus scrolls through frames). Departure: the whole `iframe`,
    /// not the focused element's rectangle.
    fn scroll_frame_into_view(&mut self, frame: FrameId) {
        let mut at = frame;
        for _ in 0..=MAX_DEPTH {
            let Some((parent, container)) = self
                .frames
                .get(at)
                .and_then(|f| Some((f.parent?, f.container?)))
            else {
                break;
            };
            self.in_frame(parent, |p| p.scroll_into_view(container));
            at = parent;
        }
    }
}
