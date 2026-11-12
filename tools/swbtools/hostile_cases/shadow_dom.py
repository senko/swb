"""Shadow trees and slots in the DOM and in rendering, style scoping and adopted style sheets
(ADR 0036 section 12, sub-features 5a to 5c-2)."""

from swbtools.hostile_cases.core import Page, case, doc, steps_args

JS = ("--js", "--settle-ms", "500")


@case(
    "200,000 shadow hosts made by a script, each with a root and one text node: the roots are "
    "nodes of the arena (within its node limit), and a timer logs `hosts ok` (ADR 0036 "
    "sections 1 and 12)",
    time_limit_s=20,
    expect_log="hosts ok",
    swb_args=JS,
)
def shadow_hosts_200000() -> Page:
    script = (
        "<script>const f = document.createDocumentFragment(); let n = 0; "
        "for (let i = 0; i < 200000; i++) { const d = document.createElement('div'); "
        "d.attachShadow({mode: 'open'}).textContent = 'x'; f.appendChild(d); } "
        "document.body.appendChild(f); for (const d of document.body.children) "
        "if (d.shadowRoot && d.shadowRoot.isConnected) n++; "
        "setTimeout(() => console.warn((n === 200000 ? 'hosts ok ' : 'hosts wrong ') + n), 10);"
        "</script>"
    )
    return Page(doc(script))


@case(
    "20,000 slots of one name in one shadow root and 20,000 children of the host with that "
    "name, then each slot renamed in tree order: each rename moves all 20,000 nodes to the next "
    "slot, the assignment counts the nodes it visits, and the task ends at the time limit of "
    "5 s (ADR 0036 sections 2 and 12)",
    time_limit_s=20,
    expect_log="the time limit was exceeded",
    swb_args=JS,
)
def shadow_slots_same_name_20000() -> Page:
    script = (
        "<script>const h = document.getElementById('h'); "
        "const r = h.attachShadow({mode: 'open'}); "
        "r.innerHTML = '<slot name=a></slot>'.repeat(20000); "
        "const slots = Array.from(r.children); "
        "for (let i = 0; i < slots.length; i++) slots[i].name = 'b' + i; "
        "console.warn('renamed ' + h.firstChild.assignedSlot);</script>"
    )
    return Page(doc("<div id=h>" + "<i slot=a></i>" * 20000 + "</div>" + script))


@case(
    "a `slotchange` listener that moves the host's child to the other slot at each event: "
    "every mutation observer microtask signals both slots again, the microtask checkpoint "
    "ends at the time limit of 5 s (ADR 0036 section 12)",
    time_limit_s=20,
    expect_log="the time limit was exceeded",
    swb_args=JS,
)
def shadow_slotchange_loop() -> Page:
    script = (
        "<script>const h = document.getElementById('h'); const c = h.firstChild; "
        "const r = h.attachShadow({mode: 'open'}); "
        "r.innerHTML = '<slot name=a></slot><slot name=b></slot>'; "
        "r.addEventListener('slotchange', () => { c.slot = c.slot === 'a' ? 'b' : 'a'; });"
        "</script>"
    )
    return Page(doc("<div id=h><i slot=a>x</i></div>" + script))


@case(
    "manual slot assignment of the 100,000 children of a host, alternately to two slots, "
    "100 times: each `assign()` moves every node out of the other slot's list in one pass, "
    "and a timer logs `assign ok` (ADR 0036 sections 2 and 12)",
    time_limit_s=30,
    expect_log="assign ok",
    swb_args=JS,
)
def shadow_assign_flood() -> Page:
    script = (
        "<script>const h = document.getElementById('h'); "
        "const r = h.attachShadow({mode: 'open', slotAssignment: 'manual'}); "
        "r.innerHTML = '<slot></slot><slot></slot>'; const [s1, s2] = r.children; "
        "const nodes = Array.from(h.children); "
        "for (let i = 0; i < 100; i++) (i % 2 ? s2 : s1).assign(...nodes); "
        "const n = s2.assignedNodes().length + s1.assignedNodes().length; "
        "setTimeout(() => console.warn((n === 100000 ? 'assign ok ' : 'assign wrong ') + n), 10);"
        "</script>"
    )
    return Page(doc("<div id=h>" + "<i></i>" * 100000 + "</div>" + script))


