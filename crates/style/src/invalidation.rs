//! Which elements a change of the element states can restyle (ADR 0041).
//!
//! [`StateIndex`] holds the dependencies of the stylist's selectors on the
//! states `:hover`, `:active`, `:focus`, `:focus-visible` and
//! `:focus-within` ([`swb_css::Selector::state_reach`]), indexed by the key
//! that the element whose state changed must have, as the rule buckets
//! index rules by the subject's key. [`plan`] turns the elements whose state
//! flipped into the elements to restyle: some marked one by one, others as
//! the descendants of a root that have one of a set of subject keys. The
//! restyle itself is in `cascade/targeted.rs`.

use std::collections::{HashMap, HashSet};

use swb_css::{BucketKey, ElementState, Selector, StateReach};
use swb_dom::{Document, ElementData, NodeId};

use crate::element::ElementStates;

/// The states whose changes restyle only the elements they can affect.
pub(crate) const TARGETED_STATES: ElementState = ElementState::HOVER
    .union(ElementState::ACTIVE)
    .union(ElementState::FOCUS)
    .union(ElementState::FOCUS_VISIBLE)
    .union(ElementState::FOCUS_WITHIN);

/// The flags of [`TARGETED_STATES`], in the order of [`StateIndex`]'s maps.
const FLAGS: [ElementState; 5] = [
    ElementState::HOVER,
    ElementState::ACTIVE,
    ElementState::FOCUS,
    ElementState::FOCUS_VISIBLE,
    ElementState::FOCUS_WITHIN,
];

/// A subject key, owned.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Key {
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
}

/// One dependency: a reach (one flag) and the subject's key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Entry {
    reach: StateReach,
    subject: Key,
}

/// The entries of one state flag, bucketed by the key that the element
/// whose state changed must have (as `RuleMap` buckets rules).
#[derive(Debug, Default)]
struct ReachMap {
    ids: HashMap<Box<str>, Vec<u32>>,
    classes: HashMap<Box<str>, Vec<u32>>,
    html_names: HashMap<Box<str>, Vec<u32>>,
    other_names: HashMap<Box<str>, Vec<u32>>,
    universal: Vec<u32>,
}

impl ReachMap {
    fn insert(&mut self, key: BucketKey<'_>, entry: u32, quirks: bool) {
        let fold = |s: &str| -> Box<str> {
            if quirks {
                s.to_ascii_lowercase().into_boxed_str()
            } else {
                s.into()
            }
        };
        match key {
            BucketKey::Id(id) => self.ids.entry(fold(id)).or_default().push(entry),
            BucketKey::Class(c) => self.classes.entry(fold(c)).or_default().push(entry),
            BucketKey::LocalName { name, lower_name } => {
                self.html_names
                    .entry(lower_name.into())
                    .or_default()
                    .push(entry);
                self.other_names.entry(name.into()).or_default().push(entry);
            }
            BucketKey::Universal => self.universal.push(entry),
        }
    }

