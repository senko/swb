//! Selectors Level 4: parsing, specificity and matching.
//!
//! <https://www.w3.org/TR/selectors-4/>
//!
//! A [`Selector`] stores its simple selectors and combinators in one slice,
//! right to left, because matching starts at the subject (the rightmost
//! compound selector). The pseudo-element, if any, is stored separately:
//! matching checks the originating element, and the style crate applies the
//! rule to the pseudo-element.
//!
//! Namespaces: `@namespace` is not supported, so every namespace prefix is
//! undeclared and makes a selector invalid, as the specification requires.
//! `*|E` is the same as `E` (any namespace). `|E` (no namespace) is
//! rejected. In attribute selectors, `[|a]` is the same as `[a]`, and
//! `[*|a]` is treated as `[a]` (an approximation: it ignores namespaced
//! attributes).

mod anb;
mod display;
mod matching;
mod parse;
mod specificity;

pub use anb::AnB;
pub use matching::{
    CaseSensitivity, Element, ElementState, MATCH_BUDGET, MatchingContext, QuirksMode, matches,
    matches_any, matches_with_scope,
};
pub use specificity::Specificity;

use crate::cursor::ParseError;
use crate::values::ComponentValue;

/// A comma-separated list of complex selectors.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectorList {
    selectors: Vec<Selector>,
}

impl SelectorList {
    /// Parses a selector list, for example the prelude of a style rule.
    /// Fails if any selector in the list is invalid.
    pub fn parse(input: &[ComponentValue]) -> Result<SelectorList, ParseError> {
        parse::parse_selector_list(input)
    }

    /// Parses a selector list from text, for example for `querySelector`.
    pub fn parse_str(css: &str) -> Result<SelectorList, ParseError> {
        Self::parse(&crate::parse_component_values(css))
    }

    /// The selectors.
    pub fn selectors(&self) -> &[Selector] {
        &self.selectors
    }

    /// An iterator over the selectors.
    pub fn iter(&self) -> std::slice::Iter<'_, Selector> {
        self.selectors.iter()
    }

    /// The number of selectors.
    pub fn len(&self) -> usize {
        self.selectors.len()
    }

    /// True if the list is empty. Only a forgiving list (inside `:is()` or
    /// `:where()`) can be empty.
    pub fn is_empty(&self) -> bool {
        self.selectors.is_empty()
    }
}

impl<'a> IntoIterator for &'a SelectorList {
    type Item = &'a Selector;
    type IntoIter = std::slice::Iter<'a, Selector>;

    fn into_iter(self) -> Self::IntoIter {
        self.selectors.iter()
    }
}

/// A complex selector, for example `nav > a.active:hover::before`.
#[derive(Clone, Debug, PartialEq)]
pub struct Selector {
    /// Simple selectors and combinators, right to left. Compound selectors
    /// are separated by [`Component::Combinator`]; inside a compound, the
    /// order is as written.
    components: Box<[Component]>,
    pseudo_element: Option<PseudoElement>,
    specificity: Specificity,
}

impl Selector {
    pub(crate) fn new(components: Vec<Component>, pseudo_element: Option<PseudoElement>) -> Self {
        let specificity = specificity::compute(&components, pseudo_element.is_some());
        Selector {
            components: components.into_boxed_slice(),
            pseudo_element,
            specificity,
        }
    }

    /// The specificity.
    /// <https://www.w3.org/TR/selectors-4/#specificity-rules>
    pub fn specificity(&self) -> Specificity {
        self.specificity
    }

    /// The pseudo-element at the end of the selector, if any. Matching
    /// ignores it and checks the originating element.
    pub fn pseudo_element(&self) -> Option<PseudoElement> {
        self.pseudo_element
    }

    /// A key for rule lookup tables, from the rightmost compound selector:
    /// its first ID selector, else its first class selector, else its type
    /// selector, else [`BucketKey::Universal`]. `:is()` and `:where()`
    /// arguments are ignored.
    ///
    /// An element can match the selector only if it has the ID, the class,
    /// or the local name of the key. In quirks mode, IDs and classes match
    /// ASCII case-insensitively, so look them up in lowercase there.
    pub fn bucket_key(&self) -> BucketKey<'_> {
        let subject = matching::split_compound(&self.components).0;
        let mut class = None;
        let mut local_name = None;
        for component in subject {
            match component {
                Component::Id(id) => return BucketKey::Id(id),
                Component::Class(c) if class.is_none() => class = Some(&**c),
                Component::LocalName(name) if local_name.is_none() => local_name = Some(name),
                _ => {}
            }
        }
        if let Some(class) = class {
            BucketKey::Class(class)
        } else if let Some(name) = local_name {
            BucketKey::LocalName {
                name: &name.name,
                lower_name: name.lower(),
            }
        } else {
            BucketKey::Universal
        }
    }

    /// The element state flags that the selector depends on, including
    /// inside `:is()`, `:not()`, `:has()` and `:nth-child(... of S)`.
    /// `:link` and `:visited` add [`ElementState::VISITED`]. The style crate
    /// can skip restyling when other flags change.
    pub fn state_dependencies(&self) -> ElementState {
        state_dependencies(&self.components)
    }

    /// True if the selector matches `element`. See [`matches()`].
    pub fn matches<E: Element>(&self, element: &E, context: &mut MatchingContext) -> bool {
        matches(self, element, context)
    }
}

