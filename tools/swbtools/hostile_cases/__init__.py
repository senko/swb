"""The cases of the hostile-page set (docs/testing.md, "Hostile-page set").

A case is a generator of one page that goes just past a limit that an ADR
documents (or a generic hostile pattern). `swbtools hostile` runs them
(`hostile.py`). To add a case, write a function that returns a `Page` and
decorate it with `@case(...)`. The name of the function is the name of the
case (with `_` as `-`).

The description names the limit and the ADR. `expect_log` is a part of the
warning that swb logs when the limit acts, or several; the case fails with
`inactive` if one is missing, so that a changed limit does not go unnoticed. The
set runs at the log level `warn`: a page that shows that its script reached
its end uses `console.warn`.
Generated pages stay below `MAX_PAGE_BYTES`.

The package: `core.py` has the types, the registry and the helpers that several
modules use; the other modules hold the cases of one topic each (see `MODULES`).
"""

import importlib

from swbtools.hostile_cases.core import CASES, MAX_PAGE_BYTES, Case, Page, case

__all__ = ["CASES", "MAX_PAGE_BYTES", "Case", "Page", "case"]

# The modules register their cases when they are imported, so the order of this list is
# the order in which the cases run.
MODULES = (
    "generic",
    "fonts",
    "inline_svg",
    "dom",
    "observers_fetch",
    "write_cssom_modules",
    "frames",
    "storage_iterator",
)
for _module in MODULES:
    importlib.import_module(f"{__name__}.{_module}")
