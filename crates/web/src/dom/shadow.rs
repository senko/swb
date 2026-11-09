//! Shadow roots for scripts (DOM §4.8 `ShadowRoot`, §4.9 `attachShadow`,
//! `shadowRoot`, the `Slottable` mixin; ADR 0036 section 9). The tree work is
//! `swb_dom`'s (`shadow.rs`, `slots.rs`); this module converts and checks.
//!
//! A closed root is reachable only from the value that `attachShadow`
//! returned: `shadowRoot` and `assignedSlot` hide it and its slots.

use swb_dom::{AttachError, NodeData, NodeId, ShadowRootMode, SlotAssignment};

use crate::bindings::{self as b, Cx, Exception};
use crate::generated as g;
use crate::generated::types::{
    ShadowRootInit, ShadowRootMode as IdlMode, SlotAssignmentMode as IdlAssignment,
};
use crate::state::{WebState, dom, dom_mut};

/// `Element.attachShadow(init)`: DOM "attach a shadow root" with Chromium's
/// messages (measured, case `attach-errors`).
/// <https://dom.spec.whatwg.org/#dom-element-attachshadow>
pub(super) fn attach_shadow(
    cx: &mut Cx<'_>,
    this: NodeId,
    init: &ShadowRootInit,
) -> b::Result<NodeId> {
    let disabled = crate::custom_elements::shadow_disabled(cx.rt, this)?;
    let init = swb_dom::ShadowRootInit {
        mode: match init.mode {
            IdlMode::Open => ShadowRootMode::Open,
            IdlMode::Closed => ShadowRootMode::Closed,
        },
        slot_assignment: match init.slot_assignment {
            IdlAssignment::Named => SlotAssignment::Named,
            IdlAssignment::Manual => SlotAssignment::Manual,
        },
        delegates_focus: init.delegates_focus,
        clonable: init.clonable,
        serializable: init.serializable,
    };
    let result = dom_mut(cx.rt)?.attach_shadow(this, init, disabled);
    match result {
        Ok(root) => {
            install_attention(cx.rt)?;
            log::trace!("attachShadow: root {root:?} of {this:?}");
            Ok(root)
        }
        Err(AttachError::Unsupported) => Err(Exception::dom(
            "NotSupportedError",
            "This element does not support attachShadow",
        )),
        Err(AttachError::Disabled) => Err(Exception::dom(
            "NotSupportedError",
            "attachShadow() is disabled by disabledFeatures static field.",
        )),
        Err(AttachError::Exists) => Err(Exception::dom(
            "NotSupportedError",
            "Shadow root cannot be created on a host which already hosts a shadow tree.",
        )),
        Err(AttachError::Limit(error)) => super::dom_result(cx.rt, Err(error)),
    }
}

/// Gives the arena the runtime's signal cell, which it sets when a
/// `slotchange` is due or its slot assignment did much work
/// (`mutation_observer::after_request` looks then).
pub(crate) fn install_attention(rt: &mut swb_js::Runtime) -> swb_js::VmResult<()> {
    let cell = rt.host_signal_cell();
    dom_mut(rt)?.set_shadow_attention(cell);
    Ok(())
}

/// `Element.shadowRoot`: the root of an open shadow root.
/// <https://dom.spec.whatwg.org/#dom-element-shadowroot>
pub(super) fn shadow_root(cx: &Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
    let dom = dom(cx.rt)?;
    Ok(dom.shadow_root(this).filter(|&root| {
        dom.shadow_root_data(root)
            .is_some_and(|d| d.mode == ShadowRootMode::Open)
    }))
}

/// `Slottable.assignedSlot`: DOM "find a slot" with the open flag.
/// <https://dom.spec.whatwg.org/#dom-slotable-assignedslot>
pub(super) fn assigned_slot(cx: &Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
    let dom = dom(cx.rt)?;
    if !dom.has_shadow_state() {
        return Ok(None);
    }
    Ok(dom.find_slot(this, true))
}

/// The data of the shadow root `this`.
fn data(cx: &Cx<'_>, this: NodeId) -> b::Result<swb_dom::ShadowRootData> {
    dom(cx.rt)?
        .shadow_root_data(this)
        .copied()
        .ok_or_else(|| Exception::type_error("Illegal invocation"))
}