fn state_dependencies(components: &[Component]) -> ElementState {
    let of_list = |list: &SelectorList| {
        list.iter()
            .fold(ElementState::empty(), |acc, s| acc | s.state_dependencies())
    };
    components
        .iter()
        .fold(ElementState::empty(), |acc, component| {
            let Component::PseudoClass(pc) = component else {
                return acc;
            };
            acc | match pc {
                PseudoClass::State(state) => *state,
                PseudoClass::Link | PseudoClass::Visited => ElementState::VISITED,
                PseudoClass::Not(list) | PseudoClass::Is(list) | PseudoClass::Where(list) => {
                    of_list(list)
                }
                PseudoClass::NthChild(nth) | PseudoClass::NthLastChild(nth) => {
                    nth.of.as_ref().map_or(ElementState::empty(), of_list)
                }
                PseudoClass::Has(relative) => {
                    relative.iter().fold(ElementState::empty(), |acc, r| {
                        acc | r.selector.state_dependencies()
                    })
                }
                _ => ElementState::empty(),
            }
        })
}

/// A rule lookup key. See [`Selector::bucket_key`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BucketKey<'a> {
    /// An ID, as written.
    Id(&'a str),
    /// A class name, as written.
    Class(&'a str),
    /// A local name.
    LocalName {
        /// The name as written. It applies to non-HTML elements.
        name: &'a str,
        /// The name in ASCII lowercase. It applies to HTML elements.
        lower_name: &'a str,
    },
    /// No key: the selector can match any element.
    Universal,
}

/// A combinator between two compound selectors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Combinator {
    /// Whitespace: `a b`.
    Descendant,
    /// `a > b`.
    Child,
    /// `a + b`.
    NextSibling,
    /// `a ~ b`.
    SubsequentSibling,
}

/// A pseudo-element.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PseudoElement {
    /// `::before`.
    Before,
    /// `::after`.
    After,
    /// `::marker`.
    Marker,
    /// `::first-line`.
    FirstLine,
    /// `::first-letter`.
    FirstLetter,
    /// `::placeholder`.
    Placeholder,
    /// `::selection`.
    Selection,
    /// `::backdrop`.
    Backdrop,
    /// `::file-selector-button`.
    FileSelectorButton,
    /// `::target-text`.
    TargetText,
    /// `::spelling-error`.
    SpellingError,
    /// `::grammar-error`.
    GrammarError,
}

impl PseudoElement {
    const ALL: [(&'static str, PseudoElement); 12] = [
        ("before", PseudoElement::Before),
        ("after", PseudoElement::After),
        ("marker", PseudoElement::Marker),
        ("first-line", PseudoElement::FirstLine),
        ("first-letter", PseudoElement::FirstLetter),
        ("placeholder", PseudoElement::Placeholder),
        ("selection", PseudoElement::Selection),
        ("backdrop", PseudoElement::Backdrop),
        ("file-selector-button", PseudoElement::FileSelectorButton),
        ("target-text", PseudoElement::TargetText),
        ("spelling-error", PseudoElement::SpellingError),
        ("grammar-error", PseudoElement::GrammarError),
    ];

    /// Looks up a pseudo-element by name (without `::`), ASCII
    /// case-insensitively.
    pub fn from_name(name: &str) -> Option<PseudoElement> {
        Self::ALL
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|&(_, pe)| pe)
    }

    /// The name, without `::`.
    pub fn name(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|&&(_, pe)| pe == self)
            .map_or("", |&(n, _)| n)
    }

    /// True for the pseudo-elements that also have a legacy single-colon
    /// form (`:before`, `:after`, `:first-line`, `:first-letter`).
    pub fn has_legacy_syntax(self) -> bool {
        matches!(
            self,
            PseudoElement::Before
                | PseudoElement::After
                | PseudoElement::FirstLine
                | PseudoElement::FirstLetter
        )
    }
}

