"""Storage keys, `storage` events and `NodeIterator` (ADR 0031, ADR 0028)."""

from swbtools.hostile_cases.core import (
    REPLAY_URL,
    Page,
    case,
    doc,
    origins_page,
)

# --- storage keys and `storage` events (ADR 0031 section 11) -----------------

SAME_ORIGIN_FRAME = "http://hostile.test/x.html"
STORAGE_ARGS = ("--js", "--replay", "{dir}", "--settle-ms", "2500")
LISTENER = "<!doctype html><script>let n = 0; addEventListener('storage', () => { n++; });</script>"


def listening_frames(count: int) -> str:
    """`count` same-origin frames that listen for `storage` events."""
    return "".join(f"<iframe src='{SAME_ORIGIN_FRAME}'></iframe>" for _ in range(count))


@case(
    "A frame that writes localStorage in a loop (200,000 writes) while 100 same-origin frames "
    "listen (M9 feature 14b): the events that wait stop at the budget of waiting messages and "
    "events of the writer's runtime, with one warning, and the writes go on",
    swb_args=STORAGE_ARGS,
    url=REPLAY_URL,
    time_limit_s=15.0,
    rss_limit_mib=1500,
    expect_log=("further storage events are dropped", "writer end"),
)
def storage_events_flood_with_100_listening_frames_js_on() -> Page:
    writer = (
        "<!doctype html><script>addEventListener('load', () => { for (let i = 0; i < 200000; "
        "i++) localStorage.setItem('k', String(i)); console.warn('writer end ' + "
        "localStorage.getItem('k')); });</script>"
    )
    return origins_page(
        doc(listening_frames(100) + "<iframe src='http://hostile.test/w.html'></iframe>"),
        {SAME_ORIGIN_FRAME: LISTENER, "http://hostile.test/w.html": writer},
    )


@case(
    "One key with a value at the quota (5 Mi units), written and removed in a loop with three "
    "frames that listen (M9 feature 14b): the bytes of the waiting events stop at the budget, "
    "and the strings are shared by the receivers of one write",
    swb_args=STORAGE_ARGS,
    url=REPLAY_URL,
    time_limit_s=15.0,
    rss_limit_mib=1500,
    expect_log=("further storage events are dropped", "quota end"),
)
def storage_value_at_the_quota_in_a_loop_with_listeners_js_on() -> Page:
    top = (
        "<script>addEventListener('load', () => { const v = 'x'.repeat((5 << 20) - 8); "
        "for (let i = 0; i < 100; i++) { localStorage.setItem('k', v); localStorage.removeItem"
        "('k'); } console.warn('quota end ' + localStorage.length); });</script>"
    )
    return origins_page(
        doc(listening_frames(3) + top),
        {SAME_ORIGIN_FRAME: LISTENER},
    )


@case(
    "clear() in a loop (400,000 calls on an area with one key) with five frames that listen "
    "(M9 feature 14b): every call changes the area and queues events until the budget",
    swb_args=STORAGE_ARGS,
    url=REPLAY_URL,
    time_limit_s=15.0,
    rss_limit_mib=1500,
    expect_log=("further storage events are dropped", "clear end"),
)
def storage_clear_in_a_loop_with_listeners_js_on() -> Page:
    top = (
        "<script>addEventListener('load', () => { for (let i = 0; i < 400000; i++) { "
        "localStorage.setItem('a', 'b'); localStorage.clear(); } console.warn('clear end'); "
        "});</script>"
    )
    return origins_page(doc(listening_frames(5) + top), {SAME_ORIGIN_FRAME: LISTENER})


@case(
    "30 frames of 30 origins that each write 4 MiB, under the quota of an area (M9 feature 14b): "
    "the total of 64 MiB over all keys stops them with QuotaExceededError, in 30 runtimes besides "
    "the top level's",
    swb_args=STORAGE_ARGS,
    url=REPLAY_URL,
    time_limit_s=15.0,
    rss_limit_mib=1800,
    expect_log="total reached",
)
def storage_total_over_30_origins_js_on() -> Page:
    urls = [f"http://s{i}.hostile.test/x.html" for i in range(30)]
    child = (
        "<!doctype html><script>let full = false; for (let i = 0; i < 4 && !full; i++) { "
        "try { localStorage.setItem('k' + i, 'x'.repeat(1 << 20)); } catch (e) { full = "
        "e.name === 'QuotaExceededError'; } } parent.postMessage(full ? 'full' : 'ok', '*');"
        "</script>"
    )
    frames = "".join(f"<iframe src='{u}'></iframe>" for u in urls)
    top = (
        "<script>let got = 0, full = 0; addEventListener('message', (e) => { got++; if "
        "(e.data === 'full') full++; if (got === 30) console.warn(full > 0 ? 'total reached' : "
        "'total not reached'); });</script>"
    )
    return origins_page(doc(frames + top), dict.fromkeys(urls, child))


