//! The glue of indexed and named properties (Web IDL §3.9, the hooks of
//! legacy platform objects) and of `iterable` declarations (§3.7.9).
//!
//! The interface that declares a special operation gets trait functions
//! for it: `index_count` (the supported property indices are `0..n`),
//! `supported_names` and `is_supported_name` (with a default body that
//! searches the names), and for a special operation without an identifier
//! `indexed_getter`, `indexed_setter`, `named_getter`, `named_setter` or
//! `named_deleter`; one with an identifier calls the trait function of its
//! regular operation. Every interface whose instances support indexed or
//! named properties (also through inheritance: js does not inherit hooks)
//! gets a `hooks` function for its class. A pair iterator adds `pair_at`
//! (the value pairs to iterate over), the iterator class "X Iterator" and
//! the natives of `entries`, `keys`, `values`, `forEach` and `next`.

use std::fmt::Write as _;

use crate::Error;
use crate::ast::{Member, Operation, Place, Special, TypeKind};
use crate::legacy::{Key, SpecialOp};
use crate::model::Iface;

use super::Gen;
use super::names::{constant, ident, literal, snake};

impl Gen<'_> {
    /// The trait functions of the special operations that `iface`
    /// declares and of its pair iterator.
    pub(crate) fn legacy_trait_items(
        &self,
        iface: &Iface,
        trait_items: &mut String,
        claim: &mut impl FnMut(&str, &Place) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let this_ty = self.this_type(iface);
        for member in &iface.members {
            let Member::Operation(op) = member else {
                continue;
            };
            let (Some(special), Some(key)) = (op.special, crate::legacy::key_of(self.model, op))
            else {
                continue;
            };
            if special == Special::Getter {
                match key {
                    Key::Indexed => {
                        claim("index_count", &op.place)?;
                        let _ = writeln!(
                            trait_items,
                            "    /// The supported property indices of `{0}` are `0..n` (Web IDL §3.9); returns `n`.\n    fn index_count(cx: &mut b::Cx<'_>, this: {this_ty}) -> b::Result<u32>;\n\n    /// Whether `index` is a supported property index. Every indexed access\n    /// asks; the default compares with [`Self::index_count`], and a list that\n    /// cannot count cheaply overrides it.\n    fn has_index(cx: &mut b::Cx<'_>, this: {this_ty}, index: u32) -> b::Result<bool> {{\n        Ok(index < Self::index_count(cx, this)?)\n    }}\n",
                            iface.name
                        );
                    }
                    Key::Named => {
                        claim("supported_names", &op.place)?;
                        claim("is_supported_name", &op.place)?;
                        let _ = writeln!(
                            trait_items,
                            "    /// The supported property names of `{0}`, in order (Web IDL §3.9).\n    fn supported_names(cx: &mut b::Cx<'_>, this: {this_ty}) -> b::Result<Vec<String>>;\n\n    /// Whether `name` is a supported property name. Every property access\n    /// asks; the default searches [`Self::supported_names`].\n    fn is_supported_name(cx: &mut b::Cx<'_>, this: {this_ty}, name: &str) -> b::Result<bool> {{\n        Ok(Self::supported_names(cx, this)?.iter().any(|n| n == name))\n    }}\n",
                            iface.name
                        );
                    }
                }
            }
            if op.name.is_some() {
                continue;
            }
            let method = unnamed_method(special, key);
            claim(method, &op.place)?;
            let key_param = match key {
                Key::Indexed => "index: u32",
                Key::Named => "name: String",
            };
            let (params, ret) = match special {
                Special::Getter | Special::Deleter => (
                    key_param.to_owned(),
                    self.rust_type(&op.ret, "super::types::", true),
                ),
                Special::Setter => {
                    let value = op.args.get(1).map_or_else(String::new, |a| {
                        self.rust_type(&a.ty, "super::types::", false)
                    });
                    (format!("{key_param}, value: {value}"), "()".to_owned())
                }
            };
            let _ = writeln!(
                trait_items,
                "    /// `{}`: the {} property {} of `{}`.\n    fn {method}(cx: &mut b::Cx<'_>, this: {this_ty}, {params}) -> b::Result<{ret}>;\n",
                self.special_text(op),
                match key {
                    Key::Indexed => "indexed",
                    Key::Named => "named",
                },
                special_word(special),
                iface.name
            );
        }
        if let Some(it) = crate::legacy::iterable(iface)
            && let Some(key) = &it.key
        {
            claim("pair_at", &it.place)?;
            let _ = writeln!(
                trait_items,
                "    /// `iterable<{}, {}>`: the pair at `index` of the value pairs to iterate\n    /// over (Web IDL §3.7.9), or `None` past the end.\n    fn pair_at(cx: &mut b::Cx<'_>, this: {this_ty}, index: u32) -> b::Result<Option<({}, {})>>;\n",
                self.idl_text(key),
                self.idl_text(&it.value),
                self.rust_type(key, "super::types::", true),
                self.rust_type(&it.value, "super::types::", true)
            );
        }
        Ok(())
    }

    /// The IDL text of a special operation, for a doc comment.
    fn special_text(&self, op: &Operation) -> String {
        let args = op
            .args
            .iter()
            .map(|a| format!("{} {}", self.idl_text(&a.ty), a.name))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "{} {} ({args})",
            special_word(op.special.unwrap_or(Special::Getter)),
            self.idl_text(&op.ret)
        )
    }

    /// The glue of the instances of a legacy platform object interface:
    /// the description of its special operations and the hooks.
    pub(crate) fn legacy_glue(&self, iface: &Iface, glue: &mut String) {
        let legacy = self.model.legacy(iface);
        if !legacy.is_legacy() {
            return;
        }
        let bound = "H: super::Bindings";
        let mut indexed = "None".to_owned();
        if let Some(getter) = legacy.indexed_getter {
            let _ = writeln!(
                glue,
                "fn legacy_count<{bound}>(rt: &mut swb_js::Runtime, realm: u32, this: b::Platform) -> b::Result<u32> {{\n    {1}<H as {2}>::index_count({0}, this)\n}}\n\nfn legacy_has<{bound}>(rt: &mut swb_js::Runtime, realm: u32, this: b::Platform, index: u32) -> b::Result<bool> {{\n    {1}<H as {2}>::has_index({0}, this, index)\n}}\n",
                legacy_cx(getter.owner),
                self.legacy_this(getter.owner),
                trait_path(getter.owner, iface)
            );
            self.legacy_getter(
                iface,
                getter,
                "legacy_indexed_get",
                "index: u32",
                "index",
                glue,
            );
            let set = legacy.indexed_setter.map_or_else(
                || "None".to_owned(),
                |setter| {
                    self.legacy_setter(
                        iface,
                        setter,
                        "legacy_indexed_set",
                        "index: u32",
                        "index",
                        glue,
                    );
                    "Some(legacy_indexed_set::<H>)".to_owned()
                },
            );
            indexed = format!(
                "Some(b::legacy::Indexed {{ count: legacy_count::<H>, has: legacy_has::<H>, get: legacy_indexed_get::<H>, set: {set} }})"
            );
        }
        let mut named = "None".to_owned();
        if let Some(getter) = legacy.named_getter {
            let path = trait_path(getter.owner, iface);
            let _ = writeln!(
                glue,
                "fn legacy_names<{bound}>(rt: &mut swb_js::Runtime, realm: u32, this: b::Platform) -> b::Result<Vec<String>> {{\n    {0}<H as {path}>::supported_names({1}, this)\n}}\n\nfn legacy_is_supported<{bound}>(rt: &mut swb_js::Runtime, realm: u32, this: b::Platform, name: &str) -> b::Result<bool> {{\n    {0}<H as {path}>::is_supported_name({1}, this, name)\n}}\n",
                self.legacy_this(getter.owner),
                legacy_cx(getter.owner)
            );
            self.legacy_getter(
                iface,
                getter,
                "legacy_named_get",
                "name: String",
                "name",
                glue,
            );
            let set = legacy.named_setter.map_or_else(
                || "None".to_owned(),
                |setter| {
                    self.legacy_setter(
                        iface,
                        setter,
                        "legacy_named_set",
                        "name: String",
                        "name",
                        glue,
                    );
                    "Some(legacy_named_set::<H>)".to_owned()
                },
            );
            let delete = legacy.named_deleter.map_or_else(
                || "None".to_owned(),
                |deleter| {
                    self.legacy_deleter(iface, deleter, glue);
                    "Some(legacy_named_delete::<H>)".to_owned()
                },
            );
            named = format!(
                "Some(b::legacy::Named {{ names: legacy_names::<H>, is_supported: legacy_is_supported::<H>, get: legacy_named_get::<H>, set: {set}, delete: {delete} }})"
            );
        }
        let unforgeable = self
            .unforgeable_names(iface)
            .iter()
            .map(|n| literal(n))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(
            glue,
            "/// The indexed and named properties of `{0}` (Web IDL §3.9).\nfn legacy<{bound}>() -> b::legacy::Legacy {{\n    b::legacy::Legacy {{\n        interface: {1},\n        dom_exception: super::DOM_EXCEPTION,\n        indexed: {indexed},\n        named: {named},\n        override_builtins: {2},\n        unenumerable: {3},\n        property_names: {4},\n        unforgeable: &[{unforgeable}],\n    }}\n}}\n",
            iface.name,
            literal(&iface.name),
            legacy.override_builtins,
            legacy.unenumerable,
            legacy.property_names
        );
        hook_wrappers(iface, glue);
    }

    /// A getter function of the description (`name`), which converts the
    /// result.
    fn legacy_getter(
        &self,
        iface: &Iface,
        op: SpecialOp<'_>,
        name: &str,
        key_param: &str,
        key: &str,
        glue: &mut String,
    ) {
        let value = self.rust_to_js(&op.op.ret, "result", "realm", true, 0);
        let _ = writeln!(
            glue,
            "fn {name}<H: super::Bindings>(rt: &mut swb_js::Runtime, realm: u32, this: b::Platform, {key_param}) -> b::Result<swb_js::Value> {{\n    {}let result = <H as {}>::{}({}, this, {key})?;\n    Ok({value}?)\n}}\n",
            self.legacy_this(op.owner),
            trait_path(op.owner, iface),
            self.method_name(op),
            legacy_cx(op.owner)
        );
    }

    /// A setter function of the description, which converts the value
    /// (an error names it as the second argument, as Chromium does).
    fn legacy_setter(
        &self,
        iface: &Iface,
        op: SpecialOp<'_>,
        name: &str,
        key_param: &str,
        key: &str,
        glue: &mut String,
    ) {
        let convert = op.op.args.get(1).map_or_else(
            || "b::Result::Ok(swb_js::Value::Undefined)".to_owned(),
            |a| self.js_to_rust(&a.ty, "v", "super::types::", "realm", 0),
        );
        let _ = writeln!(
            glue,
            "fn {name}<H: super::Bindings>(rt: &mut swb_js::Runtime, realm: u32, this: b::Platform, {key_param}, v: swb_js::Value) -> b::Result<()> {{\n    let value = {convert}.map_err(|e| e.argument(1))?;\n    {}<H as {}>::{}({}, this, {key}, value)\n}}\n",
            self.legacy_this(op.owner),
            trait_path(op.owner, iface),
            self.method_name(op),
            legacy_cx(op.owner)
        );
    }

    /// The deleter function of the description: `false` when the deletion
    /// failed (a deleter that returns `boolean`).
    fn legacy_deleter(&self, iface: &Iface, op: SpecialOp<'_>, glue: &mut String) {
        let result = if matches!(self.model.resolve(&op.op.ret).kind, TypeKind::Boolean) {
            ""
        } else {
            ".map(|()| true)"
        };
        let _ = writeln!(
            glue,
            "fn legacy_named_delete<H: super::Bindings>(rt: &mut swb_js::Runtime, realm: u32, this: b::Platform, name: String) -> b::Result<bool> {{\n    {}<H as {}>::{}({}, this, name){result}\n}}\n",
            self.legacy_this(op.owner),
            trait_path(op.owner, iface),
            self.method_name(op),
            legacy_cx(op.owner)
        );
    }

    /// The trait method that a special operation calls.
    fn method_name(&self, op: SpecialOp<'_>) -> String {
        if let Some(name) = &op.op.name {
            return ident(name);
        }
        let key = crate::legacy::key_of(self.model, op.op).unwrap_or(Key::Indexed);
        unnamed_method(op.op.special.unwrap_or(Special::Getter), key).to_owned()
    }

    /// The conversion of `this` (a `b::Platform`) for the trait of `owner`.
    fn legacy_this(&self, owner: &Iface) -> &'static str {
        if self.model.is_node(&owner.name) {
            "let this = b::legacy::node(this)?;\n    "
        } else {
            ""
        }
    }

    /// The names of the `[LegacyUnforgeable]` members of the interface and
    /// its ancestors.
    fn unforgeable_names(&self, iface: &Iface) -> Vec<String> {
        let mut names = Vec::new();
        for owner in self.model.chain(iface) {
            for member in &owner.members {
                let name = match member {
                    Member::Attribute(a) if a.ext_attrs.has("LegacyUnforgeable") => Some(&a.name),
                    Member::Operation(op) if op.ext_attrs.has("LegacyUnforgeable") => {
                        op.name.as_ref()
                    }
                    _ => None,
                };
                if let Some(name) = name
                    && !names.contains(name)
                {
                    names.push(name.clone());
                }
            }
        }
        names
    }

    // --- Pair iterators ---

    /// The natives of a pair iterator: `entries`, `keys`, `values`,
    /// `forEach` and the `next` of its iterator prototype.
    pub(crate) fn pair_iterator_glue(&self, iface: &Iface, glue: &mut String) {
        let Some(it) = crate::legacy::iterable(iface) else {
            return;
        };
        let Some(key_ty) = &it.key else {
            return;
        };
        let name = &iface.name;
        let lit = literal(name);
        let class = constant(name);
        let iterator_class = iterator_constant(name);
        let this_conv = if self.model.is_node(name) {
            "let this = b::legacy::node(platform)?;"
        } else {
            "let this = platform;"
        };
        let key_js = self.rust_to_js(key_ty, "key", "realm", true, 0);
        let value_js = self.rust_to_js(&it.value, "value", "realm", true, 0);
        for (function, method, kind) in [
            ("op_entries", "entries", "KeyValue"),
            ("op_keys", "keys", "Key"),
            ("op_values", "values", "Value"),
        ] {
            let _ = writeln!(
                glue,
                "fn {function}<H: {name}Impl>(rt: &mut swb_js::Runtime, call: &swb_js::NativeCall) -> swb_js::VmResult<swb_js::NativeReturn> {{\n    b::glue::run(rt, call, b::Context::Operation({lit}, \"{method}\"), super::DOM_EXCEPTION, |rt| {{\n        let this = b::glue::this_platform(rt, call, super::{class})?;\n        let iterator = b::iterable::new_iterator::<H>(rt, call.realm(), super::{iterator_class}, this.object, b::iterable::Kind::{kind})?;\n        Ok(swb_js::Value::Object(iterator))\n    }})\n}}\n"
            );
        }
        let _ = writeln!(
            glue,
            "fn iterator_next<H: {name}Impl>(rt: &mut swb_js::Runtime, call: &swb_js::NativeCall) -> swb_js::VmResult<swb_js::NativeReturn> {{\n    b::glue::run(rt, call, b::Context::Operation({iterator_lit}, \"next\"), super::DOM_EXCEPTION, |rt| {{\n        let realm = call.realm();\n        let (state, platform, kind, index) = b::iterable::step(rt, call, super::{iterator_class})?;\n        {this_conv}\n        let Some((key, value)) = <H as {name}Impl>::pair_at(&mut b::Cx::new(rt, call, super::{class}), this, index)? else {{\n            return Ok(b::iterable::iterator_result(rt, realm, None)?);\n        }};\n        b::iterable::advance(rt, state)?;\n        let result = match kind {{\n            b::iterable::Kind::Key => {key_js}?,\n            b::iterable::Kind::Value => {value_js}?,\n            b::iterable::Kind::KeyValue => {{\n                let key = {key_js}?;\n                let value = {value_js}?;\n                b::iterable::entry(rt, realm, key, value)?\n            }}\n        }};\n        Ok(b::iterable::iterator_result(rt, realm, Some(result))?)\n    }})\n}}\n",
            iterator_lit = literal(&format!("{name} Iterator"))
        );
        let realm = if key_js.contains("realm") || value_js.contains("realm") {
            "let realm = call.realm();\n        "
        } else {
            ""
        };
        let _ = writeln!(
            glue,
            "fn op_for_each<H: {name}Impl>(rt: &mut swb_js::Runtime, call: &swb_js::NativeCall) -> swb_js::VmResult<swb_js::NativeReturn> {{\n    b::glue::run(rt, call, b::Context::Operation({lit}, \"forEach\"), super::DOM_EXCEPTION, |rt| {{\n        {realm}let platform = b::glue::this_platform(rt, call, super::{class})?;\n        b::glue::require_arguments(call, 1)?;\n        let callback = b::iterable::callback(rt, rt.arg(call, 0)).map_err(|e| e.argument(0))?;\n        let this_arg = rt.arg(call, 1);\n        {this_conv}\n        let mut index = 0;\n        while let Some((key, value)) = <H as {name}Impl>::pair_at(&mut b::Cx::new(rt, call, super::{class}), this, index)? {{\n            let value = {value_js}?;\n            let key = {key_js}?;\n            rt.call(callback, this_arg, &[value, key, swb_js::Value::Object(platform.object)])?;\n            index = index.checked_add(1).ok_or_else(|| b::Exception::range_error(\"too many pairs\"))?;\n        }}\n        Ok(swb_js::Value::Undefined)\n    }})\n}}\n"
        );
    }

    /// The statements of `define` that create the iteration members (Web
    /// IDL §3.7.9): `first` before the attributes, `last` after the
    /// operations.
    pub(crate) fn iteration_definitions(&self, iface: &Iface) -> (String, String) {
        let mut first = String::new();
        let mut last = String::new();
        let iterable = crate::legacy::iterable(iface);
        let value_iterator = iterable.is_some_and(|it| it.key.is_none());
        if value_iterator {
            first.push_str(
                "    b::iterable::define_value_iterator(rt, realm, interface.prototype)?;\n",
            );
        }
        if iterable.is_some_and(|it| it.key.is_some()) {
            let _ = writeln!(
                last,
                "    b::iterable::iterator_prototype::<H>(rt, realm, super::{}, iterator_next::<H>)?;\n    let entries = rt.define_method(realm, interface.prototype, \"entries\", 0, op_entries::<H>, swb_js::Attributes::MEMBER)?;\n    rt.define_method(realm, interface.prototype, \"forEach\", 1, op_for_each::<H>, swb_js::Attributes::MEMBER)?;\n    rt.define_method(realm, interface.prototype, \"keys\", 0, op_keys::<H>, swb_js::Attributes::MEMBER)?;\n    rt.define_method(realm, interface.prototype, \"values\", 0, op_values::<H>, swb_js::Attributes::MEMBER)?;\n    b::iterable::define_pair_iterator(rt, interface.prototype, entries)?;",
                iterator_constant(&iface.name)
            );
        }
        if value_iterator || self.model.declares_indexed_getter(iface) {
            last.push_str(
                "    b::iterable::define_indexed_iterator(rt, realm, interface.prototype)?;\n",
            );
        }
        (first, last)
    }
}

