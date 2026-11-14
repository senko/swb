"""`@property`, `color-mix()`, `content-visibility` and `field-sizing` (ADR 0038
section "7b", M10 sub-feature 7b)."""

from swbtools.hostile_cases.core import Page, case, doc, nest

JS = ("--js", "--settle-ms", "500")


@case(
    "10,000 `@property` rules with distinct names and an element that uses the last one: the "
    "registry is a map, the CSSOM has 10,000 `CSSPropertyRule`s, and a script logs "
    '`registered ok` (ADR 0038 section "7b")',
    time_limit_s=20,
    expect_log="registered ok",
    swb_args=JS,
)
def property_rules_10000() -> Page:
    css = "".join(
        f"@property --p{i} {{ syntax: '<length>'; inherits: {'true' if i % 2 else 'false'}; "
        f"initial-value: {i % 7}px }}"
        for i in range(10000)
    )
    css += "#t { width: var(--p9999); height: 2px }"
    script = (
        "<script>window.onload = () => console.warn('registered ok ' + "
        "document.styleSheets[0].cssRules.length + ' ' + "
        "getComputedStyle(document.getElementById('t')).getPropertyValue('--p9999') + ' ' + "
        "getComputedStyle(document.getElementById('t')).width);</script>"
    )
    return Page(doc("<div id=t></div>" + script, css))


@case(
    "10,000 `@property` rules that do not inherit, and 50,000 elements that each declare one "
    "custom property: the maps do not copy the initial values of the registrations (a missing "
    "entry is the initial value), so memory does not grow with the product, and a script logs "
    '`registered declarers ok` (ADR 0038 section "7b", review fixes)',
    time_limit_s=30,
    rss_limit_mib=1024,
    expect_log="registered declarers ok",
    swb_args=JS,
)
def registered_property_10000_rules_50000_declarers() -> Page:
    css = "".join(
        f"@property --p{i} {{ syntax: '<length>'; inherits: false; initial-value: {i % 7}px }}"
        for i in range(10000)
    )
    css += "i { display: block; height: var(--p1) }"
    body = "<i style='--p1: 2px'></i>" * 50000
    script = (
        "<script>window.onload = () => console.warn('registered declarers ok ' + "
        "document.querySelectorAll('i').length + ' ' + "
        "getComputedStyle(document.querySelector('i')).height + ' ' + "
        "getComputedStyle(document.querySelector('i')).getPropertyValue('--p2'));</script>"
    )
    return Page(doc(body + script, css))


@case(
    "a registered `<length>` property that does not inherit, used by 100,000 elements, a part "
    "of them with their own value in `em` (computed with each font size): the parent's value is "
    "reset for each child, the maps are shared when nothing differs, and a script logs "
    '`registered users ok` (ADR 0038 section "7b")',
    time_limit_s=30,
    expect_log="registered users ok",
    swb_args=JS,
)
def registered_property_100000_users() -> Page:
    css = (
        "@property --w { syntax: '<length>'; inherits: false; initial-value: 3px } "
        "@property --h { syntax: '<length>'; inherits: true; initial-value: 1px } "
        "i { display: block; width: var(--w); height: var(--h) } "
        ".e { --w: 2em; font-size: 10px }"
    )
    body = "<div style='--w: 9px; --h: 2px'>" + "<i></i><i class=e></i>" * 50000 + "</div>"
    script = (
        "<script>window.onload = () => console.warn('registered users ok ' + "
        "document.querySelectorAll('i').length + ' ' + "
        "getComputedStyle(document.querySelector('.e')).width);</script>"
    )
    return Page(doc(body + script, css))


