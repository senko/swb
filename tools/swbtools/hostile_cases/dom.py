"""Scripts: loops over the DOM, events, style, geometry, history, storage and
cookies (ADR 0028)."""

import json

from swbtools.hostile_cases.core import (
    JS,
    REPLAY,
    REPLAY_URL,
    Page,
    case,
    doc,
    steps_args,
    web_font,
)

# --- Scripts: DOM, events, style, geometry, history, storage and cookies -----


@case(
    "A script that appends comments with appendChild until the node limit of 1,048,576, with "
    "JavaScript on: the task ends at the limit (ADR 0028 section 16)",
    expect_log="the DOM node limit",
    time_limit_s=10,
    swb_args=JS,
)
def dom_growth_append_child_js_on() -> Page:
    script = "<script>for (;;) document.body.appendChild(document.createComment(''));</script>"
    return Page(doc("<p>text</p>" + script + "<p>after</p>"))


@case(
    "A script that builds a chain of 100,000 nested divs with appendChild, with JavaScript on: "
    "style, layout (box depth limit 256) and paint run on a tree far deeper than the "
    "parser's limit of 512 (ADR 0028 section 16)",
    swb_args=JS,
)
def deep_append_child_js_on() -> Page:
    script = (
        "<script>let p = document.body; for (let i = 0; i < 100000; i++) "
        "{ const d = document.createElement('div'); p.appendChild(d); p = d; } "
        "p.textContent = 'deep'; console.log(document.body.cloneNode(true).textContent);</script>"
    )
    return Page(doc("<p>text</p>" + script + "<p>after</p>"))


@case(
    "100,000 elements with one id and a script that calls getElementById 100,000 times, with "
    "JavaScript on: the first element in tree order is found once per DOM generation, not by "
    "a walk at each call",
    swb_args=JS,
)
def get_element_by_id_many_js_on() -> Page:
    script = (
        "<script>let n = 0; for (let i = 0; i < 100000; i++) "
        "if (document.getElementById('same')) n++; console.log(n);</script>"
    )
    return Page(doc("<p>x</p>" * 50_000 + "<i id=same></i>" * 100_000 + script))


@case(
    "A script that clones a subtree of 20,000 nodes deeply 45 times, with JavaScript on: "
    "each clone is linear and the clones count against the node limit (900,000 nodes)",
    swb_args=JS,
    time_limit_s=10,
)
def clone_node_large_loop_js_on() -> Page:
    script = (
        "<script>const big = document.getElementById('big'); let c; "
        "for (let i = 0; i < 45; i++) c = big.cloneNode(true); "
        "console.log(c.childNodes.length);</script>"
    )
    return Page(doc("<div id=big>" + "<p>a<b>b</b></p>" * 5_000 + "</div>" + script))


@case(
    "A script that adds 100,000 class tokens with one classList.add call and then toggles "
    "and removes tokens in a loop, with JavaScript on: each call parses the attribute once",
    swb_args=JS,
    time_limit_s=10,
)
def class_list_many_tokens_js_on() -> Page:
    script = (
        "<script>const e = document.body; const tokens = []; "
        "for (let i = 0; i < 100000; i++) tokens.push('t' + i); e.classList.add(...tokens); "
        "for (let i = 0; i < 100; i++) { e.classList.toggle('x'); e.classList.remove('t' + i); } "
        "console.log(e.classList.length);</script>"
    )
    return Page(doc("<p>text</p>" + script))


@case(
    "A script that appends 2,000 elements to a parent and reads the length of the parent's "
    "children and of the new element's childNodes after each, with JavaScript on: the "
    "caches of the lists are rebuilt after each change and the stale ones are dropped",
    swb_args=JS,
    time_limit_s=10,
)
def list_caches_after_changes_js_on() -> Page:
    script = (
        "<script>const p = document.getElementById('p'); const all = []; "
        "for (let i = 0; i < 2000; i++) { const d = document.createElement('div'); "
        "p.appendChild(d); all.push(d.childNodes.length, d.children.length, p.children.length); } "
        "console.log(all.length, p.childNodes.length);</script>"
    )
    return Page(doc("<div id=p></div>" + script))


@case(
    "A script that adds 100,000 class tokens and then reads them with an indexed loop over "
    "classList.length and classList[i], with JavaScript on: the parsed tokens are kept for the "
    "DOM generation, so the loop is linear",
    swb_args=JS,
    time_limit_s=10,
)
def class_list_indexed_loop_js_on() -> Page:
    script = (
        "<script>const e = document.body; const tokens = []; "
        "for (let i = 0; i < 100000; i++) tokens.push('t' + i); e.classList.add(...tokens); "
        "const l = e.classList; let n = 0; "
        "for (let i = 0; i < l.length; i++) if (l[i] === 't' + i && l.item(i) === l[i]) n++; "
        "console.log(n);</script>"
    )
    return Page(doc("<p>text</p>" + script))


@case(
    "A script that builds a chain of 100,000 nested divs bottom-up (each new div takes the "
    "chain so far with appendChild), with JavaScript on: an insertion into a detached parent "
    "walks neither the inserted subtree nor the whole parent chain",
    swb_args=JS,
    time_limit_s=10,
)
def deep_append_child_bottom_up_js_on() -> Page:
    script = (
        "<script>let n = document.createElement('div'); for (let i = 0; i < 100000; i++) "
        "{ const w = document.createElement('div'); w.appendChild(n); n = w; } "
        "document.body.appendChild(n); console.log(document.body.childNodes.length);</script>"
    )
    return Page(doc("<p>text</p>" + script + "<p>after</p>"))


@case(
    "The same bottom-up chain of 100,000 nested divs on a page with a parsed form that holds "
    "a control, with JavaScript on: the form owner reset walks only the subtrees that were "
    "moved from another place",
    swb_args=JS,
    time_limit_s=10,
)
def deep_append_child_bottom_up_form_js_on() -> Page:
    script = (
        "<script>let n = document.createElement('div'); for (let i = 0; i < 100000; i++) "
        "{ const w = document.createElement('div'); w.appendChild(n); n = w; } "
        "document.getElementById('f').appendChild(n); console.log(document.body.childNodes.length);"
        "</script>"
    )
    return Page(doc("<form id=f><input name=a></form>" + script))


@case(
    "A script that sets innerHTML of a detached element to a text of 50 MB and reads innerHTML "
    "and outerHTML back, and sets innerHTML of a page element to 100,000 elements with 300-byte "
    "attributes (30 MB of markup), with JavaScript on: the fragment parser and the serializer "
    "make a few copies of the text, not one per character or per level (ADR 0028 section 16)",
    swb_args=JS,
    time_limit_s=10,
    rss_limit_mib=1024,
)
def inner_html_huge_markup_js_on() -> Page:
    script = (
        "<script>const d = document.createElement('div'); "
        "d.innerHTML = 'x'.repeat(50 * 1024 * 1024); "
        "const piece = '<i title=\"' + 'y'.repeat(290) + '\"></i>'; "
        "document.getElementById('e').innerHTML = piece.repeat(100000); "
        "console.log(d.innerHTML.length, d.outerHTML.length, "
        "document.getElementById('e').childNodes.length);"
        "</script>"
    )
    return Page(doc("<div id=e></div>" + script))


@case(
    "A script that sets innerHTML to 2,000,000 empty elements, with JavaScript on: the "
    "parse stops at the node limit and the task ends (ADR 0028 section 16)",
    expect_log="the DOM node limit",
    swb_args=JS,
    time_limit_s=10,
)
def inner_html_node_limit_js_on() -> Page:
    script = (
        "<script>document.getElementById('e').innerHTML = '<i></i>'.repeat(2000000); "
        "console.log('not reached');</script>"
    )
    return Page(doc("<div id=e></div>" + script))


@case(
    "A page of 500,000 elements and a script that runs querySelectorAll('*'), a selector "
    "with a descendant combinator, :nth-child and :not on all of them, with JavaScript on: "
    "each query is one pass over the tree",
    swb_args=JS,
    time_limit_s=10,
    rss_limit_mib=1536,
)
def query_selector_all_every_node_js_on() -> Page:
    script = (
        "<script>const r = []; for (const s of ['*', 'body i', 'i:nth-child(2n+1)', ':not(b)', "
        "'body > i + i']) r.push(document.querySelectorAll(s).length); console.log(r.join());"
        "</script>"
    )
    return Page(doc("<i></i>" * 500_000 + script))


