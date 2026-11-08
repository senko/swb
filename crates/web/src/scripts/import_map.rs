//! Import maps (HTML "import maps",
//! <https://html.spec.whatwg.org/multipage/webappapis.html#import-maps>; ADR
//! 0034 sections 9 and 11): "parse an import map string", "merge existing and
//! new import maps", and "resolve a module specifier" with the map. The
//! document keeps one merged map and its "resolved module set".
//!
//! - **Parse.** `serde_json` reads the text into ordered objects
//!   ([`Node`]); the normalization of keys and addresses follows HTML
//!   ("normalizing a specifier key", "normalizing a module specifier map"):
//!   a key or an address that is URL-like (`/`, `./`, `../`) or an absolute URL
//!   becomes its serialization, an invalid entry becomes a blocked (null)
//!   entry or is dropped, each with a warning (the first few are logged).
//! - **Resolution.** A specifier map is a hash map; the entries whose key ends
//!   in `/` are found by looking up the prefixes of the specifier that end in
//!   `/` (longest first), so a lookup costs the number of `/` in the
//!   specifier, not the number of entries. The scopes are found the same way
//!   from the referrer's URL. One resolution hashes at most [`MAX_LOOKUP_BYTES`]
//!   bytes in its specifier lookups, the exact lookup in each scope and in
//!   the top-level imports included (a hostile specifier of 100,000 `/` ends
//!   the search; no match).
//! - **Merge.** A key that an earlier map has stays. A rule of the new map
//!   that would change the result for a specifier that the document already
//!   resolved is dropped (HTML's resolved module set, kept up to
//!   [`MAX_RESOLVED`] records and [`MAX_RESOLVED_BYTES`] bytes, a record of
//!   more than [`MAX_RESOLVED_RECORD_BYTES`] is not kept: a departure, later
//!   resolutions are not recorded, so a later map's rule may change them).
//! - **Budget.** At most [`MAX_ENTRIES`] entries after the merge (imports and
//!   all scopes); later entries are ignored, with one warning.
//!
//! Chromium 148's messages and rules were measured in
//! `tools/probes/web-module-graph.json` (`importmap-*`, `msg-importmap-*`).

use std::collections::{HashMap, HashSet};

use serde::de::{Deserialize, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use url::Url;

/// The most entries of a document's merged import map (ADR 0034 section 11;
/// Chromium has no limit).
pub(crate) const MAX_ENTRIES: usize = 65_536;

/// The most records that the resolved module set keeps.
const MAX_RESOLVED: usize = 4096;

/// The most bytes of one record of the resolved module set (the specifier and
/// the base URL): a longer pair is not recorded.
const MAX_RESOLVED_RECORD_BYTES: usize = 4096;

/// The most bytes that the records of the resolved module set hold in all.
const MAX_RESOLVED_BYTES: usize = 1 << 20;

/// The most records of the resolved module set that the merges of one
/// document check in all (a merge checks every record).
const MAX_MERGE_WORK: usize = 4_000_000;

/// The most bytes that one resolution hashes in its prefix lookups.
const MAX_LOOKUP_BYTES: usize = 1 << 20;

/// The most warnings of one import map that are logged one by one.
const MAX_WARNINGS: usize = 8;

/// Why parsing an import map failed (the error to report).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ParseError {
    /// The text is not JSON: a `SyntaxError`.
    Syntax,
    /// The JSON has the wrong shape: a `TypeError` with this message.
    Type(String),
}

impl ParseError {
    /// The message of the error, with Chromium's prefix.
    pub(crate) fn message(&self) -> String {
        match self {
            ParseError::Syntax => "Failed to parse import map: invalid JSON".to_owned(),
            ParseError::Type(text) => format!("Failed to parse import map: {text}"),
        }
    }
}

/// A JSON value as the import map parser needs it: objects keep the order of
/// their keys; the other values are kept only as far as the algorithm tells
/// them apart.
#[derive(Debug)]
enum Node {
    Object(Vec<(String, Node)>),
    String(String),
    Other,
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Node, D::Error> {
        struct NodeVisitor;
        impl<'de> Visitor<'de> for NodeVisitor {
            type Value = Node;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON value")
            }

            fn visit_str<E>(self, v: &str) -> Result<Node, E> {
                Ok(Node::String(v.to_owned()))
            }

            fn visit_string<E>(self, v: String) -> Result<Node, E> {
                Ok(Node::String(v))
            }

            fn visit_bool<E>(self, _: bool) -> Result<Node, E> {
                Ok(Node::Other)
            }

            fn visit_i64<E>(self, _: i64) -> Result<Node, E> {
                Ok(Node::Other)
            }

            fn visit_u64<E>(self, _: u64) -> Result<Node, E> {
                Ok(Node::Other)
            }

            fn visit_f64<E>(self, _: f64) -> Result<Node, E> {
                Ok(Node::Other)
            }

            fn visit_unit<E>(self) -> Result<Node, E> {
                Ok(Node::Other)
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Node, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(Node::Other)
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
                // A repeated key keeps its first place and gets the last
                // value (JSON.parse).
                let mut entries: Vec<(String, Node)> = Vec::new();
                let mut places: HashMap<String, usize> = HashMap::new();
                while let Some((key, value)) = map.next_entry::<String, Node>()? {
                    if let Some(&at) = places.get(&key) {
                        if let Some(entry) = entries.get_mut(at) {
                            entry.1 = value;
                        }
                    } else {
                        places.insert(key.clone(), entries.len());
                        entries.push((key, value));
                    }
                }
                Ok(Node::Object(entries))
            }
        }
        deserializer.deserialize_any(NodeVisitor)
    }
}

