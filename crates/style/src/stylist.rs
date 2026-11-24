//! The stylist: all style rules of a document (user-agent, presentational
//! hints and author), indexed for fast lookup.
//!
//! Each selector of a style rule becomes one [`Rule`]. Rules are put in
//! buckets by the key of their rightmost compound selector
//! ([`swb_css::Selector::bucket_key`]): ID, class, local name, or
//! universal. To find the rules that can match an element, the cascade
//! looks up the element's ID, classes and local name, plus the universal
//! bucket. Rules for `::before`, `::after`, `::marker` and `::placeholder`
//! have their own maps; rules for other pseudo-elements are dropped. The
//! pseudo-elements of `details` get their styles in `style_details_pseudos`
//! (in the cascade), without rules.
//!
//! `@media` conditions are evaluated when styles are computed (the
//! environment can change); `@supports` conditions are evaluated when a
//! sheet is added. `@import` loads nothing here, but an `@import ... layer`
//! declares its layer. `@font-face` rules are kept with their `@media`
//! conditions ([`Stylist::font_faces`]).
//!
//! Cascade layers: every rule has the node of its layer in the
//! [`LayerTree`] of the author origin (one tree for all sheets, in document
//! order). The tree keeps the declarations of the layers; the ranks that
//! order the layers are computed with the `@media` evaluation
//! ([`Stylist::layer_ranks`]), and [`Stylist::matching_rules`] sorts by
//! origin, layer rank, specificity and source order. The cascade reverses
//! the layer order for `!important` declarations.
//!
//! Shadow trees (ADR 0036 section 5): the sheets of a shadow tree go into a
//! scoped stylist ([`Stylist::new_scoped`]: author rules only, its own layer
//! tree, no `@font-face`), which the document's stylist holds
//! ([`Stylist::add_scope`]); shadow roots with the same sheets share one
//! ([`Stylist::set_root_scope`]). Each stylist sorts its rules into
//! [`RuleSet`]s: the rules of its own tree, those whose subject is the
//! featureless host (`:host`, `:host()`, `:host-context()`), `::slotted()`
//! rules and `::part()` rules. The document's stylist keeps no `:host` and
//! `::slotted()` rules: they match nothing there.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, OnceLock, PoisonError};

use log::trace;
use swb_css::{
    BucketKey, ContainerCondition, CssRule, ElementState, FontFaceRule, HostRelation, ImportLayer,
    LayerName, MatchingContext, MediaEnvironment, MediaQueryList, PseudoElement, Selector,
    SelectorList, ShadowPseudoKind, Stylesheet, matches, parse_stylesheet,
};
use url::Url;

use smallvec::SmallVec;

use crate::animation::KeyframeSet;
use crate::bloom::{AncestorFilter, MAX_ANCESTOR_HASHES, ancestor_hashes};
use crate::container::Containers;
use crate::cssom::sheet::{Hold, RuleKind, RuleRef, StyleSheet};
use crate::element::DomElement;
use crate::font_face::FontFace;
use crate::invalidation::InvalidationIndex;
use crate::layers::{LayerTree, UNLAYERED};
use crate::parse::ParserContext;
use crate::properties::{DeclarationBlock, is_supported};
use crate::registered::{Registration, Registry};
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

/// Where rules are added: inside which `@media` chain, which layer and
/// which `@container` chain.
#[derive(Clone, Copy, Debug)]
struct Scope {
    /// The index of the `@media` condition chain.
    media: Option<u32>,
    /// The node of the layer in the layer tree.
    layer: u32,
    /// The index of the `@container` condition chain.
    container: Option<u32>,
}

impl Default for Scope {
    fn default() -> Self {
        Scope {
            media: None,
            layer: UNLAYERED,
            container: None,
        }
    }
}

/// One selector of a style rule and the rule's declarations.
#[derive(Debug)]
pub(crate) struct Rule {
    pub(crate) selector: Selector,
    pub(crate) block: Arc<DeclarationBlock>,
    pub(crate) origin: CascadeOrigin,
    /// Cascade order within the origin and the layer: specificity, then
    /// source order.
    pub(crate) sort_key: u64,
    /// The node of the rule's layer in the layer tree ([`UNLAYERED`] for
    /// the rules that are in no layer, and for the user-agent rules).
    pub(crate) layer: u32,
    /// The index of the `@media` condition chain, if the rule is inside
    /// `@media`.
    media: Option<u32>,
    /// The index of the `@container` condition chain, if the rule is inside
    /// `@container` (ADR 0038).
    pub(crate) container: Option<u32>,
    /// Hashes for the ancestor Bloom filter.
    ancestor_hashes: SmallVec<[u32; MAX_ANCESTOR_HASHES]>,
    /// True if the computed values can depend on the element itself, not
    /// only on the declarations and the parent style: the rule sets
    /// `content`, which can use `attr()`. Styles from such rules are not
    /// shared between elements.
    pub(crate) uses_element_data: bool,
    /// True if a declaration of the rule can set `content`: `content`
    /// itself, or `all` (which expands to every longhand, `content`
    /// included). Without such a rule, the `::before` and `::after` of an
    /// element have `content: normal` and no box, and the cascade skips
    /// them (ADR 0007, "The `content` flag").
    pub(crate) sets_content: bool,
}

