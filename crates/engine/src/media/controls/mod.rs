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
//!   nothing. An `<audio>` has no picture: a press on its panel outside the
//!   parts (the time label, the padding) gives the page no event and does
//!   nothing either (M14 feature 10c).
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
//!
//! The files: this one has the constants, the auto-hide state ([`Controls`],
//! `Fade`, `Drag`); `key.rs` the key and the view; `actions.rs` the actions
//! of the buttons, the timeline and the slider; `pointer.rs` the pointer
//! (hover, press, drag, the places of the boxes, the shaped labels).

mod actions;
mod key;
mod pointer;

pub(crate) use key::ControlsKey;
pub(crate) use pointer::{ControlsBox, PointerState};

use swb_layout::{MediaHit, Point};

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
