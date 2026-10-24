//! `DOMStringMap` (HTML §3.2.6.6 "Custom data attributes",
//! <https://html.spec.whatwg.org/multipage/dom.html#domstringmap>):
//! `HTMLElement.dataset`, a legacy platform object (Web IDL §3.9) whose
//! named properties are the `data-*` attributes.
//!
//! A property name maps to an attribute name by putting a hyphen before
//! each ASCII upper case letter (and lowercasing it) after the prefix
//! `data-`; the reverse removes a hyphen that is followed by an ASCII
//! lower case letter and uppercases the letter.

use swb_dom::{LocalName, NodeId, QualName, ns};
use swb_js::{Gc, GenericData, HostData, Object, Runtime, Tracer, VmError, VmResult};

use super::attrs;
use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;
use crate::state::{WebState, dom};

/// The data of a `dataset`: the element.
struct MapData {
    element: NodeId,
}

impl GenericData for MapData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        0
    }
}

/// A new `DOMStringMap` of `element`.
pub(super) fn new_string_map(
    rt: &mut Runtime,
    realm: u32,
    element: NodeId,
) -> VmResult<Gc<Object>> {
    let data = rt.alloc_host_data(MapData { element })?;
    rt.new_host_object(realm, g::DOM_STRING_MAP, data)
}

fn element_of(rt: &Runtime, this: Platform) -> b::Result<NodeId> {
    match this.data {
        HostData::Generic(handle) => Ok(rt
            .heap()
            .generic::<MapData>(handle)
            .map_err(VmError::from)?
            .element),
        HostData::Word(_) => Err(VmError::type_error("Illegal invocation").into()),
    }
}

/// The attribute name for a property name, or none if the name has a
/// hyphen followed by an ASCII lower case letter (it would not map back).
fn attribute_for(property: &str) -> Option<String> {
    let mut out = String::with_capacity(property.len() + 5);
    out.push_str("data-");
    let mut previous = '\0';
    for c in property.chars() {
        if previous == '-' && c.is_ascii_lowercase() {
            return None;
        }
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
        previous = c;
    }
    Some(out)
}

/// The property name of the attribute `name`, or none if it does not start
/// with `data-` or has an ASCII upper case letter.
fn property_for(attribute: &str) -> Option<String> {
    let rest = attribute.strip_prefix("data-")?;
    if rest.bytes().any(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let mut out = String::with_capacity(rest.len());
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        match chars.peek() {
            Some(next) if c == '-' && next.is_ascii_lowercase() => {
                out.push(next.to_ascii_uppercase());
                chars.next();
            }
            _ => out.push(c),
        }
    }
    Some(out)
}

impl g::dom_string_map::DOMStringMapImpl for WebState {
    fn supported_names(cx: &mut Cx<'_>, this: Platform) -> b::Result<Vec<String>> {
        let element = element_of(cx.rt, this)?;
        Ok(dom(cx.rt)?
            .element(element)
            .map(|e| {
                e.attributes()
                    .iter()
                    .filter(|a| a.name.ns == ns!())
                    .filter_map(|a| property_for(&a.name.local))
                    .collect()
            })
            .unwrap_or_default())
    }

    fn is_supported_name(cx: &mut Cx<'_>, this: Platform, name: &str) -> b::Result<bool> {
        let element = element_of(cx.rt, this)?;
        let Some(attribute) = attribute_for(name) else {
            return Ok(false);
        };
        Ok(dom(cx.rt)?
            .element(element)
            .is_some_and(|e| e.attr(&attribute).is_some()))
    }

    fn named_getter(cx: &mut Cx<'_>, this: Platform, name: String) -> b::Result<String> {
        let element = element_of(cx.rt, this)?;
        let attribute = attribute_for(&name).unwrap_or_default();
        Ok(dom(cx.rt)?
            .element(element)
            .and_then(|e| e.attr(&attribute))
            .unwrap_or_default()
            .to_owned())
    }

    /// DOM "set a named property": a hyphen followed by a lower case
    /// letter is a `SyntaxError`, a name that is not a valid attribute name
    /// an `InvalidCharacterError`.
    fn named_setter(cx: &mut Cx<'_>, this: Platform, name: String, value: String) -> b::Result<()> {
        let element = element_of(cx.rt, this)?;
        let Some(attribute) = attribute_for(&name) else {
            return Err(Exception::dom(
                "SyntaxError",
                format!("'{name}' is not a valid property name."),
            ));
        };
        if !attrs::is_valid_attribute_name(&attribute) {
            return Err(Exception::dom(
                "InvalidCharacterError",
                format!("'{attribute}' is not a valid attribute name."),
            ));
        }
        let qualified = QualName::new(None, ns!(), LocalName::from(attribute));
        attrs::set_attribute(cx.rt, element, &qualified, &value)
    }

    fn named_deleter(cx: &mut Cx<'_>, this: Platform, name: String) -> b::Result<()> {
        let element = element_of(cx.rt, this)?;
        if let Some(attribute) = attribute_for(&name) {
            let qualified = QualName::new(None, ns!(), LocalName::from(attribute));
            attrs::remove_attribute(cx.rt, element, &qualified)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{attribute_for, property_for};

    #[test]
    fn names_map_both_ways() {
        assert_eq!(attribute_for("fooBar").as_deref(), Some("data-foo-bar"));
        assert_eq!(attribute_for("A").as_deref(), Some("data--a"));
        assert_eq!(attribute_for("a-1").as_deref(), Some("data-a-1"));
        assert_eq!(attribute_for("foo-bar"), None);
        assert_eq!(property_for("data-a-b-c").as_deref(), Some("aBC"));
        assert_eq!(property_for("data-").as_deref(), Some(""));
        assert_eq!(property_for("data-a-").as_deref(), Some("a-"));
        assert_eq!(property_for("data--b").as_deref(), Some("B"));
        assert_eq!(property_for("data-1").as_deref(), Some("1"));
        assert_eq!(property_for("data-X"), None);
        assert_eq!(property_for("title"), None);
    }
}
