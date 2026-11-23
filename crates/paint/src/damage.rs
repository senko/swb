//! The damage of a frame (ADR 0044 section 18): the device rectangles in
//! which a raster of a new display list can differ from a raster of the
//! list that the last frame drew, found by comparing the two lists.
//!
//! [`list_damage`] walks both lists in paint order, with the transform and
//! the clip of the open groups (one state: the walk only goes into groups
//! that are equal on both sides, so both sides are in the same state):
//!
//! - equal items add nothing (shared values compare by pointer first); an
//!   `Image`, a `Frame` and a mask with images also compare their media
//!   keys ([`media_keys`]: the decoded image and its content generation,
//!   the generation of a frame's document);
//! - two different groups of one kind add the bounds of both whole groups
//!   (the bounds that `group_bounds` stored) and the walk goes on after
//!   them; an opacity group whose opacity and blend mode are equal, and a
//!   transform group whose transform is an equal translation, are entered
//!   instead (their bounds only follow their content);
//! - other differences (different drawing items, a group that starts on
//!   one side only, a different clip, inserted items): a lookahead of at
//!   most `RESYNC` items per side finds the place where the lists meet
//!   again (two equal items in a row, only whole groups and clips skipped),
//!   and the skipped items are damage (a replaced item is the case of one
//!   step on each side); without such a place, the longest common suffix of
//!   the enclosing group is kept, cut to a place at the depth of the
//!   difference, and every item between is damage on both sides (whole
//!   groups for the groups that start there);
//! - inside a filter group the content is not clipped by the enclosing
//!   clips, and its damage grows by the filter's reach when the group ends.
//!
//! Every step goes forward over the items or skips whole groups (`ends`),
//! the suffix of a group is scanned once and the lookaheads have a budget
//! per diff (`RESYNC_BUDGET`), so the walk is linear in the lengths of
//! the lists; it keeps its own stack (no recursion).
//!
//! [`merge_damage`] rounds the rectangles out to whole pixels plus 1 px,
//! merges them into at most [`MAX_RECTS`] (a grid first when there are
//! many), makes each rectangle that touches a `backdrop-filter` box hold the
//! whole box (the filter reads the whole box: [`backdrop_boxes`]), and gives
//! a full raster when the damage covers more than half of the target.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use swb_layout::{Matrix, Point, Rect, StickyCache};

use crate::display_list::{DisplayItem, DisplayList, ImageRef};
use crate::filter;
use crate::mask::MaskLayerImage;
use crate::raster::{ImageSource, PixelRect, RasterParams};

/// The most rectangles of damage that a frame rasterizes.
pub const MAX_RECTS: usize = 8;

/// The most rectangles before the merge clusters them on a grid of
/// `GRID` x `GRID` cells.
const MAX_LOOSE_RECTS: usize = 64;

/// The cells per side of the grid of the first merge.
const GRID: u32 = 8;

/// How far the diff looks ahead on each side for the place where the lists
/// meet again after different items.
const RESYNC: usize = 64;

/// The most places that the lookaheads of one diff test ([`RESYNC`]): a list
/// that differs at every item falls back to the common suffix, so the diff
/// stays linear.
const RESYNC_BUDGET: usize = 200_000;

/// The most rounds of the merge with `backdrop-filter` boxes before the
/// damage becomes a full raster.
const MAX_BACKDROP_ROUNDS: usize = 8;

/// The second half of the media key of a `Frame` item whose document is
/// drawn: [`FRAME_READS_PIXELS`] if its list reads the pixels around it.
const FRAME_DRAWN: u64 = 1;

/// The second half of the media key of a `Frame` item whose list has a
/// `backdrop-filter` or a frame: a part that touches it holds all of it.
const FRAME_READS_PIXELS: u64 = 2;

/// The media keys of a display list: for each `Image`, `Frame` and
/// `PushMask` item (by index, in order), what its pixels depend on beyond
/// the item: the decoded image and its content generation, the generation
/// of a frame's document, the images of a mask.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaKeys(Vec<(usize, [u64; 2])>);

impl MediaKeys {
    /// The key of the item at `index`, if it has one.
    fn get(&self, index: usize) -> Option<[u64; 2]> {
        let at = self.0.partition_point(|(i, _)| *i < index);
        self.0.get(at).filter(|(i, _)| *i == index).map(|(_, k)| *k)
    }
}

/// The media keys of `list` with the images and frames of `images`.
pub fn media_keys(list: &DisplayList, images: &dyn ImageSource) -> MediaKeys {
    let keys = list
        .items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| item_media_key(item, images).map(|key| (i, key)))
        .collect();
    MediaKeys(keys)
}

