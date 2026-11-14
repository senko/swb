//! Popovers and the user's input (ADR 0037 sections 3 to 6, sub-feature 6c):
//! the user's activations for the close watchers, light dismiss at a press
//! and a release, the close requests of the Escape key, and the invokers
//! that the engine activates itself (a document without scripts).
//!
//! With scripts the algorithms run in the script runtime (`swb_web`), whose
//! host fires the events; without scripts they run here with
//! [`NoScriptHost`](crate::document::top_layer::NoScriptHost), which fires
//! none. Both use the algorithms of `swb_dom::top_layer`.

use std::time::Instant;

use swb_dom::{Document, NodeId, TargetAction};

use super::Page;
use crate::document::top_layer::{NoScriptHost, log_error};
use crate::frames::FrameId;

/// The popover that an invoker names, for one walk of a click's path.
type Targets = Vec<(NodeId, NodeId)>;

/// Looks up the popover that the invoker `node` names in `targets`.
fn lookup(targets: &Targets, node: NodeId) -> Option<NodeId> {
    targets
        .iter()
        .find(|&&(invoker, _)| invoker == node)
        .map(|&(_, popover)| popover)
}

impl Page {
    /// The user pressed a button or a key in the top-level document: gives
    /// it, its frames of the same origin, and nothing else, a user
    /// activation (HTML "activation notification"). The engine's own input
    /// does it; a client that starts a script by other means (the probe
    /// tool's evaluations count as user gestures in Chromium) can call it.
    pub fn note_user_activation(&mut self) {
        self.note_activation(FrameId::TOP);
    }

    /// The user pressed a button or a key in the document of `frame`: the
    /// frame, its ancestors and the frames of its origin have an activation
    /// (`navigator.userActivation`), and their close watcher managers may
    /// allow a new group.
    pub(super) fn note_activation(&mut self, frame: FrameId) {
        let reached = self.frames.note_activation(frame);
        let now = Instant::now();
        for id in reached {
            if let Some(doc) = self.doc_of_mut(id) {
                doc.note_user_activation(now);
            }
        }
    }

    /// The root element of the document: the target of a press outside every
    /// box (it is on the canvas of the root element).
    pub(super) fn root_element(&self) -> Option<NodeId> {
        self.document()?.element_children(NodeId::DOCUMENT).next()
    }

