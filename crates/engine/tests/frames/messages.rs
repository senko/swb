//! `window.postMessage` and `MessagePort` across runtimes against Chromium (M9 feature
//! 14a, ADR 0031 section 7): the cases `msg-*` and `port-*` of
//! `tools/probes/web-frames.json` with Chromium's values, each in the forms same origin,
//! same site and cross site where the process model could matter; and what no probe can
//! show: the order inside swb, the budgets, what frees what. A departure has a row with
//! its reason.

use serde_json::{Value as Json, json};
use std::time::Duration;
use swb_engine::{AgentLimits, FrameId, Page};

use super::scripted::{compare_case, evaluate, evaluate_in};
use super::{OTHER, Res, frame, open};
use crate::common::on_page_thread;

/// A row where swb departs on purpose: (case, query, path of the value, why).
const DEPARTURES: &[(&str, usize, &str, &str)] = &[
    (
        "msg-initial-reuse-late",
        3,
        "",
        "swb parses the start of a document at its commit, so the first script of the new \
         document has its listener before a message that waited runs; Chromium runs the message \
         first and the script in a later task. A script that listens in its window gets a \
         message that it would miss in Chromium",
    ),
    (
        "message-channel",
        2,
        "[10]",
        "no BroadcastChannel (ADR 0031 section 13)",
    ),
    (
        "message-channel",
        3,
        "[1]",
        "the number of chained timers of 0 ms in 50 ms: 18 or 19 by the machine",
    ),
    (
        "post-message",
        2,
        "[4]",
        "the text holds the raw origin, with the port of the probe's server; \
         `a_message_crosses_in_both_directions` compares it",
    ),
    ("post-message", 2, "[9]", "as the row above"),
];

macro_rules! case_test {
    ($test:ident, $($case:literal),+) => {
        #[test]
        fn $test() {
            on_page_thread(|| {
                let mut failures = Vec::new();
                $(failures.extend(compare_case($case, DEPARTURES));)+
                assert!(failures.is_empty(), "{}", failures.join("\n"));
            });
        }
    };
}

