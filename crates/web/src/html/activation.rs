//! Activation behavior, `click()` and the form methods (HTML §4.10.21.3
//! "Form submission", §4.10.21.4 "Resetting a form", the activation
//! behaviors of `a`, `input`, `button` and `label`; DOM §2.9
//! "dispatch", steps for the activation target):
//! <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#form-submission-algorithm>.
//!
//! A `click` event that is a `MouseEvent` (also one that a script
//! dispatches) has an *activation target*: the event's target, or, if the
//! event bubbles, its nearest ancestor with an activation behavior. A
//! checkbox or a radio button changes its state *before* the event (the
//! legacy-pre-activation behavior) and returns to the old state if a
//! listener cancels the event; if not, `input` and `change` follow. A
//! link, a submit button, a reset button and a label act after the
//! dispatch.
//!
//! What is the engine's goes through [`crate::EngineHost`]: the
//! checkedness of controls, the invalid controls of a form, the reset of
//! its controls, and the requests that the page acts on when the task ends
//! (a navigation, a scroll).
//!
//! Feature 6d2 adds the user's clicks (the engine's input is dispatched as
//! trusted events, and the activation behavior of this module replaces the
//! engine's own action), the `summary` of a `details` element, the
//! `javascript:` URLs of links and form actions, and the simulated click of
//! a key (`simulated_click`).
//!
//! Measured in Chromium 148 (`tools/probes/web-input-handlers.json`):
//! `dispatchEvent` of a `click` `MouseEvent` activates a disabled checkbox;
//! only `click()` stops at disabled controls; a radio button that is
//! already checked fires no `input` and `change`; a disconnected control
//! fires none either; the `submit` event is a trusted `SubmitEvent` that
//! bubbles, can be canceled and is not composed; `input` is an `Event`
//! (bubbles, composed), `change` an `Event` (bubbles).

use std::collections::HashSet;

use swb_dom::{Document, NodeId, QualName, local_name, ns};
use swb_js::{Runtime, ScriptError, VmError, VmResult};

use super::{form_owner, labeled_control};
use crate::bindings::{self as b, Cx, Host};
use crate::event_loop::Task;
use crate::events::{
    DispatchTarget, EventInterface, TrustedEvent, dispatch, fire_trusted, handlers, new_click_event,
};
use crate::state::{WebState, dom, engine_mut, state, state_mut};

/// The elements that are in the middle of an operation that must not
/// start again (the flags of the HTML algorithms).
#[derive(Debug, Default)]
pub(crate) struct Busy {
    /// "Locked for focus".
    focus: HashSet<NodeId>,
    /// "Click in progress flag".
    click: HashSet<NodeId>,
    /// "Firing submission events".
    submit: HashSet<NodeId>,
    /// "Locked for reset".
    reset: HashSet<NodeId>,
    /// A label that is dispatching the click at its control.
    label: HashSet<NodeId>,
}

/// Which activation behavior an element has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Link,
    Checkbox,
    Radio,
    Submit,
    Reset,
    Label,
    /// The first `summary` child of a `details` element.
    Summary,
}

/// What the click that submits a form tells the submission.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ClickInfo {
    /// The point of the click on an image button.
    coordinate: (i32, i32),
    /// True if the click opens the target in a new tab, a new window or a
    /// download (Ctrl, Shift or Alt held).
    elsewhere: bool,
}

/// The state that the legacy-pre-activation behavior saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Saved {
    None,
    Checkbox {
        checked: bool,
        indeterminate: bool,
    },
    Radio {
        was_checked: bool,
        previous: Option<NodeId>,
    },
}

/// The activation target of a `click` event.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Activation {
    node: NodeId,
    /// The target of the event (the activation target or a descendant).
    origin: NodeId,
    kind: Kind,
    saved: Saved,
}