@case(
    "querySelectorAll(':read-only') and (':read-write') over a chain of 100,000 nested divs "
    "built by a script, half of them in an editing host, with JavaScript on: a query finds "
    "the editing state of each chain of ancestors once (feature 12b review: 11 s before)",
    swb_args=JS,
)
def read_write_query_deep_tree_js_on() -> Page:
    script = (
        "<script>let p = document.body; for (let i = 0; i < 100000; i++) "
        "{ const d = document.createElement('div'); "
        "if (i === 50000) d.contentEditable = 'true'; p.appendChild(d); p = d; } "
        "console.log(document.querySelectorAll(':read-only').length, "
        "document.querySelectorAll(':read-write').length);</script>"
    )
    return Page(doc("<p>text</p>" + script))


@case(
    "Selectors with 40 descendant combinators over a chain of 100,000 nested divs built by a "
    "script, with JavaScript on: one that matches, one that fails at the subject, one that "
    "fails at the leftmost compound (every element walks all its ancestors: the work of a "
    "query stops at 100,000,000 steps and returns what it has, with a warning)",
    expect_log="a selector query is too expensive",
    swb_args=JS,
    time_limit_s=10,
)
def selector_many_combinators_deep_tree_js_on() -> Page:
    script = (
        "<script>let n = document.createElement('div'); for (let i = 0; i < 100000; i++) "
        "{ const w = document.createElement('div'); w.appendChild(n); n = w; } "
        "document.body.appendChild(n); "
        "const hit = 'div '.repeat(40) + 'div', miss = 'div '.repeat(40) + 'span', "
        "high = 'span ' + 'div '.repeat(40) + 'div', child = 'div > '.repeat(40) + 'span'; "
        "console.log(document.querySelectorAll(hit).length, "
        "document.querySelectorAll(miss).length, "
        "document.querySelectorAll(high).length, document.querySelectorAll(child).length, "
        "n.querySelector(':scope ' + miss));</script>"
    )
    return Page(doc("<p>text</p>" + script))


@case(
    "A loop over a live getElementsByTagName('*') list with a change in each step, with "
    "JavaScript on: the list walks again after each change, but only as far as the item "
    "that the loop reads, 10,000 steps over 10,000 to 20,000 elements",
    swb_args=JS,
    time_limit_s=10,
)
def live_list_loop_with_changes_js_on() -> Page:
    script = (
        "<script>const l = document.getElementsByTagName('*'); let seen = 0; "
        "for (let i = 0; i < l.length && i < 10000; i++) { seen += l[i].nodeType; "
        "document.body.appendChild(document.createElement('u')); } console.log(seen, l.length);"
        "</script>"
    )
    return Page(doc("<i></i>" * 10_000 + script))


@case(
    "A chain of 100,000 nested divs built by a script, then innerHTML and outerHTML of the "
    "chain read, with JavaScript on: the serializer is iterative, so a deep tree needs no "
    "native stack (ADR 0028 section 16)",
    swb_args=JS,
    time_limit_s=10,
)
def inner_html_deep_tree_js_on() -> Page:
    script = (
        "<script>let n = document.createElement('div'); for (let i = 0; i < 100000; i++) "
        "{ const w = document.createElement('div'); w.appendChild(n); n = w; } "
        "document.body.appendChild(n); const html = document.body.innerHTML; "
        "const out = n.outerHTML; console.log(html.length, out.length);"
        "</script>"
    )
    return Page(doc("<p>text</p>" + script))


@case(
    "A script that sets outerHTML of an element 50,000 times, each time on the element that "
    "the last set made, in a parent of 1,000 children, with JavaScript on: each set parses "
    "one small fragment and replaces one node",
    swb_args=JS,
    time_limit_s=10,
)
def outer_html_set_loop_js_on() -> Page:
    script = (
        "<script>const h = document.getElementById('h'); let e = h.children[500]; "
        "for (let i = 0; i < 50000; i++) "
        "{ e.outerHTML = '<b>' + i + '</b>'; e = h.children[500]; } "
        "console.log(h.children.length, e.textContent);</script>"
    )
    return Page(doc("<div id=h>" + "<i></i>" * 1000 + "</div>" + script))


@case(
    "A script that calls DOMParser.parseFromString and createHTMLDocument 20,000 times each, "
    "with JavaScript on: each document is a few nodes of the arena, and the node limit ends "
    "a script that goes on (ADR 0028 section 16)",
    swb_args=JS,
    time_limit_s=10,
)
def parse_documents_loop_js_on() -> Page:
    script = (
        "<script>let n = 0; for (let i = 0; i < 20000; i++) { "
        "n += new DOMParser().parseFromString('<p>' + i + '</p>', 'text/html')"
        ".body.childNodes.length; "
        "n += document.implementation.createHTMLDocument('t').body.childNodes.length; } "
        "console.log(n);</script>"
    )
    return Page(doc("<p>text</p>" + script))


@case(
    "A chain of 100,000 nested divs built by a script and closest() calls with descendant "
    "selectors that fail (35 s and 91 s before the bound), with JavaScript on: closest stops "
    "at the 100,000,000 steps of a query, with a warning, and returns null",
    expect_log="a selector query is too expensive",
    swb_args=JS,
    time_limit_s=10,
)
def closest_deep_tree_js_on() -> Page:
    script = (
        "<script>let leaf = document.createElement('div'); let n = leaf; "
        "for (let i = 0; i < 100000; i++) { const w = document.createElement('div'); "
        "w.appendChild(n); n = w; } document.body.appendChild(n); "
        "console.log(String(leaf.closest('section div')), "
        "String(leaf.closest('section > div div:not(.q)')));</script>"
    )
    return Page(doc("<p>text</p>" + script))


@case(
    "querySelectorAll with a selector list of 200,000 entries on a page of 5,000 elements "
    "(1,000,000,000 selector tests before the bound), with JavaScript on: each selector tried "
    "counts as a step, so the query stops at 100,000,000 steps",
    expect_log="a selector query is too expensive",
    swb_args=JS,
    time_limit_s=10,
)
def selector_long_list_js_on() -> Page:
    script = (
        "<script>console.log(document.querySelectorAll('.a,'.repeat(200000) + 'p').length, "
        "document.getElementById('c').matches('.a,'.repeat(200000) + 'p'));</script>"
    )
    return Page(doc("<p id=c></p>" + "<p></p>" * 5_000 + script))


@case(
    "A script that removes 100,000 children one by one with "
    "while (r.children.length) r.removeChild(r.children[0]), and 100,000 more through "
    "childNodes and the last child, with JavaScript on: length comes from the counts that the "
    "DOM keeps and the item from the nearer end, so each step is O(1) (3 s for 50,000 "
    "children before)",
    swb_args=JS,
    time_limit_s=10,
)
def children_drain_loop_js_on() -> Page:
    script = (
        "<script>const r = document.getElementById('r'), q = document.getElementById('q'); "
        "let n = 0; while (r.children.length) { r.removeChild(r.children[0]); n++; } "
        "while (q.childNodes.length) "
        "{ q.removeChild(q.childNodes[q.childNodes.length - 1]); n++; } console.log(n);"
        "</script>"
    )
    return Page(
        doc(
            "<div id=r>"
            + "<p></p>" * 100_000
            + "</div><div id=q>"
            + "<i></i>" * 100_000
            + "</div>"
            + script
        )
    )


@case(
    "100,000 elements whose reflected attributes a script reads and writes in a loop (string, "
    "URL, enumerated, boolean and number kinds), with JavaScript on: each access is O(1) in "
    "the number of elements (feature 6c)",
    swb_args=JS,
    time_limit_s=10,
)
def reflected_attribute_loop_js_on() -> Page:
    script = (
        "<script>const l = Array.from(document.getElementsByTagName('a')); let n = 0; "
        "for (let i = 0; i < l.length; i++) { const a = l[i]; a.href = '/p' + i; a.rel = 'x'; "
        "a.hidden = i % 2 === 0; a.tabIndex = i % 5; a.title = a.hash + a.pathname.length; "
        "a.draggable = true; n += a.href.length + a.tabIndex + (a.hidden ? 1 : 0) + "
        "a.referrerPolicy.length; } console.log(n);</script>"
    )
    return Page(doc("<a>x</a>" * 100_000 + script))


