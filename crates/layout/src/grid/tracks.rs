//! The tracks of one grid axis, grouped as in Chromium's
//! `GridSizingTrackCollection`
//! (`third_party/blink/renderer/core/layout/grid/grid_track_collection.cc`):
//!
//! - A *range* is a run of tracks between two consecutive lines where an
//!   item starts or ends, or where the track list changes (the explicit
//!   grid's edges, the `repeat()` boundaries). No item starts or ends
//!   inside a range, so every item spans whole ranges.
//! - A *set* holds the tracks of a range that have the same track size.
//!   All tracks of a set are sized alike, so the sizing algorithm works on
//!   sets: its cost depends on the number of items and on the length of
//!   the track lists, not on the number of tracks.
//!
//! Sizes in a set are per track: a set of `count` tracks of `base` px
//! is `count * base` px wide.

use swb_style::{LengthPercentage, TrackBreadth, TrackSize};

use super::template::Template;

/// The minimum track sizing function (CSS Grid 2 §7.2.1, normalized as
/// in §12.2 "Track Sizing Terminology").
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum MinSizing {
    Fixed(f32),
    MinContent,
    MaxContent,
    Auto,
}

/// The maximum track sizing function.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum MaxSizing {
    Fixed(f32),
    MinContent,
    MaxContent,
    Auto,
    Flex(f32),
}

/// The normalized sizing functions of a track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Sizing {
    pub(super) min: MinSizing,
    pub(super) max: MaxSizing,
    /// The argument of `fit-content()` (its maximum is `max-content`).
    pub(super) fit_content: Option<f32>,
}

/// Resolves a breadth: lengths against `available`; a percentage of an
/// indefinite size is `auto` (§7.2.1).
fn breadth_length(lp: &LengthPercentage, available: Option<f32>) -> Option<f32> {
    lp.resolve_opt(available).map(|v| v.max(0.0))
}

impl Sizing {
    /// The sizing functions of a track size, with percentages of
    /// `available` (Chromium's `GridSet` normalization).
    fn new(size: &TrackSize, available: Option<f32>) -> Sizing {
        let min = |b: &TrackBreadth| match b {
            TrackBreadth::Length(lp) => {
                breadth_length(lp, available).map_or(MinSizing::Auto, MinSizing::Fixed)
            }
            TrackBreadth::MinContent => MinSizing::MinContent,
            TrackBreadth::MaxContent => MinSizing::MaxContent,
            TrackBreadth::Auto | TrackBreadth::Flex(_) => MinSizing::Auto,
        };
        let max = |b: &TrackBreadth| match b {
            TrackBreadth::Length(lp) => {
                breadth_length(lp, available).map_or(MaxSizing::Auto, MaxSizing::Fixed)
            }
            TrackBreadth::MinContent => MaxSizing::MinContent,
            TrackBreadth::MaxContent => MaxSizing::MaxContent,
            TrackBreadth::Auto => MaxSizing::Auto,
            TrackBreadth::Flex(f) => MaxSizing::Flex(*f),
        };
        match size {
            TrackSize::Breadth(b) => Sizing {
                min: min(b),
                max: max(b),
                fit_content: None,
            },
            TrackSize::MinMax(lo, hi) => Sizing {
                min: min(lo),
                max: max(hi),
                fit_content: None,
            },
            TrackSize::FitContent(lp) => Sizing {
                min: MinSizing::Auto,
                max: MaxSizing::MaxContent,
                // A percentage of an indefinite size: `minmax(auto,
                // max-content)`.
                fit_content: breadth_length(lp, available),
            },
        }
    }

    pub(super) fn has_intrinsic_min(&self) -> bool {
        !matches!(self.min, MinSizing::Fixed(_))
    }

    pub(super) fn has_intrinsic_max(&self) -> bool {
        matches!(
            self.max,
            MaxSizing::MinContent | MaxSizing::MaxContent | MaxSizing::Auto
        )
    }

