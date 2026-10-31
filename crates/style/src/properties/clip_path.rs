//! Parsing and computing `clip-path` (CSS Masking 1 §5,
//! <https://drafts.fxtf.org/css-masking-1/#the-clip-path>): `none`, a
//! `url()`, or a basic shape with an optional geometry box (CSS Shapes 1
//! §3, <https://drafts.csswg.org/css-shapes-1/#basic-shape-functions>):
//! `inset()`, `circle()`, `ellipse()`, `polygon()`, `path()`, and the
//! `rect()` and `xywh()` of CSS Shapes 1 (Chromium 148 takes both and
//! computes them to `inset()`). `shape()` is not supported.
//!
//! The grammar and the limits follow Chromium 148 (measured,
//! `tools/probes/clip-path.json`): `circle(closest-side)` is the same as
//! `circle()`, `polygon()` needs a point, a `path()` string is checked as
//! path data (`path_data.rs`), `inset()` takes one to four sides, a
//! shape takes at most one geometry box in either order, and a geometry
//! box alone is a value.

use std::sync::Arc;

use swb_css::{ParseError, Parser};

use super::compute::ComputeContext;
use super::longhand::{keyword, parse_position};
use super::path_data;
use super::shorthand::{one_to_four, parse_radii};
use super::specified::{SpecifiedPosition, clamp_non_negative};
use crate::parse::ParseResult;
use crate::parse::length::{LengthOptions, parse_length_percentage};
use crate::values::{
    BasicShape, CalcNode, ClipPath, CornerRadius, FillRule, GeometryBox, Inset, LengthContext,
    LengthPercentage, LengthUnit, MAX_RELATIVE_SHAPE_POINTS, MAX_SHAPE_POINTS, ShapeClip,
    ShapePosition, ShapeRadius, SpecifiedLengthPercentage as Lp,
};

/// A specified `clip-path`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SpecifiedClipPath {
    /// `none`.
    None,
    /// `url(...)`, as written (trimmed).
    Url(Arc<str>),
    /// A basic shape and/or a geometry box.
    Shape(Box<SpecifiedShapeClip>),
}

/// A specified shape with a geometry box.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SpecifiedShapeClip {
    shape: Option<SpecifiedShape>,
    /// As written; a `border-box` with a shape is dropped when computed.
    geometry_box: Option<GeometryBox>,
    /// The computed value if no length of the value depends on the
    /// element (no font-relative or viewport unit): all elements share it.
    shared: Option<Arc<ShapeClip>>,
}

/// A specified `<radial-size>` of `circle()` and `ellipse()`.
#[derive(Clone, Debug, PartialEq)]
enum SpecifiedRadius {
    ClosestSide,
    FarthestSide,
    Length(Lp),
}

/// The four sides of an `inset()`, `rect()` or `xywh()`.
type Radii = Option<Box<[(Lp, Lp); 4]>>;

/// A specified basic shape.
#[derive(Clone, Debug, PartialEq)]
enum SpecifiedShape {
    Inset {
        sides: [Lp; 4],
        radii: Radii,
    },
    /// `rect(top right bottom left)`: `None` is `auto`.
    Rect {
        sides: [Option<Lp>; 4],
        radii: Radii,
    },
    /// `xywh(x y width height)`.
    Xywh {
        rect: [Lp; 4],
        radii: Radii,
    },
    Circle {
        radius: SpecifiedRadius,
        position: Option<(SpecifiedPosition, SpecifiedPosition)>,
    },
    Ellipse {
        rx: SpecifiedRadius,
        ry: SpecifiedRadius,
        position: Option<(SpecifiedPosition, SpecifiedPosition)>,
    },
    Polygon {
        rule: FillRule,
        points: Vec<(Lp, Lp)>,
    },
    Path {
        rule: FillRule,
        /// The computed form of the data (absolute commands).
        data: Arc<str>,
    },
}

