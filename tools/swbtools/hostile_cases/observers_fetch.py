"""Observers, selection, form validation, `fetch` and `AbortSignal` (ADR 0028)."""

from swbtools.hostile_cases.core import (
    JS,
    Page,
    case,
    doc,
)

# --- Observers, selection and form validation --------------------------------


@case(
    "4 inline scripts that each register a mutation observer whose callback changes an "
    "attribute again, and change it once: the callbacks run in one microtask checkpoint "
    "without end, each checkpoint ends at the time limit of 5 s (two terminations are "
    "logged for each), and after the second such task, 10 s in all, the document runs no "
    "more scripts (ADR 0028 sections 7 and 13)",
    time_limit_s=25,
    expect_log="the scripts of the document stop",
    swb_args=JS,
)
def mutation_observer_endless_callback_js_on() -> Page:
    script = (
        "<script>var n = 0; new MutationObserver(() => { document.body.setAttribute("
        "'data-n', ++n); }).observe(document.body, {attributes: true}); "
        "document.body.setAttribute('data-n', 0);</script>"
    )
    return Page(doc("<p>text</p>" + script * 4))


@case(
    "a loop of 3,000,000 attribute changes with a mutation observer that records the old "
    "values and no yield: at most 131,072 records wait, the rest are dropped with a "
    "warning, the callback gets the kept ones, and the scripts of the document still run "
    "afterwards (a timer logs `flood ok`; ADR 0028 section 16)",
    time_limit_s=20,
    expect_log="flood ok",
    swb_args=("--js", "--settle-ms", "500"),
)
def mutation_records_flood_js_on() -> Page:
    script = (
        "<script>const p = document.querySelector('p'); let seen = 0, chars = 0; "
        "new MutationObserver((r) => { seen += r.length; "
        "for (const x of r) chars += x.oldValue === null ? 1 : x.oldValue.length; })"
        ".observe(p, {attributes: true, "
        "attributeOldValue: true}); "
        "for (let i = 0; i < 3000000; i++) p.setAttribute('title', 'v'); "
        "setTimeout(() => console.warn((seen >= 100000 && seen < 3000000 && chars >= seen "
        "? 'flood ok ' : 'flood wrong ') + seen), 10);</script>"
    )
    return Page(doc("<p>text</p>" + script))


@case(
    "a parse of 300,000 elements with a mutation observer on the document (one record "
    "each): at most 131,072 records wait, the rest are dropped with a warning, the callback "
    "gets the kept ones, and the scripts of the document still run afterwards (a timer logs "
    "`parse ok`; ADR 0028 section 16)",
    time_limit_s=20,
    expect_log="parse ok",
    swb_args=("--js", "--settle-ms", "500"),
)
def mutation_records_parse_js_on() -> Page:
    script = (
        "<script>let seen = 0; new MutationObserver((r) => { seen += r.length; })"
        ".observe(document, {childList: true, subtree: true}); "
        "setTimeout(() => console.warn((seen >= 100000 ? 'parse ok ' : 'parse wrong ') + seen),"
        " 10);</script>"
    )
    return Page(doc(script + "<i></i>" * 300000))


@case(
    "200,000 children removed one by one under 1 subtree observer, and 3,000 subtree "
    "observers on one node with 3,000 children removed one by one: the transient "
    "registrations are a set and bounded at 500,000, the work of the sink is charged to the "
    "time limit of the task, and the scripts of the document still run afterwards (a timer "
    "logs `transients ok`; ADR 0028 section 16)",
    time_limit_s=25,
    expect_log="transients ok",
    swb_args=("--js", "--settle-ms", "500"),
)
def mutation_transients_js_on() -> Page:
    script = (
        "<script>const a = document.createElement('div'); document.body.appendChild(a); "
        "for (let i = 0; i < 200000; i++) a.appendChild(document.createElement('i')); "
        "new MutationObserver(() => {}).observe(a, {childList: true, subtree: true}); "
        "while (a.firstChild) a.removeChild(a.firstChild);</script>"
        "<script>const b = document.createElement('div'); document.body.appendChild(b); "
        "for (let i = 0; i < 3000; i++) b.appendChild(document.createElement('i')); "
        "for (let i = 0; i < 3000; i++) "
        "new MutationObserver(() => {}).observe(b, {childList: true, subtree: true}); "
        "while (b.firstChild) b.removeChild(b.firstChild);</script>"
        "<script>setTimeout(() => console.warn('transients ok'), 10);</script>"
    )
    return Page(doc(script))


@case(
    "a resize observer whose callback resizes its target without end: one callback and one "
    "loop error per frame, the page stays responsive (ADR 0028 sections 7 and 13)",
    time_limit_s=15,
    swb_args=("--js", "--settle-ms", "3000"),
)
def resize_observer_endless_callback_js_on() -> Page:
    script = (
        "<script>const a = document.querySelector('div'); let n = 0; "
        "window.addEventListener('error', (e) => e.preventDefault()); "
        "new ResizeObserver(() => { n++; a.style.width = (a.offsetWidth + 1) + 'px'; "
        "if (n % 50 === 0) console.log('callbacks ' + n); }).observe(a);</script>"
    )
    return Page(doc("<div style='width:100px;height:20px'></div>" + script))


@case(
    "100,000 targets of one intersection observer, with a scroll after the first frame: "
    "the update of a frame computes all targets within the time limit of a task (feature "
    "9a)",
    time_limit_s=20,
    swb_args=("--js", "--settle-ms", "3000"),
)
def intersection_observer_100000_targets_js_on() -> Page:
    script = (
        "<script>let hits = 0; const o = new IntersectionObserver((es) => { hits += "
        "es.length; }); const f = document.createDocumentFragment(); "
        "for (let i = 0; i < 100000; i++) { const d = document.createElement('div'); "
        "f.appendChild(d); } document.body.appendChild(f); "
        "for (const d of document.body.children) o.observe(d); "
        "setTimeout(() => window.scrollTo(0, 100000), 500); "
        "setTimeout(() => console.log('hits ' + hits), 2500);</script>"
    )
    return Page(doc(script, css="div{height:2px}"))


