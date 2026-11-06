//! The agents of the page (ADR 0031 sections 5, 6 and 11): one script runtime
//! per origin. `Page::script` is the runtime of the top-level document, as on a
//! page without frames; [`Agents::others`] holds one more for each other origin
//! whose frame document has a script to run or a first need. An opaque origin
//! is unique, so a sandboxed document has a runtime of its own. An `about:blank`
//! or `about:srcdoc` child takes its creator's origin and joins its runtime (a
//! clone of an opaque origin compares equal); a child in a sandbox without
//! `allow-same-origin` has a new opaque origin (measured, case
//! `agents-opaque-state`).
//!
//! A runtime is made at its first need. It goes with the top-level document, or
//! earlier: when its last live document has gone ([`Page::sweep_agents`]; nothing
//! in it can run any more), and alone when it reaches the heap limit
//! ([`Page::drop_failed_agent`]).
//!
//! *What belongs to one runtime.* A `DocumentKey`, a `Gc`, a `NodeId` of a document,
//! a realm index and a `WindowProxy` mean nothing in another runtime.
//! [`crate::frames::FrameScript`] holds an agent and a key together, and gives the
//! key only for that agent; [`Page::frame_runtime`] is the way from a frame to a
//! runtime and its key. The registry answers for the agent of the running task
//! (`crate::frames::registry`). A task never runs a script of another runtime
//! inside a native: what must happen there waits in `AfterTask`, and the page runs
//! it after the task. What goes from one runtime to another is plain data
//! ([`swb_web::Mail`]), and the page hands it over between tasks.
//!
//! *Budgets.* At most [`MAX_AGENTS`] runtimes; a further origin's document runs
//! without scripts. The top level's runtime keeps a whole runtime's share of the
//! page's heap and long tasks. Before each task another runtime's heap limit is the
//! smaller of its own limit and what the other frames leave of the rest, and so is
//! its budget of long tasks. A page with one runtime pays one comparison.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use swb_net::Origin;
use swb_web::{AgentShared, DocumentKey, HEAP_LIMIT, MailTarget, SCRIPT_BUDGET, ScriptRuntime};

use super::Page;
use crate::frames::{AgentId, FrameId, FrameScript};

/// The most runtimes of a page (ADR 0031 section 11).
pub const MAX_AGENTS: usize = 32;

/// The most origins whose failed runtime the page remembers.
const MAX_GONE: usize = 64;

/// The limits of the runtimes of a page (ADR 0031 section 11). Tests use smaller
/// ones ([`Page::set_agent_limits`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgentLimits {
    /// The heap of one runtime, in bytes.
    pub heap: usize,
    /// The heaps of all runtimes of the page, in bytes.
    pub page_heap: usize,
    /// The long tasks of one runtime.
    pub long_tasks: Duration,
    /// The long tasks of all runtimes of the page.
    pub page_long_tasks: Duration,
    /// The time limit of a task, for the runtimes other than the top level's;
    /// `None` keeps the default.
    pub task_time: Option<Duration>,
}

impl Default for AgentLimits {
    fn default() -> AgentLimits {
        AgentLimits {
            heap: HEAP_LIMIT,
            page_heap: 2 * HEAP_LIMIT,
            long_tasks: SCRIPT_BUDGET,
            page_long_tasks: 2 * SCRIPT_BUDGET,
            task_time: None,
        }
    }
}

/// The runtime of an origin other than the top-level document's.
pub(super) struct Agent {
    id: AgentId,
    origin: Origin,
    runtime: ScriptRuntime,
}

