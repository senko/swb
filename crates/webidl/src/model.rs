//! The checked model of a set of IDL files: partial interfaces and
//! dictionaries merged, mixins included, typedefs resolved, names and
//! extended attributes checked, and the interfaces in the order of their
//! class numbers (parents first).

use std::collections::{HashMap, HashSet};

use crate::Error;
use crate::ast::{
    Argument, CallbackFunction, Definition, Dictionary, DictionaryMember, Enum, ExtAttrs, ExtValue,
    Interface, Member, Place, Type, TypeKind,
};

/// The extended attributes that the generator knows, per construct, with
/// the ones it ignores on purpose: `[CEReactions]` (no custom elements),
/// `[SameObject]` and `[NewObject]` (the hand-written code keeps or makes
/// the object), `[Serializable]` and `[Transferable]` (no
/// `structuredClone` yet), `[SecureContext]` (swb exposes these members
/// everywhere).
const INTERFACE_ATTRS: &[&str] = &[
    "Exposed",
    "Global",
    "LegacyOverrideBuiltIns",
    "LegacyUnenumerableNamedProperties",
    "SwbEnumerableNamedProperties",
    "Serializable",
    "Transferable",
    "SecureContext",
];
const ATTRIBUTE_ATTRS: &[&str] = &[
    "CEReactions",
    "SameObject",
    "NewObject",
    "LegacyUnforgeable",
    "Replaceable",
    "PutForwards",
    "Reflect",
    "ReflectSetter",
    "ReflectURL",
    "ReflectNonNegative",
    "ReflectPositive",
    "ReflectPositiveWithFallback",
    "ReflectRange",
    "ReflectDefault",
    "SwbKeywords",
    "SwbMissing",
    "SwbInvalid",
    "SwbCustom",
    "Exposed",
    "SecureContext",
];
const OPERATION_ATTRS: &[&str] = &[
    "HTMLConstructor",
    "CEReactions",
    "NewObject",
    "LegacyUnforgeable",
    "SwbCustom",
    "Exposed",
    "SecureContext",
];
const TYPE_ATTRS: &[&str] = &["Clamp", "EnforceRange", "LegacyNullToEmptyString"];

/// What a name stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Interface,
    Dictionary,
    Enum,
    Callback,
    CallbackInterface,
}

/// An interface (or callback interface) with its partials and mixins
/// merged.
#[derive(Clone, Debug)]
pub(crate) struct Iface {
    pub(crate) name: String,
    pub(crate) parent: Option<String>,
    pub(crate) ext_attrs: ExtAttrs,
    pub(crate) members: Vec<Member>,
    pub(crate) place: Place,
}

/// The checked model.
pub(crate) struct Model {
    /// The interfaces in class order.
    pub(crate) interfaces: Vec<Iface>,
    pub(crate) callback_interfaces: Vec<Iface>,
    pub(crate) dictionaries: Vec<Dictionary>,
    pub(crate) enums: Vec<Enum>,
    pub(crate) callbacks: Vec<CallbackFunction>,
    typedefs: HashMap<String, Type>,
    kinds: HashMap<String, Kind>,
}

fn error<T>(place: &Place, message: impl Into<String>) -> Result<T, Error> {
    Err(Error {
        file: place.file.clone(),
        line: place.line,
        message: message.into(),
    })
}