/// `clip-path`: `none | <url> | [<basic-shape> || <geometry-box>]`.
/// <https://drafts.fxtf.org/css-masking-1/#the-clip-path>
pub(crate) fn parse_clip_path(p: &mut Parser<'_>) -> ParseResult<SpecifiedClipPath> {
    if p.expect_ident_matching("none").is_ok() {
        return Ok(SpecifiedClipPath::None);
    }
    if let Ok(url) = p.try_parse(Parser::expect_url) {
        return Ok(SpecifiedClipPath::Url(Arc::from(url.trim())));
    }
    let mut shape = None;
    let mut geometry_box = None;
    loop {
        if shape.is_none()
            && let Ok(s) = p.try_parse(parse_shape)
        {
            shape = Some(s);
        } else if geometry_box.is_none()
            && let Ok(b) = keyword(p, GeometryBox::from_ident)
        {
            geometry_box = Some(b);
        } else {
            break;
        }
    }
    if shape.is_none() && geometry_box.is_none() {
        return Err(ParseError::Unexpected);
    }
    let mut clip = SpecifiedShapeClip {
        shape,
        geometry_box,
        shared: None,
    };
    clip.shared = clip
        .is_context_free()
        .then(|| Arc::new(clip.build(&LengthContext::DEFAULT)));
    if !clip.fits() {
        return Err(ParseError::Invalid);
    }
    Ok(SpecifiedClipPath::Shape(Box::new(clip)))
}

/// One basic shape function.
fn parse_shape(p: &mut Parser<'_>) -> ParseResult<SpecifiedShape> {
    let (name, mut args) = p.expect_function()?;
    let a = &mut args;
    let shape = match name.to_ascii_lowercase().as_str() {
        "inset" => parse_inset(a)?,
        "rect" => parse_rect(a)?,
        "xywh" => parse_xywh(a)?,
        "circle" => parse_circle(a)?,
        "ellipse" => parse_ellipse(a)?,
        "polygon" => parse_polygon(a)?,
        "path" => parse_path(a)?,
        _ => return Err(ParseError::Unexpected),
    };
    a.expect_exhausted()?;
    Ok(shape)
}

fn length_percentage(p: &mut Parser<'_>) -> ParseResult<Lp> {
    parse_length_percentage(p, LengthOptions::LENGTH_PERCENTAGE)
}

/// `[round <'border-radius'>]?`; `None` without `round`.
fn parse_round(p: &mut Parser<'_>) -> ParseResult<Radii> {
    if p.expect_ident_matching("round").is_err() {
        return Ok(None);
    }
    Ok(Some(Box::new(parse_radii(p)?)))
}

/// `inset( <length-percentage>{1,4} [round <'border-radius'>]? )`.
fn parse_inset(p: &mut Parser<'_>) -> ParseResult<SpecifiedShape> {
    let sides = one_to_four(p, length_percentage)?;
    let radii = parse_round(p)?;
    Ok(SpecifiedShape::Inset { sides, radii })
}

/// `rect( [<length-percentage> | auto]{4} [round <'border-radius'>]? )`.
fn parse_rect(p: &mut Parser<'_>) -> ParseResult<SpecifiedShape> {
    let side = |p: &mut Parser<'_>| -> ParseResult<Option<Lp>> {
        if p.expect_ident_matching("auto").is_ok() {
            return Ok(None);
        }
        length_percentage(p).map(Some)
    };
    let sides = [side(p)?, side(p)?, side(p)?, side(p)?];
    let radii = parse_round(p)?;
    Ok(SpecifiedShape::Rect { sides, radii })
}

/// `xywh( <length-percentage>{2} <length-percentage [0,∞]>{2} [round
/// <'border-radius'>]? )`.
fn parse_xywh(p: &mut Parser<'_>) -> ParseResult<SpecifiedShape> {
    let size = |p: &mut Parser<'_>| parse_length_percentage(p, LengthOptions::NON_NEGATIVE);
    let rect = [
        length_percentage(p)?,
        length_percentage(p)?,
        size(p)?,
        size(p)?,
    ];
    let radii = parse_round(p)?;
    Ok(SpecifiedShape::Xywh { rect, radii })
}

/// `<radial-size>` of one axis: `closest-side | farthest-side |
/// <length-percentage [0,∞]>`.
fn parse_radius(p: &mut Parser<'_>) -> ParseResult<SpecifiedRadius> {
    p.try_parse(|p| {
        if let Ok(extent) =
            p.try_parse(|p| p.expect_one_of(&[("closest-side", false), ("farthest-side", true)]))
        {
            return Ok(if extent {
                SpecifiedRadius::FarthestSide
            } else {
                SpecifiedRadius::ClosestSide
            });
        }
        parse_length_percentage(p, LengthOptions::NON_NEGATIVE).map(SpecifiedRadius::Length)
    })
}

