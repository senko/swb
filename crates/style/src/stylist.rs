//! The stylist: all style rules of a document (user-agent, presentational
//! hints and author), indexed for fast lookup.
//!
//! Each selector of a style rule becomes one [`Rule`]. Rules are put in
//! buckets by the key of their rightmost compound selector
//! ([`swb_css::Selector::bucket_key`]): ID, class, local name, or
//! universal. To find the rules that can match an element, the cascade
//! looks up the element's ID, classes and local name, plus the universal
//! bucket. Rules for `::before`, `::after`, `::marker` and `::placeholder`
//! have their own maps; rules for other pseudo-elements are dropped.
//!
//! `@media` conditions are evaluated when styles are computed (the
//! environment can change); `@supports` conditions are evaluated when a
//! sheet is added. `@import` is ignored here. `@font-face` rules are kept
//! with their `@media` conditions ([`Stylist::font_faces`]).

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use log::trace;
use swb_css::{
    BucketKey, CssRule, ElementState, MatchingContext, MediaEnvironment, MediaQueryList,
    PseudoElement, Selector, StyleRule, Stylesheet, matches, parse_stylesheet,
};
use url::Url;

use smallvec::SmallVec;

use crate::bloom::{AncestorFilter, MAX_ANCESTOR_HASHES, ancestor_hashes};
use crate::element::DomElement;
use crate::font_face::FontFace;
use crate::parse::ParserContext;
use crate::properties::{DeclarationBlock, is_supported};
use crate::style_map::PseudoKind;

/// The user-agent stylesheet.
static UA_SHEET: LazyLock<Stylesheet> = LazyLock::new(|| parse_stylesheet(include_str!("ua.css")));

/// User-agent rules that apply only in quirks mode.
static UA_QUIRKS_SHEET: LazyLock<Stylesheet> =
    LazyLock::new(|| parse_stylesheet(include_str!("ua-quirks.css")));

/// Presentational hints that the HTML specification gives as CSS rules.
static HINTS_SHEET: LazyLock<Stylesheet> =
    LazyLock::new(|| parse_stylesheet(include_str!("hints.css")));

/// The cascade origin of a rule, in increasing precedence for normal
/// declarations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum CascadeOrigin {
    /// The user-agent stylesheet.
    UserAgent = 0,
    /// Presentational hints (author level, specificity zero).
    PresentationalHint = 1,
    /// Author stylesheets.
    Author = 2,
}

/// One selector of a style rule and the rule's declarations.
#[derive(Debug)]
pub(crate) struct Rule {
    pub(crate) selector: Selector,
    pub(crate) block: Arc<DeclarationBlock>,
    pub(crate) origin: CascadeOrigin,
    /// Cascade order within the origin: specificity, then source order.
    pub(crate) sort_key: u64,
    /// The index of the `@media` condition chain, if the rule is inside
    /// `@media`.
    media: Option<u32>,
    /// Hashes for the ancestor Bloom filter.
    ancestor_hashes: SmallVec<[u32; MAX_ANCESTOR_HASHES]>,
    /// True if the computed values can depend on the element itself, not
    /// only on the declarations and the parent style: the rule sets
    /// `content`, which can use `attr()`. Styles from such rules are not
    /// shared between elements.
    pub(crate) uses_element_data: bool,
}

impl Rule {
    /// The full cascade sort key: origin, specificity, source order.
    fn cascade_key(&self) -> (CascadeOrigin, u64) {
        (self.origin, self.sort_key)
    }
}

/// Rules bucketed by the rightmost compound selector.
#[derive(Debug, Default)]
struct RuleMap {
    ids: HashMap<Box<str>, Vec<u32>>,
    classes: HashMap<Box<str>, Vec<u32>>,
    html_names: HashMap<Box<str>, Vec<u32>>,
    other_names: HashMap<Box<str>, Vec<u32>>,
    universal: Vec<u32>,
    len: usize,
}

