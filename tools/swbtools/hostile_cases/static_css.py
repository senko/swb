"""The static CSS of M13 feature 2a (ADR 0045): `box-shadow` and `text-shadow` with huge
blurs, spreads and lists, `text-wrap: balance` and `pretty` on very long paragraphs, extreme
`corner-shape` values, and the individual transform properties. The raster work of shadows
is bounded by the layer and filter budgets (ADR 0033); the line planner of `text-wrap`
works on at most 4,096 groups and 256 groups per line."""

from swbtools.hostile_cases.core import Page, case, doc


@case(
    "A box-shadow with a blur radius of 100,000 px (M13 feature 2a): the blur is a filter "
    "group whose standard deviation is limited, and the layer is limited by the budgets",
    time_limit_s=20,
)
def box_shadow_blur_100000() -> Page:
    css = (
        "div{width:200px;height:100px;margin:50px;background:#fff;"
        "box-shadow:0 0 100000px 50000px #000, inset 0 0 100000px #c00}"
    )
    return Page(doc("<div>x</div>", css))


@case(
    "10,000 box-shadows on one element (M13 feature 2a): only the first 64 paint",
    time_limit_s=20,
)
def box_shadow_10000_on_one_element() -> Page:
    shadows = ", ".join(
        f"{i % 7}px {i % 5}px {i % 11}px {i % 3}px rgba({i % 255},0,0,.1)" for i in range(10000)
    )
    css = f"div{{width:100px;height:60px;margin:40px;box-shadow:{shadows}}}"
    return Page(doc("<div>x</div>", css))


@case(
    "A huge spread, and shadows of 2,000 elements with blurs (M13 feature 2a): the shadow "
    "layers are skipped when the budgets are spent",
    time_limit_s=30,
)
def box_shadow_huge_spread_and_many_boxes() -> Page:
    css = (
        "div{width:1px;height:1px;margin:3px;display:inline-block;"
        "box-shadow:0 0 30px 5000000px #00f}"
        "i{display:inline-block;width:40px;height:20px;margin:6px;border-radius:9px;"
        "box-shadow:0 4px 20px rgba(0,0,0,.4), inset 0 2px 8px #fff}"
    )
    return Page(doc("<div></div>" + "<i></i>" * 2000, css))


@case(
    "text-shadow with a blur of 200 px on 5,000 words (M13 feature 2a): each text fragment "
    "is one filter group",
    time_limit_s=30,
)
def text_shadow_large_blur_on_long_text() -> Page:
    words = " ".join(f"word{i}" for i in range(5000))
    css = (
        "p{width:600px;font-size:20px;text-shadow:0 0 200px #f00, 3px 3px 40px #00f, 1px 1px #000}"
    )
    return Page(doc(f"<p>{words}</p>", css))


@case(
    "text-wrap: balance on one paragraph of 100,000 words (M13 feature 2a): more than six "
    "lines, so it breaks as `auto`; the planner gives up beyond 4,096 groups",
    time_limit_s=30,
)
def text_wrap_balance_100000_words() -> Page:
    words = " ".join(["ab"] * 100000)
    return Page(doc(f"<p>{words}</p>", "p{width:300px;text-wrap:balance}"))


@case(
    "text-wrap: pretty on one paragraph of 100,000 words, and balance on 3,000 words in a "
    "box 100,000 px wide (one line of 3,000 groups; M13 feature 2a)",
    time_limit_s=30,
)
def text_wrap_pretty_and_wide_balance() -> Page:
    words = " ".join(["ab"] * 100000)
    few = " ".join(["cd"] * 3000)
    css = "p{width:300px;text-wrap:pretty} q{display:block;width:100000px;text-wrap:balance}"
    return Page(doc(f"<p>{words}</p><q>{few}</q>", css))


@case(
    "text-wrap: balance on 5,000 paragraphs of five lines (M13 feature 2a)",
    time_limit_s=30,
)
def text_wrap_balance_5000_paragraphs() -> Page:
    para = "<p>" + " ".join(f"w{i}" for i in range(40)) + "</p>"
    return Page(doc(para * 5000, "p{width:80px;margin:0;font-size:10px;text-wrap:balance}"))


@case(
    "Extreme corner-shape values and radii: superellipse(1e30), superellipse(-infinity), NaN "
    "from calc(), radii of 1e9 px and a 1 px box, on 2,000 boxes with a border and overflow "
    "(M13 feature 2a)",
    time_limit_s=30,
)
def corner_shape_extreme_values() -> Page:
    css = (
        "b{display:inline-block;width:30px;height:20px;margin:4px;overflow:hidden;"
        "border:2px solid #040;background:#0a0;border-radius:1e9px}"
        "b:nth-child(4n){corner-shape:superellipse(1e30)}"
        "b:nth-child(4n+1){corner-shape:superellipse(-infinity)}"
        "b:nth-child(4n+2){corner-shape:superellipse(calc(0 / 0))}"
        "b:nth-child(4n+3){corner-shape:scoop notch square bevel;width:1px;height:1px}"
    )
    return Page(doc("<b><i>x</i></b>" * 2000, css))


