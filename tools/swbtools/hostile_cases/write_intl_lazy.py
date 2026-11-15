"""`document.write`, the `Intl` formatters and lazy-loading images (ADR 0028)."""

from swbtools.hostile_cases.core import (
    JS,
    Page,
    case,
    doc,
    nest,
)


@case(
    "A script that writes itself with JavaScript on (M9 feature 2): each copy runs inside the "
    "write of the one before. Nesting stops at script nesting level 22 as in Chromium (the "
    "write of that script is ignored with a warning); the page loads (ADR 0028 section 8)",
    expect_log="the script nesting level",
    swb_args=JS,
)
def document_write_itself_js_on() -> Page:
    script = (
        "<script>window.n = (window.n || 0) + 1; "
        "document.write('<script>' + document.currentScript.text + '<\\/script>'); "
        "if (window.n > 1) console.log('depth ' + window.n);</script>"
    )
    return Page(doc("<p>write</p>" + script))


@case(
    "A script that writes two copies of itself with JavaScript on (M9 feature 2): the copies "
    "double at each of the 21 levels of nesting, so the page would get 2^21 scripts; the "
    "bound of 100,000 written scripts per document ends them (before it, the task time "
    "limit ended them at more than 1 GiB), and the page loads within the time limit of "
    "the case (ADR 0028 sections 8 and 16)",
    expect_log="scripts from written text ran",
    swb_args=JS,
    time_limit_s=30,
    rss_limit_mib=512,
)
def document_write_fork_js_on() -> Page:
    script = (
        "<script>var c = '<script>' + document.currentScript.text + '<\\/script>'; "
        "document.write(c); document.write(c);</script>"
    )
    return Page(doc("<p>write</p>" + script))


@case(
    "A flood of document.write calls behind a script that the parser waits for, with "
    "JavaScript on (M9 feature 2): 3,000,000 small writes, then 5 writes of 16 MiB, all "
    "inside a comment. The waiting text is bounded at 64 MiB per document (ADR 0028 "
    "section 16): the writes past it are dropped with a warning, and the page loads when "
    "the script (a server that answers after 2 s) arrives",
    expect_log="would wait for the parser",
    server="slow",
    swb_args=JS,
    time_limit_s=30,
    rss_limit_mib=2048,
)
def document_write_flood_pending_js_on() -> Page:
    script = (
        '<script>document.write(\'<script src="http://127.0.0.1:__PORT__/w.js"><\\/script>'
        "<!--'); "
        "for (let i = 0; i < 3000000; i++) document.write('x'); "
        "const big = 'y'.repeat(16 << 20); "
        "for (let i = 0; i < 5; i++) document.write(big); "
        "document.write('-->'); "
        "console.log('flooded');</script>"
    )
    return Page(doc("<p>write</p>" + script))


@case(
    "A loop of 600,000 document.writeln calls that each write one comment with JavaScript on "
    "(M9 feature 2): the parser reads each call at once; the nodes of the document (a "
    "comment and a line feed for each call) reach the limit of 1,048,576, the parser stops "
    "with a warning, and the page loads within the time limit (ADR 0028 sections 8 and 16)",
    expect_log="HTML parser stopped",
    swb_args=JS,
    time_limit_s=30,
)
def document_writeln_loop_js_on() -> Page:
    script = (
        "<script>for (let i = 0; i < 600000; i++) document.writeln('<!--' + i + '-->');</script>"
    )
    return Page(doc("<p>write</p>" + script))


@case(
    "300,000 Intl.DateTimeFormat, NumberFormat and RelativeTimeFormat objects with options, "
    "each used once, and 100,000 toLocaleString calls with options on dates and numbers, "
    "with JavaScript on (M9 feature 3, ADR 0027 section 10): the objects are garbage at once, "
    "and the heap does not grow with the count",
    swb_args=JS,
    time_limit_s=15,
)
def intl_formatters_loop_js_on() -> Page:
    script = (
        "<script>let n = 0; for (let i = 0; i < 300000; i++) { "
        "n += new Intl.DateTimeFormat('en-GB', {hour: 'numeric', minute: '2-digit', "
        "timeZone: 'UTC', timeZoneName: 'short'}).format(i * 60000).length; "
        "n += new Intl.NumberFormat('en-US', {style: 'unit', unit: 'minute', "
        "unitDisplay: 'long'}).format(i).length; "
        "n += new Intl.RelativeTimeFormat('en', {numeric: 'auto'}).format(i % 5 - 2, 'day')"
        ".length; } "
        "for (let i = 0; i < 100000; i++) { "
        "n += new Date(i * 86400000).toLocaleDateString('en-US', {month: 'long', "
        "timeZone: 'UTC'}).length + "
        "i.toLocaleString('en-GB', {maximumFractionDigits: 2}).length; } "
        "console.log('chars ' + n);</script>"
    )
    return Page(doc("<p>intl</p>" + script))


