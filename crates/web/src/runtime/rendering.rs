//! The steps of the event loop that the page drives besides the tasks: the
//! rendering update (HTML §8.1.7.3,
//! <https://html.spec.whatwg.org/multipage/webappapis.html#update-the-rendering>:
//! animation frame callbacks, media query changes, resize and intersection
//! observers), idle periods (`requestIdleCallback`) and the flush of `storage`
//! events before a commit. The page calls these through `ScriptRuntime` for the
//! documents of the runtime (ADR 0028 section 7).

use std::time::Instant;

use swb_js::VmError;

use super::{
    DocState, DocumentKey, MAX_STORAGE_FLUSH, ScriptError, ScriptRuntime, set_listener_checkpoints,
    task_error,
};

impl ScriptRuntime {
    // ----- The rendering update (HTML §8.1.7.3) -----

    /// True if an animation frame callback waits: the engine runs a frame
    /// for it.
    pub fn has_animation_frame_callbacks(&self) -> bool {
        !self.stopped && self.any_document(DocState::has_animation_frames)
    }

    // ----- Observers -----

    /// True if an intersection or resize observer has a target: the engine
    /// then updates them in the frames that follow a change.
    pub fn has_observations(&self) -> bool {
        !self.stopped && self.any_document(DocState::has_observations)
    }

    /// True if an observation was added since the last update: the engine
    /// runs a frame for it.
    pub fn observations_requested(&self) -> bool {
        !self.stopped && self.any_document(|d| d.io.requested() || d.ro.requested())
    }

    /// The resize observer steps of the rendering update (Resize Observer
    /// §3.4): gathers the active observations and delivers them, again
    /// below the depth of the shallowest delivered target, until none is
    /// active; skipped ones fire the loop error. Each callback is a task.
    /// Returns true if a callback ran.
    pub fn run_resize_observers(&mut self) -> bool {
        if self.stopped || !crate::resize_observer::is_observing(&self.rt) {
            return false;
        }
        crate::resize_observer::clear_request(&mut self.rt);
        let mut ran = false;
        let mut limit = -1;
        let mut skipped = false;
        loop {
            // Errors are reported.
            let gathered = self.task("resize observations", |rt| {
                crate::resize_observer::gather(rt, limit).map_err(|e| match e {
                    crate::bindings::Exception::Js(error) => task_error(error),
                    other => task_error(VmError::type_error(format!("{other:?}"))),
                })
            });
            let Ok(gathered) = gathered else { break };
            skipped = gathered.has_skipped();
            if !gathered.has_active() || self.stopped {
                break;
            }
            let mut shallowest = i32::MAX;
            for id in gathered.observers() {
                if self.stopped {
                    break;
                }
                let depth = self.task("a ResizeObserver callback", |rt| {
                    crate::resize_observer::broadcast(rt, &gathered, id)
                });
                ran = true;
                if let Ok(depth) = depth {
                    shallowest = shallowest.min(depth);
                }
            }
            if shallowest == i32::MAX {
                break;
            }
            limit = shallowest;
        }
        if skipped && !self.stopped {
            // The skipped observations stay active: the next frame
            // delivers them.
            crate::resize_observer::request(&mut self.rt);
            let _ = self.task("a resize observer", |rt| {
                crate::errors::report_resize_loop_error(rt).map_err(task_error)
            });
            ran = true;
        }
        ran
    }

    /// The intersection observer steps of the rendering update
    /// (Intersection Observer "run the update intersection observations
    /// steps"): computes the observations after layout at the page time
    /// `frame_time`, queues the entries that changed and the task that
    /// delivers them. Returns true if it ran.
    pub fn update_intersection_observers(&mut self, frame_time: f64) -> bool {
        if self.stopped || !crate::state::state(&self.rt).is_ok_and(|s| s.doc.io.is_observing()) {
            return false;
        }
        let now = crate::state::state(&self.rt).map_or(0.0, |s| s.doc.clock.at(frame_time));
        let _ = self.task("intersection observations", |rt| {
            crate::intersection_observer::update(rt, now)
        });
        true
    }

    // ----- Idle periods (step 4 of the event loop) -----

