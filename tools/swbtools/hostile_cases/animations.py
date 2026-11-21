"""CSS animations, the model of M13 feature 3a (ADR 0044 section 11): the `animation-*`
properties with huge lists, `linear()` with 100,000 points, `steps()` with huge counts,
100,000 `@keyframes` names and one rule with 100,000 keyframes. Chromium has no limits for
these (`hostile-limits` in tools/probes/css-animations.json); swb parses, cascades and
serializes them in linear time. Feature 3b adds the cases that run animations: 10,000
animated elements, an infinite animation of a 30,000 px element in a full-page screenshot,
animated custom properties through 1,000 levels of inheritance, more than 100,000
animations in a document (the record limit) and a registered syntax with many alternatives
and a long animated value."""

from swbtools.hostile_cases.core import JS, Page, case, doc, nest

RUNNING = ("--js", "--virtual-time", "--settle-ms", "3000")
"""Scripts and 3 s of virtual time: the animations run (one update per iteration)."""

COUNT_ANIMATIONS = (
    "<script>setTimeout(() => console.warn('animations ' + document.getAnimations().length"
    " + ' ' + getComputedStyle(document.querySelector('#a')).opacity), 2500);</script>"
)
"""A script that counts the animations and reads an animated value after 2.5 s."""

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


@case(
    "10,000 animated elements (ADR 0044 section 11): one record each; an update is O(records), "
    "a restyle O(marked elements); in headless mode updates run at the iteration ends only",
    time_limit_s=20,
    swb_args=RUNNING,
    expect_log="animations 10000",
)
def animation_running_10000_elements() -> Page:
    css = (
        "div{width:10px;height:10px;display:inline-block;animation:k 1s linear infinite}"
        "div:nth-child(3n){animation-duration:700ms}"
        "@keyframes k{from{opacity:0;transform:translateX(0)}to{opacity:1;"
        "transform:translateX(5px)}}"
    )
    body = "<div id=a></div>" + "<div></div>" * 9_999 + COUNT_ANIMATIONS
    return Page(doc(body, css))


@case(
    "An infinite animation of a 30,000 px high element in a full-page screenshot (ADR 0044 "
    "section 11): the screenshot catches up to the frame grid once and rasterizes in strips",
    time_limit_s=20,
    rss_limit_mib=1536,
    swb_args=(*RUNNING, "--full-page"),
    expect_log="animations 1",
)
def animation_infinite_30000px_full_page() -> Page:
    css = (
        "#a{height:30000px;background:linear-gradient(red,blue);"
        "animation:k 900ms ease-in-out infinite alternate}"
        "@keyframes k{from{opacity:.2;transform:translateY(0)}to{opacity:1;"
        "transform:translateY(40px) rotate(1deg)}}"
    )
    return Page(doc("<div id=a></div>" + COUNT_ANIMATIONS, css))


@case(
    "Animated custom properties through 1,000 levels of inheritance (ADR 0044 section 11): "
    "the root's animated values enter its custom property map; the descendants inherit the "
    "map and substitute `var()` once each",
    time_limit_s=20,
    swb_args=RUNNING,
    expect_log="animations 2",
)
def animation_custom_properties_1000_levels() -> Page:
    css = (
        "@property --w{syntax:'<length>';inherits:true;initial-value:0px}"
        "#a{animation:w 1s linear infinite,u 1s steps(4) infinite}"
        "div div{padding-left:calc(var(--w) / 1000);border-left:1px solid var(--c,red)}"
        "@keyframes w{from{--w:0px}to{--w:100px}}"
        "@keyframes u{from{--c:red}to{--c:blue}}"
    )
    body = nest("<div>", "</div>", 1_000, "x")
    return Page(doc(f"<div id=a>{body}</div>" + COUNT_ANIMATIONS, css))


@case(
    "More than 100,000 animations in a document (ADR 0044 section 11): at most 100,000 "
    "records; beyond, none is created (one warning)",
    time_limit_s=30,
    rss_limit_mib=2048,
    swb_args=RUNNING,
    expect_log=("animations: the document has 100000 animations", "animations 100000"),
)
def animation_record_limit() -> Page:
    names = ", ".join(f"k{i % 5}" for i in range(101))
    css = (
        f"div{{animation-name:{names};animation-duration:1s;animation-iteration-count:infinite}}"
        "@keyframes k0{to{opacity:.5}}@keyframes k1{to{opacity:.6}}@keyframes k2{to{opacity:.7}}"
        "@keyframes k3{to{opacity:.8}}@keyframes k4{to{opacity:.9}}"
    )
    body = "<div id=a></div>" + "<div></div>" * 999 + COUNT_ANIMATIONS
    return Page(doc(body, css))


