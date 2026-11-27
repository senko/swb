//! What a `<video>` element shows (M14 feature 7a, ADR 0047 sections 6 and
//! 9): the natural size of its stream, its current frame, whether it shows
//! its poster instead, and whether it has a box (a video without one
//! converts no frame). The document's `Images` take these views
//! ([`crate::DocumentState::sync_media_images`]); the display list does not
//! change when a frame does.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use swb_dom::NodeId;
use swb_paint::Pixmap;

use super::backend::Fetch;
use super::{Element, Media};

/// What a video element shows now.
pub(crate) struct VideoView {
    /// The natural size of the stream (`videoWidth`, `videoHeight`), after
    /// the metadata.
    pub(crate) natural: Option<(u32, u32)>,
    /// HTML's show poster flag.
    pub(crate) show_poster: bool,
    /// The pixels of the current frame, if it has some.
    pub(crate) frame: Option<Arc<Pixmap>>,
}

impl Element {
    /// What the element shows: `None` for an `audio` element.
    fn video_view(&self) -> Option<VideoView> {
        if !self.video {
            return None;
        }
        Some(VideoView {
            natural: self.meta.as_ref().and_then(|m| m.size),
            show_poster: self.show_poster,
            frame: self.fetch.as_ref().and_then(Fetch::frame_image),
        })
    }
}

impl Media {
    /// Lets the pipelines of the video elements take the frames that are due
    /// at page time `now_us`, so that the next paint shows the frame at the
    /// media time of the page clock (`Page::screenshot`, the GUI's frame).
    /// The reports that this takes wait in the element's notes for the next
    /// step. A suspended pipeline keeps its frame.
    pub(crate) fn poll_frames(&mut self, now_us: i64) {
        for element in self.elements.values_mut() {
            if !element.video {
                continue;
            }
            if let Some(fetch) = element.fetch.as_mut()
                && !fetch.is_suspended()
            {
                fetch.poll(now_us);
            }
        }
    }

    /// Whether a video that is painted has a converted frame due at page
    /// time `now_us` that is not the one it shows (M14 feature 7b, ADR 0047
    /// "Implementation notes (M14 feature 7b)": the frame decision). It runs
    /// at each batch of the GUI: it converts and polls nothing, and an
    /// element without a running pipeline costs a test of its flags.
    pub(crate) fn frame_due(&self, now_us: i64) -> bool {
        self.elements.values().any(|element| {
            element.video
                && element.painted
                && element
                    .fetch
                    .as_ref()
                    .is_some_and(|fetch| fetch.frame_due(now_us))
        })
    }

    /// The counters of the video pipelines, summed.
    pub(crate) fn counters(&self) -> super::backend::MediaCounters {
        let mut sum = super::backend::MediaCounters::default();
        for element in self.elements.values() {
            if element.video
                && let Some(fetch) = &element.fetch
            {
                sum += fetch.counters();
            }
        }
        sum
    }

    /// The synchronization of each video that plays and is painted at page
    /// time `now_us`: the frame that is current and the media time (the
    /// frame log's sync trace, M14 feature 8).
    pub(crate) fn sync_samples(&self, now_us: i64) -> Vec<super::backend::MediaSync> {
        self.elements
            .values()
            .filter(|e| e.video && e.painted && !e.paused)
            .filter_map(|e| e.fetch.as_ref()?.sync_sample(now_us))
            .collect()
    }

    /// What the video element `node` shows; `None` for an element without a
    /// state and for an `audio` element.
    pub(crate) fn video_view(&self, node: NodeId) -> Option<VideoView> {
        self.elements.get(&node)?.video_view()
    }

    /// The views of the video elements that have a media state, by node.
    pub(crate) fn video_views(&self) -> Vec<(NodeId, VideoView)> {
        self.elements
            .iter()
            .filter_map(|(&node, e)| Some((node, e.video_view()?)))
            .collect()
    }

    /// Tells the video elements which of them are painted after a layout
    /// (`painted`): the others convert no frame ([`swb_media`]'s hidden
    /// elements, ADR 0047 section 6), and an element that is painted again
    /// shows the frame at its position. `boxed` (when known) are the ones
    /// that have a painted box anywhere, with the size of the box for a
    /// retained frame ([`Element::box_size`]), for the budget of retained
    /// frames (M14 feature 10a); an element whose reduced frame is smaller
    /// than its box now gets its full frame again. Returns true if one
    /// changed.
    pub(crate) fn set_painted(
        &mut self,
        painted_set: &HashSet<NodeId>,
        boxed: Option<&HashMap<NodeId, Option<(f32, f32)>>>,
    ) -> bool {
        self.paint_tick += 1;
        let tick = self.paint_tick;
        let mut changed = false;
        for (node, element) in &mut self.elements {
            if !element.video {
                continue;
            }
            let painted = painted_set.contains(node);
            if painted {
                element.painted_at = tick;
            }
            if let Some(boxed) = boxed {
                element.has_box = boxed.contains_key(node);
                element.box_size = boxed.get(node).copied().flatten();
            }
            if element.painted != painted {
                element.painted = painted;
                if let Some(fetch) = element.fetch.as_mut() {
                    fetch.set_painted(painted);
                }
                // A suspended pipeline may open again (its retained frame).
                self.touched.insert(*node);
                changed = true;
            }
            if painted && element.reduced_frame_too_small() {
                if let Some(fetch) = element.fetch.as_mut() {
                    fetch.restore_full_frame();
                }
                self.touched.insert(*node);
            }
        }
        changed
    }

    /// The frames that the pipeline of the video `node` converted (tests).
    pub(crate) fn frames_converted(&self, node: NodeId) -> Option<u64> {
        Some(self.elements.get(&node)?.fetch.as_ref()?.frames_converted())
    }
}
