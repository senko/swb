//! The cascade and style computation for a document.
//!
//! [`compute_styles`] walks the element tree (iteratively, so deep trees
//! cannot overflow the stack) and computes one style per element, plus
//! styles for `::before`, `::after` and `::marker`.
//!
//! For each element:
//!
//! 1. Find the matching rules ([`Stylist::matching_rules`]), the
//!    presentational hints ([`crate::hints`]) and the `style` attribute.
//! 2. Pick the winning declaration of each property, in this order of
//!    precedence (CSS Cascade 4, <https://www.w3.org/TR/css-cascade-4/#cascade-sort>):
//!    user-agent `!important`, author `!important` (the `style` attribute
//!    before rules), author normal (the `style` attribute, then rules,
//!    then presentational hints), user-agent normal. Within an origin,
//!    higher specificity wins, then later source order.
//! 3. Compute custom properties, then `font-family` and `font-size`
//!    (other properties depend on the font size), then all other
//!    properties. Values with `var()` are substituted and parsed here.
//! 4. Apply the computed-value fixups: blockification
//!    (<https://www.w3.org/TR/css-display-3/#transformations>), `float`
//!    for absolutely positioned boxes, zero border widths for `none`
//!    styles, and the `overflow` pairing rule.
//!
//! Elements inside a `display: none` subtree get no style.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use swb_css::{MatchingContext, MediaEnvironment, parse_style_attribute};
use swb_dom::{Document, ElementData, NodeId};
use url::Url;

use crate::ComputedStyle;
use crate::bloom::AncestorFilter;
use crate::custom::{compute_custom_properties, substitute};
use crate::element::{DomElement, ElementStates, StateSet};
use crate::hints::{HintContext, collect_hints};
use crate::parse::ParserContext;
use crate::properties::compute::{ComputeContext, apply, is_monospace};
use crate::properties::ids::LonghandValue;
use crate::properties::{
    CssWideKeyword, CustomDeclaration, DeclarationBlock, LonghandId, PropertyDeclaration,
    UnparsedValue, parse_property_value,
};
use crate::style_map::{PseudoKind, StyleMap};
use crate::stylist::{CascadeOrigin, RuleTarget, Stylist};
use crate::values::{
    Content, Display, Float, FontSizeOrigin, LengthContext, Overflow, TextAlign, UserSelect,
};

/// The ratio of the default fixed (monospace) font size to the default
/// font size: 13px / 16px.
const FIXED_FONT_RATIO: f32 = 13.0 / 16.0;

/// Computes styles for all elements (and `::before`, `::after` and
/// `::marker`) of the document.
pub fn compute_styles(
    doc: &Document,
    stylist: &Stylist,
    env: &MediaEnvironment,
    states: &ElementStates,
    base_url: &Url,
) -> StyleMap {
    compute_styles_with_options(doc, stylist, env, states, base_url, true)
}

/// [`compute_styles`] with the ancestor Bloom filter optional (tests check
/// that the filter does not change the result).
pub(crate) fn compute_styles_with_options(
    doc: &Document,
    stylist: &Stylist,
    env: &MediaEnvironment,
    states: &ElementStates,
    base_url: &Url,
    use_filter: bool,
) -> StyleMap {
    let state_set = StateSet::new(doc, states);
    let quirks = stylist.quirks();
    let author = ParserContext::author(Some(Arc::new(base_url.clone())), quirks);
    let mut styler = Styler {
        doc,
        states: &state_set,
        cx: CascadeContext {
            stylist,
            env,
            quirks,
            root_font_size: env.root_font_size,
            initial: ComputedStyle::initial(),
        },
        media: stylist.evaluate_media(env),
        matching: MatchingContext::new(if quirks {
            swb_css::QuirksMode::Quirks
        } else {
            swb_css::QuirksMode::NoQuirks
        }),
        hints: HintContext::new(doc, author.clone(), quirks),
        author,
        filter: use_filter.then(AncestorFilter::new),
        shared: HashMap::new(),
        scratch: Vec::new(),
        matched: Vec::new(),
    };
    styler.run()
}

/// What a shareable element style depends on. The parent style is
/// identified by its address: the style map keeps all parent styles alive
/// while styles are computed, so addresses are not reused.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct SharingKey {
    parent: usize,
    layout_parent: Option<Display>,
    /// See [`contents_is_none`].
    contents_is_none: bool,
    rules: Box<[u32]>,
}

/// Settings shared by all cascades of one document.
struct CascadeContext<'a> {
    stylist: &'a Stylist,
    env: &'a MediaEnvironment,
    quirks: bool,
    /// The root element's font size, for `rem` (the environment's default
    /// until the root is styled).
    root_font_size: f32,
    initial: Arc<ComputedStyle>,
}

/// The declarations that apply to an element, besides the matched rules.
#[derive(Clone, Copy, Default)]
struct ExtraDeclarations<'d> {
    /// Presentational hints computed from attributes.
    hints: &'d [PropertyDeclaration],
    /// The `style` attribute.
    style_attribute: Option<&'d DeclarationBlock>,
}

struct Styler<'a> {
    doc: &'a Document,
    states: &'a StateSet<'a>,
    cx: CascadeContext<'a>,
    media: Vec<bool>,
    matching: MatchingContext,
    hints: HintContext,
    author: ParserContext,
    /// The ancestors of the element being styled (see [`crate::bloom`]).
    filter: Option<AncestorFilter>,
    /// Styles that elements with the same inputs share.
    shared: HashMap<SharingKey, Arc<ComputedStyle>>,
    scratch: Vec<u32>,
    matched: Vec<u32>,
}

