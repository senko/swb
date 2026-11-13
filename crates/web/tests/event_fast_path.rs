//! The fast path of the events that the engine fires (M10 sub-feature 5d,
//! review fixes; ADR 0036 "5d implementation notes"): an engine event of a
//! type that no listener or handler can see skips its path, and a listener
//! or handler that a script adds later, from any source, is still called.
//! Each case fires `mousemove` (bubbling) at `#t` before the source exists
//! (nothing is called) and after it (the handler runs), with and without the
//! GC stress mode.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

mod common;

use std::time::Duration;

use common::{TestDocument, answer, expected, load, on_big_stack};
use swb_dom::NodeId;
use swb_web::{DispatchTarget, DocumentKey, ScriptRuntime, TrustedEvent};

/// The page of most cases: `#t` inside `#host`, and the log. The page has
/// no `on*` attribute: the arena does not know any handler name.
const PAGE: &str = "<!DOCTYPE html><html><body><div id=host><div id=t>x</div></div>\
    <script>window.log = [];</script>";

/// [`PAGE`] with a second document of the same arena (`DOMParser`), which
/// has the handler on `#t`.
const PAGE_D2: &str = "<!DOCTYPE html><html><body><div id=host><div id=t>x</div></div>\
    <script>window.log = [];\
    window.d2 = new DOMParser().parseFromString(\
      '<p id=t onmousemove=\"log.push(\\'foreign\\')\">p</p>', 'text/html');</script>";

/// The node with the ID `id` in the document.
fn find(rt: &mut ScriptRuntime, id: &str) -> Option<NodeId> {
    let host = rt.take_back().unwrap();
    let node = host.dom().element_by_id(id);
    rt.lend(host);
    node
}

/// Fires `mousemove` at the node `#t`, bubbling.
fn fire(rt: &mut ScriptRuntime) {
    fire_at(rt, "t");
}

/// Fires `mousemove` at the node `#id`, bubbling.
fn fire_at(rt: &mut ScriptRuntime, id: &str) {
    let node = find(rt, id).expect("the target exists");
    let mut event = TrustedEvent::new("mousemove");
    event.bubbles = true;
    event.composed = true;
    assert_eq!(
        rt.dispatch_event(DispatchTarget::Node(node), &event),
        Ok(true)
    );
}