@case(
    "a host whose shadow tree has 200 nodes and a slot moved into an iframe's document and "
    "back 1,000 times (400,000 nodes made: a transplant leaves the taken nodes in their arena, "
    "so a larger tree reaches the node limit, as a light tree does): each transplant takes the "
    "root with its host, the other arena assigns the slot again, and a timer logs "
    "`transplant ok` (ADR 0036 sections 1 and 12)",
    time_limit_s=30,
    expect_log="transplant ok",
    swb_args=JS,
)
def shadow_transplant() -> Page:
    script = (
        "<script>const fd = document.getElementById('f').contentDocument; "
        "const h = document.getElementById('h'); "
        "const r = h.attachShadow({mode: 'open'}); "
        "r.innerHTML = '<slot></slot>' + '<b>x</b>'.repeat(99); "
        "for (let i = 0; i < 1000; i++) { fd.adoptNode(h); document.body.appendChild(h); } "
        "const root = h.shadowRoot; const ok = root && root.childNodes.length === 100 && "
        "root.firstChild.assignedNodes().length === 1 && h.firstChild.assignedSlot === "
        "root.firstChild; setTimeout(() => console.warn((ok ? 'transplant ok ' : "
        "'transplant wrong ') + (root && root.childNodes.length)), 10);</script>"
    )
    return Page(doc("<iframe id=f></iframe><div id=h><i>light</i></div>" + script))


@case(
    "20,000 slots prepended one by one to a shadow root (each is the first child): a slot at "
    "either end of the root's list costs a few steps, not a scan of the siblings, and a "
    "timer logs `prepend ok` (ADR 0036 sections 2 and 12)",
    time_limit_s=20,
    expect_log="prepend ok",
    swb_args=JS,
)
def shadow_slots_prepend_20000() -> Page:
    script = (
        "<script>const h = document.getElementById('h'); "
        "const r = h.attachShadow({mode: 'open'}); let last; "
        "for (let i = 0; i < 20000; i++) { last = document.createElement('slot'); "
        "r.prepend(last); } "
        "const ok = r.firstChild === last && h.firstChild.assignedSlot === last; "
        "setTimeout(() => console.warn((ok ? 'prepend ok ' : 'prepend wrong ') + "
        "r.childNodes.length), 10);</script>"
    )
    return Page(doc("<div id=h><i>x</i></div>" + script))


@case(
    "a chain of 30,000 divs built bottom-up with `appendChild` after one `attachShadow` "
    "elsewhere in the page: the slot search walks only a bounded part of the inserted subtree "
    "when the parent is detached, and a timer logs `chain ok` (ADR 0036 sections 2 and 12)",
    time_limit_s=20,
    expect_log="chain ok",
    swb_args=JS,
)
def shadow_detached_chain_30000() -> Page:
    script = (
        "<script>document.getElementById('h').attachShadow({mode: 'open'}); "
        "let p = document.createElement('div'); "
        "for (let i = 0; i < 30000; i++) { const q = document.createElement('div'); "
        "q.appendChild(p); p = q; } document.body.appendChild(p); "
        "setTimeout(() => console.warn('chain ok'), 10);</script>"
    )
    return Page(doc("<div id=h></div>" + script))


@case(
    "a host with 100,000 children assigned to one slot, removed one by one from the end and "
    "then from the front in a second host: removing at either end of the assigned list costs "
    "a few steps, and a timer logs `remove ok` (ADR 0036 sections 2 and 12)",
    time_limit_s=30,
    expect_log="remove ok",
    swb_args=JS,
)
def shadow_assigned_remove_100000() -> Page:
    script = (
        "<script>let n = 0; for (const id of ['h', 'g']) { "
        "const h = document.getElementById(id); "
        "h.attachShadow({mode: 'open'}).innerHTML = '<slot></slot>'; "
        "while (id === 'h' ? h.lastChild : h.firstChild) "
        "h.removeChild(id === 'h' ? h.lastChild : h.firstChild); n++; } "
        "setTimeout(() => console.warn((n === 2 ? 'remove ok ' : 'remove wrong ') + n), 10);"
        "</script>"
    )
    kids = "<i></i>" * 100000
    return Page(doc(f"<div id=h>{kids}</div><div id=g>{kids}</div>" + script))


@case(
    "20,000 shadow hosts in the page and 20,000 adoptions of a one-node element into an "
    "iframe's document and back: a transplant forgets the shadow state of the moved nodes "
    "only, not a scan of all the maps, and a timer logs `adopt ok` (ADR 0036 sections 1 and 12)",
    time_limit_s=30,
    expect_log="adopt ok",
    swb_args=JS,
)
def shadow_adopt_many_hosts() -> Page:
    script = (
        "<script>for (let i = 0; i < 20000; i++) { const d = document.createElement('div'); "
        "d.attachShadow({mode: 'open'}).innerHTML = '<slot></slot>'; "
        "d.appendChild(document.createElement('i')); document.body.appendChild(d); } "
        "const fd = document.getElementById('f').contentDocument; "
        "const e = document.getElementById('e'); "
        "for (let i = 0; i < 10000; i++) { fd.adoptNode(e); document.body.appendChild(e); } "
        "setTimeout(() => console.warn('adopt ok'), 10);</script>"
    )
    return Page(doc("<iframe id=f></iframe><b id=e>x</b>" + script))


