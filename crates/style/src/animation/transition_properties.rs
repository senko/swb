//! The `transition-*` longhands and the `transition` shorthand (CSS
//! Transitions 1 §2, CSS Transitions 2 §3 for `transition-behavior`; ADR
//! 0044 section 5, M13 feature 5): their grammar, computed values and
//! texts, as Chromium 148 gives them (`tools/probes/css-transitions.json`,
//! `css-animations-3a.json`).
//!
//! The times take the math functions and the timing functions `linear()`
//! (the parsers of `time.rs` and `easing.rs`, shared with the animation
//! properties). The computed values live in one optional, shared record,
//! [`TransitionStyle`] (`ComputedStyle::transitions`): an element that sets
//! none of the properties pays one `None`. The lists keep their lengths
//! (`getComputedStyle` shows `transition-duration: 0.2s, 0.4s` for three
//! properties); [`TransitionStyle::matching`] repeats them to the length of
//! `transition-property`, as CSS Transitions 1 §2 says.
//!
//! The names in `transition-property` are lower-cased when swb knows them
//! as properties (Chromium knows more names); other names stay as written
//! and match nothing.
//!
//! <https://drafts.csswg.org/css-transitions-1/#transition-property-property>,
//! <https://drafts.csswg.org/css-transitions-2/#transition-behavior-property>.

use std::sync::{Arc, LazyLock};

use swb_css::{ComponentValue, ParseError, Parser};

use super::easing::{Easing, parse_easing};
use super::time::{Time, parse_time, seconds_text};
use crate::ComputedStyle;
use crate::parse::ParseResult;
use crate::properties::ids::{LonghandId, LonghandValue};
use crate::properties::is_known_property;

/// One item of `transition-property`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TransitionName {
    /// `all`.
    All,
    /// `none` (only as the whole value).
    None,
    /// A property name: lower case if swb knows the property, a custom
    /// property or an unknown name as written.
    Name(Arc<str>),
}

impl TransitionName {
    /// The text of the item.
    fn text(&self) -> &str {
        match self {
            TransitionName::All => "all",
            TransitionName::None => "none",
            TransitionName::Name(name) => name,
        }
    }
}

/// The computed values of the `transition-*` longhands of an element.
#[derive(Clone, Debug, PartialEq)]
pub struct TransitionStyle {
    /// `transition-property`.
    pub properties: Arc<[TransitionName]>,
    /// `transition-duration` in seconds (not negative).
    pub durations: Arc<[f64]>,
    /// `transition-delay` in seconds.
    pub delays: Arc<[f64]>,
    /// `transition-timing-function` (computed forms).
    pub timing_functions: Arc<[Easing]>,
    /// `transition-behavior`: true for `allow-discrete`.
    pub behaviors: Arc<[bool]>,
}

/// The record with every value initial.
static INITIAL: LazyLock<TransitionStyle> = LazyLock::new(|| TransitionStyle {
    properties: Arc::from([TransitionName::All]),
    durations: Arc::from([0.0]),
    delays: Arc::from([0.0]),
    timing_functions: Arc::from([Easing::Ease]),
    behaviors: Arc::from([false]),
});

/// The transition of one item of `transition-property`, with the other
/// lists repeated to its length.
#[derive(Clone, Debug, PartialEq)]
pub struct TransitionEntry<'a> {
    /// The duration in ms (finite, not negative).
    pub duration: f64,
    /// The delay in ms (finite).
    pub delay: f64,
    /// The timing function: the easing of the transition's effect.
    pub easing: &'a Easing,
    /// `transition-behavior: allow-discrete`.
    pub allow_discrete: bool,
}

impl TransitionEntry<'_> {
    /// The combined duration: max(duration, 0) + delay (CSS Transitions 1
    /// §3). A transition starts only when it is greater than 0.
    pub fn combined_duration(&self) -> f64 {
        self.duration.max(0.0) + self.delay
    }
}

/// Item `i` of a list, repeating the list (a list is never empty).
fn cycle<T>(list: &[T], i: usize) -> Option<&T> {
    list.get(i % list.len().max(1))
}

