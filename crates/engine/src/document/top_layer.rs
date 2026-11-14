//! The engine's host of the top-layer algorithms of `swb_dom` (ADR 0037
//! section 2), for the steps that no script runs: in a document without
//! scripts no listener can see an event, so the events are not fired (none
//! is canceled) and a `toggle` task only clears its state. The invokers,
//! light dismiss and close requests with JavaScript off (sub-feature 6c)
//! use it; the drain runs the attribute change steps of `popover` that the
//! bindings did not run with it.

use std::time::{Duration, Instant};

use swb_dom::{Document, NodeId, PopoverError, ToggleState, TopLayerHost};

use super::DocumentState;

/// How long a user's activation counts as transient (HTML "transient
/// activation duration"; Chromium's five seconds).
const TRANSIENT_ACTIVATION: Duration = Duration::from_secs(5);

/// The host of a document without scripts. It focuses (the popover focusing
/// steps and the restoration of the focus) unless `focus` is false.
pub(crate) struct NoScriptHost<'a> {
    pub(crate) state: &'a mut DocumentState,
    /// False for the steps that run inside the drain of the changes: the
    /// focusing steps read the styles, which the drain makes.
    pub(crate) focus: bool,
}

impl<'a> NoScriptHost<'a> {
    /// A host that focuses.
    pub(crate) fn new(state: &'a mut DocumentState) -> Self {
        NoScriptHost { state, focus: true }
    }
}

impl TopLayerHost for NoScriptHost<'_> {
    fn doc(&mut self) -> &mut Document {
        &mut self.state.dom
    }

    fn fire_beforetoggle(
        &mut self,
        _element: NodeId,
        _old: ToggleState,
        _new: ToggleState,
        _cancelable: bool,
        _source: Option<NodeId>,
    ) -> Option<bool> {
        Some(true)
    }

    fn queue_toggle_task(&mut self, element: NodeId, serial: u64) {
        // No listener can see the `toggle` event: the task is done at once.
        let _ = self.state.dom.take_toggle(element, serial);
    }

    fn focused_element(&mut self) -> Option<NodeId> {
        self.focus.then_some(self.state.states.focus).flatten()
    }

    fn focus(&mut self, element: NodeId) -> bool {
        if !self.focus || !self.state.script_can_focus(element) {
            return false;
        }
        self.state.script_set_focus(Some(element), false);
        true
    }
}

impl DocumentState {
    /// Runs the attribute change steps of the showing popovers whose
    /// `popover` attribute changed state without the bindings (a document
    /// without scripts): they hide.
    pub(super) fn run_popover_attribute_steps(&mut self) {
        let mut host = NoScriptHost {
            state: self,
            focus: false,
        };
        if let Err(error) = swb_dom::run_popover_attribute_steps(&mut host) {
            log_error(error);
        }
    }

    /// The user pressed a button or a key in this document or in a frame
    /// that the activation reaches (HTML "activation notification"): the
    /// window has an activation now, and the close watcher manager may allow
    /// a new group.
    pub(crate) fn note_user_activation(&mut self, now: Instant) {
        self.user_activation = Some(now);
        self.dom.note_user_activation();
    }

    /// Sticky and transient activation of the window (HTML "has sticky
    /// activation", "has transient activation").
    pub(crate) fn activation_state(&self) -> (bool, bool) {
        match self.user_activation {
            Some(at) => (true, at.elapsed() <= TRANSIENT_ACTIVATION),
            None => (false, false),
        }
    }
}

/// Logs an error of a top-layer algorithm of the engine at `debug!` (ADR
/// 0037 section 2).
pub(crate) fn log_error(error: PopoverError) {
    log::debug!("top layer: {error}");
}

#[cfg(test)]
mod tests {
    use swb_dom::{HideOptions, hide_popover, parse_html, show_popover, toggle_popover};

    use super::*;

    /// A document state for the host, from `html`.
    fn state_of(html: &str) -> DocumentState {
        crate::document::tests::state(
            parse_html(html),
            None,
            &"https://example.test/".parse().expect("url"),
        )
    }

    #[test]
    fn shows_and_hides_without_events() {
        let mut state = state_of("<div id=a popover>a</div><div id=b popover>b</div>");
        let a = state.dom.element_by_id("a").expect("a");
        let b = state.dom.element_by_id("b").expect("b");
        let mut host = NoScriptHost::new(&mut state);
        show_popover(&mut host, a, true, None).expect("shows");
        assert!(host.state.dom.is_popover_showing(a));
        // An auto popover closes the other.
        show_popover(&mut host, b, true, None).expect("shows");
        assert!(!host.state.dom.is_popover_showing(a));
        assert_eq!(toggle_popover(&mut host, b, None, None), Ok(false));
        assert!(hide_popover(&mut host, b, HideOptions::default()).is_ok());
        // The toggle tasks were done at once: nothing is kept.
        let dom = &host.state.dom;
        assert!(!dom.has_pending_toggle(a) && !dom.has_pending_toggle(b));
        assert_eq!(dom.top_layer().expect("state").layer().len(), 0);
        assert_eq!(dom.showing_popover_lists(), (&[][..], &[][..]));
    }

    #[test]
    fn attribute_steps_that_no_script_ran() {
        let mut state = state_of("<div id=a popover>a</div>");
        let a = state.dom.element_by_id("a").expect("a");
        show_popover(&mut NoScriptHost::new(&mut state), a, true, None).expect("shows");
        state.dom.set_attr(a, "popover", "manual").expect("set");
        assert!(state.dom.is_popover_showing(a));
        state.run_popover_attribute_steps();
        assert!(!state.dom.is_popover_showing(a));
    }

    #[test]
    fn activation_state_is_sticky_and_transient() {
        let mut state = state_of("<p>x</p>");
        assert_eq!(state.activation_state(), (false, false));
        state.note_user_activation(Instant::now());
        assert_eq!(state.activation_state(), (true, true));
        state.note_user_activation(
            Instant::now()
                .checked_sub(Duration::from_secs(6))
                .expect("an instant six seconds ago"),
        );
        assert_eq!(state.activation_state(), (true, false));
    }
}