/// The activation behavior of an element.
fn kind_of(dom: &Document, node: NodeId) -> Option<Kind> {
    let e = dom.element(node)?;
    if e.name.ns == ns!(svg) {
        let link = *e.local_name() == local_name!("a") && e.svg_href().is_some();
        return link.then_some(Kind::Link);
    }
    if e.name.ns != ns!(html) {
        return None;
    }
    let ty = || e.attr("type").map(str::to_ascii_lowercase);
    match &**e.local_name() {
        "a" | "area" => e.has_attr("href").then_some(Kind::Link),
        "label" => Some(Kind::Label),
        "summary" => dom.details_of_summary(node).map(|_| Kind::Summary),
        "input" => match ty().as_deref() {
            Some("checkbox") => Some(Kind::Checkbox),
            Some("radio") => Some(Kind::Radio),
            Some("submit" | "image") => Some(Kind::Submit),
            Some("reset") => Some(Kind::Reset),
            _ => None,
        },
        "button" => match ty().as_deref() {
            Some("reset") => Some(Kind::Reset),
            Some("button") => None,
            _ => Some(Kind::Submit),
        },
        _ => None,
    }
}

/// The activation target for a click on `node`: the node, or with
/// `bubbles` the nearest ancestor with an activation behavior.
pub(crate) fn find_target(
    rt: &Runtime,
    node: NodeId,
    bubbles: bool,
) -> VmResult<Option<Activation>> {
    let node_target = node;
    let dom = dom(rt)?;
    // Interactive content between the target and a label or a summary
    // keeps it from acting (HTML "interactive content"; Chromium).
    let mut in_interactive = false;
    let found = std::iter::once(node)
        .chain(
            dom.ancestors(node)
                .take(if bubbles { usize::MAX } else { 0 }),
        )
        .find_map(|n| {
            let kind = kind_of(dom, n);
            let blocked = in_interactive && matches!(kind, Some(Kind::Label | Kind::Summary));
            in_interactive |= is_interactive(dom, n);
            kind.filter(|_| !blocked).map(|kind| (n, kind))
        });
    Ok(found.map(|(node, kind)| Activation {
        node,
        origin: node_target,
        kind,
        saved: Saved::None,
    }))
}

/// True for interactive content other than links (HTML "interactive
/// content"): a click on it inside a label or a summary does not
/// activate them.
fn is_interactive(dom: &Document, node: NodeId) -> bool {
    let Some(e) = dom.element(node).filter(|e| e.is_html()) else {
        return false;
    };
    match &**e.local_name() {
        "button" | "select" | "textarea" | "details" | "embed" | "iframe" => true,
        "input" => !e
            .attr("type")
            .is_some_and(|t| t.eq_ignore_ascii_case("hidden")),
        "audio" | "video" => e.has_attr("controls"),
        "img" | "object" => e.has_attr("usemap"),
        _ => false,
    }
}

/// The legacy-pre-activation behavior (HTML, `input` of the type
/// `checkbox` and `radio`): the state changes before the event.
pub(crate) fn pre_activation(rt: &mut Runtime, mut a: Activation) -> VmResult<Activation> {
    match a.kind {
        Kind::Checkbox => {
            let checked = engine_mut(rt)?.control_checked(a.node).unwrap_or(false);
            let indeterminate = state(rt)?.indeterminate.contains(&a.node);
            a.saved = Saved::Checkbox {
                checked,
                indeterminate,
            };
            set_checked(rt, a.node, !checked)?;
            set_indeterminate(rt, a.node, false)?;
        }
        Kind::Radio => {
            let was_checked = engine_mut(rt)?.control_checked(a.node).unwrap_or(false);
            let previous = engine_mut(rt)?
                .checked_radio_in_group(a.node)
                .filter(|&n| n != a.node);
            a.saved = Saved::Radio {
                was_checked,
                previous,
            };
            set_checked(rt, a.node, true)?;
        }
        _ => {}
    }
    Ok(a)
}

fn set_checked(rt: &mut Runtime, node: NodeId, checked: bool) -> VmResult<()> {
    engine_mut(rt)?.set_control_checked(node, checked);
    Ok(())
}

