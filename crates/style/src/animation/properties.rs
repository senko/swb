//! The `animation-*` longhands and the `animation` and `animation-range`
//! shorthands (CSS Animations 1 §4, CSS Animations 2 §3, Scroll-driven
//! Animations 1 for `animation-timeline` and `animation-range-*`): their
//! grammar, computed values and texts, measured in Chromium 148
//! (`tools/probes/css-animations-3a.json`).
//!
//! The computed values live in one optional, shared record,
//! [`AnimationStyle`] (`ComputedStyle::animations`): an element that sets
//! none of the properties pays one `None`. Each property is a list; the
//! lists keep their lengths (Chromium's `getComputedStyle` shows
//! `animation-duration: 1s, 2s` for one name), and [`AnimationStyle::entry`]
//! repeats them to the number of names, as CSS Animations 1 §4.2 says.
//!
//! `animation-composition`, `animation-timeline` and `animation-range-*`
//! parse, cascade and compute as in Chromium, with no effect (swb animates
//! with the composite operation `replace` and the document timeline only).
//! Chromium's `animation` shorthand resets `animation-timeline` and the
//! range, not `animation-composition`.
//!
//! <https://drafts.csswg.org/css-animations-1/#animation-name>,
//! <https://drafts.csswg.org/css-animations-2/#animation-composition>,
//! <https://drafts.csswg.org/scroll-animations-1/#animation-timeline>.

use std::sync::{Arc, LazyLock};

use swb_css::{ComponentValue, ParseError, Parser, serialize_identifier, serialize_string};

use super::easing::{Easing, parse_easing};
use super::list_support::{self, cycle, list, parse_nonneg_time};
use super::time::{Number, Time, parse_number, parse_time};
use super::timing::{FillMode, PlaybackDirection, Timing};
use crate::ComputedStyle;
use crate::parse::ParseResult;
use crate::parse::length::{LengthOptions, parse_length_percentage};
use crate::properties::compute::ComputeContext;
use crate::properties::ids::{LonghandId, LonghandValue};
use crate::values::SpecifiedLengthPercentage as Lp;

// ----- Value types -----

/// One item of `animation-name`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AnimationName {
    /// `none`: no animation for this entry (the entry still takes its
    /// place in the other lists).
    None,
    /// A name: an identifier, or a string that is a valid identifier
    /// (Chromium writes `"foo"` as `foo`).
    Ident(Arc<str>),
    /// A string that cannot be an identifier (`"none"`, `"initial"`).
    String(Arc<str>),
}

impl AnimationName {
    /// The name that a `@keyframes` rule must have; `None` for `none`.
    pub fn keyframes_name(&self) -> Option<&str> {
        match self {
            AnimationName::None => None,
            AnimationName::Ident(s) | AnimationName::String(s) => Some(s),
        }
    }

    /// The text of the name (specified and computed alike).
    fn text(&self) -> String {
        let mut out = String::new();
        match self {
            AnimationName::None => out.push_str("none"),
            AnimationName::Ident(s) => serialize_identifier(s, &mut out),
            AnimationName::String(s) => serialize_string(s, &mut out),
        }
        out
    }
}

/// One item of `animation-iteration-count`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum IterationCount {
    /// `infinite`.
    Infinite,
    /// A number (the computed value is at least 0).
    Number(Number),
}

impl IterationCount {
    fn declared(self) -> String {
        match self {
            IterationCount::Infinite => "infinite".to_owned(),
            IterationCount::Number(n) => n.declared(),
        }
    }

    /// The computed count: infinite, or the number clamped to 0.
    fn computed(self) -> f64 {
        match self {
            IterationCount::Infinite => f64::INFINITY,
            IterationCount::Number(n) => n.value.max(0.0),
        }
    }
}

/// `animation-play-state`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlayState {
    /// `running`.
    #[default]
    Running,
    /// `paused`.
    Paused,
}