impl RuleMap {
    fn insert(&mut self, key: BucketKey<'_>, index: u32, quirks: bool) {
        let fold = |s: &str| -> Box<str> {
            if quirks {
                s.to_ascii_lowercase().into_boxed_str()
            } else {
                s.into()
            }
        };
        match key {
            BucketKey::Id(id) => self.ids.entry(fold(id)).or_default().push(index),
            BucketKey::Class(c) => self.classes.entry(fold(c)).or_default().push(index),
            BucketKey::LocalName { name, lower_name } => {
                self.html_names
                    .entry(lower_name.into())
                    .or_default()
                    .push(index);
                self.other_names.entry(name.into()).or_default().push(index);
            }
            BucketKey::Universal => self.universal.push(index),
        }
        self.len += 1;
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Appends the indices of the rules that may match `el`.
    fn candidates(&self, el: &DomElement<'_>, quirks: bool, out: &mut Vec<u32>) {
        let data = el.data();
        let lower = |s: &str| -> String { s.to_ascii_lowercase() };
        if let Some(id) = data.id() {
            let found = if quirks {
                self.ids.get(lower(id).as_str())
            } else {
                self.ids.get(id)
            };
            out.extend(found.into_iter().flatten());
        }
        if !self.classes.is_empty() {
            let mut seen: Vec<&str> = Vec::new();
            for class in data.classes() {
                if seen.contains(&class) {
                    continue;
                }
                seen.push(class);
                let found = if quirks {
                    self.classes.get(lower(class).as_str())
                } else {
                    self.classes.get(class)
                };
                out.extend(found.into_iter().flatten());
            }
        }
        let names = if data.is_html() {
            &self.html_names
        } else {
            &self.other_names
        };
        out.extend(names.get(&**data.local_name()).into_iter().flatten());
        out.extend(&self.universal);
    }
}

/// What a rule lookup is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuleTarget {
    /// The element itself.
    Element,
    /// A pseudo-element of the element.
    Pseudo(PseudoKind),
}

/// The style rules of a document.
#[derive(Debug)]
pub struct Stylist {
    quirks: bool,
    rules: Vec<Rule>,
    element_rules: RuleMap,
    before_rules: RuleMap,
    after_rules: RuleMap,
    marker_rules: RuleMap,
    placeholder_rules: RuleMap,
    /// All media query lists that rules depend on.
    media_lists: Vec<MediaQueryList>,
    /// Chains of media query lists (nested `@media`); each rule refers to
    /// one chain.
    media_chains: Vec<Vec<u32>>,
    next_order: u32,
    /// The element states that any selector depends on.
    state_dependencies: ElementState,
    /// The valid `@font-face` rules in document order, with their media
    /// chain.
    font_faces: Vec<(FontFace, Option<u32>)>,
    /// True if `@font-face` rules beyond [`MAX_FONT_FACES`] were ignored.
    font_faces_truncated: bool,
}

/// The most `@font-face` rules that a document can have; later rules are
/// ignored. Real pages have up to a few thousand (families split into
/// `unicode-range` subsets for many weights); the limit bounds the work of
/// font matching on hostile pages (ADR 0022).
pub(crate) const MAX_FONT_FACES: usize = 10_000;

impl Stylist {
    /// Creates a stylist with the user-agent stylesheet (and, in quirks
    /// mode, the quirks-mode user-agent rules) and the presentational hint
    /// rules.
    pub fn new(quirks: swb_dom::QuirksMode) -> Self {
        let mut stylist = Stylist {
            quirks: quirks == swb_dom::QuirksMode::Quirks,
            rules: Vec::new(),
            element_rules: RuleMap::default(),
            before_rules: RuleMap::default(),
            after_rules: RuleMap::default(),
            marker_rules: RuleMap::default(),
            placeholder_rules: RuleMap::default(),
            media_lists: Vec::new(),
            media_chains: Vec::new(),
            next_order: 0,
            state_dependencies: ElementState::empty(),
            font_faces: Vec::new(),
            font_faces_truncated: false,
        };
        let ua = ParserContext::user_agent();
        stylist.add_rules(&UA_SHEET.rules, &ua, CascadeOrigin::UserAgent, None);
        if stylist.quirks {
            stylist.add_rules(&UA_QUIRKS_SHEET.rules, &ua, CascadeOrigin::UserAgent, None);
        }
        stylist.add_rules(
            &HINTS_SHEET.rules,
            &ua,
            CascadeOrigin::PresentationalHint,
            None,
        );
        stylist
    }

