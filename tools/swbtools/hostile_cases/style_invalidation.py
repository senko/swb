"""The restyle after state changes (ADR 0041, M11 feature 5) and after DOM
changes (ADR 0042, M11 feature 6): pointer moves and scripts on pages whose
selectors make the analysis walk deep trees, long sibling lists and many
dependencies."""

from swbtools.hostile_cases.core import JS, Page, case, doc, nest, steps_args

ALTERNATE = [{"hover": [5, 5] if i % 2 == 0 else [5, 400]} for i in range(40)]
"""40 pointer moves between the top-left corner and a point below it."""


@case(
    "3,000 nested elements with `:hover` rules for children, descendants, following "
    "siblings and `:has()` anchors, and 40 pointer moves between the deepest element and "
    "another one: the marking and the walk of the targeted restyle stay linear in the depth "
    "(ADR 0041 section 2; the layout of the deep tree is most of the time)",
    swb_args=steps_args(ALTERNATE),
    time_limit_s=30,
    rss_limit_mib=1024,
)
def hover_deep_tree_3000() -> Page:
    css = (
        "div { margin: 0 } div:hover > div { color: red } .a:hover .b { color: blue } "
        ".a:has(:hover) { outline: 1px solid } .b:hover ~ .a { color: green } "
        "#other { position: absolute; top: 300px; height: 200px; width: 200px }"
    )
    body = nest("<div class='a b'>", "</div>", 3_000, "<span>x</span>") + "<p id=other>o</p>"
    return Page(doc(body, css))


@case(
    "20,000 sibling list items with `li:hover + li` and `li:hover` rules, 40 pointer moves "
    "over the list: a move restyles the hovered item and the next one, not the list (ADR "
    "0041 section 1; the layout of the list is most of the time)",
    swb_args=steps_args([{"hover": [5, 1 + 3 * i]} for i in range(40)]),
    time_limit_s=20,
    rss_limit_mib=1024,
)
def hover_20000_siblings_next() -> Page:
    css = "ul { margin: 0; padding: 0 } li { height: 3px } li:hover + li { color: red } "
    css += "li:hover { color: blue }"
    return Page(doc("<ul>" + "<li>x" * 20_000 + "</ul>", css))


@case(
    "5,000 sibling list items with `li:hover ~ li`, 40 pointer moves over the list: a move "
    "restyles the following items (the matching of `~` is quadratic in any restyle; the "
    "targeted restyle must not add to it) (ADR 0041 section 1)",
    swb_args=steps_args([{"hover": [5, 1 + 30 * i]} for i in range(40)]),
    time_limit_s=30,
    rss_limit_mib=1024,
)
def hover_5000_siblings_subsequent() -> Page:
    css = "ul { margin: 0; padding: 0 } li { height: 3px } li:hover ~ li { color: red }"
    return Page(doc("<ul>" + "<li>x" * 5_000 + "</ul>", css))


@case(
    "20,000 rules `.cN:has(:hover)` and `.cN:hover .dN` and 40 pointer moves: the index of "
    "the state dependencies is built once and a move looks up the keys of the changed "
    "elements only (ADR 0041 section 1)",
    swb_args=steps_args(ALTERNATE),
    time_limit_s=20,
    rss_limit_mib=1024,
)
def hover_many_has_rules() -> Page:
    css = "".join(
        f".c{i}:has(:hover) {{ color: red }} .c{i}:hover .d{i} {{ color: blue }}"
        for i in range(10_000)
    )
    body = "<div class=c1><p class=d1>a</p></div>" + "<p>x</p>" * 200
    return Page(doc(body, css))


def timed(body: str, label: str) -> str:
    """A script that runs `body`, then logs `LABEL ok in N ms`."""
    return (
        "<script>const start = performance.now(); " + body + " "
        f"console.warn('{label} ok in ' + Math.round(performance.now() - start) + ' ms');"
        "</script>"
    )


@case(
    "20,000 sibling list items with `li:nth-child(2n)`, `.a ~ li` and `.a + li` rules; a "
    "script toggles the class `a` on items near the end 200 times and reads the layout 5 "
    "times between: each flush restyles the siblings that the rules reach after the changed "
    "items, and the script logs `toggles ok` (ADR 0042 sections 2 and 7)",
    time_limit_s=30,
    rss_limit_mib=1536,
    expect_log="toggles ok",
    swb_args=JS,
)
def dom_class_toggles_20000_siblings() -> Page:
    css = (
        "ul { margin: 0; padding: 0 } li { height: 1px } li:nth-child(2n) { color: red } "
        ".a ~ li { color: blue } .a + li { color: green }"
    )
    script = timed(
        "const items = document.querySelectorAll('li'); "
        "for (let i = 0; i < 200; i++) { items[19900 + (i % 100)].classList.toggle('a'); "
        "if (i % 40 == 0) document.body.getBoundingClientRect(); }",
        "toggles",
    )
    return Page(doc("<ul>" + "<li>x" * 20_000 + "</ul>" + script, css))