impl Rule {
    /// The full cascade sort key of normal declarations: origin, layer rank
    /// (`ranks` from [`Stylist::layer_ranks`]), specificity, source order.
    fn cascade_key(&self, ranks: &[u32]) -> (CascadeOrigin, u32, u64) {
        debug_assert!(
            (self.layer as usize) < ranks.len(),
            "every rule's layer node has a rank"
        );
        let rank = ranks.get(self.layer as usize).copied().unwrap_or(0);
        (self.origin, rank, self.sort_key)
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

impl RuleTarget {
    /// The index of the target in [`ShadowMaps`]; `None` for the
    /// pseudo-elements without rules.
    fn index(self) -> Option<usize> {
        match self {
            RuleTarget::Element => Some(0),
            RuleTarget::Pseudo(PseudoKind::Before) => Some(1),
            RuleTarget::Pseudo(PseudoKind::After) => Some(2),
            RuleTarget::Pseudo(PseudoKind::Marker) => Some(3),
            RuleTarget::Pseudo(PseudoKind::Placeholder) => Some(4),
            RuleTarget::Pseudo(PseudoKind::Backdrop) => Some(5),
            RuleTarget::Pseudo(PseudoKind::DetailsSummary | PseudoKind::DetailsContent) => None,
        }
    }

    /// The target of the rules of a selector's pseudo-element, if the
    /// stylist keeps them.
    fn of(pseudo: Option<PseudoElement>) -> Option<RuleTarget> {
        Some(match pseudo {
            None => RuleTarget::Element,
            Some(PseudoElement::Before) => RuleTarget::Pseudo(PseudoKind::Before),
            Some(PseudoElement::After) => RuleTarget::Pseudo(PseudoKind::After),
            Some(PseudoElement::Marker) => RuleTarget::Pseudo(PseudoKind::Marker),
            Some(PseudoElement::Placeholder) => RuleTarget::Pseudo(PseudoKind::Placeholder),
            Some(PseudoElement::Backdrop) => RuleTarget::Pseudo(PseudoKind::Backdrop),
            Some(_) => return None,
        })
    }
}

/// The number of [`RuleTarget`]s that have rules.
const TARGETS: usize = 6;

/// The sets of rules of a stylist that match across a shadow boundary (ADR
/// 0036 section 5): rules whose subject is the featureless host (`:host`,
/// `:host()`, `:host-context()`), `::slotted()` rules and `::part()` rules.
/// The document's author rules are [`RuleSet::Tree`] rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuleSet {
    /// Rules that match elements of the stylist's own tree (all rules of
    /// the user-agent sheets and of the document).
    Tree,
    /// Rules of a shadow tree that match its host.
    Host,
    /// `::slotted()` rules of a shadow tree.
    Slotted,
    /// `::part()` rules, which match parts of the shadow trees of the
    /// stylist's tree.
    Part,
}

/// The maps of the [`RuleSet`]s other than [`RuleSet::Tree`], by target;
/// a map exists once it has a rule.
#[derive(Debug, Default)]
struct ShadowMaps {
    host: [Option<Box<RuleMap>>; TARGETS],
    slotted: [Option<Box<RuleMap>>; TARGETS],
    part: [Option<Box<RuleMap>>; TARGETS],
}

impl ShadowMaps {
    fn set(&self, set: RuleSet) -> Option<&[Option<Box<RuleMap>>; TARGETS]> {
        match set {
            RuleSet::Tree => None,
            RuleSet::Host => Some(&self.host),
            RuleSet::Slotted => Some(&self.slotted),
            RuleSet::Part => Some(&self.part),
        }
    }

