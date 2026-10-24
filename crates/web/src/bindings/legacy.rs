//! Legacy platform objects (Web IDL §3.9,
//! <https://webidl.spec.whatwg.org/#es-legacy-platform-objects>): the
//! internal methods of objects with indexed and named properties
//! (`NodeList`, `HTMLCollection`, `DOMStringMap`, `Storage`), as the host
//! hooks of `swb_js` (ADR 0028 section 6, item 2).
//!
//! The generated glue of an interface describes its special operations
//! with a [`Legacy`] value (functions that convert the values and call the
//! hand-written trait functions) and gives each hook a small wrapper that
//! calls the functions below with it. A hook that answers `None` leaves
//! the key to the ordinary method on the stored properties (expandos).
//!
//! Chromium's messages for refused writes, definitions and deletions
//! ([`refusal`]) and the prefixes of exceptions of the special operations
//! were measured in `tools/probes/web-legacy.json`. Where Chromium departs
//! from Web IDL, swb follows Web IDL; the exceptions are noted at the
//! functions.

use swb_js::{
    ClassId, Gc, Object, Property, PropertyDescriptor, PropertyKey, Refusal, Runtime, SetAnswer,
    Value, VmError, VmResult,
};

use super::exception::{Exception, throw_with_prefix};
use super::{NodeId, Platform, Result, node_of};

/// The supported property indices (`0..n`; the function returns `n`).
pub type CountFn = fn(&mut Runtime, u32, Platform) -> Result<u32>;
/// The indexed property getter, its result converted.
pub type IndexedGetFn = fn(&mut Runtime, u32, Platform, u32) -> Result<Value>;
/// The indexed property setter; it converts the value.
pub type IndexedSetFn = fn(&mut Runtime, u32, Platform, u32, Value) -> Result<()>;
/// The supported property names, in order.
pub type NamesFn = fn(&mut Runtime, u32, Platform) -> Result<Vec<String>>;
/// Whether a name is a supported property name.
pub type IsSupportedFn = fn(&mut Runtime, u32, Platform, &str) -> Result<bool>;
/// The named property getter, its result converted.
pub type NamedGetFn = fn(&mut Runtime, u32, Platform, String) -> Result<Value>;
/// The named property setter; it converts the value.
pub type NamedSetFn = fn(&mut Runtime, u32, Platform, String, Value) -> Result<()>;
/// The named property deleter: `false` if the deletion failed.
pub type NamedDeleteFn = fn(&mut Runtime, u32, Platform, String) -> Result<bool>;

/// The indexed properties of an interface.
#[derive(Clone, Copy, Debug)]
pub struct Indexed {
    /// The number of supported property indices.
    pub count: CountFn,
    /// The getter.
    pub get: IndexedGetFn,
    /// The setter, if the interface has one.
    pub set: Option<IndexedSetFn>,
}

/// The named properties of an interface.
#[derive(Clone, Copy, Debug)]
pub struct Named {
    /// The supported property names.
    pub names: NamesFn,
    /// The test of one name.
    pub is_supported: IsSupportedFn,
    /// The getter.
    pub get: NamedGetFn,
    /// The setter, if the interface has one.
    pub set: Option<NamedSetFn>,
    /// The deleter, if the interface has one.
    pub delete: Option<NamedDeleteFn>,
}

/// The special operations of an interface (with the inherited ones) and
/// the extended attributes of its named properties.
#[derive(Clone, Copy, Debug)]
pub struct Legacy {
    /// The interface name (in messages).
    pub interface: &'static str,
    /// The class of `DOMException` in the runtime.
    pub dom_exception: ClassId,
    /// The indexed properties.
    pub indexed: Option<Indexed>,
    /// The named properties.
    pub named: Option<Named>,
    /// `[LegacyOverrideBuiltIns]`.
    pub override_builtins: bool,
    /// `[LegacyUnenumerableNamedProperties]`.
    pub unenumerable: bool,
    /// The unforgeable property names (§3.9).
    pub unforgeable: &'static [&'static str],
}

/// The node of a legacy platform object that is a node (`HTMLFormElement`).
pub fn node(this: Platform) -> Result<NodeId> {
    node_of(this.data).ok_or_else(|| VmError::type_error("Illegal invocation").into())
}

/// `[[GetOwnProperty]]` (§3.9.1): `LegacyPlatformObjectGetOwnProperty(O,
/// P, false)`.
pub fn get_own_property(
    rt: &mut Runtime,
    object: Gc<Object>,
    key: PropertyKey,
    spec: &Legacy,
) -> VmResult<Option<Property>> {
    let cx = Call::new(rt, object, spec)?;
    cx.own_property(rt, key, false)
}

