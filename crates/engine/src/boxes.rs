//! Element boxes: the border box of every element, in tree order. This is
//! the data that comparisons with Chromium use (format: docs/testing.md).

use swb_dom::NodeId;
use swb_layout::Rect;

use crate::Page;

/// The box of one element.
#[derive(Clone, Debug, PartialEq)]
pub struct ElementBox {
    /// The element's local name, lowercase.
    pub tag: String,
    /// The union of the element's border boxes in document coordinates, or
    /// `None` if it generates no box.
    pub rect: Option<Rect>,
    /// Index of the parent element in the list, if any.
    pub parent: Option<usize>,
}

/// All elements of the page's document in tree order, with their boxes.
/// Empty if there is no document or layout.
pub fn element_boxes(page: &Page) -> Vec<ElementBox> {
    let (Some(doc), Some(tree)) = (page.document(), page.fragments()) else {
        return Vec::new();
    };
    let boxes = tree.element_boxes();
    let mut out = Vec::new();
    let mut index_of = std::collections::HashMap::new();
    for node in doc.descendants(NodeId::DOCUMENT) {
        let Some(element) = doc.element(node) else {
            continue;
        };
        let parent = doc
            .parent_element(node)
            .and_then(|p| index_of.get(&p).copied());
        index_of.insert(node, out.len());
        out.push(ElementBox {
            tag: element.local_name().to_string().to_ascii_lowercase(),
            rect: boxes.get(&node).copied(),
            parent,
        });
    }
    out
}
