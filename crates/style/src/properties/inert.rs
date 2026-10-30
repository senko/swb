//! Properties without effect on layout or paint: `transition` and its
//! longhands (CSS Transitions 1 and 2), `will-change` (CSS Will Change 1),
//! `touch-action` (Pointer Events 3), `perspective` and
//! `perspective-origin` (CSS Transforms 2), with the `-webkit-` aliases that
//! Chromium has.
//!
//! swb parses them with their grammar, cascades and computes them, and
//! shows them to scripts (`element.style`, `getComputedStyle`,
//! `CSS.supports`, feature tests such as `'transition' in element.style`),
//! but runs no transitions and draws no perspective. Pages test and write
//! these properties (Leaflet's tests for the transition and 3D support,
//! sliders that set `transition-duration`).
//!
//! The computed values of the properties live in one optional, shared
//! record, [`InertStyle`] (`ComputedStyle::inert`): an element that sets
//! none of them pays one `None`. A value in the record is the text of the
//! computed value (a time in seconds, a timing function in its short form),
//! so the cascade, `inherit` and `initial` need no code per property.
//!
//! Departures from Chromium: `calc()` and the math functions are not
//! accepted in times; `linear()` is not accepted as a timing function; the
//! names of properties in `transition-property` are lower-cased when swb
//! knows them as properties (Chromium knows more names); `will-change`
//! accepts every identifier that is not reserved, as the specification
//! does.
//!
//! <https://www.w3.org/TR/css-transitions-1/>,
//! <https://www.w3.org/TR/css-transitions-2/#transition-behavior-property>,
//! <https://www.w3.org/TR/css-will-change-1/>,
//! <https://www.w3.org/TR/pointerevents3/#the-touch-action-css-property>,
//! <https://www.w3.org/TR/css-transforms-2/#perspective-property>.

use std::sync::Arc;

use swb_css::{ComponentValue, ParseError, Parser};

use super::compute::ComputeContext;
use super::ids::{LonghandId, LonghandValue};
use super::is_known_property;
use super::longhand::parse_position;
use crate::ComputedStyle;
use crate::cssom::format_number;
use crate::parse::length::{LengthOptions, parse_length_percentage};
use crate::parse::{ParseResult, parse_integer, parse_number};
use crate::values::{PositionComponent, SpecifiedLengthPercentage as Lp};

/// The computed values of the properties of this module. `None` is the
/// initial value of each.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InertStyle {
    /// `transition-property` (initial `all`).
    pub(crate) transition_property: Option<Arc<str>>,
    /// `transition-duration` (initial `0s`).
    pub(crate) transition_duration: Option<Arc<str>>,
    /// `transition-delay` (initial `0s`).
    pub(crate) transition_delay: Option<Arc<str>>,
    /// `transition-timing-function` (initial `ease`).
    pub(crate) transition_timing_function: Option<Arc<str>>,
    /// `transition-behavior` (initial `normal`).
    pub(crate) transition_behavior: Option<Arc<str>>,
    /// `will-change` (initial `auto`).
    pub(crate) will_change: Option<Arc<str>>,
    /// `touch-action` (initial `auto`).
    pub(crate) touch_action: Option<Arc<str>>,
    /// `perspective` in px (initial `none`).
    pub(crate) perspective: Option<f32>,
    /// `perspective-origin` (initial `50% 50%`).
    pub(crate) perspective_origin: Option<[PositionComponent; 2]>,
}

/// The record with every value initial.
static INITIAL: InertStyle = InertStyle {
    transition_property: None,
    transition_duration: None,
    transition_delay: None,
    transition_timing_function: None,
    transition_behavior: None,
    will_change: None,
    touch_action: None,
    perspective: None,
    perspective_origin: None,
};

impl InertStyle {
    /// The text of the text-valued longhand `id` (its initial value if
    /// unset); `None` for the other longhands.
    pub(crate) fn text(style: &ComputedStyle, id: LonghandId) -> Option<&str> {
        use LonghandId as L;
        let inert = style.inert.as_deref().unwrap_or(&INITIAL);
        let (value, initial) = match id {
            L::TransitionProperty => (inert.transition_property.as_deref(), "all"),
            L::TransitionDuration => (inert.transition_duration.as_deref(), "0s"),
            L::TransitionDelay => (inert.transition_delay.as_deref(), "0s"),
            L::TransitionTimingFunction => (inert.transition_timing_function.as_deref(), "ease"),
            L::TransitionBehavior => (inert.transition_behavior.as_deref(), "normal"),
            L::WillChange => (inert.will_change.as_deref(), "auto"),
            L::TouchAction => (inert.touch_action.as_deref(), "auto"),
            _ => return None,
        };
        Some(value.unwrap_or(initial))
    }

