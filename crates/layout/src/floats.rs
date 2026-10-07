//! Floats: the exclusion space of a block formatting context, float
//! placement, clearance, layout opportunities, and the block position of
//! containers whose top margin can still collapse.
//!
//! CSS 2.2 §9.5 (<https://www.w3.org/TR/CSS22/visuren.html#floats>),
//! §9.5.1 (float placement rules 1–9), §9.5.2 (clearance). Chromium's
//! `LayoutNG` is the reference where the spec leaves room (see ADR 0015):
//!
//! - The exclusion space is the set of float margin boxes of one block
//!   formatting context (BFC), in the coordinates of the BFC root's content
//!   box. It is stored as two step functions over the block axis (one
//!   sorted list of segments): the rightmost right edge of the left floats
//!   and the leftmost left edge of the right floats. This model is swb's
//!   own; it is not Chromium's data structure.
//! - A float is placed at the first block position at or below its origin,
//!   the top of the last float (rule 5) and its clearance where its margin
//!   box fits beside the floats. Because no float starts below the top of
//!   the last float, the free space below that position only grows, so the
//!   free range at that position decides.
//! - Line boxes and boxes that establish a BFC use layout opportunities:
//!   rectangles free of floats from a block position down. They are
//!   produced one at a time ([`Bfc::opportunity_at`],
//!   [`Bfc::narrowing_below`], [`Bfc::narrower`], [`Bfc::next_top`]), so
//!   that a line visits only the floats beside it.
//!
//! Margin collapsing means that the block position of a container is not
//! known until its first content: a line box, a border, a box that
//! establishes a BFC, or clearance. Containers without a known position are
//! a chain of [`Frame`]s in the [`Bfc`]. Floats inside them wait in their
//! frame and are placed when the chain is resolved, or, for an empty block
//! whose margins collapse through it, at the position of that block. This is
//! what Chromium does with its relayout for a changed BFC block offset,
//! without the relayout. A container that clears waiting floats has a
//! forced position: the margins in it have no effect.
//!
//! Limits for hostile content: a BFC places at most [`MAX_FLOATS`] floats
//! (more floats are placed below all others) and keeps at most as many
//! waiting floats (more are not placed), and all exclusion space queries
//! of a layout pass share a work budget ([`WORK_BUDGET`] visited
//! segments); when it is spent, floats and boxes are placed below all
//! floats of their BFC.

use std::sync::Arc;

use swb_style::Clear;

use crate::block::CollapsedMargin;
use crate::fragment::Fragment;
use crate::geom::Point;

/// The most floats that one block formatting context places with the
/// placement rules (later floats are placed below all floats of the BFC),
/// and the most floats that wait in it for the position of their
/// container (later ones are not placed).
pub(crate) const MAX_FLOATS: usize = 10_000;

/// The work units that one layout pass may spend next to floats: one per
/// segment visit of an exclusion space query, plus the charges for lines
/// and boxes that establish a BFC that are laid out again for another
/// layout opportunity. After that, floats, lines and boxes that would need
/// a search are placed below all floats of their BFC.
pub(crate) const WORK_BUDGET: u64 = 20_000_000;

/// Tolerance for comparisons of widths and positions (f32 rounding).
pub(crate) const EPSILON: f32 = 0.01;

/// The side of a float.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Left,
    Right,
}

/// A step of the exclusion space: from `y` to the start of the next step.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Segment {
    y: f32,
    /// The rightmost right margin edge of the left floats (−∞ if none).
    left: f32,
    /// The leftmost left margin edge of the right floats (+∞ if none).
    right: f32,
}

impl Segment {
    /// The free range of the segment inside `cx0..cx1`.
    fn clip(&self, cx0: f32, cx1: f32) -> (f32, f32) {
        (self.left.max(cx0), self.right.min(cx1))
    }
}

/// A layout opportunity: a rectangle free of floats from `top` down, in
/// BFC coordinates. How far down it reaches is found only when needed
/// (`Bfc::narrowing_below`): most lines and boxes fit in the first
/// opportunity, so the floats far below are never visited.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Opportunity {
    pub(crate) left: f32,
    pub(crate) right: f32,
    pub(crate) top: f32,
    /// True if no float narrows the containing block at `top`: a line
    /// that does not fit here does not fit anywhere (Chromium's
    /// `IsEqualToAvailableFloatInlineSize`).
    pub(crate) full_width: bool,
    /// True if no float reaches below `top`: everything fits here.
    pub(crate) last: bool,
    /// The first segment below `top` that may narrow the rectangle.
    scan: usize,
}

