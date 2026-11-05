//! The `WindowProxy` objects without a target (HTML §7.2.3.4 for a
//! cross-origin `Window`; ADR 0031 section 5). Feature 13 writes the rules of
//! cross-origin objects; until then every internal method throws
//! `SecurityError`.

use swb_js::{
    Gc, Object, Property, PropertyDescriptor, PropertyKey, Runtime, Value, VmError, VmResult,
    WindowProxyHooks,
};

use crate::bindings::Exception;
use crate::generated as g;

/// Installs the internal methods of the proxies without a target.
pub(crate) fn install_hooks(rt: &mut Runtime) {
    rt.set_window_proxy_hooks(WindowProxyHooks {
        get_prototype_of,
        set_prototype_of,
        prevent_extensions,
        get_own_property,
        define_own_property,
        has,
        get,
        set,
        delete,
        own_keys,
    });
}

/// The `SecurityError` of each access.
fn blocked(rt: &mut Runtime) -> VmError {
    let realm = crate::state::doc_realm(rt).unwrap_or(0);
    crate::bindings::exception::throw_with_prefix(
        rt,
        realm,
        "",
        g::DOM_EXCEPTION,
        Exception::dom(
            "SecurityError",
            "Blocked a frame from accessing a cross-origin frame.",
        ),
    )
}

fn get_prototype_of(rt: &mut Runtime, _: Gc<Object>) -> VmResult<Option<Gc<Object>>> {
    Err(blocked(rt))
}

fn set_prototype_of(rt: &mut Runtime, _: Gc<Object>, _: Option<Gc<Object>>) -> VmResult<bool> {
    Err(blocked(rt))
}

fn prevent_extensions(rt: &mut Runtime, _: Gc<Object>) -> VmResult<bool> {
    Err(blocked(rt))
}

fn get_own_property(rt: &mut Runtime, _: Gc<Object>, _: PropertyKey) -> VmResult<Option<Property>> {
    Err(blocked(rt))
}

fn define_own_property(
    rt: &mut Runtime,
    _: Gc<Object>,
    _: PropertyKey,
    _: &PropertyDescriptor,
) -> VmResult<bool> {
    Err(blocked(rt))
}

fn has(rt: &mut Runtime, _: Gc<Object>, _: PropertyKey) -> VmResult<bool> {
    Err(blocked(rt))
}

fn get(rt: &mut Runtime, _: Gc<Object>, _: PropertyKey, _: Value) -> VmResult<Value> {
    Err(blocked(rt))
}

fn set(rt: &mut Runtime, _: Gc<Object>, _: PropertyKey, _: Value, _: Value) -> VmResult<bool> {
    Err(blocked(rt))
}

fn delete(rt: &mut Runtime, _: Gc<Object>, _: PropertyKey) -> VmResult<bool> {
    Err(blocked(rt))
}

fn own_keys(rt: &mut Runtime, _: Gc<Object>) -> VmResult<Vec<PropertyKey>> {
    Err(blocked(rt))
}

#[cfg(test)]
mod tests {
    use swb_js::PropertyDescriptor;

    use crate::clock::{ClockMode, PageClock};
    use crate::runtime::{Evaluation, JsonForm, ScriptRuntime};

    #[test]
    fn every_access_to_a_proxy_without_a_target_is_a_security_error() {
        let mut runtime = ScriptRuntime::new(PageClock::new(ClockMode::Virtual, None)).unwrap();
        let proxy = runtime.rt.new_window_proxy(9, None).unwrap();
        let global = runtime.rt.global(0).unwrap();
        let key = runtime.rt.key_from_str("remote").unwrap();
        let desc = PropertyDescriptor::data(proxy.into(), true, false, true);
        runtime.rt.define_property(global, key, desc).unwrap();
        let source = "
            const E = g => { try { g(); return 'no error'; } catch (e) { return e.name; } };
            [
              () => remote.x, () => { remote.x = 1; }, () => 'x' in remote, () => delete remote.x,
              () => Object.keys(remote), () => Object.getPrototypeOf(remote),
              () => Object.setPrototypeOf(remote, null), () => Object.preventExtensions(remote),
              () => Object.defineProperty(remote, 'x', {value: 1}),
              () => Object.getOwnPropertyDescriptor(remote, 'x'),
            ].map(E).join() + ' ' + Object.isExtensible(remote);";
        assert_eq!(
            runtime.evaluate(source, JsonForm::Probe),
            Evaluation::Value(format!("\"{} true\"", ["SecurityError"; 10].join(",")))
        );
    }
}
