"""Container queries: the style and layout passes and their bound, the query
container lookup, container units (ADR 0038 section 8, sub-feature 7a)."""

from swbtools.hostile_cases.core import Page, case, doc, nest

JS = ("--js", "--settle-ms", "500")

BOUND_LOG = "extra passes did not settle"


@case(
    "1,000 nested containers, each 300 px wide when its parent container is wider than "
    "400 px and 500 px otherwise: the query results settle one level per pass, so the "
    "layout flush stops at the bound of 4 extra style and layout passes and keeps the last "
    "styles (ADR 0038 section 4)",
    time_limit_s=20,
    expect_log=BOUND_LOG,
)
def container_chain_1000_nested() -> Page:
    css = (
        ".c{container-type:inline-size;width:500px} "
        "@container (width > 400px) { .c { width: 300px } }"
    )
    body = nest("<div class=c>", "</div>", 1000, "x")
    return Page(doc("<div class=c style='width:1000px'>" + body + "</div>", css))


@case(
    "1,000 nested containers whose query toggles the `display` of the next container between "
    "`block` and `inline-block` (and its width) at each pass, with a container unit in each "
    "level: the passes stop at the bound (ADR 0038 section 4)",
    time_limit_s=20,
    expect_log=BOUND_LOG,
)
def container_display_toggle_1000() -> Page:
    css = (
        ".c{container-type:inline-size;width:500px;padding-left:1cqw} "
        "@container (width > 400px) { .c { display: inline-block; width: 300px } }"
    )
    body = nest("<div class=c>", "</div>", 1000, "x")
    return Page(doc("<div class=c style='width:1000px'>" + body + "</div>", css))


@case(
    "100,000 sibling containers with a query each that is true at the real size: one extra "
    "style and layout pass on the first rendering, and a script logs `siblings ok` after a "
    "forced layout reads a computed style inside the last container (ADR 0038 section 8)",
    time_limit_s=30,
    expect_log="siblings ok",
    swb_args=JS,
)
def container_queries_100000_siblings() -> Page:
    css = (
        ".c{container-type:inline-size;width:200px;height:2px} .t{color:red} "
        "@container (width > 100px) { .t { color: green } }"
    )
    script = (
        "<script>window.onload = () => { const t = document.querySelectorAll('.t'); "
        "const c = getComputedStyle(t[t.length - 1]).color; "
        "console.warn('siblings ok ' + c); };</script>"
    )
    return Page(doc("<div class=c><i class=t></i></div>" * 100000 + script, css))


@case(
    "50,000 nested containers and a query whose container name is on the outermost one: the "
    "lookup is memoized per container and condition, so the style pass stays linear; the box "
    "tree flattens below its depth limit, and a script logs `deep ok` (ADR 0038 section 8)",
    time_limit_s=30,
    expect_log="deep ok",
    swb_args=JS,
)
def container_named_lookup_50000_deep() -> Page:
    css = (
        ".c{container-type:inline-size} #top{container:top/inline-size;width:600px} "
        "@container top (width > 500px) { .c, #leaf { color: green } }"
    )
    body = nest("<div class=c>", "</div>", 50000, "<i id=leaf>x</i>")
    script = (
        "<script>window.onload = () => console.warn('deep ok ' + "
        "getComputedStyle(document.getElementById('leaf')).color);</script>"
    )
    return Page(doc("<div id=top>" + body + "</div>" + script, css))


@case(
    "a script that changes a container's width and reads a computed style inside it 10,000 "
    "times: each read forces a layout with its container passes (each bounded), and a timer "
    "logs `reads ok` (ADR 0038 section 6)",
    time_limit_s=30,
    expect_log="reads ok",
    swb_args=JS,
)
def container_width_read_loop_10000() -> Page:
    css = (
        "#c{container-type:inline-size} .t{color:red} "
        "@container (width > 150px) { .t { color: green } }"
    )
    script = (
        "<script>const c = document.getElementById('c'), t = document.getElementById('t'); "
        "let green = 0; for (let i = 0; i < 10000; i++) { c.style.width = (100 + i % 100) + 'px'; "
        "if (getComputedStyle(t).color === 'rgb(0, 128, 0)') green++; } "
        "setTimeout(() => console.warn('reads ok ' + green), 10);</script>"
    )
    return Page(doc("<div id=c><p class=t id=t>x</p></div>" + script, css))