LONG_RUN = ("--js", "--virtual-time", "--settle-ms", "100000")
"""A script chain of 20,000 timers runs to its end (80 s of virtual time)."""

TIMER_CHAIN = (
    "<script>let n=0;function f(){if(++n<20000)setTimeout(f,0);else console.warn('done '+n);}"
    "f();</script>"
)
"""20,000 tasks, one after another, that read no style."""


@case(
    "One element with 50,000 animation names, running (review of 3b; ADR 0044 section 11): "
    "the records of an element are matched with its names through a map, so a restyle costs "
    "O(names), not O(names squared)",
    time_limit_s=10,
    swb_args=RUNNING,
    expect_log="n 50000",
)
def animation_names_50000_on_one_element() -> Page:
    names = ", ".join(f"k{i % 7}" for i in range(50_000))
    keyframes = "".join(f"@keyframes k{i}{{to{{opacity:.{i + 2}}}}}" for i in range(7))
    css = (
        f"#a{{width:10px;height:10px;animation-name:{names};animation-duration:1s;"
        f"animation-iteration-count:infinite}}{keyframes}"
    )
    body = (
        "<div id=a></div><script>setTimeout(() => console.warn('n ' + "
        "document.getAnimations().length), 2500)</script>"
    )
    return Page(doc(body, css))


def _loop_page(paused: bool) -> Page:
    state = " paused" if paused else ""
    css = (
        "div{width:10px;height:10px;display:inline-block;"
        f"animation:k 1s linear infinite{state}}}@keyframes k{{from{{opacity:0}}to{{opacity:1}}}}"
    )
    return Page(doc("<div></div>" * 20_000 + TIMER_CHAIN, css))


@case(
    "20,000 infinite animations and a chain of 20,000 `setTimeout(f, 0)` that read no style "
    "(review of 3b; ADR 0044 section 8): the next time of the animations is cached and the "
    "catch-up to the frame grid runs only when something reads the styles, so a task costs "
    "O(1), not O(animations)",
    time_limit_s=10,
    swb_args=LONG_RUN,
    expect_log="done 20000",
)
def animation_loop_20000_animations_20000_timers() -> Page:
    return _loop_page(False)


@case(
    "The same with the 20,000 animations paused (review of 3b): a paused animation needs no "
    "update, and the cached next time says so",
    time_limit_s=10,
    swb_args=LONG_RUN,
    expect_log="done 20000",
)
def animation_loop_20000_paused_20000_timers() -> Page:
    return _loop_page(True)


@case(
    "`getAnimations()` of each of 20,000 animated elements (review of 3b): one element without "
    "`subtree` reads its own records, so the calls cost O(1) each, not O(owners)",
    time_limit_s=10,
    swb_args=JS,
    expect_log="per-element getAnimations 20000",
)
def animation_get_animations_20000_elements() -> Page:
    css = (
        "div{width:10px;height:10px;display:inline-block;animation:k 1s linear infinite}"
        "@keyframes k{from{opacity:0}to{opacity:1}}"
    )
    script = (
        "<script>let n=0;for(const d of document.querySelectorAll('div'))n+=d.getAnimations()"
        ".length;console.warn('per-element getAnimations '+n);</script>"
    )
    return Page(doc("<div></div>" * 20_000 + script, css))


@case(
    "A registered custom property whose syntax has 255 alternatives, animated with a value of "
    "10,000 lengths (3a's review; ADR 0044 section 14): the interpolation matches both values "
    "against the terms, so the cost is their product; one restyle per iteration",
    time_limit_s=30,
    swb_args=RUNNING,
    expect_log="animations 1",
)
def animation_registered_syntax_many_alternatives() -> Page:
    terms = " | ".join(f"k{i}" for i in range(254))
    count = 10_000
    start = " ".join(f"{i % 50}px" for i in range(count))
    end = " ".join(f"{i % 30}px" for i in range(count))
    css = (
        f"@property --l{{syntax:'{terms} | <length>+';inherits:false;initial-value:0px}}"
        f"#a{{animation:l 1s linear infinite}}@keyframes l{{from{{--l:{start}}}to{{--l:{end}}}}}"
    )
    return Page(doc("<div id=a>x</div>" + COUNT_ANIMATIONS, css))
