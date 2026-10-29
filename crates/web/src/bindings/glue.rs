//! The steps that every generated member shares (Web IDL §3.7, "create an
//! operation function", the attribute getter and setter steps): the run
//! of a member body with the conversion of its exception, the brand check
//! of `this`, the argument count, the class registration in the generated
//! order, the `[LegacyUnforgeable]` templates, `[Replaceable]` and
//! `[PutForwards]`, the exposure on the global object (most interface
//! objects and the legacy factory functions as lazy properties, created at
//! their first read: ADR 0028 section 5, "Lazy interfaces notes"), and the
//! built-in objects that interfaces use, kept from before any script.

use swb_js::{
    Attributes, ClassId, ClassSpec, Gc, Interface, NativeCall, NativeReturn, Object, Property,
    PropertyDescriptor, Runtime, Value, VmError, VmResult,
};

use super::exception::{Context, Exception, not_enough_arguments, throw};
use super::{Host, NodeId, Platform, Result, glue_state, node_of};

/// Runs the body of a member and converts its exception
/// ([`super::exception::throw`]).
pub fn run(
    rt: &mut Runtime,
    call: &NativeCall,
    context: Context,
    dom_exception: ClassId,
    body: impl FnOnce(&mut Runtime) -> Result<Value>,
) -> VmResult<NativeReturn> {
    match body(rt) {
        Ok(value) => Ok(NativeReturn::Value(value)),
        Err(exception) => Err(throw(rt, call.realm(), context, dom_exception, exception)),
    }
}

/// [`run`] for an operation that returns a promise: an exception becomes
/// a rejected promise (Web IDL §3.7.6, step "if an exception was thrown
/// and the return type is a promise type"); a termination still ends the
/// task.
pub fn run_promise(
    rt: &mut Runtime,
    call: &NativeCall,
    context: Context,
    dom_exception: ClassId,
    body: impl FnOnce(&mut Runtime) -> Result<Value>,
) -> VmResult<NativeReturn> {
    let error = match body(rt) {
        Ok(value) => return Ok(NativeReturn::Value(value)),
        // Chromium names the operation in the message of a failed brand
        // check here, unlike in an exception that is thrown (measured with
        // the scroll methods).
        Err(Exception::Js(VmError::Raise {
            kind: swb_js::ThrowKind::TypeError,
            message,
        })) if message == "Illegal invocation" => throw(
            rt,
            call.realm(),
            context,
            dom_exception,
            Exception::Type(message),
        ),
        Err(exception) => throw(rt, call.realm(), context, dom_exception, exception),
    };
    let reason = match error {
        VmError::Throw(value) => value,
        VmError::Raise { kind, message } => rt.new_error(kind, &message)?.into(),
        other => return Err(other),
    };
    Ok(NativeReturn::Value(
        rt.rejected_promise(call.realm(), reason)?,
    ))
}

/// The brand check of `this` for a member of an interface that inherits
/// from `Node`: the node, or `TypeError: Illegal invocation`.
pub fn this_node(rt: &Runtime, call: &NativeCall, class: ClassId) -> Result<NodeId> {
    let data = rt.this_host_data(call, class)?;
    node_of(data).ok_or_else(|| VmError::type_error("Illegal invocation").into())
}

/// The brand check of `this` for a member of another interface.
pub fn this_platform(rt: &Runtime, call: &NativeCall, class: ClassId) -> Result<Platform> {
    let this = rt.this_or_global(call)?;
    let data = rt.host_data_of(this, class)?;
    match this {
        Value::Object(object) => Ok(Platform { object, data }),
        _ => Err(VmError::type_error("Illegal invocation").into()),
    }
}

/// Fails with Chromium's message if the call has fewer than `required`
/// arguments.
pub fn require_arguments(call: &NativeCall, required: usize) -> Result<()> {
    if call.argc() < required {
        return Err(not_enough_arguments(required, call.argc()));
    }
    Ok(())
}

/// Registers a generated class and checks that it got the number that the
/// glue names it by (the generated classes are registered first, in the
/// generated order).
pub fn define_class(rt: &mut Runtime, spec: ClassSpec, expected: ClassId) -> VmResult<()> {
    let id = rt.define_class(spec)?;
    if id != expected {
        return Err(VmError::type_error(
            "the generated classes must be registered first and in order",
        ));
    }
    Ok(())
}

