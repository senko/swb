// The cases of `tools/probes/web-image-option.json` with the values that
// Chromium 148 gave (`just probe tools/probes/web-image-option.json`, three
// runs, all equal); where swb departs on purpose, the test overrides the value.
// Generated.

use crate::common::Case;

/// The page of the cases.
pub(crate) const PAGE: &str = r"<!DOCTYPE html><html><head><title>t</title></head><body><div id=a class=k></div><select id=sel><option>1<option>2</select><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }
const N = 'http://www.w3.org/2000/svg';
function X(t) { return document.createElementNS(N, t); }
</script></body></html>";

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "image-01",
        html: PAGE,
        queries: &[
            (
                r"E(() => [typeof Image, Image.name, Image.length, Image.prototype === HTMLImageElement.prototype, Object.getPrototypeOf(Image) === Function.prototype, Object.getOwnPropertyNames(Image).join(), Image.toString()].join('|'))",
                r#""function|Image|0|true|true|length,name,prototype|function Image() { [native code] }""#,
            ),
            (
                r"E(() => { const d = Object.getOwnPropertyDescriptor(Image, 'prototype'); return [d.writable, d.enumerable, d.configurable].join(); })",
                r#""false,false,false""#,
            ),
            (
                r"E(() => { const d = Object.getOwnPropertyDescriptor(window, 'Image'); return [d.writable, d.enumerable, d.configurable, typeof d.value].join(); })",
                r#""true,false,true,function""#,
            ),
            (
                r"E(() => { const d = Object.getOwnPropertyDescriptor(Image, 'name'); const l = Object.getOwnPropertyDescriptor(Image, 'length'); return [d.writable, d.enumerable, d.configurable, l.writable, l.enumerable, l.configurable].join(); })",
                r#""false,false,true,false,false,true""#,
            ),
            (
                r"E(() => { const i = new Image(); return [i.constructor.name, i instanceof HTMLImageElement, i instanceof Image, Object.prototype.toString.call(i), i.localName, i.namespaceURI.slice(-5), i.ownerDocument === document, i.isConnected, i.parentNode, i.attributes.length, i.src, i.width, i.height, i.outerHTML].join('|'); })",
                r#""HTMLImageElement|true|true|[object HTMLImageElement]|img|xhtml|true|false||0||0|0|<img>""#,
            ),
            (
                r"E(() => { const i = new Image(3, 4); return [i.getAttribute('width'), i.getAttribute('height'), i.width, i.height, i.outerHTML].join('|'); })",
                r#""3|4|3|4|<img width=\"3\" height=\"4\">""#,
            ),
            (
                r"E(() => { const i = new Image(3); return [i.getAttribute('width'), i.getAttribute('height'), i.outerHTML].join('|'); })",
                r#""3||<img width=\"3\">""#,
            ),
            (
                r"E(() => { const i = new Image(undefined, 5); return [i.getAttribute('width'), i.getAttribute('height')].join('|'); })",
                r#""0|5""#,
            ),
            (
                r"E(() => { const i = new Image(undefined, undefined); return [i.getAttribute('width'), i.getAttribute('height')].join('|'); })",
                r#""|""#,
            ),
            (
                r"E(() => { const i = new Image(null, null); return [i.getAttribute('width'), i.getAttribute('height')].join('|'); })",
                r#""0|0""#,
            ),
            (
                r"E(() => ['7', '8.9', -1, 4294967296, NaN, Infinity, 1e10, '', 'x', true, {valueOf() { return 6; }}].map((v) => { const i = new Image(v, v); return i.getAttribute('width') + '/' + i.getAttribute('height'); }).join(' '))",
                r#""7/7 8/8 0/0 0/0 0/0 0/0 1410065408/1410065408 0/0 0/0 1/1 6/6""#,
            ),
            (
                r"E(() => new Image(Symbol('x')))",
                r#""TypeError: Failed to construct 'Image': Cannot convert a Symbol value to a number""#,
            ),
            (
                r"E(() => new Image(1, Symbol('x')))",
                r#""TypeError: Failed to construct 'Image': Cannot convert a Symbol value to a number""#,
            ),
            (
                r"E(() => new Image({valueOf() { throw new RangeError('boom'); }}))",
                r#""RangeError: boom""#,
            ),
            (
                r"E(() => Image())",
                r#""TypeError: Failed to construct 'Image': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => Image(1, 2))",
                r#""TypeError: Failed to construct 'Image': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => Image.call(document))",
                r#""TypeError: Failed to construct 'Image': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => Image.apply(null, []))",
                r#""TypeError: Failed to construct 'Image': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => Reflect.apply(Image, undefined, [1]))",
                r#""TypeError: Failed to construct 'Image': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => { const i = new Image(); i.src = 'a.png'; return [i.getAttribute('src'), i.src.slice(-5), i.complete].join('|'); })",
                r#""a.png|a.png|false""#,
            ),
            (
                r"E(() => { const i = new Image(5, 6); document.getElementById('a').appendChild(i); const r = [i.isConnected, i.parentNode.id, document.getElementsByTagName('img').length, i.width]; i.remove(); return r.join(); })",
                r#""true,a,1,5""#,
            ),
            (
                r"E(() => { class A extends Image {} ; const a = new A(1, 2); return [a instanceof A, a instanceof HTMLImageElement, a.constructor.name, a.getAttribute('width')].join(); })",
                r#""true,true,A,1""#,
            ),
            (
                r"E(() => { const i = Reflect.construct(Image, [1, 2], Object); return [Object.getPrototypeOf(i) === Object.prototype, i.constructor.name].join(); })",
                r#""true,Object""#,
            ),
            (
                r"E(() => { function F() {} F.prototype = Array.prototype; const i = Reflect.construct(Image, [], F); return [Object.getPrototypeOf(i) === Array.prototype, i instanceof HTMLImageElement].join(); })",
                r#""true,false""#,
            ),
            (
                r"E(() => { const p = new Proxy(Image, {}); const i = new p(1, 2); return [i.constructor.name, i.getAttribute('width')].join(); })",
                r#""HTMLImageElement,1""#,
            ),
            (
                r"E(() => { const i = new Image(); const j = new Image(); return [i === j, i.isEqualNode(j), i.cloneNode().constructor.name].join(); })",
                r#""false,true,HTMLImageElement""#,
            ),
            (
                r"E(() => { const i = new Image(); return Object.getOwnPropertyNames(i).join() + '|' + Object.keys(i).join(); })",
                r#""|""#,
            ),
            (
                r"E(() => { Image.expando = 1; const r = Image.expando; delete Image.expando; return r; })",
                r"1",
            ),
            (
                r"E(() => { Image.prototype = {}; return Image.prototype === HTMLImageElement.prototype; })",
                r"true",
            ),
            (
                r"E(() => { 'use strict'; Image.prototype = {}; })",
                r#""TypeError: Cannot assign to read only property 'prototype' of function 'function Image() { [native code] }'""#,
            ),
        ],
    },
    Case {
        name: "image-02",
        html: PAGE,
        queries: &[
            (
                r"E(() => { Image.name = 'x'; return Image.name; })",
                r#""Image""#,
            ),
            (
                r"E(() => [Image.hasOwnProperty('call'), Image === window.Image, typeof Image.prototype.constructor, Image.prototype.constructor === HTMLImageElement, HTMLImageElement.name].join())",
                r#""false,true,function,true,HTMLImageElement""#,
            ),
            (
                r"E(() => { const d = document.implementation.createHTMLDocument(''); const i = new Image(); const j = d.adoptNode(i); return [i.ownerDocument === document, j === i, j.ownerDocument === d].join(); })",
                r#""false,true,true""#,
            ),
            (
                r"E(() => Object.getOwnPropertyNames(window).filter((n) => n === 'Image' || n === 'Option').sort().join())",
                r#""Image,Option""#,
            ),
        ],
    },
    Case {
        name: "option-01",
        html: PAGE,
        queries: &[
            (
                r"E(() => [typeof Option, Option.name, Option.length, Option.prototype === HTMLOptionElement.prototype, Object.getOwnPropertyNames(Option).join(), Option.toString()].join('|'))",
                r#""function|Option|0|true|length,name,prototype|function Option() { [native code] }""#,
            ),
            (
                r"E(() => { const d = Object.getOwnPropertyDescriptor(window, 'Option'); return [d.writable, d.enumerable, d.configurable, typeof d.value].join(); })",
                r#""true,false,true,function""#,
            ),
            (
                r"E(() => { const d = Object.getOwnPropertyDescriptor(Option, 'prototype'); return [d.writable, d.enumerable, d.configurable].join(); })",
                r#""false,false,false""#,
            ),
            (
                r"E(() => { const o = new Option(); return [o.constructor.name, o instanceof HTMLOptionElement, Object.prototype.toString.call(o), o.localName, o.ownerDocument === document, o.isConnected, o.attributes.length, o.childNodes.length, o.text, JSON.stringify(o.value), JSON.stringify(o.label), o.defaultSelected, o.disabled, o.index, o.outerHTML].join('|'); })",
                r#""HTMLOptionElement|true|[object HTMLOptionElement]|option|true|false|0|0||\"\"|\"\"|false|false|0|<option></option>""#,
            ),
            (
                r"E(() => { const o = new Option('a'); return [o.text, o.value, o.label, o.getAttribute('value'), o.childNodes.length, o.firstChild.nodeType, o.firstChild.data, o.outerHTML].join('|'); })",
                r#""a|a|a||1|3|a|<option>a</option>""#,
            ),
            (
                r"E(() => { const o = new Option('a', 'b'); return [o.text, o.value, o.getAttribute('value'), o.outerHTML].join('|'); })",
                r#""a|b|b|<option value=\"b\">a</option>""#,
            ),
            (
                r"E(() => { const o = new Option('a', undefined); return [o.value, o.getAttribute('value'), o.outerHTML].join('|'); })",
                r#""a||<option>a</option>""#,
            ),
            (
                r"E(() => { const o = new Option('a', null); return [o.value, o.getAttribute('value'), o.outerHTML].join('|'); })",
                r#""null|null|<option value=\"null\">a</option>""#,
            ),
            (
                r"E(() => { const o = new Option('a', ''); return [JSON.stringify(o.value), JSON.stringify(o.getAttribute('value')), o.outerHTML].join('|'); })",
                r#""\"\"|\"\"|<option value=\"\">a</option>""#,
            ),
            (
                r"E(() => { const o = new Option('', 'v'); return [o.text === '', o.childNodes.length, o.value, o.outerHTML].join('|'); })",
                r#""true|0|v|<option value=\"v\"></option>""#,
            ),
            (
                r"E(() => { const o = new Option(undefined, 'v'); return [JSON.stringify(o.text), o.childNodes.length, o.value, o.outerHTML].join('|'); })",
                r#""\"\"|0|v|<option value=\"v\"></option>""#,
            ),
            (
                r"E(() => { const o = new Option(null); return [JSON.stringify(o.text), o.childNodes.length, o.outerHTML].join('|'); })",
                r#""\"null\"|1|<option>null</option>""#,
            ),
            (
                r"E(() => { const o = new Option(5, 6); return [o.text, o.value, o.outerHTML].join('|'); })",
                r#""5|6|<option value=\"6\">5</option>""#,
            ),
            (
                r"E(() => { const o = new Option({toString() { return ' x  y\n'; }}); return [JSON.stringify(o.text), JSON.stringify(o.firstChild.data), o.outerHTML].join('|'); })",
                r#""\"x y\"|\" x  y\\n\"|<option> x  y\n</option>""#,
            ),
            (
                r"E(() => { const o = new Option('a', 'b', true); return [o.defaultSelected, o.getAttribute('selected'), o.outerHTML].join('|'); })",
                r#""true||<option value=\"b\" selected=\"\">a</option>""#,
            ),
            (
                r"E(() => { const o = new Option('a', 'b', false); return [o.defaultSelected, o.getAttribute('selected'), o.outerHTML].join('|'); })",
                r#""false||<option value=\"b\">a</option>""#,
            ),
            (
                r"E(() => { const o = new Option('a', 'b', 0, 0); return [o.defaultSelected, o.getAttribute('selected')].join('|'); })",
                r#""false|""#,
            ),
            (
                r"E(() => { const o = new Option('a', 'b', '', 'x'); return [o.defaultSelected, o.getAttribute('selected')].join('|'); })",
                r#""false|""#,
            ),
            (
                r"E(() => { const o = new Option('a', 'b', undefined, undefined); return [o.defaultSelected, o.getAttribute('selected')].join('|'); })",
                r#""false|""#,
            ),
            (
                r"E(() => { const o = new Option('a', 'b', {}, {}); return [o.defaultSelected, o.getAttribute('selected')].join('|'); })",
                r#""true|""#,
            ),
            (
                r"E(() => { const o = new Option('a', 'b', true, true); return [o.defaultSelected, o.getAttribute('selected'), o.outerHTML].join('|'); })",
                r#""true||<option value=\"b\" selected=\"\">a</option>""#,
            ),
            (
                r"E(() => { const o = new Option('a', undefined, true); return [o.value, o.getAttribute('value'), o.defaultSelected, o.outerHTML].join('|'); })",
                r#""a||true|<option selected=\"\">a</option>""#,
            ),
            (
                r"E(() => { const o = new Option('a', undefined, true, true); return [o.value, o.getAttribute('value'), o.defaultSelected, o.outerHTML].join('|'); })",
                r#""a||true|<option selected=\"\">a</option>""#,
            ),
            (
                r"E(() => { const o = new Option(undefined, undefined, true); return [o.text, o.value, o.getAttribute('value'), o.defaultSelected, o.outerHTML].join('|'); })",
                r#""|||true|<option selected=\"\"></option>""#,
            ),
            (
                r"E(() => { const o = new Option('a', 'b', undefined, undefined); return o.outerHTML; })",
                r#""<option value=\"b\">a</option>""#,
            ),
            (
                r"E(() => { const i = new Image(1, 2, 3, 4); return i.outerHTML; })",
                r#""<img width=\"1\" height=\"2\">""#,
            ),
            (
                r"E(() => new Option(Symbol('x')))",
                r#""TypeError: Failed to construct 'Option': Cannot convert a Symbol value to a string""#,
            ),
            (
                r"E(() => new Option('a', Symbol('x')))",
                r#""TypeError: Failed to construct 'Option': Cannot convert a Symbol value to a string""#,
            ),
            (
                r"E(() => new Option('a', 'b', Symbol('x')))",
                r#""[object HTMLOptionElement]""#,
            ),
            (
                r"E(() => Option())",
                r#""TypeError: Failed to construct 'Option': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
        ],
    },
    Case {
        name: "option-02",
        html: PAGE,
        queries: &[
            (
                r"E(() => Option('a'))",
                r#""TypeError: Failed to construct 'Option': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => Option.call(document, 'a'))",
                r#""TypeError: Failed to construct 'Option': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => { class A extends Option {} ; const a = new A('t', 'v'); return [a instanceof A, a instanceof HTMLOptionElement, a.constructor.name, a.value].join(); })",
                r#""true,true,A,v""#,
            ),
            (
                r"E(() => { const o = Reflect.construct(Option, ['t'], Object); return [Object.getPrototypeOf(o) === Object.prototype, o.constructor.name].join(); })",
                r#""true,Object""#,
            ),
            (
                r"E(() => { const p = new Proxy(Option, {}); const o = new p('t', 'v'); return [o.constructor.name, o.value].join(); })",
                r#""HTMLOptionElement,v""#,
            ),
            (
                r"E(() => { const o = new Option('a'); o.text = 'b'; return [o.text, o.firstChild.data, o.childNodes.length].join(); })",
                r#""b,b,1""#,
            ),
            (
                r"E(() => { const o = new Option(); o.text = 'b'; o.value = 'c'; o.label = 'd'; return [o.text, o.value, o.label, o.outerHTML].join('|'); })",
                r#""b|c|d|<option value=\"c\" label=\"d\">b</option>""#,
            ),
            (
                r"E(() => { const o = new Option('a', 'b'); const p = new Option('a', 'b'); return [o === p, o.isEqualNode(p), o.cloneNode(true).value].join(); })",
                r#""false,true,b""#,
            ),
            (
                r"E(() => Object.getOwnPropertyNames(new Option('a')).join() + '|' + Object.keys(new Option('a')).join())",
                r#""|""#,
            ),
            (
                r"E(() => { const s = document.createElement('select'); s.add(new Option('x', '1')); s.add(new Option('y', '2')); s.appendChild(new Option('z')); return [s.options.length, s.length, s.children.length, s.options[1].value, s.options[2].value, s.options[2].text].join(); })",
                r#""3,3,3,2,z,z""#,
            ),
            (
                r"E(() => { const s = document.createElement('select'); s.options[0] = new Option('x', '1'); s.options[1] = new Option('y', '2'); s.options[s.options.length] = new Option('z'); return [s.length, s.options[2].text, s.options[0].index, s.options[2].index].join(); })",
                r#""3,z,0,2""#,
            ),
            (
                r"E(() => { const s = document.createElement('select'); s[0] = new Option('x', '1'); return [s.length, s[0].value].join(); })",
                r#""1,1""#,
            ),
            (
                r"E(() => { const s = document.createElement('select'); for (let i = 0; i < 4; i++) s.options.add(new Option('o' + i, i)); return [s.length, s.options.item(3).value, Array.prototype.map.call(s.options, (o) => o.text).join()].join(); })",
                r#""4,3,o0,o1,o2,o3""#,
            ),
            (
                r"E(() => { const g = document.createElement('optgroup'); g.appendChild(new Option('a')); return [g.children.length, g.firstChild.constructor.name].join(); })",
                r#""1,HTMLOptionElement""#,
            ),
            (
                r"E(() => { const d = document.implementation.createHTMLDocument(''); const o = new Option('a'); const p = d.adoptNode(o); return [o.ownerDocument === document, p.ownerDocument === d].join(); })",
                r#""false,true""#,
            ),
            (
                r"E(() => { const o = new Option('a', 'b'); return [o.form, o.index, typeof o.disabled].join(); })",
                r#"",0,boolean""#,
            ),
            (
                r"E(() => { Option.expando = 1; return Option.expando; })",
                r"1",
            ),
            (
                r"E(() => { function F() {} F.prototype = 5; const o = Reflect.construct(Image, [], F); const p = Object.getPrototypeOf(o); return [p === Object.prototype, p === HTMLImageElement.prototype, o.localName].join(); })",
                r#""true,false,""#,
            ),
            (
                r"E(() => { function F() {} F.prototype = null; const o = Reflect.construct(Option, [], F); const p = Object.getPrototypeOf(o); return [p === Object.prototype, p === HTMLOptionElement.prototype, o.localName].join(); })",
                r#""true,false,""#,
            ),
            (
                r"E(() => { const F = new Proxy(function () {}, { get(t, k) { if (k === 'prototype') throw new Error('boom'); return t[k]; } }); let m; try { Reflect.construct(Image, [], F); } catch (e) { m = e.message; } return m; })",
                r#""boom""#,
            ),
        ],
    },
];
