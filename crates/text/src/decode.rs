//! Web font file formats: WOFF 2.0, WOFF 1.0 and plain OpenType/TrueType
//! (also collections), decoded to OpenType (sfnt) data that skrifa and
//! harfrust read.
//!
//! - WOFF 2.0: <https://www.w3.org/TR/WOFF2/> (Brotli, the `glyf`/`loca`
//!   and `hmtx` transforms).
//! - WOFF 1.0: <https://www.w3.org/TR/WOFF/> (zlib per table).
//!
//! The container formats are decoded by the `wuff` crate (ADR 0022), with
//! swb's own Brotli and zlib decompressors, so that swb's size limits act
//! before memory is allocated:
//!
//! - the file: at most [`MAX_FONT_FILE_SIZE`];
//! - the decompressed data (the WOFF 2.0 stream, the sum of the WOFF 1.0
//!   table sizes): at most [`MAX_DECOMPRESSED_SIZE`], checked against the
//!   sizes that the file declares before decompression starts;
//! - the decoded font: at most [`MAX_DECODED_SIZE`].
//!
//! The format comes from the first four bytes, not from the `format()`
//! hint or the content type (as in Chromium: a TrueType file with the hint
//! `woff` loads).

use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use crate::error::TextError;

/// The largest web font file that swb decodes.
pub(crate) const MAX_FONT_FILE_SIZE: usize = 32 << 20;

/// The largest decompressed data of a WOFF file (the WOFF 2.0 Brotli
/// stream, or all WOFF 1.0 tables). Larger files are rejected before
/// decompression (a decompression bomb check).
pub(crate) const MAX_DECOMPRESSED_SIZE: usize = 32 << 20;

/// The largest decoded font. The WOFF 2.0 `glyf` transform can make the
/// font larger than the decompressed stream (about 2.5 times in the worst
/// case), so this is checked after decoding.
pub(crate) const MAX_DECODED_SIZE: usize = 64 << 20;

/// The input buffer of the Brotli decoder.
const BROTLI_BUFFER_SIZE: usize = 4096;

/// The container format of a font file.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Format {
    Woff2,
    Woff,
    /// OpenType, TrueType or a collection: used as it is.
    Sfnt,
}

fn format_of(data: &[u8]) -> Option<Format> {
    match data.get(..4)? {
        b"wOF2" => Some(Format::Woff2),
        b"wOFF" => Some(Format::Woff),
        [0, 1, 0, 0] | b"OTTO" | b"true" | b"ttcf" => Some(Format::Sfnt),
        _ => None,
    }
}

/// Decodes a downloaded font file to OpenType data. `label` names the
/// file in errors (the URL).
pub fn decode_web_font(label: &str, data: &[u8]) -> Result<Arc<[u8]>, TextError> {
    let invalid = |reason: String| TextError::InvalidFont {
        path: label.into(),
        index: 0,
        reason,
    };
    if data.len() > MAX_FONT_FILE_SIZE {
        return Err(invalid(format!(
            "the file is larger than {MAX_FONT_FILE_SIZE} bytes"
        )));
    }
    let format = format_of(data).ok_or_else(|| invalid("unknown font format".to_owned()))?;
    let decoded = match format {
        Format::Sfnt => return Ok(Arc::from(data)),
        Format::Woff2 => run_decoder(|| woff2(data)),
        Format::Woff => run_decoder(|| woff1(data)),
    }
    .map_err(invalid)?;
    if decoded.len() > MAX_DECODED_SIZE {
        return Err(invalid(format!(
            "the decoded font is larger than {MAX_DECODED_SIZE} bytes"
        )));
    }
    if format_of(&decoded) != Some(Format::Sfnt) {
        return Err(invalid(
            "the decoded data is not an OpenType font".to_owned(),
        ));
    }
    Ok(Arc::from(decoded))
}

/// Runs a decoder of the third-party `wuff` crate. A panic in it (a bug
/// that malformed input reaches) is caught and becomes an error, as for
/// SVG images (ADR 0011).
fn run_decoder(decode: impl FnOnce() -> Result<Vec<u8>, String>) -> Result<Vec<u8>, String> {
    catch_unwind(AssertUnwindSafe(decode))
        .unwrap_or_else(|_| Err("the font decoder panicked".to_owned()))
}

