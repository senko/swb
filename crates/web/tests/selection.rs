//! `Selection`, `getSelection` and the document's `selectionchange` with the
//! runtime of one document (M10 sub-feature 9b-2, ADR 0039): the queries of the
//! Chromium probe `tools/probes/web-selection.json` that need no layout, no
//! frames and no input (the objects and their descriptors, `addRange` and
//! `removeRange`, `collapse`, `extend`, `setBaseAndExtent`, `selectAllChildren`,
//! the errors, the live updates of the selection and the timing and coalescing
//! of `selectionchange`), each with and without the GC stress mode; the others
//! run in the engine (`crates/automation/tests/selection.rs`). And the checks
//! that a probe cannot make: the collector and loops.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "selection/cases.rs"]
mod cases;

mod common;

use std::time::{Duration, Instant};

use common::{Case, Departure, expected, load, on_big_stack};
use swb_web::{Evaluation, EvaluationStart, JsonForm, ScriptRuntime};

/// The values where swb departs from Chromium on purpose: (case, query index,
/// swb's value, why).
const SWB_VALUES: &[Departure] = &[
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
];

const MISSING_MEMBERS: &str = "getComposedRanges and modify are absent (ADR 0039 section 5; \
     backlog); deleteFromDocument comes with Range.deleteContents (9b-3)";

/// The cases and queries that run elsewhere or later: (case, query index or
/// `None` for all, why). The queries still run, so that none of them panics.
const ELSEWHERE: &[(&str, Option<usize>, &str)] = &[
    ("api", Some(4), ENGINE_FRAMES),
    ("add-remove", Some(1), ENGINE_FRAMES),
    ("collapse-extend", Some(5), DELETE_FROM_DOCUMENT),
    ("to-string", None, ENGINE_TEXT),
    ("events-timing", Some(4), ENGINE_CONTROLS),
    ("events-timing", Some(5), ENGINE_CONTROLS),
    ("user-click", None, ENGINE_INPUT),
    ("user-drag", None, ENGINE_INPUT),
    ("user-drag-backward", None, ENGINE_INPUT),
    ("user-dblclick", None, ENGINE_INPUT),
    ("user-click-then-dblclick", None, ENGINE_INPUT),
    ("user-shift-arrows", None, ENGINE_INPUT),
    ("user-shift-click", None, ENGINE_INPUT),
    ("user-select-all", None, ENGINE_INPUT),
    ("user-click-empty", None, ENGINE_INPUT),
    ("user-click-input", None, ENGINE_INPUT),
    ("user-drag-script-change", None, ENGINE_INPUT),
    ("user-selectstart-cancel", None, ENGINE_INPUT),
    ("shadow-user-drag", None, ENGINE_INPUT),
    ("frames", None, ENGINE_FRAMES),
    ("frames-user-click", None, ENGINE_INPUT),
    ("contains-node", None, ENGINE_TEXT),
    ("control-script-selection", None, ENGINE_CONTROLS),
    ("frames-user-click-string", None, ENGINE_INPUT),
    ("frames-user-select-all-click", None, ENGINE_INPUT),
    ("frames-user-click-contains", None, ENGINE_INPUT),
    ("frames-user-click-partial", None, ENGINE_INPUT),
    ("frames-user-drag-click", None, ENGINE_INPUT),
];

const ENGINE_FRAMES: &str = "needs a frame: crates/automation/tests/selection.rs runs it";
const ENGINE_TEXT: &str =
    "needs the rendering (rendered text, block boxes): crates/automation/tests/selection.rs";
const ENGINE_CONTROLS: &str =
    "needs the state of text controls: crates/automation/tests/selection.rs runs it";
const ENGINE_INPUT: &str = "the case has input steps: crates/automation/tests/selection.rs";
const DELETE_FROM_DOCUMENT: &str =
    "deleteFromDocument needs Range.deleteContents, which comes with 9b-3";

/// The time that an evaluation may wait for its promise.
const SETTLE: Duration = Duration::from_secs(10);

fn elsewhere(case: &str, index: usize) -> bool {
    ELSEWHERE
        .iter()
        .any(|l| l.0 == case && l.1.is_none_or(|i| i == index))
}

/// The value of an evaluation as the probe prints it, after its promise
/// settled (the tasks run in between).
fn answer(rt: &mut ScriptRuntime, source: &str) -> serde_json::Value {
    let evaluation = match rt.start_evaluation(source, JsonForm::Strict) {
        EvaluationStart::Done(evaluation) => evaluation,
        EvaluationStart::Pending(id) => {
            let deadline = Instant::now() + SETTLE;
            loop {
                rt.run_due_tasks(Duration::from_secs(1));
                if let Some(evaluation) = rt.poll_evaluation(id, JsonForm::Strict) {
                    break evaluation;
                }
                assert!(Instant::now() < deadline, "{source}: no answer");
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    };
    match evaluation {
        Evaluation::Value(text) => serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{source}: invalid JSON {text}: {e}")),
        Evaluation::Error(text) => serde_json::Value::String(format!("error: {text}")),
    }
}

/// The differences of one case from Chromium's values, in a new runtime.
fn check(case: &Case, stress: bool) -> Vec<String> {
    let mut failures = Vec::new();
    let mut rt = load(case.html, stress);
    for (index, &(query, chromium)) in case.queries.iter().enumerate() {
        let want = SWB_VALUES
            .iter()
            .find(|v| v.0 == case.name && v.1 == index)
            .map_or(chromium, |v| v.2);
        let got = answer(&mut rt, query);
        if elsewhere(case.name, index) {
            continue;
        }
        if got != expected(want) {
            failures.push(format!(
                "{} #{index} (stress {stress}): {query}\n  expected: {want}\n  actual:   {got}",
                case.name
            ));
        }
    }
    failures
}

/// Runs every probe case with and without the GC stress mode.
#[test]
fn probe_cases() {
    let failures = on_big_stack(|| {
        let mut failures = Vec::new();
        for case in cases::CASES {
            for stress in [false, true] {
                failures.extend(check(case, stress));
            }
        }
        failures
    });
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
