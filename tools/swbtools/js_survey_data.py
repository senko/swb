"""Pure functions of `swbtools js-survey`: they turn the raw data of the
tracer (js_tracer.js) and of Chromium into the numbers of the survey, and
render the Markdown summary. No browser access, so unit tests cover them."""

import re
import statistics
from collections import Counter
from collections.abc import Collection, Iterable, Mapping
from html.parser import HTMLParser
from typing import Any

PHASES = ("parse", "load", "after")
"""`parse`: until DOMContentLoaded. `load`: until the load event. `after`:
the rest (scroll and wait)."""

KINDS = ("call", "get", "set", "construct")

METRIC_NAMES = (
    "ScriptDuration",
    "TaskDuration",
    "LayoutCount",
    "LayoutDuration",
    "RecalcStyleCount",
    "RecalcStyleDuration",
    "Nodes",
    "JSHeapUsedSize",
)
"""The Chrome DevTools Protocol `Performance.getMetrics` values that the
survey keeps."""

# ---- API areas ----

_AREA_RULES: tuple[tuple[str, str], ...] = (
    # Layout queries by member name come first: they belong to many interfaces.
    (
        r"\.(getBoundingClientRect|getClientRects|offset(Width|Height|Top|Left|Parent)|"
        r"client(Width|Height|Top|Left)|scroll(Width|Height|Top|Left|X|Y|To|By|IntoView|"
        r"IntoViewIfNeeded)|inner(Width|Height)|outer(Width|Height)|page(X|Y)Offset|"
        r"devicePixelRatio|visualViewport|elementFromPoint|elementsFromPoint|"
        r"getComputedStyle|matchMedia|getBoxQuads|checkVisibility)$",
        "cssom-layout",
    ),
    (
        r"^(CSS|CSSStyleDeclaration|CSSStyleSheet|CSSRule\w*|StyleSheet\w*|MediaQuery\w*|"
        r"FontFace\w*|Screen|CSS\w+)\.",
        "cssom-layout",
    ),
    (
        r"^window\.(setTimeout|setInterval|clearTimeout|clearInterval|requestAnimationFrame|"
        r"cancelAnimationFrame|requestIdleCallback|cancelIdleCallback|queueMicrotask)\|?",
        "timers",
    ),
    (r"^window\.(fetch)$", "network"),
    (r"^(XMLHttpRequest\w*|Request|Response|Headers|EventSource|FormData)\.", "network"),
    (r"^Navigator\.sendBeacon$", "network"),
    (r"^(Storage|IDB\w*|Cache\w*|StorageManager|CookieStore\w*|Database)\.", "storage"),
    (r"^Document\.cookie$", "storage"),
    (r"^window\.(localStorage|sessionStorage|indexedDB|caches|cookieStore)$", "storage"),
    (r"^\w*Observer\.", "observers"),
    (r"^window\.\w*Observer$", "observers"),
    (r"^(History|Location|PopStateEvent|HashChangeEvent|Navigation\w*)\.", "history-location"),
    (r"^window\.(history|location|navigation)$", "history-location"),
    (
        r"^(HTMLCanvasElement|CanvasRenderingContext2D|OffscreenCanvas\w*|WebGL\w*|ImageData|"
        r"ImageBitmap\w*|Path2D|Canvas\w+)\.",
        "canvas",
    ),
    (
        r"^(Worker|SharedWorker|ServiceWorker\w*|WebSocket|MessagePort|MessageChannel|"
        r"BroadcastChannel|WebAssembly|MessageEvent|WorkerGlobalScope)\.",
        "workers-other",
    ),
    (r"^window\.(postMessage|open|close|alert|confirm|prompt|print|focus|blur)$", "workers-other"),
    (r"^(AbortController|AbortSignal|EventTarget|\w*Event|CustomEvent)\.", "events"),
    (r"^window\.(addEventListener|removeEventListener|dispatchEvent|on\w+|event)$", "events"),
    (r"^HTML\w*Element\.", "html-elements"),
    (
        r"^(Image|Option|Audio|DOMStringMap|ValidityState|Blob|File\w*|URL\w*|"
        r"TextEncoder|TextDecoder|DataTransfer\w*)\.",
        "html-elements",
    ),
    (
        r"^(Node|Element|Document|DocumentFragment|DocumentType|ShadowRoot|CharacterData|"
        r"Text|Comment|Attr|NamedNodeMap|NodeList|HTMLCollection|HTMLDocument|DOMTokenList|"
        r"Range|Selection|TreeWalker|NodeIterator|DOMParser|XMLSerializer|DOMImplementation|"
        r"DOMRect\w*|DOMPoint\w*|DOMMatrix\w*|CustomElementRegistry|ParentNode|ChildNode|"
        r"AbstractRange|StaticRange|XPath\w*|SVG\w*|Animation\w*|AnimationTimeline|"
        r"DocumentTimeline|HTMLAllCollection|HTMLFormControlsCollection|HTMLOptionsCollection)\.",
        "dom-core",
    ),
    (
        r"^(Performance\w*|Navigator|Crypto|SubtleCrypto|Intl|console|Notification|"
        r"Permissions\w*|Clipboard\w*|Geolocation|Media\w*|Speech\w*|Gamepad\w*|"
        r"BatteryManager|NetworkInformation|UserActivation|ScreenOrientation)\.",
        "browser-misc",
    ),
    (r"^(window|Window)\.", "window-misc"),
)