@case(
    "Intl.NumberFormat with the largest digit options on big and long numbers, with JavaScript "
    "on: maximumFractionDigits 100 on doubles from 5e-324 to 1.8e308 and on strings and "
    "BigInts of 300,000 digits (linear), a string of 2 million digits or an exponent of "
    "2^30 (the infinity, read in place without a copy), and a BigInt of 2 million digits "
    "(a RangeError, not a 2 GB string)",
    swb_args=JS,
    time_limit_s=15,
)
def intl_number_digits_js_on() -> Page:
    script = (
        "<script>const f = new Intl.NumberFormat('en-US', {maximumFractionDigits: 100, "
        "minimumFractionDigits: 100, minimumIntegerDigits: 21}); let n = 0; "
        "for (const x of [5e-324, 1e-10, 1/3, 123456789.123456789, 1e21, 1.7976931348623157e308]) "
        "n += f.format(x).length; "
        "n += f.format('7'.repeat(300000)).length; n += f.format(10n ** 300000n).length; "
        "const g = new Intl.NumberFormat('en-US'); let errors = 0; "
        "for (const text of ['9'.repeat(2000000), '1e1073741824', '1e-1073741824x']) "
        "try { n += g.format(text).length; } catch (e) { errors++; } "
        "try { g.format(10n ** 2000000n); } catch (e) { errors++; } "
        "console.log('chars ' + n + ' errors ' + errors);</script>"
    )
    return Page(doc("<p>intl</p>" + script))


@case(
    "Intl.getCanonicalLocales and the constructors with a list of 1,000,000 distinct "
    "language tags, a list of 100,000 invalid ones after a valid one, and an array-like "
    "of length 2^32 - 1 with JavaScript on (the time limit ends the last loop)",
    swb_args=JS,
    time_limit_s=15,
)
def intl_locale_lists_js_on() -> Page:
    script = (
        "<script>const tags = Array.from({length: 1000000}, (_, i) => 'en-x-t' + i); "
        "console.log('canonical ' + Intl.getCanonicalLocales(tags).length); "
        "console.log('format ' + new Intl.DateTimeFormat(tags).resolvedOptions().locale); "
        "try { Intl.getCanonicalLocales(['en'].concat(Array(100000).fill('en_US'))); } "
        "catch (e) { console.log(e.name); } "
        "try { Intl.getCanonicalLocales({length: 4294967295}); } "
        "catch (e) { console.log('long ' + e.name); }</script>"
    )
    return Page(doc("<p>intl</p>" + script))


@case(
    "100,000 lazy images (loading=lazy) in a page of 5 million px, and a script that scrolls "
    "down in 100 steps of 50,000 px, one step per animation frame, with JavaScript on "
    "(feature 4 of M9): the check of the lazy images walks the painted boxes once per frame "
    "that scrolled and starts only the loads within 3000 px of the viewport (ADR 0032)",
    swb_args=("--js", "--virtual-time", "--settle-ms", "5000"),
    time_limit_s=30,
)
def lazy_images_scrolled_in_steps_js_on() -> Page:
    images = "".join(f"<img loading=lazy src=l{i}.png width=1 height=50>" for i in range(100_000))
    script = (
        "<script>let n = 0; const step = () => { scrollBy(0, 50000); if (++n < 100) "
        "requestAnimationFrame(step); else console.log('lazy scrolled ' + scrollY); }; "
        "addEventListener('load', () => requestAnimationFrame(step));</script>"
    )
    return Page(doc(images + script, "img{display:block}"))


@case(
    "30,000 lazy images far below the viewport, and a script that sets loading to eager and "
    "back to lazy on all of them 20 times, with JavaScript on (feature 4 of M9): the first "
    "change starts each deferred load once (a released image never defers again), later "
    "changes cost the attribute change only (ADR 0032)",
    swb_args=("--js", "--virtual-time", "--settle-ms", "2000"),
    time_limit_s=30,
)
def lazy_loading_attribute_toggled_js_on() -> Page:
    images = "".join(f"<img loading=lazy src=t{i}.png width=1 height=100>" for i in range(30_000))
    script = (
        "<script>addEventListener('load', () => { const all = document.images; "
        "for (let round = 0; round < 20; round++) for (const im of all) "
        "im.loading = round % 2 ? 'lazy' : 'eager'; "
        "console.log('lazy toggled ' + all.length); });</script>"
    )
    return Page(doc(images + script, "img{display:block}"))


@case(
    "200 nested scroll containers (overflow: auto, 100 px high) with 50 lazy images in the "
    "innermost, the first at 5000 px, and a script that scrolls every container down in 40 "
    "animation frames, with JavaScript on (feature 4 of M9): the painted boxes of the images "
    "are found through the "
    "scroll offsets of all containers; the work of a check stays linear (ADR 0032)",
    swb_args=("--js", "--virtual-time", "--settle-ms", "3000"),
    time_limit_s=30,
)
def lazy_images_in_nested_scrollers_js_on() -> Page:
    images = "".join(
        f"<img loading=lazy src=n{i}.png width=1 height=100 "
        f"style='margin-top:{5000 if i == 0 else 0}px'>"
        for i in range(50)
    )
    body = nest("<div class=s>", "</div>", 200, images)
    script = (
        "<script>let n = 0; const step = () => { for (const s of document.querySelectorAll('.s')) "
        "s.scrollTop += 500; if (++n < 40) requestAnimationFrame(step); "
        "else console.log('nested scrolled ' + n); }; "
        "addEventListener('load', () => requestAnimationFrame(step));</script>"
    )
    return Page(doc(body + script, ".s{overflow:auto;height:100px;width:90%}img{display:block}"))