fn set_indeterminate(rt: &mut Runtime, node: NodeId, value: bool) -> VmResult<()> {
    let changed = if value {
        state_mut(rt)?.indeterminate.insert(node)
    } else {
        state_mut(rt)?.indeterminate.remove(&node)
    };
    if changed {
        engine_mut(rt)?.set_control_indeterminate(node, value);
    }
    Ok(())
}

/// DOM "dispatch" steps 5.15 and 5.16: the activation behavior if the
/// event was not canceled, else the legacy-canceled-activation behavior.
pub(crate) fn after_dispatch(
    rt: &mut Runtime,
    a: Activation,
    canceled: bool,
    origin: (bool, i32),
    offset: (i32, i32),
    modifiers: u16,
) -> VmResult<()> {
    if canceled {
        return canceled_activation(rt, a);
    }
    // A listener may have changed the element since.
    if kind_of(dom(rt)?, a.node) != Some(a.kind) {
        return Ok(());
    }
    match a.kind {
        Kind::Checkbox => fire_input_and_change(rt, a.node),
        Kind::Radio => match a.saved {
            Saved::Radio { was_checked, .. } if was_checked => Ok(()),
            _ => fire_input_and_change(rt, a.node),
        },
        Kind::Submit => {
            // The coordinate is the click's point on the button itself.
            let coordinate = if a.origin == a.node { offset } else { (0, 0) };
            let click = ClickInfo {
                coordinate,
                elsewhere: opens_elsewhere(modifiers),
            };
            activate_submit(rt, a.node, click)
        }
        Kind::Reset => activate_reset(rt, a.node),
        Kind::Link => {
            if let Some(source) = javascript_link(rt, a.node)? {
                // A link for a new browsing context or a modified click
                // (Ctrl, Shift, Meta) runs the script in that context, not
                // in this document (measured).
                if opens_here(rt, a.node)? && !opens_elsewhere(modifiers) {
                    queue_javascript_url(rt, source)?;
                }
                return Ok(());
            }
            engine_mut(rt)?.request_hyperlink(a.node);
            Ok(())
        }
        Kind::Label => activate_label(rt, a.node, a.origin, origin),
        Kind::Summary => toggle_details(rt, a.node),
    }
}

/// The activation behavior of a `summary`: toggles the `open` attribute
/// of its `details` element.
fn toggle_details(rt: &mut Runtime, summary: NodeId) -> VmResult<()> {
    let document = dom(rt)?;
    let Some(details) = document.details_of_summary(summary) else {
        return Ok(());
    };
    let open = document
        .element(details)
        .is_some_and(|e| e.has_attr("open"));
    let name = QualName::new(None, ns!(), swb_dom::LocalName::from("open"));
    if open {
        crate::dom::remove_attribute(rt, details, &name)?;
        return Ok(());
    }
    match crate::dom::set_attribute(rt, details, &name, "") {
        Err(crate::bindings::Exception::Js(error)) => Err(error),
        _ => Ok(()),
    }
}

// ----- javascript: URLs -----

/// The script of a `javascript:` URL (HTML "navigate to a javascript:
/// URL"): the part after the scheme, percent-decoded as UTF-8. `None` if
/// `url` has another scheme.
pub(crate) fn javascript_source(url: &str) -> Option<String> {
    let rest = url
        .trim()
        .get(..11)
        .filter(|s| s.eq_ignore_ascii_case("javascript:"))?;
    let body = &url.trim()[rest.len()..];
    // The URL parser drops a fragment from the opaque path; the
    // specification runs the URL without it.
    let body = body.split('#').next().unwrap_or(body);
    let bytes = body.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        let hex = |k: usize| bytes.get(i + k).and_then(|c| (*c as char).to_digit(16));
        if b == b'%'
            && let (Some(hi), Some(lo)) = (hex(1), hex(2))
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    Some(String::from_utf8_lossy(&out).into_owned())
}

/// True if a click with the modifier keys `mods` opens the link in a new
/// tab (Ctrl) or window (Shift) or downloads it (Alt). Meta does nothing
/// special on Linux (measured, `javascript-link-modifiers`).
fn opens_elsewhere(mods: u16) -> bool {
    use crate::events::modifiers::{ALT, CONTROL, SHIFT};
    mods & (CONTROL | SHIFT | ALT) != 0
}