@case(
    "A form of 100,000 controls and a loop of 100,000 named lookups through form.elements and "
    "through the form itself, with JavaScript on: the list is built once, and the names are "
    "found through an index (feature 6c)",
    swb_args=JS,
    time_limit_s=10,
)
def form_elements_named_lookups_js_on() -> Page:
    script = (
        "<script>const f = document.getElementById('f'); let found = 0; "
        "for (let i = 0; i < 100000; i++) { if (f.elements['n' + i]) found++; "
        "if (f['n' + i]) found++; } console.log(f.length, found);</script>"
    )
    controls = "".join(f"<input name=n{i}>" for i in range(100_000))
    return Page(doc(f"<form id=f>{controls}</form>" + script))


@case(
    "A select with 100,000 options (50,000 in the page, 50,000 added by a script) and a loop "
    "of add and remove calls, with JavaScript on: adding at the end, removing at the front "
    "and reading length stay cheap (feature 6c)",
    swb_args=JS,
    time_limit_s=10,
)
def select_options_add_remove_js_on() -> Page:
    script = (
        "<script>const s = document.getElementById('s'); for (let i = 0; i < 50000; i++) { "
        "const o = document.createElement('option'); o.value = i; s.add(o); } "
        "let n = s.options.length; for (let i = 0; i < 50000; i++) { s.remove(0); "
        "s.add(document.createElement('option')); } console.log(n, s.length);</script>"
    )
    options = "".join(f"<option value={i}>{i}</option>" for i in range(50_000))
    return Page(doc(f"<select id=s>{options}</select>" + script))


@case(
    "An input whose value a script sets to a string of 24 MB, reads back and replaces, with "
    "JavaScript on: the value is stored once and the page does not lay out the whole string "
    "(feature 6c; the values of controls have a budget of 32 MiB)",
    swb_args=JS,
    time_limit_s=10,
    rss_limit_mib=1536,
)
def input_huge_value_js_on() -> Page:
    script = (
        "<script>const i = document.getElementById('i'); const big = 'x'.repeat(24 << 20); "
        "i.value = big; console.log(i.value.length); i.value = ''; i.value = big + 'y'; "
        "console.log(i.value.length); i.value = '';</script>"
    )
    return Page(doc("<input id=i>" + script))


@case(
    "A script that writes a string of 5 MB to the value of 400 new inputs and, in a second "
    "page script, to 400 textareas in a hidden element, with JavaScript on: the values of "
    "controls have a budget of 32 MiB (ADR 0028 section 16), and the task that reaches it "
    "ends (feature 6c review)",
    swb_args=JS,
    time_limit_s=10,
    rss_limit_mib=512,
    expect_log="values of form controls reached the limit",
)
def form_values_budget_js_on() -> Page:
    detached = (
        "<script>const big = 'a'.repeat(5e6); for (let i = 0; i < 400; i++) { "
        "document.createElement('input').value = big; }</script>"
    )
    attached = (
        "<script>const big2 = 'a'.repeat(5e6); const h = document.getElementById('h'); "
        "for (let i = 0; i < 400; i++) { const t = document.createElement('textarea'); "
        "h.appendChild(t); t.value = big2; }</script>"
    )
    return Page(doc("<div id=h style='display:none'></div>" + detached + attached))


@case(
    "6,000 radio buttons that a script creates, appends to a form and checks one after the "
    "other, with JavaScript on: a write costs no rebuild of the form state (feature 6c review)",
    swb_args=JS,
    time_limit_s=2,
)
def radio_append_checked_loop_js_on() -> Page:
    script = (
        "<script>const f = document.getElementById('f'); for (let i = 0; i < 6000; i++) { "
        "const r = document.createElement('input'); r.type = 'radio'; r.name = 'g'; "
        "f.appendChild(r); r.checked = true; } console.log(f.length);</script>"
    )
    return Page(doc("<form id=f></form>" + script))


@case(
    "A select of 30,000 options and a loop that sets option.selected to true on each one, "
    "with JavaScript on: a write changes the options in place, without a copy of them "
    "(feature 6c review)",
    swb_args=JS,
    time_limit_s=2,
)
def option_selected_loop_js_on() -> Page:
    script = (
        "<script>const s = document.getElementById('s'); s.length = 30000; "
        "const o = Array.from(s.options); for (let i = 0; i < o.length; i++) { "
        "o[i].selected = true; } console.log(s.selectedIndex);</script>"
    )
    return Page(doc("<select id=s></select>" + script))


@case(
    "A loop of 100,000 calls of new Image() and of new Option('x', 'y') that keeps the "
    "elements in an array, with JavaScript on: the node limit and the memory of the wrappers "
    "hold (feature 6c2)",
    swb_args=JS,
    time_limit_s=2,
    rss_limit_mib=1024,
)
def factory_functions_loop_js_on() -> Page:
    script = (
        "<script>const keep = []; for (let i = 0; i < 100000; i++) { keep.push(new Image(i, i)); "
        "keep.push(new Option('x', 'y', i % 2 == 0, i % 3 == 0)); } "
        "console.log(keep.length);</script>"
    )
    return Page(doc("<p>factories</p>" + script))


@case(
    "A loop that writes className.baseVal of 100,000 SVG elements that it creates and "
    "appends to an svg element, with JavaScript on: the restyle of the changed classes "
    "stays linear (feature 6c2)",
    swb_args=JS,
    time_limit_s=2,
    rss_limit_mib=1024,
)
def svg_class_name_write_loop_js_on() -> Page:
    script = (
        "<script>const N = 'http://www.w3.org/2000/svg'; const s = document.getElementById('s'); "
        "for (let i = 0; i < 100000; i++) { const e = document.createElementNS(N, 'rect'); "
        "e.className.baseVal = 'c' + (i % 100); s.appendChild(e); } "
        "for (const e of Array.from(s.children)) { e.className.baseVal += ' d'; } "
        "console.log(s.childNodes.length);</script>"
    )
    return Page(doc("<svg id=s width=100 height=100></svg>" + script))


@case(
    "A loop over the children of an element that has 100,000 children, with JavaScript on, "
    "that sets an attribute on each child: an attribute write keeps the cache of the child "
    "list, so the loop stays linear (feature 6c2 fix)",
    swb_args=JS,
    time_limit_s=2,
    rss_limit_mib=1024,
)
def children_loop_attribute_writes_js_on() -> Page:
    script = (
        "<script>const r = document.getElementById('r'); let k = 0; "
        "for (const e of r.children) e.setAttribute('a', k++); "
        "console.log(k);</script>"
    )
    return Page(doc("<div id=r>" + "<i></i>" * 100_000 + "</div>" + script))


@case(
    "An onclick handler that calls click() on its own element and on itself again and "
    "again, with JavaScript on: the click-in-progress flag ends the recursion (HTML "
    "click(), feature 6d1); a listener that clicks another element in a chain of 20,000 "
    "elements ends with the shared recursion budget",
    swb_args=JS,
    time_limit_s=3,
    rss_limit_mib=1024,
)
def click_recursion_js_on() -> Page:
    script = (
        "<script>let n = 0; const a = document.getElementById('a'); "
        "a.onclick = () => { n++; a.click(); }; a.click(); "
        "const els = []; for (let i = 0; i < 20000; i++) { const e = document.createElement('b'); "
        "els.push(e); document.body.appendChild(e); } "
        "for (let i = 0; i < els.length - 1; i++) "
        "els[i].onclick = () => { n++; els[i + 1].click(); }; "
        "els[els.length - 1].onclick = () => { n++; }; els[0].click(); "
        "console.log('clicks ' + n);</script>"
    )
    return Page(doc("<a id=a href='#'>a</a>" + script))


