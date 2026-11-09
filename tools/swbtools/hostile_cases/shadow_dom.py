"""Shadow trees and slots in the DOM (ADR 0036 section 12, sub-feature 5a)."""

from swbtools.hostile_cases.core import Page, case, doc

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
