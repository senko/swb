//! An ancestor Bloom filter for fast rejection of descendant selectors.
//!
//! While the cascade walks the tree, a counting Bloom filter holds the
//! hashes of the IDs, classes and local names of the ancestors of the
//! element being styled. Each rule stores the hashes of up to
//! [`MAX_ANCESTOR_HASHES`] simple selectors that an ancestor must match
//! (from compound selectors left of a descendant or child combinator). If
//! the filter does not contain one of them, the rule cannot match and the
//! full selector match is skipped.
//!
//! The filter never gives false negatives, so it only affects speed. The
//! idea is the same as in Servo and Blink (`SelectorFilter`); the code is
//! our own.
//!
//! The css crate does not expose selector components, so the ancestor
//! keys are read from the selector's serialization, which has one fixed
//! form (CSSOM "serialize a selector"): compound selectors separated by
//! `" "`, `" > "`, `" + "` or `" ~ "`.

use swb_css::{BlockKind, ComponentValue, Selector, parse_component_values};
use swb_dom::ElementData;

/// The number of counters (a power of two).
const SIZE: usize = 1 << 12;
const MASK: u32 = SIZE as u32 - 1;

/// The maximum number of ancestor hashes stored per rule.
pub(crate) const MAX_ANCESTOR_HASHES: usize = 4;

/// A counting Bloom filter with two hash functions derived from one 32-bit
/// hash.
pub(crate) struct AncestorFilter {
    counters: Vec<u8>,
}

impl AncestorFilter {
    pub(crate) fn new() -> Self {
        AncestorFilter {
            counters: vec![0; SIZE],
        }
    }

    fn indices(hash: u32) -> [usize; 2] {
        [(hash & MASK) as usize, ((hash >> 12) & MASK) as usize]
    }

    fn insert_hash(&mut self, hash: u32) {
        for i in Self::indices(hash) {
            // A saturated counter stays saturated (it may be shared by
            // more elements than it can count).
            self.counters[i] = self.counters[i].saturating_add(1);
        }
    }

    fn remove_hash(&mut self, hash: u32) {
        for i in Self::indices(hash) {
            let c = &mut self.counters[i];
            if *c != u8::MAX {
                *c = c.saturating_sub(1);
            }
        }
    }

    /// True if the hash may have been inserted.
    pub(crate) fn might_contain(&self, hash: u32) -> bool {
        Self::indices(hash).iter().all(|&i| self.counters[i] != 0)
    }

    /// Adds the keys of an element (when its children are styled).
    pub(crate) fn push_element(&mut self, e: &ElementData, quirks: bool) {
        for_each_element_hash(e, quirks, |h| self.insert_hash(h));
    }

    /// Removes the keys of an element added with [`Self::push_element`].
    pub(crate) fn pop_element(&mut self, e: &ElementData, quirks: bool) {
        for_each_element_hash(e, quirks, |h| self.remove_hash(h));
    }
}

/// The kind of a key, mixed into the hash so that `#a`, `.a` and `a`
/// differ.
#[derive(Clone, Copy)]
enum Kind {
    Id = b'#' as isize,
    Class = b'.' as isize,
    Tag = b't' as isize,
}

