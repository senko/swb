//! The events of CSS animations and transitions (ADR 0044 section 6; M13
//! features 3c and 5): which events a change of an animation's phase and
//! iteration gives, with their `elapsedTime` and their scheduled event
//! time, by the tables of CSS Animations 2 §5.1
//! (<https://drafts.csswg.org/css-animations-2/#event-dispatch>) and CSS
//! Transitions 2 §5.1
//! (<https://drafts.csswg.org/css-transitions-2/#event-dispatch>).
//!
//! [`DocumentAnimations`](super::DocumentAnimations) queues the events at
//! each update (and a cancel at once, when a style change or a script
//! cancels an animation); the engine takes the queue at the step "update
//! animations and send events" of the rendering update, sorts it
//! ([`PendingAnimationEvent::scheduled`], then composite order) and
//! dispatches the events at their owning elements.

use std::sync::Arc;

use super::records::{AnimationId, AnimationOwner};
use super::timing::{Phase, Timing};

/// The type of an animation event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimationEventKind {
    /// `animationstart`.
    Start,
    /// `animationiteration`.
    Iteration,
    /// `animationend`.
    End,
    /// `animationcancel`.
    Cancel,
    /// `transitionrun`.
    TransitionRun,
    /// `transitionstart`.
    TransitionStart,
    /// `transitionend`.
    TransitionEnd,
    /// `transitioncancel`.
    TransitionCancel,
}

impl AnimationEventKind {
    /// The event type.
    pub fn as_str(self) -> &'static str {
        match self {
            AnimationEventKind::Start => "animationstart",
            AnimationEventKind::Iteration => "animationiteration",
            AnimationEventKind::End => "animationend",
            AnimationEventKind::Cancel => "animationcancel",
            AnimationEventKind::TransitionRun => "transitionrun",
            AnimationEventKind::TransitionStart => "transitionstart",
            AnimationEventKind::TransitionEnd => "transitionend",
            AnimationEventKind::TransitionCancel => "transitioncancel",
        }
    }

    /// True for the events of CSS transitions (`TransitionEvent`).
    pub fn is_transition(self) -> bool {
        matches!(
            self,
            AnimationEventKind::TransitionRun
                | AnimationEventKind::TransitionStart
                | AnimationEventKind::TransitionEnd
                | AnimationEventKind::TransitionCancel
        )
    }
}

/// An event in the document's pending animation event queue. It keeps what
/// the dispatch needs, so a record that went (a cancel) still gets its
/// event.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingAnimationEvent {
    /// The type.
    pub kind: AnimationEventKind,
    /// The animation.
    pub id: AnimationId,
    /// The owning element: the target (with the pseudo-element for
    /// `pseudoElement`).
    pub owner: AnimationOwner,
    /// `animationName`, or the `propertyName` of a transition.
    pub name: Arc<str>,
    /// The position of the name in `animation-name` when the event was
    /// queued (composite order within the owner).
    pub index: usize,
    /// `elapsedTime` in seconds.
    pub elapsed: f64,
    /// The scheduled event time: the timeline time (ms) at which the event
    /// happened; `None` sorts first.
    pub scheduled: Option<f64>,
    /// The transition generation of a CSS transition (composite order:
    /// all transitions before all animations; `None` for an animation).
    pub generation: Option<u64>,
}

/// The interval start of an effect: max(min(−delay, active duration), 0).
/// <https://drafts.csswg.org/css-animations-2/#interval-start>
fn interval_start(timing: &Timing) -> f64 {
    (-timing.delay).min(timing.active_duration()).max(0.0)
}

/// The interval end: max(min(end − delay, active duration), 0).
/// <https://drafts.csswg.org/css-animations-2/#interval-end>
fn interval_end(timing: &Timing) -> f64 {
    (timing.end_time() - timing.delay)
        .min(timing.active_duration())
        .max(0.0)
}