@case(
    "60,000 elements with `content-visibility: auto` and a child each, and 60,000 with "
    "`content-visibility: hidden`: layout and paint containment make a formatting context for "
    'each, and the hidden contents leave the fragment tree (ADR 0038 section "7b")',
    time_limit_s=30,
    expect_log="cv ok",
    swb_args=JS,
)
def content_visibility_120000_boxes() -> Page:
    css = (
        ".a { content-visibility: auto } "
        ".h { content-visibility: hidden; contain-intrinsic-size: 10px 4px }"
    )
    body = "<div class=a><i>x</i></div>" * 60000 + "<div class=h><i>y</i></div>" * 60000
    script = (
        "<script>window.onload = () => console.warn('cv ok ' + "
        "document.querySelectorAll('.h').length + ' ' + "
        "document.querySelector('.h').getBoundingClientRect().height);</script>"
    )
    return Page(doc(body + script, css))


@case(
    "a chain of 1,000 custom properties, each a `color-mix()` of the previous one and "
    "`currentcolor`, used for `background-color`: the substitution stops at the depth limit "
    "and the value is "
    'invalid at computed-value time (ADR 0038 section "7b")',
    time_limit_s=10,
)
def color_mix_chain_1000_custom_properties() -> Page:
    chain = "--c0: red;" + "".join(
        f"--c{i}: color-mix(in srgb, var(--c{i - 1}) 60%, currentcolor);" for i in range(1, 1000)
    )
    css = f"#t {{ {chain} background-color: var(--c999); color: blue; height: 2px }}"
    return Page(doc("<div id=t></div>", css))


@case(
    "`color-mix()` nested 1,000 deep in one declaration, with `currentcolor` at the bottom: the "
    "parser's nesting limit drops the value, and the next declaration still applies (ADR 0038 "
    'section "7b")',
    time_limit_s=10,
)
def color_mix_nested_1000_deep() -> Page:
    value = "red"
    for _ in range(1000):
        value = f"color-mix(in srgb, {value}, blue)"
    css = f"#t {{ background-color: {value}; background-color: rgb(0, 128, 0); height: 2px }}"
    return Page(doc("<div id=t></div>", css))


@case(
    "200,000 elements with distinct `color-mix()` colors (a percentage each, some with "
    "`currentcolor`): the table of mixed colors holds each once, and a script logs "
    '`mixes ok` (ADR 0038 section "7b")',
    time_limit_s=30,
    expect_log="mixes ok",
    swb_args=JS,
)
def color_mix_200000_distinct_colors() -> Page:
    body = "".join(
        f"<i style='background-color: color-mix(in {'oklab' if i % 3 else 'srgb'}, "
        f"{'currentcolor' if i % 2 else 'red'} {i % 100}.{i // 100 % 1000}%, blue)'></i>"
        for i in range(200000)
    )
    script = (
        "<script>window.onload = () => console.warn('mixes ok ' + "
        "getComputedStyle(document.querySelector('i')).backgroundColor);</script>"
    )
    return Page(doc(body + script, "i { display: block; height: 1px }"))


@case(
    "20,000 text fields, text areas and selects with `field-sizing: content` and long values: "
    'each is shaped once for its width, and a script logs `fields ok` (ADR 0038 section "7b")',
    time_limit_s=30,
    expect_log="fields ok",
    swb_args=JS,
)
def field_sizing_20000_controls() -> Page:
    long = "w" * 200
    body = (
        f"<div><input value='{long}'><textarea>{long}\n{long}</textarea>"
        f"<select><option>{long}<option>b</select></div>"
    ) * 7000
    css = "input, textarea, select { field-sizing: content }"
    script = (
        "<script>window.onload = () => console.warn('fields ok ' + "
        "document.querySelectorAll('input').length + ' ' + "
        "document.querySelector('input').getBoundingClientRect().width);</script>"
    )
    return Page(doc(body + script, css))


@case(
    "50,000 nested boxes with `contain: layout`: each is an independent formatting context and "
    "the containing block of its absolute children; the box tree flattens below its depth limit "
    '(ADR 0038 section "7b")',
    time_limit_s=30,
)
def contain_layout_50000_nested() -> Page:
    css = "div { contain: layout paint }"
    body = nest("<div>", "</div>", 50000, "<b style='position:absolute;right:0'>x</b>")
    return Page(doc(body, css))
