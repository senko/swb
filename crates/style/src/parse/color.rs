//! Color parsing: CSS Color 4 syntax and the HTML legacy color algorithm.
//!
//! <https://www.w3.org/TR/css-color-4/> and
//! <https://www.w3.org/TR/css-color-5/#color-mix>.
//!
//! All colors are converted to sRGB at parse time and clipped to the sRGB
//! gamut. The conversion matrices are the ones published in CSS Color 4,
//! section 18 ("Sample code for color conversions").
//!
//! Limitations: relative color syntax (`rgb(from ...)`) is not supported.
//! `color()` supports `srgb`, `srgb-linear`, `display-p3`, `xyz`,
//! `xyz-d65` and `xyz-d50`. `color-mix()` supports only colors without
//! `currentColor`; mixing in a polar space (`hsl`, `hwb`, `lch`, `oklch`)
//! is done in the matching rectangular space (`srgb`, `lab`, `oklab`),
//! which is an approximation. `light-dark()` always picks the light color.

#![allow(clippy::many_single_char_names)] // Color math uses one-letter channel names.

use swb_css::{ComponentValue, ParseError, Parser};

use super::{ParseResult, parse_hue, parse_number};
use crate::values::{Color, Rgba, named_color, system_color};

/// Consumes a `<color>`.
pub(crate) fn parse_color(p: &mut Parser<'_>) -> ParseResult<Color> {
    p.try_parse(|p| {
        let value = p.next().ok_or(ParseError::EndOfInput)?;
        match value {
            ComponentValue::Hash { value, .. } => {
                parse_hex(value).map(Color::Rgba).ok_or(ParseError::Invalid)
            }
            ComponentValue::Ident(name) => color_keyword(name).ok_or(ParseError::Unexpected),
            ComponentValue::Function(f) => {
                let args = Parser::new(&f.arguments);
                parse_color_function(&f.name, args)
            }
            _ => Err(ParseError::Unexpected),
        }
    })
}

/// A color keyword: named colors, `transparent`, `currentColor`, system
/// colors (including the deprecated ones) and a few `-webkit-` keywords.
fn color_keyword(name: &str) -> Option<Color> {
    if name.eq_ignore_ascii_case("currentcolor") {
        return Some(Color::CurrentColor);
    }
    if let Some(c) = named_color(name).or_else(|| system_color(name)) {
        return Some(Color::Rgba(c));
    }
    let lower = name.to_ascii_lowercase();
    // Deprecated system colors map to the standard ones.
    // <https://www.w3.org/TR/css-color-4/#deprecated-system-colors>
    let mapped = match lower.as_str() {
        "activeborder" | "inactiveborder" | "threeddarkshadow" | "threedhighlight"
        | "threedlightshadow" | "threedshadow" | "windowframe" => "buttonborder",
        "activecaption" | "appworkspace" | "background" | "inactivecaption" | "infobackground"
        | "menu" | "scrollbar" | "window" => "canvas",
        "buttonshadow" | "threedface" => "buttonface",
        // Chromium's `-webkit-text` is the document's default text color.
        "captiontext" | "infotext" | "menutext" | "windowtext" | "-webkit-text" => "canvastext",
        "inactivecaptiontext" => "graytext",
        "-webkit-link" => "linktext",
        "-webkit-activelink" => "activetext",
        "-webkit-focus-ring-color" => return Some(Color::Rgba(Rgba::rgb(0x10, 0x10, 0x10))),
        _ => return None,
    };
    system_color(mapped).map(Color::Rgba)
}

/// Parses the digits of a hex color (without `#`).
fn parse_hex(digits: &str) -> Option<Rgba> {
    let bytes = digits.as_bytes();
    if !bytes.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    let nibble = |i: usize| {
        bytes
            .get(i)
            .and_then(|b| (*b as char).to_digit(16))
            .map(|d| d as u8)
    };
    let short = |i: usize| nibble(i).map(|d| d * 17);
    let long = |i: usize| Some(nibble(i)? * 16 + nibble(i + 1)?);
    match bytes.len() {
        3 => Some(Rgba::rgb(short(0)?, short(1)?, short(2)?)),
        4 => Some(Rgba::new(short(0)?, short(1)?, short(2)?, short(3)?)),
        6 => Some(Rgba::rgb(long(0)?, long(2)?, long(4)?)),
        8 => Some(Rgba::new(long(0)?, long(2)?, long(4)?, long(6)?)),
        _ => None,
    }
}

