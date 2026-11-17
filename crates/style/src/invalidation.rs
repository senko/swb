//! Which elements a change of the element states or of the DOM can restyle
//! (ADR 0041, ADR 0042).
//!
//! [`InvalidationIndex`] holds the dependencies of the stylist's selectors
//! ([`swb_css::Selector::for_each_dependency`]): per state flag, class name, ID,
//! attribute name, `:empty`, the structural pseudo-classes and the presence
//! of compounds, indexed as the rule buckets index rules (by the key that
//! the element where the change happened must have). [`Planner`] turns the
//! changes into the elements to restyle ([`Plan`]): some marked one by one,
//! others as the descendants of a root that have one of a set of subject
//! keys. The marking of DOM changes is in `dom_changes.rs`; the restyle
//! itself is in `cascade/targeted.rs`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use swb_css::{BucketKey, DependencyKey, ElementState, Selector, StateReach};
use swb_dom::{Document, ElementData, NodeId};

use crate::element::{CONTROL_STATES, ElementStates};

/// The state flags whose changes the targeted restyle handles: the five
/// interactive ones, `:target` and the states of form controls.
pub(crate) const TARGETED_STATES: ElementState = ElementState::HOVER
    .union(ElementState::ACTIVE)
    .union(ElementState::FOCUS)
    .union(ElementState::FOCUS_VISIBLE)
    .union(ElementState::FOCUS_WITHIN)
    .union(ElementState::TARGET)
    .union(CONTROL_STATES);

/// The interactive states whose chains follow the tree (ADR 0041 section
/// 1): a DOM change can move the elements of their chains.
pub(crate) const CHAIN_STATES: ElementState = ElementState::HOVER
    .union(ElementState::ACTIVE)
    .union(ElementState::FOCUS)
    .union(ElementState::FOCUS_VISIBLE)
    .union(ElementState::FOCUS_WITHIN);

/// The states that HTML derives from an element's attributes (and from
/// those of its ancestors, the `fieldset`, `optgroup` and editing host):
/// every attribute change of an element flips them on it (ADR 0042 section
/// 2). `:focus-visible` is here because the type of a field and the editing
/// host decide it for the focused element.
pub(crate) const DOM_STATES: ElementState = ElementState::DISABLED
    .union(ElementState::ENABLED)
    .union(ElementState::CHECKED)
    .union(ElementState::DEFAULT)
    .union(ElementState::INDETERMINATE)
    .union(ElementState::REQUIRED)
    .union(ElementState::OPTIONAL)
    .union(ElementState::READ_ONLY)
    .union(ElementState::READ_WRITE)
    .union(ElementState::PLACEHOLDER_SHOWN)
    .union(ElementState::VALID)
    .union(ElementState::INVALID)
    .union(ElementState::USER_VALID)
    .union(ElementState::USER_INVALID)
    .union(ElementState::IN_RANGE)
    .union(ElementState::OUT_OF_RANGE)
    .union(ElementState::OPEN)
    .union(ElementState::FOCUS_VISIBLE);

/// A subject or element key, owned.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Key {
    Id(Box<str>),
    Class(Box<str>),
    Name { name: Box<str>, lower: Box<str> },
    Any,
}

impl Key {
    fn of(key: BucketKey<'_>) -> Key {
        match key {
            BucketKey::Id(id) => Key::Id(id.into()),
            BucketKey::Class(class) => Key::Class(class.into()),
            BucketKey::LocalName { name, lower_name } => Key::Name {
                name: name.into(),
                lower: lower_name.into(),
            },
            BucketKey::Universal => Key::Any,
        }
    }

    /// The key of an HTML element name (already lowercase).
    pub(crate) fn html_name(name: &str) -> Key {
        Key::Name {
            name: name.into(),
            lower: name.into(),
        }
    }
}

/// One dependency: a reach (one flag), the subject's key, and the `:has()`
/// anchors (their kind and keys) if the reach starts at them.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Entry {
    reach: StateReach,
    /// The subject key (shared by the entries of one selector).
    subject: Arc<Key>,
    anchor: Option<(StateReach, Box<[Key]>)>,
}

/// The keys of some elements (ADR 0042 section 2: the elements of a
/// removed or inserted subtree), for lookups by element key.
#[derive(Clone, Debug, Default)]
pub(crate) struct ElementKeys {
    ids: HashSet<Box<str>>,
    classes: HashSet<Box<str>>,
    html_names: HashSet<Box<str>>,
    other_names: HashSet<Box<str>>,
}

impl ElementKeys {
    /// Adds the keys of an element.
    pub(crate) fn add_element(&mut self, data: &ElementData) {
        if let Some(id) = data.id() {
            self.add_id(id);
        }
        for class in data.classes() {
            self.add_class(class);
        }
        let names = if data.is_html() {
            &mut self.html_names
        } else {
            &mut self.other_names
        };
        if !names.contains(&**data.local_name()) {
            names.insert((&**data.local_name()).into());
        }
    }

    /// Adds an ID.
    pub(crate) fn add_id(&mut self, id: &str) {
        if !self.ids.contains(id) {
            self.ids.insert(id.into());
        }
    }

    /// Adds a class.
    pub(crate) fn add_class(&mut self, class: &str) {
        if !self.classes.contains(class) {
            self.classes.insert(class.into());
        }
    }

