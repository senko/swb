//! Tests of the animation records: the play states, seeking, the update
//! of the rendering update and the next time a record needs
//! (ADR 0044 sections 2 and 8).

use super::*;
use crate::animation::easing::StepPosition;
use crate::animation::timing::FillMode;

fn owner() -> AnimationOwner {
    AnimationOwner {
        node: NodeId::DOCUMENT,
        pseudo: None,
    }
}

/// A running record with `timing` that started at the timeline time
/// `start`.
fn running(timing: Timing, start: f64) -> AnimationRecord {
    let mut r = AnimationRecord::new(1, owner(), Arc::from("a"), 0, 0);
    r.timing = timing;
    r.start_time = Some(start);
    r
}

fn timing(delay: f64, duration: f64, iterations: f64) -> Timing {
    Timing {
        delay,
        duration,
        iterations,
        ..Timing::default()
    }
}

#[test]
fn a_new_animation_plays_at_the_next_update() {
    let mut r = AnimationRecord::new(1, owner(), Arc::from("a"), 0, 0);
    r.timing = timing(0.0, 1000.0, 1.0);
    r.play(100.0);
    assert_eq!(r.current_time(100.0), Some(0.0));
    assert!(r.pending());
    assert_eq!(r.play_state(100.0), AnimationPlayState::Running);
    // The pending play waits for the next update: the next time is now.
    assert_eq!(r.next_time(100.0, false), Some(100.0));
    r.resolve_pending(116.0);
    assert_eq!(r.start_time(), Some(116.0));
    assert_eq!(r.current_time(616.0), Some(500.0));
    assert!(!r.pending());
}

#[test]
fn pause_seek_and_play() {
    let mut r = running(timing(0.0, 1000.0, 1.0), 0.0);
    r.pause(300.0);
    assert_eq!(r.play_state(300.0), AnimationPlayState::Paused);
    assert_eq!(r.current_time(900.0), Some(300.0));
    // A seek completes the pending pause.
    r.set_current_time(700.0, 900.0);
    assert!(!r.pending());
    assert_eq!(r.current_time(5000.0), Some(700.0));
    assert_eq!(r.next_time(5000.0, true), None);
    r.play(5000.0);
    r.resolve_pending(5000.0);
    assert_eq!(r.current_time(5100.0), Some(800.0));
    // A seek of a running animation moves its start time.
    r.set_current_time(100.0, 5100.0);
    assert_eq!(r.current_time(5200.0), Some(200.0));
}

#[test]
fn finished_animations_restart_on_play() {
    let mut r = running(timing(0.0, 1000.0, 1.0), 0.0);
    assert_eq!(r.play_state(1000.0), AnimationPlayState::Finished);
    assert_eq!(r.next_time(1000.0, true), None);
    r.play(2000.0);
    r.resolve_pending(2000.0);
    assert_eq!(r.current_time(2500.0), Some(500.0));
}

#[test]
fn relevance_follows_the_phase_and_the_fill() {
    let mut r = running(timing(0.0, 1000.0, 1.0), 0.0);
    assert!(r.is_relevant(500.0));
    assert!(!r.is_relevant(1500.0));
    r.timing.fill = FillMode::Forwards;
    assert!(r.is_relevant(1500.0));
    // Paused past the end without a fill: not relevant (Chromium 148).
    r.timing.fill = FillMode::None;
    r.pause(1500.0);
    assert!(!r.is_relevant(1500.0));
    r.set_current_time(-10.0, 1500.0);
    assert!(r.is_relevant(1500.0));
}

#[test]
fn without_frames_only_the_event_times_count() {
    // A delay of 300, then 3 iterations of 1000.
    let r = running(timing(300.0, 1000.0, 3.0), 0.0);
    assert_eq!(r.next_time(0.0, false), Some(300.0));
    assert_eq!(r.next_time(300.0, false), Some(1300.0));
    assert_eq!(r.next_time(1300.0, false), Some(2300.0));
    assert_eq!(r.next_time(2900.0, false), Some(3300.0));
    assert_eq!(r.next_time(3300.0, false), None);
    // An infinite marquee of 30 s: one update per iteration.
    let r = running(timing(0.0, 30_000.0, f64::INFINITY), 0.0);
    assert_eq!(r.next_time(1.0, false), Some(30_000.0));
    assert_eq!(r.next_time(95_000.0, false), Some(120_000.0));
}

