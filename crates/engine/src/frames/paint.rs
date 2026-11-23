//! The frames for the rasterizer (ADR 0031 section 4): [`FrameSource`] gives
//! the paint crate the resources of a document, with the display lists of
//! its frames, without engine types in that crate.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use swb_dom::NodeId;
use swb_paint::{DecodedImage, FrameContent, ImageRef, ImageSource, VectorCache};

use super::{FrameId, Frames};
use crate::resources::Images;

/// The images of the document of frame `frame`, and its frames.
pub(crate) struct FrameSource<'a> {
    images: &'a Images,
    frames: &'a Frames,
    frame: FrameId,
}

impl<'a> FrameSource<'a> {
    /// The source of frame `frame`, whose document has `images`.
    pub(crate) fn new(images: &'a Images, frames: &'a Frames, frame: FrameId) -> Self {
        FrameSource {
            images,
            frames,
            frame,
        }
    }
}

impl ImageSource for FrameSource<'_> {
    fn image(&self, image: &ImageRef) -> Option<&DecodedImage> {
        self.images.image(image)
    }

    fn vector_cache(&self) -> Option<&VectorCache> {
        self.images.vector_cache()
    }

    fn raster_cache(&self) -> Option<&swb_paint::RasterCache> {
        self.images.raster_cache()
    }

    fn frame(&self, node: NodeId) -> Option<FrameContent<'_>> {
        let child = self.frames.by_container(self.frame, node)?;
        let record = self.frames.get(child)?;
        let doc = record.doc.as_deref()?;
        Some(FrameContent {
            list: doc.display_list.as_deref()?,
            ends: &doc.list_ends,
            scroll: doc.scroll,
            canvas: record.canvas,
            source: Box::new(FrameSource::new(&doc.images, self.frames, child)),
            generation: subtree_generation(self.frames, child),
        })
    }

    fn generation(&self, image: &ImageRef) -> u64 {
        self.images.generation(image)
    }
}

/// A number that changes when what the frame `frame` or a frame inside it
/// draws changes: the paint stamps of their documents, their viewports and
/// canvases (ADR 0044 section 18).
fn subtree_generation(frames: &Frames, frame: FrameId) -> u64 {
    let mut hasher = DefaultHasher::new();
    let mut ids = frames.descendants_post_order(frame);
    ids.push(frame);
    for id in ids {
        let Some(record) = frames.get(id) else {
            continue;
        };
        id.hash(&mut hasher);
        record.has_box.hash(&mut hasher);
        record.viewport.width.to_bits().hash(&mut hasher);
        record.viewport.height.to_bits().hash(&mut hasher);
        record.canvas.hash(&mut hasher);
        record
            .doc
            .as_deref()
            .map(crate::document::DocumentState::paint_generation)
            .hash(&mut hasher);
    }
    hasher.finish()
}
