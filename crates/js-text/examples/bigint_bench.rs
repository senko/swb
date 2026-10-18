//! Times the `BigInt` operations of `swb_js_text::bigint`: typical sizes
//! (1, 10 and 1,000 limbs) and the slowest operations at the size limit,
//! with the work units of `bigint::cost` per nanosecond (docs/performance.md,
//! "`BigInt`"). `bigint_bench.js` runs the same cases in Node.js.
//!
//! Usage: `cargo run --release -p swb-js-text --example bigint_bench`

use std::hint::black_box;
use std::time::Instant;

use swb_js_text::String16;
use swb_js_text::bigint::{self, BigInt, MAX_BITS, cost};

/// A value of `bits` bits with a fixed pseudo-random pattern below its top
/// bit.
fn value(bits: u64, seed: u64) -> BigInt {
    let mut state = seed | 1;
    let mut limbs = Vec::new();
    for _ in 0..bits.div_ceil(64) {
        // xorshift64
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        limbs.push(state);
    }
    let rest = bits % 64;
    if let Some(top) = limbs.last_mut() {
        if rest > 0 {
            *top &= (1 << rest) - 1;
        }
        *top |= 1 << ((bits + 63) % 64);
    }
    BigInt::from_limbs(false, limbs).expect("a value within the limit")
}

/// The best time in milliseconds of `runs` runs of `f`, each repeating it
/// `inner` times (per call).
fn best<T>(runs: usize, inner: usize, mut f: impl FnMut() -> T) -> f64 {
    let mut best = f64::INFINITY;
    for _ in 0..runs {
        let start = Instant::now();
        for _ in 0..inner {
            black_box(f());
        }
        best = best.min(start.elapsed().as_secs_f64() * 1000.0 / inner as f64);
    }
    best
}

fn row(name: &str, ms: f64, units: u64) {
    let ns = ms * 1e6;
    println!(
        "| {name} | {} | {units} | {:.2} |",
        format_ms(ms),
        ns / units as f64
    );
}

fn format_ms(ms: f64) -> String {
    if ms >= 1.0 {
        format!("{ms:.1} ms")
    } else if ms >= 0.001 {
        format!("{:.2} µs", ms * 1000.0)
    } else {
        format!("{:.1} ns", ms * 1e6)
    }
}

fn main() {
    typical_sizes();
    at_the_limit();
}

/// Values of 1, 10 and 1,000 limbs.
fn typical_sizes() {
    println!("| Case | Time | Cost units | ns per unit |");
    println!("|---|---|---|---|");
    for limbs in [1u64, 10, 1000] {
        let bits = limbs * 64 - 1;
        let (a, b) = (value(bits, 1), value(bits, 2));
        let inner = (1_000_000 / (limbs * limbs)).max(3) as usize;
        let ms = best(5, inner, || bigint::add(&a, &b));
        row(&format!("add, {limbs} limbs"), ms, cost::add(&a, &b));
        let ms = best(5, inner, || bigint::mul(&a, &b));
        row(
            &format!("mul, {limbs} × {limbs} limbs"),
            ms,
            cost::mul(&a, &b),
        );
        let wide = value(bits * 2, 3);
        let ms = best(5, inner, || bigint::div(&wide, &b));
        row(
            &format!("div, {} by {limbs} limbs", limbs * 2),
            ms,
            cost::div(&wide, &b),
        );
        let small = value(40, 4);
        let ms = best(5, inner, || bigint::div(&wide, &small));
        row(
            &format!("div, {} by 1 limb", limbs * 2),
            ms,
            cost::div(&wide, &small),
        );
        let ms = best(5, inner, || bigint::to_string_radix(&a, 10));
        row(
            &format!("toString(10), {limbs} limbs"),
            ms,
            cost::to_string(&a, 10),
        );
        let ms = best(5, inner, || bigint::to_string_radix(&a, 16));
        row(
            &format!("toString(16), {limbs} limbs"),
            ms,
            cost::to_string(&a, 16),
        );
        let text = String16::from(
            String::from_utf8(bigint::to_string_radix(&a, 10).expect("text"))
                .expect("ASCII")
                .as_str(),
        );
        let ms = best(5, inner, || bigint::string_to_bigint(text.as_str16()));
        row(
            &format!("BigInt(decimal), {limbs} limbs"),
            ms,
            cost::parse(text.len()),
        );
        let ms = best(5, inner, || {
            bigint::bit_and(&a, &bigint::neg(&b).expect("neg"))
        });
        row(
            &format!("and with a negative, {limbs} limbs"),
            ms,
            cost::bitwise(&a, &b),
        );
    }
}