fn parse_color_function(name: &str, mut args: Parser<'_>) -> ParseResult<Color> {
    let lower = name.to_ascii_lowercase();
    let rgba = match lower.as_str() {
        "rgb" | "rgba" => parse_rgb(&mut args)?,
        "hsl" | "hsla" => parse_hsl(&mut args)?,
        "hwb" => args.parse_entirely(parse_hwb)?,
        "lab" | "lch" | "oklab" | "oklch" => args.parse_entirely(|p| parse_lab_like(p, &lower))?,
        "color" => args.parse_entirely(parse_color_space_function)?,
        "color-mix" => args.parse_entirely(parse_color_mix)?,
        "light-dark" => {
            let colors = args.parse_comma_separated(parse_color)?;
            return match colors.as_slice() {
                [light, _dark] => Ok(*light),
                _ => Err(ParseError::Invalid),
            };
        }
        _ => return Err(ParseError::Unexpected),
    };
    Ok(Color::Rgba(rgba))
}

/// One channel of a modern color function: a number, a percentage (as a
/// fraction of `percent_scale`) or `none` (zero).
fn modern_channel(p: &mut Parser<'_>, percent_scale: f32) -> ParseResult<f32> {
    if p.expect_ident_matching("none").is_ok() {
        return Ok(0.0);
    }
    if let Ok(v) = p.expect_percentage() {
        return Ok(v / 100.0 * percent_scale);
    }
    parse_number(p)
}

/// A hue channel of a modern color function (or `none`).
fn modern_hue(p: &mut Parser<'_>) -> ParseResult<f32> {
    if p.expect_ident_matching("none").is_ok() {
        return Ok(0.0);
    }
    parse_hue(p)
}

/// `<alpha-value>`: a number (0..1) or a percentage, clamped.
fn alpha_value(p: &mut Parser<'_>) -> ParseResult<f32> {
    let a = if let Ok(v) = p.expect_percentage() {
        v / 100.0
    } else {
        parse_number(p)?
    };
    Ok(a.clamp(0.0, 1.0))
}

/// The optional `/ <alpha-value>` at the end of a modern color function.
fn modern_alpha(p: &mut Parser<'_>) -> ParseResult<f32> {
    if p.expect_delim('/').is_err() {
        return Ok(1.0);
    }
    if p.expect_ident_matching("none").is_ok() {
        return Ok(0.0);
    }
    alpha_value(p)
}

/// `rgb()` and `rgba()`: the legacy comma syntax or the modern syntax.
/// <https://www.w3.org/TR/css-color-4/#rgb-functions>
fn parse_rgb(args: &mut Parser<'_>) -> ParseResult<Rgba> {
    if let Ok(c) = args.parse_entirely(parse_rgb_legacy) {
        return Ok(c);
    }
    args.parse_entirely(|p| {
        let r = modern_channel(p, 255.0)?;
        let g = modern_channel(p, 255.0)?;
        let b = modern_channel(p, 255.0)?;
        let a = modern_alpha(p)?;
        Ok(Rgba::from_f32(r, g, b, a))
    })
}

fn parse_rgb_legacy(p: &mut Parser<'_>) -> ParseResult<Rgba> {
    let percent = p
        .peek()
        .is_some_and(|v| matches!(v, ComponentValue::Percentage(_)));
    let channel = |p: &mut Parser<'_>| -> ParseResult<f32> {
        if percent {
            Ok(p.expect_percentage()? * 2.55)
        } else {
            parse_number(p)
        }
    };
    let r = channel(p)?;
    p.expect_comma()?;
    let g = channel(p)?;
    p.expect_comma()?;
    let b = channel(p)?;
    let a = if p.expect_comma().is_ok() {
        alpha_value(p)?
    } else {
        1.0
    };
    Ok(Rgba::from_f32(r, g, b, a))
}