    /// True if no key was added.
    pub(crate) fn is_empty(&self) -> bool {
        self.ids.is_empty()
            && self.classes.is_empty()
            && self.html_names.is_empty()
            && self.other_names.is_empty()
    }
}

/// The entries of one dependency, bucketed by the key that the element
/// where the change happened must have (as `RuleMap` buckets rules).
#[derive(Debug, Default)]
struct ReachMap {
    ids: HashMap<Box<str>, Vec<u32>>,
    classes: HashMap<Box<str>, Vec<u32>>,
    /// By lowercase local name, for all elements: a superset for the names
    /// of other namespaces, which match case-sensitively.
    names: HashMap<Box<str>, Vec<u32>>,
    universal: Vec<u32>,
}

/// The local name of an element in lowercase (no copy for HTML elements
/// and lowercase names).
fn lower_name(data: &ElementData) -> std::borrow::Cow<'_, str> {
    let name: &str = data.local_name();
    if data.is_html() || !name.bytes().any(|b| b.is_ascii_uppercase()) {
        std::borrow::Cow::Borrowed(name)
    } else {
        std::borrow::Cow::Owned(name.to_ascii_lowercase())
    }
}

/// Folds `s` to lowercase in quirks mode.
fn fold(s: &str, quirks: bool) -> Box<str> {
    if quirks {
        s.to_ascii_lowercase().into_boxed_str()
    } else {
        s.into()
    }
}

/// Appends `entry` to the list of `key` in `map`, allocating the key only
/// for a new list.
fn push_to(map: &mut HashMap<Box<str>, Vec<u32>>, key: &str, entry: u32) {
    match map.get_mut(key) {
        Some(list) => list.push(entry),
        None => {
            map.insert(key.into(), vec![entry]);
        }
    }
}

/// [`push_to`] with the key folded to lowercase in quirks mode.
fn push_folded(map: &mut HashMap<Box<str>, Vec<u32>>, key: &str, entry: u32, quirks: bool) {
    if quirks && key.bytes().any(|b| b.is_ascii_uppercase()) {
        push_to(map, &key.to_ascii_lowercase(), entry);
    } else {
        push_to(map, key, entry);
    }
}

impl ReachMap {
    fn insert(&mut self, key: BucketKey<'_>, entry: u32, quirks: bool) {
        match key {
            BucketKey::Id(id) => push_folded(&mut self.ids, id, entry, quirks),
            BucketKey::Class(c) => push_folded(&mut self.classes, c, entry, quirks),
            BucketKey::LocalName { lower_name, .. } => push_to(&mut self.names, lower_name, entry),
            BucketKey::Universal => self.universal.push(entry),
        }
    }

    fn is_empty(&self) -> bool {
        self.ids.is_empty()
            && self.classes.is_empty()
            && self.names.is_empty()
            && self.universal.is_empty()
    }

    /// Sorts and deduplicates every bucket.
    fn finish(&mut self) {
        for map in [&mut self.ids, &mut self.classes, &mut self.names] {
            for list in map.values_mut() {
                list.sort_unstable();
                list.dedup();
            }
        }
        self.universal.sort_unstable();
        self.universal.dedup();
    }

    /// Appends the entries for an element with `data`.
    fn candidates(&self, data: &ElementData, quirks: bool, out: &mut Vec<u32>) {
        let lower = |s: &str| s.to_ascii_lowercase();
        if !self.ids.is_empty()
            && let Some(id) = data.id()
        {
            let found = if quirks {
                self.ids.get(lower(id).as_str())
            } else {
                self.ids.get(id)
            };
            out.extend(found.into_iter().flatten());
        }
        if !self.classes.is_empty() {
            for class in data.classes() {
                let found = if quirks {
                    self.classes.get(lower(class).as_str())
                } else {
                    self.classes.get(class)
                };
                out.extend(found.into_iter().flatten());
            }
        }
        if !self.names.is_empty() {
            out.extend(self.names.get(&*lower_name(data)).into_iter().flatten());
        }
        out.extend(&self.universal);
    }

    /// Appends the entries for an element with one of `keys`.
    fn candidates_of_keys(&self, keys: &ElementKeys, quirks: bool, out: &mut Vec<u32>) {
        let get = |map: &HashMap<Box<str>, Vec<u32>>, s: &str, fold: bool| -> Vec<u32> {
            let found = if fold {
                map.get(s.to_ascii_lowercase().as_str())
            } else {
                map.get(s)
            };
            found.cloned().unwrap_or_default()
        };
        for id in &keys.ids {
            out.extend(get(&self.ids, id, quirks));
        }
        for class in &keys.classes {
            out.extend(get(&self.classes, class, quirks));
        }
        for name in &keys.html_names {
            out.extend(get(&self.names, name, false));
        }
        for name in &keys.other_names {
            out.extend(get(&self.names, name, true));
        }
        out.extend(&self.universal);
    }

    /// Appends all entries.
    fn all(&self, out: &mut Vec<u32>) {
        for map in [&self.ids, &self.classes, &self.names] {
            out.extend(map.values().flatten());
        }
        out.extend(&self.universal);
    }
}