case_test!(
    order_against_timers,
    "msg-order-timers-same",
    "msg-order-timers-site",
    "msg-order-timers-cross"
);
case_test!(
    a_message_posted_in_a_microtask,
    "msg-microtask-same",
    "msg-microtask-site",
    "msg-microtask-cross"
);
case_test!(
    bursts_to_one_target_and_to_two,
    "msg-burst-same",
    "msg-burst-site",
    "msg-burst-cross"
);
case_test!(
    a_target_that_navigates,
    "msg-target-navigates-same",
    "msg-target-navigates-site",
    "msg-target-navigates-cross"
);
case_test!(
    a_target_that_navigates_to_another_origin,
    "msg-target-navigates-other-origin-same",
    "msg-target-navigates-other-origin-site",
    "msg-target-navigates-other-origin-cross"
);
case_test!(
    a_sender_that_is_removed,
    "msg-sender-removed-same",
    "msg-sender-removed-site",
    "msg-sender-removed-cross"
);
case_test!(
    a_sender_that_navigates,
    "msg-sender-navigated-same",
    "msg-sender-navigated-site",
    "msg-sender-navigated-cross"
);
case_test!(a_caller_whose_frame_is_gone, "msg-caller-removed");
case_test!(
    a_target_that_is_removed,
    "msg-target-removed-same",
    "msg-target-removed-site",
    "msg-target-removed-cross"
);
case_test!(
    a_frame_that_is_loading,
    "msg-loading-same",
    "msg-loading-site",
    "msg-loading-cross"
);
case_test!(a_reused_window, "msg-initial-reuse");
case_test!(
    a_reused_window_with_a_late_message,
    "msg-initial-reuse-late"
);
case_test!(sandboxed_frames, "msg-sandbox");
case_test!(two_sandboxed_frames, "msg-sandbox-pair");
case_test!(a_data_frame, "msg-data-frame");
case_test!(a_target_with_scripts_off, "msg-scripts-off");
case_test!(
    the_source_of_a_message,
    "msg-source-identity-same",
    "msg-source-identity-site",
    "msg-source-identity-cross"
);
case_test!(a_chain_of_origins, "msg-chain");
case_test!(a_node_of_a_retired_document, "msg-retired-node");
case_test!(a_kept_window_of_a_removed_frame, "msg-kept-window-removed");
case_test!(
    a_port_across_origins,
    "port-basic-same",
    "port-basic-site",
    "port-basic-cross"
);
case_test!(
    a_port_whose_queue_is_not_enabled,
    "port-not-enabled-same",
    "port-not-enabled-site",
    "port-not-enabled-cross"
);
case_test!(
    close_on_either_side,
    "port-close-same",
    "port-close-site",
    "port-close-cross"
);
case_test!(
    a_port_sent_back_and_through_a_third,
    "port-chain-same",
    "port-chain-site",
    "port-chain-cross"
);
case_test!(
    a_port_in_the_queue_of_a_port,
    "port-in-port-same",
    "port-in-port-site",
    "port-in-port-cross"
);
case_test!(
    a_peer_that_is_removed,
    "port-peer-removed-same",
    "port-peer-removed-site",
    "port-peer-removed-cross"
);
case_test!(
    a_sender_of_port_messages_that_is_removed,
    "port-sender-removed-same",
    "port-sender-removed-site",
    "port-sender-removed-cross"
);
case_test!(
    a_port_in_a_message_whose_target_is_removed,
    "port-target-removed-same",
    "port-target-removed-site",
    "port-target-removed-cross"
);
case_test!(
    port_messages_against_window_messages,
    "port-order-same",
    "port-order-site",
    "port-order-cross"
);
case_test!(
    a_received_port,
    "port-received-same",
    "port-received-site",
    "port-received-cross"
);
case_test!(
    a_partner_that_closed_and_a_port_that_did_not_start,
    "port-partner-closed-timing-same",
    "port-partner-closed-timing-site",
    "port-partner-closed-timing-cross"
);
case_test!(
    a_partner_that_closed_and_a_port_that_started,
    "port-partner-closed-started-same",
    "port-partner-closed-started-site",
    "port-partner-closed-started-cross"
);
case_test!(
    a_peer_that_went_some_time_ago,
    "port-peer-removed-later-same",
    "port-peer-removed-later-site",
    "port-peer-removed-later-cross"
);
case_test!(the_ports_of_a_retired_realm, "port-retired-realm");
case_test!(the_probe_of_post_message, "post-message");
case_test!(the_probe_of_message_channel, "message-channel");

/// A page with a frame of another origin at `/case/x.html`.
fn page_with_frame(child: &str) -> (Page, FrameId) {
    let html = format!(
        "<!DOCTYPE html><title>top</title><iframe id=f src='{OTHER}/case/x.html'></iframe>"
    );
    let (page, _sim) = open(&html, &[("/case/x.html", Res::html(child))], true);
    let f = frame(&page, "f");
    (page, f)
}

fn run(page: &mut Page, ms: u64) {
    page.run_event_loop(Duration::from_millis(ms), Duration::from_secs(10));
}

/// A message goes from the frame to the top level and back, and to its place: the
/// runtime is another, and the listener sees the sender's origin.
#[test]
fn a_message_crosses_in_both_directions() {
    on_page_thread(|| {
        let (mut page, _f) = page_with_frame(
            "<!DOCTYPE html><script>addEventListener('message', e => { window.got = [e.data, e.origin, e.source === parent]; e.source.postMessage('pong', '*'); });</script>",
        );
        assert_eq!(page.runtime_count(), 2);
        let got = evaluate(
            &mut page,
            "new Promise(r => { addEventListener('message', e => r([e.data, e.origin, e.source === f.contentWindow])); f.contentWindow.postMessage({a: 1}, '*'); })",
        );
        assert_eq!(got, json!(["pong", OTHER, true]));
        assert_eq!(
            evaluate_in(&mut page, "f", "[got[0].a, got[1], got[2]]"),
            json!([1, super::MAIN, true])
        );
    });
}

