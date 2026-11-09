//! Upgrades (HTML §4.13.5,
//! <https://html.spec.whatwg.org/multipage/custom-elements.html#upgrades>;
//! ADR 0035 section 4): "upgrade an element", "try to upgrade an element",
//! the upgrade candidates of a new definition, and "enqueue a custom
//! element callback reaction".

use swb_dom::{CustomElementState, LocalName, NodeId, ns};
use swb_js::{ScriptError, ThrowKind, Value, VmError, VmResult};

use super::reactions::{self, Args, Place, Reaction};
use super::{Callback, StackEntry, definition, definition_mut};
use crate::state::{dom, dom_mut};
use swb_js::Runtime;

/// The nodes that a walk visits between two checks of the time limit.
const NODES_PER_CHECK: usize = 4096;

/// HTML "enqueue a custom element callback reaction" for the element at
/// `place` (a node of the current document) with the definition `number`:
/// nothing if the definition has no such callback, or for
/// `attributeChangedCallback` if the attribute's local name is not observed.
pub(crate) fn enqueue_callback(
    rt: &mut Runtime,
    place: Place,
    number: u32,
    callback: Callback,
    args: Args,
) -> VmResult<()> {
    let definition = definition(rt, number)?;
    if definition.callback(callback).is_none() {
        return Ok(());
    }
    if let Args::Attribute { local, .. } = &args
        && !definition.observed.contains(local)
    {
        return Ok(());
    }
    reactions::enqueue(rt, place, Reaction::Callback(callback, args))
}

/// HTML "enqueue a custom element upgrade reaction".
pub(crate) fn enqueue_upgrade(rt: &mut Runtime, place: Place, number: u32) -> VmResult<()> {
    reactions::enqueue(rt, place, Reaction::Upgrade(number))
}

/// HTML "try to upgrade an element" for an element of the current
/// document: an upgrade reaction if its node document's registry has a
/// definition of its name.
pub(crate) fn try_upgrade(rt: &mut Runtime, element: NodeId) -> VmResult<()> {
    let dom = dom(rt)?;
    let Some(data) = dom.element(element).filter(|e| e.name.ns == ns!(html)) else {
        return Ok(());
    };
    let Some(number) = super::look_up(rt, dom.node_document(element), &data.name.local) else {
        return Ok(());
    };
    let key = crate::state::document_key(rt)?;
    enqueue_upgrade(rt, (key, element), number)
}

/// The upgrade candidates of a new definition: an upgrade reaction for each
/// element of the window's document, in tree order, in the HTML namespace
/// with the definition's local name (HTML `define` step "upgrade
/// candidates"; shadow trees join with feature 5). The arena's set of
/// created custom element names skips the walk for a name that no element
/// has.
/// Returns the number of candidates.
pub(crate) fn upgrade_candidates(
    rt: &mut Runtime,
    number: u32,
    local: &LocalName,
) -> VmResult<usize> {
    let dom = dom(rt)?;
    if !dom.created_custom_name(local) {
        return Ok(0);
    }
    let mut visited = 0;
    let candidates: Vec<NodeId> = dom
        .descendants(NodeId::DOCUMENT)
        .inspect(|_| visited += 1)
        .filter(|&node| {
            dom.element(node)
                .is_some_and(|e| e.name.ns == ns!(html) && e.name.local == *local)
        })
        .collect();
    charge_walk(rt, visited)?;
    let key = crate::state::document_key(rt)?;
    let count = candidates.len();
    for node in candidates {
        enqueue_upgrade(rt, (key, node), number)?;
    }
    Ok(count)
}

/// `customElements.upgrade(root)`: tries to upgrade each inclusive
/// descendant of `root` in tree order.
pub(crate) fn upgrade_subtree(rt: &mut Runtime, root: NodeId) -> VmResult<()> {
    if !super::any_definitions(rt) {
        return Ok(());
    }
    let dom = dom(rt)?;
    let mut visited = 0;
    let elements: Vec<NodeId> = dom
        .descendants(root)
        .inspect(|_| visited += 1)
        .filter(|&node| dom.element(node).is_some())
        .collect();
    charge_walk(rt, visited)?;
    for element in elements {
        try_upgrade(rt, element)?;
    }
    Ok(())
}

/// The time check after a walk of `visited` nodes (native work that the
/// countdown of the VM does not see).
fn charge_walk(rt: &mut Runtime, visited: usize) -> VmResult<()> {
    if visited >= NODES_PER_CHECK {
        rt.check_time()?;
    }
    Ok(())
}

/// Runs an upgrade reaction: upgrades the element and reports an error.
pub(crate) fn upgrade_reaction(rt: &mut Runtime, place: Place, number: u32) -> VmResult<()> {
    let entered = crate::state::switch_if_other(rt, place.0)?;
    let result = upgrade(rt, place.1, number);
    crate::state::leave(rt, entered);
    result
}

/// Why an upgrade failed.
enum Failure {
    /// The constructor threw.
    Thrown(ScriptError),
    /// The constructor returned another object.
    NotTheElement,
}

