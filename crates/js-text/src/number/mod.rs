//! Numbers and their text forms (ADR 0026 section 7, memo 4.3): exact
//! conversions between doubles and digit strings in all radixes, and the
//! number operations of `Math` that are pure arithmetic on doubles
//! (`sumPrecise`, `f16round`).
//!
//! - [`radix`]: exact digits in power-of-two radixes to a double.
//! - [`parse`]: `StringToNumber`, `parseFloat` and `parseInt`.
//! - [`format`]: `Number::toString` in all radixes, `toFixed`,
//!   `toExponential`, `toPrecision` and the en-US `toLocaleString`.
//! - [`shortest`]: the shortest digits in radixes other than 10.
//! - [`natural`]: the multi-precision integers of the exact conversions.
//! - [`sum`]: the exact sum of `Math.sumPrecise` and `Math.f16round`.

mod format;
mod natural;
mod parse;
mod radix;
mod shortest;
mod sum;

pub use format::{
    locale_string, number_to_string, radix_string, shortest_digits, to_exponential, to_fixed,
    to_precision,
};
pub use parse::{is_str_whitespace, parse_float, parse_int, string_to_number};
pub use radix::RadixAccumulator;
pub use sum::{ExactSum, f16_round};