impl Model {
    /// Builds and checks the model of the definitions of all files.
    pub(crate) fn new(definitions: Vec<Definition>) -> Result<Model, Error> {
        let mut interfaces: Vec<Iface> = Vec::new();
        let mut mixins: Vec<Iface> = Vec::new();
        let mut callback_interfaces = Vec::new();
        let mut dictionaries: Vec<Dictionary> = Vec::new();
        let mut enums = Vec::new();
        let mut callbacks = Vec::new();
        let mut typedefs = HashMap::new();
        let mut typedef_places = Vec::new();
        let mut includes = Vec::new();
        let mut partials = Vec::new();
        let mut partial_dicts = Vec::new();
        let mut kinds: HashMap<String, Kind> = HashMap::new();
        let mut declare = |name: &str, kind: Kind, place: &Place| -> Result<(), Error> {
            if kinds.insert(name.to_owned(), kind).is_some() {
                return error(place, format!("'{name}' is defined twice"));
            }
            Ok(())
        };
        for definition in definitions {
            match definition {
                Definition::Interface(i) if i.partial => partials.push(i),
                Definition::Interface(i) => {
                    let iface = Iface {
                        name: i.name.clone(),
                        parent: i.inherits.clone(),
                        ext_attrs: i.ext_attrs.clone(),
                        members: i.members.clone(),
                        place: i.place.clone(),
                    };
                    if i.mixin {
                        if mixins.iter().any(|m| m.name == i.name) {
                            return error(&i.place, format!("'{}' is defined twice", i.name));
                        }
                        mixins.push(iface);
                    } else if i.callback {
                        declare(&i.name, Kind::CallbackInterface, &i.place)?;
                        callback_interfaces.push(iface);
                    } else {
                        declare(&i.name, Kind::Interface, &i.place)?;
                        interfaces.push(iface);
                    }
                }
                Definition::Dictionary(d) if d.partial => partial_dicts.push(d),
                Definition::Dictionary(d) => {
                    declare(&d.name, Kind::Dictionary, &d.place)?;
                    dictionaries.push(d);
                }
                Definition::Enum(e) => {
                    declare(&e.name, Kind::Enum, &e.place)?;
                    enums.push(e);
                }
                Definition::Callback(c) => {
                    declare(&c.name, Kind::Callback, &c.place)?;
                    callbacks.push(c);
                }
                Definition::Typedef(t) => {
                    if typedefs.insert(t.name.clone(), t.ty.clone()).is_some() {
                        return error(&t.place, format!("'{}' is defined twice", t.name));
                    }
                    typedef_places.push((t.name, t.place));
                }
                Definition::Includes(i) => includes.push(i),
            }
        }
        merge(&mut interfaces, &mut mixins, partials, includes)?;
        for partial in partial_dicts {
            let Some(target) = dictionaries.iter_mut().find(|d| d.name == partial.name) else {
                return error(
                    &partial.place,
                    format!(
                        "partial dictionary of an unknown dictionary '{}'",
                        partial.name
                    ),
                );
            };
            target.members.extend(partial.members);
        }
        check_cycles(&dictionaries, &typedefs, &typedef_places)?;
        let interfaces = class_order(interfaces)?;
        let model = Model {
            interfaces,
            callback_interfaces,
            dictionaries,
            enums,
            callbacks,
            typedefs,
            kinds,
        };
        model.check()?;
        Ok(model)
    }
}

/// Fails for a dictionary that inherits from itself and for a typedef
/// that refers to itself, directly or through other typedefs: the
/// generator would follow them without end.
fn check_cycles(
    dictionaries: &[Dictionary],
    typedefs: &HashMap<String, Type>,
    typedef_places: &[(String, Place)],
) -> Result<(), Error> {
    for dict in dictionaries {
        let mut parent = dict.inherits.as_deref();
        // A chain without a cycle has at most one link per dictionary;
        // a cycle that does not pass through `dict` is found from its
        // own members.
        for _ in 0..dictionaries.len() {
            let Some(name) = parent else { break };
            if name == dict.name {
                return error(
                    &dict.place,
                    format!("the inheritance of '{}' has a cycle", dict.name),
                );
            }
            parent = dictionaries
                .iter()
                .find(|d| d.name == name)
                .and_then(|d| d.inherits.as_deref());
        }
    }
    for (name, place) in typedef_places {
        let mut seen = HashSet::new();
        let mut pending: Vec<&Type> = typedefs.get(name).into_iter().collect();
        while let Some(ty) = pending.pop() {
            match &ty.kind {
                TypeKind::Named(other) => {
                    if other == name {
                        return error(place, format!("the typedef '{name}' refers to itself"));
                    }
                    if let Some(inner) = typedefs.get(other)
                        && seen.insert(other.as_str())
                    {
                        pending.push(inner);
                    }
                }
                TypeKind::Sequence(inner)
                | TypeKind::FrozenArray(inner)
                | TypeKind::Promise(inner) => {
                    pending.push(inner);
                }
                TypeKind::Record(key, value) => pending.extend([&**key, &**value]),
                TypeKind::Union(members) => pending.extend(members),
                _ => {}
            }
        }
    }
    Ok(())
}

