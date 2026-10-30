//! A `<transform-list>` as text, for `DOMMatrix` ("parse a string into an
//! abstract matrix", Geometry Interfaces Module Level 1 §6.1,
//! <https://drafts.fxtf.org/geometry/#parse-a-string-into-an-abstract-matrix>):
//! the functions of CSS Transforms 1 and 2 with all their 3D forms, with
//! lengths in px and angles in degrees. As in Chromium (measured), the
//! numbers of `matrix()` and `matrix3d()`, the axes and the angles keep the
//! `f64` precision of the text, and lengths and scale factors are `f32`.
//!
//! The cascade has its own, smaller parser (`properties/transform.rs`: 2D
//! only, `f32`, relative lengths kept for layout). This one has no layout
//! context, so every length must be absolute: a relative length or a
//! percentage is [`TransformListError::Unresolvable`], as in Chromium
//! (measured, `tools/probes/web-geometry-matrix.json`).
//!
//! Departures: `calc()` and the other math functions are not accepted as
//! arguments; a text of more than 1 MiB is a syntax error.

use swb_css::{ComponentValue, Parser, parse_component_values};

/// One function of a transform list.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TransformOp {
    /// `matrix(a, b, c, d, e, f)`: a 2D matrix.
    Matrix([f64; 6]),
    /// `matrix3d(...)`: the 16 values in column-major order.
    Matrix3d([f64; 16]),
    /// `translate()`, `translateX()`, `translateY()`, `translateZ()`,
    /// `translate3d()`, in px.
    Translate {
        /// The offsets.
        x: f64,
        /// See `x`.
        y: f64,
        /// See `x`.
        z: f64,
        /// True for a 3D function (`translateZ()`, `translate3d()`).
        three_d: bool,
    },
    /// `scale()`, `scaleX()`, `scaleY()`, `scaleZ()`, `scale3d()`.
    Scale {
        /// The factors.
        x: f64,
        /// See `x`.
        y: f64,
        /// See `x`.
        z: f64,
        /// True for a 3D function (`scaleZ()`, `scale3d()`).
        three_d: bool,
    },
    /// `rotate()`, `rotateX()`, `rotateY()`, `rotateZ()`, `rotate3d()`:
    /// the rotation about the axis (x, y, z), not normalized.
    Rotate {
        /// The axis.
        x: f64,
        /// See `x`.
        y: f64,
        /// See `x`.
        z: f64,
        /// The angle in degrees.
        degrees: f64,
        /// True for a 3D function (all but `rotate()` and `rotateZ()`).
        three_d: bool,
    },
    /// `skew()`, `skewX()`, `skewY()`: the angles in degrees.
    Skew {
        /// The angle along the x axis.
        x: f64,
        /// The angle along the y axis.
        y: f64,
    },
    /// `perspective()`: the distance in px (at least 1).
    Perspective(f64),
}

/// Why a string is not a transform list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransformListError {
    /// The text is not a `<transform-list>`.
    Syntax,
    /// A length is relative (`em`, `%`, `vw`, ...): the value needs a
    /// context that the string does not have.
    Unresolvable,
}

/// Parses the text of a transform list. `none` is the empty list; the empty
/// text is for the caller (it is the identity there). The functions are
/// applied in order, each after the ones before it.
pub fn parse_transform_list(
    text: &str,
) -> std::result::Result<Vec<TransformOp>, TransformListError> {
    if text.len() > MAX_TEXT_BYTES {
        return Err(TransformListError::Syntax);
    }
    let tokens = parse_component_values(text);
    let mut p = Parser::new(&tokens);
    if p.expect_ident_matching("none").is_ok() {
        return if p.is_exhausted() {
            Ok(Vec::new())
        } else {
            Err(TransformListError::Syntax)
        };
    }
    let mut ops = Vec::new();
    while !p.is_exhausted() {
        ops.push(function(&mut p)?);
    }
    if ops.is_empty() {
        return Err(TransformListError::Syntax);
    }
    Ok(ops)
}

/// The longest text that is parsed (1 MiB; about 130,000 functions): the
/// tokens of a longer one would cost more memory than the string.
/// Chromium has no such limit.
const MAX_TEXT_BYTES: usize = 1 << 20;