/// The dependencies of a stylist's selectors (ADR 0041 section 1, ADR 0042
/// section 1).
#[derive(Debug, Default)]
pub(crate) struct InvalidationIndex {
    entries: Vec<Entry>,
    /// By state flag (the flag's bits).
    states: HashMap<u32, ReachMap>,
    /// By class name and ID (folded in quirks mode): the element has (or
    /// had) the name, so no element key.
    classes: HashMap<Box<str>, Vec<u32>>,
    ids: HashMap<Box<str>, Vec<u32>>,
    /// By lowercase attribute name: a superset for the elements of other
    /// namespaces, whose attribute names match case-sensitively.
    attributes: HashMap<Box<str>, ReachMap>,
    empty: ReachMap,
    structural: ReachMap,
    /// The presence entries that reach outside the inserted or removed
    /// subtree: sibling positions, `:has()` anchors and everything.
    presence: ReachMap,
    /// True if a presence entry has `:has()` anchors.
    presence_anchors: bool,
}

impl InvalidationIndex {
    /// Builds the index of `selectors`.
    pub(crate) fn new<'a>(selectors: impl Iterator<Item = &'a Selector>, quirks: bool) -> Self {
        let mut index = InvalidationIndex::default();
        for selector in selectors {
            let subject = Arc::new(Key::of(selector.bucket_key()));
            // The entries without anchors of this selector differ only in
            // their reach: each is looked up once.
            let mut plain: [Option<u32>; 9] = [None; 9];
            selector.for_each_dependency(|dependency| {
                let anchored = dependency.anchor.is_some();
                let slot = (!anchored)
                    .then(|| dependency.reach.bits().trailing_zeros() as usize)
                    .filter(|&i| i < plain.len());
                let cached = slot.and_then(|i| plain.get(i).copied().flatten());
                let id = if let Some(id) = cached {
                    id
                } else {
                    let entry = Entry {
                        reach: dependency.reach,
                        subject: Arc::clone(&subject),
                        anchor: dependency
                            .anchor
                            .map(|(kind, keys)| (kind, keys.iter().map(|&k| Key::of(k)).collect())),
                    };
                    let id = u32::try_from(index.entries.len()).unwrap_or(u32::MAX);
                    index.entries.push(entry);
                    if let Some(cell) = slot.and_then(|i| plain.get_mut(i)) {
                        *cell = Some(id);
                    }
                    id
                };
                let bucket = |map: &mut ReachMap| {
                    if dependency.element_keys.is_empty() {
                        map.insert(BucketKey::Universal, id, quirks);
                    }
                    for &key in dependency.element_keys {
                        map.insert(key, id, quirks);
                    }
                };
                match dependency.key {
                    DependencyKey::State(flags) => {
                        for flag in flags.iter() {
                            bucket(index.states.entry(flag.bits()).or_default());
                        }
                    }
                    DependencyKey::Class(c) => push_folded(&mut index.classes, c, id, quirks),
                    DependencyKey::Id(i) => push_folded(&mut index.ids, i, id, quirks),
                    DependencyKey::Attr { lower, .. } => {
                        if let Some(map) = index.attributes.get_mut(lower) {
                            bucket(map);
                        } else {
                            let mut map = ReachMap::default();
                            bucket(&mut map);
                            index.attributes.insert(lower.into(), map);
                        }
                    }
                    DependencyKey::Empty => bucket(&mut index.empty),
                    DependencyKey::Structural => bucket(&mut index.structural),
                    DependencyKey::Presence => {
                        index.presence_anchors |= anchored;
                        bucket(&mut index.presence);
                    }
                }
            });
        }
        for map in index
            .states
            .values_mut()
            .chain(index.attributes.values_mut())
            .chain([&mut index.empty, &mut index.structural, &mut index.presence])
        {
            map.finish();
        }
        for list in index.classes.values_mut().chain(index.ids.values_mut()) {
            list.sort_unstable();
            list.dedup();
        }
        index
    }

    /// The number of distinct entries.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// The entries of the state flags `flags` for an element with `data`.
    pub(crate) fn states_of(
        &self,
        data: &ElementData,
        flags: ElementState,
        quirks: bool,
        out: &mut Vec<u32>,
    ) {
        for flag in flags.iter() {
            if let Some(map) = self.states.get(&flag.bits()) {
                map.candidates(data, quirks, out);
            }
        }
    }

    /// All entries of the state flags `flags`, for any element.
    pub(crate) fn all_states(&self, flags: ElementState, out: &mut Vec<u32>) {
        for flag in flags.iter() {
            if let Some(map) = self.states.get(&flag.bits()) {
                map.all(out);
            }
        }
    }

    /// The entries of the class `name`.
    pub(crate) fn class(&self, name: &str, quirks: bool, out: &mut Vec<u32>) {
        if let Some(list) = self.classes.get(&*fold(name, quirks)) {
            out.extend(list);
        }
    }

    /// The entries of the ID `id`.
    pub(crate) fn id(&self, id: &str, quirks: bool, out: &mut Vec<u32>) {
        if let Some(list) = self.ids.get(&*fold(id, quirks)) {
            out.extend(list);
        }
    }

    /// The entries of the attribute `name` (its local name) for an element
    /// with `data`.
    pub(crate) fn attribute(
        &self,
        data: &ElementData,
        name: &str,
        quirks: bool,
        out: &mut Vec<u32>,
    ) {
        let lower = if name.bytes().any(|b| b.is_ascii_uppercase()) {
            std::borrow::Cow::Owned(name.to_ascii_lowercase())
        } else {
            std::borrow::Cow::Borrowed(name)
        };
        if let Some(map) = self.attributes.get(&*lower) {
            map.candidates(data, quirks, out);
        }
    }

    /// The `:empty` entries for an element with `data`.
    pub(crate) fn empty(&self, data: &ElementData, quirks: bool, out: &mut Vec<u32>) {
        self.empty.candidates(data, quirks, out);
    }

    /// True if some selector has a structural pseudo-class.
    pub(crate) fn has_structural(&self) -> bool {
        !self.structural.is_empty()
    }

    /// The structural entries for an element with `data`.
    pub(crate) fn structural(&self, data: &ElementData, quirks: bool, out: &mut Vec<u32>) {
        self.structural.candidates(data, quirks, out);
    }

    /// The presence entries for an inserted or removed element with `data`.
    pub(crate) fn presence(&self, data: &ElementData, quirks: bool, out: &mut Vec<u32>) {
        self.presence.candidates(data, quirks, out);
    }

    /// The presence entries for elements with one of `keys`.
    pub(crate) fn presence_of_keys(&self, keys: &ElementKeys, quirks: bool, out: &mut Vec<u32>) {
        self.presence.candidates_of_keys(keys, quirks, out);
    }

    /// True if a presence entry has `:has()` anchors: then the keys of the
    /// whole inserted subtree matter.
    pub(crate) fn has_presence_anchors(&self) -> bool {
        self.presence_anchors
    }

    /// True if the presence map has entries.
    pub(crate) fn has_presence(&self) -> bool {
        !self.presence.is_empty()
    }
}

