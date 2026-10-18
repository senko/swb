//! Unit tests of the `BigInt` operations: edge values and the limit. The
//! reference table of `tests/bigint_reference.rs` covers the arithmetic
//! at random sizes.

use std::cmp::Ordering;

use super::*;
use crate::String16;

fn big(text: &str) -> BigInt {
    string_to_bigint(String16::from(text).as_str16()).unwrap()
}

fn text(x: &BigInt, radix: u32) -> String {
    String::from_utf8(to_string_radix(x, radix).unwrap()).unwrap()
}

fn dec(x: &BigInt) -> String {
    text(x, 10)
}

fn parse_err(text: &str) -> BigIntError {
    string_to_bigint(String16::from(text).as_str16()).unwrap_err()
}

#[test]
fn small_values_round_trip() {
    for value in [0i64, 1, -1, 42, -42, i64::MAX, i64::MIN, 1 << 40] {
        let x = BigInt::from_i64(value).unwrap();
        assert_eq!(dec(&x), value.to_string());
        assert_eq!(x.to_i64(), Some(value));
        assert_eq!(x.to_i64_wrapping(), value);
        assert_eq!(to_f64(&x), value as f64);
    }
    assert_eq!(
        dec(&BigInt::from_u64(u64::MAX).unwrap()),
        u64::MAX.to_string()
    );
    assert_eq!(big("18446744073709551616").to_i64(), None);
    assert_eq!(big("-9223372036854775809").to_i64(), None);
    assert_eq!(big("-18446744073709551615").to_u64_wrapping(), 1);
    assert_eq!(big("-18446744073709551617").to_i64_wrapping(), -1);
}

#[test]
fn small_results_have_exact_capacity() {
    let x = BigInt::from_u64(u64::MAX).unwrap();
    let one = BigInt::from_u64(1).unwrap();
    assert_eq!(add(&x, &one).unwrap().heap_bytes(), 16);
    assert_eq!(sub(&x, &one).unwrap().heap_bytes(), 8);
    assert_eq!(sub(&x, &x).unwrap().heap_bytes(), 0);
    assert_eq!(mul(&x, &x).unwrap().heap_bytes(), 16);
    assert_eq!(mul(&one, &one).unwrap().heap_bytes(), 8);
    assert_eq!(div(&x, &x).unwrap().heap_bytes(), 8);
    assert_eq!(BigInt::from_i64(0).unwrap().heap_bytes(), 0);
}

#[test]
fn signs_of_division() {
    let cases = [
        (7, 2, 3, 1),
        (-7, 2, -3, -1),
        (7, -2, -3, 1),
        (-7, -2, 3, -1),
        (1, 3, 0, 1),
        (-1, 3, 0, -1),
    ];
    for (x, y, q, r) in cases {
        let (x, y) = (BigInt::from_i64(x).unwrap(), BigInt::from_i64(y).unwrap());
        assert_eq!(div(&x, &y).unwrap().to_i64(), Some(q));
        assert_eq!(rem(&x, &y).unwrap().to_i64(), Some(r));
    }
    let zero = BigInt::ZERO;
    assert_eq!(div(&big("5"), &zero), Err(BigIntError::DivisionByZero));
    assert_eq!(rem(&zero, &zero), Err(BigIntError::DivisionByZero));
    // −0 does not exist.
    let q = div(&big("-1"), &big("2")).unwrap();
    assert!(!q.is_negative() && q.is_zero());
}

#[test]
fn shifts_round_towards_minus_infinity() {
    let cases = [
        ("-1", "1", "-1"),
        ("-2", "1", "-1"),
        ("-3", "1", "-2"),
        ("-5", "2", "-2"),
        ("5", "2", "1"),
        ("-18446744073709551616", "64", "-1"),
        ("-18446744073709551617", "64", "-2"),
        ("-36893488147419103232", "1", "-18446744073709551616"),
        ("-1", "18446744073709551616", "-1"),
        ("1", "18446744073709551616", "0"),
        ("-1", "100000000000000000000000000", "-1"),
        ("3", "-2", "12"),
        ("-3", "-64", "-55340232221128654848"),
    ];
    for (x, y, expected) in cases {
        assert_eq!(
            dec(&shift_right(&big(x), &big(y)).unwrap()),
            expected,
            "{x} >> {y}"
        );
    }
    assert_eq!(
        dec(&shift_left(&big("1"), &big("64")).unwrap()),
        "18446744073709551616"
    );
    assert_eq!(dec(&shift_left(&big("-7"), &big("-1")).unwrap()), "-4");
    assert_eq!(
        dec(&shift_left(&big("0"), &big("18446744073709551616")).unwrap()),
        "0"
    );
    assert_eq!(
        shift_left(&big("1"), &big("18446744073709551616")),
        Err(BigIntError::TooLarge)
    );
    assert_eq!(
        shift_right(&big("1"), &big("-18446744073709551616")),
        Err(BigIntError::TooLarge)
    );
}