/// `[[Set]]` (§3.9.2): the setters when the object is the receiver, else
/// the ordinary steps with the own property without the named ones.
pub fn set(
    rt: &mut Runtime,
    object: Gc<Object>,
    key: PropertyKey,
    value: Value,
    receiver: Value,
    spec: &Legacy,
) -> VmResult<SetAnswer> {
    let cx = Call::new(rt, object, spec)?;
    if receiver == Value::Object(object) {
        if let (
            Some(Indexed {
                set: Some(setter), ..
            }),
            PropertyKey::Index(index),
        ) = (spec.indexed, key)
        {
            cx.set_indexed(rt, setter, index, value)?;
            return Ok(SetAnswer::Done(true));
        }
        if let Some(Named {
            set: Some(setter), ..
        }) = spec.named
            && let Some(name) = name_of(rt, key)
        {
            cx.set_named(rt, setter, key, name, value)?;
            return Ok(SetAnswer::Done(true));
        }
    }
    // LegacyPlatformObjectGetOwnProperty(O, P, true): the indexed property,
    // else the stored one. The value is not needed.
    if let (Some(indexed), PropertyKey::Index(index)) = (spec.indexed, key)
        && index < cx.count(rt, indexed, index)?
    {
        return Ok(SetAnswer::WithOwn(Property::Data {
            value: Value::Undefined,
            writable: indexed.set.is_some(),
            enumerable: true,
            configurable: true,
        }));
    }
    Ok(SetAnswer::WithStored)
}

/// `[[DefineOwnProperty]]` (§3.9.3). Deviation: Web IDL refuses a generic
/// descriptor (`{ enumerable: true }`, not a data descriptor); Chromium
/// calls the setter with `undefined` (measured), and so does swb. Only an
/// accessor descriptor is refused.
pub fn define_own_property(
    rt: &mut Runtime,
    object: Gc<Object>,
    key: PropertyKey,
    desc: &PropertyDescriptor,
    spec: &Legacy,
) -> VmResult<Option<bool>> {
    let cx = Call::new(rt, object, spec)?;
    let accessor = desc.get.is_some() || desc.set.is_some();
    let value = desc.value.unwrap_or(Value::Undefined);
    if let (Some(indexed), PropertyKey::Index(index)) = (spec.indexed, key) {
        let Some(setter) = indexed.set.filter(|_| !accessor) else {
            return Ok(Some(false));
        };
        cx.set_indexed(rt, setter, index, value)?;
        return Ok(Some(true));
    }
    let Some(named) = spec.named else {
        return Ok(None);
    };
    let Some(name) = name_of(rt, key) else {
        return Ok(None);
    };
    if spec.unforgeable.contains(&name.as_str()) {
        return Ok(None);
    }
    let creating = !cx.is_supported(rt, named, &name)?;
    if spec.override_builtins || rt.stored_own_property(object, key)?.is_none() {
        match named.set {
            None if !creating => return Ok(Some(false)),
            None => {}
            Some(_) if accessor => return Ok(Some(false)),
            Some(setter) => {
                cx.set_named(rt, setter, key, name, value)?;
                return Ok(Some(true));
            }
        }
    }
    Ok(None)
}

/// `[[Delete]]` (§3.9.4).
pub fn delete(
    rt: &mut Runtime,
    object: Gc<Object>,
    key: PropertyKey,
    spec: &Legacy,
) -> VmResult<Option<bool>> {
    let cx = Call::new(rt, object, spec)?;
    if let (Some(indexed), PropertyKey::Index(index)) = (spec.indexed, key) {
        return Ok(Some(index >= cx.count(rt, indexed, index)?));
    }
    if let Some(named) = spec.named
        && let Some(name) = name_of(rt, key)
        && cx.is_supported(rt, named, &name)?
        && cx.visible(rt, key)?
    {
        let Some(deleter) = named.delete else {
            return Ok(Some(false));
        };
        let done = deleter(rt, cx.realm, cx.this, name);
        return Ok(Some(
            cx.check(rt, done, |rt| cx.key_prefix(rt, "delete", key))?,
        ));
    }
    Ok(None)
}

/// `[[PreventExtensions]]` (§3.9.5): legacy platform objects stay
/// extensible (`Object.freeze` throws "Cannot freeze", as in Chromium).
pub fn prevent_extensions(_rt: &mut Runtime, _object: Gc<Object>) -> VmResult<Option<bool>> {
    Ok(Some(false))
}

