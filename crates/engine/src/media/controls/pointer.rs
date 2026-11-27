//! The pointer of the default controls: the hover, the press and the drag
//! on the controls of the media elements of a document (`Media::ui_*`), the
//! places of the media boxes, the shaped time labels and the stale and
//! changed views ([`PointerState`]). A part of the module `controls`, moved
//! out of one file unchanged.

use std::sync::Arc;

use swb_dom::NodeId;
use swb_layout::{
    MediaButton, MediaContent, MediaControlsView, MediaHit, MediaPartKind, MediaText, Point, Rect,
    THUMB_BAND, media_hit, timeline_fraction, volume_fraction, volume_slider,
};

use super::{ControlsKey, Drag};
use crate::media::{Cx, Media};

/// The area of the first part of `content` that `kind` accepts.
fn part_rect(content: &MediaContent, kind: fn(&MediaPartKind) -> bool) -> Option<Rect> {
    content.parts.iter().find(|p| kind(&p.kind)).map(|p| p.rect)
}

impl Media {
    // ----- The pointer -----

    /// The media element `node` that the pointer can act on: it has a box
    /// with controls and a state.
    fn ui_target(&self, node: Option<NodeId>) -> Option<NodeId> {
        node.filter(|n| self.ui.boxes.contains_key(n) && self.elements.contains_key(n))
    }

    /// The pointer is at `painted` (painted document coordinates) over the
    /// node `target` (the innermost one the hit test found): the hover state
    /// of the controls. A repeat of the same point is no move (it does not
    /// show hidden controls). Returns true if a state changed.
    pub(crate) fn ui_pointer(
        &mut self,
        target: Option<NodeId>,
        painted: Point,
        now_us: i64,
    ) -> bool {
        let target = self.ui_target(target);
        let mut changed = false;
        if self.ui.hover != target
            && let Some(previous) = self.ui.hover.take()
        {
            changed |= self.ui_left(previous, now_us);
        }
        let Some(node) = target else {
            return changed;
        };
        let Some(b) = self.ui.boxes.get(&node) else {
            return changed;
        };
        let (local, content, size) = (b.local(painted), Arc::clone(&b.content), b.size);
        let Some(e) = self.elements.get_mut(&node) else {
            return changed;
        };
        let hit = media_hit(&content, local, e.slider_open());
        let band = e.video && local.y >= size.height - THUMB_BAND;
        let c = &mut e.controls;
        let moved = c.last != Some(local) || !c.inside;
        changed |= (c.hit, c.band, c.inside) != (hit, band, true) || moved;
        c.hit = hit;
        c.band = band;
        if moved {
            c.moved_inside(now_us);
            c.last = Some(local);
        }
        self.ui.hover = Some(node);
        changed
    }

    /// The pointer left the element `node`.
    fn ui_left(&mut self, node: NodeId, now_us: i64) -> bool {
        let Some(e) = self.elements.get_mut(&node) else {
            return false;
        };
        e.controls.left(now_us);
        e.controls.band = false;
        e.controls.last = None;
        true
    }

    /// The pointer left the document's content.
    pub(crate) fn ui_leave(&mut self, now_us: i64) -> bool {
        match self.ui.hover.take() {
            Some(node) => self.ui_left(node, now_us),
            None => false,
        }
    }

    /// A press of the primary button at `painted` over the node `target`.
    /// Returns true if it is on a control: the page gets no event for it.
    pub(crate) fn ui_down(&mut self, cx: &Cx<'_>, target: Option<NodeId>, painted: Point) -> bool {
        self.ui.picture = None;
        let node = self.ui_target(target);
        // A press takes the focus from the control that had it (the
        // timeline or the mute button: its thumb or the volume slider stays
        // open while it has the focus).
        if let Some(focused) = self.ui.focused.take()
            && let Some(e) = self.elements.get_mut(&focused)
        {
            e.controls.focus_timeline = false;
            e.controls.focus_volume = false;
        }
        let Some(node) = node else {
            return false;
        };
        let Some(b) = self.ui.boxes.get(&node) else {
            return false;
        };
        let (local, content) = (b.local(painted), Arc::clone(&b.content));
        let Some(e) = self.elements.get_mut(&node) else {
            return false;
        };
        let hit = media_hit(&content, local, e.slider_open());
        self.ui.pressed = Some(Pressed {
            node,
            hit,
            audio: content.audio,
        });
        match hit {
            // An `<audio>` has no picture: its whole panel is the controls,
            // and a press on it outside the parts gives the page no event
            // and does nothing (Chromium 148, `fmt-audio-panel-click` in
            // `tools/probes/media-exit.json`; M14 feature 10c).
            MediaHit::None => content.audio,
            MediaHit::Button(_) => true,
            MediaHit::Timeline => {
                if let Some(bar) = part_rect(&content, |k| matches!(k, MediaPartKind::Timeline)) {
                    let fraction = timeline_fraction(bar, local.x);
                    if self.controls_timeline_press(cx, node, fraction) {
                        self.ui.focused = Some(node);
                    }
                }
                true
            }
            MediaHit::Slider => {
                if let Some(mute) = part_rect(&content, |k| matches!(k, MediaPartKind::Mute)) {
                    let s = volume_slider(content.audio, mute);
                    if let Some(e) = self.elements.get_mut(&node)
                        && e.can_mute()
                    {
                        e.controls.drag = Some(Drag::Volume);
                        e.controls.focus_volume = true;
                        self.ui.focused = Some(node);
                    }
                    self.controls_set_volume(cx, node, volume_fraction(&s, local.x));
                }
                true
            }
        }
    }