/// The media key of `item` (images, frames and masks with images), if it
/// has one.
pub(crate) fn item_media_key(item: &DisplayItem, images: &dyn ImageSource) -> Option<[u64; 2]> {
    match item {
        DisplayItem::Image { image, .. } => Some(image_key(image, images)),
        DisplayItem::Frame { node, .. } => Some(images.frame(*node).map_or([0, 0], |f| {
            [
                f.generation,
                if has_readers(f.list) {
                    FRAME_READS_PIXELS
                } else {
                    FRAME_DRAWN
                },
            ]
        })),
        DisplayItem::PushMask { layers, .. } => {
            let mut hasher = DefaultHasher::new();
            let mut any = false;
            for layer in layers.iter() {
                if let MaskLayerImage::Image { image, .. } = &layer.image {
                    image_key(image, images).hash(&mut hasher);
                    any = true;
                }
            }
            any.then(|| [hasher.finish(), 0])
        }
        _ => None,
    }
}

/// The media key of an image: the decoded image (0 if it is not loaded)
/// and its content generation.
fn image_key(image: &ImageRef, images: &dyn ImageSource) -> [u64; 2] {
    let id = images.image(image).map_or(0, crate::DecodedImage::id);
    [id, images.generation(image)]
}

/// True if a frame's list has items that read pixels around them: a
/// `backdrop-filter` or a frame (whose list can have one).
fn has_readers(list: &DisplayList) -> bool {
    list.items.iter().any(|item| {
        matches!(
            item,
            DisplayItem::BackdropFilter { .. } | DisplayItem::Frame { .. }
        )
    })
}

/// A display list with its media keys.
#[derive(Clone, Copy)]
pub struct Side<'a> {
    /// The list.
    pub list: &'a DisplayList,
    /// Its media keys ([`media_keys`]).
    pub keys: &'a MediaKeys,
}

/// The damage of a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Damage {
    /// Nothing changed.
    None,
    /// These rectangles changed (at most [`MAX_RECTS`], inside the target,
    /// not overlapping).
    Rects(Vec<PixelRect>),
    /// Rasterize everything.
    Full,
}

/// The damage found by [`list_damage`], before [`merge_damage`].
#[derive(Clone, Debug, Default)]
pub struct ListDamage {
    /// Device rectangles (target device px, not rounded).
    pub rects: Vec<Rect>,
    /// True if the lists cannot be compared (a group without its end): the
    /// frame needs a full raster.
    pub full: bool,
}

/// The state of the walk at a place in a list.
#[derive(Clone, Copy, Debug)]
struct State {
    /// From the current coordinates to the list's coordinates.
    list: Matrix,
    /// The clip in device px; `None` for none.
    clip: Option<Rect>,
    /// True inside a transform that is not a translation: its layer is
    /// resampled, which spreads a change by up to one more device pixel.
    resampled: bool,
}

/// What the walk needs besides the state.
struct Walk<'a> {
    /// From list coordinates to device px.
    base: Matrix,
    viewport_scroll: Point,
    sticky: StickyCache,
    out: &'a mut Vec<Rect>,
}

/// A group that the walk went into.
struct Open {
    /// The state before the group.
    before: State,
    /// The ends of the interior of the enclosing group (old, new).
    outer_ends: (usize, usize),
    /// For a filter group: where its damage starts in `out`, and its reach
    /// in device px.
    filter: Option<(usize, f32)>,
}

/// The device rectangles in which a raster of `new` can differ from a raster
/// of `old`, both drawn with `params` (see the module documentation).
pub fn list_damage(old: Side<'_>, new: Side<'_>, params: RasterParams) -> ListDamage {
    let mut out = Vec::new();
    let (Some(old_ends), Some(new_ends)) =
        (group_ends(&old.list.items), group_ends(&new.list.items))
    else {
        return ListDamage {
            rects: out,
            full: true,
        };
    };
    let s = params.scale;
    let mut walk = Walk {
        base: Matrix::new(s, 0.0, 0.0, s, -params.scroll.x * s, -params.scroll.y * s),
        viewport_scroll: params.viewport_scroll,
        sticky: StickyCache::default(),
        out: &mut out,
    };
    let lists = Lists {
        old,
        new,
        old_ends: &old_ends,
        new_ends: &new_ends,
        resync_budget: std::cell::Cell::new(RESYNC_BUDGET),
    };
    lists.walk(&mut walk);
    ListDamage {
        rects: out,
        full: false,
    }
}

/// The two lists and their group ends.
struct Lists<'a> {
    old: Side<'a>,
    new: Side<'a>,
    old_ends: &'a [usize],
    new_ends: &'a [usize],
    /// What is left of [`RESYNC_BUDGET`].
    resync_budget: std::cell::Cell<usize>,
}