#[test]
fn bitwise_on_twos_complement() {
    let values = [
        0i64,
        1,
        -1,
        2,
        -2,
        255,
        -256,
        0x5555,
        -0x5555,
        i64::MAX,
        i64::MIN,
    ];
    for &a in &values {
        for &b in &values {
            let (x, y) = (BigInt::from_i64(a).unwrap(), BigInt::from_i64(b).unwrap());
            assert_eq!(bit_and(&x, &y).unwrap().to_i64(), Some(a & b), "{a} & {b}");
            assert_eq!(bit_or(&x, &y).unwrap().to_i64(), Some(a | b), "{a} | {b}");
            assert_eq!(bit_xor(&x, &y).unwrap().to_i64(), Some(a ^ b), "{a} ^ {b}");
        }
        let x = BigInt::from_i64(a).unwrap();
        assert_eq!(not(&x).unwrap().to_i64(), Some(!a));
    }
    // −2^64 & −1, −2^64 | 0 and carries out of the top limb.
    assert_eq!(
        dec(&bit_and(&big("-18446744073709551616"), &big("-1")).unwrap()),
        "-18446744073709551616"
    );
    assert_eq!(
        dec(&bit_or(&big("-18446744073709551616"), &big("0")).unwrap()),
        "-18446744073709551616"
    );
    assert_eq!(
        dec(&bit_xor(&big("-18446744073709551616"), &big("18446744073709551615")).unwrap()),
        "-1"
    );
    assert_eq!(
        dec(&bit_and(&big("-18446744073709551617"), &big("-18446744073709551617")).unwrap()),
        "-18446744073709551617"
    );
}

#[test]
fn as_int_n_and_as_uint_n() {
    let cases = [
        // bits, x, asIntN, asUintN
        (0, "5", "0", "0"),
        (0, "-5", "0", "0"),
        (1, "1", "-1", "1"),
        (1, "-1", "-1", "1"),
        (1, "2", "0", "0"),
        (8, "255", "-1", "255"),
        (8, "-129", "127", "127"),
        (8, "-128", "-128", "128"),
        (64, "-1", "-1", "18446744073709551615"),
        (
            64,
            "9223372036854775808",
            "-9223372036854775808",
            "9223372036854775808",
        ),
        (65, "-1", "-1", "36893488147419103231"),
        (64, "18446744073709551616", "0", "0"),
        (
            128,
            "-18446744073709551616",
            "-18446744073709551616",
            "340282366920938463444927863358058659840",
        ),
        (
            200,
            "-1",
            "-1",
            "1606938044258990275541962092341162602522202993782792835301375",
        ),
        (9_007_199_254_740_991, "5", "5", "5"),
        (9_007_199_254_740_991, "-5", "-5", "error"),
    ];
    for (bits, x, signed, unsigned) in cases {
        assert_eq!(
            dec(&as_int_n(bits, &big(x)).unwrap()),
            signed,
            "asIntN({bits}, {x})"
        );
        let result = as_uint_n(bits, &big(x));
        if unsigned == "error" {
            assert_eq!(result, Err(BigIntError::TooLarge));
        } else {
            assert_eq!(dec(&result.unwrap()), unsigned, "asUintN({bits}, {x})");
        }
    }
    // asUintN with a negative value at the limit: 2^MAX_BITS − 1 is fine,
    // one bit more is not.
    assert_eq!(as_uint_n(MAX_BITS, &big("-1")).unwrap().bit_len(), MAX_BITS);
    assert_eq!(
        as_uint_n(MAX_BITS + 1, &big("-1")),
        Err(BigIntError::TooLarge)
    );
}

