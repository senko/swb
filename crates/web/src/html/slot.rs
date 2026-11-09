//! `HTMLSlotElement` (HTML §4.12.4, <https://html.spec.whatwg.org/multipage/scripting.html#the-slot-element>):
//! the assigned nodes and the manual assignment. The lists are kept by
//! `swb_dom` (`slots.rs`; ADR 0036 section 2).

use swb_dom::NodeId;

use crate::bindings::{self as b, Cx};
use crate::generated as g;
use crate::generated::types::{AssignedNodesOptions, ElementOrText};
use crate::state::{WebState, dom, dom_mut};

impl g::html_slot_element::HTMLSlotElementImpl for WebState {
    /// <https://html.spec.whatwg.org/multipage/scripting.html#dom-slot-assignednodes>
    fn assigned_nodes(
        cx: &mut Cx<'_>,
        this: NodeId,
        options: AssignedNodesOptions,
    ) -> b::Result<Vec<NodeId>> {
        let dom = dom(cx.rt)?;
        Ok(if options.flatten {
            dom.flattened_assigned_nodes(this)
        } else {
            dom.assigned_nodes(this).to_vec()
        })
    }

    /// <https://html.spec.whatwg.org/multipage/scripting.html#dom-slot-assignedelements>
    fn assigned_elements(
        cx: &mut Cx<'_>,
        this: NodeId,
        options: AssignedNodesOptions,
    ) -> b::Result<Vec<NodeId>> {
        let nodes = Self::assigned_nodes(cx, this, options)?;
        let dom = dom(cx.rt)?;
        Ok(nodes
            .into_iter()
            .filter(|&n| dom.node(n).is_element())
            .collect())
    }

    /// <https://html.spec.whatwg.org/multipage/scripting.html#dom-slot-assign>
    fn assign(cx: &mut Cx<'_>, this: NodeId, nodes: Vec<ElementOrText>) -> b::Result<()> {
        let nodes: Vec<NodeId> = nodes
            .into_iter()
            .map(|n| match n {
                ElementOrText::Element(n) | ElementOrText::Text(n) => n,
            })
            .collect();
        dom_mut(cx.rt)?.assign_slot_manually(this, &nodes);
        // The call made the arena's shadow state if it had none.
        crate::dom::shadow::install_attention(cx.rt)?;
        if dom(cx.rt)?.has_slot_signals() {
            cx.rt.host_signal_cell().set(true);
        }
        Ok(())
    }
}
