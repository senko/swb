//! The track sizing algorithm (CSS Grid 2 §12.3–§12.8,
//! <https://www.w3.org/TR/css-grid-2/#algo-track-sizing>), on the sets of
//! `tracks.rs`.
//!
//! Chromium (`grid_track_sizing_algorithm.cc`) is the reference where it
//! differs from the specification text:
//!
//! - Items that span one track are handled like spanning items (the
//!   "size tracks to fit non-spanning items" step is the first group of
//!   §12.5 step 3); the results are the same.
//! - Under a min- or max-content constraint, intrinsic minimums use the
//!   items' minimum contributions, not their limited min-content
//!   contributions, and `auto` minimums do not take max-content
//!   contributions.
//! - The flex fraction is not recomputed for the container's `min-width`
//!   or `max-width` (§12.7 "If using this flex fraction would cause...").
//! - Maximizing tracks does not redo the step for `max-width` (§12.6).

use super::tracks::{MaxSizing, MinSizing, Set, Tracks};

/// The constraint that the grid container is sized under (§12.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Constraint {
    /// Layout with the available grid space.
    Layout,
    /// A min-content constraint (the container's min-content size).
    MinContent,
    /// A max-content constraint (the container's max-content size).
    MaxContent,
}

/// An intrinsic size contribution of an item (§12.2, §6.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Contribution {
    /// The minimum contribution (with the automatic minimum size).
    Minimum,
    MinContent,
    MaxContent,
}

/// What a grid item spans in one axis, for track sizing.
#[derive(Clone, Debug)]
pub(super) struct SizingItem {
    /// The sets of the spanned tracks.
    pub(super) sets: std::ops::Range<usize>,
    /// The number of spanned tracks.
    pub(super) span: u32,
    /// True if a spanned track has an intrinsic sizing function.
    pub(super) spans_intrinsic: bool,
    /// True if a spanned track has a flexible maximum.
    pub(super) spans_flex: bool,
}

impl SizingItem {
    /// The sizing data of an item that spans `sets` (`span` tracks).
    pub(super) fn new(tracks: &Tracks, sets: std::ops::Range<usize>, span: u32) -> SizingItem {
        let spanned = tracks.sets.get(sets.clone()).unwrap_or(&[]);
        SizingItem {
            spans_intrinsic: spanned
                .iter()
                .any(|s| s.sizing.has_intrinsic_min() || s.sizing.has_intrinsic_max()),
            spans_flex: spanned.iter().any(|s| s.sizing.flex().is_some()),
            sets,
            span,
        }
    }
}

/// What a step of §12.5 increases: the steps "for intrinsic minimums" to
/// "for max-content maximums", and the free space of §12.6 and §12.8.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    IntrinsicMinimums,
    ContentBasedMinimums,
    MaxContentMinimums,
    IntrinsicMaximums,
    MaxContentMaximums,
    FreeSpace,
}

impl Kind {
    fn grows_limits(self) -> bool {
        matches!(self, Kind::IntrinsicMaximums | Kind::MaxContentMaximums)
    }

    fn contribution(self) -> Contribution {
        match self {
            Kind::IntrinsicMinimums => Contribution::Minimum,
            Kind::ContentBasedMinimums | Kind::IntrinsicMaximums => Contribution::MinContent,
            _ => Contribution::MaxContent,
        }
    }

    /// True if the step increases the size of `set`.
    fn applies(self, set: &Set) -> bool {
        let s = &set.sizing;
        match self {
            Kind::IntrinsicMinimums => s.has_intrinsic_min(),
            Kind::ContentBasedMinimums => {
                matches!(s.min, MinSizing::MinContent | MinSizing::MaxContent)
            }
            Kind::MaxContentMinimums => s.min == MinSizing::MaxContent,
            Kind::IntrinsicMaximums => s.has_intrinsic_max(),
            Kind::MaxContentMaximums => s.has_max_content_or_auto_max(),
            Kind::FreeSpace => true,
        }
    }

    /// True if the step may grow `set` beyond its limit when space
    /// remains (§12.5.1 "Distribute space beyond limits"). When no
    /// affected set qualifies, all affected sets do.
    fn grows_beyond_limit(self, set: &Set) -> bool {
        match self {
            Kind::IntrinsicMinimums | Kind::ContentBasedMinimums => set.sizing.has_intrinsic_max(),
            Kind::MaxContentMinimums => set.sizing.has_max_content_or_auto_max(),
            _ => false,
        }
    }