/// `animation-composition`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Composition {
    /// `replace`.
    #[default]
    Replace,
    /// `add`.
    Add,
    /// `accumulate`.
    Accumulate,
}

/// One item of `animation-range-start` or `animation-range-end`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RangeItem {
    /// `normal`.
    Normal,
    /// A named timeline range with an optional offset.
    Named(&'static str, Option<Lp>),
    /// An offset.
    Offset(Lp),
}

/// The timeline range names.
const RANGE_NAMES: [&str; 6] = [
    "cover",
    "contain",
    "entry",
    "exit",
    "entry-crossing",
    "exit-crossing",
];

// ----- Keyword tables -----

fn direction_of(ident: &str) -> Option<PlaybackDirection> {
    Some(match ident.to_ascii_lowercase().as_str() {
        "normal" => PlaybackDirection::Normal,
        "reverse" => PlaybackDirection::Reverse,
        "alternate" => PlaybackDirection::Alternate,
        "alternate-reverse" => PlaybackDirection::AlternateReverse,
        _ => return None,
    })
}

fn fill_of(ident: &str) -> Option<FillMode> {
    Some(match ident.to_ascii_lowercase().as_str() {
        "none" => FillMode::None,
        "forwards" => FillMode::Forwards,
        "backwards" => FillMode::Backwards,
        "both" => FillMode::Both,
        _ => return None,
    })
}

fn play_state_of(ident: &str) -> Option<PlayState> {
    Some(match ident.to_ascii_lowercase().as_str() {
        "running" => PlayState::Running,
        "paused" => PlayState::Paused,
        _ => return None,
    })
}

fn play_state_text(p: PlayState) -> &'static str {
    match p {
        PlayState::Running => "running",
        PlayState::Paused => "paused",
    }
}

fn composition_of(ident: &str) -> Option<Composition> {
    Some(match ident.to_ascii_lowercase().as_str() {
        "replace" => Composition::Replace,
        "add" => Composition::Add,
        "accumulate" => Composition::Accumulate,
        _ => return None,
    })
}

fn composition_text(c: Composition) -> &'static str {
    match c {
        Composition::Replace => "replace",
        Composition::Add => "add",
        Composition::Accumulate => "accumulate",
    }
}

/// True for the words that cannot be a `<custom-ident>` of
/// `animation-name`: the CSS-wide keywords and `default` (`none` is the
/// keyword of the property).
fn is_reserved(ident: &str) -> bool {
    list_support::is_reserved(ident, &["none"])
}

// ----- The computed record -----

/// The computed values of the `animation-*` longhands of an element.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationStyle {
    /// `animation-name`.
    pub names: Arc<[AnimationName]>,
    /// `animation-duration` in seconds; `None` for `auto` (0 s for a
    /// time-based animation; the resolved `animation` leaves it out where
    /// it writes an explicit `0s`).
    pub durations: Arc<[Option<f64>]>,
    /// `animation-timing-function` (computed forms).
    pub timing_functions: Arc<[Easing]>,
    /// `animation-delay` in seconds.
    pub delays: Arc<[f64]>,
    /// `animation-iteration-count` (`infinite` is `f64::INFINITY`).
    pub iteration_counts: Arc<[f64]>,
    /// `animation-direction`.
    pub directions: Arc<[PlaybackDirection]>,
    /// `animation-fill-mode`.
    pub fill_modes: Arc<[FillMode]>,
    /// `animation-play-state`.
    pub play_states: Arc<[PlayState]>,
    /// `animation-composition` (no effect).
    pub compositions: Arc<[Composition]>,
    /// `animation-timeline`, as computed texts (no effect).
    pub(crate) timelines: Arc<[Arc<str>]>,
    /// `animation-range-start`, as computed texts (no effect).
    pub(crate) range_starts: Arc<[Arc<str>]>,
    /// `animation-range-end`, as computed texts (no effect).
    pub(crate) range_ends: Arc<[Arc<str>]>,
}

