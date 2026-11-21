//! `text-wrap: balance` and `text-wrap: pretty` (CSS Text 4 §5.2,
//! <https://www.w3.org/TR/css-text-4/#text-wrap-style>): where the lines of
//! a paragraph break. The specification leaves the algorithm open. The
//! rules here are fitted to Chromium 148's line breaks (213 paragraph and
//! width pairs of `tools/probes/css-videojs-2a.json`, cases `wrap-*`, all
//! equal) and are not Chromium's algorithm:
//!
//! - `balance`: a paragraph of two to [`MAX_BALANCED_LINES`] lines (as
//!   greedy breaking gives them) keeps its number of lines, and the lines
//!   break where the sum of the squares of the line widths is smallest.
//!   A longer paragraph breaks as with `auto` (Chromium does the same).
//! - `pretty`: only if the last line of the greedy breaks is one word
//!   narrower than a third of the available width (an orphan), the last
//!   [`PRETTY_LINES`] lines break again: the last line must have two words
//!   or more, and the sum of the squares of the free space of the other
//!   lines is smallest.
//!
//! Both work on the unbreakable groups of the line builder and plan the
//! groups that start a line. The planned breaks apply where no float
//! narrows the lines. A paragraph of more than [`MAX_GROUPS`] groups, or a
//! line of more than [`MAX_LINE_GROUPS`], is not planned (the work is
//! bounded by their product).

use swb_style::TextWrapStyle;

use super::FIT_TOLERANCE;

/// The most lines that `balance` balances.
pub(super) const MAX_BALANCED_LINES: usize = 6;
/// The number of lines at the end of a paragraph that `pretty` breaks
/// again.
const PRETTY_LINES: usize = 4;
/// The most groups of a paragraph that are planned.
const MAX_GROUPS: usize = 4096;
/// The most groups on one line that are planned.
const MAX_LINE_GROUPS: usize = 256;
/// A last line narrower than this part of the width is an orphan.
const ORPHAN_FRACTION: f32 = 1.0 / 3.0;
/// The cost of a last line with one word, in `pretty`.
const ORPHAN_COST: f64 = 1.0e12;

/// What the planner needs of a group.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Item {
    /// The width of the group with its trailing space.
    pub(super) width: f32,
    /// The width of the trailing space, which a line end removes.
    pub(super) trailing: f32,
    /// True if a forced break (`<br>`, a preserved newline) follows.
    pub(super) forced: bool,
}

/// The groups that start a line (not the first one of the paragraph) for
/// `items`, a paragraph of the style `style` in lines of `width`, the first
/// with the text indent `indent`. The result is sorted; it is empty if the
/// greedy breaks stay.
pub(super) fn plan(items: &[Item], width: f32, indent: f32, style: TextWrapStyle) -> Vec<usize> {
    if !matches!(style, TextWrapStyle::Balance | TextWrapStyle::Pretty) {
        return Vec::new();
    }
    let mut starts = Vec::new();
    let mut from = 0;
    while from < items.len() {
        let end = items[from..]
            .iter()
            .position(|i| i.forced)
            .map_or(items.len(), |p| from + p + 1);
        let first_indent = if from == 0 { indent } else { 0.0 };
        plan_paragraph(
            &items[from..end],
            from,
            width,
            first_indent,
            style,
            &mut starts,
        );
        from = end;
    }
    starts
}

/// The greedy lines of `items` as the group index where each line starts.
fn greedy(items: &[Item], width: f32, indent: f32) -> Vec<usize> {
    let mut starts = vec![0];
    let mut used = indent;
    for (i, item) in items.iter().enumerate() {
        if i > *starts.last().unwrap_or(&0)
            && used + item.width - item.trailing > width + FIT_TOLERANCE
        {
            starts.push(i);
            used = 0.0;
        }
        used += item.width;
    }
    starts
}

