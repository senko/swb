//! Container queries in the cascade (ADR 0038): the query container lookup,
//! the evaluation of `@container` conditions, the sizes of the container
//! query units, and the log of what the styles read.
//!
//! The style walk keeps a stack of the flat tree ancestors that are query
//! containers ([`ComputedStyle::is_query_container`]). For a condition the
//! lookup walks the stack from the top (measured in Chromium 148,
//! `tools/probes/container-queries.json`): the nearest container that has
//! the condition's name (if any) and whose `container-type` supports every
//! axis that the query reads. A name alone matches any container with the
//! name. A query with a `<general-enclosed>` part finds none. The selected
//! container's content box comes from the last layout ([`ContainerSizes`]);
//! a container without one (no layout yet, or a box that size containment
//! does not apply to) gives unknown sizes, so its queries are false.
//!
//! The units take, per axis, the nearest container that supports the axis
//! and whose box size containment applies to; containers without such a
//! box are skipped (measured: `units-odd-containers`). Without one, the
//! viewport.
//!
//! The lookup is memoized per (stack entry, condition), and a stack entry
//! that does not match defers to the entry below it, so a walk down N
//! nested containers costs O(N) per condition, not O(N²). The evaluation is
//! memoized per (container, condition).
//!
//! [`ContainerLog`] records, for each container that a style pass read, the
//! size it used, the conditions it evaluated with their results, and
//! whether units read its size. The engine compares it with the sizes of
//! the next layout ([`ContainerLog::holds`]).

use std::collections::HashMap;
use std::sync::Arc;

use swb_css::{ComponentValue, ContainerCondition, ContainerSize, MediaEnvironment};
use swb_dom::NodeId;

use crate::ComputedStyle;
use crate::values::ContainerBasis;

/// The content boxes of query containers from a layout: width and height
/// in px, for the containers whose box size containment applies to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContainerSizes {
    sizes: HashMap<NodeId, (f32, f32)>,
}

impl ContainerSizes {
    /// No sizes (before the first layout).
    pub fn new() -> Self {
        ContainerSizes::default()
    }

    /// Sets the content box size of the container `node`.
    pub fn insert(&mut self, node: NodeId, width: f32, height: f32) {
        self.sizes.insert(node, (width, height));
    }

    /// The content box size of `node`, if layout gave it one.
    pub fn get(&self, node: NodeId) -> Option<(f32, f32)> {
        self.sizes.get(&node).copied()
    }

    /// The number of containers with a size.
    pub fn len(&self) -> usize {
        self.sizes.len()
    }

    /// True if no container has a size.
    pub fn is_empty(&self) -> bool {
        self.sizes.is_empty()
    }
}

/// What a style pass read of one container.
#[derive(Clone, Debug)]
struct LogEntry {
    /// The size used: `None` if unknown.
    size: Option<(f32, f32)>,
    /// The axes (inline, block) that the container's type supports.
    axes: (bool, bool),
    /// The container's font size and line height (for `em` and `lh` in its
    /// queries).
    fonts: Fonts,
    /// The conditions evaluated against it, with their results.
    conditions: Vec<(Arc<ContainerCondition>, bool)>,
    /// True if container units read its size.
    units: bool,
}

/// What a style pass read of query containers (see the module comment).
#[derive(Clone, Debug, Default)]
pub struct ContainerLog {
    entries: HashMap<NodeId, LogEntry>,
    root_font_size: f32,
    viewport: (f32, f32),
}

impl ContainerLog {
    /// True if the styles read no container.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The containers that the styles read.
    pub fn containers(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.entries.keys().copied()
    }

    /// True if the styles stay right with the container sizes `sizes` of a
    /// new layout: for each container whose size changed, no unit read its
    /// size and every logged condition gives the same result.
    pub fn holds(&self, sizes: &ContainerSizes) -> bool {
        self.entries.iter().all(|(&node, entry)| {
            let new = sizes.get(node);
            if new == entry.size {
                return true;
            }
            if entry.units {
                return false;
            }
            let size = self.container_size(new, entry);
            entry
                .conditions
                .iter()
                .all(|(condition, result)| condition.evaluate(&size) == *result)
        })
    }

    fn container_size(&self, size: Option<(f32, f32)>, entry: &LogEntry) -> ContainerSize {
        query_size(
            size,
            entry.axes,
            entry.fonts,
            self.root_font_size,
            self.viewport,
        )
    }
}

/// The font size and line height of a container, in px.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Fonts {
    size: f32,
    line_height: f32,
}

impl Fonts {
    /// `normal` line height is 1.2 times the font size here (departure:
    /// the style crate has no font metrics; only `lh` in a query reads it).
    fn of(style: &ComputedStyle) -> Fonts {
        Fonts {
            size: style.font_size,
            line_height: style
                .line_height
                .resolve(style.font_size)
                .unwrap_or(style.font_size * 1.2),
        }
    }
}

