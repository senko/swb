//! Media queries.
//!
//! <https://www.w3.org/TR/mediaqueries-4/>
//!
//! A query that does not match the grammar becomes `not all` (it never
//! matches); the other queries in the list are not affected. Evaluation is
//! three-valued: unknown features, invalid feature values and
//! `<general-enclosed>` evaluate to "unknown", which `not`, `and` and `or`
//! propagate (Kleene logic), and which counts as false at the top level.
//!
//! The environment ([`MediaEnvironment`]) describes the viewport and user
//! preferences. Features that swb does not vary report fixed values:
//! 8 bits per color channel, sRGB gamut, no monochrome, bitmap (not grid)
//! display, no scripting, no forced or inverted colors, `browser` display
//! mode, standard dynamic range.
//!
//! Lengths: `em` and `rem` use [`MediaEnvironment::root_font_size`]; `ex`
//! and `ch` are approximated as half of it.
//! Math functions (`calc()`, `min()`, `max()`, `clamp()`) in length values
//! are evaluated when the query is parsed if all their lengths have
//! absolute units (`calc(640px - 1px)`); with relative units the feature
//! evaluates to "unknown".
//!
//! `MediaCondition` is a condition without a media type, for the `sizes`
//! attribute of images (`crate::sizes`). The source size values of that
//! attribute (`length_px`) resolve relative units against the environment,
//! also inside math functions.

use std::fmt::{self, Write as _};

use crate::cursor::{ParseError, Parser};
use crate::serialize::serialize_identifier;
use crate::values::{BlockKind, ComponentValue, split_on_commas, trim_whitespace};

/// The media type of the output device.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MediaType {
    /// A screen.
    #[default]
    Screen,
    /// Printed output (paged media).
    Print,
}

/// A preferred color scheme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ColorScheme {
    /// Light.
    #[default]
    Light,
    /// Dark.
    Dark,
}

/// The values that media queries test.
#[derive(Clone, Debug, PartialEq)]
pub struct MediaEnvironment {
    /// The media type.
    pub media_type: MediaType,
    /// The viewport width in CSS px.
    pub viewport_width: f32,
    /// The viewport height in CSS px.
    pub viewport_height: f32,
    /// Device pixels per CSS px.
    pub device_pixel_ratio: f32,
    /// `prefers-color-scheme`.
    pub prefers_color_scheme: ColorScheme,
    /// `prefers-reduced-motion: reduce`.
    pub prefers_reduced_motion: bool,
    /// The primary pointer can hover (`hover: hover`).
    pub hover: bool,
    /// The primary pointer is fine (`pointer: fine`); otherwise it is
    /// coarse.
    pub fine_pointer: bool,
    /// The initial font size in CSS px, for `em` and `rem`.
    pub root_font_size: f32,
}

impl Default for MediaEnvironment {
    fn default() -> Self {
        MediaEnvironment {
            media_type: MediaType::Screen,
            viewport_width: 800.0,
            viewport_height: 600.0,
            device_pixel_ratio: 1.0,
            prefers_color_scheme: ColorScheme::Light,
            prefers_reduced_motion: false,
            hover: true,
            fine_pointer: true,
            root_font_size: 16.0,
        }
    }
}

/// A media query list, for example `screen and (min-width: 600px), print`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaQueryList {
    queries: Vec<MediaQuery>,
}

impl MediaQueryList {
    /// Parses a media query list. Invalid queries become `not all`.
    ///
    /// <https://www.w3.org/TR/mediaqueries-4/#mq-list>
    pub fn parse(input: &[ComponentValue]) -> Self {
        if trim_whitespace(input).is_empty() {
            return MediaQueryList::default();
        }
        MediaQueryList {
            queries: split_on_commas(input).map(parse_query).collect(),
        }
    }

    /// Parses a media query list from text (for the `media` attribute of
    /// `<link>` and `<style>`).
    pub fn parse_str(css: &str) -> Self {
        Self::parse(&crate::parse_component_values(css))
    }

    /// True if the list is empty. An empty list matches all media.
    pub fn is_empty(&self) -> bool {
        self.queries.is_empty()
    }

    /// True if the list is empty or any query in it matches `env`.
    pub fn matches(&self, env: &MediaEnvironment) -> bool {
        self.queries.is_empty() || self.queries.iter().any(|q| q.evaluate(env))
    }
}

/// A `<media-condition>`: a media query without a media type, as in the
/// `sizes` attribute of images.
///
/// <https://www.w3.org/TR/mediaqueries-4/#typedef-media-condition>
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MediaCondition(Condition);

impl MediaCondition {
    /// Parses a media condition. Returns `None` if `input` is not one
    /// (an empty input is not one either).
    pub(crate) fn parse(input: &[ComponentValue]) -> Option<Self> {
        Parser::new(input)
            .parse_entirely(|p| parse_condition(p, true))
            .ok()
            .map(MediaCondition)
    }

    /// True if the condition is true in `env`. "Unknown" counts as false.
    pub(crate) fn matches(&self, env: &MediaEnvironment) -> bool {
        self.0.evaluate(env) == Kleene::True
    }
}

#[derive(Clone, Debug, PartialEq)]
enum MediaQuery {
    /// A query that did not parse: `not all`.
    Invalid,
    Query {
        negated: bool,
        media_type: TypeMatch,
        condition: Option<Condition>,
    },
}

/// The media type in a query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TypeMatch {
    All,
    Screen,
    Print,
    /// A valid type that never matches (`tv`, `speech`, unknown names).
    Other,
}

#[derive(Clone, Debug, PartialEq)]
enum Condition {
    Not(Box<Condition>),
    And(Vec<Condition>),
    Or(Vec<Condition>),
    Feature(Feature),
    /// `<general-enclosed>`, an unknown feature or an invalid value.
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
struct Feature {
    name: FeatureName,
    /// Empty for the boolean context: `(hover)`.
    comparisons: Vec<(Comparison, Value)>,
}

/// A comparison `feature <op> value`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Comparison {
    Eq,
    Lt,
    Le,
    Gt,
    Ge,
}

impl Comparison {
    /// The comparison with the operands swapped: `a < b` is `b > a`.
    fn flip(self) -> Self {
        match self {
            Comparison::Eq => Comparison::Eq,
            Comparison::Lt => Comparison::Gt,
            Comparison::Le => Comparison::Ge,
            Comparison::Gt => Comparison::Lt,
            Comparison::Ge => Comparison::Le,
        }
    }