/// One part of a selector.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Component {
    Combinator(Combinator),
    /// `*`.
    Universal,
    LocalName(LocalName),
    Id(Box<str>),
    Class(Box<str>),
    Attribute(Box<AttributeSelector>),
    PseudoClass(PseudoClass),
}

/// A type selector's name.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LocalName {
    /// As written.
    pub(crate) name: Box<str>,
    /// The ASCII-lowercase form, if it differs from `name`.
    pub(crate) lower_name: Option<Box<str>>,
}

impl LocalName {
    pub(crate) fn new(name: &str) -> Self {
        LocalName {
            name: name.into(),
            lower_name: lowercase_if_needed(name),
        }
    }

    pub(crate) fn lower(&self) -> &str {
        self.lower_name.as_deref().unwrap_or(&self.name)
    }
}

fn lowercase_if_needed(s: &str) -> Option<Box<str>> {
    s.bytes()
        .any(|b| b.is_ascii_uppercase())
        .then(|| s.to_ascii_lowercase().into_boxed_str())
}

/// An attribute selector.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AttributeSelector {
    pub(crate) name: LocalName,
    pub(crate) operation: Option<(AttrOperator, Box<str>)>,
    pub(crate) case: AttrCase,
    /// True if the HTML specification lists this attribute as matching
    /// ASCII case-insensitively on HTML elements.
    pub(crate) html_case_insensitive: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AttrOperator {
    /// `=`.
    Equals,
    /// `~=`.
    Includes,
    /// `|=`.
    DashMatch,
    /// `^=`.
    Prefix,
    /// `$=`.
    Suffix,
    /// `*=`.
    Substring,
}

/// The case-sensitivity modifier of an attribute selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AttrCase {
    /// No modifier: the document language decides.
    Default,
    /// `i`.
    Insensitive,
    /// `s`.
    Sensitive,
}

/// A pseudo-class.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PseudoClass {
    Link,
    Visited,
    AnyLink,
    Root,
    Empty,
    Scope,
    FirstChild,
    LastChild,
    OnlyChild,
    FirstOfType,
    LastOfType,
    OnlyOfType,
    /// A pseudo-class that depends only on [`ElementState`] flags.
    State(ElementState),
    NthChild(Box<Nth>),
    NthLastChild(Box<Nth>),
    NthOfType(AnB),
    NthLastOfType(AnB),
    Not(SelectorList),
    Is(SelectorList),
    Where(SelectorList),
    Has(Box<[RelativeSelector]>),
    Lang(Box<[Box<str>]>),
    Dir(Direction),
}

/// The argument of `:nth-child()` and `:nth-last-child()`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Nth {
    pub(crate) anb: AnB,
    /// The `of S` selector list.
    pub(crate) of: Option<SelectorList>,
}

/// A relative selector in `:has()`, for example `> img`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RelativeSelector {
    /// The combinator between the `:has()` element and the selector.
    pub(crate) combinator: Combinator,
    pub(crate) selector: Selector,
}

/// The argument of `:dir()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    Ltr,
    Rtl,
}

/// Pseudo-classes that depend only on element state.
pub(crate) const STATE_PSEUDO_CLASSES: &[(&str, ElementState)] = &[
    ("hover", ElementState::HOVER),
    ("active", ElementState::ACTIVE),
    ("focus", ElementState::FOCUS),
    ("focus-visible", ElementState::FOCUS_VISIBLE),
    ("focus-within", ElementState::FOCUS_WITHIN),
    ("target", ElementState::TARGET),
    ("checked", ElementState::CHECKED),
    ("disabled", ElementState::DISABLED),
    ("enabled", ElementState::ENABLED),
    ("indeterminate", ElementState::INDETERMINATE),
    ("required", ElementState::REQUIRED),
    ("optional", ElementState::OPTIONAL),
    ("read-only", ElementState::READ_ONLY),
    ("read-write", ElementState::READ_WRITE),
    ("placeholder-shown", ElementState::PLACEHOLDER_SHOWN),
    ("default", ElementState::DEFAULT),
    ("defined", ElementState::DEFINED),
    ("valid", ElementState::VALID),
    ("invalid", ElementState::INVALID),
    ("user-valid", ElementState::USER_VALID),
    ("user-invalid", ElementState::USER_INVALID),
    ("in-range", ElementState::IN_RANGE),
    ("out-of-range", ElementState::OUT_OF_RANGE),
    ("autofill", ElementState::AUTOFILL),
    ("open", ElementState::OPEN),
    ("modal", ElementState::MODAL),
    ("fullscreen", ElementState::FULLSCREEN),
    ("popover-open", ElementState::POPOVER_OPEN),
];
