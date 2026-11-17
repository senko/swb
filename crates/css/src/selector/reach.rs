//! Which elements a change of an element state or of the DOM can restyle
//! (ADR 0041 section 1, ADR 0042 section 1): for each simple selector whose
//! result can change, the position of the elements whose match can change
//! relative to the element E where the change happened, and the keys that E
//! and those elements must have.
//!
//! Derived from Selectors 4: the combinators
//! (<https://www.w3.org/TR/selectors-4/#combinators>), the logical
//! combinations (<https://www.w3.org/TR/selectors-4/#logical-combination>),
//! attribute selectors (§6), the structural pseudo-classes (§14) and
//! `:has()` (<https://www.w3.org/TR/selectors-4/#relational>); from CSS
//! Scoping 1 for `:host()`, `:host-context()` and `::slotted()`, and CSS
//! Shadow Parts 1 for `::part()`. The result may name more elements than a
//! change can affect, never fewer.

use bitflags::bitflags;
use smallvec::SmallVec;

use super::matching::split_compound;
use super::{BucketKey, Combinator, Component, ElementState, PseudoClass, Selector, ShadowPseudo};

bitflags! {
    /// Where the elements are whose match can change when something
    /// changes on an element E. A [`Dependency`] has exactly one flag.
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

/// What a simple selector depends on (ADR 0042 section 1, the table).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DependencyKey<'a> {
    /// A state flag (a state pseudo-class).
    State(ElementState),
    /// A class name, as written.
    Class(&'a str),
    /// An ID, as written.
    Id(&'a str),
    /// An attribute, by local name as written and in ASCII lowercase (HTML
    /// elements match the lowercase name). `:link`, `:any-link` and
    /// `:visited` depend on `href`, `:lang()` on `lang`, `:dir()` on `dir`.
    Attr {
        /// The name as written.
        name: &'a str,
        /// The lowercase name.
        lower: &'a str,
    },
    /// `:empty`: E is the element whose children changed.
    Empty,
    /// The structural pseudo-classes: E is an element child of the parent
    /// whose children changed.
    Structural,
    /// Any compound: E was inserted or removed.
    Presence,
}

/// The `:has()` anchors of a dependency: the elements found from E by
/// `kind` (one of the `HAS_*` flags) that have one of `keys` (any element
/// if empty) are the starting points of the dependency's reach.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Anchor<'a> {
    /// One `HAS_*` flag.
    pub kind: StateReach,
    /// The keys of the compound that holds `:has()`.
    pub keys: Vec<BucketKey<'a>>,
}

/// One dependency of a selector: a change of `key` on an element E that has
/// one of `element_keys` (any element if empty) can change the match of
/// the elements at `reach` from E (or, with an anchor, from each anchor)
/// that have `subject_key`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Dependency<'a> {
    /// What changed.
    pub key: DependencyKey<'a>,
    /// One flag: a position flag or [`StateReach::EVERYTHING`].
    pub reach: StateReach,
    /// The `:has()` anchors between E and the reach, if any.
    pub anchor: Option<Anchor<'a>>,
    /// E has one of these keys; empty means any element.
    pub element_keys: Vec<BucketKey<'a>>,
    /// The key of the selector's subject ([`BucketKey::Universal`] for
    /// none).
    pub subject_key: BucketKey<'a>,
}

/// A dependency as [`Selector::for_each_dependency`] lends it: the keys
/// are borrowed.
#[derive(Clone, Copy, Debug)]
pub struct DependencyView<'s, 'a> {
    /// What changed.
    pub key: DependencyKey<'a>,
    /// One flag: a position flag or [`StateReach::EVERYTHING`].
    pub reach: StateReach,
    /// The `:has()` anchors: their kind (one `HAS_*` flag) and keys.
    pub anchor: Option<(StateReach, &'s [BucketKey<'a>])>,
    /// E has one of these keys; empty means any element.
    pub element_keys: &'s [BucketKey<'a>],
}

/// Keys shared by the occurrences of one compound.
type Keys<'a> = SmallVec<[BucketKey<'a>; 2]>;