@case(
    "100,000 writes of on* content attributes and IDL attributes on 100,000 elements and "
    "100,000 calls of the handlers, with JavaScript on: the handlers compile once each, the "
    "maps and the listener lists stay linear (feature 6d1)",
    swb_args=JS,
    time_limit_s=5,
    rss_limit_mib=1024,
)
def event_handler_attribute_writes_js_on() -> Page:
    script = (
        "<script>let n = 0; const els = []; for (let i = 0; i < 100000; i++) { "
        "const e = document.createElement('i'); e.setAttribute('onclick', 'window.n++'); "
        "e.onmouseover = () => { n++; }; els.push(e); } window.n = 0; "
        "for (const e of els) { e.dispatchEvent(new MouseEvent('click')); "
        "e.dispatchEvent(new MouseEvent('mouseover')); } "
        "for (let i = 0; i < 100000; i++) { els[0].setAttribute('onclick', 'window.n += ' + i); "
        "els[0].onmouseout = els[0].onmouseover; } "
        "console.log('handled ' + window.n + ' ' + n);</script>"
    )
    return Page(doc("<p>handlers</p>" + script))


@case(
    "focus() on 100,000 focusable elements in a loop, with a :focus rule in the style sheet "
    "and focus listeners, with JavaScript on: the restyle waits for the next flush, so the "
    "loop does not compute the styles for each call (feature 6d1)",
    swb_args=JS,
    time_limit_s=6,
    rss_limit_mib=1024,
)
def focus_loop_js_on() -> Page:
    script = (
        "<script>let n = 0; document.addEventListener('focusin', () => n++); "
        "const els = document.querySelectorAll('input'); "
        "for (const e of els) e.focus(); console.log('focused ' + n + ' ' + "
        "(document.activeElement === els[els.length - 1]));</script>"
    )
    body = "<div>" + "<input>" * 100_000 + "</div>" + script
    return Page(doc(body, "input:focus { color: red }"))


@case(
    "click() and dispatched clicks on a label and on the text input inside it, labels that "
    "nest each other with DOM calls, a for attribute that points into another label and a "
    "control that is the label's ancestor, with JavaScript on: the label does not click a "
    "control that the event came from, and a label that is clicking its control does not "
    "start again (feature 6d1, review)",
    swb_args=JS,
    time_limit_s=3,
    rss_limit_mib=1024,
)
def label_activation_js_on() -> Page:
    script = (
        "<script>let n = 0; document.addEventListener('click', () => { n++; }, true); "
        "const l = document.getElementById('l'), i = document.getElementById('i'); "
        "l.click(); i.click(); i.dispatchEvent(new MouseEvent('click', { bubbles: true })); "
        "const a = document.createElement('label'), b = document.createElement('label'); "
        "const x = document.createElement('input'), y = document.createElement('input'); "
        "x.id = 'x'; y.id = 'y'; a.htmlFor = 'y'; b.htmlFor = 'x'; "
        "a.appendChild(x); b.appendChild(y); document.body.append(a, b); "
        "a.click(); b.click(); x.click(); y.click(); "
        "try { a.appendChild(b); b.appendChild(a); } catch (e) { } "
        "a.click(); b.click(); "
        "const c = document.createElement('label'); c.htmlFor = 'bt'; "
        "document.getElementById('bt').appendChild(c); c.click(); "
        "for (let k = 0; k < 300; k++) { const m = document.createElement('label'); "
        "m.appendChild(document.createElement('input')); document.body.appendChild(m); "
        "m.click(); m.firstChild.click(); } "
        "console.log('clicks ' + n);</script>"
    )
    return Page(doc("<label id=l>t<input id=i></label><button id=bt>b</button>" + script))


@case(
    "A submit handler that calls requestSubmit() and submit() on its form, a reset handler "
    "that calls reset(), and a loop of 100,000 requestSubmit() calls whose handler cancels "
    "the event, with JavaScript on: the firing-submission-events and locked-for-reset flags "
    "end the recursion and the loop stays linear (feature 6d1)",
    swb_args=JS,
    time_limit_s=3,
    rss_limit_mib=1024,
)
def submit_reset_loops_js_on() -> Page:
    script = (
        "<script>let n = 0, r = 0; const f = document.getElementById('f'); "
        "f.addEventListener('submit', (e) => { e.preventDefault(); n++; "
        "f.requestSubmit(); f.submit(); }); "
        "f.addEventListener('reset', () => { r++; f.reset(); }); "
        "for (let i = 0; i < 100000; i++) { f.requestSubmit(); if (i % 100 == 0) f.reset(); } "
        "console.log('submits ' + n + ' resets ' + r);</script>"
    )
    form = "<form id=f action='x'><input name=a value=1><button>go</button></form>"
    return Page(doc(form + script))


@case(
    "A mousemove listener that rebuilds a 3,000-element list and queries it on each of 200 "
    "pointer moves, with JavaScript on: each event is a task of its own under the time limit "
    "(feature 6d2)",
    swb_args=steps_args([{"hover": [10 + 3 * i, 20 + 2 * i]} for i in range(200)]),
    time_limit_s=12,
    rss_limit_mib=1024,
)
def mousemove_heavy_dom_js_on() -> Page:
    script = (
        "<script>let n = 0; const box = document.getElementById('box'); "
        "document.addEventListener('mousemove', () => { n++; box.textContent = ''; "
        "for (let i = 0; i < 3000; i++) { const d = document.createElement('i'); "
        "d.className = 'c' + (i % 7); box.appendChild(d); } "
        "n += box.querySelectorAll('.c3').length; }); "
        "window.addEventListener('load', () => setTimeout(() => console.log('moves ' + n), 50));"
        "</script>"
    )
    return Page(doc("<div id=box style='width:300px;height:300px'></div>" + script))


@case(
    "A keydown listener that moves the focus to the next of 2,000 inputs, and keypress, "
    "input and focus listeners on the document, while 300 characters are typed, with "
    "JavaScript on: a listener that calls focus() on another element changes where the "
    "default action goes, not how many events fire (feature 6d2)",
    swb_args=steps_args([{"focus": "#i0"}, {"type": "x" * 300}]),
    time_limit_s=12,
    rss_limit_mib=1024,
)
def keydown_focus_other_js_on() -> Page:
    script = (
        "<script>let k = 0, f = 0, n = 0; "
        "document.addEventListener('keydown', (e) => { k++; const next = "
        "document.activeElement.nextElementSibling; if (next) next.focus(); }); "
        "document.addEventListener('focusin', () => f++); "
        "document.addEventListener('input', () => n++); "
        "window.addEventListener('load', () => setTimeout(() => "
        "console.log('keys ' + k + ' focus ' + f + ' inputs ' + n), 50));</script>"
    )
    inputs = "".join(f"<input id=i{i}>" for i in range(2000))
    return Page(doc(inputs + script))


@case(
    "A click listener that removes its target and the next 20 siblings, 90 clicks in a "
    "row on the first of 2,000 buttons, with JavaScript on: the events that follow a "
    "removed target (mouseup, click) go to the element that is left (feature 6d2)",
    swb_args=steps_args([{"click": "button"}] * 90),
    time_limit_s=12,
    rss_limit_mib=1024,
)
def click_removes_target_js_on() -> Page:
    script = (
        "<script>let n = 0; document.addEventListener('click', (e) => { n++; "
        "let t = e.target; for (let i = 0; i < 20 && t.nextElementSibling; i++) "
        "t.nextElementSibling.remove(); t.remove(); }); "
        "document.addEventListener('mousedown', (e) => { if (e.target.id == 'x') "
        "e.target.remove(); }); "
        "window.addEventListener('load', () => setTimeout(() => console.log('clicks ' + n), 50));"
        "</script>"
    )
    body = "<div>" + "<button>b</button>" * 2000 + "</div>"
    return Page(doc(body + script))


@case(
    "A link whose href is javascript:while(1){} is clicked, with JavaScript on: the script "
    "of the URL runs under the time limit of a task (5 s) and the page stays (feature 6d2)",
    swb_args=steps_args([{"click": "a"}]),
    time_limit_s=12,
    rss_limit_mib=1024,
)
def javascript_link_loop_js_on() -> Page:
    return Page(doc("<a href='javascript:while(1){}'>loop</a>"))