    fn set_mut(&mut self, set: RuleSet) -> Option<&mut [Option<Box<RuleMap>>; TARGETS]> {
        match set {
            RuleSet::Tree => None,
            RuleSet::Host => Some(&mut self.host),
            RuleSet::Slotted => Some(&mut self.slotted),
            RuleSet::Part => Some(&mut self.part),
        }
    }
}

/// The index of a scoped stylist ([`Stylist::scope`]).
pub(crate) type ScopeIndex = u32;

/// The style rules of a document.
#[derive(Debug)]
pub struct Stylist {
    quirks: bool,
    /// False for `--animations off` (ADR 0044 section 10): the
    /// `animation-*` and `transition-*` longhands compute to their initial
    /// values.
    animations: bool,
    rules: Vec<Rule>,
    element_rules: RuleMap,
    before_rules: RuleMap,
    after_rules: RuleMap,
    marker_rules: RuleMap,
    placeholder_rules: RuleMap,
    backdrop_rules: RuleMap,
    /// No rules: the pseudo-elements of `details` have no rules.
    no_rules: RuleMap,
    /// All media query lists that rules depend on.
    media_lists: Vec<MediaQueryList>,
    /// Chains of media query lists (nested `@media`); each rule refers to
    /// one chain.
    media_chains: Vec<Vec<u32>>,
    /// Chains of `@container` conditions (nested `@container`); each rule
    /// inside `@container` refers to one chain (ADR 0038).
    container_chains: Vec<Arc<[Arc<ContainerCondition>]>>,
    /// True if some rule's declarations use container query units.
    container_units: bool,
    /// True if a rule for a pseudo-element sets a `transition-*` longhand.
    pseudo_transitions: bool,
    /// The cascade layers of the author origin.
    layers: LayerTree,
    next_order: u32,
    /// The element states that any selector depends on.
    state_dependencies: ElementState,
    /// The valid `@font-face` rules in document order, with their media
    /// chain.
    font_faces: Vec<(FontFace, Option<u32>)>,
    /// True if `@font-face` rules beyond [`MAX_FONT_FACES`] were ignored.
    font_faces_truncated: bool,
    /// The valid `@property` rules in document order, with their media
    /// chain (ADR 0038, section "7b"). Rules of shadow trees' sheets and
    /// inside `@container` do not register.
    registrations: Vec<(Registration, Option<u32>)>,
    /// [`Stylist::registry`] when no registration depends on `@media`.
    registry_cache: OnceLock<Option<Arc<Registry>>>,
    /// See [`Stylist::set_dark_canvas_text`].
    dark_canvas_text: bool,
    /// True for the stylist of a shadow tree's sheets ([`Stylist::new_scoped`]).
    scoped: bool,
    /// The rules that match across a shadow boundary.
    shadow_maps: Option<Box<ShadowMaps>>,
    /// The stylists of the sheets of shadow trees (document stylist only):
    /// one per distinct list of sheets (ADR 0036 section 5).
    scopes: Vec<Stylist>,
    /// The stylist of each shadow root that has sheets.
    scope_of_root: HashMap<swb_dom::NodeId, ScopeIndex>,
    /// The pseudo-element targets that some scoped stylist has rules for
    /// (bits by [`RuleTarget::index`]).
    scoped_targets: u8,
    /// What the scoped stylists hold of the document's style memory.
    scope_holds: Vec<Hold>,
    /// The dependencies of the selectors on the element states and the
    /// DOM (ADR 0041, ADR 0042), built at the first targeted restyle.
    invalidation_index: OnceLock<InvalidationIndex>,
    /// The `@keyframes` rules in document order (ADR 0044, M13 feature
    /// 3a), with their keyframe sets once built.
    keyframes: Keyframes,
}

/// The `@keyframes` rules of a stylist and their resolved sets.
#[derive(Debug, Default)]
struct Keyframes {
    /// The rules in document order.
    rules: Vec<KeyframesEntry>,
    /// The rules of each name, in document order; built at the first
    /// lookup.
    by_name: OnceLock<HashMap<Arc<str>, Vec<usize>>>,
    /// The resolved set of each rule that a lookup chose (by index into
    /// `rules`); built at the first lookup of the rule.
    sets: Mutex<HashMap<usize, Arc<KeyframeSet>>>,
}

/// One `@keyframes` rule and where it is.
#[derive(Debug)]
struct KeyframesEntry {
    rule: Arc<swb_css::KeyframesRule>,
    /// The context to parse its declarations with (the sheet's base URL).
    cx: ParserContext,
    /// The node of its layer.
    layer: u32,
    /// Its `@media` chain.
    media: Option<u32>,
}

/// The estimated bytes of a scoped stylist without its rules, for the
/// document's style memory: the stylist and its maps.
pub const SCOPE_BYTES: usize = 4096;

/// The estimated bytes of one rule (selector) of a scoped stylist: the
/// rule, its selector and its entries in the maps.
pub const SCOPE_RULE_BYTES: usize = 256;

/// The most `@font-face` rules that a document can have; later rules are
/// ignored. Real pages have up to a few thousand (families split into
/// `unicode-range` subsets for many weights); the limit bounds the work of
/// font matching on hostile pages (ADR 0022).
pub(crate) const MAX_FONT_FACES: usize = 10_000;

impl Stylist {
    /// True if the initial `color` is the `CanvasText` of the dark color
    /// scheme.
    pub fn dark_canvas_text(&self) -> bool {
        self.dark_canvas_text
    }

    /// Sets the initial `color` to the `CanvasText` of the dark color scheme
    /// (white, measured in a frame with `color-scheme: dark`), or back to
    /// black. The engine sets it for frame documents whose root has the dark
    /// used color scheme (ADR 0031 section 4).
    pub fn set_dark_canvas_text(&mut self, dark: bool) {
        self.dark_canvas_text = dark;
    }

    /// Creates a stylist with the user-agent stylesheet (and, in quirks
    /// mode, the quirks-mode user-agent rules) and the presentational hint
    /// rules.
    pub fn new(quirks: swb_dom::QuirksMode) -> Self {
        let mut stylist = Self::new_scoped(quirks);
        stylist.scoped = false;
        let ua = ParserContext::user_agent();
        stylist.add_rules(
            &UA_SHEET.rules,
            &ua,
            CascadeOrigin::UserAgent,
            Scope::default(),
        );
        if stylist.quirks {
            stylist.add_rules(
                &UA_QUIRKS_SHEET.rules,
                &ua,
                CascadeOrigin::UserAgent,
                Scope::default(),
            );
        }
        stylist.add_rules(
            &HINTS_SHEET.rules,
            &ua,
            CascadeOrigin::PresentationalHint,
            Scope::default(),
        );
        stylist
    }

    /// Creates the stylist of the sheets of a shadow tree (ADR 0036 section
    /// 5): only author rules, with their own cascade layers. Add it to the
    /// document's stylist with [`Stylist::add_scope`]. `@font-face` rules of
    /// shadow trees are ignored.
    pub fn new_scoped(quirks: swb_dom::QuirksMode) -> Self {
        Stylist {
            quirks: quirks == swb_dom::QuirksMode::Quirks,
            animations: true,
            rules: Vec::new(),
            element_rules: RuleMap::default(),
            before_rules: RuleMap::default(),
            after_rules: RuleMap::default(),
            marker_rules: RuleMap::default(),
            placeholder_rules: RuleMap::default(),
            backdrop_rules: RuleMap::default(),
            no_rules: RuleMap::default(),
            media_lists: Vec::new(),
            media_chains: Vec::new(),
            container_chains: Vec::new(),
            container_units: false,
            pseudo_transitions: false,
            layers: LayerTree::default(),
            next_order: 0,
            state_dependencies: ElementState::empty(),
            font_faces: Vec::new(),
            font_faces_truncated: false,
            registrations: Vec::new(),
            registry_cache: OnceLock::new(),
            dark_canvas_text: false,
            scoped: true,
            shadow_maps: None,
            scopes: Vec::new(),
            scope_of_root: HashMap::new(),
            scoped_targets: 0,
            scope_holds: Vec::new(),
            invalidation_index: OnceLock::new(),
            keyframes: Keyframes::default(),
        }
    }

