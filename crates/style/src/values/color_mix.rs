//! `color-mix()` values: the mix of two colors in a color space (CSS Color
//! 5 section 3), kept as Chromium keeps it (ADR 0038, section "7b").
//!
//! A mix of two concrete colors is computed when it is parsed. Its result
//! is a [`Wide`] color: three coordinates and an alpha in `f32`, in the
//! space that Chromium serializes (`color(srgb 0.5 0 0.5)`, `oklab(...)`),
//! not rounded to 8 bits. A mix with `currentcolor` stays a [`Mix`] until
//! the used value: it resolves with the `color` of the element that the
//! value is on, so an inherited mix follows the color of the element that
//! inherits it (measured in Chromium 148, `cm-inheritance`).
//!
//! [`Color`](super::Color) is `Copy` and 8 bytes, so these values live in
//! a process-wide table and the color holds a [`ColorId`]. The table is
//! shared by all documents and never shrinks. It has at most
//! [`MAX_WIDE_COLORS`] `Wide` entries (2^20) and, apart from them, at most
//! [`MAX_MIXES`] `Mix` entries (2^16): a page that fills the room of the
//! wide colors cannot take the room of the mixes of later pages. A full
//! table of both kinds takes about 200 MB (the entries, and a copy of each
//! in the map that finds them; measured with 1.1 million `lab()` colors).
//! When a kind is full (`warn!` once per document, see
//! [`reset_color_table_warnings`]):
//!
//! - a `Wide` color (a mix of two concrete colors, or the resolved mix of
//!   the `color` property) becomes the 8-bit `Color::Rgba` of the same
//!   color: paint stays right, and only the digits that `getComputedStyle`
//!   shows differ;
//! - a mix with `currentcolor` ([`Special::Mix`]) is an invalid value.
//!
//! One table, not one per document: a table per document needs a table
//! handle at every parse, cascade, CSSOM and paint site, and values move
//! between documents (adopted nodes, a moved `style` attribute), so ids of
//! one table would be wrong in another. Backlog: a color table per
//! document.
//!
//! <https://www.w3.org/TR/css-color-5/#color-mix>

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use super::{Color, Rgba};
use crate::parse::color::{
    LINEAR_SRGB_TO_XYZ_D65, Matrix, P3_TO_XYZ_D65, d50_to_d65, d65_to_d50, hsl_to_srgb,
    hwb_to_srgb, lab_to_xyz_d50, linear_srgb_to_rgba, linear_to_srgb, mul, srgb_to_linear,
    xyz_d50_to_lab, xyz_d65_to_linear_srgb,
};

/// The most distinct wide colors that the table holds.
pub(crate) const MAX_WIDE_COLORS: usize = 1 << 20;

/// The most distinct mixes with `currentcolor` that the table holds, in
/// addition to the wide colors.
pub(crate) const MAX_MIXES: usize = 1 << 16;

/// How many entries of each kind a table may hold.
#[derive(Clone, Copy)]
struct Limits {
    wide: usize,
    mix: usize,
}

const LIMITS: Limits = Limits {
    wide: MAX_WIDE_COLORS,
    mix: MAX_MIXES,
};

/// The index of a [`Special`] color in the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ColorId(u32);

/// A color that does not fit in 8-bit sRGB or that depends on `currentcolor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Special {
    /// A computed color with float coordinates.
    Wide(Wide),
    /// A mix with `currentcolor`.
    Mix(Mix),
}

#[derive(Default)]
struct Table {
    entries: Vec<Special>,
    ids: HashMap<Special, u32>,
    /// The entries that are mixes (the others are wide colors).
    mixes: usize,
    /// Whether the table was found full for `[wide, mix]` since the last
    /// [`reset_color_table_warnings`] (so that one document logs it once).
    warned: [bool; 2],
}

impl Table {
    /// The id of `special`, adding it; `None` if the entries of its kind
    /// are at their `limits`.
    fn intern(&mut self, special: Special, limits: Limits) -> Option<ColorId> {
        if let Some(&id) = self.ids.get(&special) {
            return Some(ColorId(id));
        }
        let is_mix = matches!(special, Special::Mix(_));
        let (count, limit) = if is_mix {
            (self.mixes, limits.mix)
        } else {
            (self.entries.len() - self.mixes, limits.wide)
        };
        if count >= limit {
            let warned = &mut self.warned[usize::from(is_mix)];
            if !*warned {
                *warned = true;
                log::warn!(
                    "the color table has {limit} {}: more are {}",
                    if is_mix {
                        "mixes with currentcolor"
                    } else {
                        "wide colors"
                    },
                    if is_mix {
                        "invalid values"
                    } else {
                        "8-bit colors"
                    },
                );
            }
            return None;
        }
        let id = u32::try_from(self.entries.len()).ok()?;
        self.entries.push(special);
        self.mixes += usize::from(is_mix);
        self.ids.insert(special, id);
        Some(ColorId(id))
    }

    /// The color for `wide`: an id, or its 8-bit color if the table is
    /// full.
    fn wide_color(&mut self, wide: Wide, limits: Limits) -> Color {
        match self.intern(Special::Wide(wide), limits) {
            Some(id) => Color::Special(id),
            None => Color::Rgba(wide.to_rgba()),
        }
    }
}

