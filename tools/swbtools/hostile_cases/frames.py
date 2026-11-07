"""Frames: input in frames and nodes across documents (ADR 0031)."""

import json

from swbtools.hostile_cases.core import (
    JS,
    REPLAY_URL,
    Page,
    case,
    doc,
    origins_page,
    steps_args,
)

# --- Input in frames (ADR 0031 section 8) ------------------------------------

_OVERLAP = "iframe{position:absolute;left:0;top:0;width:300px;height:200px;border:0}"


@case(
    "1,000 frames over each other, each with a document of 30 boxes, and 300 pointer moves "
    "across them, a click and a wheel turn, with JavaScript off (M9 feature 11): a hit test "
    "scans the display list of the page and goes into the topmost frame only, so a move costs "
    "the same as on a page without frames plus one frame (ADR 0031 section 8)",
    swb_args=(
        "--steps",
        json.dumps(
            [{"hover": [5 + i % 280, 5 + i % 190]} for i in range(300)]
            + [{"click": [40, 40]}, {"wheel": [0, 100]}]
        ),
    ),
    time_limit_s=2,
    rss_limit_mib=1100,
)
def frames_1000_over_each_other_pointer_moves() -> Page:
    boxes = "<div style='height:6px;background:#ccc'></div>" * 30
    return Page(
        doc("<iframe src=f.html></iframe>" * 1000, _OVERLAP),
        {"f.html": "<body style='margin:0'>" + boxes},
    )


@case(
    "1,000 frames over each other with JavaScript on and mouse listeners on the page "
    "and in the frames (M9 feature 11): each pointer move is a task that fires the page's "
    "events for the `iframe` only; the frames run no scripts (ADR 0031 section 8)",
    swb_args=steps_args(
        [{"hover": [5 + i % 280, 5 + i % 190]} for i in range(300)]
        + [{"click": [40, 40]}, {"wheel": [0, 100]}]
    ),
    time_limit_s=2,
    rss_limit_mib=920,
)
def frames_1000_over_each_other_pointer_moves_js_on() -> Page:
    script = (
        "<script>let n = 0; for (const t of ['mouseover', 'mouseout', 'mousemove', 'click', "
        "'wheel']) document.addEventListener(t, () => n++); "
        "setTimeout(() => console.log('events ' + n), 2000);</script>"
    )
    return Page(
        doc(script + "<iframe src=f.html></iframe>" * 1000, _OVERLAP),
        {"f.html": "<body style='margin:0'><p>frame</p>"},
    )


def _nested_transformed(depth: int) -> str:
    """The outermost of `depth` frames inside each other (through `srcdoc`), each with a
    rotation, a rounded border box and a clip, around a red box."""
    inner = "<body style='margin:0'><div style='width:20px;height:20px;background:#c00'></div>"
    frame = ""
    for _ in range(depth):
        frame = (
            "<iframe style='position:absolute;left:4px;top:4px;width:calc(100% - 8px);"
            "height:calc(100% - 8px);border:1px solid #888;transform:rotate(0.3deg);"
            "border-radius:6px;clip-path:inset(0 round 6px)' srcdoc=\""
            + inner.replace("&", "&amp;").replace('"', "&quot;")
            + '"></iframe>'
        )
        inner = "<body style='margin:0'>" + frame
    return frame


@case(
    "Frames nested 40 deep through srcdoc, each with a rotation, a rounded border box and a "
    "clip-path, and 300 pointer moves over them with a click and a wheel turn (M9 feature "
    "11): nesting stops at 32 with one warning, the hit test goes through the 32 levels with "
    "their transforms and clips under its work budget, and the wheel chains through them (ADR "
    "0031 sections 8 and 11)",
    swb_args=(
        "--steps",
        json.dumps(
            [{"hover": [20 + i % 500, 20 + i % 300]} for i in range(300)]
            + [{"click": [200, 150]}, {"wheel": [0, 100]}]
        ),
    ),
    time_limit_s=1,
    rss_limit_mib=300,
    expect_log="nested 32 deep",
)
def frames_nested_32_hit_through_transforms_and_clips() -> Page:
    outer = "<div style='position:relative;width:800px;height:500px'>"
    return Page(doc("<p>nested</p>" + outer + _nested_transformed(40) + "</div>"))


@case(
    "1,000 frames that each have a focusable element, and 1,010 Tab keys (M9 feature 11): "
    "each Tab finds the next element of the page's order, enters the frame and finds its "
    "first element; the cost of a Tab is the cost of one document's order, not of all frames "
    "(ADR 0031 section 8)",
    swb_args=("--steps", json.dumps([{"key": "Tab"}] * 1010)),
    time_limit_s=2,
    rss_limit_mib=900,
)
def frames_1000_tab_through_focusable_elements() -> Page:
    return Page(
        doc(
            "<input>" + "<iframe src=f.html></iframe>" * 1000 + "<input>",
            "iframe{float:left;width:90px;height:30px;border:0}",
        ),
        {"f.html": "<body style='margin:0'><input style='width:60px'>"},
    )


@case(
    "1,000 frames whose documents have no focusable element, and 1,010 Tab keys (M9 feature "
    "11): each frame is a stop of its own, the focus then goes on after the `iframe`; the page "
    "goes round more than once (ADR 0031 section 8)",
    swb_args=("--steps", json.dumps([{"key": "Tab"}] * 1010)),
    time_limit_s=2,
    rss_limit_mib=900,
)
def frames_1000_tab_without_focusable_elements() -> Page:
    return Page(
        doc(
            "<iframe src=f.html></iframe>" * 1000,
            "iframe{float:left;width:90px;height:30px;border:0}",
        ),
        {"f.html": "<body style='margin:0'><p>nothing to focus</p>"},
    )


@case(
    "A link of the page with `target` of a frame that a script clicks 5,000 times in a loop "
    "with JavaScript on (M9 feature 11): the frame navigates at most 64 times in 10 s of page "
    "time; later navigations are dropped with one warning, and the page stays responsive "
    "(ADR 0031 sections 8 and 11)",
    swb_args=("--js", "--settle-ms", "1500"),
    time_limit_s=5,
    rss_limit_mib=250,
    expect_log="navigated 64 times",
)
def frame_target_link_clicked_in_a_loop_js_on() -> Page:
    script = (
        "<script>let n = 0; const timer = setInterval(() => { "
        "document.getElementById('l').click(); if (++n >= 5000) clearInterval(timer); }, 0);"
        "</script>"
    )
    body = "<a id=l href='r.html?x' target=fr>go</a><iframe name=fr src=r.html></iframe>" + script
    return Page(doc(body), {"r.html": "<p>reply</p>"})


