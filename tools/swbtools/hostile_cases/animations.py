"""CSS animations, the model of M13 feature 3a (ADR 0044 section 11): the `animation-*`
properties with huge lists, `linear()` with 100,000 points, `steps()` with huge counts,
100,000 `@keyframes` names and one rule with 100,000 keyframes. Chromium has no limits for
these (`hostile-limits` in tools/probes/css-animations.json); swb parses, cascades and
serializes them in linear time. Feature 3b adds the cases that run animations."""

from swbtools.hostile_cases.core import JS, Page, case, doc

READ_STYLES = (
    "<script>const s = getComputedStyle(document.querySelector('#a'));"
    "const n = [s.animation, s.animationName, s.animationTimingFunction].join('').length;"
    "document.querySelector('#a').style.animation;"
    "console.warn('animation text length ' + n);</script>"
)
"""A script that reads the computed animation values (their serialization)."""


@case(
    "linear() with 100,000 points in animation-timing-function (ADR 0044 section 11): no point "
    "limit; the inputs are filled in one pass, the text is written once per read",
    time_limit_s=20,
    swb_args=JS,
    expect_log="animation text length",
)
def animation_linear_100000_points() -> Page:
    points = ", ".join(f"{(i * 7919) % 1000 / 1000} {i / 1000:.3f}%" for i in range(100_000))
    css = f"#a{{animation:a 1s linear({points}) infinite}}@keyframes a{{to{{opacity:0}}}}"
    return Page(doc("<div id=a>x</div>" + READ_STYLES, css))


@case(
    "An animation list of 10,000 entries (ADR 0044 section 11): every longhand is a list of "
    "10,000 items; the computed shorthand repeats the lists to the names",
    time_limit_s=20,
    swb_args=JS,
    expect_log="animation text length",
)
def animation_list_10000_entries() -> Page:
    layers = ", ".join(
        f"k{i % 97} {i % 9 + 1}s steps({i % 5 + 1}) {i % 3}s {i % 4 + 1} alternate both"
        for i in range(10_000)
    )
    css = f"#a{{animation:{layers}}}div{{animation-duration:1s,2s,3s}}"
    return Page(doc("<div id=a>x</div>" + "<div>y</div>" * 100 + READ_STYLES, css))


@case(
    "100,000 @keyframes names (ADR 0044 section 11): the stylist keeps each rule once and "
    "indexes the names at the first lookup",
    time_limit_s=20,
    rss_limit_mib=1536,
    swb_args=JS,
    expect_log="animation text length",
)
def animation_100000_keyframes_names() -> Page:
    rules = "".join(f"@keyframes k{i}{{to{{opacity:{i % 10 / 10}}}}}" for i in range(100_000))
    css = rules + "#a{animation:k99999 1s}"
    return Page(doc("<div id=a>x</div>" + READ_STYLES, css))


@case(
    "steps(1000000) and steps(2147483648) (ADR 0044 section 11): the count is clamped to "
    "2^31 - 1 and evaluated in O(1)",
    swb_args=JS,
    expect_log="animation text length",
)
def animation_steps_huge_counts() -> Page:
    css = (
        "#a{animation:a 1s steps(1000000) infinite, b 2s steps(2147483648, jump-both) both}"
        "@keyframes a{to{opacity:0}}@keyframes b{to{transform:translateX(100px)}}"
    )
    return Page(doc("<div id=a>x</div>" + READ_STYLES, css))


@case(
    "One @keyframes rule with 100,000 keyframes (ADR 0044 section 11): the keyframes parse "
    "once; a set merges duplicates through a hash map (feature 3b builds it)",
    time_limit_s=20,
    rss_limit_mib=1536,
    swb_args=JS,
    expect_log="animation text length",
)
def animation_one_rule_100000_keyframes() -> Page:
    frames = "".join(
        f"{i / 1000:.3f}%{{left:{i % 50}px;animation-timing-function:steps({i % 3 + 1})}}"
        for i in range(100_000)
    )
    css = f"@keyframes big{{{frames}}}#a{{position:relative;animation:big 10s infinite}}"
    return Page(doc("<div id=a>x</div>" + READ_STYLES, css))


@case(
    "Infinite, NaN and huge numbers in every animation value (ADR 0044 section 14, "
    "`number-ranges`): literals saturate to the f32 range, infinite math functions to the "
    "largest f64, NaN is 0; the easing outputs and the timing stay finite",
    swb_args=JS,
    expect_log="animation text length",
)
def animation_infinite_numbers() -> Page:
    layers = ", ".join(
        [
            "a calc(1s/0) cubic-bezier(0, calc(1/0), 1, calc(-1/0)) calc(-1s/0) calc(1/0)",
            "b 1e999s linear(calc(1/0), calc(-1/0) calc(1%/0), 1e999) calc(NaN * 1s) 1e999",
            "c calc(0s/0) steps(calc(0/0), jump-both) -1e999s calc(0/0)",
            "d calc(infinity * 1s) steps(calc(1/0)) calc(-infinity * 1s) calc(-1/0)",
        ]
    )
    css = (
        f"#a{{animation:{layers}}}"
        "@keyframes a{to{left:1e999px;transform:translate(-50%) rotate(1e30deg)}}"
        "@keyframes b{50%{opacity:calc(1/0)}}"
    )
    return Page(doc("<div id=a>x</div>" + READ_STYLES, css))
