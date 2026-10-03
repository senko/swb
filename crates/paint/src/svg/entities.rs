//! An upper bound on XML entity expansion, checked before parsing.
//!
//! roxmltree expands the entities that the internal DTD subset declares
//! (`<!ENTITY name "value">`): a reference `&name;` becomes the value, and
//! references in the value expand too (at most 10 levels, and at most 255
//! references per reference). A short document can expand to gigabytes: a
//! "billion laughs" chain, or one long value referenced many times. SVG
//! files from some editors declare entities (for example Adobe Illustrator
//! for namespace URIs), so DTDs cannot simply be rejected.
//!
//! roxmltree also looks entities up in a list, so every reference costs time
//! proportional to the number of declarations; their number is limited too.
//!
//! The bound over-counts: declarations and references count wherever they
//! appear in the source, also in comments and inside the DTD.

use std::collections::HashMap;

/// The maximum nesting depth of entity references, as in roxmltree.
const MAX_DEPTH: usize = 10;

/// The maximum number of entity declarations.
const MAX_DECLARATIONS: usize = 100;

/// Describes the problem if `text` declares too many entities or if its
/// entity references can produce more than `limit` bytes of text.
pub(super) fn check(text: &str, limit: usize) -> Option<&'static str> {
    let (declarations, count) = declarations(text);
    if count > MAX_DECLARATIONS {
        return Some("too many entity declarations");
    }
    if declarations.is_empty() {
        return None;
    }
    let mut sizes = HashMap::new();
    let mut total = 0usize;
    for name in references(text) {
        total = total.saturating_add(expanded_size(name, &declarations, &mut sizes, 0, limit));
        if total > limit {
            return Some("entity references expand too much");
        }
    }
    None
}

/// The entity declarations with a literal value, general and parameter
/// entities alike: name and value; and the number of all declarations. A
/// name declared twice keeps the longer value.
fn declarations(text: &str) -> (HashMap<&str, &str>, usize) {
    const START: &str = "<!ENTITY";
    let mut declarations: HashMap<&str, &str> = HashMap::new();
    let mut count = 0;
    let mut rest = text;
    while let Some(at) = rest.find(START) {
        count += 1;
        rest = &rest[at + START.len()..];
        let declaration = rest.trim_start_matches(is_xml_space);
        let declaration = declaration
            .strip_prefix('%')
            .map_or(declaration, |d| d.trim_start_matches(is_xml_space));
        let name_end = declaration
            .find(|c: char| is_xml_space(c) || matches!(c, '"' | '\'' | '>'))
            .unwrap_or(declaration.len());
        let (name, after) = declaration.split_at(name_end);
        let after = after.trim_start_matches(is_xml_space);
        let Some(quote) = after.chars().next().filter(|c| matches!(c, '"' | '\'')) else {
            continue;
        };
        let value = &after[quote.len_utf8()..];
        let value = value.find(quote).map_or(value, |end| &value[..end]);
        let known = declarations.entry(name).or_insert(value);
        if value.len() > known.len() {
            *known = value;
        }
    }
    (declarations, count)
}

/// The names of the entity references (`&name;`) in `text`. A name ends at
/// the first `;` before the next `&`, so the scan is linear.
fn references(text: &str) -> impl Iterator<Item = &str> {
    text.split('&')
        .skip(1)
        .filter_map(|after| after.find(';').map(|end| &after[..end]))
}

/// The number of bytes that a reference to `name` produces, or more than
/// `limit` if that is too many or the references nest too deeply.
fn expanded_size<'a>(
    name: &'a str,
    declarations: &HashMap<&'a str, &'a str>,
    sizes: &mut HashMap<&'a str, usize>,
    depth: usize,
    limit: usize,
) -> usize {
    let Some(value) = declarations.get(name) else {
        return 0;
    };
    if let Some(&size) = sizes.get(name) {
        return size;
    }
    if depth >= MAX_DEPTH {
        return limit.saturating_add(1);
    }
    let mut size = value.len();
    for inner in references(value) {
        size = size.saturating_add(expanded_size(inner, declarations, sizes, depth + 1, limit));
        if size > limit {
            break;
        }
    }
    sizes.insert(name, size);
    size
}

fn is_xml_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    const LIMIT: usize = 1024 * 1024;

    fn exceeds(text: &str) -> bool {
        check(text, LIMIT).is_some()
    }

    #[test]
    fn documents_without_entities_pass() {
        assert!(!exceeds("<svg>&amp;&lt;&#60;</svg>"));
        let doctype = r#"<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN"
            "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd"><svg/>"#;
        assert!(!exceeds(doctype));
    }

    #[test]
    fn namespace_entities_pass() {
        // As Adobe Illustrator writes them.
        let svg = r#"<!DOCTYPE svg [
            <!ENTITY ns_svg "http://www.w3.org/2000/svg">
            <!ENTITY ns_xlink 'http://www.w3.org/1999/xlink'>
        ]><svg xmlns="&ns_svg;" xmlns:xlink="&ns_xlink;"/>"#;
        assert!(!exceeds(svg));
    }

    #[test]
    fn billion_laughs_is_rejected() {
        let mut dtd = String::from("<!ENTITY lol0 \"lol\">");
        for i in 1..10 {
            let refs = format!("&lol{};", i - 1).repeat(10);
            write!(dtd, "<!ENTITY lol{i} \"{refs}\">").unwrap();
        }
        let svg = format!("<!DOCTYPE svg [{dtd}]><svg>&lol9;</svg>");
        assert!(exceeds(&svg));
    }

    #[test]
    fn one_long_value_referenced_often_is_rejected() {
        let value = "x".repeat(10_000);
        let refs = "&a;".repeat(200);
        let svg = format!("<!DOCTYPE svg [<!ENTITY a '{value}'>]><svg>{refs}</svg>");
        assert!(exceeds(&svg));
        let refs = "&a;".repeat(100);
        let svg = format!("<!DOCTYPE svg [<!ENTITY a '{value}'>]><svg>{refs}</svg>");
        assert!(!exceeds(&svg));
    }

    #[test]
    fn cycles_are_rejected() {
        let svg = r#"<!DOCTYPE svg [<!ENTITY a "&b;"><!ENTITY b "&a;">]><svg>&a;</svg>"#;
        assert!(exceeds(svg));
    }

    #[test]
    fn parameter_entities_count() {
        let value = "x".repeat(10_000);
        let refs = "&p;".repeat(200);
        let svg = format!("<!DOCTYPE svg [<!ENTITY % p \"{value}\">]><svg>{refs}</svg>");
        assert!(exceeds(&svg));
    }

    #[test]
    fn references_end_at_the_semicolon() {
        let names: Vec<&str> = references("&a; & b &c;&").collect();
        assert_eq!(names, ["a", "c"]);
    }

    #[test]
    fn long_names_count() {
        let name = "n".repeat(300);
        let value = "x".repeat(10_000);
        let refs = format!("&{name};").repeat(200);
        let svg = format!("<!DOCTYPE svg [<!ENTITY {name} '{value}'>]><svg>{refs}</svg>");
        assert!(exceeds(&svg));
    }

    #[test]
    fn many_declarations_are_rejected() {
        let dtd: String = (0..=MAX_DECLARATIONS)
            .map(|i| format!("<!ENTITY e{i} 'x'>"))
            .collect::<Vec<_>>()
            .concat();
        let svg = format!("<!DOCTYPE svg [{dtd}]><svg/>");
        assert_eq!(check(&svg, LIMIT), Some("too many entity declarations"));
    }
}
