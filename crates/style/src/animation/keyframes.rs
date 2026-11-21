//! Keyframe sets: the resolved keyframes of one `@keyframes` name, per
//! property (CSS Animations 1 §3,
//! <https://drafts.csswg.org/css-animations-1/#keyframes>), and the
//! interval of keyframes around an iteration progress (Web Animations 1
//! §5.3.4, "the effect value of a keyframe effect",
//! <https://drafts.csswg.org/web-animations-1/#the-effect-value-of-a-keyframe-animation-effect>).
//!
//! The rules, measured in Chromium 148 (`keyframes-resolution` in
//! `tools/probes/css-animations-3a.json`):
//!
//! - Of several `@keyframes` rules with one name, the one that wins by
//!   cascade layer and then by order counts as a whole (the stylist picks
//!   it, `Stylist::keyframes`): unlayered rules win over layered ones, a
//!   later layer over an earlier one, a later rule over an earlier one;
//!   rules in an `@media` that does not match do not count. Names are case
//!   sensitive; `@keyframes "a"` is the name `a`.
//! - Keyframes are sorted by offset; keyframes with the same offset and
//!   the same `animation-timing-function` merge, a later declaration of a
//!   property winning (with different timing functions they stay apart, in
//!   source order).
//! - `!important` declarations, invalid ones, the `animation-*` and
//!   `transition-*` properties and `all` are dropped; shorthands expand to
//!   their longhands; `animation-timing-function` is the easing of the
//!   keyframe's interval.
//! - A property without a keyframe at 0% or 100% uses the element's value
//!   without the animation there (the underlying value; 3b supplies it).
//!
//! The declarations stay as specified (with `var()`), so they are computed
//! with the element's context at each use (ADR 0044 section 3).

use std::collections::HashMap;
use std::sync::Arc;

use swb_css::{KeyframesRule, Parser};

use super::easing::{Easing, parse_easing};
use crate::parse::ParserContext;
use crate::properties::{LonghandId, PropertyDeclaration, parse_declaration};

/// The property of a list of keyframes.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum AnimatedProperty {
    /// A longhand.
    Longhand(LonghandId),
    /// A custom property.
    Custom(Arc<str>),
}

/// One keyframe of one property.
#[derive(Clone, Debug, PartialEq)]
pub struct Keyframe {
    /// The offset, 0 to 1.
    pub offset: f64,
    /// The `animation-timing-function` of the keyframe; `None`: the
    /// element's.
    pub easing: Option<Easing>,
    /// The declaration of the property (specified, maybe with `var()`).
    pub(crate) declaration: PropertyDeclaration,
}

/// The keyframes of one property, sorted by offset.
#[derive(Clone, Debug, PartialEq)]
pub struct PropertyKeyframes {
    /// The property.
    pub(crate) property: AnimatedProperty,
    /// The keyframes (never empty).
    pub keyframes: Vec<Keyframe>,
}

/// The resolved keyframes of a `@keyframes` rule.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeyframeSet {
    /// The properties in the order of [`AnimatedProperty`] (longhands in
    /// the order of `LonghandId`, then custom properties by name).
    pub properties: Vec<PropertyKeyframes>,
}

/// The two keyframes around an iteration progress and the eased progress
/// between them. A `None` keyframe is the implicit 0% or 100% keyframe:
/// the underlying value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interval<'a> {
    /// The start of the interval.
    pub from: Option<&'a Keyframe>,
    /// The end of the interval (`from` itself when the progress selects a
    /// single keyframe: then `progress` is 0).
    pub to: Option<&'a Keyframe>,
    /// The progress from `from` to `to`, after the keyframe's easing.
    pub progress: f64,
}

impl KeyframeSet {
    /// Resolves the keyframes of `rule`; `cx` parses its declarations.
    pub(crate) fn new(rule: &KeyframesRule, cx: &ParserContext) -> KeyframeSet {
        // Each property's keyframes, and the index of each (offset, easing
        // text) among them, so that merging is O(1) per declaration.
        let mut properties: Vec<(PropertyKeyframes, HashMap<MergeKey, usize>)> = Vec::new();
        let mut index: HashMap<AnimatedProperty, usize> = HashMap::new();
        for keyframe in &rule.keyframes {
            let (easing, declarations) = keyframe_declarations(&keyframe.declarations, cx);
            let easing_text: Option<Arc<str>> =
                easing.as_ref().map(|e| Arc::from(e.declared_text()));
            for &key in &keyframe.keys {
                let offset = key / 100.0;
                for declaration in &declarations {
                    let property = match declaration {
                        PropertyDeclaration::Custom(name, _) => {
                            AnimatedProperty::Custom(Arc::clone(name))
                        }
                        other => match other.longhand() {
                            Some(id) if !is_animation_longhand(id) => {
                                AnimatedProperty::Longhand(id)
                            }
                            _ => continue,
                        },
                    };
                    let slot = *index.entry(property.clone()).or_insert_with(|| {
                        let list = PropertyKeyframes {
                            property,
                            keyframes: Vec::new(),
                        };
                        properties.push((list, HashMap::new()));
                        properties.len() - 1
                    });
                    let Some((list, merge)) = properties.get_mut(slot) else {
                        continue;
                    };
                    let key = (offset.to_bits(), easing_text.clone());
                    if let Some(existing) = merge.get(&key).and_then(|&i| list.keyframes.get_mut(i))
                    {
                        existing.declaration = declaration.clone();
                        continue;
                    }
                    merge.insert(key, list.keyframes.len());
                    list.keyframes.push(Keyframe {
                        offset,
                        easing: easing.clone(),
                        declaration: declaration.clone(),
                    });
                }
            }
        }
        let mut properties: Vec<PropertyKeyframes> =
            properties.into_iter().map(|(list, _)| list).collect();
        for list in &mut properties {
            // Stable: keyframes of one offset keep their source order.
            list.keyframes.sort_by(|a, b| a.offset.total_cmp(&b.offset));
        }
        properties.sort_by(|a, b| a.property.cmp(&b.property));
        KeyframeSet { properties }
    }

