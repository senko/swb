"""Live ranges (M10 sub-feature 9b-1, ADR 0039, `crates/dom/src/ranges.rs`,
`crates/web/src/range.rs`): many ranges made and dropped, many live ranges and DOM changes,
many boundary points in one parent, deep trees."""

from swbtools.hostile_cases.core import Page, case, doc

JS = ("--js", "--settle-ms", "500")


@case(
    "A million document.createRange() and setStart in a loop that keeps none (M10 sub-feature "
    "9b-1): the entries of the freed ranges leave the arena, and a DOM change after the loop "
    "stays cheap",
    swb_args=JS,
    time_limit_s=20,
    rss_limit_mib=1024,
    expect_log="ranges made 1000000",
)
def range_million_unkept_js_on() -> Page:
    script = (
        "<script>const p = document.querySelector('p'); let ok = 0; "
        "for (let i = 0; i < 1000000; i++) { const r = document.createRange(); "
        "r.setStart(p.firstChild, 1); if (r.startOffset === 1) ok++; } "
        "for (let i = 0; i < 1000; i++) { const t = document.createTextNode('x'); "
        "p.before(t); t.remove(); } "
        "console.warn('ranges made ' + ok);</script>"
    )
    return Page(doc("<p>ranges</p>" + script))


@case(
    "100,000 live ranges, each in the child of its own element, and a loop that removes the "
    "children (M10 sub-feature 9b-1): each removal finds its range through the index of the nodes "
    "that boundary points are at, so the loop costs the removals and not their product with the "
    "ranges",
    swb_args=JS,
    time_limit_s=20,
    rss_limit_mib=1024,
    expect_log="ranges moved 100000",
)
def range_100000_live_with_removals_js_on() -> Page:
    script = (
        "<script>const a = document.createElement('div'); document.body.appendChild(a); "
        "const live = [], inner = []; "
        "for (let i = 0; i < 100000; i++) { const p = document.createElement('p'); "
        "p.appendChild(document.createElement('i')); a.appendChild(p); "
        "const r = document.createRange(); r.selectNodeContents(p.firstChild); "
        "live.push(r); inner.push(p.firstChild); } "
        "for (const c of inner) c.remove(); "
        "let moved = 0; for (const r of live) { if (r.startContainer.localName === 'p' "
        "&& r.collapsed) moved++; } "
        "console.warn('ranges moved ' + moved);</script>"
    )
    return Page(doc("<p>ranges</p>" + script))


@case(
    "100,000 ranges with a boundary point in one parent and a loop of insertions before its first "
    "child (M10 sub-feature 9b-1): each insertion moves every boundary point after it, so the loop "
    "is quadratic; the work counts toward the time limit of the task, which ends the script "
    "(ADR 0039 section 6)",
    swb_args=JS,
    time_limit_s=30,
    rss_limit_mib=1024,
    expect_log="time limit",
)
def range_100000_in_one_parent_insertions_js_on() -> Page:
    script = (
        "<script>const a = document.createElement('div'); a.append('x'); "
        "document.body.appendChild(a); const live = []; "
        "for (let i = 0; i < 100000; i++) { const r = new Range(); r.setStart(a, 1); "
        "live.push(r); } "
        "for (let i = 0; i < 1000000; i++) "
        "a.insertBefore(document.createElement('i'), a.firstChild); "
        "console.warn('range insertions done');</script>"
    )
    return Page(doc("<p>ranges</p>" + script))


@case(
    "A range at the bottom of a chain of 100,000 nested elements: its common ancestor, "
    "comparisons, toString, and cuts of the chain from the top every 1,000 levels (M10 "
    "sub-feature 9b-1): nothing recurses, and each cut finds the range by a walk that takes "
    "turns with the climb from its boundary points",
    swb_args=JS,
    time_limit_s=20,
    rss_limit_mib=1024,
    expect_log="range chain done 1 true",
)
def range_deep_chain_js_on() -> Page:
    script = (
        "<script>const start = document.createElement('div'); document.body.appendChild(start); "
        "const nodes = [start]; let cur = start; "
        "for (let i = 0; i < 100000; i++) { const d = document.createElement('div'); "
        "cur.appendChild(d); cur = d; nodes.push(d); } "
        "cur.append('deep'); const r = new Range(); r.setStart(cur.firstChild, 1); "
        "r.setEnd(start, 1); const s = new Range(); s.setStart(start, 0); "
        "const same = r.commonAncestorContainer === start && String(r) === 'eep'; "
        "const order = r.compareBoundaryPoints(Range.START_TO_START, s); "
        "for (let k = 99000; k >= 1000; k -= 1000) nodes[k].remove(); nodes[1].remove(); "
        "console.warn('range chain done ' + order + ' ' + (same && r.startContainer === start));"
        "</script>"
    )
    return Page(doc("<p>chain</p>" + script))