    /// Adds an author stylesheet. Relative `url()` values resolve against
    /// `base_url`. Sheets must be added in document order.
    pub fn add_author_sheet(&mut self, sheet: &Stylesheet, base_url: &Url) {
        let cx = ParserContext::author(Some(Arc::new(base_url.clone())), self.quirks);
        self.add_rules(&sheet.rules, &cx, CascadeOrigin::Author, None);
    }

    /// The element states that some selector depends on. When other
    /// states change, styles stay the same.
    pub fn state_dependencies(&self) -> ElementState {
        self.state_dependencies
    }

    /// True in quirks mode.
    pub(crate) fn quirks(&self) -> bool {
        self.quirks
    }

    /// The number of rules (one per selector).
    #[cfg(test)]
    fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// A rule by index.
    pub(crate) fn rule(&self, index: u32) -> &Rule {
        &self.rules[index as usize]
    }

    fn add_rules(
        &mut self,
        rules: &[CssRule],
        cx: &ParserContext,
        origin: CascadeOrigin,
        media: Option<u32>,
    ) {
        for rule in rules {
            match rule {
                CssRule::Style(style) => self.add_style_rule(style, cx, origin, media),
                CssRule::Media(m) => {
                    let list_index = self.media_lists.len() as u32;
                    self.media_lists.push(m.media.clone());
                    let mut chain = media
                        .map(|c| self.media_chains[c as usize].clone())
                        .unwrap_or_default();
                    chain.push(list_index);
                    let chain_index = self.media_chains.len() as u32;
                    self.media_chains.push(chain);
                    self.add_rules(&m.rules, cx, origin, Some(chain_index));
                }
                CssRule::Supports(s) => {
                    if s.condition.evaluate(&is_supported) {
                        self.add_rules(&s.rules, cx, origin, media);
                    }
                }
                CssRule::FontFace(rule) => {
                    let Some(face) = FontFace::parse(rule, cx) else {
                        continue;
                    };
                    if self.font_faces.len() < MAX_FONT_FACES {
                        self.font_faces.push((face, media));
                    } else if !self.font_faces_truncated {
                        log::warn!(
                            "more than {MAX_FONT_FACES} @font-face rules; ignoring the rest"
                        );
                        self.font_faces_truncated = true;
                    }
                }
                CssRule::Import(_) => {}
            }
        }
    }

    fn add_style_rule(
        &mut self,
        rule: &StyleRule,
        cx: &ParserContext,
        origin: CascadeOrigin,
        media: Option<u32>,
    ) {
        let block = DeclarationBlock::parse(&rule.declarations, cx);
        if block.is_empty() {
            return;
        }
        let uses_element_data = block
            .normal
            .iter()
            .chain(&block.important)
            .any(|d| d.longhand() == Some(crate::properties::LonghandId::Content));
        let block = Arc::new(block);
        for selector in &rule.selectors {
            let map = match selector.pseudo_element() {
                None => &mut self.element_rules,
                Some(PseudoElement::Before) => &mut self.before_rules,
                Some(PseudoElement::After) => &mut self.after_rules,
                Some(PseudoElement::Marker) => &mut self.marker_rules,
                Some(PseudoElement::Placeholder) => &mut self.placeholder_rules,
                Some(other) => {
                    trace!(
                        "ignored rule for unsupported pseudo-element ::{}",
                        other.name()
                    );
                    continue;
                }
            };
            let specificity = if origin == CascadeOrigin::PresentationalHint {
                0
            } else {
                selector.specificity().value()
            };
            let index = self.rules.len() as u32;
            map.insert(selector.bucket_key(), index, self.quirks);
            self.state_dependencies |= selector.state_dependencies();
            self.rules.push(Rule {
                selector: selector.clone(),
                block: Arc::clone(&block),
                origin,
                sort_key: (u64::from(specificity) << 32) | u64::from(self.next_order),
                media,
                ancestor_hashes: ancestor_hashes(selector, self.quirks).into_iter().collect(),
                uses_element_data,
            });
            self.next_order += 1;
        }
    }

