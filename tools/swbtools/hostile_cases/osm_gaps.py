"""`ImageData`, the `Worker` stand-in and a GIF without a color table (M12 feature 2)."""

import base64

from swbtools.hostile_cases.core import Page, case, doc

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
