//! Web Storage (HTML §12.2,
//! <https://html.spec.whatwg.org/multipage/webstorage.html>; ADR 0028
//! sections 12 and 16): the data of `localStorage` and `sessionStorage`
//! and the `Storage` interface over it.
//!
//! The data lives outside the document, in a [`StorageStore`] that the
//! page owns and lends to each document ([`crate::EngineHost::storage`]):
//! both kinds are kept per origin in memory, `localStorage` for the life of
//! the process and `sessionStorage` for the life of the page (one tab);
//! both survive navigations between documents. An origin is the
//! serialization of an `http:` or `https:` origin; all `file:` documents
//! share the origin `file://` (Chromium gives them one storage area). A
//! document with another URL has no storage: `data:` documents and the
//! others throw `SecurityError` with Chromium's texts.
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
//!   areas of both kinds, in bytes of memory (the UTF-8 bytes of keys and
//!   values, a fixed cost per key and per area, the origins), so that a
//!   session that visits many origins cannot grow without limit.
//!
//! The order of keys is the order of insertion, except that removing a key
//! moves the last key to its place. Chromium's order is that of a hash
//! table; the standard leaves the order to the user agent.
//!
//! Strings are UTF-8: a lone surrogate in a key or a value becomes U+FFFD
//! (the conversion of every `DOMString`, ADR 0028 section 3); Chromium
//! keeps lone surrogates.
//!
//! The `storage` event goes only to the other documents of the origin
//! (HTML "the storage event"). In M8 the page has one document at a time and
//! no frames, so no event is fired. A second document would get it as a
//! new `Task` variant of the event loop, queued by the writing document for
//! each other document of the origin and run by `ScriptRuntime::run_task`
//! (`runtime.rs`), which would dispatch a `StorageEvent` at its window with
//! `storageArea` set to its own `Storage` object.

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
/// origin plus [`AREA_COST_BYTES`] for each area.
pub const TOTAL_LIMIT_BYTES: usize = 64 * 1024 * 1024;

/// The cost of one key in the total, besides its bytes: the entry of the
/// area (two strings, the index).
pub const KEY_COST_BYTES: usize = 64;

/// The cost of one area in the total, besides its origin: the maps.
pub const AREA_COST_BYTES: usize = 64;

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

/// The storage of a page: all areas of both kinds, by origin. See the
/// module documentation.
#[derive(Debug, Default)]
pub struct StorageStore {
    local: HashMap<String, Area>,
    session: HashMap<String, Area>,
    /// The cost of all areas in bytes ([`TOTAL_LIMIT_BYTES`]).
    total: usize,
}

/// The handle of a [`StorageStore`] that the page and its documents share.
pub type SharedStorage = Rc<RefCell<StorageStore>>;

/// One area: the keys and values of an origin in one kind.
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
fn area_bytes(origin: &str) -> usize {
    origin.len() + AREA_COST_BYTES
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

    fn area(&self, kind: StorageKind, origin: &str) -> Option<&Area> {
        match kind {
            StorageKind::Local => self.local.get(origin),
            StorageKind::Session => self.session.get(origin),
        }
    }

    fn areas_mut(&mut self, kind: StorageKind) -> &mut HashMap<String, Area> {
        match kind {
            StorageKind::Local => &mut self.local,
            StorageKind::Session => &mut self.session,
        }
    }

    /// The number of keys of the area.
    pub fn len(&self, kind: StorageKind, origin: &str) -> usize {
        self.area(kind, origin).map_or(0, |a| a.entries.len())
    }

    /// The number of keys of all areas.
    pub fn total_keys(&self) -> usize {
        self.local
            .values()
            .chain(self.session.values())
            .map(|a| a.entries.len())
            .sum()
    }

    /// The key at `index` in the order of the area.
    pub fn key(&self, kind: StorageKind, origin: &str, index: usize) -> Option<String> {
        let area = self.area(kind, origin)?;
        area.entries.get(index).map(|(key, _)| key.to_string())
    }

    /// The value of `key`.
    pub fn get(&self, kind: StorageKind, origin: &str, key: &str) -> Option<String> {
        let area = self.area(kind, origin)?;
        let index = *area.index.get(key)?;
        area.entries.get(index).map(|(_, value)| value.clone())
    }

    /// True if the area has `key`.
    pub fn contains(&self, kind: StorageKind, origin: &str, key: &str) -> bool {
        self.area(kind, origin)
            .is_some_and(|a| a.index.contains_key(key))
    }

    /// The keys of the area, in order.
    pub fn keys(&self, kind: StorageKind, origin: &str) -> Vec<String> {
        self.area(kind, origin).map_or_else(Vec::new, |a| {
            a.entries.iter().map(|(key, _)| key.to_string()).collect()
        })
    }

    /// Sets `key` to `value`. Fails, and changes nothing, if the area would
    /// pass its quota, or a bound of swb.
    pub fn set(
        &mut self,
        kind: StorageKind,
        origin: &str,
        key: &str,
        value: &str,
    ) -> Result<(), QuotaExceeded> {
        let old = self.area(kind, origin).and_then(|area| {
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
                let new_area = if self.area(kind, origin).is_none() {
                    area_bytes(origin)
                } else {
                    0
                };
                let bytes = entry_bytes(key, value) + new_area;
                (units(key) + new_units, 0, bytes, 0, true)
            }
        };
        let area_units = self.area(kind, origin).map_or(0, |a| a.units);
        let area_keys = self.len(kind, origin);
        if area_units - area_units.min(shrink) + grow > AREA_QUOTA_UNITS
            || (new_key && area_keys >= MAX_KEYS_PER_AREA)
            || self.total - self.total.min(shrink_bytes) + grow_bytes > TOTAL_LIMIT_BYTES
        {
            return Err(QuotaExceeded);
        }
        self.total = self.total - self.total.min(shrink_bytes) + grow_bytes;
        let area = self.areas_mut(kind).entry(origin.to_owned()).or_default();
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
    pub fn remove(&mut self, kind: StorageKind, origin: &str, key: &str) -> bool {
        let Some(area) = self.areas_mut(kind).get_mut(origin) else {
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
            self.areas_mut(kind).remove(origin);
            freed_bytes += area_bytes(origin);
        }
        self.total = self.total.saturating_sub(freed_bytes);
        true
    }

    /// Removes all keys of the area. True if there were any.
    pub fn clear(&mut self, kind: StorageKind, origin: &str) -> bool {
        let Some(area) = self.areas_mut(kind).remove(origin) else {
            return false;
        };
        let cost: usize = area
            .entries
            .iter()
            .map(|(key, value)| entry_bytes(key, value))
            .sum();
        self.total = self.total.saturating_sub(cost + area_bytes(origin));
        !area.entries.is_empty()
    }
}

/// The text of the `SecurityError` of a document that has no storage, or
/// the key of its areas.
fn origin_of(url: &str) -> Result<String, &'static str> {
    let Ok(parsed) = url::Url::parse(url) else {
        return Err("Access is denied for this document.");
    };
    match parsed.scheme() {
        "http" | "https" => Ok(parsed.origin().ascii_serialization()),
        "file" => Ok("file://".to_owned()),
        "data" => Err("Storage is disabled inside 'data:' URLs."),
        _ => Err("Access is denied for this document."),
    }
}

