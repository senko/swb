"""Frames: the limits and budgets of frames (ADR 0031 section 11)."""

from swbtools.hostile_cases.core import (
    JS,
    MIB,
    Page,
    case,
    doc,
    nest,
    stripes_png,
    web_font,
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
    swb_args=JS,
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
    swb_args=JS,
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
    swb_args=JS,
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
    swb_args=JS,
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
    swb_args=JS,
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
        {"f.html": f"<style>{rules}</style>{text}", "f.ttf": web_font()},
    )