    /// Adds the stylist of a shadow tree's sheets ([`Stylist::new_scoped`])
    /// and returns its index for [`Stylist::set_root_scope`]. `hold` is
    /// what it holds of the document's style memory; it returns with the
    /// stylist.
    pub fn add_scope(&mut self, scope: Stylist, hold: Hold) -> u32 {
        debug_assert!(scope.scoped, "a scope is made with Stylist::new_scoped");
        self.scope_holds.push(hold);
        self.state_dependencies |= scope.state_dependencies;
        for target in [
            PseudoKind::Before,
            PseudoKind::After,
            PseudoKind::Marker,
            PseudoKind::Placeholder,
            PseudoKind::Backdrop,
        ] {
            let target = RuleTarget::Pseudo(target);
            if scope.has_rules(target)
                && let Some(i) = target.index()
            {
                self.scoped_targets |= 1 << i;
            }
        }
        let index = self.scopes.len() as u32;
        self.scopes.push(scope);
        index
    }

    /// Applies the scoped stylist `index` to the shadow root `root`.
    pub fn set_root_scope(&mut self, root: swb_dom::NodeId, index: u32) {
        if (index as usize) < self.scopes.len() {
            self.scope_of_root.insert(root, index);
        }
    }

    /// The number of scoped stylists.
    pub fn scope_count(&self) -> usize {
        self.scopes.len()
    }

    /// The number of rules (one per selector), for memory estimates.
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// True if the stylist has no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The scoped stylist of the shadow root `root`, if it has sheets.
    pub(crate) fn scope_of(&self, root: swb_dom::NodeId) -> Option<ScopeIndex> {
        if self.scope_of_root.is_empty() {
            return None;
        }
        self.scope_of_root.get(&root).copied()
    }

    /// The scoped stylist `index`.
    pub(crate) fn scope(&self, index: ScopeIndex) -> Option<&Stylist> {
        self.scopes.get(index as usize)
    }

    /// True if styles may need the rules of shadow trees: some shadow root
    /// has sheets, or the document has `::part()` rules.
    pub(crate) fn has_shadow_rules(&self) -> bool {
        !self.scopes.is_empty() || self.shadow_maps.is_some()
    }

    /// Adds an author stylesheet. Relative `url()` values resolve against
    /// `base_url`. Sheets must be added in document order.
    pub fn add_author_sheet(&mut self, sheet: &Stylesheet, base_url: &Url) {
        let cx = ParserContext::author(Some(Arc::new(base_url.clone())), self.quirks);
        self.add_rules(&sheet.rules, &cx, CascadeOrigin::Author, Scope::default());
    }

    /// The element states that some selector depends on. When other
    /// states change, styles stay the same.
    pub fn state_dependencies(&self) -> ElementState {
        self.state_dependencies
    }

    /// The dependencies of all selectors (of the scoped stylists too) on
    /// the element states and the DOM (ADR 0041, ADR 0042), built at the
    /// first call.
    pub(crate) fn invalidation_index(&self) -> &InvalidationIndex {
        self.invalidation_index.get_or_init(|| {
            let selectors = self
                .rules
                .iter()
                .chain(self.scopes.iter().flat_map(|s| s.rules.iter()))
                .map(|r| &r.selector);
            let started = std::time::Instant::now();
            let index = InvalidationIndex::new(selectors, self.quirks);
            log::debug!(
                "style: invalidation index of {} rules in {:?}",
                self.rules.len(),
                started.elapsed()
            );
            index
        })
    }

    /// True in quirks mode.
    pub(crate) fn quirks(&self) -> bool {
        self.quirks
    }

    /// Turns the document's animations and transitions on or off
    /// (`--animations`, ADR 0044 section 10). Off, the `animation-*` and
    /// `transition-*` longhands of every element and pseudo-element compute
    /// to their initial values.
    pub fn set_animations(&mut self, enabled: bool) {
        self.animations = enabled;
    }

    /// False if the document's animations are off ([`Stylist::set_animations`]).
    pub fn animations_enabled(&self) -> bool {
        self.animations
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
        scope: Scope,
    ) {
        for rule in rules {
            match rule {
                CssRule::Style(style) => {
                    let block = DeclarationBlock::parse(&style.declarations, cx);
                    self.add_style_rule(&style.selectors, &Arc::new(block), origin, scope);
                }
                CssRule::Media(m) => {
                    let chain = self.push_media(&m.media, scope.media);
                    let inner = Scope {
                        media: Some(chain),
                        ..scope
                    };
                    self.add_rules(&m.rules, cx, origin, inner);
                }
                CssRule::Supports(s) => {
                    if s.condition.evaluate(&is_supported) {
                        self.add_rules(&s.rules, cx, origin, scope);
                    }
                }
                CssRule::LayerBlock(b) => {
                    if let Some(layer) = self.enter_layer(b.name.as_ref(), scope) {
                        let inner = Scope { layer, ..scope };
                        self.add_rules(&b.rules, cx, origin, inner);
                    }
                }
                CssRule::LayerStatement(s) => self.declare_layers(&s.names, scope),
                CssRule::FontFace(rule) => self.add_font_face(rule, cx, scope),
                CssRule::Property(rule) => self.add_registration(rule, scope),
                CssRule::Import(import) => self.declare_import_layer(import, scope),
                CssRule::Keyframes(k) => self.add_keyframes(k, cx, scope),
                CssRule::Container(c) => {
                    let inner = Scope {
                        container: Some(self.push_container(&c.condition, scope.container)),
                        ..scope
                    };
                    self.add_rules(&c.rules, cx, origin, inner);
                }
            }
        }
    }

