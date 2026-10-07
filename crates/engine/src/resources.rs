//! Subresources of a page: stylesheets and images (also video posters),
//! and the requests that load them. The natural size of an `<img>` is
//! corrected by the pixel density of its chosen source
//! (`crate::image_source`). After a viewport or scale change, an image
//! keeps its source until the newly selected one has loaded
//! ([`Images::reselect`]).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use swb_dom::NodeId;
use swb_layout::NaturalSize;
use swb_net::{NetError, RequestId, Response, Url};
use swb_paint::{DecodedImage, ImageRef, ImageSizes, ImageSource, VectorCache};
use swb_style::Length;

use crate::image_source::SelectedImage;

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
    /// True if the load failed, was not allowed, or was stopped.
    pub(crate) failed: bool,
}

impl SheetSlot {
    /// True if the sheet arrived or failed: rendering no longer waits for
    /// it.
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
            Ok(response) if response.is_success() => Self::decode(url, &response),
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

    /// Decodes the image in `response`, which was loaded for `url`. The
    /// content type selects SVG; raster formats are recognized from the
    /// data.
    pub(crate) fn decode(url: &Url, response: &Response) -> Self {
        let content_type = response.content_type();
        let essence = content_type.as_ref().map(|c| c.essence.as_str());
        match swb_paint::decode_with_type(&response.body, essence) {
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
    /// The state of each image URL that the page uses.
    pub(crate) by_url: HashMap<Url, ImageState>,
    /// The image of each `<img>` element (its URL and pixel density) and
    /// the poster of each `<video>` element (density 1).
    pub(crate) by_node: HashMap<NodeId, SelectedImage>,
    /// Images selected again after a viewport or scale change that are
    /// still loading, by element. Until they arrive, the elements keep
    /// their images in `by_node`.
    pending: HashMap<NodeId, SelectedImage>,
    /// The elements that wait in `pending`, by URL. An entry can be stale
    /// (the element waits for another URL now); `load_ended` skips those.
    /// A set, so that an element is in it at most once per URL.
    waiting: HashMap<Url, HashSet<NodeId>>,
    /// True if a viewport or scale change happened after the last
    /// selection of image sources.
    pub(crate) sources_outdated: bool,
    /// Renderings of the page's SVG images.
    vector_cache: VectorCache,
}

impl Images {
    /// Uses `image` for `node` after a selection: at once if it is loaded
    /// or the element has no image yet, else when its load succeeds
    /// ([`Images::load_ended`]). If the load failed before, the element
    /// keeps its image. Returns true if the URL is not known (new, or
    /// forgotten by [`Images::forget_cancelled`]), so that the caller
    /// starts loading it.
    ///
    /// <https://html.spec.whatwg.org/multipage/images.html#img-environment-changes>.
    /// Deviation: the specification ignores a change while a load is
    /// pending; here a newer selection replaces the pending one, so that a
    /// change and its reversal (1x, 2x, 1x) end at the right image.
    pub(crate) fn reselect(&mut self, node: NodeId, image: SelectedImage) -> bool {
        if self.pending.get(&node) == Some(&image) {
            return false;
        }
        self.pending.remove(&node);
        if self.by_node.get(&node) == Some(&image) {
            return !self.by_url.contains_key(&image.url);
        }
        let state = self.by_url.get(&image.url);
        let unknown = state.is_none();
        let has_image = self.by_node.contains_key(&node);
        match state {
            None | Some(ImageState::Loading) if has_image => {
                self.waiting
                    .entry(image.url.clone())
                    .or_default()
                    .insert(node);
                self.pending.insert(node, image);
            }
            Some(ImageState::Failed) if has_image => {}
            _ => {
                self.by_node.insert(node, image);
            }
        }
        unknown
    }

    /// Forgets the image URLs that are `Loading` but not in `in_flight`
    /// (their requests were cancelled by `Page::stop` or a navigation),
    /// and the selections that wait for them, so that a new selection or
    /// style computation requests them again.
    pub(crate) fn forget_cancelled(&mut self, in_flight: &HashSet<&Url>) {
        self.by_url
            .retain(|url, state| !matches!(state, ImageState::Loading) || in_flight.contains(url));
        let by_url = &self.by_url;
        self.waiting.retain(|url, _| by_url.contains_key(url));
        self.pending
            .retain(|_, image| by_url.contains_key(&image.url));
    }

    /// Gives the elements that wait for `url` their new image, after its
    /// load ended. If the load failed, they keep their current image.
    pub(crate) fn load_ended(&mut self, url: &Url) {
        let Some(nodes) = self.waiting.remove(url) else {
            return;
        };
        let loaded = matches!(self.by_url.get(url), Some(ImageState::Loaded(_)));
        for node in nodes {
            if self.pending.get(&node).is_some_and(|p| &p.url == url)
                && let Some(image) = self.pending.remove(&node)
                && loaded
            {
                self.by_node.insert(node, image);
            }
        }
    }

    fn get(&self, image: &ImageRef) -> Option<&Arc<DecodedImage>> {
        let url = match image {
            ImageRef::Node(node) => self.by_node.get(node)?.url.clone(),
            ImageRef::Url(url) => Url::parse(url).ok()?,
        };
        match self.by_url.get(&url)? {
            ImageState::Loaded(image) => Some(image),
            _ => None,
        }
    }
}

impl ImageSizes for Images {
    /// The natural size of an image. For the image of an element, it is
    /// the density-corrected natural size
    /// (<https://html.spec.whatwg.org/multipage/images.html#density-corrected-natural-width-and-height>),
    /// which layout and `object-fit` use.
    fn size(&self, image: &ImageRef) -> Option<NaturalSize> {
        let size = self.get(image)?.natural_size();
        match image {
            ImageRef::Node(node) => Some(density_corrected(size, self.by_node.get(node)?.density)),
            ImageRef::Url(_) => Some(size),
        }
    }
}

impl ImageSource for Images {
    fn image(&self, image: &ImageRef) -> Option<&DecodedImage> {
        self.get(image).map(Arc::as_ref)
    }

    fn vector_cache(&self) -> Option<&VectorCache> {
        Some(&self.vector_cache)
    }
}

impl swb_layout::ReplacedSizes for Images {
    /// The density-corrected natural size of the image of `node`.
    fn natural_size(&self, node: NodeId) -> Option<NaturalSize> {
        self.size(&ImageRef::Node(node))
    }
}

/// `size` divided by the pixel density of its image candidate. The aspect
/// ratio does not change. A density of 0 gives the largest length (as in
/// Chromium), an infinite density 0. The result is never negative.
fn density_corrected(size: NaturalSize, density: f32) -> NaturalSize {
    if density == 1.0 {
        return size;
    }
    // `Length::clamp_px` turns the NaN of 0 / 0 into 0.
    let correct = |v: f32| Length::clamp_px(v / density).max(0.0);
    NaturalSize {
        width: size.width.map(correct),
        height: size.height.map(correct),
        ratio: size.ratio,
    }
}

/// In-flight requests of a page.
#[derive(Default)]
pub(crate) struct Requests {
    /// What each running request is for.
    pub(crate) pending: HashMap<RequestId, Pending>,
}

impl Requests {
    /// True if no request is running.
    pub(crate) fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn density_correction() {
        let size = NaturalSize::fixed(200.0, 100.0);
        assert_eq!(density_corrected(size, 1.0), size);
        assert_eq!(
            density_corrected(size, 2.0),
            NaturalSize {
                width: Some(100.0),
                height: Some(50.0),
                ratio: Some(2.0)
            }
        );
        assert_eq!(density_corrected(size, 0.5).width, Some(400.0));
        assert_eq!(density_corrected(size, 0.0).width, Some(Length::MAX_PX));
        assert_eq!(density_corrected(size, f32::INFINITY).width, Some(0.0));
        let empty = NaturalSize::fixed(0.0, 0.0);
        assert_eq!(density_corrected(empty, 0.0).width, Some(0.0));
        let ratio_only = NaturalSize {
            width: None,
            height: None,
            ratio: Some(2.0),
        };
        assert_eq!(density_corrected(ratio_only, 2.0), ratio_only);
        assert_eq!(density_corrected(size, -0.0).width, Some(0.0));
    }

    fn image(name: &str, density: f32) -> SelectedImage {
        SelectedImage {
            url: Url::parse(&format!("https://example.com/{name}")).unwrap(),
            density,
        }
    }

    fn loaded() -> ImageState {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2"/>"#;
        let decoded = swb_paint::decode_with_type(svg, Some(swb_paint::SVG_MIME_TYPE)).unwrap();
        ImageState::Loaded(Arc::new(decoded))
    }

    fn current(images: &Images, node: NodeId) -> Option<&str> {
        images.by_node.get(&node).map(|i| i.url.path())
    }

    #[test]
    fn reselection_keeps_the_current_image_until_the_new_one_loads() {
        let node = NodeId::from_index(5).unwrap();
        let mut images = Images::default();
        let (a, b, c) = (image("a", 1.0), image("b", 2.0), image("c", 3.0));
        images.by_url.insert(a.url.clone(), loaded());
        images.by_node.insert(node, a.clone());
        // The same image: nothing to do.
        assert!(!images.reselect(node, a.clone()));
        // An unknown URL: load it; `a` stays until it arrives.
        assert!(images.reselect(node, b.clone()));
        assert!(!images.reselect(node, b.clone()));
        assert_eq!(current(&images, node), Some("/a"));
        images.by_url.insert(b.url.clone(), ImageState::Loading);
        images.load_ended(&a.url);
        assert_eq!(current(&images, node), Some("/a"));
        images.by_url.insert(b.url.clone(), loaded());
        images.load_ended(&b.url);
        assert_eq!(current(&images, node), Some("/b"));
        assert_eq!(images.by_node[&node].density, 2.0);
        // A failed load keeps the current image.
        assert!(images.reselect(node, c.clone()));
        images.by_url.insert(c.url.clone(), ImageState::Failed);
        images.load_ended(&c.url);
        assert_eq!(current(&images, node), Some("/b"));
        assert!(!images.reselect(node, c.clone()));
        assert_eq!(current(&images, node), Some("/b"));
        // A loaded image applies at once.
        assert!(!images.reselect(node, a.clone()));
        assert_eq!(current(&images, node), Some("/a"));
        assert!(images.pending.is_empty());
    }

    #[test]
    fn flipping_selections_do_not_grow_the_waiting_lists() {
        let node = NodeId::from_index(5).unwrap();
        let mut images = Images::default();
        let (a, b) = (image("a", 1.0), image("b", 2.0));
        images.by_url.insert(a.url.clone(), loaded());
        images.by_node.insert(node, a.clone());
        images.by_url.insert(b.url.clone(), ImageState::Loading);
        for _ in 0..100 {
            images.reselect(node, b.clone());
            images.reselect(node, a.clone());
        }
        assert_eq!(images.waiting.values().map(HashSet::len).sum::<usize>(), 1);
    }

    #[test]
    fn cancelled_loads_are_requested_again() {
        let node = NodeId::from_index(5).unwrap();
        let other = NodeId::from_index(6).unwrap();
        let mut images = Images::default();
        let (a, b) = (image("a", 1.0), image("b", 2.0));
        images.by_url.insert(a.url.clone(), loaded());
        images.by_node.insert(node, a.clone());
        // `other` shows `b`, which is loading; `node` waits for it.
        images.by_url.insert(b.url.clone(), ImageState::Loading);
        images.by_node.insert(other, b.clone());
        assert!(!images.reselect(node, b.clone()));
        // The request for `b` was cancelled.
        images.forget_cancelled(&HashSet::new());
        assert!(images.pending.is_empty() && images.waiting.is_empty());
        assert!(images.by_url.contains_key(&a.url));
        assert!(images.reselect(node, b.clone()));
        assert!(images.reselect(other, b.clone()));
        // A request that is in flight stays.
        images.by_url.insert(b.url.clone(), ImageState::Loading);
        images.forget_cancelled(&[&b.url].into_iter().collect());
        assert!(images.by_url.contains_key(&b.url));
        assert_eq!(current(&images, node), Some("/a"));
    }

    #[test]
    fn a_newer_selection_replaces_a_pending_one() {
        let node = NodeId::from_index(5).unwrap();
        let mut images = Images::default();
        let (a, b, c) = (image("a", 1.0), image("b", 2.0), image("c", 3.0));
        images.by_url.insert(a.url.clone(), loaded());
        images.by_node.insert(node, a.clone());
        assert!(images.reselect(node, b.clone()));
        images.by_url.insert(b.url.clone(), ImageState::Loading);
        assert!(images.reselect(node, c.clone()));
        images.by_url.insert(c.url.clone(), ImageState::Loading);
        // `b` arrives, but the element waits for `c` now.
        images.by_url.insert(b.url.clone(), loaded());
        images.load_ended(&b.url);
        assert_eq!(current(&images, node), Some("/a"));
        images.by_url.insert(c.url.clone(), loaded());
        images.load_ended(&c.url);
        assert_eq!(current(&images, node), Some("/c"));
        // An element without an image gets the new one at once.
        let other = NodeId::from_index(6).unwrap();
        assert!(images.reselect(other, image("d", 1.0)));
        assert_eq!(current(&images, other), Some("/d"));
    }
}