impl Opportunity {
    pub(crate) fn width(&self) -> f32 {
        (self.right - self.left).max(0.0)
    }

    /// The whole containing block `cx0..cx1` at `top`, with no float
    /// anywhere.
    pub(crate) fn whole(top: f32, cx0: f32, cx1: f32) -> Self {
        Opportunity {
            left: cx0,
            right: cx1,
            top,
            full_width: true,
            last: true,
            scan: usize::MAX,
        }
    }
}

/// The float margin boxes of one block formatting context.
#[derive(Clone, Debug)]
pub(crate) struct Exclusions {
    /// Sorted by `y`; the first starts at −∞, the last extends to +∞.
    /// Empty until the first float (most BFCs have none).
    segments: Vec<Segment>,
    /// The top of the last placed float (rule 5).
    last_float_top: f32,
    /// The bottom of the lowest left and right float.
    left_bottom: f32,
    right_bottom: f32,
    count: usize,
}

impl Default for Exclusions {
    fn default() -> Self {
        Exclusions {
            segments: Vec::new(),
            last_float_top: f32::NEG_INFINITY,
            left_bottom: f32::NEG_INFINITY,
            right_bottom: f32::NEG_INFINITY,
            count: 0,
        }
    }
}

impl Exclusions {
    /// True if no float was placed.
    pub(crate) fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The bottom of the lowest float margin box, if there is a float.
    pub(crate) fn bottom(&self) -> Option<f32> {
        let bottom = self.left_bottom.max(self.right_bottom);
        (bottom > f32::NEG_INFINITY).then_some(bottom)
    }

    /// The clearance offset for `clear`: the bottom of the lowest float on
    /// the cleared sides (CSS 2.2 §9.5.2), if there is one.
    pub(crate) fn clearance(&self, clear: Clear) -> Option<f32> {
        let bottom = match clear {
            Clear::None => return None,
            Clear::Left => self.left_bottom,
            Clear::Right => self.right_bottom,
            Clear::Both => self.left_bottom.max(self.right_bottom),
        };
        (bottom > f32::NEG_INFINITY).then_some(bottom)
    }

    /// The top of the last placed float.
    pub(crate) fn last_float_top(&self) -> f32 {
        self.last_float_top
    }

    /// The index of the segment that contains `y`.
    fn index_at(&self, y: f32) -> usize {
        self.segments
            .partition_point(|s| s.y <= y)
            .saturating_sub(1)
    }

    /// The free range at block position `y` inside `cx0..cx1`.
    pub(crate) fn range_at(&self, y: f32, cx0: f32, cx1: f32) -> (f32, f32) {
        self.segments
            .get(self.index_at(y))
            .map_or((cx0, cx1), |s| s.clip(cx0, cx1))
    }

    /// The start of the first segment below `y`.
    fn next_top(&self, y: f32) -> Option<f32> {
        let i = self.segments.partition_point(|s| s.y <= y);
        self.segments.get(i).map(|s| s.y)
    }

    /// The segments that a binary search visits: its work budget cost.
    fn search_cost(&self) -> u64 {
        u64::from(self.segments.len().max(1).ilog2()) + 1
    }

    /// Makes a segment start at `y` and returns its index.
    fn split_at(&mut self, y: f32) -> usize {
        if self.segments.is_empty() {
            self.segments.push(Segment {
                y: f32::NEG_INFINITY,
                left: f32::NEG_INFINITY,
                right: f32::INFINITY,
            });
        }
        let i = self.index_at(y);
        let Some(&segment) = self.segments.get(i) else {
            return 0;
        };
        if segment.y == y {
            return i;
        }
        self.segments.insert(i + 1, Segment { y, ..segment });
        i + 1
    }

    /// Adds the margin box `x0..x1` × `top..bottom` of a float. The
    /// segments after `top` move: that costs `budget`.
    fn add(&mut self, side: Side, (x0, x1): (f32, f32), top: f32, bottom: f32, budget: &mut u64) {
        self.count += 1;
        self.last_float_top = self.last_float_top.max(top);
        match side {
            Side::Left => self.left_bottom = self.left_bottom.max(bottom),
            Side::Right => self.right_bottom = self.right_bottom.max(bottom),
        }
        if bottom <= top || top.is_nan() || bottom.is_nan() {
            return;
        }
        let start = self.split_at(top);
        let end = self.split_at(bottom);
        let moved = self.segments.len().saturating_sub(start);
        *budget = budget.saturating_sub(u64::try_from(moved).unwrap_or(u64::MAX));
        for s in self.segments.get_mut(start..end).unwrap_or_default() {
            match side {
                Side::Left => s.left = s.left.max(x1),
                Side::Right => s.right = s.right.min(x0),
            }
        }
        // Merge equal neighbors around the changed segments.
        let lo = start.saturating_sub(1);
        let hi = (end + 1).min(self.segments.len());
        let mut window = self.segments[lo..hi].to_vec();
        window.dedup_by(|b, a| a.left == b.left && a.right == b.right);
        self.segments.splice(lo..hi, window);
    }

