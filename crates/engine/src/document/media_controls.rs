//! The document's side of the default controls of media elements (M14
//! feature 9, ADR 0047 "Implementation notes (M14 feature 9)"): where the
//! controls' boxes are (one walk of the fragment tree for all of them, kept
//! while the layout, the display list and the scroll offset stay), the
//! pointer's presses and drags (`crate::media::controls`), and the views that
//! paint draws. A change of a view rebuilds the display list, not the layout;
//! the damage of the frame is the controls that changed.

use std::sync::Arc;

use swb_dom::NodeId;
use swb_layout::{BoxContent, FragmentRef, Point};

use super::DocumentState;
use crate::media::Cx;
use crate::media::controls::ControlsBox;

impl DocumentState {
    /// Builds the places of the controls' boxes when the layout, the display
    /// list or the scroll offset changed since the last walk. Nothing when
    /// the document has no media element.
    fn refresh_controls_boxes(&mut self) {
        if self.media.is_empty() {
            return;
        }
        // The display list's serial is not part of the key: a change of a
        // view builds the list again, and that would walk the tree again.
        let offsets = self
            .scrollers
            .offsets()
            .iter()
            .fold(0u64, |acc, (node, p)| {
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                std::hash::Hash::hash(&(node, p.x.to_bits(), p.y.to_bits()), &mut hasher);
                acc ^ std::hash::Hasher::finish(&hasher)
            });
        let key = (self.layout_serial, offsets, self.scroll);
        if self.media.ui.boxes_key == Some(key) {
            return;
        }
        self.media.ui.boxes_key = Some(key);
        self.media.ui.boxes.clear();
        let Some(tree) = &self.fragments else {
            return;
        };
        let mut found = Vec::new();
        tree.walk_painted(
            self.scrollers.offsets(),
            self.scroll,
            |fragment, rect, matrix| {
                if let FragmentRef::Box(b) = fragment
                    && let (Some(node), BoxContent::Media(content)) = (b.node, &b.content)
                    && !content.parts.is_empty()
                {
                    let content_box = b.content_rect();
                    let delta = rect.origin() - b.border_rect.origin();
                    let size = swb_layout::Size::new(content_box.width, content_box.height);
                    found.push((
                        node,
                        ControlsBox {
                            content: Arc::clone(content),
                            size,
                            origin: Point::new(content_box.x + delta.x, content_box.y + delta.y),
                            inverse: matrix.invert().unwrap_or(swb_layout::Matrix::IDENTITY),
                        },
                    ));
                }
            },
        );
        self.media.ui.boxes.extend(found);
        // An element draws the default controls when it has a box with
        // them; the others keep the default key (no view, no repaint).
        let ui = &self.media.ui;
        for (node, e) in &mut self.media.elements {
            e.controls.present = ui.boxes.contains_key(node);
        }
    }

    /// The context of the media state (`with_media`) with the page time of
    /// the controls' last call.
    fn with_controls<R>(
        &mut self,
        now_us: i64,
        f: impl FnOnce(&mut crate::media::Media, &Cx<'_>) -> R,
    ) -> R {
        self.media.now_us = now_us;
        let now_ms = now_us as f64 / 1000.0;
        self.with_media(now_ms, f)
    }

    /// The pointer is at `point` (viewport coordinates) over the node
    /// `target`: the hover state of the controls. True if one changed.
    pub(crate) fn controls_pointer(
        &mut self,
        target: Option<NodeId>,
        point: Point,
        now_us: i64,
    ) -> bool {
        if self.media.is_empty() {
            return false;
        }
        self.refresh_controls_boxes();
        let painted = Point::new(point.x + self.scroll.x, point.y + self.scroll.y);
        self.media.ui_pointer(target, painted, now_us)
    }

    /// The pointer left the document.
    pub(crate) fn controls_leave(&mut self, now_us: i64) -> bool {
        !self.media.is_empty() && self.media.ui_leave(now_us)
    }

    /// A press of the primary button at `point` over `target`. True if it is
    /// on a control: the page's scripts get no event for it.
    pub(crate) fn controls_down(
        &mut self,
        target: Option<NodeId>,
        point: Point,
        now_us: i64,
    ) -> bool {
        if self.media.is_empty() {
            return false;
        }
        self.refresh_controls_boxes();
        let painted = Point::new(point.x + self.scroll.x, point.y + self.scroll.y);
        self.with_controls(now_us, |media, cx| media.ui_down(cx, target, painted))
    }

    /// True if a press on a control is in progress.
    pub(crate) fn controls_pressed(&self) -> bool {
        self.media.ui.pressed.is_some()
    }

    /// The pointer moved to `point` with the primary button down: a drag on
    /// the timeline or the volume slider follows. True if one runs.
    pub(crate) fn controls_drag(&mut self, point: Point, now_us: i64) -> bool {
        if self.media.is_empty() || self.media.ui.pressed.is_none() {
            return false;
        }
        self.refresh_controls_boxes();
        let painted = Point::new(point.x + self.scroll.x, point.y + self.scroll.y);
        let changed = self.with_controls(now_us, |media, cx| media.ui_drag(cx, painted));
        self.sync_media_images(None);
        changed
    }

    /// The release of the primary button at `point`. True if the press was on
    /// a control (the page's scripts get no event for it).
    pub(crate) fn controls_up(&mut self, point: Point, now_us: i64) -> bool {
        if self.media.is_empty() || self.media.ui.pressed.is_none() {
            return false;
        }
        self.refresh_controls_boxes();
        let painted = Point::new(point.x + self.scroll.x, point.y + self.scroll.y);
        let swallowed = self.with_controls(now_us, |media, cx| media.ui_up(cx, painted));
        self.sync_media_images(None);
        swallowed
    }

    /// The `click` of a press on the picture of `target` was not canceled.
    pub(crate) fn controls_picture_click(&mut self, target: NodeId, now_us: i64) -> bool {
        if self.media.is_empty() {
            return false;
        }
        let acted = self.with_controls(now_us, |media, cx| media.ui_picture_click(cx, target));
        self.sync_media_images(None);
        acted
    }

    /// Brings the views of the controls up to date for the page time
    /// `now_us` (what the last call of the page said): shapes the time labels
    /// that changed. Returns true if a view changed; the display list is then
    /// built again (the layout is not).
    pub(crate) fn update_media_controls(&mut self, fonts: &mut swb_text::FontContext) -> bool {
        if self.media.is_empty() && self.images.no_controls() {
            return false;
        }
        self.refresh_controls_boxes();
        let now = self.media.ui.now_us;
        let images = &self.images;
        let changes = self
            .media
            .controls_changes(now, fonts, |node| images.controls_key(node));
        let mut changed = !changes.is_empty();
        for (node, key, view) in changes {
            self.images.set_controls(node, key, view);
        }
        let elements = &self.media.elements;
        changed |= self
            .images
            .retain_controls(|node| elements.contains_key(&node));
        if changed {
            self.display_list = None;
        }
        changed
    }

    /// True if the picture of some controls differs from the one that the
    /// display list has, at the page time `now_us`.
    pub(crate) fn media_controls_stale(&self, now_us: i64) -> bool {
        !self.media.is_empty()
            && self
                .media
                .controls_stale(now_us, |node| self.images.controls_key(node))
    }

    /// The page time (ms) of the next change of the controls without input.
    pub(crate) fn media_controls_next_due(&self, now_us: i64) -> Option<f64> {
        self.media.controls_next_due(now_us)
    }

    /// Tells the controls the page time of the next picture.
    pub(crate) fn set_controls_time(&mut self, now_us: i64) {
        self.media.ui.now_us = now_us;
    }
}
