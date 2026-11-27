//! The default controls of `<video>` and `<audio>` (M14 feature 9, ADR 0047
//! "Implementation notes (M14 feature 9)", ADR 0020 "Update: playback"): the
//! state of the pointer and the auto-hide, the key and the view of what the
//! controls show, and the actions of the buttons, the timeline and the volume
//! slider. Layout decides the parts and their places (`swb_layout::media`),
//! paint draws a [`MediaControlsView`] (`swb_paint::media`).
//!
//! **The actions go through the playback API of the element** (`play`,
//! `pause`, `seek`, `set_volume`, `set_muted`), so the events, the autoplay
//! rules and the promises are the script API's. A press on a control is a
//! user activation (the page notes it before the press arrives here).
//!
//! **Chromium's behavior** (probes `tools/probes/media-controls-input.json`,
//! `-states.json`, `-fade.json`, `-hit.json`, `-sizes.json`; three equal
//! runs):
//!
//! - A press, release or click on a control (a button, the timeline, the
//!   volume slider, enabled or not) gives the page no pointer, mouse or
//!   click event at all. A click on the picture does (at the element; they
//!   are trusted), and plays or pauses after the `click` event if no
//!   listener canceled it and the metadata is known (`readyState` at least
//!   1); without metadata (`preload=none`, an error, no source) it does
//!   nothing.
//! - The play button starts a load that `preload=none` held back; it is
//!   disabled by an error or the lack of a source. The mute button and the
//!   timeline need the metadata (the mute button an audio track too). A
//!   disabled control does nothing.
//! - The timeline: a press seeks (`pause` first if it plays) and starts a drag
//!   whose moves seek again, also outside the element; the release resumes
//!   (`play`) what played. The thumb's center maps 6 px inside the bar's ends
//!   (0 to 1). A media without a finite duration or that cannot seek ignores
//!   it.
//! - The volume slider opens over the mute button (and stays open over its
//!   pill), a press or drag on it sets the volume (the thumb center maps
//!   6 px inside the track's ends).
//! - Auto-hide of the video controls: they stay while the element is paused
//!   or ended, and while it plays for 2.5 s after the last pointer move
//!   inside or the start; then they fade out in 0.7 s (linear in the model;
//!   Chromium's curve is close). When the pointer leaves they fade out at
//!   once. A move fades them in (0.1 s).
//! - The loading arc shows between 0.1 s and 1.1 s after a load starts.
//!
//! The state changes of the controls that are not the element's (the pointer,
//! the fade) are in [`Controls`]; [`Element::controls_key`] gathers what
//! decides the picture (without shaping any text) and
//! [`ControlsKey::view`] makes the view.

use std::sync::Arc;

use swb_dom::NodeId;
use swb_layout::{
    MediaButton, MediaContent, MediaControlsView, MediaHit, MediaPartKind, MediaText, MediaTimes,
    Point, Rect, THUMB_BAND, media_hit, timeline_fraction, volume_fraction, volume_slider,
};

use super::{Element, Media};
use crate::media::Cx;

/// The area of the first part of `content` that `kind` accepts.
fn part_rect(content: &MediaContent, kind: fn(&MediaPartKind) -> bool) -> Option<Rect> {
    content.parts.iter().find(|p| kind(&p.kind)).map(|p| p.rect)
}

/// The controls of a playing video hide this long after the last pointer
/// move inside it (or the start).
pub(crate) const HIDE_AFTER_US: i64 = 2_500_000;

/// The fade out and the fade in, in microseconds.
const FADE_OUT_US: i64 = 700_000;
const FADE_IN_US: i64 = 100_000;

/// The loading arc: when it appears and goes (after the load starts), and the
/// time of one turn.
const SPINNER_FROM_US: i64 = 100_000;
const SPINNER_TO_US: i64 = 1_100_000;
const SPINNER_TURN_US: i64 = 1_400_000;

