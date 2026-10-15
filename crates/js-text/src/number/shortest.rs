//! The shortest digits of a double in a radix from 2 to 36, for
//! `Number::toString(x, radix)` (§6.1.6.1.20) in the radixes other than
//! 10: the digit string with the fewest significant digits that reads
//! back as the double (round to nearest, ties to even); among those the
//! one closest to the double; of two equally close ones the one whose
//! digits, read as an integer in the radix, are even. This is the rule
//! of §6.1.6.1.20 for radix 10 (note 2: "the one that is even") with the
//! radix 10 replaced by the radix. In an even radix the integer is even
//! when its last digit is; in an odd radix when the sum of its digits
//! is even.
//!
//! The algorithm is the free-format digit generation of R. G. Burger and
//! R. K. Dybvig, "Printing Floating-Point Numbers Quickly and Accurately"
//! (PLDI 1996), a refinement of G. L. Steele Jr. and J. L. White, "How to
//! Print Floating-Point Numbers Accurately" (PLDI 1990), in exact integer
//! arithmetic:
//!
//! - The double v = f × 2^e is r / s, and the values that read back as v
//!   are those within m⁻ / s below and m⁺ / s above v (half the gap to
//!   the next lower and next higher double). The gaps differ when f is
//!   the smallest normal mantissa 2^52 above the smallest exponent; the
//!   ends belong to the range when f is even (a tie reads back as the
//!   even mantissa).
//! - Scaling by radix^k makes 1 / radix ≤ v < 1, so the first digit of v
//!   is the radix^(k−1) place. A floating-point estimate of k is
//!   corrected by exact comparisons.
//! - Each step takes the next digit of v. It stops at the first digit
//!   after which the truncated value or the value one unit higher lies in
//!   the range; of the two, it takes the one in the range, the closer one
//!   if both are, and on a tie the one whose digits form an even
//!   integer (the parity of the digits so far is tracked). In the first
//!   step the value one unit higher can be radix^k, the digit "1" one
//!   place up.
//!
//! One deviation from the paper: Burger and Dybvig choose k from the high
//! end of the range (v + m⁺ < radix^k), so that no digit carries. That
//! also gives the fewest digits, but not always the closest ones: when
//! the range holds a power of the radix above v, they print that power
//! ("1"), although a one-digit value below it can be closer (only for
//! subnormals with few significant bits, such as `Number.MIN_VALUE` in
//! radix 14: 11 × 14^-283 is closer than 14^-282). With k from v the
//! first step compares the two one-digit values around v.
//!
//! The arithmetic is on `u128` when the exponent bounds all values below
//! 2^120 (the doubles from 2^-48 to 2^93, which include most values of
//! `Math.random()`), else on [`Natural`]. At most 54 significant digits
//! (radix 2) come out; the numbers are at most about 1,100 bits (the
//! scale of `Number.MIN_VALUE`, 2^1076, times a power of the radix below
//! the value of the digits).

use super::format::decompose;
use super::natural::Natural;

/// The significant digits (values below `radix`, the first not 0) of a
/// positive finite `x` in `radix` (2 to 36), and the exponent n with
/// x ≈ `0.d₁d₂…dₖ` × radix^n.
pub(crate) fn shortest_radix_digits(x: f64, radix: u32) -> (Vec<u8>, i32) {
    let (f, e) = decompose(x);
    // All values stay below 2^120 for these exponents (see `generate`).
    if U128_EXPONENTS.contains(&e) {
        generate::<u128>(x, f, e, radix)
    } else {
        generate::<Natural>(x, f, e, radix)
    }
}

/// The exponents e of x = f × 2^e for which [`generate`] works on `u128`.
const U128_EXPONENTS: std::ops::RangeInclusive<i32> = -100..=40;

/// The integer operations of the digit generation.
trait Exact: Clone + Ord {
    fn from_u64(value: u64) -> Self;
    /// Multiplies by 2^`bits`.
    fn shl(&mut self, bits: u32);
    /// Multiplies by `factor` (at most 36).
    fn mul_small(&mut self, factor: u32);
    /// Multiplies by `base`^`exponent`.
    fn mul_pow(&mut self, base: u32, exponent: u32);
    /// `self + other`.
    fn plus(&self, other: &Self) -> Self;
    /// Replaces `self` with `self mod divisor` and returns the quotient,
    /// which is below 36.
    fn quotient_digit(&mut self, divisor: &Self) -> u32;
}

