//! The timing model of Web Animations 1 as plain functions on `f64`
//! (<https://drafts.csswg.org/web-animations-1/#timing-model>): the active
//! duration, the phases, the fill modes, the overall, simple and directed
//! progress, the current iteration and the before flag that `steps()`
//! reads.
//!
//! Times are in milliseconds. The playback rate is 1 (swb has no
//! `updatePlaybackRate()`), so the animation direction is always
//! "forwards". The current iteration is computed from the local time, not
//! counted, so an infinite 1 ms animation seeked to 1e9 ms is at iteration
//! 1e9 at once (ADR 0044 section 11).

use super::easing::Easing;

/// `animation-direction` (the playback direction of an effect).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlaybackDirection {
    /// `normal`.
    #[default]
    Normal,
    /// `reverse`.
    Reverse,
    /// `alternate`.
    Alternate,
    /// `alternate-reverse`.
    AlternateReverse,
}

/// `animation-fill-mode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FillMode {
    /// `none`.
    #[default]
    None,
    /// `forwards`.
    Forwards,
    /// `backwards`.
    Backwards,
    /// `both`.
    Both,
}

impl FillMode {
    fn backwards(self) -> bool {
        matches!(self, FillMode::Backwards | FillMode::Both)
    }

    fn forwards(self) -> bool {
        matches!(self, FillMode::Forwards | FillMode::Both)
    }
}

/// The phase of an effect at a local time.
/// <https://drafts.csswg.org/web-animations-1/#animation-effect-phases-and-states>
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Before the active interval (in the delay).
    Before,
    /// In the active interval.
    Active,
    /// After the active interval.
    After,
    /// No local time (the animation has no current time).
    Idle,
}

/// The timing properties of an effect.
/// <https://drafts.csswg.org/web-animations-1/#timing-properties>
#[derive(Clone, Debug, PartialEq)]
pub struct Timing {
    /// The start delay in ms (`animation-delay`, may be negative).
    pub delay: f64,
    /// The end delay in ms (0 for CSS animations).
    pub end_delay: f64,
    /// The iteration duration in ms (not negative).
    pub duration: f64,
    /// The iteration count (not negative, may be infinite).
    pub iterations: f64,
    /// The iteration start (0 for CSS animations).
    pub iteration_start: f64,
    /// The playback direction.
    pub direction: PlaybackDirection,
    /// The fill mode.
    pub fill: FillMode,
    /// The easing of the effect (`linear` for CSS animations: their
    /// timing function is the easing of each keyframe interval).
    pub easing: Easing,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            delay: 0.0,
            end_delay: 0.0,
            duration: 0.0,
            iterations: 1.0,
            iteration_start: 0.0,
            direction: PlaybackDirection::Normal,
            fill: FillMode::None,
            easing: Easing::Linear,
        }
    }
}

/// The timing of an effect at one local time.
/// <https://drafts.csswg.org/web-animations-1/#calculating-computed-timing>
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComputedTiming {
    /// The phase.
    pub phase: Phase,
    /// The active time; `None` when unresolved.
    pub active_time: Option<f64>,
    /// The overall progress; `None` when unresolved.
    pub overall_progress: Option<f64>,
    /// The simple iteration progress (0 to 1); `None` when unresolved.
    pub simple_progress: Option<f64>,
    /// The current iteration (may be infinite); `None` when unresolved.
    pub current_iteration: Option<f64>,
    /// True if the current direction is forwards.
    pub forwards: bool,
    /// The directed progress; `None` when unresolved.
    pub directed_progress: Option<f64>,
    /// The before flag of the easing (CSS Easing 2 §4).
    pub before_flag: bool,
    /// The iteration progress: the directed progress through the effect's
    /// easing; `None` when unresolved (the effect has no value).
    pub progress: Option<f64>,
}

impl Timing {
    /// The active duration: iteration duration × iteration count, 0 when
    /// either is 0.
    /// <https://drafts.csswg.org/web-animations-1/#active-duration>
    pub fn active_duration(&self) -> f64 {
        if self.duration == 0.0 || self.iterations == 0.0 {
            0.0
        } else {
            self.duration * self.iterations
        }
    }

    /// The end time: max(delay + active duration + end delay, 0).
    /// <https://drafts.csswg.org/web-animations-1/#end-time>
    pub fn end_time(&self) -> f64 {
        (self.delay + self.active_duration() + self.end_delay).max(0.0)
    }

    /// The phase at `local_time`.
    pub fn phase(&self, local_time: Option<f64>) -> Phase {
        let Some(t) = local_time else {
            return Phase::Idle;
        };
        let end = self.end_time();
        let before_active = self.delay.min(end).max(0.0);
        let active_after = (self.delay + self.active_duration()).min(end).max(0.0);
        if t < before_active {
            Phase::Before
        } else if t >= active_after {
            // The animation direction is "forwards" (playback rate 1), so
            // the active-after boundary belongs to the after phase.
            Phase::After
        } else {
            Phase::Active
        }
    }

