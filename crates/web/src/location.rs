//! `Location` and `History` (HTML §7.4,
//! <https://html.spec.whatwg.org/multipage/nav-history-apis.html>).
//!
//! `window.location` and `document.location` are one object. Reads come
//! from the URL of the document (`EngineHost::url`). A write makes a
//! *request* that the page carries out after the task, because a script
//! task has no page (ADR 0028 section 13): a navigation
//! (`EngineHost::request_navigation`), a reload or a traversal of the
//! session history. A navigation that changes only the fragment is a
//! navigation to a fragment: the page scrolls, adds an entry and fires
//! `hashchange` (`engine/src/page`). `pushState()` and `replaceState()` act
//! at once: the document's URL and `history.state` change in the task, and
//! the page adds or replaces the entry afterwards.
//!
//! Departures from Chromium: `Location.ancestorOrigins` does not exist
//! (no frames); see the notes of ADR 0028 section 13. `history.length`
//! counts at most 50 entries, as Chromium does.

use std::rc::Rc;

use swb_js::{Gc, GenericData, Object, Tracer, Value};
use url::Url;

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::event_loop::Task;
use crate::events::{DispatchTarget, EventInterface, TrustedEvent, fire_trusted};
use crate::generated as g;
use crate::generated::types::ScrollRestoration;
use crate::scripts::scripts;
use crate::state::{NOT_A_NODE, WebState, engine, engine_mut, state, state_mut};
use crate::structured::{self, Serialized};
use crate::url_api::{get, set};

/// The host data of `Location` and `History` objects: they carry nothing.
struct NoData;

impl GenericData for NoData {
    fn trace(&self, _tracer: &mut Tracer<'_>) {}

    fn heap_size(&self) -> usize {
        0
    }
}

/// The deserialized `history.state` and the record it came from: the
/// object stays the same while the entry's state does (`history.state ===
/// history.state`).
pub(crate) struct StateCache {
    record: Rc<Serialized>,
    value: Value,
}

impl StateCache {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        tracer.value(self.value);
    }
}

/// The value of a history entry's state: the same object while the entry's
/// record is the same one, so `history.state === history.state` and the
/// `state` of a `popstate` event is the `history.state` that follows it.
pub(crate) fn state_value(
    rt: &mut swb_js::Runtime,
    realm: u32,
    record: &Rc<Serialized>,
) -> b::Result<Value> {
    if let Some(cache) = &state(rt)?.history_state
        && Rc::ptr_eq(&cache.record, record)
    {
        return Ok(cache.value);
    }
    let value = structured::deserialize(rt, realm, record)?;
    state_mut(rt)?.history_state = Some(StateCache {
        record: Rc::clone(record),
        value,
    });
    Ok(value)
}

/// The one `Location` object of the window.
pub(crate) fn location_object(cx: &mut Cx<'_>) -> b::Result<Gc<Object>> {
    if let Some(object) = state(cx.rt)?.location {
        return Ok(object);
    }
    let realm = cx.realm();
    let object = cx.rt.new_host_object(realm, g::LOCATION, NOT_A_NODE)?;
    b::glue::install_unforgeables::<WebState>(cx.rt, realm, g::LOCATION, object)?;
    // Web IDL / HTML: `Object.setPrototypeOf(location, x)` throws.
    cx.rt.set_immutable_prototype(object)?;
    state_mut(cx.rt)?.location = Some(object);
    Ok(object)
}

/// The one `History` object of the window.
pub(crate) fn history_object(cx: &mut Cx<'_>) -> b::Result<Gc<Object>> {
    if let Some(object) = state(cx.rt)?.history {
        return Ok(object);
    }
    let data = cx.rt.alloc_host_data(NoData)?;
    let object = cx.rt.new_host_object(cx.realm(), g::HISTORY, data)?;
    state_mut(cx.rt)?.history = Some(object);
    Ok(object)
}

/// The URL of the document.
fn document_url(cx: &Cx<'_>) -> b::Result<Url> {
    Url::parse(&engine(cx.rt)?.url())
        .map_err(|_| Exception::dom("SecurityError", "The document has no valid URL."))
}

