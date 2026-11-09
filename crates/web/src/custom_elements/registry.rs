//! The `CustomElementRegistry` interface (HTML §4.13.4,
//! <https://html.spec.whatwg.org/multipage/custom-elements.html#customelementregistry>;
//! ADR 0035 section 3) and `window.customElements`.
//!
//! `define` follows HTML's steps with Chromium's messages and order of reads
//! (measured, cases `define-errors` and `define-read-order`).

use std::collections::HashSet;
use std::rc::Rc;

use swb_dom::{LocalName, is_valid_custom_element_name};
use swb_js::{Gc, Object, Runtime, Value};

use super::{
    Callback, Definition, MAX_DEFINITIONS, MAX_NAME_BYTES, MAX_OBSERVED_BYTES, MAX_OBSERVED_NAMES,
    MAX_WHEN_DEFINED,
};
use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;
use crate::generated::types::{CustomElementConstructor, ElementDefinitionOptions};
use crate::state::{WebState, no_data, state, state_mut, window_object};

/// The form-associated callbacks that `define` reads when `formAssociated`
/// is true (it keeps none of them, ADR 0035 section 9).
const FORM_CALLBACKS: [&str; 4] = [
    "formAssociatedCallback",
    "formResetCallback",
    "formDisabledCallback",
    "formStateRestoreCallback",
];

/// What `define` reads from the constructor and its prototype.
struct Read {
    callbacks: [Option<Gc<Object>>; 5],
    observed: Vec<String>,
    disabled: Vec<String>,
    form_associated: bool,
}

fn not_supported(message: impl Into<std::borrow::Cow<'static, str>>) -> Exception {
    Exception::dom("NotSupportedError", message)
}

impl g::custom_element_registry::CustomElementRegistryImpl for WebState {
    /// <https://html.spec.whatwg.org/multipage/custom-elements.html#dom-customelementregistry-define>
    fn define(
        cx: &mut Cx<'_>,
        _this: Platform,
        name: String,
        constructor: CustomElementConstructor,
        options: ElementDefinitionOptions,
    ) -> b::Result<()> {
        let rt = &mut *cx.rt;
        let constructor = constructor.0;
        // Step 1.
        if !rt.is_constructor(Value::Object(constructor))? {
            return Err(Exception::type_error(
                "constructor argument is not a constructor",
            ));
        }
        // Step 2.
        if !is_valid_custom_element_name(&name) {
            return Err(Exception::dom(
                "SyntaxError",
                format!("\"{name}\" is not a valid custom element name"),
            ));
        }
        let local = LocalName::from(name.as_str());
        let registry = &state(rt)?.doc.custom;
        // Steps 3 and 4.
        if registry.by_name.contains_key(&local) {
            return Err(not_supported(format!(
                "the name \"{name}\" has already been used with this registry"
            )));
        }
        if registry.by_constructor.contains_key(&constructor) {
            return Err(not_supported(
                "this constructor has already been used with this registry",
            ));
        }
        // Step 5: customized built-in elements are not supported (ADR 0035
        // section 9); the messages of an invalid value are Chromium's.
        if let Some(extends) = &options.extends {
            if is_valid_custom_element_name(extends) {
                return Err(not_supported(format!(
                    "\"{extends}\" is a valid custom element name"
                )));
            }
            if crate::html::class_of_html(&LocalName::from(extends.as_str()))
                == g::HTML_UNKNOWN_ELEMENT
            {
                return Err(not_supported(format!(
                    "\"{extends}\" is an HTMLUnknownElement"
                )));
            }
            return Err(not_supported(
                "Customized built-in elements are not supported",
            ));
        }
        // Step 6.
        if registry.running {
            return Err(not_supported(
                "an element definition is already being processed",
            ));
        }
        let web = state_mut(rt)?;
        if web.custom.definitions.len() >= MAX_DEFINITIONS
            || web.custom.name_bytes + name.len() > MAX_NAME_BYTES
        {
            return Err(over_budget(web));
        }
        // Steps 7 to 9.
        web.doc.custom.running = true;
        let read = read_definition(rt, constructor);
        state_mut(rt)?.doc.custom.running = false;
        let read = read?;
        add_definition(rt, &local, constructor, &read)
    }