/// Logs the first warnings of a parse, then counts the rest.
#[derive(Default)]
struct Warnings {
    count: usize,
}

impl Warnings {
    fn warn(&mut self, message: &str) {
        self.count += 1;
        if self.count <= MAX_WARNINGS {
            log::warn!("import map: {message}");
        }
    }

    fn finish(&self) {
        if self.count > MAX_WARNINGS {
            log::warn!(
                "import map: {} more warnings were not logged",
                self.count - MAX_WARNINGS
            );
        }
    }
}

/// A module specifier map: the normalized keys and their addresses (`None`
/// blocks the specifier).
#[derive(Clone, Debug, Default)]
struct SpecifierMap {
    entries: HashMap<String, Option<Url>>,
    /// The keys in the order of their first insertion: the merge adds
    /// entries in this order, so the entry limit ignores the later ones.
    order: Vec<String>,
}

impl SpecifierMap {
    /// Sets the entry `key` (a repeated key keeps its place).
    fn set(&mut self, key: String, address: Option<Url>) {
        if self.entries.insert(key.clone(), address).is_none() {
            self.order.push(key);
        }
    }
}

/// A parsed import map (HTML "import map": `imports` and `scopes`).
#[derive(Debug, Default)]
pub(crate) struct ParsedMap {
    imports: SpecifierMap,
    /// The scopes by the serialization of their prefix URL.
    scopes: HashMap<String, SpecifierMap>,
    /// The prefixes of `scopes` in the order of their first insertion.
    scope_order: Vec<String>,
}

/// HTML "resolve a URL-like module specifier": the URL of a specifier that
/// starts with `/`, `./` or `../` (against `base`), or that is an absolute
/// URL; else `None`.
pub(crate) fn resolve_url_like(specifier: &str, base: &Url) -> Option<Url> {
    if specifier.starts_with('/') || specifier.starts_with("./") || specifier.starts_with("../") {
        base.join(specifier).ok()
    } else {
        Url::parse(specifier).ok()
    }
}

/// True for the special schemes of the URL standard.
fn is_special(url: &Url) -> bool {
    matches!(
        url.scheme(),
        "ftp" | "file" | "http" | "https" | "ws" | "wss"
    )
}

/// HTML "normalize a specifier key": the serialization of a URL-like key, the
/// key itself otherwise; `None` for the empty key.
fn normalize_key(key: &str, base: &Url) -> Option<String> {
    if key.is_empty() {
        return None;
    }
    Some(resolve_url_like(key, base).map_or_else(|| key.to_owned(), |url| url.to_string()))
}

/// HTML "sort and normalize a module specifier map" (the sort is not needed:
/// the lookups find the longest prefix).
fn normalize_map(entries: &[(String, Node)], base: &Url, warnings: &mut Warnings) -> SpecifierMap {
    let mut map = SpecifierMap::default();
    for (key, value) in entries {
        let Some(normalized) = normalize_key(key, base) else {
            warnings.warn("an empty key is ignored");
            continue;
        };
        let Node::String(address) = value else {
            warnings.warn(&format!("the address of {key:?} is not a string"));
            map.set(normalized, None);
            continue;
        };
        let Some(url) = resolve_url_like(address, base) else {
            warnings.warn(&format!("the address {address:?} of {key:?} is invalid"));
            map.set(normalized, None);
            continue;
        };
        if key.ends_with('/') && !url.as_str().ends_with('/') {
            warnings.warn(&format!(
                "the address of {key:?} must end with a slash: {address:?}"
            ));
            map.set(normalized, None);
            continue;
        }
        map.set(normalized, Some(url));
    }
    map
}

