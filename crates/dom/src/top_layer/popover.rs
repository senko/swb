//! The popover algorithms (HTML §6.12 "The popover attribute",
//! <https://html.spec.whatwg.org/multipage/popover.html>; ADR 0037 sections
//! 1 and 2): "check popover validity", "show popover", "hide popover" with
//! the stack walks, "toggle popover", the invokers' activation behavior and
//! the attribute change steps. The state they work on and the host trait are
//! in `top_layer.rs`; the algorithms run on the host's events.

use super::{
    HideOptions, List, PopoverError, PopoverErrorKind, PopoverState, TargetAction, ToggleState,
    TopLayerHost, WatcherKind, flags, local_name,
};
use crate::tree::{Document, NodeId};

/// HTML "check popover validity": `Ok(false)` if the element is not in the
/// expected visibility state.
pub(super) fn check(
    doc: &Document,
    element: NodeId,
    expect_showing: bool,
    expected_document: Option<NodeId>,
    ignore_dom: bool,
) -> Result<bool, PopoverErrorKind> {
    let Some(e) = doc.get(element).and_then(crate::Node::as_element) else {
        return Err(PopoverErrorKind::NotPopover);
    };
    if !ignore_dom && doc.popover_state(element).is_none() {
        return Err(PopoverErrorKind::NotPopover);
    }
    // Chromium reports a popover in template contents and in a document
    // without a browsing context as not fully active, also for a hide of
    // a hidden popover and for a recheck after an event (measured,
    // `popover-validity-6a`): that check comes before the state check.
    if doc.node_document(element) != NodeId::DOCUMENT {
        return Err(PopoverErrorKind::NotFullyActive);
    }
    if e.is_popover_showing() != expect_showing {
        return Ok(false);
    }
    if !expect_showing && !doc.is_connected(element) {
        return Err(PopoverErrorKind::Disconnected);
    }
    if expected_document.is_some_and(|d| d != doc.node_document(element)) {
        return Err(PopoverErrorKind::OtherDocument);
    }
    if e.is_modal() && e.is_html_named(&local_name!("dialog")) {
        return Err(PopoverErrorKind::ModalDialog);
    }
    Ok(true)
}

/// The end of an algorithm with the error `kind`: returned if `throw` (a
/// stop always), else the algorithm ends silently.
pub(super) fn fail(
    throw: bool,
    kind: PopoverErrorKind,
    after_event: bool,
) -> Result<(), PopoverError> {
    if throw || kind == PopoverErrorKind::Stopped {
        Err(PopoverError { kind, after_event })
    } else {
        Ok(())
    }
}

/// [`check`] after an event: `Ok(true)` to go on, `Ok(false)` to end
/// silently.
pub(super) fn recheck<H: TopLayerHost + ?Sized>(
    host: &mut H,
    element: NodeId,
    expect_showing: bool,
    document: NodeId,
    (throw, ignore_dom): (bool, bool),
) -> Result<bool, PopoverError> {
    match check(
        host.doc(),
        element,
        expect_showing,
        Some(document),
        ignore_dom,
    ) {
        Ok(go) => Ok(go),
        Err(kind) => fail(throw, kind, true).map(|()| false),
    }
}

/// Sets the "showing or hiding" flag of `element` and returns its old value.
pub(super) fn set_busy(doc: &mut Document, element: NodeId, busy: bool) -> bool {
    let state = doc.top_layer_state();
    let data = state.popovers.entry(element).or_default();
    let old = data.busy;
    data.busy = busy;
    if !busy {
        state.tidy(element);
    }
    old
}