/// A set of subject keys: an element matches if it has one of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct KeySet {
    any: bool,
    ids: HashSet<Box<str>>,
    classes: HashSet<Box<str>>,
    html_names: HashSet<Box<str>>,
    other_names: HashSet<Box<str>>,
}

impl KeySet {
    pub(crate) fn is_empty(&self) -> bool {
        !self.any
            && self.ids.is_empty()
            && self.classes.is_empty()
            && self.html_names.is_empty()
            && self.other_names.is_empty()
    }

    /// Adds a key (IDs and classes folded to lowercase in quirks mode).
    pub(crate) fn add(&mut self, key: &Key, quirks: bool) {
        match key {
            Key::Id(id) => {
                self.ids.insert(fold(id, quirks));
            }
            Key::Class(c) => {
                self.classes.insert(fold(c, quirks));
            }
            Key::Name { name, lower } => {
                self.html_names.insert(lower.clone());
                self.other_names.insert(name.clone());
            }
            Key::Any => self.any = true,
        }
    }

    /// A set that matches every element.
    pub(crate) fn any() -> KeySet {
        KeySet {
            any: true,
            ..KeySet::default()
        }
    }

    /// The number of keys.
    pub(crate) fn len(&self) -> usize {
        usize::from(self.any)
            + self.ids.len()
            + self.classes.len()
            + self.html_names.len()
            + self.other_names.len()
    }

    /// True if `self` matches every element that `other` matches: it has
    /// every key of `other`, or it matches any element.
    pub(crate) fn contains_all(&self, other: &KeySet) -> bool {
        if self.any {
            return true;
        }
        !other.any
            && other.ids.is_subset(&self.ids)
            && other.classes.is_subset(&self.classes)
            && other.html_names.is_subset(&self.html_names)
            && other.other_names.is_subset(&self.other_names)
    }

    /// Adds the keys of `other`.
    pub(crate) fn merge(&mut self, other: &KeySet) {
        if self.any {
            return;
        }
        self.any |= other.any;
        self.ids.extend(other.ids.iter().cloned());
        self.classes.extend(other.classes.iter().cloned());
        self.html_names.extend(other.html_names.iter().cloned());
        self.other_names.extend(other.other_names.iter().cloned());
    }

    /// True if an element with `data` has one of the keys.
    pub(crate) fn matches(&self, data: &ElementData, quirks: bool) -> bool {
        if self.any {
            return true;
        }
        let has = |set: &HashSet<Box<str>>, value: &str| {
            !set.is_empty()
                && if quirks {
                    set.contains(value.to_ascii_lowercase().as_str())
                } else {
                    set.contains(value)
                }
        };
        if data.id().is_some_and(|id| has(&self.ids, id)) {
            return true;
        }
        if !self.classes.is_empty() && data.classes().any(|c| has(&self.classes, c)) {
            return true;
        }
        let names = if data.is_html() {
            &self.html_names
        } else {
            &self.other_names
        };
        names.contains(&**data.local_name())
    }
}

