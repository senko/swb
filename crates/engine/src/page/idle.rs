//! Idle periods (HTML "start an idle period" and step 4 of the event loop
//! of ADR 0028 section 7; Cooperative Scheduling of Background Tasks,
//! <https://w3c.github.io/requestidlecallback/#start-an-event-loop-turn>;
//! M8 feature 8a2): the callbacks of `requestIdleCallback` run when no task
//! is due and no frame is due.
//!
//! A period ends at the next task or frame, or after 50 ms
//! ([`swb_web::IDLE_PERIOD_MS`]), whichever comes first. The next period
//! cannot start before the end of the last one (`idle_next`), so a page
//! whose idle callback requests the next one runs it 20 times a second when
//! nothing else is due (Chromium, measured), not in a busy loop. In virtual
//! time the clock jumps to the start of the next period like to a task, so
//! a headless run with such a loop ends with the run's time limit.
//!
//! A pending idle callback keeps the page from being idle (`is_idle`: the
//! `waitForIdle` of the automation API waits for it to run), except for a
//! loop: after [`LOOP_PERIODS`] periods in a row with no task and no
//! network completion between them, the callbacks count as a loop that is
//! idle, as a `requestAnimationFrame` loop is.

use super::Page;

/// The number of idle periods in a row after which the page counts the
/// idle callbacks as a loop and is idle in spite of them.
const LOOP_PERIODS: u32 = 3;

impl Page {
    /// The page time at which the next idle period can start, if idle
    /// callbacks wait. None after the scripts stopped.
    pub(super) fn idle_time(&self) -> Option<f64> {
        let script = self.script.as_ref()?;
        (self.frames_run() && script.has_idle_callbacks()).then_some(self.input.idle_next)
    }

    /// True if idle callbacks wait that `horizon` (a page time) would see
    /// run, and that are not a loop.
    pub(super) fn idle_work_before(&self, horizon: f64) -> bool {
        self.idle_time().is_some_and(|time| time <= horizon)
            && self.input.idle_streak < LOOP_PERIODS
    }

    /// Starts an idle period if one can start: idle callbacks wait, the
    /// last period is over, and no task and no frame is due. Returns true
    /// if a callback ran.
    pub(super) fn run_idle(&mut self) -> bool {
        let now = self.clock.now();
        if self.idle_time().is_none_or(|time| time > now) || !self.script_input() {
            return false;
        }
        let next_task = self
            .script
            .as_ref()
            .and_then(swb_web::ScriptRuntime::next_task_time);
        if next_task.is_some_and(|time| time <= now)
            || !self.traversals.is_empty()
            || self.frame_time().is_some_and(|time| time <= now)
        {
            return false;
        }
        let deadline = next_task
            .into_iter()
            .chain(self.frame_time())
            .fold(now + swb_web::IDLE_PERIOD_MS, f64::min);
        self.input.idle_next = deadline;
        self.input.idle_streak += 1;
        self.run_task(|script| script.run_idle_callbacks(deadline))
            .is_some_and(|ran| ran > 0)
    }
}