    /// The pointer moved to `painted` with a press in progress: a drag of the
    /// timeline or the slider follows it, also outside the element.
    pub(crate) fn ui_drag(&mut self, cx: &Cx<'_>, painted: Point) -> bool {
        let Some(pressed) = self.ui.pressed else {
            return false;
        };
        let Some(drag) = self
            .elements
            .get(&pressed.node)
            .and_then(|e| e.controls.drag)
        else {
            return false;
        };
        let Some(b) = self.ui.boxes.get(&pressed.node) else {
            return false;
        };
        let (local, content) = (b.local(painted), Arc::clone(&b.content));
        match drag {
            Drag::Timeline { .. } => {
                if let Some(bar) = part_rect(&content, |k| matches!(k, MediaPartKind::Timeline)) {
                    self.controls_seek_to(cx, pressed.node, timeline_fraction(bar, local.x));
                }
            }
            Drag::Volume => {
                if let Some(mute) = part_rect(&content, |k| matches!(k, MediaPartKind::Mute)) {
                    let s = volume_slider(content.audio, mute);
                    self.controls_set_volume(cx, pressed.node, volume_fraction(&s, local.x));
                }
            }
        }
        true
    }

    /// The release of the primary button at `painted`: ends a drag, clicks a
    /// button that the pointer is still over. Returns true if the press was
    /// on a control (the page gets no event for it); a press on the picture
    /// is left to its click ([`Media::ui_picture_click`]).
    pub(crate) fn ui_up(&mut self, cx: &Cx<'_>, painted: Point) -> bool {
        let Some(pressed) = self.ui.pressed.take() else {
            return false;
        };
        let node = pressed.node;
        match pressed.hit {
            MediaHit::None => {
                // The panel of an `<audio>` is the controls (see `ui_down`).
                if !pressed.audio {
                    self.ui.picture = Some(node);
                }
                pressed.audio
            }
            MediaHit::Button(button) => {
                let over = self.ui.boxes.get(&node).is_some_and(|b| {
                    media_hit(&b.content, b.local(painted), false) == MediaHit::Button(button)
                });
                if over
                    && self.controls_click_button(cx, node, button)
                    && button == MediaButton::Mute
                {
                    // The clicked mute button has the focus: its slider
                    // stays open when the pointer goes (measured).
                    if let Some(e) = self.elements.get_mut(&node) {
                        e.controls.focus_volume = true;
                    }
                    self.ui.focused = Some(node);
                }
                true
            }
            MediaHit::Timeline | MediaHit::Slider => {
                self.controls_end_drag(cx, node);
                true
            }
        }
    }

    /// The `click` event of a press on the picture of `target` was not
    /// canceled: it plays or pauses.
    pub(crate) fn ui_picture_click(&mut self, cx: &Cx<'_>, target: NodeId) -> bool {
        if self.ui.picture.take() != Some(target) {
            return false;
        }
        self.controls_click_picture(cx, target);
        true
    }

    /// Ends a press whose element is gone (removed from the document).
    pub(crate) fn ui_element_gone(&mut self, node: NodeId) {
        if self.ui.hover == Some(node) {
            self.ui.hover = None;
        }
        if self.ui.pressed.is_some_and(|p| p.node == node) {
            self.ui.pressed = None;
        }
        if self.ui.focused == Some(node) {
            self.ui.focused = None;
        }
        if self.ui.picture == Some(node) {
            self.ui.picture = None;
        }
    }

    /// The page time (ms) of the next change of the controls' picture of any
    /// element (see [`Element::next_due_us`]).
    pub(crate) fn controls_next_due_ms(&self, now_us: i64) -> Option<f64> {
        let animate = self.ctx.viewport_rule();
        self.elements
            .values()
            .filter_map(|e| e.next_due_us(now_us, animate))
            .min()
            .map(|us| us as f64 / 1000.0)
    }