@case(
    "A text area with a value of 20 MiB, and a script that sets its selection and reads it "
    "back 100,000 times with offsets of 4e9, with JavaScript on: each call scans the value "
    "once and the task ends at its time limit (feature 9b)",
    time_limit_s=20,
    swb_args=JS,
)
def selection_huge_value_js_on() -> Page:
    script = (
        "<script>const t = document.getElementById('t'); t.value = 'x'.repeat(20 * 1024 * 1024); "
        "let n = 0; for (let i = 0; i < 100000; i++) { t.setSelectionRange(i, 4e9, 'backward'); "
        "n += t.selectionEnd - t.selectionStart; } console.log('done ' + n);</script>"
    )
    return Page(doc('<textarea id=t style="display:none"></textarea>' + script))


@case(
    "A script that appends one character to the value of an input 300,000 times with "
    "setRangeText, with JavaScript on: each call copies the value, and the task ends at its "
    "time limit (feature 9b)",
    time_limit_s=20,
    swb_args=JS,
)
def set_range_text_growing_value_js_on() -> Page:
    script = (
        "<script>const t = document.getElementById('t'); for (let i = 0; i < 300000; i++) { "
        "t.setRangeText('y', t.value.length, t.value.length, 'end'); } "
        "console.log('done ' + t.value.length);</script>"
    )
    return Page(doc('<input id=t style="display:none">' + script))


@case(
    "A script that writes 4 times 16 MiB into a text area with setRangeText, with JavaScript "
    "on: the budget of control values (32 MiB, ADR 0028 section 16) ends the task",
    expect_log="the values of form controls reached the limit",
    time_limit_s=20,
    swb_args=JS,
)
def set_range_text_over_budget_js_on() -> Page:
    script = (
        "<script>const t = document.getElementById('t'); const s = 'z'.repeat(16 * 1024 * 1024); "
        "for (let i = 0; i < 4; i++) { t.setRangeText(s, t.value.length, t.value.length, 'end'); } "
        "console.log('not reached');</script>"
    )
    return Page(doc('<textarea id=t style="display:none"></textarea>' + script))


@case(
    "A script that sets a custom validity message of 1 MiB on 10 inputs, with JavaScript on: "
    "the budget of the messages (8 MiB, ADR 0028 section 16) ends the task (feature 9b2)",
    expect_log="the custom validity messages reached the limit",
    time_limit_s=20,
    swb_args=JS,
)
def set_custom_validity_over_budget_js_on() -> Page:
    script = (
        "<script>const s = 'm'.repeat(1 << 20); for (let i = 0; i < 10; i++) { "
        "const e = document.createElement('input'); document.body.appendChild(e); "
        "e.setCustomValidity(s); } console.log('not reached');</script>"
    )
    return Page(doc(script))


@case(
    "3,000 inputs with the pattern (a+)+$ and a value of 3,000 letters a and a !, a rule for "
    ":invalid, with JavaScript off: every style update matches the patterns with the step "
    "limit of the pass (feature 9b2)",
    time_limit_s=20,
)
def pattern_catastrophic_many_inputs() -> Page:
    value = "a" * 3000 + "!"
    inputs = "".join(f"<input pattern='(a+)+$' value='{value}{i}'>" for i in range(3000))
    return Page(doc(inputs, css="input:invalid{outline:2px solid red}"))


@case(
    "3,000 inputs with the pattern (a+)+$ and values of 3,000 letters a, a rule for :invalid, "
    "and a script that calls checkValidity() on a form of them and reads :invalid 20 times, "
    "with JavaScript on: the step limit of each pass keeps it short (feature 9b2)",
    time_limit_s=20,
    swb_args=JS,
)
def pattern_catastrophic_check_validity_js_on() -> Page:
    value = "a" * 3000 + "!"
    inputs = "".join(f"<input pattern='(a+)+$' value='{value}{i}'>" for i in range(3000))
    script = (
        "<script>const f = document.forms[0]; let n = 0; for (let i = 0; i < 20; i++) { "
        "f.elements[i].value += 'x'; n += f.checkValidity() ? 0 : 1; "
        "n += document.querySelectorAll('input:invalid').length; } "
        "console.log('done ' + n);</script>"
    )
    css = "input:invalid{outline:2px solid red}"
    return Page(doc(f"<form>{inputs}</form>" + script, css=css))


@case(
    "A form with 5,000 required radio buttons of one group and 5,000 text inputs, and a script "
    "that calls checkValidity() on single controls 200,000 times and on the form 200 times, "
    "with JavaScript on: every call is bounded and the task ends at its time limit "
    "(feature 9b2)",
    time_limit_s=20,
    swb_args=JS,
)
def check_validity_loop_js_on() -> Page:
    controls = "<input type=radio name=g required>" * 5000 + "<input required>" * 5000
    script = (
        "<script>const f = document.forms[0]; const e = f.elements; let n = 0; "
        "f.addEventListener('invalid', () => { n++; }, true); "
        "for (let i = 0; i < 200000; i++) { n += e[i % 10000].checkValidity() ? 0 : 1; } "
        "for (let i = 0; i < 200; i++) { n += f.checkValidity() ? 0 : 1; } "
        "console.log('done ' + n);</script>"
    )
    return Page(doc(f"<form>{controls}</form>" + script))


