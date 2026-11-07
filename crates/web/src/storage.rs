//! Web Storage (HTML §12.2,
//! <https://html.spec.whatwg.org/multipage/webstorage.html>; ADR 0028
//! sections 12 and 16, ADR 0031 section 3): the data of `localStorage` and
//! `sessionStorage` and the `Storage` interface over it.
//!
//! The data lives outside the document, in a [`StorageStore`] that the
//! page owns and lends to each document ([`crate::EngineHost::storage`]):
//! both kinds are kept per [`StorageKey`] in memory, `localStorage` for the life
//! of the process and `sessionStorage` for the life of the page (one tab);
//! both survive navigations between documents.
//!
//! **The key** is the origin of the document and its partition. An origin is
//! the serialization of an `http:` or `https:` origin; all `file:` documents
//! share the origin `file://` (Chromium gives them one storage area). A
//! document with another URL has no storage: `data:` documents and the others
//! throw `SecurityError` with Chromium's texts. The partition
//! ([`StoragePartition`]) is empty for a top-level document and for a frame
//! whose ancestors are all of its site; a frame under another site, or under a
//! cross-site frame, has the site of the top-level document in its key, so a
//! third party has an area per top-level site (measured, cases
//! `storage-key-*`). A key that cannot be computed gives the `SecurityError` of
//! a document without storage: the storage is a boundary between sites, and a
//! document gets no area it cannot name. A `Storage` object belongs to its
//! document: after the document retires, every call throws `SecurityError`
//! (measured, case `storage-retired-write`).
//!
//! Limits, measured in Chromium:
//!
//! - The quota of an area is 5,242,880 UTF-16 code units of all keys and
//!   values together (so 10 MiB of memory in Chromium). The code units
//!   count, not the characters (an emoji is 2). A write that replaces a
//!   value counts the difference; a write of the same value changes
//!   nothing. The area of a key or a value of exactly the free size fits.
//!   A write past the quota throws `QuotaExceededError`; the area stays as
//!   it was.
//! - swb adds two bounds that Chromium does not have (ADR 0028 section
//!   16): 100,000 keys per area, because a quota in code units lets a page
//!   store millions of one-character keys, and a total of 64 MiB over all
//!   areas of both kinds and all keys, in bytes of memory (the UTF-8 bytes of
//!   keys and values, a fixed cost per key and per area, the keys), so that
//!   a session that visits many origins cannot grow without limit.
//!
//! The order of keys is the order of insertion, except that removing a key
//! moves the last key to its place. Chromium's order is that of a hash
//! table; the standard leaves the order to the user agent.
//!
//! Strings are UTF-8: a lone surrogate in a key or a value becomes U+FFFD
//! (the conversion of every `DOMString`, ADR 0028 section 3); Chromium
//! keeps lone surrogates.
//!
//! The `storage` event goes to the other documents of the key
//! (`storage_events.rs`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use swb_js::{Gc, GenericData, HostData, Object, Runtime, Tracer, VmError};

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::generated as g;
use crate::state::{WebState, engine, state, state_mut};

/// The quota of one area, in UTF-16 code units of keys and values.
pub const AREA_QUOTA_UNITS: usize = 5 * 1024 * 1024;

/// The most keys in one area.
pub const MAX_KEYS_PER_AREA: usize = 100_000;

/// The bound over all areas of both kinds, in bytes of memory: the UTF-8
/// bytes of keys and values, [`KEY_COST_BYTES`] for each key, and the
/// storage key plus [`AREA_COST_BYTES`] for each area.
pub const TOTAL_LIMIT_BYTES: usize = 64 * 1024 * 1024;

/// The cost of one key in the total, besides its bytes: the entry of the
/// area (two strings, the index).
pub const KEY_COST_BYTES: usize = 64;

/// The cost of one area in the total, besides its storage key: the maps.
pub const AREA_COST_BYTES: usize = 64;

/// The text of the `SecurityError` of a document that cannot name an area.
pub(crate) const DENIED: &str = "Access is denied for this document.";

/// The kind of an area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageKind {
    /// `localStorage`: for the life of the process.
    Local,
    /// `sessionStorage`: for the life of the page.
    Session,
}

/// A write that no area can take: past the quota or past a bound of swb.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuotaExceeded;

