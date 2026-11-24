//! The cascade and style computation for a document.
//!
//! [`compute_styles`] walks the element tree (iteratively, so deep trees
//! cannot overflow the stack) and computes one style per element, plus
//! styles for `::before`, `::after`, `::marker` and `::placeholder`, for
//! the pseudo-elements of `details` (`style_details_pseudos`), and for the
//! `::backdrop` of each element in the top layer (ADR 0037 section 8).
//!
//! For each element:
//!
//! 1. Find the matching rules ([`Stylist::matching_rules`]; a rule inside
//!    `@container` matches only if its conditions are true for the
//!    element's query containers, see [`crate::container`]), the
//!    presentational hints ([`crate::hints`]) and the `style` attribute. In
//!    a document with shadow trees also the rules of the element's own
//!    shadow tree, the `:host` rules of its shadow root, the `::slotted()`
//!    rules of the slots it is assigned to and the `::part()` rules of the
//!    tree around its host (`Styler::match_scoped`, ADR 0036 section 5).
//! 2. Pick the winning declaration of each property, in this order of
//!    precedence (CSS Cascade 5, <https://www.w3.org/TR/css-cascade-5/#cascade-sort>):
//!    user-agent `!important`, author `!important` (the inner cascade
//!    contexts of shadow trees first; in each context, the `style` attribute
//!    of its elements before rules, the rules of the first cascade layer
//!    before those of later layers, the unlayered rules last), author normal
//!    (the outer contexts first; in each, the `style` attribute, then the
//!    unlayered rules, then the layers from the last to the first), then
//!    presentational hints, user-agent normal. Within a layer, higher
//!    specificity wins, then later source order. `revert-layer` drops the
//!    author declarations of its context in its layer and the layers above
//!    it ([`Position`]).
//! 3. Compute custom properties, then `font-family` and `font-size`
//!    (other properties depend on the font size), then all other
//!    properties. Values with `var()` are substituted and parsed here.
//! 4. Apply the computed-value fixups: for an element in the top layer and
//!    a backdrop, `position` other than `absolute` and `fixed` computes to
//!    `absolute`, and the element's `overlay` to `auto`
//!    (`top_layer_fixup`); blockification
//!    (<https://www.w3.org/TR/css-display-3/#transformations>), `float`
//!    for absolutely positioned boxes, zero border widths for `none`
//!    styles, and the `overflow` pairing rule. For elements also:
//!    `display: contents` computes to `none` on the elements that CSS
//!    Display 3 Appendix B lists
//!    (<https://www.w3.org/TR/css-display-3/#unbox-html>), and tables
//!    reset `-webkit-left/center/right` in `text-align` to `start`
//!    (Chromium).
//!
//! Elements inside a `display: none` subtree get no style.
//!
//! After the main pass, `use_instances` styles the copies that SVG `use`
//! elements draw (they inherit from the `use` element). At the end,
//! `counters::resolve` replaces `counter()` and `counters()`
//! in the `content` of pseudo-elements with their text and computes the
//! ordinal values of list items.
//!
//! The `targeted` submodule brings a computed map up to date after a change
//! of the element states or of the DOM, walking only the elements that the
//! change can affect ([`targeted::restyle_targeted`], ADR 0041 and 0042).

mod animated;
#[cfg(test)]
mod animated_tests;
#[cfg(test)]
mod container_tests;
#[cfg(test)]
mod dom_tests;
#[cfg(test)]
mod kept_style_tests;
#[cfg(test)]
mod pseudo_content_tests;
mod scoping;
#[cfg(test)]
mod scoping_tests;
#[cfg(test)]
mod shadow_tests;
#[cfg(test)]
mod small_features_tests;
pub(crate) mod targeted;
#[cfg(test)]
mod targeted_tests;
#[cfg(test)]
mod test_util;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod top_layer_tests;
mod transition_styles;
#[cfg(test)]
mod transition_tests;
pub(crate) mod use_instances;

use scoping::{DOCUMENT_STYLIST, ScopeCascade, ScopedMatch};
use transition_styles::{ChangeSource, compute_with_animations};
pub use transition_styles::{TransitionInputs, sweep_animations};

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use swb_css::{MatchingContext, MediaEnvironment, parse_style_attribute};
use swb_dom::{Document, ElementData, FlatScopes, NodeId};
use url::Url;

use crate::ComputedStyle;
use crate::animation::transition_properties::TransitionStyle;
use crate::animation::{AnimationOwner, DocumentAnimations};
use crate::bloom::AncestorFilter;
use crate::container::{ContainerSizes, Containers};
use crate::custom::{
    CustomPool, CustomProperties, Registered, compute_custom_properties, substitute,
};
use crate::element::{DomElement, ElementStates, StateSet};
use crate::font_cycle::{Property as FontProperty, cycle_names};
use crate::hints::{HintContext, collect_hints};
use crate::parse::ParserContext;
use crate::properties::compute::{ComputeContext, apply, is_monospace};
use crate::properties::ids::LonghandValue;
use crate::properties::set::LonghandSet;
use crate::properties::{
    CssWideKeyword, CustomDeclaration, DeclarationBlock, LonghandId, PropertyDeclaration,
    UnparsedValue, parse_property_value,
};
use crate::registered::Registry;
use crate::style_map::{PseudoKind, StyleMap};
use crate::stylist::{CascadeOrigin, Rule, RuleSet, RuleTarget, ScopeIndex, Stylist};
use crate::values::{
    ContainerBasis, Content, ContentItem, Display, Float, FontSizeOrigin, LengthContext,
    ListStylePosition, ListStyleType, Overflow, TextAlign, UserSelect, take_container_units_read,
};

/// The ratio of the default fixed (monospace) font size to the default
/// font size: 13px / 16px.
const FIXED_FONT_RATIO: f32 = 13.0 / 16.0;

/// Computes styles for all elements (and `::before`, `::after`, `::marker`,
/// `::placeholder`, `::backdrop` and the pseudo-elements of `details`) of
/// the document.
pub fn compute_styles(
    doc: &Document,
    stylist: &Stylist,
    env: &MediaEnvironment,
    states: &ElementStates,
    base_url: &Url,
) -> StyleMap {
    let sizes = ContainerSizes::new();
    compute_styles_with_options(doc, stylist, env, states, base_url, true, &sizes, None)
}

/// [`compute_styles`] with the content boxes of the query containers from
/// the last layout (ADR 0038). The result's
/// [`StyleMap::container_log`] says what the styles read of them.
pub fn compute_styles_with_containers(
    doc: &Document,
    stylist: &Stylist,
    env: &MediaEnvironment,
    states: &ElementStates,
    base_url: &Url,
    sizes: &ContainerSizes,
) -> StyleMap {
    compute_styles_with_options(doc, stylist, env, states, base_url, true, sizes, None)
}

/// [`compute_styles_with_containers`] for a restyle: the custom-property
/// maps of `previous` are reused for equal maps, so that an unchanged
/// element has a pointer-equal map and the comparison of the two style maps
/// ([`StyleMap::same_styles`]) is cheap (ADR 0007, "Sharing of
/// custom-property maps"). The result equals that of a fresh
/// [`compute_styles_with_containers`].
pub fn compute_styles_reusing(
    doc: &Document,
    stylist: &Stylist,
    env: &MediaEnvironment,
    states: &ElementStates,
    base_url: &Url,
    sizes: &ContainerSizes,
    previous: &StyleMap,
) -> StyleMap {
    compute_styles_with_options(
        doc,
        stylist,
        env,
        states,
        base_url,
        true,
        sizes,
        Some(previous),
    )
}

/// [`compute_styles_with_containers`] (with the maps of `previous` reused,
/// as [`compute_styles_reusing`]) with the document's animations (ADR 0044
/// section 3): each element's records are brought up to date with its
/// computed `animation-*` lists, and the styles take the animated values at
/// the timeline's time. The records of elements that have no style or no
/// animation name afterwards are cancelled ([`sweep_animations`]). With
/// `transitions`, the pass is a style change event: the elements'
/// transitions start from their styles in `previous` (CSS Transitions 1
/// §3).
#[allow(clippy::too_many_arguments)]
pub fn compute_styles_animated(
    doc: &Document,
    stylist: &Stylist,
    env: &MediaEnvironment,
    states: &ElementStates,
    base_url: &Url,
    sizes: &ContainerSizes,
    previous: Option<&StyleMap>,
    animations: &RefCell<DocumentAnimations>,
    transitions: Option<TransitionInputs<'_>>,
) -> StyleMap {
    let state_set = StateSet::new(doc, states);
    let mut styler = Styler::new(
        doc, &state_set, stylist, env, base_url, true, sizes, previous,
    );
    styler.animations = Some((animations, true));
    styler.change = transitions.map(|t| ChangeSource {
        previous,
        queried: t.queried,
        element: None,
        pseudos: None,
        all: true,
        parent_changed: false,
    });
    let map = styler.run();
    sweep_animations(&map, animations);
    map
}

