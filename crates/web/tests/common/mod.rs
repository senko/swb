//! What the runtime tests share: a test document that stands for the
//! engine's, a runtime with a parsed page, evaluation as the probe prints
//! values, and the run of the cases of a Chromium probe file.

#![allow(dead_code, reason = "each test crate uses a part")]

use std::any::Any;

use swb_dom::Document;
use swb_js::{HeapConfig, RuntimeConfig};
use swb_web::{EngineHost, Evaluation, JsonForm, ScriptRuntime};

/// The URL of the test documents.
pub(crate) const URL: &str = "file:///probe/page.html";

/// A probe case: its name, its page and its queries with Chromium's
/// values.
pub(crate) struct Case {
    pub(crate) name: &'static str,
    pub(crate) html: &'static str,
    pub(crate) queries: &'static [(&'static str, &'static str)],
}

/// A value where swb departs from Chromium on purpose: (case, query index,
/// swb's value, why).
pub(crate) type Departure = (&'static str, usize, &'static str, &'static str);

/// A document for the runtime: what the engine lends.
pub(crate) struct TestDocument {
    pub(crate) dom: Document,
}

impl EngineHost for TestDocument {
    fn dom(&self) -> &Document {
        &self.dom
    }

    fn dom_mut(&mut self) -> &mut Document {
        &mut self.dom
    }

    fn url(&self) -> String {
        URL.to_owned()
    }

    fn base_url(&mut self) -> String {
        URL.to_owned()
    }

    fn parse_url(&mut self, value: &str) -> Option<String> {
        Some(format!("file:///probe/{value}"))
    }

    fn character_set(&self) -> &'static str {
        // A file without a declared encoding (as the probe's pages).
        "windows-1252"
    }

    fn content_type(&self) -> &'static str {
        "text/html"
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

/// A runtime (in the GC stress mode with `stress`) with `html` parsed and
/// its inline scripts run in tree order (what the engine does at the end
/// of the parse).
pub(crate) fn load(html: &str, stress: bool) -> ScriptRuntime {
    let config = RuntimeConfig {
        heap: HeapConfig {
            stress,
            limit: swb_web::HEAP_LIMIT,
            ..HeapConfig::default()
        },
        ..RuntimeConfig::default()
    };
    let mut rt = ScriptRuntime::with_config(config).expect("a runtime");
    let dom = swb_dom::parse_html_with(html, true);
    rt.lend(Box::new(TestDocument { dom }));
    rt.run_inline_scripts(URL);
    rt
}

/// The value of an evaluation as the probe prints it: parsed JSON, or
/// `error: ...`.
pub(crate) fn answer(rt: &mut ScriptRuntime, expression: &str) -> serde_json::Value {
    match rt.evaluate(expression, JsonForm::Strict) {
        Evaluation::Value(text) => serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{expression}: invalid JSON {text}: {e}")),
        Evaluation::Error(text) => serde_json::Value::String(format!("error: {text}")),
    }
}

/// Parses an expected value (JSON text, or `error: ...`).
pub(crate) fn expected(text: &str) -> serde_json::Value {
    if text.starts_with("error: ") {
        return serde_json::Value::String(text.to_owned());
    }
    serde_json::from_str(text).expect("valid JSON")
}

/// Runs `f` on a thread with a big stack (the runtime recurses on the
/// native stack).
pub(crate) fn on_big_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(f)
        .expect("a thread")
        .join()
        .expect("the test thread")
}

/// Runs every case with and without the GC stress mode, each case in one
/// runtime with its queries in order (as the probe runs them in one page),
/// and returns the differences from Chromium's values (or from the
/// departures).
pub(crate) fn probe_failures(
    cases: &'static [Case],
    departures: &'static [Departure],
) -> Vec<String> {
    on_big_stack(move || {
        let mut failures = Vec::new();
        for case in cases {
            for stress in [false, true] {
                let mut rt = load(case.html, stress);
                for (index, &(query, chromium)) in case.queries.iter().enumerate() {
                    let want = departures
                        .iter()
                        .find(|v| v.0 == case.name && v.1 == index)
                        .map_or(chromium, |v| v.2);
                    let got = answer(&mut rt, query);
                    if got != expected(want) {
                        failures.push(format!(
                            "{} #{index} (stress {stress}): {query}\n  expected: {want}\n  actual:   {got}",
                            case.name
                        ));
                    }
                }
            }
        }
        failures
    })
}
