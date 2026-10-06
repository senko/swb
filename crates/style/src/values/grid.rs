//! Values of the grid properties (CSS Grid Layout 2,
//! <https://www.w3.org/TR/css-grid-2/>): track sizes and track lists,
//! grid lines, named areas and `grid-auto-flow`.
//!
//! Track sizes are generic over the length type `L`: the parser stores
//! specified lengths, the computed style stores [`LengthPercentage`].

use std::collections::HashMap;
use std::sync::Arc;

use super::{LengthPercentage, SpecifiedLengthPercentage};

/// A line name (a `<custom-ident>`, case-sensitive).
pub type LineName = Arc<str>;

/// The names of one grid line (`[a b]`).
pub type LineNames = Arc<[LineName]>;

/// A track breadth (§7.2.1, <https://www.w3.org/TR/css-grid-2/#track-sizing>).
#[derive(Clone, Debug, PartialEq)]
pub enum GenericTrackBreadth<L> {
    /// `<length-percentage [0,∞]>`.
    Length(L),
    /// `<flex>`: a number of `fr`.
    Flex(f32),
    /// `min-content`.
    MinContent,
    /// `max-content`.
    MaxContent,
    /// `auto`.
    Auto,
}

impl<L> GenericTrackBreadth<L> {
    /// The same breadth with lengths mapped by `f`.
    pub fn map<M>(&self, f: &impl Fn(&L) -> M) -> GenericTrackBreadth<M> {
        match self {
            Self::Length(l) => GenericTrackBreadth::Length(f(l)),
            Self::Flex(v) => GenericTrackBreadth::Flex(*v),
            Self::MinContent => GenericTrackBreadth::MinContent,
            Self::MaxContent => GenericTrackBreadth::MaxContent,
            Self::Auto => GenericTrackBreadth::Auto,
        }
    }

    /// True for a `<length-percentage>`.
    pub fn is_fixed(&self) -> bool {
        matches!(self, Self::Length(_))
    }
}

/// A track size: `<track-size>` (§7.2.1).
#[derive(Clone, Debug, PartialEq)]
pub enum GenericTrackSize<L> {
    /// A single breadth. A `<flex>` breadth alone means `minmax(auto,
    /// <flex>)`.
    Breadth(GenericTrackBreadth<L>),
    /// `minmax(min, max)`. The minimum is never a `<flex>`.
    MinMax(GenericTrackBreadth<L>, GenericTrackBreadth<L>),
    /// `fit-content(<length-percentage>)`.
    FitContent(L),
}

impl<L> GenericTrackSize<L> {
    /// The same size with lengths mapped by `f`.
    pub fn map<M>(&self, f: &impl Fn(&L) -> M) -> GenericTrackSize<M> {
        match self {
            Self::Breadth(b) => GenericTrackSize::Breadth(b.map(f)),
            Self::MinMax(min, max) => GenericTrackSize::MinMax(min.map(f), max.map(f)),
            Self::FitContent(l) => GenericTrackSize::FitContent(f(l)),
        }
    }

    /// True for a `<fixed-size>` (§7.2.3.2): at least one of the minimum
    /// and maximum is a `<length-percentage>` and the minimum is not a
    /// `<flex>`.
    pub fn is_fixed_size(&self) -> bool {
        match self {
            Self::Breadth(b) => b.is_fixed(),
            Self::MinMax(min, max) => min.is_fixed() || max.is_fixed(),
            Self::FitContent(_) => false,
        }
    }
}

/// How often a `repeat()` repeats its tracks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepeatCount {
    /// An integer (at least 1).
    Count(u32),
    /// `auto-fill`.
    AutoFill,
    /// `auto-fit`.
    AutoFit,
}

impl RepeatCount {
    /// True for `auto-fill` and `auto-fit`.
    pub fn is_auto(self) -> bool {
        !matches!(self, RepeatCount::Count(_))
    }
}

