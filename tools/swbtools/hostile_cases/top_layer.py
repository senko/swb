"""Popovers and the top layer (ADR 0037 section 12; sub-feature 6a: the
popover model, the algorithms and the bindings)."""

from swbtools.hostile_cases.core import Page, case, doc

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
    "and 12; the dismissal comes with sub-feature 6c)",
    time_limit_s=20,
    expect_log="nested ok 10000",
    swb_args=JS,
    # ADR 0037 section 12: memory under +50 MB (a page with scripts and a
    # small DOM peaks near 105 MiB; this case near 111 MiB).
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