    fn get(cx: &mut Cx<'_>, _this: Platform, name: String) -> b::Result<Value> {
        let state = state(cx.rt)?;
        let number = state
            .doc
            .custom
            .by_name
            .get(&LocalName::from(name.as_str()))
            .copied();
        Ok(match number {
            Some(number) => Value::Object(super::definition(cx.rt, number)?.constructor),
            None => Value::Undefined,
        })
    }

    fn get_name(
        cx: &mut Cx<'_>,
        _this: Platform,
        constructor: CustomElementConstructor,
    ) -> b::Result<Option<String>> {
        let Some(&number) = state(cx.rt)?.doc.custom.by_constructor.get(&constructor.0) else {
            return Ok(None);
        };
        Ok(Some(super::definition(cx.rt, number)?.name.to_string()))
    }

    /// <https://html.spec.whatwg.org/multipage/custom-elements.html#dom-customelementregistry-whendefined>
    fn when_defined(cx: &mut Cx<'_>, _this: Platform, name: String) -> b::Result<Gc<Object>> {
        if !is_valid_custom_element_name(&name) {
            return Err(Exception::dom(
                "SyntaxError",
                format!("\"{name}\" is not a valid custom element name"),
            ));
        }
        let realm = cx.realm();
        let rt = &mut *cx.rt;
        let registry = &state(rt)?.doc.custom;
        if let Some(&number) = registry.by_name.get(&LocalName::from(name.as_str())) {
            let constructor = super::definition(rt, number)?.constructor;
            let promise = rt.resolved_promise(realm, Value::Object(constructor))?;
            return b::convert::object(promise);
        }
        if let Some(&promise) = registry.when_defined.get(&name) {
            return Ok(promise);
        }
        let promise = rt.new_pending_promise(realm)?;
        let registry = &mut state_mut(rt)?.doc.custom;
        if registry.when_defined.len() >= MAX_WHEN_DEFINED
            || registry.when_defined_bytes + name.len() > MAX_NAME_BYTES
        {
            if !registry.when_defined_warned {
                registry.when_defined_warned = true;
                log::warn!(
                    "too many names wait in customElements.whenDefined(): later promises never settle"
                );
            }
        } else {
            registry.when_defined_bytes += name.len();
            registry.when_defined.insert(name, promise);
        }
        Ok(promise)
    }

    fn upgrade(cx: &mut Cx<'_>, _this: Platform, root: b::NodeRef) -> b::Result<()> {
        // A node of another document: its node document's registry is not
        // this one, so nothing is upgraded (measured: no error).
        let Some(root) = root.here(cx.rt) else {
            return Ok(());
        };
        Ok(super::upgrade::upgrade_subtree(cx.rt, root)?)
    }
}

/// The `NotSupportedError` of the definition budget, with one warning.
fn over_budget(web: &mut WebState) -> Exception {
    if !web.custom.budget_warned {
        web.custom.budget_warned = true;
        log::warn!("the page defined too many custom elements: later definitions fail");
    }
    not_supported("swb: too many custom element definitions")
}

/// `define` step 9: the reads of the prototype and the constructor, in
/// Chromium's order.
fn read_definition(rt: &mut Runtime, constructor: Gc<Object>) -> b::Result<Read> {
    let prototype = b::convert::member(rt, Some(constructor), "prototype")?;
    let Value::Object(prototype) = prototype else {
        return Err(Exception::type_error(
            "constructor prototype is not an object",
        ));
    };
    let mut callbacks = [None; 5];
    for (slot, callback) in callbacks.iter_mut().zip(Callback::ALL) {
        *slot = read_callback(rt, prototype, callback.name())?;
    }
    let mut observed = Vec::new();
    if callbacks[4].is_some() {
        let value = b::convert::member(rt, Some(constructor), "observedAttributes")?;
        if !value.is_undefined() {
            observed = b::convert::sequence(rt, value, b::convert::dom_string)?;
        }
    }
    let mut disabled = Vec::new();
    let value = b::convert::member(rt, Some(constructor), "disabledFeatures")?;
    if !value.is_undefined() {
        disabled = b::convert::sequence(rt, value, b::convert::dom_string)?;
    }
    let value = b::convert::member(rt, Some(constructor), "formAssociated")?;
    let form_associated = b::convert::boolean(rt, value)?;
    if form_associated {
        for name in FORM_CALLBACKS {
            read_callback(rt, prototype, name)?;
        }
    }
    Ok(Read {
        callbacks,
        observed,
        disabled,
        form_associated,
    })
}

