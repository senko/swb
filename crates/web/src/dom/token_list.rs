//! `DOMTokenList` (DOM §7.1, <https://dom.spec.whatwg.org/#interface-domtokenlist>):
//! `classList`.
//!
//! A list keeps its element and the name of the attribute and reads the
//! attribute at each access: the tokens are the unique words of the
//! value, in order of first appearance. The reads (`length`, `item`,
//! `contains`) use the parsed tokens, which `WebState::token_cache` keeps
//! for the DOM generation, so an indexed loop is linear. A change serializes the tokens
//! (joined by one space) into the attribute ("update steps"). The
//! algorithms parse the value once per call and use a hash set, so
//! `add` of 100,000 tokens in one call is linear.
//!
//! Chromium departs from the update steps in one case, which swb follows:
//! `add()` with no token creates an empty attribute (measured in
//! `tools/probes/web-dom-mutation.json`). The list has no supported tokens
//! (`classList.supports` throws), as the list of `class`.

use std::collections::HashSet;
use std::rc::Rc;

use swb_dom::{LocalName, NodeId, QualName, is_html_whitespace, ns};
use swb_js::{Gc, GenericData, HostData, Object, Runtime, Tracer, VmError, VmResult};

use super::attrs;
use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;
use crate::state::{WebState, dom, state, state_mut};

/// The data of a token list.
struct ListData {
    element: NodeId,
    attribute: LocalName,
}

impl GenericData for ListData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        0
    }
}

/// A new token list of the attribute `attribute` of `element`.
pub(super) fn new_token_list(
    rt: &mut Runtime,
    realm: u32,
    element: NodeId,
    attribute: LocalName,
) -> VmResult<Gc<Object>> {
    let data = rt.alloc_host_data(ListData { element, attribute })?;
    rt.new_host_object(realm, g::DOM_TOKEN_LIST, data)
}

fn list_of(rt: &Runtime, this: Platform) -> b::Result<(NodeId, LocalName)> {
    match this.data {
        HostData::Generic(handle) => {
            let data = rt
                .heap()
                .generic::<ListData>(handle)
                .map_err(VmError::from)?;
            Ok((data.element, data.attribute.clone()))
        }
        HostData::Word(_) => Err(VmError::type_error("Illegal invocation").into()),
    }
}

/// The value of the attribute, or none.
fn raw_value(rt: &Runtime, element: NodeId, attribute: &LocalName) -> VmResult<Option<String>> {
    Ok(dom(rt)?
        .element(element)
        .and_then(|e| e.attr_by_namespace("", attribute))
        .map(|a| a.value.clone()))
}

/// DOM "ordered set parser": the unique tokens of `input`, in order.
/// <https://dom.spec.whatwg.org/#concept-ordered-set-parser>
fn parse(input: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut tokens = Vec::new();
    for token in input.split(is_html_whitespace).filter(|t| !t.is_empty()) {
        if seen.insert(token) {
            tokens.push(token.to_owned());
        }
    }
    tokens
}

fn tokens_of(rt: &Runtime, this: Platform) -> b::Result<Vec<String>> {
    let (element, attribute) = list_of(rt, this)?;
    Ok(parse(
        &raw_value(rt, element, &attribute)?.unwrap_or_default(),
    ))
}

/// The tokens for a read, parsed once per DOM generation and list.
fn cached_tokens(rt: &mut Runtime, this: Platform) -> b::Result<Rc<Vec<String>>> {
    let (element, attribute) = list_of(rt, this)?;
    let generation = dom(rt)?.generation();
    let key = (element, attribute);
    let cache = &state(rt)?.token_cache;
    if cache.0 == generation
        && let Some(tokens) = cache.1.get(&key)
    {
        return Ok(Rc::clone(tokens));
    }
    let tokens = Rc::new(parse(&raw_value(rt, key.0, &key.1)?.unwrap_or_default()));
    let cache = &mut state_mut(rt)?.token_cache;
    if cache.0 != generation {
        cache.1.clear();
        cache.0 = generation;
    }
    cache.1.insert(key, Rc::clone(&tokens));
    Ok(tokens)
}

/// The "update steps": writes the tokens into the attribute. With
/// `create`, a missing attribute is created even for an empty set.
fn update(
    rt: &mut Runtime,
    element: NodeId,
    attribute: &LocalName,
    tokens: &[String],
    create: bool,
) -> b::Result<()> {
    let has = raw_value(rt, element, attribute)?.is_some();
    if !has && tokens.is_empty() && !create {
        return Ok(());
    }
    let name = QualName::new(None, ns!(), attribute.clone());
    attrs::set_attribute(rt, element, &name, &tokens.join(" "))
}

