//! `Blob`, `File` and Blob URLs (M8 feature 10b, ADR 0028 section 12; File
//! API, <https://w3c.github.io/FileAPI/>), and the parts of the feature that
//! build on them: `FormData` (`form_data.rs`), `FileReader` (`reader.rs`).
//!
//! # Data
//!
//! A blob is a list of pieces: slices of immutable, shared byte vectors
//! ([`BlobBytes`]). A blob made of blobs, and a slice, copy the list, not
//! the bytes, so a loop that doubles a blob or slices it again and again
//! costs memory in proportion to the list, not to the length. A list longer
//! than [`BlobBytes::piece_limit`] (1,024 pieces, plus one per KiB of the
//! last flat copy, so that many small parts cost a copy of 1 KiB each, not a
//! copy of the whole blob each time) is flattened into one vector (one copy
//! of at most [`MAX_BLOB_BYTES`]).
//!
//! The data of a blob object ([`BlobData`]) charges the sizes of the
//! distinct vectors that it holds ([`BlobBytes::backing_len`]) against the
//! script heap (`GenericData::heap_size`), not its logical length: a slice
//! keeps its parent's whole vector alive. A slice that is less than half of
//! the vectors it uses (and they are more than [`SLICE_COPY_MIN`]) is
//! copied instead, so a small slice never holds a large parent; the 256 MiB
//! heap limit then bounds the memory of the live blobs. Several blobs that
//! share a vector each charge it (the estimate is high, never low).
//!
//! The bytes of a blob that reaches the script (`text()`,
//! `arrayBuffer()`, a `FileReader`) are copied into the heap, which charges
//! them again.
//!
//! # Blob URLs
//!
//! `URL.createObjectURL` stores the bytes and the type in the binding state
//! ([`BlobUrls`], at most [`MAX_BLOB_URLS`] URLs and [`MAX_BLOB_URL_BYTES`]
//! bytes that the entries keep alive); `revokeObjectURL` removes the entry. The store lives as
//! long as the document. `fetch()` and `XMLHttpRequest` load a `blob:` URL
//! of the document's origin from the store (`fetch/mod.rs`); `img` elements
//! do not (departure: the engine's image loader has no `blob:` scheme).
//!
//! A request of a Blob URL and a `FileReader` read keep a snapshot of the
//! blob until their task runs, also when the page drops the blob.
//! [`HeldBlobs`] bounds the vectors that they keep alive to
//! [`MAX_HELD_BYTES`]: past it a request is a network error and a read
//! fails with `error`.
//!
//! # Deviations
//!
//! - `Blob.stream()` is missing (no streams, ADR 0028 section 12).
//! - A blob is at most [`MAX_BLOB_BYTES`] long; a larger one throws a
//!   `RangeError` (Chromium allocates until the browser's quota).
//! - `File` objects of a file input cannot be made (no file picker).

mod form_data;
mod reader;

use std::collections::HashMap;
use std::rc::Rc;

use swb_js::{Gc, GenericData, HostData, Object, Runtime, Slots, Tracer, Value};

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;
use crate::generated::types::{BlobPropertyBag, EndingType, FilePropertyBag};
use crate::state::{WebState, state_mut};

pub(crate) use form_data::{
    is_form_data, multipart_body, new_form_data, parse_multipart, submission_entries,
    urlencoded_entries,
};
pub(crate) use reader::{ReaderState, reader_task};

/// The longest blob: a larger one throws a `RangeError`.
pub(crate) const MAX_BLOB_BYTES: u64 = 64 << 20;

/// The most pieces in the list of a blob before it is flattened (see
/// [`BlobBytes::piece_limit`]).
const MAX_PIECES: usize = 1024;

/// A slice copies its bytes when they are less than half of the vectors it
/// uses and these are longer than this.
const SLICE_COPY_MIN: u64 = 64 << 10;

/// The most Blob URLs of a document.
pub(crate) const MAX_BLOB_URLS: usize = 16_384;

/// The most bytes that the Blob URLs of a document keep alive (the sizes of
/// the vectors, [`BlobBytes::backing_len`]).
pub(crate) const MAX_BLOB_URL_BYTES: u64 = 256 << 20;