/// A `repeat()` in a track list.
#[derive(Clone, Debug, PartialEq)]
pub struct GenericTrackRepeat<L> {
    /// The number of repetitions.
    pub count: RepeatCount,
    /// The repeated tracks (at least one).
    pub tracks: Arc<[GenericTrackSize<L>]>,
    /// The line names inside the function: `names[i]` is the line before
    /// `tracks[i]`, the last entry the line after the last track. One more
    /// entry than `tracks`.
    pub names: Arc<[LineNames]>,
}

/// A track or a `repeat()` of a track list.
#[derive(Clone, Debug, PartialEq)]
pub enum GenericTrackListValue<L> {
    /// One track.
    Track(GenericTrackSize<L>),
    /// `repeat()`.
    Repeat(GenericTrackRepeat<L>),
}

/// One entry of a track list: the line names before it and the track or
/// `repeat()`.
#[derive(Clone, Debug, PartialEq)]
pub struct GenericTrackListEntry<L> {
    /// The line names written before the value.
    pub names: LineNames,
    /// The track or repetition.
    pub value: GenericTrackListValue<L>,
}

/// Where a line name occurs in a track list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamePosition {
    /// In the names before entry `i` (the first line of the entry).
    Before(u32),
    /// In `names[offset]` of the repetition in entry `i`.
    InRepeat(u32, u32),
    /// In the names after the last entry (the last line).
    Trailing,
}

/// The positions of each line name of a track list. It is built once per
/// declaration and shared by all computed values of it, so that layout
/// finds the lines of a name without walking all names of the list.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LineNameTable {
    by_name: HashMap<LineName, Vec<NamePosition>>,
}

impl LineNameTable {
    /// The positions where `name` occurs, in the order of the list.
    pub fn positions(&self, name: &str) -> &[NamePosition] {
        self.by_name.get(name).map_or(&[], Vec::as_slice)
    }
}

/// The value of `grid-template-columns` or `grid-template-rows`
/// (§7.2): `none` (no entries) or a track list. Line names that are
/// written next to each other (`[a] repeat(2, [b] 10px)`) name the same
/// line.
#[derive(Clone, Debug)]
pub struct GenericTrackList<L> {
    /// The tracks and repetitions with the line names before each.
    entries: Arc<[GenericTrackListEntry<L>]>,
    /// The line names after the last entry.
    trailing_names: LineNames,
    /// The positions of the line names (from `entries` and
    /// `trailing_names`).
    name_table: Arc<LineNameTable>,
    /// The index of the first automatic repetition in `entries`.
    auto_repeat_index: Option<usize>,
}

// The name table and the index of the automatic repetition are derived
// from the entries and trailing names, so they are not compared.
impl<L: PartialEq> PartialEq for GenericTrackList<L> {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries && self.trailing_names == other.trailing_names
    }
}

impl<L> Default for GenericTrackList<L> {
    fn default() -> Self {
        GenericTrackList::new(Arc::from([]), Arc::from([]))
    }
}

impl<L> GenericTrackList<L> {
    /// A track list with these entries and trailing names.
    pub fn new(entries: Arc<[GenericTrackListEntry<L>]>, trailing_names: LineNames) -> Self {
        let mut by_name: HashMap<LineName, Vec<NamePosition>> = HashMap::new();
        let mut add = |names: &LineNames, position: NamePosition| {
            for name in names.iter() {
                // A name written twice for the same line (`[a a]`) is one
                // position.
                let positions = by_name.entry(Arc::clone(name)).or_default();
                if positions.last() != Some(&position) {
                    positions.push(position);
                }
            }
        };
        for (i, entry) in entries.iter().enumerate() {
            let i = u32::try_from(i).unwrap_or(u32::MAX);
            add(&entry.names, NamePosition::Before(i));
            if let GenericTrackListValue::Repeat(r) = &entry.value {
                for (o, names) in r.names.iter().enumerate() {
                    add(
                        names,
                        NamePosition::InRepeat(i, u32::try_from(o).unwrap_or(u32::MAX)),
                    );
                }
            }
        }
        add(&trailing_names, NamePosition::Trailing);
        let auto_repeat_index = entries.iter().position(
            |e| matches!(&e.value, GenericTrackListValue::Repeat(r) if r.count.is_auto()),
        );
        GenericTrackList {
            entries,
            trailing_names,
            name_table: Arc::new(LineNameTable { by_name }),
            auto_repeat_index,
        }
    }

