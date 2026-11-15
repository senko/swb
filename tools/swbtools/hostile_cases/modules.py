"""Module scripts and import maps (ADR 0028 section 8, ADR 0034)."""

import json

from swbtools.hostile_cases.core import (
    JS,
    MIB,
    REPLAY,
    REPLAY_URL,
    Page,
    case,
    doc,
)


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
