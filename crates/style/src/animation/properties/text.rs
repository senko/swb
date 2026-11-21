//! The texts of the animation properties, as Chromium 148 writes them
//! (`tools/probes/css-animations-3a.json`): the computed values of the
//! longhands and the resolved `animation` and `animation-range`
//! (`getComputedStyle`), and the values of `element.style` and `cssText`
//! (the expansion of the shorthands and their text from the longhands).

use swb_css::{ComponentValue, Parser};

use super::{
    AnimationName, AnimationStyle, INITIAL, PlayState, RANGE_NAMES, composition_text, cycle,
    direction_text, fill_text, parse_animation, parse_animation_range, parse_longhand,
    parse_range_pair, play_state_text, range_text,
};
use crate::ComputedStyle;
use crate::animation::easing::Easing;
use crate::animation::time::{Time, seconds_text};
use crate::animation::timing::{FillMode, PlaybackDirection};
use crate::cssom::format_number;
use crate::properties::ids::{LonghandId, LonghandValue};

// ----- Texts -----

/// Joins list items as `a, b`.
fn join<T>(items: &[T], f: impl Fn(&T) -> String) -> String {
    items.iter().map(f).collect::<Vec<_>>().join(", ")
}

/// The computed text of the animation longhand `name` (`getComputedStyle`);
/// `None` for other names.
pub(crate) fn computed_text(name: &str, style: &ComputedStyle) -> Option<String> {
    let a = AnimationStyle::of(style);
    Some(match name {
        "animation-name" => join(&a.names, AnimationName::text),
        "animation-duration" => join(&a.durations, |s| seconds_text(s.unwrap_or(0.0))),
        "animation-timing-function" => join(&a.timing_functions, Easing::computed_text),
        "animation-delay" => join(&a.delays, |s| seconds_text(*s)),
        "animation-iteration-count" => join(&a.iteration_counts, |c| count_text(*c)),
        "animation-direction" => join(&a.directions, |d| direction_text(*d).to_owned()),
        "animation-fill-mode" => join(&a.fill_modes, |f| fill_text(*f).to_owned()),
        "animation-play-state" => join(&a.play_states, |s| play_state_text(*s).to_owned()),
        "animation-composition" => join(&a.compositions, |c| composition_text(*c).to_owned()),
        "animation-timeline" => join(&a.timelines, ToString::to_string),
        "animation-range-start" => join(&a.range_starts, ToString::to_string),
        "animation-range-end" => join(&a.range_ends, ToString::to_string),
        _ => return None,
    })
}

fn count_text(c: f64) -> String {
    if c.is_infinite() {
        "infinite".to_owned()
    } else {
        format_number(c)
    }
}

/// The resolved value of `animation` (`getComputedStyle`), as Chromium
/// writes it: one layer per name, the other lists repeated; a layer leaves
/// out the parts that are initial, unless the name is a keyword of that
/// part (`ease ease`, `normal normal`); the duration is written when the
/// delay is; the name `none` only when nothing else is left. Empty when
/// the timeline or the range are not initial.
pub(crate) fn computed_shorthand(style: &ComputedStyle) -> String {
    let a = AnimationStyle::of(style);
    let initial = &*INITIAL;
    if a.timelines != initial.timelines
        || a.range_starts != initial.range_starts
        || a.range_ends != initial.range_ends
    {
        return String::new();
    }
    let layers: Vec<String> = (0..a.names.len()).map(|i| computed_layer(a, i)).collect();
    layers.join(", ")
}

