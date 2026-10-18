//! The `BigInt` arithmetic of js-text against a reference table from an
//! arbitrary-precision implementation outside swb (Python's `int`):
//! `data/gen-bigint-table.py` writes `data/bigint_reference.tsv` (its
//! header describes the format). All operations, operand sizes from 0 to
//! 40 limbs and a few of 100 to 300, edge values, the add-back step of
//! Algorithm D, all radixes, the conversions to and from doubles, the
//! `StringToBigInt` grammar and the size limit. The tests at the end cover
//! the texts and values at the limit, which are too long for the table.

use std::cmp::Ordering;

use swb_js_text::String16;
use swb_js_text::bigint::{self, BigInt, BigIntError, MAX_BITS};

/// A value of the table: hexadecimal with an optional `-`, `<hex>P<k>`
/// for hex × 2^k, `M<k>` for 2^k − 1.
fn value(text: &str) -> BigInt {
    let (negative, body) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let limbs = if let Some(bits) = body.strip_prefix('M') {
        let bits: u64 = bits.parse().expect("a bit count");
        let mut limbs = vec![u64::MAX; (bits / 64) as usize];
        if !bits.is_multiple_of(64) {
            limbs.push((1 << (bits % 64)) - 1);
        }
        limbs
    } else if let Some((hex, shift)) = body.split_once('P') {
        shifted(&hex_limbs(hex), shift.parse().expect("a shift"))
    } else {
        hex_limbs(body)
    };
    BigInt::from_limbs(negative, limbs).expect("a value within the limit")
}

/// The limbs of a hexadecimal magnitude, least significant first.
fn hex_limbs(hex: &str) -> Vec<u64> {
    let bytes = hex.as_bytes();
    bytes
        .rchunks(16)
        .map(|chunk| {
            u64::from_str_radix(std::str::from_utf8(chunk).expect("ASCII"), 16).expect("hex digits")
        })
        .collect()
}

/// `limbs` × 2^`shift`.
fn shifted(limbs: &[u64], shift: u64) -> Vec<u64> {
    let mut out = vec![0; (shift / 64) as usize];
    let bits = shift % 64;
    let mut carry = 0;
    for &limb in limbs {
        if bits == 0 {
            out.push(limb);
        } else {
            out.push((limb << bits) | carry);
            carry = limb >> (64 - bits);
        }
    }
    out.push(carry);
    out
}

fn error_name(error: BigIntError) -> &'static str {
    match error {
        BigIntError::TooLarge => "TooLarge",
        BigIntError::DivisionByZero => "DivisionByZero",
        BigIntError::NegativeExponent => "NegativeExponent",
        BigIntError::NotIntegral => "NotIntegral",
        BigIntError::OutOfMemory => "OutOfMemory",
        BigIntError::InvalidSyntax => "InvalidSyntax",
    }
}

/// The error names of the table.
const ERRORS: [&str; 6] = [
    "TooLarge",
    "DivisionByZero",
    "NegativeExponent",
    "NotIntegral",
    "OutOfMemory",
    "InvalidSyntax",
];

/// Whether a result matches the expected field (a value or an error).
fn matches(actual: &Result<BigInt, BigIntError>, expected: &str) -> bool {
    if ERRORS.contains(&expected) {
        matches!(actual, Err(error) if error_name(*error) == expected)
    } else {
        matches!(actual, Ok(x) if *x == value(expected))
    }
}

/// The text of a field with `\uXXXX` escapes.
fn unescape(text: &str) -> String16 {
    let mut units = Vec::new();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if let Some(hex) = rest.strip_prefix("\\u") {
            units.push(u16::from_str_radix(&hex[..4], 16).expect("an escape"));
            rest = &hex[4..];
        } else {
            units.push(c as u16);
            rest = &rest[1..];
        }
    }
    String16::from_units(&units)
}

fn f64_field(text: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(text, 16).expect("the bits of a double"))
}

fn order_name(order: Option<Ordering>) -> &'static str {
    match order {
        Some(Ordering::Less) => "lt",
        Some(Ordering::Equal) => "eq",
        Some(Ordering::Greater) => "gt",
        None => "nan",
    }
}

fn show(result: &Result<BigInt, BigIntError>) -> String {
    match result {
        Ok(x) => format!(
            "{}{}",
            if x.is_negative() { "-" } else { "" },
            String::from_utf8(bigint::to_string_radix(x, 16).expect("text"))
                .expect("ASCII")
                .trim_start_matches('-')
        ),
        Err(error) => error_name(*error).to_owned(),
    }
}