@case(
    "A form of 20,000 inputs with a pattern, minlength, min, max and step, with JavaScript on, "
    "and a script that appends and removes an input and reads :invalid 200 times: each rebuild "
    "of the control states is linear (feature 9b2)",
    time_limit_s=30,
    swb_args=JS,
)
def validity_rebuild_loop_js_on() -> Page:
    inputs = "".join(
        f"<input type=number min=1 max=100 step=3 value={i % 150}>"
        f"<input pattern='[a-z]+' minlength=2 value=ab{i}>"
        for i in range(10000)
    )
    script = (
        "<script>const f = document.forms[0]; let n = 0; for (let i = 0; i < 200; i++) { "
        "const e = document.createElement('input'); e.required = true; f.appendChild(e); "
        "n += document.querySelectorAll('input:invalid').length; e.remove(); } "
        "console.log('done ' + n);</script>"
    )
    return Page(doc(f"<form>{inputs}</form>" + script, css="input:invalid{outline:1px solid red}"))


@case(
    "A script that sets a new pattern of 10 KB on an input and reads its validity, in a loop "
    "until the time limit, with JavaScript on: the cache of compiled patterns is bounded "
    "(feature 9b2 review; it grew to 1.6 GB before)",
    time_limit_s=20,
    swb_args=JS,
)
def pattern_cache_loop_js_on() -> Page:
    script = (
        "<script>const i = document.getElementById('i'); const pad = '[a-c]'.repeat(2000); "
        "let n = 0; for (; n < 2000000; n++) { i.setAttribute('pattern', pad + '|x' + n); "
        "i.validity.patternMismatch; } console.log('not reached');</script>"
    )
    return Page(doc("<input id=i value=abc>" + script))


@case(
    "300 inputs, each with a different pattern of 44 KB that is slow to compile "
    "([\\p{RGI_Emoji}--\\q{a}] 2,000 times), and a rule for :invalid, with JavaScript off: a "
    "style update compiles a bounded amount of pattern text (feature 9b2 review; one style "
    "update took 28 s before)",
    time_limit_s=20,
)
def pattern_compile_many_inputs() -> Page:
    pad = "[\\p{RGI_Emoji}--\\q{a}]" * 2000
    inputs = "".join(f'<input pattern="{pad}|x{i}" value=a>' for i in range(300))
    return Page(doc(inputs, css="input:invalid{outline:1px solid red}"))


# --- fetch and AbortSignal ---------------------------------------------------

_NO_CORS = "{mode: 'no-cors'}"


@case(
    "5,000 fetch() calls to a server that reads each request and never answers, with "
    "JavaScript on: 6 run at once (one host), the rest wait in the queue (ADR 0028 section "
    "16); the page loads and stays responsive, and the process ends at the time limit",
    server="never",
    swb_args=JS,
)
def fetch_pending_never_js_on() -> Page:
    script = (
        "<script>let p = 0; for (let i = 0; i < 5000; i++) "
        f"fetch('http://127.0.0.1:__PORT__/n' + i, {_NO_CORS}).then(() => p++, () => p++); "
        "console.log('started 5000');</script>"
    )
    return Page(doc("<p>fetch</p>" + script))


@case(
    "20,000 fetch() calls to a server that never answers, past the queue of 16,384 requests "
    "(ADR 0028 section 16): the calls past the queue and the 6 that run fail at once with "
    "TypeError, the others wait",
    server="never",
    expect_log="too many fetch() requests wait",
    swb_args=JS,
)
def fetch_queue_overflow_js_on() -> Page:
    script = (
        "<script>let failed = 0; for (let i = 0; i < 20000; i++) "
        f"fetch('http://127.0.0.1:__PORT__/q' + i, {_NO_CORS}).catch(() => failed++); "
        "Promise.resolve().then(() => console.log('failed at once ' + failed));</script>"
    )
    return Page(doc("<p>fetch</p>" + script))


@case(
    "3,000 fetch() calls to a server that answers after 2 s, with JavaScript on: the answers "
    "come in batches of 6 (one host) and the page does not wait for them",
    server="slow",
    swb_args=JS,
)
def fetch_slow_server_js_on() -> Page:
    script = (
        "<script>let ok = 0; for (let i = 0; i < 3000; i++) "
        "fetch('http://127.0.0.1:__PORT__/s' + i).then(() => ok++, () => {}); "
        "setTimeout(() => console.log('ok after 1 s: ' + ok), 1000);</script>"
    )
    return Page(doc("<p>fetch</p>" + script))


@case(
    "A loop of 100,000 fetch() calls to a server that never answers, each aborted at once, "
    "with JavaScript on: the aborted requests leave the queue, and the ones that run give "
    "up their place when the loader puts a new thread in the place of the blocked one (at "
    "most 16 such threads; then they keep it until they end); the loop runs in bounded time "
    "and memory",
    server="never",
    swb_args=JS,
    time_limit_s=10,
)
def fetch_abort_loop_js_on() -> Page:
    script = (
        "<script>let aborted = 0; for (let i = 0; i < 100000; i++) { "
        "const c = new AbortController(); "
        "fetch('http://127.0.0.1:__PORT__/a' + i, {mode: 'no-cors', signal: c.signal})"
        ".catch(() => aborted++); c.abort(); } "
        "setTimeout(() => console.log('aborted ' + aborted), 100);</script>"
    )
    return Page(doc("<p>fetch</p>" + script))


@case(
    "64 fetch() calls whose responses have 60 MiB each, with JavaScript on, and a busy loop "
    "of 3 s so that the completions pile up in the loader (review of 10a: 3.9 GB before): "
    "the document's budget of 256 MiB for the requests and the responses not yet in the "
    "script heap (ADR 0028 section 16) fails the calls that do not fit",
    server="big",
    expect_log="memory budget",
    swb_args=JS,
    time_limit_s=10,
    rss_limit_mib=1100,
)
def fetch_big_responses_busy_js_on() -> Page:
    script = (
        "<script>let ok = 0, failed = 0; const all = []; for (let i = 0; i < 64; i++) "
        "all.push(fetch('http://127.0.0.1:__PORT__/b' + i).then(r => r.arrayBuffer())"
        ".then(() => ok++, () => failed++)); "
        "const end = Date.now() + 3000; while (Date.now() < end) {} "
        "Promise.all(all).then(() => console.log('ok ' + ok + ' failed ' + failed));</script>"
    )
    return Page(doc("<p>fetch</p>" + script))