static TABLE: LazyLock<Mutex<Table>> = LazyLock::new(Mutex::default);

/// The color for `wide`: an id in the process-wide table, or the 8-bit
/// color if the table is full.
pub(crate) fn wide_color(wide: Wide) -> Color {
    match TABLE.lock() {
        Ok(mut table) => table.wide_color(wide, LIMITS),
        Err(_) => Color::Rgba(wide.to_rgba()),
    }
}

/// Lets the color table log again that it is full: a new document calls
/// this, so that the log shows the limit once per document.
pub fn reset_color_table_warnings() {
    if let Ok(mut table) = TABLE.lock() {
        table.warned = [false; 2];
    }
}

impl ColorId {
    /// The id of `special`, adding it to the table; `None` if the table
    /// is full.
    pub(crate) fn intern(special: Special) -> Option<ColorId> {
        TABLE.lock().ok()?.intern(special, LIMITS)
    }

    /// The color that this id stands for.
    pub(crate) fn get(self) -> Special {
        let table = TABLE.lock().ok();
        table
            .and_then(|t| t.entries.get(self.0 as usize).copied())
            .unwrap_or(Special::Wide(Wide::TRANSPARENT))
    }

    /// The used color, given the element's `color`.
    pub(crate) fn resolve(self, current: Rgba) -> Rgba {
        match self.get() {
            Special::Wide(w) => w.to_rgba(),
            Special::Mix(m) => m.resolve(current).to_rgba(),
        }
    }
}

/// A float with equality and hashing by its bits (no NaN is stored).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Bits(u32);

impl Bits {
    fn new(v: f32) -> Bits {
        Bits(if v.is_finite() { v } else { 0.0 }.to_bits())
    }

    fn get(self) -> f32 {
        f32::from_bits(self.0)
    }
}

/// A color space of `color-mix()` (`in <space>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Space {
    /// `srgb`
    Srgb,
    /// `srgb-linear`
    SrgbLinear,
    /// `display-p3`
    DisplayP3,
    /// `a98-rgb`
    A98Rgb,
    /// `prophoto-rgb`
    ProphotoRgb,
    /// `rec2020`
    Rec2020,
    /// `lab`
    Lab,
    /// `lch`
    Lch,
    /// `oklab`
    Oklab,
    /// `oklch`
    Oklch,
    /// `xyz-d50`
    XyzD50,
    /// `xyz` and `xyz-d65`
    XyzD65,
    /// `hsl` (the result is in sRGB)
    Hsl,
    /// `hwb` (the result is in sRGB)
    Hwb,
}

impl Space {
    /// The space named by `name` (lower case).
    pub(crate) fn from_name(name: &str) -> Option<Space> {
        Some(match name {
            "srgb" => Space::Srgb,
            "srgb-linear" => Space::SrgbLinear,
            "display-p3" => Space::DisplayP3,
            "a98-rgb" => Space::A98Rgb,
            "prophoto-rgb" => Space::ProphotoRgb,
            "rec2020" => Space::Rec2020,
            "lab" => Space::Lab,
            "lch" => Space::Lch,
            "oklab" => Space::Oklab,
            "oklch" => Space::Oklch,
            "xyz" | "xyz-d65" => Space::XyzD65,
            "xyz-d50" => Space::XyzD50,
            "hsl" => Space::Hsl,
            "hwb" => Space::Hwb,
            _ => return None,
        })
    }

    /// True for the spaces that have a hue.
    pub(crate) fn is_polar(self) -> bool {
        matches!(self, Space::Lch | Space::Oklch | Space::Hsl | Space::Hwb)
    }

    /// The name in `color-mix()` and in a serialized color.
    fn name(self) -> &'static str {
        match self {
            Space::Srgb | Space::Hsl | Space::Hwb => "srgb",
            Space::SrgbLinear => "srgb-linear",
            Space::DisplayP3 => "display-p3",
            Space::A98Rgb => "a98-rgb",
            Space::ProphotoRgb => "prophoto-rgb",
            Space::Rec2020 => "rec2020",
            Space::Lab => "lab",
            Space::Lch => "lch",
            Space::Oklab => "oklab",
            Space::Oklch => "oklch",
            Space::XyzD50 => "xyz-d50",
            Space::XyzD65 => "xyz-d65",
        }
    }

    /// The space of the result: `hsl` and `hwb` mixes are in sRGB.
    fn result(self) -> Space {
        match self {
            Space::Hsl | Space::Hwb => Space::Srgb,
            other => other,
        }
    }

    /// The index of the hue among the coordinates.
    fn hue_index(self) -> Option<usize> {
        match self {
            Space::Lch | Space::Oklch | Space::Hsl => Some(2),
            Space::Hwb => Some(0),
            _ => None,
        }
    }

    /// The name as `color-mix()` writes it.
    fn mix_name(self) -> &'static str {
        match self {
            Space::Hsl => "hsl",
            Space::Hwb => "hwb",
            other => other.name(),
        }
    }
}

/// The hue interpolation method.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum HueMethod {
    /// `shorter hue` (the default)
    Shorter,
    /// `longer hue`
    Longer,
    /// `increasing hue`
    Increasing,
    /// `decreasing hue`
    Decreasing,
}

