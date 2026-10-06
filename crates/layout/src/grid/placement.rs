//! Grid item placement (CSS Grid 2 §8,
//! <https://www.w3.org/TR/css-grid-2/#placement>): line resolution
//! (§8.3), placement conflict handling (§8.3.1) and the auto-placement
//! algorithm (§8.5).
//!
//! Lines are numbered from 0 at the start of the explicit grid (see
//! `template.rs`). Positions are clamped to the limited grid of
//! `-MAX_LINE..=MAX_LINE` (§5.4,
//! <https://www.w3.org/TR/css-grid-2/#overlarge-grids>): an area that
//! extends outside it is truncated, an area completely outside it moves
//! into the last track on that side.
//!
//! Occupied cells are stored per row (in the auto-flow direction) as
//! sorted, merged column intervals, so that placement does not depend on
//! the size of the grid. Each placement has a work budget
//! ([`PLACEMENT_WORK`]); when it runs out, the remaining items are placed
//! after all placed items (they never overlap, but are not packed).

use std::collections::HashMap;

use swb_style::{ComputedStyle, GridAutoFlow, GridLine};

use super::MAX_LINE;
use super::template::{LineNameIndex, clamp_line};

/// The most rows that one placement checks or marks. Enough for tens of
/// thousands of items with `dense` packing.
const PLACEMENT_WORK: usize = 4_000_000;

/// A grid item's area: the lines around it in each axis (`start <
/// end`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Area {
    pub(super) rows: (i32, i32),
    pub(super) columns: (i32, i32),
}

/// The result of placement.
#[derive(Debug)]
pub(super) struct Placement {
    /// The area of each item, in the order of the input.
    pub(super) areas: Vec<Area>,
    /// The first and the last line of the implicit grid in each axis.
    pub(super) rows: (i32, i32),
    pub(super) columns: (i32, i32),
}

/// One axis of the grid, for placement: its named lines and the number of
/// explicit tracks.
pub(super) struct AxisLines<'n, 's> {
    pub(super) names: &'n mut LineNameIndex<'s>,
    pub(super) explicit: i32,
}

/// The position of an item in one axis before auto-placement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Position {
    /// Two lines.
    Definite(i32, i32),
    /// An automatic position with a span.
    Auto(i32),
}

impl Position {
    fn span(self) -> i32 {
        match self {
            Position::Definite(s, e) => e - s,
            Position::Auto(span) => span,
        }
    }
}

/// The limits that a placement reached.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Warnings {
    /// Spans were truncated (the span budget ran out).
    pub(super) spans: bool,
    /// The work budget ran out.
    pub(super) work: bool,
}

/// Places the in-flow items with styles `items` (in order-modified
/// document order). `span_budget` is the number of extra tracks (beyond
/// one per axis) that items of this layout pass may still span; items
/// beyond it span one track.
pub(super) fn place(
    items: &[&ComputedStyle],
    flow: GridAutoFlow,
    rows: &mut AxisLines<'_, '_>,
    columns: &mut AxisLines<'_, '_>,
    span_budget: &mut usize,
) -> (Placement, Warnings) {
    let mut warnings = Warnings::default();
    let explicit_rows = rows.explicit;
    let explicit_columns = columns.explicit;
    let mut positions: Vec<(Position, Position)> = items
        .iter()
        .map(|s| {
            let r = resolve_axis(
                &s.grid_row_start,
                &s.grid_row_end,
                rows.names,
                explicit_rows,
            );
            let c = resolve_axis(
                &s.grid_column_start,
                &s.grid_column_end,
                columns.names,
                explicit_columns,
            );
            (r, c)
        })
        .collect();
    for (r, c) in &mut positions {
        *r = limit_span(*r, span_budget, &mut warnings.spans);
        *c = limit_span(*c, span_budget, &mut warnings.spans);
    }
    // Work in the auto-flow direction: major = rows for `row`.
    let (major_explicit, minor_explicit) = if flow.column {
        (explicit_columns, explicit_rows)
    } else {
        (explicit_rows, explicit_columns)
    };
    let oriented: Vec<(Position, Position)> = positions
        .iter()
        .map(|&(r, c)| if flow.column { (c, r) } else { (r, c) })
        .collect();
    let mut placer = AutoPlacer::new(&oriented, flow.dense, major_explicit, minor_explicit);
    let placed = placer.run(&oriented);
    warnings.work = placer.exhausted;
    let areas: Vec<Area> = placed
        .into_iter()
        .map(|(major, minor)| {
            let (rows, columns) = if flow.column {
                (minor, major)
            } else {
                (major, minor)
            };
            Area {
                rows: clamp_area(rows),
                columns: clamp_area(columns),
            }
        })
        .collect();
    let extent = |explicit: i32, f: &dyn Fn(&Area) -> (i32, i32)| {
        let start = areas.iter().map(|a| f(a).0).min().unwrap_or(0).min(0);
        let end = areas
            .iter()
            .map(|a| f(a).1)
            .max()
            .unwrap_or(0)
            .max(explicit);
        (start, end)
    };
    let placement = Placement {
        rows: extent(explicit_rows, &|a| a.rows),
        columns: extent(explicit_columns, &|a| a.columns),
        areas,
    };
    (placement, warnings)
}