/// A message that waits for a document whose navigation commits before its task goes
/// with the document: no listener of the old or the new document gets it.
#[test]
fn a_message_goes_with_the_document_that_it_was_sent_to() {
    on_page_thread(|| {
        let old = "<!DOCTYPE html><script>addEventListener('message', e => { document.cookie = 'old=' + e.data + '; path=/'; });</script>";
        let new = "<!DOCTYPE html><script>window.got = 0; addEventListener('message', () => got++);</script>";
        let html = format!(
            "<!DOCTYPE html><title>top</title><iframe id=f src='{OTHER}/case/x.html'></iframe>"
        );
        let (mut page, _sim) = open(
            &html,
            &[
                ("/case/x.html", Res::html(old)),
                ("/case/y.html", Res::html(new)),
            ],
            true,
        );
        evaluate(
            &mut page,
            &format!("f.contentWindow.location.href = '{OTHER}/case/y.html'; 1"),
        );
        // The response comes while no turn of the loop runs; the next turn commits first.
        std::thread::sleep(Duration::from_millis(200));
        evaluate(&mut page, "f.contentWindow.postMessage('late', '*'); 1");
        run(&mut page, 100);
        assert_eq!(evaluate_in(&mut page, "f", "got"), json!(0));
        assert_eq!(evaluate(&mut page, "document.cookie"), json!(""));
    });
}

/// The loops that ask whether there is work see a message as soon as its sender's task
/// ended.
#[test]
fn a_message_in_hand_counts_as_work() {
    on_page_thread(|| {
        let (mut page, _f) = page_with_frame(
            "<!DOCTYPE html><script>window.got = 0; addEventListener('message', () => got++);</script>",
        );
        page.wait_until_idle(Duration::ZERO, Duration::from_secs(5));
        evaluate(&mut page, "f.contentWindow.postMessage(0, '*'); 1");
        assert!(!page.is_idle(Duration::ZERO));
        assert!(page.wait_until_idle(Duration::ZERO, Duration::from_secs(5)));
        assert_eq!(evaluate_in(&mut page, "f", "got"), json!(1));
    });
}

/// A frame fills its budget with messages to a port of the top level that does not start.
/// They count for the frame: the top level's budget is free, and its messages go on.
#[test]
fn a_flood_of_a_frame_leaves_the_budget_of_the_top_level() {
    on_page_thread(|| {
        let (mut page, _f) = page_with_frame(
            "<!DOCTYPE html><script>addEventListener('message', e => { window.p = e.ports[0]; const b = new Uint8Array(1 << 20); for (let i = 0; i < 400; i++) p.postMessage(b); });</script>",
        );
        evaluate(
            &mut page,
            "window.c = new MessageChannel(); f.contentWindow.postMessage(0, '*', [c.port2]); 1",
        );
        run(&mut page, 200);
        let waiting = page.waiting_messages_by_runtime();
        assert_eq!(waiting[0], (0, 0), "{waiting:?}");
        assert!(waiting[1].0 > 200 && waiting[1].0 < 400, "{waiting:?}");
        assert!(page.frames_message_bytes() > 200 << 20);
        // 64 MiB of the top level's own.
        let got = evaluate(
            &mut page,
            "new Promise(r => { const d = new MessageChannel(); d.port1.onmessage = e => r(e.data.byteLength); d.port2.postMessage(new Uint8Array(1 << 26)); })",
        );
        assert_eq!(got, json!(1 << 26));
        // The top level closes its port: what waited goes back to the frame's budget.
        evaluate(&mut page, "c.port1.close(); 1");
        evaluate(&mut page, "1");
        assert_eq!(page.waiting_messages_by_runtime()[1].0, 0);
        assert_eq!(page.frames_message_bytes(), 0);
    });
}

/// A frame that sends to the top level in a loop stops at the count of its own budget,
/// and the top level gets what was sent.
#[test]
fn a_loop_of_messages_to_the_top_level_stops_at_the_budget_of_the_frame() {
    on_page_thread(|| {
        let (mut page, _f) = page_with_frame(
            "<!DOCTYPE html><script>addEventListener('message', () => { for (let i = 0; i < 70000; i++) parent.postMessage(i, '*'); });</script>",
        );
        let got = evaluate(
            &mut page,
            "new Promise(r => { window.n = 0; addEventListener('message', () => { if (++n === 65536) setTimeout(() => r(n), 100); }); f.contentWindow.postMessage(0, '*'); })",
        );
        assert_eq!(got, json!(65536));
        assert_eq!(page.waiting_messages_by_runtime()[1].0, 0);
    });
}