@case(
    "A link in a frame that points to its own document, clicked 300 times (M9 feature 11): "
    "each click navigates the frame; the frame navigates at most 64 times in 10 s of page "
    "time, later navigations are dropped with one warning (ADR 0031 sections 8 and 11)",
    swb_args=(
        "--steps",
        json.dumps([{"click": [20, 20]}] * 300),
        "--settle-ms",
        "500",
    ),
    time_limit_s=2,
    rss_limit_mib=200,
    expect_log="navigated 64 times",
)
def frame_self_link_clicked_300_times() -> Page:
    return Page(
        doc(
            "<iframe src=l.html style='position:absolute;left:0;top:0;width:200px;height:100px;"
            "border:0'></iframe>"
        ),
        {
            "l.html": (
                "<body style='margin:0'><a href=l.html "
                "style='display:block;width:150px;height:50px'>again</a>"
            )
        },
    )


@case(
    "A script reads window.x through 1,000,000 different property names, with JavaScript on: "
    "every read misses the inline cache of the WindowProxy (M9 feature 12a)",
    swb_args=JS,
    time_limit_s=2,
    rss_limit_mib=420,
)
def window_proxy_million_names_js_on() -> Page:
    script = (
        "<script>var hits = 0; for (var i = 0; i < 1000000; i++) "
        "{ if (window['n' + i] === undefined) hits++; } "
        "document.title = String(hits);</script>"
    )
    return Page(doc(script))


@case(
    "A Proxy chain 1,000 deep over window: reads, writes and has through it, with JavaScript "
    "on: the recursion budget of proxies acts, and no stack overflow (M9 feature 12a)",
    swb_args=JS,
    time_limit_s=1,
    rss_limit_mib=220,
)
def window_proxy_proxy_chain_js_on() -> Page:
    script = (
        "<script>var p = window; for (var i = 0; i < 1000; i++) { p = new Proxy(p, {}); } "
        "var out = 0; for (var k = 0; k < 200; k++) { p.chain = k; "
        "if ('chain' in p) out += p.chain; Object.getPrototypeOf(p); } "
        "document.title = String(out);</script>"
    )
    return Page(doc(script))


@case(
    "Object.setPrototypeOf(obj, window) in a loop with reads of its properties, with "
    "JavaScript on: each read walks through the WindowProxy on the generic path (M9 feature "
    "12a)",
    swb_args=JS,
    time_limit_s=2,
    rss_limit_mib=380,
)
def window_proxy_as_prototype_js_on() -> Page:
    script = (
        "<script>var seen = 0; window.shared = 1; for (var i = 0; i < 300000; i++) "
        "{ var o = {own: i}; Object.setPrototypeOf(o, window); seen += o.shared + o.own; "
        "o.written = i; if (o.Array !== Array) seen = 0; } "
        "document.title = String(seen);</script>"
    )
    return Page(doc(script))


@case(
    "A script adds a new key to window and reads window.x in a loop, with JavaScript on: each "
    "add moves the watch epoch, so each read fills the inline cache of the WindowProxy again "
    "(M9 feature 12a)",
    swb_args=JS,
    time_limit_s=4,
    rss_limit_mib=850,
)
def window_proxy_new_key_each_read_js_on() -> Page:
    script = (
        "<script>window.x = 1; var sum = 0; for (var i = 0; i < 1000000; i++) "
        "{ window['k' + i] = 1; sum += window.x; } "
        "document.title = String(sum);</script>"
    )
    return Page(doc(script))


@case(
    "Object.getOwnPropertyNames(window) in a loop, with JavaScript on: each call lists the keys "
    "of the target through the WindowProxy (M9 feature 12a)",
    swb_args=JS,
    time_limit_s=1,
    rss_limit_mib=240,
)
def window_proxy_own_names_loop_js_on() -> Page:
    script = (
        "<script>var n = 0; for (var i = 0; i < 1000; i++) "
        "{ n += Object.getOwnPropertyNames(window).length; } "
        "document.title = String(n);</script>"
    )
    return Page(doc(script))


@case(
    "A frame's script makes and removes about:blank frames in a loop, with contentWindow read for "
    "each (M9 feature 12b1): each read makes a realm until the budget of 256 realms of the "
    "runtime; later frames have a window without a target (ADR 0031 section 11)",
    swb_args=JS,
    time_limit_s=2,
    rss_limit_mib=700,
    expect_log="realms, the limit",
)
def scripted_frames_blank_loop_js_on() -> Page:
    inner = (
        "<script>let n = 0; for (let i = 0; i < 20000; i++) { "
        "const f = document.createElement('iframe'); document.documentElement.appendChild(f); "
        "if (f.contentWindow) n++; f.remove(); } parent.document.title = String(n);</script>"
    )
    return Page(doc(f'<iframe srcdoc="{inner}"></iframe>'))


@case(
    "location.reload() in the load event of a frame (M9 feature 12b1): the limit of 64 "
    "navigations of a frame in 10 s of page time stops it (ADR 0031 section 11)",
    swb_args=("--js", "--settle-ms", "1500"),
    time_limit_s=5,
    rss_limit_mib=340,
    expect_log="navigated 64 times",
)
def scripted_frame_reload_in_load_js_on() -> Page:
    inner = "<script>addEventListener('load', () => location.reload());</script><p>x"
    return Page(doc(f'<iframe srcdoc="{inner}"></iframe>'))


@case(
    "The load listener of an about:blank frame appends another frame, without end (M9 feature "
    "12b1): a recursion through appendChild, on a stack of natives and scripts; the limit of "
    "1,000 frames ends it (ADR 0031 section 11)",
    swb_args=JS,
    time_limit_s=1,
    rss_limit_mib=500,
)
def scripted_frame_append_in_load_js_on() -> Page:
    script = (
        "<script>let n = 0; function add() { const f = document.createElement('iframe'); "
        "f.onload = () => { n++; add(); }; document.body.appendChild(f); } "
        "try { add(); } catch (e) { console.log(e.name); } document.title = String(n);</script>"
    )
    return Page(doc(script))


@case(
    "The load listener of an about:blank frame appends a frame inside the frame's own document, "
    "without end (M9 feature 12b1): the limit of 32 levels ends it (ADR 0031 section 11)",
    swb_args=JS,
    time_limit_s=1,
    rss_limit_mib=320,
)
def scripted_frame_nest_in_load_js_on() -> Page:
    script = (
        "<script>let n = 0; function add(d) { const f = d.createElement('iframe'); "
        "f.onload = () => { n++; if (f.contentDocument) add(f.contentDocument); }; "
        "d.documentElement.appendChild(f); } add(document); document.title = String(n);</script>"
    )
    return Page(doc(script))


@case(
    "1,000 frames made in one task with contentWindow read for each (M9 feature 12b1): 256 "
    "realms, then windows without a target (ADR 0031 section 11)",
    swb_args=JS,
    time_limit_s=2,
    rss_limit_mib=1240,
    expect_log="realms, the limit",
)
def scripted_frames_1000_content_window_js_on() -> Page:
    script = (
        "<script>let n = 0; for (let i = 0; i < 1000; i++) { "
        "const f = document.createElement('iframe'); document.body.appendChild(f); "
        "if (f.contentWindow) n++; } document.title = String(n);</script>"
    )
    return Page(doc(script))


