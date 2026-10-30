//! The `load` and `error` events of images and style sheets, for scripts
//! (M8 feature 10b, ADR 0028 section 12; HTML "update the image data",
//! <https://html.spec.whatwg.org/multipage/images.html#update-the-image-data>,
//! and the `link` element's "fetch and process the linked resource",
//! <https://html.spec.whatwg.org/multipage/semantics.html#link-type-stylesheet>).
//!
//! A document that was parsed with scripting watches its `img` elements:
//! when the journal drain finds an `img` that was inserted or whose `src`,
//! `srcset` or `sizes` changed (also one that is not in the document, such
//! as `new Image()`), the document selects its source (`image_source`),
//! starts the load if the URL is new, and remembers the request of the
//! element (a [`Watch`] with a token). The completion of the load (or the
//! known result of a URL that was loaded or failed before) queues an event
//! `(element, ok)` if the element still waits for that request; the page
//! hands the queue to the script runtime
//! ([`swb_web::ScriptRuntime::element_events`]), which fires `load` or
//! `error` in a task. Style sheet loads of `link` elements queue their
//! events the same way.
//!
//! The same table answers `complete`, `naturalWidth`, `naturalHeight` and
//! `currentSrc` ([`DocumentState::image_info`]).
//!
//! Bounds: at most [`MAX_WATCHES`] elements are watched. When the table is
//! full, the watches that ended for elements outside the document go first
//! (a page that makes images for the whole of its life, such as a slideshow
//! or a map, keeps its events); if none can go, further elements get no
//! events, with one warning. The queue is delivered in slices of
//! [`MAX_DELIVERY`] events per call, so a `load` handler that inserts the
//! next image cannot starve the loop.
//!
//! Deviations: `currentSrc` is set as soon as the source is selected (in
//! Chromium after a microtask); a change of the viewport that selects
//! another source does not fire `load` again.

use std::collections::{HashMap, VecDeque};

use swb_dom::NodeId;
use swb_net::Url;
use swb_paint::ImageRef;
use swb_paint::ImageSizes;
use swb_web::ImageInfo;

use super::DocumentState;
use crate::image_source;
use crate::resources::ImageState;

/// The most `img` elements that are watched.
pub(crate) const MAX_WATCHES: usize = 65_536;

/// The number of attempts to watch an element that skip the search for
/// watches to drop after a search that found none (so that a full table of
/// watches in the document does not make every attempt scan it).
const SCAN_COOLDOWN: u32 = 256;

/// The most events that one delivery hands over.
pub(crate) const MAX_DELIVERY: usize = 4096;

/// The result of the request of an element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WatchState {
    Pending,
    Done(bool),
}

/// The request of one `img` element.
#[derive(Clone, Debug)]
struct Watch {
    url: Option<Url>,
    /// The pixel density of the selected candidate.
    density: f32,
    token: u64,
    state: WatchState,
}

/// The events of images and style sheets that wait for the script runtime.
#[derive(Debug)]
pub(crate) struct ElementEvents {
    watches: HashMap<NodeId, Watch>,
    /// The elements that wait for a URL, with the token of their request.
    waiting: HashMap<Url, Vec<(NodeId, u64)>>,
    next_token: u64,
    queue: VecDeque<(NodeId, bool)>,
    warned: bool,
    /// Attempts left that skip the search for watches to drop.
    scan_cooldown: u32,
    /// The most watches ([`MAX_WATCHES`]; tests use a smaller one).
    limit: usize,
}

impl Default for ElementEvents {
    fn default() -> Self {
        ElementEvents {
            watches: HashMap::new(),
            waiting: HashMap::new(),
            next_token: 0,
            queue: VecDeque::new(),
            warned: false,
            scan_cooldown: 0,
            limit: MAX_WATCHES,
        }
    }
}

impl ElementEvents {
    /// True if events wait for delivery.
    pub(crate) fn has_queued(&self) -> bool {
        !self.queue.is_empty()
    }
}

impl DocumentState {
    /// Starts the request of the `img` element `node` for its events (see
    /// the module comment). Does nothing in a document without scripting.
    pub(crate) fn watch_image(&mut self, node: NodeId) {
        // The documents that scripts parse (`DOMParser`, `createHTMLDocument`)
        // load nothing.
        if !self.dom.scripting() || self.dom.node_document(node) != NodeId::DOCUMENT {
            return;
        }
        let Some(element) = self.dom.element(node) else {
            return;
        };
        let has_src = element.attr("src").is_some();
        let has_srcset = element.attr("srcset").is_some_and(|v| !v.trim().is_empty());
        if !has_src && !has_srcset {
            self.element_events.watches.remove(&node);
            return;
        }
        if !self.element_events.watches.contains_key(&node) && !self.make_watch_room() {
            let events = &mut self.element_events;
            if !events.warned {
                events.warned = true;
                log::warn!(
                    "more than {MAX_WATCHES} images are watched for load events; further ones get none"
                );
            }
            return;
        }
        let events = &mut self.element_events;
        events.next_token += 1;
        let token = events.next_token;
        let env = self.media_environment();
        let selection = image_source::select_images(&self.dom, self.base_url.value(), &env, [node])
            .into_iter()
            .next()
            .map(|(_, s)| s);
        if selection.as_ref().is_some_and(|s| s.auto) {
            // The source depends on the width of the box; the selection
            // after layout loads it (no event; departure).
            self.element_events.watches.remove(&node);
            return;
        }
        let Some(image) = selection.and_then(|s| s.image) else {
            // No usable candidate: a broken image (`src=""`).
            self.element_events.watches.insert(
                node,
                Watch {
                    url: None,
                    density: 1.0,
                    token,
                    state: WatchState::Done(false),
                },
            );
            self.element_events.queue.push_back((node, false));
            return;
        };
        let url = image.url;
        // The element may wait for this URL already (`src` set again while
        // the load runs): its entry takes the new token.
        let waits_already = self
            .element_events
            .watches
            .get(&node)
            .is_some_and(|w| w.state == WatchState::Pending && w.url.as_ref() == Some(&url));
        self.element_events.watches.insert(
            node,
            Watch {
                url: Some(url.clone()),
                density: image.density,
                token,
                state: WatchState::Pending,
            },
        );
        if !self.images.by_url.contains_key(&url) {
            self.start_image(url.clone());
        }
        match self.images.by_url.get(&url) {
            Some(ImageState::Loaded(_)) => self.finish_watch(node, token, true),
            Some(ImageState::Failed) => self.finish_watch(node, token, false),
            _ => {
                let list = self.element_events.waiting.entry(url).or_default();
                let known = waits_already
                    .then(|| list.iter_mut().find(|(n, _)| *n == node))
                    .flatten();
                match known {
                    Some(entry) => entry.1 = token,
                    None => list.push((node, token)),
                }
            }
        }
    }

