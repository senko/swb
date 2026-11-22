//! The region of a raster of parts (`rasterize_region`, ADR 0044 section
//! 18): the rectangles whose pixels matter. They do not overlap, so that a
//! drawing that is limited to each of them draws every pixel once.

use swb_layout::Rect;

use crate::filter::Margins;

/// The most rectangles of a region; more become their bounding box.
const MAX_PARTS: usize = 64;

/// Rectangles of whole device pixels of the rasterizer's target that do not
/// overlap, with their bounding box.
#[derive(Clone, Debug)]
pub(super) struct Region {
    parts: Vec<Rect>,
    bounds: Rect,
}

impl Region {
    /// The region of `parts` (whole device px): overlapping parts are
    /// merged into their bounding box. `None` without a part.
    pub(super) fn new(parts: Vec<Rect>) -> Option<Region> {
        let mut parts: Vec<Rect> = parts
            .into_iter()
            .filter(|r| r.width > 0.0 && r.height > 0.0)
            .collect();
        let bounds = parts.iter().copied().reduce(|a, b| a.union(&b))?;
        if parts.len() > MAX_PARTS {
            parts = vec![bounds];
        }
        // Merge overlapping parts until none overlap (few parts).
        let mut merged = true;
        while merged {
            merged = false;
            'pairs: for a in 0..parts.len() {
                for b in a + 1..parts.len() {
                    let (Some(&ra), Some(&rb)) = (parts.get(a), parts.get(b)) else {
                        continue;
                    };
                    if ra.intersection(&rb).is_some() {
                        parts.swap_remove(b);
                        if let Some(p) = parts.get_mut(a) {
                            *p = ra.union(&rb);
                        }
                        merged = true;
                        break 'pairs;
                    }
                }
            }
        }
        Some(Region { parts, bounds })
    }

    /// The bounding box of the parts.
    pub(super) fn bounds(&self) -> Rect {
        self.bounds
    }

    /// True if a part intersects `r`.
    pub(super) fn touches(&self, r: &Rect) -> bool {
        r.intersection(&self.bounds).is_some()
            && self.parts.iter().any(|p| p.intersection(r).is_some())
    }

    /// The parts of `area` inside the region.
    pub(super) fn pieces(&self, area: Rect) -> Vec<Rect> {
        self.parts
            .iter()
            .filter_map(|p| p.intersection(&area))
            .collect()
    }

    /// The region grown by `m` on each side, inside `within`; `None` if
    /// nothing is left.
    pub(super) fn grown(&self, m: Margins, within: Rect) -> Option<Region> {
        let parts = self
            .parts
            .iter()
            .filter_map(|p| super::filter_layer::grow(*p, m).intersection(&within))
            .collect();
        Region::new(parts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_parts_become_their_bounding_box() {
        let region = Region::new(vec![
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Rect::new(5.0, 5.0, 10.0, 10.0),
            Rect::new(100.0, 0.0, 5.0, 5.0),
            // Overlaps the merged box only.
            Rect::new(14.0, 14.0, 4.0, 4.0),
        ])
        .expect("parts");
        let mut parts = region.pieces(region.bounds());
        parts.sort_by(|a, b| a.x.total_cmp(&b.x));
        assert_eq!(
            parts,
            vec![
                Rect::new(0.0, 0.0, 18.0, 18.0),
                Rect::new(100.0, 0.0, 5.0, 5.0)
            ]
        );
        assert!(region.touches(&Rect::new(101.0, 1.0, 1.0, 1.0)));
        assert!(!region.touches(&Rect::new(50.0, 1.0, 10.0, 10.0)));
        assert!(Region::new(vec![Rect::new(0.0, 0.0, 0.0, 5.0)]).is_none());
    }

    #[test]
    fn grown_parts_stay_disjoint() {
        let region = Region::new(vec![
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Rect::new(20.0, 0.0, 10.0, 10.0),
        ])
        .expect("parts");
        let m = Margins {
            left: 6,
            top: 6,
            right: 6,
            bottom: 6,
        };
        let grown = region
            .grown(m, Rect::new(0.0, 0.0, 100.0, 100.0))
            .expect("grown");
        assert_eq!(
            grown.pieces(grown.bounds()),
            vec![Rect::new(0.0, 0.0, 36.0, 16.0)]
        );
    }
}
