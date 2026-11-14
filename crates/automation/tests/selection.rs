//! The selection in the engine (M10 sub-feature 9b-2, ADR 0039): every case of
//! the Chromium probe `tools/probes/web-selection.json` without pixel queries
//! runs here with its input steps (`swb_automation::run_steps`), and the
//! values of its queries are Chromium 148's (`tests/data/web-selection.json`:
//! three equal runs of `just probe --json`, ADR 0039 "Measurements"). The
//! engine gives what the runtime tests of `crates/web/tests/selection.rs`
//! cannot: the rendered text of `toString`, the user's gestures as ranges,
//! text controls, frames and the caret positions of `containsNode`. The tests
//! below check the tie in both directions beyond the probe.

// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;
use common::{on_page_thread, repo_root};

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use swb_automation::run_steps;
use swb_engine::{Evaluation, FontContext, JsonForm, Page, PageConfig, Size, Url};
use swb_net::NetworkFetcher;

/// The values where swb departs from Chromium on purpose: (case, query index,
/// swb's value as JSON, why).
const DEPARTURES: &[(&str, usize, &str, &str)] = &[
    (
        "api",
        0,
        r#"["function",0,"function Selection() { [native code] }",true,true,"[object Selection]","length,name,prototype","anchorNode,anchorOffset,focusNode,focusOffset,isCollapsed,rangeCount,type,direction,baseNode,baseOffset,extentNode,extentOffset,addRange,collapse,collapseToEnd,collapseToStart,containsNode,empty,extend,getRangeAt,removeAllRanges,removeRange,selectAllChildren,setBaseAndExtent,setPosition,toString,constructor","TypeError: Failed to construct 'Selection': Illegal constructor","TypeError: Illegal constructor"]"#,
        MISSING_MEMBERS,
    ),
    (
        "api",
        1,
        r#"["anchorNode acc:function:undefined:undefined:true:true:","anchorOffset acc:function:undefined:undefined:true:true:","focusNode acc:function:undefined:undefined:true:true:","focusOffset acc:function:undefined:undefined:true:true:","isCollapsed acc:function:undefined:undefined:true:true:","rangeCount acc:function:undefined:undefined:true:true:","type acc:function:undefined:undefined:true:true:","direction acc:function:undefined:undefined:true:true:","baseNode acc:function:undefined:undefined:true:true:","extentNode acc:function:undefined:undefined:true:true:","toString val:undefined:undefined:function:true:true:true","getComposedRanges none","setPosition val:undefined:undefined:function:true:true:true","collapse val:undefined:undefined:function:true:true:true","modify none","val:undefined:undefined:function:true:true:true","none","val:undefined:undefined:function:true:true:true","val:undefined:undefined:function:false:true:true","acc:function:function:undefined:true:true:","none","acc:function:function:undefined:true:true:","acc:function:function:undefined:true:true:","acc:function:function:undefined:true:true:","acc:function:function:undefined:true:true:","none"]"#,
        "getComposedRanges, modify and ShadowRoot.getSelection are absent (ADR 0039 \
         section 5; backlog)",
    ),
    (
        "api",
        2,
        r#"["addRange:1:addRange","collapse:1:collapse","collapseToEnd:0:collapseToEnd","collapseToStart:0:collapseToStart","containsNode:1:containsNode","empty:0:empty","extend:1:extend","getRangeAt:1:getRangeAt","removeAllRanges:0:removeAllRanges","removeRange:1:removeRange","selectAllChildren:1:selectAllChildren","setBaseAndExtent:4:setBaseAndExtent","setPosition:1:setPosition","toString:0:toString","constructor:0:Selection",0,0,false,false]"#,
        MISSING_MEMBERS,
    ),
    (
        "collapse-extend",
        6,
        r#"["undefined","TypeError: s.modify is not a function","Caret 1 t:alpha beta gamma,1 t:alpha beta gamma,1 true none t:alpha beta gamma,1-t:alpha beta gamma,1","TypeError: s.modify is not a function","Caret 1 t:alpha beta gamma,1 t:alpha beta gamma,1 true none t:alpha beta gamma,1-t:alpha beta gamma,1","TypeError: s.modify is not a function","Caret 1 t:alpha beta gamma,1 t:alpha beta gamma,1 true none t:alpha beta gamma,1-t:alpha beta gamma,1","TypeError: s.modify is not a function","Caret 1 t:alpha beta gamma,1 t:alpha beta gamma,1 true none t:alpha beta gamma,1-t:alpha beta gamma,1"]"#,
        "Selection.modify (non-standard, needs caret movement by character and word) is \
         absent (ADR 0039 section 5; backlog)",
    ),
    (
        "shadow",
        0,
        r#"["Range 1 t:shadow text,1 t:shadow text,3 false forward t:shadow text,1-t:shadow text,3","\"ha\"","undefined",null,null,"TypeError: s.getComposedRanges is not a function","TypeError: s.getComposedRanges is not a function","TypeError: s.getComposedRanges is not a function","TypeError: s.getComposedRanges is not a function","Caret 1 t:shadow text,2 t:shadow text,2 true none t:shadow text,2-t:shadow text,2","TypeError: s.getComposedRanges is not a function","TypeError: s.getComposedRanges is not a function","Range 1 t:shadow text,1 t:shadow text,2 false forward t:shadow text,1-t:shadow text,2",true,"true","false"]"#,
        "getComposedRanges and ShadowRoot.getSelection are absent (backlog); an anchor in \
         the document and a focus in a shadow tree collapse at the focus, with the type \
         and direction of a caret (ADR 0039 section 3; Chromium keeps a composed selection \
         and reports Range and forward)",
    ),
    (
        "shadow",
        1,
        r#"["Range 1 t:closed text,0 t:closed text,2 false forward t:closed text,0-t:closed text,2","TypeError: s.getComposedRanges is not a function","TypeError: s.getComposedRanges is not a function"]"#,
        "getComposedRanges is absent (ADR 0039 decision 2; backlog)",
    ),
    (
        "user-dblclick",
        0,
        r#""Range 1 t:alpha beta gamma,5 t:alpha beta gamma,6 false forward t:alpha beta gamma,5-t:alpha beta gamma,6""#,
        "the double click is at x = 72, 0.01 px inside the space before 'beta' in \
         Chromium's Liberation Mono (advance 12.002 px); Chromium selects 'beta', swb the \
         space under the point (the test fonts differ too)",
    ),
    (
        "shadow-user-drag",
        0,
        r#""Range 1 t:shadow text here,1 t:shadow text here,7 false forward t:shadow text here,1-t:shadow text here,7""#,
        "a user's selection in a shadow tree is reported with the shadow tree's nodes \
         (ADR 0039 section 3); Chromium reports a range collapsed at the host's place and \
         the inner range to ShadowRoot.getSelection",
    ),
    (
        "shadow-user-drag",
        2,
        r#""error: TypeError: EL(...).shadowRoot.getSelection is not a function""#,
        "getComposedRanges and ShadowRoot.getSelection are absent (backlog)",
    ),
];

