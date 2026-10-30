//! `TextEncoder` and `TextDecoder` (Encoding Standard §8,
//! <https://encoding.spec.whatwg.org/#interface-textdecoder>) over
//! `encoding_rs`, which implements the standard's decoders.
//!
//! `TextEncoder` only makes UTF-8. `TextDecoder` keeps an
//! `encoding_rs::Decoder`, so `decode(input, { stream: true })` carries an
//! incomplete sequence and the "BOM seen" flag to the next call; a call
//! without `stream`, or an error, makes a new decoder (the standard's "do
//! not flush" flag and "BOM seen" flag reset). The labels are the standard's
//! (`Encoding::for_label`); `replacement` and the labels that map to it are a
//! `RangeError`.
//!
//! Limit: one `decode` takes at most 64 MiB of input (a `RangeError`;
//! Chromium decodes 300 MiB, measured): the decoder works in chunks of 1 MiB,
//! but the result is one string on the heap of the script.

use encoding_rs::{CoderResult, DecoderResult, Encoding};
use swb_js::{Gc, GenericData, HostData, Object, Runtime, Slots, Tracer};

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;
use crate::generated::types::{TextDecodeOptions, TextDecoderOptions, TextEncoderEncodeIntoResult};
use crate::state::{NOT_A_NODE, WebState};

/// The most input bytes of one `decode` call.
pub(crate) const MAX_DECODE_BYTES: usize = 64 << 20;

/// The bytes that one step of the decoder takes.
const CHUNK: usize = 1 << 20;

/// The host data of a `TextDecoder`.
struct DecoderData {
    encoding: &'static Encoding,
    fatal: bool,
    ignore_bom: bool,
    decoder: encoding_rs::Decoder,
}

impl DecoderData {
    fn new_decoder(encoding: &'static Encoding, ignore_bom: bool) -> encoding_rs::Decoder {
        if ignore_bom {
            encoding.new_decoder_without_bom_handling()
        } else {
            encoding.new_decoder_with_bom_removal()
        }
    }

    /// Starts again: the end of the stream or an error.
    fn reset(&mut self) {
        self.decoder = Self::new_decoder(self.encoding, self.ignore_bom);
    }
}

impl GenericData for DecoderData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        0
    }
}

fn decoder_data(rt: &mut Runtime, this: Platform) -> b::Result<&mut DecoderData> {
    match this.data {
        HostData::Generic(handle) => rt
            .heap_mut()
            .generic_mut::<DecoderData>(handle)
            .map_err(|_| Exception::type_error("Illegal invocation")),
        HostData::Word(_) => Err(Exception::type_error("Illegal invocation")),
    }
}

/// The bytes of an `ArrayBuffer` or a view: `(buffer, offset, length)`.
pub(crate) fn byte_range(
    rt: &mut Runtime,
    object: Gc<Object>,
) -> b::Result<(Gc<Object>, usize, usize)> {
    Ok(match rt.slots_of(object)? {
        Slots::ArrayBuffer { byte_length, .. } => (object, 0, byte_length),
        Slots::View {
            buffer,
            byte_offset,
            byte_length,
            ..
        } => {
            let length = match byte_length {
                Some(length) => length,
                None => match rt.slots_of(buffer)? {
                    Slots::ArrayBuffer { byte_length, .. } => {
                        byte_length.saturating_sub(byte_offset)
                    }
                    _ => 0,
                },
            };
            (buffer, byte_offset, length)
        }
        _ => (object, 0, 0),
    })
}

/// Decodes `bytes` (Encoding Standard "decode" and "serialize I/O queue"):
/// `None` for a fatal error. `last` ends the stream.
fn decode_bytes(data: &mut DecoderData, bytes: &[u8], last: bool) -> Option<String> {
    let mut output = String::new();
    let mut rest = bytes;
    loop {
        let (chunk, tail) = rest.split_at(rest.len().min(CHUNK));
        let is_last = last && tail.is_empty();
        let mut chunk = chunk;
        loop {
            let needed = if data.fatal {
                data.decoder
                    .max_utf8_buffer_length_without_replacement(chunk.len())
            } else {
                data.decoder.max_utf8_buffer_length(chunk.len())
            }?;
            output.reserve(needed);
            let read = if data.fatal {
                let (result, read) =
                    data.decoder
                        .decode_to_string_without_replacement(chunk, &mut output, is_last);
                match result {
                    DecoderResult::Malformed(..) => return None,
                    DecoderResult::InputEmpty => break,
                    DecoderResult::OutputFull => read,
                }
            } else {
                let (result, read, _) = data.decoder.decode_to_string(chunk, &mut output, is_last);
                if matches!(result, CoderResult::InputEmpty) {
                    break;
                }
                read
            };
            chunk = chunk.get(read..).unwrap_or_default();
        }
        if tail.is_empty() {
            return Some(output);
        }
        rest = tail;
    }
}

impl g::text_decoder::TextDecoderImpl for WebState {
    fn encoding(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(decoder_data(cx.rt, this)?
            .encoding
            .name()
            .to_ascii_lowercase())
    }

    fn fatal(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(decoder_data(cx.rt, this)?.fatal)
    }

    fn ignore_bom(cx: &mut Cx<'_>, this: Platform) -> b::Result<bool> {
        Ok(decoder_data(cx.rt, this)?.ignore_bom)
    }

    fn constructor(
        cx: &mut Cx<'_>,
        label: String,
        options: TextDecoderOptions,
    ) -> b::Result<Gc<Object>> {
        let encoding = match Encoding::for_label(label.as_bytes()) {
            Some(encoding) if encoding != encoding_rs::REPLACEMENT => encoding,
            _ => {
                return Err(Exception::range_error(format!(
                    "The encoding label provided ('{label}') is invalid."
                )));
            }
        };
        let data = cx.rt.alloc_host_data(DecoderData {
            encoding,
            fatal: options.fatal,
            ignore_bom: options.ignore_bom,
            decoder: DecoderData::new_decoder(encoding, options.ignore_bom),
        })?;
        Ok(cx.new_instance::<WebState>(data)?)
    }

