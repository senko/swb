"""Input and window control on an X display through `ctypes`, for
`swbtools gui-latency`.

Uses `libX11.so.6` (window size) and `libXtst.so.6` (the XTEST extension:
fake pointer and button events). No Python package is needed (python-xlib
is LGPL, ADR 0003). The display must be a private X server (`Xvfb`): there
is no window manager, so a window resizes with `XMoveResizeWindow`.
"""

import contextlib
import ctypes
import ctypes.util
import time
from types import TracebackType
from typing import Self


class XError(RuntimeError):
    """The X libraries or the display are not usable."""


def _load(names: tuple[str, ...]) -> ctypes.CDLL:
    for name in names:
        with contextlib.suppress(OSError):
            return ctypes.CDLL(name)
    found = ctypes.util.find_library(names[0].removeprefix("lib").split(".")[0])
    if found:
        return ctypes.CDLL(found)
    raise XError(f"cannot load {names[0]}")


_ERROR_HANDLER = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.c_void_p, ctypes.c_void_p)


_ERRORS = [0]
"""The number of X errors so far (the handler counts them)."""


def _count_error(_display: int | None, _event: int | None) -> int:
    """The error handler: counts the error. (The default handler ends the
    process.) The callers that can cause an error check the count after a
    sync."""
    _ERRORS[0] += 1
    return 0


_HANDLER = _ERROR_HANDLER(_count_error)
"""A reference to the handler for the lifetime of the module."""


