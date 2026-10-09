"""Differing regions of a pixel comparison.

`find_regions` groups the differing pixels of a mask into connected areas
and returns their bounding rectangles. Pixels that are close to each other
(within one block of `BLOCK` px) belong to one region, so that the pixels of
a line of text form one region and not hundreds.
"""

from collections import deque
from dataclasses import dataclass

import numpy as np

from swbtools.boxes import BoxDump, Rect
from swbtools.scoring import element_paths

BLOCK = 8
"""Edge of the blocks that are grouped, in px. Differing pixels in the same
or in adjacent blocks (also diagonally) belong to the same region."""


@dataclass(frozen=True)
class Region:
    """A connected area of differing pixels."""

    rect: tuple[int, int, int, int]
    """x, y, width, height of the tight bounding rectangle, in px."""
    pixels: int
    """Number of differing pixels in the region."""


def _block_extents(blocks: np.ndarray) -> tuple[np.ndarray, ...]:
    """For each block of shape (bh, B, bw, B): the first and last differing
    row and column inside it (meaningless where the block has no pixel)."""
    rows = blocks.any(axis=3)  # (bh, B, bw)
    cols = blocks.any(axis=1)  # (bh, bw, B)
    first_row = rows.argmax(axis=1)
    last_row = rows.shape[1] - 1 - rows[:, ::-1, :].argmax(axis=1)
    first_col = cols.argmax(axis=2)
    last_col = cols.shape[2] - 1 - cols[:, :, ::-1].argmax(axis=2)
    return first_row, last_row, first_col, last_col


def find_regions(mask: np.ndarray, block: int = BLOCK) -> list[Region]:
    """Returns the regions of a boolean mask, most differing pixels first
    (ties: top to bottom, left to right)."""
    height, width = mask.shape
    bh, bw = -(-height // block), -(-width // block)
    padded = np.zeros((bh * block, bw * block), dtype=bool)
    padded[:height, :width] = mask
    blocks = padded.reshape(bh, block, bw, block)
    counts = blocks.sum(axis=(1, 3))
    first_row, last_row, first_col, last_col = _block_extents(blocks)
    seen = ~(counts > 0)  # blocks without pixels count as seen
    regions: list[Region] = []
    for start_y, start_x in np.argwhere(counts > 0).tolist():
        if seen[start_y, start_x]:
            continue
        seen[start_y, start_x] = True
        queue = deque([(start_y, start_x)])
        left = top = 1 << 60
        right = bottom = -1
        pixels = 0
        while queue:
            by, bx = queue.popleft()
            pixels += int(counts[by, bx])
            top = min(top, by * block + int(first_row[by, bx]))
            bottom = max(bottom, by * block + int(last_row[by, bx]))
            left = min(left, bx * block + int(first_col[by, bx]))
            right = max(right, bx * block + int(last_col[by, bx]))
            for ny in range(max(by - 1, 0), min(by + 2, bh)):
                for nx in range(max(bx - 1, 0), min(bx + 2, bw)):
                    if not seen[ny, nx]:
                        seen[ny, nx] = True
                        queue.append((ny, nx))
        regions.append(Region((left, top, right - left + 1, bottom - top + 1), pixels))
    regions.sort(key=lambda r: (-r.pixels, r.rect[1], r.rect[0]))
    return regions


def _contains(outer: Rect, inner: tuple[int, int, int, int], slack: float = 1.0) -> bool:
    ox, oy, ow, oh = outer
    ix, iy, iw, ih = inner
    return (
        ox - slack <= ix
        and oy - slack <= iy
        and ox + ow + slack >= ix + iw
        and oy + oh + slack >= iy + ih
    )


def covering_boxes(regions: list[Region], dump: BoxDump) -> list[str]:
    """For each region, the path of the smallest element of `dump` whose box
    contains the region (within 1 px), or `""` if there is none. With the
    Chromium dump, it names the element that the differing pixels belong to
    or lie in."""
    paths = element_paths(dump)
    boxed = [
        (index, element.rect)
        for index, element in enumerate(dump.elements)
        if element.rect is not None and element.rect[2] > 0 and element.rect[3] > 0
    ]
    result: list[str] = []
    for region in regions:
        best: tuple[float, int] | None = None
        for index, rect in boxed:
            if _contains(rect, region.rect):
                key = (rect[2] * rect[3], -index)
                if best is None or key < best:
                    best = key
        result.append("" if best is None else paths[-best[1]])
    return result
