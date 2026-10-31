//! The data string of `path()` in `clip-path` (CSS Shapes 1 §3.4,
//! <https://drafts.csswg.org/css-shapes-1/#funcdef-basic-shape-path>): the
//! SVG path data grammar (<https://svgwg.org/svg2-draft/paths.html#PathData>),
//! checked and written in the two forms that Chromium gives (measured,
//! `tools/probes/clip-path.json`):
//!
//! - the specified form keeps the commands as written (a relative command
//!   stays relative) with one space between the tokens and `Z` for a close:
//!   `m 0 0 l 10 0 Z`;
//! - the computed form has absolute commands (`h 10` is `H 10`):
//!   `M 0 0 L 10 0 Z`.
//!
//! A string is valid if it is not empty, starts with a move command and
//! has no error and no segment beyond [`MAX_SHAPE_POINTS`].

use svgtypes::{PathParser, PathSegment};

use crate::cssom::format_number;
use crate::values::{MAX_PATH_DATA_BYTES, MAX_SHAPE_POINTS};

/// Path data written in both forms.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NormalizedPath {
    /// With the commands as written.
    pub(crate) specified: String,
    /// With absolute commands.
    pub(crate) computed: String,
}

/// Checks `data` and writes it in both forms; `None` if it is not valid
/// path data for `path()`.
pub(crate) fn normalize(data: &str) -> Option<NormalizedPath> {
    if data.len() > MAX_PATH_DATA_BYTES {
        return None;
    }
    let mut specified = String::new();
    let mut computed = String::new();
    let mut count = 0usize;
    // The current point and the start of the current subpath.
    let mut at = (0.0_f64, 0.0_f64);
    let mut start = (0.0_f64, 0.0_f64);
    for segment in PathParser::from(data) {
        let segment = segment.ok()?;
        if count == 0 && !matches!(segment, PathSegment::MoveTo { .. }) {
            return None;
        }
        count += 1;
        if count > MAX_SHAPE_POINTS {
            return None;
        }
        write_segment(&mut specified, &segment, None);
        // A relative command far from the origin can make an absolute
        // coordinate infinite, which is not path data: the value is
        // invalid (Chromium not measured for it).
        if !write_segment(&mut computed, &segment, Some(at)) {
            return None;
        }
        at = end_point(&segment, at, start);
        if matches!(segment, PathSegment::MoveTo { .. }) {
            start = at;
        }
        if matches!(segment, PathSegment::ClosePath { .. }) {
            at = start;
        }
    }
    (count > 0).then_some(NormalizedPath {
        specified,
        computed,
    })
}

/// The point where `segment` ends, from the current point `at` and the
/// start `start` of the subpath.
fn end_point(segment: &PathSegment, at: (f64, f64), start: (f64, f64)) -> (f64, f64) {
    let to = |abs: bool, x: f64, y: f64| if abs { (x, y) } else { (at.0 + x, at.1 + y) };
    match *segment {
        PathSegment::MoveTo { abs, x, y }
        | PathSegment::LineTo { abs, x, y }
        | PathSegment::CurveTo { abs, x, y, .. }
        | PathSegment::SmoothCurveTo { abs, x, y, .. }
        | PathSegment::Quadratic { abs, x, y, .. }
        | PathSegment::SmoothQuadratic { abs, x, y }
        | PathSegment::EllipticalArc { abs, x, y, .. } => to(abs, x, y),
        PathSegment::HorizontalLineTo { abs, x } => (if abs { x } else { at.0 + x }, at.1),
        PathSegment::VerticalLineTo { abs, y } => (at.0, if abs { y } else { at.1 + y }),
        PathSegment::ClosePath { .. } => start,
    }
}