@case(
    "wheel listeners on 10,000 elements and on the document, body and window (passive by "
    "default there), 100 wheel turns over the elements, with JavaScript on: the dispatch "
    "visits the listeners of the path only (feature 6d2)",
    swb_args=steps_args([{"hover": [20, 20]}] + [{"wheel": [0, 40]}] * 100),
    time_limit_s=12,
    rss_limit_mib=1024,
)
def wheel_listeners_10000_js_on() -> Page:
    script = (
        "<script>let n = 0; const all = document.querySelectorAll('p'); "
        "for (const p of all) p.addEventListener('wheel', () => n++); "
        "for (const t of [window, document, document.body]) "
        "for (let i = 0; i < 50; i++) t.addEventListener('wheel', () => n++); "
        "window.addEventListener('load', () => setTimeout(() => console.log('wheels ' + n), 50));"
        "</script>"
    )
    body = "<p style='margin:0;height:20px'>w</p>" * 10_000
    return Page(doc(body + script))


@case(
    "A page of 100,000 paragraphs with a bold child, 200 pointer moves with JavaScript on "
    "and no listener: the offsets of the events come from boxes that are kept for the "
    "layout, so a move does not walk the whole page (feature 6d2 fix round)",
    swb_args=steps_args([{"hover": [10 + 3 * i, 20 + 2 * i]} for i in range(200)]),
    time_limit_s=12,
    rss_limit_mib=1024,
)
def hover_100000_paragraphs_js_on() -> Page:
    body = "<style>body{margin:0}p{margin:0;height:10px}</style>" + "<p><b>x</b></p>" * 100_000
    return Page(doc(body + "<script>document.title = 'hover';</script>"))


@case(
    "4,000 times: append an input to a page of 20,000 elements and focus() it, with "
    "JavaScript on: the focusability check computes the style of the input and its "
    "ancestors, not of the whole document (feature 7a; it restyled the document after "
    "each change before)",
    swb_args=JS,
    time_limit_s=5,
    rss_limit_mib=1024,
)
def append_focus_loop_js_on() -> Page:
    script = (
        "<script>let n = 0; document.addEventListener('focusin', () => n++); "
        "const box = document.getElementById('box'); "
        "for (let i = 0; i < 4000; i++) { const e = document.createElement('input'); "
        "box.appendChild(e); e.focus(); } console.log('focused ' + n);</script>"
    )
    body = "<div>" + "<p><b>x</b></p>" * 10_000 + "</div><div id=box></div>" + script
    return Page(doc(body, "input:focus { color: red } p b { font-weight: normal }"))


@case(
    "20,000 times on a page of 50,000 elements: write element.style.left, read it back and "
    "call focus(), with JavaScript on: each write changes the attribute, and the "
    "focusability check after it computes one element's style (feature 7a)",
    swb_args=JS,
    time_limit_s=6,
    rss_limit_mib=1024,
)
def style_write_query_loop_js_on() -> Page:
    script = (
        "<script>const e = document.getElementById('t'); let s = 0; "
        "for (let i = 0; i < 20000; i++) { e.style.left = i + 'px'; "
        "s += parseInt(e.style.left); e.focus(); } console.log('sum ' + s);</script>"
    )
    rows = "<p><b>x</b></p>" * 25_000
    body = "<div>" + rows + "</div><input id=t style='position:relative'>" + script
    return Page(doc(body))


@case(
    "Huge values through element.style with JavaScript on: cssText of 800,000 declarations "
    "(9.6 MB), a custom property of 10 MB, a font-family list of 1,000,000 names, and an "
    "invalid width of 10 MB (feature 7a)",
    swb_args=JS,
    time_limit_s=10,
    rss_limit_mib=2048,
)
def huge_style_values_js_on() -> Page:
    script = (
        "<script>const e = document.getElementById('t'); "
        "e.style.cssText = 'color: red; '.repeat(800000); const a = e.style.length; "
        "e.style.setProperty('--x', 'x'.repeat(10000000)); "
        "e.style.fontFamily = 'a,'.repeat(1000000) + 'b'; "
        "e.style.width = '1'.repeat(10000000); "
        "console.log('lengths ' + a + ' ' + e.style.length + ' ' + "
        "e.getAttribute('style').length);</script>"
    )
    return Page(doc("<p id=t>x</p>" + script))


@case(
    "100,000 setAttribute('style') writes with a read of element.style after each, and "
    "100,000 elements that each get a property written through element.style, with "
    "JavaScript on: a read parses the attribute only after it changed, and each element "
    "keeps one declaration block (feature 7a)",
    swb_args=JS,
    time_limit_s=8,
    rss_limit_mib=1536,
)
def style_attribute_writes_js_on() -> Page:
    script = (
        "<script>const t = document.getElementById('t'); let n = 0; "
        "for (let i = 0; i < 100000; i++) { t.setAttribute('style', 'width: ' + i + 'px'); "
        "if (t.style.width === i + 'px') n++; } "
        "const box = document.getElementById('box'); "
        "for (let i = 0; i < 100000; i++) { const p = document.createElement('i'); "
        "p.style.color = i % 2 ? 'red' : 'blue'; box.appendChild(p); } "
        "console.log('styles ' + n + ' ' + box.children.length);</script>"
    )
    return Page(doc("<p id=t>x</p><div id=box></div>" + script))


@case(
    "One element with 200,000 distinct custom properties in cssText, then 10,000 "
    "setProperty calls of new custom properties on another element, with JavaScript on: "
    "the parse of cssText is linear; each write serializes the block of its element "
    "(feature 7a)",
    swb_args=JS,
    time_limit_s=8,
    rss_limit_mib=1536,
)
def many_custom_properties_js_on() -> Page:
    script = (
        "<script>const a = document.getElementById('a'), b = document.getElementById('b'); "
        "let css = ''; for (let i = 0; i < 200000; i++) css += '--p' + i + ': ' + i + '; '; "
        "a.style.cssText = css; "
        "for (let i = 0; i < 10000; i++) b.style.setProperty('--q' + i, 'v'); "
        "console.log('custom ' + a.style.length + ' ' + b.style.length + ' ' + "
        "a.style.getPropertyValue('--p199999'));</script>"
    )
    return Page(doc("<p id=a>x</p><p id=b>y</p>" + script))


@case(
    "A loop that writes element.style.width and reads getComputedStyle(e).width, which "
    "layout resolves, 1,000,000 times on a page of 50,000 elements, with JavaScript on: "
    "each read restyles and lays out the page, so the task ends at its time limit of 5 s "
    "(the binding checks the deadline after each flush; feature 7a2, ADR 0028 sections 9 "
    "and 16)",
    swb_args=JS,
    time_limit_s=8,
    rss_limit_mib=1024,
)
def computed_style_layout_loop_js_on() -> Page:
    script = (
        "<script>const e = document.getElementById('t'); let s = 0; "
        "for (let i = 0; i < 1000000; i++) { e.style.width = (i % 500) + 'px'; "
        "s += parseFloat(getComputedStyle(e).width); } console.log('sum ' + s);</script>"
    )
    rows = "<p><b>x</b></p>" * 25_000
    return Page(doc("<div>" + rows + "</div><div id=t></div>" + script))


@case(
    "getComputedStyle of every element of a page of 100,000 elements in one task, with a "
    "style value and a value that layout resolves of each, with JavaScript on: one full "
    "restyle and one layout serve all reads (feature 7a2)",
    swb_args=JS,
    time_limit_s=8,
    rss_limit_mib=1536,
)
def computed_style_every_element_js_on() -> Page:
    script = (
        "<script>let n = 0, w = 0; for (const e of document.querySelectorAll('*')) { "
        "const s = getComputedStyle(e); if (s.color) n++; w += parseFloat(s.width) || 0; } "
        "console.log('styles ' + n + ' ' + Math.round(w));</script>"
    )
    rows = "<p><b>x</b></p>" * 50_000
    return Page(doc("<div>" + rows + "</div>" + script))