/// One layer of the resolved `animation`.
fn computed_layer(a: &AnimationStyle, i: usize) -> String {
    let name = a.names.get(i).cloned().unwrap_or(AnimationName::None);
    // A name that is also a keyword of a part makes that part written,
    // also a string name (`"none"` gives `none "none"`: the fill mode).
    let ident = match &name {
        AnimationName::Ident(s) | AnimationName::String(s) => s.to_ascii_lowercase(),
        AnimationName::None => String::new(),
    };
    let named = |words: &[&str]| words.contains(&ident.as_str());
    let duration = cycle(&a.durations, i).copied().flatten();
    let easing = cycle(&a.timing_functions, i)
        .cloned()
        .unwrap_or(Easing::Ease);
    let delay = cycle(&a.delays, i).copied().unwrap_or(0.0);
    let count = cycle(&a.iteration_counts, i).copied().unwrap_or(1.0);
    let direction = cycle(&a.directions, i).copied().unwrap_or_default();
    let fill = cycle(&a.fill_modes, i).copied().unwrap_or_default();
    let play = cycle(&a.play_states, i).copied().unwrap_or_default();
    let mut parts: Vec<String> = Vec::new();
    if duration.is_some() || delay != 0.0 {
        parts.push(seconds_text(duration.unwrap_or(0.0)));
    }
    if easing != Easing::Ease
        || named(&[
            "ease",
            "ease-in",
            "ease-out",
            "ease-in-out",
            "linear",
            "step-start",
            "step-end",
        ])
    {
        parts.push(easing.computed_text());
    }
    if delay != 0.0 {
        parts.push(seconds_text(delay));
    }
    if count != 1.0 || named(&["infinite"]) {
        parts.push(count_text(count));
    }
    if direction != PlaybackDirection::Normal
        || named(&["normal", "reverse", "alternate", "alternate-reverse"])
    {
        parts.push(direction_text(direction).to_owned());
    }
    if fill != FillMode::None || named(&["forwards", "backwards", "both", "none"]) {
        parts.push(fill_text(fill).to_owned());
    }
    if play != PlayState::Running || named(&["running", "paused"]) {
        parts.push(play_state_text(play).to_owned());
    }
    if name != AnimationName::None || parts.is_empty() {
        parts.push(name.text());
    }
    parts.join(" ")
}

/// The resolved value of `animation-range`: each start and end, the end
/// left out when it is what the start alone would give.
pub(crate) fn computed_range(style: &ComputedStyle) -> String {
    let a = AnimationStyle::of(style);
    range_pairs(&a.range_starts, &a.range_ends)
}

/// `animation-range` from the texts of the start and end lists.
fn range_pairs<S: AsRef<str>>(starts: &[S], ends: &[S]) -> String {
    let count = starts.len().max(ends.len());
    let items: Vec<String> = (0..count)
        .map(|i| {
            let start = starts.get(i).map_or("normal", AsRef::as_ref);
            let end = ends.get(i).map_or("normal", AsRef::as_ref);
            let name = start.split(' ').next().unwrap_or("");
            let implied = if RANGE_NAMES.contains(&name) {
                name
            } else {
                "normal"
            };
            if end == implied {
                start.to_owned()
            } else {
                format!("{start} {end}")
            }
        })
        .collect();
    items.join(", ")
}

// ----- element.style -----

/// The animation longhands in the order of Chromium's `animation`
/// shorthand (also the order of `element.style` after a shorthand).
pub(crate) const SHORTHAND_LONGHANDS: [&str; 11] = [
    "animation-duration",
    "animation-timing-function",
    "animation-delay",
    "animation-iteration-count",
    "animation-direction",
    "animation-fill-mode",
    "animation-play-state",
    "animation-name",
    "animation-timeline",
    "animation-range-start",
    "animation-range-end",
];

/// The text of the value `tokens` of the animation longhand `name` as
/// `element.style` writes it; `None` if `tokens` do not parse or `name`
/// is not an animation longhand.
pub(crate) fn declared_text(name: &str, tokens: &[ComponentValue]) -> Option<String> {
    let id = LonghandId::from_name(name)?;
    let mut p = Parser::new(tokens);
    let value = p.parse_entirely(|p| parse_longhand(id, p)).ok()?;
    Some(value_text(&value, tokens))
}

/// The `element.style` text of a parsed animation longhand value.
fn value_text(value: &LonghandValue, tokens: &[ComponentValue]) -> String {
    use LonghandValue as V;
    match value {
        V::AnimationName(v) => join(v, AnimationName::text),
        V::AnimationDuration(v) => join(v, |t| t.map_or_else(|| "auto".to_owned(), Time::declared)),
        V::AnimationTimingFunction(v) => join(v, Easing::declared_text),
        V::AnimationDelay(v) => join(v, |t| t.declared()),
        V::AnimationIterationCount(v) => join(v, |c| c.declared()),
        V::AnimationDirection(v) => join(v, |d| direction_text(*d).to_owned()),
        V::AnimationFillMode(v) => join(v, |f| fill_text(*f).to_owned()),
        V::AnimationPlayState(v) => join(v, |s| play_state_text(*s).to_owned()),
        V::AnimationComposition(v) => join(v, |c| composition_text(*c).to_owned()),
        V::AnimationTimeline(v) => join(v, ToString::to_string),
        V::AnimationRangeStart(v) | V::AnimationRangeEnd(v) => {
            let end = matches!(value, V::AnimationRangeEnd(_));
            let items = crate::cssom::split_commas(tokens);
            v.iter()
                .enumerate()
                .map(|(i, item)| {
                    let written = items.get(i).copied().unwrap_or(&[]);
                    range_text(item, end, &|_| declared_offset(written))
                })
                .collect::<Vec<_>>()
                .join(", ")
        }
        _ => String::new(),
    }
}

