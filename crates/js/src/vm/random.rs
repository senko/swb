//! The generator of `Math.random` (ECMA-262 §21.3.2.27): xorshift128+
//! (S. Vigna, "Further scramblings of Marsaglia's xorshift generators",
//! Journal of Computational and Applied Mathematics 315, 2017, with the
//! shifts 23, 18 and 5 of that paper), one per runtime. It is fast and
//! passes the usual statistical tests, but it is not cryptographically
//! secure, which the specification does not ask for. The seed comes from
//! the standard library's random hash keys (`RandomState`), the source of
//! the atom table's seed.

use std::hash::{BuildHasher, RandomState};

/// A xorshift128+ state; never all zero.
#[derive(Clone, Debug)]
pub(crate) struct RandomSource {
    state: [u64; 2],
}

impl RandomSource {
    /// A generator with a random seed.
    pub(crate) fn new() -> Self {
        let keys = RandomState::new();
        let first = keys.hash_one(0x9E37_79B9_7F4A_7C15u64);
        let second = keys.hash_one(0xD1B5_4A32_D192_ED03u64);
        RandomSource {
            state: [first, if first == 0 && second == 0 { 1 } else { second }],
        }
    }

    /// The next 64 random bits.
    fn next_u64(&mut self) -> u64 {
        let [mut s1, s0] = self.state;
        let result = s0.wrapping_add(s1);
        s1 ^= s1 << 23;
        self.state = [s0, s1 ^ s0 ^ (s1 >> 18) ^ (s0 >> 5)];
        result
    }

    /// A double in [0, 1) with 53 random bits.
    pub(crate) fn next_double(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doubles_are_in_the_unit_interval_and_vary() {
        let mut source = RandomSource::new();
        let values: Vec<f64> = (0..10_000).map(|_| source.next_double()).collect();
        assert!(values.iter().all(|v| (0.0..1.0).contains(v)));
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        assert!((mean - 0.5).abs() < 0.02, "{mean}");
        let mut sorted = values.clone();
        sorted.sort_by(f64::total_cmp);
        sorted.dedup();
        assert!(sorted.len() > 9_990);
        // Two runtimes get different sequences.
        let mut other = RandomSource::new();
        assert_ne!(other.next_double(), values[0]);
    }
}
