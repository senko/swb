//! Registered custom properties: `@property` (CSS Properties and Values API
//! 1). A registration gives a custom property a syntax, an initial value
//! and an inheritance flag. Its value is parsed against the syntax and
//! computed (lengths to px, colors to their computed form) when the custom
//! properties of an element are computed; a value that does not match is
//! invalid at computed-value time and becomes the initial value.
//!
//! Measured in Chromium 148 with `tools/probes/css-small-features.json`
//! (the `ap-*` cases); ADR 0038, section "7b".
//!
//! Supported syntaxes: `*`, `<length>`, `<number>`, `<integer>`,
//! `<percentage>`, `<length-percentage>`, `<color>`, `<angle>`, `<time>`,
//! `<resolution>`, `<custom-ident>`, `<string>`, keywords, `|`, and the
//! multipliers `+` and `#`. `<url>`, `<image>`, `<transform-function>` and
//! `<transform-list>` accept a function or `url()` value and keep it as
//! written (Chromium makes `url()` absolute and computes the lengths).
//! <https://www.w3.org/TR/css-properties-values-api-1/>

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

use swb_css::{
    ComponentValue, Declaration, Parser, PropertyRule, parse_component_values,
    serialize_component_values, serialize_string, split_on_commas, trim_whitespace,
};

use crate::cssom::format_number;
use crate::cssom::resolved::length_percentage_text;
use crate::parse::color::parse_color;
use crate::parse::length::{LengthOptions, parse_length_percentage};
use crate::parse::{ParseResult, parse_angle, parse_integer, parse_number};
use crate::properties::CssWideKeyword;
use crate::values::{Length, LengthContext, LengthPercentage, LengthUnit};

/// A data type of a syntax component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DataType {
    Length,
    Number,
    Percentage,
    LengthPercentage,
    Color,
    Image,
    Url,
    Integer,
    Angle,
    Time,
    Resolution,
    TransformFunction,
    TransformList,
    CustomIdent,
    String,
}

impl DataType {
    fn from_name(name: &str) -> Option<DataType> {
        Some(match name {
            "length" => DataType::Length,
            "number" => DataType::Number,
            "percentage" => DataType::Percentage,
            "length-percentage" => DataType::LengthPercentage,
            "color" => DataType::Color,
            "image" => DataType::Image,
            "url" => DataType::Url,
            "integer" => DataType::Integer,
            "angle" => DataType::Angle,
            "time" => DataType::Time,
            "resolution" => DataType::Resolution,
            "transform-function" => DataType::TransformFunction,
            "transform-list" => DataType::TransformList,
            "custom-ident" => DataType::CustomIdent,
            "string" => DataType::String,
            _ => return None,
        })
    }
}

/// A syntax component: a data type or a keyword.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Component {
    Type(DataType),
    Keyword(String),
}

/// How often a component may repeat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Multiplier {
    One,
    /// `+`: a space-separated list.
    Space,
    /// `#`: a comma-separated list.
    Comma,
}

/// One alternative of a syntax: a component and its multiplier.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Term {
    pub(crate) component: Component,
    pub(crate) multiplier: Multiplier,
}

/// A syntax string: `*` or alternatives.
/// <https://www.w3.org/TR/css-properties-values-api-1/#syntax-strings>
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Syntax {
    Universal,
    Terms(Vec<Term>),
}

/// The most terms of a syntax (hostile `syntax` strings).
const MAX_TERMS: usize = 256;

impl Syntax {
    /// Parses a syntax string; `None` if it is not valid.
    pub(crate) fn parse(text: &str) -> Option<Syntax> {
        let text = text.trim_matches(|c: char| c.is_ascii_whitespace());
        if text == "*" {
            return Some(Syntax::Universal);
        }
        let mut terms = Vec::new();
        for part in text.split('|') {
            if terms.len() >= MAX_TERMS {
                return None;
            }
            terms.push(parse_term(
                part.trim_matches(|c: char| c.is_ascii_whitespace()),
            )?);
        }
        Some(Syntax::Terms(terms))
    }

    /// The computed value of `tokens` (trimmed, `var()` substituted), or
    /// `None` if they do not match the syntax.
    pub(crate) fn compute(
        &self,
        tokens: &[ComponentValue],
        lengths: &LengthContext,
    ) -> Option<Vec<ComponentValue>> {
        let tokens = trim_whitespace(tokens);
        let Syntax::Terms(terms) = self else {
            return Some(tokens.to_vec());
        };
        if tokens.is_empty() {
            return None;
        }
        terms
            .iter()
            .find_map(|term| term.compute(tokens, lengths))
            .map(|text| parse_component_values(&text))
    }
}