/// [`compute_styles`] with the ancestor Bloom filter optional (tests check
/// that the filter does not change the result).
#[allow(clippy::too_many_arguments)]
fn compute_styles_with_options(
    doc: &Document,
    stylist: &Stylist,
    env: &MediaEnvironment,
    states: &ElementStates,
    base_url: &Url,
    use_filter: bool,
    sizes: &ContainerSizes,
    previous: Option<&StyleMap>,
) -> StyleMap {
    let state_set = StateSet::new(doc, states);
    let mut styler = Styler::new(
        doc, &state_set, stylist, env, base_url, use_filter, sizes, previous,
    );
    styler.run()
}

/// Computes the styles of the element `node` and of its ancestor elements,
/// from the document element down, as [`compute_styles`] computes them
/// (ADR 0028 section 9, "the style of one element on demand"). Unlike
/// [`compute_styles`], the descendants of `display: none` elements get
/// styles too. Empty if `node` is not an element of the document tree.
/// Pseudo-elements, `use` instances and counters are not computed.
///
/// The ancestors are the flat ancestors (ADR 0036 section 4). For an
/// element outside the flat tree (a light child of a host that no slot
/// takes, unused fallback content, or a node below one), the chain stops at
/// the first element without a flat parent, which inherits from the initial
/// style (measured in Chromium 148: cases `unslotted-computed-style` and
/// `unused-fallback-computed-style` of `tools/probes/shadow-dom.json`).
///
/// With the content boxes of the query containers from the last layout
/// (ADR 0038) and the animated values of the animation records of the
/// elements, brought up to date with their styles (an element below
/// `display: none` has none).
#[allow(clippy::too_many_arguments)]
pub fn compute_element_styles_animated(
    doc: &Document,
    stylist: &Stylist,
    env: &MediaEnvironment,
    states: &ElementStates,
    base_url: &Url,
    node: NodeId,
    sizes: &ContainerSizes,
    animations: Option<&RefCell<DocumentAnimations>>,
) -> Vec<(NodeId, Arc<ComputedStyle>)> {
    let Some(root) = doc.document_element() else {
        return Vec::new();
    };
    if doc.element(node).is_none() || doc.parent(root) != Some(NodeId::DOCUMENT) {
        return Vec::new();
    }
    let mut chain: Vec<NodeId> = std::iter::once(node)
        .chain(
            doc.flat_ancestors(node)
                .take_while(|&n| n != NodeId::DOCUMENT),
        )
        .collect();
    let Some(&top) = chain.last() else {
        return Vec::new();
    };
    // A chain that does not reach the document element: an element outside
    // the flat tree, if it is in the window's document at all.
    if top != root
        && !(doc.has_shadow_state() && doc.shadow_including_root(node) == NodeId::DOCUMENT)
    {
        return Vec::new();
    }
    chain.reverse();
    let scopes = chain_scopes(doc, &chain);
    // Without the per-document tables: a pass over the document would
    // cost more than the cascade of a few elements.
    let state_set = StateSet::lazy(doc, states);
    let mut styler = Styler::new(doc, &state_set, stylist, env, base_url, false, sizes, None);
    styler.animations = animations.map(|a| (a, false));
    let mut parent = Arc::clone(&styler.cx.initial);
    let mut layout_parent = None;
    let mut out = Vec::with_capacity(chain.len());
    // The records of an element are brought up to date with its style
    // (a script changed `animation-*` and reads the style at once), except
    // below `display: none`, where an element has no animations.
    let mut hidden = false;
    for (id, scope) in chain.into_iter().zip(scopes) {
        let Some(el) = DomElement::new(doc, id, &state_set) else {
            // Only elements are between an element and the document
            // element.
            return Vec::new();
        };
        let is_root = id == root;
        styler.scope = scope;
        styler.animations = animations.map(|a| (a, !hidden));
        let style = styler.style_element(&el, &parent, layout_parent, is_root);
        hidden |= style.display == Display::None;
        if is_root {
            styler.cx.root_font_size = style.font_size;
            styler.cx.root_line_height = style.line_height_px();
            styler.containers.set_root_font_size(style.font_size);
        }
        if style.display != Display::Contents {
            layout_parent = Some(style.display);
        }
        styler.containers.push(id, &style);
        parent = Arc::clone(&style);
        out.push((id, style));
    }
    out
}

/// The computed style of the pseudo-element `kind` of the element `node`
/// for `getComputedStyle` (ADR 0028 section 10): the cascade of its rules
/// also when it generates no box (then it inherits from the element), with
/// `counter()` kept as written. `chain` is the styles of the element and
/// its ancestors from [`compute_element_styles_animated`] or the full styles, from
/// the document element down to `node`. `None` if `node` is not the last
/// element of the chain.
///
/// With the content boxes of the query containers from the last layout
/// (ADR 0038; the originating element is a query container of its
/// pseudo-element) and the animated values of the pseudo-element's
/// existing animation records (only read).
#[allow(clippy::too_many_arguments)]
pub fn compute_pseudo_element_style_animated(
    doc: &Document,
    stylist: &Stylist,
    env: &MediaEnvironment,
    states: &ElementStates,
    base_url: &Url,
    chain: &[(NodeId, Arc<ComputedStyle>)],
    kind: PseudoKind,
    sizes: &ContainerSizes,
    animations: Option<&RefCell<DocumentAnimations>>,
) -> Option<ComputedStyle> {
    let (node, style) = chain.last()?;
    let state_set = StateSet::lazy(doc, states);
    let mut styler = Styler::new(doc, &state_set, stylist, env, base_url, false, sizes, None);
    styler.animations = animations.map(|a| (a, false));
    if let Some((_, root)) = chain.first() {
        styler.cx.root_font_size = root.font_size;
        styler.cx.root_line_height = root.line_height_px();
        styler.containers.set_root_font_size(root.font_size);
    }
    for (id, s) in chain {
        styler.containers.push(*id, s);
        // The ancestors' `style` attributes can bring container units
        // through `var()`.
        if let Some(css) = doc.element(*id).and_then(|d| d.attr("style")) {
            styler.attribute_units |=
                DeclarationBlock::parse(&parse_style_attribute(css), &styler.author)
                    .uses_container_units;
        }
    }
    // The parent box of the pseudo-element: the element's, or for
    // `display: contents` that of the nearest ancestor with a box.
    let layout_parent = chain
        .iter()
        .rev()
        .map(|(_, s)| s.display)
        .find(|&d| d != Display::Contents);
    let el = DomElement::new(doc, *node, &state_set)?;
    styler.scope = doc.root(*node);
    styler.style_pseudo_element(&el, style, kind, layout_parent, true)
}

/// The tree scope (the root of the element's tree: the document or a
/// shadow root) of each element of `chain`, a path of the flat tree from
/// its top down. Linear in the length of the chain plus the depth of the
/// top in its tree.
fn chain_scopes(doc: &Document, chain: &[NodeId]) -> Vec<NodeId> {
    if !doc.has_shadow_state() {
        return vec![NodeId::DOCUMENT; chain.len()];
    }
    let mut flat = FlatScopes::default();
    let mut scope = chain.first().map_or(NodeId::DOCUMENT, |&top| doc.root(top));
    let mut scopes: Vec<NodeId> = Vec::with_capacity(chain.len());
    for &id in chain {
        scopes.push(scope);
        scope = flat.of_children(doc, id, scope);
    }
    scopes
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
    /// The matched rules of shadow trees (empty without them).
    scoped: Box<[ScopedMatch]>,
    /// The nearest query container (its arena index plus one; 0 for none
    /// or when no value can use container units, ADR 0038): elements with
    /// the same nearest container have the same unit sizes. The query
    /// results are in `rules` already. A plain number: one hash write and
    /// one comparison per element on pages without container CSS.
    container: u32,
    /// The style that the after-change styles of the element inherit from
    /// when its parent keeps one (its address; 0 for none; ADR 0044
    /// section 20), and then whether the element keeps one for its
    /// children ([`animated::keeps_parent_style`]): elements with the same
    /// key keep the same style.
    kept_parent: usize,
    keeps: bool,
}

/// The sharing key of an element, with the parent's kept style that the
/// key names by its address.
struct SharingInput {
    key: SharingKey,
    kept_parent: Option<Arc<ComputedStyle>>,
}

/// A style of the sharing cache.
struct SharedStyle {
    style: Arc<ComputedStyle>,
    /// The style that the after-change styles of the children of the
    /// elements that take `style` inherit from (`None`: `style`).
    kept: Option<Arc<ComputedStyle>>,
    /// The parent's kept style that the key names by its address, held so
    /// that the address is not reused while the cache lives.
    _kept_parent: Option<Arc<ComputedStyle>>,
}