@case(
    "64 fetch() calls with a 64 MiB ArrayBuffer as the body (no-cors POST) to a server that "
    "never answers, with JavaScript on (review of 10a: 4.4 GB before): the request bodies "
    "count against the document's budget of 256 MiB, and the calls that do not fit fail at "
    "once with TypeError",
    server="never",
    expect_log="does not fit",
    swb_args=JS,
    time_limit_s=15,
    rss_limit_mib=1100,
)
def fetch_big_request_bodies_js_on() -> Page:
    script = (
        "<script>const body = new ArrayBuffer(64 << 20); let failed = 0; "
        "for (let i = 0; i < 64; i++) "
        "fetch('http://127.0.0.1:__PORT__/p' + i, "
        "{method: 'POST', mode: 'no-cors', body}).catch(() => failed++); "
        "Promise.resolve().then(() => console.log('failed at once ' + failed));</script>"
    )
    return Page(doc("<p>fetch</p>" + script))


@case(
    "AbortSignal.any() over 100,000 signals, and 100,000 any() signals of one source: a "
    "source keeps at most 65,536 dependents and the rest do not follow it (ADR 0028 section "
    "16); one abort reaches the tracked ones",
    expect_log="signals of AbortSignal.any()",
    swb_args=JS,
    time_limit_s=10,
)
def abort_signal_any_js_on() -> Page:
    script = (
        "<script>const many = []; for (let i = 0; i < 100000; i++) many.push(new "
        "AbortController().signal); const big = AbortSignal.any(many); "
        "const c = new AbortController(); const dependents = []; "
        "for (let i = 0; i < 100000; i++) dependents.push(AbortSignal.any([c.signal])); "
        "c.abort('x'); console.log('aborted ' + dependents.filter((s) => s.aborted).length "
        "+ ' ' + big.aborted);</script>"
    )
    return Page(doc("<p>signals</p>" + script))


@case(
    "A listener added 100,000 times with the signal option of one signal, then 100,000 "
    "AbortSignal.timeout() signals: a signal tracks at most 65,536 listeners and the timeout "
    "signals wait for their timers (ADR 0028 section 16)",
    swb_args=JS,
    time_limit_s=10,
)
def abort_signal_listeners_js_on() -> Page:
    script = (
        "<script>const c = new AbortController(); const t = new EventTarget(); "
        "for (let i = 0; i < 100000; i++) t.addEventListener('x', () => {}, {signal: c.signal}); "
        "c.abort(); const keep = []; "
        "for (let i = 0; i < 100000; i++) keep.push(AbortSignal.timeout(100000)); "
        "console.log('done ' + keep.length);</script>"
    )
    return Page(doc("<p>signals</p>" + script))


@case(
    "A loop that makes 100 Response objects of 2 MiB each, keeps them and reads 2 of them "
    "with text(), then reads a 6 MiB data: URL 20 times with fetch(), with JavaScript on: "
    "the bodies count in the heap of the document (ADR 0028 section 16; the heap limit ends "
    "a runaway loop)",
    swb_args=JS,
    time_limit_s=20,
    rss_limit_mib=1200,
)
def response_bodies_js_on() -> Page:
    script = (
        "<script>const keep = []; try { for (let i = 0; i < 100; i++) "
        "keep.push(new Response(new Uint8Array(2 << 20))); } catch (e) {} "
        "const url = 'data:text/plain;base64,' + btoa('x'.repeat(6 << 20)); "
        "(async () => { let total = 0; for (let i = 0; i < 20; i++) { "
        "const r = await fetch(url); total += (await r.text()).length; } "
        "console.log('read ' + total + ' held ' + keep.length); })();</script>"
    )
    return Page(doc("<p>bodies</p>" + script))


@case(
    "5,000 XMLHttpRequest calls to a server that never answers, with JavaScript on (feature "
    "10b): at most 64 run, the others wait in the queue of the document (ADR 0028 section 16), "
    "and the page does not wait for them",
    server="never",
    swb_args=JS,
)
def xhr_hung_loop_js_on() -> Page:
    script = (
        "<script>for (let i = 0; i < 5000; i++) { const x = new XMLHttpRequest(); "
        "x.open('GET', 'http://127.0.0.1:__PORT__/x' + i); x.send(); } "
        "console.log('started 5000');</script>"
    )
    return Page(doc("<p>xhr</p>" + script))


@case(
    "A loop of 100,000 XMLHttpRequest calls to a server that never answers, each aborted at "
    "once, with JavaScript on (feature 10b): the aborted requests leave the queue, and each "
    "abort fires its events; the loop runs in bounded time and memory",
    server="never",
    swb_args=JS,
    time_limit_s=15,
)
def xhr_abort_loop_js_on() -> Page:
    script = (
        "<script>let aborted = 0; for (let i = 0; i < 100000; i++) { "
        "const x = new XMLHttpRequest(); x.onabort = () => aborted++; "
        "x.open('GET', 'http://127.0.0.1:__PORT__/a' + i); x.send(); x.abort(); } "
        "setTimeout(() => console.log('aborted ' + aborted), 100);</script>"
    )
    return Page(doc("<p>xhr</p>" + script))


@case(
    "Blobs that are concatenated and sliced in a loop, with JavaScript on (feature 10b): "
    "40 doublings of one blob and 100,000 slices of slices; a blob shares its bytes, flattens "
    "its list of pieces past 1,024 and is at most 64 MiB (ADR 0028 section 16); the longer "
    "ones throw a RangeError",
    swb_args=JS,
    time_limit_s=15,
    rss_limit_mib=1200,
)
def blob_concat_slice_js_on() -> Page:
    script = (
        "<script>let b = new Blob(['x'.repeat(1000)]); let n = 0; try { "
        "for (let i = 0; i < 40; i++) { b = new Blob([b, b]); n++; } } catch (e) { "
        "console.log(e.name); } let s = b; for (let i = 0; i < 100000; i++) "
        "s = s.slice(1, s.size - 1); console.log('doublings ' + n + ' size ' + b.size);</script>"
    )
    return Page(doc("<p>blob</p>" + script))