/// A color with float coordinates in a space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Wide {
    space: Space,
    c: [Bits; 3],
    alpha: Bits,
    /// The components that are `none` (bits 0 to 2: the coordinates, bit 3:
    /// the alpha). Their stored values are 0.
    none: u8,
}

/// The bit of the alpha in [`Wide::none`].
pub(crate) const NONE_ALPHA: u8 = 8;

const XYZ_D65_TO_P3: Matrix = [
    [2.493_497, -0.931_383_6, -0.402_710_8],
    [-0.829_489, 1.762_664, 0.023_624_686],
    [0.035_845_83, -0.076_172_39, 0.956_884_5],
];

/// The matrices of CSS Color 4 section 9.2 (XYZ D65 to LMS, LMS' to
/// Oklab and their inverses).
/// <https://www.w3.org/TR/css-color-4/#color-conversion-code>
const XYZ_D65_TO_LMS: Matrix = [
    [0.819_022_4, 0.361_906_26, -0.128_873_78],
    [0.032_983_654, 0.929_286_9, 0.036_144_666],
    [0.048_177_19, 0.264_239_53, 0.633_547_8],
];
const LMS_TO_OKLAB: Matrix = [
    [0.210_454_27, 0.793_617_8, -0.004_072_043],
    [1.977_998_5, -2.428_592_2, 0.450_593_7],
    [0.025_904_04, 0.782_771_7, -0.808_675_77],
];
const LMS_TO_XYZ_D65: Matrix = [
    [1.226_88, -0.557_814_97, 0.281_391_05],
    [-0.040_575_745, 1.112_286_8, -0.071_711_06],
    [-0.076_372_94, -0.421_493_33, 1.586_924],
];
const OKLAB_TO_LMS: Matrix = [
    [1.0, 0.396_337_78, 0.215_803_76],
    [1.0, -0.105_561_35, -0.063_854_17],
    [1.0, -0.089_484_18, -1.291_485_5],
];

/// Linear A98 RGB to XYZ D65 (CSS Color 4 section 10.4).
const A98_TO_XYZ_D65: Matrix = [
    [0.576_669_04, 0.185_558_24, 0.188_228_65],
    [0.297_344_98, 0.627_363_57, 0.075_291_46],
    [0.027_031_362, 0.070_688_85, 0.991_337_54],
];
/// Linear Rec. 2020 to XYZ D65 (CSS Color 4 section 10.6).
const REC2020_TO_XYZ_D65: Matrix = [
    [0.636_958_05, 0.144_616_9, 0.168_880_98],
    [0.262_700_2, 0.677_998_07, 0.059_301_715],
    [0.0, 0.028_072_694, 1.060_985_1],
];
/// Linear `ProPhoto` RGB to XYZ D50 (CSS Color 4 section 10.5).
const PROPHOTO_TO_XYZ_D50: Matrix = [
    [0.797_760_5, 0.135_185_84, 0.031_349_35],
    [0.288_071_13, 0.711_843_2, 0.000_085_653_96],
    [0.0, 0.0, 0.825_104_6],
];

/// The inverse of a 3 x 3 matrix (the identity if it is singular).
fn invert(m: &Matrix) -> Matrix {
    let c = |r: usize, k: usize| {
        let (r1, r2) = ((r + 1) % 3, (r + 2) % 3);
        let (k1, k2) = ((k + 1) % 3, (k + 2) % 3);
        m[r1][k1] * m[r2][k2] - m[r1][k2] * m[r2][k1]
    };
    let det = m[0][0] * c(0, 0) + m[0][1] * c(0, 1) + m[0][2] * c(0, 2);
    if det.abs() < 1e-12 {
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    }
    // The inverse is the transposed matrix of cofactors over the
    // determinant; `c` is the cofactor.
    [0, 1, 2].map(|i| [0, 1, 2].map(|j| c(j, i) / det))
}

/// The sign-preserving power.
fn spow(v: f32, e: f32) -> f32 {
    v.signum() * v.abs().powf(e)
}

fn a98_to_linear(v: f32) -> f32 {
    spow(v, 563.0 / 256.0)
}

fn linear_to_a98(v: f32) -> f32 {
    spow(v, 256.0 / 563.0)
}

const REC2020_ALPHA: f32 = 1.099_296_8;
const REC2020_BETA: f32 = 0.018_053_97;

fn rec2020_to_linear(v: f32) -> f32 {
    if v.abs() < REC2020_BETA * 4.5 {
        v / 4.5
    } else {
        spow((v.abs() + REC2020_ALPHA - 1.0) / REC2020_ALPHA, 1.0 / 0.45) * v.signum()
    }
}

fn linear_to_rec2020(v: f32) -> f32 {
    if v.abs() < REC2020_BETA {
        4.5 * v
    } else {
        v.signum() * (REC2020_ALPHA * v.abs().powf(0.45) - (REC2020_ALPHA - 1.0))
    }
}

fn prophoto_to_linear(v: f32) -> f32 {
    if v.abs() <= 16.0 / 512.0 {
        v / 16.0
    } else {
        spow(v, 1.8)
    }
}

fn linear_to_prophoto(v: f32) -> f32 {
    if v.abs() >= 1.0 / 512.0 {
        spow(v, 1.0 / 1.8)
    } else {
        16.0 * v
    }
}

