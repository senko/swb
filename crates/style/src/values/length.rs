//! Lengths and percentages: specified values (with units and `calc()`),
//! and computed values (pixels and percentages).
//!
//! <https://www.w3.org/TR/css-values-4/#lengths>

use std::sync::Arc;

/// A length unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LengthUnit {
    /// CSS pixels.
    Px,
    /// The element's font size.
    Em,
    /// The root element's font size.
    Rem,
    /// The x-height of the element's font (approximated as 0.5em).
    Ex,
    /// The advance of "0" in the element's font (approximated as 0.5em).
    Ch,
    /// 1% of the viewport width.
    Vw,
    /// 1% of the viewport height.
    Vh,
    /// 1% of the smaller viewport dimension.
    Vmin,
    /// 1% of the larger viewport dimension.
    Vmax,
    /// Centimeters.
    Cm,
    /// Millimeters.
    Mm,
    /// Quarter-millimeters.
    Q,
    /// Inches.
    In,
    /// Points (1/72 inch).
    Pt,
    /// Picas (12 points).
    Pc,
}

impl LengthUnit {
    /// Parses a unit name (ASCII case-insensitive).
    ///
    /// The small, large and dynamic viewport units map to the plain ones
    /// (swb has no dynamic browser UI). `vi`/`vb` map to `vw`/`vh`
    /// (horizontal writing mode only). Container query units map to the
    /// viewport units: swb has no query containers, and without a container
    /// the units use the small viewport size.
    /// <https://www.w3.org/TR/css-contain-3/#container-lengths>
    pub fn from_name(unit: &str) -> Option<Self> {
        let u = match unit.to_ascii_lowercase().as_str() {
            "px" => Self::Px,
            "em" => Self::Em,
            "rem" => Self::Rem,
            "ex" => Self::Ex,
            "ch" => Self::Ch,
            "vw" | "svw" | "lvw" | "dvw" | "vi" | "svi" | "lvi" | "dvi" | "cqw" | "cqi" => Self::Vw,
            "vh" | "svh" | "lvh" | "dvh" | "vb" | "svb" | "lvb" | "dvb" | "cqh" | "cqb" => Self::Vh,
            "vmin" | "svmin" | "lvmin" | "dvmin" | "cqmin" => Self::Vmin,
            "vmax" | "svmax" | "lvmax" | "dvmax" | "cqmax" => Self::Vmax,
            "cm" => Self::Cm,
            "mm" => Self::Mm,
            "q" => Self::Q,
            "in" => Self::In,
            "pt" => Self::Pt,
            "pc" => Self::Pc,
            _ => return None,
        };
        Some(u)
    }
}

/// Values that relative lengths depend on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LengthContext {
    /// The element's computed font size, in px. For `font-size` itself this
    /// is the parent's font size.
    pub font_size: f32,
    /// The root element's computed font size, in px.
    pub root_font_size: f32,
    /// Viewport width in CSS px.
    pub viewport_width: f32,
    /// Viewport height in CSS px.
    pub viewport_height: f32,
}

impl LengthContext {
    /// A context with default values: 16px fonts, 800x600 viewport.
    pub const DEFAULT: LengthContext = LengthContext {
        font_size: 16.0,
        root_font_size: 16.0,
        viewport_width: 800.0,
        viewport_height: 600.0,
    };
}

/// A specified length: a number and a unit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Length {
    /// The number.
    pub value: f32,
    /// The unit.
    pub unit: LengthUnit,
}

impl Length {
    /// The largest magnitude of a computed or used length, in px. Larger
    /// values are clamped, so that layout never sees infinite values (sums
    /// of clamped values stay finite). Chromium's `LayoutUnit` has the same
    /// range (about 2^25 px).
    pub const MAX_PX: f32 = 33_554_431.0;

    /// Clamps a length in px to `[-MAX_PX, MAX_PX]`; NaN becomes 0.
    pub fn clamp_px(v: f32) -> f32 {
        if v.is_nan() {
            0.0
        } else {
            v.clamp(-Self::MAX_PX, Self::MAX_PX)
        }
    }