AREAS = (
    "dom-core",
    "events",
    "html-elements",
    "cssom-layout",
    "timers",
    "network",
    "storage",
    "observers",
    "history-location",
    "canvas",
    "workers-other",
    "browser-misc",
    "window-misc",
    "other",
)

_COMPILED_RULES = tuple((re.compile(pattern), area) for pattern, area in _AREA_RULES)


def area_of(key: str) -> str:
    """Returns the spec area of a member key `Interface.member` (a name from
    `AREAS`). The first matching rule wins."""
    for pattern, area in _COMPILED_RULES:
        if pattern.search(key):
            return area
    return "other"


# ---- Trace data ----


def merge_members(counts: Mapping[str, Mapping[str, int]]) -> dict[str, dict[str, dict[str, int]]]:
    """Turns the tracer's `counts` (phase -> `"Interface.member|kind"` ->
    n) into `member -> kind -> phase -> n`."""
    merged: dict[str, dict[str, dict[str, int]]] = {}
    for phase, table in counts.items():
        for composite, number in table.items():
            key, _, kind = composite.rpartition("|")
            merged.setdefault(key, {}).setdefault(kind, {})[phase] = number
    return merged


def member_total(kinds: Mapping[str, Mapping[str, int]], only: Iterable[str] | None = None) -> int:
    """Total operations of one member over `only` kinds (default all) and all phases."""
    wanted = set(only) if only is not None else None
    return sum(
        sum(phases.values()) for kind, phases in kinds.items() if wanted is None or kind in wanted
    )


def phase_total(members: Mapping[str, Mapping[str, Mapping[str, int]]]) -> dict[str, int]:
    """Total operations per phase."""
    totals = dict.fromkeys(PHASES, 0)
    for kinds in members.values():
        for phases in kinds.values():
            for phase, number in phases.items():
                totals[phase] = totals.get(phase, 0) + number
    return totals


def area_table(
    members: Mapping[str, Mapping[str, Mapping[str, int]]],
) -> dict[str, dict[str, int]]:
    """Per area: distinct members used (`members`) and operations (`ops`)."""
    table: dict[str, dict[str, int]] = {}
    for key, kinds in members.items():
        row = table.setdefault(area_of(key), {"members": 0, "ops": 0})
        row["members"] += 1
        row["ops"] += member_total(kinds)
    return dict(sorted(table.items(), key=lambda item: AREAS.index(item[0])))


def top_members(
    members: Mapping[str, Mapping[str, Mapping[str, int]]], limit: int, area: str | None = None
) -> list[tuple[str, int, dict[str, int]]]:
    """The most used members: (key, operations, operations per phase)."""
    rows = []
    for key, kinds in members.items():
        if area is not None and area_of(key) != area:
            continue
        per_phase = Counter[str]()
        for phases in kinds.values():
            per_phase.update(phases)
        rows.append((key, member_total(kinds), {p: per_phase.get(p, 0) for p in PHASES}))
    rows.sort(key=lambda row: (-row[1], row[0]))
    return rows[:limit]


