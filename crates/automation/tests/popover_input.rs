//! Popovers, dialogs and the user's input (M10 sub-features 6c and 6d, ADR
//! 0037 sections 3 to 6 and 10): the cases of the Chromium probe `tools/probes/web-top-layer.json`
//! that have input `steps` and belong to popovers run here with the same
//! steps (`swb_automation::run_steps`), and the value of each query is the one
//! that Chromium 148 gave (`tests/data/web-top-layer-steps.json`: the values
//! of the cases with steps, from three equal runs of `uv run --project tools
//! swbtools probe --json tools/probes/web-top-layer.json`, written with
//! `jq --slurpfile p tools/probes/web-top-layer.json '($p[0].cases |
//! map(select(.steps) | .name)) as $n | [.cases[] | select(.name as $x |
//! $n | index($x)) | {key: .name, value: [.results[].value]}] |
//! from_entries' chromium.json`).
//!
//! - The cases with scripts get one user activation after `load` and before
//!   the steps, as the probe tool's evaluations give (ADR 0037
//!   "Measurements"); the frame of a cross-origin document gets none.
//! - The `noscript-*` cases run with JavaScript off (Chromium ran them with a
//!   `Content-Security-Policy` that blocks scripts): the engine's own
//!   activation, light dismiss and close requests. Their queries are scripts
//!   in Chromium; here the same expressions are read from the page by the
//!   engine ([`off_value`]).
//! - The `toggle` events of a log are compared as the page's tasks make
//!   them after the whole log (the placement of their tasks among the steps
//!   varies from run to run in Chromium) and merged per element (a `toggle`
//!   task that waits takes the old state of the first change: two changes
//!   between two tasks make one event), see [`canon`].
//!
//! The dialog cases of sub-feature 6d (close requests, `cancel`, the close
//! watcher groups, `closedby=any`, the form method `dialog` without scripts)
//! run here too, and the inertness cases of sub-feature 6e (`inert-*`). The
//! key cases of `real_input.rs` are not run here.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use common::{on_page_thread, repo_root, value};
use serde_json::Value;
use swb_automation::{resolve_frame, run_steps};
use swb_engine::{FontContext, Page, PageConfig, Size, Url};
use swb_net::{Fetcher, Headers, NetError, Request, Response};

const MAIN: &str = "http://127.0.0.1:8000";
const OTHER: &str = "http://127.0.0.1:8001";

/// Cases that this test does not run: the key cases of `real_input.rs`, and
/// `inert-selection`, whose queries read `getSelection()` (swb has none:
/// backlog) and whose step (a double click on outside text selects a word of
/// the dialog) is a Chromium detail that swb does not copy (ADR 0037
/// section 11); `crates/engine/tests/modal_inert.rs` checks the engine's
/// selection instead.
fn is_later(name: &str) -> bool {
    name.starts_with("invokers-in-link-") || name == "inert-selection"
}