fn linear_to_oklab(lin: [f32; 3]) -> [f32; 3] {
    let lms = mul(&XYZ_D65_TO_LMS, mul(&LINEAR_SRGB_TO_XYZ_D65, lin)).map(f32::cbrt);
    mul(&LMS_TO_OKLAB, lms)
}

fn oklab_to_linear(lab: [f32; 3]) -> [f32; 3] {
    let lms = mul(&OKLAB_TO_LMS, lab).map(|v| v * v * v);
    xyz_d65_to_linear_srgb(mul(&LMS_TO_XYZ_D65, lms))
}

impl Wide {
    /// Transparent black in sRGB.
    pub(crate) const TRANSPARENT: Wide = Wide {
        space: Space::Srgb,
        c: [Bits(0), Bits(0), Bits(0)],
        alpha: Bits(0),
        none: 0,
    };

    /// An 8-bit color.
    pub(crate) fn from_rgba(c: Rgba) -> Wide {
        Wide {
            space: Space::Srgb,
            c: [c.r, c.g, c.b].map(|v| Bits::new(f32::from(v) / 255.0)),
            alpha: Bits::new(c.alpha_f32()),
            none: 0,
        }
    }

    pub(crate) fn new(space: Space, c: [f32; 3], alpha: f32) -> Wide {
        Wide {
            space,
            c: c.map(Bits::new),
            alpha: Bits::new(alpha.clamp(0.0, 1.0)),
            none: 0,
        }
    }

    /// The color with the components of `none` (see [`Wide::none`]) missing.
    pub(crate) fn with_none(mut self, none: u8) -> Wide {
        self.none = none & 0xf;
        for (i, bits) in self.c.iter_mut().enumerate() {
            if self.none & (1 << i) != 0 {
                *bits = Bits(0);
            }
        }
        if self.none & NONE_ALPHA != 0 {
            self.alpha = Bits(0);
        }
        self
    }

    fn coords(self) -> [f32; 3] {
        self.c.map(Bits::get)
    }

    /// The alpha, 0 to 1.
    pub(crate) fn alpha(self) -> f32 {
        self.alpha.get()
    }

    /// The linear sRGB coordinates.
    fn to_linear(self) -> [f32; 3] {
        from_space(self.space, self.coords())
    }

    /// The 8-bit sRGB color that is painted.
    pub(crate) fn to_rgba(self) -> Rgba {
        if self.space == Space::Srgb {
            let [r, g, b] = self.coords();
            return Rgba::from_f32(r * 255.0, g * 255.0, b * 255.0, self.alpha());
        }
        linear_srgb_to_rgba(self.to_linear(), self.alpha())
    }

    /// The computed value as Chromium serializes it: `color(srgb r g b /
    /// a)`, `lab(L a b / a)`, and so on, with six significant digits;
    /// missing components are `none`.
    pub(crate) fn serialize(self) -> String {
        let [a, b, c] = [0, 1, 2].map(|i| {
            if self.none & (1 << i) != 0 {
                "none".to_owned()
            } else {
                number(self.coords()[i])
            }
        });
        let alpha = self.alpha();
        let tail = if self.none & NONE_ALPHA != 0 {
            " / none".to_owned()
        } else if alpha < 1.0 {
            format!(" / {}", number(alpha))
        } else {
            String::new()
        };
        match self.space {
            Space::Lab | Space::Lch | Space::Oklab | Space::Oklch => {
                format!("{}({a} {b} {c}{tail})", self.space.name())
            }
            space => format!("color({} {a} {b} {c}{tail})", space.name()),
        }
    }
}