def merge_hists(
    per_phase: Mapping[str, Mapping[str, Mapping[str, int]]], limit: int = 50
) -> dict[str, list[dict[str, Any]]]:
    """Merges the tracer's argument histograms (phase -> name -> value -> n)
    into name -> the `limit` most frequent values with their count per phase."""
    merged: dict[str, dict[str, Counter[str]]] = {}
    for phase, tables in per_phase.items():
        for name, values in tables.items():
            for value, number in values.items():
                merged.setdefault(name, {}).setdefault(value, Counter())[phase] += number
    result: dict[str, list[dict[str, Any]]] = {}
    for name, values in merged.items():
        rows = [
            {
                "value": value,
                "total": sum(counter.values()),
                **{p: counter.get(p, 0) for p in PHASES},
            }
            for value, counter in values.items()
        ]
        rows.sort(key=lambda row: (-row["total"], row["value"]))
        result[name] = rows[:limit]
    return result


def event_types(
    per_phase: Mapping[str, Mapping[str, Mapping[str, int]]], limit: int = 40
) -> list[tuple[str, int, int]]:
    """Counts the listeners by event type, from the `addEventListener` and
    `on-handler` histograms (values `"type @ target"`): (type, listeners,
    distinct targets), most frequent first."""
    listeners = Counter[str]()
    targets: dict[str, set[str]] = {}
    for tables in per_phase.values():
        for name in ("addEventListener", "on-handler"):
            for value, number in tables.get(name, {}).items():
                event, _, target = value.partition(" @ ")
                listeners[event] += number
                targets.setdefault(event, set()).add(target)
    rows = [(event, number, len(targets[event])) for event, number in listeners.items()]
    rows.sort(key=lambda row: (-row[1], row[0]))
    return rows[:limit]


def merge_sums(
    per_phase: Mapping[str, Mapping[str, Mapping[str, int]]],
) -> dict[str, dict[str, Any]]:
    """Merges the tracer's sums (phase -> name -> {count, total}) into name
    -> count and total characters, overall and per phase."""
    result: dict[str, dict[str, Any]] = {}
    for phase, table in per_phase.items():
        for name, entry in table.items():
            row = result.setdefault(name, {"count": 0, "total": 0, "by_phase": {}})
            row["count"] += entry["count"]
            row["total"] += entry["total"]
            row["by_phase"][phase] = {"count": entry["count"], "total": entry["total"]}
    return result


def top_counter(table: Mapping[str, int], limit: int) -> list[tuple[str, int]]:
    """The `limit` largest entries of a count table."""
    return sorted(table.items(), key=lambda item: (-item[1], item[0]))[:limit]


INTERACTION_PREFIXES = (
    "click",
    "dblclick",
    "auxclick",
    "contextmenu",
    "key",
    "input",
    "beforeinput",
    "change",
    "focus",
    "blur",
    "submit",
    "reset",
    "pointer",
    "mouse",
    "touch",
    "wheel",
    "drag",
    "drop",
    "select",
    "toggle",
    "copy",
    "cut",
    "paste",
)


def is_interaction_event(event: str) -> bool:
    """True for event types that a user action causes."""
    return event.startswith(INTERACTION_PREFIXES)


def interaction_listeners(rows: Iterable[Mapping[str, Any]]) -> list[dict[str, Any]]:
    """Filters the rows of the `addEventListener` and `on-handler`
    histograms (`"type @ target"`) to the user-input event types."""
    return [row for row in rows if is_interaction_event(str(row["value"]).split(" @ ", 1)[0])]


def merge_mutations(mutations: Mapping[str, Mapping[str, Any]], limit: int = 30) -> dict[str, Any]:
    """Cuts the tag and attribute tables of the mutation stats to `limit`."""
    result: dict[str, Any] = {}
    for part, stats in mutations.items():
        row = dict(stats)
        for field in ("added_by_tag", "removed_by_tag", "attributes_by_name"):
            row[field] = top_counter(stats.get(field, {}), limit)
        result[part] = row
    return result


