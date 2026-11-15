"""The restyle after state changes (ADR 0041, M11 feature 5): pointer moves
over pages whose selectors make the analysis walk deep trees, long sibling
lists and many dependencies."""

from swbtools.hostile_cases.core import Page, case, doc, nest, steps_args

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
