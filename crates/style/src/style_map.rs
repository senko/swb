//! The result of style computation for a document: one computed style per
//! element, plus styles for generated pseudo-elements.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use swb_dom::NodeId;

use crate::ComputedStyle;
use crate::values::Image;

/// A pseudo-element that generates a box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PseudoKind {
    /// `::before`
    Before,
    /// `::after`
    After,
    /// `::marker`
    Marker,
}

/// Computed styles for the elements of a document.
#[derive(Clone, Debug, Default)]
pub struct StyleMap {
    /// Indexed by `NodeId::index()`. `None` for non-elements and for
    /// elements in `display: none` subtrees (which have no style).
    elements: Vec<Option<Arc<ComputedStyle>>>,
    pseudos: HashMap<(NodeId, PseudoKind), Arc<ComputedStyle>>,
}

impl StyleMap {
    /// Creates an empty map with room for `node_count` nodes.
    pub fn with_capacity(node_count: usize) -> Self {
        StyleMap {
            elements: vec![None; node_count],
            pseudos: HashMap::new(),
        }
    }

    /// The style of an element, if it has one.
    pub fn get(&self, id: NodeId) -> Option<&Arc<ComputedStyle>> {
        self.elements.get(id.index()).and_then(Option::as_ref)
    }

    /// The style of a pseudo-element of `id`, if it generates a box.
    pub fn pseudo(&self, id: NodeId, kind: PseudoKind) -> Option<&Arc<ComputedStyle>> {
        self.pseudos.get(&(id, kind))
    }

    /// Stores the style of an element.
    pub fn set(&mut self, id: NodeId, style: Arc<ComputedStyle>) {
        let index = id.index();
        if index >= self.elements.len() {
            self.elements.resize(index + 1, None);
        }
        self.elements[index] = Some(style);
    }

    /// Stores the style of a pseudo-element.
    pub fn set_pseudo(&mut self, id: NodeId, kind: PseudoKind, style: Arc<ComputedStyle>) {
        self.pseudos.insert((id, kind), style);
    }

    /// The absolute URLs of all `background-image` and `list-style-image`
    /// `url()` values, without duplicates, in document order (element
    /// styles first, then pseudo-element styles).
    pub fn image_urls(&self) -> Vec<Arc<str>> {
        let mut seen: HashSet<Arc<str>> = HashSet::new();
        let mut urls = Vec::new();
        let mut visited: HashSet<*const ComputedStyle> = HashSet::new();
        let styles = self.elements.iter().flatten().chain(self.pseudos.values());
        for style in styles {
            if !visited.insert(Arc::as_ptr(style)) {
                continue;
            }
            let images = style
                .background_image
                .iter()
                .flatten()
                .chain(style.list_style_image.as_ref());
            for image in images {
                if let Image::Url(url) = image
                    && seen.insert(Arc::clone(url))
                {
                    urls.push(Arc::clone(url));
                }
            }
        }
        urls
    }
}