impl Lists<'_> {
    fn old_item(&self, i: usize) -> Option<&DisplayItem> {
        self.old.list.items.get(i)
    }

    fn new_item(&self, j: usize) -> Option<&DisplayItem> {
        self.new.list.items.get(j)
    }

    /// True if old item `i` and new item `j` are equal with equal media
    /// keys.
    fn same(&self, i: usize, j: usize) -> bool {
        match (self.old_item(i), self.new_item(j)) {
            (Some(a), Some(b)) => a == b && self.same_keys(i, j, a),
            _ => false,
        }
    }

    /// True if old item `i` and new item `j` (equal items) have equal
    /// media keys.
    fn same_keys(&self, i: usize, j: usize, item: &DisplayItem) -> bool {
        match item {
            DisplayItem::Image { .. }
            | DisplayItem::Frame { .. }
            | DisplayItem::PushMask { .. } => self.old.keys.get(i) == self.new.keys.get(j),
            _ => true,
        }
    }

    /// The synchronized walk.
    fn walk(&self, walk: &mut Walk<'_>) {
        let (mut i, mut j) = (0, 0);
        let (mut oe, mut ne) = (self.old.list.items.len(), self.new.list.items.len());
        let mut state = State {
            list: Matrix::IDENTITY,
            clip: None,
            resampled: false,
        };
        let mut stack: Vec<Open> = Vec::new();
        loop {
            while i < oe && self.old_item(i).is_some_and(is_hit) {
                i += 1;
            }
            while j < ne && self.new_item(j).is_some_and(is_hit) {
                j += 1;
            }
            if i >= oe || j >= ne {
                // The end of the interior of a group: what is left on one
                // side is damage.
                self.span(walk, Which::Old, i..oe, state);
                self.span(walk, Which::New, j..ne, state);
                let Some(open) = stack.pop() else {
                    break;
                };
                if let Some((start, reach)) = open.filter {
                    grow_filter_damage(walk.out, start, reach, open.before.clip);
                }
                state = open.before;
                (i, j) = (oe + 1, ne + 1);
                (oe, ne) = open.outer_ends;
                continue;
            }
            let (Some(a), Some(b)) = (self.old_item(i), self.new_item(j)) else {
                break;
            };
            if a == b {
                if !self.same_keys(i, j, a) {
                    // Equal items with new content.
                    if is_push(a) {
                        walk.group(a, state);
                        walk.group(b, state);
                        (i, j) = (self.old_end(i) + 1, self.new_end(j) + 1);
                    } else {
                        walk.item(a, state);
                        walk.item(b, state);
                        (i, j) = (i + 1, j + 1);
                    }
                    continue;
                }
                if opens(a) {
                    let next = walk.enter(a, state);
                    stack.push(Open {
                        before: state,
                        outer_ends: (oe, ne),
                        filter: filter_reach(a, &state, walk),
                    });
                    state = next;
                    (oe, ne) = (self.old_end(i), self.new_end(j));
                    (i, j) = (i + 1, j + 1);
                } else {
                    (i, j) = (i + 1, j + 1);
                }
                continue;
            }
            if is_push(a) && same_kind(a, b) {
                if enterable(a, b) {
                    let next = walk.enter(b, state);
                    stack.push(Open {
                        before: state,
                        outer_ends: (oe, ne),
                        filter: None,
                    });
                    state = next;
                    (oe, ne) = (self.old_end(i), self.new_end(j));
                } else {
                    walk.group(a, state);
                    walk.group(b, state);
                    // The walk goes on after the two groups.
                    (i, j) = (self.old_end(i), self.new_end(j));
                }
                (i, j) = (i + 1, j + 1);
                continue;
            }
            // Different items: replaced, or items or groups were added or
            // removed here. Where the lists meet again soon, the items up to
            // there are damage; else the common suffix decides.
            if let Some((da, db)) = self.resync((i, j), (oe, ne)) {
                self.span(walk, Which::Old, i..i + da, state);
                self.span(walk, Which::New, j..j + db, state);
                (i, j) = (i + da, j + db);
                continue;
            }
            (i, j) = self.mismatch(walk, (i, j), (oe, ne), state);
        }
    }

    /// Where the lists meet again after old item `i` and new item `j`
    /// differ: the smallest steps `(da, db)` (at most [`RESYNC`] each, the
    /// smallest sum first) after which two items in a row are equal, or the
    /// interiors (which end at `oe` and `ne`) end together, and the skipped
    /// items on each side hold whole groups and clips (they start and end at
    /// the depth of `i`). `None` if there is none, or when the lookahead
    /// budget of the diff is used up: the caller falls back to the common
    /// suffix.
    fn resync(&self, (i, j): (usize, usize), (oe, ne): (usize, usize)) -> Option<(usize, usize)> {
        let budget = self.resync_budget.get();
        if budget == 0 {
            return None;
        }
        let old_ok = self.balanced(Which::Old, i, oe);
        let new_ok = self.balanced(Which::New, j, ne);
        let mut spent = 0;
        let mut found = None;
        'search: for total in 1..=2 * RESYNC {
            for da in total.saturating_sub(RESYNC)..=total.min(RESYNC) {
                let db = total - da;
                let (x, y) = (i + da, j + db);
                if x > oe || y > ne {
                    continue;
                }
                if !(old_ok.get(da).copied().unwrap_or(false)
                    && new_ok.get(db).copied().unwrap_or(false))
                {
                    continue;
                }
                spent += 1;
                let meet = (x == oe && y == ne)
                    || (self.same(x, y)
                        && ((x + 1 == oe && y + 1 == ne) || self.same(x + 1, y + 1)));
                if meet {
                    found = Some((da, db));
                    break 'search;
                }
            }
        }
        self.resync_budget.set(budget.saturating_sub(spent));
        found
    }

    /// For each `d` in `0..=RESYNC`: true if the items `from..from + d` of
    /// one list (inside an interior that ends at `end`) hold whole groups
    /// and clips: the depth never goes below the depth of `from` and ends
    /// there.
    fn balanced(&self, which: Which, from: usize, end: usize) -> Vec<bool> {
        let mut ok = Vec::with_capacity(RESYNC + 1);
        ok.push(true);
        let mut depth: i64 = 0;
        let mut broken = false;
        for q in from..(from + RESYNC).min(end) {
            let item = match which {
                Which::Old => self.old_item(q),
                Which::New => self.new_item(q),
            };
            match item {
                Some(item) if opens(item) => depth += 1,
                Some(item) if is_pop(item) => depth -= 1,
                _ => {}
            }
            broken |= depth < 0;
            ok.push(!broken && depth == 0);
        }
        ok
    }

    /// The end of the group that old item `i` starts (`i` if none).
    fn old_end(&self, i: usize) -> usize {
        self.old_ends.get(i).copied().unwrap_or(i).max(i)
    }

    /// The end of the group that new item `j` starts (`j` if none).
    fn new_end(&self, j: usize) -> usize {
        self.new_ends.get(j).copied().unwrap_or(j).max(j)
    }

    /// Where the kinds of old item `i` and new item `j` differ, inside a
    /// group whose interior ends at `oe` and `ne`: the damage up to the
    /// common suffix (cut to the depth of `i` and `j`), and the places where
    /// the walk goes on.
    fn mismatch(
        &self,
        walk: &mut Walk<'_>,
        (i, j): (usize, usize),
        (oe, ne): (usize, usize),
        state: State,
    ) -> (usize, usize) {
        let mut k = 0;
        while i + k < oe && j + k < ne && self.same(oe - 1 - k, ne - 1 - k) {
            k += 1;
        }
        // The first place of the suffix at depth 0 (the depth of `i`): the
        // depth before each item, counted back from the end of the
        // interior, which is at depth 0.
        let mut depth: i64 = 0;
        let mut start = oe;
        for q in (oe - k..oe).rev() {
            if let Some(item) = self.old_item(q) {
                if is_pop(item) {
                    depth += 1;
                } else if opens(item) {
                    depth -= 1;
                }
            }
            if depth == 0 {
                start = q;
            }
        }
        let new_start = ne - (oe - start);
        self.span(walk, Which::Old, i..start, state);
        self.span(walk, Which::New, j..new_start, state);
        (start, new_start)
    }

    /// Adds the damage of the items `range` of one list, which start at
    /// the depth of `state`: drawing items and whole groups; the walk goes
    /// into clips.
    fn span(&self, walk: &mut Walk<'_>, which: Which, range: std::ops::Range<usize>, state: State) {
        let (items, ends) = match which {
            Which::Old => (&self.old.list.items, self.old_ends),
            Which::New => (&self.new.list.items, self.new_ends),
        };
        let mut clips: Vec<State> = Vec::new();
        let mut state = state;
        let mut q = range.start;
        while q < range.end {
            let Some(item) = items.get(q) else {
                break;
            };
            match item {
                DisplayItem::PushClip(_) | DisplayItem::PushViewportClip => {
                    clips.push(state);
                    state = walk.enter(item, state);
                }
                DisplayItem::PopClip => {
                    if let Some(before) = clips.pop() {
                        state = before;
                    }
                }
                item if is_push(item) => {
                    walk.group(item, state);
                    q = ends.get(q).copied().unwrap_or(q).max(q);
                }
                item => walk.item(item, state),
            }
            q += 1;
        }
    }
}