/// Truncates a span to one track if the span budget does not allow it
/// (and sets `truncated`).
fn limit_span(p: Position, budget: &mut usize, truncated: &mut bool) -> Position {
    let extra = usize::try_from(p.span() - 1).unwrap_or(0);
    if extra <= *budget {
        *budget -= extra;
        return p;
    }
    *truncated = true;
    match p {
        Position::Definite(s, _) => Position::Definite(s, s + 1),
        Position::Auto(_) => Position::Auto(1),
    }
}

/// Clamps an area of one axis to the limited grid (§5.4 "clamp a grid
/// area").
fn clamp_area((start, end): (i32, i32)) -> (i32, i32) {
    let start = start.clamp(-MAX_LINE, MAX_LINE - 1);
    (start, end.clamp(start + 1, MAX_LINE))
}

/// Resolves the lines of one axis (§8.3 and §8.3.1).
fn resolve_axis(
    start: &GridLine,
    end: &GridLine,
    names: &mut LineNameIndex<'_>,
    explicit: i32,
) -> Position {
    let explicit = i64::from(explicit);
    let s = definite_line(start, names, explicit, true);
    let e = definite_line(end, names, explicit, false);
    let (s, e) = match (s, e) {
        (Some(s), Some(e)) => match s.cmp(&e) {
            std::cmp::Ordering::Less => (s, e),
            std::cmp::Ordering::Equal => (s, s + 1),
            std::cmp::Ordering::Greater => (e, s),
        },
        (Some(s), None) => {
            let e = match end {
                GridLine::Span(n, None) => s + i64::from(*n),
                GridLine::Span(n, Some(name)) => {
                    span_forward(names, name, s, i64::from(*n), explicit)
                }
                _ => s + 1,
            };
            (s, e)
        }
        (None, Some(e)) => {
            let s = match start {
                GridLine::Span(n, None) => e - i64::from(*n),
                GridLine::Span(n, Some(name)) => span_backward(names, name, e, i64::from(*n)),
                _ => e - 1,
            };
            (s, e)
        }
        (None, None) => {
            // Two spans: the end's is ignored; a span with a name alone is
            // a span of 1.
            let span = |l: &GridLine| match l {
                GridLine::Span(n, None) => Some(i64::from(*n)),
                GridLine::Span(_, Some(_)) => Some(1),
                _ => None,
            };
            let span = span(start).or_else(|| span(end)).unwrap_or(1);
            let limit = 2 * i64::from(MAX_LINE);
            return Position::Auto(i32::try_from(span.clamp(1, limit)).unwrap_or(1));
        }
    };
    let s = clamp_line(s);
    let e = clamp_line(e).max(s + 1);
    Position::Definite(s, e)
}