    fn test(self, ordering: Option<std::cmp::Ordering>) -> bool {
        use std::cmp::Ordering::{Equal, Greater, Less};
        matches!(
            (self, ordering),
            (Comparison::Eq, Some(Equal))
                | (Comparison::Lt, Some(Less))
                | (Comparison::Le, Some(Less | Equal))
                | (Comparison::Gt, Some(Greater))
                | (Comparison::Ge, Some(Greater | Equal))
        )
    }

    fn as_str(self) -> &'static str {
        match self {
            Comparison::Eq => "=",
            Comparison::Lt => "<",
            Comparison::Le => "<=",
            Comparison::Gt => ">",
            Comparison::Ge => ">=",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Value {
    Length(f32, LengthUnit),
    /// A number, an integer, or a resolution in dppx.
    Number(f32),
    Ratio(f32, f32),
    Keyword(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LengthUnit {
    Px,
    Em,
    Rem,
    Ex,
    Ch,
    Vw,
    Vh,
    Vmin,
    Vmax,
    Cm,
    Mm,
    Q,
    In,
    Pt,
    Pc,
}

/// The length units of media queries and `sizes`. `LengthUnit::from_name`
/// in `swb-style` (`values/length.rs`) has the units of property values;
/// keep the two lists in step (the roadmap lists the units missing here).
const LENGTH_UNITS: &[(&str, LengthUnit)] = &[
    ("px", LengthUnit::Px),
    ("em", LengthUnit::Em),
    ("rem", LengthUnit::Rem),
    ("ex", LengthUnit::Ex),
    ("ch", LengthUnit::Ch),
    ("vw", LengthUnit::Vw),
    ("svw", LengthUnit::Vw),
    ("lvw", LengthUnit::Vw),
    ("dvw", LengthUnit::Vw),
    ("vh", LengthUnit::Vh),
    ("svh", LengthUnit::Vh),
    ("lvh", LengthUnit::Vh),
    ("dvh", LengthUnit::Vh),
    ("vmin", LengthUnit::Vmin),
    ("vmax", LengthUnit::Vmax),
    ("cm", LengthUnit::Cm),
    ("mm", LengthUnit::Mm),
    ("q", LengthUnit::Q),
    ("in", LengthUnit::In),
    ("pt", LengthUnit::Pt),
    ("pc", LengthUnit::Pc),
];

impl LengthUnit {
    fn from_name(unit: &str) -> Option<Self> {
        LENGTH_UNITS
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(unit))
            .map(|&(_, u)| u)
    }

    fn name(self) -> &'static str {
        LENGTH_UNITS
            .iter()
            .find(|&&(_, u)| u == self)
            .map_or("px", |&(name, _)| name)
    }

    /// True for units that do not depend on the environment.
    fn is_absolute(self) -> bool {
        matches!(
            self,
            LengthUnit::Px
                | LengthUnit::Cm
                | LengthUnit::Mm
                | LengthUnit::Q
                | LengthUnit::In
                | LengthUnit::Pt
                | LengthUnit::Pc
        )
    }

    fn to_px(self, value: f32, env: &MediaEnvironment) -> f32 {
        let (w, h) = (env.viewport_width, env.viewport_height);
        // Multiply first, then divide, so that exact values stay exact
        // (750pt is exactly 1000px).
        let (multiplier, divisor) = match self {
            LengthUnit::Px => (1.0, 1.0),
            LengthUnit::Em | LengthUnit::Rem => (env.root_font_size, 1.0),
            LengthUnit::Ex | LengthUnit::Ch => (env.root_font_size, 2.0),
            LengthUnit::Vw => (w, 100.0),
            LengthUnit::Vh => (h, 100.0),
            LengthUnit::Vmin => (w.min(h), 100.0),
            LengthUnit::Vmax => (w.max(h), 100.0),
            LengthUnit::Cm => (96.0, 2.54),
            LengthUnit::Mm => (96.0, 25.4),
            LengthUnit::Q => (96.0, 101.6),
            LengthUnit::In => (96.0, 1.0),
            LengthUnit::Pt => (96.0, 72.0),
            LengthUnit::Pc => (16.0, 1.0),
        };
        value * multiplier / divisor
    }
}

/// The type of a feature's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ValueType {
    Length,
    Ratio,
    Resolution,
    /// A non-negative integer.
    Integer,
    /// Any number (`-webkit-device-pixel-ratio`).
    Number,
    /// One of these keywords.
    Keyword(&'static [&'static str]),
}

impl ValueType {
    /// Range features allow `min-`/`max-` prefixes and range syntax.
    fn is_range(self) -> bool {
        !matches!(self, ValueType::Keyword(_))
    }
}

macro_rules! features {
    ($($variant:ident = $name:literal : $ty:expr,)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum FeatureName { $($variant,)* }

        const FEATURES: &[(&str, FeatureName, ValueType)] = &[
            $(($name, FeatureName::$variant, $ty),)*
        ];
    };
}

const HOVER: &[&str] = &["none", "hover"];
const POINTER: &[&str] = &["none", "coarse", "fine"];

