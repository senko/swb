"""`gui_latency.play` with a fake display: the order of the URL reads."""

from swbtools import gui_latency
from swbtools.traces import Event, Trace


class FakeDisplay:
    """Records what `play` sends."""

    def __init__(self) -> None:
        self.sent: list[str] = []

    def motion(self, x: int, y: int) -> None:
        self.sent.append(f"move {x},{y}")

    def button(self, button: int, pressed: bool) -> None:
        self.sent.append(f"{'press' if pressed else 'release'} {button}")


def test_a_url_read_comes_after_its_release_and_before_later_events():
    events = (
        Event(0.0, "press"),
        Event(0.01, "release"),
        Event(0.02, "move", x=5, y=5),
        Event(0.5, "press"),
        Event(0.51, "release"),
    )
    trace = Trace("t", "", None, 0.52, events)
    display = FakeDisplay()
    reads: list[str] = []

    def read() -> str:
        reads.append(display.sent[-1])
        return f"u{len(reads)}"

    injected = gui_latency.play(display, trace, (100, 100), read)  # type: ignore[arg-type]
    # The first read is due 0.4 s after the first release: after the move
    # at 0.02 s (sent on time), before the press at 0.5 s.
    assert reads == ["move 5,5", "release 1"]
    assert [u["after"] for u in injected.urls] == ["release 1", "release 2"]
    assert [u["url"] for u in injected.urls] == ["u1", "u2"]


def test_no_reads_without_a_reader():
    trace = Trace("t", "", None, 0.05, (Event(0.0, "press"), Event(0.01, "release")))
    injected = gui_latency.play(FakeDisplay(), trace, (100, 100))  # type: ignore[arg-type]
    assert injected.urls == []


def test_a_release_within_the_delay_moves_the_pending_read():
    events = (
        Event(0.0, "press"),
        Event(0.01, "release"),
        Event(0.1, "press"),
        Event(0.11, "release"),
    )
    trace = Trace("t", "", None, 0.12, events)
    injected = gui_latency.play(FakeDisplay(), trace, (100, 100), lambda: "u")  # type: ignore[arg-type]
    assert [u["after"] for u in injected.urls] == ["release 2"]
