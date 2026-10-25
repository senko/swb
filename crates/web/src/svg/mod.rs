//! The SVG element interfaces for scripts (SVG 2,
//! <https://www.w3.org/TR/SVG2/>), feature 6c2 of M8.
//!
//! - `class_of_svg`: the interface of an element in the SVG namespace, a
//!   table by local name (measured in Chromium,
//!   `tools/probes/web-svg-elements.json`); a name outside the table is
//!   `SVGElement`;
//! - `SVGElement`: `className` (an `SVGAnimatedString` over the `class`
//!   attribute), `ownerSVGElement`, `viewportElement`, and the members of
//!   `HTMLOrSVGElement` (`dataset`, `tabIndex`, `autofocus`, `nonce`);
//! - `SVGAnimatedString`: `baseVal` and `animVal` read the attribute; a
//!   write to `baseVal` sets it;
//! - `SVGSVGElement.getElementById`, the defaults of `SVGStyleElement.type`
//!   and `media`, `SVGAElement.relList`.
//!
//! `crates/web/idl/svg.idl` says which members of SVG 2 swb leaves out
//! (animated lengths and the other value types, geometry, layout).

use std::collections::HashMap;
use std::sync::OnceLock;

use swb_dom::{Document, LocalName, NodeId, ns};
use swb_js::{ClassId, Gc, Object};

use crate::bindings::{self as b, Cx, Platform};
use crate::generated as g;
use crate::html::{attribute, set_attribute};
use crate::state::{Slot, WebState, dom, same_object};

/// The interface of each SVG tag name that has its own interface.
fn table() -> &'static HashMap<LocalName, ClassId> {
    static TABLE: OnceLock<HashMap<LocalName, ClassId>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let entries: [(&str, ClassId); 32] = [
            ("a", g::SVGA_ELEMENT),
            ("circle", g::SVG_CIRCLE_ELEMENT),
            ("clipPath", g::SVG_CLIP_PATH_ELEMENT),
            ("defs", g::SVG_DEFS_ELEMENT),
            ("desc", g::SVG_DESC_ELEMENT),
            ("ellipse", g::SVG_ELLIPSE_ELEMENT),
            ("foreignObject", g::SVG_FOREIGN_OBJECT_ELEMENT),
            ("g", g::SVGG_ELEMENT),
            ("image", g::SVG_IMAGE_ELEMENT),
            ("line", g::SVG_LINE_ELEMENT),
            ("linearGradient", g::SVG_LINEAR_GRADIENT_ELEMENT),
            ("marker", g::SVG_MARKER_ELEMENT),
            ("mask", g::SVG_MASK_ELEMENT),
            ("metadata", g::SVG_METADATA_ELEMENT),
            ("path", g::SVG_PATH_ELEMENT),
            ("pattern", g::SVG_PATTERN_ELEMENT),
            ("polygon", g::SVG_POLYGON_ELEMENT),
            ("polyline", g::SVG_POLYLINE_ELEMENT),
            ("radialGradient", g::SVG_RADIAL_GRADIENT_ELEMENT),
            ("rect", g::SVG_RECT_ELEMENT),
            ("script", g::SVG_SCRIPT_ELEMENT),
            ("stop", g::SVG_STOP_ELEMENT),
            ("style", g::SVG_STYLE_ELEMENT),
            ("svg", g::SVGSVG_ELEMENT),
            ("switch", g::SVG_SWITCH_ELEMENT),
            ("symbol", g::SVG_SYMBOL_ELEMENT),
            ("text", g::SVG_TEXT_ELEMENT),
            ("textPath", g::SVG_TEXT_PATH_ELEMENT),
            ("title", g::SVG_TITLE_ELEMENT),
            ("tspan", g::SVGT_SPAN_ELEMENT),
            ("use", g::SVG_USE_ELEMENT),
            ("view", g::SVG_VIEW_ELEMENT),
        ];
        entries
            .into_iter()
            .map(|(name, class)| (LocalName::from(name), class))
            .collect()
    })
}

/// The interface of an element in the SVG namespace with this local name.
pub(crate) fn class_of_svg(local: &LocalName) -> ClassId {
    table().get(local).copied().unwrap_or(g::SVG_ELEMENT)
}

/// True if `node` is an element in the SVG namespace.
fn is_svg_element(dom: &Document, node: NodeId) -> bool {
    dom.element(node).is_some_and(|e| e.name.ns == ns!(svg))
}

/// True if `node` is an SVG element named `local`.
fn is_svg_named(dom: &Document, node: NodeId, local: &str) -> bool {
    dom.element(node)
        .is_some_and(|e| e.name.ns == ns!(svg) && &**e.local_name() == local)
}

