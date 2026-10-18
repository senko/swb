//! The shifts and the bitwise operators (§6.1.6.2.9 to §6.1.6.2.11 and
//! §6.1.6.2.20 to §6.1.6.2.22,
//! <https://tc39.es/ecma262/#sec-numeric-types-bigint-leftShift>), and
//! `BigInt.asIntN` and `BigInt.asUintN` (§21.2.2.1 and §21.2.2.2,
//! <https://tc39.es/ecma262/#sec-bigint.asintn>).
//!
//! The bitwise operators act on the infinite two's complement form. A
//! negative operand's limbs in that form are computed as they are read
//! (the complement of the magnitude minus one), and only as many limbs as
//! the result can have: `x & 0xFFn` reads one limb of `x`.

use super::{BigInt, BigIntError, Result, check_bits, limbs};

/// `BigInt::leftShift` (§6.1.6.2.9): `x × 2^y`; for a negative `y`,
/// `floor(x / 2^−y)`.
pub fn shift_left(x: &BigInt, y: &BigInt) -> Result<BigInt> {
    shift(x, y, false)
}

/// `BigInt::signedRightShift` (§6.1.6.2.10): `x << −y`, which rounds towards
/// minus infinity.
pub fn shift_right(x: &BigInt, y: &BigInt) -> Result<BigInt> {
    shift(x, y, true)
}

/// `x << y`, or `x >> y` if `right`.
fn shift(x: &BigInt, y: &BigInt, right: bool) -> Result<BigInt> {
    if x.is_zero() {
        return Ok(BigInt::ZERO);
    }
    let amount = y.to_u64_magnitude();
    if y.negative == right {
        let amount = amount.ok_or(BigIntError::TooLarge)?;
        check_bits(x.bit_len().saturating_add(amount))?;
        shift_magnitude_left(x, amount)
    } else {
        // A right shift; one by more than the bits of x gives 0 or −1.
        match amount {
            Some(amount) => shift_magnitude_right(x, amount),
            None => BigInt::from_i64(if x.negative { -1 } else { 0 }),
        }
    }
}

/// `x × 2^amount`, known to be within the limit.
fn shift_magnitude_left(x: &BigInt, amount: u64) -> Result<BigInt> {
    let words = (amount / 64) as usize;
    let bits = (amount % 64) as u32;
    let mut out = limbs::try_vec(words + x.limbs.len() + 1)?;
    out.resize(words, 0);
    if bits == 0 {
        out.extend_from_slice(&x.limbs);
    } else {
        let mut carry = 0;
        for &limb in &x.limbs {
            out.push((limb << bits) | carry);
            carry = limb >> (64 - bits);
        }
        out.push(carry);
    }
    limbs::trim(&mut out);
    Ok(BigInt::from_normalized(x.negative, out))
}

/// `floor(x / 2^amount)`.
fn shift_magnitude_right(x: &BigInt, amount: u64) -> Result<BigInt> {
    let round_down = x.negative && limbs::any_bit_below(&x.limbs, amount);
    let bits = x.bit_len();
    let mut out = if amount >= bits {
        limbs::try_vec(1)?
    } else {
        let len = (bits - amount).div_ceil(64) as usize;
        // One more limb for the carry of the rounding.
        let mut out = limbs::try_vec(len + usize::from(round_down))?;
        out.extend((0..len as u64).map(|i| limbs::bits_at(&x.limbs, amount + i * 64)));
        out
    };
    limbs::trim(&mut out);
    if round_down {
        // −m / 2^k rounds to −(floor(m / 2^k) + 1).
        out.push(0);
        limbs::add_assign(&mut out, &[1]);
        limbs::trim(&mut out);
    }
    Ok(BigInt::from_normalized(x.negative, out))
}

/// The limbs of a value in the infinite two's complement form, from the
/// lowest; a negative value's limbs are the complement of its magnitude
/// minus one.
struct TwosComplement<'a> {
    magnitude: &'a [u64],
    negative: bool,
    /// Whether the "minus one" still borrows from the next limb.
    borrow: bool,
    index: usize,
}

impl<'a> TwosComplement<'a> {
    fn new(x: &'a BigInt) -> Self {
        TwosComplement {
            magnitude: &x.limbs,
            negative: x.negative,
            borrow: x.negative,
            index: 0,
        }
    }

    /// The next limb (the sign limb beyond the magnitude).
    fn next_limb(&mut self) -> u64 {
        let limb = self.magnitude.get(self.index).copied().unwrap_or(0);
        self.index += 1;
        if !self.negative {
            return limb;
        }
        let (diff, borrow) = limb.overflowing_sub(u64::from(self.borrow));
        self.borrow = borrow;
        !diff
    }
}

/// The operator of [`bitwise`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    And,
    Or,
    Xor,
}

impl Op {
    fn apply(self, a: u64, b: u64) -> u64 {
        match self {
            Op::And => a & b,
            Op::Or => a | b,
            Op::Xor => a ^ b,
        }
    }
}

