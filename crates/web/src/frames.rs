//! Frames as scripts see them (ADR 0031 section 5, M9 feature 12b): the
//! `WindowProxy` of a frame, the realm that a frame document gets at its first
//! need, the members of `Window` and of `iframe` that name frames, and the
//! steps that make and destroy frames inside a task.
//!
//! The frame tree belongs to the engine. A native reaches it through the
//! page's frame registry ([`FrameHost`]), which it borrows for one call and
//! never across a call into script; a registry that is in use is an internal
//! error, not a panic.
//!
//! A frame is known by its number. Every proxy of the runtime is in
//! `WebState::window_proxies` until its frame is destroyed. A frame whose
//! document has another origin, or that is past the budget of realms, gets a
//! proxy without a target, whose hooks write the rules of cross-origin objects
//! (`window_proxy.rs`). Each origin has a runtime of its own, so a document of
//! another origin is out of reach here: the registry answers for the running
//! runtime only, and what must happen in another runtime the page does after the
//! task.
//!
//! *Retired documents.* When a frame is destroyed, or its document is
//! replaced, a document with a realm stays in the runtime (`state.rs`,
//! `retire_document`), because script can still hold its nodes and functions of
//! its realm. As measured in Chromium (case `scripted-retired`): the nodes stay
//! readable and writable; `setTimeout`, `setInterval` and
//! `requestAnimationFrame` return a number and their callbacks do not run;
//! microtasks run; no script element runs and no frame starts; a read of
//! geometry gives zeros; `fetch` fails; `location` writes do nothing.

use std::cell::RefCell;
use std::rc::Rc;

use swb_dom::NodeId;
use swb_js::{Gc, Object, Runtime, Value, VmError, VmResult};

use crate::events::{DispatchTarget, TrustedEvent};
use crate::host::FrameHost;
use crate::state::{self, DocKey, dom, engine_mut, state, state_mut};

/// The most realms that a runtime makes over its life (ADR 0031 section 11):
/// realms are roots of the collector until realm collection exists.
pub(crate) const MAX_REALMS: usize = 256;

/// The registry of the page; `None` in a runtime without a page.
fn registry(rt: &Runtime) -> VmResult<Option<Rc<RefCell<dyn FrameHost>>>> {
    Ok(state(rt)?.frame_host.clone())
}

/// The error of a registry that is in use (a bug of the host).
fn in_use() -> VmError {
    VmError::invariant("the frame registry is in use")
}

/// Runs `read` with the registry; `None` without one.
fn read<R>(rt: &Runtime, read: impl FnOnce(&dyn FrameHost) -> R) -> VmResult<Option<R>> {
    let Some(frames) = registry(rt)? else {
        return Ok(None);
    };
    let frames = frames.try_borrow().map_err(|_| in_use())?;
    Ok(Some(read(&*frames)))
}

/// Runs `change` with the registry; `None` without one.
fn change<R>(rt: &Runtime, change: impl FnOnce(&mut dyn FrameHost) -> R) -> VmResult<Option<R>> {
    let Some(frames) = registry(rt)? else {
        return Ok(None);
    };
    let mut frames = frames.try_borrow_mut().map_err(|_| in_use())?;
    Ok(Some(change(&mut *frames)))
}

/// True if the page has no frames besides the top level: a read of one
/// number.
fn no_frames(rt: &Runtime) -> VmResult<bool> {
    Ok(state(rt)?.frame_count.get() == 0)
}

// ----- Windows -----

/// The `WindowProxy` of frame `frame` in this runtime; `None` for a frame that
/// does not exist. A frame of this agent gets its realm now.
pub(crate) fn window_of_frame(rt: &mut Runtime, frame: u32) -> VmResult<Option<Gc<Object>>> {
    if let Some(&proxy) = state(rt)?.window_proxies.get(&frame) {
        return Ok(Some(proxy));
    }
    let Some((alive, agent)) = read(rt, |f| (f.is_alive(frame), f.in_this_agent(frame)))? else {
        return Ok(None);
    };
    if !alive {
        return Ok(None);
    }
    if agent && make_realm(rt, frame)?.is_some() {
        return Ok(state(rt)?.window_proxies.get(&frame).copied());
    }
    let proxy = rt.new_window_proxy(frame, None)?;
    state_mut(rt)?.window_proxies.insert(frame, proxy);
    Ok(Some(proxy))
}

