"""The cases of the hostile-page set (docs/testing.md, "Hostile-page set").

A case is a generator of one page that goes just past a limit that an ADR
documents (or a generic hostile pattern). `swbtools hostile` runs them
(`hostile.py`). To add a case, write a function that returns a `Page` and
decorate it with `@case(...)`. The name of the function is the name of the
case (with `_` as `-`).

The description names the limit and the ADR. `expect_log` is a part of the
warning that swb logs when the limit acts; the case fails with `inactive`
if the warning is missing, so that a changed limit does not go unnoticed.
Generated pages stay below `MAX_PAGE_BYTES`.
"""

import base64
import json
from collections.abc import Callable
from dataclasses import dataclass, field

MAX_PAGE_BYTES = 20_000_000
"""The most that a case may generate (the HTML and its extra files)."""

MIB = 1 << 20


@dataclass(frozen=True)
class Page:
    """The generated page: `index.html` and files written next to it."""

    html: str
    files: dict[str, str | bytes] = field(default_factory=dict)

    def size(self) -> int:
        """The size of all files in bytes."""
        extra = sum(len(c.encode() if isinstance(c, str) else c) for c in self.files.values())
        return len(self.html.encode()) + extra


@dataclass(frozen=True)
class Case:
    """One hostile page with the limits that swb must keep on it."""

    name: str
    description: str
    build: Callable[[], Page]
    time_limit_s: float = 5.0
    rss_limit_mib: float = 1024
    known_failure: str | None = None
    """The reason, if swb has a known bug that this case shows."""
    expect_log: str | None = None
    swb_args: tuple[str, ...] = ()
    """Options for swb; `{dir}` stands for the directory of the case."""
    url: str | None = None
    """The URL to open instead of `index.html` (with `--replay {dir}`: a page
    of an `http:` origin, which cookies need)."""
    forbid_log: str | None = None
    """A part of a warning that swb must not log: the case shows that a limit
    does not act too early (`error` if the log has it)."""
    server: str | None = None
    """`never` or `slow`: the runner starts a local server that holds each
    request without an answer, or answers it after 2 s; the page reaches it
    at `http://127.0.0.1:__PORT__/` (feature 10a)."""


CASES: list[Case] = []
"""All cases, in the order in which they run."""


def case(
    description: str,
    time_limit_s: float = 5.0,
    rss_limit_mib: float = 1024,
    known_failure: str | None = None,
    expect_log: str | None = None,
    swb_args: tuple[str, ...] = (),
    url: str | None = None,
    server: str | None = None,
    forbid_log: str | None = None,
) -> Callable[[Callable[[], Page]], Callable[[], Page]]:
    """Registers the decorated generator as a case."""

    def register(build: Callable[[], Page]) -> Callable[[], Page]:
        name = build.__name__.removeprefix("_").replace("_", "-")
        CASES.append(
            Case(
                name,
                description,
                build,
                time_limit_s,
                rss_limit_mib,
                known_failure,
                expect_log,
                swb_args,
                url,
                forbid_log,
                server,
            )
        )
        return build

    return register


def doc(body: str, css: str = "") -> str:
    """A standards-mode document with a style sheet."""
    return f"<!doctype html><meta charset=utf-8><style>{css}</style><body style='margin:0'>{body}"


def nest(open_tag: str, close_tag: str, depth: int, inner: str = "") -> str:
    """`inner` inside `depth` levels of `open_tag` ... `close_tag`."""
    return open_tag * depth + inner + close_tag * depth


def img_page(svg: str, size: str = "width=200 height=200") -> Page:
    """A page with one SVG image from the file `img.svg`."""
    return Page(doc(f"<img src=img.svg {size}>"), {"img.svg": svg})


SVG_NS = "xmlns='http://www.w3.org/2000/svg' xmlns:xlink='http://www.w3.org/1999/xlink'"


# --- Generic hostile input -------------------------------------------------


@case("DOM nesting 3,000 deep, past the tree depth limit of 512 (ADR 0004, 0007)")
def nest_div_3000() -> Page:
    return Page(doc(nest("<div>", "</div>", 3000, "text")))


@case(
    "boxes nested past MAX_BOX_DEPTH 256, with borders (ADR 0015, 0016)",
    expect_log="boxes nested deeper",
)
def nest_boxes_300() -> Page:
    css = "div{border:1px solid;padding:1px}"
    return Page(doc(nest("<div>", "</div>", 300, "text"), css))


@case("3,000 nested inline elements (ADR 0015)")
def nest_inline_3000() -> Page:
    return Page(doc(nest("<span>", "</span>", 3000, "text words that wrap " * 50)))


@case(
    "100,000 nested divs (roadmap, M0 backlog: html5ever's scope checks are quadratic)",
    time_limit_s=3,
    known_failure="html5ever is quadratic in the nesting depth: 50,000 divs 3 s, 100,000 divs 13 s",
)
def nest_div_100000() -> Page:
    return Page(doc(nest("<div>", "</div>", 100_000, "x")))


@case("one word of 4 MB in a narrow box, and with overflow-wrap: anywhere")
def long_word() -> Page:
    word = "w" * 2_000_000
    body = f"<div style='width:50px'>{word}</div>"
    body += f"<div style='width:50px;overflow-wrap:anywhere'>{word}</div>"
    return Page(doc(body))


@case("2 MB of text with break opportunities in a 20 px box, break-all and normal")
def long_text_narrow() -> Page:
    text = "ab cd-ef " * 100_000
    body = f"<div style='width:20px'>{text}</div>"
    body += f"<div style='width:20px;word-break:break-all'>{text}</div>"
    return Page(doc(body))


@case("huge and negative lengths: 1e30px margins, sizes, font sizes, line heights, transforms")
def huge_lengths() -> Page:
    props = [
        "margin:1e30px",
        "margin:-1e30px",
        "margin-left:-1e30px;margin-top:1e30px",
        "width:1e30px;height:1e30px",
        "width:-1e30px;height:-1e30px",
        "padding:1e30px",
        "border:1e30px solid",
        "font-size:1e30px",
        "font-size:-1e30px;line-height:-1e30px",
        "line-height:1e30px",
        "line-height:1e30",
        "text-indent:1e30px;letter-spacing:1e30px;word-spacing:-1e30px",
        "transform:translate(1e30px,-1e30px)",
        "transform:scale(1e30)",
        "transform:rotate(1e30deg) skew(89.9999deg)",
        "position:absolute;top:1e30px;left:-1e30px;width:1e30px",
        "position:relative;top:-1e30px;left:1e30px",
        "position:sticky;top:1e30px",
        "display:flex;gap:1e30px;width:1e30px",
        "display:grid;grid-template-columns:1e30px 1fr;gap:1e30px",
        "display:flex;flex-basis:1e30px;flex-grow:1e30",
        "float:left;width:1e30px;height:1e30px",
        "border-radius:1e30px;box-shadow:1e30px 1e30px 1e30px 1e30px red",
        "background:linear-gradient(red 1e30px,blue 1e30%);background-size:1e30px",
        "outline:1e30px solid;outline-offset:1e30px",
        "zoom:1e30;columns:1e30",
    ]
    body = "".join(f"<div style='{p}'>text <i>in</i> it<div>child</div></div>" for p in props)
    table = "<table style='width:1e30px'><tr><td style='width:1e30px;height:1e30px'>x"
    return Page(doc(body + table))


@case("5,000 rules with :has() over 6,000 elements (ADR 0007)", time_limit_s=10)
def has_rules() -> Page:
    rules = "".join(f".a{i % 50}:has(> .b{i % 40} .c{i}) {{ color: red }}\n" for i in range(5000))
    rules += "div:has(div:has(div:has(div:has(span)))) { color: blue }\n"
    body = "".join(
        f"<div class='a{i % 50}'><div class='b{i % 40}'><i class='c{i}'>x</i></div></div>"
        for i in range(2000)
    )
    return Page(doc(body, rules))


@case("200,000 style rules (stylesheet of about 5 MB)", time_limit_s=10)
def many_rules() -> Page:
    rules = "".join(f".c{i}{{color:red}}\n" for i in range(200_000))
    return Page(doc("".join(f"<i class=c{i * 97}>x</i>" for i in range(2000)), rules))


@case("one selector list of 100,000 selectors, and 3,000 compound selectors of 200 classes")
def selector_lists() -> Page:
    big = ",".join(f".s{i}" for i in range(100_000)) + "{color:red}\n"
    compound = "".join(f"{'.k' * 200}{{color:blue}}\n" for _ in range(3000))
    return Page(doc("<i class='s5 s99999'>x</i><i class=k>y</i>", big + compound))


@case(
    "1,100,000 comments: past the limit of 1,048,576 nodes per document; the parser stops "
    "(ADR 0028 section 16)",
    expect_log="HTML parser stopped",
)
def dom_node_limit() -> Page:
    return Page(doc("<p>before</p>" + "<!---->" * 1_100_000 + "<p>after</p>"))


@case(
    "a formatting element with a 2 MB attribute that the parser clones for each of 200 "
    "paragraphs (400 MB): past the limit of 256 MiB of text and attributes per document; the "
    "parser stops (ADR 0028 section 16)",
    expect_log="HTML parser stopped",
)
def dom_byte_limit() -> Page:
    title = "t" * 2_000_000
    return Page(doc(f"<p><b title='{title}'></p>" + "<p>x</p>" * 200))


@case("250,000 elements in one document", time_limit_s=10)
def many_elements() -> Page:
    return Page(doc("<i>x</i>" * 125_000 + "<b></b>" * 125_000))


@case(
    "rule, block and function nesting 20,000 deep in CSS (css parser limit 64)",
    expect_log="nested too deeply",
)
def css_nesting() -> Page:
    n = 20_000
    css = "a{" * n + "color:red" + "}" * n + "\n"
    css += "@media screen{" * n + "a{color:red}" + "}" * n + "\n"
    css += ":is(" * n + "a" + ")" * n + "{color:red}\n"
    css += "p{width:" + "calc(" * n + "1px" + ")" * n + "}\n"
    css += "p{width:" + "(" * n + "1px" + ")" * n + "}\n"
    css += "p{width:" + "[" * n + "1px" + "]" * n + "}\n"
    css += "p{color:" + "var(--a," * n + "red" + ")" * n + "}\n"
    return Page(doc("<a>x</a><p>y</p>", css))


@case("a chain of 5,000 stylesheets with @import (extra files)")
def import_chain() -> Page:
    files: dict[str, str | bytes] = {
        f"s{i}.css": f"@import url(s{i + 1}.css); .i{i}{{color:red}}" for i in range(5000)
    }
    return Page(doc("<i class=i0>x</i>", "@import url(s0.css);"), files)


@case("3,000 style elements and 3,000 inline style attributes of 1 KB")
def many_style_elements() -> Page:
    styles = "".join(f"<style>.m{i}{{color:red}}</style>" for i in range(3000))
    pad = "color:red;" * 100
    items = "".join(f"<i class=m{i} style='{pad}'>x</i>" for i in range(3000))
    return Page(doc(styles + items))


@case("an option list of 50,000 entries in a select, and 20 selects (MAX_SELECT_OPTIONS 10,000)")
def select_options() -> Page:
    options = "".join(f"<option>option number {i}" for i in range(50_000))
    return Page(doc(f"<select>{options}</select>" * 2 + "<select multiple>" + options))


@case("300,000 character references and 90,000 broken ones")
def entities() -> Page:
    text = "&amp;&lt;&#x41;" * 100_000 + "&#x110000;&#xD800;&#0;&#xFFFFFFFFFF;&foo;&ampamp" * 15_000
    return Page(doc(f"<p>{text}</p>"))


@case("images with huge size attributes and a srcset of 50,000 candidates")
def huge_images() -> Page:
    cand = ",".join(f"img.svg {i}.5x" for i in range(1, 50_000))
    svg = f"<svg {SVG_NS} width='10' height='10'/>"
    body = "<img src=img.svg width=4294967295 height=4294967295>"
    body += "<img src=img.svg width=1e9 height=-5><img src=img.svg style='width:1e30px'>"
    body += f"<img srcset='{cand}' sizes='100vw'>"
    return Page(doc(body), {"img.svg": svg})


@case(
    "13,000 lazy images with sizes=auto (selected after layout: 10,000 with 40 candidates, "
    "1,000 with unique sources, 2,000 without containment) and 3,000 without a box"
)
def auto_sized_images() -> Page:
    cand = ",".join(f"img.svg?{i} {i * 10}w" for i in range(1, 40))
    svg = f"<svg {SVG_NS} width='10' height='10'/>"
    body = f"<img srcset='{cand}' sizes='auto, 100vw' loading=lazy width=50 height=50>" * 10_000
    for i in range(1_000):
        body += (
            f"<img srcset='img.svg?u{i} 100w, img.svg?v{i} 200w' sizes=auto loading=lazy "
            "style='width:30%'>"
        )
    # No containment (white space before `auto`), and images without a box.
    body += "<img srcset='img.svg?w1 100w, img.svg?w2 900w' sizes=' auto' loading=lazy>" * 2_000
    body += (
        "<div style='display:none'>"
        + (f"<img srcset='{cand}' sizes=auto loading=lazy>" * 3_000)
        + "</div>"
    )
    return Page(doc(body), {"img.svg": svg})


@case("text decorations: 5,000 text shadows, 10,000 box shadows with huge blur, 10,000 backgrounds")
def many_shadows() -> Page:
    shadows = ",".join(f"{i % 7}px {i % 5}px {i % 50}px red" for i in range(5000))
    boxes = ",".join(f"{i}px {i}px 1e9px {i}px blue" for i in range(10_000))
    layers = ",".join("linear-gradient(red,blue)" for _ in range(10_000))
    body = f"<p style='text-shadow:{shadows}'>shadow</p>"
    body += f"<div style='width:50px;height:50px;box-shadow:{boxes}'></div>"
    body += f"<div style='width:50px;height:50px;background:{layers}'></div>"
    return Page(doc(body))


@case(
    "60 different bold italic glyphs at 3,800 px in the viewport (glyph masks: side 4,096; "
    "no supersampling above the cached size: 44 MiB, 171 MiB with it)",
    rss_limit_mib=128,
)
def huge_glyphs() -> Page:
    text = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz01234567"
    spans = "".join(
        f"<span style='position:absolute;left:{-(i % 3) * 600}px;top:{-(i % 4) * 900}px'>{c}</span>"
        for i, c in enumerate(text)
    )
    return Page(doc(f"<div style='font:italic bold 3800px/1 serif'>{spans}</div>"))


# --- Style (ADR 0007) ------------------------------------------------------


@case("a chain of 10,000 custom properties that refer to each other (ADR 0007, depth 128)")
def var_chain() -> Page:
    decls = "--v0:1px;" + "".join(f"--v{i}:var(--v{i - 1});" for i in range(1, 10_001))
    return Page(doc("<p>x</p>", f"p{{{decls}width:var(--v10000);margin:var(--v100)}}"))


@case("exponential var() expansion: 60 levels that each use the last one twice (ADR 0007)")
def var_exponential() -> Page:
    decls = "--a0:1px;" + "".join(f"--a{i}:var(--a{i - 1}) var(--a{i - 1});" for i in range(1, 61))
    css = f"p{{{decls}margin:var(--a60);padding:var(--a60)}}"
    return Page(doc("<p>x</p>" * 200, css))


@case("var() results nested 300 deep in functions and blocks (ADR 0007, 64 levels)")
def var_nesting() -> Page:
    decls = "--n0:1px;--m0:1px;"
    for i in range(1, 301):
        decls += f"--n{i}:calc(var(--n{i - 1}));--m{i}:(var(--m{i - 1}));"
    css = f"p{{{decls}width:var(--n300);height:var(--m300);margin:var(--n300) var(--m300)}}"
    return Page(doc("<p>x</p>" * 200, css))


@case("custom properties: 20,000 declared on 500 elements, inherited by 20 levels")
def many_custom_properties() -> Page:
    decls = "".join(f"--p{i}:{i}px;" for i in range(20_000))
    css = f".d{{{decls}}}"
    body = "".join(nest("<div class=d>", "</div>", 20, "x") for _ in range(25))
    return Page(doc(body, css))


@case(
    "counters() with a 1 KB separator in 500 nested elements: 4 MiB of counter text (ADR 0007)",
    expect_log="counters: more than",
)
def counters_text() -> Page:
    sep = "s" * 1000
    css = f"div{{counter-increment:c;counter-reset:c}}div::before{{content:counters(c,'{sep}')}}"
    return Page(doc(nest("<div>", "</div>", 500, "x"), css))


@case(
    "counter-reset with 10,000 names (at most 256 per property, ADR 0007)",
    expect_log="a counter property names",
)
def counters_many_names() -> Page:
    names = " ".join(f"n{i}" for i in range(10_000))
    css = f"div{{counter-reset:{names};counter-increment:{names}}}"
    css += "div::before{content:counter(n5) counter(n9999)}"
    return Page(doc("<div>x</div>" * 300, css))


@case(
    "a list with 100,000 items and a reversed list of 50,000 (list item ordinals, ADR 0007)",
    rss_limit_mib=1536,
)
def many_list_items() -> Page:
    items = "<li>x" * 100_000
    return Page(doc(f"<ol>{items}</ol><ol reversed>{items[: 5 * 50_000]}</ol>"))


@case(
    "100 KB of ::before content text on 3,000 elements (MAX_GENERATED_TEXT 1 MiB)",
    expect_log="bytes of generated content text",
)
def content_text_bomb() -> Page:
    text = "x" * 100_000
    css = f".g::before{{content:'{text}'}}"
    return Page(doc("<div class=g></div>" * 3000, css))


@case(
    "100 KB of block ::after content text on 3,000 elements (MAX_GENERATED_TEXT 1 MiB)",
    expect_log="bytes of generated content text",
)
def content_text_bomb_block() -> Page:
    text = "y z" * 33_000
    css = f".g::after{{display:block;content:'{text}' attr(class)}}"
    return Page(doc("<div class=g></div>" * 3000, css))


@case(
    "100 KB of content text in table-row pseudos of 3,000 tables (MAX_GENERATED_TEXT 1 MiB)",
    expect_log="bytes of generated content text",
)
def content_text_bomb_table() -> Page:
    text = "x" * 100_000
    css = f".g{{display:table}}.g::before{{display:table-row;content:'{text}'}}"
    return Page(doc("<div class=g></div>" * 3000, css))


@case(
    "100 KB of ::marker content text on 3,000 list items (MAX_GENERATED_TEXT 1 MiB)",
    expect_log="bytes of generated content text",
)
def content_text_bomb_marker() -> Page:
    text = "\\e9" * 50_000
    css = f"li::marker{{content:'{text}'}}"
    return Page(doc("<ul>" + "<li>x" * 3000 + "</ul>", css))


# --- Tables (ADR 0010) -----------------------------------------------------


@case(
    "a row of 15,000 cells (MAX_COLUMNS 10,000, ADR 0010)",
    expect_log="after column",
)
def table_columns() -> Page:
    return Page(doc("<table><tr>" + "<td>x" * 15_000 + "</table>"))


@case(
    "12 cells with colspan 1000 and rowspan 65534, 200 rows below (ADR 0010)",
    expect_log="after column",
)
def table_spans() -> Page:
    cell = "<td colspan=1000 rowspan=65534>x"
    return Page(doc("<table><tr>" + cell * 12 + "<tr><td>y" * 200 + "</table>"))


@case(
    "collapsed borders: 1,000 columns, 1,100 rows, over 2,000,000 edges (ADR 0010)",
    expect_log="collapsed borders",
)
def table_collapsed_edges() -> Page:
    rows = "<tr>" + "<td>x" * 1000 + "<tr><td>x" * 1100
    css = "table{border-collapse:collapse}td{border:1px solid red}"
    return Page(doc(f"<table>{rows}</table>", css))


@case("tables nested 300 deep, with text (ADR 0010, box depth 256)")
def table_nesting() -> Page:
    return Page(doc(nest("<table><tr><td>", "</table>", 300, "text")))


@case(
    "table cells with 50 KB words and percentage widths in 300 rows of 30 columns",
    rss_limit_mib=1536,
)
def table_wide_content() -> Page:
    word = "w" * 50_000
    row = "<tr>" + "".join(
        f"<td style='width:{i % 7}%'>{word if i == 0 else 'cell'}" for i in range(30)
    )
    return Page(doc("<table>" + row * 300 + "</table>"))


# --- SVG images (ADR 0011) -------------------------------------------------


@case(
    "SVG entity bomb: 10 levels of 10 references, over 1 MiB of text (ADR 0011)",
    expect_log="expand too much",
)
def svg_entity_bomb() -> Page:
    decls = '<!ENTITY e0 "aaaaaaaaaa">'
    for i in range(1, 10):
        decls += f'<!ENTITY e{i} "{f"&e{i - 1};" * 10}">'
    svg = f"<?xml version='1.0'?><!DOCTYPE svg [{decls}]><svg {SVG_NS} width='100' height='100'>"
    svg += "<text y='50'>&e9;</text></svg>"
    return img_page(svg)


@case("SVG with 600,000 nodes (limit 500,000, ADR 0011)", expect_log="nodes limit reached")
def svg_many_nodes() -> Page:
    return img_page(f"<svg {SVG_NS} width='9' height='9'>" + "<g/>" * 600_000 + "</svg>")


@case(
    "SVG with 500 entity declarations (limit 100, ADR 0011)",
    expect_log="too many entity declarations",
)
def svg_many_entities() -> Page:
    decls = "".join(f'<!ENTITY n{i} "v">' for i in range(500))
    svg = f"<?xml version='1.0'?><!DOCTYPE svg [{decls}]><svg {SVG_NS} width='9' height='9'/>"
    return img_page(svg)


@case("SVG <use> bomb: 9 levels of 10 copies, 1e9 rects (ADR 0011)", expect_log="too many elements")
def svg_use_bomb() -> Page:
    defs = "<rect id='u0' width='1' height='1'/>"
    for i in range(1, 10):
        defs += f"<g id='u{i}'>" + f"<use href='#u{i - 1}'/>" * 10 + "</g>"
    return img_page(f"<svg {SVG_NS} width='50' height='50'>{defs}<use href='#u9'/></svg>")


@case(
    "SVG style sheet with 100,000 rules and 2,000 elements (ADR 0011)",
    time_limit_s=10,
    expect_log="style sheets too large",
)
def svg_css_rules() -> Page:
    rules = "".join(f".r{i}{{fill:red}}" for i in range(100_000))
    shapes = "".join(f"<rect class='r{i}' width='1' height='1'/>" for i in range(2000))
    return img_page(f"<svg {SVG_NS} width='50' height='50'><style>{rules}</style>{shapes}</svg>")


@case(
    "SVG groups nested 5,000 deep (limit 256 elements, 1024 effective, ADR 0011)",
    expect_log="nested deeper",
)
def svg_deep() -> Page:
    inner = nest("<g>", "</g>", 5000, "<rect width='5' height='5'/>")
    return img_page(f"<svg {SVG_NS} width='50' height='50'>{inner}</svg>")


@case("SVG source of 9 MiB (limit 8 MiB, ADR 0011)", expect_log="larger than")
def svg_big_source() -> Page:
    pad = "<!--" + "x" * (9 * MIB) + "-->"
    return img_page(f"<svg {SVG_NS} width='50' height='50'>{pad}<rect width='5' height='5'/></svg>")


@case("SVG images nested 8 deep in data: URLs (limit 4, ADR 0011)", expect_log="nested more than")
def svg_image_nesting() -> Page:
    svg = f"<svg {SVG_NS} width='20' height='20'><rect width='5' height='5' fill='red'/></svg>"
    for _ in range(8):
        data = base64.b64encode(svg.encode()).decode()
        svg = (
            f"<svg {SVG_NS} width='20' height='20'>"
            f"<image width='20' height='20' href='data:image/svg+xml;base64,{data}'/></svg>"
        )
    return img_page(svg)