/// A slice of a shared byte vector.
#[derive(Clone, Debug)]
struct Piece {
    data: Rc<Vec<u8>>,
    start: usize,
    end: usize,
}

impl Piece {
    fn bytes(&self) -> &[u8] {
        self.data.get(self.start..self.end).unwrap_or(&[])
    }
}

/// The bytes of a blob: a list of slices of shared vectors.
#[derive(Clone, Debug, Default)]
pub(crate) struct BlobBytes {
    pieces: Vec<Piece>,
    len: u64,
    /// The length at the last flattening (the list may grow by one piece per
    /// KiB of it before the next).
    flat_len: u64,
}

impl BlobBytes {
    /// A blob of one vector.
    pub(crate) fn from_vec(bytes: Vec<u8>) -> BlobBytes {
        let len = bytes.len();
        if len == 0 {
            return BlobBytes::default();
        }
        BlobBytes {
            pieces: vec![Piece {
                data: Rc::new(bytes),
                start: 0,
                end: len,
            }],
            len: len as u64,
            flat_len: 0,
        }
    }

    /// The length in bytes.
    pub(crate) fn len(&self) -> u64 {
        self.len
    }

    /// The sum of the lengths of the distinct vectors that the pieces
    /// use: what the blob keeps alive (at least its length).
    pub(crate) fn backing_len(&self) -> u64 {
        match self.pieces.as_slice() {
            [] => 0,
            [one] => one.data.len() as u64,
            _ => self.vectors().iter().map(|v| v.1).sum(),
        }
    }

    /// The distinct vectors that the pieces use: their addresses and
    /// lengths.
    fn vectors(&self) -> Vec<(usize, u64)> {
        let mut vectors: Vec<(usize, u64)> = self
            .pieces
            .iter()
            .map(|p| (Rc::as_ptr(&p.data) as usize, p.data.len() as u64))
            .collect();
        vectors.sort_unstable_by_key(|v| v.0);
        vectors.dedup_by_key(|v| v.0);
        vectors
    }

    /// The most pieces before the list is flattened.
    fn piece_limit(&self) -> usize {
        MAX_PIECES + usize::try_from(self.flat_len >> 10).unwrap_or(usize::MAX)
    }

    /// The bytes copied into one vector.
    pub(crate) fn to_vec(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.len as usize);
        for piece in &self.pieces {
            out.extend_from_slice(piece.bytes());
        }
        out
    }

    /// Appends the pieces of `other`; `None` if the result passes
    /// [`MAX_BLOB_BYTES`].
    pub(crate) fn append(&mut self, other: &BlobBytes) -> Option<()> {
        if self.len + other.len > MAX_BLOB_BYTES {
            return None;
        }
        for piece in &other.pieces {
            match self.pieces.last_mut() {
                Some(last) if Rc::ptr_eq(&last.data, &piece.data) && last.end == piece.start => {
                    last.end = piece.end;
                }
                _ => self.pieces.push(piece.clone()),
            }
        }
        self.len += other.len;
        if self.pieces.len() > self.piece_limit() {
            let mut flat = BlobBytes::from_vec(self.to_vec());
            flat.flat_len = flat.len;
            *self = flat;
        }
        Some(())
    }

    /// The bytes from `start` to `end` (both within the length, `start <=
    /// end`).
    pub(crate) fn slice(&self, start: u64, end: u64) -> BlobBytes {
        let mut out = BlobBytes::default();
        let mut offset = 0u64;
        for piece in &self.pieces {
            let piece_len = (piece.end - piece.start) as u64;
            let (from, to) = (offset, offset + piece_len);
            offset = to;
            if to <= start {
                continue;
            }
            if from >= end {
                break;
            }
            let first = start.saturating_sub(from) as usize;
            let last = (end.min(to) - from) as usize;
            out.pieces.push(Piece {
                data: piece.data.clone(),
                start: piece.start + first,
                end: piece.start + last,
            });
            out.len += (last - first) as u64;
        }
        // A small slice of a large vector copies its bytes.
        let backing = out.backing_len();
        if backing > SLICE_COPY_MIN && backing / 2 > out.len {
            return BlobBytes::from_vec(out.to_vec());
        }
        out
    }
}

