"""`document.write`, CSSOM style sheets and module scripts (ADR 0028)."""

import json

from swbtools.hostile_cases.core import (
    JS,
    MIB,
    REPLAY,
    REPLAY_URL,
    Page,
    case,
    doc,
    nest,
    stripes_png,
    web_font,
)

# --- document.write ----------------------------------------------------------


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


# --- CSSOM style sheets (ADR 0028 section 10) --------------------------------


@case(
    "100,000 insertRule calls into one style element's sheet with a style query "
    "(getComputedStyle) after each, on a page of 2,000 elements, with JavaScript on (M9 "
    "feature 6): each query builds the rule index of all sheets again (no parse of the other "
    "rules), so the loop slows as the sheet grows and the task ends at its time limit of 5 s "
    "(the binding checks the deadline after each flush; ADR 0028 sections 10 and 16)",
    swb_args=JS,
    time_limit_s=8,
)
def cssom_insert_rule_flush_loop_js_on() -> Page:
    script = (
        "<script>const s = document.createElement('style'); document.head.appendChild(s); "
        "const sh = s.sheet; const e = document.getElementById('t'); let n = 0; "
        "for (let i = 0; i < 100000; i++) { "
        "sh.insertRule('.c' + i + ' { margin-left: ' + (i % 50) + 'px }', sh.cssRules.length); "
        "e.className = 'c' + i; n += parseFloat(getComputedStyle(e).marginLeft); } "
        "console.log('inserted ' + n);</script>"
    )
    return Page(doc("<p>x</p>" * 2000 + "<div id=t></div>" + script))


@case(
    "deleteRule(0) in a loop over a style sheet of 60,000 rules, then insertRule(…, 0) of "
    "60,000 rules, with JavaScript on (M9 feature 6): each call moves the list of rule "
    "pointers once; the loops end well within the time limit and the stylist is built once "
    "at the end",
    swb_args=JS,
    time_limit_s=8,
)
def cssom_delete_first_rule_loop_js_on() -> Page:
    css = "".join(f".d{i} {{ top: {i % 7}px }}" for i in range(60_000))
    script = (
        "<script>const sh = document.querySelector('style').sheet; "
        "while (sh.cssRules.length) sh.deleteRule(0); "
        "for (let i = 0; i < 60000; i++) sh.insertRule('.e' + i + ' { left: 1px }', 0); "
        "console.log('rules ' + sh.cssRules.length);</script>"
    )
    return Page(doc("<p class='d1 e1'>x</p>" + script, css))


@case(
    "A rule with a selector list of 200,000 classes inserted by insertRule and set again by "
    "selectorText, and a second rule with 100,000 declarations set through rule.style, on a "
    "page of 5,000 elements, with JavaScript on (M9 feature 6): one parse of each text, one "
    "rule index of 200,000 selectors, the cssText reads stay linear",
    swb_args=JS,
    time_limit_s=10,
    rss_limit_mib=1536,
)
def cssom_huge_selector_list_js_on() -> Page:
    script = (
        "<script>const sh = document.querySelector('style').sheet; "
        "const sel = Array.from({length: 200000}, (_, i) => '.h' + i).join(', '); "
        "sh.insertRule(sel + ' { color: rgb(1, 2, 3) }', 0); "
        "const a = getComputedStyle(document.querySelector('p')).color; "
        "sh.cssRules[0].selectorText = sel.replace(/h/g, 'k'); "
        "const b = getComputedStyle(document.querySelector('p')).color; "
        "sh.insertRule('p { }', 1); let css = ''; "
        "for (let i = 0; i < 100000; i++) css += '--v' + i + ': ' + i + '; '; "
        "sh.cssRules[1].style.cssText = css; "
        "console.log('selectors ' + a + ' ' + b + ' ' + sh.cssRules[0].cssText.length + ' ' + "
        "sh.cssRules[1].cssText.length);</script>"
    )
    return Page(doc("<p class='h7 k9'>x</p>" * 5000 + script))