fn parse_term(part: &str) -> Option<Term> {
    if let Some(rest) = part.strip_prefix('<') {
        let (name, tail) = rest.split_once('>')?;
        let ty = DataType::from_name(name)?;
        let multiplier = match tail {
            "" => Multiplier::One,
            "+" if ty != DataType::TransformList => Multiplier::Space,
            "#" if ty != DataType::TransformList => Multiplier::Comma,
            _ => return None,
        };
        return Some(Term {
            component: Component::Type(ty),
            multiplier,
        });
    }
    let name_start = |c: char| c.is_ascii_alphabetic() || c == '_' || !c.is_ascii();
    let mut chars = part.chars();
    let first_ok = match chars.next() {
        Some(c) if name_start(c) => true,
        Some('-') => chars.next().is_some_and(|c| name_start(c) || c == '-'),
        _ => false,
    };
    let valid = first_ok
        && !part.starts_with("--")
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || !c.is_ascii());
    let reserved = [
        "initial",
        "inherit",
        "unset",
        "revert",
        "revert-layer",
        "default",
    ];
    if !valid || reserved.iter().any(|r| part.eq_ignore_ascii_case(r)) {
        return None;
    }
    Some(Term {
        component: Component::Keyword(part.to_owned()),
        multiplier: Multiplier::One,
    })
}

impl Term {
    /// The computed text of `tokens` for this term, or `None` if they do
    /// not match it.
    pub(crate) fn compute(
        &self,
        tokens: &[ComponentValue],
        lengths: &LengthContext,
    ) -> Option<String> {
        match self.multiplier {
            Multiplier::One => self.component.compute(tokens, lengths),
            Multiplier::Comma => {
                let mut items = Vec::new();
                for part in split_on_commas(tokens) {
                    items.push(self.component.compute(trim_whitespace(part), lengths)?);
                }
                Some(items.join(", "))
            }
            Multiplier::Space => {
                let mut items = Vec::new();
                for part in space_items(tokens) {
                    items.push(self.component.compute(part, lengths)?);
                }
                (!items.is_empty()).then(|| items.join(" "))
            }
        }
    }
}

/// The items of a space-separated list: the runs between white space (a
/// function is one item).
pub(crate) fn space_items(tokens: &[ComponentValue]) -> Vec<&[ComponentValue]> {
    tokens
        .split(ComponentValue::is_whitespace)
        .filter(|part| !part.is_empty())
        .collect()
}

impl Component {
    fn compute(&self, tokens: &[ComponentValue], lengths: &LengthContext) -> Option<String> {
        match self {
            Component::Keyword(word) => match tokens {
                [ComponentValue::Ident(ident)] if ident.eq_ignore_ascii_case(word) => {
                    Some(word.clone())
                }
                _ => None,
            },
            Component::Type(ty) => {
                let mut p = Parser::new(tokens);
                p.parse_entirely(|p| compute_type(*ty, p, lengths)).ok()
            }
        }
    }
}

/// A number as Chromium writes it (six significant digits).
fn number(v: f32) -> String {
    format_number(f64::from(v))
}