/// One of the two lists.
#[derive(Clone, Copy)]
enum Which {
    Old,
    New,
}

impl Walk<'_> {
    /// The matrix from the coordinates of `state` to device px.
    fn device(&self, state: &State) -> Matrix {
        self.base.multiply(&state.list)
    }

    /// Adds `rect` (device px), clipped by `clip`.
    fn add(&mut self, rect: Rect, clip: Option<Rect>, resampled: bool) {
        let rect = if resampled {
            Rect::new(
                rect.x - 1.0,
                rect.y - 1.0,
                rect.width + 2.0,
                rect.height + 2.0,
            )
        } else {
            rect
        };
        let rect = match clip {
            Some(c) => match rect.intersection(&outset(c, 1.0)) {
                Some(r) => r,
                None => return,
            },
            None => rect,
        };
        self.out.push(rect);
    }

    /// Adds the area that the drawing item `item` draws in.
    fn item(&mut self, item: &DisplayItem, state: State) {
        if let Some(bounds) = item.bounds() {
            let device = self.device(&state).map_rect(&bounds);
            self.add(device, state.clip, state.resampled);
        }
    }

    /// Adds the area of the whole group that `item` starts.
    fn group(&mut self, item: &DisplayItem, state: State) {
        let device = self.device(&state);
        // Content fixed to the viewport, at the viewport scroll offset; the
        // enclosing clips do not clip it.
        let fixed = |walk: &mut Walk<'_>, f: Option<Rect>, grow: &dyn Fn(Rect) -> Rect| {
            if let Some(f) = f {
                let at = walk.base.map_rect(&grow(f.translate(walk.viewport_scroll)));
                walk.add(at, None, state.resampled);
            }
        };
        match item {
            DisplayItem::PushTransform {
                transform,
                bounds,
                fixed_bounds,
                ..
            } => {
                let m = transform.resolve_with(&state.list, self.viewport_scroll, &mut self.sticky);
                let resampled = state.resampled || !m.is_translation();
                let rect = self.base.multiply(&m).map_rect(bounds);
                if fixed_bounds.is_some() {
                    self.add(rect, None, resampled);
                } else {
                    self.add(rect, state.clip, resampled);
                }
                fixed(self, *fixed_bounds, &|r| r);
            }
            DisplayItem::PushOpacity {
                bounds,
                fixed_bounds,
                escapes_clips,
                ..
            } => {
                let clip = if *escapes_clips { None } else { state.clip };
                self.add(device.map_rect(bounds), clip, state.resampled);
                fixed(self, *fixed_bounds, &|r| r);
            }
            DisplayItem::PushFilter {
                filter,
                bounds,
                fixed_bounds,
                escapes_clips,
                ..
            } => {
                let clip = if *escapes_clips { None } else { state.clip };
                let grown = filter::css_output(*bounds, filter);
                self.add(device.map_rect(&grown), clip, state.resampled);
                fixed(self, *fixed_bounds, &|r| filter::css_output(r, filter));
            }
            DisplayItem::PushMask { bounds, .. } | DisplayItem::PushSvgClip { bounds, .. } => {
                self.add(device.map_rect(bounds), state.clip, state.resampled);
            }
            _ => {}
        }
    }

    /// The state inside the group or clip that `item` starts.
    fn enter(&mut self, item: &DisplayItem, state: State) -> State {
        match item {
            DisplayItem::PushTransform { transform, .. } => {
                let m = transform.resolve_with(&state.list, self.viewport_scroll, &mut self.sticky);
                State {
                    list: m,
                    clip: state.clip,
                    resampled: state.resampled || !m.is_translation(),
                }
            }
            DisplayItem::PushClip(rect) => {
                let device = round_out(self.device(&state).map_rect(rect));
                let clip = match state.clip {
                    Some(c) => c
                        .intersection(&device)
                        .unwrap_or(Rect::new(c.x, c.y, 0.0, 0.0)),
                    None => device,
                };
                State {
                    clip: Some(clip),
                    ..state
                }
            }
            // A viewport clip replaces the enclosing clips; the content of a
            // filter is drawn without them (they clip the result).
            DisplayItem::PushViewportClip | DisplayItem::PushFilter { .. } => State {
                clip: None,
                ..state
            },
            _ => state,
        }
    }
}

