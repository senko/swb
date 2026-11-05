//! `MessageChannel` and `MessagePort` in one document (M9 feature 9, ADR 0031
//! section 7): the cases of the Chromium probe
//! `tools/probes/web-message-channel.json` (constructors and prototypes, the
//! queue and `start()`, the order of port messages against microtasks, timers
//! and window messages, the missing timer clamp, `close()`, the transfer of
//! ports in `port.postMessage()`, `window.postMessage()` and
//! `structuredClone()`, the errors, `MessageEvent.ports`), each with and
//! without the GC stress mode, in virtual time; and the checks that the
//! probe cannot make: the registry follows the collector, the limits, the
//! flood and the ping-pong.

#![allow(clippy::unwrap_used, reason = "the setup of test runtimes")]

#[path = "message_channel/cases.rs"]
mod cases;
#[path = "message_channel/limit_cases.rs"]
mod limit_cases;

mod common;

use std::time::{Duration, Instant};

use common::{Departure, answer, expected, load_in, on_big_stack, probe_failures_in};
use swb_web::{ClockMode, ScriptRuntime};

/// The values where swb departs from Chromium on purpose: (case, query
/// index, swb's value, why).
const SWB_VALUES: &[Departure] = &[
    (
        "api",
        7,
        r#""undefined""#,
        "swb has no BroadcastChannel (ADR 0031 section 13); the query asks for this one value, so the other answers stay compared",
    ),
    (
        "order",
        14,
        r#"["qm", "p1", "p2", "m"]"#,
        "the test runtime runs no rendering update, so no animation frame callback runs (the hostile case and the automation test cover rAF during a ping-pong)",
    ),
    (
        "clamp",
        3,
        r"[true, true, false]",
        "the test runtime runs no rendering update (see order #14)",
    ),
    (
        "event",
        6,
        r#""data,origin,lastEventId,source,ports,initMessageEvent,constructor""#,
        "MessageEvent has no `userActivation` (no UserActivation interface); the query asks for the property names only, so the other answers stay compared",
    ),
    (
        "from-frames",
        5,
        r#""undefined""#,
        "swb has no BroadcastChannel (ADR 0031 section 13); the query is in the probe file for this value only",
    ),
    (
        "from-frames",
        3,
        r"[1000, 18]",
        "the number of chained timers in 50 ms depends on the clock (Chromium 19 with a real clock; virtual time here)",
    ),
];

/// The values of the `limits` case where swb departs from Chromium, which
/// has no limit on ports or waiting messages (ADR 0031 section 11).
const LIMIT_VALUES: &[Departure] = &[
    (
        "limits",
        3,
        "65536",
        "at most 65,536 messages wait; the later ones are dropped with a warning",
    ),
    (
        "limits",
        5,
        "error: RangeError: Failed to construct 'MessageChannel': Too many message ports (65536).",
        "at most 65,536 ports are live",
    ),
];

