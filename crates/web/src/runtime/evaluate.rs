//! The evaluation of source text from outside the page (the automation server
//! and tests): a script runs as a task, and its value or error comes back as
//! JSON text. The serializer is a function that the runtime compiles before any
//! page script.

use std::time::{Duration, Instant};

use swb_js::{PromiseStatus, ReactionKind, Runtime, Value, VmResult};

use super::{
    Evaluation, EvaluationId, EvaluationStart, JsonForm, STOPPED, ScriptRuntime, error_text_of,
    pin, serialize, state_mut, task_error, thrown_text, unpin,
};

impl ScriptRuntime {
    /// Evaluates `source` as a classic script in the global scope (the
    /// value of the last expression statement) as a task, and waits for a
    /// promise through this runtime's event loop (at most
    /// [`EVALUATION_TIMEOUT`] of real time; virtual time jumps to the next
    /// task). Returns the JSON text of the value in `form`, or the error.
    /// The engine, which also waits for the network, uses
    /// [`ScriptRuntime::start_evaluation`] instead.
    pub fn evaluate(&mut self, source: &str, form: JsonForm) -> Evaluation {
        let id = match self.start_evaluation(source, form) {
            EvaluationStart::Done(evaluation) => return evaluation,
            EvaluationStart::Pending(id) => id,
        };
        let deadline = Instant::now() + EVALUATION_TIMEOUT;
        loop {
            if let Some(evaluation) = self.poll_evaluation(id, form) {
                return evaluation;
            }
            if self.run_due_tasks(Duration::from_millis(16)) > 0 {
                continue;
            }
            let now = Instant::now();
            match self.next_task_time() {
                Some(time) if now < deadline => {
                    if self.clock.is_virtual() {
                        self.clock.advance_to(time);
                    } else {
                        let until = self.clock.instant_of(time).min(deadline);
                        std::thread::sleep(until.saturating_duration_since(now));
                    }
                }
                _ => break,
            }
        }
        self.cancel_evaluation(id);
        Evaluation::Error(NOT_SETTLED.to_owned())
    }

    /// Evaluates `source` as a classic script in the global scope as a
    /// task. A value that is not a promise, a settled promise (after the
    /// microtask checkpoint) and an error are done; a pending promise waits
    /// for [`ScriptRuntime::poll_evaluation`]. The evaluation handles a
    /// rejection of its promise, so it is not reported as unhandled.
    pub fn start_evaluation(&mut self, source: &str, form: JsonForm) -> EvaluationStart {
        let mut error_text = None;
        let started = self.task("evaluate", |rt| {
            let realm = crate::state::doc_realm(rt).map_err(task_error)?;
            let value = match rt.eval_named_at(realm, source, "", (0, 0)) {
                Ok(value) => value,
                Err(error) => {
                    error_text = Some(thrown_text(rt, &error));
                    return Err(error);
                }
            };
            if rt.promise_settlement(value).is_none() {
                pin(rt, value);
                let text = serialize(rt, value, form, false);
                unpin(rt);
                return Ok(EvaluationStart::Done(match text {
                    Ok(text) => Evaluation::Value(text),
                    Err(error) => Evaluation::Error(error_text_of(&error)),
                }));
            }
            // A reaction marks the promise as handled.
            rt.react_to_promise(value, ignore_settlement, Value::Undefined)?;
            let event_loop = &mut state_mut(rt).map_err(task_error)?.event_loop;
            let id = event_loop.next_evaluation;
            event_loop.next_evaluation += 1;
            event_loop.evaluations.push((id, value));
            Ok(EvaluationStart::Pending(EvaluationId(id)))
        });
        match started {
            Ok(EvaluationStart::Pending(id)) => match self.poll_evaluation(id, form) {
                Some(evaluation) => EvaluationStart::Done(evaluation),
                None => EvaluationStart::Pending(id),
            },
            Ok(done) => done,
            Err(error) => EvaluationStart::Done(Evaluation::Error(
                error_text.unwrap_or_else(|| error_text_of(&error)),
            )),
        }
    }