/// The name and the modification time of a `File`.
#[derive(Clone, Debug)]
pub(crate) struct FileInfo {
    pub(crate) name: String,
    pub(crate) last_modified: i64,
}

/// The data of a `Blob` or `File` object.
#[derive(Clone, Debug)]
pub(crate) struct BlobData {
    pub(crate) bytes: BlobBytes,
    /// `type`.
    pub(crate) kind: String,
    /// Set for a `File`.
    pub(crate) file: Option<FileInfo>,
}

impl GenericData for BlobData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        64 + usize::try_from(self.bytes.backing_len()).unwrap_or(usize::MAX)
            + self.bytes.pieces.len() * 40
            + self.kind.capacity()
            + self.file.as_ref().map_or(0, |f| f.name.capacity())
    }
}

/// The data of the `Blob` or `File` object `value`, if it is one.
pub(crate) fn blob_of(rt: &Runtime, value: Value) -> Option<&BlobData> {
    if !rt.implements(value, g::BLOB) {
        return None;
    }
    match rt.host_object(value)?.1 {
        HostData::Generic(handle) => rt.heap().generic::<BlobData>(handle).ok(),
        HostData::Word(_) => None,
    }
}

/// The data of the platform object `blob` of an operation.
fn blob_platform(rt: &Runtime, blob: Platform) -> b::Result<&BlobData> {
    match blob.data {
        HostData::Generic(handle) => rt
            .heap()
            .generic::<BlobData>(handle)
            .map_err(|_| Exception::type_error("Illegal invocation")),
        HostData::Word(_) => Err(Exception::type_error("Illegal invocation")),
    }
}

/// A new `Blob` (or `File`, with `data.file`) object.
pub(crate) fn new_blob_object(
    rt: &mut Runtime,
    realm: u32,
    data: BlobData,
) -> b::Result<Gc<Object>> {
    let class = if data.file.is_some() {
        g::FILE
    } else {
        g::BLOB
    };
    let handle = rt.alloc_host_data(data)?;
    Ok(rt.new_host_object(realm, class, handle)?)
}

/// The type of a blob: the string if it has only characters from U+0020 to
/// U+007E, in lower case, else empty (File API "constructor").
fn normalize_type(kind: &str) -> String {
    if kind.chars().all(|c| matches!(c, '\u{20}'..='\u{7e}')) {
        kind.to_ascii_lowercase()
    } else {
        String::new()
    }
}