/// `[at <position>]?`.
fn parse_at(p: &mut Parser<'_>) -> ParseResult<Option<(SpecifiedPosition, SpecifiedPosition)>> {
    if p.expect_ident_matching("at").is_err() {
        return Ok(None);
    }
    parse_position(p).map(Some)
}

/// `circle( <radial-size>? [at <position>]? )`.
fn parse_circle(p: &mut Parser<'_>) -> ParseResult<SpecifiedShape> {
    let radius = parse_radius(p).unwrap_or(SpecifiedRadius::ClosestSide);
    let position = parse_at(p)?;
    Ok(SpecifiedShape::Circle { radius, position })
}

/// `ellipse( [<radial-size>{2}]? [at <position>]? )`.
fn parse_ellipse(p: &mut Parser<'_>) -> ParseResult<SpecifiedShape> {
    let (rx, ry) = match parse_radius(p) {
        Ok(rx) => (rx, parse_radius(p)?),
        Err(_) => (SpecifiedRadius::ClosestSide, SpecifiedRadius::ClosestSide),
    };
    let position = parse_at(p)?;
    Ok(SpecifiedShape::Ellipse { rx, ry, position })
}

/// `<'fill-rule'>?,` at the start of the arguments of `polygon()` and
/// `path()`.
fn parse_rule(p: &mut Parser<'_>) -> FillRule {
    p.try_parse(|p| {
        let rule = keyword(p, FillRule::from_ident)?;
        p.expect_comma()?;
        Ok::<_, ParseError>(rule)
    })
    .unwrap_or_default()
}

/// `polygon( <'fill-rule'>? , [<length-percentage> <length-percentage>]# )`.
fn parse_polygon(p: &mut Parser<'_>) -> ParseResult<SpecifiedShape> {
    let rule = parse_rule(p);
    let points = p.parse_comma_separated(|p| {
        Ok::<_, ParseError>((length_percentage(p)?, length_percentage(p)?))
    })?;
    if points.len() > MAX_SHAPE_POINTS {
        return Err(ParseError::Invalid);
    }
    Ok(SpecifiedShape::Polygon { rule, points })
}

/// `path( <'fill-rule'>? , <string> )`.
fn parse_path(p: &mut Parser<'_>) -> ParseResult<SpecifiedShape> {
    let rule = parse_rule(p);
    let data = p.expect_string()?;
    let normalized = path_data::normalize(data).ok_or(ParseError::Invalid)?;
    Ok(SpecifiedShape::Path {
        rule,
        data: Arc::from(normalized.computed),
    })
}

// ----- Computing -----

impl SpecifiedClipPath {
    /// Computes the value. A shape in absolute units is shared.
    pub(crate) fn compute(&self, cx: &ComputeContext<'_>) -> ClipPath {
        match self {
            SpecifiedClipPath::None => ClipPath::None,
            SpecifiedClipPath::Url(u) => ClipPath::Url(Arc::clone(u)),
            SpecifiedClipPath::Shape(clip) => ClipPath::Shape(
                clip.shared
                    .clone()
                    .unwrap_or_else(|| Arc::new(clip.build(&cx.lengths))),
            ),
        }
    }
}

impl SpecifiedShapeClip {
    /// The computed value for the lengths `lc`.
    fn build(&self, lc: &LengthContext) -> ShapeClip {
        let shape = self.shape.as_ref().map(|s| s.compute(lc));
        // A `border-box` after a shape is the initial value.
        let geometry_box = self
            .geometry_box
            .filter(|b| shape.is_none() || *b != GeometryBox::BorderBox);
        ShapeClip {
            shape,
            geometry_box,
        }
    }

    /// True if no length of the value depends on the element or the
    /// viewport.
    fn is_context_free(&self) -> bool {
        self.shape
            .as_ref()
            .is_none_or(SpecifiedShape::is_context_free)
    }