    /// The result of a pending evaluation once its promise settled (the
    /// JSON text of the value, or the error of a rejection), else `None`.
    /// A stopped document gives an error.
    pub fn poll_evaluation(&mut self, id: EvaluationId, form: JsonForm) -> Option<Evaluation> {
        let promise = crate::state::state(&self.rt)
            .ok()?
            .event_loop
            .evaluations
            .iter()
            .find(|(i, _)| *i == id.0)
            .map(|&(_, value)| value);
        let Some(promise) = promise else {
            return Some(Evaluation::Error(NOT_SETTLED.to_owned()));
        };
        let settlement = self.rt.promise_settlement(promise);
        let (value, is_error) = match settlement {
            Some((PromiseStatus::Fulfilled, result)) => (result, false),
            Some((PromiseStatus::Rejected, reason)) => (reason, true),
            _ if self.stopped => {
                self.cancel_evaluation(id);
                return Some(Evaluation::Error(error_text_of(&STOPPED)));
            }
            _ => return None,
        };
        let text = self.task("evaluate", |rt| {
            pin(rt, value);
            let text = serialize(rt, value, form, is_error);
            unpin(rt);
            text
        });
        self.cancel_evaluation(id);
        Some(match text {
            Ok(text) if is_error => Evaluation::Error(text),
            Ok(text) => Evaluation::Value(text),
            Err(error) => Evaluation::Error(error_text_of(&error)),
        })
    }

    /// True if the promise of a pending evaluation settled (or the
    /// evaluation is unknown, or the document's scripts stopped), so that
    /// [`ScriptRuntime::poll_evaluation`] gives a result.
    pub fn evaluation_settled(&self, id: EvaluationId) -> bool {
        let Ok(state) = crate::state::state(&self.rt) else {
            return true;
        };
        let promise = state
            .event_loop
            .evaluations
            .iter()
            .find(|(i, _)| *i == id.0);
        match promise {
            Some(&(_, promise)) => {
                self.stopped
                    || !matches!(
                        self.rt.promise_settlement(promise),
                        Some((PromiseStatus::Pending, _))
                    )
            }
            None => true,
        }
    }

    /// Forgets a pending evaluation (its time ran out).
    pub fn cancel_evaluation(&mut self, id: EvaluationId) {
        if let Ok(state) = state_mut(&mut self.rt) {
            state.event_loop.evaluations.retain(|(i, _)| *i != id.0);
        }
    }
}

/// The function that writes the JSON text of an evaluated value, or the
/// text of an error. It is compiled when the runtime is created, before any
/// page script, and keeps the built-ins it uses, so that a page that
/// replaces `JSON.stringify` or `Array.isArray` does not change it.
pub(super) const SERIALIZER: &str = r"(() => {
  const isArray = Array.isArray, keys = Object.keys, stringify = JSON.stringify, S = String;
  const apply = Reflect.apply, wellFormed = String.prototype.toWellFormed;
  // Strict JSON parsers refuse the escape of a lone surrogate.
  function string(s, strict) { return stringify(strict ? apply(wellFormed, s, []) : s); }
  function number(n, strict) {
    if (n !== n) return strict ? 'null' : 'NaN';
    if (n === Infinity) return strict ? 'null' : 'Infinity';
    if (n === -Infinity) return strict ? 'null' : '-Infinity';
    if (n === 0 && 1 / n < 0) return strict ? '0' : '-0.0';
    return S(n);
  }
  function value(v, strict, seen) {
    switch (typeof v) {
      case 'boolean': return v ? 'true' : 'false';
      case 'number': return number(v, strict);
      case 'bigint': return S(v);
      case 'string': return string(v, strict);
      case 'object': break;
      default: return 'null';
    }
    if (v === null || seen.length > 100) return 'null';
    for (let i = 0; i < seen.length; i++) if (seen[i] === v) return 'null';
    seen[seen.length] = v;
    let out = '';
    if (isArray(v)) {
      for (let i = 0; i < v.length; i++) out += (i ? ',' : '') + value(v[i], strict, seen);
      out = '[' + out + ']';
    } else {
      const ks = keys(v);
      for (let i = 0; i < ks.length; i++) out += (i ? ',' : '') + string(ks[i], strict) + ':' + value(v[ks[i]], strict, seen);
      out = '{' + out + '}';
    }
    seen.length = seen.length - 1;
    return out;
  }
  function error(e) {
    try {
      if (e !== null && typeof e === 'object' && 'message' in e) return S(e.name) + ': ' + S(e.message);
      return S(e);
    } catch (x) {
      return 'a thrown value that has no text';
    }
  }
  return (v, strict, isError) => isError ? error(v) : value(v, strict, []);
})()";

/// The longest that [`ScriptRuntime::evaluate`] waits for a promise.
pub const EVALUATION_TIMEOUT: Duration = Duration::from_secs(30);

/// The error of an evaluation whose promise did not settle in time.
const NOT_SETTLED: &str = "Error: the promise did not settle in time";

/// The reaction that marks an evaluated promise as handled.
#[allow(clippy::unnecessary_wraps, reason = "the type of a host reaction")]
fn ignore_settlement(
    _rt: &mut Runtime,
    _data: Value,
    _kind: ReactionKind,
    _value: Value,
) -> VmResult<Value> {
    Ok(Value::Undefined)
}