/// The reach of the filter group that `item` starts (device px) and where
/// its damage starts, or `None` if it is no filter group.
fn filter_reach(item: &DisplayItem, state: &State, walk: &Walk<'_>) -> Option<(usize, f32)> {
    let DisplayItem::PushFilter { filter, .. } = item else {
        return None;
    };
    let reach = css_reach(filter) * walk.device(state).max_scale();
    Some((walk.out.len(), reach))
}

/// How far the `filter` functions move ink, in CSS px of the group.
fn css_reach(list: &[swb_style::FilterFunction]) -> f32 {
    let out = filter::css_output(Rect::default(), list);
    (-out.x)
        .max(-out.y)
        .max(out.right())
        .max(out.bottom())
        .max(0.0)
}

/// Grows the damage `out[start..]` of the content of a filter group by its
/// reach `reach` (device px) and clips it by `clip`, the clip around the
/// group.
fn grow_filter_damage(out: &mut Vec<Rect>, start: usize, reach: f32, clip: Option<Rect>) {
    let grown: Vec<Rect> = out
        .get(start..)
        .unwrap_or(&[])
        .iter()
        .map(|r| outset(*r, reach))
        .filter_map(|r| match clip {
            Some(c) => r.intersection(&outset(c, 1.0)),
            None => Some(r),
        })
        .collect();
    out.truncate(start);
    out.extend(grown);
}