@case(
    "insertRule of an @media rule nested 100,000 deep (and of @supports and @keyframes text "
    "nested as deep), then cssText of each rule, with JavaScript on (M9 feature 6): the "
    "parser skips the blocks past its nesting limit of 64, so the rule tree, its "
    "serialization and the rule index stay shallow",
    swb_args=JS,
    expect_log="CSS blocks nested too deeply",
)
def cssom_deep_media_js_on() -> Page:
    script = (
        "<script>const sh = document.querySelector('style').sheet; let t = 0; "
        "for (const k of ['@media screen', '@supports (display: block)', '@media print']) { "
        "const text = (k + ' { ').repeat(100000) + 'p { color: red }' + ' }'.repeat(100000); "
        "try { sh.insertRule(text, 0); t += sh.cssRules[0].cssText.length; } "
        "catch (e) { t -= 1; } } "
        "try { sh.insertRule('@keyframes k { ' + '0% { '.repeat(100000) + ' }', 0); } catch (e) {} "
        "console.log('nested ' + t + ' ' + getComputedStyle(document.querySelector('p')).color);"
        "</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "insertRule of an empty @media rule into the innermost @media rule in a loop, with "
    "JavaScript on (M9 feature 6): scripts could nest grouping rules without end, and the "
    "serialization and the rule index recurse along the nesting; insertRule refuses a rule "
    "64 levels deep (HierarchyRequestError, a warning), then cssText and a style query run "
    "(ADR 0028 section 10)",
    swb_args=JS,
    expect_log="grouping rules nested",
)
def cssom_nested_group_insert_js_on() -> Page:
    script = (
        "<script>const sh = document.querySelector('style').sheet; "
        "sh.insertRule('@media screen { }', 0); let m = sh.cssRules[0]; let n = 0; "
        "for (let i = 0; i < 100000; i++) { "
        "try { m.insertRule('@media screen { p { top: 1px } }', 0); "
        "m = m.cssRules[0]; n++; } catch (e) { break; } } "
        "console.log('nested ' + n + ' ' + sh.cssRules[0].cssText.length + ' ' + "
        "getComputedStyle(document.querySelector('p')).top);</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "10,000 style elements made by a script, then cssRules and cssText of each sheet read "
    "through document.styleSheets three times (30,000 rule wrappers), then all removed and "
    "made again 5 times, with JavaScript on (M9 feature 6): each list access walks the "
    "sheets of the document once; the wrappers of sheets that left the document go from the "
    "maps when they reach their limit",
    swb_args=JS,
    time_limit_s=10,
)
def cssom_many_sheets_js_on() -> Page:
    script = (
        "<script>let n = 0; for (let round = 0; round < 5; round++) { const made = []; "
        "for (let i = 0; i < 10000; i++) { const s = document.createElement('style'); "
        "s.textContent = '.m' + i + ' { top: 1px }'; document.head.appendChild(s); made.push(s); } "
        "for (let k = 0; k < 3; k++) { const l = document.styleSheets; "
        "for (let i = 0; i < l.length; i++) { const r = l[i].cssRules[0]; "
        "if (r) n += r.cssText.length; } } "
        "for (const s of made) s.remove(); } console.log('sheets ' + n);</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "One rule of 300,000 declarations (1.8 MB of text) inserted 300 times by insertRule, with "
    "JavaScript on (M9 feature 6, review fix): the parsed rule takes about 100 MB, so 300 "
    "inserts took 29 GB before the bound. The estimated bytes of the style data that "
    "scripts make are limited to 128 MiB per document: the rule is estimated at 135 MB, so "
    "each insert is refused by the check of the text before the parse (QuotaExceededError, "
    "one warning), and the loop ends within the limits of the case (ADR 0028 section 16)",
    swb_args=JS,
    expect_log="reaches the limit",
)
def cssom_insert_large_rule_loop_js_on() -> Page:
    script = (
        "<script>const sh = document.querySelector('style').sheet; "
        "const t = '.a{' + '--v:1;'.repeat(300000) + '}'; let ok = 0, refused = 0; "
        "for (let i = 0; i < 300; i++) { try { sh.insertRule(t, 0); ok++; } "
        "catch (e) { if (e.name !== 'QuotaExceededError') throw e; refused++; } } "
        "console.log('inserted ' + ok + ' refused ' + refused + ' rules ' + sh.cssRules.length);"
        "</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "300 style elements, each with a text of 300,000 declarations made by a script, with a "
    "style query after each, and JavaScript on (M9 feature 6, review fix): a parsed sheet of "
    "this text takes about 100 MB. The style data that scripts make is limited to 128 MiB "
    "per document (estimated): later sheets have no rules (one warning), the text itself "
    "stays in the DOM (ADR 0028 section 16)",
    swb_args=JS,
    expect_log="reaches the limit",
)
def cssom_large_style_elements_js_on() -> Page:
    script = (
        "<script>const t = '.a{' + '--v:1;'.repeat(300000) + '}'; let rules = 0; "
        "for (let i = 0; i < 300; i++) { const s = document.createElement('style'); "
        "s.textContent = t; document.head.appendChild(s); "
        "getComputedStyle(document.querySelector('p')).color; rules += s.sheet.cssRules.length; } "
        "console.log('rules ' + rules);</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "A text of 100,000 different declarations set 300 times through rule.style.cssText of "
    "300 rules, with JavaScript on (M9 feature 6, review fix): each text takes about 30 MB "
    "once parsed (estimated at 46 MB). The style data that scripts make is limited to 128 "
    "MiB per document (estimated): the sets after the second change nothing (one warning; "
    "the check of the text before the parse refuses them), and the loop ends within the "
    "limits of the case (ADR 0028 section 16)",
    swb_args=JS,
    expect_log="reaches the limit",
)
def cssom_rule_style_text_loop_js_on() -> Page:
    script = (
        "<script>const sh = document.querySelector('style').sheet; let css = ''; "
        "for (let i = 0; i < 100000; i++) css += '--v' + i + ': ' + i + '; '; "
        "for (let i = 0; i < 300; i++) sh.insertRule('.r' + i + ' { }', i); "
        "let set = 0; for (let i = 0; i < 300; i++) { sh.cssRules[i].style.cssText = css; "
        "if (sh.cssRules[i].style.length) set++; } console.log('set ' + set);</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "A selector list of 20,000 classes set 300 times through selectorText of 300 rules, "
    "with JavaScript on (M9 feature 6, review fix): selectors that scripts set count "
    "against the style data that scripts may make (128 MiB per document, estimated), so "
    "the sets after the limit change nothing (one warning) and the loop ends within the "
    "limits of the case (ADR 0028 section 16)",
    swb_args=JS,
    expect_log="reaches the limit",
)
def cssom_selector_text_loop_js_on() -> Page:
    script = (
        "<script>const sh = document.querySelector('style').sheet; "
        "const sel = Array.from({length: 20000}, (_, i) => '.s' + i).join(', '); "
        "for (let i = 0; i < 300; i++) sh.insertRule('.r' + i + ' { }', i); "
        "let set = 0; for (let i = 0; i < 300; i++) { sh.cssRules[i].selectorText = sel; "
        "if (sh.cssRules[i].selectorText.length > 100) set++; } "
        "console.log('set ' + set);</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "20,000 appendMedium calls with different queries on the MediaList of one sheet, then "
    "item() and deleteMedium of each, with JavaScript on (M9 feature 6, review fix): each "
    "call compares the query with the list without parsing the list again, so the loops "
    "stay far inside the time limit (the version that parsed the list each time took more "
    "than 5 s)",
    swb_args=JS,
    time_limit_s=10,
)
def cssom_append_medium_loop_js_on() -> Page:
    script = (
        "<script>const m = document.querySelector('style').sheet.media; "
        "for (let i = 0; i < 20000; i++) m.appendMedium('(min-width: ' + i + 'px)'); "
        "let n = m.length; for (let i = 0; i < m.length; i++) n += m.item(i).length; "
        "for (let i = 0; i < 20000; i++) m.deleteMedium('(min-width: ' + i + 'px)'); "
        "console.log('media ' + n + ' ' + m.length);</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "200,000 insertRule calls of '.aN{all:initial}' into one sheet, with a style flush "
    "every 1,000, with JavaScript on (M9 feature 6, second fix round): the declaration `all` "
    "makes about 150 entries in the cascade block, so a rule takes 13 KB of parsed data "
    "for 20 bytes of text. The bound follows the parsed form (about 17 KB estimated per "
    "rule): inserts after 128 MiB estimated are refused (QuotaExceededError, one warning); "
    "before the fix 200,000 inserts would take 2.6 GB (ADR 0028 section 16)",
    swb_args=JS,
    time_limit_s=8,
    expect_log="reaches the limit",
)
def cssom_insert_all_loop_js_on() -> Page:
    script = (
        "<script>const sh = document.querySelector('style').sheet; "
        "const p = document.querySelector('p'); let ok = 0, refused = 0; "
        "for (let i = 0; i < 200000; i++) { "
        "try { sh.insertRule('.a' + i + '{all:initial}', ok); ok++; } "
        "catch (e) { if (e.name !== 'QuotaExceededError') throw e; refused++; } "
        "if (i % 1000 === 999) getComputedStyle(p).color; } "
        "console.log('inserted ' + ok + ' refused ' + refused);</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "200,000 insertRule calls of a rule with the shorthands margin, padding, border, font, "
    "background and grid, with a style flush every 1,000, with JavaScript on (M9 feature 6, "
    "second fix round): a shorthand makes one entry per longhand (about 45 entries and "
    "8 KB for 80 bytes of text). The bound follows the parsed form: inserts after 128 MiB "
    "estimated are refused (ADR 0028 section 16)",
    swb_args=JS,
    time_limit_s=8,
    expect_log="reaches the limit",
)
def cssom_insert_shorthands_loop_js_on() -> Page:
    script = (
        "<script>const sh = document.querySelector('style').sheet; "
        "const p = document.querySelector('p'); let ok = 0, refused = 0; "
        "for (let i = 0; i < 200000; i++) { "
        "try { sh.insertRule('.a' + i + '{margin:0;padding:0;border:0;font:12px a;"
        "background:red;grid:none}', ok); ok++; } "
        "catch (e) { if (e.name !== 'QuotaExceededError') throw e; refused++; } "
        "if (i % 1000 === 999) getComputedStyle(p).color; } "
        "console.log('inserted ' + ok + ' refused ' + refused);</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "rule.style.setProperty('--v' + i, 'x'.repeat(1000)) on one inserted rule, i up to "
    "1,000,000, with JavaScript on (M9 feature 6, second fix round): every property is a new "
    "entry with a value of 1 KB, and a write changed the rule without a charge. The charge "
    "follows the block (about 3 KB estimated per entry): writes after 128 MiB estimated "
    "change nothing (one warning), and the loop ends at its time limit of 5 s "
    "(ADR 0028 section 16)",
    swb_args=JS,
    time_limit_s=8,
    expect_log="reaches the limit",
)
def cssom_set_property_loop_js_on() -> Page:
    script = (
        "<script>const sh = document.querySelector('style').sheet; sh.insertRule('.a { }', 0); "
        "const st = sh.cssRules[0].style; const v = 'x'.repeat(1000); "
        "for (let i = 0; i < 1000000; i++) st.setProperty('--v' + i, v); "
        "console.log('length ' + st.length);</script>"
    )
    return Page(doc("<p>x</p>" + script))


