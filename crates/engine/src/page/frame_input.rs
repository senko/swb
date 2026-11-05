//! Pointer input in frames (ADR 0031 section 8; M9 feature 11): the hit
//! test through frames, hover per document, presses that stay in the frame
//! where they started, and the wheel with its scroll chain across frames.
//!
//! *Design.* The input code of a document (`input.rs`, `forms.rs`,
//! `scroll.rs`) acts on `Page::doc`. Input in a frame runs the same code
//! with the frame's document in that place: [`Page::in_frame`] exchanges
//! the document, the pointer and keyboard state, the viewport and the font
//! faces of the frame with the page's, runs the code, and exchanges them
//! back. Frame documents run no scripts, so the code of the frame takes the
//! paths of a page without JavaScript. What changes the page itself (a
//! navigation) waits until the exchange is undone ([`Page::in_frame`] runs
//! the deferred navigations). The page's own document then acts as the
//! parent: its events and states see the `iframe` element where the frame
//! is.
//!
//! *Hit test.* [`Page::route_at`] follows a point from the page's document
//! into the frames: each document's display list tells whether the topmost
//! thing at the point is an `iframe`'s content (`swb_paint::Hit::Frame`); the
//! point then goes on in the frame's viewport. One work budget covers the
//! whole test, and the nesting limit bounds the depth.
//!
//! *Hover* is per document: the parent's hovered element is the `iframe`,
//! the frame's chain starts at its root. A frame that the pointer leaves
//! loses its hover state. *The press* of the primary button that starts in
//! a frame keeps the following pointer events in that frame until the
//! release, at the point that the frame sees (the offset between the page's
//! viewport and the frame's, taken at the press).

use swb_dom::NodeId;
use swb_layout::Point;
use swb_paint::HitWork;

use super::Page;
use crate::frames::{FrameId, MAX_DEPTH};
use crate::hit_test::HitResult;
use crate::input::{Modifiers, MouseButton};

/// A document that a point hits, and where.
#[derive(Clone, Copy, Debug)]
pub(super) struct RouteStep {
    /// The frame whose document it is.
    pub(super) frame: FrameId,
    /// The point in the viewport of that document (CSS px).
    pub(super) point: Point,
    /// The node that the document has there: the `iframe` element for a
    /// document whose frame the point goes into; `None` if nothing.
    pub(super) node: Option<NodeId>,
}

/// The documents that a point hits, outermost first, and the node at the
/// end of the path.
#[derive(Clone, Debug)]
pub(super) struct Route {
    pub(super) steps: Vec<RouteStep>,
    /// What the innermost document has at the point.
    pub(super) hit: Option<HitResult>,
    /// True if the route stays in the page's own document (a press of it
    /// holds the pointer) but the point is over the content of a frame.
    pub(super) over_frame: bool,
}

impl Route {
    /// True if the point is in a frame.
    pub(super) fn in_frame(&self) -> bool {
        self.steps.len() > 1
    }

    /// The innermost document of the route.
    pub(super) fn last(&self) -> RouteStep {
        self.steps[self.steps.len() - 1]
    }
}

/// A press of the primary button that started in a frame.
#[derive(Clone, Debug)]
pub(super) struct FramePress {
    /// The frame where the press started.
    pub(super) frame: FrameId,
    /// The frames from the page's document down to `frame`.
    pub(super) chain: Vec<FrameId>,
    /// The point in the page's viewport minus the point in the frame's
    /// viewport, at the press.
    pub(super) offset: Point,
}

impl Page {
    /// Hit-tests a point in viewport coordinates (CSS px), through the
    /// frames: a point in the content of an `iframe` hits a node of the
    /// frame's document ([`HitResult::frame`]); a point on its border, the
    /// element.
    pub fn hit_test(&mut self, x: f32, y: f32) -> Option<HitResult> {
        self.route_at(Point::new(x, y)).hit
    }

    /// The route of a point for the pointer: through the frames, except
    /// while a press of the page's own document holds the pointer (the
    /// frames see nothing then).
    pub(super) fn route_for_pointer(&mut self, point: Point) -> Route {
        if self.input.press.is_some() {
            return self.route_in_top(point);
        }
        self.route_through_frames(point)
    }

    /// The route of a point through the frames (the page's own document
    /// alone if it has none).
    pub(super) fn route_through_frames(&mut self, point: Point) -> Route {
        if self.frames.count() == 0 {
            return self.route_in_top(point);
        }
        self.route_at(point)
    }