features! {
    Width = "width": ValueType::Length,
    Height = "height": ValueType::Length,
    DeviceWidth = "device-width": ValueType::Length,
    DeviceHeight = "device-height": ValueType::Length,
    AspectRatio = "aspect-ratio": ValueType::Ratio,
    DeviceAspectRatio = "device-aspect-ratio": ValueType::Ratio,
    Orientation = "orientation": ValueType::Keyword(&["portrait", "landscape"]),
    Resolution = "resolution": ValueType::Resolution,
    DevicePixelRatio = "-webkit-device-pixel-ratio": ValueType::Number,
    Color = "color": ValueType::Integer,
    ColorIndex = "color-index": ValueType::Integer,
    Monochrome = "monochrome": ValueType::Integer,
    Grid = "grid": ValueType::Integer,
    ColorGamut = "color-gamut": ValueType::Keyword(&["srgb", "p3", "rec2020"]),
    DynamicRange = "dynamic-range": ValueType::Keyword(&["standard", "high"]),
    VideoDynamicRange = "video-dynamic-range": ValueType::Keyword(&["standard", "high"]),
    PrefersColorScheme = "prefers-color-scheme": ValueType::Keyword(&["light", "dark"]),
    PrefersReducedMotion = "prefers-reduced-motion": ValueType::Keyword(&["no-preference", "reduce"]),
    PrefersReducedTransparency = "prefers-reduced-transparency": ValueType::Keyword(&["no-preference", "reduce"]),
    PrefersContrast = "prefers-contrast": ValueType::Keyword(&["no-preference", "more", "less", "custom"]),
    ForcedColors = "forced-colors": ValueType::Keyword(&["none", "active"]),
    InvertedColors = "inverted-colors": ValueType::Keyword(&["none", "inverted"]),
    Hover = "hover": ValueType::Keyword(HOVER),
    AnyHover = "any-hover": ValueType::Keyword(HOVER),
    Pointer = "pointer": ValueType::Keyword(POINTER),
    AnyPointer = "any-pointer": ValueType::Keyword(POINTER),
    Scripting = "scripting": ValueType::Keyword(&["none", "initial-only", "enabled"]),
    DisplayMode = "display-mode": ValueType::Keyword(&["fullscreen", "standalone", "minimal-ui", "browser", "picture-in-picture"]),
    Update = "update": ValueType::Keyword(&["none", "slow", "fast"]),
    OverflowBlock = "overflow-block": ValueType::Keyword(&["none", "scroll", "paged"]),
    OverflowInline = "overflow-inline": ValueType::Keyword(&["none", "scroll"]),
}

impl FeatureName {
    fn info(self) -> (&'static str, ValueType) {
        FEATURES
            .iter()
            .find(|&&(_, f, _)| f == self)
            .map_or(("", ValueType::Number), |&(name, _, ty)| (name, ty))
    }

    fn value_type(self) -> ValueType {
        self.info().1
    }

    /// Looks up a feature name. Returns the feature and the comparison that
    /// a `min-`/`max-` prefix implies (`None` without a prefix).
    fn lookup(name: &str) -> Option<(FeatureName, Option<Comparison>)> {
        let lower = name.to_ascii_lowercase();
        // `-webkit-min-device-pixel-ratio` puts the prefix after `-webkit-`.
        let (vendor, rest) = match lower.strip_prefix("-webkit-") {
            Some(rest) => ("-webkit-", rest),
            None => ("", lower.as_str()),
        };
        let (prefix, base) = if let Some(base) = rest.strip_prefix("min-") {
            (Some(Comparison::Ge), base)
        } else if let Some(base) = rest.strip_prefix("max-") {
            (Some(Comparison::Le), base)
        } else {
            (None, rest)
        };
        let full = format!("{vendor}{base}");
        let &(_, feature, ty) = FEATURES.iter().find(|(n, _, _)| *n == full)?;
        if prefix.is_some() && !ty.is_range() {
            return None;
        }
        Some((feature, prefix))
    }
}

/// Parses one media query. A query that does not match the grammar becomes
/// `not all`.
///
/// <https://www.w3.org/TR/mediaqueries-4/#mq-syntax>,
/// <https://www.w3.org/TR/mediaqueries-4/#error-handling>
fn parse_query(input: &[ComponentValue]) -> MediaQuery {
    let mut p = Parser::new(input);
    let typed = p.parse_entirely(parse_typed_query);
    if let Ok(query) = typed {
        return query;
    }
    match p.parse_entirely(|p| parse_condition(p, true)) {
        Ok(condition) => MediaQuery::Query {
            negated: false,
            media_type: TypeMatch::All,
            condition: Some(condition),
        },
        Err(_) => MediaQuery::Invalid,
    }
}

/// `[not | only]? <media-type> [and <media-condition-without-or>]?`
fn parse_typed_query(p: &mut Parser<'_>) -> Result<MediaQuery, ParseError> {
    let mut ident = p.expect_ident()?;
    let mut negated = false;
    if ident.eq_ignore_ascii_case("not") {
        negated = true;
        ident = p.expect_ident()?;
    } else if ident.eq_ignore_ascii_case("only") {
        ident = p.expect_ident()?;
    }
    let media_type = match ident.to_ascii_lowercase().as_str() {
        "all" => TypeMatch::All,
        "screen" => TypeMatch::Screen,
        "print" => TypeMatch::Print,
        "only" | "not" | "and" | "or" | "layer" => return Err(ParseError::Invalid),
        _ => TypeMatch::Other,
    };
    let condition = if p.expect_ident_matching("and").is_ok() {
        Some(parse_condition(p, false)?)
    } else {
        None
    };
    Ok(MediaQuery::Query {
        negated,
        media_type,
        condition,
    })
}

/// `<media-condition>`, or `<media-condition-without-or>` if `allow_or` is
/// false.
fn parse_condition(p: &mut Parser<'_>, allow_or: bool) -> Result<Condition, ParseError> {
    if p.expect_ident_matching("not").is_ok() {
        return Ok(Condition::Not(Box::new(parse_in_parens(p)?)));
    }
    let first = parse_in_parens(p)?;
    let mut rest = Vec::new();
    let mut is_and = None;
    loop {
        let and = if p.expect_ident_matching("and").is_ok() {
            true
        } else if allow_or && p.expect_ident_matching("or").is_ok() {
            false
        } else {
            break;
        };
        if is_and.is_some_and(|previous| previous != and) {
            // `and` and `or` cannot mix without parentheses.
            return Err(ParseError::Invalid);
        }
        is_and = Some(and);
        rest.push(parse_in_parens(p)?);
    }
    Ok(match is_and {
        None => first,
        Some(and) => {
            rest.insert(0, first);
            if and {
                Condition::And(rest)
            } else {
                Condition::Or(rest)
            }
        }
    })
}

/// `<media-in-parens>`
fn parse_in_parens(p: &mut Parser<'_>) -> Result<Condition, ParseError> {
    match p.peek() {
        Some(ComponentValue::Block(b)) if b.kind == BlockKind::Paren => {
            p.next();
            Ok(parse_paren_contents(&b.contents))
        }
        // `<general-enclosed>` in function form.
        Some(ComponentValue::Function(_)) => {
            p.next();
            Ok(Condition::Unknown)
        }
        Some(_) => Err(ParseError::Unexpected),
        None => Err(ParseError::EndOfInput),
    }
}