fn plan_paragraph(
    items: &[Item],
    offset: usize,
    width: f32,
    indent: f32,
    style: TextWrapStyle,
    out: &mut Vec<usize>,
) {
    if items.len() < 2 || items.len() > MAX_GROUPS {
        return;
    }
    let base = greedy(items, width, indent);
    let lines = base.len();
    if lines < 2 {
        return;
    }
    let planned = match style {
        TextWrapStyle::Balance if lines <= MAX_BALANCED_LINES => {
            split(items, 0, lines, width, indent, Cost::Balance)
        }
        TextWrapStyle::Pretty => pretty(items, &base, width, indent),
        _ => None,
    };
    if let Some(starts) = planned {
        out.extend(starts.into_iter().filter(|&s| s > 0).map(|s| s + offset));
    }
}

/// The new breaks of `pretty`, if the last greedy line is an orphan.
fn pretty(items: &[Item], base: &[usize], width: f32, indent: f32) -> Option<Vec<usize>> {
    let last = *base.last()?;
    if items.len() - last != 1 {
        return None;
    }
    let only = items.get(last)?;
    if only.width - only.trailing >= width * ORPHAN_FRACTION {
        return None;
    }
    let first_line = base.len().saturating_sub(PRETTY_LINES);
    let from = base[first_line];
    let kept = &base[..first_line];
    let tail_indent = if from == 0 { indent } else { 0.0 };
    let tail = split(
        &items[from..],
        0,
        base.len() - first_line,
        width,
        tail_indent,
        Cost::Pretty,
    )?;
    let mut starts: Vec<usize> = kept.to_vec();
    starts.extend(tail.into_iter().map(|s| s + from));
    Some(starts)
}

#[derive(Clone, Copy)]
enum Cost {
    /// The square of the line width.
    Balance,
    /// The square of the free space of a line that is not the last; a last
    /// line of one word costs [`ORPHAN_COST`].
    Pretty,
}