/// Ports that a frame makes and sends to the top level count for the frame: at the limit
/// the frame gets the `RangeError`, and the top level makes its channels.
#[test]
fn ports_sent_to_the_top_level_count_for_the_frame_that_made_them() {
    on_page_thread(|| {
        let (mut page, _f) = page_with_frame(
            "<!DOCTYPE html><script>window.kept = []; window.refused = 0; window.go = () => { for (let i = 0; i < 40000; i++) { try { const c = new MessageChannel(); kept.push(c.port1); parent.postMessage(0, '*', [c.port2]); } catch (e) { refused++; } } };</script>",
        );
        evaluate(
            &mut page,
            "window.got = []; addEventListener('message', e => got.push(e.ports[0])); 1",
        );
        evaluate_in(&mut page, "f", "go(); 1");
        run(&mut page, 300);
        evaluate_in(&mut page, "f", "go(); 1");
        run(&mut page, 300);
        let refused = evaluate_in(&mut page, "f", "refused").as_u64().unwrap();
        assert!(refused > 0);
        assert_eq!(
            evaluate(
                &mut page,
                "[new MessageChannel() instanceof MessageChannel, got.length > 30000]"
            ),
            json!([true, true])
        );
    });
}

/// A runtime that reaches the heap limit: the messages that wait in its ports and windows
/// go, their bytes go back to their senders, and what it was entangled with is free.
#[test]
fn a_runtime_that_goes_gives_back_what_waits_for_it() {
    on_page_thread(|| {
        let (mut page, f) = page_with_frame(
            "<!DOCTYPE html><script>window.a = []; addEventListener('message', e => { if (e.ports.length) { window.p = e.ports[0]; window.q = e.ports[1]; } }); window.fill = () => { for (;;) a.push(new Array(1000).fill(1)); };</script>",
        );
        page.set_agent_limits(AgentLimits {
            heap: 64 << 20,
            page_heap: 512 << 20,
            ..AgentLimits::default()
        });
        evaluate(
            &mut page,
            "window.c = new MessageChannel(); window.d = new MessageChannel(); f.contentWindow.postMessage(0, '*', [c.port2, d.port2]); 1",
        );
        run(&mut page, 50);
        assert_eq!(page.live_message_ports(), 4);
        evaluate(
            &mut page,
            "for (let i = 0; i < 20; i++) c.port1.postMessage(new Uint8Array(1 << 20)); 1",
        );
        assert_eq!(page.waiting_messages_by_runtime()[0].0, 20);
        evaluate_in(&mut page, "f", "setTimeout(fill, 1); 1");
        run(&mut page, 200);
        assert!(!page.frame_has_realm(f));
        assert_eq!(page.runtime_count(), 1);
        assert_eq!(page.waiting_messages_by_runtime()[0], (0, 0));
        assert_eq!(page.live_message_ports(), 2);
        // The sender is alone: a message goes nowhere, and no error comes.
        assert_eq!(evaluate(&mut page, "c.port1.postMessage(1); 1"), json!(1));
        assert_eq!(page.waiting_messages_by_runtime()[0], (0, 0));
    });
}

/// A runtime whose scripts stopped gives back what waits for it: no task will take it.
#[test]
fn a_runtime_that_stops_gives_back_what_waits_for_it() {
    on_page_thread(|| {
        let (mut page, _f) = page_with_frame(
            "<!DOCTYPE html><script>addEventListener('message', e => { window.p = e.ports[0]; const end = performance.now() + 300; while (performance.now() < end); });</script>",
        );
        page.set_agent_limits(AgentLimits {
            long_tasks: Duration::from_millis(100),
            page_long_tasks: Duration::from_secs(120),
            ..AgentLimits::default()
        });
        evaluate(
            &mut page,
            "window.c = new MessageChannel(); f.contentWindow.postMessage(0, '*', [c.port2]); 1",
        );
        run(&mut page, 400);
        evaluate(
            &mut page,
            "c.port1.postMessage(1); f.contentWindow.postMessage(2, '*'); 1",
        );
        run(&mut page, 50);
        assert_eq!(page.waiting_messages_by_runtime()[0], (0, 0));
        assert_eq!(page.live_message_ports(), 1);
    });
}