/// `value` parsed against the document base URL (HTML "encoding-parse a
/// URL"), as an absolute URL.
fn parse_against_base(cx: &mut Cx<'_>, value: &str) -> b::Result<Option<Url>> {
    Ok(engine_mut(cx.rt)?
        .parse_url(value)
        .and_then(|text| Url::parse(&text).ok()))
}

/// HTML "Location-object navigate": asks the page to navigate when the
/// task ends. A navigation to a fragment of the document is done at once
/// (the URL, `history.length`) and fires `popstate` now and `hashchange` as
/// a task, as Chromium does (measured). A `javascript:` URL queues its
/// script as a task.
fn navigate(cx: &mut Cx<'_>, url: &Url, replace: bool) -> b::Result<()> {
    if let Some(source) = crate::html::activation::javascript_source(url.as_str()) {
        crate::html::activation::queue_javascript_url(cx.rt, source)?;
        return Ok(());
    }
    // HTML "Location-object navigate": before the document is completely
    // loaded (its `load` event has finished), the navigation replaces the
    // current entry. Measured in Chromium: inline scripts, `DOMContentLoaded`
    // and `load` handlers replace; a task after the `load` event pushes.
    let replace = replace || !scripts(cx.rt)?.completely_loaded();
    let old = engine(cx.rt)?.url();
    if engine_mut(cx.rt)?.request_navigation(url.as_str(), replace) {
        let new = engine(cx.rt)?.url();
        if old == new {
            // HTML "navigate": the URL of the document is a replacement,
            // and the fragment did not change, so no `hashchange`. The
            // entry keeps its state, but `popstate` has a null `state`
            // (Chromium, measured).
            navigated_within_document(cx.rt, None, None)?;
        } else {
            navigated_within_document(cx.rt, None, Some((old, new)))?;
        }
    }
    Ok(())
}

/// The events of a navigation within the document: `popstate` now, with
/// the state of the entry that became current, and `hashchange` as a task
/// if the fragment changed.
pub(crate) fn navigated_within_document(
    rt: &mut swb_js::Runtime,
    state: Option<Rc<Serialized>>,
    hash_change: Option<(String, String)>,
) -> swb_js::VmResult<()> {
    let mut event = TrustedEvent::new("popstate");
    event.interface = EventInterface::PopState { state };
    fire_trusted(rt, DispatchTarget::Window, &event)?;
    if let Some((old_url, new_url)) = hash_change {
        crate::event_loop::queue_task(rt, Task::HashChange { old_url, new_url })?;
    }
    Ok(())
}

/// Fires `hashchange` at the window (the task of a navigation to a
/// fragment).
pub(crate) fn fire_hash_change(
    rt: &mut swb_js::Runtime,
    old_url: String,
    new_url: String,
) -> swb_js::VmResult<()> {
    let mut event = TrustedEvent::new("hashchange");
    event.interface = EventInterface::HashChange { old_url, new_url };
    fire_trusted(rt, DispatchTarget::Window, &event).map(drop)
}

/// The setters other than `href` and `hash`: a copy of the document's URL,
/// changed, then a navigation.
fn change(cx: &mut Cx<'_>, apply: impl FnOnce(&mut Url)) -> b::Result<()> {
    let mut copy = document_url(cx)?;
    apply(&mut copy);
    navigate(cx, &copy, false)
}

impl g::location::LocationImpl for WebState {
    fn href(cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(engine(cx.rt)?.url())
    }

    fn set_href(cx: &mut Cx<'_>, _this: Platform, value: String) -> b::Result<()> {
        assign_url(cx, &value, false)
    }

    fn origin(cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(get::origin(&document_url(cx)?))
    }

    fn protocol(cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(get::protocol(&document_url(cx)?))
    }

    fn set_protocol(cx: &mut Cx<'_>, _this: Platform, value: String) -> b::Result<()> {
        if !set::valid_scheme(&value) {
            return Err(Exception::dom(
                "SyntaxError",
                format!("'{value}' is an invalid protocol."),
            ));
        }
        change(cx, |url| set::protocol(url, &value))
    }

    fn host(cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(get::host(&document_url(cx)?))
    }

    fn set_host(cx: &mut Cx<'_>, _this: Platform, value: String) -> b::Result<()> {
        check_empty_host(cx, &value)?;
        change(cx, |url| set::host(url, &value))
    }

    fn hostname(cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(get::hostname(&document_url(cx)?))
    }

