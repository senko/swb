//! The file `types.rs` of a set: a Rust struct per dictionary (with its
//! conversions in both directions), an enum per enumeration and per union
//! type, and a struct per callback function and callback interface (with
//! the steps that call it, Web IDL §3.11).

use std::fmt::Write as _;

use crate::Error;
use crate::ast::{Argument, Member, Type, TypeKind};
use crate::model::Kind;

use super::Gen;
use super::names::{camel, constant, ident, literal};

impl Gen<'_> {
    /// The text of `types.rs`.
    pub(crate) fn types_file(&self) -> Result<String, Error> {
        let mut out = String::new();
        out.push_str(&self.header("the dictionaries, enumerations, unions and callbacks"));
        out.push_str("#![allow(clippy::large_enum_variant, clippy::too_many_arguments, clippy::collapsible_if, clippy::explicit_auto_deref, clippy::match_same_arms, unused_parens, unused_variables)]\n\n");
        let _ = writeln!(out, "use {} as b;\n", self.support);
        for dict in &self.model.dictionaries {
            self.dictionary(&mut out, &dict.name)?;
        }
        for e in &self.model.enums {
            enumeration(&mut out, &e.name, &e.values);
        }
        for callback in &self.model.callbacks {
            self.callback_function(&mut out, &callback.name, &callback.ret, &callback.args);
        }
        for iface in &self.model.callback_interfaces {
            self.callback_interface(&mut out, iface);
        }
        // Unions last: the members above register the unions they use, and
        // a union's own members may register more.
        let mut done = std::collections::BTreeSet::new();
        loop {
            let pending: Vec<(String, Vec<Type>)> = self
                .unions
                .borrow()
                .iter()
                .filter(|(name, _)| !done.contains(*name))
                .map(|(n, m)| (n.clone(), m.clone()))
                .collect();
            if pending.is_empty() {
                break;
            }
            for (name, members) in pending {
                self.union_item(&mut out, &name, &members);
                done.insert(name);
            }
        }
        // Each item ends with a blank line; the file ends with one newline.
        out.truncate(out.trim_end().len());
        out.push('\n');
        Ok(out)
    }

    /// The struct of the dictionary `name` and its conversions.
    fn dictionary(&self, out: &mut String, name: &str) -> Result<(), Error> {
        let members = self.model.dict_members(name);
        let _ = writeln!(out, "/// The dictionary `{name}`.");
        out.push_str("#[derive(Clone, Debug)]\n");
        let _ = writeln!(out, "pub struct {name} {{");
        for (_, m) in &members {
            let ty = self.rust_type(&m.ty, "", false);
            let ty = if m.required
                || m.default.is_some()
                || self.nullable(&self.model.resolve(&m.ty)) && m.default.is_some()
            {
                ty
            } else {
                format!("Option<{ty}>")
            };
            let _ = writeln!(out, "    /// `{}`.", m.name);
            let _ = writeln!(out, "    pub {}: {ty},", ident(&m.name));
        }
        out.push_str("}\n\n");
        let _ = writeln!(out, "impl {name} {{");
        let _ = writeln!(
            out,
            "    /// Converts a JavaScript value (Web IDL §3.2.17)."
        );
        let body = {
            let mut body = String::new();
            let _ = writeln!(
                body,
                "        let object = b::convert::dictionary_object(value, {})?;",
                literal(name)
            );
            for (owner, m) in &members {
                let field = ident(&m.name);
                let _ = writeln!(
                    body,
                    "        let v = b::convert::member(rt, object, {})?;",
                    literal(&m.name)
                );
                let convert = format!(
                    "{}.map_err(|e| e.member({}, {}))?",
                    self.js_to_rust(&m.ty, "v", "", "realm", 0),
                    literal(&m.name),
                    literal(owner)
                );
                if m.required {
                    let _ = writeln!(
                        body,
                        "        if v.is_undefined() {{ return Err(b::convert::required_member({}, {})); }}",
                        literal(&m.name),
                        literal(owner)
                    );
                    let _ = writeln!(body, "        let {field} = {convert};");
                } else if let Some(default) = &m.default {
                    let default = self.default_value(&m.ty, default, "", "realm", &m.place)?;
                    let _ = writeln!(
                        body,
                        "        let {field} = if v.is_undefined() {{ {default} }} else {{ {convert} }};"
                    );
                } else {
                    let _ = writeln!(
                        body,
                        "        let {field} = if v.is_undefined() {{ None }} else {{ Some({convert}) }};"
                    );
                }
            }
            let fields: Vec<String> = members.iter().map(|(_, m)| ident(&m.name)).collect();
            let _ = writeln!(body, "        Ok({name} {{ {} }})", fields.join(", "));
            body
        };
        let _ = writeln!(
            out,
            "{}",
            unused_params(&format!(
                "    pub fn from_js(rt: &mut swb_js::Runtime, realm: u32, value: swb_js::Value) -> b::Result<{name}> {{\n{body}    }}\n"
            ))
        );
        out.push_str("    /// Converts to a JavaScript object (Web IDL §3.2.17).\n");
        out.push_str("    pub fn to_js<H: b::Host>(&self, rt: &mut swb_js::Runtime, realm: u32) -> swb_js::VmResult<swb_js::Value> {\n");
        out.push_str("        let object = rt.new_plain_object(realm)?;\n");
        for (_, m) in &members {
            let field = ident(&m.name);
            let owned = if self.is_copy(&m.ty) {
                format!("self.{field}")
            } else {
                format!("self.{field}.clone()")
            };
            let always = m.required || m.default.is_some();
            if always {
                let value = self.rust_to_js(&m.ty, &owned, "realm", false, 0);
                let _ = writeln!(
                    out,
                    "        let value = {value}?;\n        b::to_js::dictionary_member(rt, object, {}, value)?;",
                    literal(&m.name)
                );
            } else {
                let inner = if self.is_copy(&m.ty) {
                    "*x".to_owned()
                } else {
                    "x.clone()".to_owned()
                };
                let value = self.rust_to_js(&m.ty, &inner, "realm", false, 0);
                let _ = writeln!(
                    out,
                    "        if let Some(x) = &self.{field} {{\n            let value = {value}?;\n            b::to_js::dictionary_member(rt, object, {}, value)?;\n        }}",
                    literal(&m.name)
                );
            }
        }
        out.push_str("        Ok(object.into())\n    }\n}\n\n");
        Ok(())
    }

    fn callback_function(&self, out: &mut String, name: &str, ret: &Type, args: &[Argument]) {
        let _ = writeln!(out, "/// The callback function type `{name}`.");
        out.push_str("#[derive(Clone, Copy, Debug, PartialEq)]\n");
        let _ = writeln!(out, "pub struct {name}(pub swb_js::Gc<swb_js::Object>);\n");
        let _ = writeln!(out, "impl {name} {{");
        out.push_str("    /// Converts a JavaScript value (Web IDL §3.2.19).\n");
        let _ = writeln!(
            out,
            "    pub fn from_js(rt: &mut swb_js::Runtime, value: swb_js::Value) -> b::Result<{name}> {{\n        Ok({name}(b::convert::callback_function(rt, value)?))\n    }}\n"
        );
        out.push_str("    /// Invokes the function (Web IDL §3.11, \"invoke a callback function\") with\n    /// `this`. Call it inside a handle scope that keeps the results alive.\n");
        self.invoke_method(out, "invoke", ret, args, None);
        out.push_str("}\n\n");
    }

    fn callback_interface(&self, out: &mut String, iface: &super::Iface) {
        let name = &iface.name;
        let _ = writeln!(out, "/// The callback interface type `{name}`.");
        out.push_str("#[derive(Clone, Copy, Debug, PartialEq)]\n");
        let _ = writeln!(out, "pub struct {name}(pub swb_js::Gc<swb_js::Object>);\n");
        let _ = writeln!(out, "impl {name} {{");
        out.push_str("    /// Converts a JavaScript value (Web IDL §3.2.16).\n");
        let _ = writeln!(
            out,
            "    pub fn from_js(value: swb_js::Value) -> b::Result<{name}> {{\n        Ok({name}(b::convert::callback_interface(value)?))\n    }}\n"
        );
        for member in &iface.members {
            if let Member::Operation(op) = member
                && let Some(op_name) = &op.name
            {
                let _ = writeln!(
                    out,
                    "    /// Calls `{op_name}` (Web IDL §3.11, \"call a user object's operation\"):\n    /// the object itself if it is callable, else its `{op_name}` method.\n    /// Call it inside a handle scope that keeps the results alive."
                );
                self.invoke_method(out, &ident(op_name), &op.ret, &op.args, Some(op_name));
            }
        }
        out.push_str("}\n\n");
    }

    /// The method that calls a callback: converts the arguments, calls,
    /// converts the result.
    fn invoke_method(
        &self,
        out: &mut String,
        method: &str,
        ret: &Type,
        args: &[Argument],
        operation: Option<&str>,
    ) {
        let mut params = Vec::new();
        for arg in args {
            let ty = self.rust_type(&arg.ty, "", true);
            let ty = if arg.variadic {
                format!("&[{ty}]")
            } else if arg.optional && arg.default.is_none() {
                format!("Option<{ty}>")
            } else {
                ty
            };
            params.push(format!("{}: {ty}", ident(&arg.name)));
        }
        let ret_ty = self.rust_type(ret, "", true);
        let outer = out;
        let mut text = String::new();
        let out = &mut text;
        let _ = writeln!(
            out,
            "    pub fn {method}<H: b::Host>(self, rt: &mut swb_js::Runtime, realm: u32, this: swb_js::Value{}{}) -> b::Result<{ret_ty}> {{",
            if params.is_empty() { "" } else { ", " },
            params.join(", ")
        );
        if args.is_empty() {
            out.push_str("        let args: Vec<swb_js::Value> = Vec::new();\n");
        } else {
            out.push_str("        let mut args: Vec<swb_js::Value> = Vec::new();\n");
        }
        for arg in args {
            let name = ident(&arg.name);
            if arg.variadic {
                let item = if self.is_copy(&arg.ty) {
                    "*x"
                } else {
                    "x.clone()"
                };
                let value = self.rust_to_js(&arg.ty, item, "realm", true, 0);
                let _ = writeln!(
                    out,
                    "        for x in {name} {{\n            let value = {value}?;\n            b::keep(rt, value);\n            args.push(value);\n        }}"
                );
            } else if arg.optional && arg.default.is_none() {
                let value = self.rust_to_js(&arg.ty, "x", "realm", true, 0);
                let _ = writeln!(
                    out,
                    "        if let Some(x) = {name} {{\n            let value = {value}?;\n            b::keep(rt, value);\n            args.push(value);\n        }}"
                );
            } else {
                let value = self.rust_to_js(&arg.ty, &name, "realm", true, 0);
                let _ = writeln!(
                    out,
                    "        let value = {value}?;\n        b::keep(rt, value);\n        args.push(value);"
                );
            }
        }
        match operation {
            Some(op) => {
                let _ = writeln!(
                    out,
                    "        let result = b::glue::call_operation(rt, self.0, {}, this, &args)?;",
                    literal(op)
                );
            }
            None => out.push_str("        let result = rt.call(self.0.into(), this, &args)?;\n"),
        }
        let t = self.model.resolve(ret);
        if matches!(t.kind, TypeKind::Undefined) {
            out.push_str("        let _ = result;\n        Ok(())\n    }\n");
        } else {
            let convert = self.js_to_rust(ret, "result", "", "realm", 0);
            let _ = writeln!(out, "        {convert}\n    }}");
        }
        let _ = writeln!(outer, "{}", unused_params(&text));
    }

    fn union_item(&self, out: &mut String, name: &str, members: &[Type]) {
        let idl = self.idl_name(&Type {
            kind: TypeKind::Union(members.to_vec()),
            nullable: false,
            ext_attrs: crate::ast::ExtAttrs::default(),
        });
        let _ = writeln!(out, "/// The union type `{idl}`.");
        out.push_str("#[derive(Clone, Debug)]\n");
        let _ = writeln!(out, "pub enum {name} {{");
        for m in members {
            let _ = writeln!(out, "    /// `{}`.", self.idl_name(m));
            let _ = writeln!(
                out,
                "    {}({}),",
                self.variant_name(m),
                self.rust_type(m, "", false)
            );
        }
        out.push_str("}\n\n");
        let _ = writeln!(out, "impl {name} {{");
        out.push_str("    /// Converts a JavaScript value (Web IDL §3.2.25).\n");
        let body = self.union_from_js(name, members, &idl);
        let _ = writeln!(
            out,
            "{}",
            unused_params(&format!(
                "    pub fn from_js(rt: &mut swb_js::Runtime, realm: u32, value: swb_js::Value) -> b::Result<{name}> {{\n{body}    }}\n"
            ))
        );
        out.push_str("    /// Converts to a JavaScript value.\n");
        let mut text = String::from(
            "    pub fn to_js<H: b::Host>(&self, rt: &mut swb_js::Runtime, realm: u32) -> swb_js::VmResult<swb_js::Value> {\n        match self {\n",
        );
        for m in members {
            let owned = if self.is_copy(m) { "*x" } else { "x.clone()" };
            let value = self.rust_to_js(m, owned, "realm", false, 0);
            let binding = if matches!(m.kind, TypeKind::Undefined) {
                "()"
            } else {
                "x"
            };
            let _ = writeln!(
                text,
                "            {name}::{}({binding}) => {value},",
                self.variant_name(m)
            );
        }
        text.push_str("        }\n    }\n");
        out.push_str(&unused_params(&text));
        out.push_str("}\n\n");
    }

    /// The steps of Web IDL §3.2.25 for the member types of a union.
    fn union_from_js(&self, name: &str, members: &[Type], idl: &str) -> String {
        let mut body = String::new();
        let kind_of = |m: &Type| match &m.kind {
            TypeKind::Named(n) => self.model.kind(n),
            _ => None,
        };
        let variant = |m: &Type, convert: String| {
            format!("return Ok({name}::{}({convert}?));", self.variant_name(m))
        };
        let find = |test: &dyn Fn(&Type) -> bool| members.iter().find(|m| test(m));
        let dictionary = find(&|m| kind_of(m) == Some(Kind::Dictionary));
        let object = find(&|m| matches!(m.kind, TypeKind::Object));
        if let Some(u) = find(&|m| matches!(m.kind, TypeKind::Undefined)) {
            let _ = writeln!(
                body,
                "        if value.is_undefined() {{ return Ok({name}::{}(())); }}",
                self.variant_name(u)
            );
        }
        if let Some(d) = dictionary {
            let _ = writeln!(
                body,
                "        if value.is_nullish() {{ {} }}",
                variant(d, self.js_to_rust(d, "value", "", "realm", 0))
            );
        }
        let interfaces: Vec<&Type> = members
            .iter()
            .filter(|m| kind_of(m) == Some(Kind::Interface))
            .collect();
        if !interfaces.is_empty() || object.is_some() {
            body.push_str("        if b::convert::is_platform_object(rt, value) {\n");
            for i in &interfaces {
                let TypeKind::Named(n) = &i.kind else {
                    continue;
                };
                let _ = writeln!(
                    body,
                    "            if rt.implements(value, super::{}) {{ {} }}",
                    constant(n),
                    variant(i, self.js_to_rust(i, "value", "", "realm", 0))
                );
            }
            if let Some(o) = object {
                let _ = writeln!(
                    body,
                    "            {}",
                    variant(o, self.js_to_rust(o, "value", "", "realm", 0))
                );
            }
            body.push_str("        }\n");
        }
        let callback = find(&|m| kind_of(m) == Some(Kind::Callback));
        if callback.is_some() || object.is_some() {
            body.push_str("        if rt.callable(value)? {\n");
            if let Some(c) = callback {
                let _ = writeln!(
                    body,
                    "            {}",
                    variant(c, self.js_to_rust(c, "value", "", "realm", 0))
                );
            }
            if let Some(o) = object {
                let _ = writeln!(
                    body,
                    "            {}",
                    variant(o, self.js_to_rust(o, "value", "", "realm", 0))
                );
            }
            body.push_str("        }\n");
        }
        self.union_object_step(&mut body, name, members);
        let boolean = find(&|m| matches!(m.kind, TypeKind::Boolean));
        let numeric = find(&|m| matches!(m.kind, TypeKind::Integer(_) | TypeKind::Float(_)));
        let string = find(&|m| {
            matches!(
                m.kind,
                TypeKind::DomString | TypeKind::UsvString | TypeKind::ByteString
            ) || kind_of(m) == Some(Kind::Enum)
        });
        if let Some(b) = boolean {
            let _ = writeln!(
                body,
                "        if let swb_js::Value::Bool(_) = value {{ {} }}",
                variant(b, self.js_to_rust(b, "value", "", "realm", 0))
            );
        }
        if let Some(n) = numeric {
            let _ = writeln!(
                body,
                "        if let swb_js::Value::Int(_) | swb_js::Value::Double(_) = value {{ {} }}",
                variant(n, self.js_to_rust(n, "value", "", "realm", 0))
            );
        }
        let fallback = string.or(numeric).or(boolean);
        match fallback {
            Some(m) => {
                let _ = writeln!(
                    body,
                    "        Ok({name}::{}({}?))",
                    self.variant_name(m),
                    self.js_to_rust(m, "value", "", "realm", 0)
                );
            }
            None => {
                let _ = writeln!(
                    body,
                    "        Err(b::Exception::NotOfType({}.into()))",
                    literal(idl)
                );
            }
        }
        body
    }

    /// The object step of a union conversion (Web IDL §3.2.25): a sequence
    /// if the object has an `@@iterator`, then a dictionary, a record, a
    /// callback interface or `object`.
    fn union_object_step(&self, body: &mut String, name: &str, members: &[Type]) {
        let kind_of = |m: &Type| match &m.kind {
            TypeKind::Named(n) => self.model.kind(n),
            _ => None,
        };
        let find = |test: &dyn Fn(&Type) -> bool| members.iter().find(|m| test(m));
        let sequence =
            find(&|m| matches!(m.kind, TypeKind::Sequence(_) | TypeKind::FrozenArray(_)));
        let others = [
            find(&|m| kind_of(m) == Some(Kind::Dictionary)),
            find(&|m| matches!(m.kind, TypeKind::Record(..))),
            find(&|m| kind_of(m) == Some(Kind::CallbackInterface)),
            find(&|m| matches!(m.kind, TypeKind::Object)),
        ];
        if sequence.is_none() && others.iter().all(Option::is_none) {
            return;
        }
        body.push_str("        if let swb_js::Value::Object(_) = value {\n");
        if let Some(s) = sequence
            && let TypeKind::Sequence(inner) | TypeKind::FrozenArray(inner) = &s.kind
        {
            let convert = self.js_to_rust(inner, "item0", "", "realm", 1);
            let _ = writeln!(
                body,
                "            if let Some(method) = b::convert::iterator_method(rt, value)? {{\n                return Ok({name}::{}(b::convert::sequence_from_method(rt, value, method, |rt: &mut swb_js::Runtime, item0: swb_js::Value| {convert})?));\n            }}",
                self.variant_name(s)
            );
        }
        for m in others.into_iter().flatten() {
            let _ = writeln!(
                body,
                "            return Ok({name}::{}({}?));",
                self.variant_name(m),
                self.js_to_rust(m, "value", "", "realm", 0)
            );
        }
        body.push_str("        }\n");
    }
}