/// True if the link `node` navigates the current browsing context: its
/// `target` (else the `target` of the first `base` element) is empty or
/// `_self`, `_parent` or `_top` (swb has no frames; a named context counts
/// as a new one).
fn opens_here(rt: &Runtime, node: NodeId) -> VmResult<bool> {
    let own = dom(rt)?
        .element(node)
        .and_then(|e| e.attr("target"))
        .map(str::to_owned);
    names_current_context(rt, own)
}

/// [`opens_here`] for the submission of `form` by `submitter`: the
/// `formtarget` of the submitter (if not empty), else the `target` of the
/// form.
fn form_opens_here(rt: &Runtime, form: NodeId, submitter: Option<NodeId>) -> VmResult<bool> {
    let document = dom(rt)?;
    let attr = |node: NodeId, name: &str| {
        document
            .element(node)
            .and_then(|e| e.attr(name))
            .map(str::to_owned)
    };
    let own = submitter
        .and_then(|s| attr(s, "formtarget"))
        .filter(|t| !t.is_empty())
        .or_else(|| attr(form, "target"));
    names_current_context(rt, own)
}

/// True if the browsing context name `own` (else the `target` of the first
/// `base` element that has one) is the current one.
fn names_current_context(rt: &Runtime, own: Option<String>) -> VmResult<bool> {
    let document = dom(rt)?;
    let target = own.or_else(|| {
        document
            .descendants(NodeId::DOCUMENT)
            .filter(|&n| document.is_html_element(n, &local_name!("base")))
            .find_map(|n| document.element(n)?.attr("target").map(str::to_owned))
    });
    Ok(target.is_none_or(|t| {
        let t = t.trim();
        t.is_empty()
            || ["_self", "_parent", "_top"]
                .iter()
                .any(|r| t.eq_ignore_ascii_case(r))
    }))
}

/// The script of the `javascript:` URL that the link `node` has, if any.
fn javascript_link(rt: &mut Runtime, node: NodeId) -> VmResult<Option<String>> {
    let href = {
        let Some(e) = dom(rt)?.element(node) else {
            return Ok(None);
        };
        let href = if e.name.ns == ns!(svg) {
            e.svg_href()
        } else {
            e.attr("href")
        };
        match href {
            Some(href) => href.trim().to_owned(),
            None => return Ok(None),
        }
    };
    Ok(engine_mut(rt)?
        .parse_url(&href)
        .and_then(|url| javascript_source(&url)))
}

/// The script of the `javascript:` URL that submitting `form` with
/// `submitter` navigates to: the `formaction` of the submitter, else the
/// `action` of the form.
fn javascript_form_action(
    rt: &mut Runtime,
    form: NodeId,
    submitter: Option<NodeId>,
) -> VmResult<Option<String>> {
    let document = dom(rt)?;
    let action = submitter
        .and_then(|s| document.element(s))
        .and_then(|e| e.attr("formaction"))
        .filter(|a| !a.is_empty())
        .or_else(|| document.element(form).and_then(|e| e.attr("action")))
        .map(|a| a.trim().to_owned());
    let Some(action) = action else {
        return Ok(None);
    };
    Ok(engine_mut(rt)?
        .parse_url(&action)
        .and_then(|url| javascript_source(&url)))
}

/// Queues the script of a `javascript:` URL as a task (HTML "navigate to
/// a javascript: URL", step "queue a global object task"): it does not
/// run inside the dispatch of the click or the submission.
/// <https://html.spec.whatwg.org/multipage/browsing-the-web.html#navigate-to-a-javascript:-url>
pub(crate) fn queue_javascript_url(rt: &mut Runtime, source: String) -> VmResult<()> {
    crate::event_loop::queue_task(rt, Task::JavascriptUrl(source))
}

