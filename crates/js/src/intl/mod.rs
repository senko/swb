//! `Intl` (ECMA-402, <https://tc39.es/ecma402/>): the formatting core of
//! `Intl.DateTimeFormat`, `Intl.NumberFormat` and `Intl.RelativeTimeFormat`
//! for the locales `en`, `en-US` and `en-GB` (ADR 0027 section 10). The
//! code does not depend on the heap or the interpreter; the built-in
//! objects are in `builtins/intl*.rs`.
//!
//! - `locale`: language tags and the negotiation of the locale.
//! - `pattern`: date-time patterns, skeletons and their matching.
//! - `datetime`, `number`, `relative`: the three formats.
//! - `data` (generated from CLDR 48 by `swbtools js-intl-data`) and
//!   `tables` (its types).

#[rustfmt::skip]
mod data;
pub(crate) mod datetime;
pub(crate) mod locale;
pub(crate) mod number;
pub(crate) mod pattern;
pub(crate) mod relative;
mod tables;

use crate::heap::Gc;
use crate::object::Object;

/// The canonical identifier of CLDR for a time zone name in any case
/// (`US/Eastern` and `Asia/Kolkata` give `America/New_York` and
/// `Asia/Calcutta`), `None` for a name that CLDR does not have.
pub(crate) fn canonical_zone(name: &str) -> Option<&'static str> {
    if let Some((_, canonical)) = data::ZONE_CANONICAL
        .iter()
        .find(|(alias, _)| alias.eq_ignore_ascii_case(name))
    {
        return Some(canonical);
    }
    crate::date::canonical_case(name)
}

/// The format that an `Intl` object holds.
#[derive(Clone, Debug)]
pub(crate) enum IntlData {
    /// `Intl.DateTimeFormat`.
    DateTime(datetime::DateTimeFormat),
    /// `Intl.NumberFormat`.
    Number(number::NumberFormat),
    /// `Intl.RelativeTimeFormat`.
    Relative(relative::RelativeTimeFormat),
}

/// The payload of an `Intl` object.
#[derive(Debug)]
pub struct IntlObject {
    /// The format.
    pub(crate) data: IntlData,
    /// The bound `format` function of a date-time or number format
    /// (`[[BoundFormat]]`), made at the first read of `format`.
    pub(crate) bound_format: Option<Gc<Object>>,
}

impl IntlObject {
    /// The bytes that the payload owns, for the accounting of the heap.
    pub(crate) fn heap_size(&self) -> usize {
        let pattern = match &self.data {
            IntlData::DateTime(format) => {
                format.pattern.len() * size_of::<pattern::Token>() + format.zone_id.len()
            }
            _ => 0,
        };
        size_of::<IntlObject>() + pattern
    }
}