/// The agents of the page. See the module documentation.
#[derive(Default)]
pub(super) struct Agents {
    /// True while the current top-level document has scripting on: its frame
    /// documents may run scripts, also after the top level's runtime has gone.
    pub(super) enabled: bool,
    /// The agent of `Page::script`.
    pub(super) top: AgentId,
    /// The runtimes of the other origins.
    pub(super) others: Vec<Agent>,
    /// The number of the next agent: a number is not used twice in a page.
    next: u32,
    /// The origins whose runtime reached the heap limit: their documents run
    /// without scripts until the next top-level document.
    gone: Vec<Origin>,
    /// True after the warning about [`MAX_AGENTS`].
    warned: bool,
    /// What the runtimes share.
    pub(super) shared: Option<AgentShared>,
    /// The long tasks of the runtimes that have gone.
    spent: Duration,
    /// True after the warning about the page's long tasks.
    stopped_warned: bool,
    limits: AgentLimits,
    /// True if a runtime has limits below its own: when the others have gone,
    /// they go back.
    lowered: bool,
    /// True if a document retired since the last sweep.
    pub(super) dirty: bool,
}

impl Agents {
    /// A number for a new agent.
    pub(super) fn new_id(&mut self) -> AgentId {
        self.next = self.next.wrapping_add(1);
        AgentId::new(self.next)
    }

    /// All runtimes but the top level's go, with what the page knows of them:
    /// a new top-level document.
    pub(super) fn reset(&mut self) {
        self.enabled = false;
        self.others.clear();
        self.gone.clear();
        self.warned = false;
        self.shared = None;
        self.spent = Duration::ZERO;
        self.stopped_warned = false;
        self.lowered = false;
        self.dirty = false;
    }
}

impl Page {
    /// Sets the limits of the page's runtimes (tests and tools).
    pub fn set_agent_limits(&mut self, limits: AgentLimits) {
        self.agents.limits = limits;
    }

    /// The number of script runtimes of the page.
    pub fn runtime_count(&self) -> usize {
        self.all_runtimes().count()
    }

    /// The number of the frames whose documents have a realm in the same
    /// runtime as the document of `frame`, itself included; `None` without a
    /// realm. For tests.
    pub fn frames_in_runtime_of(&self, frame: FrameId) -> Option<usize> {
        let agent = self.frames.get(frame)?.script?.agent();
        let in_agent = |id: &FrameId| {
            self.frames
                .get(*id)
                .and_then(|f| f.script)
                .is_some_and(|s| s.agent() == agent)
        };
        Some(
            self.frames
                .tree_order()
                .iter()
                .filter(|id| in_agent(id))
                .count(),
        )
    }

    /// The bytes of the heaps of all runtimes of the page (tests and
    /// measurements).
    pub fn script_heap_total(&self) -> usize {
        self.all_runtimes().map(ScriptRuntime::heap_size).sum()
    }

    /// The runtime of `agent`, if it is there.
    pub(super) fn runtime_of(&self, agent: AgentId) -> Option<&ScriptRuntime> {
        if agent == self.agents.top {
            return self.script.as_ref();
        }
        self.agents
            .others
            .iter()
            .find(|a| a.id == agent)
            .map(|a| &a.runtime)
    }

    /// The runtime of `agent` for changes, if it is there.
    pub(super) fn runtime_of_mut(&mut self, agent: AgentId) -> Option<&mut ScriptRuntime> {
        if agent == self.agents.top {
            return self.script.as_mut();
        }
        self.agents
            .others
            .iter_mut()
            .find(|a| a.id == agent)
            .map(|a| &mut a.runtime)
    }

    /// The runtime of the document of `frame` and its key there; `None` if it
    /// has no realm.
    pub(super) fn frame_runtime(&self, frame: FrameId) -> Option<(&ScriptRuntime, DocumentKey)> {
        let script = self.frames.get(frame)?.script?;
        let agent = script.agent();
        Some((self.runtime_of(agent)?, script.key_in(agent)?))
    }

    /// [`Page::frame_runtime`] for changes.
    pub(super) fn frame_runtime_mut(
        &mut self,
        frame: FrameId,
    ) -> Option<(&mut ScriptRuntime, DocumentKey)> {
        let script = self.frames.get(frame)?.script?;
        let agent = script.agent();
        let key = script.key_in(agent)?;
        Some((self.runtime_of_mut(agent)?, key))
    }