/// Settings shared by all cascades of one document.
struct CascadeContext<'a> {
    stylist: &'a Stylist,
    env: &'a MediaEnvironment,
    quirks: bool,
    /// The root element's font size, for `rem` (the environment's default
    /// until the root is styled).
    root_font_size: f32,
    /// The root element's computed line height, for `rlh` (`normal`
    /// until the root is styled).
    root_line_height: f32,
    initial: Arc<ComputedStyle>,
    /// The rank of each cascade layer for the environment's media
    /// conditions ([`Stylist::layer_ranks`]); `layer_ranks[0]` is the rank
    /// of the unlayered rules, the highest.
    layer_ranks: Arc<[u32]>,
    /// The registered custom properties (`@property`) for the environment's
    /// media conditions; `None` without any.
    registry: Option<Arc<Registry>>,
    /// The custom-property maps of the previous style pass and of this one,
    /// so that equal maps are shared (ADR 0007).
    custom_pool: CustomPool,
}

impl CascadeContext<'_> {
    /// The rule of a [`ScopedMatch`].
    fn scoped_rule(&self, m: &ScopedMatch) -> Option<&Rule> {
        let stylist = if m.stylist == DOCUMENT_STYLIST {
            self.stylist
        } else {
            self.stylist.scope(m.stylist)?
        };
        Some(stylist.rule(m.rule))
    }
}

/// The rules that match an element or a pseudo-element.
struct MatchedRules<'m> {
    /// Rules of the document's stylist, sorted by the cascade order of
    /// normal declarations ([`Stylist::matching_rules`]).
    document: &'m [u32],
    /// Rules of shadow trees' stylists and the document's `::part()` rules.
    scoped: &'m [ScopedMatch],
    /// The cascade context of the element's own tree (that of the document's
    /// author rules in `document`, and of the `style` attribute).
    own: u32,
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
    shared: HashMap<SharingKey, SharedStyle>,
    /// The last style of an element in the top layer and the last
    /// `::backdrop` style: an equal next one reuses it ([`share_last`]).
    last_top_layer: [Option<Arc<ComputedStyle>>; 2],
    /// The modal dialog that blocks the document, if any (HTML "blocked by a
    /// modal dialog").
    blocked: Option<NodeId>,
    scratch: Vec<u32>,
    matched: Vec<u32>,
    /// The tree scope of the element being styled (the document node or a
    /// shadow root): the document's author rules apply only to elements of
    /// the document's scope, the rules of a shadow tree's sheets to the
    /// elements of that tree (ADR 0036 section 5).
    scope: NodeId,
    /// The rules of shadow trees (and the document's `::part()` rules) that
    /// match the element being styled.
    scoped_matched: Vec<ScopedMatch>,
    found: Vec<u32>,
    /// True if the document has shadow roots and the stylist has rules of
    /// shadow trees or `::part()` rules: only then are they looked up.
    shadow_rules: bool,
    /// The evaluated media and the layer ranks of each scoped stylist, made
    /// at its first use.
    scope_cascades: Vec<Option<ScopeCascade>>,
    /// The nesting depth of each shadow root met (the document is 0).
    depths: HashMap<NodeId, u32>,
    /// The query containers of the element being styled (ADR 0038).
    containers: Containers<'a>,
    /// True if some rule uses container units: then the style sharing key
    /// holds the nearest query container. (Query results need no key: they
    /// decide the matched rules, which the key holds.)
    container_units: bool,
    /// True once the walk met a `style` attribute with container units
    /// (set by the script's `style.setProperty` too: it changes the
    /// attribute). A custom property in it reaches the descendants through
    /// `var()`, and the walk styles ancestors first, so from then on the
    /// elements may use container units like those of the rules.
    attribute_units: bool,
    /// The document's animations, if the styles take animated values (ADR
    /// 0044 section 3), and whether this pass creates, updates and cancels
    /// records (`true`) or only reads them (the styles of queries).
    animations: Option<(&'a RefCell<DocumentAnimations>, bool)>,
    /// The before-change styles if the pass is a style change event (ADR
    /// 0044 section 3); `None` if it only reads the transitions.
    change: Option<ChangeSource<'a>>,
    /// True if a rule for a pseudo-element sets a `transition-*` longhand
    /// ([`Stylist::has_pseudo_transitions`]).
    pseudo_transitions: bool,
}

impl<'a> Styler<'a> {
    /// A styler for the elements of `doc`, with the ancestor Bloom filter
    /// if `use_filter`. The custom-property maps of `previous` (the styles
    /// of the last pass, for a restyle) are reused for equal maps.
    #[allow(clippy::too_many_arguments)]
    fn new(
        doc: &'a Document,
        states: &'a StateSet<'a>,
        stylist: &'a Stylist,
        env: &'a MediaEnvironment,
        base_url: &Url,
        use_filter: bool,
        sizes: &'a ContainerSizes,
        previous: Option<&StyleMap>,
    ) -> Self {
        let quirks = stylist.quirks();
        let custom_pool = CustomPool::default();
        if let Some(previous) = previous {
            previous.seed_custom_pool(&custom_pool);
        }
        let author = ParserContext::author(Some(Arc::new(base_url.clone())), quirks);
        let media = stylist.evaluate_media(env);
        let registry = stylist.registry(&media);
        let mut initial = if stylist.dark_canvas_text() {
            ComputedStyle::initial_dark()
        } else {
            ComputedStyle::initial()
        };
        if let Some(registry) = &registry {
            // The root inherits the initial values of the registered
            // properties.
            let mut style = (*initial).clone();
            style.custom_properties =
                Some(custom_pool.intern(CustomProperties::new(Some(registry))));
            initial = Arc::new(style);
        }
        Styler {
            doc,
            states,
            cx: CascadeContext {
                stylist,
                env,
                quirks,
                root_font_size: env.root_font_size,
                root_line_height: LengthContext::normal_line_height(env.root_font_size),
                initial,
                layer_ranks: stylist.layer_ranks(&media),
                registry,
                custom_pool,
            },
            media,
            matching: MatchingContext::new(if quirks {
                swb_css::QuirksMode::Quirks
            } else {
                swb_css::QuirksMode::NoQuirks
            }),
            hints: HintContext::new(doc, author.clone(), quirks),
            author,
            filter: use_filter.then(AncestorFilter::new),
            shared: HashMap::new(),
            last_top_layer: [None, None],
            blocked: doc.blocking_dialog(),
            scratch: Vec::new(),
            matched: Vec::new(),
            scope: NodeId::DOCUMENT,
            scoped_matched: Vec::new(),
            found: Vec::new(),
            shadow_rules: doc.has_shadow_state() && stylist.has_shadow_rules(),
            scope_cascades: Vec::new(),
            depths: HashMap::new(),
            containers: Containers::new(sizes, env),
            container_units: stylist.has_container_units(),
            attribute_units: false,
            animations: None,
            change: None,
            pseudo_transitions: stylist.has_pseudo_transitions(),
        }
    }