    /// The node of the layer that a block opens inside `scope`, declared at
    /// this place. `None` if the tree is full.
    fn enter_layer(&mut self, name: Option<&LayerName>, scope: Scope) -> Option<u32> {
        let node = match name {
            Some(name) => self.layers.node(scope.layer, name),
            None => self.layers.anonymous(scope.layer),
        }?;
        // Layers inside `@container` are declared whatever the condition:
        // a condition of each element cannot order the layers.
        self.layers.declare(node, scope.media);
        Some(node)
    }

    /// Declares the layers of an `@layer` statement.
    fn declare_layers(&mut self, names: &[LayerName], scope: Scope) {
        for name in names {
            if let Some(node) = self.layers.node(scope.layer, name) {
                self.layers.declare(node, scope.media);
            }
        }
    }

    /// An `@import ... layer` declares its layer (swb loads no imports),
    /// unless its `supports()` is false or its media do not match (measured
    /// in Chromium 148: such an import does not count, a missing file does).
    fn declare_import_layer(&mut self, import: &swb_css::ImportRule, scope: Scope) {
        let Some(layer) = &import.layer else {
            return;
        };
        if import
            .supports
            .as_ref()
            .is_some_and(|c| !c.evaluate(&is_supported))
        {
            return;
        }
        let media = if import.media.is_empty() {
            scope.media
        } else {
            Some(self.push_media(&import.media, scope.media))
        };
        let scope = Scope { media, ..scope };
        match layer {
            ImportLayer::Anonymous => {
                self.enter_layer(None, scope);
            }
            ImportLayer::Named(name) => self.declare_layers(std::slice::from_ref(name), scope),
        }
    }

    /// Adds an author stylesheet from its CSSOM rules (ADR 0028 section
    /// 10): the declaration blocks that an earlier build parsed are reused,
    /// so a rebuild after a change of the rules parses nothing again but
    /// the changed rules. Relative `url()` values resolve against
    /// `base_url`. Sheets must be added in document order.
    pub fn add_cssom_sheet(&mut self, sheet: &StyleSheet, base_url: &Url) {
        let cx = ParserContext::author(Some(Arc::new(base_url.clone())), self.quirks);
        self.add_cssom_rules(sheet.rules(), &cx, Scope::default());
    }

    fn add_cssom_rules(&mut self, rules: &[RuleRef], cx: &ParserContext, scope: Scope) {
        for rule in rules {
            let mut rule = rule.borrow_mut();
            match &mut rule.kind {
                RuleKind::Style(style) => {
                    let block = style.cascade_block(cx);
                    self.add_style_rule(style.selectors(), &block, CascadeOrigin::Author, scope);
                }
                RuleKind::Media { media: list, rules } => {
                    let chain = self.push_media(list, scope.media);
                    let inner = Scope {
                        media: Some(chain),
                        ..scope
                    };
                    self.add_cssom_rules(rules, cx, inner);
                }
                RuleKind::Supports {
                    condition, rules, ..
                } => {
                    if condition.evaluate(&is_supported) {
                        self.add_cssom_rules(rules, cx, scope);
                    }
                }
                RuleKind::LayerBlock { name, rules } => {
                    if let Some(layer) = self.enter_layer(name.as_ref(), scope) {
                        let inner = Scope { layer, ..scope };
                        self.add_cssom_rules(rules, cx, inner);
                    }
                }
                RuleKind::LayerStatement { names } => self.declare_layers(names, scope),
                RuleKind::FontFace(face) => self.add_font_face(face, cx, scope),
                RuleKind::Property(rule) => self.add_registration(rule, scope),
                RuleKind::Import(import) => self.declare_import_layer(import, scope),
                RuleKind::Keyframes(k) => self.add_keyframes(k, cx, scope),
                RuleKind::Container { condition, rules } => {
                    let inner = Scope {
                        container: Some(self.push_container(condition, scope.container)),
                        ..scope
                    };
                    self.add_cssom_rules(rules, cx, inner);
                }
            }
        }
    }

    /// Adds a `@keyframes` rule (CSS Animations 1 §4), shared with the
    /// sheet. A rule inside `@container` counts whatever the condition
    /// (Chromium 148, `keyframes-conditions` of
    /// `tools/probes/css-animations-3a.json`: a name cannot depend on an
    /// element).
    fn add_keyframes(
        &mut self,
        rule: &Arc<swb_css::KeyframesRule>,
        cx: &ParserContext,
        scope: Scope,
    ) {
        self.keyframes.rules.push(KeyframesEntry {
            rule: Arc::clone(rule),
            cx: cx.clone(),
            layer: scope.layer,
            media: scope.media,
        });
    }