/// The text of a string part: with `endings: "native"`, line breaks become
/// LF (the native ending on Linux).
fn convert_endings(text: &str, native: bool) -> String {
    if !native {
        return text.to_owned();
    }
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// The bytes of the parts of a `Blob` or `File` constructor (File API
/// "process blob parts").
fn process_parts(rt: &mut Runtime, parts: Value, native: bool) -> b::Result<BlobBytes> {
    if parts.is_undefined() {
        return Ok(BlobBytes::default());
    }
    let items = b::convert::sequence(rt, parts, |_, v| Ok(v))?;
    let mut out = BlobBytes::default();
    for (index, item) in items.into_iter().enumerate() {
        if index % 256 == 0 {
            rt.check_time()?;
        }
        let part = part_bytes(rt, item, native)?;
        out.append(&part)
            .ok_or_else(|| Exception::range_error("The Blob is too large."))?;
    }
    Ok(out)
}

/// The bytes of one part: a blob, an `ArrayBuffer` or a view, or the string
/// of the value.
fn part_bytes(rt: &mut Runtime, item: Value, native: bool) -> b::Result<BlobBytes> {
    if let Some(blob) = blob_of(rt, item) {
        return Ok(blob.bytes.clone());
    }
    if let Value::Object(object) = item
        && let Some(bytes) = buffer_bytes(rt, object)?
    {
        return Ok(BlobBytes::from_vec(bytes));
    }
    let text = b::convert::usv_string(rt, item)?;
    Ok(BlobBytes::from_vec(
        convert_endings(&text, native).into_bytes(),
    ))
}

/// The bytes of an `ArrayBuffer` or a view, or `None` for another object.
pub(crate) fn buffer_bytes(rt: &mut Runtime, object: Gc<Object>) -> b::Result<Option<Vec<u8>>> {
    let (buffer, offset, length) = match rt.slots_of(object)? {
        Slots::ArrayBuffer { byte_length, .. } => (object, 0, byte_length),
        Slots::View {
            buffer,
            byte_offset,
            byte_length,
            ..
        } => {
            let length = match byte_length {
                Some(length) => length,
                None => match rt.slots_of(buffer)? {
                    Slots::ArrayBuffer { byte_length, .. } => {
                        byte_length.saturating_sub(byte_offset)
                    }
                    _ => 0,
                },
            };
            (buffer, byte_offset, length)
        }
        _ => return Ok(None),
    };
    Ok(Some(rt.buffer_range(buffer, offset, length)?))
}

/// A `Blob` object with `bytes` and the type `kind` (already normalized).
pub(crate) fn make_blob(
    rt: &mut Runtime,
    realm: u32,
    bytes: Vec<u8>,
    kind: &str,
) -> b::Result<Gc<Object>> {
    new_blob_object(
        rt,
        realm,
        BlobData {
            bytes: BlobBytes::from_vec(bytes),
            kind: normalize_type(kind),
            file: None,
        },
    )
}

/// A resolved promise with `value`.
fn resolved(cx: &mut Cx<'_>, value: Value) -> b::Result<Gc<Object>> {
    let realm = cx.realm();
    match cx.rt.resolved_promise(realm, value)? {
        Value::Object(promise) => Ok(promise),
        _ => Err(Exception::type_error("PromiseResolve gave a non-object")),
    }
}

/// Clamps a `slice` offset (File API "slice blob"): negative values count
/// from the end.
fn relative(value: Option<i64>, size: u64, default: u64) -> u64 {
    match value {
        None => default,
        Some(v) if v < 0 => size.saturating_sub(v.unsigned_abs()),
        Some(v) => (v as u64).min(size),
    }
}

impl g::blob::BlobImpl for WebState {
    fn size(cx: &mut Cx<'_>, this: Platform) -> b::Result<u64> {
        Ok(blob_platform(cx.rt, this)?.bytes.len())
    }

    fn type_(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(blob_platform(cx.rt, this)?.kind.clone())
    }

    fn constructor(
        cx: &mut Cx<'_>,
        blob_parts: Value,
        options: BlobPropertyBag,
    ) -> b::Result<Gc<Object>> {
        let bytes = process_parts(
            cx.rt,
            blob_parts,
            matches!(options.endings, EndingType::Native),
        )?;
        let data = BlobData {
            bytes,
            kind: normalize_type(&options.type_),
            file: None,
        };
        let handle = cx.rt.alloc_host_data(data)?;
        Ok(cx.new_instance::<WebState>(handle)?)
    }

    fn slice(
        cx: &mut Cx<'_>,
        this: Platform,
        start: Option<i64>,
        end: Option<i64>,
        content_type: Option<String>,
    ) -> b::Result<Gc<Object>> {
        let blob = blob_platform(cx.rt, this)?;
        let size = blob.bytes.len();
        let from = relative(start, size, 0);
        let to = relative(end, size, size);
        let bytes = if from < to {
            blob.bytes.slice(from, to)
        } else {
            BlobBytes::default()
        };
        let kind = normalize_type(&content_type.unwrap_or_default());
        let realm = cx.realm();
        new_blob_object(
            cx.rt,
            realm,
            BlobData {
                bytes,
                kind,
                file: None,
            },
        )
    }

    fn text(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let bytes = blob_platform(cx.rt, this)?.bytes.to_vec();
        cx.rt.check_time()?;
        let text = crate::fetch::utf8_text(&bytes);
        let value = b::to_js::string(cx.rt, &text)?;
        resolved(cx, value)
    }

    fn array_buffer(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let bytes = blob_platform(cx.rt, this)?.bytes.to_vec();
        let realm = cx.realm();
        let buffer = cx.rt.create_array_buffer(realm, bytes, None)?;
        resolved(cx, buffer.into())
    }

    fn bytes(cx: &mut Cx<'_>, this: Platform) -> b::Result<Gc<Object>> {
        let bytes = blob_platform(cx.rt, this)?.bytes.to_vec();
        let length = bytes.len();
        let realm = cx.realm();
        let buffer = cx.rt.create_array_buffer(realm, bytes, None)?;
        let view = cx
            .rt
            .create_view(realm, Some("Uint8"), buffer, 0, Some(length))?;
        resolved(cx, view.into())
    }
}

/// The current time in milliseconds since the epoch (`File.lastModified`).
fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

impl g::file::FileImpl for WebState {
    fn name(cx: &mut Cx<'_>, this: Platform) -> b::Result<String> {
        Ok(blob_platform(cx.rt, this)?
            .file
            .as_ref()
            .map(|f| f.name.clone())
            .unwrap_or_default())
    }

    fn last_modified(cx: &mut Cx<'_>, this: Platform) -> b::Result<i64> {
        Ok(blob_platform(cx.rt, this)?
            .file
            .as_ref()
            .map_or(0, |f| f.last_modified))
    }

    fn webkit_relative_path(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(String::new())
    }

    fn constructor(
        cx: &mut Cx<'_>,
        file_bits: Value,
        file_name: String,
        options: FilePropertyBag,
    ) -> b::Result<Gc<Object>> {
        let bytes = process_parts(
            cx.rt,
            file_bits,
            matches!(options.endings, EndingType::Native),
        )?;
        let data = BlobData {
            bytes,
            kind: normalize_type(&options.type_),
            file: Some(FileInfo {
                name: file_name,
                last_modified: options.last_modified.unwrap_or_else(now_millis),
            }),
        };
        let handle = cx.rt.alloc_host_data(data)?;
        Ok(cx.new_instance::<WebState>(handle)?)
    }
}

// ----- Blob URLs -----

/// A Blob URL's entry: the bytes and the type.
#[derive(Debug)]
struct BlobUrlEntry {
    bytes: BlobBytes,
    kind: String,
    /// The bytes that the entry keeps alive ([`BlobBytes::backing_len`]),
    /// counted against [`MAX_BLOB_URL_BYTES`].
    charge: u64,
}

/// The Blob URL store of a document.
#[derive(Debug, Default)]
pub(crate) struct BlobUrls {
    entries: HashMap<String, BlobUrlEntry>,
    bytes: u64,
}

impl BlobUrls {
    /// The bytes and the type of the entry for `url`.
    pub(crate) fn get(&self, url: &str) -> Option<(BlobBytes, String)> {
        let url = url.split('#').next().unwrap_or(url);
        self.entries
            .get(url)
            .map(|e| (e.bytes.clone(), e.kind.clone()))
    }
}

/// The origin part of a Blob URL for the document at `document_url`.
fn blob_origin(document_url: &str) -> String {
    match swb_net::Url::parse(document_url) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => url.origin().ascii_serialization(),
        _ => "null".to_owned(),
    }
}

