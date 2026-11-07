//! The documents of a script runtime (ADR 0031 section 5): adding, reusing
//! and retiring them, the frames' proxies, and the questions that the page
//! asks about one document of the runtime by its key (`*_in`) or about any of
//! them.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use swb_js::{Runtime, VmError};

use super::{
    DocState, DocumentClock, DocumentKey, EngineHost, FrameHost, ScriptError, ScriptRuntime,
    init_window, internal, state_mut,
};

impl ScriptRuntime {
    /// Adds a document to the agent: a new realm with a `Window`, the record
    /// of the document, and the `WindowProxy` of the frame `frame` (below
    /// 2^30; one object per frame, ADR 0031 section 5). Returns the key of
    /// the document. The runtime's first document has the key 0.
    pub fn add_document(&mut self, frame: u32) -> Result<DocumentKey, ScriptError> {
        let clock = DocumentClock::new(self.clock.clone());
        let setup = |rt: &mut Runtime| -> Result<u32, VmError> {
            let realm = rt.create_realm()?;
            init_window(rt, realm)?;
            crate::state::add_document(rt, realm, frame, clock)
        };
        setup(&mut self.rt)
            .map(DocumentKey)
            .map_err(|e| internal(&e))
    }

    /// Shares the page's frame registry with the runtime (ADR 0031 section
    /// 5); `count` is the number of frames besides the top level, which the
    /// page keeps current.
    pub fn set_frame_host(&mut self, host: Rc<RefCell<dyn FrameHost>>, count: Rc<Cell<usize>>) {
        if let Ok(state) = state_mut(&mut self.rt) {
            state.frame_host = Some(host);
            state.frame_count = count;
        }
    }

    /// A new realm for a document of frame `frame` that the runtime will
    /// start ([`ScriptRuntime::start_document`] under
    /// [`ScriptRuntime::with_document`]); the frame's proxy gets its `Window`
    /// as the target. `None` past the budget of realms of ADR 0031 section 11
    /// (one warning), or after an error.
    pub fn add_frame_document(&mut self, frame: u32) -> Option<DocumentKey> {
        match crate::frames::new_realm(&mut self.rt, frame) {
            Ok(key) => key.map(DocumentKey),
            Err(error) => {
                log::warn!("cannot make the realm of a frame document: {error:?}");
                None
            }
        }
    }

    /// The `Window` of the document `old` goes on with a new document of its
    /// frame (HTML "initial about:blank" reuse): the new document has the
    /// returned key, and `old` keeps the wrappers of its nodes.
    pub fn reuse_window(&mut self, old: DocumentKey) -> Option<DocumentKey> {
        crate::state::reuse_window(&mut self.rt, old.0)
            .ok()
            .map(DocumentKey)
    }

    /// The document `key` left its frame: it stays while the realm lives. With
    /// `doc`, the engine gives up the document that it held.
    pub fn retire_document(&mut self, key: DocumentKey, doc: Option<Box<dyn EngineHost>>) {
        if let Err(error) = crate::state::retire_document(&mut self.rt, key.0, doc) {
            log::warn!("cannot retire a document: {error:?}");
        }
    }

    /// The frame `frame` is destroyed: nothing can ask for its proxy again.
    pub fn forget_frame(&mut self, frame: u32) {
        if let Ok(state) = state_mut(&mut self.rt) {
            state.window_proxies.remove(&frame);
        }
    }

    /// True if the runtime has seen the proxy of frame `frame`: a script may
    /// hold it, so a new document of the frame needs its realm at once.
    pub fn knows_frame(&self, frame: u32) -> bool {
        crate::state::state(&self.rt).is_ok_and(|s| s.window_proxies.contains_key(&frame))
    }

    /// The proxy of frame `frame` loses its target: the frame has a document
    /// of another origin, or one past the budget of realms.
    pub fn clear_frame_target(&mut self, frame: u32) {
        let proxy = crate::state::state(&self.rt)
            .ok()
            .and_then(|s| s.window_proxies.get(&frame).copied());
        if let Some(proxy) = proxy
            && let Err(error) = self.rt.set_window_proxy_target(proxy, None)
        {
            log::warn!("cannot change a WindowProxy: {error:?}");
        }
    }

    /// The document `key` got a realm with its parse complete: its load goes
    /// on without a parse of the runtime (`loading`: its `load` event is to
    /// come).
    pub fn adopt_complete_document(&mut self, key: DocumentKey, loading: bool) {
        if let Ok(state) = state_mut(&mut self.rt)
            && let Some(doc) = crate::state::document_mut(state, key.0)
        {
            doc.scripts.adopt_complete(loading);
        }
    }

    /// A sandbox without `allow-scripts` stops the scripts of the document
    /// `key`, by the flags of the navigation that made it (measured, case
    /// `scripted-sandbox-scripts`). A kept `Window` takes the new value.
    pub fn set_scripts_off(&mut self, key: DocumentKey, scripts_off: bool) {
        if let Ok(state) = state_mut(&mut self.rt)
            && let Some(doc) = crate::state::document_mut(state, key.0)
        {
            doc.scripts_off = scripts_off;
        }
    }