@case(
    "300 navigations of one frame by script, each document with a script (M9 feature 12b1): "
    "each document makes a realm until the budget of 256 realms; the frame's later documents "
    "run without scripts (ADR 0031 section 11)",
    swb_args=("--js", "--settle-ms", "3000"),
    time_limit_s=10,
    rss_limit_mib=260,
)
def scripted_frame_300_navigations_js_on() -> Page:
    script = (
        "<script>let n = 0; const f = document.querySelector('iframe'); "
        "function step() { if (n++ < 300) { f.srcdoc = '<script>parent.count = (parent.count | 0) "
        "+ 1<' + '/script>' + n; setTimeout(step, 200); } } step();</script>"
    )
    return Page(doc("<iframe></iframe>" + script))


@case(
    "One task sets src of an iframe 300,000 times with a sandbox value of about 900 KB, then "
    "changes sandbox (M9 feature 12b1): one parse gives the flags of all the sets, with no copy "
    "of the value per set",
    swb_args=JS,
    time_limit_s=1,
    rss_limit_mib=340,
)
def scripted_frame_sets_big_sandbox_js_on() -> Page:
    script = (
        "<script>const f = document.querySelector('iframe'); "
        "f.setAttribute('sandbox', 'allow-scripts '.repeat(64000)); "
        "for (let i = 0; i < 300000; i++) f.src = 'about:blank'; "
        "f.removeAttribute('sandbox');</script>"
    )
    return Page(doc("<iframe></iframe>" + script))


@case(
    "A frame's script removes its own iframe and goes on using document and window, with timers "
    "and a loop of DOM changes (M9 feature 12b1): the document retires, its parse ends, its "
    "timers do not run",
    swb_args=("--js", "--settle-ms", "500"),
    time_limit_s=2,
    rss_limit_mib=230,
)
def scripted_frame_removes_itself_js_on() -> Page:
    inner = (
        "<script>const P = parent; frameElement.remove(); "
        "for (let i = 0; i < 20000; i++) { document.body.appendChild(document.createElement('p')); "
        "setTimeout(() => {}, 0); window.x = document.body.children.length; "
        "getComputedStyle(document.body).width; document.body.offsetWidth; } "
        "P.document.title = String(window.x);</script><p>rest</p><script>never()</script>"
    )
    return Page(doc(f'<iframe srcdoc="{inner}"></iframe>'))


@case(
    "A kept contentWindow of a removed frame used in a loop: natives, setTimeout, setInterval, "
    "requestAnimationFrame, postMessage, location and appendChild (M9 feature 12b1): nothing "
    "waits in queues, and the nodes are within the node limit of a document",
    swb_args=("--js", "--settle-ms", "500"),
    time_limit_s=17,
    rss_limit_mib=310,
)
def scripted_retired_window_in_a_loop_js_on() -> Page:
    script = (
        "<script>const f = document.querySelector('iframe'); const w = f.contentWindow; "
        "f.remove(); "
        "for (let i = 0; i < 100000; i++) { w.setTimeout(() => {}, 0); w.setInterval(() => {}, 0); "
        "w.requestAnimationFrame(() => {}); w.postMessage(i, '*'); w.location.href = 'x.html'; "
        "w.document.body.appendChild(w.document.createElement('p')); w.innerWidth; w.length; "
        "w.frames[0]; w.parent; w.top; w.name; } "
        "document.title = String(w.document.body.children.length);</script>"
    )
    return Page(doc("<iframe></iframe>" + script))


@case(
    "Reads of the viewport of a frame after a change of its iframe's style, in a loop (M9 feature "
    "12b1): each read lays out the parent and the frame",
    swb_args=JS,
    time_limit_s=2,
    rss_limit_mib=220,
)
def scripted_frame_viewport_reads_js_on() -> Page:
    script = (
        "<script>const f = document.querySelector('iframe'); const w = f.contentWindow; let s = 0; "
        "for (let i = 0; i < 3000; i++) { f.style.width = (100 + i % 100) + 'px'; "
        "s += w.innerWidth; s += w.document.documentElement.clientWidth; } "
        "document.title = String(s);</script>"
    )
    return Page(doc("<iframe></iframe>" + script))


@case(
    "Window index keys and names of 1,000 frames in a loop (M9 feature 12b1): each read asks the "
    "frame registry; frames past the budget of realms have a window without a target",
    swb_args=JS,
    time_limit_s=2,
    rss_limit_mib=1240,
)
def scripted_window_index_keys_js_on() -> Page:
    frames = "".join(f"<iframe name=n{i}></iframe>" for i in range(1000))
    script = (
        "<script>let s = 0; for (let k = 0; k < 20; k++) { for (let i = 0; i < 1000; i++) { "
        "if (window[i] !== undefined) s++; if (window['n' + i] !== undefined) s++; } "
        "s += Object.keys(window).length; } document.title = String(s);</script>"
    )
    return Page(doc(frames + script))


@case(
    "A same-origin frame's script allocates to the heap limit of the runtime, with another frame "
    "and a frame below it to come (M9 feature 12b1): the runtime goes with its realms, and the "
    "frame documents parse to their ends without scripts",
    swb_args=JS,
    time_limit_s=1,
    rss_limit_mib=1070,
    expect_log="reached the heap limit",
)
def scripted_frame_heap_limit_js_on() -> Page:
    inner = (
        "<p>a</p><script>const big = []; for (;;) big.push(new Array(1000).fill(0));</script>"
        "<p>after</p><iframe srcdoc='<p>inner'></iframe>"
    )
    other = "<script src=nothing.js></script><p>b</p>"
    return Page(doc(f'<iframe srcdoc="{inner}"></iframe><iframe srcdoc="{other}"></iframe>'))


# --- Nodes across documents (ADR 0031 section 5) -----------------------------


@case(
    "A subtree that moves back and forth between two documents in a loop: each move adds nodes to "
    "an arena (ids are not reused), until the node limit ends the task",
    swb_args=JS,
    time_limit_s=3,
    rss_limit_mib=1300,
)
def nodes_back_and_forth_js_on() -> Page:
    script = (
        "<script>const d = frames[0].document; const e = document.createElement('div'); "
        "e.innerHTML = '<p>a</p><p>b</p><p>c</p>'; "
        "try { for (let i = 0; i < 1000000; i++) { d.body.appendChild(e); "
        "document.body.appendChild(e); } } "
        "catch (x) {} document.title = 'done';</script>"
    )
    return Page(doc("<iframe></iframe>" + script))


@case(
    "One observer on each of 200,000 children, and their parent moved across: the observer's "
    "registrations move in one pass, not one search per node",
    swb_args=JS,
    time_limit_s=3,
    rss_limit_mib=1200,
)
def nodes_observer_on_many_moved_nodes_js_on() -> Page:
    script = (
        "<script>const d = frames[0].document; const e = document.createElement('div'); "
        "const o = new MutationObserver(() => {}); "
        "for (let i = 0; i < 200000; i++) { const s = document.createElement('span'); "
        "e.appendChild(s); o.observe(s, {attributes: true}); } "
        "document.body.appendChild(e); d.body.appendChild(e); "
        "document.title = String(e.ownerDocument === d);</script>"
    )
    return Page(doc("<iframe></iframe>" + script))