@case(
    "100,000 inserted rules, each set to rule.style.cssText = 'all: initial', with "
    "JavaScript on (M9 feature 6, second fix round): swb's block of rule.style does not "
    "store `all` (departure), so the rules stay small; the loop ends within the limits "
    "(ADR 0028 section 16)",
    swb_args=JS,
    time_limit_s=8,
)
def cssom_rule_style_all_loop_js_on() -> Page:
    script = (
        "<script>const sh = document.querySelector('style').sheet; let ok = 0; "
        "for (let i = 0; i < 100000; i++) { sh.insertRule('.c' + i + ' { }', ok); "
        "sh.cssRules[ok].style.cssText = 'all: initial'; ok++; } "
        "console.log('rules ' + sh.cssRules.length);</script>"
    )
    return Page(doc("<p>x</p>" + script))


# --- Module scripts ----------------------------------------------------------


def replay_scripts_page(html: str, scripts: dict[str, str]) -> Page:
    """Like `replay_page`, with JavaScript files of `http://hostile.test/`."""
    entries = [
        {
            "method": "GET",
            "url": REPLAY_URL,
            "status": 200,
            "headers": [["content-type", "text/html; charset=utf-8"]],
            "body": "files/index.html",
        }
    ]
    files: dict[str, str | bytes] = {"files/index.html": html}
    for name, text in scripts.items():
        entries.append(
            {
                "method": "GET",
                "url": f"http://hostile.test/{name}",
                "status": 200,
                "headers": [["content-type", "text/javascript"]],
                "body": f"files/{name}",
            }
        )
        files[f"files/{name}"] = text
    files["manifest.json"] = json.dumps({"version": 1, "entries": entries})
    return Page(html, files)