/// Runs the script of a `javascript:` URL in the document as a classic
/// script; errors are reported as for a script. The value of the script
/// is dropped: in swb a string does not replace the document (a departure,
/// ADR 0028 "Feature 6d2 notes").
pub(crate) fn run_javascript_url(rt: &mut Runtime, source: &str) -> VmResult<()> {
    let name = crate::scripts::document_url(rt);
    match rt.eval_named(source, &name) {
        Ok(_) => Ok(()),
        Err(ScriptError::Terminated(termination)) => Err(VmError::Terminated(termination)),
        Err(ScriptError::Internal(error)) => Err(VmError::Internal(error)),
        Err(error) => {
            crate::errors::report(rt, &crate::errors::Site::script(&name, source), &error)
        }
    }
}

/// The legacy-canceled-activation behavior: the old state returns.
fn canceled_activation(rt: &mut Runtime, a: Activation) -> VmResult<()> {
    match a.saved {
        Saved::Checkbox {
            checked,
            indeterminate,
        } => {
            set_checked(rt, a.node, checked)?;
            set_indeterminate(rt, a.node, indeterminate)
        }
        Saved::Radio { previous, .. } => match previous {
            Some(previous) => set_checked(rt, previous, true),
            None => set_checked(rt, a.node, false),
        },
        Saved::None => Ok(()),
    }
}

/// `input` and `change` after the user changed the selected option of a
/// select with the keyboard.
pub(crate) fn select_changed(rt: &mut Runtime, node: NodeId) -> VmResult<()> {
    fire_input_and_change(rt, node)
}

/// `input` (bubbles, composed) and `change` (bubbles) at a connected
/// control.
fn fire_input_and_change(rt: &mut Runtime, node: NodeId) -> VmResult<()> {
    if !dom(rt)?.is_connected(node) {
        return Ok(());
    }
    for (kind, composed) in [("input", true), ("change", false)] {
        let mut event = TrustedEvent::new(kind);
        event.bubbles = true;
        event.composed = composed;
        fire_trusted(rt, DispatchTarget::Node(node), &event)?;
    }
    Ok(())
}

fn is_disabled(rt: &Runtime, node: NodeId) -> VmResult<bool> {
    Ok(swb_style::is_actually_disabled(dom(rt)?, node))
}

/// The activation behavior of a submit button: submits its form.
fn activate_submit(rt: &mut Runtime, button: NodeId, click: ClickInfo) -> VmResult<()> {
    let Some(form) = form_owner(dom(rt)?, button) else {
        return Ok(());
    };
    if is_disabled(rt, button)? {
        return Ok(());
    }
    submit(rt, form, Some(button), false, click)
}

/// The activation behavior of a reset button: resets its form.
fn activate_reset(rt: &mut Runtime, button: NodeId) -> VmResult<()> {
    let Some(form) = form_owner(dom(rt)?, button) else {
        return Ok(());
    };
    if is_disabled(rt, button)? {
        return Ok(());
    }
    reset(rt, form)
}

/// The activation behavior of a label (HTML, `label`): it focuses the
/// labeled control and clicks it (Chromium's simulated click). It does
/// nothing if the event came from the control or from inside it, and a
/// label that is already clicking its control does not start again, so
/// that no chain of labels, `for` attributes and nesting can recurse.
fn activate_label(
    rt: &mut Runtime,
    label: NodeId,
    origin: NodeId,
    click: (bool, i32),
) -> VmResult<()> {
    let document = dom(rt)?;
    let Some(control) = labeled_control(document, label) else {
        return Ok(());
    };
    if origin == control || document.ancestors(origin).any(|a| a == control) {
        return Ok(());
    }
    if is_disabled(rt, control)? || !state_mut(rt)?.busy.label.insert(label) {
        return Ok(());
    }
    let result = super::focus::focus_node(rt, control, true)
        .and_then(|()| dispatch_click_as(rt, control, click));
    state_mut(rt)?.busy.label.remove(&label);
    result
}

/// Dispatches a synthetic `click` at `node` (not trusted), for `click()`.
fn dispatch_click(rt: &mut Runtime, node: NodeId) -> VmResult<()> {
    dispatch_click_as(rt, node, (false, 0))
}