@case(
    "One node moved across 400,000 times and a stale list read in a loop: the first read walks "
    "the chain of moves, and later reads find the place at once",
    swb_args=JS,
    time_limit_s=2,
    rss_limit_mib=430,
)
def nodes_stale_list_after_many_moves_js_on() -> Page:
    script = (
        "<script>const d = frames[0].document; const e = document.createElement('div'); "
        "e.id = 'n'; document.body.appendChild(e); const stale = document.querySelectorAll('#n'); "
        "for (let i = 0; i < 200000; i++) { d.body.appendChild(e); "
        "document.body.appendChild(e); } "
        "let same = 0; for (let i = 0; i < 20000; i++) { if (stale[0] === e) same++; } "
        "document.title = String(same);</script>"
    )
    return Page(doc("<iframe></iframe>" + script))


@case(
    "A chain of 1,000,000 elements moved across: the transplant is iterative, and the limits of "
    "the "
    "other arena end what does not fit",
    swb_args=JS,
    time_limit_s=4,
    rss_limit_mib=1600,
)
def nodes_deep_subtree_moves_js_on() -> Page:
    script = (
        "<script>const d = frames[0].document; const root = document.createElement('div'); "
        "let at = root; for (let i = 0; i < 1000000; i++) { const n = "
        "document.createElement('div'); "
        "at.appendChild(n); at = n; } "
        "const holder = d.createElement('div'); holder.appendChild(root); d.adoptNode(holder); "
        "document.adoptNode(root); document.title = String(root.ownerDocument === "
        "document);</script>"
    )
    return Page(doc("<iframe></iframe>" + script))


@case(
    "200,000 wrappers with listeners, handlers and same objects moved across one by one",
    swb_args=JS,
    time_limit_s=4,
    rss_limit_mib=2048,
)
def nodes_many_wrappers_move_js_on() -> Page:
    script = (
        "<script>const d = frames[0].document; const all = []; "
        "for (let i = 0; i < 200000; i++) { const n = document.createElement('i'); "
        "n.addEventListener('click', () => {}); n.onclick = () => {}; n.style.color = 'red'; "
        "n.classList.add('k'); "
        "all.push(n); document.body.appendChild(n); } "
        "for (const n of all) d.body.appendChild(n); document.title = "
        "String(d.body.children.length);</script>"
    )
    return Page(doc("<iframe></iframe>" + script))


@case(
    "A frame document's only node moves to the parent while the frame is removed, in a loop",
    swb_args=JS,
    time_limit_s=2,
    rss_limit_mib=800,
)
def nodes_only_node_leaves_a_removed_frame_js_on() -> Page:
    script = (
        "<script>for (let i = 0; i < 300; i++) { const f = document.createElement('iframe'); "
        "document.body.appendChild(f); const d = f.contentDocument, root = d.documentElement; "
        "d.addEventListener('x', () => {}); "
        "f.contentWindow.addEventListener('unload', () => { document.body.appendChild(root); }); "
        "f.remove(); d.adoptNode(root); document.body.appendChild(root); root.remove(); } "
        "document.title = 'done';</script>"
    )
    return Page(doc(script))


@case(
    "A MutationObserver, live lists and a static list on a subtree that moves away, whose document "
    "then retires; every object is read after that",
    swb_args=JS,
    time_limit_s=2,
    rss_limit_mib=300,
)
def nodes_observer_lists_of_a_retired_document_js_on() -> Page:
    script = (
        "<script>const f = document.querySelector('iframe'); const d = f.contentDocument; const "
        "saved = []; "
        "for (let i = 0; i < 2000; i++) { const e = d.createElement('div'); e.innerHTML = "
        "'<p>x</p><p>y</p>'; "
        "d.body.appendChild(e); const m = new MutationObserver(() => {}); m.observe(e, {subtree: "
        "true, childList: true}); "
        "const s = e.querySelectorAll('p'); saved.push([m, e, e.childNodes, "
        "e.getElementsByTagName('p'), s, e.style, e.classList, e.attributes]); "
        "e.setAttribute('a', i); document.body.appendChild(e); } "
        "f.remove(); let sum = 0; "
        "for (const [m, e, a, b, s, st, cl, at] of saved) { sum += a.length + b.length + s.length "
        "+ at.length; s[0].id; st.color = 'red'; cl.add('k'); "
        "e.appendChild(document.createElement('b')); sum += m.takeRecords().length; "
        "d.body.appendChild(e); sum += a.length; m.disconnect(); } "
        "document.title = String(sum);</script>"
    )
    return Page(doc("<iframe></iframe>" + script))


@case(
    "An `error` listener that throws, for a function of another window, in a loop of timers",
    swb_args=JS,
    time_limit_s=2,
    rss_limit_mib=300,
)
def nodes_error_listener_throws_across_windows_js_on() -> Page:
    script = (
        "<script>const w = frames[0]; let n = 0; "
        "addEventListener('error', () => { throw new Error('again'); }); "
        "w.addEventListener('error', () => { throw new Error('again'); }); "
        "function bad() { n++; throw new Error('x'); } "
        "const b = w.document.body; b.addEventListener('click', bad); "
        "for (let i = 0; i < 20000; i++) { w.setTimeout(bad, 0); b.click(); } "
        "document.title = 'done';</script>"
    )
    return Page(doc("<iframe></iframe>" + script))


XO_NAMES = (
    "['window', 'self', 'location', 'closed', 'frames', 'length', 'top', 'opener', 'parent', "
    "'blur', 'close', 'focus', 'postMessage', 'then']"
)
XO_FRAMES = (
    "const ws = []; for (let i = 0; i < 1000; i++) { const f = document.createElement('iframe'); "
    "f.sandbox = ''; document.body.appendChild(f); ws.push(f.contentWindow); } "
)


@case(
    "Every property of the cross-origin windows of 1,000 frames, read in a loop with their "
    "descriptors (M9 feature 13a): the functions are one per name and realm, so the memory stays",
    swb_args=JS,
    time_limit_s=1.5,
    rss_limit_mib=1000,
)
def cross_origin_all_properties_js_on() -> Page:
    script = (
        f"<script>{XO_FRAMES}const names = {XO_NAMES}; let n = 0; "
        "for (let round = 0; round < 20; round++) for (const w of ws) for (const k of names) { "
        "if (w[k] !== undefined) n++; Object.getOwnPropertyDescriptor(w, k); } "
        "document.title = String(n);</script>"
    )
    return Page(doc(script))