impl Exact for u128 {
    fn from_u64(value: u64) -> Self {
        u128::from(value)
    }
    fn shl(&mut self, bits: u32) {
        *self <<= bits;
    }
    fn mul_small(&mut self, factor: u32) {
        *self *= u128::from(factor);
    }
    fn mul_pow(&mut self, base: u32, exponent: u32) {
        *self *= u128::from(base).pow(exponent);
    }
    fn plus(&self, other: &Self) -> Self {
        self + other
    }
    fn quotient_digit(&mut self, divisor: &Self) -> u32 {
        let quotient = *self / *divisor;
        *self %= *divisor;
        quotient as u32
    }
}

impl Exact for Natural {
    fn from_u64(value: u64) -> Self {
        Natural::from_u64(value)
    }
    fn shl(&mut self, bits: u32) {
        Natural::shl(self, bits);
    }
    fn mul_small(&mut self, factor: u32) {
        Natural::mul_small(self, factor);
    }
    fn mul_pow(&mut self, base: u32, exponent: u32) {
        Natural::mul_pow(self, base, exponent);
    }
    fn plus(&self, other: &Self) -> Self {
        let mut sum = self.clone();
        sum.add_assign(other);
        sum
    }
    fn quotient_digit(&mut self, divisor: &Self) -> u32 {
        Natural::quotient_digit(self, divisor)
    }
}

/// The state of the digit generation: v = r / s; the values that read
/// back as v lie between (r − m⁻) / s and (r + m⁺) / s.
struct Range<T> {
    r: T,
    s: T,
    m_minus: T,
    m_plus: T,
    /// The ends of the range read back as v (f is even).
    inclusive: bool,
}

impl<T: Exact> Range<T> {
    /// The range of v = f × 2^e (Burger and Dybvig, table 1, for the
    /// input radix 2 and 53-bit mantissas). With the factor 2 in r and s,
    /// m⁻ and m⁺ are the half gaps.
    fn new(f: u64, e: i32) -> Self {
        // The gap below is half the gap above at a power of two, except
        // at the smallest exponent (the subnormals have the same gap).
        let unequal = f == 1 << 52 && e > -1074;
        let extra = u32::from(unequal);
        let (mut r, mut s, mut m_minus) = (T::from_u64(f), T::from_u64(1), T::from_u64(1));
        r.shl(1 + extra);
        s.shl(1 + extra);
        if e >= 0 {
            r.shl(e.unsigned_abs());
            m_minus.shl(e.unsigned_abs());
        } else {
            s.shl(e.unsigned_abs());
        }
        let mut m_plus = m_minus.clone();
        m_plus.shl(extra);
        Range {
            r,
            s,
            m_minus,
            m_plus,
            inclusive: f.is_multiple_of(2),
        }
    }

    /// Multiplies v and the range by `radix`^`exponent`.
    fn scale_up(&mut self, radix: u32, exponent: u32) {
        self.r.mul_pow(radix, exponent);
        self.m_minus.mul_pow(radix, exponent);
        self.m_plus.mul_pow(radix, exponent);
    }

    /// Whether v × `factor` is at least 1.
    fn at_least_one(&self, factor: u32) -> bool {
        let mut v = self.r.clone();
        v.mul_small(factor);
        v >= self.s
    }
}

