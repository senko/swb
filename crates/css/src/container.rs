//! Container conditions: the prelude of `@container` (CSS Containment 3).
//!
//! <https://www.w3.org/TR/css-contain-3/#container-rule>
//!
//! A [`ContainerCondition`] is an optional container name and an optional
//! query (at least one of the two). The query is a condition tree like that
//! of media queries (`not`, `and`, `or`, parentheses) whose leaves are size
//! features (`width`, `height`, `inline-size`, `block-size`, `aspect-ratio`,
//! `orientation`, with `min-`/`max-` and the range syntax). Anything else in
//! parentheses or in a function (`style()`, `scroll-state()`, an unknown
//! feature, an invalid value) is `<general-enclosed>`.
//!
//! The style crate finds the query container of an element (the nearest
//! flat tree ancestor with the name whose `container-type` supports every
//! feature of the query: [`ContainerCondition::axes`]) and evaluates the
//! query against its content box ([`ContainerCondition::evaluate`]). Measured
//! in Chromium 148 (`tools/probes/container-queries.json`, ADR 0038): a query
//! with a `<general-enclosed>` part finds no container, so it is false even
//! under `or` and `not` ([`ContainerCondition::has_unknown`]).
//!
//! The serialization (`conditionText`) is Chromium's: feature names in
//! lowercase, spaces around the comparison operators and after the colon,
//! the value on the side where it was written, math functions folded to
//! `calc(Npx)`, `<general-enclosed>` as written.

use std::fmt::{self, Write as _};

use crate::cursor::{ParseError, Parser};
use crate::media::{
    Comparison, Kleene, LengthUnit, MathValue, math_function, split_at_comparisons,
};
use crate::serialize::{serialize_component_values, serialize_identifier};
use crate::values::{BlockKind, ComponentValue, trim_whitespace};

/// The prelude of an `@container` rule.
#[derive(Clone, Debug, PartialEq)]
pub struct ContainerCondition {
    name: Option<Box<str>>,
    query: Option<Query>,
}

/// The axes that a query reads. The physical axes map to the logical axes
/// of a candidate container by its writing mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QueryAxes {
    /// `width` (or `aspect-ratio`, `orientation`).
    pub width: bool,
    /// `height` (or `aspect-ratio`, `orientation`).
    pub height: bool,
    /// `inline-size`.
    pub inline: bool,
    /// `block-size`.
    pub block: bool,
}

impl QueryAxes {
    /// The logical axes (inline, block) that a container with the inline
    /// axis vertical (`vertical`) needs for these axes.
    pub fn logical(self, vertical: bool) -> (bool, bool) {
        let (horizontal_axis, vertical_axis) = (self.width, self.height);
        let (on_inline, on_block) = if vertical {
            (vertical_axis, horizontal_axis)
        } else {
            (horizontal_axis, vertical_axis)
        };
        (self.inline || on_inline, self.block || on_block)
    }
}

/// What a query is evaluated against: the content box of the query
/// container and the values that relative lengths in the query use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContainerSize {
    /// The content box width in px; `None` if unknown (no box, no layout
    /// yet, or the container's type does not support the axis).
    pub width: Option<f32>,
    /// The content box height in px; `None` if unknown.
    pub height: Option<f32>,
    /// True if the container's inline axis is vertical.
    pub vertical: bool,
    /// The container's computed font size, for `em`, `ex` and `ch`.
    pub font_size: f32,
    /// The container's line height in px, for `lh`.
    pub line_height: f32,
    /// The root element's font size, for `rem`.
    pub root_font_size: f32,
    /// The viewport size, for the viewport units.
    pub viewport_width: f32,
    /// The viewport height.
    pub viewport_height: f32,
}