/// The line of a `<grid-line>` that names a line (not `auto` or a span).
fn definite_line(
    line: &GridLine,
    names: &mut LineNameIndex<'_>,
    explicit: i64,
    is_start: bool,
) -> Option<i64> {
    match line {
        GridLine::Auto | GridLine::Span(..) => None,
        GridLine::Line(n, None) => {
            let n = i64::from(*n);
            Some(if n > 0 { n - 1 } else { explicit + 1 + n })
        }
        GridLine::Line(n, Some(name)) => {
            let n = i64::from(*n);
            Some(if n > 0 {
                nth_from_start(names, name, n, explicit)
            } else {
                nth_from_end(names, name, -n)
            })
        }
        GridLine::Name(name) => {
            // A named area's edge (`name-start` or `name-end`) first, then
            // the first line called `name`.
            let suffixed = format!("{name}-{}", if is_start { "start" } else { "end" });
            if let Some(&first) = names.lines(&suffixed).first() {
                return Some(i64::from(first));
            }
            Some(nth_from_start(names, name, 1, explicit))
        }
    }
}

/// The `n`th line named `name` from the start. If there are fewer, the
/// implicit lines after the explicit grid count as named.
fn nth_from_start(names: &mut LineNameIndex<'_>, name: &str, n: i64, explicit: i64) -> i64 {
    let lines = names.lines(name);
    let len = lines.len() as i64;
    if n <= len {
        lines
            .get(usize::try_from(n - 1).unwrap_or(0))
            .map_or(explicit, |&l| i64::from(l))
    } else {
        explicit + (n - len)
    }
}

/// The `n`th line named `name` from the end. If there are fewer, the
/// implicit lines before the explicit grid count as named.
fn nth_from_end(names: &mut LineNameIndex<'_>, name: &str, n: i64) -> i64 {
    let lines = names.lines(name);
    let len = lines.len() as i64;
    if n <= len {
        lines
            .get(usize::try_from(len - n).unwrap_or(0))
            .map_or(0, |&l| i64::from(l))
    } else {
        -(n - len)
    }
}

/// The `n`th line named `name` after line `from`; implicit lines after the
/// explicit grid count as named.
fn span_forward(
    names: &mut LineNameIndex<'_>,
    name: &str,
    from: i64,
    n: i64,
    explicit: i64,
) -> i64 {
    let lines = names.lines(name);
    let after = lines.partition_point(|&l| i64::from(l) <= from);
    let k = (lines.len() - after) as i64;
    if n <= k {
        lines
            .get(after + usize::try_from(n - 1).unwrap_or(0))
            .map_or(from + 1, |&l| i64::from(l))
    } else {
        from.max(explicit) + (n - k)
    }
}

/// The `n`th line named `name` before line `from`; implicit lines before
/// the explicit grid count as named.
fn span_backward(names: &mut LineNameIndex<'_>, name: &str, from: i64, n: i64) -> i64 {
    let lines = names.lines(name);
    let before = lines.partition_point(|&l| i64::from(l) < from);
    let k = before as i64;
    if n <= k {
        lines
            .get(before - usize::try_from(n).unwrap_or(0))
            .map_or(from - 1, |&l| i64::from(l))
    } else {
        from.min(0) - (n - k)
    }
}

/// The occupied cells: for each row of the auto-flow direction, sorted,
/// disjoint column intervals.
#[derive(Default)]
struct Occupancy {
    rows: HashMap<i32, Vec<(i32, i32)>>,
}

impl Occupancy {
    /// Marks `minor` as occupied in the rows `major`.
    fn mark(&mut self, major: (i32, i32), minor: (i32, i32), work: &mut usize) {
        for m in major.0..major.1 {
            if *work == 0 {
                return;
            }
            *work -= 1;
            let row = self.rows.entry(m).or_default();
            let first = row.partition_point(|&(_, e)| e < minor.0);
            let mut last = first;
            let (mut start, mut end) = minor;
            while let Some(&(s, e)) = row.get(last).filter(|&&(s, _)| s <= minor.1) {
                start = start.min(s);
                end = end.max(e);
                last += 1;
            }
            row.splice(first..last, [(start, end)]);
        }
    }

