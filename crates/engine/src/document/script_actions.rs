//! What scripts ask of the engine for focus, links and forms (M8 feature
//! 6d1, [`swb_web::EngineHost`]).
//!
//! A script task has no page, so a navigation (`a.click()`,
//! `form.submit()`) waits: it is stored in [`ScriptRequests`], and the page
//! acts on it when the task ends ([`crate::Page`], `page/script.rs`). The
//! scroll into view of `focus()` waits too, but only until the next
//! geometry query of the task (`geometry.rs`). Focus changes the element
//! states at once; the restyle that `:focus` rules need waits for the next
//! flush (`styles_stale`), so a loop of 100,000 `focus()` calls does not
//! compute the styles 100,000 times. Whether an element can be focused
//! uses the styles of the last flush (`display` and `visibility`), not a
//! layout; after a DOM change, the styles of the element and its
//! ancestors only (feature 7a, [`DocumentState::query_element_style`]).

use swb_dom::NodeId;
use swb_net::Url;
use swb_style::{Display, Visibility};

use super::DocumentState;
use crate::focus;
use crate::forms::{FormContext, FormRequest, Submitter, form_submission};
use crate::hit_test::link_target;

/// A navigation that a script asked for.
#[derive(Clone, Debug)]
pub(crate) enum ScriptNavigation {
    /// Follow a hyperlink (a fragment of this document scrolls).
    Link(Url),
    /// Submit a form.
    Form(FormRequest),
    /// `location.href = url`, `assign()` (`replace` false) and `replace()`
    /// to a URL that is not a fragment of this document.
    Location { url: Url, replace: bool },
    /// `location.reload()`.
    Reload,
}

/// The requests of the scripts of one task that the page acts on when the
/// task ends.
#[derive(Clone, Debug, Default)]
pub(crate) struct ScriptRequests {
    /// The last navigation request of the task.
    pub(crate) navigation: Option<ScriptNavigation>,
    /// The element that `focus()` scrolls into view: the scroll happens at
    /// the next geometry query of the task (so scripts see it at once, as
    /// in Chromium) or when the task ends, whichever comes first, so that a
    /// loop of `focus()` calls does not lay out at each call.
    pub(crate) scroll_into_view: Option<NodeId>,
    /// The changes of the session history that scripts made
    /// (`pushState()`, `replaceState()`, navigations to a fragment), in
    /// order; the page applies them before it acts on `navigation` and
    /// `traversal`.
    pub(crate) state_changes: Vec<super::history_api::StateChange>,
    /// The `history.go()`, `back()` and `forward()` calls of the task, in
    /// order (0 reloads; it is in the list at most once). At most
    /// [`MAX_PENDING_TRAVERSALS`]: later calls are dropped.
    pub(crate) traversals: Vec<i32>,
}

/// The most traversals of the session history that are waiting (in a task's
/// requests, and in the page's queue): a script that calls `back()`,
/// `forward()` and `go()` in a loop must not grow memory or keep the page
/// busy. Departure: Chromium throttles history calls by rate (not measured
/// further); swb drops the calls over this bound.
pub(crate) const MAX_PENDING_TRAVERSALS: usize = 64;

impl DocumentState {
    /// The requests that scripts made since the last call.
    pub(crate) fn take_script_requests(&mut self) -> ScriptRequests {
        std::mem::take(&mut self.script_requests)
    }

    /// The scroll offset of the viewport.
    pub(crate) fn script_scroll_offset(&self) -> (f64, f64) {
        (f64::from(self.scroll.x), f64::from(self.scroll.y))
    }

    /// True if `node` is connected, focusable and rendered.
    pub(crate) fn script_can_focus(&mut self, node: NodeId) -> bool {
        self.process_changes();
        if self.dom.element(node).is_none()
            || !self.dom.is_connected(node)
            || !focus::is_focusable(&self.dom, node)
            || self.dom.in_closed_details_content(node)
        {
            return false;
        }
        // The styles of the last flush are enough, also when only the
        // focus made them stale; computing them again for each call would
        // make a loop of `focus()` calls quadratic.
        if let Some(styles) = &self.styles {
            return styles.get(node).is_some_and(|s| {
                s.display != Display::None && s.visibility == Visibility::Visible
            });
        }
        // After a DOM change: the styles of the element and its ancestors
        // only, not of the whole document (a loop of "append an input,
        // `focus()`" would restyle the document at each step). The result
        // is that of a full restyle: an element is not rendered if it or
        // an ancestor has `display: none` (a full restyle gives such
        // descendants no style).
        let Some(style) = self.query_element_style(node) else {
            // Without styles (the style sheets have not loaded) nothing is
            // rendered yet; the element counts as rendered.
            return true;
        };
        if style.display == Display::None || style.visibility != Visibility::Visible {
            return false;
        }
        let dom = &self.dom;
        let styles = &self.element_styles;
        dom.ancestors(node)
            .take_while(|&n| n != NodeId::DOCUMENT)
            .all(|n| styles.get(&n).is_some_and(|s| s.display != Display::None))
    }

