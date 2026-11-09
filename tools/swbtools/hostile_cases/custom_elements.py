"""Custom elements: upgrades, definitions and their budgets (ADR 0035 sections 5 and 10)."""

from swbtools.hostile_cases.core import Page, case, doc


@case(
    "100,000 custom elements in the markup, upgraded at one `define` with a constructor and "
    "a `connectedCallback` each: the upgrades run inside the call, the walk of the document "
    "is charged to the time limit, and a timer logs `upgrade ok` (ADR 0035 sections 4 and 10)",
    time_limit_s=20,
    expect_log="upgrade ok",
    swb_args=("--js", "--settle-ms", "500"),
)
def ce_upgrade_100000() -> Page:
    script = (
        "<script>let built = 0, connected = 0; customElements.define('x-m', class extends "
        "HTMLElement { constructor() { super(); built++; } connectedCallback() { connected++; } "
        "}); const left = document.querySelectorAll(':not(:defined)').length; "
        "setTimeout(() => console.warn((built === 100000 && connected === 100000 && left === 0 "
        "? 'upgrade ok ' : 'upgrade wrong ') + built + ' ' + connected + ' ' + left), 10);"
        "</script>"
    )
    return Page(doc("<x-m></x-m>" * 100000 + script))


@case(
    "a chain of definitions: each constructor defines the next of 10,000 names, whose element "
    "in the markup is upgraded inside that `define`, so each upgrade nests in the one before: "
    "the 44th nested upgrade is dropped at the nesting limit of 43 without a report, the Rust "
    "stack stays bounded, and a timer logs `chain ok` (ADR 0035 sections 5 and 10)",
    time_limit_s=20,
    expect_log="chain ok",
    swb_args=("--js", "--settle-ms", "500"),
)
def ce_define_chain() -> Page:
    script = (
        "<script>let defined = 0, built = 0; function next(i) { if (i >= 10000) return; "
        "customElements.define('x-c' + i, class extends HTMLElement { constructor() { super(); "
        "built++; next(i + 1); } }); defined++; } next(0); "
        "setTimeout(() => console.warn((defined === 44 && built === 43 "
        "? 'chain ok ' : 'chain wrong ') + defined + ' ' + built), 10);</script>"
    )
    elements = "".join(f"<x-c{i}></x-c{i}>" for i in range(10000))
    return Page(doc(elements + script))


@case(
    "100,000 `customElements.define` calls with new names in a loop: the agent keeps at most "
    "65,536 definitions, later calls throw `NotSupportedError` with one warning, and a timer "
    "logs `names ok` (ADR 0035 section 10)",
    time_limit_s=20,
    expect_log=("too many custom elements", "names ok"),
    swb_args=("--js", "--settle-ms", "500"),
)
def ce_define_100000_names() -> Page:
    script = (
        "<script>let ok = 0, refused = 0; for (let i = 0; i < 100000; i++) { try { "
        "customElements.define('x-n' + i, class extends HTMLElement {}); ok++; } catch (e) { "
        "if (e.name === 'NotSupportedError') refused++; } } "
        "setTimeout(() => console.warn((ok === 65536 && refused === 34464 "
        "? 'names ok ' : 'names wrong ') + ok + ' ' + refused), 10);</script>"
    )
    return Page(doc(script))


@case(
    "one definition with 100,000 observed attribute names that upgrades an element with "
    "1,000 attributes (1,000 `attributeChangedCallback` reactions inside the `define` call), "
    "then 100,000 attribute writes to the element: a timer logs `observed ok` (ADR 0035 "
    "sections 5 and 10)",
    time_limit_s=20,
    expect_log="observed ok",
    swb_args=("--js", "--settle-ms", "500"),
)
def ce_observed_attributes_100000() -> Page:
    script = (
        "<script>const names = []; for (let i = 0; i < 100000; i++) names.push('a' + i); "
        "const e = document.createElement('x-o'); for (let i = 0; i < 1000; i++) "
        "e.setAttribute('a' + i, '1'); document.body.appendChild(e); let calls = 0; "
        "customElements.define('x-o', class extends HTMLElement { static get "
        "observedAttributes() { return names; } attributeChangedCallback() { calls++; } }); "
        "const upgraded = calls; for (let i = 0; i < 30000; i++) e.setAttribute('a' + (i % 1000), "
        "String(i)); setTimeout(() => console.warn((upgraded === 1000 ? 'observed ok ' : "
        "'observed wrong ') + upgraded + ' ' + calls), 10);</script>"
    )
    return Page(doc(script))