/// The contents of `( ... )`: a condition, a feature, or general-enclosed.
fn parse_paren_contents(contents: &[ComponentValue]) -> Condition {
    if let Ok(condition) = Parser::new(contents).parse_entirely(|p| parse_condition(p, true)) {
        return condition;
    }
    parse_feature(trim_whitespace(contents)).map_or(Condition::Unknown, Condition::Feature)
}

/// Parses `<media-feature>`. Returns `None` for unknown features, invalid
/// values and anything that is not a feature (general-enclosed).
fn parse_feature(contents: &[ComponentValue]) -> Option<Feature> {
    // Boolean: `(name)`.
    if let [ComponentValue::Ident(name)] = contents {
        let (name, prefix) = FeatureName::lookup(name)?;
        return prefix.is_none().then_some(Feature {
            name,
            comparisons: Vec::new(),
        });
    }
    // Plain: `(name: value)`.
    if let [ComponentValue::Ident(name), rest @ ..] = contents {
        let rest = trim_whitespace(rest);
        if let [ComponentValue::Colon, value @ ..] = rest {
            let (name, prefix) = FeatureName::lookup(name)?;
            let value = parse_value(name.value_type(), trim_whitespace(value))?;
            return Some(Feature {
                name,
                comparisons: vec![(prefix.unwrap_or(Comparison::Eq), value)],
            });
        }
    }
    parse_range(contents)
}

/// Parses the range forms: `(name < value)`, `(value < name)` and
/// `(value < name < value)`.
///
/// <https://www.w3.org/TR/mediaqueries-4/#mq-range-context>
fn parse_range(contents: &[ComponentValue]) -> Option<Feature> {
    let (parts, operators) = split_at_comparisons(contents);
    let feature_name = |part: &[ComponentValue]| match part {
        [ComponentValue::Ident(name)] => match FeatureName::lookup(name) {
            Some((feature, None)) if feature.value_type().is_range() => Some(feature),
            _ => None,
        },
        _ => None,
    };
    match (&parts[..], &operators[..]) {
        ([left, right], [op]) => {
            if let Some(name) = feature_name(left) {
                let value = parse_value(name.value_type(), right)?;
                return Some(Feature {
                    name,
                    comparisons: vec![(*op, value)],
                });
            }
            let name = feature_name(right)?;
            let value = parse_value(name.value_type(), left)?;
            Some(Feature {
                name,
                comparisons: vec![(op.flip(), value)],
            })
        }
        ([low, middle, high], [op1, op2]) => {
            let is_lt = |op: Comparison| matches!(op, Comparison::Lt | Comparison::Le);
            let is_gt = |op: Comparison| matches!(op, Comparison::Gt | Comparison::Ge);
            if !((is_lt(*op1) && is_lt(*op2)) || (is_gt(*op1) && is_gt(*op2))) {
                return None;
            }
            let name = feature_name(middle)?;
            let low = parse_value(name.value_type(), low)?;
            let high = parse_value(name.value_type(), high)?;
            Some(Feature {
                name,
                comparisons: vec![(op1.flip(), low), (*op2, high)],
            })
        }
        _ => None,
    }
}

/// Splits at `<`, `<=`, `>`, `>=` and `=`. The parts are trimmed.
fn split_at_comparisons(contents: &[ComponentValue]) -> (Vec<&[ComponentValue]>, Vec<Comparison>) {
    let mut parts = Vec::new();
    let mut operators = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < contents.len() {
        let next_is_equals = contents.get(i + 1).is_some_and(|v| v.is_delim('='));
        let (op, len) = match &contents[i] {
            ComponentValue::Delim('<') if next_is_equals => (Comparison::Le, 2),
            ComponentValue::Delim('<') => (Comparison::Lt, 1),
            ComponentValue::Delim('>') if next_is_equals => (Comparison::Ge, 2),
            ComponentValue::Delim('>') => (Comparison::Gt, 1),
            ComponentValue::Delim('=') => (Comparison::Eq, 1),
            _ => {
                i += 1;
                continue;
            }
        };
        parts.push(trim_whitespace(&contents[start..i]));
        operators.push(op);
        i += len;
        start = i;
    }
    parts.push(trim_whitespace(&contents[start..]));
    (parts, operators)
}

/// Parses a feature value of the given type.
fn parse_value(ty: ValueType, input: &[ComponentValue]) -> Option<Value> {
    let mut p = Parser::new(input);
    let value = match ty {
        ValueType::Length => match p.next()? {
            ComponentValue::Dimension { number, unit } => {
                Value::Length(number.value, LengthUnit::from_name(unit)?)
            }
            ComponentValue::Number(n) if n.value == 0.0 => Value::Length(0.0, LengthUnit::Px),
            ComponentValue::Function(f) => match math_function(&f.name, &f.arguments, None)? {
                MathValue::Px(px) => Value::Length(px, LengthUnit::Px),
                MathValue::Number(_) => return None,
            },
            _ => return None,
        },
        ValueType::Ratio => {
            let numerator = p.expect_number().ok()?;
            let denominator = if p.expect_delim('/').is_ok() {
                p.expect_number().ok()?
            } else {
                1.0
            };
            if numerator < 0.0 || denominator < 0.0 {
                return None;
            }
            Value::Ratio(numerator, denominator)
        }
        ValueType::Resolution => {
            let (value, unit) = p.expect_dimension().ok()?;
            let dppx = match unit.to_ascii_lowercase().as_str() {
                "dppx" | "x" => value,
                "dpi" => value / 96.0,
                "dpcm" => value * 2.54 / 96.0,
                _ => return None,
            };
            Value::Number(dppx)
        }
        ValueType::Integer => {
            let n = p.expect_integer().ok()?;
            if n < 0 {
                return None;
            }
            Value::Number(n as f32)
        }
        ValueType::Number => Value::Number(p.expect_number().ok()?),
        ValueType::Keyword(keywords) => {
            let ident = p.expect_ident().ok()?;
            let keyword = keywords.iter().find(|k| k.eq_ignore_ascii_case(ident))?;
            Value::Keyword(keyword)
        }
    };
    p.is_exhausted().then_some(value)
}

/// The value of a math function in a media feature.
#[derive(Clone, Copy, Debug, PartialEq)]
enum MathValue {
    Number(f32),
    Px(f32),
}

