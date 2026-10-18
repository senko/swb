//! Magnitude arithmetic on slices of 64-bit limbs, least significant
//! first: comparison, addition, subtraction and schoolbook multiplication
//! (Knuth, TAOCP vol. 2, §4.3.1, Algorithms A, S and M). The inputs are
//! normalized (no zero limb at the top) unless a function says otherwise.
//! Vectors are allocated with [`try_vec`], which fails instead of
//! aborting.

use std::cmp::Ordering;

use super::{BigIntError, Result};

/// An empty vector with room for exactly `len` limbs.
pub(super) fn try_vec(len: usize) -> Result<Vec<u64>> {
    let mut vec = Vec::new();
    vec.try_reserve_exact(len)
        .map_err(|_| BigIntError::OutOfMemory)?;
    Ok(vec)
}

/// A vector of `len` zero limbs.
pub(super) fn try_zeroed(len: usize) -> Result<Vec<u64>> {
    let mut vec = try_vec(len)?;
    vec.resize(len, 0);
    Ok(vec)
}

/// Removes zero limbs at the top.
pub(super) fn trim(limbs: &mut Vec<u64>) {
    let len = significant_len(limbs);
    limbs.truncate(len);
}

/// The length of `limbs` without zero limbs at the top.
pub(super) fn significant_len(limbs: &[u64]) -> usize {
    limbs
        .iter()
        .rposition(|&limb| limb != 0)
        .map_or(0, |i| i + 1)
}

/// The number of significant bits.
pub(super) fn bit_len(limbs: &[u64]) -> u64 {
    match limbs.last() {
        None => 0,
        Some(&top) => (limbs.len() as u64 - 1) * 64 + u64::from(64 - top.leading_zeros()),
    }
}

/// Compares two normalized magnitudes.
pub(super) fn cmp(a: &[u64], b: &[u64]) -> Ordering {
    a.len()
        .cmp(&b.len())
        .then_with(|| a.iter().rev().cmp(b.iter().rev()))
}

/// `a + b`.
pub(super) fn add(a: &[u64], b: &[u64]) -> Result<Vec<u64>> {
    let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    let mut out = try_vec(long.len() + 1)?;
    out.extend_from_slice(long);
    let carry = add_assign(&mut out, short);
    if carry {
        out.push(1);
    }
    Ok(out)
}

/// `acc += b`, where `acc` is at least as long as `b`; returns the carry
/// out of the top limb of `acc`.
pub(super) fn add_assign(acc: &mut [u64], b: &[u64]) -> bool {
    let mut carry = false;
    let (low, high) = acc.split_at_mut(b.len().min(acc.len()));
    for (x, &y) in low.iter_mut().zip(b) {
        let (sum, c1) = x.overflowing_add(y);
        let (sum, c2) = sum.overflowing_add(u64::from(carry));
        *x = sum;
        carry = c1 || c2;
    }
    for x in high {
        if !carry {
            break;
        }
        let (sum, c) = x.overflowing_add(1);
        *x = sum;
        carry = c;
    }
    carry
}

/// `a − b` for `a ≥ b`, normalized.
pub(super) fn sub(a: &[u64], b: &[u64]) -> Result<Vec<u64>> {
    debug_assert!(
        cmp(a, b) != Ordering::Less,
        "a magnitude minus a larger one"
    );
    let mut out = try_vec(a.len())?;
    out.extend_from_slice(a);
    sub_assign(&mut out, b);
    trim(&mut out);
    Ok(out)
}

/// `acc −= b`, where `acc` is at least as long as `b`; returns the borrow
/// out of the top limb of `acc`.
pub(super) fn sub_assign(acc: &mut [u64], b: &[u64]) -> bool {
    let mut borrow = false;
    let (low, high) = acc.split_at_mut(b.len().min(acc.len()));
    for (x, &y) in low.iter_mut().zip(b) {
        let (diff, b1) = x.overflowing_sub(y);
        let (diff, b2) = diff.overflowing_sub(u64::from(borrow));
        *x = diff;
        borrow = b1 || b2;
    }
    for x in high {
        if !borrow {
            break;
        }
        let (diff, b) = x.overflowing_sub(1);
        *x = diff;
        borrow = b;
    }
    borrow
}