    /// The size of a track that the step increases.
    fn affected(self, set: &Set) -> f32 {
        if self.grows_limits() {
            set.definite_limit()
        } else {
            set.base
        }
    }
}

/// How far a track of `set` can grow in step `kind` (per track; infinite
/// if unlimited). `enforce` applies the "infinitely growable" rule to
/// growth limits.
fn potential(set: &Set, kind: Kind, enforce: bool) -> f32 {
    match kind {
        Kind::FreeSpace => set.limit - set.base,
        Kind::IntrinsicMaximums | Kind::MaxContentMaximums => {
            if enforce && set.limit.is_finite() && !set.infinitely_growable {
                return 0.0;
            }
            match set.sizing.fit_content {
                // A `fit-content()` maximum is `max-content` up to its
                // argument, then fixed.
                Some(fit) => (fit - set.definite_limit() - set.incurred).max(0.0),
                None => f32::INFINITY,
            }
        }
        _ => {
            if set.limit.is_infinite() {
                f32::INFINITY
            } else {
                (set.limit - set.base - set.incurred).max(0.0)
            }
        }
    }
}

/// Gives each set of `members` a share of `extra`, in proportion to
/// `ratio` (the track count or the flex factor of the set), up to
/// `limit` (the per-track potential). Sets with no potential are
/// skipped. Members must be sorted by potential. Returns the space left.
fn share_out(
    sets: &mut [Set],
    members: &[usize],
    mut extra: f32,
    ratio: impl Fn(&Set) -> f64,
    limit: impl Fn(&Set) -> f32,
) -> f32 {
    // Ratios are summed in `f64`, so that huge flex factors stay finite.
    let mut ratio_sum: f64 = members
        .iter()
        .filter_map(|&i| sets.get(i))
        .filter(|s| limit(s) > 0.0)
        .map(&ratio)
        .sum();
    for &i in members {
        let Some(set) = sets.get_mut(i) else { continue };
        if ratio_sum <= 0.0 || extra <= 0.0 {
            break;
        }
        let potential = limit(set);
        if potential <= 0.0 {
            continue;
        }
        let r = ratio(set);
        let mut share = if r >= ratio_sum {
            extra
        } else {
            (f64::from(extra) * r / ratio_sum) as f32
        };
        if potential.is_finite() {
            share = share.min(potential * set.count_f32());
        }
        set.incurred += share / set.count_f32();
        ratio_sum -= r;
        extra -= share;
    }
    extra.max(0.0)
}

/// The number of tracks of a set, as a share ratio.
fn track_count(set: &Set) -> f64 {
    f64::from(set.count)
}

/// The sets that grow beyond their limits when space remains after all
/// sets reached them.
#[derive(Clone, Copy)]
enum Beyond<'a> {
    /// None (the space stays undistributed).
    None,
    /// The sets that grow up to their limits.
    Same,
    /// These sets.
    Sets(&'a [usize]),
}

/// §12.5.1 "Distribute extra space": sets the item-incurred increase of
/// the sets `grow` (equally per track), then beyond limits. An infinite
/// `extra` (only for free space) fills every set to its limit.
fn distribute_equally(
    sets: &mut [Set],
    extra: f32,
    grow: &mut [usize],
    beyond: Beyond<'_>,
    kind: Kind,
) {
    for &i in grow.iter() {
        if let Some(s) = sets.get_mut(i) {
            s.incurred = 0.0;
        }
    }
    if extra.is_infinite() {
        for &i in grow.iter() {
            if let Some(s) = sets.get_mut(i) {
                s.incurred = potential(s, kind, true);
            }
        }
        return;
    }
    // Sorted by potential, the sets that reach their limit first come
    // first (Chromium sorts once, ignoring "infinitely growable").
    grow.sort_by(|&a, &b| {
        let pa = sets.get(a).map_or(0.0, |s| potential(s, kind, false));
        let pb = sets.get(b).map_or(0.0, |s| potential(s, kind, false));
        pa.total_cmp(&pb)
    });
    let left = share_out(sets, grow, extra, track_count, |s| potential(s, kind, true));
    let beyond = match beyond {
        Beyond::None => return,
        Beyond::Same => &*grow,
        Beyond::Sets(b) => b,
    };
    if left > 0.0 {
        // Beyond limits: base sizes grow without limit; growth limits up
        // to their `fit-content()` argument.
        share_out(sets, beyond, left, track_count, |s| {
            if kind.grows_limits() {
                potential(s, kind, false)
            } else {
                f32::INFINITY
            }
        });
    }
}

