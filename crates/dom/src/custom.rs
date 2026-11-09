//! The custom element state of elements (DOM §4.9, "custom element state";
//! HTML §4.13.4, "valid custom element name"; ADR 0035 section 1).
//!
//! Each element has a state and a definition number. The state decides
//! whether `:defined` matches; the definition number indexes the table of
//! definitions of the script runtime (`swb_web`), 0 for none. An element in
//! the HTML namespace whose local name is a valid custom element name starts
//! `undefined`; every other element starts `uncustomized`.
//!
//! A change of whether a connected element is defined sets a flag of the
//! document (`take_defined_changed`; no journal entry and no mutation
//! record), so that the engine restyles.
//!
//! Each arena keeps the local names of the elements that it ever created in
//! the state `undefined`: the upgrade candidates of a definition can only
//! have one of these names, so a definition of another name needs no walk.

use html5ever::LocalName;

use crate::tree::{Document, NodeData, NodeId};

/// DOM "custom element state".
/// <https://dom.spec.whatwg.org/#concept-element-custom-element-state>
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum CustomElementState {
    /// A custom element name that waits for its definition or upgrade.
    Undefined,
    /// Its upgrade or synchronous construction failed.
    Failed,
    /// Not a custom element (the state of built-in elements).
    #[default]
    Uncustomized,
    /// The constructor of its upgrade runs.
    Precustomized,
    /// A custom element.
    Custom,
}

impl CustomElementState {
    /// HTML "defined": the state is `uncustomized` or `custom` (`:defined`).
    /// <https://dom.spec.whatwg.org/#concept-element-defined>
    pub fn is_defined(self) -> bool {
        matches!(self, Self::Uncustomized | Self::Custom)
    }

    /// The state of a new element with this name that no definition
    /// constructed: `undefined` for an HTML element with a valid custom
    /// element name, else `uncustomized` (DOM "create an element" steps 6
    /// and 7).
    #[inline]
    pub fn initial(html: bool, local: &str) -> Self {
        // Every element creation asks: a byte loop over the short name is
        // cheaper than `memchr` (measured under callgrind, ADR 0035 "4a
        // implementation notes").
        if html && local.bytes().any(|b| b == b'-') && is_valid_custom_element_name(local) {
            Self::Undefined
        } else {
            Self::Uncustomized
        }
    }
}

/// The reserved names that look like custom element names (elements of SVG
/// and `MathML`).
const RESERVED: [&str; 8] = [
    "annotation-xml",
    "color-profile",
    "font-face",
    "font-face-src",
    "font-face-uri",
    "font-face-format",
    "font-face-name",
    "missing-glyph",
];

/// HTML "valid custom element name" (the current definition): a valid
/// element local name that starts with an ASCII lower alpha, contains a
/// hyphen and no ASCII upper alpha, and is not reserved. Chromium accepts
/// `x-a!` (measured, case `define-errors`), which the older `PCENChar`
/// production rejects.
/// <https://html.spec.whatwg.org/multipage/custom-elements.html#valid-custom-element-name>
pub fn is_valid_custom_element_name(name: &str) -> bool {
    // The hyphen first: most element names have none.
    name.as_bytes().contains(&b'-')
        && name.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && !name.bytes().any(|b| b.is_ascii_uppercase())
        && is_valid_element_local_name(name)
        && !RESERVED.contains(&name)
}

/// DOM "valid element local name".
/// <https://dom.spec.whatwg.org/#valid-element-local-name>
pub fn is_valid_element_local_name(name: &str) -> bool {
    element_local_name_hyphen(name).is_some()
}

/// [`is_valid_element_local_name`] with a by-product of the same scan:
/// `None` for an invalid name, else whether the name has a hyphen. Only a
/// name with a hyphen can be a custom element name, so `createElement`
/// (which validates the name anyway) learns it without a second scan
/// (ADR 0035 "4a implementation notes", performance).
pub fn element_local_name_hyphen(name: &str) -> Option<bool> {
    let mut chars = name.chars();
    let first = chars.next()?;
    let mut hyphen = false;
    if first.is_ascii_alphabetic() {
        for c in chars {
            if c.is_ascii_whitespace() || matches!(c, '\0' | '/' | '>') {
                return None;
            }
            hyphen |= c == '-';
        }
        return Some(hyphen);
    }
    if !(first == ':' || first == '_' || first >= '\u{80}') {
        return None;
    }
    for c in chars {
        if !(c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | ':' | '_') || c >= '\u{80}') {
            return None;
        }
        hyphen |= c == '-';
    }
    Some(hyphen)
}

