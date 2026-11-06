//! `window.postMessage()` to the own window (HTML §9.3.3 "window
//! postMessage steps",
//! <https://html.spec.whatwg.org/multipage/web-messaging.html#dom-window-postmessage>;
//! ADR 0031 section 7, M9 feature 1).
//!
//! swb has no frames yet, so the only target is the window of the page. The
//! call checks `targetOrigin`, serializes the message with the transfer list
//! (`structured.rs`; `ArrayBuffer`s detach in the sender) and queues a task
//! on the posted message task source. The task checks the target origin
//! again, deserializes the message in the window's realm and fires a
//! trusted `message` event (`MessageEvent`: `origin` is the origin of the
//! sender, `source` the sender's window, `ports` empty, `lastEventId`
//! empty) at the window. Measured in Chromium 148
//! (`tools/probes/web-small-gaps.json`):
//!
//! - a message to the own window is a task like any other: a timer of 0 ms
//!   queued before it runs first, microtasks run before it, a timer of 0 ms
//!   queued after it runs after it, and a timer of 1 ms runs last;
//! - `targetOrigin` `*` always delivers; `/` is the own origin (also an
//!   opaque origin, which delivers); a URL is parsed without a base, so
//!   `null`, `foo`, `""` and `"  /  "` are a `SyntaxError`, and its origin
//!   must be the document's (a `file:` URL has an opaque origin, which no
//!   URL names, so such a message is dropped without an error);
//! - messages arrive in the order of the calls.
//!
//! `MessagePort`s travel in the transfer list (M9 feature 9,
//! `message_port.rs`): the record owns them until the task adopts them in
//! the window's realm (`MessageEvent.ports`); a message that the origin
//! check drops closes them.
//!
//! ADR 0031 feature 6 (messaging across documents; roadmap M9 feature 14)
//! extends this module: other windows as targets, the `WindowProxy` as
//! `source`, and the page-wide queue. Until then the queue is the
//! runtime's.
//!
//! Bounds (ADR 0031 section 11): at most `MAX_WAITING` messages and
//! `MAX_BYTES` (`message_port.rs`) of serialized bytes wait, the messages of the window and
//! of the ports together (the count is in the port registry). A call past
//! a bound drops its message with one warning until nothing waits again
//! (Chromium has no limit); a dropped message does not detach its transfer
//! list. A flood of `postMessage` calls to self thus ends with the bound and
//! not with the heap.

use std::collections::VecDeque;

use swb_js::{Gc, Object, Runtime, Value, VmError, VmResult};
use swb_net::Url;
use url::Origin;

use crate::bindings::{self as b, Cx, Exception};
use crate::event_loop::{Task, queue_task};
use crate::events::{Extra, fire_at_object, misc::Other};
use crate::generated as g;
use crate::message_port;
use crate::state::{engine, state, state_mut};
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

/// A message that waits for its task.
#[derive(Debug)]
pub(crate) struct Posted {
    record: Serialized,
    bytes: usize,
    target: Target,
    /// The serialized origin of the sender (`MessageEvent.origin`).
    origin: String,
    /// The document of the code that called `postMessage`: HTML's incumbent
    /// settings object (`MessageEvent.source`, measured).
    source: crate::state::DocKey,
}

/// The messages that wait, in the order of the calls (each has a
/// [`Task::Message`] in the task queue). The count and the bytes against the
/// bounds are in the port registry, shared with the ports' queues.
#[derive(Debug, Default)]
pub(crate) struct Messages {
    queue: VecDeque<Posted>,
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

/// `window.postMessage(message, targetOrigin, transfer)` and the options
/// form: checks the target origin, serializes the message and queues the
/// task.
pub(crate) fn post(
    cx: &mut Cx<'_>,
    message: Value,
    target_origin: &str,
    transfer: &[Gc<Object>],
) -> b::Result<()> {
    if state(cx.rt)?.doc.retired {
        // A window without its document gets no message (measured, case
        // `scripted-retired`).
        parse_target(target_origin)?;
        return Ok(());
    }
    let record = serialize_for_transfer(cx.rt, message, transfer, None)?;
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
    let bytes = record.byte_size();
    if !message_port::admit(cx.rt, bytes)? {
        return Ok(());
    }
    if let Err(error) = detach_transfer(cx.rt, transfer) {
        // The message does not wait: give its count back.
        message_port::release(cx.rt, bytes);
        return Err(error);
    }
    message_port::hold_for_window(cx.rt, record.ports());
    let messages = &mut state_mut(cx.rt)?.doc.messages;
    messages.queue.push_back(Posted {
        record,
        bytes,
        target,
        origin,
        source,
    });
    queue_task(cx.rt, Task::Message)?;
    Ok(())
}

/// `postMessage` to a cross-origin `Window`: the checks and the effects on the
/// sender, in Chromium's order (measured, case `xo-post-message`: the
/// serialization and the transfer come before the check of the target origin).
/// Feature 14 delivers the message; here it goes after the checks, and a
/// transferred port closes. So a frame of another origin receives nothing yet.
pub(crate) fn post_to_remote(
    cx: &mut Cx<'_>,
    message: Value,
    target_origin: &str,
    transfer: &[Gc<Object>],
) -> b::Result<()> {
    let record = serialize_for_transfer(cx.rt, message, transfer, None)?;
    detach_transfer(cx.rt, transfer)?;
    message_port::release_ports(cx.rt, record.ports());
    parse_target(target_origin)?;
    Ok(())
}

/// The task: the oldest waiting message. Drops it if its target origin is
/// not the document's origin now (step 7.1 of the "message" task), else
/// fires `message` at the window.
pub(crate) fn deliver(rt: &mut Runtime) -> VmResult<()> {
    let messages = &mut state_mut(rt)?.doc.messages;
    let Some(posted) = messages.queue.pop_front() else {
        return Ok(());
    };
    message_port::release(rt, posted.bytes);
    message_port::unhold_for_window(rt, posted.record.ports());
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
        message_port::release_ports(rt, posted.record.ports());
        return Ok(());
    }
    // The spec fires `messageerror` when the data cannot be deserialized.
    // The record is made in this runtime, so only a heap limit fails here.
    let realm = crate::state::doc_realm(rt)?;
    let Ok((data, ports)) = deserialize_with_ports(rt, realm, &posted.record) else {
        // The ports of the record are not delivered: close them.
        message_port::release_ports(rt, posted.record.ports());
        return Err(VmError::type_error(
            "a posted message could not be deserialized",
        ));
    };
    b::keep(rt, data);
    let window = crate::state::window_proxy(rt)?;
    let source = crate::state::window_proxy_of(rt, posted.source)?;
    let other = Other::Message {
        data,
        origin: posted.origin,
        last_event_id: String::new(),
        source: Some(source),
        ports,
        frozen: None,
    };
    fire_at_object(
        rt,
        window,
        g::MESSAGE_EVENT,
        "message",
        false,
        Extra::Other(Box::new(other)),
    )
    .map(drop)
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