/// Runs every probe case with and without the GC stress mode.
#[test]
fn probe_cases() {
    let failures = probe_failures_in(cases::CASES, SWB_VALUES, ClockMode::Virtual);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The `limits` case: many channels that nobody keeps, a channel per call
/// (a `setImmediate` polyfill), a flood to a port without a listener, a
/// message with 10,000 ports, 70,000 live channels. No stress mode: the
/// loops are long.
#[test]
fn limit_cases_run_as_the_probe_asks() {
    let failures = on_big_stack(|| {
        let mut failures = Vec::new();
        for case in limit_cases::CASES {
            let mut rt = load_in(case.html, false, ClockMode::Virtual);
            for (index, &(query, chromium)) in case.queries.iter().enumerate() {
                let want = LIMIT_VALUES
                    .iter()
                    .find(|v| v.0 == case.name && v.1 == index)
                    .map_or(chromium, |v| v.2);
                let got = answer(&mut rt, query);
                if got != expected(want) {
                    failures.push(format!(
                        "{} #{index}: {query}\n  expected: {want}\n  actual:   {got}",
                        case.name
                    ));
                }
                settle(&mut rt);
            }
        }
        failures
    });
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const PAGE: &str = "<!DOCTYPE html><body><p>x</p>";

/// Runs the event loop in virtual time until no task waits.
fn settle(rt: &mut ScriptRuntime) {
    for _ in 0..100_000 {
        let ran = rt.run_due_tasks(Duration::from_secs(5));
        match rt.next_task_time() {
            Some(time) if ran == 0 => rt.clock().advance_to(time),
            Some(_) => {}
            None => break,
        }
    }
}

fn runtime(stress: bool) -> ScriptRuntime {
    load_in(PAGE, stress, ClockMode::Virtual)
}

/// A page that makes a channel per call, without keeping any, ends with a
/// handful of entries: the collector frees the pairs, and the registry
/// follows (ADR 0031 section 7, "Done").
#[test]
fn channels_that_nobody_keeps_leave_no_entries() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let source =
            "(() => { for (let i = 0; i < 200000; i++) new MessageChannel(); return 'ok'; })()";
        assert_eq!(answer(&mut rt, source), expected(r#""ok""#));
        assert!(rt.live_message_ports() < 65_536);
        rt.collect();
        assert_eq!(rt.live_message_ports(), 0);
    });
}

/// A pair with listeners and waiting messages that nothing references is
/// collected with its queues (HTML "Ports and garbage collection"), and the
/// budget of waiting messages is free again.
#[test]
fn an_unreferenced_pair_goes_with_its_queue() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = runtime(stress);
            let make = "(() => { for (let i = 0; i < 50; i++) { const c = new MessageChannel(); c.port1.addEventListener('message', () => {}); c.port2.addEventListener('message', () => {}); for (let k = 0; k < 20; k++) { c.port1.postMessage(k); c.port2.postMessage(k); } } return 'ok'; })()";
            assert_eq!(answer(&mut rt, make), expected(r#""ok""#));
            assert!(rt.live_message_ports() <= 100);
            rt.collect();
            assert_eq!(rt.live_message_ports(), 0, "stress {stress}");
        }
    });
}

/// The messages of a collected queue no longer count against the budget:
/// after the collection 65,536 messages are admitted again.
#[test]
fn a_collected_queue_frees_the_budget() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let fill = "(() => { for (let i = 0; i < 100; i++) { const c = new MessageChannel(); for (let k = 0; k < 600; k++) c.port1.postMessage(k); } return 'ok'; })()";
        assert_eq!(answer(&mut rt, fill), expected(r#""ok""#));
        rt.collect();
        assert_eq!(rt.live_message_ports(), 0);
        let flood = "(() => { const c = new MessageChannel(); for (let k = 0; k < 65536; k++) c.port1.postMessage(k); let n = 0; c.port2.onmessage = () => { n++; }; window.count = () => n; return 1; })()";
        assert_eq!(answer(&mut rt, flood), expected("1"));
        settle(&mut rt);
        assert_eq!(answer(&mut rt, "count()"), expected("65536"));
    });
}

/// Closed ports are not live, and the other end is disentangled.
#[test]
fn close_frees_the_entry_at_once() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let source =
            "(() => { window.keep = new MessageChannel(); keep.port1.close(); return 'ok'; })()";
        assert_eq!(answer(&mut rt, source), expected(r#""ok""#));
        assert_eq!(rt.live_message_ports(), 1);
        assert_eq!(answer(&mut rt, "keep.port2.close(); 1"), expected("1"));
        assert_eq!(rt.live_message_ports(), 0);
    });
}

/// The port that script holds keeps its partner and the listener on it,
/// also across collections (the spec's strong reference between entangled
/// ports).
#[test]
fn a_held_port_keeps_its_partner_with_the_listener() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = runtime(stress);
            let setup = "(() => { window.got = []; window.held = (() => { const c = new MessageChannel(); c.port1.onmessage = (e) => got.push(e.data); return c.port2; })(); return 'ok'; })()";
            assert_eq!(answer(&mut rt, setup), expected(r#""ok""#));
            rt.collect();
            rt.collect();
            assert_eq!(answer(&mut rt, "held.postMessage('x'); 1"), expected("1"));
            settle(&mut rt);
            assert_eq!(
                answer(&mut rt, "got"),
                expected(r#"["x"]"#),
                "stress {stress}"
            );
        }
    });
}

