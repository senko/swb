//! Dependency cycles through font-relative units: a registered custom
//! property whose value uses `em`, `ex`, `ch`, `cap`, `ic` or `lh`, and
//! that `font-size` references (directly, or through other custom
//! properties), is in a cycle: the font size would depend on a length that
//! depends on the font size. Such a property computes to its initial value
//! (CSS Properties and Values API 1, "Dependency cycles via relative
//! units"). On the root element `rem` and `rlh` are in a cycle too. `lh`
//! also makes a cycle for `line-height`.
//!
//! Measured in Chromium 148 (`ap-font-size-cycle-*`, `ap-line-height-em`,
//! `ap-font-size-indirect`, `ap-font-size-unrelated`); ADR 0038, section
//! "7b". Properties that no font property references are not affected.
//!
//! The module also tells the cascade whether an element's registered
//! values can depend on its own line height ([`depends_on_line_height`]).
//!
//! <https://www.w3.org/TR/css-properties-values-api-1/#dependency-cycles>

use std::collections::HashMap;
use std::sync::Arc;

use swb_css::ComponentValue;

use crate::custom::CustomProperties;
use crate::properties::CustomDeclaration;
use crate::registered::Registry;

/// The units that make a cycle for `font-size`.
fn font_size_unit(unit: &str, is_root: bool) -> bool {
    ["em", "ex", "ch", "cap", "ic", "lh"]
        .iter()
        .any(|u| unit.eq_ignore_ascii_case(u))
        || (is_root && (unit.eq_ignore_ascii_case("rem") || unit.eq_ignore_ascii_case("rlh")))
}

/// The units that make a cycle for `line-height`.
fn line_height_unit(unit: &str, _is_root: bool) -> bool {
    unit.eq_ignore_ascii_case("lh")
}

/// The font property that references custom properties.
#[derive(Clone, Copy)]
pub(crate) enum Property {
    FontSize,
    LineHeight,
}

/// The registered properties that are in a cycle with `property`, whose
/// declaration has the tokens `tokens`: those among the declared custom
/// properties that the tokens reach through `var()`, and whose value uses a
/// unit of the cycle or references such a property. Empty if the tokens
/// have no `var()`.
pub(crate) fn cycle_names(
    property: Property,
    tokens: &[ComponentValue],
    declared: &[(&Arc<str>, &CustomDeclaration)],
    registry: &Registry,
    is_root: bool,
) -> Vec<Arc<str>> {
    let mut roots = Vec::new();
    var_names(tokens, &mut roots);
    if roots.is_empty() {
        return Vec::new();
    }
    let unit_in_cycle = match property {
        Property::FontSize => font_size_unit,
        Property::LineHeight => line_height_unit,
    };
    let by_name: HashMap<&str, &Arc<[ComponentValue]>> = declared
        .iter()
        .filter_map(|(name, decl)| match decl {
            CustomDeclaration::Value(value) => Some((&***name, value)),
            CustomDeclaration::CssWide(_) => None,
        })
        .collect();
    // The graph of the declared properties that the tokens reach: indexes
    // into `nodes`, and for each node the nodes that reference it.
    let mut nodes: Vec<&str> = Vec::new();
    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut referenced_by: Vec<Vec<usize>> = Vec::new();
    let mut queue: Vec<String> = roots;
    let mut next = 0;
    while next < queue.len() {
        let name = queue[next].clone();
        next += 1;
        let Some((&key, value)) = by_name.get_key_value(name.as_str()) else {
            continue;
        };
        if index.contains_key(key) {
            continue;
        }
        index.insert(key, nodes.len());
        nodes.push(key);
        referenced_by.push(Vec::new());
        var_names(value, &mut queue);
    }
    // Edges: a node's value references other nodes.
    for (i, name) in nodes.iter().enumerate() {
        let mut names = Vec::new();
        if let Some(value) = by_name.get(name) {
            var_names(value, &mut names);
        }
        for dep in names {
            if let Some(&j) = index.get(dep.as_str()) {
                referenced_by[j].push(i);
            }
        }
    }
    // Registered properties with a font-relative unit start the marking;
    // it spreads to the properties that reference a marked one.
    let in_scope = |name: &str| registry.get(name).is_some_and(|r| !r.is_universal());
    let mut marked = vec![false; nodes.len()];
    let mut work = Vec::new();
    for (i, name) in nodes.iter().enumerate() {
        let uses_unit = by_name
            .get(name)
            .is_some_and(|value| has_unit(value, unit_in_cycle, is_root));
        if in_scope(name) && uses_unit {
            marked[i] = true;
            work.push(i);
        }
    }
    while let Some(i) = work.pop() {
        for &j in &referenced_by[i] {
            if !marked[j] && in_scope(nodes[j]) {
                marked[j] = true;
                work.push(j);
            }
        }
    }
    nodes
        .iter()
        .zip(&marked)
        .filter(|(_, m)| **m)
        .map(|(name, _)| Arc::from(*name))
        .collect()
}