/// A window message that nobody receives closes the ports of its record: its target
/// origin does not match, its target is gone, or its target has no runtime.
#[test]
fn a_dropped_message_closes_its_ports() {
    on_page_thread(|| {
        let (mut page, _f) = page_with_frame(
            "<!DOCTYPE html><script>addEventListener('message', e => { window.got = e.ports.length; });</script>",
        );
        evaluate(
            &mut page,
            "window.c = new MessageChannel(); c.port1.onmessage = () => {}; f.contentWindow.postMessage(0, 'http://elsewhere.test', [c.port2]); 1",
        );
        run(&mut page, 50);
        assert_eq!(page.live_message_ports(), 1, "the target origin");
        evaluate(
            &mut page,
            "window.d = new MessageChannel(); d.port1.onmessage = () => {}; f.contentWindow.postMessage(0, '*', [d.port2]); f.remove(); 1",
        );
        run(&mut page, 50);
        assert_eq!(page.live_message_ports(), 2, "the target that is gone");
        assert_eq!(page.waiting_messages_by_runtime()[0], (0, 0));
    });
}

/// The lifetime of a pair across runtimes. A port that no script holds and that has no
/// listener goes with its collector and frees its partner, which then goes with its own.
#[test]
fn a_port_without_a_listener_goes_and_frees_its_partner() {
    on_page_thread(|| {
        let (mut page, _f) = page_with_frame(
            "<!DOCTYPE html><script>addEventListener('message', () => {});</script>",
        );
        evaluate(
            &mut page,
            "(() => { const c = new MessageChannel(); c.port1.onmessage = () => {}; f.contentWindow.postMessage(0, '*', [c.port2]); })(); 1",
        );
        run(&mut page, 50);
        assert_eq!(page.live_message_ports(), 2);
        page.collect_all_runtimes();
        assert_eq!(
            page.live_message_ports(),
            1,
            "the port of the top level stays"
        );
        page.collect_all_runtimes();
        assert_eq!(page.live_message_ports(), 0);
    });
}

/// Listeners on both sides that no script reaches: the pair stays until a runtime goes, a
/// departure from Chromium that `MAX_PORTS` bounds (one heap cannot see the other).
#[test]
fn a_pair_with_listeners_on_both_sides_stays_until_a_side_goes() {
    on_page_thread(|| {
        let (mut page, _f) = page_with_frame(
            "<!DOCTYPE html><script>addEventListener('message', e => { e.ports[0].onmessage = () => {}; });</script>",
        );
        evaluate(
            &mut page,
            "(() => { const c = new MessageChannel(); c.port1.onmessage = () => {}; f.contentWindow.postMessage(0, '*', [c.port2]); })(); 1",
        );
        run(&mut page, 50);
        page.collect_all_runtimes();
        page.collect_all_runtimes();
        assert_eq!(page.live_message_ports(), 2);
        evaluate(&mut page, "f.remove(); 1");
        run(&mut page, 10);
        page.collect_all_runtimes();
        run(&mut page, 10);
        assert_eq!(page.live_message_ports(), 0);
    });
}

/// A message that does not fit in the heap of its target fires `messageerror`: a frame
/// cannot end the scripts of the top level with a large message.
#[test]
fn a_message_too_large_for_the_heap_gives_messageerror() {
    on_page_thread(|| {
        let (mut page, _f) = page_with_frame(
            "<!DOCTYPE html><script>addEventListener('message', () => { parent.postMessage(new Uint8Array(96 << 20), '*'); });</script>",
        );
        page.set_agent_limits(AgentLimits {
            heap: 128 << 20,
            page_heap: 512 << 20,
            ..AgentLimits::default()
        });
        evaluate(&mut page, "window.keep = new Uint8Array(64 << 20); 1");
        let got = evaluate(
            &mut page,
            "new Promise(r => { addEventListener('message', () => r('message')); addEventListener('messageerror', e => r(['messageerror', e.data, e.origin === new URL(f.src).origin, e.source === f.contentWindow])); f.contentWindow.postMessage(0, '*'); })",
        );
        assert_eq!(got, json!(["messageerror", null, true, true]));
        assert!(page.has_script_runtime());
    });
}