/// `hsl()` and `hsla()`.
/// <https://www.w3.org/TR/css-color-4/#the-hsl-notation>
fn parse_hsl(args: &mut Parser<'_>) -> ParseResult<Rgba> {
    let legacy: ParseResult<(f32, f32, f32, f32)> = args.parse_entirely(|p| {
        let h = parse_hue(p)?;
        p.expect_comma()?;
        let s = p.expect_percentage()?;
        p.expect_comma()?;
        let l = p.expect_percentage()?;
        let a = if p.expect_comma().is_ok() {
            alpha_value(p)?
        } else {
            1.0
        };
        Ok((h, s, l, a))
    });
    let (h, s, l, a) = match legacy {
        Ok(v) => v,
        Err(_) => args.parse_entirely(|p| {
            let h = modern_hue(p)?;
            let s = modern_channel(p, 100.0)?;
            let l = modern_channel(p, 100.0)?;
            let a = modern_alpha(p)?;
            Ok((h, s, l, a))
        })?,
    };
    let [r, g, b] = hsl_to_srgb(h, s / 100.0, l / 100.0);
    Ok(Rgba::from_f32(r * 255.0, g * 255.0, b * 255.0, a))
}

/// `hwb()`. <https://www.w3.org/TR/css-color-4/#the-hwb-notation>
fn parse_hwb(p: &mut Parser<'_>) -> ParseResult<Rgba> {
    let h = modern_hue(p)?;
    let w = modern_channel(p, 100.0)? / 100.0;
    let bl = modern_channel(p, 100.0)? / 100.0;
    let a = modern_alpha(p)?;
    let [r, g, b] = hwb_to_srgb(h, w, bl);
    Ok(Rgba::from_f32(r * 255.0, g * 255.0, b * 255.0, a))
}

/// `lab()`, `lch()`, `oklab()` and `oklch()`.
/// <https://www.w3.org/TR/css-color-4/#specifying-lab-lch>
fn parse_lab_like(p: &mut Parser<'_>, name: &str) -> ParseResult<Rgba> {
    let (l_scale, ab_scale) = match name {
        "lab" => (100.0, 125.0),
        "lch" => (100.0, 150.0),
        "oklab" | "oklch" => (1.0, 0.4),
        _ => return Err(ParseError::Unexpected),
    };
    let l = modern_channel(p, l_scale)?;
    let polar = name.ends_with("ch");
    let (a, b) = if polar {
        let c = modern_channel(p, ab_scale)?.max(0.0);
        let h = modern_hue(p)?.to_radians();
        (c * h.cos(), c * h.sin())
    } else {
        (modern_channel(p, ab_scale)?, modern_channel(p, ab_scale)?)
    };
    let alpha = modern_alpha(p)?;
    let linear = if name.starts_with("ok") {
        oklab_to_linear_srgb([l, a, b])
    } else {
        xyz_d65_to_linear_srgb(d50_to_d65(lab_to_xyz_d50([l.max(0.0), a, b])))
    };
    Ok(linear_srgb_to_rgba(linear, alpha))
}

/// `color(<colorspace> c1 c2 c3 [/ alpha])`.
/// <https://www.w3.org/TR/css-color-4/#color-function>
fn parse_color_space_function(p: &mut Parser<'_>) -> ParseResult<Rgba> {
    let space = p.expect_ident()?.to_ascii_lowercase();
    let mut c = [0.0; 3];
    for v in &mut c {
        *v = modern_channel(p, 1.0)?;
    }
    let alpha = modern_alpha(p)?;
    let linear = match space.as_str() {
        "srgb" => c.map(srgb_to_linear),
        "srgb-linear" => c,
        "display-p3" => xyz_d65_to_linear_srgb(mul(&P3_TO_XYZ_D65, c.map(srgb_to_linear))),
        "xyz" | "xyz-d65" => xyz_d65_to_linear_srgb(c),
        "xyz-d50" => xyz_d65_to_linear_srgb(d50_to_d65(c)),
        _ => return Err(ParseError::Invalid),
    };
    Ok(linear_srgb_to_rgba(linear, alpha))
}

