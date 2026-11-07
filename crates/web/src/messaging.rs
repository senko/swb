//! `window.postMessage()` (HTML §9.3.3 "window postMessage steps",
//! <https://html.spec.whatwg.org/multipage/web-messaging.html#dom-window-postmessage>;
//! ADR 0031 section 7).
//!
//! The call checks `targetOrigin`, serializes the message with the transfer list
//! (`structured.rs`; `ArrayBuffer`s detach in the sender, `MessagePort`s travel in the
//! record) and queues a task on the posted message task source. The task checks the
//! target origin against the target's document at that time, deserializes the message
//! in the window's realm and fires a trusted `message` event (`MessageEvent`: `origin` is
//! the origin of the sender's document, `source` the sender's `WindowProxy`, `ports` the
//! adopted ports, `lastEventId` empty). Measured in Chromium 148
//! (`tools/probes/web-small-gaps.json`, the cases `msg-*` of
//! `tools/probes/web-frames.json`):
//!
//! - a message is a task like any other: a timer of 0 ms queued before it runs first,
//!   microtasks run before it, a timer of 0 ms queued after it runs after it, and a timer
//!   of 1 ms runs last; messages arrive in the order of the calls;
//! - `targetOrigin` `*` always delivers; `/` is the own origin (also an opaque origin,
//!   which delivers in its own runtime and nowhere else); a URL is parsed without a base,
//!   so `null`, `foo`, `""` and `"  /  "` are a `SyntaxError`, and its origin must be the
//!   document's (a `file:` URL has an opaque origin, which no URL names, so such a message
//!   is dropped without an error).
//!
//! **Across runtimes.** A `Gc`, a document key and a proxy belong to one runtime, so what
//! crosses is plain data: the record, the serialized origin of the sender, and the number
//! of the sender's frame. The sender's task leaves a [`Mail`] with the sequence number of
//! the call; it takes the sender's slice of the event loop to its end
//! (`ScriptRuntime::run_due_tasks_before`), and the page hands the mail to the runtime of
//! the frame's document, between tasks (`Page::hand_over_mail`). There the message waits
//! in the queue of the document at its place by number, with a [`Task::Message`] at its
//! place in the event loop. A frame without a document in a runtime gets nothing; the
//! drop of a [`Posted`] gives its bytes back and closes the ports of its record. So does
//! the retirement of the document: a message goes with the document that it was sent to,
//! except that the initial `about:blank` passes its window and its messages on.
//!
//! Bounds (ADR 0031 section 11): at most `MAX_WAITING` messages and `MAX_BYTES`
//! (`message_port.rs`) of serialized bytes wait for each sender, the messages of the
//! window and of the ports together. A call past a bound drops its message with one
//! warning until nothing waits again (Chromium has no limit); a dropped message does not
//! detach its transfer list. A flood thus ends with the bound and not with the heap, and
//! it uses up the budget of its sender and not the target's.

use std::collections::VecDeque;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use swb_js::{Gc, Object, Runtime, Termination, Tracer, Value, VmError, VmResult};
use swb_net::Url;
use url::Origin;

use crate::bindings::{self as b, Cx, Exception};
use crate::event_loop::{Task, page_now};
use crate::events::{Extra, fire_at_object, misc::Other};
use crate::generated as g;
use crate::message_port::{self, Charge, PortHub, RuntimeTag};
use crate::state::{DocKey, document_mut, engine, state, state_mut};
use crate::structured::{
    Serialized, deserialize_with_ports, detach_transfer, serialize_for_transfer,
};

/// What the `targetOrigin` of a call names.
#[derive(Debug)]
enum Target {
    /// `*`: any origin.
    Any,
    /// `/`: the origin of the sender, which is the origin of the target.
    Own,
    /// The origin of a URL. An opaque origin never equals another.
    Origin(Origin),
}