@case(
    "1,000,000 `customElements.whenDefined` calls with new names: the registry keeps at most "
    "65,536 pending promises, later ones are new promises that never settle (one warning), "
    "and a timer logs `when ok` (ADR 0035 section 10)",
    time_limit_s=20,
    expect_log=("customElements.whenDefined", "when ok"),
    swb_args=("--js", "--settle-ms", "500"),
)
def ce_when_defined_1000000() -> Page:
    script = (
        "<script>for (let i = 0; i < 1000000; i++) customElements.whenDefined('x-w' + i); "
        "let same = 0; for (let i = 0; i < 70000; i++) { const n = 'x-w' + i; "
        "if (customElements.whenDefined(n) === customElements.whenDefined(n)) same++; } "
        "setTimeout(() => console.warn((same === 65536 ? 'when ok ' : 'when wrong ') + same), "
        "10);</script>"
    )
    return Page(doc(script))


@case(
    "100 `customElements.define` calls and 100 `customElements.whenDefined` calls with new "
    "names of 1 MiB each: the names of the definitions and the names that wait have a byte "
    "budget of 16 MiB each that the engine heap limit does not count, so later `define` calls "
    "throw `NotSupportedError`, later `whenDefined` calls return promises that are not stored "
    "(one warning each), and a timer logs `long ok` (ADR 0035 section 10)",
    time_limit_s=20,
    expect_log=("too many custom elements", "too many names wait", "long ok"),
    swb_args=("--js", "--settle-ms", "500"),
)
def ce_long_names() -> Page:
    script = (
        "<script>const base = 'x-' + 'a'.repeat(1 << 20); let ok = 0, refused = 0, same = 0; "
        "for (let i = 0; i < 100; i++) { try { customElements.define(base + i, class extends "
        "HTMLElement {}); ok++; } catch (e) { if (e.name === 'NotSupportedError') refused++; } } "
        "for (let i = 0; i < 100; i++) { const n = base + 'w' + i; "
        "if (customElements.whenDefined(n) === customElements.whenDefined(n)) same++; } "
        "setTimeout(() => console.warn((ok >= 10 && ok <= 16 && ok + refused === 100 "
        "&& same >= 10 && same <= 16 ? 'long ok ' : 'long wrong ') + ok + ' ' + refused + ' ' "
        "+ same), 10);</script>"
    )
    return Page(doc(script))


@case(
    "a `connectedCallback` that appends a new instance of its own element, so that each "
    "callback causes the next: the nested reactions stop at the nesting limit of 43 (the "
    "document holds 44 elements, as in Chromium), without a report, and a timer logs "
    "`recursion ok` (ADR 0035 sections 5 and 10)",
    time_limit_s=20,
    expect_log="recursion ok",
    swb_args=("--js", "--settle-ms", "500"),
)
def ce_connected_recursion() -> Page:
    script = (
        "<script>customElements.define('x-r', class extends HTMLElement { connectedCallback() "
        "{ this.appendChild(document.createElement('x-r')); } }); "
        "document.body.appendChild(document.createElement('x-r')); "
        "const n = document.querySelectorAll('x-r').length; "
        "setTimeout(() => console.warn((n === 44 ? 'recursion ok ' : 'recursion wrong ') + n), "
        "10);</script>"
    )
    return Page(doc(script))


@case(
    "a `connectedCallback` that removes and appends its own element again, in a loop that "
    "would run 1,000 times: it runs 43 times (Chromium's nesting limit), and a timer logs "
    "`reinsert ok` (ADR 0035 sections 5 and 10)",
    time_limit_s=20,
    expect_log="reinsert ok",
    swb_args=("--js", "--settle-ms", "500"),
)
def ce_reinsert_loop() -> Page:
    script = (
        "<script>let n = 0; customElements.define('x-l', class extends HTMLElement { "
        "connectedCallback() { n++; if (n < 1000) document.body.appendChild(this); } "
        "disconnectedCallback() {} }); "
        "document.body.appendChild(document.createElement('x-l')); "
        "setTimeout(() => console.warn((n === 43 ? 'reinsert ok ' : 'reinsert wrong ') + n), "
        "10);</script>"
    )
    return Page(doc(script))