    /// Runs `f` with the host of a document without scripts. Returns its
    /// result and whether the document changed (a repaint is needed). The
    /// focus that the algorithm moved goes through the page's bookkeeping
    /// of the form controls.
    pub(super) fn run_no_script<R>(
        &mut self,
        f: impl FnOnce(&mut NoScriptHost<'_>) -> R,
    ) -> Option<(R, bool)> {
        let old_focus = self.focused_element();
        let doc = self.doc.as_deref_mut()?;
        let generation = doc.dom.generation();
        let mut host = NoScriptHost::new(doc);
        let result = f(&mut host);
        let changed = host.state.dom.generation() != generation;
        if self.focused_element() != old_focus {
            self.focus_changed(old_focus);
        }
        Some((result, changed))
    }

    // ----- Invokers -----

    /// The popover that the element `node` invokes when it is clicked, in a
    /// document without scripts: it is a button that acts as an invoker (a
    /// `button` or an `input` of type `button`, `submit`, `reset` or `image`
    /// that is not disabled, and, for the types that submit or reset, has
    /// no form owner) whose `popovertarget` names a popover in its tree (HTML
    /// "popover target attribute activation behavior"; Chromium, measured,
    /// `invokers-types`).
    pub(super) fn invoker_popover(&self, doc: &Document, node: NodeId) -> Option<NodeId> {
        if !swb_dom::can_invoke(doc, node) || swb_style::is_actually_disabled(doc, node) {
            return None;
        }
        if swb_dom::invoker_acts_on_form(doc, node)
            && self.forms().is_some_and(|f| f.owner(node).is_some())
        {
            return None;
        }
        swb_dom::popover_target_by_id(doc, node).filter(|&t| doc.popover_state(t).is_some())
    }

    /// The first invoker among `nodes` (a part of the path of a click, the
    /// target first), before a control that takes the click for itself (a
    /// text field, a file or a color field; Chromium, measured,
    /// `invoker-inner-*`).
    pub(super) fn first_invoker(&self, doc: &Document, nodes: &[NodeId]) -> Option<NodeId> {
        for &node in nodes {
            if swb_dom::swallows_invoker_click(doc, node) {
                return None;
            }
            if self.invoker_popover(doc, node).is_some() {
                return Some(node);
            }
        }
        None
    }

    /// The activation behavior of the invoker `invoker` for a click whose
    /// target was `origin`. Returns true if the document changed.
    pub(super) fn activate_invoker(&mut self, invoker: NodeId, origin: NodeId) -> bool {
        let Some(doc) = self.document() else {
            return false;
        };
        let Some(popover) = self.invoker_popover(doc, invoker) else {
            return false;
        };
        let action = TargetAction::of(
            doc.element(invoker)
                .and_then(|e| e.attr("popovertargetaction")),
        );
        self.run_no_script(|host| {
            swb_dom::popover_target_activation(host, invoker, popover, (action, origin))
        })
        .is_some_and(|(result, changed)| {
            if let Err(error) = result {
                log_error(error);
            }
            changed
        })
    }

    // ----- Light dismiss -----

    /// The popovers that the invokers on the flat tree path of `node` name
    /// (for the "nearest inclusive target popover" of light dismiss), for a
    /// document without scripts.
    fn targets_on_path(&self, node: NodeId) -> Targets {
        let Some(doc) = self.document() else {
            return Vec::new();
        };
        std::iter::once(node)
            .chain(doc.flat_ancestors(node))
            .filter_map(|n| self.invoker_popover(doc, n).map(|p| (n, p)))
            .collect()
    }

    /// A pointer button went down on `node` (the target of the events):
    /// notes the topmost clicked popover (HTML "light dismiss open
    /// popovers", at `pointerdown`). One test when no popover shows and
    /// none is noted.
    pub(super) fn light_dismiss_down(&mut self, node: NodeId) {
        let Some(doc) = self.document() else {
            return;
        };
        if !doc.light_dismiss_active() && doc.popover_pointerdown_target().is_none() {
            return;
        }
        if self.script_input() {
            self.run_task(|script| script.light_dismiss_down(node));
            return;
        }
        let targets = self.targets_on_path(node);
        if let Some(doc) = self.doc.as_deref_mut() {
            swb_dom::light_dismiss_down(&mut doc.dom, node, |n| lookup(&targets, n));
        }
    }

    /// A pointer button went up on `node`: hides the popovers that the
    /// press and the release both left outside (HTML "light dismiss open
    /// popovers", at `pointerup`, before the listeners of the event).
    /// Returns true if the document changed.
    pub(super) fn light_dismiss_up(&mut self, node: NodeId) -> bool {
        let Some(doc) = self.document() else {
            return false;
        };
        if !doc.light_dismiss_active() && doc.popover_pointerdown_target().is_none() {
            return false;
        }
        if self.script_input() {
            let generation = self.dom_generation();
            self.run_task(|script| script.light_dismiss_up(node));
            return generation != self.dom_generation();
        }
        let targets = self.targets_on_path(node);
        self.run_no_script(|host| swb_dom::light_dismiss_up(host, node, |n| lookup(&targets, n)))
            .is_some_and(|(result, changed)| {
                if let Err(error) = result {
                    log_error(error);
                }
                changed
            })
    }

    // ----- Close requests -----

    /// The default action of Escape: requests close of the newest close
    /// watchers (HTML "process close watchers"). Returns true if a watcher
    /// closed something.
    pub(super) fn process_close_watchers(&mut self) -> bool {
        if !self.document().is_some_and(Document::has_close_watchers) {
            return false;
        }
        if self.script_input() {
            return self
                .run_task(swb_web::ScriptRuntime::process_close_watchers)
                .and_then(Result::ok)
                .unwrap_or(false);
        }
        self.run_no_script(|host| swb_dom::process_close_watchers(host))
            .is_some_and(|(result, _)| match result {
                Ok(processed) => processed,
                Err(error) => {
                    log_error(error);
                    false
                }
            })
    }
}
