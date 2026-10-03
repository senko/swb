//! Image decoding: raster formats into premultiplied RGBA pixmaps, SVG
//! into a vector image (see [`crate::svg`]).

use image::ImageDecoder;
use swb_layout::NaturalSize;
use thiserror::Error;
use tiny_skia::{IntSize, Pixmap};

use crate::svg::SvgImage;

/// The MIME type of SVG images. Chromium decodes an image as SVG only if
/// its response has this type; it does not sniff SVG.
pub const SVG_MIME_TYPE: &str = "image/svg+xml";

/// A decoded image.
#[derive(Debug)]
pub struct DecodedImage {
    kind: ImageKind,
}

/// The two kinds of images.
#[derive(Debug)]
pub(crate) enum ImageKind {
    /// Pixels, premultiplied RGBA. One pixel is one CSS px.
    Raster(Pixmap),
    /// An SVG image, rendered at the size it is drawn at.
    Vector(Box<SvgImage>),
}

impl DecodedImage {
    /// The natural dimensions in CSS px. A raster image has both; an SVG
    /// image can lack any of them.
    pub fn natural_size(&self) -> NaturalSize {
        match &self.kind {
            ImageKind::Raster(pixmap) => {
                NaturalSize::fixed(pixmap.width() as f32, pixmap.height() as f32)
            }
            ImageKind::Vector(svg) => svg.natural_size(),
        }
    }

    /// The pixels of a raster image; `None` for a vector image.
    pub fn raster(&self) -> Option<&Pixmap> {
        match &self.kind {
            ImageKind::Raster(pixmap) => Some(pixmap),
            ImageKind::Vector(_) => None,
        }
    }

    pub(crate) fn kind(&self) -> &ImageKind {
        &self.kind
    }

    #[cfg(test)]
    pub(crate) fn from_pixmap(pixmap: Pixmap) -> Self {
        DecodedImage {
            kind: ImageKind::Raster(pixmap),
        }
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
    /// The SVG image is invalid or exceeds a limit.
    #[error("cannot decode SVG image: {0}")]
    Svg(String),
}

/// Images larger than this many pixels in either dimension are rejected.
pub(crate) const MAX_DIMENSION: u32 = 16_384;

/// Images with more pixels than this are rejected: 64 Mpx is 256 MiB as
/// RGBA.
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

/// The allocation limit for the decoder: enough for `MAX_PIXELS` with 16
/// bits per channel.
const MAX_DECODER_ALLOC: u64 = MAX_PIXELS * 8;

/// Decodes an image with the given MIME type essence: SVG for
/// [`SVG_MIME_TYPE`], otherwise a raster format recognized from the data
/// (see [`decode`]).
pub fn decode_with_type(data: &[u8], mime_type: Option<&str>) -> Result<DecodedImage, ImageError> {
    if mime_type == Some(SVG_MIME_TYPE) {
        Ok(DecodedImage {
            kind: ImageKind::Vector(Box::new(crate::svg::decode(data)?)),
        })
    } else {
        decode(data)
    }
}

/// Decodes PNG, JPEG, GIF (first frame), WebP, BMP and ICO data.
///
/// The size is checked from the image header before the pixels are
/// decoded, so a small file that declares a huge image costs no memory.
pub fn decode(data: &[u8]) -> Result<DecodedImage, ImageError> {
    let (_, decoder) = raster_decoder(data)?;
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
    Ok(DecodedImage {
        kind: ImageKind::Raster(pixmap),
    })
}

/// The format, width and height of raster image data whose header declares
/// an acceptable size. Nothing is decoded beyond the header.
pub(crate) fn check_raster(data: &[u8]) -> Result<(image::ImageFormat, u32, u32), ImageError> {
    let (format, decoder) = raster_decoder(data)?;
    let (width, height) = decoder.dimensions();
    Ok((format, width, height))
}

/// The format of raster image data and a decoder for it, after the size in
/// its header has been checked against the limits.
fn raster_decoder(data: &[u8]) -> Result<(image::ImageFormat, impl ImageDecoder + '_), ImageError> {
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
    Ok((format, decoder))
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
pub(crate) mod tests {
    use super::*;

    pub(crate) fn png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
        let mut out = Vec::new();
        let pixels = rgba.repeat((width * height) as usize);
        let img = image::RgbaImage::from_raw(width, height, pixels).unwrap();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn decodes_png_and_premultiplies() {
        let image = decode(&png(1, 1, [255, 0, 0, 128])).unwrap();
        assert_eq!(image.natural_size(), NaturalSize::fixed(1.0, 1.0));
        let px = image.raster().unwrap().pixel(0, 0).unwrap();
        assert_eq!(px.alpha(), 128);
        assert_eq!(px.red(), 128);
    }

    /// The header of a 24-bit BMP file without pixel data.
    pub(crate) fn bmp_header(width: i32, height: i32) -> Vec<u8> {
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
        assert!(check_raster(&bmp_header(20_000, 20_000)).is_err());
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode(b"not an image").is_err());
    }

    #[test]
    fn the_mime_type_selects_svg() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2"/>"#;
        let image = decode_with_type(svg, Some(SVG_MIME_TYPE)).unwrap();
        assert!(image.raster().is_none());
        assert_eq!(image.natural_size(), NaturalSize::fixed(4.0, 2.0));
        // Without the type, SVG is not sniffed.
        assert!(decode_with_type(svg, Some("image/png")).is_err());
        assert!(decode_with_type(svg, None).is_err());
        // A PNG with the SVG type is not an SVG image.
        let png = png(1, 1, [0, 0, 0, 255]);
        assert!(decode_with_type(&png, Some(SVG_MIME_TYPE)).is_err());
    }
}