/// The part of a document's storage key that comes from the documents around it
/// ([`crate::EngineHost::storage_partition`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum StoragePartition {
    /// A top-level document, or a frame whose ancestors are all of its site: the
    /// key is the origin alone.
    #[default]
    FirstParty,
    /// A frame under another site, or under a frame of another site: the site of
    /// the top-level document, as text.
    Top(Rc<str>),
    /// The key cannot be computed (the top level has no site, for example a
    /// `data:` document): the document has no storage.
    Unavailable,
}

/// The key of a storage area: an origin and, for third-party content, the site of the
/// top-level document. One string (the origin; for a third party NUL and the site), so that a
/// lookup hashes and compares one string, as a lookup by origin did.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StorageKey {
    text: Rc<str>,
}

impl StorageKey {
    /// The key of a top-level document or of a first-party frame at `origin`.
    pub fn first_party(origin: &str) -> StorageKey {
        StorageKey {
            text: Rc::from(origin),
        }
    }

    /// The key of third-party content at `origin` under the top-level site `top`.
    pub fn third_party(origin: &str, top: &str) -> StorageKey {
        // An origin and a site have no NUL.
        StorageKey {
            text: Rc::from(format!("{origin}\0{top}")),
        }
    }

    /// The bytes of the key.
    fn len(&self) -> usize {
        self.text.len()
    }
}

/// The storage of a page: all areas of both kinds, by key. See the
/// module documentation.
#[derive(Debug, Default)]
pub struct StorageStore {
    local: HashMap<StorageKey, Area>,
    session: HashMap<StorageKey, Area>,
    /// The cost of all areas in bytes ([`TOTAL_LIMIT_BYTES`]).
    total: usize,
}

/// The handle of a [`StorageStore`] that the page and its documents share.
pub type SharedStorage = Rc<RefCell<StorageStore>>;

/// One area: the keys and values of a storage key in one kind.
#[derive(Debug, Default)]
struct Area {
    /// Insertion order.
    entries: Vec<(Rc<str>, String)>,
    /// The position of each key in `entries`.
    index: HashMap<Rc<str>, usize>,
    /// The code units of all keys and values.
    units: usize,
}