@case(
    "10,000 inline module scripts in one page, with JavaScript on (M9 feature 7): each runs "
    "in a task of its own after the parse, in order, with a microtask checkpoint; the page "
    "loads within the time limit (ADR 0028 section 8)",
    swb_args=JS,
    time_limit_s=10,
)
def module_scripts_inline_10000_js_on() -> Page:
    modules = "<script type=module>window.n = (window.n || 0) + 1;</script>" * 10_000
    script = "<script>addEventListener('load', () => console.log('ran ' + window.n));</script>"
    return Page(doc("<p>modules</p>" + modules + script))


@case(
    "10,000 module scripts with the same URL, with JavaScript on and an http: page from a "
    "replayed fixture (M9 feature 7): the module map has one entry, so the page fetches the "
    "URL once, the module runs once, and each element gets its `load` event (ADR 0028 "
    "section 8)",
    swb_args=REPLAY,
    time_limit_s=10,
    url=REPLAY_URL,
)
def module_scripts_same_url_10000_js_on() -> Page:
    modules = '<script type=module src="m.js"></script>' * 10_000
    script = (
        "<script>let loads = 0; document.addEventListener('load', (e) => { "
        "if (e.target.localName === 'script') loads++; }, true); "
        "addEventListener('load', () => console.log('runs ' + window.n + ' loads ' + loads));"
        "</script>"
    )
    return replay_scripts_page(
        doc("<p>modules</p>" + script + modules),
        {"m.js": "window.n = (window.n || 0) + 1;"},
    )