/// The digit generation (see the module comment). `x` = `f` × 2^`e`.
///
/// For −100 ≤ e ≤ 40 all values stay below 2^120 and fit in `u128`: s is
/// at most 2^(2 − e) ≤ 2^102, or 4 × radix^k with radix^k at most
/// radix² × v < 2^(12 + 93) (an estimate one too large); r is at most
/// f × 2^(e + 2) ≤ 2^95 or, after scaling, below radix × s; the
/// corrections and steps multiply numbers below 2 × radix × s by the
/// radix (at most 2^6 each).
fn generate<T: Exact>(x: f64, f: u64, e: i32, radix: u32) -> (Vec<u8>, i32) {
    let mut range = Range::<T>::new(f, e);
    // k = ⌊log_radix(v)⌋ + 1 estimated; the corrections make it exact.
    let estimate = (x.log2() / f64::from(radix).log2()).floor() + 1.0;
    let mut k = estimate as i32;
    if k >= 0 {
        range.s.mul_pow(radix, k.unsigned_abs());
    } else {
        range.scale_up(radix, k.unsigned_abs());
    }
    // Too small: v ≥ radix^k.
    while range.at_least_one(1) {
        range.s.mul_small(radix);
        k += 1;
    }
    // Too large: v < radix^(k−1), so the first digit would be 0.
    while !range.at_least_one(radix) {
        range.scale_up(radix, 1);
        k -= 1;
    }
    let mut digits = Vec::new();
    // Whether the integer of the digits so far is odd.
    let mut odd = false;
    // A double has at most 54 significant bits; the bound only guards the
    // loop.
    for _ in 0..1100 {
        range.r.mul_small(radix);
        let digit = range.r.quotient_digit(&range.s);
        range.m_minus.mul_small(radix);
        range.m_plus.mul_small(radix);
        // The truncated digits are in the range (r < m⁻), and the digits
        // one unit higher are (r + m⁺ > s); the ends count if inclusive.
        let (low, high) = if range.inclusive {
            (
                range.r <= range.m_minus,
                range.r.plus(&range.m_plus) >= range.s,
            )
        } else {
            (
                range.r < range.m_minus,
                range.r.plus(&range.m_plus) > range.s,
            )
        };
        let last = match (low, high) {
            (false, false) => {
                digits.push(digit as u8);
                odd = integer_is_odd(odd, digit, radix);
                continue;
            }
            (true, false) => digit,
            (false, true) => digit + 1,
            (true, true) => {
                // Both: the closer one; on a tie the one whose digits
                // form an even integer.
                let twice = range.r.plus(&range.r);
                match twice.cmp(&range.s) {
                    std::cmp::Ordering::Less => digit,
                    std::cmp::Ordering::Greater => digit + 1,
                    std::cmp::Ordering::Equal => {
                        digit + u32::from(integer_is_odd(odd, digit, radix))
                    }
                }
            }
        };
        if last == radix {
            // Only in the first step: radix^k, one digit one place up.
            // Later, the digits before it one unit higher would have been
            // in the range at the previous step.
            debug_assert!(digits.is_empty(), "a carry after the first digit");
            digits.push(1);
            k += 1;
        } else {
            digits.push(last as u8);
        }
        break;
    }
    (digits, k)
}

