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
//! [`MAX_SPECIAL_COLORS`] entries (2^20, about 40 MB). When it is full:
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

/// The most distinct mixes and wide colors that the table holds.
pub(crate) const MAX_SPECIAL_COLORS: usize = 1 << 20;

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
}

impl Table {
    /// The id of `special`, adding it; `None` if the table has `limit`
    /// entries.
    fn intern(&mut self, special: Special, limit: usize) -> Option<ColorId> {
        if let Some(&id) = self.ids.get(&special) {
            return Some(ColorId(id));
        }
        if self.entries.len() >= limit {
            return None;
        }
        let id = u32::try_from(self.entries.len()).ok()?;
        self.entries.push(special);
        self.ids.insert(special, id);
        Some(ColorId(id))
    }

    /// The color for `wide`: an id, or its 8-bit color if the table is
    /// full.
    fn wide_color(&mut self, wide: Wide, limit: usize) -> Color {
        match self.intern(Special::Wide(wide), limit) {
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
        Ok(mut table) => table.wide_color(wide, MAX_SPECIAL_COLORS),
        Err(_) => Color::Rgba(wide.to_rgba()),
    }
}

impl ColorId {
    /// The id of `special`, adding it to the table; `None` if the table
    /// is full.
    pub(crate) fn intern(special: Special) -> Option<ColorId> {
        TABLE.lock().ok()?.intern(special, MAX_SPECIAL_COLORS)
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
}

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
    };

    /// An 8-bit color.
    pub(crate) fn from_rgba(c: Rgba) -> Wide {
        Wide {
            space: Space::Srgb,
            c: [c.r, c.g, c.b].map(|v| Bits::new(f32::from(v) / 255.0)),
            alpha: Bits::new(c.alpha_f32()),
        }
    }

    fn new(space: Space, c: [f32; 3], alpha: f32) -> Wide {
        Wide {
            space,
            c: c.map(Bits::new),
            alpha: Bits::new(alpha.clamp(0.0, 1.0)),
        }
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
    /// a)`, `lab(L a b / a)`, and so on, with six significant digits.
    pub(crate) fn serialize(self) -> String {
        let [a, b, c] = self.coords().map(number);
        let alpha = self.alpha();
        let tail = if alpha < 1.0 {
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

/// A number with six significant digits (`%g`).
fn number(v: f32) -> String {
    crate::cssom::format_number(f64::from(v))
}

/// The linear sRGB coordinates of `c` in `space`.
fn from_space(space: Space, c: [f32; 3]) -> [f32; 3] {
    match space {
        Space::Srgb => c.map(srgb_to_linear),
        Space::Hsl => hsl_to_srgb(c[2], c[1], c[0]).map(srgb_to_linear),
        Space::Hwb => hwb_to_srgb(c[0], c[1], c[2]).map(srgb_to_linear),
        Space::SrgbLinear => c,
        Space::DisplayP3 => xyz_d65_to_linear_srgb(mul(&P3_TO_XYZ_D65, c.map(srgb_to_linear))),
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
    if color.space == space && !space.is_polar() {
        return (color.coords(), false);
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
    let (aa, ab) = (first.alpha(), second.alpha());
    let hue = space.hue_index();
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
    Wide::new(result, coords, alpha * alpha_scale)
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
        assert!(matches!(table.wide_color(a, 2), Color::Special(_)));
        assert!(matches!(table.wide_color(b, 2), Color::Special(_)));
        // Full: a new color is 8-bit, a known color keeps its id.
        assert_eq!(table.wide_color(c, 2), Color::Rgba(c.to_rgba()));
        assert!(matches!(table.wide_color(a, 2), Color::Special(_)));
        let mix = Special::Mix(Mix::new(
            Space::Srgb,
            HueMethod::Shorter,
            Item::Current,
            Item::Color(a),
            [50.0, 50.0],
        ));
        assert_eq!(table.intern(mix, 2), None);
        assert!(table.intern(mix, 3).is_some());
    }

    #[test]
    fn the_table_gives_equal_ids_to_equal_colors() {
        let a = ColorId::intern(Special::Wide(srgb(1, 2, 3)));
        let b = ColorId::intern(Special::Wide(srgb(1, 2, 3)));
        assert_eq!(a, b);
        assert_ne!(a, ColorId::intern(Special::Wide(srgb(1, 2, 4))));
    }
}