def summarize_trace(raw: Mapping[str, Any]) -> dict[str, Any]:
    """The survey data of one tracer result (`window.__swb_trace__.flush()`)."""
    members = merge_members(raw.get("counts", {}))
    timers = {phase: dict(table) for phase, table in raw.get("timers_run", {}).items()}
    return {
        "members": members,
        "phase_ops": phase_total(members),
        "areas": area_table(members),
        "args": merge_hists(raw.get("args", {})),
        "event_types": event_types(raw.get("args", {})),
        "sums": merge_sums(raw.get("sums", {})),
        "timers_run": timers,
        "inserted_scripts": raw.get("inserted_scripts", []),
        "iframes": raw.get("iframes", []),
        "unwrapped": raw.get("unwrapped", []),
        "wrapped": raw.get("wrapped", {}),
        "mutations": merge_mutations(raw.get("mutations", {})),
        "phase_times_ms": raw.get("phase_times", {}),
        "tracer_errors": raw.get("errors", []),
    }


# ---- Scripts, hosts, requests ----

_SECOND_LEVEL = {"co", "com", "org", "net", "ac", "gov", "edu", "or", "ne", "go"}


def site_of(host: str) -> str:
    """A rough registrable domain: the last two labels, or three when the
    second-level label is a generic one (`co.uk`). Not the public suffix
    list; good enough to tell first-party hosts from third-party ones."""
    host = host.split(":", 1)[0].lower().rstrip(".")
    labels = host.split(".")
    if len(labels) <= 2 or re.fullmatch(r"[\d.]+", host):
        return host
    if labels[-2] in _SECOND_LEVEL and len(labels[-1]) == 2:
        return ".".join(labels[-3:])
    return ".".join(labels[-2:])


def host_of_url(url: str) -> str:
    """The host of an absolute URL, or a label for URLs without one."""
    match = re.match(r"^[a-z][a-z0-9+.-]*://([^/?#]*)", url, re.IGNORECASE)
    if match:
        return match.group(1).rsplit("@", 1)[-1].lower()
    return url.split(":", 1)[0] + ":" if ":" in url else "(none)"


def page_scripts(
    scripts: Iterable[Mapping[str, Any]], tool_prefix: str, ignore_hashes: Collection[str] = ()
) -> list[dict[str, Any]]:
    """Keeps the scripts that the page ran: drops the tool's own scripts
    (source URL starting with `tool_prefix`) and scripts outside the main
    world (Playwright's helpers run in an isolated world) and scripts whose
    `hash` is in `ignore_hashes` (the scripts that Playwright injects into
    every document)."""
    return [
        dict(script)
        for script in scripts
        if script.get("main_world", True)
        and not str(script.get("url", "")).startswith(tool_prefix)
        and script.get("hash") not in ignore_hashes
    ]


def group_scripts(scripts: Iterable[Mapping[str, Any]], page_url: str) -> dict[str, dict[str, Any]]:
    """Groups the scripts that Chromium compiled by origin. Each script has
    `url` ('' for eval and `new Function`), `length` and `module`. The key
    is the host, `(inline or eval)` for the page's own URL and for scripts
    without a URL. Returns key -> count, bytes, modules, party."""
    page_host = host_of_url(page_url)
    page_site = site_of(page_host)
    groups: dict[str, dict[str, Any]] = {}
    for script in scripts:
        url = str(script.get("url", ""))
        if not url:
            key, party = "(eval or Function)", "first"
        elif url.split("#", 1)[0] == page_url.split("#", 1)[0]:
            key, party = "(inline in the page)", "first"
        else:
            key = host_of_url(url)
            party = "first" if site_of(key) == page_site else "third"
        group = groups.setdefault(key, {"count": 0, "bytes": 0, "modules": 0, "party": party})
        group["count"] += 1
        group["bytes"] += int(script.get("length", 0))
        group["modules"] += 1 if script.get("module") else 0
    return dict(sorted(groups.items(), key=lambda item: (-item[1]["bytes"], item[0])))


_JS_TYPES = {
    "",
    "text/javascript",
    "application/javascript",
    "application/x-javascript",
    "text/ecmascript",
    "application/ecmascript",
    "text/jscript",
    "text/livescript",
}