/// The color space of `color-mix()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MixSpace {
    Srgb,
    SrgbLinear,
    Lab,
    Oklab,
    Xyz,
}

/// `color-mix([in <space>,]? <color> <percentage>?, <color> <percentage>?)`.
/// <https://www.w3.org/TR/css-color-5/#color-mix>
fn parse_color_mix(p: &mut Parser<'_>) -> ParseResult<Rgba> {
    let space = if p.expect_ident_matching("in").is_ok() {
        let name = p.expect_ident()?.to_ascii_lowercase();
        let space = match name.as_str() {
            "srgb" | "hsl" | "hwb" => MixSpace::Srgb,
            "srgb-linear" => MixSpace::SrgbLinear,
            "lab" | "lch" => MixSpace::Lab,
            "oklab" | "oklch" => MixSpace::Oklab,
            "xyz" | "xyz-d65" | "xyz-d50" => MixSpace::Xyz,
            _ => return Err(ParseError::Invalid),
        };
        // An optional hue interpolation method for polar spaces.
        let _ = p.try_parse(|p| {
            p.expect_one_of(&[
                ("shorter", ()),
                ("longer", ()),
                ("increasing", ()),
                ("decreasing", ()),
            ])?;
            p.expect_ident_matching("hue")
        });
        p.expect_comma()?;
        space
    } else {
        MixSpace::Oklab
    };
    let item = |p: &mut Parser<'_>| -> ParseResult<(Rgba, Option<f32>)> {
        let mut pct = p.expect_percentage().ok();
        let color = parse_color(p)?;
        if pct.is_none() {
            pct = p.expect_percentage().ok();
        }
        if pct.is_some_and(|v| !(0.0..=100.0).contains(&v)) {
            return Err(ParseError::Invalid);
        }
        match color {
            Color::Rgba(c) => Ok((c, pct)),
            Color::CurrentColor => Err(ParseError::Invalid),
        }
    };
    let (c1, p1) = item(p)?;
    p.expect_comma()?;
    let (c2, p2) = item(p)?;
    let (p1, p2) = match (p1, p2) {
        (None, None) => (50.0, 50.0),
        (Some(a), None) => (a, 100.0 - a),
        (None, Some(b)) => (100.0 - b, b),
        (Some(a), Some(b)) => (a, b),
    };
    let sum = p1 + p2;
    if sum <= 0.0 {
        return Err(ParseError::Invalid);
    }
    let alpha_multiplier = (sum / 100.0).min(1.0);
    Ok(mix(space, c1, c2, p2 / sum, alpha_multiplier))
}

/// Interpolates from `a` to `b` by `t` (0..1) with premultiplied alpha.
fn mix(space: MixSpace, a: Rgba, b: Rgba, t: f32, alpha_multiplier: f32) -> Rgba {
    let to_space = |c: Rgba| -> [f32; 3] {
        let srgb = [c.r, c.g, c.b].map(|v| f32::from(v) / 255.0);
        let linear = srgb.map(srgb_to_linear);
        match space {
            MixSpace::Srgb => srgb,
            MixSpace::SrgbLinear => linear,
            MixSpace::Lab => xyz_d50_to_lab(d65_to_d50(mul(&LINEAR_SRGB_TO_XYZ_D65, linear))),
            MixSpace::Oklab => linear_srgb_to_oklab(linear),
            MixSpace::Xyz => mul(&LINEAR_SRGB_TO_XYZ_D65, linear),
        }
    };
    let from_space = |v: [f32; 3]| -> [f32; 3] {
        match space {
            MixSpace::Srgb => v.map(srgb_to_linear),
            MixSpace::SrgbLinear => v,
            MixSpace::Lab => xyz_d65_to_linear_srgb(d50_to_d65(lab_to_xyz_d50(v))),
            MixSpace::Oklab => oklab_to_linear_srgb(v),
            MixSpace::Xyz => xyz_d65_to_linear_srgb(v),
        }
    };
    let (aa, ab) = (a.alpha_f32(), b.alpha_f32());
    let alpha = aa * (1.0 - t) + ab * t;
    let (va, vb) = (to_space(a), to_space(b));
    let mut out = [0.0; 3];
    for i in 0..3 {
        let premultiplied = va[i] * aa * (1.0 - t) + vb[i] * ab * t;
        out[i] = if alpha > 0.0 {
            premultiplied / alpha
        } else {
            0.0
        };
    }
    linear_srgb_to_rgba(from_space(out), alpha * alpha_multiplier)
}