/// The number of UTF-16 code units of `text`.
fn units(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// The cost of an entry in the total.
fn entry_bytes(key: &str, value: &str) -> usize {
    key.len() + value.len() + KEY_COST_BYTES
}

/// The cost of an area without entries in the total.
fn area_bytes(key: &StorageKey) -> usize {
    key.len() + AREA_COST_BYTES
}

impl Area {
    /// Gives memory back after removals: the vector and the map keep their
    /// capacity otherwise, also when the keys are gone.
    fn shrink(&mut self) {
        if self.entries.len() * 4 < self.entries.capacity() && self.entries.capacity() > 64 {
            self.entries.shrink_to_fit();
            self.index.shrink_to_fit();
        }
    }
}

impl StorageStore {
    /// Creates a store without areas.
    pub fn new() -> Self {
        Self::default()
    }

    /// A new shared store.
    pub fn shared() -> SharedStorage {
        Rc::new(RefCell::new(Self::new()))
    }

    fn area(&self, kind: StorageKind, area_key: &StorageKey) -> Option<&Area> {
        match kind {
            StorageKind::Local => self.local.get(area_key),
            StorageKind::Session => self.session.get(area_key),
        }
    }

    fn areas_mut(&mut self, kind: StorageKind) -> &mut HashMap<StorageKey, Area> {
        match kind {
            StorageKind::Local => &mut self.local,
            StorageKind::Session => &mut self.session,
        }
    }

    /// The number of keys of the area.
    pub fn len(&self, kind: StorageKind, area: &StorageKey) -> usize {
        self.area(kind, area).map_or(0, |a| a.entries.len())
    }

    /// The key at `index` in the order of the area.
    pub fn key(&self, kind: StorageKind, area: &StorageKey, index: usize) -> Option<String> {
        let area = self.area(kind, area)?;
        area.entries.get(index).map(|(key, _)| key.to_string())
    }

    /// The value of `key`.
    pub fn get(&self, kind: StorageKind, area: &StorageKey, key: &str) -> Option<String> {
        let area = self.area(kind, area)?;
        let index = *area.index.get(key)?;
        area.entries.get(index).map(|(_, value)| value.clone())
    }

    /// True if the area has `key`.
    pub fn contains(&self, kind: StorageKind, area: &StorageKey, key: &str) -> bool {
        self.area(kind, area)
            .is_some_and(|a| a.index.contains_key(key))
    }

    /// The keys of the area, in order.
    pub fn keys(&self, kind: StorageKind, area: &StorageKey) -> Vec<String> {
        self.area(kind, area).map_or_else(Vec::new, |a| {
            a.entries.iter().map(|(key, _)| key.to_string()).collect()
        })
    }

    /// Sets `key` to `value`. Fails, and changes nothing, if the area would
    /// pass its quota, or a bound of swb.
    pub fn set(
        &mut self,
        kind: StorageKind,
        area_key: &StorageKey,
        key: &str,
        value: &str,
    ) -> Result<(), QuotaExceeded> {
        let old = self.area(kind, area_key).and_then(|area| {
            let index = *area.index.get(key)?;
            area.entries.get(index).map(|(_, old)| (index, old))
        });
        let new_units = units(value);
        // Units for the quota of the area, bytes for the total.
        let (grow, shrink, grow_bytes, shrink_bytes, new_key) = match old {
            // The same value: nothing changes (HTML `setItem` step 4).
            Some((_, old)) if old == value => return Ok(()),
            Some((_, old)) => (new_units, units(old), value.len(), old.len(), false),
            None => {
                let new_area = if self.area(kind, area_key).is_none() {
                    area_bytes(area_key)
                } else {
                    0
                };
                let bytes = entry_bytes(key, value) + new_area;
                (units(key) + new_units, 0, bytes, 0, true)
            }
        };
        let area_units = self.area(kind, area_key).map_or(0, |a| a.units);
        let area_keys = self.len(kind, area_key);
        if area_units - area_units.min(shrink) + grow > AREA_QUOTA_UNITS
            || (new_key && area_keys >= MAX_KEYS_PER_AREA)
            || self.total - self.total.min(shrink_bytes) + grow_bytes > TOTAL_LIMIT_BYTES
        {
            return Err(QuotaExceeded);
        }
        self.total = self.total - self.total.min(shrink_bytes) + grow_bytes;
        let area = self.areas_mut(kind).entry(area_key.clone()).or_default();
        area.units = area.units - area.units.min(shrink) + grow;
        if let Some(index) = area.index.get(key).copied() {
            if let Some(entry) = area.entries.get_mut(index) {
                // A new string, not `clone_into`: the old allocation would
                // stay at its size after a write of a shorter value.
                entry.1 = String::from(value);
            }
        } else {
            let key: Rc<str> = Rc::from(key);
            area.index.insert(Rc::clone(&key), area.entries.len());
            area.entries.push((key, value.to_owned()));
        }
        Ok(())
    }

    /// Removes `key`. True if it was there.
    pub fn remove(&mut self, kind: StorageKind, area_key: &StorageKey, key: &str) -> bool {
        let Some(area) = self.areas_mut(kind).get_mut(area_key) else {
            return false;
        };
        let Some(index) = area.index.remove(key) else {
            return false;
        };
        let (removed_key, removed_value) = area.entries.swap_remove(index);
        // The last entry moved to `index`.
        if let Some((moved, _)) = area.entries.get(index)
            && let Some(position) = area.index.get_mut(moved)
        {
            *position = index;
        }
        let freed = units(&removed_key) + units(&removed_value);
        area.units = area.units.saturating_sub(freed);
        area.shrink();
        let empty = area.entries.is_empty();
        let mut freed_bytes = entry_bytes(&removed_key, &removed_value);
        if empty {
            self.areas_mut(kind).remove(area_key);
            freed_bytes += area_bytes(area_key);
        }
        self.total = self.total.saturating_sub(freed_bytes);
        true
    }

    /// Removes all keys of the area. True if there were any.
    pub fn clear(&mut self, kind: StorageKind, area_key: &StorageKey) -> bool {
        let Some(area) = self.areas_mut(kind).remove(area_key) else {
            return false;
        };
        let cost: usize = area
            .entries
            .iter()
            .map(|(key, value)| entry_bytes(key, value))
            .sum();
        self.total = self.total.saturating_sub(cost + area_bytes(area_key));
        !area.entries.is_empty()
    }
}

/// The origin text of a document that has storage, or the text of the
/// `SecurityError` of one that has none.
fn origin_of(url: &str) -> Result<String, &'static str> {
    let Ok(parsed) = url::Url::parse(url) else {
        return Err(DENIED);
    };
    match parsed.scheme() {
        "http" | "https" => Ok(parsed.origin().ascii_serialization()),
        "file" => Ok("file://".to_owned()),
        "data" => Err("Storage is disabled inside 'data:' URLs."),
        _ => Err(DENIED),
    }
}

