//! Style sheets in SVG images, as usvg applies them: which `url()`
//! references their rules give to which elements, and bounds on the work
//! that applying them costs.
//!
//! usvg parses every `<style>` element (in any namespace, without a `type`
//! or with `text/css`) with simplecss and matches every rule against every
//! element of its tree, including the copies that `<use>` makes. simplecss
//! copies a rule's declarations into every selector of a selector list,
//! and it matches descendant combinators by backtracking: a selector with
//! `k` descendant combinators can try every choice of `k` ancestors.

use std::collections::HashMap;

use roxmltree::{Document, Node, NodeId};
use simplecss::{AttributeOperator, PseudoClass, SelectorToken, SelectorTokenizer, StyleSheet};

use super::expansion::url_ids;

/// The properties whose `url()` references usvg follows (markers are
/// handled separately).
pub(super) const LINK_PROPERTIES: [&str; 5] = ["clip-path", "mask", "filter", "fill", "stroke"];

/// The limits of style sheets.
pub(super) struct Limits {
    /// Rules times the length of the text: simplecss's parser takes time
    /// proportional to both (100,000 rules of 18 bytes take 35 s).
    pub(super) parse: f64,
    /// Declarations after simplecss copies them into every selector.
    pub(super) declarations: f64,
    /// Selector match attempts: rules × elements × ancestor choices.
    pub(super) matches: f64,
}

/// What the style sheets of a document do.
#[derive(Debug, Default)]
pub(super) struct Css<'a> {
    /// The ids of the `url()` references that rules give each element.
    pub(super) references: HashMap<NodeId, Vec<&'a str>>,
    /// True if a style sheet mentions markers (it may nest them).
    pub(super) mentions_markers: bool,
}

/// Applies the style sheets of `xml` to its elements. `elements` and
/// `depth` are the number of elements and the nesting depth of usvg's tree
/// (with `<use>` copies).
pub(super) fn analyze<'a>(
    xml: &'a Document<'_>,
    elements: f64,
    depth: f64,
    limits: &Limits,
) -> Result<Css<'a>, &'static str> {
    let texts: Vec<&'a str> = xml
        .descendants()
        .filter(|n| {
            n.is_element()
                && n.tag_name().name() == "style"
                && n.attribute("type").is_none_or(|t| t == "text/css")
        })
        .filter_map(|n| n.text())
        .collect();
    if texts.is_empty() {
        return Ok(Css::default());
    }
    let rules: f64 = texts.iter().map(|t| t.matches('{').count() as f64).sum();
    let length: f64 = texts.iter().map(|t| t.len() as f64).sum();
    if rules * length > limits.parse {
        return Err("style sheets too large");
    }
    // An upper bound of the copies: selectors times declarations.
    let copies: f64 = texts
        .iter()
        .map(|t| (t.matches(',').count() + 1) as f64 * (t.matches(';').count() + 1) as f64)
        .sum();
    if copies > limits.declarations {
        return Err("style sheets too large");
    }
    let mut sheet = StyleSheet::new();
    for text in &texts {
        sheet.parse_more(text);
    }
    let attempts: f64 = sheet
        .rules
        .iter()
        .map(|rule| ancestor_choices(depth, descendant_combinators(&rule.selector.to_string())))
        .sum();
    if attempts * elements > limits.matches {
        return Err("style sheets too expensive to apply");
    }
    let linking: Vec<(&simplecss::Rule<'a>, Vec<&'a str>)> = sheet
        .rules
        .iter()
        .filter_map(|rule| {
            let ids: Vec<&'a str> = rule
                .declarations
                .iter()
                .filter(|d| LINK_PROPERTIES.contains(&d.name))
                .flat_map(|d| url_ids(d.value))
                .collect();
            (!ids.is_empty()).then_some((rule, ids))
        })
        .collect();
    let mut references: HashMap<NodeId, Vec<&'a str>> = HashMap::new();
    for node in xml.descendants().filter(Node::is_element) {
        for (rule, ids) in &linking {
            if rule.selector.matches(&XmlElement(node)) {
                references.entry(node.id()).or_default().extend(ids);
            }
        }
    }
    Ok(Css {
        references,
        mentions_markers: texts.iter().any(|t| t.contains("marker")),
    })
}