    /// A length in px.
    pub const fn px(value: f32) -> Self {
        Length {
            value,
            unit: LengthUnit::Px,
        }
    }

    /// Converts to CSS pixels (clamped, see [`Length::MAX_PX`]).
    pub fn to_px(self, ctx: &LengthContext) -> f32 {
        Self::clamp_px(self.unscaled_px(ctx))
    }

    fn unscaled_px(self, ctx: &LengthContext) -> f32 {
        let v = self.value;
        match self.unit {
            LengthUnit::Px => v,
            LengthUnit::Em => v * ctx.font_size,
            LengthUnit::Rem => v * ctx.root_font_size,
            LengthUnit::Ex | LengthUnit::Ch => v * ctx.font_size * 0.5,
            LengthUnit::Vw => v * ctx.viewport_width / 100.0,
            LengthUnit::Vh => v * ctx.viewport_height / 100.0,
            LengthUnit::Vmin => v * ctx.viewport_width.min(ctx.viewport_height) / 100.0,
            LengthUnit::Vmax => v * ctx.viewport_width.max(ctx.viewport_height) / 100.0,
            LengthUnit::Cm => v * 96.0 / 2.54,
            LengthUnit::Mm => v * 96.0 / 25.4,
            LengthUnit::Q => v * 96.0 / 101.6,
            LengthUnit::In => v * 96.0,
            LengthUnit::Pt => v * 96.0 / 72.0,
            LengthUnit::Pc => v * 16.0,
        }
    }
}

/// A node of a specified `calc()` expression.
#[derive(Clone, Debug, PartialEq)]
pub enum CalcNode {
    /// A length.
    Length(Length),
    /// A percentage, as a fraction (50% is 0.5).
    Percentage(f32),
    /// A plain number (only valid as a factor).
    Number(f32),
    /// The sum of the children.
    Sum(Vec<CalcNode>),
    /// The product of the two children (one of them is a number).
    Product(Box<CalcNode>, Box<CalcNode>),
    /// The first child divided by the second (a number).
    Quotient(Box<CalcNode>, Box<CalcNode>),
    /// The smallest child.
    Min(Vec<CalcNode>),
    /// The largest child.
    Max(Vec<CalcNode>),
    /// `clamp(min, value, max)`.
    Clamp(Box<CalcNode>, Box<CalcNode>, Box<CalcNode>),
}

impl CalcNode {
    /// Computes this expression: lengths become px; percentages stay.
    pub fn compute(&self, ctx: &LengthContext) -> ComputedCalc {
        match self {
            CalcNode::Length(l) => ComputedCalc::Px(l.to_px(ctx)),
            CalcNode::Percentage(p) => ComputedCalc::Percent(*p),
            CalcNode::Number(n) => ComputedCalc::Number(*n),
            CalcNode::Sum(c) => ComputedCalc::Sum(c.iter().map(|n| n.compute(ctx)).collect()),
            CalcNode::Product(a, b) => {
                ComputedCalc::Product(Box::new(a.compute(ctx)), Box::new(b.compute(ctx)))
            }
            CalcNode::Quotient(a, b) => {
                ComputedCalc::Quotient(Box::new(a.compute(ctx)), Box::new(b.compute(ctx)))
            }
            CalcNode::Min(c) => ComputedCalc::Min(c.iter().map(|n| n.compute(ctx)).collect()),
            CalcNode::Max(c) => ComputedCalc::Max(c.iter().map(|n| n.compute(ctx)).collect()),
            CalcNode::Clamp(a, b, c) => ComputedCalc::Clamp(
                Box::new(a.compute(ctx)),
                Box::new(b.compute(ctx)),
                Box::new(c.compute(ctx)),
            ),
        }
    }
}