/// Splits `items[from..]` into exactly `lines` lines of at most `width`
/// (a line of one group may be wider) with the smallest total cost. The
/// result holds the group where each line starts, from `from`. `None` if
/// there is no split or the work would exceed the bound.
fn split(
    items: &[Item],
    from: usize,
    lines: usize,
    width: f32,
    indent: f32,
    cost: Cost,
) -> Option<Vec<usize>> {
    let items = &items[from..];
    let m = items.len();
    let mut prefix = vec![0.0_f64];
    for item in items {
        prefix.push(prefix.last().copied().unwrap_or(0.0) + f64::from(item.width));
    }
    let line_width = |i: usize, j: usize, first: bool| -> f64 {
        let inner = prefix[j] - prefix[i] - f64::from(items[j - 1].trailing);
        inner + if first { f64::from(indent) } else { 0.0 }
    };
    // best[k][j]: the smallest cost of the first j groups in k lines, and
    // where the last of those lines starts.
    let mut best = vec![vec![(f64::INFINITY, 0_usize); m + 1]; lines + 1];
    best[0][0] = (0.0, 0);
    for k in 1..=lines {
        for j in k..=m {
            for i in (k - 1..j).rev() {
                if j - i > MAX_LINE_GROUPS {
                    break;
                }
                let w = line_width(i, j, i == 0);
                if j - i > 1 && w > f64::from(width) + f64::from(FIT_TOLERANCE) {
                    break;
                }
                let before = best[k - 1][i].0;
                if before.is_infinite() {
                    continue;
                }
                let last = k == lines;
                let c = match cost {
                    Cost::Balance => w * w,
                    Cost::Pretty if last => {
                        if j - i == 1 {
                            ORPHAN_COST
                        } else {
                            0.0
                        }
                    }
                    Cost::Pretty => {
                        let slack = f64::from(width) - w;
                        slack * slack
                    }
                };
                if before + c < best[k][j].0 {
                    best[k][j] = (before + c, i);
                }
            }
        }
    }
    if best[lines][m].0.is_infinite() {
        return None;
    }
    let mut starts = vec![0; lines];
    let mut j = m;
    for k in (1..=lines).rev() {
        let i = best[k][j].1;
        starts[k - 1] = i + from;
        j = i;
    }
    Some(starts)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The groups of a text: each word with a space of 4.453125 px after it
    /// (Arial 16 px as Chromium measured it), the last one without.
    fn words(list: &[(&str, f32)]) -> Vec<Item> {
        const SPACE: f32 = 4.453_125;
        list.iter()
            .enumerate()
            .map(|(i, (_, w))| {
                let last = i + 1 == list.len();
                Item {
                    width: w + if last { 0.0 } else { SPACE },
                    trailing: if last { 0.0 } else { SPACE },
                    forced: false,
                }
            })
            .collect()
    }

    fn title() -> Vec<Item> {
        words(&[
            ("The", 27.578_125),
            ("Open", 39.140_625),
            ("Source", 50.703_125),
            ("Player", 45.359_375),
            ("for", 18.6875),
            ("the", 22.25),
            ("Web", 32.609_375),
        ])
    }

    /// The lines as the number of words in each.
    fn sizes(starts: &[usize], total: usize) -> Vec<usize> {
        let mut at = vec![0];
        at.extend_from_slice(starts);
        at.push(total);
        at.windows(2).map(|w| w[1] - w[0]).collect()
    }

    #[test]
    fn balance_matches_chromium_on_the_title() {
        // Chromium, 16 px Arial: width 260 gives "The Open Source|Player
        // for the Web"; width 120 three lines "The Open|Source Player|for
        // the Web".
        let items = title();
        let two = plan(&items, 260.0, 0.0, TextWrapStyle::Balance);
        assert_eq!(sizes(&two, 7), [3, 4]);
        let three = plan(&items, 120.0, 0.0, TextWrapStyle::Balance);
        assert_eq!(sizes(&three, 7), [2, 2, 3]);
        assert_eq!(
            plan(&items, 260.0, 0.0, TextWrapStyle::Auto),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn pretty_moves_a_word_down_from_an_orphan() {
        // Width 240: greedy gives "The Open Source Player for the|Web";
        // Chromium gives "...for|the Web". Width 200 keeps the greedy
        // breaks.
        let items = title();
        let moved = plan(&items, 240.0, 0.0, TextWrapStyle::Pretty);
        assert_eq!(sizes(&moved, 7), [5, 2]);
        assert_eq!(
            plan(&items, 220.0, 0.0, TextWrapStyle::Pretty),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn a_wide_last_word_is_no_orphan() {
        // 94 px of 200 px: Chromium keeps "performance." alone.
        let items = words(&[("aaaa", 150.0), ("bbbb", 20.0), ("cccc", 94.0)]);
        assert_eq!(
            plan(&items, 200.0, 0.0, TextWrapStyle::Pretty),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn long_paragraphs_are_not_balanced_and_huge_ones_not_planned() {
        let many = vec![
            Item {
                width: 50.0,
                trailing: 4.0,
                forced: false
            };
            40
        ];
        // 40 groups of 50 px in 100 px lines: 20 lines, over the limit.
        assert_eq!(
            plan(&many, 100.0, 0.0, TextWrapStyle::Balance),
            Vec::<usize>::new()
        );
        let huge = vec![
            Item {
                width: 10.0,
                trailing: 4.0,
                forced: false
            };
            MAX_GROUPS + 1
        ];
        assert_eq!(
            plan(&huge, 100.0, 0.0, TextWrapStyle::Pretty),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn forced_breaks_split_paragraphs() {
        let mut items = title();
        items[6].forced = true;
        items.extend(title());
        let starts = plan(&items, 260.0, 0.0, TextWrapStyle::Balance);
        assert_eq!(starts, [3, 10]);
    }
}