/// `r` grown by `by` on every side.
fn outset(r: Rect, by: f32) -> Rect {
    Rect::new(r.x - by, r.y - by, r.width + 2.0 * by, r.height + 2.0 * by)
}

/// The smallest rectangle of whole pixels that contains `r`.
fn round_out(r: Rect) -> Rect {
    let x = r.x.floor();
    let y = r.y.floor();
    Rect::new(x, y, r.right().ceil() - x, r.bottom().ceil() - y)
}

/// True for items that hit testing uses and that draw nothing.
fn is_hit(item: &DisplayItem) -> bool {
    matches!(
        item,
        DisplayItem::HitRegion { .. } | DisplayItem::HitShape { .. }
    )
}

/// True for the items that start a group with bounds.
fn is_push(item: &DisplayItem) -> bool {
    matches!(
        item,
        DisplayItem::PushTransform { .. }
            | DisplayItem::PushOpacity { .. }
            | DisplayItem::PushFilter { .. }
            | DisplayItem::PushMask { .. }
            | DisplayItem::PushSvgClip { .. }
    )
}

/// True for the items that start a group or a clip.
fn opens(item: &DisplayItem) -> bool {
    is_push(item)
        || matches!(
            item,
            DisplayItem::PushClip(_) | DisplayItem::PushViewportClip
        )
}

/// True for the items that end a group or a clip.
fn is_pop(item: &DisplayItem) -> bool {
    matches!(
        item,
        DisplayItem::PopTransform
            | DisplayItem::PopOpacity
            | DisplayItem::PopFilter
            | DisplayItem::PopMask
            | DisplayItem::PopSvgClip
            | DisplayItem::PopClip
    )
}

/// True if two group items are of one kind.
fn same_kind(a: &DisplayItem, b: &DisplayItem) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

/// True if two different groups of one kind draw their content in the same
/// way, so that only their content can differ: transform groups with an
/// equal translation (a fixed or sticky box, or a translation matrix).
/// Their bounds follow their content and only cull it. Opacity groups are
/// never entered when they differ: their layer has the extent of their
/// bounds, and the anti-aliasing of equal content changes with that extent.
fn enterable(a: &DisplayItem, b: &DisplayItem) -> bool {
    match (a, b) {
        (
            DisplayItem::PushTransform { transform, .. },
            DisplayItem::PushTransform { transform: t2, .. },
        ) => {
            transform == t2
                && match transform {
                    swb_layout::GroupTransform::Matrix(m) => m.is_translation(),
                    swb_layout::GroupTransform::Fixed | swb_layout::GroupTransform::Sticky(_) => {
                        true
                    }
                }
        }
        _ => false,
    }
}

/// The index of the matching end of each group and clip of `items` (0 for
/// other items), or `None` if the groups do not nest (an end without its
/// start, of another kind, or a start without its end).
pub(crate) fn group_ends(items: &[DisplayItem]) -> Option<Vec<usize>> {
    let mut ends = vec![0; items.len()];
    let mut open: Vec<usize> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if opens(item) {
            open.push(i);
        } else if is_pop(item) {
            let start = open.pop()?;
            if !closes(items.get(start)?, item) {
                return None;
            }
            *ends.get_mut(start)? = i;
        }
    }
    open.is_empty().then_some(ends)
}

/// True if `end` ends the group or clip that `start` starts.
fn closes(start: &DisplayItem, end: &DisplayItem) -> bool {
    matches!(
        (start, end),
        (DisplayItem::PushTransform { .. }, DisplayItem::PopTransform)
            | (DisplayItem::PushOpacity { .. }, DisplayItem::PopOpacity)
            | (DisplayItem::PushFilter { .. }, DisplayItem::PopFilter)
            | (DisplayItem::PushMask { .. }, DisplayItem::PopMask)
            | (DisplayItem::PushSvgClip { .. }, DisplayItem::PopSvgClip)
            | (
                DisplayItem::PushClip(_) | DisplayItem::PushViewportClip,
                DisplayItem::PopClip
            )
    )
}

