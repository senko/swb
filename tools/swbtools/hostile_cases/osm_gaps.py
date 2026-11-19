"""`ImageData`, the `Worker` stand-in, a GIF without a color table (M12 feature 2), the hover update
after the content under the pointer changes (M12 feature 3a) and blend groups in rounded clips
(M12 feature 4d)."""

import base64
import json

from swbtools.hostile_cases.core import REPLAY_URL, Page, case, doc, nest, steps_args

JS = ("--js", "--settle-ms", "500")


@case(
    "`new ImageData` with sizes past the limits (50,000 x 50,000, 1e6 x 1e6, 2^31 x 1, 16,384 x "
    "16,384 and 9,000 x 9,000, which do not fit the 256 MiB script heap) and a loop of 500 "
    "failed allocations: `IndexSizeError` or `RangeError` at once, no allocation before the "
    "check, and a script logs `image data ok` (M12 feature 2)",
    time_limit_s=20,
    rss_limit_mib=1024,
    expect_log="image data ok",
    swb_args=JS,
)
def image_data_huge_sizes() -> Page:
    script = (
        "<script>window.onload = () => { const names = []; "
        "for (const [w, h] of [[50000, 50000], [1e6, 1e6], [2 ** 31, 1], [16384, 16384], "
        "[9000, 9000], [NaN, 1], [-1, -1]]) { "
        "try { new ImageData(w, h); names.push('ok'); } catch (e) { names.push(e.name); } } "
        "for (let i = 0; i < 500; i++) { try { new ImageData(16384, 16384); } catch (e) {} } "
        "console.warn('image data ok ' + names.join() + ' ' + new ImageData(64, 64).data.length); "
        "};</script>"
    )
    return Page(doc("<p>x" + script))


@case(
    "10,000 `new Worker` stand-ins that each get a message of 300 objects (and a "
    "transferred `ArrayBuffer`): each message is serialized and dropped, nothing "
    "waits for an answer, and a script logs `workers ok` (M12 feature 2)",
    time_limit_s=30,
    rss_limit_mib=1024,
    expect_log="workers ok",
    swb_args=JS,
)
def workers_10000_with_large_messages() -> Page:
    script = (
        "<script>window.onload = () => { "
        "const big = {a: Array.from({length: 300}, (_, i) => ({i, s: 'x'.repeat(40)}))}; "
        "let n = 0; "
        "for (let i = 0; i < 10000; i++) { "
        "const w = new Worker('data:text/javascript,1'); "
        "const b = new ArrayBuffer(1024); w.postMessage(big, [b]); "
        "if (b.byteLength === 0) n++; } "
        "console.warn('workers ok ' + n); };</script>"
    )
    return Page(doc("<p>x" + script))


def gif_without_table(width: int, height: int) -> str:
    """A data URL of a GIF whose screen is `width` x `height`, with one frame that has no
    color table and no pixel data (the shape of Leaflet's `emptyImageUrl`)."""
    header = b"GIF89a" + width.to_bytes(2, "little") + height.to_bytes(2, "little") + b"\0\0\0"
    frame = b"\x2c\0\0\0\0" + width.to_bytes(2, "little") + height.to_bytes(2, "little") + b"\0"
    data = header + frame + b"\x02\0\x3b"
    return "data:image/gif;base64," + base64.b64encode(data).decode()


@case(
    "a GIF without a color table whose logical screen is 65,535 x 65,535 (4.3 Gpx), one of "
    "16,000 x 16,000 and one of 4,000 x 4,000: the size is checked from the header before "
    "anything is allocated, so the two large ones are refused (`image too large`), and the "
    "last one is a 64 MB transparent image (M12 feature 2)",
    time_limit_s=20,
    rss_limit_mib=1024,
    expect_log=("image too large", "gif ok"),
    swb_args=JS,
)
def gif_without_color_table_large_screen() -> Page:
    images = "".join(
        f"<img src='{gif_without_table(w, h)}' width=10 height=10>"
        for w, h in [(65535, 65535), (16000, 16000), (4000, 4000)]
    )
    script = (
        "<script>window.onload = () => console.warn('gif ok ' + "
        "[...document.images].map((i) => i.naturalWidth).join());</script>"
    )
    return Page(doc(images + script))