/// Merges the partial interfaces into their interfaces (or mixins), then
/// the members of the mixins into the interfaces that include them.
fn merge(
    interfaces: &mut [Iface],
    mixins: &mut [Iface],
    partials: Vec<Interface>,
    includes: Vec<crate::ast::Includes>,
) -> Result<(), Error> {
    {
        for partial in partials {
            let target = if partial.mixin {
                mixins.iter_mut().find(|m| m.name == partial.name)
            } else {
                interfaces.iter_mut().find(|i| i.name == partial.name)
            };
            let Some(target) = target else {
                return error(
                    &partial.place,
                    format!(
                        "partial interface of an unknown interface '{}'",
                        partial.name
                    ),
                );
            };
            target.members.extend(partial.members);
        }
        for include in includes {
            let Some(mixin) = mixins.iter().find(|m| m.name == include.mixin) else {
                return error(&include.place, format!("unknown mixin '{}'", include.mixin));
            };
            let members = mixin.members.clone();
            let Some(target) = interfaces.iter_mut().find(|i| i.name == include.target) else {
                return error(
                    &include.place,
                    format!("unknown interface '{}'", include.target),
                );
            };
            target.members.extend(members);
        }
    }
    Ok(())
}

impl Model {
    /// What `name` stands for.
    pub(crate) fn kind(&self, name: &str) -> Option<Kind> {
        self.kinds.get(name).copied()
    }

    /// The interface named `name`.
    pub(crate) fn iface(&self, name: &str) -> Option<&Iface> {
        self.interfaces.iter().find(|i| i.name == name)
    }

    /// The dictionary named `name`.
    pub(crate) fn dict(&self, name: &str) -> Option<&Dictionary> {
        self.dictionaries.iter().find(|d| d.name == name)
    }

    /// The enumeration named `name`.
    pub(crate) fn enumeration(&self, name: &str) -> Option<&Enum> {
        self.enums.iter().find(|e| e.name == name)
    }

    /// Whether the interface `name` is `Node` or inherits from it (its
    /// wrappers carry a node).
    pub(crate) fn is_node(&self, name: &str) -> bool {
        let mut current = Some(name.to_owned());
        while let Some(name) = current {
            if name == "Node" {
                return true;
            }
            current = self.iface(&name).and_then(|i| i.parent.clone());
        }
        false
    }

    /// The members of a dictionary in conversion order: the inherited
    /// dictionaries first, each in lexicographic order of the member
    /// names, with the dictionary that declares each member.
    pub(crate) fn dict_members(&self, name: &str) -> Vec<(String, DictionaryMember)> {
        let mut chain = Vec::new();
        let mut current = self.dict(name);
        while let Some(dict) = current {
            chain.push(dict);
            current = dict.inherits.as_deref().and_then(|p| self.dict(p));
        }
        let mut members = Vec::new();
        for dict in chain.into_iter().rev() {
            let mut own = dict.members.clone();
            own.sort_by(|a, b| a.name.cmp(&b.name));
            members.extend(own.into_iter().map(|m| (dict.name.clone(), m)));
        }
        members
    }