    pub(super) fn flex(&self) -> Option<f32> {
        match self.max {
            MaxSizing::Flex(f) => Some(f),
            _ => None,
        }
    }

    /// True if the maximum is `max-content` or `auto` (`fit-content()`
    /// included).
    pub(super) fn has_max_content_or_auto_max(&self) -> bool {
        matches!(self.max, MaxSizing::MaxContent | MaxSizing::Auto)
    }
}

/// Tracks of a range with the same track size.
#[derive(Clone, Debug)]
pub(super) struct Set {
    /// The number of tracks.
    pub(super) count: u32,
    pub(super) sizing: Sizing,
    /// The base size of each track.
    pub(super) base: f32,
    /// The growth limit of each track; infinite if not known yet.
    pub(super) limit: f32,
    /// The planned increase of each track while space for items is
    /// distributed (§12.5.1).
    pub(super) planned: Option<f32>,
    /// The item-incurred increase of each track.
    pub(super) incurred: f32,
    pub(super) infinitely_growable: bool,
}

impl Set {
    fn new(count: u32, sizing: Sizing) -> Set {
        let mut set = Set {
            count,
            sizing,
            base: 0.0,
            limit: f32::INFINITY,
            planned: None,
            incurred: 0.0,
            infinitely_growable: false,
        };
        set.initialize();
        set
    }

    /// §12.4: the initial base size and growth limit.
    fn initialize(&mut self) {
        self.base = match self.sizing.min {
            MinSizing::Fixed(v) => v,
            _ => 0.0,
        };
        self.limit = match self.sizing.max {
            MaxSizing::Fixed(v) => v.max(self.base),
            _ => f32::INFINITY,
        };
    }

    /// The growth limit, or the base size if it is infinite.
    pub(super) fn definite_limit(&self) -> f32 {
        if self.limit.is_finite() {
            self.limit
        } else {
            self.base
        }
    }

    /// Sets the base size; the growth limit is never less.
    pub(super) fn set_base(&mut self, base: f32) {
        self.base = base;
        if self.limit < base {
            self.limit = base;
        }
    }

    pub(super) fn count_f32(&self) -> f32 {
        self.count as f32
    }
}

/// A run of tracks that no item starts or ends inside.
#[derive(Clone, Debug)]
pub(super) struct Range {
    /// The number of tracks.
    pub(super) count: u32,
    /// The sets of its tracks.
    pub(super) sets: std::ops::Range<usize>,
    /// True for empty tracks of `repeat(auto-fit, ...)`, which collapse
    /// (size 0, no gaps; §7.2.3.2).
    pub(super) collapsed: bool,
}

/// The tracks of one axis.
#[derive(Debug)]
pub(super) struct Tracks {
    pub(super) ranges: Vec<Range>,
    pub(super) sets: Vec<Set>,
    /// The lines between ranges: range `i` lies between `lines[i]` and
    /// `lines[i + 1]`.
    lines: Vec<i32>,
    /// The gutter size.
    pub(super) gap: f32,
    /// True if a track size depends on the available size (a flexible
    /// or percentage track size).
    pub(super) depends_on_available: bool,
}

