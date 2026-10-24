// The cases of `tools/probes/web-html-elements.json` with the values that
// Chromium 148 gave (`just probe tools/probes/web-html-elements.json`, three
// runs, all equal); where swb departs on purpose, the test overrides the value.
// Generated.

use crate::common::Case;

/// The page of the cases.
pub(crate) const PAGE: &str = r"<!DOCTYPE html><html><head><base href='http://x.test/d/e.html'><title>t</title></head><body><div id=a class='k1 k2'><p id=p class=k1>x</p><span id=s name=nm>y</span></div><script>
function E(f) { try { const v = f(); return v === undefined ? 'undefined' : (typeof v === 'object' && v !== null ? String(v) : v); } catch (e) { return (e.constructor === DOMException ? 'DOMException ' + e.name : e.constructor.name) + ': ' + e.message; } }
function S(v) { return typeof v === 'string' ? JSON.stringify(v) : (v === undefined ? 'undefined' : (typeof v === 'number' && v !== v ? 'NaN' : (Object.is(v, -0) ? '-0' : String(v)))); }
function K(h) { const d = document.createElement('div'); d.innerHTML = h; document.body.appendChild(d); return d; }
function ids(l) { return Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); }
function C(o) { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r.join(' '); }
const VALUES = {
  str: ['', 'x', ' Mixed Case ', 'a b'],
  enum: [''],
  bool: ['', 'x', 'false', 'true', 'FALSE'],
  url: ['', 'x', ' x ', '/p?q#f', '//h.test/p', 'http://a.test/c d', 'javascript:void(0)', 'mailto:a@b.test', '#f', '?q', 'x\ty', 'http://[bad', 'ftp://u:p@h.test:21/p'],
  long: ['5', '-5', '+5', '5x', ' 5', '', 'x', '2147483647', '2147483648', '-2147483648', '-2147483649', '1e3', '0', '00', '5.9', '-0'],
};
const WRITES = {
  str: ['', 'x', null, undefined, 5, true, {toString() { return 'obj'; }}],
  enum: ['', 'x', null, 'X'],
  bool: [true, false, 0, 1, '', 'x', null, undefined, NaN, {}],
  url: ['', 'x', '/p', null, 5],
  long: [0, 1, -1, 5.9, -5.9, 2147483647, 2147483648, -2147483648, -2147483649, 4294967295, 4294967296, 1e20, NaN, Infinity, '7', 'x', null, undefined],
};
// kind: str, enum (the keywords in `kw`), bool, url, long (signed), ulong, nonneg.
function P(tag, prop, attr, kind, kw) {
  const out = [];
  const make = () => document.createElement(tag);
  let e = make();
  out.push('d=' + S(e[prop]));
  let values = VALUES[kind] || VALUES.long;
  if (kind === 'enum' || kind === 'enumnull') {
    values = VALUES.enum.concat(kw, kw.map((k) => k.toUpperCase()), kw.map((k) => ' ' + k), ['junk']);
  }
  for (const v of values) { e = make(); e.setAttribute(attr, v); out.push(S(v) + '>' + S(e[prop])); }
  const k = kind === 'enumnull' ? 'enum' : (kind === 'ulong' || kind === 'nonneg' ? 'long' : kind);
  let writes = WRITES[k];
  if (kind === 'enum' || kind === 'enumnull') writes = WRITES.enum.concat(kw.slice(0, 3));
  for (const v of writes) {
    e = make();
    try { e[prop] = v; out.push(S(v) + '<' + S(e.getAttribute(attr))); } catch (x) { out.push(S(v) + '<!' + x.constructor.name + ':' + x.message); }
  }
  return out.join(' | ');
}
</script></body></html>";

