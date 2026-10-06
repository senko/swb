//! Parsers for the grid properties (CSS Grid Layout 2): track lists
//! (§7.2, <https://www.w3.org/TR/css-grid-2/#track-sizing>), named areas
//! (§7.3), grid lines (§8.3), `grid-auto-flow` (§7.7), and the parts of
//! the `grid-template`, `grid`, `grid-row`, `grid-column` and `grid-area`
//! shorthands.
//!
//! Not supported: `subgrid` and `masonry` (the declarations are invalid,
//! so `@supports` reports them as unsupported).

use std::collections::HashMap;
use std::sync::Arc;

use swb_css::{BlockKind, ParseError, Parser};

use super::length::{LengthOptions, parse_length_percentage};
use super::{ParseResult, parse_integer};
use crate::values::{
    GenericTrackBreadth, GenericTrackList, GenericTrackListEntry, GenericTrackListValue,
    GenericTrackRepeat, GenericTrackSize, GridAutoFlow, GridLine, GridTemplateAreas, LineName,
    LineNames, NamedArea, RepeatCount, SpecifiedLengthPercentage as Lp, SpecifiedTrackList,
    SpecifiedTrackSize,
};

type SpecifiedTrackBreadth = GenericTrackBreadth<Lp>;

/// True for identifiers that are not valid `<custom-ident>`s in grid
/// values: the CSS-wide keywords, `default`, and (for line names and grid
/// lines) `span` and `auto`.
/// <https://www.w3.org/TR/css-values-4/#custom-idents>
fn is_excluded_ident(ident: &str) -> bool {
    [
        "initial",
        "inherit",
        "unset",
        "revert",
        "revert-layer",
        "default",
        "span",
        "auto",
    ]
    .iter()
    .any(|k| ident.eq_ignore_ascii_case(k))
}

/// A `<custom-ident excluding=(span, auto)>`.
fn parse_line_ident(p: &mut Parser<'_>) -> ParseResult<LineName> {
    p.try_parse(|p| {
        let ident = p.expect_ident()?;
        if is_excluded_ident(ident) {
            return Err(ParseError::Unexpected);
        }
        Ok(Arc::from(ident))
    })
}

/// `<line-names>`: `[ <custom-ident>* ]`.
fn parse_line_names(p: &mut Parser<'_>) -> ParseResult<LineNames> {
    p.try_parse(|p| {
        let block = match p.next() {
            Some(v) => v
                .as_block(BlockKind::Square)
                .ok_or(ParseError::Unexpected)?,
            None => return Err(ParseError::EndOfInput),
        };
        let mut inner = Parser::new(block);
        let mut names = Vec::new();
        while !inner.is_exhausted() {
            names.push(parse_line_ident(&mut inner)?);
        }
        Ok(Arc::from(names))
    })
}

/// The names of both lists, for line names that are written next to
/// each other and name the same line.
fn merge_names(a: &LineNames, b: &LineNames) -> LineNames {
    if a.is_empty() {
        return Arc::clone(b);
    }
    if b.is_empty() {
        return Arc::clone(a);
    }
    a.iter().chain(b.iter()).cloned().collect()
}

fn no_names() -> LineNames {
    Arc::from([])
}

/// `<flex>`: a non-negative `fr` dimension.
fn parse_flex(p: &mut Parser<'_>) -> ParseResult<f32> {
    p.try_parse(|p| {
        let (value, unit) = p.expect_dimension()?;
        if !unit.eq_ignore_ascii_case("fr") || value < 0.0 || !value.is_finite() {
            return Err(ParseError::Unexpected);
        }
        Ok(value)
    })
}

/// `<track-breadth>` (with `allow_flex`) or `<inflexible-breadth>`.
fn parse_breadth(p: &mut Parser<'_>, allow_flex: bool) -> ParseResult<SpecifiedTrackBreadth> {
    if let Ok(k) = p.expect_one_of(&[("min-content", 0), ("max-content", 1), ("auto", 2)]) {
        return Ok(match k {
            0 => GenericTrackBreadth::MinContent,
            1 => GenericTrackBreadth::MaxContent,
            _ => GenericTrackBreadth::Auto,
        });
    }
    if allow_flex && let Ok(v) = parse_flex(p) {
        return Ok(GenericTrackBreadth::Flex(v));
    }
    parse_length_percentage(p, LengthOptions::NON_NEGATIVE).map(GenericTrackBreadth::Length)
}

