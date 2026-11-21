//! Tests of the timing model against the rows of Chromium 148 in
//! `crates/automation/tests/data/css-animations.json` (cases `fill-modes`,
//! `iteration-direction`, `hero-reveal`, `spinner-steps`, `marquee`). A
//! keyframe effect from `a` to `b` with linear keyframe easing has the
//! value a + (b − a) × progress, so the rows give the progress.

// The numbers are Chromium's values as it prints them.
#![allow(clippy::unreadable_literal)]

use super::*;
use crate::animation::easing::{StepPosition, parse_easing};
use crate::parse::test_util::parse_all;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-4 || (a - b).abs() <= 1e-5 * b.abs()
}

fn timing(duration: f64, delay: f64, iterations: f64) -> Timing {
    Timing {
        delay,
        duration,
        iterations,
        ..Timing::default()
    }
}

/// The value of a linear effect from `a` to `b`, or `base` without one.
fn value(t: &Timing, local: f64, a: f64, b: f64, base: f64) -> f64 {
    t.compute(Some(local))
        .progress
        .map_or(base, |p| a + (b - a) * p)
}

#[test]
fn fill_modes() {
    // `fill-modes`, query 0: `o 1s linear 500ms`, opacity .2 to .8 over a
    // base of 1; the rows give none, forwards, backwards, both.
    let rows: [(f64, [f64; 4]); 9] = [
        (0.0, [1.0, 1.0, 0.2, 0.2]),
        (250.0, [1.0, 1.0, 0.2, 0.2]),
        (499.0, [1.0, 1.0, 0.2, 0.2]),
        (500.0, [0.2, 0.2, 0.2, 0.2]),
        (750.0, [0.35, 0.35, 0.35, 0.35]),
        (1000.0, [0.5, 0.5, 0.5, 0.5]),
        (1499.0, [0.7994, 0.7994, 0.7994, 0.7994]),
        (1500.0, [1.0, 0.8, 1.0, 0.8]),
        (2000.0, [1.0, 0.8, 1.0, 0.8]),
    ];
    let fills = [
        FillMode::None,
        FillMode::Forwards,
        FillMode::Backwards,
        FillMode::Both,
    ];
    for (time, values) in rows {
        for (fill, want) in fills.iter().zip(values) {
            let t = Timing {
                fill: *fill,
                ..timing(1000.0, 500.0, 1.0)
            };
            let got = value(&t, time, 0.2, 0.8, 1.0);
            assert!(close(got, want), "{fill:?} at {time}: {got} != {want}");
        }
    }
    // Query 1: `getComputedTiming().progress` after the end is 1.
    let t = Timing {
        fill: FillMode::Forwards,
        ..timing(1000.0, 500.0, 1.0)
    };
    assert_eq!(t.compute(Some(2000.0)).progress, Some(1.0));
}

#[test]
fn iteration_count_and_direction() {
    // `iteration-direction`, query 0: `s 1s linear both`, left 0 to 100px.
    let effects = [
        (2.5, PlaybackDirection::Normal),
        (2.0, PlaybackDirection::Reverse),
        (3.0, PlaybackDirection::Alternate),
        (3.0, PlaybackDirection::AlternateReverse),
        (f64::INFINITY, PlaybackDirection::Alternate),
        (0.0, PlaybackDirection::Normal),
    ];
    let rows: [(f64, [f64; 6]); 13] = [
        (0.0, [0.0, 100.0, 0.0, 100.0, 0.0, 0.0]),
        (250.0, [25.0, 75.0, 25.0, 75.0, 25.0, 0.0]),
        (999.0, [99.9, 0.1, 99.9, 0.1, 99.9, 0.0]),
        (1000.0, [0.0, 100.0, 100.0, 0.0, 100.0, 0.0]),
        (1250.0, [25.0, 75.0, 75.0, 25.0, 75.0, 0.0]),
        (1999.0, [99.9, 0.1, 0.1, 99.9, 0.1, 0.0]),
        (2000.0, [0.0, 0.0, 0.0, 100.0, 0.0, 0.0]),
        (2250.0, [25.0, 0.0, 25.0, 75.0, 25.0, 0.0]),
        (2499.0, [49.9, 0.0, 49.9, 50.1, 49.9, 0.0]),
        (2500.0, [50.0, 0.0, 50.0, 50.0, 50.0, 0.0]),
        (2750.0, [50.0, 0.0, 75.0, 25.0, 75.0, 0.0]),
        (3500.0, [50.0, 0.0, 100.0, 0.0, 50.0, 0.0]),
        (10250.0, [50.0, 0.0, 100.0, 0.0, 25.0, 0.0]),
    ];
    let make = |(iterations, direction): (f64, PlaybackDirection)| Timing {
        direction,
        fill: FillMode::Both,
        ..timing(1000.0, 0.0, iterations)
    };
    for (time, values) in rows {
        for (effect, want) in effects.iter().zip(values) {
            let got = value(&make(*effect), time, 0.0, 100.0, -1.0);
            assert!(close(got, want), "{effect:?} at {time}: {got} != {want}");
        }
    }
    // Query 1: current iteration and progress at 2250 ms.
    let want = [
        (2.0, 0.25),
        (1.0, 0.0),
        (2.0, 0.25),
        (2.0, 0.75),
        (2.0, 0.25),
        (0.0, 0.0),
    ];
    for (effect, (iteration, progress)) in effects.iter().zip(want) {
        let c = make(*effect).compute(Some(2250.0));
        assert_eq!(c.current_iteration, Some(iteration), "{effect:?}");
        assert!(close(c.progress.unwrap_or(-1.0), progress), "{effect:?}");
    }
}