/// A number with six significant digits as Chromium writes the
/// coordinates of a color: fixed notation for 1e-6 up to (not including)
/// 1e6, else scientific with six digits and no zeros trimmed
/// (`1.00000e-7`, `1.23457e+6`; measured in Chromium 148,
/// `c-number-format`).
fn number(v: f32) -> String {
    let v = f64::from(v);
    if v == 0.0 || !v.is_finite() {
        return "0".to_owned();
    }
    let scientific = format!("{v:.5e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .unwrap_or((scientific.as_str(), "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    if !(-6..6).contains(&exponent) {
        let sign = if exponent < 0 { '-' } else { '+' };
        return format!("{mantissa}e{sign}{}", exponent.abs());
    }
    let decimals = usize::try_from(5 - exponent).unwrap_or(0);
    let fixed = format!("{v:.decimals$}");
    if fixed.contains('.') {
        fixed.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        fixed
    }
}

/// The linear sRGB coordinates of `c` in `space`.
fn from_space(space: Space, c: [f32; 3]) -> [f32; 3] {
    match space {
        Space::Srgb => c.map(srgb_to_linear),
        Space::Hsl => hsl_to_srgb(c[2], c[1], c[0]).map(srgb_to_linear),
        Space::Hwb => hwb_to_srgb(c[0], c[1], c[2]).map(srgb_to_linear),
        Space::SrgbLinear => c,
        Space::DisplayP3 => xyz_d65_to_linear_srgb(mul(&P3_TO_XYZ_D65, c.map(srgb_to_linear))),
        Space::A98Rgb => xyz_d65_to_linear_srgb(mul(&A98_TO_XYZ_D65, c.map(a98_to_linear))),
        Space::Rec2020 => {
            xyz_d65_to_linear_srgb(mul(&REC2020_TO_XYZ_D65, c.map(rec2020_to_linear)))
        }
        Space::ProphotoRgb => xyz_d65_to_linear_srgb(d50_to_d65(mul(
            &PROPHOTO_TO_XYZ_D50,
            c.map(prophoto_to_linear),
        ))),
        Space::Lab => xyz_d65_to_linear_srgb(d50_to_d65(lab_to_xyz_d50(c))),
        Space::Lch => {
            let h = c[2].to_radians();
            from_space(Space::Lab, [c[0], c[1] * h.cos(), c[1] * h.sin()])
        }
        Space::Oklab => oklab_to_linear(c),
        Space::Oklch => {
            let h = c[2].to_radians();
            oklab_to_linear([c[0], c[1] * h.cos(), c[1] * h.sin()])
        }
        Space::XyzD50 => xyz_d65_to_linear_srgb(d50_to_d65(c)),
        Space::XyzD65 => xyz_d65_to_linear_srgb(c),
    }
}

/// The coordinates of `color` in `space`, and whether the hue is missing
/// (powerless: a gray). Colors that are in the space (or in sRGB for `hsl`
/// and `hwb`) are not converted through linear light, which would round.
fn coords_in(space: Space, color: Wide) -> ([f32; 3], bool) {
    if color.space == space {
        // A color in the space keeps its coordinates. Its hue is missing
        // if it is `none` or if the chroma is too small to tell it (a
        // chroma that is `none` takes the other color's, and the hue
        // stays).
        let c = color.coords();
        let small = |eps: f32| color.none & 2 == 0 && c[1] < eps;
        let missing = match space {
            Space::Lch => color.none & 4 != 0 || small(0.0015),
            Space::Oklch => color.none & 4 != 0 || small(0.000_004),
            _ => false,
        };
        return (c, missing);
    }
    // The polar form of a rectangular space and back are exact: no trip
    // through linear light, which rounds.
    match (color.space, space) {
        (Space::Lch, Space::Lab) | (Space::Oklch, Space::Oklab) => {
            let [l, c, h] = color.coords();
            let h = h.to_radians();
            return ([l, c * h.cos(), c * h.sin()], false);
        }
        (Space::Lab, Space::Lch) | (Space::Oklab, Space::Oklch) => {
            let [l, a, b] = color.coords();
            let chroma = a.hypot(b);
            let hue = b.atan2(a).to_degrees().rem_euclid(360.0);
            let eps = if space == Space::Lch {
                0.0015
            } else {
                0.000_004
            };
            return ([l, chroma, hue], chroma < eps);
        }
        _ => {}
    }
    if color.space == Space::Srgb && matches!(space, Space::Hsl | Space::Hwb) {
        return hsl_or_hwb(space, color.coords());
    }
    to_space(space, color.to_linear())
}

/// The `hsl` or `hwb` coordinates of the sRGB color `rgb`.
fn hsl_or_hwb(space: Space, [r, g, b]: [f32; 3]) -> ([f32; 3], bool) {
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let delta = max - min;
    let light = f32::midpoint(max, min);
    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / delta).rem_euclid(6.0) * 60.0
    } else if max == g {
        ((b - r) / delta + 2.0) * 60.0
    } else {
        ((r - g) / delta + 4.0) * 60.0
    };
    if space == Space::Hwb {
        return ([hue, min, 1.0 - max], delta < 1e-6);
    }
    let sat = if light <= 0.0 || light >= 1.0 {
        0.0
    } else {
        (max - light) / light.min(1.0 - light)
    };
    // The hsl coordinates are in the order (light, sat, hue) so that the
    // hue is index 2 as in lch.
    ([light, sat, hue], sat < 1e-6)
}

/// The coordinates of the linear sRGB color `lin` in `space`, and whether
/// the hue is missing.
fn to_space(space: Space, lin: [f32; 3]) -> ([f32; 3], bool) {
    let polar = |l: f32, a: f32, b: f32, epsilon: f32| {
        let chroma = a.hypot(b);
        let hue = b.atan2(a).to_degrees().rem_euclid(360.0);
        ([l, chroma, hue], chroma < epsilon)
    };
    match space {
        Space::Srgb => (lin.map(linear_to_srgb), false),
        Space::SrgbLinear => (lin, false),
        Space::DisplayP3 => {
            let p3 = mul(&XYZ_D65_TO_P3, mul(&LINEAR_SRGB_TO_XYZ_D65, lin));
            (p3.map(linear_to_srgb), false)
        }
        Space::A98Rgb => {
            let xyz = mul(&LINEAR_SRGB_TO_XYZ_D65, lin);
            (mul(&invert(&A98_TO_XYZ_D65), xyz).map(linear_to_a98), false)
        }
        Space::Rec2020 => {
            let xyz = mul(&LINEAR_SRGB_TO_XYZ_D65, lin);
            (
                mul(&invert(&REC2020_TO_XYZ_D65), xyz).map(linear_to_rec2020),
                false,
            )
        }
        Space::ProphotoRgb => {
            let xyz = d65_to_d50(mul(&LINEAR_SRGB_TO_XYZ_D65, lin));
            (
                mul(&invert(&PROPHOTO_TO_XYZ_D50), xyz).map(linear_to_prophoto),
                false,
            )
        }
        Space::Lab => (
            xyz_d50_to_lab(d65_to_d50(mul(&LINEAR_SRGB_TO_XYZ_D65, lin))),
            false,
        ),
        Space::Lch => {
            let [l, a, b] = xyz_d50_to_lab(d65_to_d50(mul(&LINEAR_SRGB_TO_XYZ_D65, lin)));
            polar(l, a, b, 0.0015)
        }
        Space::Oklab => (linear_to_oklab(lin), false),
        Space::Oklch => {
            let [l, a, b] = linear_to_oklab(lin);
            polar(l, a, b, 0.000_004)
        }
        Space::XyzD50 => (d65_to_d50(mul(&LINEAR_SRGB_TO_XYZ_D65, lin)), false),
        Space::XyzD65 => (mul(&LINEAR_SRGB_TO_XYZ_D65, lin), false),
        Space::Hsl | Space::Hwb => hsl_or_hwb(space, lin.map(linear_to_srgb)),
    }
}