/// Whether the integer of the digits so far followed by `digit` is
/// odd, given whether the integer of the digits so far (`odd`) is: n ×
/// radix + digit has the parity of the digit in an even radix, and of n
/// plus the digit in an odd one.
fn integer_is_odd(odd: bool, digit: u32, radix: u32) -> bool {
    (odd && radix % 2 == 1) != (digit % 2 == 1)
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::*;

    /// The exact value C × radix^j.
    struct Scaled {
        c: Natural,
        j: i32,
    }

    /// Compares C × radix^j with W × 2^a exactly.
    fn compare(left: &Scaled, radix: u32, w: &Natural, a: i32) -> Ordering {
        // Multiply both by radix^max(0, −j) × 2^max(0, −a).
        let mut lhs = left.c.clone();
        let mut rhs = w.clone();
        if left.j >= 0 {
            lhs.mul_pow(radix, left.j.unsigned_abs());
        } else {
            rhs.mul_pow(radix, left.j.unsigned_abs());
        }
        if a >= 0 {
            rhs.shl(a.unsigned_abs());
        } else {
            lhs.shl(a.unsigned_abs());
        }
        lhs.cmp(&rhs)
    }

    /// The ends of the range of values that read back as x, as
    /// (W, a) with value W × 2^a, and whether they count.
    fn range_of(x: f64) -> ((Natural, i32), (Natural, i32), bool) {
        let (f, e) = decompose(x);
        let unequal = f == 1 << 52 && e > -1074;
        let low = if unequal {
            (Natural::from_u64(4 * f - 1), e - 2)
        } else {
            (Natural::from_u64(2 * f - 1), e - 1)
        };
        let high = (Natural::from_u64(2 * f + 1), e - 1);
        (low, high, f % 2 == 0)
    }

    fn reads_back(value: &Scaled, radix: u32, x: f64) -> bool {
        let ((lw, la), (hw, ha), inclusive) = range_of(x);
        let low = compare(value, radix, &lw, la);
        let high = compare(value, radix, &hw, ha);
        if inclusive {
            low != Ordering::Less && high != Ordering::Greater
        } else {
            low == Ordering::Greater && high == Ordering::Less
        }
    }

    fn natural_of(digits: &[u8], radix: u32) -> Natural {
        let mut c = Natural::default();
        for &d in digits {
            c.mul_small(radix);
            c.add_small(u32::from(d));
        }
        c
    }

    /// x as (F, E): x = F × 2^E.
    fn exact(x: f64) -> (Natural, i32) {
        let (f, e) = decompose(x);
        (Natural::from_u64(f), e)
    }

    /// |x − C × radix^p| × radix^max(0, −base) × 2^max(0, −e) for
    /// p ≥ base: distances on one integer scale.
    fn distance(x: f64, value: &Scaled, radix: u32, base: i32) -> Natural {
        let (mut xs, ex) = exact(x);
        let mut cs = value.c.clone();
        if ex >= 0 {
            xs.shl(ex.unsigned_abs());
        } else {
            cs.shl(ex.unsigned_abs());
        }
        let shift = (-base).max(0);
        xs.mul_pow(radix, shift.unsigned_abs());
        cs.mul_pow(radix, (value.j + shift).unsigned_abs());
        if xs >= cs {
            xs.sub_assign(&cs);
            xs
        } else {
            cs.sub_assign(&xs);
            cs
        }
    }

    /// The digits plus one (in `radix`).
    fn plus_one(digits: &[u8], radix: u32) -> Vec<u8> {
        let mut out = digits.to_vec();
        for d in out.iter_mut().rev() {
            if u32::from(*d) + 1 < radix {
                *d += 1;
                return out;
            }
            *d = 0;
        }
        out.insert(0, 1);
        out
    }

    /// The digits minus one, `None` for 0.
    fn minus_one(digits: &[u8], radix: u32) -> Option<Vec<u8>> {
        let mut out = digits.to_vec();
        for d in out.iter_mut().rev() {
            if *d > 0 {
                *d -= 1;
                return Some(out);
            }
            *d = (radix - 1) as u8;
        }
        None
    }

    /// The digits × radix^p without leading and trailing zeros (empty for
    /// 0).
    fn normalize(digits: &[u8], p: i32) -> (Vec<u8>, i32) {
        let Some(first) = digits.iter().position(|&d| d != 0) else {
            return (Vec::new(), 0);
        };
        let last = digits.iter().rposition(|&d| d != 0).unwrap_or(first);
        let p = p + (digits.len() - 1 - last) as i32;
        (digits[first..=last].to_vec(), p)
    }

    /// Checks the definition by a search over the digit strings near x:
    /// the output reads back as x; no string with fewer significant
    /// digits does; no other string with as many digits that reads back
    /// is closer, and on a tie the output's digits form an even integer
    /// in the radix. The strings that can compete are the multiples of
    /// radix^p next to x for p = j − 1, j, j + 1 (j: the place of the
    /// last output digit); the search takes a superset of them.
    fn check(x: f64, radix: u32) {
        let (digits, exponent) = shortest_radix_digits(x, radix);
        let count = digits.len();
        assert!(!digits.is_empty(), "{x:e} {radix}");
        assert_ne!(digits[0], 0, "{x:e} {radix}: leading zero");
        assert_ne!(digits[count - 1], 0, "{x:e} {radix}: trailing zero");
        assert!(digits.iter().all(|&digit| u32::from(digit) < radix));
        let place = exponent - count as i32;
        let value = Scaled {
            c: natural_of(&digits, radix),
            j: place,
        };
        assert!(
            reads_back(&value, radix, x),
            "{x:e} {radix}: {digits:?} {exponent}"
        );
        let mut candidates: Vec<(Vec<u8>, i32)> = Vec::new();
        // One place up: the digits without the last one, and one more.
        let prefix = digits[..count - 1].to_vec();
        candidates.push((plus_one(&prefix, radix), place + 1));
        candidates.push((prefix, place + 1));
        // The same place: one unit lower and higher.
        let lower = minus_one(&digits, radix).expect("not zero");
        let higher = plus_one(&digits, radix);
        candidates.push((lower.clone(), place));
        candidates.push((higher.clone(), place));
        // One place down: every digit after the lower, same, higher digits.
        for base in [lower, digits.clone(), higher] {
            for digit in 0..radix as u8 {
                let mut other_digits = base.clone();
                other_digits.push(digit);
                candidates.push((other_digits, place - 1));
            }
        }
        let ours = distance(x, &value, radix, place - 1);
        for (other_digits, other_place) in candidates {
            let (other_digits, other_place) = normalize(&other_digits, other_place);
            if other_digits.is_empty() || (other_digits == digits && other_place == place) {
                continue;
            }
            let other = Scaled {
                c: natural_of(&other_digits, radix),
                j: other_place,
            };
            if other_digits.len() > count || !reads_back(&other, radix, x) {
                continue;
            }
            assert!(
                other_digits.len() == count,
                "{x:e} {radix}: {other_digits:?} × {radix}^{other_place} is shorter than {digits:?}"
            );
            match ours.cmp(&distance(x, &other, radix, place - 1)) {
                Ordering::Less => {}
                Ordering::Equal => {
                    assert!(!value.c.is_odd(), "{x:e} {radix}: tie, {digits:?} is odd");
                }
                Ordering::Greater => {
                    panic!(
                        "{x:e} {radix}: {other_digits:?} × {radix}^{other_place} is closer than {digits:?}"
                    );
                }
            }
        }
    }

    /// xorshift64, for reproducible random doubles.
    fn next(seed: &mut u64) -> u64 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    }

    fn radixes() -> impl Iterator<Item = u32> {
        (2..=36).filter(|&r| r != 10)
    }

    #[test]
    fn special_values_are_shortest_and_closest() {
        let special = [
            f64::from_bits(1),
            f64::from_bits(2),
            f64::from_bits(3),
            f64::from_bits((1 << 52) - 1),
            f64::MIN_POSITIVE,
            f64::MIN_POSITIVE * 2.0,
            f64::MAX,
            2f64.powi(1023),
            1.0,
            0.5,
            0.1,
            1.0 / 3.0,
            std::f64::consts::PI,
            2f64.powi(53),
            2f64.powi(53) - 1.0,
            2f64.powi(51) + 0.5,
            2f64.powi(52) + 1.0,
            1e21,
            1e-7,
            123.456,
            2f64.powi(-47),
            2f64.powi(-48),
            2f64.powi(103),
            2f64.powi(104),
        ];
        for x in special {
            for radix in radixes() {
                check(x, radix);
            }
        }
    }

    #[test]
    fn random_doubles_are_shortest_and_closest() {
        let mut seed = 0x2026_1015_u64;
        for i in 0..5000 {
            let bits = next(&mut seed) & !(1 << 63);
            let x = match i % 3 {
                // Any finite positive double.
                0 => f64::from_bits(bits % 0x7FF0_0000_0000_0000),
                // Around the u128 range and Math.random().
                1 => f64::from_bits((bits & ((1 << 52) - 1)) | ((900 + (bits >> 52) % 250) << 52)),
                // Powers of two (unequal gaps) and subnormals.
                _ => {
                    if bits.is_multiple_of(2) {
                        2f64.powi((bits >> 1) as i32 % 2098 - 1074)
                    } else {
                        f64::from_bits(bits % (1 << 52))
                    }
                }
            };
            if x == 0.0 || !x.is_finite() {
                continue;
            }
            let radix = radixes().nth(i % 34).expect("34 radixes");
            check(x, radix);
            let other = 2 + (i as u32 * 7) % 35;
            if other != 10 {
                check(x, other);
            }
        }
    }

    #[test]
    fn both_arithmetics_agree() {
        let mut seed = 77u64;
        for i in 0..2000 {
            let bits = next(&mut seed);
            let x = f64::from_bits((bits & ((1 << 52) - 1)) | ((975 + (bits >> 52) % 141) << 52));
            let (f, e) = decompose(x);
            if !U128_EXPONENTS.contains(&e) {
                continue;
            }
            let radix = radixes().nth(i % 34).expect("34 radixes");
            assert_eq!(
                generate::<u128>(x, f, e, radix),
                generate::<Natural>(x, f, e, radix),
                "{x:e} {radix}"
            );
        }
    }

    #[test]
    fn known_digits() {
        // MIN_VALUE = 2^-1074 is one binary digit; 0.5 one digit in even
        // radixes.
        assert_eq!(
            shortest_radix_digits(f64::from_bits(1), 2),
            (vec![1], -1073)
        );
        assert_eq!(shortest_radix_digits(0.5, 2), (vec![1], 0));
        assert_eq!(shortest_radix_digits(0.5, 6), (vec![3], 0));
        assert_eq!(shortest_radix_digits(255.0, 16), (vec![15, 15], 2));
        assert_eq!(shortest_radix_digits(1296.0, 36), (vec![1], 3));
        // Ties: 1.5 is 1.111…₃ = 1.333…₇ exactly half a unit above the
        // truncated digits. The even integer wins, not the even last
        // digit: 1 followed by 33 ones (34 digits, an even sum), and
        // 1 followed by 19 threes.
        assert_eq!(shortest_radix_digits(1.5, 3), (vec![1; 34], 1));
        let mut sevens = vec![1];
        sevens.extend([3; 19]);
        assert_eq!(shortest_radix_digits(1.5, 7), (sevens, 1));
    }
}