/// What one update saw of an animation: its phase and current iteration
/// before and after.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PhaseChange {
    pub(crate) before: (Phase, Option<f64>),
    pub(crate) after: (Phase, Option<f64>),
}

/// The events of `change` as (kind, elapsed time in ms): the table of CSS Animations 2 §5.1 without the cancel (the caller
/// queues that when it cancels). At most one `animationiteration`.
pub(crate) fn events_of(timing: &Timing, change: PhaseChange) -> Vec<(AnimationEventKind, f64)> {
    use AnimationEventKind::{End, Iteration, Start};
    let start = interval_start(timing);
    let end = interval_end(timing);
    let (before, before_iteration) = change.before;
    let (after, after_iteration) = change.after;
    match (before, after) {
        (Phase::Idle | Phase::Before, Phase::Active) => vec![(Start, start)],
        (Phase::Idle | Phase::Before, Phase::After) => vec![(Start, start), (End, end)],
        (Phase::Active, Phase::Before) => vec![(End, start)],
        (Phase::Active, Phase::Active) => match (before_iteration, after_iteration) {
            (Some(old), Some(new)) if old != new => {
                // The iteration boundary: the current iteration, plus 1
                // when the iteration went down (a seek backwards).
                let boundary = if new < old { new + 1.0 } else { new };
                let elapsed = (boundary - timing.iteration_start) * timing.duration;
                vec![(Iteration, elapsed)]
            }
            _ => Vec::new(),
        },
        (Phase::Active, Phase::After) => vec![(End, end)],
        (Phase::After, Phase::Active) => vec![(Start, end)],
        (Phase::After, Phase::Before) => vec![(Start, end), (End, start)],
        _ => Vec::new(),
    }
}

/// The events of a CSS transition for `change` as (kind, elapsed time in
/// ms, the elapsed time that places the event, `None` for the start time):
/// the table of CSS Transitions 2 §5.1 without the cancel. A
/// `transitionrun` is scheduled at the start time (measured in Chromium
/// 148, `events-run-start-end`: the `transitionrun` of a delayed transition
/// comes with the `transitionrun` and `transitionstart` of the others).
pub(crate) fn transition_events_of(
    timing: &Timing,
    change: PhaseChange,
) -> Vec<(AnimationEventKind, f64, Option<f64>)> {
    use AnimationEventKind::{
        TransitionEnd as End, TransitionRun as Run, TransitionStart as Start,
    };
    let start = interval_start(timing);
    let end = interval_end(timing);
    match (change.before.0, change.after.0) {
        (Phase::Idle, Phase::Before) => vec![(Run, start, None)],
        (Phase::Idle, Phase::Active) => vec![(Run, start, None), (Start, start, Some(start))],
        (Phase::Idle, Phase::After) => vec![
            (Run, start, None),
            (Start, start, Some(start)),
            (End, end, Some(end)),
        ],
        (Phase::Before, Phase::Active) => vec![(Start, start, Some(start))],
        (Phase::Before, Phase::After) => vec![(Start, start, Some(start)), (End, end, Some(end))],
        (Phase::Active, Phase::After) => vec![(End, end, Some(end))],
        (Phase::Active, Phase::Before) => vec![(End, start, Some(start))],
        (Phase::After, Phase::Active) => vec![(Start, end, Some(end))],
        (Phase::After, Phase::Before) => vec![(Start, end, Some(end)), (End, start, Some(start))],
        _ => Vec::new(),
    }
}

/// The scheduled event time of an event with the elapsed time `elapsed`
/// (ms) of an animation whose local time 0 is the timeline time `zero`:
/// the timeline time of the boundary ([`event_time`]).
pub(crate) fn scheduled_time(timing: &Timing, zero: f64, elapsed: f64) -> f64 {
    event_time(zero + timing.delay + elapsed)
}

