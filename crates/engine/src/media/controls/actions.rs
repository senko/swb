//! The actions of the default controls: what a press on the play button, the
//! picture, the timeline or the volume slider does through the playback API
//! of the element ([`Media::controls_play`] to [`Media::controls_end_drag`]).
//! A part of the module `controls`, moved out of one file unchanged.

use swb_dom::NodeId;
use swb_layout::MediaButton;

use super::Drag;
use crate::media::{Cx, Element, Media};

impl Media {
    /// `play()` from the controls: the play steps without a promise's use
    /// (the promise has no listener; its settling is harmless).
    fn controls_play(&mut self, cx: &Cx<'_>, node: NodeId) {
        let _ = self.play(cx, node);
    }

    /// The play button or a click on the picture: play if paused (an ended
    /// media starts again), else pause.
    pub(crate) fn controls_toggle(&mut self, cx: &Cx<'_>, node: NodeId) {
        let Some(e) = self.elements.get(&node) else {
            return;
        };
        if e.paused {
            self.controls_play(cx, node);
        } else {
            self.pause(cx, node);
        }
    }

    /// A click on the picture: it toggles when the metadata is known.
    pub(crate) fn controls_click_picture(&mut self, cx: &Cx<'_>, node: NodeId) {
        if self.elements.get(&node).is_some_and(|e| e.meta.is_some()) {
            self.controls_toggle(cx, node);
        }
    }

    /// A click on a button. Returns true if it did something.
    pub(crate) fn controls_click_button(
        &mut self,
        cx: &Cx<'_>,
        node: NodeId,
        button: MediaButton,
    ) -> bool {
        let Some(e) = self.elements.get(&node) else {
            return false;
        };
        match button {
            MediaButton::Play if e.can_play_from_controls() => {
                self.controls_toggle(cx, node);
                true
            }
            MediaButton::Mute if e.can_mute() => {
                let muted = e.muted;
                self.set_muted(cx, node, !muted);
                true
            }
            // Fullscreen and the overflow menu come with M15 and later.
            _ => false,
        }
    }

    /// A press on the timeline at `fraction` of the duration: pause if it
    /// plays, seek, and begin the drag. False if the media cannot seek.
    pub(crate) fn controls_timeline_press(
        &mut self,
        cx: &Cx<'_>,
        node: NodeId,
        fraction: f64,
    ) -> bool {
        let Some(e) = self.elements.get(&node) else {
            return false;
        };
        if !e.can_seek() {
            return false;
        }
        let was_playing = !e.paused;
        if was_playing {
            self.pause(cx, node);
        }
        if let Some(e) = self.elements.get_mut(&node) {
            e.controls.drag = Some(Drag::Timeline { was_playing });
            e.controls.focus_timeline = true;
        }
        self.controls_seek_to(cx, node, fraction);
        true
    }

    /// Seeks to `fraction` of the duration (the timeline's move).
    pub(crate) fn controls_seek_to(&mut self, cx: &Cx<'_>, node: NodeId, fraction: f64) {
        let Some(duration) = self.elements.get(&node).and_then(Element::finite_duration) else {
            return;
        };
        self.seek(cx, node, (fraction * duration).clamp(0.0, duration));
    }

    /// The end of a drag of the timeline: what played plays again.
    pub(crate) fn controls_timeline_release(&mut self, cx: &Cx<'_>, node: NodeId) {
        let Some(e) = self.elements.get_mut(&node) else {
            return;
        };
        let Some(Drag::Timeline { was_playing }) = e.controls.drag.take() else {
            return;
        };
        if was_playing {
            self.controls_play(cx, node);
        }
    }

    /// A press or move on the volume slider: `volume = value`, and a muted
    /// element is unmuted when the value is above 0 (measured in Chromium
    /// 148, `in-slider-muted-*`: a `volumechange` for the volume if it
    /// changed, and one for `muted = false`; a press at 0 on a muted element
    /// does nothing).
    pub(crate) fn controls_set_volume(&mut self, cx: &Cx<'_>, node: NodeId, volume: f64) {
        if !self.elements.get(&node).is_some_and(Element::can_mute) {
            return;
        }
        let volume = volume.clamp(0.0, 1.0);
        self.set_volume(cx, node, volume);
        if volume > 0.0 && self.elements.get(&node).is_some_and(|e| e.muted) {
            self.set_muted(cx, node, false);
        }
    }

    /// Ends the drag of an element (a release, or a state that makes the
    /// drag meaningless: the element left the document, its source changed).
    pub(crate) fn controls_end_drag(&mut self, cx: &Cx<'_>, node: NodeId) {
        match self.elements.get(&node).and_then(|e| e.controls.drag) {
            Some(Drag::Timeline { .. }) => self.controls_timeline_release(cx, node),
            Some(Drag::Volume) => {
                if let Some(e) = self.elements.get_mut(&node) {
                    e.controls.drag = None;
                }
            }
            None => {}
        }
    }
}