/// HTML "show popover": `showPopover()`, `togglePopover()` and invokers.
/// `throw` returns the errors; `source` is the invoker or the `source`
/// option.
/// <https://html.spec.whatwg.org/multipage/popover.html#show-popover>
///
/// # Errors
///
/// The failed validity check (with `throw`), or a stop of the host.
pub fn show_popover<H: TopLayerHost + ?Sized>(
    host: &mut H,
    element: NodeId,
    throw: bool,
    source: Option<NodeId>,
) -> Result<(), PopoverError> {
    match check(host.doc(), element, false, None, false) {
        Ok(true) => {}
        Ok(false) => return Ok(()),
        Err(kind) => return fail(throw, kind, false),
    }
    // An open non-modal dialog shows as a popover too (Chromium, measured:
    // `popover-validity-6a` #1); only a modal one is refused, by the check.
    let document = host.doc().node_document(element);
    let nested = set_busy(host.doc(), element, true);
    let result = show_steps(host, element, (throw, nested), source, document);
    if !nested {
        set_busy(host.doc(), element, false);
    }
    result
}

/// The steps of [`show_popover`] after its first check; `nested` is true
/// for a show inside the `beforetoggle` event of another show of the same
/// element.
pub(super) fn show_steps<H: TopLayerHost + ?Sized>(
    host: &mut H,
    element: NodeId,
    (throw, nested): (bool, bool),
    source: Option<NodeId>,
    document: NodeId,
) -> Result<(), PopoverError> {
    match host.fire_beforetoggle(
        element,
        ToggleState::Closed,
        ToggleState::Open,
        true,
        source,
    ) {
        None => return Err(PopoverError::stopped()),
        Some(false) => return Ok(()),
        Some(true) => {}
    }
    if !recheck(host, element, false, document, (throw, false))? {
        return Ok(());
    }
    let Some(original) = host.doc().popover_state(element) else {
        return Ok(());
    };
    let mut list = None;
    if original != PopoverState::Manual {
        let ancestor = topmost_popover_ancestor(host.doc(), element, source);
        let ancestor_list = ancestor.and_then(|a| host.doc().top_layer()?.list_of(a));
        // A show inside the event of the same element's show hides the others
        // without events (HTML of 2023, which Chromium follows).
        let fire = !nested;
        let target = match (original, ancestor_list) {
            (_, Some(List::Hint)) => {
                hide_stack_until(host, ancestor, List::Hint, fire)?;
                List::Hint
            }
            (PopoverState::Hint, None) => {
                hide_stack_until(host, None, List::Hint, fire)?;
                List::Hint
            }
            _ => {
                hide_stack_until(host, None, List::Hint, fire)?;
                hide_stack_until(host, ancestor, List::Auto, fire)?;
                List::Auto
            }
        };
        if host.doc().popover_state(element) != Some(original) {
            fail(throw, PopoverErrorKind::TypeChanged, true)?;
            return Ok(());
        }
        if !recheck(host, element, false, document, (throw, false))? {
            return Ok(());
        }
        list = Some(target);
    }
    let doc = host.doc();
    let state = doc.top_layer_state();
    // HTML "show popover": the focus goes back to the element that had it
    // when the popover opened a stack (an auto or hint popover while no
    // popover shows; measured, `popover-focus-restore`).
    let restore_focus = list.is_some() && state.auto.is_empty() && state.hint.is_empty();
    if let Some(list) = list {
        state.push(list, element);
        state.watchers.create(element, WatcherKind::Popover);
    }
    state.add_to_layer(element);
    let data = state.popovers.entry(element).or_default();
    data.invoker = source;
    data.watcher = list.is_some();
    let serial = state.queue_toggle(element, ToggleState::Closed, ToggleState::Open, source);
    doc.set_flags(element, flags::POPOVER_SHOWING | flags::TOP_LAYER, true);
    doc.record_top_layer(element);
    let previous = if restore_focus {
        host.focused_element()
    } else {
        None
    };
    focusing_steps(host, element);
    if let Some(previous) = previous {
        // A listener of a focus event may have hidden the popover already.
        let doc = host.doc();
        if doc.is_popover_showing(element)
            && let Some(data) = doc.top_layer_state().popovers.get_mut(&element)
        {
            data.previously_focused = Some(previous);
        }
    }
    host.queue_toggle_task(element, serial);
    Ok(())
}