@case(
    "A loop that catches the SecurityError of a cross-origin window 1,000,000 times (M9 feature "
    "13a): each makes a DOMException and its message",
    swb_args=JS,
    time_limit_s=1.5,
    rss_limit_mib=400,
)
def cross_origin_security_error_loop_js_on() -> Page:
    script = (
        "<script>const f = document.createElement('iframe'); f.sandbox = ''; "
        "document.body.appendChild(f); const w = f.contentWindow; let n = 0; "
        "for (let i = 0; i < 1000000; i++) { try { w.document; } catch (e) { n++; } } "
        "document.title = String(n);</script>"
    )
    return Page(doc(script))


@case(
    "The control of cross-origin-security-error-loop-js-on: a same-origin DOMException in a loop "
    "of 1,000,000",
    swb_args=JS,
    time_limit_s=2,
    rss_limit_mib=300,
)
def same_origin_dom_exception_loop_js_on() -> Page:
    script = (
        "<script>let n = 0; for (let i = 0; i < 1000000; i++) { "
        "try { document.createElement(''); } catch (e) { n++; } } "
        "document.title = String(n);</script>"
    )
    return Page(doc(script))


@case(
    "w.location.href = x in a loop in one task, and in a timer for each of 200 tasks, on one "
    "cross-origin frame (M9 feature 13a): one navigation per task, and the limit of 64 navigations "
    "of a frame in 10 s of page time (ADR 0031 section 11)",
    swb_args=("--js", "--settle-ms", "1500"),
    time_limit_s=5,
    rss_limit_mib=290,
    expect_log="navigated 64 times",
)
def cross_origin_location_loop_one_frame_js_on() -> Page:
    script = (
        "<script>const f = document.createElement('iframe'); f.sandbox = ''; "
        "document.body.appendChild(f); const w = f.contentWindow; "
        "for (let i = 0; i < 100000; i++) w.location.href = 'data:text/html,a' + i; "
        "let n = 0; function step() { if (n++ < 200) { w.location.href = 'data:text/html,b' + n; "
        "setTimeout(step, 1); } } step();</script>"
    )
    return Page(doc(script))


@case(
    "w.location = x 200,000 times on a frame with a sandbox attribute of about 900 KB (M9 feature "
    "13a): the flags of the write are parsed once per change of the parent's DOM",
    swb_args=JS,
    time_limit_s=1,
    rss_limit_mib=230,
)
def cross_origin_location_loop_big_sandbox_js_on() -> Page:
    script = (
        "<script>const f = document.createElement('iframe'); "
        "f.setAttribute('sandbox', 'allow-scripts '.repeat(64000)); "
        "document.body.appendChild(f); const w = f.contentWindow; "
        "for (let i = 0; i < 200000; i++) w.location = 'data:text/html,a'; "
        "document.title = 'done';</script>"
    )
    return Page(doc(script))


@case(
    "w.location.href = x for each of 1,000 cross-origin frames, in three tasks (M9 feature 13a): "
    "a request of a navigation per frame after each task",
    swb_args=("--js", "--settle-ms", "500"),
    time_limit_s=4,
    rss_limit_mib=950,
)
def cross_origin_location_1000_frames_js_on() -> Page:
    script = (
        f"<script>{XO_FRAMES}let round = 0; function step() {{ "
        "for (let k = 0; k < 20; k++) for (const w of ws) "
        "w.location.href = 'data:text/html,a' + round; "
        "if (++round < 3) setTimeout(step, 50); } step();</script>"
    )
    return Page(doc(script))


@case(
    "A cross-origin window in the prototype chain of objects, a chain of 10,000 with it at the "
    "end, and Proxy objects whose target is it, read in loops (M9 feature 13a)",
    swb_args=JS,
    time_limit_s=3,
    rss_limit_mib=320,
)
def cross_origin_in_prototype_chains_js_on() -> Page:
    script = (
        "<script>const f = document.createElement('iframe'); f.sandbox = ''; "
        "document.body.appendChild(f); const w = f.contentWindow; let n = 0; "
        "const E = g => { try { g(); } catch (e) { n++; } }; "
        "const o = Object.create(w); const q = Object.setPrototypeOf({own: 1}, w); "
        "let long = w; for (let i = 0; i < 10000; i++) long = Object.create(long); "
        "let pp = w; for (let i = 0; i < 2000; i++) pp = new Proxy(pp, {}); "
        "for (let i = 0; i < 20000; i++) { E(() => o.x); E(() => q.x = 1); E(() => 'x' in o); "
        "if (o.closed === false) n++; } "
        "for (let i = 0; i < 300; i++) { E(() => long.x); E(() => 'x' in long); "
        "E(() => pp.closed); "
        "E(() => pp.x); E(() => Object.keys(pp)); E(() => String(long)); "
        "E(() => { for (const k in long) n++; }); } "
        "document.title = String(n);</script>"
    )
    return Page(doc(script))


@case(
    "The functions of cross-origin objects read from 256 realms, kept and called with every kind "
    "of this (M9 feature 13a): the table has at most 16 per realm, for the life of the runtime",
    swb_args=JS,
    time_limit_s=1,
    rss_limit_mib=800,
    expect_log="realms, the limit",
)
def cross_origin_functions_of_many_realms_js_on() -> Page:
    script = (
        "<script>const x = document.createElement('iframe'); x.sandbox = ''; "
        "document.body.appendChild(x); const w = x.contentWindow; const kept = []; const names = "
        f"{XO_NAMES}; "
        "for (let i = 0; i < 300; i++) { const f = document.createElement('iframe'); "
        "document.body.appendChild(f); if (!f.contentDocument) continue; "
        "const r = f.contentWindow; "
        "const read = r.eval('(o, k) => o[k]'); for (const k of names) kept.push(read(w, k)); "
        "kept.push(read(w.location, 'replace')); } let n = 0; "
        "for (const g of kept) if (typeof g === 'function') "
        "for (const t of [w, window, {}, 1, undefined]) "
        "{ try { if (g.name !== 'replace') g.call(t); } catch (e) { n++; } } "
        "document.title = String(n);</script>"
    )
    return Page(doc(script))


REPLAY_1500 = ("--js", "--replay", "{dir}", "--settle-ms", "1500")


@case(
    "40 frames of 40 origins, each with a script and a timer (M9 feature 13b): 32 runtimes, then "
    "a document runs without scripts, with one warning (ADR 0031 section 11)",
    swb_args=REPLAY_1500,
    url=REPLAY_URL,
    time_limit_s=5.0,
    rss_limit_mib=320,
    expect_log="script runtimes, the limit",
)
def agents_40_origins_js_on() -> Page:
    urls = [f"http://o{i}.hostile.test/x.html" for i in range(40)]
    frames = "".join(f"<iframe src='{u}'></iframe>" for u in urls)
    child = (
        "<!doctype html><script>let n = 0; setInterval(() => { n++; }, 5); "
        "console.log('frame ran');</script>"
    )
    end = "<script>addEventListener('load', () => console.log('end'));</script>"
    return origins_page(doc(frames + end), dict.fromkeys(urls, child))