    /// True if an idle callback waits: the engine starts an idle period
    /// for it when no task and no frame is due.
    pub fn has_idle_callbacks(&self) -> bool {
        !self.stopped && self.any_document(|d| !d.idle.is_empty())
    }

    /// An idle period that ends at the page time `deadline`: the callbacks
    /// that wait now run, each as a task of its own, with an `IdleDeadline`
    /// for that deadline. The period ends early when the (real) clock
    /// reaches the deadline. Callbacks that they request wait for the next
    /// period. Returns the number of callbacks that ran.
    pub fn run_idle_callbacks(&mut self, deadline: f64) -> usize {
        let keys: Vec<DocumentKey> = crate::state::state(&self.rt)
            .map(|s| {
                std::iter::once(&*s.doc)
                    .chain(s.docs_iter())
                    .filter(|d| !d.idle.is_empty())
                    .map(|d| DocumentKey(d.key))
                    .collect()
            })
            .unwrap_or_default();
        let mut ran = 0;
        for key in keys {
            ran += self
                .with_document(key, |runtime| runtime.run_idle_of_document(deadline))
                .unwrap_or(0);
        }
        ran
    }

    /// The idle callbacks of the current document.
    fn run_idle_of_document(&mut self, deadline: f64) -> usize {
        let mut ran = 0;
        for handle in crate::idle_callbacks::handles(&self.rt) {
            if self.stopped || (!self.clock.is_virtual() && self.clock.now() >= deadline) {
                break;
            }
            if !crate::idle_callbacks::waits(&self.rt, handle) {
                continue;
            }
            // Errors are reported.
            let _ = self.task("an idle callback", |rt| {
                crate::idle_callbacks::run(rt, handle, Some(deadline))
            });
            ran += 1;
        }
        ran
    }

    /// CSSOM View "evaluate media queries and report changes": a `change`
    /// event at each `MediaQueryList` with listeners whose result changed,
    /// as one task. Nothing without such lists; returns true if the task
    /// ran.
    pub fn evaluate_media_queries(&mut self) -> bool {
        if self.stopped || !crate::media_queries::any_registered(&self.rt) {
            return false;
        }
        // Errors are reported.
        let _ = self.task("media query changes", |rt| {
            set_listener_checkpoints(rt, true);
            let result = crate::media_queries::report_changes(rt).map_err(task_error);
            set_listener_checkpoints(rt, false);
            result
        });
        true
    }

    /// HTML "run the animation frame callbacks": the callbacks that wait
    /// now, each as a task of its own, with the time of the frame
    /// (`frame_time`, in page time, as `performance.now()` gives it).
    /// Callbacks that they request wait for the next frame.
    pub fn run_animation_frame_callbacks(&mut self, frame_time: f64) {
        if self.stopped {
            return;
        }
        let now = crate::state::state(&self.rt).map_or(0.0, |s| s.doc.clock.at(frame_time));
        for handle in crate::animation_frames::handles(&self.rt) {
            if self.stopped {
                return;
            }
            // Errors are reported.
            let _ = self.task("an animation frame callback", |rt| {
                crate::animation_frames::run(rt, handle, now)
            });
        }
    }

    /// Fires the `storage` events that wait for the current document now, before a
    /// navigation replaces it: Chromium's events come before a pending navigation commits
    /// (measured, case `storage-event-retire`), and a document that is gone gets none.
    /// Only the events that wait at the call, and for at most [`MAX_STORAGE_FLUSH`]: a
    /// listener can queue new ones for ever. The commit drops the rest.
    pub fn flush_storage_events(&mut self) {
        let started = Instant::now();
        for _ in 0..crate::storage_events::waiting(&self.rt) {
            if self.stopped || started.elapsed() >= MAX_STORAGE_FLUSH {
                return;
            }
            // Errors are reported.
            let _ = self.storage_event_task();
        }
    }

    /// The task of one `storage` event of the current document.
    pub(super) fn storage_event_task(&mut self) -> Result<(), ScriptError> {
        self.task("the event 'storage'", |rt| {
            set_listener_checkpoints(rt, true);
            let result = crate::storage_events::deliver(rt).map_err(task_error);
            set_listener_checkpoints(rt, false);
            result
        })
    }
}