    /// The widest layout opportunity at block position `top` inside
    /// `cx0..cx1`: the free range there. `None` if there is no free space.
    fn opportunity_at(&self, top: f32, cx0: f32, cx1: f32) -> Option<Opportunity> {
        let index = self.index_at(top);
        let Some(segment) = self.segments.get(index) else {
            return Some(Opportunity::whole(top, cx0, cx1));
        };
        let (left, right) = segment.clip(cx0, cx1);
        let full_width = left <= cx0 && right >= cx1;
        (left < right || full_width).then(|| Opportunity {
            left,
            right,
            top,
            full_width,
            last: self.bottom().is_none_or(|bottom| top >= bottom),
            scan: index + 1,
        })
    }

    /// The first segment that starts below the top of `o` and above
    /// `until` and narrows `o` (a line or box of that height does not fit
    /// in `o`).
    fn narrowing_below(
        &self,
        o: &Opportunity,
        until: f32,
        cx0: f32,
        cx1: f32,
        budget: &mut u64,
    ) -> Option<usize> {
        let mut index = o.scan;
        while let Some(s) = self.segments.get(index) {
            if s.y >= until - EPSILON {
                return None;
            }
            *budget = budget.saturating_sub(1);
            let (l, r) = s.clip(cx0, cx1);
            if l > o.left || r < o.right {
                return Some(index);
            }
            index += 1;
        }
        None
    }

    /// The next opportunity at the top of `o`: it reaches past the segment
    /// `at`, which narrows `o`, so it is narrower (and taller).
    fn narrower(&self, o: &Opportunity, at: usize, cx0: f32, cx1: f32) -> Option<Opportunity> {
        let (l, r) = self.segments.get(at)?.clip(cx0, cx1);
        let (left, right) = (o.left.max(l), o.right.min(r));
        (left < right).then_some(Opportunity {
            left,
            right,
            top: o.top,
            full_width: false,
            last: false,
            scan: at + 1,
        })
    }

    /// The opportunity below all floats at or below `top` (the last
    /// opportunity).
    pub(crate) fn below_all(&self, top: f32, cx0: f32, cx1: f32) -> Opportunity {
        Opportunity::whole(top.max(self.bottom().unwrap_or(top)), cx0, cx1)
    }

    /// Places a float with the rules of CSS 2.2 §9.5.1 at or below
    /// `origin`. Returns the top-left corner of its margin box.
    pub(crate) fn place(&mut self, float: &FloatBox, origin: f32, budget: &mut u64) -> Point {
        let mut y = origin.max(self.last_float_top);
        if let Some(c) = self.clearance(float.clear) {
            y = y.max(c);
        }
        // A negative margin-box width (negative margins) places the float
        // as it is, but excludes nothing.
        let width = float.width.max(0.0);
        let (cx0, cx1) = (float.cb_x, float.cb_x + float.cb_width.max(0.0));
        let (left, right) = if self.count >= MAX_FLOATS || *budget == 0 {
            y = y.max(self.bottom().unwrap_or(y));
            (cx0, cx1)
        } else {
            loop {
                let (left, right) = self.range_at(y, cx0, cx1);
                *budget = budget.saturating_sub(1);
                let full = left <= cx0 && right >= cx1;
                if full || right - left + EPSILON >= width {
                    break (left, right);
                }
                match self.next_top(y) {
                    Some(next) if next > y && *budget > 0 => y = next,
                    _ => {
                        y = y.max(self.bottom().unwrap_or(y));
                        break (cx0, cx1);
                    }
                }
            }
        };
        let x = match float.side {
            Side::Left => left,
            Side::Right => right - float.width,
        };
        self.add(
            float.side,
            (x, x + width),
            y,
            y + float.height.max(0.0),
            budget,
        );
        Point::new(x, y)
    }
}