/// Appends `segment` to `out`. With `absolute_from`, a relative command is
/// written absolute, from that current point. False if a coordinate is not
/// finite.
fn write_segment(
    out: &mut String,
    segment: &PathSegment,
    absolute_from: Option<(f64, f64)>,
) -> bool {
    if !out.is_empty() {
        out.push(' ');
    }
    let relative = !segment.is_abs() && absolute_from.is_none();
    let (dx, dy) = match absolute_from {
        Some(at) if !segment.is_abs() => at,
        _ => (0.0, 0.0),
    };
    let letter = |upper: u8| {
        char::from(if relative {
            upper.to_ascii_lowercase()
        } else {
            upper
        })
    };
    let mut numbers: Vec<f64> = Vec::new();
    let command = match *segment {
        PathSegment::MoveTo { x, y, .. } => {
            numbers.extend([x + dx, y + dy]);
            letter(b'M')
        }
        PathSegment::LineTo { x, y, .. } => {
            numbers.extend([x + dx, y + dy]);
            letter(b'L')
        }
        PathSegment::HorizontalLineTo { x, .. } => {
            numbers.push(x + dx);
            letter(b'H')
        }
        PathSegment::VerticalLineTo { y, .. } => {
            numbers.push(y + dy);
            letter(b'V')
        }
        PathSegment::CurveTo {
            x1,
            y1,
            x2,
            y2,
            x,
            y,
            ..
        } => {
            numbers.extend([x1 + dx, y1 + dy, x2 + dx, y2 + dy, x + dx, y + dy]);
            letter(b'C')
        }
        PathSegment::SmoothCurveTo { x2, y2, x, y, .. } => {
            numbers.extend([x2 + dx, y2 + dy, x + dx, y + dy]);
            letter(b'S')
        }
        PathSegment::Quadratic { x1, y1, x, y, .. } => {
            numbers.extend([x1 + dx, y1 + dy, x + dx, y + dy]);
            letter(b'Q')
        }
        PathSegment::SmoothQuadratic { x, y, .. } => {
            numbers.extend([x + dx, y + dy]);
            letter(b'T')
        }
        PathSegment::EllipticalArc {
            rx,
            ry,
            x_axis_rotation,
            large_arc,
            sweep,
            x,
            y,
            ..
        } => {
            out.push(letter(b'A'));
            for n in [rx, ry, x_axis_rotation] {
                out.push(' ');
                out.push_str(&format_number(n));
            }
            out.push_str(if large_arc { " 1" } else { " 0" });
            out.push_str(if sweep { " 1" } else { " 0" });
            for n in [x + dx, y + dy] {
                out.push(' ');
                out.push_str(&format_number(n));
            }
            return (x + dx).is_finite() && (y + dy).is_finite();
        }
        // A close is `Z` in both forms.
        PathSegment::ClosePath { .. } => {
            out.push('Z');
            return true;
        }
    };
    out.push(command);
    for &n in &numbers {
        out.push(' ');
        out.push_str(&format_number(n));
    }
    numbers.iter().all(|n| n.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn both(data: &str) -> Option<(String, String)> {
        normalize(data).map(|n| (n.specified, n.computed))
    }

    #[test]
    fn forms_as_chromium_writes_them() {
        let expect = |data: &str, specified: &str, computed: &str| {
            assert_eq!(
                both(data),
                Some((specified.to_owned(), computed.to_owned()))
            );
        };
        expect(
            "M0 0 L10 0 L10 10 z",
            "M 0 0 L 10 0 L 10 10 Z",
            "M 0 0 L 10 0 L 10 10 Z",
        );
        expect("M0 0 h10 v10 z", "M 0 0 h 10 v 10 Z", "M 0 0 H 10 V 10 Z");
        expect(
            "M0,0L10,0L10,10Z",
            "M 0 0 L 10 0 L 10 10 Z",
            "M 0 0 L 10 0 L 10 10 Z",
        );
        expect(
            "m0 0 l10 0 l0 10 z",
            "m 0 0 l 10 0 l 0 10 Z",
            "M 0 0 L 10 0 L 10 10 Z",
        );
        expect(
            "M0 0 H 10 V 10 z m 5 5 h 3",
            "M 0 0 H 10 V 10 Z m 5 5 h 3",
            "M 0 0 H 10 V 10 Z M 5 5 H 8",
        );
        expect("M0 0 L1e1 0", "M 0 0 L 10 0", "M 0 0 L 10 0");
        expect("M0 0 L.5 .5", "M 0 0 L 0.5 0.5", "M 0 0 L 0.5 0.5");
        expect("M0 0 L-.5-.5", "M 0 0 L -0.5 -0.5", "M 0 0 L -0.5 -0.5");
        expect(
            " M0 0 L10 0 L10 10 z  ",
            "M 0 0 L 10 0 L 10 10 Z",
            "M 0 0 L 10 0 L 10 10 Z",
        );
        expect(
            "M0 0 A 10 10 0 0 1 20 0 z",
            "M 0 0 A 10 10 0 0 1 20 0 Z",
            "M 0 0 A 10 10 0 0 1 20 0 Z",
        );
        expect(
            "M0 0 C 10 0 10 10 0 10",
            "M 0 0 C 10 0 10 10 0 10",
            "M 0 0 C 10 0 10 10 0 10",
        );
        expect(
            "M10 10 l5 5 z l5 5",
            "M 10 10 l 5 5 Z l 5 5",
            "M 10 10 L 15 15 Z L 15 15",
        );
        expect("M0 0", "M 0 0", "M 0 0");
    }

    #[test]
    fn invalid_data() {
        for data in [
            "",
            "invalid",
            "L0 0",
            "M0 0 L 10",
            "M0 0 L10 0 L10 10 z X",
            "M0 0 L",
            "  ",
            "M1e308 0 l1e308 0 l1e308 0 z",
            "M0 0 l1e308 1e308 c1e308 0 0 0 0 0",
        ] {
            assert_eq!(both(data), None, "{data:?}");
        }
    }

    #[test]
    fn limits() {
        let ok = format!("M0 0{}", " L1 1".repeat(MAX_SHAPE_POINTS - 1));
        assert!(normalize(&ok).is_some());
        let long = format!("M0 0{}", " L1 1".repeat(MAX_SHAPE_POINTS));
        assert!(normalize(&long).is_none());
        let huge = format!("M0 0{}", " ".repeat(MAX_PATH_DATA_BYTES));
        assert!(normalize(&huge).is_none());
    }
}
