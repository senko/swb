//! Serialization of selectors.
//!
//! <https://drafts.csswg.org/cssom/#serializing-selectors>

use std::fmt::{self, Write as _};

use super::{
    AnB, AttrCase, AttrOperator, Combinator, Component, Direction, PseudoClass,
    STATE_PSEUDO_CLASSES, Selector, SelectorList,
};
use crate::serialize::{serialize_identifier, serialize_string};

impl fmt::Display for SelectorList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        write_list(self, &mut out);
        f.write_str(&out)
    }
}

impl fmt::Display for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        write_selector(self, &mut out);
        f.write_str(&out)
    }
}

fn write_list(list: &SelectorList, out: &mut String) {
    for (i, selector) in list.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        write_selector(selector, out);
    }
}

fn write_selector(selector: &Selector, out: &mut String) {
    write_components(&selector.components, out);
    if let Some(pe) = selector.pseudo_element {
        out.push_str("::");
        out.push_str(pe.name());
    }
}

/// Writes right-to-left components in left-to-right order.
fn write_components(components: &[Component], out: &mut String) {
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
            write_component(component, out);
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

fn write_component(component: &Component, out: &mut String) {
    match component {
        Component::Combinator(_) => {}
        Component::Universal => out.push('*'),
        Component::LocalName(name) => serialize_identifier(&name.name, out),
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
            serialize_identifier(&attr.name.name, out);
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
            write_pseudo_class(pc, out);
        }
    }
}

fn write_pseudo_class(pc: &PseudoClass, out: &mut String) {
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
    };
    out.push_str(name);
    match pc {
        PseudoClass::Not(list) | PseudoClass::Is(list) | PseudoClass::Where(list) => {
            write_list(list, out);
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
                write_selector(&r.selector, out);
            }
        }
        PseudoClass::NthChild(nth) | PseudoClass::NthLastChild(nth) => {
            write_anb(nth.anb, out);
            if let Some(of) = &nth.of {
                out.push_str(" of ");
                write_list(of, out);
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
        _ => return,
    }
    out.push(')');
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