impl TransitionStyle {
    /// The transition record of `style` (the initial record when none of
    /// the properties is set).
    pub fn of(style: &ComputedStyle) -> &TransitionStyle {
        style.transitions.as_deref().unwrap_or(&INITIAL)
    }

    /// True if `style` can start a transition: an item of
    /// `transition-property` other than `none` with a combined duration
    /// greater than 0.
    pub fn may_transition(style: &ComputedStyle) -> bool {
        style
            .transitions
            .as_deref()
            .is_some_and(|t| t.any_item(|name| !matches!(name, TransitionName::None)))
    }

    /// True if `style` can start a transition of a value that follows the
    /// parent: an item that is `all`, a custom property or (a shorthand
    /// of) an inherited longhand, or any named item when the style takes a
    /// non-inherited value with `inherit` (`ComputedStyle::inherits_reset`),
    /// with a combined duration greater than 0.
    /// Other lists can start no transition when only the inherited values
    /// of an element change (ADR 0044 section 20: such elements keep the
    /// style sharing, and their parents keep no style for them; a value
    /// computed from an inherited one, as `2em`, is not followed).
    pub(crate) fn may_transition_inherited(style: &ComputedStyle) -> bool {
        style.transitions.as_deref().is_some_and(|t| {
            t.any_item(|name| match name {
                TransitionName::All => true,
                TransitionName::None => false,
                // A non-inherited property set to `inherit` follows the
                // parent too (`background-color: inherit`).
                TransitionName::Name(name) => {
                    style.inherits_reset || super::visibility::is_inherited_property(name)
                }
            })
        })
    }

    /// True if an item of `transition-property` for which `f` is true has
    /// a combined duration greater than 0.
    fn any_item(&self, f: impl Fn(&TransitionName) -> bool) -> bool {
        self.properties
            .iter()
            .enumerate()
            .any(|(i, name)| f(name) && self.entry(i).combined_duration() > 0.0)
    }

    /// True if `a` and `b` have the same value for each property that an
    /// item of the `transition-property` of `b` names (`all`: every
    /// value): a style change event from `a` to `b` starts no transition of
    /// an element without running ones.
    pub(crate) fn same_transitioned_values(a: &ComputedStyle, b: &ComputedStyle) -> bool {
        let Some(t) = b.transitions.as_deref() else {
            return a == b;
        };
        t.properties.iter().all(|name| match name {
            TransitionName::All => a == b,
            TransitionName::None => true,
            TransitionName::Name(name) if name.starts_with("--") => {
                a.custom_properties == b.custom_properties
                    || a.custom_properties.as_ref().and_then(|m| m.get(name))
                        == b.custom_properties.as_ref().and_then(|m| m.get(name))
            }
            TransitionName::Name(name) => crate::properties::longhands_of(name)
                .iter()
                .all(|id| id.same_value(a, b)),
        })
    }

    /// The transition of item `i` of `transition-property`.
    pub fn entry(&self, i: usize) -> TransitionEntry<'_> {
        let ms = |seconds: f64| super::easing::finite(seconds * 1000.0);
        TransitionEntry {
            duration: ms(cycle(&self.durations, i).copied().unwrap_or(0.0)).max(0.0),
            delay: ms(cycle(&self.delays, i).copied().unwrap_or(0.0)),
            easing: cycle(&self.timing_functions, i).unwrap_or(&Easing::Ease),
            allow_discrete: cycle(&self.behaviors, i).copied().unwrap_or(false),
        }
    }

    /// The transition of the longhand `id`, or of the custom property
    /// `custom`: the last item of `transition-property` that names it, a
    /// shorthand of it, or (for a longhand) `all` (CSS Transitions 1 §2.1);
    /// `None` if no item matches.
    pub(crate) fn matching(
        &self,
        id: Option<LonghandId>,
        custom: Option<&str>,
    ) -> Option<TransitionEntry<'_>> {
        let index = self.properties.iter().rposition(|name| match name {
            TransitionName::All => id.is_some_and(is_transitionable_longhand),
            TransitionName::None => false,
            TransitionName::Name(n) => match (id, custom) {
                (_, Some(c)) => **n == *c,
                (Some(id), None) => crate::properties::longhands_of(n).contains(&id),
                (None, None) => false,
            },
        })?;
        Some(self.entry(index))
    }

    /// Copies the value of `id` from `source`.
    fn copy_field(&mut self, id: LonghandId, source: &TransitionStyle) {
        use LonghandId as L;
        match id {
            L::TransitionProperty => self.properties = Arc::clone(&source.properties),
            L::TransitionDuration => self.durations = Arc::clone(&source.durations),
            L::TransitionDelay => self.delays = Arc::clone(&source.delays),
            L::TransitionTimingFunction => {
                self.timing_functions = Arc::clone(&source.timing_functions);
            }
            L::TransitionBehavior => self.behaviors = Arc::clone(&source.behaviors),
            _ => {}
        }
    }
}