/// A computed `calc()` expression: lengths are in px, percentages are kept
/// until layout supplies the basis.
#[derive(Clone, Debug, PartialEq)]
pub enum ComputedCalc {
    /// A length in px.
    Px(f32),
    /// A percentage as a fraction.
    Percent(f32),
    /// A plain number.
    Number(f32),
    /// The sum of the children.
    Sum(Vec<ComputedCalc>),
    /// The product of the two children.
    Product(Box<ComputedCalc>, Box<ComputedCalc>),
    /// The first child divided by the second.
    Quotient(Box<ComputedCalc>, Box<ComputedCalc>),
    /// The smallest child.
    Min(Vec<ComputedCalc>),
    /// The largest child.
    Max(Vec<ComputedCalc>),
    /// `clamp(min, value, max)`.
    Clamp(Box<ComputedCalc>, Box<ComputedCalc>, Box<ComputedCalc>),
}

impl ComputedCalc {
    /// Evaluates the expression with `basis` as the 100% reference.
    pub fn resolve(&self, basis: f32) -> f32 {
        match self {
            ComputedCalc::Px(v) | ComputedCalc::Number(v) => *v,
            ComputedCalc::Percent(p) => p * basis,
            ComputedCalc::Sum(c) => c.iter().map(|n| n.resolve(basis)).sum(),
            ComputedCalc::Product(a, b) => a.resolve(basis) * b.resolve(basis),
            ComputedCalc::Quotient(a, b) => {
                let d = b.resolve(basis);
                if d == 0.0 { 0.0 } else { a.resolve(basis) / d }
            }
            ComputedCalc::Min(c) => c
                .iter()
                .map(|n| n.resolve(basis))
                .fold(f32::INFINITY, f32::min),
            ComputedCalc::Max(c) => c
                .iter()
                .map(|n| n.resolve(basis))
                .fold(f32::NEG_INFINITY, f32::max),
            ComputedCalc::Clamp(lo, v, hi) => {
                let lo = lo.resolve(basis);
                v.resolve(basis).min(hi.resolve(basis)).max(lo)
            }
        }
    }

    /// True if the expression contains a percentage.
    pub fn has_percentage(&self) -> bool {
        match self {
            ComputedCalc::Px(_) | ComputedCalc::Number(_) => false,
            ComputedCalc::Percent(_) => true,
            ComputedCalc::Sum(c) | ComputedCalc::Min(c) | ComputedCalc::Max(c) => {
                c.iter().any(ComputedCalc::has_percentage)
            }
            ComputedCalc::Product(a, b) | ComputedCalc::Quotient(a, b) => {
                a.has_percentage() || b.has_percentage()
            }
            ComputedCalc::Clamp(a, b, c) => {
                a.has_percentage() || b.has_percentage() || c.has_percentage()
            }
        }
    }
}

/// A specified `<length-percentage>`.
#[derive(Clone, Debug, PartialEq)]
pub enum SpecifiedLengthPercentage {
    /// A length.
    Length(Length),
    /// A percentage as a fraction.
    Percentage(f32),
    /// A `calc()`-like expression.
    Calc(Box<CalcNode>),
}

impl SpecifiedLengthPercentage {
    /// Zero pixels.
    pub const ZERO: Self = SpecifiedLengthPercentage::Length(Length::px(0.0));

    /// Computes the value.
    pub fn compute(&self, ctx: &LengthContext) -> LengthPercentage {
        match self {
            Self::Length(l) => LengthPercentage::Px(l.to_px(ctx)),
            Self::Percentage(p) => LengthPercentage::Percent(*p),
            Self::Calc(c) => match c.compute(ctx) {
                ComputedCalc::Px(v) => LengthPercentage::Px(v),
                ComputedCalc::Percent(p) => LengthPercentage::Percent(p),
                other if !other.has_percentage() => {
                    LengthPercentage::Px(Length::clamp_px(other.resolve(0.0)))
                }
                other => LengthPercentage::Calc(Arc::new(other)),
            },
        }
    }
}

/// A computed `<length-percentage>`.
#[derive(Clone, Debug, PartialEq)]
pub enum LengthPercentage {
    /// A length in px.
    Px(f32),
    /// A percentage as a fraction.
    Percent(f32),
    /// An expression that mixes lengths and percentages.
    Calc(Arc<ComputedCalc>),
}