@case(
    "A loop that writes element.style.width and reads offsetWidth 1,000,000 times on a page "
    "of 50,000 elements, with JavaScript on: each read lays out the page, so the task ends "
    "at its time limit of 5 s (the binding checks the deadline after each geometry read; "
    "feature 7b)",
    swb_args=JS,
    time_limit_s=8,
    rss_limit_mib=1024,
)
def geometry_layout_loop_js_on() -> Page:
    script = (
        "<script>const e = document.getElementById('t'); let s = 0; "
        "for (let i = 0; i < 1000000; i++) { e.style.width = (i % 500) + 'px'; "
        "s += e.offsetWidth + e.getBoundingClientRect().width; } console.log('sum ' + s);</script>"
    )
    rows = "<p><b>x</b></p>" * 25_000
    return Page(doc("<div>" + rows + "</div><div id=t></div>" + script))


@case(
    "getBoundingClientRect, getClientRects, offsetTop and scrollWidth of every element of a "
    "page of 100,000 elements in one task, with JavaScript on: one layout and one walk of the "
    "fragments per index serve all reads (feature 7b)",
    swb_args=JS,
    time_limit_s=8,
    rss_limit_mib=1536,
)
def bounding_rect_every_element_js_on() -> Page:
    script = (
        "<script>let n = 0, h = 0; for (const e of document.querySelectorAll('*')) { "
        "h += e.getBoundingClientRect().height + e.offsetTop + e.scrollWidth; "
        "n += e.getClientRects().length; } "
        "console.log('rects ' + n + ' ' + Math.round(h));</script>"
    )
    rows = "<p><b>x</b></p>" * 50_000
    return Page(doc("<div>" + rows + "</div>" + script))


@case(
    "A loop of 1,000,000 script scrolls of the viewport and of a scroll container with "
    "geometry reads between them, with JavaScript on: each scroll changes the scroll state, "
    "so each read walks the fragments again; the task ends at its time limit (feature 7b)",
    swb_args=JS,
    time_limit_s=8,
    rss_limit_mib=1024,
)
def scroll_loop_js_on() -> Page:
    script = (
        "<script>const sc = document.getElementById('sc'); let s = 0; "
        "for (let i = 0; i < 1000000; i++) { scrollTo(0, i % 5000); sc.scrollTop = i % 300; "
        "s += document.getElementById('t').getBoundingClientRect().top + scrollY; } "
        "console.log('sum ' + s);</script>"
    )
    rows = "<p>x</p>" * 5_000
    return Page(
        doc(
            "<div id=sc style='height:100px;overflow:auto'><div style='height:500px'></div></div>"
            + rows
            + "<div id=t>t</div>"
            + script
        )
    )


@case(
    "elementFromPoint and elementsFromPoint at every second px of the viewport (240,000 "
    "calls) on a page of 20,000 elements, with JavaScript on: each call hit-tests the display "
    "list; the task ends at its time limit or finishes (feature 7b)",
    swb_args=JS,
    time_limit_s=8,
    rss_limit_mib=1024,
)
def element_from_point_loop_js_on() -> Page:
    script = (
        "<script>let n = 0; for (let y = 0; y < 600; y += 2) for (let x = 0; x < 800; x += 2) "
        "{ if (document.elementFromPoint(x, y)) n++; "
        "n += document.elementsFromPoint(x, y).length; } "
        "console.log('hits ' + n);</script>"
    )
    rows = "<span style='display:inline-block;width:7px;height:7px'>x</span>" * 20_000
    return Page(doc("<div>" + rows + "</div>" + script))


@case(
    "Huge scroll offsets and sizes from scripts, with JavaScript on: scrollTo, scrollBy, "
    "scrollTop and scrollIntoView with 1e308, -1e308, Infinity and NaN on a page and a "
    "scroll container 1e9 px high (clamped to the layout limit; feature 7b)",
    swb_args=JS,
    time_limit_s=8,
)
def huge_scroll_offsets_js_on() -> Page:
    script = (
        "<script>const sc = document.getElementById('sc'), end = document.getElementById('end'); "
        "for (const v of [1e308, -1e308, Infinity, -Infinity, NaN, 1e20, 33554431]) { "
        "scrollTo(v, v); scrollBy(v, -v); scroll({left: v, top: v}); sc.scrollTop = v; "
        "sc.scrollLeft = v; sc.scrollBy(v, v); end.scrollIntoView({block: 'center'}); "
        "if (isFinite(v)) document.elementFromPoint(v, v); } "
        "console.log('at ' + scrollX + ' ' + scrollY + ' ' + sc.scrollTop + ' ' + "
        "end.getBoundingClientRect().top + ' ' + document.documentElement.scrollHeight);</script>"
    )
    return Page(
        doc(
            "<div id=sc style='height:100px;width:100px;overflow:auto'>"
            "<div style='height:1e9px;width:1e9px'></div></div>"
            "<div style='height:1e9px;width:1e9px'></div><div id=end>end</div>" + script
        )
    )


@case(
    "Layout queries while the page lays out web fonts: 300 @font-face families on 300 "
    "spans, an inline script that reads the width of each span (each layout in the task "
    "starts font loads through the page's loader) and a timer that reads them again while "
    "the fonts arrive, with JavaScript on (feature 7a2)",
    swb_args=("--js", "--settle-ms", "1000"),
    time_limit_s=10,
)
def computed_style_web_fonts_js_on() -> Page:
    rules = "".join(f"@font-face{{font-family:F{i};src:url(f.ttf?{i})}}" for i in range(300))
    spans = "".join(
        f"<span id=s{i} style='display:inline-block;font-family:F{i}'>w{i}</span>"
        for i in range(300)
    )
    script = (
        "<script>function widths() { let w = 0; for (let i = 0; i < 300; i++) "
        "w += parseFloat(getComputedStyle(document.getElementById('s' + i)).width); "
        "return Math.round(w); } console.log('first ' + widths()); "
        "let k = 0; const t = setInterval(() => { console.log('again ' + widths()); "
        "if (++k == 5) clearInterval(t); }, 100);</script>"
    )
    return Page(doc(spans + script, rules), {"f.ttf": web_font()})


@case(
    "A requestAnimationFrame loop that changes the width of an element and reads offsetWidth "
    "in every frame for 2 s of virtual time (120 frames) on a page of 10,000 elements, with "
    "JavaScript on: each frame lays out once, as in Chromium (feature 7b2)",
    swb_args=("--js", "--virtual-time", "--settle-ms", "2000"),
    time_limit_s=8,
)
def raf_layout_loop_js_on() -> Page:
    script = (
        "<script>const e = document.getElementById('t'); let n = 0, s = 0; "
        "function f() { e.style.width = (n++ % 500) + 'px'; s += e.offsetWidth; "
        "requestAnimationFrame(f); } requestAnimationFrame(f); "
        "setTimeout(() => console.log('frames ' + n + ' ' + s), 1900);</script>"
    )
    rows = "<p><b>x</b></p>" * 5_000
    return Page(doc("<div>" + rows + "</div><div id=t></div>" + script))


@case(
    "A requestAnimationFrame callback that never returns and asks for the next frame first, "
    "with JavaScript on: each callback ends at the time limit of its task (5 s), and the "
    "scripts of the document stop after the third (feature 7b2)",
    swb_args=("--js", "--virtual-time", "--settle-ms", "1000"),
    time_limit_s=20,
    expect_log="the scripts of the document stop",
)
def raf_endless_callback_js_on() -> Page:
    script = (
        "<script>function f() { requestAnimationFrame(f); for (;;) {} } "
        "requestAnimationFrame(f);</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "innerText of the body of a page of 200,000 elements three times and of every 100th "
    "element, with JavaScript on: one style flush, one walk per read (feature 7b2)",
    swb_args=JS,
    time_limit_s=8,
    rss_limit_mib=1536,
)
def inner_text_large_page_js_on() -> Page:
    script = (
        "<script>let n = 0; for (let i = 0; i < 3; i++) n += document.body.innerText.length; "
        "const all = document.querySelectorAll('p'); "
        "for (let i = 0; i < all.length; i += 100) n += all[i].innerText.length; "
        "console.log('chars ' + n);</script>"
    )
    rows = "<p>a <b>b</b> <i>c</i></p><table><tr><td>x</td><td>y</td></tr></table>" * 33_000
    return Page(doc("<div>" + rows + "</div>" + script))