/// The [`ContainerSize`] of a container for its queries.
fn query_size(
    size: Option<(f32, f32)>,
    (inline, block): (bool, bool),
    fonts: Fonts,
    root_font_size: f32,
    (viewport_width, viewport_height): (f32, f32),
) -> ContainerSize {
    ContainerSize {
        width: size.filter(|_| inline).map(|s| s.0),
        height: size.filter(|_| block).map(|s| s.1),
        vertical: false,
        font_size: fonts.size,
        line_height: fonts.line_height,
        root_font_size,
        viewport_width,
        viewport_height,
    }
}

/// A query container on the walk's stack.
struct Entry {
    node: NodeId,
    style: Arc<ComputedStyle>,
}

/// The container state of one style walk.
pub(crate) struct Containers<'a> {
    sizes: &'a ContainerSizes,
    stack: Vec<Entry>,
    /// The result of the lookup per (stack entry node, condition address):
    /// the node of the container, or `None`.
    lookups: HashMap<(NodeId, usize), Option<NodeId>>,
    /// The results per (container, condition address).
    results: HashMap<(NodeId, usize), bool>,
    /// The unit sizes per nearest container (the top of the stack).
    bases: HashMap<NodeId, ContainerBasis>,
    log: ContainerLog,
    viewport: ContainerBasis,
}

impl<'a> Containers<'a> {
    /// The state of a walk with the container sizes of the last layout.
    pub(crate) fn new(sizes: &'a ContainerSizes, env: &MediaEnvironment) -> Self {
        Containers {
            sizes,
            stack: Vec::new(),
            lookups: HashMap::new(),
            results: HashMap::new(),
            bases: HashMap::new(),
            log: ContainerLog {
                entries: HashMap::new(),
                root_font_size: env.root_font_size,
                viewport: (env.viewport_width, env.viewport_height),
            },
            viewport: ContainerBasis {
                width: env.viewport_width,
                height: env.viewport_height,
            },
        }
    }

    /// Sets the root font size (for `rem` in queries) once the root is
    /// styled.
    pub(crate) fn set_root_font_size(&mut self, size: f32) {
        self.log.root_font_size = size;
    }

    /// Pushes `node` if its style makes it a query container. Returns true
    /// if it pushed (the caller pops with [`Containers::pop`]).
    pub(crate) fn push(&mut self, node: NodeId, style: &Arc<ComputedStyle>) -> bool {
        if !style.is_query_container() {
            return false;
        }
        self.stack.push(Entry {
            node,
            style: Arc::clone(style),
        });
        true
    }

    /// Pops the top container if it is `node`.
    pub(crate) fn pop_if(&mut self, node: NodeId) {
        if self.stack.last().is_some_and(|e| e.node == node) {
            self.stack.pop();
        }
    }

    /// The nearest query container (the top of the stack), for the style
    /// sharing key.
    pub(crate) fn nearest(&self) -> Option<NodeId> {
        self.stack.last().map(|e| e.node)
    }

    /// Takes the log of the walk.
    pub(crate) fn take_log(&mut self) -> ContainerLog {
        std::mem::take(&mut self.log)
    }

    /// True if all conditions of a rule's chain are true for the element
    /// whose ancestors are on the stack.
    pub(crate) fn chain_matches(&mut self, chain: &[Arc<ContainerCondition>]) -> bool {
        chain.iter().all(|c| self.condition_matches(c))
    }

    fn condition_matches(&mut self, condition: &Arc<ContainerCondition>) -> bool {
        if condition.has_unknown() {
            return false;
        }
        let Some(index) = self.lookup(condition) else {
            return false;
        };
        let Some(entry) = self.stack.get(index) else {
            return false;
        };
        let node = entry.node;
        let key = (node, Arc::as_ptr(condition) as usize);
        if let Some(&result) = self.results.get(&key) {
            return result;
        }
        let style = &entry.style;
        let size = style
            .size_containment_applies()
            .then(|| self.sizes.get(node))
            .flatten();
        let axes = style.size_container_axes();
        let fonts = Fonts::of(style);
        let query = query_size(
            size,
            axes,
            fonts,
            self.log.root_font_size,
            self.log.viewport,
        );
        let result = condition.evaluate(&query);
        self.results.insert(key, result);
        let log = self.log_entry(node, size, axes, fonts);
        log.conditions.push((Arc::clone(condition), result));
        result
    }

    fn log_entry(
        &mut self,
        node: NodeId,
        size: Option<(f32, f32)>,
        axes: (bool, bool),
        fonts: Fonts,
    ) -> &mut LogEntry {
        self.log.entries.entry(node).or_insert_with(|| LogEntry {
            size,
            axes,
            fonts,
            conditions: Vec::new(),
            units: false,
        })
    }

