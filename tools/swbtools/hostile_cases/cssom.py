"""CSSOM style sheets (ADR 0028 section 10)."""

from swbtools.hostile_cases.core import (
    JS,
    Page,
    case,
    doc,
)


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