@case(
    "A frame that navigates in a loop while the top level writes localStorage in a loop "
    "(M9 feature 14b): the events that wait for a document that retires go with it",
    swb_args=STORAGE_ARGS,
    url=REPLAY_URL,
    time_limit_s=15.0,
    rss_limit_mib=800,
    expect_log="storage end",
)
def storage_events_for_a_frame_that_navigates_in_a_loop_js_on() -> Page:
    child = (
        "addEventListener('storage', () => {}); addEventListener('load', () => { location.href"
        " = location.pathname === '/x.html' ? '/y.html' : '/x.html'; });"
    )
    top = (
        "let n = 0; addEventListener('load', () => { const go = () => { for (let i = 0; i < "
        "5000; i++) localStorage.setItem('k', String(i)); if (++n === 20) console.warn('storage "
        "end'); else setTimeout(go, 5); }; go(); });"
    )
    return origins_page(
        doc(f"<iframe id=f src='{SAME_ORIGIN_FRAME}'></iframe><script>{top}</script>"),
        {
            SAME_ORIGIN_FRAME: f"<!doctype html><script>{child}</script>",
            "http://hostile.test/y.html": f"<!doctype html><script>{child}</script>",
        },
    )


@case(
    "A Storage object of a frame document that retired, written through in a loop "
    "(M9 feature 14b): every call throws SecurityError, and the area of the top level stays empty",
    swb_args=STORAGE_ARGS,
    url=REPLAY_URL,
    time_limit_s=10.0,
    rss_limit_mib=500,
    expect_log="retired end 200000 0",
)
def storage_object_of_a_retired_document_js_on() -> Page:
    top = (
        "<script>addEventListener('load', () => { const held = f.contentWindow.localStorage; "
        "const held2 = f.contentWindow.sessionStorage; f.remove(); let denied = 0; for (let i = "
        "0; i < 100000; i++) { try { held.setItem('k' + i, 'v'); } catch (e) { if (e.name === "
        "'SecurityError') denied++; } try { held2.clear(); } catch (e) { if (e.name === "
        "'SecurityError') denied++; } } console.warn('retired end ' + denied + ' ' + "
        "localStorage.length); });</script>"
    )
    return origins_page(
        doc(f"<iframe id=f src='{SAME_ORIGIN_FRAME}'></iframe>{top}"),
        {SAME_ORIGIN_FRAME: LISTENER},
    )


@case(
    "A chain of 30 nested same-origin frames that all listen for storage events while the top "
    "level writes in a loop (M9 feature 14b): the events reach each document as tasks, up to the "
    "budget",
    swb_args=STORAGE_ARGS,
    url=REPLAY_URL,
    time_limit_s=15.0,
    rss_limit_mib=1500,
    expect_log="chain end",
)
def storage_events_in_a_chain_of_nested_frames_js_on() -> Page:
    depth = 30
    documents = {}
    for i in range(depth):
        inner = ""
        if i + 1 < depth:
            inner = f"<iframe src='http://hostile.test/n{i + 1}.html'></iframe>"
        documents[f"http://hostile.test/n{i}.html"] = (
            "<!doctype html><script>addEventListener('storage', () => {});</script>" + inner
        )
    top = (
        "<script>addEventListener('load', () => { for (let i = 0; i < 100000; i++) "
        "localStorage.setItem('k', String(i)); console.warn('chain end'); });</script>"
    )
    return origins_page(doc("<iframe src='http://hostile.test/n0.html'></iframe>" + top), documents)


# --- NodeIterator (ADR 0028 section 17) --------------------------------------


@case(
    "A million document.createNodeIterator() in a loop that keeps none (M9 feature 15a): the "
    "collector frees the objects and the next createNodeIterator removes their entries in the "
    "arena, so the entries do not pile up and each iterator works (ADR 0028 section 17)",
    swb_args=("--js", "--settle-ms", "500"),
    time_limit_s=10,
    rss_limit_mib=512,
    expect_log="node-iterators made 1000000",
)
def node_iterator_million_unkept_js_on() -> Page:
    script = (
        "<script>let ok = 0; for (let i = 0; i < 1000000; i++) { "
        "const it = document.createNodeIterator(document.body); "
        "if (it.nextNode() === document.body) ok++; } "
        "console.warn('node-iterators made ' + ok);</script>"
    )
    return Page(doc("<p>iterators</p>" + script))