impl crate::tree::ElementData {
    /// Sets the custom element state and the definition number of a new
    /// element (before it is in an arena: no journal entry).
    pub fn set_custom_element_state(&mut self, state: CustomElementState, definition: u32) {
        self.custom_state = state;
        self.definition = definition;
    }
}

impl Document {
    /// Sets the namespace prefix of an element (DOM "create an element"
    /// step 6.1.3.9: the result of a constructor gets the prefix of the
    /// qualified name).
    pub fn set_element_prefix(&mut self, element: NodeId, prefix: Option<html5ever::Prefix>) {
        if let Some(e) = self.element_mut(element) {
            e.name.prefix = prefix;
        }
    }

    /// The custom element state and definition number of an element (the
    /// state `uncustomized` and 0 for another node).
    pub fn custom_element_state(&self, element: NodeId) -> (CustomElementState, u32) {
        match self.get(element).map(|n| &n.data) {
            Some(NodeData::Element(e)) => (e.custom_state, e.definition),
            _ => (CustomElementState::Uncustomized, 0),
        }
    }

    /// Sets the custom element state of an element, and its definition
    /// number if `definition` is given. A change of whether a connected
    /// element is defined (`:defined`) sets the flag of
    /// [`take_defined_changed`](Self::take_defined_changed); the journal and
    /// the mutation sink do not see it.
    pub fn set_custom_element_state(
        &mut self,
        element: NodeId,
        state: CustomElementState,
        definition: Option<u32>,
    ) {
        let Some(e) = self.element_mut(element) else {
            return;
        };
        let changed = e.custom_state.is_defined() != state.is_defined();
        e.custom_state = state;
        if let Some(definition) = definition {
            e.definition = definition;
        }
        if changed && self.is_connected(element) {
            self.generation += 1;
            self.defined_changed = true;
        }
    }

    /// True if a connected element became defined or undefined since the
    /// last call, and clears the flag. It is no journal entry: a new
    /// variant of [`Change`](crate::Change) made every recorded change
    /// dearer (a measured 20 instructions for `appendChild` and `remove`).
    pub fn take_defined_changed(&mut self) -> bool {
        std::mem::take(&mut self.defined_changed)
    }

    /// True if the arena ever created an element with this local name in the
    /// state `undefined` (the possible upgrade candidates of a definition).
    pub fn created_custom_name(&self, local: &LocalName) -> bool {
        self.custom_names
            .as_ref()
            .is_some_and(|names| names.contains(local))
    }

    /// True if the arena ever held an element with a custom element name
    /// (any state but `uncustomized`): the filter of the custom element
    /// reactions of the mutation sink. Without one, no inserted or removed
    /// subtree can hold a custom element.
    #[inline]
    pub fn has_custom_names(&self) -> bool {
        self.custom_names.is_some()
    }

    /// Notes the name of a new element whose state is not `uncustomized` (the
    /// caller tested the state: this is the rare path).
    #[cold]
    pub(crate) fn note_custom_name(&mut self, data: &NodeData) {
        if let NodeData::Element(e) = data
            && e.custom_state != CustomElementState::Uncustomized
        {
            self.note_name(&e.name.local);
        }
    }