    /// True if a runtime of the page holds `test`. The paths of each turn of the
    /// event loop ask this, so a page with one runtime pays one comparison more.
    #[inline]
    pub(super) fn any_runtime(&self, test: impl Fn(&ScriptRuntime) -> bool) -> bool {
        self.script.as_ref().is_some_and(&test)
            || (!self.agents.others.is_empty()
                && self.agents.others.iter().any(|a| test(&a.runtime)))
    }

    /// All runtimes of the page, the top level's first.
    pub(super) fn all_runtimes(&self) -> impl Iterator<Item = &ScriptRuntime> {
        self.script
            .iter()
            .chain(self.agents.others.iter().map(|a| &a.runtime))
    }

    /// All runtimes of the page for changes.
    pub(super) fn all_runtimes_mut(&mut self) -> impl Iterator<Item = &mut ScriptRuntime> {
        self.script
            .iter_mut()
            .chain(self.agents.others.iter_mut().map(|a| &mut a.runtime))
    }

    /// The agents of the page, the top level's first.
    pub(super) fn agent_ids(&self) -> Vec<AgentId> {
        let top = self.script.as_ref().map(|_| self.agents.top);
        top.into_iter()
            .chain(self.agents.others.iter().map(|a| a.id))
            .collect()
    }

    /// The agent of `origin`, if it has a runtime.
    pub(super) fn agent_of_origin(&self, origin: &Origin) -> Option<AgentId> {
        if self.script.is_some() && self.frames.top().origin.as_ref() == Some(origin) {
            return Some(self.agents.top);
        }
        self.agents
            .others
            .iter()
            .find(|a| a.origin == *origin)
            .map(|a| a.id)
    }

    /// True if a document of `origin` can have a realm: its runtime is there or
    /// can be made. One warning per top-level document past [`MAX_AGENTS`].
    pub(super) fn may_host_scripts(&mut self, origin: &Origin) -> bool {
        if self.agent_of_origin(origin).is_some() {
            return true;
        }
        if self.agents.gone.iter().any(|o| o == origin) {
            return false;
        }
        if self.runtime_count() < MAX_AGENTS {
            return true;
        }
        self.agents.dirty = true;
        self.sweep_agents();
        if self.runtime_count() < MAX_AGENTS {
            return true;
        }
        if !std::mem::replace(&mut self.agents.warned, true) {
            log::warn!(
                "the page has {MAX_AGENTS} script runtimes, the limit; documents of further origins run without scripts"
            );
        }
        false
    }

    /// Makes the runtime of `origin`, whose first document is the current one of
    /// `frame`. `None` if it cannot be made.
    pub(super) fn make_agent(&mut self, frame: FrameId, origin: Origin) -> Option<FrameScript> {
        // Fail closed: `may_host_scripts` asked earlier, and scripts ran since.
        if self.runtime_count() >= MAX_AGENTS {
            return None;
        }
        let shared = self
            .agents
            .shared
            .get_or_insert_with(AgentShared::default)
            .clone();
        let mut runtime =
            match ScriptRuntime::for_frame(self.clock.clone(), frame.number(), &shared) {
                Ok(runtime) => runtime,
                Err(error) => {
                    log::warn!("cannot create the script runtime of a frame: {error}");
                    return None;
                }
            };
        let registry: Rc<RefCell<dyn swb_web::FrameHost>> = self.registry.clone();
        runtime.set_frame_host(registry, Rc::clone(&self.frames.count_cell));
        if let Some(limit) = self.agents.limits.task_time {
            runtime.set_time_limit(limit);
        }
        log::debug!("{frame}: a script runtime for its origin");
        let id = self.agents.new_id();
        self.agents.others.push(Agent {
            id,
            origin,
            runtime,
        });
        Some(FrameScript::new(id, DocumentKey::FIRST))
    }