/// The computed value of one component of type `ty`.
#[allow(clippy::too_many_lines)] // One arm per data type.
fn compute_type(ty: DataType, p: &mut Parser<'_>, lengths: &LengthContext) -> ParseResult<String> {
    use swb_css::ParseError::Invalid;
    Ok(match ty {
        DataType::Length => {
            let v = parse_length_percentage(p, LengthOptions::LENGTH)?.compute(lengths);
            match v {
                LengthPercentage::Px(px) => format!("{}px", number(px)),
                _ => return Err(Invalid),
            }
        }
        DataType::LengthPercentage => length_percentage_text(
            &parse_length_percentage(p, LengthOptions::LENGTH_PERCENTAGE)?.compute(lengths),
        ),
        DataType::Percentage => {
            let v = parse_length_percentage(p, LengthOptions::LENGTH_PERCENTAGE)?.compute(lengths);
            let pure = match &v {
                LengthPercentage::Px(_) => false,
                LengthPercentage::Percent(_) => true,
                LengthPercentage::Calc(_) => v.resolve(0.0) == 0.0,
            };
            if !pure {
                return Err(Invalid);
            }
            length_percentage_text(&v)
        }
        DataType::Number => number(parse_number(p)?),
        DataType::Integer => parse_integer(p)?.to_string(),
        DataType::Color => parse_color(p)?.computed_text(),
        DataType::Angle => format!("{}deg", number(parse_angle(p, false)?)),
        DataType::Time => match p.next() {
            Some(ComponentValue::Dimension { number: n, unit })
                if unit.eq_ignore_ascii_case("s") =>
            {
                format!("{}s", number(n.value))
            }
            Some(ComponentValue::Dimension { number: n, unit })
                if unit.eq_ignore_ascii_case("ms") =>
            {
                format!("{}s", number(n.value / 1000.0))
            }
            _ => return Err(Invalid),
        },
        DataType::Resolution => match p.next() {
            Some(ComponentValue::Dimension { number: n, unit }) => {
                let dppx = match unit.to_ascii_lowercase().as_str() {
                    "dppx" | "x" => n.value,
                    "dpi" => n.value / 96.0,
                    "dpcm" => n.value * 2.54 / 96.0,
                    _ => return Err(Invalid),
                };
                if n.value < 0.0 {
                    return Err(Invalid);
                }
                format!("{}dppx", number(dppx))
            }
            _ => return Err(Invalid),
        },
        DataType::CustomIdent => match p.next() {
            Some(ComponentValue::Ident(ident))
                if CssWideKeyword::from_value(&[ComponentValue::Ident(ident.clone())])
                    .is_none()
                    && !ident.eq_ignore_ascii_case("default") =>
            {
                let mut out = String::new();
                swb_css::serialize_identifier(ident, &mut out);
                out
            }
            _ => return Err(Invalid),
        },
        DataType::String => match p.next() {
            Some(ComponentValue::String(s)) => {
                let mut out = String::new();
                serialize_string(s, &mut out);
                out
            }
            _ => return Err(Invalid),
        },
        DataType::TransformFunction | DataType::TransformList => {
            let rest = p.remaining();
            let items: Vec<&ComponentValue> = rest.iter().filter(|v| !v.is_whitespace()).collect();
            let functions = !items.is_empty()
                && items.iter().all(
                    |v| matches!(v, ComponentValue::Function(f) if is_transform_function(&f.name)),
                );
            let none = matches!(items.as_slice(), [ComponentValue::Ident(i)] if i.eq_ignore_ascii_case("none"));
            let ok = if ty == DataType::TransformFunction {
                items.len() == 1 && functions
            } else {
                functions || none
            };
            if !ok {
                return Err(Invalid);
            }
            while p.next().is_some() {}
            let computed: Vec<ComponentValue> =
                rest.iter().map(|v| with_px_lengths(v, lengths)).collect();
            serialize_component_values(&computed)
        }
        DataType::Url | DataType::Image => {
            let rest = p.remaining();
            let ok = match ty {
                DataType::Url => {
                    matches!(rest, [ComponentValue::Url(_)])
                        || matches!(rest, [ComponentValue::Function(f)] if f.name.eq_ignore_ascii_case("url"))
                }
                DataType::Image => {
                    rest.iter()
                        .all(|v| matches!(v, ComponentValue::Url(_) | ComponentValue::Function(_)))
                        && !rest.is_empty()
                }
                _ => false,
            };
            if !ok {
                return Err(Invalid);
            }
            while p.next().is_some() {}
            serialize_component_values(rest)
        }
    })
}

/// The transform functions of CSS Transforms 1 and 2.
fn is_transform_function(name: &str) -> bool {
    [
        "matrix",
        "matrix3d",
        "translate",
        "translatex",
        "translatey",
        "translatez",
        "translate3d",
        "scale",
        "scalex",
        "scaley",
        "scalez",
        "scale3d",
        "rotate",
        "rotatex",
        "rotatey",
        "rotatez",
        "rotate3d",
        "skew",
        "skewx",
        "skewy",
        "perspective",
    ]
    .iter()
    .any(|f| f.eq_ignore_ascii_case(name))
}

