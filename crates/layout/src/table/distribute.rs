//! The distribution of a width to columns: the width distribution
//! algorithm of automatic table layout (CSS Tables 3 §3.9.3,
//! <https://www.w3.org/TR/css-tables-3/#width-distribution-algorithm>) and
//! the column widths of fixed table layout, as Chromium's `LayoutNG`
//! implements them (`table_layout_utils.cc`).

use super::columns::Column;

/// The width distribution algorithm of automatic layout (CSS Tables 3
/// §3.9.3, <https://www.w3.org/TR/css-tables-3/#width-distribution-algorithm>,
/// as Chromium's `DistributeInlineSizeToComputedInlineSizeAuto`): returns
/// the width of each column for a total of `target`. Mergeable columns get
/// 0. With `constrained_target`, columns with a specified width may grow
/// beyond it when there are no auto columns.
pub(crate) fn distribute_auto(
    target: f32,
    columns: &[Column],
    constrained_target: bool,
) -> Vec<f32> {
    let guesses = Guesses::new(columns, target);
    let target = target.max(guesses.size[0]);
    let mut sizes = vec![0.0; columns.len()];
    let start = guesses.size.iter().position(|&g| g >= target);
    let last = match start {
        Some(0) => {
            for &i in &guesses.active {
                sizes[i] = columns[i].min();
            }
            return sizes;
        }
        Some(1) => grow_percent(columns, &guesses, target, &mut sizes),
        Some(2) => grow_fixed(columns, &guesses, target, &mut sizes),
        Some(_) if target == guesses.size[3] => {
            // An exact match uses the max-content widths (and percentage
            // widths), without rounding errors.
            for &i in &guesses.active {
                let c = &columns[i];
                sizes[i] = match kind(c) {
                    Kind::Percent => c.percent_width(target),
                    Kind::Fixed | Kind::Auto => c.max(),
                };
            }
            return sizes;
        }
        Some(_) => grow_auto(columns, &guesses, target, &mut sizes),
        None => grow_above_max(columns, &guesses, target, constrained_target, &mut sizes),
    };
    // Rounding: the last growing column takes the rest.
    if let Some(i) = last {
        let sum: f32 = guesses.active.iter().map(|&j| sizes[j]).sum();
        sizes[i] += target - sum;
    }
    sizes
}

/// The totals of the guesses of the width distribution algorithm (all
/// columns at their min-content width; percentage columns at their
/// percentage; fixed columns at their specified width; auto columns at
/// max-content), and how much the columns of each kind can grow.
struct Guesses {
    /// The columns that are not mergeable.
    active: Vec<usize>,
    size: [f32; 4],
    increase: [f32; 4],
    /// The number of percentage, fixed and auto columns.
    counts: [usize; 3],
    auto_max: f32,
    fixed_max: f32,
    total_percent: f32,
}

impl Guesses {
    fn new(columns: &[Column], target: f32) -> Self {
        let mut g = Guesses {
            active: (0..columns.len())
                .filter(|&i| !columns[i].mergeable)
                .collect(),
            size: [0.0; 4],
            increase: [0.0; 4],
            counts: [0; 3],
            auto_max: 0.0,
            fixed_max: 0.0,
            total_percent: 0.0,
        };
        for &i in &g.active {
            let c = &columns[i];
            let (min, max) = (c.min(), c.max());
            let sizes = match kind(c) {
                Kind::Percent => {
                    g.counts[0] += 1;
                    g.total_percent += c.percent.unwrap_or(0.0);
                    let p = c.percent_width(target);
                    g.increase[1] += p - min;
                    [min, p, p, p]
                }
                Kind::Fixed => {
                    g.counts[1] += 1;
                    g.fixed_max += max;
                    g.increase[2] += max - min;
                    [min, min, max, max]
                }
                Kind::Auto => {
                    g.counts[2] += 1;
                    g.auto_max += max;
                    g.increase[3] += max - min;
                    [min, min, min, max]
                }
            };
            for (total, size) in g.size.iter_mut().zip(sizes) {
                *total += size;
            }
        }
        g
    }
}

/// The kind of a column in width distribution.
fn kind(c: &Column) -> Kind {
    if c.percent.is_some() {
        Kind::Percent
    } else if c.constrained {
        Kind::Fixed
    } else {
        Kind::Auto
    }
}

/// Percentage columns grow in proportion to the difference between their
/// percentage width and their min-content width; the others take their
/// min-content width. Returns the last growing column.
fn grow_percent(columns: &[Column], g: &Guesses, target: f32, sizes: &mut [f32]) -> Option<usize> {
    let available = target - g.size[0];
    let mut last = None;
    for &i in &g.active {
        let c = &columns[i];
        sizes[i] = c.min();
        if kind(c) == Kind::Percent {
            let grow = c.percent_width(target) - c.min();
            sizes[i] += share(available, grow, g.increase[1], g.counts[0]);
            last = Some(i);
        }
    }
    last
}

