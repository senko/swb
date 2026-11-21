//! Easing functions (CSS Easing 2, <https://drafts.csswg.org/css-easing-2/>):
//! parsing, the texts of the specified and computed values, and the
//! evaluation of an input progress.
//!
//! The keywords are their `cubic-bezier()` forms; `step-start` and
//! `step-end` compute to `steps()`. `linear()` gets its missing input
//! positions at parse time, so its specified and computed texts list every
//! point with a percentage, as Chromium 148 writes them
//! (`linear(0, 1)` is `linear(0 0%, 1 100%)`). `steps()` takes up to
//! 2^31 − 1 steps; the evaluation of every function is O(1), except
//! `linear()`, which finds its segment by binary search. All math is `f64`.
//!
//! The arguments have Chromium's range (`number-ranges` of
//! `tools/probes/css-animations-3a.json`): literals saturate to the `f32`
//! range, an infinite math function is the largest finite `f64` and NaN is
//! 0 (`cubic-bezier(0, calc(1/0), 1, 1)` is `cubic-bezier(0,
//! 1.79769e+308, 1, 1)`). The count of `steps()` keeps its math function
//! in the specified text (`steps(calc(2.5))`) and computes to the rounded
//! value clamped to 1 to 2^31 − 1, except NaN, which Chromium computes to
//! `steps(0)`. The output of [`Easing::evaluate`] is always finite.
//!
//! `animation-timing-function` uses this module now; feature 5 moves
//! `transition-timing-function` to it.

use std::sync::Arc;

use swb_css::{ComponentValue, ParseError, Parser};

use super::time::{Number, Written, parse_number, saturate_literal};
use crate::cssom::format_number;
use crate::parse::ParseResult;
use crate::parse::length::{CalcKind, parse_context_free_math};

/// The position of the jumps of `steps()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepPosition {
    /// `jump-start`.
    JumpStart,
    /// `jump-end` or `end` (the default).
    End,
    /// `jump-none`.
    JumpNone,
    /// `jump-both`.
    JumpBoth,
    /// `start` (the same jumps as `jump-start`, another text).
    Start,
}

impl StepPosition {
    /// The keyword, `None` for the default `end`.
    fn keyword(self) -> Option<&'static str> {
        match self {
            StepPosition::JumpStart => Some("jump-start"),
            StepPosition::End => None,
            StepPosition::JumpNone => Some("jump-none"),
            StepPosition::JumpBoth => Some("jump-both"),
            StepPosition::Start => Some("start"),
        }
    }

    /// True if the first jump is at the start of the interval.
    fn jumps_at_start(self) -> bool {
        matches!(
            self,
            StepPosition::JumpStart | StepPosition::Start | StepPosition::JumpBoth
        )
    }
}

/// One point of `linear()`: the output and the input progress (a
/// fraction, `50%` is 0.5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearPoint {
    /// The output progress.
    pub output: f64,
    /// The input progress.
    pub input: f64,
}

/// An easing function.
#[derive(Clone, Debug, PartialEq)]
pub enum Easing {
    /// `linear` (the keyword).
    Linear,
    /// `ease`.
    Ease,
    /// `ease-in`.
    EaseIn,
    /// `ease-out`.
    EaseOut,
    /// `ease-in-out`.
    EaseInOut,
    /// `cubic-bezier(x1, y1, x2, y2)`, with `x1` and `x2` in 0 to 1.
    CubicBezier([f64; 4]),
    /// `step-start` (specified only).
    StepStart,
    /// `step-end` (specified only).
    StepEnd,
    /// `steps(count, position)`.
    Steps {
        /// The number of steps, 1 to 2^31 − 1 (2 or more for `jump-none`;
        /// 0 for a math function that is NaN, as in Chromium).
        count: u32,
        /// The position of the jumps.
        position: StepPosition,
        /// The count as written when it is a math function (for the
        /// specified text: `steps(calc(2.5))`).
        math: Option<Number>,
    },
    /// `linear()` with every input position filled in.
    Points(Arc<[LinearPoint]>),
}

/// The largest number of steps (Chromium clamps `steps(2147483648)` to it).
pub const MAX_STEPS: u32 = i32::MAX as u32;

