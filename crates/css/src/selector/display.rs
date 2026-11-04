//! Serialization of selectors.
//!
//! <https://drafts.csswg.org/cssom/#serializing-selectors>
//!
//! [`fmt::Display`] writes type and attribute names as written;
//! [`SelectorList::cssom_text`] writes them in ASCII lowercase, as Chromium
//! serializes the selectors of a style rule in an HTML document (CSSOM
//! `selectorText`; measured: `DIV.Foo` is `div.Foo`).

use std::fmt::{self, Write as _};

use super::{
    AnB, AttrCase, AttrOperator, Combinator, Component, Direction, LocalName, PseudoClass,
    STATE_PSEUDO_CLASSES, Selector, SelectorList,
};
use crate::serialize::{serialize_identifier, serialize_string};

impl fmt::Display for SelectorList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        Writer { lower: false }.list(self, &mut out);
        f.write_str(&out)
    }
}

impl fmt::Display for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        Writer { lower: false }.selector(self, &mut out);
        f.write_str(&out)
    }
}

impl SelectorList {
    /// The list as CSSOM serializes the selectors of a style rule in an
    /// HTML document: as [`fmt::Display`], with type and attribute names in
    /// ASCII lowercase.
    pub fn cssom_text(&self) -> String {
        let mut out = String::new();
        Writer { lower: true }.list(self, &mut out);
        out
    }
}

/// Writes selectors; `lower` lowercases type and attribute names.
#[derive(Clone, Copy)]
struct Writer {
    lower: bool,
}

impl Writer {
    fn name(self, name: &LocalName) -> &str {
        if self.lower { name.lower() } else { &name.name }
    }