    /// True if the picture of the controls of some element is outdated:
    /// its key at `now_us` differs from the one in `shown` (the key that the
    /// last built view had, or the default). `shown` returns it for a node.
    pub(crate) fn controls_stale(
        &self,
        now_us: i64,
        shown: impl Fn(NodeId) -> ControlsKey,
    ) -> bool {
        let animate = self.ctx.viewport_rule();
        self.elements
            .iter()
            .any(|(&node, e)| e.controls_key(now_us, animate) != shown(node))
    }

    /// The elements whose controls' picture at `now_us` differs from the one
    /// that `shown` tells (the key of the view that the display list has):
    /// each with its new key and view (`None` for the state before any
    /// playback, which is the default look). Settles the fades; shapes the
    /// time labels that changed with `fonts`.
    pub(crate) fn controls_changes(
        &mut self,
        now_us: i64,
        fonts: &mut swb_text::FontContext,
        shown: impl Fn(NodeId) -> ControlsKey,
    ) -> Vec<(NodeId, ControlsKey, Option<Arc<MediaControlsView>>)> {
        let animate = self.ctx.viewport_rule();
        let Media { elements, ui, .. } = self;
        let mut changes = Vec::new();
        for (&node, e) in elements.iter_mut() {
            e.controls.settle(e.paused, now_us);
            let key = e.controls_key(now_us, animate);
            if key == shown(node) {
                continue;
            }
            let view = (key != ControlsKey::default()).then(|| {
                let times = key.labels().map(|(current, full)| {
                    let (current, _) = ui.labels.get(fonts, &current);
                    let (full, width) = ui.labels.get(fonts, &full);
                    (current, full, width)
                });
                Arc::new(key.view(times))
            });
            changes.push((node, key, view));
        }
        changes
    }
}

/// The place of a media box with controls: what the pointer's coordinates map
/// to.
#[derive(Clone, Debug)]
pub(crate) struct ControlsBox {
    /// The parts of the controls.
    pub(crate) content: Arc<MediaContent>,
    /// The content box's size.
    pub(crate) size: swb_layout::Size,
    /// The content box's origin in the coordinates of the box fragment's
    /// border box (before the transforms).
    pub(crate) origin: Point,
    /// Painted document coordinates to those of the fragment (the inverse
    /// of the transforms; the identity if they cannot be inverted).
    pub(crate) inverse: swb_layout::Matrix,
}

impl ControlsBox {
    /// The point `painted` (painted document coordinates) in the content
    /// box.
    pub(crate) fn local(&self, painted: Point) -> Point {
        let p = self.inverse.apply(painted);
        Point::new(p.x - self.origin.x, p.y - self.origin.y)
    }
}

/// A press on the controls that has not ended.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Pressed {
    /// The element.
    pub(crate) node: NodeId,
    /// What the press started on: the picture ([`MediaHit::None`]) or a
    /// control.
    pub(crate) hit: MediaHit,
    /// Whether the element is an `<audio>` (at the press: its panel is the
    /// controls, and a press outside the parts is no picture click).
    pub(crate) audio: bool,
}

/// The pointer state of the default controls of one document's media
/// elements (the hovered element, the press, the focus, the places of the
/// media boxes and the shaped labels).
#[derive(Default)]
pub(crate) struct PointerState {
    /// The element whose box the pointer is in.
    pub(crate) hover: Option<NodeId>,
    /// The press in progress.
    pub(crate) pressed: Option<Pressed>,
    /// The element whose timeline has the focus.
    pub(crate) focused: Option<NodeId>,
    /// The element whose picture a press and release hit: its `click` event
    /// plays or pauses it when no listener cancels it.
    pub(crate) picture: Option<NodeId>,
    /// Shaped time labels.
    pub(crate) labels: LabelCache,
    /// The page time (microseconds) that the page last gave: the picture of
    /// the controls is as of this time.
    pub(crate) now_us: i64,
    /// What the places of `boxes` depend on: the layout serial, the scroll
    /// offsets of the scroll containers and the viewport's scroll offset.
    pub(crate) boxes_key: Option<(u64, u64, Point)>,
    /// The media boxes with controls (one walk of the fragment tree for all).
    pub(crate) boxes: std::collections::HashMap<NodeId, ControlsBox>,
}

/// Shaped time labels, cached by their text: the controls shape two labels a
/// second and the same labels come back.
#[derive(Default)]
pub(crate) struct LabelCache {
    labels: std::collections::HashMap<String, (MediaText, f32)>,
}

impl LabelCache {
    /// The shaped label `text` and its advance.
    pub(crate) fn get(
        &mut self,
        fonts: &mut swb_text::FontContext,
        text: &str,
    ) -> (MediaText, f32) {
        if self.labels.len() > 256 {
            self.labels.clear();
        }
        self.labels
            .entry(text.to_owned())
            .or_insert_with(|| swb_layout::media_time_text(fonts, text))
            .clone()
    }
}