    /// Gives the document a realm at its first need, outside a native (an
    /// evaluation in a frame). False if it cannot have one.
    pub fn ensure_frame_window(&mut self, frame: u32) -> bool {
        matches!(
            crate::frames::window_of_frame(&mut self.rt, frame),
            Ok(Some(_))
        )
    }

    /// The current document gets its host from the registry if it has none. A
    /// runtime whose documents the page does not lend calls it at the start of a
    /// task, so that a native that runs in the current document finds it.
    pub fn borrow_current_document(&mut self) {
        crate::state::ensure_current_lent(&mut self.rt);
    }

    /// Takes back what the task borrowed besides the first document, which
    /// [`ScriptRuntime::take_back`] takes.
    pub fn take_back_documents(&mut self) -> Vec<Box<dyn EngineHost>> {
        let Ok(state) = state_mut(&mut self.rt) else {
            return Vec::new();
        };
        if state.lent.is_empty() {
            return Vec::new();
        }
        let keys = std::mem::take(&mut state.lent);
        keys.into_iter()
            .filter_map(|key| crate::state::document_mut(state, key)?.host.take())
            .collect()
    }

    /// Reads the state of the document `key`, without making it current.
    fn read_document<R>(&self, key: DocumentKey, read: impl FnOnce(&DocState) -> R) -> Option<R> {
        let state = crate::state::state(&self.rt).ok()?;
        crate::state::document_ref(state, key.0).map(read)
    }

    /// [`ScriptRuntime::load_event_due`] of the document `key`.
    pub fn load_event_due_in(&self, key: DocumentKey) -> bool {
        self.read_document(key, |d| {
            d.scripts.phase == crate::scripts::Phase::AfterContentLoaded
                && d.scripts.asap.is_empty()
                && d.scripts.in_order.is_empty()
        })
        .unwrap_or(false)
    }

    /// [`ScriptRuntime::document_loaded`] of the document `key`.
    pub fn document_loaded_in(&self, key: DocumentKey) -> bool {
        self.read_document(key, |d| {
            matches!(
                d.scripts.phase,
                crate::scripts::Phase::NotStarted
                    | crate::scripts::Phase::Loaded
                    | crate::scripts::Phase::Unloaded
            )
        })
        .unwrap_or(true)
    }

    /// [`ScriptRuntime::waits_for_style_sheets`] of the document `key`.
    pub fn waits_for_style_sheets_in(&self, key: DocumentKey) -> bool {
        self.read_document(key, |d| match d.scripts.phase {
            crate::scripts::Phase::Parsing => d.scripts.pending_parsing_blocking.is_some(),
            crate::scripts::Phase::Deferred => true,
            _ => false,
        })
        .unwrap_or(false)
    }

    /// True if the document `key` has animation frame callbacks.
    pub fn has_animation_frames_in(&self, key: DocumentKey) -> bool {
        !self.stopped
            && self
                .read_document(key, DocState::has_animation_frames)
                .unwrap_or(false)
    }

    /// True if the document `key` has an observation.
    pub fn has_observations_in(&self, key: DocumentKey) -> bool {
        !self.stopped
            && self
                .read_document(key, DocState::has_observations)
                .unwrap_or(false)
    }

    /// True if the window of the document `key` reacts to a new size of its
    /// viewport: a `resize` listener or handler, or a `MediaQueryList` with a
    /// listener.
    pub fn watches_viewport_in(&self, key: DocumentKey) -> bool {
        !self.stopped
            && self
                .read_document(key, |d| {
                    !d.media_queries.is_empty()
                        || d.window_listeners.has_kind("resize")
                        || d.handlers.has_window_handler("resize")
                })
                .unwrap_or(false)
    }

    /// True if the runtime has a document besides the first.
    pub fn has_several_documents(&self) -> bool {
        self.rt.host_flag()
    }

    /// Queues the script of a `javascript:` URL as a task of the current
    /// document (a navigation of a frame to it, ADR 0031 section 2).
    pub fn queue_javascript_url(&mut self, url: &str) {
        if let Some(source) = crate::html::activation::javascript_source(url)
            && let Err(error) = crate::html::activation::queue_javascript_url(&mut self.rt, source)
        {
            log::warn!("a javascript: URL was not queued: {error:?}");
        }
    }

    /// True if a document of the runtime holds this condition.
    pub(super) fn any_document(&self, test: impl Fn(&DocState) -> bool) -> bool {
        crate::state::state(&self.rt).is_ok_and(|s| test(&s.doc) || s.docs_iter().any(&test))
    }

    /// Defines the global `name` in the document `into` with the
    /// `WindowProxy` of the document `window`: for tests that have no page,
    /// so no frames, and need two documents that know each other.
    #[doc(hidden)]
    pub fn expose_window_for_tests(
        &mut self,
        into: DocumentKey,
        name: &str,
        window: DocumentKey,
    ) -> Result<(), ScriptError> {
        let define = |rt: &mut Runtime| -> Result<(), VmError> {
            let proxy = crate::state::window_proxy_of(rt, window.0)?;
            let global = crate::state::global_of(rt, into.0)?;
            let key = rt.key_from_str(name)?;
            let desc = swb_js::PropertyDescriptor::data(proxy.into(), true, false, true);
            rt.define_property(global, key, desc)
        };
        define(&mut self.rt).map_err(|e| internal(&e))
    }
}
