//! `FileReader` (File API §6,
//! <https://w3c.github.io/FileAPI/#APIASynch>).
//!
//! A read takes a snapshot of the blob (the pieces, not a copy) and queues
//! two tasks, as Chromium does (measured): the first fires `loadstart`; the
//! second fires `progress` (one event, with the whole length), then
//! sets the result and the state `DONE` and fires `load` and `loadend`.
//! `abort()` during a read sets the state `DONE`, the error `AbortError` and
//! fires `abort` and `loadend`; the queued tasks then do nothing (each read
//! has a generation number). The result is made in the second task: UTF-8
//! (or the label's) decode, base64 for a data URL, or a copy into an
//! `ArrayBuffer`; the work checks the script's time.
//!
//! Deviation: the progress event is single (Chromium fires one per 50 ms
//! chunk for large blobs); `readAsText` decodes with `encoding_rs`, whose
//! labels are the Encoding Standard's.

use swb_js::{Gc, Object, Runtime, Tracer};

use super::{BlobBytes, blob_platform};
use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::event_loop::Task;
use crate::events::TargetData;
use crate::generated as g;
use crate::generated::types::DomStringOrObject;
use crate::state::{WebState, state_mut};

/// What a read makes of the bytes.
#[derive(Clone, Debug)]
enum Kind {
    ArrayBuffer,
    BinaryString,
    /// The encoding label, if the caller gave one.
    Text(Option<String>),
    /// The type of the blob for the data URL.
    DataUrl(String),
}

/// The result of a read.
#[derive(Debug)]
enum Output {
    Text(String),
    Buffer(Gc<Object>),
}

/// The state of a `FileReader` (in the `TargetData` of the object).
#[derive(Debug, Default)]
pub(crate) struct ReaderState {
    /// `readyState`.
    ready_state: u16,
    result: Option<Output>,
    error: Option<Gc<Object>>,
    /// The current read: its snapshot and what to make. The snapshot holds
    /// its vectors in `WebState::held_blobs` unless `refused`.
    read: Option<(BlobBytes, Kind)>,
    /// True when the held bytes had no room for the snapshot: the read
    /// fails in its task (its snapshot is empty).
    refused: bool,
    /// The id in `WebState::readers` while a read is in progress.
    id: u64,
    /// Counts the reads and aborts, so that a queued task of an old read
    /// does nothing.
    generation: u64,
}

impl ReaderState {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        if let Some(Output::Buffer(buffer)) = &self.result {
            tracer.object(*buffer);
        }
        if let Some(error) = self.error {
            tracer.object(error);
        }
    }

    pub(crate) fn heap_size(&self) -> usize {
        64 + match &self.result {
            Some(Output::Text(t)) => t.capacity(),
            _ => 0,
        }
    }
}

/// Runs `change` on the state of the `FileReader` `object` and charges the
/// change of its size to the heap at once (`Heap::generic_resized`).
fn reader_resize<R>(
    rt: &mut Runtime,
    object: Gc<Object>,
    change: impl FnOnce(&mut ReaderState) -> R,
) -> b::Result<R> {
    let illegal = || Exception::type_error("Illegal invocation");
    let Some((_, swb_js::HostData::Generic(handle))) = rt.host_object(object.into()) else {
        return Err(illegal());
    };
    rt.heap_mut()
        .generic_resized::<TargetData, _>(handle, |data| data.reader.as_deref_mut().map(change))
        .map_err(|_| illegal())?
        .ok_or_else(illegal)
}

fn reader_mut(rt: &mut Runtime, object: Gc<Object>) -> b::Result<&mut ReaderState> {
    let illegal = || Exception::type_error("Illegal invocation");
    let Some((_, swb_js::HostData::Generic(handle))) = rt.host_object(object.into()) else {
        return Err(illegal());
    };
    rt.heap_mut()
        .generic_mut::<TargetData>(handle)
        .map_err(|_| illegal())?
        .reader
        .as_deref_mut()
        .ok_or_else(illegal)
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | (u32::from(b) << (16 - 8 * i)));
        let sextet = |shift: u32| char::from(TABLE[((n >> shift) & 63) as usize]);
        out.push(sextet(18));
        out.push(sextet(12));
        out.push(if chunk.len() > 1 { sextet(6) } else { '=' });
        out.push(if chunk.len() > 2 { sextet(0) } else { '=' });
    }
    out
}