    /// True if the value is within the limits that depend on whether the
    /// value is shared.
    fn fits(&self) -> bool {
        match &self.shape {
            Some(SpecifiedShape::Polygon { points, .. }) => {
                self.shared.is_some() || points.len() <= MAX_RELATIVE_SHAPE_POINTS
            }
            _ => true,
        }
    }
}

/// True if `lp` is in absolute units or a percentage.
fn absolute(lp: &Lp) -> bool {
    match lp {
        Lp::Length(l) => absolute_unit(l.unit),
        Lp::Percentage(_) => true,
        Lp::Calc(node) => absolute_node(node),
    }
}

fn absolute_unit(unit: LengthUnit) -> bool {
    matches!(
        unit,
        LengthUnit::Px
            | LengthUnit::Cm
            | LengthUnit::Mm
            | LengthUnit::Q
            | LengthUnit::In
            | LengthUnit::Pt
            | LengthUnit::Pc
    )
}

fn absolute_node(node: &CalcNode) -> bool {
    match node {
        CalcNode::Length(l) => absolute_unit(l.unit),
        CalcNode::Percentage(_) | CalcNode::Number(_) => true,
        CalcNode::Sum(c) | CalcNode::Min(c) | CalcNode::Max(c) => c.iter().all(absolute_node),
        CalcNode::Product(a, b) | CalcNode::Quotient(a, b) => absolute_node(a) && absolute_node(b),
        CalcNode::Clamp(a, b, c) => absolute_node(a) && absolute_node(b) && absolute_node(c),
    }
}

fn position_absolute(p: Option<&(SpecifiedPosition, SpecifiedPosition)>) -> bool {
    p.is_none_or(|(x, y)| absolute(&x.offset) && absolute(&y.offset))
}

fn radius_absolute(r: &SpecifiedRadius) -> bool {
    match r {
        SpecifiedRadius::Length(l) => absolute(l),
        _ => true,
    }
}

fn radii_absolute(r: &Radii) -> bool {
    r.as_ref()
        .is_none_or(|r| r.iter().all(|(h, v)| absolute(h) && absolute(v)))
}

impl SpecifiedShape {
    fn is_context_free(&self) -> bool {
        match self {
            SpecifiedShape::Inset { sides, radii } => {
                sides.iter().all(absolute) && radii_absolute(radii)
            }
            SpecifiedShape::Rect { sides, radii } => {
                sides.iter().flatten().all(absolute) && radii_absolute(radii)
            }
            SpecifiedShape::Xywh { rect, radii } => {
                rect.iter().all(absolute) && radii_absolute(radii)
            }
            SpecifiedShape::Circle { radius, position } => {
                radius_absolute(radius) && position_absolute(position.as_ref())
            }
            SpecifiedShape::Ellipse { rx, ry, position } => {
                radius_absolute(rx) && radius_absolute(ry) && position_absolute(position.as_ref())
            }
            SpecifiedShape::Polygon { points, .. } => {
                points.iter().all(|(x, y)| absolute(x) && absolute(y))
            }
            SpecifiedShape::Path { .. } => true,
        }
    }

    fn compute(&self, lc: &LengthContext) -> BasicShape {
        match self {
            SpecifiedShape::Inset { sides, radii } => {
                let [top, right, bottom, left] = sides.each_ref().map(|s| s.compute(lc));
                inset(top, right, bottom, left, compute_radii(radii, lc))
            }
            SpecifiedShape::Rect { sides, radii } => {
                let side = |s: &Option<Lp>| {
                    s.as_ref()
                        .map_or(LengthPercentage::Percent(0.0), |s| s.compute(lc))
                };
                let [top, right, bottom, left] = sides.each_ref().map(side);
                // The right and bottom edges are distances from the
                // top left: `rect(auto)` is no inset on that side.
                let from_end = |edge: &Option<Lp>, v: LengthPercentage| {
                    if edge.is_none() {
                        LengthPercentage::Percent(0.0)
                    } else {
                        sub_from_full(&[&v])
                    }
                };
                let right_inset = from_end(&sides[1], right);
                let bottom_inset = from_end(&sides[2], bottom);
                inset(
                    top,
                    right_inset,
                    bottom_inset,
                    left,
                    compute_radii(radii, lc),
                )
            }
            SpecifiedShape::Xywh { rect, radii } => {
                let [x, y, w, h] = rect.each_ref().map(|s| s.compute(lc));
                let right = sub_from_full(&[&x, &w]);
                let bottom = sub_from_full(&[&y, &h]);
                inset(y, right, bottom, x, compute_radii(radii, lc))
            }
            SpecifiedShape::Circle { radius, position } => BasicShape::Circle {
                radius: compute_radius(radius, lc),
                position: compute_position(position.as_ref(), lc),
            },
            SpecifiedShape::Ellipse { rx, ry, position } => BasicShape::Ellipse {
                rx: compute_radius(rx, lc),
                ry: compute_radius(ry, lc),
                position: compute_position(position.as_ref(), lc),
            },
            SpecifiedShape::Polygon { rule, points } => BasicShape::Polygon {
                rule: *rule,
                points: points
                    .iter()
                    .map(|(x, y)| (x.compute(lc), y.compute(lc)))
                    .collect(),
            },
            SpecifiedShape::Path { rule, data } => BasicShape::Path {
                rule: *rule,
                data: Arc::clone(data),
            },
        }
    }
}