/// HTML "upgrade an element" of an element of the current document.
/// <https://html.spec.whatwg.org/multipage/custom-elements.html#concept-upgrade-an-element>
fn upgrade(rt: &mut Runtime, element: NodeId, number: u32) -> VmResult<()> {
    let dom = dom(rt)?;
    let Some(data) = dom.element(element) else {
        return Ok(());
    };
    // Step 1.
    if !matches!(
        data.custom_element_state(),
        CustomElementState::Undefined | CustomElementState::Uncustomized
    ) {
        return Ok(());
    }
    let wanted = definition(rt, number)?;
    let attributes = observed_attributes(
        data,
        wanted.callback(Callback::AttributeChanged).is_some(),
        &wanted.observed,
    );
    let connected = dom.is_connected(element);
    let key = crate::state::document_key(rt)?;
    let place = (key, element);
    // Steps 2 to 5.
    dom_mut(rt)?.set_custom_element_state(element, CustomElementState::Failed, Some(number));
    for args in attributes {
        enqueue_callback(rt, place, number, Callback::AttributeChanged, args)?;
    }
    if connected {
        enqueue_callback(rt, place, number, Callback::Connected, Args::None)?;
    }
    // Steps 6 to 9.
    definition_mut(rt, number)?
        .construction_stack
        .push(StackEntry::Element(key, element));
    dom_mut(rt)?.set_custom_element_state(element, CustomElementState::Precustomized, None);
    let constructor = definition(rt, number)?.constructor;
    let result = rt.construct_function(Value::Object(constructor), &[]);
    definition_mut(rt, number)?.construction_stack.pop();
    let failure = match result {
        Ok(value) => {
            let wrapper = crate::state::existing_wrapper(rt, element)?;
            (wrapper.map(Value::Object) != Some(value)).then_some(Failure::NotTheElement)
        }
        Err(error) => Some(Failure::Thrown(error)),
    };
    let Some(failure) = failure else {
        // Step 11.
        dom_mut(rt)?.set_custom_element_state(element, CustomElementState::Custom, None);
        log::debug!(
            "upgraded a custom element: {}",
            definition(rt, number)?.name
        );
        return Ok(());
    };
    log::debug!(
        "the upgrade of a custom element failed: {}",
        definition(rt, number)?.name
    );
    // The failure steps: no definition, no reactions; the state stays
    // `failed` or `precustomized` (measured: not `:defined`, never upgraded
    // again).
    let (state, _) = crate::state::dom(rt)?.custom_element_state(element);
    dom_mut(rt)?.set_custom_element_state(element, state, Some(0));
    reactions::shared(rt)?.borrow_mut().clear(place);
    match failure {
        Failure::Thrown(ScriptError::Terminated(termination)) => {
            Err(VmError::Terminated(termination))
        }
        Failure::Thrown(ScriptError::Internal(error)) => Err(VmError::Internal(error)),
        Failure::Thrown(error) => {
            let site = crate::errors::Site::named("a custom element constructor");
            crate::errors::report_callback(rt, constructor, &site, &error)
        }
        Failure::NotTheElement => {
            let error = rt.new_error(
                ThrowKind::TypeError,
                "custom element constructors must call super() first and must not return a different object",
            )?;
            crate::errors::report_host_error(rt, Value::Object(error), false)
        }
    }
}

/// The `attributeChangedCallback` arguments of the upgrade for the attributes
/// of `data`: only the observed ones, and none if the definition has no
/// callback. `enqueue_callback` would drop the others, but only after their
/// values were copied.
fn observed_attributes(
    data: &swb_dom::ElementData,
    has_callback: bool,
    observed: &std::collections::HashSet<LocalName>,
) -> Vec<Args> {
    if !has_callback {
        return Vec::new();
    }
    data.attributes()
        .iter()
        .filter(|a| observed.contains(&a.name.local))
        .map(|a| Args::Attribute {
            local: a.name.local.clone(),
            old: None,
            new: Some(a.value.clone()),
            namespace: (a.name.ns != ns!()).then(|| a.name.ns.to_string()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use swb_dom::{Attribute, ElementData, QualName};

    use super::*;

    fn element() -> ElementData {
        let attribute = |name: &str, value: &str| Attribute {
            name: QualName::new(None, ns!(), LocalName::from(name)),
            value: value.to_owned(),
        };
        ElementData::new(
            QualName::new(None, ns!(html), LocalName::from("x-a")),
            vec![
                attribute("a", "1"),
                attribute("b", "2"),
                attribute("c", "3"),
            ],
        )
    }

    /// An upgrade copies the values of the observed attributes only.
    #[test]
    fn the_upgrade_copies_only_the_observed_attributes() {
        let observed: HashSet<LocalName> = [LocalName::from("b")].into_iter().collect();
        let args = observed_attributes(&element(), true, &observed);
        assert_eq!(
            args,
            vec![Args::Attribute {
                local: LocalName::from("b"),
                old: None,
                new: Some("2".to_owned()),
                namespace: None,
            }]
        );
        assert_eq!(observed_attributes(&element(), false, &observed), vec![]);
    }
}