/// One side of a mix.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Item {
    /// `currentcolor`.
    Current,
    /// A color.
    Color(Wide),
}

/// `color-mix()` with `currentcolor` among its two colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Mix {
    space: Space,
    hue: HueMethod,
    first: Item,
    second: Item,
    /// The percentages after defaulting (the first and second).
    percent: [Bits; 2],
}

impl Mix {
    /// A mix of two items: `percent` are the normalized percentages (the
    /// second is `100 - first` if left out).
    pub(crate) fn new(
        space: Space,
        hue: HueMethod,
        first: Item,
        second: Item,
        percent: [f32; 2],
    ) -> Mix {
        Mix {
            space,
            hue,
            first,
            second,
            percent: percent.map(Bits::new),
        }
    }

    /// Resolves `currentcolor` and mixes.
    pub(crate) fn resolve(self, current: Rgba) -> Wide {
        self.resolve_wide(Wide::from_rgba(current))
    }

    /// [`Mix::resolve`] with `currentcolor` a color with float coordinates
    /// (the exact `color` of the parent, for the `color` property).
    pub(crate) fn resolve_wide(self, current: Wide) -> Wide {
        let color = |item: Item| match item {
            Item::Current => current,
            Item::Color(w) => w,
        };
        mix(
            self.space,
            self.hue,
            color(self.first),
            color(self.second),
            [self.percent[0].get(), self.percent[1].get()],
        )
    }

    /// The computed value as Chromium writes it when the color stays a
    /// mix: `color-mix(in srgb, currentcolor 25%, rgb(255, 255, 255))`.
    pub(crate) fn serialize(self) -> String {
        let item = |i: Item| match i {
            Item::Current => "currentcolor".to_owned(),
            Item::Color(w) => {
                let rgba = w.to_rgba();
                if w.space == Space::Srgb && Wide::from_rgba(rgba) == w {
                    crate::cssom::rgba_text(rgba)
                } else {
                    w.serialize()
                }
            }
        };
        let [p1, p2] = self.percent.map(Bits::get);
        let percents = percent_suffixes(p1, p2);
        format!(
            "color-mix({}, {}{}, {}{})",
            space_text(self.space, self.hue),
            item(self.first),
            percents[0],
            item(self.second),
            percents[1],
        )
    }
}

/// `in srgb`, `in lch longer hue`.
fn space_text(space: Space, hue: HueMethod) -> String {
    let mut text = format!("in {}", space.mix_name());
    if space.is_polar() {
        match hue {
            HueMethod::Shorter => {}
            HueMethod::Longer => text.push_str(" longer hue"),
            HueMethod::Increasing => text.push_str(" increasing hue"),
            HueMethod::Decreasing => text.push_str(" decreasing hue"),
        }
    }
    text
}

/// The specified `color-mix()` as `element.style` writes it: the color
/// space is left out for the default (`oklab`), the percentages are
/// normalized ([`percent_suffixes`]) and each color is written by
/// `color_text`. `None` if the arguments do not have the form (they have
/// `var()` for example).
pub(crate) fn specified_text(
    args: &[swb_css::ComponentValue],
    color_text: &dyn Fn(&[swb_css::ComponentValue]) -> String,
) -> Option<String> {
    use swb_css::{ComponentValue as C, split_on_commas, trim_whitespace};
    let parts: Vec<&[C]> = split_on_commas(args).map(trim_whitespace).collect();
    let (space, items) = match parts.as_slice() {
        [space, a, b] => (Some(*space), [*a, *b]),
        [a, b] => (None, [*a, *b]),
        _ => return None,
    };
    let prefix = match space {
        None => String::new(),
        Some(tokens) => {
            let words: Vec<String> = tokens
                .iter()
                .filter(|t| !t.is_whitespace())
                .map(|t| match t {
                    C::Ident(w) => Some(w.to_ascii_lowercase()),
                    _ => None,
                })
                .collect::<Option<_>>()?;
            let [_, name, rest @ ..] = words.as_slice() else {
                return None;
            };
            let space = Space::from_name(name)?;
            let method = match rest {
                [] => HueMethod::Shorter,
                [m, h] if h == "hue" => match m.as_str() {
                    "shorter" => HueMethod::Shorter,
                    "longer" => HueMethod::Longer,
                    "increasing" => HueMethod::Increasing,
                    "decreasing" => HueMethod::Decreasing,
                    _ => return None,
                },
                _ => return None,
            };
            if space == Space::Oklab {
                String::new()
            } else {
                format!("{}, ", space_text(space, method))
            }
        }
    };
    let item = |tokens: &[C]| -> Option<(String, Option<f32>)> {
        let (color, pct) = match tokens {
            [C::Percentage(p), C::Whitespace, rest @ ..]
            | [rest @ .., C::Whitespace, C::Percentage(p)] => (rest, Some(p.value)),
            other => (other, None),
        };
        (!color.is_empty()).then(|| (color_text(color), pct))
    };
    let ((c1, p1), (c2, p2)) = (item(items[0])?, item(items[1])?);
    let (p1, p2) = match (p1, p2) {
        (None, None) => (50.0, 50.0),
        (Some(a), None) => (a, 100.0 - a),
        (None, Some(b)) => (100.0 - b, b),
        (Some(a), Some(b)) => (a, b),
    };
    let [s1, s2] = percent_suffixes(p1, p2);
    Some(format!("color-mix({prefix}{c1}{s1}, {c2}{s2})"))
}