/// `<track-size>`: `<track-breadth> | minmax(<inflexible-breadth>,
/// <track-breadth>) | fit-content(<length-percentage [0,∞]>)`.
pub(crate) fn parse_track_size(p: &mut Parser<'_>) -> ParseResult<SpecifiedTrackSize> {
    // On error the function token stays unconsumed, so that the caller
    // sees an invalid value, not the end of a list.
    p.try_parse(|p| {
        if let Ok(mut args) = p.expect_function_matching("minmax") {
            return args.parse_entirely(|a| {
                let min = parse_breadth(a, false)?;
                a.expect_comma()?;
                let max = parse_breadth(a, true)?;
                Ok(GenericTrackSize::MinMax(min, max))
            });
        }
        if let Ok(mut args) = p.expect_function_matching("fit-content") {
            return args
                .parse_entirely(|a| parse_length_percentage(a, LengthOptions::NON_NEGATIVE))
                .map(GenericTrackSize::FitContent);
        }
        parse_breadth(p, true).map(GenericTrackSize::Breadth)
    })
}

/// Which track lists a parser accepts.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TrackListKind {
    /// `<track-list> | <auto-track-list>` (`grid-template-rows` and
    /// `-columns`).
    Template,
    /// `<explicit-track-list>`: no `repeat()` (the columns of the
    /// `grid-template` shorthand with areas).
    Explicit,
}

/// `repeat()`: the count, then `[ <line-names>? <track-size> ]+
/// <line-names>?`.
fn parse_repeat(p: &mut Parser<'_>) -> ParseResult<GenericTrackRepeat<Lp>> {
    p.try_parse(|p| {
        p.expect_function_matching("repeat")?
            .parse_entirely(repeat_arguments)
    })
}

/// The arguments of `repeat()`.
fn repeat_arguments(a: &mut Parser<'_>) -> ParseResult<GenericTrackRepeat<Lp>> {
    let count = if let Ok(auto) = a.expect_one_of(&[("auto-fill", true), ("auto-fit", false)]) {
        if auto {
            RepeatCount::AutoFill
        } else {
            RepeatCount::AutoFit
        }
    } else {
        let n = parse_integer(a)?;
        RepeatCount::Count(
            u32::try_from(n)
                .ok()
                .filter(|&n| n >= 1)
                .ok_or(ParseError::Invalid)?,
        )
    };
    a.expect_comma()?;
    let mut tracks = Vec::new();
    let mut names = vec![parse_line_names(a).unwrap_or_else(|_| no_names())];
    while !a.is_exhausted() {
        tracks.push(parse_track_size(a)?);
        names.push(parse_line_names(a).unwrap_or_else(|_| no_names()));
    }
    if tracks.is_empty() {
        return Err(ParseError::Invalid);
    }
    if count.is_auto() && !tracks.iter().all(GenericTrackSize::is_fixed_size) {
        return Err(ParseError::Invalid);
    }
    Ok(GenericTrackRepeat {
        count,
        tracks: Arc::from(tracks),
        names: Arc::from(names),
    })
}

/// A track list: `[ <line-names>? [ <track-size> | <track-repeat> ] ]+
/// <line-names>?`. Stops before anything that is not part of the list
/// (a `/` of a shorthand). An `<auto-track-list>` has exactly one
/// `repeat(auto-fill | auto-fit, ...)`, and all its tracks are
/// `<fixed-size>`s.
fn parse_track_list_items(
    p: &mut Parser<'_>,
    kind: TrackListKind,
) -> ParseResult<SpecifiedTrackList> {
    p.try_parse(|p| {
        let mut entries = Vec::new();
        let mut trailing_names = no_names();
        loop {
            let names = parse_line_names(p).ok();
            let value = if kind == TrackListKind::Template
                && let Ok(r) = parse_repeat(p)
            {
                Some(GenericTrackListValue::Repeat(r))
            } else {
                parse_track_size(p).ok().map(GenericTrackListValue::Track)
            };
            let Some(value) = value else {
                if let Some(names) = names {
                    trailing_names = names;
                }
                break;
            };
            entries.push(GenericTrackListEntry {
                names: names.unwrap_or_else(no_names),
                value,
            });
        }
        if entries.is_empty() {
            return Err(ParseError::Unexpected);
        }
        let list = GenericTrackList::new(Arc::from(entries), trailing_names);
        validate_auto_repeat(&list)?;
        Ok(list)
    })
}