    /// Copies the value of `id` from `source` (`None`: all initial).
    fn copy_field(&mut self, id: LonghandId, source: Option<&InertStyle>) {
        use LonghandId as L;
        let s = source.unwrap_or(&INITIAL);
        match id {
            L::TransitionProperty => self.transition_property.clone_from(&s.transition_property),
            L::TransitionDuration => self.transition_duration.clone_from(&s.transition_duration),
            L::TransitionDelay => self.transition_delay.clone_from(&s.transition_delay),
            L::TransitionTimingFunction => {
                self.transition_timing_function
                    .clone_from(&s.transition_timing_function);
            }
            L::TransitionBehavior => self.transition_behavior.clone_from(&s.transition_behavior),
            L::WillChange => self.will_change.clone_from(&s.will_change),
            L::TouchAction => self.touch_action.clone_from(&s.touch_action),
            L::Perspective => self.perspective = s.perspective,
            L::PerspectiveOrigin => self.perspective_origin.clone_from(&s.perspective_origin),
            _ => {}
        }
    }
}

/// Changes the record of `style`, made when the first value is set (and
/// copied first if another style shares it). A record that is initial
/// again is dropped.
fn update(style: &mut ComputedStyle, f: impl FnOnce(&mut InertStyle)) {
    let record = style
        .inert
        .get_or_insert_with(|| Arc::new(InertStyle::default()));
    f(Arc::make_mut(record));
    if **record == INITIAL {
        style.inert = None;
    }
}

/// Copies the value of the longhand `id` from `from` to `to` (inheritance
/// of the keywords `inherit` and `initial`).
pub(crate) fn copy_value(id: LonghandId, from: &ComputedStyle, to: &mut ComputedStyle) {
    if from.inert.is_none() && to.inert.is_none() {
        return;
    }
    update(to, |record| record.copy_field(id, from.inert.as_deref()));
}

/// Stores the computed value of `value` in `s` (`apply` of `compute.rs`).
pub(crate) fn apply(value: &LonghandValue, cx: &ComputeContext<'_>, s: &mut ComputedStyle) {
    use LonghandValue as V;
    update(s, |r| match value {
        V::TransitionProperty(v) => r.transition_property = Some(Arc::clone(v)),
        V::TransitionDuration(v) => r.transition_duration = Some(Arc::clone(v)),
        V::TransitionDelay(v) => r.transition_delay = Some(Arc::clone(v)),
        V::TransitionTimingFunction(v) => r.transition_timing_function = Some(Arc::clone(v)),
        V::TransitionBehavior(v) => r.transition_behavior = Some(Arc::clone(v)),
        V::WillChange(v) => r.will_change = Some(Arc::clone(v)),
        V::TouchAction(v) => r.touch_action = Some(Arc::clone(v)),
        V::Perspective(v) => r.perspective = v.as_ref().map(|lp| cx.px(lp).max(0.0)),
        V::PerspectiveOrigin([x, y]) => {
            r.perspective_origin = Some([x.compute(cx), y.compute(cx)]);
        }
        _ => {}
    });
}

/// Parses the value of the inert longhand `id` (the caller parses the
/// whole value).
pub(crate) fn parse_longhand(id: LonghandId, p: &mut Parser<'_>) -> ParseResult<LonghandValue> {
    use LonghandId as L;
    use LonghandValue as V;
    Ok(match id {
        L::TransitionProperty => V::TransitionProperty(parse_transition_property(p)?),
        L::TransitionDuration => V::TransitionDuration(parse_times(p, true)?),
        L::TransitionDelay => V::TransitionDelay(parse_times(p, false)?),
        L::TransitionTimingFunction => V::TransitionTimingFunction(parse_easings(p)?),
        L::TransitionBehavior => V::TransitionBehavior(parse_behaviors(p)?),
        L::WillChange => V::WillChange(parse_will_change(p)?),
        L::TouchAction => V::TouchAction(parse_touch_action(p)?),
        L::Perspective => V::Perspective(parse_perspective(p)?),
        L::PerspectiveOrigin => {
            let (x, y) = parse_position(p)?;
            V::PerspectiveOrigin([x, y])
        }
        _ => return Err(ParseError::Invalid),
    })
}