impl Tracks {
    /// The tracks of the implicit grid `extent` of an axis with `template`,
    /// split at the lines of the item areas `areas` (start and end lines).
    /// `available` is the available grid space for percentages.
    pub(super) fn new(
        template: &Template<'_>,
        extent: (i32, i32),
        areas: &[(i32, i32)],
        available: Option<f32>,
        gap: f32,
    ) -> Tracks {
        let mut lines = Vec::with_capacity(2 * areas.len() + 4);
        lines.extend([extent.0, extent.1, 0, template.explicit_tracks]);
        for s in template.segments() {
            lines.extend([s.start, s.end]);
        }
        for &(start, end) in areas {
            lines.extend([start, end]);
        }
        lines.retain(|l| (extent.0..=extent.1).contains(l));
        lines.sort_unstable();
        lines.dedup();

        // Ranges covered by items do not collapse.
        let mut covered = vec![0_i32; lines.len()];
        for &(start, end) in areas {
            let a = lines.partition_point(|&l| l < start);
            let b = lines.partition_point(|&l| l < end);
            if let Some(c) = covered.get_mut(a) {
                *c += 1;
            }
            if let Some(c) = covered.get_mut(b) {
                *c -= 1;
            }
        }

        let mut ranges = Vec::with_capacity(lines.len());
        let mut sets = Vec::new();
        let mut depends_on_available = false;
        let mut open = 0;
        for (i, w) in lines.windows(2).enumerate() {
            open += covered.get(i).copied().unwrap_or(0);
            let (a, b) = (w[0], w[1]);
            let count = u32::try_from(b - a).unwrap_or(0);
            let (pattern, offset, auto_fit) = template.source(a);
            let start = sets.len();
            let collapsed = auto_fit && open == 0;
            if !collapsed {
                let len = pattern.len();
                let n = count as usize;
                for j in 0..len.min(n) {
                    let size = &pattern[(offset + j) % len];
                    depends_on_available |= depends_on_available_size(size);
                    let set_count = n / len + usize::from(j < n % len);
                    sets.push(Set::new(
                        u32::try_from(set_count).unwrap_or(u32::MAX),
                        Sizing::new(size, available),
                    ));
                }
            }
            ranges.push(Range {
                count,
                sets: start..sets.len(),
                collapsed,
            });
        }
        Tracks {
            ranges,
            sets,
            lines,
            gap,
            depends_on_available,
        }
    }

    /// The index of the range that starts at `line` (`ranges.len()` for
    /// the last line).
    pub(super) fn range_at(&self, line: i32) -> usize {
        self.lines.partition_point(|&l| l < line)
    }

    /// The sets of the ranges `ranges`.
    pub(super) fn sets_of(&self, ranges: (usize, usize)) -> std::ops::Range<usize> {
        let start = self.ranges.get(ranges.0).map_or(0, |r| r.sets.start);
        let end = ranges
            .1
            .checked_sub(1)
            .and_then(|i| self.ranges.get(i))
            .map_or(start, |r| r.sets.end);
        start..end.max(start)
    }

    /// Resets every set to its initial sizes (§12.4).
    pub(super) fn reset(&mut self) {
        for set in &mut self.sets {
            set.initialize();
        }
    }

    /// The number of tracks that do not collapse.
    pub(super) fn track_count(&self) -> u32 {
        self.sets.iter().map(|s| s.count).sum()
    }

    /// The sum of the base sizes of all tracks and the gaps between them.
    pub(super) fn total_size(&self) -> f32 {
        let count = self.track_count();
        if count == 0 {
            return 0.0;
        }
        let tracks: f32 = self.sets.iter().map(|s| s.count_f32() * s.base).sum();
        tracks + self.gap * (count - 1) as f32
    }

    /// The sum of the fixed maximum sizes of the sets `sets` and the gaps
    /// between their `span` tracks, if all of them have a fixed maximum
    /// (Chromium's `CalculateSetSpanSize` after initialization). Used to
    /// clamp automatic minimum sizes (§6.6).
    pub(super) fn fixed_max_span(&self, sets: std::ops::Range<usize>, span: u32) -> Option<f32> {
        let mut sum = self.gap * span.saturating_sub(1) as f32;
        for set in self.sets.get(sets)? {
            if !matches!(set.sizing.max, MaxSizing::Fixed(_)) {
                return None;
            }
            sum += set.count_f32() * set.limit;
        }
        Some(sum)
    }

    /// The start and end of each range, for tracks that start at `start`
    /// with gutters of `gap` (the gutter size plus distributed space).
    /// Collapsed ranges are empty, and the gutters around them collapse.
    pub(super) fn positions(&self, start: f32, gap: f32) -> Vec<(f32, f32)> {
        let mut x = start;
        self.ranges
            .iter()
            .map(|r| {
                if r.collapsed {
                    return (x, x);
                }
                let size: f32 = self.sets.get(r.sets.clone()).map_or(0.0, |sets| {
                    sets.iter().map(|s| s.count_f32() * s.base).sum()
                });
                let size = size + gap * r.count.saturating_sub(1) as f32;
                let begin = x;
                x += size + gap;
                (begin, begin + size)
            })
            .collect()
    }
}