// ----- Color space conversions (CSS Color 4, section 18) -----

type Matrix = [[f32; 3]; 3];

fn mul(m: &Matrix, v: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2])
}

const LINEAR_SRGB_TO_XYZ_D65: Matrix = [
    [0.412_390_8, 0.357_584_33, 0.180_480_8],
    [0.212_639, 0.715_168_7, 0.072_192_32],
    [0.019_330_818, 0.119_194_78, 0.950_532_14],
];

const XYZ_D65_TO_LINEAR_SRGB: Matrix = [
    [3.240_97, -1.537_383_2, -0.498_610_76],
    [-0.969_243_65, 1.875_967_5, 0.041_555_06],
    [0.055_630_08, -0.203_976_96, 1.056_971_5],
];

const D50_TO_D65: Matrix = [
    [0.955_473_4, -0.023_098_454, 0.063_259_24],
    [-0.028_369_706, 1.009_995_4, 0.021_041_442],
    [0.012_314_015, -0.020_507_65, 1.330_365_9],
];

const D65_TO_D50: Matrix = [
    [1.047_929_8, 0.022_946_87, -0.050_192_264],
    [0.029_627_81, 0.990_434_4, -0.017_073_8],
    [-0.009_243_041, 0.015_055_191, 0.751_874_3],
];

const P3_TO_XYZ_D65: Matrix = [
    [0.486_570_95, 0.265_667_7, 0.198_217_29],
    [0.228_974_56, 0.691_738_5, 0.079_286_91],
    [0.0, 0.045_113_38, 1.043_944_4],
];

/// The D50 white point.
const D50_WHITE: [f32; 3] = [0.3457 / 0.3585, 1.0, (1.0 - 0.3457 - 0.3585) / 0.3585];

fn d50_to_d65(v: [f32; 3]) -> [f32; 3] {
    mul(&D50_TO_D65, v)
}

fn d65_to_d50(v: [f32; 3]) -> [f32; 3] {
    mul(&D65_TO_D50, v)
}

fn xyz_d65_to_linear_srgb(v: [f32; 3]) -> [f32; 3] {
    mul(&XYZ_D65_TO_LINEAR_SRGB, v)
}