/// Defines the interface object of an exposed interface as a property of
/// the global object of `realm`.
pub fn expose(rt: &mut Runtime, realm: u32, name: &str, interface: Interface) -> VmResult<()> {
    let global = rt.global(realm)?;
    rt.define_value(global, name, interface.object.into(), Attributes::HIDDEN)
}

/// A property of the global object that an interface defines (Web IDL
/// §3.7.1, §3.7.2): the interface object of an exposed interface, or a
/// legacy factory function (`Image`). The generated `GLOBALS` lists them
/// in the order of their definition; the number of a lazy property is its
/// index there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlobalName {
    /// The name of the property.
    pub name: &'static str,
    /// The interface.
    pub class: ClassId,
    /// True for a legacy factory function of the interface.
    pub factory: bool,
}

/// Defines the property `id` of `globals` on the global object of `realm`
/// with a value that is created at its first read (writable, not
/// enumerable, configurable, as Web IDL §3.7 has it; ADR 0028 section 5,
/// "Lazy interfaces notes").
pub fn expose_lazy(rt: &mut Runtime, realm: u32, globals: &[GlobalName], id: u32) -> VmResult<()> {
    let name = globals
        .get(id as usize)
        .ok_or(VmError::type_error("a lazy global without a name"))?
        .name;
    rt.define_lazy_global(realm, name, id)
}

/// The value of the lazy global property `id` of `globals` in `realm`:
/// the interface object (its interface is created now if needed) or the
/// legacy factory function.
pub fn global_value<H: Host>(
    rt: &mut Runtime,
    realm: u32,
    globals: &[GlobalName],
    id: u32,
) -> VmResult<Value> {
    let global = *globals
        .get(id as usize)
        .ok_or(VmError::type_error("a lazy global without a name"))?;
    let interface = rt.interface_or_create(realm, global.class)?;
    if global.factory {
        return Ok(factory::<H>(rt, realm, global.name)?.into());
    }
    Ok(interface.object.into())
}

/// Keeps the function of a legacy factory function (`Image`) of `realm`
/// in the glue state (traced from here on); the global object gets it as
/// a lazy property ([`global_value`]) or with [`expose_factory`].
pub fn keep_factory<H: Host>(
    rt: &mut Runtime,
    realm: u32,
    name: &'static str,
    function: Gc<Object>,
) -> VmResult<()> {
    let factories = &mut glue_state::<H>(rt)?.factories;
    factories.retain(|(key, _)| *key != (realm, name));
    factories.push(((realm, name), function));
    Ok(())
}

/// The function of the legacy factory function `name` of `realm` (its
/// interface exists).
fn factory<H: Host>(rt: &mut Runtime, realm: u32, name: &str) -> VmResult<Gc<Object>> {
    glue_state::<H>(rt)?
        .factories
        .iter()
        .find(|((r, n), _)| *r == realm && *n == name)
        .map(|&(_, function)| function)
        .ok_or(VmError::type_error("a legacy factory function is missing"))
}

/// Defines the legacy factory function `name` of an interface that is
/// created with the runtime as a property of the global object of `realm`
/// (Web IDL §3.7.2: writable, not enumerable, configurable).
pub fn expose_factory<H: Host>(rt: &mut Runtime, realm: u32, name: &str) -> VmResult<()> {
    let function = factory::<H>(rt, realm, name)?;
    let global = rt.global(realm)?;
    rt.define_value(global, name, function.into(), Attributes::HIDDEN)
}

/// The built-in objects of a realm that the interfaces use (Web IDL names
/// the intrinsics: `%Error.prototype%` for `DOMException`, the functions
/// of `%Array.prototype%` for iterable declarations, `%Iterator.prototype%`
/// for pair iterators). They are read from the global object before any
/// script runs ([`keep_builtins`]): an interface is created at its first
/// use, when a script may have replaced the global properties.
#[derive(Clone, Copy, Debug)]
pub struct Builtins {
    /// `%Error.prototype%`.
    pub error_prototype: Gc<Object>,
    /// `%Iterator.prototype%`.
    pub iterator_prototype: Gc<Object>,
    /// `%Array.prototype.entries%`.
    pub array_entries: Value,
    /// `%Array.prototype.keys%`.
    pub array_keys: Value,
    /// `%Array.prototype.values%`.
    pub array_values: Value,
    /// `%Array.prototype.forEach%`.
    pub array_for_each: Value,
}