/// An occurrence: the dependency, its reach (relative to the subject of the
/// components it was found in), the keys of E and the anchor (kind and
/// keys).
#[derive(Clone, Debug)]
struct Occurrence<'a> {
    key: DependencyKey<'a>,
    reach: StateReach,
    keys: Keys<'a>,
    anchor: Option<(StateReach, Keys<'a>)>,
}

/// The reaches that combinator positions give (the first five flags).
const POSITIONS: StateReach = StateReach::SELF
    .union(StateReach::CHILDREN)
    .union(StateReach::DESCENDANTS)
    .union(StateReach::FOLLOWING_SIBLINGS)
    .union(StateReach::FOLLOWING_SIBLING_DESCENDANTS);

impl Selector {
    /// All dependencies of the selector (ADR 0042 section 1), one per
    /// dependency, reach, anchor and keys, without duplicates.
    pub fn dependencies(&self) -> Vec<Dependency<'_>> {
        let mut out: Vec<Dependency<'_>> = Vec::new();
        let subject_key = self.bucket_key();
        self.for_each_dependency(|d| {
            let dependency = Dependency {
                key: d.key,
                reach: d.reach,
                anchor: d.anchor.map(|(kind, keys)| Anchor {
                    kind,
                    keys: keys.to_vec(),
                }),
                element_keys: d.element_keys.to_vec(),
                subject_key,
            };
            if !out.contains(&dependency) {
                out.push(dependency);
            }
        });
        out
    }

    /// Calls `f` with each dependency of the selector (the subject key is
    /// [`Selector::bucket_key`]); the same one can come more than once.
    /// Without the allocations of [`Selector::dependencies`], for indexes of
    /// many selectors.
    pub fn for_each_dependency<'a>(&'a self, mut f: impl FnMut(DependencyView<'_, 'a>)) {
        let mut found = Vec::new();
        match self.shadow.as_deref() {
            None => occurrences(&self.components, &mut found),
            Some(ShadowPseudo::Slotted(argument)) => {
                occurrences(argument, &mut found);
                // The other compounds match the slot (or a slot that it is
                // assigned to, further up the flat tree) and its tree.
                let mut outer = Vec::new();
                occurrences(&self.components, &mut outer);
                found.extend(
                    outer
                        .into_iter()
                        .map(|o| compose(o, StateReach::DESCENDANTS)),
                );
            }
            Some(ShadowPseudo::Part { after, .. }) => {
                occurrences(after, &mut found);
                // The other compounds match the host of the part's tree.
                let mut outer = Vec::new();
                occurrences(&self.components, &mut outer);
                found.extend(
                    outer
                        .into_iter()
                        .map(|o| compose(o, StateReach::DESCENDANTS)),
                );
            }
        }
        for occurrence in &found {
            for flag in occurrence.reach.iter() {
                // An inserted or removed element restyles its own subtree
                // anyway: only the presence dependencies that reach outside
                // it count.
                if occurrence.key == DependencyKey::Presence
                    && occurrence.anchor.is_none()
                    && !matches!(
                        flag,
                        StateReach::FOLLOWING_SIBLINGS
                            | StateReach::FOLLOWING_SIBLING_DESCENDANTS
                            | StateReach::EVERYTHING
                    )
                {
                    continue;
                }
                f(DependencyView {
                    key: occurrence.key,
                    reach: flag,
                    anchor: occurrence
                        .anchor
                        .as_ref()
                        .map(|(kind, keys)| (*kind, &keys[..])),
                    element_keys: &occurrence.keys,
                });
            }
        }
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

/// `occurrence` followed by the reach `second`: an anchored occurrence
/// keeps its anchor and composes the reach after it.
fn compose(occurrence: Occurrence<'_>, second: StateReach) -> Occurrence<'_> {
    Occurrence {
        reach: then(occurrence.reach, second),
        ..occurrence
    }
}

/// The keys that an element matching `compound` has one of (empty: any):
/// the first ID, else the first class, else the type, as
/// [`Selector::bucket_key`]; else, for an `:is()` or `:where()` whose
/// arguments all have subject keys, those keys.
fn compound_keys(compound: &[Component]) -> Keys<'_> {
    let mut class = None;
    let mut local_name = None;
    for component in compound {
        match component {
            Component::Id(id) => return smallvec::smallvec![BucketKey::Id(id)],
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
        return smallvec::smallvec![BucketKey::Class(class)];
    }
    if let Some(key) = local_name {
        return smallvec::smallvec![key];
    }
    for component in compound {
        if let Component::PseudoClass(PseudoClass::Is(list) | PseudoClass::Where(list)) = component
        {
            let keys: Keys<'_> = list.iter().map(Selector::bucket_key).collect();
            if !keys.is_empty() && !keys.contains(&BucketKey::Universal) {
                return keys;
            }
        }
    }
    SmallVec::new()
}

/// Appends the occurrences in the right-to-left `components`, with reaches
/// relative to their subject.
fn occurrences<'a>(components: &'a [Component], out: &mut Vec<Occurrence<'a>>) {
    let (mut compound, mut rest) = split_compound(components);
    let mut combinators: Vec<Combinator> = Vec::new();
    loop {
        let here = position(&combinators);
        let keys: Keys<'a> = compound_keys(compound);
        plain(out, DependencyKey::Presence, here, keys.clone());
        // An element inserted or removed between this compound's element
        // and the next sibling changes `+`, whatever it matches.
        if combinators.last() == Some(&Combinator::NextSibling) {
            plain(out, DependencyKey::Presence, here, no_keys());
        }
        for component in compound {
            match component {
                // The changed name is the key of the index: E has it (or
                // had it); the index does not look at the element keys.
                Component::Class(c) => plain(out, DependencyKey::Class(c), here, keys.clone()),
                Component::Id(id) => plain(out, DependencyKey::Id(id), here, keys.clone()),
                Component::Attribute(a) => plain(
                    out,
                    DependencyKey::Attr {
                        name: &a.name.name,
                        lower: a.name.lower(),
                    },
                    here,
                    keys.clone(),
                ),
                Component::PseudoClass(pc) => pseudo_class(pc, here, &keys, out),
                _ => {}
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

/// No keys: any element.
fn no_keys<'a>() -> Keys<'a> {
    SmallVec::new()
}

/// Appends an occurrence without an anchor.
fn plain<'a>(
    out: &mut Vec<Occurrence<'a>>,
    key: DependencyKey<'a>,
    reach: StateReach,
    keys: Keys<'a>,
) {
    out.push(Occurrence {
        key,
        reach,
        keys,
        anchor: None,
    });
}

/// The attribute key of a pseudo-class that reads an attribute.
fn attribute_key(name: &'static str) -> DependencyKey<'static> {
    DependencyKey::Attr { name, lower: name }
}

/// The occurrences of one pseudo-class of a compound at position `here`
/// whose element has `keys`.
fn pseudo_class<'a>(
    pc: &'a PseudoClass,
    here: StateReach,
    keys: &Keys<'a>,
    out: &mut Vec<Occurrence<'a>>,
) {
    // The nearest `lang` or `dir` of an ancestor-or-self decides: E is the
    // element or one of its ancestors.
    let inherited = then(StateReach::SELF | StateReach::DESCENDANTS, here);
    match pc {
        PseudoClass::State(s) => plain(out, DependencyKey::State(*s), here, keys.clone()),
        PseudoClass::Link | PseudoClass::Visited | PseudoClass::AnyLink => {
            plain(out, attribute_key("href"), here, keys.clone());
        }
        PseudoClass::Lang(_) => plain(out, attribute_key("lang"), inherited, no_keys()),
        PseudoClass::Dir(_) => plain(out, attribute_key("dir"), inherited, no_keys()),
        PseudoClass::Empty => plain(out, DependencyKey::Empty, here, keys.clone()),
        PseudoClass::FirstChild
        | PseudoClass::LastChild
        | PseudoClass::OnlyChild
        | PseudoClass::FirstOfType
        | PseudoClass::LastOfType
        | PseudoClass::OnlyOfType
        | PseudoClass::NthOfType(_)
        | PseudoClass::NthLastOfType(_) => {
            plain(out, DependencyKey::Structural, here, keys.clone());
        }
        PseudoClass::NthChild(nth) | PseudoClass::NthLastChild(nth) => {
            plain(out, DependencyKey::Structural, here, keys.clone());
            // A change in a sibling changes the index of E's siblings in
            // both directions.
            for selector in nth.of.iter().flatten() {
                let mut inner = Vec::new();
                occurrences(&selector.components, &mut inner);
                out.extend(inner.into_iter().map(|o| Occurrence {
                    key: o.key,
                    reach: StateReach::EVERYTHING,
                    keys: o.keys,
                    anchor: None,
                }));
            }
        }
        PseudoClass::Is(list) | PseudoClass::Where(list) | PseudoClass::Not(list) => {
            for selector in list {
                argument(selector, here, keys, out);
            }
        }
        PseudoClass::Host(Some(selector)) => argument(selector, here, keys, out),
        PseudoClass::HostContext(selector) => {
            // E is the host or one of its flat ancestors.
            let mut inner = Vec::new();
            occurrences(&selector.components, &mut inner);
            for o in inner {
                let reach = if o.reach == StateReach::SELF && o.anchor.is_none() {
                    then(StateReach::SELF | StateReach::DESCENDANTS, here)
                } else {
                    StateReach::EVERYTHING
                };
                out.push(Occurrence {
                    key: o.key,
                    reach,
                    keys: o.keys,
                    anchor: None,
                });
            }
        }
        PseudoClass::Has(relatives) => {
            for relative in relatives {
                has(
                    relative.combinator,
                    &relative.selector.components,
                    here,
                    keys,
                    out,
                );
            }
        }
        PseudoClass::Root | PseudoClass::Scope | PseudoClass::Host(None) => {}
    }
}

/// The occurrences of an argument selector, whose subject matches the
/// compound's element: at the argument's subject, E is that element, so the
/// compound's keys apply too.
fn argument<'a>(
    selector: &'a Selector,
    here: StateReach,
    keys: &Keys<'a>,
    out: &mut Vec<Occurrence<'a>>,
) {
    let mut inner = Vec::new();
    occurrences(&selector.components, &mut inner);
    for o in inner {
        let keys = if o.reach == StateReach::SELF && o.anchor.is_none() && o.keys.is_empty() {
            keys.clone()
        } else {
            o.keys.clone()
        };
        let o = compose(o, here);
        out.push(Occurrence { keys, ..o });
    }
}

/// The occurrences inside `:has(leading components)` of a compound at
/// position `here` with keys `keys` (ADR 0042 section 1, "`:has()`,
/// revised"). Each chain element is below the anchor (a down step first) or
/// below or at a following sibling of it (a sibling step first), whatever
/// compound of the relative selector it matches; so a dependency directly
/// in a compound has anchors among E's ancestors or among the preceding
/// siblings of E's inclusive ancestors. A dependency inside an argument at
/// another position can be outside the anchor's subtree: everything.
fn has<'a>(
    leading: Combinator,
    components: &'a [Component],
    here: StateReach,
    keys: &Keys<'a>,
    out: &mut Vec<Occurrence<'a>>,
) {
    let all_siblings = !is_down(leading)
        && !components
            .iter()
            .any(|c| matches!(c, Component::Combinator(c) if is_down(*c)));
    // An element inserted or removed next to a chain element changes a
    // `+` step, whatever it matches.
    let adjacent = leading == Combinator::NextSibling
        || components
            .iter()
            .any(|c| matches!(c, Component::Combinator(Combinator::NextSibling)));
    if adjacent {
        out.push(Occurrence {
            key: DependencyKey::Presence,
            reach: here,
            keys: no_keys(),
            anchor: Some((
                if is_down(leading) {
                    StateReach::HAS_ANCESTORS
                } else {
                    StateReach::HAS_PRECEDING_SIBLINGS_OF_ANCESTORS
                },
                keys.clone(),
            )),
        });
    }
    let (mut compound, mut rest) = split_compound(components);
    let mut subject = true;
    loop {
        let mut inner = Vec::new();
        occurrences(compound, &mut inner);
        let kind = if is_down(leading) {
            StateReach::HAS_ANCESTORS
        } else if subject && all_siblings {
            StateReach::HAS_PRECEDING_SIBLINGS
        } else {
            StateReach::HAS_PRECEDING_SIBLINGS_OF_ANCESTORS
        };
        for o in inner {
            if o.reach == StateReach::SELF && o.anchor.is_none() {
                out.push(Occurrence {
                    key: o.key,
                    reach: here,
                    keys: o.keys,
                    anchor: Some((kind, keys.clone())),
                });
            } else {
                out.push(Occurrence {
                    key: o.key,
                    reach: StateReach::EVERYTHING,
                    keys: o.keys,
                    anchor: None,
                });
            }
        }
        subject = false;
        match rest.split_first() {
            Some((Component::Combinator(_), after)) => {
                (compound, rest) = split_compound(after);
            }
            _ => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selector::SelectorList;

    type R = StateReach;

    fn key(k: &BucketKey<'_>) -> String {
        match k {
            BucketKey::Id(id) => format!("#{id}"),
            BucketKey::Class(c) => format!(".{c}"),
            BucketKey::LocalName { lower_name, .. } => (*lower_name).to_owned(),
            BucketKey::Universal => "*".to_owned(),
        }
    }

    /// The dependencies of the first selector of `css` on `on`, as (reach,
    /// element keys, subject key) with keys written as `#id`, `.class`,
    /// `name` or `*`; an anchored dependency's reach is written as the
    /// anchor kind for a reach of the anchor itself, else as its reach
    /// after the anchor (see `anchored`).
    fn deps_on(css: &str, on: DependencyKey<'_>) -> Vec<(R, Vec<String>, String)> {
        let list = SelectorList::parse_str(css).expect("valid selector");
        let selector = &list.selectors()[0];
        selector
            .dependencies()
            .iter()
            .filter(|d| d.key == on)
            .map(|d| {
                let reach = match &d.anchor {
                    Some(a) if d.reach == R::SELF => a.kind,
                    _ => d.reach,
                };
                (
                    reach,
                    d.element_keys.iter().map(key).collect(),
                    key(&d.subject_key),
                )
            })
            .collect()
    }

    fn deps(css: &str) -> Vec<(R, Vec<String>, String)> {
        deps_on(css, DependencyKey::State(ElementState::HOVER))
    }

    /// The anchored dependencies on `on`: (anchor kind, anchor keys, reach
    /// after the anchors, subject key).
    fn anchored(css: &str, on: DependencyKey<'_>) -> Vec<(R, Vec<String>, R, String)> {
        let list = SelectorList::parse_str(css).expect("valid selector");
        list.selectors()[0]
            .dependencies()
            .iter()
            .filter(|d| d.key == on)
            .filter_map(|d| {
                let a = d.anchor.as_ref()?;
                Some((
                    a.kind,
                    a.keys.iter().map(key).collect(),
                    d.reach,
                    key(&d.subject_key),
                ))
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
        // ADR 0042: a dependency in any compound of the relative selector
        // has its anchors among E's ancestors (a down step first)...
        assert_eq!(
            deps(".a:has(.c:hover .b)"),
            one(R::HAS_ANCESTORS, &[".c"], ".a")
        );
        // ...or among the preceding siblings of E's inclusive ancestors (a
        // sibling step first, not at the subject or not only siblings).
        assert_eq!(
            deps(".a:has(+ .c:hover + .b)"),
            one(R::HAS_PRECEDING_SIBLINGS_OF_ANCESTORS, &[".c"], ".a")
        );
        // `:has()` in a compound that is not the subject: the anchors with
        // the anchor key start the compound's position.
        assert_eq!(
            anchored(
                ".a:has(:hover) .d",
                DependencyKey::State(ElementState::HOVER)
            ),
            vec![(
                R::HAS_ANCESTORS,
                vec![".a".to_owned()],
                R::DESCENDANTS,
                ".d".to_owned()
            )]
        );
        assert_eq!(
            anchored(
                ".a:has(:hover) > .d",
                DependencyKey::State(ElementState::HOVER)
            ),
            vec![(
                R::HAS_ANCESTORS,
                vec![".a".to_owned()],
                R::CHILDREN,
                ".d".to_owned()
            )]
        );
        assert_eq!(
            anchored(
                ".a:has(+ .b:hover) ~ .d",
                DependencyKey::State(ElementState::HOVER)
            ),
            vec![(
                R::HAS_PRECEDING_SIBLINGS,
                vec![".a".to_owned()],
                R::FOLLOWING_SIBLINGS,
                ".d".to_owned()
            )]
        );
        // A dependency inside an argument with a combinator can be above
        // the anchor: not analysed.
        assert_eq!(
            deps(".a:has(:is(.c:hover .b))"),
            one(R::EVERYTHING, &[".c"], ".a")
        );
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
        let on = |s| DependencyKey::State(s);
        assert_eq!(
            deps_on(".a:focus-visible .b", on(ElementState::FOCUS_VISIBLE)),
            one(R::DESCENDANTS, &[".a"], ".b")
        );
        assert_eq!(
            deps_on(".a:focus-visible .b", on(ElementState::FOCUS)),
            vec![]
        );
        assert_eq!(
            deps_on(".c:focus-within", on(ElementState::FOCUS_WITHIN)),
            one(R::SELF, &[".c"], ".c")
        );
        assert_eq!(
            deps_on("input:checked + label", on(ElementState::CHECKED)),
            one(R::FOLLOWING_SIBLINGS, &["input"], "label")
        );
    }

    #[test]
    fn classes_ids_and_attributes() {
        // The element keys of a class or ID are the compound's (the index
        // does not use them: the name itself is the key).
        assert_eq!(
            deps_on(".a .b", DependencyKey::Class("a")),
            one(R::DESCENDANTS, &[".a"], ".b")
        );
        assert_eq!(
            deps_on(".a .b", DependencyKey::Class("b")),
            one(R::SELF, &[".b"], ".b")
        );
        assert_eq!(
            deps_on("#x ~ p", DependencyKey::Id("x")),
            one(R::FOLLOWING_SIBLINGS, &["#x"], "p")
        );
        let attr = |name| DependencyKey::Attr { name, lower: name };
        assert_eq!(
            deps_on("button[aria-expanded=true] > .m", attr("aria-expanded")),
            one(R::CHILDREN, &["button"], ".m")
        );
        assert_eq!(
            deps_on("a:link .i", attr("href")),
            one(R::DESCENDANTS, &["a"], ".i")
        );
        // `:lang()` and `:dir()` read the nearest attribute up the tree.
        assert_eq!(
            deps_on("p:lang(en)", attr("lang")),
            vec![
                (R::SELF, vec![], "p".to_owned()),
                (R::DESCENDANTS, vec![], "p".to_owned()),
            ]
        );
        assert_eq!(
            deps_on(":dir(rtl) > .x", attr("dir")),
            vec![
                (R::CHILDREN, vec![], ".x".to_owned()),
                (R::DESCENDANTS, vec![], ".x".to_owned()),
            ]
        );
        // The name as written and in lowercase.
        let list = SelectorList::parse_str("[Data-X]").expect("valid");
        assert!(list.selectors()[0].dependencies().iter().any(|d| d.key
            == DependencyKey::Attr {
                name: "Data-X",
                lower: "data-x"
            }));
    }

    #[test]
    fn structure_and_presence() {
        assert_eq!(
            deps_on("li:nth-child(2n) .x", DependencyKey::Structural),
            one(R::DESCENDANTS, &["li"], ".x")
        );
        assert_eq!(
            deps_on(".a:first-child", DependencyKey::Structural),
            one(R::SELF, &[".a"], ".a")
        );
        assert_eq!(
            deps_on("p:empty", DependencyKey::Empty),
            one(R::SELF, &["p"], "p")
        );
        // Each compound is a presence dependency at its position.
        assert_eq!(
            deps_on(".a + .b", DependencyKey::Presence),
            // Only the positions outside the inserted subtree; any element
            // between them changes `+`.
            vec![
                (
                    R::FOLLOWING_SIBLINGS,
                    vec![".a".to_owned()],
                    ".b".to_owned()
                ),
                (R::FOLLOWING_SIBLINGS, vec![], ".b".to_owned()),
            ]
        );
        // The compounds of a relative selector anchor at the ancestors.
        assert_eq!(
            anchored(
                "html:has(link[rel=canonical]) #o .t",
                DependencyKey::Presence
            ),
            vec![(
                R::HAS_ANCESTORS,
                vec!["html".to_owned()],
                R::DESCENDANTS,
                ".t".to_owned()
            )]
        );
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