/// Makes the realm of the document of frame `frame`, whose parse is complete
/// (a document with scripts got its realm when it committed). `None` past the
/// budget of realms.
fn make_realm(rt: &mut Runtime, frame: u32) -> VmResult<Option<DocKey>> {
    let Some(key) = new_realm(rt, frame)? else {
        return Ok(None);
    };
    let loading = read(rt, |f| f.document_loading(frame))?.unwrap_or(false);
    let allowed = read(rt, |f| f.scripts_allowed(frame))?.unwrap_or(false);
    if let Some(record) = state::document_mut(state_mut(rt)?, key) {
        record.scripts_off = !allowed;
        record.scripts.adopt_complete(loading);
    }
    Ok(Some(key))
}

/// A new realm with a `Window` and the record of a document of frame `frame`,
/// and the frame's proxy with the new `Window` as its target. `None` past the
/// budget of realms (with one warning).
pub(crate) fn new_realm(rt: &mut Runtime, frame: u32) -> VmResult<Option<DocKey>> {
    let state_now = state_mut(rt)?;
    if state::realm_count(state_now) >= MAX_REALMS {
        if !std::mem::replace(&mut state_now.realm_budget_warned, true) {
            log::warn!(
                "the page made {MAX_REALMS} realms, the limit; later frame documents run without scripts"
            );
        }
        return Ok(None);
    }
    let clock = crate::clock::DocumentClock::new(state_now.doc.clock.page.clone());
    let realm = rt.create_realm()?;
    crate::runtime::init_window(rt, realm)?;
    let key = state::add_document(rt, realm, frame, clock)?;
    change(rt, |f| {
        f.set_document_key(frame, Some(crate::runtime::DocumentKey::of(key)));
    })?;
    Ok(Some(key))
}

/// The wrapper of `node` of the document `key`.
fn wrap_in_document(rt: &mut Runtime, key: DocKey, node: NodeId) -> VmResult<Gc<Object>> {
    let entered = state::switch_if_other(rt, key)?;
    let wrapper = state::wrap_node_with_proto(rt, node, None);
    state::leave(rt, entered);
    wrapper
}

/// `iframe.contentWindow`.
pub(crate) fn content_window(rt: &mut Runtime, container: NodeId) -> VmResult<Option<Gc<Object>>> {
    if no_frames(rt)? {
        return Ok(None);
    }
    let parent_doc = &state(rt)?.doc;
    // A node of a retired document has no frame: the number of the node can be that of
    // a container of the frame's document now (measured, case `msg-retired-node`).
    if parent_doc.retired {
        return Ok(None);
    }
    let parent = parent_doc.frame;
    let Some(Some(child)) = read(rt, |f| f.child_of_container(parent, container))? else {
        return Ok(None);
    };
    window_of_frame(rt, child)
}

/// `iframe.contentDocument`: `null` for a frame of another origin or past the
/// budget of realms.
pub(crate) fn content_document(
    rt: &mut Runtime,
    container: NodeId,
) -> VmResult<Option<Gc<Object>>> {
    if content_window(rt, container)?.is_none() {
        return Ok(None);
    }
    let parent = state(rt)?.doc.frame;
    let key = read(rt, |f| {
        f.child_of_container(parent, container)
            .and_then(|child| f.document_key(child))
    })?
    .flatten();
    match key {
        Some(key) => Ok(Some(wrap_in_document(rt, key.raw(), NodeId::DOCUMENT)?)),
        None => Ok(None),
    }
}

/// True if the current document's window has lost its frame.
fn window_is_closed(rt: &Runtime) -> VmResult<bool> {
    let state_now = state(rt)?;
    if state_now.doc.retired {
        return Ok(true);
    }
    let frame = state_now.doc.frame;
    Ok(read(rt, |f| !f.is_alive(frame))?.unwrap_or(false))
}

/// `window.closed`.
pub(crate) fn closed(rt: &Runtime) -> VmResult<bool> {
    window_is_closed(rt)
}

/// `window.length`: the number of child frames.
pub(crate) fn length(rt: &Runtime) -> VmResult<u32> {
    if no_frames(rt)? || window_is_closed(rt)? {
        return Ok(0);
    }
    let frame = state(rt)?.doc.frame;
    Ok(read(rt, |f| f.child_count(frame))?.unwrap_or(0))
}

/// `window.parent`: the window itself for the top level, `null` for a window
/// without a frame.
pub(crate) fn parent(rt: &mut Runtime, this: Gc<Object>) -> VmResult<Option<Gc<Object>>> {
    if window_is_closed(rt)? {
        return Ok(None);
    }
    let frame = state(rt)?.doc.frame;
    match read(rt, |f| f.parent(frame))?.flatten() {
        Some(parent) => window_of_frame(rt, parent),
        None => Ok(Some(this)),
    }
}