/// The most nodes that the search for an `autofocus` descendant visits.
pub(super) const AUTOFOCUS_SCAN_LIMIT: usize = 100_000;

/// HTML "popover focusing steps": the popover itself if it has `autofocus`,
/// else its first descendant with `autofocus` (in flat tree order) that the
/// host can focus; nothing moves otherwise (measured, `popover-focus`). The
/// search runs only for arenas that ever had an `autofocus` attribute, and
/// visits at most [`AUTOFOCUS_SCAN_LIMIT`] nodes (a departure: a popover with
/// a huge subtree whose `autofocus` element is past the limit does not give
/// it the focus).
/// <https://html.spec.whatwg.org/multipage/popover.html#popover-focusing-steps>
pub(super) fn focusing_steps<H: TopLayerHost + ?Sized>(host: &mut H, popover: NodeId) {
    let doc = host.doc();
    if !doc.ever_had_autofocus() {
        return;
    }
    if doc
        .element(popover)
        .is_some_and(|e| e.has_attr("autofocus"))
    {
        host.focus(popover);
        return;
    }
    let mut stack: Vec<NodeId> = doc.flat_children(popover).collect();
    stack.reverse();
    let mut visited = 0;
    let mut candidates = Vec::new();
    while let Some(node) = stack.pop() {
        visited += 1;
        if visited > AUTOFOCUS_SCAN_LIMIT {
            break;
        }
        let Some(element) = doc.element(node) else {
            continue;
        };
        if element.has_attr("autofocus") {
            candidates.push(node);
        }
        let mark = stack.len();
        stack.extend(doc.flat_children(node));
        stack[mark..].reverse();
    }
    for candidate in candidates {
        if host.focus(candidate) {
            return;
        }
    }
}

/// HTML "hide popover algorithm": `hidePopover()`, invokers, the attribute
/// change steps (`ignore_dom`) and the removing steps (no events).
/// <https://html.spec.whatwg.org/multipage/popover.html#hide-popover-algorithm>
///
/// # Errors
///
/// The failed validity check (with `options.throw`), or a stop of the host.
pub fn hide_popover<H: TopLayerHost + ?Sized>(
    host: &mut H,
    element: NodeId,
    mut options: HideOptions,
) -> Result<(), PopoverError> {
    match check(host.doc(), element, true, None, options.ignore_dom) {
        Ok(true) => {}
        Ok(false) => return Ok(()),
        Err(kind) => return fail(options.throw, kind, true),
    }
    let document = host.doc().node_document(element);
    let nested = set_busy(host.doc(), element, true);
    if nested {
        options.fire_events = false;
    }
    let result = hide_steps(host, element, options, document);
    if !nested {
        set_busy(host.doc(), element, false);
    }
    result
}

pub(super) fn hide_steps<H: TopLayerHost + ?Sized>(
    host: &mut H,
    element: NodeId,
    options: HideOptions,
    document: NodeId,
) -> Result<(), PopoverError> {
    let flags_of = (options.throw, options.ignore_dom);
    let list = host.doc().top_layer().and_then(|s| s.list_of(element));
    if let Some(list) = list {
        hide_above(host, element, list, options.fire_events)?;
        if !recheck(host, element, true, document, flags_of)? {
            return Ok(());
        }
    }
    if options.fire_events {
        let fired = host.fire_beforetoggle(
            element,
            ToggleState::Open,
            ToggleState::Closed,
            false,
            options.source,
        );
        if fired.is_none() {
            return Err(PopoverError::stopped());
        }
        // A listener may have shown popovers above this one.
        if let Some(list) = host.doc().top_layer().and_then(|s| s.list_of(element)) {
            hide_above(host, element, list, false)?;
        }
        if !recheck(host, element, true, document, flags_of)? {
            return Ok(());
        }
    }
    let previous = take_previously_focused(host.doc(), element);
    remove_popover(host.doc(), element);
    if let Some(previous) = previous
        && options.focus_previous
    {
        restore_focus(host, element, previous);
    }
    if options.fire_events {
        let doc = host.doc();
        let serial = doc.top_layer_state().queue_toggle(
            element,
            ToggleState::Open,
            ToggleState::Closed,
            options.source,
        );
        host.queue_toggle_task(element, serial);
    }
    Ok(())
}