    fn set_hostname(cx: &mut Cx<'_>, _this: Platform, value: String) -> b::Result<()> {
        check_empty_host(cx, &value)?;
        change(cx, |url| set::hostname(url, &value))
    }

    fn port(cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(get::port(&document_url(cx)?))
    }

    fn set_port(cx: &mut Cx<'_>, _this: Platform, value: String) -> b::Result<()> {
        change(cx, |url| set::port(url, &value))
    }

    fn pathname(cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(document_url(cx)?.path().to_owned())
    }

    fn set_pathname(cx: &mut Cx<'_>, _this: Platform, value: String) -> b::Result<()> {
        change(cx, |url| set::pathname(url, &value))
    }

    fn search(cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(get::search(&document_url(cx)?))
    }

    fn set_search(cx: &mut Cx<'_>, _this: Platform, value: String) -> b::Result<()> {
        change(cx, |url| set::search(url, &value))
    }

    fn hash(cx: &mut Cx<'_>, _this: Platform) -> b::Result<String> {
        Ok(get::hash(&document_url(cx)?))
    }

    /// Step 7 of the `hash` setter: the same fragment is no navigation.
    fn set_hash(cx: &mut Cx<'_>, _this: Platform, value: String) -> b::Result<()> {
        let current = document_url(cx)?;
        let mut copy = current.clone();
        set::location_hash(&mut copy, &value);
        if copy.fragment() == current.fragment() {
            return Ok(());
        }
        navigate(cx, &copy, false)
    }

    fn assign(cx: &mut Cx<'_>, _this: Platform, url: String) -> b::Result<()> {
        assign_url(cx, &url, false)
    }

    fn replace(cx: &mut Cx<'_>, _this: Platform, url: String) -> b::Result<()> {
        assign_url(cx, &url, true)
    }

    fn reload(cx: &mut Cx<'_>, _this: Platform) -> b::Result<()> {
        engine_mut(cx.rt)?.request_reload();
        Ok(())
    }
}

/// An empty `host` or `hostname` of a document with a special (not
/// `file:`) URL: the standard's parser fails and the URL stays, but
/// Chromium (measured) throws a `SyntaxError` that names the URL without a
/// host. The text is `http://:PORT/path?query#fragment`.
fn check_empty_host(cx: &mut Cx<'_>, value: &str) -> b::Result<()> {
    if !value.is_empty() {
        return Ok(());
    }
    let url = document_url(cx)?;
    let special = matches!(url.scheme(), "http" | "https" | "ws" | "wss" | "ftp");
    if !special {
        return Ok(());
    }
    let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
    let query = url.query().map(|q| format!("?{q}")).unwrap_or_default();
    let fragment = url.fragment().map(|f| format!("#{f}")).unwrap_or_default();
    Err(Exception::dom(
        "SyntaxError",
        format!(
            "'{}://{port}{}{query}{fragment}' is not a valid URL.",
            url.scheme(),
            url.path()
        ),
    ))
}

/// `location.href = value`, `assign` and `replace`: a URL parse that fails
/// is a `SyntaxError`.
fn assign_url(cx: &mut Cx<'_>, value: &str, replace: bool) -> b::Result<()> {
    match parse_against_base(cx, value)? {
        Some(url) => navigate(cx, &url, replace),
        None => Err(Exception::dom(
            "SyntaxError",
            format!("'{value}' is not a valid URL."),
        )),
    }
}

// ----- History -----

/// The `SecurityError` of a URL that `pushState()` may not set.
fn security_error(url: &str, document: &Url) -> Exception {
    // An opaque origin serializes as `null` (HTML "serialization of an
    // origin"), and so does the origin of a `file:` document here, although
    // `location.origin` of a `file:` URL is `file://` (Chromium, measured).
    let origin =
        if document.scheme() == "file" || matches!(document.origin(), url::Origin::Opaque(_)) {
            "null".to_owned()
        } else {
            get::origin(document)
        };
    Exception::dom(
        "SecurityError",
        format!(
            "A history state object with URL '{url}' cannot be created in a document with origin '{origin}' and URL '{document}'."
        ),
    )
}