@case(
    "SVG render cost: huge blur, morphology radius, dashes, radii, clip and mask chains",
    expect_log="too expensive",
)
def svg_render_cost() -> Page:
    shapes = (
        "<filter id='b' x='-100' y='-100' width='300' height='300'>"
        "<feGaussianBlur stdDeviation='1e6'/></filter>"
        "<filter id='m' x='-100' y='-100' width='300' height='300'>"
        "<feMorphology operator='dilate' radius='1e5'/></filter>"
        "<filter id='t' x='0' y='0' width='1' height='1'><feTurbulence baseFrequency='0.01' "
        "numOctaves='1000'/></filter>"
        "<rect width='100' height='100' filter='url(#b)'/>"
        "<rect width='100' height='100' filter='url(#m)'/>"
        "<rect width='100' height='100' filter='url(#t)'/>"
        "<path d='M0 0 L1e6 1e6' stroke='red' stroke-width='3' stroke-dasharray='0.001'/>"
        "<rect width='1e9' height='1e9' rx='1e9' ry='1e9' stroke='red' stroke-dasharray='0.01'/>"
        "<circle r='1e12' stroke='blue' stroke-dasharray='1 1'/>"
    )
    return img_page(f"<svg {SVG_NS} width='100' height='100'>{shapes}</svg>")


@case(
    "SVG patterns and masks that refer to each other 40 levels deep (ADR 0011)",
    expect_log="nested too deeply",
)
def svg_reference_chains() -> Page:
    defs = "<pattern id='p0' width='2' height='2' patternUnits='userSpaceOnUse'>"
    defs += "<rect width='1' height='1'/></pattern>"
    for i in range(1, 41):
        defs += f"<pattern id='p{i}' width='2' height='2' patternUnits='userSpaceOnUse'>"
        defs += f"<rect width='2' height='2' fill='url(#p{i - 1})' mask='url(#k{i - 1})'/>"
        defs += f"</pattern><mask id='k{i}'><rect width='9' height='9' fill='url(#p{i})'/></mask>"
    shapes = "<rect width='100' height='100' fill='url(#p40)'/>"
    return img_page(f"<svg {SVG_NS} width='100' height='100'><defs>{defs}</defs>{shapes}</svg>")


@case(
    "SVG markers at 200,000 vertices, and markers that contain paths with markers (ADR 0011)",
    expect_log="too many elements",
)
def svg_markers() -> Page:
    defs = "<marker id='in' markerWidth='3' markerHeight='3'><path d='M0 0 L3 3'/></marker>"
    defs += "<marker id='mk' markerWidth='9' markerHeight='9'>"
    defs += "<path d='M0 0 l1 1 l1 0 l1 1' marker-mid='url(#in)' stroke='blue'/></marker>"
    path = "M0 0 " + "l1 1 l-1 0 " * 100_000
    shapes = f"<path d='{path}' marker-mid='url(#mk)' marker-start='url(#mk)' stroke='red'/>"
    return img_page(f"<svg {SVG_NS} width='100' height='100'><defs>{defs}</defs>{shapes}</svg>")


# --- Floats (ADR 0015) -----------------------------------------------------


@case("25,000 floats in one block formatting context (MAX_FLOATS 10,000, ADR 0015)")
def floats_many() -> Page:
    floats = "<div style='float:left;width:3px;height:3px'></div>" * 25_000
    return Page(doc(floats + "<p>text after the floats</p>"))


@case("30,000 floats waiting in 250 nested empty blocks (ADR 0015)")
def floats_waiting() -> Page:
    floats = "<i style='float:left;width:1px;height:1px'></i>" * 30_000
    return Page(doc(nest("<div>", "</div>", 250, floats)))


@case("3,000 floats of 1270 px with 500 paragraphs pulled back over them (ADR 0015)")
def floats_staircase() -> Page:
    floats = "<div style='float:left;width:1270px;height:1px'></div>" * 3000
    paragraphs = "<p style='margin:-2000px 0 0'>" + "word " * 200 + "</p>"
    return Page(doc(floats + paragraphs * 500))


@case("200 nested BFC roots next to floats, each with text (work budget, ADR 0015)")
def floats_nested_bfc() -> Page:
    inner = "<div style='float:left;width:100px;height:50px'></div>" + "text " * 100
    root = (
        "<div style='overflow:hidden;width:90%'><b style='float:right;width:30px;height:9px'></b>"
    )
    return Page(doc(nest(root, "</div>", 200, inner)))


@case("8,000 floated links next to 8,000 short paragraphs (ADR 0015)")
def floats_links_paragraphs() -> Page:
    links = "<a style='float:left;width:20px;height:20px'>x</a>" * 8000
    return Page(doc(links + "<p>text text</p>" * 8000))


# --- Positioning and transforms (ADR 0016) ---------------------------------


@case("20,000 absolutely positioned siblings, 2,000 fixed and 2,000 sticky boxes (ADR 0016)")
def positioned_many() -> Page:
    style = "position:absolute;top:{}px;left:{}px;width:9px;height:9px"
    boxes = "".join(
        f"<div style='{style.format(i % 900, i % 1200)}'>x</div>" for i in range(20_000)
    )
    boxes += "<div style='position:fixed;top:5px;left:5px;width:5px;height:5px'></div>" * 2000
    boxes += "<div style='position:sticky;top:5px;height:5px'></div>" * 2000
    return Page(doc(boxes))


@case("positioned boxes nested 300 deep: absolute, fixed, sticky, relative (ADR 0016)")
def positioned_nesting() -> Page:
    kinds = ["absolute", "fixed", "sticky", "relative"]
    inner = "x"
    for i in range(300):
        inner = f"<div style='position:{kinds[i % 4]};top:1px;left:1px;padding:1px'>{inner}</div>"
    return Page(doc(inner))


@case(
    "300 nested rotated boxes and 2,000 viewport-sized rotated boxes (ADR 0016)",
    expect_log="transforms:",
)
def transforms_many() -> Page:
    nested = nest(
        "<div style='transform:rotate(1deg);width:900px;height:600px;padding:2px'>",
        "</div>",
        300,
        "x",
    )
    flat = "".join(
        f"<div style='position:absolute;inset:0;transform:rotate({i}deg);background:#0001'></div>"
        for i in range(2000)
    )
    return Page(doc(nested + flat))


@case("scale(1e30), scale(1e30, 1e-6), scale(0), matrix(1e38, ...) (ADR 0016)")
def transforms_extreme() -> Page:
    values = [
        "scale(1e30)",
        "scale(1e30,1e-6)",
        "scale(0)",
        "matrix(1e38,0,0,1e38,0,0)",
        "matrix(1,1e38,1e38,1,1e38,1e38)",
        "scale3d(1e30,1e30,1e30) rotate3d(1,1,1,1e30deg)",
        "perspective(1px) rotateY(89.9deg)",
        "matrix(0,0,0,0,0,0)",
        "translate(-1e38px) scale(1e-38)",
    ]
    body = "".join(
        f"<div style='transform:{v};width:300px;height:300px;background:red;opacity:.5'>t</div>"
        for v in values * 20
    )
    return Page(doc(body))


@case("opacity groups nested 300 deep and 2,000 opacity groups around small fixed boxes (ADR 0018)")
def opacity_nesting() -> Page:
    nested = nest("<div style='opacity:.99;padding:1px;background:#0001'>", "</div>", 300, "x")
    inner = "<div style='position:fixed;width:2px;height:2px;top:3px'></div>"
    fixed = f"<div style='opacity:.5'>{inner}</div>" * 2000
    return Page(doc(nested + fixed))


# --- Grid (ADR 0017) -------------------------------------------------------


@case(
    "grid: 4,000 grids of repeat(100000, 1px), 3 grids with 200,000 listed tracks (ADR 0017)",
    time_limit_s=10,
)
def grid_tracks() -> Page:
    repeat = (
        "<div style='display:grid;grid-template-columns:repeat(100000,1px)'><i>a</i><i>b</i></div>"
    )
    listed = "1px " * 200_000
    style = f"display:grid;grid-template-columns:{listed};grid-template-rows:{listed}"
    big = f"<div style='{style}'><i>a</i></div>"
    return Page(doc(repeat * 4000 + big * 3))


@case(
    "grid lines at ±1e9 on 5,000 items, spans of 1e9 (MAX_LINE 10,000, ADR 0017)",
    expect_log="grid limits reached",
)
def grid_lines() -> Page:
    items = "".join(
        f"<i style='grid-column:{-(10**9) + i} / {10**9 - i};grid-row:span 999999999 / {i}'>x</i>"
        for i in range(5000)
    )
    return Page(
        doc(f"<div style='display:grid;grid-template-columns:repeat(50,1fr)'>{items}</div>")
    )


@case(
    "120,000 named lines in one grid, items that refer to them (NAMED_LINE_BUDGET, ADR 0017)",
    expect_log="grid limits reached",
)
def grid_named_lines() -> Page:
    names = " ".join(f"l{i}" for i in range(12))
    template = f"repeat(10000,[{names}] 1px)"
    items = "".join(f"<i style='grid-column:l{i % 12} {i}'>x</i>" for i in range(2000))
    return Page(doc(f"<div style='display:grid;grid-template-columns:{template}'>{items}</div>"))


@case(
    "30,000 auto-placed items with spans of 100 rows, sparse and dense (PLACEMENT_WORK, ADR 0017)",
    expect_log="grid limits reached",
)
def grid_autoplace() -> Page:
    items = "<i style='grid-row:span 100'>x</i>" * 30_000
    sparse = f"<div style='display:grid;grid-template-columns:repeat(100,5px)'>{items}</div>"
    dense = (
        "<div style='display:grid;grid-auto-flow:row dense;grid-template-columns:repeat(100,5px)'>"
        f"{items}</div>"
    )
    return Page(doc(sparse + dense))


@case("grids and flex containers nested 300 deep (ADR 0017, box depth 256)")
def grid_flex_nesting() -> Page:
    inner = "x"
    for i in range(300):
        kind = "grid" if i % 2 else "flex"
        inner = f"<div style='display:{kind};padding:1px'>{inner}<i>y</i></div>"
    return Page(doc(inner))


@case("a wrapping flex container with 50,000 items of growing sizes")
def flex_many_items() -> Page:
    items = "".join(f"<i style='flex:{i % 5} 1 {i % 90}px'>x</i>" for i in range(50_000))
    return Page(doc(f"<div style='display:flex;flex-wrap:wrap'>{items}</div>"))


# --- Masks and scroll containers (ADR 0018, 0019) --------------------------


@case("mask with 10,000 layers on 500 boxes (MAX_MASK_LAYERS 32, ADR 0018)")
def mask_layers() -> Page:
    layers = ",".join("linear-gradient(#000,transparent)" for _ in range(10_000))
    return Page(
        doc(
            "<div style='width:50px;height:50px;background:red'></div>" * 500,
            f"div{{mask:{layers}}}",
        )
    )


@case(
    "300 nested masked boxes, and 500 overlapping viewport-sized masked boxes (ADR 0018)",
    expect_log="work budget",
)
def mask_nesting() -> Page:
    mask = "mask-image:linear-gradient(#000,transparent)"
    nested = nest(f"<div style='{mask};padding:1px;background:#00f1'>", "</div>", 300, "x")
    flat = f"<div style='position:absolute;inset:0;{mask};background:red'></div>" * 500
    return Page(doc(nested + flat))


@case("ten 10000x10000 px boxes with 32 tiled gradient mask layers, 2x2 px visible (ADR 0018)")
def mask_tiled_gradients() -> Page:
    layers = ",".join("linear-gradient(90deg,red,blue)" for _ in range(32))
    box = (
        f"<div style='width:10000px;height:10000px;margin:-9998px 0 0;background:red;"
        f"mask-image:{layers};mask-size:3px 3px'></div>"
    )
    return Page(doc(box * 10))


@case(
    "200 overlapping viewport-sized boxes with 32 mask layers, full page at scale 8 (ADR 0018)",
    time_limit_s=10,
    rss_limit_mib=2048,
    swb_args=("--full-page", "--scale", "8"),
    expect_log="work budget",
)
def mask_full_page_scale() -> Page:
    layers = ",".join("linear-gradient(red,blue)" for _ in range(32))
    box = (
        f"<div style='position:absolute;inset:0 0 -800px;mask-image:{layers};background:red'></div>"
    )
    return Page(doc(box * 200))


@case(
    "clip-path: polygons of 1,024 and 100,000 points, relative-unit polygons of 128 and 100,000 "
    "points, paths of 1,000 and 200,000 segments (MAX_SHAPE_POINTS, ADR 0028 section 16)"
)
def clip_path_limits() -> Page:
    def points(n: int, unit: str = "px") -> str:
        return ",".join(f"{i % 50}{unit} {i % 7}{unit}" for i in range(n))

    def path(n: int) -> str:
        return "M0 0" + " L1 1" * n

    css = (
        f".a{{clip-path:polygon({points(1024)})}}.b{{clip-path:polygon({points(100_000)})}}"
        f".c{{clip-path:polygon({points(128, 'em')})}}"
        f".d{{clip-path:polygon({points(100_000, 'em')})}}"
        f".e{{clip-path:path('{path(1000)}')}}.f{{clip-path:path('{path(200_000)}')}}"
        "div{width:50px;height:50px;background:red}"
    )
    return Page(doc("".join(f"<div class={c}></div>" for c in "abcdef"), css))


@case(
    "clip-path on 20,000 boxes, 500 boxes with a 1,024-point polygon, and 300 nested boxes "
    "(ADR 0028)",
    time_limit_s=10,
)
def clip_path_many() -> Page:
    polygon = ",".join(f"{i % 50}px {i % 7}px" for i in range(1024))
    css = f".p{{clip-path:polygon({polygon})}}.c{{clip-path:circle(40%)}}"
    css += "div{width:30px;height:30px;background:red}"
    shapes = "<div class=c></div>" * 20_000
    polygons = "<div class=p></div>" * 500
    nested = nest("<div class=c style='width:auto;height:auto;padding:1px'>", "</div>", 300, "x")
    return Page(doc(shapes + polygons + nested, css))


@case(
    "clip-path with huge radii, percentages and offsets: circle(1e9%), inset(-1e9px round 1e9px), "
    'ellipse(1e9px 1e-9px), path("M1e9 1e9 L-1e9 -1e9") (ADR 0028)'
)
def clip_path_extreme_values() -> Page:
    shapes = [
        "circle(1e9%)",
        "circle(1e9px at 1e9px -1e9px)",
        "ellipse(1e9px 1e-9px)",
        "inset(-1e9px round 1e9px)",
        "inset(1e9px)",
        "inset(0 round 1e9% / 1e9px)",
        "polygon(1e9px 1e9px, -1e9px 0, 0 1e9%)",
        "path('M1e9 1e9 L-1e9 -1e9 L0 1e9 z')",
        "path('M0 0 A 1e9 1e9 0 1 1 1e-9 1e-9 z')",
        "xywh(-1e9px -1e9px 1e9px 1e9px round 1e9px)",
        "rect(1e9px 1e9px -1e9px -1e9px)",
        "circle(calc(1e9px * 1e9) at calc(100% * 1e9) 0)",
    ]
    boxes = "".join(
        f"<div style='clip-path:{s};width:200px;height:100px;background:red;"
        f"transform:rotate(7deg)'></div>"
        for s in shapes * 20
    )
    return Page(doc(boxes))


@case(
    "filter with huge lengths: blur(1e9px), blur(100000px), drop-shadow(1e9px -1e9px 1e9px) and "
    "drop-shadow(1e-9px 1e-9px 0.001px) on boxes (the blur is limited to 256 device px, "
    "MAX_SIGMA; ADR 0028 section 16)"
)
def filter_huge_lengths() -> Page:
    filters = [
        "blur(1e9px)",
        "blur(100000px)",
        "blur(300px)",
        "drop-shadow(1e9px -1e9px 1e9px red)",
        "drop-shadow(1e-9px 1e-9px 0.001px red)",
        "blur(1e9px) drop-shadow(1e9px 1e9px 1e9px blue) blur(1e9px)",
        "blur(0.0001px) blur(1e-9px)",
    ]
    boxes = "".join(
        f"<div style='filter:{f};width:300px;height:200px;background:linear-gradient("
        f"rgba(255,0,0,0),#000)'><p>text</p></div>"
        for f in filters * 3
    )
    return Page(doc(boxes))


@case(
    "filter with 64 functions (the limit, MAX_FILTER_FUNCTIONS) of blurs on a box the size of the "
    "viewport: the filter work budget acts and the box is drawn without the rest of its filter; "
    "and a declaration of 100,000 functions is invalid (ADR 0028 section 16)",
    time_limit_s=10,
    expect_log="filters: the work budget",
)
def filter_many_functions() -> Page:
    blurs = " ".join(["blur(40px)"] * 64)
    too_many = " ".join(["invert(0.01)"] * 100_000)
    css = (
        f".a{{filter:{blurs}}}.b{{filter:{too_many}}}"
        "div{width:1200px;height:700px;background:linear-gradient(red,blue)}"
    )
    return Page(doc("<div class=a></div><div class=b></div>", css))


@case(
    "filter on 300 nested boxes (blur(2px) at every level, and invert), and on 5,000 boxes "
    "(ADR 0028 section 16)",
    time_limit_s=10,
)
def filter_nested_and_many() -> Page:
    nested = nest(
        "<div style='filter:blur(2px) invert(0.5);padding:1px;background:rgba(0,0,255,.1)'>",
        "</div>",
        300,
        "text",
    )
    box = "<i style='display:block;width:20px;height:20px;filter:invert(1);background:red'></i>"
    many = box * 5000
    return Page(doc(nested + many))


@case(
    "filter on a box of 100,000 by 100,000 px with a blur, a box of 100,000 px with a drop "
    "shadow, and the same boxes in a screenshot of the viewport at scale 4 (ADR 0028 "
    "section 16)",
    swb_args=("--scale", "4"),
)
def filter_huge_box() -> Page:
    huge = (
        "<div style='width:100000px;height:100000px;filter:blur(20px);background:red'></div>"
        "<div style='position:absolute;left:0;top:0;width:100000px;height:100000px;"
        "filter:drop-shadow(30px 30px 20px blue);background:rgba(0,255,0,.3)'></div>"
    )
    return Page(doc(huge))


@case(
    "filter on 200 overlapping boxes of 2,000 by 2,000 px, each with a blur, a drop shadow, a "
    "rotation and a mask (ADR 0028 section 16)",
    time_limit_s=10,
    rss_limit_mib=2048,
)
def filter_large_boxes() -> Page:
    many = (
        "<div style='position:absolute;left:0;top:0;width:2000px;height:2000px;"
        "filter:blur(30px) drop-shadow(20px 20px 10px blue);background:rgba(255,0,0,.3);"
        "transform:rotate(3deg);-webkit-mask-image:linear-gradient(#000,transparent)'></div>"
    ) * 200
    return Page(doc(many))


@case(
    "backdrop-filter: blur(100px) on 1,000 nested boxes (each one is a backdrop root with a "
    "layer; the box depth limit 256, the layer and filter budgets act; ADR 0033)",
    time_limit_s=10,
    rss_limit_mib=2048,
)
def backdrop_filter_nested() -> Page:
    nested = nest(
        "<div style='backdrop-filter:blur(100px);padding:1px;background:rgba(0,0,255,.05)'>",
        "</div>",
        1000,
        "text",
    )
    return Page(doc(nested))


@case(
    "backdrop-filter on boxes the size of the viewport with huge lengths: blur(1e9px), "
    "blur(100000px), drop-shadow(1e9px 1e9px 1e9px), 64 blurs; over a gradient (the blur is "
    "limited to 256 device px and the filter work budget acts; ADR 0033)",
    time_limit_s=10,
    expect_log="filters: the work budget",
)
def backdrop_filter_viewport_huge() -> Page:
    blurs = " ".join(["blur(40px)"] * 64)
    filters = [
        "blur(1e9px)",
        "blur(100000px)",
        "drop-shadow(1e9px 1e9px 1e9px red)",
        "blur(1e9px) drop-shadow(1e9px 1e9px 1e9px blue) blur(1e9px)",
        blurs,
    ]
    boxes = "".join(
        f"<div style='position:fixed;left:0;top:0;right:0;bottom:0;backdrop-filter:{f}'></div>"
        for f in filters * 3
    )
    css = "body{height:3000px;background:linear-gradient(red,blue,green)}"
    return Page(doc("<p>text</p>" + boxes, css))


@case(
    "backdrop-filter on 3,000 boxes of 300 by 100 px with blur(10px) in a page of 30,000 px "
    "(full-page screenshot in strips), over text and gradients; each box is a backdrop root "
    "of the next (ADR 0033)",
    time_limit_s=15,
    swb_args=("--full-page",),
)
def backdrop_filter_many_in_long_page() -> Page:
    box = (
        "<div style='position:relative;width:300px;height:100px;margin-bottom:-90px;"
        "backdrop-filter:blur(10px);background:rgba(255,255,255,.3)'>"
        "<span style='position:absolute;left:150px;top:0;width:3px;height:100px;"
        "background:#000'></span></div>"
    )
    css = "body{background:repeating-linear-gradient(90deg,#fff,#00f 7px)}"
    return Page(doc("<p>text</p>" + box * 3000, css))


@case(
    "scroll containers nested 300 deep with large content, and 20,000 sibling scrollers (ADR 0019)"
)
def scroll_containers() -> Page:
    inner = "<div style='height:5000px;width:5000px'>x</div>"
    nested = nest("<div style='overflow:auto;height:900px;width:1000px'>", "</div>", 300, inner)
    flat = (
        "<div style='overflow:scroll;width:20px;height:20px'><p>text text text</p></div>" * 20_000
    )
    return Page(doc(nested + flat))


@case("scroll containers with overflow of 1e30 px and 100,000 abspos children (ADR 0019)")
def scroll_overflow() -> Page:
    child = "<i style='position:absolute;left:1e30px;top:1e30px'>x</i>"
    far = "<div style='position:relative;overflow:auto;width:100px;height:100px'>" + child * 100_000
    return Page(doc(far + "</div>"))


# --- Web fonts (ADR 0022) --------------------------------------------------

WEB_FONT_FILE = "crates/text/tests/webfonts/dejavu-subset.ttf"
"""A small valid TrueType font (a DejaVu Sans subset) in the repository."""


def _web_font() -> bytes:
    from swbtools import paths

    return (paths.repo_root() / WEB_FONT_FILE).read_bytes()


@case(
    "100,000 @font-face rules in one family with one code point each, past the limits of"
    " 10,000 rules and 1,000 faces per family (ADR 0022)",
    expect_log="@font-face rules; ignoring the rest",
)
def font_face_many_rules() -> Page:
    rules = "".join(
        f"@font-face{{font-family:F;src:url(f{i}.woff2);unicode-range:U+{i:X}}}"
        for i in range(100_000)
    )
    text = "".join(chr(0x21 + i % 0x5E) for i in range(5_000))
    return Page(doc(f"<p style='font-family:F'>{text}</p>", rules))


@case("an @font-face rule with 300,000 unicode-range entries, used by long text (ADR 0022)")
def font_face_long_unicode_range() -> Page:
    ranges = ",".join(f"U+{i * 3:X}-{i * 3 + 1:X}" for i in range(300_000))
    rules = f"@font-face{{font-family:R;src:url(f.ttf);unicode-range:{ranges}}}"
    text = "".join(chr(0x21 + i % 0x5E) for i in range(20_000))
    return Page(doc(f"<p style='font-family:R'>{text}</p>", rules), {"f.ttf": _web_font()})


@case(
    "5,000 web font families in use, each with its own font URL, past the limit of 1,000"
    " faces loaded (ADR 0022)",
    time_limit_s=10,
    expect_log="web font faces; not loading more",
)
def font_families_many() -> Page:
    rules = "".join(f"@font-face{{font-family:F{i};src:url(f.ttf?{i})}}" for i in range(5_000))
    spans = "".join(f"<span style='font-family:F{i}'>x{i} </span>" for i in range(5_000))
    return Page(doc(spans, rules), {"f.ttf": _web_font()})


@case(
    "1,000 loaded faces of one family with the same descriptors and 300,000 characters that"
    " no face has (ADR 0022)",
)
def font_composite_many_faces() -> Page:
    rules = "".join(f"@font-face{{font-family:C;src:url(f.ttf?{i})}}" for i in range(1_000))
    text = "".join(chr(0x400 + i % 0x100) for i in range(300_000))
    return Page(doc(f"<p style='font-family:C'>{text}</p>", rules), {"f.ttf": _web_font()})


@case(
    "1,000 loaded faces of one family with the same descriptors and the CJK block (20,992"
    " distinct characters) that no face has; at most 256 faces are checked (ADR 0022)",
)
def font_composite_many_chars() -> Page:
    rules = "".join(f"@font-face{{font-family:C;src:url(f.ttf?{i})}}" for i in range(1_000))
    text = "".join(chr(0x4E00 + i) for i in range(0x5200))
    return Page(doc(f"<p style='font-family:C'>{text}</p>", rules), {"f.ttf": _web_font()})