    /// Sorts and deduplicates every bucket.
    fn finish(&mut self) {
        for map in [
            &mut self.ids,
            &mut self.classes,
            &mut self.html_names,
            &mut self.other_names,
        ] {
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
        if let Some(id) = data.id() {
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
        let names = if data.is_html() {
            &self.html_names
        } else {
            &self.other_names
        };
        out.extend(names.get(&**data.local_name()).into_iter().flatten());
        out.extend(&self.universal);
    }
}

/// The dependencies of a stylist's selectors on the [`TARGETED_STATES`]
/// (ADR 0041 section 1).
#[derive(Debug, Default)]
pub(crate) struct StateIndex {
    maps: [ReachMap; 5],
    entries: Vec<Entry>,
}

impl StateIndex {
    /// Builds the index of `selectors`.
    pub(crate) fn new<'a>(selectors: impl Iterator<Item = &'a Selector>, quirks: bool) -> Self {
        let mut index = StateIndex::default();
        let mut known: HashMap<Entry, u32> = HashMap::new();
        for selector in selectors {
            let depends = selector.state_dependencies();
            if !depends.intersects(TARGETED_STATES) {
                continue;
            }
            for (map, flag) in index.maps.iter_mut().zip(FLAGS) {
                if !depends.intersects(flag) {
                    continue;
                }
                for dependency in selector.state_reach(flag) {
                    let entry = Entry {
                        reach: dependency.reach,
                        subject: Key::of(dependency.subject_key),
                    };
                    let next = u32::try_from(index.entries.len()).unwrap_or(u32::MAX);
                    let id = *known.entry(entry.clone()).or_insert_with(|| {
                        index.entries.push(entry);
                        next
                    });
                    if dependency.element_keys.is_empty() {
                        map.insert(BucketKey::Universal, id, quirks);
                    }
                    for &key in &dependency.element_keys {
                        map.insert(key, id, quirks);
                    }
                }
            }
        }
        for map in &mut index.maps {
            map.finish();
        }
        index
    }

    /// The number of distinct entries.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
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
    fn is_empty(&self) -> bool {
        !self.any
            && self.ids.is_empty()
            && self.classes.is_empty()
            && self.html_names.is_empty()
            && self.other_names.is_empty()
    }

    /// Adds a key (IDs and classes folded to lowercase in quirks mode).
    fn add(&mut self, key: &Key, quirks: bool) {
        let fold = |s: &str| -> Box<str> {
            if quirks {
                s.to_ascii_lowercase().into_boxed_str()
            } else {
                s.into()
            }
        };
        match key {
            Key::Id(id) => {
                self.ids.insert(fold(id));
            }
            Key::Class(c) => {
                self.classes.insert(fold(c));
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

/// What a state change restyles (ADR 0041 section 2, "Marking").
#[derive(Debug, Default)]
pub(crate) struct Plan {
    /// Elements to restyle.
    pub(crate) marked: HashSet<NodeId>,
    /// Roots whose descendants with one of the keys are restyled.
    pub(crate) roots: HashMap<NodeId, KeySet>,
    /// The flat ancestors of the marked elements and of the roots: the
    /// walk enters them.
    pub(crate) below: HashSet<NodeId>,
    /// A dependency that the analysis does not handle: restyle all.
    pub(crate) everything: bool,
}

impl Plan {
    /// True if nothing is restyled.
    pub(crate) fn is_empty(&self) -> bool {
        self.marked.is_empty() && self.roots.is_empty() && !self.everything
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
    let mut add = |nodes: &mut dyn Iterator<Item = NodeId>, flag: ElementState| {
        for node in nodes {
            *flips.entry(node).or_default() |= flag;
        }
    };
    let pairs = [
        (old.hover, new.hover, ElementState::HOVER),
        (old.active, new.active, ElementState::ACTIVE),
        (old.focus, new.focus, ElementState::FOCUS_WITHIN),
    ];
    for (a, b, flag) in pairs {
        if changed.intersects(flag) && a != b {
            let (a, b) = (chain(doc, a), chain(doc, b));
            add(&mut a.symmetric_difference(&b).copied(), flag);
        }
    }
    if changed.intersects(ElementState::FOCUS) && old.focus != new.focus {
        add(
            &mut [old.focus, new.focus].into_iter().flatten(),
            ElementState::FOCUS,
        );
        let (a, b) = (focus_hosts(doc, old.focus), focus_hosts(doc, new.focus));
        add(
            &mut a.symmetric_difference(&b).copied(),
            ElementState::FOCUS,
        );
    }
    if changed.intersects(ElementState::FOCUS_VISIBLE)
        && (old.focus != new.focus || old.focus_visible != new.focus_visible)
    {
        add(
            &mut [old.focus, new.focus].into_iter().flatten(),
            ElementState::FOCUS_VISIBLE,
        );
    }
    let mut out: Vec<(NodeId, ElementState)> = flips
        .into_iter()
        .filter(|(node, _)| doc.element(*node).is_some())
        .collect();
    out.sort_unstable_by_key(|(node, _)| *node);
    out
}

/// The reaches in the order of the slots of [`plan`]'s key sets.
const REACHES: [StateReach; 9] = [
    StateReach::SELF,
    StateReach::CHILDREN,
    StateReach::DESCENDANTS,
    StateReach::FOLLOWING_SIBLINGS,
    StateReach::FOLLOWING_SIBLING_DESCENDANTS,
    StateReach::HAS_ANCESTORS,
    StateReach::HAS_PRECEDING_SIBLINGS,
    StateReach::HAS_PRECEDING_SIBLINGS_OF_ANCESTORS,
    StateReach::EVERYTHING,
];

/// The elements that a state change restyles: for each flipped element,
/// the index's entries whose element key it has, by reach (ADR 0041
/// section 2, "Marking"). Each element is visited a bounded number of times
/// per flipped element that reaches it; ancestor walks stop at ancestors
/// that an earlier walk visited.
pub(crate) fn plan(
    doc: &Document,
    index: &StateIndex,
    quirks: bool,
    flipped: &[(NodeId, ElementState)],
) -> Plan {
    let mut planner = Planner {
        doc,
        quirks,
        plan: Plan::default(),
        ancestors: KeySet::default(),
        ancestor_starts: Vec::new(),
        siblings: KeySet::default(),
        sibling_starts: Vec::new(),
    };
    let mut candidates = Vec::new();
    for &(node, flags) in flipped {
        let Some(data) = doc.element(node) else {
            continue;
        };
        let sets = index.key_sets(data, flags, quirks, &mut candidates);
        if !sets[8].is_empty() {
            planner.plan.everything = true;
            return planner.plan;
        }
        planner.mark_element(node, &sets);
    }
    planner.mark_anchors();
    let mut plan = planner.plan;
    plan.mark_ancestors(doc);
    plan
}

impl StateIndex {
    /// The subject keys of the entries for an element with `data` whose
    /// states `flags` flipped, by reach (in the order of [`REACHES`]).
    fn key_sets(
        &self,
        data: &ElementData,
        flags: ElementState,
        quirks: bool,
        candidates: &mut Vec<u32>,
    ) -> [KeySet; 9] {
        let mut sets: [KeySet; 9] = Default::default();
        for (map, flag) in self.maps.iter().zip(FLAGS) {
            if !flags.intersects(flag) {
                continue;
            }
            candidates.clear();
            map.candidates(data, quirks, candidates);
            for &id in candidates.iter() {
                let Some(entry) = self.entries.get(id as usize) else {
                    continue;
                };
                if let Some(slot) = REACHES.iter().position(|&r| r == entry.reach)
                    && let Some(set) = sets.get_mut(slot)
                {
                    set.add(&entry.subject, quirks);
                }
            }
        }
        sets
    }
}

/// The state of [`plan`].
struct Planner<'a> {
    doc: &'a Document,
    quirks: bool,
    plan: Plan,
    /// The subject keys of the `:has()` anchors among ancestors, of all
    /// flipped elements together, and the elements whose ancestors to walk.
    ancestors: KeySet,
    ancestor_starts: Vec<NodeId>,
    /// The same for the anchors among the preceding siblings of ancestors.
    siblings: KeySet,
    sibling_starts: Vec<NodeId>,
}

impl Planner<'_> {
    /// Marks `n` if it has one of `keys`.
    fn mark(&mut self, n: NodeId, keys: &KeySet) {
        if self
            .doc
            .element(n)
            .is_some_and(|d| keys.matches(d, self.quirks))
        {
            self.plan.marked.insert(n);
        }
    }

    /// Marks the elements that the entries `sets` of the flipped element
    /// `node` reach, except the anchors among ancestors (see
    /// [`Planner::mark_anchors`]).
    fn mark_element(&mut self, node: NodeId, sets: &[KeySet; 9]) {
        let doc = self.doc;
        let [
            itself,
            children,
            descendants,
            following,
            following_descendants,
            ancestors,
            preceding,
            preceding_of_ancestors,
            _,
        ] = sets;
        if !itself.is_empty() {
            self.mark(node, itself);
        }
        if !children.is_empty() {
            for child in doc
                .element_children(node)
                .chain(doc.flat_element_children(node))
            {
                self.mark(child, children);
            }
        }
        if !descendants.is_empty() {
            self.plan.roots.entry(node).or_default().merge(descendants);
        }
        if !following.is_empty() || !following_descendants.is_empty() {
            let mut sibling = doc.next_sibling_element(node);
            while let Some(s) = sibling {
                if !following.is_empty() {
                    self.mark(s, following);
                }
                if !following_descendants.is_empty() {
                    self.plan
                        .roots
                        .entry(s)
                        .or_default()
                        .merge(following_descendants);
                }
                sibling = doc.next_sibling_element(s);
            }
        }
        if !ancestors.is_empty() {
            self.ancestors.merge(ancestors);
            self.ancestor_starts.push(node);
        }
        if !preceding.is_empty() {
            let mut sibling = doc.prev_sibling_element(node);
            while let Some(s) = sibling {
                self.mark(s, preceding);
                sibling = doc.prev_sibling_element(s);
            }
        }
        if !preceding_of_ancestors.is_empty() {
            self.siblings.merge(preceding_of_ancestors);
            self.sibling_starts.push(node);
        }
    }

    /// Marks the `:has()` anchors among the ancestors and among the preceding
    /// siblings of the inclusive ancestors: the keys of all flipped elements
    /// together (a superset), so that each ancestor is visited once.
    fn mark_anchors(&mut self) {
        let doc = self.doc;
        let ancestors = std::mem::take(&mut self.ancestors);
        if !ancestors.is_empty() {
            let mut visited = HashSet::new();
            for start in std::mem::take(&mut self.ancestor_starts) {
                for step in [Step::Parent, Step::FlatParent] {
                    let mut current = step.next(doc, start);
                    while let Some(n) = current {
                        if !visited.insert((n, step)) {
                            break;
                        }
                        self.mark(n, &ancestors);
                        current = step.next(doc, n);
                    }
                }
            }
        }
        let siblings = std::mem::take(&mut self.siblings);
        if !siblings.is_empty() {
            let mut visited = HashSet::new();
            for start in std::mem::take(&mut self.sibling_starts) {
                for step in [Step::Parent, Step::FlatParent] {
                    let mut current = Some(start);
                    while let Some(n) = current {
                        if !visited.insert((n, step)) {
                            break;
                        }
                        let mut sibling = doc.prev_sibling_element(n);
                        while let Some(s) = sibling {
                            self.mark(s, &siblings);
                            sibling = doc.prev_sibling_element(s);
                        }
                        current = step.next(doc, n);
                    }
                }
            }
        }
    }
}

/// A step up the tree: to the DOM parent or the flat tree parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Step {
    Parent,
    FlatParent,
}

impl Step {
    /// The parent element of `node` by this step.
    fn next(self, doc: &Document, node: NodeId) -> Option<NodeId> {
        let parent = match self {
            Step::Parent => doc.parent(node),
            Step::FlatParent => doc.flat_parent(node),
        };
        parent.filter(|&p| doc.element(p).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(css: &str) -> StateIndex {
        let list = swb_css::SelectorList::parse_str(css).expect("valid selectors");
        StateIndex::new(list.iter(), false)
    }

    #[test]
    fn equal_entries_are_merged() {
        let index = index(".a:hover .b, .c:hover .b, .a:hover, .a:focus .b");
        // (descendants, .b) and (self, .a).
        assert_eq!(index.len(), 2);
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
}