/// `window.top`.
pub(crate) fn top(rt: &mut Runtime, this: Gc<Object>) -> VmResult<Option<Gc<Object>>> {
    if window_is_closed(rt)? {
        return Ok(None);
    }
    if state(rt)?.doc.frame == 0 {
        return Ok(Some(this));
    }
    window_of_frame(rt, 0)
}

/// `window.frameElement`: the container, if its document is in this agent.
pub(crate) fn frame_element(rt: &mut Runtime) -> VmResult<Option<Gc<Object>>> {
    if window_is_closed(rt)? {
        return Ok(None);
    }
    let frame = state(rt)?.doc.frame;
    let Some(Some((parent, container))) =
        read(rt, |f| Some((f.parent(frame)?, f.container(frame)?)))?
    else {
        return Ok(None);
    };
    if window_of_frame(rt, parent)?.is_none() {
        return Ok(None);
    }
    match read(rt, |f| f.document_key(parent))?.flatten() {
        Some(key) => Ok(Some(wrap_in_document(rt, key.raw(), container)?)),
        None => Ok(None),
    }
}

/// `window.name`: empty for a window without a frame.
pub(crate) fn name(rt: &Runtime) -> VmResult<String> {
    if window_is_closed(rt)? {
        return Ok(String::new());
    }
    let frame = state(rt)?.doc.frame;
    match read(rt, |f| f.name(frame))? {
        Some(name) => Ok(name),
        // A runtime without a page keeps the name itself.
        None => Ok(state(rt)?.doc.own_name.clone()),
    }
}

/// `window.name = value`.
pub(crate) fn set_name(rt: &mut Runtime, value: &str) -> VmResult<()> {
    if window_is_closed(rt)? {
        return Ok(());
    }
    let frame = state(rt)?.doc.frame;
    if change(rt, |f| f.set_name(frame, value))?.is_none() {
        value.clone_into(&mut state_mut(rt)?.doc.own_name);
    }
    Ok(())
}

/// The child frame named `name` of the current document's frame, for the
/// named properties of the window.
pub(crate) fn child_named(rt: &Runtime, name: &str) -> VmResult<Option<u32>> {
    if no_frames(rt)? || window_is_closed(rt)? {
        return Ok(None);
    }
    let frame = state(rt)?.doc.frame;
    Ok(read(rt, |f| f.child_named(frame, name))?.flatten())
}

/// True if the frame exists.
pub(crate) fn frame_alive(rt: &Runtime, frame: u32) -> VmResult<bool> {
    Ok(read(rt, |f| f.is_alive(frame))?.unwrap_or(false))
}

/// The number of child frames of the frame `frame`.
pub(crate) fn child_count_of_frame(rt: &Runtime, frame: u32) -> VmResult<u32> {
    Ok(read(rt, |f| f.child_count(frame))?.unwrap_or(0))
}

/// The first child frame named `name` of the frame `frame`, as a proxy.
pub(crate) fn child_named_of_frame(
    rt: &mut Runtime,
    frame: u32,
    name: &str,
) -> VmResult<Option<Gc<Object>>> {
    match read(rt, |f| f.child_named(frame, name))?.flatten() {
        Some(child) => window_of_frame(rt, child),
        None => Ok(None),
    }
}

/// `parent` of the cross-origin `Window` `proxy` of the frame `frame`: a proxy of
/// this runtime, `null` for a frame that is gone (measured).
pub(crate) fn parent_of_remote(
    rt: &mut Runtime,
    proxy: Gc<Object>,
    frame: u32,
) -> VmResult<Option<Gc<Object>>> {
    if !frame_alive(rt, frame)? {
        return Ok(None);
    }
    match read(rt, |f| f.parent(frame))?.flatten() {
        Some(parent) => window_of_frame(rt, parent),
        None => Ok(Some(proxy)),
    }
}

/// `top` of the cross-origin `Window` `proxy` of the frame `frame`.
pub(crate) fn top_of_remote(
    rt: &mut Runtime,
    proxy: Gc<Object>,
    frame: u32,
) -> VmResult<Option<Gc<Object>>> {
    if !frame_alive(rt, frame)? {
        return Ok(None);
    }
    if frame == 0 {
        return Ok(Some(proxy));
    }
    window_of_frame(rt, 0)
}

