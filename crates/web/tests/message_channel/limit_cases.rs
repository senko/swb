// The cases of tools/probes/web-message-channel.json with the values of Chromium 148.
// Generated from `swbtools probe --json` (three runs, all equal; then `just fmt`);
// do not edit. The cases `limits` and `gc` are in limit_cases.rs and the tests.

use crate::common::Case;

pub(crate) const CASES: &[Case] = &[Case {
    name: r"limits",
    html: r"<!DOCTYPE html><html><head><script>
const E = g => { try { const v = g(); return typeof v === 'string' ? v : String(v); } catch (e) { return e.name + ': ' + e.message; } };
const W = ms => new Promise(r => setTimeout(r, ms));
window.log = [];
</script></head><body><p>x</p></body></html>",
    queries: &[
        (
            r"(() => { for (let i = 0; i < 200000; i++) new MessageChannel(); return 'ok'; })()",
            r#""ok""#,
        ),
        (
            r"(() => { const l = []; for (let i = 0; i < 20000; i++) { const c = new MessageChannel(); c.port1.onmessage = () => l.push(1); c.port2.postMessage(i); } return 'ok'; })()",
            r#""ok""#,
        ),
        (
            r"new Promise(r => { let k = 0; const run = () => { const c = new MessageChannel(); c.port1.onmessage = () => { c.port1.close(); if (++k < 20000) run(); else r(k); }; c.port2.postMessage(0); }; run(); })",
            r"20000",
        ),
        (
            r"new Promise(r => { const c = new MessageChannel(); for (let i = 0; i < 70000; i++) c.port2.postMessage(i); let k = 0; c.port1.onmessage = () => { k++; }; setTimeout(() => r(k), 1500); })",
            r"70000",
        ),
        (
            r"new Promise(r => { const c = new MessageChannel(); const ports = []; for (let i = 0; i < 5000; i++) { const d = new MessageChannel(); ports.push(d.port1, d.port2); } c.port1.postMessage('many', ports); c.port2.onmessage = e => r([e.ports.length, e.ports.every(p => p instanceof MessagePort)]); })",
            r"[10000, true]",
        ),
        (
            r"(() => { const keep = []; for (let i = 0; i < 70000; i++) keep.push(new MessageChannel()); return keep.length; })()",
            r"70000",
        ),
    ],
}];