/// True if a transition can run on the longhand `id`: every longhand but
/// the animation and transition properties and the properties whose
/// animation type is "not animatable" (`direction`, `unicode-bidi`,
/// `will-change`).
pub(crate) fn is_transitionable_longhand(id: LonghandId) -> bool {
    use LonghandId as L;
    let name = id.name();
    !(name.starts_with("animation-")
        || name.starts_with("transition-")
        || matches!(id, L::Direction | L::UnicodeBidi | L::WillChange))
}

/// Changes the record of `style`, made when the first value is set (and
/// copied first if another style shares it). A record that is initial
/// again is dropped.
fn update(style: &mut ComputedStyle, f: impl FnOnce(&mut TransitionStyle)) {
    let record = style
        .transitions
        .get_or_insert_with(|| Arc::new(INITIAL.clone()));
    f(Arc::make_mut(record));
    if **record == *INITIAL {
        style.transitions = None;
    }
}

/// Copies the value of the longhand `id` from `from` to `to` (inheritance
/// and the keywords `inherit` and `initial`).
pub(crate) fn copy_value(id: LonghandId, from: &ComputedStyle, to: &mut ComputedStyle) {
    if from.transitions.is_none() && to.transitions.is_none() {
        return;
    }
    let source = TransitionStyle::of(from);
    update(to, |record| record.copy_field(id, source));
}

/// Stores the computed value of `value` in `s` (`apply` of `compute.rs`):
/// times in seconds (a math function of a duration below 0 is 0), the
/// timing functions in their computed forms.
pub(crate) fn apply(value: &LonghandValue, s: &mut ComputedStyle) {
    use LonghandValue as V;
    update(s, |r| match value {
        V::TransitionProperty(v) => r.properties = Arc::clone(v),
        V::TransitionDuration(v) => {
            r.durations = computed_list(&DURATIONS, v, |t| t.seconds.max(0.0));
        }
        V::TransitionDelay(v) => r.delays = computed_list(&DELAYS, v, |t| t.seconds),
        V::TransitionTimingFunction(v) => {
            r.timing_functions = computed_list(&EASINGS, v, Easing::computed);
        }
        V::TransitionBehavior(v) => r.behaviors = Arc::clone(v),
        _ => {}
    });
}

/// The last specified list of a longhand and its computed list: the
/// elements that one declaration styles share one computed list (a long
/// `transition` list on many elements costs its memory once).
type ListSlot<S, C> = std::cell::RefCell<Option<(Arc<[S]>, Arc<[C]>)>>;

/// The thread-local [`ListSlot`] of one longhand.
type ListCache<S, C> = std::thread::LocalKey<ListSlot<S, C>>;

thread_local! {
    static DURATIONS: ListSlot<Time, f64> = const { std::cell::RefCell::new(None) };
    static DELAYS: ListSlot<Time, f64> = const { std::cell::RefCell::new(None) };
    static EASINGS: ListSlot<Easing, Easing> = const { std::cell::RefCell::new(None) };
}

