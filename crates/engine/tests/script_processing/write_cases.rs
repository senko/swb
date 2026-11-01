// The cases of `tools/probes/web-document-write.json` with the values that
// Chromium 148 gave (`just probe --json tools/probes/web-document-write.json`,
// three equal runs); where swb departs on purpose, `tests/script_processing.rs`
// overrides the value (SWB_VALUES). Generated; do not edit.

use super::{Case, Resource};

pub(crate) const WRITE_CASES: &[Case] = &[
    Case {
        name: "basic",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script>/*s1*/log('s1');document.write('<p id=w1>x</p>');log('s1:p='+!!document.getElementById('w1'));document.write('<di');document.write('v id=w2>y</div>');log('s1:div='+!!document.getElementById('w2'));document.write('<!--');document.write('c-->');log('s1:ret='+document.write('<i id=w3></i>'));</script><script>/*s2*/log('s2:'+kids())</script></body></html>",
        file: true,
        resources: &[],
        queries: &[
            (
                r"L.join(' ')",
                r#""s1:l:s1 s1:p=true:l:s1 s1:div=true:l:s1 s1:ret=undefined:l:s1 s2:script,w1,w2,w3,script:l:s2""#,
            ),
            (r"kids()", r#""script,w1,w2,w3,script""#),
            (
                r"[].map.call(document.body.childNodes,function(x){return x.nodeType+(x.nodeType===8?x.data:'')}).join()",
                r#""1,1,1,8c,1,1""#,
            ),
        ],
    },
    Case {
        name: "nested-inline",
        html: r#"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script>/*s1*/log('s1');document.write('<script>/*n1*/log("n1");document.write("<script>/*n2*/log(\\"n2\\")<\\/script>");log("n1b")<\/script>');log('s1b')</script><script>/*s2*/log('s2')</script></body></html>"#,
        file: true,
        resources: &[],
        queries: &[
            (
                r"L.join(' ')",
                r#""s1:l:s1 n1:l:n1 n2:l:n2 n1b:l:n1 s1b:l:s1 s2:l:s2""#,
            ),
            (r"document.scripts.length", r"5"),
        ],
    },
    Case {
        name: "arguments",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script>/*s1*/document.write('<div id=t>','a','b',1,null,undefined,{toString:function(){return 'o'}},'</div>');document.writeln('<pre id=p>x');document.writeln('y','z');document.write('</pre>');L.push('ret:'+document.write()+','+document.writeln());L.push('sym:'+E(function(){document.write(Symbol())}));L.push('obj:'+E(function(){document.write({toString:function(){throw new RangeError('ts')}})}));L.push('trusted:'+E(function(){document.write(new String('s'))}));</script></body></html>",
        file: true,
        resources: &[],
        queries: &[
            (
                r"L.join(' ')",
                r#""ret:undefined,undefined sym:TypeError: Failed to execute 'write' on 'Document': Cannot convert a Symbol value to a string obj:RangeError: ts trusted:undefined""#,
            ),
            (
                r"document.getElementById('t').textContent",
                r#""ab1nullundefinedo""#,
            ),
            (
                r"JSON.stringify(document.getElementById('p').textContent)",
                r#""\"x\\nyz\\n\"""#,
            ),
            (
                r"[document.write.length,document.writeln.length,document.open.length,document.close.length,document.write.name,document.writeln.name,document.open.name,document.close.name].join()",
                r#""0,0,0,0,write,writeln,open,close""#,
            ),
            (
                r"[Object.getOwnPropertyNames(Document.prototype).filter(function(k){return /^(write|writeln|open|close)$/.test(k)}).sort().join(), Object.getOwnPropertyNames(HTMLDocument.prototype).join()].join('|')",
                r#""close,open,write,writeln|constructor""#,
            ),
        ],
    },
    Case {
        name: "external-written",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script>/*s1*/log('s1');document.write('<script src=w.js><\/script><p id=after>after</p>');log('s1end:after='+!!document.getElementById('after')+',w='+!!document.querySelector('script[src]'));</script><script>/*s2*/log('s2:after='+!!document.getElementById('after'))</script></body></html>",
        file: false,
        resources: &[Resource {
            name: "w.js",
            text: r"log('w');",
            delay_ms: 200,
            content_type: "text/javascript",
        }],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},700)})",
                r#""s1:l:s1 s1end:after=false,w=true:l:s1 w:l:w.js s2:after=true:l:s2""#,
            ),
            (r"kids()", r#""script,script,after,script""#),
        ],
    },
    Case {
        name: "external-two-writes",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script>/*s1*/document.write('<script src=w.js><\/script>foo');document.write('bar');document.write('<b id=b1>baz</b>');log('s1end:'+document.body.textContent.replace(/\s+/g,' '))</script><script>/*s2*/log('s2:'+document.body.textContent.replace(/\s+/g,' '))</script></body></html>",
        file: false,
        resources: &[Resource {
            name: "w.js",
            text: r"log('w:'+document.body.textContent.replace(/\s+/g,' '));",
            delay_ms: 200,
            content_type: "text/javascript",
        }],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},700)})",
                r#""s1end:/*s1*/document.write('<script src=w.js><\\/script>foo');document.write('bar');document.write('<b id=b1>baz</b>');log('s1end:'+document.body.textContent.replace(/\\s+/g,' ')):l:s1 w:/*s1*/document.write('<script src=w.js><\\/script>foo');document.write('bar');document.write('<b id=b1>baz</b>');log('s1end:'+document.body.textContent.replace(/\\s+/g,' ')):l:w.js s2:/*s1*/document.write('<script src=w.js><\\/script>foo');document.write('bar');document.write('<b id=b1>baz</b>');log('s1end:'+document.body.textContent.replace(/\\s+/g,' '))foobarbaz/*s2*/log('s2:'+document.body.textContent.replace(/\\s+/g,' ')):l:s2""#,
            ),
            (
                r"document.body.textContent.replace(/\s+/g,' ')",
                r#""/*s1*/document.write('<script src=w.js><\\/script>foo');document.write('bar');document.write('<b id=b1>baz</b>');log('s1end:'+document.body.textContent.replace(/\\s+/g,' '))foobarbaz/*s2*/log('s2:'+document.body.textContent.replace(/\\s+/g,' '))""#,
            ),
            (r"kids()", r#""script,script,b1,script""#),
        ],
    },
    Case {
        name: "written-mixed",
        html: r#"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script>/*s1*/document.write('<script src=w1.js><\/script><script>/*i1*/log("i1")<\/script><script src=w2.js><\/script><p id=pp></p><script>/*i2*/log("i2:pp="+!!document.getElementById("pp"))<\/script>');log('s1end')</script><script>/*s2*/log('s2')</script></body></html>"#,
        file: false,
        resources: &[
            Resource {
                name: "w1.js",
                text: r"log('w1')",
                delay_ms: 300,
                content_type: "text/javascript",
            },
            Resource {
                name: "w2.js",
                text: r"log('w2')",
                delay_ms: 0,
                content_type: "text/javascript",
            },
        ],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},900)})",
                r#""s1end:l:s1 w1:l:w1.js i1:l:i1 w2:l:w2.js i2:pp=true:l:i2 s2:l:s2""#,
            ),
            (
                r"kids()",
                r#""script,script,script,script,pp,script,script""#,
            ),
        ],
    },
    Case {
        name: "current-script",
        html: r#"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script id=o>/*o*/log('o0');document.write('<script id=wi>/*wi*/log("wi:"+document.currentScript.id+":"+document.currentScript.parentNode.localName+":"+document.scripts.length+":"+document.currentScript.previousElementSibling.id);document.write("<b id=nb></b>");<\/script><script id=we src=cs.js><\/script>');log('o1:'+document.currentScript.id+':'+document.getElementById('wi').nextElementSibling.id);L.push('rs:'+document.readyState);</script><p id=last></p></body></html>"#,
        file: false,
        resources: &[Resource {
            name: "cs.js",
            text: r"log('cs:'+document.currentScript.id+':'+document.scripts.length+':'+document.currentScript.previousElementSibling.id);",
            delay_ms: 100,
            content_type: "text/javascript",
        }],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},500)})",
                r#""o0:l:o wi:wi:body:3:o:l:wi o1:o:nb:l:o rs:loading cs:we:4:nb:l:cs.js""#,
            ),
            (r"kids()", r#""o,wi,nb,we,last""#),
            (r"document.currentScript===null", r"true"),
        ],
    },
    Case {
        name: "async-write-during-parse",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script async src=a.js></script><script defer src=d.js></script><script src=slow.js></script><p id=base>base</p></body></html>",
        file: false,
        resources: &[
            Resource {
                name: "a.js",
                text: r"log('a');document.write('<p id=fromasync>A</p>');log('a-end')",
                delay_ms: 100,
                content_type: "text/javascript",
            },
            Resource {
                name: "d.js",
                text: r"log('d');document.write('<p id=fromdefer>D</p>');log('d-end')",
                delay_ms: 0,
                content_type: "text/javascript",
            },
            Resource {
                name: "slow.js",
                text: r"log('slow')",
                delay_ms: 400,
                content_type: "text/javascript",
            },
        ],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},900)})",
                r#""a:l:a.js a-end:l:a.js slow:l:slow.js d:i:d.js d-end:i:d.js""#,
            ),
            (r"kids()", r#""script,script,script,base""#),
            (
                r"!!document.getElementById('fromasync')+','+!!document.getElementById('fromdefer')+','+!!document.getElementById('base')+','+document.readyState",
                r#""false,false,true,complete""#,
            ),
        ],
    },
    Case {
        name: "async-write-after-parse",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script async src=a.js></script><script defer src=d.js></script><p id=base>base</p></body></html>",
        file: false,
        resources: &[
            Resource {
                name: "a.js",
                text: r"log('a');document.write('<p id=fromasync>A</p>');log('a-end')",
                delay_ms: 300,
                content_type: "text/javascript",
            },
            Resource {
                name: "d.js",
                text: r"log('d');document.write('<p id=fromdefer>D</p>');log('d-end')",
                delay_ms: 100,
                content_type: "text/javascript",
            },
        ],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},900)})",
                r#""d:i:d.js d-end:i:d.js a:i:a.js a-end:i:a.js""#,
            ),
            (r"kids()", r#""script,script,base""#),
            (
                r"!!document.getElementById('fromasync')+','+!!document.getElementById('fromdefer')+','+!!document.getElementById('base')+','+document.readyState",
                r#""false,false,true,complete""#,
            ),
        ],
    },
    Case {
        name: "inserted-script-write",
        html: r#"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script>/*s1*/var s=document.createElement('script');s.textContent='log("ins");document.write("<p id=fi>I</p>");log("ins-end")';document.body.appendChild(s);log('s1:'+!!document.getElementById('fi'));var e=document.createElement('script');e.src='e.js';document.body.appendChild(e);</script><p id=base></p></body></html>"#,
        file: false,
        resources: &[Resource {
            name: "e.js",
            text: r"log('e');document.write('<p id=fe>E</p>');log('e-end')",
            delay_ms: 100,
            content_type: "text/javascript",
        }],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},700)})",
                r#""ins:l:? ins-end:l:? s1:true:l:s1 e:i:e.js e-end:i:e.js""#,
            ),
            (r"kids()", r#""script,script,fi,script,base""#),
        ],
    },
    Case {
        name: "timer-after-load",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script>/*s1*/setTimeout(function(){log('t');document.write('<p id=tw>T</p>');log('t2');},100)</script><p id=orig>orig</p></body></html>",
        file: true,
        resources: &[],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},500)})",
                r#""t:c t2:l""#,
            ),
            (
                r"document.documentElement.outerHTML.replace(/<script>[\s\S]*?<\/script>/g,'S')",
                r#""<html><head></head><body><p id=\"tw\">T</p></body></html>""#,
            ),
            (
                r"document.readyState+','+document.getElementById('orig')+','+!!document.getElementById('tw')",
                r#""loading,null,true""#,
            ),
        ],
    },
    Case {
        name: "timer-while-waiting",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script>/*s1*/setTimeout(function(){log('t');document.write('<p id=tw>T</p>');log('t2');document.close();log('t3:'+document.readyState);},50)</script><script src=slow.js></script><p id=after>after</p></body></html>",
        file: false,
        resources: &[Resource {
            name: "slow.js",
            text: r"log('slow')",
            delay_ms: 300,
            content_type: "text/javascript",
        }],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},700)})",
                r#""t:l t2:l t3:complete:c""#,
            ),
            (r"kids()", r#""tw""#),
            (
                r"document.documentElement.outerHTML.replace(/<script>[\s\S]*?<\/script>/g,'S').slice(0,300)",
                r#""<html><head></head><body><p id=\"tw\">T</p></body></html>""#,
            ),
        ],
    },
    Case {
        name: "load-listener-write",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script>/*s1*/addEventListener('load',function(){log('load');document.write('<p id=lw>L</p>');log('load2');document.close();})</script><p id=orig>orig</p></body></html>",
        file: true,
        resources: &[],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},400)})",
                r#""load:c load2:l""#,
            ),
            (
                r"document.documentElement.outerHTML.replace(/<script>[\s\S]*?<\/script>/g,'S')",
                r#""<html><head></head><body><p id=\"lw\">L</p></body></html>""#,
            ),
        ],
    },
    Case {
        name: "dcl-listener-write",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script>/*s1*/document.addEventListener('DOMContentLoaded',function(){log('dcl');document.write('<p id=dw>D</p>');log('dcl2');document.close();})</script><p id=orig>orig</p></body></html>",
        file: true,
        resources: &[],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},400)})",
                r#""dcl:i dcl2:l""#,
            ),
            (
                r"document.documentElement.outerHTML.replace(/<script>[\s\S]*?<\/script>/g,'S')",
                r#""<html><head></head><body><p id=\"dw\">D</p></body></html>""#,
            ),
        ],
    },
    Case {
        name: "explicit-open-after-load",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script>/*s1*/setTimeout(function(){var d=document.open();log('open:'+(d===document));document.write('<p id=ow>O</p>');log('written');var c=document.close();log('closed:'+c+':'+document.readyState)},100)</script><p id=orig>orig</p></body></html>",
        file: true,
        resources: &[],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},500)})",
                r#""open:true:l written:l closed:undefined:complete:c""#,
            ),
            (
                r"document.documentElement.outerHTML.replace(/<script>[\s\S]*?<\/script>/g,'S')",
                r#""<html><head></head><body><p id=\"ow\">O</p></body></html>""#,
            ),
        ],
    },
    Case {
        name: "open-close-parse",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script>/*s1*/var o=document.open();log('open:'+(o===document));log('open-args:'+(document.open('text/html','replace')===document));var c=document.close();log('close:'+c);log('rs:'+document.readyState);document.write('<p id=w>w</p>');log('w:'+!!document.getElementById('w'));</script><p id=after>after</p><script>/*s2*/log('s2:'+kids())</script></body></html>",
        file: true,
        resources: &[],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},300)})",
                r#""open:true:l:s1 open-args:true:l:s1 close:undefined:l:s1 rs:loading:l:s1 w:true:l:s1 s2:script,w,after,script:l:s2""#,
            ),
            (r"kids()", r#""script,w,after,script""#),
            (r"document.readyState", r#""complete""#),
        ],
    },
    Case {
        name: "open-close-nested",
        html: r#"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script>/*s1*/document.write('<script>/*n*/log("n:open="+(document.open()===document));document.close();log("n:closed");<\/script>');log('s1end')</script><p id=after>after</p></body></html>"#,
        file: true,
        resources: &[],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},300)})",
                r#""n:open=true:l:n n:closed:l:n s1end:l:s1""#,
            ),
            (r"kids()", r#""script,script,after""#),
        ],
    },
    Case {
        name: "error-written",
        html: r#"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script>/*s1*/log('s1');document.write('<script>/*e1*/throw new Error("boom")<\/script>');log('s1b');document.write('<script>\n\n  /*e2*/throw new Error("boom2")<\/script>');log('s1c');document.write('<script>var 1x;<\/script>');log('s1d');document.write('<p>x</p><script>/*e4*/ null.x<\/script>');log('s1e');</script><script>/*s2*/log('s2')</script></body></html>"#,
        file: true,
        resources: &[],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},300)})",
                r#""s1:l:s1 err:Uncaught Error: boom@:1:13 s1b:l:s1 err:Uncaught Error: boom2@:3:15 s1c:l:s1 err:Uncaught SyntaxError: Failed to execute 'write' on 'Document': Invalid or unexpected token@DOC:1:5 s1d:l:s1 err:Uncaught TypeError: Cannot read properties of null (reading 'x')@:1:13 s1e:l:s1 s2:l:s2""#,
            ),
            (
                r"L.join(' ')",
                r#""s1:l:s1 err:Uncaught Error: boom@:1:13 s1b:l:s1 err:Uncaught Error: boom2@:3:15 s1c:l:s1 err:Uncaught SyntaxError: Failed to execute 'write' on 'Document': Invalid or unexpected token@DOC:1:5 s1d:l:s1 err:Uncaught TypeError: Cannot read properties of null (reading 'x')@:1:13 s1e:l:s1 s2:l:s2""#,
            ),
        ],
    },
    Case {
        name: "error-written-syntax",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><p>x</p>
<script>/*s1*/

log('s1');document.write('<script>\n\n  var 1x;<\/script>');log('s1b');window.onerror=function(m,s,l,c,e){L.push('err2:'+m+'@'+n(s)+':'+l+':'+c+':'+(e&&e.constructor.name)+':'+(e&&e.stack&&n(e.stack).split('\n').join('|')))};document.write('<script>\n let a = ;<\/script>');log('s1c')</script></body></html>",
        file: true,
        resources: &[],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},300)})",
                r#""s1:l:s1 err:Uncaught SyntaxError: Failed to execute 'write' on 'Document': Invalid or unexpected token@DOC:3:7 s1b:l:s1 err2:Uncaught SyntaxError: Failed to execute 'write' on 'Document': Unexpected token ';'@DOC:2:10:SyntaxError:SyntaxError: Unexpected token ';'|    at DOC:11:235 s1c:l:s1""#,
            ),
            (
                r"L.join(' ')",
                r#""s1:l:s1 err:Uncaught SyntaxError: Failed to execute 'write' on 'Document': Invalid or unexpected token@DOC:3:7 s1b:l:s1 err2:Uncaught SyntaxError: Failed to execute 'write' on 'Document': Unexpected token ';'@DOC:2:10:SyntaxError:SyntaxError: Unexpected token ';'|    at DOC:11:235 s1c:l:s1""#,
            ),
        ],
    },
    Case {
        name: "error-written-stack",
        html: r#"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script>/*s1*/document.write('<script>function f(){return new Error("st").stack}\nlog("stack:"+n(f()).split("\\n").join("|"));<\/script>');</script></body></html>"#,
        file: true,
        resources: &[],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},300)})",
                r#""stack:Error: st|    at f (<anonymous>:1:21)|    at <anonymous>:2:16|    at DOC:8:46:l:?""#,
            ),
            (
                r"L.join(' ')",
                r#""stack:Error: st|    at f (<anonymous>:1:21)|    at <anonymous>:2:16|    at DOC:8:46:l:?""#,
            ),
        ],
    },
    Case {
        name: "write-in-written-external",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script>/*s1*/document.write('<script src=w.js><\/script><p id=tail>tail</p>');log('s1end')</script><p id=orig></p></body></html>",
        file: false,
        resources: &[Resource {
            name: "w.js",
            text: r#"log('w');document.write('<p id=fw>fw</p><script>/*fi*/log("fw-inl")<\/script>');log('w-end')"#,
            delay_ms: 100,
            content_type: "text/javascript",
        }],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},600)})",
                r#""s1end:l:s1 w:l:w.js fw-inl:l:fi w-end:l:w.js""#,
            ),
            (r"kids()", r#""script,script,fw,script,tail,orig""#),
        ],
    },
    Case {
        name: "write-in-blocking-external",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script src=b.js></script><p id=tail>tail</p><script>/*s2*/log('s2')</script></body></html>",
        file: false,
        resources: &[Resource {
            name: "b.js",
            text: r#"log('b');document.write('<p id=fb>fb</p><script>/*bi*/log("b-inl")<\/script>');log('b-end')"#,
            delay_ms: 100,
            content_type: "text/javascript",
        }],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},600)})",
                r#""b:l:b.js b-inl:l:bi b-end:l:b.js s2:l:s2""#,
            ),
            (r"kids()", r#""script,fb,script,tail,script""#),
        ],
    },
    Case {
        name: "head-write",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script><script>/*s0*/document.write('<meta name=x content=1><title>T</title><p id=p1>hello');log('s0')</script><link rel=x><title>t2</title></head><body id=b><p id=p2>bye</p></body></html>",
        file: true,
        resources: &[],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},200)})",
                r#""s0:l:s0""#,
            ),
            (
                r"document.head.innerHTML.replace(/<script>[\s\S]*?<\/script>/g,'S')",
                r#""SS<meta name=\"x\" content=\"1\"><title>T</title>""#,
            ),
            (
                r"document.body.id+'|'+document.body.innerHTML.replace(/<script>[\s\S]*?<\/script>/g,'S')",
                r#""b|<p id=\"p1\">hello<link rel=\"x\"><title>t2</title></p><p id=\"p2\">bye</p>""#,
            ),
            (r"document.title", r#""T""#),
        ],
    },
    Case {
        name: "recursion",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script>/*r*/window.k=(window.k||0)+1;if(window.k<=2000){try{document.write('<script>'+document.currentScript.text+'<\/script>')}catch(e){log('caught:'+e.name+':'+window.k)}}</script><p id=after></p></body></html>",
        file: true,
        resources: &[],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},500)})",
                r#""""#,
            ),
            (
                r"[window.k, L.length, L.slice(0,3).join(' ')].join('|')",
                r#""22|0|""#,
            ),
            (
                r"document.scripts.length+','+kids().split(',').length",
                r#""23,23""#,
            ),
        ],
    },
    Case {
        name: "recursion-detail",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script>/*r*/var k=window.k=(window.k||0)+1;L.push('in'+k);if(k<=40){document.write('<script>'+document.currentScript.text+'<\/script>')}L.push('out'+k+':'+document.scripts.length);</script><p id=after></p></body></html>",
        file: true,
        resources: &[],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},500)})",
                r#""in1 in2 in3 in4 in5 in6 in7 in8 in9 in10 in11 in12 in13 in14 in15 in16 in17 in18 in19 in20 in21 in22 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23""#,
            ),
            (
                r"L.join(' ')",
                r#""in1 in2 in3 in4 in5 in6 in7 in8 in9 in10 in11 in12 in13 in14 in15 in16 in17 in18 in19 in20 in21 in22 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23 out22:23""#,
            ),
            (
                r"document.scripts.length+','+kids().split(',').length",
                r#""23,23""#,
            ),
        ],
    },
    Case {
        name: "microtasks",
        html: r#"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script>/*s1*/Promise.resolve().then(function(){log('m1')});document.write('<script>/*w*/Promise.resolve().then(function(){log("mw")});log("w")<\/script>');log('s1end')</script><script>/*s2*/log('s2')</script></body></html>"#,
        file: true,
        resources: &[],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},300)})",
                r#""w:l:w s1end:l:s1 m1:l:s1 mw:l:s1 s2:l:s2""#,
            ),
            (
                r"L.join(' ')",
                r#""w:l:w s1end:l:s1 m1:l:s1 mw:l:s1 s2:l:s2""#,
            ),
        ],
    },
    Case {
        name: "split-script",
        html: r#"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script>/*s1*/document.write('<scr');document.write('ipt>log("x1");');log('mid');document.write('log("x2")</scr');log('mid2');document.write('ipt><p id=q></p>');log('end:'+!!document.getElementById('q'))</script></body></html>"#,
        file: true,
        resources: &[],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},200)})",
                r#""mid:l:s1 mid2:l:s1 x1:l:? x2:l:? end:true:l:s1""#,
            ),
            (
                r"L.join(' ')",
                r#""mid:l:s1 mid2:l:s1 x1:l:? x2:l:? end:true:l:s1""#,
            ),
            (r"kids()", r#""script,script,q""#),
        ],
    },
    Case {
        name: "open-element",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script>/*s1*/document.write('<div id=d><span id=sp>')</script><p id=in>in</p><script>/*s2*/log('s2:'+document.getElementById('in').parentNode.id+':'+document.getElementById('sp').parentNode.id)</script></span></div></body></html>",
        file: true,
        resources: &[],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},200)})",
                r#""s2:sp:d:l:s2""#,
            ),
            (r"L.join(' ')", r#""s2:sp:d:l:s2""#),
            (r"kids()", r#""script,d""#),
        ],
    },
    Case {
        name: "after-body",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><p id=a>a</p></body></html><script>/*s1*/document.write('<p id=late>late</p>');log('s1')</script><p id=after>after</p>",
        file: true,
        resources: &[],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},200)})",
                r#""s1:l:s1""#,
            ),
            (r"L.join(' ')", r#""s1:l:s1""#),
            (r"kids()", r#""a,script,late,after""#),
        ],
    },
    Case {
        name: "sheet-blocked",
        html: r#"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><link rel=stylesheet href=s.css><script>/*s1*/log('s1');document.write('<p id=sw>x</p><script>/*i*/log("i")<\/script>');log('s1end:'+!!document.getElementById('sw'))</script><script>/*s2*/log('s2')</script></body></html>"#,
        file: false,
        resources: &[Resource {
            name: "s.css",
            text: r"p{margin:0}",
            delay_ms: 200,
            content_type: "text/css",
        }],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},600)})",
                r#""s1:l:s1 i:l:i s1end:true:l:s1 s2:l:s2""#,
            ),
            (r"L.join(' ')", r#""s1:l:s1 i:l:i s1end:true:l:s1 s2:l:s2""#),
            (r"kids()", r#""link,script,sw,script,script""#),
        ],
    },
    Case {
        name: "ajax-in-anchor",
        html: r#"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><a id=l href=#>title<span id=sp></span><script>/*s1*/document.write('\x3Cscript type="text/javascript" src="aj.js?x=1&y=2">\x3C/script>');log('s1end')</script></a><a id=l2 href=#>second<script>/*s2*/document.write('\x3Cscript src="aj.js?x=3">\x3C/script>');log('s2end')</script></a><p id=after></p></body></html>"#,
        file: false,
        resources: &[Resource {
            name: "aj.js",
            text: r"log('aj:'+document.currentScript.src.slice(-6))",
            delay_ms: 100,
            content_type: "text/javascript",
        }],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},700)})",
                r#""s1end:l:s1 aj:=1&y=2:l:aj.js?x=1&y=2 s2end:l:s2 aj:js?x=3:l:aj.js?x=3""#,
            ),
            (
                r"[].map.call(document.querySelectorAll('a'),function(a){return a.id+'['+[].map.call(a.childNodes,function(c){return c.nodeType===3?'t':c.id||c.localName+(c.src?'('+n(c.src)+')':'')}).join()+']'}).join(' ')",
                r#""l[t,sp,script,script(O/aj.js?x=1&y=2)] l2[t,script,script(O/aj.js?x=3)]""#,
            ),
        ],
    },
    Case {
        name: "other-document",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];