/// The window of the code that called `postMessage` (`MessageEvent.source`; HTML's
/// incumbent settings object, measured).
#[derive(Debug)]
enum Source {
    /// A document of this runtime.
    Document(DocKey),
    /// The number of the sender's frame, in another runtime.
    Frame(u32),
    /// The proxy of that frame in this runtime, which it had when the message came.
    /// It keeps a window whose frame goes before the delivery (measured).
    Proxy(Gc<Object>),
}

/// A message that waits for its task. Its drop gives its bytes back and closes the ports
/// of its record that no realm adopted.
#[derive(Debug)]
pub(crate) struct Posted {
    /// The sequence number of the call.
    seq: u64,
    record: Serialized,
    charge: Charge,
    target: Target,
    /// The serialized origin of the sender (`MessageEvent.origin`).
    origin: String,
    source: Source,
    hub: Weak<PortHub>,
}

impl Drop for Posted {
    fn drop(&mut self) {
        if !self.record.ports().is_empty()
            && let Some(hub) = self.hub.upgrade()
        {
            hub.doom(self.record.ports());
        }
    }
}

/// The messages that wait for a window, by sequence number (each has a
/// [`Task::Message`] in the task queue, and the tasks of a window run in that order, so
/// a task takes the first one). A message of another runtime can come after one of this
/// runtime with a higher number.
#[derive(Debug, Default)]
pub(crate) struct Messages {
    queue: VecDeque<Posted>,
}

impl Messages {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for posted in &self.queue {
            if let Source::Proxy(proxy) = posted.source {
                tracer.object(proxy);
            }
        }
    }

    fn insert(&mut self, posted: Posted) {
        let at = self.queue.partition_point(|other| other.seq < posted.seq);
        self.queue.insert(at, posted);
    }
}

/// What the page hands from one runtime to another between tasks. It holds nothing that
/// belongs to a runtime.
#[derive(Debug)]
pub struct Mail {
    due: f64,
    seq: u64,
    kind: MailKind,
}

#[derive(Debug)]
enum MailKind {
    /// A window message for the current document of a frame.
    Window { frame: u32, posted: Box<Posted> },
    /// The port `id` of the runtime `home` has a message: its task.
    Port { home: RuntimeTag, id: u64 },
}

/// Which runtime a [`Mail`] is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailTarget {
    /// The runtime of the current document of this frame.
    Frame(u32),
    /// The runtime with this tag ([`crate::ScriptRuntime::mail_tag`]).
    Runtime(u32),
}

impl Mail {
    pub(crate) fn port(home: RuntimeTag, id: u64, due: f64, seq: u64) -> Mail {
        Mail {
            due,
            seq,
            kind: MailKind::Port { home, id },
        }
    }

    /// Which runtime it is for. A mail whose runtime is not there goes with its drop: its
    /// bytes go back and the ports in it close.
    pub fn target(&self) -> MailTarget {
        match self.kind {
            MailKind::Window { frame, .. } => MailTarget::Frame(frame),
            MailKind::Port { home, .. } => MailTarget::Runtime(home),
        }
    }
}

/// Step 5 to 7 of the window postMessage steps: `*`, `/`, or the origin of
/// the parsed URL. A URL that does not parse is a `SyntaxError`.
fn parse_target(target_origin: &str) -> b::Result<Target> {
    match target_origin {
        "*" => Ok(Target::Any),
        "/" => Ok(Target::Own),
        other => Url::parse(other)
            .map(|url| Target::Origin(url.origin()))
            .map_err(|_| {
                Exception::dom(
                    "SyntaxError",
                    format!("Invalid target origin '{other}' in a call to 'postMessage'."),
                )
            }),
    }
}

