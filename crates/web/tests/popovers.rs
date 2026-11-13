//! Popovers for scripts (M10 sub-feature 6a, ADR 0037 sections 2, 3 and 9),
//! with and without the GC stress mode: `ToggleEvent` and its dictionary,
//! the options of `showPopover` and `togglePopover`, the events of a show
//! and a hide with their `toggle` task, `popoverTargetElement` with a
//! target whose wrapper was collected, and the `source` of an invoker in a
//! shadow tree. The cases that Chromium measured are in
//! `tools/probes/web-top-layer.json` and run in
//! `crates/engine/tests/probe_net.rs`; these tests add the collector and the
//! retargeting.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

mod common;

use common::{TestDocument, answer, expected, load_in, on_big_stack};
use swb_web::ClockMode;

const PAGE: &str = "<!DOCTYPE html><html><body>\
    <div id=pa popover>a</div><div id=pm popover=manual>m</div>\
    <button id=b popovertarget=pa>b</button><div id=h></div>\
    <script>\
    function k(e) { if (e == null) return String(e); if (e.nodeType === 9) return 'doc'; \
      return e.localName + (e.id ? '#' + e.id : ''); }\
    var L = [];\
    function on(t) { ['beforetoggle', 'toggle'].forEach(function (type) { \
      t.addEventListener(type, function (e) { L.push(type + ':' + k(e.target) + '(' + \
        e.oldState + '>' + e.newState + ')' + (e.cancelable ? '!' : '') + ':' + k(e.source)); \
      }, true); }); }\
    on(document);\
    function take() { var a = L.join(' '); L.length = 0; return a; }\
    </script></body></html>";

/// Runs `steps` (expressions, in order, with the due tasks run after each)
/// with and without the GC stress mode, and compares the value of the last
/// one with `want`.
fn check(steps: &[&str], want: &str) {
    let steps: Vec<String> = steps.iter().map(|&s| s.to_owned()).collect();
    let want = want.to_owned();
    on_big_stack(move || {
        for stress in [false, true] {
            let mut rt = load_in(PAGE, stress, ClockMode::Virtual);
            let mut last = serde_json::Value::Null;
            for step in &steps {
                last = answer(&mut rt, step);
                rt.run_due_tasks(std::time::Duration::from_secs(1));
            }
            assert_eq!(last, expected(&format!("{want:?}")), "stress {stress}");
        }
    });
}

#[test]
fn toggle_event_constructor_and_dictionary() {
    check(
        &[
            "(function () { var e = new ToggleEvent('toggle', {oldState: 'closed', \
           newState: 'open', source: document.body, bubbles: true}); \
           var f = new ToggleEvent('x'); \
           return [e.oldState, e.newState, e.bubbles, e.cancelable, k(e.source), \
           JSON.stringify(f.oldState), String(f.source), \
           e instanceof Event, Object.prototype.toString.call(e)].join(); })()",
        ],
        "closed,open,true,false,body,\"\",null,true,[object ToggleEvent]",
    );
}

/// The event objects of a show and a hide keep their fields after the
/// collector ran: a listener keeps them, a later task reads them.
#[test]
fn events_survive_collections() {
    check(
        &[
            "var kept = []; pa.addEventListener('beforetoggle', function (e) { kept.push(e); }); \
             pa.addEventListener('toggle', function (e) { kept.push(e); }); \
             pa.showPopover(); pa.hidePopover(); pa.showPopover(); take()",
            "kept.map(function (e) { return e.type + ':' + e.oldState + '>' + e.newState + ':' \
             + e.isTrusted; }).join(' ') + ' | ' + take()",
        ],
        "beforetoggle:closed>open:true beforetoggle:open>closed:true \
         beforetoggle:closed>open:true toggle:closed>open:true | \
         toggle:div#pa(closed>open):null",
    );
}

#[test]
fn options_of_show_and_toggle() {
    check(
        &["[pa.togglePopover({force: true}), pa.togglePopover({}), \
             pa.togglePopover(true), pa.togglePopover(false), pa.togglePopover(null)].join() \
             + ' ' + take()"],
        "true,false,true,false,true beforetoggle:div#pa(closed>open)!:null \
         beforetoggle:div#pa(open>closed):null beforetoggle:div#pa(closed>open)!:null \
         beforetoggle:div#pa(open>closed):null beforetoggle:div#pa(closed>open)!:null",
    );
    check(
        &["pm.showPopover({source: b}); take()", "take()"],
        "toggle:div#pm(closed>open):button#b",
    );
}