/// Starts a read of `blob` (File API "readOperation").
fn start(cx: &mut Cx<'_>, this: Platform, blob: Platform, kind: Kind) -> b::Result<()> {
    let bytes = blob_platform(cx.rt, blob)?.bytes.clone();
    let kind = match kind {
        Kind::DataUrl(_) => Kind::DataUrl(blob_platform(cx.rt, blob)?.kind.clone()),
        other => other,
    };
    if reader_mut(cx.rt, this.object)?.ready_state == 1 {
        return Err(Exception::dom(
            "InvalidStateError",
            "The object is already busy reading Blobs.",
        ));
    }
    // The page may drop the Blob while the read waits for its task.
    let refused = !state_mut(cx.rt)?.held_blobs.hold(&bytes);
    let bytes = if refused { BlobBytes::default() } else { bytes };
    let (generation, id) = {
        let state = reader_mut(cx.rt, this.object)?;
        state.ready_state = 1;
        state.result = None;
        state.error = None;
        state.read = Some((bytes, kind));
        state.refused = refused;
        state.generation += 1;
        (state.generation, state.id)
    };
    let id = if id == 0 {
        let app = state_mut(cx.rt)?;
        app.next_reader += 1;
        let id = app.next_reader;
        reader_mut(cx.rt, this.object)?.id = id;
        id
    } else {
        id
    };
    let app = state_mut(cx.rt)?;
    app.readers.insert(id, this.object);
    let due = app.clock.page_now();
    app.event_loop.tasks.push(
        due,
        Task::FileReader {
            id,
            generation,
            finish: false,
        },
    );
    Ok(())
}

/// A queued task of a read: `finish` false fires `loadstart`, true the rest.
pub(crate) fn reader_task(
    rt: &mut Runtime,
    id: u64,
    generation: u64,
    finish: bool,
) -> swb_js::VmResult<()> {
    let Some(object) = state_mut(rt)?.readers.get(&id).copied() else {
        return Ok(());
    };
    // The reader leaves the table before its last events; it lives as long
    // as the task's scope (the page may hold it only through its handlers).
    rt.heap_mut().record(object);
    let Ok(state) = reader_mut(rt, object) else {
        return Ok(());
    };
    if state.generation != generation || state.ready_state != 1 {
        return Ok(());
    }
    let total = state.read.as_ref().map_or(0, |(b, _)| b.len());
    if !finish {
        crate::events::misc::fire_progress(rt, object, "loadstart", true, 0, total)?;
        let app = state_mut(rt)?;
        let due = app.clock.page_now();
        app.event_loop.tasks.push(
            due,
            Task::FileReader {
                id,
                generation,
                finish: true,
            },
        );
        return Ok(());
    }
    crate::events::misc::fire_progress(rt, object, "progress", true, total, total)?;
    // A listener may have aborted or restarted the read.
    let Ok(state) = reader_mut(rt, object) else {
        return Ok(());
    };
    if state.generation != generation || state.ready_state != 1 {
        return Ok(());
    }
    let Some((bytes, kind)) = state.read.take() else {
        return Ok(());
    };
    if std::mem::take(&mut state.refused) {
        let error = Exception::range_error("Too many bytes wait for reads.");
        return read_failed(rt, object, id, total, error);
    }
    let output = make_result(rt, &bytes, &kind);
    state_mut(rt)?.held_blobs.release(&bytes);
    drop(bytes);
    let output = match output {
        Ok(output) => output,
        Err(error) => return read_failed(rt, object, id, total, error),
    };
    // The result counts against the heap as soon as the state holds it.
    reader_resize(rt, object, |state| {
        state.result = Some(output);
        state.ready_state = 2;
    })
    .map_err(crate::fetch::exception_error)?;
    state_mut(rt)?.readers.remove(&id);
    crate::events::misc::fire_progress(rt, object, "load", true, total, total)?;
    crate::events::misc::fire_progress(rt, object, "loadend", true, total, total)
}

/// The read failed (the result does not fit in the heap): the failure steps
/// of File API "readOperation" (state `DONE`, `error` with a
/// `NotReadableError`, then `error` and `loadend`). A termination of the
/// script passes on after the reader is cleaned up.
fn read_failed(
    rt: &mut Runtime,
    object: Gc<Object>,
    id: u64,
    total: u64,
    error: Exception,
) -> swb_js::VmResult<()> {
    if let Ok(state) = reader_mut(rt, object) {
        state.ready_state = 2;
        state.read = None;
    }
    let app = state_mut(rt)?;
    app.readers.remove(&id);
    if !app.reader_warned {
        app.reader_warned = true;
        log::warn!(
            "a FileReader read failed (later failures of the document are not logged): {error:?}"
        );
    }
    let error = crate::fetch::exception_error(error);
    if matches!(error, swb_js::VmError::Terminated(_)) {
        return Err(error);
    }
    let dom = b::dom_exception::create(
        rt,
        0,
        g::DOM_EXCEPTION,
        "NotReadableError",
        "The requested file could not be read.",
    )?;
    if let Ok(state) = reader_mut(rt, object) {
        state.error = Some(dom);
    }
    crate::events::misc::fire_progress(rt, object, "error", true, 0, total)?;
    crate::events::misc::fire_progress(rt, object, "loadend", true, 0, total)
}