/// The record with every value initial.
static INITIAL: LazyLock<AnimationStyle> = LazyLock::new(|| AnimationStyle {
    names: Arc::from([AnimationName::None]),
    durations: Arc::from([None]),
    timing_functions: Arc::from([Easing::Ease]),
    delays: Arc::from([0.0]),
    iteration_counts: Arc::from([1.0]),
    directions: Arc::from([PlaybackDirection::Normal]),
    fill_modes: Arc::from([FillMode::None]),
    play_states: Arc::from([PlayState::Running]),
    compositions: Arc::from([Composition::Replace]),
    timelines: Arc::from([Arc::from("auto")]),
    range_starts: Arc::from([Arc::from("normal")]),
    range_ends: Arc::from([Arc::from("normal")]),
});

/// One animation of an element: entry `i` of the lists, with the lists
/// repeated to the number of names.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationEntry<'a> {
    /// The name (`None` for `none`: no animation).
    pub name: Option<&'a str>,
    /// The timing of the effect (delay and duration in ms; the effect
    /// easing is `linear`).
    pub timing: Timing,
    /// The `animation-timing-function`: the default easing of the
    /// keyframe intervals.
    pub keyframe_easing: &'a Easing,
    /// `animation-play-state`.
    pub play_state: PlayState,
}

impl AnimationStyle {
    /// The animation record of `style` (the initial record when none of
    /// the properties is set).
    pub fn of(style: &ComputedStyle) -> &AnimationStyle {
        style.animations.as_deref().unwrap_or(&INITIAL)
    }

    /// True if `style` has an animation name other than `none`.
    pub fn has_names(style: &ComputedStyle) -> bool {
        style
            .animations
            .as_deref()
            .is_some_and(|a| a.names.iter().any(|n| !matches!(n, AnimationName::None)))
    }

    /// The number of animations: the length of `animation-name`.
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// True if there is no name at all (never: the initial list is
    /// `none`).
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Animation `i` (`None` beyond the names). The times are finite in ms
    /// (a computed `1.79769e+308s` is the largest `f64` in ms), so the
    /// timing model never adds opposite infinities.
    pub fn entry(&self, i: usize) -> Option<AnimationEntry<'_>> {
        let name = self.names.get(i)?;
        let ms = |seconds: f64| super::easing::finite(seconds * 1000.0);
        Some(AnimationEntry {
            name: name.keyframes_name(),
            timing: Timing {
                delay: ms(cycle(&self.delays, i).copied().unwrap_or(0.0)),
                duration: ms(cycle(&self.durations, i).copied().flatten().unwrap_or(0.0)),

                iterations: cycle(&self.iteration_counts, i).copied().unwrap_or(1.0),
                direction: cycle(&self.directions, i).copied().unwrap_or_default(),
                fill: cycle(&self.fill_modes, i).copied().unwrap_or_default(),
                ..Timing::default()
            },
            keyframe_easing: cycle(&self.timing_functions, i).unwrap_or(&Easing::Ease),
            play_state: cycle(&self.play_states, i).copied().unwrap_or_default(),
        })
    }

    /// Copies the value of `id` from `source`.
    fn copy_field(&mut self, id: LonghandId, source: &AnimationStyle) {
        use LonghandId as L;
        match id {
            L::AnimationName => self.names = Arc::clone(&source.names),
            L::AnimationDuration => self.durations = Arc::clone(&source.durations),
            L::AnimationTimingFunction => {
                self.timing_functions = Arc::clone(&source.timing_functions);
            }
            L::AnimationDelay => self.delays = Arc::clone(&source.delays),
            L::AnimationIterationCount => {
                self.iteration_counts = Arc::clone(&source.iteration_counts);
            }
            L::AnimationDirection => self.directions = Arc::clone(&source.directions),
            L::AnimationFillMode => self.fill_modes = Arc::clone(&source.fill_modes),
            L::AnimationPlayState => self.play_states = Arc::clone(&source.play_states),
            L::AnimationComposition => self.compositions = Arc::clone(&source.compositions),
            L::AnimationTimeline => self.timelines = Arc::clone(&source.timelines),
            L::AnimationRangeStart => self.range_starts = Arc::clone(&source.range_starts),
            L::AnimationRangeEnd => self.range_ends = Arc::clone(&source.range_ends),
            _ => {}
        }
    }
}