ALLOCATE = (
    "<script>const a = []; setTimeout(() => { for (;;) a.push(new Array(10000).fill(1)); "
    "}, 10);</script>"
)


@case(
    "A frame of another origin that allocates to its heap limit while the top document keeps "
    "running (M9 feature 13b): its runtime goes alone",
    swb_args=REPLAY_1500,
    url=REPLAY_URL,
    time_limit_s=5.0,
    rss_limit_mib=900,
    expect_log="reached the heap limit",
)
def agents_frame_at_the_heap_limit_js_on() -> Page:
    top = (
        "<iframe src='http://o.hostile.test/x.html'></iframe><script>let t = 0; "
        "setInterval(() => { t++; if (t === 100) console.log('top still runs'); }, "
        "5);</script>"
    )
    return origins_page(doc(top), {"http://o.hostile.test/x.html": "<!doctype html>" + ALLOCATE})


FRAME_A = "http://a.hostile.test/x.html"
FRAME_B = "http://b.hostile.test/x.html"


def in_a_frame(child: str, top: str = "") -> Page:
    """A top level with a frame of another origin that runs `child`, a script."""
    return origins_page(
        doc(f"<iframe id=f src='{FRAME_A}'></iframe><script>{top}</script>"),
        {FRAME_A: f"<!doctype html><script>{child}</script>"},
    )


@case(
    "A frame that floods its parent with postMessage in a loop (M9 feature 14a): the budget of the "
    "sender stops it; the listener and a timer of the top level go on",
    swb_args=REPLAY_1500,
    url=REPLAY_URL,
    time_limit_s=6.0,
    expect_log=("messages (", "flood end", "top still runs"),
    rss_limit_mib=420,
)
def post_message_flood_of_the_parent_js_on() -> Page:
    child = (
        "let n = 0; const go = () => { for (let i = 0; i < 200000; i++) parent.postMessage(i,"
        " '*'); if (++n < 5) setTimeout(go, 0); else console.warn('flood end'); }; go();"
    )
    top = (
        "let got = 0, ticks = 0; addEventListener('message', () => { got++; }); setInterval(("
        ") => { if (++ticks === 50) console.warn('top still runs'); }, 5);"
    )
    return in_a_frame(child, top)


@case(
    "Two frames of two origins in an endless ping-pong, with a timer of the top that must go on "
    "(M9 feature 14a)",
    swb_args=REPLAY_1500,
    url=REPLAY_URL,
    time_limit_s=5.0,
    expect_log=("ping-pong runs", "top still runs"),
    rss_limit_mib=400,
)
def post_message_ping_pong_of_two_origins_js_on() -> Page:
    child = (
        "let n = 0; addEventListener('message', e => { if (++n === 1000) console.warn('ping-po"
        "ng runs'); (e.data === 1 ? parent.frames[1] : e.source).postMessage(0, '*'); });"
    )
    top = (
        "let ticks = 0; addEventListener('load', () => frames[0].postMessage(1, '*')); setInt"
        "erval(() => { if (++ticks === 100) console.warn('top still runs'); }, 5);"
    )
    page = f"<!doctype html><script>{child}</script>"
    return origins_page(
        doc(
            f"<iframe src='{FRAME_A}'></iframe><iframe src='{FRAME_B}'></iframe>"
            f"<script>{top}</script>"
        ),
        {FRAME_A: page, FRAME_B: page},
    )


@case(
    "A transfer of 10,000 ports to a frame of another origin (M9 feature 14a)",
    swb_args=REPLAY_1500,
    url=REPLAY_URL,
    time_limit_s=5.0,
    expect_log=("got 10000", "all ports answered"),
    rss_limit_mib=340,
)
def transfer_of_10000_ports_to_another_runtime_js_on() -> Page:
    child = (
        "addEventListener('message', e => { for (const p of e.ports) p.postMessage(1); consol"
        "e.warn('got ' + e.ports.length); });"
    )
    top = (
        "addEventListener('load', () => { const ports = []; const keep = []; let n = 0; for ("
        "let i = 0; i < 10000; i++) { const c = new MessageChannel(); ports.push(c.port2); ke"
        "ep.push(c.port1); c.port1.onmessage = () => { if (++n === 10000) console.warn('all po"
        "rts answered'); }; } f.contentWindow.postMessage(0, '*', ports); });"
    )
    return in_a_frame(child, top)


@case(
    "An endless port ping-pong across runtimes (M9 feature 14a): timers and frames get their turns",
    swb_args=REPLAY_1500,
    url=REPLAY_URL,
    time_limit_s=5.0,
    expect_log=("ping-pong runs", "top still runs"),
    rss_limit_mib=360,
)
def port_ping_pong_across_runtimes_js_on() -> Page:
    child = (
        "addEventListener('message', e => { const p = e.ports[0]; p.onmessage = () => p.postM"
        "essage(0); });"
    )
    top = (
        "addEventListener('load', () => { const c = new MessageChannel(); let n = 0, ticks = "
        "0; c.port1.onmessage = () => { if (++n === 1000) console.warn('ping-pong runs'); c.po"
        "rt1.postMessage(0); }; f.contentWindow.postMessage(0, '*', [c.port2]); c.port1.postM"
        "essage(0); setInterval(() => { if (++ticks === 100) console.warn('top still runs'); }"
        ", 5); });"
    )
    return in_a_frame(child, top)


@case(
    "200,000 channels, each with one end sent to another runtime and dropped (M9 feature "
    "14a): the port budget of the runtime that makes them stops it, and the collector and the "
    "sweep free them",
    swb_args=REPLAY_1500,
    url=REPLAY_URL,
    time_limit_s=5.0,
    expect_log="channels end",
    rss_limit_mib=560,
)
def channels_with_one_end_in_another_runtime_js_on() -> Page:
    child = (
        "let made = 0, refused = 0; const go = () => { for (let i = 0; i < 20000; i++) { try "
        "{ const c = new MessageChannel(); parent.postMessage(0, '*', [c.port2]); made++; } c"
        "atch (e) { refused++; } } if (made + refused < 200000) setTimeout(go, 0); else cons"
        "ole.warn('channels end ' + (made + refused)); }; go();"
    )
    top = "addEventListener('message', () => {});"
    return in_a_frame(child, top)


@case(
    "Ports sent through each other's queues across two runtimes in a chain of 100,000 (M9 feature "
    "14a): moves, drops and sweeps have no recursion",
    swb_args=REPLAY_1500,
    url=REPLAY_URL,
    time_limit_s=5.0,
    expect_log=("chain end", "dropped"),
    rss_limit_mib=400,
)
def ports_in_queues_in_a_long_chain_across_runtimes_js_on() -> Page:
    child = (
        "let keep = null, n = 0, refused = 0; addEventListener('message', e => { for (let i ="
        " 0; i < 25000; i++) { try { const c = new MessageChannel(); if (keep) c.port1.postMe"
        "ssage(0, [keep]); keep = c.port2; n++; } catch (x) { refused++; } } if (n + refused "
        "< 100000) { parent.postMessage(0, '*', [keep]); keep = null; } else { parent.postMes"
        "sage('close', '*', [keep]); console.warn('chain end ' + (n + refused)); } });"
    )
    top = (
        "let n = 0; addEventListener('load', () => f.contentWindow.postMessage(0, '*')); addE"
        "ventListener('message', e => { window.last = e.ports[0]; if (e.data === 'close') { w"
        "indow.last = null; console.warn('dropped'); } else { f.contentWindow.postMessage(0, '"
        "*'); } });"
    )
    return in_a_frame(child, top)