fn inset(
    top: LengthPercentage,
    right: LengthPercentage,
    bottom: LengthPercentage,
    left: LengthPercentage,
    radii: Option<Box<[CornerRadius; 4]>>,
) -> BasicShape {
    BasicShape::Inset(Inset {
        top,
        right,
        bottom,
        left,
        radii,
    })
}

/// The corner radii, or `None` if all are zero.
fn compute_radii(radii: &Radii, lc: &LengthContext) -> Option<Box<[CornerRadius; 4]>> {
    let radii = radii.as_ref()?;
    let corner = |(h, v): &(Lp, Lp)| CornerRadius {
        horizontal: clamp_non_negative(h.compute(lc)),
        vertical: clamp_non_negative(v.compute(lc)),
    };
    let computed = radii.each_ref().map(corner);
    let zero = computed
        .iter()
        .all(|c| c.horizontal.is_zero() && c.vertical.is_zero());
    (!zero).then(|| Box::new(computed))
}

fn compute_radius(r: &SpecifiedRadius, lc: &LengthContext) -> ShapeRadius {
    match r {
        SpecifiedRadius::ClosestSide => ShapeRadius::ClosestSide,
        SpecifiedRadius::FarthestSide => ShapeRadius::FarthestSide,
        SpecifiedRadius::Length(l) => ShapeRadius::Length(clamp_non_negative(l.compute(lc))),
    }
}

fn compute_position(
    position: Option<&(SpecifiedPosition, SpecifiedPosition)>,
    lc: &LengthContext,
) -> Option<ShapePosition> {
    let (x, y) = position?;
    // A zero offset is a percentage: `0` is `0%`, and `right 0` is `100%`
    // (measured in Chromium).
    let component = |p: &SpecifiedPosition| {
        let offset = p.offset.compute(lc);
        if offset.is_zero() {
            crate::values::PositionComponent {
                offset: LengthPercentage::Percent(if p.from_end { 1.0 } else { 0.0 }),
                from_end: false,
            }
        } else {
            crate::values::PositionComponent {
                offset,
                from_end: p.from_end,
            }
        }
    };
    Some(ShapePosition {
        x: component(x),
        y: component(y),
    })
}

/// `(px, percent)` of a computed length-percentage that is linear in the
/// percentage basis (the percentage as a fraction).
fn linear(v: &LengthPercentage) -> (f32, f32) {
    match v {
        LengthPercentage::Px(px) => (*px, 0.0),
        LengthPercentage::Percent(p) => (0.0, *p),
        LengthPercentage::Calc(c) => {
            let fixed = c.resolve(0.0);
            (fixed, (c.resolve(100.0) - fixed) / 100.0)
        }
    }
}

/// `100% - a - b ...` for `rect()` and `xywh()`. A pure length stays a
/// length, a pure percentage a percentage (`0%` stays `0%`), a mix is
/// `calc()` with the percentage first, as Chromium writes it.
fn sub_from_full(parts: &[&LengthPercentage]) -> LengthPercentage {
    let (mut px, mut percent) = (0.0, 1.0);
    for part in parts {
        let (p, f) = linear(part);
        px -= p;
        percent -= f;
    }
    if px == 0.0 {
        LengthPercentage::Percent(percent)
    } else if percent == 0.0 {
        LengthPercentage::Px(px)
    } else {
        LengthPercentage::Calc(Arc::new(crate::values::ComputedCalc::Sum(vec![
            crate::values::ComputedCalc::Percent(percent),
            crate::values::ComputedCalc::Px(px),
        ])))
    }
}