    /// True if no property has a keyframe.
    pub fn is_empty(&self) -> bool {
        self.properties.is_empty()
    }

    /// The interval of the keyframes of property `index` at the iteration
    /// `progress`, with `default_easing` (the element's
    /// `animation-timing-function`) for the keyframes that have none, and
    /// the before flag of the effect's timing. `None` if `index` is out of
    /// range.
    pub fn interval(
        &self,
        index: usize,
        progress: f64,
        default_easing: &Easing,
        before: bool,
    ) -> Option<Interval<'_>> {
        let list = &self.properties.get(index)?.keyframes;
        let frames = Frames::new(list);
        let single = |i: usize| {
            let k = frames.get(i).1;
            Interval {
                from: k,
                to: k,
                progress: 0.0,
            }
        };
        let at_zero = frames.count_at_or_below(0.0);
        let below_one = frames.count_below(1.0);
        if progress < 0.0 && at_zero > 1 {
            return Some(single(0));
        }
        if progress >= 1.0 && frames.len() - below_one > 1 {
            return Some(single(frames.len() - 1));
        }
        // The last keyframe at or before the progress and before 1, else
        // the last keyframe at 0.
        let k = frames.partition(|o| o <= progress && o < 1.0);
        let a = if k > 0 {
            k - 1
        } else {
            at_zero.saturating_sub(1)
        };
        let b = (a + 1).min(frames.len() - 1);
        let (start, from) = frames.get(a);
        let (end, to) = frames.get(b);
        let distance = if end > start {
            (progress - start) / (end - start)
        } else {
            0.0
        };
        let easing = from
            .and_then(|k| k.easing.as_ref())
            .unwrap_or(default_easing);
        Some(Interval {
            from,
            to,
            progress: easing.evaluate(distance, before),
        })
    }
}

/// The key that merges keyframes of one property: the offset's bits and
/// the text of the keyframe's easing.
type MergeKey = (u64, Option<Arc<str>>);

/// The easing and the declarations of one keyframe: `!important`,
/// invalid and not animatable declarations dropped.
fn keyframe_declarations(
    list: &[swb_css::Declaration],
    cx: &ParserContext,
) -> (Option<Easing>, Vec<PropertyDeclaration>) {
    let mut easing = None;
    let mut declarations = Vec::new();
    for d in list {
        if d.important {
            continue;
        }
        let name = d.name.to_ascii_lowercase();
        if name == "animation-timing-function" || name == "-webkit-animation-timing-function" {
            let mut p = Parser::new(&d.value);
            if let Ok(e) = p.parse_entirely(parse_easing) {
                easing = Some(e);
            }
            continue;
        }
        if name == "all" || is_not_animatable(&name) {
            continue;
        }
        parse_declaration(&d.name, &d.value, cx, &mut declarations);
    }
    (easing, declarations)
}

/// True for the properties that keyframes ignore besides the animation
/// longhands: the transition properties (Chromium drops them).
fn is_not_animatable(name: &str) -> bool {
    let name = name.strip_prefix("-webkit-").unwrap_or(name);
    name.starts_with("transition") || name.starts_with("animation")
}

/// True for the animation longhands.
fn is_animation_longhand(id: LonghandId) -> bool {
    id.name().starts_with("animation-") || id.name().starts_with("transition-")
}

/// The keyframes of a property with the implicit 0% and 100% keyframes
/// (`None`) where the list has none, without copying the list.
struct Frames<'a> {
    list: &'a [Keyframe],
    /// True if an implicit keyframe at 0 comes first.
    implicit_start: bool,
    /// True if an implicit keyframe at 1 comes last.
    implicit_end: bool,
}

impl<'a> Frames<'a> {
    fn new(list: &'a [Keyframe]) -> Self {
        Frames {
            list,
            implicit_start: list.first().is_none_or(|k| k.offset != 0.0),
            implicit_end: list.last().is_none_or(|k| k.offset != 1.0),
        }
    }

    fn len(&self) -> usize {
        self.list.len() + usize::from(self.implicit_start) + usize::from(self.implicit_end)
    }

    /// The offset and keyframe at `i` (`None`: implicit).
    fn get(&self, i: usize) -> (f64, Option<&'a Keyframe>) {
        let start = usize::from(self.implicit_start);
        if self.implicit_start && i == 0 {
            return (0.0, None);
        }
        match self.list.get(i - start) {
            Some(k) => (k.offset, Some(k)),
            None => (1.0, None),
        }
    }

    /// The number of leading keyframes whose offset satisfies `f` (which
    /// must hold for a prefix of the sorted offsets).
    fn partition(&self, f: impl Fn(f64) -> bool) -> usize {
        let start = usize::from(self.implicit_start);
        if self.implicit_start && !f(0.0) {
            return 0;
        }
        let inner = self.list.partition_point(|k| f(k.offset));
        if inner == self.list.len() && self.implicit_end && f(1.0) {
            return start + inner + 1;
        }
        start + inner
    }

    fn count_at_or_below(&self, offset: f64) -> usize {
        self.partition(|o| o <= offset)
    }

    fn count_below(&self, offset: f64) -> usize {
        self.partition(|o| o < offset)
    }
}

#[cfg(test)]
mod tests;
