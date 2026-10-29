// The cases of tools/probes/web-crypto-encoding.json with the values of Chromium 148.
// Generated from `just probe --json` (three runs, all equal); do not edit.

use crate::common::Case;

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "crypto",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function U(a) { return Array.from(a).join(); }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
</script></body></html>",
        queries: &[
            (
                r"[typeof crypto, Object.prototype.toString.call(crypto), Object.getOwnPropertyNames(Crypto.prototype).filter((n) => n !== 'subtle').sort().join(), crypto === crypto, window.crypto === crypto, Object.getOwnPropertyDescriptor(window, 'crypto').enumerable, typeof Object.getOwnPropertyDescriptor(window, 'crypto').set, Crypto.length]",
                r#"["object", "[object Crypto]", "constructor,getRandomValues,randomUUID", true, true, true, "undefined", 0]"#,
            ),
            (
                r"E(() => new Crypto())",
                r#""TypeError: Failed to construct 'Crypto': Illegal constructor""#,
            ),
            (
                r"E(() => crypto.getRandomValues())",
                r#""TypeError: Failed to execute 'getRandomValues' on 'Crypto': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => crypto.getRandomValues(5))",
                r#""TypeError: Failed to execute 'getRandomValues' on 'Crypto': parameter 1 is not of type 'ArrayBufferView'.""#,
            ),
            (
                r"E(() => crypto.getRandomValues([1, 2]))",
                r#""TypeError: Failed to execute 'getRandomValues' on 'Crypto': parameter 1 is not of type 'ArrayBufferView'.""#,
            ),
            (
                r"E(() => crypto.getRandomValues(new Float32Array(2)))",
                r#""DOMException TypeMismatchError: Failed to execute 'getRandomValues' on 'Crypto': The provided ArrayBufferView is of type 'Float32', which is not an integer array type.""#,
            ),
            (
                r"E(() => crypto.getRandomValues(new Float64Array(2)))",
                r#""DOMException TypeMismatchError: Failed to execute 'getRandomValues' on 'Crypto': The provided ArrayBufferView is of type 'Float64', which is not an integer array type.""#,
            ),
            (
                r"E(() => crypto.getRandomValues(new DataView(new ArrayBuffer(8))))",
                r#""DOMException TypeMismatchError: Failed to execute 'getRandomValues' on 'Crypto': The provided ArrayBufferView is of type 'DataView', which is not an integer array type.""#,
            ),
            (
                r"E(() => crypto.getRandomValues(new ArrayBuffer(8)))",
                r#""TypeError: Failed to execute 'getRandomValues' on 'Crypto': parameter 1 is not of type 'ArrayBufferView'.""#,
            ),
            (
                r"E(() => crypto.getRandomValues(new Uint8Array(65537)))",
                r#""DOMException QuotaExceededError: Failed to execute 'getRandomValues' on 'Crypto': The ArrayBufferView's byte length (65537) exceeds the number of bytes of entropy available via this API (65536).""#,
            ),
            (
                r"E(() => crypto.getRandomValues(new Uint8Array(65536)).length)",
                r"65536",
            ),
            (
                r"E(() => crypto.getRandomValues(new Uint32Array(16385)))",
                r#""DOMException QuotaExceededError: Failed to execute 'getRandomValues' on 'Crypto': The ArrayBufferView's byte length (65540) exceeds the number of bytes of entropy available via this API (65536).""#,
            ),
            (
                r"E(() => crypto.getRandomValues(new Uint32Array(16384)).length)",
                r"16384",
            ),
            (
                r"E(() => crypto.getRandomValues(new BigUint64Array(8192)).length)",
                r"8192",
            ),
            (
                r"E(() => crypto.getRandomValues(new BigUint64Array(8193)))",
                r#""DOMException QuotaExceededError: Failed to execute 'getRandomValues' on 'Crypto': The ArrayBufferView's byte length (65544) exceeds the number of bytes of entropy available via this API (65536).""#,
            ),
            (
                r"E(() => crypto.getRandomValues(new Uint8Array(0)).length)",
                r"0",
            ),
            (
                r"E(() => [crypto.getRandomValues(new Uint8ClampedArray(4)).length, crypto.getRandomValues(new Int8Array(4)).length, crypto.getRandomValues(new Int16Array(4)).length, crypto.getRandomValues(new Uint16Array(4)).length, crypto.getRandomValues(new Int32Array(4)).length, crypto.getRandomValues(new BigInt64Array(4)).length])",
                r"[4, 4, 4, 4, 4, 4]",
            ),
            (
                r"E(() => { const a = new Uint8Array(32); const r = crypto.getRandomValues(a); return [r === a, a.some((x) => x !== 0)]; })",
                r"[true, true]",
            ),
            (
                r"E(() => { const a = new Uint32Array(64); crypto.getRandomValues(a); return [a.some((x) => x > 0xffff), new Set(a).size > 32]; })",
                r"[true, true]",
            ),
            (
                r"E(() => { const b = new ArrayBuffer(16); const a = new Uint8Array(b, 4, 8); crypto.getRandomValues(a); const all = new Uint8Array(b); return [all.slice(0, 4).every((x) => x === 0), all.slice(12).every((x) => x === 0), all.slice(4, 12).some((x) => x !== 0)]; })",
                r"[true, true, true]",
            ),
            (
                r"E(() => crypto.getRandomValues.call({}, new Uint8Array(1)))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => { const rb = new ArrayBuffer(4, { maxByteLength: 8 }); return crypto.getRandomValues(new Uint8Array(rb)).length; })",
                r#""TypeError: Failed to execute 'getRandomValues' on 'Crypto': The provided ArrayBufferView value must not be resizable.""#,
            ),
            (
                r"E(() => { const x = new Uint8Array(4); structuredClone(x.buffer, { transfer: [x.buffer] }); return crypto.getRandomValues(x).length; })",
                r"0",
            ),
            (
                r"E(() => { const a = crypto.randomUUID(); return [typeof a, a.length, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(a), crypto.randomUUID() !== crypto.randomUUID(), crypto.randomUUID.length, crypto.randomUUID.name]; })",
                r#"["string", 36, true, true, 0, "randomUUID"]"#,
            ),
            (
                r"E(() => crypto.randomUUID.call({}))",
                r#""TypeError: Illegal invocation""#,
            ),
            (r"typeof crypto.subtle", r#""object""#),
        ],
    },
    Case {
        name: "base64",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function U(a) { return Array.from(a).join(); }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
</script></body></html>",
        queries: &[
            (
                r"[typeof atob, atob.length, btoa.length, atob.name, btoa.name, Object.getOwnPropertyDescriptor(window, 'atob').enumerable, Object.getOwnPropertyDescriptor(window, 'atob').writable, Object.getOwnPropertyDescriptor(window, 'btoa').configurable]",
                r#"["function", 1, 1, "atob", "btoa", true, true, true]"#,
            ),
            (
                r"E(() => atob())",
                r#""TypeError: Failed to execute 'atob' on 'Window': 1 argument required, but only 0 present.""#,
            ),
            (
                r"E(() => btoa())",
                r#""TypeError: Failed to execute 'btoa' on 'Window': 1 argument required, but only 0 present.""#,
            ),
            (
                r"[btoa(''), btoa('a'), btoa('ab'), btoa('abc'), btoa('abcd'), btoa('ÿ\u0000\u0080'), btoa(null), btoa(undefined), btoa(12), btoa({ toString() { return 'xy'; } })]",
                r#"["", "YQ==", "YWI=", "YWJj", "YWJjZA==", "/wCA", "bnVsbA==", "dW5kZWZpbmVk", "MTI=", "eHk="]"#,
            ),
            (
                r"E(() => btoa('Ā'))",
                r#""DOMException InvalidCharacterError: Failed to execute 'btoa' on 'Window': The string to be encoded contains characters outside of the Latin1 range.""#,
            ),
            (
                r"E(() => btoa('\ud800'))",
                r#""DOMException InvalidCharacterError: Failed to execute 'btoa' on 'Window': The string to be encoded contains characters outside of the Latin1 range.""#,
            ),
            (
                r"E(() => btoa('a€'))",
                r#""DOMException InvalidCharacterError: Failed to execute 'btoa' on 'Window': The string to be encoded contains characters outside of the Latin1 range.""#,
            ),
            (
                r"[atob(''), atob('YQ=='), atob('YQ'), atob('YWI='), atob('YWI'), atob('YWJj'), atob(' Y W J j '), atob('YW\nJj'), atob('YQ==\n'), atob('/+8=').split('').map((c) => c.charCodeAt(0)).join(), atob(null), JSON.stringify(atob('/w=='))]",
                r#"["", "a", "a", "ab", "ab", "abc", "abc", "abc", "a", "255,239", "ée", "\"ÿ\""]"#,
            ),
            (
                r"E(() => atob('Y'))",
                r#""DOMException InvalidCharacterError: Failed to execute 'atob' on 'Window': The string to be decoded is not correctly encoded.""#,
            ),
            (
                r"E(() => atob('YQ='))",
                r#""DOMException InvalidCharacterError: Failed to execute 'atob' on 'Window': The string to be decoded is not correctly encoded.""#,
            ),
            (
                r"E(() => atob('YQ==='))",
                r#""DOMException InvalidCharacterError: Failed to execute 'atob' on 'Window': The string to be decoded is not correctly encoded.""#,
            ),
            (
                r"E(() => atob('YQ=a'))",
                r#""DOMException InvalidCharacterError: Failed to execute 'atob' on 'Window': The string to be decoded is not correctly encoded.""#,
            ),
            (
                r"E(() => atob('Y=Q='))",
                r#""DOMException InvalidCharacterError: Failed to execute 'atob' on 'Window': The string to be decoded is not correctly encoded.""#,
            ),
            (
                r"E(() => atob('Y*Q='))",
                r#""DOMException InvalidCharacterError: Failed to execute 'atob' on 'Window': The string to be decoded is not correctly encoded.""#,
            ),
            (
                r"E(() => atob('-_'))",
                r#""DOMException InvalidCharacterError: Failed to execute 'atob' on 'Window': The string to be decoded is not correctly encoded.""#,
            ),
            (
                r"E(() => atob('ĀAAA'))",
                r#""DOMException InvalidCharacterError: Failed to execute 'atob' on 'Window': The string to be decoded contains characters outside of the Latin1 range.""#,
            ),
            (r"E(() => atob('YQ== '))", r#""a""#),
            (
                r"E(() => atob('A'))",
                r#""DOMException InvalidCharacterError: Failed to execute 'atob' on 'Window': The string to be decoded is not correctly encoded.""#,
            ),
            (
                r"E(() => atob('AAAAA'))",
                r#""DOMException InvalidCharacterError: Failed to execute 'atob' on 'Window': The string to be decoded is not correctly encoded.""#,
            ),
            (r"E(() => atob('AAAAAA'))", r#""\u0000\u0000\u0000\u0000""#),
            (
                r"E(() => atob('AAAAAA=='))",
                r#""\u0000\u0000\u0000\u0000""#,
            ),
            (
                r"E(() => atob('AAAA='))",
                r#""DOMException InvalidCharacterError: Failed to execute 'atob' on 'Window': The string to be decoded is not correctly encoded.""#,
            ),
            (
                r"E(() => atob('AAA=='))",
                r#""DOMException InvalidCharacterError: Failed to execute 'atob' on 'Window': The string to be decoded is not correctly encoded.""#,
            ),
            (
                r"E(() => atob('AAAA===='))",
                r#""DOMException InvalidCharacterError: Failed to execute 'atob' on 'Window': The string to be decoded is not correctly encoded.""#,
            ),
            (r"E(() => atob('YR=='))", r#""a""#),
            (r"E(() => atob('YWJ='))", r#""ab""#),
            (
                r"E(() => atob(' YQ=='))",
                r#""DOMException InvalidCharacterError: Failed to execute 'atob' on 'Window': The string to be decoded is not correctly encoded.""#,
            ),
            (
                r"E(() => { const all = Array.from({ length: 256 }, (_, i) => String.fromCharCode(i)).join(''); return atob(btoa(all)) === all; })",
                r"true",
            ),
            (r"E(() => btoa('x'.repeat(100000)).length)", r"133336"),
        ],
    },
    Case {
        name: "text-encoder",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function U(a) { return Array.from(a).join(); }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
</script></body></html>",
        queries: &[
            (
                r"[typeof TextEncoder, TextEncoder.length, Object.getOwnPropertyNames(TextEncoder.prototype).sort().join(), new TextEncoder().encoding, Object.prototype.toString.call(new TextEncoder())]",
                r#"["function", 0, "constructor,encode,encodeInto,encoding", "utf-8", "[object TextEncoder]"]"#,
            ),
            (
                r"E(() => TextEncoder())",
                r#""TypeError: Failed to construct 'TextEncoder': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"[U(new TextEncoder().encode('a€😀')), U(new TextEncoder().encode()), U(new TextEncoder().encode(undefined)), U(new TextEncoder().encode(null)), U(new TextEncoder().encode('\ud800')), U(new TextEncoder().encode('x\udc00y')), new TextEncoder().encode('ab') instanceof Uint8Array, new TextEncoder().encode('ab').buffer.byteLength]",
                r#"["97,226,130,172,240,159,152,128", "", "", "110,117,108,108", "239,191,189", "120,239,191,189,121", true, 2]"#,
            ),
            (
                r"E(() => { const d = new Uint8Array(4); const r = new TextEncoder().encodeInto('a€b', d); return [r.read, r.written, U(d), Object.keys(r).join(), Object.getPrototypeOf(r) === Object.prototype]; })",
                r#"[2, 4, "97,226,130,172", "read,written", true]"#,
            ),
            (
                r"E(() => { const d = new Uint8Array(4); const r = new TextEncoder().encodeInto('€€', d); return [r.read, r.written, U(d)]; })",
                r#"[1, 3, "226,130,172,0"]"#,
            ),
            (
                r"E(() => { const d = new Uint8Array(4); const r = new TextEncoder().encodeInto('😀😀', d); return [r.read, r.written, U(d)]; })",
                r#"[2, 4, "240,159,152,128"]"#,
            ),
            (
                r"E(() => { const d = new Uint8Array(3); const r = new TextEncoder().encodeInto('😀', d); return [r.read, r.written, U(d)]; })",
                r#"[0, 0, "0,0,0"]"#,
            ),
            (
                r"E(() => { const d = new Uint8Array(0); const r = new TextEncoder().encodeInto('a', d); return [r.read, r.written]; })",
                r"[0, 0]",
            ),
            (
                r"E(() => { const d = new Uint8Array(8); const r = new TextEncoder().encodeInto('\ud800a', d); return [r.read, r.written, U(d)]; })",
                r#"[2, 4, "239,191,189,97,0,0,0,0"]"#,
            ),
            (
                r"E(() => { const b = new ArrayBuffer(8); const d = new Uint8Array(b, 2, 3); new TextEncoder().encodeInto('abcdef', d); return U(new Uint8Array(b)); })",
                r#""0,0,97,98,99,0,0,0""#,
            ),
            (
                r"E(() => new TextEncoder().encodeInto('a', new Uint16Array(2)))",
                r#""TypeError: Failed to execute 'encodeInto' on 'TextEncoder': parameter 2 is not of type 'Uint8Array'.""#,
            ),
            (
                r"E(() => new TextEncoder().encodeInto('a', new ArrayBuffer(2)))",
                r#""TypeError: Failed to execute 'encodeInto' on 'TextEncoder': parameter 2 is not of type 'Uint8Array'.""#,
            ),
            (
                r"E(() => new TextEncoder().encodeInto('a'))",
                r#""TypeError: Failed to execute 'encodeInto' on 'TextEncoder': 2 arguments required, but only 1 present.""#,
            ),
            (
                r"E(() => new TextEncoder().encodeInto('a', [1]))",
                r#""TypeError: Failed to execute 'encodeInto' on 'TextEncoder': parameter 2 is not of type 'Uint8Array'.""#,
            ),
            (
                r"E(() => new TextEncoder().encodeInto('a', new Uint8ClampedArray(2)))",
                r#""TypeError: Failed to execute 'encodeInto' on 'TextEncoder': parameter 2 is not of type 'Uint8Array'.""#,
            ),
            (
                r"E(() => new TextEncoder().encodeInto('a', new DataView(new ArrayBuffer(2))))",
                r#""TypeError: Failed to execute 'encodeInto' on 'TextEncoder': parameter 2 is not of type 'Uint8Array'.""#,
            ),
            (
                r"E(() => new TextEncoder().encode.call({}))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => U(new TextEncoder().encode('é'.repeat(100000))).length)",
                r"799999",
            ),
        ],
    },
    Case {
        name: "text-decoder",
        html: r"<!DOCTYPE html><html><head></head><body><script>
window.L = [];
function log(x) { L.push(x); }
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return (e instanceof DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function U(a) { return Array.from(a).join(); }
function wait(ms) { return new Promise((r) => setTimeout(r, ms)); }
</script></body></html>",
        queries: &[
            (
                r"[typeof TextDecoder, TextDecoder.length, Object.getOwnPropertyNames(TextDecoder.prototype).sort().join(), Object.prototype.toString.call(new TextDecoder())]",
                r#"["function", 0, "constructor,decode,encoding,fatal,ignoreBOM", "[object TextDecoder]"]"#,
            ),
            (
                r"E(() => TextDecoder())",
                r#""TypeError: Failed to construct 'TextDecoder': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new TextDecoder().encoding + ' ' + new TextDecoder().fatal + ' ' + new TextDecoder().ignoreBOM)",
                r#""utf-8 false false""#,
            ),
            (
                r"[new TextDecoder('UTF-8').encoding, new TextDecoder(' utf8 ').encoding, new TextDecoder('latin1').encoding, new TextDecoder('ascii').encoding, new TextDecoder('utf-16').encoding, new TextDecoder('utf-16be').encoding, new TextDecoder('windows-1251').encoding, new TextDecoder('sjis').encoding, new TextDecoder('gbk').encoding, new TextDecoder('big5').encoding, new TextDecoder('euc-kr').encoding, new TextDecoder('koi8-r').encoding, new TextDecoder('macintosh').encoding, new TextDecoder('x-user-defined').encoding, new TextDecoder('iso-8859-2').encoding, new TextDecoder('gb2312').encoding, new TextDecoder('us-ascii').encoding, new TextDecoder('utf-16le').encoding].join()",
                r#""utf-8,utf-8,windows-1252,windows-1252,utf-16le,utf-16be,windows-1251,shift_jis,gbk,big5,euc-kr,koi8-r,macintosh,x-user-defined,iso-8859-2,gbk,windows-1252,utf-16le""#,
            ),
            (
                r"E(() => new TextDecoder('nope'))",
                r#""RangeError: Failed to construct 'TextDecoder': The encoding label provided ('nope') is invalid.""#,
            ),
            (
                r"E(() => new TextDecoder(''))",
                r#""RangeError: Failed to construct 'TextDecoder': The encoding label provided ('') is invalid.""#,
            ),
            (
                r"E(() => new TextDecoder('replacement'))",
                r#""RangeError: Failed to construct 'TextDecoder': The encoding label provided ('replacement') is invalid.""#,
            ),
            (
                r"E(() => new TextDecoder('iso-2022-kr'))",
                r#""RangeError: Failed to construct 'TextDecoder': The encoding label provided ('iso-2022-kr') is invalid.""#,
            ),
            (
                r"E(() => new TextDecoder('hz-gb-2312'))",
                r#""RangeError: Failed to construct 'TextDecoder': The encoding label provided ('hz-gb-2312') is invalid.""#,
            ),
            (
                r"E(() => new TextDecoder(undefined).encoding)",
                r#""utf-8""#,
            ),
            (
                r"E(() => new TextDecoder(null))",
                r#""RangeError: Failed to construct 'TextDecoder': The encoding label provided ('null') is invalid.""#,
            ),
            (
                r"E(() => new TextDecoder('utf-8', null).encoding)",
                r#""utf-8""#,
            ),
            (
                r"E(() => new TextDecoder('utf-8', 5))",
                r#""TypeError: Failed to construct 'TextDecoder': The provided value is not of type 'TextDecoderOptions'.""#,
            ),
            (
                r"E(() => { const d = new TextDecoder('utf-8', { fatal: 1, ignoreBOM: 'x' }); return [d.fatal, d.ignoreBOM]; })",
                r"[true, true]",
            ),
            (
                r"[new TextDecoder().decode(new Uint8Array([0xe2, 0x82, 0xac, 0x61])), new TextDecoder().decode(), new TextDecoder().decode(undefined), new TextDecoder().decode(new Uint8Array([0xef, 0xbb, 0xbf, 0x61])).length, new TextDecoder('utf-8', { ignoreBOM: true }).decode(new Uint8Array([0xef, 0xbb, 0xbf, 0x61])).length]",
                r#"["€a", "", "", 1, 2]"#,
            ),
            (
                r"E(() => new TextDecoder().decode(null))",
                r#""TypeError: Failed to execute 'decode' on 'TextDecoder': parameter 1 is not of type 'ArrayBuffer'.""#,
            ),
            (
                r"E(() => new TextDecoder().decode(5))",
                r#""TypeError: Failed to execute 'decode' on 'TextDecoder': parameter 1 is not of type 'ArrayBuffer'.""#,
            ),
            (
                r"E(() => new TextDecoder().decode('abc'))",
                r#""TypeError: Failed to execute 'decode' on 'TextDecoder': parameter 1 is not of type 'ArrayBuffer'.""#,
            ),
            (
                r"E(() => new TextDecoder().decode([1, 2]))",
                r#""TypeError: Failed to execute 'decode' on 'TextDecoder': parameter 1 is not of type 'ArrayBuffer'.""#,
            ),
            (
                r"E(() => new TextDecoder().decode({}))",
                r#""TypeError: Failed to execute 'decode' on 'TextDecoder': parameter 1 is not of type 'ArrayBuffer'.""#,
            ),
            (
                r"[new TextDecoder().decode(new Uint8Array([0x61, 0xff, 0x62])), new TextDecoder().decode(new Uint8Array([0xe2, 0x82])), new TextDecoder().decode(new Uint8Array([0xf0, 0x9f, 0x98, 0x80])), new TextDecoder().decode(new Uint8Array([0xc0, 0x80])).length, new TextDecoder().decode(new Uint8Array([0xed, 0xa0, 0x80])).length]",
                r#"["a�b", "�", "😀", 2, 3]"#,
            ),
            (
                r"E(() => new TextDecoder('utf-8', { fatal: true }).decode(new Uint8Array([0x61, 0xff])))",
                r#""TypeError: Failed to execute 'decode' on 'TextDecoder': The encoded data was not valid.""#,
            ),
            (
                r"E(() => new TextDecoder('utf-8', { fatal: true }).decode(new Uint8Array([0xe2, 0x82])))",
                r#""TypeError: Failed to execute 'decode' on 'TextDecoder': The encoded data was not valid.""#,
            ),
            (
                r"E(() => new TextDecoder('utf-16le', { fatal: true }).decode(new Uint8Array([0x00, 0xd8])))",
                r#""TypeError: Failed to execute 'decode' on 'TextDecoder': The encoded data was not valid.""#,
            ),
            (
                r"E(() => new TextDecoder('utf-16le').decode(new Uint8Array([0x61, 0, 0x62, 0])))",
                r#""ab""#,
            ),
            (
                r"E(() => new TextDecoder('utf-16be').decode(new Uint8Array([0, 0x61, 0, 0x62])))",
                r#""ab""#,
            ),
            (
                r"E(() => new TextDecoder('utf-16le').decode(new Uint8Array([0xff, 0xfe, 0x61, 0])).length)",
                r"1",
            ),
            (
                r"E(() => new TextDecoder('latin1').decode(new Uint8Array([0x80, 0xe9, 0xff])))",
                r#""€éÿ""#,
            ),
            (
                r"E(() => new TextDecoder('windows-1252').decode(new Uint8Array([0x80, 0x9f, 0x81])))",
                r#""€Ÿ""#,
            ),
            (
                r"E(() => new TextDecoder('windows-1251').decode(new Uint8Array([0xcf, 0xf0, 0xe8, 0xe2, 0xe5, 0xf2])))",
                r#""Привет""#,
            ),
            (
                r"E(() => new TextDecoder('shift_jis').decode(new Uint8Array([0x82, 0xa0, 0x82, 0xa2])))",
                r#""あい""#,
            ),
            (
                r"E(() => new TextDecoder('gbk').decode(new Uint8Array([0xc4, 0xe3, 0xba, 0xc3])))",
                r#""你好""#,
            ),
            (
                r"E(() => new TextDecoder('x-user-defined').decode(new Uint8Array([0x41, 0x80, 0xff])).split('').map((c) => c.charCodeAt(0).toString(16)).join())",
                r#""41,f780,f7ff""#,
            ),
            (
                r"E(() => { const d = new TextDecoder(); const a = d.decode(new Uint8Array([0xe2, 0x82]), { stream: true }); const b = d.decode(new Uint8Array([0xac])); return [a, b]; })",
                r#"["", "€"]"#,
            ),
            (
                r"E(() => { const d = new TextDecoder(); const a = d.decode(new Uint8Array([0xe2, 0x82]), { stream: true }); const b = d.decode(); return [a, b.length, b.charCodeAt(0)]; })",
                r#"["", 1, 65533]"#,
            ),
            (
                r"E(() => { const d = new TextDecoder(); const a = d.decode(new Uint8Array([0xe2, 0x82]), { stream: true }); const b = d.decode(new Uint8Array([0x61])); const c = d.decode(new Uint8Array([0xac])); return [a, b.length, b.charCodeAt(0), c.length, c]; })",
                r#"["", 2, 65533, 1, "�"]"#,
            ),
            (
                r"E(() => { const d = new TextDecoder(); return [d.decode(new Uint8Array([0xef, 0xbb]), { stream: true }), d.decode(new Uint8Array([0xbf, 0x61]), { stream: true }), d.decode(new Uint8Array([0xef, 0xbb, 0xbf]), { stream: true }).length, d.decode().length]; })",
                r#"["", "a", 1, 0]"#,
            ),
            (
                r"E(() => { const d = new TextDecoder(); return [d.decode(new Uint8Array([0xef, 0xbb, 0xbf, 0x61])).length, d.decode(new Uint8Array([0xef, 0xbb, 0xbf, 0x61])).length]; })",
                r"[1, 1]",
            ),
            (
                r"E(() => { const d = new TextDecoder('utf-8', { fatal: true }); try { d.decode(new Uint8Array([0xff])); } catch (e) {} return d.decode(new Uint8Array([0x61])); })",
                r#""a""#,
            ),
            (
                r"E(() => { const d = new TextDecoder('utf-8', { fatal: true }); d.decode(new Uint8Array([0xe2]), { stream: true }); try { d.decode(new Uint8Array([0x61]), { stream: true }); } catch (e) { return e.name; } })",
                r#""TypeError""#,
            ),
            (
                r"E(() => { const d = new TextDecoder(); d.decode(new Uint8Array([0xe2]), { stream: true }); return d.decode(new Uint8Array([0x61])).length; })",
                r"2",
            ),
            (
                r"E(() => new TextDecoder().decode(new Uint16Array([0x6261])))",
                r#""ab""#,
            ),
            (
                r"E(() => new TextDecoder().decode(new DataView(new ArrayBuffer(2), 1, 1)).length)",
                r"1",
            ),
            (
                r"E(() => new TextDecoder().decode(new Uint8Array([0x61, 0x62, 0x63]).subarray(1)))",
                r#""bc""#,
            ),
            (
                r"E(() => new TextDecoder().decode(new Uint8Array([0x61, 0x62, 0x63]).buffer))",
                r#""abc""#,
            ),
            (
                r"E(() => new TextDecoder().decode(new Float32Array([1])).length)",
                r"4",
            ),
            (
                r"E(() => new TextDecoder().decode(new Uint8Array([0, 0])).length)",
                r"2",
            ),
            (
                r"E(() => new TextDecoder().decode(new Uint8Array([0xe2, 0x82]), 5))",
                r#""TypeError: Failed to execute 'decode' on 'TextDecoder': The provided value is not of type 'TextDecodeOptions'.""#,
            ),
            (
                r"E(() => new TextDecoder().decode(new Uint8Array([0x61]), { stream: 'yes' }))",
                r#""a""#,
            ),
            (
                r"E(() => new TextDecoder().decode.call({}))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(TextDecoder.prototype, 'encoding').get.call({}))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => new TextDecoder().decode(new Uint8Array(1 << 20)).length)",
                r"1048576",
            ),
            (
                r"E(() => new TextDecoder('windows-1252').decode(new Uint8Array(30 << 20).fill(0x80)).length)",
                r"31457280",
            ),
            (
                r"E(() => new TextDecoder().decode(new Uint8Array(100 << 20)).length)",
                r"104857600",
            ),
        ],
    },
];