@case(
    "URL.createObjectURL on 100,000 small blobs without revokeObjectURL, with JavaScript on "
    "(feature 10b): a document keeps at most 16,384 Blob URLs and 256 MiB of logical bytes "
    "(ADR 0028 section 16); the calls past the limit throw QuotaExceededError",
    expect_log="too many Blob URLs",
    swb_args=JS,
    time_limit_s=15,
)
def blob_urls_no_revoke_js_on() -> Page:
    script = (
        "<script>let ok = 0, failed = 0; for (let i = 0; i < 100000; i++) { try { "
        "URL.createObjectURL(new Blob(['x'])); ok++; } catch (e) { failed++; } } "
        "console.log('ok ' + ok + ' failed ' + failed);</script>"
    )
    return Page(doc("<p>blob urls</p>" + script))


@case(
    "A FormData with 300,000 entries sent with XMLHttpRequest to a server that never answers, "
    "20 times, with JavaScript on (feature 10b): the multipart body counts against the "
    "document's request budget of 256 MiB (ADR 0028 section 16); the sends that do not fit "
    "fail",
    server="never",
    swb_args=JS,
    time_limit_s=20,
    rss_limit_mib=1200,
)
def formdata_many_entries_xhr_js_on() -> Page:
    script = (
        "<script>const fd = new FormData(); for (let i = 0; i < 300000; i++) "
        "fd.append('name' + i, 'value' + i); let failed = 0; for (let i = 0; i < 20; i++) { "
        "const x = new XMLHttpRequest(); x.onerror = () => failed++; "
        "x.open('POST', 'http://127.0.0.1:__PORT__/f' + i); x.send(fd); } "
        "setTimeout(() => console.log('failed ' + failed), 300);</script>"
    )
    return Page(doc("<p>formdata</p>" + script))


@case(
    "FileReader.readAsDataURL on a blob of 48 MiB, 20 times in a row, with JavaScript on "
    "(feature 10b): each read makes a string of 64 MiB in the heap of the document (ADR 0028 "
    "section 16: the heap limit of 256 MiB ends a runaway page); the page stays alive",
    swb_args=JS,
    time_limit_s=30,
    rss_limit_mib=1400,
)
def filereader_data_url_large_js_on() -> Page:
    script = (
        "<script>const b = new Blob([new Uint8Array(48 << 20)]); let n = 0; const next = () => { "
        "if (n++ >= 20) return; const r = new FileReader(); r.onload = next; r.onerror = next; "
        "r.readAsDataURL(b); }; try { next(); } catch (e) { console.log(e.name); } "
        "setTimeout(() => console.log('reads ' + n), 2000);</script>"
    )
    return Page(doc("<p>reader</p>" + script))


@case(
    "A loop of 100,000 navigator.sendBeacon calls with 1 KiB each to a server that never "
    "answers, with JavaScript on (feature 10b): at most 64 KiB of beacon data is queued "
    "(Chromium's quota), the other calls return false",
    server="never",
    swb_args=JS,
)
def send_beacon_loop_js_on() -> Page:
    script = (
        "<script>let ok = 0; const data = new Uint8Array(1024); for (let i = 0; i < 100000; i++) "
        "if (navigator.sendBeacon('http://127.0.0.1:__PORT__/b' + i, data)) ok++; "
        "console.log('queued ' + ok);</script>"
    )
    return Page(doc("<p>beacon</p>" + script))


@case(
    "Images whose load handlers insert more images: a chain of 100,000 data: URL images, each "
    "created in the load handler of the one before, with JavaScript on (feature 10b): the "
    "events are delivered in slices of 4,096, the loop gives the page time between them, and "
    "the finished watches of images outside the document leave the table of 65,536 (ADR 0028 "
    "section 16; the review fix of 10b: the limit used to act here)",
    forbid_log="images are watched for load events",
    swb_args=JS,
    time_limit_s=30,
)
def image_load_chain_js_on() -> Page:
    svg = (
        "data:image/svg+xml,%3Csvg xmlns=%27http://www.w3.org/2000/svg%27 "
        "width=%271%27 height=%271%27/%3E"
    )
    script = (
        "<script>let n = 0; const make = () => { const i = new Image(); "
        f"i.onload = i.onerror = () => {{ if (++n < 100000) make(); }}; i.src = '{svg}#' + n; }}; "
        "make(); setTimeout(() => console.log('loads ' + n), 3000);</script>"
    )
    return Page(doc("<p>images</p>" + script))


@case(
    "100 XMLHttpRequest calls to a server that answers with 60 MiB, each response kept in an "
    "array as an ArrayBuffer-typed request, with JavaScript on and 4 s of settling (review of "
    "10b: 3 GB before): the bytes of a response count against the heap of 256 MiB when they "
    "arrive (ADR 0028 section 16), so the heap limit stops the page after a few responses",
    server="big",
    expect_log="reached a limit",
    swb_args=("--js", "--settle-ms", "4000"),
    time_limit_s=20,
    rss_limit_mib=1100,
)
def xhr_retained_responses_js_on() -> Page:
    script = (
        "<script>let done = 0; const keep = []; for (let i = 0; i < 100; i++) { "
        "const x = new XMLHttpRequest(); x.open('GET', 'http://127.0.0.1:__PORT__/b' + i); "
        "x.responseType = 'arraybuffer'; x.onloadend = () => { done++; }; keep.push(x); "
        "x.send(); }</script>"
    )
    return Page(doc("<p>xhr</p>" + script))