/// True if the registered custom properties of an element can depend on
/// its own line height: a declared value uses `lh`, or it references
/// through `var()` an inherited value that does. Inherited values have no
/// `var()` left, so one lookup in the parent's map is enough. The cascade
/// computes the registered lengths again with the element's line height
/// only then (on videojs.org, most elements change the line height and
/// none uses `lh`).
pub(crate) fn depends_on_line_height(
    declared: &[(&Arc<str>, &CustomDeclaration)],
    parent: Option<&CustomProperties>,
) -> bool {
    let mut names = Vec::new();
    for (_, decl) in declared {
        if let CustomDeclaration::Value(value) = decl {
            if has_unit(value, line_height_unit, false) {
                return true;
            }
            var_names(value, &mut names);
        }
    }
    parent.is_some_and(|parent| {
        names.iter().any(|name| {
            parent
                .get(name)
                .is_some_and(|value| has_unit(value, line_height_unit, false))
        })
    })
}

/// Appends the names that `var()` references in `values` use, at any
/// depth (also those in fallbacks).
fn var_names(values: &[ComponentValue], out: &mut Vec<String>) {
    for value in values {
        match value {
            ComponentValue::Function(f) => {
                if f.name.eq_ignore_ascii_case("var")
                    && let Some(ComponentValue::Ident(name)) =
                        f.arguments.iter().find(|v| !v.is_whitespace())
                {
                    out.push(name.to_string());
                }
                var_names(&f.arguments, out);
            }
            ComponentValue::Block(b) => var_names(&b.contents, out),
            _ => {}
        }
    }
}