/// What a message takes in the heap can be more than its estimate, and a frame does not
/// know the heap of the top level. The rows: objects of the top level, objects of the message,
/// the size of a string of the top level, and the event if it is sure. In the last two rows
/// the heap has no free slots: the message takes more than its estimate there, and only the
/// collection after the deserialization shows that. The top level keeps running in each, and
/// the port in a message that does not fit closes.
fn a_message_near_the_limit(send: &str) {
    const ROWS: &[(u32, u32, u64, Option<&str>)] = &[
        (1_040_000, 50_000, 40_000_000, Some("message")),
        (1_040_000, 50_000, 85_400_000, Some("messageerror")),
        (0, 100_000, 136_000_000, None),
        (0, 100_000, 137_000_000, None),
    ];
    for &(objects, message, size, event) in ROWS {
        let child = format!(
            "<!DOCTYPE html><script>addEventListener('message', e => {{ const v = Array.from({{length: {message}}}, (_, i) => ({{['a' + i]: 1, ['b' + i]: 2, ['c' + i]: 3}})); window.d = new MessageChannel(); {send}; }});</script>"
        );
        let (mut page, _f) = page_with_frame(&child);
        let filled = evaluate(
            &mut page,
            &format!(
                "window.fill = Array.from({{length: {objects}}}, () => ({{}})); \
                 window.keep = 'x'.repeat({size}); keep.charCodeAt(1); window.got = []; \
                 for (const name of ['message', 'messageerror']) addEventListener(name, e => got.push(name)); \
                 window.c = new MessageChannel(); \
                 c.port1.onmessage = () => got.push('message'); c.port1.onmessageerror = () => got.push('messageerror'); \
                 f.contentWindow.postMessage(0, '*', [c.port2]); 1"
            ),
        );
        assert_eq!(filled, json!(1));
        let got = evaluate(
            &mut page,
            "new Promise(r => { const check = () => got.length ? r(got) : setTimeout(check, 5); check(); })",
        );
        let got = got.as_array().unwrap_or_else(|| panic!("{size}: {got}"));
        assert_eq!(got.len(), 1, "{size}");
        if let Some(event) = event {
            assert_eq!(got[0], json!(event), "{size}");
        }
        if got[0] == json!("messageerror") {
            // Those of `c`, and the end of `d` that stayed in the frame.
            assert_eq!(page.live_message_ports(), 3, "{size}");
        }
        page.collect_all_runtimes();
        let more = evaluate(
            &mut page,
            "new Promise(r => setTimeout(() => r(new Array(1000).fill(0).map(() => ({})).length), 5))",
        );
        assert_eq!(more, json!(1000), "{size}");
        assert!(page.has_script_runtime());
    }
}

#[test]
fn a_window_message_near_the_limit_leaves_the_top_level_running() {
    on_page_thread(|| a_message_near_the_limit("parent.postMessage(v, '*', [d.port2])"));
}

#[test]
fn a_port_message_near_the_limit_leaves_the_top_level_running() {
    on_page_thread(|| a_message_near_the_limit("e.ports[0].postMessage(v, [d.port2])"));
}

/// A frame sends messages that take long to deserialize, each of which needs what the last left
/// as garbage: all arrive. The long tasks of the top level do not
/// count that time, so the frame cannot use up the top level's budget with them.
#[test]
fn the_deserialization_of_a_message_is_no_long_task_of_its_target() {
    on_page_thread(|| {
        let (mut page, _f) = page_with_frame(
            "<!DOCTYPE html><script>addEventListener('message', () => { const v = Array.from({length: 200000}, (_, i) => ({a: i, b: i, c: i})); for (let i = 0; i < 4; i++) parent.postMessage(v, '*'); });</script>",
        );
        let got = evaluate(
            &mut page,
            "new Promise(r => { let n = 0; addEventListener('message', () => { if (++n === 4) r(n); }); addEventListener('messageerror', () => { r('messageerror after ' + n); }); f.contentWindow.postMessage(0, '*'); })",
        );
        assert_eq!(got, json!(4));
        let spent = page.script_runtime_mut().unwrap().long_task_time();
        assert!(spent < Duration::from_millis(300), "{spent:?}");
    });
}

