//! What the runtime tests share: a test document that stands for the
//! engine's, a runtime with a parsed page, evaluation as the probe prints
//! values, and the run of the cases of a Chromium probe file.

#![allow(dead_code, reason = "each test crate uses a part")]

use std::any::Any;

use swb_dom::{Document, HtmlParser, ParseStep, TextPosition};
use swb_js::{HeapConfig, RuntimeConfig};
use swb_web::{ClockMode, EngineHost, Evaluation, JsonForm, PageClock, ScriptRuntime};

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

/// A document for the runtime: what the engine lends, with the parser of
/// a parse in steps.
pub(crate) struct TestDocument {
    pub(crate) dom: Document,
    pub(crate) parser: Option<HtmlParser>,
    /// The size of the page in bytes (the navigation entry of the
    /// performance timeline).
    pub(crate) bytes: u64,
    /// The requests that scripts started with `fetch()` or XHR (see
    /// [`TestDocument::accepting_fetches`]); the id of a request is its
    /// number from 1.
    pub(crate) started: Vec<swb_net::Request>,
    /// Whether `start_api_fetch` accepts requests (else it refuses them).
    pub(crate) accept_fetches: bool,
}

impl TestDocument {
    /// A parsed document (no parser).
    pub(crate) fn parsed(dom: Document) -> TestDocument {
        TestDocument {
            dom,
            parser: None,
            bytes: 0,
            started: Vec::new(),
            accept_fetches: false,
        }
    }

    /// A document that accepts the requests of scripts and keeps them in
    /// `started`; the test hands the results to the runtime.
    pub(crate) fn accepting_fetches(html: &str) -> TestDocument {
        let mut doc = TestDocument::parsing(html);
        doc.accept_fetches = true;
        doc
    }

    /// A document that the runtime parses in steps.
    pub(crate) fn parsing(html: &str) -> TestDocument {
        let mut dom = Document::new();
        let parser = HtmlParser::new(&mut dom, html, true);
        TestDocument {
            dom,
            parser: Some(parser),
            bytes: html.len() as u64,
            started: Vec::new(),
            accept_fetches: false,
        }
    }
}

impl TestDocument {
    /// The document base URL: the first `base` element with an `href`
    /// (resolved against the document URL), else the document URL.
    fn current_base(&self) -> url::Url {
        let document = url::Url::parse(URL).expect("the test URL");
        let href = self
            .dom
            .find_element(swb_dom::NodeId::DOCUMENT, |e| {
                e.is_html() && &**e.local_name() == "base" && e.has_attr("href")
            })
            .and_then(|n| self.dom.element(n))
            .and_then(|e| e.attr("href"));
        href.and_then(|h| document.join(h).ok()).unwrap_or(document)
    }
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
        self.current_base().to_string()
    }

    fn parse_url(&mut self, value: &str) -> Option<String> {
        self.current_base().join(value).ok().map(String::from)
    }

    fn parse_url_against_document_url(&mut self, value: &str) -> Option<String> {
        let document = url::Url::parse(URL).ok()?;
        document.join(value).ok().map(String::from)
    }

    fn character_set(&self) -> &'static str {
        // A file without a declared encoding (as the probe's pages).
        "windows-1252"
    }

    fn content_type(&self) -> &'static str {
        "text/html"
    }

    fn navigation_info(&self) -> swb_web::NavigationInfo {
        swb_web::NavigationInfo {
            kind: swb_web::NavigationKind::Navigate,
            status: 200,
            body_bytes: self.bytes,
        }
    }

    fn start_api_fetch(&mut self, request: swb_net::Request) -> Option<u64> {
        if !self.accept_fetches {
            return None;
        }
        self.started.push(request);
        Some(self.started.len() as u64)
    }

    fn parse_step(&mut self) -> Option<(ParseStep, TextPosition)> {
        let parser = self.parser.as_mut()?;
        if let Ok(ParseStep::Script(node)) = parser.resume(&mut self.dom) {
            return Some((ParseStep::Script(node), parser.script_position()));
        }
        self.parser = None;
        Some((ParseStep::Finished, TextPosition::default()))
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

/// A runtime (in the GC stress mode with `stress`, in real time) with
/// `html` parsed with its scripts (external scripts fail to load), the
/// `load` event fired and the tasks run that are due then.
pub(crate) fn load(html: &str, stress: bool) -> ScriptRuntime {
    load_in(html, stress, ClockMode::Real)
}

/// [`load`] with a page clock in `mode`. In virtual time timers do not
/// wait, and the order of tasks does not depend on the speed of the
/// machine.
pub(crate) fn load_in(html: &str, stress: bool, mode: ClockMode) -> ScriptRuntime {
    load_with(TestDocument::parsing(html), stress, mode)
}

/// [`load_in`] with a document of the caller's.
pub(crate) fn load_with(doc: TestDocument, stress: bool, mode: ClockMode) -> ScriptRuntime {
    let config = RuntimeConfig {
        heap: HeapConfig {
            stress,
            limit: swb_web::HEAP_LIMIT,
            ..HeapConfig::default()
        },
        ..RuntimeConfig::default()
    };
    let clock = PageClock::new(mode, None);
    let mut rt = ScriptRuntime::with_config(config, clock).expect("a runtime");
    rt.lend(Box::new(doc));
    rt.start_document();
    // Nothing delays the load event of the test document.
    rt.fire_load_event();
    // The tasks that the scripts queued for now (notifications of
    // rejected promises; timers of 0 ms).
    rt.run_due_tasks(std::time::Duration::from_secs(5));
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
    probe_failures_in(cases, departures, ClockMode::Real)
}

/// [`probe_failures`] with a page clock in `mode`.
pub(crate) fn probe_failures_in(
    cases: &'static [Case],
    departures: &'static [Departure],
    mode: ClockMode,
) -> Vec<String> {
    on_big_stack(move || {
        let mut failures = Vec::new();
        for case in cases {
            for stress in [false, true] {
                let mut rt = load_in(case.html, stress, mode);
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
