//! The `storage` event (HTML §12.2.1 "the storage event",
//! <https://html.spec.whatwg.org/multipage/webstorage.html#the-storage-event>; ADR 0031
//! section 3 and "Done (feature 14b)").
//!
//! A change of an area (`setItem` with another value, `removeItem` of a key that is
//! there, `clear` of a full area) queues one task for every other document of the
//! runtime with the same [`crate::StorageKey`] and the same kind: the writer gets
//! none, and a document of another key gets none, also when its origin is the same
//! (measured, cases `storage-event-*`). Documents of one key have one origin, and
//! the runtimes of a page are one per origin (ADR 0031 section 5), so all of them
//! are in the runtime of the writer; a document of that origin in another runtime
//! cannot exist. The event has `key`, `oldValue` and `newValue` (`null` for `clear`
//! and for a missing value), `url` (the URL of the document whose `Storage` object
//! changed) and `storageArea` (the receiver's own `Storage` object), and it is
//! trusted, does not bubble and cannot be canceled. A change that does not change the
//! area fires nothing.
//!
//! The task has a due time a little after the write, and the sequence number of the
//! write. Chromium delivers through its storage service, so the event comes after the
//! tasks that the writer queued later with a delay of 0 ms (a message, a timer of 0 ms)
//! and before a timer of 1 ms (measured, case `storage-event-order`); swb runs it
//! [`DELAY_MS`] after the write. The listeners are those of the window when the task
//! runs, so a listener that the writer adds after the write gets the event. The task of
//! a document that retired does not run, and its queue goes with it. A document that a
//! navigation replaces gets the events that wait for it first
//! ([`crate::ScriptRuntime::flush_storage_events`]): in Chromium the event is faster than
//! the commit (measured, case `storage-event-retire`). A listener cannot hold up the commit
//! with new events: the flush fires what waits at its start, for a bounded time.
//!
//! Bounds (ADR 0031 section 11): each event that waits counts in the message budget of
//! the runtime that wrote it (`message_port.rs`: 65,536 waiting and 256 MiB, and the
//! runtimes of a page's frames count their bytes together), with the bytes of its
//! strings once for each receiver. A write past the bound fires nothing at the
//! receivers that it has no room for, with one warning until nothing waits. A
//! runtime with one document has no receivers, and a write pays one comparison.

use std::collections::VecDeque;
use std::rc::Rc;

use swb_js::{Gc, Object, Runtime, Value, VmError, VmResult};

use crate::bindings as b;
use crate::event_loop::{Task, page_now};
use crate::events::{Extra, fire_at_object, misc::Other};
use crate::generated as g;
use crate::message_port::Charge;
use crate::state::{DocKey, document_mut, engine, state, state_mut};
use crate::storage::{StorageKey, StorageKind};

/// The time after a write at which its events run, in milliseconds: after a task that
/// the writer queued with no delay, before one with a delay of 1 ms.
pub(crate) const DELAY_MS: f64 = 0.5;

/// What changed in an area.
#[derive(Debug)]
pub(crate) enum Change {
    /// `setItem` gave `key` the value `new`; `old` was there or not.
    Set {
        key: String,
        old: Option<String>,
        new: String,
    },
    /// `removeItem` removed `key`, which had the value `old`.
    Remove { key: String, old: Option<String> },
    /// `clear` removed the keys.
    Clear,
}

/// The key, the old value and the new value of an event.
type Texts = (Option<Rc<str>>, Option<Rc<str>>, Option<Rc<str>>);

impl Change {
    /// The bytes of the strings of the event.
    fn bytes(&self) -> usize {
        let len = |text: &Option<String>| text.as_ref().map_or(0, String::len);
        match self {
            Change::Set { key, old, new } => key.len() + len(old) + new.len(),
            Change::Remove { key, old } => key.len() + len(old),
            Change::Clear => 0,
        }
    }

    /// The strings of the event, to share among the receivers.
    fn into_texts(self) -> Texts {
        match self {
            Change::Set { key, old, new } => {
                (Some(Rc::from(key)), old.map(Rc::from), Some(Rc::from(new)))
            }
            Change::Remove { key, old } => (Some(Rc::from(key)), old.map(Rc::from), None),
            Change::Clear => (None, None, None),
        }
    }
}