@case(
    "100,000 live NodeIterator objects, each at the child of its own root, and a loop that "
    "removes the children (M9 feature 15a): every removal finds its iterator through the index "
    "of the nodes that iterators point at, so the loop costs the removals and not their product "
    "with the iterators; then 100,000 iterators at one node move together (ADR 0028 section 17)",
    swb_args=("--js", "--settle-ms", "500"),
    time_limit_s=15,
    rss_limit_mib=1024,
    expect_log="node-iterators moved 100000 100000",
)
def node_iterator_100000_live_with_removals_js_on() -> Page:
    script = (
        "<script>const a = document.createElement('div'); document.body.appendChild(a); "
        "const live = [], inner = []; "
        "for (let i = 0; i < 100000; i++) { const p = document.createElement('p'); "
        "p.appendChild(document.createElement('i')); a.appendChild(p); "
        "const it = document.createNodeIterator(p); it.nextNode(); it.nextNode(); "
        "live.push(it); inner.push(p.firstChild); } "
        "for (const c of inner) c.remove(); "
        "let moved = 0; for (const it of live) { if (it.referenceNode === it.root) moved++; } "
        "const shared = []; "
        "for (let i = 0; i < 100000; i++) { const it = document.createNodeIterator(a); "
        "it.nextNode(); it.nextNode(); shared.push(it); } "
        "a.firstChild.remove(); let together = 0; "
        "for (const it of shared) { if (it.referenceNode === a) together++; } "
        "console.warn('node-iterators moved ' + moved + ' ' + together);</script>"
    )
    return Page(doc("<p>iterators</p>" + script))


@case(
    "A chain of 100,000 nested elements walked to the end and back by 20 NodeIterator objects, "
    "then cut every 1,000 levels from the top with iterators inside (M9 feature 15a): the walk "
    "is iterative, each cut walks the cut subtree once, and the work counts toward the time limit "
    "of the task (ADR 0028 section 17)",
    swb_args=("--js", "--settle-ms", "500"),
    time_limit_s=15,
    rss_limit_mib=1024,
    expect_log="node-iterator chain done",
)
def node_iterator_chain_100000_js_on() -> Page:
    script = (
        "<script>const start = document.createElement('div'); document.body.appendChild(start); "
        "const nodes = [start]; let cur = start; "
        "for (let i = 0; i < 100000; i++) { const d = document.createElement('div'); "
        "cur.appendChild(d); cur = d; nodes.push(d); } "
        "const iterators = []; for (let i = 0; i < 20; i++) { "
        "const it = document.createNodeIterator(start); let n, k = 0; "
        "while (k++ < i * 5000 && it.nextNode()); iterators.push(it); } "
        "let steps = 0; const last = document.createNodeIterator(start); "
        "while (last.nextNode()) steps++; while (last.previousNode()) steps++; "
        "for (let k = 99000; k >= 1000; k -= 1000) nodes[k].remove(); "
        "nodes[1].remove(); "
        "let atTop = 0; for (const it of iterators) { if (it.referenceNode === start) atTop++; } "
        "console.warn('node-iterator chain done ' + steps + ' ' + atTop);</script>"
    )
    return Page(doc("<p>chain</p>" + script))


@case(
    "One NodeIterator at the bottom of a chain of 100,000 nested elements, and 100,000 "
    "appendChild and remove calls of an unrelated element (M9 feature 15a): with few "
    "iterators a removal looks for the pointers by walking up from them and through the removed "
    "subtree in turns, so its cost is the size of the removed subtree and not the depth of the "
    "pointers (ADR 0028 section 17)",
    swb_args=("--js", "--settle-ms", "500"),
    time_limit_s=10,
    rss_limit_mib=1024,
    expect_log="node-iterator unrelated 100000",
)
def node_iterator_unrelated_removals_in_a_deep_chain_js_on() -> Page:
    script = (
        "<script>const start = document.createElement('div'); document.body.appendChild(start); "
        "let cur = start; "
        "for (let i = 0; i < 100000; i++) { const d = document.createElement('div'); "
        "cur.appendChild(d); cur = d; } "
        "const it = document.createNodeIterator(start); while (it.nextNode()); "
        "const other = document.createElement('p'); let n = 0; "
        "for (let i = 0; i < 100000; i++) { "
        "document.body.appendChild(other); other.remove(); n++; } "
        "console.warn('node-iterator unrelated ' + n);</script>"
    )
    return Page(doc("<p>chain</p>" + script))


