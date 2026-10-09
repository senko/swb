"""Pixel comparison of two screenshots."""

import io
from pathlib import Path

import numpy as np
from PIL import Image

DEFAULT_THRESHOLD = 32
"""A pixel matches if no color channel differs by more than this (0 to 255)."""


def load_rgb(path: Path | io.BytesIO, dtype: type = np.int16) -> np.ndarray:
    """Loads an image as an array of shape (height, width, 3), dtype int16
    unless `dtype` says otherwise."""
    with Image.open(path) as image:
        return np.asarray(image.convert("RGB"), dtype=dtype)


def difference_mask(
    reference: np.ndarray, other: np.ndarray, threshold: int = DEFAULT_THRESHOLD
) -> np.ndarray:
    """Returns a boolean array with the shape of `reference`: True where the
    pixels differ. Pixels outside `other` differ."""
    height, width = reference.shape[:2]
    mask = np.ones((height, width), dtype=bool)
    h = min(height, other.shape[0])
    w = min(width, other.shape[1])
    channel_delta = np.abs(reference[:h, :w] - other[:h, :w]).max(axis=2)
    mask[:h, :w] = channel_delta > threshold
    return mask


BAND = 512
"""Rows per band in the full-page functions, which work band by band so
that a tall page needs no full-size temporary arrays."""


def load_rgb8(path: Path) -> np.ndarray:
    """Loads an image as an array of shape (height, width, 3), dtype uint8
    (half the memory of `load_rgb`; for full-page screenshots)."""
    return load_rgb(path, np.uint8)


def union_difference(
    reference: np.ndarray, other: np.ndarray, threshold: int = DEFAULT_THRESHOLD
) -> np.ndarray:
    """Compares two images of possibly different sizes over the union of
    their areas and returns the mask of differing pixels. Pixels outside the
    common area differ, so a taller or wider image lowers the score."""
    height = max(reference.shape[0], other.shape[0])
    width = max(reference.shape[1], other.shape[1])
    mask = np.ones((height, width), dtype=bool)
    h = min(reference.shape[0], other.shape[0])
    w = min(reference.shape[1], other.shape[1])
    for top in range(0, h, BAND):
        bottom = min(top + BAND, h)
        a = reference[top:bottom, :w].astype(np.int16)
        b = other[top:bottom, :w].astype(np.int16)
        mask[top:bottom, :w] = np.abs(a - b).max(axis=2) > threshold
    return mask


def union_diff_image(reference: np.ndarray, mask: np.ndarray) -> Image.Image:
    """`diff_image` for a mask that can be larger than `reference` (the
    union of two sizes): the area outside `reference` is white before the
    differing pixels are drawn in red."""
    height, width = mask.shape
    image = np.full((height, width, 3), 255, dtype=np.uint8)
    rows = min(height, reference.shape[0])
    cols = min(width, reference.shape[1])
    for top in range(0, rows, BAND):
        bottom = min(top + BAND, rows)
        gray = reference[top:bottom, :cols].mean(axis=2)
        pale = (255 - (255 - gray) * 0.3).astype(np.uint8)
        image[top:bottom, :cols] = pale[:, :, np.newaxis]
    image[mask] = (255, 0, 0)
    return Image.fromarray(image, "RGB")


def pixel_score(mask: np.ndarray) -> float:
    """The fraction of pixels that match."""
    return 1.0 - float(mask.mean()) if mask.size else 1.0


def diff_image(reference: np.ndarray, mask: np.ndarray) -> Image.Image:
    """A pale grayscale copy of the reference with differing pixels in red."""
    return union_diff_image(reference, mask)


def compare_screenshots(
    reference_path: Path, swb_path: Path, diff_path: Path, threshold: int = DEFAULT_THRESHOLD
) -> float:
    """Compares two PNG files, writes the diff image and returns the score."""
    reference = load_rgb(reference_path)
    mask = difference_mask(reference, load_rgb(swb_path), threshold)
    diff_image(reference, mask).save(diff_path)
    return pixel_score(mask)


NOISE = 2
"""Channel difference that `write_png_unless_close` treats as noise."""


def write_png_unless_close(path: Path, png: bytes, noise: int = NOISE) -> bool:
    """Writes `png` to `path` unless the existing file has the same size and
    no channel differs by more than `noise`. Chromium does not always render
    some images bit for bit the same; this keeps such noise out of git
    diffs. Returns True if the file was written."""
    if path.is_file():
        new = load_rgb(io.BytesIO(png))
        old = load_rgb(path)
        if old.shape == new.shape and int(np.abs(old - new).max(initial=0)) <= noise:
            return False
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(png)
    return True