/// A listener port whose partner travels in a message that waits survives a
/// collection (the partner is not in the heap while it travels).
#[test]
fn a_listener_port_survives_while_its_partner_travels() {
    on_big_stack(|| {
        for stress in [false, true] {
            let mut rt = runtime(stress);
            let setup = "(() => { window.got = []; const a = new MessageChannel(); a.port1.onmessage = (e) => got.push(e.data); const t = new MessageChannel(); t.port2.onmessage = (e) => { e.ports[0].postMessage('hello'); }; t.port1.postMessage(1, [a.port2]); return 'ok'; })()";
            assert_eq!(answer(&mut rt, setup), expected(r#""ok""#));
            rt.collect();
            settle(&mut rt);
            assert_eq!(
                answer(&mut rt, "got"),
                expected(r#"["hello"]"#),
                "stress {stress}"
            );
        }
    });
}

/// A message that waits for a port that is then collected takes the ports
/// in it with it: the partner of such a port is disentangled.
#[test]
fn ports_in_a_dropped_message_are_closed() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let setup = "(() => { const a = new MessageChannel(), b = new MessageChannel(); a.port1.postMessage('x', [b.port1]); window.h = b.port2; return 'ok'; })()";
        assert_eq!(answer(&mut rt, setup), expected(r#""ok""#));
        // a.port1, a.port2 and the travelling b.port1 and b.port2 (held).
        assert_eq!(rt.live_message_ports(), 4);
        rt.collect();
        assert_eq!(rt.live_message_ports(), 1);
        assert_eq!(
            answer(&mut rt, "h.postMessage(1); 'no error'"),
            expected(r#""no error""#)
        );
    });
}

/// A message that `window.postMessage()` drops (the target origin does not
/// match) closes the ports in it.
#[test]
fn a_dropped_window_message_closes_its_ports() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let setup = "(() => { window.l = []; const d = new MessageChannel(); d.port2.onmessage = (e) => l.push(e.data); window.d2 = d.port2; postMessage('p', 'http://other.invalid/', [d.port1]); return 'ok'; })()";
        assert_eq!(answer(&mut rt, setup), expected(r#""ok""#));
        settle(&mut rt);
        assert_eq!(rt.live_message_ports(), 1);
        assert_eq!(answer(&mut rt, "d2.postMessage(1); 1"), expected("1"));
        settle(&mut rt);
        assert_eq!(answer(&mut rt, "l"), expected("[]"));
    });
}

/// The window's messages and the ports' messages share the budget of 65,536
/// waiting messages.
#[test]
fn window_and_port_messages_share_the_budget() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let source = "(() => { window.n = 0; window.w = 0; const c = new MessageChannel(); c.port2.onmessage = () => { n++; }; addEventListener('message', () => { w++; }); for (let i = 0; i < 40000; i++) postMessage(i, '*'); for (let i = 0; i < 40000; i++) c.port1.postMessage(i); return 'ok'; })()";
        assert_eq!(answer(&mut rt, source), expected(r#""ok""#));
        settle(&mut rt);
        assert_eq!(
            answer(&mut rt, "[w, n, w + n]"),
            expected("[40000, 25536, 65536]")
        );
    });
}

/// An endless ping-pong between two ports leaves room for timers: the
/// timers keep running and the loop returns (the engine runs a frame
/// between the batches of tasks, which the hostile case checks).
#[test]
fn a_ping_pong_leaves_room_for_timers() {
    on_big_stack(|| {
        let page = "<!DOCTYPE html><script>window.k = 0; window.ticks = 0; const c = new MessageChannel(); c.port1.onmessage = (e) => c.port2.postMessage(0); c.port2.onmessage = (e) => { k++; c.port1.postMessage(0); }; c.port1.postMessage(0); setInterval(() => ticks++, 10); window.t5 = 0; setTimeout(() => { t5 = k; }, 5);</script>";
        let mut rt = load_in(page, false, ClockMode::Real);
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(300) {
            let before = Instant::now();
            rt.run_due_tasks(Duration::from_millis(20));
            assert!(before.elapsed() < Duration::from_secs(1));
        }
        assert_eq!(
            answer(&mut rt, "[k > 1000, ticks >= 10, t5 > 0 && t5 < k]"),
            expected("[true, true, true]")
        );
    });
}

/// The ports of a transfer list that is not delivered are closed, and the
/// port limit counts live ports only: a loop that moves ports through
/// messages does not grow the registry.
#[test]
fn ports_moved_through_messages_do_not_leak() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let setup = "(() => { window.done = 0; const hub = new MessageChannel(); hub.port2.onmessage = (e) => { e.ports[0].close(); done++; }; for (let i = 0; i < 5000; i++) { const c = new MessageChannel(); hub.port1.postMessage(i, [c.port1]); } window.hub = hub; return 'ok'; })()";
        assert_eq!(answer(&mut rt, setup), expected(r#""ok""#));
        settle(&mut rt);
        assert_eq!(answer(&mut rt, "done"), expected("5000"));
        rt.collect();
        assert_eq!(rt.live_message_ports(), 2);
    });
}