/// FNV-1a over the kind and the bytes of `key`, optionally ASCII
/// lowercased.
fn hash(kind: Kind, key: &str, lowercase: bool) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in std::iter::once(kind as u8).chain(key.bytes()) {
        let b = if lowercase { b.to_ascii_lowercase() } else { b };
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// Calls `f` with the hash of each key of an element: its local name
/// (lowercased), ID and classes (lowercased in quirks mode, where they
/// match case-insensitively).
fn for_each_element_hash(e: &ElementData, quirks: bool, mut f: impl FnMut(u32)) {
    f(hash(Kind::Tag, e.local_name(), true));
    if let Some(id) = e.id() {
        f(hash(Kind::Id, id, quirks));
    }
    for class in e.classes() {
        f(hash(Kind::Class, class, quirks));
    }
}

/// The hashes of simple selectors that some ancestor of the subject must
/// match (at most [`MAX_ANCESTOR_HASHES`]; IDs first, then classes, then
/// local names).
pub(crate) fn ancestor_hashes(selector: &Selector, quirks: bool) -> Vec<u32> {
    let text = selector.to_string();
    let tokens = parse_component_values(&text);
    let (compounds, combinators) = split_compounds(&tokens);
    let mut ids = Vec::new();
    let mut classes = Vec::new();
    let mut tags = Vec::new();
    // compounds[i] is an ancestor of the subject if the combinator to its
    // right is a descendant or child combinator.
    for (compound, combinator) in compounds.iter().zip(&combinators) {
        if !matches!(combinator, Combinator::Descendant | Combinator::Child) {
            continue;
        }
        let mut previous: Option<&ComponentValue> = None;
        for (j, token) in compound.iter().enumerate() {
            let after_colon = matches!(previous, Some(ComponentValue::Colon));
            match token {
                ComponentValue::Hash { value, .. } => ids.push(hash(Kind::Id, value, quirks)),
                ComponentValue::Ident(name) if matches!(previous, Some(v) if v.is_delim('.')) => {
                    classes.push(hash(Kind::Class, name, quirks));
                }
                ComponentValue::Ident(name) if j == 0 && !after_colon => {
                    tags.push(hash(Kind::Tag, name, true));
                }
                _ => {}
            }
            previous = Some(token);
        }
    }
    ids.into_iter()
        .chain(classes)
        .chain(tags)
        .take(MAX_ANCESTOR_HASHES)
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Combinator {
    Descendant,
    Child,
    Sibling,
}

/// Splits serialized selector tokens into compound selectors (left to
/// right) and the combinator to the right of each compound except the
/// last.
fn split_compounds(tokens: &[ComponentValue]) -> (Vec<&[ComponentValue]>, Vec<Combinator>) {
    let mut compounds = Vec::new();
    let mut combinators = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];
        let combinator = match token {
            ComponentValue::Whitespace => Some(Combinator::Descendant),
            ComponentValue::Delim('>') => Some(Combinator::Child),
            ComponentValue::Delim('+' | '~') => Some(Combinator::Sibling),
            _ => None,
        };
        let Some(mut combinator) = combinator else {
            i += 1;
            continue;
        };
        if start < i {
            compounds.push(&tokens[start..i]);
        }
        // Merge `" > "` into one combinator.
        let mut j = i;
        while j < tokens.len() {
            match &tokens[j] {
                ComponentValue::Whitespace => {}
                ComponentValue::Delim('>') => combinator = Combinator::Child,
                ComponentValue::Delim('+' | '~') => combinator = Combinator::Sibling,
                _ => break,
            }
            j += 1;
        }
        combinators.push(combinator);
        start = j;
        i = j;
    }
    if start < tokens.len() {
        compounds.push(&tokens[start..]);
    }
    // Blocks never appear at the top level of a serialized selector; a
    // malformed split must not produce wrong keys.
    if compounds.len() != combinators.len() + 1
        || tokens
            .iter()
            .any(|t| matches!(t, ComponentValue::Block(b) if b.kind == BlockKind::Curly))
    {
        return (Vec::new(), Vec::new());
    }
    (compounds, combinators)
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_css::SelectorList;

    fn hashes(selector: &str) -> Vec<u32> {
        let list = SelectorList::parse_str(selector).expect("valid selector");
        ancestor_hashes(&list.selectors()[0], false)
    }

    #[test]
    fn keys_from_ancestor_compounds() {
        assert_eq!(hashes(".a"), Vec::<u32>::new());
        assert_eq!(hashes(".a .b"), vec![hash(Kind::Class, "a", false)]);
        assert_eq!(
            hashes("div#x.y > p.z"),
            vec![
                hash(Kind::Id, "x", false),
                hash(Kind::Class, "y", false),
                hash(Kind::Tag, "div", true)
            ]
        );
        // Siblings are not ancestors; the ancestor of a sibling is.
        assert_eq!(hashes(".s + .t"), Vec::<u32>::new());
        assert_eq!(hashes(".a .s ~ .t"), vec![hash(Kind::Class, "a", false)]);
        // Arguments of functional pseudo-classes are not required.
        assert_eq!(hashes(":is(.a, .b) .c"), Vec::<u32>::new());
        assert_eq!(hashes(".a:not(.b) .c"), vec![hash(Kind::Class, "a", false)]);
        assert_eq!(hashes("[data-x='a b'] .c"), Vec::<u32>::new());
        assert_eq!(hashes("a:hover .c"), vec![hash(Kind::Tag, "a", true)]);
        // Escaped class names are unescaped.
        assert_eq!(hashes(r".a\:b .c"), vec![hash(Kind::Class, "a:b", false)]);
        assert_eq!(hashes(".a .b .c .d .e .f").len(), MAX_ANCESTOR_HASHES);
    }

    #[test]
    fn filter_counts() {
        let doc = swb_dom::parse_html("<div id=X class='a b'></div>");
        let id = doc.element_by_id("X").expect("div");
        let e = doc.element(id).expect("element");
        let mut f = AncestorFilter::new();
        let a = hash(Kind::Class, "a", false);
        assert!(!f.might_contain(a));
        f.push_element(e, false);
        f.push_element(e, false);
        assert!(f.might_contain(a));
        assert!(f.might_contain(hash(Kind::Tag, "div", true)));
        assert!(f.might_contain(hash(Kind::Id, "X", false)));
        f.pop_element(e, false);
        assert!(f.might_contain(a));
        f.pop_element(e, false);
        assert!(!f.might_contain(a));
        // Quirks mode folds case.
        f.push_element(e, true);
        assert!(f.might_contain(hash(Kind::Id, "x", true)));
    }
}