    fn run(&mut self) -> StyleMap {
        let mut map = StyleMap::with_capacity(self.doc.len());
        let Some(root) = self.doc.document_element() else {
            return map;
        };
        // Pre-order traversal of the flat tree (ADR 0036 section 4). `path`
        // holds the flat ancestors of the current element, whose keys are in
        // the Bloom filter: a superset of the DOM ancestors inside the
        // element's tree, so the filter never rejects a rule wrongly. Each
        // entry carries the element's tree scope (the root of its tree:
        // the document or a shadow root).
        let mut stack: Vec<Pending> = vec![(
            root,
            Arc::clone(&self.cx.initial),
            None,
            0,
            NodeId::DOCUMENT,
        )];
        let mut path: Vec<NodeId> = Vec::new();
        let mut children = Vec::new();
        let mut scopes = FlatScopes::default();
        let shadow = self.doc.has_shadow_state();
        let quirks = self.cx.quirks;
        while let Some((node, parent, layout_parent, depth, scope)) = stack.pop() {
            while path.len() > depth {
                if let Some(ancestor) = path.pop() {
                    self.containers.pop_if(ancestor);
                    if let (Some(filter), Some(e)) = (&mut self.filter, self.doc.element(ancestor))
                    {
                        filter.pop_element(e, quirks);
                    }
                }
            }
            let Some(el) = DomElement::new(self.doc, node, self.states) else {
                continue;
            };
            let is_root = node == root;
            self.scope = scope;
            self.containers.take_read();
            let style = self.style_element(&el, &parent, layout_parent, is_root);
            if is_root {
                self.cx.root_font_size = style.font_size;
                self.cx.root_line_height = style.line_height_px();
                self.containers.set_root_font_size(style.font_size);
            }
            if style.display != Display::None {
                // The element is a query container of its pseudo-elements
                // and of its descendants (ADR 0038).
                let pushed = self.containers.push(node, &style);
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
                children.extend(self.doc.flat_element_children(node));
                if !children.is_empty() {
                    if let Some(filter) = &mut self.filter {
                        filter.push_element(el.data(), quirks);
                    }
                    path.push(node);
                } else if pushed {
                    self.containers.pop_if(node);
                }
                let child_scope = if shadow {
                    scopes.of_children(self.doc, node, scope)
                } else {
                    scope
                };
                for &child in children.iter().rev() {
                    stack.push((
                        child,
                        Arc::clone(&style),
                        child_layout_parent,
                        depth + 1,
                        child_scope,
                    ));
                }
            }
            if self.containers.take_read() {
                map.add_container_reader(node);
            }
            map.set(node, style);
        }
        self.expand_uses(&mut map);
        crate::counters::resolve(self.doc, &mut map);
        map.set_container_log(self.containers.take_log());
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
            &self.cx.layer_ranks,
            self.filter.as_ref(),
            self.scope == NodeId::DOCUMENT,
            &mut self.matching,
            &mut self.containers,
            &mut self.scratch,
            &mut matched,
        );
        let mut scoped = std::mem::take(&mut self.scoped_matched);
        scoped.clear();
        if self.shadow_rules {
            self.match_scoped(el, RuleTarget::Element, &mut scoped);
        }
        let own = if scoped.is_empty() {
            0
        } else {
            self.depth(self.scope)
        };
        let hints = self.hints_of(el);
        let style_attribute = self.parse_style_attribute_of(data);
        // Elements with the same parent style, the same rules and nothing
        // element-specific share one style.
        let stylist = self.cx.stylist;
        // The fix-up of the top layer is not in the sharing key. The rules
        // of an element in the top layer differ from those of its siblings
        // anyway (`:popover-open`, `:modal`); this keeps the cache right
        // without that.
        let top_layer = data.in_top_layer();
        let shareable = !is_root
            && !top_layer
            && hints.is_empty()
            && style_attribute.is_none()
            && matched.iter().all(|&i| !stylist.rule(i).uses_element_data)
            && scoped
                .iter()
                .all(|m| !self.cx.scoped_rule(m).is_some_and(|r| r.uses_element_data));
        let contents_is_none = contents_is_none(data);
        let sharing = shareable.then(|| {
            self.sharing_input(
                el.node_id(),
                parent,
                layout_parent,
                contents_is_none,
                &matched,
                &scoped,
            )
        });
        let owner = AnimationOwner {
            node: el.node_id(),
            pseudo: None,
        };
        if let Some(shared) = sharing.as_ref().and_then(|s| self.take_shared(s, owner)) {
            self.matched = matched;
            self.scoped_matched = scoped;
            return shared;
        }
        let extra = ExtraDeclarations {
            hints: &hints,
            style_attribute: style_attribute.as_ref(),
        };
        let rules = MatchedRules {
            document: &matched,
            scoped: &scoped,
            own,
        };
        let fixup = animated::Fixup::Parent(layout_parent);
        let mut style = self.cascade_with_units(
            &rules,
            extra,
            parent,
            Some(data),
            is_root,
            Some((owner, fixup)),
        );
        let sharing = sharing.filter(|_| self.shareable_animated(owner, &style));
        if top_layer {
            top_layer_fixup(&mut style, true);
        }
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
        self.apply_modal_inert(&mut style, el.node_id(), is_root);
        // HTML rendering: `noscript { display: none !important }` when
        // scripting is enabled (the element is also not rendered if an
        // author sets `display`; measured in Chromium 148). Chromium keeps
        // the authored value as the computed one; swb sets `none`.
        if self.cx.env.scripting && data.is_html_named(&swb_dom::local_name!("noscript")) {
            style.display = Display::None;
        }
        self.matched = matched;
        self.scoped_matched = scoped;
        self.finish_style(style, top_layer, sharing, owner.node)
    }

    /// The sharing key of the element `node` (see [`SharingKey`]) with the
    /// style that its parent keeps for it, if any.
    fn sharing_input(
        &self,
        node: NodeId,
        parent: &Arc<ComputedStyle>,
        layout_parent: Option<Display>,
        contents_is_none: bool,
        matched: &[u32],
        scoped: &[ScopedMatch],
    ) -> SharingInput {
        let kept_parent = self.kept_parent_style(node);
        let key = SharingKey {
            parent: Arc::as_ptr(parent) as usize,
            layout_parent,
            contents_is_none,
            rules: matched.into(),
            scoped: scoped.into(),
            container: self.sharing_container(),
            kept_parent: kept_parent.as_ref().map_or(0, |k| Arc::as_ptr(k) as usize),
            keeps: kept_parent.is_some() && self.has_dependents(node),
        };
        SharingInput { key, kept_parent }
    }

    /// The shared style for the inputs `sharing`, if `owner` may take it
    /// ([`Self::may_take_shared`]); `owner` then keeps the style of the
    /// cache for its children, or drops a stale one.
    fn take_shared(
        &self,
        sharing: &SharingInput,
        owner: AnimationOwner,
    ) -> Option<Arc<ComputedStyle>> {
        let shared = self.shared.get(&sharing.key)?;
        if !self.may_take_shared(owner, &shared.style, sharing.kept_parent.is_some()) {
            return None;
        }
        self.set_kept_style(owner.node, shared.kept.clone());
        Some(Arc::clone(&shared.style))
    }

    /// The style that the after-change styles of the transitions of the
    /// element `node` inherit from, if its parent keeps one (ADR 0044
    /// section 20).
    fn kept_parent_style(&self, node: NodeId) -> Option<Arc<ComputedStyle>> {
        let (animations, _) = self.animations?;
        let animations = animations.try_borrow().ok()?;
        if !animations.keeps_transition_parents() {
            return None;
        }
        animations.transition_parent(self.doc.flat_parent_element(node)?)
    }

    /// The style that `node` keeps for its children, if any.
    fn kept_style(&self, node: NodeId) -> Option<Arc<ComputedStyle>> {
        let (animations, _) = self.animations?;
        animations.try_borrow().ok()?.transition_parent(node)
    }

    /// Keeps `kept` as the style of `node` for its children (`None`: drops a
    /// stale one).
    fn set_kept_style(&self, node: NodeId, kept: Option<Arc<ComputedStyle>>) {
        if let Some((animations, _)) = self.animations
            && let Ok(mut animations) = animations.try_borrow_mut()
        {
            animations.set_transition_parent(node, kept);
        }
    }

    /// Notes that the style of the animated element `node` changed in a
    /// restyle ([`DocumentAnimations::note_value_changed`]).
    fn note_value_changed(&self, node: NodeId) {
        if let Some((animations, true)) = self.animations
            && let Ok(mut animations) = animations.try_borrow_mut()
        {
            animations.note_value_changed(node);
        }
    }

    /// True if styles inherit from the style of the element `node`: it has
    /// element children, or pseudo-elements can have transitions
    /// ([`animated::AnimationTarget::children`]).
    fn has_dependents(&self, node: NodeId) -> bool {
        self.pseudo_transitions || self.doc.flat_element_children(node).next().is_some()
    }

    /// The presentational hints of the element `el` ([`crate::hints`]).
    fn hints_of(&self, el: &DomElement<'_>) -> Vec<PropertyDeclaration> {
        let mut hints = Vec::new();
        let dimension_source = self
            .states
            .dimension_source(el.node_id())
            .and_then(|source| self.doc.element(source));
        collect_hints(
            self.doc,
            el.node_id(),
            el.data(),
            dimension_source,
            &self.hints,
            &mut hints,
        );
        hints
    }

    /// HTML "blocked by a modal dialog" (ADR 0037 section 7): the root starts
    /// the inert subtree, the dialog ends it. The flag is inherited, so the
    /// elements in between need nothing. The root and the dialog are never
    /// shared (`shareable`), so a shared style never needs a different flag.
    fn apply_modal_inert(&self, style: &mut ComputedStyle, node: NodeId, is_root: bool) {
        if let Some(dialog) = self.blocked {
            if node == dialog {
                style.modal_inert = false;
            } else if is_root {
                style.modal_inert = true;
            }
        }
    }

    /// The declarations of the element's `style` attribute. Notes that the
    /// walk met container units in one ([`Self::units_possible`]).
    fn parse_style_attribute_of(&mut self, data: &ElementData) -> Option<DeclarationBlock> {
        let block = data
            .attr("style")
            .map(|css| DeclarationBlock::parse(&parse_style_attribute(css), &self.author))?;
        self.attribute_units |= block.uses_container_units;
        Some(block)
    }

    /// True if some value of the elements styled from now on can use
    /// container units: a rule or an ancestor's `style` attribute has them.
    fn units_possible(&self) -> bool {
        self.container_units || self.attribute_units
    }

    /// True if `owner` with the style `style` may share it with the elements
    /// styled later: an element with animation names has a style of its own
    /// (its animations are its own: their play states, the values at their
    /// times; other `animation-*` values alone, as in a reduced-motion
    /// reset, make no animation and keep the sharing), and so has an
    /// element with running transitions (their values are its own).
    fn shareable_animated(&self, owner: AnimationOwner, style: &ComputedStyle) -> bool {
        !crate::animation::AnimationStyle::has_names(style)
            && (!TransitionStyle::may_transition(style) || !self.has_transitions(owner))
    }

    /// True if `owner` may take the shared style `shared` instead of a
    /// cascade of its own (ADR 0044 section 20): always when `shared` can
    /// start no transition; else only when the owner has no running
    /// transition, its after-change style would be `shared` (not so when
    /// the parent keeps a style, `kept_parent`, and the list can follow an
    /// inherited value), and its style change event, if it has one, would
    /// start none (it has no before-change style, or that style has the
    /// values of `shared` for the properties of the list).
    fn may_take_shared(
        &self,
        owner: AnimationOwner,
        shared: &ComputedStyle,
        kept_parent: bool,
    ) -> bool {
        if !TransitionStyle::may_transition(shared) {
            return true;
        }
        if self.has_transitions(owner)
            || (kept_parent && TransitionStyle::may_transition_inherited(shared))
        {
            return false;
        }
        match self.change.as_ref() {
            Some(change) if change.all || change.parent_changed => change
                .before(owner)
                .is_none_or(|before| TransitionStyle::same_transitioned_values(&before, shared)),
            _ => true,
        }
    }

    /// True if `owner` has running transitions.
    fn has_transitions(&self, owner: AnimationOwner) -> bool {
        self.animations.is_some_and(|(animations, _)| {
            animations
                .try_borrow()
                .is_ok_and(|a| !a.is_empty() && a.has_transitions(owner))
        })
    }

    /// The nearest query container for the style sharing key, if values
    /// can use container units ([`Self::units_possible`]).
    fn sharing_container(&self) -> u32 {
        if !self.units_possible() {
            return 0;
        }
        self.containers
            .nearest()
            .map_or(0, |n| u32::try_from(n.index() + 1).unwrap_or(0))
    }

    /// Runs [`cascade`] with the container units of the viewport and, if
    /// a value used a container unit (the computed values or `var()` can
    /// bring them from anywhere), again with the sizes of the element's
    /// query containers (ADR 0038). Only the second run reads the
    /// containers, so the log records the units of the elements that use
    /// them.
    /// `owner` is the element or pseudo-element whose animations give
    /// animated values, if any, with the fix-ups its style goes through.
    fn cascade_with_units(
        &mut self,
        rules: &MatchedRules<'_>,
        extra: ExtraDeclarations<'_>,
        parent: &ComputedStyle,
        element: Option<&ElementData>,
        is_root: bool,
        owner: Option<(AnimationOwner, animated::Fixup)>,
    ) -> ComputedStyle {
        let viewport = ContainerBasis {
            width: self.cx.env.viewport_width,
            height: self.cx.env.viewport_height,
        };
        let target = match (self.animations, owner) {
            (Some((animations, sync)), Some((owner, fixup))) => Some(animated::AnimationTarget {
                animations,
                owner,
                media: &self.media,
                sync,
                change: self
                    .change
                    .as_ref()
                    .filter(|c| sync && (c.all || c.parent_changed))
                    .map(|c| animated::StyleChange {
                        before: c.before(owner),
                        is_root,
                        fixup,
                        prefers: self.cx.env.prefers_color_scheme,
                    }),
                children: owner.pseudo.is_none() && self.has_dependents(owner.node),
                parent: match owner.pseudo {
                    None => self.doc.flat_parent_element(owner.node),
                    Some(_) => Some(owner.node),
                },
            }),
            _ => None,
        };
        let target = target.as_ref();
        // Without container units in the rules or in the `style` attribute
        // of the element or an ancestor no value can use one: no flag to
        // read.
        if !self.units_possible() || self.containers.nearest().is_none() {
            return cascade(
                &self.cx, rules, extra, parent, element, is_root, viewport, target,
            );
        }
        take_container_units_read();
        let style = cascade(
            &self.cx, rules, extra, parent, element, is_root, viewport, target,
        );
        if !take_container_units_read() {
            return style;
        }
        let units = self.containers.unit_basis();
        if units == viewport {
            style
        } else {
            cascade(
                &self.cx, rules, extra, parent, element, is_root, units, target,
            )
        }
    }

    /// Puts the computed `style` in an `Arc`, shared with an equal previous
    /// one in the top layer, and in the sharing cache under the inputs
    /// `sharing` of the element `node`.
    fn finish_style(
        &mut self,
        style: ComputedStyle,
        top_layer: bool,
        sharing: Option<SharingInput>,
        node: NodeId,
    ) -> Arc<ComputedStyle> {
        let style = if top_layer {
            share_last(&mut self.last_top_layer[0], style)
        } else {
            Arc::new(style)
        };
        if let Some(SharingInput { key, kept_parent }) = sharing {
            let shared = SharedStyle {
                style: Arc::clone(&style),
                kept: self.kept_style(node),
                _kept_parent: kept_parent,
            };
            self.shared.insert(key, shared);
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
        let mut kinds = Vec::with_capacity(5);
        if crate::element_kinds::generates_content_pseudos(data) {
            kinds.extend([PseudoKind::Before, PseudoKind::After]);
        }
        if style.display == Display::ListItem {
            kinds.push(PseudoKind::Marker);
        }
        if has_placeholder(data) {
            kinds.push(PseudoKind::Placeholder);
        }
        let details = data.is_html_named(&swb_dom::local_name!("details"));
        let default_summary = details && self.doc.details_summary(el.node_id()).is_none();
        if default_summary {
            // The marker of the default summary.
            kinds.push(PseudoKind::Marker);
        }
        // The backdrop of an element in the top layer (ADR 0037 section 8).
        if data.in_top_layer() {
            kinds.push(PseudoKind::Backdrop);
        }
        for kind in kinds {
            // Markers and placeholders always have a box (the user-agent
            // sheet has rules for both).
            let always = matches!(kind, PseudoKind::Marker | PseudoKind::Placeholder);
            // A backdrop's parent box is the initial containing block.
            let layout_parent = if kind == PseudoKind::Backdrop {
                None
            } else {
                pseudo_layout_parent
            };
            if let Some(pseudo) = self.style_pseudo_element(el, style, kind, layout_parent, false) {
                // `content` does not apply to a backdrop: it has a box
                // unless it is `display: none`.
                let generates_box = always
                    || (kind == PseudoKind::Backdrop && pseudo.display != Display::None)
                    || (!matches!(pseudo.content, Content::Normal | Content::None)
                        && pseudo.display != Display::None);
                if generates_box {
                    let pseudo = if kind == PseudoKind::Backdrop {
                        share_last(&mut self.last_top_layer[1], pseudo)
                    } else {
                        Arc::new(pseudo)
                    };
                    map.set_pseudo(el.node_id(), kind, pseudo);
                }
            }
        }
        if details {
            Self::style_details_pseudos(el, style, default_summary, map);
        }
    }

    /// The style of the pseudo-element `kind` of `el`, whose style is
    /// `style`: `None` if no rule matches it and it is not a marker or a
    /// placeholder (which always have a box), unless `always`.
    fn style_pseudo_element(
        &mut self,
        el: &DomElement<'_>,
        style: &Arc<ComputedStyle>,
        kind: PseudoKind,
        pseudo_layout_parent: Option<Display>,
        always: bool,
    ) -> Option<ComputedStyle> {
        let mut matched = std::mem::take(&mut self.matched);
        matched.clear();
        let mut scoped = std::mem::take(&mut self.scoped_matched);
        scoped.clear();
        if self.cx.stylist.has_pseudo_rules(kind) {
            self.cx.stylist.matching_rules(
                el,
                RuleTarget::Pseudo(kind),
                &self.media,
                &self.cx.layer_ranks,
                self.filter.as_ref(),
                self.scope == NodeId::DOCUMENT,
                &mut self.matching,
                &mut self.containers,
                &mut self.scratch,
                &mut matched,
            );
            if self.shadow_rules {
                self.match_scoped(el, RuleTarget::Pseudo(kind), &mut scoped);
            }
        }
        let own = if scoped.is_empty() {
            0
        } else {
            self.depth(self.scope)
        };
        let boxed = matches!(kind, PseudoKind::Marker | PseudoKind::Placeholder);
        // `::before` and `::after` have a box only with a `content` other
        // than `normal` and `none`, and `content: normal` is the value
        // without a matched declaration that sets `content`: no cascade
        // then (ADR 0007, "The `content` flag"). `getComputedStyle`
        // (`always`) needs the style in any case.
        let content_possible = always
            || !matches!(kind, PseudoKind::Before | PseudoKind::After)
            || matched
                .iter()
                .any(|&i| self.cx.stylist.rule(i).sets_content)
            || scoped
                .iter()
                .any(|m| self.cx.scoped_rule(m).is_some_and(|r| r.sets_content));
        let pseudo =
            if content_possible && (!matched.is_empty() || !scoped.is_empty() || boxed || always) {
                let rules = MatchedRules {
                    document: &matched,
                    scoped: &scoped,
                    own,
                };
                // `::before`, `::after` and `::marker` have animations
                // (ADR 0044 section 6); the style of a query (`always`)
                // only reads them.
                let owner = matches!(
                    kind,
                    PseudoKind::Before | PseudoKind::After | PseudoKind::Marker
                )
                .then_some(AnimationOwner {
                    node: el.node_id(),
                    pseudo: Some(kind),
                });
                let saved = self.animations;
                if always && let Some((cell, _)) = saved {
                    self.animations = Some((cell, false));
                }
                let fixup = if boxed {
                    animated::Fixup::None
                } else {
                    animated::Fixup::Parent(pseudo_layout_parent)
                };
                let mut pseudo = self.cascade_with_units(
                    &rules,
                    ExtraDeclarations::default(),
                    style,
                    Some(el.data()),
                    false,
                    owner.map(|o| (o, fixup)),
                );
                self.animations = saved;
                // Also the backdrop of an element outside the top layer, for
                // `getComputedStyle` (measured: `position: absolute` and
                // `display: block`, case `backdrop-popover`).
                if kind == PseudoKind::Backdrop {
                    top_layer_fixup(&mut pseudo, false);
                }
                if !boxed {
                    apply_fixups(&mut pseudo, false, pseudo_layout_parent);
                }
                Some(pseudo)
            } else {
                None
            };
        self.matched = matched;
        self.scoped_matched = scoped;
        pseudo
    }

    /// Styles the pseudo-elements of a `details` element: the default
    /// summary (if `default_summary`) and `::details-content`. They have
    /// no rules; their styles are set here.
    /// <https://html.spec.whatwg.org/multipage/rendering.html#the-details-and-summary-elements>
    fn style_details_pseudos(
        el: &DomElement<'_>,
        style: &Arc<ComputedStyle>,
        default_summary: bool,
        map: &mut StyleMap,
    ) {
        let data = el.data();
        let node = el.node_id();
        let open = data.has_attr("open");
        if default_summary {
            let mut summary = ComputedStyle::anonymous_from(style);
            summary.display = Display::ListItem;
            summary.list_style_type = if open {
                ListStyleType::DisclosureOpen
            } else {
                ListStyleType::DisclosureClosed
            };
            summary.list_style_position = ListStylePosition::Inside;
            // The label is English, as Chromium's with the default locale.
            summary.content = Content::Items(Arc::from([ContentItem::String("Details".into())]));
            map.set_pseudo(node, PseudoKind::DetailsSummary, Arc::new(summary));
            map.set_list_item_ordinal(node, 0);
        }
        let mut content = ComputedStyle::anonymous_from(style);
        content.display = Display::Block;
        content.contents_hidden = !open;
        map.set_pseudo(node, PseudoKind::DetailsContent, Arc::new(content));
    }
}