#[test]
fn powers() {
    let cases = [
        ("0", "0", "1"),
        ("0", "5", "0"),
        ("1", "1000000000000000000000", "1"),
        ("-1", "1000000000000000000001", "-1"),
        ("-1", "1000000000000000000000", "1"),
        ("2", "64", "18446744073709551616"),
        ("-2", "3", "-8"),
        ("3", "40", "12157665459056928801"),
        ("-10", "21", "-1000000000000000000000"),
        (
            "18446744073709551616",
            "2",
            "340282366920938463463374607431768211456",
        ),
    ];
    for (b, e, expected) in cases {
        assert_eq!(dec(&pow(&big(b), &big(e)).unwrap()), expected, "{b} ** {e}");
    }
    assert_eq!(
        pow(&big("2"), &big("-1")),
        Err(BigIntError::NegativeExponent)
    );
    assert_eq!(
        pow(&big("0"), &big("-1")),
        Err(BigIntError::NegativeExponent)
    );
    assert_eq!(
        pow(&big("3"), &big("100000000000")),
        Err(BigIntError::TooLarge)
    );
    assert_eq!(
        pow(&big("2"), &big(&MAX_BITS.to_string())),
        Err(BigIntError::TooLarge)
    );
    assert_eq!(
        pow(&big("2"), &big(&(MAX_BITS - 1).to_string()))
            .unwrap()
            .bit_len(),
        MAX_BITS
    );
    assert_eq!(
        pow(&big("-2"), &big("18446744073709551617")),
        Err(BigIntError::TooLarge)
    );
}

#[test]
fn conversion_to_double_rounds_to_nearest_even() {
    let cases = [
        ("9007199254740993", 9_007_199_254_740_992.0),
        ("9007199254740995", 9_007_199_254_740_996.0),
        ("9007199254740994", 9_007_199_254_740_994.0),
        ("-9007199254740993", -9_007_199_254_740_992.0),
        ("18446744073709551615", 18_446_744_073_709_551_616.0),
        // 2^64 + 2^11 is a tie between 2^64 and 2^64 + 2^12: to even.
        ("18446744073709553664", 18_446_744_073_709_551_616.0),
        ("18446744073709553665", 18_446_744_073_709_555_712.0),
    ];
    for (x, expected) in cases {
        assert_eq!(to_f64(&big(x)), expected, "{x}");
    }
    // f64::MAX, the midpoint above it (infinity) and just below that.
    let max = from_f64(f64::MAX).unwrap();
    assert_eq!(to_f64(&max), f64::MAX);
    let half_ulp = shift_left(&big("1"), &big("970")).unwrap();
    let midpoint = add(&max, &half_ulp).unwrap();
    assert_eq!(to_f64(&midpoint), f64::INFINITY);
    let below = sub(&midpoint, &big("1")).unwrap();
    assert_eq!(to_f64(&below), f64::MAX);
    assert_eq!(to_f64(&neg(&midpoint).unwrap()), f64::NEG_INFINITY);
}

#[test]
fn conversion_from_double() {
    assert_eq!(dec(&from_f64(0.0).unwrap()), "0");
    assert_eq!(dec(&from_f64(-0.0).unwrap()), "0");
    assert_eq!(dec(&from_f64(-1.0).unwrap()), "-1");
    assert_eq!(
        dec(&from_f64(2f64.powi(64)).unwrap()),
        "18446744073709551616"
    );
    assert_eq!(dec(&from_f64(1e21).unwrap()), "1000000000000000000000");
    for bad in [
        0.5,
        -1.5,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        5e-324,
    ] {
        assert_eq!(from_f64(bad), Err(BigIntError::NotIntegral), "{bad}");
    }
    assert_eq!(from_f64(f64::MAX).unwrap().bit_len(), 1024);
}

