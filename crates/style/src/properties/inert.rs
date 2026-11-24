//! Properties that layout and paint ignore: `will-change` (CSS Will
//! Change 1), `touch-action` (Pointer Events 3), `perspective` and
//! `perspective-origin` (CSS Transforms 2), with the `-webkit-` aliases that
//! Chromium has; and `scroll-margin-*` and `scroll-padding-*` (CSS Scroll
//! Snap 1), which only the scroll to an element reads.
//!
//! swb parses them with their grammar, cascades and computes them, and
//! shows them to scripts (`element.style`, `getComputedStyle`,
//! `CSS.supports`), but draws no perspective. Pages test and write these
//! properties (Leaflet's tests for the 3D support). The scroll margin and padding
//! are read by the scroll to a fragment, `scrollIntoView()` and focus
//! scrolling ([`InertStyle::scroll_margin`], [`InertStyle::scroll_padding`]).
//!
//! The computed values of the properties live in one optional, shared
//! record, [`InertStyle`] (`ComputedStyle::inert`): an element that sets
//! none of them pays one `None`. A value of `will-change` and
//! `touch-action` in the record is the text of the computed value, so the
//! cascade, `inherit` and `initial` need no code per property.
//!
//! The `transition-*` properties were here until M13 feature 5; they are
//! typed now (`animation/transition_properties.rs`).
//!
//! Departure from Chromium: `will-change` accepts every identifier that is
//! not reserved, as the specification does.
//!
//! <https://www.w3.org/TR/css-will-change-1/>,
//! <https://www.w3.org/TR/pointerevents3/#the-touch-action-css-property>,
//! <https://www.w3.org/TR/css-transforms-2/#perspective-property>,
//! <https://drafts.csswg.org/css-scroll-snap-1/#scroll-margin>,
//! <https://drafts.csswg.org/css-scroll-snap-1/#scroll-padding>.

use std::sync::Arc;

use swb_css::{ComponentValue, ParseError, Parser};

use super::compute::ComputeContext;
use super::ids::{LonghandId, LonghandValue};
use super::longhand::parse_position;
use crate::ComputedStyle;
use crate::parse::ParseResult;
use crate::parse::length::{LengthOptions, parse_length_percentage};
use crate::values::{LengthPercentage, PositionComponent, SpecifiedLengthPercentage as Lp};

/// The computed values of the properties of this module. `None` is the
/// initial value of each.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InertStyle {
    /// `will-change` (initial `auto`).
    pub(crate) will_change: Option<Arc<str>>,
    /// What the properties in `will_change` do to stacking contexts and
    /// containing blocks; made when the value is computed.
    pub(crate) will_change_effects: WillChangeEffects,
    /// `touch-action` (initial `auto`).
    pub(crate) touch_action: Option<Arc<str>>,
    /// `perspective` in px (initial `none`).
    pub(crate) perspective: Option<f32>,
    /// `perspective-origin` (initial `50% 50%`).
    pub(crate) perspective_origin: Option<[PositionComponent; 2]>,
    /// `scroll-margin-top`, `-right`, `-bottom`, `-left` in px (initial 0).
    pub(crate) scroll_margin: [f32; 4],
    /// `scroll-padding-top`, `-right`, `-bottom`, `-left` (initial `auto`,
    /// which is `None`).
    pub(crate) scroll_padding: [Option<LengthPercentage>; 4],
}

/// The record with every value initial.
static INITIAL: InertStyle = InertStyle {
    will_change: None,
    will_change_effects: WillChangeEffects::NONE,
    touch_action: None,
    perspective: None,
    perspective_origin: None,
    scroll_margin: [0.0; 4],
    scroll_padding: [None, None, None, None],
};

/// What the properties that `will-change` names do to the element (CSS
/// Will Change 1 §3, <https://drafts.csswg.org/css-will-change/#will-change>:
/// naming a property whose non-initial value makes a stacking context or a
/// containing block does the same). Made once when the value is computed.
/// Measured in Chromium 148 (ADR 0033).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WillChangeEffects {
    /// A stacking context and a backdrop root: `opacity`, `filter`,
    /// `backdrop-filter`, a mask property or `clip-path`.
    pub(crate) stacking_context: bool,
    /// The containing block of absolutely positioned and fixed
    /// descendants: `filter` and `backdrop-filter`. (Chromium also does so
    /// for `transform`, `perspective` and, for absolute ones, `position`;
    /// swb does not, because `will-change` is inert for them.)
    pub(crate) containing_block: bool,
}

impl WillChangeEffects {
    /// `will-change: auto`.
    const NONE: WillChangeEffects = WillChangeEffects {
        stacking_context: false,
        containing_block: false,
    };