@case(
    "100 ranges in 100,000 sibling text nodes, 1,000 splitText calls, a normalize of the parent "
    "and a loop of data changes (M10 sub-feature 9b-1): the steps look the nodes up in the index "
    "and the indexes of children are counted as work (each split and each removal of the "
    "normalize costs the index of the child and the boundary points at the parent, as the "
    "standard's steps do)",
    swb_args=JS,
    time_limit_s=15,
    rss_limit_mib=1024,
    expect_log="range texts done",
)
def range_text_siblings_js_on() -> Page:
    script = (
        "<script>const a = document.createElement('div'); document.body.appendChild(a); "
        "const live = [], texts = []; for (let i = 0; i < 100000; i++) { "
        "const t = document.createTextNode('ab'); a.appendChild(t); texts.push(t); "
        "if (i % 1000 === 0) { const r = new Range(); r.setStart(t, 1); "
        "r.setEnd(a, i + 1); live.push(r); } } "
        "for (let i = 0; i < 1000; i++) texts[i * 100].splitText(1); "
        "a.normalize(); for (let i = 0; i < 1000; i++) a.firstChild.insertData(0, 'z'); "
        "console.warn('range texts done ' + live.length);</script>"
    )
    return Page(doc("<p>texts</p>" + script))


@case(
    "100,000 ranges with boundary points in a parent of 100,000 children, then textContent = '' "
    "(M10 sub-feature 9b-1 review fix): all children leave in one native call, and the live range "
    "steps run once for all of them (one pass over the boundary points), not once per child",
    swb_args=JS,
    time_limit_s=20,
    rss_limit_mib=1024,
    expect_log="range clear done 100000 0 0",
)
def range_100000_text_content_clear_js_on() -> Page:
    script = (
        "<script>const a = document.createElement('div'); document.body.appendChild(a); "
        "for (let i = 0; i < 100000; i++) a.appendChild(document.createElement('i')); "
        "const live = []; "
        "for (let i = 0; i < 100000; i++) { const r = new Range(); r.setStart(a, 1); "
        "r.setEnd(a, 100000); live.push(r); } "
        "a.textContent = ''; "
        "let ok = 0; for (const r of live) if (r.startOffset === 0 && r.endOffset === 0 "
        "&& r.startContainer === a) ok++; "
        "console.warn('range clear done ' + ok + ' ' + live[0].endOffset + ' ' "
        "+ a.childNodes.length);</script>"
    )
    return Page(doc("<p>ranges</p>" + script))


@case(
    "A parent of 100,000 elements followed by 100,000 adjacent text nodes, a range with boundary "
    "points at the parent, and normalize() (M10 sub-feature 9b-1 review fix): the live range "
    "steps of the merges run once for the run of merged nodes, and the index of the first node "
    "is counted once",
    swb_args=JS,
    time_limit_s=20,
    rss_limit_mib=1024,
    expect_log="range normalize done 100001 100000 100001 true",
)
def range_100000_normalize_js_on() -> Page:
    script = (
        "<script>const a = document.createElement('div'); document.body.appendChild(a); "
        "for (let i = 0; i < 100000; i++) a.appendChild(document.createElement('i')); "
        "for (let i = 0; i < 100000; i++) a.appendChild(document.createTextNode('t')); "
        "const r = new Range(); r.setStart(a, 100000); r.setEnd(a, 200000); "
        "a.normalize(); "
        "console.warn('range normalize done ' + a.childNodes.length + ' ' + r.startOffset + ' ' "
        "+ r.endOffset + ' ' + (r.endContainer === a));</script>"
    )
    return Page(doc("<p>ranges</p>" + script))


@case(
    "100,000 ranges with boundary points in a parent of 100,000 elements, each followed by an "
    "empty text node, and normalize() (M10 sub-feature 9b-1 review fix): the steps of the "
    "removals are collected per parent and run in one pass over its boundary points",
    swb_args=JS,
    time_limit_s=20,
    rss_limit_mib=1024,
    expect_log="range normalize empty done 100000 100000 1 100000",
)
def range_100000_normalize_empty_js_on() -> Page:
    script = (
        "<script>const a = document.createElement('div'); document.body.appendChild(a); "
        "for (let i = 0; i < 100000; i++) { a.appendChild(document.createElement('i')); "
        "a.appendChild(document.createTextNode('')); } "
        "const live = []; "
        "for (let i = 0; i < 100000; i++) { const r = new Range(); r.setStart(a, 1); "
        "r.setEnd(a, 200000); live.push(r); } "
        "a.normalize(); "
        "let ok = 0; "
        "for (const r of live) if (r.startOffset === 1 && r.endOffset === 100000) ok++; "
        "console.warn('range normalize empty done ' + a.childNodes.length + ' ' + ok + ' ' "
        "+ live[0].startOffset + ' ' + live[0].endOffset);</script>"
    )
    return Page(doc("<p>ranges</p>" + script))