/// What float placement needs to know about a laid-out float.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FloatBox {
    pub(crate) side: Side,
    pub(crate) clear: Clear,
    /// The size of the margin box.
    pub(crate) width: f32,
    pub(crate) height: f32,
    /// The offset of the border box in the margin box (the left and top
    /// margins).
    pub(crate) offset: Point,
    /// The offset of relative positioning.
    pub(crate) relative: Point,
    /// The left edge and width of the containing block's content box.
    pub(crate) cb_x: f32,
    pub(crate) cb_width: f32,
}

/// A float that waits for the position of its container.
#[derive(Clone, Debug)]
pub(crate) struct PendingFloat {
    /// Fragment indices, innermost first: the last is an index into the
    /// children of the frame's container, each one before it an index into
    /// the children of the fragment that the next one selects. (Reversed,
    /// so that moving the float to an outer frame appends an index.)
    pub(crate) path: Vec<u32>,
    pub(crate) float: FloatBox,
    /// The BFC x of the coordinate origin of the float's parent fragment.
    /// Its parent's origin y is the position of the frame.
    pub(crate) origin_x: f32,
}

/// A float that was placed while it was pending: the position of its
/// border box in its parent fragment (relative positioning included).
#[derive(Clone, Debug)]
pub(crate) struct PlacedFloat {
    pub(crate) path: Vec<u32>,
    pub(crate) position: Point,
}

/// A block container in the BFC that is being laid out.
#[derive(Clone, Debug, Default)]
struct Frame {
    /// Where the collapsing margins before the container start.
    before: f32,
    /// The margins before the container's own top margin.
    strut_before: CollapsedMargin,
    /// The container's clearance offset, if it has `clear` and there are
    /// floats to clear.
    clearance: Option<f32>,
    /// The position of a container that clears floats waiting in the chain
    /// (see [`Bfc::push_frame`]).
    forced: Option<f32>,
    /// The BFC y of the container's border-box top, once known. A
    /// container without a known position has no top border or padding,
    /// so it is also the top of its content box.
    resolved: Option<f32>,
    pending: Vec<PendingFloat>,
    placed: Vec<PlacedFloat>,
}

/// The floats of a container frame after its layout.
pub(crate) struct FrameFloats {
    /// Pending floats that were placed: the container moves their
    /// fragments.
    pub(crate) placed: Vec<PlacedFloat>,
    /// Floats that still wait (the container is an empty block whose
    /// margins collapse through it).
    pub(crate) pending: Vec<PendingFloat>,
}

/// A saved state of the BFC, to undo a speculative resolution.
pub(crate) struct Checkpoint {
    exclusions: Exclusions,
    /// The first frame of the chain.
    first: usize,
    frames: Vec<FrameState>,
    waiting: Waiting,
}

/// The saved state of a frame of the chain.
struct FrameState {
    resolved: Option<f32>,
    pending: Vec<PendingFloat>,
    /// The number of placed floats.
    placed: usize,
}

/// The numbers of floats that wait for the position of their container,
/// per side. They all wait in the chain of frames without a known
/// position: a frame with a known position places its floats at once.
#[derive(Clone, Copy, Debug, Default)]
struct Waiting {
    left: usize,
    right: usize,
}

impl Waiting {
    fn total(self) -> usize {
        self.left + self.right
    }

    fn count(&mut self, side: Side) -> &mut usize {
        match side {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        }
    }
}

/// The state of one block formatting context during layout.
#[derive(Debug)]
pub(crate) struct Bfc {
    pub(crate) exclusions: Exclusions,
    frames: Vec<Frame>,
    waiting: Waiting,
    /// The remaining work budget of the layout pass.
    pub(crate) budget: u64,
}

impl Bfc {
    /// A new BFC whose root container is at the origin.
    pub(crate) fn new(budget: u64) -> Self {
        Bfc {
            exclusions: Exclusions::default(),
            frames: vec![Frame {
                resolved: Some(0.0),
                ..Frame::default()
            }],
            waiting: Waiting::default(),
            budget,
        }
    }

    /// Makes this a new BFC again, keeping the allocations.
    pub(crate) fn reset(&mut self, budget: u64) {
        let mut segments = std::mem::take(&mut self.exclusions.segments);
        segments.clear();
        self.exclusions = Exclusions {
            segments,
            ..Exclusions::default()
        };
        self.frames.clear();
        self.frames.push(Frame {
            resolved: Some(0.0),
            ..Frame::default()
        });
        self.waiting = Waiting::default();
        self.budget = budget;
    }