    /// The tracks and repetitions with the line names before each.
    pub fn entries(&self) -> &[GenericTrackListEntry<L>] {
        &self.entries
    }

    /// The line names after the last entry.
    pub fn trailing_names(&self) -> &LineNames {
        &self.trailing_names
    }

    /// The positions of the line names.
    pub fn name_table(&self) -> &LineNameTable {
        &self.name_table
    }

    /// The same list with lengths mapped by `f`.
    pub fn map<M>(&self, f: &impl Fn(&L) -> M) -> GenericTrackList<M> {
        let entries = self
            .entries
            .iter()
            .map(|e| GenericTrackListEntry {
                names: Arc::clone(&e.names),
                value: match &e.value {
                    GenericTrackListValue::Track(t) => GenericTrackListValue::Track(t.map(f)),
                    GenericTrackListValue::Repeat(r) => {
                        GenericTrackListValue::Repeat(GenericTrackRepeat {
                            count: r.count,
                            tracks: r.tracks.iter().map(|t| t.map(f)).collect(),
                            names: Arc::clone(&r.names),
                        })
                    }
                },
            })
            .collect();
        GenericTrackList {
            entries,
            trailing_names: Arc::clone(&self.trailing_names),
            name_table: Arc::clone(&self.name_table),
            auto_repeat_index: self.auto_repeat_index,
        }
    }

    /// True for `none`.
    pub fn is_none(&self) -> bool {
        self.entries.is_empty()
    }

    /// The `repeat(auto-fill | auto-fit, ...)` of the list, if any (a valid
    /// list has at most one; otherwise the first).
    pub fn auto_repeat(&self) -> Option<&GenericTrackRepeat<L>> {
        match &self.entries.get(self.auto_repeat_index?)?.value {
            GenericTrackListValue::Repeat(r) => Some(r),
            GenericTrackListValue::Track(_) => None,
        }
    }
}

/// A computed track breadth.
pub type TrackBreadth = GenericTrackBreadth<LengthPercentage>;
/// A computed track size.
pub type TrackSize = GenericTrackSize<LengthPercentage>;
/// A computed `repeat()`.
pub type TrackRepeat = GenericTrackRepeat<LengthPercentage>;
/// A computed track list entry value.
pub type TrackListValue = GenericTrackListValue<LengthPercentage>;
/// A computed track list entry.
pub type TrackListEntry = GenericTrackListEntry<LengthPercentage>;
/// A computed `grid-template-columns` or `grid-template-rows`.
pub type TrackList = GenericTrackList<LengthPercentage>;

/// A specified track size.
pub(crate) type SpecifiedTrackSize = GenericTrackSize<SpecifiedLengthPercentage>;
/// A specified track list.
pub(crate) type SpecifiedTrackList = GenericTrackList<SpecifiedLengthPercentage>;

/// The value of `grid-row-start`, `grid-row-end`, `grid-column-start` or
/// `grid-column-end` (§8.3, <https://www.w3.org/TR/css-grid-2/#line-placement>).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum GridLine {
    /// `auto`.
    #[default]
    Auto,
    /// A `<custom-ident>` alone: a named area's edge or a named line.
    Name(LineName),
    /// `<integer> <custom-ident>?`; the integer is not 0.
    Line(i32, Option<LineName>),
    /// `span <integer>? <custom-ident>?`; the integer is at least 1.
    Span(u32, Option<LineName>),
}

impl GridLine {
    /// True for a `<custom-ident>` alone (for the shorthands, which copy
    /// it to the omitted end lines).
    pub fn is_name(&self) -> bool {
        matches!(self, GridLine::Name(_))
    }
}