/// `value` with its lengths in px (a transform function's arguments).
fn with_px_lengths(value: &ComponentValue, lengths: &LengthContext) -> ComponentValue {
    match value {
        ComponentValue::Dimension { number, unit } => match LengthUnit::from_name(unit) {
            Some(unit) => {
                let px = Length {
                    value: number.value,
                    unit,
                }
                .to_px(lengths);
                ComponentValue::Dimension {
                    number: swb_css::Number {
                        value: px,
                        value_f64: f64::from(px),
                        int_value: (px.fract() == 0.0 && px.abs() < 1e9).then_some(px as i32),
                        has_sign: false,
                    },
                    unit: "px".into(),
                }
            }
            None => value.clone(),
        },
        ComponentValue::Function(f) => ComponentValue::Function(swb_css::Function {
            name: f.name.clone(),
            arguments: f
                .arguments
                .iter()
                .map(|v| with_px_lengths(v, lengths))
                .collect(),
        }),
        other => other.clone(),
    }
}

/// A registered custom property.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Registration {
    /// The name (`--foo`).
    pub(crate) name: Arc<str>,
    /// The syntax string as written.
    pub(crate) syntax_text: String,
    pub(crate) syntax: Syntax,
    pub(crate) inherits: bool,
    /// The initial value as written (`None` if the descriptor is missing:
    /// only for syntax `*`).
    pub(crate) initial_text: Option<Vec<ComponentValue>>,
    /// The computed initial value (`None`: the guaranteed-invalid value).
    pub(crate) initial: Option<Arc<[ComponentValue]>>,
}

/// True if `tokens` are not computationally independent: they have `var()`
/// or a font-relative or container-relative unit.
/// <https://www.w3.org/TR/css-properties-values-api-1/#computationally-independent>
fn depends_on_context(tokens: &[ComponentValue]) -> bool {
    tokens.iter().any(|v| match v {
        ComponentValue::Dimension { unit, .. } => {
            let unit = unit.to_ascii_lowercase();
            matches!(
                unit.as_str(),
                "em" | "ex"
                    | "cap"
                    | "ch"
                    | "ic"
                    | "lh"
                    | "rem"
                    | "rex"
                    | "rcap"
                    | "rch"
                    | "ric"
                    | "rlh"
            ) || unit.starts_with("cq")
        }
        ComponentValue::Function(f) => {
            f.name.eq_ignore_ascii_case("var") || depends_on_context(&f.arguments)
        }
        ComponentValue::Block(b) => depends_on_context(&b.contents),
        _ => false,
    })
}

fn descriptor<'a>(declarations: &'a [Declaration], name: &str) -> Option<&'a Declaration> {
    declarations.iter().rev().find(|d| d.name == name)
}

impl Registration {
    /// The registration of an `@property` rule, or `None` if the rule is
    /// not valid (it is then not in the CSSOM either).
    /// <https://www.w3.org/TR/css-properties-values-api-1/#the-syntax-descriptor>
    pub(crate) fn from_rule(rule: &PropertyRule) -> Option<Registration> {
        let declarations = &rule.declarations;
        if declarations.iter().any(|d| d.important) {
            return None;
        }
        let syntax_text = match descriptor(declarations, "syntax")?.value.as_slice() {
            [ComponentValue::String(s)] => s.to_string(),
            _ => return None,
        };
        let syntax = Syntax::parse(&syntax_text)?;
        let inherits = match descriptor(declarations, "inherits")?.value.as_slice() {
            [ComponentValue::Ident(i)] if i.eq_ignore_ascii_case("true") => true,
            [ComponentValue::Ident(i)] if i.eq_ignore_ascii_case("false") => false,
            _ => return None,
        };
        let initial_text = descriptor(declarations, "initial-value").map(|d| d.value.clone());
        let initial = match (&syntax, &initial_text) {
            (Syntax::Universal, None) => None,
            (Syntax::Universal, Some(tokens)) => {
                let tokens = trim_whitespace(tokens);
                (!tokens.is_empty()).then(|| Arc::from(tokens))
            }
            (_, None) => return None,
            (_, Some(tokens)) => {
                if depends_on_context(tokens) {
                    return None;
                }
                let computed = syntax.compute(tokens, &LengthContext::DEFAULT)?;
                Some(Arc::from(computed))
            }
        };
        Some(Registration {
            name: Arc::from(rule.name.as_str()),
            syntax_text,
            syntax,
            inherits,
            initial_text,
            initial,
        })
    }

    /// True for the syntax `*`.
    pub(crate) fn is_universal(&self) -> bool {
        self.syntax == Syntax::Universal
    }

