"""Pixel comparison of two screenshots."""

import io
from pathlib import Path

import numpy as np
from PIL import Image

DEFAULT_THRESHOLD = 32
"""A pixel matches if no color channel differs by more than this (0 to 255)."""


def load_rgb(path: Path) -> np.ndarray:
    """Loads an image as an array of shape (height, width, 3), dtype int16."""
    with Image.open(path) as image:
        return np.asarray(image.convert("RGB"), dtype=np.int16)


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


def pixel_score(mask: np.ndarray) -> float:
    """The fraction of pixels that match."""
    return 1.0 - float(mask.mean()) if mask.size else 1.0


def diff_image(reference: np.ndarray, mask: np.ndarray) -> Image.Image:
    """A pale grayscale copy of the reference with differing pixels in red."""
    gray = reference.mean(axis=2)
    pale = (255 - (255 - gray) * 0.3).astype(np.uint8)
    image = np.stack([pale, pale, pale], axis=2)
    image[mask] = (255, 0, 0)
    return Image.fromarray(image, "RGB")


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
        with Image.open(io.BytesIO(png)) as image:
            new = np.asarray(image.convert("RGB"), dtype=np.int16)
        old = load_rgb(path)
        if old.shape == new.shape and int(np.abs(old - new).max(initial=0)) <= noise:
            return False
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(png)
    return True