/// Dispatches the synthetic `click` of a key at `node` (Space or Enter on
/// a button, a checkbox, a link; Enter in a field for the default button):
/// trusted, as Chromium makes it (measured).
pub(crate) fn simulated_click(rt: &mut Runtime, node: NodeId) -> VmResult<()> {
    if !dom(rt)?.is_connected(node) {
        return Ok(());
    }
    dispatch_click_as(rt, node, (true, 0))
}

/// Dispatches a synthetic `click` at `node` that is `trusted` or not and
/// has the `detail` of the event that caused it (a label passes the
/// click of the user on: measured).
fn dispatch_click_as(
    rt: &mut Runtime,
    node: NodeId,
    (trusted, detail): (bool, i32),
) -> VmResult<()> {
    let scope = rt.heap_mut().open_scope();
    let result = (|| {
        let target = WebState::wrap_node(rt, 0, node)?;
        let event = new_click_event(rt, trusted, detail)?;
        dispatch(rt, 0, target, event, None).map(|_| ())
    })();
    if let Err(error) = rt.heap_mut().close_scope(scope) {
        log::warn!("the handle scope of a click was not the last one: {error:?}");
    }
    result
}

/// `HTMLElement.click()` (HTML §6.2.1, "dom-click"): a disabled form control
/// does nothing; a click that is in progress on the element is not
/// repeated; the event is a synthetic `click` that is not trusted.
pub(crate) fn click(cx: &mut Cx<'_>, node: NodeId) -> b::Result<()> {
    if is_disabled(cx.rt, node)? {
        return Ok(());
    }
    if !state_mut(cx.rt)?.busy.click.insert(node) {
        return Ok(());
    }
    let result =
        handlers::with_context(cx.rt, "Failed to execute 'click' on 'HTMLElement'", |rt| {
            dispatch_click(rt, node)
        });
    state_mut(cx.rt)?.busy.click.remove(&node);
    Ok(result?)
}

// ----- Forms -----

/// True if the form `form` has the `novalidate` attribute, or the
/// submitter has `formnovalidate`.
fn skips_validation(dom: &Document, form: NodeId, submitter: Option<NodeId>) -> bool {
    dom.element(form).is_some_and(|e| e.has_attr("novalidate"))
        || submitter
            .and_then(|n| dom.element(n))
            .is_some_and(|e| e.has_attr("formnovalidate"))
}

/// HTML "submit": validates (unless `from_method`: `form.submit()`),
/// fires `submit` (also unless `from_method`) and, if nothing canceled it,
/// asks the engine to navigate.
/// <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#form-submission-algorithm>
pub(crate) fn submit(
    rt: &mut Runtime,
    form: NodeId,
    submitter: Option<NodeId>,
    from_method: bool,
    click: ClickInfo,
) -> VmResult<()> {
    let document = dom(rt)?;
    if !document.is_connected(form) || document.node_document(form) != NodeId::DOCUMENT {
        return Ok(());
    }
    if !state_mut(rt)?.busy.submit.insert(form) {
        return Ok(());
    }
    let result = submit_steps(rt, form, submitter, from_method, click);
    state_mut(rt)?.busy.submit.remove(&form);
    result
}

fn submit_steps(
    rt: &mut Runtime,
    form: NodeId,
    submitter: Option<NodeId>,
    from_method: bool,
    click: ClickInfo,
) -> VmResult<()> {
    if !from_method {
        if !skips_validation(dom(rt)?, form, submitter)
            && !super::validation::report_form(rt, form)?
        {
            return Ok(());
        }
        let mut event = TrustedEvent::new("submit");
        event.bubbles = true;
        event.cancelable = true;
        event.interface = EventInterface::Submit { submitter };
        let proceed = fire_trusted(rt, DispatchTarget::Node(form), &event)?;
        // A listener may have taken the form out of the document.
        if !proceed || !dom(rt)?.is_connected(form) {
            return Ok(());
        }
    }
    if let Some(source) = javascript_form_action(rt, form, submitter)? {
        // A submission for a new context or with Ctrl, Shift or Alt held
        // runs the script there, not in this document (measured).
        if !click.elsewhere && form_opens_here(rt, form, submitter)? {
            queue_javascript_url(rt, source)?;
        }
        return Ok(());
    }
    engine_mut(rt)?.request_form_submission(form, submitter, click.coordinate);
    Ok(())
}