/// Like [`distribute_equally`], in proportion to the flex factors of the
/// sets (§12.5 step 4).
fn distribute_by_flex(sets: &mut [Set], extra: f32, grow: &[usize], kind: Kind) {
    for &i in grow {
        if let Some(s) = sets.get_mut(i) {
            s.incurred = 0.0;
        }
    }
    share_out(
        sets,
        grow,
        extra,
        |s| f64::from(s.sizing.flex().unwrap_or(0.0)) * track_count(s),
        |s| potential(s, kind, true),
    );
}

/// The free space (§12.2): `None` if it is indefinite or infinite.
fn free_space(tracks: &Tracks, constraint: Constraint, available: Option<f32>) -> Option<f32> {
    match constraint {
        Constraint::Layout => available.map(|a| (a - tracks.total_size()).max(0.0)),
        Constraint::MaxContent => None,
        Constraint::MinContent => Some(0.0),
    }
}

/// The parameters of a run of the track sizing algorithm.
pub(super) struct SizingInput {
    pub(super) constraint: Constraint,
    /// The available grid space (§12.2), if definite.
    pub(super) available: Option<f32>,
    /// The container's minimum content size in this axis, for stretching
    /// `auto` tracks when the available space is indefinite.
    pub(super) min_available: f32,
    /// True if `auto` tracks stretch (content distribution `normal` or
    /// `stretch`).
    pub(super) stretch: bool,
}

/// Runs the track sizing algorithm (§12.3) on `tracks`. `contribution`
/// gives the size contribution of an item (by index into `items`).
pub(super) fn size_tracks(
    tracks: &mut Tracks,
    items: &[SizingItem],
    input: &SizingInput,
    contribution: &mut dyn FnMut(usize, Contribution) -> f32,
) {
    if tracks
        .sets
        .iter()
        .any(|s| s.sizing.has_intrinsic_min() || s.sizing.has_intrinsic_max())
    {
        resolve_intrinsic_sizes(tracks, items, contribution);
    }
    // §12.5 step 5.
    for set in &mut tracks.sets {
        if set.limit.is_infinite() {
            set.limit = set.base;
        }
    }
    maximize(tracks, input);
    if tracks.sets.iter().any(|s| s.sizing.flex().is_some()) {
        expand_flexible(tracks, items, input, contribution);
    }
    if input.stretch {
        stretch_auto(tracks, input);
    }
}

/// §12.5: resolves intrinsic track sizes. Items that span no flexible
/// track are processed in groups of equal span, smallest first; then
/// all items that span a flexible track together.
fn resolve_intrinsic_sizes(
    tracks: &mut Tracks,
    items: &[SizingItem],
    contribution: &mut dyn FnMut(usize, Contribution) -> f32,
) {
    let mut order: Vec<usize> = (0..items.len())
        .filter(|&i| items[i].spans_intrinsic)
        .collect();
    order.sort_by_key(|&i| {
        (
            items[i].spans_flex,
            if items[i].spans_flex {
                0
            } else {
                items[i].span
            },
        )
    });
    let flex_start = order.partition_point(|&i| !items[i].spans_flex);
    let (plain, flexible) = order.split_at(flex_start);
    for group in plain.chunk_by(|&a, &b| items[a].span == items[b].span) {
        for kind in [
            Kind::IntrinsicMinimums,
            Kind::ContentBasedMinimums,
            Kind::MaxContentMinimums,
            Kind::IntrinsicMaximums,
            Kind::MaxContentMaximums,
        ] {
            increase_sizes(tracks, items, group, kind, false, contribution);
        }
    }
    if !flexible.is_empty() {
        // A flexible maximum is not intrinsic: only the minimums.
        for kind in [
            Kind::IntrinsicMinimums,
            Kind::ContentBasedMinimums,
            Kind::MaxContentMinimums,
        ] {
            increase_sizes(tracks, items, flexible, kind, true, contribution);
        }
    }
}

