//! The interpolation of colors (CSS Color 4 §12,
//! <https://www.w3.org/TR/css-color-4/#interpolation>), as Chromium 148
//! does it (`interp-colors`, `interp-color-current`; ADR 0044 section 4):
//!
//! - two legacy colors (`rgb()`, `hsl()`, `hwb()`, names, hex,
//!   `transparent`) mix in sRGB with premultiplied alpha, and the channels
//!   are rounded to 8 bits (`red` to `blue` at 0.25 is `rgb(191, 0, 64)`);
//! - otherwise (`oklch()`, `lab()`, `color()`, `color-mix()` on one end)
//!   in Oklab with premultiplied alpha;
//! - `currentcolor` is the element's (animated) `color`.
//!
//! The result is an 8-bit color, never an entry of the process-wide color
//! table (one entry per frame would fill it). So `getComputedStyle` during
//! an animation between non-legacy colors shows `rgb()` where Chromium
//! shows `oklab()`; the pixels are the same.

use super::to_f32;
use crate::ComputedStyle;
use crate::values::color_mix::{HueMethod, Space, Special, Wide, mix as mix_wide};
use crate::values::{Color, ColorId, Rgba};

/// A color resolved for interpolation.
enum Resolved {
    /// A legacy sRGB color.
    Legacy(Rgba),
    /// A color with float coordinates in some space.
    Wide(Wide),
}

fn resolve(c: Color, current: Rgba) -> Resolved {
    match c {
        Color::CurrentColor => Resolved::Legacy(current),
        Color::Rgba(rgba) => Resolved::Legacy(rgba),
        Color::Special(id) => match id.get() {
            Special::Wide(w) => Resolved::Wide(w),
            Special::Mix(m) => Resolved::Wide(m.resolve(current)),
        },
    }
}

/// `a` to `b` at `p` with `currentcolor` as `current`.
pub(super) fn mix(a: Color, b: Color, current: Rgba, p: f64) -> Color {
    Color::Rgba(mix_resolved(resolve(a, current), resolve(b, current), p))
}

/// The `color` property: `currentcolor` cannot occur (it is the parent's
/// color, already resolved); the exact color of a `color-mix()` or a
/// non-legacy color is in `color_exact`.
pub(super) fn mix_color_property(a: &ComputedStyle, b: &ComputedStyle, p: f64) -> Rgba {
    let of = |s: &ComputedStyle| match s.color_exact.map(ColorId::get) {
        Some(Special::Wide(w)) => Resolved::Wide(w),
        _ => Resolved::Legacy(s.color),
    };
    mix_resolved(of(a), of(b), p)
}

fn mix_resolved(a: Resolved, b: Resolved, p: f64) -> Rgba {
    match (a, b) {
        (Resolved::Legacy(x), Resolved::Legacy(y)) => mix_srgb(x, y, p),
        (x, y) => {
            let wide = |r: Resolved| match r {
                Resolved::Legacy(c) => Wide::from_rgba(c),
                Resolved::Wide(w) => w,
            };
            let percent = [to_f32(100.0 * (1.0 - p)), to_f32(100.0 * p)];
            mix_wide(Space::Oklab, HueMethod::Shorter, wide(x), wide(y), percent).to_rgba()
        }
    }
}

/// Two legacy colors in sRGB with premultiplied alpha, rounded to 8 bits.
fn mix_srgb(a: Rgba, b: Rgba, p: f64) -> Rgba {
    let alpha_a = f64::from(a.a) / 255.0;
    let alpha_b = f64::from(b.a) / 255.0;
    let alpha = (alpha_a + (alpha_b - alpha_a) * p).clamp(0.0, 1.0);
    let channel = |x: u8, y: u8| -> f32 {
        if alpha <= 0.0 {
            return 0.0;
        }
        let pa = f64::from(x) * alpha_a;
        let pb = f64::from(y) * alpha_b;
        to_f32((pa + (pb - pa) * p) / alpha)
    };
    Rgba::from_f32(
        channel(a.r, b.r),
        channel(a.g, b.g),
        channel(a.b, b.b),
        alpha as f32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::color::parse_color;
    use crate::parse::test_util::parse_all;

    fn color(css: &str) -> Color {
        parse_all(css, parse_color).unwrap_or_else(|_| panic!("{css} parses"))
    }

    /// The 8-bit channels of a Chromium value: exact for the legacy rows,
    /// whose Chromium values are 8-bit; the `oklab()` rows are converted to
    /// sRGB with one level of tolerance (ADR 0044 section 12).
    fn close(got: Rgba, want: Rgba, oklab: bool) -> bool {
        let tolerance = u8::from(oklab);
        let d = |x: u8, y: u8| x.abs_diff(y) <= tolerance;
        d(got.r, want.r) && d(got.g, want.g) && d(got.b, want.b) && d(got.a, want.a)
    }

    #[test]
    fn interp_colors() {
        // `interp-colors` at 250 and 500 ms of a linear 1 s animation;
        // the element's `color` is rgb(0, 128, 0).
        let current = Rgba::rgb(0, 128, 0);
        let cases: [(&str, &str, [&str; 2]); 9] = [
            (
                "rgb(255, 0, 0)",
                "rgb(0, 0, 255)",
                ["rgb(191, 0, 64)", "rgb(128, 0, 128)"],
            ),
            (
                "rgb(255, 0, 0)",
                "rgb(0, 0, 254)",
                ["rgb(191, 0, 64)", "rgb(128, 0, 127)"],
            ),
            (
                "rgba(255, 0, 0, 1)",
                "rgba(0, 0, 255, 0)",
                ["rgba(255, 0, 0, 0.75)", "rgba(255, 0, 0, 0.5)"],
            ),
            (
                "transparent",
                "rgb(255, 255, 255)",
                ["rgba(255, 255, 255, 0.25)", "rgba(255, 255, 255, 0.5)"],
            ),
            (
                "hsl(0 100% 50%)",
                "hsl(240 100% 50%)",
                ["rgb(191, 0, 64)", "rgb(128, 0, 128)"],
            ),
            (
                "oklch(0.7 0.1 30)",
                "oklch(0.7 0.1 200)",
                [
                    "oklab(0.7 0.0414596 0.0289495)",
                    "oklab(0.7 -0.00368336 0.00789899)",
                ],
            ),
            (
                "color(display-p3 1 0 0)",
                "rgb(0, 0, 255)",
                [
                    "oklab(0.599426 0.188415 0.0308708)",
                    "oklab(0.550278 0.11479 -0.0832604)",
                ],
            ),
            (
                "rgb(255 0 0 / 50%)",
                "lab(50 40 -20)",
                [
                    "oklab(0.610977 0.183185 0.0537069 / 0.626471)",
                    "oklab(0.599621 0.155315 0.0054796 / 0.75098)",
                ],
            ),
            (
                "currentcolor",
                "rgb(255, 0, 0)",
                ["rgb(64, 96, 0)", "rgb(128, 64, 0)"],
            ),
        ];
        for (a, b, wants) in cases {
            for (p, want) in [0.25, 0.5].iter().zip(wants) {
                let got = mix(color(a), color(b), current, *p).resolve(current);
                let oklab = want.starts_with("oklab(");
                let want = color(want).resolve(current);
                assert!(
                    close(got, want, oklab),
                    "{a} to {b} at {p}: {got:?} != {want:?}"
                );
            }
        }
        // `interp-color-current`: `border-top-color` from `currentcolor`.
        let got = mix(color("currentcolor"), color("rgb(255, 0, 0)"), current, 0.5);
        assert_eq!(got, Color::Rgba(Rgba::rgb(128, 64, 0)));
    }
}
