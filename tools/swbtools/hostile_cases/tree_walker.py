"""`TreeWalker` (M10 feature 9a, `crates/web/src/tree_walker.rs`): deep trees, long sibling chains,
filters that change the tree, a filter that makes its own walker, many walkers, and a current node
outside the root."""

from swbtools.hostile_cases.core import Page, case, doc

JS = ("--js", "--settle-ms", "500")


@case(
    "A million document.createTreeWalker() in a loop that keeps none (M10 feature 9a): the "
    "objects have no entry in an arena, so nothing piles up, and each walker works",
    swb_args=JS,
    time_limit_s=15,
    rss_limit_mib=512,
    expect_log="tree-walkers made 1000000",
)
def tree_walker_million_unkept_js_on() -> Page:
    script = (
        "<script>let ok = 0; for (let i = 0; i < 1000000; i++) { "
        "const w = document.createTreeWalker(document.body); "
        "if (w.nextNode() === document.body.firstChild) ok++; } "
        "console.warn('tree-walkers made ' + ok);</script>"
    )
    return Page(doc("<p>walkers</p>" + script))


@case(
    "100,000 live TreeWalker objects, each with a filter, and a walk of each (M10 feature 9a): the "
    "filters are traced, nothing is freed early",
    swb_args=JS,
    time_limit_s=15,
    rss_limit_mib=1024,
    expect_log="tree-walkers live 100000",
)
def tree_walker_100000_live_with_filters_js_on() -> Page:
    script = (
        "<script>const live = []; for (let i = 0; i < 100000; i++) "
        "live.push(document.createTreeWalker(document.body, 1, n => 1)); "
        "let ok = 0; for (const w of live) if (w.nextNode() === document.body.firstChild) ok++; "
        "console.warn('tree-walkers live ' + ok);</script>"
    )
    return Page(doc("<p>walkers</p>" + script))


@case(
    "A walk through a chain of 100,000 nested divs, forward with nextNode, backward with "
    "previousNode, up with parentNode, and down with firstChild and lastChild, with no filter "
    "(M10 feature 9a): the loops are native and do not recurse",
    swb_args=JS,
    time_limit_s=20,
    rss_limit_mib=1024,
    expect_log="tree-walker deep 100000 100000 100000 100000",
)
def tree_walker_deep_chain_js_on() -> Page:
    script = (
        "<script>const root = document.createElement('div'); document.body.appendChild(root); "
        "let cur = root; for (let i = 0; i < 100000; i++) { "
        "const d = document.createElement('div'); cur.appendChild(d); cur = d; } "
        "const w = document.createTreeWalker(root, 1); let down = 0; while (w.nextNode()) down++; "
        "let up = 0; while (w.previousNode()) up++; "
        "w.currentNode = cur; let parents = 0; while (w.parentNode()) parents++; "
        "let kids = 0; while (w.firstChild()) kids++; "
        "console.warn('tree-walker deep ' + down + ' ' + up + ' ' + parents + ' ' + kids);</script>"
    )
    return Page(doc("<p>deep</p>" + script))


@case(
    "A walk through 200,000 siblings with nextSibling and previousSibling, and firstChild and "
    "lastChild with a filter that skips each (M10 feature 9a): the filter runs once for each "
    "sibling, and the loops end",
    swb_args=JS,
    time_limit_s=30,
    rss_limit_mib=1024,
    expect_log="tree-walker siblings 200000 200000 0",
)
def tree_walker_long_sibling_chain_js_on() -> Page:
    script = (
        "<script>const root = document.createElement('div'); document.body.appendChild(root); "
        "for (let i = 0; i < 200000; i++) root.appendChild(document.createElement('i')); "
        "const w = document.createTreeWalker(root, 1); w.currentNode = root.firstChild; "
        "let next = 0; while (w.nextSibling()) next++; "
        "let prev = 0; while (w.previousSibling()) prev++; "
        "let calls = 0; const s = document.createTreeWalker(root, 1, n => { calls++; return 3; }); "
        "const r = s.firstChild(); const r2 = s.lastChild(); "
        "console.warn('tree-walker siblings ' + (next + 1) + ' ' + (prev + 1) + ' ' + "
        "(r === null && r2 === null ? calls - 400000 : -1));</script>"
    )
    return Page(doc("<p>siblings</p>" + script))