/// An element that the style walk will visit: the element, its parent's
/// style, the display of its parent box, its depth (the length of the path
/// of its ancestors) and its tree scope.
type Pending = (NodeId, Arc<ComputedStyle>, Option<Display>, usize, NodeId);

/// True if `display: contents` computes to `none` for the element (the
/// list in CSS Display 3 Appendix B).
/// <https://www.w3.org/TR/css-display-3/#unbox-html>
fn contents_is_none(data: &ElementData) -> bool {
    crate::element_kinds::is_svg_element(data)
        || data.is_html()
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

/// The cascade origin of a group of declarations, for `revert`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Level {
    /// Author rules, the `style` attribute and presentational hints.
    Author,
    /// The user-agent stylesheet.
    UserAgent,
}

/// Where a group of author declarations stands in the layer order of its
/// cascade context, for `revert-layer`: the context in the high 32 bits,
/// and in the low bits the presentational hints are 0 (below every layer),
/// the layers follow in their order from 1, the unlayered rules come after
/// the layers, and the `style` attribute last. A `revert-layer` at position
/// `p` drops the author declarations of its context at positions `p` and
/// above, whatever their importance (measured in Chromium 148,
/// `tools/probes/cascade-layers.json`, `rl-*` cases); each context has its
/// own layers.
type Position = u64;