/// The steps of the opacity and of the arc's angle in the key: a repaint at
/// each (about 60 Hz for the opacity).
const OPACITY_STEPS: f32 = 64.0;
const ANGLE_STEPS: i64 = 64;

/// How often the page wakes for an animation of the controls.
const ANIMATION_TICK_US: i64 = 16_000;

/// A drag that a press on the controls started.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Drag {
    /// The timeline: whether the media played when the press came (it
    /// plays again at the release).
    Timeline { was_playing: bool },
    /// The volume slider.
    Volume,
}

/// A fade of the video controls between hidden (0) and shown (1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Fade {
    /// The opacity when it started.
    from: f32,
    /// True if it goes to shown.
    to_shown: bool,
    /// The page time (microseconds) when it started.
    start_us: i64,
}

impl Fade {
    /// The controls are shown, from the start.
    const SHOWN: Fade = Fade {
        from: 1.0,
        to_shown: true,
        start_us: 0,
    };

    /// The opacity at `now_us`.
    fn at(&self, now_us: i64) -> f32 {
        let (target, duration) = if self.to_shown {
            (1.0, FADE_IN_US)
        } else {
            (0.0, FADE_OUT_US)
        };
        let elapsed = now_us.saturating_sub(self.start_us).max(0);
        let p = (elapsed as f32 / duration as f32).clamp(0.0, 1.0);
        self.from + (target - self.from) * p
    }

    /// The fade after the controls ought to be `shown` since `since_us`: the
    /// same fade if it goes there already.
    fn retarget(&self, shown: bool, since_us: i64) -> Fade {
        if self.to_shown == shown {
            return *self;
        }
        Fade {
            from: self.at(since_us),
            to_shown: shown,
            start_us: since_us,
        }
    }

    /// True while the opacity still moves at `now_us`.
    fn running(&self, now_us: i64) -> bool {
        let duration = if self.to_shown {
            FADE_IN_US
        } else {
            FADE_OUT_US
        };
        now_us.saturating_sub(self.start_us) < duration
            && (self.at(now_us) - if self.to_shown { 1.0 } else { 0.0 }).abs() > f32::EPSILON
    }
}

/// What the pointer and the user did to the controls of one element.
#[derive(Clone, Debug)]
pub(crate) struct Controls {
    /// What is under the pointer: [`MediaHit::None`] is the picture.
    pub(crate) hit: MediaHit,
    /// The pointer is inside the element's box.
    pub(crate) inside: bool,
    /// The page time of the last pointer move inside the element, or of the
    /// start of the playback.
    pub(crate) activity_us: Option<i64>,
    /// The page time when the pointer left the element, if it did since
    /// the last move inside.
    pub(crate) left_us: Option<i64>,
    /// The drag of a press on the timeline or the slider.
    pub(crate) drag: Option<Drag>,
    /// The timeline has the focus (a press on it): its thumb stays.
    pub(crate) focus_timeline: bool,
    /// The mute button or the slider has the focus (a press on it): the
    /// slider stays open.
    pub(crate) focus_volume: bool,
    /// The pointer is in the bottom 25 px of a video (measured: the
    /// timeline's thumb shows there, over the whole width).
    pub(crate) band: bool,
    /// The last place of the pointer in the content box (a repeat is no move).
    pub(crate) last: Option<Point>,
    /// The fade of the video controls.
    pub(crate) fade: Fade,
    /// The page time (microseconds) when the last load started: the arc.
    pub(crate) loading_us: Option<i64>,
    /// The element has a box with default controls (the document sets it
    /// after each layout).
    pub(crate) present: bool,
}

impl Default for Controls {
    fn default() -> Controls {
        Controls {
            present: false,
            hit: MediaHit::None,
            inside: false,
            band: false,
            last: None,
            activity_us: None,
            left_us: None,
            drag: None,
            focus_timeline: false,
            focus_volume: false,
            fade: Fade::SHOWN,
            loading_us: None,
        }
    }
}