    /// Starts a block container whose margins start at `before` after the
    /// margins `strut` (the container's own top margin not included).
    /// `clearance` is its clearance offset; `forced` its position if it
    /// clears floats that waited in the chain (Chromium's forced BFC block
    /// offset: then the margins in it do not count).
    pub(crate) fn push_frame(
        &mut self,
        before: f32,
        strut: CollapsedMargin,
        clearance: Option<f32>,
        forced: Option<f32>,
    ) {
        self.frames.push(Frame {
            before,
            strut_before: strut,
            clearance,
            forced,
            ..Frame::default()
        });
    }

    /// Ends the innermost block container.
    pub(crate) fn pop_frame(&mut self) -> FrameFloats {
        let frame = self.frames.pop().unwrap_or_default();
        FrameFloats {
            placed: frame.placed,
            pending: frame.pending,
        }
    }

    /// The BFC y of the innermost container's border-box top, if known.
    pub(crate) fn resolved(&self) -> Option<f32> {
        self.frames.last().and_then(|f| f.resolved)
    }

    /// Adds a float that waits for the position of the innermost
    /// container. A BFC keeps at most [`MAX_FLOATS`] waiting floats; a
    /// float beyond that is not placed (it stays at the top left of its
    /// container and excludes nothing).
    pub(crate) fn add_pending(&mut self, float: PendingFloat) {
        if self.waiting.total() >= MAX_FLOATS {
            return;
        }
        if let Some(frame) = self.frames.last_mut() {
            *self.waiting.count(float.float.side) += 1;
            frame.pending.push(float);
        }
    }

    /// Adds the waiting floats of an empty child, whose fragment has index
    /// `index` among the innermost container's children.
    pub(crate) fn adopt_pending(&mut self, floats: Vec<PendingFloat>, index: usize) {
        if let Some(frame) = self.frames.last_mut() {
            let index = u32::try_from(index).unwrap_or(u32::MAX);
            for mut float in floats {
                float.path.push(index);
                frame.pending.push(float);
            }
        }
    }

    /// Places a waiting float of an empty child at the child's top
    /// (`top`); returns its position in its parent fragment.
    pub(crate) fn place_waiting(&mut self, float: &PendingFloat, top: f32) -> Point {
        let count = self.waiting.count(float.float.side);
        *count = count.saturating_sub(1);
        self.place_in(&float.float, top, Point::new(float.origin_x, top))
    }

    /// True if a float waits in the chain of containers without a known
    /// position.
    pub(crate) fn has_pending(&self) -> bool {
        self.waiting.total() > 0
    }

    /// True if a box with `clear` would clear a float that waits in the
    /// chain of containers without a known position (Chromium's
    /// `HasClearancePastAdjoiningFloats`).
    pub(crate) fn clears_pending(&self, clear: Clear) -> bool {
        let Waiting { left, right } = self.waiting;
        match clear {
            Clear::None => false,
            Clear::Left => left > 0,
            Clear::Right => right > 0,
            Clear::Both => left + right > 0,
        }
    }

    /// The index of the first frame without a known position.
    fn first_unresolved(&self) -> usize {
        let mut first = self.frames.len();
        while first > 0 && self.frames[first - 1].resolved.is_none() {
            first -= 1;
        }
        first
    }

    /// Resolves the position of the chain of containers without a known
    /// position, at their first content: `strut` are all margins from the
    /// start of the chain down to that content. A container with clearance
    /// past the resulting position is placed at its clearance offset; the
    /// containers outside it end before its margins (CSS 2.2 §9.5.2,
    /// Chromium). Then the waiting floats are placed, outer containers
    /// first. Returns the position of the innermost container.
    pub(crate) fn resolve(&mut self, strut: CollapsedMargin) -> f32 {
        let first = self.first_unresolved();
        let len = self.frames.len();
        if first == len {
            return self.resolved().unwrap_or(0.0);
        }
        let before = self.frames[first].before;
        let hypothetical = before + strut.solve();
        // From the inside out: a container needs clearance if its clearance
        // offset is below its hypothetical position, which is the position
        // before the margins of the next inner container with clearance, if
        // there is one. The outermost container with clearance decides.
        // A container with a forced position always counts.
        let mut outer_cleared = None;
        let mut position = hypothetical;
        for i in (first..len).rev() {
            let frame = &self.frames[i];
            if frame.forced.is_some() || frame.clearance.is_some_and(|c| c > position) {
                outer_cleared = Some(i);
                position = before + frame.strut_before.solve();
            }
        }
        match outer_cleared {
            None => {
                for f in &mut self.frames[first..] {
                    f.resolved = Some(hypothetical);
                }
            }
            Some(k) => {
                // The containers outside it end before its margins; it and
                // the containers inside it are at its clearance offset, or
                // lower at their own.
                for f in &mut self.frames[first..k] {
                    f.resolved = Some(position);
                }
                let mut y = position;
                for f in &mut self.frames[k..] {
                    y = f.forced.unwrap_or(y);
                    y = f.clearance.map_or(y, |c| y.max(c));
                    f.resolved = Some(y);
                }
            }
        }
        for i in first..len {
            let origin = self.frames[i].resolved.unwrap_or(hypothetical);
            let pending = std::mem::take(&mut self.frames[i].pending);
            for float in pending {
                let count = self.waiting.count(float.float.side);
                *count = count.saturating_sub(1);
                let parent = Point::new(float.origin_x, origin);
                let position = self.place_in(&float.float, origin, parent);
                self.frames[i].placed.push(PlacedFloat {
                    path: float.path,
                    position,
                });
            }
        }
        self.frames[len - 1].resolved.unwrap_or(hypothetical)
    }