#[test]
fn with_frames_continuous_easings_need_each_frame() {
    let r = running(timing(0.0, 1000.0, 1.0), 0.0);
    let next = r.next_time(100.0, true).unwrap();
    assert!((next - (100.0 + FRAME_MS)).abs() < 1e-9);
    // The end of the iteration comes first when it is closer.
    assert_eq!(r.next_time(995.0, true), Some(1000.0));
    // Before the delay nothing changes until it ends.
    let r = running(timing(500.0, 1000.0, 1.0), 0.0);
    assert_eq!(r.next_time(100.0, true), Some(500.0));
}

#[test]
fn steps_need_their_boundaries_only() {
    // The spinner: `steps(8)` over 1 s, 8 updates per second.
    let mut r = running(timing(0.0, 1000.0, f64::INFINITY), 0.0);
    r.step_period = Some(125.0);
    assert_eq!(r.next_time(0.0, true), Some(125.0));
    assert_eq!(r.next_time(130.0, true), Some(250.0));
    assert_eq!(r.next_time(1000.0, true), Some(1125.0));
    // Steps shorter than a frame count as continuous.
    r.step_period = Some(5.0);
    let next = r.next_time(10.0, true).unwrap();
    assert!((next - (10.0 + FRAME_MS)).abs() < 1e-9);
}

#[test]
fn the_step_period_of_a_keyframe_set() {
    let steps = Easing::Steps {
        count: 8,
        position: StepPosition::End,
        math: None,
    };
    let set = KeyframeSet::default();
    assert_eq!(
        AnimationRecord::step_period_of(&set, &steps, 1000.0),
        Some(125.0)
    );
    assert_eq!(
        AnimationRecord::step_period_of(&set, &Easing::Linear, 1000.0),
        None
    );
}

#[test]
fn update_marks_owners_whose_output_changes() {
    let mut animations = DocumentAnimations::new(0.0);
    let id = animations
        .create(owner(), "a", 0, 0)
        .expect("below the limit");
    if let Some(r) = animations.records.get_mut(&id) {
        r.timing = timing(0.0, 1000.0, 1.0);
        r.play(0.0);
    }
    assert!(animations.update(0.0));
    assert_eq!(animations.take_marked(), vec![NodeId::DOCUMENT]);
    assert!(animations.update(100.0));
    animations.take_marked();
    // Paused: the output stays.
    animations.pause(id);
    animations.take_marked();
    assert!(!animations.update(200.0));
    assert!(!animations.has_marked());
    // A seek marks the owner at once.
    animations.set_current_time(id, 50.0);
    assert!(animations.has_marked());
    // A paused animation needs no update.
    assert_eq!(animations.next_time(true), None);
}

#[test]
fn the_record_limit_creates_no_more() {
    let mut animations = DocumentAnimations::new(0.0);
    for i in 0..MAX_RECORDS {
        assert!(animations.create(owner(), "a", i, i).is_some());
    }
    assert!(animations.create(owner(), "a", 0, 0).is_none());
    assert_eq!(animations.len(), MAX_RECORDS);
}

#[test]
fn released_records_are_reported() {
    let mut animations = DocumentAnimations::new(0.0);
    animations.track_released();
    let id = animations.create(owner(), "a", 0, 0).expect("created");
    animations.owners.insert(owner(), vec![id]);
    animations.retain_owners(|_| false);
    assert!(animations.is_empty());
    assert_eq!(animations.take_released(), vec![id]);
    assert_eq!(animations.take_released(), Vec::<u64>::new());
}

#[test]
fn released_ids_are_kept_only_while_the_bindings_track_them() {
    let mut animations = DocumentAnimations::new(0.0);
    // Churn without wrappers: nothing accumulates.
    for _ in 0..1000 {
        let id = animations.create(owner(), "a", 0, 0).expect("created");
        animations.owners.insert(owner(), vec![id]);
        animations.retain_owners(|_| false);
    }
    assert!(animations.is_empty());
    assert_eq!(animations.released, Vec::<u64>::new());
    assert_eq!(animations.take_released(), Vec::<u64>::new());
    animations.track_released();
    let id = animations.create(owner(), "a", 0, 0).expect("created");
    animations.owners.insert(owner(), vec![id]);
    animations.retain_owners(|_| false);
    assert_eq!(animations.take_released(), vec![id]);
}