/// The percentages of a serialized `color-mix()`: none for 50% and 50%,
/// only the first if they add up to 100%, else both (measured in Chromium
/// 148, `cm-spaces`).
pub(crate) fn percent_suffixes(p1: f32, p2: f32) -> [String; 2] {
    let percent = |p: f32| format!(" {}%", number(p));
    if (p1 - 50.0).abs() < 1e-4 && (p2 - 50.0).abs() < 1e-4 {
        [String::new(), String::new()]
    } else if ((p1 + p2) - 100.0).abs() < 1e-4 {
        [percent(p1), String::new()]
    } else {
        [percent(p1), percent(p2)]
    }
}

/// The mix of two colors (CSS Color 5 section 3.3 to 3.5): the coordinates
/// are interpolated premultiplied by the alpha, the hue by its method;
/// percentages that add up to less than 100% scale the alpha.
/// <https://www.w3.org/TR/css-color-5/#interpolation>
pub(crate) fn mix(
    space: Space,
    method: HueMethod,
    first: Wide,
    second: Wide,
    percent: [f32; 2],
) -> Wide {
    let sum = percent[0] + percent[1];
    let weight = if sum > 0.0 { percent[1] / sum } else { 0.5 };
    let alpha_scale = (sum / 100.0).min(1.0);
    let ((mut ca, miss_a), (mut cb, miss_b)) = (coords_in(space, first), coords_in(space, second));
    let (mut aa, mut ab) = (first.alpha(), second.alpha());
    // A missing alpha takes the other color's.
    match (first.none & NONE_ALPHA != 0, second.none & NONE_ALPHA != 0) {
        (true, false) => aa = ab,
        (false, true) => ab = aa,
        _ => {}
    }
    let hue = space.hue_index();
    // A missing coordinate of a color in the space takes the other
    // color's, if that one is in the space too (CSS Color 4 section 12.2).
    let both = first.space == space && second.space == space;
    let mut none_out = 0;
    if both {
        for i in 0..3 {
            let (na, nb) = (first.none & (1 << i) != 0, second.none & (1 << i) != 0);
            match (na, nb) {
                (true, false) => ca[i] = cb[i],
                (false, true) => cb[i] = ca[i],
                (true, true) => none_out |= 1 << i,
                _ => {}
            }
        }
    }
    if let Some(at) = hue {
        // A missing hue takes the other color's.
        match (miss_a, miss_b) {
            (true, false) => ca[at] = cb[at],
            (false, true) => cb[at] = ca[at],
            _ => {}
        }
        let (hue_a, hue_b) = fix_hue(ca[at], cb[at], method);
        ca[at] = hue_a;
        cb[at] = hue_b;
    }
    let alpha = aa * (1.0 - weight) + ab * weight;
    let mut out = [0.0; 3];
    for i in 0..3 {
        out[i] = if Some(i) == hue {
            ca[i] * (1.0 - weight) + cb[i] * weight
        } else if alpha > 0.0 {
            (ca[i] * aa * (1.0 - weight) + cb[i] * ab * weight) / alpha
        } else {
            0.0
        };
    }
    let result = space.result();
    let coords = match space {
        Space::Hsl => hsl_to_srgb(out[2], out[1], out[0]),
        Space::Hwb => hwb_to_srgb(out[0], out[1], out[2]),
        _ => {
            if let Some(at) = hue {
                out[at] = out[at].rem_euclid(360.0);
            }
            out
        }
    };
    let mut wide = Wide::new(result, coords, alpha * alpha_scale);
    if first.none & second.none & NONE_ALPHA != 0 {
        none_out |= NONE_ALPHA;
    }
    if none_out != 0 {
        wide = wide.with_none(none_out);
    }
    wide
}

