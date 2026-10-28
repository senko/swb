//! Indexed and named properties (Web IDL §2.5.6 "Special operations",
//! §3.9 "Legacy platform objects") and `iterable` declarations (§2.5.9):
//! which special operations an interface has, counting the inherited ones
//! (the derived-most declaration wins, §3.9), and the checks of the
//! declarations.

use crate::Error;
use crate::ast::{IntegerType, Iterable, Member, Operation, Place, Special, TypeKind};
use crate::model::{Iface, Model};

/// The key of a special operation: an index (`unsigned long`) or a name
/// (`DOMString`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    Indexed,
    Named,
}

/// A special operation and the interface that declares it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SpecialOp<'a> {
    pub(crate) op: &'a Operation,
    pub(crate) owner: &'a Iface,
}

/// The special operations that apply to the instances of an interface, and
/// the extended attributes of its named properties.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Legacy<'a> {
    pub(crate) indexed_getter: Option<SpecialOp<'a>>,
    pub(crate) indexed_setter: Option<SpecialOp<'a>>,
    pub(crate) named_getter: Option<SpecialOp<'a>>,
    pub(crate) named_setter: Option<SpecialOp<'a>>,
    pub(crate) named_deleter: Option<SpecialOp<'a>>,
    /// `[LegacyOverrideBuiltIns]` on the interface or an ancestor.
    pub(crate) override_builtins: bool,
    /// `[LegacyUnenumerableNamedProperties]` on the interface or an
    /// ancestor.
    pub(crate) unenumerable: bool,
    /// `[SwbPropertyNames]` (swb's, for `CSSStyleDeclaration`): the named
    /// properties behave like attributes of the prototype, as in Chromium:
    /// a write of a name that is not supported makes an ordinary property,
    /// and a deletion of a supported name succeeds and changes nothing.
    pub(crate) property_names: bool,
}

impl Legacy<'_> {
    /// Whether the instances are legacy platform objects (they support
    /// indexed or named properties).
    pub(crate) fn is_legacy(&self) -> bool {
        self.indexed_getter.is_some() || self.named_getter.is_some()
    }
}

/// The `iterable` declaration of the interface itself.
pub(crate) fn iterable(iface: &Iface) -> Option<&Iterable> {
    iface.members.iter().find_map(|m| match m {
        Member::Iterable(it) => Some(it),
        _ => None,
    })
}

/// The key of a special operation, from the type of its first argument.
pub(crate) fn key_of(model: &Model, op: &Operation) -> Option<Key> {
    let first = op.args.first()?;
    match model.resolve(&first.ty).kind {
        TypeKind::Integer(IntegerType::UnsignedLong) => Some(Key::Indexed),
        TypeKind::DomString => Some(Key::Named),
        _ => None,
    }
}

impl Model {
    /// The interface and its ancestors, the interface first.
    pub(crate) fn chain<'a>(&'a self, iface: &'a Iface) -> Vec<&'a Iface> {
        let mut chain = vec![iface];
        let mut current = iface.parent.as_deref().and_then(|p| self.iface(p));
        while let Some(parent) = current {
            if chain.iter().any(|i| i.name == parent.name) {
                break;
            }
            chain.push(parent);
            current = parent.parent.as_deref().and_then(|p| self.iface(p));
        }
        chain
    }

