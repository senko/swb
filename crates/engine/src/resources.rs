//! Subresources of a page: stylesheets and images, and the requests that
//! load them.

use std::collections::HashMap;
use std::sync::Arc;

use swb_dom::NodeId;
use swb_net::{NetError, RequestId, Response, Url};
use swb_paint::{DecodedImage, ImageRef, ImageSizes, ImageSource, Pixmap};

/// What an in-flight request is for.
#[derive(Clone, Debug)]
pub(crate) enum Pending {
    /// The document of the pending navigation. Starting a navigation
    /// replaces all pending requests, so there is at most one.
    Document,
    /// The stylesheet in this slot of the author sheet list.
    Stylesheet { slot: usize },
    /// An image.
    Image { url: Url },
}

/// An author stylesheet slot: inline `<style>` text or a `<link>` load.
#[derive(Debug)]
pub(crate) struct SheetSlot {
    /// The CSS text, once available. `None` while loading or after an
    /// error (`failed` tells which).
    pub(crate) css: Option<String>,
    /// The URL relative `url()` values resolve against.
    pub(crate) base_url: Url,
    /// The `media` attribute.
    pub(crate) media: Option<String>,
    pub(crate) failed: bool,
}

impl SheetSlot {
    pub(crate) fn is_settled(&self) -> bool {
        self.css.is_some() || self.failed
    }
}

/// The state of an image URL.
#[derive(Debug)]
pub(crate) enum ImageState {
    Loading,
    Loaded(Arc<DecodedImage>),
    Failed,
}

impl ImageState {
    /// The state after a fetch of the image at `url` completed.
    pub(crate) fn from_fetch(url: &Url, result: Result<Response, NetError>) -> Self {
        match result {
            Ok(response) if response.is_success() => Self::decode(url, &response.body),
            Ok(response) => {
                log::warn!("image {url}: HTTP {}", response.status);
                ImageState::Failed
            }
            Err(e) => {
                log::warn!("image {url}: {e}");
                ImageState::Failed
            }
        }
    }

    /// Decodes the image data of `url`.
    pub(crate) fn decode(url: &Url, data: &[u8]) -> Self {
        match swb_paint::decode(data) {
            Ok(image) => ImageState::Loaded(Arc::new(image)),
            Err(e) => {
                log::warn!("image {url}: {e}");
                ImageState::Failed
            }
        }
    }
}

/// Images of a page, by URL, and which elements use them.
#[derive(Default)]
pub(crate) struct Images {
    pub(crate) by_url: HashMap<Url, ImageState>,
    /// The image URL of each `<img>` element.
    pub(crate) by_node: HashMap<NodeId, Url>,
}

impl Images {
    fn get(&self, image: &ImageRef) -> Option<&Arc<DecodedImage>> {
        let url = match image {
            ImageRef::Node(node) => self.by_node.get(node)?.clone(),
            ImageRef::Url(url) => Url::parse(url).ok()?,
        };
        match self.by_url.get(&url)? {
            ImageState::Loaded(image) => Some(image),
            _ => None,
        }
    }

    /// The natural size of an element's image, if loaded.
    pub(crate) fn node_size(&self, node: NodeId) -> Option<(f32, f32)> {
        self.get(&ImageRef::Node(node)).map(|i| i.natural_size())
    }
}

impl ImageSizes for Images {
    fn size(&self, image: &ImageRef) -> Option<(f32, f32)> {
        self.get(image).map(|i| i.natural_size())
    }
}

impl ImageSource for Images {
    fn pixmap(&self, image: &ImageRef) -> Option<&Pixmap> {
        self.get(image).map(|i| &i.pixmap)
    }
}

impl swb_layout::ReplacedSizes for Images {
    fn natural_size(&self, node: NodeId) -> Option<(f32, f32)> {
        self.node_size(node)
    }
}

/// In-flight requests of a page.
#[derive(Default)]
pub(crate) struct Requests {
    pub(crate) pending: HashMap<RequestId, Pending>,
}

impl Requests {
    pub(crate) fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}