/// Queries where swb's value differs on purpose: (case, query index, swb's
/// value, why).
type Departure = (&'static str, usize, &'static str, &'static str);

/// The reason of the rows `invoker-inner-text` and `-textarea`.
const TEXT_IN_INVOKER: &str = "a click on a text field inside an invoking button: Chromium shows and hides \
     the popover in the one click (two `beforetoggle` events, no change); swb does nothing (ADR 0037 \
     \"6c implementation notes\")";

/// The reason of the rows of `inert-click` that read `interactivity`.
const NO_INTERACTIVITY: &str = "`getComputedStyle(el).interactivity` is `undefined` in swb: the property is backlog \
     (ADR 0037 section 11); the rest of the row (`inert` false everywhere, the frame's \
     document) matches";

const DEPARTURES: &[Departure] = &[
    (
        "inert-click",
        3,
        "button#ob:undefined:false dialog#d:undefined:false button#db:undefined:false \
         html:undefined:false body:undefined:false iframe#fr:undefined:false",
        NO_INTERACTIVITY,
    ),
    ("inert-click", 4, "undefined:button#fb", NO_INTERACTIVITY),
    (
        "invoker-inner-text",
        1,
        "click:input#i | false | - | false",
        TEXT_IN_INVOKER,
    ),
    (
        "invoker-inner-textarea",
        1,
        "click:textarea#i | false | - | ",
        TEXT_IN_INVOKER,
    ),
    (
        "popover-tab-order-source-other-tree",
        1,
        "x y z pb1 pb1 z y x | open=div#pp",
        "a popover whose `source` is the `body` follows the whole body in the order; \
         Shift+Tab that enters the page from its end goes to the popover's last element in swb, \
         to the last element of the body in Chromium (a rare case: ADR 0037 \"6c implementation \
         notes\")",
    ),
];

/// A directory with test pages, removed at the end of the test.
struct Site(PathBuf);

impl Site {
    fn new(name: &str) -> Site {
        let dir =
            std::env::temp_dir().join(format!("swb-popover-input-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Site(dir)
    }

    fn page(&self, html: &str) -> Url {
        let path = self.0.join("page.html");
        std::fs::write(&path, html).unwrap();
        Url::from_file_path(path).unwrap()
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The simulated server of a case with a cross-origin frame: its files under
/// `/case/`, served for both origins.
struct Sim {
    jar: swb_net::CookieJar,
    files: HashMap<String, String>,
}

impl Fetcher for Sim {
    fn cookie_jar(&self) -> Option<&swb_net::CookieJar> {
        Some(&self.jar)
    }

    fn fetch(&self, request: &Request) -> Result<Response, NetError> {
        let (status, body) = match self.files.get(request.url.path()) {
            Some(text) => (200, text.clone().into_bytes()),
            None => (404, b"not found".to_vec()),
        };
        Ok(Response {
            url: request.url.clone(),
            status,
            headers: [("content-type".to_owned(), "text/html".to_owned())]
                .into_iter()
                .collect::<Headers>(),
            body,
            redirected: false,
        })
    }
}

fn new_page(fetcher: Arc<dyn Fetcher>, javascript: bool) -> Page {
    let config = PageConfig {
        fetcher,
        notify: Arc::new(|| {}),
        network_threads: 2,
        scripts_allowed: true,
    };
    let mut page = Page::new(
        config,
        FontContext::for_tests(),
        Size::new(800.0, 600.0),
        1.0,
    );
    page.set_javascript(javascript);
    page
}

/// A page that shows the `html` of a case, loaded.
fn open(html: &str, javascript: bool, site: &Site) -> Page {
    let mut page = new_page(Arc::new(swb_net::NetworkFetcher::new()), javascript);
    page.navigate(site.page(html));
    assert!(page.wait_until_loaded(Duration::from_secs(30)));
    page
}

/// The page of a case with files (a cross-origin frame), loaded.
fn open_with_files(case: &Value, javascript: bool) -> Page {
    let mut files = HashMap::new();
    let substitute = |text: &str| {
        text.replace("$ORIGIN", MAIN)
            .replace("$OTHER", OTHER)
            .replace("$FOLDER", "case")
    };
    files.insert(
        "/case/case.html".to_owned(),
        substitute(case["html"].as_str().unwrap()),
    );
    for (name, resource) in case["resources"].as_object().into_iter().flatten() {
        let text = substitute(resource["text"].as_str().unwrap());
        files.insert(format!("/case/{name}"), text);
    }
    let sim = Sim {
        jar: swb_net::CookieJar::default(),
        files,
    };
    let mut page = new_page(Arc::new(sim), javascript);
    page.navigate(Url::parse(&format!("{MAIN}/case/case.html")).unwrap());
    assert!(page.wait_until_idle(Duration::from_millis(50), Duration::from_secs(30)));
    page.update_layout();
    page
}

fn load_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The probe file and Chromium's values.
fn load_cases() -> (Value, Value) {
    (
        load_json(&repo_root().join("tools/probes/web-top-layer.json")),
        load_json(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/web-top-layer-steps.json"),
        ),
    )
}

/// A log with the `toggle` events after the others, one for each element
/// (the old state of the first, the new state of the last), in the order of
/// their first appearance.
fn canon(text: &str) -> String {
    let mut rest = Vec::new();
    let mut toggles: Vec<(String, String, String)> = Vec::new();
    for token in text.split(' ') {
        let parsed = token.strip_prefix("toggle:").and_then(|t| {
            let (name, states) = t.split_once('(')?;
            let (old, new) = states.strip_suffix(')')?.split_once('>')?;
            Some((name.to_owned(), old.to_owned(), new.to_owned()))
        });
        match parsed {
            Some((name, old, new)) => match toggles.iter_mut().find(|t| t.0 == name) {
                Some(entry) => entry.2 = new,
                None => toggles.push((name, old, new)),
            },
            None => rest.push(token.to_owned()),
        }
    }
    let merged: Vec<String> = toggles
        .into_iter()
        .map(|(name, old, new)| format!("toggle:{name}({old}>{new})"))
        .collect();
    format!("{} || {}", rest.join(" "), merged.join(" "))
}

/// `canon` of a string value; other values stay.
fn canonical(v: &Value) -> Value {
    match v {
        Value::String(s) => Value::String(canon(s)),
        other => other.clone(),
    }
}

/// The failure of one query, if any.
fn compare(name: &str, index: usize, query: &str, want: &Value, got: &Value) -> Option<String> {
    let departure = DEPARTURES.iter().find(|d| d.0 == name && d.1 == index);
    match departure {
        Some((_, _, swb, _)) => {
            let ok = canonical(got) == canonical(&Value::String((*swb).to_owned()));
            (!ok)
                .then(|| format!("{name} #{index} (departure)\n  swb was: {swb}\n  actual:  {got}"))
        }
        None => (canonical(got) != canonical(want)).then(|| {
            format!(
                "{name} #{index}: {}\n  expected: {want}\n  actual:   {got}",
                &query[..query.len().min(100)]
            )
        }),
    }
}

/// The cases with steps to run, with their names, in the file's order.
fn cases_to_run(probe: &Value, javascript: bool) -> Vec<&Value> {
    probe["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| {
            let name = case["name"].as_str().unwrap();
            case.get("steps").is_some()
                && !is_later(name)
                && name.starts_with("noscript-") != javascript
        })
        .collect()
}

/// The steps that the automation API can send: a press of a modifier key
/// alone (`Shift`, `Control`, `Alt`, `Meta`) has no key in the engine
/// (`Key::from_dom` knows none), so no event reaches the page and no
/// activation is noted. Chromium notes none either (cases `xo-act-key-*`,
/// whose log leaves the modifier keys out), so the page sees the same.
fn sendable(steps: &[Value]) -> Vec<Value> {
    steps
        .iter()
        .filter(|step| {
            !matches!(
                step.get("key").and_then(Value::as_str),
                Some("Shift" | "Control" | "Alt" | "Meta")
            )
        })
        .cloned()
        .collect()
}

/// The cases with scripts: one activation after `load`, then the steps.
#[test]
fn cases_with_scripts() {
    on_page_thread(|| {
        let (probe, expected) = load_cases();
        let mut failures = Vec::new();
        let mut run = 0;
        for case in cases_to_run(&probe, true) {
            run += 1;
            let name = case["name"].as_str().unwrap();
            let site = Site::new(name);
            let mut page = if case.get("resources").is_some() {
                open_with_files(case, true)
            } else {
                open(case["html"].as_str().unwrap(), true, &site)
            };
            // The probe tool's evaluations before the steps are user gestures.
            page.note_user_activation();
            if let Err(error) = run_steps(&mut page, &sendable(case["steps"].as_array().unwrap())) {
                failures.push(format!("{name}: {error}"));
                continue;
            }
            for (index, query) in case["measure"].as_array().unwrap().iter().enumerate() {
                let js = query["js"].as_str().unwrap();
                // A query with `frame` runs in that frame's document.
                let got = match query.get("frame").and_then(Value::as_str) {
                    Some(path) => {
                        let frame = resolve_frame(&mut page, path).unwrap();
                        page.with_frame(frame, |p| value(p, js)).unwrap()
                    }
                    None => value(&mut page, js),
                };
                let want = &expected[name][index];
                failures.extend(compare(name, index, js, want, &got));
            }
        }
        assert!(run > 60, "{run} cases with scripts");
        assert!(
            failures.is_empty(),
            "{} failures\n{}",
            failures.len(),
            failures.join("\n")
        );
    });
}

// ----- Cases without scripts -----

/// The id of the element that `selector` names (`'#x'`).
fn node_of(page: &Page, id: &str) -> swb_dom::NodeId {
    page.query_selector_all(&format!("#{id}"))
        .and_then(|nodes| nodes.first().copied())
        .unwrap_or_else(|| panic!("no element #{id}"))
}

/// The value of one item of a query without scripts: the states that the
/// queries of the `noscript-*` cases read.
fn off_item(page: &Page, item: &str) -> String {
    let id_of = |prefix: &str, suffix: &str| {
        item.strip_prefix(prefix)
            .and_then(|r| r.strip_suffix(suffix))
            .map(str::to_owned)
    };
    if let Some(id) = id_of("document.querySelector('#", "').matches(':popover-open')") {
        let node = node_of(page, &id);
        return page
            .document()
            .unwrap()
            .is_popover_showing(node)
            .to_string();
    }
    if let Some(id) = id_of("document.querySelector('#", "').open") {
        let node = node_of(page, &id);
        let doc = page.document().unwrap();
        return doc.element(node).unwrap().has_attr("open").to_string();
    }
    if let Some(id) = id_of("document.querySelector('#", "').returnValue") {
        let node = node_of(page, &id);
        return page
            .document()
            .unwrap()
            .dialog_return_value(node)
            .to_owned();
    }
    if let Some(id) = id_of("document.querySelector('#", "').checked") {
        let node = node_of(page, &id);
        return page.control_checked(node).unwrap().to_string();
    }
    match item {
        "document.activeElement.id" => page
            .focused_element()
            .and_then(|n| {
                page.document()?
                    .element(n)
                    .and_then(|e| e.attr("id").map(str::to_owned))
            })
            .unwrap_or_default(),
        "location.hash||'-'" => match page.url().unwrap().fragment() {
            Some(fragment) if !fragment.is_empty() => format!("#{fragment}"),
            _ => "-".to_owned(),
        },
        "location.hash" => match page.url().unwrap().fragment() {
            Some(fragment) if !fragment.is_empty() => format!("#{fragment}"),
            _ => String::new(),
        },
        other => panic!("a query without scripts has an item that the test does not know: {other}"),
    }
}

/// The value of a query of a case without scripts, as Chromium's script gave
/// it: `typeof window.RAN` is `"undefined"` (no script ran), `[items].join()`
/// a string, one item a boolean.
fn off_value(page: &Page, js: &str) -> Value {
    if js == "typeof window.RAN" {
        return Value::String("undefined".to_owned());
    }
    if let Some(list) = js
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix("].join()"))
    {
        let items: Vec<String> = list.split(',').map(|item| off_item(page, item)).collect();
        return Value::String(items.join(","));
    }
    match off_item(page, js).as_str() {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        other => Value::String(other.to_owned()),
    }
}

/// One step of a case without scripts: `focus` goes through the engine (it is
/// a script in the steps of the probe), the others through `run_steps`.
fn run_off_step(page: &mut Page, step: &Value) -> Result<(), String> {
    if let Some(selector) = step.get("focus").and_then(Value::as_str) {
        let node = page
            .query_selector_all(selector)
            .and_then(|nodes| nodes.first().copied())
            .ok_or("no element to focus")?;
        page.focus(Some(node), true);
        return Ok(());
    }
    run_steps(page, std::slice::from_ref(step))
}

/// The inputs of the cases `xo-act-*` on a page without scripts: the engine's
/// own activation rule (which inputs note a user activation) gives what
/// Chromium measured with scripts (the last `poll:` of the frame's log says
/// whether `navigator.userActivation` was active). The activation shows as
/// the history-action activation of the document.
#[test]
fn inputs_that_activate_with_scripts_off() {
    on_page_thread(|| {
        let (probe, expected) = load_cases();
        let html = "<!DOCTYPE html><style>body{margin:0}button{width:100px;height:50px;margin:0}</style>\
                    <button id=b>b</button>";
        let mut failures = Vec::new();
        let mut run = 0;
        for case in probe["cases"].as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            if !name.starts_with("xo-act-") || name.starts_with("xo-act-after-") {
                continue;
            }
            run += 1;
            let want = expected[name][0]
                .as_str()
                .unwrap()
                .ends_with("poll:true/true");
            let site = Site::new(name);
            let mut page = open(html, false, &site);
            assert!(!page.document().unwrap().has_history_action_activation());
            if let Err(error) = run_steps(&mut page, &sendable(case["steps"].as_array().unwrap())) {
                failures.push(format!("{name}: {error}"));
                continue;
            }
            let got = page.document().unwrap().has_history_action_activation();
            if got != want {
                failures.push(format!("{name}: activation {got}, Chromium {want}"));
            }
        }
        assert_eq!(run, 18, "the input cases xo-act-*");
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}

/// The cases `noscript-*`: the engine's own invokers, light dismiss, close
/// requests and focus, with JavaScript off.
#[test]
fn cases_without_scripts() {
    on_page_thread(|| {
        let (probe, expected) = load_cases();
        let mut failures = Vec::new();
        let mut run = 0;
        for case in cases_to_run(&probe, false) {
            run += 1;
            let name = case["name"].as_str().unwrap();
            let site = Site::new(name);
            let mut page = open(case["html"].as_str().unwrap(), false, &site);
            page.note_user_activation();
            if let Some(error) = case["steps"]
                .as_array()
                .unwrap()
                .iter()
                .find_map(|step| run_off_step(&mut page, step).err())
            {
                failures.push(format!("{name}: {error}"));
                continue;
            }
            page.wait_until_loaded(Duration::from_secs(5));
            for (index, query) in case["measure"].as_array().unwrap().iter().enumerate() {
                let js = query["js"].as_str().unwrap();
                let got = off_value(&page, js);
                let want = &expected[name][index];
                failures.extend(compare(name, index, js, want, &got));
            }
        }
        assert!(run > 25, "{run} cases without scripts");
        assert!(
            failures.is_empty(),
            "{} failures\n{}",
            failures.len(),
            failures.join("\n")
        );
    });
}

#[test]
fn canon_merges_and_moves_the_toggles() {
    assert_eq!(
        canon("a toggle:p(closed>open) b toggle:p(open>closed) toggle:q(closed>open)"),
        "a b || toggle:p(closed>closed) toggle:q(closed>open)"
    );
    assert_eq!(canon("x"), "x || ");
}