@case(
    "A large message close to the budget to a frame in a loop (M9 feature 14a)",
    swb_args=REPLAY_1500,
    url=REPLAY_URL,
    time_limit_s=5.0,
    expect_log=("frame got", "large end"),
    rss_limit_mib=2000,
)
def large_message_to_a_frame_in_a_loop_js_on() -> Page:
    child = (
        "let n = 0; addEventListener('message', e => { if (++n === 3) console.warn('frame got "
        "' + e.data.byteLength); });"
    )
    top = (
        "addEventListener('load', () => { let i = 0; const go = () => { f.contentWindow.postM"
        "essage(new Uint8Array(200 << 20), '*'); if (++i < 12) setTimeout(go, 20); else conso"
        "le.warn('large end'); }; go(); });"
    )
    return in_a_frame(child, top)


@case(
    "A flood to a frame that navigates in a loop (M9 feature 14a): the messages of a document that "
    "left go with it, with their bytes",
    swb_args=REPLAY_1500,
    url=REPLAY_URL,
    time_limit_s=5.0,
    expect_log="flood end",
    rss_limit_mib=870,
)
def flood_of_a_frame_that_navigates_in_a_loop_js_on() -> Page:
    child = (
        "addEventListener('load', () => { location.href = location.pathname === '/x.html' ? '"
        "/y.html' : '/x.html'; });"
    )
    top = (
        "let n = 0; addEventListener('load', () => { const go = () => { const w = f.contentWi"
        "ndow; for (let i = 0; i < 20000; i++) w.postMessage(new Uint8Array(1024), '*'); if ("
        "++n === 20) console.warn('flood end'); else if (n < 20) setTimeout(go, 5); }; go();"
        " });"
    )
    return origins_page(
        doc(f"<iframe id=f src='{FRAME_A}'></iframe><script>{top}</script>"),
        {
            FRAME_A: f"<!doctype html><script>{child}</script>",
            "http://a.hostile.test/y.html": f"<!doctype html><script>{child}</script>",
        },
    )


@case(
    "A flood to a frame that is removed (M9 feature 14a): the messages go with it",
    swb_args=REPLAY_1500,
    url=REPLAY_URL,
    time_limit_s=5.0,
    expect_log="flood end",
    rss_limit_mib=300,
)
def flood_of_a_frame_that_is_removed_js_on() -> Page:
    child = "let n = 0; addEventListener('message', () => { n++; });"
    top = (
        "let rounds = 0; addEventListener('load', () => { const go = () => { const old = docu"
        "ment.querySelector('iframe'); const w = old.contentWindow; for (let i = 0; i < 60000"
        "; i++) w.postMessage(i, '*'); const c = new MessageChannel(); w.postMessage(0, '*', "
        "[c.port2]); old.remove(); const b = document.createElement('iframe'); b.src = 'http:"
        "//a.hostile.test/x.html'; document.body.append(b); if (++rounds === 20) console.warn("
        "'flood end'); else b.onload = go; }; go(); });"
    )
    return in_a_frame(child, top)


@case(
    "Messages to a top level whose heap is near the room for messages (M9 feature 14a): those "
    "that do not fit give messageerror and force no collection of the large heap each",
    swb_args=REPLAY_1500,
    url=REPLAY_URL,
    time_limit_s=5.0,
    expect_log="near end 299",
    rss_limit_mib=800,
)
def messages_near_the_heap_limit_js_on() -> Page:
    child = (
        "const b = new Uint8Array(6 << 20); addEventListener('message', e => { parent.postMes"
        "sage(b, '*'); });"
    )
    top = (
        "let n = 0, errors = 0; const next = () => { if (++n === 300) console.warn('near end "
        "' + errors); else f.contentWindow.postMessage(0, '*'); }; addEventListener('message"
        "', next); addEventListener('messageerror', () => { errors++; next(); }); addEventLis"
        "tener('load', () => { window.fill = Array.from({length: 1040000}, () => ({})); windo"
        "w.keep = 'x'.repeat(95000000); keep.charCodeAt(1); next(); });"
    )
    return in_a_frame(child, top)


@case(
    "A frame whose runtime reaches the heap limit while messages and ports wait for it (M9 feature "
    "14a): they go with the runtime, and the top level's budget is free again",
    swb_args=REPLAY_1500,
    url=REPLAY_URL,
    time_limit_s=5.0,
    expect_log=("reached the heap limit", "top still runs"),
    rss_limit_mib=1100,
)
def runtime_at_the_heap_limit_with_waiting_messages_js_on() -> Page:
    child = (
        "addEventListener('message', e => { window.p = e.ports[0]; for (const q of e.ports) w"
        "indow.q = q; const a = []; setTimeout(() => { for (;;) a.push(new Array(10000).fill("
        "1)); }, 10); });"
    )
    top = (
        "addEventListener('load', () => { const c = new MessageChannel(); window.c = c; f.con"
        "tentWindow.postMessage(0, '*', [c.port2]); const fill = () => { const b = new Uint8A"
        "rray(1 << 20); for (let i = 0; i < 50; i++) c.port1.postMessage(b); }; fill(); setTi"
        "meout(() => { fill(); const d = new MessageChannel(); for (let i = 0; i < 50; i++) d"
        ".port1.postMessage(new Uint8Array(1 << 20)); console.warn('top still runs'); }, 1000)"
        "; });"
    )
    return in_a_frame(child, top)


@case(
    "Two frames of two origins, each allocating without end (M9 feature 13b): each task's limit "
    "is what the other frames leave of the 256 MiB that the top level's runtime does not keep",
    swb_args=REPLAY_1500,
    url=REPLAY_URL,
    time_limit_s=5.0,
    rss_limit_mib=960,
    expect_log="reached the heap limit",
)
def agents_two_frames_at_the_page_cap_js_on() -> Page:
    urls = ["http://o1.hostile.test/x.html", "http://o2.hostile.test/x.html"]
    frames = "".join(f"<iframe src='{u}'></iframe>" for u in urls)
    child = (
        "<!doctype html><script>const a = []; setInterval(() => { for (let i = 0; i < 200; "
        "i++) a.push(new Array(10000).fill(1)); }, 1);</script>"
    )
    timer = (
        "<script>let t = 0; setInterval(() => { t++; if (t === 50) console.log('top still "
        "runs'); }, 5);</script>"
    )
    top = frames + timer
    return origins_page(doc(top), dict.fromkeys(urls, child))


