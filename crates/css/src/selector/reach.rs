//! Which elements a change of an element state can restyle (ADR 0041
//! section 1): for each place where a state pseudo-class occurs in a
//! selector, the position of the elements whose match can change relative
//! to the element E whose state changed, and the keys that E and those
//! elements must have.
//!
//! Derived from Selectors 4: the combinators
//! (<https://www.w3.org/TR/selectors-4/#combinators>), the logical
//! combinations (<https://www.w3.org/TR/selectors-4/#logical-combination>)
//! and `:has()` (<https://www.w3.org/TR/selectors-4/#relational>); from CSS
//! Scoping 1 for `:host()`, `:host-context()` and `::slotted()`, and CSS
//! Shadow Parts 1 for `::part()`. The result may name more elements than a
//! change can affect, never fewer.

use bitflags::bitflags;

use super::matching::split_compound;
use super::{BucketKey, Combinator, Component, ElementState, PseudoClass, Selector, ShadowPseudo};

bitflags! {
    /// Where the elements are whose match can change when a state of an
    /// element E changes. A [`StateDependency`] has exactly one flag.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct StateReach: u16 {
        /// E itself (and its pseudo-elements).
        const SELF = 1 << 0;
        /// E's element children, in the DOM and in the flat tree.
        const CHILDREN = 1 << 1;
        /// E's descendants in the flat tree.
        const DESCENDANTS = 1 << 2;
        /// E's following sibling elements.
        const FOLLOWING_SIBLINGS = 1 << 3;
        /// The flat descendants of E's following siblings.
        const FOLLOWING_SIBLING_DESCENDANTS = 1 << 4;
        /// `:has()` anchors: E's proper ancestors.
        const HAS_ANCESTORS = 1 << 5;
        /// `:has()` anchors: E's preceding siblings.
        const HAS_PRECEDING_SIBLINGS = 1 << 6;
        /// `:has()` anchors: the preceding siblings of E and of its
        /// ancestors.
        const HAS_PRECEDING_SIBLINGS_OF_ANCESTORS = 1 << 7;
        /// A shape that the analysis does not handle: any element.
        const EVERYTHING = 1 << 8;
    }
}

/// One dependency of a selector on a state: a change of the state on an
/// element E that has one of `element_keys` (any element if empty) can
/// change the match of the elements at `reach` from E that have
/// `subject_key`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateDependency<'a> {
    /// One flag.
    pub reach: StateReach,
    /// E has one of these keys; empty means any element.
    pub element_keys: Vec<BucketKey<'a>>,
    /// The key of the selector's subject ([`BucketKey::Universal`] for
    /// none).
    pub subject_key: BucketKey<'a>,
}

/// An occurrence: its reach (relative to the subject of the components
/// it was found in) and the keys of E.
type Occurrence<'a> = (StateReach, Vec<BucketKey<'a>>);

/// The reaches that combinator positions give (the first five flags).
const POSITIONS: StateReach = StateReach::SELF
    .union(StateReach::CHILDREN)
    .union(StateReach::DESCENDANTS)
    .union(StateReach::FOLLOWING_SIBLINGS)
    .union(StateReach::FOLLOWING_SIBLING_DESCENDANTS);

impl Selector {
    /// The dependencies of the selector on `state` (ADR 0041 section 1),
    /// one per reach and keys, without duplicates. Empty if the selector
    /// does not depend on `state`.
    pub fn state_reach(&self, state: ElementState) -> Vec<StateDependency<'_>> {
        let mut found = Vec::new();
        match self.shadow.as_deref() {
            None => occurrences(&self.components, state, &mut found),
            Some(ShadowPseudo::Slotted(argument)) => {
                occurrences(argument, state, &mut found);
                // The other compounds match the slot (or a slot that it is
                // assigned to, further up the flat tree) and its tree.
                let mut outer = Vec::new();
                occurrences(&self.components, state, &mut outer);
                found.extend(
                    outer
                        .into_iter()
                        .map(|(r, k)| (then(r, StateReach::DESCENDANTS), k)),
                );
            }
            Some(ShadowPseudo::Part { after, .. }) => {
                occurrences(after, state, &mut found);
                // The other compounds match the host of the part's tree.
                let mut outer = Vec::new();
                occurrences(&self.components, state, &mut outer);
                found.extend(
                    outer
                        .into_iter()
                        .map(|(r, k)| (then(r, StateReach::DESCENDANTS), k)),
                );
            }
        }
        let subject_key = self.bucket_key();
        let mut out: Vec<StateDependency<'_>> = Vec::new();
        for (reach, element_keys) in found {
            for flag in reach.iter() {
                let dependency = StateDependency {
                    reach: flag,
                    element_keys: element_keys.clone(),
                    subject_key,
                };
                if !out.contains(&dependency) {
                    out.push(dependency);
                }
            }
        }
        out
    }
}