/// `window.postMessage(message, targetOrigin, transfer)` and the options form to a window
/// of this runtime: the current document's. Serializes the message, checks the target
/// origin and queues the task.
pub(crate) fn post(
    cx: &mut Cx<'_>,
    message: Value,
    target_origin: &str,
    transfer: &[Gc<Object>],
) -> b::Result<()> {
    let record = serialize_for_transfer(cx.rt, message, transfer, None)?;
    if state(cx.rt)?.doc.retired {
        // A window without its document gets no message, and a window that is gone does
        // not look at the target origin; the transfer list still detaches (measured,
        // cases `scripted-retired`, `msg-target-removed-same`).
        return drop_at_a_window_that_is_gone(cx, &record, transfer);
    }
    // Measured, case `xo-post-message`: the transfer list detaches before the
    // check of the target origin fails.
    let target = match parse_target(target_origin) {
        Ok(target) => target,
        Err(error) => {
            detach_transfer(cx.rt, transfer)?;
            message_port::release_ports(cx.rt, record.ports());
            return Err(error);
        }
    };
    let source = crate::state::caller_key(cx.rt)?;
    let origin = crate::state::origin_of_document(cx.rt, source)?;
    let Some(charge) = state(cx.rt)?.ports.admit(record.message_bytes()) else {
        return Ok(());
    };
    // A failure here drops the charge: the message does not wait.
    detach_transfer(cx.rt, transfer)?;
    let now = page_now(cx.rt);
    let state = state_mut(cx.rt)?;
    let hub = state.ports.hub();
    hub.hold_for_window(record.ports());
    let weak = Rc::downgrade(hub);
    let tasks = &mut state.event_loop.tasks;
    let seq = tasks.take_seq();
    let key = state.doc.key;
    tasks.push_for(key, now, seq, Task::Message);
    state.doc.messages.queue.push_back(Posted {
        seq,
        record,
        charge,
        target,
        origin,
        source: Source::Document(source),
        hub: weak,
    });
    Ok(())
}

/// The effects on the sender of a message to a window that is gone.
fn drop_at_a_window_that_is_gone(
    cx: &mut Cx<'_>,
    record: &Serialized,
    transfer: &[Gc<Object>],
) -> b::Result<()> {
    detach_transfer(cx.rt, transfer)?;
    message_port::release_ports(cx.rt, record.ports());
    Ok(())
}

/// `postMessage` to a `Window` of another runtime, the frame `frame`: the checks and the
/// effects on the sender, in Chromium's order (measured, case `xo-post-message`: the
/// serialization and the transfer come before the check of the target origin), and a
/// [`Mail`] for the page. The `origin` of the message comes from the sender's document.
pub(crate) fn post_across_runtimes(
    cx: &mut Cx<'_>,
    frame: u32,
    message: Value,
    target_origin: &str,
    transfer: &[Gc<Object>],
) -> b::Result<()> {
    let record = serialize_for_transfer(cx.rt, message, transfer, None)?;
    if !crate::frames::frame_alive(cx.rt, frame)? {
        return drop_at_a_window_that_is_gone(cx, &record, transfer);
    }
    let target = match parse_target(target_origin) {
        Ok(target) => target,
        Err(error) => {
            detach_transfer(cx.rt, transfer)?;
            message_port::release_ports(cx.rt, record.ports());
            return Err(error);
        }
    };
    let source = crate::state::caller_key(cx.rt)?;
    let origin = crate::state::origin_of_document(cx.rt, source)?;
    // `/` names the sender's origin. An opaque origin is of one document's runtime, and
    // the serialized `null` of another runtime is not the same origin.
    let target = match target {
        Target::Own if origin == "null" => Target::Origin(Origin::new_opaque()),
        other => other,
    };
    let source_frame = crate::state::frame_of_document(cx.rt, source)?;
    let Some(charge) = state(cx.rt)?.ports.admit(record.message_bytes()) else {
        return Ok(());
    };
    log::debug!(
        "postMessage: frame {source_frame} of {origin} to frame {frame}, target {target:?}, {} bytes, {} ports",
        record.message_bytes(),
        record.ports().len()
    );
    // A failure here drops the charge: the message does not wait.
    detach_transfer(cx.rt, transfer)?;
    let due = page_now(cx.rt);
    let state = state_mut(cx.rt)?;
    let seq = state.event_loop.tasks.take_seq();
    let hub = Rc::clone(state.ports.hub());
    hub.hold_for_window(record.ports());
    hub.post_mail(Mail {
        due,
        seq,
        kind: MailKind::Window {
            frame,
            posted: Box::new(Posted {
                seq,
                record,
                charge,
                target,
                origin,
                source: Source::Frame(source_frame),
                hub: Rc::downgrade(&hub),
            }),
        },
    });
    Ok(())
}