/// One step of §12.5 step 3 (or step 4 with `flex_group`) for the items
/// `group`.
fn increase_sizes(
    tracks: &mut Tracks,
    items: &[SizingItem],
    group: &[usize],
    kind: Kind,
    flex_group: bool,
    contribution: &mut dyn FnMut(usize, Contribution) -> f32,
) {
    for set in &mut tracks.sets {
        set.planned = None;
    }
    let gap = tracks.gap;
    let mut grow = Vec::new();
    let mut beyond = Vec::new();
    for &index in group {
        let item = &items[index];
        grow.clear();
        beyond.clear();
        let mut flex_sum = 0.0;
        let mut spanned = gap * item.span.saturating_sub(1) as f32;
        for s in item.sets.clone() {
            let Some(set) = tracks.sets.get_mut(s) else {
                continue;
            };
            spanned += set.count_f32() * kind.affected(set);
            if flex_group && set.sizing.flex().is_none() {
                // Only flexible tracks grow; the others count as fixed.
                continue;
            }
            if kind.applies(set) {
                set.planned.get_or_insert(0.0);
                if flex_group {
                    flex_sum += set.sizing.flex().unwrap_or(0.0) * set.count_f32();
                }
                grow.push(s);
                if kind.grows_beyond_limit(set) {
                    beyond.push(s);
                }
            }
        }
        if grow.is_empty() {
            continue;
        }
        let extra = (contribution(index, kind.contribution()) - spanned).max(0.0);
        if extra <= 0.0 {
            continue;
        }
        if !flex_group || flex_sum <= 0.0 {
            let beyond = if beyond.is_empty() {
                Beyond::Same
            } else {
                Beyond::Sets(&beyond)
            };
            distribute_equally(&mut tracks.sets, extra, &mut grow, beyond, kind);
        } else {
            distribute_by_flex(&mut tracks.sets, extra, &grow, kind);
        }
        for &s in &grow {
            if let Some(set) = tracks.sets.get_mut(s) {
                let planned = set.planned.unwrap_or(0.0).max(set.incurred);
                set.planned = Some(planned);
            }
        }
    }
    for set in &mut tracks.sets {
        set.infinitely_growable = false;
        let Some(planned) = set.planned else { continue };
        match kind {
            Kind::IntrinsicMaximums => {
                // Tracks whose growth limit was infinite stay "infinitely
                // growable" for the next step.
                set.infinitely_growable = set.limit.is_infinite();
                set.limit = set.definite_limit() + planned;
            }
            Kind::MaxContentMaximums => set.limit = set.definite_limit() + planned,
            _ => set.set_base(set.base + planned),
        }
    }
}

/// §12.6: grows all tracks equally up to their growth limits.
fn maximize(tracks: &mut Tracks, input: &SizingInput) {
    let free = free_space(tracks, input.constraint, input.available);
    if free == Some(0.0) {
        return;
    }
    let mut all: Vec<usize> = (0..tracks.sets.len()).collect();
    distribute_equally(
        &mut tracks.sets,
        free.unwrap_or(f32::INFINITY),
        &mut all,
        Beyond::None,
        Kind::FreeSpace,
    );
    for set in &mut tracks.sets {
        set.set_base(set.base + set.incurred);
    }
}

/// §12.8: shares the free space among tracks with an `auto` maximum.
fn stretch_auto(tracks: &mut Tracks, input: &SizingInput) {
    let mut grow: Vec<usize> = (0..tracks.sets.len())
        .filter(|&i| {
            tracks.sets[i].sizing.max == MaxSizing::Auto
                && tracks.sets[i].sizing.fit_content.is_none()
        })
        .collect();
    if grow.is_empty() {
        return;
    }
    let free = free_space(tracks, input.constraint, input.available)
        .unwrap_or_else(|| input.min_available - tracks.total_size());
    if free <= 0.0 {
        return;
    }
    distribute_equally(
        &mut tracks.sets,
        free,
        &mut grow,
        Beyond::Same,
        Kind::FreeSpace,
    );
    for &i in &grow {
        if let Some(set) = tracks.sets.get_mut(i) {
            set.set_base(set.base + set.incurred);
        }
    }
}