/// The slowest operations at the size limit, and operations that fail at
/// once.
fn at_the_limit() {
    println!();
    println!("At the limit ({MAX_BITS} bits):");
    println!();
    println!("| Case | Time | Cost units | ns per unit |");
    println!("|---|---|---|---|");
    let full = value(MAX_BITS, 5);
    let half = value(MAX_BITS / 2, 6);
    let half2 = value(MAX_BITS / 2, 7);
    let ms = best(3, 1, || {
        bigint::mul(&half, &half2).expect("within the limit")
    });
    row(
        "mul, two values of half the limit",
        ms,
        cost::mul(&half, &half2),
    );
    let ms = best(3, 1, || bigint::div(&full, &half).expect("a quotient"));
    row("div, the limit by half of it", ms, cost::div(&full, &half));
    let quarter = value(MAX_BITS / 4, 8);
    let ms = best(3, 1, || bigint::div(&full, &quarter).expect("a quotient"));
    row(
        "div, the limit by a quarter of it",
        ms,
        cost::div(&full, &quarter),
    );
    let ms = best(3, 1, || bigint::to_string_radix(&full, 10).expect("text"));
    row("toString(10) of the limit", ms, cost::to_string(&full, 10));
    let ms = best(3, 1, || bigint::to_string_radix(&full, 3).expect("text"));
    row("toString(3) of the limit", ms, cost::to_string(&full, 3));
    let ms = best(3, 1, || bigint::to_string_radix(&full, 36).expect("text"));
    row("toString(36) of the limit", ms, cost::to_string(&full, 36));
    let ms = best(3, 1, || bigint::to_string_radix(&full, 2).expect("text"));
    row("toString(2) of the limit", ms, cost::to_string(&full, 2));
    let text = String16::from(
        String::from_utf8(bigint::to_string_radix(&full, 10).expect("text"))
            .expect("ASCII")
            .as_str(),
    );
    let ms = best(3, 1, || {
        bigint::string_to_bigint(text.as_str16()).expect("a value")
    });
    row(
        "BigInt(decimal text of the limit)",
        ms,
        cost::parse(text.len()),
    );
    // 3^e just below the limit.
    let three = BigInt::from_u64(3).expect("small");
    let e = BigInt::from_u64((MAX_BITS as f64 / 3f64.log2()) as u64 - 1).expect("small");
    let ms = best(3, 1, || bigint::pow(&three, &e).expect("within the limit"));
    row("3 ** e at the limit", ms, cost::pow(&three, &e));
    let base = value(MAX_BITS / 7, 9);
    let seven = BigInt::from_u64(7).expect("small");
    let ms = best(3, 1, || bigint::pow(&base, &seven));
    row("(limit/7 bits) ** 7", ms, cost::pow(&base, &seven));
    let ones = bigint::neg(&full).expect("neg");
    let ms = best(3, 1, || bigint::bit_xor(&full, &ones).expect("xor"));
    row(
        "xor of two values at the limit",
        ms,
        cost::bitwise(&full, &ones),
    );
    let ms = best(3, 1, || {
        bigint::as_uint_n(MAX_BITS, &ones).expect("asUintN")
    });
    row("asUintN(limit, −x)", ms, cost::as_n(MAX_BITS, &ones));
    let big_shift = BigInt::from_u64(1 << 40).expect("small");
    let ms = best(3, 1, || bigint::shift_left(&full, &big_shift));
    row(
        "x << 2^40 (an error at once)",
        ms,
        cost::shift(&full, &big_shift, false),
    );
    let huge = BigInt::from_u64(u64::MAX).expect("small");
    let ms = best(3, 1, || bigint::pow(&three, &huge));
    row(
        "3 ** (2^64 − 1) (an error at once)",
        ms,
        cost::pow(&three, &huge),
    );
}