@case(
    "a custom element with 100 observed attributes: 30,000 `setAttribute` calls with 10 KB "
    "values (each reaction copies the old and the new value), then one `innerHTML` of 100 "
    "such elements with 100 attributes of 1 KB each (10,000 `attributeChangedCallback` "
    "reactions at the upgrades): the callbacks run, the memory stays bounded, and a timer "
    "logs `flood ok` or the task ends at its time limit (ADR 0035 sections 5 and 10)",
    time_limit_s=20,
    expect_log="flood ok",
    swb_args=("--js", "--settle-ms", "500"),
)
def ce_attribute_flood() -> Page:
    script = (
        "<script>const names = []; for (let i = 0; i < 100; i++) names.push('a' + i); "
        "let calls = 0; customElements.define('x-f', class extends HTMLElement { static get "
        "observedAttributes() { return names; } attributeChangedCallback() { calls++; } }); "
        "const e = document.createElement('x-f'); document.body.appendChild(e); "
        "const big = 'x'.repeat(10240); "
        "for (let i = 0; i < 30000; i++) e.setAttribute('a' + (i % 100), big + i); "
        "const before = calls; const small = 'y'.repeat(1024); let one = '<x-f'; "
        "for (let i = 0; i < 100; i++) one += ' a' + i + '=' + small; one += '></x-f>'; "
        "const h = document.createElement('div'); document.body.appendChild(h); "
        "h.innerHTML = one.repeat(100); "
        "setTimeout(() => console.warn((before === 30000 && calls === 40000 "
        "? 'flood ok ' : 'flood wrong ') + before + ' ' + calls), 10);</script>"
    )
    return Page(doc(script))


@case(
    "100,000 connected custom elements removed by `textContent = ''`: 100,000 "
    "`disconnectedCallback` reactions in one element queue, in tree order, and a timer logs "
    "`disconnect ok` (ADR 0035 section 10)",
    time_limit_s=20,
    expect_log="disconnect ok",
    swb_args=("--js", "--settle-ms", "500"),
)
def ce_disconnect_100000() -> Page:
    script = (
        "<script>let gone = 0, last = -1, ordered = true; customElements.define('x-d', class "
        "extends HTMLElement { disconnectedCallback() { gone++; const i = +this.id; "
        "if (i <= last) ordered = false; last = i; } }); "
        "const h = document.getElementById('h'); h.textContent = ''; "
        "setTimeout(() => console.warn((gone === 100000 && ordered "
        "? 'disconnect ok ' : 'disconnect wrong ') + gone + ' ' + ordered), 10);</script>"
    )
    elements = "".join(f"<x-d id={i}></x-d>" for i in range(100000))
    return Page(doc(f"<div id=h>{elements}</div>" + script))


@case(
    "100,000 custom elements in the markup, upgraded at one `define`; each constructor moves "
    "a new element of an iframe's document into the page's document (100,000 transplants "
    "between arenas while 100,000 upgrades are queued): a transplant from a document with "
    "nothing queued costs O(1), so the page is not quadratic, and a timer logs `transplant "
    "ok` (ADR 0035 section 7)",
    time_limit_s=20,
    expect_log="transplant ok",
    swb_args=("--js", "--settle-ms", "500"),
)
def ce_transplant_100000() -> Page:
    script = (
        "<script>const fd = document.getElementById('f').contentDocument; let built = 0, "
        "moved = 0; customElements.define('x-t', class extends HTMLElement { constructor() { "
        "super(); built++; if (moved < 100000) { moved++; document.adoptNode(fd.createElement("
        "'p')); } } }); setTimeout(() => console.warn((built === 100000 && moved === 100000 "
        "? 'transplant ok ' : 'transplant wrong ') + built + ' ' + moved), 10);</script>"
    )
    return Page(doc("<iframe id=f></iframe>" + "<x-t></x-t>" * 100000 + script))