    /// The first conflict of an area with the occupied cells: the row and
    /// the end of the occupied interval there. `None` if the area is free.
    fn conflict(
        &self,
        major: (i32, i32),
        minor: (i32, i32),
        work: &mut usize,
    ) -> Option<(i32, i32)> {
        for m in major.0..major.1 {
            *work = work.saturating_sub(1);
            let Some(row) = self.rows.get(&m) else {
                continue;
            };
            let i = row.partition_point(|&(_, e)| e <= minor.0);
            if let Some(&(s, e)) = row.get(i)
                && s < minor.1
            {
                return Some((m, e));
            }
        }
        None
    }

    /// True if row `m` is occupied from `minor.0` to `minor.1`.
    fn is_full(&self, m: i32, minor: (i32, i32)) -> bool {
        self.rows.get(&m).is_some_and(|row| {
            row.first()
                .is_some_and(|&(s, e)| s <= minor.0 && e >= minor.1)
        })
    }
}

/// The state of the auto-placement algorithm, in the auto-flow direction.
struct AutoPlacer {
    dense: bool,
    occupancy: Occupancy,
    /// True if an item needs auto-placement (otherwise nothing is marked).
    needs_occupancy: bool,
    work: usize,
    exhausted: bool,
    /// The extent of the implicit grid in the minor axis.
    minor: (i32, i32),
    /// The start-most major line.
    major_start: i32,
    /// The end of all placed areas in the major axis (rows from here on
    /// are free), for placement after the work budget ran out.
    major_end: i32,
    /// Rows before this one are full (for `dense` packing).
    first_open_row: i32,
}

impl AutoPlacer {
    fn new(
        items: &[(Position, Position)],
        dense: bool,
        major_explicit: i32,
        minor_explicit: i32,
    ) -> Self {
        let mut minor = (0, minor_explicit);
        let mut major_start = 0;
        let mut major_end = major_explicit;
        let mut needs_occupancy = false;
        let mut max_auto_minor_span = 0;
        for &(major, m) in items {
            match m {
                Position::Definite(s, e) => minor = (minor.0.min(s), minor.1.max(e)),
                Position::Auto(span) => {
                    needs_occupancy = true;
                    max_auto_minor_span = max_auto_minor_span.max(span);
                }
            }
            match major {
                Position::Definite(s, e) => {
                    major_start = major_start.min(s);
                    major_end = major_end.max(e);
                }
                Position::Auto(_) => needs_occupancy = true,
            }
        }
        // §8.5 step 3: the largest automatic minor span fits.
        minor.1 = minor.1.max(minor.0.saturating_add(max_auto_minor_span));
        AutoPlacer {
            dense,
            occupancy: Occupancy::default(),
            needs_occupancy,
            work: PLACEMENT_WORK,
            exhausted: false,
            minor,
            major_start,
            major_end,
            first_open_row: major_start,
        }
    }

    fn mark(&mut self, major: (i32, i32), minor: (i32, i32)) {
        self.major_end = self.major_end.max(major.1);
        if self.needs_occupancy {
            self.occupancy.mark(major, minor, &mut self.work);
        }
    }

    fn out_of_work(&mut self) -> bool {
        if self.work == 0 && !self.exhausted {
            self.exhausted = true;
        }
        self.work == 0
    }