type Result<T> = std::result::Result<T, TransformListError>;

/// One `<transform-function>`.
#[allow(clippy::too_many_lines)] // One arm per function name.
fn function(p: &mut Parser<'_>) -> Result<TransformOp> {
    let (name, mut args) = p
        .expect_function()
        .map_err(|_| TransformListError::Syntax)?;
    let name = name.to_ascii_lowercase();
    let values = split_arguments(&mut args)?;
    let a = Arguments(&values);
    let op = match name.as_str() {
        "matrix" => {
            let v = a.numbers::<6>()?;
            TransformOp::Matrix(v)
        }
        "matrix3d" => TransformOp::Matrix3d(a.numbers::<16>()?),
        "translate" => {
            a.count(1..=2)?;
            translate(a.length(0)?, a.optional_length(1, 0.0)?, 0.0, false)
        }
        "translatex" => {
            a.count(1..=1)?;
            translate(a.length(0)?, 0.0, 0.0, false)
        }
        "translatey" => {
            a.count(1..=1)?;
            translate(0.0, a.length(0)?, 0.0, false)
        }
        "translatez" => {
            a.count(1..=1)?;
            translate(0.0, 0.0, a.length(0)?, true)
        }
        "translate3d" => {
            a.count(3..=3)?;
            translate(a.length(0)?, a.length(1)?, a.length(2)?, true)
        }
        "scale" => {
            a.count(1..=2)?;
            let x = a.factor(0)?;
            let y = if a.len() > 1 { a.factor(1)? } else { x };
            scale(x, y, 1.0, false)
        }
        "scalex" => {
            a.count(1..=1)?;
            scale(a.factor(0)?, 1.0, 1.0, false)
        }
        "scaley" => {
            a.count(1..=1)?;
            scale(1.0, a.factor(0)?, 1.0, false)
        }
        "scalez" => {
            a.count(1..=1)?;
            scale(1.0, 1.0, a.factor(0)?, true)
        }
        "scale3d" => {
            a.count(3..=3)?;
            scale(a.factor(0)?, a.factor(1)?, a.factor(2)?, true)
        }
        "rotate" | "rotatez" => {
            a.count(1..=1)?;
            rotate(0.0, 0.0, 1.0, a.angle(0)?, false)
        }
        "rotatex" => {
            a.count(1..=1)?;
            rotate(1.0, 0.0, 0.0, a.angle(0)?, true)
        }
        "rotatey" => {
            a.count(1..=1)?;
            rotate(0.0, 1.0, 0.0, a.angle(0)?, true)
        }
        "rotate3d" => {
            a.count(4..=4)?;
            rotate(a.number(0)?, a.number(1)?, a.number(2)?, a.angle(3)?, true)
        }
        "skew" => {
            a.count(1..=2)?;
            let y = if a.len() > 1 { a.angle(1)? } else { 0.0 };
            TransformOp::Skew { x: a.angle(0)?, y }
        }
        "skewx" => {
            a.count(1..=1)?;
            TransformOp::Skew {
                x: a.angle(0)?,
                y: 0.0,
            }
        }
        "skewy" => {
            a.count(1..=1)?;
            TransformOp::Skew {
                x: 0.0,
                y: a.angle(0)?,
            }
        }
        "perspective" => {
            a.count(1..=1)?;
            // `none` is the identity (css-transforms-2: `perspective( none )`);
            // below 1px the distance is 1px.
            if let Ok(ComponentValue::Ident(none)) = a.single(0)
                && none.eq_ignore_ascii_case("none")
            {
                TransformOp::Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
            } else {
                let d = a.length(0)?;
                if d < 0.0 {
                    return Err(TransformListError::Syntax);
                }
                TransformOp::Perspective(d.max(1.0))
            }
        }
        _ => return Err(TransformListError::Syntax),
    };
    Ok(op)
}

fn translate(x: f64, y: f64, z: f64, three_d: bool) -> TransformOp {
    TransformOp::Translate { x, y, z, three_d }
}

fn scale(x: f64, y: f64, z: f64, three_d: bool) -> TransformOp {
    TransformOp::Scale { x, y, z, three_d }
}