    /// Places a float at or below `origin`. Its parent fragment's
    /// coordinate origin is at BFC position `parent`. Returns the position
    /// of the float's border box in its parent (relative positioning
    /// included).
    pub(crate) fn place_in(&mut self, float: &FloatBox, origin: f32, parent: Point) -> Point {
        let corner = self.place_float(float, origin);
        Point::new(
            corner.x + float.offset.x + float.relative.x - parent.x,
            corner.y + float.offset.y + float.relative.y - parent.y,
        )
    }

    /// The widest layout opportunity at block position `y` inside
    /// `cx0..cx1` (`None` if there is no free space). With no work budget
    /// left, the opportunity below all floats.
    pub(crate) fn opportunity_at(&mut self, y: f32, cx0: f32, cx1: f32) -> Option<Opportunity> {
        if self.budget == 0 {
            return Some(self.exclusions.below_all(y, cx0, cx1));
        }
        self.charge(self.exclusions.search_cost());
        self.exclusions.opportunity_at(y, cx0, cx1)
    }

    /// The segment that narrows `o` (in a containing block `cx0..cx1`)
    /// within `height` below its top, if any: a line or box that tall does
    /// not fit in `o`; the next narrower opportunity reaches past that
    /// segment ([`Bfc::narrower`]). With `before`, the check uses the
    /// exclusion space saved there: a line checks its height without the
    /// floats it placed itself, in the state that `o` was found in and that
    /// a rejected line returns to.
    pub(crate) fn narrowing_below(
        &mut self,
        o: &Opportunity,
        height: f32,
        cx0: f32,
        cx1: f32,
        before: Option<&Checkpoint>,
    ) -> Option<usize> {
        if o.last {
            return None;
        }
        let exclusions = before.map_or(&self.exclusions, |c| &c.exclusions);
        exclusions.narrowing_below(o, o.top + height, cx0, cx1, &mut self.budget)
    }

    /// The next opportunity at the top of `o`, past the narrowing segment
    /// `at`.
    pub(crate) fn narrower(
        &mut self,
        o: &Opportunity,
        at: usize,
        cx0: f32,
        cx1: f32,
    ) -> Option<Opportunity> {
        self.budget = self.budget.saturating_sub(1);
        self.exclusions.narrower(o, at, cx0, cx1)
    }

    /// The next block position below `y` where the free space can change
    /// (`None` after the last float, or with no work budget left).
    pub(crate) fn next_top(&mut self, y: f32) -> Option<f32> {
        if self.budget == 0 {
            return None;
        }
        self.charge(self.exclusions.search_cost());
        self.exclusions.next_top(y).filter(|&n| n > y)
    }

    /// Charges `units` of work to the budget.
    pub(crate) fn charge(&mut self, units: u64) {
        self.budget = self.budget.saturating_sub(units);
    }

    /// Places a float at or below `origin`; returns the BFC position of the
    /// top-left corner of its margin box.
    pub(crate) fn place_float(&mut self, float: &FloatBox, origin: f32) -> Point {
        self.exclusions.place(float, origin, &mut self.budget)
    }

    /// Saves the state of the chain and of the exclusion space. The copy
    /// costs work budget.
    pub(crate) fn checkpoint(&mut self) -> Checkpoint {
        let first = self.first_unresolved();
        let size = self.exclusions.segments.len()
            + self.frames[first..]
                .iter()
                .map(|f| f.pending.len())
                .sum::<usize>();
        self.budget = self
            .budget
            .saturating_sub(u64::try_from(size).unwrap_or(u64::MAX));
        Checkpoint {
            exclusions: self.exclusions.clone(),
            first,
            frames: self.frames[first..]
                .iter()
                .map(|f| FrameState {
                    resolved: f.resolved,
                    pending: f.pending.clone(),
                    placed: f.placed.len(),
                })
                .collect(),
            waiting: self.waiting,
        }
    }