@case(
    "A chain of 100,000 nested elements cut from the top one node at a time with 11 NodeIterator "
    "objects (M9 feature 15a): with more than 8 iterators each cut walks the cut subtree, so the "
    "loop is quadratic. The work counts toward the time limit of the task, which ends the "
    "script (ADR 0028 sections 16 and 17)",
    swb_args=("--js", "--settle-ms", "500"),
    time_limit_s=30,
    rss_limit_mib=1024,
    expect_log="time limit",
)
def node_iterator_chain_cut_one_by_one_js_on() -> Page:
    script = (
        "<script>const start = document.createElement('div'); document.body.appendChild(start); "
        "let cur = start; "
        "for (let i = 0; i < 100000; i++) { const d = document.createElement('div'); "
        "cur.appendChild(d); cur = d; } "
        "const live = []; "
        "for (let i = 0; i < 10; i++) live.push(document.createNodeIterator(start)); "
        "const it = document.createNodeIterator(start); while (it.nextNode()); "
        "let n = start.firstChild; "
        "while (n) { const next = n.firstChild; n.remove(); n = next; } "
        "console.warn('node-iterator cut done');</script>"
    )
    return Page(doc("<p>chain</p>" + script))


@case(
    "A NodeFilter that removes the iframe that holds the iterator's document while nextNode runs "
    "(M9 feature 15a): the walk goes on in the detached document or ends, and nothing panics or "
    "hangs (ADR 0028 section 17, ADR 0031 section 5)",
    swb_args=("--js", "--settle-ms", "500"),
    time_limit_s=10,
    rss_limit_mib=512,
    expect_log="node-iterator frame gone",
)
def node_iterator_filter_removes_its_frame_js_on() -> Page:
    script = (
        "<script>addEventListener('load', () => { const f = document.getElementById('f'); "
        "const fd = f.contentDocument; let removed = 0; "
        "const it = fd.createNodeIterator(fd.body, NodeFilter.SHOW_ALL, (n) => { "
        "if (!removed++) f.remove(); return NodeFilter.FILTER_ACCEPT; }); "
        "let count = 0; while (it.nextNode()) count++; "
        "const it2 = fd.createNodeIterator(fd.body, NodeFilter.SHOW_ALL, (n) => { "
        "if (n.localName === 'b') { f.srcdoc = '<p>again</p>'; } return 1; }); "
        "let more = 0; while (it2.nextNode()) more++; "
        "console.warn('node-iterator frame gone ' + (count > 0) + ' ' + (more >= 0)); });</script>"
    )
    frame = "<iframe id=f srcdoc='<body><p>a<b>b</b></p><i>c</i></body>'></iframe>"
    return Page(doc(frame + script))


@case(
    "A NodeFilter that makes a NodeIterator with itself as the filter and walks it, so the "
    "recursion goes on until the stack limit (M9 feature 15a): the RangeError of the stack "
    "ends the recursion, the active flag of each iterator is off again, and the page goes on "
    "(ADR 0028 sections 7 and 17)",
    swb_args=("--js", "--settle-ms", "500"),
    time_limit_s=10,
    rss_limit_mib=512,
    expect_log="node-iterator recursion RangeError",
)
def node_iterator_filter_recursion_js_on() -> Page:
    script = (
        "<script>let depth = 0; function filter(n) { depth++; "
        "const it = document.createNodeIterator(document.body, 1, filter); "
        "it.nextNode(); return 1; } "
        "let name = 'none'; "
        "try { const it = document.createNodeIterator(document.body, 1, filter); "
        "it.nextNode(); } catch (e) { name = e.name; } "
        "const again = document.createNodeIterator(document.body, 1, null); "
        "console.warn('node-iterator recursion ' + name + ' ' + (depth > 100) + ' ' + "
        "(again.nextNode() === document.body));</script>"
    )
    return Page(doc("<p>recursion</p>" + script))


@case(
    "A NodeFilter that throws at every call, in a loop of a million nextNode calls (M9 feature "
    "15a): each exception leaves the position and the active flag as they were, and the catch "
    "in the loop keeps running (ADR 0028 section 17)",
    swb_args=("--js", "--settle-ms", "500"),
    time_limit_s=15,
    rss_limit_mib=512,
    expect_log="node-iterator throws 1000000",
)
def node_iterator_filter_throws_in_a_loop_js_on() -> Page:
    script = (
        "<script>const it = document.createNodeIterator(document.body, 4294967295, "
        "() => { throw new Error('no'); }); let caught = 0; "
        "for (let i = 0; i < 1000000; i++) { try { it.nextNode(); } catch (e) { caught++; } } "
        "console.warn('node-iterator throws ' + caught + ' ' + (it.referenceNode === "
        "document.body));</script>"
    )
    return Page(doc("<p>throws</p>" + script))