/// Runs one line; `None` if it passes, else the description of the
/// difference.
fn check(fields: &[&str]) -> Option<String> {
    let binary = |f: fn(&BigInt, &BigInt) -> Result<BigInt, BigIntError>| {
        let result = f(&value(fields[1]), &value(fields[2]));
        (!matches(&result, fields[3])).then(|| format!("got {}", show(&result)))
    };
    match fields[0] {
        "add" => binary(bigint::add),
        "sub" => binary(bigint::sub),
        "mul" => binary(bigint::mul),
        "and" => binary(bigint::bit_and),
        "or" => binary(bigint::bit_or),
        "xor" => binary(bigint::bit_xor),
        "shl" => binary(bigint::shift_left),
        "shr" => binary(bigint::shift_right),
        "pow" => binary(bigint::pow),
        "cmp" => {
            let order = value(fields[1]).cmp(&value(fields[2]));
            (order_name(Some(order)) != fields[3]).then(|| format!("got {order:?}"))
        }
        "divrem" | "divab" => check_division(fields),
        "not" => {
            let result = bigint::not(&value(fields[1]));
            (!matches(&result, fields[2])).then(|| format!("got {}", show(&result)))
        }
        "asint" | "asuint" => {
            let bits: u64 = fields[1].parse().expect("a bit count");
            let x = value(fields[2]);
            let result = if fields[0] == "asint" {
                bigint::as_int_n(bits, &x)
            } else {
                bigint::as_uint_n(bits, &x)
            };
            (!matches(&result, fields[3])).then(|| format!("got {}", show(&result)))
        }
        "tof64" => {
            let actual = bigint::to_f64(&value(fields[1]));
            (actual.to_bits() != f64_field(fields[2]).to_bits()).then(|| format!("got {actual:e}"))
        }
        "fromf64" => {
            let result = bigint::from_f64(f64_field(fields[1]));
            (!matches(&result, fields[2])).then(|| format!("got {}", show(&result)))
        }
        "cmpf" => {
            let order = bigint::compare_f64(&value(fields[1]), f64_field(fields[2]));
            (order_name(order) != fields[3]).then(|| format!("got {order:?}"))
        }
        "tostr" => {
            let radix: u32 = fields[1].parse().expect("a radix");
            let text = bigint::to_string_radix(&value(fields[2]), radix).expect("text");
            (text != fields[3].as_bytes())
                .then(|| format!("got {}", String::from_utf8_lossy(&text)))
        }
        "parse" | "lit" => {
            let text = unescape(fields[1]);
            let result = if fields[0] == "parse" {
                bigint::string_to_bigint(text.as_str16())
            } else {
                bigint::parse_literal(text.as_str16())
            };
            (!matches(&result, fields[2])).then(|| format!("got {}", show(&result)))
        }
        other => Some(format!("unknown operation {other}")),
    }
}

/// A `divrem` or `divab` line: `div`, `rem` and `div_rem` agree with each
/// other and with the table.
fn check_division(fields: &[&str]) -> Option<String> {
    let (dividend, divisor) = (value(fields[1]), value(fields[2]));
    let quotient = bigint::div(&dividend, &divisor);
    let remainder = bigint::rem(&dividend, &divisor);
    let same = match (bigint::div_rem(&dividend, &divisor), &quotient, &remainder) {
        (Ok((q, r)), Ok(quotient), Ok(remainder)) => q == *quotient && r == *remainder,
        (Err(a), Err(b), Err(c)) => a == *b && a == *c,
        _ => false,
    };
    (!same || !matches(&quotient, fields[3]) || !matches(&remainder, fields[4]))
        .then(|| format!("got {} {}", show(&quotient), show(&remainder)))
}

#[test]
fn reference_table() {
    let table = include_str!("data/bigint_reference.tsv");
    let mut failures = Vec::new();
    let mut count = 0;
    for line in table.lines().filter(|l| !l.is_empty()) {
        let fields: Vec<&str> = line.split('\t').collect();
        count += 1;
        if let Some(problem) = check(&fields) {
            failures.push(format!("{line:.300}: {problem:.300}"));
        }
    }
    assert!(count > 6000, "{count} cases");
    assert!(
        failures.is_empty(),
        "{} of {count} differ: {:#?}",
        failures.len(),
        &failures[..failures.len().min(10)]
    );
}

fn parse(text: &str) -> Result<BigInt, BigIntError> {
    bigint::string_to_bigint(String16::from(text).as_str16())
}

#[test]
fn texts_at_the_limit() {
    // Bit lengths of the decimal values from Python's int.
    let cases = [
        ("9".repeat(78_912), Ok(262_140)),
        ("9".repeat(78_913), Ok(262_144)),
        ("9".repeat(78_914), Err(BigIntError::TooLarge)),
        (
            format!("1{}", "0".repeat(79_000)),
            Err(BigIntError::TooLarge),
        ),
        (
            format!("0x{}", "f".repeat((MAX_BITS / 4) as usize)),
            Ok(MAX_BITS),
        ),
        (
            format!("0x1{}", "0".repeat((MAX_BITS / 4) as usize)),
            Err(BigIntError::TooLarge),
        ),
        (
            format!("0b1{}", "0".repeat((MAX_BITS - 1) as usize)),
            Ok(MAX_BITS),
        ),
        (
            format!("0b1{}", "0".repeat(MAX_BITS as usize)),
            Err(BigIntError::TooLarge),
        ),
        (
            format!("0o7{}", "7".repeat(((MAX_BITS - 3) / 3) as usize)),
            Ok(MAX_BITS - 1),
        ),
        (format!("{}1", "0".repeat(1_000_000)), Ok(1)),
    ];
    for (text, expected) in cases {
        let result = parse(&text).map(|x| x.bit_len());
        assert_eq!(result, expected, "{:.20}… ({} units)", text, text.len());
    }
}

