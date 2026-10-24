// The cases of `tools/probes/web-scripts.json` with the values that
// Chromium 148 gave (`just probe --json tools/probes/web-scripts.json`);
// where swb departs on purpose, `tests/script_processing.rs` overrides
// the value (SWB_VALUES). Generated; do not edit.

use super::{Case, Resource};

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "order",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script><script>/*i1*/log('i1');setTimeout(function(){log('t0')},0);Promise.resolve().then(function(){log('m1')})</script><link rel=stylesheet href=s.css><script>/*i2*/log('i2')</script><script src=b1.js></script><script nomodule>/*n*/log('nomodule')</script><script type=text/plain>/*p*/log('plain')</script><script type='text/javascript; charset=utf-8'>/*q*/log('param')</script><script src=missing.js></script></head><body><p>x</p><script>/*i3*/log('i3')</script><script defer src=d1.js></script><script defer src=d2.js></script><script>/*i4*/log('i4')</script></body></html>",
        file: false,
        resources: &[
            Resource {
                name: "s.css",
                text: r"p { margin: 0 }",
                delay_ms: 200,
                content_type: "text/css",
            },
            Resource {
                name: "b1.js",
                text: r"log('b1')",
                delay_ms: 200,
                content_type: "text/javascript",
            },
            Resource {
                name: "d1.js",
                text: r"log('d1')",
                delay_ms: 100,
                content_type: "text/javascript",
            },
            Resource {
                name: "d2.js",
                text: r"log('d2');Promise.resolve().then(function(){log('m-d2')})",
                delay_ms: 0,
                content_type: "text/javascript",
            },
        ],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},800)})",
            r#""i1:l:i1 m1:l:i1 t0:l i2:l:i2 b1:l:b1.js load-b1.js:l error-missing.js:l i3:l:i3 i4:l:i4 rsc:i d1:i:d1.js load-d1.js:i d2:i:d2.js m-d2:i:d2.js load-d2.js:i dcl:i rsc:c load:c pageshow:c""#,
        )],
    },
    Case {
        name: "timer-vs-dcl",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script><script>/*i1*/log('i1');setTimeout(function(){log('t0')},0);Promise.resolve().then(function(){log('m1')})</script></head><body><p>x</p><script>/*i2*/log('i2');setTimeout(function(){log('t0b')},0)</script></body></html>",
        file: true,
        resources: &[],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},400)})",
            r#""i1:l:i1 m1:l:i1 i2:l:i2 rsc:i dcl:i rsc:c load:c pageshow:c t0:c t0b:c""#,
        )],
    },
    Case {
        name: "timer-vs-dcl-loading",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script><script>/*i1*/setTimeout(function(){log('t0')},0)</script><script defer src=d.js></script></head><body><p>x</p><script>/*i2*/log('i2')</script></body></html>",
        file: false,
        resources: &[Resource {
            name: "d.js",
            text: r"log('d');setTimeout(function(){log('t-d')},0);Promise.resolve().then(function(){log('m-d')})",
            delay_ms: 200,
            content_type: "text/javascript",
        }],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},400)})",
            r#""i2:l:i2 t0:l rsc:i d:i:d.js m-d:i:d.js load-d.js:i dcl:i rsc:c load:c pageshow:c t-d:c""#,
        )],
    },
    Case {
        name: "timer-vs-dcl-image",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script><script>/*i1*/setTimeout(function(){log('t0')},0)</script></head><body><img src=i.png><p>x</p><script>/*i2*/log('i2')</script></body></html>",
        file: false,
        resources: &[Resource {
            name: "i.png",
            text: r"x",
            delay_ms: 200,
            content_type: "image/png",
        }],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},400)})",
            r#""i2:l:i2 t0:l rsc:i dcl:i rsc:c load:c pageshow:c""#,
        )],
    },
    Case {
        name: "inserted",
        html: r#"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script><script>/*i1*/var s=document.createElement('script');s.src='x1.js';log('x1async '+s.async);document.head.appendChild(s);var s2=document.createElement('script');s2.src='x2.js';s2.async=false;log('x2async '+s2.async);document.head.appendChild(s2);var s3=document.createElement('script');s3.src='x3.js';s3.async=false;document.head.appendChild(s3);var s4=document.createElement('script');s4.text="/*x4*/log('x4')";log('before');document.head.appendChild(s4);log('after');log('parser async '+document.currentScript.async);</script></head><body><p>x</p><script>/*i2*/log('i2')</script></body></html>"#,
        file: false,
        resources: &[
            Resource {
                name: "x1.js",
                text: r"log('x1')",
                delay_ms: 150,
                content_type: "text/javascript",
            },
            Resource {
                name: "x2.js",
                text: r"log('x2')",
                delay_ms: 400,
                content_type: "text/javascript",
            },
            Resource {
                name: "x3.js",
                text: r"log('x3')",
                delay_ms: 0,
                content_type: "text/javascript",
            },
        ],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},800)})",
            r#""x1async true:l:i1 x2async false:l:i1 before:l:i1 x4:l:x4 after:l:i1 parser async false:l:i1 i2:l:i2 rsc:i dcl:i x1:i:x1.js load-x1.js:i x2:i:x2.js load-x2.js:i x3:i:x3.js load-x3.js:i rsc:c load:c pageshow:c""#,
        )],
    },
    Case {
        name: "inserted-error",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script><script>/*i1*/var s=document.createElement('script');s.src='missing.js';document.head.appendChild(s);</script></head><body><p>x</p></body></html>",
        file: true,
        resources: &[],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},400)})",
            r#""rsc:i dcl:i error-missing.js:i rsc:c load:c pageshow:c""#,
        )],
    },
    Case {
        name: "inserted-after-load",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script><script>/*i1*/addEventListener('load',function(){var s=document.createElement('script');s.src='y1.js';document.head.appendChild(s);log('in load')})</script></head><body></body></html>",
        file: false,
        resources: &[Resource {
            name: "y1.js",
            text: r"log('y1')",
            delay_ms: 100,
            content_type: "text/javascript",
        }],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},400)})",
            r#""rsc:i dcl:i rsc:c load:c in load:c pageshow:c y1:c:y1.js load-y1.js:c""#,
        )],
    },
    Case {
        name: "load-waits-async-chain",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script><script async src=c1.js></script></head><body></body></html>",
        file: false,
        resources: &[
            Resource {
                name: "c1.js",
                text: r"log('c1');var s=document.createElement('script');s.src='c2.js';document.head.appendChild(s)",
                delay_ms: 50,
                content_type: "text/javascript",
            },
            Resource {
                name: "c2.js",
                text: r"log('c2')",
                delay_ms: 200,
                content_type: "text/javascript",
            },
        ],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},400)})",
            r#""rsc:i dcl:i c1:i:c1.js load-c1.js:i c2:i:c2.js load-c2.js:i rsc:c load:c pageshow:c""#,
        )],
    },
    Case {
        name: "defer-waits-sheet",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script><link rel=stylesheet href=s.css><script defer src=d1.js></script><script async src=a1.js></script></head><body><p>x</p></body></html>",
        file: false,
        resources: &[
            Resource {
                name: "s.css",
                text: r"p { margin: 0 }",
                delay_ms: 300,
                content_type: "text/css",
            },
            Resource {
                name: "d1.js",
                text: r"log('d1')",
                delay_ms: 0,
                content_type: "text/javascript",
            },
            Resource {
                name: "a1.js",
                text: r"log('a1')",
                delay_ms: 0,
                content_type: "text/javascript",
            },
        ],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},400)})",
            r#""rsc:i a1:i:a1.js load-a1.js:i d1:i:d1.js load-d1.js:i dcl:i rsc:c load:c pageshow:c""#,
        )],
    },
    Case {
        name: "sheet-blocks-inline-in-body",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script><script>/*i0*/setTimeout(function(){log('t0')},0)</script></head><body><link rel=stylesheet href=s.css><script>/*i1*/log('i1');setTimeout(function(){log('t1')},0)</script><link rel=stylesheet href=p.css media=print><script>/*i2*/log('i2')</script></body></html>",
        file: false,
        resources: &[
            Resource {
                name: "s.css",
                text: r"body { margin: 0 }",
                delay_ms: 300,
                content_type: "text/css",
            },
            Resource {
                name: "p.css",
                text: r"body { color: red }",
                delay_ms: 600,
                content_type: "text/css",
            },
        ],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},900)})",
            r#""t0:l i1:l:i1 i2:l:i2 t1:l rsc:i dcl:i rsc:c load:c pageshow:c""#,
        )],
    },
    Case {
        name: "ignored-attributes-and-errors",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script><script defer>/*i1*/log('i1')</script><script async>/*i2*/log('i2')</script><script src=''>/*e*/log('empty src')</script><script src='  '>/*w*/</script><script src='http://['>/*u*/</script><script type=application/json>/*j*/{}</script></head><body><script>/*i3*/log('i3')</script></body></html>",
        file: true,
        resources: &[],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},400)})",
            r#""i1:l:i1 i2:l:i2 i3:l:i3 rsc:i dcl:i rsc:c load:c pageshow:c error-doc:c error-doc:c error-[:c""#,
        )],
    },
    Case {
        name: "types",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script><script language=javascript>/*a*/log('a')</script><script type='text/javascript1.5'>/*b*/log('b')</script><script type=' application/x-javascript '>/*c*/log('c')</script><script language=vbscript>/*e*/log('e')</script><script src=g.js nomodule></script><script src=g.js type=text/x-foo></script><script defer async src=i.js></script><script for=window event=onload>/*f1*/log('for-window')</script><script for=x event=onload>/*f2*/log('for-x')</script></head><body></body></html>",
        file: false,
        resources: &[
            Resource {
                name: "g.js",
                text: r"log('g')",
                delay_ms: 0,
                content_type: "text/javascript",
            },
            Resource {
                name: "i.js",
                text: r"log('i')",
                delay_ms: 100,
                content_type: "text/javascript",
            },
        ],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},400)})",
            r#""a:l:a b:l:b c:l:c for-window:l:f1 rsc:i dcl:i i:i:i.js load-i.js:i rsc:c load:c pageshow:c""#,
        )],
    },
    Case {
        name: "response-types",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script><script src=a.js></script><script src=b.js></script><script src=c.js></script><script src=d.js></script><script src=e.js></script></head><body></body></html>",
        file: false,
        resources: &[
            Resource {
                name: "a.js",
                text: r"log('a')",
                delay_ms: 0,
                content_type: "text/plain",
            },
            Resource {
                name: "b.js",
                text: r"log('b')",
                delay_ms: 0,
                content_type: "image/png",
            },
            Resource {
                name: "c.js",
                text: r"log('c')",
                delay_ms: 0,
                content_type: "text/csv",
            },
            Resource {
                name: "d.js",
                text: r"log('d é')",
                delay_ms: 0,
                content_type: "text/javascript; charset=utf-8",
            },
            Resource {
                name: "e.js",
                text: r"log('e')",
                delay_ms: 0,
                content_type: "video/mp4",
            },
        ],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},400)})",
            r#""a:l:a.js load-a.js:l error-b.js:l error-c.js:l d é:l:d.js load-d.js:l error-e.js:l rsc:i dcl:i rsc:c load:c pageshow:c""#,
        )],
    },
    Case {
        name: "module-not-run",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script><script type=module>/*m*/log('module')</script><script src=e.js type=module></script></head><body><script>/*i*/log('i')</script></body></html>",
        file: false,
        resources: &[Resource {
            name: "e.js",
            text: r"log('external module')",
            delay_ms: 0,
            content_type: "text/javascript",
        }],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},400)})",
            r#""i:l:i rsc:i module:i external module:i load-e.js:i dcl:i rsc:c load:c pageshow:c""#,
        )],
    },
    Case {
        name: "element-events",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script><script>/*h2*/var EV=[];document.addEventListener('load',function(e){if(e.target.localName==='script')EV.push([e.type,e.bubbles,e.cancelable,e.isTrusted,e.eventPhase,e.constructor.name])},true);document.addEventListener('error',function(e){if(e.target.localName==='script')EV.push([e.type,e.bubbles,e.cancelable,e.isTrusted,e.eventPhase,e.constructor.name])},true);var W=[];addEventListener('load',function(e){W.push([e.type,e.bubbles,e.cancelable,e.isTrusted,e.target===document,e.currentTarget===window,e.constructor.name])});addEventListener('pageshow',function(e){W.push([e.type,e.bubbles,e.cancelable,e.persisted,e.target===document,e.constructor.name])});document.addEventListener('DOMContentLoaded',function(e){W.push([e.type,e.bubbles,e.cancelable,e.isTrusted,e.target===document,e.constructor.name])});addEventListener('DOMContentLoaded',function(e){W.push(['window-dcl',e.eventPhase])});document.addEventListener('readystatechange',function(e){W.push([e.type,e.bubbles,e.cancelable,document.readyState])});</script><script src=ok.js></script><script src=missing.js></script></head><body></body></html>",
        file: false,
        resources: &[Resource {
            name: "ok.js",
            text: r"1",
            delay_ms: 0,
            content_type: "text/javascript",
        }],
        queries: &[
            (
                r"EV",
                r#"[["load", false, false, true, 1, "Event"], ["error", false, false, true, 1, "Event"]]"#,
            ),
            (
                r"W",
                r#"[["readystatechange", false, false, "interactive"], ["DOMContentLoaded", true, false, true, true, "Event"], ["window-dcl", 3], ["readystatechange", false, false, "complete"], ["load", false, false, true, true, true, "Event"], ["pageshow", true, true, false, true, "PageTransitionEvent"]]"#,
            ),
        ],
    },
    Case {
        name: "current-script",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script><script>/*c1*/var C=[nm(document.currentScript), document.currentScript===document.head.lastChild, document.currentScript.constructor.name];setTimeout(function(){C.push(String(document.currentScript))},50);</script></head><body><script src=cs.js></script></body></html>",
        file: false,
        resources: &[Resource {
            name: "cs.js",
            text: r"C.push(nm(document.currentScript))",
            delay_ms: 0,
            content_type: "text/javascript",
        }],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(C.concat([String(document.currentScript)]))},200)})",
            r#"["c1", true, "HTMLScriptElement", "cs.js", "null", "null"]"#,
        )],
    },
    Case {
        name: "script-properties",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script><script src=x.js async defer type=text/javascript nomodule></script><script>/*s2*/var s1=document.head.lastChild.previousSibling;var s2=document.head.lastChild;</script></head><body></body></html>",
        file: false,
        resources: &[Resource {
            name: "x.js",
            text: r"1",
            delay_ms: 0,
            content_type: "text/javascript",
        }],
        queries: &[
            (
                r"[s1.src.slice(s1.src.lastIndexOf('/')), s1.async, s1.defer, s1.type, s1.noModule, s2.src, s2.async, s2.defer, s2.type, s2.noModule, s2.text.slice(0, 6)]",
                r#"["/x.js", true, true, "text/javascript", true, "", false, false, "", false, "/*s2*/"]"#,
            ),
            (
                r"(function(){var s=document.createElement('script');var r=[s.async];s.async=false;r.push(s.async);s.async=true;r.push(s.async);s.async=false;r.push(s.async);s.src='y.js';r.push(s.src.slice(s.src.lastIndexOf('/')));s.text='a<b';r.push(s.text, s.childNodes.length, s.firstChild.data);s.text='';r.push(s.childNodes.length);return r})()",
                r#"[true, false, true, false, "/y.js", "a<b", 1, "a<b", 0]"#,
            ),
            (
                r"[Object.prototype.toString.call(document.createElement('script')), document.createElement('SCRIPT').localName, document.createElement('Div').tagName]",
                r#"["[object HTMLScriptElement]", "script", "DIV"]"#,
            ),
            (
                r"(function(){try{document.createElement('1a');return 'no error'}catch(e){return e.name+': '+e.message}})()",
                r#""InvalidCharacterError: Failed to execute 'createElement' on 'Document': The tag name provided ('1a') is not a valid name.""#,
            ),
            (
                r"(function(){try{document.createElement('a b');return 'no error'}catch(e){return e.name}})()",
                r#""InvalidCharacterError""#,
            ),
            (
                r"[document.createElement('a@b').localName, document.createElement('x-y').localName]",
                r#"["a@b", "x-y"]"#,
            ),
        ],
    },
    Case {
        name: "read-below",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script><script>/*r1*/var R=[document.body===null, document.documentElement.childNodes.length, document.head.childNodes.length, document.readyState]</script></head><body><p>late</p><script>/*r2*/R.push(document.body.childNodes.length, document.body.firstChild.nodeName, document.body.lastChild.nodeName)</script><div>after</div></body></html>",
        file: true,
        resources: &[],
        queries: &[
            (r"R", r#"[true, 1, 2, "loading", 2, "P", "SCRIPT"]"#),
            (
                r"[document.body.childNodes.length, document.readyState]",
                r#"[3, "complete"]"#,
            ),
        ],
    },
    Case {
        name: "detached-and-moved",
        html: r#"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script></head><body><script>/*i1*/var d=document.createElement('div');var s=document.createElement('script');s.text="/*x1*/log('x1')";d.appendChild(s);log('detached');document.body.appendChild(d);log('connected');document.body.appendChild(s);log('moved');var e=document.createElement('script');document.body.appendChild(e);log('empty');e.text="/*x2*/log('x2')";log('text set');var t=document.createElement('script');t.type='text/plain';t.text="/*x3*/log('x3')";document.body.appendChild(t);t.type='';document.body.appendChild(t);log('retyped');var n=document.createElement('script');n.text="/*x4*/log('x4');document.body.appendChild(n2)";var n2=document.createElement('script');n2.text="/*x5*/log('x5')";document.body.appendChild(n);log('nested');</script></body></html>"#,
        file: true,
        resources: &[],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},400)})",
            r#""detached:l:i1 x1:l:x1 connected:l:i1 moved:l:i1 empty:l:i1 x2:l:x2 text set:l:i1 x3:l:x3 retyped:l:i1 x4:l:x4 x5:l:x5 nested:l:i1 rsc:i dcl:i rsc:c load:c pageshow:c""#,
        )],
    },
    Case {
        name: "errors-inline-positions",
        html: r"<!DOCTYPE html><html><head><script>var E=[];addEventListener('error',function(e){E.push([e.message,e.lineno,e.colno,e.filename===document.URL])})</script></head><body>
<p>x</p>   <script>/*e1*/throw new Error('first')</script>
<script>
  var a = 1;
    null.x;
</script><script>function f(){throw new TypeError('in f')}</script>
<script>  setTimeout(f, 50)</script><script>  }</script><script src=bad.js></script></body></html>",
        file: false,
        resources: &[Resource {
            name: "bad.js",
            text: r"
  throw new RangeError('ext')",
            delay_ms: 0,
            content_type: "text/javascript",
        }],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(E)},400)})",
            r#"[["Uncaught Error: first", 2, 32, true], ["Uncaught TypeError: Cannot read properties of null (reading 'x')", 5, 10, true], ["Uncaught SyntaxError: Unexpected token '}'", 7, 47, true], ["Uncaught RangeError: ext", 2, 9, false], ["Uncaught TypeError: in f", 6, 37, true]]"#,
        )],
    },
    Case {
        name: "blocking-timers",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script><script>/*i1*/setTimeout(function(){log('t-i1')},0)</script><script src=b.js></script><script>/*i2*/log('i2');setTimeout(function(){log('t-i2')},0)</script><p>x</p><script>/*i3*/log('i3')</script></head><body></body></html>",
        file: false,
        resources: &[Resource {
            name: "b.js",
            text: r"log('b');setTimeout(function(){log('t-b')},0)",
            delay_ms: 150,
            content_type: "text/javascript",
        }],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},800)})",
            r#""t-i1:l b:l:b.js load-b.js:l t-b:l i2:l:i2 i3:l:i3 t-i2:l rsc:i dcl:i rsc:c load:c pageshow:c""#,
        )],
    },
    Case {
        name: "end-http-script-timer-last",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script></head><body><script src=b.js></script><script>/*i2*/setTimeout(function(){log('t')},0)</script></body></html>",
        file: false,
        resources: &[
            Resource {
                name: "b.js",
                text: r"log('b')",
                delay_ms: 150,
                content_type: "text/javascript",
            },
            Resource {
                name: "c.js",
                text: r"log('c')",
                delay_ms: 150,
                content_type: "text/javascript",
            },
            Resource {
                name: "s.css",
                text: r"p{margin:0}",
                delay_ms: 150,
                content_type: "text/css",
            },
        ],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},800)})",
            r#""b:l:b.js load-b.js:l t:l rsc:i dcl:i rsc:c load:c pageshow:c""#,
        )],
    },
    Case {
        name: "end-http-script-p-timer",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script></head><body><script src=b.js></script><p>x</p><script>/*i2*/log('i2')</script><script>/*i3*/setTimeout(function(){log('t')},0)</script><p>y</p></body></html>",
        file: false,
        resources: &[
            Resource {
                name: "b.js",
                text: r"log('b')",
                delay_ms: 150,
                content_type: "text/javascript",
            },
            Resource {
                name: "c.js",
                text: r"log('c')",
                delay_ms: 150,
                content_type: "text/javascript",
            },
            Resource {
                name: "s.css",
                text: r"p{margin:0}",
                delay_ms: 150,
                content_type: "text/css",
            },
        ],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},800)})",
            r#""b:l:b.js load-b.js:l i2:l:i2 t:l rsc:i dcl:i rsc:c load:c pageshow:c""#,
        )],
    },
    Case {
        name: "end-http-sheet-timer",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script></head><body><link rel=stylesheet href=s.css><script>/*i1*/log('i1');setTimeout(function(){log('t')},0)</script><p>y</p></body></html>",
        file: false,
        resources: &[
            Resource {
                name: "b.js",
                text: r"log('b')",
                delay_ms: 150,
                content_type: "text/javascript",
            },
            Resource {
                name: "c.js",
                text: r"log('c')",
                delay_ms: 150,
                content_type: "text/javascript",
            },
            Resource {
                name: "s.css",
                text: r"p{margin:0}",
                delay_ms: 150,
                content_type: "text/css",
            },
        ],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},800)})",
            r#""i1:l:i1 t:l rsc:i dcl:i rsc:c load:c pageshow:c""#,
        )],
    },
    Case {
        name: "end-http-nowait-control",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script></head><body><script>/*i1*/setTimeout(function(){log('t')},0)</script><p>x</p></body></html>",
        file: false,
        resources: &[
            Resource {
                name: "b.js",
                text: r"log('b')",
                delay_ms: 150,
                content_type: "text/javascript",
            },
            Resource {
                name: "c.js",
                text: r"log('c')",
                delay_ms: 150,
                content_type: "text/javascript",
            },
            Resource {
                name: "s.css",
                text: r"p{margin:0}",
                delay_ms: 150,
                content_type: "text/css",
            },
        ],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},800)})",
            r#""t:l rsc:i dcl:i rsc:c load:c pageshow:c""#,
        )],
    },
    Case {
        name: "end-http-head-script-timer",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script><script src=b.js></script></head><body><script>/*i2*/setTimeout(function(){log('t')},0)</script></body></html>",
        file: false,
        resources: &[
            Resource {
                name: "b.js",
                text: r"log('b')",
                delay_ms: 150,
                content_type: "text/javascript",
            },
            Resource {
                name: "c.js",
                text: r"log('c')",
                delay_ms: 150,
                content_type: "text/javascript",
            },
            Resource {
                name: "s.css",
                text: r"p{margin:0}",
                delay_ms: 150,
                content_type: "text/css",
            },
        ],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},800)})",
            r#""b:l:b.js load-b.js:l t:l rsc:i dcl:i rsc:c load:c pageshow:c""#,
        )],
    },
    Case {
        name: "end-file-script-timer-last",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script></head><body><script src=b.js></script><script>/*i1*/setTimeout(function(){log('t')},0)</script></body></html>",
        file: true,
        resources: &[Resource {
            name: "b.js",
            text: r"log('b')
",
            delay_ms: 0,
            content_type: "text/javascript",
        }],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},800)})",
            r#""b:l:b.js load-b.js:l t:l rsc:i dcl:i rsc:c load:c pageshow:c""#,
        )],
    },
    Case {
        name: "end-file-defer-timer",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script></head><body><script defer src=b.js></script><script>/*i1*/setTimeout(function(){log('t')},0)</script></body></html>",
        file: true,
        resources: &[Resource {
            name: "b.js",
            text: r"log('b')
",
            delay_ms: 0,
            content_type: "text/javascript",
        }],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},800)})",
            r#""rsc:i t:i b:i:b.js load-b.js:i dcl:i rsc:c load:c pageshow:c""#,
        )],
    },
    Case {
        name: "end-file-async-missing",
        html: r"<!DOCTYPE html><html><head><script>/*h*/var L=[];function nm(c){if(!c)return '';if(c.src===document.URL)return 'doc';if(c.src)return c.src.slice(c.src.lastIndexOf('/')+1);var t=c.text;return t.slice(0,2)==='/*'?t.slice(2,t.indexOf('*/')):'?'}function log(x){var c=document.currentScript;L.push(x+':'+document.readyState[0]+(c?':'+nm(c):''))}document.addEventListener('readystatechange',function(){log('rsc')});document.addEventListener('DOMContentLoaded',function(){log('dcl')});addEventListener('load',function(e){log('load')});addEventListener('pageshow',function(e){log('pageshow')});document.addEventListener('load',function(e){if(e.target.localName==='script')log('load-'+nm(e.target))},true);document.addEventListener('error',function(e){if(e.target.localName==='script')log('error-'+nm(e.target))},true);</script></head><body><script>var s=document.createElement('script');s.async=true;s.src='missing.js';document.body.appendChild(s);setTimeout(function(){log('t')},0)</script></body></html>",
        file: true,
        resources: &[],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(L.join(' '))},800)})",
            r#""rsc:i dcl:i t:i error-missing.js:i rsc:c load:c pageshow:c""#,
        )],
    },
    Case {
        name: "nl-lf-before",
        html: r"<!DOCTYPE html><html><head><script>var E=[];addEventListener('error',function(e){E.push([e.lineno,e.colno])})</script></head><body>


<script>null.x</script>",
        file: true,
        resources: &[],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(E)},500)})",
            r"[[4, 14]]",
        )],
    },
    Case {
        name: "nl-lf-inside",
        html: r"<!DOCTYPE html><html><head><script>var E=[];addEventListener('error',function(e){E.push([e.lineno,e.colno])})</script></head><body><script>


null.x</script>",
        file: true,
        resources: &[],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(E)},500)})",
            r"[[4, 6]]",
        )],
    },
    Case {
        name: "nl-lf-two",
        html: r"<!DOCTYPE html><html><head><script>var E=[];addEventListener('error',function(e){E.push([e.lineno,e.colno])})</script></head><body><script>null.a</script>

<script>null.x</script>",
        file: true,
        resources: &[],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(E)},500)})",
            r"[[1, 145], [3, 14]]",
        )],
    },
    Case {
        name: "nl-cr-before",
        html: "<!DOCTYPE html><html><head><script>var E=[];addEventListener('error',function(e){E.push([e.lineno,e.colno])})</script></head><body>\r\r\r<script>null.x</script>",
        file: true,
        resources: &[],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(E)},500)})",
            r"[[1, 148]]",
        )],
    },
    Case {
        name: "nl-cr-inside",
        html: "<!DOCTYPE html><html><head><script>var E=[];addEventListener('error',function(e){E.push([e.lineno,e.colno])})</script></head><body><script>\r\r\rnull.x</script>",
        file: true,
        resources: &[],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(E)},500)})",
            r"[[4, 6]]",
        )],
    },
    Case {
        name: "nl-cr-two",
        html: "<!DOCTYPE html><html><head><script>var E=[];addEventListener('error',function(e){E.push([e.lineno,e.colno])})</script></head><body><script>null.a</script>\r\r<script>null.x</script>",
        file: true,
        resources: &[],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(E)},500)})",
            r"[[1, 145], [1, 170]]",
        )],
    },
    Case {
        name: "nl-crlf-before",
        html: "<!DOCTYPE html><html><head><script>var E=[];addEventListener('error',function(e){E.push([e.lineno,e.colno])})</script></head><body>\r\n\r\n\r\n<script>null.x</script>",
        file: true,
        resources: &[],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(E)},500)})",
            r"[[4, 14]]",
        )],
    },
    Case {
        name: "nl-crlf-inside",
        html: "<!DOCTYPE html><html><head><script>var E=[];addEventListener('error',function(e){E.push([e.lineno,e.colno])})</script></head><body><script>\r\n\r\n\r\nnull.x</script>",
        file: true,
        resources: &[],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(E)},500)})",
            r"[[4, 6]]",
        )],
    },
    Case {
        name: "nl-crlf-two",
        html: "<!DOCTYPE html><html><head><script>var E=[];addEventListener('error',function(e){E.push([e.lineno,e.colno])})</script></head><body><script>null.a</script>\r\n\r\n<script>null.x</script>",
        file: true,
        resources: &[],
        queries: &[(
            r"new Promise(function(r){setTimeout(function(){r(E)},500)})",
            r"[[1, 145], [3, 14]]",
        )],
    },
];