HOVER_STYLE = (
    "<style>body{margin:0}#box{position:absolute;left:20px;top:20px;width:200px}"
    "a{display:block;height:40px}.rev{display:flex;flex-direction:column-reverse}</style>"
)


@case(
    "a page whose `mouseover` handler moves the hovered link away (a class turns the two links "
    "around), so the next hover update hovers the other link, whose handler moves it back, "
    "with the pointer still for 3 s: one hover update per rendering update, not a loop in one "
    "update (M12 feature 3a)",
    time_limit_s=20,
    rss_limit_mib=1024,
    expect_log="hover loop ok true",
    swb_args=steps_args([{"hover": [60, 30]}, {"wait": 3000}]),
)
def hover_update_ping_pong() -> Page:
    script = (
        "<script>let n = 0; const box = document.getElementById('box'); "
        "document.addEventListener('mouseover', () => { n++; box.classList.toggle('rev'); }, "
        "true); "
        "setTimeout(() => console.warn('hover loop ok ' + (n < 1000)), 2500);</script>"
    )
    return Page(doc(HOVER_STYLE + "<div id=box><a href=#a>a</a><a href=#b>b</a></div>" + script))


@case(
    "a page that replaces the link under the pointer in every animation frame for 3 s: one "
    "hit test per rendering update and the boundary events of each (M12 feature 3a)",
    time_limit_s=20,
    rss_limit_mib=1024,
    expect_log="hover replace ok true",
    swb_args=steps_args([{"hover": [60, 30]}, {"wait": 3000}]),
)
def hover_update_replace_every_frame() -> Page:
    script = (
        "<script>let n = 0, frames = 0; const box = document.getElementById('box'); "
        "document.addEventListener('mouseover', () => { n++; }, true); "
        "const tick = () => { frames++; box.innerHTML = '<a href=#>y</a>'; "
        "requestAnimationFrame(tick); }; requestAnimationFrame(tick); "
        "setTimeout(() => console.warn('hover replace ok ' + (n <= frames + 2)), 2500);</script>"
    )
    return Page(doc(HOVER_STYLE + "<div id=box><a href=#>x</a></div>" + script))


def replay_text_page(html: str, text: str = "hello") -> Page:
    """A replayed page at `http://hostile.test/index.html` with the text file `x.txt`."""
    entries = [
        {
            "method": "GET",
            "url": REPLAY_URL,
            "status": 200,
            "headers": [["content-type", "text/html; charset=utf-8"]],
            "body": "files/index.html",
        },
        {
            "method": "GET",
            "url": "http://hostile.test/x.txt",
            "status": 200,
            "headers": [["content-type", "text/plain"]],
            "body": "files/x.txt",
        },
    ]
    files: dict[str, str | bytes] = {"files/index.html": html, "files/x.txt": text}
    files["manifest.json"] = json.dumps({"version": 1, "entries": entries})
    return Page(html, files)


CLICK_STEPS = json.dumps([{"click": [20, 20]}, {"wait": 2000}])
BUTTON = "<button id=b style='width:100px;height:40px'>go</button>"
FETCH_ARGS = ("--js", "--virtual-time", "--replay", "{dir}", f"--steps={CLICK_STEPS}")


@case(
    "a chain of 10,000 `fetch()` calls, each started in the completion of the one before, the "
    "first in a click handler, with a replayed fixture: only the first completion waits for a "
    "rendering update in virtual time, the rest follow at once, so the chain ends within the "
    "time limit (M12 feature 3c)",
    time_limit_s=20,
    rss_limit_mib=1024,
    expect_log="chain done 10000",
    swb_args=FETCH_ARGS,
    url=REPLAY_URL,
)
def fetch_chain_10000_from_click() -> Page:
    script = (
        "<script>let n = 0; const next = () => { n++; "
        "if (n < 10000) fetch('x.txt').then((r) => r.text()).then(next); "
        "else console.warn('chain done ' + n); }; "
        "b.addEventListener('click', () => { fetch('x.txt').then((r) => r.text()).then(next); });"
        "</script>"
    )
    return replay_text_page(doc(BUTTON + script))