/// An enumeration: a Rust enum with its values.
fn enumeration(out: &mut String, name: &str, values: &[String]) {
    let _ = writeln!(out, "/// The enumeration `{name}`.");
    out.push_str("#[derive(Clone, Copy, Debug, PartialEq, Eq)]\n");
    let _ = writeln!(out, "pub enum {name} {{");
    for v in values {
        let _ = writeln!(out, "    /// `{}`.", literal(v));
        let _ = writeln!(out, "    {},", camel(v));
    }
    out.push_str("}\n\n");
    let _ = writeln!(out, "impl {name} {{");
    out.push_str("    /// The values in order.\n");
    let list: Vec<String> = values.iter().map(|v| literal(v)).collect();
    let _ = writeln!(
        out,
        "    pub const VALUES: [&'static str; {}] = [{}];\n",
        values.len(),
        list.join(", ")
    );
    out.push_str("    /// The value as a string.\n");
    let _ = writeln!(
        out,
        "    pub fn as_str(self) -> &'static str {{\n        match self {{"
    );
    for v in values {
        let _ = writeln!(out, "            {name}::{} => {},", camel(v), literal(v));
    }
    out.push_str("        }\n    }\n\n");
    out.push_str("    /// Converts a JavaScript value (Web IDL §3.2.18).\n");
    let _ = writeln!(
        out,
        "    pub fn from_js(rt: &mut swb_js::Runtime, value: swb_js::Value) -> b::Result<{name}> {{\n        let index = b::convert::enumeration(rt, value, &{name}::VALUES, {})?;\n        Ok(match index {{",
        literal(name)
    );
    for (i, v) in values.iter().enumerate() {
        if i + 1 == values.len() {
            let _ = writeln!(out, "            _ => {name}::{},", camel(v));
        } else {
            let _ = writeln!(out, "            {i} => {name}::{},", camel(v));
        }
    }
    out.push_str("        })\n    }\n}\n\n");
}

/// A function's text with `_` before the parameters `rt` and `realm` of
/// its first line if its body does not use them.
fn unused_params(text: &str) -> String {
    let (head, body) = text.split_once('\n').unwrap_or((text, ""));
    let mut head = head.to_owned();
    for name in ["rt", "realm"] {
        if !uses_word(body, name) {
            head = head.replacen(&format!("{name}: "), &format!("_{name}: "), 1);
        }
    }
    format!("{head}\n{body}")
}

/// Whether `word` occurs in `text` as a whole identifier.
fn uses_word(text: &str, word: &str) -> bool {
    let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    text.match_indices(word).any(|(i, _)| {
        let before = text.get(..i).and_then(|t| t.chars().next_back());
        let after = text.get(i + word.len()..).and_then(|t| t.chars().next());
        !before.is_some_and(is_ident) && !after.is_some_and(is_ident)
    })
}