impl g::svg_element::SVGElementImpl for WebState {
    /// `className`: the one `SVGAnimatedString` of the element.
    fn class_name(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Gc<Object>> {
        let realm = cx.realm();
        Ok(same_object(cx.rt, this, Slot::ClassName, |rt| {
            rt.new_host_object(realm, g::SVG_ANIMATED_STRING, b::node_data(this))
        })?)
    }

    /// `ownerSVGElement`: the nearest `svg` ancestor, also through HTML
    /// elements (`foreignObject`); none for an outermost `svg` element
    /// (whose parent is no SVG element or is a `foreignObject`). Measured
    /// in Chromium.
    fn owner_svg_element(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        let dom = dom(cx.rt)?;
        if is_svg_named(dom, this, "svg")
            && !dom
                .parent(this)
                .is_some_and(|p| is_svg_element(dom, p) && !is_svg_named(dom, p, "foreignObject"))
        {
            return Ok(None);
        }
        Ok(dom.ancestors(this).find(|&n| is_svg_named(dom, n, "svg")))
    }

    /// `viewportElement`: the nearest ancestor that is an `svg`, `symbol`
    /// or `image` element (measured in Chromium; `marker` and `pattern`
    /// are not viewports for it).
    fn viewport_element(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        let dom = dom(cx.rt)?;
        Ok(dom.ancestors(this).find(|&n| {
            ["svg", "symbol", "image"]
                .iter()
                .any(|name| is_svg_named(dom, n, name))
        }))
    }

    fn dataset(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Gc<Object>> {
        crate::html::element::dataset(cx, this)
    }

    fn tab_index(cx: &mut Cx<'_>, this: NodeId) -> b::Result<i32> {
        crate::html::element::tab_index(cx, this)
    }
}

/// The element of an `SVGAnimatedString` (its host data is the node).
fn animated_element(this: Platform) -> b::Result<NodeId> {
    b::node_of(this.data).ok_or_else(|| swb_js::VmError::type_error("Illegal invocation").into())
}

/// The value of the `class` attribute (the only attribute that swb
/// animates as a string).
fn class_value(cx: &Cx<'_>, this: Platform) -> b::Result<String> {
    let element = animated_element(this)?;
    Ok(attribute(cx, element, "class")?.unwrap_or_default())
}

impl g::svg_animated_string::SVGAnimatedStringImpl for WebState {
    fn base_val(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        class_value(cx, this)
    }

    /// Setting `baseVal` sets the attribute (an empty string leaves an
    /// empty attribute).
    fn set_base_val(cx: &mut Cx<'_>, this: Platform, value: String) -> b::Result<()> {
        let element = animated_element(this)?;
        set_attribute(cx, element, "class", &value)
    }

    /// Without animation `animVal` is `baseVal`.
    fn anim_val(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        class_value(cx, this)
    }
}

impl g::svgsvg_element::SVGSVGElementImpl for WebState {
    /// <https://www.w3.org/TR/SVG2/struct.html#__svg__SVGSVGElement__getElementById>:
    /// the first descendant element with the id (an empty id finds none).
    fn get_element_by_id(
        cx: &mut Cx<'_>,
        this: NodeId,
        element_id: String,
    ) -> b::Result<Option<NodeId>> {
        if element_id.is_empty() {
            return Ok(None);
        }
        let dom = dom(cx.rt)?;
        Ok(dom.descendants(this).skip(1).find(|&n| {
            dom.element(n).and_then(swb_dom::ElementData::id) == Some(element_id.as_str())
        }))
    }
}

impl g::svg_style_element::SVGStyleElementImpl for WebState {
    /// `type`: `text/css` without the attribute (measured in Chromium).
    fn type_(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(attribute(cx, this, "type")?.unwrap_or_else(|| "text/css".to_owned()))
    }

    /// `media`: `all` without the attribute (measured in Chromium).
    fn media(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(attribute(cx, this, "media")?.unwrap_or_else(|| "all".to_owned()))
    }

    /// `disabled`: the enabled state of the style sheet, which swb does
    /// not model for SVG `style` elements; always false (measured in
    /// Chromium: a write changes neither the value nor an attribute).
    fn disabled(cx: &mut Cx<'_>, this: NodeId) -> b::Result<bool> {
        let _ = (cx, this);
        Ok(false)
    }

    fn set_disabled(cx: &mut Cx<'_>, this: NodeId, value: bool) -> b::Result<()> {
        let _ = (cx, this, value);
        Ok(())
    }
}

impl g::svga_element::SVGAElementImpl for WebState {
    fn rel_list(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Gc<Object>> {
        crate::html::token_list(cx, this, "rel")
    }
}