def _woff2_bomb() -> bytes:
    """A WOFF2 header and one table that declares a decoded size of 2^31
    bytes, with 16 bytes of data."""
    import struct

    directory = bytes([0]) + bytes([0x88, 0x80, 0x80, 0x80, 0x00])
    data = bytes(16)
    length = 48 + len(directory) + len(data)
    header = b"wOF2" + struct.pack(
        ">IIHHIIHHIIIII", 0x00010000, length, 1, 0, 0xFFFFFFFF, 16, 0, 0, 0, 0, 0, 0, 0
    )
    return header + directory + data


def _woff1_bomb() -> bytes:
    """A WOFF 1.0 file with 1,000 compressed tables that all point at the
    same small zlib stream, which expands to 40,000 bytes: the sum of
    `origLength` is 40 MB, above the limit of 32 MiB."""
    import struct
    import zlib

    tables = 1000
    data_at = 44 + 20 * tables
    original = 40_000
    blob = zlib.compress(bytes(original))
    length = data_at + len(blob)
    header = b"wOFF" + struct.pack(
        ">IIHHIHHIIIII", 0x00010000, length, tables, 0, 0xFFFFFFFF, 1, 0, 0, 0, 0, 0, 0
    )
    entries = b"".join(
        struct.pack(">4sIIII", f"t{i:03d}".encode(), data_at, len(blob), original, 0)
        for i in range(tables)
    )
    return header + entries + blob