/// The page hands `mail` to this runtime: a window message for the document `document`,
/// or the task of a port. It waits at its place in the order of the page.
pub(crate) fn receive(rt: &mut Runtime, document: Option<DocKey>, mail: Mail) -> VmResult<()> {
    let Mail { due, seq, kind } = mail;
    match kind {
        MailKind::Port { id, .. } => message_port::receive_pump(rt, id, due, seq),
        MailKind::Window { posted, .. } => {
            let Some(document) = document else {
                return Ok(());
            };
            let mut posted = *posted;
            let state = state_mut(rt)?;
            if let Source::Frame(frame) = posted.source
                && let Some(&proxy) = state.window_proxies.get(&frame)
            {
                posted.source = Source::Proxy(proxy);
            }
            let Some(record) = document_mut(state, document) else {
                return Ok(());
            };
            if record.retired {
                return Ok(());
            }
            record.messages.insert(posted);
            state
                .event_loop
                .tasks
                .push_for(document, due, seq, Task::Message);
            Ok(())
        }
    }
}

/// A message of at most this estimate comes into the heap at any heap size: the channels
/// of a page's scheduler keep working near the limit, and such a message forces no
/// collection. One task holds one message, so it cannot take the heap past the limit.
const SMALL_MESSAGE: usize = 64 << 10;

/// What large messages cost their target besides the listeners. Another runtime sends them,
/// and it must not end the scripts of the target with them (ADR 0031 section 7).
#[derive(Debug, Default)]
pub(crate) struct MessageCost {
    /// The estimates of the large messages that were deserialized since the last collection
    /// for a message. A collection in a listener measures a message as live, and it is
    /// garbage after its task, so these bytes count as allocated.
    delivered: usize,
    /// The time of the deserializations of the running task, which the long tasks of the
    /// target do not count.
    time: Duration,
}

impl MessageCost {
    /// Takes the time of the deserializations of the task that ends.
    pub(crate) fn take_time(&mut self) -> Duration {
        std::mem::take(&mut self.time)
    }
}

/// The heap size up to which a larger message comes in: 7/8 of the limit. A collection that
/// leaves more than 90 % ends the scripts (`Heap::safepoint`), and the listener needs room.
fn message_room_end(rt: &Runtime) -> usize {
    let limit = rt.heap().config().limit;
    limit - limit / 8
}

/// A full collection for a message; false if there was none, because too little came in
/// since the last: an eighth of the heap pays for it, as with the collector's own rule. So a
/// frame gets no collection per message from messages that do not come in.
fn collect_for_message(rt: &mut Runtime) -> bool {
    let delivered = state(rt).map_or(0, |state| state.message_cost.delivered);
    let heap = rt.heap();
    if heap.allocated_since_gc().saturating_add(delivered) < heap.heap_size() / 8 {
        return false;
    }
    rt.collect();
    if let Ok(state) = state_mut(rt) {
        state.message_cost.delivered = 0;
    }
    true
}

/// True if the objects of `record` are likely to fit in the heap, with a collection when
/// that can decide it.
fn fits_heap(rt: &mut Runtime, record: &Serialized) -> bool {
    let needed = record.heap_estimate();
    let end = message_room_end(rt);
    if rt.heap().heap_size().saturating_add(needed) <= end {
        return true;
    }
    needed <= end && collect_for_message(rt) && rt.heap().heap_size().saturating_add(needed) <= end
}

/// True if the heap keeps its room with the deserialized message `data` and `ports` in it.
/// The size before a collection is not exact, and neither is the estimate, so near the
/// end of the room a collection measures it.
fn stays_in_room(rt: &mut Runtime, record: &Serialized, data: Value, ports: &[Gc<Object>]) -> bool {
    let end = message_room_end(rt);
    if rt
        .heap()
        .heap_size()
        .saturating_add(record.heap_estimate() / 4)
        <= end
    {
        return true;
    }
    keep_message(rt, data, ports);
    collect_for_message(rt) && rt.heap().heap_size() <= end
}