impl Styler<'_> {
    fn run(&mut self) -> StyleMap {
        let mut map = StyleMap::with_capacity(self.doc.len());
        let Some(root) = self.doc.document_element() else {
            return map;
        };
        // Pre-order traversal. `path` holds the ancestors of the current
        // element, whose keys are in the Bloom filter.
        let mut stack: Vec<(NodeId, Arc<ComputedStyle>, Option<Display>, usize)> =
            vec![(root, Arc::clone(&self.cx.initial), None, 0)];
        let mut path: Vec<NodeId> = Vec::new();
        let mut children = Vec::new();
        let quirks = self.cx.quirks;
        while let Some((node, parent, layout_parent, depth)) = stack.pop() {
            while path.len() > depth {
                if let Some(ancestor) = path.pop()
                    && let (Some(filter), Some(e)) = (&mut self.filter, self.doc.element(ancestor))
                {
                    filter.pop_element(e, quirks);
                }
            }
            let Some(el) = DomElement::new(self.doc, node, self.states) else {
                continue;
            };
            let is_root = node == root;
            let style = self.style_element(&el, &parent, layout_parent, is_root);
            if is_root {
                self.cx.root_font_size = style.font_size;
            }
            if style.display != Display::None {
                // Children (and pseudo-elements) of the element see the
                // element's box as their parent, or the element's own
                // parent box for `display: contents`.
                let child_layout_parent = if style.display == Display::Contents {
                    layout_parent
                } else {
                    Some(style.display)
                };
                self.style_pseudo_elements(&el, &style, child_layout_parent, &mut map);
                children.clear();
                children.extend(self.doc.element_children(node));
                if !children.is_empty() {
                    if let Some(filter) = &mut self.filter {
                        filter.push_element(el.data(), quirks);
                    }
                    path.push(node);
                }
                for &child in children.iter().rev() {
                    stack.push((child, Arc::clone(&style), child_layout_parent, depth + 1));
                }
            }
            map.set(node, style);
        }
        map
    }

    fn style_element(
        &mut self,
        el: &DomElement<'_>,
        parent: &Arc<ComputedStyle>,
        layout_parent: Option<Display>,
        is_root: bool,
    ) -> Arc<ComputedStyle> {
        let data = el.data();
        let mut matched = std::mem::take(&mut self.matched);
        matched.clear();
        self.cx.stylist.matching_rules(
            el,
            RuleTarget::Element,
            &self.media,
            self.filter.as_ref(),
            &mut self.matching,
            &mut self.scratch,
            &mut matched,
        );
        let mut hints = Vec::new();
        collect_hints(self.doc, el.node_id(), data, &self.hints, &mut hints);
        let style_attribute = data
            .attr("style")
            .map(|css| DeclarationBlock::parse(&parse_style_attribute(css), &self.author));
        // Elements with the same parent style, the same rules and nothing
        // element-specific share one style.
        let stylist = self.cx.stylist;
        let shareable = !is_root
            && hints.is_empty()
            && style_attribute.is_none()
            && matched.iter().all(|&i| !stylist.rule(i).uses_element_data);
        let contents_is_none = contents_is_none(data);
        let key = shareable.then(|| SharingKey {
            parent: Arc::as_ptr(parent) as usize,
            layout_parent,
            contents_is_none,
            rules: matched.clone().into_boxed_slice(),
        });
        if let Some(shared) = key.as_ref().and_then(|k| self.shared.get(k)) {
            let shared = Arc::clone(shared);
            self.matched = matched;
            return shared;
        }
        let extra = ExtraDeclarations {
            hints: &hints,
            style_attribute: style_attribute.as_ref(),
        };
        let mut style = cascade(&self.cx, &matched, extra, parent, Some(data), is_root);
        apply_fixups(&mut style, is_root, layout_parent);
        // Chromium (`StyleAdjuster`): tables do not take the `-webkit-`
        // values of `text-align`, so `<center>` and `align` do not align the
        // content of their cells.
        if data.is_html_named(&swb_dom::local_name!("table"))
            && matches!(
                style.text_align,
                TextAlign::WebkitLeft | TextAlign::WebkitCenter | TextAlign::WebkitRight
            )
        {
            style.text_align = TextAlign::Start;
        }
        if contents_is_none && style.display == Display::Contents {
            style.display = Display::None;
        }
        self.matched = matched;
        let style = Arc::new(style);
        if let Some(key) = key {
            self.shared.insert(key, Arc::clone(&style));
        }
        style
    }

    /// Styles the pseudo-elements of `el`, whose style is `style`.
    /// `pseudo_layout_parent` is the display of their parent box.
    fn style_pseudo_elements(
        &mut self,
        el: &DomElement<'_>,
        style: &Arc<ComputedStyle>,
        pseudo_layout_parent: Option<Display>,
        map: &mut StyleMap,
    ) {
        let data = el.data();
        let mut kinds = Vec::with_capacity(3);
        if generates_content_pseudos(data) {
            kinds.extend([PseudoKind::Before, PseudoKind::After]);
        }
        if style.display == Display::ListItem {
            kinds.push(PseudoKind::Marker);
        }
        if has_placeholder(data) {
            kinds.push(PseudoKind::Placeholder);
        }
        for kind in kinds {
            let mut matched = std::mem::take(&mut self.matched);
            matched.clear();
            if self.cx.stylist.has_pseudo_rules(kind) {
                self.cx.stylist.matching_rules(
                    el,
                    RuleTarget::Pseudo(kind),
                    &self.media,
                    self.filter.as_ref(),
                    &mut self.matching,
                    &mut self.scratch,
                    &mut matched,
                );
            }
            let is_marker = kind == PseudoKind::Marker;
            // Markers and placeholders always have a box (the user-agent
            // sheet has rules for both).
            let always = is_marker || kind == PseudoKind::Placeholder;
            if !matched.is_empty() || always {
                let mut pseudo = cascade(
                    &self.cx,
                    &matched,
                    ExtraDeclarations::default(),
                    style,
                    Some(data),
                    false,
                );
                if !always {
                    apply_fixups(&mut pseudo, false, pseudo_layout_parent);
                }
                let generates_box = always
                    || (!matches!(pseudo.content, Content::Normal | Content::None)
                        && pseudo.display != Display::None);
                if generates_box {
                    map.set_pseudo(el.node_id(), kind, Arc::new(pseudo));
                }
            }
            self.matched = matched;
        }
    }
}

/// True if `display: contents` computes to `none` for the element:
/// replaced elements and form controls.
/// <https://www.w3.org/TR/css-display-3/#unbox-html>
fn contents_is_none(data: &ElementData) -> bool {
    data.is_html()
        && matches!(
            &**data.local_name(),
            "br" | "wbr"
                | "meter"
                | "progress"
                | "canvas"
                | "embed"
                | "object"
                | "audio"
                | "iframe"
                | "img"
                | "video"
                | "input"
                | "textarea"
                | "select"
                | "frame"
                | "frameset"
        )
}

/// True if `::placeholder` applies: an `input` or `textarea` with a
/// `placeholder` attribute. (Input types without a placeholder, such as
/// checkboxes, never show it.)
fn has_placeholder(data: &ElementData) -> bool {
    data.is_html()
        && matches!(&**data.local_name(), "input" | "textarea")
        && data.has_attr("placeholder")
}

/// True if `::before` and `::after` apply: not for replaced elements and
/// form controls (as in Chromium).
fn generates_content_pseudos(data: &ElementData) -> bool {
    !(data.is_html()
        && matches!(
            &**data.local_name(),
            "img"
                | "input"
                | "select"
                | "textarea"
                | "iframe"
                | "video"
                | "audio"
                | "canvas"
                | "embed"
                | "object"
                | "br"
                | "wbr"
        ))
}

/// The cascade origin of a group of declarations, for `revert`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Level {
    /// Author rules, the `style` attribute and presentational hints.
    Author,
    /// The user-agent stylesheet.
    UserAgent,
}

/// The winning declaration of each property.
struct Winners<'d> {
    longhands: [Option<&'d PropertyDeclaration>; LonghandId::COUNT],
    /// Properties for which an author-level `revert` won: all further
    /// author-level declarations are skipped, so the user-agent value (if
    /// any) wins. <https://www.w3.org/TR/css-cascade-5/#default>
    reverted: [bool; LonghandId::COUNT],
    custom: Vec<(&'d Arc<str>, &'d CustomDeclaration)>,
    custom_seen: HashSet<&'d str>,
}

impl<'d> Winners<'d> {
    fn new() -> Self {
        Winners {
            longhands: [None; LonghandId::COUNT],
            reverted: [false; LonghandId::COUNT],
            custom: Vec::new(),
            custom_seen: HashSet::new(),
        }
    }

    /// Adds declarations of lower precedence than all added so far. Later
    /// declarations in `list` take precedence over earlier ones.
    fn add_all(&mut self, list: &'d [PropertyDeclaration], level: Level) {
        for decl in list.iter().rev() {
            match decl {
                PropertyDeclaration::Custom(name, value) => {
                    if self.custom_seen.insert(name) {
                        self.custom.push((name, value));
                    }
                }
                other => {
                    let Some(id) = other.longhand() else {
                        continue;
                    };
                    let index = id as usize;
                    if self.longhands[index].is_some()
                        || (self.reverted[index] && level == Level::Author)
                    {
                        continue;
                    }
                    if level == Level::Author
                        && matches!(
                            other,
                            PropertyDeclaration::CssWide(_, CssWideKeyword::Revert)
                        )
                    {
                        self.reverted[index] = true;
                    } else {
                        self.longhands[index] = Some(other);
                    }
                }
            }
        }
    }
}