/// True for the combinators that move to a parent.
fn is_down(combinator: Combinator) -> bool {
    matches!(combinator, Combinator::Descendant | Combinator::Child)
}

/// The reach of a compound from the subject: `combinators` are those to
/// its right, the nearest last.
fn position(combinators: &[Combinator]) -> StateReach {
    let Some((&nearest, rest)) = combinators.split_last() else {
        return StateReach::SELF;
    };
    let later_down = rest.iter().any(|&c| is_down(c));
    match (is_down(nearest), later_down) {
        // Sibling steps after a step down stay below E.
        (true, false) if nearest == Combinator::Child => StateReach::CHILDREN,
        (true, _) => StateReach::DESCENDANTS,
        (false, false) => StateReach::FOLLOWING_SIBLINGS,
        (false, true) => StateReach::FOLLOWING_SIBLING_DESCENDANTS,
    }
}

/// The reach of `first` (from E to an element Y) followed by `second`
/// (from Y to the subject X), for one flag each.
fn then_one(first: StateReach, second: StateReach) -> StateReach {
    use StateReach as R;
    if first == R::EVERYTHING || second == R::EVERYTHING {
        return R::EVERYTHING;
    }
    if first == R::SELF {
        return second;
    }
    if second == R::SELF {
        return first;
    }
    if !POSITIONS.contains(first) || !POSITIONS.contains(second) {
        // `:has()` anchors with a position other than the subject: the
        // descendants or siblings of every ancestor.
        return R::EVERYTHING;
    }
    match first {
        // Siblings of a child are children; everything else below a child
        // is below E.
        R::CHILDREN if second == R::FOLLOWING_SIBLINGS => R::CHILDREN,
        R::CHILDREN | R::DESCENDANTS => R::DESCENDANTS,
        R::FOLLOWING_SIBLINGS if second == R::FOLLOWING_SIBLINGS => R::FOLLOWING_SIBLINGS,
        _ => R::FOLLOWING_SIBLING_DESCENDANTS,
    }
}

/// [`then_one`] for every pair of flags.
fn then(first: StateReach, second: StateReach) -> StateReach {
    let mut out = StateReach::empty();
    for a in first.iter() {
        for b in second.iter() {
            out |= then_one(a, b);
        }
    }
    out
}

/// The keys that an element matching `compound` has one of (empty: any):
/// the first ID, else the first class, else the type, as
/// [`Selector::bucket_key`]; else, for an `:is()` or `:where()` whose
/// arguments all have subject keys, those keys.
fn compound_keys(compound: &[Component]) -> Vec<BucketKey<'_>> {
    let mut class = None;
    let mut local_name = None;
    for component in compound {
        match component {
            Component::Id(id) => return vec![BucketKey::Id(id)],
            Component::Class(c) if class.is_none() => class = Some(&**c),
            Component::LocalName(name) if local_name.is_none() => {
                local_name = Some(BucketKey::LocalName {
                    name: &name.name,
                    lower_name: name.lower(),
                });
            }
            _ => {}
        }
    }
    if let Some(class) = class {
        return vec![BucketKey::Class(class)];
    }
    if let Some(key) = local_name {
        return vec![key];
    }
    for component in compound {
        if let Component::PseudoClass(PseudoClass::Is(list) | PseudoClass::Where(list)) = component
        {
            let keys: Vec<BucketKey<'_>> = list.iter().map(Selector::bucket_key).collect();
            if !keys.is_empty() && !keys.contains(&BucketKey::Universal) {
                return keys;
            }
        }
    }
    Vec::new()
}