/// A named grid area of `grid-template-areas`: the lines around it,
/// counted from 0 (line 1 of the explicit grid is 0).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedArea {
    /// The area name.
    pub name: LineName,
    /// The first and the last row line.
    pub rows: (u32, u32),
    /// The first and the last column line.
    pub columns: (u32, u32),
}

/// The value of `grid-template-areas` other than `none` (§7.3).
#[derive(Clone, Debug, Eq)]
pub struct GridTemplateAreas {
    /// The number of rows (strings).
    pub rows: u32,
    /// The number of columns (cells per string).
    pub columns: u32,
    /// The named areas, in order of their first cell.
    areas: Arc<[NamedArea]>,
    /// The index of each area in `areas` by name.
    by_name: HashMap<LineName, usize>,
}

// The index is derived from the areas, so it is not compared.
impl PartialEq for GridTemplateAreas {
    fn eq(&self, other: &Self) -> bool {
        self.rows == other.rows && self.columns == other.columns && self.areas == other.areas
    }
}

impl GridTemplateAreas {
    /// The areas of a grid of `rows` × `columns` cells.
    pub fn new(rows: u32, columns: u32, areas: Arc<[NamedArea]>) -> Self {
        let by_name = areas
            .iter()
            .enumerate()
            .map(|(i, a)| (Arc::clone(&a.name), i))
            .collect();
        GridTemplateAreas {
            rows,
            columns,
            areas,
            by_name,
        }
    }

    /// The named areas, in order of their first cell.
    pub fn areas(&self) -> &[NamedArea] {
        &self.areas
    }

    /// The area called `name`.
    pub fn area(&self, name: &str) -> Option<&NamedArea> {
        self.by_name.get(name).and_then(|&i| self.areas.get(i))
    }
}

/// The value of `grid-auto-flow` (§8.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct GridAutoFlow {
    /// True for `column`, false for `row`.
    pub column: bool,
    /// True for `dense`.
    pub dense: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_sizes() {
        let px = |v: f32| TrackBreadth::Length(LengthPercentage::Px(v));
        assert!(TrackSize::Breadth(px(10.0)).is_fixed_size());
        assert!(!TrackSize::Breadth(TrackBreadth::Flex(1.0)).is_fixed_size());
        assert!(TrackSize::MinMax(px(10.0), TrackBreadth::Flex(1.0)).is_fixed_size());
        assert!(TrackSize::MinMax(TrackBreadth::Auto, px(10.0)).is_fixed_size());
        assert!(!TrackSize::MinMax(TrackBreadth::Auto, TrackBreadth::MaxContent).is_fixed_size());
        assert!(!TrackSize::FitContent(LengthPercentage::Px(10.0)).is_fixed_size());
    }

    #[test]
    fn line_name_positions() {
        let names = |list: &[&str]| -> LineNames { list.iter().map(|n| Arc::from(*n)).collect() };
        let track = TrackSize::Breadth(TrackBreadth::Auto);
        let list = TrackList::new(
            Arc::from([
                TrackListEntry {
                    names: names(&["a"]),
                    value: TrackListValue::Track(track.clone()),
                },
                TrackListEntry {
                    names: names(&["b"]),
                    value: TrackListValue::Repeat(TrackRepeat {
                        count: RepeatCount::Count(2),
                        tracks: Arc::from([track]),
                        names: Arc::from([names(&["a"]), names(&["c"])]),
                    }),
                },
            ]),
            names(&["a"]),
        );
        assert_eq!(
            list.name_table().positions("a"),
            &[
                NamePosition::Before(0),
                NamePosition::InRepeat(1, 0),
                NamePosition::Trailing
            ]
        );
        assert_eq!(
            list.name_table().positions("c"),
            &[NamePosition::InRepeat(1, 1)]
        );
        assert_eq!(list.name_table().positions("d"), &[] as &[NamePosition]);
        // Computed values share the table.
        let mapped = list.map(&Clone::clone);
        assert!(std::ptr::eq(mapped.name_table(), list.name_table()));
    }
}