    #[cold]
    fn note_name(&mut self, local: &LocalName) {
        let names = self.custom_names.get_or_insert_default();
        if !names.contains(local) {
            names.insert(local.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The names of the probe case `define-errors` (Chromium 148) and of
    /// the HTML standard's examples.
    #[test]
    fn custom_element_names() {
        for name in [
            "x-a",
            "foo-bar",
            "x-\u{e9}",
            "a-",
            "x-a!",
            "a-b.c_d",
            "x-\u{1F600}",
        ] {
            assert!(is_valid_custom_element_name(name), "{name}");
        }
        for name in [
            "foo",
            "Foo-bar",
            "-foo",
            "font-face",
            "annotation-xml",
            "a-B",
            "1-a",
            "x-a b",
            "x-a/b",
            "x-a>",
            "x-a\0",
            "\u{e9}-a",
            "",
        ] {
            assert!(!is_valid_custom_element_name(name), "{name:?}");
        }
    }

    #[test]
    fn element_local_names() {
        for name in [
            "a",
            "my-element",
            "a@b",
            "a:b",
            "\u{e9}t\u{e9}",
            "_x",
            ":x",
            "A",
        ] {
            assert!(is_valid_element_local_name(name), "{name}");
        }
        for name in ["", "1a", "a b", "a/b", "a>", "-x", "_x@", "a\0"] {
            assert!(!is_valid_element_local_name(name), "{name:?}");
        }
    }

    #[test]
    fn element_local_name_scan_reports_the_hyphen() {
        assert_eq!(element_local_name_hyphen("div"), Some(false));
        assert_eq!(element_local_name_hyphen("x-a"), Some(true));
        assert_eq!(element_local_name_hyphen("a-"), Some(true));
        assert_eq!(element_local_name_hyphen("_x-a"), Some(true));
        assert_eq!(element_local_name_hyphen(":xa"), Some(false));
        assert_eq!(element_local_name_hyphen("x-a b"), None);
        assert_eq!(element_local_name_hyphen("-x"), None);
        assert_eq!(element_local_name_hyphen(""), None);
    }

    /// The state and the definition fit in the padding of `ElementData`
    /// (ADR 0035 section 1).
    #[test]
    #[cfg(target_pointer_width = "64")]
    fn element_data_keeps_its_size() {
        assert_eq!(size_of::<crate::tree::ElementData>(), 64);
    }

    #[test]
    fn initial_states() {
        assert_eq!(
            CustomElementState::initial(true, "x-a"),
            CustomElementState::Undefined
        );
        assert_eq!(
            CustomElementState::initial(false, "x-a"),
            CustomElementState::Uncustomized
        );
        assert_eq!(
            CustomElementState::initial(true, "div"),
            CustomElementState::Uncustomized
        );
        assert!(!CustomElementState::Undefined.is_defined());
        assert!(!CustomElementState::Failed.is_defined());
        assert!(!CustomElementState::Precustomized.is_defined());
        assert!(CustomElementState::Custom.is_defined());
        assert!(CustomElementState::Uncustomized.is_defined());
    }

    #[test]
    fn a_state_change_of_a_connected_element_sets_the_flag() {
        let mut doc = crate::parse_html("<x-a></x-a>");
        let _ = doc.take_defined_changed();
        let x = doc
            .descendants(NodeId::DOCUMENT)
            .find(|&n| doc.element(n).is_some_and(|e| &*e.name.local == "x-a"))
            .unwrap();
        assert_eq!(
            doc.custom_element_state(x),
            (CustomElementState::Undefined, 0)
        );
        assert!(doc.created_custom_name(&LocalName::from("x-a")));
        assert!(!doc.created_custom_name(&LocalName::from("x-b")));
        // `failed` and `precustomized` are not defined either: no change.
        doc.set_custom_element_state(x, CustomElementState::Failed, Some(3));
        doc.set_custom_element_state(x, CustomElementState::Precustomized, None);
        assert!(!doc.take_defined_changed());
        let generation = doc.generation();
        doc.set_custom_element_state(x, CustomElementState::Custom, None);
        assert_eq!(doc.custom_element_state(x), (CustomElementState::Custom, 3));
        assert!(doc.take_defined_changed());
        assert!(!doc.take_defined_changed());
        assert!(doc.generation() > generation);
        // A detached element: no change.
        let name = html5ever::QualName::new(None, html5ever::ns!(html), LocalName::from("x-b"));
        let d = doc.create_element(name, Vec::new()).unwrap();
        let _ = doc.take_journal();
        doc.set_custom_element_state(d, CustomElementState::Custom, None);
        assert!(!doc.take_defined_changed());
        assert!(doc.created_custom_name(&LocalName::from("x-b")));
        // A copy is a new element (DOM "clone a single node"): `undefined`,
        // without the definition.
        let copy = doc.clone_node(x, NodeId::DOCUMENT, false).unwrap().root;
        assert_eq!(
            doc.custom_element_state(copy),
            (CustomElementState::Undefined, 0)
        );
    }
}
