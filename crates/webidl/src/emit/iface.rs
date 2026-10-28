//! The module of an interface: the trait of its hand-written behaviour,
//! the native functions of its members (Web IDL §3.7: the brand check,
//! the argument conversions and overload resolution, the call, the result
//! conversion), and the function that defines the interface object, the
//! prototype and the members in a realm.
//!
//! Property order (measured in Chromium, `tools/probes/web-bindings.json`):
//! the attributes in IDL order, the constants, the operations sorted by
//! name, then `constructor`. The interface object has `length`, `name`,
//! `prototype`, then the constants.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::Error;
use crate::ast::{Argument, Attribute, ExtValue, Member, Operation, Place, TypeKind};
use crate::model::{Iface, exposed_in_window};

use super::Gen;
use super::names::{constant, ident, literal, snake};

/// An operation (or constructor) with its overloads.
struct OpGroup<'a> {
    name: String,
    overloads: Vec<&'a Operation>,
    is_static: bool,
    /// The name of a legacy factory function (`Image`); such a group has
    /// one overload and the empty `name`, like a constructor.
    factory: Option<String>,
}

/// One entry of an effective overload set for an argument count: the
/// overload and how many of its arguments the count covers.
struct Entry<'a> {
    index: usize,
    op: &'a Operation,
}