@case(
    "A frame of another origin with an endless loop in each task (M9 feature 13b): the time limit "
    "of a task, three times, stops that runtime; the top document goes on",
    swb_args=("--js", "--replay", "{dir}", "--settle-ms", "16000"),
    url=REPLAY_URL,
    time_limit_s=40.0,
    rss_limit_mib=220,
    expect_log="reached a limit",
)
def agents_endless_loop_in_each_task_js_on() -> Page:
    child = (
        "<!doctype html><script>const go = () => { setTimeout(go, 1); for (;;); }; "
        "setTimeout(go, 1);</script>"
    )
    top = (
        "<iframe src='http://o.hostile.test/x.html'></iframe><script>let t = 0; "
        "setInterval(() => { t++; if (t === 20) console.log('top still runs'); }, "
        "5);</script>"
    )
    return origins_page(doc(top), {"http://o.hostile.test/x.html": child})


@case(
    "A frame of another origin that is removed while it runs a timer loop (M9 feature 13b)",
    swb_args=REPLAY_1500,
    url=REPLAY_URL,
    time_limit_s=5.0,
    rss_limit_mib=220,
)
def agents_removed_in_a_timer_loop_js_on() -> Page:
    child = (
        "<!doctype html><script>let n = 0; const go = () => { n++; setTimeout(go, 0); }; "
        "go();</script>"
    )
    top = (
        "<iframe id=f src='http://o.hostile.test/x.html'></iframe><script>"
        "setTimeout(() => { f.remove(); console.log('removed'); setTimeout(() => "
        "console.log('end'), 10); }, 100);</script>"
    )
    return origins_page(doc(top), {"http://o.hostile.test/x.html": child})


@case(
    "A frame of another origin whose unload never ends, removed by its parent (M9 feature 13b): "
    "the time limit of the task ends it, after the parent's task, and the top goes on",
    swb_args=("--js", "--replay", "{dir}", "--settle-ms", "6500"),
    url=REPLAY_URL,
    time_limit_s=16.0,
    rss_limit_mib=230,
    expect_log="time limit",
)
def agents_unload_loops_js_on() -> Page:
    child = "<!doctype html><script>addEventListener('unload', () => { for (;;); });</script>"
    top = (
        "<iframe id=f src='http://o.hostile.test/x.html'></iframe><script>"
        "setTimeout(() => { f.remove(); console.log('removed'); setTimeout(() => "
        "console.log('end'), 10); }, 100);</script>"
    )
    return origins_page(doc(top), {"http://o.hostile.test/x.html": child})


@case(
    "A chain of frames A > B > A > B to the depth limit (M9 feature 13b), each level with a "
    "script that reads parent, top and frames: two runtimes",
    swb_args=REPLAY_1500,
    url=REPLAY_URL,
    time_limit_s=5.0,
    rss_limit_mib=320,
)
def agents_chain_of_two_origins_js_on() -> Page:
    script = (
        "<script>let seen = 0; try { seen = window.parent.frames.length + "
        "window.top.frames.length; } catch (e) { seen = -1; } "
        "console.log('level ' + location.search + ' ' + seen);</script>"
    )
    documents = {}
    for level in range(1, 40):
        host = "o.hostile.test" if level % 2 else "hostile.test"
        next_host = "hostile.test" if level % 2 else "o.hostile.test"
        documents[f"http://{host}/c.html?{level}"] = (
            f"<!doctype html>{script}<iframe src='http://{next_host}/c.html?{level + 1}'></iframe>"
        )
    top = "<iframe src='http://o.hostile.test/c.html?1'></iframe>"
    return origins_page(doc(top), documents)


@case(
    "1,000 frames of one other origin, each with a script (M9 feature 13b): one runtime, and the "
    "realm budget of 256 stops the scripts of the rest",
    swb_args=REPLAY_1500,
    url=REPLAY_URL,
    time_limit_s=7.0,
    rss_limit_mib=1340,
    expect_log="realms, the limit",
)
def agents_1000_frames_of_one_origin_js_on() -> Page:
    child = "<!doctype html><script>window.m = 1;</script>"
    frames = "<iframe src='http://o.hostile.test/x.html'></iframe>" * 1000
    return origins_page(doc(frames), {"http://o.hostile.test/x.html": child})


@case(
    "A sandboxed frame with `allow-scripts` that makes frames with scripts in a loop (M9 feature "
    "13b): each document has an opaque origin, so the budget of 32 runtimes acts",
    swb_args=("--js", "--settle-ms", "1500"),
    time_limit_s=6.0,
    rss_limit_mib=900,
)
def agents_sandbox_makes_many_frames_js_on() -> Page:
    child = (
        "<script>for (let i = 0; i < 900; i++) { const f = document.createElement('iframe'); "
        "f.sandbox = 'allow-scripts'; f.srcdoc = '<scr' + 'ipt>window.m = 1;</scr' + 'ipt>'; "
        "document.body.append(f); } console.log('made');</script>"
    )
    srcdoc = f"<!doctype html><body>{child}"
    return Page(doc(f"<iframe sandbox='allow-scripts' srcdoc=\"{srcdoc}\"></iframe>"))


@case(
    "16 sandboxed frames, each with Blob URLs of 64 x 4 MiB (M9 feature 13b): the heaps stay "
    "small, and the page's budget of 256 MiB of Blob URLs acts across the 16 runtimes",
    swb_args=("--js", "--settle-ms", "1500"),
    time_limit_s=5.0,
    rss_limit_mib=1500,
    expect_log="too many Blob URLs",
)
def agents_blob_urls_of_sandboxed_frames_js_on() -> Page:
    child = (
        "<script>const a = new Uint8Array(4 << 20); const keep = []; try { for (let i = 0; "
        "i < 64; i++) keep.push(URL.createObjectURL(new Blob([a]))); } catch (e) { "
        "console.log(e.name); } console.log('frame end ' + keep.length);</script>"
    )
    srcdoc = f"<!doctype html>{child}"
    frame = f"<iframe sandbox='allow-scripts' srcdoc=\"{srcdoc}\"></iframe>"
    return Page(doc(frame * 16))


@case(
    "A top-level navigation from a frame of another origin in a loop (M9 feature 13b): without "
    "an activation each attempt throws, and it costs no memory",
    swb_args=REPLAY_1500,
    url=REPLAY_URL,
    time_limit_s=5.0,
    rss_limit_mib=300,
)
def agents_top_navigation_in_a_loop_js_on() -> Page:
    child = (
        "<!doctype html><script>let n = 0; for (let i = 0; i < 100000; i++) { try { "
        "top.location.href = 'http://hostile.test/index.html?' + i; } catch (e) { n++; } } "
        "console.log('refused ' + n);</script>"
    )
    top = "<iframe src='http://o.hostile.test/x.html'></iframe>"
    return origins_page(doc(top), {"http://o.hostile.test/x.html": child})