#[test]
fn comparison_with_doubles_is_exact() {
    use Ordering::{Equal, Greater, Less};
    let cases = [
        ("1", 1.5, Some(Less)),
        ("2", 1.5, Some(Greater)),
        ("-1", -1.5, Some(Greater)),
        ("-2", -1.5, Some(Less)),
        ("0", 0.0, Some(Equal)),
        ("0", -0.0, Some(Equal)),
        ("0", 5e-324, Some(Less)),
        ("0", -5e-324, Some(Greater)),
        ("1", 5e-324, Some(Greater)),
        ("-1", -0.0, Some(Less)),
        ("5", f64::NAN, None),
        ("5", f64::INFINITY, Some(Less)),
        ("5", f64::NEG_INFINITY, Some(Greater)),
        ("9007199254740993", 9_007_199_254_740_992.0, Some(Greater)),
        ("9007199254740992", 9_007_199_254_740_992.0, Some(Equal)),
        (
            "18446744073709551617",
            18_446_744_073_709_551_616.0,
            Some(Greater),
        ),
        (
            "18446744073709551616",
            18_446_744_073_709_551_616.0,
            Some(Equal),
        ),
        (
            "18446744073709551615",
            18_446_744_073_709_551_616.0,
            Some(Less),
        ),
        (
            "-18446744073709551617",
            -18_446_744_073_709_551_616.0,
            Some(Less),
        ),
        ("1000000000000000000000", 1e21, Some(Equal)),
        ("999999999999999999999", 1e21, Some(Less)),
        ("2251799813685249", 2_251_799_813_685_248.5, Some(Greater)),
        ("2251799813685248", 2_251_799_813_685_248.5, Some(Less)),
        ("-2251799813685248", -2_251_799_813_685_248.5, Some(Greater)),
    ];
    for (x, y, expected) in cases {
        assert_eq!(compare_f64(&big(x), y), expected, "{x} vs {y:e}");
    }
    let max = from_f64(f64::MAX).unwrap();
    assert_eq!(compare_f64(&max, f64::MAX), Some(Equal));
    let above = add(&max, &big("1")).unwrap();
    assert_eq!(compare_f64(&above, f64::MAX), Some(Greater));
    assert_eq!(compare_f64(&above, f64::INFINITY), Some(Less));
}

#[test]
fn string_to_bigint_grammar() {
    assert_eq!(dec(&big("")), "0");
    assert_eq!(dec(&big(" \t\n\u{A0}\u{FEFF}\u{2028}\u{3000} ")), "0");
    assert_eq!(dec(&big("  -0  ")), "0");
    assert_eq!(dec(&big("+12")), "12");
    assert_eq!(dec(&big("-12")), "-12");
    assert_eq!(dec(&big("000123")), "123");
    assert_eq!(dec(&big("0x1F")), "31");
    assert_eq!(dec(&big("0XfF")), "255");
    assert_eq!(dec(&big("0o17")), "15");
    assert_eq!(dec(&big("0B101")), "5");
    assert_eq!(dec(&big("\u{2029}0x00ff\u{1680}")), "255");
    for bad in [
        "0x",
        "0o",
        "0b",
        "-0x1",
        "+0x1",
        "1_000",
        "0x_1",
        "1n",
        "1.5",
        "1e3",
        "+",
        "-",
        "--1",
        "0b2",
        "0o8",
        "0xg",
        "Infinity",
        "1 2",
        "\u{180E}1",
        "１",
    ] {
        assert_eq!(parse_err(bad), BigIntError::InvalidSyntax, "{bad:?}");
    }
}

#[test]
fn comparison_with_text() {
    let cmp = |x: &str, text: &str| compare_text(&big(x), String16::from(text).as_str16()).unwrap();
    assert_eq!(cmp("5", " 5 "), Some(Ordering::Equal));
    assert_eq!(cmp("5", "0x6"), Some(Ordering::Less));
    assert_eq!(cmp("-5", ""), Some(Ordering::Less));
    assert_eq!(cmp("5", "5.0"), None);
    assert_eq!(cmp("5", "0x"), None);
    // Valid texts beyond the limit compare by their sign.
    let digits = format!("1{}", "0".repeat(100_000));
    assert_eq!(cmp("5", &format!(" {digits} ")), Some(Ordering::Less));
    assert_eq!(
        cmp("5", &format!("\u{a0}-{digits}")),
        Some(Ordering::Greater)
    );
    assert_eq!(cmp("-5", &format!("+{digits}")), Some(Ordering::Less));
    assert_eq!(
        cmp("5", &format!("0x1{}", "0".repeat(70_000))),
        Some(Ordering::Less)
    );
    assert_eq!(cmp("5", &format!("{digits}x")), None);
}