/// A target that a script set stays the target after its wrapper was
/// collected (the invoker keeps the node, not the wrapper); a removed
/// target is no target.
#[test]
fn popover_target_element_after_a_collection() {
    check(
        &[
            "(function () { var d = document.createElement('div'); d.id = 'made'; \
             d.popover = 'auto'; document.body.appendChild(d); b.popoverTargetElement = d; \
             })(); 'set'",
            "[k(b.popoverTargetElement), JSON.stringify(b.getAttribute('popovertarget'))].join()",
        ],
        "div#made,\"\"",
    );
    check(
        &[
            "(function () { var d = document.createElement('div'); d.popover = 'manual'; \
             document.body.appendChild(d); b.popoverTargetElement = d; })(); 'set'",
            "b.click(); document.querySelectorAll(':popover-open').length",
            "b.popoverTargetElement.remove(); [String(b.popoverTargetElement), \
             document.querySelectorAll(':popover-open').length].join()",
        ],
        "null,0",
    );
}

/// The `source` of an invoker in a shadow tree is retargeted against each
/// listener's node: the document sees the host.
#[test]
fn the_source_is_retargeted() {
    check(
        &[
            "var r = h.attachShadow({mode: 'open'}); \
             r.innerHTML = '<button id=sb>s</button><div id=sp popover=manual>p</div>' \
             + '<button id=sb2 popovertarget=sp>t</button>'; \
             var sb = r.firstChild; var seen = []; \
             sb.popoverTargetElement = pm; \
             r.addEventListener('beforetoggle', function (e) { seen.push(k(e.source)); }, true); \
             sb.click(); r.getElementById('sb2').click(); take()",
            "seen.join() + ' | ' + take()",
        ],
        "button#sb2 | toggle:div#pm(closed>open):div#h",
    );
}

/// A target that a script set is no target when it is not a descendant of
/// one of the invoker's shadow-including ancestors: inside a detached
/// invoker, the invoker itself does not count (HTML "attr-associated
/// element").
#[test]
fn a_target_inside_a_detached_invoker_is_no_target() {
    check(
        &["(function () { var x = document.createElement('button'); \
           var d = document.createElement('div'); x.appendChild(d); \
           x.popoverTargetElement = d; var a = String(x.popoverTargetElement); \
           var w = document.createElement('div'); w.appendChild(x); \
           return a + ',' + k(x.popoverTargetElement); })()"],
        "null,div",
    );
}

/// The `source` of a `ToggleEvent` lives as long as the event: the element
/// that only the event holds survives the collections.
#[test]
fn the_source_lives_with_the_event() {
    check(
        &[
            "var ev = (function () { var s = document.createElement('i'); s.id = 'only'; \
             return new ToggleEvent('t', {source: s}); })(); \
             var junk = []; for (var i = 0; i < 20000; i++) junk.push({i: i}); junk = null; 'made'",
            "k(ev.source)",
        ],
        "i#only",
    );
}

/// A runtime with `PAGE` and a second document, which the first knows as
/// `other` (another arena: nodes that move there get new ids).
fn two_documents(stress: bool) -> swb_web::ScriptRuntime {
    let mut rt = load_in(PAGE, stress, ClockMode::Virtual);
    let second = rt.add_document(1).unwrap();
    rt.lend_document(
        second,
        Box::new(TestDocument::parsing(
            "<!DOCTYPE html><html><body id=body-b></body></html>",
        )),
    );
    rt.with_document(second, |rt| {
        rt.start_document();
        rt.fire_load_event();
        rt.run_due_tasks(std::time::Duration::from_secs(5));
    })
    .unwrap();
    rt.expose_window_for_tests(swb_web::DocumentKey::FIRST, "other", second)
        .unwrap();
    rt
}