@case(
    "10,000 inline module scripts that import one module, with JavaScript on and an http: "
    "page from a replayed fixture (M10 3b-1; replaces M9's case of 10,000 refused modules): "
    "the module map fetches the module once and it runs once; each inline module runs after "
    "its graph is complete (ADR 0034 section 8)",
    swb_args=REPLAY,
    time_limit_s=10,
    url=REPLAY_URL,
    expect_log="dep 1 modules 10000",
)
def module_scripts_import_10000_js_on() -> Page:
    modules = "<script type=module>import './dep.js'; window.m = (window.m || 0) + 1;</script>"
    script = (
        "<script>addEventListener('load', () => "
        "console.warn('dep ' + window.n + ' modules ' + window.m));</script>"
    )
    return replay_scripts_page(
        doc("<p>modules</p>" + script + modules * 10_000),
        {"dep.js": "window.n = (window.n || 0) + 1;"},
    )


@case(
    "A chain of 10,000 modules, each importing the next, with JavaScript on and an http: page "
    "from a replayed fixture (M10 3b-1): the graph loads one fetch after the other, links and "
    "evaluates without Rust stack per module, and the leaf runs first (ADR 0034 sections 6 and "
    "8)",
    swb_args=REPLAY,
    time_limit_s=30,
    url=REPLAY_URL,
    expect_log="chain ran 10000",
)
def module_chain_10000_js_on() -> Page:
    n = 10_000
    files = {
        f"m{i}.js": f"import './m{i + 1}.js'; window.c = (window.c || 0) + 1;" for i in range(n - 1)
    }
    files[f"m{n - 1}.js"] = "window.c = (window.c || 0) + 1;"
    script = (
        "<script>addEventListener('load', () => console.warn('chain ran ' + window.c));</script>"
    )
    return replay_scripts_page(
        doc("<p>modules</p>" + script + "<script type=module src=m0.js></script>"), files
    )