    /// The text of the rule as Chromium writes it.
    pub(crate) fn css_text(&self) -> String {
        let mut out = format!("@property {} {{ syntax: ", self.name);
        serialize_string(&self.syntax_text, &mut out);
        let _ = write!(out, "; inherits: {};", self.inherits);
        if let Some(text) = &self.initial_text {
            let value = serialize_component_values(trim_whitespace(text));
            out.push_str(" initial-value: ");
            out.push_str(&value);
            out.push(';');
        }
        out.push_str(" }");
        out
    }

    /// The computed value of the declared `tokens` (`var()` substituted),
    /// or the initial value if they do not match the syntax.
    pub(crate) fn compute(
        &self,
        tokens: &[ComponentValue],
        lengths: &LengthContext,
    ) -> Option<Arc<[ComponentValue]>> {
        match self.syntax.compute(tokens, lengths) {
            Some(value) => Some(Arc::from(value)),
            None => self.initial.clone(),
        }
    }
}

/// The registrations that apply to a document: the last valid rule of a
/// name wins.
#[derive(Debug, Default)]
pub(crate) struct Registry {
    by_name: HashMap<Arc<str>, Registration>,
}

impl Registry {
    /// The registry of `registrations` in document order.
    pub(crate) fn new<'a>(registrations: impl Iterator<Item = &'a Registration>) -> Registry {
        let mut by_name: HashMap<Arc<str>, Registration> = HashMap::new();
        for registration in registrations {
            by_name.insert(Arc::clone(&registration.name), registration.clone());
        }
        Registry { by_name }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    pub(crate) fn get(&self, name: &str) -> Option<&Registration> {
        self.by_name.get(name)
    }

    /// The names of the registered properties that have an initial value.
    pub(crate) fn initial_names(&self) -> impl Iterator<Item = &Arc<str>> {
        self.by_name
            .values()
            .filter(|r| r.initial.is_some())
            .map(|r| &r.name)
    }

    /// The initial value of the registered property `name`.
    pub(crate) fn initial(&self, name: &str) -> Option<&Arc<[ComponentValue]>> {
        self.by_name.get(name).and_then(|r| r.initial.as_ref())
    }

    /// True if any of `names` is registered.
    pub(crate) fn declares_any<'a>(&self, mut names: impl Iterator<Item = &'a Arc<str>>) -> bool {
        names.any(|n| self.by_name.contains_key(&**n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_css::{CssRule, parse_stylesheet};

    fn registration(css: &str) -> Option<Registration> {
        let sheet = parse_stylesheet(css);
        match sheet.rules.first() {
            Some(CssRule::Property(rule)) => Registration::from_rule(rule),
            _ => None,
        }
    }

    fn compute(syntax: &str, value: &str) -> Option<String> {
        let syntax = Syntax::parse(syntax)?;
        let lengths = LengthContext {
            font_size: 10.0,
            ..LengthContext::DEFAULT
        };
        syntax
            .compute(&parse_component_values(value), &lengths)
            .map(|v| serialize_component_values(&v))
    }

    #[test]
    fn syntax_strings() {
        assert_eq!(Syntax::parse(" * "), Some(Syntax::Universal));
        assert!(Syntax::parse("<length> | <percentage>").is_some());
        assert!(Syntax::parse("<length>+ | auto").is_some());
        assert!(Syntax::parse("<length>#").is_some());
        for bad in [
            "",
            "<length> <length>",
            "<foo>",
            "<length>++",
            "<transform-list>+",
            "<length>|",
            "3px",
            "initial",
            "*|auto",
        ] {
            assert_eq!(Syntax::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn lengths_are_absolute() {
        assert_eq!(compute("<length>", "2em").as_deref(), Some("20px"));
        assert_eq!(
            compute("<length>", "calc(1px + 2px)").as_deref(),
            Some("3px")
        );
        assert_eq!(compute("<length>", "1in").as_deref(), Some("96px"));
        assert_eq!(compute("<length>", "0").as_deref(), Some("0px"));
        assert_eq!(compute("<length>", "1cm").as_deref(), Some("37.7953px"));
        assert_eq!(compute("<length>", "10"), None);
        assert_eq!(compute("<length>", "50%"), None);
        assert_eq!(compute("<length>", "red"), None);
        assert_eq!(compute("<length>", "1px 2px"), None);
    }

    #[test]
    fn other_types() {
        assert_eq!(compute("<number>", " 3.50 ").as_deref(), Some("3.5"));
        assert_eq!(compute("<integer>", "3.5"), None);
        assert_eq!(
            compute("<percentage>", "calc(10% + 5%)").as_deref(),
            Some("15%")
        );
        assert_eq!(compute("<percentage>", "5px"), None);
        assert_eq!(
            compute("<length-percentage>", "calc(1em + 1%)").as_deref(),
            Some("calc(1% + 10px)")
        );
        assert_eq!(
            compute("<color>", "hsl(120 100% 25%)").as_deref(),
            Some("rgb(0, 128, 0)")
        );
        assert_eq!(
            compute("<color>", "currentcolor").as_deref(),
            Some("currentcolor")
        );
        assert_eq!(compute("<angle>", "1turn").as_deref(), Some("360deg"));
        assert_eq!(compute("<time>", "500ms").as_deref(), Some("0.5s"));
        assert_eq!(compute("<resolution>", "96dpi").as_deref(), Some("1dppx"));
        assert_eq!(compute("<custom-ident>", "Foo").as_deref(), Some("Foo"));
        assert_eq!(compute("<custom-ident>", "default"), None);
        assert_eq!(compute("<string>", "'c'").as_deref(), Some("\"c\""));
    }

    #[test]
    fn alternatives_and_lists() {
        assert_eq!(compute("auto | <length>", "AUTO").as_deref(), Some("auto"));
        assert_eq!(compute("auto | <length>", "2em").as_deref(), Some("20px"));
        assert_eq!(
            compute("<length>+", "1px   2em").as_deref(),
            Some("1px 20px")
        );
        assert_eq!(
            compute("<length>#", "1px,2em").as_deref(),
            Some("1px, 20px")
        );
        assert_eq!(compute("<length>#", "1px 2px"), None);
        assert_eq!(compute("*", "  anything  ").as_deref(), Some("anything"));
    }

    #[test]
    fn rule_validity() {
        let good = "@property --p { syntax: '<length>'; inherits: false; initial-value: 0 }";
        let r = registration(good).expect("valid");
        assert_eq!(
            serialize_component_values(r.initial.as_deref().expect("initial")),
            "0px"
        );
        assert_eq!(
            r.css_text(),
            "@property --p { syntax: \"<length>\"; inherits: false; initial-value: 0; }"
        );
        for bad in [
            "@property --p { syntax: '<length>'; inherits: false }",
            "@property --p { inherits: false; initial-value: 0px }",
            "@property --p { syntax: '<length>'; initial-value: 0px }",
            "@property --p { syntax: '<length>'; inherits: maybe; initial-value: 0px }",
            "@property --p { syntax: '<length>'; inherits: false; initial-value: 1em }",
            "@property --p { syntax: '<length>'; inherits: false; initial-value: red }",
            "@property --p { syntax: '<length>'; inherits: false; initial-value: 5% }",
            "@property --p { syntax: 'length'; inherits: false; initial-value: 0px }",
            "@property --p { syntax: <length>; inherits: false; initial-value: 0px }",
            "@property --p { syntax: '<length>'; inherits: false !important; initial-value: 1px }",
            "@property p { syntax: '<length>'; inherits: false; initial-value: 0px }",
            "@property -- { syntax: '<length>'; inherits: false; initial-value: 0px }",
        ] {
            assert!(registration(bad).is_none(), "{bad}");
        }
        let universal =
            registration("@property --u { syntax: '*'; inherits: true }").expect("valid");
        assert_eq!(universal.initial, None);
        assert_eq!(
            universal.css_text(),
            "@property --u { syntax: \"*\"; inherits: true; }"
        );
        assert!(
            registration(
                "@property --p { SYNTAX: '<length>'; INHERITS: FALSE; INITIAL-VALUE: 1PX }"
            )
            .is_some()
        );
    }

    #[test]
    fn the_last_registration_wins() {
        let sheet = parse_stylesheet(
            "@property --p { syntax: '<length>'; inherits: false; initial-value: 1px } \
             @property --p { syntax: '<number>'; inherits: true; initial-value: 2 }",
        );
        let regs: Vec<Registration> = sheet
            .rules
            .iter()
            .filter_map(|r| match r {
                CssRule::Property(p) => Registration::from_rule(p),
                _ => None,
            })
            .collect();
        let registry = Registry::new(regs.iter());
        assert_eq!(registry.get("--p").map(|r| r.inherits), Some(true));
        assert_eq!(registry.initial_names().count(), 1);
    }
}