/// The value in px of a length in a `sizes` attribute: a dimension with a
/// length unit, a unitless zero, or a math function. Relative units use
/// `env`, as in media queries. Returns `None` for anything else. The value
/// can be negative; callers decide what that means.
pub(crate) fn length_px(value: &ComponentValue, env: &MediaEnvironment) -> Option<f32> {
    match value {
        ComponentValue::Dimension { number, unit } => {
            Some(LengthUnit::from_name(unit)?.to_px(number.value, env))
        }
        ComponentValue::Number(n) if n.value == 0.0 => Some(0.0),
        ComponentValue::Function(f) => match math_function(&f.name, &f.arguments, Some(env))? {
            MathValue::Px(px) => Some(px),
            MathValue::Number(_) => None,
        },
        _ => None,
    }
}

/// Evaluates a math function (`calc()`, `min()`, `max()`, `clamp()`) in a
/// length value, as CSS Values 4 allows. Without `env` (media features,
/// evaluated when the query is parsed), only absolute length units are
/// supported; relative units (`em`, `vw`, ...) make the feature evaluate to
/// "unknown". With `env`, relative units resolve against it.
/// <https://www.w3.org/TR/css-values-4/#math>
fn math_function(
    name: &str,
    args: &[ComponentValue],
    env: Option<&MediaEnvironment>,
) -> Option<MathValue> {
    let name = name.to_ascii_lowercase();
    if name == "calc" {
        let mut p = Parser::new(args);
        let value = math_sum(&mut p, env)?;
        return p.is_exhausted().then_some(value);
    }
    let items: Vec<MathValue> = split_on_commas(args)
        .map(|item| {
            let mut p = Parser::new(item);
            let v = math_sum(&mut p, env)?;
            p.is_exhausted().then_some(v)
        })
        .collect::<Option<_>>()?;
    let px = |v: &MathValue| match v {
        MathValue::Px(px) => Some(*px),
        MathValue::Number(_) => None,
    };
    let values: Vec<f32> = items.iter().map(px).collect::<Option<_>>()?;
    let result = match (name.as_str(), values.as_slice()) {
        ("min", [first, rest @ ..]) => rest.iter().fold(*first, |a, b| a.min(*b)),
        ("max", [first, rest @ ..]) => rest.iter().fold(*first, |a, b| a.max(*b)),
        ("clamp", [lo, v, hi]) => v.min(*hi).max(*lo),
        _ => return None,
    };
    Some(MathValue::Px(result))
}

/// `<calc-sum>`; the operators need whitespace on both sides.
fn math_sum(p: &mut Parser<'_>, env: Option<&MediaEnvironment>) -> Option<MathValue> {
    let mut acc = math_product(p, env)?;
    loop {
        let start = p.position();
        if !p.skip_whitespace() {
            break;
        }
        let negate = match p.next_including_whitespace() {
            Some(v) if v.is_delim('+') => false,
            Some(v) if v.is_delim('-') => true,
            _ => {
                p.reset(start);
                break;
            }
        };
        if !p.skip_whitespace() {
            return None;
        }
        let rhs = math_product(p, env)?;
        acc = match (acc, rhs) {
            (MathValue::Px(a), MathValue::Px(b)) => {
                MathValue::Px(if negate { a - b } else { a + b })
            }
            (MathValue::Number(a), MathValue::Number(b)) => {
                MathValue::Number(if negate { a - b } else { a + b })
            }
            _ => return None,
        };
    }
    Some(acc)
}

/// `<calc-product>`.
fn math_product(p: &mut Parser<'_>, env: Option<&MediaEnvironment>) -> Option<MathValue> {
    let mut acc = math_operand(p, env)?;
    while let Ok(op) = p.try_parse(|p| match p.next() {
        Some(v) if v.is_delim('*') => Ok('*'),
        Some(v) if v.is_delim('/') => Ok('/'),
        _ => Err(ParseError::Unexpected),
    }) {
        let rhs = math_operand(p, env)?;
        acc = match (op, acc, rhs) {
            ('*', MathValue::Number(a), MathValue::Number(b)) => MathValue::Number(a * b),
            ('*', MathValue::Px(a), MathValue::Number(b))
            | ('*', MathValue::Number(b), MathValue::Px(a)) => MathValue::Px(a * b),
            ('/', MathValue::Number(a), MathValue::Number(b)) if b != 0.0 => {
                MathValue::Number(a / b)
            }
            ('/', MathValue::Px(a), MathValue::Number(b)) if b != 0.0 => MathValue::Px(a / b),
            _ => return None,
        };
    }
    Some(acc)
}

/// A number, a length (only an absolute one without `env`), a nested math
/// function or a parenthesized sum.
fn math_operand(p: &mut Parser<'_>, env: Option<&MediaEnvironment>) -> Option<MathValue> {
    match p.next()? {
        ComponentValue::Number(n) => Some(MathValue::Number(n.value)),
        ComponentValue::Dimension { number, unit } => {
            let unit = LengthUnit::from_name(unit)?;
            match env {
                Some(env) => Some(MathValue::Px(unit.to_px(number.value, env))),
                None => unit
                    .is_absolute()
                    .then(|| MathValue::Px(unit.to_px(number.value, &MediaEnvironment::default()))),
            }
        }
        ComponentValue::Function(f) => math_function(&f.name, &f.arguments, env),
        ComponentValue::Block(b) if b.kind == BlockKind::Paren => {
            let mut inner = Parser::new(&b.contents);
            let v = math_sum(&mut inner, env)?;
            inner.is_exhausted().then_some(v)
        }
        _ => None,
    }
}

/// A three-valued truth value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kleene {
    True,
    False,
    Unknown,
}

impl Kleene {
    fn from_bool(b: bool) -> Self {
        if b { Kleene::True } else { Kleene::False }
    }

    fn not(self) -> Self {
        match self {
            Kleene::True => Kleene::False,
            Kleene::False => Kleene::True,
            Kleene::Unknown => Kleene::Unknown,
        }
    }

    fn and(self, other: Self) -> Self {
        match (self, other) {
            (Kleene::False, _) | (_, Kleene::False) => Kleene::False,
            (Kleene::True, Kleene::True) => Kleene::True,
            _ => Kleene::Unknown,
        }
    }

    fn or(self, other: Self) -> Self {
        match (self, other) {
            (Kleene::True, _) | (_, Kleene::True) => Kleene::True,
            (Kleene::False, Kleene::False) => Kleene::False,
            _ => Kleene::Unknown,
        }
    }
}