/// The boxes of the `backdrop-filter` items of `list` in device px (with
/// `params`), and of the frames whose lists can have them: a rectangle of
/// damage that touches one must hold all of it, because the filter reads
/// the whole box. Inside a filter group a box grows by the filter's reach
/// (the filter's content is drawn where it reads), inside a resampled
/// transform by 4 px.
pub fn backdrop_boxes(list: &DisplayList, keys: &MediaKeys, params: RasterParams) -> Vec<Rect> {
    let mut boxes = Vec::new();
    if !list.items.iter().any(|item| {
        matches!(
            item,
            DisplayItem::BackdropFilter { .. } | DisplayItem::Frame { .. }
        )
    }) {
        return boxes;
    }
    let s = params.scale;
    let base = Matrix::new(s, 0.0, 0.0, s, -params.scroll.x * s, -params.scroll.y * s);
    let mut sticky = StickyCache::default();
    // The list matrix, the growth, and whether the group is a transform.
    let mut stack: Vec<(Matrix, f32)> = Vec::new();
    let mut matrix = Matrix::IDENTITY;
    let mut grow = 0.0_f32;
    for (i, item) in list.items.iter().enumerate() {
        match item {
            DisplayItem::PushTransform { transform, .. } => {
                stack.push((matrix, grow));
                matrix = transform.resolve_with(&matrix, params.viewport_scroll, &mut sticky);
                if !matrix.is_translation() {
                    grow = grow.max(4.0);
                }
            }
            DisplayItem::PushFilter { filter, .. } => {
                stack.push((matrix, grow));
                grow += css_reach(filter) * base.multiply(&matrix).max_scale();
            }
            DisplayItem::PushOpacity { .. }
            | DisplayItem::PushMask { .. }
            | DisplayItem::PushSvgClip { .. } => stack.push((matrix, grow)),
            DisplayItem::PopTransform
            | DisplayItem::PopFilter
            | DisplayItem::PopOpacity
            | DisplayItem::PopMask
            | DisplayItem::PopSvgClip => {
                if let Some((m, g)) = stack.pop() {
                    matrix = m;
                    grow = g;
                }
            }
            DisplayItem::BackdropFilter { rect, .. } => {
                boxes.push(outset(base.multiply(&matrix).map_rect(rect), grow));
            }
            DisplayItem::Frame { rect, .. }
                if keys.get(i).is_some_and(|k| k[1] == FRAME_READS_PIXELS) =>
            {
                boxes.push(outset(base.multiply(&matrix).map_rect(rect), grow));
            }
            _ => {}
        }
    }
    boxes
}

/// The damage of a target of `size` device px from the device rectangles
/// `rects` (see the module documentation); `boxes` from
/// [`backdrop_boxes`].
pub fn merge_damage(rects: &[Rect], boxes: &[Rect], size: (u32, u32)) -> Damage {
    let (width, height) = size;
    let mut merged = Vec::with_capacity(rects.len().min(MAX_LOOSE_RECTS));
    for r in rects {
        let Some(p) = pixel_rect(*r, 1.0, size) else {
            if !(r.x.is_finite() && r.y.is_finite() && r.width.is_finite() && r.height.is_finite())
            {
                return Damage::Full;
            }
            continue;
        };
        merged.push(p);
    }
    if merged.is_empty() {
        return Damage::None;
    }
    if merged.len() > MAX_LOOSE_RECTS {
        merged = on_grid(&merged, size);
    }
    merge_pairs(&mut merged);
    let boxes: Vec<PixelRect> = boxes
        .iter()
        .filter_map(|b| pixel_rect(*b, 1.0, size))
        .collect();
    let mut rounds = 0;
    while hold_boxes(&mut merged, &boxes) {
        rounds += 1;
        if rounds > MAX_BACKDROP_ROUNDS {
            return Damage::Full;
        }
        merge_pairs(&mut merged);
    }
    let area: u64 = merged.iter().map(PixelRect::area).sum();
    if area.saturating_mul(2) > u64::from(width) * u64::from(height) {
        return Damage::Full;
    }
    Damage::Rects(merged)
}

/// `rects` (inside a target of `size`) made disjoint (overlapping ones
/// become their bounding box) and grown so that each one that touches one
/// of `boxes` (from [`backdrop_boxes`]) holds all of it. For a raster of
/// parts that are not the damage of a frame (the oracle of the region
/// raster); after [`MAX_BACKDROP_ROUNDS`] rounds of growth, the whole
/// target.
pub fn hold_backdrop_boxes(
    rects: &[PixelRect],
    boxes: &[Rect],
    size: (u32, u32),
) -> Vec<PixelRect> {
    let boxes: Vec<PixelRect> = boxes
        .iter()
        .filter_map(|b| pixel_rect(*b, 1.0, size))
        .collect();
    let mut rects: Vec<PixelRect> = rects
        .iter()
        .map(|r| r.clamp_to(size.0, size.1))
        .filter(|r| !r.is_empty())
        .collect();
    if rects.len() > MAX_LOOSE_RECTS {
        rects = on_grid(&rects, size);
    }
    for _ in 0..MAX_BACKDROP_ROUNDS {
        merge_overlaps(&mut rects);
        if !hold_boxes(&mut rects, &boxes) {
            return rects;
        }
    }
    vec![PixelRect::from_edges(0, 0, size.0, size.1)]
}