@case(
    "400 blobs of 32 MiB, each reduced to a slice of 1 byte that the page keeps, and the same "
    "slices passed to URL.createObjectURL, with JavaScript on (review of 10b: abort at 8 GB "
    "before): a small slice of a large blob is a copy, and a blob charges the vectors it "
    "keeps alive, not its length (ADR 0028 section 16)",
    swb_args=JS,
    time_limit_s=30,
    rss_limit_mib=1100,
)
def blob_slices_of_large_parents_js_on() -> Page:
    script = (
        "<script>const keep = [], urls = []; let n = 0; for (let i = 0; i < 400; i++) { "
        "const b = new Blob([new Uint8Array(32 << 20)]); keep.push(b.slice(0, 1)); "
        "urls.push(URL.createObjectURL(b.slice(1, 3))); n++; } "
        "console.log('slices ' + n);</script>"
    )
    return Page(doc("<p>blob</p>" + script))


@case(
    "300 XMLHttpRequest calls and 300 fetch() calls to a Blob URL of a 32 MiB blob in one "
    "task, with JavaScript on and 4 s of settling (review of 10b: abort at 60 MiB x 300 "
    "before): a request of a Blob URL keeps the blob's handle until its task runs and copies "
    "the bytes then; at most 256 wait, the others fail (ADR 0028 section 16)",
    expect_log="requests of Blob URLs wait",
    swb_args=("--js", "--settle-ms", "4000"),
    time_limit_s=30,
    rss_limit_mib=1100,
)
def blob_url_request_loop_js_on() -> Page:
    script = (
        "<script>const b = new Blob([new Uint8Array(32 << 20)]); const u = URL.createObjectURL(b); "
        "let ok = 0, failed = 0; for (let i = 0; i < 300; i++) { const x = new XMLHttpRequest(); "
        "x.open('GET', u); x.onload = () => ok++; x.onerror = () => failed++; x.send(); } "
        "for (let i = 0; i < 300; i++) fetch(u).then(() => ok++, () => failed++); "
        "setTimeout(() => console.log('ok ' + ok + ' failed ' + failed), 8000);</script>"
    )
    return Page(doc("<p>blob url</p>" + script))


@case(
    "200 blobs of 32 MiB, each dropped right after an XMLHttpRequest and a fetch() of its Blob "
    "URL (revoked at once), then 200 more, each dropped right after a FileReader read, with "
    "JavaScript on and 4 s of settling (delta review of 10b: abort at 3.6 GB before): the "
    "requests and reads that wait for their task keep at most 256 MiB of blobs alive, the "
    "others fail (ADR 0028 section 16)",
    expect_log="FileReader reads wait with more than",
    swb_args=("--js", "--settle-ms", "4000"),
    time_limit_s=40,
    rss_limit_mib=1100,
)
def blobs_dropped_while_requests_wait_js_on() -> Page:
    script = (
        "<script>let ok = 0, failed = 0; for (let i = 0; i < 200; i++) { "
        "let b = new Blob([new Uint8Array(32 << 20)]); const u = URL.createObjectURL(b); "
        "const x = new XMLHttpRequest(); x.open('GET', u); x.onload = () => ok++; "
        "x.onerror = () => failed++; x.send(); fetch(u).then(() => ok++, () => failed++); "
        "URL.revokeObjectURL(u); b = null; } "
        "for (let i = 0; i < 200; i++) { let b = new Blob([new Uint8Array(32 << 20)]); "
        "const r = new FileReader(); r.onload = () => ok++; r.onerror = () => failed++; "
        "r.readAsArrayBuffer(b); b = null; } "
        "setTimeout(() => console.log('ok ' + ok + ' failed ' + failed), 8000);</script>"
    )
    return Page(doc("<p>dropped blobs</p>" + script))


@case(
    "30 XMLHttpRequest calls to a server that answers with 60 MiB, none kept by the page, "
    "each with an onload handler that reads its request (as jQuery's handlers do), with "
    "JavaScript on and 6 s of settling (delta review of 10b: the handler map of the document "
    "kept every request and its response alive, so the heap limit stopped the scripts after "
    "four responses): the handlers live in the request's own data and die with it",
    server="big",
    forbid_log="reached a limit",
    swb_args=("--js", "--settle-ms", "6000"),
    time_limit_s=40,
    rss_limit_mib=1100,
)
def xhr_handlers_of_unreferenced_requests_js_on() -> Page:
    script = (
        "<script>let done = 0; for (let i = 0; i < 30; i++) { "
        "const x = new XMLHttpRequest(); x.open('GET', 'http://127.0.0.1:__PORT__/b' + i); "
        "x.responseType = 'arraybuffer'; x.onload = () => { done += x.response.byteLength > 0; }; "
        "x.send(); }</script>"
    )
    return Page(doc("<p>xhr handlers</p>" + script))


@case(
    "setRequestHeader with a value of 1 MiB 2,000 times on one header, and 300,000 headers "
    "with different names on one more request, with JavaScript on (review of 10b: abort at "
    "8 GB before): a request keeps at most 256 headers and 64 KiB of them, the rest is "
    "dropped with a warning (ADR 0028 section 16)",
    expect_log="a request holds at most",
    swb_args=JS,
    time_limit_s=30,
    rss_limit_mib=1100,
)
def xhr_request_headers_js_on() -> Page:
    script = (
        "<script>const s = 'a'.repeat(1 << 20); const x = new XMLHttpRequest(); "
        "x.open('GET', 'http://127.0.0.1:1/'); for (let i = 0; i < 2000; i++) "
        "x.setRequestHeader('x-a', s); const y = new XMLHttpRequest(); "
        "y.open('GET', 'http://127.0.0.1:1/'); for (let i = 0; i < 300000; i++) "
        "y.setRequestHeader('x-' + i, 'v'); console.log('headers set');</script>"
    )
    return Page(doc("<p>headers</p>" + script))


