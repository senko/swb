//! The load state of frames and the refresh of frame documents (ADR 0031
//! sections 2 and 4): the completion of a frame document's requests, the
//! declarative refresh (`<meta http-equiv=refresh>`), when a frame has
//! loaded, when the frames delay the page's `load` event, and the `load`
//! event at the container. `frames.rs` has the frame tree, `frame_commit.rs`
//! the arrival of documents.

use swb_dom::NodeId;
use swb_net::Url;
use swb_web::{DispatchTarget, ScriptRuntime, TrustedEvent};

use super::Page;
use crate::document::DocumentState;
use crate::frames::{FrameId, FrameScript, FrameTarget};
use crate::resources::Pending;

impl Page {
    /// Hands a completion of a request of frame `id`'s document to it. Its
    /// scripts and events follow, as for the top-level document.
    pub(super) fn frame_completion(
        &mut self,
        id: FrameId,
        completion: swb_net::Completion,
    ) -> bool {
        let Some(frame) = self.frames.get_mut(id) else {
            return false;
        };
        let Some(doc) = frame.doc.as_deref_mut() else {
            return false;
        };
        if doc.is_api_fetch(completion.id) {
            doc.api_fetch_completed(
                completion.id,
                completion.result,
                completion.kind,
                completion.reservation,
            );
            self.deliver_api_fetches_in(id);
            return true;
        }
        let Some(pending) = doc.requests.pending.remove(&completion.id) else {
            return false;
        };
        let sheet = matches!(pending, Pending::Stylesheet { .. });
        doc.with_own_fonts(&mut self.fonts, |doc, fonts| {
            doc.handle_completion(pending, completion.result, fonts);
        });
        self.deliver_element_events_in(id);
        self.deliver_scripts_in(id, sheet);
        true
    }

    // ----- The declarative refresh of frame documents -----

    /// The page time of the next refresh of a frame document.
    pub(super) fn next_frame_refresh(&self) -> Option<f64> {
        if self.frames.count() == 0 {
            return None;
        }
        self.frames
            .tree_order()
            .into_iter()
            .filter_map(|id| self.frames.get(id)?.refresh.as_ref().map(|(t, _)| *t))
            .reduce(f64::min)
    }

    /// Navigates the frames whose refresh is due. True if any did.
    pub(super) fn run_frame_refreshes(&mut self) -> bool {
        if self.lent || self.frames.count() == 0 {
            return false;
        }
        let now = self.clock.now();
        let due: Vec<(FrameId, Url)> = self
            .frames
            .tree_order()
            .into_iter()
            .filter_map(|id| {
                let frame = self.frames.get_mut(id)?;
                if frame.refresh.as_ref().is_some_and(|(t, _)| *t <= now) {
                    frame.refresh.take().map(|(_, url)| (id, url))
                } else {
                    None
                }
            })
            .collect();
        let any = !due.is_empty();
        for (id, url) in due {
            log::debug!("{id}: refresh to {url}");
            self.navigate_frame(id, FrameTarget::Url(url));
        }
        any
    }

    // ----- Load state -----

    /// True if frame `id` has loaded: its `load` fired, or its document
    /// and its frames have loaded (without a check of its layout).
    fn frame_settled(&self, id: FrameId) -> bool {
        let Some(frame) = self.frames.get(id) else {
            return true;
        };
        frame.load_fired
            || (frame.pending.is_none()
                && !self.doc_of(id).is_some_and(DocumentState::is_loading)
                && frame.children.iter().all(|&c| self.frame_settled(c)))
    }

    /// True while frames delay the page's load (JavaScript off, or before
    /// the `load` event with JavaScript on).
    pub(super) fn frames_loading(&self) -> bool {
        let top = self.frames.top();
        !top.load_fired && top.children.iter().any(|&c| !self.frame_settled(c))
    }

    /// True if a child frame has not fired its container's `load`: the
    /// page's `load` event waits.
    pub(super) fn frames_delay_load(&self) -> bool {
        let top = self.frames.top();
        !top.load_fired
            && top
                .children
                .iter()
                .any(|&c| self.frames.get(c).is_some_and(|f| !f.load_fired))
    }

    /// True if any frame loads (requests, a navigation): the loops of the
    /// event loop do not jump in virtual time meanwhile.
    pub(super) fn frames_busy(&self) -> bool {
        !self.frame_navigations.is_empty()
            || self
                .frames
                .tree_order()
                .into_iter()
                .skip(1)
                .any(|id| self.frame_is_loading(id))
    }