impl MediaQuery {
    /// <https://www.w3.org/TR/mediaqueries-4/#evaluating>
    fn evaluate(&self, env: &MediaEnvironment) -> bool {
        let MediaQuery::Query {
            negated,
            media_type,
            condition,
        } = self
        else {
            return false;
        };
        let type_matches = match media_type {
            TypeMatch::All => true,
            TypeMatch::Screen => env.media_type == MediaType::Screen,
            TypeMatch::Print => env.media_type == MediaType::Print,
            TypeMatch::Other => false,
        };
        let condition = condition.as_ref().map_or(Kleene::True, |c| c.evaluate(env));
        let mut result = Kleene::from_bool(type_matches).and(condition);
        if *negated {
            result = result.not();
        }
        result == Kleene::True
    }
}

impl Condition {
    fn evaluate(&self, env: &MediaEnvironment) -> Kleene {
        match self {
            Condition::Not(c) => c.evaluate(env).not(),
            Condition::And(list) => list
                .iter()
                .fold(Kleene::True, |acc, c| acc.and(c.evaluate(env))),
            Condition::Or(list) => list
                .iter()
                .fold(Kleene::False, |acc, c| acc.or(c.evaluate(env))),
            Condition::Feature(f) => Kleene::from_bool(f.evaluate(env)),
            Condition::Unknown => Kleene::Unknown,
        }
    }
}

/// The value of a feature in the environment.
enum Actual {
    Number(f32),
    Ratio(f32, f32),
    Keyword(&'static str),
}

impl Feature {
    fn evaluate(&self, env: &MediaEnvironment) -> bool {
        let actual = self.name.actual(env);
        if self.comparisons.is_empty() {
            return match actual {
                Actual::Number(n) => n != 0.0,
                Actual::Ratio(w, h) => w != 0.0 && h != 0.0,
                Actual::Keyword(k) => !matches!(k, "none" | "no-preference"),
            };
        }
        self.comparisons
            .iter()
            .all(|&(op, value)| op.test(compare(&actual, value, env)))
    }
}

/// Compares an environment value with a query value.
fn compare(actual: &Actual, value: Value, env: &MediaEnvironment) -> Option<std::cmp::Ordering> {
    match (actual, value) {
        (Actual::Number(a), Value::Number(v)) => a.partial_cmp(&v),
        (Actual::Number(a), Value::Length(v, unit)) => a.partial_cmp(&unit.to_px(v, env)),
        // a/b compared with c/d as a*d with c*b.
        (Actual::Ratio(w, h), Value::Ratio(n, d)) => (w * d).partial_cmp(&(n * h)),
        (Actual::Keyword(a), Value::Keyword(v)) => (*a == v).then_some(std::cmp::Ordering::Equal),
        _ => None,
    }
}

impl FeatureName {
    fn actual(self, env: &MediaEnvironment) -> Actual {
        let screen = env.media_type == MediaType::Screen;
        let keyword = Actual::Keyword;
        match self {
            FeatureName::Width | FeatureName::DeviceWidth => Actual::Number(env.viewport_width),
            FeatureName::Height | FeatureName::DeviceHeight => Actual::Number(env.viewport_height),
            FeatureName::AspectRatio | FeatureName::DeviceAspectRatio => {
                Actual::Ratio(env.viewport_width, env.viewport_height)
            }
            FeatureName::Orientation => keyword(if env.viewport_height >= env.viewport_width {
                "portrait"
            } else {
                "landscape"
            }),
            FeatureName::Resolution | FeatureName::DevicePixelRatio => {
                Actual::Number(env.device_pixel_ratio)
            }
            FeatureName::Color => Actual::Number(8.0),
            FeatureName::ColorIndex | FeatureName::Monochrome | FeatureName::Grid => {
                Actual::Number(0.0)
            }
            FeatureName::ColorGamut => keyword("srgb"),
            FeatureName::DynamicRange | FeatureName::VideoDynamicRange => keyword("standard"),
            FeatureName::PrefersColorScheme => keyword(match env.prefers_color_scheme {
                ColorScheme::Light => "light",
                ColorScheme::Dark => "dark",
            }),
            FeatureName::PrefersReducedMotion => keyword(if env.prefers_reduced_motion {
                "reduce"
            } else {
                "no-preference"
            }),
            FeatureName::PrefersReducedTransparency | FeatureName::PrefersContrast => {
                keyword("no-preference")
            }
            FeatureName::ForcedColors | FeatureName::InvertedColors | FeatureName::Scripting => {
                keyword("none")
            }
            FeatureName::Hover | FeatureName::AnyHover => {
                keyword(if env.hover { "hover" } else { "none" })
            }
            FeatureName::Pointer | FeatureName::AnyPointer => {
                keyword(if env.fine_pointer { "fine" } else { "coarse" })
            }
            FeatureName::DisplayMode => keyword("browser"),
            FeatureName::Update => keyword(if screen { "fast" } else { "none" }),
            FeatureName::OverflowBlock => keyword(if screen { "scroll" } else { "paged" }),
            FeatureName::OverflowInline => keyword(if screen { "scroll" } else { "none" }),
        }
    }
}

impl fmt::Display for MediaQueryList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        for (i, query) in self.queries.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            write_query(query, &mut out);
        }
        f.write_str(&out)
    }
}

fn write_query(query: &MediaQuery, out: &mut String) {
    let MediaQuery::Query {
        negated,
        media_type,
        condition,
    } = query
    else {
        out.push_str("not all");
        return;
    };
    if *negated {
        out.push_str("not ");
    }
    let type_name = match media_type {
        TypeMatch::All => "all",
        TypeMatch::Screen => "screen",
        TypeMatch::Print => "print",
        TypeMatch::Other => "unknown",
    };
    match condition {
        Some(c) if *media_type == TypeMatch::All && !*negated => write_condition(c, out),
        Some(c) => {
            out.push_str(type_name);
            out.push_str(" and ");
            write_condition(c, out);
        }
        None => out.push_str(type_name),
    }
}

fn write_condition(condition: &Condition, out: &mut String) {
    let join = |list: &[Condition], word: &str, out: &mut String| {
        for (i, c) in list.iter().enumerate() {
            if i > 0 {
                out.push_str(word);
            }
            write_nested_condition(c, out);
        }
    };
    match condition {
        Condition::Not(c) => {
            out.push_str("not ");
            write_nested_condition(c, out);
        }
        Condition::And(list) => join(list, " and ", out),
        Condition::Or(list) => join(list, " or ", out),
        Condition::Feature(_) | Condition::Unknown => write_nested_condition(condition, out),
    }
}

