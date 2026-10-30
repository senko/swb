//! `FormData` (XHR Standard §6.2, <https://xhr.spec.whatwg.org/#interface-formdata>),
//! `FormDataEvent` and the `multipart/form-data` body.
//!
//! The entry list of `new FormData(form)` and of a form submission comes
//! from the engine ([`crate::EngineHost::form_entries`], the same function
//! that builds the submission), and fires the `formdata` event at the form
//! (HTML "constructing the entry list",
//! <https://html.spec.whatwg.org/multipage/form-control-infrastructure.html#constructing-the-form-data-set>):
//! a listener may change the list. A `FormData` made inside the event
//! throws `InvalidStateError`.
//!
//! A `File` entry is a `File` object (a `Blob` is wrapped when it is
//! appended: name `blob`). The multipart body follows Chromium
//! (measured, `tools/probes/web-blob-formdata.json`): the boundary is
//! `----WebKitFormBoundary` and 16 letters or digits; a name and a file name
//! escape `"`, CR and LF as `%22`, `%0D` and `%0A`; the text values and
//! names have CR LF line breaks (HTML "multipart/form-data encoding
//! algorithm"). Encoding checks the time every 256 entries and refuses a
//! body larger than [`MAX_BODY_BYTES`].

use swb_js::{Gc, GenericData, HostData, Object, Runtime, Tracer};

use super::{BlobBytes, BlobData, FileInfo, blob_of, blob_platform, new_blob_object, now_millis};
use crate::bindings::{self as b, Cx, Exception, NodeId, Platform};
use crate::events::{DispatchTarget, EventInterface, TrustedEvent};
use crate::generated as g;
use crate::generated::types::FileOrUsvString;
use crate::host::{FormDataEntry, FormDataValue};
use crate::state::{WebState, engine_mut, state_mut};

/// The longest multipart body.
pub(crate) const MAX_BODY_BYTES: u64 = 64 << 20;

/// The most entries of one `FormData`.
const MAX_ENTRIES: usize = 1 << 20;

/// An entry of a `FormData`: a name and a string or a `File` object.
#[derive(Debug)]
pub(crate) struct FormEntry {
    pub(crate) name: String,
    pub(crate) value: FormEntryValue,
}

/// The value of an entry.
#[derive(Debug)]
pub(crate) enum FormEntryValue {
    Text(String),
    /// A `File` object.
    File(Gc<Object>),
}

/// The data of a `FormData` object.
#[derive(Debug, Default)]
pub(crate) struct FormDataState {
    pub(crate) entries: Vec<FormEntry>,
}

impl GenericData for FormDataState {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for entry in &self.entries {
            if let FormEntryValue::File(file) = &entry.value {
                tracer.object(*file);
            }
        }
    }

    fn heap_size(&self) -> usize {
        64 + self
            .entries
            .iter()
            .map(|e| {
                48 + e.name.capacity()
                    + match &e.value {
                        FormEntryValue::Text(t) => t.capacity(),
                        FormEntryValue::File(_) => 0,
                    }
            })
            .sum::<usize>()
    }
}

/// The state of the `FormData` object `object`.
pub(crate) fn state_of(rt: &mut Runtime, object: Gc<Object>) -> b::Result<&mut FormDataState> {
    let illegal = || Exception::type_error("Illegal invocation");
    let Some((_, HostData::Generic(handle))) = rt.host_object(object.into()) else {
        return Err(illegal());
    };
    rt.heap_mut()
        .generic_mut::<FormDataState>(handle)
        .map_err(|_| illegal())
}

fn state_p(rt: &mut Runtime, this: Platform) -> b::Result<&mut FormDataState> {
    state_of(rt, this.object)
}

/// True if `value` is a `FormData` object.
pub(crate) fn is_form_data(rt: &Runtime, value: swb_js::Value) -> bool {
    rt.implements(value, g::FORM_DATA)
}

