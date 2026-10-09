//! A fast keyed hasher for tables whose keys are small integers or
//! handles: the scope analysis (scope and name ids) and the heap
//! (transitions, root shapes, shape and dictionary indices).
//!
//! The keys are indices that the engine assigns, not text from the
//! network, so a multiply-rotate hash is enough. Its key is random per
//! process, so a page cannot compute in advance which keys collide.
//! Tables keyed by string content use the atom table's `SipHash` instead
//! (ADR 0026 section 7).

use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher, RandomState};
use std::sync::OnceLock;

/// A map with the keyed hasher.
pub type KeyedMap<K, V> = HashMap<K, V, BuildKeyedHasher>;

/// Builds [`KeyedHasher`]s with the process's random key.
#[derive(Clone, Copy, Debug)]
pub struct BuildKeyedHasher {
    key: u64,
}

impl Default for BuildKeyedHasher {
    fn default() -> Self {
        static KEY: OnceLock<u64> = OnceLock::new();
        let key = *KEY.get_or_init(|| RandomState::new().hash_one(0x5EED_u64));
        BuildKeyedHasher { key }
    }
}

impl BuildHasher for BuildKeyedHasher {
    type Hasher = KeyedHasher;

    fn build_hasher(&self) -> KeyedHasher {
        KeyedHasher(self.key)
    }
}

/// A multiply-rotate hasher over 64-bit words, with a final mix.
pub struct KeyedHasher(u64);

impl KeyedHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(0x517C_C1B7_2722_0A95);
    }
}

impl Hasher for KeyedHasher {
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.add(u64::from_le_bytes(word));
        }
    }

    #[inline]
    fn write_u8(&mut self, value: u8) {
        self.add(u64::from(value));
    }

    #[inline]
    fn write_u32(&mut self, value: u32) {
        self.add(u64::from(value));
    }

    #[inline]
    fn write_u64(&mut self, value: u64) {
        self.add(value);
    }

    #[inline]
    fn write_usize(&mut self, value: usize) {
        self.add(value as u64);
    }

    #[inline]
    fn write_isize(&mut self, value: isize) {
        self.add(value as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        // The final mix of MurmurHash3, so that the high bits (which the
        // table uses for its control bytes) depend on all input bits.
        let mut x = self.0;
        x ^= x >> 33;
        x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
        x ^= x >> 33;
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_keyed_map_stores_integer_and_pair_keys() {
        let mut ids: KeyedMap<u64, u32> = KeyedMap::default();
        for key in 0..1000_u64 {
            ids.insert(key << 32 | (key % 7), key as u32);
        }
        assert_eq!(ids.len(), 1000);
        assert_eq!(ids.get(&(5 << 32 | 5)), Some(&5));
        assert_eq!(ids.get(&6), None);
    }

    #[test]
    fn the_key_is_shared_by_all_builders() {
        let one = BuildKeyedHasher::default();
        let two = BuildKeyedHasher::default();
        assert_eq!(one.hash_one(42_u64), two.hash_one(42_u64));
        assert_ne!(one.hash_one(1_u64), one.hash_one(2_u64));
    }
}