    /// Returns to a saved state (the frames are the same as when it was
    /// saved).
    pub(crate) fn restore(&mut self, checkpoint: Checkpoint) {
        self.exclusions = checkpoint.exclusions;
        self.waiting = checkpoint.waiting;
        for (frame, state) in self.frames[checkpoint.first..]
            .iter_mut()
            .zip(checkpoint.frames)
        {
            frame.resolved = state.resolved;
            frame.pending = state.pending;
            frame.placed.truncate(state.placed);
        }
    }
}

/// Moves the fragment at `path` (indices, innermost first: the last into
/// `fragments`, each one before it into the children of the fragment that
/// the next one selects) to `position` in its parent.
pub(crate) fn move_fragment(fragments: &mut [Fragment], path: &[u32], position: Point) {
    let Some((&last, rest)) = path.split_last() else {
        return;
    };
    let Some(Fragment::Box(b)) = usize::try_from(last)
        .ok()
        .and_then(|i| fragments.get_mut(i))
    else {
        return;
    };
    if rest.is_empty() {
        b.border_rect.x = position.x;
        b.border_rect.y = position.y;
    } else {
        move_fragment(
            Arc::make_mut(&mut b.children).as_mut_slice(),
            rest,
            position,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn float(side: Side, width: f32, height: f32) -> FloatBox {
        FloatBox {
            side,
            clear: Clear::None,
            width,
            height,
            offset: Point::default(),
            relative: Point::default(),
            cb_x: 0.0,
            cb_width: 300.0,
        }
    }

    fn place(e: &mut Exclusions, f: FloatBox, origin: f32) -> Point {
        e.place(&f, origin, &mut u64::MAX.clone())
    }

    #[test]
    fn floats_stack_side_by_side_and_move_down() {
        let mut e = Exclusions::default();
        assert_eq!(
            place(&mut e, float(Side::Left, 100.0, 50.0), 0.0),
            Point::new(0.0, 0.0)
        );
        assert_eq!(
            place(&mut e, float(Side::Left, 100.0, 20.0), 0.0),
            Point::new(100.0, 0.0)
        );
        assert_eq!(
            place(&mut e, float(Side::Right, 100.0, 30.0), 0.0),
            Point::new(200.0, 0.0)
        );
        // No room beside: below the shortest float on the line.
        assert_eq!(
            place(&mut e, float(Side::Left, 150.0, 10.0), 0.0),
            Point::new(100.0, 30.0)
        );
        // Rule 5: not above the last float.
        assert_eq!(
            place(&mut e, float(Side::Right, 10.0, 10.0), 0.0),
            Point::new(290.0, 30.0)
        );
        assert_eq!(e.clearance(Clear::Left), Some(50.0));
        assert_eq!(e.clearance(Clear::Right), Some(40.0));
        assert_eq!(e.clearance(Clear::Both), Some(50.0));
    }

    #[test]
    fn a_float_wider_than_the_containing_block_goes_below_the_floats() {
        let mut e = Exclusions::default();
        place(&mut e, float(Side::Left, 100.0, 50.0), 0.0);
        assert_eq!(
            place(&mut e, float(Side::Left, 400.0, 10.0), 0.0),
            Point::new(0.0, 50.0)
        );
    }

    #[test]
    fn opportunities_narrow_below() {
        let mut bfc = Bfc::new(u64::MAX);
        bfc.place_float(&float(Side::Left, 100.0, 50.0), 0.0);
        // A right float that starts lower.
        bfc.place_float(&float(Side::Right, 100.0, 50.0), 20.0);
        let o = bfc.opportunity_at(0.0, 0.0, 300.0).expect("free space");
        assert_eq!((o.left, o.right), (100.0, 300.0));
        assert!(!o.full_width && !o.last);
        // A 10 px line fits above the right float; a 30 px one does not.
        assert_eq!(bfc.narrowing_below(&o, 10.0, 0.0, 300.0, None), None);
        let at = bfc
            .narrowing_below(&o, 30.0, 0.0, 300.0, None)
            .expect("the right float narrows it");
        let narrower = bfc.narrower(&o, at, 0.0, 300.0).expect("free space");
        assert_eq!(
            (narrower.left, narrower.right, narrower.top),
            (100.0, 200.0, 0.0)
        );
        assert!(!narrower.full_width);
        let below = bfc.opportunity_at(70.0, 0.0, 300.0).expect("free space");
        assert!(below.full_width && below.last);
    }

    #[test]
    fn opportunities_move_down_to_where_floats_end() {
        let mut bfc = Bfc::new(u64::MAX);
        bfc.place_float(&float(Side::Left, 100.0, 50.0), 0.0);
        bfc.place_float(&float(Side::Right, 150.0, 80.0), 0.0);
        let o = bfc.opportunity_at(0.0, 0.0, 300.0).expect("free space");
        assert_eq!((o.left, o.right), (100.0, 150.0));
        assert_eq!(bfc.next_top(0.0), Some(50.0));
        let o = bfc.opportunity_at(50.0, 0.0, 300.0).expect("free space");
        assert_eq!((o.left, o.right), (0.0, 150.0));
        assert_eq!(bfc.next_top(50.0), Some(80.0));
        let o = bfc.opportunity_at(80.0, 0.0, 300.0).expect("free space");
        assert!(o.full_width && o.last);
        assert_eq!(bfc.next_top(80.0), None);
    }

    #[test]
    fn many_floats_stay_bounded() {
        let mut e = Exclusions::default();
        let mut budget = 1_000_000;
        for i in 0..(MAX_FLOATS + 100) {
            e.place(&float(Side::Left, 0.5, 0.25), i as f32 * 0.01, &mut budget);
        }
        assert!(e.segments.len() <= 2 * MAX_FLOATS + 202);
        // With no budget left, the only opportunity is below all floats.
        let mut bfc = Bfc::new(0);
        bfc.exclusions = e;
        let o = bfc.opportunity_at(0.0, 0.0, 300.0).expect("below all");
        assert!(o.full_width && o.last && o.top >= bfc.exclusions.bottom().unwrap_or(0.0));
        assert_eq!(bfc.next_top(0.0), None);
    }

    #[test]
    fn waiting_floats_are_capped() {
        let mut bfc = Bfc::new(u64::MAX);
        bfc.push_frame(0.0, CollapsedMargin::default(), None, None);
        for _ in 0..(MAX_FLOATS + 10) {
            bfc.add_pending(PendingFloat {
                path: vec![0],
                float: float(Side::Left, 1.0, 1.0),
                origin_x: 0.0,
            });
        }
        let floats = bfc.pop_frame();
        assert_eq!(floats.pending.len(), MAX_FLOATS);
    }

    #[test]
    fn chain_resolution_places_waiting_floats_at_the_resolved_position() {
        let mut bfc = Bfc::new(u64::MAX);
        bfc.push_frame(10.0, CollapsedMargin::default(), None, None);
        bfc.add_pending(PendingFloat {
            path: vec![0],
            float: float(Side::Left, 50.0, 20.0),
            origin_x: 0.0,
        });
        let y = bfc.resolve(CollapsedMargin::new(30.0));
        assert_eq!(y, 40.0);
        let floats = bfc.pop_frame();
        assert_eq!(floats.placed[0].position, Point::new(0.0, 0.0));
        assert_eq!(bfc.exclusions.clearance(Clear::Left), Some(60.0));
    }

    #[test]
    fn clearance_separates_the_margins_of_outer_containers() {
        let mut bfc = Bfc::new(u64::MAX);
        bfc.place_float(&float(Side::Left, 50.0, 30.0), 0.0);
        // A container with a 10 px margin, and inside it a child with
        // clear: left and a 5 px margin.
        bfc.push_frame(0.0, CollapsedMargin::default(), None, None);
        bfc.push_frame(0.0, CollapsedMargin::new(10.0), Some(30.0), None);
        let y = bfc.resolve(CollapsedMargin::new(10.0).adjoin(CollapsedMargin::new(5.0)));
        assert_eq!(y, 30.0);
        bfc.pop_frame();
        assert_eq!(bfc.resolved(), Some(10.0));
    }

    #[test]
    fn a_forced_position_ignores_the_margins_in_it() {
        let mut bfc = Bfc::new(u64::MAX);
        // A container that clears floats at 50, with a 10 px margin, and in
        // it a child with a 70 px margin.
        bfc.push_frame(0.0, CollapsedMargin::default(), Some(50.0), Some(50.0));
        bfc.push_frame(0.0, CollapsedMargin::new(10.0), None, None);
        let y = bfc.resolve(CollapsedMargin::new(10.0).adjoin(CollapsedMargin::new(70.0)));
        assert_eq!(y, 50.0);
        bfc.pop_frame();
        assert_eq!(bfc.resolved(), Some(50.0));
    }
}
