//! The result of style computation for a document: one computed style per
//! element, plus styles for generated pseudo-elements.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use swb_dom::NodeId;

use crate::ComputedStyle;
use crate::content::content_text;
use crate::counter_style::marker_text;
use crate::values::{Content, Image, ListStyleType, MaskImage};

/// A pseudo-element that generates a box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PseudoKind {
    /// `::before`
    Before,
    /// `::after`
    After,
    /// `::marker`
    Marker,
    /// `::placeholder` of a text field (`input` or `textarea` with a
    /// `placeholder` attribute). Layout uses its style for the placeholder
    /// text.
    Placeholder,
}

/// Computed styles for the elements of a document.
#[derive(Clone, Debug, Default)]
pub struct StyleMap {
    /// Indexed by `NodeId::index()`. `None` for non-elements and for
    /// elements in `display: none` subtrees (which have no style).
    elements: Vec<Option<Arc<ComputedStyle>>>,
    pseudos: HashMap<(NodeId, PseudoKind), Arc<ComputedStyle>>,
    /// The ordinal value of each list item (see `counters.rs`).
    ordinals: HashMap<NodeId, i32>,
}

impl StyleMap {
    /// Creates an empty map with room for `node_count` nodes.
    pub(crate) fn with_capacity(node_count: usize) -> Self {
        StyleMap {
            elements: vec![None; node_count],
            pseudos: HashMap::new(),
            ordinals: HashMap::new(),
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

    /// The style of a pseudo-element of `id`, for changes.
    pub(crate) fn pseudo_mut(
        &mut self,
        id: NodeId,
        kind: PseudoKind,
    ) -> Option<&mut Arc<ComputedStyle>> {
        self.pseudos.get_mut(&(id, kind))
    }

    /// The ordinal value of list item `id` (an element with
    /// `display: list-item`): the number that its marker shows with
    /// `content: normal`.
    /// <https://html.spec.whatwg.org/multipage/grouping-content.html#ordinal-value>
    pub fn list_item_ordinal(&self, id: NodeId) -> Option<i32> {
        self.ordinals.get(&id).copied()
    }

    /// The text of the marker of list item `id`, whose style is `style`:
    /// the `content` of its `::marker` (with counters resolved), or for
    /// `content: normal` its ordinal value in its `list-style-type`. `None`
    /// if `id` is not a list item (also a replaced element or form control
    /// with `display: list-item`) or if the marker has no box:
    /// `content: none`, or `content: normal` with `list-style-type: none`
    /// and no `list-style-image`.
    pub fn list_marker_text(&self, id: NodeId, style: &ComputedStyle) -> Option<String> {
        let ordinal = self.list_item_ordinal(id)?;
        let marker = self.pseudo(id, PseudoKind::Marker)?;
        match &marker.content {
            Content::None => None,
            Content::Items(_) => content_text(marker),
            Content::Normal => {
                if style.list_style_type == ListStyleType::None && style.list_style_image.is_none()
                {
                    return None;
                }
                Some(marker_text(style.list_style_type, ordinal))
            }
        }
    }

    /// Stores the ordinal value of a list item.
    pub(crate) fn set_list_item_ordinal(&mut self, id: NodeId, value: i32) {
        self.ordinals.insert(id, value);
    }

    /// Stores the style of an element.
    pub(crate) fn set(&mut self, id: NodeId, style: Arc<ComputedStyle>) {
        let index = id.index();
        if index >= self.elements.len() {
            self.elements.resize(index + 1, None);
        }
        self.elements[index] = Some(style);
    }

    /// True if both maps have equal styles for all elements and
    /// pseudo-elements, and equal list item ordinals.
    pub fn same_styles(&self, other: &StyleMap) -> bool {
        let same = |a: &Arc<ComputedStyle>, b: &Arc<ComputedStyle>| Arc::ptr_eq(a, b) || a == b;
        self.elements.len() == other.elements.len()
            && self
                .elements
                .iter()
                .zip(&other.elements)
                .all(|pair| match pair {
                    (Some(a), Some(b)) => same(a, b),
                    (None, None) => true,
                    _ => false,
                })
            && self.pseudos.len() == other.pseudos.len()
            && self
                .pseudos
                .iter()
                .all(|(key, a)| other.pseudos.get(key).is_some_and(|b| same(a, b)))
            && self.ordinals == other.ordinals
    }

    /// Stores the style of a pseudo-element.
    pub(crate) fn set_pseudo(&mut self, id: NodeId, kind: PseudoKind, style: Arc<ComputedStyle>) {
        self.pseudos.insert((id, kind), style);
    }

    /// The absolute URLs of all `background-image`, `mask-image` and
    /// `list-style-image` `url()` values, without duplicates: those of
    /// element styles in document order, then those of pseudo-element
    /// styles (in no particular order).
    pub fn image_urls(&self) -> Vec<Arc<str>> {
        let mut seen: HashSet<Arc<str>> = HashSet::new();
        let mut urls = Vec::new();
        let mut visited: HashSet<*const ComputedStyle> = HashSet::new();
        let styles = self.elements.iter().flatten().chain(self.pseudos.values());
        for style in styles {
            if !visited.insert(Arc::as_ptr(style)) {
                continue;
            }
            let masks = style.mask_image.iter().filter_map(|m| match m {
                MaskImage::Image(image) => Some(image),
                _ => None,
            });
            let images = style
                .background_image
                .iter()
                .flatten()
                .chain(masks)
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
