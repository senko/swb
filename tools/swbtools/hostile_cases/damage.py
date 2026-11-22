"""The damage of frames, M13 feature 4b (ADR 0044 section 18): each frame compares its
display list with the last one and rasterizes only the damage. The cases run `--bench 1`,
whose animation and hover stages render with damage into a kept pixmap as the GUI does: a
list of 100,000 items with one change per frame (the diff is one pass), 10,000 scattered
boxes that all change each frame (the merge of 10,000 rectangles), a large blur over a small
change (the reach of the filter), 3,000 `backdrop-filter` boxes over changing content (the
boxes that the damage must hold), and groups that appear and go at every item each frame
(the common prefix and suffix). The nesting of 10,000 groups cannot come from a page (box
depth limit); the unit tests of `crates/paint/src/damage.rs` cover it."""

from swbtools.hostile_cases.core import Page, case, doc

BENCH = ("--bench", "1")
"""One run of each stage of `--bench`: the animation stage renders up to 30 frames with
damage."""

FRAMES = '"region_frames"'
"""The bench output has the counts of the frames with damage."""


@case(
    "One animated box among 100,000 items (M13 feature 4b): each frame builds the list "
    "again, compares it with the last one in one pass and rasterizes one small rectangle",
    time_limit_s=30,
    rss_limit_mib=2048,
    swb_args=BENCH,
    expect_log=FRAMES,
)
def damage_one_change_in_100000_items() -> Page:
    css = (
        "i{display:inline-block;width:4px;height:4px;background:#999}"
        "#a{animation:k 1s steps(10) infinite}@keyframes k{to{background:red}}"
    )
    body = "<i id=a></i>" + "<i></i>" * 99_999
    return Page(doc(body, css))


@case(
    "10,000 boxes in scattered places that all change color each frame (M13 feature 4b): "
    "10,000 rectangles of damage, merged on a grid first, then a full raster",
    time_limit_s=30,
    rss_limit_mib=1536,
    swb_args=BENCH,
    expect_log=FRAMES,
)
def damage_10000_scattered_color_changes() -> Page:
    css = (
        "b{position:absolute;width:3px;height:3px;animation:k 0.5s linear infinite}"
        "@keyframes k{from{background:red}to{background:blue}}"
    )
    body = "".join(
        f"<b style='left:{(i * 7919) % 1277}px;top:{(i * 104729) % 797}px'></b>"
        for i in range(10_000)
    )
    return Page(doc(body, css))


@case(
    "A small change under a large blur (M13 feature 4b): the damage grows by the reach of "
    "the filter, and the region raster draws the filter's content where the blur reads it",
    time_limit_s=30,
    swb_args=BENCH,
    expect_log=FRAMES,
)
def damage_large_blur_over_small_change() -> Page:
    css = (
        "#f{position:absolute;left:100px;top:50px;width:1000px;height:700px;"
        "filter:blur(60px);background:linear-gradient(red,blue)}"
        "#a{position:absolute;left:480px;top:330px;width:20px;height:20px;background:#fff;"
        "animation:k 1s linear infinite}@keyframes k{to{background:#000}}"
    )
    return Page(doc("<div id=f><p>text</p><div id=a></div></div>", css))


@case(
    "3,000 `backdrop-filter` boxes over a moving box (M13 feature 4b): every rectangle of "
    "damage that touches a box holds all of it, in a few rounds or a full raster",
    time_limit_s=30,
    rss_limit_mib=1536,
    swb_args=BENCH,
    expect_log=FRAMES,
)
def damage_many_backdrop_filters() -> Page:
    css = (
        "#a{position:absolute;left:0;top:0;width:200px;height:200px;background:red;"
        "animation:k 1s linear infinite}@keyframes k{to{transform:translate(1000px,500px)}}"
        "s{position:absolute;width:18px;height:12px;backdrop-filter:blur(3px) invert(1)}"
    )
    body = "<div id=a></div>" + "".join(
        f"<s style='left:{(i % 60) * 21}px;top:{(i // 60) * 15}px'></s>" for i in range(3_000)
    )
    return Page(doc(body, css))


@case(
    "Opacity groups that appear and go at each of 5,000 boxes in every frame (M13 feature "
    "4b): the kinds of the items differ along the whole list, the diff keeps the common "
    "prefix and suffix of each group and damages the rest",
    time_limit_s=30,
    rss_limit_mib=1536,
    swb_args=BENCH,
    expect_log=FRAMES,
)
def damage_kind_change_every_item() -> Page:
    css = (
        "i{display:inline-block;width:12px;height:12px;background:#36c;"
        "animation:k 0.2s steps(2) infinite}@keyframes k{from{opacity:1}to{opacity:0.5}}"
    )
    return Page(doc("<i></i>" * 5_000, css))