/// The position of the presentational hints in their context.
const HINT_POSITION: u32 = 0;

/// The position of the `style` attribute in its context: above every layer.
const ATTRIBUTE_POSITION: u32 = u32::MAX - 1;

/// The position of the rules of the layer of rank `rank` in their context.
fn rule_position(rank: u32) -> u32 {
    rank.saturating_add(1)
}

/// The [`Position`] of `layer_position` in the cascade context `context`.
fn position(context: u32, layer_position: u32) -> Position {
    (u64::from(context) << 32) | u64::from(layer_position)
}

/// True if a declaration at `position` is dropped by a `revert-layer` that
/// won at `floor`: same context, same layer or a later one.
fn reverted_by_layer(position: Position, floor: Position) -> bool {
    context_of(position) == context_of(floor) && position >= floor
}

/// The cascade context of a [`Position`].
fn context_of(position: Position) -> u32 {
    // The high 32 bits; the shift leaves no more than 32 bits.
    u32::try_from(position >> 32).unwrap_or(u32::MAX)
}

/// The registered custom properties that are in a dependency cycle through
/// font-relative units with the winning `font-size` or `line-height`
/// ([`crate::font_cycle`]), and the properties that reference them: those
/// declarations are invalid at computed-value time (`unset`).
fn font_cycle_names(
    winners: &Winners<'_>,
    registry: &Registry,
    is_root: bool,
) -> (Vec<Arc<str>>, Vec<LonghandId>) {
    let mut names = Vec::new();
    let mut invalid = Vec::new();
    for (id, property) in [
        (LonghandId::FontSize, FontProperty::FontSize),
        (LonghandId::LineHeight, FontProperty::LineHeight),
    ] {
        if let Some(PropertyDeclaration::WithVariables(_, unparsed)) =
            winners.longhands[id as usize]
        {
            let cyclic = cycle_names(
                property,
                &unparsed.tokens,
                &winners.custom,
                registry,
                is_root,
            );
            if !cyclic.is_empty() {
                invalid.push(id);
                names.extend(cyclic);
            }
        }
    }
    (names, invalid)
}

/// The winning declaration of each property.
struct Winners<'d> {
    longhands: [Option<&'d PropertyDeclaration>; LonghandId::COUNT],
    /// The longhands that have a winner, one bit each (`LonghandId as
    /// usize`), so that `compute` visits only those, not all of them.
    set: LonghandSet,
    /// Properties for which an author-level `revert` won: all further
    /// author-level declarations are skipped, so the user-agent value (if
    /// any) wins. <https://www.w3.org/TR/css-cascade-5/#default>
    reverted: [bool; LonghandId::COUNT],
    /// Per property, the position of the lowest `revert-layer` that won in
    /// the first context that had one: author declarations of that context
    /// at this position and above are skipped ([`Position`]).
    /// `Position::MAX` if none won.
    layer_floor: [Position; LonghandId::COUNT],
    /// The floors of the other contexts, by property index and context: a
    /// `revert-layer` rolls back the layers of its own context only, so a
    /// floor of one context must not replace the floor of another
    /// (measured, `tools/probes/web-shadow-dom.json`, case
    /// `revert-layer-contexts`). Made at the first such floor: `None`
    /// without shadow trees.
    other_floors: Option<HashMap<(usize, u32), Position>>,
    custom: Vec<(&'d Arc<str>, &'d CustomDeclaration)>,
    custom_seen: HashSet<&'d str>,
    /// The same as `layer_floor` and `other_floors`, for the custom
    /// properties that a `revert-layer` rolled back, by name and context.
    custom_floor: HashMap<(&'d str, u32), Position>,
    /// The registered custom properties in a dependency cycle through
    /// font-relative units ([`crate::font_cycle`]); empty without
    /// `@property`.
    font_cycle: Vec<Arc<str>>,
}

impl<'d> Winners<'d> {
    fn new() -> Self {
        Winners {
            longhands: [None; LonghandId::COUNT],
            set: LonghandSet::new(),
            reverted: [false; LonghandId::COUNT],
            layer_floor: [Position::MAX; LonghandId::COUNT],
            other_floors: None,
            custom: Vec::new(),
            custom_seen: HashSet::new(),
            custom_floor: HashMap::new(),
            font_cycle: Vec::new(),
        }
    }

    /// True if a `revert-layer` of the context of `position` drops an author
    /// declaration of property `index` at `position`.
    fn layer_reverted(&self, index: usize, position: Position) -> bool {
        let floor = self.layer_floor[index];
        if floor == Position::MAX {
            // No floor in any context: `other_floors` gets one only after
            // `layer_floor` has one.
            return false;
        }
        if context_of(floor) == context_of(position) {
            return position >= floor;
        }
        self.other_floors
            .as_ref()
            .and_then(|floors| floors.get(&(index, context_of(position))))
            .is_some_and(|&f| reverted_by_layer(position, f))
    }

    /// Records a `revert-layer` of property `index` that won at `position`.
    fn set_layer_floor(&mut self, index: usize, position: Position) {
        let floor = &mut self.layer_floor[index];
        if *floor == Position::MAX || context_of(*floor) == context_of(position) {
            *floor = position.min(*floor);
        } else {
            let other = self
                .other_floors
                .get_or_insert_with(HashMap::new)
                .entry((index, context_of(position)))
                .or_insert(position);
            *other = position.min(*other);
        }
    }

    /// Adds declarations of lower precedence than all added so far. Later
    /// declarations in `list` take precedence over earlier ones. `position`
    /// is where the declarations stand in the layer order (author level
    /// only).
    fn add_all(&mut self, list: &'d [PropertyDeclaration], level: Level, position: Position) {
        for decl in list.iter().rev() {
            match decl {
                PropertyDeclaration::Custom(name, value) => {
                    self.add_custom(name, value, level, position);
                }
                other => {
                    let Some(id) = other.longhand() else {
                        continue;
                    };
                    let index = id as usize;
                    if self.longhands[index].is_some()
                        || (level == Level::Author
                            && (self.reverted[index] || self.layer_reverted(index, position)))
                    {
                        continue;
                    }
                    match other {
                        PropertyDeclaration::CssWide(_, CssWideKeyword::Revert)
                            if level == Level::Author =>
                        {
                            self.reverted[index] = true;
                        }
                        PropertyDeclaration::CssWide(_, CssWideKeyword::RevertLayer)
                            if level == Level::Author =>
                        {
                            self.set_layer_floor(index, position);
                        }
                        _ => {
                            self.longhands[index] = Some(other);
                            self.set.insert(id);
                        }
                    }
                }
            }
        }
    }

    fn add_custom(
        &mut self,
        name: &'d Arc<str>,
        value: &'d CustomDeclaration,
        level: Level,
        position: Position,
    ) {
        let key = (&**name, context_of(position));
        let floor = self.custom_floor.get(&key).copied();
        if self.custom_seen.contains(&**name)
            || (level == Level::Author && floor.is_some_and(|f| reverted_by_layer(position, f)))
        {
            return;
        }
        if level == Level::Author
            && matches!(
                value,
                CustomDeclaration::CssWide(CssWideKeyword::RevertLayer)
            )
        {
            let floor = self.custom_floor.entry(key).or_insert(position);
            *floor = position.min(*floor);
        } else {
            self.custom_seen.insert(name);
            self.custom.push((name, value));
        }
    }
}

/// The `!important` phase of the cascade of one element: the user-agent
/// and author declarations that win over everything normal. It runs first
/// ([`ImportantPhase::add_to`]); an element with animations runs it again
/// alone ([`ImportantPhase::collect`]) to find the winners that the
/// animations must not change (ADR 0044 section 3), so that other elements
/// keep no record of it.
struct ImportantPhase<'a, 'd> {
    stylist: &'d Stylist,
    ua: &'a [u32],
    author: &'a [AuthorRule<'d>],
    style_attribute: Option<&'d DeclarationBlock>,
    own: u32,
    attr_position: Position,
}