/// HTML "parse an import map string": `text` is the script's text, `base` the
/// document's base URL when the script was prepared.
pub(crate) fn parse(text: &str, base: &Url) -> Result<ParsedMap, ParseError> {
    let node: Node = serde_json::from_str(text).map_err(|_| ParseError::Syntax)?;
    let Node::Object(top) = node else {
        return Err(ParseError::Type("not an object".to_owned()));
    };
    let mut warnings = Warnings::default();
    let mut imports = None;
    let mut scopes = None;
    for (key, value) in &top {
        match key.as_str() {
            "imports" => imports = Some(value),
            "scopes" => scopes = Some(value),
            other => warnings.warn(&format!("the top-level key {other:?} is ignored")),
        }
    }
    let imports_object = match imports {
        None => None,
        Some(Node::Object(entries)) => Some(entries),
        Some(_) => {
            return Err(ParseError::Type(
                "\"imports\" top-level key must be a JSON object.".to_owned(),
            ));
        }
    };
    let scopes_object = match scopes {
        None => None,
        Some(Node::Object(entries)) => Some(entries),
        Some(_) => {
            return Err(ParseError::Type(
                "\"scopes\" top-level key must be a JSON object.".to_owned(),
            ));
        }
    };
    let mut parsed = ParsedMap::default();
    if let Some(entries) = imports_object {
        parsed.imports = normalize_map(entries, base, &mut warnings);
    }
    for (prefix, value) in scopes_object.into_iter().flatten() {
        let Node::Object(entries) = value else {
            return Err(ParseError::Type(format!(
                "the value of the scope with prefix \"{prefix}\" must be a JSON object."
            )));
        };
        let Ok(prefix_url) = base.join(prefix) else {
            warnings.warn(&format!("the scope prefix {prefix:?} is not a URL"));
            continue;
        };
        let map = normalize_map(entries, base, &mut warnings);
        let prefix_url = prefix_url.to_string();
        if parsed.scopes.insert(prefix_url.clone(), map).is_none() {
            parsed.scope_order.push(prefix_url);
        }
    }
    warnings.finish();
    Ok(parsed)
}

/// Why a specifier did not resolve through the map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Blocked {
    /// The map maps the specifier to null (`Import Map: "x" matches with
    /// "k" but is blocked by a null value`).
    Null { specifier: String, key: String },
    /// A prefix match leaves the address (`... but is blocked due to
    /// backtracking`).
    Backtracking { specifier: String, key: String },
}

impl Blocked {
    /// Chromium's text, which follows the message of the failed resolution.
    pub(crate) fn text(&self) -> String {
        match self {
            Blocked::Null { specifier, key } => format!(
                "Import Map: \"{specifier}\" matches with \"{key}\" but is blocked by a null value"
            ),
            Blocked::Backtracking { specifier, key } => format!(
                "Import Map: \"{specifier}\" matches with \"{key}\" but is blocked due to \
                 backtracking"
            ),
        }
    }
}

/// The prefixes of `text` that end in `/`, longest first, after `text`
/// itself when `whole`; each costs its length against `budget`.
fn prefixes<'a, 'b>(
    text: &'a str,
    whole: bool,
    budget: &'b mut usize,
) -> impl Iterator<Item = &'a str> + use<'a, 'b> {
    let first = whole.then_some(text);
    let slashes = text.rmatch_indices('/').map(move |(at, _)| &text[..=at]);
    first.into_iter().chain(slashes).take_while(move |prefix| {
        *budget = budget.saturating_sub(prefix.len());
        *budget > 0
    })
}

impl SpecifierMap {
    /// HTML "resolve an imports match": the address for `specifier`
    /// (`as_url`: its URL-like parse), `Ok(None)` if the map has no entry.
    fn lookup(
        &self,
        specifier: &str,
        as_url: Option<&Url>,
        budget: &mut usize,
    ) -> Result<Option<Url>, Blocked> {
        // The exact lookup hashes the whole specifier: it costs its length,
        // and none is made after the budget is gone.
        if *budget == 0 {
            return Ok(None);
        }
        *budget = budget.saturating_sub(specifier.len());
        if let Some(address) = self.entries.get(specifier) {
            return address.clone().map(Some).ok_or_else(|| Blocked::Null {
                specifier: specifier.to_owned(),
                key: specifier.to_owned(),
            });
        }
        if as_url.is_some_and(|url| !is_special(url)) {
            return Ok(None);
        }
        for prefix in prefixes(specifier, false, budget) {
            let Some(address) = self.entries.get(prefix) else {
                continue;
            };
            let blocked = |null: bool| {
                let (specifier, key) = (specifier.to_owned(), prefix.to_owned());
                if null {
                    Blocked::Null { specifier, key }
                } else {
                    Blocked::Backtracking { specifier, key }
                }
            };
            let Some(address) = address else {
                return Err(blocked(true));
            };
            let after = &specifier[prefix.len()..];
            let Ok(url) = address.join(after) else {
                return Err(blocked(false));
            };
            if !url.as_str().starts_with(address.as_str()) {
                return Err(blocked(false));
            }
            return Ok(Some(url));
        }
        Ok(None)
    }

    /// Adds the entries of `new` that this map does not have; `room` is the
    /// number of entries that may still be added. Returns how many were
    /// added.
    fn merge(&mut self, mut new: SpecifierMap, room: usize, dropped: &mut usize) -> usize {
        let mut added = 0;
        for key in new.order {
            // A rule that the resolved module set dropped is not in `entries`.
            let Some(address) = new.entries.remove(&key) else {
                continue;
            };
            if self.entries.contains_key(&key) {
                log::warn!("import map: the key {key:?} is already in an earlier map");
                continue;
            }
            if added >= room {
                *dropped += 1;
                continue;
            }
            self.entries.insert(key.clone(), address);
            self.order.push(key);
            added += 1;
        }
        added
    }