    /// The special operations of the instances of `iface`.
    pub(crate) fn legacy<'a>(&'a self, iface: &'a Iface) -> Legacy<'a> {
        // The derived-most declaration decides whether the named properties
        // are enumerable; `[SwbEnumerableNamedProperties]` undoes the
        // inherited `[LegacyUnenumerableNamedProperties]` (Chromium's
        // `HTMLFormControlsCollection`).
        let unenumerable = self
            .chain(iface)
            .iter()
            .find_map(|owner| {
                if owner.ext_attrs.has("SwbEnumerableNamedProperties") {
                    Some(false)
                } else if owner.ext_attrs.has("LegacyUnenumerableNamedProperties") {
                    Some(true)
                } else {
                    None
                }
            })
            .unwrap_or(false);
        let mut legacy = Legacy {
            unenumerable,
            ..Legacy::default()
        };
        for owner in self.chain(iface) {
            legacy.override_builtins |= owner.ext_attrs.has("LegacyOverrideBuiltIns");
            legacy.property_names |= owner.ext_attrs.has("SwbPropertyNames");
            for member in &owner.members {
                let Member::Operation(op) = member else {
                    continue;
                };
                let (Some(special), Some(key)) = (op.special, key_of(self, op)) else {
                    continue;
                };
                let slot = match (special, key) {
                    (Special::Getter, Key::Indexed) => &mut legacy.indexed_getter,
                    (Special::Setter, Key::Indexed) => &mut legacy.indexed_setter,
                    (Special::Getter, Key::Named) => &mut legacy.named_getter,
                    (Special::Setter, Key::Named) => &mut legacy.named_setter,
                    (Special::Deleter, Key::Named) => &mut legacy.named_deleter,
                    (Special::Deleter, Key::Indexed) => continue,
                };
                if slot.is_none() {
                    *slot = Some(SpecialOp { op, owner });
                }
            }
        }
        legacy
    }

    /// Whether the interface itself declares an indexed property getter
    /// (its prototype gets `@@iterator` = `%Array.prototype.values%`,
    /// Web IDL §3.7.9).
    pub(crate) fn declares_indexed_getter(&self, iface: &Iface) -> bool {
        iface.members.iter().any(|m| {
            matches!(m, Member::Operation(op)
                if op.special == Some(Special::Getter) && key_of(self, op) == Some(Key::Indexed))
        })
    }

    /// The checks of the special operations and of the `iterable`
    /// declaration of an interface.
    pub(crate) fn check_legacy(&self, iface: &Iface, callback: bool) -> Result<(), Error> {
        let mut seen: Vec<(Special, Key)> = Vec::new();
        let mut iterables = 0;
        for member in &iface.members {
            match member {
                Member::Operation(op) if op.special.is_some() => {
                    let special = op.special.unwrap_or(Special::Getter);
                    if callback {
                        return fail(&op.place, "a callback interface has no special operations");
                    }
                    let key = self.check_special(iface, op, special)?;
                    if seen.contains(&(special, key)) {
                        return fail(
                            &op.place,
                            format!("'{}' declares this special operation twice", iface.name),
                        );
                    }
                    seen.push((special, key));
                }
                Member::Iterable(it) => {
                    iterables += 1;
                    if iterables > 1 || callback {
                        return fail(
                            &it.place,
                            "an interface has at most one iterable declaration",
                        );
                    }
                    self.check_iterable(iface, it)?;
                }
                _ => {}
            }
        }
        let legacy = self.legacy(iface);
        for name in [
            "LegacyOverrideBuiltIns",
            "LegacyUnenumerableNamedProperties",
            "SwbPropertyNames",
        ] {
            if let Some(attr) = iface.ext_attrs.get(name)
                && legacy.named_getter.is_none()
            {
                return fail(
                    &attr.place,
                    format!("[{name}] needs a named property getter"),
                );
            }
        }
        if legacy.indexed_setter.is_some() && legacy.indexed_getter.is_none() {
            return fail(
                &iface.place,
                "an indexed property setter needs an indexed property getter",
            );
        }
        if (legacy.named_setter.is_some() || legacy.named_deleter.is_some())
            && legacy.named_getter.is_none()
        {
            return fail(
                &iface.place,
                "a named property setter or deleter needs a named property getter",
            );
        }
        Ok(())
    }