    fn list(self, list: &SelectorList, out: &mut String) {
        for (i, selector) in list.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            self.selector(selector, out);
        }
    }

    fn selector(self, selector: &Selector, out: &mut String) {
        self.components(&selector.components, out);
        if let Some(pe) = selector.pseudo_element {
            out.push_str("::");
            out.push_str(pe.name());
        }
    }

    /// Writes right-to-left components in left-to-right order.
    fn components(self, components: &[Component], out: &mut String) {
        let mut compounds: Vec<&[Component]> = Vec::new();
        let mut combinators: Vec<Combinator> = Vec::new();
        for part in components.split(|c| matches!(c, Component::Combinator(_))) {
            compounds.push(part);
        }
        for c in components {
            if let Component::Combinator(c) = c {
                combinators.push(*c);
            }
        }
        for (i, compound) in compounds.iter().enumerate().rev() {
            for component in *compound {
                self.component(component, out);
            }
            if i > 0 {
                out.push_str(match combinators[i - 1] {
                    Combinator::Descendant => " ",
                    Combinator::Child => " > ",
                    Combinator::NextSibling => " + ",
                    Combinator::SubsequentSibling => " ~ ",
                });
            }
        }
    }

    fn component(self, component: &Component, out: &mut String) {
        match component {
            Component::Combinator(_) => {}
            Component::Universal => out.push('*'),
            Component::LocalName(name) => serialize_identifier(self.name(name), out),
            Component::Id(id) => {
                out.push('#');
                serialize_identifier(id, out);
            }
            Component::Class(class) => {
                out.push('.');
                serialize_identifier(class, out);
            }
            Component::Attribute(attr) => {
                out.push('[');
                serialize_identifier(self.name(&attr.name), out);
                if let Some((operator, value)) = &attr.operation {
                    out.push_str(match operator {
                        AttrOperator::Equals => "=",
                        AttrOperator::Includes => "~=",
                        AttrOperator::DashMatch => "|=",
                        AttrOperator::Prefix => "^=",
                        AttrOperator::Suffix => "$=",
                        AttrOperator::Substring => "*=",
                    });
                    serialize_string(value, out);
                    match attr.case {
                        AttrCase::Default => {}
                        AttrCase::Insensitive => out.push_str(" i"),
                        AttrCase::Sensitive => out.push_str(" s"),
                    }
                }
                out.push(']');
            }
            Component::PseudoClass(pc) => {
                out.push(':');
                self.pseudo_class(pc, out);
            }
        }
    }

    fn pseudo_class(self, pc: &PseudoClass, out: &mut String) {
        let name = match pc {
            PseudoClass::Link => "link",
            PseudoClass::Visited => "visited",
            PseudoClass::AnyLink => "any-link",
            PseudoClass::Root => "root",
            PseudoClass::Empty => "empty",
            PseudoClass::Scope => "scope",
            PseudoClass::FirstChild => "first-child",
            PseudoClass::LastChild => "last-child",
            PseudoClass::OnlyChild => "only-child",
            PseudoClass::FirstOfType => "first-of-type",
            PseudoClass::LastOfType => "last-of-type",
            PseudoClass::OnlyOfType => "only-of-type",
            PseudoClass::State(state) => STATE_PSEUDO_CLASSES
                .iter()
                .find(|(_, s)| s == state)
                .map_or("", |(name, _)| name),
            PseudoClass::Not(_) => "not(",
            PseudoClass::Is(_) => "is(",
            PseudoClass::Where(_) => "where(",
            PseudoClass::Has(_) => "has(",
            PseudoClass::NthChild(_) => "nth-child(",
            PseudoClass::NthLastChild(_) => "nth-last-child(",
            PseudoClass::NthOfType(_) => "nth-of-type(",
            PseudoClass::NthLastOfType(_) => "nth-last-of-type(",
            PseudoClass::Lang(_) => "lang(",
            PseudoClass::Dir(_) => "dir(",
            PseudoClass::Host(None) => "host",
            PseudoClass::Host(Some(_)) => "host(",
            PseudoClass::HostContext(_) => "host-context(",
        };
        out.push_str(name);
        match pc {
            PseudoClass::Not(list) | PseudoClass::Is(list) | PseudoClass::Where(list) => {
                self.list(list, out);
            }
            PseudoClass::Has(relative) => {
                for (i, r) in relative.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(match r.combinator {
                        Combinator::Descendant => "",
                        Combinator::Child => "> ",
                        Combinator::NextSibling => "+ ",
                        Combinator::SubsequentSibling => "~ ",
                    });
                    self.selector(&r.selector, out);
                }
            }
            PseudoClass::NthChild(nth) | PseudoClass::NthLastChild(nth) => {
                write_anb(nth.anb, out);
                if let Some(of) = &nth.of {
                    out.push_str(" of ");
                    self.list(of, out);
                }
            }
            PseudoClass::NthOfType(anb) | PseudoClass::NthLastOfType(anb) => write_anb(*anb, out),
            PseudoClass::Lang(ranges) => {
                for (i, range) in ranges.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    serialize_string(range, out);
                }
            }
            PseudoClass::Dir(Direction::Ltr) => out.push_str("ltr"),
            PseudoClass::Dir(Direction::Rtl) => out.push_str("rtl"),
            PseudoClass::Host(Some(arg)) | PseudoClass::HostContext(arg) => {
                self.selector(arg, out);
            }
            _ => return,
        }
        out.push(')');
    }
}

/// <https://www.w3.org/TR/css-syntax-3/#serializing-anb>
fn write_anb(anb: AnB, out: &mut String) {
    let AnB { a, b } = anb;
    let _ = match (a, b) {
        (0, b) => write!(out, "{b}"),
        (1, _) => write!(out, "n"),
        (-1, _) => write!(out, "-n"),
        (a, _) => write!(out, "{a}n"),
    };
    if a != 0 && b != 0 {
        let _ = write!(out, "{b:+}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cssom_text_lowercases_names() {
        let list = SelectorList::parse_str("DIV.Foo#Bar > SPAN[DATA-X=Y], A:not(B)").unwrap();
        assert_eq!(
            list.cssom_text(),
            "div.Foo#Bar > span[data-x=\"Y\"], a:not(b)"
        );
        assert_eq!(
            list.to_string(),
            "DIV.Foo#Bar > SPAN[DATA-X=\"Y\"], A:not(B)"
        );
    }
}