/// A scheduled event time rounded to 1e-6 ms. Two boundaries that are the
/// same time in exact arithmetic (the start of an animation, and its end
/// after `finish()` moved the start time back by the end) can differ by
/// float noise; rounded, they are equal, and the stable sort keeps the
/// order in which they were queued. A time too large to round stays as it
/// is.
pub(crate) fn event_time(time: f64) -> f64 {
    let rounded = (time * 1e6).round() / 1e6;
    if rounded.is_finite() { rounded } else { time }
}

/// The active time at the local time `local` with a fill mode of both
/// (the `elapsedTime` of `animationcancel`).
pub(crate) fn cancel_elapsed(timing: &Timing, local: Option<f64>) -> f64 {
    local.map_or(0.0, |t| {
        (t - timing.delay).min(timing.active_duration()).max(0.0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use AnimationEventKind::{End, Iteration, Start};

    fn timing(delay: f64, duration: f64, iterations: f64) -> Timing {
        Timing {
            delay,
            duration,
            iterations,
            ..Timing::default()
        }
    }

    fn change(before: (Phase, Option<f64>), after: (Phase, Option<f64>)) -> PhaseChange {
        PhaseChange { before, after }
    }

    #[test]
    fn the_table_of_phase_changes() {
        // `a 80ms linear 40ms 2` (probe `events-single`).
        let t = timing(40.0, 80.0, 2.0);
        assert_eq!(
            events_of(&t, change((Phase::Idle, None), (Phase::Before, None))),
            vec![]
        );
        assert_eq!(
            events_of(
                &t,
                change((Phase::Before, None), (Phase::Active, Some(0.0)))
            ),
            vec![(Start, 0.0)]
        );
        assert_eq!(
            events_of(
                &t,
                change((Phase::Active, Some(0.0)), (Phase::Active, Some(1.0)))
            ),
            vec![(Iteration, 80.0)]
        );
        assert_eq!(
            events_of(&t, change((Phase::Active, Some(1.0)), (Phase::After, None))),
            vec![(End, 160.0)]
        );
        // A skipped interval gives the start and the end.
        assert_eq!(
            events_of(&t, change((Phase::Idle, None), (Phase::After, None))),
            vec![(Start, 0.0), (End, 160.0)]
        );
        // Seeks back.
        assert_eq!(
            events_of(&t, change((Phase::After, None), (Phase::Active, Some(1.0)))),
            vec![(Start, 160.0)]
        );
        assert_eq!(
            events_of(&t, change((Phase::After, None), (Phase::Before, None))),
            vec![(Start, 160.0), (End, 0.0)]
        );
        assert_eq!(
            events_of(
                &t,
                change((Phase::Active, Some(1.0)), (Phase::Before, None))
            ),
            vec![(End, 0.0)]
        );
        assert_eq!(
            events_of(
                &t,
                change((Phase::Active, Some(1.0)), (Phase::Active, Some(0.0)))
            ),
            vec![(Iteration, 80.0)]
        );
        // Nothing for the same phase and iteration, or from the after phase
        // to itself.
        assert_eq!(
            events_of(
                &t,
                change((Phase::Active, Some(1.0)), (Phase::Active, Some(1.0)))
            ),
            vec![]
        );
        assert_eq!(
            events_of(&t, change((Phase::After, None), (Phase::After, None))),
            vec![]
        );
    }

    #[test]
    fn a_negative_delay_starts_inside_the_interval() {
        let t = timing(-250.0, 100.0, 5.0);
        assert_eq!(
            events_of(&t, change((Phase::Idle, None), (Phase::Active, Some(2.0)))),
            vec![(Start, 250.0)]
        );
        assert_eq!(scheduled_time(&t, 1000.0, 250.0), 1000.0);
        assert_eq!(cancel_elapsed(&t, Some(100.0)), 350.0);
        assert_eq!(cancel_elapsed(&t, Some(1e9)), 500.0);
        assert_eq!(cancel_elapsed(&timing(50.0, 10.0, 1.0), Some(10.0)), 0.0);
    }
}