/// What a change restyles (ADR 0041 section 2, "Marking"; ADR 0042 section
/// 2).
#[derive(Debug, Default)]
pub(crate) struct Plan {
    /// Elements to restyle.
    pub(crate) marked: HashSet<NodeId>,
    /// Roots whose descendants with one of the keys are restyled.
    pub(crate) roots: HashMap<NodeId, KeySet>,
    /// The flat ancestors of the marked elements and of the roots: the
    /// walk enters them.
    pub(crate) below: HashSet<NodeId>,
    /// The roots of inserted subtrees (also roots in `roots`).
    pub(crate) inserted: HashSet<NodeId>,
    /// A dependency that the analysis does not handle: restyle all.
    pub(crate) everything: bool,
    /// The marking did more work than a full restyle would.
    pub(crate) too_large: bool,
    /// An attribute that the counter pass reads changed.
    pub(crate) counter_inputs: bool,
    /// The instance trees of `use` elements may change (an ID, a reference
    /// or the tree changed in a document with `use` elements).
    pub(crate) uses: bool,
}

impl Plan {
    /// True if nothing is restyled.
    pub(crate) fn is_empty(&self) -> bool {
        self.marked.is_empty() && self.roots.is_empty() && !self.everything && !self.too_large
    }

    /// Marks the flat ancestors of the marked elements and of the roots:
    /// each ancestor once.
    fn mark_ancestors(&mut self, doc: &Document) {
        let starts: Vec<NodeId> = self
            .marked
            .iter()
            .chain(self.roots.keys())
            .copied()
            .collect();
        for start in starts {
            let mut current = doc.flat_parent(start);
            while let Some(node) = current {
                if !self.below.insert(node) {
                    break;
                }
                current = doc.flat_parent(node);
            }
        }
    }
}

/// The element and its flat ancestor elements, as `StateSet` makes the
/// hover, active and focus chains.
fn chain(doc: &Document, node: Option<NodeId>) -> HashSet<NodeId> {
    let Some(node) = node.filter(|&n| doc.get(n).is_some()) else {
        return HashSet::new();
    };
    std::iter::once(node)
        .chain(doc.flat_ancestors(node))
        .filter(|&n| doc.element(n).is_some())
        .collect()
}

/// The hosts whose shadow trees hold `focus` (they match `:focus`).
fn focus_hosts(doc: &Document, focus: Option<NodeId>) -> HashSet<NodeId> {
    let mut hosts = HashSet::new();
    let Some(mut at) = focus.filter(|&n| doc.get(n).is_some()) else {
        return hosts;
    };
    while let Some(host) = doc.shadow_host(doc.root(at)) {
        if !hosts.insert(host) {
            break;
        }
        at = host;
    }
    hosts
}

/// The control states that differ between two maps, per element.
fn control_flips(
    a: &HashMap<NodeId, ElementState>,
    b: &HashMap<NodeId, ElementState>,
    add: &mut dyn FnMut(NodeId, ElementState),
) {
    for (&node, &state) in a {
        let flips = match b.get(&node) {
            Some(&other) => (state ^ other) & CONTROL_STATES,
            None => CONTROL_STATES,
        };
        if !flips.is_empty() {
            add(node, flips);
        }
    }
    for &node in b.keys() {
        if !a.contains_key(&node) {
            add(node, CONTROL_STATES);
        }
    }
}

/// The elements whose flags among `changed` (a subset of
/// [`TARGETED_STATES`]) differ between `old` and `new`, with those flags
/// (ADR 0041 section 1, "Terms"). The sets may be larger than needed.
pub(crate) fn flipped(
    doc: &Document,
    old: &ElementStates,
    new: &ElementStates,
    changed: ElementState,
) -> Vec<(NodeId, ElementState)> {
    let mut flips: HashMap<NodeId, ElementState> = HashMap::new();
    let mut add = |node: NodeId, flag: ElementState| {
        *flips.entry(node).or_default() |= flag;
    };
    let pairs = [
        (old.hover, new.hover, ElementState::HOVER),
        (old.active, new.active, ElementState::ACTIVE),
        (old.focus, new.focus, ElementState::FOCUS_WITHIN),
    ];
    for (a, b, flag) in pairs {
        if changed.intersects(flag) && a != b {
            let (a, b) = (chain(doc, a), chain(doc, b));
            for &node in a.symmetric_difference(&b) {
                add(node, flag);
            }
        }
    }
    if changed.intersects(ElementState::FOCUS) && old.focus != new.focus {
        for node in [old.focus, new.focus].into_iter().flatten() {
            add(node, ElementState::FOCUS);
        }
        let (a, b) = (focus_hosts(doc, old.focus), focus_hosts(doc, new.focus));
        for &node in a.symmetric_difference(&b) {
            add(node, ElementState::FOCUS);
        }
    }
    if changed.intersects(ElementState::FOCUS_VISIBLE)
        && (old.focus != new.focus || old.focus_visible != new.focus_visible)
    {
        for node in [old.focus, new.focus].into_iter().flatten() {
            add(node, ElementState::FOCUS_VISIBLE);
        }
    }
    if changed.intersects(ElementState::TARGET) && old.target != new.target {
        for node in [old.target, new.target].into_iter().flatten() {
            add(node, ElementState::TARGET);
        }
    }
    if changed.intersects(CONTROL_STATES) && !Arc::ptr_eq(&old.controls, &new.controls) {
        control_flips(&old.controls, &new.controls, &mut add);
    }
    let mut out: Vec<(NodeId, ElementState)> = flips
        .into_iter()
        .filter(|(node, _)| doc.element(*node).is_some())
        .map(|(node, flags)| (node, flags & changed))
        .filter(|(_, flags)| !flags.is_empty())
        .collect();
    out.sort_unstable_by_key(|(node, _)| *node);
    out
}

