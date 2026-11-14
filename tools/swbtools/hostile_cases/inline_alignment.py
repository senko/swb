"""`vertical-align: top` and `bottom` and the soft wrap opportunities around atomic
inlines (M10 sub-feature 8a). A line with line-relative boxes is built twice; these
cases check that the work stays linear in the number of boxes and the nesting depth."""

from swbtools.hostile_cases.core import Page, case, doc


@case(
    "10,000 nested inline boxes that alternate `vertical-align: top` and `bottom`, with "
    "text in the innermost: each aligned subtree aligns with the one around it, and the "
    "line is built twice (M10 sub-feature 8a)",
    time_limit_s=20,
)
def vertical_align_top_bottom_nested_10000() -> Page:
    inner = "x"
    for i in range(10000):
        align = "top" if i % 2 else "bottom"
        inner = f"<span style='vertical-align:{align};line-height:{10 + i % 30}px'>{inner}</span>"
    return Page(doc(f"<div>a{inner}b</div>"))


@case(
    "50,000 inline blocks with `vertical-align: top` or `bottom` and growing heights on one "
    "line that does not wrap: the tallest decides the line height and each box gets one "
    "offset (M10 sub-feature 8a)",
    time_limit_s=20,
)
def vertical_align_top_bottom_one_line_50000() -> Page:
    boxes = "".join(
        f"<i style='vertical-align:{'top' if i % 2 else 'bottom'};height:{1 + i % 997}px'></i>"
        for i in range(50000)
    )
    css = "div{white-space:nowrap} i{display:inline-block;width:1px}"
    return Page(doc(f"<div>text{boxes}</div>", css))


@case(
    "20,000 lines in a narrow box, each with a `vertical-align: top` inline block inside a "
    "`super` span and a `bottom` span, and 20,000 `li` items of `display: inline` with an "
    "`inline-flex` box each (breaks after atomic inlines at `unicode-bidi: isolate` edges; "
    "M10 sub-feature 8a)",
    time_limit_s=30,
)
def vertical_align_lines_and_inline_items_20000() -> Page:
    line = (
        "<span style='vertical-align:super'>s<i style='vertical-align:top;height:30px'></i>"
        "</span><span style='vertical-align:bottom;font-size:30px'>b</span> "
    )
    items = "".join(
        "<li><a style='white-space:nowrap;display:inline-flex'>item</a></li>" for _ in range(20000)
    )
    css = (
        "div{width:60px} i{display:inline-block;width:10px} ul{width:200px;padding:0} "
        "li{display:inline}"
    )
    return Page(doc(f"<div>{line * 20000}</div><ul>{items}</ul>", css))