    /// Makes room for one more watch: drops the watches that ended for
    /// elements outside the document. False if the table is still full.
    fn make_watch_room(&mut self) -> bool {
        let events = &mut self.element_events;
        if events.watches.len() < events.limit {
            return true;
        }
        if events.scan_cooldown > 0 {
            events.scan_cooldown -= 1;
            return false;
        }
        let dom = &self.dom;
        events
            .watches
            .retain(|&node, watch| watch.state == WatchState::Pending || dom.is_connected(node));
        if events.watches.len() < events.limit {
            return true;
        }
        events.scan_cooldown = SCAN_COOLDOWN;
        false
    }

    /// Sets the most watches (tests).
    #[cfg(test)]
    pub(crate) fn set_watch_limit(&mut self, limit: usize) {
        self.element_events.limit = limit;
    }

    /// True if the `img` element `node` has a request that is watched.
    pub(crate) fn image_is_watched(&self, node: NodeId) -> bool {
        self.element_events.watches.contains_key(&node)
    }

    /// Records the result of the request `token` of `node` and queues the
    /// event.
    fn finish_watch(&mut self, node: NodeId, token: u64, ok: bool) {
        let events = &mut self.element_events;
        if let Some(watch) = events.watches.get_mut(&node)
            && watch.token == token
        {
            watch.state = WatchState::Done(ok);
            events.queue.push_back((node, ok));
        }
    }

    /// The load of the image at `url` ended: the elements that wait for it
    /// get their event.
    pub(crate) fn image_done(&mut self, url: &Url, ok: bool) {
        let Some(waiting) = self.element_events.waiting.remove(url) else {
            return;
        };
        for (node, token) in waiting {
            self.finish_watch(node, token, ok);
        }
    }

    /// Queues the event of a style sheet load of the `link` element `owner`.
    pub(crate) fn sheet_event(&mut self, owner: NodeId, ok: bool) {
        if self.dom.scripting() && self.element_events.queue.len() < MAX_WATCHES {
            self.element_events.queue.push_back((owner, ok));
        }
    }

    /// Takes the next events for the script runtime (at most
    /// [`MAX_DELIVERY`]): the elements and whether they loaded.
    pub(crate) fn take_element_events(&mut self) -> Vec<(NodeId, bool)> {
        self.process_changes();
        let queue = &mut self.element_events.queue;
        let count = queue.len().min(MAX_DELIVERY);
        queue.drain(..count).collect()
    }

    /// True if element events wait for delivery.
    pub(crate) fn has_element_events(&self) -> bool {
        self.element_events.has_queued()
    }

    /// `complete`, the natural size and `currentSrc` of the `img` element
    /// `node` (HTML "img.complete"): without `src` and `srcset`, complete;
    /// with a request in flight, not complete.
    pub(crate) fn image_info(&mut self, node: NodeId) -> ImageInfo {
        self.process_changes();
        let Some(watch) = self.element_events.watches.get(&node).cloned() else {
            let has = |name: &str| {
                self.dom
                    .element(node)
                    .and_then(|e| e.attr(name))
                    .is_some_and(|v| !v.is_empty())
            };
            // Without scripting no request is watched: the engine's own
            // table decides.
            let source = has("src") || has("srcset");
            return ImageInfo {
                complete: !source,
                ..ImageInfo::default()
            };
        };
        let current_src = watch.url.as_ref().map(Url::to_string).unwrap_or_default();
        match watch.state {
            // Chromium sets `currentSrc` when the request starts, a
            // microtask after the `src` changed; the first read in the
            // same task sees an empty string.
            WatchState::Pending => ImageInfo {
                complete: false,
                natural_width: 0,
                natural_height: 0,
                current_src: String::new(),
            },
            WatchState::Done(ok) => {
                let size = watch
                    .url
                    .as_ref()
                    .filter(|_| ok)
                    .and_then(|url| self.images.size(&ImageRef::Url(url.as_str().into())));
                let scaled = |v: Option<f32>| {
                    let v = v.unwrap_or(0.0) / watch.density.max(f32::MIN_POSITIVE);
                    if v.is_finite() {
                        v.round().max(0.0) as u32
                    } else {
                        0
                    }
                };
                ImageInfo {
                    complete: true,
                    natural_width: scaled(size.as_ref().and_then(|s| s.width)),
                    natural_height: scaled(size.as_ref().and_then(|s| s.height)),
                    current_src,
                }
            }
        }
    }
}
