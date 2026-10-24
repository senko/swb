//! The members that the `ParentNode` and `ChildNode` mixins add to several
//! interface traits (`Element`, `Document`, `DocumentFragment`,
//! `CharacterData`, `DocumentType`): one macro each, with the interface
//! name for the exception messages.

/// The `ParentNode` members of an interface trait. `$interface` is the
/// interface name for the messages.
macro_rules! parent_node_methods {
    ($interface:literal) => {
        fn children(
            cx: &mut $crate::bindings::Cx<'_>,
            this: ::swb_dom::NodeId,
        ) -> $crate::bindings::Result<::swb_js::Gc<::swb_js::Object>> {
            $crate::dom::children_collection(cx, this)
        }

        fn first_element_child(
            cx: &mut $crate::bindings::Cx<'_>,
            this: ::swb_dom::NodeId,
        ) -> $crate::bindings::Result<Option<::swb_dom::NodeId>> {
            $crate::dom::first_element_child(cx, this)
        }

        fn last_element_child(
            cx: &mut $crate::bindings::Cx<'_>,
            this: ::swb_dom::NodeId,
        ) -> $crate::bindings::Result<Option<::swb_dom::NodeId>> {
            $crate::dom::last_element_child(cx, this)
        }

        fn child_element_count(
            cx: &mut $crate::bindings::Cx<'_>,
            this: ::swb_dom::NodeId,
        ) -> $crate::bindings::Result<u32> {
            $crate::dom::child_element_count(cx, this)
        }

        fn prepend(
            cx: &mut $crate::bindings::Cx<'_>,
            this: ::swb_dom::NodeId,
            nodes: Vec<$crate::generated::types::NodeOrDomString>,
        ) -> $crate::bindings::Result<()> {
            $crate::dom::tree_ops::prepend(cx, this, nodes)
        }

        fn append(
            cx: &mut $crate::bindings::Cx<'_>,
            this: ::swb_dom::NodeId,
            nodes: Vec<$crate::generated::types::NodeOrDomString>,
        ) -> $crate::bindings::Result<()> {
            $crate::dom::tree_ops::append(cx, this, nodes)
        }

        fn replace_children(
            cx: &mut $crate::bindings::Cx<'_>,
            this: ::swb_dom::NodeId,
            nodes: Vec<$crate::generated::types::NodeOrDomString>,
        ) -> $crate::bindings::Result<()> {
            $crate::dom::tree_ops::replace_children(cx, this, nodes)
        }

        fn query_selector(
            cx: &mut $crate::bindings::Cx<'_>,
            this: ::swb_dom::NodeId,
            selectors: String,
        ) -> $crate::bindings::Result<Option<::swb_dom::NodeId>> {
            $crate::dom::queries::query_selector(cx, this, &selectors)
        }

        fn query_selector_all(
            cx: &mut $crate::bindings::Cx<'_>,
            this: ::swb_dom::NodeId,
            selectors: String,
        ) -> $crate::bindings::Result<::swb_js::Gc<::swb_js::Object>> {
            $crate::dom::queries::query_selector_all(cx, this, &selectors)
        }
    };
}

/// The `ChildNode` members of an interface trait.
macro_rules! child_node_methods {
    ($interface:literal) => {
        fn before(
            cx: &mut $crate::bindings::Cx<'_>,
            this: ::swb_dom::NodeId,
            nodes: Vec<$crate::generated::types::NodeOrDomString>,
        ) -> $crate::bindings::Result<()> {
            $crate::dom::tree_ops::before(cx, this, nodes)
        }

        fn after(
            cx: &mut $crate::bindings::Cx<'_>,
            this: ::swb_dom::NodeId,
            nodes: Vec<$crate::generated::types::NodeOrDomString>,
        ) -> $crate::bindings::Result<()> {
            $crate::dom::tree_ops::after(cx, this, nodes)
        }

        fn replace_with(
            cx: &mut $crate::bindings::Cx<'_>,
            this: ::swb_dom::NodeId,
            nodes: Vec<$crate::generated::types::NodeOrDomString>,
        ) -> $crate::bindings::Result<()> {
            $crate::dom::tree_ops::replace_with(cx, this, nodes)
        }

        fn remove(
            cx: &mut $crate::bindings::Cx<'_>,
            this: ::swb_dom::NodeId,
        ) -> $crate::bindings::Result<()> {
            $crate::dom::tree_ops::remove(cx, this)
        }
    };
}
