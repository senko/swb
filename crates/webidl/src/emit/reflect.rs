//! The glue of reflected attributes (`[Reflect]`, `[ReflectURL]`, `[ReflectNonNegative]`,
//! `[ReflectPositive]`, `[ReflectPositiveWithFallback]`, `[ReflectSetter]`, `[ReflectDefault]`,
//! `[ReflectRange]`, `[SwbKeywords]`): the call of the getter and of the setter in
//! `b::reflect` for the type of the attribute, and the attributes that name its
//! parameters.

use crate::Error;
use crate::ast::{Attribute, ExtValue, TypeKind};

use super::Gen;
use super::iface::err;
use super::names::literal;

impl Gen<'_> {
    /// The getter call of a reflected attribute (an expression with the
    /// placeholder of the context `CX` and `this`).
    pub(super) fn reflect_get(&self, attr: &Attribute, kind: Reflect) -> Result<String, Error> {
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
    pub(super) fn reflect_set(&self, attr: &Attribute, kind: Reflect) -> Result<String, Error> {
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
}

/// The variants of reflection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Reflect {
    Plain,
    Url,
    NonNegative,
    Positive,
    PositiveWithFallback,
}

pub(super) fn reflect_kind(attr: &Attribute) -> Option<Reflect> {
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
pub(super) fn content_attribute(attr: &Attribute) -> String {
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
pub(super) fn unsigned_limit(attr: &Attribute, kind: Reflect) -> Result<String, Error> {
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