    /// Runs §8.5 steps 1, 2 and 4 (step 3 is in [`AutoPlacer::new`]).
    /// Returns the (major, minor) areas.
    fn run(&mut self, items: &[(Position, Position)]) -> Vec<((i32, i32), (i32, i32))> {
        let mut placed = vec![((0, 1), (0, 1)); items.len()];
        let mut done = vec![false; items.len()];
        // Step 1: items with definite positions in both axes.
        for (i, &(major, minor)) in items.iter().enumerate() {
            if let (Position::Definite(a, b), Position::Definite(c, d)) = (major, minor) {
                placed[i] = ((a, b), (c, d));
                done[i] = true;
                self.mark((a, b), (c, d));
            }
        }
        // Step 2: items locked to a major track.
        let mut cursors: HashMap<i32, i32> = HashMap::new();
        for (i, &(major, minor)) in items.iter().enumerate() {
            let (Position::Definite(a, b), Position::Auto(span)) = (major, minor) else {
                continue;
            };
            let start = if self.dense {
                self.minor.0
            } else {
                cursors.get(&a).copied().unwrap_or(self.minor.0)
            };
            let column = self.find_minor((a, b), start, span);
            let area = ((a, b), (column, column + span));
            cursors.insert(a, column + span);
            self.minor.1 = self.minor.1.max(column + span);
            placed[i] = area;
            done[i] = true;
            self.mark(area.0, area.1);
        }
        // Step 4: the remaining items.
        let mut cursor = (self.major_start, self.minor.0);
        for (i, &(major, minor)) in items.iter().enumerate() {
            if done[i] {
                continue;
            }
            let span = major.span();
            let area = match minor {
                Position::Definite(c, d) => {
                    if self.dense {
                        cursor = (self.major_start, c);
                    } else {
                        if c < cursor.1 {
                            cursor.0 += 1;
                        }
                        cursor.1 = c;
                    }
                    let row = self.find_major(cursor.0, span, (c, d));
                    cursor.0 = row;
                    ((row, row + span), (c, d))
                }
                Position::Auto(minor_span) => {
                    if self.dense {
                        cursor = (self.major_start, self.minor.0);
                    }
                    cursor = self.find_both(cursor, span, minor_span);
                    (
                        (cursor.0, cursor.0 + span),
                        (cursor.1, cursor.1 + minor_span),
                    )
                }
            };
            placed[i] = area;
            self.mark(area.0, area.1);
        }
        placed
    }

    /// The first minor line from `start` where an item of `span` minor
    /// tracks in the major tracks `major` overlaps nothing (it may extend
    /// the grid).
    fn find_minor(&mut self, major: (i32, i32), start: i32, span: i32) -> i32 {
        let mut column = start;
        while column < MAX_LINE {
            if self.out_of_work() {
                // Past everything placed in the minor axis.
                return self.minor.1;
            }
            match self
                .occupancy
                .conflict(major, (column, column + span), &mut self.work)
            {
                Some((_, end)) => column = end,
                None => break,
            }
        }
        column
    }

    /// The first major line from `start` where an item of `span` major
    /// tracks in the minor tracks `minor` overlaps nothing.
    fn find_major(&mut self, start: i32, span: i32, minor: (i32, i32)) -> i32 {
        let mut row = self.skip_full_rows(start);
        while row < MAX_LINE {
            if self.out_of_work() {
                return self.major_end;
            }
            match self
                .occupancy
                .conflict((row, row + span), minor, &mut self.work)
            {
                Some((r, _)) => row = r + 1,
                None => break,
            }
        }
        row
    }

    /// The first position from `cursor` (in auto-flow order) where an item
    /// of `span` × `minor_span` tracks overlaps nothing (§8.5 step 4 for
    /// items without a definite position).
    fn find_both(&mut self, cursor: (i32, i32), span: i32, minor_span: i32) -> (i32, i32) {
        let (mut row, mut column) = cursor;
        if self.dense || row < self.first_open_row {
            let open = self.skip_full_rows(row);
            if open > row {
                row = open;
                column = self.minor.0;
            }
        }
        while row < MAX_LINE {
            if self.out_of_work() {
                return (self.major_end, self.minor.0);
            }
            if column + minor_span > self.minor.1 {
                row += 1;
                column = self.minor.0;
                continue;
            }
            match self.occupancy.conflict(
                (row, row + span),
                (column, column + minor_span),
                &mut self.work,
            ) {
                Some((_, end)) => column = end,
                None => return (row, column),
            }
        }
        (row, column)
    }

