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

/// A frame has the document of one key in one runtime: an equal key of another runtime
/// is another document (ADR 0031 section 5).
#[test]
fn a_key_of_another_runtime_names_no_document() {
    let mut frame = Frame::new(Some(FrameId::TOP), None, 1);
    let (one, two) = (AgentId::new(1), AgentId::new(2));
    assert!(!frame.has_document(one, DocumentKey::FIRST));
    frame.script = Some(FrameScript::new(one, DocumentKey::FIRST));
    assert!(frame.has_document(one, DocumentKey::FIRST));
    assert!(!frame.has_document(two, DocumentKey::FIRST));
}

/// The storage key of a frame has the top-level site only if some document from the top
/// level down to the frame is of another site (measured, cases `storage-key-*`); a top
/// level or a frame without a site has no key.
#[test]
fn the_storage_partition_follows_the_chain_of_sites() {
    use swb_web::StoragePartition::{FirstParty, Top, Unavailable};
    let site = |text: &str| -> Option<Rc<str>> { Some(Rc::from(text)) };
    let mut frames = Frames::new();
    let add = |frames: &mut Frames, parent, depth, at: &str| {
        let index = frames.slots.len() as u32;
        let id = FrameId {
            index,
            generation: 0,
        };
        let mut frame = Frame::new(Some(parent), None, depth);
        frame.storage_site = site(at);
        frames.slots.push(Slot {
            generation: 0,
            frame: Some(frame),
        });
        frames.get_mut(parent).unwrap().children.push(id);
        id
    };
    frames.get_mut(FrameId::TOP).unwrap().storage_site = site("http://a.test");
    // Under the top level: its own site, or another one.
    assert_eq!(
        frames.storage_partition(FrameId::TOP, Some("http://a.test")),
        FirstParty
    );
    assert_eq!(
        frames.storage_partition(FrameId::TOP, Some("http://l.test")),
        Top("http://a.test".into())
    );
    // A > B > A': the cross-site document between counts.
    let b = add(&mut frames, FrameId::TOP, 1, "http://b.test");
    assert_eq!(
        frames.storage_partition(b, Some("http://a.test")),
        Top("http://a.test".into())
    );
    // A > A'' > A''': all of one site.
    let a2 = add(&mut frames, FrameId::TOP, 1, "http://a.test");
    assert_eq!(
        frames.storage_partition(a2, Some("http://a.test")),
        FirstParty
    );
    // A document without a site, and a top level without one.
    assert_eq!(frames.storage_partition(a2, None), Unavailable);
    frames.get_mut(FrameId::TOP).unwrap().storage_site = None;
    assert_eq!(
        frames.storage_partition(a2, Some("http://a.test")),
        Unavailable
    );
}

/// HTML "activation notification", as Chromium measures (cases `activation-xo-*`): the
/// frame, its ancestors and the frames of its origin; not a child or a sibling of
/// another origin.
#[test]
fn an_activation_reaches_ancestors_and_the_origin() {
    let origin = |host: &str| Url::parse(host).unwrap().origin();
    let mut frames = Frames::new();
    let add = |frames: &mut Frames, parent, depth, at: &str| {
        let index = frames.slots.len() as u32;
        let id = FrameId {
            index,
            generation: 0,
        };
        let mut frame = Frame::new(Some(parent), None, depth);
        frame.origin = Some(origin(at));
        frames.slots.push(Slot {
            generation: 0,
            frame: Some(frame),
        });
        frames.get_mut(parent).unwrap().children.push(id);
        id
    };
    frames.get_mut(FrameId::TOP).unwrap().origin = Some(origin("http://top.test/"));
    let clicked = add(&mut frames, FrameId::TOP, 1, "http://a.test/");
    let same_child = add(&mut frames, clicked, 2, "http://a.test/");
    let other_child = add(&mut frames, clicked, 2, "http://c.test/");
    let same_sibling = add(&mut frames, FrameId::TOP, 1, "http://a.test/");
    let other_sibling = add(&mut frames, FrameId::TOP, 1, "http://b.test/");
    frames.note_activation(clicked);
    for id in [FrameId::TOP, clicked, same_child, same_sibling] {
        assert!(frames.has_sticky_activation(id), "{id}");
        assert!(frames.has_transient_activation(id), "{id}");
    }
    for id in [other_child, other_sibling] {
        assert!(!frames.has_sticky_activation(id), "{id}");
        assert!(!frames.has_transient_activation(id), "{id}");
    }
}