@case(
    "3,000 nested elements with `:has()` rules in the subject and in other compounds; a "
    "script changes the class of the deepest element 100 times with a layout read after every "
    "fifth: the anchors are the ancestors with the anchor's key, each walked once per flush, "
    "and the script logs `has ok` (ADR 0042 section 1)",
    time_limit_s=30,
    rss_limit_mib=1024,
    expect_log="has ok",
    swb_args=JS,
)
def dom_deep_has_3000() -> Page:
    css = (
        "div { margin: 0 } .x:has(.y) { color: red } .x:has(> .y) .z { color: blue } "
        ".x:has(.y) + .w { color: green }"
    )
    body = nest("<div class='x z'>", "</div>", 3_000, "<span id=leaf>x</span>")
    script = timed(
        "const leaf = document.getElementById('leaf'); "
        "for (let i = 0; i < 100; i++) { leaf.classList.toggle('y'); "
        "if (i % 5 == 0) document.body.getBoundingClientRect(); }",
        "has",
    )
    return Page(doc(body + script, css))


@case(
    "100,000 children inserted into a list in one fragment and removed again, 3 times, with a "
    "layout read after each change: the changes are larger than half of the styled elements, "
    "so each flush is one full restyle, and the script logs `insert ok` (ADR 0042 section 5)",
    time_limit_s=30,
    rss_limit_mib=2048,
    expect_log="insert ok",
    swb_args=JS,
)
def dom_insert_100000_children() -> Page:
    css = "ul { margin: 0; padding: 0 } li { height: 1px } li:nth-child(odd) { color: red }"
    script = timed(
        "const ul = document.getElementById('u'); "
        "for (let r = 0; r < 3; r++) { const f = document.createDocumentFragment(); "
        "for (let i = 0; i < 100000; i++) f.appendChild(document.createElement('li')); "
        "ul.appendChild(f); document.body.getBoundingClientRect(); "
        "ul.replaceChildren(); document.body.getBoundingClientRect(); }",
        "insert",
    )
    return Page(doc("<ul id=u><li>a</ul>" + script, css))


@case(
    "10,000 rules `[data-xN] .dN` and a script that sets and removes the attributes `data-x0` "
    "to `data-x999` on an element with 1,000 descendants, with a layout read every 100 "
    "changes: each change looks up the rules of its attribute name only and restyles the "
    "descendants with that rule's key, and the script logs `attributes ok` (ADR 0042 section "
    "1)",
    time_limit_s=30,
    rss_limit_mib=1024,
    expect_log="attributes ok",
    swb_args=JS,
)
def dom_attribute_rules_10000() -> Page:
    css = "".join(f"[data-x{i}] .d{i} {{ color: red }}" for i in range(10_000))
    body = "<div id=r>" + "<p class=c>x</p>" * 1_000 + "</div>"
    script = timed(
        "const r = document.getElementById('r'); "
        "for (let i = 0; i < 1000; i++) { r.setAttribute('data-x' + i, ''); "
        "if (i % 2) r.removeAttribute('data-x' + (i - 1)); "
        "if (i % 100 == 0) document.body.getBoundingClientRect(); }",
        "attributes",
    )
    return Page(doc(body + script, css))


@case(
    "a script that appends 1,000 items one at a time to a list with `li:nth-child(odd)` and "
    "`li:last-child` rules and reads the layout after each: each flush examines the children "
    "of the changed list (as a full restyle restyles them), and the script logs `append ok` (ADR "
    "0042 section 2)",
    time_limit_s=30,
    rss_limit_mib=1024,
    expect_log="append ok",
    swb_args=JS,
)
def dom_append_with_layout_1000() -> Page:
    css = (
        "ul { margin: 0; padding: 0 } li { height: 1px } li:nth-child(odd) { color: red } "
        "li:last-child { color: blue }"
    )
    script = timed(
        "const ul = document.getElementById('u'); "
        "for (let i = 0; i < 1000; i++) { ul.appendChild(document.createElement('li')); "
        "document.body.getBoundingClientRect(); }",
        "append",
    )
    return Page(doc("<ul id=u></ul>" + script, css))