@case(
    "A ring of 10,000 modules (each imports the next, the last the first) with live bindings, "
    "with JavaScript on and an http: page from a replayed fixture (M10 3b-1): the cycle loads, "
    "links and evaluates once (ADR 0034 sections 6 and 8)",
    swb_args=REPLAY,
    time_limit_s=30,
    url=REPLAY_URL,
    expect_log="ring ran 10000",
)
def module_cycle_ring_10000_js_on() -> Page:
    n = 10_000
    files = {
        f"r{i}.js": (
            f"import {{v{(i + 1) % n}}} from './r{(i + 1) % n}.js'; export let v{i} = {i}; "
            "window.c = (window.c || 0) + 1;"
        )
        for i in range(n)
    }
    script = (
        "<script>addEventListener('load', () => console.warn('ring ran ' + window.c));</script>"
    )
    return replay_scripts_page(
        doc("<p>modules</p>" + script + "<script type=module src=r0.js></script>"), files
    )


@case(
    "A mesh of 200 modules where each re-exports all others with `export *` (40,000 star "
    "edges) and a namespace import of it, with JavaScript on and an http: page from a replayed "
    "fixture (M10 3b-1): the link finishes or ends at the task's time limit, never hangs "
    "(ADR 0034 sections 6 and 11)",
    swb_args=REPLAY,
    time_limit_s=30,
    url=REPLAY_URL,
)
def module_star_mesh_js_on() -> Page:
    n = 200
    files = {
        f"s{i}.js": "".join(f"export * from './s{j}.js';" for j in range(n) if j != i)
        + f"export const v{i} = {i};"
        for i in range(n)
    }
    script = (
        "<script type=module>import * as ns from './s0.js'; "
        "console.warn('mesh names ' + Object.keys(ns).length);</script>"
    )
    return replay_scripts_page(doc("<p>modules</p>" + script), files)


@case(
    "A module with 100,000 export names (re-exports of one binding) and a namespace import of "
    "it, with JavaScript on and an http: page from a replayed fixture (M10 3b-1): the link "
    "and the namespace are linear in the names (ADR 0034 sections 6 and 11)",
    swb_args=REPLAY,
    time_limit_s=20,
    url=REPLAY_URL,
    expect_log="exports 100000",
)
def module_exports_100000_js_on() -> Page:
    names = ",".join(f"a as e{i}" for i in range(100_000))
    files = {"big.js": f"const a = 1; export {{{names}}};"}
    script = (
        "<script type=module>import * as ns from './big.js'; "
        "console.warn('exports ' + Object.keys(ns).length);</script>"
    )
    return replay_scripts_page(doc("<p>modules</p>" + script), files)


@case(
    "`import()` of 100,000 different URLs (one module with 100,000 fragments), with "
    "JavaScript on and an http: page from a replayed fixture (M10 3b-1): one request serves "
    "the URLs while it is in flight; the module map takes 65,536 entries (65,536 modules), "
    "the later URLs fail at once with one warning (ADR 0034 sections 8 and 11). The fetches "
    "of `import()` do not delay `load`, so swb runs the event loop in virtual time until "
    "they end",
    swb_args=(*REPLAY, "--virtual-time", "--settle-ms", "60000"),
    time_limit_s=40,
    url=REPLAY_URL,
    expect_log=("the module map has 65536 entries", "fulfilled 65536 rejected 34464"),
    rss_limit_mib=1500,
)
def module_import_call_urls_100000_js_on() -> Page:
    script = (
        "<script type=module>let ok = 0, failed = 0; const all = []; "
        "for (let i = 0; i < 100000; i++) "
        "all.push(import('./m.js#' + i).then(() => { ok++; }, () => { failed++; })); "
        "Promise.all(all).then(() => console.warn('fulfilled ' + ok + ' rejected ' + failed));"
        "</script>"
    )
    return replay_scripts_page(doc("<p>modules</p>" + script), {"m.js": "export default 1;"})