/// §12.7.1 "Find the size of an fr" for the sets `sets` and the space
/// `space`.
fn find_fr_size(tracks: &Tracks, sets: std::ops::Range<usize>, space: f32) -> f32 {
    // Sums in `f64`, so that huge flex factors stay finite.
    let mut space = f64::from(space);
    let flex = |s: &Set| f64::from(s.sizing.flex().unwrap_or(0.0));
    let mut flex_sum = 0.0;
    let mut flexible: Vec<&Set> = Vec::new();
    let mut count = 0_u32;
    for set in tracks.sets.get(sets).unwrap_or(&[]) {
        if flex(set) > 0.0 {
            flex_sum += flex(set) * track_count(set);
            flexible.push(set);
        } else {
            space -= track_count(set) * f64::from(set.base);
        }
        count = count.saturating_add(set.count);
    }
    space -= f64::from(tracks.gap) * f64::from(count.saturating_sub(1));
    if space < 0.0 || flexible.is_empty() {
        return 0.0;
    }
    // Sets whose base size exceeds their share become inflexible, largest
    // base size per flex factor first.
    let ratio = |s: &Set| f64::from(s.base) / flex(s);
    flexible.sort_by(|a, b| ratio(b).total_cmp(&ratio(a)));
    let mut i = 0;
    while space > 0.0 && i < flexible.len() {
        flex_sum = f64::max(flex_sum, 1.0);
        let mut j = i;
        while j < flexible.len() && ratio(flexible[j]) > space / flex_sum {
            j += 1;
        }
        if i == j {
            return (space / flex_sum) as f32;
        }
        for s in &flexible[i..j] {
            flex_sum -= flex(s) * track_count(s);
            space -= f64::from(s.base) * track_count(s);
        }
        i = j;
    }
    0.0
}