impl Default for LengthPercentage {
    fn default() -> Self {
        LengthPercentage::ZERO
    }
}

impl LengthPercentage {
    /// Zero pixels.
    pub const ZERO: LengthPercentage = LengthPercentage::Px(0.0);

    /// The used value, with `basis` as the 100% reference (clamped, see
    /// [`Length::MAX_PX`]).
    pub fn resolve(&self, basis: f32) -> f32 {
        Length::clamp_px(match self {
            LengthPercentage::Px(v) => *v,
            LengthPercentage::Percent(p) => p * basis,
            LengthPercentage::Calc(c) => c.resolve(basis),
        })
    }

    /// The used value when the basis may be unknown (indefinite). Returns
    /// `None` if the value depends on the unknown basis.
    pub fn resolve_opt(&self, basis: Option<f32>) -> Option<f32> {
        match (self, basis) {
            (LengthPercentage::Px(v), _) => Some(Length::clamp_px(*v)),
            (_, Some(b)) => Some(self.resolve(b)),
            (_, None) => None,
        }
    }

    /// True if the value contains a percentage.
    pub fn has_percentage(&self) -> bool {
        match self {
            LengthPercentage::Px(_) => false,
            LengthPercentage::Percent(_) => true,
            LengthPercentage::Calc(c) => c.has_percentage(),
        }
    }

    /// True if this is exactly zero pixels.
    pub fn is_zero(&self) -> bool {
        matches!(self, LengthPercentage::Px(v) if *v == 0.0)
    }
}

/// A computed `<length-percentage> | auto`.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum LengthPercentageOrAuto {
    /// `auto`.
    #[default]
    Auto,
    /// A length or percentage.
    LengthPercentage(LengthPercentage),
}

impl LengthPercentageOrAuto {
    /// Zero pixels.
    pub const ZERO: Self = Self::LengthPercentage(LengthPercentage::ZERO);

    /// A px value.
    #[cfg(test)]
    pub(crate) fn px(v: f32) -> Self {
        Self::LengthPercentage(LengthPercentage::Px(v))
    }

    /// True if `auto`.
    pub fn is_auto(&self) -> bool {
        matches!(self, Self::Auto)
    }

    /// The length-percentage, or `None` for `auto`.
    pub fn non_auto(&self) -> Option<&LengthPercentage> {
        match self {
            Self::Auto => None,
            Self::LengthPercentage(lp) => Some(lp),
        }
    }

    /// The used value, with `auto` as `None`.
    pub fn resolve(&self, basis: f32) -> Option<f32> {
        self.non_auto().map(|lp| lp.resolve(basis))
    }
}

/// A computed `width`/`height` value.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Size {
    /// `auto`.
    #[default]
    Auto,
    /// A length or percentage.
    LengthPercentage(LengthPercentage),
    /// `min-content`.
    MinContent,
    /// `max-content`.
    MaxContent,
    /// `fit-content`, `fit-content(<length-percentage>)`.
    FitContent(Option<LengthPercentage>),
}

impl Size {
    /// The length-percentage, if this is one.
    pub fn as_length_percentage(&self) -> Option<&LengthPercentage> {
        match self {
            Size::LengthPercentage(lp) => Some(lp),
            _ => None,
        }
    }

    /// True if `auto`.
    pub fn is_auto(&self) -> bool {
        matches!(self, Size::Auto)
    }
}

/// A computed `max-width`/`max-height` value.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum MaxSize {
    /// `none`.
    #[default]
    None,
    /// A length or percentage.
    LengthPercentage(LengthPercentage),
    /// `min-content`.
    MinContent,
    /// `max-content`.
    MaxContent,
    /// `fit-content`.
    FitContent(Option<LengthPercentage>),
}