/// Runs the cascade for one element or pseudo-element and computes its
/// style. `matched` holds rule indices sorted by cascade order.
fn cascade(
    cx: &CascadeContext<'_>,
    matched: &[u32],
    extra: ExtraDeclarations<'_>,
    parent: &ComputedStyle,
    element: Option<&ElementData>,
    is_root: bool,
) -> ComputedStyle {
    let stylist = cx.stylist;
    let rules: Vec<&crate::stylist::Rule> = matched.iter().map(|&i| stylist.rule(i)).collect();
    let ua_end = rules
        .iter()
        .position(|r| r.origin != CascadeOrigin::UserAgent)
        .unwrap_or(rules.len());
    let author_start = rules
        .iter()
        .position(|r| r.origin == CascadeOrigin::Author)
        .unwrap_or(rules.len());
    let ua = &rules[..ua_end];
    let hint_rules = &rules[ua_end..author_start];
    let author = &rules[author_start..];

    let mut winners = Winners::new();
    for r in ua.iter().rev() {
        winners.add_all(&r.block.important, Level::UserAgent);
    }
    if let Some(attr) = extra.style_attribute {
        winners.add_all(&attr.important, Level::Author);
    }
    for r in author.iter().rev() {
        winners.add_all(&r.block.important, Level::Author);
    }
    if let Some(attr) = extra.style_attribute {
        winners.add_all(&attr.normal, Level::Author);
    }
    for r in author.iter().rev() {
        winners.add_all(&r.block.normal, Level::Author);
    }
    winners.add_all(extra.hints, Level::Author);
    for r in hint_rules.iter().rev() {
        winners.add_all(&r.block.normal, Level::Author);
    }
    for r in ua.iter().rev() {
        winners.add_all(&r.block.normal, Level::UserAgent);
    }
    compute(cx, &winners, parent, element, is_root)
}

/// Computes a style from the winning declarations.
fn compute(
    cx: &CascadeContext<'_>,
    winners: &Winners<'_>,
    parent: &ComputedStyle,
    element: Option<&ElementData>,
    is_root: bool,
) -> ComputedStyle {
    let mut style = ComputedStyle::inherit_from(parent);
    style.custom_properties =
        compute_custom_properties(parent.custom_properties.as_ref(), &winners.custom);
    let env = cx.env;
    let mut ccx = ComputeContext {
        parent,
        lengths: LengthContext {
            font_size: parent.font_size,
            root_font_size: if is_root {
                env.root_font_size
            } else {
                cx.root_font_size
            },
            viewport_width: env.viewport_width,
            viewport_height: env.viewport_height,
        },
        element,
        quirks: cx.quirks,
    };
    let mut resolved = ResolvedVariables::default();
    let mut apply_winner = |id: LonghandId, ccx: &ComputeContext<'_>, style: &mut ComputedStyle| {
        if let Some(decl) = winners.longhands[id as usize] {
            apply_declaration(id, decl, ccx, style, &cx.initial, &mut resolved);
        }
    };
    // Font properties first: other values depend on the font size, and
    // the font size depends on the family.
    apply_winner(LonghandId::FontFamily, &ccx, &mut style);
    apply_winner(LonghandId::FontSize, &ccx, &mut style);
    adjust_font_size_for_family(&mut style, parent, cx.quirks);
    ccx.lengths.font_size = style.font_size;
    if is_root {
        ccx.lengths.root_font_size = style.font_size;
    }
    for &id in LonghandId::ALL {
        if !matches!(id, LonghandId::FontFamily | LonghandId::FontSize) {
            apply_winner(id, &ccx, &mut style);
        }
    }
    // `user-select: auto` takes `none` or `all` from the parent. The
    // specification defines this for the used value; Chromium (and swb)
    // store it in the computed value.
    // <https://www.w3.org/TR/css-ui-4/#propdef-user-select>
    if style.user_select == UserSelect::Auto
        && matches!(parent.user_select, UserSelect::None | UserSelect::All)
    {
        style.user_select = parent.user_select;
    }
    style
}

/// Shorthand values with `var()`, substituted and parsed once per element.
#[derive(Default)]
struct ResolvedVariables {
    entries: Vec<(*const UnparsedValue, Option<Resolved>)>,
}

/// The result of substituting and parsing an unparsed value.
enum Resolved {
    Values(Vec<LonghandValue>),
    CssWide(CssWideKeyword),
}

impl ResolvedVariables {
    fn get(
        &mut self,
        unparsed: &Arc<UnparsedValue>,
        custom: Option<&crate::computed::CustomProperties>,
    ) -> Option<&Resolved> {
        let key = Arc::as_ptr(unparsed);
        let index = if let Some(i) = self.entries.iter().position(|(k, _)| *k == key) {
            i
        } else {
            let result = substitute(&unparsed.tokens, custom).and_then(|tokens| {
                if let Some(k) = CssWideKeyword::from_value(&tokens) {
                    return Some(Resolved::CssWide(k));
                }
                let mut values = Vec::new();
                parse_property_value(unparsed.property, &tokens, &unparsed.context, &mut values)
                    .ok()
                    .map(|()| Resolved::Values(values))
            });
            self.entries.push((key, result));
            self.entries.len() - 1
        };
        self.entries[index].1.as_ref()
    }
}

fn apply_declaration(
    id: LonghandId,
    decl: &PropertyDeclaration,
    cx: &ComputeContext<'_>,
    style: &mut ComputedStyle,
    initial: &ComputedStyle,
    resolved: &mut ResolvedVariables,
) {
    match decl {
        PropertyDeclaration::Value(v) => apply(v, cx, style),
        PropertyDeclaration::CssWide(_, keyword) => {
            apply_css_wide(id, *keyword, cx.parent, initial, style);
        }
        PropertyDeclaration::WithVariables(_, unparsed) => {
            let custom = style.custom_properties.clone();
            match resolved.get(unparsed, custom.as_deref()) {
                Some(Resolved::Values(values)) => match values.iter().find(|v| v.id() == id) {
                    Some(v) => apply(v, cx, style),
                    None => apply_css_wide(id, CssWideKeyword::Unset, cx.parent, initial, style),
                },
                Some(Resolved::CssWide(keyword)) => {
                    apply_css_wide(id, *keyword, cx.parent, initial, style);
                }
                // Invalid at computed-value time: the property is `unset`.
                // <https://www.w3.org/TR/css-variables-1/#invalid-at-computed-value-time>
                None => apply_css_wide(id, CssWideKeyword::Unset, cx.parent, initial, style),
            }
        }
        PropertyDeclaration::Custom(..) => {}
    }
}

fn apply_css_wide(
    id: LonghandId,
    keyword: CssWideKeyword,
    parent: &ComputedStyle,
    initial: &ComputedStyle,
    style: &mut ComputedStyle,
) {
    // `revert` reaches this point only from the user-agent origin or from
    // `var()` substitution; there it behaves as `unset`.
    let inherit = match keyword {
        CssWideKeyword::Initial => false,
        CssWideKeyword::Inherit => true,
        CssWideKeyword::Unset | CssWideKeyword::Revert => id.is_inherited(),
    };
    id.copy_value(if inherit { parent } else { initial }, style);
}

/// Chromium's monospace font size quirk: when the font family changes
/// between the single generic `monospace` and anything else, a font size
/// that came from a keyword is recomputed for the new default size (13px
/// for monospace, 16px otherwise), and a size relative to a keyword size
/// is scaled by 13/16 (or 16/13). Absolute sizes do not change. This
/// follows Blink's `FontBuilder::CheckForGenericFamilyChange`.
pub(crate) fn adjust_font_size_for_family(
    style: &mut ComputedStyle,
    parent: &ComputedStyle,
    quirks: bool,
) {
    let monospace = is_monospace(&style.font_family);
    let parent_monospace = is_monospace(&parent.font_family);
    if monospace == parent_monospace {
        return;
    }
    match style.font_size_origin {
        FontSizeOrigin::Absolute => {}
        FontSizeOrigin::Keyword(k) => style.font_size = k.to_px(monospace, quirks),
        FontSizeOrigin::RelativeToKeyword => {
            style.font_size = if parent_monospace {
                style.font_size / FIXED_FONT_RATIO
            } else {
                style.font_size * FIXED_FONT_RATIO
            };
        }
    }
}