/// Takes the previously focused element out of the data of `element`.
pub(super) fn take_previously_focused(doc: &mut Document, element: NodeId) -> Option<NodeId> {
    doc.top_layer
        .as_deref_mut()?
        .popovers
        .get_mut(&element)?
        .previously_focused
        .take()
}

/// HTML "hide popover algorithm", last steps: if the focus is inside the
/// popover (a shadow-including inclusive descendant), the focusing steps run
/// for the element that had the focus before. The host leaves the focus where
/// it is if that element is gone or cannot be focused (measured,
/// `popover-focus-restore`).
pub(super) fn restore_focus<H: TopLayerHost + ?Sized>(
    host: &mut H,
    popover: NodeId,
    previous: NodeId,
) {
    let Some(focused) = host.focused_element() else {
        return;
    };
    if host
        .doc()
        .is_shadow_including_inclusive_ancestor(popover, focused)
    {
        host.focus(previous);
    }
}

/// Hides the popovers above `element` in `list`; for the auto list also all
/// popovers of the hint list.
pub(super) fn hide_above<H: TopLayerHost + ?Sized>(
    host: &mut H,
    element: NodeId,
    list: List,
    fire: bool,
) -> Result<(), PopoverError> {
    if list == List::Auto {
        hide_stack_until(host, None, List::Hint, fire)?;
    }
    hide_stack_until(host, Some(element), list, fire)
}

/// The end of a hide: the popover leaves its list and the top layer.
pub(super) fn remove_popover(doc: &mut Document, element: NodeId) {
    let state = doc.top_layer_state();
    state.remove_from_list(element);
    state.remove_from_layer(element);
    if let Some(data) = state.popovers.get_mut(&element) {
        data.invoker = None;
        if std::mem::take(&mut data.watcher) {
            state.watchers.destroy(element, WatcherKind::Popover);
        }
    }
    state.tidy(element);
    doc.set_flags(element, flags::POPOVER_SHOWING | flags::TOP_LAYER, false);
    doc.record_top_layer(element);
}

/// HTML "hide popover stack until": hides the popovers of `list` above
/// `endpoint` (all of them if it is not in the list), the top first. The
/// popovers that the events of the first ones show are hidden without
/// events.
pub(super) fn hide_stack_until<H: TopLayerHost + ?Sized>(
    host: &mut H,
    endpoint: Option<NodeId>,
    list: List,
    fire_events: bool,
) -> Result<(), PopoverError> {
    let keep = |host: &mut H| {
        let state = host.doc().top_layer()?;
        let items = state.list(list);
        let keep = endpoint
            .and_then(|e| state.index_in(e, list))
            .map_or(0, |i| i + 1);
        (items.len() > keep).then(|| (items.len() - keep, items.last().copied()))
    };
    let Some((mut with_events, _)) = keep(host) else {
        return Ok(());
    };
    while let Some((_, Some(top))) = keep(host) {
        let options = HideOptions {
            fire_events: fire_events && with_events > 0,
            // The list holds showing popovers only: one whose attribute
            // changed is hidden too.
            ignore_dom: true,
            ..HideOptions::default()
        };
        with_events = with_events.saturating_sub(1);
        hide_popover(host, top, options)?;
        // A hide that did not finish (an element that moved to another
        // document) must not stop the walk.
        let still = host
            .doc()
            .top_layer()
            .is_some_and(|s| s.list(list).last() == Some(&top));
        if still {
            remove_popover(host.doc(), top);
        }
    }
    Ok(())
}