/// Fixed columns grow toward their specified width; percentage columns
/// take their percentage width, auto columns their min-content width.
fn grow_fixed(columns: &[Column], g: &Guesses, target: f32, sizes: &mut [f32]) -> Option<usize> {
    let available = target - g.size[1];
    let mut last = None;
    for &i in &g.active {
        let c = &columns[i];
        sizes[i] = match kind(c) {
            Kind::Percent => c.percent_width(target),
            Kind::Fixed => {
                last = Some(i);
                c.min() + share(available, c.max() - c.min(), g.increase[2], g.counts[1])
            }
            Kind::Auto => c.min(),
        };
    }
    last
}

/// Auto columns grow toward their max-content width; the others take
/// their percentage or specified width.
fn grow_auto(columns: &[Column], g: &Guesses, target: f32, sizes: &mut [f32]) -> Option<usize> {
    let available = target - g.size[2];
    let mut last = None;
    for &i in &g.active {
        let c = &columns[i];
        sizes[i] = match kind(c) {
            Kind::Percent => c.percent_width(target),
            Kind::Fixed => c.max(),
            Kind::Auto => {
                last = Some(i);
                c.min() + share(available, c.max() - c.min(), g.increase[3], g.counts[2])
            }
        };
    }
    last
}

/// Above all max-content widths: auto columns grow in proportion to their
/// max-content widths; without auto columns, fixed columns (if allowed);
/// else percentage columns in proportion to their percentages.
fn grow_above_max(
    columns: &[Column],
    g: &Guesses,
    target: f32,
    constrained_target: bool,
    sizes: &mut [f32],
) -> Option<usize> {
    let available = target - g.size[3];
    for &i in &g.active {
        let c = &columns[i];
        sizes[i] = match kind(c) {
            Kind::Percent => c.percent_width(target),
            Kind::Fixed | Kind::Auto => c.max(),
        };
    }
    let (grow_kind, total, count) = if g.counts[2] > 0 {
        (Kind::Auto, g.auto_max, g.counts[2])
    } else if g.counts[1] > 0 && constrained_target {
        (Kind::Fixed, g.fixed_max, g.counts[1])
    } else if g.counts[0] > 0 {
        (Kind::Percent, g.total_percent, g.counts[0])
    } else {
        return None;
    };
    let mut last = None;
    for &i in &g.active {
        let c = &columns[i];
        if kind(c) != grow_kind {
            continue;
        }
        let weight = if grow_kind == Kind::Percent {
            c.percent.unwrap_or(0.0)
        } else {
            c.max()
        };
        sizes[i] += share(available, weight, total, count);
        last = Some(i);
    }
    last
}

/// Kinds of columns in width distribution.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Percent,
    Fixed,
    Auto,
}

/// The share of `available` for a column with `weight` out of `total`;
/// an even share of `count` columns if the total is zero.
fn share(available: f32, weight: f32, total: f32, count: usize) -> f32 {
    if total > 0.0 {
        available * weight / total
    } else {
        available / count.max(1) as f32
    }
}

/// The column widths of fixed table layout for an assignable width of
/// `target` (Chromium's `SynchronizeAssignableTableInlineSizeAndColumnsFixed`):
/// columns with a fixed width first (scaled down if they do not fit, up if
/// there are no auto columns), then percentage columns, then auto columns
/// share the rest evenly. Columns with a width of 0 count as auto columns
/// that grow only if all columns have a width of 0.
pub(crate) fn distribute_fixed(target: f32, columns: &[Column]) -> Vec<f32> {
    let mut sizes = vec![0.0; columns.len()];
    if columns.is_empty() {
        return sizes;
    }
    let treat_as_fixed = |c: &Column| c.is_fixed() && c.max() != 0.0;
    let zero_width = |c: &Column| c.constrained && c.max() == 0.0;
    let is_percent = |c: &Column| c.percent.is_some();
    let percent_width = |c: &Column| c.percent_width(target);
    let zero_count = columns.iter().filter(|c| zero_width(c)).count();
    let auto_count = columns
        .iter()
        .filter(|c| !is_percent(c) && !treat_as_fixed(c) && !zero_width(c))
        .count();
    let percent_total: f32 = columns
        .iter()
        .filter(|c| is_percent(c))
        .map(percent_width)
        .sum();
    let grow = auto_count == 0;
    let mut last = 0;
    let fixed_room = (target - percent_total).max(0.0);
    let fixed = Scaled {
        room: fixed_room,
        limit: target,
        grow,
    };
    let mut assigned = fixed.assign(columns, &mut sizes, &mut last, treat_as_fixed, Column::max);
    if assigned >= target {
        return sizes;
    }
    let left = target - assigned;
    let percent = Scaled {
        room: left,
        limit: left,
        grow,
    };
    assigned += percent.assign(columns, &mut sizes, &mut last, is_percent, percent_width);
    // Auto columns share the rest; columns of width 0 only if all are.
    let left = target - assigned;
    let all_zero = zero_count == columns.len();
    let share_count = if all_zero { zero_count } else { auto_count };
    for (i, c) in columns.iter().enumerate() {
        if is_percent(c) || treat_as_fixed(c) || (zero_width(c) && !all_zero) {
            continue;
        }
        last = i;
        sizes[i] = left / share_count.max(1) as f32;
        assigned += sizes[i];
    }
    sizes[last] += target - assigned;
    sizes
}

