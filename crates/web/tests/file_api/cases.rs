// The cases of tools/probes/web-blob-formdata.json with the values of Chromium 148.0
// that need no network. Generated from `swbtools probe --json` (three runs, all
// equal); do not edit.

use crate::common::Case;

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
        queries: &[(
            r"(() => { const fd = new FormData(); fd.append('a', '1'); fd.append('a', '2'); fd.append('b', new Blob(['x'], {type: 't/u'})); fd.append('c', new Blob(['y']), 'fn.txt'); fd.append('d', new File(['z'], 'zz.txt')); fd.set('a', 'three'); fd.append('e', 5); return [fd.get('a'), fd.getAll('a'), fd.has('a'), fd.has('q'), fd.get('q'), fd.getAll('q'), [...fd].map(([k, v]) => k + '=' + (typeof v === 'string' ? v : v.constructor.name + ':' + v.name + ':' + v.type + ':' + v.size)), [...fd.keys()], [...fd.values()].length, (fd.delete('a'), [...fd.keys()]), Object.prototype.toString.call(fd), Q(() => fd.append('a')), Q(() => fd.append('a', 'b', 'c')), Q(() => fd.append('a', {})), Q(() => fd.get()), Q(() => { fd.forEach((v, k, o) => { if (o !== fd) throw 1; }); return 'ok'; }), Q(() => fd.set('q', new Blob(['u']), 'nm').toString()), fd.get('q').name, fd.get('b').name]; })()",
            r#"["three", ["three"], true, false, null, [], ["a=three", "b=File:blob:t/u:1", "c=File:fn.txt::1", "d=File:zz.txt::1", "e=5"], ["a", "b", "c", "d", "e"], 5, ["b", "c", "d", "e"], "[object FormData]", "TypeError: Failed to execute 'append' on 'FormData': 2 arguments required, but only 1 present.", "TypeError: Failed to execute 'append' on 'FormData': parameter 2 is not of type 'Blob'.", null, "TypeError: Failed to execute 'get' on 'FormData': 1 argument required, but only 0 present.", "ok", "TypeError: Cannot read properties of undefined (reading 'toString')", "nm", "blob"]"#,
        )],
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
];
