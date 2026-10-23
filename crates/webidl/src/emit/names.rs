//! Rust names for IDL names: `snake_case` functions, fields and modules,
//! `CamelCase` variants, `SCREAMING_CASE` constants, string literals.

/// The keywords that a Rust name must not be (a `_` is appended).
const KEYWORDS: &[&str] = &[
    "as", "async", "await", "box", "break", "const", "continue", "crate", "do", "dyn", "else",
    "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl", "in", "let", "loop",
    "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref", "return", "self",
    "static", "struct", "super", "trait", "true", "try", "type", "typeof", "unsafe", "unsized",
    "use", "virtual", "where", "while", "yield", "abstract", "become",
];

/// `nodeType` → `node_type`, `lookupNamespaceURI` → `lookup_namespace_uri`,
/// `DOMException` → `dom_exception`, `innerHTML` → `inner_html`.
pub(crate) fn snake(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c == '-' || c == '_' || c == ' ' {
            if !out.ends_with('_') && !out.is_empty() {
                out.push('_');
            }
            continue;
        }
        if c.is_ascii_uppercase() {
            let prev = i.checked_sub(1).and_then(|p| chars.get(p)).copied();
            let next = chars.get(i + 1).copied();
            let boundary = match prev {
                Some(p) if p.is_ascii_lowercase() || p.is_ascii_digit() => true,
                Some(p) if p.is_ascii_uppercase() => next.is_some_and(|n| n.is_ascii_lowercase()),
                _ => false,
            };
            if boundary && !out.ends_with('_') {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// A `snake_case` name that is not a Rust keyword.
pub(crate) fn ident(name: &str) -> String {
    let name = snake(name);
    if KEYWORDS.contains(&name.as_str()) {
        format!("{name}_")
    } else {
        name
    }
}

/// `ELEMENT_NODE` stays; `nodeType` → `NODE_TYPE`.
pub(crate) fn constant(name: &str) -> String {
    snake(name).to_ascii_uppercase()
}

/// A `CamelCase` variant for an enumeration value or a type name:
/// `"two-words"` → `TwoWords`, `""` → `Empty`, `"2d"` → `V2d`,
/// `DOMString` → `DomString`.
pub(crate) fn camel(text: &str) -> String {
    if text.is_empty() {
        return "Empty".to_owned();
    }
    let mut out = String::new();
    for word in snake(text).split(|c: char| !c.is_ascii_alphanumeric()) {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.extend(chars);
        }
    }
    if out.is_empty() {
        return "Empty".to_owned();
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, 'V');
    }
    out
}

/// A Rust string literal.
pub(crate) fn literal(text: &str) -> String {
    format!("{text:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(snake("nodeType"), "node_type");
        assert_eq!(snake("lookupNamespaceURI"), "lookup_namespace_uri");
        assert_eq!(snake("DOMException"), "dom_exception");
        assert_eq!(snake("innerHTML"), "inner_html");
        assert_eq!(snake("URLSearchParams"), "url_search_params");
        assert_eq!(snake("EventTarget"), "event_target");
        assert_eq!(snake("baseURI"), "base_uri");
        assert_eq!(snake("ELEMENT_NODE"), "element_node");
        assert_eq!(snake("use-credentials"), "use_credentials");
        assert_eq!(ident("type"), "type_");
        assert_eq!(constant("nodeType"), "NODE_TYPE");
        assert_eq!(camel("two-words"), "TwoWords");
        assert_eq!(camel(""), "Empty");
        assert_eq!(camel("2d"), "V2d");
        assert_eq!(camel("DOMString"), "DomString");
        assert_eq!(literal("a\"b"), "\"a\\\"b\"");
    }
}