fn srgb_to_linear(c: f32) -> f32 {
    let abs = c.abs();
    if abs <= 0.040_45 {
        c / 12.92
    } else {
        c.signum() * ((abs + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    let abs = c.abs();
    if abs > 0.003_130_8 {
        c.signum() * (1.055 * abs.powf(1.0 / 2.4) - 0.055)
    } else {
        12.92 * c
    }
}

fn linear_srgb_to_rgba(linear: [f32; 3], alpha: f32) -> Rgba {
    let [r, g, b] = linear.map(|c| linear_to_srgb(c) * 255.0);
    let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
    Rgba::from_f32(finite(r), finite(g), finite(b), alpha)
}

const LAB_KAPPA: f32 = 24389.0 / 27.0;
const LAB_EPSILON: f32 = 216.0 / 24389.0;

fn lab_to_xyz_d50([l, a, b]: [f32; 3]) -> [f32; 3] {
    let f1 = (l + 16.0) / 116.0;
    let f0 = a / 500.0 + f1;
    let f2 = f1 - b / 200.0;
    let x = if f0.powi(3) > LAB_EPSILON {
        f0.powi(3)
    } else {
        (116.0 * f0 - 16.0) / LAB_KAPPA
    };
    let y = if l > LAB_KAPPA * LAB_EPSILON {
        f1.powi(3)
    } else {
        l / LAB_KAPPA
    };
    let z = if f2.powi(3) > LAB_EPSILON {
        f2.powi(3)
    } else {
        (116.0 * f2 - 16.0) / LAB_KAPPA
    };
    [x * D50_WHITE[0], y * D50_WHITE[1], z * D50_WHITE[2]]
}

fn xyz_d50_to_lab(xyz: [f32; 3]) -> [f32; 3] {
    let f = |i: usize| {
        let v = xyz[i] / D50_WHITE[i];
        if v > LAB_EPSILON {
            v.cbrt()
        } else {
            (LAB_KAPPA * v + 16.0) / 116.0
        }
    };
    let (f0, f1, f2) = (f(0), f(1), f(2));
    [116.0 * f1 - 16.0, 500.0 * (f0 - f1), 200.0 * (f1 - f2)]
}

/// <https://bottosson.github.io/posts/oklab/>
fn oklab_to_linear_srgb([l, a, b]: [f32; 3]) -> [f32; 3] {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l3, m3, s3) = (l_.powi(3), m_.powi(3), s_.powi(3));
    [
        4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_94 * s3,
        -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_38 * s3,
        -0.004_196_086_3 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3,
    ]
}

fn linear_srgb_to_oklab([r, g, b]: [f32; 3]) -> [f32; 3] {
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// HSL to sRGB (0..1), saturation and lightness as fractions.
/// <https://www.w3.org/TR/css-color-4/#hsl-to-rgb>
fn hsl_to_srgb(hue: f32, sat: f32, light: f32) -> [f32; 3] {
    let hue = hue.rem_euclid(360.0);
    let (sat, light) = (sat.clamp(0.0, 1.0), light.clamp(0.0, 1.0));
    let f = |n: f32| {
        let k = (n + hue / 30.0) % 12.0;
        let a = sat * light.min(1.0 - light);
        light - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    [f(0.0), f(8.0), f(4.0)]
}

/// HWB to sRGB (0..1). <https://www.w3.org/TR/css-color-4/#hwb-to-rgb>
fn hwb_to_srgb(hue: f32, white: f32, black: f32) -> [f32; 3] {
    let (white, black) = (white.clamp(0.0, 1.0), black.clamp(0.0, 1.0));
    if white + black >= 1.0 {
        let gray = white / (white + black);
        return [gray; 3];
    }
    hsl_to_srgb(hue, 1.0, 0.5).map(|c| c * (1.0 - white - black) + white)
}

// ----- HTML legacy colors -----

/// The rules for parsing a legacy color value (for `bgcolor`, `color`,
/// `text` and similar attributes).
/// <https://html.spec.whatwg.org/multipage/common-microsyntaxes.html#rules-for-parsing-a-legacy-colour-value>
pub(crate) fn parse_legacy_color(input: &str) -> Option<Rgba> {
    let input = input.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\x0C' | '\r'));
    if input.is_empty() || input.eq_ignore_ascii_case("transparent") {
        return None;
    }
    if let Some(c) = named_color(input) {
        return Some(c);
    }
    let chars: Vec<char> = input.chars().collect();
    if chars.len() == 4
        && chars[0] == '#'
        && chars[1..].iter().all(char::is_ascii_hexdigit)
        && let Some(c) = parse_hex(&input[1..])
    {
        return Some(c);
    }
    // Replace code points above U+FFFF with "00", truncate to 128 code
    // points, drop a leading '#', replace non-hex digits with '0'.
    let mut s: Vec<u8> = Vec::with_capacity(chars.len());
    for c in chars {
        if u32::from(c) > 0xFFFF {
            s.extend_from_slice(b"00");
        } else if c.is_ascii_hexdigit() {
            s.push(c as u8);
        } else {
            s.push(b'0');
        }
    }
    s.truncate(128);
    if input.starts_with('#') && !s.is_empty() {
        s.remove(0);
    }
    while s.is_empty() || !s.len().is_multiple_of(3) {
        s.push(b'0');
    }
    let len = s.len() / 3;
    let mut parts: Vec<&[u8]> = s.chunks(len).collect();
    if len > 8 {
        for part in &mut parts {
            *part = &part[len - 8..];
        }
    }
    while parts[0].len() > 2 && parts.iter().all(|p| p[0] == b'0') {
        for part in &mut parts {
            *part = &part[1..];
        }
    }
    let channel = |part: &[u8]| -> Option<u8> {
        let text = std::str::from_utf8(&part[..part.len().min(2)]).ok()?;
        u8::from_str_radix(text, 16).ok()
    };
    Some(Rgba::rgb(
        channel(parts[0])?,
        channel(parts[1])?,
        channel(parts[2])?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::test_util::parse_all;

    fn c(css: &str) -> Option<Color> {
        parse_all(css, parse_color).ok()
    }

    #[allow(clippy::unnecessary_wraps)] // Matches the result type of `c()`.
    fn rgba(r: u8, g: u8, b: u8, a: u8) -> Option<Color> {
        Some(Color::Rgba(Rgba::new(r, g, b, a)))
    }

    #[test]
    fn hex_and_keywords() {
        let cases: &[(&str, Option<Color>)] = &[
            ("#f00", rgba(255, 0, 0, 255)),
            ("#F008", rgba(255, 0, 0, 0x88)),
            ("#ff6600", rgba(255, 0x66, 0, 255)),
            ("#11223344", rgba(0x11, 0x22, 0x33, 0x44)),
            ("#12345", None),
            ("#ggg", None),
            ("red", rgba(255, 0, 0, 255)),
            ("RebeccaPurple", rgba(0x66, 0x33, 0x99, 255)),
            ("transparent", rgba(0, 0, 0, 0)),
            ("currentColor", Some(Color::CurrentColor)),
            ("LinkText", rgba(0, 0, 0xEE, 255)),
            ("ThreeDFace", rgba(0xEF, 0xEF, 0xEF, 255)),
            ("WindowText", rgba(0, 0, 0, 255)),
            ("-webkit-link", rgba(0, 0, 0xEE, 255)),
            ("notacolor", None),
            ("10px", None),
        ];
        for (css, expected) in cases {
            assert_eq!(c(css), *expected, "{css}");
        }
    }

    #[test]
    fn rgb_and_hsl() {
        let cases: &[(&str, Option<Color>)] = &[
            ("rgb(255, 0, 0)", rgba(255, 0, 0, 255)),
            ("rgba(255, 0, 0, 0.5)", rgba(255, 0, 0, 128)),
            ("rgb(100%, 50%, 0%)", rgba(255, 128, 0, 255)),
            ("rgb(255 0 0 / 50%)", rgba(255, 0, 0, 128)),
            ("rgb(255 none 0)", rgba(255, 0, 0, 255)),
            ("rgb(100% 0 0)", rgba(255, 0, 0, 255)),
            ("rgba(0 0 0 / 0.1)", rgba(0, 0, 0, 26)),
            ("rgb(300, -10, 0)", rgba(255, 0, 0, 255)),
            ("rgb(1.5, 2, 3)", rgba(2, 2, 3, 255)),
            ("rgb(255, 0%, 0)", None),
            ("rgb(255, 0 0)", None),
            ("rgb(255 0)", None),
            ("rgb(none, 0, 0)", None),
            ("hsl(0, 100%, 50%)", rgba(255, 0, 0, 255)),
            ("hsl(120deg 100% 25%)", rgba(0, 128, 0, 255)),
            ("hsla(240, 100%, 50%, 0.5)", rgba(0, 0, 255, 128)),
            ("hsl(0.5turn 100 50 / 1)", rgba(0, 255, 255, 255)),
            ("hsl(0, 100, 50)", None),
            ("hwb(0 0% 0%)", rgba(255, 0, 0, 255)),
            ("hwb(0 50% 50%)", rgba(128, 128, 128, 255)),
            ("light-dark(white, black)", rgba(255, 255, 255, 255)),
        ];
        for (css, expected) in cases {
            assert_eq!(c(css), *expected, "{css}");
        }
    }

    fn close(a: Option<Color>, b: Rgba) -> bool {
        let Some(Color::Rgba(a)) = a else {
            return false;
        };
        let d = |x: u8, y: u8| (i16::from(x) - i16::from(y)).abs() <= 1;
        d(a.r, b.r) && d(a.g, b.g) && d(a.b, b.b) && d(a.a, b.a)
    }

    #[test]
    fn lab_and_friends() {
        // sRGB red in CSS Lab (D50) and LCH, from CSS Color 4.
        assert!(close(c("lab(54.29 80.82 69.91)"), Rgba::rgb(255, 0, 0)));
        assert!(close(c("lch(54.29 106.84 40.85)"), Rgba::rgb(255, 0, 0)));
        assert!(close(c("oklab(0.628 0.225 0.126)"), Rgba::rgb(255, 0, 0)));
        assert!(close(c("oklch(62.8% 0.2577 29.23)"), Rgba::rgb(255, 0, 0)));
        assert!(close(c("oklch(1 0 0)"), Rgba::rgb(255, 255, 255)));
        assert!(close(c("lab(0 0 0 / 0.5)"), Rgba::new(0, 0, 0, 128)));
        assert!(close(c("color(srgb 1 0.5 0)"), Rgba::rgb(255, 128, 0)));
        assert!(close(c("color(srgb-linear 1 0 0)"), Rgba::rgb(255, 0, 0)));
        assert!(close(
            c("color(xyz 0.9505 1 1.089)"),
            Rgba::rgb(255, 255, 255)
        ));
        assert!(close(c("color(display-p3 0 0 0)"), Rgba::rgb(0, 0, 0)));
        assert_eq!(c("color(unknown 1 0 0)"), None);
    }

    #[test]
    fn color_mix() {
        assert!(close(
            c("color-mix(in srgb, red, blue)"),
            Rgba::rgb(128, 0, 128)
        ));
        assert!(close(
            c("color-mix(in srgb, red 25%, blue)"),
            Rgba::rgb(64, 0, 191)
        ));
        assert!(close(
            c("color-mix(in srgb, white 50%, transparent)"),
            Rgba::new(255, 255, 255, 128)
        ));
        assert!(close(
            c("color-mix(in srgb, red 20%, blue 20%)"),
            Rgba::new(128, 0, 128, 102)
        ));
        assert!(c("color-mix(in oklab, red, blue)").is_some());
        assert!(c("color-mix(in oklch longer hue, red, blue)").is_some());
        assert_eq!(c("color-mix(in srgb, currentColor, blue)"), None);
        assert_eq!(c("color-mix(in srgb, red 0%, blue 0%)"), None);
        assert_eq!(c("color-mix(in nope, red, blue)"), None);
    }

    #[test]
    fn legacy_colors() {
        let cases: &[(&str, Option<Rgba>)] = &[
            ("#ff6600", Some(Rgba::rgb(255, 0x66, 0))),
            ("#f6f6ef", Some(Rgba::rgb(0xf6, 0xf6, 0xef))),
            ("ff6600", Some(Rgba::rgb(255, 0x66, 0))),
            ("#fff", Some(Rgba::rgb(255, 255, 255))),
            ("red", Some(Rgba::rgb(255, 0, 0))),
            (" Blue ", Some(Rgba::rgb(0, 0, 255))),
            ("transparent", None),
            ("", None),
            ("chucknorris", Some(Rgba::rgb(0xc0, 0, 0))),
            ("#abcd", Some(Rgba::rgb(0xab, 0xcd, 0))),
            ("#1234567890", Some(Rgba::rgb(0x12, 0x56, 0x90))),
        ];
        for (input, expected) in cases {
            assert_eq!(parse_legacy_color(input), *expected, "{input}");
        }
    }
}