/// The hook functions of the class and `hooks`.
fn hook_wrappers(iface: &Iface, glue: &mut String) {
    let bound = "H: super::Bindings";
    let object = "object: swb_js::Gc<swb_js::Object>";
    let _ = writeln!(
        glue,
        "fn hook_get_own_property<{bound}>(rt: &mut swb_js::Runtime, {object}, key: swb_js::PropertyKey) -> swb_js::VmResult<Option<swb_js::Property>> {{\n    b::legacy::get_own_property(rt, object, key, &legacy::<H>())\n}}\n\nfn hook_define<{bound}>(rt: &mut swb_js::Runtime, {object}, key: swb_js::PropertyKey, desc: &swb_js::PropertyDescriptor) -> swb_js::VmResult<Option<bool>> {{\n    b::legacy::define_own_property(rt, object, key, desc, &legacy::<H>())\n}}\n\nfn hook_set<{bound}>(rt: &mut swb_js::Runtime, {object}, key: swb_js::PropertyKey, value: swb_js::Value, receiver: swb_js::Value) -> swb_js::VmResult<swb_js::SetAnswer> {{\n    b::legacy::set(rt, object, key, value, receiver, &legacy::<H>())\n}}\n\nfn hook_delete<{bound}>(rt: &mut swb_js::Runtime, {object}, key: swb_js::PropertyKey) -> swb_js::VmResult<Option<bool>> {{\n    b::legacy::delete(rt, object, key, &legacy::<H>())\n}}\n\nfn hook_own_keys<{bound}>(rt: &mut swb_js::Runtime, {object}, keys: &mut Vec<swb_js::PropertyKey>) -> swb_js::VmResult<()> {{\n    b::legacy::own_keys(rt, object, keys, &legacy::<H>())\n}}\n\nfn hook_refusal<{bound}>(rt: &mut swb_js::Runtime, {object}, key: swb_js::PropertyKey, why: swb_js::Refusal) -> swb_js::VmResult<Option<String>> {{\n    b::legacy::refusal(rt, object, key, why, &legacy::<H>())\n}}\n"
    );
    let _ = writeln!(
        glue,
        "/// The hooks of the class of `{}`: its instances are legacy platform\n/// objects (Web IDL §3.9).\npub fn hooks<{bound}>() -> swb_js::HostHooks {{\n    swb_js::HostHooks {{\n        get_own_property: Some(hook_get_own_property::<H>),\n        define_own_property: Some(hook_define::<H>),\n        get: None,\n        set: Some(hook_set::<H>),\n        delete: Some(hook_delete::<H>),\n        own_keys: Some(hook_own_keys::<H>),\n        prevent_extensions: Some(b::legacy::prevent_extensions),\n        refusal: Some(hook_refusal::<H>),\n    }}\n}}\n",
        iface.name
    );
}