/// `Get(prototype, name)` converted to a callback function when it is not
/// `undefined`.
fn read_callback(
    rt: &mut Runtime,
    prototype: Gc<Object>,
    name: &str,
) -> b::Result<Option<Gc<Object>>> {
    let value = b::convert::member(rt, Some(prototype), name)?;
    match value {
        Value::Undefined => Ok(None),
        Value::Object(function) if rt.callable(value)? => Ok(Some(function)),
        _ => Err(Exception::type_error(format!(
            "\"{name}\" is not a function"
        ))),
    }
}

/// `define` steps 10 to 13: the new definition, its upgrade candidates and
/// the when-defined promise.
fn add_definition(
    rt: &mut Runtime,
    local: &LocalName,
    constructor: Gc<Object>,
    read: &Read,
) -> b::Result<()> {
    let web = state_mut(rt)?;
    let bytes: usize = read.observed.iter().map(String::len).sum();
    if web.custom.observed_names + read.observed.len() > MAX_OBSERVED_NAMES
        || web.custom.observed_bytes + bytes > MAX_OBSERVED_BYTES
    {
        return Err(over_budget(web));
    }
    web.custom.observed_names += read.observed.len();
    web.custom.observed_bytes += bytes;
    web.custom.name_bytes += local.len();
    let observed: Rc<HashSet<LocalName>> = Rc::new(
        read.observed
            .iter()
            .map(|name| LocalName::from(name.as_str()))
            .collect(),
    );
    web.custom.definitions.push(Definition {
        name: local.clone(),
        constructor,
        callbacks: read.callbacks,
        observed: Rc::clone(&observed),
        form_associated: read.form_associated,
        disable_internals: read.disabled.iter().any(|f| f == "internals"),
        disable_shadow: read.disabled.iter().any(|f| f == "shadow"),
        construction_stack: Vec::new(),
        window: web.doc.key,
    });
    let number = u32::try_from(web.custom.definitions.len())
        .map_err(|_| Exception::range_error("too many custom element definitions"))?;
    web.doc.custom.by_name.insert(local.clone(), number);
    web.doc.custom.by_constructor.insert(constructor, number);
    let mask = read
        .callbacks
        .iter()
        .enumerate()
        .fold(0u8, |mask, (i, c)| mask | (u8::from(c.is_some()) << i));
    let key = web.doc.key;
    web.custom.reactions.borrow_mut().add_definition(
        key,
        local.clone(),
        number,
        super::sink::DefInfo {
            callbacks: mask,
            observed,
        },
    );
    let promise = web.doc.custom.when_defined.remove(&**local);
    if promise.is_some() {
        let registry = &mut web.doc.custom;
        registry.when_defined_bytes = registry.when_defined_bytes.saturating_sub(local.len());
    }
    rt.set_host_flag_bit(b::ce::DEFINITIONS_BIT, true);
    crate::state::install_reaction_sinks(rt)?;
    // `define` has `[CEReactions]`: the upgrades run before it returns. The
    // queue is pushed here too, as the glue's scope was not pushed if this
    // is the agent's first definition.
    super::reactions::push_queue(rt);
    let enqueued = super::upgrade::upgrade_candidates(rt, number, local);
    if let Ok(candidates) = &enqueued {
        log::debug!("customElements.define('{local}'): {candidates} upgrade candidates");
    }
    let resolved = match promise {
        Some(promise) => rt.resolve_pending_promise(promise, Value::Object(constructor)),
        None => Ok(()),
    };
    let invoked = super::reactions::pop_and_invoke(rt);
    enqueued?;
    resolved?;
    invoked?;
    Ok(())
}

/// `window.customElements`: the window's registry object, made at the first
/// read.
pub(crate) fn registry_object(cx: &mut Cx<'_>) -> b::Result<Gc<Object>> {
    window_object(
        cx,
        |d| &mut d.custom.object,
        g::CUSTOM_ELEMENT_REGISTRY,
        no_data,
    )
}