function nm(c){if(!c)return '';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}
function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}
function n(s){return String(s).split(location.href).join('DOC').split(location.origin).join('O').replace(/\/case-[0-9]+\//g,'/')}
function E(f){try{var v=f();return v===undefined?'undefined':v}catch(e){return (e instanceof DOMException?'DOMException '+e.name:e.constructor.name)+': '+e.message}}
function kids(){return [].map.call(document.body.children,function(e){return e.id||e.localName}).join()}
window.onerror=function(m,s,l,c){L.push('err:'+m+'@'+n(s)+':'+l+':'+c)};
</script></head><body><script>/*s1*/var d=document.implementation.createHTMLDocument('x');log('rs:'+d.readyState);log('w:'+E(function(){d.write('<p id=q>q</p>')}));log('body:'+(d.body?d.body.innerHTML:'none'));log('open:'+(d.open()===d));log('close:'+d.close());</script></body></html>",
        file: true,
        resources: &[],
        queries: &[
            (
                r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},200)})",
                r#""rs:loading:l:s1 w:undefined:l:s1 body:<p id=\"q\">q</p>:l:s1 open:true:l:s1 close:undefined:l:s1""#,
            ),
            (
                r"L.join(' ')",
                r#""rs:loading:l:s1 w:undefined:l:s1 body:<p id=\"q\">q</p>:l:s1 open:true:l:s1 close:undefined:l:s1""#,
            ),
        ],
    },
];
