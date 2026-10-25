//! The event interfaces of feature 6d1 with the runtime of a document: the
//! cases of the Chromium probe `tools/probes/web-input-events.json`
//! (constructors and init dictionaries of `UIEvent`, `MouseEvent`,
//! `PointerEvent`, `WheelEvent`, `KeyboardEvent`, `FocusEvent`,
//! `InputEvent`, `CompositionEvent`, `TextEvent`, `SubmitEvent` and the
//! other events of `createEvent`, and `createEvent` itself), each with and
//! without the GC stress mode. The tests below check what a probe cannot
//! see.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "input_events/cases.rs"]
mod cases;

mod common;

use common::{Departure, answer, expected, load, on_big_stack, probe_failures};

/// The values where swb departs from Chromium on purpose: (case, query
/// index, swb's value, why).
const SWB_VALUES: &[Departure] = &[
    (
        "mouse-event",
        20,
        r#""clientX=5 pageX=5 offsetX=5 layerX=5 x=5""#,
        "offsetX and layerX stay the page coordinates after the dispatch: a script task has no layout to make them relative to the target",
    ),
    (
        "mouse-event",
        21,
        r#""clientX=50 clientY=60 pageX=50 pageY=60 offsetX=50 offsetY=60 layerX=50 layerY=60 x=50 y=60 fromElement=null toElement=[object HTMLDivElement]""#,
        "offsetX and layerX are not relative to the target (see query 20)",
    ),
    (
        "mouse-event",
        22,
        r#""clientX=50 clientY=60 pageX=50 pageY=60 offsetX=50 offsetY=60 layerX=50 layerY=60""#,
        "offsetX and layerX are not relative to the target (see query 20)",
    ),
    (
        "pointer-wheel",
        6,
        r#""1,PointerEvent,[object PointerEvent] [object MouseEvent] [object UIEvent] [object Event] [object Object],pointerId,width,height,pressure,tiltX,tiltY,azimuthAngle,altitudeAngle,tangentialPressure,twist,pointerType,isPrimary,persistentDeviceId,getCoalescedEvents,getPredictedEvents,constructor""#,
        "the generator defines the attributes, then the operations, then the constructor; Chromium mixes them in the order of the IDL",
    ),
    (
        "other-events",
        14,
        r#""TypeError: Failed to construct 'MessageEvent': Failed to read the 'ports' property from 'MessageEventInit': Failed to convert value to 'object'.""#,
        "swb has no MessagePort: `ports` holds objects",
    ),
    (
        "other-events",
        15,
        "false",
        "`ports` is a new array at each read, not a frozen array",
    ),
    (
        "other-events",
        17,
        r#""data,origin,lastEventId,source,ports,initMessageEvent,constructor""#,
        "MessageEvent has no `userActivation` (no UserActivation interface)",
    ),
];

/// Runs every probe case with and without the GC stress mode.
#[test]
fn probe_cases() {
    let failures = probe_failures(cases::CASES, SWB_VALUES);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An event object keeps its objects alive across a collection (the
/// `view`, the related target, the submitter, the coalesced events).
#[test]
fn event_objects_survive_collections() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = load(
                "<!DOCTYPE html><body><p id=p>x</p><button id=b>b</button>",
                stress,
            );
            let set = "{ window.e = new PointerEvent('x', { relatedTarget: p, view: window, coalescedEvents: [new PointerEvent('y', { pointerId: 7 })] }); window.s = new SubmitEvent('submit', { submitter: b }); window.m = new MessageEvent('message', { data: { a: [1, 2] }, source: window }); 1 }";
            assert_eq!(answer(&mut rt, set), expected("1"));
            rt.collect();
            let check = "[e.relatedTarget === p, e.view === window, e.getCoalescedEvents()[0].pointerId, s.submitter === b, m.data.a.length, m.source === window].join()";
            assert_eq!(
                answer(&mut rt, check),
                expected(r#""true,true,7,true,2,true""#),
                "stress {stress}"
            );
        }
    });
}

/// A loop that makes 50,000 mouse events and dispatches them is linear.
#[test]
fn events_in_a_loop() {
    on_big_stack(|| {
        let mut rt = load("<!DOCTYPE html><body><div id=d></div>", false);
        let source = "{ let n = 0; d.addEventListener('mousemove', (e) => { n += e.clientX; }); for (let i = 0; i < 50000; i++) d.dispatchEvent(new MouseEvent('mousemove', { clientX: 1, bubbles: true })); n }";
        assert_eq!(answer(&mut rt, source), expected("50000"));
    });
}