/// The trait method of a special operation without an identifier.
fn unnamed_method(special: Special, key: Key) -> &'static str {
    match (special, key) {
        (Special::Getter, Key::Indexed) => "indexed_getter",
        (Special::Setter, Key::Indexed) => "indexed_setter",
        (Special::Getter, Key::Named) => "named_getter",
        (Special::Setter, Key::Named) => "named_setter",
        (Special::Deleter, _) => "named_deleter",
    }
}

fn special_word(special: Special) -> &'static str {
    match special {
        Special::Getter => "getter",
        Special::Setter => "setter",
        Special::Deleter => "deleter",
    }
}

/// The path of the trait of `owner` from the module of `module`.
fn trait_path(owner: &Iface, module: &Iface) -> String {
    if owner.name == module.name {
        format!("{}Impl", owner.name)
    } else {
        format!("super::{}::{}Impl", snake(&owner.name), owner.name)
    }
}

/// The context of a call of a trait function of `owner` from a hook.
fn legacy_cx(owner: &Iface) -> String {
    format!(
        "&mut b::Cx::without_call(rt, realm, super::{})",
        constant(&owner.name)
    )
}

/// The constant of the iterator class of an interface with a pair
/// iterator.
pub(crate) fn iterator_constant(name: &str) -> String {
    format!("{}_ITERATOR", constant(name))
}