/// A message without a payload counts for its budget too, so frames together stop at
/// the bytes of the budget, not at the count per runtime.
#[test]
fn messages_without_a_payload_count_in_the_budget() {
    on_page_thread(|| {
        let (mut page, _f) = page_with_frame(
            "<!DOCTYPE html><script>addEventListener('message', e => { for (let i = 0; i < 1000; i++) e.ports[0].postMessage(null); });</script>",
        );
        evaluate(
            &mut page,
            "window.c = new MessageChannel(); f.contentWindow.postMessage(0, '*', [c.port2]); 1",
        );
        run(&mut page, 200);
        let waiting = page.waiting_messages_by_runtime();
        assert_eq!(waiting[1].0, 1000, "{waiting:?}");
        assert!(waiting[1].1 >= 400_000, "{waiting:?}");
    });
}

/// A frame of another origin whose document has no script has no runtime: nothing takes the
/// message, nothing fails, and the port of the message closes.
#[test]
fn a_target_without_a_runtime_gets_nothing() {
    on_page_thread(|| {
        let (mut page, f) = page_with_frame("<!DOCTYPE html><p>no script");
        assert_eq!(page.runtime_count(), 1);
        assert!(!page.frame_has_realm(f));
        let got = evaluate(
            &mut page,
            "(() => { const c = new MessageChannel(); window.c = c; c.port1.onmessage = () => {}; f.contentWindow.postMessage(0, '*', [c.port2]); return 1; })()",
        );
        assert_eq!(got, json!(1));
        run(&mut page, 10);
        assert_eq!(page.live_message_ports(), 1);
        assert_eq!(page.runtime_count(), 1);
    });
}

/// `document-domain-setter` as the frame reports it, with the row of `domain.rs`: a value
/// with a space.
#[test]
fn the_setter_of_a_domain_in_a_frame() {
    on_page_thread(|| {
        let failures = compare_case("document-domain", DEPARTURES);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        let want = super::js_value("document-domain-setter", 0);
        let want: Json = serde_json::from_str(want.as_str().unwrap()).unwrap();
        let (mut page, _sim) = super::open_case("document-domain-setter");
        let label = super::results("document-domain-setter")[0]["label"]
            .as_str()
            .unwrap()
            .to_owned();
        let got = evaluate(&mut page, &label);
        let got: Json = serde_json::from_str(got.as_str().unwrap()).unwrap();
        let (got, want) = (got.as_array().unwrap(), want.as_array().unwrap());
        assert_eq!(got.len(), want.len());
        for (index, (got, want)) in got.iter().zip(want).enumerate() {
            if index != 13 {
                assert_eq!(got, want, "row {index}");
            }
        }
    });
}

/// `cookies-frames`: what the frames report (the case's log of requests comes from the
/// server of the probe; `cookies_of_frame_documents_follow_the_site_for_cookies` has the
/// requests).
#[test]
fn what_the_frames_report_about_cookies() {
    on_page_thread(|| {
        let (mut page, _sim) = super::open_case("cookies-frames");
        let got = evaluate(
            &mut page,
            "new Promise(r => { const res = {}; addEventListener('message', e => { if (e.data && 'cookie' in e.data) { res[e.data.host] = e.data; if (Object.keys(res).length === 2) r(res); } }); f1.src = L3('tp.html'); f2.src = X('tp.html'); })",
        );
        let want = super::js_value("cookies-frames", 0);
        let want: Json = serde_json::from_str(want[0].as_str().unwrap()).unwrap();
        for host in ["localhost", "127.0.0.1"] {
            // `hsa`: `document.hasStorageAccess` does not exist in swb, and the rows of
            // `cookie` belong to the rules of `document.cookie` in a frame (feature 14b).
            for key in ["host", "ls"] {
                assert_eq!(got[host][key], want[host][key], "{host} {key}");
            }
        }
    });
}