/// A new `FormData` object with `entries`.
pub(crate) fn new_form_data(
    rt: &mut Runtime,
    realm: u32,
    entries: Vec<FormEntry>,
) -> b::Result<Gc<Object>> {
    let handle = rt.alloc_host_data(FormDataState { entries })?;
    Ok(rt.new_host_object(realm, g::FORM_DATA, handle)?)
}

/// The File entry for `blob` (`FormData` "create an entry"): a `File` with
/// the bytes and type of the blob and the name `filename`, else the name
/// of a `File`, else `blob`. A `File` without a new name is the entry
/// itself.
fn file_entry(cx: &mut Cx<'_>, blob: Platform, filename: Option<String>) -> b::Result<Gc<Object>> {
    let data = blob_platform(cx.rt, blob)?;
    if data.file.is_some() && filename.is_none() {
        return Ok(blob.object);
    }
    let name = filename.unwrap_or_else(|| {
        data.file
            .as_ref()
            .map_or_else(|| "blob".to_owned(), |f| f.name.clone())
    });
    let last_modified = data
        .file
        .as_ref()
        .map_or_else(now_millis, |f| f.last_modified);
    let copy = BlobData {
        bytes: data.bytes.clone(),
        kind: data.kind.clone(),
        file: Some(FileInfo {
            name,
            last_modified,
        }),
    };
    let realm = cx.realm();
    new_blob_object(cx.rt, realm, copy)
}

fn push(rt: &mut Runtime, this: Platform, entry: FormEntry) -> b::Result<()> {
    let state = state_p(rt, this)?;
    if state.entries.len() >= MAX_ENTRIES {
        return Err(Exception::range_error("The FormData has too many entries."));
    }
    state.entries.push(entry);
    Ok(())
}

/// `FormData` "set an entry": replaces the first entry of the name and drops
/// the others, else appends.
fn set_entry(rt: &mut Runtime, this: Platform, entry: FormEntry) -> b::Result<()> {
    let state = state_p(rt, this)?;
    match state.entries.iter().position(|e| e.name == entry.name) {
        Some(first) => {
            let name = entry.name.clone();
            state.entries[first] = entry;
            let mut seen = 0usize;
            state.entries.retain(|e| {
                if e.name == name {
                    seen += 1;
                    seen == 1
                } else {
                    true
                }
            });
            Ok(())
        }
        None => push(rt, this, entry),
    }
}

fn entry_value(entry: &FormEntry, rt: &Runtime) -> FileOrUsvString {
    match &entry.value {
        FormEntryValue::Text(text) => FileOrUsvString::UsvString(text.clone()),
        FormEntryValue::File(file) => {
            let data = rt
                .host_object((*file).into())
                .map_or(HostData::Word(0), |(_, d)| d);
            FileOrUsvString::File(Platform {
                object: *file,
                data,
            })
        }
    }
}

impl g::form_data::FormDataImpl for WebState {
    fn constructor(
        cx: &mut Cx<'_>,
        form: Option<NodeId>,
        submitter: Option<NodeId>,
    ) -> b::Result<Gc<Object>> {
        let entries = match form {
            Some(form) => {
                crate::html::activation::check_submitter(cx, form, submitter)?;
                let realm = cx.realm();
                match build_entry_list(cx.rt, realm, form, submitter)? {
                    Some(entries) => entries,
                    None => {
                        return Err(Exception::dom(
                            "InvalidStateError",
                            "The form is already being constructed.",
                        ));
                    }
                }
            }
            None => Vec::new(),
        };
        let handle = cx.rt.alloc_host_data(FormDataState { entries })?;
        Ok(cx.new_instance::<WebState>(handle)?)
    }

    fn append_1(cx: &mut Cx<'_>, this: Platform, name: String, value: String) -> b::Result<()> {
        push(
            cx.rt,
            this,
            FormEntry {
                name,
                value: FormEntryValue::Text(value),
            },
        )
    }

    fn append_2(
        cx: &mut Cx<'_>,
        this: Platform,
        name: String,
        blob_value: Platform,
        filename: Option<String>,
    ) -> b::Result<()> {
        let file = file_entry(cx, blob_value, filename)?;
        push(
            cx.rt,
            this,
            FormEntry {
                name,
                value: FormEntryValue::File(file),
            },
        )
    }

