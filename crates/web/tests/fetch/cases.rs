// The cases of tools/probes/web-fetch.json with the values of Chromium 148.
// Generated from `swbtools probe --json` (three runs, all equal); do not edit.

use crate::common::Case;

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "headers",
        html: r"<!DOCTYPE html><p>x</p>",
        queries: &[
            (
                r"(() => { const h = new Headers([['B','2'],['a','1'],['b','3'],['Set-Cookie','x=1'],['set-cookie','y=2']]); return [[...h], h.get('b'), h.get('set-cookie'), h.getSetCookie(), h.has('A'), Object.prototype.toString.call(h), Object.keys(Headers.prototype), Object.getOwnPropertyNames(Headers.prototype).join()]; })()",
                r#"[[["a", "1"], ["b", "2, 3"], ["set-cookie", "x=1"], ["set-cookie", "y=2"]], "2, 3", "x=1, y=2", ["x=1", "y=2"], true, "[object Headers]", ["append", "delete", "get", "getSetCookie", "has", "set", "entries", "forEach", "keys", "values"], "append,delete,get,getSetCookie,has,set,entries,forEach,keys,values,constructor"]"#,
            ),
            (
                r"(() => { const h = new Headers({'X-A': ' 1 ', 'x-b': '2'}); h.append('x-a', '3'); h.set('X-B', 'new'); h.delete('nothing'); return [[...h.entries()], [...h.keys()], [...h.values()], h.get('x-a'), h.get('zz')]; })()",
                r#"[[["x-a", "1, 3"], ["x-b", "new"]], ["x-a", "x-b"], ["1, 3", "new"], "1, 3", null]"#,
            ),
            (
                r"(() => { const out = []; for (const f of [() => new Headers({'a b': '1'}), () => new Headers([['a']]), () => new Headers(5), () => new Headers(null), () => new Headers([['a', 'b', 'c']]), () => new Headers().append('a', 'x\ny'), () => new Headers().get('a b'), () => new Headers().set('', 'x'), () => new Headers([1]), () => Headers.prototype.get.call({}, 'a')]) { try { f(); out.push('ok'); } catch (e) { out.push(e.name + ': ' + e.message); } } return out; })()",
                r#"["TypeError: Failed to construct 'Headers': Invalid name", "TypeError: Failed to construct 'Headers': Invalid value", "TypeError: Failed to construct 'Headers': The provided value is not of type '(record<ByteString, ByteString> or sequence<sequence<ByteString>>)'.", "TypeError: Failed to construct 'Headers': The provided value is not of type '(record<ByteString, ByteString> or sequence<sequence<ByteString>>)'.", "TypeError: Failed to construct 'Headers': Invalid value", "TypeError: Failed to execute 'append' on 'Headers': Invalid value", "TypeError: Failed to execute 'get' on 'Headers': Invalid name", "TypeError: Failed to execute 'set' on 'Headers': Invalid name", "TypeError: Failed to construct 'Headers': The provided value cannot be converted to a sequence.", "TypeError: Illegal invocation"]"#,
            ),
            (
                r"(() => { const h = new Headers(); h.append('Content-Type', 'a'); h.append('content-type', 'b'); const s = []; h.forEach((v, k, o) => s.push(k + '=' + v + ':' + (o === h))); return [s, new Headers(h).get('content-type'), [...new Headers({b: '1', a: '2'})]]; })()",
                r#"[["content-type=a, b:true"], "a, b", [["a", "2"], ["b", "1"]]]"#,
            ),
        ],
    },
    Case {
        name: "request",
        html: r"<!DOCTYPE html><p>x</p>",
        queries: &[
            (
                r"(() => { const r = new Request('/a/b?q=1#frag'); return [r.url === new URL('/a/b?q=1#frag', location.href).href, r.method, r.mode, r.credentials, r.cache, r.redirect, r.referrer, r.referrerPolicy, r.destination, r.integrity, r.keepalive, r.bodyUsed, r.body, r.signal.aborted, r.headers instanceof Headers, Object.prototype.toString.call(r)]; })()",
                r#"[true, "GET", "cors", "same-origin", "default", "follow", "about:client", "", "", "", false, false, null, false, true, "[object Request]"]"#,
            ),
            (
                r"(() => { const out = []; for (const f of [() => new Request(), () => new Request('http://u:p@x.test/'), () => new Request('http://[bad'), () => new Request('/', {method: 'TRACE'}), () => new Request('/', {method: 'a b'}), () => new Request('/', {method: 'GET', body: 'x'}), () => new Request('/', {mode: 'navigate'}), () => new Request('/', {mode: 'no-cors', method: 'PUT'}), () => new Request('/', {cache: 'only-if-cached'}), () => new Request('/', {credentials: 'x'}), () => new Request('/', {headers: 5}), () => new Request('/', {method: 'post', body: 'x'}).method, () => new Request('/', {method: 'patch'}).method, () => new Request('/', {method: 'PATCH'}).method]) { try { const v = f(); out.push(v === undefined || typeof v === 'object' ? 'ok' : v); } catch (e) { out.push(e.name + ': ' + e.message); } } return out; })()",
                r#"["TypeError: Failed to construct 'Request': 1 argument required, but only 0 present.", "TypeError: Failed to construct 'Request': Request cannot be constructed from a URL that includes credentials: http://u:p@x.test/", "TypeError: Failed to construct 'Request': Failed to parse URL from http://[bad", "TypeError: Failed to construct 'Request': 'TRACE' HTTP method is unsupported.", "TypeError: Failed to construct 'Request': 'a b' is not a valid HTTP method.", "TypeError: Failed to construct 'Request': Request with GET/HEAD method cannot have body.", "TypeError: Failed to construct 'Request': Cannot construct a Request with a RequestInit whose mode member is set as 'navigate'.", "TypeError: Failed to construct 'Request': 'PUT' is unsupported in no-cors mode.", "TypeError: Failed to construct 'Request': 'only-if-cached' can be set only with 'same-origin' mode", "TypeError: Failed to construct 'Request': Failed to read the 'credentials' property from 'RequestInit': The provided value 'x' is not a valid enum value of type RequestCredentials.", "TypeError: Failed to construct 'Request': Failed to read the 'headers' property from 'RequestInit': The provided value is not of type '(record<ByteString, ByteString> or sequence<sequence<ByteString>>)'.", "POST", "patch", "PATCH"]"#,
            ),
            (
                r"(async () => { const r = new Request('/', {method: 'POST', body: 'hello', headers: {'X-A': '1', 'Host': 'evil', 'Content-Type': 'text/x'}}); const c = r.clone(); const t = await r.text(); let again; try { await r.text(); } catch (e) { again = e.name + ': ' + e.message; } return [t, r.bodyUsed, again, await c.text(), [...r.headers], c.bodyUsed]; })()",
                r#"["hello", true, "TypeError: Failed to execute 'text' on 'Request': body stream already read", "hello", [["content-type", "text/x"], ["x-a", "1"]], true]"#,
            ),
            (
                r"(async () => { const bodies = [new URLSearchParams('a=1&b=2'), new Uint8Array([104, 105]), new Uint8Array([104, 105]).buffer, 'str', 5, {}, null, undefined]; const out = []; for (const b of bodies) { const r = new Request('/', {method: 'POST', body: b}); out.push([r.headers.get('content-type'), r.body === null, await r.text()]); } return out; })()",
                r#"[["application/x-www-form-urlencoded;charset=UTF-8", false, "a=1&b=2"], [null, false, "hi"], [null, false, "hi"], ["text/plain;charset=UTF-8", false, "str"], ["text/plain;charset=UTF-8", false, "5"], ["text/plain;charset=UTF-8", false, "[object Object]"], [null, true, ""], [null, true, ""]]"#,
            ),
            (
                r"(async () => { const a = new Request('/x', {method: 'POST', body: 'b'}); const b = new Request(a, {method: 'PUT'}); let used; try { new Request(a); } catch (e) { used = e.message; } return [a.bodyUsed, b.method, await b.text(), used]; })()",
                r#"[true, "PUT", "b", "Failed to construct 'Request': Cannot construct a Request with a Request object that has already been used."]"#,
            ),
            (
                r"(() => { const c = new AbortController(); const r = new Request('/', {signal: c.signal}); const before = r.signal.aborted; c.abort(); return [before, r.signal.aborted, r.signal === c.signal, r.signal.reason.name]; })()",
                r#"[false, true, false, "AbortError"]"#,
            ),
        ],
    },
    Case {
        name: "response",
        html: r"<!DOCTYPE html><p>x</p>",
        queries: &[
            (
                r"(() => { const r = new Response('hi', {status: 201, statusText: 'Made', headers: {'X-A': '1'}}); return [r.status, r.statusText, r.ok, r.type, r.url, r.redirected, r.bodyUsed, r.body === null, r.headers.get('content-type'), r.headers.get('x-a'), Object.prototype.toString.call(r)]; })()",
                r#"[201, "Made", true, "default", "", false, false, false, "text/plain;charset=UTF-8", "1", "[object Response]"]"#,
            ),
            (
                r"(() => { const out = []; for (const f of [() => new Response('x', {status: 99}), () => new Response('x', {status: 600}), () => new Response('x', {status: 204}), () => new Response(null, {status: 204}), () => new Response('x', {statusText: 'a\nb'}), () => new Response(undefined, {status: 'a'}), () => Response.redirect('/a', 200), () => Response.redirect('http://[bad'), () => Response.json(undefined), () => Response.json(1n), () => new Response('x', {headers: 5})]) { try { f(); out.push('ok'); } catch (e) { out.push(e.name + ': ' + e.message); } } return out; })()",
                r#"["RangeError: Failed to construct 'Response': The status provided (99) is outside the range [200, 599].", "RangeError: Failed to construct 'Response': The status provided (600) is outside the range [200, 599].", "TypeError: Failed to construct 'Response': Response with null body status cannot have body", "ok", "TypeError: Failed to construct 'Response': Invalid statusText", "RangeError: Failed to construct 'Response': The status provided (0) is outside the range [200, 599].", "RangeError: Failed to execute 'redirect' on 'Response': Invalid status code", "TypeError: Failed to execute 'redirect' on 'Response': Failed to parse URL from http://[bad", "TypeError: Failed to execute 'json' on 'Response': The data is not JSON serializable", "TypeError: Failed to execute 'json' on 'Response': Do not know how to serialize a BigInt", "TypeError: Failed to construct 'Response': Failed to read the 'headers' property from 'ResponseInit': The provided value is not of type '(record<ByteString, ByteString> or sequence<sequence<ByteString>>)'."]"#,
            ),
            (
                r"(async () => { const e = Response.error(); const rd = Response.redirect('/there', 301); const j = Response.json({a: [1, 2]}, {status: 202}); let imm; try { e.headers.set('a', 'b'); } catch (x) { imm = x.name + ': ' + x.message; } return [e.type, e.status, e.ok, e.statusText, [...e.headers], imm, rd.status, rd.headers.get('location') === new URL('/there', location.href).href, rd.type, j.status, j.headers.get('content-type'), await j.text()]; })()",
                r#"["error", 0, false, "", [], "TypeError: Failed to execute 'set' on 'Headers': Headers are immutable", 301, true, "default", 202, "application/json", "{\"a\":[1,2]}"]"#,
            ),
            (
                r#"(async () => { const r = new Response('{"a":1}'); const c = r.clone(); const j = await r.json(); let again; try { await r.text(); } catch (e) { again = e.name + ': ' + e.message; } let bad; try { await new Response('{x').json(); } catch (e) { bad = e.name; } const ab = await c.arrayBuffer(); const by = await new Response('hi').bytes(); return [j, again, bad, ab.byteLength, Object.prototype.toString.call(ab), Object.prototype.toString.call(by), by.length, r.bodyUsed, c.bodyUsed]; })()"#,
                r#"[{"a": 1}, "TypeError: Failed to execute 'text' on 'Response': body stream already read", "SyntaxError", 7, "[object ArrayBuffer]", "[object Uint8Array]", 2, true, true]"#,
            ),
            (
                r"(async () => { const out = []; for (const m of ['blob', 'formData']) { out.push(typeof Response.prototype[m]); } return [out, Object.getOwnPropertyNames(Response.prototype).join(), Object.getOwnPropertyNames(Response).join(), Object.getOwnPropertyNames(Request.prototype).join()]; })()",
                r#"[["function", "function"], "type,url,redirected,status,ok,statusText,headers,body,bodyUsed,arrayBuffer,blob,clone,formData,json,text,bytes,constructor", "length,name,prototype,error,json,redirect", "method,url,headers,destination,referrer,referrerPolicy,mode,credentials,cache,redirect,integrity,keepalive,signal,duplex,isHistoryNavigation,bodyUsed,arrayBuffer,blob,clone,formData,json,text,targetAddressSpace,body,bytes,constructor"]"#,
            ),
            (
                r"(async () => { const r = new Response('a\u00e9\ud83d\ude00'); const t = await r.text(); const u = await new Response(new Uint8Array([0xef, 0xbb, 0xbf, 0x68, 0xff])).text(); return [t, u.length, u.charCodeAt(u.length - 1)]; })()",
                r#"["aé😀", 2, 65533]"#,
            ),
        ],
    },
    Case {
        name: "abort",
        html: r"<!DOCTYPE html><p>x</p>",
        queries: &[
            (
                r"(() => { const c = new AbortController(); const s = c.signal; const log = []; s.addEventListener('abort', e => log.push(['l1', e.type, e.target === s, e.isTrusted, e.bubbles, e.cancelable])); s.onabort = e => log.push('on'); const before = [s.aborted, s.reason]; c.abort(); c.abort('again'); return [before, s.aborted, s.reason.name, s.reason.message, s.reason.code, log, Object.prototype.toString.call(s), Object.prototype.toString.call(c), s === c.signal]; })()",
                r#"[[false, null], true, "AbortError", "signal is aborted without reason", 20, [["l1", "abort", true, true, false, false], "on"], "[object AbortSignal]", "[object AbortController]", true]"#,
            ),
            (
                r"(() => { const c = new AbortController(); c.abort('why'); const s = AbortSignal.abort(); const t = AbortSignal.abort(42); let thrown; try { c.signal.throwIfAborted(); } catch (e) { thrown = e; } let none; try { new AbortController().signal.throwIfAborted(); none = 'no throw'; } catch (e) { none = 'threw'; } return [c.signal.reason, s.reason.name, t.reason, thrown, none, Object.getOwnPropertyNames(AbortSignal).join(), Object.getOwnPropertyNames(AbortSignal.prototype).join(), Object.getOwnPropertyNames(AbortController.prototype).join()]; })()",
                r#"["why", "AbortError", 42, "why", "no throw", "length,name,prototype,abort,any,timeout", "aborted,reason,onabort,throwIfAborted,constructor", "signal,abort,constructor"]"#,
            ),
            (
                r"(() => { const out = []; for (const f of [() => new AbortSignal(), () => AbortSignal.any(5), () => AbortSignal.any([1]), () => AbortSignal.timeout(-1), () => AbortSignal.timeout('x'), () => AbortSignal.any([]).aborted, () => AbortSignal.abort().aborted]) { try { const v = f(); out.push(typeof v === 'boolean' ? v : 'ok'); } catch (e) { out.push(e.name + ': ' + e.message); } } return out; })()",
                r#"["TypeError: Failed to construct 'AbortSignal': Illegal constructor", "TypeError: Failed to execute 'any' on 'AbortSignal': The provided value cannot be converted to a sequence.", "TypeError: Failed to execute 'any' on 'AbortSignal': Failed to convert value to 'AbortSignal'.", "TypeError: Failed to execute 'timeout' on 'AbortSignal': Value is outside the 'unsigned long long' value range.", "TypeError: Failed to execute 'timeout' on 'AbortSignal': Value is not of type 'unsigned long long'.", false, true]"#,
            ),
            (
                r"(async () => { const s = AbortSignal.timeout(50); const before = s.aborted; await new Promise(r => setTimeout(r, 120)); return [before, s.aborted, s.reason.name, s.reason.message, s.reason.code]; })()",
                r#"[false, true, "TimeoutError", "signal timed out", 23]"#,
            ),
            (
                r"(() => { const a = new AbortController(), b = new AbortController(); const any = AbortSignal.any([a.signal, b.signal]); const nested = AbortSignal.any([any]); const log = []; any.addEventListener('abort', () => log.push('any')); nested.addEventListener('abort', () => log.push('nested')); a.signal.addEventListener('abort', () => log.push('a')); b.abort('B'); a.abort('A'); return [any.aborted, any.reason, nested.aborted, nested.reason, log, a.signal.reason, AbortSignal.any([AbortSignal.abort('early'), a.signal]).reason]; })()",
                r#"[true, "B", true, "B", ["any", "nested", "a"], "A", "early"]"#,
            ),
            (
                r"(() => { const c = new AbortController(); const log = []; const f = () => log.push('f'); const g = () => log.push('g'); const t = new EventTarget(); t.addEventListener('x', f, {signal: c.signal}); t.addEventListener('x', g); t.dispatchEvent(new Event('x')); c.abort(); t.dispatchEvent(new Event('x')); const done = new AbortController(); done.abort(); t.addEventListener('x', () => log.push('late'), {signal: done.signal}); t.dispatchEvent(new Event('x')); let bad; try { t.addEventListener('x', f, {signal: 5}); } catch (e) { bad = e.name + ': ' + e.message; } try { t.addEventListener('x', f, {signal: null}); } catch (e) { bad += ' | ' + e.message; } return [log, bad]; })()",
                r#"[["f", "g", "g", "g"], "TypeError: Failed to execute 'addEventListener' on 'EventTarget': Failed to read the 'signal' property from 'AddEventListenerOptions': Failed to convert value to 'AbortSignal'. | Failed to execute 'addEventListener' on 'EventTarget': Failed to read the 'signal' property from 'AddEventListenerOptions': Failed to convert value to 'AbortSignal'."]"#,
            ),
            (
                r"(() => { const c = new AbortController(); const log = []; const w = () => log.push('w'); document.addEventListener('click', w, {signal: c.signal, once: true, capture: true}); document.dispatchEvent(new Event('click')); document.dispatchEvent(new Event('click')); document.addEventListener('click', w, {signal: c.signal}); c.abort(); document.dispatchEvent(new Event('click')); return log; })()",
                r#"["w"]"#,
            ),
        ],
    },
];