/// The longhands and custom properties whose winner is `!important`.
pub(crate) struct ImportantWinners<'d> {
    /// The longhands, one bit each (`LonghandId as usize`).
    pub(crate) longhands: LonghandSet,
    /// The custom properties.
    pub(crate) custom: Vec<&'d str>,
}

impl<'d> ImportantPhase<'_, 'd> {
    /// Adds the important declarations to `winners` (before all others).
    /// `!important`: the inner contexts first. Inside a context, the order
    /// of the layers is reversed: the rules are sorted by layer rank, so the
    /// layers run from the first to the last and, inside a layer, the rules
    /// from the last to the first. The unlayered rules, ranked last, are the
    /// weakest. The `style` attribute comes before the rules of its context.
    fn add_to(&self, winners: &mut Winners<'d>) {
        for &i in self.ua.iter().rev() {
            winners.add_all(&self.stylist.rule(i).block.important, Level::UserAgent, 0);
        }
        let mut attr = self.style_attribute;
        for context_run in self.author.chunk_by(|a, b| a.context == b.context).rev() {
            if context_run[0].context <= self.own
                && let Some(a) = attr.take()
            {
                winners.add_all(&a.important, Level::Author, self.attr_position);
            }
            for run in context_run.chunk_by(|a, b| a.rank == b.rank) {
                let position = position(run[0].context, rule_position(run[0].rank));
                for r in run.iter().rev() {
                    winners.add_all(&r.rule.block.important, Level::Author, position);
                }
            }
        }
        if let Some(a) = attr {
            winners.add_all(&a.important, Level::Author, self.attr_position);
        }
    }

    /// The important winners alone: what the first phase of the cascade
    /// decided, which the normal declarations cannot change.
    pub(crate) fn collect(&self) -> ImportantWinners<'d> {
        let mut winners = Winners::new();
        self.add_to(&mut winners);
        ImportantWinners {
            longhands: winners.set,
            custom: winners.custom.iter().map(|(name, _)| &***name).collect(),
        }
    }
}

/// An author rule in the cascade: its cascade context, the rank of its
/// layer, and the rule.
#[derive(Clone, Copy)]
struct AuthorRule<'r> {
    context: u32,
    rank: u32,
    rule: &'r Rule,
}

/// Runs the cascade for one element or pseudo-element and computes its
/// style.
///
/// The author rules come from several cascade contexts when shadow trees
/// are involved (CSS Cascade 5 §6.1; ADR 0036 section 5): for normal
/// declarations the outer context wins, for `!important` ones the inner
/// (measured in Chromium 148, `style-scoping`, `slotted-nested-cascade`,
/// `host-attribute-and-hints`, `part-depth`). The context step comes before
/// the `style` attribute (which belongs to the element's own context) and
/// before the layers. The presentational hints stay below all author rules
/// (measured: a host's `align` hint loses to `:host` in its shadow tree).
///
/// With an animation `target`, the style computed from the winners (the
/// base style) brings the target's animation records up to date, and the
/// style is computed again with the animated values of its animations
/// ([`animated::input`], ADR 0044 section 3).
#[allow(clippy::too_many_arguments)]
fn cascade(
    cx: &CascadeContext<'_>,
    matched: &MatchedRules<'_>,
    extra: ExtraDeclarations<'_>,
    parent: &ComputedStyle,
    element: Option<&ElementData>,
    is_root: bool,
    units: ContainerBasis,
    target: Option<&animated::AnimationTarget<'_>>,
) -> ComputedStyle {
    let stylist = cx.stylist;
    let document = matched.document;
    let origin = |i: &u32| stylist.rule(*i).origin;
    let ua_end = document
        .iter()
        .position(|i| origin(i) != CascadeOrigin::UserAgent)
        .unwrap_or(document.len());
    let author_start = document
        .iter()
        .position(|i| origin(i) == CascadeOrigin::Author)
        .unwrap_or(document.len());
    let ua = &document[..ua_end];
    let hint_rules = &document[ua_end..author_start];

    let ranks = &cx.layer_ranks;
    let rank_of = |r: &Rule| {
        debug_assert!(
            (r.layer as usize) < ranks.len(),
            "every rule's layer node has a rank"
        );
        ranks.get(r.layer as usize).copied().unwrap_or(0)
    };
    let own = matched.own;
    // Sorted by context, then layer rank, then specificity and order (the
    // document's rules are sorted already).
    let mut author: Vec<AuthorRule<'_>> =
        Vec::with_capacity(document.len() - author_start + matched.scoped.len());
    author.extend(document[author_start..].iter().map(|&i| {
        let rule = stylist.rule(i);
        AuthorRule {
            context: own,
            rank: rank_of(rule),
            rule,
        }
    }));
    if !matched.scoped.is_empty() {
        author.extend(matched.scoped.iter().filter_map(|m| {
            Some(AuthorRule {
                context: m.context,
                rank: m.rank,
                rule: cx.scoped_rule(m)?,
            })
        }));
        author.sort_by_key(|a| (a.context, a.rank, a.rule.sort_key));
    }
    // The `style` attribute stands above every layer of its context.
    let attr_position = position(own, ATTRIBUTE_POSITION);

    let important = ImportantPhase {
        stylist,
        ua,
        author: &author,
        style_attribute: extra.style_attribute,
        own,
        attr_position,
    };
    let mut winners = Winners::new();
    important.add_to(&mut winners);
    // Normal declarations: the outer contexts first; the `style` attribute
    // before the rules of its context.
    let mut attr = extra.style_attribute;
    for context_run in author.chunk_by(|a, b| a.context == b.context) {
        if context_run[0].context >= own
            && let Some(a) = attr.take()
        {
            winners.add_all(&a.normal, Level::Author, attr_position);
        }
        for r in context_run.iter().rev() {
            let position = position(r.context, rule_position(r.rank));
            winners.add_all(&r.rule.block.normal, Level::Author, position);
        }
    }
    if let Some(a) = attr {
        winners.add_all(&a.normal, Level::Author, attr_position);
    }
    winners.add_all(extra.hints, Level::Author, position(own, HINT_POSITION));
    for &i in hint_rules.iter().rev() {
        let hint = &stylist.rule(i).block.normal;
        winners.add_all(hint, Level::Author, position(own, HINT_POSITION));
    }
    for &i in ua.iter().rev() {
        winners.add_all(&stylist.rule(i).block.normal, Level::UserAgent, 0);
    }
    compute_with_animations(
        cx, winners, &important, parent, element, is_root, units, target,
    )
}

