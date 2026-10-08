//! Selector specificity.
//!
//! <https://www.w3.org/TR/selectors-4/#specificity-rules>

use std::fmt;

use super::{Component, PseudoClass, SelectorList};

/// The specificity of a selector: (IDs, classes, types).
///
/// Packed into one `u32` with 10 bits per part, so comparison is one
/// integer comparison. Each part saturates at 1023.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Specificity(u32);

const MAX_PART: u32 = (1 << 10) - 1;

impl Specificity {
    /// Creates a specificity from its three parts (each clamped to 1023).
    pub fn new(ids: u32, classes: u32, types: u32) -> Self {
        Specificity((ids.min(MAX_PART) << 20) | (classes.min(MAX_PART) << 10) | types.min(MAX_PART))
    }

    /// The number of ID selectors (A).
    pub fn ids(self) -> u32 {
        self.0 >> 20
    }

    /// The number of class selectors, attribute selectors and
    /// pseudo-classes (B).
    pub fn classes(self) -> u32 {
        (self.0 >> 10) & MAX_PART
    }

    /// The number of type selectors and pseudo-elements (C).
    pub fn types(self) -> u32 {
        self.0 & MAX_PART
    }

    /// The packed value. Larger means more specific.
    pub fn value(self) -> u32 {
        self.0
    }

    fn add(self, other: Specificity) -> Specificity {
        Specificity::new(
            self.ids() + other.ids(),
            self.classes() + other.classes(),
            self.types() + other.types(),
        )
    }
}

impl fmt::Display for Specificity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {}, {})", self.ids(), self.classes(), self.types())
    }
}

const ID: Specificity = Specificity(1 << 20);
const CLASS: Specificity = Specificity(1 << 10);
const TYPE: Specificity = Specificity(1);

/// Computes the specificity of a selector from its components.
pub(crate) fn compute(components: &[Component], has_pseudo_element: bool) -> Specificity {
    let mut total = if has_pseudo_element {
        TYPE
    } else {
        Specificity::default()
    };
    for component in components {
        let part = match component {
            Component::Combinator(_) | Component::Universal => continue,
            Component::Id(_) => ID,
            Component::Class(_) | Component::Attribute(_) => CLASS,
            Component::LocalName(_) => TYPE,
            Component::PseudoClass(pc) => pseudo_class(pc),
        };
        total = total.add(part);
    }
    total
}

fn pseudo_class(pc: &PseudoClass) -> Specificity {
    match pc {
        PseudoClass::Is(list) | PseudoClass::Not(list) => max_of(list),
        PseudoClass::Has(relative) => relative
            .iter()
            .map(|r| r.selector.specificity())
            .max()
            .unwrap_or_default(),
        PseudoClass::Where(_) => Specificity::default(),
        // The pseudo-class plus its argument.
        PseudoClass::Host(Some(arg)) | PseudoClass::HostContext(arg) => {
            CLASS.add(arg.specificity())
        }
        PseudoClass::NthChild(nth) | PseudoClass::NthLastChild(nth) => {
            CLASS.add(nth.of.as_ref().map(max_of).unwrap_or_default())
        }
        _ => CLASS,
    }
}

/// The specificity of the most specific selector in the list.
fn max_of(list: &SelectorList) -> Specificity {
    list.iter()
        .map(super::Selector::specificity)
        .max()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packing_and_order() {
        let s = Specificity::new(1, 2, 3);
        assert_eq!((s.ids(), s.classes(), s.types()), (1, 2, 3));
        assert_eq!(s.to_string(), "(1, 2, 3)");
        assert!(Specificity::new(1, 0, 0) > Specificity::new(0, 1023, 1023));
        assert!(Specificity::new(0, 1, 0) > Specificity::new(0, 0, 1023));
        assert_eq!(Specificity::new(5000, 0, 0).ids(), 1023);
        assert_eq!(
            Specificity::new(0, 1023, 0)
                .add(Specificity::new(0, 5, 0))
                .classes(),
            1023
        );
    }
}