fn rotate(x: f64, y: f64, z: f64, degrees: f64, three_d: bool) -> TransformOp {
    TransformOp::Rotate {
        x,
        y,
        z,
        degrees,
        three_d,
    }
}

/// The arguments of a function: comma-separated, none empty.
fn split_arguments<'a>(args: &mut Parser<'a>) -> Result<Vec<&'a [ComponentValue]>> {
    let rest = args.remaining();
    let trimmed: Vec<&ComponentValue> = rest.iter().filter(|v| !v.is_whitespace()).collect();
    if trimmed.is_empty() {
        return Err(TransformListError::Syntax);
    }
    let items: Vec<&[ComponentValue]> = rest.split(ComponentValue::is_comma).collect();
    if items
        .iter()
        .any(|item| item.iter().all(ComponentValue::is_whitespace))
    {
        return Err(TransformListError::Syntax);
    }
    Ok(items)
}

/// The arguments of one function, as token slices.
struct Arguments<'a, 'b>(&'b [&'a [ComponentValue]]);

impl Arguments<'_, '_> {
    fn len(&self) -> usize {
        self.0.len()
    }

    /// Fails unless the number of arguments is in `range`.
    fn count(&self, range: std::ops::RangeInclusive<usize>) -> Result<()> {
        if range.contains(&self.len()) {
            Ok(())
        } else {
            Err(TransformListError::Syntax)
        }
    }

    /// Argument `i` as one value.
    fn single(&self, i: usize) -> Result<&ComponentValue> {
        let item = self.0.get(i).ok_or(TransformListError::Syntax)?;
        let mut values = item.iter().filter(|v| !v.is_whitespace());
        match (values.next(), values.next()) {
            (Some(v), None) => Ok(v),
            _ => Err(TransformListError::Syntax),
        }
    }

    /// Argument `i` as a number.
    fn number(&self, i: usize) -> Result<f64> {
        match self.single(i)? {
            ComponentValue::Number(n) => Ok(n.value_f64),
            _ => Err(TransformListError::Syntax),
        }
    }