_FONT = "<style>body { margin: 8px; font: 16px/20px sans-serif }</style>"


@case(
    "100,000 nested shadow trees, each host a `div` in the previous root, with a link at the "
    "bottom: style and layout walk the flat tree without recursion (below 256 levels the "
    "content is flattened into text), a click on the text reaches the document's listener at "
    "the outermost host, and a Tab key does not fail (ADR 0036 sections 4 and 12, 5b)",
    time_limit_s=30,
    expect_log="clicked h",
    swb_args=(*steps_args([{"click": [12, 14]}, {"key": "Tab"}]), "--settle-ms", "500"),
)
def shadow_nested_100000() -> Page:
    script = (
        "<script>let e = document.getElementById('h'); "
        "for (let i = 0; i < 100000; i++) { const d = document.createElement('div'); "
        "e.attachShadow({mode: 'open'}).appendChild(d); e = d; } "
        "e.innerHTML = '<a href=\"#x\">deep deep deep</a>'; "
        "document.addEventListener('click', (ev) => console.warn('clicked ' + ev.target.id));"
        "</script>"
    )
    return Page(doc(_FONT + "<div id=h></div>" + script))


@case(
    "10,000 nested hosts that pass one slot on to the next, with one light child at the top: "
    "the light child's flat parent is the outermost slot, and each slot's the next one, so a "
    "style pass costs the number of slots, not its square; a click on the text reaches the "
    "light child (ADR 0036 sections 4 and 12, 5b)",
    time_limit_s=30,
    expect_log="clicked x",
    swb_args=(*steps_args([{"click": [12, 14]}]), "--settle-ms", "500"),
)
def shadow_slot_passthrough_10000() -> Page:
    script = (
        "<script>let e = document.getElementById('h'); "
        "for (let i = 0; i < 10000; i++) { const d = document.createElement('div'); "
        "d.appendChild(document.createElement('slot')); "
        "e.attachShadow({mode: 'open'}).appendChild(d); e = d; } "
        "document.addEventListener('click', (ev) => console.warn('clicked ' + ev.target.id));"
        "</script>"
    )
    return Page(doc(_FONT + "<div id=h><b id=x>pass pass pass</b></div>" + script))


@case(
    "100,000 shadow roots, each with a distinct `<style>` of 1 KB and a `b`: the parsed sheets "
    "and the scoped stylists count in the document's style memory (128 MiB), so the sheets past "
    "it are not parsed, with one warning; style and layout run, and a timer logs `styles ok` "
    "(ADR 0036 sections 5 and 12, 5c-1)",
    time_limit_s=30,
    rss_limit_mib=2048,
    expect_log="styles ok",
    swb_args=JS,
)
def shadow_styles_100000() -> Page:
    script = (
        "<script>const base = 'b{margin:1px 2px 3px 4px;padding:1px}'.repeat(28); "
        "const f = document.createDocumentFragment(); let last = null; "
        "for (let i = 0; i < 100000; i++) { const d = document.createElement('div'); "
        "const s = document.createElement('style'); "
        "s.textContent = base + '.c' + i + '{color:red}'; "
        "last = document.createElement('b'); last.className = 'c' + i; last.textContent = 'x'; "
        "d.attachShadow({mode: 'open'}).append(s, last); f.appendChild(d); } "
        "document.body.appendChild(f); const c = getComputedStyle(last).marginTop; "
        "setTimeout(() => console.warn('styles ok ' + c), 10);</script>"
    )
    return Page(doc(script))


@case(
    "10,000 nested shadow hosts, each root with `:host-context(.x)` and the next host, below a "
    "`.x` body: each host's match walks its flat ancestors up to `.x`, and the walks share "
    "their answers, so the style pass is linear; a timer logs `context ok` (ADR 0036 sections "
    "5 and 12, 5c-1)",
    time_limit_s=20,
    expect_log="context ok",
    swb_args=JS,
)
def shadow_host_context_nested_10000() -> Page:
    script = (
        "<script>document.body.className = 'x'; let e = document.getElementById('h'); "
        "for (let i = 0; i < 10000; i++) { const r = e.attachShadow({mode: 'open'}); "
        "r.innerHTML = '<style>:host-context(.x){padding-left:1px}</style><div></div>'; "
        "e = r.lastChild; } const p = getComputedStyle(e).paddingLeft; "
        "setTimeout(() => console.warn('context ok ' + p), 10);</script>"
    )
    return Page(doc(_FONT + "<div id=h></div>" + script))