    /// The index in the stack of the query container of `condition`.
    fn lookup(&mut self, condition: &Arc<ContainerCondition>) -> Option<usize> {
        let address = Arc::as_ptr(condition) as usize;
        let axes = condition.axes();
        let name_only = !condition.has_query();
        let mut visited: Vec<NodeId> = Vec::new();
        let mut found = None;
        for entry in self.stack.iter().rev() {
            if let Some(&known) = self.lookups.get(&(entry.node, address)) {
                found = known;
                break;
            }
            visited.push(entry.node);
            let style = &entry.style;
            let named = condition.name().is_none_or(|n| style.has_container_name(n));
            let supported = if name_only {
                true
            } else {
                // Every size container supports the inline axis.
                let (inline, block) = style.size_container_axes();
                let (_, need_block) = axes.logical(false);
                inline && (block || !need_block)
            };
            if named && supported {
                found = Some(entry.node);
                break;
            }
        }
        for node in visited {
            self.lookups.insert((node, address), found);
        }
        let node = found?;
        self.stack.iter().rposition(|e| e.node == node)
    }

    /// The sizes of the container units for an element whose ancestors are
    /// on the stack (and, for a pseudo-element, its originating element).
    pub(crate) fn unit_basis(&mut self) -> ContainerBasis {
        let Some(top) = self.nearest() else {
            return self.viewport;
        };
        if let Some(&basis) = self.bases.get(&top) {
            return basis;
        }
        let mut width = None;
        let mut height = None;
        let mut read = Vec::new();
        for entry in self.stack.iter().rev() {
            if width.is_some() && height.is_some() {
                break;
            }
            let style = &entry.style;
            if !style.size_containment_applies() {
                continue;
            }
            let (inline, block) = style.size_container_axes();
            let size = self.sizes.get(entry.node);
            let mut used = false;
            if inline && width.is_none() {
                width = Some(size.map(|s| s.0));
                used = true;
            }
            if block && height.is_none() {
                height = Some(size.map(|s| s.1));
                used = true;
            }
            if used {
                read.push((entry.node, size, (inline, block), Fonts::of(style)));
            }
        }
        for (node, size, axes, fonts) in read {
            self.log_entry(node, size, axes, fonts).units = true;
        }
        // A container without a size yet (no layout): the viewport, until
        // the next pass (the log asks for it).
        let basis = ContainerBasis {
            width: width.flatten().unwrap_or(self.viewport.width),
            height: height.flatten().unwrap_or(self.viewport.height),
        };
        self.bases.insert(top, basis);
        basis
    }
}

/// True if the component values have a dimension with a container query
/// unit (`cqw`, `cqh`, `cqi`, `cqb`, `cqmin`, `cqmax`), also inside
/// functions and blocks. The nesting of values is bounded by the parser.
pub(crate) fn has_container_units(values: &[ComponentValue]) -> bool {
    values.iter().any(|v| match v {
        ComponentValue::Dimension { unit, .. } => {
            unit.len() >= 3
                && unit
                    .get(..2)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case("cq"))
                && crate::values::LengthUnit::from_name(unit)
                    .is_some_and(crate::values::LengthUnit::is_container_relative)
        }
        ComponentValue::Function(f) => has_container_units(&f.arguments),
        ComponentValue::Block(b) => has_container_units(&b.contents),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_units_are_found_in_functions() {
        let has = |css: &str| has_container_units(&swb_css::parse_component_values(css));
        assert!(has("10cqw"));
        assert!(has("calc(1px + 2CQMIN)"));
        assert!(has("var(--x, 3cqb)"));
        assert!(!has("10vw 3px"));
        assert!(!has("10cq"));
        assert!(!has("cqw"));
    }

    #[test]
    fn the_log_holds_until_a_result_or_a_unit_changes() {
        let condition = Arc::new(ContainerCondition::parse_str("(width > 300px)").unwrap());
        let mut log = ContainerLog {
            entries: HashMap::new(),
            root_font_size: 16.0,
            viewport: (800.0, 600.0),
        };
        let a = NodeId::DOCUMENT;
        log.entries.insert(
            a,
            LogEntry {
                size: None,
                axes: (true, false),
                fonts: Fonts {
                    size: 16.0,
                    line_height: 19.0,
                },
                conditions: vec![(Arc::clone(&condition), false)],
                units: false,
            },
        );
        let mut sizes = ContainerSizes::new();
        assert!(log.holds(&sizes));
        sizes.insert(a, 200.0, 0.0);
        assert!(log.holds(&sizes), "still false at 200px");
        sizes.insert(a, 400.0, 0.0);
        assert!(!log.holds(&sizes), "true at 400px");
        if let Some(entry) = log.entries.get_mut(&a) {
            entry.conditions.clear();
            entry.units = true;
        }
        assert!(!log.holds(&sizes), "units read the size");
    }
}