@case(
    "50,000 runs of two text nodes in a parent, each after an element with its own run, 50,000 "
    "ranges with boundary points in the parent and one in each element, and normalize() (M10 "
    "sub-feature 9b-1 review fix): one pass over the boundary points of each parent for all its "
    "runs, and an index count per parent that the runs in the elements do not restart",
    swb_args=JS,
    time_limit_s=20,
    rss_limit_mib=1024,
    expect_log="range normalize runs done 100000 50000 1 100000 1",
)
def range_50000_normalize_runs_js_on() -> Page:
    script = (
        "<script>const a = document.createElement('div'); document.body.appendChild(a); "
        "const inner = []; "
        "for (let i = 0; i < 50000; i++) { const b = document.createElement('b'); "
        "b.append('u', 'u'); a.append(b, 't', 't'); "
        "const r = new Range(); r.setStart(b, 2); inner.push(r); } "
        "const live = []; "
        "for (let i = 0; i < 50000; i++) { const r = new Range(); r.setStart(a, 1); "
        "r.setEnd(a, 150000); live.push(r); } "
        "a.normalize(); "
        "let ok = 0; "
        "for (const r of live) if (r.startOffset === 1 && r.endOffset === 100000) ok++; "
        "console.warn('range normalize runs done ' + a.childNodes.length + ' ' + ok + ' ' "
        "+ live[0].startOffset + ' ' + live[0].endOffset + ' ' + inner[0].startOffset);</script>"
    )
    return Page(doc("<p>ranges</p>" + script))


@case(
    "100,000 ranges with a boundary point in a parent and a fragment of 100,000 children inserted "
    "before its first child (M10 sub-feature 9b-1 review fix): the offsets shift once for all the "
    'nodes of the fragment, as the standard\'s "insert" does, not once per node',
    swb_args=JS,
    time_limit_s=20,
    rss_limit_mib=1024,
    expect_log="range fragment done 100001 100001 100001",
)
def range_100000_fragment_insertion_js_on() -> Page:
    script = (
        "<script>const a = document.createElement('div'); a.append('x'); "
        "document.body.appendChild(a); const live = []; "
        "for (let i = 0; i < 100000; i++) { const r = new Range(); r.setStart(a, 1); "
        "live.push(r); } "
        "const f = document.createDocumentFragment(); "
        "for (let i = 0; i < 100000; i++) f.appendChild(document.createElement('i')); "
        "a.insertBefore(f, a.firstChild); "
        "console.warn('range fragment done ' + a.childNodes.length + ' ' + "
        "live[0].startOffset + ' ' + live[99999].startOffset);</script>"
    )
    return Page(doc("<p>ranges</p>" + script))


@case(
    "cloneContents and extractContents of a range over 100,000 elements with a text node each "
    "(M10 sub-feature 9b-3): the copies and the moves are loops, and the live range steps of the "
    "moved children run once for the run of children",
    swb_args=JS,
    time_limit_s=20,
    rss_limit_mib=1536,
    expect_log="range contents done 200001 200001 0 1",
)
def range_contents_100000_subtree_js_on() -> Page:
    script = (
        "<script>const a = document.createElement('div'); document.body.appendChild(a); "
        "for (let i = 0; i < 100000; i++) { const b = document.createElement('b'); "
        "b.append('t'); a.appendChild(b); } a.append('end'); "
        "const r = new Range(); r.setStart(a, 0); r.setEnd(a.lastChild, 2); "
        "const c = r.cloneContents(); const x = r.extractContents(); "
        "const count = f => { let n = 0; const w = document.createTreeWalker(f); "
        "while (w.nextNode()) n++; return n; }; "
        "console.warn('range contents done ' + count(c) + ' ' + count(x) + ' ' + "
        "r.startOffset + ' ' + a.childNodes.length);</script>"
    )
    return Page(doc("<p>contents</p>" + script))