/// Runs `body` after the setup of a button with a script's target in a box,
/// in two documents, with and without the GC stress mode: `r.join()` is the
/// value.
fn check_two(body: &str, want: &str) {
    let source = format!(
        "(function () {{ var box = document.createElement('div'); \
         var bt = document.createElement('button'); bt.type = 'button'; \
         var dd = document.createElement('div'); dd.id = 'moved'; dd.popover = 'manual'; \
         box.append(bt, dd); document.body.appendChild(box); bt.popoverTargetElement = dd; \
         var r = []; {body} return r.join(); }})()"
    );
    let want = want.to_owned();
    on_big_stack(move || {
        for stress in [false, true] {
            let mut rt = two_documents(stress);
            assert_eq!(
                answer(&mut rt, &source),
                expected(&format!("{want:?}")),
                "stress {stress}"
            );
        }
    });
}

/// 6a fix round, review finding 1: a script's `popoverTargetElement` follows
/// the invoker and its target when both move to the arena of another
/// document, also twice and back; with one end left behind the getter gives
/// `null` (Chromium: `popover-target-arenas`). Nodes made afterwards do not
/// pick up an old entry.
#[test]
fn popover_target_element_follows_a_move_to_another_arena() {
    check_two(
        "var d = other.document; d.body.appendChild(box); r.push(bt.popoverTargetElement === dd); \
         document.body.appendChild(box); r.push(bt.popoverTargetElement === dd); \
         d.body.appendChild(box); d.body.removeChild(box); d.body.appendChild(box); \
         r.push(bt.popoverTargetElement === dd); \
         for (var i = 0; i < 6; i++) { var n = d.createElement('button'); d.body.appendChild(n); \
           var m = document.createElement('button'); document.body.appendChild(m); \
           if (n.popoverTargetElement !== null || m.popoverTargetElement !== null) r.push('stale' + i); } \
         document.body.appendChild(box); r.push(bt.popoverTargetElement === dd);",
        "true,true,true,true",
    );
    // One end moves.
    check_two(
        "other.document.body.appendChild(bt); r.push(String(bt.popoverTargetElement));",
        "null",
    );
    check_two(
        "other.document.body.appendChild(dd); r.push(String(bt.popoverTargetElement));",
        "null",
    );
}

/// Review finding 4: an element of another document is accepted by
/// `popoverTargetElement` and by the `source` option of `showPopover` and
/// `togglePopover` (Chromium: `popover-target-arenas`). The getter gives
/// `null` and the attribute is the empty string; the `beforetoggle` event
/// has the element as its `source`, the `toggle` event has none (a
/// departure).
#[test]
fn elements_of_another_document_are_accepted() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = two_documents(stress);
            let setup = "var x = other.document.createElement('p'); x.id = 'other'; \
                other.document.body.appendChild(x);";
            answer(&mut rt, &format!("{setup} 'ok'"));
            assert_eq!(
                answer(
                    &mut rt,
                    "(function () { b.popoverTargetElement = x; \
                     return [String(b.popoverTargetElement), \
                     JSON.stringify(b.getAttribute('popovertarget'))].join(); })()"
                ),
                expected(r#""null,\"\"""#),
                "stress {stress}"
            );
            assert_eq!(
                answer(
                    &mut rt,
                    "b.click(); [document.querySelectorAll(':popover-open').length, take()].join()"
                ),
                expected(r#""0,""#),
                "stress {stress}"
            );
            assert_eq!(
                answer(&mut rt, "pm.showPopover({source: x}); take()"),
                expected(r#""beforetoggle:div#pm(closed>open)!:p#other""#),
                "stress {stress}"
            );
            rt.run_due_tasks(std::time::Duration::from_secs(1));
            assert_eq!(
                answer(&mut rt, "pm.hidePopover(); take()"),
                expected(
                    r#""toggle:div#pm(closed>open):null beforetoggle:div#pm(open>closed):null""#
                ),
                "stress {stress}"
            );
            rt.run_due_tasks(std::time::Duration::from_secs(1));
            assert_eq!(
                answer(
                    &mut rt,
                    "pm.togglePopover({source: x, force: true}); take()"
                ),
                expected(
                    r#""toggle:div#pm(open>closed):null beforetoggle:div#pm(closed>open)!:p#other""#
                ),
                "stress {stress}"
            );
        }
    });
}