/// The position reaches, in the order of the slots of [`Planner`].
const POSITIONS: [StateReach; 5] = [
    StateReach::SELF,
    StateReach::CHILDREN,
    StateReach::DESCENDANTS,
    StateReach::FOLLOWING_SIBLINGS,
    StateReach::FOLLOWING_SIBLING_DESCENDANTS,
];

/// The `:has()` anchor kinds, in the order of the slots of [`Planner`].
const ANCHOR_KINDS: [StateReach; 3] = [
    StateReach::HAS_ANCESTORS,
    StateReach::HAS_PRECEDING_SIBLINGS,
    StateReach::HAS_PRECEDING_SIBLINGS_OF_ANCESTORS,
];

/// How a change relates to the element that [`Planner::apply`] gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum At {
    /// The change happened on the element.
    Element,
    /// The change happened on the element and on all its descendants (an
    /// attribute that the states of the subtree read).
    Subtree,
    /// The changed element was a child of this element and is gone (a
    /// removal).
    RemovedChild,
}

/// The anchors of one kind: the starts, and per position the keys of the
/// anchors and of the subjects (of all entries together: a superset).
#[derive(Debug, Default)]
struct Anchors {
    /// The elements to start from; true if the change was below the start
    /// (the start is an anchor candidate itself, and its children are
    /// preceding siblings).
    starts: Vec<(NodeId, bool)>,
    slots: [(KeySet, KeySet); 5],
}

/// The key sets of a list of entries, for [`Planner::apply`]: the subject
/// keys per position, all subject keys, and the anchor kinds used.
#[derive(Debug, Default)]
struct Prepared {
    ids: Vec<u32>,
    plain: [KeySet; 5],
    subjects: KeySet,
    kinds: [bool; 3],
    everything: bool,
}

/// Turns changes into a [`Plan`] (ADR 0041 section 2, ADR 0042 section 2).
/// Each element is visited a bounded number of times per change that
/// reaches it; ancestor walks stop at ancestors that an earlier walk
/// visited. The work is counted: past `budget`, the plan is too large.
pub(crate) struct Planner<'a> {
    pub(crate) doc: &'a Document,
    pub(crate) index: &'a InvalidationIndex,
    pub(crate) quirks: bool,
    pub(crate) plan: Plan,
    anchors: [Anchors; 3],
    work: usize,
    budget: usize,
    /// Scratch space for entry lists.
    pub(crate) scratch: Vec<u32>,
    /// The key sets of the last entry list: the children of one parent
    /// often have the same entries.
    last: Option<Prepared>,
}

impl<'a> Planner<'a> {
    /// A planner with an empty plan; `budget` bounds the marking work.
    pub(crate) fn new(
        doc: &'a Document,
        index: &'a InvalidationIndex,
        quirks: bool,
        budget: usize,
    ) -> Self {
        Planner {
            doc,
            index,
            quirks,
            plan: Plan::default(),
            anchors: Default::default(),
            work: 0,
            budget,
            scratch: Vec::new(),
            last: None,
        }
    }

    /// True if the plan cannot be targeted any more.
    pub(crate) fn done(&self) -> bool {
        self.plan.everything || self.plan.too_large
    }

    /// Counts `n` units of marking work.
    pub(crate) fn count(&mut self, n: usize) {
        self.work = self.work.saturating_add(n);
        if self.work > self.budget {
            self.plan.too_large = true;
        }
    }

    /// Marks `n` (an element that a change restyles).
    pub(crate) fn mark_one(&mut self, n: NodeId) {
        self.count(1);
        self.plan.marked.insert(n);
    }

    /// Makes `n` a root whose descendants with one of `keys` are restyled.
    pub(crate) fn root(&mut self, n: NodeId, keys: &KeySet) {
        self.count(1);
        self.plan.roots.entry(n).or_default().merge(keys);
    }

    /// Marks `n` if it has one of `keys`.
    fn mark(&mut self, n: NodeId, keys: &KeySet) {
        self.count(1);
        if self
            .doc
            .element(n)
            .is_some_and(|d| keys.matches(d, self.quirks))
        {
            self.plan.marked.insert(n);
        }
    }

    /// The state flips of ADR 0041 (and of `:target` and the control
    /// states).
    pub(crate) fn flip_states(&mut self, flips: &[(NodeId, ElementState)]) {
        let mut ids = std::mem::take(&mut self.scratch);
        for &(node, flags) in flips {
            let Some(data) = self.doc.element(node) else {
                continue;
            };
            ids.clear();
            self.index.states_of(data, flags, self.quirks, &mut ids);
            self.apply(node, &ids, At::Element);
            if self.done() {
                break;
            }
        }
        self.scratch = ids;
    }

