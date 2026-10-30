//! `CharacterData`, `Text`, `Comment`, `DocumentType` and
//! `ProcessingInstruction` (DOM §4.10 to §4.14,
//! <https://dom.spec.whatwg.org/#interface-characterdata>).
//!
//! The DOM stores text as UTF-8; the offsets and counts of the API are
//! UTF-16 code units. An offset that falls inside a surrogate pair (a
//! character outside the BMP) cannot split it, since the DOM string cannot
//! hold a lone surrogate (ADR 0028 section 3); swb rounds it up to the end
//! of the character.

use swb_dom::{NodeData, NodeId};

use super::{dom_result, next_element_sibling, previous_element_sibling, utf16_len};
use crate::bindings::{self as b, Cx, Exception};
use crate::generated as g;
use crate::state::{WebState, dom, dom_mut};

/// The byte index of the UTF-16 `offset` in `text`, or none if it is past
/// the end.
fn byte_index(text: &str, offset: u32) -> Option<usize> {
    let mut units = 0u32;
    for (i, c) in text.char_indices() {
        if units >= offset {
            return Some(i);
        }
        units += u32::try_from(c.len_utf16()).unwrap_or(2);
    }
    (units >= offset).then_some(text.len())
}

/// The exception for an offset past the end of the data.
fn offset_error(offset: u32, length: u32) -> Exception {
    Exception::dom(
        "IndexSizeError",
        format!("The offset {offset} is greater than the node's length ({length})."),
    )
}

/// Sets the data of `node`, then runs the children changed steps of a
/// `script` parent (HTML: a connected script that has not started runs
/// when its text changes).
pub(super) fn set_data(cx: &mut Cx<'_>, node: NodeId, data: &str) -> b::Result<()> {
    let result = dom_mut(cx.rt)?.set_data(node, data);
    dom_result(cx.rt, result)?;
    Ok(crate::scripts::data_changed(cx.rt, node)?)
}

/// DOM "replace data" with UTF-16 `offset` and `count`.
/// <https://dom.spec.whatwg.org/#concept-cd-replace>
fn replace_data(
    cx: &mut Cx<'_>,
    node: NodeId,
    offset: u32,
    count: u32,
    data: &str,
) -> b::Result<()> {
    let text = dom(cx.rt)?.char_data(node).unwrap_or_default();
    let start = byte_index(text, offset).ok_or_else(|| offset_error(offset, utf16_len(text)))?;
    let end = byte_index(&text[start..], count).map_or(text.len(), |n| start + n);
    let result = dom_mut(cx.rt)?.replace_data(node, start, end - start, data);
    dom_result(cx.rt, result)?;
    Ok(crate::scripts::data_changed(cx.rt, node)?)
}

impl g::character_data::CharacterDataImpl for WebState {
    child_node_methods!("CharacterData");

    fn data(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(dom(cx.rt)?.char_data(this).unwrap_or_default().to_owned())
    }

    fn set_data(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
        set_data(cx, this, &value)
    }

    fn length(cx: &mut Cx<'_>, this: NodeId) -> b::Result<u32> {
        Ok(utf16_len(dom(cx.rt)?.char_data(this).unwrap_or_default()))
    }

    fn previous_element_sibling(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        previous_element_sibling(cx, this)
    }

    fn next_element_sibling(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        next_element_sibling(cx, this)
    }

    fn substring_data(cx: &mut Cx<'_>, this: NodeId, offset: u32, count: u32) -> b::Result<String> {
        let text = dom(cx.rt)?.char_data(this).unwrap_or_default();
        let start =
            byte_index(text, offset).ok_or_else(|| offset_error(offset, utf16_len(text)))?;
        let end = byte_index(&text[start..], count).map_or(text.len(), |n| start + n);
        Ok(text[start..end].to_owned())
    }

    fn append_data(cx: &mut Cx<'_>, this: NodeId, data: String) -> b::Result<()> {
        let result = dom_mut(cx.rt)?.append_data(this, &data);
        dom_result(cx.rt, result)?;
        Ok(crate::scripts::data_changed(cx.rt, this)?)
    }

    fn insert_data(cx: &mut Cx<'_>, this: NodeId, offset: u32, data: String) -> b::Result<()> {
        replace_data(cx, this, offset, 0, &data)
    }

    fn delete_data(cx: &mut Cx<'_>, this: NodeId, offset: u32, count: u32) -> b::Result<()> {
        replace_data(cx, this, offset, count, "")
    }

    fn replace_data(
        cx: &mut Cx<'_>,
        this: NodeId,
        offset: u32,
        count: u32,
        data: String,
    ) -> b::Result<()> {
        replace_data(cx, this, offset, count, &data)
    }
}

