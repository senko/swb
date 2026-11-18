//! `ImageData` (HTML,
//! <https://html.spec.whatwg.org/multipage/imagebitmap-and-animations.html#imagedata>):
//! the two constructors, `width`, `height`, `data`, `colorSpace` and
//! `pixelFormat`, and the structured clone of the object
//! ([`crate::structured`]). swb has no Canvas 2D, so nothing reads or draws
//! the pixels: pages use the interface to hold pixel data (`MapLibre`'s
//! message serializer tests `instanceof ImageData`, the `createImageBitmap`
//! fallbacks make `new ImageData(1, 1)`).
//!
//! The checks and their order are Chromium's (measured, probe case
//! `image-data` in `tools/probes/web-osm-gaps.json`): a width or height of
//! zero (also a `NaN`, and 2^32, which wraps to zero) is an
//! `IndexSizeError`, as is an image of 2^32 bytes or more ("exceeds the
//! supported range"; a negative number wraps to a huge one); an image that
//! fits that limit but not the memory is a `RangeError` ("Out of memory at
//! `ImageData` creation"). Chromium allocates up to about 1.6 GB here; swb
//! asks the script heap (`Runtime::array_buffer_fits`: buffers up to 90 %
//! of its 256 MiB limit), so a page cannot make swb allocate without
//! bound. Departures: the pixel format `rgba-float16` (a
//! `Float16Array` of data) throws `NotSupportedError`, and a first argument
//! that is an object but no `Uint8ClampedArray` is a `TypeError` where
//! Chromium takes the numeric constructor and converts the object to a
//! number.

use swb_js::{Gc, GenericData, Object, Runtime, Slots, Tracer, Value};

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;
use crate::generated::types::{ImageDataPixelFormat, ImageDataSettings, PredefinedColorSpace};
use crate::state::WebState;

/// The most bytes of one `ImageData`: Chromium counts the bytes in a
/// 32-bit unsigned integer.
const MAX_BYTES: u128 = 1 << 32;

/// The data of an `ImageData` object.
struct ImageDataState {
    width: u32,
    height: u32,
    color_space: PredefinedColorSpace,
    /// The `Uint8ClampedArray`.
    data: Gc<Object>,
}

impl GenericData for ImageDataState {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.object(self.data);
    }

    fn heap_size(&self) -> usize {
        size_of::<ImageDataState>()
    }
}

/// The error of a size that is zero.
fn zero(what: &str) -> Exception {
    Exception::dom(
        "IndexSizeError",
        format!("The source {what} is zero or not a number."),
    )
}

/// The `Uint8ClampedArray` of `bytes` zeros, or the `RangeError` of
/// Chromium when they do not fit.
fn zeroed_array(cx: &mut Cx<'_>, bytes: usize) -> b::Result<Gc<Object>> {
    let out_of_memory = || Exception::range_error("Out of memory at ImageData creation");
    if !cx.rt.array_buffer_fits(bytes) {
        return Err(out_of_memory());
    }
    let mut zeros = Vec::new();
    zeros
        .try_reserve_exact(bytes)
        .map_err(|_| out_of_memory())?;
    zeros.resize(bytes, 0u8);
    array_of(cx.rt, cx.realm(), zeros)
}

/// A `Uint8ClampedArray` of `realm` over `bytes`.
fn array_of(rt: &mut Runtime, realm: u32, bytes: Vec<u8>) -> b::Result<Gc<Object>> {
    let length = bytes.len();
    let buffer = rt.create_array_buffer(realm, bytes, None)?;
    Ok(rt.create_view(realm, Some("Uint8Clamped"), buffer, 0, Some(length))?)
}

/// An `ImageData` object of `realm`.
fn new_object(rt: &mut Runtime, realm: u32, state: ImageDataState) -> b::Result<Gc<Object>> {
    let data = rt.alloc_host_data(state)?;
    Ok(rt.new_host_object(realm, g::IMAGE_DATA, data)?)
}

/// The settings' color space (default `srgb`); `rgba-float16` is not
/// supported.
fn color_space_of(settings: &ImageDataSettings) -> b::Result<PredefinedColorSpace> {
    if settings.pixel_format == ImageDataPixelFormat::RgbaFloat16 {
        return Err(Exception::dom(
            "NotSupportedError",
            "The pixel format 'rgba-float16' is not supported.",
        ));
    }
    Ok(settings.color_space.unwrap_or(PredefinedColorSpace::Srgb))
}

/// The length of the `Uint8ClampedArray` `array` from its internal slots,
/// not from the `length` property, which a script can redefine: 0 when its
/// buffer is detached or the view is out of bounds.
fn length_of(rt: &mut Runtime, array: Gc<Object>) -> b::Result<u64> {
    let Slots::View {
        buffer,
        byte_offset,
        byte_length,
        ..
    } = rt.slots_of(array)?
    else {
        return Ok(0);
    };
    let length = match byte_length {
        Some(length) => length,
        // A view that tracks the length of a resizable buffer.
        None => match rt.slots_of(buffer)? {
            Slots::ArrayBuffer {
                byte_length,
                detached: false,
                ..
            } => byte_length.saturating_sub(byte_offset),
            _ => 0,
        },
    };
    Ok(u64::try_from(length).unwrap_or(u64::MAX))
}

/// The state of `this`.
fn state_of(rt: &Runtime, this: Platform) -> b::Result<&ImageDataState> {
    b::glue::generic_data::<ImageDataState>(rt, this)
}