pub(crate) const CASES: &[Case] = &[
    Case {
        name: "reflect-element-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("abbr", "title", "title", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("abbr", "lang", "lang", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("abbr", "dir", "dir", "enum", ["ltr", "rtl", "auto"]))"#,
                r#""d=\"\" | \"\">\"\" | \"ltr\">\"ltr\" | \"rtl\">\"rtl\" | \"auto\">\"auto\" | \"LTR\">\"ltr\" | \"RTL\">\"rtl\" | \"AUTO\">\"auto\" | \" ltr\">\"\" | \" rtl\">\"\" | \" auto\">\"\" | \"junk\">\"\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"ltr\"<\"ltr\" | \"rtl\"<\"rtl\" | \"auto\"<\"auto\"""#,
            ),
            (
                r#"E(() => P("abbr", "inert", "inert", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("abbr", "accessKey", "accesskey", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("abbr", "autocapitalize", "autocapitalize", "enum", ["none", "off", "on", "sentences", "words", "characters"]))"#,
                r#""d=\"\" | \"\">\"\" | \"none\">\"none\" | \"off\">\"none\" | \"on\">\"sentences\" | \"sentences\">\"sentences\" | \"words\">\"words\" | \"characters\">\"characters\" | \"NONE\">\"none\" | \"OFF\">\"none\" | \"ON\">\"sentences\" | \"SENTENCES\">\"sentences\" | \"WORDS\">\"words\" | \"CHARACTERS\">\"characters\" | \" none\">\"sentences\" | \" off\">\"sentences\" | \" on\">\"sentences\" | \" sentences\">\"sentences\" | \" words\">\"sentences\" | \" characters\">\"sentences\" | \"junk\">\"sentences\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"none\"<\"none\" | \"off\"<\"off\" | \"on\"<\"on\"""#,
            ),
            (
                r#"E(() => P("abbr", "inputMode", "inputmode", "enum", ["none", "text", "tel", "url", "email", "numeric", "decimal", "search"]))"#,
                r#""d=\"\" | \"\">\"\" | \"none\">\"none\" | \"text\">\"text\" | \"tel\">\"tel\" | \"url\">\"url\" | \"email\">\"email\" | \"numeric\">\"numeric\" | \"decimal\">\"decimal\" | \"search\">\"search\" | \"NONE\">\"none\" | \"TEXT\">\"text\" | \"TEL\">\"tel\" | \"URL\">\"url\" | \"EMAIL\">\"email\" | \"NUMERIC\">\"numeric\" | \"DECIMAL\">\"decimal\" | \"SEARCH\">\"search\" | \" none\">\"\" | \" text\">\"\" | \" tel\">\"\" | \" url\">\"\" | \" email\">\"\" | \" numeric\">\"\" | \" decimal\">\"\" | \" search\">\"\" | \"junk\">\"\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"none\"<\"none\" | \"text\"<\"text\" | \"tel\"<\"tel\"""#,
            ),
            (
                r#"E(() => P("abbr", "enterKeyHint", "enterkeyhint", "enum", ["enter", "done", "go", "next", "previous", "search", "send"]))"#,
                r#""d=\"\" | \"\">\"\" | \"enter\">\"enter\" | \"done\">\"done\" | \"go\">\"go\" | \"next\">\"next\" | \"previous\">\"previous\" | \"search\">\"search\" | \"send\">\"send\" | \"ENTER\">\"enter\" | \"DONE\">\"done\" | \"GO\">\"go\" | \"NEXT\">\"next\" | \"PREVIOUS\">\"previous\" | \"SEARCH\">\"search\" | \"SEND\">\"send\" | \" enter\">\"\" | \" done\">\"\" | \" go\">\"\" | \" next\">\"\" | \" previous\">\"\" | \" search\">\"\" | \" send\">\"\" | \"junk\">\"\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"enter\"<\"enter\" | \"done\"<\"done\" | \"go\"<\"go\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-base-01",
        html: PAGE,
        queries: &[(
            r#"E(() => P("base", "target", "target", "str", []))"#,
            r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
        )],
    },
    Case {
        name: "reflect-link-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("link", "href", "href", "url", []))"#,
                r##""d=\"\" | \"\">\"http://x.test/d/e.html\" | \"x\">\"http://x.test/d/x\" | \" x \">\"http://x.test/d/x\" | \"/p?q#f\">\"http://x.test/p?q#f\" | \"//h.test/p\">\"http://h.test/p\" | \"http://a.test/c d\">\"http://a.test/c%20d\" | \"javascript:void(0)\">\"javascript:void(0)\" | \"mailto:a@b.test\">\"mailto:a@b.test\" | \"#f\">\"http://x.test/d/e.html#f\" | \"?q\">\"http://x.test/d/e.html?q\" | \"x\\ty\">\"http://x.test/d/xy\" | \"http://[bad\">\"http://[bad\" | \"ftp://u:p@h.test:21/p\">\"ftp://u:p@h.test/p\" | \"\"<\"\" | \"x\"<\"x\" | \"/p\"<\"/p\" | null<\"null\" | 5<\"5\"""##,
            ),
            (
                r#"E(() => P("link", "crossOrigin", "crossorigin", "enumnull", ["anonymous", "use-credentials"]))"#,
                r#""d=null | \"\">\"anonymous\" | \"anonymous\">\"anonymous\" | \"use-credentials\">\"use-credentials\" | \"ANONYMOUS\">\"anonymous\" | \"USE-CREDENTIALS\">\"use-credentials\" | \" anonymous\">\"anonymous\" | \" use-credentials\">\"anonymous\" | \"junk\">\"anonymous\" | \"\"<\"\" | \"x\"<\"x\" | null<null | \"X\"<\"X\" | \"anonymous\"<\"anonymous\" | \"use-credentials\"<\"use-credentials\"""#,
            ),
            (
                r#"E(() => P("link", "rel", "rel", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("link", "as", "as", "enum", ["fetch", "font", "image", "script", "style", "track"]))"#,
                r#""d=\"\" | \"\">\"\" | \"fetch\">\"fetch\" | \"font\">\"font\" | \"image\">\"image\" | \"script\">\"script\" | \"style\">\"style\" | \"track\">\"track\" | \"FETCH\">\"fetch\" | \"FONT\">\"font\" | \"IMAGE\">\"image\" | \"SCRIPT\">\"script\" | \"STYLE\">\"style\" | \"TRACK\">\"track\" | \" fetch\">\"\" | \" font\">\"\" | \" image\">\"\" | \" script\">\"\" | \" style\">\"\" | \" track\">\"\" | \"junk\">\"\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"fetch\"<\"fetch\" | \"font\"<\"font\" | \"image\"<\"image\"""#,
            ),
            (
                r#"E(() => P("link", "media", "media", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("link", "integrity", "integrity", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("link", "hreflang", "hreflang", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("link", "type", "type", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("link", "imageSrcset", "imagesrcset", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("link", "imageSizes", "imagesizes", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("link", "referrerPolicy", "referrerpolicy", "enum", ["no-referrer", "no-referrer-when-downgrade", "same-origin", "origin", "strict-origin", "origin-when-cross-origin", "strict-origin-when-cross-origin", "unsafe-url"]))"#,
                r#""d=\"\" | \"\">\"\" | \"no-referrer\">\"no-referrer\" | \"no-referrer-when-downgrade\">\"no-referrer-when-downgrade\" | \"same-origin\">\"same-origin\" | \"origin\">\"origin\" | \"strict-origin\">\"strict-origin\" | \"origin-when-cross-origin\">\"origin-when-cross-origin\" | \"strict-origin-when-cross-origin\">\"strict-origin-when-cross-origin\" | \"unsafe-url\">\"unsafe-url\" | \"NO-REFERRER\">\"no-referrer\" | \"NO-REFERRER-WHEN-DOWNGRADE\">\"no-referrer-when-downgrade\" | \"SAME-ORIGIN\">\"same-origin\" | \"ORIGIN\">\"origin\" | \"STRICT-ORIGIN\">\"strict-origin\" | \"ORIGIN-WHEN-CROSS-ORIGIN\">\"origin-when-cross-origin\" | \"STRICT-ORIGIN-WHEN-CROSS-ORIGIN\">\"strict-origin-when-cross-origin\" | \"UNSAFE-URL\">\"unsafe-url\" | \" no-referrer\">\"\" | \" no-referrer-when-downgrade\">\"\" | \" same-origin\">\"\" | \" origin\">\"\" | \" strict-origin\">\"\" | \" origin-when-cross-origin\">\"\" | \" strict-origin-when-cross-origin\">\"\" | \" unsafe-url\">\"\" | \"junk\">\"\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"no-referrer\"<\"no-referrer\" | \"no-referrer-when-downgrade\"<\"no-referrer-when-downgrade\" | \"same-origin\"<\"same-origin\"""#,
            ),
            (
                r#"E(() => P("link", "disabled", "disabled", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-link-02",
        html: PAGE,
        queries: &[(
            r#"E(() => P("link", "fetchPriority", "fetchpriority", "enum", ["high", "low", "auto"]))"#,
            r#""d=\"auto\" | \"\">\"auto\" | \"high\">\"high\" | \"low\">\"low\" | \"auto\">\"auto\" | \"HIGH\">\"high\" | \"LOW\">\"low\" | \"AUTO\">\"auto\" | \" high\">\"auto\" | \" low\">\"auto\" | \" auto\">\"auto\" | \"junk\">\"auto\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"high\"<\"high\" | \"low\"<\"low\" | \"auto\"<\"auto\"""#,
        )],
    },
    Case {
        name: "reflect-meta-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("meta", "name", "name", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("meta", "httpEquiv", "http-equiv", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("meta", "content", "content", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("meta", "media", "media", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-style-01",
        html: PAGE,
        queries: &[(
            r#"E(() => P("style", "media", "media", "str", []))"#,
            r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
        )],
    },
    Case {
        name: "reflect-quote-01",
        html: PAGE,
        queries: &[(
            r#"E(() => P("blockquote", "cite", "cite", "url", []))"#,
            r##""d=\"\" | \"\">\"http://x.test/d/e.html\" | \"x\">\"http://x.test/d/x\" | \" x \">\"http://x.test/d/x\" | \"/p?q#f\">\"http://x.test/p?q#f\" | \"//h.test/p\">\"http://h.test/p\" | \"http://a.test/c d\">\"http://a.test/c%20d\" | \"javascript:void(0)\">\"javascript:void(0)\" | \"mailto:a@b.test\">\"mailto:a@b.test\" | \"#f\">\"http://x.test/d/e.html#f\" | \"?q\">\"http://x.test/d/e.html?q\" | \"x\\ty\">\"http://x.test/d/xy\" | \"http://[bad\">\"http://[bad\" | \"ftp://u:p@h.test:21/p\">\"ftp://u:p@h.test/p\" | \"\"<\"\" | \"x\"<\"x\" | \"/p\"<\"/p\" | null<\"null\" | 5<\"5\"""##,
        )],
    },
    Case {
        name: "reflect-olist-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("ol", "reversed", "reversed", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("ol", "start", "start", "long", []))"#,
                r#""d=1 | \"5\">5 | \"-5\">-5 | \"+5\">5 | \"5x\">5 | \" 5\">5 | \"\">1 | \"x\">1 | \"2147483647\">2147483647 | \"2147483648\">1 | \"-2147483648\">-2147483648 | \"-2147483649\">1 | \"1e3\">1 | \"0\">0 | \"00\">0 | \"5.9\">5 | \"-0\">0 | 0<\"0\" | 1<\"1\" | -1<\"-1\" | 5.9<\"5\" | -5.9<\"-5\" | 2147483647<\"2147483647\" | 2147483648<\"-2147483648\" | -2147483648<\"-2147483648\" | -2147483649<\"2147483647\" | 4294967295<\"-1\" | 4294967296<\"0\" | 100000000000000000000<\"1661992960\" | NaN<\"0\" | Infinity<\"0\" | \"7\"<\"7\" | \"x\"<\"0\" | null<\"0\" | undefined<\"0\"""#,
            ),
            (
                r#"E(() => P("ol", "type", "type", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-li-01",
        html: PAGE,
        queries: &[(
            r#"E(() => P("li", "value", "value", "long", []))"#,
            r#""d=0 | \"5\">5 | \"-5\">-5 | \"+5\">5 | \"5x\">5 | \" 5\">5 | \"\">0 | \"x\">0 | \"2147483647\">2147483647 | \"2147483648\">0 | \"-2147483648\">-2147483648 | \"-2147483649\">0 | \"1e3\">1 | \"0\">0 | \"00\">0 | \"5.9\">5 | \"-0\">0 | 0<\"0\" | 1<\"1\" | -1<\"-1\" | 5.9<\"5\" | -5.9<\"-5\" | 2147483647<\"2147483647\" | 2147483648<\"-2147483648\" | -2147483648<\"-2147483648\" | -2147483649<\"2147483647\" | 4294967295<\"-1\" | 4294967296<\"0\" | 100000000000000000000<\"1661992960\" | NaN<\"0\" | Infinity<\"0\" | \"7\"<\"7\" | \"x\"<\"0\" | null<\"0\" | undefined<\"0\"""#,
        )],
    },
    Case {
        name: "reflect-anchor-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("a", "download", "download", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("a", "ping", "ping", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("a", "rel", "rel", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("a", "hreflang", "hreflang", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("a", "type", "type", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("a", "referrerPolicy", "referrerpolicy", "enum", ["no-referrer", "no-referrer-when-downgrade", "same-origin", "origin", "strict-origin", "origin-when-cross-origin", "strict-origin-when-cross-origin", "unsafe-url"]))"#,
                r#""d=\"\" | \"\">\"\" | \"no-referrer\">\"no-referrer\" | \"no-referrer-when-downgrade\">\"no-referrer-when-downgrade\" | \"same-origin\">\"same-origin\" | \"origin\">\"origin\" | \"strict-origin\">\"strict-origin\" | \"origin-when-cross-origin\">\"origin-when-cross-origin\" | \"strict-origin-when-cross-origin\">\"strict-origin-when-cross-origin\" | \"unsafe-url\">\"unsafe-url\" | \"NO-REFERRER\">\"no-referrer\" | \"NO-REFERRER-WHEN-DOWNGRADE\">\"no-referrer-when-downgrade\" | \"SAME-ORIGIN\">\"same-origin\" | \"ORIGIN\">\"origin\" | \"STRICT-ORIGIN\">\"strict-origin\" | \"ORIGIN-WHEN-CROSS-ORIGIN\">\"origin-when-cross-origin\" | \"STRICT-ORIGIN-WHEN-CROSS-ORIGIN\">\"strict-origin-when-cross-origin\" | \"UNSAFE-URL\">\"unsafe-url\" | \" no-referrer\">\"\" | \" no-referrer-when-downgrade\">\"\" | \" same-origin\">\"\" | \" origin\">\"\" | \" strict-origin\">\"\" | \" origin-when-cross-origin\">\"\" | \" strict-origin-when-cross-origin\">\"\" | \" unsafe-url\">\"\" | \"junk\">\"\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"no-referrer\"<\"no-referrer\" | \"no-referrer-when-downgrade\"<\"no-referrer-when-downgrade\" | \"same-origin\"<\"same-origin\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-data-01",
        html: PAGE,
        queries: &[(
            r#"E(() => P("data", "value", "value", "str", []))"#,
            r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
        )],
    },
    Case {
        name: "reflect-time-01",
        html: PAGE,
        queries: &[(
            r#"E(() => P("time", "dateTime", "datetime", "str", []))"#,
            r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
        )],
    },
    Case {
        name: "reflect-mod-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("del", "cite", "cite", "url", []))"#,
                r##""d=\"\" | \"\">\"http://x.test/d/e.html\" | \"x\">\"http://x.test/d/x\" | \" x \">\"http://x.test/d/x\" | \"/p?q#f\">\"http://x.test/p?q#f\" | \"//h.test/p\">\"http://h.test/p\" | \"http://a.test/c d\">\"http://a.test/c%20d\" | \"javascript:void(0)\">\"javascript:void(0)\" | \"mailto:a@b.test\">\"mailto:a@b.test\" | \"#f\">\"http://x.test/d/e.html#f\" | \"?q\">\"http://x.test/d/e.html?q\" | \"x\\ty\">\"http://x.test/d/xy\" | \"http://[bad\">\"http://[bad\" | \"ftp://u:p@h.test:21/p\">\"ftp://u:p@h.test/p\" | \"\"<\"\" | \"x\"<\"x\" | \"/p\"<\"/p\" | null<\"null\" | 5<\"5\"""##,
            ),
            (
                r#"E(() => P("del", "dateTime", "datetime", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-source-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("source", "src", "src", "url", []))"#,
                r##""d=\"\" | \"\">\"http://x.test/d/e.html\" | \"x\">\"http://x.test/d/x\" | \" x \">\"http://x.test/d/x\" | \"/p?q#f\">\"http://x.test/p?q#f\" | \"//h.test/p\">\"http://h.test/p\" | \"http://a.test/c d\">\"http://a.test/c%20d\" | \"javascript:void(0)\">\"javascript:void(0)\" | \"mailto:a@b.test\">\"mailto:a@b.test\" | \"#f\">\"http://x.test/d/e.html#f\" | \"?q\">\"http://x.test/d/e.html?q\" | \"x\\ty\">\"http://x.test/d/xy\" | \"http://[bad\">\"http://[bad\" | \"ftp://u:p@h.test:21/p\">\"ftp://u:p@h.test/p\" | \"\"<\"\" | \"x\"<\"x\" | \"/p\"<\"/p\" | null<\"null\" | 5<\"5\"""##,
            ),
            (
                r#"E(() => P("source", "type", "type", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("source", "srcset", "srcset", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("source", "sizes", "sizes", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("source", "media", "media", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("source", "width", "width", "ulong", []))"#,
                r#""d=0 | \"5\">5 | \"-5\">0 | \"+5\">5 | \"5x\">5 | \" 5\">5 | \"\">0 | \"x\">0 | \"2147483647\">2147483647 | \"2147483648\">0 | \"-2147483648\">0 | \"-2147483649\">0 | \"1e3\">1 | \"0\">0 | \"00\">0 | \"5.9\">5 | \"-0\">0 | 0<\"0\" | 1<\"1\" | -1<\"0\" | 5.9<\"5\" | -5.9<\"0\" | 2147483647<\"2147483647\" | 2147483648<\"0\" | -2147483648<\"0\" | -2147483649<\"2147483647\" | 4294967295<\"0\" | 4294967296<\"0\" | 100000000000000000000<\"1661992960\" | NaN<\"0\" | Infinity<\"0\" | \"7\"<\"7\" | \"x\"<\"0\" | null<\"0\" | undefined<\"0\"""#,
            ),
            (
                r#"E(() => P("source", "height", "height", "ulong", []))"#,
                r#""d=0 | \"5\">5 | \"-5\">0 | \"+5\">5 | \"5x\">5 | \" 5\">5 | \"\">0 | \"x\">0 | \"2147483647\">2147483647 | \"2147483648\">0 | \"-2147483648\">0 | \"-2147483649\">0 | \"1e3\">1 | \"0\">0 | \"00\">0 | \"5.9\">5 | \"-0\">0 | 0<\"0\" | 1<\"1\" | -1<\"0\" | 5.9<\"5\" | -5.9<\"0\" | 2147483647<\"2147483647\" | 2147483648<\"0\" | -2147483648<\"0\" | -2147483649<\"2147483647\" | 4294967295<\"0\" | 4294967296<\"0\" | 100000000000000000000<\"1661992960\" | NaN<\"0\" | Infinity<\"0\" | \"7\"<\"7\" | \"x\"<\"0\" | null<\"0\" | undefined<\"0\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-image-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("img", "alt", "alt", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("img", "src", "src", "url", []))"#,
                r##""d=\"\" | \"\">\"http://x.test/d/e.html\" | \"x\">\"http://x.test/d/x\" | \" x \">\"http://x.test/d/x\" | \"/p?q#f\">\"http://x.test/p?q#f\" | \"//h.test/p\">\"http://h.test/p\" | \"http://a.test/c d\">\"http://a.test/c%20d\" | \"javascript:void(0)\">\"javascript:void(0)\" | \"mailto:a@b.test\">\"mailto:a@b.test\" | \"#f\">\"http://x.test/d/e.html#f\" | \"?q\">\"http://x.test/d/e.html?q\" | \"x\\ty\">\"http://x.test/d/xy\" | \"http://[bad\">\"http://[bad\" | \"ftp://u:p@h.test:21/p\">\"ftp://u:p@h.test/p\" | \"\"<\"\" | \"x\"<\"x\" | \"/p\"<\"/p\" | null<\"null\" | 5<\"5\"""##,
            ),
            (
                r#"E(() => P("img", "srcset", "srcset", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("img", "sizes", "sizes", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("img", "crossOrigin", "crossorigin", "enumnull", ["anonymous", "use-credentials"]))"#,
                r#""d=null | \"\">\"anonymous\" | \"anonymous\">\"anonymous\" | \"use-credentials\">\"use-credentials\" | \"ANONYMOUS\">\"anonymous\" | \"USE-CREDENTIALS\">\"use-credentials\" | \" anonymous\">\"anonymous\" | \" use-credentials\">\"anonymous\" | \"junk\">\"anonymous\" | \"\"<\"\" | \"x\"<\"x\" | null<null | \"X\"<\"X\" | \"anonymous\"<\"anonymous\" | \"use-credentials\"<\"use-credentials\"""#,
            ),
            (
                r#"E(() => P("img", "useMap", "usemap", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("img", "isMap", "ismap", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("img", "width", "width", "ulong", []))"#,
                r#""d=0 | \"5\">5 | \"-5\">0 | \"+5\">5 | \"5x\">5 | \" 5\">5 | \"\">0 | \"x\">0 | \"2147483647\">2147483647 | \"2147483648\">2147483648 | \"-2147483648\">0 | \"-2147483649\">0 | \"1e3\">1 | \"0\">0 | \"00\">0 | \"5.9\">5 | \"-0\">0 | 0<\"0\" | 1<\"1\" | -1<\"0\" | 5.9<\"5\" | -5.9<\"0\" | 2147483647<\"2147483647\" | 2147483648<\"0\" | -2147483648<\"0\" | -2147483649<\"2147483647\" | 4294967295<\"0\" | 4294967296<\"0\" | 100000000000000000000<\"1661992960\" | NaN<\"0\" | Infinity<\"0\" | \"7\"<\"7\" | \"x\"<\"0\" | null<\"0\" | undefined<\"0\"""#,
            ),
            (
                r#"E(() => P("img", "height", "height", "ulong", []))"#,
                r#""d=0 | \"5\">5 | \"-5\">0 | \"+5\">5 | \"5x\">5 | \" 5\">5 | \"\">0 | \"x\">0 | \"2147483647\">2147483647 | \"2147483648\">2147483648 | \"-2147483648\">0 | \"-2147483649\">0 | \"1e3\">1 | \"0\">0 | \"00\">0 | \"5.9\">5 | \"-0\">0 | 0<\"0\" | 1<\"1\" | -1<\"0\" | 5.9<\"5\" | -5.9<\"0\" | 2147483647<\"2147483647\" | 2147483648<\"0\" | -2147483648<\"0\" | -2147483649<\"2147483647\" | 4294967295<\"0\" | 4294967296<\"0\" | 100000000000000000000<\"1661992960\" | NaN<\"0\" | Infinity<\"0\" | \"7\"<\"7\" | \"x\"<\"0\" | null<\"0\" | undefined<\"0\"""#,
            ),
            (
                r#"E(() => P("img", "referrerPolicy", "referrerpolicy", "enum", ["no-referrer", "no-referrer-when-downgrade", "same-origin", "origin", "strict-origin", "origin-when-cross-origin", "strict-origin-when-cross-origin", "unsafe-url"]))"#,
                r#""d=\"\" | \"\">\"\" | \"no-referrer\">\"no-referrer\" | \"no-referrer-when-downgrade\">\"no-referrer-when-downgrade\" | \"same-origin\">\"same-origin\" | \"origin\">\"origin\" | \"strict-origin\">\"strict-origin\" | \"origin-when-cross-origin\">\"origin-when-cross-origin\" | \"strict-origin-when-cross-origin\">\"strict-origin-when-cross-origin\" | \"unsafe-url\">\"unsafe-url\" | \"NO-REFERRER\">\"no-referrer\" | \"NO-REFERRER-WHEN-DOWNGRADE\">\"no-referrer-when-downgrade\" | \"SAME-ORIGIN\">\"same-origin\" | \"ORIGIN\">\"origin\" | \"STRICT-ORIGIN\">\"strict-origin\" | \"ORIGIN-WHEN-CROSS-ORIGIN\">\"origin-when-cross-origin\" | \"STRICT-ORIGIN-WHEN-CROSS-ORIGIN\">\"strict-origin-when-cross-origin\" | \"UNSAFE-URL\">\"unsafe-url\" | \" no-referrer\">\"\" | \" no-referrer-when-downgrade\">\"\" | \" same-origin\">\"\" | \" origin\">\"\" | \" strict-origin\">\"\" | \" origin-when-cross-origin\">\"\" | \" strict-origin-when-cross-origin\">\"\" | \" unsafe-url\">\"\" | \"junk\">\"\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"no-referrer\"<\"no-referrer\" | \"no-referrer-when-downgrade\"<\"no-referrer-when-downgrade\" | \"same-origin\"<\"same-origin\"""#,
            ),
            (
                r#"E(() => P("img", "decoding", "decoding", "enum", ["sync", "async", "auto"]))"#,
                r#""d=\"auto\" | \"\">\"auto\" | \"sync\">\"sync\" | \"async\">\"async\" | \"auto\">\"auto\" | \"SYNC\">\"sync\" | \"ASYNC\">\"async\" | \"AUTO\">\"auto\" | \" sync\">\"auto\" | \" async\">\"auto\" | \" auto\">\"auto\" | \"junk\">\"auto\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"sync\"<\"sync\" | \"async\"<\"async\" | \"auto\"<\"auto\"""#,
            ),
            (
                r#"E(() => P("img", "loading", "loading", "enum", ["lazy", "eager"]))"#,
                r#""d=\"auto\" | \"\">\"auto\" | \"lazy\">\"lazy\" | \"eager\">\"eager\" | \"LAZY\">\"lazy\" | \"EAGER\">\"eager\" | \" lazy\">\"auto\" | \" eager\">\"auto\" | \"junk\">\"auto\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"lazy\"<\"lazy\" | \"eager\"<\"eager\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-image-02",
        html: PAGE,
        queries: &[(
            r#"E(() => P("img", "fetchPriority", "fetchpriority", "enum", ["high", "low", "auto"]))"#,
            r#""d=\"auto\" | \"\">\"auto\" | \"high\">\"high\" | \"low\">\"low\" | \"auto\">\"auto\" | \"HIGH\">\"high\" | \"LOW\">\"low\" | \"AUTO\">\"auto\" | \" high\">\"auto\" | \" low\">\"auto\" | \" auto\">\"auto\" | \"junk\">\"auto\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"high\"<\"high\" | \"low\"<\"low\" | \"auto\"<\"auto\"""#,
        )],
    },
    Case {
        name: "reflect-iframe-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("iframe", "src", "src", "url", []))"#,
                r##""d=\"\" | \"\">\"http://x.test/d/e.html\" | \"x\">\"http://x.test/d/x\" | \" x \">\"http://x.test/d/x\" | \"/p?q#f\">\"http://x.test/p?q#f\" | \"//h.test/p\">\"http://h.test/p\" | \"http://a.test/c d\">\"http://a.test/c%20d\" | \"javascript:void(0)\">\"javascript:void(0)\" | \"mailto:a@b.test\">\"mailto:a@b.test\" | \"#f\">\"http://x.test/d/e.html#f\" | \"?q\">\"http://x.test/d/e.html?q\" | \"x\\ty\">\"http://x.test/d/xy\" | \"http://[bad\">\"http://[bad\" | \"ftp://u:p@h.test:21/p\">\"ftp://u:p@h.test/p\" | \"\"<\"\" | \"x\"<\"x\" | \"/p\"<\"/p\" | null<\"null\" | 5<\"5\"""##,
            ),
            (
                r#"E(() => P("iframe", "srcdoc", "srcdoc", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("iframe", "name", "name", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("iframe", "allow", "allow", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("iframe", "allowFullscreen", "allowfullscreen", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("iframe", "width", "width", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("iframe", "height", "height", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("iframe", "referrerPolicy", "referrerpolicy", "enum", ["no-referrer", "no-referrer-when-downgrade", "same-origin", "origin", "strict-origin", "origin-when-cross-origin", "strict-origin-when-cross-origin", "unsafe-url"]))"#,
                r#""d=\"\" | \"\">\"\" | \"no-referrer\">\"no-referrer\" | \"no-referrer-when-downgrade\">\"no-referrer-when-downgrade\" | \"same-origin\">\"same-origin\" | \"origin\">\"origin\" | \"strict-origin\">\"strict-origin\" | \"origin-when-cross-origin\">\"origin-when-cross-origin\" | \"strict-origin-when-cross-origin\">\"strict-origin-when-cross-origin\" | \"unsafe-url\">\"unsafe-url\" | \"NO-REFERRER\">\"no-referrer\" | \"NO-REFERRER-WHEN-DOWNGRADE\">\"no-referrer-when-downgrade\" | \"SAME-ORIGIN\">\"same-origin\" | \"ORIGIN\">\"origin\" | \"STRICT-ORIGIN\">\"strict-origin\" | \"ORIGIN-WHEN-CROSS-ORIGIN\">\"origin-when-cross-origin\" | \"STRICT-ORIGIN-WHEN-CROSS-ORIGIN\">\"strict-origin-when-cross-origin\" | \"UNSAFE-URL\">\"unsafe-url\" | \" no-referrer\">\"\" | \" no-referrer-when-downgrade\">\"\" | \" same-origin\">\"\" | \" origin\">\"\" | \" strict-origin\">\"\" | \" origin-when-cross-origin\">\"\" | \" strict-origin-when-cross-origin\">\"\" | \" unsafe-url\">\"\" | \"junk\">\"\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"no-referrer\"<\"no-referrer\" | \"no-referrer-when-downgrade\"<\"no-referrer-when-downgrade\" | \"same-origin\"<\"same-origin\"""#,
            ),
            (
                r#"E(() => P("iframe", "loading", "loading", "enum", ["lazy", "eager"]))"#,
                r#""d=\"auto\" | \"\">\"auto\" | \"lazy\">\"lazy\" | \"eager\">\"eager\" | \"LAZY\">\"lazy\" | \"EAGER\">\"eager\" | \" lazy\">\"auto\" | \" eager\">\"auto\" | \"junk\">\"auto\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"lazy\"<\"lazy\" | \"eager\"<\"eager\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-embed-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("embed", "src", "src", "url", []))"#,
                r##""d=\"\" | \"\">\"http://x.test/d/e.html\" | \"x\">\"http://x.test/d/x\" | \" x \">\"http://x.test/d/x\" | \"/p?q#f\">\"http://x.test/p?q#f\" | \"//h.test/p\">\"http://h.test/p\" | \"http://a.test/c d\">\"http://a.test/c%20d\" | \"javascript:void(0)\">\"javascript:void(0)\" | \"mailto:a@b.test\">\"mailto:a@b.test\" | \"#f\">\"http://x.test/d/e.html#f\" | \"?q\">\"http://x.test/d/e.html?q\" | \"x\\ty\">\"http://x.test/d/xy\" | \"http://[bad\">\"http://[bad\" | \"ftp://u:p@h.test:21/p\">\"ftp://u:p@h.test/p\" | \"\"<\"\" | \"x\"<\"x\" | \"/p\"<\"/p\" | null<\"null\" | 5<\"5\"""##,
            ),
            (
                r#"E(() => P("embed", "type", "type", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("embed", "width", "width", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("embed", "height", "height", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-object-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("object", "data", "data", "url", []))"#,
                r##""d=\"\" | \"\">\"http://x.test/d/e.html\" | \"x\">\"http://x.test/d/x\" | \" x \">\"http://x.test/d/x\" | \"/p?q#f\">\"http://x.test/p?q#f\" | \"//h.test/p\">\"http://h.test/p\" | \"http://a.test/c d\">\"http://a.test/c%20d\" | \"javascript:void(0)\">\"javascript:void(0)\" | \"mailto:a@b.test\">\"mailto:a@b.test\" | \"#f\">\"http://x.test/d/e.html#f\" | \"?q\">\"http://x.test/d/e.html?q\" | \"x\\ty\">\"http://x.test/d/xy\" | \"http://[bad\">\"http://[bad\" | \"ftp://u:p@h.test:21/p\">\"ftp://u:p@h.test/p\" | \"\"<\"\" | \"x\"<\"x\" | \"/p\"<\"/p\" | null<\"null\" | 5<\"5\"""##,
            ),
            (
                r#"E(() => P("object", "type", "type", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("object", "name", "name", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("object", "width", "width", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("object", "height", "height", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-media-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("audio", "src", "src", "url", []))"#,
                r##""d=\"\" | \"\">\"http://x.test/d/e.html\" | \"x\">\"http://x.test/d/x\" | \" x \">\"http://x.test/d/x\" | \"/p?q#f\">\"http://x.test/p?q#f\" | \"//h.test/p\">\"http://h.test/p\" | \"http://a.test/c d\">\"http://a.test/c%20d\" | \"javascript:void(0)\">\"javascript:void(0)\" | \"mailto:a@b.test\">\"mailto:a@b.test\" | \"#f\">\"http://x.test/d/e.html#f\" | \"?q\">\"http://x.test/d/e.html?q\" | \"x\\ty\">\"http://x.test/d/xy\" | \"http://[bad\">\"http://[bad\" | \"ftp://u:p@h.test:21/p\">\"ftp://u:p@h.test/p\" | \"\"<\"\" | \"x\"<\"x\" | \"/p\"<\"/p\" | null<\"null\" | 5<\"5\"""##,
            ),
            (
                r#"E(() => P("audio", "preload", "preload", "enum", ["none", "metadata", "auto"]))"#,
                r#""d=\"metadata\" | \"\">\"auto\" | \"none\">\"none\" | \"metadata\">\"metadata\" | \"auto\">\"auto\" | \"NONE\">\"none\" | \"METADATA\">\"metadata\" | \"AUTO\">\"auto\" | \" none\">\"metadata\" | \" metadata\">\"metadata\" | \" auto\">\"metadata\" | \"junk\">\"metadata\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"none\"<\"none\" | \"metadata\"<\"metadata\" | \"auto\"<\"auto\"""#,
            ),
            (
                r#"E(() => P("audio", "crossOrigin", "crossorigin", "enumnull", ["anonymous", "use-credentials"]))"#,
                r#""d=null | \"\">\"anonymous\" | \"anonymous\">\"anonymous\" | \"use-credentials\">\"use-credentials\" | \"ANONYMOUS\">\"anonymous\" | \"USE-CREDENTIALS\">\"use-credentials\" | \" anonymous\">\"anonymous\" | \" use-credentials\">\"anonymous\" | \"junk\">\"anonymous\" | \"\"<\"\" | \"x\"<\"x\" | null<null | \"X\"<\"X\" | \"anonymous\"<\"anonymous\" | \"use-credentials\"<\"use-credentials\"""#,
            ),
            (
                r#"E(() => P("audio", "autoplay", "autoplay", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("audio", "loop", "loop", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("audio", "controls", "controls", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("audio", "defaultMuted", "muted", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-video-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("video", "width", "width", "ulong", []))"#,
                r#""d=0 | \"5\">5 | \"-5\">0 | \"+5\">5 | \"5x\">5 | \" 5\">5 | \"\">0 | \"x\">0 | \"2147483647\">2147483647 | \"2147483648\">0 | \"-2147483648\">0 | \"-2147483649\">0 | \"1e3\">1 | \"0\">0 | \"00\">0 | \"5.9\">5 | \"-0\">0 | 0<\"0\" | 1<\"1\" | -1<\"0\" | 5.9<\"5\" | -5.9<\"0\" | 2147483647<\"2147483647\" | 2147483648<\"0\" | -2147483648<\"0\" | -2147483649<\"2147483647\" | 4294967295<\"0\" | 4294967296<\"0\" | 100000000000000000000<\"1661992960\" | NaN<\"0\" | Infinity<\"0\" | \"7\"<\"7\" | \"x\"<\"0\" | null<\"0\" | undefined<\"0\"""#,
            ),
            (
                r#"E(() => P("video", "height", "height", "ulong", []))"#,
                r#""d=0 | \"5\">5 | \"-5\">0 | \"+5\">5 | \"5x\">5 | \" 5\">5 | \"\">0 | \"x\">0 | \"2147483647\">2147483647 | \"2147483648\">0 | \"-2147483648\">0 | \"-2147483649\">0 | \"1e3\">1 | \"0\">0 | \"00\">0 | \"5.9\">5 | \"-0\">0 | 0<\"0\" | 1<\"1\" | -1<\"0\" | 5.9<\"5\" | -5.9<\"0\" | 2147483647<\"2147483647\" | 2147483648<\"0\" | -2147483648<\"0\" | -2147483649<\"2147483647\" | 4294967295<\"0\" | 4294967296<\"0\" | 100000000000000000000<\"1661992960\" | NaN<\"0\" | Infinity<\"0\" | \"7\"<\"7\" | \"x\"<\"0\" | null<\"0\" | undefined<\"0\"""#,
            ),
            (
                r#"E(() => P("video", "poster", "poster", "url", []))"#,
                r##""d=\"\" | \"\">\"http://x.test/d/e.html\" | \"x\">\"http://x.test/d/x\" | \" x \">\"http://x.test/d/x\" | \"/p?q#f\">\"http://x.test/p?q#f\" | \"//h.test/p\">\"http://h.test/p\" | \"http://a.test/c d\">\"http://a.test/c%20d\" | \"javascript:void(0)\">\"javascript:void(0)\" | \"mailto:a@b.test\">\"mailto:a@b.test\" | \"#f\">\"http://x.test/d/e.html#f\" | \"?q\">\"http://x.test/d/e.html?q\" | \"x\\ty\">\"http://x.test/d/xy\" | \"http://[bad\">\"http://[bad\" | \"ftp://u:p@h.test:21/p\">\"ftp://u:p@h.test/p\" | \"\"<\"\" | \"x\"<\"x\" | \"/p\"<\"/p\" | null<\"null\" | 5<\"5\"""##,
            ),
            (
                r#"E(() => P("video", "playsInline", "playsinline", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-track-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("track", "kind", "kind", "enum", ["subtitles", "captions", "descriptions", "chapters", "metadata"]))"#,
                r#""d=\"subtitles\" | \"\">\"metadata\" | \"subtitles\">\"subtitles\" | \"captions\">\"captions\" | \"descriptions\">\"descriptions\" | \"chapters\">\"chapters\" | \"metadata\">\"metadata\" | \"SUBTITLES\">\"subtitles\" | \"CAPTIONS\">\"captions\" | \"DESCRIPTIONS\">\"descriptions\" | \"CHAPTERS\">\"chapters\" | \"METADATA\">\"metadata\" | \" subtitles\">\"metadata\" | \" captions\">\"metadata\" | \" descriptions\">\"metadata\" | \" chapters\">\"metadata\" | \" metadata\">\"metadata\" | \"junk\">\"metadata\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"subtitles\"<\"subtitles\" | \"captions\"<\"captions\" | \"descriptions\"<\"descriptions\"""#,
            ),
            (
                r#"E(() => P("track", "src", "src", "url", []))"#,
                r##""d=\"\" | \"\">\"http://x.test/d/e.html\" | \"x\">\"http://x.test/d/x\" | \" x \">\"http://x.test/d/x\" | \"/p?q#f\">\"http://x.test/p?q#f\" | \"//h.test/p\">\"http://h.test/p\" | \"http://a.test/c d\">\"http://a.test/c%20d\" | \"javascript:void(0)\">\"javascript:void(0)\" | \"mailto:a@b.test\">\"mailto:a@b.test\" | \"#f\">\"http://x.test/d/e.html#f\" | \"?q\">\"http://x.test/d/e.html?q\" | \"x\\ty\">\"http://x.test/d/xy\" | \"http://[bad\">\"http://[bad\" | \"ftp://u:p@h.test:21/p\">\"ftp://u:p@h.test/p\" | \"\"<\"\" | \"x\"<\"x\" | \"/p\"<\"/p\" | null<\"null\" | 5<\"5\"""##,
            ),
            (
                r#"E(() => P("track", "srclang", "srclang", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("track", "label", "label", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("track", "default", "default", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-map-01",
        html: PAGE,
        queries: &[(
            r#"E(() => P("map", "name", "name", "str", []))"#,
            r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
        )],
    },
    Case {
        name: "reflect-area-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("area", "alt", "alt", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("area", "coords", "coords", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("area", "shape", "shape", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("area", "download", "download", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("area", "ping", "ping", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("area", "rel", "rel", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("area", "referrerPolicy", "referrerpolicy", "enum", ["no-referrer", "no-referrer-when-downgrade", "same-origin", "origin", "strict-origin", "origin-when-cross-origin", "strict-origin-when-cross-origin", "unsafe-url"]))"#,
                r#""d=\"\" | \"\">\"\" | \"no-referrer\">\"no-referrer\" | \"no-referrer-when-downgrade\">\"no-referrer-when-downgrade\" | \"same-origin\">\"same-origin\" | \"origin\">\"origin\" | \"strict-origin\">\"strict-origin\" | \"origin-when-cross-origin\">\"origin-when-cross-origin\" | \"strict-origin-when-cross-origin\">\"strict-origin-when-cross-origin\" | \"unsafe-url\">\"unsafe-url\" | \"NO-REFERRER\">\"no-referrer\" | \"NO-REFERRER-WHEN-DOWNGRADE\">\"no-referrer-when-downgrade\" | \"SAME-ORIGIN\">\"same-origin\" | \"ORIGIN\">\"origin\" | \"STRICT-ORIGIN\">\"strict-origin\" | \"ORIGIN-WHEN-CROSS-ORIGIN\">\"origin-when-cross-origin\" | \"STRICT-ORIGIN-WHEN-CROSS-ORIGIN\">\"strict-origin-when-cross-origin\" | \"UNSAFE-URL\">\"unsafe-url\" | \" no-referrer\">\"\" | \" no-referrer-when-downgrade\">\"\" | \" same-origin\">\"\" | \" origin\">\"\" | \" strict-origin\">\"\" | \" origin-when-cross-origin\">\"\" | \" strict-origin-when-cross-origin\">\"\" | \" unsafe-url\">\"\" | \"junk\">\"\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"no-referrer\"<\"no-referrer\" | \"no-referrer-when-downgrade\"<\"no-referrer-when-downgrade\" | \"same-origin\"<\"same-origin\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-tablecol-01",
        html: PAGE,
        queries: &[(
            r#"E(() => P("col", "span", "span", "ulong", []))"#,
            r#""d=1 | \"5\">5 | \"-5\">1 | \"+5\">5 | \"5x\">5 | \" 5\">5 | \"\">1 | \"x\">1 | \"2147483647\">1000 | \"2147483648\">1000 | \"-2147483648\">1 | \"-2147483649\">1 | \"1e3\">1 | \"0\">1 | \"00\">1 | \"5.9\">5 | \"-0\">1 | 0<\"0\" | 1<\"1\" | -1<\"1\" | 5.9<\"5\" | -5.9<\"1\" | 2147483647<\"2147483647\" | 2147483648<\"1\" | -2147483648<\"1\" | -2147483649<\"2147483647\" | 4294967295<\"1\" | 4294967296<\"0\" | 100000000000000000000<\"1661992960\" | NaN<\"0\" | Infinity<\"0\" | \"7\"<\"7\" | \"x\"<\"0\" | null<\"0\" | undefined<\"0\"""#,
        )],
    },
    Case {
        name: "reflect-tablecell-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("td", "colSpan", "colspan", "ulong", []))"#,
                r#""d=1 | \"5\">5 | \"-5\">1 | \"+5\">5 | \"5x\">5 | \" 5\">5 | \"\">1 | \"x\">1 | \"2147483647\">1000 | \"2147483648\">1000 | \"-2147483648\">1 | \"-2147483649\">1 | \"1e3\">1 | \"0\">1 | \"00\">1 | \"5.9\">5 | \"-0\">1 | 0<\"0\" | 1<\"1\" | -1<\"1\" | 5.9<\"5\" | -5.9<\"1\" | 2147483647<\"2147483647\" | 2147483648<\"1\" | -2147483648<\"1\" | -2147483649<\"2147483647\" | 4294967295<\"1\" | 4294967296<\"0\" | 100000000000000000000<\"1661992960\" | NaN<\"0\" | Infinity<\"0\" | \"7\"<\"7\" | \"x\"<\"0\" | null<\"0\" | undefined<\"0\"""#,
            ),
            (
                r#"E(() => P("td", "rowSpan", "rowspan", "ulong", []))"#,
                r#""d=1 | \"5\">5 | \"-5\">1 | \"+5\">5 | \"5x\">5 | \" 5\">5 | \"\">1 | \"x\">1 | \"2147483647\">65534 | \"2147483648\">65534 | \"-2147483648\">1 | \"-2147483649\">1 | \"1e3\">1 | \"0\">0 | \"00\">0 | \"5.9\">5 | \"-0\">0 | 0<\"0\" | 1<\"1\" | -1<\"1\" | 5.9<\"5\" | -5.9<\"1\" | 2147483647<\"2147483647\" | 2147483648<\"1\" | -2147483648<\"1\" | -2147483649<\"2147483647\" | 4294967295<\"1\" | 4294967296<\"0\" | 100000000000000000000<\"1661992960\" | NaN<\"0\" | Infinity<\"0\" | \"7\"<\"7\" | \"x\"<\"0\" | null<\"0\" | undefined<\"0\"""#,
            ),
            (
                r#"E(() => P("td", "headers", "headers", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("td", "scope", "scope", "enum", ["row", "col", "rowgroup", "colgroup"]))"#,
                r#""d=\"\" | \"\">\"\" | \"row\">\"row\" | \"col\">\"col\" | \"rowgroup\">\"rowgroup\" | \"colgroup\">\"colgroup\" | \"ROW\">\"row\" | \"COL\">\"col\" | \"ROWGROUP\">\"rowgroup\" | \"COLGROUP\">\"colgroup\" | \" row\">\"\" | \" col\">\"\" | \" rowgroup\">\"\" | \" colgroup\">\"\" | \"junk\">\"\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"row\"<\"row\" | \"col\"<\"col\" | \"rowgroup\"<\"rowgroup\"""#,
            ),
            (
                r#"E(() => P("td", "abbr", "abbr", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-form-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("form", "acceptCharset", "accept-charset", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("form", "autocomplete", "autocomplete", "enum", ["on", "off"]))"#,
                r#""d=\"on\" | \"\">\"on\" | \"on\">\"on\" | \"off\">\"off\" | \"ON\">\"on\" | \"OFF\">\"off\" | \" on\">\"on\" | \" off\">\"on\" | \"junk\">\"on\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"on\"<\"on\" | \"off\"<\"off\"""#,
            ),
            (
                r#"E(() => P("form", "enctype", "enctype", "enum", ["application/x-www-form-urlencoded", "multipart/form-data", "text/plain"]))"#,
                r#""d=\"application/x-www-form-urlencoded\" | \"\">\"application/x-www-form-urlencoded\" | \"application/x-www-form-urlencoded\">\"application/x-www-form-urlencoded\" | \"multipart/form-data\">\"multipart/form-data\" | \"text/plain\">\"text/plain\" | \"APPLICATION/X-WWW-FORM-URLENCODED\">\"application/x-www-form-urlencoded\" | \"MULTIPART/FORM-DATA\">\"multipart/form-data\" | \"TEXT/PLAIN\">\"text/plain\" | \" application/x-www-form-urlencoded\">\"application/x-www-form-urlencoded\" | \" multipart/form-data\">\"application/x-www-form-urlencoded\" | \" text/plain\">\"application/x-www-form-urlencoded\" | \"junk\">\"application/x-www-form-urlencoded\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"application/x-www-form-urlencoded\"<\"application/x-www-form-urlencoded\" | \"multipart/form-data\"<\"multipart/form-data\" | \"text/plain\"<\"text/plain\"""#,
            ),
            (
                r#"E(() => P("form", "encoding", "enctype", "enum", ["application/x-www-form-urlencoded", "multipart/form-data", "text/plain"]))"#,
                r#""d=\"application/x-www-form-urlencoded\" | \"\">\"application/x-www-form-urlencoded\" | \"application/x-www-form-urlencoded\">\"application/x-www-form-urlencoded\" | \"multipart/form-data\">\"multipart/form-data\" | \"text/plain\">\"text/plain\" | \"APPLICATION/X-WWW-FORM-URLENCODED\">\"application/x-www-form-urlencoded\" | \"MULTIPART/FORM-DATA\">\"multipart/form-data\" | \"TEXT/PLAIN\">\"text/plain\" | \" application/x-www-form-urlencoded\">\"application/x-www-form-urlencoded\" | \" multipart/form-data\">\"application/x-www-form-urlencoded\" | \" text/plain\">\"application/x-www-form-urlencoded\" | \"junk\">\"application/x-www-form-urlencoded\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"application/x-www-form-urlencoded\"<\"application/x-www-form-urlencoded\" | \"multipart/form-data\"<\"multipart/form-data\" | \"text/plain\"<\"text/plain\"""#,
            ),
            (
                r#"E(() => P("form", "method", "method", "enum", ["get", "post", "dialog"]))"#,
                r#""d=\"get\" | \"\">\"get\" | \"get\">\"get\" | \"post\">\"post\" | \"dialog\">\"dialog\" | \"GET\">\"get\" | \"POST\">\"post\" | \"DIALOG\">\"dialog\" | \" get\">\"get\" | \" post\">\"get\" | \" dialog\">\"get\" | \"junk\">\"get\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"get\"<\"get\" | \"post\"<\"post\" | \"dialog\"<\"dialog\"""#,
            ),
            (
                r#"E(() => P("form", "name", "name", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("form", "noValidate", "novalidate", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("form", "target", "target", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("form", "rel", "rel", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-label-01",
        html: PAGE,
        queries: &[(
            r#"E(() => P("label", "htmlFor", "for", "str", []))"#,
            r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
        )],
    },
    Case {
        name: "reflect-input-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("input", "accept", "accept", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("input", "alt", "alt", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("input", "defaultChecked", "checked", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("input", "dirName", "dirname", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("input", "disabled", "disabled", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("input", "formEnctype", "formenctype", "enum", ["application/x-www-form-urlencoded", "multipart/form-data", "text/plain"]))"#,
                r#""d=\"\" | \"\">\"application/x-www-form-urlencoded\" | \"application/x-www-form-urlencoded\">\"application/x-www-form-urlencoded\" | \"multipart/form-data\">\"multipart/form-data\" | \"text/plain\">\"text/plain\" | \"APPLICATION/X-WWW-FORM-URLENCODED\">\"application/x-www-form-urlencoded\" | \"MULTIPART/FORM-DATA\">\"multipart/form-data\" | \"TEXT/PLAIN\">\"text/plain\" | \" application/x-www-form-urlencoded\">\"application/x-www-form-urlencoded\" | \" multipart/form-data\">\"application/x-www-form-urlencoded\" | \" text/plain\">\"application/x-www-form-urlencoded\" | \"junk\">\"application/x-www-form-urlencoded\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"application/x-www-form-urlencoded\"<\"application/x-www-form-urlencoded\" | \"multipart/form-data\"<\"multipart/form-data\" | \"text/plain\"<\"text/plain\"""#,
            ),
            (
                r#"E(() => P("input", "formMethod", "formmethod", "enum", ["get", "post", "dialog"]))"#,
                r#""d=\"\" | \"\">\"get\" | \"get\">\"get\" | \"post\">\"post\" | \"dialog\">\"dialog\" | \"GET\">\"get\" | \"POST\">\"post\" | \"DIALOG\">\"dialog\" | \" get\">\"get\" | \" post\">\"get\" | \" dialog\">\"get\" | \"junk\">\"get\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"get\"<\"get\" | \"post\"<\"post\" | \"dialog\"<\"dialog\"""#,
            ),
            (
                r#"E(() => P("input", "formNoValidate", "formnovalidate", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("input", "formTarget", "formtarget", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("input", "max", "max", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("input", "maxLength", "maxlength", "nonneg", []))"#,
                r#""d=-1 | \"5\">5 | \"-5\">-1 | \"+5\">5 | \"5x\">5 | \" 5\">5 | \"\">-1 | \"x\">-1 | \"2147483647\">2147483647 | \"2147483648\">-1 | \"-2147483648\">-1 | \"-2147483649\">-1 | \"1e3\">1 | \"0\">0 | \"00\">0 | \"5.9\">5 | \"-0\">0 | 0<\"0\" | 1<\"1\" | -1<!DOMException:Failed to set the 'maxLength' property on 'HTMLInputElement': The value provided (-1) is not positive or 0. | 5.9<\"5\" | -5.9<!DOMException:Failed to set the 'maxLength' property on 'HTMLInputElement': The value provided (-5) is not positive or 0. | 2147483647<\"2147483647\" | 2147483648<!DOMException:Failed to set the 'maxLength' property on 'HTMLInputElement': The value provided (-2147483648) is not positive or 0. | -2147483648<!DOMException:Failed to set the 'maxLength' property on 'HTMLInputElement': The value provided (-2147483648) is not positive or 0. | -2147483649<\"2147483647\" | 4294967295<!DOMException:Failed to set the 'maxLength' property on 'HTMLInputElement': The value provided (-1) is not positive or 0. | 4294967296<\"0\" | 100000000000000000000<\"1661992960\" | NaN<\"0\" | Infinity<\"0\" | \"7\"<\"7\" | \"x\"<\"0\" | null<\"0\" | undefined<\"0\"""#,
            ),
            (
                r#"E(() => P("input", "min", "min", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-input-02",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("input", "minLength", "minlength", "nonneg", []))"#,
                r#""d=-1 | \"5\">5 | \"-5\">-1 | \"+5\">5 | \"5x\">5 | \" 5\">5 | \"\">-1 | \"x\">-1 | \"2147483647\">2147483647 | \"2147483648\">-1 | \"-2147483648\">-1 | \"-2147483649\">-1 | \"1e3\">1 | \"0\">0 | \"00\">0 | \"5.9\">5 | \"-0\">0 | 0<\"0\" | 1<\"1\" | -1<!DOMException:Failed to set the 'minLength' property on 'HTMLInputElement': The value provided (-1) is not positive or 0. | 5.9<\"5\" | -5.9<!DOMException:Failed to set the 'minLength' property on 'HTMLInputElement': The value provided (-5) is not positive or 0. | 2147483647<\"2147483647\" | 2147483648<!DOMException:Failed to set the 'minLength' property on 'HTMLInputElement': The value provided (-2147483648) is not positive or 0. | -2147483648<!DOMException:Failed to set the 'minLength' property on 'HTMLInputElement': The value provided (-2147483648) is not positive or 0. | -2147483649<\"2147483647\" | 4294967295<!DOMException:Failed to set the 'minLength' property on 'HTMLInputElement': The value provided (-1) is not positive or 0. | 4294967296<\"0\" | 100000000000000000000<\"1661992960\" | NaN<\"0\" | Infinity<\"0\" | \"7\"<\"7\" | \"x\"<\"0\" | null<\"0\" | undefined<\"0\"""#,
            ),
            (
                r#"E(() => P("input", "multiple", "multiple", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("input", "name", "name", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("input", "pattern", "pattern", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("input", "placeholder", "placeholder", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("input", "readOnly", "readonly", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("input", "required", "required", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("input", "size", "size", "ulong", []))"#,
                r#""d=20 | \"5\">5 | \"-5\">20 | \"+5\">5 | \"5x\">5 | \" 5\">5 | \"\">20 | \"x\">20 | \"2147483647\">2147483647 | \"2147483648\">20 | \"-2147483648\">20 | \"-2147483649\">20 | \"1e3\">1 | \"0\">20 | \"00\">20 | \"5.9\">5 | \"-0\">20 | 0<!DOMException:Failed to set the 'size' property on 'HTMLInputElement': The value provided is 0, which is an invalid size. | 1<\"1\" | -1<\"20\" | 5.9<\"5\" | -5.9<\"20\" | 2147483647<\"2147483647\" | 2147483648<\"20\" | -2147483648<\"20\" | -2147483649<\"2147483647\" | 4294967295<\"20\" | 4294967296<!DOMException:Failed to set the 'size' property on 'HTMLInputElement': The value provided is 0, which is an invalid size. | 100000000000000000000<\"1661992960\" | NaN<!DOMException:Failed to set the 'size' property on 'HTMLInputElement': The value provided is 0, which is an invalid size. | Infinity<!DOMException:Failed to set the 'size' property on 'HTMLInputElement': The value provided is 0, which is an invalid size. | \"7\"<\"7\" | \"x\"<!DOMException:Failed to set the 'size' property on 'HTMLInputElement': The value provided is 0, which is an invalid size. | null<!DOMException:Failed to set the 'size' property on 'HTMLInputElement': The value provided is 0, which is an invalid size. | undefined<!DOMException:Failed to set the 'size' property on 'HTMLInputElement': The value provided is 0, which is an invalid size.""#,
            ),
            (
                r#"E(() => P("input", "src", "src", "url", []))"#,
                r##""d=\"\" | \"\">\"http://x.test/d/e.html\" | \"x\">\"http://x.test/d/x\" | \" x \">\"http://x.test/d/x\" | \"/p?q#f\">\"http://x.test/p?q#f\" | \"//h.test/p\">\"http://h.test/p\" | \"http://a.test/c d\">\"http://a.test/c%20d\" | \"javascript:void(0)\">\"javascript:void(0)\" | \"mailto:a@b.test\">\"mailto:a@b.test\" | \"#f\">\"http://x.test/d/e.html#f\" | \"?q\">\"http://x.test/d/e.html?q\" | \"x\\ty\">\"http://x.test/d/xy\" | \"http://[bad\">\"http://[bad\" | \"ftp://u:p@h.test:21/p\">\"ftp://u:p@h.test/p\" | \"\"<\"\" | \"x\"<\"x\" | \"/p\"<\"/p\" | null<\"null\" | 5<\"5\"""##,
            ),
            (
                r#"E(() => P("input", "step", "step", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("input", "type", "type", "enum", ["hidden", "text", "search", "tel", "url", "email", "password", "date", "month", "week", "time", "datetime-local", "number", "range", "color", "checkbox", "radio", "file", "submit", "image", "reset", "button"]))"#,
                r#""d=\"text\" | \"\">\"text\" | \"hidden\">\"hidden\" | \"text\">\"text\" | \"search\">\"search\" | \"tel\">\"tel\" | \"url\">\"url\" | \"email\">\"email\" | \"password\">\"password\" | \"date\">\"date\" | \"month\">\"month\" | \"week\">\"week\" | \"time\">\"time\" | \"datetime-local\">\"datetime-local\" | \"number\">\"number\" | \"range\">\"range\" | \"color\">\"color\" | \"checkbox\">\"checkbox\" | \"radio\">\"radio\" | \"file\">\"file\" | \"submit\">\"submit\" | \"image\">\"image\" | \"reset\">\"reset\" | \"button\">\"button\" | \"HIDDEN\">\"hidden\" | \"TEXT\">\"text\" | \"SEARCH\">\"search\" | \"TEL\">\"tel\" | \"URL\">\"url\" | \"EMAIL\">\"email\" | \"PASSWORD\">\"password\" | \"DATE\">\"date\" | \"MONTH\">\"month\" | \"WEEK\">\"week\" | \"TIME\">\"time\" | \"DATETIME-LOCAL\">\"datetime-local\" | \"NUMBER\">\"number\" | \"RANGE\">\"range\" | \"COLOR\">\"color\" | \"CHECKBOX\">\"checkbox\" | \"RADIO\">\"radio\" | \"FILE\">\"file\" | \"SUBMIT\">\"submit\" | \"IMAGE\">\"image\" | \"RESET\">\"reset\" | \"BUTTON\">\"button\" | \" hidden\">\"text\" | \" text\">\"text\" | \" search\">\"text\" | \" tel\">\"text\" | \" url\">\"text\" | \" email\">\"text\" | \" password\">\"text\" | \" date\">\"text\" | \" month\">\"text\" | \" week\">\"text\" | \" time\">\"text\" | \" datetime-local\">\"text\" | \" number\">\"text\" | \" range\">\"text\" | \" color\">\"text\" | \" checkbox\">\"text\" | \" radio\">\"text\" | \" file\">\"text\" | \" submit\">\"text\" | \" image\">\"text\" | \" reset\">\"text\" | \" button\">\"text\" | \"junk\">\"text\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"hidden\"<\"hidden\" | \"text\"<\"text\" | \"search\"<\"search\"""#,
            ),
            (
                r#"E(() => P("input", "defaultValue", "value", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-button-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("button", "disabled", "disabled", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("button", "formEnctype", "formenctype", "enum", ["application/x-www-form-urlencoded", "multipart/form-data", "text/plain"]))"#,
                r#""d=\"\" | \"\">\"application/x-www-form-urlencoded\" | \"application/x-www-form-urlencoded\">\"application/x-www-form-urlencoded\" | \"multipart/form-data\">\"multipart/form-data\" | \"text/plain\">\"text/plain\" | \"APPLICATION/X-WWW-FORM-URLENCODED\">\"application/x-www-form-urlencoded\" | \"MULTIPART/FORM-DATA\">\"multipart/form-data\" | \"TEXT/PLAIN\">\"text/plain\" | \" application/x-www-form-urlencoded\">\"application/x-www-form-urlencoded\" | \" multipart/form-data\">\"application/x-www-form-urlencoded\" | \" text/plain\">\"application/x-www-form-urlencoded\" | \"junk\">\"application/x-www-form-urlencoded\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"application/x-www-form-urlencoded\"<\"application/x-www-form-urlencoded\" | \"multipart/form-data\"<\"multipart/form-data\" | \"text/plain\"<\"text/plain\"""#,
            ),
            (
                r#"E(() => P("button", "formMethod", "formmethod", "enum", ["get", "post", "dialog"]))"#,
                r#""d=\"\" | \"\">\"get\" | \"get\">\"get\" | \"post\">\"post\" | \"dialog\">\"dialog\" | \"GET\">\"get\" | \"POST\">\"post\" | \"DIALOG\">\"dialog\" | \" get\">\"get\" | \" post\">\"get\" | \" dialog\">\"get\" | \"junk\">\"get\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"get\"<\"get\" | \"post\"<\"post\" | \"dialog\"<\"dialog\"""#,
            ),
            (
                r#"E(() => P("button", "formNoValidate", "formnovalidate", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("button", "formTarget", "formtarget", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("button", "name", "name", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("button", "type", "type", "enum", ["submit", "reset", "button"]))"#,
                r#""d=\"submit\" | \"\">\"submit\" | \"submit\">\"submit\" | \"reset\">\"reset\" | \"button\">\"button\" | \"SUBMIT\">\"submit\" | \"RESET\">\"reset\" | \"BUTTON\">\"button\" | \" submit\">\"submit\" | \" reset\">\"submit\" | \" button\">\"submit\" | \"junk\">\"submit\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"submit\"<\"submit\" | \"reset\"<\"reset\" | \"button\"<\"button\"""#,
            ),
            (
                r#"E(() => P("button", "value", "value", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-select-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("select", "disabled", "disabled", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("select", "multiple", "multiple", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("select", "name", "name", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("select", "required", "required", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("select", "size", "size", "ulong", []))"#,
                r#""d=0 | \"5\">5 | \"-5\">0 | \"+5\">5 | \"5x\">5 | \" 5\">5 | \"\">0 | \"x\">0 | \"2147483647\">2147483647 | \"2147483648\">0 | \"-2147483648\">0 | \"-2147483649\">0 | \"1e3\">1 | \"0\">0 | \"00\">0 | \"5.9\">5 | \"-0\">0 | 0<\"0\" | 1<\"1\" | -1<\"0\" | 5.9<\"5\" | -5.9<\"0\" | 2147483647<\"2147483647\" | 2147483648<\"0\" | -2147483648<\"0\" | -2147483649<\"2147483647\" | 4294967295<\"0\" | 4294967296<\"0\" | 100000000000000000000<\"1661992960\" | NaN<\"0\" | Infinity<\"0\" | \"7\"<\"7\" | \"x\"<\"0\" | null<\"0\" | undefined<\"0\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-optgroup-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("optgroup", "disabled", "disabled", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("optgroup", "label", "label", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-option-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("option", "disabled", "disabled", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("option", "defaultSelected", "selected", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-textarea-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("textarea", "cols", "cols", "ulong", []))"#,
                r#""d=20 | \"5\">5 | \"-5\">20 | \"+5\">5 | \"5x\">5 | \" 5\">5 | \"\">20 | \"x\">20 | \"2147483647\">2147483647 | \"2147483648\">20 | \"-2147483648\">20 | \"-2147483649\">20 | \"1e3\">1 | \"0\">20 | \"00\">20 | \"5.9\">5 | \"-0\">20 | 0<\"20\" | 1<\"1\" | -1<\"20\" | 5.9<\"5\" | -5.9<\"20\" | 2147483647<\"2147483647\" | 2147483648<\"20\" | -2147483648<\"20\" | -2147483649<\"2147483647\" | 4294967295<\"20\" | 4294967296<\"20\" | 100000000000000000000<\"1661992960\" | NaN<\"20\" | Infinity<\"20\" | \"7\"<\"7\" | \"x\"<\"20\" | null<\"20\" | undefined<\"20\"""#,
            ),
            (
                r#"E(() => P("textarea", "dirName", "dirname", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("textarea", "disabled", "disabled", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("textarea", "maxLength", "maxlength", "nonneg", []))"#,
                r#""d=-1 | \"5\">5 | \"-5\">-1 | \"+5\">5 | \"5x\">5 | \" 5\">5 | \"\">-1 | \"x\">-1 | \"2147483647\">2147483647 | \"2147483648\">-1 | \"-2147483648\">-1 | \"-2147483649\">-1 | \"1e3\">1 | \"0\">0 | \"00\">0 | \"5.9\">5 | \"-0\">0 | 0<\"0\" | 1<\"1\" | -1<!DOMException:Failed to set the 'maxLength' property on 'HTMLTextAreaElement': The value provided (-1) is not positive or 0. | 5.9<\"5\" | -5.9<!DOMException:Failed to set the 'maxLength' property on 'HTMLTextAreaElement': The value provided (-5) is not positive or 0. | 2147483647<\"2147483647\" | 2147483648<!DOMException:Failed to set the 'maxLength' property on 'HTMLTextAreaElement': The value provided (-2147483648) is not positive or 0. | -2147483648<!DOMException:Failed to set the 'maxLength' property on 'HTMLTextAreaElement': The value provided (-2147483648) is not positive or 0. | -2147483649<\"2147483647\" | 4294967295<!DOMException:Failed to set the 'maxLength' property on 'HTMLTextAreaElement': The value provided (-1) is not positive or 0. | 4294967296<\"0\" | 100000000000000000000<\"1661992960\" | NaN<\"0\" | Infinity<\"0\" | \"7\"<\"7\" | \"x\"<\"0\" | null<\"0\" | undefined<\"0\"""#,
            ),
            (
                r#"E(() => P("textarea", "minLength", "minlength", "nonneg", []))"#,
                r#""d=-1 | \"5\">5 | \"-5\">-1 | \"+5\">5 | \"5x\">5 | \" 5\">5 | \"\">-1 | \"x\">-1 | \"2147483647\">2147483647 | \"2147483648\">-1 | \"-2147483648\">-1 | \"-2147483649\">-1 | \"1e3\">1 | \"0\">0 | \"00\">0 | \"5.9\">5 | \"-0\">0 | 0<\"0\" | 1<\"1\" | -1<!DOMException:Failed to set the 'minLength' property on 'HTMLTextAreaElement': The value provided (-1) is not positive or 0. | 5.9<\"5\" | -5.9<!DOMException:Failed to set the 'minLength' property on 'HTMLTextAreaElement': The value provided (-5) is not positive or 0. | 2147483647<\"2147483647\" | 2147483648<!DOMException:Failed to set the 'minLength' property on 'HTMLTextAreaElement': The value provided (-2147483648) is not positive or 0. | -2147483648<!DOMException:Failed to set the 'minLength' property on 'HTMLTextAreaElement': The value provided (-2147483648) is not positive or 0. | -2147483649<\"2147483647\" | 4294967295<!DOMException:Failed to set the 'minLength' property on 'HTMLTextAreaElement': The value provided (-1) is not positive or 0. | 4294967296<\"0\" | 100000000000000000000<\"1661992960\" | NaN<\"0\" | Infinity<\"0\" | \"7\"<\"7\" | \"x\"<\"0\" | null<\"0\" | undefined<\"0\"""#,
            ),
            (
                r#"E(() => P("textarea", "name", "name", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("textarea", "placeholder", "placeholder", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("textarea", "readOnly", "readonly", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("textarea", "required", "required", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("textarea", "rows", "rows", "ulong", []))"#,
                r#""d=2 | \"5\">5 | \"-5\">2 | \"+5\">5 | \"5x\">5 | \" 5\">5 | \"\">2 | \"x\">2 | \"2147483647\">2147483647 | \"2147483648\">2 | \"-2147483648\">2 | \"-2147483649\">2 | \"1e3\">1 | \"0\">2 | \"00\">2 | \"5.9\">5 | \"-0\">2 | 0<\"2\" | 1<\"1\" | -1<\"2\" | 5.9<\"5\" | -5.9<\"2\" | 2147483647<\"2147483647\" | 2147483648<\"2\" | -2147483648<\"2\" | -2147483649<\"2147483647\" | 4294967295<\"2\" | 4294967296<\"2\" | 100000000000000000000<\"1661992960\" | NaN<\"2\" | Infinity<\"2\" | \"7\"<\"7\" | \"x\"<\"2\" | null<\"2\" | undefined<\"2\"""#,
            ),
            (
                r#"E(() => P("textarea", "wrap", "wrap", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-output-01",
        html: PAGE,
        queries: &[(
            r#"E(() => P("output", "name", "name", "str", []))"#,
            r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
        )],
    },
    Case {
        name: "reflect-fieldset-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("fieldset", "disabled", "disabled", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("fieldset", "name", "name", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-details-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("details", "name", "name", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("details", "open", "open", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-dialog-01",
        html: PAGE,
        queries: &[(
            r#"E(() => P("dialog", "open", "open", "bool", []))"#,
            r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
        )],
    },
    Case {
        name: "reflect-slot-01",
        html: PAGE,
        queries: &[(
            r#"E(() => P("slot", "name", "name", "str", []))"#,
            r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
        )],
    },
    Case {
        name: "reflect-canvas-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("canvas", "width", "width", "ulong", []))"#,
                r#""d=300 | \"5\">5 | \"-5\">300 | \"+5\">5 | \"5x\">5 | \" 5\">5 | \"\">300 | \"x\">300 | \"2147483647\">2147483647 | \"2147483648\">300 | \"-2147483648\">300 | \"-2147483649\">300 | \"1e3\">1 | \"0\">0 | \"00\">0 | \"5.9\">5 | \"-0\">0 | 0<\"0\" | 1<\"1\" | -1<\"300\" | 5.9<\"5\" | -5.9<\"300\" | 2147483647<\"2147483647\" | 2147483648<\"300\" | -2147483648<\"300\" | -2147483649<\"2147483647\" | 4294967295<\"300\" | 4294967296<\"0\" | 100000000000000000000<\"1661992960\" | NaN<\"0\" | Infinity<\"0\" | \"7\"<\"7\" | \"x\"<\"0\" | null<\"0\" | undefined<\"0\"""#,
            ),
            (
                r#"E(() => P("canvas", "height", "height", "ulong", []))"#,
                r#""d=150 | \"5\">5 | \"-5\">150 | \"+5\">5 | \"5x\">5 | \" 5\">5 | \"\">150 | \"x\">150 | \"2147483647\">2147483647 | \"2147483648\">150 | \"-2147483648\">150 | \"-2147483649\">150 | \"1e3\">1 | \"0\">0 | \"00\">0 | \"5.9\">5 | \"-0\">0 | 0<\"0\" | 1<\"1\" | -1<\"150\" | 5.9<\"5\" | -5.9<\"150\" | 2147483647<\"2147483647\" | 2147483648<\"150\" | -2147483648<\"150\" | -2147483649<\"2147483647\" | 4294967295<\"150\" | 4294967296<\"0\" | 100000000000000000000<\"1661992960\" | NaN<\"0\" | Infinity<\"0\" | \"7\"<\"7\" | \"x\"<\"0\" | null<\"0\" | undefined<\"0\"""#,
            ),
        ],
    },
    Case {
        name: "reflect-script-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => P("script", "src", "src", "url", []))"#,
                r##""d=\"\" | \"\">\"http://x.test/d/e.html\" | \"x\">\"http://x.test/d/x\" | \" x \">\"http://x.test/d/x\" | \"/p?q#f\">\"http://x.test/p?q#f\" | \"//h.test/p\">\"http://h.test/p\" | \"http://a.test/c d\">\"http://a.test/c%20d\" | \"javascript:void(0)\">\"javascript:void(0)\" | \"mailto:a@b.test\">\"mailto:a@b.test\" | \"#f\">\"http://x.test/d/e.html#f\" | \"?q\">\"http://x.test/d/e.html?q\" | \"x\\ty\">\"http://x.test/d/xy\" | \"http://[bad\">\"http://[bad\" | \"ftp://u:p@h.test:21/p\">\"ftp://u:p@h.test/p\" | \"\"<\"\" | \"x\"<\"x\" | \"/p\"<\"/p\" | null<\"null\" | 5<\"5\"""##,
            ),
            (
                r#"E(() => P("script", "type", "type", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("script", "noModule", "nomodule", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("script", "defer", "defer", "bool", []))"#,
                r#""d=false | \"\">true | \"x\">true | \"false\">true | \"true\">true | \"FALSE\">true | true<\"\" | false<null | 0<null | 1<\"\" | \"\"<null | \"x\"<\"\" | null<null | undefined<null | NaN<null | [object Object]<\"\"""#,
            ),
            (
                r#"E(() => P("script", "crossOrigin", "crossorigin", "enumnull", ["anonymous", "use-credentials"]))"#,
                r#""d=null | \"\">\"anonymous\" | \"anonymous\">\"anonymous\" | \"use-credentials\">\"use-credentials\" | \"ANONYMOUS\">\"anonymous\" | \"USE-CREDENTIALS\">\"use-credentials\" | \" anonymous\">\"anonymous\" | \" use-credentials\">\"anonymous\" | \"junk\">\"anonymous\" | \"\"<\"\" | \"x\"<\"x\" | null<null | \"X\"<\"X\" | \"anonymous\"<\"anonymous\" | \"use-credentials\"<\"use-credentials\"""#,
            ),
            (
                r#"E(() => P("script", "referrerPolicy", "referrerpolicy", "enumnull", ["no-referrer", "no-referrer-when-downgrade", "same-origin", "origin", "strict-origin", "origin-when-cross-origin", "strict-origin-when-cross-origin", "unsafe-url"]))"#,
                r#""d=\"\" | \"\">\"\" | \"no-referrer\">\"no-referrer\" | \"no-referrer-when-downgrade\">\"no-referrer-when-downgrade\" | \"same-origin\">\"same-origin\" | \"origin\">\"origin\" | \"strict-origin\">\"strict-origin\" | \"origin-when-cross-origin\">\"origin-when-cross-origin\" | \"strict-origin-when-cross-origin\">\"strict-origin-when-cross-origin\" | \"unsafe-url\">\"unsafe-url\" | \"NO-REFERRER\">\"no-referrer\" | \"NO-REFERRER-WHEN-DOWNGRADE\">\"no-referrer-when-downgrade\" | \"SAME-ORIGIN\">\"same-origin\" | \"ORIGIN\">\"origin\" | \"STRICT-ORIGIN\">\"strict-origin\" | \"ORIGIN-WHEN-CROSS-ORIGIN\">\"origin-when-cross-origin\" | \"STRICT-ORIGIN-WHEN-CROSS-ORIGIN\">\"strict-origin-when-cross-origin\" | \"UNSAFE-URL\">\"unsafe-url\" | \" no-referrer\">\"\" | \" no-referrer-when-downgrade\">\"\" | \" same-origin\">\"\" | \" origin\">\"\" | \" strict-origin\">\"\" | \" origin-when-cross-origin\">\"\" | \" strict-origin-when-cross-origin\">\"\" | \" unsafe-url\">\"\" | \"junk\">\"\" | \"\"<\"\" | \"x\"<\"x\" | null<null | \"X\"<\"X\" | \"no-referrer\"<\"no-referrer\" | \"no-referrer-when-downgrade\"<\"no-referrer-when-downgrade\" | \"same-origin\"<\"same-origin\"""#,
            ),
            (
                r#"E(() => P("script", "integrity", "integrity", "str", []))"#,
                r#""d=\"\" | \"\">\"\" | \"x\">\"x\" | \" Mixed Case \">\" Mixed Case \" | \"a b\">\"a b\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | undefined<\"undefined\" | 5<\"5\" | true<\"true\" | obj<\"obj\"""#,
            ),
            (
                r#"E(() => P("script", "fetchPriority", "fetchpriority", "enum", ["high", "low", "auto"]))"#,
                r#""d=\"auto\" | \"\">\"auto\" | \"high\">\"high\" | \"low\">\"low\" | \"auto\">\"auto\" | \"HIGH\">\"high\" | \"LOW\">\"low\" | \"AUTO\">\"auto\" | \" high\">\"auto\" | \" low\">\"auto\" | \" auto\">\"auto\" | \"junk\">\"auto\" | \"\"<\"\" | \"x\"<\"x\" | null<\"null\" | \"X\"<\"X\" | \"high\"<\"high\" | \"low\"<\"low\" | \"auto\"<\"auto\"""#,
            ),
        ],
    },
    Case {
        name: "chains-01",
        html: PAGE,
        queries: &[
            (
                r"E(() => ['a', 'abbr', 'address', 'area', 'article', 'aside', 'audio', 'b', 'base', 'bdi', 'bdo', 'blockquote', 'body', 'br', 'button', 'canvas', 'caption', 'cite', 'code', 'col', 'colgroup', 'data', 'datalist', 'dd', 'del'].map((t) => { const e = document.createElement(t); return t + '=' + e.constructor.name + '/' + Object.prototype.toString.call(e) + '/' + C(e).split(' ').slice(0, 2).join(''); }).join(' '))",
                r#""a=HTMLAnchorElement/[object HTMLAnchorElement]/[objectHTMLAnchorElement] abbr=HTMLElement/[object HTMLElement]/[objectHTMLElement] address=HTMLElement/[object HTMLElement]/[objectHTMLElement] area=HTMLAreaElement/[object HTMLAreaElement]/[objectHTMLAreaElement] article=HTMLElement/[object HTMLElement]/[objectHTMLElement] aside=HTMLElement/[object HTMLElement]/[objectHTMLElement] audio=HTMLAudioElement/[object HTMLAudioElement]/[objectHTMLAudioElement] b=HTMLElement/[object HTMLElement]/[objectHTMLElement] base=HTMLBaseElement/[object HTMLBaseElement]/[objectHTMLBaseElement] bdi=HTMLElement/[object HTMLElement]/[objectHTMLElement] bdo=HTMLElement/[object HTMLElement]/[objectHTMLElement] blockquote=HTMLQuoteElement/[object HTMLQuoteElement]/[objectHTMLQuoteElement] body=HTMLBodyElement/[object HTMLBodyElement]/[objectHTMLBodyElement] br=HTMLBRElement/[object HTMLBRElement]/[objectHTMLBRElement] button=HTMLButtonElement/[object HTMLButtonElement]/[objectHTMLButtonElement] canvas=HTMLCanvasElement/[object HTMLCanvasElement]/[objectHTMLCanvasElement] caption=HTMLTableCaptionElement/[object HTMLTableCaptionElement]/[objectHTMLTableCaptionElement] cite=HTMLElement/[object HTMLElement]/[objectHTMLElement] code=HTMLElement/[object HTMLElement]/[objectHTMLElement] col=HTMLTableColElement/[object HTMLTableColElement]/[objectHTMLTableColElement] colgroup=HTMLTableColElement/[object HTMLTableColElement]/[objectHTMLTableColElement] data=HTMLDataElement/[object HTMLDataElement]/[objectHTMLDataElement] datalist=HTMLDataListElement/[object HTMLDataListElement]/[objectHTMLDataListElement] dd=HTMLElement/[object HTMLElement]/[objectHTMLElement] del=HTMLModElement/[object HTMLModElement]/[objectHTMLModElement]""#,
            ),
            (
                r"E(() => ['details', 'dfn', 'dialog', 'div', 'dl', 'dt', 'em', 'embed', 'fieldset', 'figcaption', 'figure', 'footer', 'form', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'head', 'header', 'hgroup', 'hr', 'html', 'i'].map((t) => { const e = document.createElement(t); return t + '=' + e.constructor.name + '/' + Object.prototype.toString.call(e) + '/' + C(e).split(' ').slice(0, 2).join(''); }).join(' '))",
                r#""details=HTMLDetailsElement/[object HTMLDetailsElement]/[objectHTMLDetailsElement] dfn=HTMLElement/[object HTMLElement]/[objectHTMLElement] dialog=HTMLDialogElement/[object HTMLDialogElement]/[objectHTMLDialogElement] div=HTMLDivElement/[object HTMLDivElement]/[objectHTMLDivElement] dl=HTMLDListElement/[object HTMLDListElement]/[objectHTMLDListElement] dt=HTMLElement/[object HTMLElement]/[objectHTMLElement] em=HTMLElement/[object HTMLElement]/[objectHTMLElement] embed=HTMLEmbedElement/[object HTMLEmbedElement]/[objectHTMLEmbedElement] fieldset=HTMLFieldSetElement/[object HTMLFieldSetElement]/[objectHTMLFieldSetElement] figcaption=HTMLElement/[object HTMLElement]/[objectHTMLElement] figure=HTMLElement/[object HTMLElement]/[objectHTMLElement] footer=HTMLElement/[object HTMLElement]/[objectHTMLElement] form=HTMLFormElement/[object HTMLFormElement]/[objectHTMLFormElement] h1=HTMLHeadingElement/[object HTMLHeadingElement]/[objectHTMLHeadingElement] h2=HTMLHeadingElement/[object HTMLHeadingElement]/[objectHTMLHeadingElement] h3=HTMLHeadingElement/[object HTMLHeadingElement]/[objectHTMLHeadingElement] h4=HTMLHeadingElement/[object HTMLHeadingElement]/[objectHTMLHeadingElement] h5=HTMLHeadingElement/[object HTMLHeadingElement]/[objectHTMLHeadingElement] h6=HTMLHeadingElement/[object HTMLHeadingElement]/[objectHTMLHeadingElement] head=HTMLHeadElement/[object HTMLHeadElement]/[objectHTMLHeadElement] header=HTMLElement/[object HTMLElement]/[objectHTMLElement] hgroup=HTMLElement/[object HTMLElement]/[objectHTMLElement] hr=HTMLHRElement/[object HTMLHRElement]/[objectHTMLHRElement] html=HTMLHtmlElement/[object HTMLHtmlElement]/[objectHTMLHtmlElement] i=HTMLElement/[object HTMLElement]/[objectHTMLElement]""#,
            ),
            (
                r"E(() => ['iframe', 'img', 'input', 'ins', 'kbd', 'label', 'legend', 'li', 'link', 'main', 'map', 'mark', 'menu', 'meta', 'meter', 'nav', 'noscript', 'object', 'ol', 'optgroup', 'option', 'output', 'p', 'param', 'picture'].map((t) => { const e = document.createElement(t); return t + '=' + e.constructor.name + '/' + Object.prototype.toString.call(e) + '/' + C(e).split(' ').slice(0, 2).join(''); }).join(' '))",
                r#""iframe=HTMLIFrameElement/[object HTMLIFrameElement]/[objectHTMLIFrameElement] img=HTMLImageElement/[object HTMLImageElement]/[objectHTMLImageElement] input=HTMLInputElement/[object HTMLInputElement]/[objectHTMLInputElement] ins=HTMLModElement/[object HTMLModElement]/[objectHTMLModElement] kbd=HTMLElement/[object HTMLElement]/[objectHTMLElement] label=HTMLLabelElement/[object HTMLLabelElement]/[objectHTMLLabelElement] legend=HTMLLegendElement/[object HTMLLegendElement]/[objectHTMLLegendElement] li=HTMLLIElement/[object HTMLLIElement]/[objectHTMLLIElement] link=HTMLLinkElement/[object HTMLLinkElement]/[objectHTMLLinkElement] main=HTMLElement/[object HTMLElement]/[objectHTMLElement] map=HTMLMapElement/[object HTMLMapElement]/[objectHTMLMapElement] mark=HTMLElement/[object HTMLElement]/[objectHTMLElement] menu=HTMLMenuElement/[object HTMLMenuElement]/[objectHTMLMenuElement] meta=HTMLMetaElement/[object HTMLMetaElement]/[objectHTMLMetaElement] meter=HTMLMeterElement/[object HTMLMeterElement]/[objectHTMLMeterElement] nav=HTMLElement/[object HTMLElement]/[objectHTMLElement] noscript=HTMLElement/[object HTMLElement]/[objectHTMLElement] object=HTMLObjectElement/[object HTMLObjectElement]/[objectHTMLObjectElement] ol=HTMLOListElement/[object HTMLOListElement]/[objectHTMLOListElement] optgroup=HTMLOptGroupElement/[object HTMLOptGroupElement]/[objectHTMLOptGroupElement] option=HTMLOptionElement/[object HTMLOptionElement]/[objectHTMLOptionElement] output=HTMLOutputElement/[object HTMLOutputElement]/[objectHTMLOutputElement] p=HTMLParagraphElement/[object HTMLParagraphElement]/[objectHTMLParagraphElement] param=HTMLParamElement/[object HTMLParamElement]/[objectHTMLParamElement] picture=HTMLPictureElement/[object HTMLPictureElement]/[objectHTMLPictureElement]""#,
            ),
            (
                r"E(() => ['pre', 'progress', 'q', 'rp', 'rt', 'ruby', 's', 'samp', 'script', 'search', 'section', 'select', 'slot', 'small', 'source', 'span', 'strong', 'style', 'sub', 'summary', 'sup', 'table', 'tbody', 'td', 'template'].map((t) => { const e = document.createElement(t); return t + '=' + e.constructor.name + '/' + Object.prototype.toString.call(e) + '/' + C(e).split(' ').slice(0, 2).join(''); }).join(' '))",
                r#""pre=HTMLPreElement/[object HTMLPreElement]/[objectHTMLPreElement] progress=HTMLProgressElement/[object HTMLProgressElement]/[objectHTMLProgressElement] q=HTMLQuoteElement/[object HTMLQuoteElement]/[objectHTMLQuoteElement] rp=HTMLElement/[object HTMLElement]/[objectHTMLElement] rt=HTMLElement/[object HTMLElement]/[objectHTMLElement] ruby=HTMLElement/[object HTMLElement]/[objectHTMLElement] s=HTMLElement/[object HTMLElement]/[objectHTMLElement] samp=HTMLElement/[object HTMLElement]/[objectHTMLElement] script=HTMLScriptElement/[object HTMLScriptElement]/[objectHTMLScriptElement] search=HTMLElement/[object HTMLElement]/[objectHTMLElement] section=HTMLElement/[object HTMLElement]/[objectHTMLElement] select=HTMLSelectElement/[object HTMLSelectElement]/[objectHTMLSelectElement] slot=HTMLSlotElement/[object HTMLSlotElement]/[objectHTMLSlotElement] small=HTMLElement/[object HTMLElement]/[objectHTMLElement] source=HTMLSourceElement/[object HTMLSourceElement]/[objectHTMLSourceElement] span=HTMLSpanElement/[object HTMLSpanElement]/[objectHTMLSpanElement] strong=HTMLElement/[object HTMLElement]/[objectHTMLElement] style=HTMLStyleElement/[object HTMLStyleElement]/[objectHTMLStyleElement] sub=HTMLElement/[object HTMLElement]/[objectHTMLElement] summary=HTMLElement/[object HTMLElement]/[objectHTMLElement] sup=HTMLElement/[object HTMLElement]/[objectHTMLElement] table=HTMLTableElement/[object HTMLTableElement]/[objectHTMLTableElement] tbody=HTMLTableSectionElement/[object HTMLTableSectionElement]/[objectHTMLTableSectionElement] td=HTMLTableCellElement/[object HTMLTableCellElement]/[objectHTMLTableCellElement] template=HTMLTemplateElement/[object HTMLTemplateElement]/[objectHTMLTemplateElement]""#,
            ),
            (
                r"E(() => ['textarea', 'tfoot', 'th', 'thead', 'time', 'title', 'tr', 'track', 'u', 'ul', 'var', 'video', 'wbr', 'acronym', 'applet', 'basefont', 'bgsound', 'big', 'blink', 'center', 'command', 'content', 'dir', 'font', 'frame'].map((t) => { const e = document.createElement(t); return t + '=' + e.constructor.name + '/' + Object.prototype.toString.call(e) + '/' + C(e).split(' ').slice(0, 2).join(''); }).join(' '))",
                r#""textarea=HTMLTextAreaElement/[object HTMLTextAreaElement]/[objectHTMLTextAreaElement] tfoot=HTMLTableSectionElement/[object HTMLTableSectionElement]/[objectHTMLTableSectionElement] th=HTMLTableCellElement/[object HTMLTableCellElement]/[objectHTMLTableCellElement] thead=HTMLTableSectionElement/[object HTMLTableSectionElement]/[objectHTMLTableSectionElement] time=HTMLTimeElement/[object HTMLTimeElement]/[objectHTMLTimeElement] title=HTMLTitleElement/[object HTMLTitleElement]/[objectHTMLTitleElement] tr=HTMLTableRowElement/[object HTMLTableRowElement]/[objectHTMLTableRowElement] track=HTMLTrackElement/[object HTMLTrackElement]/[objectHTMLTrackElement] u=HTMLElement/[object HTMLElement]/[objectHTMLElement] ul=HTMLUListElement/[object HTMLUListElement]/[objectHTMLUListElement] var=HTMLElement/[object HTMLElement]/[objectHTMLElement] video=HTMLVideoElement/[object HTMLVideoElement]/[objectHTMLVideoElement] wbr=HTMLElement/[object HTMLElement]/[objectHTMLElement] acronym=HTMLElement/[object HTMLElement]/[objectHTMLElement] applet=HTMLUnknownElement/[object HTMLUnknownElement]/[objectHTMLUnknownElement] basefont=HTMLElement/[object HTMLElement]/[objectHTMLElement] bgsound=HTMLUnknownElement/[object HTMLUnknownElement]/[objectHTMLUnknownElement] big=HTMLElement/[object HTMLElement]/[objectHTMLElement] blink=HTMLUnknownElement/[object HTMLUnknownElement]/[objectHTMLUnknownElement] center=HTMLElement/[object HTMLElement]/[objectHTMLElement] command=HTMLUnknownElement/[object HTMLUnknownElement]/[objectHTMLUnknownElement] content=HTMLUnknownElement/[object HTMLUnknownElement]/[objectHTMLUnknownElement] dir=HTMLDirectoryElement/[object HTMLDirectoryElement]/[objectHTMLDirectoryElement] font=HTMLFontElement/[object HTMLFontElement]/[objectHTMLFontElement] frame=HTMLFrameElement/[object HTMLFrameElement]/[objectHTMLFrameElement]""#,
            ),
            (
                r"E(() => ['frameset', 'image', 'isindex', 'keygen', 'listing', 'marquee', 'menuitem', 'multicol', 'nextid', 'nobr', 'noembed', 'noframes', 'plaintext', 'rb', 'rtc', 'shadow', 'spacer', 'strike', 'tt', 'xmp', 'selectedcontent', 'math', 'foo-bar', 'x-y', 'a.b'].map((t) => { const e = document.createElement(t); return t + '=' + e.constructor.name + '/' + Object.prototype.toString.call(e) + '/' + C(e).split(' ').slice(0, 2).join(''); }).join(' '))",
                r#""frameset=HTMLFrameSetElement/[object HTMLFrameSetElement]/[objectHTMLFrameSetElement] image=HTMLUnknownElement/[object HTMLUnknownElement]/[objectHTMLUnknownElement] isindex=HTMLUnknownElement/[object HTMLUnknownElement]/[objectHTMLUnknownElement] keygen=HTMLUnknownElement/[object HTMLUnknownElement]/[objectHTMLUnknownElement] listing=HTMLPreElement/[object HTMLPreElement]/[objectHTMLPreElement] marquee=HTMLMarqueeElement/[object HTMLMarqueeElement]/[objectHTMLMarqueeElement] menuitem=HTMLUnknownElement/[object HTMLUnknownElement]/[objectHTMLUnknownElement] multicol=HTMLUnknownElement/[object HTMLUnknownElement]/[objectHTMLUnknownElement] nextid=HTMLUnknownElement/[object HTMLUnknownElement]/[objectHTMLUnknownElement] nobr=HTMLElement/[object HTMLElement]/[objectHTMLElement] noembed=HTMLElement/[object HTMLElement]/[objectHTMLElement] noframes=HTMLElement/[object HTMLElement]/[objectHTMLElement] plaintext=HTMLElement/[object HTMLElement]/[objectHTMLElement] rb=HTMLElement/[object HTMLElement]/[objectHTMLElement] rtc=HTMLElement/[object HTMLElement]/[objectHTMLElement] shadow=HTMLUnknownElement/[object HTMLUnknownElement]/[objectHTMLUnknownElement] spacer=HTMLUnknownElement/[object HTMLUnknownElement]/[objectHTMLUnknownElement] strike=HTMLElement/[object HTMLElement]/[objectHTMLElement] tt=HTMLElement/[object HTMLElement]/[objectHTMLElement] xmp=HTMLPreElement/[object HTMLPreElement]/[objectHTMLPreElement] selectedcontent=HTMLSelectedContentElement/[object HTMLSelectedContentElement]/[objectHTMLSelectedContentElement] math=HTMLUnknownElement/[object HTMLUnknownElement]/[objectHTMLUnknownElement] foo-bar=HTMLElement/[object HTMLElement]/[objectHTMLElement] x-y=HTMLElement/[object HTMLElement]/[objectHTMLElement] a.b=HTMLUnknownElement/[object HTMLUnknownElement]/[objectHTMLUnknownElement]""#,
            ),
            (
                r"E(() => C(document.createElement('div')))",
                r#""[object HTMLDivElement] [object HTMLElement] [object Element] [object Node] [object EventTarget] [object Object]""#,
            ),
            (
                r"E(() => C(document.createElement('input')))",
                r#""[object HTMLInputElement] [object HTMLElement] [object Element] [object Node] [object EventTarget] [object Object]""#,
            ),
            (
                r"E(() => C(document.createElement('td')))",
                r#""[object HTMLTableCellElement] [object HTMLElement] [object Element] [object Node] [object EventTarget] [object Object]""#,
            ),
            (
                r"E(() => C(document.createElement('video')))",
                r#""[object HTMLVideoElement] [object HTMLMediaElement] [object HTMLElement] [object Element] [object Node] [object EventTarget] [object Object]""#,
            ),
            (
                r"E(() => C(document.createElement('foo')))",
                r#""[object HTMLUnknownElement] [object HTMLElement] [object Element] [object Node] [object EventTarget] [object Object]""#,
            ),
            (
                r"E(() => C(document.createElement('foo-bar')))",
                r#""[object HTMLElement] [object Element] [object Node] [object EventTarget] [object Object]""#,
            ),
            (
                r"E(() => C(document))",
                r#""[object HTMLDocument] [object Document] [object Node] [object EventTarget] [object Object]""#,
            ),
            (
                r"E(() => C(document.implementation.createHTMLDocument('x')))",
                r#""[object HTMLDocument] [object Document] [object Node] [object EventTarget] [object Object]""#,
            ),
            (
                r"E(() => [document.constructor.name, Object.prototype.toString.call(document), document instanceof Document, document instanceof HTMLDocument, Object.getPrototypeOf(HTMLDocument) === Document])",
                r#""HTMLDocument,[object HTMLDocument],true,true,true""#,
            ),
            (
                r"E(() => [typeof HTMLDocument, HTMLDocument.name, HTMLDocument.length, Object.prototype.toString.call(HTMLDocument.prototype)])",
                r#""function,HTMLDocument,0,[object HTMLDocument]""#,
            ),
            (
                r"E(() => document.createElement('é').constructor.name)",
                r#""HTMLUnknownElement""#,
            ),
            (
                r"E(() => document.createElement('aé').constructor.name)",
                r#""HTMLUnknownElement""#,
            ),
            (
                r"E(() => document.createElement('DIV').constructor.name)",
                r#""HTMLDivElement""#,
            ),
            (
                r"E(() => document.createElementNS('http://www.w3.org/1999/xhtml', 'DIV').constructor.name)",
                r#""HTMLUnknownElement""#,
            ),
            (
                r"E(() => document.createElementNS('http://www.w3.org/1999/xhtml', 'div').constructor.name)",
                r#""HTMLDivElement""#,
            ),
            (
                r"E(() => document.createElementNS('http://www.w3.org/1999/xhtml', 'x-y').constructor.name)",
                r#""HTMLElement""#,
            ),
            (
                r"E(() => document.createElementNS('http://www.w3.org/1999/xhtml', 'font-face').constructor.name)",
                r#""HTMLUnknownElement""#,
            ),
            (
                r"E(() => document.createElement('annotation-xml').constructor.name)",
                r#""HTMLUnknownElement""#,
            ),
            (
                r"E(() => document.createElement('color-profile').constructor.name)",
                r#""HTMLUnknownElement""#,
            ),
            (
                r"E(() => document.createElement('x-é').constructor.name)",
                r#""HTMLElement""#,
            ),
            (
                r"E(() => document.createElement('-x').constructor.name)",
                r#""DOMException InvalidCharacterError: Failed to execute 'createElement' on 'Document': The tag name provided ('-x') is not a valid name.""#,
            ),
            (
                r"E(() => document.createElement('x-').constructor.name)",
                r#""HTMLElement""#,
            ),
            (
                r"E(() => document.createElement('X-Y').constructor.name)",
                r#""HTMLElement""#,
            ),
            (
                r"E(() => document.createElement('x-y.z_w').constructor.name)",
                r#""HTMLElement""#,
            ),
            (
                r"E(() => document.createElement('x-😀').constructor.name)",
                r#""HTMLElement""#,
            ),
            (
                r"E(() => document.getElementById('a').constructor.name + ' ' + document.getElementById('p').constructor.name + ' ' + document.head.constructor.name + ' ' + document.body.constructor.name + ' ' + document.documentElement.constructor.name)",
                r#""HTMLDivElement HTMLParagraphElement HTMLHeadElement HTMLBodyElement HTMLHtmlElement""#,
            ),
            (
                r"E(() => [document.createElement('div') instanceof HTMLDivElement, document.createElement('div') instanceof HTMLSpanElement, document.createElement('div') instanceof HTMLElement, document.createElement('div') instanceof Element, document.createElement('em') instanceof HTMLUnknownElement, document.createElement('foo') instanceof HTMLUnknownElement])",
                r#""true,false,true,true,false,true""#,
            ),
            (
                r"E(() => [Object.getPrototypeOf(HTMLDivElement) === HTMLElement, Object.getPrototypeOf(HTMLVideoElement) === HTMLMediaElement, Object.getPrototypeOf(HTMLTableSectionElement) === HTMLElement, Object.getPrototypeOf(HTMLOptionsCollection) === HTMLCollection, Object.getPrototypeOf(HTMLFormControlsCollection) === HTMLCollection, Object.getPrototypeOf(RadioNodeList) === NodeList])",
                r#""true,true,true,true,true,true""#,
            ),
            (
                r"E(() => [HTMLDivElement.name, HTMLDivElement.length, HTMLTableCellElement.name, HTMLTableCaptionElement.name, HTMLDListElement.name, HTMLOListElement.name, HTMLUListElement.name, HTMLLIElement.name, HTMLHRElement.name, HTMLBRElement.name, HTMLIFrameElement.name, HTMLTextAreaElement.name, HTMLFieldSetElement.name, HTMLOptGroupElement.name, HTMLDataListElement.name])",
                r#""HTMLDivElement,0,HTMLTableCellElement,HTMLTableCaptionElement,HTMLDListElement,HTMLOListElement,HTMLUListElement,HTMLLIElement,HTMLHRElement,HTMLBRElement,HTMLIFrameElement,HTMLTextAreaElement,HTMLFieldSetElement,HTMLOptGroupElement,HTMLDataListElement""#,
            ),
            (
                r"E(() => Object.prototype.toString.call(HTMLDivElement.prototype) + Object.prototype.toString.call(HTMLOptionsCollection.prototype) + Object.prototype.toString.call(RadioNodeList.prototype) + Object.prototype.toString.call(HTMLFormControlsCollection.prototype))",
                r#""[object HTMLDivElement][object HTMLOptionsCollection][object RadioNodeList][object HTMLFormControlsCollection]""#,
            ),
            (
                r"E(() => Object.getOwnPropertyNames(HTMLSpanElement.prototype).join())",
                r#""constructor""#,
            ),
            (
                r"E(() => Object.getOwnPropertyNames(HTMLUnknownElement.prototype).join())",
                r#""constructor""#,
            ),
            (
                r"E(() => Object.getOwnPropertyNames(HTMLQuoteElement.prototype).join())",
                r#""cite,constructor""#,
            ),
            (
                r"E(() => Object.getOwnPropertyNames(HTMLTimeElement.prototype).join())",
                r#""dateTime,constructor""#,
            ),
        ],
    },
    Case {
        name: "chains-02",
        html: PAGE,
        queries: &[
            (
                r"E(() => Object.getOwnPropertyNames(HTMLLabelElement.prototype).join())",
                r#""form,htmlFor,control,constructor""#,
            ),
            (
                r"E(() => Object.getOwnPropertyNames(HTMLOptGroupElement.prototype).join())",
                r#""disabled,label,constructor""#,
            ),
            (
                r"E(() => Object.getOwnPropertyNames(HTMLOptionsCollection.prototype).join())",
                r#""length,selectedIndex,add,remove,constructor""#,
            ),
            (
                r"E(() => Object.getOwnPropertyNames(HTMLFormControlsCollection.prototype).join())",
                r#""namedItem,constructor""#,
            ),
            (
                r"E(() => Object.getOwnPropertyNames(RadioNodeList.prototype).join())",
                r#""value,constructor""#,
            ),
            (
                r"E(() => Object.getOwnPropertyNames(HTMLDocument.prototype).join())",
                r#""constructor""#,
            ),
        ],
    },
    Case {
        name: "constructors-01",
        html: PAGE,
        queries: &[
            (
                r"E(() => new HTMLDocument())",
                r#""TypeError: Failed to construct 'HTMLDocument': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLDocument())",
                r#""TypeError: Illegal constructor""#,
            ),
            (
                r"E(() => new HTMLUnknownElement())",
                r#""TypeError: Failed to construct 'HTMLUnknownElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLUnknownElement())",
                r#""TypeError: Illegal constructor""#,
            ),
            (
                r"E(() => new HTMLHtmlElement())",
                r#""TypeError: Failed to construct 'HTMLHtmlElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLHtmlElement())",
                r#""TypeError: Failed to construct 'HTMLHtmlElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLHeadElement())",
                r#""TypeError: Failed to construct 'HTMLHeadElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLHeadElement())",
                r#""TypeError: Failed to construct 'HTMLHeadElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLTitleElement())",
                r#""TypeError: Failed to construct 'HTMLTitleElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLTitleElement())",
                r#""TypeError: Failed to construct 'HTMLTitleElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLBaseElement())",
                r#""TypeError: Failed to construct 'HTMLBaseElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLBaseElement())",
                r#""TypeError: Failed to construct 'HTMLBaseElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLLinkElement())",
                r#""TypeError: Failed to construct 'HTMLLinkElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLLinkElement())",
                r#""TypeError: Failed to construct 'HTMLLinkElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLMetaElement())",
                r#""TypeError: Failed to construct 'HTMLMetaElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLMetaElement())",
                r#""TypeError: Failed to construct 'HTMLMetaElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLStyleElement())",
                r#""TypeError: Failed to construct 'HTMLStyleElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLStyleElement())",
                r#""TypeError: Failed to construct 'HTMLStyleElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLBodyElement())",
                r#""TypeError: Failed to construct 'HTMLBodyElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLBodyElement())",
                r#""TypeError: Failed to construct 'HTMLBodyElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLHeadingElement())",
                r#""TypeError: Failed to construct 'HTMLHeadingElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLHeadingElement())",
                r#""TypeError: Failed to construct 'HTMLHeadingElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLParagraphElement())",
                r#""TypeError: Failed to construct 'HTMLParagraphElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLParagraphElement())",
                r#""TypeError: Failed to construct 'HTMLParagraphElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLHRElement())",
                r#""TypeError: Failed to construct 'HTMLHRElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLHRElement())",
                r#""TypeError: Failed to construct 'HTMLHRElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLPreElement())",
                r#""TypeError: Failed to construct 'HTMLPreElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLPreElement())",
                r#""TypeError: Failed to construct 'HTMLPreElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLQuoteElement())",
                r#""TypeError: Failed to construct 'HTMLQuoteElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLQuoteElement())",
                r#""TypeError: Failed to construct 'HTMLQuoteElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLOListElement())",
                r#""TypeError: Failed to construct 'HTMLOListElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLOListElement())",
                r#""TypeError: Failed to construct 'HTMLOListElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLUListElement())",
                r#""TypeError: Failed to construct 'HTMLUListElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLUListElement())",
                r#""TypeError: Failed to construct 'HTMLUListElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLMenuElement())",
                r#""TypeError: Failed to construct 'HTMLMenuElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLMenuElement())",
                r#""TypeError: Failed to construct 'HTMLMenuElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLLIElement())",
                r#""TypeError: Failed to construct 'HTMLLIElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLLIElement())",
                r#""TypeError: Failed to construct 'HTMLLIElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLDListElement())",
                r#""TypeError: Failed to construct 'HTMLDListElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLDListElement())",
                r#""TypeError: Failed to construct 'HTMLDListElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
        ],
    },
    Case {
        name: "constructors-02",
        html: PAGE,
        queries: &[
            (
                r"E(() => new HTMLDivElement())",
                r#""TypeError: Failed to construct 'HTMLDivElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLDivElement())",
                r#""TypeError: Failed to construct 'HTMLDivElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLAnchorElement())",
                r#""TypeError: Failed to construct 'HTMLAnchorElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLAnchorElement())",
                r#""TypeError: Failed to construct 'HTMLAnchorElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLDataElement())",
                r#""TypeError: Failed to construct 'HTMLDataElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLDataElement())",
                r#""TypeError: Failed to construct 'HTMLDataElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLTimeElement())",
                r#""TypeError: Failed to construct 'HTMLTimeElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLTimeElement())",
                r#""TypeError: Failed to construct 'HTMLTimeElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLSpanElement())",
                r#""TypeError: Failed to construct 'HTMLSpanElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLSpanElement())",
                r#""TypeError: Failed to construct 'HTMLSpanElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLBRElement())",
                r#""TypeError: Failed to construct 'HTMLBRElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLBRElement())",
                r#""TypeError: Failed to construct 'HTMLBRElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLModElement())",
                r#""TypeError: Failed to construct 'HTMLModElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLModElement())",
                r#""TypeError: Failed to construct 'HTMLModElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLPictureElement())",
                r#""TypeError: Failed to construct 'HTMLPictureElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLPictureElement())",
                r#""TypeError: Failed to construct 'HTMLPictureElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLSourceElement())",
                r#""TypeError: Failed to construct 'HTMLSourceElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLSourceElement())",
                r#""TypeError: Failed to construct 'HTMLSourceElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLImageElement())",
                r#""TypeError: Failed to construct 'HTMLImageElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLImageElement())",
                r#""TypeError: Failed to construct 'HTMLImageElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLIFrameElement())",
                r#""TypeError: Failed to construct 'HTMLIFrameElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLIFrameElement())",
                r#""TypeError: Failed to construct 'HTMLIFrameElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLEmbedElement())",
                r#""TypeError: Failed to construct 'HTMLEmbedElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLEmbedElement())",
                r#""TypeError: Failed to construct 'HTMLEmbedElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLObjectElement())",
                r#""TypeError: Failed to construct 'HTMLObjectElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLObjectElement())",
                r#""TypeError: Failed to construct 'HTMLObjectElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLMediaElement())",
                r#""TypeError: Failed to construct 'HTMLMediaElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLMediaElement())",
                r#""TypeError: Illegal constructor""#,
            ),
            (
                r"E(() => new HTMLVideoElement())",
                r#""TypeError: Failed to construct 'HTMLVideoElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLVideoElement())",
                r#""TypeError: Failed to construct 'HTMLVideoElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLAudioElement())",
                r#""TypeError: Failed to construct 'HTMLAudioElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLAudioElement())",
                r#""TypeError: Failed to construct 'HTMLAudioElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLTrackElement())",
                r#""TypeError: Failed to construct 'HTMLTrackElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLTrackElement())",
                r#""TypeError: Failed to construct 'HTMLTrackElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLMapElement())",
                r#""TypeError: Failed to construct 'HTMLMapElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLMapElement())",
                r#""TypeError: Failed to construct 'HTMLMapElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLAreaElement())",
                r#""TypeError: Failed to construct 'HTMLAreaElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLAreaElement())",
                r#""TypeError: Failed to construct 'HTMLAreaElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLTableElement())",
                r#""TypeError: Failed to construct 'HTMLTableElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLTableElement())",
                r#""TypeError: Failed to construct 'HTMLTableElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
        ],
    },
    Case {
        name: "constructors-03",
        html: PAGE,
        queries: &[
            (
                r"E(() => new HTMLTableCaptionElement())",
                r#""TypeError: Failed to construct 'HTMLTableCaptionElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLTableCaptionElement())",
                r#""TypeError: Failed to construct 'HTMLTableCaptionElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLTableColElement())",
                r#""TypeError: Failed to construct 'HTMLTableColElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLTableColElement())",
                r#""TypeError: Failed to construct 'HTMLTableColElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLTableSectionElement())",
                r#""TypeError: Failed to construct 'HTMLTableSectionElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLTableSectionElement())",
                r#""TypeError: Failed to construct 'HTMLTableSectionElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLTableRowElement())",
                r#""TypeError: Failed to construct 'HTMLTableRowElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLTableRowElement())",
                r#""TypeError: Failed to construct 'HTMLTableRowElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLTableCellElement())",
                r#""TypeError: Failed to construct 'HTMLTableCellElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLTableCellElement())",
                r#""TypeError: Failed to construct 'HTMLTableCellElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLFormElement())",
                r#""TypeError: Failed to construct 'HTMLFormElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLFormElement())",
                r#""TypeError: Failed to construct 'HTMLFormElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLLabelElement())",
                r#""TypeError: Failed to construct 'HTMLLabelElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLLabelElement())",
                r#""TypeError: Failed to construct 'HTMLLabelElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLInputElement())",
                r#""TypeError: Failed to construct 'HTMLInputElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLInputElement())",
                r#""TypeError: Failed to construct 'HTMLInputElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLButtonElement())",
                r#""TypeError: Failed to construct 'HTMLButtonElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLButtonElement())",
                r#""TypeError: Failed to construct 'HTMLButtonElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLSelectElement())",
                r#""TypeError: Failed to construct 'HTMLSelectElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLSelectElement())",
                r#""TypeError: Failed to construct 'HTMLSelectElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLDataListElement())",
                r#""TypeError: Failed to construct 'HTMLDataListElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLDataListElement())",
                r#""TypeError: Failed to construct 'HTMLDataListElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLOptGroupElement())",
                r#""TypeError: Failed to construct 'HTMLOptGroupElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLOptGroupElement())",
                r#""TypeError: Failed to construct 'HTMLOptGroupElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLOptionElement())",
                r#""TypeError: Failed to construct 'HTMLOptionElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLOptionElement())",
                r#""TypeError: Failed to construct 'HTMLOptionElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLTextAreaElement())",
                r#""TypeError: Failed to construct 'HTMLTextAreaElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLTextAreaElement())",
                r#""TypeError: Failed to construct 'HTMLTextAreaElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLOutputElement())",
                r#""TypeError: Failed to construct 'HTMLOutputElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLOutputElement())",
                r#""TypeError: Failed to construct 'HTMLOutputElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLProgressElement())",
                r#""TypeError: Failed to construct 'HTMLProgressElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLProgressElement())",
                r#""TypeError: Failed to construct 'HTMLProgressElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLMeterElement())",
                r#""TypeError: Failed to construct 'HTMLMeterElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLMeterElement())",
                r#""TypeError: Failed to construct 'HTMLMeterElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLFieldSetElement())",
                r#""TypeError: Failed to construct 'HTMLFieldSetElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLFieldSetElement())",
                r#""TypeError: Failed to construct 'HTMLFieldSetElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLLegendElement())",
                r#""TypeError: Failed to construct 'HTMLLegendElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLLegendElement())",
                r#""TypeError: Failed to construct 'HTMLLegendElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLSelectedContentElement())",
                r#""TypeError: Failed to construct 'HTMLSelectedContentElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLSelectedContentElement())",
                r#""TypeError: Failed to construct 'HTMLSelectedContentElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
        ],
    },
    Case {
        name: "constructors-04",
        html: PAGE,
        queries: &[
            (
                r"E(() => new HTMLDetailsElement())",
                r#""TypeError: Failed to construct 'HTMLDetailsElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLDetailsElement())",
                r#""TypeError: Failed to construct 'HTMLDetailsElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLDialogElement())",
                r#""TypeError: Failed to construct 'HTMLDialogElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLDialogElement())",
                r#""TypeError: Failed to construct 'HTMLDialogElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLFormControlsCollection())",
                r#""TypeError: Failed to construct 'HTMLFormControlsCollection': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLFormControlsCollection())",
                r#""TypeError: Illegal constructor""#,
            ),
            (
                r"E(() => new RadioNodeList())",
                r#""TypeError: Failed to construct 'RadioNodeList': Illegal constructor""#,
            ),
            (
                r"E(() => RadioNodeList())",
                r#""TypeError: Illegal constructor""#,
            ),
            (
                r"E(() => new HTMLOptionsCollection())",
                r#""TypeError: Failed to construct 'HTMLOptionsCollection': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLOptionsCollection())",
                r#""TypeError: Illegal constructor""#,
            ),
            (
                r"E(() => new HTMLSlotElement())",
                r#""TypeError: Failed to construct 'HTMLSlotElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLSlotElement())",
                r#""TypeError: Failed to construct 'HTMLSlotElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLCanvasElement())",
                r#""TypeError: Failed to construct 'HTMLCanvasElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLCanvasElement())",
                r#""TypeError: Failed to construct 'HTMLCanvasElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLMarqueeElement())",
                r#""TypeError: Failed to construct 'HTMLMarqueeElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLMarqueeElement())",
                r#""TypeError: Failed to construct 'HTMLMarqueeElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLFrameSetElement())",
                r#""TypeError: Failed to construct 'HTMLFrameSetElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLFrameSetElement())",
                r#""TypeError: Failed to construct 'HTMLFrameSetElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLFrameElement())",
                r#""TypeError: Failed to construct 'HTMLFrameElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLFrameElement())",
                r#""TypeError: Failed to construct 'HTMLFrameElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLDirectoryElement())",
                r#""TypeError: Failed to construct 'HTMLDirectoryElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLDirectoryElement())",
                r#""TypeError: Failed to construct 'HTMLDirectoryElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLFontElement())",
                r#""TypeError: Failed to construct 'HTMLFontElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLFontElement())",
                r#""TypeError: Failed to construct 'HTMLFontElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLParamElement())",
                r#""TypeError: Failed to construct 'HTMLParamElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLParamElement())",
                r#""TypeError: Failed to construct 'HTMLParamElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLTemplateElement())",
                r#""TypeError: Failed to construct 'HTMLTemplateElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLTemplateElement())",
                r#""TypeError: Failed to construct 'HTMLTemplateElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => new HTMLScriptElement())",
                r#""TypeError: Failed to construct 'HTMLScriptElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLScriptElement())",
                r#""TypeError: Failed to construct 'HTMLScriptElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.""#,
            ),
            (
                r"E(() => Reflect.construct(HTMLDivElement, [], Object))",
                r#""TypeError: Failed to construct 'HTMLDivElement': Illegal constructor""#,
            ),
            (
                r"E(() => new (class X extends HTMLDivElement {})())",
                r#""TypeError: Failed to construct 'HTMLDivElement': Illegal constructor""#,
            ),
            (
                r"E(() => HTMLDivElement.prototype.title)",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => HTMLDivElement.prototype.hidden)",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'title').get.call(document.createElement('div')))",
                r#""""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'title').get.call(document))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'title').get.call({}))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').get.call(document.createElement('div')))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(HTMLAnchorElement.prototype, 'href').get.call(document.createElementNS('http://www.w3.org/2000/svg', 'a')))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => HTMLAnchorElement.prototype.toString.call(document.createElement('a')))",
                r#""""#,
            ),
        ],
    },
    Case {
        name: "constructors-05",
        html: PAGE,
        queries: &[
            (
                r"E(() => HTMLAnchorElement.prototype.toString.call(document.createElement('div')))",
                r#""TypeError: Illegal invocation""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call({}, 'x'))",
                r#""TypeError: Illegal invocation""#,
            ),
        ],
    },
    Case {
        name: "html-element-01",
        html: PAGE,
        queries: &[
            (
                r"E(() => { const e = document.createElement('abbr'); e.hidden = true; return [e.hidden, e.getAttribute('hidden')]; })",
                r#""true,""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.hidden = 'until-found'; return [e.hidden, e.getAttribute('hidden')]; })",
                r#""until-found,until-found""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.hidden = 'UNTIL-found'; return [e.hidden, e.getAttribute('hidden')]; })",
                r#""until-found,until-found""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.hidden = 'x'; return [e.hidden, e.getAttribute('hidden')]; })",
                r#""true,""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.hidden = ''; return [e.hidden, e.getAttribute('hidden')]; })",
                r#""false,""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.hidden = 0; return [e.hidden, e.getAttribute('hidden')]; })",
                r#""false,""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.hidden = 2; return [e.hidden, e.getAttribute('hidden')]; })",
                r#""true,""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.hidden = NaN; return [e.hidden, e.getAttribute('hidden')]; })",
                r#""false,""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.hidden = null; return [e.hidden, e.getAttribute('hidden')]; })",
                r#""false,""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.hidden = undefined; return [e.hidden, e.getAttribute('hidden')]; })",
                r#""false,""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.hidden = {}; return [e.hidden, e.getAttribute('hidden')]; })",
                r#""true,""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.setAttribute('hidden', 'until-found'); const a = e.hidden; e.hidden = false; return [a, e.hidden, e.hasAttribute('hidden')]; })",
                r#""until-found,false,false""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.setAttribute('hidden', 'Until-Found'); return e.hidden; })",
                r#""until-found""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.setAttribute('hidden', ''); return e.hidden; })",
                r"true",
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.setAttribute('hidden', 'false'); return e.hidden; })",
                r"true",
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); return [e.translate, e.getAttribute('translate')]; })",
                r#""true,""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.translate = false; return [e.translate, e.getAttribute('translate')]; })",
                r#""false,no""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.translate = true; return [e.translate, e.getAttribute('translate')]; })",
                r#""true,yes""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.setAttribute('translate', 'NO'); return [e.translate]; })",
                r#""false""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.setAttribute('translate', ''); return [e.translate]; })",
                r#""true""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.setAttribute('translate', 'x'); return [e.translate]; })",
                r#""true""#,
            ),
            (
                r"E(() => { const p = document.createElement('div'); p.setAttribute('translate', 'no'); const e = p.appendChild(document.createElement('abbr')); const a = e.translate; e.setAttribute('translate', 'yes'); return [a, e.translate]; })",
                r#""false,true""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.setAttribute('translate', 'yes'); e.translate = 0; return e.getAttribute('translate'); })",
                r#""no""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); return [e.draggable, e.getAttribute('draggable')]; })",
                r#""false,""#,
            ),
            (
                r"E(() => { const e = document.createElement('img'); return [e.draggable]; })",
                r#""true""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); const a = e.draggable; e.href = 'x'; return [a, e.draggable]; })",
                r#""false,true""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.draggable = true; return [e.draggable, e.getAttribute('draggable')]; })",
                r#""true,true""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.draggable = false; return [e.draggable, e.getAttribute('draggable')]; })",
                r#""false,false""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.setAttribute('draggable', 'TRUE'); return [e.draggable]; })",
                r#""true""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.setAttribute('draggable', 'auto'); return [e.draggable]; })",
                r#""false""#,
            ),
            (
                r"E(() => { const e = document.createElement('img'); e.setAttribute('draggable', 'false'); return [e.draggable]; })",
                r#""false""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); return [e.spellcheck, e.getAttribute('spellcheck')]; })",
                r#""true,""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.spellcheck = false; return [e.spellcheck, e.getAttribute('spellcheck')]; })",
                r#""false,false""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.spellcheck = true; return [e.spellcheck, e.getAttribute('spellcheck')]; })",
                r#""true,true""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.setAttribute('spellcheck', ''); return [e.spellcheck]; })",
                r#""true""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.setAttribute('spellcheck', 'x'); return [e.spellcheck]; })",
                r#""true""#,
            ),
            (
                r"E(() => { const p = document.createElement('div'); p.setAttribute('spellcheck', 'false'); const e = p.appendChild(document.createElement('abbr')); return [e.spellcheck]; })",
                r#""false""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); return [e.tabIndex, e.getAttribute('tabindex')]; })",
                r#""-1,""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); const a = e.tabIndex; e.href = 'x'; return [a, e.tabIndex]; })",
                r#""0,0""#,
            ),
            (
                r"E(() => (['button', 'input', 'select', 'textarea', 'iframe', 'object', 'embed', 'summary', 'area', 'audio', 'video', 'div', 'span', 'body', 'label', 'details', 'img', 'canvas', 'dialog'].map((t) => t + document.createElement(t).tabIndex).join()))",
                r#""button0,input0,select0,textarea0,iframe0,object0,embed-1,summary-1,area0,audio0,video0,div-1,span-1,body-1,label-1,details-1,img-1,canvas-1,dialog-1""#,
            ),
        ],
    },
    Case {
        name: "html-element-02",
        html: PAGE,
        queries: &[
            (
                r"E(() => { const e = document.createElement('abbr'); e.contentEditable; e.setAttribute('tabindex', '3'); return [e.tabIndex]; })",
                r#""3""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.setAttribute('tabindex', ' 7x'); return [e.tabIndex]; })",
                r#""7""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.setAttribute('tabindex', 'x'); return [e.tabIndex]; })",
                r#""-1""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.setAttribute('tabindex', '99999999999'); return [e.tabIndex]; })",
                r#""-1""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.tabIndex = 5.7; return [e.tabIndex, e.getAttribute('tabindex')]; })",
                r#""5,5""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.tabIndex = -1; return [e.tabIndex, e.getAttribute('tabindex')]; })",
                r#""-1,-1""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.tabIndex = 2147483648; return [e.tabIndex, e.getAttribute('tabindex')]; })",
                r#""-2147483648,-2147483648""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.tabIndex = 'x'; return [e.tabIndex, e.getAttribute('tabindex')]; })",
                r#""0,0""#,
            ),
            (
                r"E(() => { const e = document.createElementNS('http://www.w3.org/2000/svg', 'g'); return [e.tabIndex]; })",
                r#""-1""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); return [e.autofocus, e.getAttribute('autofocus')]; })",
                r#""false,""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.autofocus = true; return [e.autofocus, e.getAttribute('autofocus')]; })",
                r#""true,""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.dataset.fooBar = 'x'; return [e.getAttribute('data-foo-bar'), e.dataset === e.dataset]; })",
                r#""x,true""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.setAttribute('inert', ''); return [e.inert]; })",
                r#""true""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); return [e.dir, e.lang, e.title, e.accessKey, e.autocapitalize]; })",
                r#"",,,,""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.dir = 'RTL'; e.lang = 'en'; e.title = 'T'; e.accessKey = 'k'; e.autocapitalize = 'words'; return [e.dir, e.getAttribute('dir'), e.lang, e.title, e.accessKey, e.autocapitalize, e.getAttribute('autocapitalize')]; })",
                r#""rtl,RTL,en,T,k,words,words""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.setAttribute('autocapitalize', 'off'); const a = e.autocapitalize; e.setAttribute('autocapitalize', 'ON'); const b = e.autocapitalize; e.setAttribute('autocapitalize', 'x'); return [a, b, e.autocapitalize]; })",
                r#""none,sentences,sentences""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.setAttribute('dir', 'x'); return e.dir; })",
                r#""""#,
            ),
            (
                r"E(() => { const e = document.createElement('abbr'); e.dir = 'x'; return [e.dir, e.getAttribute('dir')]; })",
                r#"",x""#,
            ),
            (
                r"E(() => (Object.keys(Object.getOwnPropertyDescriptors(HTMLElement.prototype)).filter((n) => ['title', 'lang', 'translate', 'dir', 'hidden', 'inert', 'accessKey', 'draggable', 'spellcheck', 'autocapitalize', 'dataset', 'autofocus', 'tabIndex', 'nonce'].includes(n)).join()))",
                r#""title,lang,translate,dir,hidden,inert,accessKey,draggable,spellcheck,autocapitalize,dataset,nonce,autofocus,tabIndex""#,
            ),
            (
                r"E(() => (Object.getOwnPropertyNames(HTMLElement.prototype).filter((n) => ['title', 'lang', 'translate', 'dir', 'hidden', 'inert', 'accessKey', 'draggable', 'spellcheck', 'autocapitalize', 'dataset', 'autofocus', 'tabIndex'].includes(n)).join()))",
                r#""title,lang,translate,dir,hidden,inert,accessKey,draggable,spellcheck,autocapitalize,dataset,autofocus,tabIndex""#,
            ),
        ],
    },
    Case {
        name: "simple-members-01",
        html: PAGE,
        queries: &[
            (
                r"E(() => { const e = document.createElement('title'); e.text = 'a b'; return [e.text, e.textContent, e.childNodes.length]; })",
                r#""a b,a b,1""#,
            ),
            (
                r"E(() => { const e = document.createElement('title'); e.innerHTML = 'x<b>y</b>'; return [e.text, e.textContent]; })",
                r#""x<b>y</b>,x<b>y</b>""#,
            ),
            (
                r"E(() => { const e = document.createElement('title'); e.text = ''; return [e.text, e.childNodes.length]; })",
                r#"",0""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); e.text = 'link'; e.href = 'q'; return [e.text, e.textContent, e.outerHTML]; })",
                r#""link,link,<a href=\"q\">link</a>""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); e.innerHTML = '<b>x</b> y'; return e.text; })",
                r#""x y""#,
            ),
            (
                r"E(() => { const e = document.createElement('base'); return [e.href === document.URL, e.target]; })",
                r#""true,""#,
            ),
            (
                r"E(() => { const e = document.createElement('base'); e.setAttribute('href', 'z/y'); return e.href.endsWith('/z/y') && e.href.startsWith(document.URL.slice(0, 8)); })",
                r"true",
            ),
            (
                r"E(() => { const e = document.createElement('base'); e.setAttribute('href', 'http://[bad'); return e.href; })",
                r#""http://[bad""#,
            ),
            (
                r"E(() => { const e = document.createElement('base'); e.href = '/abs'; return [e.getAttribute('href'), e.href]; })",
                r#""/abs,file:///abs""#,
            ),
            (
                r"E(() => document.querySelector('base').href)",
                r#""http://x.test/d/e.html""#,
            ),
            (
                r"E(() => { const e = document.createElement('img'); return [e.complete, e.naturalWidth, e.naturalHeight, e.currentSrc, e.width, e.height, e.alt, e.src]; })",
                r#""true,0,0,,0,0,,""#,
            ),
            (
                r"E(() => { const e = document.createElement('img'); e.width = 10; e.height = 20; return [e.width, e.height, e.getAttribute('width'), e.getAttribute('height')]; })",
                r#""10,20,10,20""#,
            ),
            (
                r"E(() => { const e = document.createElement('img'); e.setAttribute('width', '12x'); return [e.width]; })",
                r#""12""#,
            ),
            (
                r"E(() => { const e = document.createElement('img'); e.setAttribute('alt', 'q'); e.setAttribute('src', 'pic.png'); return [e.alt, e.src]; })",
                r#""q,http://x.test/d/pic.png""#,
            ),
            (
                r"E(() => { const e = document.createElement('form'); return [e.action === document.URL, e.method, e.enctype, e.encoding, e.target, e.name, e.acceptCharset, e.autocomplete, e.noValidate]; })",
                r#""true,get,application/x-www-form-urlencoded,application/x-www-form-urlencoded,,,,on,false""#,
            ),
            (
                r"E(() => { const e = document.createElement('form'); e.setAttribute('action', 'go'); return e.action; })",
                r#""http://x.test/d/go""#,
            ),
            (
                r"E(() => { const e = document.createElement('form'); e.setAttribute('action', ''); return e.action === document.URL; })",
                r"true",
            ),
            (
                r"E(() => { const e = document.createElement('form'); e.action = 'q'; return [e.getAttribute('action'), e.action]; })",
                r#""q,http://x.test/d/q""#,
            ),
            (
                r"E(() => { const e = document.createElement('button'); e.setAttribute('formaction', 'go'); return e.formAction; })",
                r#""http://x.test/d/go""#,
            ),
            (
                r"E(() => { const e = document.createElement('button'); return e.formAction === document.URL; })",
                r"true",
            ),
            (
                r"E(() => { const e = document.createElement('button'); e.formAction = 'q'; return [e.getAttribute('formaction'), e.formAction]; })",
                r#""q,http://x.test/d/q""#,
            ),
            (
                r"E(() => { const e = document.createElement('input'); e.setAttribute('formaction', 'go'); return e.formAction; })",
                r#""http://x.test/d/go""#,
            ),
        ],
    },
    Case {
        name: "hyperlink-01",
        html: PAGE,
        queries: &[
            (
                r"E(() => { const e = document.createElement('a'); return [e.href, e.origin, e.protocol, e.username, e.password, e.host, e.hostname, e.port, e.pathname, e.search, e.hash, String(e)]; })",
                r#"",,:,,,,,,,,,""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); e.setAttribute('href', 'https://u:p@h.test:8080/a/b?q=1#frag'); return [e.href, e.origin, e.protocol, e.username, e.password, e.host, e.hostname, e.port, e.pathname, e.search, e.hash, String(e)]; })",
                r#""https://u:p@h.test:8080/a/b?q=1#frag,https://h.test:8080,https:,u,p,h.test:8080,h.test,8080,/a/b,?q=1,#frag,https://u:p@h.test:8080/a/b?q=1#frag""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); e.setAttribute('href', 'rel/x'); return [e.href, e.origin, e.protocol, e.host, e.pathname]; })",
                r#""http://x.test/d/rel/x,http://x.test,http:,x.test,/d/rel/x""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); e.setAttribute('href', '?'); return [e.href, e.search, e.hash]; })",
                r#""http://x.test/d/e.html?,,""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); e.setAttribute('href', '#'); return [e.href, e.search, e.hash]; })",
                r#""http://x.test/d/e.html#,,""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); e.setAttribute('href', 'mailto:a@b.test'); return [e.href, e.origin, e.protocol, e.host, e.pathname, e.search]; })",
                r#""mailto:a@b.test,null,mailto:,,a@b.test,""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); e.setAttribute('href', 'javascript:void(0)'); return [e.href, e.origin, e.protocol, e.pathname]; })",
                r#""javascript:void(0),null,javascript:,void(0)""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); e.setAttribute('href', 'http://[bad'); return [e.href, e.origin, e.protocol, e.host, e.pathname]; })",
                r#""http://[bad,,:,,""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); e.setAttribute('href', 'file:///a/b'); return [e.href, e.origin, e.protocol, e.host, e.pathname]; })",
                r#""file:///a/b,file://,file:,,/a/b""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); e.setAttribute('href', 'http://a.test/p'); e.hash = 'z'; const a = e.href; e.hash = ''; const b = e.href; e.hash = '#y z'; return [a, b, e.href, e.getAttribute('href')]; })",
                r#""http://a.test/p#z,http://a.test/p,http://a.test/p#y%20z,http://a.test/p#y%20z""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); e.setAttribute('href', 'http://a.test/p'); e.search = 'q=1'; const a = e.href; e.search = '?r'; const b = e.href; e.search = ''; return [a, b, e.href]; })",
                r#""http://a.test/p?q=1,http://a.test/p?r,http://a.test/p""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); e.setAttribute('href', 'http://a.test/p?x'); e.pathname = 'new path'; const a = e.href; e.pathname = '/q/%41'; return [a, e.href]; })",
                r#""http://a.test/new%20path?x,http://a.test/q/%41?x""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); e.setAttribute('href', 'http://a.test/p'); e.port = '81'; const a = e.href; e.port = ''; const b = e.href; e.port = '99999'; const c = e.href; e.port = '8x'; return [a, b, c, e.href]; })",
                r#""http://a.test:81/p,http://a.test/p,http://a.test/p,http://a.test:8/p""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); e.setAttribute('href', 'http://a.test/p'); e.host = 'b.test:90'; const a = e.href; e.hostname = 'c.test'; const b = e.href; e.host = ''; return [a, b, e.href]; })",
                r#""http://b.test:90/p,http://c.test:90/p,http://c.test:90/p""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); e.setAttribute('href', 'http://a.test/p'); e.protocol = 'https'; const a = e.href; e.protocol = 'ftp:'; const b = e.href; e.protocol = 'mailto'; return [a, b, e.href]; })",
                r#""https://a.test/p,ftp://a.test/p,ftp://a.test/p""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); e.setAttribute('href', 'http://a.test/p'); e.username = 'u'; e.password = 'p w'; const a = e.href; e.username = ''; e.password = ''; return [a, e.href]; })",
                r#""http://u:p%20w@a.test/p,http://a.test/p""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); e.hash = 'x'; e.host = 'h'; e.pathname = '/p'; return [e.href, e.getAttribute('href')]; })",
                r#"",""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); e.href = 'http://q.test/'; return [e.href, e.getAttribute('href')]; })",
                r#""http://q.test/,http://q.test/""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); e.setAttribute('href', 'HTTP://A.TEST:80/%7Ea'); return [e.href, e.host, e.port, e.pathname]; })",
                r#""http://a.test/%7Ea,a.test,,/%7Ea""#,
            ),
            (
                r"E(() => { const e = document.createElement('area'); return [e.href, e.origin, e.protocol, e.username, e.password, e.host, e.hostname, e.port, e.pathname, e.search, e.hash, String(e)]; })",
                r#"",,:,,,,,,,,,""#,
            ),
            (
                r"E(() => { const e = document.createElement('area'); e.setAttribute('href', 'https://u:p@h.test:8080/a/b?q=1#frag'); return [e.href, e.origin, e.protocol, e.username, e.password, e.host, e.hostname, e.port, e.pathname, e.search, e.hash, String(e)]; })",
                r#""https://u:p@h.test:8080/a/b?q=1#frag,https://h.test:8080,https:,u,p,h.test:8080,h.test,8080,/a/b,?q=1,#frag,https://u:p@h.test:8080/a/b?q=1#frag""#,
            ),
            (
                r"E(() => { const e = document.createElement('area'); e.setAttribute('href', 'rel/x'); return [e.href, e.origin, e.protocol, e.host, e.pathname]; })",
                r#""http://x.test/d/rel/x,http://x.test,http:,x.test,/d/rel/x""#,
            ),
            (
                r"E(() => { const e = document.createElement('area'); e.setAttribute('href', '?'); return [e.href, e.search, e.hash]; })",
                r#""http://x.test/d/e.html?,,""#,
            ),
            (
                r"E(() => { const e = document.createElement('area'); e.setAttribute('href', '#'); return [e.href, e.search, e.hash]; })",
                r#""http://x.test/d/e.html#,,""#,
            ),
            (
                r"E(() => { const e = document.createElement('area'); e.setAttribute('href', 'mailto:a@b.test'); return [e.href, e.origin, e.protocol, e.host, e.pathname, e.search]; })",
                r#""mailto:a@b.test,null,mailto:,,a@b.test,""#,
            ),
            (
                r"E(() => { const e = document.createElement('area'); e.setAttribute('href', 'javascript:void(0)'); return [e.href, e.origin, e.protocol, e.pathname]; })",
                r#""javascript:void(0),null,javascript:,void(0)""#,
            ),
            (
                r"E(() => { const e = document.createElement('area'); e.setAttribute('href', 'http://[bad'); return [e.href, e.origin, e.protocol, e.host, e.pathname]; })",
                r#""http://[bad,,:,,""#,
            ),
            (
                r"E(() => { const e = document.createElement('area'); e.setAttribute('href', 'file:///a/b'); return [e.href, e.origin, e.protocol, e.host, e.pathname]; })",
                r#""file:///a/b,file://,file:,,/a/b""#,
            ),
            (
                r"E(() => { const e = document.createElement('area'); e.setAttribute('href', 'http://a.test/p'); e.hash = 'z'; const a = e.href; e.hash = ''; const b = e.href; e.hash = '#y z'; return [a, b, e.href, e.getAttribute('href')]; })",
                r#""http://a.test/p#z,http://a.test/p,http://a.test/p#y%20z,http://a.test/p#y%20z""#,
            ),
            (
                r"E(() => { const e = document.createElement('area'); e.setAttribute('href', 'http://a.test/p'); e.search = 'q=1'; const a = e.href; e.search = '?r'; const b = e.href; e.search = ''; return [a, b, e.href]; })",
                r#""http://a.test/p?q=1,http://a.test/p?r,http://a.test/p""#,
            ),
            (
                r"E(() => { const e = document.createElement('area'); e.setAttribute('href', 'http://a.test/p?x'); e.pathname = 'new path'; const a = e.href; e.pathname = '/q/%41'; return [a, e.href]; })",
                r#""http://a.test/new%20path?x,http://a.test/q/%41?x""#,
            ),
            (
                r"E(() => { const e = document.createElement('area'); e.setAttribute('href', 'http://a.test/p'); e.port = '81'; const a = e.href; e.port = ''; const b = e.href; e.port = '99999'; const c = e.href; e.port = '8x'; return [a, b, c, e.href]; })",
                r#""http://a.test:81/p,http://a.test/p,http://a.test/p,http://a.test:8/p""#,
            ),
            (
                r"E(() => { const e = document.createElement('area'); e.setAttribute('href', 'http://a.test/p'); e.host = 'b.test:90'; const a = e.href; e.hostname = 'c.test'; const b = e.href; e.host = ''; return [a, b, e.href]; })",
                r#""http://b.test:90/p,http://c.test:90/p,http://c.test:90/p""#,
            ),
            (
                r"E(() => { const e = document.createElement('area'); e.setAttribute('href', 'http://a.test/p'); e.protocol = 'https'; const a = e.href; e.protocol = 'ftp:'; const b = e.href; e.protocol = 'mailto'; return [a, b, e.href]; })",
                r#""https://a.test/p,ftp://a.test/p,ftp://a.test/p""#,
            ),
            (
                r"E(() => { const e = document.createElement('area'); e.setAttribute('href', 'http://a.test/p'); e.username = 'u'; e.password = 'p w'; const a = e.href; e.username = ''; e.password = ''; return [a, e.href]; })",
                r#""http://u:p%20w@a.test/p,http://a.test/p""#,
            ),
            (
                r"E(() => { const e = document.createElement('area'); e.hash = 'x'; e.host = 'h'; e.pathname = '/p'; return [e.href, e.getAttribute('href')]; })",
                r#"",""#,
            ),
            (
                r"E(() => { const e = document.createElement('area'); e.href = 'http://q.test/'; return [e.href, e.getAttribute('href')]; })",
                r#""http://q.test/,http://q.test/""#,
            ),
            (
                r"E(() => { const e = document.createElement('area'); e.setAttribute('href', 'HTTP://A.TEST:80/%7Ea'); return [e.href, e.host, e.port, e.pathname]; })",
                r#""http://a.test/%7Ea,a.test,,/%7Ea""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); e.setAttribute('href', 'http://a.test'); return [e.href, e.pathname]; })",
                r#""http://a.test/,/""#,
            ),
        ],
    },
    Case {
        name: "token-lists-01",
        html: PAGE,
        queries: &[
            (
                r"E(() => { const e = document.createElement('a'); e.rel = 'noopener  NoReferrer x'; return [e.relList.length, e.relList.value, e.relList.contains('noreferrer'), e.relList === e.relList, String(e.relList)]; })",
                r#""3,noopener  NoReferrer x,false,true,noopener  NoReferrer x""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); e.relList.add('noopener'); e.relList.add('x'); return [e.rel, e.getAttribute('rel')]; })",
                r#""noopener x,noopener x""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); e.relList = 'p q'; return [e.rel, e.relList.length]; })",
                r#""p q,2""#,
            ),
            (
                r"E(() => { const e = document.createElement('a'); return ['noreferrer', 'noopener', 'opener', 'nofollow', 'NOREFERRER', 'x'].map((t) => e.relList.supports(t)).join(); })",
                r#""true,true,true,false,true,false""#,
            ),
            (
                r"E(() => { const e = document.createElement('area'); return ['noreferrer', 'noopener', 'opener', 'nofollow', 'x'].map((t) => e.relList.supports(t)).join(); })",
                r#""true,true,true,false,false""#,
            ),
            (
                r"E(() => { const e = document.createElement('form'); return ['noreferrer', 'noopener', 'opener', 'x'].map((t) => e.relList.supports(t)).join(); })",
                r#""true,true,true,false""#,
            ),
            (
                r"E(() => { const e = document.createElement('link'); return ['modulepreload', 'preload', 'preconnect', 'dns-prefetch', 'stylesheet', 'icon', 'alternate', 'prefetch', 'prerender', 'next', 'manifest', 'canonical', 'shortcut', 'apple-touch-icon', 'x'].map((t) => e.relList.supports(t)).join(); })",
                r#""true,true,true,true,true,true,true,true,true,true,true,true,false,true,false""#,
            ),
            (
                r"E(() => { const e = document.createElement('link'); e.rel = 'stylesheet icon'; return [e.relList.length, e.relList.value, e.sizes.length, e.sizes === e.sizes, e.blocking.length, e.relList === e.relList]; })",
                r#""2,stylesheet icon,0,true,0,true""#,
            ),
            (
                r"E(() => { const e = document.createElement('link'); e.sizes.add('16x16'); e.sizes = '1x1 2x2'; return [e.getAttribute('sizes'), e.sizes.length]; })",
                r#""1x1 2x2,2""#,
            ),
            (
                r"E(() => { const e = document.createElement('link'); return [e.sizes.supports('x')]; })",
                r#""TypeError: Failed to execute 'supports' on 'DOMTokenList': DOMTokenList has no supported tokens.""#,
            ),
            (
                r"E(() => { const e = document.createElement('link'); return [e.blocking.supports('render'), e.blocking.supports('x'), e.blocking.supports('RENDER')]; })",
                r#""true,false,true""#,
            ),
            (
                r"E(() => { const e = document.createElement('style'); return [e.blocking.supports('render'), e.blocking.length]; })",
                r#""true,0""#,
            ),
            (
                r"E(() => { const e = document.createElement('script'); return [e.blocking.supports('render'), e.blocking.length]; })",
                r#""true,0""#,
            ),
            (
                r"E(() => { const e = document.createElement('iframe'); return ['allow-forms', 'allow-scripts', 'allow-same-origin', 'allow-top-navigation', 'allow-popups', 'allow-modals', 'allow-downloads', 'allow-presentation', 'allow-pointer-lock', 'allow-orientation-lock', 'allow-popups-to-escape-sandbox', 'allow-top-navigation-by-user-activation', 'allow-top-navigation-to-custom-protocols', 'allow-storage-access-by-user-activation', 'x'].map((t) => e.sandbox.supports(t)).join(); })",
                r#""true,true,true,true,true,true,true,true,true,true,true,true,false,true,false""#,
            ),
            (
                r"E(() => { const e = document.createElement('iframe'); e.sandbox = 'allow-forms allow-scripts'; return [e.sandbox.length, e.getAttribute('sandbox'), e.sandbox === e.sandbox]; })",
                r#""2,allow-forms allow-scripts,true""#,
            ),
            (
                r"E(() => { const e = document.createElement('output'); e.htmlFor = 'a b'; return [e.htmlFor.length, e.getAttribute('for'), e.htmlFor.value]; })",
                r#""2,a b,a b""#,
            ),
            (
                r"E(() => { const e = document.createElement('output'); e.htmlFor.add('z'); return [e.getAttribute('for'), e.htmlFor.supports('z')]; })",
                r#""TypeError: Failed to execute 'supports' on 'DOMTokenList': DOMTokenList has no supported tokens.""#,
            ),
            (
                r"E(() => { const e = document.createElement('div'); return [e.classList.supports('x')]; })",
                r#""TypeError: Failed to execute 'supports' on 'DOMTokenList': DOMTokenList has no supported tokens.""#,
            ),
            (
                r"E(() => Object.getOwnPropertyDescriptor(HTMLAnchorElement.prototype, 'relList').set !== undefined)",
                r"true",
            ),
            (
                r"E(() => { const e = document.createElement('script'); return [HTMLScriptElement.supports('classic'), HTMLScriptElement.supports('module'), HTMLScriptElement.supports('importmap'), HTMLScriptElement.supports('speculationrules'), HTMLScriptElement.supports('x'), HTMLScriptElement.supports('')]; })",
                r#""true,true,true,true,false,false""#,
            ),
            (
                r"E(() => HTMLScriptElement.supports.length + ' ' + HTMLScriptElement.supports.name)",
                r#""1 supports""#,
            ),
            (
                r"E(() => Object.getOwnPropertyNames(HTMLScriptElement).join())",
                r#""length,name,prototype,supports""#,
            ),
        ],
    },
    Case {
        name: "tables-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); return [ids(t.rows), t.rows.length, ids(t.tBodies), t.caption.textContent, t.tHead.tagName, t.tFoot.id, ids(t.tBodies[0].rows), t.rows === t.rows, t.rows.item(2).id, t.rows.namedItem('r4').id, t.rows[10]]; })"#,
                r#""TR,r1,r2,r3,r4,r5,6,b1,TBODY,b2,c,THEAD,tf,r1,r2,true,r2,r4,""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); return [t.rows[0].rowIndex, t.rows[1].rowIndex, t.rows[3].rowIndex, t.rows[1].sectionRowIndex, t.rows[2].sectionRowIndex, t.rows[3].sectionRowIndex, t.rows[5].rowIndex, t.rows[5].sectionRowIndex, document.createElement('tr').rowIndex, document.createElement('tr').sectionRowIndex]; })"#,
                r#""0,1,3,0,1,0,5,0,-1,-1""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); const r = t.rows[1]; return [ids(r.cells), r.cells.length, r.cells[1].cellIndex, r.cells[0].cellIndex, document.createElement('td').cellIndex, r.cells.item(5), r.cells === r.cells, r.cells.namedItem('c2').id]; })"#,
                r#""c1,c2,2,1,0,-1,,true,c2""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); const r = t.insertRow(); r.id = 'new'; return [ids(t.rows), r.parentNode.id, r.localName, ids(t.tBodies[1].rows)]; })"#,
                r#""TR,r1,r2,r3,r4,r5,new,tf,tr,r3""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); const r = t.insertRow(0); r.id = 'new'; return [ids(t.rows), r.parentNode.nodeName]; })"#,
                r#""new,TR,r1,r2,r3,r4,r5,THEAD""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); const r = t.insertRow(2); r.id = 'new'; return [ids(t.rows), r.parentNode.id]; })"#,
                r#""TR,r1,new,r2,r3,r4,r5,b1""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); const r = t.insertRow(5); r.id = 'new'; return [ids(t.rows), r.parentNode.nodeName]; })"#,
                r#""TR,r1,r2,r3,r4,new,r5,TFOOT""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); const r = t.insertRow(-1); r.id = 'new'; return [ids(t.rows), r.parentNode.nodeName]; })"#,
                r#""TR,r1,r2,r3,r4,r5,new,TFOOT""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); return t.insertRow(7); })"#,
                r#""DOMException IndexSizeError: Failed to execute 'insertRow' on 'HTMLTableElement': The index provided (7) is greater than the number of rows in the table (6).""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); return t.insertRow(-2); })"#,
                r#""DOMException IndexSizeError: Failed to execute 'insertRow' on 'HTMLTableElement': The index provided (-2) is less than -1.""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); t.deleteRow(0); return ids(t.rows); })"#,
                r#""r1,r2,r3,r4,r5""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); t.deleteRow(-1); return ids(t.rows); })"#,
                r#""TR,r1,r2,r3,r4""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); t.deleteRow(6); return ids(t.rows); })"#,
                r#""DOMException IndexSizeError: Failed to execute 'deleteRow' on 'HTMLTableElement': The index provided (6) is greater than the number of rows in the table (6).""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); t.deleteRow(-2); return ids(t.rows); })"#,
                r#""DOMException IndexSizeError: Failed to execute 'deleteRow' on 'HTMLTableElement': The index provided (-2) is less than -1.""#,
            ),
            (
                r"E(() => { const t = document.createElement('table'); t.deleteRow(-1); t.deleteRow(0); })",
                r#""DOMException IndexSizeError: Failed to execute 'deleteRow' on 'HTMLTableElement': The index provided (0) is greater than the number of rows in the table (0).""#,
            ),
            (
                r"E(() => { const t = document.createElement('table'); t.deleteRow(-1); return t.rows.length; })",
                r"0",
            ),
            (
                r"E(() => { const t = document.createElement('table'); const r = t.insertRow(); return [t.outerHTML, r.parentNode.nodeName, t.tBodies.length]; })",
                r#""<table><tbody><tr></tr></tbody></table>,TBODY,1""#,
            ),
            (
                r"E(() => { const t = document.createElement('table'); t.appendChild(document.createElement('tbody')); const r = t.insertRow(); return [t.outerHTML]; })",
                r#""<table><tbody><tr></tr></tbody></table>""#,
            ),
            (
                r"E(() => { const t = document.createElement('table'); t.appendChild(document.createElement('tr')); t.insertRow(); return [t.outerHTML]; })",
                r#""<table><tr></tr><tr></tr></table>""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); const s = t.tBodies[0]; const r = s.insertRow(); r.id = 'n1'; const q = s.insertRow(0); q.id = 'n2'; return [ids(s.rows), s.rows === s.rows]; })"#,
                r#""n2,r1,r2,n1,true""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); const s = t.tBodies[0]; return s.insertRow(3); })"#,
                r#""DOMException IndexSizeError: Failed to execute 'insertRow' on 'HTMLTableSectionElement': The provided index (3 is outside the range [-1, 2].""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); const s = t.tBodies[0]; s.deleteRow(-1); s.deleteRow(0); return ids(s.rows); })"#,
                r#""""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); const s = t.tBodies[0]; s.deleteRow(2); })"#,
                r#""DOMException IndexSizeError: Failed to execute 'deleteRow' on 'HTMLTableSectionElement': The provided index (2 is outside the range [-1, 2].""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); const r = t.rows[1]; const c = r.insertCell(); c.id = 'nc'; const e = r.insertCell(0); e.id = 'nd'; return [ids(r.cells), c.localName]; })"#,
                r#""nd,c1,c2,nc,td""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); const r = t.rows[1]; return r.insertCell(3); })"#,
                r#""DOMException IndexSizeError: Failed to execute 'insertCell' on 'HTMLTableRowElement': The value provided (3) is outside the range [-1, 2].""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); const r = t.rows[1]; r.deleteCell(0); r.deleteCell(-1); return ids(r.cells); })"#,
                r#""""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); const r = t.rows[1]; r.deleteCell(5); })"#,
                r#""DOMException IndexSizeError: Failed to execute 'deleteCell' on 'HTMLTableRowElement': The value provided (5) is outside the range [0, 2).""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); const r = t.rows[1]; r.deleteCell(-1); r.deleteCell(-1); r.deleteCell(-1); return r.cells.length; })"#,
                r"0",
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); const c = t.caption; t.deleteCaption(); const a = t.caption; const n = t.createCaption(); const m = t.createCaption(); return [c.tagName, a, n === m, t.firstChild === n]; })"#,
                r#""CAPTION,,true,true""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); const h = t.tHead; t.deleteTHead(); const a = t.tHead; const n = t.createTHead(); return [h.tagName, a, n.tagName, ids(t.children)]; })"#,
                r#""THEAD,,THEAD,CAPTION,COLGROUP,THEAD,b1,TBODY,b2,tf""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); t.deleteTFoot(); const a = t.tFoot; const n = t.createTFoot(); return [a, n.tagName, ids(t.children)]; })"#,
                r#"",TFOOT,CAPTION,COLGROUP,THEAD,b1,TBODY,b2,TFOOT""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); const b = t.createTBody(); return [b.tagName, ids(t.children), t.tBodies.length]; })"#,
                r#""TBODY,CAPTION,COLGROUP,THEAD,b1,TBODY,b2,TBODY,tf,4""#,
            ),
            (
                r"E(() => { const t = document.createElement('table'); const b = t.createTBody(); const c = t.createTBody(); t.createTFoot(); t.createTHead(); t.createCaption(); return [t.outerHTML]; })",
                r#""<table><caption></caption><thead></thead><tbody></tbody><tbody></tbody><tfoot></tfoot></table>""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); const h = document.createElement('thead'); t.tHead = h; return [t.tHead === h, ids(t.children)]; })"#,
                r#""true,CAPTION,COLGROUP,THEAD,b1,TBODY,b2,tf""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); t.tHead = document.createElement('tbody'); })"#,
                r#""DOMException HierarchyRequestError: Failed to set the 'tHead' property on 'HTMLTableElement': Not a thead element.""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); t.tFoot = document.createElement('thead'); })"#,
                r#""DOMException HierarchyRequestError: Failed to set the 'tFoot' property on 'HTMLTableElement': Not a tfoot element.""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); t.caption = document.createElement('div'); })"#,
                r#""TypeError: Failed to set the 'caption' property on 'HTMLTableElement': Failed to convert value to 'HTMLTableCaptionElement'.""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); t.tHead = null; return [t.tHead, ids(t.children)]; })"#,
                r#"",CAPTION,COLGROUP,b1,TBODY,b2,tf""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); const f = document.createElement('tfoot'); t.tFoot = f; return [t.tFoot === f, t.lastChild === f, ids(t.children)]; })"#,
                r#""true,true,CAPTION,COLGROUP,THEAD,b1,TBODY,b2,TFOOT""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); const c = document.createElement('caption'); t.caption = c; return [t.caption === c, t.firstChild === c, t.children.length]; })"#,
                r#""true,true,7""#,
            ),
        ],
    },
    Case {
        name: "tables-02",
        html: PAGE,
        queries: &[
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); t.tHead = 5; })"#,
                r#""TypeError: Failed to set the 'tHead' property on 'HTMLTableElement': Failed to convert value to 'HTMLTableSectionElement'.""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); t.rows = 5; return [typeof t.rows]; })"#,
                r#""object""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); const c = document.createElement('col'); c.span = 3; const d2 = document.createElement('td'); d2.colSpan = 5; d2.rowSpan = 0; d2.setAttribute('rowspan', '70000'); return [c.span, d2.colSpan, d2.rowSpan, d2.getAttribute('colspan'), d2.headers, d2.abbr, d2.scope]; })"#,
                r#""3,5,65534,5,,,""#,
            ),
            (
                r"E(() => { const d2 = document.createElement('th'); d2.scope = 'ROW'; d2.setAttribute('colspan', '2000'); d2.setAttribute('rowspan', '70000'); return [d2.scope, d2.getAttribute('scope'), d2.colSpan, d2.rowSpan]; })",
                r#""row,ROW,1000,65534""#,
            ),
            (
                r"E(() => { const c = document.createElement('col'); c.span = 0; return [c.span, c.getAttribute('span')]; })",
                r#""1,0""#,
            ),
            (
                r"E(() => { const c = document.createElement('col'); c.span = 5000; return [c.span, c.getAttribute('span')]; })",
                r#""1000,5000""#,
            ),
            (
                r"E(() => { const c = document.createElement('col'); c.setAttribute('span', '5000'); return [c.span]; })",
                r#""1000""#,
            ),
            (
                r"E(() => { const c = document.createElement('td'); c.colSpan = 0; return [c.colSpan, c.getAttribute('colspan')]; })",
                r#""1,0""#,
            ),
            (
                r"E(() => { const c = document.createElement('td'); c.rowSpan = 70000; return [c.rowSpan, c.getAttribute('rowspan')]; })",
                r#""65534,70000""#,
            ),
            (
                r"E(() => { const c = document.createElement('td'); c.rowSpan = -1; return [c.rowSpan, c.getAttribute('rowspan')]; })",
                r#""1,1""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); const l = t.rows; t.deleteRow(0); const n = l.length; t.insertRow(); return [n, l.length]; })"#,
                r#""5,6""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); return [t.rows.constructor.name, t.tBodies.constructor.name, t.rows[0].cells.constructor.name, Object.prototype.toString.call(t.rows)]; })"#,
                r#""HTMLCollection,HTMLCollection,HTMLCollection,[object HTMLCollection]""#,
            ),
            (
                r#"E(() => { const d = document.createElement('div'); d.innerHTML = "<table id=t><caption>c</caption><colgroup><col></colgroup><thead><tr><th>h1<th>h2</thead><tbody id=b1><tr id=r1><td id=c1>a<td id=c2>b<tr id=r2><td>c</tbody><tr id=r3><td>d</tr><tbody id=b2><tr id=r4><td>e</tbody><tfoot id=tf><tr id=r5><td>f</tfoot></table>"; const t = d.firstChild; const ids = (l) => Array.prototype.map.call(l, (n) => n.id || n.nodeName).join(); return [Object.getOwnPropertyNames(t.rows).join(), Object.keys(t.rows).join()]; })"#,
                r#""0,1,2,3,4,5,r1,r2,r3,r4,r5,0,1,2,3,4,5""#,
            ),
        ],
    },
    Case {
        name: "lists-01",
        html: PAGE,
        queries: &[
            (
                r"E(() => { const m = document.createElement('map'); m.innerHTML = '<area id=a1><div><area id=a2></div>'; return [m.areas.length, m.areas === m.areas, m.areas[1].id, m.areas.constructor.name]; })",
                r#""2,true,a2,HTMLCollection""#,
            ),
            (
                r"E(() => { const m = document.createElement('datalist'); m.innerHTML = '<option id=o1><optgroup><option id=o2></optgroup>'; return [m.options.length, m.options[1].id, m.options.constructor.name]; })",
                r#""2,o2,HTMLCollection""#,
            ),
            (
                r"E(() => { const f = document.createElement('fieldset'); f.innerHTML = '<input id=i1><div><button id=b1></button><select id=s1></select><textarea id=t1></textarea><output id=o1></output><fieldset id=f2></fieldset><object id=ob></object><img id=im><input type=image id=i2></div>'; return [f.elements.length, Array.prototype.map.call(f.elements, (e) => e.id).join(), f.elements.constructor.name, f.elements === f.elements]; })",
                r#""8,i1,b1,s1,t1,o1,f2,ob,i2,HTMLCollection,true""#,
            ),
            (
                r"E(() => { const f = document.createElement('fieldset'); return [f.type, f.name, f.disabled, f.form]; })",
                r#""fieldset,,false,""#,
            ),
            (
                r"E(() => { const f = document.createElement('fieldset'); f.disabled = true; f.name = 'n'; return [f.disabled, f.getAttribute('disabled'), f.name]; })",
                r#""true,,n""#,
            ),
        ],
    },
    Case {
        name: "fix-html-01",
        html: PAGE,
        queries: &[
            (
                r#"E(() => { const a = K('<a href="http://h.test:5/p"></a>').firstChild; const r = []; for (const v of ['h2.test:99', 'a b', 'h/x', 'h?q', 'ex ample', 'h#f', 'h\\x', '', 'h2.test']) { a.href = 'http://h.test:5/p'; a.hostname = v; r.push(a.href); } return r; })"#,
                r#""http://h.test:5/p,http://a%20b:5/p,http://h:5/p,http://h:5/p,http://ex%20ample:5/p,http://h:5/p,http://h:5/p,http://h.test:5/p,http://h2.test:5/p""#,
            ),
            (
                r#"E(() => { const a = K('<a href="http://h.test:5/p"></a>').firstChild; const r = []; for (const v of ['h2.test:99', 'a/b', 'h/x:99', 'h?q:7', 'h2.test:', 'h2.test:x', 'a b', 'h#f:1', '[::1]:9', '']) { a.href = 'http://h.test:5/p'; a.host = v; r.push(a.href); } return r; })"#,
                r#""http://h2.test:99/p,http://a:5/p,http://h:5/p,http://h:5/p,http://h2.test:5/p,http://h2.test:5/p,http://a%20b:5/p,http://h:5/p,http://[::1]:9/p,http://h.test:5/p""#,
            ),
            (
                r#"E(() => { const r = []; for (const h of ['<span contenteditable>', '<span contenteditable=true>', '<span contenteditable=false>', '<div contenteditable=plaintext-only>', '<span contenteditable="">', '<span contenteditable=x>', '<span contenteditable tabindex=3>', '<a href=x contenteditable>', '<span tabindex=-1 contenteditable>', '<span>']) r.push(K(h).firstChild.tabIndex); return r; })"#,
                r#""-1,-1,-1,-1,-1,-1,3,0,-1,-1""#,
            ),
            (
                r"E(() => { const d = K('<div id=ime inputmode=Numeric enterkeyhint=SEND></div>').firstChild; const r = [d.inputMode, d.enterKeyHint]; for (const v of ['', 'none', 'junk', 'TEXT', ' text']) { d.inputMode = v; d.enterKeyHint = v; r.push(d.getAttribute('inputmode') + '/' + d.inputMode + '/' + d.enterKeyHint); } return r; })",
                r#""numeric,send,//,none/none/,junk//,TEXT/text/, text//""#,
            ),
            (
                r"E(() => { const d = document.createElement('div'); const r = [d.inputMode, d.enterKeyHint, 'inputMode' in d, 'enterKeyHint' in d]; d.inputMode = null; r.push(d.getAttribute('inputmode')); return r; })",
                r#"",,true,true,null""#,
            ),
        ],
    },
];
