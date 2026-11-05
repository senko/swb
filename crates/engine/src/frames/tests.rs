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