    fn delete(cx: &mut Cx<'_>, this: Platform, name: String) -> b::Result<()> {
        state_p(cx.rt, this)?.entries.retain(|e| e.name != name);
        Ok(())
    }

    fn get(cx: &mut Cx<'_>, this: Platform, name: String) -> b::Result<Option<FileOrUsvString>> {
        let state = state_p(cx.rt, this)?;
        let Some(position) = state.entries.iter().position(|e| e.name == name) else {
            return Ok(None);
        };
        let value = match &state.entries[position].value {
            FormEntryValue::Text(text) => FileOrUsvString::UsvString(text.clone()),
            FormEntryValue::File(file) => {
                let file = *file;
                return Ok(Some(file_value(cx.rt, file)));
            }
        };
        Ok(Some(value))
    }

    fn get_all(cx: &mut Cx<'_>, this: Platform, name: String) -> b::Result<Vec<FileOrUsvString>> {
        let state = state_p(cx.rt, this)?;
        let mut values = Vec::new();
        let mut files = Vec::new();
        for (index, entry) in state.entries.iter().enumerate() {
            if entry.name != name {
                continue;
            }
            match &entry.value {
                FormEntryValue::Text(text) => {
                    values.push((index, FileOrUsvString::UsvString(text.clone())));
                }
                FormEntryValue::File(file) => files.push((index, *file)),
            }
        }
        for (index, file) in files {
            values.push((index, file_value(cx.rt, file)));
        }
        values.sort_by_key(|(index, _)| *index);
        Ok(values.into_iter().map(|(_, v)| v).collect())
    }

    fn has(cx: &mut Cx<'_>, this: Platform, name: String) -> b::Result<bool> {
        Ok(state_p(cx.rt, this)?.entries.iter().any(|e| e.name == name))
    }

    fn set_1(cx: &mut Cx<'_>, this: Platform, name: String, value: String) -> b::Result<()> {
        set_entry(
            cx.rt,
            this,
            FormEntry {
                name,
                value: FormEntryValue::Text(value),
            },
        )
    }

    fn set_2(
        cx: &mut Cx<'_>,
        this: Platform,
        name: String,
        blob_value: Platform,
        filename: Option<String>,
    ) -> b::Result<()> {
        let file = file_entry(cx, blob_value, filename)?;
        set_entry(
            cx.rt,
            this,
            FormEntry {
                name,
                value: FormEntryValue::File(file),
            },
        )
    }

    fn pair_at(
        cx: &mut Cx<'_>,
        this: Platform,
        index: u32,
    ) -> b::Result<Option<(String, FileOrUsvString)>> {
        let state = state_p(cx.rt, this)?;
        let Some(entry) = state.entries.get(index as usize) else {
            return Ok(None);
        };
        let name = entry.name.clone();
        let value = match &entry.value {
            FormEntryValue::Text(text) => FileOrUsvString::UsvString(text.clone()),
            FormEntryValue::File(file) => {
                let file = *file;
                file_value(cx.rt, file)
            }
        };
        Ok(Some((name, value)))
    }
}

/// The `File` entry value as the glue wants it.
fn file_value(rt: &Runtime, file: Gc<Object>) -> FileOrUsvString {
    let entry = FormEntry {
        name: String::new(),
        value: FormEntryValue::File(file),
    };
    entry_value(&entry, rt)
}

/// The entry list of `form` (HTML "constructing the entry list"): the
/// engine's entries and the `formdata` event; `None` if the form is
/// already being constructed.
pub(crate) fn build_entry_list(
    rt: &mut Runtime,
    realm: u32,
    form: NodeId,
    submitter: Option<NodeId>,
) -> b::Result<Option<Vec<FormEntry>>> {
    if !state_mut(rt)?.constructing_forms.insert(form) {
        return Ok(None);
    }
    let result = construct(rt, realm, form, submitter);
    state_mut(rt)?.constructing_forms.remove(&form);
    result.map(Some)
}