// ----- Words -----

/// True for the keywords that no list item of these properties may be as an
/// identifier of its own: the CSS-wide keywords and `default`
/// (css-values-4 §2.2 "custom identifiers").
fn is_reserved(ident: &str) -> bool {
    [
        "initial",
        "inherit",
        "unset",
        "revert",
        "revert-layer",
        "default",
    ]
    .iter()
    .any(|k| ident.eq_ignore_ascii_case(k))
}

/// Names of Chromium's animatable properties that swb does not parse as
/// properties, for the lower-casing of `transition-property`.
const OTHER_PROPERTIES: &[&str] = &[
    "box-shadow",
    "text-shadow",
    "filter",
    "backdrop-filter",
    "scale",
    "rotate",
    "translate",
    "offset",
    "caret-color",
    "accent-color",
    "text-decoration-thickness",
    "border-image",
    "border-spacing",
    "word-spacing",
    "letter-spacing",
];

/// A property name in the canonical case: lower case for a name that is a
/// property, else as written.
fn property_name(ident: &str) -> String {
    let lower = ident.to_ascii_lowercase();
    if lower == "all" || lower == "none" {
        return lower;
    }
    if is_known_property(&lower) || OTHER_PROPERTIES.contains(&lower.as_str()) {
        lower
    } else {
        ident.to_owned()
    }
}

/// Writes a list of items as `a, b`.
fn join(items: &[String]) -> Arc<str> {
    Arc::from(items.join(", "))
}

// ----- transition-property -----

/// `none | <single-transition-property>#`, with `all | <custom-ident>` for
/// the single property.
fn parse_transition_property(p: &mut Parser<'_>) -> ParseResult<Arc<str>> {
    if p.try_parse(|p| p.expect_ident_matching("none")).is_ok() {
        return Ok(Arc::from("none"));
    }
    let items = p.parse_comma_separated(single_property)?;
    Ok(join(&items))
}

/// One `<single-transition-property>`; `none` is not one.
fn single_property(p: &mut Parser<'_>) -> ParseResult<String> {
    p.try_parse(|p| {
        let ident = p.expect_ident()?;
        if is_reserved(ident) || ident.eq_ignore_ascii_case("none") {
            return Err(ParseError::Invalid);
        }
        Ok(property_name(ident))
    })
}

/// The canonical text of a `transition-property` value.
fn declared_transition_property(tokens: &[ComponentValue]) -> Option<String> {
    let mut p = Parser::new(tokens);
    let value = p.parse_entirely(parse_transition_property).ok()?;
    Some(value.to_string())
}

// ----- Times -----

/// A `<time>` in seconds; `calc()` is not accepted.
fn time_seconds(p: &mut Parser<'_>) -> ParseResult<f64> {
    p.try_parse(|p| {
        let (value, unit) = p.expect_dimension()?;
        if unit.eq_ignore_ascii_case("s") {
            Ok(f64::from(value))
        } else if unit.eq_ignore_ascii_case("ms") {
            Ok(f64::from(value) / 1000.0)
        } else {
            Err(ParseError::Unexpected)
        }
    })
}

/// A time as the computed value writes it: seconds.
fn seconds_text(seconds: f64) -> String {
    format!("{}s", format_number(seconds))
}

/// `<time>#`; the duration cannot be negative.
fn parse_times(p: &mut Parser<'_>, non_negative: bool) -> ParseResult<Arc<str>> {
    let items = p.parse_comma_separated(|p| {
        let seconds = time_seconds(p)?;
        if non_negative && seconds < 0.0 {
            return Err(ParseError::Invalid);
        }
        Ok(seconds_text(seconds))
    })?;
    Ok(join(&items))
}

// ----- Timing functions -----

