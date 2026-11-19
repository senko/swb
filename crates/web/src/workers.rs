//! A stand-in for dedicated workers (HTML,
//! <https://html.spec.whatwg.org/multipage/workers.html#dedicated-workers-and-the-worker-interface>;
//! M12 feature 2).
//!
//! swb has no workers: a `Worker` checks its arguments as Chromium does,
//! and then it does nothing. The script is not fetched and does not run,
//! `postMessage()` serializes the message with its transfer list (so a
//! function is a `DataCloneError` and a transferred `ArrayBuffer` is
//! detached, as in Chromium) and drops it, and no event is ever fired at
//! the object. A page that makes a worker and waits for its answer waits
//! for ever; `MapLibre` makes a worker pool at load and uses it only for its
//! vector maps (`docs/osm-diagnosis.md` section 3), which is why a worker
//! that takes messages is enough for openstreetmap.org. The first worker of
//! a document is logged as a warning, because the page expects a script to
//! run and none does.
//!
//! Checks of the constructor (measured, probe case `worker-stand-in` in
//! `tools/probes/web-osm-gaps.json`): the URL is parsed against the document
//! base URL (`SyntaxError`: "'URL' is not a valid URL."), and its origin must
//! be the document's, except for a `data:` URL (`SecurityError`: "Script at
//! 'URL' cannot be accessed from origin 'ORIGIN'."; a `blob:` URL has the
//! origin of its inner URL). `SharedWorker` does not exist in swb: Craigslist
//! then takes its path for browsers without it.

use swb_js::{Gc, Object};

use crate::bindings::{self as b, Cx, Exception, Platform};
use crate::events::TargetData;
use crate::generated as g;
use crate::generated::types::{StructuredSerializeOptions, WorkerOptions};
use crate::state::{WebState, engine, engine_mut, state_mut};

/// "Run a worker" is not implemented, so a message goes nowhere: the
/// serialization and the detaching that HTML's `postMessage` does are the
/// only effects.
fn drop_message(cx: &mut Cx<'_>, message: swb_js::Value, transfer: &[Gc<Object>]) -> b::Result<()> {
    let record = crate::structured::serialize_for_transfer(cx.rt, message, transfer, None)?;
    crate::structured::detach_transfer(cx.rt, transfer)?;
    log::trace!(
        "Worker.postMessage: {} bytes dropped",
        record.message_bytes()
    );
    Ok(())
}

impl g::worker::WorkerImpl for WebState {
    /// <https://html.spec.whatwg.org/multipage/workers.html#dom-worker>
    fn constructor(
        cx: &mut Cx<'_>,
        script_url: String,
        options: WorkerOptions,
    ) -> b::Result<Gc<Object>> {
        let Some(resolved) = engine_mut(cx.rt)?.parse_url(&script_url) else {
            return Err(Exception::dom(
                "SyntaxError",
                format!("'{script_url}' is not a valid URL."),
            ));
        };
        let origin = engine(cx.rt)?.origin_text();
        if !may_load(&resolved, &origin) {
            return Err(Exception::security(format!(
                "Script at '{resolved}' cannot be accessed from origin '{origin}'."
            )));
        }
        let first = {
            let warned = &mut state_mut(cx.rt)?.doc.warned_no_workers;
            !std::mem::replace(warned, true)
        };
        if first {
            log::warn!(
                "new Worker({resolved:?}): swb has no workers; the script does not run and the worker never answers"
            );
        } else {
            log::debug!(
                "new Worker({resolved:?}): a stand-in (name {:?})",
                options.name
            );
        }
        let data = cx.rt.alloc_host_data(TargetData::default())?;
        Ok(cx.new_instance::<WebState>(data)?)
    }

    fn terminate(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<()> {
        Ok(())
    }

    fn post_message_1(
        cx: &mut Cx<'_>,
        _this: Platform,
        message: swb_js::Value,
        transfer: Vec<Gc<Object>>,
    ) -> b::Result<()> {
        drop_message(cx, message, &transfer)
    }

    fn post_message_2(
        cx: &mut Cx<'_>,
        _this: Platform,
        message: swb_js::Value,
        options: StructuredSerializeOptions,
    ) -> b::Result<()> {
        drop_message(cx, message, &options.transfer)
    }
}

/// True if a document of `origin` (serialized; `"null"` for an opaque one)
/// may make a worker of the script at `url` (serialized): the same origin,
/// or a `data:` URL.
fn may_load(url: &str, origin: &str) -> bool {
    let Ok(url) = url::Url::parse(url) else {
        return false;
    };
    if url.scheme() == "data" {
        return true;
    }
    let script_origin = url.origin().ascii_serialization();
    script_origin != "null" && script_origin == origin
}

#[cfg(test)]
mod tests {
    use super::may_load;

    #[test]
    fn the_script_must_have_the_origin_of_the_document() {
        let origin = "http://127.0.0.1:8000";
        assert!(may_load("http://127.0.0.1:8000/w.js", origin));
        assert!(may_load("blob:http://127.0.0.1:8000/1234", origin));
        assert!(may_load("data:text/javascript,1", origin));
        assert!(!may_load("http://127.0.0.1:8001/w.js", origin));
        assert!(!may_load("https://127.0.0.1:8000/w.js", origin));
        assert!(!may_load("blob:http://example.com/1234", origin));
        assert!(!may_load("javascript:1", origin));
        assert!(!may_load("about:blank", origin));
        assert!(!may_load("file:///w.js", origin));
        // An opaque origin has no script of its own origin.
        assert!(!may_load("file:///w.js", "null"));
        assert!(!may_load("http://127.0.0.1:8000/w.js", "null"));
        assert!(may_load("data:text/javascript,1", "null"));
    }
}
