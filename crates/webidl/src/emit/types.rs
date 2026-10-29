//! The Rust side of IDL types: the Rust type of each IDL type, the
//! expressions that convert a JavaScript value to it and back (Web IDL
//! §3.2), default values, the overload test of a type, and the file
//! `types.rs` with the dictionaries, enumerations, unions and callbacks
//! of a set.

use crate::Error;
use crate::ast::{ConstValue, FloatType, IntegerType, Type, TypeKind};
use crate::model::Kind;

use super::Gen;
use super::names::{camel, constant, ident, literal};

/// `swb_js::Gc<swb_js::Object>`.
pub(crate) const GC_OBJECT: &str = "swb_js::Gc<swb_js::Object>";

impl Gen<'_> {
    /// The Rust type of an IDL type. `p` is the path prefix of the types of
    /// `types.rs`; `out` selects the result form of other interfaces (an
    /// object reference instead of a [`Platform`]).
    pub(crate) fn rust_type(&self, ty: &Type, p: &str, out: bool) -> String {
        let t = self.model.resolve(ty);
        let base = match &t.kind {
            TypeKind::Any => return "swb_js::Value".to_owned(),
            TypeKind::Undefined => "()".to_owned(),
            TypeKind::Boolean => "bool".to_owned(),
            TypeKind::Integer(k) => integer_type(*k).to_owned(),
            TypeKind::Float(FloatType::Float | FloatType::UnrestrictedFloat) => "f32".to_owned(),
            TypeKind::Float(_) => "f64".to_owned(),
            TypeKind::DomString | TypeKind::UsvString | TypeKind::ByteString => "String".to_owned(),
            TypeKind::Object | TypeKind::Promise(_) => GC_OBJECT.to_owned(),
            TypeKind::Named(name) => match self.model.kind(name) {
                Some(Kind::Interface) if self.model.is_node(name) => "b::NodeId".to_owned(),
                Some(Kind::Interface) if out => GC_OBJECT.to_owned(),
                Some(Kind::Interface) => "b::Platform".to_owned(),
                _ => format!("{p}{name}"),
            },
            TypeKind::Sequence(inner) | TypeKind::FrozenArray(inner) => {
                format!("Vec<{}>", self.rust_type(inner, p, out))
            }
            TypeKind::Record(_, value) => {
                format!("Vec<(String, {})>", self.rust_type(value, p, out))
            }
            TypeKind::Union(_) => format!("{p}{}", self.union(&t)),
            TypeKind::BigInt | TypeKind::Symbol => "swb_js::Value".to_owned(),
        };
        if self.nullable(&t) {
            format!("Option<{base}>")
        } else {
            base
        }
    }

    /// Whether a resolved type converts `null` and `undefined` to `None`
    /// (a nullable type, or a union with a nullable member).
    pub(crate) fn nullable(&self, t: &Type) -> bool {
        if matches!(t.kind, TypeKind::Any) {
            return false;
        }
        t.nullable || matches!(&t.kind, TypeKind::Union(members) if self.flatten(members).1)
    }

    /// Whether the Rust type of an IDL type is `Copy`.
    pub(crate) fn is_copy(&self, ty: &Type) -> bool {
        let t = self.model.resolve(ty);
        match &t.kind {
            TypeKind::Named(name) => !matches!(self.model.kind(name), Some(Kind::Dictionary)),
            TypeKind::DomString
            | TypeKind::UsvString
            | TypeKind::ByteString
            | TypeKind::Sequence(_)
            | TypeKind::FrozenArray(_)
            | TypeKind::Record(..)
            | TypeKind::Union(_) => false,
            _ => true,
        }
    }

    /// The flattened member types of a union (Web IDL §2.13.32) without
    /// their nullability, and whether a member was nullable.
    pub(crate) fn flatten(&self, members: &[Type]) -> (Vec<Type>, bool) {
        let mut flat = Vec::new();
        let mut nullable = false;
        for member in members {
            let mut t = self.model.resolve(member);
            nullable |= t.nullable;
            t.nullable = false;
            if let TypeKind::Union(inner) = &t.kind {
                let (inner, inner_nullable) = self.flatten(inner);
                nullable |= inner_nullable;
                flat.extend(inner);
            } else {
                flat.push(t);
            }
        }
        (flat, nullable)
    }

    /// The Rust name of a union type, registered for `types.rs`.
    pub(crate) fn union(&self, t: &Type) -> String {
        let TypeKind::Union(members) = &t.kind else {
            return String::new();
        };
        let (flat, _) = self.flatten(members);
        let name = flat
            .iter()
            .map(|m| self.variant_name(m))
            .collect::<Vec<_>>()
            .join("Or");
        self.unions.borrow_mut().entry(name.clone()).or_insert(flat);
        name
    }

    /// The variant of a member type in a union.
    pub(crate) fn variant_name(&self, t: &Type) -> String {
        match &t.kind {
            TypeKind::Any => "Any".to_owned(),
            TypeKind::Undefined => "Undefined".to_owned(),
            TypeKind::Boolean => "Boolean".to_owned(),
            TypeKind::Integer(k) => camel(integer_idl(*k)),
            TypeKind::Float(k) => camel(float_idl(*k)),
            TypeKind::DomString => "DomString".to_owned(),
            TypeKind::UsvString => "UsvString".to_owned(),
            TypeKind::ByteString => "ByteString".to_owned(),
            TypeKind::Object => "Object".to_owned(),
            TypeKind::Named(name) => name.clone(),
            TypeKind::Sequence(inner) | TypeKind::FrozenArray(inner) => {
                format!("{}Sequence", self.variant_name(&self.model.resolve(inner)))
            }
            TypeKind::Record(_, value) => {
                format!("{}Record", self.variant_name(&self.model.resolve(value)))
            }
            TypeKind::Promise(_) => "Promise".to_owned(),
            TypeKind::Union(_) => "Union".to_owned(),
            TypeKind::BigInt => "BigInt".to_owned(),
            TypeKind::Symbol => "Symbol".to_owned(),
        }
    }

    /// The IDL name of a type as written, with `?` (for documentation).
    pub(crate) fn idl_text(&self, t: &Type) -> String {
        let name = self.idl_name(t);
        if t.nullable { format!("{name}?") } else { name }
    }

    /// The IDL name of a type for messages ("Node", "(A or B)").
    pub(crate) fn idl_name(&self, t: &Type) -> String {
        match &t.kind {
            TypeKind::Any => "any".to_owned(),
            TypeKind::Undefined => "undefined".to_owned(),
            TypeKind::Boolean => "boolean".to_owned(),
            TypeKind::Integer(k) => integer_idl(*k).to_owned(),
            TypeKind::Float(k) => float_idl(*k).to_owned(),
            TypeKind::DomString => "DOMString".to_owned(),
            TypeKind::UsvString => "USVString".to_owned(),
            TypeKind::ByteString => "ByteString".to_owned(),
            TypeKind::Object => "object".to_owned(),
            TypeKind::Named(name) => name.clone(),
            TypeKind::Sequence(inner) => format!("sequence<{}>", self.idl_name(inner)),
            TypeKind::FrozenArray(inner) => format!("FrozenArray<{}>", self.idl_name(inner)),
            TypeKind::Record(key, value) => {
                format!("record<{}, {}>", self.idl_name(key), self.idl_name(value))
            }
            TypeKind::Promise(inner) => format!("Promise<{}>", self.idl_name(inner)),
            TypeKind::Union(members) => {
                let (flat, _) = self.flatten(members);
                let mut names: Vec<String> = flat.iter().map(|m| self.idl_name(m)).collect();
                names.sort();
                format!("({})", names.join(" or "))
            }
            TypeKind::BigInt => "bigint".to_owned(),
            TypeKind::Symbol => "symbol".to_owned(),
        }
    }

    // --- JavaScript to Rust ---

    /// An expression of type `b::Result<T>` that converts the value in the
    /// variable `v` (with `rt` and the realm expression `realm` in scope).
    pub(crate) fn js_to_rust(
        &self,
        ty: &Type,
        v: &str,
        p: &str,
        realm: &str,
        depth: usize,
    ) -> String {
        let t = self.model.resolve(ty);
        let base = self.js_to_rust_base(&t, v, p, realm, depth);
        if self.nullable(&t) {
            format!("(if {v}.is_nullish() {{ Ok(None) }} else {{ {base}.map(Some) }})")
        } else {
            base
        }
    }

    fn js_to_rust_base(&self, t: &Type, v: &str, p: &str, realm: &str, depth: usize) -> String {
        let int_mode = if t.ext_attrs.has("Clamp") {
            "b::convert::IntMode::Clamp"
        } else if t.ext_attrs.has("EnforceRange") {
            "b::convert::IntMode::EnforceRange"
        } else {
            "b::convert::IntMode::Plain"
        };
        match &t.kind {
            TypeKind::Any | TypeKind::BigInt | TypeKind::Symbol => {
                format!("b::Result::Ok({v})")
            }
            TypeKind::Undefined => "b::Result::Ok(())".to_owned(),
            TypeKind::Boolean => format!("b::convert::boolean(rt, {v})"),
            TypeKind::Integer(k) => {
                format!(
                    "b::convert::{}(rt, {v}, {int_mode})",
                    ident(integer_idl(*k))
                )
            }
            TypeKind::Float(k) => format!("b::convert::{}(rt, {v})", ident(float_idl(*k))),
            TypeKind::DomString if t.ext_attrs.has("LegacyNullToEmptyString") => {
                format!("b::convert::dom_string_null_empty(rt, {v})")
            }
            TypeKind::DomString => format!("b::convert::dom_string(rt, {v})"),
            TypeKind::UsvString => format!("b::convert::usv_string(rt, {v})"),
            TypeKind::ByteString => format!("b::convert::byte_string(rt, {v})"),
            TypeKind::Object => format!("b::convert::object({v})"),
            TypeKind::Promise(_) => format!("b::convert::promise(rt, {realm}, {v})"),
            TypeKind::Named(name) => match self.model.kind(name) {
                Some(Kind::Interface) if self.model.is_node(name) => format!(
                    "b::convert::node(rt, {v}, super::{}, {})",
                    constant(name),
                    literal(name)
                ),
                Some(Kind::Interface) => format!(
                    "b::convert::platform(rt, {v}, super::{}, {})",
                    constant(name),
                    literal(name)
                ),
                Some(Kind::Dictionary) => format!("{p}{name}::from_js(rt, {realm}, {v})"),
                Some(Kind::Enum | Kind::Callback) => format!("{p}{name}::from_js(rt, {v})"),
                Some(Kind::CallbackInterface) => format!("{p}{name}::from_js({v})"),
                None => "b::Result::Ok(())".to_owned(),
            },
            TypeKind::Sequence(inner) | TypeKind::FrozenArray(inner) => {
                let item = format!("item{depth}");
                let convert = self.js_to_rust(inner, &item, p, realm, depth + 1);
                format!(
                    "b::convert::sequence(rt, {v}, |rt: &mut swb_js::Runtime, {item}: swb_js::Value| {convert})"
                )
            }
            TypeKind::Record(key, value) => {
                let k = format!("key{depth}");
                let item = format!("item{depth}");
                let convert_key = self.js_to_rust(key, &k, p, realm, depth + 1);
                let convert_value = self.js_to_rust(value, &item, p, realm, depth + 1);
                format!(
                    "b::convert::record(rt, {v}, |rt: &mut swb_js::Runtime, {k}: swb_js::Value| {convert_key}, |rt: &mut swb_js::Runtime, {item}: swb_js::Value| {convert_value})"
                )
            }
            TypeKind::Union(_) => format!("{p}{}::from_js(rt, {realm}, {v})", self.union(t)),
        }
    }

    // --- Rust to JavaScript ---

    /// An expression of type `swb_js::VmResult<swb_js::Value>` that converts
    /// the owned value of the expression `e` (with `rt`, `realm` and the
    /// host type `H` in scope). `out`: the result form of other
    /// interfaces.
    pub(crate) fn rust_to_js(
        &self,
        ty: &Type,
        e: &str,
        realm: &str,
        out: bool,
        depth: usize,
    ) -> String {
        let t = self.model.resolve(ty);
        if self.nullable(&t) {
            let x = format!("x{depth}");
            let inner = self.rust_to_js_base(&t, &x, realm, out, depth + 1);
            return format!(
                "(match {e} {{ Some({x}) => {inner}, None => swb_js::VmResult::Ok(swb_js::Value::Null) }})"
            );
        }
        self.rust_to_js_base(&t, e, realm, out, depth)
    }

    fn rust_to_js_base(&self, t: &Type, e: &str, realm: &str, out: bool, depth: usize) -> String {
        match &t.kind {
            TypeKind::Any | TypeKind::BigInt | TypeKind::Symbol => {
                format!("swb_js::VmResult::Ok({e})")
            }
            TypeKind::Undefined => "swb_js::VmResult::Ok(swb_js::Value::Undefined)".to_owned(),
            TypeKind::Boolean => format!("swb_js::VmResult::Ok(swb_js::Value::Bool({e}))"),
            TypeKind::Integer(IntegerType::LongLong | IntegerType::UnsignedLongLong) => {
                format!("swb_js::VmResult::Ok(swb_js::Value::number({e} as f64))")
            }
            TypeKind::Integer(_)
            | TypeKind::Float(FloatType::Float | FloatType::UnrestrictedFloat) => {
                format!("swb_js::VmResult::Ok(swb_js::Value::number(f64::from({e})))")
            }
            TypeKind::Float(_) => format!("swb_js::VmResult::Ok(swb_js::Value::number({e}))"),
            TypeKind::DomString | TypeKind::UsvString | TypeKind::ByteString => {
                format!("b::to_js::string(rt, &{e})")
            }
            TypeKind::Object | TypeKind::Promise(_) => {
                format!("swb_js::VmResult::Ok(swb_js::Value::Object({e}))")
            }
            TypeKind::Named(name) => match self.model.kind(name) {
                Some(Kind::Interface) if self.model.is_node(name) => {
                    format!("b::to_js::node::<H>(rt, {realm}, {e})")
                }
                Some(Kind::Interface) if out => {
                    format!("swb_js::VmResult::Ok(swb_js::Value::Object({e}))")
                }
                Some(Kind::Interface) => format!(
                    "swb_js::VmResult::Ok(swb_js::Value::Object({}.object))",
                    recv(e)
                ),
                Some(Kind::Dictionary) => format!("{}.to_js::<H>(rt, {realm})", recv(e)),
                Some(Kind::Enum) => format!("b::to_js::string(rt, {}.as_str())", recv(e)),
                Some(Kind::Callback | Kind::CallbackInterface) => {
                    format!("swb_js::VmResult::Ok(swb_js::Value::Object({}.0))", recv(e))
                }
                None => "swb_js::VmResult::Ok(swb_js::Value::Undefined)".to_owned(),
            },
            TypeKind::Sequence(inner) | TypeKind::FrozenArray(inner) => {
                let item = format!("item{depth}");
                let owned = if self.is_copy(inner) {
                    format!("*{item}")
                } else {
                    format!("{item}.clone()")
                };
                let convert = self.rust_to_js(inner, &owned, realm, out, depth + 1);
                format!(
                    "b::to_js::sequence(rt, {realm}, &{e}, |{}, {item}| {convert})",
                    rt_param(&convert)
                )
            }
            TypeKind::Record(_, value) => {
                let item = format!("item{depth}");
                let owned = if self.is_copy(value) {
                    format!("*{item}")
                } else {
                    format!("{item}.clone()")
                };
                let convert = self.rust_to_js(value, &owned, realm, out, depth + 1);
                format!(
                    "b::to_js::record(rt, {realm}, &{e}, |{}, {item}| {convert})",
                    rt_param(&convert)
                )
            }
            TypeKind::Union(_) => format!("{}.to_js::<H>(rt, {realm})", recv(e)),
        }
    }

    // --- Default values ---

    /// An expression of the Rust type of `ty` for a default value (it may
    /// use `?`, `rt` and `realm`).
    pub(crate) fn default_value(
        &self,
        ty: &Type,
        value: &ConstValue,
        p: &str,
        realm: &str,
        place: &crate::ast::Place,
    ) -> Result<String, Error> {
        let t = self.model.resolve(ty);
        let fail = || {
            Err(Error {
                file: place.file.clone(),
                line: place.line,
                message: format!(
                    "the default value does not fit the type {}",
                    self.idl_name(&t)
                ),
            })
        };
        let nullable = self.nullable(&t);
        if *value == ConstValue::Null {
            return match t.kind {
                TypeKind::Any => Ok("swb_js::Value::Null".to_owned()),
                _ if nullable => Ok("None".to_owned()),
                _ => fail(),
            };
        }
        let wrap = |text: String| {
            if nullable {
                format!("Some({text})")
            } else {
                text
            }
        };
        let js_value = match value {
            ConstValue::Undefined | ConstValue::EmptyDictionary => {
                "swb_js::Value::Undefined".to_owned()
            }
            ConstValue::EmptySequence => {
                format!("swb_js::Value::Object(rt.new_array_of({realm}, &[])?)")
            }
            ConstValue::Bool(b) => format!("swb_js::Value::Bool({b})"),
            ConstValue::Integer(i) => format!("swb_js::Value::number({i}_f64)"),
            ConstValue::Float(f) => format!("swb_js::Value::number({})", float_literal(f, "f64")),
            ConstValue::String(s) => format!("rt.string_value({})?", literal(s)),
            ConstValue::Null => "swb_js::Value::Null".to_owned(),
        };
        let direct = match (&t.kind, value) {
            (TypeKind::Any, _) => Some(js_value.clone()),
            (TypeKind::Boolean, ConstValue::Bool(b)) => Some(b.to_string()),
            (TypeKind::Integer(_), ConstValue::Integer(i)) => Some(i.to_string()),
            (TypeKind::Float(k), ConstValue::Integer(i)) => {
                Some(format!("{i}.0_{}", float_suffix(*k)))
            }
            (TypeKind::Float(k), ConstValue::Float(f)) => Some(float_literal(f, float_suffix(*k))),
            (
                TypeKind::DomString | TypeKind::UsvString | TypeKind::ByteString,
                ConstValue::String(s),
            ) => Some(if s.is_empty() {
                "String::new()".to_owned()
            } else {
                format!("String::from({})", literal(s))
            }),
            (TypeKind::Sequence(_) | TypeKind::FrozenArray(_), ConstValue::EmptySequence) => {
                Some("Vec::new()".to_owned())
            }
            (TypeKind::Named(name), ConstValue::String(s))
                if self.model.kind(name) == Some(Kind::Enum) =>
            {
                let Some(e) = self.model.enumeration(name) else {
                    return fail();
                };
                if !e.values.contains(s) {
                    return fail();
                }
                Some(format!("{p}{name}::{}", camel(s)))
            }
            (TypeKind::Named(name), ConstValue::EmptyDictionary)
                if self.model.kind(name) == Some(Kind::Dictionary) =>
            {
                return Ok(format!(
                    "{p}{name}::from_js(rt, {realm}, swb_js::Value::Undefined)?"
                ));
            }
            (TypeKind::Union(_), _) => {
                // The value is made first: it can borrow the runtime.
                return Ok(format!(
                    "{{ let d = {js_value}; {p}{}::from_js(rt, {realm}, d)? }}",
                    self.union(&t)
                ));
            }
            _ => None,
        };
        match direct {
            Some(text) if matches!(t.kind, TypeKind::Any) => Ok(text),
            Some(text) => Ok(wrap(text)),
            None => fail(),
        }
    }

    // --- Overload resolution ---

    /// The `b::overload::Candidate` of an argument's type.
    pub(crate) fn candidate(&self, ty: &Type, optional: bool) -> String {
        let t = self.model.resolve(ty);
        let (members, nullable) = match &t.kind {
            TypeKind::Union(members) => {
                let (flat, nullable) = self.flatten(members);
                (flat, nullable || t.nullable)
            }
            _ => (vec![t.clone()], t.nullable),
        };
        let mut flags = Vec::new();
        let mut interfaces = Vec::new();
        if optional {
            flags.push("optional");
        }
        let mut dictionary = false;
        for m in &members {
            match &m.kind {
                TypeKind::Named(name) => match self.model.kind(name) {
                    Some(Kind::Interface) => interfaces.push(format!("super::{}", constant(name))),
                    Some(Kind::Dictionary) => {
                        dictionary = true;
                        flags.push("object_like");
                    }
                    Some(Kind::CallbackInterface) => flags.push("object_like"),
                    Some(Kind::Callback) => flags.push("callback_function"),
                    Some(Kind::Enum) => flags.push("string"),
                    None => {}
                },
                TypeKind::Record(..) => flags.push("object_like"),
                TypeKind::Object => flags.push("object"),
                TypeKind::Sequence(_) | TypeKind::FrozenArray(_) => flags.push("sequence"),
                TypeKind::Boolean => flags.push("boolean"),
                TypeKind::Integer(_) | TypeKind::Float(_) => flags.push("numeric"),
                TypeKind::DomString | TypeKind::UsvString | TypeKind::ByteString => {
                    flags.push("string");
                }
                TypeKind::Any => flags.push("any"),
                _ => {}
            }
        }
        if nullable || dictionary {
            flags.push("nullable_or_dictionary");
        }
        flags.sort_unstable();
        flags.dedup();
        let mut fields: Vec<String> = flags.iter().map(|f| format!("{f}: true")).collect();
        if !interfaces.is_empty() {
            fields.push(format!("interfaces: &[{}]", interfaces.join(", ")));
        }
        fields.push("..b::overload::Candidate::default()".to_owned());
        format!("b::overload::Candidate {{ {} }}", fields.join(", "))
    }
}