impl Builtins {
    /// Reports the objects.
    pub(crate) fn trace(&self, tracer: &mut swb_js::Tracer<'_>) {
        tracer.object(self.error_prototype);
        tracer.object(self.iterator_prototype);
        for value in [
            self.array_entries,
            self.array_keys,
            self.array_values,
            self.array_for_each,
        ] {
            tracer.value(value);
        }
    }
}

/// Reads the [`Builtins`] of `realm` from its global object and keeps them
/// in the glue state. Call it before any script runs.
pub fn keep_builtins<H: Host>(rt: &mut Runtime, realm: u32) -> VmResult<()> {
    let error_prototype = global_prototype(rt, realm, "Error")?;
    let iterator_prototype = global_prototype(rt, realm, "Iterator")?;
    let array = global_prototype(rt, realm, "Array")?;
    let mut functions = [Value::Undefined; 4];
    for (slot, name) in functions
        .iter_mut()
        .zip(["entries", "keys", "values", "forEach"])
    {
        let key = rt.key_from_str(name)?;
        *slot = rt.get_key(array.into(), key)?;
    }
    let [array_entries, array_keys, array_values, array_for_each] = functions;
    let builtins = Builtins {
        error_prototype,
        iterator_prototype,
        array_entries,
        array_keys,
        array_values,
        array_for_each,
    };
    let state = glue_state::<H>(rt)?;
    state.builtins.retain(|(r, _)| *r != realm);
    state.builtins.push((realm, builtins));
    Ok(())
}

/// The [`Builtins`] of `realm` ([`keep_builtins`] read them).
pub fn builtins<H: Host>(rt: &mut Runtime, realm: u32) -> VmResult<Builtins> {
    glue_state::<H>(rt)?
        .builtins
        .iter()
        .find(|(r, _)| *r == realm)
        .map(|&(_, builtins)| builtins)
        .ok_or(VmError::type_error(
            "the built-in objects of the realm are missing",
        ))
}

/// `C.prototype` of the global constructor `C` of `realm`.
fn global_prototype(rt: &mut Runtime, realm: u32, name: &str) -> VmResult<Gc<Object>> {
    let global = rt.global(realm)?;
    let key = rt.key_from_str(name)?;
    let constructor = rt.get_key(global.into(), key)?;
    let key = rt.key_from_str("prototype")?;
    match rt.get_key(constructor, key)? {
        Value::Object(prototype) => Ok(prototype),
        _ => Err(VmError::type_error("a built-in prototype is missing")),
    }
}

/// Makes the prototypes of `classes` in `realm` immutable prototype
/// objects (Web IDL §3.7.3: a `[Global]` interface and its ancestors).
pub fn immutable_prototypes(rt: &mut Runtime, realm: u32, classes: &[ClassId]) -> VmResult<()> {
    for &class in classes {
        if let Some(interface) = rt.interface(realm, class) {
            rt.set_immutable_prototype(interface.prototype)?;
        }
    }
    Ok(())
}

/// Makes `%Error.prototype%` of `realm` the `[[Prototype]]` of the
/// interface prototype object (`DOMException`, Web IDL §3.14.1), as the
/// realm had it before any script ran ([`Builtins`]).
pub fn inherit_error_prototype<H: Host>(
    rt: &mut Runtime,
    realm: u32,
    interface: Interface,
) -> VmResult<()> {
    let proto = builtins::<H>(rt, realm)?.error_prototype;
    rt.heap_mut()
        .set_prototype_of(interface.prototype, Some(proto))
        .map_err(VmError::from)?;
    Ok(())
}

/// A new template for the `[LegacyUnforgeable]` members of `class` in
/// `realm`, kept in the glue state (so it is traced from here on), with
/// the members of the parent's template (the same accessor functions);
/// the glue defines the class's own members on it.
pub fn new_unforgeable_template<H: Host>(
    rt: &mut Runtime,
    realm: u32,
    class: ClassId,
    parent: Option<ClassId>,
) -> VmResult<Gc<Object>> {
    let template = rt.new_plain_object(realm)?;
    // A creation that failed may have left a template (the engine forgets
    // its interfaces, ADR 0028 section 6, item 9): the new one replaces it.
    let templates = &mut glue_state::<H>(rt)?.unforgeable;
    templates.retain(|(key, _)| *key != (realm, class));
    templates.push(((realm, class), template));
    if let Some(parent) = parent
        && let Some(source) = unforgeable_template::<H>(rt, realm, parent)?
    {
        copy_properties(rt, source, template)?;
    }
    Ok(template)
}