fn woff2(data: &[u8]) -> Result<Vec<u8>, String> {
    wuff::decompress_woff2_with_custom_brotli(data, &mut |stream, size| {
        decompress(
            brotli_decompressor::Decompressor::new(stream, BROTLI_BUFFER_SIZE),
            size,
        )
    })
    .map_err(|e| format!("invalid WOFF2 data ({e})"))
}

fn woff1(data: &[u8]) -> Result<Vec<u8>, String> {
    let total = woff1_decompressed_size(data).ok_or("invalid WOFF table directory")?;
    if total > MAX_DECOMPRESSED_SIZE as u64 {
        return Err(format!(
            "the WOFF tables are larger than {MAX_DECOMPRESSED_SIZE} bytes"
        ));
    }
    wuff::decompress_woff1_with_custom_z(data, &mut |stream, size| {
        decompress(flate2::read::ZlibDecoder::new(stream), size)
    })
    .map_err(|e| format!("invalid WOFF data ({e})"))
}

/// Decompresses exactly `size` bytes from `decoder`. Fails if `size` is
/// above the limit (before anything is allocated), or if the data is
/// shorter or longer.
fn decompress(decoder: impl Read, size: usize) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if size > MAX_DECOMPRESSED_SIZE {
        return Err(format!("decompressed data larger than {MAX_DECOMPRESSED_SIZE} bytes").into());
    }
    let mut out = Vec::with_capacity(size);
    decoder.take(size as u64 + 1).read_to_end(&mut out)?;
    if out.len() != size {
        return Err("the decompressed size does not match".into());
    }
    Ok(out)
}