@case("web fonts that fail to decode: garbage, truncated, decompression bombs (ADR 0022)")
def font_decode_failures() -> Page:
    good = _web_font()
    files: dict[str, str | bytes] = {
        "garbage.woff2": bytes(range(256)) * 64,
        "empty.ttf": b"",
        "truncated.ttf": good[: len(good) // 3],
        "bomb.woff2": _woff2_bomb(),
        "bomb.woff": _woff1_bomb(),
        "html.ttf": "<!doctype html><p>not a font",
    }
    rules = "".join(
        f"@font-face{{font-family:D{i};src:url({name}),url(missing-{i}.ttf)}}"
        for i, name in enumerate(files)
    )
    paragraphs = "".join(f"<p style='font-family:D{i}'>text {i}</p>" for i in range(len(files)))
    return Page(doc(paragraphs, rules), files)


def _composite_bomb_font(depth: int = 14, points: int = 3000) -> bytes:
    """A TrueType font whose glyph for `A` nests composites `depth` deep,
    each with two copies of the level below, over a contour of `points`
    points (2^depth copies when expanded); `maxp` claims the largest
    values."""
    import io

    from fontTools.fontBuilder import FontBuilder
    from fontTools.pens.ttGlyphPen import TTGlyphPen
    from fontTools.ttLib import TTFont
    from fontTools.ttLib.tables._g_l_y_f import Glyph, GlyphComponent

    names = [".notdef", "base"] + [f"g{i}" for i in range(1, depth + 1)]
    builder = FontBuilder(1000, isTTF=True)
    builder.setupGlyphOrder(names)
    builder.setupCharacterMap({0x41: f"g{depth}", 0x42: "base"})
    pen = TTGlyphPen(None)
    pen.moveTo((0, 0))
    for i in range(points):
        pen.lineTo((i % 500, (i * 7) % 700))
    pen.closePath()
    glyphs = {".notdef": TTGlyphPen(None).glyph(), "base": pen.glyph()}
    for i in range(1, depth + 1):
        glyph = Glyph()
        glyph.numberOfContours = -1
        glyph.components = []
        for dx in (0, 1):
            component = GlyphComponent()
            component.glyphName = "base"
            component.x, component.y = dx, 0
            component.flags = 0x4
            glyph.components.append(component)
        glyphs[f"g{i}"] = glyph
    builder.setupGlyf(glyphs)
    builder.setupHorizontalMetrics({n: (600, 0) for n in names})
    builder.setupHorizontalHeader(ascent=800, descent=-200)
    builder.setupNameTable({"familyName": "Bomb", "styleName": "Regular"})
    builder.setupOS2()
    builder.setupPost()
    first = io.BytesIO()
    builder.save(first)
    # Point each level at the level below without recomputing bounds:
    # fontTools itself would expand the composites.
    font = TTFont(io.BytesIO(first.getvalue()), recalcBBoxes=False)
    for i in range(2, depth + 1):
        for component in font["glyf"][f"g{i}"].components:
            component.glyphName = f"g{i - 1}"
    maxp = font["maxp"]
    maxp.maxPoints = maxp.maxCompositePoints = 0xFFFF
    maxp.maxContours = maxp.maxCompositeContours = 0xFFFF
    maxp.maxComponentElements = maxp.maxComponentDepth = 0xFFFF
    out = io.BytesIO()
    font.save(out)
    return out.getvalue()


@case("a web font with composite glyphs nested 14 deep (16,384 copies of 3,000 points) (ADR 0022)")
def font_composite_glyphs() -> Page:
    text = "AAAA BBBB " * 200
    rules = "@font-face{font-family:B;src:url(b.ttf)}"
    return Page(doc(f"<p style='font:60px B'>{text}</p>", rules), {"b.ttf": _composite_bomb_font()})


# --- Font settings, metric descriptors, local() (ADR 0022, part 2) ----------

VARIABLE_FONT_FILE = "crates/text/tests/webfonts/swb-variable.ttf"
"""The variable test font of the web font tests (axes `wght`, `wdth`, `SWBX`)."""


def _variable_font() -> bytes:
    from swbtools import paths

    return (paths.repo_root() / VARIABLE_FONT_FILE).read_bytes()


@case(
    "font-variation-settings and font-feature-settings with 200,000 distinct tags each, in"
    " properties and @font-face descriptors, past the limit of 64 tags per declaration"
    " (ADR 0022)",
    expect_log="font-variation-settings: more than 64 tags",
)
def font_settings_long_lists() -> Page:
    def tags(n: int) -> list[str]:
        letters = "ABCDEFGHIJKLMNOPQRSTUVWXYZ"
        return [
            letters[i // 676 % 26] + letters[i // 26 % 26] + letters[i % 26] + "x" for i in range(n)
        ]

    variations = ",".join(f'"{tag}" {i}' for i, tag in enumerate(tags(200_000)))
    features = ",".join(f'"{tag}" {i}' for i, tag in enumerate(tags(200_000)))
    rules = (
        "@font-face{font-family:V;src:url(v.ttf);font-weight:100 900;"
        f"font-variation-settings:{variations};font-feature-settings:{features}}}"
    )
    style = f"font-family:V;font-variation-settings:{variations};font-feature-settings:{features}"
    body = f"<p style='{style}'>AAAA BBBB " + "AB " * 2000 + "</p>"
    return Page(doc(body, rules), {"v.ttf": _variable_font()})


@case(
    "extreme numbers in font-variation-settings, font-feature-settings, size-adjust and the"
    " metric overrides: 1e308, 1e400, -1e999, 2^32, a million percent (ADR 0022)",
)
def font_settings_extreme_values() -> Page:
    values = ["1e308", "1e400", "-1e999", "4294967295", "99999999999", "-99999999999", "0.0000001"]
    rules = ""
    body = ""
    for i, value in enumerate(values):
        rules += (
            f"@font-face{{font-family:E{i};src:url(v.ttf);font-weight:100 900;"
            f"size-adjust:{value}%;ascent-override:{value}%;descent-override:{value}%;"
            f"line-gap-override:{value}%;font-variation-settings:'wght' {value},'SWBX' {value};"
            f"font-feature-settings:'liga' {value}}}"
        )
        style = (
            f"font:24px E{i};font-variation-settings:'wdth' {value},'wght' {value};"
            f"font-feature-settings:'kern' {value},'liga' {value}"
        )
        body += f'<p style="{style}">AAAA BBBB fi fl ffi</p>'
    return Page(doc(body, rules), {"v.ttf": _variable_font()})


@case(
    "20,000 elements with distinct font-variation-settings on a variable font, past the limit of"
    " 4,096 remembered lists (ADR 0022)",
    expect_log="font-variation-settings lists",
)
def font_variation_many_lists() -> Page:
    rules = "@font-face{font-family:V;src:url(v.ttf);font-weight:100 900}"
    spans = "".join(
        '<span style="font-family:V;font-variation-settings:'
        f"'wght' {100 + i % 800}.{i // 800:02d}\">A{i} </span>"
        for i in range(20_000)
    )
    return Page(doc(spans, rules), {"v.ttf": _variable_font()})


@case(
    "60,000 elements with distinct font-feature-settings lists on one font: each list needs its"
    " own shape plan, at most 32 are kept per font (ADR 0022)"
)
def font_feature_many_lists() -> Page:
    spans = "".join(
        f"<span style=\"font-feature-settings:'liga' {i}\">fi{i} </span>" for i in range(60_000)
    )
    return Page(doc(spans))


@case(
    "1,000 @font-face rules with local() names of 10 KB, and 5,000 local() faces in use, past"
    " the limit of 1,000 face loads (ADR 0022)",
    expect_log="web font faces; not loading more",
)
def font_local_many_and_long() -> Page:
    long_rules = "".join(
        f"@font-face{{font-family:L{i};src:local('{'x' * 10_000}{i}')}}" for i in range(1_000)
    )
    many_rules = "".join(
        f"@font-face{{font-family:M{i};src:local('Nope {i}'),local('Liberation Sans')}}"
        for i in range(5_000)
    )
    spans = "".join(f"<span style='font-family:M{i}'>x{i} </span>" for i in range(5_000))
    spans += "".join(f"<span style='font-family:L{i}'>y{i} </span>" for i in range(1_000))
    return Page(doc(spans, long_rules + many_rules))


# --- Inline SVG (ADR 0023) ---------------------------------------------------


@case(
    "inline SVG: 60,000 shapes in 600 <svg> elements, past the limit of 50,000 per document "
    "(ADR 0023)",
    expect_log="shapes in the document",
)
def inline_svg_many_shapes() -> Page:
    svg = "<svg width=20 height=20>" + "<rect width=9 height=9 fill=red />" * 100 + "</svg>"
    return Page(doc(svg * 600))


@case(
    "inline SVG: a path with 1,200,000 segments, past the limit of 1,000,000 per document "
    "(ADR 0023)",
    expect_log="path segments in the document",
)
def inline_svg_many_segments() -> Page:
    d = "M0 0" + "l1 1" * 1_200_000
    return Page(doc(f"<svg width=200 height=200><path d='{d}' stroke=black /></svg>"))


@case(
    "inline SVG: groups nested 500 deep, past the limit of 64 (ADR 0023)",
    expect_log="groups nested more than",
)
def inline_svg_deep_groups() -> Page:
    inner = nest("<g opacity=0.9>", "</g>", 500, "<rect width=100 height=100 />")
    return Page(doc(f"<svg width=200 height=200>{inner}</svg>"))


@case(
    "inline SVG: 2,000 strokes with dash arrays of 256 entries (the limit) and 2,000 with 300 "
    "(invalid) (ADR 0023)"
)
def inline_svg_long_dash_arrays() -> Page:
    ok = " ".join(["0.5"] * 256)
    long = " ".join(["0.5"] * 300)
    shapes = f"<path d='M0 0L1000 1000' stroke=black stroke-dasharray='{ok}' />" * 2_000
    shapes += f"<path d='M0 0L1000 9' stroke=black stroke-dasharray='{long}' />" * 2_000
    return Page(doc(f"<svg width=1000 height=800>{shapes}</svg>"))


@case(
    "inline SVG: strokes with dashes of 1e-6 px on long paths, past the limit of 100,000 "
    "dashes per stroke (ADR 0023)",
    expect_log="too many dashes",
)
def inline_svg_tiny_dashes() -> Page:
    shapes = (
        "<path d='M0 10H1000V700H0Z' stroke=black stroke-width=3 "
        "stroke-dasharray='0.000001 0.000001' fill=none />"
    ) * 200
    return Page(doc(f"<svg width=1000 height=800>{shapes}</svg>"))


@case(
    "inline SVG: 3,000 translucent shapes that cover the viewport, past the path work budget "
    "(ADR 0023)",
    expect_log="path work budget ran out",
)
def inline_svg_paint_budget() -> Page:
    shapes = "<rect width=1280 height=800 fill=red fill-opacity=0.01 />" * 3_000
    return Page(doc(f"<svg width=1280 height=800>{shapes}</svg>"))


@case(
    "inline SVG: 3,000 opaque shapes that cover the viewport, each in a group, past the path "
    "work budget (ADR 0023)",
    expect_log="path work budget ran out",
)
def inline_svg_opaque_fills() -> Page:
    shapes = "<g><rect width=1200 height=800 fill=red /></g>" * 3_000
    return Page(doc(f"<svg width=1280 height=800>{shapes}</svg>"))


@case(
    "inline SVG: 20,000 groups with opacity, each with a viewport-size shape, past the limit "
    "of 256 opacity layers per document (ADR 0023)",
    expect_log="opacity layers in the document",
)
def inline_svg_opacity_layers() -> Page:
    group = (
        "<g opacity=.5><rect width=1200 height=800 fill=red />"
        "<rect width=3 height=3 stroke=blue /></g>"
    )
    return Page(doc(f"<svg width=1280 height=800>{group * 20_000}</svg>"))


@case("inline SVG: huge transforms, stroke widths, radii, view boxes and coordinates (ADR 0023)")
def inline_svg_huge_geometry() -> Page:
    shapes = [
        "<rect width=10 height=10 transform='scale(1e30)' />",
        "<rect width=10 height=10 transform='matrix(1e38 0 0 1e38 -1e38 -1e38)' />",
        "<rect width=1e38 height=1e38 x=-1e38 y=-1e38 stroke=red stroke-width=1e38 />",
        "<path d='M0 0A1e30 1e30 0 1 1 1 1A1e-30 1e30 45 0 0 2 2Z' stroke=blue />",
        "<circle r=1e38 cx=-1e38 stroke=red stroke-width=1e-38 />",
        "<ellipse rx=1e20 ry=1e-20 stroke-dasharray='1e38 1' stroke=black />",
        "<polyline points='0,0 1e38,1e38 -1e38,1e38 0,0' stroke=black stroke-width=1e9 "
        "stroke-linejoin=miter stroke-miterlimit=1e9 />",
        "<g style='transform: rotate(1e30deg) scale(1e-30)'><rect width=1e30 height=1 /></g>",
        "<line x2=1e38 y2=1e38 stroke=black stroke-dashoffset=1e38 stroke-dasharray=1 />",
        "<rect width='1e30%' height='1e30%' rx='1e30%' />",
    ]
    svgs = "".join(
        f"<svg width=300 height=200 viewBox='{vb}'>{''.join(shapes)}</svg>"
        for vb in ("0 0 300 200", "0 0 1e-30 1e-30", "-1e38 -1e38 1e38 1e38", "0 0 1e38 1")
    )
    svgs += "<svg width=1e9 height=1e9 viewBox='0 0 1 1'><rect width=1 height=1 /></svg>"
    return Page(doc(svgs))


@case(
    "inline SVG: 20 levels of a group of two <use> of the level below (2^20 copies), past the "
    "limit of 20,000 elements in the instances of use elements (ADR 0023)",
    expect_log="elements in the instances",
)
def inline_svg_use_bomb() -> Page:
    levels = "<rect id=l0 width=5 height=5 />"
    for i in range(1, 21):
        levels += f"<g id=l{i}><use href='#l{i - 1}' x=1 /><use href='#l{i - 1}' y=1 /></g>"
    return Page(doc(f"<svg width=200 height=200><defs>{levels}</defs><use href='#l20' /></svg>"))


@case(
    "inline SVG: 10,000 <use> of a group of 50 shapes (510,000 copies), past the limit of 20,000 "
    "elements in the instances of use elements (ADR 0023)",
    expect_log="elements in the instances",
)
def inline_svg_many_uses() -> Page:
    group = "<g id=big>" + "<rect width=5 height=5 />" * 50 + "</g>"
    uses = "<use href='#big' x=3 />" * 10_000
    return Page(doc(f"<svg width=200 height=200><defs>{group}</defs>{uses}</svg>"))


@case(
    "inline SVG: use elements that refer to themselves, to their ancestors, in a cycle, and in "
    "a chain of 5,000 (ADR 0023)"
)
def inline_svg_use_cycles() -> Page:
    chain = "".join(f"<use id=c{i} href='#c{i + 1}' />" for i in range(5_000))
    body = (
        "<g id=a><use href='#a' /><use href='#b' /></g><g id=b><use href='#a' /></g>"
        "<use id=s href='#s' /><use href='#a' />" + chain
    )
    return Page(doc(f"<svg width=200 height=200>{body}<rect id=c5000 width=9 height=9 /></svg>"))


@case(
    "inline SVG: 1,000 clip paths in a chain of clip-path references, and a cycle, past the "
    "depth of 8 (ADR 0023)"
)
def inline_svg_clip_chain() -> Page:
    paths = "".join(
        f"<clipPath id=k{i} clip-path='url(#k{i + 1})'><rect width=150 height=150 /></clipPath>"
        for i in range(1_000)
    )
    paths += "<clipPath id=k1000 clip-path='url(#k0)'><circle cx=50 cy=50 r=40 /></clipPath>"
    shapes = "<rect width=100 height=100 clip-path='url(#k0)' />" * 1_000
    return Page(doc(f"<svg width=200 height=200><defs>{paths}</defs>{shapes}</svg>"))


@case(
    "inline SVG: 20,000 groups with a clip path that is not a rectangle, each with a "
    "viewport-size shape, past the limit of 256 clip paths that need a layer (ADR 0023)",
    expect_log="clip paths that need a layer",
)
def inline_svg_clip_layers() -> Page:
    clip = "<clipPath id=k><circle cx=640 cy=400 r=300 /></clipPath>"
    group = "<g clip-path='url(#k)'><rect width=1280 height=800 fill=red /></g>"
    return Page(doc(f"<svg width=1280 height=800><defs>{clip}</defs>{group * 20_000}</svg>"))


@case(
    "inline SVG: one clip path of 10,000 shapes used by 10,000 elements, past the limit of "
    "20,000 clip shapes per <svg> (ADR 0023)",
    expect_log="too many clip path shapes",
)
def inline_svg_clip_shapes() -> Page:
    clip = "<clipPath id=k>" + "<circle cx=50 cy=50 r=3 />" * 10_000 + "</clipPath>"
    shapes = "<rect width=100 height=100 clip-path='url(#k)' />" * 10_000
    return Page(doc(f"<svg width=200 height=200><defs>{clip}</defs>{shapes}</svg>"))


@case(
    "inline SVG: 19,000 <use> of a group with a display:none child that holds 300,000 groups: "
    "the hidden subtree is not walked (ADR 0023)"
)
def inline_svg_use_hidden_subtree() -> Page:
    hidden = "<g style='display:none'>" + "<g/>" * 300_000 + "</g>"
    defs = f"<defs><g id=g>{hidden}</g></defs>"
    return Page(doc(f"<svg width=200 height=200>{defs}{'<use href=#g />' * 19_000}</svg>"))


@case(
    "inline SVG: one clip path of 5,000 circles that each have a clip-path, used by 200 groups "
    "on a 1280 x 800 <svg>: the passes over pixels count against the work budget (ADR 0023)"
)
def inline_svg_clip_nested_shapes() -> Page:
    inner = "<clipPath id=i><circle cx=640 cy=400 r=300 /></clipPath>"
    circles = "<circle cx=640 cy=400 r=250 clip-path='url(#i)' />" * 5_000
    group = "<g clip-path='url(#k)'><rect width=1280 height=800 fill=red /></g>"
    defs = f"<defs>{inner}<clipPath id=k>{circles}</clipPath></defs>"
    return Page(doc(f"<svg width=1280 height=800>{defs}{group * 200}</svg>"))


def dense_path(curves: int = 40_000) -> str:
    """The `d` of a path of overlapping cubic curves in a 100 x 100 box: each curve
    reaches across the whole height, so tiny-skia's scan converter has `curves` active
    edges that cross each other (about 4 s for 40,000, the square of the segments)."""
    parts = ["M0 0"]
    for i in range(curves):
        k = i % 50
        parts.append(f"C{k} 0 {100 - k} 100 {(7 * i) % 98} 50")
    return "".join(parts)


@case(
    "inline SVG: four paths of 40,000 overlapping curves in a 100 x 100 <svg>, filled with and "
    "without anti-aliasing, stroked, and filled and stroked: the time of a scan conversion "
    "grows with the square of the segments, so the pairs of edges count against the path work "
    "budget (ADR 0023)",
    expect_log="path work budget ran out",
)
def inline_svg_dense_paths() -> Page:
    d = dense_path()
    paths = (
        f"<path d='{d}' fill=red />"
        f"<path d='{d}' fill=red shape-rendering=crispEdges />"
        f"<path d='{d}' fill=none stroke=blue stroke-width=2 />"
        f"<path d='{d}' fill=green stroke=blue />"
    )
    return Page(doc(f"<svg width=100 height=100>{paths}</svg>"))


@case(
    "inline SVG: a clip path of one path of 40,000 overlapping curves, used by five elements: "
    "the coverage fill counts the pairs of edges (ADR 0023)",
    expect_log="path work budget ran out",
)
def inline_svg_dense_clip() -> Page:
    clip = f"<clipPath id=k><path d='{dense_path()}' /></clipPath>"
    uses = "<rect width=100 height=100 clip-path='url(#k)' />" * 5
    return Page(doc(f"<svg width=100 height=100><defs>{clip}</defs>{uses}</svg>"))


@case(
    "inline SVG: a clip path of 40 paths of 3,000 overlapping curves, used by 300 elements "
    "(12,000 fills, and 12,000 point tests per hit test): the cost of counting the segments is "
    "charged also for fills that do not fit (ADR 0023)",
    expect_log="path work budget ran out",
)
def inline_svg_dense_clip_references() -> Page:
    d = dense_path(3_000)
    clip = "<clipPath id=k>" + f"<path d='{d}' />" * 40 + "</clipPath>"
    uses = "<rect width=100 height=100 clip-path='url(#k)' />" * 300
    return Page(doc(f"<svg width=100 height=100><defs>{clip}</defs>{uses}</svg>"))


@case(
    "SVG image with a path of 40,000 overlapping curves: its estimate exceeds the render budget "
    "at any resolution (ADR 0011)",
    expect_log="too expensive",
)
def svg_dense_path() -> Page:
    svg = f"<svg {SVG_NS} width='100' height='100'><path d='{dense_path()}' fill='red'/></svg>"
    return img_page(svg, "width=100 height=100")


@case(
    "SVG image with 100 paths of 400 overlapping curves each, shown at 1000 x 1000: the paths "
    "are cheap alone but add up, and the image is drawn at a lower resolution (about 33,000 "
    "pixels, 'in at most' in the debug log; 120,000 before the spans of ADR 0023 part 3; "
    "ADR 0011)"
)
def svg_many_dense_paths() -> Page:
    d = dense_path(400)
    paths = f"<path d='{d}' fill='red'/>" * 100
    svg = f"<svg {SVG_NS} width='100' height='100'>{paths}</svg>"
    return img_page(svg, "width=1000 height=1000")


def comb(teeth: int, width: float = 1200, height: float = 780, top: float = 0) -> str:
    """The `d` of a comb of `teeth` separate rectangles across `width`, each half
    as wide as its pitch (0.2 px for 3,000 teeth on 1,200 px): every row has
    `teeth` spans narrower than a pixel, which tiny-skia's anti-aliased fill takes
    about 2.6 us each to draw (6 s for 780 rows)."""
    pitch = width / teeth
    return "".join(
        f"M{i * pitch:.4f} {top}v{height}h{pitch / 2:.4f}v-{height}z" for i in range(teeth)
    )


@case(
    "inline SVG: a comb of 3,000 separate teeth 0.2 px wide in one path on 1,200 x 780 px: "
    "the spans narrower than a pixel count against the path work budget (6 s before; "
    "ADR 0023, part 3)",
    expect_log="path work budget ran out",
)
def inline_svg_separate_spans() -> Page:
    path = f"<path d='{comb(3_000)}' />"
    return Page(doc(f"<svg width=1200 height=780>{path}</svg>"))


@case(
    "inline SVG: ten stacked combs of 3,000 teeth, 78 px tall, in separate paths: each path "
    "is charged for its spans (6 s before; ADR 0023, part 3)",
    expect_log="path work budget ran out",
)
def inline_svg_separate_spans_stacked() -> Page:
    paths = "".join(f"<path d='{comb(3_000, height=78, top=78 * j)}' />" for j in range(10))
    return Page(doc(f"<svg width=1200 height=780>{paths}</svg>"))


@case(
    "inline SVG: the filled area under a noisy chart of 40,000 points on 1,200 x 780 px: its "
    "spikes are spans narrower than a pixel (3.3 s before; ADR 0023, part 3)",
    expect_log="path work budget ran out",
)
def inline_svg_separate_spans_area() -> Page:
    state = 7
    points = []
    for i in range(40_001):
        state = (state * 1_103_515_245 + 12_345) % 2**31
        points.append(f"L{i * 0.03:.2f} {100 + state % 600}")
    path = f"<path d='M0 780{''.join(points)}L1200 780z' />"
    return Page(doc(f"<svg width=1200 height=780>{path}</svg>"))


@case(
    "inline SVG: a clip path of a comb of 3,000 teeth used by five rectangles: the coverage "
    "fill counts the spans (ADR 0023, part 3)",
    expect_log="path work budget ran out",
)
def inline_svg_separate_spans_clip() -> Page:
    clip = f"<clipPath id=k><path d='{comb(3_000)}' /></clipPath>"
    uses = "<rect width=1200 height=780 clip-path='url(#k)' />" * 5
    return Page(doc(f"<svg width=1200 height=780><defs>{clip}</defs>{uses}</svg>"))


@case(
    "SVG image of a comb of 3,000 teeth, shown at 1,200 x 780 (6 s before): the spans are "
    "counted at 17 scales and the image is drawn at about 48,000 pixels ('in at most' in the "
    "debug log; ADR 0011, ADR 0023 part 3)"
)
def svg_separate_spans() -> Page:
    svg = f"<svg {SVG_NS} width='1200' height='780'><path d='{comb(3_000)}'/></svg>"
    return img_page(svg, "width=1200 height=780")


@case(
    "inline SVG: a line 780 px wide stroked with a dash pattern of 0.2 px dashes and 0.2 px "
    "gaps: the dashes are 3,000 separate spans narrower than a pixel in every row, charged "
    "from the outline of the stroke (6 s before; ADR 0023, part 3)",
    expect_log="path work budget ran out",
)
def inline_svg_dashed_thin() -> Page:
    path = "<path d='M0 390H1200' stroke=black stroke-width=780 stroke-dasharray='0.2 0.2'/>"
    return Page(doc(f"<svg width=1200 height=780>{path}</svg>"))


@case(
    "inline SVG: 15 lines of 50 px width with dashes of 0.2 px and gaps of 0.2 px, each its own "
    "path: every path is charged for the spans of its dashes (5.8 s before; ADR 0023, part 3)",
    expect_log="path work budget ran out",
)
def inline_svg_dashed_lines() -> Page:
    paths = "".join(
        f"<path d='M0 {26 * i + 25}H1200' stroke=black stroke-width=50 stroke-dasharray='0.2 0.2'/>"
        for i in range(15)
    )
    return Page(doc(f"<svg width=1200 height=780>{paths}</svg>"))


@case(
    "SVG image of a line 780 px wide with dashes and gaps of 0.2 px, shown at 1,200 x 780 "
    "(6 s before): the dashes are counted as spans and the image is drawn at a lower "
    "resolution (ADR 0023, part 3)"
)
def svg_dashed_thin() -> Page:
    path = "<path d='M0 390H1200' stroke=black stroke-width=780 stroke-dasharray='0.2 0.2'/>"
    return img_page(
        f"<svg {SVG_NS} width='1200' height='780'>{path}</svg>", "width=1200 height=780"
    )


def random_walk(segments: int, width: float = 1200) -> str:
    """The `d` of a closed polyline of `segments` segments from left to right across
    `width`, with a noisy height between 397 and 403."""
    state = 5
    step = width / segments
    points = []
    for i in range(segments):
        state = (state * 1_103_515_245 + 12_345) % 2**31
        points.append(f"L{i * step:.3f} {397 + (state >> 8) % 7}")
    return "M0 400" + "".join(points) + "z"


@case(
    "30 different SVG images, each a filled random walk of 250,000 segments: counting the "
    "spans of the paths has a budget per document (7.8 s of decoding before; ADR 0023, part 3)",
    time_limit_s=4.0,
)
def svg_many_counted_images() -> Page:
    svg = f"<svg {SVG_NS} width='1200' height='780'><path d='{random_walk(250_000)}'/></svg>"
    images = "".join(f"<img src='walk.svg?{i}' width=40 height=26>" for i in range(30))
    return Page(doc(images), {"walk.svg": svg})


def stripes_png(width: int, height: int) -> bytes:
    """An RGB PNG of `width` x `height` with 8-pixel stripes (small as a file, `width * height`
    pixels once decoded)."""
    import struct
    import zlib

    def chunk(kind: bytes, data: bytes) -> bytes:
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    stripe = [b"\xff\x00\x00" if (x // 8) % 2 else b"\x00\x00\xff" for x in range(width)]
    row = b"\x00" + b"".join(stripe)
    packer = zlib.compressobj(6)
    packed = b"".join(packer.compress(row) for _ in range(height)) + packer.flush()
    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    ihdr, idat, iend = chunk(b"IHDR", header), chunk(b"IDAT", packed), chunk(b"IEND", b"")
    return b"\x89PNG\r\n\x1a\n" + ihdr + idat + iend


@case(
    "2,000 images of one 4000 x 3000 PNG drawn at 20 x 20: the reduced levels are made once "
    "(ADR 0024)"
)
def images_many_reduced() -> Page:
    body = "<img src=big.png width=20 height=20>" * 2_000
    return Page(doc(body), {"big.png": stripes_png(4000, 3000)})


@case(
    "two 8000 x 8000 PNGs drawn at 20 x 20: the second reduction is past the work budget of "
    "the frame (ADR 0024)",
    expect_log="work budget for reduced copies",
)
def images_reduce_budget() -> Page:
    png = stripes_png(8000, 8000)
    body = "<img src=a.png width=20 height=20><img src=b.png width=20 height=20>"
    return Page(doc(body), {"a.png": png, "b.png": png})


@case(
    "150 nested boxes with overflow: hidden and border-radius around a 3,000 px block: the "
    "layers run out and the clips fall back to rectangles (ADR 0024)",
    expect_log="layer memory budget ran out",
    swb_args=("--full-page",),
)
def rounded_clips_nested() -> Page:
    css = ".c{overflow:hidden;border-radius:12px}"
    inner = "<div style='height:3000px;background:#0a0'></div>"
    return Page(doc(nest("<div class=c>", "</div>", 150, inner), css))


@case("2,000 cards of 300 x 300 with overflow: hidden and border-radius, nested 3 deep (ADR 0024)")
def rounded_cards() -> Page:
    css = ".c{width:300px;height:300px;overflow:hidden;border-radius:12px;display:inline-block}"
    card = nest("<div class=c>", "</div>", 3, "<div style='height:300px;background:#f00'>x</div>")
    return Page(doc(card * 2_000, css))


@case(
    "400 nested open details elements: each adds the box of its ::details-content to the box "
    "depth limit (ADR 0015)",
    expect_log="boxes nested deeper",
)
def details_nested_open() -> Page:
    return Page(doc(nest("<details open><summary>s</summary>", "</details>", 400, "text")))


@case("400 nested closed details elements (laid out, hidden contents inside hidden contents)")
def details_nested_closed() -> Page:
    return Page(doc(nest("<details><summary>s</summary>", "</details>", 400, "text")))


@case(
    "20 closed details elements with 1,500 list items and links each, and a nested closed "
    "details in each item (60,000 laid out boxes that are not drawn)"
)
def details_much_hidden_content() -> Page:
    item = "<li><a href='#x'>link text that is a little longer</a>"
    items = (item + "<details><summary>n</summary>sub</details></li>") * 1_500
    return Page(doc(f"<details><summary>menu</summary><ul>{items}</ul></details>" * 20))


@case(
    "3,000 canvas elements with huge, zero, invalid and out-of-range width and height "
    "attributes in blocks, flex and grid containers, a table and a percentage context, with "
    "JavaScript on: lengths are clamped, nothing allocates a bitmap (ADR 0028 section 15)",
    swb_args=("--js",),
)
def canvas_huge_sizes_js_on() -> Page:
    sizes = [
        "width=2147483647 height=2147483647",
        "width=99999999999999999999 height=1",
        "width=0 height=0",
        "width=-1 height=-1",
        "width=1e9 height=abc",
        "width=4294967296 height=2147483648",
        "width=16384 height=16384 style='width:50%'",
        "width=100000 height=100000 style='height:100%;max-width:50vw'",
    ]
    canvases = "".join(
        f"<canvas {sizes[i % len(sizes)]}>fallback <b>text</b></canvas>" for i in range(1000)
    )
    flex = f"<div style='display:flex;flex-wrap:wrap'>{canvases}</div>"
    grid = f"<div style='display:grid;grid-template-columns:repeat(4,1fr)'>{canvases}</div>"
    block = f"<div style='width:300px'>{canvases}</div>"
    table = f"<table><tr><td>{canvases[: len(canvases) // 4]}</td></tr></table>"
    return Page(doc(flex + grid + block + table, "canvas{display:block;background:#c00}"))


@case(
    "50,000 noscript elements with markup text in body and head, with JavaScript on: the "
    "content is raw text, nothing is loaded (ADR 0028 section 15)",
    time_limit_s=5,
    swb_args=("--js",),
)
def noscript_flood_js_on() -> Page:
    item = "<noscript><link rel=stylesheet href=a.css><img src=b.png><p>text &amp; more</p>"
    item += "</noscript>"
    return Page(f"<!doctype html><head>{item * 100}</head><body>{item * 50_000}")


@case(
    "20,000 style elements, each after a script, with JavaScript on: the parse pauses at "
    "each script and the engine drains the change journal after each step; a drain does not "
    "scan or sort the whole list of sheets (ADR 0028 sections 3 and 9)",
    swb_args=("--js",),
)
def parse_steps_many_sheets_js_on() -> Page:
    items = "".join(
        f"<style>#p{i}{{color:#c00}}</style><script>x=1</script><p id=p{i}>t</p>"
        for i in range(20_000)
    )
    return Page(doc(items))


@case(
    "50,000 images and 1,000 scripts inside 400 nested divs, with JavaScript on: each parser "
    "step starts the loads of the images that it inserted; the drain does not walk the "
    "ancestors of each change (ADR 0028 section 9)",
    swb_args=("--js",),
)
def parse_steps_deep_images_js_on() -> Page:
    items = ("<img src=a.svg>x" * 50 + "<script></script>") * 1_000
    svg = "<svg xmlns='http://www.w3.org/2000/svg' width='4' height='4'/>"
    return Page(doc(nest("<div>", "</div>", 400, items)), {"a.svg": svg})


@case(
    "20,000 images with unique URLs, each followed by a script, with JavaScript on: each "
    "parser step selects only its own image; the work of a step does not grow with the "
    "number of images (ADR 0028 section 9)",
    swb_args=("--js",),
)
def parse_steps_unique_images_js_on() -> Page:
    items = "".join(f"<img src=i{i}.png><script></script>" for i in range(20_000))
    return Page(doc(items))


@case(
    "1,100,000 comments with a script after each 1,000, with JavaScript on: past the node "
    "limit while the parser runs in steps and the journal records each node (ADR 0028 "
    "section 16)",
    expect_log="HTML parser stopped",
    swb_args=("--js",),
)
def dom_node_limit_js_on() -> Page:
    return Page(doc(("<!---->" * 1_000 + "<script></script>") * 1_100))


@case(
    "6 inline scripts with endless loops, with JavaScript on: each task ends at the time "
    "limit of 5 s, and after the third the document runs no more scripts (ADR 0028 "
    "sections 7 and 16)",
    time_limit_s=25,
    expect_log="the scripts of the document stop",
    swb_args=("--js",),
)
def inline_script_endless_loops_js_on() -> Page:
    return Page(doc("<p>text</p>" + "<script>for (;;) {}</script>" * 6))


@case(
    "50,000 paragraphs whose wrappers, child lists and expandos a script creates, with "
    "JavaScript on: one wrapper per node in the wrapper vector, the lists in the side "
    "table, and each list walks its children once per DOM generation (ADR 0028 section 4)",
    swb_args=("--js",),
)
def script_wrappers_of_many_nodes_js_on() -> Page:
    script = (
        "<script>let n = 0; for (let e = document.body.firstChild; e; e = e.nextSibling) "
        "{ e.mark = n++; const c = e.childNodes; for (let i = 0; i < c.length; i++) c[i].x = i; } "
        "const all = document.body.childNodes; let sum = 0; "
        "for (let i = 0; i < all.length; i++) sum += all[i].mark | 0; console.log(sum);</script>"
    )
    return Page(doc("<p>a<b>b</b></p>" * 50_000 + script))


@case(
    "50,000 elements with ids and a script that changes the DOM and reads a missing global "
    "1,000 times, with JavaScript on: each read after a change looks up the window's named "
    "properties again; the task ends at the time limit at the latest (ADR 0028 section 4)",
    time_limit_s=12,
    swb_args=("--js",),
)
def window_named_lookups_after_changes_js_on() -> Page:
    script = (
        "<script>for (let i = 0; i < 1000; i++) { document.title = 't' + i; "
        "if (typeof missingGlobal !== 'undefined') break; }</script>"
    )
    return Page(doc("".join(f"<i id=e{i}></i>" for i in range(50_000)) + script))


@case(
    "A listener that dispatches a new event at its own target, with JavaScript on: each "
    "nested dispatchEvent charges the shared recursion budget, the innermost one ends with "
    "a RangeError that is reported, and the outer dispatches return (ADR 0028 section 16)",
    expect_log="Uncaught RangeError: Maximum call stack size exceeded",
    swb_args=("--js",),
)
def nested_dispatch_event_recursion_js_on() -> Page:
    script = (
        "<script>let depth = 0; const t = document.body; "
        "t.addEventListener('r', () => { depth++; t.dispatchEvent(new Event('r')); }); "
        "t.dispatchEvent(new Event('r')); console.log('depth ' + depth);</script>"
    )
    return Page(doc("<p>text</p>" + script))


@case(
    "An event listener with an endless loop, with JavaScript on: the task that dispatches "
    "the event ends at the time limit of 5 s; the next listener does not run (ADR 0028 "
    "section 16)",
    time_limit_s=12,
    expect_log="the time limit was exceeded",
    swb_args=("--js",),
)
def event_listener_endless_loop_js_on() -> Page:
    script = (
        "<script>window.addEventListener('spin', () => { for (;;) {} }); "
        "window.addEventListener('spin', () => console.log('not reached')); "
        "window.dispatchEvent(new Event('spin'));</script>"
    )
    return Page(doc("<p>text</p>" + script))


@case(
    "200,000 listeners of one type and 200,000 of different types on one element, with "
    "JavaScript on: adding, the duplicate check, one dispatch and the removal of half stay "
    "linear (DOM §2.7)",
    swb_args=("--js",),
)
def many_event_listeners_on_one_target_js_on() -> Page:
    script = (
        "<script>let n = 0; const t = document.body; const fs = []; "
        "for (let i = 0; i < 200000; i++) { const f = () => n++; fs.push(f); "
        "t.addEventListener('x', f); t.addEventListener('x', f); } "
        "const g = () => n++; for (let i = 0; i < 200000; i++) t.addEventListener('t' + i, g); "
        "t.dispatchEvent(new Event('x')); "
        "for (let i = 0; i < 200000; i += 2) t.removeEventListener('x', fs[i]); "
        "t.dispatchEvent(new Event('x')); t.dispatchEvent(new Event('t7')); "
        "console.log('calls ' + n);</script>"
    )
    return Page(doc("<p>text</p>" + script))


@case(
    "More than 262,144 event listeners on the nodes of a document, with JavaScript on: "
    "the lists live outside the JavaScript heap, so the add past the limit ends the task "
    "(ADR 0028 section 16)",
    time_limit_s=10,
    expect_log="more than 262144 event listeners",
    swb_args=("--js",),
)
def event_listener_limit_js_on() -> Page:
    script = (
        "<script>const f = () => 0; const t = document.body; "
        "for (let i = 0; i < 300000; i++) t.addEventListener('t' + i, f); "
        "console.log('not reached');</script>"
    )
    return Page(doc("<p>text</p>" + script))


@case(
    "Four timers with endless loops, with JavaScript on and 16 s of settle time: each timer "
    "task ends at the time limit of 5 s, and after the third the document runs no more "
    "scripts (ADR 0028 sections 7 and 16)",
    time_limit_s=25,
    expect_log="the scripts of the document stop",
    swb_args=("--js", "--settle-ms", "16000"),
)
def timer_endless_loops_js_on() -> Page:
    script = "<script>for (let i = 0; i < 4; i++) setTimeout(() => { for (;;) {} }, i);</script>"
    return Page(doc("<p>text</p>" + script))


@case(
    "An endless chain of microtasks (queueMicrotask and promise reactions), with JavaScript "
    "on: the checkpoint of the script's task runs under its time limit of 5 s and ends "
    "there (ADR 0028 section 7)",
    time_limit_s=12,
    expect_log="the time limit was exceeded",
    swb_args=("--js",),
)
def microtask_endless_chain_js_on() -> Page:
    script = (
        "<script>(function f() { queueMicrotask(f); })(); "
        "Promise.resolve().then(function g() { Promise.resolve().then(g); });</script>"
    )
    return Page(doc("<p>text</p>" + script))


@case(
    "A promise chain where each reaction returns the next promise, with JavaScript on: the "
    "chain of resolutions grows until the heap limit of 256 MiB, the scripts stop, and the "
    "engine drops the runtime; the page stays (ADR 0028 sections 7 and 16)",
    time_limit_s=10,
    expect_log="reached the heap limit and is dropped",
    swb_args=("--js",),
)
def promise_chain_heap_limit_js_on() -> Page:
    script = (
        "<script>Promise.resolve().then(function g() { return Promise.resolve().then(g); });"
        "</script>"
    )
    return Page(doc("<p>text</p>" + script))


@case(
    "setInterval(f, 0) and a chain of setTimeout(f, 0), with JavaScript on and 2 s of settle "
    "time: the nesting clamp gives 4 ms after level 5, each turn of the event loop runs at "
    "most 16 ms of tasks, and swb ends after the settle time (HTML §8.7)",
    time_limit_s=6,
    swb_args=("--js", "--settle-ms", "2000"),
)
def interval_zero_and_timer_chain_js_on() -> Page:
    script = (
        "<script>let n = 0, c = 0; setInterval(() => { n++; document.title = 'n' + n; }, 0); "
        "(function f() { c++; setTimeout(f, 0); })();</script>"
    )
    return Page(doc("<p>text</p>" + script))


@case(
    "A script that schedules timers without end, with JavaScript on: the timers live outside "
    "the JavaScript heap, so the one past 1,048,576 timers and extra arguments ends the task "
    "(ADR 0028 section 16)",
    time_limit_s=10,
    expect_log="the timers of the document reached the limit",
    swb_args=("--js",),
)
def timer_limit_js_on() -> Page:
    script = "<script>for (;;) setTimeout(() => {}, 1e9, 1, 2, 3);</script>"
    return Page(doc("<p>text</p>" + script))


@case(
    "A script that schedules timers with a string handler of 20 MB without end, with "
    "JavaScript on: each timer keeps a copy of the code outside the JavaScript heap, so the "
    "one past 32 MiB of handler code ends the task (ADR 0028 section 16)",
    time_limit_s=10,
    expect_log="the string handlers of the timers of the document reached the limit",
    swb_args=("--js",),
)
def timer_string_handlers_js_on() -> Page:
    script = "<script>const s = 'x'.repeat(20e6); for (;;) setTimeout(s, 1e9);</script>"
    return Page(doc("<p>text</p>" + script))


@case(
    "200,000 timers of 0 ms that each schedule one more and 200,000 cleared timers, with "
    "JavaScript on and 1 s of settle time: the queue orders them by due time in O(log n), "
    "and cleared timers leave no task behind (HTML §8.7)",
    time_limit_s=8,
    swb_args=("--js", "--settle-ms", "1000"),
)
def many_timers_js_on() -> Page:
    script = (
        "<script>let n = 0; for (let i = 0; i < 200000; i++) setTimeout(() => { n++; "
        "setTimeout(() => n++, 1); }, 0); "
        "for (let i = 0; i < 200000; i++) clearTimeout(setTimeout(() => {}, 10));</script>"
    )
    return Page(doc("<p>text</p>" + script))


@case(
    "Inline scripts that each run for 4.9 s, just under the time limit, with JavaScript on: "
    "the long tasks of a document may run for 30 s in total, then the document runs no more "
    "scripts (ADR 0028 section 16)",
    time_limit_s=40,
    expect_log="its long tasks ran for",
    swb_args=("--js",),
)
def scripts_under_the_time_limit_js_on() -> Page:
    script = "<script>{ const t = Date.now(); while (Date.now() - t < 4900) {} }</script>"
    return Page(doc("<p>text</p>" + script * 1000))


@case(
    "A script that writes console.error lines in an endless loop, with JavaScript on: the "
    "log gets the first 1,000 lines of the document, the next ones go to the debug level, "
    "and the task ends at the time limit of 5 s (ADR 0028 section 13)",
    time_limit_s=12,
    expect_log="the next ones go to the debug log",
    swb_args=("--js",),
)
def console_error_loop_js_on() -> Page:
    script = "<script>for (let i = 0; ; i++) console.error('line ' + i);</script>"
    return Page(doc("<p>text</p>" + script))


@case(
    "Inline scripts that each insert the next inline script without end, with JavaScript on: "
    "an inserted inline script runs inside the insertion, so the chain nests; the shared "
    "recursion budget ends it with a RangeError (ADR 0028 sections 8 and 16)",
    expect_log="Maximum call stack size exceeded",
    swb_args=("--js",),
)
def script_chain_inline_js_on() -> Page:
    script = (
        "<script>function next(n) { const s = document.createElement('script'); "
        "s.text = 'next(' + (n + 1) + ')'; document.body.appendChild(s); } next(0);</script>"
    )
    return Page(doc("<p>text</p>" + script + "<p>after</p>"))


@case(
    "An external script that inserts itself again 3,000 times, with JavaScript on: each "
    "inserted script delays the load event until it ran, so the load waits for the end of "
    "the chain, one fetch after the other (ADR 0028 section 8)",
    swb_args=("--js",),
)
def script_chain_external_js_on() -> Page:
    chain = (
        "window.n = (window.n || 0) + 1; if (window.n < 3000) { "
        "const s = document.createElement('script'); s.src = 'chain.js'; "
        "document.head.appendChild(s); }"
    )
    return Page(doc("<p>text</p><script src=chain.js></script><p>after</p>"), {"chain.js": chain})


@case(
    "External scripts that each insert two more until 20,000 ran, with JavaScript on: at most "
    "64 script fetches run at once and the rest wait in a queue (ADR 0028 section 16); the "
    "load waits for all of them",
    swb_args=("--js",),
)
def script_fanout_js_on() -> Page:
    fanout = (
        "window.n = (window.n || 0) + 1; if (window.n < 10000) "
        "for (let i = 0; i < 2; i++) { const s = document.createElement('script'); "
        "s.src = 'fanout.js'; document.head.appendChild(s); }"
    )
    return Page(doc("<p>text</p><script src=fanout.js></script>"), {"fanout.js": fanout})


@case(
    "60 deferred scripts that share one file of 6 MiB and a last blocking script, with "
    "JavaScript on: the deferred scripts arrive while the parser waits, so their text is held; "
    "the text of fetched scripts that has not run is limited to 256 MiB per document, and the "
    "scripts beyond it fail with an error event (ADR 0028 section 16)",
    expect_log="script text that has not run",
    rss_limit_mib=900,
    swb_args=("--js",),
)
def script_text_held_js_on() -> Page:
    big = "/*" + "x" * (6 * MIB) + "*/"
    blocker = "/*" + "y" * (10 * MIB) + "*/"
    scripts = "<script defer src=big.js></script>" * 60 + "<script src=blocker.js></script>"
    return Page(
        doc("<p>text</p>" + scripts + "<p>after</p>"), {"big.js": big, "blocker.js": blocker}
    )


# --- DOM changes from scripts (feature 6a) ----------------------------------


@case(
    "A script that appends comments with appendChild until the node limit of 1,048,576, with "
    "JavaScript on: the task ends at the limit (ADR 0028 section 16)",
    expect_log="the DOM node limit",
    time_limit_s=10,
    swb_args=("--js",),
)
def dom_growth_append_child_js_on() -> Page:
    script = "<script>for (;;) document.body.appendChild(document.createComment(''));</script>"
    return Page(doc("<p>text</p>" + script + "<p>after</p>"))


@case(
    "A script that builds a chain of 100,000 nested divs with appendChild, with JavaScript on: "
    "style, layout (box depth limit 256) and paint run on a tree far deeper than the "
    "parser's limit of 512 (ADR 0028 section 16)",
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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


def _steps(steps: list[dict[str, object]]) -> tuple[str, ...]:
    """The swb arguments of a case that sends input after the load."""
    return ("--js", f"--steps={json.dumps(steps)}")


@case(
    "A mousemove listener that rebuilds a 3,000-element list and queries it on each of 200 "
    "pointer moves, with JavaScript on: each event is a task of its own under the time limit "
    "(feature 6d2)",
    swb_args=_steps([{"hover": [10 + 3 * i, 20 + 2 * i]} for i in range(200)]),
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
    swb_args=_steps([{"focus": "#i0"}, {"type": "x" * 300}]),
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
    swb_args=_steps([{"click": "button"}] * 90),
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
    swb_args=_steps([{"click": "a"}]),
    time_limit_s=12,
    rss_limit_mib=1024,
)
def javascript_link_loop_js_on() -> Page:
    return Page(doc("<a href='javascript:while(1){}'>loop</a>"))


@case(
    "wheel listeners on 10,000 elements and on the document, body and window (passive by "
    "default there), 100 wheel turns over the elements, with JavaScript on: the dispatch "
    "visits the listeners of the path only (feature 6d2)",
    swb_args=_steps([{"hover": [20, 20]}] + [{"wheel": [0, 40]}] * 100),
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
    swb_args=_steps([{"hover": [10 + 3 * i, 20 + 2 * i]} for i in range(200)]),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    return Page(doc(spans + script, rules), {"f.ttf": _web_font()})


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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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
    swb_args=("--js",),
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


REPLAY_URL = "http://hostile.test/index.html"


@case(
    "localStorage and sessionStorage written in loops past their quota of 5,242,880 code "
    "units, the errors caught: 1 MiB values, 100-unit values under distinct keys, 200,000 "
    "one-unit keys (the limit of 100,000 keys), then reads, removals and clear, with "
    "JavaScript on (feature 8b)",
    swb_args=("--js",),
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
    swb_args=("--js", "--replay", "{dir}"),
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
    swb_args=("--js", "--replay", "{dir}"),
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


# --- Observers (feature 9a) --------------------------------------------------


@case(
    "4 inline scripts that each register a mutation observer whose callback changes an "
    "attribute again, and change it once: the callbacks run in one microtask checkpoint "
    "without end, each checkpoint ends at the time limit of 5 s (two terminations are "
    "logged for each), and after the second such task, 10 s in all, the document runs no "
    "more scripts (ADR 0028 sections 7 and 13)",
    time_limit_s=25,
    expect_log="the scripts of the document stop",
    swb_args=("--js",),
)
def mutation_observer_endless_callback_js_on() -> Page:
    script = (
        "<script>var n = 0; new MutationObserver(() => { document.body.setAttribute("
        "'data-n', ++n); }).observe(document.body, {attributes: true}); "
        "document.body.setAttribute('data-n', 0);</script>"
    )
    return Page(doc("<p>text</p>" + script * 4))


@case(
    "a loop of 3,000,000 attribute changes with a mutation observer that records the old "
    "values and no yield: at most 131,072 records wait, the rest are dropped with a "
    "warning, the callback gets the kept ones, and the scripts of the document still run "
    "afterwards (a timer logs `flood ok`; ADR 0028 section 16)",
    time_limit_s=20,
    expect_log="flood ok",
    swb_args=("--js", "--settle-ms", "500"),
)
def mutation_records_flood_js_on() -> Page:
    script = (
        "<script>const p = document.querySelector('p'); let seen = 0, chars = 0; "
        "new MutationObserver((r) => { seen += r.length; "
        "for (const x of r) chars += x.oldValue === null ? 1 : x.oldValue.length; })"
        ".observe(p, {attributes: true, "
        "attributeOldValue: true}); "
        "for (let i = 0; i < 3000000; i++) p.setAttribute('title', 'v'); "
        "setTimeout(() => console.warn((seen >= 100000 && seen < 3000000 && chars >= seen "
        "? 'flood ok ' : 'flood wrong ') + seen), 10);</script>"
    )
    return Page(doc("<p>text</p>" + script))


@case(
    "a parse of 300,000 elements with a mutation observer on the document (one record "
    "each): at most 131,072 records wait, the rest are dropped with a warning, the callback "
    "gets the kept ones, and the scripts of the document still run afterwards (a timer logs "
    "`parse ok`; ADR 0028 section 16)",
    time_limit_s=20,
    expect_log="parse ok",
    swb_args=("--js", "--settle-ms", "500"),
)
def mutation_records_parse_js_on() -> Page:
    script = (
        "<script>let seen = 0; new MutationObserver((r) => { seen += r.length; })"
        ".observe(document, {childList: true, subtree: true}); "
        "setTimeout(() => console.warn((seen >= 100000 ? 'parse ok ' : 'parse wrong ') + seen),"
        " 10);</script>"
    )
    return Page(doc(script + "<i></i>" * 300000))


@case(
    "200,000 children removed one by one under 1 subtree observer, and 3,000 subtree "
    "observers on one node with 3,000 children removed one by one: the transient "
    "registrations are a set and bounded at 500,000, the work of the sink is charged to the "
    "time limit of the task, and the scripts of the document still run afterwards (a timer "
    "logs `transients ok`; ADR 0028 section 16)",
    time_limit_s=25,
    expect_log="transients ok",
    swb_args=("--js", "--settle-ms", "500"),
)
def mutation_transients_js_on() -> Page:
    script = (
        "<script>const a = document.createElement('div'); document.body.appendChild(a); "
        "for (let i = 0; i < 200000; i++) a.appendChild(document.createElement('i')); "
        "new MutationObserver(() => {}).observe(a, {childList: true, subtree: true}); "
        "while (a.firstChild) a.removeChild(a.firstChild);</script>"
        "<script>const b = document.createElement('div'); document.body.appendChild(b); "
        "for (let i = 0; i < 3000; i++) b.appendChild(document.createElement('i')); "
        "for (let i = 0; i < 3000; i++) "
        "new MutationObserver(() => {}).observe(b, {childList: true, subtree: true}); "
        "while (b.firstChild) b.removeChild(b.firstChild);</script>"
        "<script>setTimeout(() => console.warn('transients ok'), 10);</script>"
    )
    return Page(doc(script))


@case(
    "a resize observer whose callback resizes its target without end: one callback and one "
    "loop error per frame, the page stays responsive (ADR 0028 sections 7 and 13)",
    time_limit_s=15,
    swb_args=("--js", "--settle-ms", "3000"),
)
def resize_observer_endless_callback_js_on() -> Page:
    script = (
        "<script>const a = document.querySelector('div'); let n = 0; "
        "window.addEventListener('error', (e) => e.preventDefault()); "
        "new ResizeObserver(() => { n++; a.style.width = (a.offsetWidth + 1) + 'px'; "
        "if (n % 50 === 0) console.log('callbacks ' + n); }).observe(a);</script>"
    )
    return Page(doc("<div style='width:100px;height:20px'></div>" + script))


@case(
    "100,000 targets of one intersection observer, with a scroll after the first frame: "
    "the update of a frame computes all targets within the time limit of a task (feature "
    "9a)",
    time_limit_s=20,
    swb_args=("--js", "--settle-ms", "3000"),
)
def intersection_observer_100000_targets_js_on() -> Page:
    script = (
        "<script>let hits = 0; const o = new IntersectionObserver((es) => { hits += "
        "es.length; }); const f = document.createDocumentFragment(); "
        "for (let i = 0; i < 100000; i++) { const d = document.createElement('div'); "
        "f.appendChild(d); } document.body.appendChild(f); "
        "for (const d of document.body.children) o.observe(d); "
        "setTimeout(() => window.scrollTo(0, 100000), 500); "
        "setTimeout(() => console.log('hits ' + hits), 2500);</script>"
    )
    return Page(doc(script, css="div{height:2px}"))


@case(
    "A text area with a value of 20 MiB, and a script that sets its selection and reads it "
    "back 100,000 times with offsets of 4e9, with JavaScript on: each call scans the value "
    "once and the task ends at its time limit (feature 9b)",
    time_limit_s=20,
    swb_args=("--js",),
)
def selection_huge_value_js_on() -> Page:
    script = (
        "<script>const t = document.getElementById('t'); t.value = 'x'.repeat(20 * 1024 * 1024); "
        "let n = 0; for (let i = 0; i < 100000; i++) { t.setSelectionRange(i, 4e9, 'backward'); "
        "n += t.selectionEnd - t.selectionStart; } console.log('done ' + n);</script>"
    )
    return Page(doc('<textarea id=t style="display:none"></textarea>' + script))


@case(
    "A script that appends one character to the value of an input 300,000 times with "
    "setRangeText, with JavaScript on: each call copies the value, and the task ends at its "
    "time limit (feature 9b)",
    time_limit_s=20,
    swb_args=("--js",),
)
def set_range_text_growing_value_js_on() -> Page:
    script = (
        "<script>const t = document.getElementById('t'); for (let i = 0; i < 300000; i++) { "
        "t.setRangeText('y', t.value.length, t.value.length, 'end'); } "
        "console.log('done ' + t.value.length);</script>"
    )
    return Page(doc('<input id=t style="display:none">' + script))


@case(
    "A script that writes 4 times 16 MiB into a text area with setRangeText, with JavaScript "
    "on: the budget of control values (32 MiB, ADR 0028 section 16) ends the task",
    expect_log="the values of form controls reached the limit",
    time_limit_s=20,
    swb_args=("--js",),
)
def set_range_text_over_budget_js_on() -> Page:
    script = (
        "<script>const t = document.getElementById('t'); const s = 'z'.repeat(16 * 1024 * 1024); "
        "for (let i = 0; i < 4; i++) { t.setRangeText(s, t.value.length, t.value.length, 'end'); } "
        "console.log('not reached');</script>"
    )
    return Page(doc('<textarea id=t style="display:none"></textarea>' + script))


@case(
    "A script that sets a custom validity message of 1 MiB on 10 inputs, with JavaScript on: "
    "the budget of the messages (8 MiB, ADR 0028 section 16) ends the task (feature 9b2)",
    expect_log="the custom validity messages reached the limit",
    time_limit_s=20,
    swb_args=("--js",),
)
def set_custom_validity_over_budget_js_on() -> Page:
    script = (
        "<script>const s = 'm'.repeat(1 << 20); for (let i = 0; i < 10; i++) { "
        "const e = document.createElement('input'); document.body.appendChild(e); "
        "e.setCustomValidity(s); } console.log('not reached');</script>"
    )
    return Page(doc(script))


@case(
    "3,000 inputs with the pattern (a+)+$ and a value of 3,000 letters a and a !, a rule for "
    ":invalid, with JavaScript off: every style update matches the patterns with the step "
    "limit of the pass (feature 9b2)",
    time_limit_s=20,
)
def pattern_catastrophic_many_inputs() -> Page:
    value = "a" * 3000 + "!"
    inputs = "".join(f"<input pattern='(a+)+$' value='{value}{i}'>" for i in range(3000))
    return Page(doc(inputs, css="input:invalid{outline:2px solid red}"))


@case(
    "3,000 inputs with the pattern (a+)+$ and values of 3,000 letters a, a rule for :invalid, "
    "and a script that calls checkValidity() on a form of them and reads :invalid 20 times, "
    "with JavaScript on: the step limit of each pass keeps it short (feature 9b2)",
    time_limit_s=20,
    swb_args=("--js",),
)
def pattern_catastrophic_check_validity_js_on() -> Page:
    value = "a" * 3000 + "!"
    inputs = "".join(f"<input pattern='(a+)+$' value='{value}{i}'>" for i in range(3000))
    script = (
        "<script>const f = document.forms[0]; let n = 0; for (let i = 0; i < 20; i++) { "
        "f.elements[i].value += 'x'; n += f.checkValidity() ? 0 : 1; "
        "n += document.querySelectorAll('input:invalid').length; } "
        "console.log('done ' + n);</script>"
    )
    css = "input:invalid{outline:2px solid red}"
    return Page(doc(f"<form>{inputs}</form>" + script, css=css))


@case(
    "A form with 5,000 required radio buttons of one group and 5,000 text inputs, and a script "
    "that calls checkValidity() on single controls 200,000 times and on the form 200 times, "
    "with JavaScript on: every call is bounded and the task ends at its time limit "
    "(feature 9b2)",
    time_limit_s=20,
    swb_args=("--js",),
)
def check_validity_loop_js_on() -> Page:
    controls = "<input type=radio name=g required>" * 5000 + "<input required>" * 5000
    script = (
        "<script>const f = document.forms[0]; const e = f.elements; let n = 0; "
        "f.addEventListener('invalid', () => { n++; }, true); "
        "for (let i = 0; i < 200000; i++) { n += e[i % 10000].checkValidity() ? 0 : 1; } "
        "for (let i = 0; i < 200; i++) { n += f.checkValidity() ? 0 : 1; } "
        "console.log('done ' + n);</script>"
    )
    return Page(doc(f"<form>{controls}</form>" + script))


@case(
    "A form of 20,000 inputs with a pattern, minlength, min, max and step, with JavaScript on, "
    "and a script that appends and removes an input and reads :invalid 200 times: each rebuild "
    "of the control states is linear (feature 9b2)",
    time_limit_s=30,
    swb_args=("--js",),
)
def validity_rebuild_loop_js_on() -> Page:
    inputs = "".join(
        f"<input type=number min=1 max=100 step=3 value={i % 150}>"
        f"<input pattern='[a-z]+' minlength=2 value=ab{i}>"
        for i in range(10000)
    )
    script = (
        "<script>const f = document.forms[0]; let n = 0; for (let i = 0; i < 200; i++) { "
        "const e = document.createElement('input'); e.required = true; f.appendChild(e); "
        "n += document.querySelectorAll('input:invalid').length; e.remove(); } "
        "console.log('done ' + n);</script>"
    )
    return Page(doc(f"<form>{inputs}</form>" + script, css="input:invalid{outline:1px solid red}"))


@case(
    "A script that sets a new pattern of 10 KB on an input and reads its validity, in a loop "
    "until the time limit, with JavaScript on: the cache of compiled patterns is bounded "
    "(feature 9b2 review; it grew to 1.6 GB before)",
    time_limit_s=20,
    swb_args=("--js",),
)
def pattern_cache_loop_js_on() -> Page:
    script = (
        "<script>const i = document.getElementById('i'); const pad = '[a-c]'.repeat(2000); "
        "let n = 0; for (; n < 2000000; n++) { i.setAttribute('pattern', pad + '|x' + n); "
        "i.validity.patternMismatch; } console.log('not reached');</script>"
    )
    return Page(doc("<input id=i value=abc>" + script))


@case(
    "300 inputs, each with a different pattern of 44 KB that is slow to compile "
    "([\\p{RGI_Emoji}--\\q{a}] 2,000 times), and a rule for :invalid, with JavaScript off: a "
    "style update compiles a bounded amount of pattern text (feature 9b2 review; one style "
    "update took 28 s before)",
    time_limit_s=20,
)
def pattern_compile_many_inputs() -> Page:
    pad = "[\\p{RGI_Emoji}--\\q{a}]" * 2000
    inputs = "".join(f'<input pattern="{pad}|x{i}" value=a>' for i in range(300))
    return Page(doc(inputs, css="input:invalid{outline:1px solid red}"))


# --- fetch and AbortSignal (feature 10a) -------------------------------------

_NO_CORS = "{mode: 'no-cors'}"


@case(
    "5,000 fetch() calls to a server that reads each request and never answers, with "
    "JavaScript on: 6 run at once (one host), the rest wait in the queue (ADR 0028 section "
    "16); the page loads and stays responsive, and the process ends at the time limit",
    server="never",
    swb_args=("--js",),
)
def fetch_pending_never_js_on() -> Page:
    script = (
        "<script>let p = 0; for (let i = 0; i < 5000; i++) "
        f"fetch('http://127.0.0.1:__PORT__/n' + i, {_NO_CORS}).then(() => p++, () => p++); "
        "console.log('started 5000');</script>"
    )
    return Page(doc("<p>fetch</p>" + script))


@case(
    "20,000 fetch() calls to a server that never answers, past the queue of 16,384 requests "
    "(ADR 0028 section 16): the calls past the queue and the 6 that run fail at once with "
    "TypeError, the others wait",
    server="never",
    expect_log="too many fetch() requests wait",
    swb_args=("--js",),
)
def fetch_queue_overflow_js_on() -> Page:
    script = (
        "<script>let failed = 0; for (let i = 0; i < 20000; i++) "
        f"fetch('http://127.0.0.1:__PORT__/q' + i, {_NO_CORS}).catch(() => failed++); "
        "Promise.resolve().then(() => console.log('failed at once ' + failed));</script>"
    )
    return Page(doc("<p>fetch</p>" + script))


@case(
    "3,000 fetch() calls to a server that answers after 2 s, with JavaScript on: the answers "
    "come in batches of 6 (one host) and the page does not wait for them",
    server="slow",
    swb_args=("--js",),
)
def fetch_slow_server_js_on() -> Page:
    script = (
        "<script>let ok = 0; for (let i = 0; i < 3000; i++) "
        "fetch('http://127.0.0.1:__PORT__/s' + i).then(() => ok++, () => {}); "
        "setTimeout(() => console.log('ok after 1 s: ' + ok), 1000);</script>"
    )
    return Page(doc("<p>fetch</p>" + script))


@case(
    "A loop of 100,000 fetch() calls to a server that never answers, each aborted at once, "
    "with JavaScript on: the aborted requests leave the queue, and the ones that run give "
    "up their place when the loader puts a new thread in the place of the blocked one (at "
    "most 16 such threads; then they keep it until they end); the loop runs in bounded time "
    "and memory",
    server="never",
    swb_args=("--js",),
    time_limit_s=10,
)
def fetch_abort_loop_js_on() -> Page:
    script = (
        "<script>let aborted = 0; for (let i = 0; i < 100000; i++) { "
        "const c = new AbortController(); "
        "fetch('http://127.0.0.1:__PORT__/a' + i, {mode: 'no-cors', signal: c.signal})"
        ".catch(() => aborted++); c.abort(); } "
        "setTimeout(() => console.log('aborted ' + aborted), 100);</script>"
    )
    return Page(doc("<p>fetch</p>" + script))


@case(
    "64 fetch() calls whose responses have 60 MiB each, with JavaScript on, and a busy loop "
    "of 3 s so that the completions pile up in the loader (review of 10a: 3.9 GB before): "
    "the document's budget of 256 MiB for the requests and the responses not yet in the "
    "script heap (ADR 0028 section 16) fails the calls that do not fit",
    server="big",
    expect_log="memory budget",
    swb_args=("--js",),
    time_limit_s=10,
    rss_limit_mib=1100,
)
def fetch_big_responses_busy_js_on() -> Page:
    script = (
        "<script>let ok = 0, failed = 0; const all = []; for (let i = 0; i < 64; i++) "
        "all.push(fetch('http://127.0.0.1:__PORT__/b' + i).then(r => r.arrayBuffer())"
        ".then(() => ok++, () => failed++)); "
        "const end = Date.now() + 3000; while (Date.now() < end) {} "
        "Promise.all(all).then(() => console.log('ok ' + ok + ' failed ' + failed));</script>"
    )
    return Page(doc("<p>fetch</p>" + script))


@case(
    "64 fetch() calls with a 64 MiB ArrayBuffer as the body (no-cors POST) to a server that "
    "never answers, with JavaScript on (review of 10a: 4.4 GB before): the request bodies "
    "count against the document's budget of 256 MiB, and the calls that do not fit fail at "
    "once with TypeError",
    server="never",
    expect_log="does not fit",
    swb_args=("--js",),
    time_limit_s=15,
    rss_limit_mib=1100,
)
def fetch_big_request_bodies_js_on() -> Page:
    script = (
        "<script>const body = new ArrayBuffer(64 << 20); let failed = 0; "
        "for (let i = 0; i < 64; i++) "
        "fetch('http://127.0.0.1:__PORT__/p' + i, "
        "{method: 'POST', mode: 'no-cors', body}).catch(() => failed++); "
        "Promise.resolve().then(() => console.log('failed at once ' + failed));</script>"
    )
    return Page(doc("<p>fetch</p>" + script))


@case(
    "AbortSignal.any() over 100,000 signals, and 100,000 any() signals of one source: a "
    "source keeps at most 65,536 dependents and the rest do not follow it (ADR 0028 section "
    "16); one abort reaches the tracked ones",
    expect_log="signals of AbortSignal.any()",
    swb_args=("--js",),
    time_limit_s=10,
)
def abort_signal_any_js_on() -> Page:
    script = (
        "<script>const many = []; for (let i = 0; i < 100000; i++) many.push(new "
        "AbortController().signal); const big = AbortSignal.any(many); "
        "const c = new AbortController(); const dependents = []; "
        "for (let i = 0; i < 100000; i++) dependents.push(AbortSignal.any([c.signal])); "
        "c.abort('x'); console.log('aborted ' + dependents.filter((s) => s.aborted).length "
        "+ ' ' + big.aborted);</script>"
    )
    return Page(doc("<p>signals</p>" + script))


@case(
    "A listener added 100,000 times with the signal option of one signal, then 100,000 "
    "AbortSignal.timeout() signals: a signal tracks at most 65,536 listeners and the timeout "
    "signals wait for their timers (ADR 0028 section 16)",
    swb_args=("--js",),
    time_limit_s=10,
)
def abort_signal_listeners_js_on() -> Page:
    script = (
        "<script>const c = new AbortController(); const t = new EventTarget(); "
        "for (let i = 0; i < 100000; i++) t.addEventListener('x', () => {}, {signal: c.signal}); "
        "c.abort(); const keep = []; "
        "for (let i = 0; i < 100000; i++) keep.push(AbortSignal.timeout(100000)); "
        "console.log('done ' + keep.length);</script>"
    )
    return Page(doc("<p>signals</p>" + script))


@case(
    "A loop that makes 100 Response objects of 2 MiB each, keeps them and reads 2 of them "
    "with text(), then reads a 6 MiB data: URL 20 times with fetch(), with JavaScript on: "
    "the bodies count in the heap of the document (ADR 0028 section 16; the heap limit ends "
    "a runaway loop)",
    swb_args=("--js",),
    time_limit_s=20,
    rss_limit_mib=1200,
)
def response_bodies_js_on() -> Page:
    script = (
        "<script>const keep = []; try { for (let i = 0; i < 100; i++) "
        "keep.push(new Response(new Uint8Array(2 << 20))); } catch (e) {} "
        "const url = 'data:text/plain;base64,' + btoa('x'.repeat(6 << 20)); "
        "(async () => { let total = 0; for (let i = 0; i < 20; i++) { "
        "const r = await fetch(url); total += (await r.text()).length; } "
        "console.log('read ' + total + ' held ' + keep.length); })();</script>"
    )
    return Page(doc("<p>bodies</p>" + script))


@case(
    "5,000 XMLHttpRequest calls to a server that never answers, with JavaScript on (feature "
    "10b): at most 64 run, the others wait in the queue of the document (ADR 0028 section 16), "
    "and the page does not wait for them",
    server="never",
    swb_args=("--js",),
)
def xhr_hung_loop_js_on() -> Page:
    script = (
        "<script>for (let i = 0; i < 5000; i++) { const x = new XMLHttpRequest(); "
        "x.open('GET', 'http://127.0.0.1:__PORT__/x' + i); x.send(); } "
        "console.log('started 5000');</script>"
    )
    return Page(doc("<p>xhr</p>" + script))


@case(
    "A loop of 100,000 XMLHttpRequest calls to a server that never answers, each aborted at "
    "once, with JavaScript on (feature 10b): the aborted requests leave the queue, and each "
    "abort fires its events; the loop runs in bounded time and memory",
    server="never",
    swb_args=("--js",),
    time_limit_s=15,
)
def xhr_abort_loop_js_on() -> Page:
    script = (
        "<script>let aborted = 0; for (let i = 0; i < 100000; i++) { "
        "const x = new XMLHttpRequest(); x.onabort = () => aborted++; "
        "x.open('GET', 'http://127.0.0.1:__PORT__/a' + i); x.send(); x.abort(); } "
        "setTimeout(() => console.log('aborted ' + aborted), 100);</script>"
    )
    return Page(doc("<p>xhr</p>" + script))


@case(
    "Blobs that are concatenated and sliced in a loop, with JavaScript on (feature 10b): "
    "40 doublings of one blob and 100,000 slices of slices; a blob shares its bytes, flattens "
    "its list of pieces past 1,024 and is at most 64 MiB (ADR 0028 section 16); the longer "
    "ones throw a RangeError",
    swb_args=("--js",),
    time_limit_s=15,
    rss_limit_mib=1200,
)
def blob_concat_slice_js_on() -> Page:
    script = (
        "<script>let b = new Blob(['x'.repeat(1000)]); let n = 0; try { "
        "for (let i = 0; i < 40; i++) { b = new Blob([b, b]); n++; } } catch (e) { "
        "console.log(e.name); } let s = b; for (let i = 0; i < 100000; i++) "
        "s = s.slice(1, s.size - 1); console.log('doublings ' + n + ' size ' + b.size);</script>"
    )
    return Page(doc("<p>blob</p>" + script))


@case(
    "URL.createObjectURL on 100,000 small blobs without revokeObjectURL, with JavaScript on "
    "(feature 10b): a document keeps at most 16,384 Blob URLs and 256 MiB of logical bytes "
    "(ADR 0028 section 16); the calls past the limit throw QuotaExceededError",
    expect_log="too many Blob URLs",
    swb_args=("--js",),
    time_limit_s=15,
)
def blob_urls_no_revoke_js_on() -> Page:
    script = (
        "<script>let ok = 0, failed = 0; for (let i = 0; i < 100000; i++) { try { "
        "URL.createObjectURL(new Blob(['x'])); ok++; } catch (e) { failed++; } } "
        "console.log('ok ' + ok + ' failed ' + failed);</script>"
    )
    return Page(doc("<p>blob urls</p>" + script))


@case(
    "A FormData with 300,000 entries sent with XMLHttpRequest to a server that never answers, "
    "20 times, with JavaScript on (feature 10b): the multipart body counts against the "
    "document's request budget of 256 MiB (ADR 0028 section 16); the sends that do not fit "
    "fail",
    server="never",
    swb_args=("--js",),
    time_limit_s=20,
    rss_limit_mib=1200,
)
def formdata_many_entries_xhr_js_on() -> Page:
    script = (
        "<script>const fd = new FormData(); for (let i = 0; i < 300000; i++) "
        "fd.append('name' + i, 'value' + i); let failed = 0; for (let i = 0; i < 20; i++) { "
        "const x = new XMLHttpRequest(); x.onerror = () => failed++; "
        "x.open('POST', 'http://127.0.0.1:__PORT__/f' + i); x.send(fd); } "
        "setTimeout(() => console.log('failed ' + failed), 300);</script>"
    )
    return Page(doc("<p>formdata</p>" + script))


@case(
    "FileReader.readAsDataURL on a blob of 48 MiB, 20 times in a row, with JavaScript on "
    "(feature 10b): each read makes a string of 64 MiB in the heap of the document (ADR 0028 "
    "section 16: the heap limit of 256 MiB ends a runaway page); the page stays alive",
    swb_args=("--js",),
    time_limit_s=30,
    rss_limit_mib=1400,
)
def filereader_data_url_large_js_on() -> Page:
    script = (
        "<script>const b = new Blob([new Uint8Array(48 << 20)]); let n = 0; const next = () => { "
        "if (n++ >= 20) return; const r = new FileReader(); r.onload = next; r.onerror = next; "
        "r.readAsDataURL(b); }; try { next(); } catch (e) { console.log(e.name); } "
        "setTimeout(() => console.log('reads ' + n), 2000);</script>"
    )
    return Page(doc("<p>reader</p>" + script))


@case(
    "A loop of 100,000 navigator.sendBeacon calls with 1 KiB each to a server that never "
    "answers, with JavaScript on (feature 10b): at most 64 KiB of beacon data is queued "
    "(Chromium's quota), the other calls return false",
    server="never",
    swb_args=("--js",),
)
def send_beacon_loop_js_on() -> Page:
    script = (
        "<script>let ok = 0; const data = new Uint8Array(1024); for (let i = 0; i < 100000; i++) "
        "if (navigator.sendBeacon('http://127.0.0.1:__PORT__/b' + i, data)) ok++; "
        "console.log('queued ' + ok);</script>"
    )
    return Page(doc("<p>beacon</p>" + script))


@case(
    "Images whose load handlers insert more images: a chain of 100,000 data: URL images, each "
    "created in the load handler of the one before, with JavaScript on (feature 10b): the "
    "events are delivered in slices of 4,096, the loop gives the page time between them, and "
    "the finished watches of images outside the document leave the table of 65,536 (ADR 0028 "
    "section 16; the review fix of 10b: the limit used to act here)",
    forbid_log="images are watched for load events",
    swb_args=("--js",),
    time_limit_s=30,
)
def image_load_chain_js_on() -> Page:
    svg = (
        "data:image/svg+xml,%3Csvg xmlns=%27http://www.w3.org/2000/svg%27 "
        "width=%271%27 height=%271%27/%3E"
    )
    script = (
        "<script>let n = 0; const make = () => { const i = new Image(); "
        f"i.onload = i.onerror = () => {{ if (++n < 100000) make(); }}; i.src = '{svg}#' + n; }}; "
        "make(); setTimeout(() => console.log('loads ' + n), 3000);</script>"
    )
    return Page(doc("<p>images</p>" + script))


@case(
    "100 XMLHttpRequest calls to a server that answers with 60 MiB, each response kept in an "
    "array as an ArrayBuffer-typed request, with JavaScript on and 4 s of settling (review of "
    "10b: 3 GB before): the bytes of a response count against the heap of 256 MiB when they "
    "arrive (ADR 0028 section 16), so the heap limit stops the page after a few responses",
    server="big",
    expect_log="reached a limit",
    swb_args=("--js", "--settle-ms", "4000"),
    time_limit_s=20,
    rss_limit_mib=1100,
)
def xhr_retained_responses_js_on() -> Page:
    script = (
        "<script>let done = 0; const keep = []; for (let i = 0; i < 100; i++) { "
        "const x = new XMLHttpRequest(); x.open('GET', 'http://127.0.0.1:__PORT__/b' + i); "
        "x.responseType = 'arraybuffer'; x.onloadend = () => { done++; }; keep.push(x); "
        "x.send(); }</script>"
    )
    return Page(doc("<p>xhr</p>" + script))


@case(
    "400 blobs of 32 MiB, each reduced to a slice of 1 byte that the page keeps, and the same "
    "slices passed to URL.createObjectURL, with JavaScript on (review of 10b: abort at 8 GB "
    "before): a small slice of a large blob is a copy, and a blob charges the vectors it "
    "keeps alive, not its length (ADR 0028 section 16)",
    swb_args=("--js",),
    time_limit_s=30,
    rss_limit_mib=1100,
)
def blob_slices_of_large_parents_js_on() -> Page:
    script = (
        "<script>const keep = [], urls = []; let n = 0; for (let i = 0; i < 400; i++) { "
        "const b = new Blob([new Uint8Array(32 << 20)]); keep.push(b.slice(0, 1)); "
        "urls.push(URL.createObjectURL(b.slice(1, 3))); n++; } "
        "console.log('slices ' + n);</script>"
    )
    return Page(doc("<p>blob</p>" + script))


@case(
    "300 XMLHttpRequest calls and 300 fetch() calls to a Blob URL of a 32 MiB blob in one "
    "task, with JavaScript on and 4 s of settling (review of 10b: abort at 60 MiB x 300 "
    "before): a request of a Blob URL keeps the blob's handle until its task runs and copies "
    "the bytes then; at most 256 wait, the others fail (ADR 0028 section 16)",
    expect_log="requests of Blob URLs wait",
    swb_args=("--js", "--settle-ms", "4000"),
    time_limit_s=30,
    rss_limit_mib=1100,
)
def blob_url_request_loop_js_on() -> Page:
    script = (
        "<script>const b = new Blob([new Uint8Array(32 << 20)]); const u = URL.createObjectURL(b); "
        "let ok = 0, failed = 0; for (let i = 0; i < 300; i++) { const x = new XMLHttpRequest(); "
        "x.open('GET', u); x.onload = () => ok++; x.onerror = () => failed++; x.send(); } "
        "for (let i = 0; i < 300; i++) fetch(u).then(() => ok++, () => failed++); "
        "setTimeout(() => console.log('ok ' + ok + ' failed ' + failed), 8000);</script>"
    )
    return Page(doc("<p>blob url</p>" + script))


@case(
    "200 blobs of 32 MiB, each dropped right after an XMLHttpRequest and a fetch() of its Blob "
    "URL (revoked at once), then 200 more, each dropped right after a FileReader read, with "
    "JavaScript on and 4 s of settling (delta review of 10b: abort at 3.6 GB before): the "
    "requests and reads that wait for their task keep at most 256 MiB of blobs alive, the "
    "others fail (ADR 0028 section 16)",
    expect_log="FileReader reads wait with more than",
    swb_args=("--js", "--settle-ms", "4000"),
    time_limit_s=40,
    rss_limit_mib=1100,
)
def blobs_dropped_while_requests_wait_js_on() -> Page:
    script = (
        "<script>let ok = 0, failed = 0; for (let i = 0; i < 200; i++) { "
        "let b = new Blob([new Uint8Array(32 << 20)]); const u = URL.createObjectURL(b); "
        "const x = new XMLHttpRequest(); x.open('GET', u); x.onload = () => ok++; "
        "x.onerror = () => failed++; x.send(); fetch(u).then(() => ok++, () => failed++); "
        "URL.revokeObjectURL(u); b = null; } "
        "for (let i = 0; i < 200; i++) { let b = new Blob([new Uint8Array(32 << 20)]); "
        "const r = new FileReader(); r.onload = () => ok++; r.onerror = () => failed++; "
        "r.readAsArrayBuffer(b); b = null; } "
        "setTimeout(() => console.log('ok ' + ok + ' failed ' + failed), 8000);</script>"
    )
    return Page(doc("<p>dropped blobs</p>" + script))


@case(
    "30 XMLHttpRequest calls to a server that answers with 60 MiB, none kept by the page, "
    "each with an onload handler that reads its request (as jQuery's handlers do), with "
    "JavaScript on and 6 s of settling (delta review of 10b: the handler map of the document "
    "kept every request and its response alive, so the heap limit stopped the scripts after "
    "four responses): the handlers live in the request's own data and die with it",
    server="big",
    forbid_log="reached a limit",
    swb_args=("--js", "--settle-ms", "6000"),
    time_limit_s=40,
    rss_limit_mib=1100,
)
def xhr_handlers_of_unreferenced_requests_js_on() -> Page:
    script = (
        "<script>let done = 0; for (let i = 0; i < 30; i++) { "
        "const x = new XMLHttpRequest(); x.open('GET', 'http://127.0.0.1:__PORT__/b' + i); "
        "x.responseType = 'arraybuffer'; x.onload = () => { done += x.response.byteLength > 0; }; "
        "x.send(); }</script>"
    )
    return Page(doc("<p>xhr handlers</p>" + script))


@case(
    "setRequestHeader with a value of 1 MiB 2,000 times on one header, and 300,000 headers "
    "with different names on one more request, with JavaScript on (review of 10b: abort at "
    "8 GB before): a request keeps at most 256 headers and 64 KiB of them, the rest is "
    "dropped with a warning (ADR 0028 section 16)",
    expect_log="a request holds at most",
    swb_args=("--js",),
    time_limit_s=30,
    rss_limit_mib=1100,
)
def xhr_request_headers_js_on() -> Page:
    script = (
        "<script>const s = 'a'.repeat(1 << 20); const x = new XMLHttpRequest(); "
        "x.open('GET', 'http://127.0.0.1:1/'); for (let i = 0; i < 2000; i++) "
        "x.setRequestHeader('x-a', s); const y = new XMLHttpRequest(); "
        "y.open('GET', 'http://127.0.0.1:1/'); for (let i = 0; i < 300000; i++) "
        "y.setRequestHeader('x-' + i, 'v'); console.log('headers set');</script>"
    )
    return Page(doc("<p>headers</p>" + script))


@case(
    "80 batches of 1,000 new Image() objects with a load handler, all loaded and garbage "
    "(review of 10b: no load event after 65,536 images before), then one more image, with "
    "JavaScript on: the table that watches the images drops the finished watches of images "
    "outside the document, so the last image still gets its load event (ADR 0028 section 16)",
    forbid_log="images are watched for load events",
    swb_args=("--js",),
    time_limit_s=30,
)
def image_watches_of_a_long_page_js_on() -> Page:
    svg = (
        "data:image/svg+xml,%3Csvg xmlns=%27http://www.w3.org/2000/svg%27 "
        "width=%271%27 height=%271%27/%3E"
    )
    # Each batch starts in the load handler of the last image of the one
    # before (images that script created delay the window's load event, so
    # swb stays); without events after 65,536 images the chain stops.
    script = (
        "<script>let fired = 0, round = 0; const batch = () => { round++; "
        "for (let i = 0; i < 1000; i++) { const im = new Image(); "
        "im.onload = () => { fired++; if (i === 999) next(); }; "
        f"im.src = '{svg}#' + round; }} }}; const next = () => {{ if (round < 80) batch(); else {{ "
        f"const l = new Image(); l.onload = () => console.log('lastload true fired ' + fired); "
        f"l.src = '{svg}#last'; }} }}; batch();</script>"
    )
    return Page(doc("<p>images</p>" + script))


@case(
    "DOMMatrix from strings of 18 MiB (5 times) and of 0.9 MiB (30 times), 200,000 matrices "
    "kept by the page, and 20,000 writes of long transition and will-change values to one "
    "element, with JavaScript on (feature 11c1): a matrix string over 1 MiB is a syntax error "
    "without being tokenized, a string inside the limit is parsed in time, and a matrix or a "
    "style value costs no more than its data (ADR 0028 sections 10, 13 and 16)",
    expect_log="matrices 5 200000",
    swb_args=("--js",),
    time_limit_s=30,
)
def dom_matrix_strings_js_on() -> Page:
    script = (
        "<script>const big = 'scale(1) '.repeat(1 << 21); let errors = 0; "
        "for (let i = 0; i < 5; i++) { try { new DOMMatrix(big); } catch (e) { errors++; } } "
        "const ok = 'translate(1px) '.repeat(60000); "
        "for (let i = 0; i < 30; i++) new DOMMatrix(ok); "
        "const keep = []; for (let i = 0; i < 200000; i++) "
        "keep.push(new DOMMatrix([1, 0, 0, 1, i, 0]).translate(1, 2)); "
        "const s = document.body.style; for (let i = 0; i < 20000; i++) { "
        "s.transition = 'opacity 1s ease ' + i + 'ms, width 2s steps(4, end), color 3s'; "
        "s.willChange = 'transform, opacity, ' + i; } "
        "console.warn('matrices ' + errors + ' ' + keep.length);</script>"
    )
    return Page(doc("<p>matrices</p>" + script))


@case(
    "1,000,000 calls of alert, confirm and prompt with messages of 2.6 KiB, with JavaScript "
    "on (feature 11c2): a dialog is dismissed at once and costs one string conversion (about "
    "12 microseconds), so the time limit of 5 s ends the loop; the log (RUST_LOG=debug) shows "
    "the first 100 dialogs (ADR 0028 section 4)",
    expect_log="the time limit was exceeded",
    swb_args=("--js",),
    time_limit_s=25,
)
def dialogs_loop_js_on() -> Page:
    script = (
        "<script>const m = 'a long message '.repeat(200); let n = 0; "
        "for (let i = 0; i < 1000000; i++) { "
        "if (i % 3 === 0) alert(m + i); else if (i % 3 === 1) { if (confirm(m)) n++; } "
        "else if (prompt(m, m) !== null) n++; }</script>"
    )
    return Page(doc("<p>dialogs</p>" + script))


@case(
    "200,000 print() calls with listeners after the load and 50,000 writes to "
    "navigator.clipboard, with JavaScript on (feature 11c2): each print() fires two events, "
    "and a rejected clipboard promise costs no more than its data, with 2 s of settle time "
    "(ADR 0028 sections 4 and 13)",
    expect_log="prints 200000 clipboard 50000",
    swb_args=("--js", "--settle-ms", "2000"),
    time_limit_s=30,
)
def print_clipboard_loop_js_on() -> Page:
    script = (
        "<script>let prints = 0, rejected = 0; "
        "addEventListener('beforeprint', () => { prints++; }); "
        "addEventListener('load', () => setTimeout(() => { "
        "for (let i = 0; i < 200000; i++) print(); "
        "const m = 'a long message '.repeat(200); "
        "for (let i = 0; i < 50000; i++) "
        "navigator.clipboard.writeText(m).catch(() => { rejected++; }); "
        "Promise.resolve().then(() => setTimeout(() => "
        "console.warn('prints ' + prints + ' clipboard ' + rejected), 100)); }, 0));</script>"
    )
    return Page(doc("<p>print</p>" + script))


@case(
    "A flood of window.postMessage to the own window with JavaScript on (M9 feature 1): 100 "
    "messages of 4 MiB and then 200,000 small ones in one task, and a listener that posts a "
    "message for each message that it gets, without end. At most 65,536 messages and 256 MiB "
    "of serialized "
    "bytes wait; later calls drop their message with one warning, and the task source runs "
    "the messages as tasks, so the page settles after --settle-ms (ADR 0031 section 11)",
    expect_log="further messages are dropped",
    swb_args=("--js", "--settle-ms", "1500"),
    time_limit_s=30,
    rss_limit_mib=2048,
)
def post_message_flood_to_self_js_on() -> Page:
    script = (
        "<script>let got = 0, loops = 0; "
        "addEventListener('message', (e) => { if (e.data === 'loop') { loops++; "
        "postMessage('loop', '*'); } else got++; }); "
        "const big = 'x'.repeat(4 << 20); "
        "for (let i = 0; i < 100; i++) postMessage(big + i, '*'); "
        "for (let i = 0; i < 200000; i++) postMessage(i, '*'); "
        "setTimeout(() => { postMessage('loop', '*'); "
        "setTimeout(() => console.log('got ' + got + ' loops ' + loops), 700); }, 300);"
        "</script>"
    )
    return Page(doc("<p>messages</p>" + script))


@case(
    "A MessageChannel ping-pong without end (M9 feature 9): two ports send each other a message "
    "from the handler of the last one, while a timer counts and an animation frame callback runs. "
    "Each message is a task with no timer clamp; the event loop orders the tasks by queuing time, "
    "so the timer and the frames get their turns and --settle-ms ends (ADR 0031 sections 7 and 11)",
    swb_args=("--js", "--settle-ms", "1500"),
    time_limit_s=10,
    rss_limit_mib=320,
)
def message_channel_ping_pong_js_on() -> Page:
    script = (
        "<script>const c = new MessageChannel(); let pongs = 0, ticks = 0, frames = 0; "
        "c.port1.onmessage = (e) => c.port2.postMessage(e.data); "
        "c.port2.onmessage = (e) => { pongs++; c.port1.postMessage(e.data); }; "
        "c.port1.postMessage('ping'); "
        "setInterval(() => ticks++, 10); "
        "const frame = () => { frames++; requestAnimationFrame(frame); }; "
        "requestAnimationFrame(frame); "
        "setTimeout(() => console.log('pongs ' + pongs + ' ticks ' + ticks + ' frames ' + "
        "frames), 1000);"
        "</script>"
    )
    return Page(doc("<p>ping-pong</p>" + script))


@case(
    "One message with 10,000 ports (M9 feature 9): 5,000 channels, both ports of each in the "
    "transfer list. The ports travel in the record, the receiver gets 10,000 new objects, and the "
    "ports that nobody holds go with the collector (ADR 0031 sections 7 and 11)",
    swb_args=("--js", "--settle-ms", "1500"),
    time_limit_s=10,
    rss_limit_mib=256,
)
def message_channel_transfer_10000_ports_js_on() -> Page:
    script = (
        "<script>const hub = new MessageChannel(); const ports = []; "
        "for (let i = 0; i < 5000; i++) { const c = new MessageChannel(); "
        "ports.push(c.port1, c.port2); } "
        "hub.port2.onmessage = (e) => console.log('received ' + e.ports.length + ' ' + "
        "e.ports.every((p) => p instanceof MessagePort)); "
        "hub.port1.postMessage('many', ports);</script>"
    )
    return Page(doc("<p>ports</p>" + script))


@case(
    "A million new MessageChannel() in a loop that keeps none (M9 feature 9; some setImmediate "
    "polyfills make a channel per call): the collector frees the pairs and the registry follows, "
    "so the limit of 65,536 live ports is never reached (ADR 0031 sections 7 and 11)",
    swb_args=("--js", "--settle-ms", "500"),
    time_limit_s=10,
    rss_limit_mib=512,
    forbid_log="message ports are live",
)
def message_channel_million_unkept_js_on() -> Page:
    script = (
        "<script>for (let i = 0; i < 1000000; i++) new MessageChannel(); "
        "console.log('made 1000000');</script>"
    )
    return Page(doc("<p>channels</p>" + script))


@case(
    "70,000 channels whose second port travels in a message that only it holds (M9 feature 9, "
    "review fix): c.port1.postMessage(1, [c.port2]) puts the port into its own queue, a cycle "
    "that the collector cannot see. The registry frees such entries and their waiting messages, "
    "so the limits of 65,536 ports and 65,536 waiting messages are not reached, and a message "
    "after the loop is delivered (ADR 0031 section 7); the case fails if swb logs the warning "
    "about the port limit",
    swb_args=("--js", "--settle-ms", "500"),
    time_limit_s=10,
    rss_limit_mib=512,
    forbid_log="message ports are live",
)
def message_channel_travelling_cycle_js_on() -> Page:
    script = (
        "<script>let error = ''; "
        "try { for (let i = 0; i < 70000; i++) { const c = new MessageChannel(); "
        "c.port1.postMessage(1, [c.port2]); } } catch (e) { error = e.name; } "
        "const d = new MessageChannel(); "
        "d.port2.onmessage = (e) => console.log('after ' + e.data + error); "
        "d.port1.postMessage(7);</script>"
    )
    return Page(doc("<p>channels</p>" + script))


@case(
    "70,000 MessageChannel objects that the page keeps (M9 feature 9): at most 65,536 ports are "
    "live; new MessageChannel() past it throws a RangeError and swb logs a warning (Chromium has "
    "no limit; ADR 0031 section 11)",
    expect_log="message ports are live",
    swb_args=("--js", "--settle-ms", "500"),
    time_limit_s=10,
    rss_limit_mib=384,
)
def message_channel_70000_live_js_on() -> Page:
    script = (
        "<script>const keep = []; let error = ''; "
        "try { for (let i = 0; i < 70000; i++) keep.push(new MessageChannel()); } "
        "catch (e) { error = e.name; } "
        "console.log('kept ' + keep.length + ' ' + error);</script>"
    )
    return Page(doc("<p>channels</p>" + script))


@case(
    "A flood of port messages to a port without a listener (M9 feature 9): the queue of the port "
    "holds at most 65,536 waiting messages (window and ports together); a later message is "
    "dropped with a warning, the page settles (Chromium has no limit; ADR 0031 section 11)",
    expect_log="further messages are dropped",
    swb_args=("--js", "--settle-ms", "1500"),
    time_limit_s=10,
    rss_limit_mib=1024,
)
def message_port_flood_without_listener_js_on() -> Page:
    script = (
        "<script>const c = new MessageChannel(); "
        "const big = 'x'.repeat(4 << 20); "
        "for (let i = 0; i < 100; i++) c.port1.postMessage(big + i); "
        "for (let i = 0; i < 300000; i++) c.port1.postMessage(i); "
        "let got = 0; setTimeout(() => { c.port2.onmessage = () => { got++; }; "
        "setTimeout(() => console.log('got ' + got), 700); }, 300);</script>"
    )
    return Page(doc("<p>messages</p>" + script))


# --- document.write (M9 feature 2) -------------------------------------------


@case(
    "A script that writes itself with JavaScript on (M9 feature 2): each copy runs inside the "
    "write of the one before. Nesting stops at script nesting level 22 as in Chromium (the "
    "write of that script is ignored with a warning); the page loads (ADR 0028 section 8)",
    expect_log="the script nesting level",
    swb_args=("--js",),
)
def document_write_itself_js_on() -> Page:
    script = (
        "<script>window.n = (window.n || 0) + 1; "
        "document.write('<script>' + document.currentScript.text + '<\\/script>'); "
        "if (window.n > 1) console.log('depth ' + window.n);</script>"
    )
    return Page(doc("<p>write</p>" + script))


@case(
    "A script that writes two copies of itself with JavaScript on (M9 feature 2): the copies "
    "double at each of the 21 levels of nesting, so the page would get 2^21 scripts; the "
    "bound of 100,000 written scripts per document ends them (before it, the task time "
    "limit ended them at more than 1 GiB), and the page loads within the time limit of "
    "the case (ADR 0028 sections 8 and 16)",
    expect_log="scripts from written text ran",
    swb_args=("--js",),
    time_limit_s=30,
    rss_limit_mib=512,
)
def document_write_fork_js_on() -> Page:
    script = (
        "<script>var c = '<script>' + document.currentScript.text + '<\\/script>'; "
        "document.write(c); document.write(c);</script>"
    )
    return Page(doc("<p>write</p>" + script))


@case(
    "A flood of document.write calls behind a script that the parser waits for, with "
    "JavaScript on (M9 feature 2): 3,000,000 small writes, then 5 writes of 16 MiB, all "
    "inside a comment. The waiting text is bounded at 64 MiB per document (ADR 0028 "
    "section 16): the writes past it are dropped with a warning, and the page loads when "
    "the script (a server that answers after 2 s) arrives",
    expect_log="would wait for the parser",
    server="slow",
    swb_args=("--js",),
    time_limit_s=30,
    rss_limit_mib=2048,
)
def document_write_flood_pending_js_on() -> Page:
    script = (
        '<script>document.write(\'<script src="http://127.0.0.1:__PORT__/w.js"><\\/script>'
        "<!--'); "
        "for (let i = 0; i < 3000000; i++) document.write('x'); "
        "const big = 'y'.repeat(16 << 20); "
        "for (let i = 0; i < 5; i++) document.write(big); "
        "document.write('-->'); "
        "console.log('flooded');</script>"
    )
    return Page(doc("<p>write</p>" + script))


@case(
    "A loop of 600,000 document.writeln calls that each write one comment with JavaScript on "
    "(M9 feature 2): the parser reads each call at once; the nodes of the document (a "
    "comment and a line feed for each call) reach the limit of 1,048,576, the parser stops "
    "with a warning, and the page loads within the time limit (ADR 0028 sections 8 and 16)",
    expect_log="HTML parser stopped",
    swb_args=("--js",),
    time_limit_s=30,
)
def document_writeln_loop_js_on() -> Page:
    script = (
        "<script>for (let i = 0; i < 600000; i++) document.writeln('<!--' + i + '-->');</script>"
    )
    return Page(doc("<p>write</p>" + script))


@case(
    "300,000 Intl.DateTimeFormat, NumberFormat and RelativeTimeFormat objects with options, "
    "each used once, and 100,000 toLocaleString calls with options on dates and numbers, "
    "with JavaScript on (M9 feature 3, ADR 0027 section 10): the objects are garbage at once, "
    "and the heap does not grow with the count",
    swb_args=("--js",),
    time_limit_s=15,
)
def intl_formatters_loop_js_on() -> Page:
    script = (
        "<script>let n = 0; for (let i = 0; i < 300000; i++) { "
        "n += new Intl.DateTimeFormat('en-GB', {hour: 'numeric', minute: '2-digit', "
        "timeZone: 'UTC', timeZoneName: 'short'}).format(i * 60000).length; "
        "n += new Intl.NumberFormat('en-US', {style: 'unit', unit: 'minute', "
        "unitDisplay: 'long'}).format(i).length; "
        "n += new Intl.RelativeTimeFormat('en', {numeric: 'auto'}).format(i % 5 - 2, 'day')"
        ".length; } "
        "for (let i = 0; i < 100000; i++) { "
        "n += new Date(i * 86400000).toLocaleDateString('en-US', {month: 'long', "
        "timeZone: 'UTC'}).length + "
        "i.toLocaleString('en-GB', {maximumFractionDigits: 2}).length; } "
        "console.log('chars ' + n);</script>"
    )
    return Page(doc("<p>intl</p>" + script))


@case(
    "Intl.NumberFormat with the largest digit options on big and long numbers, with JavaScript "
    "on: maximumFractionDigits 100 on doubles from 5e-324 to 1.8e308 and on strings and "
    "BigInts of 300,000 digits (linear), a string of 2 million digits or an exponent of "
    "2^30 (the infinity, read in place without a copy), and a BigInt of 2 million digits "
    "(a RangeError, not a 2 GB string)",
    swb_args=("--js",),
    time_limit_s=15,
)
def intl_number_digits_js_on() -> Page:
    script = (
        "<script>const f = new Intl.NumberFormat('en-US', {maximumFractionDigits: 100, "
        "minimumFractionDigits: 100, minimumIntegerDigits: 21}); let n = 0; "
        "for (const x of [5e-324, 1e-10, 1/3, 123456789.123456789, 1e21, 1.7976931348623157e308]) "
        "n += f.format(x).length; "
        "n += f.format('7'.repeat(300000)).length; n += f.format(10n ** 300000n).length; "
        "const g = new Intl.NumberFormat('en-US'); let errors = 0; "
        "for (const text of ['9'.repeat(2000000), '1e1073741824', '1e-1073741824x']) "
        "try { n += g.format(text).length; } catch (e) { errors++; } "
        "try { g.format(10n ** 2000000n); } catch (e) { errors++; } "
        "console.log('chars ' + n + ' errors ' + errors);</script>"
    )
    return Page(doc("<p>intl</p>" + script))


@case(
    "Intl.getCanonicalLocales and the constructors with a list of 1,000,000 distinct "
    "language tags, a list of 100,000 invalid ones after a valid one, and an array-like "
    "of length 2^32 - 1 with JavaScript on (the time limit ends the last loop)",
    swb_args=("--js",),
    time_limit_s=15,
)
def intl_locale_lists_js_on() -> Page:
    script = (
        "<script>const tags = Array.from({length: 1000000}, (_, i) => 'en-x-t' + i); "
        "console.log('canonical ' + Intl.getCanonicalLocales(tags).length); "
        "console.log('format ' + new Intl.DateTimeFormat(tags).resolvedOptions().locale); "
        "try { Intl.getCanonicalLocales(['en'].concat(Array(100000).fill('en_US'))); } "
        "catch (e) { console.log(e.name); } "
        "try { Intl.getCanonicalLocales({length: 4294967295}); } "
        "catch (e) { console.log('long ' + e.name); }</script>"
    )
    return Page(doc("<p>intl</p>" + script))


@case(
    "100,000 lazy images (loading=lazy) in a page of 5 million px, and a script that scrolls "
    "down in 100 steps of 50,000 px, one step per animation frame, with JavaScript on "
    "(feature 4 of M9): the check of the lazy images walks the painted boxes once per frame "
    "that scrolled and starts only the loads within 3000 px of the viewport (ADR 0032)",
    swb_args=("--js", "--virtual-time", "--settle-ms", "5000"),
    time_limit_s=30,
)
def lazy_images_scrolled_in_steps_js_on() -> Page:
    images = "".join(f"<img loading=lazy src=l{i}.png width=1 height=50>" for i in range(100_000))
    script = (
        "<script>let n = 0; const step = () => { scrollBy(0, 50000); if (++n < 100) "
        "requestAnimationFrame(step); else console.log('lazy scrolled ' + scrollY); }; "
        "addEventListener('load', () => requestAnimationFrame(step));</script>"
    )
    return Page(doc(images + script, "img{display:block}"))


@case(
    "30,000 lazy images far below the viewport, and a script that sets loading to eager and "
    "back to lazy on all of them 20 times, with JavaScript on (feature 4 of M9): the first "
    "change starts each deferred load once (a released image never defers again), later "
    "changes cost the attribute change only (ADR 0032)",
    swb_args=("--js", "--virtual-time", "--settle-ms", "2000"),
    time_limit_s=30,
)
def lazy_loading_attribute_toggled_js_on() -> Page:
    images = "".join(f"<img loading=lazy src=t{i}.png width=1 height=100>" for i in range(30_000))
    script = (
        "<script>addEventListener('load', () => { const all = document.images; "
        "for (let round = 0; round < 20; round++) for (const im of all) "
        "im.loading = round % 2 ? 'lazy' : 'eager'; "
        "console.log('lazy toggled ' + all.length); });</script>"
    )
    return Page(doc(images + script, "img{display:block}"))


@case(
    "200 nested scroll containers (overflow: auto, 100 px high) with 50 lazy images in the "
    "innermost, the first at 5000 px, and a script that scrolls every container down in 40 "
    "animation frames, with JavaScript on (feature 4 of M9): the painted boxes of the images "
    "are found through the "
    "scroll offsets of all containers; the work of a check stays linear (ADR 0032)",
    swb_args=("--js", "--virtual-time", "--settle-ms", "3000"),
    time_limit_s=30,
)
def lazy_images_in_nested_scrollers_js_on() -> Page:
    images = "".join(
        f"<img loading=lazy src=n{i}.png width=1 height=100 "
        f"style='margin-top:{5000 if i == 0 else 0}px'>"
        for i in range(50)
    )
    body = nest("<div class=s>", "</div>", 200, images)
    script = (
        "<script>let n = 0; const step = () => { for (const s of document.querySelectorAll('.s')) "
        "s.scrollTop += 500; if (++n < 40) requestAnimationFrame(step); "
        "else console.log('nested scrolled ' + n); }; "
        "addEventListener('load', () => requestAnimationFrame(step));</script>"
    )
    return Page(doc(body + script, ".s{overflow:auto;height:100px;width:90%}img{display:block}"))


# --- CSSOM style sheets (M9 feature 6, ADR 0028 section 10) ---------------


@case(
    "100,000 insertRule calls into one style element's sheet with a style query "
    "(getComputedStyle) after each, on a page of 2,000 elements, with JavaScript on (M9 "
    "feature 6): each query builds the rule index of all sheets again (no parse of the other "
    "rules), so the loop slows as the sheet grows and the task ends at its time limit of 5 s "
    "(the binding checks the deadline after each flush; ADR 0028 sections 10 and 16)",
    swb_args=("--js",),
    time_limit_s=8,
)
def cssom_insert_rule_flush_loop_js_on() -> Page:
    script = (
        "<script>const s = document.createElement('style'); document.head.appendChild(s); "
        "const sh = s.sheet; const e = document.getElementById('t'); let n = 0; "
        "for (let i = 0; i < 100000; i++) { "
        "sh.insertRule('.c' + i + ' { margin-left: ' + (i % 50) + 'px }', sh.cssRules.length); "
        "e.className = 'c' + i; n += parseFloat(getComputedStyle(e).marginLeft); } "
        "console.log('inserted ' + n);</script>"
    )
    return Page(doc("<p>x</p>" * 2000 + "<div id=t></div>" + script))


@case(
    "deleteRule(0) in a loop over a style sheet of 60,000 rules, then insertRule(…, 0) of "
    "60,000 rules, with JavaScript on (M9 feature 6): each call moves the list of rule "
    "pointers once; the loops end well within the time limit and the stylist is built once "
    "at the end",
    swb_args=("--js",),
    time_limit_s=8,
)
def cssom_delete_first_rule_loop_js_on() -> Page:
    css = "".join(f".d{i} {{ top: {i % 7}px }}" for i in range(60_000))
    script = (
        "<script>const sh = document.querySelector('style').sheet; "
        "while (sh.cssRules.length) sh.deleteRule(0); "
        "for (let i = 0; i < 60000; i++) sh.insertRule('.e' + i + ' { left: 1px }', 0); "
        "console.log('rules ' + sh.cssRules.length);</script>"
    )
    return Page(doc("<p class='d1 e1'>x</p>" + script, css))


@case(
    "A rule with a selector list of 200,000 classes inserted by insertRule and set again by "
    "selectorText, and a second rule with 100,000 declarations set through rule.style, on a "
    "page of 5,000 elements, with JavaScript on (M9 feature 6): one parse of each text, one "
    "rule index of 200,000 selectors, the cssText reads stay linear",
    swb_args=("--js",),
    time_limit_s=10,
    rss_limit_mib=1536,
)
def cssom_huge_selector_list_js_on() -> Page:
    script = (
        "<script>const sh = document.querySelector('style').sheet; "
        "const sel = Array.from({length: 200000}, (_, i) => '.h' + i).join(', '); "
        "sh.insertRule(sel + ' { color: rgb(1, 2, 3) }', 0); "
        "const a = getComputedStyle(document.querySelector('p')).color; "
        "sh.cssRules[0].selectorText = sel.replace(/h/g, 'k'); "
        "const b = getComputedStyle(document.querySelector('p')).color; "
        "sh.insertRule('p { }', 1); let css = ''; "
        "for (let i = 0; i < 100000; i++) css += '--v' + i + ': ' + i + '; '; "
        "sh.cssRules[1].style.cssText = css; "
        "console.log('selectors ' + a + ' ' + b + ' ' + sh.cssRules[0].cssText.length + ' ' + "
        "sh.cssRules[1].cssText.length);</script>"
    )
    return Page(doc("<p class='h7 k9'>x</p>" * 5000 + script))


@case(
    "insertRule of an @media rule nested 100,000 deep (and of @supports and @keyframes text "
    "nested as deep), then cssText of each rule, with JavaScript on (M9 feature 6): the "
    "parser skips the blocks past its nesting limit of 64, so the rule tree, its "
    "serialization and the rule index stay shallow",
    swb_args=("--js",),
    expect_log="CSS blocks nested too deeply",
)
def cssom_deep_media_js_on() -> Page:
    script = (
        "<script>const sh = document.querySelector('style').sheet; let t = 0; "
        "for (const k of ['@media screen', '@supports (display: block)', '@media print']) { "
        "const text = (k + ' { ').repeat(100000) + 'p { color: red }' + ' }'.repeat(100000); "
        "try { sh.insertRule(text, 0); t += sh.cssRules[0].cssText.length; } "
        "catch (e) { t -= 1; } } "
        "try { sh.insertRule('@keyframes k { ' + '0% { '.repeat(100000) + ' }', 0); } catch (e) {} "
        "console.log('nested ' + t + ' ' + getComputedStyle(document.querySelector('p')).color);"
        "</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "insertRule of an empty @media rule into the innermost @media rule in a loop, with "
    "JavaScript on (M9 feature 6): scripts could nest grouping rules without end, and the "
    "serialization and the rule index recurse along the nesting; insertRule refuses a rule "
    "64 levels deep (HierarchyRequestError, a warning), then cssText and a style query run "
    "(ADR 0028 section 10)",
    swb_args=("--js",),
    expect_log="grouping rules nested",
)
def cssom_nested_group_insert_js_on() -> Page:
    script = (
        "<script>const sh = document.querySelector('style').sheet; "
        "sh.insertRule('@media screen { }', 0); let m = sh.cssRules[0]; let n = 0; "
        "for (let i = 0; i < 100000; i++) { "
        "try { m.insertRule('@media screen { p { top: 1px } }', 0); "
        "m = m.cssRules[0]; n++; } catch (e) { break; } } "
        "console.log('nested ' + n + ' ' + sh.cssRules[0].cssText.length + ' ' + "
        "getComputedStyle(document.querySelector('p')).top);</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "10,000 style elements made by a script, then cssRules and cssText of each sheet read "
    "through document.styleSheets three times (30,000 rule wrappers), then all removed and "
    "made again 5 times, with JavaScript on (M9 feature 6): each list access walks the "
    "sheets of the document once; the wrappers of sheets that left the document go from the "
    "maps when they reach their limit",
    swb_args=("--js",),
    time_limit_s=10,
)
def cssom_many_sheets_js_on() -> Page:
    script = (
        "<script>let n = 0; for (let round = 0; round < 5; round++) { const made = []; "
        "for (let i = 0; i < 10000; i++) { const s = document.createElement('style'); "
        "s.textContent = '.m' + i + ' { top: 1px }'; document.head.appendChild(s); made.push(s); } "
        "for (let k = 0; k < 3; k++) { const l = document.styleSheets; "
        "for (let i = 0; i < l.length; i++) { const r = l[i].cssRules[0]; "
        "if (r) n += r.cssText.length; } } "
        "for (const s of made) s.remove(); } console.log('sheets ' + n);</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "One rule of 300,000 declarations (1.8 MB of text) inserted 300 times by insertRule, with "
    "JavaScript on (M9 feature 6, review fix): the parsed rule takes about 100 MB, so 300 "
    "inserts took 29 GB before the bound. The estimated bytes of the style data that "
    "scripts make are limited to 128 MiB per document: the rule is estimated at 135 MB, so "
    "each insert is refused by the check of the text before the parse (QuotaExceededError, "
    "one warning), and the loop ends within the limits of the case (ADR 0028 section 16)",
    swb_args=("--js",),
    expect_log="reaches the limit",
)
def cssom_insert_large_rule_loop_js_on() -> Page:
    script = (
        "<script>const sh = document.querySelector('style').sheet; "
        "const t = '.a{' + '--v:1;'.repeat(300000) + '}'; let ok = 0, refused = 0; "
        "for (let i = 0; i < 300; i++) { try { sh.insertRule(t, 0); ok++; } "
        "catch (e) { if (e.name !== 'QuotaExceededError') throw e; refused++; } } "
        "console.log('inserted ' + ok + ' refused ' + refused + ' rules ' + sh.cssRules.length);"
        "</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "300 style elements, each with a text of 300,000 declarations made by a script, with a "
    "style query after each, and JavaScript on (M9 feature 6, review fix): a parsed sheet of "
    "this text takes about 100 MB. The style data that scripts make is limited to 128 MiB "
    "per document (estimated): later sheets have no rules (one warning), the text itself "
    "stays in the DOM (ADR 0028 section 16)",
    swb_args=("--js",),
    expect_log="reaches the limit",
)
def cssom_large_style_elements_js_on() -> Page:
    script = (
        "<script>const t = '.a{' + '--v:1;'.repeat(300000) + '}'; let rules = 0; "
        "for (let i = 0; i < 300; i++) { const s = document.createElement('style'); "
        "s.textContent = t; document.head.appendChild(s); "
        "getComputedStyle(document.querySelector('p')).color; rules += s.sheet.cssRules.length; } "
        "console.log('rules ' + rules);</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "A text of 100,000 different declarations set 300 times through rule.style.cssText of "
    "300 rules, with JavaScript on (M9 feature 6, review fix): each text takes about 30 MB "
    "once parsed (estimated at 46 MB). The style data that scripts make is limited to 128 "
    "MiB per document (estimated): the sets after the second change nothing (one warning; "
    "the check of the text before the parse refuses them), and the loop ends within the "
    "limits of the case (ADR 0028 section 16)",
    swb_args=("--js",),
    expect_log="reaches the limit",
)
def cssom_rule_style_text_loop_js_on() -> Page:
    script = (
        "<script>const sh = document.querySelector('style').sheet; let css = ''; "
        "for (let i = 0; i < 100000; i++) css += '--v' + i + ': ' + i + '; '; "
        "for (let i = 0; i < 300; i++) sh.insertRule('.r' + i + ' { }', i); "
        "let set = 0; for (let i = 0; i < 300; i++) { sh.cssRules[i].style.cssText = css; "
        "if (sh.cssRules[i].style.length) set++; } console.log('set ' + set);</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "A selector list of 20,000 classes set 300 times through selectorText of 300 rules, "
    "with JavaScript on (M9 feature 6, review fix): selectors that scripts set count "
    "against the style data that scripts may make (128 MiB per document, estimated), so "
    "the sets after the limit change nothing (one warning) and the loop ends within the "
    "limits of the case (ADR 0028 section 16)",
    swb_args=("--js",),
    expect_log="reaches the limit",
)
def cssom_selector_text_loop_js_on() -> Page:
    script = (
        "<script>const sh = document.querySelector('style').sheet; "
        "const sel = Array.from({length: 20000}, (_, i) => '.s' + i).join(', '); "
        "for (let i = 0; i < 300; i++) sh.insertRule('.r' + i + ' { }', i); "
        "let set = 0; for (let i = 0; i < 300; i++) { sh.cssRules[i].selectorText = sel; "
        "if (sh.cssRules[i].selectorText.length > 100) set++; } "
        "console.log('set ' + set);</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "20,000 appendMedium calls with different queries on the MediaList of one sheet, then "
    "item() and deleteMedium of each, with JavaScript on (M9 feature 6, review fix): each "
    "call compares the query with the list without parsing the list again, so the loops "
    "stay far inside the time limit (the version that parsed the list each time took more "
    "than 5 s)",
    swb_args=("--js",),
    time_limit_s=10,
)
def cssom_append_medium_loop_js_on() -> Page:
    script = (
        "<script>const m = document.querySelector('style').sheet.media; "
        "for (let i = 0; i < 20000; i++) m.appendMedium('(min-width: ' + i + 'px)'); "
        "let n = m.length; for (let i = 0; i < m.length; i++) n += m.item(i).length; "
        "for (let i = 0; i < 20000; i++) m.deleteMedium('(min-width: ' + i + 'px)'); "
        "console.log('media ' + n + ' ' + m.length);</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "200,000 insertRule calls of '.aN{all:initial}' into one sheet, with a style flush "
    "every 1,000, with JavaScript on (M9 feature 6, second fix round): the declaration `all` "
    "makes about 150 entries in the cascade block, so a rule takes 13 KB of parsed data "
    "for 20 bytes of text. The bound follows the parsed form (about 17 KB estimated per "
    "rule): inserts after 128 MiB estimated are refused (QuotaExceededError, one warning); "
    "before the fix 200,000 inserts would take 2.6 GB (ADR 0028 section 16)",
    swb_args=("--js",),
    time_limit_s=8,
    expect_log="reaches the limit",
)
def cssom_insert_all_loop_js_on() -> Page:
    script = (
        "<script>const sh = document.querySelector('style').sheet; "
        "const p = document.querySelector('p'); let ok = 0, refused = 0; "
        "for (let i = 0; i < 200000; i++) { "
        "try { sh.insertRule('.a' + i + '{all:initial}', ok); ok++; } "
        "catch (e) { if (e.name !== 'QuotaExceededError') throw e; refused++; } "
        "if (i % 1000 === 999) getComputedStyle(p).color; } "
        "console.log('inserted ' + ok + ' refused ' + refused);</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "200,000 insertRule calls of a rule with the shorthands margin, padding, border, font, "
    "background and grid, with a style flush every 1,000, with JavaScript on (M9 feature 6, "
    "second fix round): a shorthand makes one entry per longhand (about 45 entries and "
    "8 KB for 80 bytes of text). The bound follows the parsed form: inserts after 128 MiB "
    "estimated are refused (ADR 0028 section 16)",
    swb_args=("--js",),
    time_limit_s=8,
    expect_log="reaches the limit",
)
def cssom_insert_shorthands_loop_js_on() -> Page:
    script = (
        "<script>const sh = document.querySelector('style').sheet; "
        "const p = document.querySelector('p'); let ok = 0, refused = 0; "
        "for (let i = 0; i < 200000; i++) { "
        "try { sh.insertRule('.a' + i + '{margin:0;padding:0;border:0;font:12px a;"
        "background:red;grid:none}', ok); ok++; } "
        "catch (e) { if (e.name !== 'QuotaExceededError') throw e; refused++; } "
        "if (i % 1000 === 999) getComputedStyle(p).color; } "
        "console.log('inserted ' + ok + ' refused ' + refused);</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "rule.style.setProperty('--v' + i, 'x'.repeat(1000)) on one inserted rule, i up to "
    "1,000,000, with JavaScript on (M9 feature 6, second fix round): every property is a new "
    "entry with a value of 1 KB, and a write changed the rule without a charge. The charge "
    "follows the block (about 3 KB estimated per entry): writes after 128 MiB estimated "
    "change nothing (one warning), and the loop ends at its time limit of 5 s "
    "(ADR 0028 section 16)",
    swb_args=("--js",),
    time_limit_s=8,
    expect_log="reaches the limit",
)
def cssom_set_property_loop_js_on() -> Page:
    script = (
        "<script>const sh = document.querySelector('style').sheet; sh.insertRule('.a { }', 0); "
        "const st = sh.cssRules[0].style; const v = 'x'.repeat(1000); "
        "for (let i = 0; i < 1000000; i++) st.setProperty('--v' + i, v); "
        "console.log('length ' + st.length);</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "100,000 inserted rules, each set to rule.style.cssText = 'all: initial', with "
    "JavaScript on (M9 feature 6, second fix round): swb's block of rule.style does not "
    "store `all` (departure), so the rules stay small; the loop ends within the limits "
    "(ADR 0028 section 16)",
    swb_args=("--js",),
    time_limit_s=8,
)
def cssom_rule_style_all_loop_js_on() -> Page:
    script = (
        "<script>const sh = document.querySelector('style').sheet; let ok = 0; "
        "for (let i = 0; i < 100000; i++) { sh.insertRule('.c' + i + ' { }', ok); "
        "sh.cssRules[ok].style.cssText = 'all: initial'; ok++; } "
        "console.log('rules ' + sh.cssRules.length);</script>"
    )
    return Page(doc("<p>x</p>" + script))


# --- Module scripts (M9 feature 7) -------------------------------------------


def replay_scripts_page(html: str, scripts: dict[str, str]) -> Page:
    """Like `replay_page`, with JavaScript files of `http://hostile.test/`."""
    entries = [
        {
            "method": "GET",
            "url": REPLAY_URL,
            "status": 200,
            "headers": [["content-type", "text/html; charset=utf-8"]],
            "body": "files/index.html",
        }
    ]
    files: dict[str, str | bytes] = {"files/index.html": html}
    for name, text in scripts.items():
        entries.append(
            {
                "method": "GET",
                "url": f"http://hostile.test/{name}",
                "status": 200,
                "headers": [["content-type", "text/javascript"]],
                "body": f"files/{name}",
            }
        )
        files[f"files/{name}"] = text
    files["manifest.json"] = json.dumps({"version": 1, "entries": entries})
    return Page(html, files)


@case(
    "10,000 inline module scripts in one page, with JavaScript on (M9 feature 7): each runs "
    "in a task of its own after the parse, in order, with a microtask checkpoint; the page "
    "loads within the time limit (ADR 0028 section 8)",
    swb_args=("--js",),
    time_limit_s=10,
)
def module_scripts_inline_10000_js_on() -> Page:
    modules = "<script type=module>window.n = (window.n || 0) + 1;</script>" * 10_000
    script = "<script>addEventListener('load', () => console.log('ran ' + window.n));</script>"
    return Page(doc("<p>modules</p>" + modules + script))


@case(
    "10,000 module scripts with the same URL, with JavaScript on and an http: page from a "
    "replayed fixture (M9 feature 7): the module map has one entry, so the page fetches the "
    "URL once, the module runs once, and each element gets its `load` event (ADR 0028 "
    "section 8)",
    swb_args=("--js", "--replay", "{dir}"),
    time_limit_s=10,
    url=REPLAY_URL,
)
def module_scripts_same_url_10000_js_on() -> Page:
    modules = '<script type=module src="m.js"></script>' * 10_000
    script = (
        "<script>let loads = 0; document.addEventListener('load', (e) => { "
        "if (e.target.localName === 'script') loads++; }, true); "
        "addEventListener('load', () => console.log('runs ' + window.n + ' loads ' + loads));"
        "</script>"
    )
    return replay_scripts_page(
        doc("<p>modules</p>" + script + modules),
        {"m.js": "window.n = (window.n || 0) + 1;"},
    )


@case(
    "10,000 module scripts that need another module (an import), with JavaScript on (M9 "
    "feature 7): swb refuses each one with an `error` event; the first 10 log a warning and "
    "the others a debug line; the page loads within the time limit (ADR 0028 section 8)",
    swb_args=("--js",),
    time_limit_s=10,
    expect_log="is not run",
)
def module_scripts_refused_10000_js_on() -> Page:
    modules = "<script type=module>import './dep.js';</script>" * 10_000
    script = (
        "<script>let errors = 0; document.addEventListener('error', (e) => { "
        "if (e.target.localName === 'script') errors++; }, true); "
        "addEventListener('load', () => console.log('errors ' + errors));</script>"
    )
    return Page(doc("<p>modules</p>" + script + modules))


@case(
    "A module with 70,000 top-level let bindings, with JavaScript on (M9 feature 7): the "
    "bindings of a module are registers of its function, so the module is over the compiler's "
    "limit and fails to compile as a function with so many variables does, with an error "
    "report; the page loads (ADR 0028 sections 6 and 16)",
    swb_args=("--js",),
    time_limit_s=10,
    expect_log="Uncaught",
)
def module_top_level_bindings_70000_js_on() -> Page:
    lets = "".join(f"let v{i}=0;" for i in range(70_000))
    return Page(doc("<p>modules</p><script type=module>" + lets + "</script>"))


@case(
    "A module that inserts 20,000 inline module scripts in a loop, and each of the first "
    "1,000 inserts 10 more, with JavaScript on (M9 feature 7): each inserted module runs in "
    "a task of its own as soon as possible; the page loads within the time limit (ADR 0028 "
    "section 8)",
    swb_args=("--js",),
    time_limit_s=15,
)
def module_inserts_modules_loop_js_on() -> Page:
    insert = (
        "const s = document.createElement('script'); s.type = 'module'; "
        "s.text = 'window.n++'; document.body.append(s);"
    )
    heavy = f"window.n++; for (let j = 0; j < 10; j++) {{ {insert} }}"
    script = (
        "<script type=module>"
        "window.n = 0; "
        "function add(text) { const s = document.createElement('script'); s.type = 'module'; "
        "s.text = text; document.body.append(s); } "
        f"const heavy = {json.dumps(heavy)}; "
        "for (let i = 0; i < 20000; i++) add(i < 1000 ? heavy : 'window.n++'); "
        "addEventListener('load', () => console.log('ran ' + window.n));"
        "</script>"
    )
    return Page(doc("<p>modules</p>" + script))


@case(
    "A module with a syntax error at the end of 1 MiB of source, inline and as a file, "
    "with JavaScript on and an http: page from a replayed fixture (M9 feature 7): the "
    "parser reports the error once for each; the page loads (ADR 0028 section 8)",
    swb_args=("--js", "--replay", "{dir}"),
    time_limit_s=10,
    url=REPLAY_URL,
    expect_log="Unexpected token",
)
def module_syntax_error_1mib_js_on() -> Page:
    text = "var a = 1;\n" * (MIB // 11) + "var x = ;"
    return replay_scripts_page(
        doc(
            "<p>modules</p><script type=module>"
            + text
            + '</script><script type=module src="big.js"></script>'
        ),
        {"big.js": text},
    )


FRAMES_1100 = "<iframe src=f.html></iframe>" * 1100


@case(
    "1,100 iframe elements in markup, each with a small document (M9 feature 10a): the first "
    "1,000 get frames, the others none, with one warning (ADR 0031 section 11)",
    time_limit_s=10,
    expect_log="frames, the limit",
)
def frames_1100_in_markup() -> Page:
    return Page(doc("<p>frames</p>" + FRAMES_1100), {"f.html": "<p>frame"})


@case(
    "1,100 iframe elements in markup with JavaScript on (M9 feature 10a): the first 1,000 get "
    "frames, the others none; the page's load event waits for the 1,000 frames (ADR 0031 "
    "section 11)",
    swb_args=("--js",),
    time_limit_s=10,
    expect_log="frames, the limit",
)
def frames_1100_in_markup_js_on() -> Page:
    script = "<script>addEventListener('load', () => console.log('loaded'));</script>"
    return Page(doc("<p>frames</p>" + script + FRAMES_1100), {"f.html": "<p>frame"})


@case(
    "A frame nested 100 deep through srcdoc (M9 feature 10a): frames nest at most 32 deep; "
    "the deeper iframe elements get no frame, with one warning (ADR 0031 section 11)",
    expect_log="nested 32 deep",
)
def frames_nested_100_srcdoc() -> Page:
    inner = "<p>deepest</p>"
    for _ in range(100):
        inner = (
            '<iframe srcdoc="' + inner.replace("&", "&amp;").replace('"', "&quot;") + '"></iframe>'
        )
    return Page(doc("<p>nested</p>" + inner))


@case(
    "A frame whose document reloads itself with <meta http-equiv=refresh content=0> (M9 "
    "feature 10a): the frame navigates at most 64 times in 10 s of page time; later "
    "navigations are dropped with one warning, and the event loop ends (ADR 0031 section 11)",
    swb_args=("--settle-ms", "2000"),
    time_limit_s=10,
    expect_log="navigated 64 times",
)
def frame_meta_refresh_0() -> Page:
    refresh = "<meta http-equiv=refresh content=0><p>again</p>"
    return Page(doc("<p>refresh</p><iframe src=r.html></iframe>"), {"r.html": refresh})


@case(
    "A page that embeds its own URL, and each level embeds it again (M9 feature 10a): the "
    "first frame loads the page once more, the second is refused because two ancestors' "
    "documents have the URL (Chromium's rule; ADR 0031 sections 2 and 11)",
    expect_log="recursion",
)
def frame_embeds_its_page() -> Page:
    return Page(
        doc("<p>self</p><iframe src=index.html></iframe><iframe src=index.html#b></iframe>")
    )


@case(
    "1,100 iframe elements made by a script in a loop with JavaScript on (M9 feature 10a): the "
    "frames are made when the task ends; the first 1,000 get frames, with one warning (ADR "
    "0031 section 11)",
    swb_args=("--js",),
    time_limit_s=10,
    expect_log="frames, the limit",
)
def frames_1100_by_script_js_on() -> Page:
    script = (
        "<script>for (let i = 0; i < 1100; i++) "
        "document.body.appendChild(document.createElement('iframe'));</script>"
    )
    return Page(doc("<p>frames</p>" + script))


@case(
    "1,000 iframes made by a script, each with the same srcdoc of 1 MiB of small elements, with "
    "JavaScript on (review of M9 feature 10a): the frame documents that are alive together "
    "are capped at 64 MiB of source and 1 Mi nodes per page; later documents show an error, "
    "with one warning (ADR 0031 section 11)",
    swb_args=("--js",),
    time_limit_s=30,
    rss_limit_mib=2048,
    expect_log="nodes, the limit",
)
def frames_1000_big_srcdoc_js_on() -> Page:
    script = (
        "<script>const t = '<p>x</p>'.repeat(130000); "
        "for (let i = 0; i < 1000; i++) { const f = document.createElement('iframe'); "
        "f.srcdoc = t; document.body.appendChild(f); }</script>"
    )
    return Page(doc("<p>big srcdoc</p>" + script))


@case(
    "1,000 iframes made by a script, each with the same srcdoc of 350,000 elements in 1 MiB "
    "(<p> without end tags, 3 bytes per element), with JavaScript on (review of M9 feature "
    "10a): the node budget of 1 Mi per page stops the documents long before the budget of "
    "bytes (ADR 0031 section 11)",
    swb_args=("--js",),
    time_limit_s=30,
    rss_limit_mib=2048,
    expect_log="nodes, the limit",
)
def frames_1000_dense_srcdoc_js_on() -> Page:
    script = (
        "<script>const t = '<p>'.repeat(350000); "
        "for (let i = 0; i < 1000; i++) { const f = document.createElement('iframe'); "
        "f.srcdoc = t; document.body.appendChild(f); }</script>"
    )
    return Page(doc("<p>dense srcdoc</p>" + script))


@case(
    "1,000 iframes with a small srcdoc, then a script changes each srcdoc to one of 2,000,000 "
    "elements, with JavaScript on (second review of M9 feature 10a): a rejected frame gives its "
    "small document back, and that must not lift the limit of the next parse; each parse "
    "would otherwise take about 0.2 s (ADR 0031 section 11)",
    swb_args=("--js",),
    time_limit_s=30,
    rss_limit_mib=2048,
    expect_log="nodes, the limit",
)
def frames_1000_small_to_big_srcdoc_js_on() -> Page:
    script = (
        "<script>const fs = []; for (let i = 0; i < 1000; i++) { "
        "const f = document.createElement('iframe'); f.srcdoc = '<p>small'; "
        "document.body.appendChild(f); fs.push(f); } "
        "setTimeout(() => { const t = '<p>'.repeat(2000000); "
        "for (const f of fs) f.srcdoc = t; }, 100);</script>"
    )
    return Page(doc("<p>small to big</p>" + script))


@case(
    "1,000 iframes in markup, each loading the same file of 10 MiB (review of M9 feature 10a): "
    "the documents of frames that are alive together are capped at 64 MiB of source per page; "
    "later frames show an error, with one warning (ADR 0031 section 11)",
    time_limit_s=30,
    rss_limit_mib=2048,
    expect_log="nodes, the limit",
)
def frames_1000_big_documents() -> Page:
    big = "<p>x</p>" * (10 * MIB // 8)
    return Page(
        doc("<p>big documents</p>" + "<iframe src=big.html></iframe>" * 1000), {"big.html": big}
    )


@case(
    "100 iframes in markup, each with 20 different images of 4000 x 3000 px (a file of a few "
    "KB, 48 MB decoded each; review of M9 feature 10a): the decoded pixels of the raster images "
    "of frame documents are capped at 512 MiB per page; later images fail, with a warning "
    "each, and the page ends (ADR 0031 section 11)",
    time_limit_s=20,
    rss_limit_mib=1536,
    expect_log="have no room left",
)
def frames_100_big_images() -> Page:
    inner = "".join(f"<img src=big.png?{i} width=20 height=20>" for i in range(20))
    return Page(
        doc("<p>big images</p>" + "<iframe src=f.html></iframe>" * 100),
        {"f.html": inner, "big.png": stripes_png(4000, 3000)},
    )


@case(
    "200 iframes in markup, each with an external style sheet of 5 MB of comment and one rule "
    "(review of M9 feature 10a): the style sheets of frame documents are capped at 8 MiB per "
    "page; later sheets fail, with a warning each (ADR 0031 section 11)",
    time_limit_s=30,
    expect_log="style sheets of frame documents hold",
)
def frames_200_big_style_sheets() -> Page:
    css = "/*" + " " * (5 * MIB) + "*/p{color:red}"
    return Page(
        doc("<p>big sheets</p>" + "<iframe src=f.html></iframe>" * 200),
        {"f.html": "<link rel=stylesheet href=big.css><p>x", "big.css": css},
    )


@case(
    "A script replaces the srcdoc of 4 frames with 1 MiB of text 5,000 times, and makes and "
    "removes frames with such a srcdoc, with JavaScript on (review of M9 feature 10a): the "
    "bytes of replaced and destroyed documents leave the budget, so it is never reached",
    swb_args=("--js", "--settle-ms", "1500"),
    time_limit_s=30,
    forbid_log="nodes, the limit",
)
def frames_replaced_in_a_loop_js_on() -> Page:
    script = (
        "<script>const t = 'x'.repeat(1 << 20); let n = 0; "
        "const fs = [...document.querySelectorAll('iframe')]; "
        "function step() { for (let i = 0; i < 50; i++, n++) { fs[n % 4].srcdoc = t + n; "
        "const g = document.createElement('iframe'); g.srcdoc = t; "
        "document.body.appendChild(g); g.remove(); } "
        "if (n < 5000) setTimeout(step, 0); else console.log('done ' + n); } step();</script>"
    )
    return Page(doc("<p>replace</p>" + "<iframe></iframe>" * 4 + script))


@case(
    "Frames made and removed in a loop by a script with JavaScript on, also with src, srcdoc "
    "and src changes (M9 feature 10a): 200,000 insertions and removals in tasks of 1,000; "
    "frames that leave the document before the task ends get no frame, the others are "
    "destroyed with their requests (ADR 0031 section 2)",
    swb_args=("--js", "--settle-ms", "1500"),
    time_limit_s=15,
)
def frames_made_and_removed_js_on() -> Page:
    script = (
        "<script>let n = 0; function step() { for (let i = 0; i < 1000; i++) { "
        "const f = document.createElement('iframe'); "
        "if (i % 3 === 0) f.src = 'f.html?' + n; else if (i % 3 === 1) f.srcdoc = '<p>' + n; "
        "document.body.appendChild(f); if (i % 2) f.remove(); n++; } "
        "for (const f of [...document.querySelectorAll('iframe')]) { f.src = 'f.html?x' + n; "
        "f.remove(); } "
        "if (n < 200000) setTimeout(step, 0); else console.log('done ' + n); } step();</script>"
    )
    return Page(doc("<p>frames</p>" + script), {"f.html": "<p>frame"})


FULL_FRAMES = "iframe{position:absolute;left:0;top:0;width:100%;height:100%;border:0}"
"""Frames that all cover the viewport, so that the rasterizer draws all of them."""


@case(
    "1,000 frames over the whole viewport, each with a document of 250 boxes with text (M9 "
    "feature 10b): the rasterizer walks 1.25 million items in one frame of the page, within its "
    "budget of 4 Mi items. 250 boxes keep the case under the memory cap of this set; at 500 "
    "boxes, the node budget's end, it takes 2.6 GiB (ADR 0031 section 11)",
    time_limit_s=10,
    rss_limit_mib=2048,
)
def frames_1000_big_lists() -> Page:
    box = "<div style='background:rgb(1,2,3);border:1px solid'>text</div>"
    return Page(
        doc("<iframe src=f.html></iframe>" * 1000, FULL_FRAMES),
        {"f.html": box * 250},
    )


@case(
    "One frame with a document of 800,000 boxes with an opacity, a filter, a border and a clip "
    "(M9 feature 10b): its display list is longer than the 4 Mi items of a rasterization of "
    "frames, so it paints nothing, with one warning (ADR 0031 section 11)",
    time_limit_s=6,
    rss_limit_mib=2048,
    expect_log="frames: the display lists",
)
def frame_past_the_item_budget() -> Page:
    css = (
        "div{opacity:.9;filter:invert(0);background:rgb(1,2,3);border:1px solid;overflow:hidden;"
        "width:1px;height:1px}"
    )
    return Page(
        doc("<p>big list</p><iframe src=f.html></iframe>"),
        {"f.html": f"<style>{css}</style>" + "<div></div>" * 800_000},
    )


def nested_frames(depth: int, css: str, inner: str = "<p>end") -> dict[str, str]:
    """Files `f0.html` to `f{depth - 1}.html`: each document has a style sheet
    `css` and a frame with the next one (separate URLs: a document does not
    embed itself)."""
    files = {
        f"f{i}.html": f"<!doctype html><style>{css}</style><iframe src=f{i + 1}.html></iframe>"
        for i in range(depth - 1)
    }
    files[f"f{depth - 1}.html"] = inner
    return files


@case(
    "Frames nested 40 deep, each iframe with a blur and a rotation (M9 feature 10b): frames nest "
    "at most 32 deep and transform layers 8 deep; the filter and layer budgets of the "
    "rasterization cover the frames (ADR 0031 section 11)",
    time_limit_s=3,
    rss_limit_mib=512,
    expect_log="frames are nested 32 deep",
)
def frames_nested_filter_transform() -> Page:
    css = (
        "html{background:rgb(200,200,255)} iframe{filter:blur(2px);transform:rotate(1deg);"
        "width:95vw;height:95vh;border:0}"
    )
    return Page(
        doc(f"<style>{css}</style><iframe src=f0.html></iframe>"),
        nested_frames(40, css),
    )


@case(
    "Frames nested 40 deep, each iframe with an opacity (M9 feature 10b): the layers of the 32 "
    "levels stay within the layer budget of the rasterization (ADR 0031 section 11)",
    time_limit_s=3,
    rss_limit_mib=512,
    expect_log="frames are nested 32 deep",
)
def frames_nested_opacity() -> Page:
    css = "iframe{opacity:.9;width:100vw;height:100vh;border:0}"
    return Page(
        doc(f"<style>{css}</style><iframe src=f0.html></iframe>"),
        nested_frames(40, css),
    )


@case(
    "Frames nested 32 deep, each document with 250 nested boxes with a translation (M9 feature "
    "10b): one rasterizer recurses through the groups of all documents of the chain; the groups "
    "nest at most 1,024 deep in frames, with one warning (ADR 0031 section 11)",
    time_limit_s=3,
    rss_limit_mib=512,
    expect_log="frames: the display lists",
)
def frames_nested_groups() -> Page:
    box = "<div style='transform:translate(0,0);width:100vw;height:100vh'>"
    css = "body{margin:0} iframe{width:100vw;height:100vh;border:0}"
    files = {
        f"f{i}.html": (
            f"<!doctype html><style>{css}</style>"
            + nest(box, "</div>", 250, f"<iframe src=f{i + 1}.html></iframe>")
        )
        for i in range(31)
    }
    files["f31.html"] = "<p>end"
    return Page(
        doc(
            "<style>iframe{width:100vw;height:100vh;border:0}</style><iframe src=f0.html></iframe>"
        ),
        files,
    )


def many_shapes_svg(shapes: int) -> str:
    """An SVG image with `shapes` translucent rectangles over its whole area."""
    rect = "<rect width='1000' height='1000' fill='rgb(10,20,30)' fill-opacity='.5'/>"
    return (
        "<svg xmlns='http://www.w3.org/2000/svg' width='1000' height='1000'>"
        + rect * shapes
        + "</svg>"
    )


@case(
    "200 frames over the whole viewport, each with a different SVG image of 100,000 shapes, one "
    "rendering each (M9 feature 10b): the frames share the rendering budget of one frame of the "
    "page and one cache of renderings of 128 MiB (ADR 0031 section 11)",
    time_limit_s=8,
    rss_limit_mib=1800,
    expect_log="SVG images: the rendering budget",
)
def frames_big_svg_images() -> Page:
    svg = many_shapes_svg(100_000)
    return Page(
        doc("<iframe src=f.html></iframe>" * 200, FULL_FRAMES),
        {
            "f.html": "<body style='margin:0'><img src=i.svg style='width:100vw;height:100vh'>",
            "i.svg": svg,
        },
    )


@case(
    "1,000 frames of 300 x 300 px over each other, with an SVG image of the size of the frame "
    "(M9 feature 10b): without a page-wide cache, the renderings would take 360 MB; the frames "
    "share a cache of 128 MiB (ADR 0031 section 11)",
    time_limit_s=5,
    rss_limit_mib=600,
)
def frames_1000_svg_renderings() -> Page:
    svg = many_shapes_svg(1)
    css = "iframe{position:absolute;left:0;top:0;width:300px;height:300px;border:0}"
    return Page(
        doc("<iframe src=f.html></iframe>" * 1000, css),
        {
            "f.html": "<body style='margin:0'><img src=i.svg style='width:100vw;height:100vh'>",
            "i.svg": svg,
        },
    )


@case(
    "Four viewport changes with 1,000 visible frames whose size depends on the viewport, with "
    "JavaScript on (M9 feature 10b): each change lays out all frame documents again, and the "
    "last one paints them (the budgets of a rendering stay as they are)",
    swb_args=(
        "--js",
        "--steps",
        '[{"viewport":[1000,700]},{"wait":50},{"viewport":[900,700]},{"wait":50},'
        '{"viewport":[800,600]},{"wait":50},{"viewport":[1200,800]},{"wait":50}]',
    ),
    time_limit_s=5,
    rss_limit_mib=1024,
)
def frames_1000_viewport_changes_js_on() -> Page:
    css = "iframe{width:9%;height:40px;border:0;float:left}"
    body = "<p>text in a frame</p>" * 5
    return Page(doc("<iframe src=f.html></iframe>" * 1000, css), {"f.html": body})


@case(
    "1,000 frames, each with 40 @font-face rules with font files of its own (M9 feature 10b): "
    "each frame document has its own faces, and the requests come in one burst, so the budget of "
    "response bodies acts first, with one warning; the decoded fonts of frame documents have "
    "their own budget per page (ADR 0031 section 11)",
    time_limit_s=5,
    rss_limit_mib=2048,
    expect_log="over the memory budget",
)
def frames_1000_web_fonts() -> Page:
    fonts = range(40)
    rules = "".join(f"@font-face{{font-family:F{i};src:url(f.ttf?{i})}}" for i in fonts)
    text = "".join(f"<p style='font-family:F{i}'>x" for i in fonts)
    return Page(
        doc("<iframe src=f.html></iframe>" * 1000),
        {"f.html": f"<style>{rules}</style>{text}", "f.ttf": _web_font()},
    )