/// How fixed layout gives a kind of columns their widths: scaled to fill
/// `room` if their total is more than `limit` or (with `grow`) less than
/// `room`.
struct Scaled {
    room: f32,
    limit: f32,
    grow: bool,
}

impl Scaled {
    /// Gives the columns that `pick` selects their `width`, scaled; evenly
    /// if their total is 0. Returns the total and updates `last`, the last
    /// column with a width.
    fn assign(
        &self,
        columns: &[Column],
        sizes: &mut [f32],
        last: &mut usize,
        pick: impl Fn(&Column) -> bool,
        width: impl Fn(&Column) -> f32,
    ) -> f32 {
        let picked: Vec<usize> = (0..columns.len()).filter(|&i| pick(&columns[i])).collect();
        let total: f32 = picked.iter().map(|&i| width(&columns[i])).sum();
        let scale = if (self.grow && total < self.room) || total > self.limit {
            (total != 0.0).then(|| self.room / total)
        } else {
            Some(1.0)
        };
        let mut assigned = 0.0;
        for &i in &picked {
            *last = i;
            sizes[i] = match scale {
                Some(scale) => scale * width(&columns[i]),
                None => self.room / picked.len() as f32,
            };
            assigned += sizes[i];
        }
        assigned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auto(min: f32, max: f32) -> Column {
        Column {
            min: Some(min),
            max: Some(max),
            ..Column::default()
        }
    }

    fn fixed(min: f32, max: f32) -> Column {
        Column {
            constrained: true,
            ..auto(min, max)
        }
    }

    fn percent(min: f32, max: f32, p: f32) -> Column {
        Column {
            percent: Some(p),
            ..auto(min, max)
        }
    }

    fn close(a: &[f32], b: &[f32]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.01)
    }

    #[test]
    fn excess_goes_to_auto_columns_by_max_content() {
        // Hacker News's header table: 100% wide in 1070.39px.
        let columns = [fixed(24.0, 24.0), auto(5.0, 380.97), auto(5.0, 32.17)];
        let sizes = distribute_auto(1070.39, &columns, true);
        assert!(close(&sizes, &[24.0, 964.91, 81.48]), "{sizes:?}");
    }

    #[test]
    fn guesses() {
        let columns = [
            auto(10.0, 100.0),
            fixed(20.0, 50.0),
            percent(10.0, 10.0, 50.0),
        ];
        // Below all min-content widths: min-content widths.
        assert!(close(
            &distribute_auto(10.0, &columns, true),
            &[10.0, 20.0, 10.0]
        ));
        // Between min and percentage guess: only the percentage column
        // grows.
        assert!(close(
            &distribute_auto(60.0, &columns, true),
            &[10.0, 20.0, 30.0]
        ));
        // Between percentage and specified guess: the fixed column grows.
        let sizes = distribute_auto(120.0, &columns, true);
        assert!(close(&sizes, &[10.0, 50.0, 60.0]), "{sizes:?}");
        // Between specified and max guess: the auto column grows.
        let sizes = distribute_auto(200.0, &columns, true);
        assert!(close(&sizes, &[50.0, 50.0, 100.0]), "{sizes:?}");
    }

    #[test]
    fn fixed_layout() {
        let columns = [fixed(0.0, 30.0), auto(0.0, 0.0), auto(0.0, 0.0)];
        assert!(close(
            &distribute_fixed(192.0, &columns),
            &[30.0, 81.0, 81.0]
        ));
        // Fixed columns scale down when they do not fit.
        let columns = [fixed(0.0, 100.0), fixed(0.0, 100.0)];
        assert!(close(&distribute_fixed(100.0, &columns), &[50.0, 50.0]));
        // ... and up when there are no auto columns.
        assert!(close(&distribute_fixed(400.0, &columns), &[200.0, 200.0]));
    }
}