/// HTML "hide all popovers until" (light dismiss): hides the popovers above
/// `endpoint`, or all of them if it is none or not showing. With an endpoint
/// in the hint list only the hints above it hide and the auto list stays; else
/// the hint list hides first and then the auto list down to the endpoint
/// (measured, `light-dismiss-hint-*`). The hides give the focus back to no
/// one (light dismiss moves no focus).
/// <https://html.spec.whatwg.org/multipage/popover.html#hide-all-popovers-until>
///
/// # Errors
///
/// A stop of the host.
pub fn hide_all_popovers_until<H: TopLayerHost + ?Sized>(
    host: &mut H,
    endpoint: Option<NodeId>,
    fire_events: bool,
) -> Result<(), PopoverError> {
    let list = endpoint.and_then(|e| host.doc().top_layer()?.list_of(e));
    if list == Some(List::Hint) {
        hide_stack_until(host, endpoint, List::Hint, fire_events)
    } else {
        hide_stack_until(host, None, List::Hint, fire_events)?;
        hide_stack_until(host, endpoint, List::Auto, fire_events)
    }
}

/// HTML "topmost popover ancestor" of the popover `element` that is about
/// to show: the showing auto or hint popover with the highest position of
/// which `element` is a flat tree descendant, or `source` an inclusive one.
/// The walks stop at the top of the lists: a chain of nested popovers shown
/// in order costs one step for each show.
///
/// The algorithm takes the highest position over all flat ancestors. The
/// nearest open ancestor would give the same popover in every state that
/// the algorithms reach: a farther ancestor that showed after a nearer one
/// would have hidden the nearer one (a show hides the popovers above its own
/// topmost ancestor), so no nearer ancestor has a lower position than a
/// farther one (ADR 0037, "6a implementation notes", review fixes).
/// <https://html.spec.whatwg.org/multipage/popover.html#topmost-popover-ancestor>
pub fn topmost_popover_ancestor(
    doc: &Document,
    element: NodeId,
    source: Option<NodeId>,
) -> Option<NodeId> {
    let state = doc.top_layer.as_deref()?;
    let top = (state.auto.len() + state.hint.len()).checked_sub(1)?;
    let mut best: Option<(usize, NodeId)> = None;
    let mut walk = |start: Option<NodeId>| {
        let mut current = start;
        while let Some(node) = current {
            if doc
                .element(node)
                .is_some_and(crate::ElementData::is_popover_showing)
                && let Some(position) = state.position(node)
            {
                if best.is_none_or(|(b, _)| position > b) {
                    best = Some((position, node));
                }
                if position == top {
                    return true;
                }
            }
            current = doc.flat_parent(node);
        }
        false
    };
    if !walk(doc.flat_parent(element)) {
        walk(source);
    }
    best.map(|(_, node)| node)
}

/// `togglePopover(force)`: hides a showing popover, else shows it; with
/// `force`, only towards that state. Returns whether it shows afterwards.
///
/// # Errors
///
/// The errors of [`show_popover`] and [`hide_popover`], or of the validity
/// check when nothing changes.
pub fn toggle_popover<H: TopLayerHost + ?Sized>(
    host: &mut H,
    element: NodeId,
    force: Option<bool>,
    source: Option<NodeId>,
) -> Result<bool, PopoverError> {
    let showing = host.doc().is_popover_showing(element);
    if showing && force != Some(true) {
        let options = HideOptions {
            focus_previous: true,
            fire_events: true,
            throw: true,
            ignore_dom: false,
            source: None,
        };
        hide_popover(host, element, options)?;
    } else if force != Some(false) {
        show_popover(host, element, true, source)?;
    } else if let Err(kind) = check(host.doc(), element, showing, None, false) {
        return Err(PopoverError {
            kind,
            after_event: false,
        });
    }
    Ok(host.doc().is_popover_showing(element))
}