/// Computed-value fixups that depend on several properties and on the
/// parent box.
pub(crate) fn apply_fixups(
    style: &mut ComputedStyle,
    is_root: bool,
    layout_parent: Option<Display>,
) {
    let absolutely_positioned = style.position.is_absolutely_positioned();
    // <https://www.w3.org/TR/CSS2/visuren.html#dis-pos-flo>
    if absolutely_positioned {
        style.float = Float::None;
    }
    if is_root && style.display == Display::Contents {
        style.display = Display::Block;
    }
    let in_flex_or_grid = matches!(
        layout_parent,
        Some(Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid)
    );
    let blockify =
        is_root || absolutely_positioned || style.float != Float::None || in_flex_or_grid;
    if blockify && !matches!(style.display, Display::None | Display::Contents) {
        style.display = style.display.blockify();
    }
    // Border widths compute to zero for `none` and `hidden` styles.
    // <https://www.w3.org/TR/css-backgrounds-3/#border-width> (The outline
    // width does not: CSS UI 4 removed that rule, and Chromium follows it.)
    if style.border_top_style.has_no_width() {
        style.border_top_width = 0.0;
    }
    if style.border_right_style.has_no_width() {
        style.border_right_width = 0.0;
    }
    if style.border_bottom_style.has_no_width() {
        style.border_bottom_width = 0.0;
    }
    if style.border_left_style.has_no_width() {
        style.border_left_width = 0.0;
    }
    // `visible` and `clip` compute to `auto` and `hidden` if the other axis
    // is scrollable. <https://www.w3.org/TR/css-overflow-3/#overflow-properties>
    let scrollable =
        |o: Overflow| matches!(o, Overflow::Hidden | Overflow::Scroll | Overflow::Auto);
    let adjust = |o: Overflow| match o {
        Overflow::Visible => Overflow::Auto,
        Overflow::Clip => Overflow::Hidden,
        other => other,
    };
    if scrollable(style.overflow_x) && !scrollable(style.overflow_y) {
        style.overflow_y = adjust(style.overflow_y);
    } else if scrollable(style.overflow_y) && !scrollable(style.overflow_x) {
        style.overflow_x = adjust(style.overflow_x);
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use swb_css::MediaEnvironment;
    use swb_dom::{Document, parse_html};

    use super::*;
    use crate::content::content_text;
    use crate::values::*;

    fn base() -> Url {
        Url::parse("https://example.com/dir/page.html").expect("valid URL")
    }

    /// Styles `html` (a standards-mode document unless it says otherwise)
    /// with one author stylesheet.
    fn render_with(
        html: &str,
        css: &str,
        env: &MediaEnvironment,
        states: &ElementStates,
    ) -> (Document, StyleMap) {
        let doc = parse_html(html);
        let mut stylist = Stylist::new(doc.quirks_mode);
        stylist.add_author_sheet(&swb_css::parse_stylesheet(css), &base());
        let map = compute_styles(&doc, &stylist, env, states, &base());
        (doc, map)
    }

    fn render(html: &str, css: &str) -> (Document, StyleMap) {
        let html = format!("<!DOCTYPE html>{html}");
        render_with(
            &html,
            css,
            &MediaEnvironment::default(),
            &ElementStates::default(),
        )
    }

    fn node(doc: &Document, id: &str) -> NodeId {
        doc.element_by_id(id)
            .unwrap_or_else(|| panic!("no element #{id}"))
    }

    fn get<'a>(doc: &Document, map: &'a StyleMap, id: &str) -> &'a ComputedStyle {
        map.get(node(doc, id))
            .unwrap_or_else(|| panic!("#{id} has no style"))
    }

    fn rgb(r: u8, g: u8, b: u8) -> Rgba {
        Rgba::rgb(r, g, b)
    }

    const RED: Rgba = Rgba::rgb(255, 0, 0);
    const GREEN: Rgba = Rgba::rgb(0, 128, 0);
    const BLUE: Rgba = Rgba::rgb(0, 0, 255);

    #[test]
    fn specificity_order_and_importance() {
        let (doc, map) = render(
            "<p id=a class=c>x</p><p id=b class=c style='color: blue'>y</p>\
             <p id=d class=c style='color: blue !important'>z</p><p id=e>w</p>",
            "#a { color: red } .c { color: blue } p { color: green }\
             .c { color: green } #b { color: red }\
             #d { color: red !important }\
             p { color: green !important } #e { color: red }",
        );
        // An ID beats classes and types; `!important` beats everything
        // normal; `style` beats rules; `style !important` beats rule
        // `!important`.
        assert_eq!(get(&doc, &map, "a").color, GREEN);
        assert_eq!(get(&doc, &map, "b").color, GREEN);
        assert_eq!(get(&doc, &map, "d").color, BLUE);
        assert_eq!(get(&doc, &map, "e").color, GREEN);
        let (doc, map) = render(
            "<p id=a class='x y'>x</p>",
            ".x { color: red } .y { color: blue }",
        );
        assert_eq!(
            get(&doc, &map, "a").color,
            BLUE,
            "later rule wins at equal specificity"
        );
        let (doc, map) = render("<p id=a>x</p>", "p { color: red; color: blue }");
        assert_eq!(get(&doc, &map, "a").color, BLUE);
    }

    #[test]
    fn origins() {
        // Author rules beat the UA sheet regardless of specificity, and
        // UA !important beats author !important.
        let (doc, map) = render(
            "<a id=l href=x>link</a><input id=h type=hidden>",
            "* { color: green; text-decoration: none } input { display: block !important }",
        );
        let link = get(&doc, &map, "l");
        assert_eq!(link.color, GREEN);
        assert!(link.text_decoration_line.is_empty());
        assert!(
            map.get(node(&doc, "h"))
                .is_some_and(|s| s.display == Display::None)
        );
        // Presentational hints lose to any author rule.
        let (doc, map) = render(
            "<table id=t bgcolor=red width=50 align=right><tr><td id=c bgcolor=blue>x</td></tr></table>",
            "* { background-color: green } table { float: none }",
        );
        let t = get(&doc, &map, "t");
        assert_eq!(t.background_color, Color::Rgba(GREEN));
        assert_eq!(t.width, Size::LengthPercentage(LengthPercentage::Px(50.0)));
        assert_eq!(t.float, Float::None);
        assert_eq!(get(&doc, &map, "c").background_color, Color::Rgba(GREEN));
        // ...but beat the UA sheet.
        let (doc, map) = render("<p id=p align=center>x</p>", "");
        assert_eq!(get(&doc, &map, "p").text_align, TextAlign::WebkitCenter);
    }

    #[test]
    fn inheritance_and_css_wide_keywords() {
        let (doc, map) = render(
            "<div id=p><span id=c>x</span><span id=i>y</span><span id=u>z</span><b id=n>q</b></div>",
            "#p { color: red; border: 2px solid; padding: 3px; font-size: 20px }\
             #i { border-top-width: inherit; border-top-style: solid; padding-left: inherit;\
             color: initial }\
             #u { color: unset; padding-top: unset; font-size: 50% }\
             #n { font-size: inherit; font-weight: inherit }",
        );
        let c = get(&doc, &map, "c");
        assert_eq!(c.color, RED);
        assert_eq!(c.border_top_width, 0.0);
        assert_eq!(c.padding_left, LengthPercentage::ZERO);
        let i = get(&doc, &map, "i");
        assert_eq!(i.border_top_width, 2.0);
        assert_eq!(i.padding_left, LengthPercentage::Px(3.0));
        assert_eq!(i.color, Rgba::BLACK);
        let u = get(&doc, &map, "u");
        assert_eq!(u.color, RED);
        assert_eq!(u.padding_top, LengthPercentage::ZERO);
        assert_eq!(u.font_size, 10.0);
        let n = get(&doc, &map, "n");
        assert_eq!(n.font_size, 20.0);
        assert_eq!(n.font_weight, 400.0, "inherit beats the UA `b` rule");
    }

    #[test]
    fn relative_units() {
        let env = MediaEnvironment {
            viewport_width: 1000.0,
            viewport_height: 500.0,
            ..MediaEnvironment::default()
        };
        let (doc, map) = render_with(
            "<!DOCTYPE html><html id=h><div id=d><p id=p>x</p></div></html>",
            "html { font-size: 20px } #d { font-size: 2em; width: 10vw; height: 10vh }\
             #p { font-size: 1.5rem; margin-left: 2em; padding-left: 1rem; width: 50vmin; line-height: 1.5em }",
            &env,
            &ElementStates::default(),
        );
        let d = get(&doc, &map, "d");
        assert_eq!(d.font_size, 40.0);
        assert_eq!(d.width, Size::LengthPercentage(LengthPercentage::Px(100.0)));
        assert_eq!(d.height, Size::LengthPercentage(LengthPercentage::Px(50.0)));
        let p = get(&doc, &map, "p");
        assert_eq!(p.font_size, 30.0);
        assert_eq!(p.margin_left, LengthPercentageOrAuto::px(60.0));
        assert_eq!(p.padding_left, LengthPercentage::Px(20.0));
        assert_eq!(p.width, Size::LengthPercentage(LengthPercentage::Px(250.0)));
        assert_eq!(p.line_height, LineHeight::Px(45.0));
        // `rem` on the root refers to the initial font size.
        let (doc, map) = render(
            "<html id=h></html>",
            "html { font-size: 2rem; width: 1rem }",
        );
        let h = get(&doc, &map, "h");
        assert_eq!(h.font_size, 32.0);
        assert_eq!(h.width, Size::LengthPercentage(LengthPercentage::Px(32.0)));
    }

    #[test]
    fn display_fixups() {
        let (doc, map) = render(
            "<html id=root><span id=f>a</span><span id=a>b</span>\
             <div id=flex><span id=item>c</span><span id=cont><em id=deep>d</em></span></div>\
             <div id=grid><span id=g>e</span></div><span id=none><b id=hidden>x</b></span></html>",
            "html { display: inline } #f { float: left } #a { position: absolute; float: right }\
             #flex { display: flex } #cont { display: contents } #grid { display: inline-grid }\
             #none { display: none } #item { display: table-cell }",
        );
        assert_eq!(get(&doc, &map, "root").display, Display::Block);
        assert_eq!(get(&doc, &map, "f").display, Display::Block);
        let a = get(&doc, &map, "a");
        assert_eq!(a.display, Display::Block);
        assert_eq!(a.float, Float::None);
        assert_eq!(get(&doc, &map, "item").display, Display::Block);
        assert_eq!(get(&doc, &map, "cont").display, Display::Contents);
        assert_eq!(
            get(&doc, &map, "deep").display,
            Display::Block,
            "flex item through contents"
        );
        assert_eq!(get(&doc, &map, "g").display, Display::Block);
        assert!(map.get(node(&doc, "hidden")).is_none());
        // Border widths are zero without a border style; the overflow axes
        // pair up.
        let (doc, map) = render(
            "<div id=d>x</div>",
            "#d { border-width: 5px; border-left-style: solid; outline-width: 4px; overflow-x: hidden }",
        );
        let d = get(&doc, &map, "d");
        assert_eq!(d.border_widths(), [0.0, 0.0, 0.0, 5.0]);
        assert_eq!(d.outline_width, 4.0);
        assert_eq!(d.overflow_y, Overflow::Auto);
        let (doc, map) = render(
            "<div id=n><span id=c>x</span><b id=t>y</b></div>",
            "#n { user-select: none } #t { user-select: text }",
        );
        assert_eq!(get(&doc, &map, "c").user_select, UserSelect::None);
        assert_eq!(get(&doc, &map, "t").user_select, UserSelect::Text);
    }

    #[test]
    fn custom_properties_and_var() {
        let (doc, map) = render(
            "<div id=p><p id=c>x</p><p id=bad>y</p><p id=sh>z</p><p id=cyc>w</p></div>",
            ":root { --main: rgb(255 0 0); --gap: 4px; --w: 2px }\
             #p { --main: blue; color: var(--main); padding: var(--gap) calc(var(--gap) * 2) }\
             #c { background-color: var(--missing, var(--main)); margin: var(--gap, 1px) }\
             #bad { color: var(--missing); border-top-width: var(--nope); border-top-style: solid; width: var(--main) }\
             #sh { border: var(--w) solid var(--main); --main: green; font: var(--font, bold 10px serif) }\
             #cyc { --a: var(--b); --b: var(--a); color: var(--a, red); --x: env(nope, 7px); margin-top: var(--x) }",
        );
        let p = get(&doc, &map, "p");
        assert_eq!(p.color, BLUE);
        assert_eq!(p.padding_top, LengthPercentage::Px(4.0));
        assert_eq!(p.padding_right, LengthPercentage::Px(8.0));
        let c = get(&doc, &map, "c");
        assert_eq!(c.background_color, Color::Rgba(BLUE));
        assert_eq!(c.margin_top, LengthPercentageOrAuto::px(4.0));
        // Invalid at computed-value time: `unset` (inherit for `color`,
        // initial for the others).
        let bad = get(&doc, &map, "bad");
        assert_eq!(bad.color, BLUE);
        assert_eq!(bad.border_top_width, 3.0);
        assert_eq!(bad.width, Size::Auto);
        let sh = get(&doc, &map, "sh");
        assert_eq!(sh.border_left_width, 2.0);
        assert_eq!(sh.border_left_style, BorderStyle::Solid);
        assert_eq!(sh.border_left_color, Color::Rgba(GREEN));
        assert_eq!(sh.font_weight, 700.0);
        assert_eq!(sh.font_size, 10.0);
        let cyc = get(&doc, &map, "cyc");
        assert_eq!(cyc.color, RED);
        assert_eq!(cyc.margin_top, LengthPercentageOrAuto::px(7.0));
        let custom = cyc.custom_properties.as_ref().expect("custom properties");
        assert!(!custom.contains_key("--a"));
        assert!(custom.contains_key("--gap"));
        // Children share the parent's map when they declare nothing.
        let parent = map.get(node(&doc, "p")).expect("style");
        let child = map.get(node(&doc, "c")).expect("style");
        assert!(Arc::ptr_eq(
            parent.custom_properties.as_ref().expect("map"),
            child.custom_properties.as_ref().expect("map")
        ));
    }

    #[test]
    fn media_and_supports() {
        let css = "p { color: red } @media (min-width: 600px) { p { color: green } }\
                   @media print { p { color: blue } }\
                   @supports (display: flex) { p { background-color: green } }\
                   @supports (display: nonsense) { p { background-color: red } }";
        let html = "<!DOCTYPE html><p id=p>x</p>";
        let wide = MediaEnvironment::default();
        let narrow = MediaEnvironment {
            viewport_width: 400.0,
            ..MediaEnvironment::default()
        };
        let (doc, map) = render_with(html, css, &wide, &ElementStates::default());
        assert_eq!(get(&doc, &map, "p").color, GREEN);
        assert_eq!(get(&doc, &map, "p").background_color, Color::Rgba(GREEN));
        let (doc, map) = render_with(html, css, &narrow, &ElementStates::default());
        assert_eq!(get(&doc, &map, "p").color, RED);
    }

    #[test]
    fn pseudo_elements() {
        let (doc, map) = render(
            "<p id=a title=T>x</p><p id=b>y</p><p id=n>z</p><ul><li id=li>i</li></ul><q id=q>q</q>\
             <div id=flex><span id=fi>s</span></div><img id=img src=x.png>",
            "#a::before { content: '[' attr(title) ']'; color: red } #a::after { content: 'end' }\
             #b::before { color: red } #n::before { content: 'x'; display: none }\
             li::marker { color: blue } #flex { display: flex } #flex::before { content: 'f' }\
             img::before { content: 'no' }",
        );
        let a = node(&doc, "a");
        let before = map.pseudo(a, PseudoKind::Before).expect("::before");
        assert_eq!(content_text(before).as_deref(), Some("[T]"));
        assert_eq!(before.color, RED);
        assert_eq!(before.display, Display::Inline);
        assert!(map.pseudo(a, PseudoKind::After).is_some());
        assert!(map.pseudo(node(&doc, "b"), PseudoKind::Before).is_none());
        assert!(map.pseudo(node(&doc, "n"), PseudoKind::Before).is_none());
        let marker = map
            .pseudo(node(&doc, "li"), PseudoKind::Marker)
            .expect("::marker");
        assert_eq!(marker.color, BLUE);
        assert_eq!(marker.white_space, WhiteSpace::Pre);
        assert_eq!(content_text(marker), None);
        let q = node(&doc, "q");
        let open = map.pseudo(q, PseudoKind::Before).expect("q::before");
        assert_eq!(content_text(open).as_deref(), Some("\u{201C}"));
        let flex_before = map
            .pseudo(node(&doc, "flex"), PseudoKind::Before)
            .expect("::before");
        assert_eq!(flex_before.display, Display::Block, "blockified flex item");
        assert!(map.pseudo(node(&doc, "img"), PseudoKind::Before).is_none());
    }

    #[test]
    fn placeholder_styles() {
        let (doc, map) = render(
            "<input id=a placeholder=x><textarea id=b placeholder=y></textarea><input id=c>",
            "#b::placeholder { color: red }",
        );
        let a = map
            .pseudo(node(&doc, "a"), PseudoKind::Placeholder)
            .expect("input::placeholder");
        assert_eq!(a.color, Rgba::rgb(0x75, 0x75, 0x75));
        assert_eq!(
            a.font_size,
            map.get(node(&doc, "a")).expect("style").font_size
        );
        let b = map
            .pseudo(node(&doc, "b"), PseudoKind::Placeholder)
            .expect("textarea::placeholder");
        assert_eq!(b.color, RED);
        assert!(
            map.pseudo(node(&doc, "c"), PseudoKind::Placeholder)
                .is_none()
        );
    }

    #[test]
    fn user_agent_defaults() {
        let (doc, map) = render(
            "<body id=body><h1 id=h1>t</h1><p id=p>x</p><ul id=ul><li><ul id=ul2></ul></li></ul>\
             <ol id=ol></ol><table id=t><tr id=tr><th id=th>h</th><td id=td>d</td></tr></table>\
             <pre id=pre>x</pre><a id=a href=x>l</a><b id=b>b</b><small id=small>s</small>\
             <center id=center>c</center><input id=input><button id=button>b</button>\
             <details id=det><summary id=sum>s</summary><p id=hidden>h</p></details>\
             <hr id=hr><sub id=sub>s</sub><script id=script></script><noscript id=ns>n</noscript></body>",
            "",
        );
        assert_eq!(
            get(&doc, &map, "body").margin_top,
            LengthPercentageOrAuto::px(8.0)
        );
        let h1 = get(&doc, &map, "h1");
        assert_eq!((h1.font_size, h1.font_weight), (32.0, 700.0));
        assert_eq!(h1.margin_top, LengthPercentageOrAuto::px(32.0 * 0.67));
        assert_eq!(
            get(&doc, &map, "p").margin_bottom,
            LengthPercentageOrAuto::px(16.0)
        );
        let ul = get(&doc, &map, "ul");
        assert_eq!(ul.list_style_type, ListStyleType::Disc);
        assert_eq!(ul.padding_left, LengthPercentage::Px(40.0));
        let ul2 = get(&doc, &map, "ul2");
        assert_eq!(ul2.list_style_type, ListStyleType::Circle);
        assert_eq!(ul2.margin_top, LengthPercentageOrAuto::px(0.0));
        assert_eq!(
            get(&doc, &map, "ol").list_style_type,
            ListStyleType::Decimal
        );
        let t = get(&doc, &map, "t");
        assert_eq!(t.display, Display::Table);
        assert_eq!(t.border_spacing_horizontal, 2.0);
        assert_eq!(t.box_sizing, BoxSizing::BorderBox);
        assert_eq!(
            get(&doc, &map, "tr").vertical_align,
            VerticalAlign::Keyword(VerticalAlignKeyword::Middle)
        );
        let th = get(&doc, &map, "th");
        assert_eq!(th.text_align, TextAlign::Center);
        assert_eq!(th.font_weight, 700.0);
        assert_eq!(
            th.vertical_align,
            VerticalAlign::Keyword(VerticalAlignKeyword::Middle)
        );
        assert_eq!(get(&doc, &map, "td").padding_top, LengthPercentage::Px(1.0));
        let pre = get(&doc, &map, "pre");
        assert_eq!(pre.white_space, WhiteSpace::Pre);
        assert_eq!(pre.font_size, 13.0);
        let a = get(&doc, &map, "a");
        assert_eq!(a.color, rgb(0, 0, 0xEE));
        assert_eq!(a.text_decoration_line, TextDecorationLine::UNDERLINE);
        assert_eq!(a.cursor, Cursor::Pointer);
        assert_eq!(get(&doc, &map, "b").font_weight, 700.0);
        assert!((get(&doc, &map, "small").font_size - 16.0 / 1.2).abs() < 1e-4);
        assert_eq!(
            get(&doc, &map, "center").text_align,
            TextAlign::WebkitCenter
        );
        let input = get(&doc, &map, "input");
        assert_eq!(input.display, Display::InlineBlock);
        assert_eq!(input.border_top_width, 2.0);
        assert_eq!(input.border_top_style, BorderStyle::Inset);
        assert!((input.font_size - 13.333_333).abs() < 1e-4);
        assert_eq!(
            get(&doc, &map, "button").border_top_style,
            BorderStyle::Outset
        );
        assert_eq!(get(&doc, &map, "sum").display, Display::ListItem);
        assert_eq!(
            get(&doc, &map, "sum").list_style_type,
            ListStyleType::DisclosureClosed
        );
        assert_eq!(get(&doc, &map, "hidden").display, Display::None);
        let hr = get(&doc, &map, "hr");
        assert_eq!(hr.border_widths(), [1.0; 4]);
        assert_eq!(hr.margin_left, LengthPercentageOrAuto::Auto);
        assert_eq!(
            get(&doc, &map, "sub").vertical_align,
            VerticalAlign::Keyword(VerticalAlignKeyword::Sub)
        );
        assert_eq!(get(&doc, &map, "script").display, Display::None);
        assert_eq!(get(&doc, &map, "ns").display, Display::Inline);
    }

    #[test]
    fn presentational_hints() {
        let (doc, map) = render(
            "<body id=body text=red bgcolor=#ffffee link=green>\
             <a id=a href=x>l</a>\
             <table id=t border=2 cellpadding=5 cellspacing=3 width=80% height=20>\
             <tbody><tr id=tr valign=top align=center height=30><td id=td nowrap width=10 background=bg.png>x</td>\
             <th id=th align=left bgcolor=blue>y</th></tr></tbody></table>\
             <table id=t0 border=0><tr><td id=td0>z</td></tr></table>\
             <font id=f color=blue face='Courier New, monospace' size=+2>f</font>\
             <font id=f1 size=1>g</font>\
             <img id=img width=50 height=40% hspace=4 vspace=2 border=1 align=left>\
             <hr id=hr size=1 width=50%><hr id=hr2 noshade size=4>\
             <br id=br clear=all><ol id=ol type=a></ol><ul id=ul type=square></ul>\
             <div id=div align=right>d</div><pre id=pre wrap>p</pre>\
             <iframe id=ifr frameborder=0></iframe></body>",
            "",
        );
        let body = get(&doc, &map, "body");
        assert_eq!(body.color, RED);
        assert_eq!(body.background_color, Color::Rgba(rgb(0xff, 0xff, 0xee)));
        assert_eq!(get(&doc, &map, "a").color, GREEN);
        let t = get(&doc, &map, "t");
        assert_eq!(t.border_widths(), [2.0; 4]);
        assert_eq!(t.border_top_style, BorderStyle::Outset);
        assert_eq!(t.border_spacing_vertical, 3.0);
        assert_eq!(
            t.width,
            Size::LengthPercentage(LengthPercentage::Percent(0.8))
        );
        assert_eq!(t.height, Size::LengthPercentage(LengthPercentage::Px(20.0)));
        let tr = get(&doc, &map, "tr");
        assert_eq!(
            tr.vertical_align,
            VerticalAlign::Keyword(VerticalAlignKeyword::Top)
        );
        assert_eq!(tr.text_align, TextAlign::WebkitCenter);
        assert_eq!(
            tr.height,
            Size::LengthPercentage(LengthPercentage::Px(30.0))
        );
        let td = get(&doc, &map, "td");
        assert_eq!(td.padding_left, LengthPercentage::Px(5.0));
        assert_eq!(td.border_widths(), [1.0; 4]);
        assert_eq!(td.border_top_style, BorderStyle::Inset);
        assert_eq!(td.white_space, WhiteSpace::Nowrap);
        assert_eq!(td.width, Size::LengthPercentage(LengthPercentage::Px(10.0)));
        assert_eq!(
            td.vertical_align,
            VerticalAlign::Keyword(VerticalAlignKeyword::Top)
        );
        assert_eq!(
            td.text_align,
            TextAlign::WebkitCenter,
            "inherited from the row"
        );
        assert_eq!(
            &*td.background_image,
            &[Some(Image::Url("https://example.com/dir/bg.png".into()))]
        );
        let th = get(&doc, &map, "th");
        assert_eq!(th.text_align, TextAlign::WebkitLeft);
        assert_eq!(th.background_color, Color::Rgba(BLUE));
        let td0 = get(&doc, &map, "td0");
        assert_eq!(td0.border_widths(), [0.0; 4]);
        assert_eq!(get(&doc, &map, "t0").border_widths(), [0.0; 4]);
        let f = get(&doc, &map, "f");
        assert_eq!(f.color, BLUE);
        assert_eq!(f.font_family[0], FontFamily::Named("Courier New".into()));
        assert_eq!(f.font_size, 24.0);
        assert_eq!(get(&doc, &map, "f1").font_size, 10.0);
        let img = get(&doc, &map, "img");
        assert_eq!(
            img.width,
            Size::LengthPercentage(LengthPercentage::Px(50.0))
        );
        assert_eq!(
            img.height,
            Size::LengthPercentage(LengthPercentage::Percent(0.4))
        );
        assert_eq!(img.margin_left, LengthPercentageOrAuto::px(4.0));
        assert_eq!(img.margin_top, LengthPercentageOrAuto::px(2.0));
        assert_eq!(img.border_top_style, BorderStyle::Solid);
        assert_eq!(img.float, Float::Left);
        let hr = get(&doc, &map, "hr");
        assert_eq!(hr.border_bottom_width, 0.0);
        assert_eq!(
            hr.width,
            Size::LengthPercentage(LengthPercentage::Percent(0.5))
        );
        let hr2 = get(&doc, &map, "hr2");
        assert_eq!(hr2.border_top_style, BorderStyle::Solid);
        assert_eq!(hr2.border_top_width, 2.0);
        assert_eq!(get(&doc, &map, "br").clear, Clear::Both);
        assert_eq!(
            get(&doc, &map, "ol").list_style_type,
            ListStyleType::LowerAlpha
        );
        assert_eq!(get(&doc, &map, "ul").list_style_type, ListStyleType::Square);
        assert_eq!(get(&doc, &map, "div").text_align, TextAlign::WebkitRight);
        assert_eq!(get(&doc, &map, "pre").white_space, WhiteSpace::PreWrap);
        assert_eq!(get(&doc, &map, "ifr").border_widths(), [0.0; 4]);
    }

    #[test]
    fn tables_reset_webkit_text_align() {
        let (doc, map) = render(
            "<!DOCTYPE html><center><table id=t><tr><td id=td>x</td></tr></table>\
             <div id=d style='display:table'>y</div></center>",
            "",
        );
        assert_eq!(get(&doc, &map, "t").text_align, TextAlign::Start);
        assert_eq!(get(&doc, &map, "td").text_align, TextAlign::Start);
        // Only table elements, as in Chromium.
        assert_eq!(get(&doc, &map, "d").text_align, TextAlign::WebkitCenter);
    }

    #[test]
    fn quirks_mode() {
        let quirks_html = "<html><body style='font-size: 20px; color: red; text-align: center'>\
             <table id=t><tr><td id=td>x</td></tr></table>\
             <div id=d class=Foo>y</div><ul><li id=li>z</li></ul><div><li id=li2>w</li></div>\
             </body></html>";
        let css = "#d { width: 100; height: 50px } .foo { color: green } #td { padding: 3 }";
        let (doc, map) = render_with(
            quirks_html,
            css,
            &MediaEnvironment::default(),
            &ElementStates::default(),
        );
        assert_eq!(doc.quirks_mode, swb_dom::QuirksMode::Quirks);
        let t = get(&doc, &map, "t");
        assert_eq!(t.font_size, 16.0);
        assert_eq!(t.color, RED, "Chromium: tables inherit the color");
        assert_eq!(t.text_align, TextAlign::Start);
        let d = get(&doc, &map, "d");
        assert_eq!(d.width, Size::LengthPercentage(LengthPercentage::Px(100.0)));
        assert_eq!(d.color, GREEN, "class selectors match case-insensitively");
        assert_eq!(get(&doc, &map, "td").padding_top, LengthPercentage::Px(3.0));
        // `li` is `inside`, except in lists (Chromium's quirks.css).
        assert_eq!(
            get(&doc, &map, "li").list_style_position,
            ListStylePosition::Outside
        );
        assert_eq!(
            get(&doc, &map, "li2").list_style_position,
            ListStylePosition::Inside
        );
        // The same document in standards mode.
        let html = format!("<!DOCTYPE html>{quirks_html}");
        let (doc, map) = render_with(
            &html,
            css,
            &MediaEnvironment::default(),
            &ElementStates::default(),
        );
        assert_eq!(get(&doc, &map, "t").font_size, 20.0);
        let d = get(&doc, &map, "d");
        assert_eq!(d.width, Size::Auto);
        assert_eq!(d.color, RED);
        assert_eq!(
            get(&doc, &map, "li2").list_style_position,
            ListStylePosition::Outside
        );
    }

    #[test]
    fn monospace_font_size_quirk() {
        let (doc, map) = render(
            "<div id=body><pre id=pre>p<code id=nested>c</code><span id=sans>s</span></pre>\
             <code id=code>c</code><code id=fixed>f</code><pre id=pct>p</pre>\
             <code id=two>t</code><code id=small_c><small id=small>s</small></code>\
             <span id=big style='font-size: x-large'><code id=xl>x</code></span>\
             <span id=px style='font-size: 20px'><code id=px_code>y</code></span></div>",
            "#fixed { font-size: 12px } #pct { font-size: 120% } #two { font-family: monospace, monospace }\
             #sans { font-family: sans-serif }",
        );
        let size = |id: &str| get(&doc, &map, id).font_size;
        assert_eq!(size("body"), 16.0);
        assert_eq!(size("pre"), 13.0);
        assert_eq!(size("nested"), 13.0);
        assert_eq!(size("sans"), 16.0);
        assert_eq!(size("code"), 13.0);
        assert_eq!(size("fixed"), 12.0);
        assert!((size("pct") - 15.6).abs() < 1e-4, "{}", size("pct"));
        assert_eq!(size("two"), 16.0);
        assert!((size("small") - 13.0 / 1.2).abs() < 1e-4);
        assert_eq!(size("xl"), 20.0, "the monospace x-large size");
        assert_eq!(size("px_code"), 20.0, "absolute sizes do not change");
    }

    #[test]
    fn dynamic_states() {
        let html = "<!DOCTYPE html><div id=outer><a id=link href=x>l</a></div><p id=other>o</p>";
        let css = "div:hover { color: red } a:hover { color: green } :focus { outline: 1px solid } p:hover { color: blue }";
        let doc = parse_html(html);
        let states = ElementStates {
            hover: doc.element_by_id("link"),
            focus: doc.element_by_id("link"),
            ..ElementStates::default()
        };
        let (doc, map) = render_with(html, css, &MediaEnvironment::default(), &states);
        assert_eq!(get(&doc, &map, "outer").color, RED);
        assert_eq!(get(&doc, &map, "link").color, GREEN);
        assert_eq!(get(&doc, &map, "link").outline_width, 1.0);
        assert_eq!(get(&doc, &map, "other").color, Rgba::BLACK);
    }

    #[test]
    fn image_urls_are_absolute_and_unique() {
        let (_, map) = render(
            "<div class=a></div><div class=a></div><ul class=b><li>x</li></ul><p class=c></p>",
            ".a { background: url(img/a.png) } .b { list-style-image: url('/b.png') }\
             .c { background-image: url(img/a.png), linear-gradient(red, blue) }",
        );
        let urls: Vec<String> = map.image_urls().iter().map(ToString::to_string).collect();
        assert_eq!(
            urls,
            vec![
                "https://example.com/dir/img/a.png".to_owned(),
                "https://example.com/b.png".to_owned()
            ]
        );
    }

    #[test]
    fn revert_rolls_back_to_the_user_agent_origin() {
        let (doc, map) = render(
            "<div id=d>x</div><p id=p>y</p><h1 id=h style='font-size: revert'>z</h1>\
             <span id=s style='color: revert'>s</span><a id=a href=x>l</a>",
            "* { all: unset } div { display: revert } p { all: revert }\
             h1 { font-size: 10px !important } span { color: red } #s { color: blue }\
             a { color: revert-layer; text-decoration: revert }",
        );
        assert_eq!(get(&doc, &map, "d").display, Display::Block);
        let p = get(&doc, &map, "p");
        assert_eq!(p.display, Display::Block);
        assert_eq!(p.margin_top, LengthPercentageOrAuto::px(16.0));
        // An `!important` author declaration still wins over a normal
        // `revert` in the style attribute.
        assert_eq!(get(&doc, &map, "h").font_size, 10.0);
        // `revert` in the style attribute skips all author rules: the
        // span has no UA color, so it inherits.
        assert_eq!(get(&doc, &map, "s").color, Rgba::BLACK);
        let a = get(&doc, &map, "a");
        assert_eq!(a.color, rgb(0, 0, 0xEE));
        assert_eq!(a.text_decoration_line, TextDecorationLine::UNDERLINE);
    }

    #[test]
    fn replaced_elements_and_review_fixes() {
        let (doc, map) = render(
            "<img id=img src=x style='display: contents'><div id=div style='display: contents'></div>\
             <canvas id=canvas width=300 height=150></canvas><img id=mid align=middle>\
             <table id=t border=1><tr><td>x</td></tr></table><p id=q>q</p>",
            "#q { --x: 1; -swb-border-spacing-horizontal: 9px; content: no-open-quote }",
        );
        assert_eq!(get(&doc, &map, "img").display, Display::None);
        assert_eq!(get(&doc, &map, "div").display, Display::Contents);
        assert_eq!(get(&doc, &map, "canvas").width, Size::Auto);
        assert_eq!(
            get(&doc, &map, "mid").vertical_align,
            VerticalAlign::Keyword(VerticalAlignKeyword::Middle)
        );
        assert_eq!(get(&doc, &map, "t").border_top_color, Color::CurrentColor);
        let q = get(&doc, &map, "q");
        assert_eq!(
            q.border_spacing_horizontal, 0.0,
            "internal names are not valid"
        );
        assert_eq!(q.content, Content::Items(Arc::from([])));
    }

    #[test]
    fn deep_nesting_through_custom_properties() {
        // Each element wraps the value of its parent in 60 more levels of
        // parentheses. The values must stop growing (and not overflow the
        // stack in recursive code).
        let html = "<div class=e><div class=o>".repeat(1500);
        let css = format!(
            ":root {{ --b: x }} .e {{ --a: {0}var(--b){1} }} .o {{ --b: {0}var(--a){1} }}\
             div {{ width: var(--a, 1px) }}",
            "(".repeat(60),
            ")".repeat(60)
        );
        let (doc, map) = render(&html, &css);
        let styled = doc
            .descendants(NodeId::DOCUMENT)
            .filter(|&n| map.get(n).is_some())
            .count();
        assert!(styled > 3000);
    }

    #[test]
    fn many_custom_properties() {
        let mut css = String::from(":root {");
        for i in 0..20_000 {
            let _ = write!(css, "--v{i}: {i}px;");
        }
        css.push_str("} p { width: var(--v19999) }");
        let (doc, map) = render("<p id=p>x</p>", &css);
        assert_eq!(
            get(&doc, &map, "p").width,
            Size::LengthPercentage(LengthPercentage::Px(19999.0))
        );
    }

    #[test]
    fn styles_are_shared_when_inputs_match() {
        let (doc, map) = render(
            "<p><a id=a href=x>1</a><a id=b href=y>2</a><a id=c href=z style='color: red'>3</a>\
             <span id=d title=x>4</span><span id=e title=y>5</span></p>",
            "span::before { content: attr(title) } p span { content: attr(title) }",
        );
        let style = |id: &str| map.get(node(&doc, id)).expect("style");
        assert!(Arc::ptr_eq(style("a"), style("b")));
        assert!(!Arc::ptr_eq(style("a"), style("c")));
        // Rules with `content` (which can use `attr()`) prevent sharing.
        assert!(!Arc::ptr_eq(style("d"), style("e")));
        let before = |id: &str| {
            map.pseudo(node(&doc, id), PseudoKind::Before)
                .expect("::before")
        };
        assert_eq!(content_text(before("d")).as_deref(), Some("x"));
        assert_eq!(content_text(before("e")).as_deref(), Some("y"));
    }

    /// Loads a fixture page with its stylesheets (by file name).
    fn fixture(name: &str, html: &str, sheets: &[&str]) -> (Document, Stylist) {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/pages")
            .join(name)
            .join("files");
        let bytes = std::fs::read(dir.join(html)).expect("fixture HTML");
        let (doc, _) = swb_dom::parse_html_bytes(&bytes, Some("utf-8"));
        let mut stylist = Stylist::new(doc.quirks_mode);
        for sheet in sheets {
            let css = std::fs::read_to_string(dir.join(sheet)).expect("fixture CSS");
            stylist.add_author_sheet(&swb_css::parse_stylesheet(&css), &base());
        }
        for node in doc.descendants(NodeId::DOCUMENT) {
            if doc.is_html_element(node, &swb_dom::local_name!("style")) {
                let css = doc.text_content(node);
                stylist.add_author_sheet(&swb_css::parse_stylesheet(&css), &base());
            }
        }
        (doc, stylist)
    }

    #[test]
    fn bloom_filter_does_not_change_results() {
        let pages: [(&str, &str, &[&str]); 2] = [
            (
                "wikipedia-web-browser",
                "e21b21467da0d04c.html",
                &["54eaa11ea56e20f7.css", "3f439934c51c220c.css"],
            ),
            (
                "hacker-news",
                "8e8eec10c3f22bf2.html",
                &["de4722818cb6e785.css"],
            ),
        ];
        for (name, html, sheets) in pages {
            let (doc, stylist) = fixture(name, html, sheets);
            let env = MediaEnvironment::default();
            let states = ElementStates::default();
            let with = compute_styles_with_options(&doc, &stylist, &env, &states, &base(), true);
            let without =
                compute_styles_with_options(&doc, &stylist, &env, &states, &base(), false);
            let mut compared = 0;
            for node in doc.descendants(NodeId::DOCUMENT) {
                assert_eq!(with.get(node), without.get(node), "{name}: {node:?}");
                for kind in [PseudoKind::Before, PseudoKind::After, PseudoKind::Marker] {
                    assert_eq!(with.pseudo(node, kind), without.pseudo(node, kind));
                }
                compared += usize::from(with.get(node).is_some());
            }
            assert!(compared > 100, "{name}: {compared}");
        }
    }

    #[test]
    fn hostile_input_does_not_panic() {
        // Deep nesting is handled iteratively.
        let deep = "<div>".repeat(5000);
        let (doc, map) = render(&deep, "div div { margin: 1px } div { --x: var(--x) }");
        let deepest = doc
            .descendants(NodeId::DOCUMENT)
            .filter(|&n| doc.element(n).is_some())
            .last()
            .expect("an element");
        assert!(map.get(deepest).is_some());
        let mut css = String::from(":root { --v0: x x x x x x x x }");
        for i in 1..30 {
            let _ = write!(css, ":root {{ --v{i}: var(--v{0}) var(--v{0}) }}", i - 1);
        }
        css.push_str("p { width: var(--v29) }");
        let (doc, map) = render("<p id=p>x</p>", &css);
        assert_eq!(get(&doc, &map, "p").width, Size::Auto);
    }
}