#[cfg(test)]
mod tests {
    use swb_css::{ComponentValue, parse_component_values};

    use super::*;
    use crate::values::PositionComponent;

    fn parse(css: &str) -> Option<SpecifiedClipPath> {
        let values: Vec<ComponentValue> = parse_component_values(css);
        Parser::new(&values).parse_entirely(parse_clip_path).ok()
    }

    fn computed(css: &str) -> Option<ClipPath> {
        let cx = ComputeContext {
            parent: &crate::ComputedStyle::initial(),
            lengths: LengthContext::DEFAULT,
            element: None,
            quirks: false,
        };
        parse(css).map(|s| s.compute(&cx))
    }

    fn clip(css: &str) -> Arc<ShapeClip> {
        match computed(css) {
            Some(ClipPath::Shape(clip)) => clip,
            other => panic!("{css:?}: {other:?}"),
        }
    }

    fn px(v: f32) -> LengthPercentage {
        LengthPercentage::Px(v)
    }

    #[test]
    fn keywords_and_urls() {
        assert_eq!(computed("none"), Some(ClipPath::None));
        assert_eq!(
            computed("url( '#a' )"),
            Some(ClipPath::Url(Arc::from("#a")))
        );
        for bad in [
            "auto",
            "url(#a) url(#b)",
            "url(#a) border-box",
            "none border-box",
            "border-box none",
            "scroll-box",
            "circle(50%) padding-box margin-box",
            "border-box border-box",
            "circle() circle()",
            "",
            "shape(from 0 0, line to 10px 10px)",
        ] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn geometry_boxes() {
        let c = clip("content-box circle(50%)");
        assert_eq!(c.geometry_box, Some(GeometryBox::ContentBox));
        assert!(c.shape.is_some());
        // A bare box keeps its keyword; after a shape, `border-box` is the
        // initial value.
        assert_eq!(
            clip("border-box").geometry_box,
            Some(GeometryBox::BorderBox)
        );
        assert_eq!(clip("circle(50%) border-box").geometry_box, None);
        assert_eq!(clip("view-box").shape, None);
        assert_eq!(clip("fill-box").reference_box(), GeometryBox::FillBox);
    }

    #[test]
    #[allow(clippy::many_single_char_names)]
    fn inset_and_rect() {
        let BasicShape::Inset(i) = clip("inset(1px 2px 3px)").shape.clone().unwrap() else {
            panic!("inset")
        };
        assert_eq!(
            (i.top, i.right, i.bottom, i.left),
            (px(1.0), px(2.0), px(3.0), px(2.0))
        );
        assert_eq!(i.radii, None);
        // Zero radii are no radii.
        assert!(matches!(
            clip("inset(1px round 0)").shape,
            Some(BasicShape::Inset(Inset { radii: None, .. }))
        ));
        let BasicShape::Inset(r) = clip("rect(10px 20px 30px 40px)").shape.clone().unwrap() else {
            panic!("rect")
        };
        assert_eq!(r.top, px(10.0));
        assert_eq!(r.left, px(40.0));
        // `100% - 20px`, as Chromium writes `inset(10px calc(100% - 20px) ...)`.
        assert_eq!(linear(&r.right), (-20.0, 1.0));
        assert_eq!(linear(&r.bottom), (-30.0, 1.0));
        let BasicShape::Inset(a) = clip("rect(auto auto auto auto)").shape.clone().unwrap() else {
            panic!("rect")
        };
        assert_eq!(a.right, LengthPercentage::Percent(0.0));
        let BasicShape::Inset(x) = clip("xywh(10% 20% 30% 40%)").shape.clone().unwrap() else {
            panic!("xywh")
        };
        assert_eq!(x.top, LengthPercentage::Percent(0.2));
        assert!(matches!(x.right, LengthPercentage::Percent(p) if (p - 0.6).abs() < 1e-6));
        for bad in [
            "inset()",
            "inset(1px 2px 3px 4px 5px)",
            "inset(auto)",
            "inset(10)",
            "inset(10px,20px)",
            "inset(10px round -5px)",
            "inset(round 5px)",
            "rect(10%)",
            "rect(1px, 2px, 3px, 4px)",
            "xywh(0 0 -1px 1px)",
            "xywh(10px 20px)",
        ] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn circle_and_ellipse() {
        let BasicShape::Circle { radius, position } = clip("circle()").shape.clone().unwrap()
        else {
            panic!("circle")
        };
        assert_eq!(radius, ShapeRadius::ClosestSide);
        assert_eq!(position, None);
        let BasicShape::Circle { radius, position } =
            clip("circle(farthest-side at right 10px bottom 20px)")
                .shape
                .clone()
                .unwrap()
        else {
            panic!("circle")
        };
        assert_eq!(radius, ShapeRadius::FarthestSide);
        let position = position.unwrap();
        assert_eq!(
            position.x,
            PositionComponent {
                offset: px(10.0),
                from_end: true
            }
        );
        assert_eq!(
            position.y,
            PositionComponent {
                offset: px(20.0),
                from_end: true
            }
        );
        assert!(matches!(
            clip("ellipse(10px 20px)").shape,
            Some(BasicShape::Ellipse {
                rx: ShapeRadius::Length(_),
                ry: ShapeRadius::Length(_),
                position: None
            })
        ));
        for bad in [
            "circle(-5px)",
            "circle(10px 20px)",
            "circle(10px at)",
            "circle(at 10px 20px 30px)",
            "circle(at left 10px top)",
            "circle(at top 10px)",
            "circle(at 10px right)",
            "ellipse(10px)",
            "ellipse(-1px 2px)",
            "ellipse(closest-side)",
        ] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
        assert!(parse("circle(50%at 10px 10px)").is_some());
        assert!(parse("CIRCLE(at left bottom)").is_some());
    }

    #[test]
    fn polygon_and_path() {
        let BasicShape::Polygon { rule, points } = clip("polygon(evenodd, 0 0, 1em 2em)")
            .shape
            .clone()
            .unwrap()
        else {
            panic!("polygon")
        };
        assert_eq!(rule, FillRule::EvenOdd);
        assert_eq!(points, vec![(px(0.0), px(0.0)), (px(16.0), px(32.0))]);
        let BasicShape::Path { rule, data } = clip("path(nonzero, 'M0 0 h10 v10 z')")
            .shape
            .clone()
            .unwrap()
        else {
            panic!("path")
        };
        assert_eq!(rule, FillRule::NonZero);
        assert_eq!(&*data, "M 0 0 H 10 V 10 Z");
        for bad in [
            "polygon()",
            "polygon(0 0, 1px)",
            "polygon(0 0, 10px 0, 0 10px,)",
            "polygon(0 0 10px 0)",
            "polygon(evenodd 0 0, 10px 10px)",
            "polygon(evenodd,)",
            "path()",
            "path('')",
            "path('invalid')",
            "path(M0 0 L10 0)",
            "path('M0 0 L 10')",
            "path(evenodd 'M0 0 h10')",
            "path('M0 0 h10',)",
        ] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn computed_values_are_shared_for_absolute_units() {
        let a = parse("polygon(0 0, 10px 0, 0 10px)").unwrap();
        let SpecifiedClipPath::Shape(s) = &a else {
            panic!("shape")
        };
        assert!(s.shared.is_some());
        let r = parse("polygon(0 0, 1em 0, 0 1rem)").unwrap();
        let SpecifiedClipPath::Shape(s) = &r else {
            panic!("shape")
        };
        assert!(s.shared.is_none());
        let long = |unit: &str, n: usize| {
            format!(
                "polygon({})",
                vec![format!("1{unit} 1{unit}"); n].join(", ")
            )
        };
        assert!(parse(&long("px", MAX_SHAPE_POINTS)).is_some());
        assert!(parse(&long("px", MAX_SHAPE_POINTS + 1)).is_none());
        assert!(parse(&long("em", MAX_RELATIVE_SHAPE_POINTS)).is_some());
        assert!(parse(&long("em", MAX_RELATIVE_SHAPE_POINTS + 1)).is_none());
    }
}