/// Merges overlapping rectangles into their bounding box until none
/// overlap.
fn merge_overlaps(rects: &mut Vec<PixelRect>) {
    let mut merged = true;
    while merged {
        merged = false;
        'pairs: for a in 0..rects.len() {
            for b in a + 1..rects.len() {
                let (Some(&ra), Some(&rb)) = (rects.get(a), rects.get(b)) else {
                    continue;
                };
                if overlaps(ra, rb) {
                    rects.swap_remove(b);
                    if let Some(r) = rects.get_mut(a) {
                        *r = union(ra, rb);
                    }
                    merged = true;
                    break 'pairs;
                }
            }
        }
    }
}

/// `r` (device px) rounded out to whole pixels and grown by `margin` px,
/// inside the target; `None` if nothing is left or `r` is not finite.
fn pixel_rect(r: Rect, margin: f32, (width, height): (u32, u32)) -> Option<PixelRect> {
    let finite = [r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite());
    if !finite || r.width < 0.0 || r.height < 0.0 {
        return None;
    }
    let clamp = |v: f32, max: u32| v.clamp(0.0, max as f32) as u32;
    let p = PixelRect::from_edges(
        clamp((r.x - margin).floor(), width),
        clamp((r.y - margin).floor(), height),
        clamp((r.right() + margin).ceil(), width),
        clamp((r.bottom() + margin).ceil(), height),
    );
    (!p.is_empty()).then_some(p)
}

/// The rectangles merged by grid cell: each goes to the cell of its center.
fn on_grid(rects: &[PixelRect], (width, height): (u32, u32)) -> Vec<PixelRect> {
    let cell_w = width.div_ceil(GRID).max(1);
    let cell_h = height.div_ceil(GRID).max(1);
    let mut cells: Vec<Option<PixelRect>> = vec![None; (GRID * GRID) as usize];
    for r in rects {
        let cx = ((r.x + r.width / 2) / cell_w).min(GRID - 1);
        let cy = ((r.y + r.height / 2) / cell_h).min(GRID - 1);
        if let Some(cell) = cells.get_mut((cy * GRID + cx) as usize) {
            *cell = Some(cell.map_or(*r, |c| union(c, *r)));
        }
    }
    cells.into_iter().flatten().collect()
}

/// Merges rectangles until none overlap and there are at most
/// [`MAX_RECTS`]: each step merges the pair whose bounding box adds the
/// least area (overlapping pairs first). At most [`MAX_LOOSE_RECTS`]
/// rectangles come in, so the steps are bounded.
fn merge_pairs(rects: &mut Vec<PixelRect>) {
    loop {
        let mut best: Option<(bool, i64, usize, usize)> = None;
        for a in 0..rects.len() {
            for b in a + 1..rects.len() {
                let (Some(&ra), Some(&rb)) = (rects.get(a), rects.get(b)) else {
                    continue;
                };
                let overlap = overlaps(ra, rb);
                let cost = union(ra, rb).area() as i64 - ra.area() as i64 - rb.area() as i64;
                let better = match best {
                    None => true,
                    Some((o, c, _, _)) => (overlap && !o) || (overlap == o && cost < c),
                };
                if better {
                    best = Some((overlap, cost, a, b));
                }
            }
        }
        let Some((overlap, _, a, b)) = best else {
            return;
        };
        if !overlap && rects.len() <= MAX_RECTS {
            return;
        }
        let rb = rects.swap_remove(b);
        if let Some(ra) = rects.get_mut(a) {
            *ra = union(*ra, rb);
        }
    }
}

/// Makes each rectangle that touches a box hold the whole box. True if a
/// rectangle grew.
fn hold_boxes(rects: &mut [PixelRect], boxes: &[PixelRect]) -> bool {
    let mut grew = false;
    for b in boxes {
        for r in rects.iter_mut() {
            if overlaps(*r, *b) && !contains(*r, *b) {
                *r = union(*r, *b);
                grew = true;
            }
        }
    }
    grew
}

fn overlaps(a: PixelRect, b: PixelRect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

fn contains(outer: PixelRect, inner: PixelRect) -> bool {
    outer.x <= inner.x
        && outer.y <= inner.y
        && outer.right() >= inner.right()
        && outer.bottom() >= inner.bottom()
}

fn union(a: PixelRect, b: PixelRect) -> PixelRect {
    PixelRect::from_edges(
        a.x.min(b.x),
        a.y.min(b.y),
        a.right().max(b.right()),
        a.bottom().max(b.bottom()),
    )
}

#[cfg(test)]
mod random_pairs;
#[cfg(test)]
mod tests;