/// Checks the rules of `<auto-track-list>` (§7.2.3.1): at most one
/// automatic repetition, and with one, only `<fixed-size>` tracks.
fn validate_auto_repeat(list: &SpecifiedTrackList) -> ParseResult<()> {
    let autos = list
        .entries()
        .iter()
        .filter(|e| matches!(&e.value, GenericTrackListValue::Repeat(r) if r.count.is_auto()))
        .count();
    if autos == 0 {
        return Ok(());
    }
    let all_fixed = list.entries().iter().all(|e| match &e.value {
        GenericTrackListValue::Track(t) => t.is_fixed_size(),
        GenericTrackListValue::Repeat(r) => r.tracks.iter().all(GenericTrackSize::is_fixed_size),
    });
    if autos == 1 && all_fixed {
        Ok(())
    } else {
        Err(ParseError::Invalid)
    }
}

/// `grid-template-rows` / `grid-template-columns`: `none | <track-list> |
/// <auto-track-list>`.
pub(crate) fn parse_track_list(p: &mut Parser<'_>) -> ParseResult<SpecifiedTrackList> {
    if p.expect_ident_matching("none").is_ok() {
        return Ok(GenericTrackList::default());
    }
    parse_track_list_items(p, TrackListKind::Template)
}

/// `grid-auto-rows` / `grid-auto-columns`: `<track-size>+`.
pub(crate) fn parse_track_sizes(p: &mut Parser<'_>) -> ParseResult<Arc<[SpecifiedTrackSize]>> {
    let mut sizes = vec![parse_track_size(p)?];
    while let Ok(size) = parse_track_size(p) {
        sizes.push(size);
    }
    Ok(Arc::from(sizes))
}

/// The track list of `grid-auto-rows` and `grid-auto-columns` initial
/// value: `auto`.
pub(crate) fn auto_track_sizes() -> Arc<[SpecifiedTrackSize]> {
    Arc::from([GenericTrackSize::Breadth(GenericTrackBreadth::Auto)])
}

/// `<grid-line>`: `auto | <custom-ident> | [ <integer [-∞,-1]> |
/// <integer [1,∞]> ] && <custom-ident>? | span && [ <integer [1,∞]> ||
/// <custom-ident> ]` (§8.3). The parts are accepted in the order that
/// Chromium accepts (`span` first or last).
pub(crate) fn parse_grid_line(p: &mut Parser<'_>) -> ParseResult<GridLine> {
    if p.expect_ident_matching("auto").is_ok() {
        return Ok(GridLine::Auto);
    }
    p.try_parse(|p| {
        let mut span = p.expect_ident_matching("span").is_ok();
        let mut number = None;
        let mut name = None;
        loop {
            if number.is_none()
                && let Ok(n) = parse_integer(p)
            {
                number = Some(n);
                continue;
            }
            if name.is_none()
                && let Ok(ident) = parse_line_ident(p)
            {
                name = Some(ident);
                continue;
            }
            break;
        }
        if !span {
            span = p.expect_ident_matching("span").is_ok();
        }
        match (span, number, name) {
            (_, None, None) => Err(ParseError::Unexpected),
            (true, Some(n), name) => {
                let n = u32::try_from(n)
                    .ok()
                    .filter(|&n| n >= 1)
                    .ok_or(ParseError::Invalid)?;
                Ok(GridLine::Span(n, name))
            }
            (true, None, name) => Ok(GridLine::Span(1, name)),
            (false, Some(0), _) => Err(ParseError::Invalid),
            (false, Some(n), name) => Ok(GridLine::Line(n, name)),
            (false, None, Some(name)) => Ok(GridLine::Name(name)),
        }
    })
}

/// The end line of a `grid-row` or `grid-column` shorthand (and the
/// omitted lines of `grid-area`) when it is omitted: the start line if it
/// is a `<custom-ident>`, else `auto` (§8.4).
pub(crate) fn omitted_line(start: &GridLine) -> GridLine {
    if start.is_name() {
        start.clone()
    } else {
        GridLine::Auto
    }
}