/// Changes the record of `style`, made when the first value is set (and
/// copied first if another style shares it). A record that is initial
/// again is dropped.
fn update(style: &mut ComputedStyle, f: impl FnOnce(&mut AnimationStyle)) {
    let record = style
        .animations
        .get_or_insert_with(|| Arc::new(INITIAL.clone()));
    f(Arc::make_mut(record));
    if **record == *INITIAL {
        style.animations = None;
    }
}

/// Copies the value of the longhand `id` from `from` to `to` (inheritance
/// and the keywords `inherit` and `initial`).
pub(crate) fn copy_value(id: LonghandId, from: &ComputedStyle, to: &mut ComputedStyle) {
    if id.is_transition() {
        super::transition_properties::copy_value(id, from, to);
        return;
    }
    if from.animations.is_none() && to.animations.is_none() {
        return;
    }
    let source = AnimationStyle::of(from);
    update(to, |record| record.copy_field(id, source));
}

/// Stores the computed value of `value` in `s` (`apply` of `compute.rs`).
pub(crate) fn apply(value: &LonghandValue, cx: &ComputeContext<'_>, s: &mut ComputedStyle) {
    use LonghandValue as V;
    if value.id().is_transition() {
        super::transition_properties::apply(value, s);
        return;
    }
    update(s, |r| match value {
        V::AnimationName(v) => r.names = Arc::clone(v),
        V::AnimationDuration(v) => {
            r.durations = v.iter().map(|t| t.map(|t| t.seconds.max(0.0))).collect();
        }
        V::AnimationTimingFunction(v) => {
            r.timing_functions = v.iter().map(Easing::computed).collect();
        }
        V::AnimationDelay(v) => r.delays = v.iter().map(|t| t.seconds).collect(),
        V::AnimationIterationCount(v) => {
            r.iteration_counts = v.iter().map(|c| c.computed()).collect();
        }
        V::AnimationDirection(v) => r.directions = Arc::clone(v),
        V::AnimationFillMode(v) => r.fill_modes = Arc::clone(v),
        V::AnimationPlayState(v) => r.play_states = Arc::clone(v),
        V::AnimationComposition(v) => r.compositions = Arc::clone(v),
        V::AnimationTimeline(v) => r.timelines = Arc::clone(v),
        V::AnimationRangeStart(v) => r.range_starts = range_texts(v, cx, false),
        V::AnimationRangeEnd(v) => r.range_ends = range_texts(v, cx, true),
        _ => {}
    });
}

/// The computed texts of a range list.
fn range_texts(items: &[RangeItem], cx: &ComputeContext<'_>, end: bool) -> Arc<[Arc<str>]> {
    items
        .iter()
        .map(|item| Arc::from(range_text(item, end, &|lp| lp_text(&cx.lp(lp)))))
        .collect()
}

/// The text of a computed length-percentage.
fn lp_text(v: &crate::values::LengthPercentage) -> String {
    crate::cssom::resolved::length_percentage_text(v)
}

/// The text of a range item; the offset is left out when it is the
/// default of the side (0% for the start, 100% for the end).
fn range_text(item: &RangeItem, end: bool, lp: &dyn Fn(&Lp) -> String) -> String {
    match item {
        RangeItem::Normal => "normal".to_owned(),
        RangeItem::Offset(v) => lp(v),
        RangeItem::Named(name, None) => (*name).to_owned(),
        RangeItem::Named(name, Some(v)) => {
            let default = if end { 1.0 } else { 0.0 };
            if *v == Lp::Percentage(default) {
                (*name).to_owned()
            } else {
                format!("{name} {}", lp(v))
            }
        }
    }
}