    /// Moves the focus. The styles that depend on `:focus` are outdated
    /// until the next flush.
    pub(crate) fn script_set_focus(&mut self, node: Option<NodeId>, visible: bool) {
        let mut states = self.states.clone();
        states.focus = node;
        states.focus_visible = visible && node.is_some();
        let changed = self.states.changes(&states);
        self.states = states;
        if changed.is_empty() {
            return;
        }
        self.clear_element_styles();
        let affected = self
            .stylist
            .as_ref()
            .is_none_or(|s| s.state_dependencies().intersects(changed));
        if affected {
            self.styles_stale = true;
            self.invalidate_layout();
        }
    }

    /// True if the user edited the text control `node` since its last
    /// `change` event and its value differs from the one before the edit.
    /// Forgets the edit of the control.
    pub(crate) fn script_take_pending_change(&mut self, node: NodeId) -> bool {
        if self
            .commit_baseline
            .as_ref()
            .is_none_or(|(n, _)| *n != node)
        {
            return false;
        }
        let Some((_, before)) = self.commit_baseline.take() else {
            return false;
        };
        self.script_control_value(node)
            .is_some_and(|value| value != before)
    }

    /// Notes that the user is about to edit the text control `node`: its
    /// value now is the one that `change` compares with, unless an earlier
    /// edit of the control noted it.
    pub(crate) fn note_user_edit(&mut self, node: NodeId) {
        if self
            .commit_baseline
            .as_ref()
            .is_some_and(|(n, _)| *n == node)
        {
            return;
        }
        let value = self.script_control_value(node).unwrap_or_default();
        self.commit_baseline = Some((node, value));
    }

    /// Stores the request to follow the link of `node` (an `a` or `area`
    /// element). False if it has no link.
    pub(crate) fn script_request_hyperlink(&mut self, node: NodeId) -> bool {
        self.process_changes();
        let Some(url) = link_target(&self.dom, node, self.base_url.value()) else {
            return false;
        };
        self.script_requests.navigation = Some(ScriptNavigation::Link(url));
        true
    }

    /// The invalid controls of `form`.
    pub(crate) fn script_invalid_controls(&mut self, form: NodeId) -> Vec<NodeId> {
        self.ensure_forms();
        self.forms.value().invalid_controls(&self.dom, form)
    }

    /// Stores the request to submit `form`. False if the submission does
    /// not navigate.
    pub(crate) fn script_request_form_submission(
        &mut self,
        form: NodeId,
        submitter: Option<NodeId>,
        coordinate: (i32, i32),
    ) -> bool {
        self.process_changes();
        self.ensure_forms();
        let cx = FormContext {
            doc: &self.dom,
            forms: self.forms.value(),
            base_url: self.base_url.value(),
            document_url: &self.url,
            encoding: self.encoding,
        };
        let submitter = match submitter {
            Some(node) => Submitter::Button {
                node,
                coordinate: Some(coordinate),
            },
            None => Submitter::Form,
        };
        let Some(request) = form_submission(&cx, form, submitter) else {
            return false;
        };
        if !self.may_load(&request.url) {
            log::warn!("not allowed to submit a form to {}", request.url);
            return false;
        }
        self.script_requests.navigation = Some(ScriptNavigation::Form(request));
        true
    }

    /// The reset algorithm of `form`. The values that scripts wrote to
    /// controls outside the document are forgotten.
    pub(crate) fn script_reset_form(&mut self, form: NodeId) {
        for node in self.dom.descendants(form).collect::<Vec<_>>() {
            self.seeds.forget(node);
        }
        self.ensure_forms();
        self.forms.value_mut().reset(&self.dom, form);
        self.controls_stale = true;
        self.invalidate_layout();
    }

    /// The checked radio button in the group of `radio`.
    pub(crate) fn script_checked_radio(&mut self, radio: NodeId) -> Option<NodeId> {
        self.ensure_forms();
        let forms = self.forms.value();
        forms
            .radio_group(&self.dom, radio)
            .into_iter()
            .find(|&n| forms.checked(n).unwrap_or(false))
    }
}