impl Easing {
    /// The control points of the `cubic-bezier()` form of a keyword.
    fn bezier(&self) -> Option<[f64; 4]> {
        Some(match self {
            Easing::Ease => [0.25, 0.1, 0.25, 1.0],
            Easing::EaseIn => [0.42, 0.0, 1.0, 1.0],
            Easing::EaseOut => [0.0, 0.0, 0.58, 1.0],
            Easing::EaseInOut => [0.42, 0.0, 0.58, 1.0],
            Easing::CubicBezier(points) => *points,
            _ => return None,
        })
    }

    /// The computed value: `step-start` and `step-end` are `steps()`, a
    /// math function in the count is its value.
    #[must_use]
    pub fn computed(&self) -> Easing {
        match self {
            Easing::StepStart => Easing::Steps {
                count: 1,
                position: StepPosition::Start,
                math: None,
            },
            Easing::StepEnd => Easing::Steps {
                count: 1,
                position: StepPosition::End,
                math: None,
            },
            Easing::Steps {
                count, position, ..
            } => Easing::Steps {
                count: *count,
                position: *position,
                math: None,
            },
            other => other.clone(),
        }
    }

    /// The text of the specified value (`element.style`).
    pub fn declared_text(&self) -> String {
        self.text(false)
    }

    /// The text of the computed value (`getComputedStyle`; call it on the
    /// [`Easing::computed`] value). Chromium writes the count of `steps()`
    /// with six significant digits here (`steps(1e+06)`).
    pub fn computed_text(&self) -> String {
        self.computed().text(true)
    }

    fn text(&self, computed: bool) -> String {
        match self {
            Easing::Linear => "linear".to_owned(),
            Easing::Ease => "ease".to_owned(),
            Easing::EaseIn => "ease-in".to_owned(),
            Easing::EaseOut => "ease-out".to_owned(),
            Easing::EaseInOut => "ease-in-out".to_owned(),
            Easing::StepStart => "step-start".to_owned(),
            Easing::StepEnd => "step-end".to_owned(),
            Easing::CubicBezier(v) => format!(
                "cubic-bezier({}, {}, {}, {})",
                format_number(v[0]),
                format_number(v[1]),
                format_number(v[2]),
                format_number(v[3])
            ),
            Easing::Steps {
                count,
                position,
                math,
            } => {
                let count = match math {
                    _ if computed => format_number(f64::from(*count)),
                    Some(written) => written.declared(),
                    None => count.to_string(),
                };
                match position.keyword() {
                    Some(k) => format!("steps({count}, {k})"),
                    None => format!("steps({count})"),
                }
            }
            Easing::Points(points) => {
                let items: Vec<String> = points
                    .iter()
                    .map(|p| {
                        format!(
                            "{} {}%",
                            format_number(p.output),
                            format_number(finite(p.input * 100.0))
                        )
                    })
                    .collect();
                format!("linear({})", items.join(", "))
            }
        }
    }

    /// The output progress for `input`. `before` is the before flag of
    /// CSS Easing 2 §4 (only `steps()` reads it). The result is finite
    /// (huge control points or outputs saturate at the largest `f64`).
    pub fn evaluate(&self, input: f64, before: bool) -> f64 {
        if let Some(points) = self.bezier() {
            return finite(cubic_bezier(points, input));
        }
        finite(match self {
            Easing::StepStart => steps(1, StepPosition::Start, input, before),
            Easing::StepEnd => steps(1, StepPosition::End, input, before),
            Easing::Steps {
                count, position, ..
            } => steps(*count, *position, input, before),
            Easing::Points(points) => linear_points(points, input),
            _ => input,
        })
    }

    /// True if the output is a step function of the input (`steps()`):
    /// constant between the step boundaries.
    pub fn is_stepped(&self) -> bool {
        matches!(
            self,
            Easing::StepStart | Easing::StepEnd | Easing::Steps { .. }
        )
    }
}

/// `v` clamped to the finite `f64` range; NaN is 0.
pub(crate) fn finite(v: f64) -> f64 {
    if v.is_nan() {
        0.0
    } else {
        v.clamp(-f64::MAX, f64::MAX)
    }
}