/// `URL.createObjectURL(blob)`.
pub(crate) fn create_object_url(cx: &mut Cx<'_>, obj: Platform) -> b::Result<String> {
    {
        let blob = blob_platform(cx.rt, obj)?;
        let (bytes, kind) = (blob.bytes.clone(), blob.kind.clone());
        let charge = bytes.backing_len();
        let document_url = crate::scripts::document_url(cx.rt);
        let uuid = crate::crypto::random_uuid()?;
        let url = format!("blob:{}/{uuid}", blob_origin(&document_url));
        let urls = &mut state_mut(cx.rt)?.blob_urls;
        if urls.entries.len() >= MAX_BLOB_URLS || urls.bytes + charge > MAX_BLOB_URL_BYTES {
            log::warn!("a document has too many Blob URLs");
            return Err(Exception::dom(
                "QuotaExceededError",
                "Too many Blob URLs are in use.",
            ));
        }
        urls.bytes += charge;
        urls.entries.insert(
            url.clone(),
            BlobUrlEntry {
                bytes,
                kind,
                charge,
            },
        );
        Ok(url)
    }
}

/// `URL.revokeObjectURL(url)`.
pub(crate) fn revoke_object_url(cx: &mut Cx<'_>, url: &str) -> b::Result<()> {
    let urls = &mut state_mut(cx.rt)?.blob_urls;
    let key = url.split('#').next().unwrap_or(url);
    if let Some(entry) = urls.entries.remove(key) {
        urls.bytes -= entry.charge;
    }
    Ok(())
}