    /// The document of `frame` gets a realm at its first need, outside a task:
    /// in the runtime of its origin, which is made now if it has none.
    pub(super) fn ensure_frame_realm(&mut self, frame: FrameId) {
        let Some(record) = self.frames.get(frame) else {
            return;
        };
        if record.script.is_some() || !self.agents.enabled {
            return;
        }
        let Some(origin) = record.origin.clone() else {
            return;
        };
        let allowed = record.sandbox.scripts_allowed();
        let number = frame.number();
        if let Some(agent) = self.agent_of_origin(&origin) {
            self.with_registry(agent, |script| script.ensure_frame_window(number));
            return;
        }
        if !self.may_host_scripts(&origin) {
            return;
        }
        let Some(script) = self.make_agent(frame, origin) else {
            return;
        };
        let loading = self
            .doc_of(frame)
            .is_some_and(crate::document::DocumentState::is_loading);
        if let Some(record) = self.frames.get_mut(frame) {
            record.script = Some(script);
        }
        if let Some((runtime, key)) = self.frame_runtime_mut(frame) {
            runtime.set_scripts_off(key, !allowed);
            runtime.adopt_complete_document(key, loading);
        }
    }

    /// A new document of `frame` has come: the proxy of the frame loses its target
    /// in each runtime but `home`.
    pub(super) fn clear_frame_targets_except(&mut self, frame: FrameId, home: Option<AgentId>) {
        let number = frame.number();
        let top = self.agents.top;
        if home != Some(top)
            && let Some(script) = self.script.as_mut()
            && script.knows_frame(number)
        {
            script.clear_frame_target(number);
        }
        for agent in &mut self.agents.others {
            if Some(agent.id) != home && agent.runtime.knows_frame(number) {
                agent.runtime.clear_frame_target(number);
            }
        }
    }

    /// Hands what the last task left for other runtimes to them (ADR 0031 section 7): a
    /// task only queues, and no script runs here. A message whose target has no document
    /// in a runtime goes, with its bytes and the ports in it. A page without such a
    /// message pays one comparison.
    #[inline]
    pub(super) fn hand_over_mail(&mut self) {
        if self
            .agents
            .shared
            .as_ref()
            .is_some_and(AgentShared::has_mail)
        {
            self.hand_over_all_mail();
        }
    }

    #[cold]
    fn hand_over_all_mail(&mut self) {
        let Some(shared) = self.agents.shared.clone() else {
            return;
        };
        for mail in shared.take_mail() {
            match mail.target() {
                MailTarget::Frame(number) => {
                    if let Some((runtime, key)) =
                        self.frame_runtime_mut(FrameId::from_number(number))
                    {
                        runtime.receive_mail(Some(key), mail);
                    }
                }
                MailTarget::Runtime(tag) => {
                    if let Some(runtime) = self.all_runtimes_mut().find(|r| r.mail_tag() == tag) {
                        runtime.receive_mail(None, mail);
                    }
                }
            }
        }
        shared.settle_ports();
    }

    /// A runtime went: its heap dropped its port objects, and its queues dropped their
    /// messages. What was entangled with them is told, and what they held in budgets goes
    /// back.
    fn settle_ports(&self) {
        if let Some(shared) = &self.agents.shared {
            shared.settle_ports();
        }
    }

    /// The messages that wait in the page and their bytes: for each runtime what it sent
    /// (tests and measurements).
    pub fn waiting_messages_by_runtime(&self) -> Vec<(usize, u64)> {
        self.all_runtimes()
            .map(|r| (r.waiting_messages(), r.waiting_message_bytes()))
            .collect()
    }

    /// The live message ports of the page (tests and measurements).
    pub fn live_message_ports(&self) -> usize {
        self.agents
            .shared
            .as_ref()
            .map_or(0, AgentShared::live_message_ports)
    }

    /// The bytes of the waiting messages that the runtimes of frames sent (tests and
    /// measurements).
    pub fn frames_message_bytes(&self) -> u64 {
        self.agents
            .shared
            .as_ref()
            .map_or(0, AgentShared::frames_message_bytes)
    }

    /// Runs a full collection in each runtime of the page (tests).
    pub fn collect_all_runtimes(&mut self) {
        for runtime in self.all_runtimes_mut() {
            runtime.collect();
        }
    }

    /// Frame `frame` is destroyed: each runtime forgets its proxy.
    pub(super) fn forget_frame_everywhere(&mut self, frame: FrameId) {
        let number = frame.number();
        for runtime in self.all_runtimes_mut() {
            runtime.forget_frame(number);
        }
    }