#[test]
fn hero_reveal_delay_and_fill_both() {
    // `hero-reveal`: 1.1 s, delay 300 ms, fill both; the keyframe easing
    // `cubic-bezier(.2,.8,.2,1)` takes the iteration progress. Opacity 0
    // to 1 (query 5).
    let t = Timing {
        fill: FillMode::Both,
        ..timing(1100.0, 300.0, 1.0)
    };
    assert!(close(t.end_time(), 1400.0));
    let soft = parse_all("cubic-bezier(.2,.8,.2,1)", parse_easing).expect("valid");
    let rows = [
        (0.0, 0.0),
        (299.0, 0.0),
        (300.0, 0.0),
        (350.0, 0.183855),
        (850.0, 0.946079),
        (1400.0, 1.0),
        (1401.0, 1.0),
        (2000.0, 1.0),
    ];
    for (time, want) in rows {
        let c = t.compute(Some(time));
        let p = c.progress.expect("fill both");
        let got = soft.evaluate(p, c.before_flag);
        assert!(close(got, want), "{time}: {got} != {want}");
    }
}

#[test]
fn spinner_steps_with_delays() {
    // `spinner-steps`: `media-spinner-fade 1s steps(8, end) infinite`,
    // opacity 1 to 0 over a base of 0.5, no fill; queries 1 to 3 (delays
    // 0, 375 and 875 ms).
    let steps = Easing::Steps {
        count: 8,
        position: StepPosition::End,
        math: None,
    };
    let rows: [(f64, &[(f64, f64)]); 3] = [
        (
            0.0,
            &[
                (0.0, 1.0),
                (1.0, 1.0),
                (124.0, 1.0),
                (125.0, 0.875),
                (126.0, 0.875),
                (499.0, 0.625),
                (500.0, 0.5),
                (999.0, 0.125),
                (1000.0, 1.0),
                (1124.0, 1.0),
                (1125.0, 0.875),
                (2500.0, 0.5),
            ],
        ),
        (
            375.0,
            &[
                (0.0, 0.5),
                (374.0, 0.5),
                (375.0, 1.0),
                (376.0, 1.0),
                (499.0, 1.0),
                (500.0, 0.875),
                (875.0, 0.5),
                (1374.0, 0.125),
                (1375.0, 1.0),
                (1400.0, 1.0),
                (2375.0, 1.0),
            ],
        ),
        (
            875.0,
            &[
                (0.0, 0.5),
                (500.0, 0.5),
                (874.0, 0.5),
                (875.0, 1.0),
                (876.0, 1.0),
                (1000.0, 0.875),
                (1875.0, 1.0),
                (2000.0, 0.875),
            ],
        ),
    ];
    for (delay, samples) in rows {
        let t = timing(1000.0, delay, f64::INFINITY);
        for (time, want) in samples {
            let c = t.compute(Some(*time));
            let got = c
                .progress
                .map_or(0.5, |p| 1.0 - steps.evaluate(p, c.before_flag));
            assert!(close(got, *want), "delay {delay} at {time}: {got}");
        }
    }
}