/// True if a dimension in `values` has a unit that `in_cycle` accepts.
fn has_unit(values: &[ComponentValue], in_cycle: fn(&str, bool) -> bool, is_root: bool) -> bool {
    values.iter().any(|value| match value {
        ComponentValue::Dimension { unit, .. } => in_cycle(unit, is_root),
        ComponentValue::Function(f) => has_unit(&f.arguments, in_cycle, is_root),
        ComponentValue::Block(b) => has_unit(&b.contents, in_cycle, is_root),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_css::parse_component_values;

    const NONE: [&str; 0] = [];

    fn registry(names: &[&str]) -> Registry {
        let css = names.iter().fold(String::new(), |mut css, n| {
            css.push_str("@property ");
            css.push_str(n);
            css.push_str(" { syntax: '<length>'; inherits: true; initial-value: 0px }");
            css
        });
        let sheet = swb_css::parse_stylesheet(&css);
        let regs: Vec<_> = sheet
            .rules
            .iter()
            .filter_map(|r| match r {
                swb_css::CssRule::Property(rule) => {
                    crate::registered::Registration::from_rule(rule)
                }
                _ => None,
            })
            .collect();
        Registry::new(regs.iter())
    }

    fn names(
        property: Property,
        font: &str,
        decls: &[(&str, &str)],
        registered: &[&str],
        is_root: bool,
    ) -> Vec<String> {
        let owned = declarations(decls);
        let refs: Vec<(&Arc<str>, &CustomDeclaration)> =
            owned.iter().map(|(n, d)| (n, d)).collect();
        let mut out: Vec<String> = cycle_names(
            property,
            &parse_component_values(font),
            &refs,
            &registry(registered),
            is_root,
        )
        .iter()
        .map(ToString::to_string)
        .collect();
        out.sort();
        out
    }

    fn declarations(decls: &[(&str, &str)]) -> Vec<(Arc<str>, CustomDeclaration)> {
        decls
            .iter()
            .map(|(n, v)| {
                let tokens = parse_component_values(v);
                (
                    Arc::from(*n),
                    CustomDeclaration::Value(Arc::from(swb_css::trim_whitespace(&tokens))),
                )
            })
            .collect()
    }

    fn uses_lh(decls: &[(&str, &str)], inherited: &[(&str, &str)]) -> bool {
        let parent_decls = declarations(inherited);
        let parent_refs: Vec<(&Arc<str>, &CustomDeclaration)> =
            parent_decls.iter().map(|(n, d)| (n, d)).collect();
        let pool = crate::custom::CustomPool::default();
        let parent = crate::custom::compute_custom_properties(None, &parent_refs, None, &pool);
        let owned = declarations(decls);
        let refs: Vec<(&Arc<str>, &CustomDeclaration)> =
            owned.iter().map(|(n, d)| (n, d)).collect();
        depends_on_line_height(&refs, parent.as_deref())
    }

    #[test]
    fn registered_values_depend_on_the_line_height_only_through_lh() {
        assert!(uses_lh(&[("--a", "2lh")], &[]));
        assert!(uses_lh(&[("--a", "calc(1px + 2LH)")], &[]));
        assert!(uses_lh(&[("--a", "var(--x, 1lh)")], &[]));
        assert!(!uses_lh(&[("--a", "2em"), ("--b", "3rlh")], &[]));
        // Through an inherited value, also through a declared one.
        assert!(uses_lh(&[("--a", "var(--x)")], &[("--x", "1lh")]));
        assert!(uses_lh(
            &[("--a", "var(--b)"), ("--b", "var(--x)")],
            &[("--x", "1lh")]
        ));
        assert!(!uses_lh(
            &[("--a", "var(--x)")],
            &[("--x", "1em"), ("--y", "1lh")]
        ));
        assert!(!uses_lh(&[], &[("--x", "1lh")]));
    }

    #[test]
    fn a_font_relative_registered_property_that_font_size_uses_is_a_cycle() {
        let fs = Property::FontSize;
        assert_eq!(
            names(fs, "var(--p)", &[("--p", "2em")], &["--p"], false),
            ["--p"]
        );
        assert_eq!(
            names(fs, "var(--p)", &[("--p", "3LH")], &["--p"], false),
            ["--p"]
        );
        assert_eq!(
            names(fs, "var(--p)", &[("--p", "2px")], &["--p"], false),
            NONE
        );
        assert_eq!(
            names(fs, "var(--p)", &[("--p", "2rem")], &["--p"], false),
            NONE
        );
        assert_eq!(
            names(fs, "var(--p)", &[("--p", "2rem")], &["--p"], true),
            ["--p"]
        );
        // Not registered, or not used by the font size.
        assert_eq!(names(fs, "var(--p)", &[("--p", "2em")], &[], false), NONE);
        assert_eq!(
            names(fs, "var(--p)", &[("--q", "2em")], &["--q"], false),
            NONE
        );
        assert_eq!(names(fs, "12px", &[("--q", "2em")], &["--q"], false), NONE);
    }

    #[test]
    fn the_cycle_spreads_through_references_and_fallbacks() {
        let fs = Property::FontSize;
        let decls = [
            ("--p", "var(--q)"),
            ("--q", "calc(2em + 1px)"),
            ("--r", "1em"),
        ];
        assert_eq!(
            names(
                fs,
                "calc(var(--p) * 2)",
                &decls,
                &["--p", "--q", "--r"],
                false
            ),
            ["--p", "--q"]
        );
        // The fallback is a reference too; `--p` is not reached.
        assert_eq!(
            names(
                fs,
                "var(--x, var(--q))",
                &decls,
                &["--p", "--q", "--r"],
                false
            ),
            ["--q"]
        );
    }

    #[test]
    fn line_height_cycles_only_through_lh() {
        let lh = Property::LineHeight;
        assert_eq!(
            names(lh, "var(--p)", &[("--p", "2lh")], &["--p"], false),
            ["--p"]
        );
        assert_eq!(
            names(lh, "var(--p)", &[("--p", "2em")], &["--p"], false),
            NONE
        );
    }
}