/// Appends the occurrences of `state` in the right-to-left `components`,
/// with reaches relative to their subject.
fn occurrences<'a>(
    components: &'a [Component],
    state: ElementState,
    out: &mut Vec<Occurrence<'a>>,
) {
    let (mut compound, mut rest) = split_compound(components);
    let mut combinators: Vec<Combinator> = Vec::new();
    loop {
        let here = position(&combinators);
        let keys = compound_keys(compound);
        for component in compound {
            if let Component::PseudoClass(pc) = component {
                pseudo_class(pc, state, here, &keys, out);
            }
        }
        match rest.split_first() {
            Some((Component::Combinator(c), after)) => {
                combinators.push(*c);
                (compound, rest) = split_compound(after);
            }
            _ => break,
        }
    }
}

/// The occurrences of `state` in one pseudo-class of a compound at
/// position `here` whose element has `keys`.
fn pseudo_class<'a>(
    pc: &'a PseudoClass,
    state: ElementState,
    here: StateReach,
    keys: &[BucketKey<'a>],
    out: &mut Vec<Occurrence<'a>>,
) {
    // The occurrences of an argument selector, whose subject matches the
    // compound's element: at the argument's subject, E is that element, so
    // the compound's keys apply too.
    let argument = |selector: &'a Selector, out: &mut Vec<Occurrence<'a>>| {
        let mut inner = Vec::new();
        occurrences(&selector.components, state, &mut inner);
        for (reach, inner_keys) in inner {
            let keys = if reach == StateReach::SELF && inner_keys.is_empty() {
                keys.to_vec()
            } else {
                inner_keys
            };
            out.push((then(reach, here), keys));
        }
    };
    match pc {
        PseudoClass::State(s) if s.intersects(state) => out.push((here, keys.to_vec())),
        PseudoClass::Is(list) | PseudoClass::Where(list) | PseudoClass::Not(list) => {
            for selector in list {
                argument(selector, out);
            }
        }
        PseudoClass::Host(Some(selector)) => argument(selector, out),
        PseudoClass::HostContext(selector) => {
            // E is the host or one of its flat ancestors.
            let mut inner = Vec::new();
            occurrences(&selector.components, state, &mut inner);
            for (_, inner_keys) in inner {
                out.push((
                    then(StateReach::SELF | StateReach::DESCENDANTS, here),
                    inner_keys,
                ));
            }
        }
        PseudoClass::NthChild(nth) | PseudoClass::NthLastChild(nth) => {
            // The index of E's siblings in both directions can change.
            for selector in nth.of.iter().flatten() {
                let mut inner = Vec::new();
                occurrences(&selector.components, state, &mut inner);
                out.extend(
                    inner
                        .into_iter()
                        .map(|(_, keys)| (StateReach::EVERYTHING, keys)),
                );
            }
        }
        PseudoClass::Has(relatives) => {
            for relative in relatives {
                let mut inner = Vec::new();
                occurrences(&relative.selector.components, state, &mut inner);
                for (reach, inner_keys) in inner {
                    let anchors = if reach == StateReach::SELF {
                        has_anchors(relative.combinator, &relative.selector.components)
                    } else {
                        StateReach::EVERYTHING
                    };
                    out.push((then(anchors, here), inner_keys));
                }
            }
        }
        _ => {}
    }
}