impl Controls {
    /// Whether the video controls ought to show at `now_us`, and since when
    /// that holds: shown while the element is paused (or ended) or a drag
    /// runs; while it plays, from the last activity for [`HIDE_AFTER_US`],
    /// and hidden from the moment that the pointer left.
    fn wanted(&self, paused: bool, now_us: i64) -> (bool, i64) {
        if paused || self.drag.is_some() {
            return (true, now_us);
        }
        if !self.inside
            && let Some(left) = self.left_us
        {
            return (false, left);
        }
        let activity = self.activity_us.unwrap_or(0);
        let deadline = activity.saturating_add(HIDE_AFTER_US);
        if now_us >= deadline {
            (false, deadline)
        } else {
            (true, activity)
        }
    }

    /// The opacity of the controls at `now_us` and the fade that has it.
    fn opacity(&self, paused: bool, now_us: i64) -> (f32, Fade) {
        let (shown, since) = self.wanted(paused, now_us);
        let fade = self.fade.retarget(shown, since.min(now_us));
        (fade.at(now_us), fade)
    }

    /// Stores the fade of [`Controls::opacity`] (a later look continues it).
    pub(crate) fn settle(&mut self, paused: bool, now_us: i64) {
        self.fade = self.opacity(paused, now_us).1;
    }

    /// A pointer move inside the element at `now_us`.
    pub(crate) fn moved_inside(&mut self, now_us: i64) {
        self.inside = true;
        self.activity_us = Some(now_us);
        self.left_us = None;
    }

    /// The pointer left the element at `now_us`.
    pub(crate) fn left(&mut self, now_us: i64) {
        self.inside = false;
        self.left_us = Some(now_us);
        self.hit = MediaHit::None;
    }
}

/// What decides the picture of the controls of an element at a page time:
/// two equal keys give the same view. Built without fonts; the time labels
/// are the whole seconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ControlsKey {
    playing: bool,
    metadata: bool,
    has_audio: bool,
    /// The whole seconds of the current time and of the duration (-1: none).
    secs: u64,
    duration_secs: i64,
    /// The played part, in quarters of a second (the `timeupdate` points).
    quarter: u64,
    duration_ms: u64,
    thumb: bool,
    hover: u8,
    volume_milli: u16,
    muted: bool,
    slider: bool,
    slider_thumb: bool,
    opacity_step: u8,
    spinner: Option<u8>,
}

impl Default for ControlsKey {
    /// The key of the state before any playback (the view of it is
    /// [`MediaControlsView::default`]).
    fn default() -> ControlsKey {
        ControlsKey {
            playing: false,
            metadata: false,
            has_audio: false,
            secs: 0,
            duration_secs: -1,
            quarter: 0,
            duration_ms: 0,
            thumb: false,
            hover: 0,
            volume_milli: 1000,
            muted: false,
            slider: false,
            slider_thumb: false,
            opacity_step: OPACITY_STEPS as u8,
            spinner: None,
        }
    }
}

/// A number for a button (0 for none), for the key.
fn button_number(button: Option<MediaButton>) -> u8 {
    match button {
        None => 0,
        Some(MediaButton::Play) => 1,
        Some(MediaButton::Mute) => 2,
        Some(MediaButton::Fullscreen) => 3,
        Some(MediaButton::Menu) => 4,
    }
}

impl ControlsKey {
    /// The time labels this key shows: the current time alone, and with the
    /// duration ("m:ss / m:ss"); `None` before the metadata (layout's text
    /// shows).
    pub(crate) fn labels(&self) -> Option<(String, String)> {
        if !self.metadata {
            return None;
        }
        let duration = if self.duration_secs >= 0 {
            self.duration_secs as f64
        } else {
            f64::INFINITY
        };
        Some((
            swb_layout::time_label(self.secs as f64, duration, false),
            swb_layout::time_label(self.secs as f64, duration, true),
        ))
    }