@case(
    "80 batches of 1,000 new Image() objects with a load handler, all loaded and garbage "
    "(review of 10b: no load event after 65,536 images before), then one more image, with "
    "JavaScript on: the table that watches the images drops the finished watches of images "
    "outside the document, so the last image still gets its load event (ADR 0028 section 16)",
    forbid_log="images are watched for load events",
    swb_args=JS,
    time_limit_s=30,
)
def image_watches_of_a_long_page_js_on() -> Page:
    svg = (
        "data:image/svg+xml,%3Csvg xmlns=%27http://www.w3.org/2000/svg%27 "
        "width=%271%27 height=%271%27/%3E"
    )
    # Each batch starts in the load handler of the last image of the one
    # before (images that script created delay the window's load event, so
    # swb stays); without events after 65,536 images the chain stops.
    script = (
        "<script>let fired = 0, round = 0; const batch = () => { round++; "
        "for (let i = 0; i < 1000; i++) { const im = new Image(); "
        "im.onload = () => { fired++; if (i === 999) next(); }; "
        f"im.src = '{svg}#' + round; }} }}; const next = () => {{ if (round < 80) batch(); else {{ "
        f"const l = new Image(); l.onload = () => console.log('lastload true fired ' + fired); "
        f"l.src = '{svg}#last'; }} }}; batch();</script>"
    )
    return Page(doc("<p>images</p>" + script))


@case(
    "DOMMatrix from strings of 18 MiB (5 times) and of 0.9 MiB (30 times), 200,000 matrices "
    "kept by the page, and 20,000 writes of long transition and will-change values to one "
    "element, with JavaScript on (feature 11c1): a matrix string over 1 MiB is a syntax error "
    "without being tokenized, a string inside the limit is parsed in time, and a matrix or a "
    "style value costs no more than its data (ADR 0028 sections 10, 13 and 16)",
    expect_log="matrices 5 200000",
    swb_args=JS,
    time_limit_s=30,
)
def dom_matrix_strings_js_on() -> Page:
    script = (
        "<script>const big = 'scale(1) '.repeat(1 << 21); let errors = 0; "
        "for (let i = 0; i < 5; i++) { try { new DOMMatrix(big); } catch (e) { errors++; } } "
        "const ok = 'translate(1px) '.repeat(60000); "
        "for (let i = 0; i < 30; i++) new DOMMatrix(ok); "
        "const keep = []; for (let i = 0; i < 200000; i++) "
        "keep.push(new DOMMatrix([1, 0, 0, 1, i, 0]).translate(1, 2)); "
        "const s = document.body.style; for (let i = 0; i < 20000; i++) { "
        "s.transition = 'opacity 1s ease ' + i + 'ms, width 2s steps(4, end), color 3s'; "
        "s.willChange = 'transform, opacity, ' + i; } "
        "console.warn('matrices ' + errors + ' ' + keep.length);</script>"
    )
    return Page(doc("<p>matrices</p>" + script))


@case(
    "1,000,000 calls of alert, confirm and prompt with messages of 2.6 KiB, with JavaScript "
    "on (feature 11c2): a dialog is dismissed at once and costs one string conversion (about "
    "12 microseconds), so the time limit of 5 s ends the loop; the log (RUST_LOG=debug) shows "
    "the first 100 dialogs (ADR 0028 section 4)",
    expect_log="the time limit was exceeded",
    swb_args=JS,
    time_limit_s=25,
)
def dialogs_loop_js_on() -> Page:
    script = (
        "<script>const m = 'a long message '.repeat(200); let n = 0; "
        "for (let i = 0; i < 1000000; i++) { "
        "if (i % 3 === 0) alert(m + i); else if (i % 3 === 1) { if (confirm(m)) n++; } "
        "else if (prompt(m, m) !== null) n++; }</script>"
    )
    return Page(doc("<p>dialogs</p>" + script))


@case(
    "200,000 print() calls with listeners after the load and 50,000 writes to "
    "navigator.clipboard, with JavaScript on (feature 11c2): each print() fires two events, "
    "and a rejected clipboard promise costs no more than its data, with 2 s of settle time "
    "(ADR 0028 sections 4 and 13)",
    expect_log="prints 200000 clipboard 50000",
    swb_args=("--js", "--settle-ms", "2000"),
    time_limit_s=30,
)
def print_clipboard_loop_js_on() -> Page:
    script = (
        "<script>let prints = 0, rejected = 0; "
        "addEventListener('beforeprint', () => { prints++; }); "
        "addEventListener('load', () => setTimeout(() => { "
        "for (let i = 0; i < 200000; i++) print(); "
        "const m = 'a long message '.repeat(200); "
        "for (let i = 0; i < 50000; i++) "
        "navigator.clipboard.writeText(m).catch(() => { rejected++; }); "
        "Promise.resolve().then(() => setTimeout(() => "
        "console.warn('prints ' + prints + ' clipboard ' + rejected), 100)); }, 0));</script>"
    )
    return Page(doc("<p>print</p>" + script))


@case(
    "A flood of window.postMessage to the own window with JavaScript on (M9 feature 1): 100 "
    "messages of 4 MiB and then 200,000 small ones in one task, and a listener that posts a "
    "message for each message that it gets, without end. At most 65,536 messages and 256 MiB "
    "of serialized "
    "bytes wait; later calls drop their message with one warning, and the task source runs "
    "the messages as tasks, so the page settles after --settle-ms (ADR 0031 section 11)",
    expect_log="further messages are dropped",
    swb_args=("--js", "--settle-ms", "1500"),
    time_limit_s=30,
    rss_limit_mib=2048,
)
def post_message_flood_to_self_js_on() -> Page:
    script = (
        "<script>let got = 0, loops = 0; "
        "addEventListener('message', (e) => { if (e.data === 'loop') { loops++; "
        "postMessage('loop', '*'); } else got++; }); "
        "const big = 'x'.repeat(4 << 20); "
        "for (let i = 0; i < 100; i++) postMessage(big + i, '*'); "
        "for (let i = 0; i < 200000; i++) postMessage(i, '*'); "
        "setTimeout(() => { postMessage('loop', '*'); "
        "setTimeout(() => console.log('got ' + got + ' loops ' + loops), 700); }, 300);"
        "</script>"
    )
    return Page(doc("<p>messages</p>" + script))