/// `[[OwnPropertyKeys]]` (§3.9.6): the supported indices, then the visible
/// supported names (the runtime appends the stored keys). Chromium lists
/// the stored string keys before the names; swb follows Web IDL.
pub fn own_keys(
    rt: &mut Runtime,
    object: Gc<Object>,
    keys: &mut Vec<PropertyKey>,
    spec: &Legacy,
) -> VmResult<()> {
    let cx = Call::new(rt, object, spec)?;
    if let Some(indexed) = spec.indexed {
        let count = cx.count(rt, indexed, 0)?;
        // The runtime charges the keys after the hook returns; a failed
        // reservation is an error, not an abort.
        keys.try_reserve(count as usize)
            .map_err(swb_js::Error::from)?;
        keys.extend((0..count).map(PropertyKey::Index));
    }
    if let Some(named) = spec.named {
        let names = (named.names)(rt, cx.realm, cx.this);
        let prefix = |_: &Runtime| {
            format!(
                "Failed to enumerate the properties of '{}': ",
                spec.interface
            )
        };
        for name in cx.check(rt, names, prefix)? {
            let key = rt.key_from_str(&name)?;
            if cx.visible(rt, key)? {
                keys.push(key);
            }
        }
    }
    Ok(())
}

/// The message of a refused write, definition or deletion (Chromium's,
/// measured): "Failed to set an indexed property \[0\] on '`NodeList`':
/// Indexed property setter is not supported." and the others. `None` when
/// the refusal does not come from the indexed or named properties (a
/// read-only expando, an accessor on the prototype): V8's message.
pub fn refusal(
    rt: &mut Runtime,
    object: Gc<Object>,
    key: PropertyKey,
    why: Refusal,
    spec: &Legacy,
) -> VmResult<Option<String>> {
    let cx = Call::new(rt, object, spec)?;
    let interface = spec.interface;
    if let (Some(indexed), PropertyKey::Index(index)) = (spec.indexed, key) {
        let set = format!("Failed to set an indexed property [{index}] on '{interface}': ");
        return Ok(match (why, indexed.set.is_some()) {
            (Refusal::Delete, _) => Some(format!(
                "Failed to delete an indexed property [{index}] from '{interface}': Index property deleter is not supported."
            )),
            (Refusal::Set, false) => {
                Some(format!("{set}Indexed property setter is not supported."))
            }
            (Refusal::Define, false) => {
                Some(format!("{set}Index property setter is not supported."))
            }
            (Refusal::Define, true) => Some(format!("{set}Accessor properties are not allowed.")),
            (Refusal::Set, true) => None,
        });
    }
    let (Some(named), Some(name)) = (spec.named, name_of(rt, key)) else {
        return Ok(None);
    };
    let unshadowed = spec.override_builtins || rt.stored_own_property(object, key)?.is_none();
    if !unshadowed {
        return Ok(None);
    }
    let set = format!("Failed to set a named property '{name}' on '{interface}': ");
    Ok(match (why, named.set.is_some()) {
        (Refusal::Define, true) => Some(format!("{set}Accessor properties are not allowed.")),
        (Refusal::Set | Refusal::Define, false) if cx.is_supported(rt, named, &name)? => {
            Some(format!("{set}Named property setter is not supported."))
        }
        (Refusal::Delete, _) if named.delete.is_none() && cx.is_supported(rt, named, &name)? => {
            Some(format!(
                "Failed to delete a named property '{name}' from '{interface}': Named property deleter is not supported."
            ))
        }
        _ => None,
    })
}

/// The name of a key that is a String in the sense of Web IDL (an array
/// index too, in its decimal form); `None` for a symbol.
fn name_of(rt: &Runtime, key: PropertyKey) -> Option<String> {
    rt.key_to_string(key)
}

/// One internal method call on a legacy platform object: the object, its
/// host data, the realm of the running code, and the description.
struct Call<'a> {
    object: Gc<Object>,
    this: Platform,
    realm: u32,
    spec: &'a Legacy,
}

