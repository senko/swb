//! Image decoding: raster formats into premultiplied RGBA pixmaps, SVG
//! into a vector image (see [`crate::svg`]).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use image::ImageDecoder;
use swb_layout::NaturalSize;
use thiserror::Error;
use tiny_skia::{IntSize, Pixmap};

use crate::reduce::Reductions;
use crate::svg::{CountBudget, SvgImage};

/// The MIME type of SVG images. Chromium decodes an image as SVG only if
/// its response has this type; it does not sniff SVG.
pub const SVG_MIME_TYPE: &str = "image/svg+xml";

/// The MIME types of the formats that swb decodes: the names from
/// Chromium's list of supported image types for the raster formats of
/// [`decode`], and SVG.
const SUPPORTED_TYPES: &[&str] = &[
    "image/png",
    "image/x-png",
    "image/apng",
    "image/jpeg",
    "image/jpg",
    "image/pjpeg",
    "image/gif",
    "image/webp",
    "image/bmp",
    "image/vnd.microsoft.icon",
    "image/x-icon",
    SVG_MIME_TYPE,
];

/// True if swb decodes images of the MIME type `essence` (type and
/// subtype, without parameters; ASCII case-insensitive). For the `type`
/// attribute of `<source>` elements.
pub fn is_supported_image_type(essence: &str) -> bool {
    SUPPORTED_TYPES
        .iter()
        .any(|t| t.eq_ignore_ascii_case(essence))
}

/// A decoded image.
#[derive(Debug)]
pub struct DecodedImage {
    /// A number that no other image of the process has (see
    /// [`DecodedImage::id`]).
    id: u64,
    kind: ImageKind,
    /// The reduced levels of a raster image, made when it is drawn small.
    reductions: Reductions,
}

