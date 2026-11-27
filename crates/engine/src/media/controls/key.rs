//! The key and the view of the default controls: [`ControlsKey`] gathers
//! what decides the picture of an element's controls at a page time
//! ([`Element::controls_key`], without shaping any text) and makes the
//! [`MediaControlsView`]; the element's controls state that it reads
//! (`can_seek`, `can_mute`, the slider) and the page time of its next change
//! ([`Element::next_due_us`]) are here too. A part of the module `controls`,
//! moved out of one file unchanged.

use swb_layout::{MediaButton, MediaControlsView, MediaHit, MediaText, MediaTimes};

use super::{
    ANGLE_STEPS, ANIMATION_TICK_US, Drag, HIDE_AFTER_US, OPACITY_STEPS, SPINNER_FROM_US,
    SPINNER_TO_US, SPINNER_TURN_US,
};
use crate::media::Element;

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
    hover: Option<MediaButton>,
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
            hover: None,
            volume_milli: 1000,
            muted: false,
            slider: false,
            slider_thumb: false,
            opacity_step: OPACITY_STEPS as u8,
            spinner: None,
        }
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
            hover: self.hover,
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
    pub(super) fn finite_duration(&self) -> Option<f64> {
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
        self.error.is_none() && self.phase != crate::media::Phase::Empty
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
            hover,
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
    pub(crate) fn next_due_us(&self, now_us: i64, animate: bool) -> Option<i64> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::backend::Meta;

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
        assert_eq!(e.next_due_us(1_000_000, false), None);
    }

    /// The due times: the auto-hide's deadline while shown and playing, the
    /// fade's ticks, nothing when paused or hidden.
    #[test]
    fn the_due_times_are_the_deadline_and_the_fade() {
        let mut e = element(Some(meta(10.0, true, true)));
        assert_eq!(e.next_due_us(0, false), None, "paused");
        e.paused = false;
        e.controls.activity_us = Some(1_000_000);
        e.controls.inside = true;
        assert_eq!(e.next_due_us(2_000_000, false), Some(3_500_000));
        // During the fade: the next tick.
        assert_eq!(e.next_due_us(3_700_000, false), Some(3_716_000));
        // Hidden: nothing.
        assert_eq!(e.next_due_us(4_300_000, false), None);
        // The loading arc counts on a page that animates only.
        e.paused = true;
        e.controls.loading_us = Some(0);
        assert_eq!(e.next_due_us(0, false), None);
        assert_eq!(e.next_due_us(0, true), Some(100_000));
        assert!(e.next_due_us(500_000, true).is_some());
        assert_eq!(e.next_due_us(2_000_000, true), None);
    }
}