/// True if a track size uses a flexible or percentage size (Chromium's
/// `kIsDependentOnAvailableSize`).
fn depends_on_available_size(size: &TrackSize) -> bool {
    let breadth = |b: &TrackBreadth| match b {
        TrackBreadth::Length(lp) => lp.has_percentage(),
        TrackBreadth::Flex(_) => true,
        _ => false,
    };
    match size {
        TrackSize::Breadth(b) => breadth(b),
        TrackSize::MinMax(lo, hi) => breadth(lo) || breadth(hi),
        TrackSize::FitContent(lp) => lp.has_percentage(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use swb_style::{RepeatCount, TrackList, TrackListEntry, TrackListValue, TrackRepeat};

    use super::*;

    fn px(v: f32) -> TrackSize {
        TrackSize::Breadth(TrackBreadth::Length(LengthPercentage::Px(v)))
    }

    fn repeat(count: RepeatCount, tracks: &[TrackSize]) -> TrackList {
        TrackList::new(
            Arc::from([TrackListEntry {
                names: Arc::from([]),
                value: TrackListValue::Repeat(TrackRepeat {
                    count,
                    tracks: Arc::from(tracks),
                    names: (0..=tracks.len()).map(|_| Arc::from([])).collect(),
                }),
            }]),
            Arc::from([]),
        )
    }

    #[test]
    fn repeated_tracks_form_few_sets() {
        let list = repeat(RepeatCount::Count(5000), &[px(1.0), px(2.0)]);
        let t = Template::new(&list, &[], 0, 0);
        let tracks = Tracks::new(&t, (0, 10_000), &[(10, 13)], None, 0.0);
        // Ranges: 0..10, 10..13, 13..10000.
        assert_eq!(tracks.ranges.len(), 3);
        assert_eq!(tracks.sets.len(), 6);
        assert_eq!(tracks.track_count(), 10_000);
        assert_eq!(tracks.total_size(), 15_000.0);
        // The range 10..13 has the sizes 1, 2, 1.
        let sets = &tracks.sets[tracks.ranges[1].sets.clone()];
        assert_eq!((sets[0].count, sets[0].base), (2, 1.0));
        assert_eq!((sets[1].count, sets[1].base), (1, 2.0));
        assert_eq!(tracks.range_at(13), 2);
        assert_eq!(tracks.sets_of((1, 2)), 2..4);
    }

    #[test]
    fn empty_auto_fit_tracks_collapse() {
        let list = repeat(RepeatCount::AutoFit, &[px(100.0)]);
        let t = Template::new(&list, &[], 4, 0);
        let tracks = Tracks::new(&t, (0, 4), &[(0, 1)], None, 10.0);
        assert!(!tracks.ranges[0].collapsed);
        assert!(tracks.ranges[1].collapsed);
        assert_eq!(tracks.total_size(), 100.0);
        let positions = tracks.positions(0.0, 10.0);
        assert_eq!(positions[0], (0.0, 100.0));
        assert_eq!(positions[1], (110.0, 110.0));
    }

    #[test]
    fn percentages_of_an_indefinite_size_are_auto() {
        let size = TrackSize::Breadth(TrackBreadth::Length(LengthPercentage::Percent(0.5)));
        assert_eq!(Sizing::new(&size, None).min, MinSizing::Auto);
        assert_eq!(Sizing::new(&size, Some(200.0)).max, MaxSizing::Fixed(100.0));
        let fit = TrackSize::FitContent(LengthPercentage::Percent(0.5));
        assert_eq!(Sizing::new(&fit, None).fit_content, None);
    }
}