/// The result of a read.
fn make_result(rt: &mut Runtime, bytes: &BlobBytes, kind: &Kind) -> b::Result<Output> {
    rt.check_time()?;
    // The copy and the result (text takes up to twice the bytes) must fit in
    // the heap before the work starts.
    let wanted = usize::try_from(bytes.len()).map_or(usize::MAX, |l| l.saturating_mul(2));
    if rt.heap().check_room(wanted).is_err() {
        return Err(Exception::range_error(
            "The result does not fit in the heap.",
        ));
    }
    let data = bytes.to_vec();
    Ok(match kind {
        Kind::ArrayBuffer => Output::Buffer(rt.create_array_buffer(0, data, None)?),
        Kind::BinaryString => Output::Text(data.iter().map(|&b| char::from(b)).collect()),
        Kind::Text(label) => {
            let encoding = label
                .as_deref()
                .and_then(|l| encoding_rs::Encoding::for_label(l.trim().as_bytes()))
                .unwrap_or(encoding_rs::UTF_8);
            Output::Text(encoding.decode(&data).0.into_owned())
        }
        Kind::DataUrl(blob_type) => {
            let blob_type = if blob_type.is_empty() {
                "application/octet-stream"
            } else {
                blob_type
            };
            Output::Text(format!("data:{blob_type};base64,{}", base64(&data)))
        }
    })
}

impl g::file_reader::FileReaderImpl for WebState {
    fn constructor(cx: &mut Cx<'_>) -> b::Result<Gc<Object>> {
        let mut data = TargetData::default();
        data.reader = Some(Box::new(ReaderState::default()));
        let handle = cx.rt.alloc_host_data(data)?;
        Ok(cx.new_instance::<WebState>(handle)?)
    }

    fn ready_state(cx: &mut Cx<'_>, this: Platform) -> b::Result<u16> {
        Ok(reader_mut(cx.rt, this.object)?.ready_state)
    }

    fn result(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<DomStringOrObject>> {
        Ok(match &reader_mut(cx.rt, this.object)?.result {
            None => None,
            Some(Output::Text(text)) => Some(DomStringOrObject::DomString(text.clone())),
            Some(Output::Buffer(buffer)) => Some(DomStringOrObject::Object(*buffer)),
        })
    }

    fn error(cx: &mut Cx<'_>, this: Platform) -> b::Result<Option<Gc<Object>>> {
        Ok(reader_mut(cx.rt, this.object)?.error)
    }

    fn read_as_array_buffer(cx: &mut Cx<'_>, this: Platform, blob: Platform) -> b::Result<()> {
        start(cx, this, blob, Kind::ArrayBuffer)
    }

    fn read_as_binary_string(cx: &mut Cx<'_>, this: Platform, blob: Platform) -> b::Result<()> {
        start(cx, this, blob, Kind::BinaryString)
    }

    fn read_as_text(
        cx: &mut Cx<'_>,
        this: Platform,
        blob: Platform,
        encoding: Option<String>,
    ) -> b::Result<()> {
        start(cx, this, blob, Kind::Text(encoding))
    }

    fn read_as_data_url(cx: &mut Cx<'_>, this: Platform, blob: Platform) -> b::Result<()> {
        start(cx, this, blob, Kind::DataUrl(String::new()))
    }

    fn abort(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        let realm = cx.realm();
        let (id, total) = {
            let state = reader_mut(cx.rt, this.object)?;
            if state.ready_state != 1 {
                return Ok(());
            }
            let total = state.read.as_ref().map_or(0, |(b, _)| b.len());
            let read = state.read.take();
            if !std::mem::take(&mut state.refused)
                && let Some((bytes, _)) = &read
            {
                state_mut(cx.rt)?.held_blobs.release(bytes);
            }
            let state = reader_mut(cx.rt, this.object)?;
            state.ready_state = 2;
            state.result = None;
            state.generation += 1;
            (state.id, total)
        };
        let error = b::dom_exception::create(
            cx.rt,
            realm,
            g::DOM_EXCEPTION,
            "AbortError",
            "An ongoing operation was aborted, typically with a call to abort().",
        )?;
        reader_mut(cx.rt, this.object)?.error = Some(error);
        state_mut(cx.rt)?.readers.remove(&id);
        crate::events::misc::fire_progress(cx.rt, this.object, "abort", true, 0, total)?;
        crate::events::misc::fire_progress(cx.rt, this.object, "loadend", true, 0, total)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn base64_pads() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"hi"), "aGk=");
        assert_eq!(base64(b"a"), "YQ==");
        assert_eq!(base64(b"abc"), "YWJj");
    }
}
