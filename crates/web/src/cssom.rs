//! `element.style` (CSSOM §6.7 `CSSStyleDeclaration` and §7.1
//! `ElementCSSInlineStyle`, <https://drafts.csswg.org/cssom/>, ADR 0028
//! section 10) and the `CSS` namespace (`CSS.supports`, `CSS.escape`).
//!
//! The declaration block of an element is `swb_style::cssom::
//! StyleDeclarations`, built from the `style` attribute and kept in the
//! binding state with the attribute text and the DOM generation it was
//! built for: reads in a loop parse the attribute once, and a block is
//! parsed again only when the attribute text changed
//! (`setAttribute('style', ...)` is seen by the next read). A change by a
//! script (`setProperty`, `removeProperty`, `cssText`, a property write)
//! writes the block's serialization to the attribute, through the
//! attribute funnel (journaled like any attribute change, so the next
//! style flush sees it), and keeps the block.
//!
//! The camel-cased, webkit-cased and dashed property names
//! (`style.backgroundColor`, `style.webkitTransform`,
//! `style['background-color']`) are named properties of each object
//! (`[SwbPropertyNames]` in `idl/cssom.idl`), as in Chromium: a name exists
//! exactly when swb parses the property, so feature tests such as
//! `'transform' in el.style` see what swb supports.

use std::collections::HashMap;

use swb_dom::{LocalName, NodeId, QualName, QuirksMode, ns};
use swb_js::{
    Attributes, Gc, GenericData, HostData, NativeCall, NativeReturn, Object, Runtime, Tracer,
    Value, VmError, VmResult,
};
use swb_style::cssom::{self, StyleDeclarations};

use crate::bindings::{self as b, Context, Cx, Platform};
use crate::dom::set_attribute;
use crate::generated as g;
use crate::state::{Slot, WebState, dom, same_object, state, state_mut};

/// The block of an element's `style` attribute, with what it was built
/// from.
pub(crate) struct StyleCache {
    /// The DOM generation when the attribute was last compared.
    generation: u64,
    /// The attribute text the block stands for (`None`: no attribute).
    source: Option<String>,
    /// The block.
    block: StyleDeclarations,
}

/// The blocks by element.
///
/// Nodes live as long as their document (ADR 0028 section 3), so the
/// blocks of elements that scripts styled and threw away would stay too.
/// When the map reaches its limit, the blocks of elements that are not
/// connected go; a later read builds such a block again from the
/// attribute. (Departure: the parse moves `!important` declarations to
/// the end, so `cssText` of such an element can change order.)
#[derive(Default)]
pub(crate) struct StyleBlocks {
    map: HashMap<NodeId, StyleCache>,
    /// The size at which the next new block drops the blocks of elements
    /// that are not connected (at least [`MIN_BLOCKS`]).
    limit: usize,
}

/// The number of blocks kept before those of elements that are not
/// connected go.
const MIN_BLOCKS: usize = 4096;

/// Drops the blocks of elements that are not connected, and sets the next
/// limit to twice the blocks that stay, so the drops cost O(1) per block.
fn drop_detached_blocks(rt: &mut Runtime) -> b::Result<()> {
    let mut map = std::mem::take(&mut state_mut(rt)?.style_blocks.map);
    let doc = dom(rt)?;
    map.retain(|&node, _| doc.is_connected(node));
    let blocks = &mut state_mut(rt)?.style_blocks;
    blocks.limit = map.len().saturating_mul(2).max(MIN_BLOCKS);
    blocks.map = map;
    Ok(())
}

/// The data of a `CSSStyleDeclaration`: its element.
struct StyleData {
    element: NodeId,
}

impl GenericData for StyleData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        0
    }
}

/// `style` of an HTML or SVG element: its one `CSSStyleDeclaration`.
pub(crate) fn style(cx: &mut Cx<'_>, element: NodeId) -> b::Result<Gc<Object>> {
    let realm = cx.realm();
    Ok(same_object(cx.rt, element, Slot::Style, |rt| {
        let data = rt.alloc_host_data(StyleData { element })?;
        rt.new_host_object(realm, g::CSS_STYLE_DECLARATION, data)
    })?)
}

fn element_of(rt: &Runtime, this: Platform) -> b::Result<NodeId> {
    match this.data {
        HostData::Generic(handle) => Ok(rt
            .heap()
            .generic::<StyleData>(handle)
            .map_err(VmError::from)?
            .element),
        HostData::Word(_) => Err(VmError::type_error("Illegal invocation").into()),
    }
}