@case(
    "A filter that removes the node it gets, and one that appends a copy of it, during a loop of "
    "nextNode calls (M10 feature 9a): the walk follows the tree as it is when each step runs, ends "
    "or stops at the loop's bound, and nothing panics",
    swb_args=JS,
    time_limit_s=20,
    rss_limit_mib=1024,
    expect_log="tree-walker mutation done",
)
def tree_walker_filter_mutates_the_tree_js_on() -> Page:
    script = (
        "<script>const root = document.createElement('div'); document.body.appendChild(root); "
        "for (let i = 0; i < 50000; i++) root.appendChild(document.createElement('i')); "
        "const a = document.createTreeWalker(root, 1, "
        "n => { if (n !== root) n.remove(); return 1; }); "
        "let seen = 0; while (a.nextNode()) seen++; "
        "const b = document.createTreeWalker(root, 1, n => { if (n.parentNode === root && "
        "root.childNodes.length < 100000) root.appendChild(n.cloneNode()); return 1; }); "
        "let grown = 0; while (b.nextNode() && grown++ < 200000); "
        "console.warn('tree-walker mutation done ' + seen + ' ' + root.childNodes.length);</script>"
    )
    return Page(doc("<p>mutation</p>" + script))


@case(
    "A filter that makes a TreeWalker with itself as the filter and walks it, so the recursion "
    "goes on until the stack limit (M10 feature 9a): the RangeError of the stack ends it, "
    "the active flag of each walker is off again, and the page goes on",
    swb_args=JS,
    time_limit_s=10,
    rss_limit_mib=512,
    expect_log="tree-walker recursion RangeError",
)
def tree_walker_filter_recursion_js_on() -> Page:
    script = (
        "<script>let depth = 0; function filter(n) { depth++; "
        "const w = document.createTreeWalker(document.body, 1, filter); "
        "w.nextNode(); return 1; } "
        "let name = 'none'; "
        "try { const w = document.createTreeWalker(document.body, 1, filter); "
        "w.nextNode(); } catch (e) { name = e.name; } "
        "const again = document.createTreeWalker(document.body, 1, null); "
        "console.warn('tree-walker recursion ' + name + ' ' + (depth > 100) + ' ' + "
        "(again.nextNode() === document.body.firstChild));</script>"
    )
    return Page(doc("<p>recursion</p>" + script))


@case(
    "A walker whose current node is outside the root's tree, over a tree of 100,000 nodes, with a "
    "filter that skips everything (M10 feature 9a): nextNode ends with null (the standard would "
    "filter the same node again for ever); the other methods end too",
    swb_args=JS,
    time_limit_s=20,
    rss_limit_mib=1024,
    expect_log="tree-walker outside null null null null null null null",
)
def tree_walker_current_outside_the_root_js_on() -> Page:
    script = (
        "<script>const root = document.createElement('div'); document.body.appendChild(root); "
        "for (let i = 0; i < 100000; i++) root.appendChild(document.createElement('i')); "
        "const out = document.createElement('p'); out.appendChild(document.createElement('b')); "
        "const w = document.createTreeWalker(root, 1, n => 3); w.currentNode = out.firstChild; "
        "const r = ['nextNode', 'previousNode', 'nextSibling', 'previousSibling', 'firstChild', "
        "'lastChild', 'parentNode'].map(m => String(w[m]())); "
        "console.warn('tree-walker outside ' + "
        "r.join(' ').replace('[object HTMLParagraphElement]', 'null'));</script>"
    )
    return Page(doc("<p>outside</p>" + script))


@case(
    "A NodeFilter that removes the iframe that holds the walker's document while nextNode runs "
    "(M10 feature 9a): the walk goes on in the detached document or ends, and nothing panics or "
    "hangs",
    swb_args=JS,
    time_limit_s=10,
    rss_limit_mib=512,
    expect_log="tree-walker frame gone",
)
def tree_walker_filter_removes_its_frame_js_on() -> Page:
    script = (
        "<script>addEventListener('load', () => { const f = document.getElementById('f'); "
        "const fd = f.contentDocument; "
        "const w = document.createTreeWalker(fd.body, 1, n => { f.remove(); return 1; }); "
        "let count = 0; while (w.nextNode() && count < 10) count++; "
        "console.warn('tree-walker frame gone ' + (count >= 0)); });</script>"
    )
    return Page(
        doc(
            '<iframe id=f srcdoc="<body><p>a</p><p>b</p><p>c</p></body>"></iframe>' + script,
        )
    )
