//! Text for the JavaScript engine (ADR 0026 sections 2 and 7).
//!
//! JavaScript strings are sequences of 16-bit code units that can contain
//! unpaired surrogates. swb stores them in one of two widths: one byte per
//! code unit when all units are below 256 (Latin-1), otherwise two bytes.
//! This crate has the borrowed view ([`Str16`]) and the owned form
//! ([`String16`]) of such text, the [`CodeUnit`] trait that the lexer and
//! the regular expression matcher are generic over, and the Unicode data
//! that the lexer needs ([`unicode`]), the exact radix-digit number
//! conversion ([`RadixAccumulator`]) and a keyed hasher ([`hash`]). It
//! also has the engine's shared [`RecursionBudget`], because every
//! engine crate depends on this one.

mod budget;
pub mod hash;
mod number;
mod string16;
pub mod unicode;

pub use budget::{BudgetExhausted, RecursionBudget};
pub use number::RadixAccumulator;
pub use string16::{
    CodePoints, CodeUnit, Str16, String16, Units, combine_surrogates, is_lead_surrogate,
    is_trail_surrogate,
};