/// The template of `class` in `realm`, if the class has
/// `[LegacyUnforgeable]` members.
fn unforgeable_template<H: Host>(
    rt: &mut Runtime,
    realm: u32,
    class: ClassId,
) -> VmResult<Option<Gc<Object>>> {
    Ok(glue_state::<H>(rt)?
        .unforgeable
        .iter()
        .find(|(key, _)| *key == (realm, class))
        .map(|&(_, template)| template))
}

/// Defines the `[LegacyUnforgeable]` members of `class` (and of its
/// ancestors) on a new instance `object` (Web IDL §3.7.8, "unforgeable"
/// properties are own properties of each instance). Feature 4c calls it
/// for each new node wrapper; [`super::Cx::new_instance`] calls it for
/// constructed objects.
pub fn install_unforgeables<H: Host>(
    rt: &mut Runtime,
    realm: u32,
    class: ClassId,
    object: Gc<Object>,
) -> VmResult<()> {
    let mut current = Some(class);
    while let Some(class) = current {
        if let Some(template) = unforgeable_template::<H>(rt, realm, class)? {
            return copy_properties(rt, template, object);
        }
        current = rt.class_parent(class);
    }
    Ok(())
}

/// Copies the own properties of `from` to `to` (the same values and
/// accessor functions).
fn copy_properties(rt: &mut Runtime, from: Gc<Object>, to: Gc<Object>) -> VmResult<()> {
    for key in rt.own_keys_of(from)? {
        let desc = match rt.own_property_of(from, key)? {
            Some(Property::Accessor {
                get,
                set,
                enumerable,
                configurable,
            }) => PropertyDescriptor::accessor(get, set, enumerable, configurable),
            Some(Property::Data {
                value,
                writable,
                enumerable,
                configurable,
            }) => PropertyDescriptor::data(value, writable, enumerable, configurable),
            None => continue,
        };
        rt.define_property(to, key, desc)?;
    }
    Ok(())
}

/// The setter of a `[Replaceable]` attribute (Web IDL §3.7.6): defines an
/// own data property `name` of `this` with the value.
pub fn replace(rt: &mut Runtime, call: &NativeCall, this: Value, name: &str) -> Result<Value> {
    let Value::Object(object) = this else {
        return Err(VmError::type_error("Illegal invocation").into());
    };
    let value = rt.arg(call, 0);
    let key = rt.key_from_str(name)?;
    rt.create_data_property(object, key, value)?;
    Ok(Value::Undefined)
}

/// The setter of a `[PutForwards=forward]` attribute (Web IDL §3.7.6):
/// `Set(Get(this, name), forward, V, false)`.
pub fn put_forwards(
    rt: &mut Runtime,
    call: &NativeCall,
    this: Value,
    name: &str,
    forward: &str,
) -> Result<Value> {
    let value = rt.arg(call, 0);
    let key = rt.key_from_str(name)?;
    let target = rt.get_key(this, key)?;
    if !matches!(target, Value::Object(_)) {
        return Err(Exception::type_error(format!(
            "The '{name}' attribute is not an object."
        )));
    }
    let key = rt.key_from_str(forward)?;
    rt.set_key(target, key, value, false)?;
    Ok(Value::Undefined)
}

/// "Call a user object's operation" (Web IDL §3.11): calls `object` with
/// `this` if it is callable, else its method `name` with `this` = the
/// object. A method that is not callable is a `TypeError`.
pub fn call_operation(
    rt: &mut Runtime,
    object: Gc<Object>,
    name: &str,
    this: Value,
    args: &[Value],
) -> Result<Value> {
    if rt.callable(object.into())? {
        return Ok(rt.call(object.into(), this, args)?);
    }
    let key = rt.key_from_str(name)?;
    let method = rt.get_key(object.into(), key)?;
    if !rt.callable(method)? {
        return Err(VmError::type_error(format!(
            "'{name}' of the callback object is not a function"
        ))
        .into());
    }
    Ok(rt.call(method, object.into(), args)?)
}