    /// Evaluates the media conditions for an environment. The result is
    /// indexed by media chain.
    pub(crate) fn evaluate_media(&self, env: &MediaEnvironment) -> Vec<bool> {
        let lists: Vec<bool> = self.media_lists.iter().map(|m| m.matches(env)).collect();
        self.media_chains
            .iter()
            .map(|chain| chain.iter().all(|&i| lists[i as usize]))
            .collect()
    }

    /// The `@font-face` rules whose `@media` conditions match `env`, in
    /// document order.
    pub fn font_faces(&self, env: &MediaEnvironment) -> Vec<&FontFace> {
        let media = self.evaluate_media(env);
        self.font_faces
            .iter()
            .filter(|(_, chain)| {
                chain.is_none_or(|c| media.get(c as usize).copied().unwrap_or(false))
            })
            .map(|(face, _)| face)
            .collect()
    }

    /// True if any rule targets the pseudo-element.
    pub(crate) fn has_pseudo_rules(&self, kind: PseudoKind) -> bool {
        !self.map(RuleTarget::Pseudo(kind)).is_empty()
    }

    fn map(&self, target: RuleTarget) -> &RuleMap {
        match target {
            RuleTarget::Element => &self.element_rules,
            RuleTarget::Pseudo(PseudoKind::Before) => &self.before_rules,
            RuleTarget::Pseudo(PseudoKind::After) => &self.after_rules,
            RuleTarget::Pseudo(PseudoKind::Marker) => &self.marker_rules,
            RuleTarget::Pseudo(PseudoKind::Placeholder) => &self.placeholder_rules,
        }
    }