#[test]
fn decimal_text_round_trips_at_the_limit() {
    let all_ones = value(&format!("M{MAX_BITS}"));
    let text = bigint::to_string_radix(&all_ones, 10).expect("text");
    assert_eq!(text.len(), 78_914);
    let back = parse(std::str::from_utf8(&text).expect("ASCII")).expect("a value");
    assert_eq!(back, all_ones);
    let negative = bigint::neg(&all_ones).expect("a value");
    for radix in [2, 3, 16, 36] {
        let text = bigint::to_string_radix(&negative, radix).expect("text");
        assert_eq!(text[0], b'-');
        if radix == 16 {
            assert_eq!(text.len(), 1 + (MAX_BITS / 4) as usize);
            let back = parse(&format!(
                "-0x{}",
                std::str::from_utf8(&text[1..]).expect("ASCII")
            ));
            assert_eq!(back, Err(BigIntError::InvalidSyntax));
        }
    }
}

#[test]
fn operations_at_the_limit() {
    let one = BigInt::from_u64(1).expect("small");
    let all_ones = value(&format!("M{MAX_BITS}"));
    let half_ones = value(&format!("M{}", MAX_BITS / 2));
    // The largest product, quotient and power within the limit.
    let product = bigint::mul(&half_ones, &half_ones).expect("within the limit");
    assert_eq!(product.bit_len(), MAX_BITS);
    let (q, r) = bigint::div_rem(&product, &half_ones).expect("a quotient");
    assert_eq!(q, half_ones);
    assert!(r.is_zero());
    let (q, r) = bigint::div_rem(&all_ones, &bigint::add(&half_ones, &one).expect("sum"))
        .expect("a quotient");
    assert_eq!(q, half_ones);
    assert_eq!(r, half_ones);
    let three = BigInt::from_u64(3).expect("small");
    // 3^165,394 has 262,144 bits and 3^165,395 has 262,145 (Python's
    // int): the estimate does not exclude the second, the final check does.
    let e = BigInt::from_u64(165_394).expect("small");
    assert_eq!(
        bigint::pow(&three, &e).expect("within the limit").bit_len(),
        262_144
    );
    let e = BigInt::from_u64(165_395).expect("small");
    assert_eq!(bigint::pow(&three, &e), Err(BigIntError::TooLarge));
    // One bit above the limit.
    assert_eq!(bigint::add(&all_ones, &one), Err(BigIntError::TooLarge));
    assert_eq!(
        bigint::sub(&bigint::neg(&all_ones).expect("neg"), &one),
        Err(BigIntError::TooLarge)
    );
    assert_eq!(bigint::mul(&all_ones, &three), Err(BigIntError::TooLarge));
    assert_eq!(
        bigint::shift_left(&all_ones, &one),
        Err(BigIntError::TooLarge)
    );
    assert_eq!(bigint::not(&all_ones), Err(BigIntError::TooLarge));
    // Huge shift amounts and exponents end at once.
    let huge = value("1P64");
    assert_eq!(bigint::shift_left(&one, &huge), Err(BigIntError::TooLarge));
    assert_eq!(
        bigint::shift_right(&all_ones, &huge).expect("zero"),
        BigInt::ZERO
    );
    let minus_one = BigInt::from_i64(-1).expect("small");
    assert_eq!(
        bigint::shift_right(&bigint::neg(&all_ones).expect("neg"), &huge).expect("-1"),
        minus_one
    );
    assert_eq!(bigint::pow(&three, &huge), Err(BigIntError::TooLarge));
    assert_eq!(bigint::pow(&minus_one, &huge).expect("one"), one);
    assert_eq!(
        bigint::as_uint_n(u64::MAX, &minus_one),
        Err(BigIntError::TooLarge)
    );
    assert_eq!(
        bigint::as_int_n(u64::MAX, &all_ones).expect("itself"),
        all_ones
    );
    // The bitwise operators on the largest values.
    let negative = bigint::neg(&all_ones).expect("neg");
    assert_eq!(bigint::bit_and(&negative, &all_ones).expect("and"), one);
    assert_eq!(bigint::bit_or(&negative, &all_ones).expect("or"), minus_one);
    assert_eq!(bigint::to_f64(&all_ones), f64::INFINITY);
    assert_eq!(
        bigint::compare_f64(&negative, f64::MIN),
        Some(Ordering::Less)
    );
}
