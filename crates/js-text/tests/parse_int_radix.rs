//! `parseInt` in the radixes that are not powers of two and not 10
//! against a table of exact values: the integer of the leading digits
//! rounded to the nearest double, ties to even (`Number` of a `BigInt` in
//! Node.js 22). Random digit strings of 1 to 800 digits, leading zeros,
//! signs, the ties at 2^53 and the boundary to infinity in every such
//! radix. `data/gen-parse-int-table.js` writes the table; each line is
//! `radix<TAB>text<TAB>bits`, with the expected double's bit pattern in
//! hexadecimal.

use swb_js_text::String16;
use swb_js_text::number::parse_int;

#[test]
fn parse_int_is_exact() {
    let table = include_str!("data/parse_int_radix.tsv");
    let mut failures = Vec::new();
    let mut count = 0;
    for line in table.lines().filter(|l| !l.is_empty()) {
        let fields: Vec<&str> = line.split('\t').collect();
        let [radix, text, bits] = fields[..] else {
            panic!("bad line: {line}");
        };
        let radix: i32 = radix.parse().expect("numeric radix");
        let expected = f64::from_bits(u64::from_str_radix(bits, 16).expect("hex bits"));
        let actual = parse_int(String16::from(text).as_str16(), radix);
        count += 1;
        if actual.to_bits() != expected.to_bits() {
            failures.push(format!(
                "parseInt({text:.40}…, {radix}): expected {expected:e}, got {actual:e}"
            ));
        }
    }
    assert!(count > 1000, "{count} cases");
    assert!(
        failures.is_empty(),
        "{} of {count} differ: {:#?}",
        failures.len(),
        &failures[..failures.len().min(10)]
    );
}