/// The number of child frames of the frame of `proxy`.
pub(crate) fn child_count_of_proxy(rt: &Runtime, proxy: Gc<Object>) -> VmResult<u32> {
    if no_frames(rt)? {
        return Ok(0);
    }
    let Some(frame) = rt.window_proxy_frame(proxy.into()) else {
        return Ok(0);
    };
    Ok(read(rt, |f| f.child_count(frame))?.unwrap_or(0))
}

/// The child frame of the frame of `proxy` at `index`.
pub(crate) fn child_of_proxy(
    rt: &mut Runtime,
    proxy: Gc<Object>,
    index: u32,
) -> VmResult<Option<Gc<Object>>> {
    if no_frames(rt)? {
        return Ok(None);
    }
    let Some(frame) = rt.window_proxy_frame(proxy.into()) else {
        return Ok(None);
    };
    match read(rt, |f| f.child_at(frame, index))?.flatten() {
        Some(child) => window_of_frame(rt, child),
        None => Ok(None),
    }
}

// ----- Making and destroying frames inside a task -----

/// The post-connection steps of an `iframe` element of the current document
/// (HTML "the iframe element"): the frame exists at once with its initial
/// `about:blank` document, and without a document to load, `load` fires at
/// the element now (measured, cases `about-blank`, `scripted-load-recursion`).
/// A listener can add or remove frames again; the limits of frames and depth
/// bound that, and so does the script stack.
pub(crate) fn connected(rt: &mut Runtime, element: NodeId) -> VmResult<()> {
    let state_now = state(rt)?;
    if state_now.doc.retired {
        return Ok(());
    }
    let parent = state_now.doc.frame;
    let Some(frames) = state_now.frame_host.clone() else {
        return Ok(());
    };
    {
        let document = dom(rt)?;
        if document.get(element).is_none() || document.root(element) != NodeId::DOCUMENT {
            return Ok(());
        }
    }
    let made = {
        let mut frames = frames.try_borrow_mut().map_err(|_| in_use())?;
        if frames.child_of_container(parent, element).is_some() {
            return Ok(());
        }
        frames.connect(parent, element, engine_mut(rt)?)
    };
    let Some(made) = made else {
        return Ok(());
    };
    if made.loaded_at_once {
        change(rt, |f| f.frame_loaded(made.frame))?;
        fire_load_at(rt, element)?;
    }
    Ok(())
}

/// The `iframe` elements that the parser inserted get their frames, as
/// [`connected`] does for the insertions of scripts. The parser runs to the
/// next `script` end tag, so their `load` events fire after that step and
/// before the script (measured order: `load` of an `iframe` before the next
/// inline script).
pub(crate) fn parser_inserted(rt: &mut Runtime, elements: Vec<NodeId>) -> VmResult<()> {
    for element in elements {
        connected(rt, element)?;
    }
    Ok(())
}

/// Fires `load` at the element `element` of the current document.
fn fire_load_at(rt: &mut Runtime, element: NodeId) -> VmResult<()> {
    crate::scripts::fire(
        rt,
        DispatchTarget::Node(element),
        &TrustedEvent::new("load"),
    )?;
    Ok(())
}

/// The `load` event of the current document's window has fired: its
/// container's `load` fires next, before `pageshow` (measured, case
/// `scripted-frame-document`).
///
/// True if the parent's document is of another origin: a task runs no script of
/// another runtime, so the page fires that `load` after this task, and `pageshow`
/// waits for it.
pub(crate) fn container_load(rt: &mut Runtime) -> VmResult<bool> {
    let frame = state(rt)?.doc.frame;
    if frame == 0 || state(rt)?.doc.retired {
        return Ok(false);
    }
    change(rt, |f| f.frame_loaded(frame))?;
    let Some(Some(parent)) = read(rt, |f| f.parent(frame))? else {
        return Ok(false);
    };
    if read(rt, |f| f.in_this_agent(parent))? != Some(true) {
        return Ok(true);
    }
    let Some(Some(container)) = read(rt, |f| f.container(frame))? else {
        return Ok(false);
    };
    let Some(Some(key)) = read(rt, |f| f.document_key(parent))? else {
        return Ok(false);
    };
    let entered = state::switch_if_other(rt, key.raw())?;
    let result = fire_load_at(rt, container);
    state::leave(rt, entered);
    result.map(|()| false)
}