#[test]
fn every_step_position_jumps_at_multiples_of_the_period() {
    // The scheduling treats the interval as `duration / n` for every
    // position; check that the output of `steps()` changes exactly there
    // (`jump-both` has n + 1 levels and `jump-none` n - 1, but the jumps
    // stay at k / n of the iteration).
    let n = 4u32;
    for position in [
        StepPosition::End,
        StepPosition::Start,
        StepPosition::JumpStart,
        StepPosition::JumpNone,
        StepPosition::JumpBoth,
    ] {
        let easing = Easing::Steps {
            count: n,
            position,
            math: None,
        };
        assert_eq!(
            AnimationRecord::step_period_of(&KeyframeSet::default(), &easing, 1000.0),
            Some(250.0),
            "{position:?}"
        );
        let at = |t: f64| easing.evaluate(t, false);
        for k in 1..n {
            let boundary = f64::from(k) / f64::from(n);
            assert_ne!(at(boundary - 1e-6), at(boundary + 1e-6), "{position:?} {k}");
            // Constant between two boundaries.
            let from = boundary + 1e-6;
            let to = f64::from(k + 1) / f64::from(n) - 1e-6;
            assert_eq!(at(from), at(to), "{position:?} {k}");
            assert_eq!(at(from), at(f64::midpoint(from, to)), "{position:?} {k}");
        }
        assert_eq!(at(1e-6), at(0.25 - 1e-6), "{position:?} first interval");
    }
}

#[test]
fn a_huge_time_has_no_next_event() {
    // At 1e19 ms the boundary of the next iteration is not after the time.
    let r = running(timing(0.0, 1000.0, f64::INFINITY), -1e19);
    let timeline = 1000.0;
    assert_eq!(r.next_time(timeline, false), None);
    assert_eq!(r.next_time(timeline, true), Some(timeline + FRAME_MS));
}

#[test]
fn the_next_time_is_cached_until_a_record_changes() {
    let mut animations = DocumentAnimations::new(0.0);
    let id = animations.create(owner(), "a", 0, 0).expect("created");
    if let Some(r) = animations.records.get_mut(&id) {
        r.timing = timing(0.0, 1000.0, f64::INFINITY);
        r.play(0.0);
    }
    animations.update(0.0);
    assert_eq!(animations.next_time(false), Some(1000.0));
    // The cache holds the value of the update: a change behind its back is
    // not seen until something invalidates it.
    if let Some(r) = animations.records.get_mut(&id) {
        r.timing = timing(0.0, 500.0, f64::INFINITY);
    }
    assert_eq!(animations.next_time(false), Some(1000.0));
    animations.invalidate();
    assert_eq!(animations.next_time(false), Some(500.0));
    // A script's pause invalidates it.
    animations.pause(id);
    assert_eq!(animations.next_time(true), Some(0.0));
    animations.update(16.0);
    assert_eq!(animations.next_time(true), None);
    // A play resumes it.
    animations.play(id);
    animations.update(32.0);
    assert!(animations.next_time(false).is_some());
}

#[test]
fn a_catch_up_runs_when_settled_not_when_requested() {
    let mut animations = DocumentAnimations::new(0.0);
    let id = animations.create(owner(), "a", 0, 0).expect("created");
    if let Some(r) = animations.records.get_mut(&id) {
        r.timing = timing(0.0, 1000.0, f64::INFINITY);
        r.play(0.0);
    }
    animations.update(0.0);
    assert!(!animations.unsettled());
    animations.request_catch_up(5000.0);
    animations.request_catch_up(3000.0);
    assert!(animations.unsettled());
    assert_eq!(animations.updates(), 1);
    animations.settle();
    assert_eq!(animations.updates(), 2);
    assert_eq!(animations.time(), 5000.0);
    assert!(!animations.unsettled());
    animations.settle();
    assert_eq!(animations.updates(), 2);
}