    /// Removes the entries of the map that a resolution of `specifier` would
    /// use (HTML "merge existing and new import maps", step 2.1.2 and 3):
    /// the key `specifier`, and the keys that end in `/` and that are
    /// prefixes of it (when it is not a URL with a non-special scheme).
    fn remove_resolved(&mut self, specifier: &str, budget: &mut usize) {
        if *budget == 0 {
            return;
        }
        *budget = budget.saturating_sub(specifier.len());
        if self.entries.remove(specifier).is_some() {
            log::warn!("import map: the rule for {specifier:?} is ignored: it was resolved");
        }
        if Url::parse(specifier).is_ok_and(|url| !is_special(&url)) {
            return;
        }
        for prefix in prefixes(specifier, false, budget) {
            if self.entries.remove(prefix).is_some() {
                log::warn!("import map: the rule for {prefix:?} is ignored: it was resolved");
            }
        }
    }
}

/// The import map of a document and its resolved module set.
#[derive(Debug, Default)]
pub(crate) struct ImportMap {
    imports: SpecifierMap,
    scopes: HashMap<String, SpecifierMap>,
    /// The entries of `imports` and of all scopes.
    entries: usize,
    /// HTML "resolved module set": (serialized base URL, normalized
    /// specifier).
    resolved: HashSet<(String, String)>,
    /// The bytes of the records of `resolved` ([`MAX_RESOLVED_BYTES`]).
    resolved_bytes: usize,
    limit_warned: bool,
    /// The records that the merges checked so far ([`MAX_MERGE_WORK`]).
    merge_work: usize,
    work_warned: bool,
}

/// The result of a resolution through the map.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Resolved {
    /// The URL.
    Url(Url),
    /// The specifier is not mapped and not a URL.
    None,
    /// The map blocks the specifier.
    Blocked(Blocked),
}

