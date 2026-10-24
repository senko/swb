// The cases of `tools/probes/web-dom-tree.json` with the values that
// Chromium 148 gave (`just probe tools/probes/web-dom-tree.json`); where
// swb departs on purpose, `tests/dom.rs` overrides the value (SWB_VALUES).

use crate::common::Case;

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "descriptors",
        html: r"<!DOCTYPE html><html><head><title>  Hello 
  world </title></head><body><div id=a>x<!--c--><span id=s>y</span></div><svg id=g><circle></circle></svg><template id=t><p>q</p></template><script>function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return e.constructor.name + ': ' + e.message; } }function D(o, names) { return names.map((n) => { const d = Object.getOwnPropertyDescriptor(o, n); if (!d) return n + ':none'; const k = d.get || d.set ? (d.get ? 'get' : '') + (d.set ? 'set' : '') : typeof d.value + (d.writable ? 'W' : ''); return n + ':' + k + (d.enumerable ? 'E' : '') + (d.configurable ? 'C' : ''); }).join(' '); }function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }function O(o, names) { return Object.getOwnPropertyNames(o).filter((n) => names.includes(n)).join(' '); }</script>",
        queries: &[
            (
                r#"D(Node.prototype, ["nodeType", "nodeName", "baseURI", "isConnected", "ownerDocument", "parentNode", "parentElement", "childNodes", "firstChild", "lastChild", "previousSibling", "nextSibling", "ELEMENT_NODE", "DOCUMENT_POSITION_DISCONNECTED", "contains", "getRootNode", "hasChildNodes", "isSameNode", "constructor"])"#,
                r#""nodeType:getEC nodeName:getEC baseURI:getEC isConnected:getEC ownerDocument:getEC parentNode:getEC parentElement:getEC childNodes:getEC firstChild:getEC lastChild:getEC previousSibling:getEC nextSibling:getEC ELEMENT_NODE:numberE DOCUMENT_POSITION_DISCONNECTED:numberE contains:functionWEC getRootNode:functionWEC hasChildNodes:functionWEC isSameNode:functionWEC constructor:functionWC""#,
            ),
            (
                r#"D(Element.prototype, ["namespaceURI", "prefix", "localName", "tagName", "firstElementChild", "lastElementChild", "childElementCount", "previousElementSibling", "nextElementSibling", "constructor"])"#,
                r#""namespaceURI:getEC prefix:getEC localName:getEC tagName:getEC firstElementChild:getEC lastElementChild:getEC childElementCount:getEC previousElementSibling:getEC nextElementSibling:getEC constructor:functionWC""#,
            ),
            (
                r#"D(Document.prototype, ["URL", "documentURI", "compatMode", "characterSet", "charset", "inputEncoding", "contentType", "doctype", "documentElement", "firstElementChild", "lastElementChild", "childElementCount", "title", "head", "body", "constructor"])"#,
                r#""URL:getEC documentURI:getEC compatMode:getEC characterSet:getEC charset:getEC inputEncoding:getEC contentType:getEC doctype:getEC documentElement:getEC firstElementChild:getEC lastElementChild:getEC childElementCount:getEC title:getsetEC head:getEC body:getsetEC constructor:functionWC""#,
            ),
            (
                r#"D(CharacterData.prototype, ["data", "length", "previousElementSibling", "nextElementSibling", "constructor"])"#,
                r#""data:getsetEC length:getEC previousElementSibling:getEC nextElementSibling:getEC constructor:functionWC""#,
            ),
            (
                r#"D(DocumentType.prototype, ["name", "publicId", "systemId", "constructor"])"#,
                r#""name:getEC publicId:getEC systemId:getEC constructor:functionWC""#,
            ),
            (
                r#"D(window, ["window", "self", "document", "frames", "top", "parent", "console", "Node", "Window", "globalThis"])"#,
                r#""window:getE self:getsetEC document:getE frames:getsetEC top:getE parent:getsetEC console:objectWC Node:functionWC Window:functionWC globalThis:objectWC""#,
            ),
            (
                r#"O(Node.prototype, ["nodeType", "nodeName", "baseURI", "isConnected", "ownerDocument", "parentNode", "parentElement", "childNodes", "firstChild", "lastChild", "previousSibling", "nextSibling", "ELEMENT_NODE", "DOCUMENT_POSITION_DISCONNECTED", "contains", "getRootNode", "hasChildNodes", "isSameNode", "constructor"])"#,
                r#""nodeType nodeName baseURI isConnected ownerDocument parentNode parentElement childNodes firstChild lastChild previousSibling nextSibling ELEMENT_NODE DOCUMENT_POSITION_DISCONNECTED contains getRootNode hasChildNodes isSameNode constructor""#,
            ),
            (
                r#"O(Element.prototype, ["namespaceURI", "prefix", "localName", "tagName", "firstElementChild", "lastElementChild", "childElementCount", "previousElementSibling", "nextElementSibling", "constructor"])"#,
                r#""namespaceURI prefix localName tagName firstElementChild lastElementChild childElementCount previousElementSibling nextElementSibling constructor""#,
            ),
            (
                r#"O(Document.prototype, ["URL", "documentURI", "compatMode", "characterSet", "charset", "inputEncoding", "contentType", "doctype", "documentElement", "firstElementChild", "lastElementChild", "childElementCount", "title", "head", "body", "constructor"])"#,
                r#""URL documentURI compatMode characterSet charset inputEncoding contentType doctype documentElement title body head firstElementChild lastElementChild childElementCount constructor""#,
            ),
            (
                r#"O(CharacterData.prototype, ["data", "length", "previousElementSibling", "nextElementSibling", "constructor"])"#,
                r#""data length previousElementSibling nextElementSibling constructor""#,
            ),
            (
                r"D(NodeList.prototype, ['item', 'length', 'forEach', 'keys', 'values', 'entries', 'constructor'])",
                r#""item:functionWEC length:getEC forEach:functionWEC keys:functionWEC values:functionWEC entries:functionWEC constructor:functionWC""#,
            ),
            (
                r"['childNodes', 'firstChild', 'parentElement'].map((n) => { const g = Object.getOwnPropertyDescriptor(Node.prototype, n).get; return g.name + '/' + g.length; }).concat([Node.prototype.contains.length, Node.prototype.getRootNode.length, Node.prototype.isSameNode.length, Node.prototype.hasChildNodes.length]).join(' ')",
                r#""get childNodes/0 get firstChild/0 get parentElement/0 1 0 1 0""#,
            ),
            (
                r"[Object.getOwnPropertyDescriptor(Document.prototype, 'title').set.name, Object.getOwnPropertyDescriptor(Document.prototype, 'body').set.name, Object.getOwnPropertyDescriptor(CharacterData.prototype, 'data').set.name].join(' ')",
                r#""set title set body set data""#,
            ),
            (
                r"[Node.length, Element.length, CharacterData.length, Document.length, Text.length, Comment.length, DocumentType.length, HTMLElement.length, Window.length, NodeList.length].join(' ')",
                r#""0 0 0 0 0 0 0 0 0 0""#,
            ),
        ],
    },
    Case {
        name: "chains",
        html: r"<!DOCTYPE html><html><head><title>  Hello 
  world </title></head><body><div id=a>x<!--c--><span id=s>y</span></div><svg id=g><circle></circle></svg><template id=t><p>q</p></template><script>function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return e.constructor.name + ': ' + e.message; } }function D(o, names) { return names.map((n) => { const d = Object.getOwnPropertyDescriptor(o, n); if (!d) return n + ':none'; const k = d.get || d.set ? (d.get ? 'get' : '') + (d.set ? 'set' : '') : typeof d.value + (d.writable ? 'W' : ''); return n + ':' + k + (d.enumerable ? 'E' : '') + (d.configurable ? 'C' : ''); }).join(' '); }function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }function O(o, names) { return Object.getOwnPropertyNames(o).filter((n) => names.includes(n)).join(' '); }</script>",
        queries: &[
            (
                r"C(window)",
                r#""[object Window] [object WindowProperties] [object EventTarget] [object Object]""#,
            ),
            (
                r"C(document)",
                r#""[object HTMLDocument] [object Document] [object Node] [object EventTarget] [object Object]""#,
            ),
            (
                r"C(document.documentElement)",
                r#""[object HTMLHtmlElement] [object HTMLElement] [object Element] [object Node] [object EventTarget] [object Object]""#,
            ),
            (
                r"C(document.body.firstChild)",
                r#""[object HTMLDivElement] [object HTMLElement] [object Element] [object Node] [object EventTarget] [object Object]""#,
            ),
            (
                r"C(document.body.firstChild.firstChild)",
                r#""[object Text] [object CharacterData] [object Node] [object EventTarget] [object Object]""#,
            ),
            (
                r"C(document.body.firstChild.childNodes[1])",
                r#""[object Comment] [object CharacterData] [object Node] [object EventTarget] [object Object]""#,
            ),
            (
                r"C(document.doctype)",
                r#""[object DocumentType] [object Node] [object EventTarget] [object Object]""#,
            ),
            (
                r"C(document.body.childNodes[1])",
                r#""[object SVGSVGElement] [object SVGGraphicsElement] [object SVGElement] [object Element] [object Node] [object EventTarget] [object Object]""#,
            ),
            (
                r"C(document.body.childNodes[1].firstChild)",
                r#""[object SVGCircleElement] [object SVGGeometryElement] [object SVGGraphicsElement] [object SVGElement] [object Element] [object Node] [object EventTarget] [object Object]""#,
            ),
            (
                r"C(document.head)",
                r#""[object HTMLHeadElement] [object HTMLElement] [object Element] [object Node] [object EventTarget] [object Object]""#,
            ),
            (
                r"C(document.body.childNodes)",
                r#""[object NodeList] [object Object]""#,
            ),
            (
                r"C(Window.prototype)",
                r#""[object WindowProperties] [object EventTarget] [object Object]""#,
            ),
            (
                r"[Object.prototype.toString.call(window), Object.prototype.toString.call(Object.getPrototypeOf(Window.prototype)), Object.getPrototypeOf(Window.prototype)[Symbol.toStringTag], Object.getOwnPropertyNames(Object.getPrototypeOf(Window.prototype)).length, Object.getPrototypeOf(Object.getPrototypeOf(Window.prototype)) === EventTarget.prototype, 'WindowProperties' in window]",
                r#"["[object Window]", "[object WindowProperties]", "WindowProperties", 0, true, false]"#,
            ),
            (
                r"[Object.getPrototypeOf(Element) === Node, Object.getPrototypeOf(CharacterData) === Node, Object.getPrototypeOf(Text) === CharacterData, Object.getPrototypeOf(Comment) === CharacterData, Object.getPrototypeOf(Document) === Node, Object.getPrototypeOf(DocumentType) === Node, Object.getPrototypeOf(HTMLElement) === Element, Object.getPrototypeOf(Window) === EventTarget, Object.getPrototypeOf(NodeList) === Function.prototype]",
                r"[true, true, true, true, true, true, true, true, true]",
            ),
        ],
    },
    Case {
        name: "immutable",
        html: r"<!DOCTYPE html><html><head><title>  Hello 
  world </title></head><body><div id=a>x<!--c--><span id=s>y</span></div><svg id=g><circle></circle></svg><template id=t><p>q</p></template><script>function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return e.constructor.name + ': ' + e.message; } }function D(o, names) { return names.map((n) => { const d = Object.getOwnPropertyDescriptor(o, n); if (!d) return n + ':none'; const k = d.get || d.set ? (d.get ? 'get' : '') + (d.set ? 'set' : '') : typeof d.value + (d.writable ? 'W' : ''); return n + ':' + k + (d.enumerable ? 'E' : '') + (d.configurable ? 'C' : ''); }).join(' '); }function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }function O(o, names) { return Object.getOwnPropertyNames(o).filter((n) => names.includes(n)).join(' '); }</script>",
        queries: &[
            (
                r"E(() => Object.setPrototypeOf(window, {}))",
                r#""TypeError: Immutable prototype object '#<Window>' cannot have their prototype set""#,
            ),
            (
                r"E(() => Object.setPrototypeOf(Window.prototype, {}))",
                r#""TypeError: Immutable prototype object '#<Window>' cannot have their prototype set""#,
            ),
            (
                r"E(() => Object.setPrototypeOf(Object.getPrototypeOf(Window.prototype), {}))",
                r#""TypeError: Immutable prototype object '#<EventTarget>' cannot have their prototype set""#,
            ),
            (
                r"E(() => Object.setPrototypeOf(EventTarget.prototype, {}))",
                r#""TypeError: Immutable prototype object '#<EventTarget>' cannot have their prototype set""#,
            ),
            (
                r"[Reflect.setPrototypeOf(window, {}), Reflect.setPrototypeOf(window, Window.prototype), Reflect.setPrototypeOf(Object.getPrototypeOf(Window.prototype), null), Reflect.setPrototypeOf(Node.prototype, Node.prototype.__proto__)]",
                r"[false, true, false, true]",
            ),
            (
                r"E(() => { window.__proto__ = {}; return 'set'; })",
                r#""TypeError: Immutable prototype object '#<Window>' cannot have their prototype set""#,
            ),
        ],
    },
    Case {
        name: "identity",
        html: r"<!DOCTYPE html><body><div id=a>x<!--c--><span>y</span></div><script>function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return e.constructor.name + ': ' + e.message; } }function D(o, names) { return names.map((n) => { const d = Object.getOwnPropertyDescriptor(o, n); if (!d) return n + ':none'; const k = d.get || d.set ? (d.get ? 'get' : '') + (d.set ? 'set' : '') : typeof d.value + (d.writable ? 'W' : ''); return n + ':' + k + (d.enumerable ? 'E' : '') + (d.configurable ? 'C' : ''); }).join(' '); }function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }function O(o, names) { return Object.getOwnPropertyNames(o).filter((n) => names.includes(n)).join(' '); }</script>",
        queries: &[
            (
                r"(() => { const a = document.body.firstChild; return [a === document.body.firstChild, a.firstChild === a.firstChild, a.childNodes === a.childNodes, a.childNodes[0] === a.firstChild, a.childNodes.item(2) === a.lastChild, document.body.parentNode === document.documentElement, document.documentElement.parentNode === document, a.ownerDocument === document, a.firstChild.getRootNode() === document]; })()",
                r"[true, true, true, true, true, true, true, true, true]",
            ),
            (
                r"(() => { const a = document.body.firstChild; a.foo = 42; a.firstChild.bar = { x: 'kept' }; a.childNodes.baz = 7; return 1; })()",
                r"1",
            ),
            (
                r"(() => { const a = document.body.firstChild; return [a.foo, a.firstChild.bar.x, a.childNodes.baz, Object.keys(a).join(), Object.keys(a.childNodes).join()]; })()",
                r#"[42, "kept", 7, "foo", "0,1,2,baz"]"#,
            ),
            (
                r"(() => { const w = new WeakMap(); w.set(document.body, 'b'); return w.get(document.documentElement.lastChild); })()",
                r#""b""#,
            ),
        ],
    },
    Case {
        name: "instanceof",
        html: r"<!DOCTYPE html><body><div id=a>x<!--c--><span>y</span></div><script>function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return e.constructor.name + ': ' + e.message; } }function D(o, names) { return names.map((n) => { const d = Object.getOwnPropertyDescriptor(o, n); if (!d) return n + ':none'; const k = d.get || d.set ? (d.get ? 'get' : '') + (d.set ? 'set' : '') : typeof d.value + (d.writable ? 'W' : ''); return n + ':' + k + (d.enumerable ? 'E' : '') + (d.configurable ? 'C' : ''); }).join(' '); }function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }function O(o, names) { return Object.getOwnPropertyNames(o).filter((n) => names.includes(n)).join(' '); }</script>",
        queries: &[(
            r"(() => { const a = document.body.firstChild; return [a instanceof HTMLElement, a instanceof Element, a instanceof Node, a instanceof EventTarget, a instanceof CharacterData, a.firstChild instanceof Text, a.firstChild instanceof CharacterData, a.firstChild instanceof Element, a.childNodes[1] instanceof Comment, a.childNodes[1] instanceof Text, document instanceof Document, document instanceof Node, document instanceof Element, document.doctype instanceof DocumentType, window instanceof Window, window instanceof EventTarget, window instanceof Node, a.childNodes instanceof NodeList]; })()",
            r"[true, true, true, true, false, true, true, false, true, false, true, true, false, true, true, true, false, true]",
        )],
    },
    Case {
        name: "brand-checks",
        html: r"<!DOCTYPE html><body><div id=a>x</div><script>function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return e.constructor.name + ': ' + e.message; } }function D(o, names) { return names.map((n) => { const d = Object.getOwnPropertyDescriptor(o, n); if (!d) return n + ':none'; const k = d.get || d.set ? (d.get ? 'get' : '') + (d.set ? 'set' : '') : typeof d.value + (d.writable ? 'W' : ''); return n + ':' + k + (d.enumerable ? 'E' : '') + (d.configurable ? 'C' : ''); }).join(' '); }function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }function O(o, names) { return Object.getOwnPropertyNames(o).filter((n) => names.includes(n)).join(' '); }</script>",
        queries: &[
            (
                r"E(() => Object.getOwnPropertyDescriptor(Node.prototype, 'firstChild').get.call({}))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(Node.prototype, 'childNodes').get.call(window))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(Node.prototype, 'parentNode').get.call(undefined))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Node.prototype.hasChildNodes.call(Object.create(Node.prototype)))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(Element.prototype, 'tagName').get.call(document))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(Element.prototype, 'tagName').get.call(document.body.firstChild.firstChild))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(Document.prototype, 'title').get.call(document.body))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(CharacterData.prototype, 'data').get.call(document.body))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => NodeList.prototype.item.call({}, 0))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(NodeList.prototype, 'length').get.call(document.body))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(window, 'document').get.call({}))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Node.prototype.firstChild)",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => new Node())",
                r#""TypeError: Failed to construct 'Node': Illegal constructor""#,
            ),
            (
                r"E(() => new Element())",
                r#""TypeError: Failed to construct 'Element': Illegal constructor""#,
            ),
            (
                r"E(() => new NodeList())",
                r#""TypeError: Failed to construct 'NodeList': Illegal constructor""#,
            ),
            (
                r"E(() => Node.prototype.contains.call(document, {}))",
                r#""TypeError: Failed to execute 'contains' on 'Node': parameter 1 is not of type 'Node'.""#,
            ),
            (
                r"E(() => document.contains())",
                r#""TypeError: Failed to execute 'contains' on 'Node': 1 argument required, but only 0 present.""#,
            ),
            (
                r"[document.contains(null), document.contains(document.body), document.body.contains(document)]",
                r"[false, true, false]",
            ),
        ],
    },
    Case {
        name: "tree",
        html: r"<!DOCTYPE html><html><head><title>  Hello 
  world </title></head><body><div id=a>x<!--c--><span id=s>y</span></div><svg id=g><circle></circle></svg><template id=t><p>q</p></template><script>function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return e.constructor.name + ': ' + e.message; } }function D(o, names) { return names.map((n) => { const d = Object.getOwnPropertyDescriptor(o, n); if (!d) return n + ':none'; const k = d.get || d.set ? (d.get ? 'get' : '') + (d.set ? 'set' : '') : typeof d.value + (d.writable ? 'W' : ''); return n + ':' + k + (d.enumerable ? 'E' : '') + (d.configurable ? 'C' : ''); }).join(' '); }function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }function O(o, names) { return Object.getOwnPropertyNames(o).filter((n) => names.includes(n)).join(' '); }</script>",
        queries: &[
            (
                r"(() => { const a = document.body.firstChild; const g = document.body.childNodes[1]; return [document, document.doctype, document.documentElement, a, a.firstChild, a.childNodes[1], g, g.firstChild].map((n) => [n.nodeType, n.nodeName, n.tagName, n.localName, n.namespaceURI, n.prefix].map(String).join('|')).join(' '); })()",
                r#""9|#document|undefined|undefined|undefined|undefined 10|html|undefined|undefined|undefined|undefined 1|HTML|HTML|html|http://www.w3.org/1999/xhtml|null 1|DIV|DIV|div|http://www.w3.org/1999/xhtml|null 3|#text|undefined|undefined|undefined|undefined 8|#comment|undefined|undefined|undefined|undefined 1|svg|svg|svg|http://www.w3.org/2000/svg|null 1|circle|circle|circle|http://www.w3.org/2000/svg|null""#,
            ),
            (
                r"(() => { const h = document.documentElement; const a = document.body.firstChild; return [h.parentNode === document, h.parentElement, document.parentNode, document.ownerDocument, document.isConnected, a.isConnected, a.ownerDocument === document]; })()",
                r"[true, null, null, null, true, true, true]",
            ),
            (
                r"(() => { const a = document.body.firstChild; return [a.hasChildNodes(), a.firstChild.hasChildNodes(), a.firstElementChild.nodeName, a.lastElementChild.nodeName, a.childElementCount, a.lastChild.previousElementSibling, a.nextElementSibling.nodeName, a.firstChild.nextElementSibling.nodeName, a.childNodes[1].previousElementSibling, document.firstElementChild.nodeName, document.childElementCount, document.lastElementChild === document.documentElement]; })()",
                r#"[true, false, "SPAN", "SPAN", 1, null, "svg", "SPAN", null, "HTML", 1, true]"#,
            ),
            (
                r"(() => { const a = document.body.firstChild; const c = a.childNodes; return [c.length, c.item(0).nodeName, c.item(5), c[5], c[-1], c.item(-1), c.item(4294967296).nodeName, [...c].map((n) => n.nodeName).join(), Object.keys(c).join(), 1 in c, 3 in c]; })()",
                r##"[3, "#text", null, null, null, null, "#text", "#text,#comment,SPAN", "0,1,2", true, false]"##,
            ),
            (
                r"(() => { const t = document.body.firstChild.firstChild; const c = document.body.firstChild.childNodes[1]; return [t.data, t.length, c.data, c.length, document.doctype.name, document.doctype.publicId, document.doctype.systemId, document.head.firstChild.firstChild.data]; })()",
                r#"["x", 1, "c", 1, "html", "", "", "  Hello \n  world "]"#,
            ),
            (
                r"(() => { const a = document.body.firstChild; return [a.contains(a), a.contains(a.firstChild), a.firstChild.contains(a), document.contains(a), a.isSameNode(a), a.isSameNode(null), a.firstChild.getRootNode() === document, document.getRootNode() === document]; })()",
                r"[true, true, false, true, true, false, true, true]",
            ),
            (
                r"[document.URL.startsWith('file:'), document.documentURI === document.URL, document.body.baseURI === document.URL, document.compatMode, document.characterSet, document.charset, document.inputEncoding, document.contentType]",
                r#"[true, true, true, "CSS1Compat", "windows-1252", "windows-1252", "windows-1252", "text/html"]"#,
            ),
            (
                r"[document.title, document.head.nodeName, document.body.nodeName]",
                r#"["Hello world", "HEAD", "BODY"]"#,
            ),
        ],
    },
    Case {
        name: "quirks",
        html: r"<html><body><p>x<script>function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return e.constructor.name + ': ' + e.message; } }function D(o, names) { return names.map((n) => { const d = Object.getOwnPropertyDescriptor(o, n); if (!d) return n + ':none'; const k = d.get || d.set ? (d.get ? 'get' : '') + (d.set ? 'set' : '') : typeof d.value + (d.writable ? 'W' : ''); return n + ':' + k + (d.enumerable ? 'E' : '') + (d.configurable ? 'C' : ''); }).join(' '); }function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }function O(o, names) { return Object.getOwnPropertyNames(o).filter((n) => names.includes(n)).join(' '); }</script>",
        queries: &[(
            r"[document.compatMode, document.doctype, document.title, document.head.childNodes.length]",
            r#"["BackCompat", null, "", 0]"#,
        )],
    },
    Case {
        name: "title",
        html: r"<!DOCTYPE html><head><meta charset=utf-8></head><body><title>second</title><p>x<script>function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return e.constructor.name + ': ' + e.message; } }function D(o, names) { return names.map((n) => { const d = Object.getOwnPropertyDescriptor(o, n); if (!d) return n + ':none'; const k = d.get || d.set ? (d.get ? 'get' : '') + (d.set ? 'set' : '') : typeof d.value + (d.writable ? 'W' : ''); return n + ':' + k + (d.enumerable ? 'E' : '') + (d.configurable ? 'C' : ''); }).join(' '); }function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }function O(o, names) { return Object.getOwnPropertyNames(o).filter((n) => names.includes(n)).join(' '); }</script>",
        queries: &[
            (
                r"[document.title, document.head.childNodes.length]",
                r#"["second", 1]"#,
            ),
            (
                r"(() => { document.title = '  New \t title  '; return [document.title, document.body.firstChild.firstChild.data, document.head.childNodes.length]; })()",
                r#"["New title", "  New \t title  ", 1]"#,
            ),
        ],
    },
    Case {
        name: "title-set",
        html: r"<!DOCTYPE html><head><meta charset=windows-1252></head><body><p>x<script>function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return e.constructor.name + ': ' + e.message; } }function D(o, names) { return names.map((n) => { const d = Object.getOwnPropertyDescriptor(o, n); if (!d) return n + ':none'; const k = d.get || d.set ? (d.get ? 'get' : '') + (d.set ? 'set' : '') : typeof d.value + (d.writable ? 'W' : ''); return n + ':' + k + (d.enumerable ? 'E' : '') + (d.configurable ? 'C' : ''); }).join(' '); }function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }function O(o, names) { return Object.getOwnPropertyNames(o).filter((n) => names.includes(n)).join(' '); }</script>",
        queries: &[
            (
                r"(() => { const c = document.head.childNodes; const n = c.length; document.title = 'Made'; return [n, c.length, c[n].nodeName, c[n].namespaceURI, document.title, document.head.lastChild.firstChild.data, document.characterSet]; })()",
                r#"[1, 2, "TITLE", "http://www.w3.org/1999/xhtml", "Made", "Made", "windows-1252"]"#,
            ),
            (
                r"(() => { document.title = ''; return [document.title, document.head.lastChild.childNodes.length]; })()",
                r#"["", 0]"#,
            ),
        ],
    },
    Case {
        name: "body-set",
        html: r"<!DOCTYPE html><body><p>x<script>function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return e.constructor.name + ': ' + e.message; } }function D(o, names) { return names.map((n) => { const d = Object.getOwnPropertyDescriptor(o, n); if (!d) return n + ':none'; const k = d.get || d.set ? (d.get ? 'get' : '') + (d.set ? 'set' : '') : typeof d.value + (d.writable ? 'W' : ''); return n + ':' + k + (d.enumerable ? 'E' : '') + (d.configurable ? 'C' : ''); }).join(' '); }function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }function O(o, names) { return Object.getOwnPropertyNames(o).filter((n) => names.includes(n)).join(' '); }</script>",
        queries: &[
            (
                r"E(() => { document.body = document.head; })",
                r#""DOMException: Failed to set the 'body' property on 'Document': The new body element is of type 'HEAD'. It must be either a 'BODY' or 'FRAMESET' element.""#,
            ),
            (
                r"E(() => { document.body = document.body; return document.body.nodeName; })",
                r#""BODY""#,
            ),
            (
                r"E(() => { document.body = 5; })",
                r#""TypeError: Failed to set the 'body' property on 'Document': Failed to convert value to 'HTMLElement'.""#,
            ),
            (
                r"E(() => { document.body = null; })",
                r#""DOMException: Failed to set the 'body' property on 'Document': The 1st argument provided is either null, or an invalid HTMLElement object.""#,
            ),
        ],
    },
    Case {
        name: "window",
        html: r"<!DOCTYPE html><body><div id=a>x</div><div id=b></div><div id=b></div><p id=window></p><form name=f></form><script>function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return e.constructor.name + ': ' + e.message; } }function D(o, names) { return names.map((n) => { const d = Object.getOwnPropertyDescriptor(o, n); if (!d) return n + ':none'; const k = d.get || d.set ? (d.get ? 'get' : '') + (d.set ? 'set' : '') : typeof d.value + (d.writable ? 'W' : ''); return n + ':' + k + (d.enumerable ? 'E' : '') + (d.configurable ? 'C' : ''); }).join(' '); }function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }function O(o, names) { return Object.getOwnPropertyNames(o).filter((n) => names.includes(n)).join(' '); }</script>",
        queries: &[
            (
                r"[window === self, window.window === window, frames === window, top === window, parent === window, globalThis === window, window.document === document, this === window]",
                r"[true, true, true, true, true, true, true, true]",
            ),
            (
                r"[typeof a, a === document.body.firstChild, 'a' in window, Object.getOwnPropertyDescriptor(window, 'a') === undefined, window.hasOwnProperty('a'), D(Object.getPrototypeOf(Window.prototype), ['a', 'zz']), typeof zz]",
                r#"["object", true, true, true, false, "a:objectWC zz:none", "undefined"]"#,
            ),
            (
                r"[Object.prototype.toString.call(window.b), window.b.length, typeof f, window.window === window]",
                r#"["[object HTMLCollection]", 2, "object", true]"#,
            ),
            (
                r"E(() => { window.a = 5; return [window.a, Object.getOwnPropertyDescriptor(window, 'a').value]; })",
                r"[5, 5]",
            ),
            (
                r"E(() => Object.keys(window).filter((k) => k === 'a' || k === 'document' || k === 'window').join())",
                r#""window,document,a""#,
            ),
            (
                r"E(() => { delete window.document; return typeof window.document; })",
                r#""object""#,
            ),
            (
                r"E(() => { 'use strict'; delete window.document; })",
                r#""TypeError: Cannot delete property 'document' of #<Window>""#,
            ),
            (r"E(() => { self = 5; return self; })", r"5"),
            (
                r"E(() => { window.document = 5; return window.document === document; })",
                r"true",
            ),
        ],
    },
    Case {
        name: "console",
        html: r"<!DOCTYPE html><body><script>function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return e.constructor.name + ': ' + e.message; } }function D(o, names) { return names.map((n) => { const d = Object.getOwnPropertyDescriptor(o, n); if (!d) return n + ':none'; const k = d.get || d.set ? (d.get ? 'get' : '') + (d.set ? 'set' : '') : typeof d.value + (d.writable ? 'W' : ''); return n + ':' + k + (d.enumerable ? 'E' : '') + (d.configurable ? 'C' : ''); }).join(' '); }function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }function O(o, names) { return Object.getOwnPropertyNames(o).filter((n) => names.includes(n)).join(' '); }</script>",
        queries: &[
            (
                r"D(console, ['log', 'info', 'warn', 'error', 'debug'])",
                r#""log:functionWEC info:functionWEC warn:functionWEC error:functionWEC debug:functionWEC""#,
            ),
            (
                r"[Object.prototype.toString.call(console), console.log.name, console.log.length, console.warn.name, typeof console.log('x'), Object.getPrototypeOf(Object.getPrototypeOf(console)) === Object.prototype, Object.getOwnPropertyNames(Object.getPrototypeOf(console)).length]",
                r#"["[object console]", "log", 0, "warn", "undefined", true, 0]"#,
            ),
        ],
    },
    Case {
        name: "inline-scripts",
        html: r"<!DOCTYPE html><body><script>window.r = [1];</script><script type=text/plain>window.r.push('plain');</script><script>throw new Error('stop');</script><script type='text/javascript'>window.r.push(2);</script><script language=vbscript>window.r.push('vb');</script><script type=' TEXT/JavaScript '>window.r.push(3);</script><script nomodule>window.r.push('nomodule');</script><script>function E(f) { try { const v = f(); return v === undefined ? 'undefined' : v; } catch (e) { return e.constructor.name + ': ' + e.message; } }function D(o, names) { return names.map((n) => { const d = Object.getOwnPropertyDescriptor(o, n); if (!d) return n + ':none'; const k = d.get || d.set ? (d.get ? 'get' : '') + (d.set ? 'set' : '') : typeof d.value + (d.writable ? 'W' : ''); return n + ':' + k + (d.enumerable ? 'E' : '') + (d.configurable ? 'C' : ''); }).join(' '); }function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }function O(o, names) { return Object.getOwnPropertyNames(o).filter((n) => names.includes(n)).join(' '); }</script>",
        queries: &[(r"window.r", r"[1, 2, 3]")],
    },
];