/// The written offset of a range item (its last component value).
fn declared_offset(item: &[ComponentValue]) -> String {
    let item = swb_css::trim_whitespace(item);
    let last = item.len().saturating_sub(1);
    crate::cssom::serialize_value(
        item.get(last..).unwrap_or(&[]),
        crate::cssom::ValueStyle::LENGTH,
    )
}

/// The expansion of an `animation` value for a declaration block: the
/// texts of the eleven longhands of [`SHORTHAND_LONGHANDS`]. The duration
/// that a layer leaves out is `auto` (Chromium 148). `None` if the value
/// is not valid.
pub(crate) fn expand_animation(tokens: &[ComponentValue]) -> Option<Vec<String>> {
    let mut values = Vec::new();
    Parser::new(tokens)
        .parse_entirely(|p| parse_animation(p, &mut values))
        .ok()?;
    Some(values.iter().map(|v| value_text(v, &[])).collect())
}

/// The value of `animation` in `element.style` from the texts of its
/// longhands (in the order of [`SHORTHAND_LONGHANDS`]), as Chromium writes
/// it: every part of every layer; when the lists differ in length, layer
/// `i` has the items that exist at `i`. Empty when the timeline is not
/// `auto` or the range not `normal`.
pub(crate) fn animation_value(texts: &[&str]) -> String {
    let [rest @ .., timeline, start, end] = texts else {
        return String::new();
    };
    if *timeline != "auto" || *start != "normal" || *end != "normal" {
        return String::new();
    }
    // A longhand with `var()` has no value yet (Chromium: empty).
    if rest.iter().any(|t| t.contains("var(")) {
        return String::new();
    }
    let lists: Vec<Vec<&str>> = rest
        .iter()
        .map(|t| crate::properties::inert::split_list(t))
        .collect();
    let count = lists.iter().map(Vec::len).max().unwrap_or(0);
    let layers: Vec<String> = (0..count)
        .map(|i| {
            lists
                .iter()
                .filter_map(|l| l.get(i).copied())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    layers.join(", ")
}

/// True if the lists of the eight layered longhands (texts in the order
/// of [`SHORTHAND_LONGHANDS`]) have the same length. Only then does the
/// `animation` text of [`animation_value`] parse back to the same
/// longhands; `cssText` writes the longhands otherwise (Chromium writes
/// the shorthand, and its declaration block keeps the longhands).
pub(crate) fn lists_match(texts: &[&str]) -> bool {
    let mut lengths = texts
        .iter()
        .take(8)
        .map(|t| crate::properties::inert::split_list(t).len());
    let first = lengths.next();
    lengths.all(|n| Some(n) == first)
}

/// The value of `animation-range` in `element.style` from the texts of
/// its two longhands.
pub(crate) fn range_value(start: &str, end: &str) -> String {
    let starts = crate::properties::inert::split_list(start);
    let ends = crate::properties::inert::split_list(end);
    if starts.len() != ends.len() {
        return String::new();
    }
    range_pairs(&starts, &ends)
}

/// The expansion of an `animation-range` value: the texts of the start
/// and end lists. `None` if the value is not valid.
pub(crate) fn expand_range(tokens: &[ComponentValue]) -> Option<Vec<String>> {
    let mut values = Vec::new();
    Parser::new(tokens)
        .parse_entirely(|p| parse_animation_range(p, &mut values))
        .ok()?;
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    for pair in crate::cssom::split_commas(tokens) {
        let mut p = Parser::new(pair);
        let (start, end) = p.parse_entirely(parse_range_pair).ok()?;
        let parts = written_range_parts(pair);
        starts.push(range_text(&start, false, &|_| {
            parts.first().cloned().unwrap_or_default()
        }));
        ends.push(range_text(&end, true, &|_| {
            parts.last().cloned().unwrap_or_default()
        }));
    }
    Some(vec![starts.join(", "), ends.join(", ")])
}

/// The written offsets of a range pair, in order.
fn written_range_parts(pair: &[ComponentValue]) -> Vec<String> {
    pair.iter()
        .filter(|v| {
            !matches!(v, ComponentValue::Whitespace)
                && !matches!(v, ComponentValue::Ident(i) if RANGE_NAMES.iter().any(|n| i.eq_ignore_ascii_case(n)) || i.eq_ignore_ascii_case("normal"))
        })
        .map(|v| {
            crate::cssom::serialize_value(std::slice::from_ref(v), crate::cssom::ValueStyle::LENGTH)
        })
        .collect()
}