#[test]
fn marquee_infinite_iterations() {
    // `marquee`: 30 s linear infinite, translate from -579px to 0.
    let t = timing(30_000.0, 0.0, f64::INFINITY);
    assert!(t.active_duration().is_infinite());
    assert!(t.end_time().is_infinite());
    let rows = [
        (0.0, -579.0),
        (1000.0, -559.7),
        (7500.0, -434.25),
        (15000.0, -289.5),
        (29999.0, -0.0193),
        (30000.0, -579.0),
        (37500.0, -434.25),
        (60000.0, -579.0),
        (95000.0, -482.5),
    ];
    for (time, want) in rows {
        let got = value(&t, time, -579.0, 0.0, 0.0);
        assert!(close(got, want), "{time}: {got} != {want}");
    }
    // Query 2 at 95 s: iteration 3, progress 1/6; query 3 at 37.5 s.
    let c = t.compute(Some(95_000.0));
    assert_eq!(c.current_iteration, Some(3.0));
    assert!(close(c.progress.unwrap_or(0.0), 0.166_666_666_666_666_52));
    assert_eq!(t.compute(Some(37_500.0)).current_iteration, Some(1.0));
}

#[test]
fn edge_cases() {
    // No local time: idle, no value.
    let t = timing(1000.0, 0.0, 1.0);
    assert_eq!(t.compute(None).phase, Phase::Idle);
    assert_eq!(t.compute(None).progress, None);
    // A zero duration: the after phase at once; with a backwards fill the
    // before phase is at the iteration start.
    let zero = Timing {
        fill: FillMode::Both,
        ..timing(0.0, 100.0, 3.0)
    };
    assert_eq!(zero.compute(Some(50.0)).progress, Some(0.0));
    let c = zero.compute(Some(100.0));
    assert_eq!(c.phase, Phase::After);
    assert_eq!(c.current_iteration, Some(2.0));
    assert_eq!(c.progress, Some(1.0));
    // A negative delay starts inside the interval.
    let early = timing(1000.0, -250.0, 1.0);
    assert_eq!(early.compute(Some(0.0)).progress, Some(0.25));
    // A delay longer than the end: before phase until the end time.
    let late = Timing {
        delay: -2000.0,
        fill: FillMode::Forwards,
        ..timing(1000.0, 0.0, 1.0)
    };
    assert_eq!(late.end_time(), 0.0);
    assert_eq!(late.compute(Some(0.0)).phase, Phase::After);
    // An infinite 1 ms animation seeked far: computed, not counted.
    let fast = timing(1.0, 0.0, f64::INFINITY);
    let c = fast.compute(Some(1e9 + 0.5));
    assert_eq!(c.current_iteration, Some(1e9));
    // The before flag: before phase going forwards.
    let back = Timing {
        fill: FillMode::Backwards,
        ..timing(1000.0, 500.0, 1.0)
    };
    assert!(back.compute(Some(0.0)).before_flag);
    assert!(!back.compute(Some(600.0)).before_flag);
    // After phase going backwards.
    let rev = Timing {
        fill: FillMode::Forwards,
        direction: PlaybackDirection::Reverse,
        ..timing(1000.0, 0.0, 1.0)
    };
    assert!(rev.compute(Some(2000.0)).before_flag);
}

/// The largest times of `number-ranges` (`calc(1s/0)` computes to
/// 1.79769e+308 s, which `AnimationStyle::entry` makes the largest `f64` in
/// ms): no end time or progress is NaN, whatever the combination.
#[test]
fn extreme_times_give_no_nan() {
    let big = [-f64::MAX, -1e300, 0.0, 1.0, 1e300, f64::MAX];
    for &duration in &[0.0, 1.0, 1e300, f64::MAX] {
        for &delay in &big {
            for &iterations in &[0.0, 0.5, 1.0, f64::MAX, f64::INFINITY] {
                for fill in [FillMode::None, FillMode::Both] {
                    let t = Timing {
                        fill,
                        direction: PlaybackDirection::AlternateReverse,
                        ..timing(duration, delay, iterations)
                    };
                    assert!(!t.end_time().is_nan());
                    for &local in &big {
                        let c = t.compute(Some(local));
                        let ok = c.progress.is_none_or(f64::is_finite);
                        assert!(ok, "{duration} {delay} {iterations} at {local}: {c:?}");
                    }
                }
            }
        }
    }
}
