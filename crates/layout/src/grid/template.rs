//! The explicit grid of one axis (CSS Grid 2 §7.1,
//! <https://www.w3.org/TR/css-grid-2/#explicit-grids>): the template's
//! tracks as runs of a repeated track list (segments), the sizes of
//! implicit tracks (§7.6), and the lookup of named lines (§8.3).
//!
//! Lines are numbered from 0: line 1 of the explicit grid is 0, and
//! implicit lines before the explicit grid are negative. Repetitions are
//! never expanded into one entry per track, so a `repeat(10000, ...)`
//! costs as much as one track. Named lines are expanded only for the
//! names that grid items use, within [`NAMED_LINE_BUDGET`]; the positions
//! of the names come from the track list's shared name table and the
//! named areas' index, so a grid does no work per name it does not use.

use std::collections::HashMap;

use swb_style::{
    GridTemplateAreas, NamePosition, RepeatCount, TrackBreadth, TrackList, TrackListValue,
    TrackSize,
};

use super::MAX_LINE;

/// The most named lines that one grid expands (all names together). Beyond
/// it, further lines of a name are not found (they count as missing).
const NAMED_LINE_BUDGET: usize = 100_000;

/// The track size of `grid-auto-rows: auto`, for an empty list.
static AUTO_TRACK: [TrackSize; 1] = [TrackSize::Breadth(TrackBreadth::Auto)];

/// A run of tracks with a repeated track list.
#[derive(Clone, Copy, Debug)]
pub(super) struct Segment<'a> {
    /// The first track.
    pub(super) start: i32,
    /// One past the last track.
    pub(super) end: i32,
    /// The repeated track sizes (at least one).
    pub(super) tracks: &'a [TrackSize],
    /// True for the tracks of `repeat(auto-fit, ...)`, which collapse when
    /// they are empty (§7.2.3.2).
    pub(super) auto_fit: bool,
}

/// The lines of one entry of a track list: it covers the tracks
/// `start..end` with `repetitions` repetitions of `len` tracks.
#[derive(Clone, Copy, Debug)]
struct EntryLines {
    start: i32,
    end: i32,
    len: i32,
    repetitions: u32,
}

/// The tracks of one axis that grid items can reach: the explicit grid
/// and the sizes of implicit tracks.
pub(super) struct Template<'a> {
    segments: Vec<Segment<'a>>,
    /// The lines of each entry of the track list that starts inside the
    /// limited grid.
    entries: Vec<EntryLines>,
    /// The number of tracks that the track list defines.
    list_tracks: i32,
    /// The number of explicit tracks: the track list's or the named
    /// areas', whichever is more; at most [`MAX_LINE`].
    pub(super) explicit_tracks: i32,
    auto_tracks: &'a [TrackSize],
}

/// The number of tracks of `repeat(auto-fill | auto-fit, ...)` that the
/// track list of `list` has per repetition, and the number of tracks
/// outside it, if it has one.
pub(super) fn auto_repeat_shape(list: &TrackList) -> Option<(usize, i64)> {
    let repeat = list.auto_repeat().filter(|r| !r.tracks.is_empty())?;
    // Every entry other than the automatic repetition has at least one
    // track, so the first `MAX_LINE + 1` entries reach the limit.
    let others =
        list.entries()
            .iter()
            .take(MAX_LINE as usize + 1)
            .map(|e| match &e.value {
                TrackListValue::Track(_) => 1,
                TrackListValue::Repeat(r) => match r.count {
                    RepeatCount::Count(n) => i64::from(n)
                        .saturating_mul(i64::try_from(r.tracks.len()).unwrap_or(i64::MAX)),
                    _ => 0,
                },
            })
            .fold(0_i64, i64::saturating_add);
    Some((repeat.tracks.len(), others))
}

impl<'a> Template<'a> {
    /// The template of an axis with track list `list`, implicit track
    /// sizes `auto_tracks`, `auto_repeat` repetitions of the automatic
    /// repetition (if any), and `area_tracks` tracks of named areas.
    pub(super) fn new(
        list: &'a TrackList,
        auto_tracks: &'a [TrackSize],
        auto_repeat: u32,
        area_tracks: u32,
    ) -> Self {
        let mut segments = Vec::new();
        let mut entries = Vec::with_capacity(list.entries().len().min(MAX_LINE as usize));
        let mut line: i32 = 0;
        for entry in list.entries() {
            if line >= MAX_LINE {
                break;
            }
            let (tracks, repetitions, auto_fit): (&'a [TrackSize], u32, bool) = match &entry.value {
                TrackListValue::Track(t) => (std::slice::from_ref(t), 1, false),
                TrackListValue::Repeat(r) => match r.count {
                    RepeatCount::Count(n) => (&r.tracks, n, false),
                    RepeatCount::AutoFill => (&r.tracks, auto_repeat, false),
                    RepeatCount::AutoFit => (&r.tracks, auto_repeat, true),
                },
            };
            let len = i32::try_from(tracks.len()).unwrap_or(i32::MAX);
            let total = i64::from(repetitions) * i64::from(len);
            let end = (i64::from(line) + total).min(i64::from(MAX_LINE));
            let end = i32::try_from(end).unwrap_or(MAX_LINE).max(line);
            // The parser never builds an empty repetition, but the type
            // allows it; it covers no tracks.
            entries.push(EntryLines {
                start: line,
                end,
                len: len.max(1),
                repetitions: if len > 0 { repetitions } else { 0 },
            });
            if end > line {
                segments.push(Segment {
                    start: line,
                    end,
                    tracks,
                    auto_fit,
                });
            }
            line = end;
        }
        let explicit_tracks = line.max(clamp_line(i64::from(area_tracks)));
        let auto_tracks = if auto_tracks.is_empty() {
            &AUTO_TRACK[..]
        } else {
            auto_tracks
        };
        Template {
            segments,
            entries,
            list_tracks: line,
            explicit_tracks,
            auto_tracks,
        }
    }