// ----- Parsing the longhands -----

/// Parses the value of the animation longhand `id` (the caller parses the
/// whole value).
pub(crate) fn parse_longhand(id: LonghandId, p: &mut Parser<'_>) -> ParseResult<LonghandValue> {
    use LonghandId as L;
    use LonghandValue as V;
    Ok(match id {
        L::AnimationName => V::AnimationName(list(p, parse_name)?),
        L::AnimationDuration => V::AnimationDuration(list(p, parse_duration)?),
        L::AnimationTimingFunction => V::AnimationTimingFunction(list(p, parse_easing)?),
        L::AnimationDelay => V::AnimationDelay(list(p, parse_time)?),
        L::AnimationIterationCount => V::AnimationIterationCount(list(p, parse_count)?),
        L::AnimationDirection => V::AnimationDirection(list(p, |p| keyword(p, direction_of))?),
        L::AnimationFillMode => V::AnimationFillMode(list(p, |p| keyword(p, fill_of))?),
        L::AnimationPlayState => V::AnimationPlayState(list(p, |p| keyword(p, play_state_of))?),
        L::AnimationComposition => {
            V::AnimationComposition(list(p, |p| keyword(p, composition_of))?)
        }
        L::AnimationTimeline => V::AnimationTimeline(list(p, parse_timeline)?),
        L::AnimationRangeStart => V::AnimationRangeStart(list(p, parse_range_item)?),
        L::AnimationRangeEnd => V::AnimationRangeEnd(list(p, parse_range_item)?),
        L::TransitionProperty
        | L::TransitionDuration
        | L::TransitionDelay
        | L::TransitionTimingFunction
        | L::TransitionBehavior => super::transition_properties::parse_longhand(id, p)?,
        _ => return Err(ParseError::Invalid),
    })
}

fn keyword<T>(p: &mut Parser<'_>, of: impl FnOnce(&str) -> Option<T>) -> ParseResult<T> {
    p.try_parse(|p| of(p.expect_ident()?).ok_or(ParseError::Unexpected))
}

/// `none | <custom-ident> | <string>`.
fn parse_name(p: &mut Parser<'_>) -> ParseResult<AnimationName> {
    p.try_parse(|p| match p.next() {
        Some(ComponentValue::Ident(ident)) => {
            if ident.eq_ignore_ascii_case("none") {
                Ok(AnimationName::None)
            } else if is_reserved(ident) {
                Err(ParseError::Invalid)
            } else {
                Ok(AnimationName::Ident(Arc::from(&**ident)))
            }
        }
        Some(ComponentValue::String(s)) => {
            if s.is_empty() {
                Err(ParseError::Invalid)
            } else if is_reserved(s) {
                Ok(AnimationName::String(Arc::from(&**s)))
            } else {
                Ok(AnimationName::Ident(Arc::from(&**s)))
            }
        }
        _ => Err(ParseError::Unexpected),
    })
}

/// `auto | <time [0,∞]>`: `None` for `auto`; a math function may be
/// negative (it computes to 0).
fn parse_duration(p: &mut Parser<'_>) -> ParseResult<Option<Time>> {
    if p.try_parse(|p| p.expect_ident_matching("auto")).is_ok() {
        return Ok(None);
    }
    Ok(Some(parse_nonneg_time(p)?))
}

/// `infinite | <number [0,∞]>`; a math function may be negative (it
/// computes to 0).
fn parse_count(p: &mut Parser<'_>) -> ParseResult<IterationCount> {
    if p.try_parse(|p| p.expect_ident_matching("infinite")).is_ok() {
        return Ok(IterationCount::Infinite);
    }
    let n = parse_number(p)?;
    if n.value < 0.0 && !n.is_math() {
        return Err(ParseError::Invalid);
    }
    Ok(IterationCount::Number(n))
}