    /// Marks the frames whose document and frames loaded, children first,
    /// and fires the `load` event at their containers (HTML "iframe load
    /// event steps"): as a task of the parent's document when it has a
    /// realm. A document that the runtime drives fires its own `load` first;
    /// that task also fires the container's. True if any frame loaded.
    pub(super) fn check_frame_loads(&mut self) -> bool {
        if self.lent || self.frames.count() == 0 {
            return false;
        }
        let mut any = false;
        for id in self.frames.descendants_post_order(FrameId::TOP) {
            let Some(frame) = self.frames.get(id) else {
                continue;
            };
            if frame.load_fired
                || frame.pending.is_some()
                || self.doc_of(id).is_none_or(DocumentState::is_loading)
                || frame
                    .children
                    .iter()
                    .any(|&c| self.frames.get(c).is_some_and(|f| !f.load_fired))
            {
                continue;
            }
            let script = frame.script;
            if let Some((runtime, key)) = self.frame_runtime(id) {
                if runtime.load_event_due_in(key) {
                    // Layout can find more resources (ADR 0031 section 2).
                    self.layout_frame_alone(id);
                    if self.doc_of(id).is_none_or(DocumentState::is_loading) {
                        continue;
                    }
                    self.run_task_in(id, ScriptRuntime::fire_load_event);
                    any = true;
                    if let Some(script) = script {
                        self.finish_load_across_agents(id, script);
                        self.finish_load_after_limit(id, script);
                    }
                    continue;
                }
                if !runtime.document_loaded_in(key) {
                    continue;
                }
            }
            any = true;
            self.mark_frame_loaded(id);
        }
        any
    }

    /// A limit can end the task of the `load` event of frame `id`'s document
    /// before the container's `load`, which marks the frame. No later event has
    /// to come, and the page's `load` waits for the frame, so the frame is done
    /// here. A listener that navigated the frame left a pending navigation or
    /// another document.
    fn finish_load_after_limit(&mut self, id: FrameId, script: FrameScript) {
        let waiting = self
            .frames
            .get(id)
            .is_some_and(|f| !f.load_fired && f.pending.is_none() && f.script == Some(script));
        if waiting
            && self
                .frame_runtime(id)
                .is_some_and(|(runtime, key)| runtime.document_loaded_in(key))
        {
            self.mark_frame_loaded(id);
        }
    }

    /// The `load` task of frame `id`'s document ended, and its container is in
    /// another runtime, where no task of the frame can run a listener: the page
    /// fires the container's `load` now, then the frame's `pageshow` (ADR 0031
    /// section 6).
    fn finish_load_across_agents(&mut self, id: FrameId, script: FrameScript) {
        let waits = self
            .frames
            .get(id)
            .is_some_and(|f| f.script == Some(script))
            && self
                .frame_runtime(id)
                .is_some_and(|(runtime, key)| runtime.pageshow_waits_in(key));
        if !waits {
            return;
        }
        if let Some((parent, container)) = self
            .frames
            .get(id)
            .and_then(|f| Some((f.parent?, f.container?)))
        {
            self.fire_container_load(id, parent, container);
        }
        if self
            .frames
            .get(id)
            .is_some_and(|f| f.script == Some(script))
        {
            self.run_task_in(id, ScriptRuntime::fire_waiting_pageshow);
        }
    }

    /// Frame `id` has loaded: its container gets `load`.
    fn mark_frame_loaded(&mut self, id: FrameId) {
        let Some(frame) = self.frames.get_mut(id) else {
            return;
        };
        frame.load_fired = true;
        frame.reuse_window = false;
        let (parent, container) = (frame.parent, frame.container);
        if let (Some(parent), Some(container)) = (parent, container) {
            self.fire_container_load(id, parent, container);
        }
    }

    /// Fires `load` at the container of frame `id`, if its parent's document
    /// has a realm. A listener of an earlier `load` may have removed the
    /// container, or removed and inserted it again (a new frame, which has
    /// not loaded).
    fn fire_container_load(&mut self, id: FrameId, parent: FrameId, container: NodeId) {
        if self.frames.by_container(parent, container) != Some(id) {
            return;
        }
        if self.frames.get(parent).is_some_and(|f| f.script.is_some()) {
            let event = TrustedEvent::new("load");
            self.run_task_in(parent, |script| {
                script.dispatch_event(DispatchTarget::Node(container), &event)
            });
        }
    }
}