@case(
    "translate, rotate and scale with huge and invalid values on 5,000 boxes (M13 feature "
    "2a): the matrix stays finite",
    time_limit_s=30,
)
def individual_transforms_extreme_values() -> Page:
    css = (
        "i{display:inline-block;width:20px;height:20px;background:#a0a}"
        "i:nth-child(3n){translate:1e30px -1e30px;rotate:1e30deg;scale:1e30}"
        "i:nth-child(3n+1){rotate:1 1 1 1e38deg;scale:-1e-30 1e38}"
        "i:nth-child(3n+2){rotate:0 0 0 45deg;translate:50% 50% 1e9px;scale:0}"
    )
    return Page(doc("<i></i>" * 5000, css))


@case(
    "text-wrap: balance on 300 paragraphs of 1,400 groups each in six long lines (M13 "
    "feature 2a, fix round): the planner works at its bound of 256 groups per line",
    time_limit_s=30,
)
def text_wrap_balance_300_long_paragraphs() -> Page:
    para = "<p>" + " ".join(["ab"] * 1400) + "</p>"
    return Page(doc(para * 300, "p{width:5300px;margin:0;text-wrap:balance}"))


@case(
    "-webkit-text-stroke of 1,000 px, 1e9 px and 8,000 px on 5,000 words each (M13 feature "
    "2a, fix round): the used width is limited to twice the font size, and a width from "
    "8,191 px paints no stroke",
    time_limit_s=30,
)
def text_stroke_huge_width_on_long_text() -> Page:
    words = " ".join(f"word{i}" for i in range(5000))
    css = "p{width:600px;font-size:20px;margin:0}"
    widths = ("1000px", "1e9px", "8000px")
    body = "".join(f"<p style='-webkit-text-stroke:{w} #f00'>{words}</p>" for w in widths)
    return Page(doc(body, css))


@case(
    "1,100,000 distinct lab() colors set by a script (M13 feature 2b, ADR 0046): the table of "
    "wide colors stops at 2^20 entries and later colors fall back to 8-bit sRGB, so memory "
    "stays bounded and the page does not slow down; swb logs the limit once",
    time_limit_s=40,
    rss_limit_mib=2048,
    swb_args=("--js",),
    expect_log="the color table has 1048576 wide colors",
)
def lab_1_100_000_distinct_colors() -> Page:
    script = (
        "var p=document.getElementById('p');"
        "for(var i=0;i<1100000;i++){p.style.color='lab(0 '+i+' 0)';}"
    )
    return Page(doc(f"<p id=p>x</p><script>{script}</script>"))


@case(
    "1,000 elements with backgrounds of 1 px gradient tiles in boxes of 5,000 x 5,000 px (M13 "
    "feature 2b, fix round): a tiled gradient is one display item that the rasterizer "
    "repeats, so the cost does not depend on the number of tiles",
    time_limit_s=30,
)
def gradient_one_pixel_tiles_in_large_boxes() -> Page:
    css = (
        "div{width:5000px;height:5000px;"
        "background:linear-gradient(red,blue) 0 0/1px 1px repeat,"
        "linear-gradient(blue,red) 3px 3px/2px 7px space}"
    )
    return Page(doc("<div></div>" * 1000, css))


@case(
    "60,000 boxes of 100 x 100 px with three gradient layers of 4, 5 and 6 px tiles (M13 "
    "feature 2b, fix round): 1,314 tiles per box, 79 million in all, as 180,000 display "
    "items; one item per tile took 12 GiB",
    time_limit_s=30,
    rss_limit_mib=1536,
)
def gradient_tiles_in_60000_boxes() -> Page:
    css = (
        "div{width:100px;height:100px;"
        "background:linear-gradient(red,blue) 0 0/4px 4px,"
        "linear-gradient(blue,red) 0 0/5px 5px,"
        "linear-gradient(#0f0,#00f) 0 0/6px 6px}"
    )
    return Page(doc("<div></div>" * 60000, css))


@case(
    "`all: unset` in 20,000 rules on 20,000 elements (M13 feature 2b): `all` sets every "
    "longhand, so each rule holds about 270 declarations",
    time_limit_s=30,
    rss_limit_mib=2048,
)
def all_unset_in_20000_rules() -> Page:
    css = "".join(f".c{i}{{all:unset;display:block}}" for i in range(20000))
    body = "".join(f"<i class=c{i}>x</i>" for i in range(20000))
    return Page(doc(body, css))


@case(
    "font-variant-numeric and font-variant-ligatures with 100,000 keywords and with var() "
    "chains (M13 feature 2b): each group may appear once, so the value is invalid",
    time_limit_s=10,
)
def font_variant_many_keywords() -> Page:
    many = " ".join(["tabular-nums"] * 100_000)
    css = (
        f"p{{font-variant-numeric:{many};font-variant-ligatures:{many}}}"
        "q{--a:tabular-nums;--b:var(--a) var(--a);font-variant-numeric:var(--b)}"
        "s{font-variant:" + " ".join(["small-caps"] * 100_000) + "}"
    )
    return Page(doc("<p>1111</p><q>2222</q><s>3333</s>", css))