#[test]
fn literals() {
    let lit = |s: &str| dec(&parse_literal(String16::from(s).as_str16()).unwrap());
    assert_eq!(lit("0"), "0");
    assert_eq!(lit("123"), "123");
    assert_eq!(lit("0x1Ff"), "511");
    assert_eq!(lit("0b1"), "1");
    assert_eq!(lit("0o7"), "7");
    for bad in ["", "-1", " 1", "1 "] {
        assert_eq!(
            parse_literal(String16::from(bad).as_str16()),
            Err(BigIntError::InvalidSyntax),
            "{bad:?}"
        );
    }
}

#[test]
fn to_string_in_all_radixes() {
    let x = big("-123456789012345678901234567890");
    assert_eq!(
        text(&x, 2),
        "-1100011101110100100001111111101101100001101110011111000001110111001001110001111110000101011010010"
    );
    assert_eq!(text(&x, 16), "-18ee90ff6c373e0ee4e3f0ad2");
    assert_eq!(text(&x, 36), "-byw97um9s91dlz68tsi");
    assert_eq!(text(&x, 7), "-21653251153414601406403630240331250");
    assert_eq!(text(&BigInt::ZERO, 2), "0");
    assert_eq!(text(&BigInt::ZERO, 10), "0");
    // A value whose chunks have leading zeros.
    assert_eq!(
        dec(&big("10000000000000000000000000000000000000001")),
        "10000000000000000000000000000000000000001"
    );
}

#[test]
fn the_limit_holds() {
    let one = big("1");
    let max_shift = big(&(MAX_BITS - 1).to_string());
    let top = shift_left(&one, &max_shift).unwrap();
    assert_eq!(top.bit_len(), MAX_BITS);
    assert_eq!(add(&top, &top), Err(BigIntError::TooLarge));
    assert_eq!(mul(&top, &big("2")), Err(BigIntError::TooLarge));
    assert_eq!(shift_left(&top, &one), Err(BigIntError::TooLarge));
    assert_eq!(sub(&neg(&top).unwrap(), &top), Err(BigIntError::TooLarge));
    let all_ones = add(&sub(&top, &one).unwrap(), &top).unwrap();
    assert_eq!(all_ones.bit_len(), MAX_BITS);
    assert_eq!(add(&all_ones, &one), Err(BigIntError::TooLarge));
    // ~(2^MAX_BITS − 1) = −2^MAX_BITS.
    assert_eq!(not(&all_ones), Err(BigIntError::TooLarge));
    assert_eq!(not(&neg(&all_ones).unwrap()).unwrap().bit_len(), MAX_BITS);
    // A product just at the limit and one bit above it.
    let half = shift_left(&one, &big(&(MAX_BITS / 2 - 1).to_string())).unwrap();
    assert_eq!(mul(&half, &half).unwrap().bit_len(), MAX_BITS - 1);
    let half_ones = sub(&shift_left(&half, &one).unwrap(), &one).unwrap();
    assert_eq!(mul(&half_ones, &half_ones).unwrap().bit_len(), MAX_BITS);
    let more = shift_left(&half_ones, &one).unwrap();
    assert_eq!(mul(&more, &half_ones), Err(BigIntError::TooLarge));
    assert_eq!(
        BigInt::from_limbs(false, vec![1; MAX_BITS.div_ceil(64) as usize + 1]),
        Err(BigIntError::TooLarge)
    );
    // Text with too many digits fails before any arithmetic.
    let digits: String = "9".repeat(MAX_BITS as usize);
    assert_eq!(parse_err(&digits), BigIntError::TooLarge);
    let hex = format!("0x1{}", "0".repeat((MAX_BITS / 4) as usize));
    assert_eq!(parse_err(&hex), BigIntError::TooLarge);
    let hex = format!("0x{}", "f".repeat((MAX_BITS / 4) as usize));
    assert_eq!(big(&hex).bit_len(), MAX_BITS);
}