    /// The route that stays in the page's own document: its hit at the
    /// point (the `iframe` element over a frame).
    fn route_in_top(&mut self, point: Point) -> Route {
        let hit = self.hit_doc(point.x, point.y);
        let over_frame = hit
            .as_ref()
            .is_some_and(|h| self.frames.by_container(FrameId::TOP, h.node).is_some());
        Route {
            steps: vec![RouteStep {
                frame: FrameId::TOP,
                point,
                node: hit.as_ref().map(|h| h.node),
            }],
            hit,
            over_frame,
        }
    }

    /// The text selected in the document of frame `id` (as
    /// [`Page::selected_text`] for the focused document), for tests and
    /// tools. Empty for a destroyed frame.
    pub fn frame_selected_text(&mut self, id: FrameId) -> String {
        if self.active_frame != FrameId::TOP {
            return String::new();
        }
        self.in_frame(id, |p| {
            p.parts()
                .map(|(doc, fonts)| doc.selected_text(fonts))
                .unwrap_or_default()
        })
        .unwrap_or_default()
    }

    /// Runs `run` with the document of frame `id` as the page's document:
    /// the queries, scrolls and input of [`Page`] then act on the frame
    /// (the automation API takes a `frameId` this way). Coordinates are the
    /// frame's viewport's. `None` for a destroyed frame, a document that is
    /// lent to a script task, and a call inside another `with_frame`. The
    /// code must not navigate the page.
    pub fn with_frame<R>(&mut self, id: FrameId, run: impl FnOnce(&mut Page) -> R) -> Option<R> {
        self.in_frame(id, run)
    }

    /// The content box of frame `id`'s `iframe` in the document
    /// coordinates of its parent (the viewport of the frame), as the
    /// parent's last display list has it; `None` for the top-level frame
    /// and for a frame whose `iframe` has no box.
    pub fn frame_content_box(&mut self, id: FrameId) -> Option<swb_layout::Rect> {
        let (parent, container) = {
            let record = self.frames.get(id)?;
            (record.parent?, record.container?)
        };
        if parent == FrameId::TOP {
            let (doc, fonts) = self.parts()?;
            return doc.frame_item_rect(container, fonts);
        }
        let doc = self.frames.get_mut(parent)?.doc.as_deref_mut()?;
        doc.with_own_fonts(&mut self.fonts, |doc, fonts| {
            doc.frame_item_rect(container, fonts)
        })
    }

    /// The top left corner of frame `id`'s viewport in the page's viewport
    /// coordinates (the page's own for the top-level frame: the origin),
    /// the sum of the content boxes of the frames on the way, less the
    /// parents' scroll offsets. Transforms of the `iframe` elements are not
    /// followed. `None` if a frame on the way has no box.
    pub fn frame_origin(&mut self, id: FrameId) -> Option<Point> {
        let mut origin = Point::default();
        let mut at = id;
        for _ in 0..=MAX_DEPTH {
            if at == FrameId::TOP {
                return Some(origin);
            }
            let rect = self.frame_content_box(at)?;
            let parent = self.frames.get(at)?.parent?;
            let scroll = self.frame_document(parent)?.scroll_position();
            origin = Point::new(origin.x + rect.x - scroll.x, origin.y + rect.y - scroll.y);
            at = parent;
        }
        None
    }

    /// The hit test of the active document alone: for a point in the
    /// content of an `iframe`, the element.
    pub(super) fn hit_doc(&mut self, x: f32, y: f32) -> Option<HitResult> {
        let frame = self.active_frame;
        let (doc, fonts) = self.parts()?;
        let hit = doc.hit_test_in(x, y, fonts, &HitWork::new())?;
        Some(HitResult {
            frame,
            ..hit.result
        })
    }

    /// The documents that `point` (in the page's viewport) hits, from the
    /// page's own down through the frames. Must run outside
    /// [`Page::in_frame`].
    pub(super) fn route_at(&mut self, point: Point) -> Route {
        let work = HitWork::new();
        let (mut frame, mut local) = (FrameId::TOP, point);
        let mut steps = Vec::new();
        let mut result = None;
        loop {
            let Some(hit) = self.document_hit(frame, local, &work) else {
                steps.push(RouteStep {
                    frame,
                    point: local,
                    node: None,
                });
                break;
            };
            let node = hit.result.node;
            steps.push(RouteStep {
                frame,
                point: local,
                node: Some(node),
            });
            let child = hit
                .into_frame
                .zip(self.frames.by_container(frame, node))
                .filter(|_| steps.len() <= MAX_DEPTH as usize && !work.exhausted());
            let Some((inner, child)) = child.filter(|(_, c)| self.doc_of(*c).is_some()) else {
                result = Some(HitResult {
                    frame,
                    ..hit.result
                });
                break;
            };
            self.layout_frame_alone(child);
            (frame, local) = (child, inner);
        }
        Route {
            steps,
            hit: result,
            over_frame: false,
        }
    }