    /// The key sets of the entries `ids`; their anchor keys are added to
    /// the anchors of the plan.
    fn prepare(&mut self, ids: &[u32]) -> Prepared {
        let quirks = self.quirks;
        let mut prepared = Prepared {
            ids: ids.to_vec(),
            ..Prepared::default()
        };
        for &id in ids {
            let Some(entry) = self.index.entries.get(id as usize) else {
                continue;
            };
            if entry.reach == StateReach::EVERYTHING {
                prepared.everything = true;
                return prepared;
            }
            let Some(slot) = POSITIONS.iter().position(|&r| r == entry.reach) else {
                continue;
            };
            let subject = &*entry.subject;
            prepared.subjects.add(subject, quirks);
            match &entry.anchor {
                Some((kind, keys)) => {
                    let Some(k) = ANCHOR_KINDS.iter().position(|r| r == kind) else {
                        continue;
                    };
                    if let Some(used) = prepared.kinds.get_mut(k) {
                        *used = true;
                    }
                    if let Some(anchors) = self.anchors.get_mut(k)
                        && let Some((anchor_keys, subject_keys)) = anchors.slots.get_mut(slot)
                    {
                        if keys.is_empty() {
                            anchor_keys.add(&Key::Any, quirks);
                        }
                        for key in keys {
                            anchor_keys.add(key, quirks);
                        }
                        subject_keys.add(subject, quirks);
                    }
                }
                None => {
                    if let Some(set) = prepared.plain.get_mut(slot) {
                        set.add(subject, quirks);
                    }
                }
            }
        }
        prepared
    }

    /// Applies the entries `ids` to a change at `node` (ADR 0041 section 2,
    /// "Marking"; ADR 0042 section 2).
    pub(crate) fn apply(&mut self, node: NodeId, ids: &[u32], at: At) {
        if ids.is_empty() {
            return;
        }
        let prepared = match self.last.take() {
            Some(last) if last.ids == ids => last,
            _ => self.prepare(ids),
        };
        self.apply_prepared(node, &prepared, at);
        self.last = Some(prepared);
    }

    /// [`Planner::apply`] with the key sets of the entries.
    fn apply_prepared(&mut self, node: NodeId, prepared: &Prepared, at: At) {
        if prepared.everything {
            self.plan.everything = true;
            return;
        }
        for (k, &used) in prepared.kinds.iter().enumerate() {
            if used && let Some(anchors) = self.anchors.get_mut(k) {
                anchors.starts.push((node, at != At::Element));
            }
        }
        match at {
            At::Element | At::Subtree => {
                for (reach, keys) in POSITIONS.into_iter().zip(&prepared.plain) {
                    if !keys.is_empty() {
                        self.position(node, reach, keys);
                    }
                }
                if at == At::Subtree && !prepared.subjects.is_empty() {
                    self.root(node, &prepared.subjects);
                }
            }
            At::RemovedChild => {
                // The removed element's position is gone: its following
                // siblings are among all children of `node`.
                let [_, _, _, following, following_descendants] = &prepared.plain;
                if !following.is_empty() || !following_descendants.is_empty() {
                    let mut child = self.doc.first_element_child(node);
                    while let Some(c) = child {
                        if !following.is_empty() {
                            self.mark(c, following);
                        }
                        if !following_descendants.is_empty() {
                            self.root(c, following_descendants);
                        }
                        child = self.doc.next_sibling_element(c);
                    }
                }
            }
        }
    }

    /// Marks the elements at `reach` from `node` that have one of `keys`.
    fn position(&mut self, node: NodeId, reach: StateReach, keys: &KeySet) {
        let doc = self.doc;
        match reach {
            StateReach::SELF => self.mark(node, keys),
            StateReach::CHILDREN => {
                let children: Vec<NodeId> = doc
                    .element_children(node)
                    .chain(doc.flat_element_children(node))
                    .collect();
                for child in children {
                    self.mark(child, keys);
                }
            }
            StateReach::DESCENDANTS => self.root(node, keys),
            StateReach::FOLLOWING_SIBLINGS | StateReach::FOLLOWING_SIBLING_DESCENDANTS => {
                let mut sibling = doc.next_sibling_element(node);
                while let Some(s) = sibling {
                    if reach == StateReach::FOLLOWING_SIBLINGS {
                        self.mark(s, keys);
                    } else {
                        self.root(s, keys);
                    }
                    if self.plan.too_large {
                        return;
                    }
                    sibling = doc.next_sibling_element(s);
                }
            }
            _ => {}
        }
    }

    /// Applies the slots of `anchors` to the candidate `n`.
    fn anchor_candidate(&mut self, anchors: &Anchors, n: NodeId) {
        let Some(data) = self.doc.element(n) else {
            return;
        };
        self.count(1);
        for (reach, (anchor_keys, subjects)) in POSITIONS.into_iter().zip(&anchors.slots) {
            if !subjects.is_empty() && anchor_keys.matches(data, self.quirks) {
                self.position(n, reach, subjects);
            }
        }
    }

    /// Visits the preceding siblings of `n` as anchor candidates, up to one
    /// that an earlier start visited (its preceding siblings were visited
    /// then).
    fn preceding_candidates(
        &mut self,
        anchors: &Anchors,
        n: NodeId,
        visited: &mut HashSet<(NodeId, Step)>,
    ) {
        let mut sibling = self.doc.prev_sibling_element(n);
        while let Some(s) = sibling {
            if !visited.insert((s, Step::Sibling)) {
                break;
            }
            self.anchor_candidate(anchors, s);
            sibling = self.doc.prev_sibling_element(s);
        }
    }