/// One case: the page, the script that adds the handler, and the log
/// after the second event. An empty script means that the page has the
/// handler from the start (no first event).
fn case(page: &'static str, script: &'static str, want: &'static str) {
    on_big_stack(move || {
        for stress in [false, true] {
            let mut rt = load(page, stress);
            if !script.is_empty() && find(&mut rt, "t").is_some() {
                fire(&mut rt);
                assert_eq!(
                    answer(&mut rt, "log.join()"),
                    expected(r#""""#),
                    "{script}: before, stress {stress}"
                );
            }
            let value = answer(&mut rt, if script.is_empty() { "1" } else { script });
            assert!(
                !value.as_str().is_some_and(|s| s.starts_with("error")),
                "{script}: {value}"
            );
            fire(&mut rt);
            assert_eq!(
                answer(&mut rt, "log.join()"),
                expected(want),
                "{script}: after, stress {stress}"
            );
        }
    });
}

#[test]
fn a_listener_on_a_node_is_called() {
    case(
        PAGE,
        "t.addEventListener('mousemove', () => log.push('node'))",
        r#""node""#,
    );
}

#[test]
fn a_listener_on_an_ancestor_is_called() {
    case(
        PAGE,
        "host.addEventListener('mousemove', () => log.push('host'), true)",
        r#""host""#,
    );
}

#[test]
fn a_listener_on_the_document_is_called() {
    case(
        PAGE,
        "document.addEventListener('mousemove', () => log.push('doc'))",
        r#""doc""#,
    );
}

#[test]
fn a_listener_on_the_window_is_called() {
    case(
        PAGE,
        "addEventListener('mousemove', () => log.push('win'))",
        r#""win""#,
    );
}

#[test]
fn an_idl_handler_of_a_node_is_called() {
    case(PAGE, "t.onmousemove = () => log.push('idl')", r#""idl""#);
}

#[test]
fn an_idl_handler_of_the_window_is_called() {
    case(
        PAGE,
        "window.onmousemove = () => log.push('widl')",
        r#""widl""#,
    );
}

#[test]
fn a_content_attribute_from_set_attribute_is_called() {
    case(
        PAGE,
        "host.setAttribute('onmousemove', \"log.push('sa')\")",
        r#""sa""#,
    );
}

#[test]
fn a_content_attribute_from_set_attribute_ns_is_called() {
    case(
        PAGE,
        "host.setAttributeNS(null, 'onmousemove', \"log.push('ns')\")",
        r#""ns""#,
    );
}

#[test]
fn a_content_attribute_from_toggle_attribute_and_attr_value_is_called() {
    case(
        PAGE,
        "host.toggleAttribute('onmousemove'); \
         host.getAttributeNode('onmousemove').value = \"log.push('tv')\"",
        r#""tv""#,
    );
}

#[test]
fn a_content_attribute_from_set_attribute_node_is_called() {
    case(
        PAGE,
        "const a = document.createAttribute('onmousemove'); a.value = \"log.push('an')\"; \
         host.setAttributeNode(a)",
        r#""an""#,
    );
}

#[test]
fn a_content_attribute_from_the_parser_is_called() {
    case(
        "<!DOCTYPE html><body><div id=t onmousemove=\"log.push('parsed')\">x</div>\
         <script>window.log = []</script>",
        "",
        r#""parsed""#,
    );
}

#[test]
fn a_content_attribute_of_a_second_body_tag_is_called() {
    case(
        "<!DOCTYPE html><body><body id=t onmousemove=\"log.push('body2')\">x\
         <script>window.log = []</script>",
        "",
        r#""body2""#,
    );
}

#[test]
fn a_content_attribute_from_inner_html_is_called() {
    case(
        PAGE,
        "host.innerHTML = '<i id=t onmousemove=\"log.push(\\'ih\\')\">x</i>'",
        r#""ih""#,
    );
}

#[test]
fn a_content_attribute_from_insert_adjacent_html_is_called() {
    case(
        PAGE,
        "t.remove(); host.insertAdjacentHTML('beforeend', \
         '<i id=t onmousemove=\"log.push(\\'iah\\')\">x</i>')",
        r#""iah""#,
    );
}

#[test]
fn a_content_attribute_from_document_write_is_called() {
    case(
        "<!DOCTYPE html><body><script>window.log = [];\
         document.write('<b id=t onmousemove=\"log.push(\\'dw\\')\">x</b>')</script>",
        "",
        r#""dw""#,
    );
}

#[test]
fn a_content_attribute_of_a_node_moved_in_by_adopt_node_is_called() {
    case(
        PAGE_D2,
        "t.remove(); host.appendChild(document.adoptNode(d2.getElementById('t')))",
        r#""foreign""#,
    );
}

#[test]
fn a_content_attribute_of_a_node_moved_in_by_import_node_is_called() {
    case(
        PAGE_D2,
        "t.remove(); host.appendChild(document.importNode(d2.getElementById('t'), true))",
        r#""foreign""#,
    );
}

#[test]
fn a_content_attribute_of_a_node_cloned_in_the_other_document_is_called() {
    case(
        PAGE_D2,
        "t.remove(); host.appendChild(d2.getElementById('t').cloneNode(true))",
        r#""foreign""#,
    );
}

/// A node that moves in from the arena of another document of the runtime
/// (a frame): its handler attribute is new to this arena. Each way of
/// getting it here is a case; the page starts with no `on*` attribute.
#[test]
fn a_content_attribute_of_a_node_from_another_arena_is_called() {
    const SECOND: &str = "<!DOCTYPE html><html><body>\
        <p id=u onmousemove=\"top.log.push('frame')\">p</p></body></html>";
    let moves = [
        "host.appendChild(document.adoptNode(other.document.getElementById('u')))",
        "host.appendChild(document.importNode(other.document.getElementById('u'), true))",
        "host.appendChild(other.document.getElementById('u').cloneNode(true))",
        "host.appendChild(other.document.getElementById('u'))",
    ];
    for script in moves {
        on_big_stack(move || {
            for stress in [false, true] {
                let mut rt = two_documents(stress, SECOND);
                fire(&mut rt);
                assert_eq!(answer(&mut rt, "log.join()"), expected(r#""""#));
                assert_eq!(answer(&mut rt, &format!("{script}; 1")), expected("1"));
                fire_at(&mut rt, "u");
                assert_eq!(
                    answer(&mut rt, "log.join()"),
                    expected(r#""frame""#),
                    "{script}, stress {stress}"
                );
            }
        });
    }
}

/// A runtime with [`PAGE`] and a second document with `second`, which the
/// first knows as the global `other`.
fn two_documents(stress: bool, second: &str) -> ScriptRuntime {
    let mut rt = load(PAGE, stress);
    let key = rt.add_document(1).unwrap();
    rt.lend_document(key, Box::new(TestDocument::parsing(second)));
    rt.with_document(key, |rt| {
        rt.start_document();
        rt.fire_load_event();
        rt.run_due_tasks(Duration::from_secs(5));
    })
    .unwrap();
    rt.expose_window_for_tests(DocumentKey::FIRST, "other", key)
        .unwrap();
    rt
}

/// A `slotchange` with no listener costs no path; a listener that is added
/// before the next change is called.
#[test]
fn a_late_slotchange_listener_is_called() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = load(
                "<!DOCTYPE html><body><div id=host></div><script>window.log = [];\
                 var r = host.attachShadow({mode: 'open'}); r.innerHTML = '<slot></slot>';\
                 host.appendChild(document.createElement('b'));</script>",
                stress,
            );
            let setup = "r.addEventListener('slotchange', () => log.push('sc')); \
                host.appendChild(document.createElement('i')); 1";
            assert_eq!(answer(&mut rt, "log.join()"), expected(r#""""#));
            assert_eq!(answer(&mut rt, setup), expected("1"));
            assert_eq!(
                answer(&mut rt, "log.join()"),
                expected(r#""sc""#),
                "stress {stress}"
            );
        }
    });
}

/// The skipped event has the result of an event without a listener.
#[test]
fn a_skipped_event_gives_the_result_of_an_unheard_event() {
    on_big_stack(|| {
        let mut rt = load(PAGE, false);
        let node = find(&mut rt, "t").unwrap();
        let mut event = TrustedEvent::new("wheel");
        event.bubbles = true;
        event.cancelable = true;
        assert_eq!(
            rt.dispatch_event(DispatchTarget::Node(node), &event),
            Ok(true)
        );
        // A listener that cancels the event cancels it (a click is never
        // skipped).
        answer(
            &mut rt,
            "t.addEventListener('wheel', (e) => e.preventDefault())",
        );
        assert_eq!(
            rt.dispatch_event(DispatchTarget::Node(node), &event),
            Ok(false)
        );
    });
}