class XDisplay:
    """A connection to an X display."""

    def __init__(self, display: str) -> None:
        self._x11 = _load(("libX11.so.6", "libX11.so"))
        self._xtst = _load(("libXtst.so.6", "libXtst.so"))
        x11, xtst = self._x11, self._xtst
        x11.XOpenDisplay.restype = ctypes.c_void_p
        x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
        x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
        x11.XDefaultRootWindow.restype = ctypes.c_ulong
        x11.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
        x11.XSetErrorHandler.argtypes = [_ERROR_HANDLER]
        x11.XFlush.argtypes = [ctypes.c_void_p]
        x11.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
        x11.XFree.argtypes = [ctypes.c_void_p]
        x11.XQueryTree.argtypes = [
            ctypes.c_void_p,
            ctypes.c_ulong,
            ctypes.POINTER(ctypes.c_ulong),
            ctypes.POINTER(ctypes.c_ulong),
            ctypes.POINTER(ctypes.POINTER(ctypes.c_ulong)),
            ctypes.POINTER(ctypes.c_uint),
        ]
        x11.XGetGeometry.argtypes = [
            ctypes.c_void_p,
            ctypes.c_ulong,
            ctypes.POINTER(ctypes.c_ulong),
            ctypes.POINTER(ctypes.c_int),
            ctypes.POINTER(ctypes.c_int),
            ctypes.POINTER(ctypes.c_uint),
            ctypes.POINTER(ctypes.c_uint),
            ctypes.POINTER(ctypes.c_uint),
            ctypes.POINTER(ctypes.c_uint),
        ]
        x11.XMoveResizeWindow.argtypes = [
            ctypes.c_void_p,
            ctypes.c_ulong,
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_uint,
            ctypes.c_uint,
        ]
        xtst.XTestFakeMotionEvent.argtypes = [
            ctypes.c_void_p,
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_ulong,
        ]
        xtst.XTestFakeButtonEvent.argtypes = [
            ctypes.c_void_p,
            ctypes.c_uint,
            ctypes.c_int,
            ctypes.c_ulong,
        ]
        xtst.XTestQueryExtension.argtypes = [
            ctypes.c_void_p,
            *[ctypes.POINTER(ctypes.c_int)] * 4,
        ]
        x11.XSetErrorHandler(_HANDLER)
        handle = x11.XOpenDisplay(display.encode())
        if not handle:
            raise XError(f"cannot open the X display {display}")
        self._display: int | None = handle
        self.root = x11.XDefaultRootWindow(handle)
        numbers = [ctypes.c_int() for _ in range(4)]
        if not xtst.XTestQueryExtension(handle, *(ctypes.byref(n) for n in numbers)):
            self.close()
            raise XError(f"the X display {display} has no XTEST extension")

    def close(self) -> None:
        """Closes the connection."""
        if self._display is not None:
            self._x11.XCloseDisplay(self._display)
            self._display = None

    def __enter__(self) -> Self:
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        self.close()

    def _handle(self) -> int:
        if self._display is None:
            raise XError("the display is closed")
        return self._display

    def children(self) -> list[int]:
        """The windows that are children of the root window."""
        root, parent = ctypes.c_ulong(), ctypes.c_ulong()
        children = ctypes.POINTER(ctypes.c_ulong)()
        count = ctypes.c_uint()
        status = self._x11.XQueryTree(
            self._handle(),
            self.root,
            ctypes.byref(root),
            ctypes.byref(parent),
            ctypes.byref(children),
            ctypes.byref(count),
        )
        if not status:
            return []
        try:
            return [children[i] for i in range(count.value)]
        finally:
            if children:
                self._x11.XFree(children)

    def geometry(self, window: int) -> tuple[int, int, int, int] | None:
        """The position and size `(x, y, width, height)` of a window, or None
        if it does not exist."""
        root = ctypes.c_ulong()
        x, y = ctypes.c_int(), ctypes.c_int()
        width, height, border, depth = (ctypes.c_uint() for _ in range(4))
        status = self._x11.XGetGeometry(
            self._handle(),
            window,
            ctypes.byref(root),
            ctypes.byref(x),
            ctypes.byref(y),
            ctypes.byref(width),
            ctypes.byref(height),
            ctypes.byref(border),
            ctypes.byref(depth),
        )
        if not status:
            return None
        return (x.value, y.value, width.value, height.value)

    def find_window(self, min_width: int = 100) -> int | None:
        """A child of the root window that is at least `min_width` wide (the
        browser's window; the helper windows of the toolkit are small), or
        None."""
        for window in self.children():
            geometry = self.geometry(window)
            if geometry is not None and geometry[2] >= min_width:
                return window
        return None

    def wait_for_window(self, timeout_s: float) -> int:
        """Waits for the browser's window. Raises XError on timeout."""
        deadline = time.monotonic() + timeout_s
        while time.monotonic() < deadline:
            window = self.find_window()
            if window is not None:
                return window
            time.sleep(0.05)
        raise XError("the browser's window did not appear")

    def move_resize(self, window: int, x: int, y: int, width: int, height: int) -> None:
        """Places a window; there is no window manager to ask. Raises XError
        if the server reports an error (for example a window that is gone)."""
        errors = _ERRORS[0]
        self._x11.XMoveResizeWindow(self._handle(), window, x, y, width, height)
        self.sync()
        if _ERRORS[0] != errors:
            raise XError("the X server refused to move and resize the window")

    def motion(self, x: int, y: int) -> None:
        """Moves the pointer to (x, y) in root window coordinates. Sent at
        once (flushed). Raises XError if XTEST refuses the event."""
        if not self._xtst.XTestFakeMotionEvent(self._handle(), -1, x, y, 0):
            raise XError("XTEST refused a pointer motion")
        self._x11.XFlush(self._handle())

    def button(self, button: int, press: bool) -> None:
        """Presses or releases a pointer button (1 to 3; 4 is a wheel notch
        up, 5 down). Sent at once (flushed). Raises XError if XTEST refuses
        the event."""
        if not self._xtst.XTestFakeButtonEvent(self._handle(), button, 1 if press else 0, 0):
            raise XError("XTEST refused a button event")
        self._x11.XFlush(self._handle())

    def sync(self) -> None:
        """Waits until the server handled all requests."""
        self._x11.XSync(self._handle(), 0)