// ----- Parsing -----

/// Consumes an `<easing-function>`.
pub fn parse_easing(p: &mut Parser<'_>) -> ParseResult<Easing> {
    p.try_parse(|p| {
        if let Ok(keyword) = p.try_parse(|p| p.expect_ident().map(easing_keyword)) {
            if let Some(easing) = keyword {
                return Ok(easing);
            }
            return Err(ParseError::Unexpected);
        }
        let (name, mut args) = p.expect_function()?;
        if name.eq_ignore_ascii_case("cubic-bezier") {
            args.parse_entirely(parse_bezier_args)
        } else if name.eq_ignore_ascii_case("steps") {
            args.parse_entirely(parse_steps_args)
        } else if name.eq_ignore_ascii_case("linear") {
            args.parse_entirely(parse_linear_args)
        } else {
            Err(ParseError::Unexpected)
        }
    })
}

/// The easing function of a keyword (ASCII case-insensitive).
pub(crate) fn easing_keyword(ident: &str) -> Option<Easing> {
    Some(match ident.to_ascii_lowercase().as_str() {
        "linear" => Easing::Linear,
        "ease" => Easing::Ease,
        "ease-in" => Easing::EaseIn,
        "ease-out" => Easing::EaseOut,
        "ease-in-out" => Easing::EaseInOut,
        "step-start" => Easing::StepStart,
        "step-end" => Easing::StepEnd,
        _ => return None,
    })
}

/// `x1, y1, x2, y2` of `cubic-bezier()`.
fn parse_bezier_args(p: &mut Parser<'_>) -> ParseResult<Easing> {
    let mut v = [0.0f64; 4];
    for (i, slot) in v.iter_mut().enumerate() {
        if i > 0 {
            p.expect_comma()?;
        }
        *slot = parse_number(p)?.value;
    }
    let in_range = |x: f64| (0.0..=1.0).contains(&x);
    if in_range(v[0]) && in_range(v[2]) {
        Ok(Easing::CubicBezier(v))
    } else {
        Err(ParseError::Invalid)
    }
}

/// `<integer [1,∞]> [, <step-position>]?` of `steps()`.
fn parse_steps_args(p: &mut Parser<'_>) -> ParseResult<Easing> {
    let (count, math) = p.try_parse(parse_step_count)?;
    let position = if p.expect_comma().is_ok() {
        p.expect_one_of(&[
            ("jump-start", StepPosition::JumpStart),
            ("jump-end", StepPosition::End),
            ("jump-none", StepPosition::JumpNone),
            ("jump-both", StepPosition::JumpBoth),
            ("start", StepPosition::Start),
            ("end", StepPosition::End),
        ])?
    } else {
        StepPosition::End
    };
    let jump_none = position == StepPosition::JumpNone;
    // Chromium checks a literal count, and the rounded count of a math
    // function only for `jump-none` (`steps(calc(-1))` is valid and computes
    // to `steps(1)`; `steps(calc(1), jump-none)` is invalid).
    let least = if jump_none { 2 } else { 1 };
    if count < least && (math.is_none() || jump_none) {
        return Err(ParseError::Invalid);
    }
    let count = if math.is_some_and(Number::is_nan) {
        0
    } else {
        count.clamp(least, i64::from(MAX_STEPS)) as u32
    };
    Ok(Easing::Steps {
        count,
        position,
        math,
    })
}

/// The count of `steps()`: an integer token, or a math function rounded to
/// the nearest integer (halves toward +∞) and clamped to the `i64` range,
/// with the math function as written.
fn parse_step_count(p: &mut Parser<'_>) -> ParseResult<(i64, Option<Number>)> {
    if let Some(ComponentValue::Number(n)) = p.peek() {
        let Some(int) = n.int_value else {
            return Err(ParseError::Invalid);
        };
        p.next();
        return Ok((i64::from(int), None));
    }
    let number = parse_number(p)?;
    if !number.is_math() {
        return Err(ParseError::Invalid);
    }
    let rounded = (number.value + 0.5).floor();
    let clamped = rounded.clamp(i64::MIN as f64, i64::MAX as f64) as i64;
    Ok((clamped, Some(number)))
}