impl MaxSize {
    /// The length-percentage, if this is one.
    pub fn as_length_percentage(&self) -> Option<&LengthPercentage> {
        match self {
            MaxSize::LengthPercentage(lp) => Some(lp),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_units() {
        let ctx = LengthContext::DEFAULT;
        assert_eq!(
            Length {
                value: 1.0,
                unit: LengthUnit::In
            }
            .to_px(&ctx),
            96.0
        );
        assert_eq!(
            Length {
                value: 12.0,
                unit: LengthUnit::Pt
            }
            .to_px(&ctx),
            16.0
        );
        assert!(
            (Length {
                value: 2.54,
                unit: LengthUnit::Cm
            }
            .to_px(&ctx)
                - 96.0)
                .abs()
                < 1e-3
        );
    }

    #[test]
    fn relative_units() {
        let ctx = LengthContext {
            font_size: 20.0,
            root_font_size: 10.0,
            viewport_width: 1000.0,
            viewport_height: 500.0,
        };
        assert_eq!(
            Length {
                value: 2.0,
                unit: LengthUnit::Em
            }
            .to_px(&ctx),
            40.0
        );
        assert_eq!(
            Length {
                value: 2.0,
                unit: LengthUnit::Rem
            }
            .to_px(&ctx),
            20.0
        );
        assert_eq!(
            Length {
                value: 10.0,
                unit: LengthUnit::Vw
            }
            .to_px(&ctx),
            100.0
        );
        assert_eq!(
            Length {
                value: 10.0,
                unit: LengthUnit::Vmin
            }
            .to_px(&ctx),
            50.0
        );
    }

    #[test]
    fn calc_resolution() {
        // calc(100% - 2em) with 16px font.
        let calc = CalcNode::Sum(vec![
            CalcNode::Percentage(1.0),
            CalcNode::Product(
                Box::new(CalcNode::Number(-1.0)),
                Box::new(CalcNode::Length(Length {
                    value: 2.0,
                    unit: LengthUnit::Em,
                })),
            ),
        ]);
        let lp = SpecifiedLengthPercentage::Calc(Box::new(calc)).compute(&LengthContext::DEFAULT);
        assert!(lp.has_percentage());
        assert_eq!(lp.resolve(200.0), 168.0);

        // calc(10px + 1em) folds to a plain length.
        let calc = CalcNode::Sum(vec![
            CalcNode::Length(Length::px(10.0)),
            CalcNode::Length(Length {
                value: 1.0,
                unit: LengthUnit::Em,
            }),
        ]);
        let lp = SpecifiedLengthPercentage::Calc(Box::new(calc)).compute(&LengthContext::DEFAULT);
        assert_eq!(lp, LengthPercentage::Px(26.0));
    }

    #[test]
    fn min_max_clamp() {
        let c = ComputedCalc::Clamp(
            Box::new(ComputedCalc::Px(10.0)),
            Box::new(ComputedCalc::Percent(0.5)),
            Box::new(ComputedCalc::Px(100.0)),
        );
        assert_eq!(c.resolve(10.0), 10.0);
        assert_eq!(c.resolve(100.0), 50.0);
        assert_eq!(c.resolve(1000.0), 100.0);
        let m = ComputedCalc::Min(vec![ComputedCalc::Px(30.0), ComputedCalc::Percent(0.1)]);
        assert_eq!(m.resolve(100.0), 10.0);
    }

    #[test]
    fn huge_lengths_are_clamped() {
        let ctx = LengthContext::DEFAULT;
        assert_eq!(Length::px(f32::MAX).to_px(&ctx), Length::MAX_PX);
        let em = Length {
            value: f32::MAX,
            unit: LengthUnit::Em,
        };
        assert_eq!(em.to_px(&ctx), Length::MAX_PX);
        assert_eq!(
            LengthPercentage::Percent(1.0).resolve(f32::INFINITY),
            Length::MAX_PX
        );
        assert_eq!(
            LengthPercentage::Px(-f32::MAX).resolve(0.0),
            -Length::MAX_PX
        );
        let sum = ComputedCalc::Sum(vec![ComputedCalc::Px(f32::MAX), ComputedCalc::Px(f32::MAX)]);
        let lp = LengthPercentage::Calc(Arc::new(sum));
        assert_eq!(lp.resolve(0.0), Length::MAX_PX);
        assert_eq!(Length::clamp_px(f32::NAN), 0.0);
    }
}
