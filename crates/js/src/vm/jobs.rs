//! The job queue (ECMA-262 §9.5, <https://tc39.es/ecma262/#sec-jobs>) and
//! the rejection tracker (§27.2.1.9 `HostPromiseRejectionTracker`).
//!
//! `HostEnqueuePromiseJob` puts a job at the end of one FIFO queue of the
//! runtime; a job carries its realm (`None` when the specification gives
//! none). The host runs the queue with `Runtime::run_jobs` at its
//! microtask checkpoints (HTML); the `swb-js` shell after each script.
//! Jobs run only when no frame is on the stack. The queue is a root, and
//! its memory counts for the heap limit ([`JobQueue::bytes`]), so a queue
//! that grows without end ends the script at the heap limit.
//!
//! The rejection tracker keeps the promises that were rejected while no
//! handler was attached ("reject"). "handle" sets `[[PromiseIsHandled]]`,
//! which `PerformPromiseThen` does anyway; the list drops such promises
//! when the host takes it (and when it has doubled since the last prune),
//! so both operations cost O(1).

use std::collections::VecDeque;

use crate::heap::{Gc, Heap, Tracer};
use crate::object::Object;
use crate::runtime::Runtime;
use crate::value::Value;
use crate::vm::promise::{Capability, Handler, ReactionKind};
use crate::vm::{VmError, VmResult};

/// A job of the queue.
pub(crate) enum Job {
    /// `NewPromiseReactionJob(reaction, argument)` (§27.2.2.1).
    Reaction {
        capability: Option<Capability>,
        handler: Handler,
        kind: ReactionKind,
        argument: Value,
    },
    /// `NewPromiseResolveThenableJob(promiseToResolve, thenable, then)`
    /// (§27.2.2.2).
    ResolveThenable {
        promise: Gc<Object>,
        thenable: Value,
        then: Value,
    },
}

impl Job {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for value in self.values() {
            tracer.value(value);
        }
    }

    /// The values that the job holds (at most five).
    fn values(&self) -> impl Iterator<Item = Value> {
        let mut values = [Value::Undefined; 5];
        match self {
            Job::Reaction {
                capability,
                handler,
                argument,
                ..
            } => {
                if let Some(capability) = capability {
                    values[..3].copy_from_slice(&capability.values());
                }
                values[3] = handler.value();
                values[4] = *argument;
            }
            Job::ResolveThenable {
                promise,
                thenable,
                then,
            } => {
                values[0] = Value::Object(*promise);
                values[1] = *thenable;
                values[2] = *then;
            }
        }
        values.into_iter()
    }

    /// Records the job's values in the current handle scope, while it runs
    /// outside the queue.
    fn record(&self, heap: &mut Heap) {
        for value in self.values() {
            heap.record(value);
        }
    }
}

/// The capacity (in jobs) that an empty queue keeps; above it, the queue
/// gives its memory back when it becomes empty.
const KEPT_JOB_CAPACITY: usize = 1024;

/// A job and its realm.
pub(crate) struct QueuedJob {
    job: Job,
    realm: Option<u32>,
}

/// The job queue and the rejection tracker of a runtime (part of the VM's
/// roots).
#[derive(Default)]
pub(crate) struct JobQueue {
    queue: VecDeque<QueuedJob>,
    /// The promises rejected without a handler since the host took the
    /// list (HTML's "about-to-be-notified rejected promises list").
    rejected: Vec<Gc<Object>>,
    /// The length of `rejected` after its last prune.
    pruned: usize,
    /// The promises that the host took last; they stay alive until it
    /// takes the next list.
    taken: Vec<Gc<Object>>,
}

impl JobQueue {
    pub(crate) fn trace(&self, tracer: &mut Tracer<'_>) {
        for queued in &self.queue {
            queued.job.trace(tracer);
        }
        for promise in self.rejected.iter().chain(&self.taken) {
            tracer.object(*promise);
        }
    }

    /// The bytes of the queue and the lists (counted as live heap memory).
    pub(crate) fn bytes(&self) -> usize {
        self.queue.capacity() * size_of::<QueuedJob>()
            + (self.rejected.capacity() + self.taken.capacity()) * size_of::<Gc<Object>>()
    }

    /// The number of jobs in the queue.
    pub(crate) fn len(&self) -> usize {
        self.queue.len()
    }
}

impl Runtime {
    /// `HostEnqueuePromiseJob(job, realm)` (§9.5.5). Never collects; the
    /// job is charged to the heap.
    pub(crate) fn enqueue_job(&mut self, job: Job, realm: Option<u32>) {
        self.heap.charge(size_of::<QueuedJob>());
        self.vm.jobs.queue.push_back(QueuedJob { job, realm });
    }