@case(
    "A module whose top-level `await` never settles, imported by a deferred module, with "
    "JavaScript on and an http: page from a replayed fixture (M10 3b-1): neither "
    "`DOMContentLoaded` nor `load` wait for it; the importer never runs (ADR 0034 section 10)",
    swb_args=REPLAY,
    time_limit_s=10,
    url=REPLAY_URL,
    expect_log="loaded without the importer",
)
def module_tla_never_settles_js_on() -> Page:
    files = {"never.js": "window.started = 1; await new Promise(() => {});"}
    script = (
        "<script>addEventListener('load', () => { if (window.started && !window.importer) "
        "console.warn('loaded without the importer'); });</script>"
        "<script type=module>import './never.js'; window.importer = 1;</script>"
    )
    return replay_scripts_page(doc("<p>modules</p>" + script), files)


@case(
    "An import map with 100,000 entries, with JavaScript on and an http: page from a replayed "
    "fixture (M10 3b-2): the document's map takes 65,536 entries and ignores the later ones, "
    "with one warning; the first key resolves and the last does not (ADR 0034 sections 9 and "
    "11)",
    swb_args=REPLAY,
    time_limit_s=20,
    url=REPLAY_URL,
    expect_log=("the import map has 65536 entries", "first ok last failed"),
)
def importmap_entries_100000_js_on() -> Page:
    entries = ",".join(f'"k{i}":"./v{i}.js"' for i in range(100_000))
    script = (
        "<script type=module>const ok = (s) => { try { import.meta.resolve(s); return true; } "
        "catch { return false; } }; "
        "console.warn('first ' + (ok('k0') ? 'ok' : 'failed') + ' last ' + "
        "(ok('k99999') ? 'ok' : 'failed'));</script>"
    )
    head = '<p>import map</p><script type=importmap>{"imports":{' + entries + "}}</script>"
    return replay_scripts_page(doc(head + script), {})


@case(
    "An import map with 50,000 scopes, 100,000 resolutions with it, and one specifier of 1 MiB "
    "of slashes, with JavaScript on and an http: page from a replayed fixture (M10 3b-2): a "
    "resolution looks up the prefixes of the base URL and of the specifier in hash maps, so "
    "its time does not depend on the number of scopes that the base URL does not match, "
    "and the long specifier ends its search at the lookup budget, which the exact lookup "
    "in each matching scope also uses (ADR 0034 sections 9 and 11)",
    swb_args=REPLAY,
    time_limit_s=20,
    url=REPLAY_URL,
    expect_log="resolved 100000",
)
def importmap_scopes_50000_js_on() -> Page:
    scopes = ",".join(f'"/s{i}/":{{"a":"/scoped{i}.js"}}' for i in range(50_000))
    script = (
        "<script type=module>let n = 0; "
        "for (let i = 0; i < 100000; i++) { import.meta.resolve('a'); n++; } "
        "try { import.meta.resolve('x' + '/'.repeat(1 << 20)); } catch {} "
        "console.warn('resolved ' + n);</script>"
    )
    return replay_scripts_page(
        doc(
            '<p>import map</p><script type=importmap>{"imports":{"a":"/top.js"},"scopes":{'
            + scopes
            + "}}</script>"
            + script
        ),
        {},
    )


@case(
    "300 `import.meta.resolve` calls with a specifier of 1 MiB each, with JavaScript on and an "
    "http: page from a replayed fixture (M10 3b-2 fix round): the resolved module set records "
    "no pair of more than 4 KiB and holds at most 1 MiB in all, so the page does not keep "
    "300 MiB of specifiers (ADR 0034 sections 9 and 11)",
    swb_args=REPLAY,
    time_limit_s=30,
    url=REPLAY_URL,
    expect_log="resolved 300",
)
def importmap_resolve_long_js_on() -> Page:
    script = (
        "<script type=module>for (let i = 0; i < 300; i++) { "
        "import.meta.resolve('./k' + i + '/'.repeat(1 << 20)); } "
        "console.warn('resolved 300');</script>"
    )
    return replay_scripts_page(doc("<p>import map</p>" + script), {})