    /// The runs of repeated tracks of the track list.
    pub(super) fn segments(&self) -> &[Segment<'a>] {
        &self.segments
    }

    /// The track list that `track` takes its size from (a segment's or
    /// the implicit track sizes), the index of its size in that list, and
    /// true if it belongs to `repeat(auto-fit, ...)`.
    pub(super) fn source(&self, track: i32) -> (&'a [TrackSize], usize, bool) {
        if (0..self.list_tracks).contains(&track) {
            let i = self.segments.partition_point(|s| s.end <= track);
            if let Some(s) = self.segments.get(i) {
                let offset = usize::try_from(track - s.start).unwrap_or(0) % s.tracks.len();
                return (s.tracks, offset, s.auto_fit);
            }
        }
        // §7.6: the first implicit track after the explicit grid takes the
        // first size, the last one before it the last size. Tracks of
        // named areas beyond the track list are sized as implicit tracks
        // after it (as in Chromium).
        let n = self.auto_tracks.len() as i64;
        let offset = if track < 0 {
            i64::from(track).rem_euclid(n)
        } else {
            (i64::from(track) - i64::from(self.list_tracks)).rem_euclid(n)
        };
        (
            self.auto_tracks,
            usize::try_from(offset).unwrap_or(0),
            false,
        )
    }
}

/// Clamps a line number to the limited grid (`-MAX_LINE..=MAX_LINE`).
pub(super) fn clamp_line(line: i64) -> i32 {
    i32::try_from(line.clamp(-i64::from(MAX_LINE), i64::from(MAX_LINE))).unwrap_or(0)
}

/// The named lines of one axis: the names of the track list and the
/// implicit `-start` and `-end` lines of the named areas (§7.3.2).
pub(super) struct LineNameIndex<'s> {
    list: &'s TrackList,
    template: &'s Template<'s>,
    /// The named areas and true for the row axis.
    areas: Option<(&'s GridTemplateAreas, bool)>,
    /// The sorted lines of each name that was looked up.
    expanded: HashMap<Box<str>, Vec<i32>>,
    budget: usize,
    /// True once the budget ran out.
    pub(super) exhausted: bool,
}

