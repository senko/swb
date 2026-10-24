// The cases of `tools/probes/web-dom-mutation.json` with the values that
// Chromium 148 gave (`just probe tools/probes/web-dom-mutation.json`, three
// runs, all equal); where swb departs on purpose, `tests/dom_mutation.rs`
// overrides the value (SWB_VALUES). Generated; the page of most cases is `PAGE`.

use crate::common::Case;

/// The page of most cases.
const PAGE: &str = r#"<!DOCTYPE html><html><head><title>t</title></head><body><div id=a><p id=p>x</p><span id=s>y</span></div><div id=b></div><!--c--><template id=t><i>q</i></template><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message + (e instanceof DOMException ? ' [' + e.code + ']' : ''); } }
function OK(f) { try { f(); return 'ok'; } catch (e) { return e.name; } }
function H(n) { if (n.nodeType === 3) return n.data; if (n.nodeType === 8) return '<!--' + n.data + '-->'; if (n.nodeType !== 1 && n.nodeType !== 11) return '#' + n.nodeType; const kids = Array.prototype.map.call(n.childNodes, H).join(''); if (n.nodeType === 11) return kids; const attrs = Array.prototype.map.call(n.attributes, (a) => ' ' + a.name + '=\"' + a.value + '\"').join(''); const t = n.prefix ? n.prefix + ':' + n.localName : n.localName; return '<' + t + attrs + '>' + kids + '</' + t + '>'; }
Object.defineProperty(Element.prototype, 'outerHTML', {get() { return H(this); }, configurable: true});
Object.defineProperty(Element.prototype, 'innerHTML', {get() { return Array.prototype.map.call(this.childNodes, H).join(''); }, configurable: true});
</script></body></html>"#;

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "m000",
        html: PAGE,
        queries: &[(
            r"E(() => document.body.appendChild(document.body))",
            r#""DOMException HierarchyRequestError: Failed to execute 'appendChild' on 'Node': The new child element contains the parent. [3]""#,
        )],
    },
    Case {
        name: "m001",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('p').appendChild(document.getElementById('a')))",
            r#""DOMException HierarchyRequestError: Failed to execute 'appendChild' on 'Node': The new child element contains the parent. [3]""#,
        )],
    },
    Case {
        name: "m002",
        html: PAGE,
        queries: &[(
            r"E(() => document.createTextNode('x').appendChild(document.createElement('i')))",
            r#""DOMException HierarchyRequestError: Failed to execute 'appendChild' on 'Node': This node type does not support this method. [3]""#,
        )],
    },
    Case {
        name: "m003",
        html: PAGE,
        queries: &[(
            r"E(() => document.createComment('x').appendChild(document.createElement('i')))",
            r#""DOMException HierarchyRequestError: Failed to execute 'appendChild' on 'Node': This node type does not support this method. [3]""#,
        )],
    },
    Case {
        name: "m004",
        html: PAGE,
        queries: &[(
            r"E(() => document.appendChild(document.createTextNode('x')))",
            r#""DOMException HierarchyRequestError: Failed to execute 'appendChild' on 'Node': Nodes of type '#text' may not be inserted inside nodes of type '#document'. [3]""#,
        )],
    },
    Case {
        name: "m005",
        html: PAGE,
        queries: &[(
            r"E(() => document.appendChild(document.createElement('div')))",
            r#""DOMException HierarchyRequestError: Failed to execute 'appendChild' on 'Node': Only one element on document allowed. [3]""#,
        )],
    },
    Case {
        name: "m006",
        html: PAGE,
        queries: &[(
            r"E(() => document.appendChild(document))",
            r#""DOMException HierarchyRequestError: Failed to execute 'appendChild' on 'Node': Nodes of type '#document' may not be inserted inside nodes of type '#document'. [3]""#,
        )],
    },
    Case {
        name: "m007",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').appendChild(document))",
            r#""DOMException HierarchyRequestError: Failed to execute 'appendChild' on 'Node': The new child element contains the parent. [3]""#,
        )],
    },
    Case {
        name: "m008",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').appendChild(document.doctype))",
            r#""DOMException HierarchyRequestError: Failed to execute 'appendChild' on 'Node': Nodes of type 'html' may not be inserted inside nodes of type 'DIV'. [3]""#,
        )],
    },
    Case {
        name: "m009",
        html: PAGE,
        queries: &[(
            r"E(() => document.insertBefore(document.createElement('div'), document.doctype))",
            r#""DOMException HierarchyRequestError: Failed to execute 'insertBefore' on 'Node': Can't insert an element before a doctype. [3]""#,
        )],
    },
    Case {
        name: "m010",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').insertBefore(document.createElement('i'), document.getElementById('b')))",
            r#""DOMException NotFoundError: Failed to execute 'insertBefore' on 'Node': The node before which the new node is to be inserted is not a child of this node. [8]""#,
        )],
    },
    Case {
        name: "m011",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').insertBefore(document.createElement('i'), 5))",
            r#""TypeError: Failed to execute 'insertBefore' on 'Node': parameter 2 is not of type 'Node'.""#,
        )],
    },
    Case {
        name: "m012",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').insertBefore(document.createElement('i')))",
            r#""TypeError: Failed to execute 'insertBefore' on 'Node': 2 arguments required, but only 1 present.""#,
        )],
    },
    Case {
        name: "m013",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').insertBefore(document.createElement('i'), undefined) && 'ok')",
            r#""ok""#,
        )],
    },
    Case {
        name: "m014",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').appendChild())",
            r#""TypeError: Failed to execute 'appendChild' on 'Node': 1 argument required, but only 0 present.""#,
        )],
    },
    Case {
        name: "m015",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').appendChild(null))",
            r#""TypeError: Failed to execute 'appendChild' on 'Node': parameter 1 is not of type 'Node'.""#,
        )],
    },
    Case {
        name: "m016",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').appendChild('str'))",
            r#""TypeError: Failed to execute 'appendChild' on 'Node': parameter 1 is not of type 'Node'.""#,
        )],
    },
    Case {
        name: "m017",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').appendChild({}))",
            r#""TypeError: Failed to execute 'appendChild' on 'Node': parameter 1 is not of type 'Node'.""#,
        )],
    },
    Case {
        name: "m018",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').removeChild(document.getElementById('b')))",
            r#""DOMException NotFoundError: Failed to execute 'removeChild' on 'Node': The node to be removed is not a child of this node. [8]""#,
        )],
    },
    Case {
        name: "m019",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').removeChild(null))",
            r#""TypeError: Failed to execute 'removeChild' on 'Node': parameter 1 is not of type 'Node'.""#,
        )],
    },
    Case {
        name: "m020",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').removeChild())",
            r#""TypeError: Failed to execute 'removeChild' on 'Node': 1 argument required, but only 0 present.""#,
        )],
    },
    Case {
        name: "m021",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').replaceChild(document.createElement('i'), document.getElementById('b')))",
            r#""DOMException NotFoundError: Failed to execute 'replaceChild' on 'Node': The node to be replaced is not a child of this node. [8]""#,
        )],
    },
    Case {
        name: "m022",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').replaceChild(document.getElementById('a'), document.getElementById('p')))",
            r#""DOMException HierarchyRequestError: Failed to execute 'replaceChild' on 'Node': The new child element contains the parent. [3]""#,
        )],
    },
    Case {
        name: "m023",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').replaceChild(document.createElement('i')))",
            r#""TypeError: Failed to execute 'replaceChild' on 'Node': 2 arguments required, but only 1 present.""#,
        )],
    },
    Case {
        name: "m024",
        html: PAGE,
        queries: &[(
            r"E(() => document.createTextNode('x').replaceChild(document.createElement('i'), document.createElement('b')))",
            r#""DOMException HierarchyRequestError: Failed to execute 'replaceChild' on 'Node': This node type does not support this method. [3]""#,
        )],
    },
    Case {
        name: "m025",
        html: PAGE,
        queries: &[(
            r"E(() => document.replaceChild(document.createElement('div'), document.doctype))",
            r#""DOMException HierarchyRequestError: Failed to execute 'replaceChild' on 'Node': Only one element on document allowed. [3]""#,
        )],
    },
    Case {
        name: "m026",
        html: PAGE,
        queries: &[(
            r"E(() => document.replaceChild(document.createElement('div'), document.documentElement) && document.documentElement.tagName)",
            r#""DIV""#,
        )],
    },
    Case {
        name: "m027",
        html: PAGE,
        queries: &[(
            r"E(() => document.createTextNode('x').insertBefore(document.createElement('i'), null))",
            r#""DOMException HierarchyRequestError: Failed to execute 'insertBefore' on 'Node': This node type does not support this method. [3]""#,
        )],
    },
    Case {
        name: "m028",
        html: PAGE,
        queries: &[(
            r"E(() => document.documentElement.insertBefore(document.createTextNode('t'), document.head).nodeValue)",
            r#""t""#,
        )],
    },
    Case {
        name: "m029",
        html: PAGE,
        queries: &[(
            r"E(() => { const f = document.createDocumentFragment(); f.appendChild(document.createElement('a')); f.appendChild(document.createElement('b')); document.appendChild(f); })",
            r#""DOMException HierarchyRequestError: Failed to execute 'appendChild' on 'Node': Only one element on document allowed. [3]""#,
        )],
    },
    Case {
        name: "m030",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').append(document.getElementById('a')))",
            r#""DOMException HierarchyRequestError: Failed to execute 'append' on 'Element': The new child element contains the parent. [3]""#,
        )],
    },
    Case {
        name: "m031",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').append({}))",
            r#""undefined""#,
        )],
    },
    Case {
        name: "m032",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').append(1, null, undefined, [2]) || document.getElementById('a').lastChild.data)",
            r#""2""#,
        )],
    },
    Case {
        name: "m033",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').prepend(document.documentElement))",
            r#""DOMException HierarchyRequestError: Failed to execute 'prepend' on 'Element': The new child element contains the parent. [3]""#,
        )],
    },
    Case {
        name: "m034",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').before(document.getElementById('a')))",
            r#""undefined""#,
        )],
    },
    Case {
        name: "m035",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').before(document.createElement('b')))",
            r#""undefined""#,
        )],
    },
    Case {
        name: "m036",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').after('x'))",
            r#""undefined""#,
        )],
    },
    Case {
        name: "m037",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').replaceWith('x'))",
            r#""undefined""#,
        )],
    },
    Case {
        name: "m038",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').replaceChildren(document.getElementById('a')))",
            r#""DOMException HierarchyRequestError: Failed to execute 'replaceChildren' on 'Element': The new child element contains the parent. [3]""#,
        )],
    },
    Case {
        name: "m039",
        html: PAGE,
        queries: &[(
            r"E(() => document.documentElement.after(document.createElement('b')))",
            r#""DOMException HierarchyRequestError: Failed to execute 'after' on 'Element': Only one element on document allowed. [3]""#,
        )],
    },
    Case {
        name: "m040",
        html: PAGE,
        queries: &[(
            r"E(() => document.documentElement.before('x'))",
            r#""DOMException HierarchyRequestError: Failed to execute 'before' on 'Element': Nodes of type '#text' may not be inserted inside nodes of type '#document'. [3]""#,
        )],
    },
    Case {
        name: "m041",
        html: PAGE,
        queries: &[(
            r"E(() => document.documentElement.replaceWith('x'))",
            r#""DOMException HierarchyRequestError: Failed to execute 'replaceWith' on 'Element': Nodes of type '#text' may not be inserted inside nodes of type '#document'. [3]""#,
        )],
    },
    Case {
        name: "m042",
        html: PAGE,
        queries: &[(
            r"E(() => document.documentElement.remove() === undefined && document.documentElement)",
            r"null",
        )],
    },
    Case {
        name: "m043",
        html: PAGE,
        queries: &[(
            r"E(() => { document.getElementById('a').nodeValue = 'x'; return document.getElementById('a').nodeValue; })",
            r"null",
        )],
    },
    Case {
        name: "m044",
        html: PAGE,
        queries: &[(r"E(() => document.textContent)", r"null")],
    },
    Case {
        name: "m045",
        html: PAGE,
        queries: &[(r"E(() => document.doctype.textContent)", r"null")],
    },
    Case {
        name: "m046",
        html: PAGE,
        queries: &[(
            r"E(() => { document.textContent = 'x'; return document.textContent; })",
            r"null",
        )],
    },
    Case {
        name: "m047",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').textContent)",
            r#""xy""#,
        )],
    },
    Case {
        name: "m048",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); a.textContent = null; return a.childNodes.length; })",
            r"0",
        )],
    },
    Case {
        name: "m049",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); a.textContent = ''; return a.childNodes.length; })",
            r"0",
        )],
    },
    Case {
        name: "m050",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); a.textContent = 42; return a.firstChild.data; })",
            r#""42""#,
        )],
    },
    Case {
        name: "m051",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').setAttribute('1a', 'x'))",
            r#""undefined""#,
        )],
    },
    Case {
        name: "m052",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').setAttribute('', 'x'))",
            r#""DOMException InvalidCharacterError: Failed to execute 'setAttribute' on 'Element': '' is not a valid attribute name. [5]""#,
        )],
    },
    Case {
        name: "m053",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').setAttribute('a:b', 'x'))",
            r#""undefined""#,
        )],
    },
    Case {
        name: "m054",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').setAttribute('a'))",
            r#""TypeError: Failed to execute 'setAttribute' on 'Element': 2 arguments required, but only 1 present.""#,
        )],
    },
    Case {
        name: "m055",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').setAttributeNS('http://www.w3.org/2000/xmlns/', 'a', 'x'))",
            r#""DOMException NamespaceError: Failed to execute 'setAttributeNS' on 'Element': 'http://www.w3.org/2000/xmlns/' is an invalid namespace for attributes. [14]""#,
        )],
    },
    Case {
        name: "m056",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').setAttributeNS('http://x', 'a:b:c', 'x'))",
            r#""undefined""#,
        )],
    },
    Case {
        name: "m057",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').setAttributeNS(null, 'a:b', 'x'))",
            r#""DOMException NamespaceError: Failed to execute 'setAttributeNS' on 'Element': '' is an invalid namespace for attributes. [14]""#,
        )],
    },
    Case {
        name: "m058",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').setAttributeNS('', 'a', 'x'))",
            r#""undefined""#,
        )],
    },
    Case {
        name: "m059",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').setAttributeNS('http://x', 'xml:a', 'x'))",
            r#""DOMException NamespaceError: Failed to execute 'setAttributeNS' on 'Element': 'http://x' is an invalid namespace for attributes. [14]""#,
        )],
    },
    Case {
        name: "m060",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').setAttributeNS('http://www.w3.org/XML/1998/namespace', 'xml:a', 'x'))",
            r#""undefined""#,
        )],
    },
    Case {
        name: "m061",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').setAttributeNS('http://www.w3.org/2000/xmlns/', 'xmlns:a', 'x'))",
            r#""undefined""#,
        )],
    },
    Case {
        name: "m062",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').setAttributeNS('http://x', 'xmlns:a', 'x'))",
            r#""DOMException NamespaceError: Failed to execute 'setAttributeNS' on 'Element': 'http://x' is an invalid namespace for attributes. [14]""#,
        )],
    },
    Case {
        name: "m063",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').setAttributeNS('http://x', 'xmlns', 'x'))",
            r#""DOMException NamespaceError: Failed to execute 'setAttributeNS' on 'Element': 'http://x' is an invalid namespace for attributes. [14]""#,
        )],
    },
    Case {
        name: "m064",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').setAttributeNS('http://x', ':a', 'x'))",
            r#""DOMException InvalidCharacterError: Failed to execute 'setAttributeNS' on 'Element': The qualified name provided (':a') has an empty namespace prefix. [5]""#,
        )],
    },
    Case {
        name: "m065",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').setAttributeNS('http://x', 'a:', 'x'))",
            r#""DOMException InvalidCharacterError: Failed to execute 'setAttributeNS' on 'Element': The qualified name provided ('a:') has an empty local name. [5]""#,
        )],
    },
    Case {
        name: "m066",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').toggleAttribute('1a'))",
            r"true",
        )],
    },
    Case {
        name: "m067",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').removeAttribute('1a'))",
            r#""undefined""#,
        )],
    },
    Case {
        name: "m068",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').hasAttribute('1a'))",
            r"false",
        )],
    },
    Case {
        name: "m069",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').getAttribute('1a'))",
            r"null",
        )],
    },
    Case {
        name: "m070",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').getAttribute())",
            r#""TypeError: Failed to execute 'getAttribute' on 'Element': 1 argument required, but only 0 present.""#,
        )],
    },
    Case {
        name: "m071",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.setAttribute('ClAsS', 'a'); return [e.getAttribute('class'), e.getAttribute('CLASS'), e.hasAttribute('Class'), e.getAttributeNames().join()].join('|'); })",
            r#""a|a|true|class""#,
        )],
    },
    Case {
        name: "m072",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElementNS('http://www.w3.org/1999/xhtml', 'I'); e.setAttribute('ClAsS', 'a'); return [e.tagName, e.localName, e.getAttribute('class'), e.getAttribute('CLASS'), e.getAttributeNames().join()].join('|'); })",
            r#""I|I|a|a|class""#,
        )],
    },
    Case {
        name: "m073",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElementNS('http://www.w3.org/2000/svg', 'svg:Circle'); e.setAttribute('ClAsS', 'a'); e.setAttributeNS('http://www.w3.org/1999/xlink', 'xlink:href', 'u'); return [e.tagName, e.localName, e.prefix, e.namespaceURI, e.getAttribute('class'), e.getAttribute('ClAsS'), e.getAttributeNames().join(), e.getAttribute('xlink:href'), e.getAttributeNS('http://www.w3.org/1999/xlink', 'href'), e.attributes[1].name, e.attributes[1].localName, e.attributes[1].prefix, e.attributes[1].namespaceURI].join('|'); })",
            r#""svg:Circle|Circle|svg|http://www.w3.org/2000/svg||a|ClAsS,xlink:href|u|u|xlink:href|href|xlink|http://www.w3.org/1999/xlink""#,
        )],
    },
    Case {
        name: "m074",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); return [e.toggleAttribute('a'), e.toggleAttribute('a'), e.toggleAttribute('a', true), e.toggleAttribute('a', false), e.toggleAttribute('B'), e.outerHTML].join('|'); })",
            r#""true|false|true|false|true|<i b=\"\"></i>""#,
        )],
    },
    Case {
        name: "m075",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.setAttribute('x', 1); e.setAttribute('y', null); e.setAttribute('z', undefined); e.setAttribute('w', {}); return e.outerHTML; })",
            r#""<i x=\"1\" y=\"null\" z=\"undefined\" w=\"[object Object]\"></i>""#,
        )],
    },
    Case {
        name: "m076",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.setAttribute('a', 'x'); e.setAttribute('b', 'y'); e.setAttribute('a', 'z'); e.removeAttribute('a'); e.setAttribute('a', 'q'); return e.getAttributeNames().join() + e.outerHTML; })",
            r#""b,a<i b=\"y\" a=\"q\"></i>""#,
        )],
    },
    Case {
        name: "m077",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.setAttribute('a', 'x'); const n = e.attributes; return [n.length, n[0].name, n.a.value, n.getNamedItem('A').value, n.item(1), n[1], n.x, Object.keys(n).join(), n.getNamedItem('zz')].join('|'); })",
            r#""1|a|x|x||||0|""#,
        )],
    },
    Case {
        name: "m078",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.setAttribute('a', 'x'); const n = e.attributes; const a = n[0]; e.removeAttribute('a'); return [a.value, a.ownerElement, a.name, n.length, a.specified].join('|'); })",
            r#""x||a|0|true""#,
        )],
    },
    Case {
        name: "m079",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.setAttribute('a', 'x'); const a = e.getAttributeNode('a'); e.setAttribute('a', 'y'); return [a.value, a === e.getAttributeNode('a'), a === e.attributes[0], a === e.attributes.a, a.ownerElement === e].join('|'); })",
            r#""y|true|true|true|true""#,
        )],
    },
    Case {
        name: "m080",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); const a = document.createAttribute('Foo'); a.value = 'v'; const old = e.setAttributeNode(a); return [old, a.name, e.getAttribute('foo'), a.ownerElement === e, e.outerHTML].join('|'); })",
            r#""|foo|v|true|<i foo=\"v\"></i>""#,
        )],
    },
    Case {
        name: "m081",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); const a = document.createAttribute('foo'); e.setAttributeNode(a); const f = document.createElement('b'); return f.setAttributeNode(a); })",
            r#""DOMException InUseAttributeError: Failed to execute 'setAttributeNode' on 'Element': The node provided is an attribute node that is already an attribute of another Element; attribute nodes must be explicitly cloned. [10]""#,
        )],
    },
    Case {
        name: "m082",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); const a = document.createAttribute('foo'); return e.removeAttributeNode(a); })",
            r#""DOMException NotFoundError: Failed to execute 'removeAttributeNode' on 'Element': The node provided is owned by another element. [8]""#,
        )],
    },
    Case {
        name: "m083",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); const a = document.createAttribute('foo'); a.value = 'q'; e.setAttributeNode(a); const b = document.createAttribute('foo'); b.value = 'r'; const old = e.setAttributeNode(b); return [old === a, old.value, old.ownerElement, e.getAttribute('foo')].join('|'); })",
            r#""true|q||r""#,
        )],
    },
    Case {
        name: "m084",
        html: PAGE,
        queries: &[(
            r"E(() => document.createAttribute('1a'))",
            r#""[object Attr]""#,
        )],
    },
    Case {
        name: "m085",
        html: PAGE,
        queries: &[(
            r"E(() => String(Object.prototype.toString.call(document.createAttribute('A'))) + document.createAttribute('A').name)",
            r#""[object Attr]a""#,
        )],
    },
    Case {
        name: "m086",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.createAttribute('x'); return [a.value === '', a.ownerElement, a.namespaceURI, a.prefix, a.localName, a.specified].join('|'); })",
            r#""true||||x|true""#,
        )],
    },
    Case {
        name: "m087",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.createAttributeNS('http://x', 'p:q'); return [a.name, a.localName, a.prefix, a.namespaceURI].join('|'); })",
            r#""p:q|q|p|http://x""#,
        )],
    },
    Case {
        name: "m088",
        html: PAGE,
        queries: &[(
            r"E(() => document.createAttributeNS('http://x', 'p:'))",
            r#""DOMException InvalidCharacterError: Failed to execute 'createAttributeNS' on 'Document': The qualified name provided ('p:') has an empty local name. [5]""#,
        )],
    },
    Case {
        name: "m089",
        html: PAGE,
        queries: &[(
            r"E(() => document.createAttributeNS(null, 'p:q'))",
            r#""DOMException NamespaceError: Failed to execute 'createAttributeNS' on 'Document': The namespace URI provided ('') is not valid for the qualified name provided ('p:q'). [14]""#,
        )],
    },
    Case {
        name: "m090",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.createAttribute('x'); return [a.nodeType, a.nodeName, a.nodeValue, a instanceof Node, Object.getPrototypeOf(Attr.prototype) === EventTarget.prototype].join(); })",
            r#""2,x,,true,false""#,
        )],
    },
    Case {
        name: "m091",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.setAttribute('a', 'x'); const n = e.attributes; return [n.setNamedItem(document.createAttribute('b')) , n.length, n.removeNamedItem('a').value, n.length].join('|'); })",
            r#""|2|x|1""#,
        )],
    },
    Case {
        name: "m092",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').attributes.removeNamedItem('x'))",
            r#""DOMException NotFoundError: Failed to execute 'removeNamedItem' on 'NamedNodeMap': No item with name 'x' was found. [8]""#,
        )],
    },
    Case {
        name: "m093",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').attributes.removeNamedItemNS(null, 'x'))",
            r#""DOMException NotFoundError: Failed to execute 'removeNamedItemNS' on 'NamedNodeMap': No item with name '::x' was found. [8]""#,
        )],
    },
    Case {
        name: "m094",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); return e.attributes === e.attributes; })",
            r"true",
        )],
    },
    Case {
        name: "m095",
        html: PAGE,
        queries: &[(
            r"E(() => Object.prototype.toString.call(document.body.attributes))",
            r#""[object NamedNodeMap]""#,
        )],
    },
    Case {
        name: "m096",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.setAttribute('length', 'x'); return e.attributes.length; })",
            r"1",
        )],
    },
    Case {
        name: "m097",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').classList.add(''))",
            r#""DOMException SyntaxError: Failed to execute 'add' on 'DOMTokenList': The token provided must not be empty. [12]""#,
        )],
    },
    Case {
        name: "m098",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').classList.remove(''))",
            r#""DOMException SyntaxError: Failed to execute 'remove' on 'DOMTokenList': The token provided must not be empty. [12]""#,
        )],
    },
    Case {
        name: "m099",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').classList.toggle(''))",
            r#""DOMException SyntaxError: Failed to execute 'toggle' on 'DOMTokenList': The token provided must not be empty. [12]""#,
        )],
    },
    Case {
        name: "m100",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').classList.replace('', 'x'))",
            r#""DOMException SyntaxError: Failed to execute 'replace' on 'DOMTokenList': The token provided must not be empty. [12]""#,
        )],
    },
    Case {
        name: "m101",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').classList.replace('x', ''))",
            r#""DOMException SyntaxError: Failed to execute 'replace' on 'DOMTokenList': The token provided must not be empty. [12]""#,
        )],
    },
    Case {
        name: "m102",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').classList.contains(''))",
            r"false",
        )],
    },
    Case {
        name: "m103",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').classList.supports('x'))",
            r#""TypeError: Failed to execute 'supports' on 'DOMTokenList': DOMTokenList has no supported tokens.""#,
        )],
    },
    Case {
        name: "m104",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i').classList.add())",
            r#""undefined""#,
        )],
    },
    Case {
        name: "m105",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.className = '  a  b a  c '; const c = e.classList; return [c.length, c.value, c.item(0), c.item(5), c[1], c[9], c.contains('a'), c.contains('A'), String(c)].join('|'); })",
            r#""3|  a  b a  c |a||b||true|false|  a  b a  c ""#,
        )],
    },
    Case {
        name: "m106",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.className = '  a  b a  c '; const c = e.classList; c.add('d'); return [e.className, e.getAttribute('class')].join('|'); })",
            r#""a b c d|a b c d""#,
        )],
    },
    Case {
        name: "m107",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.className = '  a  b a  c '; const c = e.classList; c.remove('b'); return [e.className].join('|'); })",
            r#""a c""#,
        )],
    },
    Case {
        name: "m108",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.className = '  a  b a  c '; const c = e.classList; c.add('a'); return [e.className].join('|'); })",
            r#""a b c""#,
        )],
    },
    Case {
        name: "m109",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); const c = e.classList; c.remove('x'); return [e.hasAttribute('class'), e.className].join('|'); })",
            r#""false|""#,
        )],
    },
    Case {
        name: "m110",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); const c = e.classList; c.add(); return [e.hasAttribute('class'), e.className].join('|'); })",
            r#""true|""#,
        )],
    },
    Case {
        name: "m111",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); const c = e.classList; c.toggle('x'); c.toggle('x'); return [e.hasAttribute('class'), JSON.stringify(e.className)].join('|'); })",
            r#""true|\"\"""#,
        )],
    },
    Case {
        name: "m112",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.className = 'a b c'; const c = e.classList; return [c.toggle('b'), c.toggle('b'), c.toggle('z', true), c.toggle('z', true), c.toggle('z', false), c.toggle('z', false), e.className].join('|'); })",
            r#""false|true|true|true|false|false|a c b""#,
        )],
    },
    Case {
        name: "m113",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.className = 'a b c'; const c = e.classList; return [c.replace('b', 'x'), c.replace('q', 'x'), c.replace('a', 'c'), e.className].join('|'); })",
            r#""true|false|true|c x""#,
        )],
    },
    Case {
        name: "m114",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.className = 'a b c'; const c = e.classList; return [c.replace('c', 'a'), e.className].join('|'); })",
            r#""true|a b""#,
        )],
    },
    Case {
        name: "m115",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.className = 'a b a'; const c = e.classList; c.replace('b', 'a'); return e.className; })",
            r#""a""#,
        )],
    },
    Case {
        name: "m116",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); const c = e.classList; return [c === e.classList, Object.prototype.toString.call(c), c.constructor.name].join('|'); })",
            r#""true|[object DOMTokenList]|DOMTokenList""#,
        )],
    },
    Case {
        name: "m117",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.className = 'a'; const c = e.classList; c.foo = 1; c[3] = 3; return [Object.keys(c).join(), c.foo, c[3], JSON.stringify(c), 'length' in c].join('|'); })",
            r#""0,foo|1||{\"0\":\"a\",\"foo\":1}|true""#,
        )],
    },
    Case {
        name: "m118",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.setAttribute('class', 'tab\tnl\nff\fcr\rsp x'); return e.classList.length + '|' + e.classList.value; })",
            r#""6|tab\tnl\nff\fcr\rsp x""#,
        )],
    },
    Case {
        name: "m119",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.className = 'a'; const c = e.classList; e.className = 'b c'; return [c.length, c[0], c.value].join(); })",
            r#""2,b,b c""#,
        )],
    },
    Case {
        name: "m120",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.dataset.fooBar = 'x'; e.dataset.baz = 1; return [e.outerHTML, e.dataset.fooBar, e.dataset['foo-bar'], Object.keys(e.dataset).join(), 'fooBar' in e.dataset, 'zz' in e.dataset].join('|'); })",
            r#""<i data-foo-bar=\"x\" data-baz=\"1\"></i>|x||fooBar,baz|true|false""#,
        )],
    },
    Case {
        name: "m121",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.dataset['foo-bar'] = 'x'; return e.outerHTML; })",
            r#""DOMException SyntaxError: Failed to set a named property 'foo-bar' on 'DOMStringMap': 'foo-bar' is not a valid property name. [12]""#,
        )],
    },
    Case {
        name: "m122",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.dataset['a-1'] = 'x'; return e.outerHTML; })",
            r#""<i data-a-1=\"x\"></i>""#,
        )],
    },
    Case {
        name: "m123",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.dataset.fooBar = 'x'; delete e.dataset.fooBar; return e.outerHTML; })",
            r#""<i></i>""#,
        )],
    },
    Case {
        name: "m124",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.setAttribute('data-a-b-c', '1'); e.setAttribute('data-', '2'); e.setAttribute('data-X', '3'); e.setAttribute('data-a-', '4'); e.setAttribute('data--b', '5'); e.setAttribute('data-1', '6'); e.setAttribute('data-ä', '7'); return JSON.stringify(Object.entries(e.dataset)); })",
            r#""[[\"aBC\",\"1\"],[\"\",\"2\"],[\"x\",\"3\"],[\"a-\",\"4\"],[\"B\",\"5\"],[\"1\",\"6\"],[\"ä\",\"7\"]]""#,
        )],
    },
    Case {
        name: "m125",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.dataset.x = 1; return [e.dataset.x, typeof e.dataset.y, e.dataset === e.dataset, Object.prototype.toString.call(e.dataset)].join('|'); })",
            r#""1|undefined|true|[object DOMStringMap]""#,
        )],
    },
    Case {
        name: "m126",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.dataset['A'] = 'x'; return e.outerHTML; })",
            r#""<i data--a=\"x\"></i>""#,
        )],
    },
    Case {
        name: "m127",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.dataset['-a'] = 'x'; return e.outerHTML; })",
            r#""DOMException SyntaxError: Failed to set a named property '-a' on 'DOMStringMap': '-a' is not a valid property name. [12]""#,
        )],
    },
    Case {
        name: "m128",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.dataset.aB = 'x'; e.setAttribute('data-a-b', 'y'); return [e.dataset.aB, e.outerHTML, Object.keys(e.dataset).join()].join('|'); })",
            r#""y|<i data-a-b=\"y\"></i>|aB""#,
        )],
    },
    Case {
        name: "m129",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.dataset.length = 3; return [e.dataset.length, e.outerHTML].join('|'); })",
            r#""3|<i data-length=\"3\"></i>""#,
        )],
    },
    Case {
        name: "m130",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.dataset.toString = 3; return [typeof e.dataset.toString, e.outerHTML].join('|'); })",
            r#""string|<i data-to-string=\"3\"></i>""#,
        )],
    },
    Case {
        name: "m131",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); return [typeof e.dataset.hasOwnProperty, typeof e.dataset.toString, e.dataset.constructor === Object].join('|'); })",
            r#""function|function|false""#,
        )],
    },
    Case {
        name: "m132",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.dataset.a = 1; return Object.getOwnPropertyDescriptor(e.dataset, 'a') && JSON.stringify(Object.getOwnPropertyDescriptor(e.dataset, 'a')); })",
            r#""{\"value\":\"1\",\"writable\":true,\"enumerable\":true,\"configurable\":true}""#,
        )],
    },
    Case {
        name: "m133",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement(''))",
            r#""DOMException InvalidCharacterError: Failed to execute 'createElement' on 'Document': The tag name provided ('') is not a valid name. [5]""#,
        )],
    },
    Case {
        name: "m134",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('1'))",
            r#""DOMException InvalidCharacterError: Failed to execute 'createElement' on 'Document': The tag name provided ('1') is not a valid name. [5]""#,
        )],
    },
    Case {
        name: "m135",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('A').tagName + document.createElement('A').localName)",
            r#""Aa""#,
        )],
    },
    Case {
        name: "m136",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('svg').namespaceURI)",
            r#""http://www.w3.org/1999/xhtml""#,
        )],
    },
    Case {
        name: "m137",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('x:y').tagName + ' ' + document.createElement('x:y').localName + ' ' + document.createElement('x:y').prefix)",
            r#""X:Y x:y null""#,
        )],
    },
    Case {
        name: "m138",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i', 'x-y').outerHTML)",
            r#""<i></i>""#,
        )],
    },
    Case {
        name: "m139",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i', null).outerHTML)",
            r#""<i></i>""#,
        )],
    },
    Case {
        name: "m140",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('i', 5).outerHTML)",
            r#""<i></i>""#,
        )],
    },
    Case {
        name: "m141",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement(null).outerHTML)",
            r#""<null></null>""#,
        )],
    },
    Case {
        name: "m142",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement(undefined).outerHTML)",
            r#""<undefined></undefined>""#,
        )],
    },
    Case {
        name: "m143",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement())",
            r#""TypeError: Failed to execute 'createElement' on 'Document': 1 argument required, but only 0 present.""#,
        )],
    },
    Case {
        name: "m144",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElementNS('http://www.w3.org/2000/svg', 'svg').outerHTML)",
            r#""<svg></svg>""#,
        )],
    },
    Case {
        name: "m145",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElementNS('http://www.w3.org/2000/svg', 'svg:g').outerHTML)",
            r#""<svg:g></svg:g>""#,
        )],
    },
    Case {
        name: "m146",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElementNS('', 'g').namespaceURI)",
            r"null",
        )],
    },
    Case {
        name: "m147",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElementNS(null, 'g').namespaceURI)",
            r"null",
        )],
    },
    Case {
        name: "m148",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElementNS(undefined, 'g').namespaceURI)",
            r"null",
        )],
    },
    Case {
        name: "m149",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElementNS(null, 'p:g'))",
            r#""DOMException NamespaceError: Failed to execute 'createElementNS' on 'Document': The namespace URI provided ('') is not valid for the qualified name provided ('p:g'). [14]""#,
        )],
    },
    Case {
        name: "m150",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElementNS('http://x', 'p:g').prefix)",
            r#""p""#,
        )],
    },
    Case {
        name: "m151",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElementNS('http://x', 'p:'))",
            r#""DOMException InvalidCharacterError: Failed to execute 'createElementNS' on 'Document': The qualified name provided ('p:') has an empty local name. [5]""#,
        )],
    },
    Case {
        name: "m152",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElementNS('http://x', ':g'))",
            r#""DOMException InvalidCharacterError: Failed to execute 'createElementNS' on 'Document': The qualified name provided (':g') has an empty namespace prefix. [5]""#,
        )],
    },
    Case {
        name: "m153",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElementNS('http://x', '1'))",
            r#""DOMException InvalidCharacterError: Failed to execute 'createElementNS' on 'Document': The qualified name provided ('1') contains the invalid character '1'. [5]""#,
        )],
    },
    Case {
        name: "m154",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElementNS('http://x', 'a:b:c'))",
            r#""[object Element]""#,
        )],
    },
    Case {
        name: "m155",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElementNS('http://x', 'xml:g'))",
            r#""DOMException NamespaceError: Failed to execute 'createElementNS' on 'Document': The namespace URI provided ('http://x') is not valid for the qualified name provided ('xml:g'). [14]""#,
        )],
    },
    Case {
        name: "m156",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElementNS('http://www.w3.org/XML/1998/namespace', 'xml:g').localName)",
            r#""g""#,
        )],
    },
    Case {
        name: "m157",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElementNS('http://x', 'xmlns:g'))",
            r#""DOMException NamespaceError: Failed to execute 'createElementNS' on 'Document': The namespace URI provided ('http://x') is not valid for the qualified name provided ('xmlns:g'). [14]""#,
        )],
    },
    Case {
        name: "m158",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElementNS('http://www.w3.org/2000/xmlns/', 'xmlns:g').localName)",
            r#""g""#,
        )],
    },
    Case {
        name: "m159",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElementNS('http://www.w3.org/2000/xmlns/', 'g'))",
            r#""DOMException NamespaceError: Failed to execute 'createElementNS' on 'Document': The namespace URI provided ('http://www.w3.org/2000/xmlns/') is not valid for the qualified name provided ('g'). [14]""#,
        )],
    },
    Case {
        name: "m160",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElementNS('http://x', 'xmlns'))",
            r#""DOMException NamespaceError: Failed to execute 'createElementNS' on 'Document': The namespace URI provided ('http://x') is not valid for the qualified name provided ('xmlns'). [14]""#,
        )],
    },
    Case {
        name: "m161",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElementNS('http://x', ''))",
            r#""DOMException InvalidCharacterError: Failed to execute 'createElementNS' on 'Document': The qualified name provided is empty. [5]""#,
        )],
    },
    Case {
        name: "m162",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElementNS('http://x', 'a').tagName)",
            r#""a""#,
        )],
    },
    Case {
        name: "m163",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElementNS('http://www.w3.org/1999/xhtml', 'a:B').tagName)",
            r#""A:B""#,
        )],
    },
    Case {
        name: "m164",
        html: PAGE,
        queries: &[(
            r"E(() => document.createTextNode().data)",
            r#""TypeError: Failed to execute 'createTextNode' on 'Document': 1 argument required, but only 0 present.""#,
        )],
    },
    Case {
        name: "m165",
        html: PAGE,
        queries: &[(r"E(() => document.createTextNode(5).data)", r#""5""#)],
    },
    Case {
        name: "m166",
        html: PAGE,
        queries: &[(r"E(() => document.createTextNode(null).data)", r#""null""#)],
    },
    Case {
        name: "m167",
        html: PAGE,
        queries: &[(
            r"E(() => document.createComment().data)",
            r#""TypeError: Failed to execute 'createComment' on 'Document': 1 argument required, but only 0 present.""#,
        )],
    },
    Case {
        name: "m168",
        html: PAGE,
        queries: &[(r"E(() => document.createComment('a--b').data)", r#""a--b""#)],
    },
    Case {
        name: "m169",
        html: PAGE,
        queries: &[(
            r"E(() => document.createDocumentFragment().nodeName)",
            r##""#document-fragment""##,
        )],
    },
    Case {
        name: "m170",
        html: PAGE,
        queries: &[(
            r"E(() => new Text().data + '|' + new Text('x').data + '|' + new Comment('q').data + '|' + new DocumentFragment().nodeName + '|' + new Text(null).data)",
            r#""|x|q|#document-fragment|null""#,
        )],
    },
    Case {
        name: "m171",
        html: PAGE,
        queries: &[(
            r"E(() => Text())",
            r#""TypeError: Failed to construct 'Text': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
        )],
    },
    Case {
        name: "m172",
        html: PAGE,
        queries: &[(
            r"E(() => [new Text('a').ownerDocument === document, new DocumentFragment().ownerDocument === document].join())",
            r#""true,true""#,
        )],
    },
    Case {
        name: "m173",
        html: PAGE,
        queries: &[(
            r"E(() => { const n = document.importNode(document.getElementById('a'), true); return [n === document.getElementById('a'), n.parentNode, n.ownerDocument === document, n.childNodes.length, n.outerHTML].join('|'); })",
            r#""false||true|2|<div id=\"a\"><p id=\"p\">x</p><span id=\"s\">y</span></div>""#,
        )],
    },
    Case {
        name: "m174",
        html: PAGE,
        queries: &[(
            r"E(() => document.importNode(document.getElementById('a')).childNodes.length)",
            r"0",
        )],
    },
    Case {
        name: "m175",
        html: PAGE,
        queries: &[(
            r"E(() => document.importNode())",
            r#""TypeError: Failed to execute 'importNode' on 'Document': 1 argument required, but only 0 present.""#,
        )],
    },
    Case {
        name: "m176",
        html: PAGE,
        queries: &[(
            r"E(() => document.importNode(document))",
            r#""DOMException NotSupportedError: Failed to execute 'importNode' on 'Document': The node provided is a document, which may not be imported. [9]""#,
        )],
    },
    Case {
        name: "m177",
        html: PAGE,
        queries: &[(
            r"E(() => document.adoptNode(document))",
            r#""DOMException NotSupportedError: Failed to execute 'adoptNode' on 'Document': The node provided is of type '#document', which may not be adopted. [9]""#,
        )],
    },
    Case {
        name: "m178",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); const r = document.adoptNode(a); return [r === a, a.parentNode].join(); })",
            r#""true,""#,
        )],
    },
    Case {
        name: "m179",
        html: PAGE,
        queries: &[(
            r"E(() => document.adoptNode(document.createElement('i')).tagName)",
            r#""I""#,
        )],
    },
    Case {
        name: "m180",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); const c = a.cloneNode(); return [c.outerHTML, c === a, c.parentNode, c.isEqualNode(a), c.ownerDocument === document].join('|'); })",
            r#""<div id=\"a\"></div>|false||false|true""#,
        )],
    },
    Case {
        name: "m181",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); const c = a.cloneNode(true); return [c.outerHTML, c.isEqualNode(a), c.firstChild === a.firstChild].join('|'); })",
            r#""<div id=\"a\"><p id=\"p\">x</p><span id=\"s\">y</span></div>|true|false""#,
        )],
    },
    Case {
        name: "m182",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); return a.cloneNode(undefined).childNodes.length + '|' + a.cloneNode(1).childNodes.length + '|' + a.cloneNode(0).childNodes.length + '|' + a.cloneNode('').childNodes.length; })",
            r#""0|2|0|0""#,
        )],
    },
    Case {
        name: "m184",
        html: PAGE,
        queries: &[(
            r"E(() => document.cloneNode(true).nodeName + document.cloneNode(true).childNodes.length + (document.cloneNode(true) !== document))",
            r##""#document2true""##,
        )],
    },
    Case {
        name: "m185",
        html: PAGE,
        queries: &[(r"E(() => document.doctype.cloneNode().name)", r#""html""#)],
    },
    Case {
        name: "m186",
        html: PAGE,
        queries: &[(
            r"E(() => document.createTextNode('x').cloneNode().data)",
            r#""x""#,
        )],
    },
    Case {
        name: "m187",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.setAttribute('a', '1'); const c = e.cloneNode(); e.setAttribute('a', '2'); return c.getAttribute('a'); })",
            r#""1""#,
        )],
    },
    Case {
        name: "m188",
        html: PAGE,
        queries: &[(
            r"E(() => { const s = document.createElement('script'); s.textContent = 'window.__ran = (window.__ran||0)+1'; document.head.appendChild(s); const c = s.cloneNode(true); document.head.appendChild(c); return window.__ran; })",
            r"1",
        )],
    },
    Case {
        name: "m189",
        html: PAGE,
        queries: &[(
            r"E(() => { const s = document.createElement('script'); s.textContent = 'window.__ran2 = (window.__ran2||0)+1'; const c = s.cloneNode(true); document.head.appendChild(s); document.head.appendChild(c); return window.__ran2; })",
            r"2",
        )],
    },
    Case {
        name: "m190",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.createElement('i'); const b = document.createElement('i'); a.setAttribute('x', '1'); a.setAttribute('y', '2'); b.setAttribute('y', '2'); b.setAttribute('x', '1'); return [a.isEqualNode(b), a.isEqualNode(null), a.isEqualNode(), a.isEqualNode(document.createElement('b'))].join(); })",
            r#""TypeError: Failed to execute 'isEqualNode' on 'Node': 1 argument required, but only 0 present.""#,
        )],
    },
    Case {
        name: "m191",
        html: PAGE,
        queries: &[(
            r"E(() => document.createTextNode('a').isEqualNode(document.createComment('a')))",
            r"false",
        )],
    },
    Case {
        name: "m192",
        html: PAGE,
        queries: &[(
            r"E(() => document.createTextNode('a').isEqualNode(document.createTextNode('a')))",
            r"true",
        )],
    },
    Case {
        name: "m193",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').isEqualNode(5))",
            r#""TypeError: Failed to execute 'isEqualNode' on 'Node': parameter 1 is not of type 'Node'.""#,
        )],
    },
    Case {
        name: "m194",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'), p = document.getElementById('p'), b = document.getElementById('b'); const c = document.createElement('x'); return [a.compareDocumentPosition(p), p.compareDocumentPosition(a), a.compareDocumentPosition(b), b.compareDocumentPosition(a), a.compareDocumentPosition(a), a.compareDocumentPosition(document), document.compareDocumentPosition(a)].join(); })",
            r#""20,10,4,2,0,10,20""#,
        )],
    },
    Case {
        name: "m195",
        html: PAGE,
        queries: &[(
            r"E(() => { const c = document.createElement('x'); const d = document.createElement('y'); return [c.compareDocumentPosition(d), d.compareDocumentPosition(c), c.compareDocumentPosition(document.body), document.body.compareDocumentPosition(c)].join(); })",
            r#""37,35,35,37""#,
        )],
    },
    Case {
        name: "m196",
        html: PAGE,
        queries: &[(
            r"E(() => { const c = document.createElement('x'); const d = document.createElement('y'); return [document.createElement('a').compareDocumentPosition(document.createElement('b')), document.createElement('a').compareDocumentPosition(document.createElement('b'))].join(); })",
            r#""37,37""#,
        )],
    },
    Case {
        name: "m197",
        html: PAGE,
        queries: &[(
            r"E(() => document.body.compareDocumentPosition())",
            r#""TypeError: Failed to execute 'compareDocumentPosition' on 'Node': 1 argument required, but only 0 present.""#,
        )],
    },
    Case {
        name: "m198",
        html: PAGE,
        queries: &[(
            r"E(() => document.body.compareDocumentPosition(null))",
            r#""TypeError: Failed to execute 'compareDocumentPosition' on 'Node': parameter 1 is not of type 'Node'.""#,
        )],
    },
    Case {
        name: "m199",
        html: PAGE,
        queries: &[(
            r"E(() => document.body.contains())",
            r#""TypeError: Failed to execute 'contains' on 'Node': 1 argument required, but only 0 present.""#,
        )],
    },
    Case {
        name: "m200",
        html: PAGE,
        queries: &[(
            r"E(() => document.body.contains(null) + '|' + document.body.contains(document.body) + '|' + document.body.contains(document.documentElement) + '|' + document.contains(document.body))",
            r#""false|true|false|true""#,
        )],
    },
    Case {
        name: "m201",
        html: PAGE,
        queries: &[(
            r"E(() => document.body.contains({}))",
            r#""TypeError: Failed to execute 'contains' on 'Node': parameter 1 is not of type 'Node'.""#,
        )],
    },
    Case {
        name: "m202",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); const t = document.createElement('b'); e.appendChild(t); return [t.getRootNode() === e, e.getRootNode() === e, t.isConnected, document.body.getRootNode() === document, document.getRootNode() === document, t.getRootNode({composed: true}) === e].join(); })",
            r#""true,true,false,true,true,true""#,
        )],
    },
    Case {
        name: "m203",
        html: PAGE,
        queries: &[(
            r"E(() => { const t = document.createTextNode('abcdef'); return [t.substringData(1, 2), t.substringData(4, 100), t.length].join(); })",
            r#""bc,ef,6""#,
        )],
    },
    Case {
        name: "m204",
        html: PAGE,
        queries: &[(
            r"E(() => document.createTextNode('abc').substringData(4, 1))",
            r#""DOMException IndexSizeError: Failed to execute 'substringData' on 'CharacterData': The offset 4 is greater than the node's length (3). [1]""#,
        )],
    },
    Case {
        name: "m205",
        html: PAGE,
        queries: &[(
            r"E(() => document.createTextNode('abc').substringData(3, 1) === '')",
            r"true",
        )],
    },
    Case {
        name: "m206",
        html: PAGE,
        queries: &[(
            r"E(() => document.createTextNode('abc').substringData(-1, 1))",
            r#""DOMException IndexSizeError: Failed to execute 'substringData' on 'CharacterData': The offset 4294967295 is greater than the node's length (3). [1]""#,
        )],
    },
    Case {
        name: "m207",
        html: PAGE,
        queries: &[(
            r"E(() => document.createTextNode('abc').substringData(1))",
            r#""TypeError: Failed to execute 'substringData' on 'CharacterData': 2 arguments required, but only 1 present.""#,
        )],
    },
    Case {
        name: "m208",
        html: PAGE,
        queries: &[(
            r"E(() => { const t = document.createTextNode('abc'); t.appendData('de'); t.insertData(1, 'X'); t.deleteData(0, 1); t.replaceData(1, 1, 'YY'); return t.data; })",
            r#""XYYcde""#,
        )],
    },
    Case {
        name: "m209",
        html: PAGE,
        queries: &[(
            r"E(() => document.createTextNode('abc').insertData(4, 'x'))",
            r#""DOMException IndexSizeError: Failed to execute 'insertData' on 'CharacterData': The offset 4 is greater than the node's length (3). [1]""#,
        )],
    },
    Case {
        name: "m210",
        html: PAGE,
        queries: &[(
            r"E(() => document.createTextNode('abc').deleteData(4, 1))",
            r#""DOMException IndexSizeError: Failed to execute 'deleteData' on 'CharacterData': The offset 4 is greater than the node's length (3). [1]""#,
        )],
    },
    Case {
        name: "m211",
        html: PAGE,
        queries: &[(
            r"E(() => document.createTextNode('abc').replaceData(4, 1, 'x'))",
            r#""DOMException IndexSizeError: Failed to execute 'replaceData' on 'CharacterData': The offset 4 is greater than the node's length (3). [1]""#,
        )],
    },
    Case {
        name: "m212",
        html: PAGE,
        queries: &[(
            r"E(() => { const t = document.createTextNode('abc'); t.data = null; return JSON.stringify(t.data) + t.length; })",
            r#""\"\"0""#,
        )],
    },
    Case {
        name: "m213",
        html: PAGE,
        queries: &[(
            r"E(() => { const t = document.createTextNode('abc'); t.data = undefined; return t.data; })",
            r#""undefined""#,
        )],
    },
    Case {
        name: "m214",
        html: PAGE,
        queries: &[(
            r"E(() => { const t = document.createTextNode('abc'); t.nodeValue = null; return JSON.stringify(t.nodeValue); })",
            r#""\"\"""#,
        )],
    },
    Case {
        name: "m215",
        html: PAGE,
        queries: &[(
            r"E(() => { const t = document.createTextNode('abc'); t.textContent = null; return JSON.stringify(t.textContent); })",
            r#""\"\"""#,
        )],
    },
    Case {
        name: "m216",
        html: PAGE,
        queries: &[(
            r"E(() => { const t = document.createTextNode('abc'); t.textContent = 5; return t.data; })",
            r#""5""#,
        )],
    },
    Case {
        name: "m217",
        html: PAGE,
        queries: &[(
            r"E(() => { const t = document.createTextNode('a\ud83d\ude00b'); return [t.length, t.substringData(1, 2) === '\ud83d\ude00', t.substringData(0, 1) + t.substringData(3, 1)].join(); })",
            r#""4,true,ab""#,
        )],
    },
    Case {
        name: "m218",
        html: PAGE,
        queries: &[(
            r"E(() => { const t = document.createTextNode('a\ud83d\ude00b'); t.insertData(2, 'X'); return t.length + ':' + t.data.length; })",
            r#""5:5""#,
        )],
    },
    Case {
        name: "m219",
        html: PAGE,
        queries: &[(
            r"E(() => { const t = document.createTextNode('abc'); const s = t.splitText(1); return [t.data, s.data, s.parentNode, t.nextSibling].join(); })",
            r#""a,bc,,""#,
        )],
    },
    Case {
        name: "m220",
        html: PAGE,
        queries: &[(
            r"E(() => { const p = document.createElement('p'); const t = document.createTextNode('abc'); p.appendChild(t); const s = t.splitText(1); return [t.data, s.data, s.parentNode === p, t.nextSibling === s, p.childNodes.length].join(); })",
            r#""a,bc,true,true,2""#,
        )],
    },
    Case {
        name: "m221",
        html: PAGE,
        queries: &[(
            r"E(() => document.createTextNode('abc').splitText(4))",
            r#""DOMException IndexSizeError: Failed to execute 'splitText' on 'Text': The offset 4 is larger than the Text node's length. [1]""#,
        )],
    },
    Case {
        name: "m222",
        html: PAGE,
        queries: &[(
            r"E(() => document.createTextNode('abc').splitText(3).data === '')",
            r"true",
        )],
    },
    Case {
        name: "m223",
        html: PAGE,
        queries: &[(
            r"E(() => document.createTextNode('abc').splitText())",
            r#""TypeError: Failed to execute 'splitText' on 'Text': 1 argument required, but only 0 present.""#,
        )],
    },
    Case {
        name: "m224",
        html: PAGE,
        queries: &[(
            r"E(() => { const p = document.createElement('p'); p.append('a', 'b'); p.appendChild(document.createElement('i')); p.append('c'); const t = p.firstChild; return [p.childNodes.length, t.wholeText, p.lastChild.wholeText, p.childNodes[1].wholeText].join('|'); })",
            r#""4|ab|c|ab""#,
        )],
    },
    Case {
        name: "m225",
        html: PAGE,
        queries: &[(
            r"E(() => { const p = document.createElement('p'); p.appendChild(document.createTextNode('a')); p.appendChild(document.createTextNode('')); p.appendChild(document.createTextNode('b')); p.appendChild(document.createElement('i')); p.appendChild(document.createTextNode('')); p.appendChild(document.createTextNode('c')); const t = p.firstChild; p.normalize(); return [p.childNodes.length, p.firstChild.data, p.firstChild === t, p.lastChild.data].join('|'); })",
            r#""3|ab|true|c""#,
        )],
    },
    Case {
        name: "m226",
        html: PAGE,
        queries: &[(
            r"E(() => { const p = document.createElement('p'); p.appendChild(document.createTextNode('')); p.normalize(); return p.childNodes.length; })",
            r"0",
        )],
    },
    Case {
        name: "m227",
        html: PAGE,
        queries: &[(
            r"E(() => { const d = document.createElement('div'); const p = document.createElement('p'); p.appendChild(document.createTextNode('a')); p.appendChild(document.createTextNode('b')); d.appendChild(p); d.appendChild(document.createTextNode('')); d.normalize(); return [d.childNodes.length, p.childNodes.length].join(); })",
            r#""1,1""#,
        )],
    },
    Case {
        name: "m228",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').id + document.getElementById() )",
            r#""TypeError: Failed to execute 'getElementById' on 'Document': 1 argument required, but only 0 present.""#,
        )],
    },
    Case {
        name: "m229",
        html: PAGE,
        queries: &[(r"E(() => document.getElementById(null))", r"null")],
    },
    Case {
        name: "m230",
        html: PAGE,
        queries: &[(r"E(() => document.getElementById(''))", r"null")],
    },
    Case {
        name: "m231",
        html: PAGE,
        queries: &[(r"E(() => document.getElementById('nope'))", r"null")],
    },
    Case {
        name: "m232",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.id = 'dyn'; const before = document.getElementById('dyn'); document.body.appendChild(e); const mid = document.getElementById('dyn') === e; document.body.removeChild(e); return [before, mid, document.getElementById('dyn')].join('|'); })",
            r#""|true|""#,
        )],
    },
    Case {
        name: "m233",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.id = 'dyn2'; document.body.appendChild(e); e.id = 'dyn3'; return [document.getElementById('dyn2'), document.getElementById('dyn3') === e].join('|'); })",
            r#""|true""#,
        )],
    },
    Case {
        name: "m234",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.id = 'dup'; const f = document.createElement('b'); f.id = 'dup'; document.body.appendChild(e); document.body.insertBefore(f, e); return document.getElementById('dup') === f; })",
            r"true",
        )],
    },
    Case {
        name: "m235",
        html: PAGE,
        queries: &[(
            r"E(() => { const f = document.createDocumentFragment(); const e = document.createElement('i'); e.id = 'infrag'; f.appendChild(e); return [f.getElementById('infrag') === e, document.getElementById('infrag')].join('|'); })",
            r#""true|""#,
        )],
    },
    Case {
        name: "m237",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); return [a.children.length, a.children[0].id, a.children.item(1).id, a.children.p.id, a.children.namedItem('s').id, a.children.namedItem('zz'), a.children === a.children, Object.prototype.toString.call(a.children), a.childElementCount, a.firstElementChild.id, a.lastElementChild.id, a.children.item(5), a.children[5]].join('|'); })",
            r#""2|p|s|p|s||true|[object HTMLCollection]|2|p|s||""#,
        )],
    },
    Case {
        name: "m238",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); const c = a.children; const n = c.length; a.appendChild(document.createElement('u')); return [n, c.length, c[2].tagName].join(); })",
            r#""2,3,U""#,
        )],
    },
    Case {
        name: "m239",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); const c = a.children; return [...c].map(e => e.id).join() + Object.keys(c).join(); })",
            r#""p,s0,1""#,
        )],
    },
    Case {
        name: "m240",
        html: PAGE,
        queries: &[(
            r"E(() => { const d = document.createElement('d'); d.innerHTML === undefined; d.append(document.createElement('x')); d.firstChild.setAttribute('name', 'nm'); return [d.children.nm === d.firstChild, d.children.namedItem('nm') === d.firstChild, Object.keys(d.children).join(), 'nm' in d.children].join(); })",
            r#""true,true,0,true""#,
        )],
    },
    Case {
        name: "m241",
        html: PAGE,
        queries: &[(
            r"E(() => Object.getOwnPropertyNames(Node.prototype).join())",
            r#""nodeType,nodeName,baseURI,isConnected,ownerDocument,parentNode,parentElement,childNodes,firstChild,lastChild,previousSibling,nextSibling,nodeValue,textContent,ELEMENT_NODE,ATTRIBUTE_NODE,TEXT_NODE,CDATA_SECTION_NODE,ENTITY_REFERENCE_NODE,ENTITY_NODE,PROCESSING_INSTRUCTION_NODE,COMMENT_NODE,DOCUMENT_NODE,DOCUMENT_TYPE_NODE,DOCUMENT_FRAGMENT_NODE,NOTATION_NODE,DOCUMENT_POSITION_DISCONNECTED,DOCUMENT_POSITION_PRECEDING,DOCUMENT_POSITION_FOLLOWING,DOCUMENT_POSITION_CONTAINS,DOCUMENT_POSITION_CONTAINED_BY,DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC,appendChild,cloneNode,compareDocumentPosition,contains,getRootNode,hasChildNodes,insertBefore,isDefaultNamespace,isEqualNode,isSameNode,lookupNamespaceURI,lookupPrefix,normalize,removeChild,replaceChild,constructor""#,
        )],
    },
    Case {
        name: "m242",
        html: PAGE,
        queries: &[(
            r"E(() => { const log = []; const a = document.getElementById('a'); const p = document.getElementById('p'); a.insertBefore(p, null); return [...a.children].map(e => e.id).join(); })",
            r#""s,p""#,
        )],
    },
    Case {
        name: "m243",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); const p = document.getElementById('p'); const s = document.getElementById('s'); a.insertBefore(s, p); return [...a.children].map(e => e.id).join() + '|' + a.insertBefore(s, s).id + [...a.children].map(e => e.id).join(); })",
            r#""s,p|ss,p""#,
        )],
    },
    Case {
        name: "m244",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); const f = document.createDocumentFragment(); f.append('1', document.createElement('b'), '2'); a.append(f); return a.childNodes.length + '|' + f.childNodes.length; })",
            r#""5|0""#,
        )],
    },
    Case {
        name: "m245",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); const p = document.getElementById('p'); const r = a.replaceChild(document.createElement('r'), p); return [r === p, p.parentNode, a.firstChild.tagName].join(); })",
            r#""true,,R""#,
        )],
    },
    Case {
        name: "m246",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); const p = document.getElementById('p'); const r = a.replaceChild(p, p); return [r === p, p.parentNode === a].join(); })",
            r#""true,true""#,
        )],
    },
    Case {
        name: "m247",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); const s = document.getElementById('s'); const p = document.getElementById('p'); a.replaceChild(s, p); return [...a.children].map(e => e.id).join(); })",
            r#""s""#,
        )],
    },
    Case {
        name: "m248",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); const s = document.getElementById('s'); const r = a.removeChild(s); return [r === s, s.parentNode, a.childNodes.length].join(); })",
            r#""true,,1""#,
        )],
    },
    Case {
        name: "m249",
        html: PAGE,
        queries: &[(
            r"E(() => { const p = document.getElementById('p'); p.remove(); p.remove(); return [p.parentNode, document.getElementById('a').children.length].join(); })",
            r#"",1""#,
        )],
    },
    Case {
        name: "m250",
        html: PAGE,
        queries: &[(
            r"E(() => { const p = document.getElementById('p'); p.before('B', document.createElement('b')); p.after('A'); return document.getElementById('a').innerHTML; })",
            r#""B<b></b><p id=\"p\">x</p>A<span id=\"s\">y</span>""#,
        )],
    },
    Case {
        name: "m251",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); const p = document.getElementById('p'); const s = document.getElementById('s'); p.before(s); return [...a.childNodes].map(n => n.id || n.data).join(); })",
            r#""s,p""#,
        )],
    },
    Case {
        name: "m252",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); const p = document.getElementById('p'); const s = document.getElementById('s'); p.after(p); return [...a.childNodes].map(n => n.id || n.data).join(); })",
            r#""p,s""#,
        )],
    },
    Case {
        name: "m253",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); const p = document.getElementById('p'); const s = document.getElementById('s'); p.replaceWith(s, 'x'); return [...a.childNodes].map(n => n.id || n.data).join(); })",
            r#""s,x""#,
        )],
    },
    Case {
        name: "m254",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); const p = document.getElementById('p'); p.replaceWith(p); return [...a.childNodes].map(n => n.id || n.data).join(); })",
            r#""p,s""#,
        )],
    },
    Case {
        name: "m255",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); const p = document.getElementById('p'); p.replaceWith(); return [...a.childNodes].map(n => n.id || n.data).join(); })",
            r#""s""#,
        )],
    },
    Case {
        name: "m256",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); const p = document.getElementById('p'); const s = document.getElementById('s'); p.replaceWith(s, p); return [...a.childNodes].map(n => n.id || n.data).join(); })",
            r#""s,p""#,
        )],
    },
    Case {
        name: "m257",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); const p = document.getElementById('p'); const s = document.getElementById('s'); p.before(p, s); return [...a.childNodes].map(n => n.id || n.data).join(); })",
            r#""p,s""#,
        )],
    },
    Case {
        name: "m258",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); const p = document.getElementById('p'); const s = document.getElementById('s'); s.before(s); return [...a.childNodes].map(n => n.id || n.data).join(); })",
            r#""p,s""#,
        )],
    },
    Case {
        name: "m259",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); a.replaceChildren('x', document.createElement('y')); return a.innerHTML; })",
            r#""x<y></y>""#,
        )],
    },
    Case {
        name: "m260",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); a.replaceChildren(); return a.childNodes.length; })",
            r"0",
        )],
    },
    Case {
        name: "m261",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); a.prepend('x', 'y'); return a.innerHTML + '|' + a.firstChild.data + a.childNodes.length; })",
            r#""xy<p id=\"p\">x</p><span id=\"s\">y</span>|x4""#,
        )],
    },
    Case {
        name: "m262",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); a.append(); return a.childNodes.length; })",
            r"2",
        )],
    },
    Case {
        name: "m263",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); const p = document.getElementById('p'); a.append(p, p); return [...a.childNodes].map(n => n.id || n.data).join(); })",
            r#""s,p""#,
        )],
    },
    Case {
        name: "m264",
        html: PAGE,
        queries: &[(
            r"E(() => { const d = document.documentElement; d.after(document.createComment('c')); return document.childNodes.length + '|' + document.lastChild.nodeName; })",
            r#""3|#comment""#,
        )],
    },
    Case {
        name: "m265",
        html: PAGE,
        queries: &[(
            r"E(() => { const d = document.documentElement; d.after(document.createElement('z')); })",
            r#""DOMException HierarchyRequestError: Failed to execute 'after' on 'Element': Only one element on document allowed. [3]""#,
        )],
    },
    Case {
        name: "m266",
        html: PAGE,
        queries: &[(
            r"E(() => { const d = document.doctype; d.remove(); return document.doctype + '|' + document.childNodes.length; })",
            r#""null|1""#,
        )],
    },
    Case {
        name: "m267",
        html: PAGE,
        queries: &[(
            r"E(() => { const d = document.doctype; d.before('x') })",
            r#""DOMException HierarchyRequestError: Failed to execute 'before' on 'DocumentType': Nodes of type '#text' may not be inserted inside nodes of type '#document'. [3]""#,
        )],
    },
    Case {
        name: "m268",
        html: PAGE,
        queries: &[(
            r"E(() => document.head.replaceWith('x') || document.documentElement.childNodes.length)",
            r"2",
        )],
    },
    Case {
        name: "m269",
        html: PAGE,
        queries: &[(
            r"E(() => document.body.append(document.head))",
            r#""undefined""#,
        )],
    },
    Case {
        name: "m270",
        html: PAGE,
        queries: &[(
            r"E(() => document.insertBefore(document.doctype.cloneNode(), document.doctype))",
            r#""DOMException HierarchyRequestError: Failed to execute 'insertBefore' on 'Node': Only one doctype on document allowed. [3]""#,
        )],
    },
    Case {
        name: "m271",
        html: PAGE,
        queries: &[(
            r"E(() => document.insertBefore(document.doctype.cloneNode(), document.documentElement))",
            r#""DOMException HierarchyRequestError: Failed to execute 'insertBefore' on 'Node': Only one doctype on document allowed. [3]""#,
        )],
    },
    Case {
        name: "m272",
        html: PAGE,
        queries: &[(
            r"E(() => document.appendChild(document.doctype.cloneNode()))",
            r#""DOMException HierarchyRequestError: Failed to execute 'appendChild' on 'Node': Can't insert a doctype after the root element. [3]""#,
        )],
    },
    Case {
        name: "m273",
        html: PAGE,
        queries: &[(
            r"E(() => { const f = document.createDocumentFragment(); f.append('x'); document.appendChild(f); })",
            r#""DOMException HierarchyRequestError: Failed to execute 'appendChild' on 'Node': Nodes of type '#text' may not be inserted inside nodes of type '#document'. [3]""#,
        )],
    },
    Case {
        name: "m274",
        html: PAGE,
        queries: &[(
            r"E(() => { const f = document.createDocumentFragment(); f.append('x'); document.insertBefore(f, document.doctype); })",
            r#""DOMException HierarchyRequestError: Failed to execute 'insertBefore' on 'Node': Nodes of type '#text' may not be inserted inside nodes of type '#document'. [3]""#,
        )],
    },
    Case {
        name: "m275",
        html: PAGE,
        queries: &[(
            r"E(() => { const f = document.createDocumentFragment(); f.append(document.createElement('a')); document.insertBefore(f, document.doctype); })",
            r#""DOMException HierarchyRequestError: Failed to execute 'insertBefore' on 'Node': Can't insert an element before a doctype. [3]""#,
        )],
    },
    Case {
        name: "m276",
        html: PAGE,
        queries: &[(
            r"E(() => { document.documentElement.remove(); document.appendChild(document.createElement('z')); return document.documentElement.tagName; })",
            r#""Z""#,
        )],
    },
    Case {
        name: "m277",
        html: PAGE,
        queries: &[(
            r"E(() => { document.documentElement.remove(); document.insertBefore(document.createElement('z'), document.doctype); })",
            r#""DOMException HierarchyRequestError: Failed to execute 'insertBefore' on 'Node': Can't insert an element before a doctype. [3]""#,
        )],
    },
    Case {
        name: "m278",
        html: PAGE,
        queries: &[(
            r"E(() => { document.documentElement.remove(); document.insertBefore(document.createComment('z'), document.doctype); return document.firstChild.nodeName; })",
            r##""#comment""##,
        )],
    },
    Case {
        name: "m279",
        html: PAGE,
        queries: &[(
            r"E(() => document.replaceChild(document.createComment('z'), document.documentElement) && document.childNodes.length)",
            r"2",
        )],
    },
    Case {
        name: "m280",
        html: PAGE,
        queries: &[(
            r"E(() => document.replaceChild(document.createTextNode('z'), document.documentElement))",
            r#""DOMException HierarchyRequestError: Failed to execute 'replaceChild' on 'Node': Nodes of type '#text' may not be inserted inside nodes of type '#document'. [3]""#,
        )],
    },
    Case {
        name: "m281",
        html: PAGE,
        queries: &[(
            r"E(() => document.replaceChild(document.doctype.cloneNode(), document.documentElement))",
            r#""DOMException HierarchyRequestError: Failed to execute 'replaceChild' on 'Node': Only one doctype on document allowed. [3]""#,
        )],
    },
    Case {
        name: "m282",
        html: PAGE,
        queries: &[(
            r"E(() => { const d = document.doctype; document.replaceChild(document.createElement('z'), d); })",
            r#""DOMException HierarchyRequestError: Failed to execute 'replaceChild' on 'Node': Only one element on document allowed. [3]""#,
        )],
    },
    Case {
        name: "m283",
        html: PAGE,
        queries: &[(
            r"E(() => { const d = document.doctype; document.replaceChild(d.cloneNode(), d); return document.childNodes.length; })",
            r"2",
        )],
    },
    Case {
        name: "m284",
        html: PAGE,
        queries: &[(
            r"E(() => { const f = document.createDocumentFragment(); f.append('x'); document.replaceChild(f, document.documentElement); })",
            r#""DOMException HierarchyRequestError: Failed to execute 'replaceChild' on 'Node': Nodes of type '#text' may not be inserted inside nodes of type '#document'. [3]""#,
        )],
    },
    Case {
        name: "m285",
        html: PAGE,
        queries: &[(
            r"E(() => { const f = document.createDocumentFragment(); f.append(document.createElement('a'), document.createElement('b')); document.replaceChild(f, document.documentElement); })",
            r#""DOMException HierarchyRequestError: Failed to execute 'replaceChild' on 'Node': Only one element on document allowed. [3]""#,
        )],
    },
    Case {
        name: "m286",
        html: PAGE,
        queries: &[(
            r"E(() => { const f = document.createDocumentFragment(); f.append(document.createElement('a')); document.replaceChild(f, document.doctype); })",
            r#""DOMException HierarchyRequestError: Failed to execute 'replaceChild' on 'Node': Only one element on document allowed. [3]""#,
        )],
    },
    Case {
        name: "m287",
        html: PAGE,
        queries: &[(
            r"E(() => { const f = document.createDocumentFragment(); return f.appendChild(document.doctype.cloneNode()); })",
            r#""DOMException HierarchyRequestError: Failed to execute 'appendChild' on 'Node': Nodes of type 'html' may not be inserted inside nodes of type '#document-fragment'. [3]""#,
        )],
    },
    Case {
        name: "m288",
        html: PAGE,
        queries: &[(
            r"E(() => document.createDocumentFragment().appendChild(document))",
            r#""DOMException HierarchyRequestError: Failed to execute 'appendChild' on 'Node': Nodes of type '#document' may not be inserted inside nodes of type '#document-fragment'. [3]""#,
        )],
    },
    Case {
        name: "m289",
        html: PAGE,
        queries: &[(
            r"E(() => document.createDocumentFragment().appendChild(document.createDocumentFragment()).nodeName)",
            r##""#document-fragment""##,
        )],
    },
    Case {
        name: "m290",
        html: PAGE,
        queries: &[(
            r"E(() => document.createDocumentFragment().appendChild(document.createComment('c')).nodeName)",
            r##""#comment""##,
        )],
    },
    Case {
        name: "m291",
        html: PAGE,
        queries: &[(
            r"E(() => { const f = document.createDocumentFragment(); f.append('x'); return f.appendChild(f); })",
            r#""DOMException HierarchyRequestError: Failed to execute 'appendChild' on 'Node': The new child element contains the parent. [3]""#,
        )],
    },
    Case {
        name: "m292",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').appendChild(document.getElementById('a').parentNode))",
            r#""DOMException HierarchyRequestError: Failed to execute 'appendChild' on 'Node': The new child element contains the parent. [3]""#,
        )],
    },
    Case {
        name: "m293",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').insertBefore(document.getElementById('a').parentNode, document.getElementById('p')))",
            r#""DOMException HierarchyRequestError: Failed to execute 'insertBefore' on 'Node': The new child element contains the parent. [3]""#,
        )],
    },
    Case {
        name: "m294",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').insertBefore(document.getElementById('s'), document.getElementById('s')) && [...document.getElementById('a').children].map(e => e.id).join())",
            r#""p,s""#,
        )],
    },
    Case {
        name: "m295",
        html: PAGE,
        queries: &[(
            r"E(() => document.getElementById('a').insertBefore(document.getElementById('s'), document.getElementById('p')) && [...document.getElementById('a').children].map(e => e.id).join())",
            r#""s,p""#,
        )],
    },
    Case {
        name: "m296",
        html: PAGE,
        queries: &[(
            r"E(() => document.body.appendChild(document.documentElement))",
            r#""DOMException HierarchyRequestError: Failed to execute 'appendChild' on 'Node': The new child element contains the parent. [3]""#,
        )],
    },
    Case {
        name: "m297",
        html: PAGE,
        queries: &[(
            r"E(() => document.createComment('x').replaceChild(document.createElement('i'), document.createElement('b')))",
            r#""DOMException HierarchyRequestError: Failed to execute 'replaceChild' on 'Node': This node type does not support this method. [3]""#,
        )],
    },
    Case {
        name: "m298",
        html: PAGE,
        queries: &[(
            r"E(() => { const x = document.createElement('x'); const y = document.createElement('y'); x.append(y); y.append(x); })",
            r#""DOMException HierarchyRequestError: Failed to execute 'append' on 'Element': The new child element contains the parent. [3]""#,
        )],
    },
    Case {
        name: "m299",
        html: PAGE,
        queries: &[(
            r"E(() => { const x = document.createElement('x'); x.append(x); })",
            r#""DOMException HierarchyRequestError: Failed to execute 'append' on 'Element': The new child element contains the parent. [3]""#,
        )],
    },
    Case {
        name: "m300",
        html: PAGE,
        queries: &[(
            r"E(() => { const x = document.createElement('x'); x.prepend(x); })",
            r#""DOMException HierarchyRequestError: Failed to execute 'prepend' on 'Element': The new child element contains the parent. [3]""#,
        )],
    },
    Case {
        name: "m301",
        html: PAGE,
        queries: &[(
            r"E(() => { const x = document.createElement('x'); x.replaceChildren(x); })",
            r#""DOMException HierarchyRequestError: Failed to execute 'replaceChildren' on 'Element': The new child element contains the parent. [3]""#,
        )],
    },
    Case {
        name: "m302",
        html: PAGE,
        queries: &[(
            r"E(() => { const x = document.createElement('x'); const y = document.createElement('y'); x.append(y); y.before(x); })",
            r#""DOMException HierarchyRequestError: Failed to execute 'before' on 'Element': The new child element contains the parent. [3]""#,
        )],
    },
    Case {
        name: "m303",
        html: PAGE,
        queries: &[(
            r"E(() => { const x = document.createElement('x'); const y = document.createElement('y'); x.append(y); y.after(x); })",
            r#""DOMException HierarchyRequestError: Failed to execute 'after' on 'Element': The new child element contains the parent. [3]""#,
        )],
    },
    Case {
        name: "m304",
        html: PAGE,
        queries: &[(
            r"E(() => { const x = document.createElement('x'); const y = document.createElement('y'); x.append(y); y.replaceWith(x); })",
            r#""DOMException HierarchyRequestError: Failed to execute 'replaceWith' on 'Element': The new child element contains the parent. [3]""#,
        )],
    },
    Case {
        name: "m305",
        html: PAGE,
        queries: &[(
            r"E(() => { const x = document.createElement('x'); x.append(document.createElement('y')); x.firstChild.replaceChildren(document.createDocumentFragment()); return x.outerHTML; })",
            r#""<x><y></y></x>""#,
        )],
    },
    Case {
        name: "m306",
        html: PAGE,
        queries: &[(
            r"E(() => document.createElement('x').textContent = 'q')",
            r#""q""#,
        )],
    },
    Case {
        name: "m307",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.textContent = 'q'; e.textContent = ''; return e.childNodes.length; })",
            r"0",
        )],
    },
    Case {
        name: "m308",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.append('a','b'); e.textContent = 'z'; return [e.childNodes.length, e.firstChild.data].join(); })",
            r#""1,z""#,
        )],
    },
    Case {
        name: "m309",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.append('a', document.createElement('i'), 'c'); return e.textContent; })",
            r#""ac""#,
        )],
    },
    Case {
        name: "m310",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.append(document.createComment('cm'), 'c'); return e.textContent; })",
            r#""c""#,
        )],
    },
    Case {
        name: "m311",
        html: PAGE,
        queries: &[(
            r"E(() => document.createComment('q').textContent + document.createComment('q').nodeValue)",
            r#""qq""#,
        )],
    },
    Case {
        name: "m312",
        html: PAGE,
        queries: &[(
            r"E(() => document.createDocumentFragment().textContent === '')",
            r"true",
        )],
    },
    Case {
        name: "m313",
        html: PAGE,
        queries: &[(
            r"E(() => { const f = document.createDocumentFragment(); f.textContent = 'x'; return f.firstChild.data; })",
            r#""x""#,
        )],
    },
    Case {
        name: "m315",
        html: PAGE,
        queries: &[(
            r"E(() => { const c = document.createComment('x'); c.nodeValue = 'y'; return c.data; })",
            r#""y""#,
        )],
    },
    Case {
        name: "m316",
        html: PAGE,
        queries: &[(
            r"E(() => { const c = document.createElement('x'); c.nodeValue = 'y'; return c.nodeValue; })",
            r"null",
        )],
    },
    Case {
        name: "m317",
        html: PAGE,
        queries: &[(
            r"E(() => { const c = document.createTextNode('x'); c.nodeValue = undefined; return c.nodeValue; })",
            r#""""#,
        )],
    },
    Case {
        name: "m318",
        html: PAGE,
        queries: &[(
            r"E(() => [document.nodeValue, document.doctype.nodeValue, document.createDocumentFragment().nodeValue].join())",
            r#"",,""#,
        )],
    },
    Case {
        name: "m319",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); return [e.hasAttributes(), (e.setAttribute('a', ''), e.hasAttributes())].join(); })",
            r#""false,true""#,
        )],
    },
    Case {
        name: "m320",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.setAttribute('a', 1); e.setAttribute('b', 2); e.setAttribute('c', 3); return e.getAttributeNames().join() + '|' + Array.prototype.map.call(e.attributes, a => a.name + a.value).join(); })",
            r#""a,b,c|a1,b2,c3""#,
        )],
    },
    Case {
        name: "m321",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.setAttribute('a', 1); const l = e.attributes; const a = l[0]; a.value = 'zz'; return e.getAttribute('a'); })",
            r#""zz""#,
        )],
    },
    Case {
        name: "m322",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); const a = document.createAttribute('a'); a.value = 'v'; e.setAttributeNode(a); a.value = 'w'; return e.getAttribute('a'); })",
            r#""w""#,
        )],
    },
    Case {
        name: "m323",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); const a = document.createAttribute('id'); a.value = 'idv'; document.body.append(e); e.setAttributeNode(a); return document.getElementById('idv') === e; })",
            r"true",
        )],
    },
    Case {
        name: "m324",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.id = 'i1'; document.body.append(e); const a = e.getAttributeNode('id'); a.value = 'i2'; return [document.getElementById('i1'), document.getElementById('i2') === e].join(); })",
            r#"",true""#,
        )],
    },
    Case {
        name: "m325",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.id = 'i1'; document.body.append(e); e.removeAttribute('id'); return document.getElementById('i1'); })",
            r"null",
        )],
    },
    Case {
        name: "m326",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.setAttribute('id', 'i1'); document.body.append(e); e.attributes.removeNamedItem('id'); return document.getElementById('i1'); })",
            r"null",
        )],
    },
    Case {
        name: "m327",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); const a = e.attributes; e.setAttribute('q', 1); return [a.length, a.q.value, a === e.attributes].join(); })",
            r#""1,1,true""#,
        )],
    },
    Case {
        name: "m328",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); const a = document.createAttribute('q'); a.value = '1'; const m = e.attributes; const r = m.setNamedItem(a); return [r, m.length, m.q === a].join(); })",
            r#"",1,true""#,
        )],
    },
    Case {
        name: "m329",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.setAttribute('q', '1'); const r = e.attributes.removeNamedItem('Q'); return r.name + r.value + e.attributes.length; })",
            r#""q10""#,
        )],
    },
    Case {
        name: "m330",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElementNS('http://x', 'x'); e.setAttributeNS('http://y', 'p:q', '1'); e.setAttributeNS('http://y', 'p:q', '2'); e.setAttributeNS('http://z', 'p:q', '3'); return [e.attributes.length, e.getAttributeNS('http://y', 'q'), e.getAttribute('p:q'), e.getAttributeNS('http://z', 'q'), e.hasAttributeNS('http://y', 'q'), e.hasAttributeNS('', 'q'), e.getAttributeNS(null, 'q')].join(); })",
            r#""2,2,2,3,true,false,""#,
        )],
    },
    Case {
        name: "m331",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElementNS('http://x', 'x'); e.setAttributeNS('http://y', 'p:q', '1'); e.setAttributeNS('http://y', 'r:q', '2'); return [e.attributes.length, e.attributes[0].name, e.getAttribute('p:q'), e.getAttribute('r:q')].join(); })",
            r#""1,p:q,2,""#,
        )],
    },
    Case {
        name: "m332",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElementNS('http://x', 'x'); e.setAttributeNS('http://y', 'p:q', '1'); e.removeAttributeNS('http://y', 'q'); return e.attributes.length; })",
            r"0",
        )],
    },
    Case {
        name: "m333",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElementNS('http://x', 'x'); e.setAttributeNS(null, 'q', '1'); return [e.getAttribute('q'), e.getAttributeNS('', 'q'), e.getAttributeNS(undefined, 'q'), e.getAttributeNS('http://y', 'q')].join(); })",
            r#""1,1,1,""#,
        )],
    },
    Case {
        name: "m334",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.setAttributeNS(undefined, 'q', 1); return e.getAttribute('q'); })",
            r#""1""#,
        )],
    },
    Case {
        name: "m335",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.setAttribute('q', 1); e.toggleAttribute('q'); return e.hasAttribute('q'); })",
            r"false",
        )],
    },
    Case {
        name: "m336",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.setAttribute('q', 1); return [e.toggleAttribute('q', true), e.getAttribute('q'), e.toggleAttribute('Q', true)].join(); })",
            r#""true,1,true""#,
        )],
    },
    Case {
        name: "m337",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); return [e.toggleAttribute('q', undefined), e.toggleAttribute('q', null), e.toggleAttribute('q', 0), e.toggleAttribute('z', 1)].join(); })",
            r#""true,false,false,true""#,
        )],
    },
    Case {
        name: "m338",
        html: PAGE,
        queries: &[(
            r"E(() => { const s = document.createElement('script'); s.setAttribute('async', ''); return [s.async, s.hasAttribute('async')].join(); })",
            r#""true,true""#,
        )],
    },
    Case {
        name: "m339",
        html: PAGE,
        queries: &[(
            r"E(() => { const s = document.createElement('script'); s.async = false; s.setAttribute('async', ''); return s.async; })",
            r"true",
        )],
    },
    Case {
        name: "m340",
        html: PAGE,
        queries: &[(
            r"E(() => { const s = document.createElement('script'); s.async = false; return [s.async, s.hasAttribute('async')].join(); })",
            r#""false,false""#,
        )],
    },
    Case {
        name: "m341",
        html: PAGE,
        queries: &[(
            r"E(() => { const s = document.createElement('script'); s.async = false; s.removeAttribute('async'); return s.async; })",
            r"false",
        )],
    },
    Case {
        name: "m342",
        html: PAGE,
        queries: &[(
            r"E(() => { const s = document.createElement('script'); s.setAttribute('async', ''); s.removeAttribute('async'); return s.async; })",
            r"false",
        )],
    },
    Case {
        name: "m343",
        html: PAGE,
        queries: &[(
            r"E(() => { window.__o = []; const s = document.createElement('script'); s.textContent = '__o.push(1)'; document.head.appendChild(s); s.setAttribute('src', 'data:text/javascript,__o.push(2)'); return __o.join(); })",
            r#""1""#,
        )],
    },
    Case {
        name: "m344",
        html: PAGE,
        queries: &[(
            r"E(() => { window.__o2 = []; const s = document.createElement('script'); s.setAttribute('src', 'data:text/javascript,__o2.push(2)'); const t = document.createElement('script'); document.head.appendChild(t); t.setAttribute('src', 'data:text/javascript,__o2.push(3)'); return __o2.join(); })",
            r#""""#,
        )],
    },
    Case {
        name: "m345",
        html: PAGE,
        queries: &[(
            r"E(() => { window.__o3 = []; const s = document.createElement('script'); document.head.appendChild(s); s.textContent = '__o3.push(1)'; s.textContent = '__o3.push(2)'; return __o3.join(); })",
            r#""1""#,
        )],
    },
    Case {
        name: "m346",
        html: PAGE,
        queries: &[(
            r"E(() => { window.__o4 = []; const s = document.createElement('script'); document.head.appendChild(s); s.appendChild(document.createTextNode('__o4.push(1)')); s.appendChild(document.createTextNode('__o4.push(2)')); return __o4.join(); })",
            r#""1""#,
        )],
    },
    Case {
        name: "m347",
        html: PAGE,
        queries: &[(
            r"E(() => { window.__o5 = []; const s = document.createElement('script'); s.appendChild(document.createTextNode('__o5.push(1)')); document.head.appendChild(s); s.firstChild.data = '__o5.push(2)'; return __o5.join(); })",
            r#""1""#,
        )],
    },
    Case {
        name: "m348",
        html: PAGE,
        queries: &[(
            r"E(() => { window.__o6 = []; const s = document.createElement('script'); document.head.appendChild(s); const t = document.createTextNode('__o6.push(1)'); s.appendChild(t); return __o6.join(); })",
            r#""1""#,
        )],
    },
    Case {
        name: "m349",
        html: PAGE,
        queries: &[(
            r"E(() => { window.__o7 = []; const s = document.createElement('script'); document.head.appendChild(s); s.append('__o7.push(1)'); s.firstChild.nodeValue = '__o7.push(2)'; return __o7.join(); })",
            r#""1""#,
        )],
    },
    Case {
        name: "m350",
        html: PAGE,
        queries: &[(
            r"E(() => { window.__o8 = []; const d = document.createElement('div'); const s = document.createElement('script'); s.textContent = '__o8.push(1)'; d.appendChild(s); document.body.appendChild(d); return __o8.join(); })",
            r#""1""#,
        )],
    },
    Case {
        name: "m351",
        html: PAGE,
        queries: &[(
            r"E(() => { window.__o9 = []; const s = document.createElement('script'); s.textContent = '__o9.push(1)'; const f = document.createDocumentFragment(); f.append(s); document.body.append(f); return __o9.join(); })",
            r#""1""#,
        )],
    },
    Case {
        name: "m352",
        html: PAGE,
        queries: &[(
            r"E(() => { window.__o10 = []; const s = document.createElement('script'); s.textContent = '__o10.push(1)'; document.body.append(s); s.remove(); document.body.append(s); return __o10.join(); })",
            r#""1""#,
        )],
    },
    Case {
        name: "m353",
        html: PAGE,
        queries: &[(
            r"E(() => { window.__o11 = []; const s = document.createElement('script'); s.textContent = '__o11.push(1)'; const s2 = document.createElement('script'); s2.textContent = '__o11.push(2)'; document.body.append(s, s2); return __o11.join(); })",
            r#""1,2""#,
        )],
    },
    Case {
        name: "m354",
        html: PAGE,
        queries: &[(
            r"E(() => { window.__o12 = []; const s = document.createElement('script'); s.textContent = '__o12.push(1); s2.remove()'; window.s2 = document.createElement('script'); s2.textContent = '__o12.push(2)'; document.body.append(s, s2); return __o12.join(); })",
            r#""1""#,
        )],
    },
    Case {
        name: "m355",
        html: PAGE,
        queries: &[(
            r"E(() => { window.__o13 = []; const s = document.createElement('script'); s.textContent = '__o13.push(document.currentScript === window.sx)'; window.sx = s; document.head.append(s); return __o13.join() + document.currentScript; })",
            r#""truenull""#,
        )],
    },
    Case {
        name: "m356",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.classList.add('a', 'b'); e.classList.add('c'); return e.className; })",
            r#""a b c""#,
        )],
    },
    Case {
        name: "m357",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.classList.add('a', ''); return e.className; })",
            r#""DOMException SyntaxError: Failed to execute 'add' on 'DOMTokenList': The token provided must not be empty. [12]""#,
        )],
    },
    Case {
        name: "m358",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.classList.add('a', 'b c'); return e.className; })",
            r#""DOMException InvalidCharacterError: Failed to execute 'add' on 'DOMTokenList': The token provided ('b c') contains HTML space characters, which are not valid in tokens. [5]""#,
        )],
    },
    Case {
        name: "m359",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.className = 'a'; e.classList.add('b', ''); return e.className; })",
            r#""DOMException SyntaxError: Failed to execute 'add' on 'DOMTokenList': The token provided must not be empty. [12]""#,
        )],
    },
    Case {
        name: "m360",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.className = 'a'; e.classList.remove('a', ''); return e.className; })",
            r#""DOMException SyntaxError: Failed to execute 'remove' on 'DOMTokenList': The token provided must not be empty. [12]""#,
        )],
    },
    Case {
        name: "m361",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); return [e.classList.toggle('a', true), e.className, e.classList.toggle('a', false), JSON.stringify(e.className), e.hasAttribute('class')].join(); })",
            r#""true,a,false,\"\",true""#,
        )],
    },
    Case {
        name: "m362",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); return [e.classList.toggle('a', false), e.hasAttribute('class')].join(); })",
            r#""false,false""#,
        )],
    },
    Case {
        name: "m363",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.classList.add(1, null, undefined); return e.className; })",
            r#""1 null undefined""#,
        )],
    },
    Case {
        name: "m364",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.classList.add({toString() { return 'q' }}); return e.className; })",
            r#""q""#,
        )],
    },
    Case {
        name: "m365",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.classList.add(Symbol()); })",
            r#""TypeError: Failed to execute 'add' on 'DOMTokenList': Cannot convert a Symbol value to a string""#,
        )],
    },
    Case {
        name: "m366",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.className = 'a'; return Object.getOwnPropertyDescriptor(e.classList, '0') && JSON.stringify(Object.getOwnPropertyDescriptor(e.classList, '0')); })",
            r#""{\"value\":\"a\",\"writable\":false,\"enumerable\":true,\"configurable\":true}""#,
        )],
    },
    Case {
        name: "m367",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.className = 'a'; e.classList[0] = 'q'; return e.className; })",
            r#""a""#,
        )],
    },
    Case {
        name: "m368",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.className = 'a'; e.classList.length = 5; return e.classList.length; })",
            r"1",
        )],
    },
    Case {
        name: "m369",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); const c = e.classList; e.classList = undefined; return JSON.stringify(c.value) + e.className; })",
            r#""\"undefined\"undefined""#,
        )],
    },
    Case {
        name: "m370",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.classList.value = 'q'; return e.className; })",
            r#""q""#,
        )],
    },
    Case {
        name: "m371",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.setAttribute('data-foo', '1'); e.setAttribute('DATA-bar', '2'); return JSON.stringify(Object.keys(e.dataset)) + e.dataset.foo + e.dataset.bar; })",
            r#""[\"foo\",\"bar\"]12""#,
        )],
    },
    Case {
        name: "m372",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.setAttribute('data-a-b', '1'); return [e.dataset.aB, e.dataset['a-b'], 'aB' in e.dataset, 'a-b' in e.dataset, e.dataset.hasOwnProperty('aB')].join(); })",
            r#""1,,true,false,true""#,
        )],
    },
    Case {
        name: "m373",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.dataset.x = 1; delete e.dataset.x; delete e.dataset.nonexistent; return [e.hasAttribute('data-x'), delete e.dataset.zz].join(); })",
            r#""false,true""#,
        )],
    },
    Case {
        name: "m374",
        html: PAGE,
        queries: &[(
            r"E(() => { 'use strict'; const e = document.createElement('x'); return delete e.dataset.zz; })",
            r"true",
        )],
    },
    Case {
        name: "m375",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.dataset.x = 1; Object.defineProperty(e.dataset, 'y', {value: 2, configurable: true, writable: true, enumerable: true}); return e.outerHTML; })",
            r#""<x data-x=\"1\" data-y=\"2\"></x>""#,
        )],
    },
    Case {
        name: "m376",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); Object.defineProperty(e.dataset, 'y', {get() { return 1; }}); })",
            r#""TypeError: Failed to set a named property 'y' on 'DOMStringMap': Accessor properties are not allowed.""#,
        )],
    },
    Case {
        name: "m377",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.dataset[Symbol.toStringTag] = 1; return Object.prototype.toString.call(e.dataset); })",
            r#""[object DOMStringMap]""#,
        )],
    },
    Case {
        name: "m378",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.dataset[5] = 'five'; return [e.outerHTML, e.dataset[5], e.dataset['5']].join('|'); })",
            r#""<x data-5=\"five\"></x>|five|five""#,
        )],
    },
    Case {
        name: "m379",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.dataset.fooBAR = 'q'; return e.outerHTML; })",
            r#""<x data-foo-b-a-r=\"q\"></x>""#,
        )],
    },
    Case {
        name: "m380",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.dataset.a_b = 'q'; e.dataset.a1 = 'q'; return e.outerHTML; })",
            r#""<x data-a_b=\"q\" data-a1=\"q\"></x>""#,
        )],
    },
    Case {
        name: "m381",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.dataset.__proto__ = 'q'; return e.outerHTML + typeof e.dataset.__proto__ ; })",
            r#""<x data-__proto__=\"q\"></x>string""#,
        )],
    },
    Case {
        name: "m382",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.dataset.constructor = 'q'; return e.outerHTML + typeof e.dataset.constructor ; })",
            r#""<x data-constructor=\"q\"></x>string""#,
        )],
    },
    Case {
        name: "m383",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.setAttribute('data-length', '3'); return e.dataset.length; })",
            r#""3""#,
        )],
    },
    Case {
        name: "m384",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); e.dataset.x = 1; return JSON.stringify(e.dataset) + Object.entries(e.dataset); })",
            r#""{\"x\":\"1\"}x,1""#,
        )],
    },
    Case {
        name: "m385",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('x'); const d = e.dataset; e.dataset = {a: 1}; return [JSON.stringify(d), e.dataset === d, e.outerHTML].join(); })",
            r#""{},true,<x></x>""#,
        )],
    },
    Case {
        name: "m386",
        html: PAGE,
        queries: &[(
            r"E(() => { const c = document.getElementById('a').children; return Object.getOwnPropertyNames(c).join() + '|' + Reflect.ownKeys(c).length; })",
            r#""0,1,p,s|4""#,
        )],
    },
    Case {
        name: "m387",
        html: PAGE,
        queries: &[(
            r"E(() => { const c = document.getElementById('a').children; c.foo = 1; c[7] = 2; return [c.foo, c[7], Object.keys(c).join()].join(); })",
            r#""1,,0,1,foo""#,
        )],
    },
    Case {
        name: "m388",
        html: PAGE,
        queries: &[(
            r"E(() => { const c = document.getElementById('a').children; return [c.p === c[0], 'p' in c, 's' in c, 'x' in c, c.hasOwnProperty('p'), c.hasOwnProperty(0), Object.getOwnPropertyDescriptor(c, 'p') && JSON.stringify(Object.getOwnPropertyDescriptor(c, 'p'))].join(); })",
            r#""true,true,true,false,true,true,{\"value\":{},\"writable\":false,\"enumerable\":false,\"configurable\":true}""#,
        )],
    },
    Case {
        name: "m389",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); a.children.p = 5; return typeof a.children.p; })",
            r#""object""#,
        )],
    },
    Case {
        name: "m390",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); const c = a.children; return [c.namedItem(''), c.namedItem(), c.item(), c.item(-1), c.item('1').id, c.item(1.9).id, c.item(NaN).id, c.namedItem(null)].join(); })",
            r#""TypeError: Failed to execute 'namedItem' on 'HTMLCollection': 1 argument required, but only 0 present.""#,
        )],
    },
    Case {
        name: "m391",
        html: PAGE,
        queries: &[(
            r"E(() => { const c = document.getElementById('a').children; return JSON.stringify(Object.getOwnPropertyDescriptor(c, '0')); })",
            r#""{\"value\":{},\"writable\":false,\"enumerable\":true,\"configurable\":true}""#,
        )],
    },
    Case {
        name: "m392",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); a.firstElementChild.name = 'nm'; return [a.children.nm, a.children.namedItem('nm').id].join(); })",
            r#""TypeError: Cannot read properties of null (reading 'id')""#,
        )],
    },
    Case {
        name: "m393",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); a.firstElementChild.setAttribute('name', 'nm'); a.lastElementChild.id = 'nm'; return [a.children.nm.id, a.children.namedItem('nm').id].join(); })",
            r#""nm,nm""#,
        )],
    },
    Case {
        name: "m394",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.createElementNS('http://x', 'a'); const b = document.createElementNS('http://x', 'b'); b.setAttribute('name', 'nm'); a.append(b); return [a.children.nm, a.children.namedItem('nm')].join(); })",
            r#"",""#,
        )],
    },
    Case {
        name: "m395",
        html: PAGE,
        queries: &[(
            r"E(() => { const l = document.getElementById('a').childNodes; return [l.length, l.item(0).id, l[1].id, l.item(2), l[2], typeof l.forEach, [...l].length, l.keys().next().value, Object.prototype.toString.call(l)].join(); })",
            r#""2,p,s,,,function,2,0,[object NodeList]""#,
        )],
    },
    Case {
        name: "m396",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); const l = a.childNodes; a.append('t'); return [l.length, l[2].data].join(); })",
            r#""3,t""#,
        )],
    },
    Case {
        name: "m397",
        html: PAGE,
        queries: &[(
            r"E(() => { const a = document.getElementById('a'); const l = a.childNodes; const p = a.firstChild; a.removeChild(p); return [l.length, l[0].id].join(); })",
            r#""1,s""#,
        )],
    },
    Case {
        name: "m398",
        html: PAGE,
        queries: &[(
            r"E(() => Object.getOwnPropertyDescriptor(Element.prototype, 'children').get.call(document.createElement('i')).length)",
            r"0",
        )],
    },
    Case {
        name: "m399",
        html: PAGE,
        queries: &[(
            r"E(() => Element.prototype.append.call(document, 'x'))",
            r#""TypeError: Illegal invocation""#,
        )],
    },
    Case {
        name: "m400",
        html: PAGE,
        queries: &[(
            r"E(() => Element.prototype.getAttribute.call(document, 'x'))",
            r#""TypeError: Illegal invocation""#,
        )],
    },
    Case {
        name: "m401",
        html: PAGE,
        queries: &[(
            r"E(() => Element.prototype.getAttribute.call({}, 'x'))",
            r#""TypeError: Illegal invocation""#,
        )],
    },
    Case {
        name: "m402",
        html: PAGE,
        queries: &[(
            r"E(() => Node.prototype.appendChild.call({}, document.createElement('i')))",
            r#""TypeError: Illegal invocation""#,
        )],
    },
    Case {
        name: "m403",
        html: PAGE,
        queries: &[(
            r"E(() => Node.prototype.cloneNode.call(null))",
            r#""TypeError: Illegal invocation""#,
        )],
    },
    Case {
        name: "m404",
        html: PAGE,
        queries: &[(
            r"E(() => Element.prototype.classList)",
            r#""TypeError: Illegal invocation""#,
        )],
    },
    Case {
        name: "m405",
        html: PAGE,
        queries: &[(
            r"E(() => DOMTokenList.prototype.add.call({}, 'x'))",
            r#""TypeError: Illegal invocation""#,
        )],
    },
    Case {
        name: "m406",
        html: PAGE,
        queries: &[(
            r"E(() => new DOMTokenList())",
            r#""TypeError: Failed to construct 'DOMTokenList': Illegal constructor""#,
        )],
    },
    Case {
        name: "m407",
        html: PAGE,
        queries: &[(
            r"E(() => new Attr())",
            r#""TypeError: Failed to construct 'Attr': Illegal constructor""#,
        )],
    },
    Case {
        name: "m408",
        html: PAGE,
        queries: &[(
            r"E(() => new Node())",
            r#""TypeError: Failed to construct 'Node': Illegal constructor""#,
        )],
    },
    Case {
        name: "m409",
        html: PAGE,
        queries: &[(
            r"E(() => new NamedNodeMap())",
            r#""TypeError: Failed to construct 'NamedNodeMap': Illegal constructor""#,
        )],
    },
    Case {
        name: "m410",
        html: PAGE,
        queries: &[(
            r"E(() => new HTMLCollection())",
            r#""TypeError: Failed to construct 'HTMLCollection': Illegal constructor""#,
        )],
    },
    Case {
        name: "m411",
        html: PAGE,
        queries: &[(
            r"E(() => new DOMStringMap())",
            r#""TypeError: Failed to construct 'DOMStringMap': Illegal constructor""#,
        )],
    },
    Case {
        name: "m412",
        html: PAGE,
        queries: &[(
            r"[OK(() => document.createElement('i').setAttribute('a=b', 'x')), OK(() => document.createElement('i').toggleAttribute('a=b')), OK(() => document.createAttribute('a=b')), OK(() => document.createElement('a=b')), OK(() => document.createElementNS('http://x', 'a=b')), OK(() => document.createElementNS('http://x', 'p:a=b')), OK(() => document.createElement('i').setAttributeNS('http://x', 'a=b', 'v'))].join()",
            r#""InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,ok,ok,ok,InvalidCharacterError""#,
        )],
    },
    Case {
        name: "m413",
        html: PAGE,
        queries: &[(
            r"[OK(() => document.createElement('i').setAttribute('a/b', 'x')), OK(() => document.createElement('i').toggleAttribute('a/b')), OK(() => document.createAttribute('a/b')), OK(() => document.createElement('a/b')), OK(() => document.createElementNS('http://x', 'a/b')), OK(() => document.createElementNS('http://x', 'p:a/b')), OK(() => document.createElement('i').setAttributeNS('http://x', 'a/b', 'v'))].join()",
            r#""InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError""#,
        )],
    },
    Case {
        name: "m414",
        html: PAGE,
        queries: &[(
            r"[OK(() => document.createElement('i').setAttribute('a>b', 'x')), OK(() => document.createElement('i').toggleAttribute('a>b')), OK(() => document.createAttribute('a>b')), OK(() => document.createElement('a>b')), OK(() => document.createElementNS('http://x', 'a>b')), OK(() => document.createElementNS('http://x', 'p:a>b')), OK(() => document.createElement('i').setAttributeNS('http://x', 'a>b', 'v'))].join()",
            r#""InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError""#,
        )],
    },
    Case {
        name: "m415",
        html: PAGE,
        queries: &[(
            r"[OK(() => document.createElement('i').setAttribute('a<b', 'x')), OK(() => document.createElement('i').toggleAttribute('a<b')), OK(() => document.createAttribute('a<b')), OK(() => document.createElement('a<b')), OK(() => document.createElementNS('http://x', 'a<b')), OK(() => document.createElementNS('http://x', 'p:a<b')), OK(() => document.createElement('i').setAttributeNS('http://x', 'a<b', 'v'))].join()",
            r#""ok,ok,ok,ok,ok,ok,ok""#,
        )],
    },
    Case {
        name: "m416",
        html: PAGE,
        queries: &[(
            r#"[OK(() => document.createElement('i').setAttribute('a"b', 'x')), OK(() => document.createElement('i').toggleAttribute('a"b')), OK(() => document.createAttribute('a"b')), OK(() => document.createElement('a"b')), OK(() => document.createElementNS('http://x', 'a"b')), OK(() => document.createElementNS('http://x', 'p:a"b')), OK(() => document.createElement('i').setAttributeNS('http://x', 'a"b', 'v'))].join()"#,
            r#""ok,ok,ok,ok,ok,ok,ok""#,
        )],
    },
    Case {
        name: "m417",
        html: PAGE,
        queries: &[(
            r"[OK(() => document.createElement('i').setAttribute('a@b', 'x')), OK(() => document.createElement('i').toggleAttribute('a@b')), OK(() => document.createAttribute('a@b')), OK(() => document.createElement('a@b')), OK(() => document.createElementNS('http://x', 'a@b')), OK(() => document.createElementNS('http://x', 'p:a@b')), OK(() => document.createElement('i').setAttributeNS('http://x', 'a@b', 'v'))].join()",
            r#""ok,ok,ok,ok,ok,ok,ok""#,
        )],
    },
    Case {
        name: "m418",
        html: PAGE,
        queries: &[(
            r"[OK(() => document.createElement('i').setAttribute('\u00e9', 'x')), OK(() => document.createElement('i').toggleAttribute('\u00e9')), OK(() => document.createAttribute('\u00e9')), OK(() => document.createElement('\u00e9')), OK(() => document.createElementNS('http://x', '\u00e9')), OK(() => document.createElementNS('http://x', 'p:\u00e9')), OK(() => document.createElement('i').setAttributeNS('http://x', '\u00e9', 'v'))].join()",
            r#""ok,ok,ok,ok,ok,ok,ok""#,
        )],
    },
    Case {
        name: "m419",
        html: PAGE,
        queries: &[(
            r"[OK(() => document.createElement('i').setAttribute('a\tb', 'x')), OK(() => document.createElement('i').toggleAttribute('a\tb')), OK(() => document.createAttribute('a\tb')), OK(() => document.createElement('a\tb')), OK(() => document.createElementNS('http://x', 'a\tb')), OK(() => document.createElementNS('http://x', 'p:a\tb')), OK(() => document.createElement('i').setAttributeNS('http://x', 'a\tb', 'v'))].join()",
            r#""InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError""#,
        )],
    },
    Case {
        name: "m420",
        html: PAGE,
        queries: &[(
            r"[OK(() => document.createElement('i').setAttribute('-a', 'x')), OK(() => document.createElement('i').toggleAttribute('-a')), OK(() => document.createAttribute('-a')), OK(() => document.createElement('-a')), OK(() => document.createElementNS('http://x', '-a')), OK(() => document.createElementNS('http://x', 'p:-a')), OK(() => document.createElement('i').setAttributeNS('http://x', '-a', 'v'))].join()",
            r#""ok,ok,ok,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,ok""#,
        )],
    },
    Case {
        name: "m421",
        html: PAGE,
        queries: &[(
            r"[OK(() => document.createElement('i').setAttribute('.a', 'x')), OK(() => document.createElement('i').toggleAttribute('.a')), OK(() => document.createAttribute('.a')), OK(() => document.createElement('.a')), OK(() => document.createElementNS('http://x', '.a')), OK(() => document.createElementNS('http://x', 'p:.a')), OK(() => document.createElement('i').setAttributeNS('http://x', '.a', 'v'))].join()",
            r#""ok,ok,ok,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,ok""#,
        )],
    },
    Case {
        name: "m422",
        html: PAGE,
        queries: &[(
            r"[OK(() => document.createElement('i').setAttribute('1', 'x')), OK(() => document.createElement('i').toggleAttribute('1')), OK(() => document.createAttribute('1')), OK(() => document.createElement('1')), OK(() => document.createElementNS('http://x', '1')), OK(() => document.createElementNS('http://x', 'p:1')), OK(() => document.createElement('i').setAttributeNS('http://x', '1', 'v'))].join()",
            r#""ok,ok,ok,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,ok""#,
        )],
    },
    Case {
        name: "m423",
        html: PAGE,
        queries: &[(
            r"[OK(() => document.createElement('i').setAttribute('a\u0000b', 'x')), OK(() => document.createElement('i').toggleAttribute('a\u0000b')), OK(() => document.createAttribute('a\u0000b')), OK(() => document.createElement('a\u0000b')), OK(() => document.createElementNS('http://x', 'a\u0000b')), OK(() => document.createElementNS('http://x', 'p:a\u0000b')), OK(() => document.createElement('i').setAttributeNS('http://x', 'a\u0000b', 'v'))].join()",
            r#""InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError""#,
        )],
    },
    Case {
        name: "m424",
        html: PAGE,
        queries: &[(
            r"[OK(() => document.createElement('i').setAttribute('a:b', 'x')), OK(() => document.createElement('i').toggleAttribute('a:b')), OK(() => document.createAttribute('a:b')), OK(() => document.createElement('a:b')), OK(() => document.createElementNS('http://x', 'a:b')), OK(() => document.createElementNS('http://x', 'p:a:b')), OK(() => document.createElement('i').setAttributeNS('http://x', 'a:b', 'v'))].join()",
            r#""ok,ok,ok,ok,ok,ok,ok""#,
        )],
    },
    Case {
        name: "m425",
        html: PAGE,
        queries: &[(
            r"[OK(() => document.createElement('i').setAttribute(':', 'x')), OK(() => document.createElement('i').toggleAttribute(':')), OK(() => document.createAttribute(':')), OK(() => document.createElement(':')), OK(() => document.createElementNS('http://x', ':')), OK(() => document.createElementNS('http://x', 'p::')), OK(() => document.createElement('i').setAttributeNS('http://x', ':', 'v'))].join()",
            r#""ok,ok,ok,ok,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError""#,
        )],
    },
    Case {
        name: "m426",
        html: PAGE,
        queries: &[(
            r"[OK(() => document.createElement('i').setAttribute('::', 'x')), OK(() => document.createElement('i').toggleAttribute('::')), OK(() => document.createAttribute('::')), OK(() => document.createElement('::')), OK(() => document.createElementNS('http://x', '::')), OK(() => document.createElementNS('http://x', 'p:::')), OK(() => document.createElement('i').setAttributeNS('http://x', '::', 'v'))].join()",
            r#""ok,ok,ok,ok,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError""#,
        )],
    },
    Case {
        name: "m427",
        html: PAGE,
        queries: &[(
            r"[OK(() => document.createElement('i').setAttribute('[a]', 'x')), OK(() => document.createElement('i').toggleAttribute('[a]')), OK(() => document.createAttribute('[a]')), OK(() => document.createElement('[a]')), OK(() => document.createElementNS('http://x', '[a]')), OK(() => document.createElementNS('http://x', 'p:[a]')), OK(() => document.createElement('i').setAttributeNS('http://x', '[a]', 'v'))].join()",
            r#""ok,ok,ok,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,ok""#,
        )],
    },
    Case {
        name: "m428",
        html: PAGE,
        queries: &[(
            r"[OK(() => document.createElement('i').setAttribute('{', 'x')), OK(() => document.createElement('i').toggleAttribute('{')), OK(() => document.createAttribute('{')), OK(() => document.createElement('{')), OK(() => document.createElementNS('http://x', '{')), OK(() => document.createElementNS('http://x', 'p:{')), OK(() => document.createElement('i').setAttributeNS('http://x', '{', 'v'))].join()",
            r#""ok,ok,ok,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,ok""#,
        )],
    },
    Case {
        name: "m429",
        html: PAGE,
        queries: &[(
            r"[OK(() => document.createElement('i').setAttribute('a\u00b7', 'x')), OK(() => document.createElement('i').toggleAttribute('a\u00b7')), OK(() => document.createAttribute('a\u00b7')), OK(() => document.createElement('a\u00b7')), OK(() => document.createElementNS('http://x', 'a\u00b7')), OK(() => document.createElementNS('http://x', 'p:a\u00b7')), OK(() => document.createElement('i').setAttributeNS('http://x', 'a\u00b7', 'v'))].join()",
            r#""ok,ok,ok,ok,ok,ok,ok""#,
        )],
    },
    Case {
        name: "m430",
        html: PAGE,
        queries: &[(
            r"[OK(() => document.createElement('i').setAttribute('a!', 'x')), OK(() => document.createElement('i').toggleAttribute('a!')), OK(() => document.createAttribute('a!')), OK(() => document.createElement('a!')), OK(() => document.createElementNS('http://x', 'a!')), OK(() => document.createElementNS('http://x', 'p:a!')), OK(() => document.createElement('i').setAttributeNS('http://x', 'a!', 'v'))].join()",
            r#""ok,ok,ok,ok,ok,ok,ok""#,
        )],
    },
    Case {
        name: "m431",
        html: PAGE,
        queries: &[(
            r"[OK(() => document.createElement('i').setAttribute(' a', 'x')), OK(() => document.createElement('i').toggleAttribute(' a')), OK(() => document.createAttribute(' a')), OK(() => document.createElement(' a')), OK(() => document.createElementNS('http://x', ' a')), OK(() => document.createElementNS('http://x', 'p: a')), OK(() => document.createElement('i').setAttributeNS('http://x', ' a', 'v'))].join()",
            r#""InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError""#,
        )],
    },
    Case {
        name: "m432",
        html: PAGE,
        queries: &[(
            r"[OK(() => document.createElement('i').setAttribute('a ', 'x')), OK(() => document.createElement('i').toggleAttribute('a ')), OK(() => document.createAttribute('a ')), OK(() => document.createElement('a ')), OK(() => document.createElementNS('http://x', 'a ')), OK(() => document.createElementNS('http://x', 'p:a ')), OK(() => document.createElement('i').setAttributeNS('http://x', 'a ', 'v'))].join()",
            r#""InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError,InvalidCharacterError""#,
        )],
    },
    Case {
        name: "m433",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.setAttribute('a', 'x'); const n = e.attributes; n.foo = 1; n[5] = 2; return [Object.keys(n).join(), n.foo, n[5]].join('|'); })",
            r#""0,foo|1|""#,
        )],
    },
    Case {
        name: "m434",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.setAttribute('a', 'x'); return [typeof e.dataset, typeof document.createElement('p').dataset].join(); })",
            r#""object,object""#,
        )],
    },
    Case {
        name: "m435",
        html: PAGE,
        queries: &[(
            r"E(() => Object.getOwnPropertyNames(CharacterData.prototype).join() + '|' + Object.getOwnPropertyNames(DocumentType.prototype).join())",
            r#""data,length,previousElementSibling,nextElementSibling,after,appendData,before,deleteData,insertData,remove,replaceData,replaceWith,substringData,constructor|name,publicId,systemId,after,before,remove,replaceWith,constructor""#,
        )],
    },
    Case {
        name: "m436",
        html: PAGE,
        queries: &[(
            r"E(() => Object.getOwnPropertyNames(NamedNodeMap.prototype).join() + '|' + Object.getOwnPropertyNames(DOMTokenList.prototype).join() + '|' + Object.getOwnPropertyNames(DOMStringMap.prototype).join() + '|' + Object.getOwnPropertyNames(Attr.prototype).join() + '|' + Object.getOwnPropertyNames(HTMLCollection.prototype).join())",
            r#""length,getNamedItem,getNamedItemNS,item,removeNamedItem,removeNamedItemNS,setNamedItem,setNamedItemNS,constructor|entries,keys,values,forEach,length,value,add,contains,item,remove,replace,supports,toggle,toString,constructor|constructor|namespaceURI,prefix,localName,name,value,ownerElement,specified,constructor|length,item,namedItem,constructor""#,
        )],
    },
    Case {
        name: "m437",
        html: PAGE,
        queries: &[(
            r"E(() => ['children', 'append', 'prepend', 'replaceChildren', 'before', 'after', 'replaceWith', 'remove', 'getElementById', 'attributes', 'classList', 'getAttribute', 'setAttribute', 'removeAttribute', 'hasAttribute', 'toggleAttribute', 'getAttributeNames', 'hasAttributes', 'id', 'className'].map((n) => n + ':' + (n in Element.prototype)).join())",
            r#""children:true,append:true,prepend:true,replaceChildren:true,before:true,after:true,replaceWith:true,remove:true,getElementById:false,attributes:true,classList:true,getAttribute:true,setAttribute:true,removeAttribute:true,hasAttribute:true,toggleAttribute:true,getAttributeNames:true,hasAttributes:true,id:true,className:true""#,
        )],
    },
    Case {
        name: "m438",
        html: PAGE,
        queries: &[(
            r"E(() => ['createElement', 'createElementNS', 'createTextNode', 'createComment', 'createDocumentFragment', 'importNode', 'adoptNode', 'getElementById', 'createAttribute', 'referrer', 'defaultView', 'body', 'head', 'title', 'documentElement'].map((n) => n + ':' + (n in Document.prototype)).join())",
            r#""createElement:true,createElementNS:true,createTextNode:true,createComment:true,createDocumentFragment:true,importNode:true,adoptNode:true,getElementById:true,createAttribute:true,referrer:true,defaultView:true,body:true,head:true,title:true,documentElement:true""#,
        )],
    },
    Case {
        name: "m439",
        html: PAGE,
        queries: &[(
            r"E(() => ['appendChild', 'insertBefore', 'removeChild', 'replaceChild', 'cloneNode', 'normalize', 'textContent', 'nodeValue', 'isEqualNode', 'compareDocumentPosition', 'lookupPrefix', 'lookupNamespaceURI', 'isDefaultNamespace'].map((n) => n + ':' + (n in Node.prototype)).join())",
            r#""appendChild:true,insertBefore:true,removeChild:true,replaceChild:true,cloneNode:true,normalize:true,textContent:true,nodeValue:true,isEqualNode:true,compareDocumentPosition:true,lookupPrefix:true,lookupNamespaceURI:true,isDefaultNamespace:true""#,
        )],
    },
    Case {
        name: "m440",
        html: PAGE,
        queries: &[(
            r"E(() => [Node.prototype.insertBefore.length, Node.prototype.appendChild.length, Node.prototype.replaceChild.length, Node.prototype.removeChild.length, Node.prototype.cloneNode.length, Element.prototype.append.length, Element.prototype.setAttribute.length, Element.prototype.toggleAttribute.length, DOMTokenList.prototype.add.length, DOMTokenList.prototype.toggle.length, Document.prototype.createElement.length, Document.prototype.createElementNS.length, CharacterData.prototype.replaceData.length].join())",
            r#""2,1,2,1,0,0,2,1,0,1,1,2,3""#,
        )],
    },
    Case {
        name: "m441",
        html: PAGE,
        queries: &[(
            r"E(() => [Element.prototype.append.name, Element.prototype.before.name, DOMTokenList.prototype.add.name].join())",
            r#""append,before,add""#,
        )],
    },
    Case {
        name: "m442",
        html: PAGE,
        queries: &[(
            r"E(() => { const d = Object.getOwnPropertyDescriptor(Element.prototype, 'classList'); const i = Object.getOwnPropertyDescriptor(Element.prototype, 'id'); const c = Object.getOwnPropertyDescriptor(Element.prototype, 'className'); return [typeof d.get, typeof d.set, typeof i.get, typeof i.set, typeof c.get, typeof c.set, d.enumerable, d.configurable].join(); })",
            r#""function,function,function,function,function,function,true,true""#,
        )],
    },
    Case {
        name: "m443",
        html: PAGE,
        queries: &[(
            r"E(() => { const d = Object.getOwnPropertyDescriptor(Node.prototype, 'textContent'); const v = Object.getOwnPropertyDescriptor(Node.prototype, 'nodeValue'); return [typeof d.get, typeof d.set, d.enumerable, d.configurable, typeof v.set].join(); })",
            r#""function,function,true,true,function""#,
        )],
    },
    Case {
        name: "m444",
        html: PAGE,
        queries: &[(
            r"E(() => { const d = Object.getOwnPropertyDescriptor(Document.prototype, 'referrer'); const w = Object.getOwnPropertyDescriptor(Document.prototype, 'defaultView'); return [typeof d.get, typeof d.set, typeof w.get, typeof w.set].join(); })",
            r#""function,undefined,function,undefined""#,
        )],
    },
    Case {
        name: "m445",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('div', {is: 'x-y'}); return [e.attributes.length, e.getAttribute('is'), e.outerHTML].join(); })",
            r#""0,,<div></div>""#,
        )],
    },
    Case {
        name: "m446",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('div', {is: null}); return [e.attributes.length, e.getAttribute('is'), e.outerHTML].join(); })",
            r#""0,,<div></div>""#,
        )],
    },
    Case {
        name: "m447",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('div', {}); return [e.attributes.length, e.outerHTML].join(); })",
            r#""0,<div></div>""#,
        )],
    },
    Case {
        name: "m448",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElementNS('http://www.w3.org/1999/xhtml', 'div', {is: 'x-y'}); return [e.attributes.length, e.getAttribute('is'), e.outerHTML].join(); })",
            r#""0,,<div></div>""#,
        )],
    },
    Case {
        name: "m449",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElementNS('http://www.w3.org/2000/svg', 'g', {is: 'x-y'}); return [e.attributes.length, e.hasAttribute('is')].join(); })",
            r#""0,false""#,
        )],
    },
    Case {
        name: "m450",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('div', 'x-y'); return [e.attributes.length, e.outerHTML].join(); })",
            r#""0,<div></div>""#,
        )],
    },
    Case {
        name: "m451",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('i'); e.setAttributeNS('urn:q', 'q:c', 'y'); e.setAttribute('Q:C', 'z'); return [e.attributes.length, Array.from(e.attributes).map((a) => a.name + '=' + a.value + '@' + a.namespaceURI).join()].join('|'); })",
            r#""2|q:c=y@urn:q,q:c=z@null""#,
        )],
    },
    Case {
        name: "m452",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('div'); e.append('a'); const t = e.firstChild; e.textContent = 'q'; return [t === e.firstChild, e.childNodes.length, e.firstChild && e.firstChild.data ].join(); })",
            r#""false,1,q""#,
        )],
    },
    Case {
        name: "m453",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('div'); e.append('a', document.createElement('b')); const t = e.firstChild; e.textContent = 'q'; return [t === e.firstChild, e.childNodes.length, e.firstChild && e.firstChild.data ].join(); })",
            r#""false,1,q""#,
        )],
    },
    Case {
        name: "m454",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('div'); e.append('a', 'b'); const t = e.firstChild; e.textContent = 'q'; return [t === e.firstChild, e.childNodes.length, e.firstChild && e.firstChild.data ].join(); })",
            r#""false,1,q""#,
        )],
    },
    Case {
        name: "m455",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('div'); e.append(document.createComment('c')); const t = e.firstChild; e.textContent = 'q'; return [t === e.firstChild, e.childNodes.length, e.firstChild && e.firstChild.data ].join(); })",
            r#""false,1,q""#,
        )],
    },
    Case {
        name: "m456",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('div'); e.append(document.createElement('b'), 'a'); const t = e.firstChild; e.textContent = 'q'; return [t === e.firstChild, e.childNodes.length, e.firstChild && e.firstChild.data ].join(); })",
            r#""false,1,q""#,
        )],
    },
    Case {
        name: "m457",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('div'); e.append('q'); const t = e.firstChild; e.textContent = 'q'; return [t === e.firstChild, e.childNodes.length, e.firstChild && e.firstChild.data ].join(); })",
            r#""true,1,q""#,
        )],
    },
    Case {
        name: "m458",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('div'); e.append('q', document.createElement('b')); const t = e.firstChild; e.textContent = 'q'; return [t === e.firstChild, e.childNodes.length, e.firstChild && e.firstChild.data ].join(); })",
            r#""false,1,q""#,
        )],
    },
    Case {
        name: "m459",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('div'); e.append('q', 'q'); const t = e.firstChild; e.textContent = 'q'; return [t === e.firstChild, e.childNodes.length, e.firstChild && e.firstChild.data ].join(); })",
            r#""false,1,q""#,
        )],
    },
    Case {
        name: "m460",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('div'); e.append(document.createComment('q')); const t = e.firstChild; e.textContent = 'q'; return [t === e.firstChild, e.childNodes.length, e.firstChild && e.firstChild.data ].join(); })",
            r#""false,1,q""#,
        )],
    },
    Case {
        name: "m461",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('div'); e.append(document.createElement('b'), 'q'); const t = e.firstChild; e.textContent = 'q'; return [t === e.firstChild, e.childNodes.length, e.firstChild && e.firstChild.data ].join(); })",
            r#""false,1,q""#,
        )],
    },
    Case {
        name: "m462",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('div'); e.append('q', document.createComment('c')); const t = e.firstChild; e.textContent = 'q'; return [t === e.firstChild, e.childNodes.length, e.firstChild && e.firstChild.data ].join(); })",
            r#""false,1,q""#,
        )],
    },
    Case {
        name: "m463",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('div'); e.append('q'); const t = e.firstChild; e.textContent = 'q'; return [t.data, e.textContent, e.childNodes.length].join(); })",
            r#""q,q,1""#,
        )],
    },
    Case {
        name: "m464",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('div'); e.append('q'); const t = e.firstChild; t.data = 'z'; e.textContent = 'z'; return [t === e.firstChild, e.childNodes.length].join(); })",
            r#""true,1""#,
        )],
    },
    Case {
        name: "m465",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('div'); e.append('a'); const t = e.firstChild; e.textContent = ''; e.textContent = 'q'; return [t === e.firstChild, e.childNodes.length].join(); })",
            r#""false,1""#,
        )],
    },
    Case {
        name: "m466",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('div'); e.append('a'); const t = e.firstChild; e.textContent = ''; return [t.parentNode === null, e.childNodes.length].join(); })",
            r#""true,0""#,
        )],
    },
    Case {
        name: "m467",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('div'); e.append('a'); const t = e.firstChild; e.textContent = 'q'; e.textContent = 'q'; return [t === e.firstChild, t.data].join(); })",
            r#""false,a""#,
        )],
    },
    Case {
        name: "m468",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createDocumentFragment(); e.append('a'); const t = e.firstChild; e.textContent = 'q'; return [t === e.firstChild, e.childNodes.length].join(); })",
            r#""false,1""#,
        )],
    },
    Case {
        name: "m469",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('style'); e.append('a'); const t = e.firstChild; e.textContent = 'q'; return [t === e.firstChild, e.childNodes.length].join(); })",
            r#""false,1""#,
        )],
    },
    Case {
        name: "m470",
        html: PAGE,
        queries: &[(
            r"E(() => { const e = document.createElement('div'); e.append('a'); const t = e.firstChild; e.replaceChildren('q'); return [t === e.firstChild, e.childNodes.length].join(); })",
            r#""false,1""#,
        )],
    },
    Case {
        name: "parse-remove-current",
        html: r"<!DOCTYPE html><html><head><title>t</title></head><body><div id=out></div><script>window.log = []; log.push('a'); document.currentScript.remove(); log.push(String(document.currentScript === null));</script><script>log.push('b')</script><script>document.getElementById('out').textContent = log.join()</script></body></html>",
        queries: &[(
            r"document.getElementById('out').textContent",
            r#""a,false,b""#,
        )],
    },
    Case {
        name: "parse-remove-later-script",
        html: r"<!DOCTYPE html><html><head><title>t</title></head><body><div id=out></div><script>window.log = []; log.push('a'); document.getElementById('later').remove();</script><script id=later>log.push('later')</script><script>log.push('c'); document.getElementById('out').textContent = log.join()</script></body></html>",
        queries: &[
            (
                r"document.getElementById('out').textContent",
                r#""a,later,c""#,
            ),
            (r"log.join()", r#""a,later,c""#),
        ],
    },
    Case {
        name: "parse-remove-parent-of-later-script",
        html: r"<!DOCTYPE html><html><head><title>t</title></head><body><div id=out></div><div id=gone><script>window.log = []; log.push('a'); document.getElementById('gone').remove();</script><script>log.push('later')</script></div><script>log.push('c'); document.getElementById('out').textContent = log.join()</script></body></html>",
        queries: &[
            (r"document.getElementById('out').textContent", r#""a,c""#),
            (r"document.body.childNodes.length", r"2"),
        ],
    },
    Case {
        name: "parse-insert-before-current",
        html: r#"<!DOCTYPE html><html><head><title>t</title></head><body><div id=out></div><script>window.log = []; var s = document.createElement('script'); s.textContent = 'log.push("inserted")'; document.currentScript.parentNode.insertBefore(s, document.currentScript); log.push('after');</script><script>log.push('next'); document.getElementById('out').textContent = log.join()</script></body></html>"#,
        queries: &[(
            r"document.getElementById('out').textContent",
            r#""inserted,after,next""#,
        )],
    },
    Case {
        name: "parse-move-current-script",
        html: r"<!DOCTYPE html><html><head><title>t</title></head><body><div id=out></div><div id=box></div><script>window.log = []; log.push('first'); document.getElementById('box').appendChild(document.currentScript);</script><script>log.push('second'); document.getElementById('out').textContent = log.join() + '|' + document.getElementById('box').children.length</script></body></html>",
        queries: &[(
            r"document.getElementById('out').textContent",
            r#""first,second|1""#,
        )],
    },
    Case {
        name: "parse-append-in-script",
        html: r"<!DOCTYPE html><html><head><title>t</title></head><body><div id=out></div><script>window.log = []; var d = document.createElement('div'); d.id = 'made'; d.className = 'k'; document.body.appendChild(d); log.push(document.getElementById('made') === d, document.body.lastChild === d, document.body.children.length);</script><p id=after>x</p><script>log.push(document.body.children.length); document.getElementById('out').textContent = log.join()</script></body></html>",
        queries: &[(
            r"document.getElementById('out').textContent",
            r#""true,true,3,5""#,
        )],
    },
    Case {
        name: "parse-textcontent-script",
        html: r#"<!DOCTYPE html><html><head><title>t</title></head><body><div id=out></div><script>window.log = []; var s = document.createElement('script'); document.body.appendChild(s); s.textContent = 'log.push("ran")'; s.textContent = 'log.push("again")'; log.push('done'); </script><script>document.getElementById('out').textContent = log.join()</script></body></html>"#,
        queries: &[(
            r"document.getElementById('out').textContent",
            r#""ran,done""#,
        )],
    },
    Case {
        name: "parse-clone-script",
        html: r#"<!DOCTYPE html><html><head><title>t</title></head><body><div id=out></div><script id=orig>window.log = window.log || []; log.push('orig');</script><script>var c = document.getElementById('orig').cloneNode(true); document.body.appendChild(c); log.push('cloned'); var n = document.createElement('script'); n.textContent = 'log.push("fresh")'; var c2 = n.cloneNode(true); document.body.appendChild(n); document.body.appendChild(c2); document.getElementById('out').textContent = log.join()</script></body></html>"#,
        queries: &[(
            r"document.getElementById('out').textContent",
            r#""orig,cloned,fresh,fresh""#,
        )],
    },
];