/// Makes the declarations that a font cycle makes invalid `unset`: both
/// properties inherit, and the style starts as the parent's.
fn drop_font_cycles(cx: &CascadeContext<'_>, winners: &mut Winners<'_>, is_root: bool) {
    if let Some(registry) = cx.registry.as_ref() {
        let (names, invalid) = font_cycle_names(winners, registry, is_root);
        for id in invalid {
            winners.longhands[id as usize] = None;
            winners.set.remove(id);
        }
        winners.font_cycle = names;
    }
}

/// The lengths that the values of an element with the style `parent` as
/// its parent's start from (its own font size and line height are not
/// known yet).
fn parent_lengths(
    cx: &CascadeContext<'_>,
    parent: &ComputedStyle,
    is_root: bool,
    units: ContainerBasis,
) -> LengthContext {
    let env = cx.env;
    LengthContext {
        font_size: parent.font_size,
        root_font_size: if is_root {
            env.root_font_size
        } else {
            cx.root_font_size
        },
        line_height: parent.line_height_px(),
        // The root element's own value is not known yet: `normal`.
        root_line_height: if is_root {
            LengthContext::normal_line_height(env.root_font_size)
        } else {
            cx.root_line_height
        },
        viewport_width: env.viewport_width,
        viewport_height: env.viewport_height,
        container: units,
    }
}

/// Computes a style from the winning declarations, and with `input` the
/// animated values of the element's animations at the place of their
/// properties in the order (ADR 0044 section 3): the custom properties
/// first, then `font-family`, `font-size` and `line-height`, then the other
/// longhands in the order of `LonghandId::ALL`.
fn compute(
    cx: &CascadeContext<'_>,
    winners: &Winners<'_>,
    parent: &ComputedStyle,
    element: Option<&ElementData>,
    is_root: bool,
    units: ContainerBasis,
    input: Option<&animated::AnimationInput>,
) -> ComputedStyle {
    let mut style = ComputedStyle::inherit_from(parent);
    let lengths = parent_lengths(cx, parent, is_root, units);
    let registry = cx.registry.as_ref();
    // The animated custom properties replace their winners (ADR 0044
    // section 3): `var()` in other declarations sees the animated values.
    let animated_custom = input
        .map(|input| {
            let registered = registry.map(|registry| Registered {
                registry,
                lengths: &lengths,
                font_cycle: &winners.font_cycle,
            });
            animated::custom_values(input, registered)
        })
        .unwrap_or_default();
    let declared_custom = animated::with_custom_values(&winners.custom, &animated_custom);
    let declared_custom: &[(&Arc<str>, &CustomDeclaration)] = &declared_custom;
    style.custom_properties = compute_custom_properties(
        parent.custom_properties.as_ref(),
        declared_custom,
        registry.map(|registry| Registered {
            registry,
            lengths: &lengths,
            font_cycle: &winners.font_cycle,
        }),
        &cx.custom_pool,
    );
    let mut ccx = ComputeContext {
        parent,
        lengths,
        element,
        quirks: cx.quirks,
    };
    let mut resolved = ResolvedVariables::default();
    let mut keyframe_resolved = ResolvedVariables::default();
    let mut apply_winner = |id: LonghandId, ccx: &ComputeContext<'_>, style: &mut ComputedStyle| {
        if let Some(decl) = winners.longhands[id as usize] {
            apply_declaration(id, decl, ccx, style, &cx.initial, &mut resolved);
        }
        if let Some(input) = input {
            animated::animate_longhand(cx, input, id, ccx, style, &mut keyframe_resolved);
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
    // Registered lengths in `em` and `rem` are computed against the font
    // size of the element, which is known only now.
    let recompute = |style: &mut ComputedStyle, lengths: &LengthContext| {
        if let Some(registry) = registry
            && registry.declares_any(declared_custom.iter().map(|(name, _)| *name))
        {
            style.custom_properties = compute_custom_properties(
                parent.custom_properties.as_ref(),
                declared_custom,
                Some(Registered {
                    registry,
                    lengths,
                    font_cycle: &winners.font_cycle,
                }),
                &cx.custom_pool,
            );
        }
    };
    if ccx.lengths.font_size != lengths.font_size
        || ccx.lengths.root_font_size != lengths.root_font_size
    {
        recompute(&mut style, &ccx.lengths);
    }
    // `lh` is the element's own line height, except in `line-height` and
    // `font-size`: compute `line-height` (with the parent's) first.
    apply_winner(LonghandId::LineHeight, &ccx, &mut style);
    ccx.lengths.line_height = style.line_height_px();
    let customs = parent.custom_properties.as_deref();
    if ccx.lengths.line_height != lengths.line_height
        && crate::font_cycle::depends_on_line_height(declared_custom, customs)
    {
        recompute(&mut style, &ccx.lengths);
    }
    // The other longhands that have a winner or an animated value, in the
    // order of `ALL`.
    let mut set = winners.set;
    if let Some(input) = input {
        set.union_with(&input.set);
    }
    for id in set.iter() {
        if !matches!(
            id,
            LonghandId::FontFamily | LonghandId::FontSize | LonghandId::LineHeight
        ) {
            apply_winner(id, &ccx, &mut style);
        }
    }
    finish_computed(cx, parent, &mut style);
    style
}

/// The last steps of [`compute`]: `user-select: auto` takes `none` or `all`
/// from the parent (the specification defines this for the used value;
/// Chromium and swb store it in the computed value,
/// <https://www.w3.org/TR/css-ui-4/#propdef-user-select>), and
/// `--animations off` resets the animation properties.
fn finish_computed(cx: &CascadeContext<'_>, parent: &ComputedStyle, style: &mut ComputedStyle) {
    if style.user_select == UserSelect::Auto
        && matches!(parent.user_select, UserSelect::None | UserSelect::All)
    {
        style.user_select = parent.user_select;
    }
    if !cx.stylist.animations_enabled() {
        animated::reset_animation_properties(style, &cx.initial);
    }
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
        custom: Option<&CustomProperties>,
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
    // `revert` and `revert-layer` reach this point only from the user-agent
    // origin or from `var()` substitution; there they behave as `unset`.
    let inherit = match keyword {
        CssWideKeyword::Initial => false,
        CssWideKeyword::Inherit => true,
        CssWideKeyword::Unset | CssWideKeyword::Revert | CssWideKeyword::RevertLayer => {
            id.is_inherited()
        }
    };
    if inherit && !id.is_inherited() {
        style.inherits_reset = true;
    }
    id.copy_value(if inherit { parent } else { initial }, style);
}

/// Chromium's monospace font size quirk: when the font family changes
/// between the single generic `monospace` and anything else, a font size
/// that came from a keyword is recomputed for the new default size (13px
/// for monospace, 16px otherwise), and a size relative to a keyword size
/// is scaled by 13/16 (or 16/13). Absolute sizes do not change. This
/// follows Blink's `FontBuilder::CheckForGenericFamilyChange`.
fn adjust_font_size_for_family(style: &mut ComputedStyle, parent: &ComputedStyle, quirks: bool) {
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

/// `style` as an `Arc`, shared with `last` if equal to it. Elements in the
/// top layer are not in the sharing cache (their parent styles differ in
/// a chain of nested popovers although the values are equal), so equal
/// neighbours in the layer share here: 10,000 nested popovers hold one
/// style of their own and one of their backdrop, not 10,000 of each.
fn share_last(last: &mut Option<Arc<ComputedStyle>>, style: ComputedStyle) -> Arc<ComputedStyle> {
    if let Some(last) = last
        && **last == style
    {
        return Arc::clone(last);
    }
    let style = Arc::new(style);
    *last = Some(Arc::clone(&style));
    style
}

/// The fix-up of an element in the top layer and of every `::backdrop` (CSS
/// Position 4 §3.1, <https://drafts.csswg.org/css-position-4/#top-styling>):
/// a `position` other than `absolute` and `fixed` computes to `absolute`
/// (measured in Chromium 148 for `static` and `relative`, case
/// `popover-author-position`). `display: contents` computes to `block` for
/// the element (measured in Chromium 148, case `contents`: computed
/// `block`, one client rect, children laid out in it). [`apply_fixups`] then
/// blockifies the box.
/// An `element` in the top layer also gets `overlay: auto`, the value of
/// the user agent's `:popover-open, dialog:modal { overlay: auto
/// !important }` (HTML §15.3; measured: `popover-open-ua`), set here so
/// that no rule is tested against every element.
fn top_layer_fixup(style: &mut ComputedStyle, element: bool) {
    if !style.position.is_absolutely_positioned() {
        style.position = crate::values::Position::Absolute;
    }
    if element {
        if style.display == Display::Contents {
            style.display = Display::Block;
        }
        style.overlay = crate::values::Overlay::Auto;
    }
}

/// Computed-value fixups that depend on several properties and on the
/// parent box.
fn apply_fixups(style: &mut ComputedStyle, is_root: bool, layout_parent: Option<Display>) {
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
    style.original_display = style.display;
    style.flex_or_grid_item = in_flex_or_grid;
    // `float` does not apply to flex and grid items (CSS Flexbox 1 §4,
    // CSS Grid 2 §6.1); they stay in flow.
    if in_flex_or_grid {
        style.float = Float::None;
    }
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