impl<'s> LineNameIndex<'s> {
    /// The named lines of an axis with track list `list` (laid out as
    /// `template`) and named areas `areas` (with true for the row axis).
    pub(super) fn new(
        list: &'s TrackList,
        template: &'s Template<'s>,
        areas: Option<(&'s GridTemplateAreas, bool)>,
    ) -> Self {
        LineNameIndex {
            list,
            template,
            areas,
            expanded: HashMap::new(),
            budget: NAMED_LINE_BUDGET,
            exhausted: false,
        }
    }

    /// Adds `count` lines from `first`, `step` apart, within the budget.
    fn push_lines(&mut self, lines: &mut Vec<i32>, first: i32, step: i32, count: i32) {
        for k in 0..count {
            if self.budget == 0 {
                self.exhausted = true;
                return;
            }
            self.budget -= 1;
            lines.push(first + k * step);
        }
    }

    /// The lines named `name`, sorted.
    pub(super) fn lines(&mut self, name: &str) -> &[i32] {
        if !self.expanded.contains_key(name) {
            let mut lines = Vec::new();
            let positions = self.list.name_table().positions(name);
            // Positions come in the order of the list, the trailing names
            // last. The first one beyond the limited grid ends the walk,
            // so the cost does not depend on the length of the list.
            for &position in positions {
                match position {
                    NamePosition::Before(i) => {
                        let Some(e) = self.entry(i) else { break };
                        self.push_lines(&mut lines, e.start, 1, 1);
                    }
                    NamePosition::InRepeat(i, offset) => {
                        // `names[offset]` names the lines `start + k * len
                        // + offset` of each repetition `k`.
                        let Some(e) = self.entry(i) else { break };
                        let first = e.start.saturating_add(i32::try_from(offset).unwrap_or(0));
                        if first <= e.end {
                            let repetitions = i32::try_from(e.repetitions).unwrap_or(i32::MAX);
                            let count = ((e.end - first) / e.len + 1).min(repetitions);
                            self.push_lines(&mut lines, first, e.len, count);
                        } else if e.end >= MAX_LINE {
                            break;
                        }
                    }
                    NamePosition::Trailing => {}
                }
            }
            if positions.last() == Some(&NamePosition::Trailing) {
                let end = self.template.list_tracks;
                self.push_lines(&mut lines, end, 1, 1);
            }
            if let Some(line) = self.area_line(name) {
                lines.push(line);
            }
            lines.sort_unstable();
            lines.dedup();
            self.expanded.insert(Box::from(name), lines);
        }
        self.expanded.get(name).map_or(&[], Vec::as_slice)
    }

    fn entry(&self, i: u32) -> Option<EntryLines> {
        self.template.entries.get(usize::try_from(i).ok()?).copied()
    }

    /// The line of `name` if it is the `-start` or `-end` line of a named
    /// area.
    fn area_line(&self, name: &str) -> Option<i32> {
        let (areas, rows) = self.areas?;
        let (stem, start) = match name.strip_suffix("-start") {
            Some(stem) => (stem, true),
            None => (name.strip_suffix("-end")?, false),
        };
        let area = areas.area(stem)?;
        let (s, e) = if rows { area.rows } else { area.columns };
        Some(clamp_line(i64::from(if start { s } else { e })))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use swb_style::{LengthPercentage, LineNames, NamedArea, TrackListEntry, TrackRepeat};

    use super::*;

    fn px(v: f32) -> TrackSize {
        TrackSize::Breadth(TrackBreadth::Length(LengthPercentage::Px(v)))
    }

    fn names(list: &[&str]) -> LineNames {
        list.iter().map(|n| Arc::from(*n)).collect()
    }

    /// `[a] 10px repeat(count, [b] 20px 30px [c]) [d]`.
    fn list(count: RepeatCount) -> TrackList {
        TrackList::new(
            Arc::from([
                TrackListEntry {
                    names: names(&["a"]),
                    value: TrackListValue::Track(px(10.0)),
                },
                TrackListEntry {
                    names: names(&[]),
                    value: TrackListValue::Repeat(TrackRepeat {
                        count,
                        tracks: Arc::from([px(20.0), px(30.0)]),
                        names: Arc::from([names(&["b"]), names(&[]), names(&["c"])]),
                    }),
                },
            ]),
            names(&["d"]),
        )
    }

    #[test]
    fn repeats_are_runs() {
        let l = list(RepeatCount::Count(3));
        let t = Template::new(&l, &[], 0, 0);
        let mut n = LineNameIndex::new(&l, &t, None);
        assert_eq!(t.explicit_tracks, 7);
        assert_eq!(t.segments().len(), 2);
        assert_eq!(t.source(4).1, 1);
        assert_eq!(t.source(5).1, 0);
        assert_eq!(n.lines("a"), &[0]);
        assert_eq!(n.lines("b"), &[1, 3, 5]);
        assert_eq!(n.lines("c"), &[3, 5, 7]);
        assert_eq!(n.lines("d"), &[7]);
        assert_eq!(n.lines("e"), &[] as &[i32]);
    }

    #[test]
    fn implicit_track_sizes_follow_the_pattern() {
        let l = list(RepeatCount::Count(1));
        let auto = [px(1.0), px(2.0), px(3.0)];
        let t = Template::new(&l, &auto, 0, 0);
        // The first track after the explicit grid (3 tracks) takes the
        // first size; the last one before it the last size.
        assert_eq!(t.source(3).1, 0);
        assert_eq!(t.source(5).1, 2);
        assert_eq!(t.source(-1).1, 2);
        assert_eq!(t.source(-3).1, 0);
    }

    #[test]
    fn huge_repeats_are_cheap_and_limited() {
        let l = list(RepeatCount::Count(u32::MAX));
        let t = Template::new(&l, &[], 0, 0);
        let mut n = LineNameIndex::new(&l, &t, None);
        assert_eq!(t.explicit_tracks, MAX_LINE);
        assert_eq!(t.segments().len(), 2);
        assert_eq!(n.lines("b").len(), (MAX_LINE as usize) / 2);
        assert!(!n.exhausted);
    }

    #[test]
    fn named_areas_give_start_and_end_lines() {
        let l = list(RepeatCount::Count(1));
        let areas = GridTemplateAreas::new(
            2,
            3,
            Arc::from([NamedArea {
                name: Arc::from("x"),
                rows: (1, 2),
                columns: (0, 3),
            }]),
        );
        let t = Template::new(&l, &[], 0, areas.columns);
        let mut n = LineNameIndex::new(&l, &t, Some((&areas, false)));
        assert_eq!(n.lines("x-start"), &[0]);
        assert_eq!(n.lines("x-end"), &[3]);
        assert_eq!(n.lines("y-start"), &[] as &[i32]);
        assert_eq!(n.lines("a"), &[0]);
    }
}