impl ImportMap {
    /// The number of entries (imports and scopes).
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries
    }

    /// True if the map has no rule (the lookups are skipped).
    fn is_empty(&self) -> bool {
        self.entries == 0
    }

    /// HTML "register an import map" with a parsed map: merges it into the
    /// document's map.
    pub(crate) fn register(&mut self, mut new: ParsedMap) {
        self.drop_resolved(&mut new);
        let mut dropped = 0;
        let imports = std::mem::take(&mut new.imports);
        let room = MAX_ENTRIES.saturating_sub(self.entries);
        self.entries += self.imports.merge(imports, room, &mut dropped);
        for prefix in std::mem::take(&mut new.scope_order) {
            let Some(map) = new.scopes.remove(&prefix) else {
                continue;
            };
            let room = MAX_ENTRIES.saturating_sub(self.entries);
            // A scope with no kept rule is not kept: it would make every
            // resolution walk the prefixes of the referrer for nothing.
            if let Some(scope) = self.scopes.get_mut(&prefix) {
                self.entries += scope.merge(map, room, &mut dropped);
            } else {
                let mut scope = SpecifierMap::default();
                self.entries += scope.merge(map, room, &mut dropped);
                if !scope.entries.is_empty() {
                    self.scopes.insert(prefix, scope);
                }
            }
        }
        if dropped > 0 && !std::mem::replace(&mut self.limit_warned, true) {
            log::warn!(
                "the import map has {MAX_ENTRIES} entries, the limit: later entries are ignored"
            );
        }
    }

    /// The first step of HTML "merge existing and new import maps": the
    /// rules of `new` that would change the result for a specifier that the
    /// document resolved are removed.
    fn drop_resolved(&mut self, new: &mut ParsedMap) {
        let rules = new.imports.entries.len() + new.scopes.len();
        if self.resolved.is_empty() || rules == 0 {
            return;
        }
        // A page that registers many maps pays for each record each time:
        // the work of the merges of one document has a budget.
        if self.merge_work >= MAX_MERGE_WORK {
            if !std::mem::replace(&mut self.work_warned, true) {
                log::warn!(
                    "import maps: the merges of this document used their budget; new rules are \
                     not checked against the resolved specifiers any more"
                );
            }
            return;
        }
        self.merge_work += self.resolved.len();
        let mut budget = MAX_LOOKUP_BYTES * 16;
        for (base, specifier) in &self.resolved {
            if !new.scopes.is_empty() {
                // The base prefixes are charged to the budget of the merge.
                let matching: Vec<&str> = prefixes(base, true, &mut budget)
                    .filter(|prefix| new.scopes.contains_key(*prefix))
                    .collect();
                for prefix in matching {
                    if let Some(scope) = new.scopes.get_mut(prefix) {
                        scope.remove_resolved(specifier, &mut budget);
                    }
                }
            }
            if !new.imports.entries.is_empty() {
                new.imports.remove_resolved(specifier, &mut budget);
            }
        }
    }

    /// HTML "resolve a module specifier" through the map for a specifier
    /// `specifier` against the base URL `base`: the scopes whose prefix
    /// matches `base`, longest first, then the top-level imports, then the
    /// URL-like parse. A result is added to the resolved module set.
    pub(crate) fn resolve(&mut self, specifier: &str, base: &Url) -> Resolved {
        let as_url = resolve_url_like(specifier, base);
        let normalized = as_url
            .as_ref()
            .map_or_else(|| specifier.to_owned(), ToString::to_string);
        let mut found = None;
        if !self.is_empty() {
            match self.find(&normalized, as_url.as_ref(), base.as_str()) {
                Ok(address) => found = address,
                Err(blocked) => return Resolved::Blocked(blocked),
            }
        }
        let Some(url) = found.or(as_url) else {
            return Resolved::None;
        };
        self.record_resolved(base, normalized);
        Resolved::Url(url)
    }

    /// Adds the pair to the resolved module set unless the set is full
    /// ([`MAX_RESOLVED`], [`MAX_RESOLVED_BYTES`]) or the pair is longer than
    /// [`MAX_RESOLVED_RECORD_BYTES`].
    fn record_resolved(&mut self, base: &Url, normalized: String) {
        let bytes = base.as_str().len() + normalized.len();
        if bytes > MAX_RESOLVED_RECORD_BYTES
            || self.resolved.len() >= MAX_RESOLVED
            || self.resolved_bytes + bytes > MAX_RESOLVED_BYTES
        {
            return;
        }
        if self.resolved.insert((base.to_string(), normalized)) {
            self.resolved_bytes += bytes;
        }
    }

    /// The address that the map gives for `normalized`.
    fn find(
        &self,
        normalized: &str,
        as_url: Option<&Url>,
        base: &str,
    ) -> Result<Option<Url>, Blocked> {
        let mut budget = MAX_LOOKUP_BYTES;
        if !self.scopes.is_empty() {
            let mut scope_budget = MAX_LOOKUP_BYTES;
            for prefix in prefixes(base, true, &mut scope_budget) {
                let Some(scope) = self.scopes.get(prefix) else {
                    continue;
                };
                if let Some(address) = scope.lookup(normalized, as_url, &mut budget)? {
                    return Ok(Some(address));
                }
            }
        }
        self.imports.lookup(normalized, as_url, &mut budget)
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    fn base() -> Url {
        Url::parse("https://example.com/app/index.html").unwrap()
    }

    fn registered(texts: &[&str]) -> ImportMap {
        let mut map = ImportMap::default();
        for text in texts {
            map.register(parse(text, &base()).unwrap());
        }
        map
    }

    fn url(map: &mut ImportMap, specifier: &str) -> String {
        url_from(map, specifier, &base())
    }

    fn url_from(map: &mut ImportMap, specifier: &str, from: &Url) -> String {
        match map.resolve(specifier, from) {
            Resolved::Url(url) => url.to_string(),
            Resolved::None => "none".to_owned(),
            Resolved::Blocked(blocked) => blocked.text(),
        }
    }

    // The examples of the HTML Standard, "import maps" (the sections
    // "Example import maps" of the standard).

    #[test]
    fn bare_specifiers_and_prefixes() {
        let mut map = registered(&[r#"{"imports": {
            "lodash": "/node_modules/lodash-es/lodash.js",
            "lodash/": "/node_modules/lodash-es/",
            "moment": "/node_modules/moment/src/moment.js"
        }}"#]);
        assert_eq!(
            url(&mut map, "lodash"),
            "https://example.com/node_modules/lodash-es/lodash.js"
        );
        assert_eq!(
            url(&mut map, "lodash/fp.js"),
            "https://example.com/node_modules/lodash-es/fp.js"
        );
        assert_eq!(url(&mut map, "unknown"), "none");
        // A relative URL is not mapped when no key matches.
        assert_eq!(url(&mut map, "./lodash"), "https://example.com/app/lodash");
    }

    #[test]
    fn url_like_keys_map_urls() {
        // "Remapping URLs": the key is resolved against the map's base URL.
        let mut map = registered(&[r#"{"imports": {
            "https://www.unpkg.com/vue/dist/vue.runtime.esm.js": "/node_modules/vue/dist/vue.runtime.esm.js",
            "./a.js": "./b.js"
        }}"#]);
        assert_eq!(
            url(
                &mut map,
                "https://www.unpkg.com/vue/dist/vue.runtime.esm.js"
            ),
            "https://example.com/node_modules/vue/dist/vue.runtime.esm.js"
        );
        assert_eq!(url(&mut map, "./a.js"), "https://example.com/app/b.js");
        // From another directory the same relative specifier is another URL.
        let other = Url::parse("https://example.com/other/x.js").unwrap();
        assert_eq!(
            url_from(&mut map, "./a.js", &other),
            "https://example.com/other/a.js"
        );
        // The key was the absolute URL of ./a.js in /app/.
        assert_eq!(
            url_from(&mut map, "/app/a.js", &other),
            "https://example.com/app/b.js"
        );
    }

    #[test]
    fn scopes_override_the_top_level_by_the_longest_prefix() {
        // "Scoping": the most specific scope first, then the less specific
        // ones, then the top-level imports.
        let mut map = registered(&[r#"{
            "imports": {"a": "/a-1.mjs", "b": "/b-1.mjs", "c": "/c-1.mjs"},
            "scopes": {
                "/scope2/": {"a": "/a-2.mjs"},
                "/scope2/scope3/": {"b": "/b-3.mjs"}
            }
        }"#]);
        let at = |path: &str| Url::parse(&format!("https://example.com{path}")).unwrap();
        let cases = [
            ("/", "a", "/a-1.mjs"),
            ("/", "b", "/b-1.mjs"),
            ("/", "c", "/c-1.mjs"),
            ("/scope2/", "a", "/a-2.mjs"),
            ("/scope2/", "b", "/b-1.mjs"),
            ("/scope2/", "c", "/c-1.mjs"),
            ("/scope2/scope3/", "a", "/a-2.mjs"),
            ("/scope2/scope3/", "b", "/b-3.mjs"),
            ("/scope2/scope3/", "c", "/c-1.mjs"),
            ("/scope2/scope3/deep/x.js", "b", "/b-3.mjs"),
            ("/scope2", "a", "/a-1.mjs"),
        ];
        for (from, specifier, expected) in cases {
            assert_eq!(
                url_from(&mut map, specifier, &at(from)),
                format!("https://example.com{expected}"),
                "{specifier} from {from}"
            );
        }
    }

    #[test]
    fn a_scope_matches_the_exact_url_without_a_slash() {
        let mut map = registered(&[r#"{"scopes": {"/app/index.html": {"x": "/exact.js"}}}"#]);
        assert_eq!(url(&mut map, "x"), "https://example.com/exact.js");
        let other = Url::parse("https://example.com/app/index.html2").unwrap();
        assert_eq!(url_from(&mut map, "x", &other), "none");
    }

    #[test]
    fn invalid_addresses_and_keys_are_ignored_or_blocked() {
        let mut map = registered(&[r#"{"imports": {
            "a": 1, "b": "bare", "c/": "/c", "": "/empty.js", "d": null,
            "e": ["/e.js"], "f": "https://f.test/f.js"
        }}"#]);
        for key in ["a", "b", "d", "e"] {
            assert_eq!(
                url(&mut map, key),
                format!(
                    "Import Map: \"{key}\" matches with \"{key}\" but is blocked by a null value"
                )
            );
        }
        assert_eq!(
            url(&mut map, "c/x"),
            "Import Map: \"c/x\" matches with \"c/\" but is blocked by a null value"
        );
        assert_eq!(url(&mut map, "f"), "https://f.test/f.js");
        assert_eq!(map.len(), 6);
    }

    #[test]
    fn a_prefix_match_cannot_leave_the_address() {
        let mut map = registered(&[r#"{"imports": {"p/": "/dir/"}}"#]);
        assert_eq!(url(&mut map, "p/a/b.js"), "https://example.com/dir/a/b.js");
        assert_eq!(
            url(&mut map, "p/../x.js"),
            "Import Map: \"p/../x.js\" matches with \"p/\" but is blocked due to backtracking"
        );
        assert_eq!(url(&mut map, "p/a/../b.js"), "https://example.com/dir/b.js");
    }

    #[test]
    fn the_longest_prefix_and_the_exact_key_win() {
        let mut map = registered(&[r#"{"imports": {
            "m/": "/m/", "m/n/": "/mn/", "m/n": "/exact.js", "m/n/z": "/z.js"
        }}"#]);
        assert_eq!(url(&mut map, "m/x"), "https://example.com/m/x");
        assert_eq!(url(&mut map, "m/n/y"), "https://example.com/mn/y");
        assert_eq!(url(&mut map, "m/n/z"), "https://example.com/z.js");
        assert_eq!(url(&mut map, "m/n"), "https://example.com/exact.js");
        assert_eq!(url(&mut map, "m/n/"), "https://example.com/mn/");
    }

    #[test]
    fn prefix_keys_do_not_match_urls_with_other_schemes() {
        // A key that ends in `/` matches only specifiers that are not URLs
        // or have a special scheme.
        let mut map =
            registered(&[r#"{"imports": {"data:text/": "/t/", "https://h.test/": "/h/"}}"#]);
        assert_eq!(url(&mut map, "data:text/x"), "data:text/x");
        assert_eq!(url(&mut map, "https://h.test/q"), "https://example.com/h/q");
    }

    #[test]
    fn json_errors_are_reported_with_chromiums_messages() {
        let message = |text: &str| parse(text, &base()).unwrap_err().message();
        assert_eq!(message("{"), "Failed to parse import map: invalid JSON");
        assert_eq!(message("  "), "Failed to parse import map: invalid JSON");
        assert_eq!(message("{} x"), "Failed to parse import map: invalid JSON");
        assert_eq!(message("[]"), "Failed to parse import map: not an object");
        assert_eq!(message("null"), "Failed to parse import map: not an object");
        assert_eq!(
            message(r#"{"imports": []}"#),
            "Failed to parse import map: \"imports\" top-level key must be a JSON object."
        );
        assert_eq!(
            message(r#"{"scopes": 1}"#),
            "Failed to parse import map: \"scopes\" top-level key must be a JSON object."
        );
        assert_eq!(
            message(r#"{"scopes": {"./a/": 1}}"#),
            "Failed to parse import map: the value of the scope with prefix \"./a/\" must be a JSON object."
        );
        assert!(parse(r#"{"scopes": {"http://[bad": {}}, "other": 1}"#, &base()).is_ok());
    }

    #[test]
    fn a_key_of_an_earlier_map_stays() {
        let mut map = registered(&[
            r#"{"imports": {"x": "/x1.js"}}"#,
            r#"{"imports": {"x": "/x2.js", "y": "/y.js"}}"#,
        ]);
        assert_eq!(url(&mut map, "x"), "https://example.com/x1.js");
        assert_eq!(url(&mut map, "y"), "https://example.com/y.js");
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn scopes_merge_by_prefix() {
        let mut map = registered(&[
            r#"{"scopes": {"/app/": {"x": "/s1.js"}}}"#,
            r#"{"scopes": {"/app/": {"x": "/s2.js", "y": "/s3.js"}}}"#,
        ]);
        assert_eq!(url(&mut map, "x"), "https://example.com/s1.js");
        assert_eq!(url(&mut map, "y"), "https://example.com/s3.js");
    }

    #[test]
    fn a_new_rule_for_a_resolved_specifier_is_dropped() {
        // HTML's merge: "./a.js" resolved from /app/index.html; a later map
        // cannot change it, in the imports or in a scope; a prefix key that
        // covers it goes too; a rule for another specifier stays.
        let mut map = ImportMap::default();
        assert_eq!(url(&mut map, "./a.js"), "https://example.com/app/a.js");
        // A failed resolution is not in the set.
        assert_eq!(url(&mut map, "lib/x.js"), "none");
        assert_eq!(url(&mut map, "/x/y.js"), "https://example.com/x/y.js");
        map.register(
            parse(
                r#"{"imports": {"./a.js": "/b.js", "./d.js": "/e.js", "lib/": "/lib/",
                                "https://example.com/x/": "/other/", "https://example.com/x/z.js": "/z2.js"},
                    "scopes": {"/app/": {"./a.js": "/c.js", "./f.js": "/g.js"}}}"#,
                &base(),
            )
            .unwrap(),
        );
        assert_eq!(url(&mut map, "./a.js"), "https://example.com/app/a.js");
        assert_eq!(url(&mut map, "./d.js"), "https://example.com/e.js");
        assert_eq!(url(&mut map, "./f.js"), "https://example.com/g.js");
        // The prefix key covers the resolved `/x/y.js`: dropped; the exact
        // key of another URL stays; `lib/` was never resolved: stays.
        assert_eq!(url(&mut map, "/x/q.js"), "https://example.com/x/q.js");
        assert_eq!(url(&mut map, "/x/z.js"), "https://example.com/z2.js");
        assert_eq!(url(&mut map, "lib/z.js"), "https://example.com/lib/z.js");
    }

    #[test]
    fn the_entry_limit_ends_the_merge() {
        let mut imports = String::from("{\"imports\":{");
        for i in 0..MAX_ENTRIES + 10 {
            if i > 0 {
                imports.push(',');
            }
            write!(imports, "\"k{i}\":\"/v{i}.js\"").unwrap();
        }
        imports.push_str("}}");
        let mut map = registered(&[&imports]);
        assert_eq!(map.len(), MAX_ENTRIES);
        // The first entries stay, the last ones are ignored.
        assert_eq!(url(&mut map, "k0"), "https://example.com/v0.js");
        let last = MAX_ENTRIES + 9;
        assert_eq!(url(&mut map, &format!("k{last}")), "none");
        // A second map adds nothing.
        map.register(parse(r#"{"imports": {"later": "/later.js"}}"#, &base()).unwrap());
        assert_eq!(map.len(), MAX_ENTRIES);
        assert_eq!(url(&mut map, "later"), "none");
    }

    #[test]
    fn long_pairs_are_not_recorded_and_the_set_has_a_byte_budget() {
        let mut map = ImportMap::default();
        // A pair over the record limit is resolved but not recorded: a later
        // map's rule for it takes effect (a departure from HTML).
        let long = format!("./{}", "a".repeat(MAX_RESOLVED_RECORD_BYTES));
        assert!(url(&mut map, &long).ends_with(&"a".repeat(MAX_RESOLVED_RECORD_BYTES)));
        assert!(map.resolved.is_empty());
        assert_eq!(map.resolved_bytes, 0);
        map.register(parse(&format!(r#"{{"imports": {{"{long}": "/z.js"}}}}"#), &base()).unwrap());
        assert_eq!(url(&mut map, &long), "https://example.com/z.js");
        // Many pairs of 2 KiB stop at the byte budget, not at the count.
        let mut map = ImportMap::default();
        for i in 0..2000 {
            let specifier = format!("./{i}/{}", "b".repeat(2000));
            url(&mut map, &specifier);
        }
        assert!(map.resolved_bytes <= MAX_RESOLVED_BYTES);
        assert!(map.resolved.len() > 300 && map.resolved.len() < 2000);
        let counted: usize = map.resolved.iter().map(|(b, s)| b.len() + s.len()).sum();
        assert_eq!(counted, map.resolved_bytes);
    }

    #[test]
    fn exact_lookups_in_scopes_are_charged_to_the_budget() {
        // 1,000 nested scopes that match the referrer and a specifier that
        // is longer than the budget: only the first lookups hash it.
        let mut scopes = String::new();
        let mut path = String::from("/");
        for i in 0..1000 {
            path.push_str("a/");
            if i > 0 {
                scopes.push(',');
            }
            write!(scopes, "\"{path}\": {{\"x\": \"/s.js\"}}").unwrap();
        }
        let specifier = "s".repeat(MAX_LOOKUP_BYTES + MAX_LOOKUP_BYTES / 2);
        let text =
            format!("{{\"imports\": {{\"{specifier}\": \"/top.js\"}}, \"scopes\": {{{scopes}}}}}");
        let from = Url::parse(&format!("https://example.com{path}x.js")).unwrap();
        let mut map = registered(&[&text]);
        assert_eq!(map.scopes.len(), 1000);
        // The budget is gone after the first lookup: the top-level key is
        // not hashed either (hostile input; no match).
        assert_eq!(url_from(&mut map, &specifier, &from), "none");
        // Without scopes the lookup is made once.
        let mut plain = registered(&[&text.replace("\"scopes\"", "\"ignored\"")]);
        assert_eq!(url(&mut plain, &specifier), "https://example.com/top.js");
        // The charge itself.
        let mut budget = 100;
        let map = SpecifierMap::default();
        assert_eq!(map.lookup("abc", None, &mut budget), Ok(None));
        assert_eq!(budget, 97);
    }

    #[test]
    fn the_merge_shares_one_budget_for_the_base_prefixes() {
        // 100 resolved pairs with a deep base: each walk of the prefixes costs
        // about 260 KB. The budget of the merge (16 MiB) ends before all of
        // them reach the scope; the rules of the later ones stay.
        let deep = format!("https://example.com/{}", "a/".repeat(500));
        let mut map = ImportMap::default();
        let mut rules = String::new();
        for i in 0..100 {
            map.resolved
                .insert((format!("{deep}x.js"), format!("s{i}")));
            if i > 0 {
                rules.push(',');
            }
            write!(rules, "\"s{i}\": \"/x.js\"").unwrap();
        }
        let text = format!("{{\"scopes\": {{\"/\": {{{rules}}}}}}}");
        let mut new = parse(&text, &base()).unwrap();
        map.drop_resolved(&mut new);
        let left = new
            .scopes
            .values()
            .map(|scope| scope.entries.len())
            .sum::<usize>();
        assert!(left > 0 && left < 100, "{left} rules left");
    }

    #[test]
    fn a_scope_without_a_kept_rule_is_not_kept() {
        let mut map = ImportMap::default();
        assert_eq!(url(&mut map, "./a.js"), "https://example.com/app/a.js");
        // Its only rule is dropped: the scope is not kept.
        map.register(parse(r#"{"scopes": {"/app/": {"./a.js": "/c.js"}}}"#, &base()).unwrap());
        assert!(map.scopes.is_empty());
        assert!(map.is_empty());
        // An empty scope, too.
        map.register(parse(r#"{"scopes": {"/x/": {}}}"#, &base()).unwrap());
        assert!(map.scopes.is_empty());
        // A scope with a rule stays.
        map.register(parse(r#"{"scopes": {"/x/": {"y": "/y.js"}}}"#, &base()).unwrap());
        assert_eq!(map.scopes.len(), 1);
    }

    #[test]
    fn a_hostile_specifier_ends_the_prefix_search() {
        let mut map = registered(&[r#"{"imports": {"a/": "/a/"}}"#]);
        let specifier = "/".repeat(2_000_000);
        // Not a bare specifier: URL-like, resolved against the base.
        assert!(matches!(
            map.resolve(&format!("x{specifier}"), &base()),
            Resolved::None
        ));
    }
}