fn construct(
    rt: &mut Runtime,
    realm: u32,
    form: NodeId,
    submitter: Option<NodeId>,
) -> b::Result<Vec<FormEntry>> {
    let host_entries = engine_mut(rt)?.form_entries(form, submitter, (0, 0));
    let entries = host_entries_to_objects(rt, realm, host_entries)?;
    let form_data = new_form_data(rt, realm, entries)?;
    let event = TrustedEvent {
        bubbles: true,
        interface: EventInterface::FormData { form_data },
        ..TrustedEvent::new("formdata")
    };
    crate::events::fire_trusted(rt, DispatchTarget::Node(form), &event)?;
    // The listeners may have changed the list; the object is the result.
    let state = state_of(rt, form_data)?;
    Ok(std::mem::take(&mut state.entries))
}

/// The entries of the engine as `FormData` entries.
fn host_entries_to_objects(
    rt: &mut Runtime,
    realm: u32,
    host_entries: Vec<FormDataEntry>,
) -> b::Result<Vec<FormEntry>> {
    let mut entries = Vec::with_capacity(host_entries.len());
    for entry in host_entries {
        let value = match entry.value {
            FormDataValue::Text(text) => FormEntryValue::Text(text),
            FormDataValue::File {
                filename,
                content_type,
                data,
            } => FormEntryValue::File(new_blob_object(
                rt,
                realm,
                BlobData {
                    bytes: BlobBytes::from_vec(data),
                    kind: content_type,
                    file: Some(FileInfo {
                        name: filename,
                        last_modified: now_millis(),
                    }),
                },
            )?),
        };
        entries.push(FormEntry {
            name: entry.name,
            value,
        });
    }
    Ok(entries)
}

/// The entries of the submission of `form`, after the `formdata` event
/// (HTML "submit": the entry list is constructed once). `None` if the
/// listeners left the list as the engine built it, so the engine builds
/// the submission itself.
pub(crate) fn submission_entries(
    rt: &mut Runtime,
    form: NodeId,
    submitter: Option<NodeId>,
    coordinate: (i32, i32),
) -> b::Result<Option<Vec<FormDataEntry>>> {
    let before = engine_mut(rt)?.form_entries(form, submitter, coordinate);
    if !state_mut(rt)?.constructing_forms.insert(form) {
        return Ok(None);
    }
    let result = changed_entries(rt, form, &before);
    state_mut(rt)?.constructing_forms.remove(&form);
    result
}

/// Fires `formdata` at `form` with the entries `before` and returns the
/// entries if a listener changed them. The objects are made in realm 0, the
/// document's (swb has one realm).
fn changed_entries(
    rt: &mut Runtime,
    form: NodeId,
    before: &[FormDataEntry],
) -> b::Result<Option<Vec<FormDataEntry>>> {
    let entries = host_entries_to_objects(rt, 0, before.to_vec())?;
    let form_data = new_form_data(rt, 0, entries)?;
    let event = TrustedEvent {
        bubbles: true,
        interface: EventInterface::FormData { form_data },
        ..TrustedEvent::new("formdata")
    };
    crate::events::fire_trusted(rt, DispatchTarget::Node(form), &event)?;
    let after = to_host_entries(rt, form_data)?;
    Ok((after != before).then_some(after))
}

/// The entries of a `FormData` as the engine's entries.
fn to_host_entries(rt: &mut Runtime, form_data: Gc<Object>) -> b::Result<Vec<FormDataEntry>> {
    let list: Vec<(String, Result<String, Gc<Object>>)> = state_of(rt, form_data)?
        .entries
        .iter()
        .map(|e| {
            (
                e.name.clone(),
                match &e.value {
                    FormEntryValue::Text(t) => Ok(t.clone()),
                    FormEntryValue::File(f) => Err(*f),
                },
            )
        })
        .collect();
    let mut out = Vec::with_capacity(list.len());
    for (name, value) in list {
        let value = match value {
            Ok(text) => FormDataValue::Text(text),
            Err(file) => {
                let Some(data) = blob_of(rt, file.into()) else {
                    continue;
                };
                FormDataValue::File {
                    filename: data
                        .file
                        .as_ref()
                        .map(|f| f.name.clone())
                        .unwrap_or_default(),
                    content_type: data.kind.clone(),
                    data: data.bytes.to_vec(),
                }
            }
        };
        out.push(FormDataEntry { name, value });
    }
    Ok(out)
}