    /// The hit test of one document in the frame tree (not the active one).
    fn document_hit(
        &mut self,
        frame: FrameId,
        point: Point,
        work: &HitWork,
    ) -> Option<crate::hit_test::DocumentHit> {
        if frame == FrameId::TOP {
            let (doc, fonts) = self.parts()?;
            return doc.hit_test_in(point.x, point.y, fonts, work);
        }
        let doc = self.frames.get_mut(frame)?.doc.as_deref_mut()?;
        doc.with_own_fonts(&mut self.fonts, |doc, fonts| {
            doc.hit_test_in(point.x, point.y, fonts, work)
        })
    }

    /// Runs `run` with the document of `id` in place of the page's own and
    /// the input state, viewport and font faces of the frame (see the
    /// module documentation). `None` if the frame has no document or the
    /// page's document is lent. The navigations that the input asked for
    /// run after the exchange is undone. Not nestable: inside another
    /// exchange it does nothing and returns `None`, also for the page's own
    /// document, so a caller cannot run on the wrong document.
    pub(super) fn in_frame<R>(
        &mut self,
        id: FrameId,
        run: impl FnOnce(&mut Page) -> R,
    ) -> Option<R> {
        if self.active_frame != FrameId::TOP {
            return None;
        }
        if id == FrameId::TOP {
            return Some(run(self));
        }
        self.doc.as_ref()?;
        let record = self.frames.get_mut(id)?;
        let mut inner = record.doc.take()?;
        let viewport = record.viewport;
        let mut faces = std::mem::take(&mut inner.web_fonts.set);
        self.fonts.swap_web_fonts(&mut faces);
        let mut input = self.frame_inputs.remove(&id).unwrap_or_default();
        std::mem::swap(&mut self.input, &mut input);
        let script = self.script.take();
        let outer_viewport = std::mem::replace(&mut self.view.viewport, viewport);
        let outer = self.doc.replace(inner);
        self.active_frame = id;

        let result = run(self);

        self.active_frame = FrameId::TOP;
        self.view.viewport = outer_viewport;
        self.script = script;
        std::mem::swap(&mut self.input, &mut input);
        let inner = std::mem::replace(&mut self.doc, outer);
        self.fonts.swap_web_fonts(&mut faces);
        if let (Some(mut inner), Some(record)) = (inner, self.frames.get_mut(id)) {
            inner.web_fonts.set = faces;
            record.doc = Some(inner);
            self.frame_inputs.insert(id, input);
        }
        // The input may have changed the frame's own `iframe` elements
        // (a `details` that opened) or asked for navigations.
        self.update_frames(id);
        self.run_deferred_navigations();
        Some(result)
    }

    /// The frames from the page's document down to `frame`.
    pub(super) fn frame_chain(&self, frame: FrameId) -> Vec<FrameId> {
        let mut chain = vec![frame];
        let mut at = frame;
        while let Some(parent) = self.frames.get(at).and_then(|f| f.parent) {
            chain.push(parent);
            at = parent;
            if chain.len() > MAX_DEPTH as usize + 2 {
                break;
            }
        }
        chain.reverse();
        chain
    }

    /// True if the user can scroll the viewport of frame `id`: not for a
    /// frame whose `iframe` has `scrolling=no` (also `off` and `noscroll`,
    /// ADR 0031 section 4).
    pub(super) fn frame_user_scrollable(&self, id: FrameId) -> bool {
        let Some(frame) = self.frames.get(id) else {
            return true;
        };
        let (Some(parent), Some(container)) = (frame.parent, frame.container) else {
            return true;
        };
        let value = self
            .doc_of(parent)
            .and_then(|d| d.dom().element(container))
            .and_then(|e| e.attr("scrolling"));
        !value.is_some_and(|v| {
            ["no", "off", "noscroll"]
                .iter()
                .any(|k| v.eq_ignore_ascii_case(k))
        })
    }

