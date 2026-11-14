"""Popovers and the top layer (ADR 0037 section 12; sub-feature 6a: the
popover model, the algorithms and the bindings; 6b: the top layer in
rendering)."""

import json

from swbtools.hostile_cases.core import Page, case, doc, steps_args

JS = ("--js", "--settle-ms", "500")


@case(
    "`beforetoggle` listeners that show their own element again (each show inside the event "
    "of the last, down to the stack limit: `RangeError`), that show and hide the other "
    "popovers 300 levels deep, a `toggle` listener that toggles its popover again at each "
    "event (a new task each time), and 10 rounds of `togglePopover()`: no crash, the toggle "
    "tasks keep the event loop responsive, and a timer logs `reentry ok` (ADR 0037 sections "
    "2 and 12)",
    time_limit_s=20,
    expect_log="reentry ok",
    swb_args=JS,
)
def popover_beforetoggle_reentry() -> Page:
    script = (
        "<script>const a = document.getElementById('a'), b = document.getElementById('b'), "
        "c = document.getElementById('c'); let n = 0, depth = 0, errors = 0; "
        "const t = f => { try { f(); } catch (e) { errors++; } }; "
        "a.addEventListener('beforetoggle', () => { n++; a.showPopover(); }); "
        "b.addEventListener('beforetoggle', () => { n++; if (depth++ < 300) { "
        "t(() => a.hidePopover()); t(() => c.togglePopover()); t(() => b.togglePopover()); "
        "} depth--; }); "
        "c.addEventListener('toggle', () => t(() => c.togglePopover())); "
        "for (let i = 0; i < 10; i++) { t(() => a.togglePopover()); t(() => b.togglePopover()); } "
        "setTimeout(() => console.warn('reentry ok ' + n + ' ' + errors), 100);</script>"
    )
    return Page(
        doc(
            "<div id=a popover>a</div><div id=b popover>b</div>"
            "<div id=c popover=manual>c</div>" + script
        )
    )


@case(
    "the `popover` attribute of an open popover changed 100,000 times between `auto` and "
    "`manual`, the popover shown again after each change: each change hides it with "
    "`beforetoggle` and coalesces its `toggle` task, and a timer logs `flip ok` (ADR 0037 "
    "sections 1 and 12)",
    time_limit_s=20,
    expect_log="flip ok",
    swb_args=JS,
)
def popover_attribute_flip_100000() -> Page:
    script = (
        "<script>const p = document.getElementById('p'); let events = 0; "
        "p.addEventListener('beforetoggle', () => events++); "
        "p.addEventListener('toggle', () => events++); "
        "const start = performance.now(); "
        "for (let i = 0; i < 100000; i++) { p.showPopover(); "
        "p.popover = i % 2 ? 'auto' : 'manual'; } "
        "const ms = Math.round(performance.now() - start); "
        "setTimeout(() => console.warn('flip ok ' + events + ' in ' + ms + ' ms'), 10);</script>"
    )
    return Page(doc("<div id=p popover>p</div>" + script))


@case(
    "a popover shown and its ancestor removed and inserted again, 100,000 times: each removal "
    "hides the popover without events, and a timer logs `remove ok` (ADR 0037 sections 1 "
    "and 12)",
    time_limit_s=20,
    expect_log="remove ok",
    swb_args=JS,
)
def popover_remove_while_open_100000() -> Page:
    script = (
        "<script>const w = document.getElementById('w'), p = document.getElementById('p'); "
        "let events = 0; p.addEventListener('beforetoggle', () => events++); "
        "const start = performance.now(); "
        "for (let i = 0; i < 100000; i++) { p.showPopover(); w.remove(); "
        "document.body.appendChild(w); } "
        "const ms = Math.round(performance.now() - start); "
        "setTimeout(() => console.warn('remove ok ' + events + ' ' + p.matches(':popover-open') "
        "+ ' in ' + ms + ' ms'), 10);</script>"
    )
    return Page(doc("<section id=w><div id=p popover>p</div></section>" + script))


@case(
    "10,000 nested auto popovers shown in order (each is the child of the one before): each "
    "show finds its ancestor at the top of the list in one step, the show loop takes less than "
    "2 s, and a timer logs `nested ok` with the number of open popovers (ADR 0037 sections 2 "
    "and 12). Sub-feature 6c: the script shows no popover with a user activation, so the 10,000 "
    "close watchers are alive in one group: Escape closes them all (`escape ok` logs the open "
    "popovers before and after), then the script shows them again and a click outside closes "
    "all of them by light dismiss (`dismiss ok`)",
    time_limit_s=20,
    expect_log=("nested ok 10000", "escape ok 10000 0", "dismiss ok 10000 0"),
    swb_args=steps_args(
        [
            {
                "eval": "window.__before = document.querySelectorAll(':popover-open').length, 1",
            },
            {"key": "Escape"},
            {
                "eval": "console.warn('escape ok ' + window.__before + ' ' + "
                "document.querySelectorAll(':popover-open').length), 1"
            },
            {"eval": "all.forEach(d => d.showPopover()), 1"},
            {
                "eval": "window.__open = document.querySelectorAll(':popover-open').length, 1",
            },
            {"click": [5, 5]},
            {
                "eval": "console.warn('dismiss ok ' + window.__open + ' ' + "
                "document.querySelectorAll(':popover-open').length), 1"
            },
            {"wait": 50},
        ]
    ),
    # ADR 0037 section 12: memory under +50 MB (a page with scripts and a
    # small DOM peaks near 105 MiB; this case near 111 MiB in 6a, near
    # 150 MiB when 6b rendered the 10,000 open popovers and their backdrops
    # with one style each, and near 120 MiB with equal styles shared).
    rss_limit_mib=160,
)
def popover_nested_auto_10000() -> Page:
    script = (
        "<script>let parent = document.body; const all = []; "
        "for (let i = 0; i < 10000; i++) { const d = document.createElement('div'); "
        "d.popover = 'auto'; parent.appendChild(d); all.push(d); parent = d; } "
        "const start = performance.now(); for (const d of all) d.showPopover(); "
        "const ms = Math.round(performance.now() - start); "
        "const open = all.filter(d => d.matches(':popover-open')).length; "
        "setTimeout(() => console.warn('nested ok ' + open + ' in ' + ms + ' ms'), 10);"
        "</script>"
    )
    return Page(doc(script))