impl g::image_data::ImageDataImpl for WebState {
    fn width(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        Ok(state_of(cx.rt, this)?.width)
    }

    fn height(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        Ok(state_of(cx.rt, this)?.height)
    }

    fn data(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        Ok(state_of(cx.rt, this)?.data)
    }

    fn color_space(cx: &mut Cx<'_>, this: Platform) -> b::Result<PredefinedColorSpace> {
        Ok(state_of(cx.rt, this)?.color_space)
    }

    fn pixel_format(_cx: &mut Cx<'_>, this: Platform) -> b::Result<ImageDataPixelFormat> {
        let _ = this;
        Ok(ImageDataPixelFormat::RgbaUnorm8)
    }

    /// `new ImageData(sw, sh, settings)`: the pixels are transparent black.
    /// <https://html.spec.whatwg.org/multipage/imagebitmap-and-animations.html#dom-imagedata>
    fn constructor_1(
        cx: &mut Cx<'_>,
        sw: u32,
        sh: u32,
        settings: ImageDataSettings,
    ) -> b::Result<Gc<Object>> {
        if sw == 0 {
            return Err(zero("width"));
        }
        if sh == 0 {
            return Err(zero("height"));
        }
        let color_space = color_space_of(&settings)?;
        let bytes = u128::from(sw) * u128::from(sh) * 4;
        if bytes >= MAX_BYTES {
            return Err(Exception::dom(
                "IndexSizeError",
                "The requested image size exceeds the supported range.",
            ));
        }
        let data = zeroed_array(cx, bytes as usize)?;
        let state = ImageDataState {
            width: sw,
            height: sh,
            color_space,
            data,
        };
        let data = cx.rt.alloc_host_data(state)?;
        Ok(cx.new_instance::<WebState>(data)?)
    }

    /// `new ImageData(data, sw, sh, settings)`: the object uses `data`
    /// itself. The height is the one that the length of `data` gives when
    /// it is missing.
    fn constructor_2(
        cx: &mut Cx<'_>,
        data: Gc<Object>,
        sw: u32,
        sh: Option<u32>,
        settings: ImageDataSettings,
    ) -> b::Result<Gc<Object>> {
        let invalid = |message: &'static str| Exception::dom("InvalidStateError", message);
        let size = |message: &'static str| Exception::dom("IndexSizeError", message);
        let color_space = color_space_of(&settings)?;
        let length = length_of(cx.rt, data)?;
        if length == 0 {
            return Err(invalid("The input data has zero elements."));
        }
        if length % 4 != 0 {
            return Err(invalid("The input data length is not a multiple of 4."));
        }
        if sw == 0 {
            return Err(zero("width"));
        }
        let pixels = length / 4;
        let height = match sh {
            Some(0) => return Err(zero("height")),
            Some(sh) => {
                if u128::from(sw) * u128::from(sh) != u128::from(pixels) {
                    return Err(size(
                        "The input data length is not equal to (4 * width * height).",
                    ));
                }
                sh
            }
            None => {
                if pixels % u64::from(sw) != 0 {
                    return Err(size(
                        "The input data length is not a multiple of (4 * width).",
                    ));
                }
                u32::try_from(pixels / u64::from(sw))
                    .map_err(|_| size("The requested image size exceeds the supported range."))?
            }
        };
        let state = ImageDataState {
            width: sw,
            height,
            color_space,
            data,
        };
        let data = cx.rt.alloc_host_data(state)?;
        Ok(cx.new_instance::<WebState>(data)?)
    }
}

/// What structured clone copies of an `ImageData`: its size, color space
/// and a copy of the pixel bytes.
pub(crate) struct Pixels {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) display_p3: bool,
    pub(crate) bytes: Vec<u8>,
}

/// The pixels of `object` if it is an `ImageData`; `Ok(None)` for another
/// object, and an empty `bytes` if the buffer of its data is detached.
pub(crate) fn pixels_of(rt: &mut Runtime, object: Gc<Object>) -> b::Result<Option<Pixels>> {
    if !rt.implements(Value::Object(object), g::IMAGE_DATA) {
        return Ok(None);
    }
    let Some((_, swb_js::HostData::Generic(handle))) = rt.host_object(object.into()) else {
        return Ok(None);
    };
    let Ok(state) = rt.heap().generic::<ImageDataState>(handle) else {
        return Ok(None);
    };
    let (width, height, display_p3, data) = (
        state.width,
        state.height,
        state.color_space == PredefinedColorSpace::DisplayP3,
        state.data,
    );
    let bytes = match rt.slots_of(data)? {
        Slots::View {
            buffer,
            byte_offset,
            ..
        } => {
            let length = (width as usize) * (height as usize) * 4;
            rt.buffer_range(buffer, byte_offset, length)?
        }
        _ => Vec::new(),
    };
    Ok(Some(Pixels {
        width,
        height,
        display_p3,
        bytes,
    }))
}

/// A new `ImageData` of `realm` that owns `bytes` (the result of a
/// structured clone).
pub(crate) fn create(
    rt: &mut Runtime,
    realm: u32,
    width: u32,
    height: u32,
    display_p3: bool,
    bytes: Vec<u8>,
) -> b::Result<Gc<Object>> {
    let data = array_of(rt, realm, bytes)?;
    let color_space = if display_p3 {
        PredefinedColorSpace::DisplayP3
    } else {
        PredefinedColorSpace::Srgb
    };
    new_object(
        rt,
        realm,
        ImageDataState {
            width,
            height,
            color_space,
            data,
        },
    )
}
