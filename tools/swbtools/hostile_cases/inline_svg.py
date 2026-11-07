"""Inline SVG (ADR 0023)."""

from swbtools.hostile_cases.core import (
    JS,
    MIB,
    SVG_NS,
    Page,
    case,
    doc,
    img_page,
    nest,
    stripes_png,
)

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
    swb_args=JS,
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
    swb_args=JS,
)
def noscript_flood_js_on() -> Page:
    item = "<noscript><link rel=stylesheet href=a.css><img src=b.png><p>text &amp; more</p>"
    item += "</noscript>"
    return Page(f"<!doctype html><head>{item * 100}</head><body>{item * 50_000}")


@case(
    "20,000 style elements, each after a script, with JavaScript on: the parse pauses at "
    "each script and the engine drains the change journal after each step; a drain does not "
    "scan or sort the whole list of sheets (ADR 0028 sections 3 and 9)",
    swb_args=JS,
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
    swb_args=JS,
)
def parse_steps_deep_images_js_on() -> Page:
    items = ("<img src=a.svg>x" * 50 + "<script></script>") * 1_000
    svg = "<svg xmlns='http://www.w3.org/2000/svg' width='4' height='4'/>"
    return Page(doc(nest("<div>", "</div>", 400, items)), {"a.svg": svg})


@case(
    "20,000 images with unique URLs, each followed by a script, with JavaScript on: each "
    "parser step selects only its own image; the work of a step does not grow with the "
    "number of images (ADR 0028 section 9)",
    swb_args=JS,
)
def parse_steps_unique_images_js_on() -> Page:
    items = "".join(f"<img src=i{i}.png><script></script>" for i in range(20_000))
    return Page(doc(items))


@case(
    "1,100,000 comments with a script after each 1,000, with JavaScript on: past the node "
    "limit while the parser runs in steps and the journal records each node (ADR 0028 "
    "section 16)",
    expect_log="HTML parser stopped",
    swb_args=JS,
)
def dom_node_limit_js_on() -> Page:
    return Page(doc(("<!---->" * 1_000 + "<script></script>") * 1_100))


@case(
    "6 inline scripts with endless loops, with JavaScript on: each task ends at the time "
    "limit of 5 s, and after the third the document runs no more scripts (ADR 0028 "
    "sections 7 and 16)",
    time_limit_s=25,
    expect_log="the scripts of the document stop",
    swb_args=JS,
)
def inline_script_endless_loops_js_on() -> Page:
    return Page(doc("<p>text</p>" + "<script>for (;;) {}</script>" * 6))


@case(
    "50,000 paragraphs whose wrappers, child lists and expandos a script creates, with "
    "JavaScript on: one wrapper per node in the wrapper vector, the lists in the side "
    "table, and each list walks its children once per DOM generation (ADR 0028 section 4)",
    swb_args=JS,
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
    swb_args=JS,
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
    swb_args=JS,
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
    swb_args=JS,
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
    swb_args=JS,
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
    swb_args=JS,
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
    swb_args=JS,
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
    swb_args=JS,
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
    swb_args=JS,
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
    swb_args=JS,
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
    swb_args=JS,
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
    swb_args=JS,
)
def console_error_loop_js_on() -> Page:
    script = "<script>for (let i = 0; ; i++) console.error('line ' + i);</script>"
    return Page(doc("<p>text</p>" + script))


@case(
    "Inline scripts that each insert the next inline script without end, with JavaScript on: "
    "an inserted inline script runs inside the insertion, so the chain nests; the shared "
    "recursion budget ends it with a RangeError (ADR 0028 sections 8 and 16)",
    expect_log="Maximum call stack size exceeded",
    swb_args=JS,
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
    swb_args=JS,
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
    swb_args=JS,
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
    swb_args=JS,
)
def script_text_held_js_on() -> Page:
    big = "/*" + "x" * (6 * MIB) + "*/"
    blocker = "/*" + "y" * (10 * MIB) + "*/"
    scripts = "<script defer src=big.js></script>" * 60 + "<script src=blocker.js></script>"
    return Page(
        doc("<p>text</p>" + scripts + "<p>after</p>"), {"big.js": big, "blocker.js": blocker}
    )