    // ----- Pointer -----

    /// The pointer moved to `point` while no press holds it in a frame: the
    /// page's document sees the move (its events end at the `iframe`), each
    /// frame on the route gets its hover state, and the frames that the
    /// pointer left lose theirs. Returns true if a repaint is needed.
    pub(super) fn move_in_frames(&mut self, route: &Route, point: Point) -> bool {
        let mut changed = self.move_top(point, route.in_frame() || route.over_frame);
        changed |= self.update_hover_route(route, true);
        changed
    }

    /// Sets the hover state of the documents on the route (when `restyle`),
    /// clears it in the frames that left the route, and updates the hovered
    /// link and the cursor from the innermost document. Returns true if a
    /// repaint is needed.
    pub(super) fn update_hover_route(&mut self, route: &Route, restyle: bool) -> bool {
        let link = route.hit.as_ref().and_then(|h| h.link.clone());
        let mut changed = link != self.input.hovered_link;
        self.input.hovered_link = link;
        let frames: Vec<FrameId> = route.steps.iter().skip(1).map(|s| s.frame).collect();
        for step in &route.steps {
            let (point, node) = (step.point, step.node);
            let hover = move |p: &mut Page| {
                p.input.pointer = Some(point);
                if !restyle {
                    return false;
                }
                let element = node.and_then(|n| p.element_of(n));
                p.update_states(|s| s.hover = element)
            };
            changed |= self.in_frame(step.frame, hover).unwrap_or(false);
        }
        if restyle {
            for gone in std::mem::replace(&mut self.input.hover_frames, frames.clone()) {
                if !frames.contains(&gone) {
                    changed |= self.clear_hover(gone);
                }
            }
        }
        // After the restyle: a `:hover` rule can change the cursor.
        let last = route.last().frame;
        let hit = route.hit.clone();
        self.input.cursor = self
            .in_frame(last, |p| p.cursor_for(hit.as_ref()))
            .unwrap_or_default();
        changed
    }

    /// Removes the pointer and the hover state of frame `id`'s document.
    fn clear_hover(&mut self, id: FrameId) -> bool {
        self.in_frame(id, |p| {
            p.input.pointer = None;
            p.input.cursor = swb_style::Cursor::Auto;
            p.update_states(|s| s.hover = None)
        })
        .unwrap_or(false)
    }

    /// Clears the hover state of all frames (the pointer left the page).
    pub(super) fn clear_frame_hover(&mut self) -> bool {
        let mut changed = false;
        for id in std::mem::take(&mut self.input.hover_frames) {
            changed |= self.clear_hover(id);
        }
        changed
    }

    /// A press of the primary button in the content of a frame (`route`
    /// goes into it). The events of the page's document end at the
    /// `iframe` (the pointer events of entering it); the press itself goes
    /// to the innermost document. Returns true if a repaint is needed.
    pub(super) fn press_in_frame(
        &mut self,
        route: &Route,
        point: Point,
        modifiers: Modifiers,
        click_count: u32,
    ) -> bool {
        let scripted = self.script_input();
        if scripted && self.input.last_point != Some(point) {
            self.scripted_move(point, true);
        }
        let last = route.last();
        // The documents above the innermost one see the press on the
        // `iframe` that holds the frame: it is `:active`.
        for step in &route.steps[..route.steps.len() - 1] {
            let element = step.node.and_then(|n| self.element_of_in(step.frame, n));
            self.in_frame(step.frame, |p| p.update_states(|s| s.active = element));
        }
        self.in_frame(last.frame, |p| {
            p.press_in_document(last.point, modifiers, click_count, false, true)
        });
        let chain: Vec<FrameId> = route.steps.iter().map(|s| s.frame).collect();
        self.input.press_frame = Some(FramePress {
            frame: last.frame,
            chain,
            offset: Point::new(point.x - last.point.x, point.y - last.point.y),
        });
        self.set_focus_frame(last.frame);
        self.update_hover_route(route, true);
        true
    }

    /// The element of `node` in the document of `frame`.
    fn element_of_in(&mut self, frame: FrameId, node: NodeId) -> Option<NodeId> {
        self.in_frame(frame, |p| p.element_of(node)).flatten()
    }