/// The number of descendant combinators in a selector.
fn descendant_combinators(selector: &str) -> u32 {
    SelectorTokenizer::from(selector)
        .filter(|t| matches!(t, Ok(SelectorToken::DescendantCombinator)))
        .count() as u32
}

/// The number of ways to choose `k` ancestors out of `depth` (at least
/// one attempt).
fn ancestor_choices(depth: f64, k: u32) -> f64 {
    (0..k)
        .map(|i| (depth - f64::from(i)).max(0.0) / f64::from(i + 1))
        .product::<f64>()
        .max(1.0)
}

/// An element for selector matching, as usvg's `XmlNode`.
struct XmlElement<'a, 'input>(Node<'a, 'input>);

impl simplecss::Element for XmlElement<'_, '_> {
    fn parent_element(&self) -> Option<Self> {
        self.0.parent_element().map(XmlElement)
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        self.0.prev_sibling_element().map(XmlElement)
    }

    fn has_local_name(&self, local_name: &str) -> bool {
        self.0.tag_name().name() == local_name
    }

    fn attribute_matches(&self, local_name: &str, operator: AttributeOperator<'_>) -> bool {
        self.0
            .attribute(local_name)
            .is_some_and(|value| operator.matches(value))
    }

    fn pseudo_class_matches(&self, class: PseudoClass<'_>) -> bool {
        matches!(class, PseudoClass::FirstChild) && self.prev_sibling_element().is_none()
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    const LIMITS: Limits = Limits {
        parse: 1e8,
        declarations: 1e4,
        matches: 1e6,
    };

    fn analyze_source(source: &str) -> Result<usize, &'static str> {
        let xml = Document::parse(source).unwrap();
        let elements = xml.descendants().filter(Node::is_element).count() as f64;
        analyze(&xml, elements, 10.0, &LIMITS).map(|css| css.references.len())
    }

    #[test]
    fn rules_give_references_to_matching_elements() {
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg">
            <style>.a { fill: url(#g) } .b { stroke: red } rect.c, circle { mask: url('#m') }</style>
            <rect class="a"/><rect class="b"/><rect class="c"/><circle/><path/></svg>"#;
        assert_eq!(analyze_source(source), Ok(3));
    }

    #[test]
    fn large_and_expensive_style_sheets_are_rejected() {
        let selectors = vec![".a"; 200].join(",");
        let declarations = "fill:red;".repeat(100);
        let source = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><style>{selectors}{{{declarations}}}</style></svg>"#
        );
        assert_eq!(analyze_source(&source), Err("style sheets too large"));
        // simplecss parses in time proportional to rules × length.
        let mut many = String::new();
        for i in 0..10_000 {
            write!(many, ".c{i}{{fill:red}}").unwrap();
        }
        let source =
            format!(r#"<svg xmlns="http://www.w3.org/2000/svg"><style>{many}</style></svg>"#);
        assert_eq!(analyze_source(&source), Err("style sheets too large"));
        let rules = ".a{fill:red}".repeat(1000);
        let source = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><style>{rules}</style>{}</svg>"#,
            "<g/>".repeat(1000)
        );
        assert_eq!(
            analyze_source(&source),
            Err("style sheets too expensive to apply")
        );
    }

    #[test]
    fn descendant_combinators_cost_ancestor_choices() {
        assert_eq!(descendant_combinators("g g > rect + path"), 1);
        assert_eq!(descendant_combinators("x g g g g"), 4);
        assert_eq!(ancestor_choices(40.0, 0), 1.0);
        assert_eq!(ancestor_choices(40.0, 2), 780.0);
        assert!(ancestor_choices(40.0, 8) > 7e7);
    }
}
