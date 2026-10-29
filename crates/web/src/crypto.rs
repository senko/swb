//! `crypto` (Web Cryptography §10,
//! <https://w3c.github.io/webcrypto/#crypto-interface>): `getRandomValues`
//! and `randomUUID`, from the operating system's random source. There is no
//! `crypto.subtle` (ADR 0028 section 13).
//!
//! Chromium's behaviour, measured in `tools/probes/web-crypto-encoding.json`:
//! only integer typed arrays (a `DataView` or a float array is a
//! `TypeMismatchError`), at most 65,536 bytes (a `QuotaExceededError` with
//! the length in the message), no resizable buffer, and a detached buffer
//! gives a view of length 0.

use std::fmt::Write;

use swb_js::{Gc, Object, Slots};

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;
use crate::state::WebState;

/// The most bytes of one `getRandomValues` call (Web Crypto §10.1.1).
const MAX_BYTES: usize = 65_536;

/// Fills `bytes` from the operating system's random source.
pub(crate) fn fill(bytes: &mut [u8]) -> b::Result<()> {
    getrandom::fill(bytes).map_err(|error| {
        log::warn!("the operating system's random source failed: {error}");
        Exception::dom("OperationError", "The random source failed.")
    })
}

/// A version 4 UUID in its canonical text (RFC 9562 §5.4).
pub(crate) fn random_uuid() -> b::Result<String> {
    let mut bytes = [0u8; 16];
    fill(&mut bytes)?;
    if let Some(byte) = bytes.get_mut(6) {
        *byte = (*byte & 0x0f) | 0x40;
    }
    if let Some(byte) = bytes.get_mut(8) {
        *byte = (*byte & 0x3f) | 0x80;
    }
    let mut text = String::with_capacity(36);
    for (i, byte) in bytes.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            text.push('-');
        }
        // Writing to a `String` does not fail.
        let _ = write!(text, "{byte:02x}");
    }
    Ok(text)
}

impl g::crypto::CryptoImpl for WebState {
    fn get_random_values(
        cx: &mut Cx<'_>,
        _this: Platform,
        array: Gc<Object>,
    ) -> b::Result<Gc<Object>> {
        let Slots::View {
            element,
            buffer,
            byte_offset,
            byte_length,
        } = cx.rt.slots_of(array)?
        else {
            return Err(Exception::NotOfType("ArrayBufferView".into()));
        };
        let type_name = element.unwrap_or("DataView");
        if element.is_none_or(|name| name.starts_with("Float")) {
            return Err(Exception::dom(
                "TypeMismatchError",
                format!(
                    "The provided ArrayBufferView is of type '{type_name}', which is not an integer array type."
                ),
            ));
        }
        let (buffer_length, resizable) = match cx.rt.slots_of(buffer)? {
            Slots::ArrayBuffer {
                byte_length,
                max_byte_length,
                ..
            } => (byte_length, max_byte_length.is_some()),
            _ => (0, false),
        };
        if resizable {
            return Err(Exception::type_error(
                "The provided ArrayBufferView value must not be resizable.",
            ));
        }
        let length = byte_length.unwrap_or_else(|| buffer_length.saturating_sub(byte_offset));
        if length > MAX_BYTES {
            return Err(Exception::dom(
                "QuotaExceededError",
                format!(
                    "The ArrayBufferView's byte length ({length}) exceeds the number of bytes of entropy available via this API ({MAX_BYTES})."
                ),
            ));
        }
        let mut bytes = vec![0u8; length];
        fill(&mut bytes)?;
        cx.rt.write_buffer_bytes(buffer, byte_offset, &bytes)?;
        Ok(array)
    }

    fn random_uuid(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        random_uuid()
    }
}

/// The `crypto` attribute of `Window` ([`SameObject`]).
pub(crate) fn crypto_object(cx: &mut Cx<'_>) -> b::Result<Gc<Object>> {
    if let Some(object) = crate::state::state(cx.rt)?.crypto {
        return Ok(object);
    }
    let object = cx
        .rt
        .new_host_object(cx.realm(), g::CRYPTO, crate::state::NOT_A_NODE)?;
    crate::state::state_mut(cx.rt)?.crypto = Some(object);
    Ok(object)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_uuid_has_the_version_and_variant_bits() {
        let a = random_uuid().expect("the random source works");
        let b = random_uuid().expect("the random source works");
        assert_eq!(a.len(), 36);
        assert_ne!(a, b);
        let parts: Vec<&str> = a.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            [8, 4, 4, 4, 12]
        );
        assert!(parts[2].starts_with('4'));
        assert!(matches!(parts[3].as_bytes()[0], b'8' | b'9' | b'a' | b'b'));
    }

    #[test]
    fn the_random_source_fills_a_buffer() {
        let mut bytes = [0u8; 64];
        fill(&mut bytes).expect("the random source works");
        assert!(bytes.iter().any(|&b| b != 0));
    }
}