    /// `HostPromiseRejectionTracker(promise, "reject")`.
    pub(crate) fn track_rejection(&mut self, promise: Gc<Object>) {
        let tracker = &mut self.vm.jobs;
        if tracker.rejected.len() >= 64 && tracker.rejected.len() >= 2 * tracker.pruned {
            self.prune_rejections();
        }
        self.heap.charge(size_of::<Gc<Object>>());
        self.vm.jobs.rejected.push(promise);
    }

    /// Drops the promises that got a handler from the tracker's list.
    fn prune_rejections(&mut self) {
        let mut rejected = std::mem::take(&mut self.vm.jobs.rejected);
        rejected.retain(|&promise| {
            self.promise_state(promise)
                .is_ok_and(|state| !state.is_handled)
        });
        self.vm.jobs.pruned = rejected.len();
        self.vm.jobs.rejected = rejected;
    }

    /// The promises that were rejected without a handler and still have
    /// none, in the order of their rejection; the list starts again empty.
    /// They stay alive until the next call.
    pub(crate) fn take_rejections(&mut self) -> Vec<Gc<Object>> {
        self.prune_rejections();
        let rejected = std::mem::take(&mut self.vm.jobs.rejected);
        self.vm.jobs.pruned = 0;
        // A new vector, not `clone_from`, so that a long list taken earlier
        // does not keep its capacity.
        self.vm.jobs.taken = Vec::from(rejected.as_slice());
        rejected
    }

    /// Takes the next job off the queue; its values are recorded in the
    /// current handle scope.
    pub(crate) fn next_job(&mut self) -> Option<(Job, Option<u32>)> {
        let queue = &mut self.vm.jobs.queue;
        let queued = queue.pop_front()?;
        // An empty queue gives back the memory of a burst, which would
        // otherwise count as live heap for the life of the runtime.
        if queue.is_empty() && queue.capacity() > KEPT_JOB_CAPACITY {
            *queue = VecDeque::new();
        }
        queued.job.record(&mut self.heap);
        Some((queued.job, queued.realm))
    }

    /// Drops the jobs of the queue (for a host that stops the scripts of a
    /// document).
    pub(crate) fn drop_jobs(&mut self) {
        self.vm.jobs.queue = VecDeque::new();
    }

    /// Runs one job. A thrown value that no capability takes is the job's
    /// error; the host reports it.
    pub(crate) fn run_job(&mut self, job: Job, realm: Option<u32>) -> VmResult<()> {
        match job {
            Job::Reaction {
                capability,
                handler,
                kind,
                argument,
            } => {
                let result = match handler {
                    Handler::Empty => match kind {
                        ReactionKind::Fulfill => Ok(argument),
                        ReactionKind::Reject => Err(argument),
                    },
                    Handler::Function(function) => {
                        let result = self.call(function, Value::Undefined, &[argument]);
                        self.completion(result)?
                    }
                    Handler::Internal(continuation) => {
                        let result = (continuation.func)(self, continuation.data, kind, argument);
                        self.completion(result)?
                    }
                };
                match (capability, result) {
                    (Some(capability), Ok(value)) => self.capability_resolve(&capability, value),
                    (Some(capability), Err(reason)) => self.capability_reject(&capability, reason),
                    (None, Ok(_)) => Ok(()),
                    // Step 1.f asserts a normal completion: only a host
                    // continuation without a capability can get here.
                    (None, Err(reason)) => Err(VmError::Throw(reason)),
                }
            }
            Job::ResolveThenable {
                promise,
                thenable,
                then,
            } => {
                let realm = realm.unwrap_or_else(|| self.current_realm());
                let (resolve, reject) = self.create_resolving_functions(promise, realm)?;
                let result = self.call(then, thenable, &[resolve.into(), reject.into()]);
                if let Err(reason) = self.completion(result)? {
                    self.call(reject.into(), Value::Undefined, &[reason])?;
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::QueuedJob;
    use crate::runtime::{Runtime, RuntimeConfig};

    #[test]
    fn a_drained_queue_and_taken_lists_give_back_their_memory() {
        let mut rt = Runtime::new(RuntimeConfig::default()).expect("runtime");
        rt.eval(
            "for (var i = 0; i < 100000; i++) Promise.resolve().then(() => {}); \
             for (var i = 0; i < 100000; i++) Promise.reject(i);",
        )
        .expect("script");
        assert!(rt.vm.jobs.bytes() > 100_000 * size_of::<QueuedJob>());
        rt.run_jobs().expect("jobs");
        assert_eq!(rt.take_unhandled_rejections().len(), 100_000);
        // The taken promises stay alive until the next take.
        assert_eq!(rt.take_unhandled_rejections(), []);
        assert_eq!(rt.vm.jobs.bytes(), 0);
    }
}
