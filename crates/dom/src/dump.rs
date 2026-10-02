//! Text dump of a DOM tree, in the format of the html5lib tree-construction
//! tests. Used by tests and by the `swb --dump-dom` debugging option.

use std::fmt::Write;

use html5ever::ns;

use crate::tree::{Document, NodeData, NodeId};

/// Dumps the children of the document node.
pub fn dump_tree(doc: &Document) -> String {
    let mut out = String::new();
    let roots: Vec<NodeId> = doc.children(NodeId::DOCUMENT).collect();
    dump_nodes(doc, &roots, &mut out);
    out
}

/// Dumps `roots` and their subtrees. Iterative, because trees (with
/// template contents) can be deeper than the parser's depth limit. Indents
/// are capped so the output stays linear in the number of nodes for
/// pathological depths.
fn dump_nodes(doc: &Document, roots: &[NodeId], out: &mut String) {
    const MAX_INDENT: usize = 200;
    let mut stack: Vec<(NodeId, usize)> = roots.iter().rev().map(|&id| (id, 0)).collect();
    while let Some((id, depth)) = stack.pop() {
        let indent = "  ".repeat(depth.min(MAX_INDENT));
        write_node(doc, id, &indent, out);
        // Children are pushed in reverse so they are dumped in order;
        // template contents come before the template's own children.
        let children: Vec<NodeId> = doc.children(id).collect();
        for &child in children.iter().rev() {
            stack.push((child, depth + 1));
        }
        if let Some(contents) = doc.element(id).and_then(|e| e.template_contents) {
            let contents: Vec<NodeId> = doc.children(contents).collect();
            for &child in contents.iter().rev() {
                stack.push((child, depth + 2));
            }
        }
    }
}

/// Writes the line(s) of one node (without its children).
fn write_node(doc: &Document, id: NodeId, indent: &str, out: &mut String) {
    let node = doc.node(id);
    // Writing to a String cannot fail.
    match &node.data {
        NodeData::Document | NodeData::DocumentFragment => {
            let _ = writeln!(out, "| {indent}#document");
        }
        NodeData::Doctype { name, .. } => {
            let _ = writeln!(out, "| {indent}<!DOCTYPE {name}>");
        }
        NodeData::Element(e) => {
            let prefix = if e.name.ns == ns!(svg) {
                "svg "
            } else if e.name.ns == ns!(mathml) {
                "math "
            } else {
                ""
            };
            let _ = writeln!(out, "| {indent}<{prefix}{}>", e.name.local);
            let mut attrs: Vec<_> = e.attributes().iter().collect();
            attrs.sort_by(|a, b| a.name.local.cmp(&b.name.local));
            for attr in attrs {
                let _ = writeln!(out, "| {indent}  {}=\"{}\"", attr.name.local, attr.value);
            }
            if e.template_contents.is_some() {
                let _ = writeln!(out, "| {indent}  content");
            }
        }
        NodeData::Text(t) => {
            let _ = writeln!(out, "| {indent}\"{t}\"");
        }
        NodeData::Comment(c) => {
            let _ = writeln!(out, "| {indent}<!-- {c} -->");
        }
        NodeData::ProcessingInstruction { target, data } => {
            let _ = writeln!(out, "| {indent}<?{target} {data}>");
        }
    }
}