/// Runs `f` on the block of `element`, built or brought up to date from
/// the attribute.
fn with_block<R>(
    rt: &mut Runtime,
    element: NodeId,
    f: impl FnOnce(&mut StyleDeclarations) -> R,
) -> b::Result<R> {
    let doc = dom(rt)?;
    let generation = doc.generation();
    let quirks = doc.quirks_mode == QuirksMode::Quirks;
    let current = doc.element(element).and_then(|e| e.attr("style"));
    let blocks = &state(rt)?.style_blocks;
    let cached = blocks.map.get(&element);
    let fresh =
        cached.is_some_and(|c| c.generation == generation || c.source.as_deref() == current);
    let full = cached.is_none() && blocks.map.len() >= blocks.limit.max(MIN_BLOCKS);
    let rebuilt = (!fresh).then(|| StyleCache {
        generation,
        source: current.map(str::to_owned),
        block: StyleDeclarations::parse(current.unwrap_or(""), quirks),
    });
    if full {
        drop_detached_blocks(rt)?;
    }
    let blocks = &mut state_mut(rt)?.style_blocks.map;
    let cache = match rebuilt {
        Some(cache) => {
            blocks.insert(element, cache);
            blocks.get_mut(&element)
        }
        None => blocks.get_mut(&element),
    }
    .ok_or(VmError::type_error("the style block is missing"))?;
    cache.generation = generation;
    Ok(f(&mut cache.block))
}

/// Writes the block of `element` to its `style` attribute (CSSOM "update
/// style attribute for") and keeps the block as the attribute's.
fn write_attribute(rt: &mut Runtime, element: NodeId) -> b::Result<()> {
    let text = state(rt)?
        .style_blocks
        .map
        .get(&element)
        .map(|c| c.block.css_text())
        .unwrap_or_default();
    let name = QualName::new(None, ns!(), LocalName::from("style"));
    set_attribute(rt, element, &name, &text)?;
    let generation = dom(rt)?.generation();
    if let Some(cache) = state_mut(rt)?.style_blocks.map.get_mut(&element) {
        cache.generation = generation;
        cache.source = Some(text);
    }
    Ok(())
}

/// Changes the block of `this` with `f` (which returns whether it changed)
/// and writes the attribute if it did.
fn change(
    cx: &mut Cx<'_>,
    this: Platform,
    f: impl FnOnce(&mut StyleDeclarations) -> bool,
) -> b::Result<()> {
    let element = element_of(cx.rt, this)?;
    if with_block(cx.rt, element, f)? {
        write_attribute(cx.rt, element)?;
    }
    Ok(())
}

/// Reads from the block of `this`.
fn read<R>(
    cx: &mut Cx<'_>,
    this: Platform,
    f: impl FnOnce(&mut StyleDeclarations) -> R,
) -> b::Result<R> {
    let element = element_of(cx.rt, this)?;
    with_block(cx.rt, element, f)
}

impl g::css_style_declaration::CSSStyleDeclarationImpl for WebState {
    fn css_text(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        read(cx, this, |b| b.css_text())
    }

    /// The `cssText` setter always writes the attribute (also `""`).
    fn set_css_text(cx: &mut Cx<'_>, this: Platform, value: String) -> b::Result<()> {
        change(cx, this, |b| {
            b.set_css_text(&value);
            true
        })
    }

    fn length(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        read(cx, this, |b| u32::try_from(b.len()).unwrap_or(u32::MAX))
    }