    /// Marks the `:has()` anchors and what their positions reach (ADR 0042
    /// section 1): the keys of all changes together (a superset), so that
    /// each candidate is visited once per kind.
    fn mark_anchors(&mut self) {
        let doc = self.doc;
        for (k, kind) in ANCHOR_KINDS.into_iter().enumerate() {
            let Some(slot) = self.anchors.get_mut(k) else {
                continue;
            };
            let mut anchors = std::mem::take(slot);
            let starts = std::mem::take(&mut anchors.starts);
            let mut visited = HashSet::new();
            for (start, below) in starts {
                if self.done() {
                    return;
                }
                // A change below `start`: its children are preceding
                // siblings of the changed position (a superset).
                if below && kind != StateReach::HAS_ANCESTORS {
                    let children: Vec<NodeId> = doc.element_children(start).collect();
                    for c in children {
                        if visited.insert((c, Step::Sibling)) {
                            self.anchor_candidate(&anchors, c);
                        }
                    }
                }
                match kind {
                    StateReach::HAS_ANCESTORS => {
                        if below && visited.insert((start, Step::Parent)) {
                            self.anchor_candidate(&anchors, start);
                        }
                        for step in [Step::Parent, Step::FlatParent] {
                            let mut current = step.next(doc, start);
                            while let Some(n) = current {
                                if !visited.insert((n, step)) {
                                    break;
                                }
                                self.anchor_candidate(&anchors, n);
                                current = step.next(doc, n);
                            }
                        }
                    }
                    StateReach::HAS_PRECEDING_SIBLINGS => {
                        if !below {
                            self.preceding_candidates(&anchors, start, &mut visited);
                        }
                    }
                    _ => {
                        for step in [Step::Parent, Step::FlatParent] {
                            let mut current = Some(start);
                            while let Some(n) = current {
                                if n != start && !visited.insert((n, step)) {
                                    break;
                                }
                                self.preceding_candidates(&anchors, n, &mut visited);
                                current = step.next(doc, n);
                            }
                        }
                    }
                }
            }
        }
    }

    /// The plan: the anchors marked, and the ancestors of all marks.
    pub(crate) fn finish(mut self) -> Plan {
        if !self.done() {
            self.mark_anchors();
        }
        let mut plan = self.plan;
        if !plan.everything && !plan.too_large {
            plan.mark_ancestors(self.doc);
        }
        plan
    }
}

/// A step up the tree: to the DOM parent or the flat tree parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Step {
    Parent,
    FlatParent,
    /// For the visited marks of preceding siblings.
    Sibling,
}

impl Step {
    /// The parent element of `node` by this step.
    fn next(self, doc: &Document, node: NodeId) -> Option<NodeId> {
        let parent = match self {
            Step::Parent | Step::Sibling => doc.parent(node),
            Step::FlatParent => doc.flat_parent(node),
        };
        parent.filter(|&p| doc.element(p).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(css: &str) -> InvalidationIndex {
        let list = swb_css::SelectorList::parse_str(css).expect("valid selectors");
        InvalidationIndex::new(list.iter(), false)
    }

    #[test]
    fn the_entries_of_a_selector_are_shared() {
        // (descendants, .b) for `:hover` and the class `a`, (self, .b) for
        // the class `b`.
        assert_eq!(index(".a:hover .b").len(), 2);
        assert_eq!(index(".a:hover .b, .c:focus .b").len(), 4);
    }

    #[test]
    fn key_sets() {
        let (doc, _) = swb_dom::parse_html_bytes(
            b"<div id=x class='a b'></div><svg><Foo/></svg>",
            Some("utf-8"),
        );
        let div = doc
            .find_element(NodeId::DOCUMENT, |e| e.id() == Some("x"))
            .expect("div");
        let data = doc.element(div).expect("element");
        let mut set = KeySet::default();
        set.add(&Key::Class("b".into()), false);
        assert!(set.matches(data, false));
        let mut set = KeySet::default();
        set.add(&Key::Class("B".into()), true);
        assert!(set.matches(data, true));
        let mut exact = KeySet::default();
        exact.add(&Key::Class("B".into()), false);
        assert!(
            !exact.matches(data, false),
            "classes are case-sensitive outside quirks mode"
        );
        let mut set = KeySet::default();
        set.add(
            &Key::Name {
                name: "DIV".into(),
                lower: "div".into(),
            },
            false,
        );
        assert!(set.matches(data, false));
        let mut bigger = set.clone();
        bigger.add(&Key::Any, false);
        assert!(bigger.contains_all(&set));
        assert!(!set.contains_all(&bigger));
        assert!(
            KeySet::any().contains_all(&set),
            "any element covers all keys"
        );
        assert_eq!(bigger.len(), 3);
    }

    #[test]
    fn presence_keeps_only_entries_that_reach_outside() {
        let index = index(".a + .b, .c .d, .e");
        let mut keys = ElementKeys::default();
        keys.add_class("a");
        keys.add_class("c");
        keys.add_class("e");
        let mut out = Vec::new();
        index.presence_of_keys(&keys, false, &mut out);
        // Only `.a` reaches its following siblings, and any element between
        // `.a` and `.b` changes `+`.
        assert_eq!(out.len(), 2);
    }
}
