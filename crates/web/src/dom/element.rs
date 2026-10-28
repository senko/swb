//! `Element` (DOM §4.9, <https://dom.spec.whatwg.org/#interface-element>)
//! and `HTMLElement`'s `dataset`: the names, the identifying attributes,
//! the attribute methods (`attrs.rs`) and the `ParentNode`,
//! `NonDocumentTypeChildNode` and `ChildNode` mixins.

use swb_dom::{NodeId, local_name};

use super::tag_name;
use super::{
    attrs, lists, markup, next_element_sibling, previous_element_sibling, queries, token_list,
};
use crate::bindings::{self as b, Cx, Platform};
use crate::generated as g;
use crate::state::{Slot, WebState, dom, same_object};

impl g::element::ElementImpl for WebState {
    parent_node_methods!("Element");
    child_node_methods!("Element");

    fn namespace_uri(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<String>> {
        let dom = dom(cx.rt)?;
        let ns = dom.element(this).map(|e| e.name.ns.to_string());
        Ok(ns.filter(|ns| !ns.is_empty()))
    }

    fn prefix(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<String>> {
        let dom = dom(cx.rt)?;
        Ok(dom
            .element(this)
            .and_then(|e| e.name.prefix.as_ref())
            .map(ToString::to_string))
    }

    fn local_name(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        let dom = dom(cx.rt)?;
        Ok(dom
            .element(this)
            .map(|e| e.name.local.to_string())
            .unwrap_or_default())
    }

    fn tag_name(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(tag_name(&dom(cx.rt)?.node(this).data))
    }

    /// `classList`: the one token list of the `class` attribute.
    fn class_list(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
        let realm = cx.realm();
        Ok(same_object(cx.rt, this, Slot::ClassList, |rt| {
            token_list::new_token_list(rt, realm, this, local_name!("class"))
        })?)
    }

    fn attributes(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
        let realm = cx.realm();
        Ok(same_object(cx.rt, this, Slot::Attributes, |rt| {
            attrs::new_named_node_map(rt, realm, this)
        })?)
    }

    fn previous_element_sibling(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        previous_element_sibling(cx, this)
    }

    fn next_element_sibling(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        next_element_sibling(cx, this)
    }

    fn has_attributes(cx: &mut Cx<'_>, this: NodeId) -> b::Result<bool> {
        Ok(dom(cx.rt)?
            .element(this)
            .is_some_and(|e| !e.attributes().is_empty()))
    }

    fn get_attribute_names(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Vec<String>> {
        attrs::attribute_names(cx, this)
    }

    fn get_attribute(
        cx: &mut Cx<'_>,
        this: NodeId,
        qualified_name: String,
    ) -> b::Result<Option<String>> {
        attrs::get_attribute(cx, this, &qualified_name)
    }

    fn get_attribute_ns(
        cx: &mut Cx<'_>,
        this: NodeId,
        namespace: Option<String>,
        local_name: String,
    ) -> b::Result<Option<String>> {
        attrs::get_attribute_ns(cx, this, namespace.as_deref(), &local_name)
    }

    fn set_attribute(
        cx: &mut Cx<'_>,
        this: NodeId,
        qualified_name: String,
        value: String,
    ) -> b::Result<()> {
        attrs::set_attribute_by_name(cx, this, &qualified_name, &value)
    }

    fn set_attribute_ns(
        cx: &mut Cx<'_>,
        this: NodeId,
        namespace: Option<String>,
        qualified_name: String,
        value: String,
    ) -> b::Result<()> {
        attrs::set_attribute_ns(cx, this, namespace, &qualified_name, &value)
    }

    fn remove_attribute(cx: &mut Cx<'_>, this: NodeId, qualified_name: String) -> b::Result<()> {
        attrs::remove_attribute_by_name(cx, this, &qualified_name)
    }

    fn remove_attribute_ns(
        cx: &mut Cx<'_>,
        this: NodeId,
        namespace: Option<String>,
        local_name: String,
    ) -> b::Result<()> {
        attrs::remove_attribute_ns(cx, this, namespace.as_deref(), &local_name)
    }

    fn toggle_attribute(
        cx: &mut Cx<'_>,
        this: NodeId,
        qualified_name: String,
        force: Option<bool>,
    ) -> b::Result<bool> {
        attrs::toggle_attribute(cx, this, &qualified_name, force)
    }

    fn has_attribute(cx: &mut Cx<'_>, this: NodeId, qualified_name: String) -> b::Result<bool> {
        Ok(attrs::find_by_qualified_name(cx.rt, this, &qualified_name)?.is_some())
    }

    fn has_attribute_ns(
        cx: &mut Cx<'_>,
        this: NodeId,
        namespace: Option<String>,
        local_name: String,
    ) -> b::Result<bool> {
        Ok(attrs::find_by_namespace(cx.rt, this, namespace.as_deref(), &local_name)?.is_some())
    }

    fn get_attribute_node(
        cx: &mut Cx<'_>,
        this: NodeId,
        qualified_name: String,
    ) -> b::Result<Option<swb_js::Gc<swb_js::Object>>> {
        attrs::get_attribute_node(cx, this, &qualified_name)
    }

    fn get_attribute_node_ns(
        cx: &mut Cx<'_>,
        this: NodeId,
        namespace: Option<String>,
        local_name: String,
    ) -> b::Result<Option<swb_js::Gc<swb_js::Object>>> {
        attrs::get_attribute_node_ns(cx, this, namespace.as_deref(), &local_name)
    }

    fn set_attribute_node(
        cx: &mut Cx<'_>,
        this: NodeId,
        attr: Platform,
    ) -> b::Result<Option<swb_js::Gc<swb_js::Object>>> {
        attrs::set_attribute_node(cx, this, attr)
    }

    fn set_attribute_node_ns(
        cx: &mut Cx<'_>,
        this: NodeId,
        attr: Platform,
    ) -> b::Result<Option<swb_js::Gc<swb_js::Object>>> {
        attrs::set_attribute_node(cx, this, attr)
    }

    fn remove_attribute_node(
        cx: &mut Cx<'_>,
        this: NodeId,
        attr: Platform,
    ) -> b::Result<swb_js::Gc<swb_js::Object>> {
        attrs::remove_attribute_node(cx, this, attr)
    }

    fn matches(cx: &mut Cx<'_>, this: NodeId, selectors: String) -> b::Result<bool> {
        queries::matches(cx, this, &selectors)
    }

    fn closest(cx: &mut Cx<'_>, this: NodeId, selectors: String) -> b::Result<Option<NodeId>> {
        queries::closest(cx, this, &selectors)
    }

    fn webkit_matches_selector(
        cx: &mut Cx<'_>,
        this: NodeId,
        selectors: String,
    ) -> b::Result<bool> {
        queries::matches(cx, this, &selectors)
    }

    fn get_elements_by_tag_name(
        cx: &mut Cx<'_>,
        this: NodeId,
        qualified_name: String,
    ) -> b::Result<swb_js::Gc<swb_js::Object>> {
        lists::tag_name(cx, this, &qualified_name)
    }

    fn get_elements_by_tag_name_ns(
        cx: &mut Cx<'_>,
        this: NodeId,
        namespace: Option<String>,
        local_name: String,
    ) -> b::Result<swb_js::Gc<swb_js::Object>> {
        lists::tag_name_ns(cx, this, namespace.as_deref(), &local_name)
    }

    fn get_elements_by_class_name(
        cx: &mut Cx<'_>,
        this: NodeId,
        class_names: String,
    ) -> b::Result<swb_js::Gc<swb_js::Object>> {
        lists::class_names(cx, this, &class_names)
    }

    fn insert_adjacent_element(
        cx: &mut Cx<'_>,
        this: NodeId,
        where_: String,
        element: NodeId,
    ) -> b::Result<Option<NodeId>> {
        markup::insert_adjacent_element(cx, this, &where_, element)
    }

    fn insert_adjacent_text(
        cx: &mut Cx<'_>,
        this: NodeId,
        where_: String,
        data: String,
    ) -> b::Result<()> {
        markup::insert_adjacent_text(cx, this, &where_, data)
    }

    fn inner_html(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        markup::inner_html(cx, this)
    }

    fn set_inner_html(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
        markup::set_inner_html(cx, this, &value)
    }

    fn outer_html(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        markup::outer_html(cx, this)
    }

    fn set_outer_html(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
        markup::set_outer_html(cx, this, &value)
    }

    fn insert_adjacent_html(
        cx: &mut Cx<'_>,
        this: NodeId,
        position: String,
        text: String,
    ) -> b::Result<()> {
        markup::insert_adjacent_html(cx, this, &position, &text)
    }

    // ----- CSSOM View (`cssom_view.rs`) -----

    fn scroll_top(cx: &mut Cx<'_>, this: NodeId) -> b::Result<f64> {
        Ok(crate::cssom_view::scroll_metrics(cx.rt, this)?.top)
    }

    fn set_scroll_top(cx: &mut Cx<'_>, this: NodeId, value: f64) -> b::Result<()> {
        let request = crate::ScrollRequest {
            y: Some(value),
            ..crate::ScrollRequest::default()
        };
        crate::cssom_view::scroll(cx.rt, Some(this), request)
    }

    fn scroll_left(cx: &mut Cx<'_>, this: NodeId) -> b::Result<f64> {
        Ok(crate::cssom_view::scroll_metrics(cx.rt, this)?.left)
    }

    fn set_scroll_left(cx: &mut Cx<'_>, this: NodeId, value: f64) -> b::Result<()> {
        let request = crate::ScrollRequest {
            x: Some(value),
            ..crate::ScrollRequest::default()
        };
        crate::cssom_view::scroll(cx.rt, Some(this), request)
    }

    fn scroll_width(cx: &mut Cx<'_>, this: NodeId) -> b::Result<i32> {
        Ok(crate::cssom_view::scroll_metrics(cx.rt, this)?.width)
    }

    fn scroll_height(cx: &mut Cx<'_>, this: NodeId) -> b::Result<i32> {
        Ok(crate::cssom_view::scroll_metrics(cx.rt, this)?.height)
    }

    fn client_top(cx: &mut Cx<'_>, this: NodeId) -> b::Result<i32> {
        Ok(crate::cssom_view::client_metrics(cx.rt, this)?[1])
    }

    fn client_left(cx: &mut Cx<'_>, this: NodeId) -> b::Result<i32> {
        Ok(crate::cssom_view::client_metrics(cx.rt, this)?[0])
    }

    fn client_width(cx: &mut Cx<'_>, this: NodeId) -> b::Result<i32> {
        Ok(crate::cssom_view::client_metrics(cx.rt, this)?[2])
    }

    fn client_height(cx: &mut Cx<'_>, this: NodeId) -> b::Result<i32> {
        Ok(crate::cssom_view::client_metrics(cx.rt, this)?[3])
    }

    fn get_client_rects(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
        crate::cssom_view::get_client_rects(cx, this)
    }

    fn get_bounding_client_rect(
        cx: &mut Cx<'_>,
        this: NodeId,
    ) -> b::Result<swb_js::Gc<swb_js::Object>> {
        crate::cssom_view::get_bounding_client_rect(cx, this)
    }

    fn scroll_into_view(
        cx: &mut Cx<'_>,
        this: NodeId,
        arg: crate::generated::types::BooleanOrScrollIntoViewOptions,
    ) -> b::Result<()> {
        crate::cssom_view::scroll_into_view(cx.rt, this, arg)
    }

    fn scroll_1(
        cx: &mut Cx<'_>,
        this: NodeId,
        options: crate::generated::types::ScrollToOptions,
    ) -> b::Result<()> {
        let request = crate::cssom_view::options_request(&options, false);
        crate::cssom_view::scroll(cx.rt, Some(this), request)
    }

    fn scroll_2(cx: &mut Cx<'_>, this: NodeId, x: f64, y: f64) -> b::Result<()> {
        let request = crate::cssom_view::point_request(x, y, false);
        crate::cssom_view::scroll(cx.rt, Some(this), request)
    }

    fn scroll_to_1(
        cx: &mut Cx<'_>,
        this: NodeId,
        options: crate::generated::types::ScrollToOptions,
    ) -> b::Result<()> {
        Self::scroll_1(cx, this, options)
    }

    fn scroll_to_2(cx: &mut Cx<'_>, this: NodeId, x: f64, y: f64) -> b::Result<()> {
        Self::scroll_2(cx, this, x, y)
    }

    fn scroll_by_1(
        cx: &mut Cx<'_>,
        this: NodeId,
        options: crate::generated::types::ScrollToOptions,
    ) -> b::Result<()> {
        let request = crate::cssom_view::options_request(&options, true);
        crate::cssom_view::scroll(cx.rt, Some(this), request)
    }

    fn scroll_by_2(cx: &mut Cx<'_>, this: NodeId, x: f64, y: f64) -> b::Result<()> {
        let request = crate::cssom_view::point_request(x, y, true);
        crate::cssom_view::scroll(cx.rt, Some(this), request)
    }
}