@case(
    "10,000 open manual popovers, each a fixed box with its own position (sub-feature 6b: "
    "10,000 entries of the top layer, each with a backdrop box and a stacking context of its "
    "own after the root's): style, layout and paint, 100 pointer moves over them, then all "
    "hidden; a timer logs `manual ok` (ADR 0037 section 12)",
    time_limit_s=20,
    expect_log="manual ok 0",
    swb_args=steps_args(
        [{"hover": [10 + 7 * i, 10 + 5 * i]} for i in range(100)]
        + [
            {
                "eval": "document.querySelectorAll('[popover]').forEach(p => p.hidePopover()), "
                "setTimeout(() => console.warn('manual ok ' + "
                "document.querySelectorAll(':popover-open').length), 10)"
            },
            {"wait": 100},
        ]
    ),
    rss_limit_mib=400,
)
def popover_manual_10000_open() -> Page:
    script = (
        "<script>for (let i = 0; i < 10000; i++) { const d = document.createElement('div'); "
        "d.popover = 'manual'; d.textContent = 'popover ' + i; "
        "d.style.cssText = 'inset:auto;margin:0;left:' + (i % 700) + 'px;top:' + (i % 500) + 'px'; "
        "document.body.appendChild(d); d.showPopover(); }</script>"
    )
    return Page(doc(script))


@case(
    "1,000 open popovers whose backdrops have `backdrop-filter: blur(20px)` (sub-feature 6b: "
    "each backdrop fills the viewport and filters everything painted before it; ADR 0033's "
    "budgets of the filter work and the layers act; ADR 0037 section 12)",
    time_limit_s=20,
    expect_log="backdrops ok 1000",
    swb_args=JS,
    rss_limit_mib=1024,
)
def top_layer_backdrop_filter_1000() -> Page:
    script = (
        "<script>for (let i = 0; i < 1000; i++) { const d = document.createElement('div'); "
        "d.popover = 'manual'; d.textContent = 'p' + i; "
        "d.style.cssText = 'inset:auto;margin:0;left:' + (i % 700) + 'px;top:' + (i % 500) + 'px'; "
        "document.body.appendChild(d); d.showPopover(); } "
        "setTimeout(() => console.warn('backdrops ok ' + "
        "document.querySelectorAll(':popover-open').length), 10);</script>"
    )
    css = "[popover]::backdrop{backdrop-filter:blur(20px);background:rgba(0,0,255,.01)}"
    return Page(doc(script, css))


@case(
    "10,000 nested popovers, each with a button whose `popovertarget` names the next, and "
    "JavaScript off: 10 clicks along the chain, then a click outside (ADR 0037 sections 3, "
    "10 and 12, sub-feature 6c)",
    time_limit_s=20,
    rss_limit_mib=400,
    swb_args=(
        "--steps="
        + json.dumps([{"click": f"#b{i}"} for i in range(10)] + [{"click": [1, 1]}, {"wait": 50}]),
    ),
)
def invoker_chain_10000_js_off() -> Page:
    parts = ["<button id=b0 popovertarget=p0>b0</button>"]
    for i in range(10000):
        nxt = f"<button id=b{i + 1} popovertarget=p{i + 1}>b</button>" if i < 9999 else ""
        parts.append(f"<div id=p{i} popover=auto style='position:static'>{nxt}")
    parts.append("</div>" * 10000)
    return Page(doc("".join(parts)))


@case(
    "100,000 watchers made and destroyed: a script shows 100,000 popovers (auto and hint ones, "
    "each show hides the one before, so one is open at the end) without any user activation, "
    "then Escape; a timer logs `watchers ok` (ADR 0037 section 12, sub-feature 6c; watchers that "
    "stay alive together exist only with nested popovers, see `popover-nested-auto-10000`; the "
    "dialogs come with 6d)",
    time_limit_s=20,
    expect_log="watchers ok 0",
    rss_limit_mib=600,
    swb_args=steps_args(
        [
            {"key": "Escape"},
            {
                "eval": "console.warn('watchers ok ' + "
                "document.querySelectorAll(':popover-open').length), 1"
            },
            {"wait": 50},
        ]
    ),
)
def close_watchers_100000_without_activation() -> Page:
    script = (
        "<script>for (let i = 0; i < 100000; i++) { const d = document.createElement('div'); "
        "d.popover = i % 2 ? 'auto' : 'hint'; document.body.appendChild(d); d.showPopover(); }"
        "</script>"
    )
    return Page(doc(script))