    /// The keyframe set of the animation name `name` for the evaluated
    /// `@media` conditions `media` and the layer ranks `ranks`
    /// ([`Stylist::evaluate_media`], [`Stylist::layer_ranks`]): the set of
    /// the rule that wins by layer and then by order. `None` if no rule
    /// has the name.
    pub(crate) fn keyframes(
        &self,
        name: &str,
        media: &[bool],
        ranks: &[u32],
    ) -> Option<Arc<KeyframeSet>> {
        let k = &self.keyframes;
        let by_name = k.by_name.get_or_init(|| {
            let mut map: HashMap<Arc<str>, Vec<usize>> = HashMap::new();
            for (i, entry) in k.rules.iter().enumerate() {
                map.entry(Arc::from(entry.rule.name.as_str()))
                    .or_default()
                    .push(i);
            }
            map
        });
        let applies = |e: &KeyframesEntry| {
            e.media
                .is_none_or(|c| media.get(c as usize).copied().unwrap_or(false))
        };
        // An `@keyframes` rule beats every `@-webkit-keyframes` rule, then
        // the highest layer rank wins, then the last rule (Chromium 148,
        // `keyframes-conditions`: also a layered `@keyframes` against an
        // unlayered `@-webkit-keyframes`).
        let key = |e: &KeyframesEntry, i: usize| {
            let rank = ranks.get(e.layer as usize).copied().unwrap_or(0);
            (!e.rule.webkit_prefixed, rank, i)
        };
        let winner = by_name
            .get(name)?
            .iter()
            .filter_map(|&i| k.rules.get(i).filter(|e| applies(e)).map(|e| key(e, i)))
            .max()?
            .2;
        let mut sets = k.sets.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(set) = sets.get(&winner) {
            return Some(Arc::clone(set));
        }
        let entry = k.rules.get(winner)?;
        let set = Arc::new(KeyframeSet::new(&entry.rule, &entry.cx));
        sets.insert(winner, Arc::clone(&set));
        Some(set)
    }

    /// True if the stylist has an `@keyframes` rule (a style computation
    /// can create animations).
    pub fn has_keyframes(&self) -> bool {
        !self.keyframes.rules.is_empty()
    }

    /// The keyframe set of the animation name `name` in the environment
    /// `env` (see [`Stylist::keyframes`]).
    pub fn keyframes_in(&self, name: &str, env: &MediaEnvironment) -> Option<Arc<KeyframeSet>> {
        let media = self.evaluate_media(env);
        let ranks = self.layer_ranks(&media);
        self.keyframes(name, &media, &ranks)
    }

    /// Adds the media query list of an `@media` rule inside the chain
    /// `parent`; returns the index of the new chain.
    fn push_media(&mut self, list: &MediaQueryList, parent: Option<u32>) -> u32 {
        let list_index = self.media_lists.len() as u32;
        self.media_lists.push(list.clone());
        let mut chain = parent
            .map(|c| self.media_chains[c as usize].clone())
            .unwrap_or_default();
        chain.push(list_index);
        let chain_index = self.media_chains.len() as u32;
        self.media_chains.push(chain);
        chain_index
    }

    /// Adds the condition of an `@container` rule inside the chain
    /// `parent`; returns the index of the new chain.
    fn push_container(&mut self, condition: &ContainerCondition, parent: Option<u32>) -> u32 {
        let mut chain: Vec<Arc<ContainerCondition>> = parent
            .and_then(|c| self.container_chains.get(c as usize))
            .map(|c| c.to_vec())
            .unwrap_or_default();
        chain.push(Arc::new(condition.clone()));
        let index = self.container_chains.len() as u32;
        self.container_chains.push(chain.into());
        index
    }

    /// The conditions of the `@container` chain `index`.
    pub(crate) fn container_chain(&self, index: u32) -> &[Arc<ContainerCondition>] {
        self.container_chains
            .get(index as usize)
            .map_or(&[], |c| &**c)
    }

    /// True if styles depend on query containers: some rule is inside
    /// `@container` or uses container query units (also in a shadow tree's
    /// stylist).
    pub fn has_container_dependence(&self) -> bool {
        !self.container_chains.is_empty()
            || self.container_units
            || self.scopes.iter().any(Stylist::has_container_dependence)
    }

    /// True if some rule's declarations use container query units (also in
    /// a shadow tree's stylist).
    pub(crate) fn has_container_units(&self) -> bool {
        self.container_units || self.scopes.iter().any(Stylist::has_container_units)
    }

    /// True if a rule for a pseudo-element sets a `transition-*` longhand
    /// (also in a shadow tree's stylist): only then can a pseudo-element
    /// have transitions.
    pub(crate) fn has_pseudo_transitions(&self) -> bool {
        self.pseudo_transitions || self.scopes.iter().any(Stylist::has_pseudo_transitions)
    }

    /// Adds the registration of a valid `@property` rule. Rules in a
    /// shadow tree's sheet and inside `@container` are ignored.
    fn add_registration(&mut self, rule: &swb_css::PropertyRule, scope: Scope) {
        if self.scoped || scope.container.is_some() {
            trace!("ignored @property in a shadow tree or inside @container");
            return;
        }
        if let Some(registration) = Registration::from_rule(rule) {
            self.registrations.push((registration, scope.media));
        }
    }