/// Before `node` (or, without `inclusive`, its children) leaves the current
/// document: the frames of the `iframe` elements inside are destroyed
/// (`contentWindow` is `null` at once; a move makes a new frame, measured).
pub(crate) fn before_removal(rt: &mut Runtime, node: NodeId, inclusive: bool) -> VmResult<()> {
    let state_now = state(rt)?;
    if state_now.frame_count.get() == 0 || state_now.doc.retired {
        return Ok(());
    }
    let frame = state_now.doc.frame;
    let Some(children) = read(rt, |f| f.child_containers(frame))? else {
        return Ok(());
    };
    if children.is_empty() {
        return Ok(());
    }
    let doomed: Vec<u32> = {
        let document = dom(rt)?;
        children
            .into_iter()
            .filter(|&(_, container)| {
                (inclusive && container == node)
                    || (document.get(container).is_some()
                        && document.ancestors(container).any(|a| a == node))
            })
            .map(|(child, _)| child)
            .collect()
    };
    for child in doomed {
        destroy(rt, child)?;
    }
    Ok(())
}

/// Destroys frame `frame` and its descendants: the documents with a realm
/// unload (their listeners can run script), then they retire. The page acts on
/// the rest after the task.
fn destroy(rt: &mut Runtime, frame: u32) -> VmResult<()> {
    let subtree = read(rt, |f| f.subtree(frame))?.unwrap_or_default();
    for number in subtree {
        if let Some(Some(key)) = read(rt, |f| f.document_key(number))? {
            let entered = state::switch_if_other(rt, key.raw())?;
            let result = crate::document_load::unload_steps(rt);
            state::leave(rt, entered);
            result?;
        }
    }
    let retired = change(rt, |f| f.disconnect(frame))?.unwrap_or_default();
    for one in retired {
        if let Some(key) = one.key {
            state::retire_document(rt, key.raw(), one.doc)?;
        }
        state_mut(rt)?.window_proxies.remove(&one.frame);
    }
    state::with_registry_and_documents(rt, |frames, docs| frames.focus_after_disconnect(docs))
}

/// `document.hasFocus()` of a document of a frame: its frame holds the page's
/// focus, or a frame inside it does (measured, case `focus-by-script`).
pub(crate) fn holds_focus(rt: &Runtime) -> VmResult<bool> {
    let state_now = state(rt)?;
    if state_now.doc.retired {
        return Ok(false);
    }
    let frame = state_now.doc.frame;
    Ok(read(rt, |f| f.holds_focus(frame))?.unwrap_or(false))
}

/// A script focuses an element or the window of the current document: if the
/// page's focus is in another document, it moves here first. Measured (cases
/// `focus-by-script`, `focus-by-script-nested`), all inside the call: the old
/// element gets `blur`, the old window `blur`, this window `focus`, and then the
/// element its own `focus`. Windows between the two get nothing.
pub(crate) fn focus_enters_current(rt: &mut Runtime) -> VmResult<()> {
    if no_frames(rt)? || state(rt)?.doc.retired {
        return Ok(());
    }
    let here = state(rt)?.doc.frame;
    let Some(old) = read(rt, |f| f.focused_frame())? else {
        return Ok(());
    };
    if old == here {
        return Ok(());
    }
    if let Some(Some(key)) = read(rt, |f| f.document_key(old))? {
        let entered = state::switch_if_other(rt, key.raw())?;
        let result = crate::html::focus::focus_leaves_document(rt);
        state::leave(rt, entered);
        result?;
    }
    state::with_registry_and_documents(rt, |frames, docs| frames.set_focused_frame(here, docs))?;
    crate::html::focus::window_focus(rt, true)
}

/// `focus()` of an `iframe` element with a frame of this agent: the page's focus
/// goes to the frame's document (measured: the frame's window gets `focus`).
/// False for another element.
pub(crate) fn focus_enters_frame_of(rt: &mut Runtime, element: NodeId) -> VmResult<bool> {
    if no_frames(rt)? {
        return Ok(false);
    }
    let parent = state(rt)?.doc.frame;
    let Some(Some(child)) = read(rt, |f| f.child_of_container(parent, element))? else {
        return Ok(false);
    };
    if window_of_frame(rt, child)?.is_none() {
        return Ok(false);
    }
    let Some(Some(key)) = read(rt, |f| f.document_key(child))? else {
        focus_enters_remote(rt, child)?;
        return Ok(true);
    };
    let entered = state::switch_if_other(rt, key.raw())?;
    let result = focus_enters_current(rt);
    state::leave(rt, entered);
    result?;
    Ok(true)
}