    fn decode(
        cx: &mut Cx<'_>,
        this: Platform,
        input: Option<Gc<Object>>,
        options: TextDecodeOptions,
    ) -> b::Result<String> {
        let bytes = match input {
            Some(object) => {
                let (buffer, offset, length) = byte_range(cx.rt, object)?;
                if length > MAX_DECODE_BYTES {
                    return Err(Exception::range_error(format!(
                        "The buffer of {length} bytes is too large to decode (the limit is {MAX_DECODE_BYTES})."
                    )));
                }
                cx.rt.buffer_range(buffer, offset, length)?
            }
            None => Vec::new(),
        };
        let data = decoder_data(cx.rt, this)?;
        let result = decode_bytes(data, &bytes, !options.stream);
        if !options.stream || result.is_none() {
            data.reset();
        }
        result.ok_or_else(|| Exception::type_error("The encoded data was not valid."))
    }
}

impl g::text_encoder::TextEncoderImpl for WebState {
    fn encoding(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok("utf-8".to_owned())
    }

    fn constructor(cx: &mut Cx<'_>) -> b::Result<Gc<Object>> {
        Ok(cx.new_instance::<WebState>(NOT_A_NODE)?)
    }

    fn encode(cx: &mut Cx<'_>, _this: Platform, input: String) -> b::Result<Gc<Object>> {
        let length = input.len();
        let realm = cx.realm();
        let buffer = cx.rt.create_array_buffer(realm, input.into_bytes(), None)?;
        Ok(cx
            .rt
            .create_view(realm, Some("Uint8"), buffer, 0, Some(length))?)
    }

    fn encode_into(
        cx: &mut Cx<'_>,
        _this: Platform,
        source: String,
        destination: Gc<Object>,
    ) -> b::Result<TextEncoderEncodeIntoResult> {
        let (buffer, offset, length) = byte_range(cx.rt, destination)?;
        // The longest prefix of whole code points that fits.
        let mut written = 0;
        let mut read = 0;
        for (index, c) in source.char_indices() {
            if index + c.len_utf8() > length {
                break;
            }
            written = index + c.len_utf8();
            read += c.len_utf16();
        }
        let prefix = source.as_bytes().get(..written).unwrap_or_default();
        cx.rt.write_buffer_bytes(buffer, offset, prefix)?;
        Ok(TextEncoderEncodeIntoResult {
            read: Some(read as u64),
            written: Some(written as u64),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(label: &str, fatal: bool, ignore_bom: bool) -> DecoderData {
        let encoding = Encoding::for_label(label.as_bytes()).expect("a label");
        DecoderData {
            encoding,
            fatal,
            ignore_bom,
            decoder: DecoderData::new_decoder(encoding, ignore_bom),
        }
    }

    #[test]
    fn a_stream_carries_a_partial_sequence() {
        let mut d = data("utf-8", false, false);
        assert_eq!(
            decode_bytes(&mut d, &[0xe2, 0x82], false).as_deref(),
            Some("")
        );
        assert_eq!(decode_bytes(&mut d, &[0xac], true).as_deref(), Some("€"));
    }

    #[test]
    fn the_end_of_the_stream_flushes_a_partial_sequence() {
        let mut d = data("utf-8", false, false);
        assert_eq!(
            decode_bytes(&mut d, &[0xe2, 0x82], true).as_deref(),
            Some("\u{fffd}")
        );
    }

    #[test]
    fn the_bom_goes_once() {
        let mut d = data("utf-8", false, false);
        assert_eq!(
            decode_bytes(&mut d, &[0xef, 0xbb], false).as_deref(),
            Some("")
        );
        assert_eq!(
            decode_bytes(&mut d, &[0xbf, 0x61], false).as_deref(),
            Some("a")
        );
        assert_eq!(
            decode_bytes(&mut d, &[0xef, 0xbb, 0xbf], false).as_deref(),
            Some("\u{feff}")
        );
        let mut keep = data("utf-8", false, true);
        assert_eq!(
            decode_bytes(&mut keep, &[0xef, 0xbb, 0xbf, 0x61], true).as_deref(),
            Some("\u{feff}a")
        );
    }

    #[test]
    fn a_fatal_decoder_refuses_bad_input() {
        let mut d = data("utf-8", true, false);
        assert_eq!(decode_bytes(&mut d, &[0x61, 0xff], true), None);
        let mut d = data("utf-8", true, false);
        assert_eq!(decode_bytes(&mut d, &[0xe2, 0x82], true), None);
        let mut d = data("utf-16le", true, false);
        assert_eq!(decode_bytes(&mut d, &[0x00, 0xd8], true), None);
    }

    #[test]
    fn legacy_encodings_decode() {
        let mut d = data("latin1", false, false);
        assert_eq!(
            decode_bytes(&mut d, &[0x80, 0xe9], true).as_deref(),
            Some("€é")
        );
        let mut d = data("shift_jis", false, false);
        assert_eq!(
            decode_bytes(&mut d, &[0x82, 0xa0, 0x82, 0xa2], true).as_deref(),
            Some("あい")
        );
    }

    #[test]
    fn a_long_input_decodes_in_chunks() {
        let mut d = data("windows-1252", false, false);
        let bytes = vec![0x80u8; 3 * CHUNK + 5];
        let text = decode_bytes(&mut d, &bytes, true).expect("decodes");
        assert_eq!(text.chars().count(), 3 * CHUNK + 5);
    }
}