    /// Finds the rules that match `el` (or its pseudo-element), sorted by
    /// cascade order (origin, specificity, source order). `filter`, if
    /// given, must hold exactly the ancestors of `el`. `scratch` is a
    /// reusable buffer.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn matching_rules(
        &self,
        el: &DomElement<'_>,
        target: RuleTarget,
        media: &[bool],
        filter: Option<&AncestorFilter>,
        context: &mut MatchingContext,
        scratch: &mut Vec<u32>,
        out: &mut Vec<u32>,
    ) {
        scratch.clear();
        self.map(target).candidates(el, self.quirks, scratch);
        for &index in scratch.iter() {
            let rule = &self.rules[index as usize];
            if let Some(chain) = rule.media
                && !media.get(chain as usize).copied().unwrap_or(false)
            {
                continue;
            }
            if let Some(filter) = filter
                && !rule
                    .ancestor_hashes
                    .iter()
                    .all(|&h| filter.might_contain(h))
            {
                continue;
            }
            if matches(&rule.selector, el, context) {
                out.push(index);
            }
        }
        out.sort_unstable_by_key(|&i| self.rules[i as usize].cascade_key());
        // In quirks mode, two classes that differ only in case look up the
        // same bucket.
        out.dedup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_faces_follow_media_and_supports() {
        let mut stylist = Stylist::new(swb_dom::QuirksMode::NoQuirks);
        let css = "@font-face{font-family:A;src:url(a.woff2)}\
                   @media (max-width:100px){@font-face{font-family:B;src:url(b.woff2)}}\
                   @media (min-width:100px){@font-face{font-family:C;src:url(c.woff2)}}\
                   @supports (display:grid){@font-face{font-family:D;src:url(d.woff2)}}\
                   @supports (display:bogus){@font-face{font-family:E;src:url(e.woff2)}}\
                   @font-face{font-family:F}";
        let base = Url::parse("https://example.com/s/x.css").unwrap();
        stylist.add_author_sheet(&parse_stylesheet(css), &base);
        let env = MediaEnvironment {
            viewport_width: 800.0,
            ..MediaEnvironment::default()
        };
        let families: Vec<&str> = stylist
            .font_faces(&env)
            .iter()
            .map(|f| &*f.family)
            .collect();
        assert_eq!(families, ["A", "C", "D"]);
    }

    #[test]
    fn font_faces_are_limited() {
        let mut stylist = Stylist::new(swb_dom::QuirksMode::NoQuirks);
        let css = "@font-face{font-family:A;src:url(a)}".repeat(MAX_FONT_FACES + 5);
        let base = Url::parse("https://example.com/").unwrap();
        stylist.add_author_sheet(&parse_stylesheet(&css), &base);
        assert_eq!(
            stylist.font_faces(&MediaEnvironment::default()).len(),
            MAX_FONT_FACES
        );
    }

    #[test]
    fn ua_sheets_parse_and_are_all_used() {
        let stylist = Stylist::new(swb_dom::QuirksMode::Quirks);
        assert!(stylist.rule_count() > 100);
        // Every UA declaration must be valid: an invalid one would be
        // silently dropped.
        for sheet in [&*UA_SHEET, &*UA_QUIRKS_SHEET, &*HINTS_SHEET] {
            let cx = ParserContext::user_agent();
            let mut rules = Vec::new();
            collect_style_rules(&sheet.rules, &mut rules);
            for rule in rules {
                for d in &rule.declarations {
                    let mut out = Vec::new();
                    assert!(
                        crate::properties::parse_declaration(&d.name, &d.value, &cx, &mut out),
                        "invalid UA declaration `{}` in `{}`",
                        d.name,
                        rule.selectors
                    );
                }
            }
        }
    }

    /// Counts the rule blocks (`{`) in CSS source without comments.
    fn count_blocks(css: &str) -> usize {
        let mut count = 0;
        let mut rest = css;
        while let Some(start) = rest.find("/*") {
            count += rest[..start].matches('{').count();
            rest = rest[start..]
                .find("*/")
                .map_or("", |end| &rest[start + end + 2..]);
        }
        count + rest.matches('{').count()
    }

    #[test]
    fn every_ua_selector_parses() {
        // A selector that the css crate rejects drops its whole rule.
        for (source, sheet) in [
            (include_str!("ua.css"), &*UA_SHEET),
            (include_str!("ua-quirks.css"), &*UA_QUIRKS_SHEET),
            (include_str!("hints.css"), &*HINTS_SHEET),
        ] {
            let mut rules = Vec::new();
            collect_style_rules(&sheet.rules, &mut rules);
            assert_eq!(rules.len(), count_blocks(source));
        }
    }

    fn collect_style_rules<'a>(rules: &'a [CssRule], out: &mut Vec<&'a StyleRule>) {
        for rule in rules {
            match rule {
                CssRule::Style(s) => out.push(s),
                CssRule::Media(m) => collect_style_rules(&m.rules, out),
                CssRule::Supports(s) => collect_style_rules(&s.rules, out),
                _ => {}
            }
        }
    }

    #[test]
    fn media_chains() {
        let mut stylist = Stylist::new(swb_dom::QuirksMode::NoQuirks);
        let sheet = parse_stylesheet(
            "@media (min-width: 500px) { a { color: red } @media (max-width: 900px) { b { color: red } } }",
        );
        stylist.add_author_sheet(&sheet, &Url::parse("https://example.com/").expect("url"));
        let narrow = MediaEnvironment {
            viewport_width: 400.0,
            ..MediaEnvironment::default()
        };
        let medium = MediaEnvironment::default();
        let wide = MediaEnvironment {
            viewport_width: 1000.0,
            ..MediaEnvironment::default()
        };
        assert_eq!(stylist.evaluate_media(&narrow), vec![false, false]);
        assert_eq!(stylist.evaluate_media(&medium), vec![true, true]);
        assert_eq!(stylist.evaluate_media(&wide), vec![true, false]);
    }
}
