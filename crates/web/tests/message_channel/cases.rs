// The cases of tools/probes/web-message-channel.json with the values of Chromium 148.
// Generated from `swbtools probe --json` (three runs, all equal; then `just fmt`);
// do not edit. The cases `limits` and `gc` are in limit_cases.rs and the tests.

use crate::common::Case;

pub(crate) const CASES: &[Case] = &[
    Case {
        name: r"api",
        html: r"<!DOCTYPE html><html><head><script>
const E = g => { try { const v = g(); return typeof v === 'string' ? v : String(v); } catch (e) { return e.name + ': ' + e.message; } };
const W = ms => new Promise(r => setTimeout(r, ms));
window.log = [];
</script></head><body><p>x</p></body></html>",
        queries: &[
            (
                r"[Object.getOwnPropertyNames(MessagePort.prototype).join(), Object.getOwnPropertyNames(MessageChannel.prototype).join(), MessageChannel.length, MessagePort.length, typeof MessagePort, typeof MessageChannel, Object.getPrototypeOf(MessagePort.prototype) === EventTarget.prototype, Object.getPrototypeOf(MessageChannel.prototype) === Object.prototype, Object.prototype.toString.call(new MessageChannel()), Object.prototype.toString.call(new MessageChannel().port1), MessagePort.name, MessageChannel.name]",
                r#"["onmessage,onmessageerror,close,postMessage,start,constructor", "port1,port2,constructor", 0, 0, "function", "function", true, true, "[object MessageChannel]", "[object MessagePort]", "MessagePort", "MessageChannel"]"#,
            ),
            (
                r"[E(() => new MessagePort()), E(() => MessageChannel()), E(() => MessagePort()), E(() => new MessageChannel(1, 2).port1 instanceof MessagePort), E(() => Object.getOwnPropertyNames(window).filter(k => /^Message/.test(k)).join()), JSON.stringify(Object.getOwnPropertyDescriptor(window, 'MessageChannel')), JSON.stringify(Object.getOwnPropertyDescriptor(window, 'MessagePort'))]",
                r#"["TypeError: Failed to construct 'MessagePort': Illegal constructor", "TypeError: Failed to construct 'MessageChannel': Please use the 'new' operator, this DOM object constructor cannot be called as a function.", "TypeError: Illegal constructor", "true", "MessagePort,MessageEvent,MessageChannel", "{\"writable\":true,\"enumerable\":false,\"configurable\":true}", "{\"writable\":true,\"enumerable\":false,\"configurable\":true}"]"#,
            ),
            (
                r"(() => { const c = new MessageChannel(); const d = Object.getOwnPropertyDescriptor(MessageChannel.prototype, 'port1'); const o = Object.getOwnPropertyDescriptor(MessagePort.prototype, 'onmessage'); const m = Object.getOwnPropertyDescriptor(MessagePort.prototype, 'postMessage'); return [c.port1 === c.port1, c.port1 !== c.port2, typeof d.get, d.set, d.enumerable, d.configurable, typeof o.get, typeof o.set, o.enumerable, o.configurable, m.enumerable, m.writable, m.configurable, MessagePort.prototype.postMessage.length, MessagePort.prototype.start.length, MessagePort.prototype.close.length, MessagePort.prototype.postMessage.name, String(c.port1.onmessage), String(c.port1.onmessageerror), Object.keys(c).join(), Object.keys(c.port1).join(), JSON.stringify(c.port1), String(c.port1), c.port1.toString === EventTarget.prototype.toString]; })()",
                r#"[true, true, "function", null, true, true, "function", "function", true, true, true, true, true, 1, 0, 0, "postMessage", "null", "null", "", "", "{}", "[object MessagePort]", true]"#,
            ),
            (
                r"(() => { const c = new MessageChannel(); const p = c.port1; const f = () => 1; p.onmessage = 5; const a = String(p.onmessage); p.onmessage = f; const b = p.onmessage === f; p.onmessage = {}; const d = String(p.onmessage); p.onmessage = {handleEvent() {}}; const e = String(p.onmessage); p.onmessage = null; p.onmessageerror = f; return [a, b, d, e, String(p.onmessage), p.onmessageerror === f]; })()",
                r#"["null", true, "[object Object]", "[object Object]", "null", true]"#,
            ),
            (
                r"[E(() => new MessageChannel().port1.postMessage()), E(() => new MessageChannel().port1.postMessage(1, 5)), E(() => new MessageChannel().port1.postMessage(1, [1])), E(() => new MessageChannel().port1.postMessage(1, {transfer: 5})), E(() => new MessageChannel().port1.postMessage(1, {transfer: [1]})), E(() => new MessageChannel().port1.postMessage(1, [{}])), E(() => new MessageChannel().port1.postMessage(1, null)), E(() => new MessageChannel().port1.postMessage(1, undefined)), E(() => new MessageChannel().port1.postMessage(1, {})), E(() => new MessageChannel().port1.postMessage(() => 1)), E(() => new MessageChannel().port1.postMessage(Symbol('s'))), E(() => new MessageChannel().port1.postMessage(document.body)), E(() => new MessageChannel().port1.postMessage(window)), E(() => new MessageChannel().port1.postMessage(1, 1, 1))]",
                r#"["TypeError: Failed to execute 'postMessage' on 'MessagePort': 1 argument required, but only 0 present.", "TypeError: Failed to execute 'postMessage' on 'MessagePort': Overload resolution failed.", "TypeError: Failed to execute 'postMessage' on 'MessagePort': Failed to convert value to 'object'.", "TypeError: Failed to execute 'postMessage' on 'MessagePort': Failed to read the 'transfer' property from 'StructuredSerializeOptions': The provided value cannot be converted to a sequence.", "TypeError: Failed to execute 'postMessage' on 'MessagePort': Failed to read the 'transfer' property from 'StructuredSerializeOptions': Failed to convert value to 'object'.", "DataCloneError: Failed to execute 'postMessage' on 'MessagePort': Value at index 0 does not have a transferable type.", "undefined", "undefined", "undefined", "DataCloneError: Failed to execute 'postMessage' on 'MessagePort': () => 1 could not be cloned.", "DataCloneError: Failed to execute 'postMessage' on 'MessagePort': Symbol(s) could not be cloned.", "DataCloneError: Failed to execute 'postMessage' on 'MessagePort': HTMLBodyElement object could not be cloned.", "DataCloneError: Failed to execute 'postMessage' on 'MessagePort': #<Window> could not be cloned.", "TypeError: Failed to execute 'postMessage' on 'MessagePort': Overload resolution failed."]"#,
            ),
            (
                r"[E(() => MessagePort.prototype.postMessage.call({}, 1)), E(() => MessagePort.prototype.start.call({})), E(() => MessagePort.prototype.close.call({})), E(() => Object.getOwnPropertyDescriptor(MessageChannel.prototype, 'port1').get.call({})), E(() => Object.getOwnPropertyDescriptor(MessagePort.prototype, 'onmessage').get.call({})), E(() => Object.getOwnPropertyDescriptor(MessagePort.prototype, 'onmessage').set.call({}, null)), E(() => MessagePort.prototype.postMessage.call(new MessageChannel().port1, 1)), E(() => MessagePort.prototype.close.call(new EventTarget())), E(() => new (class extends MessageChannel {})().port1 instanceof MessagePort), E(() => { class P extends MessagePort {} return new P(); })]",
                r#"["TypeError: Illegal invocation", "TypeError: Illegal invocation", "TypeError: Illegal invocation", "TypeError: Illegal invocation", "TypeError: Illegal invocation", "TypeError: Illegal invocation", "undefined", "TypeError: Illegal invocation", "true", "TypeError: Failed to construct 'MessagePort': Illegal constructor"]"#,
            ),
            (
                r"[E(() => new MessageChannel().port1.dispatchEvent(new Event('message'))), E(() => { const p = new MessageChannel().port1; return [typeof p.addEventListener, typeof p.removeEventListener, 'onclose' in p, 'onmessage' in p, 'onmessageerror' in p, 'detach' in p, 'transfer' in p].join(); }), E(() => { const c = new MessageChannel(); return Object.isExtensible(c.port1) && Object.isExtensible(c); }), E(() => { const c = new MessageChannel(); c.port1.x = 5; return c.port1.x; }), E(() => { const c = new MessageChannel(); c.x = 5; return c.x; }), E(() => { const c = new MessageChannel(); return Object.isFrozen(c) + ',' + Object.isSealed(c.port1); })]",
                r#"["true", "function,function,false,true,true,false,false", "true", "5", "5", "false,false"]"#,
            ),
            (r"typeof BroadcastChannel", r#""function""#),
        ],
    },
    Case {
        name: r"queue-start",
        html: r"<!DOCTYPE html><html><head><script>
const E = g => { try { const v = g(); return typeof v === 'string' ? v : String(v); } catch (e) { return e.name + ': ' + e.message; } };
const W = ms => new Promise(r => setTimeout(r, ms));
window.log = [];
</script></head><body><p>x</p></body></html>",
        queries: &[
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.onmessage = e => l.push('on' + e.data); c.port2.postMessage(1); setTimeout(() => r(l), 100); })",
                r#"["on1"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.addEventListener('message', e => l.push('a' + e.data)); c.port2.postMessage(1); setTimeout(() => { l.push('before start'); c.port1.start(); c.port1.start(); c.port2.postMessage(2); l.push('after start'); setTimeout(() => r(l), 50); }, 50); })",
                r#"["before start", "after start", "a1", "a2"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.onmessageerror = e => l.push('err'); c.port2.postMessage(1); setTimeout(() => { l.push('onmessageerror alone'); c.port1.onmessage = e => l.push('m' + e.data); setTimeout(() => r(l), 50); }, 50); })",
                r#"["onmessageerror alone"]"#,
            ),
            (
                r"new Promise(r => { const out = []; let i = 0; const one = (name, act) => { const c = new MessageChannel(); const l = []; c.port2.postMessage(1); act(c.port1); setTimeout(() => { c.port1.onmessage = e => l.push(e.data); setTimeout(() => { out.push(name + ':' + l.join()); if (++i === 6) r(out); }, 30); }, 30); }; one('null', p => { p.onmessage = null; }); one('number', p => { p.onmessage = 5; }); one('onmessageerror-null', p => { p.onmessageerror = null; }); one('onmessageerror-fn', p => { p.onmessageerror = () => {}; }); one('addEventListener-messageerror', p => { p.addEventListener('messageerror', () => {}); }); one('listener-then-removed', p => { const f = () => {}; p.addEventListener('message', f); p.removeEventListener('message', f); }); })",
                r#"["null:", "number:", "onmessageerror-null:", "onmessageerror-fn:", "addEventListener-messageerror:1", "listener-then-removed:1"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.onmessage = e => l.push('first' + e.data); c.port2.postMessage(1); setTimeout(() => { c.port1.onmessage = null; c.port2.postMessage(2); setTimeout(() => { c.port1.addEventListener('message', e => l.push('late' + e.data)); c.port2.postMessage(3); setTimeout(() => r(l), 50); }, 50); }, 50); })",
                r#"["first1", "late3"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port2.postMessage('a'); c.port2.postMessage('b'); setTimeout(() => { c.port1.onmessage = e => l.push(e.data); l.push('set'); setTimeout(() => r(l), 50); }, 50); })",
                r#"["set", "a", "b"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port2.postMessage('a'); c.port1.onmessage = e => l.push(e.data); l.push('sync'); setTimeout(() => r(l), 50); })",
                r#"["sync", "a"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); const f = e => l.push('f' + e.data); const g = e => { l.push('g' + e.data); throw new Error('x'); }; c.port1.addEventListener('message', f); c.port1.addEventListener('message', g); c.port1.addEventListener('message', f); c.port1.start(); c.port2.postMessage(1); setTimeout(() => { c.port1.removeEventListener('message', f); c.port2.postMessage(2); setTimeout(() => r(l), 50); }, 50); })",
                r#"["f1", "g1", "g2"]"#,
            ),
            (
                r"new Promise(r => { const c = new MessageChannel(); c.port1.onmessage = function (e) { r([this === c.port1, e.target === c.port1, e.currentTarget === c.port1, e.constructor.name, Object.prototype.toString.call(e), e.type, e.data, e.origin, JSON.stringify(e.lastEventId), String(e.source), e.ports.length, Array.isArray(e.ports), e.isTrusted, e.bubbles, e.cancelable, e.composed, e.eventPhase, e.defaultPrevented, e.timeStamp > 0, e.srcElement === c.port1, e.returnValue, window.event === e]); }; c.port2.postMessage({a: [1, 2]}); })",
                r#"[true, true, true, "MessageEvent", "[object MessageEvent]", "message", {"a": [1, 2]}, "", "\"\"", "null", 0, true, true, false, false, false, 2, false, true, true, true, true]"#,
            ),
            (
                r"new Promise(r => { const l = []; addEventListener('message', e => l.push('window')); document.addEventListener('message', e => l.push('document'), true); const c = new MessageChannel(); c.port1.onmessage = e => l.push('port'); c.port2.postMessage(1); setTimeout(() => r(l), 50); })",
                r#"["port"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.onmessage = e => l.push('p1' + e.data); c.port2.onmessage = e => l.push('p2' + e.data); c.port1.postMessage(1); c.port2.postMessage(2); setTimeout(() => r(l), 50); })",
                r#"["p21", "p12"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.addEventListener('message', e => { l.push('a'); e.stopImmediatePropagation(); }); c.port1.addEventListener('message', e => l.push('b')); c.port1.start(); c.port2.postMessage(1); c.port1.dispatchEvent(new MessageEvent('message', {data: 'manual'})); setTimeout(() => r(l), 50); })",
                r#"["a", "a"]"#,
            ),
            (
                r"new Promise(r => { const c = new MessageChannel(); const o = {a: [1, {b: 2}], d: new Date(5), m: new Map([[1, 2]])}; o.self = o; c.port1.onmessage = e => r([e.data !== o, e.data.self === e.data, e.data.a[1].b, e.data.d.getTime(), [...e.data.m].join(), Object.getPrototypeOf(e.data) === Object.prototype]); c.port2.postMessage(o); })",
                r#"[true, true, 2, 5, "1,2", true]"#,
            ),
            (
                r"new Promise(r => { const c = new MessageChannel(); const b = new ArrayBuffer(8); const v = new Uint8Array(b); v[0] = 9; c.port1.onmessage = e => r([b.byteLength, v.length, e.data.byteLength, new Uint8Array(e.data)[0], e.ports.length]); c.port2.postMessage(b, [b]); })",
                r"[0, 0, 8, 9, 0]",
            ),
            (
                r"E(() => { const c = new MessageChannel(); const b = new ArrayBuffer(8); c.port1.postMessage(b, [b, b]); })",
                r#""DataCloneError: Failed to execute 'postMessage' on 'MessagePort': ArrayBuffer at index 1 is a duplicate of an earlier ArrayBuffer.""#,
            ),
            (
                r"E(() => { const c = new MessageChannel(); const b = new ArrayBuffer(8); c.port1.postMessage(b, {transfer: [b]}); return b.byteLength; })",
                r#""0""#,
            ),
            (
                r"E(() => { const c = new MessageChannel(); const b = new ArrayBuffer(8); c.port1.postMessage(b, [b]); c.port1.postMessage(b, [b]); })",
                r#""DataCloneError: Failed to execute 'postMessage' on 'MessagePort': ArrayBuffer at index 0 is already detached.""#,
            ),
        ],
    },
    Case {
        name: r"order",
        html: r"<!DOCTYPE html><html><head><script>
const E = g => { try { const v = g(); return typeof v === 'string' ? v : String(v); } catch (e) { return e.name + ': ' + e.message; } };
const W = ms => new Promise(r => setTimeout(r, ms));
window.log = [];
</script></head><body><p>x</p></body></html>",
        queries: &[
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.onmessage = e => l.push('m' + e.data); setTimeout(() => l.push('t0'), 0); c.port2.postMessage(1); Promise.resolve().then(() => l.push('micro')); c.port2.postMessage(2); addEventListener('message', function h(e) { removeEventListener('message', h); l.push('win'); }); postMessage('w', '*'); c.port2.postMessage(3); setTimeout(() => l.push('t0b'), 0); setTimeout(() => l.push('t1'), 1); l.push('sync'); setTimeout(() => r(l), 100); })",
                r#"["sync", "micro", "t0", "m1", "win", "t0b", "m2", "m3", "t1"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.onmessage = e => l.push('m' + e.data); c.port2.postMessage(1); setTimeout(() => l.push('t0'), 0); setTimeout(() => r(l), 100); })",
                r#"["m1", "t0"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const a = new MessageChannel(), b = new MessageChannel(); a.port1.onmessage = e => l.push('a' + e.data); b.port1.onmessage = e => l.push('b' + e.data); a.port2.postMessage(1); b.port2.postMessage(2); a.port2.postMessage(3); b.port2.postMessage(4); setTimeout(() => r(l), 100); })",
                r#"["a1", "b2", "a3", "b4"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.onmessage = e => { l.push('m' + e.data); if (e.data === 1) c.port2.postMessage(3); }; c.port2.postMessage(1); c.port2.postMessage(2); setTimeout(() => l.push('t0'), 0); setTimeout(() => r(l), 100); })",
                r#"["m1", "t0", "m2", "m3"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.onmessage = e => { l.push('m' + e.data); if (e.data === 1) setTimeout(() => l.push('t0'), 0); }; c.port2.postMessage(1); c.port2.postMessage(2); setTimeout(() => r(l), 100); })",
                r#"["m1", "t0", "m2"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); const h = e => l.push('win'); addEventListener('message', h); c.port1.onmessage = e => { l.push('m' + e.data); if (e.data === 1) postMessage('w', '*'); }; c.port2.postMessage(1); c.port2.postMessage(2); setTimeout(() => { removeEventListener('message', h); r(l); }, 100); })",
                r#"["m1", "win", "m2"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.onmessage = e => { l.push('m' + e.data); if (e.data === 1) { c.port2.postMessage(3); setTimeout(() => l.push('t0'), 0); } }; c.port2.postMessage(1); c.port2.postMessage(2); setTimeout(() => r(l), 100); })",
                r#"["m1", "t0", "m2", "m3"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.onmessage = e => { l.push('m' + e.data); if (e.data === 1) { setTimeout(() => l.push('t0'), 0); c.port2.postMessage(3); } }; c.port2.postMessage(1); setTimeout(() => r(l), 100); })",
                r#"["m1", "t0", "m3"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.onmessage = e => { l.push('m' + e.data); if (e.data === 1) { setTimeout(() => l.push('t0'), 0); c.port2.postMessage(3); c.port2.postMessage(4); } }; c.port2.postMessage(1); setTimeout(() => r(l), 100); })",
                r#"["m1", "t0", "m3", "m4"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const a = new MessageChannel(), b = new MessageChannel(); a.port1.onmessage = e => { l.push('a' + e.data); if (e.data < 3) b.port2.postMessage(e.data); }; b.port1.onmessage = e => { l.push('b' + e.data); a.port2.postMessage(e.data + 1); }; a.port2.postMessage(1); setTimeout(() => r(l), 100); })",
                r#"["a1", "b1", "a2", "b2", "a3"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.onmessage = e => { l.push('m' + e.data); Promise.resolve().then(() => l.push('micro' + e.data)); }; c.port2.postMessage(1); c.port2.postMessage(2); setTimeout(() => r(l), 100); })",
                r#"["m1", "micro1", "m2", "micro2"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port2.postMessage(1); setTimeout(() => l.push('t0'), 0); c.port1.onmessage = e => l.push('m' + e.data); setTimeout(() => l.push('t0b'), 0); setTimeout(() => r(l), 100); })",
                r#"["t0", "m1", "t0b"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.addEventListener('message', e => l.push('m' + e.data)); c.port2.postMessage(1); setTimeout(() => { setTimeout(() => l.push('t0'), 0); c.port1.start(); setTimeout(() => l.push('t0b'), 0); setTimeout(() => r(l), 100); }, 20); })",
                r#"["t0", "m1", "t0b"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.onmessage = e => l.push('m' + e.data); setTimeout(() => { l.push('t1'); c.port2.postMessage(1); }, 0); setTimeout(() => l.push('t2'), 0); setTimeout(() => r(l), 100); })",
                r#"["t1", "t2", "m1"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.onmessage = e => l.push('m'); c.port2.postMessage(1); requestAnimationFrame(() => l.push('raf')); queueMicrotask(() => l.push('qm')); Promise.resolve().then(() => l.push('p1')).then(() => l.push('p2')); setTimeout(() => r(l), 100); })",
                r#"["qm", "p1", "p2", "m", "raf"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.onmessage = e => l.push('port'); const h = e => l.push('win' + e.data); addEventListener('message', h); postMessage(1, '*'); c.port2.postMessage(1); postMessage(2, '*'); setTimeout(() => { removeEventListener('message', h); r(l); }, 100); })",
                r#"["win1", "port", "win2"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.onmessage = e => l.push('port'); const h = e => { l.push('win' + e.data); if (e.data === 1) { c.port2.postMessage(1); postMessage(2, '*'); } }; addEventListener('message', h); postMessage(1, '*'); setTimeout(() => { removeEventListener('message', h); r(l); }, 100); })",
                r#"["win1", "port", "win2"]"#,
            ),
        ],
    },
    Case {
        name: r"clamp",
        html: r"<!DOCTYPE html><html><head><script>
const E = g => { try { const v = g(); return typeof v === 'string' ? v : String(v); } catch (e) { return e.name + ': ' + e.message; } };
const W = ms => new Promise(r => setTimeout(r, ms));
window.log = [];
</script></head><body><p>x</p></body></html>",
        queries: &[
            (
                r"new Promise(r => { const c = new MessageChannel(); let k = 0; c.port1.onmessage = () => { k++; if (k < 1000) c.port2.postMessage(0); }; let t = 0; const tick = () => { t++; if (t < 1000) setTimeout(tick, 0); }; setTimeout(tick, 0); c.port2.postMessage(0); setTimeout(() => r([k, t < 1000]), 100); })",
                r"[1000, true]",
            ),
            (
                r"new Promise(r => { const c = new MessageChannel(); let k = 0; const t0 = performance.now(); c.port1.onmessage = () => { k++; if (k < 1000) c.port2.postMessage(0); else r(performance.now() - t0 < 500); }; c.port2.postMessage(0); })",
                r"true",
            ),
            (
                r"new Promise(r => { const c = new MessageChannel(); let k = 0; c.port1.onmessage = e => c.port1.postMessage(e.data); c.port2.onmessage = e => { k++; if (k < 2000) c.port2.postMessage(e.data); else r(k); }; c.port2.postMessage(0); })",
                r"2000",
            ),
            (
                r"new Promise(r => { const c = new MessageChannel(); let k = 0, stop = false; c.port1.onmessage = e => { if (!stop) c.port2.postMessage(0); }; c.port2.onmessage = e => { k++; if (!stop) c.port1.postMessage(0); }; c.port1.postMessage(0); let ticks = 0, frames = 0; const iv = setInterval(() => ticks++, 10); const raf = () => { frames++; if (!stop) requestAnimationFrame(raf); }; requestAnimationFrame(raf); setTimeout(() => { stop = true; clearInterval(iv); r([k > 1000, ticks >= 5, frames >= 3]); }, 300); })",
                r"[true, true, true]",
            ),
            (
                r"new Promise(r => { const c = new MessageChannel(); let stop = false, k = 0; c.port1.onmessage = () => { k++; if (!stop) c.port2.postMessage(0); }; c.port2.postMessage(0); const order = []; setTimeout(() => order.push('t0'), 0); setTimeout(() => order.push('t5'), 5); setTimeout(() => { stop = true; r([order.join(), k > 100]); }, 100); })",
                r#"["t0,t5", true]"#,
            ),
            (
                r"new Promise(r => { const c = new MessageChannel(); let n = 0; const t0 = performance.now(); c.port1.onmessage = () => { n++; if (n < 100) c.port2.postMessage(0); else r(performance.now() - t0 < 100); }; let d = 0; const nest = () => { if (++d < 10) setTimeout(nest, 0); else c.port2.postMessage(0); }; nest(); })",
                r"true",
            ),
        ],
    },
    Case {
        name: r"close",
        html: r"<!DOCTYPE html><html><head><script>
const E = g => { try { const v = g(); return typeof v === 'string' ? v : String(v); } catch (e) { return e.name + ': ' + e.message; } };
const W = ms => new Promise(r => setTimeout(r, ms));
window.log = [];
</script></head><body><p>x</p></body></html>",
        queries: &[
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.onmessage = e => l.push('m' + e.data); c.port2.postMessage(1); c.port2.close(); c.port2.postMessage(2); setTimeout(() => r(l), 100); })",
                r#"["m1"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.onmessage = e => l.push('m' + e.data); c.port2.postMessage(1); c.port1.close(); setTimeout(() => r(l), 100); })",
                r"[]",
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.onmessage = e => { l.push('m' + e.data); c.port1.close(); }; c.port2.postMessage(1); c.port2.postMessage(2); setTimeout(() => r(l), 100); })",
                r#"["m1"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.onmessage = e => l.push('m' + e.data); c.port2.postMessage(1); c.port2.postMessage(2); setTimeout(() => { c.port2.postMessage(3); c.port2.close(); c.port2.postMessage(4); setTimeout(() => r(l), 50); }, 20); })",
                r#"["m1", "m2", "m3"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.close(); c.port1.close(); l.push(String(c.port1.start()), String(c.port1.close())); c.port2.onmessage = e => l.push('m' + e.data); c.port1.postMessage(1); c.port2.postMessage(2); c.port1.onmessage = e => l.push('late' + e.data); l.push(typeof c.port1.onmessage); setTimeout(() => r(l), 100); })",
                r#"["undefined", "undefined", "function"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port2.postMessage(1); c.port1.close(); c.port1.onmessage = e => l.push('m' + e.data); c.port1.start(); setTimeout(() => r(l), 100); })",
                r"[]",
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port2.postMessage(1); c.port2.close(); setTimeout(() => { c.port1.onmessage = e => l.push('m' + e.data); setTimeout(() => r(l), 50); }, 20); })",
                r#"["m1"]"#,
            ),
            (
                r"E(() => { const c = new MessageChannel(); const d = new MessageChannel(); d.port1.close(); c.port1.postMessage(1, [d.port1]); return 'sent'; })",
                r#""sent""#,
            ),
            (
                r"E(() => { const c = new MessageChannel(); c.port1.close(); c.port1.postMessage(1, [c.port1]); return 'sent'; })",
                r#""sent""#,
            ),
            (
                r"E(() => { const c = new MessageChannel(); c.port1.close(); return structuredClone(c.port1, {transfer: [c.port1]}) instanceof MessagePort; })",
                r#""true""#,
            ),
            (
                r"new Promise(r => { const out = []; const c = new MessageChannel(); c.port1.close(); const b = new ArrayBuffer(8); c.port1.postMessage(1, [b]); out.push('closed+buffer:' + b.byteLength); out.push('closed+uncloneable:' + E(() => c.port1.postMessage(() => 1))); out.push('closed+dup:' + E(() => c.port1.postMessage(1, [b, b]))); const d = new MessageChannel(); c.port1.postMessage(1, [d.port1]); d.port1.postMessage('after'); d.port2.onmessage = e => out.push('closed+port:' + e.data); const n = new MessageChannel(); const t = new MessageChannel(); t.port1.postMessage(0, [n.port1]); const b2 = new ArrayBuffer(8); n.port1.postMessage(1, [b2]); out.push('neutered+buffer:' + b2.byteLength); out.push('neutered+uncloneable:' + E(() => n.port1.postMessage(() => 1))); out.push('neutered+dup:' + E(() => n.port1.postMessage(1, [new ArrayBuffer(1), n.port2, n.port2]))); out.push('neutered+self:' + E(() => n.port1.postMessage(1, [n.port1]))); setTimeout(() => r(out), 50); })",
                r#"["closed+buffer:8", "closed+uncloneable:undefined", "closed+dup:undefined", "neutered+buffer:8", "neutered+uncloneable:undefined", "neutered+dup:undefined", "neutered+self:undefined", "closed+port:after"]"#,
            ),
            (
                r"new Promise(r => { const out = []; const c = new MessageChannel(); c.port1.close(); const b = new ArrayBuffer(8); c.port2.postMessage(1, [b]); out.push('partner closed+buffer:' + b.byteLength); out.push('partner closed+uncloneable:' + E(() => c.port2.postMessage(() => 1))); out.push('partner closed+dup:' + E(() => c.port2.postMessage(1, [b, b]))); const d = new MessageChannel(); const e = new MessageChannel(); d.port1.postMessage(0, [e.port1]); const b2 = new ArrayBuffer(8); const f = new MessageChannel(); const g = new MessageChannel(); f.port1.postMessage('x', [g.port1]); out.push('partner moved+uncloneable:' + E(() => g.port2.postMessage(() => 1))); out.push('partner moved+buffer:' + (g.port2.postMessage(1, [b2]), b2.byteLength)); setTimeout(() => r(out), 50); })",
                r#"["partner closed+buffer:0", "partner closed+uncloneable:DataCloneError: Failed to execute 'postMessage' on 'MessagePort': () => 1 could not be cloned.", "partner closed+dup:DataCloneError: Failed to execute 'postMessage' on 'MessagePort': ArrayBuffer at index 1 is a duplicate of an earlier ArrayBuffer.", "partner moved+uncloneable:DataCloneError: Failed to execute 'postMessage' on 'MessagePort': () => 1 could not be cloned.", "partner moved+buffer:0"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); const d = new MessageChannel(); c.port1.postMessage('x', [d.port1]); d.port1.close(); c.port2.onmessage = e => { e.ports[0].onmessage = f => l.push('m' + f.data); d.port2.postMessage(1); }; setTimeout(() => r(l), 100); })",
                r#"["m1"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port2.close(); c.port1.onmessage = e => l.push('m' + e.data); c.port2.postMessage(1); c.port1.postMessage(2); setTimeout(() => r(l), 50); })",
                r"[]",
            ),
        ],
    },
    Case {
        name: r"transfer",
        html: r"<!DOCTYPE html><html><head><script>
const E = g => { try { const v = g(); return typeof v === 'string' ? v : String(v); } catch (e) { return e.name + ': ' + e.message; } };
const W = ms => new Promise(r => setTimeout(r, ms));
window.log = [];
</script></head><body><p>x</p></body></html>",
        queries: &[
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); const d = new MessageChannel(); c.port2.onmessage = e => { const p = e.ports[0]; l.push([p instanceof MessagePort, p !== d.port1, e.ports.length, e.data].join()); p.onmessage = f => { l.push('got' + f.data); r(l); }; d.port2.postMessage(7); }; c.port1.postMessage('hi', [d.port1]); l.push('sent'); })",
                r#"["sent", "true,true,1,hi", "got7"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); const d = new MessageChannel(); c.port1.postMessage('hi', [d.port1]); l.push(E(() => d.port1.postMessage(1)), E(() => d.port1.start()), E(() => d.port1.close()), E(() => d.port1.postMessage(1, [new ArrayBuffer(1)])), typeof d.port1.onmessage, d.port1 instanceof MessagePort); d.port1.onmessage = e => l.push('neutered got ' + e.data); d.port2.postMessage(2); setTimeout(() => r(l), 100); })",
                r#"["undefined", "undefined", "undefined", "undefined", "object", true]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); const d = new MessageChannel(); d.port2.postMessage('q1'); d.port2.postMessage('q2'); c.port1.postMessage('hi', [d.port1]); d.port2.postMessage('q3'); c.port2.onmessage = e => { e.ports[0].onmessage = f => l.push(f.data); d.port2.postMessage('q4'); setTimeout(() => r(l), 50); }; })",
                r#"["q1", "q2", "q3", "q4"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); const d = new MessageChannel(); d.port1.onmessage = e => l.push('old' + e.data); d.port2.postMessage('q1'); c.port1.postMessage('hi', [d.port1]); c.port2.onmessage = e => { const p = e.ports[0]; p.addEventListener('message', f => l.push('new' + f.data)); d.port2.postMessage('q2'); setTimeout(() => { l.push('before start'); p.start(); setTimeout(() => r(l), 50); }, 50); }; })",
                r#"["before start", "newq1", "newq2"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); const d = new MessageChannel(); d.port1.onmessage = e => l.push('old' + e.data); d.port2.postMessage('q1'); d.port2.postMessage('q2'); c.port1.postMessage('hi', [d.port1]); c.port2.onmessage = e => { e.ports[0].onmessage = f => l.push('new' + f.data); setTimeout(() => r(l), 50); }; })",
                r#"["newq1", "newq2"]"#,
            ),
            (
                r"[E(() => { const c = new MessageChannel(); c.port1.postMessage(1, [c.port2]); return 'sent'; }), E(() => { const c = new MessageChannel(); c.port1.postMessage(1, [c.port1]); return 'sent'; }), E(() => { const c = new MessageChannel(); c.port1.postMessage(1, [c.port2, c.port1]); return 'sent'; }), E(() => { const c = new MessageChannel(); c.port1.postMessage(1, {transfer: [c.port1]}); return 'sent'; }), E(() => { const c = new MessageChannel(); const d = new MessageChannel(); c.port1.postMessage(1, [d.port1, d.port1]); return 'sent'; }), E(() => { const c = new MessageChannel(); const d = new MessageChannel(); c.port1.postMessage(1, [d.port1, d.port2]); return 'sent'; }), E(() => { const c = new MessageChannel(); const d = new MessageChannel(); const b = new ArrayBuffer(1); c.port1.postMessage(1, [b, d.port1, b]); return 'sent'; }), E(() => { const c = new MessageChannel(); const d = new MessageChannel(); c.port1.postMessage(1, [d.port1]); c.port1.postMessage(2, [d.port1]); return 'sent'; })]",
                r#"["sent", "DataCloneError: Failed to execute 'postMessage' on 'MessagePort': Port at index 0 contains the source port.", "DataCloneError: Failed to execute 'postMessage' on 'MessagePort': Port at index 1 contains the source port.", "DataCloneError: Failed to execute 'postMessage' on 'MessagePort': Port at index 0 contains the source port.", "DataCloneError: Failed to execute 'postMessage' on 'MessagePort': Message port at index 1 is a duplicate of an earlier port.", "sent", "DataCloneError: Failed to execute 'postMessage' on 'MessagePort': ArrayBuffer at index 2 is a duplicate of an earlier ArrayBuffer.", "DataCloneError: Failed to execute 'postMessage' on 'MessagePort': Port at index 0 is already neutered."]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.postMessage('self', [c.port2]); setTimeout(() => r(['done']), 50); })",
                r#"["done"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); const d = new MessageChannel(); d.port1.postMessage('a', [c.port2]); d.port2.onmessage = e => { const p = e.ports[0]; p.onmessage = f => r(['via', f.data]); c.port1.postMessage('b'); }; })",
                r#"["via", "b"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const a = new MessageChannel(), b = new MessageChannel(), t = new MessageChannel(); t.port2.postMessage('early'); let hops = 0; const hop = (from, to, port) => { from.postMessage(hops++, [port]); }; a.port1.postMessage(0, [t.port1]); a.port2.onmessage = e => { b.port1.postMessage(1, [e.ports[0]]); }; b.port2.onmessage = e => { e.ports[0].onmessage = f => { l.push(f.data); if (f.data === 'late') r(l); }; t.port2.postMessage('late'); }; })",
                r#"["early", "late"]"#,
            ),
            (
                r"new Promise(r => { const c = new MessageChannel(); const d = new MessageChannel(); const h = e => { removeEventListener('message', h); const p = e.ports[0]; p.onmessage = f => r([e.ports.length, e.source === window, e.origin, p instanceof MessagePort, f.data, c.port1 instanceof MessagePort]); d.port2.postMessage('through'); }; addEventListener('message', h); postMessage('p', '*', [d.port1]); })",
                r#"[1, true, "null", true, "through", true]"#,
            ),
            (
                r"new Promise(r => { const d = new MessageChannel(); const h = e => { removeEventListener('message', h); r([e.ports.length, e.ports[0] instanceof MessagePort, Object.isFrozen(e.ports), e.ports === e.ports]); }; addEventListener('message', h); postMessage('p', {targetOrigin: '*', transfer: [d.port1, d.port2]}); })",
                r"[2, true, true, true]",
            ),
            (
                r"new Promise(r => { const l = []; const d = new MessageChannel(); d.port2.onmessage = e => l.push('m' + e.data); postMessage('p', 'http://other.invalid/', [d.port1]); setTimeout(() => { d.port2.postMessage(1); setTimeout(() => r(l), 50); }, 50); })",
                r"[]",
            ),
            (
                r"[E(() => { const c = new MessageChannel(); const p = structuredClone(c.port1, {transfer: [c.port1]}); return [p instanceof MessagePort, p !== c.port1, E(() => c.port1.postMessage(1))].join(); }), E(() => { const c = new MessageChannel(); const o = structuredClone({p: c.port1, a: [c.port2]}, {transfer: [c.port1, c.port2]}); return [o.p instanceof MessagePort, o.a[0] instanceof MessagePort].join(); }), E(() => { const c = new MessageChannel(); return structuredClone({p: c.port1}, {transfer: [c.port1]}).p instanceof MessagePort; }), E(() => { const c = new MessageChannel(); return structuredClone(1, {transfer: [c.port1]}); }), E(() => { const c = new MessageChannel(); structuredClone(c.port1, {transfer: [c.port1, c.port1]}); }), E(() => { const c = new MessageChannel(); structuredClone(1, {transfer: [c.port1, c.port1]}); }), E(() => { const c = new MessageChannel(); structuredClone(c.port1, {transfer: [c.port2]}); })]",
                r#"["true,true,undefined", "true,true", "true", "1", "DataCloneError: Failed to execute 'structuredClone' on 'Window': Message port at index 1 is a duplicate of an earlier port.", "DataCloneError: Failed to execute 'structuredClone' on 'Window': Message port at index 1 is a duplicate of an earlier port.", "DataCloneError: Failed to execute 'structuredClone' on 'Window': A MessagePort could not be cloned because it was not transferred."]"#,
            ),
            (
                r"new Promise(r => { const c = new MessageChannel(); const p = structuredClone(c.port1, {transfer: [c.port1]}); p.onmessage = e => r([e.data, e.ports.length]); c.port2.postMessage('cl'); })",
                r#"["cl", 0]"#,
            ),
            (
                r"E(() => { const c = new MessageChannel(); const a = structuredClone(c.port1, {transfer: [c.port1]}); return structuredClone(c.port1, {transfer: [c.port1]}); })",
                r#""DataCloneError: Failed to execute 'structuredClone' on 'Window': Port at index 0 is already neutered.""#,
            ),
            (
                r"[E(() => { const c = new MessageChannel(); const d = new MessageChannel(); c.port1.postMessage({p: d.port1}); }), E(() => { const d = new MessageChannel(); postMessage({p: d.port1}, '*'); }), E(() => structuredClone(new MessageChannel().port1)), E(() => structuredClone({a: [new MessageChannel().port2]})), E(() => { const d = new MessageChannel(); history.replaceState({p: d.port1}, ''); }), E(() => { const d = new MessageChannel(); d.port1.postMessage(d.port1); }), E(() => { const d = new MessageChannel(); postMessage(d.port2, '*'); }), E(() => new MessageChannel().port1.postMessage({m: new Map([[1, new MessageChannel().port1]])})), E(() => { const c = new MessageChannel(); c.port1.postMessage(c.port2, [c.port2]); return 'ok'; })]",
                r#"["DataCloneError: Failed to execute 'postMessage' on 'MessagePort': A MessagePort could not be cloned because it was not transferred.", "DataCloneError: Failed to execute 'postMessage' on 'Window': A MessagePort could not be cloned because it was not transferred.", "DataCloneError: Failed to execute 'structuredClone' on 'Window': A MessagePort could not be cloned because it was not transferred.", "DataCloneError: Failed to execute 'structuredClone' on 'Window': A MessagePort could not be cloned because it was not transferred.", "DataCloneError: Failed to execute 'replaceState' on 'History': A MessagePort could not be cloned because it was not transferred.", "DataCloneError: Failed to execute 'postMessage' on 'MessagePort': A MessagePort could not be cloned because it was not transferred.", "DataCloneError: Failed to execute 'postMessage' on 'Window': A MessagePort could not be cloned because it was not transferred.", "DataCloneError: Failed to execute 'postMessage' on 'MessagePort': A MessagePort could not be cloned because it was not transferred.", "ok"]"#,
            ),
            (
                r"E(() => { const c = new MessageChannel(); const d = new MessageChannel(); try { c.port1.postMessage(() => 1, [d.port1]); } catch (e) {} d.port1.postMessage('still'); return 'ok'; })",
                r#""ok""#,
            ),
            (
                r"new Promise(r => { const c = new MessageChannel(); const d = new MessageChannel(); try { c.port1.postMessage(() => 1, [d.port1]); } catch (e) {} d.port1.onmessage = e => r(['alive', e.data]); d.port2.postMessage('x'); })",
                r#"["alive", "x"]"#,
            ),
            (
                r"[E(() => new MessageChannel().port1.postMessage(1, [new MessageChannel()])), E(() => new MessageChannel().port1.postMessage(1, [new Blob([])])), E(() => new MessageChannel().port1.postMessage(1, [new Uint8Array(1)])), E(() => structuredClone(1, {transfer: [new MessageChannel()]})), E(() => postMessage(1, '*', [{}]))]",
                r#"["DataCloneError: Failed to execute 'postMessage' on 'MessagePort': Value at index 0 does not have a transferable type.", "DataCloneError: Failed to execute 'postMessage' on 'MessagePort': Value at index 0 does not have a transferable type.", "DataCloneError: Failed to execute 'postMessage' on 'MessagePort': Value at index 0 does not have a transferable type.", "DataCloneError: Failed to execute 'structuredClone' on 'Window': Value at index 0 does not have a transferable type.", "DataCloneError: Failed to execute 'postMessage' on 'Window': Value at index 0 does not have a transferable type."]"#,
            ),
            (
                r"new Promise(r => { const c = new MessageChannel(); const d = new MessageChannel(); const o = {a: d.port1, b: [d.port1, {c: d.port1}]}; c.port1.postMessage(o, [d.port1]); c.port2.onmessage = e => r([e.data.a === e.data.b[0], e.data.a === e.data.b[1].c, e.ports.length, e.ports[0] === e.data.a, e.data.a instanceof MessagePort]); })",
                r"[true, true, 1, true, true]",
            ),
            (
                r"new Promise(r => { const c = new MessageChannel(); const d = new MessageChannel(), e2 = new MessageChannel(); c.port1.postMessage({x: e2.port1, y: d.port1}, [d.port1, e2.port1]); c.port2.onmessage = e => { e.ports[0].onmessage = f => r(['first is d', f.data]); e.ports[1].onmessage = f => r(['first is e2', f.data]); d.port2.postMessage('from d'); }; })",
                r#"["first is d", "from d"]"#,
            ),
            (
                r"new Promise(r => { const c = new MessageChannel(); const d = new MessageChannel(); const l = []; c.port2.addEventListener('message', e => l.push(e.ports[0])); c.port2.addEventListener('message', e => l.push(e.ports[0])); c.port2.start(); c.port1.postMessage(1, [d.port1]); setTimeout(() => r([l.length, l[0] === l[1]]), 50); })",
                r"[2, true]",
            ),
        ],
    },
    Case {
        name: r"event",
        html: r"<!DOCTYPE html><html><head><script>
const E = g => { try { const v = g(); return typeof v === 'string' ? v : String(v); } catch (e) { return e.name + ': ' + e.message; } };
const W = ms => new Promise(r => setTimeout(r, ms));
window.log = [];
</script></head><body><p>x</p></body></html>",
        queries: &[
            (
                r"new Promise(r => { const c = new MessageChannel(); const d = new MessageChannel(); c.port1.onmessage = e => r([e.origin === '', e.source === null, e.ports.length, e.isTrusted, e.bubbles, e.cancelable, e.lastEventId === '', e.data, e.ports[0] instanceof MessagePort, e.ports === e.ports, Object.isFrozen(e.ports), Array.isArray(e.ports), Object.getPrototypeOf(e.ports) === Array.prototype, E(() => e.ports.push(1)), E(() => { 'use strict'; e.ports[0] = 1; }), E(() => { 'use strict'; e.ports.length = 0; }), E(() => { 'use strict'; e.ports = []; return 'set'; })]); c.port2.postMessage('d', [d.port1]); })",
                r#"[true, true, 1, true, false, false, true, "d", true, true, true, true, true, "TypeError: Cannot add property 1, object is not extensible", "TypeError: Cannot assign to read only property '0' of object '[object Array]'", "TypeError: Cannot assign to read only property 'length' of object '[object Array]'", "TypeError: Cannot set property ports of #<MessageEvent> which has only a getter"]"#,
            ),
            (
                r"new Promise(r => { const c = new MessageChannel(); c.port1.onmessage = e => r([e.ports === e.ports, e.ports === e.ports, Object.isFrozen(e.ports), e.ports.length]); c.port2.postMessage(1); })",
                r"[true, true, true, 0]",
            ),
            (
                r"[E(() => { const e = new MessageEvent('message'); return [e.ports === e.ports, Object.isFrozen(e.ports), e.ports.length, Array.isArray(e.ports)].join(); }), E(() => { const c = new MessageChannel(); const a = [c.port1, c.port2]; const e = new MessageEvent('message', {ports: a}); a.length = 0; return [e.ports === e.ports, e.ports === a, e.ports.length, Object.isFrozen(e.ports), e.ports[0] === c.port1, e.ports[1] === c.port2, Array.isArray(e.ports)].join(); }), E(() => { const c = new MessageChannel(); const a = [c.port1]; const e = new MessageEvent('message', {ports: a}); const f = new MessageEvent('message', {ports: a}); return [e.ports === f.ports, e.ports[0] === f.ports[0]].join(); }), E(() => new MessageEvent('message', {ports: [1]})), E(() => new MessageEvent('message', {ports: [null]})), E(() => new MessageEvent('message', {ports: [{}]})), E(() => new MessageEvent('message', {ports: 5})), E(() => new MessageEvent('message', {ports: null}).ports.length), E(() => new MessageEvent('message', {ports: undefined}).ports.length), E(() => new MessageEvent('message', {ports: [new MessageChannel().port1, new MessageChannel().port1]}).ports.length), E(() => new MessageEvent('message', {ports: [new EventTarget()]})), E(() => { const c = new MessageChannel(); return new MessageEvent('message', {ports: [c.port1, c.port1]}).ports.length; }), E(() => new MessageEvent('message', {ports: new Set([new MessageChannel().port1])}).ports.length)]",
                r#"["true,true,0,true", "true,false,2,true,true,true,true", "false,true", "TypeError: Failed to construct 'MessageEvent': Failed to read the 'ports' property from 'MessageEventInit': Failed to convert value to 'MessagePort'.", "TypeError: Failed to construct 'MessageEvent': Failed to read the 'ports' property from 'MessageEventInit': Failed to convert value to 'MessagePort'.", "TypeError: Failed to construct 'MessageEvent': Failed to read the 'ports' property from 'MessageEventInit': Failed to convert value to 'MessagePort'.", "TypeError: Failed to construct 'MessageEvent': Failed to read the 'ports' property from 'MessageEventInit': The provided value cannot be converted to a sequence.", "TypeError: Failed to construct 'MessageEvent': Failed to read the 'ports' property from 'MessageEventInit': The provided value cannot be converted to a sequence.", "0", "2", "TypeError: Failed to construct 'MessageEvent': Failed to read the 'ports' property from 'MessageEventInit': Failed to convert value to 'MessagePort'.", "2", "1"]"#,
            ),
            (
                r"[E(() => { const c = new MessageChannel(); const e = new MessageEvent('message', {source: c.port1}); return e.source === c.port1; }), E(() => new MessageEvent('message', {source: window}).source === window), E(() => new MessageEvent('message', {source: {}})), E(() => new MessageEvent('message', {source: document})), E(() => new MessageEvent('message', {source: null}).source), E(() => new MessageEvent('message', {source: 5})), E(() => new MessageEvent('message', {source: new EventTarget()})), E(() => { const c = new MessageChannel(); const e = document.createEvent('MessageEvent'); e.initMessageEvent('message', false, false, 1, 'o', 'l', c.port1, [c.port2]); return [e.source === c.port1, e.ports.length, e.ports === e.ports, e.ports[0] === c.port2].join(); }), E(() => { const e = document.createEvent('MessageEvent'); e.initMessageEvent('message', false, false, 1, 'o', 'l', null, [1]); }), E(() => { const e = document.createEvent('MessageEvent'); e.initMessageEvent('message', false, false, 1, 'o', 'l', {}); }), E(() => { const c = new MessageChannel(); const e = new MessageEvent('message', {ports: [c.port2]}); const a = e.ports; e.initMessageEvent('message', false, false, 1, 'o', 'l', null, []); return [a === e.ports, e.ports.length, a.length].join(); })]",
                r#"["true", "true", "TypeError: Failed to construct 'MessageEvent': Failed to read the 'source' property from 'MessageEventInit': Failed to convert value to 'EventTarget'.", "TypeError: Failed to construct 'MessageEvent': The optional 'source' property is neither a Window nor MessagePort.", "null", "TypeError: Failed to construct 'MessageEvent': Failed to read the 'source' property from 'MessageEventInit': Failed to convert value to 'EventTarget'.", "TypeError: Failed to construct 'MessageEvent': The optional 'source' property is neither a Window nor MessagePort.", "true,1,true,true", "TypeError: Failed to execute 'initMessageEvent' on 'MessageEvent': Failed to convert value to 'MessagePort'.", "TypeError: Failed to execute 'initMessageEvent' on 'MessageEvent': parameter 7 is not of type 'EventTarget'.", "false,0,1"]"#,
            ),
            (
                r"new Promise(r => { const c = new MessageChannel(); const d = new MessageChannel(); c.port1.addEventListener('message', e => r([e.data, e.ports.length, e.ports[0] === d.port1, e.isTrusted, e.source === c.port2])); c.port1.dispatchEvent(new MessageEvent('message', {data: 'hand', ports: [d.port1], source: c.port2})); })",
                r#"["hand", 1, true, false, true]"#,
            ),
            (
                r"[E(() => typeof MessageEvent), E(() => 'onmessageerror' in window), E(() => 'onmessage' in window), E(() => { const c = new MessageChannel(); return [c.port1.onmessageerror === null, 'onmessageerror' in MessagePort.prototype].join(); })]",
                r#"["function", "true", "true", "true,true"]"#,
            ),
            (
                r"Object.getOwnPropertyNames(MessageEvent.prototype).join()",
                r#""data,origin,lastEventId,source,ports,userActivation,initMessageEvent,constructor""#,
            ),
        ],
    },
    Case {
        name: r"from-frames",
        html: r"<!DOCTYPE html><html><head><script>
const E = g => { try { const v = g(); return typeof v === 'string' ? v : String(v); } catch (e) { return e.name + ': ' + e.message; } };
const W = ms => new Promise(r => setTimeout(r, ms));
window.log = [];
</script></head><body><p>x</p></body></html>",
        queries: &[
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.onmessage = e => l.push('m' + e.data); setTimeout(() => l.push('t0'), 0); c.port2.postMessage(1); Promise.resolve().then(() => l.push('micro')); c.port2.postMessage(2); addEventListener('message', function h(e) { removeEventListener('message', h); l.push('win'); }); postMessage('w', '*'); c.port2.postMessage(3); l.push('sync'); setTimeout(() => r(l), 100); })",
                r#"["sync", "micro", "t0", "m1", "win", "m2", "m3"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.addEventListener('message', e => l.push('a' + e.data)); c.port2.postMessage(1); setTimeout(() => { l.push('before start'); c.port1.start(); setTimeout(() => r(l), 50); }, 50); })",
                r#"["before start", "a1"]"#,
            ),
            (
                r"[Object.getOwnPropertyNames(MessagePort.prototype).join(), Object.getOwnPropertyNames(MessageChannel.prototype).join(), MessageChannel.length, typeof MessagePort, E(() => new MessagePort()), E(() => MessageChannel()), Object.getPrototypeOf(MessagePort.prototype) === EventTarget.prototype, E(() => { const c = new MessageChannel(); c.port1.postMessage(1, [c.port1]); }), E(() => { const c = new MessageChannel(); const d = new MessageChannel(); c.port1.postMessage(1, [d.port1, d.port1]); }), E(() => { const c = new MessageChannel(); c.port1.close(); c.port1.postMessage(1); return 'ok'; }), E(() => structuredClone(new MessageChannel().port1)), E(() => { const c = new MessageChannel(); return structuredClone(c.port1, {transfer: [c.port1]}) instanceof MessagePort; }), E(() => { const c = new MessageChannel(); c.port1.postMessage(1, {transfer: []}); return 'opts'; }), String(new MessageChannel().port1.onmessage)]",
                r#"["onmessage,onmessageerror,close,postMessage,start,constructor", "port1,port2,constructor", 0, "function", "TypeError: Failed to construct 'MessagePort': Illegal constructor", "TypeError: Failed to construct 'MessageChannel': Please use the 'new' operator, this DOM object constructor cannot be called as a function.", true, "DataCloneError: Failed to execute 'postMessage' on 'MessagePort': Port at index 0 contains the source port.", "DataCloneError: Failed to execute 'postMessage' on 'MessagePort': Message port at index 1 is a duplicate of an earlier port.", "ok", "DataCloneError: Failed to execute 'structuredClone' on 'Window': A MessagePort could not be cloned because it was not transferred.", "true", "opts", "null"]"#,
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); let k = 0; c.port1.onmessage = () => { k++; if (k < 1000) c.port2.postMessage(0); }; let t = 0; const tick = () => { t++; if (t < 1000) setTimeout(tick, 0); }; setTimeout(tick, 0); c.port2.postMessage(0); setTimeout(() => r([k, t]), 50); })",
                r"[1000, 19]",
            ),
            (
                r"new Promise(r => { const l = []; const c = new MessageChannel(); c.port1.onmessage = e => l.push('m' + e.data); c.port1.onclose = () => l.push('close'); c.port2.postMessage(1); c.port2.close(); setTimeout(() => r(l), 100); })",
                r#"["m1"]"#,
            ),
            (r"typeof BroadcastChannel", r#""function""#),
        ],
    },
];