/// The key of the areas of the document of `host`, or the text of the `SecurityError`
/// that it throws: a sandbox without `allow-same-origin`, a URL without storage, a
/// partition that cannot be computed.
pub(crate) fn document_key(host: &dyn crate::EngineHost) -> Result<Rc<StorageKey>, &'static str> {
    if host.sandboxed_origin() {
        // Measured, case `agents-opaque-state`.
        return Err(crate::cookie::SANDBOXED);
    }
    // The texts for `data:` are those of a `data:` document itself; an `about:` document
    // of its origin gets the plain one (measured, case `domain-about-frames`).
    let origin = origin_of(&host.storage_url()).map_err(|message| {
        if host.url().starts_with("data:") {
            message
        } else {
            DENIED
        }
    })?;
    Ok(Rc::new(match host.storage_partition() {
        StoragePartition::FirstParty => StorageKey::first_party(&origin),
        StoragePartition::Top(site) => StorageKey::third_party(&origin, &site),
        StoragePartition::Unavailable => return Err(DENIED),
    }))
}

/// The data of a `Storage` object.
struct StorageData {
    kind: StorageKind,
    key: Rc<StorageKey>,
}

impl GenericData for StorageData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        self.key.len()
    }
}

/// The kind and the key of the area of a `Storage` object. The object belongs to
/// its document: if the document retired, or has another key, the call throws.
fn area_of(rt: &Runtime, this: Platform) -> b::Result<(StorageKind, Rc<StorageKey>)> {
    let HostData::Generic(handle) = this.data else {
        return Err(VmError::type_error("Illegal invocation").into());
    };
    let data = rt
        .heap()
        .generic::<StorageData>(handle)
        .map_err(VmError::from)?;
    let doc = &state(rt)?.doc;
    let own = match &doc.storage_key {
        Some(Ok(key)) if !doc.retired => key,
        _ => return Err(Exception::security(DENIED)),
    };
    if !Rc::ptr_eq(own, &data.key) && **own != *data.key {
        return Err(Exception::security(DENIED));
    }
    Ok((data.kind, Rc::clone(&data.key)))
}

/// The store of the document: the page's, or the runtime's own when the
/// engine has none.
fn store(rt: &mut Runtime) -> b::Result<SharedStorage> {
    if let Some(shared) = engine(rt)?.storage() {
        return Ok(shared);
    }
    let state = state_mut(rt)?;
    Ok(Rc::clone(
        state.own_storage.get_or_insert_with(StorageStore::shared),
    ))
}

/// The storage key of the current document: the document has it from when its host came,
/// and a document that has none yet computes it now.
pub(crate) fn current_key(rt: &mut Runtime) -> Result<Rc<StorageKey>, &'static str> {
    if let Ok(state) = state(rt)
        && let Some(key) = &state.doc.storage_key
    {
        return key.clone();
    }
    let Ok(host) = engine(rt) else {
        return Err(DENIED);
    };
    let key = document_key(host);
    if let Ok(state) = state_mut(rt) {
        state.doc.storage_key = Some(key.clone());
    }
    key
}

/// The `Storage` object of `kind` of the current document, whose realm is `realm`:
/// made at the first access. A `SecurityError` if the document has no storage.
pub(crate) fn storage_object_in(
    rt: &mut Runtime,
    realm: u32,
    kind: StorageKind,
) -> b::Result<Gc<Object>> {
    let slot = kind as usize;
    if let Some(object) = state(rt)?.doc.storage_objects.get(slot).copied().flatten() {
        return Ok(object);
    }
    if state(rt)?.doc.retired {
        return Err(Exception::security(DENIED));
    }
    let key = current_key(rt).map_err(Exception::security)?;
    let data = rt.alloc_host_data(StorageData { kind, key })?;
    let object = rt.new_host_object(realm, g::STORAGE, data)?;
    if let Some(entry) = state_mut(rt)?.doc.storage_objects.get_mut(slot) {
        *entry = Some(object);
    }
    Ok(object)
}

