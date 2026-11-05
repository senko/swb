//! The frames for the rasterizer (ADR 0031 section 4): [`FrameSource`] gives
//! the paint crate the resources of a document, with the display lists of
//! its frames, without engine types in that crate.

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

    fn frame(&self, node: NodeId) -> Option<FrameContent<'_>> {
        let child = self.frames.by_container(self.frame, node)?;
        let record = self.frames.get(child)?;
        let doc = record.doc.as_deref()?;
        Some(FrameContent {
            list: doc.display_list.as_ref()?,
            ends: &doc.list_ends,
            scroll: doc.scroll,
            canvas: record.canvas,
            source: Box::new(FrameSource::new(&doc.images, self.frames, child)),
        })
    }
}