/// The data of a `Storage` object.
struct StorageData {
    kind: StorageKind,
    origin: Rc<str>,
}

impl GenericData for StorageData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        self.origin.len()
    }
}

/// The kind and the origin of the area of a `Storage` object.
fn area_of(rt: &Runtime, this: Platform) -> b::Result<(StorageKind, Rc<str>)> {
    match this.data {
        HostData::Generic(handle) => {
            let data = rt
                .heap()
                .generic::<StorageData>(handle)
                .map_err(VmError::from)?;
            Ok((data.kind, Rc::clone(&data.origin)))
        }
        HostData::Word(_) => Err(VmError::type_error("Illegal invocation").into()),
    }
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

/// The `Storage` object of `kind` of the window: made at the first access.
/// A `SecurityError` if the document has no storage.
pub(crate) fn storage_object(cx: &mut Cx<'_>, kind: StorageKind) -> b::Result<Gc<Object>> {
    let slot = kind as usize;
    if let Some(object) = state(cx.rt)?.storage_objects.get(slot).copied().flatten() {
        return Ok(object);
    }
    let url = engine(cx.rt)?.url();
    let origin = origin_of(&url).map_err(|message| Exception::dom("SecurityError", message))?;
    let data = cx.rt.alloc_host_data(StorageData {
        kind,
        origin: Rc::from(origin),
    })?;
    let object = cx.rt.new_host_object(cx.realm(), g::STORAGE, data)?;
    if let Some(entry) = state_mut(cx.rt)?.storage_objects.get_mut(slot) {
        *entry = Some(object);
    }
    Ok(object)
}

impl g::storage::StorageImpl for WebState {
    fn length(cx: &mut Cx<'_>, this: Platform) -> b::Result<u32> {
        let (kind, origin) = area_of(cx.rt, this)?;
        let len = store(cx.rt)?.borrow().len(kind, &origin);
        Ok(u32::try_from(len).unwrap_or(u32::MAX))
    }

    fn clear(cx: &mut Cx<'_>, this: Platform) -> b::Result<()> {
        let (kind, origin) = area_of(cx.rt, this)?;
        store(cx.rt)?.borrow_mut().clear(kind, &origin);
        Ok(())
    }

    fn get_item(cx: &mut Cx<'_>, this: Platform, key: String) -> b::Result<Option<String>> {
        let (kind, origin) = area_of(cx.rt, this)?;
        Ok(store(cx.rt)?.borrow().get(kind, &origin, &key))
    }

    fn key(cx: &mut Cx<'_>, this: Platform, index: u32) -> b::Result<Option<String>> {
        let (kind, origin) = area_of(cx.rt, this)?;
        let index = usize::try_from(index).unwrap_or(usize::MAX);
        Ok(store(cx.rt)?.borrow().key(kind, &origin, index))
    }

    fn remove_item(cx: &mut Cx<'_>, this: Platform, key: String) -> b::Result<()> {
        let (kind, origin) = area_of(cx.rt, this)?;
        store(cx.rt)?.borrow_mut().remove(kind, &origin, &key);
        Ok(())
    }

    fn set_item(cx: &mut Cx<'_>, this: Platform, key: String, value: String) -> b::Result<()> {
        let (kind, origin) = area_of(cx.rt, this)?;
        let result = store(cx.rt)?.borrow_mut().set(kind, &origin, &key, &value);
        result.map_err(|QuotaExceeded| {
            Exception::dom(
                "QuotaExceededError",
                format!("Setting the value of '{key}' exceeded the quota."),
            )
        })
    }

    fn supported_names(cx: &mut Cx<'_>, this: Platform) -> b::Result<Vec<String>> {
        let (kind, origin) = area_of(cx.rt, this)?;
        Ok(store(cx.rt)?.borrow().keys(kind, &origin))
    }

    fn is_supported_name(cx: &mut Cx<'_>, this: Platform, name: &str) -> b::Result<bool> {
        let (kind, origin) = area_of(cx.rt, this)?;
        Ok(store(cx.rt)?.borrow().contains(kind, &origin, name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const O: &str = "http://a.test";

    fn set(store: &mut StorageStore, key: &str, value: &str) -> Result<(), QuotaExceeded> {
        store.set(StorageKind::Local, O, key, value)
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
        assert_eq!(store.len(StorageKind::Local, O), 2);
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
            store.get(StorageKind::Local, O, "a").as_deref(),
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
        store.set(StorageKind::Session, O, "k", "session").unwrap();
        store
            .set(StorageKind::Local, "http://b.test", "k", "other")
            .unwrap();
        assert_eq!(
            store.get(StorageKind::Local, O, "k").as_deref(),
            Some("local")
        );
        assert_eq!(
            store.get(StorageKind::Session, O, "k").as_deref(),
            Some("session")
        );
        assert_eq!(
            store
                .get(StorageKind::Local, "http://b.test", "k")
                .as_deref(),
            Some("other")
        );
        assert!(store.clear(StorageKind::Local, O));
        assert_eq!(store.get(StorageKind::Local, O, "k"), None);
        assert_eq!(
            store.get(StorageKind::Session, O, "k").as_deref(),
            Some("session")
        );
        assert!(!store.clear(StorageKind::Local, O));
    }

    #[test]
    fn order_removal_and_replacement() {
        let mut store = StorageStore::new();
        for key in ["a", "b", "c", "d"] {
            set(&mut store, key, key).unwrap();
        }
        set(&mut store, "b", "again").unwrap();
        assert_eq!(store.keys(StorageKind::Local, O), ["a", "b", "c", "d"]);
        assert!(store.remove(StorageKind::Local, O, "b"));
        assert!(!store.remove(StorageKind::Local, O, "b"));
        // The last key moves to the place of the removed one.
        assert_eq!(store.keys(StorageKind::Local, O), ["a", "d", "c"]);
        assert_eq!(store.get(StorageKind::Local, O, "d").as_deref(), Some("d"));
        assert_eq!(store.key(StorageKind::Local, O, 1).as_deref(), Some("d"));
        assert_eq!(store.key(StorageKind::Local, O, 3), None);
        assert!(store.remove(StorageKind::Local, O, "c"));
        assert!(store.remove(StorageKind::Local, O, "d"));
        assert!(store.remove(StorageKind::Local, O, "a"));
        assert_eq!(store.len(StorageKind::Local, O), 0);
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
        assert!(store.remove(StorageKind::Local, O, "7"));
        assert_eq!(set(&mut store, "one more", ""), Ok(()));
    }

    #[test]
    fn bound_over_all_areas() {
        let mut store = StorageStore::new();
        let chunk = "x".repeat(AREA_QUOTA_UNITS - 8);
        let mut written = 0;
        for i in 0..20 {
            let origin = format!("http://o{i}.test");
            if store.set(StorageKind::Local, &origin, "k", &chunk).is_err() {
                break;
            }
            written += 1;
        }
        // 64 MiB of total / (5 MiB - 8 + a key + an area) = 12 areas.
        assert_eq!(written, 12);
        assert!(store.total <= TOTAL_LIMIT_BYTES);
        // A smaller write still fits, and clearing an area frees its share.
        assert!(store.clear(StorageKind::Local, "http://o0.test"));
        assert_eq!(
            store.set(StorageKind::Session, "http://new.test", "k", &chunk),
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
            let origin = format!("http://o{i}.test");
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
            store.set(StorageKind::Local, "http://o0.test", "k", "x"),
            Ok(())
        );
        assert_eq!(before - store.total, chunk.len() - 1);
        for i in 0..4 {
            assert!(store.remove(StorageKind::Local, &format!("http://o{i}.test"), "k"));
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
        assert_eq!(
            origin_of("about:blank"),
            Err("Access is denied for this document.")
        );
        assert_eq!(
            origin_of("garbage"),
            Err("Access is denied for this document.")
        );
    }
}
