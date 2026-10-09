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


CASES: list[Case] = []
"""All cases, in the order in which they run."""


def case(
    description: str,
    time_limit_s: float = 5.0,
    rss_limit_mib: float = 1024,
    known_failure: str | None = None,
    expect_log: str | None = None,
    swb_args: tuple[str, ...] = (),
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