    /// The view of the key, with the shaped labels `times` (see
    /// [`ControlsKey::labels`]).
    pub(crate) fn view(&self, times: Option<(MediaText, MediaText, f32)>) -> MediaControlsView {
        let played = if self.duration_ms > 0 {
            (self.quarter as f32 * 250.0 / self.duration_ms as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let hover = match self.hover {
            1 => Some(MediaButton::Play),
            2 => Some(MediaButton::Mute),
            3 => Some(MediaButton::Fullscreen),
            4 => Some(MediaButton::Menu),
            _ => None,
        };
        MediaControlsView {
            playing: self.playing,
            metadata: self.metadata,
            has_audio: self.has_audio,
            time: times.map(|(current, full, full_width)| MediaTimes {
                current,
                full,
                full_width,
            }),
            played,
            thumb: self.thumb,
            hover,
            volume: f32::from(self.volume_milli) / 1000.0,
            muted: self.muted,
            slider: self.slider,
            slider_thumb: self.slider_thumb,
            opacity: f32::from(self.opacity_step) / OPACITY_STEPS,
            spinner: self
                .spinner
                .map(|s| f32::from(s) / ANGLE_STEPS as f32 * std::f32::consts::TAU),
        }
    }
}

impl Element {
    /// The duration if it is a finite, positive number of seconds.
    fn finite_duration(&self) -> Option<f64> {
        self.meta
            .as_ref()
            .map(|m| m.duration)
            .filter(|d| d.is_finite() && *d > 0.0)
    }

    /// The controls of the element can seek: the metadata is known, the
    /// duration is finite and the resource is seekable.
    pub(crate) fn can_seek(&self) -> bool {
        self.error.is_none()
            && self.meta.as_ref().is_some_and(|m| m.seekable)
            && self.finite_duration().is_some()
    }

    /// The play button works: no error and a resource.
    pub(crate) fn can_play_from_controls(&self) -> bool {
        self.error.is_none() && self.phase != super::Phase::Empty
    }

    /// The mute button works: the metadata and an audio track.
    pub(crate) fn can_mute(&self) -> bool {
        self.meta.as_ref().is_some_and(|m| m.has_audio)
    }

    /// The volume slider is open: the pointer is over the mute button or the
    /// slider (or drags it) and the mute button works.
    pub(crate) fn slider_open(&self) -> bool {
        self.can_mute()
            && (matches!(
                self.controls.hit,
                MediaHit::Slider | MediaHit::Button(MediaButton::Mute)
            ) || self.controls.drag == Some(Drag::Volume)
                || self.controls.focus_volume)
    }

    /// The key of the controls at page time `now_us`. An element that has
    /// no controls on the page (no `controls` attribute with scripting, no
    /// box) has the key of the state before playback: nothing to draw and no
    /// repaint when it plays.
    /// The loading arc turns only on a page that paints frames (`animate`:
    /// the GUI): a screenshot of a page that did not animate shows none, as
    /// Chromium's references (taken after the arc) do.
    pub(crate) fn controls_key(&self, now_us: i64, animate: bool) -> ControlsKey {
        let c = &self.controls;
        if !c.present {
            return ControlsKey::default();
        }
        let now_ms = now_us as f64 / 1000.0;
        let position = self.position(now_ms).max(0.0);
        let duration = self.meta.as_ref().map(|m| m.duration);
        let finite = duration.filter(|d| d.is_finite() && *d >= 0.0);
        let (opacity, _) = c.opacity(self.paused, now_us);
        let slider_open = self.slider_open();
        let hover = match c.hit {
            MediaHit::Button(b) if b == MediaButton::Mute && !self.can_mute() => None,
            MediaHit::Button(b) if b == MediaButton::Play && !self.can_play_from_controls() => None,
            MediaHit::Button(b) => Some(b),
            _ => None,
        };
        let thumb = self.can_seek()
            && (c.hit == MediaHit::Timeline
                || c.focus_timeline
                || matches!(c.drag, Some(Drag::Timeline { .. }))
                || (self.video && c.inside && c.band));
        let spinner = c
            .loading_us
            .filter(|_| animate)
            .and_then(|since| spinner_step(since, now_us));
        let opacity_step = if self.video {
            (opacity * OPACITY_STEPS).round().clamp(0.0, OPACITY_STEPS) as u8
        } else {
            OPACITY_STEPS as u8
        };
        if opacity_step == 0 && spinner.is_none() {
            // Hidden controls draw nothing: the time, the position and the
            // pointer's state change nothing that shows, so they are not
            // part of the key (a playing video with hidden controls makes
            // no repaint of its own).
            return ControlsKey {
                opacity_step: 0,
                ..ControlsKey::default()
            };
        }
        ControlsKey {
            playing: !self.paused,
            metadata: self.meta.is_some(),
            has_audio: self.can_mute(),
            secs: position.floor() as u64,
            duration_secs: finite.map_or(-1, |d| d.floor() as i64),
            quarter: (position * 4.0).floor() as u64,
            duration_ms: finite.map_or(0, |d| (d * 1000.0).round() as u64),
            thumb,
            hover: button_number(hover),
            volume_milli: (self.volume.clamp(0.0, 1.0) * 1000.0).round() as u16,
            muted: self.muted,
            slider: slider_open,
            slider_thumb: slider_open
                && (c.hit == MediaHit::Slider || c.drag == Some(Drag::Volume)),
            opacity_step,
            spinner,
        }
    }

    /// The page time (microseconds) of the next change of the controls'
    /// picture that time brings: the end of a fade and its steps, the
    /// auto-hide, the arc. `None` if nothing changes without input.
    pub(crate) fn controls_next_due(&self, now_us: i64, animate: bool) -> Option<i64> {
        let c = &self.controls;
        if !c.present {
            return None;
        }
        let mut due: Option<i64> = None;
        let mut note = |t: i64| due = Some(due.map_or(t, |d| d.min(t)));
        if self.video {
            let (_, fade) = c.opacity(self.paused, now_us);
            if fade.running(now_us) {
                note(now_us + ANIMATION_TICK_US);
            }
            // The auto-hide of a playing video: the fade starts at the
            // deadline.
            if !self.paused && c.drag.is_none() && (c.inside || c.left_us.is_none()) {
                let deadline = c.activity_us.unwrap_or(0).saturating_add(HIDE_AFTER_US);
                if deadline > now_us && fade.to_shown {
                    note(deadline);
                }
            }
        }
        if let Some(since) = c.loading_us.filter(|_| animate) {
            let (from, to) = (since + SPINNER_FROM_US, since + SPINNER_TO_US);
            if now_us < from {
                note(from);
            } else if now_us < to {
                note((now_us + SPINNER_TURN_US / ANGLE_STEPS).min(to));
            }
        }
        due
    }
}

/// The step of the arc's angle at `now_us` for a load that started at
/// `since`, while it shows.
fn spinner_step(since: i64, now_us: i64) -> Option<u8> {
    let age = now_us.saturating_sub(since);
    if !(SPINNER_FROM_US..SPINNER_TO_US).contains(&age) {
        return None;
    }
    let step = (age % SPINNER_TURN_US) * ANGLE_STEPS / SPINNER_TURN_US;
    u8::try_from(step).ok()
}

// ----- The actions -----

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
        self.ui.pressed = Some(Pressed { node, hit });
        match hit {
            MediaHit::None => false,
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
                self.ui.picture = Some(node);
                false
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
    /// element (see [`Element::controls_next_due`]).
    pub(crate) fn controls_next_due(&self, now_us: i64) -> Option<f64> {
        let animate = self.ctx.viewport_rule();
        self.elements
            .values()
            .filter_map(|e| e.controls_next_due(now_us, animate))
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

#[cfg(test)]
mod tests {
    use super::super::backend::Meta;
    use super::*;

    fn meta(duration: f64, seekable: bool, has_audio: bool) -> Meta {
        Meta {
            duration,
            start: 0.0,
            size: Some((320, 240)),
            seekable,
            has_audio,
        }
    }

    fn element(meta: Option<Meta>) -> Element {
        let mut e = Element::new(true, false);
        e.controls.present = true;
        e.meta = meta;
        e
    }

    /// The fade: linear from the opacity where it starts; a new target
    /// starts from the opacity that the old fade had reached.
    #[test]
    fn a_fade_goes_from_where_it_is() {
        let shown = Fade::SHOWN;
        let out = shown.retarget(false, 1_000_000);
        assert_eq!(out.at(1_000_000), 1.0);
        assert!((out.at(1_350_000) - 0.5).abs() < 1e-4);
        assert_eq!(out.at(1_700_000), 0.0);
        assert_eq!(out.at(9_000_000), 0.0);
        // Back to shown half way: from 0.5, in 0.1 s at the full speed.
        let back = out.retarget(true, 1_350_000);
        assert!((back.at(1_350_000) - 0.5).abs() < 1e-4);
        assert!((back.at(1_400_000) - 0.75).abs() < 1e-4);
        assert_eq!(back.at(1_500_000), 1.0);
        // The same target keeps the fade; a time before its start is its
        // start (no negative elapsed time).
        assert_eq!(out.retarget(false, 5_000_000), out);
        assert_eq!(out.at(0), 1.0);
        assert!(out.running(1_300_000) && !out.running(1_800_000));
    }

    /// Paused and dragged controls are shown; a playing video's controls
    /// are shown for 2.5 s after the last move, hidden from the leave.
    #[test]
    fn the_controls_show_while_paused_and_for_2_5_s_after_a_move() {
        let mut c = Controls::default();
        assert_eq!(c.wanted(true, 9_000_000), (true, 9_000_000));
        c.moved_inside(1_000_000);
        assert_eq!(c.wanted(false, 3_400_000), (true, 1_000_000));
        assert_eq!(c.wanted(false, 3_500_000), (false, 3_500_000));
        assert_eq!(c.wanted(false, 8_000_000), (false, 3_500_000));
        c.drag = Some(Drag::Volume);
        assert!(c.wanted(false, 8_000_000).0, "a drag keeps them");
        c.drag = None;
        c.left(2_000_000);
        assert_eq!(c.wanted(false, 2_100_000), (false, 2_000_000));
        c.moved_inside(5_000_000);
        assert_eq!(c.wanted(false, 5_100_000), (true, 5_000_000));
        // The opacity at a time: half way through the fade out.
        let (opacity, fade) = c.opacity(false, 7_850_000);
        assert!((opacity - 0.5).abs() < 0.01, "{opacity}");
        c.fade = fade;
        assert_eq!(c.opacity(false, 8_300_000).0, 0.0);
    }

    #[test]
    fn the_arc_shows_between_0_1_and_1_1_s() {
        assert_eq!(spinner_step(1_000_000, 1_050_000), None);
        // 0.1 s into a turn of 1.4 s is the step 4 of 64.
        assert_eq!(spinner_step(1_000_000, 1_100_000), Some(4));
        assert!(spinner_step(1_000_000, 1_700_000).is_some());
        assert_eq!(spinner_step(1_000_000, 2_100_000), None);
        // Before the load (a time before the start) shows nothing.
        assert_eq!(spinner_step(1_000_000, 500_000), None);
    }

    /// A duration that is infinite, `NaN` or 0 gives no seeking and no label
    /// of the duration, and the key does not panic.
    #[test]
    fn a_duration_that_is_not_finite_cannot_seek() {
        for duration in [f64::INFINITY, f64::NAN, 0.0, -1.0] {
            let e = element(Some(meta(duration, true, true)));
            assert!(!e.can_seek(), "{duration}");
            let key = e.controls_key(5_000_000, false);
            assert!(key.duration_secs <= 0 || duration > 0.0);
            let (current, full) = key.labels().unwrap();
            assert_eq!(current, "0:00");
            assert!(full == "0:00" || full == "0:00 / 0:00", "{full}");
            let _ = key.view(None);
        }
        assert!(element(Some(meta(10.0, true, true))).can_seek());
        assert!(!element(Some(meta(10.0, false, true))).can_seek());
        assert!(!element(None).can_seek());
        // A source that failed cannot seek either.
        let mut failed = element(Some(meta(10.0, true, true)));
        failed.error = Some(swb_web::MediaErrorInfo {
            code: 4,
            message: String::new(),
            serial: 1,
        });
        assert!(!failed.can_seek());
    }

    /// The buttons that need the metadata or an audio track.
    #[test]
    fn the_mute_button_needs_an_audio_track() {
        assert!(!element(None).can_mute());
        assert!(!element(Some(meta(10.0, true, false))).can_mute());
        assert!(element(Some(meta(10.0, true, true))).can_mute());
        // The slider opens only for a mute button that works.
        let mut e = element(Some(meta(10.0, true, false)));
        e.controls.hit = MediaHit::Button(MediaButton::Mute);
        assert!(!e.slider_open());
        let mut e = element(Some(meta(10.0, true, true)));
        e.controls.hit = MediaHit::Button(MediaButton::Mute);
        assert!(e.slider_open());
        e.controls.hit = MediaHit::None;
        assert!(!e.slider_open());
        e.controls.focus_volume = true;
        assert!(e.slider_open(), "a focused button keeps it open");
    }

    /// Hidden controls have one key whatever the position; an element
    /// without controls on the page has the key of the state before
    /// playback.
    #[test]
    fn hidden_controls_and_absent_controls_have_a_constant_key() {
        let mut e = element(Some(meta(10.0, true, true)));
        e.paused = false;
        e.controls.activity_us = Some(0);
        e.controls.inside = true;
        let shown = e.controls_key(1_000_000, false);
        assert_ne!(
            shown,
            e.controls_key(3_000_000 + 700_000 / 2, false),
            "the fade"
        );
        let hidden_a = e.controls_key(5_000_000, false);
        let hidden_b = e.controls_key(8_700_000, false);
        assert_eq!(hidden_a, hidden_b);
        assert_ne!(hidden_a, ControlsKey::default());
        e.controls.present = false;
        assert_eq!(e.controls_key(1_000_000, false), ControlsKey::default());
        assert_eq!(e.controls_next_due(1_000_000, false), None);
    }

    /// The due times: the auto-hide's deadline while shown and playing, the
    /// fade's ticks, nothing when paused or hidden.
    #[test]
    fn the_due_times_are_the_deadline_and_the_fade() {
        let mut e = element(Some(meta(10.0, true, true)));
        assert_eq!(e.controls_next_due(0, false), None, "paused");
        e.paused = false;
        e.controls.activity_us = Some(1_000_000);
        e.controls.inside = true;
        assert_eq!(e.controls_next_due(2_000_000, false), Some(3_500_000));
        // During the fade: the next tick.
        assert_eq!(e.controls_next_due(3_700_000, false), Some(3_716_000));
        // Hidden: nothing.
        assert_eq!(e.controls_next_due(4_300_000, false), None);
        // The loading arc counts on a page that animates only.
        e.paused = true;
        e.controls.loading_us = Some(0);
        assert_eq!(e.controls_next_due(0, false), None);
        assert_eq!(e.controls_next_due(0, true), Some(100_000));
        assert!(e.controls_next_due(500_000, true).is_some());
        assert_eq!(e.controls_next_due(2_000_000, true), None);
    }
}