/// The two kinds of images.
#[derive(Debug)]
pub(crate) enum ImageKind {
    /// Pixels, premultiplied RGBA. One pixel is one CSS px. Shared, so
    /// that a video frame is not copied ([`DecodedImage::from_shared_pixmap`]).
    Raster(Arc<Pixmap>),
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
            ImageKind::Raster(pixmap) => Some(pixmap.as_ref()),
            ImageKind::Vector(_) => None,
        }
    }

    /// An image that shares `pixmap` (premultiplied RGBA, one pixel per CSS
    /// px): a frame of a video, which the media pipeline's pool takes back
    /// when no image holds it any more (ADR 0047 section 9). Each call makes
    /// a new image with a new [`DecodedImage::id`].
    pub fn from_shared_pixmap(pixmap: Arc<Pixmap>) -> DecodedImage {
        DecodedImage::new(ImageKind::Raster(pixmap))
    }

    /// A number that no other decoded image of the process has: the damage
    /// of a frame (`crate::damage`) tells a newly decoded image from the
    /// one that the last frame drew.
    pub fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn kind(&self) -> &ImageKind {
        &self.kind
    }

    /// The reduced levels of a raster image.
    pub(crate) fn reductions(&self) -> &Reductions {
        &self.reductions
    }

    fn new(kind: ImageKind) -> Self {
        /// The id of the next decoded image (0 means "no image").
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        DecodedImage {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            kind,
            reductions: Reductions::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_pixmap(pixmap: Pixmap) -> Self {
        DecodedImage::new(ImageKind::Raster(Arc::new(pixmap)))
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
    decode_in_document(data, mime_type, &CountBudget::default())
}

/// Like [`decode_with_type`], for an image of a document whose SVG images
/// share `counting` (a [`CountBudget`] per document).
pub fn decode_in_document(
    data: &[u8],
    mime_type: Option<&str>,
    counting: &CountBudget,
) -> Result<DecodedImage, ImageError> {
    if mime_type == Some(SVG_MIME_TYPE) {
        Ok(DecodedImage::new(ImageKind::Vector(Box::new(
            crate::svg::decode_counting(data, counting)?,
        ))))
    } else {
        decode(data)
    }
}

/// Decodes PNG, JPEG, GIF (first frame), WebP, BMP and ICO data.
///
/// The size is checked from the image header before the pixels are
/// decoded, so a small file that declares a huge image costs no memory.
pub fn decode(data: &[u8]) -> Result<DecodedImage, ImageError> {
    let Some((patched, no_pixel_data)) = gif_with_color_table(data) else {
        return decode_raster(data);
    };
    // A frame without pixel data (Leaflet's `emptyImageUrl`) is a
    // transparent image of the size of the screen in Chromium; other data
    // that the decoder refuses stays an error (a broken image).
    decode_raster(&patched).or_else(|error| {
        if !no_pixel_data {
            return Err(error);
        }
        let (_, width, height) = check_raster(&patched)?;
        let pixmap = Pixmap::new(width.max(1), height.max(1)).ok_or(error)?;
        Ok(DecodedImage::new(ImageKind::Raster(Arc::new(pixmap))))
    })
}

/// [`decode`] for data that the decoder accepts as it is.
fn decode_raster(data: &[u8]) -> Result<DecodedImage, ImageError> {
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
    Ok(DecodedImage::new(ImageKind::Raster(Arc::new(pixmap))))
}

/// The size of the color table that [`gif_with_color_table`] adds: 256
/// entries of 3 bytes.
const GIF_BLACK_TABLE: usize = 256 * 3;

/// A copy of a GIF whose first frame has no color table, with an all-black
/// global color table, and whether that frame has no pixel data (its first
/// data sub-block is the block terminator); `None` for any other data.
/// GIF 89a section 18 and
/// 20 (<https://www.w3.org/Graphics/GIF/spec-gif89a.txt>) give a frame its
/// local table or else the global one, and the decoder of the `image`
/// crate fails when there is neither ("no color table available for
/// current frame"). Chromium draws such a frame (Leaflet's
/// `emptyImageUrl` is one): its pixels are opaque black (measured, probe
/// `gif-without-color-table-pixels`), and a frame without pixel data is
/// transparent. The copy only makes the decoder accept the data; the
/// header checks of [`raster_decoder`] see the same size.
fn gif_with_color_table(data: &[u8]) -> Option<(Vec<u8>, bool)> {
    const GLOBAL_TABLE: u8 = 0x80;
    if !(data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a")) {
        return None;
    }
    // Logical screen descriptor: width, height, packed flags, background
    // color index, pixel aspect ratio.
    let flags = *data.get(10)?;
    if flags & GLOBAL_TABLE != 0 {
        return None;
    }
    // Skip the extensions in front of the first image descriptor.
    let mut at = 13;
    loop {
        match *data.get(at)? {
            0x21 => {
                at += 2;
                loop {
                    let size = usize::from(*data.get(at)?);
                    at += 1 + size;
                    if size == 0 {
                        break;
                    }
                }
            }
            0x2C => break,
            _ => return None,
        }
    }
    // Image descriptor: separator, left, top, width, height, packed flags.
    let frame_flags = *data.get(at + 9)?;
    if frame_flags & GLOBAL_TABLE != 0 {
        return None;
    }
    // Table based image data: the LZW minimum code size, then the data
    // sub-blocks; a first block of size 0 ends the data at once.
    let no_pixel_data = data.get(at + 11) == Some(&0);
    let mut patched = Vec::with_capacity(data.len() + GIF_BLACK_TABLE);
    patched.extend_from_slice(data.get(..10)?);
    // A global table of 2^(7 + 1) entries.
    patched.push(flags | GLOBAL_TABLE | 0x07);
    patched.extend_from_slice(data.get(11..13)?);
    patched.resize(patched.len() + GIF_BLACK_TABLE, 0);
    patched.extend_from_slice(data.get(13..)?);
    Some((patched, no_pixel_data))
}

/// The width and height that the header of raster image data declares, if
/// they are acceptable; nothing is decoded. A caller can charge the
/// decoded size to a budget before it decodes.
pub fn raster_size(data: &[u8]) -> Option<(u32, u32)> {
    check_raster(data).ok().map(|(_, w, h)| (w, h))
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
        let a = u32::from(px[3]);
        if a == 255 {
            continue;
        }
        for c in &mut px[..3] {
            *c = mul_255(u32::from(*c), a) as u8;
        }
    }
}

/// `a * b / 255`, rounded to nearest, for `a` and `b` in 0 to 255.
pub(crate) fn mul_255(a: u32, b: u32) -> u32 {
    (a * b + 127) / 255
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
    fn mul_255_matches_the_16_bit_formula() {
        for a in 0..=255u16 {
            for b in 0..=255u16 {
                let expected = (a * b + 127) / 255;
                assert_eq!(mul_255(u32::from(a), u32::from(b)), u32::from(expected));
            }
        }
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
    fn supported_types() {
        for t in [
            "image/png",
            "IMAGE/PNG",
            "image/webp",
            "image/svg+xml",
            "image/x-icon",
        ] {
            assert!(is_supported_image_type(t), "{t}");
        }
        for t in [
            "image/avif",
            "image/jxl",
            "video/webm",
            "image/png; a=b",
            "",
        ] {
            assert!(!is_supported_image_type(t), "{t}");
        }
    }

    /// A GIF of `width` x `height` with `frame` as the rest of the data
    /// after the 13-byte header (no global color table).
    fn gif_without_table(width: u16, height: u16, frame: &[u8]) -> Vec<u8> {
        let mut out = b"GIF89a".to_vec();
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0]);
        out.extend_from_slice(frame);
        out
    }

    #[test]
    fn decodes_a_gif_without_color_table() {
        // Leaflet's `emptyImageUrl`: one frame, no table, no pixel data.
        let leaflet = b"\x2c\0\0\0\0\x01\0\x01\0\0\x02\0\x3b";
        let image = decode(&gif_without_table(1, 1, leaflet)).unwrap();
        assert_eq!(image.natural_size(), NaturalSize::fixed(1.0, 1.0));
        assert_eq!(image.raster().unwrap().pixel(0, 0).unwrap().alpha(), 0);
        // 2 x 1 with pixel data (indexes 0 and 1; LZW with minimum code
        // size 2: clear, 0, 1, end): Chromium draws opaque black.
        let frame = b"\x2c\0\0\0\0\x02\0\x01\0\0\x02\x02\x44\x0a\0\x3b";
        let image = decode(&gif_without_table(2, 1, frame)).unwrap();
        let pixmap = image.raster().unwrap();
        for x in 0..2 {
            let px = pixmap.pixel(x, 0).unwrap();
            assert_eq!(
                (px.red(), px.green(), px.blue(), px.alpha()),
                (0, 0, 0, 255)
            );
        }
    }

    #[test]
    fn a_broken_gif_without_color_table_stays_an_error() {
        // Pixel data that is not valid LZW (a code above the table) and a
        // file cut after the image descriptor: broken images, not
        // transparent ones.
        let corrupt = b"\x2c\0\0\0\0\x02\0\x01\0\0\x02\x02\xff\xff\0\x3b";
        assert!(decode(&gif_without_table(2, 1, corrupt)).is_err());
        let truncated = b"\x2c\0\0\0\0\x02\0\x01\0\0";
        assert!(decode(&gif_without_table(2, 1, truncated)).is_err());
    }

    #[test]
    fn a_huge_gif_without_color_table_is_refused() {
        // A 65535 x 65535 screen and a frame with no data: no allocation.
        let frame = b"\x2c\0\0\0\0\xff\xff\xff\xff\0\x02\0\x3b";
        let result = decode(&gif_without_table(65535, 65535, frame));
        assert!(matches!(result, Err(ImageError::TooLarge(65535, 65535))));
    }

    #[test]
    fn a_gif_with_a_table_is_not_changed() {
        let mut with_global = gif_without_table(1, 1, b"\x3b");
        with_global[10] = 0x80;
        assert!(gif_with_color_table(&with_global).is_none());
        let with_local = gif_without_table(1, 1, b"\x2c\0\0\0\0\x01\0\x01\0\x80");
        assert!(gif_with_color_table(&with_local).is_none());
        assert!(gif_with_color_table(b"GIF89a").is_none());
        assert!(gif_with_color_table(b"GIF89a\x01\0\x01\0\0\0\0\x21").is_none());
        assert!(gif_with_color_table(b"not a gif at all").is_none());
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