@case(
    "10,000 `fetch()` calls started at once in a click handler, with a replayed fixture: the "
    "completions wait for the next rendering update in virtual time, are handled in order "
    "after it, and the page counts all of them (M12 feature 3c)",
    time_limit_s=20,
    rss_limit_mib=2048,
    expect_log="all done 10000",
    swb_args=FETCH_ARGS,
    url=REPLAY_URL,
)
def fetch_10000_at_once_from_click() -> Page:
    script = (
        "<script>let n = 0; b.addEventListener('click', () => { "
        "for (let i = 0; i < 10000; i++) fetch('x.txt').then((r) => r.text()).then(() => { "
        "if (++n === 10000) console.warn('all done ' + n); }, () => { "
        "if (++n === 10000) console.warn('all done ' + n); }); });</script>"
    )
    return replay_text_page(doc(BUTTON + script))


@case(
    "a loop that starts a `fetch()` in every animation frame (400 frames), and a click that "
    "starts 50 more, with a replayed fixture: one completion per request, none lost while "
    "some wait for a rendering update (M12 feature 3c)",
    time_limit_s=20,
    rss_limit_mib=1024,
    expect_log="frames ok true",
    swb_args=(
        "--js",
        "--virtual-time",
        "--replay",
        "{dir}",
        f"--steps={json.dumps([{'click': [20, 20]}, {'wait': 9000}])}",
    ),
    url=REPLAY_URL,
)
def fetch_in_every_frame() -> Page:
    script = (
        "<script>let started = 0, done = 0; const tick = () => { started++; "
        "fetch('x.txt').then((r) => r.text()).then(() => { done++; }); "
        "if (started < 400) requestAnimationFrame(tick); }; requestAnimationFrame(tick); "
        "b.addEventListener('click', () => { for (let i = 0; i < 50; i++) "
        "fetch('x.txt').then((r) => r.text()).then(() => { done++; }); started += 50; }); "
        "setTimeout(() => console.warn('frames ok ' + (done === started)), 8500);</script>"
    )
    return replay_text_page(doc(BUTTON + script))


@case(
    "4,000 overlapping boxes of 200 x 200 px in the viewport, each with a rounded `overflow` "
    "clip (a clip layer) that holds a `mix-blend-mode: difference` box: every blend group "
    "blends through its clip layer (`blend_through_clip_layers`) within the layer budget (M12 "
    "feature 4d)",
    time_limit_s=10,
    rss_limit_mib=2048,
)
def blend_in_4000_rounded_clips() -> Page:
    css = (
        ".p{position:absolute;overflow:hidden;border-radius:12px;width:200px;height:200px;"
        "background:#4a4}.b{height:200px;background:#e80;mix-blend-mode:difference}"
    )
    boxes = "".join(
        f"<div class=p style='left:{(i * 37) % 1000}px;top:{(i * 53) % 600}px'>"
        "<div class=b></div></div>"
        for i in range(4_000)
    )
    return Page(doc(boxes, css))


@case(
    "500 nested rounded `overflow` clips (a layer each) with a `mix-blend-mode` box in the "
    "innermost one: the layer budget ends the nesting without a panic, and the clips that fit "
    "blend (M12 feature 4d)",
    time_limit_s=10,
    rss_limit_mib=2048,
)
def blend_in_500_nested_rounded_clips() -> Page:
    css = (
        ".c{overflow:hidden;border-radius:20px;padding:1px;background:rgba(0,0,200,.05)}"
        ".b{mix-blend-mode:difference;background:#e80;height:200px}"
    )
    inner = nest("<div class=c>", "</div>", 500, "<div class=b></div>")
    return Page(doc(inner, css))