/// The size of the font that a WOFF 1.0 file decodes to, from its table
/// directory: the sum of the original table lengths (each padded to four
/// bytes) and the sfnt table directory. `None` if the header or the
/// directory is truncated, or a table lies outside the file.
/// <https://www.w3.org/TR/WOFF/#WOFFHeader>
fn woff1_decompressed_size(data: &[u8]) -> Option<u64> {
    const HEADER: usize = 44;
    const ENTRY: usize = 20;
    let be16 = |at: usize| Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?));
    let be32 = |at: usize| Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?));
    let tables = usize::from(be16(12)?);
    let mut total = 12 + 16 * tables as u64;
    for i in 0..tables {
        let entry = HEADER + i * ENTRY;
        let offset = u64::from(be32(entry + 4)?);
        let compressed = u64::from(be32(entry + 8)?);
        let original = u64::from(be32(entry + 12)?);
        if offset + compressed > data.len() as u64 {
            return None;
        }
        // An uncompressed table is copied with its stored length.
        total += original.max(compressed).div_ceil(4) * 4;
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn read(path: &str) -> Vec<u8> {
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap()
    }

    fn subset(ext: &str) -> Vec<u8> {
        read(&format!("tests/webfonts/dejavu-subset.{ext}"))
    }

    fn parses(data: &[u8]) -> bool {
        skrifa::FontRef::new(data).is_ok()
    }

    #[test]
    fn decodes_all_formats() {
        let ttf = subset("ttf");
        assert_eq!(&*decode_web_font("t", &ttf).unwrap(), &ttf[..]);
        for ext in ["woff", "woff2"] {
            let decoded = decode_web_font(ext, &subset(ext)).unwrap();
            assert!(parses(&decoded), "{ext}");
        }
        // A variable font from the Ars Technica fixture (glyf transform).
        let exo = read("../../fixtures/pages/ars-technica/files/d2f675f4572825d0.woff2");
        let decoded = decode_web_font("exo", &exo).unwrap();
        let font = skrifa::FontRef::new(&decoded).unwrap();
        assert_eq!(skrifa::MetadataProvider::axes(&font).len(), 1);
    }

    #[test]
    fn rejects_unknown_and_oversized_files() {
        assert!(decode_web_font("x", b"not a font").is_err());
        assert!(decode_web_font("x", b"").is_err());
        let mut huge = vec![0u8; MAX_FONT_FILE_SIZE + 1];
        huge[..4].copy_from_slice(b"wOF2");
        assert!(decode_web_font("x", &huge).is_err());
    }

    /// Truncated and corrupted copies of the test fonts fail to decode or
    /// decode to something; they never panic.
    #[test]
    fn damaged_files_do_not_panic() {
        for ext in ["woff", "woff2"] {
            let data = subset(ext);
            for len in (0..data.len()).step_by(97) {
                let _ = decode_web_font("t", &data[..len]);
            }
            let mut state = 0x1234_5678u32;
            for _ in 0..300 {
                let mut copy = data.clone();
                for _ in 0..4 {
                    // xorshift
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    // Half of the changes hit the header and the
                    // table directory.
                    let at = if state.is_multiple_of(2) {
                        state as usize % copy.len().min(300)
                    } else {
                        state as usize % copy.len()
                    };
                    copy[at] ^= (state >> 8) as u8 | 1;
                }
                if let Ok(font) = decode_web_font("t", &copy) {
                    let _ = skrifa::FontRef::new(&font);
                }
            }
        }
    }

    #[test]
    fn woff2_with_a_huge_declared_size_is_rejected_before_decompression() {
        // A WOFF2 header and one table (`glyf`, null transform) that
        // declares an original length of 2^31 bytes.
        let mut data = Vec::new();
        data.extend_from_slice(b"wOF2");
        data.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // flavor
        let length_at = data.len();
        data.extend_from_slice(&0u32.to_be_bytes()); // length (set below)
        data.extend_from_slice(&1u16.to_be_bytes()); // numTables
        data.extend_from_slice(&0u16.to_be_bytes()); // reserved
        data.extend_from_slice(&u32::MAX.to_be_bytes()); // totalSfntSize
        data.extend_from_slice(&16u32.to_be_bytes()); // totalCompressedSize
        data.extend_from_slice(&[0; 4]); // major, minor version
        data.extend_from_slice(&[0; 20]); // meta and private blocks
        // Table directory: `cmap` (known tag 0), transform 0; origLength
        // 2^31 as UIntBase128.
        data.push(0);
        data.extend_from_slice(&[0x88, 0x80, 0x80, 0x80, 0x00]);
        data.extend_from_slice(&[0; 16]); // "compressed" data
        let len = data.len() as u32;
        data[length_at..length_at + 4].copy_from_slice(&len.to_be_bytes());
        assert!(decode_web_font("bomb", &data).is_err());
    }

    #[test]
    fn woff1_with_a_huge_table_sum_is_rejected_before_decompression() {
        // 1,000 tables of 40,000 bytes each (each far below the limit)
        // that all point at the same 16 bytes: the sum of `origLength`
        // is 40 MB, above `MAX_DECOMPRESSED_SIZE`.
        let tables: u16 = 1000;
        let data_at = 44 + 20 * u32::from(tables);
        let mut data = Vec::new();
        data.extend_from_slice(b"wOFF");
        data.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // flavor
        data.extend_from_slice(&(data_at + 16).to_be_bytes()); // length
        data.extend_from_slice(&tables.to_be_bytes());
        data.extend_from_slice(&[0; 2]); // reserved
        data.extend_from_slice(&u32::MAX.to_be_bytes()); // totalSfntSize
        data.extend_from_slice(&[0; 24]); // versions, meta, private
        for i in 0..tables {
            data.extend_from_slice(format!("t{i:03}").as_bytes());
            data.extend_from_slice(&data_at.to_be_bytes()); // offset
            data.extend_from_slice(&16u32.to_be_bytes()); // compLength
            data.extend_from_slice(&40_000u32.to_be_bytes()); // origLength
            data.extend_from_slice(&0u32.to_be_bytes()); // checksum
        }
        data.extend_from_slice(&[0; 16]);
        let total = woff1_decompressed_size(&data).unwrap();
        assert!(total > MAX_DECOMPRESSED_SIZE as u64, "{total}");
        let error = decode_web_font("bomb", &data).unwrap_err().to_string();
        assert!(error.contains("larger than"), "{error}");
    }

    #[test]
    fn woff1_directory_size() {
        let data = subset("woff");
        let total = woff1_decompressed_size(&data).unwrap();
        let decoded = decode_web_font("t", &data).unwrap();
        assert!(total >= decoded.len() as u64);
        assert!(woff1_decompressed_size(&data[..50]).is_none());
    }
}