/// Ports that travel only in the queues of other travelling ports (case A:
/// a channel sends its own second port through its first; case B: two
/// channels send each other one port) are freed with their queues and
/// their budget (ADR 0031 section 7, "Done"). The variants set `onmessage`
/// on the partner of the travelling port; the root rule of the collector
/// then keeps that partner until the sweep has removed the travelling
/// port, so it goes within two collections, each followed by a sweep
/// (`live_message_ports()` sweeps).
#[test]
fn ports_that_travel_in_each_others_queues_are_freed() {
    const CASES: &[(&str, &str)] = &[
        (
            "A",
            "const c = new MessageChannel(); c.port1.postMessage(1, [c.port2]);",
        ),
        (
            "A with onmessage",
            "const c = new MessageChannel(); c.port1.onmessage = () => {}; c.port1.postMessage(1, [c.port2]);",
        ),
        (
            "B",
            "const a = new MessageChannel(), b = new MessageChannel(); a.port1.postMessage(1, [b.port1]); b.port2.postMessage(1, [a.port2]);",
        ),
        (
            "B with onmessage",
            "const a = new MessageChannel(), b = new MessageChannel(); a.port1.onmessage = () => {}; b.port2.onmessage = () => {}; a.port1.postMessage(1, [b.port1]); b.port2.postMessage(1, [a.port2]);",
        ),
    ];
    on_big_stack(|| {
        for stress in [false, true] {
            for &(name, body) in CASES {
                let mut rt = runtime(stress);
                let source = format!(
                    "(() => {{ for (let i = 0; i < 1000; i++) {{ {body} }} return 'ok'; }})()"
                );
                assert_eq!(answer(&mut rt, &source), expected(r#""ok""#));
                for _ in 0..2 {
                    rt.collect();
                    rt.live_message_ports();
                }
                assert_eq!(rt.live_message_ports(), 0, "{name}, stress {stress}");
                assert_eq!(rt.waiting_messages(), 0, "{name}, stress {stress}");
            }
        }
    });
}

/// 70,000 iterations of case A reach neither the limit of 65,536 ports nor
/// the limit of 65,536 waiting messages, and the budget is free after the
/// loop (the hostile case `message-channel-travelling-cycle-js-on` is the
/// same page).
#[test]
fn a_loop_of_travelling_cycles_does_not_exhaust_the_limits() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let source = "(() => { let error = 'none'; try { for (let i = 0; i < 70000; i++) { const c = new MessageChannel(); c.port1.postMessage(1, [c.port2]); } } catch (e) { error = e.name; } return error; })()";
        assert_eq!(answer(&mut rt, source), expected(r#""none""#));
        rt.collect();
        rt.live_message_ports();
        rt.collect();
        assert_eq!(rt.live_message_ports(), 0);
        assert_eq!(rt.waiting_messages(), 0);
        let again = "(() => { for (let i = 0; i < 70000; i++) { const c = new MessageChannel(); c.port1.postMessage(1, [c.port2]); } return 'ok'; })()";
        assert_eq!(answer(&mut rt, again), expected(r#""ok""#));
    });
}

/// A port that travels to a port that is then closed goes with the closed
/// port, and its message leaves the budget.
#[test]
fn dropped_messages_leave_no_waiting_count() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let source = "(() => { for (let i = 0; i < 70000; i++) { const c = new MessageChannel(); const d = new MessageChannel(); d.port1.close(); c.port1.postMessage(1, [d.port2]); c.port2.close(); } return 'ok'; })()";
        assert_eq!(answer(&mut rt, source), expected(r#""ok""#));
        rt.collect();
        assert_eq!(rt.live_message_ports(), 0);
        assert_eq!(rt.waiting_messages(), 0);
    });
}

/// Closing the port whose partner travels in its own queue lets the sweep
/// free the partner (`close()` counts as churn).
#[test]
fn close_frees_a_partner_that_only_its_own_queue_holds() {
    on_big_stack(|| {
        let mut rt = runtime(false);
        let source = "(() => { for (let i = 0; i < 100; i++) { const c = new MessageChannel(); c.port1.postMessage(1, [c.port2]); c.port1.close(); } return 'ok'; })()";
        assert_eq!(answer(&mut rt, source), expected(r#""ok""#));
        assert_eq!(rt.live_message_ports(), 0);
        assert_eq!(rt.waiting_messages(), 0);
    });
}