    /// The first row from `row` that is not full.
    fn skip_full_rows(&mut self, row: i32) -> i32 {
        while self.first_open_row < MAX_LINE
            && self.occupancy.is_full(self.first_open_row, self.minor)
        {
            self.first_open_row += 1;
        }
        if row >= self.major_start && row < self.first_open_row {
            self.first_open_row
        } else {
            row
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use swb_style::{TrackList, TrackSize};

    use super::super::template::Template;
    use super::*;

    /// Places items with the given `grid-row` and `grid-column` lines in a
    /// grid of `rows` × `columns` explicit tracks.
    fn place_lines(
        lines: &[[GridLine; 4]],
        flow: GridAutoFlow,
        rows: u32,
        columns: u32,
    ) -> Placement {
        let styles: Vec<ComputedStyle> = lines
            .iter()
            .map(|[rs, re, cs, ce]| {
                let mut s = (*ComputedStyle::initial()).clone();
                s.grid_row_start = rs.clone();
                s.grid_row_end = re.clone();
                s.grid_column_start = cs.clone();
                s.grid_column_end = ce.clone();
                s
            })
            .collect();
        let refs: Vec<&ComputedStyle> = styles.iter().collect();
        let empty = TrackList::default();
        let auto: [TrackSize; 0] = [];
        let template = Template::new(&empty, &auto, 0, 0);
        let mut row_names = LineNameIndex::new(&empty, &template, None);
        let mut column_names = LineNameIndex::new(&empty, &template, None);
        let mut budget = usize::MAX;
        place(
            &refs,
            flow,
            &mut AxisLines {
                names: &mut row_names,
                explicit: i32::try_from(rows).unwrap_or(0),
            },
            &mut AxisLines {
                names: &mut column_names,
                explicit: i32::try_from(columns).unwrap_or(0),
            },
            &mut budget,
        )
        .0
    }

    const A: GridLine = GridLine::Auto;

    fn line(n: i32) -> GridLine {
        GridLine::Line(n, None)
    }

    fn span(n: u32) -> GridLine {
        GridLine::Span(n, None)
    }

    #[test]
    fn auto_items_fill_rows() {
        let p = place_lines(
            &[[A, A, A, A], [A, A, span(2), A], [A, A, A, A], [A, A, A, A]],
            GridAutoFlow::default(),
            0,
            3,
        );
        let cells: Vec<_> = p.areas.iter().map(|a| (a.rows.0, a.columns.0)).collect();
        assert_eq!(cells, [(0, 0), (0, 1), (1, 0), (1, 1)]);
        assert_eq!((p.rows, p.columns), ((0, 2), (0, 3)));
    }

    #[test]
    fn sparse_and_dense_packing() {
        let items = [[A, A, span(2), A], [A, A, span(2), A], [A, A, A, A]];
        let sparse = place_lines(&items, GridAutoFlow::default(), 0, 3);
        assert_eq!(sparse.areas[2].rows.0, 1);
        assert_eq!(sparse.areas[2].columns.0, 2);
        let dense = place_lines(
            &items,
            GridAutoFlow {
                column: false,
                dense: true,
            },
            0,
            3,
        );
        assert_eq!((dense.areas[2].rows.0, dense.areas[2].columns.0), (0, 2));
    }

    #[test]
    fn definite_lines_and_negative_numbers() {
        let p = place_lines(
            &[[line(-1), A, line(2), line(-1)], [line(-5), A, A, A]],
            GridAutoFlow::default(),
            2,
            3,
        );
        assert_eq!(
            p.areas[0],
            Area {
                rows: (2, 3),
                columns: (1, 3)
            }
        );
        // Line -5 of 3 lines is two implicit lines before the explicit
        // grid; the item gets the first free column there.
        assert_eq!(p.areas[1].rows, (-2, -1));
        assert_eq!(p.rows, (-2, 3));
    }

    #[test]
    fn items_locked_to_a_row() {
        let p = place_lines(
            &[[line(1), A, A, A], [line(1), A, A, A], [A, A, A, A]],
            GridAutoFlow::default(),
            0,
            2,
        );
        assert_eq!(p.areas[0].columns, (0, 1));
        assert_eq!(p.areas[1].columns, (1, 2));
        assert_eq!((p.areas[2].rows.0, p.areas[2].columns.0), (1, 0));
    }

    #[test]
    fn column_flow() {
        let p = place_lines(
            &[[A, A, A, A], [A, A, A, A], [A, A, A, A]],
            GridAutoFlow {
                column: true,
                dense: false,
            },
            2,
            0,
        );
        let cells: Vec<_> = p.areas.iter().map(|a| (a.rows.0, a.columns.0)).collect();
        assert_eq!(cells, [(0, 0), (1, 0), (0, 1)]);
    }

    #[test]
    fn huge_lines_are_clamped() {
        let p = place_lines(
            &[
                [line(i32::MAX), A, line(i32::MIN), span(u32::MAX)],
                [span(u32::MAX), A, A, A],
            ],
            GridAutoFlow::default(),
            0,
            0,
        );
        assert_eq!(p.areas[0].rows, (MAX_LINE - 1, MAX_LINE));
        assert_eq!(p.areas[0].columns, (-MAX_LINE, MAX_LINE));
        assert!(p.areas[1].rows.1 <= MAX_LINE);
        assert!(p.rows.0 >= -MAX_LINE && p.rows.1 <= MAX_LINE);
    }

    #[test]
    fn named_lines() {
        let name = |n: &str| Some(Arc::from(n));
        // Lines named `a`: 0, 1, 2, 3 of 3 explicit tracks.
        let cases = [
            // The second `a`; the span ends at the next one.
            (
                GridLine::Line(2, name("a")),
                GridLine::Span(1, name("a")),
                (1, 2),
            ),
            // The last `a`.
            (GridLine::Line(-1, name("a")), A, (3, 4)),
            // Two `a` lines back from line 3.
            (
                GridLine::Span(2, name("a")),
                GridLine::Line(-1, name("a")),
                (1, 3),
            ),
            // Not enough lines: implicit lines count as named.
            (GridLine::Line(-5, name("a")), A, (-1, 0)),
            (GridLine::Line(2, name("b")), A, (5, 6)),
            (line(2), GridLine::Span(2, name("b")), (1, 5)),
            (GridLine::Span(1, name("b")), line(2), (-1, 1)),
        ];
        for (start, end, expected) in cases {
            assert_eq!(
                place_named(start.clone(), end.clone()),
                expected,
                "{start:?} {end:?}"
            );
        }
    }

    /// The columns of an item with column lines `start` and `end` in a
    /// grid with `grid-template-columns: [a] repeat(3, auto [a])`.
    fn place_named(start: GridLine, end: GridLine) -> (i32, i32) {
        let mut s = (*ComputedStyle::initial()).clone();
        s.grid_column_start = start;
        s.grid_column_end = end;
        let list = TrackList::new(
            Arc::from([swb_style::TrackListEntry {
                names: Arc::from([Arc::from("a")]),
                value: swb_style::TrackListValue::Repeat(swb_style::TrackRepeat {
                    count: swb_style::RepeatCount::Count(3),
                    tracks: Arc::from([TrackSize::Breadth(swb_style::TrackBreadth::Auto)]),
                    names: Arc::from([Arc::from([]), Arc::from([Arc::from("a")])]),
                }),
            }]),
            Arc::from([]),
        );
        let auto: [TrackSize; 0] = [];
        let t = Template::new(&list, &auto, 0, 0);
        let mut names = LineNameIndex::new(&list, &t, None);
        let empty = TrackList::default();
        let empty_template = Template::new(&empty, &auto, 0, 0);
        let mut row_names = LineNameIndex::new(&empty, &empty_template, None);
        let mut budget = usize::MAX;
        let (p, _) = place(
            &[&s],
            GridAutoFlow::default(),
            &mut AxisLines {
                names: &mut row_names,
                explicit: 0,
            },
            &mut AxisLines {
                names: &mut names,
                explicit: t.explicit_tracks,
            },
            &mut budget,
        );
        p.areas[0].columns
    }

    #[test]
    fn many_dense_items_stay_within_the_work_budget() {
        let mut items = Vec::new();
        for i in 0..3000 {
            let s = if i % 2 == 0 { span(2) } else { A };
            items.push([A, A, s, A]);
        }
        let p = place_lines(
            &items,
            GridAutoFlow {
                column: false,
                dense: true,
            },
            0,
            3,
        );
        // No two items overlap.
        let mut cells = std::collections::HashSet::new();
        for a in &p.areas {
            for r in a.rows.0..a.rows.1 {
                for c in a.columns.0..a.columns.1 {
                    assert!(cells.insert((r, c)), "overlap at {r} {c}");
                }
            }
        }
    }
}