// ----- Blobs held by waiting operations -----

/// The most bytes that the operations waiting for their task keep alive
/// ([`HeldBlobs`]).
pub(crate) const MAX_HELD_BYTES: u64 = 256 << 20;

/// The vectors that the operations waiting for their task hold: requests of
/// Blob URLs (`fetch/mod.rs`) and `FileReader` reads (`reader.rs`). The page
/// can drop the blob while the operation waits, and then the heap no longer
/// counts its bytes; this count bounds them. Each vector counts once,
/// however many operations hold it.
#[derive(Debug, Default)]
pub(crate) struct HeldBlobs {
    /// By the address of the vector: the number of holds and the length.
    /// Each hold keeps a clone of the blob, so the vector stays alive and
    /// its address is not reused while it has an entry.
    vectors: HashMap<usize, (usize, u64)>,
    bytes: u64,
    /// True after the warning about a refused hold.
    warned: bool,
}

impl HeldBlobs {
    /// Takes a hold on the vectors of `blob`. False, with one warning per
    /// document, when the held bytes would pass [`MAX_HELD_BYTES`].
    pub(crate) fn hold(&mut self, blob: &BlobBytes) -> bool {
        let vectors = blob.vectors();
        let added: u64 = vectors
            .iter()
            .filter(|(address, _)| !self.vectors.contains_key(address))
            .map(|(_, len)| *len)
            .sum();
        if self.bytes + added > MAX_HELD_BYTES {
            if !self.warned {
                self.warned = true;
                log::warn!(
                    "Blob URL requests and FileReader reads wait with more than {MAX_HELD_BYTES} bytes; a request or read fails"
                );
            }
            return false;
        }
        for (address, len) in vectors {
            self.vectors.entry(address).or_insert((0, len)).0 += 1;
        }
        self.bytes += added;
        true
    }