    /// Sets the limits of the runtime of `agent` for its next task (ADR 0031
    /// section 11). The top level's runtime keeps a whole runtime's share of the
    /// page, so that frames cannot take its heap or its time: they share the rest.
    /// With one runtime this is one comparison.
    #[inline]
    pub(super) fn set_limits_for_task(&mut self, agent: AgentId) {
        if !self.agents.others.is_empty() || self.agents.lowered {
            self.set_limits_with_others(agent);
        }
    }

    #[cold]
    fn set_limits_with_others(&mut self, agent: AgentId) {
        let limits = self.agents.limits;
        let top = self.agents.top;
        let (heap, budget) = if agent == top {
            (limits.heap, limits.long_tasks)
        } else {
            let mut heap = 0usize;
            let mut time = self.agents.spent;
            for (id, runtime) in self.agent_runtimes() {
                if id != agent && id != top {
                    heap = heap.saturating_add(runtime.heap_size());
                    time = time.saturating_add(runtime.long_task_time());
                }
            }
            // What the top level keeps, while it has a runtime.
            if self.script.is_some() {
                heap = heap.saturating_add(limits.heap);
                time = time.saturating_add(limits.long_tasks);
            }
            (
                limits.heap.min(limits.page_heap.saturating_sub(heap)),
                limits
                    .long_tasks
                    .min(limits.page_long_tasks.saturating_sub(time)),
            )
        };
        self.agents.lowered = !self.agents.others.is_empty();
        if let Some(runtime) = self.runtime_of_mut(agent) {
            runtime.set_heap_limit(heap);
            runtime.set_script_budget(budget);
        }
    }

    fn agent_runtimes(&self) -> impl Iterator<Item = (AgentId, &ScriptRuntime)> {
        let top = self.agents.top;
        self.script
            .iter()
            .map(move |runtime| (top, runtime))
            .chain(self.agents.others.iter().map(|a| (a.id, &a.runtime)))
    }

    /// Past the page's cap on long tasks, all runtimes stop (ADR 0031 section 11).
    #[inline]
    pub(super) fn check_page_long_tasks(&mut self) {
        if !self.agents.others.is_empty() {
            self.check_long_tasks_of_all();
        }
    }

    #[cold]
    fn check_long_tasks_of_all(&mut self) {
        let spent = self.agents.spent;
        let total = self.all_runtimes().fold(spent, |sum, runtime| {
            sum.saturating_add(runtime.long_task_time())
        });
        if total < self.agents.limits.page_long_tasks {
            return;
        }
        if !std::mem::replace(&mut self.agents.stopped_warned, true) {
            log::warn!(
                "the scripts of the page stop: its long tasks ran for {:.1} s",
                total.as_secs_f64()
            );
        }
        for agent in self.agent_ids() {
            if let Some(runtime) = self.runtime_of_mut(agent)
                && !runtime.stopped()
            {
                runtime.stop_scripts();
                self.finish_stopped_scripts(agent);
            }
        }
    }

    /// The runtime of `agent` reached the heap limit: it goes alone, and its
    /// frames go on without scripts. After the top level's runtime the page keeps
    /// the top-level document as it is, with the input of a page without
    /// JavaScript, and the other runtimes go on.
    pub(super) fn drop_failed_agent(&mut self, agent: AgentId) {
        log::warn!(
            "a script runtime of the page reached the heap limit and is dropped; its documents stay as they are"
        );
        if agent == self.agents.top {
            if let Some(origin) = self.frames.top().origin.clone() {
                self.remember_gone(origin);
            }
            if let Some(runtime) = self.script.take() {
                self.agents.spent = self.agents.spent.saturating_add(runtime.long_task_time());
            }
            self.scripts_dropped = true;
            self.finish_parse_without_scripts();
        } else if let Some(at) = self.agents.others.iter().position(|a| a.id == agent) {
            let gone = self.agents.others.swap_remove(at);
            self.agents.spent = self
                .agents
                .spent
                .saturating_add(gone.runtime.long_task_time());
            self.remember_gone(gone.origin);
        }
        self.agents.lowered = true;
        self.frames.release_retired(agent);
        self.frames_lose_runtime(agent);
        self.settle_ports();
    }

