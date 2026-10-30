// The cases of tools/probes/web-blob-formdata.json with the values of Chromium 148.0.
// Generated from `swbtools probe --json` (three runs, all equal); do not edit.

use crate::{Case, Options, Res};

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "blob",
        html: r#"<!DOCTYPE html><form id=f method=post action=echo.txt>
<input name=a value=1><input name=b value="x y"><input type=checkbox name=c value=on checked><input type=checkbox name=d><input type=radio name=r value=1 checked><input type=radio name=r value=2>
<select name=s><option value=o1>1<option value=o2 selected>2</select><textarea name=t>line1
line2</textarea><input type=file name=file><input type=submit name=go value=Go><button name=btn value=B>B</button><input name=nov disabled value=z><input value=noname><input type=image name=im>
</form><script>
const E = e => e.name + ': ' + e.message;
const Q = f => { try { return f(); } catch (e) { return E(e); } };
const S = async b => ({size: b.size, type: b.type, text: await b.text()});
const nb = s => s.replace(/----[A-Za-z0-9]{16,}/g, 'BOUNDARY');
const logOf = async () => (await (await fetch('__log')).json()).filter(e => !e.path.endsWith('hello.txt')).map(e => [e.method, e.path.split('/').pop(), nb(e.headers['content-type'] || ''), JSON.stringify(nb(e.body || ''))].join(' '));
const wait = ms => new Promise(r => setTimeout(r, ms));
</script>"#,
        resources: &[
            Res {
                path: r"echo.txt",
                text: r"echo",
                status: 200,
                delay_ms: 0,
                content_type: None,
                headers: &[(r"Content-Type", r"text/plain")],
                options: Options::None,
            },
            Res {
                path: r"hello.txt",
                text: r"hello",
                status: 200,
                delay_ms: 0,
                content_type: None,
                headers: &[(r"Content-Type", r"text/plain")],
                options: Options::None,
            },
        ],
        queries: &[
            (
                r"(async () => { const out = []; const b = new Blob(['ab', new Uint8Array([99, 100]), new Blob(['ef'], {type: 'x/y'}), new ArrayBuffer(2)], {type: 'Text/PLAIN'}); out.push(await S(b)); const s = b.slice(1, 5, 'a/b'); out.push(await S(s)); out.push(await S(b.slice(-3))); out.push(await S(b.slice(2, 1))); out.push(Object.prototype.toString.call(b), b instanceof Blob, new Blob().size, new Blob([]).type, Q(() => new Blob('abc')), Q(() => new Blob(['a'], {endings: 'bad'})), Q(() => new Blob(1)), (await b.arrayBuffer()).byteLength, [...await b.bytes()].join(), (await new Blob(['a\nb\r\nc'], {endings: 'native'}).text()).length, new Blob(['é']).size, await new Blob([{toString() { return 'obj'; }}]).text(), await new Blob([1, null, undefined]).text(), new Blob(['x'], {type: 'a\u00e9'}).type, new Blob(['x'], {type: 'é/b'}).type.length); return out; })()",
                r#"[{"size": 8, "type": "text/plain", "text": "abcdef\u0000\u0000"}, {"size": 4, "type": "a/b", "text": "bcde"}, {"size": 3, "type": "", "text": "f\u0000\u0000"}, {"size": 0, "type": "", "text": ""}, "[object Blob]", true, 0, "", "TypeError: Failed to construct 'Blob': The provided value cannot be converted to a sequence.", "TypeError: Failed to construct 'Blob': Failed to read the 'endings' property from 'BlobPropertyBag': The provided value 'bad' is not a valid enum value of type EndingType.", "TypeError: Failed to construct 'Blob': The provided value cannot be converted to a sequence.", 8, "97,98,99,100,101,102,0,0", 5, 2, "obj", "1nullundefined", "", 0]"#,
            ),
            (
                r"[Object.getOwnPropertyNames(Blob.prototype).join(), Object.getOwnPropertyNames(File.prototype).join(), Object.getPrototypeOf(File.prototype) === Blob.prototype, Object.getOwnPropertyNames(FileReader.prototype).join(), Object.getOwnPropertyNames(FormData.prototype).join(), Object.getOwnPropertyNames(URL).join()]",
                r#"["size,type,arrayBuffer,slice,stream,text,bytes,constructor", "name,lastModified,lastModifiedDate,webkitRelativePath,constructor", true, "readyState,result,error,onloadstart,onprogress,onload,onabort,onerror,onloadend,EMPTY,LOADING,DONE,abort,readAsArrayBuffer,readAsBinaryString,readAsDataURL,readAsText,constructor", "append,delete,get,getAll,has,set,entries,forEach,keys,values,constructor", "length,name,prototype,canParse,parse,createObjectURL,revokeObjectURL"]"#,
            ),
            (
                r"(async () => { const f = new File(['abc', 'd'], 'n.txt', {type: 'text/x', lastModified: 5}); return [f.name, f.size, f.type, f.lastModified, f instanceof Blob, f instanceof File, Object.prototype.toString.call(f), typeof new File([], 'x').lastModified, Q(() => new File(['a'])), Q(() => new File()), await f.text(), (await f.slice(1, 3).text()), f.slice(1,3).constructor.name, f.webkitRelativePath]; })()",
                r#"["n.txt", 4, "text/x", 5, true, true, "[object File]", "number", "TypeError: Failed to construct 'File': 2 arguments required, but only 1 present.", "TypeError: Failed to construct 'File': 2 arguments required, but only 0 present.", "abcd", "bc", "Blob", ""]"#,
            ),
        ],
    },
    Case {
        name: "formdata",
        html: r#"<!DOCTYPE html><form id=f method=post action=echo.txt>
<input name=a value=1><input name=b value="x y"><input type=checkbox name=c value=on checked><input type=checkbox name=d><input type=radio name=r value=1 checked><input type=radio name=r value=2>
<select name=s><option value=o1>1<option value=o2 selected>2</select><textarea name=t>line1
line2</textarea><input type=file name=file><input type=submit name=go value=Go><button name=btn value=B>B</button><input name=nov disabled value=z><input value=noname><input type=image name=im>
</form><script>
const E = e => e.name + ': ' + e.message;
const Q = f => { try { return f(); } catch (e) { return E(e); } };
const S = async b => ({size: b.size, type: b.type, text: await b.text()});
const nb = s => s.replace(/----[A-Za-z0-9]{16,}/g, 'BOUNDARY');
const logOf = async () => (await (await fetch('__log')).json()).filter(e => !e.path.endsWith('hello.txt')).map(e => [e.method, e.path.split('/').pop(), nb(e.headers['content-type'] || ''), JSON.stringify(nb(e.body || ''))].join(' '));
const wait = ms => new Promise(r => setTimeout(r, ms));
</script>"#,
        resources: &[
            Res {
                path: r"echo.txt",
                text: r"echo",
                status: 200,
                delay_ms: 0,
                content_type: None,
                headers: &[(r"Content-Type", r"text/plain")],
                options: Options::None,
            },
            Res {
                path: r"hello.txt",
                text: r"hello",
                status: 200,
                delay_ms: 0,
                content_type: None,
                headers: &[(r"Content-Type", r"text/plain")],
                options: Options::None,
            },
        ],
        queries: &[
            (
                r"(() => { const fd = new FormData(); fd.append('a', '1'); fd.append('a', '2'); fd.append('b', new Blob(['x'], {type: 't/u'})); fd.append('c', new Blob(['y']), 'fn.txt'); fd.append('d', new File(['z'], 'zz.txt')); fd.set('a', 'three'); fd.append('e', 5); return [fd.get('a'), fd.getAll('a'), fd.has('a'), fd.has('q'), fd.get('q'), fd.getAll('q'), [...fd].map(([k, v]) => k + '=' + (typeof v === 'string' ? v : v.constructor.name + ':' + v.name + ':' + v.type + ':' + v.size)), [...fd.keys()], [...fd.values()].length, (fd.delete('a'), [...fd.keys()]), Object.prototype.toString.call(fd), Q(() => fd.append('a')), Q(() => fd.append('a', 'b', 'c')), Q(() => fd.append('a', {})), Q(() => fd.get()), Q(() => { fd.forEach((v, k, o) => { if (o !== fd) throw 1; }); return 'ok'; }), Q(() => fd.set('q', new Blob(['u']), 'nm').toString()), fd.get('q').name, fd.get('b').name]; })()",
                r#"["three", ["three"], true, false, null, [], ["a=three", "b=File:blob:t/u:1", "c=File:fn.txt::1", "d=File:zz.txt::1", "e=5"], ["a", "b", "c", "d", "e"], 5, ["b", "c", "d", "e"], "[object FormData]", "TypeError: Failed to execute 'append' on 'FormData': 2 arguments required, but only 1 present.", "TypeError: Failed to execute 'append' on 'FormData': parameter 2 is not of type 'Blob'.", null, "TypeError: Failed to execute 'get' on 'FormData': 1 argument required, but only 0 present.", "ok", "TypeError: Cannot read properties of undefined (reading 'toString')", "nm", "blob"]"#,
            ),
            (
                r"(() => { const fd = new FormData(document.getElementById('f')); return [[...fd].map(([k, v]) => k + '=' + (typeof v === 'string' ? JSON.stringify(v) : v.constructor.name + ':' + JSON.stringify(v.name) + ':' + v.type + ':' + v.size))]; })()",
                r#"[["a=\"1\"", "b=\"x y\"", "c=\"on\"", "r=\"1\"", "s=\"o2\"", "t=\"line1\\nline2\"", "file=File:\"\":application/octet-stream:0"]]"#,
            ),
            (
                r"(() => { const f = document.getElementById('f'); const sub = f.querySelector('[type=submit]'); const btn = f.querySelector('button'); return [[...new FormData(f, sub)].map(e => e[0] + '=' + e[1]).filter(s => /^(go|btn)=/.test(s)), [...new FormData(f, btn)].map(e => e[0] + '=' + e[1]).filter(s => /^(go|btn)=/.test(s)), Q(() => new FormData(f, document.body)), Q(() => new FormData(5)), Q(() => new FormData({})), Q(() => new FormData(document.createElement('div')))]; })()",
                r#"[["go=Go"], ["btn=B"], "TypeError: Failed to construct 'FormData': The specified element is not a submit button.", "TypeError: Failed to construct 'FormData': parameter 1 is not of type 'HTMLFormElement'.", "TypeError: Failed to construct 'FormData': parameter 1 is not of type 'HTMLFormElement'.", "TypeError: Failed to construct 'FormData': parameter 1 is not of type 'HTMLFormElement'."]"#,
            ),
            (
                r"(() => { const f = document.getElementById('f'); const log = []; f.addEventListener('formdata', e => { log.push([e.type, e.constructor.name, e.bubbles, e.cancelable, e.isTrusted, e.target === f, e.formData instanceof FormData, e.formData.get('a')]); e.formData.append('added', 'yes'); }); const fd = new FormData(f); log.push(fd.get('added')); return log; })()",
                r#"[["formdata", "FormDataEvent", true, false, true, true, true, "1"], "yes"]"#,
            ),
            (
                r"(() => { const f = document.getElementById('f'); const log = []; f.addEventListener('formdata', e => log.push('formdata')); f.addEventListener('submit', e => { log.push('submit'); e.preventDefault(); }); f.requestSubmit(); f.reset(); return log; })()",
                r#"["submit"]"#,
            ),
        ],
    },
    Case {
        name: "multipart",
        html: r#"<!DOCTYPE html><form id=f method=post action=echo.txt>
<input name=a value=1><input name=b value="x y"><input type=checkbox name=c value=on checked><input type=checkbox name=d><input type=radio name=r value=1 checked><input type=radio name=r value=2>
<select name=s><option value=o1>1<option value=o2 selected>2</select><textarea name=t>line1
line2</textarea><input type=file name=file><input type=submit name=go value=Go><button name=btn value=B>B</button><input name=nov disabled value=z><input value=noname><input type=image name=im>
</form><script>
const E = e => e.name + ': ' + e.message;
const Q = f => { try { return f(); } catch (e) { return E(e); } };
const S = async b => ({size: b.size, type: b.type, text: await b.text()});
const nb = s => s.replace(/----[A-Za-z0-9]{16,}/g, 'BOUNDARY');
const logOf = async () => (await (await fetch('__log')).json()).filter(e => !e.path.endsWith('hello.txt')).map(e => [e.method, e.path.split('/').pop(), nb(e.headers['content-type'] || ''), JSON.stringify(nb(e.body || ''))].join(' '));
const wait = ms => new Promise(r => setTimeout(r, ms));
</script>"#,
        resources: &[
            Res {
                path: r"echo.txt",
                text: r"echo",
                status: 200,
                delay_ms: 0,
                content_type: None,
                headers: &[(r"Content-Type", r"text/plain")],
                options: Options::None,
            },
            Res {
                path: r"hello.txt",
                text: r"hello",
                status: 200,
                delay_ms: 0,
                content_type: None,
                headers: &[(r"Content-Type", r"text/plain")],
                options: Options::None,
            },
        ],
        queries: &[
            (
                r#"(async () => { const fd = new FormData(); fd.append('a', 'v1'); fd.append('b', new Blob(['bin'], {type: 'x/y'}), 'f.bin'); fd.append('c', 'line\nbreak"q'); fd.append('d\r\ne"', new File(['t'], 'a"b\nc.txt')); fd.append('e', new Blob(['u'])); await fetch('echo.txt', {method: 'POST', body: fd}); return await logOf(); })()"#,
                r#"["POST echo.txt multipart/form-data; boundary=BOUNDARY \"--BOUNDARY\\r\\nContent-Disposition: form-data; name=\\\"a\\\"\\r\\n\\r\\nv1\\r\\n--BOUNDARY\\r\\nContent-Disposition: form-data; name=\\\"b\\\"; filename=\\\"f.bin\\\"\\r\\nContent-\""]"#,
            ),
            (
                r"(async () => { const f = document.getElementById('f'); await fetch('echo.txt', {method: 'POST', body: new FormData(f)}); return (await logOf()).slice(-1); })()",
                r#"["POST echo.txt multipart/form-data; boundary=BOUNDARY \"--BOUNDARY\\r\\nContent-Disposition: form-data; name=\\\"a\\\"\\r\\n\\r\\n1\\r\\n--BOUNDARY\\r\\nContent-Disposition: form-data; name=\\\"b\\\"\\r\\n\\r\\nx y\\r\\n------WebKitFormBoun\""]"#,
            ),
            (
                r#"(async () => { const fd = new FormData(); fd.append('q"n\nx', 'v\nw'); await fetch('echo.txt', {method: 'POST', body: fd}); return (await logOf()).slice(-1); })()"#,
                r#"["POST echo.txt multipart/form-data; boundary=BOUNDARY \"--BOUNDARY\\r\\nContent-Disposition: form-data; name=\\\"q%22n%0D%0Ax\\\"\\r\\n\\r\\nv\\r\\nw\\r\\n--BOUNDARY--\\r\\n\""]"#,
            ),
            (
                r#"(async () => { const fd = new FormData(); fd.append('k', new File(['t'], 'a"b\nc.txt', {type: 'T/X'})); await fetch('echo.txt', {method: 'POST', body: fd}); return (await logOf()).slice(-1); })()"#,
                r#"["POST echo.txt multipart/form-data; boundary=BOUNDARY \"--BOUNDARY\\r\\nContent-Disposition: form-data; name=\\\"k\\\"; filename=\\\"a%22b%0Ac.txt\\\"\\r\\nContent-Type: t/x\\r\\n\\r\\nt\\r\\n--BOUNDARY--\\r\\n\""]"#,
            ),
            (
                r"(async () => { const r = new Response(new FormData()); const q = new Request('echo.txt', {method: 'POST', body: new FormData()}); return [r.headers.get('content-type').replace(/----[A-Za-z0-9]+/, 'B'), q.headers.get('content-type').replace(/----[A-Za-z0-9]+/, 'B'), (await r.text()).replace(/----[A-Za-z0-9]+/g, 'B'), /^multipart\/form-data; boundary=----[A-Za-z0-9]{16}/.test(q.headers.get('content-type')), /^----[A-Za-z0-9]{16}/.exec(q.headers.get('content-type').split('boundary=')[1]) !== null]; })()",
                r#"["multipart/form-data; boundary=B", "multipart/form-data; boundary=B", "--B--\r\n", true, true]"#,
            ),
            (
                r"(async () => { const fd = new FormData(); fd.append('k', 'v'); fd.append('f', new Blob(['xyz'], {type: 'a/b'}), 'x.txt'); const r = new Response(fd); const f2 = await r.formData(); return [[...f2].map(([k, v]) => typeof v === 'string' ? k + '=' + v : k + ':' + v.constructor.name + ':' + v.name + ':' + v.type + ':' + v.size)]; })()",
                r#"[["k=v", "f:File:x.txt:a/b:3"]]"#,
            ),
            (
                r"(async () => { const r = new Response('a=1&b=%20x&a=2', {headers: {'content-type': 'application/x-www-form-urlencoded'}}); const f2 = await r.formData(); return [[...f2]]; })()",
                r#"[[["a", "1"], ["b", " x"], ["a", "2"]]]"#,
            ),
            (
                r"(async () => { const r = new Response('a=1'); try { await r.formData(); return 'ok'; } catch (e) { return E(e); } })()",
                r#""TypeError: Failed to fetch""#,
            ),
            (
                r"(async () => { const r = new Response('hello', {headers: {'content-type': 'text/x'}}); const b = await r.blob(); const r2 = new Response(new Blob(['q'], {type: 'a/b'})); return [b.type, b.size, r2.headers.get('content-type'), await r2.text()]; })()",
                r#"["text/x", 5, "a/b", "q"]"#,
            ),
            (
                r"(async () => { await fetch('echo.txt', {method: 'POST', body: new Blob(['blobbody'], {type: 'a/b'})}); await fetch('echo.txt', {method: 'POST', body: new Blob(['plain'])}); return await logOf(); })()",
                r#"["POST echo.txt multipart/form-data; boundary=BOUNDARY \"--BOUNDARY\\r\\nContent-Disposition: form-data; name=\\\"a\\\"\\r\\n\\r\\nv1\\r\\n--BOUNDARY\\r\\nContent-Disposition: form-data; name=\\\"b\\\"; filename=\\\"f.bin\\\"\\r\\nContent-\"", "POST echo.txt multipart/form-data; boundary=BOUNDARY \"--BOUNDARY\\r\\nContent-Disposition: form-data; name=\\\"a\\\"\\r\\n\\r\\n1\\r\\n--BOUNDARY\\r\\nContent-Disposition: form-data; name=\\\"b\\\"\\r\\n\\r\\nx y\\r\\n------WebKitFormBoun\"", "POST echo.txt multipart/form-data; boundary=BOUNDARY \"--BOUNDARY\\r\\nContent-Disposition: form-data; name=\\\"q%22n%0D%0Ax\\\"\\r\\n\\r\\nv\\r\\nw\\r\\n--BOUNDARY--\\r\\n\"", "POST echo.txt multipart/form-data; boundary=BOUNDARY \"--BOUNDARY\\r\\nContent-Disposition: form-data; name=\\\"k\\\"; filename=\\\"a%22b%0Ac.txt\\\"\\r\\nContent-Type: t/x\\r\\n\\r\\nt\\r\\n--BOUNDARY--\\r\\n\"", "POST echo.txt a/b \"blobbody\"", "POST echo.txt  \"plain\""]"#,
            ),
        ],
    },
    Case {
        name: "filereader",
        html: r#"<!DOCTYPE html><form id=f method=post action=echo.txt>
<input name=a value=1><input name=b value="x y"><input type=checkbox name=c value=on checked><input type=checkbox name=d><input type=radio name=r value=1 checked><input type=radio name=r value=2>
<select name=s><option value=o1>1<option value=o2 selected>2</select><textarea name=t>line1
line2</textarea><input type=file name=file><input type=submit name=go value=Go><button name=btn value=B>B</button><input name=nov disabled value=z><input value=noname><input type=image name=im>
</form><script>
const E = e => e.name + ': ' + e.message;
const Q = f => { try { return f(); } catch (e) { return E(e); } };
const S = async b => ({size: b.size, type: b.type, text: await b.text()});
const nb = s => s.replace(/----[A-Za-z0-9]{16,}/g, 'BOUNDARY');
const logOf = async () => (await (await fetch('__log')).json()).filter(e => !e.path.endsWith('hello.txt')).map(e => [e.method, e.path.split('/').pop(), nb(e.headers['content-type'] || ''), JSON.stringify(nb(e.body || ''))].join(' '));
const wait = ms => new Promise(r => setTimeout(r, ms));
</script>"#,
        resources: &[
            Res {
                path: r"echo.txt",
                text: r"echo",
                status: 200,
                delay_ms: 0,
                content_type: None,
                headers: &[(r"Content-Type", r"text/plain")],
                options: Options::None,
            },
            Res {
                path: r"hello.txt",
                text: r"hello",
                status: 200,
                delay_ms: 0,
                content_type: None,
                headers: &[(r"Content-Type", r"text/plain")],
                options: Options::None,
            },
        ],
        queries: &[
            (
                r"(async () => { const log = []; const fr = new FileReader(); for (const t of ['loadstart', 'progress', 'load', 'abort', 'error', 'loadend']) fr.addEventListener(t, e => log.push(t + ':' + fr.readyState + ':' + e.loaded + '/' + e.total + (e.lengthComputable ? 'c' : '') + ':' + e.constructor.name + ':' + (fr.result === null ? 'null' : typeof fr.result === 'string' ? JSON.stringify(fr.result) : Object.prototype.toString.call(fr.result)))); log.push('init:' + fr.readyState + ':' + fr.result + ':' + fr.error); fr.readAsText(new Blob(['héllo'])); log.push('after:' + fr.readyState + ':' + fr.result); await wait(100); return log; })()",
                r#"["init:0:null:null", "after:1:null", "loadstart:1:0/6c:ProgressEvent:null", "progress:1:6/6c:ProgressEvent:null", "load:2:6/6c:ProgressEvent:\"héllo\"", "loadend:2:6/6c:ProgressEvent:\"héllo\""]"#,
            ),
            (
                r"(async () => { const fr = new FileReader(); const r = await new Promise(res => { fr.onload = () => res(fr.result); fr.readAsDataURL(new Blob(['hi'], {type: 'text/plain'})); }); const r2 = await new Promise(res => { const f = new FileReader(); f.onload = () => res(f.result); f.readAsDataURL(new Blob(['hi'])); }); const r3 = await new Promise(res => { const f = new FileReader(); f.onload = () => res(f.result); f.readAsDataURL(new Blob([])); }); const r4 = await new Promise(res => { const f = new FileReader(); f.onload = () => res(f.result); f.readAsArrayBuffer(new Blob(['ab'])); }); const r5 = await new Promise(res => { const f = new FileReader(); f.onload = () => res(f.result); f.readAsText(new Blob([new Uint8Array([0xE9])]), 'iso-8859-1'); }); const r6 = await new Promise(res => { const f = new FileReader(); f.onload = () => res(f.result); f.readAsBinaryString(new Blob([new Uint8Array([0xE9, 65])])); }); return [r, r2, r3, Object.prototype.toString.call(r4), r4.byteLength, r5, r6]; })()",
                r#"["data:text/plain;base64,aGk=", "data:application/octet-stream;base64,aGk=", "data:application/octet-stream;base64,", "[object ArrayBuffer]", 2, "é", "éA"]"#,
            ),
            (
                r"(async () => { const log = []; const fr = new FileReader(); for (const t of ['loadstart', 'progress', 'load', 'abort', 'error', 'loadend']) fr.addEventListener(t, e => log.push(t + ':' + fr.readyState + ':' + (fr.error && fr.error.name))); fr.readAsText(new Blob(['x'])); log.push(Q(() => fr.readAsText(new Blob(['y'])))); fr.abort(); log.push('aborted:' + fr.readyState + ':' + fr.result); await wait(100); log.push(String(fr.error)); fr.abort(); log.push('abort-again'); await wait(50); return log; })()",
                r#"["InvalidStateError: Failed to execute 'readAsText' on 'FileReader': The object is already busy reading Blobs.", "abort:2:AbortError", "loadend:2:AbortError", "aborted:2:null", "AbortError: An ongoing operation was aborted, typically with a call to abort().", "abort-again"]"#,
            ),
            (
                r"(async () => { const log = []; const fr = new FileReader(); fr.onloadend = () => log.push('loadend'); fr.readAsText(new Blob(['x'])); await wait(100); fr.readAsText(new Blob(['second'])); log.push('re:' + fr.readyState + ':' + fr.result); await wait(100); log.push(fr.result); return log; })()",
                r#"["loadend", "re:1:null", "loadend", "second"]"#,
            ),
        ],
    },
    Case {
        name: "blob-urls",
        html: r#"<!DOCTYPE html><form id=f method=post action=echo.txt>
<input name=a value=1><input name=b value="x y"><input type=checkbox name=c value=on checked><input type=checkbox name=d><input type=radio name=r value=1 checked><input type=radio name=r value=2>
<select name=s><option value=o1>1<option value=o2 selected>2</select><textarea name=t>line1
line2</textarea><input type=file name=file><input type=submit name=go value=Go><button name=btn value=B>B</button><input name=nov disabled value=z><input value=noname><input type=image name=im>
</form><script>
const E = e => e.name + ': ' + e.message;
const Q = f => { try { return f(); } catch (e) { return E(e); } };
const S = async b => ({size: b.size, type: b.type, text: await b.text()});
const nb = s => s.replace(/----[A-Za-z0-9]{16,}/g, 'BOUNDARY');
const logOf = async () => (await (await fetch('__log')).json()).filter(e => !e.path.endsWith('hello.txt')).map(e => [e.method, e.path.split('/').pop(), nb(e.headers['content-type'] || ''), JSON.stringify(nb(e.body || ''))].join(' '));
const wait = ms => new Promise(r => setTimeout(r, ms));
</script>"#,
        resources: &[
            Res {
                path: r"echo.txt",
                text: r"echo",
                status: 200,
                delay_ms: 0,
                content_type: None,
                headers: &[(r"Content-Type", r"text/plain")],
                options: Options::None,
            },
            Res {
                path: r"hello.txt",
                text: r"hello",
                status: 200,
                delay_ms: 0,
                content_type: None,
                headers: &[(r"Content-Type", r"text/plain")],
                options: Options::None,
            },
        ],
        queries: &[
            (
                r"(async () => { const b = new Blob(['url-body'], {type: 'text/x'}); const u = URL.createObjectURL(b); const out = [u.replace(location.origin, 'O').replace(/[0-9a-f-]{36}/, 'UUID'), new URL(u).origin === location.origin, u.startsWith('blob:' + location.origin + '/')]; const r = await fetch(u); out.push(r.status, r.ok, r.headers.get('content-type'), r.headers.get('content-length'), await r.text()); out.push(Q(() => URL.revokeObjectURL(u))); out.push(await fetch(u).then(() => 'ok', e => E(e))); out.push(Q(() => URL.createObjectURL({})), Q(() => URL.createObjectURL('x'))); return out; })()",
                r#"["blob:O/UUID", true, true, 200, true, "text/x", "8", "url-body", null, "TypeError: Failed to fetch", "TypeError: Failed to execute 'createObjectURL' on 'URL': Overload resolution failed.", "TypeError: Failed to execute 'createObjectURL' on 'URL': Overload resolution failed."]"#,
            ),
            (
                r"(async () => { const u = URL.createObjectURL(new Blob(['xhr-body'], {type: 'text/y'})); const log = []; await new Promise(res => { const x = new XMLHttpRequest(); x.onloadend = () => { log.push([x.status, x.statusText, x.responseText, x.getResponseHeader('content-type'), x.getAllResponseHeaders().replace(/\r\n/g, '|'), x.responseURL === u]); res(); }; x.open('GET', u); x.send(); }); URL.revokeObjectURL(u); await new Promise(res => { const x = new XMLHttpRequest(); x.onloadend = () => { log.push([x.status, x.responseText]); res(); }; x.onerror = () => log.push('error'); x.open('GET', u); x.send(); }); return log; })()",
                r#"[[200, "OK", "xhr-body", "text/y", "content-length: 8|content-type: text/y|", true], "error", [0, ""]]"#,
            ),
            (
                r#"(async () => { const svg = "<svg xmlns='http://www.w3.org/2000/svg' width='12' height='8'/>"; const u = URL.createObjectURL(new Blob([svg], {type: 'image/svg+xml'})); const i = new Image(); const r = await new Promise(res => { i.onload = () => res('load:' + i.naturalWidth + 'x' + i.naturalHeight); i.onerror = () => res('error'); i.src = u; }); URL.revokeObjectURL(u); const j = new Image(); const r2 = await new Promise(res => { j.onload = () => res('load'); j.onerror = () => res('error'); j.src = u; }); return [r, r2]; })()"#,
                r#"["load:12x8", "error"]"#,
            ),
            (
                r"(async () => { const u = URL.createObjectURL(new Blob(['abc'])); const w = (await fetch(u, {method: 'POST'}).then(() => 'ok', e => E(e))); const w2 = await fetch(u + 'x').then(() => 'ok', e => E(e)); URL.revokeObjectURL(u); return [w, w2]; })()",
                r#"["TypeError: Failed to fetch", "TypeError: Failed to fetch"]"#,
            ),
        ],
    },
    Case {
        name: "beacon-0",
        html: r#"<!DOCTYPE html><form id=f method=post action=echo.txt>
<input name=a value=1><input name=b value="x y"><input type=checkbox name=c value=on checked><input type=checkbox name=d><input type=radio name=r value=1 checked><input type=radio name=r value=2>
<select name=s><option value=o1>1<option value=o2 selected>2</select><textarea name=t>line1
line2</textarea><input type=file name=file><input type=submit name=go value=Go><button name=btn value=B>B</button><input name=nov disabled value=z><input value=noname><input type=image name=im>
</form><script>
const E = e => e.name + ': ' + e.message;
const Q = f => { try { return f(); } catch (e) { return E(e); } };
const S = async b => ({size: b.size, type: b.type, text: await b.text()});
const nb = s => s.replace(/----[A-Za-z0-9]{16,}/g, 'BOUNDARY');
const logOf = async () => (await (await fetch('__log')).json()).filter(e => !e.path.endsWith('hello.txt')).map(e => [e.method, e.path.split('/').pop(), nb(e.headers['content-type'] || ''), JSON.stringify(nb(e.body || ''))].join(' '));
const wait = ms => new Promise(r => setTimeout(r, ms));
</script>"#,
        resources: &[
            Res {
                path: r"echo.txt",
                text: r"echo",
                status: 200,
                delay_ms: 0,
                content_type: None,
                headers: &[(r"Content-Type", r"text/plain")],
                options: Options::None,
            },
            Res {
                path: r"hello.txt",
                text: r"hello",
                status: 200,
                delay_ms: 0,
                content_type: None,
                headers: &[(r"Content-Type", r"text/plain")],
                options: Options::None,
            },
        ],
        queries: &[(
            r"(async () => { const out = [typeof navigator.sendBeacon, navigator.sendBeacon.length, Q(() => navigator.sendBeacon()), Q(() => navigator.sendBeacon('echo.txt')), navigator.sendBeacon('echo.txt', 'str'), navigator.sendBeacon('echo.txt', new Blob(['bl'], {type: 'a/b'})), navigator.sendBeacon('echo.txt', new Blob(['plain'])), navigator.sendBeacon('echo.txt', new URLSearchParams({k: 'v'})), navigator.sendBeacon('echo.txt', new Uint8Array([65, 66])), navigator.sendBeacon('echo.txt'), navigator.sendBeacon('echo.txt', null), navigator.sendBeacon('echo.txt', undefined), navigator.sendBeacon('echo.txt', ''), Q(() => navigator.sendBeacon('http://[bad', 'x')), Q(() => navigator.sendBeacon('data:text/plain,x', 'x')), Q(() => navigator.sendBeacon('ftp://x.test/', 'x'))]; await wait(500); out.push((await logOf()).sort()); return out; })()",
            r#"["function", 1, "TypeError: Failed to execute 'sendBeacon' on 'Navigator': 1 argument required, but only 0 present.", true, true, true, true, true, true, true, true, true, true, "TypeError: Failed to execute 'sendBeacon' on 'Navigator': The URL argument is ill-formed or unsupported.", "TypeError: Failed to execute 'sendBeacon' on 'Navigator': Beacons are only supported over HTTP(S).", "TypeError: Failed to execute 'sendBeacon' on 'Navigator': Beacons are only supported over HTTP(S).", ["POST echo.txt  \"\"", "POST echo.txt  \"\"", "POST echo.txt  \"\"", "POST echo.txt  \"\"", "POST echo.txt  \"AB\"", "POST echo.txt  \"plain\"", "POST echo.txt a/b \"bl\"", "POST echo.txt application/x-www-form-urlencoded;charset=UTF-8 \"k=v\"", "POST echo.txt text/plain;charset=UTF-8 \"\"", "POST echo.txt text/plain;charset=UTF-8 \"str\""]]"#,
        )],
    },
    Case {
        name: "beacon-1",
        html: r#"<!DOCTYPE html><form id=f method=post action=echo.txt>
<input name=a value=1><input name=b value="x y"><input type=checkbox name=c value=on checked><input type=checkbox name=d><input type=radio name=r value=1 checked><input type=radio name=r value=2>
<select name=s><option value=o1>1<option value=o2 selected>2</select><textarea name=t>line1
line2</textarea><input type=file name=file><input type=submit name=go value=Go><button name=btn value=B>B</button><input name=nov disabled value=z><input value=noname><input type=image name=im>
</form><script>
const E = e => e.name + ': ' + e.message;
const Q = f => { try { return f(); } catch (e) { return E(e); } };
const S = async b => ({size: b.size, type: b.type, text: await b.text()});
const nb = s => s.replace(/----[A-Za-z0-9]{16,}/g, 'BOUNDARY');
const logOf = async () => (await (await fetch('__log')).json()).filter(e => !e.path.endsWith('hello.txt')).map(e => [e.method, e.path.split('/').pop(), nb(e.headers['content-type'] || ''), JSON.stringify(nb(e.body || ''))].join(' '));
const wait = ms => new Promise(r => setTimeout(r, ms));
</script>"#,
        resources: &[
            Res {
                path: r"echo.txt",
                text: r"echo",
                status: 200,
                delay_ms: 0,
                content_type: None,
                headers: &[(r"Content-Type", r"text/plain")],
                options: Options::None,
            },
            Res {
                path: r"hello.txt",
                text: r"hello",
                status: 200,
                delay_ms: 0,
                content_type: None,
                headers: &[(r"Content-Type", r"text/plain")],
                options: Options::None,
            },
        ],
        queries: &[(
            r"(async () => { const fd = new FormData(); fd.append('k', 'v'); const out = [navigator.sendBeacon('echo.txt', fd)]; await wait(300); out.push((await logOf()).slice(-1)); return out; })()",
            r#"[true, ["POST echo.txt multipart/form-data; boundary=BOUNDARY \"--BOUNDARY\\r\\nContent-Disposition: form-data; name=\\\"k\\\"\\r\\n\\r\\nv\\r\\n--BOUNDARY--\\r\\n\""]]"#,
        )],
    },
    Case {
        name: "beacon-2",
        html: r#"<!DOCTYPE html><form id=f method=post action=echo.txt>
<input name=a value=1><input name=b value="x y"><input type=checkbox name=c value=on checked><input type=checkbox name=d><input type=radio name=r value=1 checked><input type=radio name=r value=2>
<select name=s><option value=o1>1<option value=o2 selected>2</select><textarea name=t>line1
line2</textarea><input type=file name=file><input type=submit name=go value=Go><button name=btn value=B>B</button><input name=nov disabled value=z><input value=noname><input type=image name=im>
</form><script>
const E = e => e.name + ': ' + e.message;
const Q = f => { try { return f(); } catch (e) { return E(e); } };
const S = async b => ({size: b.size, type: b.type, text: await b.text()});
const nb = s => s.replace(/----[A-Za-z0-9]{16,}/g, 'BOUNDARY');
const logOf = async () => (await (await fetch('__log')).json()).filter(e => !e.path.endsWith('hello.txt')).map(e => [e.method, e.path.split('/').pop(), nb(e.headers['content-type'] || ''), JSON.stringify(nb(e.body || ''))].join(' '));
const wait = ms => new Promise(r => setTimeout(r, ms));
</script>"#,
        resources: &[
            Res {
                path: r"echo.txt",
                text: r"echo",
                status: 200,
                delay_ms: 0,
                content_type: None,
                headers: &[(r"Content-Type", r"text/plain")],
                options: Options::None,
            },
            Res {
                path: r"hello.txt",
                text: r"hello",
                status: 200,
                delay_ms: 0,
                content_type: None,
                headers: &[(r"Content-Type", r"text/plain")],
                options: Options::None,
            },
        ],
        queries: &[(
            r"(async () => { const out = []; out.push(navigator.sendBeacon('echo.txt', new Uint8Array(65536))); out.push(navigator.sendBeacon('echo.txt', 'x')); out.push(navigator.sendBeacon('echo.txt', new Uint8Array(1))); await wait(500); return out; })()",
            r"[true, false, false]",
        )],
    },
    Case {
        name: "beacon-3",
        html: r#"<!DOCTYPE html><form id=f method=post action=echo.txt>
<input name=a value=1><input name=b value="x y"><input type=checkbox name=c value=on checked><input type=checkbox name=d><input type=radio name=r value=1 checked><input type=radio name=r value=2>
<select name=s><option value=o1>1<option value=o2 selected>2</select><textarea name=t>line1
line2</textarea><input type=file name=file><input type=submit name=go value=Go><button name=btn value=B>B</button><input name=nov disabled value=z><input value=noname><input type=image name=im>
</form><script>
const E = e => e.name + ': ' + e.message;
const Q = f => { try { return f(); } catch (e) { return E(e); } };
const S = async b => ({size: b.size, type: b.type, text: await b.text()});
const nb = s => s.replace(/----[A-Za-z0-9]{16,}/g, 'BOUNDARY');
const logOf = async () => (await (await fetch('__log')).json()).filter(e => !e.path.endsWith('hello.txt')).map(e => [e.method, e.path.split('/').pop(), nb(e.headers['content-type'] || ''), JSON.stringify(nb(e.body || ''))].join(' '));
const wait = ms => new Promise(r => setTimeout(r, ms));
</script>"#,
        resources: &[
            Res {
                path: r"echo.txt",
                text: r"echo",
                status: 200,
                delay_ms: 0,
                content_type: None,
                headers: &[(r"Content-Type", r"text/plain")],
                options: Options::None,
            },
            Res {
                path: r"hello.txt",
                text: r"hello",
                status: 200,
                delay_ms: 0,
                content_type: None,
                headers: &[(r"Content-Type", r"text/plain")],
                options: Options::None,
            },
        ],
        queries: &[(
            r"(async () => { const out = []; out.push(navigator.sendBeacon('echo.txt', new Uint8Array(65537))); out.push(navigator.sendBeacon('echo.txt', 'x')); out.push(navigator.sendBeacon('echo.txt', new Uint8Array(65535))); out.push(navigator.sendBeacon('echo.txt', 'x')); out.push(navigator.sendBeacon('echo.txt', 'x')); await wait(500); return out; })()",
            r"[false, true, true, false, false]",
        )],
    },
    Case {
        name: "beacon-4",
        html: r#"<!DOCTYPE html><form id=f method=post action=echo.txt>
<input name=a value=1><input name=b value="x y"><input type=checkbox name=c value=on checked><input type=checkbox name=d><input type=radio name=r value=1 checked><input type=radio name=r value=2>
<select name=s><option value=o1>1<option value=o2 selected>2</select><textarea name=t>line1
line2</textarea><input type=file name=file><input type=submit name=go value=Go><button name=btn value=B>B</button><input name=nov disabled value=z><input value=noname><input type=image name=im>
</form><script>
const E = e => e.name + ': ' + e.message;
const Q = f => { try { return f(); } catch (e) { return E(e); } };
const S = async b => ({size: b.size, type: b.type, text: await b.text()});
const nb = s => s.replace(/----[A-Za-z0-9]{16,}/g, 'BOUNDARY');
const logOf = async () => (await (await fetch('__log')).json()).filter(e => !e.path.endsWith('hello.txt')).map(e => [e.method, e.path.split('/').pop(), nb(e.headers['content-type'] || ''), JSON.stringify(nb(e.body || ''))].join(' '));
const wait = ms => new Promise(r => setTimeout(r, ms));
</script>"#,
        resources: &[
            Res {
                path: r"echo.txt",
                text: r"echo",
                status: 200,
                delay_ms: 0,
                content_type: None,
                headers: &[(r"Content-Type", r"text/plain")],
                options: Options::None,
            },
            Res {
                path: r"hello.txt",
                text: r"hello",
                status: 200,
                delay_ms: 0,
                content_type: None,
                headers: &[(r"Content-Type", r"text/plain")],
                options: Options::None,
            },
        ],
        queries: &[(
            r"(async () => { const out = []; for (let i = 0; i < 10; i++) out.push(navigator.sendBeacon('echo.txt', new Uint8Array(10000))); await wait(800); return out; })()",
            r"[true, true, true, true, true, true, false, false, false, false]",
        )],
    },
    Case {
        name: "beacon-5",
        html: r#"<!DOCTYPE html><form id=f method=post action=echo.txt>
<input name=a value=1><input name=b value="x y"><input type=checkbox name=c value=on checked><input type=checkbox name=d><input type=radio name=r value=1 checked><input type=radio name=r value=2>
<select name=s><option value=o1>1<option value=o2 selected>2</select><textarea name=t>line1
line2</textarea><input type=file name=file><input type=submit name=go value=Go><button name=btn value=B>B</button><input name=nov disabled value=z><input value=noname><input type=image name=im>
</form><script>
const E = e => e.name + ': ' + e.message;
const Q = f => { try { return f(); } catch (e) { return E(e); } };
const S = async b => ({size: b.size, type: b.type, text: await b.text()});
const nb = s => s.replace(/----[A-Za-z0-9]{16,}/g, 'BOUNDARY');
const logOf = async () => (await (await fetch('__log')).json()).filter(e => !e.path.endsWith('hello.txt')).map(e => [e.method, e.path.split('/').pop(), nb(e.headers['content-type'] || ''), JSON.stringify(nb(e.body || ''))].join(' '));
const wait = ms => new Promise(r => setTimeout(r, ms));
</script>"#,
        resources: &[
            Res {
                path: r"echo.txt",
                text: r"echo",
                status: 200,
                delay_ms: 0,
                content_type: None,
                headers: &[(r"Content-Type", r"text/plain")],
                options: Options::None,
            },
            Res {
                path: r"hello.txt",
                text: r"hello",
                status: 200,
                delay_ms: 0,
                content_type: None,
                headers: &[(r"Content-Type", r"text/plain")],
                options: Options::None,
            },
        ],
        queries: &[(
            r"(async () => { const out = []; for (let i = 0; i < 10; i++) out.push(navigator.sendBeacon('echo.txt', new Uint8Array(10000))); await wait(800); out.push(navigator.sendBeacon('echo.txt', new Uint8Array(10000))); return out; })()",
            r"[true, true, true, true, true, true, false, false, false, false, true]",
        )],
    },
];