    /// Drops what the page keeps for frame `id`, which is destroyed: its
    /// input state, its hover and press, and the focus that it held (the
    /// focus falls to its parent).
    pub(super) fn forget_frame_input(&mut self, id: FrameId) {
        self.frame_inputs.remove(&id);
        self.input.hover_frames.retain(|&f| f != id);
        if self
            .input
            .press_frame
            .as_ref()
            .is_some_and(|p| p.frame == id)
        {
            self.input.press_frame = None;
        }
        if self.input.focus_frame != Some(id) {
            return;
        }
        let parent = self.frames.get(id).and_then(|f| f.parent);
        self.input.focus_frame = parent.filter(|&p| p != FrameId::TOP);
        self.sync_window_focus();
        match parent {
            Some(FrameId::TOP) | None => {
                if let Some(doc) = self.doc.as_deref_mut() {
                    doc.frame_focus = None;
                }
            }
            Some(parent) => {
                if let Some(doc) = self
                    .frames
                    .get_mut(parent)
                    .and_then(|f| f.doc.as_deref_mut())
                {
                    doc.frame_focus = None;
                }
            }
        }
    }

    /// The pointer moved to `point` while a press holds it in frame
    /// `press.frame`: the frame sees the move at its own point, also
    /// outside its viewport; the other documents see nothing. Returns true
    /// if a repaint is needed.
    pub(super) fn move_in_pressed_frame(&mut self, press: &FramePress, point: Point) -> bool {
        let local = Point::new(point.x - press.offset.x, point.y - press.offset.y);
        self.in_frame(press.frame, |p| {
            p.input.pointer = Some(local);
            let changed = p.drag_press(local);
            changed | p.hover_while_pressed(local)
        })
        .unwrap_or(false)
    }

    /// The hover state of a document that keeps a press while the pointer
    /// moves: the element under it, and the root element when the pointer
    /// is outside the viewport (measured in Chromium 148, case
    /// `input-press-in-frame`: the frame then sees the pointer on its
    /// `html` element). Returns true if a repaint is needed.
    fn hover_while_pressed(&mut self, point: Point) -> bool {
        let viewport = self.view.viewport;
        let inside = point.x >= 0.0
            && point.y >= 0.0
            && point.x <= viewport.width
            && point.y <= viewport.height;
        let hit = if inside {
            self.hit_doc(point.x, point.y)
        } else {
            None
        };
        let mut element = hit.and_then(|h| self.element_of(h.node));
        if !inside {
            element = self.doc.as_ref().and_then(|d| d.dom().document_element());
        }
        self.update_states(|s| s.hover = element)
    }

    /// The release of the primary button of a press that started in a
    /// frame: the frame gets it at its own point. Returns whether a
    /// repaint is needed and whether the click activated something.
    pub(super) fn release_in_frame(&mut self, press: &FramePress, point: Point) -> (bool, bool) {
        let local = Point::new(point.x - press.offset.x, point.y - press.offset.y);
        let mut changed = false;
        for &frame in &press.chain[..press.chain.len().saturating_sub(1)] {
            changed |= self
                .in_frame(frame, |p| p.update_states(|s| s.active = None))
                .unwrap_or(false);
        }
        let (inner, activated) = self
            .in_frame(press.frame, |p| {
                p.release_in_document(local, MouseButton::Primary)
            })
            .unwrap_or((false, false));
        (changed | inner, activated)
    }

    // ----- Wheel -----

    /// The wheel over the content of a frame (`route` goes into it):
    /// scrolls the first scroll container in the chain that can scroll in
    /// the direction of the delta, from the innermost scroller of the
    /// innermost document, through the viewport of each frame (not for a
    /// frame with `scrolling=no`) to the parent's chain from the `iframe`
    /// (ADR 0019 and ADR 0031 section 8; measured: a wheel that reaches the
    /// end of a frame does not move the parent in the same event). The
    /// documents get no `wheel` event: the page's scripts see none for a
    /// wheel over a frame (measured). Returns true if something scrolled.
    pub(super) fn wheel_in_frames(&mut self, route: &Route, dx: f32, dy: f32) -> bool {
        let mut scrolled = false;
        for step in route.steps.iter().rev() {
            let user = self.frame_user_scrollable(step.frame);
            let start = step.node;
            let viewport = if user { (dx, dy) } else { (0.0, 0.0) };
            scrolled = self
                .in_frame(step.frame, |p| p.user_scroll(start, |_| (dx, dy), viewport))
                .unwrap_or(false);
            if scrolled {
                break;
            }
        }
        scrolled
    }
}