impl g::text::TextImpl for WebState {
    /// The data of the contiguous text nodes around this one, in order.
    /// <https://dom.spec.whatwg.org/#dom-text-wholetext>
    fn whole_text(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        let dom = dom(cx.rt)?;
        let is_text = |n: &NodeId| matches!(dom.node(*n).data, NodeData::Text(_));
        let mut first = this;
        while let Some(p) = dom.node(first).prev_sibling().filter(is_text) {
            first = p;
        }
        let mut out = String::new();
        let mut current = Some(first);
        while let Some(n) = current.filter(is_text) {
            out.push_str(dom.char_data(n).unwrap_or_default());
            current = dom.node(n).next_sibling();
        }
        Ok(out)
    }

    /// `new Text(data)`.
    fn constructor(cx: &mut Cx<'_>, data: String) -> b::Result<swb_js::Gc<swb_js::Object>> {
        let result = dom_mut(cx.rt)?.create_node(NodeData::Text(data));
        let node = dom_result(cx.rt, result)?;
        let realm = cx.realm();
        Ok(<WebState as b::Host>::wrap_node(cx.rt, realm, node)?)
    }

    /// DOM "split a Text node": the data from `offset` goes into a new
    /// node that follows this one.
    /// <https://dom.spec.whatwg.org/#concept-text-split>
    fn split_text(cx: &mut Cx<'_>, this: NodeId, offset: u32) -> b::Result<NodeId> {
        let text = dom(cx.rt)?.char_data(this).unwrap_or_default();
        let start = byte_index(text, offset).ok_or_else(|| {
            Exception::dom(
                "IndexSizeError",
                format!("The offset {offset} is larger than the Text node's length."),
            )
        })?;
        let tail = text[start..].to_owned();
        let document = dom(cx.rt)?.node_document(this);
        let result = dom_mut(cx.rt)?.create_node_in(document, NodeData::Text(tail));
        let new_node = dom_result(cx.rt, result)?;
        // The standard inserts the new node first and truncates this one
        // after; Chromium truncates first (its "characterData" record comes
        // before the "childList" one; measured, feature 9a).
        let length = dom(cx.rt)?.char_data(this).map_or(0, str::len);
        let result = dom_mut(cx.rt)?.replace_data(this, start, length - start, "");
        dom_result(cx.rt, result)?;
        if let Some(parent) = dom(cx.rt)?.parent(this) {
            let next = dom(cx.rt)?.node(this).next_sibling();
            let result = dom_mut(cx.rt)?.insert_before(parent, new_node, next);
            dom_result(cx.rt, result)?;
        }
        Ok(new_node)
    }
}

impl g::comment::CommentImpl for WebState {
    /// `new Comment(data)`.
    fn constructor(cx: &mut Cx<'_>, data: String) -> b::Result<swb_js::Gc<swb_js::Object>> {
        let result = dom_mut(cx.rt)?.create_node(NodeData::Comment(data));
        let node = dom_result(cx.rt, result)?;
        let realm = cx.realm();
        Ok(<WebState as b::Host>::wrap_node(cx.rt, realm, node)?)
    }
}

impl g::document_type::DocumentTypeImpl for WebState {
    child_node_methods!("DocumentType");

    fn name(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(match &dom(cx.rt)?.node(this).data {
            NodeData::Doctype { name, .. } => name.clone(),
            _ => String::new(),
        })
    }

    fn public_id(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(match &dom(cx.rt)?.node(this).data {
            NodeData::Doctype { public_id, .. } => public_id.clone(),
            _ => String::new(),
        })
    }

    fn system_id(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(match &dom(cx.rt)?.node(this).data {
            NodeData::Doctype { system_id, .. } => system_id.clone(),
            _ => String::new(),
        })
    }
}

impl g::processing_instruction::ProcessingInstructionImpl for WebState {
    fn target(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(match &dom(cx.rt)?.node(this).data {
            NodeData::ProcessingInstruction { target, .. } => target.clone(),
            _ => String::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::byte_index;

    #[test]
    fn utf16_offsets_map_to_bytes() {
        let text = "a\u{1F600}b\u{e9}";
        assert_eq!(byte_index(text, 0), Some(0));
        assert_eq!(byte_index(text, 1), Some(1));
        // Inside the pair: rounded up to the end of the character.
        assert_eq!(byte_index(text, 2), Some(5));
        assert_eq!(byte_index(text, 3), Some(5));
        assert_eq!(byte_index(text, 4), Some(6));
        assert_eq!(byte_index(text, 5), Some(8));
        assert_eq!(byte_index(text, 6), None);
    }
}