    /// The active time at `local_time` in `phase`.
    /// <https://drafts.csswg.org/web-animations-1/#calculating-the-active-time>
    fn active_time(&self, local_time: f64, phase: Phase) -> Option<f64> {
        match phase {
            Phase::Before => self
                .fill
                .backwards()
                .then(|| (local_time - self.delay).max(0.0)),
            Phase::Active => Some(local_time - self.delay),
            Phase::After => self.fill.forwards().then(|| {
                (local_time - self.delay)
                    .min(self.active_duration())
                    .max(0.0)
            }),
            Phase::Idle => None,
        }
    }

    /// The overall progress for an active time.
    /// <https://drafts.csswg.org/web-animations-1/#calculating-the-overall-progress>
    fn overall_progress(&self, active_time: f64, phase: Phase) -> f64 {
        if self.duration == 0.0 {
            if phase == Phase::Before {
                self.iteration_start
            } else {
                self.iteration_start + self.iterations
            }
        } else {
            active_time / self.duration + self.iteration_start
        }
    }

    /// The before flag that a `steps()` keyframe easing gets, measured in
    /// Chromium 148 (`keyframe-easing-before-flag` of
    /// `tools/probes/css-animations-3b.json`): set in the before phase of a
    /// forwards iteration, and at the start of the active interval of an
    /// effect with a delay and a backwards fill (the left limit there, as in
    /// the before phase); never in the after phase (a reversed iteration
    /// that ends at progress 0 takes the right limit, unlike the effect's
    /// before flag of Web Animations §4.10).
    pub fn keyframe_before_flag(&self, computed: &ComputedTiming) -> bool {
        match computed.phase {
            Phase::Before => computed.forwards,
            Phase::Active => {
                computed.forwards
                    && self.delay > 0.0
                    && self.fill.backwards()
                    && computed.active_time == Some(0.0)
            }
            Phase::After | Phase::Idle => false,
        }
    }

    /// The timing at `local_time` (`None`: no current time).
    pub fn compute(&self, local_time: Option<f64>) -> ComputedTiming {
        let phase = self.phase(local_time);
        let active_time = local_time.and_then(|t| self.active_time(t, phase));
        let mut out = ComputedTiming {
            phase,
            active_time,
            overall_progress: None,
            simple_progress: None,
            current_iteration: None,
            forwards: true,
            directed_progress: None,
            before_flag: false,
            progress: None,
        };
        let Some(active_time) = active_time else {
            return out;
        };
        let overall = self.overall_progress(active_time, phase);
        // The simple iteration progress.
        // <https://drafts.csswg.org/web-animations-1/#calculating-the-simple-iteration-progress>
        let mut simple = if overall.is_infinite() {
            self.iteration_start % 1.0
        } else {
            overall % 1.0
        };
        if simple == 0.0
            && matches!(phase, Phase::Active | Phase::After)
            && active_time == self.active_duration()
            && self.iterations != 0.0
        {
            simple = 1.0;
        }
        // The current iteration.
        // <https://drafts.csswg.org/web-animations-1/#calculating-the-current-iteration>
        let iteration = if phase == Phase::After && self.iterations.is_infinite() {
            f64::INFINITY
        } else if simple == 1.0 {
            overall.floor() - 1.0
        } else {
            overall.floor()
        };
        // The directed progress.
        // <https://drafts.csswg.org/web-animations-1/#calculating-the-directed-progress>
        let forwards = match self.direction {
            PlaybackDirection::Normal => true,
            PlaybackDirection::Reverse => false,
            PlaybackDirection::Alternate | PlaybackDirection::AlternateReverse => {
                let mut d = iteration;
                if self.direction == PlaybackDirection::AlternateReverse {
                    d += 1.0;
                }
                d.is_infinite() || d % 2.0 == 0.0
            }
        };
        let directed = if forwards { simple } else { 1.0 - simple };
        // The transformed progress and the before flag.
        // <https://drafts.csswg.org/web-animations-1/#calculating-the-transformed-progress>
        let before_flag =
            (phase == Phase::Before && forwards) || (phase == Phase::After && !forwards);
        out.overall_progress = Some(overall);
        out.simple_progress = Some(simple);
        out.current_iteration = Some(iteration);
        out.forwards = forwards;
        out.directed_progress = Some(directed);
        out.before_flag = before_flag;
        out.progress = Some(self.easing.evaluate(directed, before_flag));
        out
    }
}

#[cfg(test)]
mod tests;