/// The Rust integer type of an IDL integer type.
pub(crate) fn integer_type(k: IntegerType) -> &'static str {
    match k {
        IntegerType::Byte => "i8",
        IntegerType::Octet => "u8",
        IntegerType::Short => "i16",
        IntegerType::UnsignedShort => "u16",
        IntegerType::Long => "i32",
        IntegerType::UnsignedLong => "u32",
        IntegerType::LongLong => "i64",
        IntegerType::UnsignedLongLong => "u64",
    }
}

/// The IDL name of an integer type.
pub(crate) fn integer_idl(k: IntegerType) -> &'static str {
    match k {
        IntegerType::Byte => "byte",
        IntegerType::Octet => "octet",
        IntegerType::Short => "short",
        IntegerType::UnsignedShort => "unsigned short",
        IntegerType::Long => "long",
        IntegerType::UnsignedLong => "unsigned long",
        IntegerType::LongLong => "long long",
        IntegerType::UnsignedLongLong => "unsigned long long",
    }
}

/// The IDL name of a floating-point type.
pub(crate) fn float_idl(k: FloatType) -> &'static str {
    match k {
        FloatType::Float => "float",
        FloatType::UnrestrictedFloat => "unrestricted float",
        FloatType::Double => "double",
        FloatType::UnrestrictedDouble => "unrestricted double",
    }
}

fn float_suffix(k: FloatType) -> &'static str {
    match k {
        FloatType::Float | FloatType::UnrestrictedFloat => "f32",
        FloatType::Double | FloatType::UnrestrictedDouble => "f64",
    }
}

/// An expression as the receiver of a field or method: in parentheses
/// unless it is a plain name.
fn recv(e: &str) -> String {
    if e.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        e.to_owned()
    } else {
        format!("({e})")
    }
}

/// The name of the runtime parameter of a closure: `_rt` if `body` does
/// not use it.
fn rt_param(body: &str) -> &'static str {
    if body.contains("(rt") || body.contains(" rt,") || body.contains("rt.") {
        "rt"
    } else {
        "_rt"
    }
}

/// A Rust floating-point literal of an IDL decimal (or `Infinity`,
/// `-Infinity`, `NaN`).
fn float_literal(text: &str, suffix: &str) -> String {
    match text {
        "Infinity" => format!("{suffix}::INFINITY"),
        "-Infinity" => format!("{suffix}::NEG_INFINITY"),
        "NaN" => format!("{suffix}::NAN"),
        _ if text.contains(['.', 'e', 'E']) => format!("{text}_{suffix}"),
        _ => format!("{text}.0_{suffix}"),
    }
}