/// `grid-row` / `grid-column`: `<grid-line> [ / <grid-line> ]?`.
pub(crate) fn parse_grid_line_pair(p: &mut Parser<'_>) -> ParseResult<(GridLine, GridLine)> {
    let start = parse_grid_line(p)?;
    let end = if p.expect_delim('/').is_ok() {
        parse_grid_line(p)?
    } else {
        omitted_line(&start)
    };
    Ok((start, end))
}

/// `grid-area`: `<grid-line> [ / <grid-line> ]{0,3}`. Returns row-start,
/// column-start, row-end, column-end.
pub(crate) fn parse_grid_area(p: &mut Parser<'_>) -> ParseResult<[GridLine; 4]> {
    let mut lines = vec![parse_grid_line(p)?];
    while lines.len() < 4 && p.expect_delim('/').is_ok() {
        lines.push(parse_grid_line(p)?);
    }
    let row_start = lines[0].clone();
    let column_start = lines
        .get(1)
        .cloned()
        .unwrap_or_else(|| omitted_line(&row_start));
    let row_end = lines
        .get(2)
        .cloned()
        .unwrap_or_else(|| omitted_line(&row_start));
    let column_end = lines
        .get(3)
        .cloned()
        .unwrap_or_else(|| omitted_line(&column_start));
    Ok([row_start, column_start, row_end, column_end])
}

/// `grid-auto-flow`: `[ row | column ] || dense`.
pub(crate) fn parse_auto_flow(p: &mut Parser<'_>) -> ParseResult<GridAutoFlow> {
    let mut direction = None;
    let mut dense = false;
    loop {
        if direction.is_none()
            && let Ok(column) = p.expect_one_of(&[("row", false), ("column", true)])
        {
            direction = Some(column);
            continue;
        }
        if !dense && p.expect_ident_matching("dense").is_ok() {
            dense = true;
            continue;
        }
        break;
    }
    if direction.is_none() && !dense {
        return Err(ParseError::Unexpected);
    }
    Ok(GridAutoFlow {
        column: direction.unwrap_or(false),
        dense,
    })
}

/// `grid-template-areas`: `none | <string>+`.
pub(crate) fn parse_template_areas(
    p: &mut Parser<'_>,
) -> ParseResult<Option<Arc<GridTemplateAreas>>> {
    if p.expect_ident_matching("none").is_ok() {
        return Ok(None);
    }
    let mut rows = Vec::new();
    while let Ok(s) = p.expect_string() {
        rows.push(s);
    }
    if rows.is_empty() {
        return Err(ParseError::Unexpected);
    }
    areas_from_rows(&rows).map(|a| Some(Arc::new(a)))
}

/// One cell token of a `grid-template-areas` string: a name, or `None` for
/// a null cell token (one or more `.`).
fn cell_tokens(row: &str) -> ParseResult<Vec<Option<&str>>> {
    // Name code points (CSS Syntax 3 §4.2).
    let is_name = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_' || !c.is_ascii();
    let is_space = |c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C');
    let mut tokens = Vec::new();
    let mut rest = row;
    while let Some(c) = rest.chars().next() {
        let run = |pred: &dyn Fn(char) -> bool| rest.find(|c: char| !pred(c)).unwrap_or(rest.len());
        if is_space(c) {
            rest = &rest[run(&is_space)..];
        } else if c == '.' {
            rest = &rest[run(&|c| c == '.')..];
            tokens.push(None);
        } else if is_name(c) {
            let end = run(&is_name);
            tokens.push(Some(&rest[..end]));
            rest = &rest[end..];
        } else {
            // A trash token makes the declaration invalid.
            return Err(ParseError::Invalid);
        }
    }
    Ok(tokens)
}