/// §12.7: sizes flexible tracks with the largest `fr` that fits.
fn expand_flexible(
    tracks: &mut Tracks,
    items: &[SizingItem],
    input: &SizingInput,
    contribution: &mut dyn FnMut(usize, Contribution) -> f32,
) {
    let free = free_space(tracks, input.constraint, input.available);
    if free == Some(0.0) {
        return;
    }
    let fr = if let (Some(_), Some(available)) = (free, input.available) {
        find_fr_size(tracks, 0..tracks.sets.len(), available)
    } else {
        // Indefinite: the largest of each flexible track's base size per
        // flex factor (at least 1) and the fr size that each item across
        // flexible tracks needs for its max-content contribution.
        let mut fr: f32 = 0.0;
        for (i, item) in items.iter().enumerate() {
            if item.spans_flex {
                let size = contribution(i, Contribution::MaxContent);
                fr = fr.max(find_fr_size(tracks, item.sets.clone(), size));
            }
        }
        for set in &tracks.sets {
            if let Some(f) = set.sizing.flex() {
                fr = fr.max(set.base / f.max(1.0));
            }
        }
        fr
    };
    for set in &mut tracks.sets {
        if let Some(f) = set.sizing.flex() {
            let size = fr * f;
            if size.is_finite() && size >= set.base {
                set.set_base(size);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use swb_style::{
        LengthPercentage, TrackBreadth, TrackList, TrackListEntry, TrackListValue, TrackSize,
    };

    use super::super::template::Template;
    use super::*;

    fn list(sizes: &[TrackSize]) -> TrackList {
        TrackList::new(
            sizes
                .iter()
                .map(|s| TrackListEntry {
                    names: Arc::from([]),
                    value: TrackListValue::Track(s.clone()),
                })
                .collect(),
            Arc::from([]),
        )
    }

    fn px(v: f32) -> TrackBreadth {
        TrackBreadth::Length(LengthPercentage::Px(v))
    }

    /// Sizes the tracks `sizes` with items `(first track, span,
    /// min-content, max-content)` and returns the base sizes per track.
    fn sized(
        sizes: &[TrackSize],
        items: &[(i32, i32, f32, f32)],
        constraint: Constraint,
        available: Option<f32>,
        gap: f32,
    ) -> Vec<f32> {
        let l = list(sizes);
        let t = Template::new(&l, &[], 0, 0);
        let areas: Vec<(i32, i32)> = items.iter().map(|&(s, n, ..)| (s, s + n)).collect();
        let extent = (0, i32::try_from(sizes.len()).unwrap_or(0));
        let mut tracks = Tracks::new(&t, extent, &areas, available, gap);
        let sizing: Vec<SizingItem> = areas
            .iter()
            .map(|&(s, e)| {
                let sets = tracks.sets_of((tracks.range_at(s), tracks.range_at(e)));
                SizingItem::new(&tracks, sets, u32::try_from(e - s).unwrap_or(1))
            })
            .collect();
        let input = SizingInput {
            constraint,
            available,
            min_available: 0.0,
            stretch: true,
        };
        size_tracks(&mut tracks, &sizing, &input, &mut |i, c| match c {
            Contribution::MaxContent => items[i].3,
            _ => items[i].2,
        });
        tracks
            .sets
            .iter()
            .flat_map(|s| std::iter::repeat_n(s.base, s.count as usize))
            .collect()
    }

    const AUTO: TrackSize = TrackSize::Breadth(TrackBreadth::Auto);

    #[test]
    fn fixed_and_flexible_tracks() {
        let sizes = [
            TrackSize::Breadth(px(196.0)),
            TrackSize::MinMax(px(0.0), TrackBreadth::Flex(1.0)),
        ];
        let bases = sized(
            &sizes,
            &[(1, 1, 500.0, 900.0)],
            Constraint::Layout,
            Some(1000.0),
            24.0,
        );
        assert_eq!(bases, [196.0, 780.0]);
    }

    #[test]
    fn auto_tracks_take_their_content_and_stretch() {
        let bases = sized(
            &[AUTO, AUTO],
            &[(0, 1, 50.0, 100.0), (1, 1, 20.0, 40.0)],
            Constraint::Layout,
            Some(300.0),
            0.0,
        );
        // Max-content sizes 100 and 40, then the free 160 px is shared.
        assert_eq!(bases, [180.0, 120.0]);
        let bases = sized(
            &[AUTO, AUTO],
            &[(0, 1, 50.0, 100.0), (1, 1, 20.0, 40.0)],
            Constraint::MinContent,
            None,
            0.0,
        );
        assert_eq!(bases, [50.0, 20.0]);
    }

    #[test]
    fn spanning_items_grow_intrinsic_tracks() {
        let bases = sized(
            &[TrackSize::Breadth(px(10.0)), AUTO],
            &[(0, 2, 110.0, 110.0)],
            Constraint::MaxContent,
            None,
            0.0,
        );
        assert_eq!(bases, [10.0, 100.0]);
    }

    #[test]
    fn indefinite_flex_fraction() {
        // 1fr and 2fr with items of max-content 100 and 50: an fr is 100.
        let bases = sized(
            &[
                TrackSize::Breadth(TrackBreadth::Flex(1.0)),
                TrackSize::Breadth(TrackBreadth::Flex(2.0)),
            ],
            &[(0, 1, 10.0, 100.0), (1, 1, 10.0, 50.0)],
            Constraint::MaxContent,
            None,
            0.0,
        );
        assert_eq!(bases, [100.0, 200.0]);
    }

    #[test]
    fn tracks_whose_base_exceeds_their_share_become_inflexible() {
        // `1fr 1fr` in 100 px; the first track's content needs 80 px, so
        // the second gets the remaining 20 px (§12.7.1 step 4).
        let fr = TrackSize::Breadth(TrackBreadth::Flex(1.0));
        let bases = sized(
            &[fr.clone(), fr],
            &[(0, 1, 80.0, 80.0)],
            Constraint::Layout,
            Some(100.0),
            0.0,
        );
        assert_eq!(bases, [80.0, 20.0]);
    }

    #[test]
    fn fit_content_growth_limits_stop_at_their_argument() {
        let fit = |v: f32| TrackSize::FitContent(LengthPercentage::Px(v));
        let bases = sized(
            &[fit(50.0), fit(100.0)],
            &[(0, 2, 100.0, 300.0)],
            Constraint::MaxContent,
            None,
            0.0,
        );
        assert_eq!(bases, [50.0, 100.0]);
    }

    #[test]
    fn huge_flex_factors_stay_finite() {
        let fr = TrackSize::Breadth(TrackBreadth::Flex(3e38));
        let bases = sized(
            &[fr.clone(), fr],
            &[(0, 2, 100.0, 100.0)],
            Constraint::Layout,
            Some(200.0),
            0.0,
        );
        assert!(bases.iter().all(|b| b.is_finite()), "{bases:?}");
        assert_eq!(bases, [100.0, 100.0]);
    }

    #[test]
    fn fit_content_limits_growth() {
        let bases = sized(
            &[TrackSize::FitContent(LengthPercentage::Px(80.0)), AUTO],
            &[(0, 1, 30.0, 200.0)],
            Constraint::Layout,
            Some(1000.0),
            0.0,
        );
        // The fit-content track stops at 80 px; the auto track stretches.
        assert_eq!(bases[0], 80.0);
        assert_eq!(bases[1], 920.0);
    }
}