/// The `Storage` object of `kind` of the window: made at the first access.
/// A `SecurityError` if the document has no storage.
pub(crate) fn storage_object(cx: &mut Cx<'_>, kind: StorageKind) -> b::Result<Gc<Object>> {
    let realm = cx.realm();
    storage_object_in(cx.rt, realm, kind)
}

impl g::storage::StorageImpl for WebState {
    fn length(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        let (kind, key) = area_of(cx.rt, this)?;
        let len = store(cx.rt)?.borrow().len(kind, &key);
        Ok(u32::try_from(len).unwrap_or(u32::MAX))
    }

    fn clear(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        let (kind, key) = area_of(cx.rt, this)?;
        let changed = store(cx.rt)?.borrow_mut().clear(kind, &key);
        if changed {
            crate::storage_events::queue(cx.rt, kind, &key, crate::storage_events::Change::Clear)?;
        }
        Ok(())
    }

    fn get_item(cx: &mut Cx<'_>, this: Platform, key: String) -> b::Result<Option<String>> {
        let (kind, area) = area_of(cx.rt, this)?;
        Ok(store(cx.rt)?.borrow().get(kind, &area, &key))
    }

    fn key(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<Option<String>> {
        let (kind, area) = area_of(cx.rt, this)?;
        let index = usize::try_from(index).unwrap_or(usize::MAX);
        Ok(store(cx.rt)?.borrow().key(kind, &area, index))
    }

    fn remove_item(cx: &mut Cx<'_>, this: Platform, key: String) -> b::Result<()> {
        let (kind, area) = area_of(cx.rt, this)?;
        let store = store(cx.rt)?;
        // The old value is for the event, which only another document of the runtime
        // can get.
        let old = crate::storage_events::has_peers(cx.rt)?
            .then(|| store.borrow().get(kind, &area, &key))
            .flatten();
        let removed = store.borrow_mut().remove(kind, &area, &key);
        if removed {
            let change = crate::storage_events::Change::Remove { key, old };
            crate::storage_events::queue(cx.rt, kind, &area, change)?;
        }
        Ok(())
    }

    fn set_item(cx: &mut Cx<'_>, this: Platform, key: String, value: String) -> b::Result<()> {
        let (kind, area) = area_of(cx.rt, this)?;
        let store = store(cx.rt)?;
        let peers = crate::storage_events::has_peers(cx.rt)?;
        let old = peers
            .then(|| store.borrow().get(kind, &area, &key))
            .flatten();
        if peers && old.as_deref() == Some(value.as_str()) {
            // The same value: no change, no event (HTML `setItem` step 4).
            return Ok(());
        }
        let result = store.borrow_mut().set(kind, &area, &key, &value);
        result.map_err(|QuotaExceeded| {
            Exception::dom(
                "QuotaExceededError",
                format!("Setting the value of '{key}' exceeded the quota."),
            )
        })?;
        if peers {
            let change = crate::storage_events::Change::Set {
                key,
                old,
                new: value,
            };
            crate::storage_events::queue(cx.rt, kind, &area, change)?;
        }
        Ok(())
    }

    fn supported_names(cx: &mut Cx<'_>, this: Platform) -> b::Result<Vec<String>> {
        let (kind, area) = area_of(cx.rt, this)?;
        Ok(store(cx.rt)?.borrow().keys(kind, &area))
    }

    fn is_supported_name(cx: &mut Cx<'_>, this: Platform, name: &str) -> b::Result<bool> {
        let (kind, area) = area_of(cx.rt, this)?;
        Ok(store(cx.rt)?.borrow().contains(kind, &area, name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(origin: &str) -> StorageKey {
        StorageKey::first_party(origin)
    }

    fn o() -> StorageKey {
        key("http://a.test")
    }

    fn set(store: &mut StorageStore, k: &str, value: &str) -> Result<(), QuotaExceeded> {
        store.set(StorageKind::Local, &o(), k, value)
    }

    #[test]
    fn counts_utf16_code_units() {
        assert_eq!(units(""), 0);
        assert_eq!(units("abc"), 3);
        assert_eq!(units("\u{e9}\u{20ac}"), 2);
        assert_eq!(units("\u{1f600}"), 2);
    }

    #[test]
    fn quota_is_keys_plus_values_in_code_units() {
        let mut store = StorageStore::new();
        assert_eq!(
            set(&mut store, "a", &"x".repeat(AREA_QUOTA_UNITS - 1)),
            Ok(())
        );
        assert_eq!(set(&mut store, "b", ""), Err(QuotaExceeded));
        assert_eq!(set(&mut store, "", ""), Ok(()));
        assert_eq!(store.len(StorageKind::Local, &o()), 2);
        // The same value changes nothing, also with a full area.
        assert_eq!(
            set(&mut store, "a", &"x".repeat(AREA_QUOTA_UNITS - 1)),
            Ok(())
        );
        // A smaller value frees space, a bigger one fails and keeps the old.
        assert_eq!(set(&mut store, "a", "small"), Ok(()));
        assert_eq!(
            set(&mut store, "a", &"y".repeat(AREA_QUOTA_UNITS + 1)),
            Err(QuotaExceeded)
        );
        assert_eq!(
            store.get(StorageKind::Local, &o(), "a").as_deref(),
            Some("small")
        );
        // An emoji is two units.
        let mut store = StorageStore::new();
        let emoji = "\u{1f600}".repeat(AREA_QUOTA_UNITS / 2 - 1);
        assert_eq!(set(&mut store, "a", &emoji), Ok(()));
        assert_eq!(set(&mut store, "b", ""), Ok(()));
        assert_eq!(set(&mut store, "c", ""), Err(QuotaExceeded));
    }

    #[test]
    fn kinds_and_origins_are_separate() {
        let mut store = StorageStore::new();
        set(&mut store, "k", "local").unwrap();
        store
            .set(StorageKind::Session, &o(), "k", "session")
            .unwrap();
        store
            .set(StorageKind::Local, &key("http://b.test"), "k", "other")
            .unwrap();
        assert_eq!(
            store.get(StorageKind::Local, &o(), "k").as_deref(),
            Some("local")
        );
        assert_eq!(
            store.get(StorageKind::Session, &o(), "k").as_deref(),
            Some("session")
        );
        assert_eq!(
            store
                .get(StorageKind::Local, &key("http://b.test"), "k")
                .as_deref(),
            Some("other")
        );
        assert!(store.clear(StorageKind::Local, &o()));
        assert_eq!(store.get(StorageKind::Local, &o(), "k"), None);
        assert_eq!(
            store.get(StorageKind::Session, &o(), "k").as_deref(),
            Some("session")
        );
        assert!(!store.clear(StorageKind::Local, &o()));
    }

    /// The area of third-party content is its own per top-level site, and none of
    /// the other areas of its origin.
    #[test]
    fn partitions_are_separate_areas() {
        let mut store = StorageStore::new();
        let first = key("http://l.test");
        let under_a = StorageKey::third_party("http://l.test", "http://a.test");
        let under_b = StorageKey::third_party("http://l.test", "http://b.test");
        for (area, value) in [(&first, "first"), (&under_a, "a"), (&under_b, "b")] {
            store.set(StorageKind::Local, area, "k", value).unwrap();
        }
        for (area, value) in [(&first, "first"), (&under_a, "a"), (&under_b, "b")] {
            assert_eq!(
                store.get(StorageKind::Local, area, "k").as_deref(),
                Some(value)
            );
        }
        assert!(store.clear(StorageKind::Local, &under_a));
        assert_eq!(store.get(StorageKind::Local, &under_a, "k"), None);
        assert_eq!(
            store.get(StorageKind::Local, &first, "k").as_deref(),
            Some("first")
        );
        assert_eq!(
            store.get(StorageKind::Local, &under_b, "k").as_deref(),
            Some("b")
        );
        // A key of the origin of another top-level site is a third key.
        assert_ne!(
            first,
            StorageKey::third_party("http://l.test", "http://l.test")
        );
    }

    /// The quota counts per key, and the total counts all keys.
    #[test]
    fn quota_is_per_key_and_the_total_is_over_all() {
        let mut store = StorageStore::new();
        let full = "x".repeat(AREA_QUOTA_UNITS - 1);
        let under_a = StorageKey::third_party("http://l.test", "http://a.test");
        let under_b = StorageKey::third_party("http://l.test", "http://b.test");
        for area in [&o(), &under_a, &under_b] {
            assert_eq!(store.set(StorageKind::Local, area, "a", &full), Ok(()));
            assert_eq!(
                store.set(StorageKind::Local, area, "b", "xx"),
                Err(QuotaExceeded)
            );
        }
    }

    #[test]
    fn order_removal_and_replacement() {
        let mut store = StorageStore::new();
        for k in ["a", "b", "c", "d"] {
            set(&mut store, k, k).unwrap();
        }
        set(&mut store, "b", "again").unwrap();
        assert_eq!(store.keys(StorageKind::Local, &o()), ["a", "b", "c", "d"]);
        assert!(store.remove(StorageKind::Local, &o(), "b"));
        assert!(!store.remove(StorageKind::Local, &o(), "b"));
        // The last key moves to the place of the removed one.
        assert_eq!(store.keys(StorageKind::Local, &o()), ["a", "d", "c"]);
        assert_eq!(
            store.get(StorageKind::Local, &o(), "d").as_deref(),
            Some("d")
        );
        assert_eq!(store.key(StorageKind::Local, &o(), 1).as_deref(), Some("d"));
        assert_eq!(store.key(StorageKind::Local, &o(), 3), None);
        assert!(store.remove(StorageKind::Local, &o(), "c"));
        assert!(store.remove(StorageKind::Local, &o(), "d"));
        assert!(store.remove(StorageKind::Local, &o(), "a"));
        assert_eq!(store.len(StorageKind::Local, &o()), 0);
        assert_eq!(store.total, 0);
    }

    #[test]
    fn bound_on_the_keys_of_an_area() {
        let mut store = StorageStore::new();
        for i in 0..MAX_KEYS_PER_AREA {
            assert_eq!(set(&mut store, &i.to_string(), ""), Ok(()));
        }
        assert_eq!(set(&mut store, "one more", ""), Err(QuotaExceeded));
        assert_eq!(set(&mut store, "5", "replacing is fine"), Ok(()));
        assert!(store.remove(StorageKind::Local, &o(), "7"));
        assert_eq!(set(&mut store, "one more", ""), Ok(()));
    }

    #[test]
    fn bound_over_all_areas() {
        let mut store = StorageStore::new();
        let chunk = "x".repeat(AREA_QUOTA_UNITS - 8);
        let mut written = 0;
        for i in 0..20 {
            let origin = key(&format!("http://o{i}.test"));
            if store.set(StorageKind::Local, &origin, "k", &chunk).is_err() {
                break;
            }
            written += 1;
        }
        // 64 MiB of total / (5 MiB - 8 + a key + an area) = 12 areas.
        assert_eq!(written, 12);
        assert!(store.total <= TOTAL_LIMIT_BYTES);
        // A smaller write still fits, and clearing an area frees its share.
        assert!(store.clear(StorageKind::Local, &key("http://o0.test")));
        assert_eq!(
            store.set(StorageKind::Session, &key("http://new.test"), "k", &chunk),
            Ok(())
        );
    }

    #[test]
    fn total_counts_bytes() {
        let mut store = StorageStore::new();
        // A unit of U+20AC is three bytes, so a full area costs 15 MiB.
        let chunk = "\u{20ac}".repeat(AREA_QUOTA_UNITS - 8);
        let mut written = 0;
        for i in 0..20 {
            let origin = key(&format!("http://o{i}.test"));
            if store.set(StorageKind::Local, &origin, "k", &chunk).is_err() {
                break;
            }
            written += 1;
        }
        assert_eq!(written, 4);
        // A shorter value gives its bytes back; removing the last key gives
        // back the area too.
        let before = store.total;
        assert_eq!(
            store.set(StorageKind::Local, &key("http://o0.test"), "k", "x"),
            Ok(())
        );
        assert_eq!(before - store.total, chunk.len() - 1);
        for i in 0..4 {
            assert!(store.remove(StorageKind::Local, &key(&format!("http://o{i}.test")), "k"));
        }
        assert_eq!(store.total, 0);
    }

    #[test]
    fn origins_of_documents() {
        assert_eq!(
            origin_of("https://a.test/x?y#z").as_deref(),
            Ok("https://a.test")
        );
        assert_eq!(
            origin_of("http://a.test:8080/").as_deref(),
            Ok("http://a.test:8080")
        );
        assert_eq!(
            origin_of("http://a.test:80/").as_deref(),
            Ok("http://a.test")
        );
        assert_eq!(origin_of("file:///tmp/a.html").as_deref(), Ok("file://"));
        assert_eq!(
            origin_of("data:text/html,x"),
            Err("Storage is disabled inside 'data:' URLs.")
        );
        assert_eq!(origin_of("about:blank"), Err(DENIED));
        assert_eq!(origin_of("garbage"), Err(DENIED));
    }
}