@case(
    "10,000 nested hosts that pass one slot on to the next, each root with a `::slotted(*)` "
    "rule, with one light child at the top: an element gets the `::slotted()` rules of at most "
    "256 slots of its chain, so the style pass is not quadratic; a click on the text reaches "
    "the light child (ADR 0036 sections 5 and 12, 5c-1)",
    time_limit_s=30,
    expect_log="clicked x",
    swb_args=(*steps_args([{"click": [12, 14]}]), "--settle-ms", "500"),
)
def shadow_slotted_passthrough_10000() -> Page:
    script = (
        "<script>let e = document.getElementById('h'); "
        "for (let i = 0; i < 10000; i++) { const d = document.createElement('div'); "
        "const s = document.createElement('style'); s.textContent = '::slotted(*){color:green}'; "
        "d.appendChild(document.createElement('slot')); "
        "e.attachShadow({mode: 'open'}).append(s, d); e = d; } "
        "document.addEventListener('click', (ev) => console.warn('clicked ' + ev.target.id));"
        "</script>"
    )
    return Page(doc(_FONT + "<div id=h><b id=x>pass pass pass</b></div>" + script))


@case(
    "`adoptedStyleSheets` past its bounds: one constructed sheet pushed 65,537 times into the "
    "document's array (the last push throws `NotAllowedError`, one warning: at most 65,536 "
    "entries, each sheet applied once), then 10,000 roots that each adopt a distinct sheet of "
    "10 KB (the parsed rules count in the document's style memory of 128 MiB: the sheets past "
    "it get no rules, one warning); style and layout run, and a timer logs `adopted ok` (ADR "
    "0036 sections 6 and 12, 5c-2)",
    time_limit_s=30,
    rss_limit_mib=2048,
    expect_log=("adopted ok", "more than 65536 entries"),
    swb_args=JS,
)
def shadow_adopted_many() -> Page:
    script = (
        "<script>const one = new CSSStyleSheet(); one.replaceSync('p{color:green}'); "
        "const a = document.adoptedStyleSheets; let refused = ''; "
        "for (let i = 0; i < 65537; i++) { try { a.push(one); } catch (e) { refused = e.name; } } "
        "const base = 'b{margin:1px 2px 3px 4px;padding:1px}'.repeat(280); "
        "const f = document.createDocumentFragment(); let last = null; "
        "for (let i = 0; i < 10000; i++) { const d = document.createElement('div'); "
        "const s = new CSSStyleSheet(); s.replaceSync(base + '.c' + i + '{color:red}'); "
        "last = document.createElement('b'); last.className = 'c' + i; last.textContent = 'x'; "
        "const r = d.attachShadow({mode: 'open'}); r.append(last); r.adoptedStyleSheets = [s]; "
        "f.appendChild(d); } "
        "document.body.appendChild(f); const c = getComputedStyle(last).marginTop; "
        "setTimeout(() => console.warn('adopted ok ' + a.length + ' ' + refused + ' ' + c), 10);"
        "</script>"
    )
    return Page(doc("<p>p</p>" + script))


@case(
    "20,000 `push` and `pop` of a constructed sheet on the document's `adoptedStyleSheets` "
    "with a style read after each, then 100,000 on a root's without reads: each change drops "
    "the stylist, and a read builds it for the few sheets of the page (about 0.14 ms), so the "
    "loop is linear; a timer logs `push ok` (ADR 0036 sections 6 and 12, 5c-2)",
    time_limit_s=20,
    expect_log="push ok",
    swb_args=JS,
)
def shadow_adopted_push_pop_loop() -> Page:
    script = (
        "<script>const s = new CSSStyleSheet(); s.replaceSync('p{color:green} b{color:blue}'); "
        "const p = document.querySelector('p'); const a = document.adoptedStyleSheets; let c = ''; "
        "for (let i = 0; i < 20000; i++) { a.push(s); c = getComputedStyle(p).color; a.pop(); } "
        "const r = document.getElementById('h').attachShadow({mode: 'open'}); "
        "r.innerHTML = '<b>b</b>'; const ra = r.adoptedStyleSheets; "
        "for (let i = 0; i < 100000; i++) { ra.push(s); ra.pop(); } ra.push(s); "
        "const b = getComputedStyle(r.firstChild).color; "
        "setTimeout(() => console.warn('push ok ' + c + ' ' + b), 10);</script>"
    )
    return Page(doc("<p>p</p><div id=h></div>" + script))