/// The named areas of the strings of `grid-template-areas` (§7.3): every
/// string has the same number of cells (at least one), and the cells of
/// each name form a filled rectangle.
pub(crate) fn areas_from_rows(rows: &[&str]) -> ParseResult<GridTemplateAreas> {
    struct Extent {
        rows: (u32, u32),
        columns: (u32, u32),
        cells: u64,
        order: usize,
    }
    let mut columns = None;
    let mut extents: HashMap<&str, Extent> = HashMap::new();
    for (r, row) in rows.iter().enumerate() {
        let tokens = cell_tokens(row)?;
        if tokens.is_empty() || columns.is_some_and(|c| c != tokens.len()) {
            return Err(ParseError::Invalid);
        }
        columns = Some(tokens.len());
        let r = u32::try_from(r).map_err(|_| ParseError::Invalid)?;
        for (c, token) in tokens.iter().enumerate() {
            let Some(name) = token else { continue };
            let c = u32::try_from(c).map_err(|_| ParseError::Invalid)?;
            let order = extents.len();
            let e = extents.entry(name).or_insert(Extent {
                rows: (r, r + 1),
                columns: (c, c + 1),
                cells: 0,
                order,
            });
            e.rows = (e.rows.0.min(r), e.rows.1.max(r + 1));
            e.columns = (e.columns.0.min(c), e.columns.1.max(c + 1));
            e.cells += 1;
        }
    }
    let columns = columns.ok_or(ParseError::Invalid)?;
    let mut areas: Vec<(usize, NamedArea)> = Vec::with_capacity(extents.len());
    for (name, e) in extents {
        let area = u64::from(e.rows.1 - e.rows.0) * u64::from(e.columns.1 - e.columns.0);
        if area != e.cells {
            return Err(ParseError::Invalid);
        }
        areas.push((
            e.order,
            NamedArea {
                name: Arc::from(name),
                rows: e.rows,
                columns: e.columns,
            },
        ));
    }
    areas.sort_by_key(|(order, _)| *order);
    Ok(GridTemplateAreas::new(
        u32::try_from(rows.len()).map_err(|_| ParseError::Invalid)?,
        u32::try_from(columns).map_err(|_| ParseError::Invalid)?,
        areas.into_iter().map(|(_, a)| a).collect(),
    ))
}

/// The longhands that the `grid-template` shorthand sets.
pub(crate) struct GridTemplate {
    pub(crate) rows: SpecifiedTrackList,
    pub(crate) columns: SpecifiedTrackList,
    pub(crate) areas: Option<Arc<GridTemplateAreas>>,
}

/// `grid-template`: `none | [ <'grid-template-rows'> /
/// <'grid-template-columns'> ] | [ <line-names>? <string> <track-size>?
/// <line-names>? ]+ [ / <explicit-track-list> ]?` (§7.4).
pub(crate) fn parse_grid_template(p: &mut Parser<'_>) -> ParseResult<GridTemplate> {
    if p.try_parse(|p| {
        p.expect_ident_matching("none")?;
        p.expect_exhausted()
    })
    .is_ok()
    {
        return Ok(GridTemplate {
            rows: GenericTrackList::default(),
            columns: GenericTrackList::default(),
            areas: None,
        });
    }
    if let Ok(t) = p.try_parse(|p| {
        let rows = parse_track_list(p)?;
        p.expect_delim('/')?;
        let columns = parse_track_list(p)?;
        p.expect_exhausted()?;
        Ok::<_, ParseError>(GridTemplate {
            rows,
            columns,
            areas: None,
        })
    }) {
        return Ok(t);
    }
    p.try_parse(parse_template_with_areas)
}

/// The third form of `grid-template`: rows with area strings, then
/// optionally `/` and the columns.
fn parse_template_with_areas(p: &mut Parser<'_>) -> ParseResult<GridTemplate> {
    let mut entries = Vec::new();
    let mut strings = Vec::new();
    let mut pending = no_names();
    loop {
        let before = parse_line_names(p).unwrap_or_else(|_| no_names());
        let Ok(s) = p.expect_string() else {
            if before.is_empty() && !strings.is_empty() {
                break;
            }
            return Err(ParseError::Unexpected);
        };
        strings.push(s);
        let size =
            parse_track_size(p).unwrap_or(GenericTrackSize::Breadth(GenericTrackBreadth::Auto));
        entries.push(GenericTrackListEntry {
            names: merge_names(&pending, &before),
            value: GenericTrackListValue::Track(size),
        });
        pending = parse_line_names(p).unwrap_or_else(|_| no_names());
        if p.is_exhausted() || p.peek().is_some_and(|v| v.is_delim('/')) {
            break;
        }
    }
    let columns = if p.expect_delim('/').is_ok() {
        parse_track_list_items(p, TrackListKind::Explicit)?
    } else {
        GenericTrackList::default()
    };
    p.expect_exhausted()?;
    let areas = areas_from_rows(&strings)?;
    Ok(GridTemplate {
        rows: GenericTrackList::new(Arc::from(entries), pending),
        columns,
        areas: Some(Arc::new(areas)),
    })
}