/// An event that waits for its task.
#[derive(Debug)]
struct Waiting {
    kind: StorageKind,
    key: Option<Rc<str>>,
    old: Option<Rc<str>>,
    new: Option<Rc<str>>,
    url: Rc<str>,
    charge: Charge,
}

/// The events that wait for a document, in the order of their tasks.
#[derive(Debug, Default)]
pub(crate) struct StorageEvents {
    queue: VecDeque<Waiting>,
}

/// True if another document of the runtime may need an event: a runtime with one
/// document has none.
pub(crate) fn has_peers(rt: &Runtime) -> b::Result<bool> {
    Ok(state(rt)?.docs.len() > 1)
}

/// The number of events that wait for the current document.
pub(crate) fn waiting(rt: &Runtime) -> usize {
    state(rt).map_or(0, |state| state.doc.storage_events.queue.len())
}

/// Queues the events of `change` of the area `area` of `kind`, written through the
/// `Storage` object of the current document, for the other documents of its key.
pub(crate) fn queue(
    rt: &mut Runtime,
    kind: StorageKind,
    area: &Rc<StorageKey>,
    change: Change,
) -> b::Result<()> {
    // The writer is the current document, whose record `docs` does not hold.
    let receivers: Vec<DocKey> = state(rt)?
        .docs
        .iter()
        .flatten()
        .filter(|doc| !doc.retired && doc.forward.is_none())
        .filter(|doc| matches!(&doc.storage_key, Some(Ok(key)) if **key == **area))
        .map(|doc| doc.key)
        .collect();
    if receivers.is_empty() {
        return Ok(());
    }
    let url = engine(rt)?.url();
    let bytes = change.bytes() + url.len();
    let admit = |rt: &Runtime| state(rt).ok()?.ports.admit_as(bytes, "storage event");
    // A budget that is full costs no copy of the strings.
    let mut first = admit(rt);
    if first.is_none() {
        return Ok(());
    }
    let url: Rc<str> = Rc::from(url);
    let (key, old, new) = change.into_texts();
    let due = page_now(rt) + DELAY_MS;
    for receiver in receivers {
        let Some(charge) = first.take().or_else(|| admit(rt)) else {
            break;
        };
        let state = state_mut(rt)?;
        let seq = state.event_loop.tasks.take_seq();
        let Some(record) = document_mut(state, receiver) else {
            continue;
        };
        record.storage_events.queue.push_back(Waiting {
            kind,
            key: key.clone(),
            old: old.clone(),
            new: new.clone(),
            url: Rc::clone(&url),
            charge,
        });
        state
            .event_loop
            .tasks
            .push_for(receiver, due, seq, Task::Storage);
    }
    Ok(())
}

/// The task: fires the first waiting event of the current document at its window.
pub(crate) fn deliver(rt: &mut Runtime) -> VmResult<()> {
    let Some(mut waiting) = state_mut(rt)?.doc.storage_events.queue.pop_front() else {
        return Ok(());
    };
    waiting.charge.settle();
    let realm = crate::state::doc_realm(rt)?;
    let area: Gc<Object> = match crate::storage::storage_object_in(rt, realm, waiting.kind) {
        Ok(area) => area,
        Err(b::Exception::Js(error @ (VmError::Terminated(_) | VmError::Internal(_)))) => {
            return Err(error);
        }
        // The document has no storage now: it was not a receiver.
        Err(_) => return Ok(()),
    };
    b::keep(rt, Value::Object(area));
    let window = crate::state::window_proxy(rt)?;
    let text = |text: Option<Rc<str>>| text.map(|text| text.to_string());
    let other = Other::Storage {
        key: text(waiting.key),
        old_value: text(waiting.old),
        new_value: text(waiting.new),
        url: waiting.url.to_string(),
        storage_area: Some(area),
    };
    fire_at_object(
        rt,
        window,
        g::STORAGE_EVENT,
        "storage",
        false,
        Extra::Other(Box::new(other)),
    )
    .map(drop)
}