class _ScriptCollector(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.counts = Counter[str]()
        self.has_script = False

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag != "script":
            return
        names = {name: (value or "") for name, value in attrs}
        kind = names.get("type", "").strip().lower()
        self.counts["total"] += 1
        if kind == "module":
            self.counts["module"] += 1
        elif kind not in _JS_TYPES:
            self.counts["data_or_other_type"] += 1
            return
        if "nomodule" in names:
            self.counts["nomodule"] += 1
        if "src" not in names:
            self.counts["inline"] += 1
            return
        self.counts["external"] += 1
        if "async" in names:
            self.counts["async"] += 1
        if "defer" in names:
            self.counts["defer"] += 1
        if kind != "module" and "async" not in names and "defer" not in names:
            self.counts["classic_blocking"] += 1


def script_kinds(html: str) -> dict[str, int]:
    """Counts the `<script>` elements of an HTML text by kind: `total`,
    `external`, `inline`, `classic_blocking` (external, neither async nor
    defer nor module), `async`, `defer`, `module`, `nomodule` and
    `data_or_other_type` (JSON and other non-JavaScript types)."""
    collector = _ScriptCollector()
    collector.feed(html)
    collector.close()
    keys = (
        "total",
        "external",
        "inline",
        "classic_blocking",
        "async",
        "defer",
        "module",
        "nomodule",
        "data_or_other_type",
    )
    return {key: collector.counts.get(key, 0) for key in keys}


def request_summary(
    requests: Iterable[tuple[str, str]], missing: Iterable[str], page_url: str
) -> dict[str, Any]:
    """Counts requests by resource type (`document`, `script`, `xhr`,
    ...), served and missing, from (resource type, URL) pairs and the URLs
    that the fixture lacked. Also the missing requests by host."""
    absent = {url.split("#", 1)[0] for url in missing}
    by_type: dict[str, dict[str, int]] = {}
    missing_hosts = Counter[str]()
    page_site = site_of(host_of_url(page_url))
    third_party = 0
    total = 0
    for kind, url in requests:
        if not url.startswith(("http://", "https://")):
            continue
        total += 1
        row = by_type.setdefault(kind, {"served": 0, "missing": 0})
        if url.split("#", 1)[0] in absent:
            row["missing"] += 1
            missing_hosts[host_of_url(url)] += 1
        else:
            row["served"] += 1
        if site_of(host_of_url(url)) != page_site:
            third_party += 1
    return {
        "total": total,
        "third_party": third_party,
        "by_type": dict(sorted(by_type.items())),
        "missing_by_host": top_counter(missing_hosts, 20),
    }


# ---- Metrics ----


def metrics_from_cdp(metrics: Iterable[Mapping[str, Any]]) -> dict[str, float]:
    """Selects the survey's values from the result list of
    `Performance.getMetrics`."""
    wanted = set(METRIC_NAMES)
    return {m["name"]: float(m["value"]) for m in metrics if m["name"] in wanted}


def median_metrics(runs: list[Mapping[str, float]]) -> dict[str, float]:
    """The median of each metric over several runs."""
    result: dict[str, float] = {}
    for name in METRIC_NAMES:
        values = [run[name] for run in runs if name in run]
        if values:
            result[name] = statistics.median(values)
    return result


def first_line(text: str, limit: int = 200) -> str:
    """The first non-empty line of `text`, cut to `limit` characters."""
    for line in text.splitlines():
        if line.strip():
            return line.strip()[:limit]
    return ""


# ---- Markdown ----


def _table(header: list[str], rows: list[list[Any]]) -> list[str]:
    lines = ["| " + " | ".join(header) + " |", "|" + "|".join("---" for _ in header) + "|"]
    lines += ["| " + " | ".join(str(cell) for cell in row) + " |" for row in rows]
    return lines


def _hist_line(rows: list[Mapping[str, Any]], limit: int) -> str:
    return ", ".join(f"`{row['value']}` {row['total']}" for row in rows[:limit]) or "none"


def render_markdown(name: str, survey: Mapping[str, Any]) -> str:
    """The Markdown summary of one page's survey."""
    trace = survey["trace"]
    lines = [f"## {name}", ""]
    lines.append(f"URL: {survey['url']}; Chromium {survey['chromium']}; run {survey['date']}.")
    lines.append("")
    html = survey["html_scripts"]
    lines.append(
        f"Script elements in the HTML: {html['total']} "
        f"({html['external']} external, {html['inline']} inline, "
        f"{html['async']} async, {html['defer']} defer, {html['module']} module, "
        f"{html['data_or_other_type']} data blocks)."
    )
    compiled = survey["compiled_scripts"]
    total_kb = sum(g["bytes"] for g in compiled.values()) / 1024
    lines.append(
        f"Compiled scripts: {sum(g['count'] for g in compiled.values())}, {total_kb:.0f} KB."
    )
    lines.append("")
    lines += _table(
        ["origin", "party", "scripts", "KB"],
        [
            [k, g["party"], g["count"], f"{g['bytes'] / 1024:.0f}"]
            for k, g in list(compiled.items())[:12]
        ],
    )
    lines.append("")
    ops = trace["phase_ops"]
    lines.append(
        f"API operations: parse {ops['parse']}, load {ops['load']}, after {ops['after']}; "
        f"{len(trace['members'])} distinct members."
    )
    lines.append("")
    lines += _table(
        ["area", "members", "operations"],
        [[a, r["members"], r["ops"]] for a, r in trace["areas"].items()],
    )
    lines.append("")
    top = top_members(trace["members"], 15)
    lines += _table(
        ["member", "ops", "parse", "load", "after"],
        [[f"`{k}`", n, p["parse"], p["load"], p["after"]] for k, n, p in top],
    )
    lines.append("")
    for hist_name in ("addEventListener", "on-handler"):
        rows = trace["args"].get(hist_name, [])
        lines.append(f"{hist_name} (top): {_hist_line(rows, 12)}")
    kinds = ", ".join(f"{e} {n}/{t}" for e, n, t in trace.get("event_types", [])[:20])
    lines.append(f"Listeners by event type (listeners/targets): {kinds or 'none'}")
    lines.append(f"Interaction listeners:{_hist_line(survey['interaction_listeners'], 20)}")
    for hist_name in ("createElement", "setTimeout", "setInterval", "fetch", "XMLHttpRequest.open"):
        rows = trace["args"].get(hist_name, [])
        if rows:
            lines.append(f"{hist_name}: {_hist_line(rows, 10)}")
    for name_, entry in trace["sums"].items():
        lines.append(f"{name_}: {entry['count']} calls, {entry['total']} characters")
    lines.append(f"Timer and frame callbacks run: {trace['timers_run']}")
    lines.append(
        f"Scripts inserted by scripts: {len(trace['inserted_scripts'])}; "
        f"iframes created by scripts: {len(trace['iframes'])}"
    )
    mutations = trace["mutations"]
    for part in ("parse", "later"):
        stats = mutations.get(part)
        if stats:
            lines.append(
                f"Mutations ({part}): {stats['records']} records, "
                f"{stats.get('added_elements_total', 0)} elements added, "
                f"{stats.get('removed_elements_total', 0)} removed, "
                f"{sum(n for _, n in stats.get('attributes_by_name', []))} attribute changes"
            )
    lines.append(f"Unwrappable members: {len(trace['unwrapped'])}")
    lines.append(
        f"Console errors: {len(survey['errors']['console'])}; page errors: "
        f"{len(survey['errors']['pageerror'])}"
    )
    req = survey["requests"]
    lines.append(
        f"Requests: {req['total']} ({req['third_party']} third-party); missing from the fixture: "
        f"{sum(r['missing'] for r in req['by_type'].values())}"
    )
    for label in ("jit", "jitless"):
        m = survey["metrics"][label]["median"]
        if m:
            lines.append(
                f"{label}: script {m.get('ScriptDuration', 0) * 1000:.0f} ms, "
                f"task {m.get('TaskDuration', 0) * 1000:.0f} ms, "
                f"layout {m.get('LayoutCount', 0):.0f} x "
                f"{m.get('LayoutDuration', 0) * 1000:.0f} ms, "
                f"style {m.get('RecalcStyleCount', 0):.0f} x "
                f"{m.get('RecalcStyleDuration', 0) * 1000:.0f} ms, "
                f"nodes {m.get('Nodes', 0):.0f}"
            )
    off = survey["js_off"]
    lines.append(
        f"JS on/off: {off['elements_on']} / {off['elements_off']} elements; "
        f"first viewport differs in {off['diff_fraction'] * 100:.2f}% of the pixels"
    )
    lines.append("")
    return "\n".join(lines)