@case(
    "A MessageChannel ping-pong without end (M9 feature 9): two ports send each other a message "
    "from the handler of the last one, while a timer counts and an animation frame callback runs. "
    "Each message is a task with no timer clamp; the event loop orders the tasks by queuing time, "
    "so the timer and the frames get their turns and --settle-ms ends (ADR 0031 sections 7 and 11)",
    swb_args=("--js", "--settle-ms", "1500"),
    time_limit_s=10,
    rss_limit_mib=320,
)
def message_channel_ping_pong_js_on() -> Page:
    script = (
        "<script>const c = new MessageChannel(); let pongs = 0, ticks = 0, frames = 0; "
        "c.port1.onmessage = (e) => c.port2.postMessage(e.data); "
        "c.port2.onmessage = (e) => { pongs++; c.port1.postMessage(e.data); }; "
        "c.port1.postMessage('ping'); "
        "setInterval(() => ticks++, 10); "
        "const frame = () => { frames++; requestAnimationFrame(frame); }; "
        "requestAnimationFrame(frame); "
        "setTimeout(() => console.log('pongs ' + pongs + ' ticks ' + ticks + ' frames ' + "
        "frames), 1000);"
        "</script>"
    )
    return Page(doc("<p>ping-pong</p>" + script))


@case(
    "One message with 10,000 ports (M9 feature 9): 5,000 channels, both ports of each in the "
    "transfer list. The ports travel in the record, the receiver gets 10,000 new objects, and the "
    "ports that nobody holds go with the collector (ADR 0031 sections 7 and 11)",
    swb_args=("--js", "--settle-ms", "1500"),
    time_limit_s=10,
    rss_limit_mib=256,
)
def message_channel_transfer_10000_ports_js_on() -> Page:
    script = (
        "<script>const hub = new MessageChannel(); const ports = []; "
        "for (let i = 0; i < 5000; i++) { const c = new MessageChannel(); "
        "ports.push(c.port1, c.port2); } "
        "hub.port2.onmessage = (e) => console.log('received ' + e.ports.length + ' ' + "
        "e.ports.every((p) => p instanceof MessagePort)); "
        "hub.port1.postMessage('many', ports);</script>"
    )
    return Page(doc("<p>ports</p>" + script))


@case(
    "A million new MessageChannel() in a loop that keeps none (M9 feature 9; some setImmediate "
    "polyfills make a channel per call): the collector frees the pairs and the registry follows, "
    "so the limit of 65,536 live ports is never reached (ADR 0031 sections 7 and 11)",
    swb_args=("--js", "--settle-ms", "500"),
    time_limit_s=10,
    rss_limit_mib=512,
    forbid_log="message ports are live",
)
def message_channel_million_unkept_js_on() -> Page:
    script = (
        "<script>for (let i = 0; i < 1000000; i++) new MessageChannel(); "
        "console.log('made 1000000');</script>"
    )
    return Page(doc("<p>channels</p>" + script))


@case(
    "70,000 channels whose second port travels in a message that only it holds (M9 feature 9, "
    "review fix): c.port1.postMessage(1, [c.port2]) puts the port into its own queue, a cycle "
    "that the collector cannot see. The registry frees such entries and their waiting messages, "
    "so the limits of 65,536 ports and 65,536 waiting messages are not reached, and a message "
    "after the loop is delivered (ADR 0031 section 7); the case fails if swb logs the warning "
    "about the port limit",
    swb_args=("--js", "--settle-ms", "500"),
    time_limit_s=10,
    rss_limit_mib=512,
    forbid_log="message ports are live",
)
def message_channel_travelling_cycle_js_on() -> Page:
    script = (
        "<script>let error = ''; "
        "try { for (let i = 0; i < 70000; i++) { const c = new MessageChannel(); "
        "c.port1.postMessage(1, [c.port2]); } } catch (e) { error = e.name; } "
        "const d = new MessageChannel(); "
        "d.port2.onmessage = (e) => console.log('after ' + e.data + error); "
        "d.port1.postMessage(7);</script>"
    )
    return Page(doc("<p>channels</p>" + script))


@case(
    "70,000 MessageChannel objects that the page keeps (M9 feature 9): at most 65,536 ports are "
    "live; new MessageChannel() past it throws a RangeError and swb logs a warning (Chromium has "
    "no limit; ADR 0031 section 11)",
    expect_log="message ports are live",
    swb_args=("--js", "--settle-ms", "500"),
    time_limit_s=10,
    rss_limit_mib=384,
)
def message_channel_70000_live_js_on() -> Page:
    script = (
        "<script>const keep = []; let error = ''; "
        "try { for (let i = 0; i < 70000; i++) keep.push(new MessageChannel()); } "
        "catch (e) { error = e.name; } "
        "console.log('kept ' + keep.length + ' ' + error);</script>"
    )
    return Page(doc("<p>channels</p>" + script))


@case(
    "A flood of port messages to a port without a listener (M9 feature 9): the queue of the port "
    "holds at most 65,536 waiting messages (window and ports together); a later message is "
    "dropped with a warning, the page settles (Chromium has no limit; ADR 0031 section 11)",
    expect_log="further messages are dropped",
    swb_args=("--js", "--settle-ms", "1500"),
    time_limit_s=10,
    rss_limit_mib=1024,
)
def message_port_flood_without_listener_js_on() -> Page:
    script = (
        "<script>const c = new MessageChannel(); "
        "const big = 'x'.repeat(4 << 20); "
        "for (let i = 0; i < 100; i++) c.port1.postMessage(big + i); "
        "for (let i = 0; i < 300000; i++) c.port1.postMessage(i); "
        "let got = 0; setTimeout(() => { c.port2.onmessage = () => { got++; }; "
        "setTimeout(() => console.log('got ' + got), 700); }, 300);</script>"
    )
    return Page(doc("<p>messages</p>" + script))