/// `focus()` of a frame without a realm here (a cross-origin `Window`): as
/// [`focus_enters_current`]. The frame's own window is in another runtime, and the
/// page fires its events after the task. Nothing for a frame that is gone
/// (measured).
pub(crate) fn focus_enters_remote(rt: &mut Runtime, frame: u32) -> VmResult<()> {
    if !frame_alive(rt, frame)? {
        return Ok(());
    }
    let Some(old) = read(rt, |f| f.focused_frame())? else {
        return Ok(());
    };
    if old == frame {
        return Ok(());
    }
    if let Some(Some(key)) = read(rt, |f| f.document_key(old))? {
        let entered = state::switch_if_other(rt, key.raw())?;
        let result = crate::html::focus::focus_leaves_document(rt);
        state::leave(rt, entered);
        result?;
    }
    state::with_registry_and_documents(rt, |frames, docs| frames.set_focused_frame(frame, docs))
}

/// True if the code that called the running native may navigate the frame of
/// the current document (a write to `location`, ADR 0031 section 3).
pub(crate) fn caller_may_navigate(rt: &Runtime) -> VmResult<bool> {
    if !rt.host_flag() || state(rt)?.doc.retired {
        return Ok(true);
    }
    let target = state(rt)?.doc.frame;
    let caller = state::caller_document(rt)?;
    if caller.retired {
        return Ok(false);
    }
    let source = caller.frame;
    if source == target {
        return Ok(true);
    }
    Ok(read(rt, |f| f.may_navigate(source, target))?.unwrap_or(true))
}

/// True if the document of frame `source` may navigate frame `target`.
pub(crate) fn may_navigate(rt: &Runtime, source: u32, target: u32) -> VmResult<bool> {
    Ok(read(rt, |f| f.may_navigate(source, target))?.unwrap_or(false))
}

/// Asks the page to navigate frame `target` to `url` when the task ends, for the
/// document of frame `source`.
pub(crate) fn request_navigation(
    rt: &mut Runtime,
    target: u32,
    url: &str,
    source: u32,
    replace: bool,
) -> VmResult<()> {
    state::with_registry_and_documents(rt, |frames, docs| {
        frames.note_location_flags(target, docs);
        frames.navigate_frame(target, url, source, replace);
    })
}

/// A script writes to the `location` of the current document's frame, for a
/// navigation that the page starts after the task: it has the sandboxing flags of
/// this moment (measured, case `xo-navigate-flags-at-the-write`).
pub(crate) fn note_location_flags(rt: &mut Runtime) -> VmResult<()> {
    let frame = state(rt)?.doc.frame;
    if frame == 0 || no_frames(rt)? {
        return Ok(());
    }
    state::with_registry_and_documents(rt, |frames, docs| {
        frames.note_location_flags(frame, docs);
    })
}

/// Where a `javascript:` URL of the current document goes.
pub(crate) enum ScriptTarget {
    /// The current document.
    Here,
    /// Another document of this agent.
    Document(DocKey),
    /// Nowhere: no such frame, a frame that the rules refuse, a frame of another
    /// origin or without scripts.
    None,
}

/// The document of the frame that the name `target` means for the current
/// document, for a `javascript:` URL in a link or a form.
pub(crate) fn script_target(rt: &mut Runtime, target: &str) -> VmResult<ScriptTarget> {
    let source = state(rt)?.doc.frame;
    let chosen = read(rt, |f| f.choose_frame(source, target))?;
    let Some(chosen) = chosen else {
        // A runtime without a page: one document, which `_parent` and `_top`
        // name, and no other frames.
        let current = ["_parent", "_top"]
            .iter()
            .any(|name| target.eq_ignore_ascii_case(name));
        return Ok(if current {
            ScriptTarget::Here
        } else {
            ScriptTarget::None
        });
    };
    let Some(frame) = chosen else {
        return Ok(ScriptTarget::None);
    };
    if frame == source {
        return Ok(ScriptTarget::Here);
    }
    let allowed = read(rt, |f| f.in_this_agent(frame) && f.scripts_allowed(frame))?;
    if allowed != Some(true) || window_of_frame(rt, frame)?.is_none() {
        return Ok(ScriptTarget::None);
    }
    Ok(match read(rt, |f| f.document_key(frame))?.flatten() {
        Some(key) => ScriptTarget::Document(key.raw()),
        None => ScriptTarget::None,
    })
}

/// The value of `window.opener`: swb has no auxiliary browsing contexts.
pub(crate) fn opener() -> Value {
    Value::Null
}