    /// Always null: the declarations of a `style` attribute have no rule.
    fn parent_rule(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<Option<Gc<Object>>> {
        Ok(None)
    }

    fn css_float(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        read(cx, this, |b| b.property_value("float"))
    }

    fn set_css_float(cx: &mut Cx<'_>, this: Platform, value: String) -> b::Result<()> {
        change(cx, this, |b| b.set_property("float", &value, ""))
    }

    fn item(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<String> {
        read(cx, this, |b| b.item(index as usize).unwrap_or_default())
    }

    fn get_property_value(cx: &mut Cx<'_>, this: Platform, property: String) -> b::Result<String> {
        read(cx, this, |b| b.property_value(&property))
    }

    fn get_property_priority(
        cx: &mut Cx<'_>,
        this: Platform,
        property: String,
    ) -> b::Result<String> {
        read(cx, this, |b| b.property_priority(&property).to_owned())
    }

    fn set_property(
        cx: &mut Cx<'_>,
        this: Platform,
        property: String,
        value: String,
        priority: String,
    ) -> b::Result<()> {
        change(cx, this, |b| b.set_property(&property, &value, &priority))
    }

    fn remove_property(cx: &mut Cx<'_>, this: Platform, property: String) -> b::Result<String> {
        let element = element_of(cx.rt, this)?;
        let (old, changed) = with_block(cx.rt, element, |b| b.remove_property(&property))?;
        if changed {
            write_attribute(cx.rt, element)?;
        }
        Ok(old)
    }

    fn index_count(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        Self::length(cx, this)
    }

    /// The camel-cased and webkit-cased names of the supported properties
    /// (Chromium lists these; the dashed names are not listed).
    fn supported_names(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<Vec<String>> {
        Ok(cssom::attribute_names().to_vec())
    }

    fn is_supported_name(_cx: &mut Cx<'_>, _this: Platform, name: &str) -> b::Result<bool> {
        Ok(cssom::property_of_attribute(name).is_some())
    }

    fn named_getter(cx: &mut Cx<'_>, this: Platform, name: String) -> b::Result<String> {
        let property = cssom::property_of_attribute(&name).unwrap_or_default();
        read(cx, this, |b| b.property_value(&property))
    }

    /// A property write is `setProperty(name, value)` without a priority.
    fn named_setter(cx: &mut Cx<'_>, this: Platform, name: String, value: String) -> b::Result<()> {
        let Some(property) = cssom::property_of_attribute(&name) else {
            return Ok(());
        };
        change(cx, this, |b| b.set_property(&property, &value, ""))
    }
}

// ----- The CSS namespace -----

/// Defines the `CSS` namespace object on the global object of `realm`
/// (CSS Conditional 3 §8.1, CSSOM §2.1): an ordinary object whose
/// `@@toStringTag` is `"CSS"`, with `supports` and `escape` (Web IDL §3.13
/// "Namespaces"; written by hand, the generator has no namespaces). The
/// property of the global object is writable, configurable and not
/// enumerable, as an interface object (measured in Chromium).
pub(crate) fn install_css_namespace(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let scope = rt.heap_mut().open_scope();
    let result = (|| {
        let object = rt.new_plain_object(realm)?;
        rt.heap_mut().record(object);
        rt.define_method(realm, object, "escape", 1, css_escape, Attributes::MEMBER)?;
        rt.define_method(
            realm,
            object,
            "supports",
            1,
            css_supports,
            Attributes::MEMBER,
        )?;
        let tag = rt.string_value("CSS")?;
        let key = rt.to_string_tag_key();
        rt.define_value_with_key(
            object,
            key,
            tag,
            Attributes {
                writable: false,
                enumerable: false,
                configurable: true,
            },
        )?;
        let global = rt.global(realm)?;
        rt.define_value(global, "CSS", Value::Object(object), Attributes::HIDDEN)
    })();
    rt.heap_mut().close_scope(scope).map_err(VmError::from)?;
    result
}

/// The string argument `index` of a call.
fn string_argument(rt: &mut Runtime, call: &NativeCall, index: usize) -> b::Result<String> {
    let value = rt.arg(call, index);
    b::convert::dom_string(rt, value).map_err(|e| e.argument(index + 1))
}

/// `CSS.supports(property, value)` and `CSS.supports(conditionText)`.
fn css_supports(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let context = Context::Operation("CSS", "supports");
    b::glue::run(rt, call, context, g::DOM_EXCEPTION, |rt| {
        let supported = match call.argc() {
            0 => return Err(b::exception::not_enough_arguments(1, 0)),
            1 => cssom::supports_condition(&string_argument(rt, call, 0)?),
            _ => {
                let property = string_argument(rt, call, 0)?;
                let value = string_argument(rt, call, 1)?;
                cssom::supports_property(&property, &value)
            }
        };
        Ok(Value::Bool(supported))
    })
}

/// `CSS.escape(ident)`: the identifier serialized (CSSOM §2.1).
fn css_escape(rt: &mut Runtime, call: &NativeCall) -> VmResult<NativeReturn> {
    let context = Context::Operation("CSS", "escape");
    b::glue::run(rt, call, context, g::DOM_EXCEPTION, |rt| {
        if call.argc() == 0 {
            return Err(b::exception::not_enough_arguments(1, 0));
        }
        let ident = string_argument(rt, call, 0)?;
        Ok(b::to_js::string(rt, &cssom::escape_identifier(&ident))?)
    })
}