    /// Gives up a hold that [`HeldBlobs::hold`] took on the vectors of
    /// `blob`.
    pub(crate) fn release(&mut self, blob: &BlobBytes) {
        for (address, len) in blob.vectors() {
            let Some(entry) = self.vectors.get_mut(&address) else {
                continue;
            };
            entry.0 = entry.0.saturating_sub(1);
            if entry.0 == 0 {
                self.vectors.remove(&address);
                self.bytes = self.bytes.saturating_sub(len);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(bytes: &BlobBytes) -> String {
        String::from_utf8(bytes.to_vec()).unwrap_or_default()
    }

    #[test]
    fn slices_share_the_bytes() {
        let blob = BlobBytes::from_vec(b"abcdef".to_vec());
        let middle = blob.slice(1, 5);
        assert_eq!(text(&middle), "bcde");
        assert_eq!(text(&middle.slice(1, 3)), "cd");
        assert_eq!(middle.len(), 4);
        assert!(Rc::ptr_eq(&blob.pieces[0].data, &middle.pieces[0].data));
    }

    #[test]
    fn held_blobs_count_each_vector_once() {
        let mut held = HeldBlobs::default();
        let big = BlobBytes::from_vec(vec![0; 100 << 20]);
        let other = BlobBytes::from_vec(vec![0; 100 << 20]);
        // The same vector twice (also through a slice that shares it).
        assert!(held.hold(&big));
        assert!(held.hold(&big.slice(0, 90 << 20)));
        assert_eq!(held.bytes, 100 << 20);
        assert!(held.hold(&other));
        assert_eq!(held.bytes, 200 << 20);
        // A third vector of 100 MiB passes 256 MiB.
        let third = BlobBytes::from_vec(vec![0; 100 << 20]);
        assert!(!held.hold(&third));
        assert_eq!(held.bytes, 200 << 20);
        // A vector counts until its last hold is given up.
        held.release(&big);
        assert_eq!(held.bytes, 200 << 20);
        held.release(&big);
        assert_eq!(held.bytes, 100 << 20);
        assert!(held.hold(&third));
        held.release(&other);
        held.release(&third);
        assert_eq!(held.bytes, 0);
        assert!(held.vectors.is_empty());
    }

    #[test]
    fn appended_slices_of_one_vector_merge() {
        let blob = BlobBytes::from_vec(b"abcdef".to_vec());
        let mut joined = blob.slice(0, 2);
        joined.append(&blob.slice(2, 6));
        assert_eq!(joined.pieces.len(), 1);
        assert_eq!(text(&joined), "abcdef");
    }

    #[test]
    fn doubling_is_flattened_and_bounded() {
        let mut blob = BlobBytes::from_vec(vec![1u8; 1000]);
        let mut size = 1000u64;
        loop {
            let copy = blob.clone();
            if blob.append(&copy).is_none() {
                break;
            }
            size *= 2;
            assert!(blob.pieces.len() <= blob.piece_limit());
            assert_eq!(blob.len(), size);
        }
        assert!(blob.len() <= MAX_BLOB_BYTES);
    }

    #[test]
    fn a_small_slice_of_a_large_vector_is_copied() {
        let blob = BlobBytes::from_vec(vec![7u8; 1 << 20]);
        let tiny = blob.slice(0, 1);
        assert_eq!(tiny.len(), 1);
        assert_eq!(tiny.backing_len(), 1);
        assert!(!Rc::ptr_eq(&blob.pieces[0].data, &tiny.pieces[0].data));
        // More than half of the vector: shared, and charged for the vector.
        let big = blob.slice(0, 700_000);
        assert!(Rc::ptr_eq(&blob.pieces[0].data, &big.pieces[0].data));
        assert_eq!(big.backing_len(), 1 << 20);
        // A small vector is shared whatever the slice.
        let small = BlobBytes::from_vec(vec![1u8; 100]);
        assert_eq!(small.slice(0, 1).backing_len(), 100);
    }

    #[test]
    fn the_backing_counts_each_vector_once() {
        let a = BlobBytes::from_vec(vec![1u8; 1000]);
        let b = BlobBytes::from_vec(vec![2u8; 500]);
        let mut joined = BlobBytes::default();
        for part in [&a, &b, &a, &b] {
            joined.append(part);
        }
        assert_eq!(joined.len(), 3000);
        assert_eq!(joined.backing_len(), 1500);
    }

    #[test]
    fn many_small_parts_are_copied_in_proportion_to_their_number() {
        // 200,000 parts of 64 bytes: the copies of all the flattenings add
        // up to a small multiple of the length, not to its square.
        let part = BlobBytes::from_vec(vec![3u8; 64]);
        let mut blob = BlobBytes::default();
        let mut flattenings = 0;
        for _ in 0..200_000 {
            let before = blob.pieces.len();
            blob.append(&part);
            if blob.pieces.len() < before {
                flattenings += 1;
            }
        }
        assert_eq!(blob.len(), 200_000 * 64);
        assert!(flattenings < 400, "{flattenings} flattenings");
    }

    #[test]
    fn relative_offsets() {
        assert_eq!(relative(Some(-3), 8, 0), 5);
        assert_eq!(relative(Some(-30), 8, 0), 0);
        assert_eq!(relative(Some(30), 8, 0), 8);
        assert_eq!(relative(None, 8, 8), 8);
    }

    #[test]
    fn types_are_lowercased_ascii() {
        assert_eq!(normalize_type("Text/PLAIN"), "text/plain");
        assert_eq!(normalize_type("a\u{e9}"), "");
        assert_eq!(convert_endings("a\r\nb\rc", true), "a\nb\nc");
    }
}
