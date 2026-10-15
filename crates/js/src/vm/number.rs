//! Number to string and string to number for the runtime (ADR 0026
//! section 7). The conversions are pure functions of `js-text`
//! ([`swb_js_text::number`]); this module adds the time charges that
//! long texts need.

pub(crate) use swb_js_text::number::number_to_string;

use crate::heap::Gc;
use crate::runtime::Runtime;
use crate::string::JsString;
use crate::vm::VmResult;

/// The code units of a number scan that cost as much as one time step
/// (about 12 ns). Measured in release builds on strings of a million
/// units: decimal digits 1.6 to 1.7 ns per unit in `Number`, `parseFloat`
/// and `parseInt` (zeros 1.3 to 1.4 ns), hexadecimal digits 1.8 ns,
/// `parseInt` in radixes that are not powers of two 0.2 to 0.4 ns. Six
/// units per step (2 ns) cover them; a digit scan is about ten times as
/// slow per unit as a copy ([`crate::vm::time::UNITS_PER_STEP`]).
const SCAN_UNITS_PER_STEP: usize = 6;

/// The most leading units of a decimal text for which Rust's float parser
/// can do multi-precision work: `js-text` keeps 800 significant digits.
const DECIMAL_EXACT_UNITS: usize = 800;

impl Runtime {
    /// `StringToNumber` (§7.1.4.1.1) of a string value, charged for the
    /// scan of its text.
    pub(crate) fn string_to_number(&mut self, string: Gc<JsString>) -> VmResult<f64> {
        let text = self.heap.string(string)?;
        let units = text.len();
        let value = swb_js_text::number::string_to_number(text.as_str16());
        self.charge_number_scan(units)?;
        self.charge_decimal_digits(units)?;
        Ok(value)
    }

    /// The cost of scanning `units` code units for a number: one step per
    /// [`SCAN_UNITS_PER_STEP`] units. Call it after the scan.
    pub(crate) fn charge_number_scan(&mut self, units: usize) -> VmResult<()> {
        self.charge(units / SCAN_UNITS_PER_STEP)
    }

    /// The cost of rounding the kept digits of a decimal text of `units`
    /// code units: 3 steps per 2 units, up to [`DECIMAL_EXACT_UNITS`]
    /// units. Rust's float parser takes up to 13 µs (about 1.4 steps per
    /// digit) for about 760 digits near a midpoint between two doubles
    /// close to the subnormals (its multi-precision path), and about
    /// 3.5 ns per digit otherwise. Charged for every text that may be
    /// decimal, so also for some that are not (an upper bound).
    pub(crate) fn charge_decimal_digits(&mut self, units: usize) -> VmResult<()> {
        self.charge(units.min(DECIMAL_EXACT_UNITS) * 3 / 2)
    }
}