/// The items of `linear()`: `[<number> && <percentage>{0,2}]#`, at least
/// two points. The missing inputs are filled in (CSS Easing 2 §2.1,
/// <https://drafts.csswg.org/css-easing-2/#linear-easing-function-parsing>).
fn parse_linear_args(p: &mut Parser<'_>) -> ParseResult<Easing> {
    let items = p.parse_comma_separated(parse_linear_item)?;
    let mut points: Vec<(f64, Option<f64>)> = Vec::with_capacity(items.len());
    for (output, inputs) in items {
        match inputs {
            [None, _] => points.push((output, None)),
            [Some(a), None] => points.push((output, Some(a))),
            [Some(a), Some(b)] => {
                points.push((output, Some(a)));
                points.push((output, Some(b)));
            }
        }
    }
    if points.len() < 2 {
        return Err(ParseError::Invalid);
    }
    Ok(Easing::Points(Arc::from(fill_inputs(&points))))
}

/// One item of `linear()`: the output and up to two input percentages (as
/// fractions), in any order.
fn parse_linear_item(p: &mut Parser<'_>) -> ParseResult<(f64, [Option<f64>; 2])> {
    let mut output = None;
    let mut inputs = [None, None];
    let mut count = 0;
    while !p.is_exhausted() {
        if output.is_none()
            && let Ok(n) = parse_number(p)
        {
            output = Some(n.value);
            continue;
        }
        let pct = parse_percentage(p)?;
        let slot = inputs.get_mut(count).ok_or(ParseError::Invalid)?;
        *slot = Some(pct);
        count += 1;
    }
    Ok((output.ok_or(ParseError::Invalid)?, inputs))
}

/// A `<percentage>` (also a math function of percentages) as a fraction,
/// with the range of numbers of the module comment (`calc(1%/0)` is
/// `1.79769e+308%`).
fn parse_percentage(p: &mut Parser<'_>) -> ParseResult<f64> {
    if let Some(ComponentValue::Percentage(n)) = p.peek() {
        let v = saturate_literal(n.value_f64) / 100.0;
        p.next();
        return Ok(v);
    }
    let (fraction, kind) = parse_context_free_math(p)?;
    if kind != CalcKind::Percentage {
        return Err(ParseError::Invalid);
    }
    let (_, percent) = Written::math(fraction * 100.0);
    Ok(percent / 100.0)
}

/// Fills the missing inputs of `linear()`: the first is 0 and the last 1
/// when missing; an input below the largest one before it becomes that
/// one; a run of missing inputs is spread evenly between its neighbours.
fn fill_inputs(points: &[(f64, Option<f64>)]) -> Vec<LinearPoint> {
    let last = points.len().saturating_sub(1);
    let mut inputs: Vec<Option<f64>> = points.iter().map(|(_, i)| *i).collect();
    if let Some(first) = inputs.first_mut()
        && first.is_none()
    {
        *first = Some(0.0);
    }
    if let Some(end) = inputs.get_mut(last)
        && end.is_none()
    {
        *end = Some(1.0);
    }
    let mut largest = f64::NEG_INFINITY;
    for input in inputs.iter_mut().flatten() {
        if *input < largest {
            *input = largest;
        }
        largest = largest.max(*input);
    }
    let mut i = 0;
    while i < inputs.len() {
        if inputs.get(i).is_some_and(Option::is_some) {
            i += 1;
            continue;
        }
        // A run of missing inputs from `i` to `j - 1`; the ends are set.
        let start = i.saturating_sub(1);
        let mut j = i;
        while inputs.get(j).is_some_and(Option::is_none) {
            j += 1;
        }
        let a = inputs.get(start).copied().flatten().unwrap_or(0.0);
        let b = inputs.get(j).copied().flatten().unwrap_or(1.0);
        let steps = (j - start) as f64;
        for (k, slot) in inputs.iter_mut().enumerate().take(j).skip(i) {
            *slot = Some(a + (b - a) * (k - start) as f64 / steps);
        }
        i = j;
    }
    points
        .iter()
        .zip(inputs)
        .map(|((output, _), input)| LinearPoint {
            output: *output,
            input: input.unwrap_or(0.0),
        })
        .collect()
}