// ----- multipart/form-data -----

/// The boundary of a body: Chromium's `----WebKitFormBoundary` and 16
/// letters and digits.
fn boundary() -> b::Result<String> {
    const ALPHABET: &[u8; 62] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut random = [0u8; 16];
    crate::crypto::fill(&mut random)?;
    let mut out = String::from("----WebKitFormBoundary");
    for byte in random {
        out.push(char::from(ALPHABET[usize::from(byte) % ALPHABET.len()]));
    }
    Ok(out)
}

/// A name or file name inside the quotes of a `Content-Disposition`.
fn escape(name: &str) -> String {
    name.replace('\r', "%0D")
        .replace('\n', "%0A")
        .replace('"', "%22")
}

/// A text with CR LF line breaks (HTML "multipart/form-data encoding
/// algorithm": names and values are normalized).
fn crlf(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str("\r\n");
            }
            '\n' => out.push_str("\r\n"),
            c => out.push(c),
        }
    }
    out
}

/// The `multipart/form-data` body of `form_data` (a `FormData` object) and
/// its `Content-Type`.
pub(crate) fn multipart_body(
    rt: &mut Runtime,
    form_data: Gc<Object>,
) -> b::Result<(Vec<u8>, String)> {
    let boundary = boundary()?;
    let items: Vec<(String, Result<String, Gc<Object>>)> = state_of(rt, form_data)?
        .entries
        .iter()
        .map(|e| {
            (
                e.name.clone(),
                match &e.value {
                    FormEntryValue::Text(t) => Ok(t.clone()),
                    FormEntryValue::File(f) => Err(*f),
                },
            )
        })
        .collect();
    let mut out: Vec<u8> = Vec::new();
    for (index, (name, value)) in items.into_iter().enumerate() {
        if index % 256 == 0 {
            rt.check_time()?;
        }
        out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        let name = escape(&crlf(&name));
        match value {
            Ok(text) => {
                out.extend_from_slice(
                    format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
                );
                out.extend_from_slice(crlf(&text).as_bytes());
            }
            Err(file) => {
                let Some(data) = blob_of(rt, file.into()) else {
                    continue;
                };
                let filename = data
                    .file
                    .as_ref()
                    .map(|f| f.name.clone())
                    .unwrap_or_default();
                let kind = if data.kind.is_empty() {
                    "application/octet-stream".to_owned()
                } else {
                    data.kind.clone()
                };
                if out.len() as u64 + data.bytes.len() > MAX_BODY_BYTES {
                    return Err(Exception::range_error("The FormData body is too large."));
                }
                out.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{name}\"; filename=\"{}\"\r\nContent-Type: {kind}\r\n\r\n",
                        escape(&filename)
                    )
                    .as_bytes(),
                );
                for piece in &data.bytes.pieces {
                    out.extend_from_slice(piece.bytes());
                }
            }
        }
        out.extend_from_slice(b"\r\n");
        if out.len() as u64 > MAX_BODY_BYTES {
            return Err(Exception::range_error("The FormData body is too large."));
        }
    }
    out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok((out, format!("multipart/form-data; boundary={boundary}")))
}

// ----- Parsing a body into a FormData (`Response.formData()`) -----

/// The entries of an `application/x-www-form-urlencoded` body.
pub(crate) fn urlencoded_entries(text: &str) -> Vec<FormEntry> {
    url::form_urlencoded::parse(text.as_bytes())
        .map(|(name, value)| FormEntry {
            name: name.into_owned(),
            value: FormEntryValue::Text(value.into_owned()),
        })
        .collect()
}