#[derive(Clone, Debug, PartialEq)]
enum Query {
    Not(Box<Query>),
    And(Vec<Query>),
    Or(Vec<Query>),
    /// A query in parentheses.
    Group(Box<Query>),
    Feature(SizeFeature),
    /// `<general-enclosed>`, with its text.
    Unknown(Box<str>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SizeName {
    Width,
    Height,
    InlineSize,
    BlockSize,
    AspectRatio,
    Orientation,
}

const SIZE_FEATURES: &[(&str, SizeName)] = &[
    ("width", SizeName::Width),
    ("height", SizeName::Height),
    ("inline-size", SizeName::InlineSize),
    ("block-size", SizeName::BlockSize),
    ("aspect-ratio", SizeName::AspectRatio),
    ("orientation", SizeName::Orientation),
];

impl SizeName {
    fn name(self) -> &'static str {
        SIZE_FEATURES
            .iter()
            .find(|&&(_, n)| n == self)
            .map_or("width", |&(name, _)| name)
    }

    fn is_range(self) -> bool {
        self != SizeName::Orientation
    }

    /// The feature and the comparison of a `min-`/`max-` prefix.
    fn lookup(name: &str) -> Option<(SizeName, Option<Comparison>)> {
        let lower = name.to_ascii_lowercase();
        let (prefix, base) = if let Some(base) = lower.strip_prefix("min-") {
            (Some(Comparison::Ge), base)
        } else if let Some(base) = lower.strip_prefix("max-") {
            (Some(Comparison::Le), base)
        } else {
            (None, lower.as_str())
        };
        let &(_, feature) = SIZE_FEATURES.iter().find(|(n, _)| *n == base)?;
        if prefix.is_some() && !feature.is_range() {
            return None;
        }
        Some((feature, prefix))
    }
}

/// A size feature with the form it was written in.
#[derive(Clone, Debug, PartialEq)]
struct SizeFeature {
    name: SizeName,
    form: Form,
}

#[derive(Clone, Debug, PartialEq)]
enum Form {
    /// `(width)`.
    Boolean,
    /// `(width: v)`, `(min-width: v)`: the comparison and the prefix.
    Plain(Comparison, Option<&'static str>, Value),
    /// `(width < v)`.
    NameFirst(Comparison, Value),
    /// `(v < width)`: the comparison as written (value on the left).
    ValueFirst(Value, Comparison),
    /// `(v1 < width < v2)`.
    Between(Value, Comparison, Comparison, Value),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Value {
    Length(f32, LengthUnit),
    /// A multiple of the container's line height (`lh`).
    Lh(f32),
    /// A math function with absolute lengths, folded to px.
    Calc(f32),
    Zero,
    Ratio(f32, f32),
    Keyword(&'static str),
}

impl ContainerCondition {
    /// Parses `<container-condition>`: `[ <container-name> ]?
    /// <container-query>?`, not both empty. Returns `None` if the prelude is
    /// invalid (the rule is dropped).
    pub fn parse(prelude: &[ComponentValue]) -> Option<ContainerCondition> {
        let prelude = trim_whitespace(prelude);
        let mut p = Parser::new(prelude);
        let mut name = None;
        if let Some(ComponentValue::Ident(ident)) = p.peek()
            && !ident.eq_ignore_ascii_case("not")
        {
            if !is_valid_name(ident) {
                return None;
            }
            name = Some(ident.clone());
            p.next();
        }
        let query = if p.is_exhausted() {
            None
        } else {
            Some(p.parse_entirely(parse_query).ok()?)
        };
        if name.is_none() && query.is_none() {
            return None;
        }
        Some(ContainerCondition { name, query })
    }

    /// Parses a condition from text (tests and the CSSOM).
    pub fn parse_str(text: &str) -> Option<ContainerCondition> {
        Self::parse(&crate::parse_component_values(text))
    }

    /// The container name, if the condition has one.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// True if the condition has a query (not only a name).
    pub fn has_query(&self) -> bool {
        self.query.is_some()
    }

    /// The query, serialized (`containerQuery`); empty without a query.
    pub fn query_text(&self) -> String {
        let mut out = String::new();
        if let Some(query) = &self.query {
            write_query(query, &mut out);
        }
        out
    }

    /// The axes that the query reads.
    pub fn axes(&self) -> QueryAxes {
        let mut axes = QueryAxes::default();
        if let Some(query) = &self.query {
            query.collect_axes(&mut axes);
        }
        axes
    }

    /// True if the query has a `<general-enclosed>` part: no container
    /// supports it, so the condition is false (measured).
    pub fn has_unknown(&self) -> bool {
        self.query.as_ref().is_some_and(Query::has_unknown)
    }

    /// Evaluates the query against `size`; "unknown" is false. A condition
    /// without a query is true (the container was found by name).
    pub fn evaluate(&self, size: &ContainerSize) -> bool {
        self.query
            .as_ref()
            .is_none_or(|q| q.evaluate(size) == Kleene::True)
    }
}

/// True if `name` is a valid `<container-name>`: a `<custom-ident>` that is
/// not `none`, `and`, `not`, `or`, `default` (measured: `none` drops the
/// rule, `CSS.supports` rejects the others in `container-name`) or a
/// CSS-wide keyword.
pub fn is_valid_name(name: &str) -> bool {
    const RESERVED: &[&str] = &[
        "none",
        "and",
        "not",
        "or",
        "default",
        "initial",
        "inherit",
        "unset",
        "revert",
        "revert-layer",
    ];
    !RESERVED.iter().any(|r| r.eq_ignore_ascii_case(name))
}

/// `<container-query>`: `not <query-in-parens>`, or `<query-in-parens>`
/// joined by only `and` or only `or`.
fn parse_query(p: &mut Parser<'_>) -> Result<Query, ParseError> {
    if p.expect_ident_matching("not").is_ok() {
        return Ok(Query::Not(Box::new(parse_in_parens(p)?)));
    }
    let first = parse_in_parens(p)?;
    let mut rest = Vec::new();
    let mut is_and = None;
    loop {
        let and = if p.expect_ident_matching("and").is_ok() {
            true
        } else if p.expect_ident_matching("or").is_ok() {
            false
        } else {
            break;
        };
        if is_and.is_some_and(|previous| previous != and) {
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
                Query::And(rest)
            } else {
                Query::Or(rest)
            }
        }
    })
}

/// `<query-in-parens>`.
fn parse_in_parens(p: &mut Parser<'_>) -> Result<Query, ParseError> {
    match p.peek() {
        Some(ComponentValue::Block(b)) if b.kind == BlockKind::Paren => {
            p.next();
            Ok(parse_paren_contents(&b.contents))
        }
        Some(value @ ComponentValue::Function(_)) => {
            let text = value.to_string();
            p.next();
            Ok(Query::Unknown(text.into()))
        }
        Some(_) => Err(ParseError::Unexpected),
        None => Err(ParseError::EndOfInput),
    }
}

fn parse_paren_contents(contents: &[ComponentValue]) -> Query {
    if let Ok(query) = Parser::new(contents).parse_entirely(parse_query) {
        return Query::Group(Box::new(query));
    }
    let contents = trim_whitespace(contents);
    parse_feature(contents).map_or_else(
        || {
            let text = serialize_component_values(contents);
            Query::Unknown(format!("({text})").into())
        },
        Query::Feature,
    )
}

fn parse_feature(contents: &[ComponentValue]) -> Option<SizeFeature> {
    if let [ComponentValue::Ident(name)] = contents {
        let (name, prefix) = SizeName::lookup(name)?;
        return prefix.is_none().then_some(SizeFeature {
            name,
            form: Form::Boolean,
        });
    }
    if let [ComponentValue::Ident(name), rest @ ..] = contents
        && let [ComponentValue::Colon, value @ ..] = trim_whitespace(rest)
    {
        let (name, prefix) = SizeName::lookup(name)?;
        let value = parse_value(name, trim_whitespace(value))?;
        let prefix_text = prefix.map(|c| if c == Comparison::Ge { "min-" } else { "max-" });
        return Some(SizeFeature {
            name,
            form: Form::Plain(prefix.unwrap_or(Comparison::Eq), prefix_text, value),
        });
    }
    parse_range(contents)
}

fn parse_range(contents: &[ComponentValue]) -> Option<SizeFeature> {
    let (parts, operators) = split_at_comparisons(contents);
    let feature_name = |part: &[ComponentValue]| match part {
        [ComponentValue::Ident(name)] => match SizeName::lookup(name) {
            Some((feature, None)) if feature.is_range() => Some(feature),
            _ => None,
        },
        _ => None,
    };
    match (&parts[..], &operators[..]) {
        ([left, right], [op]) => {
            if let Some(name) = feature_name(left) {
                let value = parse_value(name, right)?;
                return Some(SizeFeature {
                    name,
                    form: Form::NameFirst(*op, value),
                });
            }
            let name = feature_name(right)?;
            let value = parse_value(name, left)?;
            Some(SizeFeature {
                name,
                form: Form::ValueFirst(value, *op),
            })
        }
        ([low, middle, high], [op1, op2]) => {
            let is_lt = |op: Comparison| matches!(op, Comparison::Lt | Comparison::Le);
            let is_gt = |op: Comparison| matches!(op, Comparison::Gt | Comparison::Ge);
            if !((is_lt(*op1) && is_lt(*op2)) || (is_gt(*op1) && is_gt(*op2))) {
                return None;
            }
            let name = feature_name(middle)?;
            let low = parse_value(name, low)?;
            let high = parse_value(name, high)?;
            Some(SizeFeature {
                name,
                form: Form::Between(low, *op1, *op2, high),
            })
        }
        _ => None,
    }
}

fn parse_value(name: SizeName, input: &[ComponentValue]) -> Option<Value> {
    let mut p = Parser::new(input);
    let value = match name {
        SizeName::AspectRatio => {
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
        SizeName::Orientation => {
            let ident = p.expect_ident().ok()?;
            let keyword = ["portrait", "landscape"]
                .into_iter()
                .find(|k| k.eq_ignore_ascii_case(ident))?;
            Value::Keyword(keyword)
        }
        _ => match p.next()? {
            ComponentValue::Dimension { number, unit } if unit.eq_ignore_ascii_case("lh") => {
                Value::Lh(number.value)
            }
            ComponentValue::Dimension { number, unit } => {
                Value::Length(number.value, condition_unit(unit)?)
            }
            ComponentValue::Number(n) if n.value == 0.0 => Value::Zero,
            // Math functions with absolute lengths only; relative units in
            // them make the feature `<general-enclosed>` (departure: rare).
            ComponentValue::Function(f) => match math_function(&f.name, &f.arguments, None)? {
                MathValue::Px(px) => Value::Calc(px),
                MathValue::Number(_) => return None,
            },
            _ => return None,
        },
    };
    p.is_exhausted().then_some(value)
}

/// A length unit of a query value. The container query units use the
/// viewport (departure: CSS Containment 3 resolves them against the query
/// container's own query container; measured only without one, where the
/// two agree: `condition-units`).
fn condition_unit(unit: &str) -> Option<LengthUnit> {
    let viewport = match unit.to_ascii_lowercase().as_str() {
        "cqw" | "cqi" => LengthUnit::Vw,
        "cqh" | "cqb" => LengthUnit::Vh,
        "cqmin" => LengthUnit::Vmin,
        "cqmax" => LengthUnit::Vmax,
        _ => return LengthUnit::from_name(unit),
    };
    Some(viewport)
}

impl Query {
    fn collect_axes(&self, axes: &mut QueryAxes) {
        match self {
            Query::Not(q) | Query::Group(q) => q.collect_axes(axes),
            Query::And(list) | Query::Or(list) => {
                for q in list {
                    q.collect_axes(axes);
                }
            }
            Query::Feature(f) => match f.name {
                SizeName::Width => axes.width = true,
                SizeName::Height => axes.height = true,
                SizeName::InlineSize => axes.inline = true,
                SizeName::BlockSize => axes.block = true,
                SizeName::AspectRatio | SizeName::Orientation => {
                    axes.width = true;
                    axes.height = true;
                }
            },
            Query::Unknown(_) => {}
        }
    }

    fn has_unknown(&self) -> bool {
        match self {
            Query::Not(q) | Query::Group(q) => q.has_unknown(),
            Query::And(list) | Query::Or(list) => list.iter().any(Query::has_unknown),
            Query::Feature(_) => false,
            Query::Unknown(_) => true,
        }
    }

    fn evaluate(&self, size: &ContainerSize) -> Kleene {
        match self {
            Query::Not(q) => q.evaluate(size).not(),
            Query::And(list) => list
                .iter()
                .fold(Kleene::True, |acc, q| acc.and(q.evaluate(size))),
            Query::Or(list) => list
                .iter()
                .fold(Kleene::False, |acc, q| acc.or(q.evaluate(size))),
            Query::Group(q) => q.evaluate(size),
            Query::Feature(f) => f.evaluate(size),
            Query::Unknown(_) => Kleene::Unknown,
        }
    }
}

/// The value of a feature on the container.
enum Actual {
    Px(f32),
    Ratio(f32, f32),
    Keyword(&'static str),
}

impl SizeFeature {
    fn actual(&self, size: &ContainerSize) -> Option<Actual> {
        let (inline, block) = if size.vertical {
            (size.height, size.width)
        } else {
            (size.width, size.height)
        };
        Some(match self.name {
            SizeName::Width => Actual::Px(size.width?),
            SizeName::Height => Actual::Px(size.height?),
            SizeName::InlineSize => Actual::Px(inline?),
            SizeName::BlockSize => Actual::Px(block?),
            SizeName::AspectRatio => Actual::Ratio(size.width?, size.height?),
            SizeName::Orientation => {
                let (w, h) = (size.width?, size.height?);
                Actual::Keyword(if h >= w { "portrait" } else { "landscape" })
            }
        })
    }

    fn evaluate(&self, size: &ContainerSize) -> Kleene {
        let Some(actual) = self.actual(size) else {
            return Kleene::Unknown;
        };
        let test = |op: Comparison, value: Value| op.test(compare(&actual, value, size));
        Kleene::from_bool(match self.form {
            Form::Boolean => match actual {
                Actual::Px(v) => v != 0.0,
                // A zero height gives an infinite ratio (measured in
                // Chromium 148, `edge-values`: true); 0 / 0 is not.
                Actual::Ratio(w, _) => w != 0.0,
                Actual::Keyword(_) => true,
            },
            Form::Plain(op, _, value) | Form::NameFirst(op, value) => test(op, value),
            Form::ValueFirst(value, op) => test(op.flip(), value),
            Form::Between(low, op1, op2, high) => test(op1.flip(), low) && test(op2, high),
        })
    }
}

fn compare(actual: &Actual, value: Value, size: &ContainerSize) -> Option<std::cmp::Ordering> {
    match (actual, value) {
        (Actual::Px(a), Value::Length(v, unit)) => a.partial_cmp(&length_px(v, unit, size)),
        (Actual::Px(a), Value::Lh(v)) => a.partial_cmp(&(v * size.line_height)),
        (Actual::Px(a), Value::Calc(v)) => a.partial_cmp(&v),
        (Actual::Px(a), Value::Zero) => a.partial_cmp(&0.0),
        (Actual::Ratio(w, h), Value::Ratio(n, d)) => (w * d).partial_cmp(&(n * h)),
        (Actual::Keyword(a), Value::Keyword(v)) => (*a == v).then_some(std::cmp::Ordering::Equal),
        _ => None,
    }
}

/// A length of a query in px: `em`, `ex` and `ch` use the container's font
/// size (measured: `condition-units`), `rem` the root's, the viewport units
/// the viewport. `ex` and `ch` are half an `em`, as in media queries.
fn length_px(value: f32, unit: LengthUnit, size: &ContainerSize) -> f32 {
    let (w, h) = (size.viewport_width, size.viewport_height);
    let (multiplier, divisor) = match unit {
        LengthUnit::Px => (1.0, 1.0),
        LengthUnit::Em => (size.font_size, 1.0),
        LengthUnit::Rem => (size.root_font_size, 1.0),
        LengthUnit::Ex | LengthUnit::Ch => (size.font_size, 2.0),
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

impl fmt::Display for ContainerCondition {
    /// `conditionText`: the name and the query, separated by a space.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        if let Some(name) = &self.name {
            serialize_identifier(name, &mut out);
        }
        if let Some(query) = &self.query {
            if !out.is_empty() {
                out.push(' ');
            }
            write_query(query, &mut out);
        }
        f.write_str(&out)
    }
}

fn write_query(query: &Query, out: &mut String) {
    let join = |list: &[Query], word: &str, out: &mut String| {
        for (i, q) in list.iter().enumerate() {
            if i > 0 {
                out.push_str(word);
            }
            write_query(q, out);
        }
    };
    match query {
        Query::Not(q) => {
            out.push_str("not ");
            write_query(q, out);
        }
        Query::And(list) => join(list, " and ", out),
        Query::Or(list) => join(list, " or ", out),
        Query::Group(q) => {
            out.push('(');
            write_query(q, out);
            out.push(')');
        }
        Query::Feature(feature) => {
            out.push('(');
            write_feature(feature, out);
            out.push(')');
        }
        Query::Unknown(text) => out.push_str(text),
    }
}

fn write_feature(feature: &SizeFeature, out: &mut String) {
    let name = feature.name.name();
    match feature.form {
        Form::Boolean => out.push_str(name),
        Form::Plain(_, prefix, value) => {
            out.push_str(prefix.unwrap_or(""));
            out.push_str(name);
            out.push_str(": ");
            write_value(value, out);
        }
        Form::NameFirst(op, value) => {
            let _ = write!(out, "{name} {} ", op.as_str());
            write_value(value, out);
        }
        Form::ValueFirst(value, op) => {
            write_value(value, out);
            let _ = write!(out, " {} {name}", op.as_str());
        }
        Form::Between(low, op1, op2, high) => {
            write_value(low, out);
            let _ = write!(out, " {} {name} {} ", op1.as_str(), op2.as_str());
            write_value(high, out);
        }
    }
}

fn write_value(value: Value, out: &mut String) {
    let _ = match value {
        Value::Length(v, unit) => write!(out, "{v}{}", unit.name()),
        Value::Lh(v) => write!(out, "{v}lh"),
        Value::Calc(v) => write!(out, "calc({v}px)"),
        Value::Zero => write!(out, "0"),
        Value::Ratio(n, d) => write!(out, "{n} / {d}"),
        Value::Keyword(k) => write!(out, "{k}"),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(width: Option<f32>, height: Option<f32>) -> ContainerSize {
        ContainerSize {
            width,
            height,
            vertical: false,
            font_size: 10.0,
            line_height: 12.0,
            root_font_size: 16.0,
            viewport_width: 800.0,
            viewport_height: 600.0,
        }
    }

    fn eval(text: &str, s: &ContainerSize) -> bool {
        ContainerCondition::parse_str(text)
            .unwrap_or_else(|| panic!("{text} parses"))
            .evaluate(s)
    }

    /// The queries of the `features-size` and `logic` probe cases on a
    /// 300 x 200 container, with Chromium 148's results.
    #[test]
    fn size_features_and_logic_match_chromium() {
        let s = size(Some(300.0), Some(200.0));
        let cases = [
            ("(width: 300px)", true),
            ("(min-width: 300px)", true),
            ("(max-width: 299px)", false),
            ("(width > 299.5px)", true),
            ("(250px < width <= 300px)", true),
            ("(height: 200px)", true),
            ("(inline-size: 300px)", true),
            ("(block-size: 200px)", true),
            ("(aspect-ratio: 3/2)", true),
            ("(orientation: landscape)", true),
            ("(orientation: portrait)", false),
            ("(min-aspect-ratio: 1/1)", true),
            ("(width >= 20em)", true),
            ("(width > 18.75rem)", false),
            ("(width > calc(100px * 2))", true),
            ("(width < 50vw)", true),
            ("(300px = width)", true),
            ("(min-height: 200.5px)", false),
            ("(max-aspect-ratio: 1.5)", true),
            ("(aspect-ratio > 1)", true),
            ("(width > 100px) and (width < 400px)", true),
            ("(width < 100px) or (width > 200px)", true),
            ("not (width < 100px)", true),
            (
                "((width > 100px) and (height > 1000px)) or (width = 300px)",
                true,
            ),
            ("(width)", true),
            ("(width: 0)", false),
            ("((width > 100px))", true),
            ("(WIDTH > 100px)", true),
            ("(width >= 300px) and (not (height > 100px))", false),
        ];
        for (text, expected) in cases {
            assert_eq!(eval(text, &s), expected, "{text}");
        }
    }

    #[test]
    fn em_uses_the_container_font_size() {
        // `condition-units`: font size 10px, width 300px.
        let s = size(Some(300.0), None);
        assert!(eval("(width > 25em)", &s));
        assert!(!eval("(width > 20rem)", &s));
        assert!(eval("(width = 30em)", &s));
        // Container units in a condition use the viewport (800 x 600).
        assert!(eval("(width > 10cqw)", &s));
        assert!(!eval("(width > 60cqh)", &s));
    }

    /// The `edge-values` probe case: a 300 x 0 container.
    #[test]
    fn edge_values_match_chromium() {
        let s = size(Some(300.0), Some(0.0));
        let cases = [
            ("(aspect-ratio)", true),
            ("(aspect-ratio > 1000)", true),
            ("(aspect-ratio: 1/0)", true),
            ("(width < calc(1px / 0))", true),
            ("(width > calc(0px * infinity))", true),
            ("(width < calc(NaN * 1px))", false),
            ("(width > 1e40px)", false),
            ("(width > 1lh)", true),
            ("(width > 30lh)", false),
            ("(height)", false),
        ];
        for (text, expected) in cases {
            assert_eq!(eval(text, &s), expected, "{text}");
        }
        let flat = size(Some(0.0), Some(0.0));
        assert!(!eval("(aspect-ratio)", &flat), "0 / 0");
        let tall = size(Some(0.0), Some(10.0));
        assert!(!eval("(aspect-ratio)", &tall), "0 / 10");
    }

    #[test]
    fn unknown_sizes_and_general_enclosed() {
        let s = size(Some(300.0), None);
        assert!(!eval("(height > 0px)", &s));
        assert!(!eval("not (height > 0px)", &s));
        assert!(eval("(height > 0px) or (width > 0px)", &s));
        let c = ContainerCondition::parse_str("(width > 100px) or (foo: 1)").unwrap();
        assert!(c.has_unknown());
        assert!(
            ContainerCondition::parse_str("style(--x: 1)")
                .unwrap()
                .has_unknown()
        );
        assert!(
            !ContainerCondition::parse_str("(width > 1px)")
                .unwrap()
                .has_unknown()
        );
        // A unitless length is not a feature.
        assert!(
            ContainerCondition::parse_str("(width > 100)")
                .unwrap()
                .has_unknown()
        );
    }

    #[test]
    fn invalid_preludes_drop_the_rule() {
        for text in [
            "",
            "(width > 100px) and (height > 50px) or (width > 0px)",
            "width > 100px",
            "none (width > 1px)",
            "a b (width > 1px)",
            "screen and (width > 100px)",
            "and",
            "card (width > 1px) junk",
        ] {
            assert!(ContainerCondition::parse_str(text).is_none(), "{text}");
        }
    }

    #[test]
    fn names_and_axes() {
        let c = ContainerCondition::parse_str("card").unwrap();
        assert_eq!(c.name(), Some("card"));
        assert!(!c.has_query());
        assert!(c.evaluate(&size(None, None)));
        let c = ContainerCondition::parse_str("Card not (width < 300px)").unwrap();
        assert_eq!(c.name(), Some("Card"));
        assert_eq!(
            c.axes(),
            QueryAxes {
                width: true,
                ..QueryAxes::default()
            }
        );
        let axes = ContainerCondition::parse_str("(orientation: portrait)")
            .unwrap()
            .axes();
        assert_eq!(axes.logical(false), (true, true));
        let axes = ContainerCondition::parse_str("(width > 0px)")
            .unwrap()
            .axes();
        assert_eq!(axes.logical(false), (true, false));
        assert_eq!(axes.logical(true), (false, true));
        let axes = ContainerCondition::parse_str("(block-size > 0px)")
            .unwrap()
            .axes();
        assert_eq!(axes.logical(true), (false, true));
    }

    /// `cssom-container-rule`: Chromium's `conditionText`.
    #[test]
    fn serialization_matches_chromium() {
        let cases = [
            ("card (min-width:300px)", "card (min-width: 300px)"),
            (
                "(width>=300px) and (height<10em)",
                "(width >= 300px) and (height < 10em)",
            ),
            ("  x   not (width > 1px)", "x not (width > 1px)"),
            (
                "(orientation:landscape) or (aspect-ratio:16/9)",
                "(orientation: landscape) or (aspect-ratio: 16 / 9)",
            ),
            ("(foo:bar)", "(foo:bar)"),
            ("( WIDTH > 1PX )", "(width > 1px)"),
            ("card", "card"),
            ("(100px<width<=200px)", "(100px < width <= 200px)"),
            ("(calc(1px + 2px) < width)", "(calc(3px) < width)"),
            ("(width)", "(width)"),
        ];
        for (input, expected) in cases {
            let c = ContainerCondition::parse_str(input).unwrap();
            assert_eq!(c.to_string(), expected, "{input}");
        }
        let c = ContainerCondition::parse_str("card (min-width:300px)").unwrap();
        assert_eq!(c.query_text(), "(min-width: 300px)");
        let c = ContainerCondition::parse_str("card").unwrap();
        assert_eq!(c.query_text(), "");
    }

    #[test]
    fn vertical_containers_map_logical_features() {
        let mut s = size(Some(300.0), Some(200.0));
        s.vertical = true;
        assert!(eval("(inline-size: 200px)", &s));
        assert!(eval("(block-size: 300px)", &s));
        assert!(eval("(width: 300px)", &s));
    }
}
