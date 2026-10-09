//! A fast hash for tables keyed by handles (transitions, root shapes,
//! shape and dictionary indices).
//!
//! The keys are slot indices and generations that the heap assigns, not
//! text from the network, so a multiply-rotate hash is enough. Its key
//! is random per process, so a page cannot compute in advance which
//! handles collide. Tables keyed by string content use the atom table's
//! `SipHash` instead (ADR 0026 section 7).

use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher, RandomState};
use std::sync::OnceLock;

/// A hash map keyed by handles.
pub(crate) type HandleMap<K, V> = HashMap<K, V, BuildHandleHasher>;

/// Builds [`HandleHasher`]s with the process's random key.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BuildHandleHasher {
    key: u64,
}

impl Default for BuildHandleHasher {
    fn default() -> Self {
        static KEY: OnceLock<u64> = OnceLock::new();
        let key = *KEY.get_or_init(|| RandomState::new().hash_one(0x5EED_u64));
        BuildHandleHasher { key }
    }
}

impl BuildHasher for BuildHandleHasher {
    type Hasher = HandleHasher;

    fn build_hasher(&self) -> HandleHasher {
        HandleHasher(self.key)
    }
}

/// A multiply-rotate hasher over 64-bit words, with a final mix.
pub(crate) struct HandleHasher(u64);

impl HandleHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(0x517C_C1B7_2722_0A95);
    }
}

impl Hasher for HandleHasher {
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