/// A timing function.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Easing {
    /// `ease`, `linear`, `step-start` and the like.
    Keyword(&'static str),
    /// `cubic-bezier(x1, y1, x2, y2)`.
    Bezier([f32; 4]),
    /// `steps(count)` with the position when it is not `end`.
    Steps(i32, Option<&'static str>),
}

impl Easing {
    /// The text that `element.style` shows.
    fn declared(self) -> String {
        match self {
            Easing::Keyword(k) => k.to_owned(),
            Easing::Bezier([a, b, c, d]) => format!(
                "cubic-bezier({}, {}, {}, {})",
                format_number(f64::from(a)),
                format_number(f64::from(b)),
                format_number(f64::from(c)),
                format_number(f64::from(d))
            ),
            Easing::Steps(n, None) => format!("steps({n})"),
            Easing::Steps(n, Some(position)) => format!("steps({n}, {position})"),
        }
    }

    /// The computed value: the keywords `step-start` and `step-end` are
    /// `steps()`.
    fn computed(self) -> String {
        match self {
            Easing::Keyword("step-start") => "steps(1, start)".to_owned(),
            Easing::Keyword("step-end") => "steps(1)".to_owned(),
            other => other.declared(),
        }
    }
}

/// `<easing-function>` (without `linear()`).
/// <https://www.w3.org/TR/css-easing-1/#easing-functions>
fn parse_easing(p: &mut Parser<'_>) -> ParseResult<Easing> {
    p.try_parse(|p| {
        if let Ok(keyword) = p.expect_one_of(&[
            ("ease", "ease"),
            ("ease-in", "ease-in"),
            ("ease-out", "ease-out"),
            ("ease-in-out", "ease-in-out"),
            ("linear", "linear"),
            ("step-start", "step-start"),
            ("step-end", "step-end"),
        ]) {
            return Ok(Easing::Keyword(keyword));
        }
        let (name, mut args) = p.expect_function()?;
        if name.eq_ignore_ascii_case("cubic-bezier") {
            args.parse_entirely(|a| {
                let mut v = [0.0f32; 4];
                for (i, slot) in v.iter_mut().enumerate() {
                    if i > 0 {
                        a.expect_comma()?;
                    }
                    *slot = parse_number(a)?;
                }
                let in_range = |x: f32| (0.0..=1.0).contains(&x);
                if in_range(v[0]) && in_range(v[2]) {
                    Ok(Easing::Bezier(v))
                } else {
                    Err(ParseError::Invalid)
                }
            })
        } else if name.eq_ignore_ascii_case("steps") {
            args.parse_entirely(|a| {
                let count = parse_integer(a)?;
                let position = if a.expect_comma().is_ok() {
                    Some(a.expect_one_of(&[
                        ("jump-start", "jump-start"),
                        ("jump-end", ""),
                        ("jump-none", "jump-none"),
                        ("jump-both", "jump-both"),
                        ("start", "start"),
                        ("end", ""),
                    ])?)
                } else {
                    None
                };
                let position = position.filter(|s| !s.is_empty());
                let least = if position == Some("jump-none") { 2 } else { 1 };
                if count < least {
                    return Err(ParseError::Invalid);
                }
                Ok(Easing::Steps(count, position))
            })
        } else {
            Err(ParseError::Unexpected)
        }
    })
}

/// `<easing-function>#`, as computed.
fn parse_easings(p: &mut Parser<'_>) -> ParseResult<Arc<str>> {
    let items = p.parse_comma_separated(|p| Ok(parse_easing(p)?.computed()))?;
    Ok(join(&items))
}

/// The text of a `transition-timing-function` value in `element.style`.
fn declared_easings(tokens: &[ComponentValue]) -> Option<String> {
    let mut p = Parser::new(tokens);
    let items = p
        .parse_entirely(|p| {
            p.parse_comma_separated(|p| -> ParseResult<String> { Ok(parse_easing(p)?.declared()) })
        })
        .ok()?;
    Some(items.join(", "))
}

// ----- transition-behavior -----

/// One `<transition-behavior-value>`.
fn behavior(p: &mut Parser<'_>) -> ParseResult<&'static str> {
    p.expect_one_of(&[("normal", "normal"), ("allow-discrete", "allow-discrete")])
}

/// `<transition-behavior-value>#`.
fn parse_behaviors(p: &mut Parser<'_>) -> ParseResult<Arc<str>> {
    let items = p.parse_comma_separated(|p| behavior(p).map(str::to_owned))?;
    Ok(join(&items))
}

// ----- will-change -----

/// `auto | <animateable-feature>#`.
fn parse_will_change(p: &mut Parser<'_>) -> ParseResult<Arc<str>> {
    if p.try_parse(|p| p.expect_ident_matching("auto")).is_ok() {
        return Ok(Arc::from("auto"));
    }
    let items = p.parse_comma_separated(|p| {
        let ident = p.expect_ident()?;
        if is_reserved(ident)
            || ["none", "all", "auto", "will-change"]
                .iter()
                .any(|k| ident.eq_ignore_ascii_case(k))
        {
            return Err(ParseError::Invalid);
        }
        // `scroll-position` and `contents` are keywords.
        let lower = ident.to_ascii_lowercase();
        Ok(if lower == "scroll-position" || lower == "contents" {
            lower
        } else {
            ident.to_owned()
        })
    })?;
    Ok(join(&items))
}

/// The text of a `will-change` value in `element.style`.
fn declared_will_change(tokens: &[ComponentValue]) -> Option<String> {
    let mut p = Parser::new(tokens);
    let value = p.parse_entirely(parse_will_change).ok()?;
    Some(value.to_string())
}

// ----- touch-action -----

/// `auto | none | [ [ pan-x | pan-left | pan-right ] || [ pan-y | pan-up |
/// pan-down ] || pinch-zoom ] | manipulation`, written in the order
/// horizontal pan, vertical pan, `pinch-zoom`.
fn parse_touch_action(p: &mut Parser<'_>) -> ParseResult<Arc<str>> {
    if let Ok(word) = p.try_parse(|p| {
        p.expect_one_of(&[
            ("auto", "auto"),
            ("none", "none"),
            ("manipulation", "manipulation"),
        ])
    }) {
        return Ok(Arc::from(word));
    }
    let mut horizontal = None;
    let mut vertical = None;
    let mut pinch = false;
    let mut any = false;
    while let Ok(word) = p.try_parse(|p| {
        p.expect_one_of(&[
            ("pan-x", 0),
            ("pan-left", 1),
            ("pan-right", 2),
            ("pan-y", 3),
            ("pan-up", 4),
            ("pan-down", 5),
            ("pinch-zoom", 6),
        ])
    }) {
        any = true;
        let slot = match word {
            0..=2 => &mut horizontal,
            3..=5 => &mut vertical,
            _ => {
                if pinch {
                    return Err(ParseError::Invalid);
                }
                pinch = true;
                continue;
            }
        };
        if slot.is_some() {
            return Err(ParseError::Invalid);
        }
        *slot = Some(word);
    }
    if !any {
        return Err(ParseError::Unexpected);
    }
    let names = [
        "pan-x",
        "pan-left",
        "pan-right",
        "pan-y",
        "pan-up",
        "pan-down",
    ];
    let mut out: Vec<&str> = [horizontal, vertical]
        .into_iter()
        .flatten()
        .filter_map(|i| names.get(i))
        .copied()
        .collect();
    if pinch {
        out.push("pinch-zoom");
    }
    Ok(Arc::from(out.join(" ")))
}

/// The text of a `touch-action` value in `element.style`.
fn declared_touch_action(tokens: &[ComponentValue]) -> Option<String> {
    let mut p = Parser::new(tokens);
    let value = p.parse_entirely(parse_touch_action).ok()?;
    Some(value.to_string())
}

// ----- perspective -----

/// `none | <length [0,∞]>`.
fn parse_perspective(p: &mut Parser<'_>) -> ParseResult<Option<Lp>> {
    if p.try_parse(|p| p.expect_ident_matching("none")).is_ok() {
        return Ok(None);
    }
    parse_length_percentage(p, LengthOptions::NON_NEGATIVE_LENGTH).map(Some)
}

// ----- The shorthand -----

/// One layer of `transition`.
#[derive(Default)]
struct Layer {
    property: Option<String>,
    duration: Option<f64>,
    delay: Option<f64>,
    easing: Option<Easing>,
    behavior: Option<&'static str>,
}

/// The property of a layer: `none`, `all` or a name. A keyword of another
/// part is that part first; a second one is the property (`ease ease`,
/// as in Chromium).
fn layer_property(p: &mut Parser<'_>) -> ParseResult<String> {
    if p.expect_ident_matching("none").is_ok() {
        return Ok("none".to_owned());
    }
    single_property(p)
}

/// One layer: the parts in any order, each at most once (two times: the
/// duration, then the delay).
fn parse_layer(p: &mut Parser<'_>) -> ParseResult<Layer> {
    let mut layer = Layer::default();
    let mut times = 0;
    let mut any = false;
    while !p.is_exhausted() {
        if let Ok(seconds) = time_seconds(p) {
            // Chromium does not check the range of the duration here.
            match times {
                0 => layer.duration = Some(seconds),
                1 => layer.delay = Some(seconds),
                _ => return Err(ParseError::Invalid),
            }
            times += 1;
        } else if layer.easing.is_none()
            && let Ok(easing) = parse_easing(p)
        {
            layer.easing = Some(easing);
        } else if layer.behavior.is_none()
            && let Ok(word) = p.try_parse(behavior)
        {
            layer.behavior = Some(word);
        } else if layer.property.is_none()
            && let Ok(name) = p.try_parse(layer_property)
        {
            layer.property = Some(name);
        } else {
            return Err(ParseError::Invalid);
        }
        any = true;
    }
    if any {
        Ok(layer)
    } else {
        Err(ParseError::EndOfInput)
    }
}

/// `transition`: `<single-transition>#`; `none` is a property and so only
/// valid in a list of one.
/// <https://www.w3.org/TR/css-transitions-1/#transition-shorthand-property>
pub(crate) fn parse_transition(
    p: &mut Parser<'_>,
    out: &mut Vec<LonghandValue>,
) -> ParseResult<()> {
    let layers = p.parse_comma_separated(parse_layer)?;
    if layers.len() > 1 && layers.iter().any(|l| l.property.as_deref() == Some("none")) {
        return Err(ParseError::Invalid);
    }
    let list = |f: &dyn Fn(&Layer) -> String| -> Arc<str> {
        join(&layers.iter().map(f).collect::<Vec<_>>())
    };
    out.push(LonghandValue::TransitionProperty(list(&|l| {
        l.property.clone().unwrap_or_else(|| "all".to_owned())
    })));
    out.push(LonghandValue::TransitionDuration(list(&|l| {
        seconds_text(l.duration.unwrap_or(0.0))
    })));
    out.push(LonghandValue::TransitionTimingFunction(list(&|l| {
        l.easing.map_or_else(|| "ease".to_owned(), Easing::computed)
    })));
    out.push(LonghandValue::TransitionDelay(list(&|l| {
        seconds_text(l.delay.unwrap_or(0.0))
    })));
    out.push(LonghandValue::TransitionBehavior(list(&|l| {
        l.behavior.unwrap_or("normal").to_owned()
    })));
    Ok(())
}

// ----- element.style -----

/// The text of the value `tokens` of the longhand `name` as `element.style`
/// writes it, for the longhands whose text differs from the generic
/// writing; `None` if `tokens` do not parse.
pub(crate) fn declared_text(name: &str, tokens: &[ComponentValue]) -> Option<String> {
    match name {
        "transition-property" => declared_transition_property(tokens),
        "transition-timing-function" => declared_easings(tokens),
        "will-change" => declared_will_change(tokens),
        "touch-action" => declared_touch_action(tokens),
        _ => None,
    }
}

/// The layers of a `transition-*` longhand value `text` as `element.style`
/// writes it, split at the commas outside parentheses.
pub(crate) fn split_list(text: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut depth = 0u32;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                items.push(text.get(start..i).unwrap_or("").trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    items.push(text.get(start..).unwrap_or("").trim());
    items
}

/// The value of `transition` from the texts of its longhands in the order
/// behavior, duration, timing function, delay, property, as Chromium
/// serializes it: one layer per item of the longest list, a missing item
/// is the initial value; a layer leaves out what is initial (the property
/// `all` if something else follows, the duration if it is `0s` (so a delay
/// reads as a duration, as in Chromium),
/// the timing function `ease`, the delay `0s`, the behavior `normal`) and
/// is `all` if nothing is left.
pub(crate) fn transition_value(texts: &[&str]) -> String {
    let [behavior, duration, timing, delay, property] = texts else {
        return String::new();
    };
    let lists: Vec<Vec<&str>> = [property, duration, timing, delay, behavior]
        .iter()
        .map(|t| split_list(t))
        .collect();
    let count = lists.iter().map(Vec::len).max().unwrap_or(0);
    let item = |list: usize, layer: usize, initial: &'static str| -> &str {
        lists
            .get(list)
            .and_then(|l| l.get(layer))
            .copied()
            .unwrap_or(initial)
    };
    let layers: Vec<String> = (0..count)
        .map(|i| {
            let mut parts: Vec<&str> = Vec::new();
            let (property, duration) = (item(0, i, "all"), item(1, i, "0s"));
            let (timing, delay, behavior) =
                (item(2, i, "ease"), item(3, i, "0s"), item(4, i, "normal"));
            if property != "all" {
                parts.push(property);
            }
            if duration != "0s" {
                parts.push(duration);
            }
            if timing != "ease" {
                parts.push(timing);
            }
            if delay != "0s" {
                parts.push(delay);
            }
            if behavior != "normal" {
                parts.push(behavior);
            }
            if parts.is_empty() {
                "all".to_owned()
            } else {
                parts.join(" ")
            }
        })
        .collect();
    layers.join(", ")
}

/// The expansion of a `transition` value for a declaration block: the
/// texts of the five longhands in the order behavior, duration, timing
/// function, delay, property, with the written units and the values that
/// the layers leave out filled in. `None` if the value is not valid.
pub(crate) fn expand_transition(tokens: &[ComponentValue]) -> Option<Vec<String>> {
    // The value must be valid as a whole.
    Parser::new(tokens)
        .parse_entirely(|p| parse_transition(p, &mut Vec::new()))
        .ok()?;
    let mut rows: [Vec<String>; 5] = Default::default();
    for item in tokens.split(ComponentValue::is_comma) {
        let mut q = Parser::new(item);
        let layer = parse_layer(&mut q).ok()?;
        let times = written_times(item);
        let [behavior, duration, timing, delay, property] = &mut rows;
        behavior.push(layer.behavior.unwrap_or("normal").to_owned());
        duration.push(times.first().cloned().unwrap_or_else(|| "0s".to_owned()));
        timing.push(
            layer
                .easing
                .map_or_else(|| "ease".to_owned(), Easing::declared),
        );
        delay.push(times.get(1).cloned().unwrap_or_else(|| "0s".to_owned()));
        property.push(layer.property.unwrap_or_else(|| "all".to_owned()));
    }
    Some(rows.iter().map(|r| r.join(", ")).collect())
}

/// The times of a layer as `element.style` writes them: the number and the
/// lower-case unit.
fn written_times(layer: &[ComponentValue]) -> Vec<String> {
    layer
        .iter()
        .filter_map(|v| match v {
            ComponentValue::Dimension { number, unit }
                if unit.eq_ignore_ascii_case("s") || unit.eq_ignore_ascii_case("ms") =>
            {
                Some(format!(
                    "{}{}",
                    format_number(f64::from(number.value)),
                    unit.to_ascii_lowercase()
                ))
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_css::parse_component_values;

    fn tokens(text: &str) -> Vec<ComponentValue> {
        parse_component_values(text)
    }

    fn longhand(id: LonghandId, text: &str) -> Option<LonghandValue> {
        let t = tokens(text);
        let mut p = Parser::new(&t);
        p.parse_entirely(|p| parse_longhand(id, p)).ok()
    }

    fn text_of(id: LonghandId, text: &str) -> Option<String> {
        match longhand(id, text)? {
            LonghandValue::TransitionProperty(v)
            | LonghandValue::TransitionDuration(v)
            | LonghandValue::TransitionDelay(v)
            | LonghandValue::TransitionTimingFunction(v)
            | LonghandValue::TransitionBehavior(v)
            | LonghandValue::WillChange(v)
            | LonghandValue::TouchAction(v) => Some(v.to_string()),
            _ => None,
        }
    }

    fn shorthand(text: &str) -> Option<Vec<String>> {
        let t = tokens(text);
        let mut p = Parser::new(&t);
        let mut out = Vec::new();
        p.parse_entirely(|p| parse_transition(p, &mut out)).ok()?;
        Some(
            out.iter()
                .filter_map(|v| match v {
                    LonghandValue::TransitionProperty(v)
                    | LonghandValue::TransitionDuration(v)
                    | LonghandValue::TransitionDelay(v)
                    | LonghandValue::TransitionTimingFunction(v)
                    | LonghandValue::TransitionBehavior(v) => Some(v.to_string()),
                    _ => None,
                })
                .collect(),
        )
    }

    #[test]
    fn times_are_seconds() {
        let d = LonghandId::TransitionDuration;
        assert_eq!(text_of(d, "300ms").as_deref(), Some("0.3s"));
        assert_eq!(text_of(d, "1s, 2500ms").as_deref(), Some("1s, 2.5s"));
        assert_eq!(text_of(d, "-1s"), None);
        assert_eq!(text_of(d, "0"), None);
        assert_eq!(
            text_of(LonghandId::TransitionDelay, "-300ms").as_deref(),
            Some("-0.3s")
        );
    }

    #[test]
    fn timing_functions() {
        let f = LonghandId::TransitionTimingFunction;
        assert_eq!(text_of(f, "step-start").as_deref(), Some("steps(1, start)"));
        assert_eq!(text_of(f, "steps(4, end)").as_deref(), Some("steps(4)"));
        assert_eq!(
            text_of(f, "steps(4, jump-none)").as_deref(),
            Some("steps(4, jump-none)")
        );
        assert_eq!(text_of(f, "steps(1, jump-none)"), None);
        assert_eq!(text_of(f, "steps(0)"), None);
        assert_eq!(
            text_of(f, "cubic-bezier(.1,.2,.3,.4), ease").as_deref(),
            Some("cubic-bezier(0.1, 0.2, 0.3, 0.4), ease")
        );
        assert_eq!(text_of(f, "cubic-bezier(2, 0, 1, 1)"), None);
        assert_eq!(text_of(f, "linear(0, 1)"), None);
    }

    #[test]
    fn property_lists() {
        let id = LonghandId::TransitionProperty;
        assert_eq!(text_of(id, "OPACITY, Foo").as_deref(), Some("opacity, Foo"));
        assert_eq!(text_of(id, "none").as_deref(), Some("none"));
        assert_eq!(text_of(id, "none, width"), None);
        assert_eq!(text_of(id, "width, default"), None);
        assert_eq!(text_of(id, "1s"), None);
    }

    #[test]
    fn will_change_and_touch_action() {
        let w = LonghandId::WillChange;
        assert_eq!(
            text_of(w, "TRANSFORM, Contents").as_deref(),
            Some("TRANSFORM, contents")
        );
        assert_eq!(text_of(w, "auto, transform"), None);
        assert_eq!(text_of(w, "none"), None);
        let t = LonghandId::TouchAction;
        assert_eq!(
            text_of(t, "pinch-zoom pan-y pan-left").as_deref(),
            Some("pan-left pan-y pinch-zoom")
        );
        assert_eq!(text_of(t, "pan-x pan-left"), None);
        assert_eq!(text_of(t, "none pan-x"), None);
        assert_eq!(text_of(t, "manipulation").as_deref(), Some("manipulation"));
    }

    #[test]
    fn shorthand_layers() {
        assert_eq!(
            shorthand("opacity 1s ease-in 2s, width 3s").unwrap(),
            [
                "opacity, width",
                "1s, 3s",
                "ease-in, ease",
                "2s, 0s",
                "normal, normal"
            ]
        );
        assert_eq!(shorthand("ease").unwrap()[0], "all");
        assert_eq!(shorthand("none").unwrap()[0], "none");
        assert!(shorthand("none, width").is_none());
        assert!(shorthand("1s 2s 3s").is_none());
        assert!(shorthand("a b").is_none());
        assert!(shorthand("inherit 1s").is_none());
        assert!(shorthand("opacity,, width").is_none());
    }

    #[test]
    fn shorthand_serialization() {
        // Behavior, duration, timing function, delay, property.
        let cases = [
            (["normal", "1s", "ease", "0s", "opacity"], "opacity 1s"),
            (["normal", "0s", "ease", "0s", "all"], "all"),
            (["normal", "0.3s", "ease", "0s", "all"], "0.3s"),
            (["normal", "0s", "ease", "2s", "opacity"], "opacity 2s"),
            (
                ["allow-discrete", "1s", "linear", "2s", "a, b"],
                "a 1s linear 2s allow-discrete, b",
            ),
            (
                ["allow-discrete", "0s", "ease", "0s", "all"],
                "allow-discrete",
            ),
            (
                ["normal", "1s", "steps(4, start)", "0s", "all"],
                "1s steps(4, start)",
            ),
        ];
        for (texts, want) in cases {
            assert_eq!(transition_value(&texts), want, "{texts:?}");
        }
    }

    #[test]
    fn written_expansion() {
        assert_eq!(
            expand_transition(&tokens("opacity 300MS ease-in 2s, width 3s")).unwrap(),
            [
                "normal, normal",
                "300ms, 3s",
                "ease-in, ease",
                "2s, 0s",
                "opacity, width"
            ]
        );
        assert!(expand_transition(&tokens("opacity 1s 2s 3s")).is_none());
    }

    #[test]
    fn copy_and_drop() {
        let mut a = ComputedStyle::initial().as_ref().clone();
        let mut b = ComputedStyle::initial().as_ref().clone();
        update(&mut a, |r| r.will_change = Some(Arc::from("transform")));
        copy_value(LonghandId::WillChange, &a, &mut b);
        assert_eq!(
            InertStyle::text(&b, LonghandId::WillChange),
            Some("transform")
        );
        copy_value(
            LonghandId::WillChange,
            &ComputedStyle::initial().as_ref().clone(),
            &mut b,
        );
        assert!(b.inert.is_none());
    }
}
