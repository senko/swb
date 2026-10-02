//! Decoding of `Content-Encoding`: `gzip`, `deflate` and `br`.
//!
//! <https://www.rfc-editor.org/rfc/rfc9110#name-content-encoding>

use std::io::{self, Read};

use flate2::read::{DeflateDecoder, MultiGzDecoder, ZlibDecoder};
use log::warn;

use crate::error::NetError;
use crate::headers::Headers;

/// The buffer size of the brotli decoder.
const BROTLI_BUFFER_SIZE: usize = 4096;

/// The most content codings a response may have. Real servers use one;
/// a long chain of nested codings only costs CPU time.
const MAX_CODINGS: usize = 4;

/// Undoes the content codings in the `Content-Encoding` headers, last coding
/// first. An unknown coding stops decoding and leaves the body as it is at
/// that point. The decoded body must not be larger than `limit` bytes.
pub(crate) fn decode_body(
    body: Vec<u8>,
    headers: &Headers,
    limit: u64,
) -> Result<Vec<u8>, NetError> {
    let codings: Vec<String> = headers
        .get_all("content-encoding")
        .flat_map(|value| value.split(','))
        .map(|coding| coding.trim().to_ascii_lowercase())
        .filter(|coding| !coding.is_empty() && coding != "identity")
        .collect();
    if codings.len() > MAX_CODINGS {
        return Err(NetError::ContentDecoding {
            coding: codings.join(", "),
            source: io::Error::new(io::ErrorKind::InvalidData, "too many content codings"),
        });
    }
    let mut body = body;
    for coding in codings.iter().rev() {
        // Responses to HEAD, 204 and 304 have no body, but can have a
        // Content-Encoding header.
        if body.is_empty() {
            break;
        }
        body = match coding.as_str() {
            "gzip" | "x-gzip" => read_all(MultiGzDecoder::new(body.as_slice()), coding, limit)?,
            "deflate" if has_zlib_header(&body) => {
                read_all(ZlibDecoder::new(body.as_slice()), coding, limit)?
            }
            // Some servers send raw deflate data without the zlib wrapper.
            "deflate" => read_all(DeflateDecoder::new(body.as_slice()), coding, limit)?,
            "br" => read_all(
                brotli_decompressor::Decompressor::new(body.as_slice(), BROTLI_BUFFER_SIZE),
                coding,
                limit,
            )?,
            _ => {
                warn!("unsupported content coding {coding:?}, body is not decoded");
                return Ok(body);
            }
        };
    }
    Ok(body)
}

/// Returns true if `data` starts with a valid zlib header (RFC 1950,
/// section 2.2): compression method 8 and a valid check value.
fn has_zlib_header(data: &[u8]) -> bool {
    match data {
        [cmf, flg, ..] => cmf & 0x0F == 8 && ((u16::from(*cmf) << 8) | u16::from(*flg)) % 31 == 0,
        _ => false,
    }
}

/// Reads the decoder to the end. If the data is damaged after some decoded
/// bytes, returns the decoded part and logs a warning: a truncated page is
/// more useful than no page.
fn read_all(decoder: impl Read, coding: &str, limit: u64) -> Result<Vec<u8>, NetError> {
    let mut output = Vec::new();
    let result = decoder
        .take(limit.saturating_add(1))
        .read_to_end(&mut output);
    if output.len() as u64 > limit {
        return Err(NetError::BodyTooLarge { limit });
    }
    match result {
        Ok(_) => Ok(output),
        Err(source) if output.is_empty() => Err(NetError::ContentDecoding {
            coding: coding.to_string(),
            source,
        }),
        Err(error) => {
            warn!(
                "{coding} body is damaged after {} decoded bytes: {error}",
                output.len()
            );
            Ok(output)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::Compression;
    use flate2::write::{DeflateEncoder, GzEncoder, ZlibEncoder};

    use super::*;

    const TEXT: &[u8] = b"hello hello hello hello compressed world";

    /// "hello brotli", compressed with Python's `brotli.compress`.
    const BROTLI_HELLO: &[u8] = b"\x8b\x05\x80hello brotli\x03";

    fn gzip(data: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    fn raw_deflate(data: &[u8]) -> Vec<u8> {
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    fn decode(body: Vec<u8>, encodings: &[&str]) -> Result<Vec<u8>, NetError> {
        let headers: Headers = encodings.iter().map(|e| ("Content-Encoding", *e)).collect();
        decode_body(body, &headers, 1000)
    }

    #[test]
    fn no_encoding() {
        assert_eq!(decode(TEXT.to_vec(), &[]).unwrap(), TEXT);
        assert_eq!(decode(TEXT.to_vec(), &["identity"]).unwrap(), TEXT);
    }

    #[test]
    fn gzip_and_x_gzip() {
        assert_eq!(decode(gzip(TEXT), &["gzip"]).unwrap(), TEXT);
        assert_eq!(decode(gzip(TEXT), &["X-GZIP"]).unwrap(), TEXT);
    }

    #[test]
    fn deflate_with_and_without_zlib_wrapper() {
        assert_eq!(decode(zlib(TEXT), &["deflate"]).unwrap(), TEXT);
        assert_eq!(decode(raw_deflate(TEXT), &["deflate"]).unwrap(), TEXT);
    }

    #[test]
    fn brotli() {
        assert_eq!(
            decode(BROTLI_HELLO.to_vec(), &["br"]).unwrap(),
            b"hello brotli"
        );
    }

    #[test]
    fn several_codings_are_undone_in_reverse_order() {
        let body = gzip(&zlib(TEXT));
        assert_eq!(decode(body.clone(), &["deflate, gzip"]).unwrap(), TEXT);
        assert_eq!(decode(body, &["deflate", "gzip"]).unwrap(), TEXT);
    }

    #[test]
    fn unknown_coding_leaves_body() {
        assert_eq!(decode(TEXT.to_vec(), &["zstd"]).unwrap(), TEXT);
    }

    #[test]
    fn empty_body_is_not_decoded() {
        assert_eq!(decode(Vec::new(), &["gzip"]).unwrap(), b"");
    }

    #[test]
    fn invalid_data_is_an_error() {
        assert!(matches!(
            decode(TEXT.to_vec(), &["gzip"]),
            Err(NetError::ContentDecoding { .. })
        ));
    }

    #[test]
    fn truncated_data_gives_the_decoded_part() {
        // 643 bytes, below the limit of 1000.
        let long: Vec<u8> = (0..250u32)
            .flat_map(|i| i.to_string().into_bytes())
            .collect();
        let mut body = gzip(&long);
        body.truncate(body.len() - 20);
        let decoded = decode(body, &["gzip"]).unwrap();
        assert_ne!(decoded, b"");
        assert!(long.starts_with(&decoded));
    }

    #[test]
    fn limit_applies_to_decoded_size() {
        let big = vec![b'a'; 1001];
        assert!(matches!(
            decode(gzip(&big), &["gzip"]),
            Err(NetError::BodyTooLarge { limit: 1000 })
        ));
        let fits = vec![b'a'; 1000];
        assert_eq!(decode(gzip(&fits), &["gzip"]).unwrap(), fits);
    }
}