/// `BigInt::bitwiseAND` (§6.1.6.2.20): `x & y`.
pub fn bit_and(x: &BigInt, y: &BigInt) -> Result<BigInt> {
    bitwise(x, y, Op::And)
}

/// `BigInt::bitwiseOR` (§6.1.6.2.22): `x | y`.
pub fn bit_or(x: &BigInt, y: &BigInt) -> Result<BigInt> {
    bitwise(x, y, Op::Or)
}

/// `BigInt::bitwiseXOR` (§6.1.6.2.21): `x ^ y`.
pub fn bit_xor(x: &BigInt, y: &BigInt) -> Result<BigInt> {
    bitwise(x, y, Op::Xor)
}

/// `BigIntBitwiseOp` (§6.1.6.2.19) on the two's complement forms.
fn bitwise(x: &BigInt, y: &BigInt, op: Op) -> Result<BigInt> {
    let (lx, ly) = (x.limbs.len(), y.limbs.len());
    // The limbs that can differ from the result's sign limb. Above them
    // all limbs are the sign limb.
    // A non-negative operand of `&` bounds the result's bits, a negative
    // operand of `|` bounds the result's magnitude.
    let width = match (op, x.negative, y.negative) {
        (Op::And, false, false) | (Op::Or, true, true) => lx.min(ly),
        (Op::And, false, true) | (Op::Or, true, false) => lx,
        (Op::And, true, false) | (Op::Or, false, true) => ly,
        _ => lx.max(ly),
    };
    let negative = op.apply(u64::from(x.negative), u64::from(y.negative)) == 1;
    let mut a = TwosComplement::new(x);
    let mut b = TwosComplement::new(y);
    let mut out = limbs::try_vec(width + 1)?;
    out.extend((0..width).map(|_| op.apply(a.next_limb(), b.next_limb())));
    if negative {
        negate_in_place(&mut out);
    }
    limbs::trim(&mut out);
    Ok(BigInt::from_normalized(negative, out))
}

/// Replaces the low limbs of a negative two's complement value (all limbs
/// above them are ones) with its magnitude, one limb longer if needed.
fn negate_in_place(limbs: &mut Vec<u64>) {
    for limb in limbs.iter_mut() {
        *limb = !*limb;
    }
    if limbs::add_assign(limbs, &[1]) || limbs.is_empty() {
        // The magnitude is 2^(64 × len).
        limbs.push(1);
    }
}

/// `BigInt`.asUintN (§21.2.2.2): `x` modulo 2^`bits`.
pub fn as_uint_n(bits: u64, x: &BigInt) -> Result<BigInt> {
    if !x.negative && x.bit_len() <= bits {
        return x.try_clone();
    }
    if x.negative && bits > x.bit_len() {
        // 2^bits − |x|, which has exactly `bits` bits.
        check_bits(bits)?;
    }
    let out = low_bits(x, bits)?;
    Ok(BigInt::from_normalized(false, out))
}

/// `BigInt`.asIntN (§21.2.2.1): `x` modulo 2^`bits`, minus 2^`bits` if that
/// is at least 2^(`bits` − 1).
pub fn as_int_n(bits: u64, x: &BigInt) -> Result<BigInt> {
    if bits == 0 {
        return Ok(BigInt::ZERO);
    }
    if x.bit_len() < bits {
        // −2^(bits − 1) < x < 2^(bits − 1).
        return x.try_clone();
    }
    // Here bits ≤ bit_len(x), so the low bits are within the limit.
    let mut out = low_bits(x, bits)?;
    let top = bits - 1;
    let negative = out
        .get((top / 64) as usize)
        .is_some_and(|&limb| limb >> (top % 64) & 1 == 1);
    if negative {
        // The magnitude is 2^bits − the low bits.
        negate_in_place(&mut out);
        mask(&mut out, bits);
    }
    limbs::trim(&mut out);
    if out.is_empty() {
        return Ok(BigInt::ZERO);
    }
    Ok(BigInt::from_normalized(negative, out))
}

/// The low `bits` bits of `x`'s two's complement form, normalized. The
/// caller has checked that `bits` is within the limit or not above the
/// bits of `x`.
fn low_bits(x: &BigInt, bits: u64) -> Result<Vec<u64>> {
    let len = usize::try_from(bits.div_ceil(64)).map_err(|_| BigIntError::TooLarge)?;
    let len = if x.negative {
        len
    } else {
        len.min(x.limbs.len())
    };
    let mut out = limbs::try_vec(len)?;
    let mut source = TwosComplement::new(x);
    out.extend((0..len).map(|_| source.next_limb()));
    mask(&mut out, bits);
    limbs::trim(&mut out);
    Ok(out)
}

/// Clears the bits from bit `bits` upward.
fn mask(limbs: &mut Vec<u64>, bits: u64) {
    let len = usize::try_from(bits.div_ceil(64)).unwrap_or(usize::MAX);
    limbs.truncate(len);
    let rest = (bits % 64) as u32;
    if rest > 0
        && limbs.len() == len
        && let Some(top) = limbs.last_mut()
    {
        *top &= (1u64 << rest) - 1;
    }
}