/// `Node.getRootNode(options)`: the root, or with `composed` the
/// shadow-including root.
/// <https://dom.spec.whatwg.org/#dom-node-getrootnode>
pub(super) fn root_node(cx: &Cx<'_>, this: NodeId, composed: bool) -> b::Result<NodeId> {
    let dom = dom(cx.rt)?;
    Ok(if composed {
        dom.shadow_including_root(this)
    } else {
        dom.root(this)
    })
}

impl g::shadow_root::ShadowRootImpl for WebState {
    fn mode(cx: &mut Cx<'_>, this: NodeId) -> b::Result<IdlMode> {
        Ok(match data(cx, this)?.mode {
            ShadowRootMode::Open => IdlMode::Open,
            ShadowRootMode::Closed => IdlMode::Closed,
        })
    }

    fn delegates_focus(cx: &mut Cx<'_>, this: NodeId) -> b::Result<bool> {
        Ok(data(cx, this)?.delegates_focus)
    }

    fn slot_assignment(cx: &mut Cx<'_>, this: NodeId) -> b::Result<IdlAssignment> {
        Ok(match data(cx, this)?.slot_assignment {
            SlotAssignment::Named => IdlAssignment::Named,
            SlotAssignment::Manual => IdlAssignment::Manual,
        })
    }

    fn clonable(cx: &mut Cx<'_>, this: NodeId) -> b::Result<bool> {
        Ok(data(cx, this)?.clonable)
    }

    fn serializable(cx: &mut Cx<'_>, this: NodeId) -> b::Result<bool> {
        Ok(data(cx, this)?.serializable)
    }

    fn host(cx: &mut Cx<'_>, this: NodeId) -> b::Result<NodeId> {
        Ok(data(cx, this)?.host)
    }

    fn inner_html(cx: &mut Cx<'_>, this: NodeId) -> b::Result<String> {
        Ok(swb_dom::inner_html(dom(cx.rt)?, this))
    }

    /// The fragment is parsed in the context of the host (HTML "the
    /// `innerHTML` setter": the context of a shadow root is its host).
    fn set_inner_html(cx: &mut Cx<'_>, this: NodeId, value: String) -> b::Result<()> {
        let host = data(cx, this)?.host;
        super::markup::set_root_inner_html(cx, this, host, &value)
    }

    fn style_sheets(cx: &mut Cx<'_>, this: NodeId) -> b::Result<swb_js::Gc<swb_js::Object>> {
        crate::cssom_sheets::style_sheets(cx, this)
    }

    /// HTML `DocumentOrShadowRoot.activeElement` of a root: the focused
    /// element retargeted against the root, if that is in the root's tree.
    /// <https://html.spec.whatwg.org/multipage/interaction.html#dom-documentorshadowroot-activeelement>
    fn active_element(cx: &mut Cx<'_>, this: NodeId) -> b::Result<Option<NodeId>> {
        let Some(focused) = crate::html::focus::focused(cx.rt)? else {
            return Ok(None);
        };
        let dom = dom(cx.rt)?;
        if dom.get(focused).is_none() {
            return Ok(None);
        }
        let candidate = retarget(dom, focused, this);
        Ok((dom.root(candidate) == this).then_some(candidate))
    }
}

/// DOM "retarget `a` against `b`": while `a`'s root is a shadow root that is
/// not a shadow-including inclusive ancestor of `b`, `a` becomes that root's
/// host.
/// <https://dom.spec.whatwg.org/#retarget>
pub(crate) fn retarget(dom: &swb_dom::Document, mut a: NodeId, b: NodeId) -> NodeId {
    loop {
        let root = dom.root(a);
        let Some(host) = dom.shadow_host(root) else {
            return a;
        };
        if dom.is_shadow_including_inclusive_ancestor(root, b) {
            return a;
        }
        a = host;
    }
}

/// True if `node` is a shadow root (the conversions of `cloneNode`,
/// `importNode` and `adoptNode` refuse one).
pub(crate) fn is_shadow_root(dom: &swb_dom::Document, node: NodeId) -> bool {
    matches!(dom.node(node).data, NodeData::ShadowRoot(_))
}