/// The validation of the tokens (DOM "validate a token", for all tokens:
/// an empty one first, then one with whitespace).
fn validate_all<S: AsRef<str>>(tokens: &[S]) -> b::Result<()> {
    if tokens.iter().any(|t| t.as_ref().is_empty()) {
        return Err(Exception::dom(
            "SyntaxError",
            "The token provided must not be empty.",
        ));
    }
    if let Some(token) = tokens
        .iter()
        .find(|t| t.as_ref().contains(is_html_whitespace))
    {
        return Err(Exception::dom(
            "InvalidCharacterError",
            format!(
                "The token provided ('{}') contains HTML space characters, which are not valid in tokens.",
                token.as_ref()
            ),
        ));
    }
    Ok(())
}

impl g::dom_token_list::DOMTokenListImpl for WebState {
    fn length(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        let count = cached_tokens(cx.rt, this)?.len();
        u32::try_from(count).map_err(|_| Exception::range_error("too many tokens"))
    }

    fn value(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        let (element, attribute) = list_of(cx.rt, this)?;
        Ok(raw_value(cx.rt, element, &attribute)?.unwrap_or_default())
    }

    fn set_value(cx: &mut Cx<'_>, this: Platform, value: String) -> b::Result<()> {
        let (element, attribute) = list_of(cx.rt, this)?;
        let name = QualName::new(None, ns!(), attribute);
        attrs::set_attribute(cx.rt, element, &name, &value)
    }

    fn item(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<Option<String>> {
        Ok(cached_tokens(cx.rt, this)?.get(index as usize).cloned())
    }

    fn contains(cx: &mut Cx<'_>, this: Platform, token: String) -> b::Result<bool> {
        Ok(cached_tokens(cx.rt, this)?.contains(&token))
    }

    fn add(cx: &mut Cx<'_>, this: Platform, tokens: Vec<String>) -> b::Result<()> {
        validate_all(&tokens)?;
        let (element, attribute) = list_of(cx.rt, this)?;
        let mut set = tokens_of(cx.rt, this)?;
        let mut seen: HashSet<String> = set.iter().cloned().collect();
        for token in tokens {
            if seen.insert(token.clone()) {
                set.push(token);
            }
        }
        update(cx.rt, element, &attribute, &set, true)
    }

    fn remove(cx: &mut Cx<'_>, this: Platform, tokens: Vec<String>) -> b::Result<()> {
        validate_all(&tokens)?;
        let (element, attribute) = list_of(cx.rt, this)?;
        let drop: HashSet<&str> = tokens.iter().map(String::as_str).collect();
        let set: Vec<String> = tokens_of(cx.rt, this)?
            .into_iter()
            .filter(|t| !drop.contains(t.as_str()))
            .collect();
        update(cx.rt, element, &attribute, &set, false)
    }

    fn toggle(
        cx: &mut Cx<'_>,
        this: Platform,
        token: String,
        force: Option<bool>,
    ) -> b::Result<bool> {
        validate_all(&[&token])?;
        let (element, attribute) = list_of(cx.rt, this)?;
        let mut set = tokens_of(cx.rt, this)?;
        if let Some(i) = set.iter().position(|t| *t == token) {
            if force != Some(true) {
                set.remove(i);
                update(cx.rt, element, &attribute, &set, false)?;
                return Ok(false);
            }
            return Ok(true);
        }
        if force == Some(false) {
            return Ok(false);
        }
        set.push(token);
        update(cx.rt, element, &attribute, &set, false)?;
        Ok(true)
    }

    /// DOM `replace`: the first occurrence of either token becomes
    /// `new_token`; the other occurrences go.
    /// <https://dom.spec.whatwg.org/#dom-domtokenlist-replace>
    fn replace(
        cx: &mut Cx<'_>,
        this: Platform,
        token: String,
        new_token: String,
    ) -> b::Result<bool> {
        validate_all(&[&token, &new_token])?;
        let (element, attribute) = list_of(cx.rt, this)?;
        let set = tokens_of(cx.rt, this)?;
        if !set.contains(&token) {
            return Ok(false);
        }
        let mut replaced = false;
        let mut out = Vec::with_capacity(set.len());
        for t in set {
            if t == token || t == new_token {
                if !replaced {
                    replaced = true;
                    out.push(new_token.clone());
                }
            } else {
                out.push(t);
            }
        }
        update(cx.rt, element, &attribute, &out, false)?;
        Ok(true)
    }

    /// The tokens of the `class` attribute have no supported tokens.
    fn supports(_cx: &mut Cx<'_>, _this: Platform, _token: String) -> b::Result<bool> {
        Err(Exception::type_error(
            "DOMTokenList has no supported tokens.",
        ))
    }

    fn index_count(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        Self::length(cx, this)
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn tokens_are_unique_and_ordered() {
        assert_eq!(parse("  a  b a\tc\n"), ["a", "b", "c"]);
        assert_eq!(parse(" \t"), Vec::<String>::new());
    }
}