/// HTML "popover target attribute activation behavior": the invoker
/// `invoker` acts on its target `popover` with `action`, for a click whose
/// target was `event_target`. Errors end it silently.
/// <https://html.spec.whatwg.org/multipage/popover.html#popover-target-attribute-activation-behavior>
///
/// # Errors
///
/// A stop of the host.
pub fn popover_target_activation<H: TopLayerHost + ?Sized>(
    host: &mut H,
    invoker: NodeId,
    popover: NodeId,
    (action, event_target): (TargetAction, NodeId),
) -> Result<(), PopoverError> {
    let doc = host.doc();
    if popover != invoker
        && doc.is_shadow_including_inclusive_ancestor(popover, event_target)
        && doc.is_shadow_including_inclusive_ancestor(invoker, popover)
    {
        return Ok(());
    }
    let showing = doc.is_popover_showing(popover);
    match (action, showing) {
        (TargetAction::Show, true) | (TargetAction::Hide, false) => Ok(()),
        (_, true) => {
            let options = HideOptions {
                focus_previous: true,
                fire_events: true,
                throw: false,
                ignore_dom: false,
                source: Some(invoker),
            };
            hide_popover(host, popover, options)
        }
        (_, false) => {
            if check(doc, popover, false, None, false) != Ok(true) {
                return Ok(());
            }
            show_popover(host, popover, false, Some(invoker))
        }
    }
}

/// True if `node` is an element that can be a popover invoker: a `button`, or
/// an `input` of type `button`, `submit`, `reset` or `image` (Chromium,
/// measured: `invokers-types`). Whether it acts depends on more (disabled,
/// form owner: the caller's).
pub fn can_invoke(doc: &Document, node: NodeId) -> bool {
    let Some(e) = doc.element(node).filter(|e| e.is_html()) else {
        return false;
    };
    match &**e.local_name() {
        "button" => true,
        "input" => e.attr("type").is_some_and(|t| {
            ["button", "submit", "reset", "image"]
                .iter()
                .any(|k| t.eq_ignore_ascii_case(k))
        }),
        _ => false,
    }
}

/// True if the invoker `node` (see [`can_invoke`]) submits or resets its form
/// when it has a form owner: a `button` of any type but `button`, and an
/// `input` of type `submit`, `image` or `reset`. Such a button with a form
/// owner submits or resets instead of invoking (Chromium, measured).
pub fn invoker_acts_on_form(doc: &Document, node: NodeId) -> bool {
    doc.element(node).is_some_and(|e| {
        !e.attr("type")
            .is_some_and(|t| t.eq_ignore_ascii_case("button"))
    })
}

/// The element that the `popovertarget` attribute of `invoker` names: the
/// element with that id in the invoker's tree (HTML "get the
/// `popovertarget`-associated element" without an explicitly set element).
pub fn popover_target_by_id(doc: &Document, invoker: NodeId) -> Option<NodeId> {
    let id = doc.element(invoker)?.attr("popovertarget")?;
    if id.is_empty() {
        return None;
    }
    doc.get_element_by_id(doc.root(invoker), id)
}

/// Runs the attribute change steps of the showing popovers whose `popover`
/// attribute changed state ([`Document::take_popover_attribute_hides`]): a
/// hide with `beforetoggle` (not cancelable) and a `toggle` task.
///
/// # Errors
///
/// A stop of the host.
pub fn run_popover_attribute_steps<H: TopLayerHost + ?Sized>(
    host: &mut H,
) -> Result<(), PopoverError> {
    loop {
        let pending = host.doc().take_popover_attribute_hides();
        if pending.is_empty() {
            return Ok(());
        }
        for element in pending {
            let options = HideOptions {
                focus_previous: true,
                fire_events: true,
                throw: false,
                ignore_dom: true,
                source: None,
            };
            hide_popover(host, element, options)?;
        }
    }
}
