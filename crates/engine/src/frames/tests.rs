//! Tests of the frame records that need no document. The tree with
//! documents is tested through the page (`tests/frames.rs`).

use super::*;

#[test]
fn frame_ids_round_trip_and_stale_ids_find_nothing() {
    let id = FrameId {
        index: 7,
        generation: 3,
    };
    assert_eq!(FrameId::from_u64(id.to_u64()), id);
    assert_eq!(FrameId::from_u64(0), FrameId::TOP);
    // The number in the script runtime: below 2^30 (bit 30 names a `Location`), the
    // top level is 0.
    assert_eq!(FrameId::from_number(id.number()), id);
    assert_eq!(FrameId::TOP.number(), 0);
    let last = FrameId {
        index: 1023,
        generation: MAX_GENERATION - 1,
    };
    assert_eq!(FrameId::from_number(last.number()), last);
    assert!(last.number() < 1 << 30);
    let frames = Frames::new();
    assert!(frames.get(FrameId::TOP).is_some());
    assert!(
        frames
            .get(FrameId {
                index: 0,
                generation: 1
            })
            .is_none()
    );
    assert!(frames.get(id).is_none());
    assert_eq!(frames.count(), 0);
    assert_eq!(frames.tree_order(), [FrameId::TOP]);
    assert_eq!(frames.descendants_post_order(FrameId::TOP).len(), 0);
}

/// A slot goes back on the free list until its generation reaches the limit of a
/// frame number; then it stays empty, so that a number is never used twice.
#[test]
fn a_slot_retires_at_the_last_generation() {
    let mut frames = Frames::new();
    for generation in [MAX_GENERATION - 2, MAX_GENERATION - 1] {
        let index = frames.slots.len() as u32;
        frames.slots.push(Slot {
            generation,
            frame: Some(Frame::new(Some(FrameId::TOP), None, 1)),
        });
        frames.count += 1;
        let id = FrameId { index, generation };
        assert!(frames.remove(id).is_some());
        assert_eq!(
            frames.free.contains(&index),
            generation + 1 < MAX_GENERATION,
            "generation {generation}"
        );
        assert!(frames.get(id).is_none());
    }
}

#[test]
fn the_top_level_frame_cannot_be_removed() {
    let mut frames = Frames::new();
    assert!(frames.remove(FrameId::TOP).is_none());
    assert!(frames.get(FrameId::TOP).is_some());
}

#[test]
fn navigations_are_limited_per_window_of_page_time() {
    let mut frame = Frame::new(Some(FrameId::TOP), None, 1);
    for i in 0..MAX_NAVIGATIONS {
        assert!(frame.note_navigation(i as f64), "navigation {i}");
    }
    assert!(!frame.note_navigation(100.0));
    // The first one leaves the window 10 s after it.
    assert!(!frame.note_navigation(NAVIGATION_WINDOW_MS - 1.0));
    assert!(frame.note_navigation(NAVIGATION_WINDOW_MS));
    assert!(!frame.note_navigation(NAVIGATION_WINDOW_MS));
}