/// The implicit submission of a form without a submit button (Enter in a
/// field): HTML "submit" with the `submit` event.
pub(crate) fn implicit_submit(rt: &mut Runtime, form: NodeId) -> VmResult<()> {
    submit(rt, form, None, false, ClickInfo::default())
}

/// HTML "reset": `reset` at the form (bubbles, can be canceled), then the
/// controls return to their defaults. A reset that runs inside the `reset`
/// event of the same form does nothing ("locked for reset").
pub(crate) fn reset(rt: &mut Runtime, form: NodeId) -> VmResult<()> {
    if !state_mut(rt)?.busy.reset.insert(form) {
        return Ok(());
    }
    let mut event = TrustedEvent::new("reset");
    event.bubbles = true;
    event.cancelable = true;
    let result = fire_trusted(rt, DispatchTarget::Node(form), &event);
    state_mut(rt)?.busy.reset.remove(&form);
    if result? {
        engine_mut(rt)?.reset_form(form);
        reset_outputs(rt, form)?;
    }
    Ok(())
}

/// The reset algorithm of `output` elements: the text content becomes the
/// default value override, and the value mode flag is "default" again.
fn reset_outputs(rt: &mut Runtime, form: NodeId) -> VmResult<()> {
    if state_mut(rt)?.output_defaults.is_empty() {
        return Ok(());
    }
    let keys: Vec<NodeId> = state_mut(rt)?.output_defaults.keys().copied().collect();
    let document = dom(rt)?;
    let owned: Vec<NodeId> = keys
        .into_iter()
        .filter(|&n| form_owner(document, n) == Some(form))
        .collect();
    for output in owned {
        if let Some(default) = state_mut(rt)?.output_defaults.remove(&output) {
            // Only a failure of the DOM could fail; the output keeps its text.
            let _ = crate::dom::string_replace_all_in(rt, output, &default);
        }
    }
    Ok(())
}

/// `form.requestSubmit(submitter)` (HTML §4.10.21.3): the submitter must
/// be a submit button owned by the form.
pub(crate) fn request_submit(
    cx: &mut Cx<'_>,
    form: NodeId,
    submitter: Option<NodeId>,
) -> b::Result<()> {
    if let Some(node) = submitter {
        let document = dom(cx.rt)?;
        if kind_of(document, node) != Some(Kind::Submit) {
            return Err(b::Exception::type_error(
                "The specified element is not a submit button.",
            ));
        }
        if form_owner(document, node) != Some(form) {
            return Err(b::Exception::dom(
                "NotFoundError",
                "The specified element is not owned by this form element.",
            ));
        }
    }
    Ok(submit(cx.rt, form, submitter, false, ClickInfo::default())?)
}

/// `form.submit()`: no validation and no `submit` event.
pub(crate) fn submit_method(cx: &mut Cx<'_>, form: NodeId) -> b::Result<()> {
    Ok(submit(cx.rt, form, None, true, ClickInfo::default())?)
}

/// `form.reset()`.
pub(crate) fn reset_method(cx: &mut Cx<'_>, form: NodeId) -> b::Result<()> {
    Ok(reset(cx.rt, form)?)
}

/// Used by `focus.rs`: the busy set for "locked for focus".
pub(crate) fn lock_focus(rt: &mut Runtime, node: NodeId) -> VmResult<bool> {
    Ok(state_mut(rt)?.busy.focus.insert(node))
}

/// Used by `focus.rs`: leaves "locked for focus".
pub(crate) fn unlock_focus(rt: &mut Runtime, node: NodeId) -> VmResult<()> {
    state_mut(rt)?.busy.focus.remove(&node);
    Ok(())
}