    /// The effects of the comma-separated `will-change` list `text`.
    fn of(text: &str) -> WillChangeEffects {
        const FILTERS: [&str; 3] = ["filter", "-webkit-filter", "backdrop-filter"];
        const OTHERS: [&str; 7] = [
            "opacity",
            "mask",
            "mask-image",
            "-webkit-mask",
            "-webkit-mask-image",
            "clip-path",
            "-webkit-clip-path",
        ];
        let names = |list: &[&str]| {
            text.split(',')
                .map(str::trim)
                .any(|item| list.iter().any(|k| item.eq_ignore_ascii_case(k)))
        };
        let filter = names(&FILTERS);
        WillChangeEffects {
            stacking_context: filter || names(&OTHERS),
            containing_block: filter,
        }
    }
}

/// The index of a side in `scroll_margin` and `scroll_padding`: top,
/// right, bottom, left.
fn side_index(id: LonghandId) -> usize {
    use LonghandId as L;
    match id {
        L::ScrollMarginTop | L::ScrollPaddingTop => 0,
        L::ScrollMarginRight | L::ScrollPaddingRight => 1,
        L::ScrollMarginBottom | L::ScrollPaddingBottom => 2,
        _ => 3,
    }
}

impl InertStyle {
    /// `scroll-margin` of the element in px: top, right, bottom, left.
    /// The scroll to the element treats the margin box as the part of the
    /// element to reveal.
    pub fn scroll_margin(style: &ComputedStyle) -> [f32; 4] {
        style.inert.as_deref().unwrap_or(&INITIAL).scroll_margin
    }

    /// `scroll-padding` of the element: top, right, bottom, left, `None`
    /// for `auto` (no padding). A scroll container leaves this much of its
    /// scrollport out of the area where the scroll to an element places the
    /// element; a percentage is of the scrollport's size on that axis.
    pub fn scroll_padding(style: &ComputedStyle) -> [Option<LengthPercentage>; 4] {
        style
            .inert
            .as_deref()
            .unwrap_or(&INITIAL)
            .scroll_padding
            .clone()
    }

    /// What the `will-change` list of the element does to its stacking
    /// context and containing blocks.
    pub(crate) fn will_change_effects(style: &ComputedStyle) -> WillChangeEffects {
        style
            .inert
            .as_deref()
            .unwrap_or(&INITIAL)
            .will_change_effects
    }

