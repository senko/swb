"""The selection (M10 sub-feature 9b-2, ADR 0039 section 3, `crates/dom/src/ranges/selection.rs`,
`crates/web/src/selection.rs`): loops of selection changes, a listener that changes the
selection at every `selectionchange`, and a selection over many text nodes that the engine
paints and copies."""

from swbtools.hostile_cases.core import Page, case, doc

JS = ("--js", "--settle-ms", "500")


@case(
    "A million addRange and removeAllRanges calls with a new range each, then a million collapse "
    "calls (M10 sub-feature 9b-2): each change frees the old range entry when no object holds it, "
    "and the selectionchange events coalesce into one task per turn of the event loop",
    swb_args=JS,
    time_limit_s=30,
    rss_limit_mib=1024,
    expect_log="selection loop done 1",
)
def selection_add_remove_loop_js_on() -> Page:
    script = (
        "<script>const t = document.querySelector('p').firstChild; const s = getSelection(); "
        "let events = 0; document.addEventListener('selectionchange', () => events++); "
        "for (let i = 0; i < 1000000; i++) { const r = new Range(); r.setStart(t, i % 5); "
        "s.addRange(r); s.removeAllRanges(); } "
        "for (let i = 0; i < 1000000; i++) s.collapse(t, i % 5); "
        "setTimeout(() => console.warn('selection loop done ' + events), 100);</script>"
    )
    return Page(doc("<p>selection</p>" + script))


@case(
    "A selectionchange listener that changes the selection again at every event, and a loop of "
    "100,000 collapse calls (M10 sub-feature 9b-2): the document has one waiting event at most, "
    "so the listener makes one task per turn of the event loop and the page settles",
    swb_args=JS,
    time_limit_s=20,
    rss_limit_mib=1024,
    expect_log="selection listener running",
)
def selection_listener_loop_js_on() -> Page:
    script = (
        "<script>const t = document.querySelector('p').firstChild; const s = getSelection(); "
        "let n = 0; document.onselectionchange = () => { n++; s.collapse(t, n % 5); "
        "if (n === 100) console.warn('selection listener running'); }; "
        "for (let i = 0; i < 100000; i++) s.collapse(t, i % 5);</script>"
    )
    return Page(doc("<p>selection</p>" + script))


@case(
    "A selection over 100,000 text nodes in 100,000 spans, painted and copied "
    "(Selection.toString, the engine's copy code; M10 sub-feature 9b-2): the highlight and the "
    "copied text walk the tree once, by the keys of the tree order",
    swb_args=JS,
    time_limit_s=30,
    rss_limit_mib=1536,
    expect_log="selected text 500003",
)
def selection_100000_text_nodes_js_on() -> Page:
    script = (
        "<script>const a = document.createElement('div'); "
        "for (let i = 0; i < 100000; i++) { const s = document.createElement('span'); "
        "s.textContent = 'word '; a.appendChild(s); } a.append('end'); "
        "document.body.appendChild(a); getSelection().selectAllChildren(a); "
        "console.warn('selected text ' + String(getSelection()).length);</script>"
    )
    return Page(doc("<p>selection</p>" + script))
