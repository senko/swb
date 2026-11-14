//! The `on*` attributes and `autofocus` that an arena ever had.
//!
//! Event handler content attributes
//! (<https://html.spec.whatwg.org/multipage/webappapis.html#event-handler-content-attributes>)
//! are read by the script runtime when a script first touches the element,
//! so the runtime cannot tell from its own state whether an event can reach
//! a handler. The arena keeps the names of the attributes that begin with
//! `on` that an element ever had (born with it: the parser, `cloneNode`,
//! the markup setters, a node moved in from another arena; or set later).
//! A name stays when the attribute goes. The set is an over-approximation by
//! design: an event whose handler name is not in it has no content
//! attribute handler on any node of the arena. The set also holds the name
//! `autofocus`, so that the popover focusing steps search a popover for an
//! `autofocus` descendant only in arenas that ever had one.

use crate::tree::{Document, NodeData};

impl Document {
    /// True if an element of this arena ever had an attribute named `name`
    /// (any namespace) that begins with `on`.
    pub fn ever_had_on_attribute(&self, name: &str) -> bool {
        self.on_attrs
            .as_ref()
            .is_some_and(|names| names.contains(name))
    }

    /// True if an element of this arena ever had an `autofocus` attribute.
    pub fn ever_had_autofocus(&self) -> bool {
        self.ever_had_on_attribute("autofocus")
    }

    /// Notes the `on*` attributes of a new node.
    #[inline]
    pub(crate) fn note_on_attributes(&mut self, data: &NodeData) {
        let NodeData::Element(element) = data else {
            return;
        };
        for attribute in &element.attrs {
            self.note_on_attribute(&attribute.name.local);
        }
    }

    /// Notes the attribute `local` if its name begins with `on` or is
    /// `autofocus`.
    #[inline]
    pub(crate) fn note_on_attribute(&mut self, local: &str) {
        if !local.starts_with("on") && local != "autofocus" {
            return;
        }
        let names = self.on_attrs.get_or_insert_default();
        if !names.contains(local) {
            names.insert(local.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{NodeId, parse_fragment, parse_html};

    #[test]
    fn the_parser_and_set_attribute_note_names() {
        let mut doc = parse_html("<body onload=a><p id=p onclick=b data-on=1></p><i>");
        assert!(doc.ever_had_on_attribute("onload"));
        assert!(doc.ever_had_on_attribute("onclick"));
        assert!(!doc.ever_had_on_attribute("onfocus"));
        assert!(!doc.ever_had_on_attribute("data-on"));
        let p = doc.element_by_id("p").expect("p");
        // Set later, then removed: the name stays.
        doc.set_attr(p, "onfocus", "c").expect("set");
        doc.remove_attr(p, "onfocus");
        assert!(doc.ever_had_on_attribute("onfocus"));
        // A second `body` tag adds its attributes to the first.
        let doc = parse_html("<body><body onkeyup=a>");
        assert!(doc.ever_had_on_attribute("onkeyup"));
    }

    #[test]
    fn markup_clones_and_moves_note_names() {
        let mut doc = parse_html("<body><div id=d></div>");
        let d = doc.element_by_id("d").expect("d");
        let fragment = parse_fragment(&mut doc, d, "<b onmouseup=x></b>").expect("fragment");
        assert!(doc.ever_had_on_attribute("onmouseup"));
        assert!(doc.first_element_child(fragment).is_some());
        // A node moved in from another arena.
        let mut other = parse_html("<body><i id=i onwheel=y></i>");
        let i = other.element_by_id("i").expect("i");
        other.remove(i);
        let subtree = other.take_subtree(i);
        assert!(!doc.ever_had_on_attribute("onwheel"));
        doc.put_subtree(NodeId::DOCUMENT, subtree).expect("put");
        assert!(doc.ever_had_on_attribute("onwheel"));
    }
}