@case(
    "1,000,000 matchMedia calls with different queries and 100,000 lists with a change "
    "listener, with JavaScript on: lists without listeners are not kept; the lists with "
    "listeners are evaluated only when the media environment changes (feature 7b2)",
    swb_args=("--js", "--virtual-time", "--settle-ms", "500"),
    time_limit_s=8,
    rss_limit_mib=1536,
)
def match_media_loop_js_on() -> Page:
    script = (
        "<script>let n = 0; for (let i = 0; i < 1000000; i++) "
        "if (matchMedia('(min-width: ' + (i % 2000) + 'px)').matches) n++; "
        "const kept = []; for (let i = 0; i < 100000; i++) { "
        "const m = matchMedia('(max-width: ' + i + 'px)'); "
        "m.addListener(() => n++); kept.push(m); } console.log('matched ' + n);</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "Every global name read in 1,000 rounds (as an identifier, as window[name] and every "
    "50th round with getOwnPropertyDescriptor), with JavaScript on: each lazy interface is "
    "created once, and the reads of the identifiers stay cached (ADR 0028 section 5, lazy "
    "interfaces)",
    swb_args=JS,
    time_limit_s=4,
    rss_limit_mib=1024,
)
def global_names_read_loop_js_on() -> Page:
    script = (
        "<script>const names = Object.getOwnPropertyNames(window); let n = 0; "
        "const reads = names.map(k => new Function('return typeof ' + k)); "
        "for (let round = 0; round < 1000; round++) "
        "for (let i = 0; i < names.length; i++) { const k = names[i]; "
        "if (reads[i]() === 'function') n++; if (window[k] !== undefined) n++; "
        "if (round % 50 === 0 && Object.getOwnPropertyDescriptor(window, k)) n++; } "
        "console.log('reads ' + n);</script>"
    )
    return Page(doc("<p>names</p>" + script))


@case(
    "Every interface name deleted, overwritten or redefined before its first use, then "
    "32,000 elements of 16 kinds created and appended, every 500th clicked first, with "
    "JavaScript on: the "
    "prototype chains stay whole and nothing breaks (ADR 0028 section 5, lazy interfaces)",
    swb_args=JS,
    time_limit_s=4,
    rss_limit_mib=1024,
)
def global_names_replaced_before_use_js_on() -> Page:
    script = (
        "<script>const all = Object.getOwnPropertyNames(window), define = Object.defineProperty; "
        "const names = all.slice(all.indexOf('Screen')).filter(k => /^[A-Z]/.test(k)); "
        "names.forEach((k, i) => { if (i % 3 === 0) delete window[k]; "
        "else if (i % 3 === 1) window[k] = i; "
        "else define(window, k, { value: null, writable: false }); }); "
        "let ok = 0; for (const tag of ['div', 'span', 'img', 'option', 'svg', 'table', 'input', "
        "'select', 'form', 'a', 'p', 'canvas', 'video', 'template', 'textarea', 'unknown-x']) { "
        "for (let i = 0; i < 2000; i++) { const e = document.createElement(tag); "
        "if (e instanceof Object.getPrototypeOf(e).constructor) ok++; "
        "e.onclick = ev => { if (ev instanceof Object.getPrototypeOf(ev).constructor) ok++; }; "
        "if (i % 500 === 0) e.click(); document.body.appendChild(e); } } "
        "console.log('ok ' + ok + ' ' + names.length);</script>"
    )
    return Page(doc("<p>replaced</p>" + script))


@case(
    "100,000 pushState and replaceState calls with objects as state, then 100,000 location.hash "
    "changes with popstate and hashchange listeners, with JavaScript on: the session history "
    "keeps 50 entries, the state is serialized once per call, and the 100,000 hashchange "
    "tasks run after the loop (feature 8a1)",
    swb_args=("--js", "--settle-ms", "2000"),
    time_limit_s=10,
    rss_limit_mib=1536,
)
def push_state_and_hash_loops_js_on() -> Page:
    script = (
        "<script>let pops = 0, hashes = 0; addEventListener('popstate', () => pops++); "
        "addEventListener('hashchange', () => hashes++); "
        "for (let i = 0; i < 100000; i++) { "
        "history.pushState({i, list: [i, i + 1], at: new Date(i)}, '', '?p=' + (i % 10)); "
        "if (i % 3 === 0) history.replaceState({i}, ''); } "
        "const length = history.length; "
        "for (let i = 0; i < 100000; i++) location.hash = 'h' + i; "
        "setTimeout(() => console.log('length ' + length + ' pops ' + pops + ' hashes ' + hashes), "
        "1500);</script>"
    )
    return Page(doc("<p>push</p>" + script))


@case(
    "A hashchange handler that sets location.hash again, with JavaScript on: every change is "
    "a task of its own, the history keeps 50 entries and the page settles after "
    "--settle-ms (feature 8a1)",
    swb_args=("--js", "--settle-ms", "500"),
    time_limit_s=6,
)
def hash_change_endless_js_on() -> Page:
    script = (
        "<script>let n = 0; addEventListener('hashchange', () => { location.hash = 'n' + n++; }); "
        "location.hash = 'start';</script>"
    )
    return Page(doc("<p>hash</p>" + script))


@case(
    "A loop of 1,000,000 rounds of history.back(), go(0), forward() and location.reload() "
    "after a few pushState calls, with JavaScript on: at most 64 traversals wait, repeated "
    "reloads are one, so the page stays small and settles (fix round of feature 8a1)",
    swb_args=("--js", "--settle-ms", "500"),
    time_limit_s=10,
    rss_limit_mib=512,
)
def history_traversal_loop_js_on() -> Page:
    script = (
        "<script>if (!location.search) { "
        "for (let i = 0; i < 3; i++) history.pushState(i, '', '?p=' + i); "
        "let n = 0; for (let i = 0; i < 1000000; i++) { history.back(); history.go(0); "
        "history.forward(); location.reload(); n++; } "
        "console.log('rounds ' + n); }</script>"
    )
    return Page(doc("<p>traverse</p>" + script))


@case(
    "structuredClone of a chain 200,000 levels deep (a RangeError at 4,096), of an array of "
    "150,000 objects, of a Map of 100,000 entries, of 4 buffers of 16 MB and of a string of "
    "8 MB, with JavaScript on: the walks are iterative and bounded by the depth, the item "
    "and the byte limits (feature 8a1)",
    swb_args=JS,
    time_limit_s=10,
    rss_limit_mib=2048,
)
def structured_clone_large_js_on() -> Page:
    script = (
        "<script>let o = {}; const root = o; "
        "for (let i = 0; i < 200000; i++) { o.n = {}; o = o.n; } "
        "let depth = 'cloned'; try { structuredClone(root); } catch (e) { depth = e.name; } "
        "const a = []; for (let i = 0; i < 150000; i++) a.push({i, s: 'x' + i}); "
        "const m = new Map(); for (let i = 0; i < 100000; i++) m.set(i, [i]); "
        "const b = new ArrayBuffer(16 << 20); "
        "const c = [structuredClone(a).length, structuredClone(m).size]; "
        "for (let i = 0; i < 4; i++) c.push(structuredClone(b).byteLength); "
        "c.push(structuredClone('x'.repeat(8 << 20)).length); "
        "console.log(depth + ' ' + c.join());</script>"
    )
    return Page(doc("<p>clone</p>" + script))


@case(
    "Endless loops of requestIdleCallback with JavaScript on, one with idle time and one with a "
    "timeout of 1 ms: an idle period starts only when no task and no frame is due and lasts "
    "at most 50 ms (a loop alone runs 20 times a second; with a task due every millisecond, "
    "about 1,000 times), and the page settles after --settle-ms (feature 8a2)",
    swb_args=("--js", "--settle-ms", "1500"),
    time_limit_s=6,
)
def idle_callback_loops_js_on() -> Page:
    script = (
        "<script>let idle = 0, timed = 0; "
        "function f() { idle++; requestIdleCallback(f); } f(); "
        "function g() { timed++; requestIdleCallback(g, { timeout: 1 }); } g(); "
        "setTimeout(() => console.log('idle ' + idle + ' timed ' + timed), 1200);</script>"
    )
    return Page(doc("<p>idle</p>" + script))


@case(
    "1,100,000 requestIdleCallback requests in one task, each with a timeout, with JavaScript "
    "on: the limit of 1,048,576 waiting callbacks ends the task (ADR 0028 section 16, "
    "feature 8a2)",
    swb_args=("--js", "--settle-ms", "300"),
    time_limit_s=10,
    rss_limit_mib=2048,
    expect_log="too many idle callbacks",
)
def idle_callback_flood_js_on() -> Page:
    script = (
        "<script>let n = 0; "
        "for (let i = 0; i < 1100000; i++) { requestIdleCallback(() => n++, { timeout: 5000 }); } "
        "console.log('not reached');</script>"
    )
    return Page(doc("<p>flood</p>" + script))


@case(
    "performance.mark and performance.measure 400,000 times in a loop with details, with "
    "JavaScript on, while two observers wait for them and getEntries is called: the buffer "
    "keeps 131,072 entries and the queue of an observer 131,072 (ADR 0028 section 16, "
    "feature 8a2)",
    swb_args=("--js", "--settle-ms", "500"),
    time_limit_s=15,
    rss_limit_mib=2048,
    expect_log="performance entry buffer is full",
)
def performance_mark_loop_js_on() -> Page:
    script = (
        "<script>let delivered = 0; "
        "for (let k = 0; k < 2; k++) new PerformanceObserver((list) => { "
        "delivered += list.getEntries().length; }).observe({ entryTypes: ['mark', 'measure'] }); "
        "for (let i = 0; i < 400000; i++) { "
        "performance.mark('m' + (i % 100), { detail: { i } }); "
        "if (i % 4 === 0) performance.measure('d' + (i % 50), 'm' + (i % 100)); } "
        "const all = performance.getEntries().length; performance.clearMarks(); "
        "performance.clearMeasures(); "
        "setTimeout(() => console.log('buffer ' + all + ' delivered ' + delivered), 300);</script>"
    )
    return Page(doc("<p>marks</p>" + script))


@case(
    "TextDecoder on buffers of 60 MiB (decoded), 100 MiB (a RangeError) and 64 chunks of 1 MiB "
    "in stream mode, TextEncoder on a string of 16 MB, with JavaScript on: the decoder works "
    "in chunks of 1 MiB and takes at most 64 MiB at a time (feature 8a2)",
    swb_args=JS,
    time_limit_s=12,
)
def text_decoder_huge_js_on() -> Page:
    script = (
        "<script>const out = []; "
        "out.push(new TextDecoder().decode(new Uint8Array(60 << 20).fill(97)).length); "
        "try { new TextDecoder().decode(new Uint8Array(100 << 20)); } "
        "catch (e) { out.push(e.name); } "
        "const d = new TextDecoder('windows-1252'); "
        "const chunk = new Uint8Array(1 << 20).fill(0x80); "
        "let n = 0; for (let i = 0; i < 64; i++) n += d.decode(chunk, { stream: true }).length; "
        "out.push(n); out.push(new TextEncoder().encode('é'.repeat(8 << 20)).length); "
        "console.log(out.join());</script>"
    )
    return Page(doc("<p>decode</p>" + script))


@case(
    "crypto.getRandomValues over its limit 100,000 times (a QuotaExceededError), 2,000 times "
    "with the largest array, on a DataView and on float arrays, and crypto.randomUUID 200,000 "
    "times, with JavaScript on (feature 8a2)",
    swb_args=JS,
    time_limit_s=10,
)
def random_values_over_limit_js_on() -> Page:
    script = (
        "<script>const big = new Uint8Array(65537), ok = new Uint8Array(65536); let errors = 0; "
        "for (let i = 0; i < 100000; i++) { try { crypto.getRandomValues(big); } "
        "catch (e) { errors += e.name === 'QuotaExceededError' ? 1 : 0; } } "
        "for (let i = 0; i < 2000; i++) { crypto.getRandomValues(ok); "
        "try { crypto.getRandomValues(new Float32Array(2)); } catch (e) { errors++; } "
        "try { crypto.getRandomValues(new DataView(new ArrayBuffer(8))); } "
        "catch (e) { errors++; } } "
        "const ids = new Set(); for (let i = 0; i < 200000; i++) ids.add(crypto.randomUUID()); "
        "console.log('errors ' + errors + ' ids ' + ids.size);</script>"
    )
    return Page(doc("<p>random</p>" + script))


def replay_page(html: str) -> Page:
    """A page of `http://hostile.test/` that a replayed fixture serves, for
    the cases that need an `http:` origin (cookies)."""
    manifest = {
        "version": 1,
        "entries": [
            {
                "method": "GET",
                "url": REPLAY_URL,
                "status": 200,
                "headers": [["content-type", "text/html; charset=utf-8"]],
                "body": "files/index.html",
            }
        ],
    }
    return Page(
        html,
        {"manifest.json": json.dumps(manifest), "files/index.html": html},
    )


@case(
    "localStorage and sessionStorage written in loops past their quota of 5,242,880 code "
    "units, the errors caught: 1 MiB values, 100-unit values under distinct keys, 200,000 "
    "one-unit keys (the limit of 100,000 keys), then reads, removals and clear, with "
    "JavaScript on (feature 8b)",
    swb_args=JS,
    time_limit_s=10,
)
def storage_past_quota_js_on() -> Page:
    script = (
        "<script>let ok = 0, errors = 0; "
        "for (let i = 0; i < 40; i++) { "
        "try { localStorage.setItem('big' + i, 'x'.repeat(1 << 20)); "
        "ok++; } catch (e) { errors += e.name === 'QuotaExceededError' ? 1 : 0; } } "
        "localStorage.clear(); "
        "for (let i = 0; i < 200000; i++) { "
        "try { sessionStorage.setItem('key-' + i, 'v'.repeat(100)); "
        "ok++; } catch (e) { errors++; } } "
        "sessionStorage.clear(); "
        "for (let i = 0; i < 200000; i++) { try { localStorage['k' + i] = ''; ok++; } "
        "catch (e) { errors++; } } "
        "const n = localStorage.length; let sum = 0; "
        "for (let i = 0; i < 1000; i++) sum += localStorage.key(i).length; "
        "for (let i = 0; i < 100000; i++) localStorage.removeItem('k' + i); "
        "console.log('ok ' + ok + ' errors ' + errors + ' keys ' + n + ' after ' "
        "+ localStorage.length + ' ' + sum);"
        "</script>"
    )
    return Page(doc("<p>storage</p>" + script))


@case(
    "100,000 distinct cookies set through document.cookie and read back every 100 sets (the "
    "jar keeps at most 180 per site and 3,300 in all), then a read loop of 100,000 reads with "
    "the jar full, with JavaScript on and an http: page from a replayed fixture (feature 8b)",
    swb_args=REPLAY,
    time_limit_s=10,
    url="http://hostile.test/index.html",
)
def cookies_flood_js_on() -> Page:
    script = (
        "<script>let longest = 0; "
        "for (let i = 0; i < 100000; i++) { document.cookie = 'c' + i + '=' + 'v'.repeat(20); "
        "if (i % 100 === 0) longest = Math.max(longest, document.cookie.length); } "
        "let total = 0; for (let i = 0; i < 100000; i++) total += document.cookie.length; "
        "console.log('longest ' + longest + ' total ' + total + ' now ' "
        "+ document.cookie.split('; ').length);"
        "</script>"
    )
    return replay_page(doc("<p>cookies</p>" + script))


@case(
    "400 cookies of 4,000 bytes under two paths set through document.cookie (the per-site "
    "limit of 180 keeps 150), then 5,000 reads of a cookie string of about 600 KB, with "
    "JavaScript on and an http: page from a replayed fixture (feature 8b)",
    swb_args=REPLAY,
    time_limit_s=10,
    url="http://hostile.test/index.html",
)
def cookies_big_values_js_on() -> Page:
    script = (
        "<script>let sizes = 0; "
        "for (let i = 0; i < 400; i++) { document.cookie = 'big' + i + '=' + 'x'.repeat(4000) "
        "+ '; Path=' + (i % 2 ? '/' : '/index.html'); } "
        "for (let i = 0; i < 5000; i++) sizes += document.cookie.length; "
        "console.log('sizes ' + sizes);</script>"
    )
    return replay_page(doc("<p>cookies</p>" + script))
