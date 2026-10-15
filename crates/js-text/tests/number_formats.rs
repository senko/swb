//! The number formats of `js-text` against a table of expected outputs:
//! Node.js 22 for `toFixed`, `toExponential` and `toPrecision`, Chromium
//! 148 for `toLocaleString`, and an own `BigInt` reference written from
//! the definition (`data/radix-reference.js`) for `toString(radix)`.
//! The cases are random doubles over the whole exponent range,
//! subnormals, ties, powers of ten and of two, the extreme values, all
//! radixes and all digit counts from 0 to 100. `data/gen-number-table.js`
//! writes the table (it says how); each line is
//! `op<TAB>arg<TAB>bits<TAB>expected`, with the double's bit pattern in
//! hexadecimal.

use swb_js_text::number::{locale_string, radix_string, to_exponential, to_fixed, to_precision};

#[test]
fn formats_match_the_table() {
    let table = include_str!("data/number_formats.tsv");
    let mut failures = Vec::new();
    let mut count = 0;
    for line in table.lines().filter(|l| !l.is_empty()) {
        let fields: Vec<&str> = line.split('\t').collect();
        let [op, arg, bits, expected] = fields[..] else {
            panic!("bad line: {line}");
        };
        let x = f64::from_bits(u64::from_str_radix(bits, 16).expect("hex bits"));
        let number = || arg.parse::<u32>().expect("numeric argument");
        let actual = match op {
            "fixed" => to_fixed(x, number()),
            "exponential" if arg == "u" => to_exponential(x, None),
            "exponential" => to_exponential(x, Some(number())),
            "precision" => to_precision(x, number()),
            "radix" => radix_string(x, number()),
            "locale" => locale_string(x),
            _ => panic!("bad op: {op}"),
        };
        count += 1;
        if actual != expected {
            failures.push(format!(
                "{op}({arg}) of {x:e}: expected {expected}, got {actual}"
            ));
        }
    }
    assert!(count > 4000, "{count} cases");
    assert!(
        failures.is_empty(),
        "{} of {count} differ: {:#?}",
        failures.len(),
        &failures[..failures.len().min(10)]
    );
}