/// One item of `animation-timeline`: `auto | none | <dashed-ident> |
/// scroll() | view()`, as its computed text.
fn parse_timeline(p: &mut Parser<'_>) -> ParseResult<Arc<str>> {
    p.try_parse(|p| match p.next() {
        Some(ComponentValue::Ident(ident)) => {
            let lower = ident.to_ascii_lowercase();
            if lower == "auto" || lower == "none" {
                Ok(Arc::from(lower))
            } else if ident.starts_with("--") {
                Ok(Arc::from(&**ident))
            } else {
                Err(ParseError::Invalid)
            }
        }
        Some(ComponentValue::Function(f)) => {
            let mut args = Parser::new(&f.arguments);
            if f.name.eq_ignore_ascii_case("scroll") {
                args.parse_entirely(parse_scroll_args).map(Arc::from)
            } else if f.name.eq_ignore_ascii_case("view") {
                args.parse_entirely(parse_view_args).map(Arc::from)
            } else {
                Err(ParseError::Invalid)
            }
        }
        _ => Err(ParseError::Unexpected),
    })
}

/// The axis keyword of `scroll()` and `view()`.
fn axis(p: &mut Parser<'_>) -> ParseResult<&'static str> {
    p.expect_one_of(&[
        ("block", "block"),
        ("inline", "inline"),
        ("x", "x"),
        ("y", "y"),
    ])
}

/// `scroll([<scroller> || <axis>]?)`, without the defaults `nearest` and
/// `block`.
fn parse_scroll_args(p: &mut Parser<'_>) -> ParseResult<String> {
    let mut scroller = None;
    let mut axis_word = None;
    while !p.is_exhausted() {
        if scroller.is_none()
            && let Ok(s) = p.try_parse(|p| {
                p.expect_one_of(&[("nearest", "nearest"), ("root", "root"), ("self", "self")])
            })
        {
            scroller = Some(s);
        } else if axis_word.is_none()
            && let Ok(a) = p.try_parse(axis)
        {
            axis_word = Some(a);
        } else {
            return Err(ParseError::Invalid);
        }
    }
    let parts: Vec<&str> = [
        scroller.filter(|s| *s != "nearest"),
        axis_word.filter(|a| *a != "block"),
    ]
    .into_iter()
    .flatten()
    .collect();
    Ok(format!("scroll({})", parts.join(" ")))
}

/// `view([<axis> || <'view-timeline-inset'>]?)`, without the defaults
/// `block` and `auto`; the axis first, then the insets (one when both
/// are equal).
fn parse_view_args(p: &mut Parser<'_>) -> ParseResult<String> {
    let all = *p;
    let mut axis_word = None;
    let mut insets = 0;
    while !p.is_exhausted() {
        if axis_word.is_none()
            && let Ok(a) = p.try_parse(axis)
        {
            axis_word = Some(a);
        } else if insets < 2
            && p.try_parse(|p| {
                if p.try_parse(|p| p.expect_ident_matching("auto")).is_ok() {
                    return Ok(());
                }
                parse_length_percentage(p, LengthOptions::LENGTH_PERCENTAGE).map(|_| ())
            })
            .is_ok()
        {
            insets += 1;
        } else {
            return Err(ParseError::Invalid);
        }
    }
    let text = crate::cssom::serialize_value(all.remaining(), crate::cssom::ValueStyle::LENGTH);
    let words: Vec<&str> = text.split(' ').filter(|w| !w.is_empty()).collect();
    let is_axis = |w: &&str| matches!(*w, "block" | "inline" | "x" | "y");
    let mut parts: Vec<&str> = words
        .iter()
        .filter(|w| is_axis(w) && **w != "block")
        .copied()
        .collect();
    let mut inset_words: Vec<&str> = words.iter().filter(|w| !is_axis(w)).copied().collect();
    if inset_words.len() == 2 && inset_words.first() == inset_words.get(1) {
        inset_words.pop();
    }
    if !inset_words.iter().all(|w| *w == "auto") {
        parts.extend(inset_words);
    }
    Ok(format!("view({})", parts.join(" ")))
}

/// One item of `animation-range-start` or `-end`: `normal |
/// <length-percentage> | <timeline-range-name> <length-percentage>?`.
fn parse_range_item(p: &mut Parser<'_>) -> ParseResult<RangeItem> {
    if p.try_parse(|p| p.expect_ident_matching("normal")).is_ok() {
        return Ok(RangeItem::Normal);
    }
    if let Ok(name) = p.try_parse(range_name) {
        let offset = p
            .try_parse(|p| parse_length_percentage(p, LengthOptions::LENGTH_PERCENTAGE))
            .ok();
        return Ok(RangeItem::Named(name, offset));
    }
    Ok(RangeItem::Offset(parse_length_percentage(
        p,
        LengthOptions::LENGTH_PERCENTAGE,
    )?))
}

fn range_name(p: &mut Parser<'_>) -> ParseResult<&'static str> {
    let ident = p.expect_ident()?;
    RANGE_NAMES
        .iter()
        .find(|n| ident.eq_ignore_ascii_case(n))
        .copied()
        .ok_or(ParseError::Unexpected)
}

// ----- The shorthands -----

/// One layer of `animation`.
#[derive(Default)]
struct Layer {
    /// `None`: not given; `Some(None)`: `auto`.
    #[allow(clippy::option_option)]
    duration: Option<Option<Time>>,
    easing: Option<Easing>,
    delay: Option<Time>,
    count: Option<IterationCount>,
    direction: Option<PlaybackDirection>,
    fill: Option<FillMode>,
    play_state: Option<PlayState>,
    name: Option<AnimationName>,
}

/// One layer of `animation`: the parts in any order, each at most once.
/// The first time is the duration and the second the delay; a keyword of
/// a part that is not set yet is that part (so `ease ease` is the easing
/// `ease` and the name `ease`, and `none` is the fill mode first).
fn parse_layer(p: &mut Parser<'_>) -> ParseResult<Layer> {
    let mut layer = Layer::default();
    let mut any = false;
    while !p.is_exhausted() {
        any = true;
        if let Ok(t) = p.try_parse(parse_time) {
            if layer.duration.is_none() {
                if t.seconds < 0.0 && !t.is_math() {
                    // A negative literal is not a duration: Chromium takes
                    // it as the delay (`a -1s`).
                    if layer.delay.is_some() {
                        return Err(ParseError::Invalid);
                    }
                    layer.delay = Some(t);
                    continue;
                }
                layer.duration = Some(Some(t));
            } else if layer.delay.is_none() {
                layer.delay = Some(t);
            } else {
                return Err(ParseError::Invalid);
            }
            continue;
        }
        if layer.duration.is_none() && p.try_parse(|p| p.expect_ident_matching("auto")).is_ok() {
            layer.duration = Some(None);
            continue;
        }
        if layer.easing.is_none()
            && let Ok(e) = parse_easing(p)
        {
            layer.easing = Some(e);
            continue;
        }
        if layer.count.is_none()
            && let Ok(c) = parse_count(p)
        {
            layer.count = Some(c);
            continue;
        }
        if layer.direction.is_none()
            && let Ok(d) = keyword(p, direction_of)
        {
            layer.direction = Some(d);
            continue;
        }
        if layer.fill.is_none()
            && let Ok(f) = keyword(p, fill_of)
        {
            layer.fill = Some(f);
            continue;
        }
        if layer.play_state.is_none()
            && let Ok(s) = keyword(p, play_state_of)
        {
            layer.play_state = Some(s);
            continue;
        }
        if layer.name.is_none()
            && let Ok(n) = parse_name(p)
        {
            layer.name = Some(n);
            continue;
        }
        return Err(ParseError::Invalid);
    }
    if any {
        Ok(layer)
    } else {
        Err(ParseError::EndOfInput)
    }
}

/// `animation`: `<single-animation>#`. Sets the eight longhands of the
/// layers, and resets `animation-timeline` and the range to their initial
/// values (Chromium's shorthand; `animation-composition` is not part of
/// it).
/// <https://drafts.csswg.org/css-animations-1/#animation>
pub(crate) fn parse_animation(p: &mut Parser<'_>, out: &mut Vec<LonghandValue>) -> ParseResult<()> {
    let layers = p.parse_comma_separated(parse_layer)?;
    out.push(LonghandValue::AnimationDuration(
        layers.iter().map(|l| l.duration.flatten()).collect(),
    ));
    out.push(LonghandValue::AnimationTimingFunction(
        layers
            .iter()
            .map(|l| l.easing.clone().unwrap_or(Easing::Ease))
            .collect(),
    ));
    out.push(LonghandValue::AnimationDelay(
        layers
            .iter()
            .map(|l| l.delay.unwrap_or(Time::seconds(0.0)))
            .collect(),
    ));
    out.push(LonghandValue::AnimationIterationCount(
        layers
            .iter()
            .map(|l| {
                l.count
                    .unwrap_or(IterationCount::Number(Number::literal(1.0)))
            })
            .collect(),
    ));
    out.push(LonghandValue::AnimationDirection(
        layers
            .iter()
            .map(|l| l.direction.unwrap_or_default())
            .collect(),
    ));
    out.push(LonghandValue::AnimationFillMode(
        layers.iter().map(|l| l.fill.unwrap_or_default()).collect(),
    ));
    out.push(LonghandValue::AnimationPlayState(
        layers
            .iter()
            .map(|l| l.play_state.unwrap_or_default())
            .collect(),
    ));
    out.push(LonghandValue::AnimationName(
        layers
            .iter()
            .map(|l| l.name.clone().unwrap_or(AnimationName::None))
            .collect(),
    ));
    out.push(LonghandValue::AnimationTimeline(Arc::from([Arc::from(
        "auto",
    )])));
    out.push(LonghandValue::AnimationRangeStart(Arc::from([
        RangeItem::Normal,
    ])));
    out.push(LonghandValue::AnimationRangeEnd(Arc::from([
        RangeItem::Normal,
    ])));
    Ok(())
}

/// One item of `animation-range`: a start and an optional end. Without
/// an end, a named start gives the end the same name at 100%; else the
/// end is `normal`.
fn parse_range_pair(p: &mut Parser<'_>) -> ParseResult<(RangeItem, RangeItem)> {
    let start = parse_range_item(p)?;
    let end = if p.is_exhausted() {
        match &start {
            RangeItem::Named(name, _) => RangeItem::Named(name, None),
            _ => RangeItem::Normal,
        }
    } else {
        parse_range_item(p)?
    };
    Ok((start, end))
}

/// `animation-range`: `[<'animation-range-start'> <'animation-range-end'>?]#`.
pub(crate) fn parse_animation_range(
    p: &mut Parser<'_>,
    out: &mut Vec<LonghandValue>,
) -> ParseResult<()> {
    let pairs = p.parse_comma_separated(parse_range_pair)?;
    let (starts, ends): (Vec<RangeItem>, Vec<RangeItem>) = pairs.into_iter().unzip();
    out.push(LonghandValue::AnimationRangeStart(Arc::from(starts)));
    out.push(LonghandValue::AnimationRangeEnd(Arc::from(ends)));
    Ok(())
}

// ----- Texts (in `text.rs`) -----

mod text;

pub(crate) use text::{
    SHORTHAND_LONGHANDS, animation_value, computed_range, computed_shorthand, computed_text,
    declared_text, expand_animation, expand_range, lists_match, range_value,
};

#[cfg(test)]
mod tests;