    /// The text of the text-valued longhand `id` (its initial value if
    /// unset); `None` for the other longhands.
    pub(crate) fn text(style: &ComputedStyle, id: LonghandId) -> Option<&str> {
        use LonghandId as L;
        let inert = style.inert.as_deref().unwrap_or(&INITIAL);
        let (value, initial) = match id {
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
            L::WillChange => {
                self.will_change.clone_from(&s.will_change);
                self.will_change_effects = s.will_change_effects;
            }
            L::TouchAction => self.touch_action.clone_from(&s.touch_action),
            L::Perspective => self.perspective = s.perspective,
            L::PerspectiveOrigin => self.perspective_origin.clone_from(&s.perspective_origin),
            L::ScrollMarginTop
            | L::ScrollMarginRight
            | L::ScrollMarginBottom
            | L::ScrollMarginLeft => {
                self.scroll_margin[side_index(id)] = s.scroll_margin[side_index(id)];
            }
            L::ScrollPaddingTop
            | L::ScrollPaddingRight
            | L::ScrollPaddingBottom
            | L::ScrollPaddingLeft => {
                self.scroll_padding[side_index(id)].clone_from(&s.scroll_padding[side_index(id)]);
            }
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

/// True if `a` and `b` have the same computed value of the longhand `id`.
pub(crate) fn same_value(id: LonghandId, a: &ComputedStyle, b: &ComputedStyle) -> bool {
    use LonghandId as L;
    let (a, b) = (
        a.inert.as_deref().unwrap_or(&INITIAL),
        b.inert.as_deref().unwrap_or(&INITIAL),
    );
    match id {
        L::WillChange => a.will_change == b.will_change,
        L::TouchAction => a.touch_action == b.touch_action,
        L::Perspective => a.perspective == b.perspective,
        L::PerspectiveOrigin => a.perspective_origin == b.perspective_origin,
        L::ScrollMarginTop | L::ScrollMarginRight | L::ScrollMarginBottom | L::ScrollMarginLeft => {
            a.scroll_margin[side_index(id)] == b.scroll_margin[side_index(id)]
        }
        L::ScrollPaddingTop
        | L::ScrollPaddingRight
        | L::ScrollPaddingBottom
        | L::ScrollPaddingLeft => {
            a.scroll_padding[side_index(id)] == b.scroll_padding[side_index(id)]
        }
        _ => a == b,
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
        V::WillChange(v) => {
            r.will_change = Some(Arc::clone(v));
            r.will_change_effects = WillChangeEffects::of(v);
        }
        V::TouchAction(v) => r.touch_action = Some(Arc::clone(v)),
        V::Perspective(v) => r.perspective = v.as_ref().map(|lp| cx.px(lp).max(0.0)),
        V::PerspectiveOrigin([x, y]) => {
            r.perspective_origin = Some([x.compute(cx), y.compute(cx)]);
        }
        V::ScrollMarginTop(v)
        | V::ScrollMarginRight(v)
        | V::ScrollMarginBottom(v)
        | V::ScrollMarginLeft(v) => r.scroll_margin[side_index(value.id())] = cx.px(v),
        V::ScrollPaddingTop(v)
        | V::ScrollPaddingRight(v)
        | V::ScrollPaddingBottom(v)
        | V::ScrollPaddingLeft(v) => {
            r.scroll_padding[side_index(value.id())] = v.as_ref().map(|lp| cx.lp(lp));
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
        L::WillChange => V::WillChange(parse_will_change(p)?),
        L::TouchAction => V::TouchAction(parse_touch_action(p)?),
        L::Perspective => V::Perspective(parse_perspective(p)?),
        L::PerspectiveOrigin => {
            let (x, y) = parse_position(p)?;
            V::PerspectiveOrigin([x, y])
        }
        L::ScrollMarginTop => V::ScrollMarginTop(scroll_margin(p)?),
        L::ScrollMarginRight => V::ScrollMarginRight(scroll_margin(p)?),
        L::ScrollMarginBottom => V::ScrollMarginBottom(scroll_margin(p)?),
        L::ScrollMarginLeft => V::ScrollMarginLeft(scroll_margin(p)?),
        L::ScrollPaddingTop => V::ScrollPaddingTop(scroll_padding(p)?),
        L::ScrollPaddingRight => V::ScrollPaddingRight(scroll_padding(p)?),
        L::ScrollPaddingBottom => V::ScrollPaddingBottom(scroll_padding(p)?),
        L::ScrollPaddingLeft => V::ScrollPaddingLeft(scroll_padding(p)?),
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

/// Writes a list of items as `a, b`.
fn join(items: &[String]) -> Arc<str> {
    Arc::from(items.join(", "))
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

// ----- scroll-margin and scroll-padding -----

/// `<length>` (any sign): one side of `scroll-margin`.
pub(crate) fn scroll_margin(p: &mut Parser<'_>) -> ParseResult<Lp> {
    parse_length_percentage(p, LengthOptions::LENGTH)
}

/// `auto | <length-percentage [0,∞]>`: one side of `scroll-padding`;
/// `auto` is `None`.
pub(crate) fn scroll_padding(p: &mut Parser<'_>) -> ParseResult<Option<Lp>> {
    if p.try_parse(|p| p.expect_ident_matching("auto")).is_ok() {
        return Ok(None);
    }
    parse_length_percentage(p, LengthOptions::NON_NEGATIVE).map(Some)
}

// ----- element.style -----

/// The text of the value `tokens` of the longhand `name` as `element.style`
/// writes it, for the longhands whose text differs from the generic
/// writing; `None` if `tokens` do not parse.
pub(crate) fn declared_text(name: &str, tokens: &[ComponentValue]) -> Option<String> {
    match name {
        "will-change" => declared_will_change(tokens),
        "touch-action" => declared_touch_action(tokens),
        _ => None,
    }
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
            LonghandValue::WillChange(v) | LonghandValue::TouchAction(v) => Some(v.to_string()),
            _ => None,
        }
    }

    #[test]
    fn will_change_effects_are_computed_from_the_list() {
        let effects = WillChangeEffects::of;
        let context = |stacking_context, containing_block| WillChangeEffects {
            stacking_context,
            containing_block,
        };
        assert_eq!(effects("auto"), WillChangeEffects::NONE);
        assert_eq!(effects("transform, contents"), WillChangeEffects::NONE);
        assert_eq!(effects("Opacity"), context(true, false));
        assert_eq!(effects("top, clip-path"), context(true, false));
        assert_eq!(effects("mask-image"), context(true, false));
        assert_eq!(effects("filter"), context(true, true));
        assert_eq!(effects("left,BACKDROP-FILTER"), context(true, true));
        assert_eq!(effects("opacity, -webkit-filter"), context(true, true));
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