/// The longhands that the `grid` shorthand sets.
pub(crate) struct Grid {
    pub(crate) template: GridTemplate,
    pub(crate) auto_rows: Arc<[SpecifiedTrackSize]>,
    pub(crate) auto_columns: Arc<[SpecifiedTrackSize]>,
    pub(crate) auto_flow: GridAutoFlow,
}

/// `[ auto-flow && dense? ]`: returns true for `dense`.
fn parse_auto_flow_keyword(p: &mut Parser<'_>) -> ParseResult<bool> {
    p.try_parse(|p| {
        let dense = p.expect_ident_matching("dense").is_ok();
        p.expect_ident_matching("auto-flow")?;
        Ok(dense || p.expect_ident_matching("dense").is_ok())
    })
}

/// `grid`: `<'grid-template'> | <'grid-template-rows'> / [ auto-flow &&
/// dense? ] <'grid-auto-columns'>? | [ auto-flow && dense? ]
/// <'grid-auto-rows'>? / <'grid-template-columns'>` (§7.8). Omitted
/// longhands get their initial values.
pub(crate) fn parse_grid(p: &mut Parser<'_>) -> ParseResult<Grid> {
    if let Ok(template) = p.try_parse(|p| {
        let t = parse_grid_template(p)?;
        p.expect_exhausted()?;
        Ok::<_, ParseError>(t)
    }) {
        return Ok(Grid {
            template,
            auto_rows: auto_track_sizes(),
            auto_columns: auto_track_sizes(),
            auto_flow: GridAutoFlow::default(),
        });
    }
    if let Ok(g) = p.try_parse(|p| {
        let rows = parse_track_list(p)?;
        p.expect_delim('/')?;
        let dense = parse_auto_flow_keyword(p)?;
        let auto_columns = parse_track_sizes(p).unwrap_or_else(|_| auto_track_sizes());
        p.expect_exhausted()?;
        Ok::<_, ParseError>(Grid {
            template: GridTemplate {
                rows,
                columns: GenericTrackList::default(),
                areas: None,
            },
            auto_rows: auto_track_sizes(),
            auto_columns,
            auto_flow: GridAutoFlow {
                column: true,
                dense,
            },
        })
    }) {
        return Ok(g);
    }
    p.try_parse(|p| {
        let dense = parse_auto_flow_keyword(p)?;
        let auto_rows = parse_track_sizes(p).unwrap_or_else(|_| auto_track_sizes());
        p.expect_delim('/')?;
        let columns = parse_track_list(p)?;
        p.expect_exhausted()?;
        Ok(Grid {
            template: GridTemplate {
                rows: GenericTrackList::default(),
                columns,
                areas: None,
            },
            auto_rows,
            auto_columns: auto_track_sizes(),
            auto_flow: GridAutoFlow {
                column: false,
                dense,
            },
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::test_util::parse_all;
    use crate::values::{Length, LengthUnit};

    fn px(v: f32) -> SpecifiedTrackBreadth {
        GenericTrackBreadth::Length(Lp::Length(Length {
            value: v,
            unit: LengthUnit::Px,
        }))
    }

    fn names(list: &[&str]) -> LineNames {
        list.iter().map(|n| Arc::from(*n)).collect()
    }

    #[test]
    fn track_sizes() {
        assert_eq!(
            parse_all("minmax(0, 1fr)", parse_track_size),
            Ok(GenericTrackSize::MinMax(
                px(0.0),
                GenericTrackBreadth::Flex(1.0)
            ))
        );
        assert_eq!(
            parse_all("2.5FR", parse_track_size),
            Ok(GenericTrackSize::Breadth(GenericTrackBreadth::Flex(2.5)))
        );
        assert!(parse_all("minmax(1fr, 10px)", parse_track_size).is_err());
        assert!(parse_all("-1fr", parse_track_size).is_err());
        assert!(parse_all("-10px", parse_track_size).is_err());
        assert!(parse_all("fit-content(auto)", parse_track_size).is_err());
        assert!(parse_all("fit-content(20%)", parse_track_size).is_ok());
        assert_eq!(
            parse_all("min-content", parse_track_size),
            Ok(GenericTrackSize::Breadth(GenericTrackBreadth::MinContent))
        );
    }

    #[test]
    fn track_lists() {
        let list = parse_all(
            "[a] 10px [b c] repeat(2, [d] 1fr [e]) [f]",
            parse_track_list,
        )
        .expect("valid");
        assert_eq!(list.entries().len(), 2);
        assert_eq!(list.entries()[0].names, names(&["a"]));
        assert_eq!(list.entries()[1].names, names(&["b", "c"]));
        let GenericTrackListValue::Repeat(r) = &list.entries()[1].value else {
            panic!("a repeat");
        };
        assert_eq!(r.count, RepeatCount::Count(2));
        assert_eq!(&*r.names, &[names(&["d"]), names(&["e"])]);
        assert_eq!(list.trailing_names(), &names(&["f"]));
        assert!(parse_all("none", parse_track_list).is_ok_and(|l| l.is_none()));
        // Line names cannot be `span` or `auto`; two groups in a row are
        // invalid.
        assert!(parse_all("[span] 10px", parse_track_list).is_err());
        assert!(parse_all("[a] [b] 10px", parse_track_list).is_err());
        assert!(parse_all("repeat(0, 10px)", parse_track_list).is_err());
        assert!(parse_all("repeat(2, repeat(2, 10px))", parse_track_list).is_err());
        assert!(parse_all("subgrid", parse_track_list).is_err());
        assert!(parse_all("masonry", parse_track_list).is_err());
    }

    #[test]
    fn an_invalid_function_after_a_valid_prefix_is_invalid() {
        for css in [
            "10px minmax(1fr, 10px)",
            "10px repeat(auto-fill, 1fr)",
            "10px fit-content(auto)",
            "repeat(2, 10px) repeat(0, 10px)",
        ] {
            assert!(parse_all(css, parse_track_list).is_err(), "{css}");
        }
        assert!(parse_all("10px minmax(1fr, 10px)", parse_track_sizes).is_err());
        assert!(parse_all("auto / 10px minmax(1fr, 10px)", parse_grid_template).is_err());
        assert!(parse_all("'a' minmax(1fr, 10px)", parse_grid_template).is_err());
        assert!(parse_all("auto-flow 10px / 10px minmax(1fr, 1px)", parse_grid).is_err());
    }

    #[test]
    fn auto_repeat_rules() {
        assert!(parse_all("repeat(auto-fill, minmax(100px, 1fr))", parse_track_list).is_ok());
        assert!(parse_all("10px repeat(auto-fit, 100px) 20%", parse_track_list).is_ok());
        // Intrinsic or flexible sizes are not allowed with an automatic
        // repetition, and there can be only one.
        assert!(parse_all("repeat(auto-fill, 1fr)", parse_track_list).is_err());
        assert!(parse_all("auto repeat(auto-fill, 10px)", parse_track_list).is_err());
        assert!(
            parse_all(
                "repeat(auto-fill, 10px) repeat(auto-fit, 10px)",
                parse_track_list
            )
            .is_err()
        );
    }

    #[test]
    fn grid_lines() {
        let line = |css: &str| parse_all(css, parse_grid_line);
        assert_eq!(line("auto"), Ok(GridLine::Auto));
        assert_eq!(line("-2"), Ok(GridLine::Line(-2, None)));
        assert_eq!(line("foo 3"), Ok(GridLine::Line(3, Some(Arc::from("foo")))));
        assert_eq!(line("span 2"), Ok(GridLine::Span(2, None)));
        assert_eq!(
            line("span foo"),
            Ok(GridLine::Span(1, Some(Arc::from("foo"))))
        );
        assert_eq!(
            line("2 foo span"),
            Ok(GridLine::Span(2, Some(Arc::from("foo"))))
        );
        assert_eq!(line("Foo"), Ok(GridLine::Name(Arc::from("Foo"))));
        assert!(line("0").is_err());
        assert!(line("span 0").is_err());
        assert!(line("span -1").is_err());
        assert!(line("span").is_err());
        assert!(line("2 span foo").is_err());
        assert!(line("inherit 2").is_err());
        assert!(line("foo bar").is_err());
    }

    #[test]
    fn line_pairs_and_areas() {
        let (s, e) = parse_all("a", parse_grid_line_pair).expect("valid");
        assert_eq!((s.clone(), e), (GridLine::Name(Arc::from("a")), s));
        let (_, e) = parse_all("2", parse_grid_line_pair).expect("valid");
        assert_eq!(e, GridLine::Auto);
        let [rs, cs, re, ce] = parse_all("a / 2", parse_grid_area).expect("valid");
        assert_eq!(rs, GridLine::Name(Arc::from("a")));
        assert_eq!(cs, GridLine::Line(2, None));
        assert_eq!(re, rs);
        assert_eq!(ce, GridLine::Auto);
        assert!(parse_all("1 / 2 / 3 / 4 / 5", parse_grid_area).is_err());
    }

    #[test]
    fn template_areas() {
        let areas = parse_all("'a a .' 'b b c'", parse_template_areas)
            .expect("valid")
            .expect("not none");
        assert_eq!((areas.rows, areas.columns), (2, 3));
        assert_eq!(&*areas.areas()[0].name, "a");
        assert_eq!(
            (areas.areas()[0].rows, areas.areas()[0].columns),
            ((0, 1), (0, 2))
        );
        assert_eq!(
            (areas.areas()[2].rows, areas.areas()[2].columns),
            ((1, 2), (2, 3))
        );
        // Dots in a row are one null cell; rows must have equal lengths;
        // areas must be rectangles; other characters are invalid.
        assert!(
            parse_all("'a...b'", parse_template_areas)
                .is_ok_and(|a| { a.is_some_and(|a| a.columns == 3) })
        );
        assert!(parse_all("'a a' 'b'", parse_template_areas).is_err());
        assert!(parse_all("'a b' 'b a'", parse_template_areas).is_err());
        assert!(parse_all("'a a' 'a .'", parse_template_areas).is_err());
        assert!(parse_all("'a #'", parse_template_areas).is_err());
        assert!(parse_all("''", parse_template_areas).is_err());
        assert!(parse_all("none", parse_template_areas).is_ok_and(|a| a.is_none()));
    }

    #[test]
    fn grid_template_shorthand() {
        let t = parse_all(
            "min-content 1fr min-content / 12.25rem minmax(0,1fr)",
            parse_grid_template,
        )
        .expect("valid");
        assert_eq!((t.rows.entries().len(), t.columns.entries().len()), (3, 2));
        assert!(t.areas.is_none());
        let t = parse_all(
            "[top] 'a a' 10px [mid] [mid2] 'b c' / [l] 1fr 2fr",
            parse_grid_template,
        )
        .expect("valid");
        assert_eq!(t.rows.entries().len(), 2);
        assert_eq!(t.rows.entries()[1].names, names(&["mid", "mid2"]));
        assert_eq!(
            t.rows.entries()[1].value,
            GenericTrackListValue::Track(GenericTrackSize::Breadth(GenericTrackBreadth::Auto))
        );
        assert_eq!(t.columns.entries().len(), 2);
        assert!(t.areas.is_some_and(|a| a.columns == 2));
        // No repeat() in the columns of the areas form.
        assert!(parse_all("'a' / repeat(2, 1fr)", parse_grid_template).is_err());
        assert!(parse_all("none", parse_grid_template).is_ok());
        assert!(parse_all("none / 10px", parse_grid_template).is_ok());
    }

    #[test]
    fn grid_shorthand() {
        let g = parse_all("auto-flow dense 40px / 1fr 1fr", parse_grid).expect("valid");
        assert_eq!(
            g.auto_flow,
            GridAutoFlow {
                column: false,
                dense: true
            }
        );
        assert_eq!(g.auto_rows.len(), 1);
        assert_eq!(g.template.columns.entries().len(), 2);
        let g = parse_all("100px / auto-flow", parse_grid).expect("valid");
        assert!(g.auto_flow.column);
        assert_eq!(g.template.rows.entries().len(), 1);
        let g = parse_all("'a b' / 1fr 1fr", parse_grid).expect("valid");
        assert!(g.template.areas.is_some());
        assert!(parse_all("auto-flow / auto-flow", parse_grid).is_err());
    }

    #[test]
    fn auto_flow() {
        let flow = |css: &str| parse_all(css, parse_auto_flow);
        assert_eq!(
            flow("column"),
            Ok(GridAutoFlow {
                column: true,
                dense: false
            })
        );
        assert_eq!(
            flow("dense"),
            Ok(GridAutoFlow {
                column: false,
                dense: true
            })
        );
        assert_eq!(
            flow("dense column"),
            Ok(GridAutoFlow {
                column: true,
                dense: true
            })
        );
        assert!(flow("row column").is_err());
    }
}