/// The computed list of the specified list `specified` (`f` per item),
/// from `cache` when it holds the same specified list.
fn computed_list<S, C: Clone>(
    cache: &'static ListCache<S, C>,
    specified: &Arc<[S]>,
    f: impl Fn(&S) -> C,
) -> Arc<[C]> {
    cache.with(|cell| {
        let Ok(mut slot) = cell.try_borrow_mut() else {
            return specified.iter().map(&f).collect();
        };
        if let Some((s, c)) = slot.as_ref()
            && Arc::ptr_eq(s, specified)
        {
            return Arc::clone(c);
        }
        let computed: Arc<[C]> = specified.iter().map(&f).collect();
        *slot = Some((Arc::clone(specified), Arc::clone(&computed)));
        computed
    })
}

// ----- Parsing -----

/// True for the keywords that cannot be a `<custom-ident>` of
/// `transition-property`: the CSS-wide keywords and `default`.
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
    "offset",
    "caret-color",
    "accent-color",
    "text-decoration-thickness",
    "border-image",
];

/// A property name in the canonical case: lower case for a name that is a
/// property, else as written (custom properties keep their case).
fn property_name(ident: &str) -> TransitionName {
    let lower = ident.to_ascii_lowercase();
    if lower == "all" {
        return TransitionName::All;
    }
    if !ident.starts_with("--")
        && (is_known_property(&lower) || OTHER_PROPERTIES.contains(&lower.as_str()))
    {
        TransitionName::Name(Arc::from(lower))
    } else {
        TransitionName::Name(Arc::from(ident))
    }
}

/// One `<single-transition-property>` (`all | <custom-ident>`); `none` is
/// not one.
fn single_property(p: &mut Parser<'_>) -> ParseResult<TransitionName> {
    p.try_parse(|p| {
        let ident = p.expect_ident()?;
        if is_reserved(ident) || ident.eq_ignore_ascii_case("none") {
            return Err(ParseError::Invalid);
        }
        Ok(property_name(ident))
    })
}

/// `none | <single-transition-property>#`.
fn parse_property_list(p: &mut Parser<'_>) -> ParseResult<Arc<[TransitionName]>> {
    if p.try_parse(|p| p.expect_ident_matching("none")).is_ok() {
        return Ok(Arc::from([TransitionName::None]));
    }
    Ok(Arc::from(p.parse_comma_separated(single_property)?))
}

/// A duration: a `<time>` not below 0 (a math function may be; it computes
/// to 0).
fn parse_duration(p: &mut Parser<'_>) -> ParseResult<Time> {
    let t = parse_time(p)?;
    if t.seconds < 0.0 && !t.is_math() {
        return Err(ParseError::Invalid);
    }
    Ok(t)
}

/// One `<transition-behavior-value>`: true for `allow-discrete`.
fn behavior(p: &mut Parser<'_>) -> ParseResult<bool> {
    p.expect_one_of(&[("normal", false), ("allow-discrete", true)])
}

