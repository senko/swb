//! Image sources with `sizes="auto"`: the choice of the source of a
//! lazy-loaded `img` that uses the width of its own box
//! (`crate::image_source`). There is no box when the sources of a document
//! are selected, so the page selects them after each layout, and again when
//! the width of a box (or the viewport or the scale) changes.
//!
//! <https://html.spec.whatwg.org/multipage/images.html#parse-a-sizes-attribute>
//!
//! This does not loop: an image whose `sizes` is `auto` or starts with
//! `auto,` has size containment (user-agent style sheet), so its box does
//! not depend on its source; and each image selects at most
//! [`MAX_AUTO_SELECTIONS`](crate::resources::MAX_AUTO_SELECTIONS) times
//! (an image with `sizes=" auto"` has no containment).

use std::collections::{HashMap, HashSet};

use swb_dom::NodeId;
use swb_layout::{FragmentRef, FragmentTree};
use swb_net::Url;

use super::Page;
use crate::image_source;

impl Page {
    /// Selects the sources of the images that wait for the width of their
    /// box, after a layout, and starts loading the new ones. An image that
    /// has no box uses the width of its last selection, else `100vw`.
    pub(super) fn select_auto_sized_images(&mut self) {
        if self.images.auto_nodes.is_empty() {
            return;
        }
        let (Some(doc), Some(base), Some(tree)) =
            (&self.document, self.base_url.clone(), &self.fragments)
        else {
            return;
        };
        let env = self.media_environment();
        let env_key = [
            env.viewport_width.to_bits(),
            env.viewport_height.to_bits(),
            env.device_pixel_ratio.to_bits(),
        ];
        let nodes = self.images.auto_nodes.clone();
        let widths = content_widths(tree, &nodes);
        let mut loads: Vec<Url> = Vec::new();
        for node in nodes {
            let state = self.images.auto.entry(node).or_default();
            if !state.update(widths.get(&node).copied(), env_key) {
                continue;
            }
            let width = state.width();
            let Some(image) = image_source::select_auto_image(doc, &base, node, &env, width) else {
                continue;
            };
            let url = image.url.clone();
            if self.images.reselect(node, image) {
                loads.push(url);
            }
        }
        for url in loads {
            self.start_image(url);
        }
    }
}

/// The content-box width of the box of each of `nodes` that has one, in
/// CSS px: the "concrete object size" width of an image.
fn content_widths(tree: &FragmentTree, nodes: &[NodeId]) -> HashMap<NodeId, f32> {
    let wanted: HashSet<NodeId> = nodes.iter().copied().collect();
    let mut widths = HashMap::new();
    tree.walk(|fragment, _| {
        if let FragmentRef::Box(b) = fragment
            && b.pseudo.is_none()
            && let Some(node) = b.node
            && wanted.contains(&node)
        {
            let width = b.border_rect.width
                - (b.border.left + b.border.right + b.padding.left + b.padding.right);
            widths.entry(node).or_insert_with(|| width.max(0.0));
        }
    });
    widths
}