    /// The registered custom properties for the evaluated `@media`
    /// conditions `media` ([`Stylist::evaluate_media`]); `None` without
    /// registrations.
    pub(crate) fn registry(&self, media: &[bool]) -> Option<Arc<Registry>> {
        if self.registrations.is_empty() {
            return None;
        }
        let build = |applies: &dyn Fn(Option<u32>) -> bool| {
            let regs = self
                .registrations
                .iter()
                .filter(|(_, chain)| applies(*chain))
                .map(|(r, _)| r);
            let registry = Registry::new(regs);
            (!registry.is_empty()).then(|| Arc::new(registry))
        };
        if self.registrations.iter().all(|(_, chain)| chain.is_none()) {
            return self.registry_cache.get_or_init(|| build(&|_| true)).clone();
        }
        build(&|chain| chain.is_none_or(|c| media.get(c as usize).copied().unwrap_or(false)))
    }

    /// Adds a valid `@font-face` rule, up to [`MAX_FONT_FACES`]. Rules
    /// inside `@container` are ignored (a face cannot depend on an
    /// element).
    fn add_font_face(&mut self, rule: &FontFaceRule, cx: &ParserContext, scope: Scope) {
        let media = scope.media;
        if self.scoped {
            trace!("ignored @font-face in a shadow tree's sheet");
            return;
        }
        if scope.container.is_some() {
            trace!("ignored @font-face inside @container");
            return;
        }
        let Some(face) = FontFace::parse(rule, cx) else {
            return;
        };
        if self.font_faces.len() < MAX_FONT_FACES {
            self.font_faces.push((face, media));
        } else if !self.font_faces_truncated {
            log::warn!("more than {MAX_FONT_FACES} @font-face rules; ignoring the rest");
            self.font_faces_truncated = true;
        }
    }

    fn add_style_rule(
        &mut self,
        selectors: &SelectorList,
        block: &Arc<DeclarationBlock>,
        origin: CascadeOrigin,
        scope: Scope,
    ) {
        if block.is_empty() {
            return;
        }
        // `all` is stored as one declaration per longhand, so it shows as
        // `content` here.
        let uses_element_data = block
            .normal
            .iter()
            .chain(&block.important)
            .any(|d| d.longhand() == Some(crate::properties::LonghandId::Content));
        let sets_content = uses_element_data;
        let sets_transitions = block.normal.iter().chain(&block.important).any(|d| {
            use crate::properties::LonghandId as L;
            matches!(
                d.longhand(),
                Some(
                    L::TransitionProperty
                        | L::TransitionDuration
                        | L::TransitionDelay
                        | L::TransitionTimingFunction
                        | L::TransitionBehavior
                )
            )
        });
        for selector in selectors {
            let Some(target) = RuleTarget::of(selector.pseudo_element()) else {
                if let Some(other) = selector.pseudo_element() {
                    trace!(
                        "ignored rule for unsupported pseudo-element ::{}",
                        other.name()
                    );
                }
                continue;
            };
            let sets = self.rule_sets(selector);
            if sets.is_empty() {
                continue;
            }
            let specificity = if origin == CascadeOrigin::PresentationalHint {
                0
            } else {
                selector.specificity().value()
            };
            let index = self.rules.len() as u32;
            let quirks = self.quirks;
            for &set in &sets {
                self.map_mut(set, target)
                    .insert(selector.bucket_key(), index, quirks);
            }
            self.state_dependencies |= selector.state_dependencies();
            self.container_units |= block.uses_container_units;
            self.pseudo_transitions |= sets_transitions && target != RuleTarget::Element;
            self.rules.push(Rule {
                selector: selector.clone(),
                block: Arc::clone(block),
                origin,
                sort_key: (u64::from(specificity) << 32) | u64::from(self.next_order),
                layer: scope.layer,
                media: scope.media,
                container: scope.container,
                ancestor_hashes: ancestor_hashes(selector, self.quirks).into_iter().collect(),
                uses_element_data,
                sets_content,
            });
            self.next_order += 1;
        }
    }

    /// The rule sets that a selector goes into (CSS Scoping 1 §3). In the
    /// document's stylist, `:host` and `::slotted()` rules match nothing
    /// (measured in Chromium 148, `host-featureless`), so they are not kept.
    fn rule_sets(&self, selector: &Selector) -> SmallVec<[RuleSet; 2]> {
        let mut sets = SmallVec::new();
        match (selector.shadow_pseudo(), self.scoped) {
            (Some(ShadowPseudoKind::Part), _) => sets.push(RuleSet::Part),
            (Some(ShadowPseudoKind::Slotted), true) => sets.push(RuleSet::Slotted),
            (Some(ShadowPseudoKind::Slotted), false) => {}
            (None, scoped) => match selector.host_relation() {
                HostRelation::Never => sets.push(RuleSet::Tree),
                HostRelation::Only if scoped => sets.push(RuleSet::Host),
                HostRelation::Only => {}
                HostRelation::Also => {
                    sets.push(RuleSet::Tree);
                    if scoped {
                        sets.push(RuleSet::Host);
                    }
                }
            },
        }
        sets
    }

    /// The map of `set` for `target`, made if needed.
    fn map_mut(&mut self, set: RuleSet, target: RuleTarget) -> &mut RuleMap {
        let Some(i) = target.index() else {
            return &mut self.no_rules;
        };
        if set == RuleSet::Tree {
            return match target {
                RuleTarget::Element => &mut self.element_rules,
                RuleTarget::Pseudo(PseudoKind::Before) => &mut self.before_rules,
                RuleTarget::Pseudo(PseudoKind::After) => &mut self.after_rules,
                RuleTarget::Pseudo(PseudoKind::Marker) => &mut self.marker_rules,
                RuleTarget::Pseudo(PseudoKind::Backdrop) => &mut self.backdrop_rules,
                RuleTarget::Pseudo(_) => &mut self.placeholder_rules,
            };
        }
        let maps = self.shadow_maps.get_or_insert_with(Box::default);
        match maps.set_mut(set) {
            Some(maps) => maps[i].get_or_insert_with(Box::default),
            None => &mut self.no_rules,
        }
    }

