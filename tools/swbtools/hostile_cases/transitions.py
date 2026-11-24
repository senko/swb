"""CSS transitions, M13 feature 5 (ADR 0044 section 11; videojs-diagnosis section 7, item 8):
a `transitionend` listener that sets the property back (ping-pong), a class change that
starts transitions on 10,000 elements, a `transition` list of 10,000 entries (on one
element, and of custom properties on 3,000 elements whose inherited color changes), a property
toggled every frame for 1,000 frames (each toggle reverses the running transition), and an
inherited `color` that starts transitions on 100,000 elements through one class on the
body. Chromium has no limits for these; swb bounds the records (100,000 per page), and
events run only in rendering updates."""

from swbtools.hostile_cases.core import Page, case, doc

RUNNING = ("--js", "--virtual-time", "--settle-ms", "3000")
"""Scripts and 3 s of virtual time: the transitions run."""


@case(
    "A `transitionend` listener that sets the property back (ping-pong) with a 1 ms transition "
    "(M13 feature 5; ADR 0044 section 11): events are dispatched only in rendering updates, so "
    "the loop runs at most once per frame (at most 180 rounds in 3 s of virtual time)",
    time_limit_s=10,
    swb_args=RUNNING,
    expect_log="ping-pong bounded",
)
def transition_end_ping_pong() -> Page:
    css = "#a{opacity:1;transition:opacity 1ms linear}#a.on{opacity:0}"
    script = (
        "<script>let n=0;const e=document.querySelector('#a');"
        "e.addEventListener('transitionend',()=>{n++;e.classList.toggle('on');});"
        "getComputedStyle(e).opacity;e.classList.add('on');"
        "setTimeout(()=>console.warn('ping-pong '"
        "+(n>0&&n<=200?'bounded ':'unbounded ')+n),2500);"
        "</script>"
    )
    return Page(doc("<div id=a>x</div>" + script, css))


@case(
    "A class change that starts transitions of four properties on 10,000 elements (M13 "
    "feature 5): one style change event; 40,000 records, then their events and their frames",
    time_limit_s=40,
    rss_limit_mib=1536,
    swb_args=RUNNING,
    expect_log="transitions 40000",
)
def transition_class_change_10000_elements() -> Page:
    css = (
        "div{width:10px;height:10px;display:inline-block;background-color:#eee;opacity:1;"
        "transition:background-color 1s,opacity 1s,transform 1s,width 1s}"
        "body.on div{background-color:#333;opacity:.5;transform:translateX(2px);width:12px}"
    )
    script = (
        "<script>getComputedStyle(document.querySelector('div')).opacity;"
        "document.body.classList.add('on');"
        "console.warn('transitions '+document.getAnimations().length);"
        "setTimeout(()=>console.warn('later '+document.getAnimations().length),2500);</script>"
    )
    return Page(doc("<div></div>" * 10_000 + script, css))


@case(
    "A `transition` list of 10,000 entries (M13 feature 5): the matching item of each "
    "property is the last one that names it; the lists are read once per changed property",
    time_limit_s=20,
    swb_args=RUNNING,
    expect_log="transition text length",
)
def transition_list_10000_entries() -> Page:
    layers = ", ".join(
        f"{'opacity' if i % 7 == 0 else 'width' if i % 7 == 1 else f'--p{i}'} "
        f"{i % 9 + 1}s steps({i % 5 + 1}) {i % 3}s"
        for i in range(10_000)
    )
    css = f"#a{{opacity:1;width:10px;transition:{layers}}}#a.on{{opacity:0;width:20px}}"
    script = (
        "<script>const a=document.querySelector('#a');getComputedStyle(a).opacity;"
        "a.classList.add('on');const s=getComputedStyle(a);"
        "const n=[s.transition,s.transitionProperty,s.transitionTimingFunction].join('').length;"
        "console.warn('transition text length '+n+' '+document.getAnimations().length);</script>"
    )
    return Page(doc("<div id=a>x</div>" + script, css))


@case(
    "A `transition` list of 10,000 custom properties on 3,000 elements and one class change "
    "on the body that changes their inherited `color` (M13 feature 5, fix round): each "
    "element's style change event compares the custom properties once, not per item, and "
    "the elements share the computed lists",
    time_limit_s=20,
    rss_limit_mib=1024,
    swb_args=("--js",),
    expect_log="list on many elements",
)
def transition_list_10000_entries_3000_elements() -> Page:
    layers = ",".join(f"--p{i} 1s" for i in range(10_000))
    css = f"div{{transition:{layers}}}body.on{{color:red}}"
    script = (
        "<script>getComputedStyle(document.querySelector('div')).color;"
        "document.body.classList.add('on');"
        "console.warn('list on many elements '"
        "+getComputedStyle(document.querySelector('div')).color"
        "+' '+document.getAnimations().length);</script>"
    )
    return Page(doc("<div>x</div>" * 3_000 + script, css))


@case(
    "A property toggled every 16 ms for 1,000 rounds while its transition runs (M13 feature "
    "5): each toggle reverses the running transition (a cancel and a new record); the number "
    "of live records stays one",
    time_limit_s=30,
    swb_args=("--js", "--virtual-time", "--settle-ms", "20000"),
    expect_log="toggles 1000",
)
def transition_toggle_every_frame_1000() -> Page:
    css = (
        "#a{opacity:1;transform:none;transition:opacity 1s,transform 1s}"
        "#a.on{opacity:0;transform:translateX(50px)}"
    )
    script = (
        "<script>let i=0;const a=document.querySelector('#a');getComputedStyle(a).opacity;"
        "function f(){a.classList.toggle('on');getComputedStyle(a).opacity;"
        "if(++i<1000)setTimeout(f,16);"
        "else console.warn('toggles '+i+' live '+document.getAnimations().length);}"
        "setTimeout(f,16);</script>"
    )
    return Page(doc("<div id=a>x</div>" + script, css))


@case(
    "An inherited `color` change through one class on the body that starts a transition on "
    "each of 100,000 elements (M13 feature 5; ADR 0044 section 11): the record limit "
    "(100,000 per page) bounds them; the frames are paint-only",
    time_limit_s=40,
    rss_limit_mib=2048,
    swb_args=RUNNING,
    expect_log="inherited colors",
)
def transition_inherited_color_100000_elements() -> Page:
    css = "p{margin:0;transition:color 1s linear}body.on{color:red}"
    script = (
        "<script>getComputedStyle(document.querySelector('p')).color;"
        "document.body.classList.add('on');"
        "setTimeout(()=>console.warn('inherited colors '"
        "+getComputedStyle(document.querySelector('p')).color),500);</script>"
    )
    return Page(doc("<p>text</p>" * 100_000 + script, css))