    /// A type with its typedefs replaced (recursively), the nullability
    /// and the extended attributes of the typedef's use kept.
    pub(crate) fn resolve(&self, ty: &Type) -> Type {
        let mut resolved = match &ty.kind {
            TypeKind::Named(name) if self.typedefs.contains_key(name) => {
                let mut inner = self.resolve(&self.typedefs[name]);
                inner.nullable |= ty.nullable;
                inner.ext_attrs.0.extend(ty.ext_attrs.0.iter().cloned());
                return inner;
            }
            _ => ty.clone(),
        };
        resolved.kind = match &ty.kind {
            TypeKind::Sequence(inner) => TypeKind::Sequence(Box::new(self.resolve(inner))),
            TypeKind::FrozenArray(inner) => TypeKind::FrozenArray(Box::new(self.resolve(inner))),
            TypeKind::Promise(inner) => TypeKind::Promise(Box::new(self.resolve(inner))),
            TypeKind::Record(k, v) => {
                TypeKind::Record(Box::new(self.resolve(k)), Box::new(self.resolve(v)))
            }
            TypeKind::Union(members) => {
                TypeKind::Union(members.iter().map(|m| self.resolve(m)).collect())
            }
            other => other.clone(),
        };
        resolved
    }

    // --- Checks ---

    fn check(&self) -> Result<(), Error> {
        for iface in self.interfaces.iter().chain(&self.callback_interfaces) {
            check_attrs(&iface.ext_attrs, INTERFACE_ATTRS, &iface.place, "interface")?;
            for member in &iface.members {
                self.check_member(iface, member)?;
            }
        }
        for iface in &self.interfaces {
            self.check_legacy(iface, false)?;
        }
        for iface in &self.callback_interfaces {
            self.check_legacy(iface, true)?;
        }
        for dict in &self.dictionaries {
            if let Some(parent) = &dict.inherits
                && self.kind(parent) != Some(Kind::Dictionary)
            {
                return error(
                    &dict.place,
                    format!(
                        "'{}' inherits from an unknown dictionary '{parent}'",
                        dict.name
                    ),
                );
            }
            check_attrs(&dict.ext_attrs, &[], &dict.place, "dictionary")?;
            for member in &dict.members {
                check_attrs(&member.ext_attrs, &[], &member.place, "dictionary member")?;
                self.check_type(&member.ty, &member.place)?;
            }
        }
        for callback in &self.callbacks {
            check_attrs(&callback.ext_attrs, &[], &callback.place, "callback")?;
            self.check_type(&callback.ret, &callback.place)?;
            self.check_args(&callback.args, &callback.place)?;
        }
        Ok(())
    }

    fn check_member(&self, iface: &Iface, member: &Member) -> Result<(), Error> {
        match member {
            Member::Const(c) => {
                check_attrs(&c.ext_attrs, &[], &c.place, "constant")?;
                self.check_type(&c.ty, &c.place)
            }
            Member::Attribute(a) => {
                check_attrs(&a.ext_attrs, ATTRIBUTE_ATTRS, &a.place, "attribute")?;
                if a.is_static {
                    return error(&a.place, "static attributes are not generated yet");
                }
                self.check_type(&a.ty, &a.place)
            }
            Member::Operation(op) | Member::Constructor(op) => {
                check_attrs(&op.ext_attrs, OPERATION_ATTRS, &op.place, "operation")?;
                if iface.ext_attrs.has("Global") && op.is_static {
                    return error(
                        &op.place,
                        "static operations of a [Global] interface are not generated",
                    );
                }
                self.check_type(&op.ret, &op.place)?;
                self.check_args(&op.args, &op.place)
            }
            // `legacy.rs` checks iterable declarations.
            Member::Stringifier(_) | Member::Iterable(_) => Ok(()),
        }
    }