@case(
    "A range from the bottom of a chain of 100,000 nested elements to a text after the chain, "
    "with cloneContents, extractContents, deleteContents and surroundContents (M10 sub-feature "
    "9b-3): the partially contained elements are copied by a loop down the path of the boundary "
    "point, without recursion",
    swb_args=JS,
    time_limit_s=20,
    rss_limit_mib=1536,
    expect_log="range chain contents done 100002 100002 d true",
)
def range_contents_deep_chain_js_on() -> Page:
    script = (
        "<script>const outer = document.createElement('div'); document.body.appendChild(outer); "
        "let cur = outer; "
        "for (let i = 0; i < 100000; i++) { const d = document.createElement('div'); "
        "cur.appendChild(d); cur = d; } "
        "cur.append('deep'); outer.append('tail'); "
        "const r = new Range(); r.setStart(cur.firstChild, 1); r.setEnd(outer.lastChild, 2); "
        "const count = f => { let n = 0; const w = document.createTreeWalker(f); "
        "while (w.nextNode()) n++; return n; }; "
        "const c = count(r.cloneContents()); const x = count(r.extractContents()); "
        "const left = cur.firstChild.data; "
        "const s = new Range(); s.setStart(cur.firstChild, 0); s.setEnd(outer.lastChild, 1); "
        "s.deleteContents(); "
        "const u = new Range(); u.selectNodeContents(outer); "
        "u.surroundContents(document.createElement('section')); "
        "console.warn('range chain contents done ' + c + ' ' + x + ' ' + left + ' ' "
        "+ (outer.firstChild.localName === 'section'));</script>"
    )
    return Page(doc("<p>chain</p>" + script))


@case(
    "100,000 live ranges in the children of a parent of 100,000 elements, 100,000 more at every "
    "offset of the parent, and deleteContents of all children (M10 sub-feature 9b-3): the "
    "removals of the run of children run the live range steps once, so the call costs the "
    "children plus the boundary points",
    swb_args=JS,
    time_limit_s=20,
    rss_limit_mib=1536,
    expect_log="range delete done 200000 0",
)
def range_delete_100000_live_js_on() -> Page:
    script = (
        "<script>const a = document.createElement('div'); document.body.appendChild(a); "
        "const live = []; for (let i = 0; i < 100000; i++) { "
        "const b = document.createElement('b'); a.appendChild(b); "
        "const r = new Range(); r.setStart(b, 0); live.push(r); } "
        "for (let i = 0; i < 100000; i++) { const r = new Range(); r.setStart(a, i + 1); "
        "live.push(r); } "
        "const all = new Range(); all.selectNodeContents(a); all.deleteContents(); "
        "let ok = 0; for (const r of live) if (r.startContainer === a && r.startOffset === 0 "
        "&& r.collapsed) ok++; "
        "console.warn('range delete done ' + ok + ' ' + a.childNodes.length);</script>"
    )
    return Page(doc("<p>delete</p>" + script))


@case(
    "createContextualFragment of 2 MB of markup and of 10,000 nested elements (M10 sub-feature "
    "9b-3): the fragment parser is iterative and the result stays within the DOM limits (deeper "
    "nesting is slow in the tree builder, as the known failure nest-div-100000 shows)",
    swb_args=JS,
    time_limit_s=20,
    rss_limit_mib=1536,
    expect_log="range fragment parse done 250000 div",
)
def range_contextual_fragment_large_js_on() -> Page:
    script = (
        "<script>const r = new Range(); r.selectNodeContents(document.body); "
        "const big = r.createContextualFragment('<p>x</p>'.repeat(250000)); "
        "const deep = r.createContextualFragment('<div>'.repeat(10000) + 'z'); "
        "document.body.append(deep); "
        "console.warn('range fragment parse done ' + big.childNodes.length + ' ' + "
        "document.body.lastChild.localName);</script>"
    )
    return Page(doc("<p>parse</p>" + script))


@case(
    "getClientRects and getBoundingClientRect of a range over 100,000 text nodes (M10 "
    "sub-feature 9b-3): one layout, one index of the text fragments, and a list of 100,000 "
    "rectangles charged to the heap",
    swb_args=JS,
    time_limit_s=30,
    rss_limit_mib=2048,
    expect_log="range rects done",
)
def range_client_rects_100000_texts_js_on() -> Page:
    script = (
        "<script>const a = document.createElement('div'); document.body.appendChild(a); "
        "for (let i = 0; i < 100000; i++) a.appendChild(document.createTextNode('w ')); "
        "const r = new Range(); r.selectNodeContents(a); const l = r.getClientRects(); "
        "const b = r.getBoundingClientRect(); "
        "console.warn('range rects done ' + (l.length > 0) + ' ' + (b.height > 0));</script>"
    )
    return Page(doc("<p>rects</p>" + script))