/// Writes a condition in parentheses.
fn write_nested_condition(condition: &Condition, out: &mut String) {
    out.push('(');
    match condition {
        Condition::Feature(feature) => write_feature(feature, out),
        // General-enclosed text is not kept.
        Condition::Unknown => out.push_str("unknown"),
        _ => write_condition(condition, out),
    }
    out.push(')');
}

fn write_feature(feature: &Feature, out: &mut String) {
    let name = feature.name.info().0;
    match &feature.comparisons[..] {
        [] => serialize_identifier(name, out),
        [(Comparison::Eq, value)] => {
            serialize_identifier(name, out);
            out.push_str(": ");
            write_value(*value, out);
        }
        [(op, value)] => {
            serialize_identifier(name, out);
            let _ = write!(out, " {} ", op.as_str());
            write_value(*value, out);
        }
        [(low_op, low), (high_op, high), ..] => {
            write_value(*low, out);
            let _ = write!(out, " {} ", low_op.flip().as_str());
            serialize_identifier(name, out);
            let _ = write!(out, " {} ", high_op.as_str());
            write_value(*high, out);
        }
    }
}

fn write_value(value: Value, out: &mut String) {
    let _ = match value {
        Value::Length(v, unit) => write!(out, "{v}{}", unit.name()),
        Value::Number(v) => write!(out, "{v}"),
        Value::Ratio(n, d) => write!(out, "{n}/{d}"),
        Value::Keyword(k) => write!(out, "{k}"),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> MediaEnvironment {
        MediaEnvironment {
            media_type: MediaType::Screen,
            viewport_width: 1000.0,
            viewport_height: 800.0,
            device_pixel_ratio: 2.0,
            prefers_color_scheme: ColorScheme::Light,
            prefers_reduced_motion: false,
            hover: true,
            fine_pointer: true,
            root_font_size: 16.0,
        }
    }

    fn eval(query: &str) -> bool {
        MediaQueryList::parse_str(query).matches(&env())
    }

    /// Queries and their results in [`env`].
    const EVALUATION_CASES: &[(&str, bool)] = &[
        ("", true),
        ("all", true),
        ("screen", true),
        ("SCREEN", true),
        ("print", false),
        ("tv", false),
        ("unknown-type", false),
        ("only screen", true),
        ("not screen", false),
        ("not print", true),
        ("not tv", true),
        ("screen, print", true),
        ("print, screen", true),
        ("print, tv", false),
        ("screen and (min-width: 1000px)", true),
        ("screen and (min-width: 1001px)", false),
        ("screen and (max-width: 999.5px)", false),
        ("(width: 1000px)", true),
        ("(width: 62.5em)", true),
        ("(width: 62.5rem)", true),
        ("(min-width: 125ex)", true),
        ("(max-width: 100vw)", true),
        ("(max-height: 100vh)", true),
        ("(min-width: 10in)", true),
        ("(min-width: 26.45cm)", true),
        ("(min-width: 26.46cm)", false),
        ("(min-width: 264mm)", true),
        ("(min-width: 750pt)", true),
        ("(min-width: 62.5pc)", true),
        ("(min-width: 1058Q)", true),
        ("(min-width: 0)", true),
        ("(min-width: 10)", false),
        ("(width >= 600px)", true),
        ("(width > 1000px)", false),
        ("(width>=1000px)", true),
        ("(width <= 1000px)", true),
        ("(width < 1000px)", false),
        ("(width = 1000px)", true),
        ("(600px <= width)", true),
        ("(1000px < width)", false),
        ("(400px < width <= 1000px)", true),
        ("(400px < width < 1000px)", false),
        ("(1200px > width > 900px)", true),
        ("(400px < width > 300px)", false),
        ("(width < = 1000px)", false),
        ("(height: 800px)", true),
        ("(aspect-ratio: 5/4)", true),
        ("(aspect-ratio: 5 / 4)", true),
        ("(aspect-ratio: 1.25)", true),
        ("(min-aspect-ratio: 16/9)", false),
        ("(max-aspect-ratio: 16/9)", true),
        ("(aspect-ratio > 1)", true),
        ("(orientation: landscape)", true),
        ("(orientation: portrait)", false),
        ("(orientation)", true),
        ("(min-orientation: portrait)", false),
        ("(resolution: 2dppx)", true),
        ("(resolution: 2x)", true),
        ("(min-resolution: 192dpi)", true),
        ("(min-resolution: 2dpcm)", true),
        ("(max-resolution: 1.5dppx)", false),
        ("(-webkit-min-device-pixel-ratio: 2)", true),
        ("(-webkit-max-device-pixel-ratio: 1)", false),
        ("(prefers-color-scheme: light)", true),
        ("(prefers-color-scheme: dark)", false),
        ("(prefers-color-scheme: blue)", false),
        ("not (prefers-color-scheme: blue)", false),
        ("(prefers-reduced-motion: reduce)", false),
        ("(prefers-reduced-motion: no-preference)", true),
        ("(prefers-reduced-motion)", false),
        ("(prefers-contrast: no-preference)", true),
        ("(prefers-contrast: more)", false),
        ("(hover: hover)", true),
        ("(hover)", true),
        ("(any-hover: none)", false),
        ("(pointer: fine)", true),
        ("(pointer: coarse)", false),
        ("(any-pointer: fine)", true),
        ("(color)", true),
        ("(min-color: 8)", true),
        ("(min-color: 9)", false),
        ("(color: 8)", true),
        ("(color-index)", false),
        ("(monochrome)", false),
        ("(monochrome: 0)", true),
        ("(grid)", false),
        ("(grid: 0)", true),
        ("(color-gamut: srgb)", true),
        ("(color-gamut: p3)", false),
        ("(scripting: none)", true),
        ("(scripting: enabled)", false),
        ("(scripting)", false),
        ("(forced-colors: none)", true),
        ("(forced-colors: active)", false),
        ("(inverted-colors: none)", true),
        ("(display-mode: browser)", true),
        ("(update: fast)", true),
        ("(dynamic-range: standard)", true),
        ("(overflow-block: scroll)", true),
        ("(min-width: 100px) and (max-width: 2000px)", true),
        ("(min-width: 100px) and (max-width: 200px)", false),
        ("(max-width: 100px) or (min-width: 200px)", true),
        ("not (max-width: 100px)", true),
        ("not (min-width: 100px)", false),
        ("((min-width: 100px) and (hover))", true),
        ("(not (hover)) or (color)", true),
        ("((((color))))", true),
        ("screen and (not (hover))", false),
        ("not screen and (color)", false),
        ("not print and (color)", true),
        // Unknown parts: unknown at the top level is false.
        ("(unknown-feature)", false),
        ("not (unknown-feature)", false),
        ("(unknown-feature) or (color)", true),
        ("(unknown-feature) and (color)", false),
        ("(foo bar baz)", false),
        ("foo(bar)", false),
        ("(width: red)", false),
        ("(width: 10foo)", false),
        ("not screen and (unknown)", false),
        // Math functions with absolute units.
        ("(min-width: calc(100px))", true),
        ("(max-width: calc(1120px - 1px))", true),
        (
            "(min-width: calc(640px - 1px)) and (max-width: calc(1680px - 1px))",
            true,
        ),
        ("(max-width: calc(800px - 1px))", false),
        ("(min-width: calc(2 * 300px + (100px / 4)))", true),
        ("(min-width: min(900px, 1in * 10))", true),
        ("(min-width: max(1100px, 10px))", false),
        ("(min-width: clamp(10px, 1001px, 2000px))", false),
        // Relative units in math functions and type errors are unknown.
        ("(min-width: calc(10em))", false),
        ("(min-width: calc(1px + 1))", false),
        ("(min-width: calc(1px+1px))", false),
        ("(min-width: calc(1px * 2px))", false),
        ("(min-width: calc(5))", false),
        // Invalid queries are `not all`; other queries still count.
        ("screen and", false),
        ("screen and (color) or (hover)", false),
        ("(color) and (hover) or (grid)", false),
        ("only (color)", false),
        ("not only screen", false),
        ("and", false),
        ("screen and(color)", false),
        ("(color),", true),
        (",(color)", true),
        ("@media", false),
        ("print, garbage!, screen", true),
        ("not", false),
        ("(color) (hover)", false),
        ("only", false),
        ("screen print", false),
        ("layer", false),
    ];

    #[test]
    fn evaluation_table() {
        for (query, expected) in EVALUATION_CASES {
            assert_eq!(eval(query), *expected, "{query}");
        }
    }

    #[test]
    fn environment_changes() {
        let list = MediaQueryList::parse_str("print");
        let print = MediaEnvironment {
            media_type: MediaType::Print,
            ..env()
        };
        assert!(list.matches(&print));
        assert!(!list.matches(&env()));
        let dark = MediaEnvironment {
            prefers_color_scheme: ColorScheme::Dark,
            prefers_reduced_motion: true,
            hover: false,
            fine_pointer: false,
            ..env()
        };
        for (query, expected) in [
            ("(prefers-color-scheme: dark)", true),
            ("(prefers-reduced-motion)", true),
            ("(hover: none)", true),
            ("(hover)", false),
            ("(pointer: coarse)", true),
        ] {
            assert_eq!(
                MediaQueryList::parse_str(query).matches(&dark),
                expected,
                "{query}"
            );
        }
        let narrow = MediaEnvironment {
            viewport_width: 300.0,
            root_font_size: 20.0,
            ..env()
        };
        assert!(MediaQueryList::parse_str("(orientation: portrait)").matches(&narrow));
        assert!(MediaQueryList::parse_str("(width: 15em)").matches(&narrow));
    }

    #[test]
    fn serialization() {
        let cases = [
            ("screen", "screen"),
            ("not print", "not print"),
            (
                "only screen and (min-width: 600px)",
                "screen and (width >= 600px)",
            ),
            ("(400px < width <= 50em)", "(400px < width <= 50em)"),
            ("(1200px > width > 900px)", "(1200px > width > 900px)"),
            ("(width: 600px)", "(width: 600px)"),
            ("(aspect-ratio: 16/9)", "(aspect-ratio: 16/9)"),
            ("(hover) or (color)", "(hover) or (color)"),
            ("not (hover)", "not (hover)"),
            ("(orientation: LANDSCAPE)", "(orientation: landscape)"),
            ("screen, bad and", "screen, not all"),
            ("(foo)", "(unknown)"),
        ];
        for (query, expected) in cases {
            assert_eq!(
                MediaQueryList::parse_str(query).to_string(),
                expected,
                "{query}"
            );
        }
    }

    #[test]
    fn media_conditions() {
        let cases = [
            ("(min-width: 900px)", Some(true)),
            ("(min-width: 900px) and (hover)", Some(true)),
            ("(max-width: 900px) or (hover)", Some(true)),
            ("not (hover)", Some(false)),
            ("(foo)", Some(false)),
            ("screen", None),
            ("screen and (hover)", None),
            ("", None),
            ("(hover) (color)", None),
        ];
        for (condition, expected) in cases {
            let parsed = MediaCondition::parse(&crate::parse_component_values(condition));
            assert_eq!(parsed.map(|c| c.matches(&env())), expected, "{condition}");
        }
    }

    #[test]
    fn lengths_resolve_relative_units() {
        let px = |s: &str| {
            let values = crate::parse_component_values(s);
            length_px(&values[0], &env())
        };
        assert_eq!(px("10vw"), Some(100.0));
        assert_eq!(px("2em"), Some(32.0));
        assert_eq!(px("calc(10vw - 2em)"), Some(68.0));
        assert_eq!(px("min(50vh, 30rem)"), Some(400.0));
        assert_eq!(px("calc(10px - 20px)"), Some(-10.0));
        assert_eq!(px("0"), Some(0.0));
        assert_eq!(px("5"), None);
        assert_eq!(px("10%"), None);
        assert_eq!(px("calc(10%)"), None);
        assert_eq!(px("calc(5)"), None);
        assert_eq!(px("10foo"), None);
    }

    #[test]
    fn empty_and_invalid_lists() {
        assert!(MediaQueryList::parse_str("").is_empty());
        assert!(MediaQueryList::parse_str("   ").matches(&env()));
        assert!(!MediaQueryList::parse_str("!!").is_empty());
        assert!(!MediaQueryList::parse_str("!!").matches(&env()));
    }
}