    fn check_args(&self, args: &[Argument], place: &Place) -> Result<(), Error> {
        let mut optional = false;
        for (i, arg) in args.iter().enumerate() {
            check_attrs(&arg.ext_attrs, TYPE_ATTRS, place, "argument")?;
            self.check_type(&arg.ty, place)?;
            if arg.variadic && i + 1 != args.len() {
                return error(
                    place,
                    format!("the variadic argument '{}' is not the last", arg.name),
                );
            }
            if optional && !arg.optional && !arg.variadic {
                return error(
                    place,
                    format!("the argument '{}' follows an optional argument", arg.name),
                );
            }
            optional |= arg.optional;
        }
        Ok(())
    }

    pub(crate) fn check_type(&self, ty: &Type, place: &Place) -> Result<(), Error> {
        check_attrs(&ty.ext_attrs, TYPE_ATTRS, place, "type")?;
        match &ty.kind {
            TypeKind::Named(name) => {
                if !self.kinds.contains_key(name) && !self.typedefs.contains_key(name) {
                    return error(place, format!("unknown type '{name}'"));
                }
                Ok(())
            }
            TypeKind::BigInt | TypeKind::Symbol => error(
                place,
                "the types bigint and symbol are not in the WebIDL subset of swb",
            ),
            TypeKind::Sequence(inner) | TypeKind::FrozenArray(inner) | TypeKind::Promise(inner) => {
                self.check_type(inner, place)
            }
            TypeKind::Record(key, value) => {
                if !matches!(
                    key.kind,
                    TypeKind::DomString | TypeKind::UsvString | TypeKind::ByteString
                ) {
                    return error(place, "the key type of a record must be a string type");
                }
                self.check_type(value, place)
            }
            TypeKind::Union(members) => {
                for member in members {
                    self.check_type(member, place)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// Fails for an extended attribute that is not in `allowed`.
fn check_attrs(attrs: &ExtAttrs, allowed: &[&str], place: &Place, what: &str) -> Result<(), Error> {
    for attr in &attrs.0 {
        if attr.name == "LegacyFactoryFunction" {
            return error(&attr.place, "[LegacyFactoryFunction] is not generated yet");
        }
        if !allowed.contains(&attr.name.as_str()) {
            return error(
                &attr.place,
                format!(
                    "the extended attribute [{}] is not supported on a {what}",
                    attr.name
                ),
            );
        }
    }
    let _ = place;
    Ok(())
}

/// Whether an interface (by its `[Exposed]`) is exposed in `Window`.
pub(crate) fn exposed_in_window(attrs: &ExtAttrs) -> bool {
    match attrs.get("Exposed").map(|a| &a.value) {
        None => true,
        Some(ExtValue::Ident(name)) => name == "Window" || name == "*",
        Some(ExtValue::List(items)) => items
            .iter()
            .any(|i| matches!(i, ExtValue::Ident(n) if n == "Window" || n == "*")),
        Some(_) => false,
    }
}

/// Orders the interfaces so that each parent comes before its children,
/// otherwise in definition order.
fn class_order(interfaces: Vec<Iface>) -> Result<Vec<Iface>, Error> {
    let mut ordered: Vec<Iface> = Vec::with_capacity(interfaces.len());
    let mut pending = interfaces;
    while !pending.is_empty() {
        let before = pending.len();
        let mut rest = Vec::new();
        for iface in pending {
            let ready = match &iface.parent {
                None => true,
                Some(parent) => ordered.iter().any(|i| &i.name == parent),
            };
            if ready {
                ordered.push(iface);
            } else {
                rest.push(iface);
            }
        }
        if rest.len() == before {
            let iface = &rest[0];
            let parent = iface.parent.clone().unwrap_or_default();
            let known = rest.iter().any(|i| i.name == parent);
            let message = if known {
                format!("the inheritance of '{}' has a cycle", iface.name)
            } else {
                format!(
                    "'{}' inherits from an unknown interface '{parent}'",
                    iface.name
                )
            };
            return error(&iface.place, message);
        }
        pending = rest;
    }
    Ok(ordered)
}
