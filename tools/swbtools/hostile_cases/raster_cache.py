"""The raster cache and composited groups, M13 feature 4c (ADR 0044 section 19): the
coverage of clips and the layers of the transform groups of running `transform` and
`opacity` animations are kept between frames, within a memory limit. The cases run
`--bench 1` (the animation stage renders up to 30 frames with damage, as the GUI does):
many composited groups (more layers than the cache keeps), one layer larger than the
limits of a transform layer, many rounded clips whose content and place change, and a
composited group whose content changes in every frame."""

from swbtools.hostile_cases.core import Page, case, doc

BENCH = ("--bench", "1")
"""One run of each stage of `--bench`."""

CACHE = '"raster_cache"'
"""The bench output has the counts of the raster cache."""


@case(
    "2,000 rotating boxes with rounded corners and text (M13 feature 4c): 2,000 "
    "composited groups, more layers than the raster cache keeps (256 entries, 48 MiB), "
    "so each frame finds some and draws the others again",
    time_limit_s=30,
    rss_limit_mib=1536,
    swb_args=BENCH,
    expect_log=CACHE,
)
def cache_many_composited_groups() -> Page:
    css = (
        "b{display:inline-block;width:24px;height:16px;margin:1px;border-radius:5px;"
        "background:#36c;color:#fff;font:9px sans-serif;overflow:hidden;"
        "animation:k 2s linear infinite}@keyframes k{to{transform:rotate(360deg)}}"
    )
    return Page(doc("<b>ab</b>" * 2_000, css))


@case(
    "A rotating box of 20,000 x 20,000 px (M13 feature 4c): its composited layer would be "
    "larger than the limits of a transform layer, so it is drawn as one each frame",
    time_limit_s=30,
    rss_limit_mib=1536,
    swb_args=BENCH,
    expect_log=CACHE,
)
def cache_one_huge_composited_group() -> Page:
    css = (
        "#a{width:20000px;height:20000px;border-radius:40px;"
        "background:linear-gradient(45deg,red,blue);animation:k 4s linear infinite}"
        "@keyframes k{to{transform:rotate(360deg) scale(1.5)}}"
    )
    return Page(doc("<div id=a>text</div>", css))


@case(
    "1,000 rounded clips that move and whose content changes color each frame (M13 "
    "feature 4c): each coverage misses (a moved clip is another coverage), and no "
    "coverage is kept on its first miss",
    time_limit_s=30,
    rss_limit_mib=1536,
    swb_args=BENCH,
    expect_log=CACHE,
)
def cache_many_rounded_clips_over_a_changing_area() -> Page:
    css = (
        "s{display:inline-block;width:30px;height:20px;margin:2px;border-radius:9px;"
        "overflow:hidden;animation:m 1s linear infinite}"
        "s i{display:block;width:40px;height:30px;animation:c .5s linear infinite}"
        "@keyframes m{to{margin-left:7px}}@keyframes c{from{background:red}"
        "to{background:blue}}"
    )
    return Page(doc("<s><i></i></s>" * 1_000, css))


@case(
    "A large rotating box whose content changes color in every frame (M13 feature 4c): "
    "the composited layer misses in every frame; after three misses in a row a frame "
    "with damage rectangles draws only the part it needs and keeps nothing",
    time_limit_s=30,
    rss_limit_mib=1536,
    swb_args=BENCH,
    expect_log=CACHE,
)
def cache_layer_content_changes_every_frame() -> Page:
    css = (
        "#a{width:900px;height:600px;border-radius:30px;overflow:hidden;background:#eee;"
        "animation:r 8s linear infinite}@keyframes r{to{transform:rotate(360deg)}}"
        "#a p{margin:40px;font:30px sans-serif;animation:c .3s linear infinite}"
        "@keyframes c{from{color:red}to{color:blue}}"
    )
    return Page(doc("<div id=a>" + "<p>changing text</p>" * 6 + "</div>", css))