@case(
    "20,000 import maps that a script inserts, each after a resolution, with JavaScript on and "
    "an http: page from a replayed fixture (M10 3b-2): every merge checks the resolved "
    "specifiers, and the work of the merges of a document has a budget, so the page finishes "
    "within the time limit (ADR 0034 sections 9 and 11)",
    swb_args=REPLAY,
    time_limit_s=30,
    url=REPLAY_URL,
    expect_log="maps 20000",
)
def importmap_inserted_20000_js_on() -> Page:
    script = (
        "<script type=module>for (let i = 0; i < 20000; i++) { "
        "const s = document.createElement('script'); s.type = 'importmap'; "
        "s.text = JSON.stringify({imports: {['k' + i]: './v' + i + '.js'}}); "
        "document.head.append(s); import.meta.resolve('k' + i); } "
        "console.warn('maps ' + 20000);</script>"
    )
    return replay_scripts_page(doc("<p>import maps</p>" + script), {})


@case(
    "3,000 `link rel=modulepreload` elements, each for another file, with JavaScript on and an "
    "http: page from a replayed fixture (M10 3b-2): the fetches go through the queue of 64 "
    "script fetches, none delays `load`, and every link gets its `load` event; the event loop "
    "runs in virtual time until they end (ADR 0034 sections 9 and 11)",
    swb_args=(*REPLAY, "--virtual-time", "--settle-ms", "60000"),
    time_limit_s=40,
    url=REPLAY_URL,
    expect_log="links loaded 3000",
)
def modulepreload_links_3000_js_on() -> Page:
    n = 3_000
    links = "".join(f"<link rel=modulepreload href=p{i}.js>" for i in range(n))
    files = {f"p{i}.js": f"export default {i};" for i in range(n)}
    script = (
        "<script>let n = 0; document.addEventListener('load', (e) => { "
        "if (e.target.localName === 'link' && ++n === 3000) console.warn('links loaded ' + n); "
        "}, true);</script>"
    )
    return replay_scripts_page(doc("<p>preload</p>" + script + links), files)


@case(
    "A module with 70,000 top-level let bindings, with JavaScript on (M9 feature 7): the "
    "bindings of a module are registers of its function, so the module is over the compiler's "
    "limit and fails to compile as a function with so many variables does, with an error "
    "report; the page loads (ADR 0028 sections 6 and 16)",
    swb_args=JS,
    time_limit_s=10,
    expect_log="Uncaught",
)
def module_top_level_bindings_70000_js_on() -> Page:
    lets = "".join(f"let v{i}=0;" for i in range(70_000))
    return Page(doc("<p>modules</p><script type=module>" + lets + "</script>"))


@case(
    "A module that inserts 20,000 inline module scripts in a loop, and each of the first "
    "1,000 inserts 10 more, with JavaScript on (M9 feature 7): each inserted module runs in "
    "a task of its own as soon as possible; the page loads within the time limit (ADR 0028 "
    "section 8)",
    swb_args=JS,
    time_limit_s=15,
)
def module_inserts_modules_loop_js_on() -> Page:
    insert = (
        "const s = document.createElement('script'); s.type = 'module'; "
        "s.text = 'window.n++'; document.body.append(s);"
    )
    heavy = f"window.n++; for (let j = 0; j < 10; j++) {{ {insert} }}"
    script = (
        "<script type=module>"
        "window.n = 0; "
        "function add(text) { const s = document.createElement('script'); s.type = 'module'; "
        "s.text = text; document.body.append(s); } "
        f"const heavy = {json.dumps(heavy)}; "
        "for (let i = 0; i < 20000; i++) add(i < 1000 ? heavy : 'window.n++'); "
        "addEventListener('load', () => console.log('ran ' + window.n));"
        "</script>"
    )
    return Page(doc("<p>modules</p>" + script))


@case(
    "A module with a syntax error at the end of 1 MiB of source, inline and as a file, "
    "with JavaScript on and an http: page from a replayed fixture (M9 feature 7): the "
    "parser reports the error once for each; the page loads (ADR 0028 section 8)",
    swb_args=REPLAY,
    time_limit_s=10,
    url=REPLAY_URL,
    expect_log="Unexpected token",
)
def module_syntax_error_1mib_js_on() -> Page:
    text = "var a = 1;\n" * (MIB // 11) + "var x = ;"
    return replay_scripts_page(
        doc(
            "<p>modules</p><script type=module>"
            + text
            + '</script><script type=module src="big.js"></script>'
        ),
        {"big.js": text},
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