    /// The checks of one special operation (§2.5.6); its key.
    fn check_special(&self, iface: &Iface, op: &Operation, special: Special) -> Result<Key, Error> {
        if iface.ext_attrs.has("Global") {
            return fail(
                &op.place,
                "special operations of a [Global] interface (the named properties object) are not generated yet",
            );
        }
        if op.is_static || op.ext_attrs.has("SwbCustom") {
            return fail(
                &op.place,
                "a special operation cannot be static or [SwbCustom]",
            );
        }
        let Some(key) = key_of(self, op) else {
            return fail(
                &op.place,
                "the first argument of a special operation must be unsigned long (indexed) or DOMString (named)",
            );
        };
        let count = match special {
            Special::Getter | Special::Deleter => 1,
            Special::Setter => 2,
        };
        if op.args.len() != count || op.args.iter().any(|a| a.optional || a.variadic) {
            return fail(
                &op.place,
                format!("this special operation takes {count} required argument(s)"),
            );
        }
        if special == Special::Deleter && key == Key::Indexed {
            return fail(
                &op.place,
                "indexed property deleters do not exist in Web IDL",
            );
        }
        let ret = self.resolve(&op.ret).kind;
        match special {
            Special::Getter if ret == TypeKind::Undefined => {
                return fail(&op.place, "a getter must return a value");
            }
            Special::Setter if ret != TypeKind::Undefined => {
                return fail(&op.place, "a setter must return undefined");
            }
            Special::Deleter if !matches!(ret, TypeKind::Undefined | TypeKind::Boolean) => {
                return fail(&op.place, "a deleter must return undefined or boolean");
            }
            _ => {}
        }
        if let Some(name) = &op.name {
            let overloads = iface
                .members
                .iter()
                .filter(|m| matches!(m, Member::Operation(o) if o.name.as_ref() == Some(name)))
                .count();
            if overloads > 1 {
                return fail(
                    &op.place,
                    format!("the special operation '{name}' cannot be overloaded"),
                );
            }
        }
        Ok(key)
    }

    /// The checks of an `iterable` declaration (§2.5.9).
    fn check_iterable(&self, iface: &Iface, it: &Iterable) -> Result<(), Error> {
        if iface.ext_attrs.has("Global") {
            return fail(&it.place, "a [Global] interface cannot be iterable");
        }
        let legacy = self.legacy(iface);
        match (&it.key, legacy.indexed_getter) {
            (None, None) => {
                return fail(
                    &it.place,
                    "a value iterator needs an indexed property getter",
                );
            }
            (None, Some(getter)) => {
                let getter_ty = self.resolve(&getter.op.ret);
                if self.resolve(&it.value).kind != getter_ty.kind {
                    return fail(
                        &it.place,
                        "the value type of a value iterator must be the type of the indexed property getter",
                    );
                }
            }
            (Some(_), Some(_)) => {
                return fail(
                    &it.place,
                    "a pair iterator cannot be declared on an interface with indexed properties",
                );
            }
            (Some(_), None) => {}
        }
        for owner in self.chain(iface) {
            for member in &owner.members {
                let name = match member {
                    Member::Attribute(a) => Some(a.name.as_str()),
                    Member::Operation(op) => op.name.as_deref(),
                    Member::Const(c) => Some(c.name.as_str()),
                    Member::Iterable(_) if !std::ptr::eq(owner, iface) => {
                        return fail(
                            &it.place,
                            format!("'{}' inherits an iterable declaration", iface.name),
                        );
                    }
                    _ => None,
                };
                if let Some(name) = name
                    && matches!(name, "entries" | "forEach" | "keys" | "values")
                {
                    return fail(
                        &it.place,
                        format!("an iterable interface cannot have a member named '{name}'"),
                    );
                }
            }
        }
        for ty in it.key.iter().chain(std::iter::once(&it.value)) {
            self.check_type(ty, &it.place)?;
        }
        Ok(())
    }
}

fn fail<T>(place: &Place, message: impl Into<String>) -> Result<T, Error> {
    Err(Error {
        file: place.file.clone(),
        line: place.line,
        message: message.into(),
    })
}