/// A comma-separated list of at least one item.
fn list<T>(
    p: &mut Parser<'_>,
    item: impl FnMut(&mut Parser<'_>) -> ParseResult<T>,
) -> ParseResult<Arc<[T]>> {
    Ok(Arc::from(p.parse_comma_separated(item)?))
}

/// Parses the value of the transition longhand `id` (the caller parses the
/// whole value).
pub(crate) fn parse_longhand(id: LonghandId, p: &mut Parser<'_>) -> ParseResult<LonghandValue> {
    use LonghandId as L;
    use LonghandValue as V;
    Ok(match id {
        L::TransitionProperty => V::TransitionProperty(parse_property_list(p)?),
        L::TransitionDuration => V::TransitionDuration(list(p, parse_duration)?),
        L::TransitionDelay => V::TransitionDelay(list(p, parse_time)?),
        L::TransitionTimingFunction => V::TransitionTimingFunction(list(p, parse_easing)?),
        L::TransitionBehavior => V::TransitionBehavior(list(p, behavior)?),
        _ => return Err(ParseError::Invalid),
    })
}

/// One layer of `transition`.
#[derive(Default)]
struct Layer {
    property: Option<TransitionName>,
    duration: Option<Time>,
    delay: Option<Time>,
    easing: Option<Easing>,
    behavior: Option<bool>,
}

/// The property of a layer: `none`, `all` or a name.
fn layer_property(p: &mut Parser<'_>) -> ParseResult<TransitionName> {
    if p.try_parse(|p| p.expect_ident_matching("none")).is_ok() {
        return Ok(TransitionName::None);
    }
    single_property(p)
}

/// One layer: the parts in any order, each at most once (two times: the
/// duration, then the delay). A keyword of another part is that part
/// first; a second one is the property (`ease ease`, as in Chromium).
fn parse_layer(p: &mut Parser<'_>) -> ParseResult<Layer> {
    let mut layer = Layer::default();
    let mut any = false;
    while !p.is_exhausted() {
        if let Ok(time) = p.try_parse(parse_time) {
            // The duration is a `<time [0s,∞]>`: a negative literal can only
            // be the delay (`transition: width -1s` has the duration 0s and
            // the delay -1s, as in Chromium 148). A math function is checked
            // at computed-value time.
            let negative = time.seconds < 0.0
                && matches!(
                    time.written,
                    super::time::Written::Plain | super::time::Written::Milliseconds
                );
            if layer.duration.is_none() && !negative {
                layer.duration = Some(time);
            } else if layer.delay.is_none() {
                layer.delay = Some(time);
            } else {
                return Err(ParseError::Invalid);
            }
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

/// The layers of a `transition` value; `none` is a property and so only
/// valid in a list of one.
fn parse_layers(p: &mut Parser<'_>) -> ParseResult<Vec<Layer>> {
    let layers = p.parse_comma_separated(parse_layer)?;
    if layers.len() > 1
        && layers
            .iter()
            .any(|l| l.property == Some(TransitionName::None))
    {
        return Err(ParseError::Invalid);
    }
    Ok(layers)
}

/// `transition`: `<single-transition>#`.
/// <https://drafts.csswg.org/css-transitions-1/#transition-shorthand-property>
pub(crate) fn parse_transition(
    p: &mut Parser<'_>,
    out: &mut Vec<LonghandValue>,
) -> ParseResult<()> {
    let layers = parse_layers(p)?;
    out.push(LonghandValue::TransitionProperty(
        layers
            .iter()
            .map(|l| l.property.clone().unwrap_or(TransitionName::All))
            .collect(),
    ));
    out.push(LonghandValue::TransitionDuration(
        layers
            .iter()
            .map(|l| l.duration.unwrap_or(Time::seconds(0.0)))
            .collect(),
    ));
    out.push(LonghandValue::TransitionTimingFunction(
        layers
            .iter()
            .map(|l| l.easing.clone().unwrap_or(Easing::Ease))
            .collect(),
    ));
    out.push(LonghandValue::TransitionDelay(
        layers
            .iter()
            .map(|l| l.delay.unwrap_or(Time::seconds(0.0)))
            .collect(),
    ));
    out.push(LonghandValue::TransitionBehavior(
        layers.iter().map(|l| l.behavior.unwrap_or(false)).collect(),
    ));
    Ok(())
}

// ----- Texts -----

/// Joins list items as `a, b`.
fn join<T>(items: &[T], f: impl Fn(&T) -> String) -> String {
    items.iter().map(f).collect::<Vec<_>>().join(", ")
}

fn behavior_text(allow_discrete: bool) -> String {
    if allow_discrete {
        "allow-discrete"
    } else {
        "normal"
    }
    .to_owned()
}

/// The computed text of the transition longhand `name`
/// (`getComputedStyle`); `None` for other names.
pub(crate) fn computed_text(name: &str, style: &ComputedStyle) -> Option<String> {
    let t = TransitionStyle::of(style);
    Some(match name {
        "transition-property" => join(&t.properties, |n| n.text().to_owned()),
        "transition-duration" => join(&t.durations, |s| seconds_text(*s)),
        "transition-delay" => join(&t.delays, |s| seconds_text(*s)),
        "transition-timing-function" => join(&t.timing_functions, Easing::computed_text),
        "transition-behavior" => join(&t.behaviors, |b| behavior_text(*b)),
        _ => return None,
    })
}

/// The text of the value `tokens` of the transition longhand `name` as
/// `element.style` writes it; `None` if `tokens` do not parse or `name` is
/// not a transition longhand.
pub(crate) fn declared_text(name: &str, tokens: &[ComponentValue]) -> Option<String> {
    let id = LonghandId::from_name(name)?;
    let mut p = Parser::new(tokens);
    let value = p.parse_entirely(|p| parse_longhand(id, p)).ok()?;
    value_text(&value)
}

/// The `element.style` text of a parsed transition longhand value.
fn value_text(value: &LonghandValue) -> Option<String> {
    use LonghandValue as V;
    Some(match value {
        V::TransitionProperty(v) => join(v, |n| n.text().to_owned()),
        V::TransitionDuration(v) | V::TransitionDelay(v) => join(v, |t| t.declared()),
        V::TransitionTimingFunction(v) => join(v, Easing::declared_text),
        V::TransitionBehavior(v) => join(v, |b| behavior_text(*b)),
        _ => return None,
    })
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
/// reads as a duration, as in Chromium), the timing function `ease`, the
/// delay `0s`, the behavior `normal`) and is `all` if nothing is left.
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
/// function, delay, property, with the written times and the values that
/// the layers leave out filled in. `None` if the value is not valid.
pub(crate) fn expand_transition(tokens: &[ComponentValue]) -> Option<Vec<String>> {
    let layers = Parser::new(tokens).parse_entirely(parse_layers).ok()?;
    let mut rows: [Vec<String>; 5] = Default::default();
    for layer in layers {
        let [behavior, duration, timing, delay, property] = &mut rows;
        behavior.push(behavior_text(layer.behavior.unwrap_or(false)));
        duration.push(
            layer
                .duration
                .map_or_else(|| "0s".to_owned(), Time::declared),
        );
        timing.push(
            layer
                .easing
                .map_or_else(|| "ease".to_owned(), |e| e.declared_text()),
        );
        delay.push(layer.delay.map_or_else(|| "0s".to_owned(), Time::declared));
        property.push(
            layer
                .property
                .map_or_else(|| "all".to_owned(), |n| n.text().to_owned()),
        );
    }
    Some(rows.iter().map(|r| r.join(", ")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_css::parse_component_values;

    fn tokens(text: &str) -> Vec<ComponentValue> {
        parse_component_values(text)
    }

    fn text_of(id: LonghandId, text: &str) -> Option<String> {
        declared_text(id.name(), &tokens(text))
    }

    fn computed_of(id: LonghandId, text: &str) -> Option<String> {
        let t = tokens(text);
        let value = Parser::new(&t)
            .parse_entirely(|p| parse_longhand(id, p))
            .ok()?;
        let mut style = ComputedStyle::initial().as_ref().clone();
        apply(&value, &mut style);
        computed_text(id.name(), &style)
    }

    fn shorthand(text: &str) -> Option<Vec<String>> {
        let t = tokens(text);
        let mut out = Vec::new();
        Parser::new(&t)
            .parse_entirely(|p| parse_transition(p, &mut out))
            .ok()?;
        let mut style = ComputedStyle::initial().as_ref().clone();
        for value in &out {
            apply(value, &mut style);
        }
        Some(
            [
                "transition-property",
                "transition-duration",
                "transition-timing-function",
                "transition-delay",
                "transition-behavior",
            ]
            .iter()
            .filter_map(|n| computed_text(n, &style))
            .collect(),
        )
    }

    #[test]
    fn times_take_units_and_math_functions() {
        let d = LonghandId::TransitionDuration;
        assert_eq!(computed_of(d, "300ms").as_deref(), Some("0.3s"));
        assert_eq!(computed_of(d, "1s, 2500ms").as_deref(), Some("1s, 2.5s"));
        assert_eq!(text_of(d, "300ms").as_deref(), Some("300ms"));
        assert_eq!(computed_of(d, "-1s"), None);
        assert_eq!(computed_of(d, "0"), None);
        // `calc()` (Chromium: `calc(var(--d) * 2)` with `--d: 200ms` is
        // 0.4s; `attribute-popup`); a negative math function computes to 0.
        assert_eq!(computed_of(d, "calc(200ms * 2)").as_deref(), Some("0.4s"));
        assert_eq!(text_of(d, "calc(200ms * 2)").as_deref(), Some("calc(0.4s)"));
        assert_eq!(computed_of(d, "calc(-1s)").as_deref(), Some("0s"));
        assert_eq!(
            computed_of(LonghandId::TransitionDelay, "-300ms").as_deref(),
            Some("-0.3s")
        );
    }

    #[test]
    fn timing_functions_with_linear() {
        let f = LonghandId::TransitionTimingFunction;
        assert_eq!(
            computed_of(f, "step-start").as_deref(),
            Some("steps(1, start)")
        );
        assert_eq!(computed_of(f, "steps(4, end)").as_deref(), Some("steps(4)"));
        assert_eq!(text_of(f, "steps(4, end)").as_deref(), Some("steps(4)"));
        assert_eq!(computed_of(f, "steps(1, jump-none)"), None);
        assert_eq!(
            computed_of(f, "cubic-bezier(.1,.2,.3,.4), ease").as_deref(),
            Some("cubic-bezier(0.1, 0.2, 0.3, 0.4), ease")
        );
        assert_eq!(computed_of(f, "cubic-bezier(2, 0, 1, 1)"), None);
        assert_eq!(
            computed_of(f, "linear(0, -24 20%, 16 40%, -8 60%, 4 80%, 1)").as_deref(),
            Some("linear(0 0%, -24 20%, 16 40%, -8 60%, 4 80%, 1 100%)")
        );
    }

    #[test]
    fn property_lists() {
        let id = LonghandId::TransitionProperty;
        assert_eq!(text_of(id, "OPACITY, Foo").as_deref(), Some("opacity, Foo"));
        assert_eq!(text_of(id, "--Fill").as_deref(), Some("--Fill"));
        assert_eq!(text_of(id, "none").as_deref(), Some("none"));
        assert_eq!(text_of(id, "none, width"), None);
        assert_eq!(text_of(id, "width, default"), None);
        assert_eq!(text_of(id, "1s"), None);
        let b = LonghandId::TransitionBehavior;
        assert_eq!(
            computed_of(b, "normal, allow-discrete").as_deref(),
            Some("normal, allow-discrete")
        );
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
        assert_eq!(
            shorthand("display 1s allow-discrete").unwrap()[4],
            "allow-discrete"
        );
        assert_eq!(
            shorthand("transform .3s linear(0, 1)").unwrap()[2],
            "linear(0 0%, 1 100%)"
        );
        assert!(shorthand("none, width").is_none());
        assert!(shorthand("1s 2s 3s").is_none());
        // A negative literal is the delay (measured in Chromium 148).
        let negative = shorthand("width -1s").unwrap();
        assert_eq!((&*negative[1], &*negative[3]), ("0s", "-1s"));
        let both = shorthand("width -1s 2s").unwrap();
        assert_eq!((&*both[1], &*both[3]), ("2s", "-1s"));
        assert_eq!(shorthand("width 2s -1s").unwrap()[3], "-1s");
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
    fn matching_takes_the_last_item_and_repeats_the_lists() {
        let mut style = ComputedStyle::initial().as_ref().clone();
        let t = tokens("opacity 1s, all 2s linear 50ms, margin 3s, --fill 4s");
        let mut out = Vec::new();
        Parser::new(&t)
            .parse_entirely(|p| parse_transition(p, &mut out))
            .unwrap();
        for value in &out {
            apply(value, &mut style);
        }
        let t = TransitionStyle::of(&style);
        let opacity = t.matching(Some(LonghandId::Opacity), None).unwrap();
        assert_eq!((opacity.duration, opacity.delay), (2000.0, 50.0));
        let margin = t.matching(Some(LonghandId::MarginLeft), None).unwrap();
        assert_eq!(margin.duration, 3000.0);
        assert_eq!(t.matching(None, Some("--fill")).unwrap().duration, 4000.0);
        assert!(t.matching(None, Some("--other")).is_none());
        assert!(
            t.matching(Some(LonghandId::TransitionDuration), None)
                .is_none()
        );
        assert!(TransitionStyle::may_transition(&style));
        let none = ComputedStyle::initial();
        assert!(!TransitionStyle::may_transition(&none));
    }
}