const MISSING_MEMBERS: &str = "getComposedRanges and modify are absent (ADR 0039 section 5; \
     backlog); deleteFromDocument comes with Range.deleteContents (9b-3)";

/// The queries that a later sub-feature covers or that swb leaves out: (case,
/// query index, why). The queries still run.
const SKIPPED: &[(&str, usize, &str)] = &[(
    "collapse-extend",
    5,
    "deleteFromDocument needs Range.deleteContents, which comes with 9b-3",
)];

/// A directory with test pages, removed at the end of the test.
struct Site(PathBuf);

impl Site {
    fn new(name: &str) -> Site {
        let dir = std::env::temp_dir().join(format!("swb-selection-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Site(dir)
    }

    fn page(&self, name: &str, html: &str) -> Url {
        let path = self.0.join(name);
        std::fs::write(&path, html).unwrap();
        Url::from_file_path(path).unwrap()
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A page with JavaScript on that shows `html` (400x300 as the probe, test
/// fonts), loaded.
fn page_of(site: &Site, html: &str) -> Page {
    let config = PageConfig {
        fetcher: Arc::new(NetworkFetcher::new()),
        notify: Arc::new(|| {}),
        network_threads: 2,
        scripts_allowed: true,
    };
    let mut page = Page::new(
        config,
        FontContext::for_tests(),
        Size::new(400.0, 300.0),
        1.0,
    );
    page.set_javascript(true);
    page.navigate(site.page("page.html", html));
    assert!(page.wait_until_loaded(Duration::from_secs(30)));
    page.run_event_loop(Duration::from_millis(50), Duration::from_secs(5));
    page
}

/// The value of an evaluation as JSON, or `error: ...`.
fn value(page: &mut Page, source: &str) -> Value {
    match page.evaluate(source, JsonForm::Strict) {
        Some(Evaluation::Value(text)) => serde_json::from_str(&text).unwrap(),
        Some(Evaluation::Error(text)) => Value::String(format!("error: {text}")),
        None => Value::String("no runtime".to_owned()),
    }
}

fn probe() -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("tools/probes/web-selection.json")).unwrap(),
    )
    .unwrap()
}

fn chromium() -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/web-selection.json"),
        )
        .unwrap(),
    )
    .unwrap()
}

/// The distinct events of an event log (`FLUSH()`), sorted. The logs of the
/// cases with input steps are compared as sets: Chromium handles input
/// before the task of `selectionchange`, so the changes of a press and the
/// moves or keys that follow it make one event, while `run_steps` runs the
/// tasks after each step (one event per step). Which events come is the
/// same.
fn event_set(log: &Value) -> Vec<String> {
    let mut events: Vec<String> = log
        .as_str()
        .unwrap_or_default()
        .split(' ')
        .filter(|e| !e.is_empty())
        .map(str::to_owned)
        .collect();
    events.sort();
    events.dedup();
    events
}

/// Runs the cases of the probe file against Chromium's values.
#[test]
fn probe_cases() {
    on_page_thread(|| {
        let probe = probe();
        let expected = chromium();
        let mut failures = Vec::new();
        for case in probe["cases"].as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let Some(want) = expected.get(name) else {
                // A case with pixel queries: `crates/engine/tests/selection_paint.rs`.
                continue;
            };
            let site = Site::new(name);
            let mut page = page_of(&site, case["html"].as_str().unwrap());
            if let Some(steps) = case["steps"].as_array()
                && let Err(error) = run_steps(&mut page, steps)
            {
                failures.push(format!("{name}: {error}"));
                continue;
            }
            for (index, query) in case["measure"].as_array().unwrap().iter().enumerate() {
                let source = query["js"].as_str().unwrap();
                let got = value(&mut page, source);
                if SKIPPED.iter().any(|s| s.0 == name && s.1 == index) {
                    continue;
                }
                let want = DEPARTURES
                    .iter()
                    .find(|d| d.0 == name && d.1 == index)
                    .map_or_else(
                        || want[index].clone(),
                        |d| serde_json::from_str(d.2).unwrap(),
                    );
                let same = if case.get("steps").is_some() && source.contains("FLUSH()") {
                    event_set(&got) == event_set(&want)
                } else {
                    got == want
                };
                if !same {
                    failures.push(format!(
                        "{name} #{index}: {source}\n  expected: {want}\n  actual:   {got}"
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
}
