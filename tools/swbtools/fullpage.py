"""Full-page pixel comparison: score and differing regions."""

from dataclasses import dataclass
from pathlib import Path

from swbtools.boxes import BoxDump
from swbtools.pixels import load_rgb8, pixel_score, union_diff_image, union_difference
from swbtools.regions import Region, covering_boxes, find_regions

COVERED_REGIONS = 50
"""Number of regions (the first ones) whose covering box is looked up; the
report lists these."""


@dataclass(frozen=True)
class FullPage:
    """The result of a full-page comparison."""

    score: float
    """The fraction of matching pixels over the union of both areas."""
    reference_size: tuple[int, int]
    """Width and height of the Chromium screenshot, in px."""
    swb_size: tuple[int, int]
    """Width and height of the swb screenshot, in px."""
    regions: list[Region]
    """Differing regions, most differing pixels first."""
    covers: list[str]
    """For each of the first `COVERED_REGIONS` regions, the path of the
    smallest Chromium box that contains it (empty if there is none)."""

    @property
    def sizes_differ(self) -> bool:
        """True if the screenshots have different sizes."""
        return self.reference_size != self.swb_size


def compare_full_page(
    reference_png: Path,
    swb_png: Path,
    diff_path: Path,
    threshold: int,
    reference_dump: BoxDump,
) -> FullPage:
    """Compares two full-page screenshots. If the sizes differ, the score
    covers the union of the areas and the part outside the smaller image
    counts as different. Writes the diff image (union size) and returns the
    score and regions."""
    reference = load_rgb8(reference_png)
    other = load_rgb8(swb_png)
    mask = union_difference(reference, other, threshold)
    union_diff_image(reference, mask).save(diff_path)
    regions = find_regions(mask)
    return FullPage(
        score=pixel_score(mask),
        reference_size=(reference.shape[1], reference.shape[0]),
        swb_size=(other.shape[1], other.shape[0]),
        regions=regions,
        covers=covering_boxes(regions[:COVERED_REGIONS], reference_dump),
    )