impl Gen<'_> {
    /// The text of an interface's module.
    pub(crate) fn iface_file(&self, iface: &Iface) -> Result<String, Error> {
        let mut out = String::new();
        out.push_str(&self.header(&format!("the interface `{}`", iface.name)));
        out.push_str("#![allow(clippy::too_many_arguments, clippy::too_many_lines, clippy::type_complexity, clippy::extra_unused_type_parameters, clippy::unreadable_literal, clippy::single_match_else, clippy::fn_params_excessive_bools, unused_variables)]\n\n");
        let _ = writeln!(out, "use {} as b;\n", self.support);
        let groups = op_groups(iface);
        let mut trait_items = String::new();
        let mut glue = String::new();
        let mut names: BTreeMap<String, Place> = BTreeMap::new();
        let mut claim = |name: &str, place: &Place| -> Result<(), Error> {
            if names.insert(name.to_owned(), place.clone()).is_some() {
                return Err(Error {
                    file: place.file.clone(),
                    line: place.line,
                    message: format!(
                        "two members of '{}' map to the Rust name '{name}'",
                        iface.name
                    ),
                });
            }
            Ok(())
        };
        for member in &iface.members {
            if let Member::Attribute(attr) = member {
                if !member_exposed(&attr.ext_attrs) {
                    continue;
                }
                self.attribute(iface, attr, &mut trait_items, &mut glue, &mut claim)?;
            }
        }
        for group in &groups {
            self.operation(iface, group, &mut trait_items, &mut glue, &mut claim)?;
        }
        let factory_ops = factory_operations(iface);
        let factories: Vec<OpGroup<'_>> = factory_ops
            .iter()
            .map(|(name, op)| OpGroup {
                name: String::new(),
                overloads: vec![op],
                is_static: false,
                factory: Some(name.clone()),
            })
            .collect();
        for group in &factories {
            self.operation(iface, group, &mut trait_items, &mut glue, &mut claim)?;
        }
        if let Some(place) = iface.members.iter().find_map(|m| match m {
            Member::Stringifier(place) => Some(place),
            _ => None,
        }) {
            claim("stringify", place)?;
            let _ = writeln!(
                trait_items,
                "    /// `stringifier;`: the `toString` of the interface.\n    fn stringify(cx: &mut b::Cx<'_>, this: {}) -> b::Result<String>;\n",
                self.this_type(iface)
            );
            self.stringifier_glue(iface, "stringify", &mut glue);
        }
        self.legacy_trait_items(iface, &mut trait_items, &mut claim)?;
        self.legacy_glue(iface, &mut glue);
        self.pair_iterator_glue(iface, &mut glue);
        let trait_name = format!("{}Impl", iface.name);
        let _ = writeln!(out, "/// The hand-written behaviour of `{}`.", iface.name);
        let _ = writeln!(out, "pub trait {trait_name}: b::Host {{");
        out.push_str(&trait_items);
        out.push_str("}\n\n");
        if trait_items.is_empty() {
            // Nothing to write by hand: the implementation is ready.
            let _ = writeln!(out, "impl<T: b::Host> {trait_name} for T {{}}\n");
        }
        out.push_str(&glue);
        self.define_fn(iface, &groups, &factories, &mut out)?;
        Ok(out)
    }

    /// The Rust type of `this`.
    pub(super) fn this_type(&self, iface: &Iface) -> &'static str {
        if self.model.is_node(&iface.name) {
            "b::NodeId"
        } else {
            "b::Platform"
        }
    }

    /// The statement that checks `this` (Web IDL §3.7.6 step "if thisValue
    /// does not implement the interface, throw").
    fn this_check(&self, iface: &Iface) -> String {
        let check = if self.model.is_node(&iface.name) {
            "this_node"
        } else {
            "this_platform"
        };
        format!(
            "let this = b::glue::{check}(rt, call, super::{})?;",
            constant(&iface.name)
        )
    }

    // --- Attributes ---

    fn attribute(
        &self,
        iface: &Iface,
        attr: &Attribute,
        trait_items: &mut String,
        glue: &mut String,
        claim: &mut impl FnMut(&str, &Place) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let getter = snake(&attr.name);
        let fn_name = ident(&attr.name);
        if is_event_handler(attr) {
            self.event_handler_glue(iface, attr, glue);
            return Ok(());
        }
        let reflect = reflect_kind(attr);
        let this_ty = self.this_type(iface);
        if (reflect.is_some() || attr.ext_attrs.has("ReflectSetter"))
            && !self.model.is_node(&iface.name)
        {
            return Err(err(
                &attr.place,
                "[Reflect] needs an interface that inherits from Node",
            ));
        }
        let out_ty = self.rust_type(&attr.ty, "super::types::", true);
        let in_ty = self.rust_type(&attr.ty, "super::types::", false);
        let custom = attr.ext_attrs.has("SwbCustom");
        let context = format!(
            "b::Context::Getter({}, {})",
            literal(&iface.name),
            literal(&attr.name)
        );
        // The getter.
        if custom {
            claim(&fn_name, &attr.place)?;
            let _ = writeln!(
                trait_items,
                "    /// The getter of `{}`, written by hand (`[SwbCustom]`).\n    fn {fn_name}(rt: &mut swb_js::Runtime, call: &swb_js::NativeCall) -> swb_js::VmResult<swb_js::NativeReturn>;\n",
                attr.name
            );
        } else if reflect.is_none() {
            claim(&fn_name, &attr.place)?;
            let _ = writeln!(
                trait_items,
                "    /// `{}attribute {} {}`.\n    fn {fn_name}(cx: &mut b::Cx<'_>, this: {this_ty}) -> b::Result<{out_ty}>;\n",
                if attr.readonly { "readonly " } else { "" },
                self.idl_text(&attr.ty),
                attr.name
            );
            let value = self.rust_to_js(&attr.ty, "result", "call.realm()", true, 0);
            let _ = writeln!(
                glue,
                "fn get_{getter}<H: {}Impl>(rt: &mut swb_js::Runtime, call: &swb_js::NativeCall) -> swb_js::VmResult<swb_js::NativeReturn> {{\n    b::glue::run(rt, call, {context}, super::DOM_EXCEPTION, |rt| {{\n        {}\n        let result = <H as {}Impl>::{fn_name}({}, this)?;\n        Ok({value}?)\n    }})\n}}\n",
                iface.name,
                self.this_check(iface),
                iface.name,
                cx(iface)
            );
        } else if let Some(kind) = &reflect {
            let get = self.reflect_get(attr, *kind)?.replace("CX", &cx(iface));
            let value = self.rust_to_js(&attr.ty, "result", "call.realm()", true, 0);
            let _ = writeln!(
                glue,
                "fn get_{getter}<H: {}Impl>(rt: &mut swb_js::Runtime, call: &swb_js::NativeCall) -> swb_js::VmResult<swb_js::NativeReturn> {{\n    b::glue::run(rt, call, {context}, super::DOM_EXCEPTION, |rt| {{\n        {}\n        let result = {get}?;\n        Ok({value}?)\n    }})\n}}\n",
                iface.name,
                self.this_check(iface)
            );
        }
        // The setter.
        if attr.readonly && !attr.ext_attrs.has("Replaceable") && !attr.ext_attrs.has("PutForwards")
        {
            return Ok(());
        }
        let context = format!(
            "b::Context::Setter({}, {})",
            literal(&iface.name),
            literal(&attr.name)
        );
        let set_fn = format!("set_{getter}");
        let head = format!(
            "fn {set_fn}<H: {}Impl>(rt: &mut swb_js::Runtime, call: &swb_js::NativeCall) -> swb_js::VmResult<swb_js::NativeReturn> {{\n    b::glue::run(rt, call, {context}, super::DOM_EXCEPTION, |rt| {{\n        {}\n",
            iface.name,
            self.this_check(iface)
        );
        if attr.ext_attrs.has("Replaceable") {
            let _ = writeln!(
                glue,
                "{head}        let _ = this;\n        let this = rt.this_or_global(call)?;\n        b::glue::replace(rt, call, this, {})\n    }})\n}}\n",
                literal(&attr.name)
            );
            return Ok(());
        }
        if let Some(forward) = attr.ext_attrs.get("PutForwards") {
            let ExtValue::Ident(target) = &forward.value else {
                return Err(err(&attr.place, "[PutForwards] needs an attribute name"));
            };
            let _ = writeln!(
                glue,
                "{head}        let _ = this;\n        let this = rt.this_or_global(call)?;\n        b::glue::put_forwards(rt, call, this, {}, {})\n    }})\n}}\n",
                literal(&attr.name),
                literal(target)
            );
            return Ok(());
        }
        if custom {
            claim(&set_fn, &attr.place)?;
            let _ = writeln!(
                trait_items,
                "    /// The setter of `{}`, written by hand (`[SwbCustom]`).\n    fn {set_fn}(rt: &mut swb_js::Runtime, call: &swb_js::NativeCall) -> swb_js::VmResult<swb_js::NativeReturn>;\n",
                attr.name
            );
            return Ok(());
        }
        let convert = self.js_to_rust(&attr.ty, "v", "super::types::", "call.realm()", 0);
        let call_impl = if let Some(kind) = &reflect {
            self.reflect_set(attr, *kind)?
        } else if attr.ext_attrs.has("ReflectSetter") {
            let name = content_attribute(attr);
            match self.model.resolve(&attr.ty).kind {
                TypeKind::DomString | TypeKind::UsvString => format!(
                    "b::reflect::set_string::<H>({}, this, {}, &value)",
                    cx(iface),
                    literal(&name)
                ),
                TypeKind::Integer(crate::ast::IntegerType::Long) => format!(
                    "b::reflect::set_long::<H>({}, this, {}, false, value)",
                    cx(iface),
                    literal(&name)
                ),
                TypeKind::Integer(crate::ast::IntegerType::UnsignedLong) => format!(
                    "b::reflect::set_unsigned_long::<H>({}, this, {}, {}, None, value)",
                    cx(iface),
                    literal(&name),
                    unsigned_limit(attr, Reflect::Plain)?
                ),
                _ => {
                    return Err(err(
                        &attr.place,
                        "[ReflectSetter] needs a string, long or unsigned long attribute",
                    ));
                }
            }
        } else {
            claim(&set_fn, &attr.place)?;
            let _ = writeln!(
                trait_items,
                "    /// The setter of `attribute {} {}`.\n    fn {set_fn}(cx: &mut b::Cx<'_>, this: {this_ty}, value: {in_ty}) -> b::Result<()>;\n",
                self.idl_text(&attr.ty),
                attr.name
            );
            format!(
                "<H as {}Impl>::{set_fn}({}, this, value)",
                iface.name,
                cx(iface)
            )
        };
        let call_impl = call_impl.replace(
            "b::Cx::new(rt, call, CLASS)",
            &format!("b::Cx::new(rt, call, super::{})", constant(&iface.name)),
        );
        let _ = writeln!(
            glue,
            "{head}        let v = rt.arg(call, 0);\n        let value = {convert}.map_err(b::Exception::nested)?;\n        {call_impl}?;\n        Ok(swb_js::Value::Undefined)\n    }})\n}}\n"
        );
        Ok(())
    }

    /// The getter and the setter of an event handler IDL attribute (HTML
    /// §8.1.8.1, <https://html.spec.whatwg.org/multipage/webappapis.html#event-handler-idl-attributes>):
    /// the host decides what the attribute holds (`Host::event_handler`
    /// and `Host::set_event_handler`), so that the interface has no trait
    /// method for it. The target is the node, or the window for `Window`.
    fn event_handler_glue(&self, iface: &Iface, attr: &Attribute, glue: &mut String) {
        let getter = snake(&attr.name);
        let target = if self.model.is_node(&iface.name) {
            "b::HandlerTarget::Node(this)"
        } else {
            "b::HandlerTarget::Window"
        };
        for (set, context, body) in [
            (
                false,
                format!(
                    "b::Context::Getter({}, {})",
                    literal(&iface.name),
                    literal(&attr.name)
                ),
                format!(
                    "Ok(<H as b::Host>::event_handler(rt, {target}, {})?)",
                    literal(&attr.name)
                ),
            ),
            (
                true,
                format!(
                    "b::Context::Setter({}, {})",
                    literal(&iface.name),
                    literal(&attr.name)
                ),
                format!(
                    "let v = rt.arg(call, 0);\n        <H as b::Host>::set_event_handler(rt, {target}, {}, v)?;\n        Ok(swb_js::Value::Undefined)",
                    literal(&attr.name)
                ),
            ),
        ] {
            let _ = writeln!(
                glue,
                "fn {}_{getter}<H: {}Impl>(rt: &mut swb_js::Runtime, call: &swb_js::NativeCall) -> swb_js::VmResult<swb_js::NativeReturn> {{\n    b::glue::run(rt, call, {context}, super::DOM_EXCEPTION, |rt| {{\n        {}\n        {body}\n    }})\n}}\n",
                if set { "set" } else { "get" },
                iface.name,
                self.this_check(iface),
            );
        }
    }

    /// The getter call of a reflected attribute (an expression with the
    /// placeholder of the context `CX` and `this`).
    fn reflect_get(&self, attr: &Attribute, kind: Reflect) -> Result<String, Error> {
        let name = literal(&content_attribute(attr));
        let t = self.model.resolve(&attr.ty);
        let keywords = keywords(attr)?;
        Ok(match (&t.kind, t.nullable, kind) {
            (TypeKind::DomString, false, Reflect::Plain) => match keywords {
                Some(k) => format!("b::reflect::get_enumerated::<H>(CX, this, {name}, &{k})"),
                None => format!("b::reflect::get_string::<H>(CX, this, {name})"),
            },
            (TypeKind::DomString, true, Reflect::Plain) => match keywords {
                Some(k) => {
                    format!("b::reflect::get_nullable_enumerated::<H>(CX, this, {name}, &{k})")
                }
                None => format!("b::reflect::get_nullable_string::<H>(CX, this, {name})"),
            },
            (TypeKind::UsvString, false, Reflect::Plain) => {
                format!("b::reflect::get_string::<H>(CX, this, {name})")
            }
            (TypeKind::UsvString, false, Reflect::Url) => {
                format!("b::reflect::get_url::<H>(CX, this, {name})")
            }
            (TypeKind::Boolean, false, Reflect::Plain) => {
                format!("b::reflect::get_boolean::<H>(CX, this, {name})")
            }
            (
                TypeKind::Integer(crate::ast::IntegerType::Long),
                false,
                Reflect::Plain | Reflect::NonNegative,
            ) => {
                let non_negative = kind == Reflect::NonNegative;
                let default = default_integer(attr)?;
                format!("b::reflect::get_long::<H>(CX, this, {name}, {non_negative}, {default})")
            }
            (TypeKind::Integer(crate::ast::IntegerType::UnsignedLong), false, _) => {
                let limit = unsigned_limit(attr, kind)?;
                let default = default_integer(attr)?;
                format!("b::reflect::get_unsigned_long::<H>(CX, this, {name}, {limit}, {default})")
            }
            _ => {
                return Err(err(
                    &attr.place,
                    format!(
                        "[Reflect] of the type {} is not generated",
                        self.idl_name(&t)
                    ),
                ));
            }
        })
    }

    /// The setter call of a reflected attribute (`value` in scope).
    fn reflect_set(&self, attr: &Attribute, kind: Reflect) -> Result<String, Error> {
        let name = literal(&content_attribute(attr));
        let t = self.model.resolve(&attr.ty);
        let cx = "&mut b::Cx::new(rt, call, CLASS)";
        Ok(match (&t.kind, t.nullable) {
            (TypeKind::DomString | TypeKind::UsvString, false) => {
                format!("b::reflect::set_string::<H>({cx}, this, {name}, &value)")
            }
            (TypeKind::DomString, true) => {
                format!(
                    "b::reflect::set_nullable_string::<H>({cx}, this, {name}, value.as_deref())"
                )
            }
            (TypeKind::Boolean, false) => {
                format!("b::reflect::set_boolean::<H>({cx}, this, {name}, value)")
            }
            (TypeKind::Integer(crate::ast::IntegerType::Long), false) => {
                let non_negative = kind == Reflect::NonNegative;
                format!("b::reflect::set_long::<H>({cx}, this, {name}, {non_negative}, value)")
            }
            (TypeKind::Integer(crate::ast::IntegerType::UnsignedLong), false) => {
                let limit = unsigned_limit(attr, kind)?;
                let default = default_integer(attr)?;
                format!(
                    "b::reflect::set_unsigned_long::<H>({cx}, this, {name}, {limit}, {default}, value)"
                )
            }
            _ => {
                return Err(err(
                    &attr.place,
                    format!(
                        "[Reflect] of the type {} is not generated",
                        self.idl_name(&t)
                    ),
                ));
            }
        })
    }

    // --- Operations ---

    fn operation(
        &self,
        iface: &Iface,
        group: &OpGroup<'_>,
        trait_items: &mut String,
        glue: &mut String,
        claim: &mut impl FnMut(&str, &Place) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let constructor = group.name.is_empty();
        let base = if let Some(factory) = &group.factory {
            format!("legacy_factory_{}", snake(factory))
        } else if constructor {
            "constructor".to_owned()
        } else {
            ident(&group.name)
        };
        let overloaded = group.overloads.len() > 1;
        if constructor
            && group
                .overloads
                .iter()
                .any(|op| op.ext_attrs.has("HTMLConstructor"))
        {
            // Element constructors are for custom elements; swb has none,
            // so `new` throws (Chromium: "Illegal constructor").
            let _ = writeln!(
                glue,
                "pub(super) fn construct<H: {0}Impl>(rt: &mut swb_js::Runtime, call: &swb_js::NativeCall) -> swb_js::VmResult<swb_js::NativeReturn> {{\n    b::glue::run(rt, call, b::Context::Constructor({1}), super::DOM_EXCEPTION, |_| Err(b::Exception::type_error(\"Illegal constructor\")))\n}}\n",
                iface.name,
                literal(&iface.name)
            );
            return Ok(());
        }
        let custom = group
            .overloads
            .iter()
            .any(|op| op.ext_attrs.has("SwbCustom"));
        let first = group
            .overloads
            .first()
            .map(|op| &op.place)
            .cloned()
            .unwrap_or_default();
        if custom {
            if overloaded || constructor {
                return Err(err(
                    &first,
                    "[SwbCustom] is for operations without overloads",
                ));
            }
            claim(&base, &first)?;
            let _ = writeln!(
                trait_items,
                "    /// `{}`, written by hand (`[SwbCustom]`).\n    fn {base}(rt: &mut swb_js::Runtime, call: &swb_js::NativeCall) -> swb_js::VmResult<swb_js::NativeReturn>;\n",
                group.name
            );
            return Ok(());
        }
        let this_ty = self.this_type(iface);
        let mut methods = Vec::new();
        for (i, op) in group.overloads.iter().enumerate() {
            let method = if overloaded {
                format!("{base}_{}", i + 1)
            } else {
                base.clone()
            };
            claim(&method, &op.place)?;
            let mut params = vec!["cx: &mut b::Cx<'_>".to_owned()];
            if !constructor && !group.is_static {
                params.push(format!("this: {this_ty}"));
            }
            for arg in &op.args {
                params.push(format!("{}: {}", arg_ident(&arg.name), self.arg_type(arg)));
            }
            let ret = if constructor {
                super::types::GC_OBJECT.to_owned()
            } else {
                self.rust_type(&op.ret, "super::types::", true)
            };
            let signature = op
                .args
                .iter()
                .map(|a| {
                    format!(
                        "{}{}{} {}",
                        if a.optional { "optional " } else { "" },
                        self.idl_text(&a.ty),
                        if a.variadic { "..." } else { "" },
                        a.name
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(
                trait_items,
                "    /// `{}({signature})`.\n    fn {method}({}) -> b::Result<{ret}>;\n",
                if let Some(factory) = &group.factory {
                    format!("new {factory}")
                } else if constructor {
                    "constructor".to_owned()
                } else {
                    format!("{} {}", self.idl_text(&op.ret), group.name)
                },
                params.join(", ")
            );
            methods.push(method);
        }
        self.operation_glue(iface, group, &methods, glue)
    }

    /// The Rust type of an argument as the trait method takes it.
    fn arg_type(&self, arg: &Argument) -> String {
        let ty = self.rust_type(&arg.ty, "super::types::", false);
        if arg.variadic {
            format!("Vec<{ty}>")
        } else if arg.optional
            && arg.default.is_none()
            && !matches!(self.model.resolve(&arg.ty).kind, TypeKind::Any)
        {
            format!("Option<{ty}>")
        } else {
            ty
        }
    }

    fn operation_glue(
        &self,
        iface: &Iface,
        group: &OpGroup<'_>,
        methods: &[String],
        glue: &mut String,
    ) -> Result<(), Error> {
        let constructor = group.name.is_empty();
        let fn_name = if let Some(factory) = &group.factory {
            format!("construct_{}", snake(factory))
        } else if constructor {
            "construct".to_owned()
        } else {
            format!("op_{}", snake(&group.name))
        };
        let context = if let Some(factory) = &group.factory {
            format!("b::Context::Constructor({})", literal(factory))
        } else if constructor {
            format!("b::Context::Constructor({})", literal(&iface.name))
        } else {
            format!(
                "b::Context::Operation({}, {})",
                literal(&iface.name),
                literal(&group.name)
            )
        };
        let promise = !constructor
            && group
                .overloads
                .iter()
                .any(|op| matches!(self.model.resolve(&op.ret).kind, TypeKind::Promise(_)));
        let run = if promise { "run_promise" } else { "run" };
        let vis = if constructor { "pub(super) " } else { "" };
        let _ = writeln!(
            glue,
            "{vis}fn {fn_name}<H: {}Impl>(rt: &mut swb_js::Runtime, call: &swb_js::NativeCall) -> swb_js::VmResult<swb_js::NativeReturn> {{\n    b::glue::{run}(rt, call, {context}, super::DOM_EXCEPTION, |rt| {{",
            iface.name
        );
        if !constructor && !group.is_static {
            let _ = writeln!(glue, "        {}", self.this_check(iface));
        }
        if group.factory.is_some() {
            // Web IDL §3.7.2: a factory function needs `new` (Chromium's
            // message).
            glue.push_str("        if call.new_target().is_undefined() {\n            return Err(b::Exception::type_error(\"Please use the 'new' operator, this DOM object constructor cannot be called as a function.\"));\n        }\n");
        }
        let min_required = group
            .overloads
            .iter()
            .map(|op| required_count(op))
            .min()
            .unwrap_or(0);
        if group.overloads.len() == 1 {
            if min_required > 0 {
                let _ = writeln!(
                    glue,
                    "        b::glue::require_arguments(call, {min_required})?;"
                );
            }
            let body = self.overload_call(iface, group, 0, &methods[0], constructor)?;
            glue.push_str(&body);
        } else {
            let max_args = group
                .overloads
                .iter()
                .map(|op| op.args.len())
                .max()
                .unwrap_or(0);
            if min_required > 0 {
                let _ = writeln!(
                    glue,
                    "        b::glue::require_arguments(call, {min_required})?;"
                );
            }
            let _ = writeln!(glue, "        match call.argc().min({max_args}) {{");
            for count in min_required..=max_args {
                let entries: Vec<Entry<'_>> = group
                    .overloads
                    .iter()
                    .enumerate()
                    .filter(|(_, op)| accepts(op, count))
                    .map(|(index, op)| Entry { index, op })
                    .collect();
                let arm = if count == max_args {
                    "_".to_owned()
                } else {
                    count.to_string()
                };
                match entries.as_slice() {
                    [] => {
                        let _ = writeln!(
                            glue,
                            "            {arm} => Err(b::Exception::type_error(\"Overload resolution failed.\")),"
                        );
                    }
                    [entry] => {
                        let body = self.overload_call(
                            iface,
                            group,
                            entry.index,
                            &methods[entry.index],
                            constructor,
                        )?;
                        let _ = writeln!(glue, "            {arm} => {{\n{body}            }}");
                    }
                    entries => {
                        let d = distinguishing_index(self, entries, count).ok_or_else(|| {
                            err(&entries[0].op.place, format!("the overloads of '{}' with {count} arguments cannot be told apart", group.name))
                        })?;
                        let candidates: Vec<String> = entries
                            .iter()
                            .map(|e| {
                                let arg = &e.op.args[d.min(e.op.args.len() - 1)];
                                self.candidate(&arg.ty, arg.optional)
                            })
                            .collect();
                        let _ = writeln!(
                            glue,
                            "            {arm} => {{\n                let v = rt.arg(call, {d});\n                let which = b::overload::select(rt, v, &[{}])?;\n                match which {{",
                            candidates.join(", ")
                        );
                        for (i, entry) in entries.iter().enumerate() {
                            let body = self.overload_call(
                                iface,
                                group,
                                entry.index,
                                &methods[entry.index],
                                constructor,
                            )?;
                            let pattern = if i + 1 == entries.len() {
                                "_".to_owned()
                            } else {
                                i.to_string()
                            };
                            let _ = writeln!(
                                glue,
                                "                    {pattern} => {{\n{body}                    }}"
                            );
                        }
                        glue.push_str("                }\n            }\n");
                    }
                }
            }
            glue.push_str("        }\n");
        }
        glue.push_str("    })\n}\n\n");
        Ok(())
    }

    /// The conversion of the arguments of one overload and the call of its
    /// trait method; the last expression is the converted result.
    fn overload_call(
        &self,
        iface: &Iface,
        group: &OpGroup<'_>,
        index: usize,
        method: &str,
        constructor: bool,
    ) -> Result<String, Error> {
        let op = group.overloads[index];
        let mut body = String::new();
        let mut args = Vec::new();
        if !constructor && !group.is_static {
            args.push("this".to_owned());
        }
        for (i, arg) in op.args.iter().enumerate() {
            let name = format!("a{i}");
            let convert = self.js_to_rust(&arg.ty, "v", "super::types::", "call.realm()", 0);
            let convert = format!("{convert}.map_err(|e| e.argument({i}))?");
            if arg.variadic {
                let item_convert =
                    self.js_to_rust(&arg.ty, "v", "super::types::", "call.realm()", 0);
                let _ = writeln!(
                    body,
                    "        let mut {name} = Vec::new();\n        for i in {i}..call.argc() {{\n            let v = rt.arg(call, i);\n            {name}.push({item_convert}.map_err(|e| e.argument(i))?);\n        }}"
                );
            } else if arg.optional {
                let missing = match &arg.default {
                    Some(default) => self.default_value(
                        &arg.ty,
                        default,
                        "super::types::",
                        "call.realm()",
                        &op.place,
                    )?,
                    None if matches!(self.model.resolve(&arg.ty).kind, TypeKind::Any) => {
                        "swb_js::Value::Undefined".to_owned()
                    }
                    None => "None".to_owned(),
                };
                let present = if arg.default.is_none()
                    && !matches!(self.model.resolve(&arg.ty).kind, TypeKind::Any)
                {
                    format!("Some({convert})")
                } else {
                    convert
                };
                let _ = writeln!(
                    body,
                    "        let v = rt.arg(call, {i});\n        let {name} = if v.is_undefined() {{ {missing} }} else {{ {present} }};"
                );
            } else {
                let _ = writeln!(
                    body,
                    "        let v = rt.arg(call, {i});\n        let {name} = {convert};"
                );
            }
            args.push(name);
        }
        let call = format!(
            "<H as {}Impl>::{method}({}, {})",
            iface.name,
            cx(iface),
            args.join(", ")
        )
        .replace(", )", ")");
        if constructor {
            let _ = writeln!(
                body,
                "        let result = {call}?;\n        Ok(swb_js::Value::Object(result))"
            );
        } else if matches!(self.model.resolve(&op.ret).kind, TypeKind::Undefined) {
            let _ = writeln!(
                body,
                "        {call}?;\n        Ok(swb_js::Value::Undefined)"
            );
        } else {
            let value = self.rust_to_js(&op.ret, "result", "call.realm()", true, 0);
            let _ = writeln!(body, "        let result = {call}?;\n        Ok({value}?)");
        }
        Ok(body)
    }

    fn stringifier_glue(&self, iface: &Iface, method: &str, glue: &mut String) {
        let _ = writeln!(
            glue,
            "fn op_to_string<H: {0}Impl>(rt: &mut swb_js::Runtime, call: &swb_js::NativeCall) -> swb_js::VmResult<swb_js::NativeReturn> {{\n    b::glue::run(rt, call, b::Context::Operation({1}, \"toString\"), super::DOM_EXCEPTION, |rt| {{\n        {2}\n        let result = <H as {0}Impl>::{method}({3}, this)?;\n        Ok(b::to_js::string(rt, &result)?)\n    }})\n}}\n",
            iface.name,
            literal(&iface.name),
            self.this_check(iface),
            cx(iface)
        );
    }

    // --- The definition in a realm ---

    fn define_fn(
        &self,
        iface: &Iface,
        groups: &[OpGroup<'_>],
        factories: &[OpGroup<'_>],
        out: &mut String,
    ) -> Result<(), Error> {
        let class = format!("super::{}", constant(&iface.name));
        let global = iface.ext_attrs.has("Global");
        let parent = iface.parent.as_deref().map_or_else(
            || "None".to_owned(),
            |p| format!("Some(super::{})", constant(p)),
        );
        let _ = writeln!(
            out,
            "/// Creates the interface object and the prototype of `{}` in `realm`\n/// with their members (the parent's interface must exist).",
            iface.name
        );
        let _ = writeln!(
            out,
            "pub fn define<H: {}Impl>(rt: &mut swb_js::Runtime, realm: u32) -> swb_js::VmResult<swb_js::Interface> {{",
            iface.name
        );
        let _ = writeln!(
            out,
            "    let interface = rt.begin_interface(realm, {class})?;"
        );
        if iface.name == "DOMException" {
            out.push_str("    b::glue::inherit_error_prototype(rt, realm, interface)?;\n");
        }
        let target = if global {
            "rt.global(realm)?"
        } else {
            "interface.prototype"
        };
        let _ = writeln!(out, "    let target = {target};");
        out.push_str("    let _ = target;\n");
        // Unforgeable members go on a template (or on the global object).
        let unforgeable_attrs = iface.members.iter().any(|m| {
            matches!(m, Member::Attribute(a) if a.ext_attrs.has("LegacyUnforgeable") && member_exposed(&a.ext_attrs))
        });
        let unforgeable_ops = groups.iter().any(|g| {
            g.overloads
                .iter()
                .any(|op| op.ext_attrs.has("LegacyUnforgeable"))
        });
        let template =
            !global && (unforgeable_attrs || unforgeable_ops || self.inherits_unforgeable(iface));
        if template {
            // An interface without unforgeable members of its own only
            // copies its parent's template.
            let name = if unforgeable_attrs || unforgeable_ops {
                "template"
            } else {
                "_template"
            };
            let _ = writeln!(
                out,
                "    let {name} = b::glue::new_unforgeable_template::<H>(rt, realm, {class}, {parent})?;"
            );
        }
        let (first, last) = self.iteration_definitions(iface);
        out.push_str(&first);
        define_attributes(iface, global, out);
        define_constants(iface, out)?;
        define_operations(iface, groups, global, out);
        out.push_str(&last);
        out.push_str("    rt.end_interface(interface)?;\n");
        if exposed_in_window(&iface.ext_attrs) {
            define_factories(factories, out);
        }
        out.push_str("    Ok(interface)\n}\n");
        Ok(())
    }

    /// Whether an ancestor of the interface has `[LegacyUnforgeable]`
    /// members.
    fn inherits_unforgeable(&self, iface: &Iface) -> bool {
        let mut current = iface.parent.as_deref().and_then(|p| self.model.iface(p));
        while let Some(parent) = current {
            if parent.members.iter().any(|m| match m {
                Member::Attribute(a) => a.ext_attrs.has("LegacyUnforgeable"),
                Member::Operation(op) => op.ext_attrs.has("LegacyUnforgeable"),
                _ => false,
            }) {
                return true;
            }
            current = parent.parent.as_deref().and_then(|p| self.model.iface(p));
        }
        false
    }
}

/// The object and the attributes of a member's property: the prototype
/// (or the global object of a `[Global]` interface) for regular members;
/// the template (or the global object) for `[LegacyUnforgeable]` ones.
/// True for an attribute of the type `EventHandler`,
/// `OnErrorEventHandler` or `OnBeforeUnloadEventHandler` (the typedefs of
/// HTML §8.1.8.1).
fn is_event_handler(attr: &Attribute) -> bool {
    matches!(&attr.ty.kind, TypeKind::Named(name)
        if matches!(name.as_str(), "EventHandler" | "OnErrorEventHandler" | "OnBeforeUnloadEventHandler"))
}

fn member_place(unforgeable: bool, global: bool) -> (&'static str, &'static str) {
    const UNFORGEABLE: &str =
        "swb_js::Attributes { writable: false, enumerable: true, configurable: false }";
    match (unforgeable, global) {
        (true, false) => ("template", UNFORGEABLE),
        (true, true) => ("target", UNFORGEABLE),
        (false, _) => ("target", "swb_js::Attributes::MEMBER"),
    }
}

/// The accessor definitions of the attributes, in IDL order.
fn define_attributes(iface: &Iface, global: bool, out: &mut String) {
    for member in &iface.members {
        let Member::Attribute(attr) = member else {
            continue;
        };
        if !member_exposed(&attr.ext_attrs) {
            continue;
        }
        let getter = snake(&attr.name);
        let custom = attr.ext_attrs.has("SwbCustom");
        let get = if custom {
            format!("<H as {}Impl>::{}", iface.name, ident(&attr.name))
        } else {
            format!("get_{getter}::<H>")
        };
        let has_setter = !attr.readonly
            || attr.ext_attrs.has("Replaceable")
            || attr.ext_attrs.has("PutForwards");
        let set = if !has_setter {
            "None".to_owned()
        } else if custom {
            format!(
                "Some(<H as {}Impl>::set_{getter} as swb_js::NativeFn)",
                iface.name
            )
        } else {
            format!("Some(set_{getter}::<H> as swb_js::NativeFn)")
        };
        let (object, attributes) = member_place(attr.ext_attrs.has("LegacyUnforgeable"), global);
        let _ = writeln!(
            out,
            "    rt.define_accessor(realm, {object}, {}, (Some({get} as swb_js::NativeFn), {set}), {attributes})?;",
            literal(&attr.name)
        );
    }
}

/// The constants on the interface object and the prototype.
fn define_constants(iface: &Iface, out: &mut String) -> Result<(), Error> {
    for member in &iface.members {
        let Member::Const(c) = member else { continue };
        let value = match &c.value {
            crate::ast::ConstValue::Bool(b) => format!("swb_js::Value::Bool({b})"),
            crate::ast::ConstValue::Integer(i) if i32::try_from(*i).is_ok() => {
                format!("swb_js::Value::Int({i})")
            }
            crate::ast::ConstValue::Integer(i) => format!("swb_js::Value::number({i}_f64)"),
            crate::ast::ConstValue::Float(f) => match f.as_str() {
                "Infinity" => "swb_js::Value::number(f64::INFINITY)".to_owned(),
                "-Infinity" => "swb_js::Value::number(f64::NEG_INFINITY)".to_owned(),
                "NaN" => "swb_js::Value::number(f64::NAN)".to_owned(),
                text => format!("swb_js::Value::number({text}_f64)"),
            },
            _ => return Err(err(&c.place, "a constant needs a boolean or a number")),
        };
        let _ = writeln!(
            out,
            "    rt.define_value(interface.object, {0}, {value}, swb_js::Attributes::CONSTANT)?;\n    rt.define_value(interface.prototype, {0}, {value}, swb_js::Attributes::CONSTANT)?;",
            literal(&c.name)
        );
    }
    Ok(())
}

/// The method definitions, sorted by name (Chromium's order), then the
/// `toString` of a stringifier (Chromium puts it after the sorted
/// operations: `DOMTokenList` has `toggle` before `toString`).
fn define_operations(iface: &Iface, groups: &[OpGroup<'_>], global: bool, out: &mut String) {
    let mut definitions: Vec<(String, String)> = Vec::new();
    for group in groups.iter().filter(|g| !g.name.is_empty()) {
        let custom = group
            .overloads
            .iter()
            .any(|op| op.ext_attrs.has("SwbCustom"));
        let func = if custom {
            format!("<H as {}Impl>::{}", iface.name, ident(&group.name))
        } else {
            format!("op_{}::<H>", snake(&group.name))
        };
        let length = group
            .overloads
            .iter()
            .map(|op| required_count(op))
            .min()
            .unwrap_or(0);
        let unforgeable = group
            .overloads
            .iter()
            .any(|op| op.ext_attrs.has("LegacyUnforgeable"));
        let (object, attributes) = if group.is_static {
            ("interface.object", "swb_js::Attributes::MEMBER")
        } else {
            member_place(unforgeable, global)
        };
        definitions.push((
            group.name.clone(),
            format!(
                "    rt.define_method(realm, {object}, {}, {length}, {func}, {attributes})?;",
                literal(&group.name)
            ),
        ));
    }
    let stringifier_attr = iface.members.iter().find_map(|m| match m {
        Member::Attribute(a) if a.stringifier => Some(a),
        _ => None,
    });
    let stringifier = stringifier_attr.is_some()
        || iface
            .members
            .iter()
            .any(|m| matches!(m, Member::Stringifier(_)));
    definitions.sort_by(|a, b| a.0.cmp(&b.0));
    if stringifier {
        let func = stringifier_attr.map_or_else(
            || "op_to_string::<H>".to_owned(),
            |attr| format!("get_{}::<H>", snake(&attr.name)),
        );
        definitions.push((
            "toString".to_owned(),
            format!("    rt.define_method(realm, target, \"toString\", 0, {func}, swb_js::Attributes::MEMBER)?;"),
        ));
    }
    for (_, line) in definitions {
        let _ = writeln!(out, "{line}");
    }
}

/// The legacy factory functions of an interface (`[LegacyFactoryFunction=
/// Image(optional unsigned long width)]`) as operations without a name:
/// the factory's name and its arguments.
fn factory_operations(iface: &Iface) -> Vec<(String, Operation)> {
    iface
        .ext_attrs
        .0
        .iter()
        .filter(|a| a.name == crate::model::FACTORY)
        .filter_map(|attr| {
            let ExtValue::Ident(name) = &attr.value else {
                return None;
            };
            let op = Operation {
                name: None,
                ret: crate::ast::Type {
                    kind: TypeKind::Undefined,
                    nullable: false,
                    ext_attrs: crate::ast::ExtAttrs::default(),
                },
                args: attr.args.clone().unwrap_or_default(),
                special: None,
                is_static: false,
                ext_attrs: crate::ast::ExtAttrs::default(),
                place: attr.place.clone(),
            };
            Some((name.clone(), op))
        })
        .collect()
}

/// The functions of the legacy factory functions: each is a constructor
/// whose `prototype` is the interface prototype object, a property of the
/// global object (Web IDL §3.7.2).
fn define_factories(factories: &[OpGroup<'_>], out: &mut String) {
    for group in factories {
        let Some(name) = &group.factory else {
            continue;
        };
        let length = group
            .overloads
            .iter()
            .map(|op| required_count(op))
            .min()
            .unwrap_or(0);
        let _ = writeln!(
            out,
            "    let factory = rt.define_constructor(realm, interface.prototype, {}, {length}, construct_{}::<H>)?;\n    b::glue::expose_function(rt, realm, {}, factory)?;",
            literal(name),
            snake(name),
            literal(name)
        );
    }
}

/// The groups of operations (by name; the constructors as the group with
/// the empty name), in order of their first declaration.
fn op_groups(iface: &Iface) -> Vec<OpGroup<'_>> {
    let mut groups: Vec<OpGroup<'_>> = Vec::new();
    for member in &iface.members {
        let (name, op) = match member {
            Member::Operation(op) if member_exposed(&op.ext_attrs) => match &op.name {
                Some(name) => (name.clone(), op),
                None => continue,
            },
            Member::Constructor(op) => (String::new(), op),
            _ => continue,
        };
        match groups
            .iter_mut()
            .find(|g| g.name == name && g.is_static == op.is_static)
        {
            Some(group) => group.overloads.push(op),
            None => groups.push(OpGroup {
                name,
                overloads: vec![op],
                is_static: op.is_static,
                factory: None,
            }),
        }
    }
    groups
}

/// Whether a member is exposed in `Window` (its own `[Exposed]`).
fn member_exposed(attrs: &crate::ast::ExtAttrs) -> bool {
    !attrs.has("Exposed") || exposed_in_window(attrs)
}

/// The number of arguments before the first optional or variadic one.
fn required_count(op: &Operation) -> usize {
    op.args
        .iter()
        .take_while(|a| !a.optional && !a.variadic)
        .count()
}

/// Whether an overload is in the effective overload set for `count`
/// arguments.
fn accepts(op: &Operation, count: usize) -> bool {
    let required = required_count(op);
    let variadic = op.args.last().is_some_and(|a| a.variadic);
    count >= required && (count <= op.args.len() || variadic)
}

/// The distinguishing argument index of entries with `count` arguments:
/// the first index where their types differ.
fn distinguishing_index(generator: &Gen<'_>, entries: &[Entry<'_>], count: usize) -> Option<usize> {
    let type_at = |op: &Operation, i: usize| -> Option<String> {
        let arg = op
            .args
            .get(i)
            .or_else(|| op.args.last().filter(|a| a.variadic))?;
        Some(generator.idl_name(&generator.model.resolve(&arg.ty)))
    };
    (0..count.max(1)).find(|&i| {
        let first = entries.first().and_then(|e| type_at(e.op, i));
        entries.iter().any(|e| type_at(e.op, i) != first)
    })
}

/// The name of an argument in a trait method.
fn arg_ident(name: &str) -> String {
    let name = ident(name);
    if matches!(name.as_str(), "cx" | "this" | "rt" | "call") {
        format!("{name}_")
    } else {
        name
    }
}

/// The variants of reflection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reflect {
    Plain,
    Url,
    NonNegative,
    Positive,
    PositiveWithFallback,
}

fn reflect_kind(attr: &Attribute) -> Option<Reflect> {
    let a = &attr.ext_attrs;
    if a.has("Reflect") {
        Some(Reflect::Plain)
    } else if a.has("ReflectURL") {
        Some(Reflect::Url)
    } else if a.has("ReflectNonNegative") {
        Some(Reflect::NonNegative)
    } else if a.has("ReflectPositive") {
        Some(Reflect::Positive)
    } else if a.has("ReflectPositiveWithFallback") {
        Some(Reflect::PositiveWithFallback)
    } else {
        None
    }
}

/// The content attribute name: the string of the reflection attribute, or
/// the IDL name in ASCII lowercase.
fn content_attribute(attr: &Attribute) -> String {
    for name in [
        "Reflect",
        "ReflectSetter",
        "ReflectURL",
        "ReflectNonNegative",
        "ReflectPositive",
        "ReflectPositiveWithFallback",
    ] {
        if let Some(a) = attr.ext_attrs.get(name)
            && let ExtValue::String(text) | ExtValue::Ident(text) = &a.value
        {
            return text.clone();
        }
    }
    attr.name.to_ascii_lowercase()
}

/// `Some(n)` of `[ReflectDefault=n]`, or `None`.
fn default_integer(attr: &Attribute) -> Result<String, Error> {
    match attr.ext_attrs.get("ReflectDefault").map(|a| &a.value) {
        None => Ok("None".to_owned()),
        Some(ExtValue::Integer(n)) => Ok(format!("Some({n})")),
        Some(_) => Err(err(&attr.place, "[ReflectDefault] needs an integer")),
    }
}

/// The `b::reflect::UnsignedLimit` of an `unsigned long` reflection.
fn unsigned_limit(attr: &Attribute, kind: Reflect) -> Result<String, Error> {
    if let Some(range) = attr.ext_attrs.get("ReflectRange") {
        if let ExtValue::List(items) = &range.value
            && let [ExtValue::Integer(min), ExtValue::Integer(max)] = items.as_slice()
            && kind == Reflect::Plain
        {
            return Ok(format!("b::reflect::UnsignedLimit::Range({min}, {max})"));
        }
        return Err(err(
            &attr.place,
            "[ReflectRange] needs two integers and [Reflect] or [ReflectSetter]",
        ));
    }
    Ok(match kind {
        Reflect::Positive => "b::reflect::UnsignedLimit::Positive",
        Reflect::PositiveWithFallback => "b::reflect::UnsignedLimit::PositiveWithFallback",
        Reflect::Plain => "b::reflect::UnsignedLimit::None",
        Reflect::Url | Reflect::NonNegative => {
            return Err(err(
                &attr.place,
                "this reflection does not apply to unsigned long",
            ));
        }
    }
    .to_owned())
}

/// The keyword table of an enumerated attribute (`[SwbKeywords]`,
/// `[SwbMissing]`, `[SwbInvalid]`), as a Rust expression.
///
/// `SwbKeywords` is a space-separated list of keywords; `alias=canonical`
/// maps a keyword to the canonical keyword of its state, and `=canonical`
/// is the empty keyword.
fn keywords(attr: &Attribute) -> Result<Option<String>, Error> {
    let Some(list) = attr.ext_attrs.get("SwbKeywords") else {
        return Ok(None);
    };
    let ExtValue::String(list) = &list.value else {
        return Err(err(&attr.place, "[SwbKeywords] needs a string"));
    };
    let string = |name: &str| -> Result<String, Error> {
        match attr.ext_attrs.get(name).map(|a| &a.value) {
            None => Ok("None".to_owned()),
            Some(ExtValue::String(text)) => Ok(format!("Some({})", literal(text))),
            Some(_) => Err(err(&attr.place, format!("[{name}] needs a string"))),
        }
    };
    let mut pairs = Vec::new();
    for token in list.split_ascii_whitespace() {
        let (keyword, canonical) = token.split_once('=').unwrap_or((token, token));
        pairs.push(format!(
            "({}, {})",
            literal(&keyword.to_ascii_lowercase()),
            literal(canonical)
        ));
    }
    Ok(Some(format!(
        "b::reflect::Keywords {{ keywords: &[{}], missing: {}, invalid: {} }}",
        pairs.join(", "),
        string("SwbMissing")?,
        string("SwbInvalid")?
    )))
}

/// The context argument of a member's trait call.
fn cx(iface: &Iface) -> String {
    format!(
        "&mut b::Cx::new(rt, call, super::{})",
        constant(&iface.name)
    )
}

fn err(place: &Place, message: impl Into<String>) -> Error {
    Error {
        file: place.file.clone(),
        line: place.line,
        message: message.into(),
    }
}