/// The position of `needle` in `haystack` at or after `from`.
fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() || from > haystack.len() {
        return None;
    }
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// The value of the parameter `key` in a `Content-Disposition` value.
fn disposition_param(line: &str, key: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    let pattern = format!("{key}=\"");
    let at = lower.find(&pattern)? + pattern.len();
    let rest = line.get(at..)?;
    let value = rest.split('"').next()?;
    Some(
        value
            .replace("%0D", "\r")
            .replace("%0A", "\n")
            .replace("%22", "\""),
    )
}

/// The entries of a `multipart/form-data` body with `boundary` (HTML
/// "multipart/form-data decoding" in the simple form that browsers write:
/// parts with a `Content-Disposition` `name` and maybe a `filename`);
/// `None` if the body is malformed.
pub(crate) fn parse_multipart(
    rt: &mut Runtime,
    realm: u32,
    bytes: &[u8],
    boundary: &str,
) -> b::Result<Option<Vec<FormEntry>>> {
    let delimiter = format!("--{boundary}").into_bytes();
    let separator = format!("\r\n--{boundary}").into_bytes();
    let Some(first) = find(bytes, &delimiter, 0) else {
        return Ok(None);
    };
    let mut at = first + delimiter.len();
    let mut entries = Vec::new();
    loop {
        if entries.len() % 256 == 255 {
            rt.check_time()?;
        }
        if bytes.get(at..at + 2) == Some(b"--") {
            return Ok(Some(entries));
        }
        if bytes.get(at..at + 2) != Some(b"\r\n") {
            return Ok(None);
        }
        at += 2;
        let Some(head_end) = find(bytes, b"\r\n\r\n", at) else {
            return Ok(None);
        };
        let head = String::from_utf8_lossy(&bytes[at..head_end]).into_owned();
        let body_start = head_end + 4;
        let Some(body_end) = find(bytes, &separator, head_end + 2) else {
            return Ok(None);
        };
        let body = bytes
            .get(body_start..body_end.max(body_start))
            .unwrap_or(&[]);
        at = body_end + separator.len();
        let mut name = None;
        let mut filename = None;
        let mut kind = String::new();
        for line in head.split("\r\n") {
            let lower = line.to_ascii_lowercase();
            if lower.starts_with("content-disposition:") {
                name = disposition_param(line, "name");
                filename = disposition_param(line, "filename");
            } else if let Some(value) = lower.strip_prefix("content-type:") {
                value.trim().clone_into(&mut kind);
            }
        }
        let Some(name) = name else {
            return Ok(None);
        };
        let value = match filename {
            Some(filename) => FormEntryValue::File(new_blob_object(
                rt,
                realm,
                BlobData {
                    bytes: BlobBytes::from_vec(body.to_vec()),
                    kind,
                    file: Some(FileInfo {
                        name: filename,
                        last_modified: now_millis(),
                    }),
                },
            )?),
            None => FormEntryValue::Text(String::from_utf8_lossy(body).into_owned()),
        };
        entries.push(FormEntry { name, value });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_needle_is_found_from_a_position() {
        assert_eq!(find(b"abcabc", b"bc", 0), Some(1));
        assert_eq!(find(b"abcabc", b"bc", 2), Some(4));
        assert_eq!(find(b"abc", b"x", 0), None);
    }

    #[test]
    fn a_disposition_parameter() {
        let line = "Content-Disposition: form-data; name=\"a%22b\"; filename=\"f.txt\"";
        assert_eq!(disposition_param(line, "name").as_deref(), Some("a\"b"));
        assert_eq!(
            disposition_param(line, "filename").as_deref(),
            Some("f.txt")
        );
    }

    #[test]
    fn names_are_escaped_and_lines_are_crlf() {
        assert_eq!(escape("a\"b\r\nc"), "a%22b%0D%0Ac");
        assert_eq!(crlf("a\nb\rc\r\nd"), "a\r\nb\r\nc\r\nd");
    }
}