impl<'a> Call<'a> {
    fn new(rt: &Runtime, object: Gc<Object>, spec: &'a Legacy) -> VmResult<Call<'a>> {
        let (_, data) = rt
            .host_object(object.into())
            .ok_or(VmError::type_error("Illegal invocation"))?;
        Ok(Call {
            object,
            this: Platform { object, data },
            realm: rt.running_realm(),
            spec,
        })
    }

    /// `LegacyPlatformObjectGetOwnProperty` (§3.9.7).
    fn own_property(
        &self,
        rt: &mut Runtime,
        key: PropertyKey,
        mut ignore_named: bool,
    ) -> VmResult<Option<Property>> {
        if let (Some(indexed), PropertyKey::Index(index)) = (self.spec.indexed, key) {
            if index < self.count(rt, indexed, index)? {
                let value = (indexed.get)(rt, self.realm, self.this, index);
                let value = self.check(rt, value, |_| self.index_prefix("read", index))?;
                return Ok(Some(Property::Data {
                    value,
                    writable: indexed.set.is_some(),
                    enumerable: true,
                    configurable: true,
                }));
            }
            ignore_named = true;
        }
        if let Some(named) = self.spec.named
            && !ignore_named
            && let Some(name) = name_of(rt, key)
            && self.visible(rt, key)?
            && self.is_supported(rt, named, &name)?
        {
            let value = (named.get)(rt, self.realm, self.this, name);
            let value = self.check(rt, value, |rt| self.key_prefix(rt, "read", key))?;
            return Ok(Some(Property::Data {
                value,
                writable: named.set.is_some(),
                enumerable: !self.spec.unenumerable,
                configurable: true,
            }));
        }
        Ok(None)
    }

    /// The named property visibility algorithm (§3.9.7) for a supported
    /// name: no own (stored) property of that name, and with
    /// `[LegacyOverrideBuiltIns]` or none on the prototype chain. (No
    /// named properties object is on these chains.)
    fn visible(&self, rt: &mut Runtime, key: PropertyKey) -> VmResult<bool> {
        if rt.stored_own_property(self.object, key)?.is_some() {
            return Ok(false);
        }
        if self.spec.override_builtins {
            return Ok(true);
        }
        let mut current = rt.prototype_of(self.object)?;
        while let Some(prototype) = current {
            if rt.own_property_of(prototype, key)?.is_some() {
                return Ok(false);
            }
            current = rt.prototype_of(prototype)?;
        }
        Ok(true)
    }

    /// The number of supported indices (`index` names the property in
    /// the message of an exception).
    fn count(&self, rt: &mut Runtime, indexed: Indexed, index: u32) -> VmResult<u32> {
        let count = (indexed.count)(rt, self.realm, self.this);
        self.check(rt, count, |_| self.index_prefix("read", index))
    }

    fn is_supported(&self, rt: &mut Runtime, named: Named, name: &str) -> VmResult<bool> {
        let supported = (named.is_supported)(rt, self.realm, self.this, name);
        self.check(rt, supported, |_| self.named_prefix("read", name))
    }

    fn set_indexed(
        &self,
        rt: &mut Runtime,
        setter: IndexedSetFn,
        index: u32,
        value: Value,
    ) -> VmResult<()> {
        let done = setter(rt, self.realm, self.this, index, value);
        self.check(rt, done, |_| self.index_prefix("set", index))
    }

    fn set_named(
        &self,
        rt: &mut Runtime,
        setter: NamedSetFn,
        key: PropertyKey,
        name: String,
        value: Value,
    ) -> VmResult<()> {
        let done = setter(rt, self.realm, self.this, name, value);
        self.check(rt, done, |rt| self.key_prefix(rt, "set", key))
    }

    /// The error of an exception, with the prefix of the operation. The
    /// prefix is made only for an exception: the special operations run on
    /// every property access.
    fn check<T>(
        &self,
        rt: &mut Runtime,
        result: Result<T>,
        prefix: impl FnOnce(&Runtime) -> String,
    ) -> VmResult<T> {
        result.map_err(|exception: Exception| {
            let prefix = prefix(rt);
            throw_with_prefix(rt, self.realm, &prefix, self.spec.dom_exception, exception)
        })
    }

    /// "Failed to set an indexed property [0] on 'X': " (Chromium's;
    /// "read" for the getter is swb's guess in the same form).
    fn index_prefix(&self, verb: &str, index: u32) -> String {
        format!(
            "Failed to {verb} an indexed property [{index}] {} '{}': ",
            preposition(verb),
            self.spec.interface
        )
    }

    /// [`Call::named_prefix`] for the name of `key` (the operation took the
    /// name string).
    fn key_prefix(&self, rt: &Runtime, verb: &str, key: PropertyKey) -> String {
        self.named_prefix(verb, &name_of(rt, key).unwrap_or_default())
    }

    /// "Failed to set a named property 'x' on 'X': " (Chromium's; "read"
    /// for the getter is swb's guess in the same form).
    fn named_prefix(&self, verb: &str, name: &str) -> String {
        format!(
            "Failed to {verb} a named property '{name}' {} '{}': ",
            preposition(verb),
            self.spec.interface
        )
    }
}

/// "on" after "set", "from" after "read" and "delete" (Chromium's words).
fn preposition(verb: &str) -> &'static str {
    if verb == "set" { "on" } else { "from" }
}