    fn remember_gone(&mut self, origin: Origin) {
        if self.agents.gone.len() >= MAX_GONE {
            self.agents.gone.remove(0);
        }
        self.agents.gone.push(origin);
    }

    /// The runtime of `agent` went away: its frame documents go on without
    /// scripts.
    fn frames_lose_runtime(&mut self, agent: AgentId) {
        for id in self.frames.tree_order() {
            let Some(frame) = self.frames.get_mut(id) else {
                continue;
            };
            if frame.script.and_then(|s| s.key_in(agent)).is_none() {
                continue;
            }
            frame.script = None;
            if let Some(doc) = frame.doc.as_deref_mut() {
                doc.abandon_scripts();
                doc.finish_parse_without_scripts();
            }
            self.update_frames(id);
        }
    }

    /// A runtime other than the top level's goes when no live frame has a document
    /// in it: only tasks run it, and the tasks of a retired document do not run.
    /// So the budget counts what can run, and a page that replaces a sandboxed
    /// frame (each document a new opaque origin) does not run out of runtimes.
    #[inline]
    pub(super) fn sweep_agents(&mut self) {
        if self.agents.dirty && !self.lent {
            self.sweep_empty_agents();
        }
    }

    #[cold]
    fn sweep_empty_agents(&mut self) {
        self.agents.dirty = false;
        if self.agents.others.is_empty() {
            return;
        }
        let live: Vec<AgentId> = self
            .frames
            .tree_order()
            .into_iter()
            .filter_map(|id| Some(self.frames.get(id)?.script?.agent()))
            .collect();
        let mut gone = Vec::new();
        let mut spent = self.agents.spent;
        self.agents.others.retain(|agent| {
            let keep = live.contains(&agent.id);
            if !keep {
                gone.push(agent.id);
                spent = spent.saturating_add(agent.runtime.long_task_time());
            }
            keep
        });
        self.agents.spent = spent;
        if !gone.is_empty() {
            self.agents.lowered = true;
        }
        if !gone.is_empty() {
            self.settle_ports();
        }
        for agent in gone {
            log::debug!("a script runtime of the page has no document left");
            self.frames.release_retired(agent);
        }
    }

    /// The agent with the first task that is due at `now`, and the first task of
    /// the other agents (ADR 0031 section 6: the earliest due time, then the lowest
    /// sequence number).
    pub(super) fn first_due_agent(&self, now: f64) -> Option<(AgentId, Option<(f64, u64)>)> {
        let mut first: Option<(AgentId, (f64, u64))> = None;
        let mut second: Option<(f64, u64)> = None;
        let before = |a: (f64, u64), b: (f64, u64)| a.0 < b.0 || (a.0 == b.0 && a.1 < b.1);
        for (id, runtime) in self.agent_runtimes() {
            let Some(key) = runtime.next_task_key() else {
                continue;
            };
            match first {
                Some((_, best)) if before(best, key) => {
                    if second.is_none_or(|s| before(key, s)) {
                        second = Some(key);
                    }
                }
                _ => {
                    second = first.map(|(_, best)| best);
                    first = Some((id, key));
                }
            }
        }
        let (agent, key) = first?;
        (key.0 <= now).then_some((agent, second))
    }

    /// The page time of the next task of all runtimes.
    pub(super) fn next_task_of_all(&self) -> Option<f64> {
        let top = self.script.as_ref().and_then(ScriptRuntime::next_task_time);
        if self.agents.others.is_empty() {
            return top;
        }
        self.agents
            .others
            .iter()
            .filter_map(|a| a.runtime.next_task_time())
            .chain(top)
            .reduce(f64::min)
    }
}

/// The origin of the agent `agent` among `others`.
pub(super) fn origin_of_agent(others: &[Agent], agent: AgentId) -> Option<Origin> {
    others
        .iter()
        .find(|a| a.id == agent)
        .map(|a| a.origin.clone())
}