/// The two hues after the interpolation method (CSS Color 4 section 12.4).
fn fix_hue(a: f32, b: f32, method: HueMethod) -> (f32, f32) {
    let (mut a, mut b) = (a.rem_euclid(360.0), b.rem_euclid(360.0));
    match method {
        HueMethod::Shorter => {
            if b - a > 180.0 {
                a += 360.0;
            } else if b - a < -180.0 {
                b += 360.0;
            }
        }
        HueMethod::Longer => {
            let d = b - a;
            if 0.0 < d && d < 180.0 {
                a += 360.0;
            } else if -180.0 < d && d <= 0.0 {
                b += 360.0;
            }
        }
        HueMethod::Increasing => {
            if b < a {
                b += 360.0;
            }
        }
        HueMethod::Decreasing => {
            if a < b {
                a += 360.0;
            }
        }
    }
    (a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn srgb(r: u8, g: u8, b: u8) -> Wide {
        Wide::from_rgba(Rgba::rgb(r, g, b))
    }

    #[test]
    fn mixes_in_srgb_keep_floats() {
        let w = mix(
            Space::Srgb,
            HueMethod::Shorter,
            srgb(255, 0, 0),
            srgb(0, 0, 255),
            [50.0, 50.0],
        );
        assert_eq!(w.serialize(), "color(srgb 0.5 0 0.5)");
        assert_eq!(w.to_rgba(), Rgba::rgb(128, 0, 128));
    }

    #[test]
    fn percentages_below_100_scale_the_alpha() {
        let w = mix(
            Space::Srgb,
            HueMethod::Shorter,
            srgb(255, 0, 0),
            srgb(0, 0, 255),
            [20.0, 30.0],
        );
        assert_eq!(w.serialize(), "color(srgb 0.4 0 0.6 / 0.5)");
    }

    #[test]
    fn transparent_is_premultiplied() {
        let w = mix(
            Space::Srgb,
            HueMethod::Shorter,
            srgb(255, 0, 0),
            Wide::TRANSPARENT,
            [50.0, 50.0],
        );
        assert_eq!(w.serialize(), "color(srgb 1 0 0 / 0.5)");
    }

    #[test]
    fn oklab_and_lch_results_keep_their_space() {
        let w = mix(
            Space::Oklab,
            HueMethod::Shorter,
            srgb(255, 0, 0),
            srgb(0, 0, 255),
            [50.0, 50.0],
        );
        assert!(
            w.serialize().starts_with("oklab(0.5399"),
            "{}",
            w.serialize()
        );
        let w = mix(
            Space::Lch,
            HueMethod::Shorter,
            srgb(255, 0, 0),
            srgb(0, 0, 255),
            [50.0, 50.0],
        );
        assert!(w.serialize().starts_with("lch(41.9"), "{}", w.serialize());
    }

    #[test]
    fn a_mix_with_currentcolor_follows_the_element() {
        let m = Mix::new(
            Space::Srgb,
            HueMethod::Shorter,
            Item::Current,
            Item::Color(Wide::TRANSPARENT),
            [50.0, 50.0],
        );
        assert_eq!(
            m.resolve(Rgba::rgb(0, 128, 0)).serialize(),
            "color(srgb 0 0.501961 0 / 0.5)"
        );
        assert_eq!(
            m.resolve(Rgba::rgb(255, 0, 0)).serialize(),
            "color(srgb 1 0 0 / 0.5)"
        );
        assert_eq!(
            m.serialize(),
            "color-mix(in srgb, currentcolor, rgba(0, 0, 0, 0))"
        );
    }

    #[test]
    fn serialized_percentages() {
        assert_eq!(percent_suffixes(50.0, 50.0), ["", ""]);
        assert_eq!(percent_suffixes(70.0, 30.0), [" 70%", ""]);
        assert_eq!(percent_suffixes(20.0, 30.0), [" 20%", " 30%"]);
        assert_eq!(percent_suffixes(60.0, 60.0), [" 60%", " 60%"]);
    }

    #[test]
    fn a_full_table_gives_the_8_bit_color_for_wide_colors_only() {
        let mut table = Table::default();
        let (a, b, c) = (srgb(1, 2, 3), srgb(1, 2, 4), srgb(1, 2, 5));
        let two = Limits { wide: 2, mix: 1 };
        assert!(matches!(table.wide_color(a, two), Color::Special(_)));
        assert!(matches!(table.wide_color(b, two), Color::Special(_)));
        // Full: a new color is 8-bit, a known color keeps its id.
        assert_eq!(table.wide_color(c, two), Color::Rgba(c.to_rgba()));
        assert!(matches!(table.wide_color(a, two), Color::Special(_)));
        let mix = Special::Mix(Mix::new(
            Space::Srgb,
            HueMethod::Shorter,
            Item::Current,
            Item::Color(a),
            [50.0, 50.0],
        ));
        // The full wide colors do not take the room of the mixes.
        assert!(table.intern(mix, two).is_some());
        let other = Special::Mix(Mix::new(
            Space::Srgb,
            HueMethod::Shorter,
            Item::Current,
            Item::Color(b),
            [50.0, 50.0],
        ));
        // The mixes are full too: an invalid value; a known one is found.
        assert_eq!(table.intern(other, two), None);
        assert!(table.intern(mix, two).is_some());
        assert!(table.warned[0] && table.warned[1]);
    }

    #[test]
    fn the_table_gives_equal_ids_to_equal_colors() {
        let a = ColorId::intern(Special::Wide(srgb(1, 2, 3)));
        let b = ColorId::intern(Special::Wide(srgb(1, 2, 3)));
        assert_eq!(a, b);
        assert_ne!(a, ColorId::intern(Special::Wide(srgb(1, 2, 4))));
    }
}
