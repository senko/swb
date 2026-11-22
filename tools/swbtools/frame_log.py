"""The frame log of the GUI and the latency numbers from it.

The GUI prints two kinds of lines at the debug level of the target
`swb::gui` (`RUST_LOG=swb::gui=debug`; the writer is
`crates/swb/src/gui/framelog.rs`). Times are seconds of `CLOCK_MONOTONIC`:

    batch start=T end=T moves=N wheels=N presses=N releases=N keys=N frame=F
    frame at=T render=D compose=D present=D [full=F rects=N pixels=N]

A `batch` is one batch of window events that had input (from `new_events`
to the end of `about_to_wait`) with the number of input events by kind;
`frame=F` is 1 if the batch requested a redraw, else 0 (a log from before
the field has none). A
`frame` is one presented frame: `at` is the time after `present`; the
other times are durations of the page render, the composition and
`present`; since M13 feature 4b the damage of the page raster follows
(ignored here).

`map_events` gives each injected event the batch that handled it, by order
within its kind: the k-th event of a kind belongs to the batch in which the
running count of that kind first reaches k.
"""

import bisect
import re
from collections.abc import Iterable
from dataclasses import dataclass

NO_FRAME_S = 2.0
"""In a log without the `frame` field of batches, an event that no frame
follows within this time after its batch is counted as having no frame."""

KIND_FIELDS = {
    "move": "moves",
    "wheel": "wheels",
    "press": "presses",
    "release": "releases",
    "key": "keys",
}
"""The trace kinds and the field of `Batch` that counts them."""


@dataclass(frozen=True)
class Batch:
    """One batch of window events that had input."""

    start: float
    end: float
    moves: int = 0
    wheels: int = 0
    presses: int = 0
    releases: int = 0
    keys: int = 0
    frame: bool | None = None
    """Whether the batch requested a redraw; None in an old log."""


@dataclass(frozen=True)
class Frame:
    """One presented frame."""

    at: float
    render: float
    compose: float
    present: float


_LINE = re.compile(r"swb::gui[^\]]*\]\s+(batch|frame)((?:\s+\w+=[0-9.]+)+)\s*$")


def parse_line(text: str) -> Batch | Frame | None:
    """Parses a line of swb's log: a `Batch`, a `Frame`, or None for any
    other line."""
    match = _LINE.search(text)
    if match is None:
        return None
    fields = {k: v for k, _, v in (item.partition("=") for item in match.group(2).split())}
    try:
        if match.group(1) == "batch":
            return Batch(
                start=float(fields["start"]),
                end=float(fields["end"]),
                moves=int(fields["moves"]),
                wheels=int(fields["wheels"]),
                presses=int(fields["presses"]),
                releases=int(fields["releases"]),
                keys=int(fields["keys"]),
                frame=int(fields["frame"]) != 0 if "frame" in fields else None,
            )
        return Frame(
            at=float(fields["at"]),
            render=float(fields["render"]),
            compose=float(fields["compose"]),
            present=float(fields["present"]),
        )
    except (KeyError, ValueError):
        return None


def parse_log(lines: Iterable[str]) -> tuple[list[Batch], list[Frame]]:
    """The batches and the frames of a log, each in the order of the log."""
    batches: list[Batch] = []
    frames: list[Frame] = []
    for line in lines:
        item = parse_line(line)
        if isinstance(item, Batch):
            batches.append(item)
        elif isinstance(item, Frame):
            frames.append(item)
    return batches, frames


def map_events(
    count: int, batches: Iterable[Batch], kind: str, per_event: int = 1
) -> tuple[list[Batch | None], int]:
    """The batch of each of `count` injected events of a kind, in order (None
    for an event that no batch counted), and the number of events of that
    kind that the batches counted. The numbers differ when the GUI saw
    more or fewer events than the tool sent. With `per_event` above 1, each
    injected event makes that many events in the GUI (X11 gives a wheel
    notch as two: the press and the release of the button); an injected
    event is handled when the last of its events is."""
    field = KIND_FIELDS[kind]
    counted = [batch for batch in batches for _ in range(getattr(batch, field))]
    mapped: list[Batch | None] = [
        counted[(k + 1) * per_event - 1] if (k + 1) * per_event <= len(counted) else None
        for k in range(count)
    ]
    return mapped, len(counted)


def next_frame(frames: list[Frame], since: float) -> Frame | None:
    """The first frame presented at or after a time, or None. `frames` is
    sorted by `at`."""
    index = bisect.bisect_left([frame.at for frame in frames], since)
    return frames[index] if index < len(frames) else None


def summary(values: list[float]) -> dict[str, float | int | None]:
    """The count, median, 90th percentile (nearest rank) and maximum of
    values; None for the numbers of an empty list."""
    if not values:
        return {"n": 0, "median": None, "p90": None, "max": None}
    ordered = sorted(values)
    n = len(ordered)
    return {
        "n": n,
        "median": ordered[n // 2],
        "p90": ordered[max(0, -(-n * 9 // 10) - 1)],
        "max": ordered[-1],
    }


def event_latencies(
    injected: list[float],
    batches: list[Batch],
    frames: list[Frame],
    kind: str,
    per_event: int = 1,
) -> dict[str, object]:
    """The latency numbers of the events of one kind. `injected` are the
    times at which the tool sent the events; times are in seconds, the
    results in milliseconds: `to_batch_end` (from the input to the end of
    its batch, all events) and `to_frame` (to the first frame presented at
    or after the start of its batch, for the events whose batch requested a
    frame) as summaries, `no_frame_needed` (events whose batch requested no
    frame; they are not in `to_frame`), `no_frame` (events whose batch
    requested a frame but none was presented after it; in an old log
    without the field: none within `NO_FRAME_S` after the batch),
    `unmapped` (events that no batch counted), and `logged` (the events
    that the batches counted; it equals `injected` times `per_event` when
    all arrived).

    winit delivers the redraw of a turn of its loop after the turn's input
    events and before `about_to_wait`, so the frame that shows a batch's
    input is usually inside the batch (the GUI's wheel, which scrolls in
    `about_to_wait`, draws in the next turn)."""
    mapped, logged = map_events(len(injected), batches, kind, per_event)
    to_batch, to_frame = [], []
    no_frame = no_frame_needed = 0
    for sent, batch in zip(injected, mapped, strict=True):
        if batch is None:
            continue
        to_batch.append(max(0.0, batch.end - sent) * 1000)
        if batch.frame is False:
            no_frame_needed += 1
            continue
        frame = next_frame(frames, batch.start)
        # Only a log without the `frame` field needs the cut-off.
        if frame is None or (batch.frame is None and frame.at - batch.end > NO_FRAME_S):
            no_frame += 1
        else:
            to_frame.append(max(0.0, frame.at - sent) * 1000)
    return {
        "injected": len(injected),
        "logged": logged,
        "unmapped": sum(1 for batch in mapped if batch is None),
        "to_batch_end_ms": summary(to_batch),
        "to_frame_ms": summary(to_frame),
        "no_frame": no_frame,
        "no_frame_needed": no_frame_needed,
    }
