//! Image decoding into premultiplied RGBA pixmaps.

use image::ImageDecoder;
use thiserror::Error;
use tiny_skia::{IntSize, Pixmap};

/// A decoded image.
#[derive(Clone, Debug)]
pub struct DecodedImage {
    /// The pixels, premultiplied RGBA.
    pub pixmap: Pixmap,
}

impl DecodedImage {
    /// The natural size in CSS px (one image pixel is one CSS px).
    pub fn natural_size(&self) -> (f32, f32) {
        (self.pixmap.width() as f32, self.pixmap.height() as f32)
    }
}

/// Image decoding errors.
#[derive(Debug, Error)]
pub enum ImageError {
    /// The format is not supported or not recognized.
    #[error("unsupported image format")]
    Unsupported,
    /// The data is corrupt.
    #[error("cannot decode image: {0}")]
    Decode(String),
    /// The image is too large.
    #[error("image too large: {0}x{1}")]
    TooLarge(u32, u32),
}

/// Images larger than this many pixels in either dimension are rejected.
const MAX_DIMENSION: u32 = 16_384;

/// Images with more pixels than this are rejected: 64 Mpx is 256 MiB as
/// RGBA.
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

/// The allocation limit for the decoder: enough for `MAX_PIXELS` with 16
/// bits per channel.
const MAX_DECODER_ALLOC: u64 = MAX_PIXELS * 8;

/// Decodes PNG, JPEG, GIF (first frame), WebP, BMP and ICO data.
///
/// The size is checked from the image header before the pixels are
/// decoded, so a small file that declares a huge image costs no memory.
pub fn decode(data: &[u8]) -> Result<DecodedImage, ImageError> {
    let format = image::guess_format(data).map_err(|_| ImageError::Unsupported)?;
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(data), format);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_DECODER_ALLOC);
    reader.limits(limits);
    let decoder = reader.into_decoder().map_err(|e| decode_error(&e))?;
    let (w, h) = decoder.dimensions();
    if w > MAX_DIMENSION || h > MAX_DIMENSION || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(ImageError::TooLarge(w, h));
    }
    let decoded = image::DynamicImage::from_decoder(decoder).map_err(|e| decode_error(&e))?;
    let rgba = decoded.into_rgba8();
    let (w, h) = rgba.dimensions();
    let mut data = rgba.into_raw();
    premultiply(&mut data);
    let pixmap = match IntSize::from_wh(w, h) {
        Some(size) => Pixmap::from_vec(data, size)
            .ok_or_else(|| ImageError::Decode("bad buffer size".into()))?,
        // A zero-size image: use one transparent pixel.
        None => Pixmap::new(1, 1).ok_or(ImageError::TooLarge(w, h))?,
    };
    Ok(DecodedImage { pixmap })
}

fn decode_error(e: &image::ImageError) -> ImageError {
    ImageError::Decode(e.to_string())
}

fn premultiply(data: &mut [u8]) {
    let (pixels, _) = data.as_chunks_mut::<4>();
    for px in pixels {
        let a = u16::from(px[3]);
        if a == 255 {
            continue;
        }
        for c in &mut px[..3] {
            *c = ((u16::from(*c) * a + 127) / 255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_1x1(rgba: [u8; 4]) -> Vec<u8> {
        let mut out = Vec::new();
        let img = image::RgbaImage::from_raw(1, 1, rgba.to_vec()).unwrap();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn decodes_png_and_premultiplies() {
        let image = decode(&png_1x1([255, 0, 0, 128])).unwrap();
        assert_eq!(image.natural_size(), (1.0, 1.0));
        let px = image.pixmap.pixel(0, 0).unwrap();
        assert_eq!(px.alpha(), 128);
        assert_eq!(px.red(), 128);
    }

    /// The header of a 24-bit BMP file without pixel data.
    fn bmp_header(width: i32, height: i32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"BM");
        out.extend_from_slice(&54u32.to_le_bytes()); // file size
        out.extend_from_slice(&[0; 4]); // reserved
        out.extend_from_slice(&54u32.to_le_bytes()); // pixel data offset
        out.extend_from_slice(&40u32.to_le_bytes()); // info header size
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&24u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&[0; 24]); // compression, sizes, colors
        out
    }

    #[test]
    fn rejects_huge_images_before_decoding() {
        // 54 bytes that declare a 20000x20000 image: decoding would need
        // 1.6 GB of RGBA.
        assert!(matches!(
            decode(&bmp_header(20_000, 20_000)),
            Err(ImageError::TooLarge(20_000, 20_000))
        ));
        assert!(matches!(
            decode(&bmp_header(16_000, 16_000)),
            Err(ImageError::TooLarge(16_000, 16_000))
        ));
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode(b"not an image").is_err());
    }
}