    /// `N` numbers, exactly.
    fn numbers<const N: usize>(&self) -> Result<[f64; N]> {
        self.count(N..=N)?;
        let mut out = [0.0; N];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = self.number(i)?;
        }
        Ok(out)
    }

    /// A scale factor: a number or a percentage.
    fn factor(&self, i: usize) -> Result<f64> {
        match self.single(i)? {
            ComponentValue::Number(n) => Ok(f64::from(n.value)),
            ComponentValue::Percentage(n) => Ok(f64::from(n.value) / 100.0),
            _ => Err(TransformListError::Syntax),
        }
    }

    /// An absolute `<length>` in px.
    fn length(&self, i: usize) -> Result<f64> {
        match self.single(i)? {
            ComponentValue::Number(n) if n.value == 0.0 => Ok(0.0),
            ComponentValue::Dimension { number, unit } => {
                // Chromium keeps lengths in `f32`: `1cm` is 37.795277.
                let value = number.value;
                let px: f32 = match unit.to_ascii_lowercase().as_str() {
                    "px" => 1.0,
                    "cm" => 96.0 / 2.54,
                    "mm" => 96.0 / 25.4,
                    "q" => 96.0 / 101.6,
                    "in" => 96.0,
                    "pt" => 96.0 / 72.0,
                    "pc" => 16.0,
                    "em" | "rem" | "ex" | "ch" | "vw" | "vh" | "vmin" | "vmax" | "svw" | "svh"
                    | "lvw" | "lvh" | "dvw" | "dvh" | "vi" | "vb" | "cap" | "ic" | "lh" | "rlh" => {
                        return Err(TransformListError::Unresolvable);
                    }
                    _ => return Err(TransformListError::Syntax),
                };
                Ok(f64::from(value * px))
            }
            ComponentValue::Percentage(_) => Err(TransformListError::Unresolvable),
            _ => Err(TransformListError::Syntax),
        }
    }

    /// An optional absolute length, `default` if the argument is missing.
    fn optional_length(&self, i: usize, default: f64) -> Result<f64> {
        if i < self.len() {
            self.length(i)
        } else {
            Ok(default)
        }
    }

    /// An `<angle>` in degrees (a bare `0` is allowed).
    fn angle(&self, i: usize) -> Result<f64> {
        match self.single(i)? {
            ComponentValue::Number(n) if n.value == 0.0 => Ok(0.0),
            ComponentValue::Dimension { number, unit } => {
                let value = number.value_f64;
                match unit.to_ascii_lowercase().as_str() {
                    "deg" => Ok(value),
                    "grad" => Ok(value * 0.9),
                    "rad" => Ok(value.to_degrees()),
                    "turn" => Ok(value * 360.0),
                    _ => Err(TransformListError::Syntax),
                }
            }
            _ => Err(TransformListError::Syntax),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Vec<TransformOp>> {
        parse_transform_list(text)
    }

    #[test]
    fn functions_and_units() {
        assert_eq!(
            parse("translate(1in, 1pt) scale(50%)"),
            Ok(vec![
                TransformOp::Translate {
                    x: 96.0,
                    y: f64::from(96.0_f32 / 72.0_f32),
                    z: 0.0,
                    three_d: false
                },
                TransformOp::Scale {
                    x: 0.5,
                    y: 0.5,
                    z: 1.0,
                    three_d: false
                },
            ])
        );
        assert_eq!(parse("none"), Ok(Vec::new()));
        assert_eq!(parse("NONE"), Ok(Vec::new()));
        assert_eq!(
            parse("rotate3d(0, 0, 0, 90deg)"),
            Ok(vec![TransformOp::Rotate {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                degrees: 90.0,
                three_d: true
            }])
        );
        assert_eq!(
            parse("rotate(1turn) skewX(10deg)"),
            Ok(vec![
                TransformOp::Rotate {
                    x: 0.0,
                    y: 0.0,
                    z: 1.0,
                    degrees: 360.0,
                    three_d: false
                },
                TransformOp::Skew { x: 10.0, y: 0.0 },
            ])
        );
        assert_eq!(
            parse("perspective(0)"),
            Ok(vec![TransformOp::Perspective(1.0)])
        );
        // An unterminated function closes at the end of the text.
        assert!(parse("rotate(90deg").is_ok());
        assert!(parse("MATRIX( 1 , 2 , 3 , 4 , 5 , 6 )").is_ok());
    }

    #[test]
    fn errors() {
        for text in [
            "foo",
            "translate()",
            "translate(1px,)",
            "translate(1px 2px)",
            "translate(1px, 2px) foo",
            "matrix(1,2,3)",
            "matrix(1 2 3 4 5 6)",
            "rotate(30)",
            "translate(1px) ; scale(2)",
            "none translate(1px)",
            "initial",
            "1",
            "",
            "   ",
        ] {
            assert_eq!(parse(text), Err(TransformListError::Syntax), "{text:?}");
        }
        for text in ["translate(1em)", "translate(10%)", "translate(1vw)"] {
            assert_eq!(
                parse(text),
                Err(TransformListError::Unresolvable),
                "{text:?}"
            );
        }
    }

    #[test]
    fn precision_is_chromiums() {
        // Matrix values and angles in `f64`, scale factors in `f32`
        // (Chromium: `new DOMMatrix('matrix(0.1,0,0,1,0,0)').a` is 0.1).
        assert_eq!(
            parse("matrix(0.1, 1e-50, 0, 1, 0, 0)"),
            Ok(vec![TransformOp::Matrix([0.1, 1e-50, 0.0, 1.0, 0.0, 0.0])])
        );
        assert_eq!(
            parse("skewX(89.9deg)"),
            Ok(vec![TransformOp::Skew { x: 89.9, y: 0.0 }])
        );
        assert_eq!(
            parse("scale(0.1)"),
            Ok(vec![TransformOp::Scale {
                x: f64::from(0.1_f32),
                y: f64::from(0.1_f32),
                z: 1.0,
                three_d: false
            }])
        );
    }

    #[test]
    fn long_lists_are_bounded() {
        let long = "scale(1) ".repeat(100_000);
        assert_eq!(parse(&long).map(|ops| ops.len()), Ok(100_000));
        let too_long = "scale(1) ".repeat(200_000);
        assert_eq!(parse(&too_long), Err(TransformListError::Syntax));
    }
}