/// Keeps a deserialized message in the handle scope that is open: a port that no value of
/// the message refers to has no other root yet.
fn keep_message(rt: &mut Runtime, data: Value, ports: &[Gc<Object>]) {
    b::keep(rt, data);
    for &port in ports {
        b::keep(rt, Value::Object(port));
    }
}

fn no_room() -> Exception {
    Exception::type_error("the message does not fit in the heap")
}

/// [`deserialize_message`] for a message above [`SMALL_MESSAGE`].
fn deserialize_large_message(
    rt: &mut Runtime,
    realm: u32,
    record: &Serialized,
) -> b::Result<(Value, Vec<Gc<Object>>)> {
    let started = Instant::now();
    let result = if fits_heap(rt, record) {
        let scope = rt.heap_mut().open_scope();
        let result = match deserialize_with_ports(rt, realm, record) {
            Ok((data, ports)) if !stays_in_room(rt, record, data, &ports) => Err(no_room()),
            result => result,
        };
        if let Err(error) = rt.heap_mut().close_scope(scope) {
            log::warn!("the handle scope of a message was not the last one: {error:?}");
        }
        if let Ok((data, ports)) = &result {
            keep_message(rt, *data, ports);
        }
        if let Ok(state) = state_mut(rt) {
            let cost = &mut state.message_cost;
            cost.delivered = cost.delivered.saturating_add(record.heap_estimate());
        }
        result
    } else {
        Err(no_room())
    };
    if let Ok(state) = state_mut(rt) {
        state.message_cost.time += started.elapsed();
    }
    result
}

/// The value of the message `record` and its ports in the realm `realm`. A message that
/// does not fit in the heap gives an exception, so `messageerror`, instead of ending the
/// scripts of its target: another runtime can send it. What was made is garbage then.
pub(crate) fn deserialize_message(
    rt: &mut Runtime,
    realm: u32,
    record: &Serialized,
) -> b::Result<(Value, Vec<Gc<Object>>)> {
    let result = if record.heap_estimate() <= SMALL_MESSAGE {
        deserialize_with_ports(rt, realm, record)
    } else {
        deserialize_large_message(rt, realm, record)
    };
    match result {
        Err(Exception::Js(VmError::Terminated(Termination::HeapLimit))) => Err(no_room()),
        result => result,
    }
}

/// The task: the first waiting message of the document. Drops it if its target origin is
/// not the document's origin now (step 7.1 of the "message" task), else fires `message`
/// at the window, or `messageerror` if the data cannot come into the realm.
pub(crate) fn deliver(rt: &mut Runtime) -> VmResult<()> {
    let messages = &mut state_mut(rt)?.doc.messages;
    let Some(mut posted) = messages.queue.pop_front() else {
        return Ok(());
    };
    posted.charge.settle();
    if let Some(hub) = posted.hub.upgrade() {
        hub.unhold_for_window(posted.record.ports());
    }
    if let Source::Proxy(proxy) = posted.source {
        // It is out of the queue: the collector no longer sees it there.
        b::keep(rt, Value::Object(proxy));
    }
    let delivered = match &posted.target {
        Target::Any => true,
        Target::Own => posted.origin == engine(rt)?.origin_text(),
        // The document's origin: a same-origin `about:` frame has its creator's.
        // An opaque origin equals no other.
        Target::Origin(origin) => {
            origin.is_tuple() && origin.ascii_serialization() == engine(rt)?.origin_text()
        }
    };
    if !delivered {
        log::debug!(
            "postMessage: the message is not delivered: the target origin does not match the document"
        );
        return Ok(());
    }
    let realm = crate::state::doc_realm(rt)?;
    let received = receive_message(rt, realm, &posted.record, |_| Ok(()))?;
    if received.kind == "message" {
        // The ports have their objects now: the record no longer holds them.
        posted.record.forget_ports();
    }
    let window = crate::state::window_proxy(rt)?;
    let source = source_window(rt, &posted.source)?;
    let origin = std::mem::take(&mut posted.origin);
    fire_message(rt, window, received, origin, Some(source))
}

