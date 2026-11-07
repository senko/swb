"""Generic hostile input and the limits of the style, table, SVG image, float,
positioning, grid, mask and scroll container code (ADR 0004 to 0019)."""

import base64

from swbtools.hostile_cases.core import (
    MIB,
    SVG_NS,
    Page,
    case,
    doc,
    img_page,
    nest,
)

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


@case("@layer blocks nested 3,000 deep (past the block depth limit of 64) (ADR 0007)")
def layers_nested_blocks() -> Page:
    css = nest("@layer a{", "}", 3000, "p{color:red}") + nest("@layer{", "}", 3000, "p{margin:0}")
    return Page(doc("<p>x</p>", css))


@case(
    "a layer name of 200,000 segments (a.a.a...), as a statement and a block, and 1,000 "
    "different names of 1,000 segments: past the limit of 65,536 layers (ADR 0007)",
    time_limit_s=10,
    expect_log="more than 65536 cascade layers",
)
def layers_long_dotted_names() -> Page:
    long = ".".join(["a"] * 200_000)
    css = f"@layer {long};@layer {long}{{p{{color:red}}}}\n"
    for i in range(1000):
        name = ".".join([f"n{i}"] + ["s"] * 999)
        css += f"@layer {name}{{p{{color:blue}}}}\n"
    return Page(doc("<p>x</p>", css))


@case(
    "100,000 layer names in statements and blocks, and 100,000 anonymous layers: past the "
    "limit of 65,536 layers (ADR 0007)",
    time_limit_s=10,
    expect_log="more than 65536 cascade layers",
)
def layers_many_names() -> Page:
    names = ",".join(f"l{i}" for i in range(100_000))
    css = f"@layer {names};\n"
    css += "".join(f"@layer l{i}{{.c{i % 500}{{color:red}}}}\n" for i in range(100_000))
    css += "@layer{.c1{color:blue}}" * 100_000
    return Page(doc("".join(f"<i class=c{i}>x</i>" for i in range(500)), css))


@case(
    "3,000 layers whose rules all match 5,000 elements, with !important, revert-layer and "
    "the style attribute: 15,000,000 declarations (ADR 0007)",
    time_limit_s=20,
)
def layers_important_revert_layer() -> Page:
    css = "".join(
        f"@layer l{i}{{p{{color:red!important;margin-top:revert-layer}} "
        f".k{{width:{i}px!important;--v:{i}}}}}\n"
        for i in range(3000)
    )
    css += "p{color:blue;margin-top:revert-layer}"
    body = "".join("<p class=k style='color:revert-layer'>x</p>" for _ in range(5000))
    return Page(doc(body, css))


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


@case("single-line flex containers nested 200 deep in floats, items limited by flex-basis")
def flex_min_content_nesting() -> Page:
    # The min-content size of each level sums the contributions of its two
    # items; the limit by `flex-basis` needs the minimum width of the item,
    # which must not make the work grow with 2^depth.
    inner = "aaaa bbbb"
    for _ in range(200):
        inner = (
            "<div style='float:left'><div style='display:flex'>"
            f"<div style='flex:0 1 5px'>{inner}</div>"
            "<div style='width:30px;flex:0 1 5px'>y</div></div></div>"
        )
    return Page(doc(f"<div style='width:0'>{inner}</div>"))


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
