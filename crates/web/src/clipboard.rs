//! `navigator.clipboard` (Clipboard API,
//! <https://w3c.github.io/clipboard-apis/#clipboard-interface>; ADR 0028
//! section 13, feature 11c2): `readText()` and `writeText()`.
//!
//! swb has no system clipboard and no permission prompt. In Chromium the
//! outcome of both methods depends on the permission that the embedder
//! gave (`clipboard-read`, `clipboard-write`), and not on user activation
//! or on the focus of the document. Measured with Playwright, on
//! `http://127.0.0.1`, `https://` and the `file:` scheme:
//!
//! | Permission | `writeText(s)` | `readText()` |
//! |---|---|---|
//! | none (the default) | rejects `NotAllowedError`, "Failed to execute 'writeText' on 'Clipboard': Write permission denied." | rejects `NotAllowedError`, "Failed to execute 'readText' on 'Clipboard': Read permission denied." |
//! | granted | resolves `undefined` | resolves the text of the last write |
//!
//! The same holds with and without a click and with and without focus. swb
//! has no permission model, so it gives the default: both methods reject
//! with Chromium's messages, which is the browser the fixtures were
//! measured in. A page that detects `navigator.clipboard.writeText` (the
//! Craigslist share menu and reply panel do) then shows its copy button
//! and, as in Chromium under the probe tool, gets an unhandled rejection
//! when it does not wait for the promise. The argument checks come first,
//! as in Chromium: `writeText()` rejects with a `TypeError`.
//!
//! The Clipboard interface has `[SecureContext]`, which the generator does
//! not support: swb exposes it on every origin. Chromium hides
//! `navigator.clipboard`, `Clipboard` and `ClipboardItem` on an insecure
//! origin (a non-local `http:` origin; `http://127.0.0.1`, `file:` and
//! `https:` are secure contexts).

use swb_js::{Gc, Object};

use crate::bindings::{self as b, Cx, Platform};
use crate::generated as g;
use crate::state::{WebState, state, state_mut};

impl g::clipboard::ClipboardImpl for WebState {
    fn read_text(_cx: &mut Cx<'_>, _this: Platform) -> b::Result<Gc<Object>> {
        Err(b::Exception::dom(
            "NotAllowedError",
            "Read permission denied.",
        ))
    }

    fn write_text(_cx: &mut Cx<'_>, _this: Platform, _data: String) -> b::Result<Gc<Object>> {
        Err(b::Exception::dom(
            "NotAllowedError",
            "Write permission denied.",
        ))
    }
}

/// The `navigator.clipboard` attribute ([`SameObject`]): one object per
/// window. An `EventTarget` keeps its listeners in its data.
pub(crate) fn clipboard_object(cx: &mut Cx<'_>) -> b::Result<Gc<Object>> {
    if let Some(object) = state(cx.rt)?.clipboard {
        return Ok(object);
    }
    let data = cx
        .rt
        .alloc_host_data(crate::events::TargetData::default())?;
    let object = cx.rt.new_host_object(cx.realm(), g::CLIPBOARD, data)?;
    state_mut(cx.rt)?.clipboard = Some(object);
    Ok(object)
}