/// The data of a `message` event, or the lack of it.
pub(crate) struct Received {
    /// `message`, or `messageerror` if the data cannot come into the realm.
    pub(crate) kind: &'static str,
    pub(crate) data: Value,
    pub(crate) ports: Vec<Gc<Object>>,
}

/// Deserializes `record` in the realm `realm` for a `message` event. If the data cannot
/// come into the realm, `on_error` runs and the result is a `messageerror`. A
/// termination or an internal error passes on.
pub(crate) fn receive_message(
    rt: &mut Runtime,
    realm: u32,
    record: &Serialized,
    on_error: impl FnOnce(&mut Runtime) -> VmResult<()>,
) -> VmResult<Received> {
    match deserialize_message(rt, realm, record) {
        Ok((data, ports)) => {
            b::keep(rt, data);
            Ok(Received {
                kind: "message",
                data,
                ports,
            })
        }
        Err(Exception::Js(error @ (VmError::Terminated(_) | VmError::Internal(_)))) => Err(error),
        Err(_) => {
            on_error(rt)?;
            Ok(Received {
                kind: "messageerror",
                data: Value::Null,
                ports: Vec::new(),
            })
        }
    }
}

/// Fires the `message` (or `messageerror`) event `received` at `target`, with the
/// `origin` and `source` of the sender (a port message has neither).
pub(crate) fn fire_message(
    rt: &mut Runtime,
    target: Gc<Object>,
    received: Received,
    origin: String,
    source: Option<Gc<Object>>,
) -> VmResult<()> {
    let other = Other::Message {
        data: received.data,
        origin,
        last_event_id: String::new(),
        source,
        ports: received.ports,
        frozen: None,
    };
    fire_at_object(
        rt,
        target,
        g::MESSAGE_EVENT,
        received.kind,
        false,
        Extra::Other(Box::new(other)),
    )
    .map(drop)
}

/// `MessageEvent.source`: the proxy of the sender in this runtime. A frame that went
/// before this runtime saw it still has a window, which is closed (measured: the sender
/// that goes after the call is an object, case `msg-sender-removed-*`).
fn source_window(rt: &mut Runtime, source: &Source) -> VmResult<Gc<Object>> {
    let proxy = match *source {
        Source::Document(key) => crate::state::window_proxy_of(rt, key)?,
        Source::Proxy(proxy) => proxy,
        Source::Frame(frame) => match crate::frames::window_of_frame(rt, frame)? {
            Some(proxy) => proxy,
            None => rt.new_window_proxy(frame, None)?,
        },
    };
    b::keep(rt, Value::Object(proxy));
    Ok(proxy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_origins() {
        assert!(matches!(parse_target("*"), Ok(Target::Any)));
        assert!(matches!(parse_target("/"), Ok(Target::Own)));
        let origin = |text: &str| match parse_target(text) {
            Ok(Target::Origin(origin)) => origin,
            other => panic!("{text}: {other:?}"),
        };
        assert_eq!(
            origin("http://a.test/path?q#f"),
            origin("http://a.test:80/")
        );
        assert_eq!(origin("HTTP://A.TEST "), origin("http://a.test"));
        assert_ne!(origin("http://a.test/"), origin("https://a.test/"));
        // A `file:` URL has an opaque origin, which equals no other.
        assert_ne!(origin("file:///x"), origin("file:///x"));
        for bad in ["", "foo", "null", "  /  ", "//a.test", "a.test"] {
            assert!(
                matches!(
                    parse_target(bad),
                    Err(Exception::Dom {
                        name: "SyntaxError",
                        ..
                    })
                ),
                "{bad}"
            );
        }
    }
}