/// `a × b` (schoolbook), normalized. Neither is empty.
pub(super) fn mul(a: &[u64], b: &[u64]) -> Result<Vec<u64>> {
    let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    let mut out = try_zeroed(long.len() + short.len())?;
    for (j, &factor) in short.iter().enumerate() {
        if let Some(row) = out.get_mut(j..j + long.len() + 1) {
            let (body, top) = row.split_at_mut(long.len());
            if let Some(top) = top.first_mut() {
                *top = mul_add_row(body, long, factor);
            }
        }
    }
    trim(&mut out);
    Ok(out)
}

/// `acc += a × factor` over `a.len()` limbs of `acc` (which has exactly
/// that length); returns the limb that carries out of the top.
pub(super) fn mul_add_row(acc: &mut [u64], a: &[u64], factor: u64) -> u64 {
    let mut carry = 0u64;
    for (x, &y) in acc.iter_mut().zip(a) {
        let t = u128::from(y) * u128::from(factor) + u128::from(*x) + u128::from(carry);
        *x = t as u64;
        carry = (t >> 64) as u64;
    }
    carry
}

/// `acc = acc × factor + addend` in place; returns the carry out of the
/// top limb.
pub(super) fn mul_small_add(acc: &mut [u64], factor: u64, addend: u64) -> u64 {
    let mut carry = addend;
    for x in acc {
        let t = u128::from(*x) * u128::from(factor) + u128::from(carry);
        *x = t as u64;
        carry = (t >> 64) as u64;
    }
    carry
}

/// Whether any of the bits below bit `shift` is set.
pub(super) fn any_bit_below(limbs: &[u64], shift: u64) -> bool {
    let words = usize::try_from(shift / 64).unwrap_or(usize::MAX);
    let bits = (shift % 64) as u32;
    let whole = limbs.iter().take(words).any(|&limb| limb != 0);
    let part = bits > 0
        && limbs
            .get(words)
            .is_some_and(|&limb| limb & ((1u64 << bits) - 1) != 0);
    whole || part
}

/// The 64 bits from bit `shift` upward (zeros above the top).
pub(super) fn bits_at(limbs: &[u64], shift: u64) -> u64 {
    let word = usize::try_from(shift / 64).unwrap_or(usize::MAX);
    let bits = (shift % 64) as u32;
    let low = limbs.get(word).copied().unwrap_or(0);
    if bits == 0 {
        return low;
    }
    let high = limbs.get(word.saturating_add(1)).copied().unwrap_or(0);
    (low >> bits) | (high << (64 - bits))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carries_and_borrows_cross_limbs() {
        let sum = add(&[u64::MAX, u64::MAX], &[1]).unwrap();
        assert_eq!(sum, [0, 0, 1]);
        assert_eq!(sub(&sum, &[1]).unwrap(), [u64::MAX, u64::MAX]);
        assert_eq!(sub(&[0, 1], &[1]).unwrap(), [u64::MAX]);
        assert_eq!(sub(&[5], &[5]).unwrap(), [0u64; 0]);
        assert_eq!(mul(&[u64::MAX], &[u64::MAX]).unwrap(), [1, u64::MAX - 1]);
        assert_eq!(
            mul(&[u64::MAX, u64::MAX], &[u64::MAX]).unwrap(),
            [1, u64::MAX, u64::MAX - 1]
        );
        assert_eq!(cmp(&[1, 2], &[2, 1]), Ordering::Greater);
        assert_eq!(cmp(&[1], &[2, 1]), Ordering::Less);
        assert_eq!(bit_len(&[0, 1]), 65);
        assert_eq!(bits_at(&[0xF0, 0xAB], 4), 0xB000_0000_0000_000F);
        assert!(any_bit_below(&[0, 2], 66));
        assert!(!any_bit_below(&[0, 4], 66));
    }
}