/// The anchors of a `:has()` whose relative selector (leading combinator
/// `leading`, then `components`) has the state in its subject compound.
fn has_anchors(leading: Combinator, components: &[Component]) -> StateReach {
    if is_down(leading) {
        return StateReach::HAS_ANCESTORS;
    }
    let any_down = components
        .iter()
        .any(|c| matches!(c, Component::Combinator(c) if is_down(*c)));
    if any_down {
        StateReach::HAS_PRECEDING_SIBLINGS_OF_ANCESTORS
    } else {
        StateReach::HAS_PRECEDING_SIBLINGS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selector::SelectorList;

    type R = StateReach;

    /// The dependencies of the first selector of `css` on `:hover`, as
    /// (reach, element keys, subject key) with keys written as `#id`,
    /// `.class`, `name` or `*`.
    fn deps(css: &str) -> Vec<(R, Vec<String>, String)> {
        let list = SelectorList::parse_str(css).expect("valid selector");
        let selector = &list.selectors()[0];
        let key = |k: &BucketKey<'_>| match k {
            BucketKey::Id(id) => format!("#{id}"),
            BucketKey::Class(c) => format!(".{c}"),
            BucketKey::LocalName { lower_name, .. } => (*lower_name).to_owned(),
            BucketKey::Universal => "*".to_owned(),
        };
        selector
            .state_reach(ElementState::HOVER)
            .iter()
            .map(|d| {
                (
                    d.reach,
                    d.element_keys.iter().map(key).collect(),
                    key(&d.subject_key),
                )
            })
            .collect()
    }

    fn one(reach: R, keys: &[&str], subject: &str) -> Vec<(R, Vec<String>, String)> {
        vec![(
            reach,
            keys.iter().map(|k| (*k).to_owned()).collect(),
            subject.to_owned(),
        )]
    }

    #[test]
    fn positions_of_the_combinators() {
        assert_eq!(deps("a.x:hover"), one(R::SELF, &[".x"], ".x"));
        assert_eq!(deps(":hover"), one(R::SELF, &[], "*"));
        assert_eq!(deps(".a:hover > .b"), one(R::CHILDREN, &[".a"], ".b"));
        assert_eq!(deps(".a:hover .b"), one(R::DESCENDANTS, &[".a"], ".b"));
        assert_eq!(
            deps(".a:hover > .b > .c"),
            one(R::DESCENDANTS, &[".a"], ".c")
        );
        assert_eq!(deps(".a:hover > .b ~ .c"), one(R::CHILDREN, &[".a"], ".c"));
        assert_eq!(deps(".a:hover .b + .c"), one(R::DESCENDANTS, &[".a"], ".c"));
        assert_eq!(
            deps("#a:hover + .b"),
            one(R::FOLLOWING_SIBLINGS, &["#a"], ".b")
        );
        assert_eq!(
            deps(".a:hover ~ .b + .c"),
            one(R::FOLLOWING_SIBLINGS, &[".a"], ".c")
        );
        assert_eq!(
            deps(".a:hover ~ .b .c"),
            one(R::FOLLOWING_SIBLING_DESCENDANTS, &[".a"], ".c")
        );
        assert_eq!(
            deps(".a:hover ~ .b > .c ~ .d"),
            one(R::FOLLOWING_SIBLING_DESCENDANTS, &[".a"], ".d")
        );
        assert_eq!(deps("li:hover::before"), one(R::SELF, &["li"], "li"));
        assert_eq!(deps(".a:focus .b"), vec![]);
    }

    #[test]
    fn nested_arguments_compose() {
        assert_eq!(deps(".a:is(:hover)"), one(R::SELF, &[".a"], ".a"));
        assert_eq!(deps(".a:not(.b:hover)"), one(R::SELF, &[".b"], ".a"));
        assert_eq!(
            deps(".x:where(:not(:hover)) .y"),
            one(R::DESCENDANTS, &[".x"], ".y")
        );
        // Inside the argument: E is an ancestor of the argument's subject.
        assert_eq!(
            deps(".x:is(.a:hover .b)"),
            one(R::DESCENDANTS, &[".a"], ".x")
        );
        // Siblings, then children: the children of following siblings.
        assert_eq!(
            deps(":is(.a:hover ~ .b) > .c"),
            one(R::FOLLOWING_SIBLING_DESCENDANTS, &[".a"], ".c")
        );
        // Children, then siblings: still children.
        assert_eq!(
            deps(":is(.a:hover > .b) ~ .c"),
            one(R::CHILDREN, &[".a"], ".c")
        );
        // Two levels deep.
        assert_eq!(
            deps(":is(:where(.a:hover + .b) + .c)"),
            one(R::FOLLOWING_SIBLINGS, &[".a"], "*")
        );
        // The keys of the arguments of `:is()`.
        assert_eq!(
            deps(":is(.l1, .l2):hover .c"),
            one(R::DESCENDANTS, &[".l1", ".l2"], ".c")
        );
        assert_eq!(
            deps(":is(.l1, div):hover"),
            one(R::SELF, &[".l1", "div"], "*")
        );
        assert_eq!(deps(":is(.l1, *):hover"), one(R::SELF, &[], "*"));
    }

    #[test]
    fn has_shapes() {
        assert_eq!(deps(".a:has(:hover)"), one(R::HAS_ANCESTORS, &[], ".a"));
        assert_eq!(
            deps(".a:has(> .b:hover)"),
            one(R::HAS_ANCESTORS, &[".b"], ".a")
        );
        assert_eq!(
            deps(".a:has(.c ~ .b:hover)"),
            one(R::HAS_ANCESTORS, &[".b"], ".a")
        );
        assert_eq!(
            deps(".a:has(+ .b:hover)"),
            one(R::HAS_PRECEDING_SIBLINGS, &[".b"], ".a")
        );
        assert_eq!(
            deps(".a:has(~ .c .b:hover)"),
            one(R::HAS_PRECEDING_SIBLINGS_OF_ANCESTORS, &[".b"], ".a")
        );
        assert_eq!(deps(":is(.a:has(:hover))"), one(R::HAS_ANCESTORS, &[], "*"));
        // Shapes that are not analysed.
        assert_eq!(
            deps(".a:has(.c:hover .b)"),
            one(R::EVERYTHING, &[".c"], ".a")
        );
        assert_eq!(deps(".a:has(:hover) .d"), one(R::EVERYTHING, &[], ".d"));
        assert_eq!(deps(".a:has(:hover) > .d"), one(R::EVERYTHING, &[], ".d"));
    }

    #[test]
    fn nth_child_of_a_state_is_everything() {
        assert_eq!(
            deps("li:nth-child(2 of .x:hover)"),
            one(R::EVERYTHING, &[".x"], "li")
        );
        assert_eq!(deps("li:nth-child(2 of .x)"), vec![]);
    }

    #[test]
    fn shadow_selectors() {
        assert_eq!(deps(":host(.h:hover)"), one(R::SELF, &[".h"], "*"));
        assert_eq!(deps(":host(:hover) .b"), one(R::DESCENDANTS, &[], ".b"));
        assert_eq!(
            deps(":host-context(.t:hover) .b"),
            one(R::DESCENDANTS, &[".t"], ".b")
        );
        assert_eq!(
            deps(":host-context(.t:hover)"),
            vec![
                (R::SELF, vec![".t".to_owned()], "*".to_owned()),
                (R::DESCENDANTS, vec![".t".to_owned()], "*".to_owned()),
            ]
        );
        assert_eq!(deps("::slotted(.s:hover)"), one(R::SELF, &[".s"], ".s"));
        assert_eq!(
            deps("slot:hover::slotted(.s)"),
            one(R::DESCENDANTS, &["slot"], ".s")
        );
        assert_eq!(deps("::part(p):hover"), one(R::SELF, &[], "*"));
        assert_eq!(
            deps("x-a:hover::part(p)"),
            one(R::DESCENDANTS, &["x-a"], "*")
        );
    }

    #[test]
    fn other_states() {
        let list = SelectorList::parse_str(".a:focus-visible .b, .c:focus-within").expect("valid");
        let reach = |i: usize, s: ElementState| -> Vec<R> {
            list.selectors()[i]
                .state_reach(s)
                .iter()
                .map(|d| d.reach)
                .collect()
        };
        assert_eq!(reach(0, ElementState::FOCUS_VISIBLE), vec![R::DESCENDANTS]);
        assert_eq!(reach(0, ElementState::FOCUS), vec![]);
        assert_eq!(reach(1, ElementState::FOCUS_WITHIN), vec![R::SELF]);
    }

    #[test]
    fn composition_table() {
        use StateReach as R;
        let all = [
            R::SELF,
            R::CHILDREN,
            R::DESCENDANTS,
            R::FOLLOWING_SIBLINGS,
            R::FOLLOWING_SIBLING_DESCENDANTS,
        ];
        for a in all {
            assert_eq!(then(R::SELF, a), a);
            assert_eq!(then(a, R::SELF), a);
            assert_eq!(
                then(R::HAS_ANCESTORS, a),
                if a == R::SELF {
                    R::HAS_ANCESTORS
                } else {
                    R::EVERYTHING
                }
            );
            assert_eq!(then(R::EVERYTHING, a), R::EVERYTHING);
        }
        assert_eq!(then(R::CHILDREN, R::CHILDREN), R::DESCENDANTS);
        assert_eq!(
            then(R::FOLLOWING_SIBLINGS, R::DESCENDANTS),
            R::FOLLOWING_SIBLING_DESCENDANTS
        );
        assert_eq!(
            then(R::DESCENDANTS, R::FOLLOWING_SIBLING_DESCENDANTS),
            R::DESCENDANTS
        );
        assert_eq!(
            then(R::FOLLOWING_SIBLING_DESCENDANTS, R::FOLLOWING_SIBLINGS),
            R::FOLLOWING_SIBLING_DESCENDANTS
        );
    }
}