    /// The map of `set` for `target`, if it has rules.
    fn set_map(&self, set: RuleSet, target: RuleTarget) -> Option<&RuleMap> {
        if set == RuleSet::Tree {
            return Some(self.map(target));
        }
        self.shadow_maps.as_ref()?.set(set)?[target.index()?].as_deref()
    }

    /// True if the stylist has rules for `target` in any set.
    fn has_rules(&self, target: RuleTarget) -> bool {
        !self.map(target).is_empty()
            || [RuleSet::Host, RuleSet::Slotted, RuleSet::Part]
                .into_iter()
                .any(|set| self.set_map(set, target).is_some())
    }

    /// Appends the rules of `set` for `target` that match: candidates by
    /// the keys of `el` (the element whose key the rule's subject has: the
    /// element, also for `::slotted()`; the host for `:host`), whose `@media`
    /// chain matches (`media`), whose ancestor keys `filter` (the flat
    /// ancestors of the styled element) may hold, for which `test` answers
    /// true, and whose `@container` chain matches (`containers`). Unsorted.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn collect(
        &self,
        set: RuleSet,
        target: RuleTarget,
        el: &DomElement<'_>,
        media: &[bool],
        filter: Option<&AncestorFilter>,
        scratch: &mut Vec<u32>,
        mut test: impl FnMut(&Selector) -> bool,
        containers: &mut Containers<'_>,
        out: &mut Vec<u32>,
    ) {
        let Some(map) = self.set_map(set, target) else {
            return;
        };
        scratch.clear();
        map.candidates(el, self.quirks, scratch);
        let start = out.len();
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
            if test(&rule.selector)
                && rule
                    .container
                    .is_none_or(|c| containers.chain_matches(self.container_chain(c)))
            {
                out.push(index);
            }
        }
        // In quirks mode, two classes that differ only in case look up the
        // same bucket.
        if self.quirks {
            let mut found = out.split_off(start);
            found.sort_unstable();
            found.dedup();
            out.append(&mut found);
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

    /// The rank of each layer node for the evaluated `@media` conditions
    /// `media` (from [`Stylist::evaluate_media`]): see
    /// [`LayerTree::ranks`]. Index it with [`Rule::layer`].
    pub(crate) fn layer_ranks(&self, media: &[bool]) -> Arc<[u32]> {
        self.layers.ranks(media)
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

    /// True if any rule targets the pseudo-element (also a rule of a
    /// shadow tree).
    pub(crate) fn has_pseudo_rules(&self, kind: PseudoKind) -> bool {
        let target = RuleTarget::Pseudo(kind);
        !self.map(target).is_empty()
            || target
                .index()
                .is_some_and(|i| self.scoped_targets & (1 << i) != 0)
            || self.set_map(RuleSet::Part, target).is_some()
    }

    fn map(&self, target: RuleTarget) -> &RuleMap {
        match target {
            RuleTarget::Element => &self.element_rules,
            RuleTarget::Pseudo(PseudoKind::Before) => &self.before_rules,
            RuleTarget::Pseudo(PseudoKind::After) => &self.after_rules,
            RuleTarget::Pseudo(PseudoKind::Marker) => &self.marker_rules,
            RuleTarget::Pseudo(PseudoKind::Placeholder) => &self.placeholder_rules,
            RuleTarget::Pseudo(PseudoKind::Backdrop) => &self.backdrop_rules,
            RuleTarget::Pseudo(PseudoKind::DetailsSummary | PseudoKind::DetailsContent) => {
                &self.no_rules
            }
        }
    }

    /// Finds the rules that match `el` (or its pseudo-element), sorted by
    /// the cascade order of normal declarations (origin, layer rank,
    /// specificity, source order). `layer_ranks` is from
    /// [`Stylist::layer_ranks`]. `filter`, if given, must hold exactly the
    /// ancestors of `el`. `containers` evaluates the `@container` chains of
    /// rules whose selector matches. `scratch` is a reusable buffer.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn matching_rules(
        &self,
        el: &DomElement<'_>,
        target: RuleTarget,
        media: &[bool],
        layer_ranks: &[u32],
        filter: Option<&AncestorFilter>,
        author: bool,
        context: &mut MatchingContext,
        containers: &mut Containers<'_>,
        scratch: &mut Vec<u32>,
        out: &mut Vec<u32>,
    ) {
        scratch.clear();
        self.map(target).candidates(el, self.quirks, scratch);
        if !author {
            // An element outside the document's tree scope (ADR 0036
            // section 5): the document's author rules do not match it.
            scratch.retain(|&i| self.rules[i as usize].origin != CascadeOrigin::Author);
        }
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
            if matches(&rule.selector, el, context)
                && rule
                    .container
                    .is_none_or(|c| containers.chain_matches(self.container_chain(c)))
            {
                out.push(index);
            }
        }
        out.sort_unstable_by_key(|&i| self.rules[i as usize].cascade_key(layer_ranks));
        // In quirks mode, two classes that differ only in case look up the
        // same bucket.
        out.dedup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_css::StyleRule;

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