/// Chromium's rule for the URL of `pushState()` and `replaceState()`:
/// scheme, host and port stay (no credentials); a document with an opaque
/// origin or a `file:` URL may change only the query and the fragment.
fn may_change_to(document: &Url, url: &Url) -> bool {
    if document == url {
        return true;
    }
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    if document.scheme() != url.scheme() {
        return false;
    }
    let opaque = matches!(document.origin(), url::Origin::Opaque(_)) || document.scheme() == "file";
    let strip = |u: &Url| {
        let mut u = u.clone();
        u.set_query(None);
        u.set_fragment(None);
        u
    };
    if opaque {
        return strip(document) == strip(url);
    }
    document.host() == url.host() && document.port() == url.port()
}

/// HTML "shared history push/replace state steps".
fn push_or_replace(
    cx: &mut Cx<'_>,
    data: Value,
    url: Option<String>,
    replace: bool,
) -> b::Result<()> {
    let record = structured::serialize_state(cx.rt, data)?;
    let document = document_url(cx)?;
    // An empty URL is no URL, as Chromium has it (measured: the fragment
    // stays).
    let new_url = match url.filter(|text| !text.is_empty()) {
        Some(text) => {
            let parsed = parse_against_base(cx, &text)?;
            match parsed {
                Some(parsed) if may_change_to(&document, &parsed) => Some(parsed),
                // Chromium names the URL as it parsed it; swb, when the
                // text does not parse, as it was written.
                Some(parsed) => return Err(security_error(parsed.as_str(), &document)),
                None => return Err(security_error(&text, &document)),
            }
        }
        None => None,
    };
    engine_mut(cx.rt)?.push_state(record, new_url.as_ref().map(Url::as_str), replace);
    Ok(())
}

impl g::history::HistoryImpl for WebState {
    fn length(cx: &mut Cx<'_>, _this: Platform) -> b::Result<u32> {
        Ok(u32::try_from(engine(cx.rt)?.history_length()).unwrap_or(u32::MAX))
    }

    fn scroll_restoration(cx: &mut Cx<'_>, _this: Platform) -> b::Result<ScrollRestoration> {
        Ok(if engine(cx.rt)?.scroll_restoration_manual() {
            ScrollRestoration::Manual
        } else {
            ScrollRestoration::Auto
        })
    }

    fn set_scroll_restoration(
        cx: &mut Cx<'_>,
        _this: Platform,
        value: ScrollRestoration,
    ) -> b::Result<()> {
        engine_mut(cx.rt)?.set_scroll_restoration(value == ScrollRestoration::Manual);
        Ok(())
    }

    /// The state of the current entry: the same object until the state
    /// changes.
    fn state(cx: &mut Cx<'_>, _this: Platform) -> b::Result<Value> {
        let Some(record) = engine(cx.rt)?.history_state() else {
            return Ok(Value::Null);
        };
        state_value(cx.rt, cx.realm(), &record)
    }

    fn go(cx: &mut Cx<'_>, _this: Platform, delta: i32) -> b::Result<()> {
        engine_mut(cx.rt)?.request_traversal(delta);
        Ok(())
    }

    fn back(cx: &mut Cx<'_>, _this: Platform) -> b::Result<()> {
        engine_mut(cx.rt)?.request_traversal(-1);
        Ok(())
    }

    fn forward(cx: &mut Cx<'_>, _this: Platform) -> b::Result<()> {
        engine_mut(cx.rt)?.request_traversal(1);
        Ok(())
    }

    fn push_state(
        cx: &mut Cx<'_>,
        _this: Platform,
        data: Value,
        _unused: String,
        url: Option<String>,
    ) -> b::Result<()> {
        push_or_replace(cx, data, url, false)
    }

    fn replace_state(
        cx: &mut Cx<'_>,
        _this: Platform,
        data: Value,
        _unused: String,
        url: Option<String>,
    ) -> b::Result<()> {
        push_or_replace(cx, data, url, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(url: &str, document: &str) -> String {
        match security_error(url, &Url::parse(document).unwrap()) {
            Exception::Dom { name, message } => {
                assert_eq!(name, "SecurityError");
                message.into_owned()
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn opaque_and_file_origins_are_null_in_the_security_error() {
        assert!(message("x", "file:///a/b.html").contains("origin 'null'"));
        assert!(message("x", "data:text/html,a").contains("origin 'null'"));
        assert!(message("x", "http://h:81/a").contains("origin 'http://h:81'"));
    }
}