// ----- Evaluation -----

/// `cubic-bezier()` at `x`: solve x(t) = `x` (Newton's method from t = x,
/// bisection on [0, 1] when it does not converge), then y(t). Outside 0 to
/// 1 the curve continues along its tangent at the nearer end point.
/// <https://drafts.csswg.org/css-easing-2/#cubic-bezier-algo>
fn cubic_bezier([x1, y1, x2, y2]: [f64; 4], x: f64) -> f64 {
    /// The precision of the solution of x(t) = x.
    const EPSILON: f64 = 1e-9;
    if x < 0.0 {
        let slope = if x1 > 0.0 {
            y1 / x1
        } else if y1 == 0.0 && x2 > 0.0 {
            y2 / x2
        } else {
            0.0
        };
        return slope * x;
    }
    if x > 1.0 {
        let slope = if x2 < 1.0 {
            (y2 - 1.0) / (x2 - 1.0)
        } else if y2 == 1.0 && x1 < 1.0 {
            (y1 - 1.0) / (x1 - 1.0)
        } else {
            0.0
        };
        return 1.0 + slope * (x - 1.0);
    }
    // The polynomial coefficients of x(t) and y(t).
    let cx = 3.0 * x1;
    let bx = 3.0 * (x2 - x1) - cx;
    let ax = 1.0 - cx - bx;
    let cy = 3.0 * y1;
    let by = 3.0 * (y2 - y1) - cy;
    let ay = 1.0 - cy - by;
    let curve_x = |t: f64| ((ax * t + bx) * t + cx) * t;
    let slope_x = |t: f64| (3.0 * ax * t + 2.0 * bx) * t + cx;
    let curve_y = |t: f64| ((ay * t + by) * t + cy) * t;
    let mut t = x;
    for _ in 0..8 {
        let error = curve_x(t) - x;
        if error.abs() < EPSILON {
            return curve_y(t);
        }
        let d = slope_x(t);
        if d.abs() < 1e-12 {
            break;
        }
        t -= error / d;
    }
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    t = x;
    for _ in 0..64 {
        let value = curve_x(t);
        if (value - x).abs() < EPSILON {
            break;
        }
        if value < x {
            lo = t;
        } else {
            hi = t;
        }
        t = f64::midpoint(lo, hi);
    }
    curve_y(t)
}

/// `steps()` at `input` with the before flag.
/// <https://drafts.csswg.org/css-easing-2/#step-easing-algo>
fn steps(count: u32, position: StepPosition, input: f64, before: bool) -> f64 {
    let n = f64::from(count);
    let scaled = input * n;
    let mut step = scaled.floor();
    if position.jumps_at_start() {
        step += 1.0;
    }
    if before && scaled.fract() == 0.0 {
        step -= 1.0;
    }
    if input >= 0.0 && step < 0.0 {
        step = 0.0;
    }
    let jumps = match position {
        StepPosition::JumpStart | StepPosition::Start | StepPosition::End => n,
        StepPosition::JumpNone => n - 1.0,
        StepPosition::JumpBoth => n + 1.0,
    };
    if input <= 1.0 && step > jumps {
        step = jumps;
    }
    step / jumps
}

/// `linear()` at `input`: the segment of the last point at or before the
/// input (the first or last segment beyond the ends, extended).
/// <https://drafts.csswg.org/css-easing-2/#linear-easing-function-output>
fn linear_points(points: &[LinearPoint], input: f64) -> f64 {
    if points.len() < 2 {
        return input;
    }
    // The number of points with an input at or before `input`.
    let count = points.partition_point(|p| p.input <= input);
    let a = count.saturating_sub(1).min(points.len() - 2);
    let (Some(pa), Some(pb)) = (points.get(a), points.get(a + 1)) else {
        return input;
    };
    if pa.input == pb.input {
        return pb.output;
    }
    let t = (input - pa.input) / (pb.input - pa.input);
    pa.output + t * (pb.output - pa.output)
}

#[cfg(test)]
mod tests;
