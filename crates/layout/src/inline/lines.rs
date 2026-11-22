//! The line builder's steps (CSS Inline 3 §2 "line box"; CSS 2 §9.4.2,
//! §10.8): the fit of lines against the floats of a block formatting
//! context, the retries, the placement of text, atomic inlines, line breaks
//! and out-of-flow boxes, and the closing of each line. The data of the
//! passes, the groups and the line state are in `mod.rs`.

use super::{
    Arc, BoxContent, BoxEdges, BoxFragment, ComputedStyle, Cut, Display, EPSILON, FIT_TOLERANCE,
    Fill, FloatAt, FloatSite, Flow, FlowY, Fragment, Group, GroupText, InlineBoxEdges, InlineItem,
    LINE_RETRY_COST, Line, LineBuilder, LineEdge, LineFloat, LineMetrics, LineStart, LineState,
    MAX_LINE_ATTEMPTS, OpenBox, Opportunity, PendingFloat, PendingMarker, Piece, Point, Purpose,
    Rect, Reject, Run, StaticParent, StaticRule, Strut, TextAlign, TriedLine, VerticalAlign,
    apply_relative_position, baseline_shift, break_lines, collapse_to_line_top,
    float_margin_box_end, fonts, has_inline_end_edge, has_inline_start_edge, has_quirky_end_edge,
    has_quirky_start_edge, inline_box_style, is_continued, mark_hanging, move_placeholders,
    piece_extent, placed_text, placeholder, set_line_box, shape_marker, strut_for, strut_metrics,
    text_fragment,
};

impl LineBuilder<'_, '_> {
    /// The pieces of a line. A line that ends at a cut ends with the piece
    /// of the cut.
    pub(super) fn piece_range(groups: &[Group], line: &Line) -> std::ops::Range<usize> {
        if line.groups.is_empty() {
            return 0..0;
        }
        let start = groups[line.groups.start].pieces.start;
        match line.tail {
            Some((cut, _)) if cut.glyph.is_some() => start..cut.piece + 1,
            Some((cut, _)) => start..cut.piece,
            None => start..groups[line.groups.end - 1].pieces.end,
        }
    }

    /// True if the line is not empty (CSS 2.2 §9.4.2): it has text other
    /// than collapsible spaces, an atomic inline, a forced break, or an
    /// inline box edge with a non-zero margin, border or padding.
    pub(super) fn has_content(&self, groups: &[Group], line: &Line) -> bool {
        Self::piece_range(groups, line).any(|i| self.piece_has_content(i))
    }

    /// True if piece `i` makes a line not empty (see `has_content`). In
    /// quirks mode, margins do not make a line non-empty.
    pub(super) fn piece_has_content(&self, i: usize) -> bool {
        match self.shaped.pieces.get(i) {
            Some(Piece::Text { run, text, .. }) => {
                let collapses = self
                    .text_style(&self.shaped.runs[*run])
                    .white_space
                    .collapses_spaces();
                let text = self.ifc.text.get(text.clone()).unwrap_or("");
                !text.is_empty() && (!collapses || text.chars().any(|c| c != ' '))
            }
            Some(Piece::Atomic(_) | Piece::LineBreak(_)) => true,
            Some(Piece::StartBox(item)) => {
                !is_continued(self.ifc, *item)
                    && inline_box_style(self.ifc, *item).is_some_and(|s| {
                        has_quirky_start_edge(s)
                            || (!self.ctx.line_height_quirks && has_inline_start_edge(s))
                    })
            }
            Some(Piece::EndBox { start, split }) => {
                !split
                    && inline_box_style(self.ifc, *start).is_some_and(|s| {
                        has_quirky_end_edge(s)
                            || (!self.ctx.line_height_quirks && has_inline_end_edge(s))
                    })
            }
            Some(Piece::OutOfFlow(_)) | None => false,
        }
    }

    /// The width that piece `i` takes on a line.
    pub(super) fn piece_width(&self, i: usize) -> f32 {
        piece_extent(self.ifc, self.shaped, &self.atomics, self.width, i)
    }

    /// The cuts of `group` where it may break if it does not fit, with the
    /// width before each, up to the first one after `limit`.
    pub(super) fn cuts(&self, group: &Group, limit: f32) -> Vec<(Cut, f32)> {
        if !group.cuttable {
            return Vec::new();
        }
        GroupText {
            ifc: self.ifc,
            shaped: self.shaped,
            pieces: group.pieces.clone(),
            head: group.head,
        }
        .cuts(Purpose::Layout, limit, |i| self.piece_width(i))
    }

    /// The float at piece `i`, if it is one.
    pub(super) fn float_at(&self, i: usize) -> Option<usize> {
        match self.shaped.pieces.get(i) {
            Some(Piece::OutOfFlow(item)) if self.floats.get(*item).is_some_and(Option::is_some) => {
                Some(*item)
            }
            _ => None,
        }
    }

    /// Lays out a context without line boxes (no content, maybe floats): its
    /// lines are empty, and its floats are placed at the top of the
    /// container, or wait for the container's position.
    pub(super) fn layout_empty(&mut self, groups: &[Group], indent: f32, flow: Flow) {
        // The floats are placed first: the empty lines start after them, if
        // the position is known (then the static positions of the
        // placeholders on the lines follow from there).
        if let FlowY::Resolved(y) = flow.y {
            for slot in self.floats.iter_mut().flatten() {
                let (fragment, float) = &mut **slot;
                let parent = Point::new(self.flow_x, y);
                let position = self.ctx.bfc().place_in(float, y, parent);
                fragment.border_rect.x = position.x;
                fragment.border_rect.y = position.y;
                let end = float_margin_box_end(fragment, self.cb);
                self.in_flow_floats.width = self.in_flow_floats.width.max(end.width);
                self.in_flow_floats.height = self.in_flow_floats.height.max(end.height);
            }
        }
        let left = match flow.y {
            FlowY::Resolved(y) => {
                let cx1 = self.flow_x + self.width;
                self.ctx.bfc().exclusions.range_at(y, self.flow_x, cx1).0 - self.flow_x
            }
            FlowY::Pending { .. } => 0.0,
        };
        // The floats go among the fragments of the lines in tree order.
        let floats: Vec<FloatAt> = self
            .shaped
            .pieces
            .iter()
            .enumerate()
            .filter_map(|(piece, p)| match p {
                Piece::OutOfFlow(item) if self.floats.get(*item).is_some_and(Option::is_some) => {
                    Some(FloatAt { item: *item, piece })
                }
                _ => None,
            })
            .collect();
        let line_start = self.fragments.len();
        let mut pieces = Vec::new();
        for line in break_lines(groups, self.width, indent) {
            let open = std::mem::take(&mut self.open);
            let (state, open) = self.build_line(groups, &line, 0.0, &[], &open);
            self.open = open;
            pieces.extend(self.finish_empty_line(state, left));
        }
        let waiting = matches!(flow.y, FlowY::Pending { .. });
        self.insert_line_floats(line_start, &pieces, floats, waiting);
    }

    /// Lays out the lines of a context with line boxes, in a container
    /// whose content box is at BFC y `content_y`.
    pub(super) fn layout_lines(
        &mut self,
        groups: &mut [Group],
        indent: f32,
        content_y: f32,
        markers: &mut Vec<PendingMarker<'_>>,
    ) {
        let mut next = 0;
        let mut first = true;
        while next < groups.len() {
            let line_top = content_y + self.y;
            let start = LineStart {
                group: next,
                leading_end: self.place_leading_floats(groups, next, line_top, content_y),
                indent: if first { indent } else { 0.0 },
                content_y,
            };
            let Some(line) = self.fit_line(groups, &start, line_top, markers) else {
                return;
            };
            next = self.commit_line(groups, line, content_y, markers);
            first = false;
        }
    }

    /// Finds the first layout opportunity at or below `line_top` where the
    /// line from `start` fits, and builds the line there (Chromium's
    /// `InlineLayoutAlgorithm::Layout`). The opportunities are produced one
    /// at a time ([`crate::floats::Bfc`]): at each position, the widest
    /// first; if the line is too tall for it, the next narrower one that
    /// reaches lower; if the line is too wide, the next position where the
    /// free space changes. Once the line was too wide, opportunities
    /// narrower than its first word are skipped. After
    /// [`MAX_LINE_ATTEMPTS`] attempts, the line goes below all floats.
    pub(super) fn fit_line(
        &mut self,
        groups: &[Group],
        start: &LineStart,
        line_top: f32,
        markers: &[PendingMarker<'_>],
    ) -> Option<TriedLine> {
        let (cx0, cx1) = (self.flow_x, self.flow_x + self.width);
        if !self.ctx.bfc().exclusions.is_empty() {
            let mut attempts = 0;
            // The width the line needs, once it was too wide somewhere.
            let mut need = 0.0;
            let mut top = line_top;
            'positions: loop {
                let mut candidate = self.ctx.bfc().opportunity_at(top, cx0, cx1);
                while let Some(o) = candidate {
                    if !o.full_width && o.width() + FIT_TOLERANCE < need {
                        // Too wide here too, and in the narrower ones.
                        break;
                    }
                    candidate = match self.try_line(groups, start, &o, markers) {
                        Ok(line) => return Some(line),
                        Err(Reject::TooTall(at)) => self.ctx.bfc().narrower(&o, at, cx0, cx1),
                        Err(Reject::TooWide { need: n }) => {
                            need = n;
                            None
                        }
                    };
                    attempts += 1;
                    if attempts >= MAX_LINE_ATTEMPTS {
                        break 'positions;
                    }
                }
                match self.ctx.bfc().next_top(top) {
                    Some(next) => top = next,
                    None => break,
                }
            }
        }
        // The last opportunity always takes the line.
        let last = self.ctx.bfc().exclusions.below_all(line_top, cx0, cx1);
        self.try_line(groups, start, &last, markers).ok()
    }

    /// Places the floats at the start of the line that starts with group
    /// `start` (before its first content), at the line's top. Returns the
    /// index of the first piece after them.
    pub(super) fn place_leading_floats(
        &mut self,
        groups: &[Group],
        start: usize,
        line_top: f32,
        content_y: f32,
    ) -> usize {
        let Some(group) = groups.get(start) else {
            return 0;
        };
        if self.floats.is_empty() {
            return group.pieces.start;
        }
        let mut end = group.pieces.start;
        for i in group.pieces.clone() {
            if self.piece_has_content(i) {
                break;
            }
            if let Some(item) = self.float_at(i) {
                self.place_float_now(item, line_top, content_y);
                if let Some(Fragment::Box(b)) = self.fragments.last() {
                    let right = float_margin_box_end(b, self.cb).width;
                    self.in_flow_floats.width = self.in_flow_floats.width.max(right);
                }
            }
            end = i + 1;
        }
        end
    }

    /// Places float `item` at or below `origin` and adds its fragment.
    pub(super) fn place_float_now(&mut self, item: usize, origin: f32, content_y: f32) {
        self.place_float(item, origin, content_y);
        self.push_float(item, false);
    }

    /// Places float `item` at or below `origin` (its fragment stays in
    /// `floats` until [`LineBuilder::push_float`]).
    pub(super) fn place_float(&mut self, item: usize, origin: f32, content_y: f32) {
        let Some(Some(slot)) = self.floats.get_mut(item) else {
            return;
        };
        let (fragment, float) = &mut **slot;
        let parent = Point::new(self.flow_x, content_y);
        let position = self.ctx.bfc().place_in(float, origin, parent);
        fragment.border_rect.x = position.x;
        fragment.border_rect.y = position.y;
    }

    /// Adds the fragment of float `item`; with `waiting`, the float waits
    /// for the container's position (as the child with its index).
    pub(super) fn push_float(&mut self, item: usize, waiting: bool) {
        let Some((fragment, float)) = self.floats.get_mut(item).and_then(Option::take).map(|b| *b)
        else {
            return;
        };
        if waiting {
            self.ctx.bfc().add_pending(PendingFloat {
                path: vec![u32::try_from(self.fragments.len()).unwrap_or(u32::MAX)],
                float,
                origin_x: self.flow_x,
            });
        }
        self.fragments.push(Fragment::Box(fragment));
    }

    /// Puts the floats of a line among its fragments (from `line_start`,
    /// added at the pieces `pieces`) in tree order: before the first
    /// fragment added at a later piece. The paint order of positioned boxes
    /// with equal z-index follows the fragment order. A float inside an
    /// inline box goes before that box's fragment (which is added at the
    /// box's end).
    pub(super) fn insert_line_floats(
        &mut self,
        line_start: usize,
        pieces: &[usize],
        mut floats: Vec<FloatAt>,
        waiting: bool,
    ) {
        if floats.is_empty() {
            return;
        }
        floats.sort_by_key(|f| f.piece);
        let line = self
            .fragments
            .split_off(line_start.min(self.fragments.len()));
        let mut floats = floats.into_iter().peekable();
        for (k, fragment) in line.into_iter().enumerate() {
            let piece = pieces.get(k).copied().unwrap_or(usize::MAX);
            while let Some(float) = floats.next_if(|f| f.piece < piece) {
                self.push_float(float.item, waiting);
            }
            self.fragments.push(fragment);
        }
        for float in floats {
            self.push_float(float.item, waiting);
        }
    }

    /// Fills and builds the line from `start` in opportunity `o`. If it
    /// does not fit there, returns why; then nothing changed, and the
    /// attempt is charged to the work budget.
    pub(super) fn try_line(
        &mut self,
        groups: &[Group],
        start: &LineStart,
        o: &Opportunity,
        markers: &[PendingMarker<'_>],
    ) -> Result<TriedLine, Reject> {
        let fill = self.fill_line(groups, start, o);
        let line = Line {
            groups: start.group..fill.end,
            tail: fill.tail,
        };
        let empty = !self.has_content(groups, &line);
        // A line that is too wide moves down, unless no float narrows the
        // containing block here (Chromium's
        // `IsEqualToAvailableFloatInlineSize`) or the container does not
        // wrap lines (`ShouldWrapLine`).
        if fill.overflow && !empty && !o.full_width && self.container_style.white_space.wraps() {
            let need = self.min_line_width(groups, start);
            return Err(self.reject(groups, &line, fill, Reject::TooWide { need }));
        }
        let line_markers: &[PendingMarker<'_>] = if empty { &[] } else { markers };
        let open = self.open.clone();
        let (state, open) = self.build_line(groups, &line, start.indent, line_markers, &open);
        // The line's own floats do not count (they are beside it).
        let (cx0, cx1) = (self.flow_x, self.flow_x + self.width);
        let before = fill.checkpoint.as_ref();
        if !empty
            && let Some(at) = self
                .ctx
                .bfc()
                .narrowing_below(o, state.height(), cx0, cx1, before)
        {
            return Err(self.reject(groups, &line, fill, Reject::TooTall(at)));
        }
        Ok(TriedLine {
            state,
            open,
            line,
            empty,
            top: o.top,
            left: fill.left,
            right: fill.right,
            placed: fill.placed,
            queued: fill.queued,
        })
    }

    /// The width that the line from `start` needs at least: its text indent
    /// and its first group, which it always takes; 0 if that group has no
    /// content (the line may then be empty, and an empty line does not
    /// overflow).
    pub(super) fn min_line_width(&self, groups: &[Group], start: &LineStart) -> f32 {
        groups
            .get(start.group)
            .filter(|g| g.pieces.clone().any(|i| self.piece_has_content(i)))
            .map_or(0.0, |g| {
                // A group that can be cut needs room for its first part.
                let first = self.cuts(g, 0.0).first().map(|&(_, before)| before);
                start.indent + first.unwrap_or(g.width - g.trailing_space)
            })
    }

    /// Undoes the floats of a line attempt that does not fit, and charges
    /// the attempt to the work budget. Returns `reason`.
    pub(super) fn reject(
        &mut self,
        groups: &[Group],
        line: &Line,
        fill: Fill,
        reason: Reject,
    ) -> Reject {
        if let Some(checkpoint) = fill.checkpoint {
            self.ctx.bfc().restore(checkpoint);
        }
        let pieces = u64::try_from(Self::piece_range(groups, line).len()).unwrap_or(u64::MAX);
        self.ctx
            .bfc()
            .charge(LINE_RETRY_COST.saturating_mul(pieces.saturating_add(1)));
        reason
    }

    /// Fills a line greedily from `start` in opportunity `o`, and places
    /// the floats on it that fit beside the content before them (Chromium's
    /// `LineBreaker::HandleFloat`); the others are queued for below the
    /// line.
    pub(super) fn fill_line(
        &mut self,
        groups: &[Group],
        start: &LineStart,
        o: &Opportunity,
    ) -> Fill {
        let mut fill = Fill {
            end: start.group,
            left: o.left,
            right: o.right,
            overflow: false,
            tail: None,
            placed: Vec::new(),
            queued: Vec::new(),
            checkpoint: None,
            group_checkpoint: None,
        };
        let mut used = start.indent;
        // True once a group with content (see `piece_has_content`) is on
        // the line.
        let mut content_before = false;
        let mut trailing = 0.0;
        for (index, group) in groups.iter().enumerate().skip(start.group) {
            let width = used + group.width - group.trailing_space;
            if fill.end > start.group && width > fill.right - fill.left + FIT_TOLERANCE {
                break;
            }
            // A line that `text-wrap: balance` or `pretty` planned ends
            // here.
            if fill.end > start.group && self.planned.binary_search(&index).is_ok() {
                break;
            }
            if !self.floats.is_empty() && group.last_out_of_flow.is_some() {
                let site = FloatSite {
                    piece: 0,
                    position: used,
                    trailing_space: 0.0,
                    content_before,
                };
                if !self.group_floats(group, site, start, o, &mut fill) {
                    break;
                }
            }
            if !self.floats.is_empty() {
                content_before =
                    content_before || group.pieces.clone().any(|i| self.piece_has_content(i));
            }
            used += group.width;
            trailing = group.trailing_space;
            fill.end = index + 1;
            if group.forced_break_after {
                break;
            }
        }
        let available = fill.right - fill.left;
        fill.overflow = used - trailing > available + FIT_TOLERANCE;
        // A first group that does not fit is cut if it can be: at the last
        // cut where its start fits, or else at the first one.
        if fill.overflow
            && fill.end == start.group + 1
            && let Some(group) = groups.get(start.group)
        {
            let room = available - start.indent + FIT_TOLERANCE;
            let cuts = self.cuts(group, room);
            let cut = cuts.iter().rev().find(|(_, before)| *before <= room);
            if let Some(&(cut, before)) = cut.or(cuts.first()) {
                fill.tail = Some((cut, before));
                fill.overflow = before > room;
            }
        }
        fill
    }

    /// Handles the floats in `group`, which starts at `site.position` on
    /// the line. Returns false if a float inside the group (no break
    /// opportunity around it) leaves no room for the rest of the group:
    /// then the group's floats are undone, the line ends before the group,
    /// and the floats are placed again on the next line (Chromium rewinds
    /// the line breaker).
    pub(super) fn group_floats(
        &mut self,
        group: &Group,
        mut site: FloatSite,
        start: &LineStart,
        o: &Opportunity,
        fill: &mut Fill,
    ) -> bool {
        let used = site.position;
        let (left, right) = (fill.left, fill.right);
        let (placed, queued) = (fill.placed.len(), fill.queued.len());
        let (cx0, cx1) = (self.flow_x, self.flow_x + self.width);
        for i in group.pieces.clone() {
            if let Some(item) = self.float_at(i) {
                if i >= start.leading_end {
                    self.handle_float(item, FloatSite { piece: i, ..site }, start, o, fill);
                    let (l, r) = self.ctx.bfc().exclusions.range_at(o.top, cx0, cx1);
                    fill.left = fill.left.max(l);
                    fill.right = fill.right.min(r).max(fill.left);
                }
                continue;
            }
            site.position += self.piece_width(i);
            site.trailing_space = match self.shaped.pieces.get(i) {
                Some(Piece::Text { trailing_space, .. }) => *trailing_space,
                _ => 0.0,
            };
        }
        if let Some(checkpoint) = fill.group_checkpoint.take()
            && used + group.width - group.trailing_space > fill.right - fill.left + FIT_TOLERANCE
        {
            self.ctx.bfc().restore(checkpoint);
            (fill.left, fill.right) = (left, right);
            fill.placed.truncate(placed);
            fill.queued.truncate(queued);
            return false;
        }
        true
    }

    /// Places float `item`, found at `site` on a line at the top of `o`, on
    /// the line if it fits, else queues it for below the line. The state
    /// before the first float placed on the line is saved, for a retry in
    /// another opportunity; if the line already has content, also the
    /// state before the first float of the group.
    pub(super) fn handle_float(
        &mut self,
        item: usize,
        site: FloatSite,
        start: &LineStart,
        o: &Opportunity,
        fill: &mut Fill,
    ) {
        let line_top = o.top;
        let Some((_, float)) = self.floats.get(item).and_then(Option::as_deref) else {
            return;
        };
        let width = float.width.max(0.0);
        let available = fill.right - fill.left;
        let fits = site.position + width <= available + FIT_TOLERANCE
            || site.position - site.trailing_space + width <= available + FIT_TOLERANCE;
        let exclusions = &self.ctx.bfc().exclusions;
        let below = !fits
            || exclusions.last_float_top() > line_top + EPSILON
            || exclusions
                .clearance(float.clear)
                .is_some_and(|c| c > line_top + EPSILON)
            || !fill.queued.is_empty();
        if below {
            fill.queued.push(FloatAt {
                item,
                piece: site.piece,
            });
            return;
        }
        let float = *float;
        // The last opportunity always takes the line, so nothing to undo
        // there; once the work budget is spent, every opportunity is the
        // last and nothing is copied. The rewind of a word is skipped then
        // too.
        if fill.checkpoint.is_none() && !o.last {
            fill.checkpoint = Some(self.ctx.bfc().checkpoint());
        }
        if site.content_before && fill.group_checkpoint.is_none() && self.ctx.bfc().budget > 0 {
            fill.group_checkpoint = Some(self.ctx.bfc().checkpoint());
        }
        let parent = Point::new(self.flow_x, start.content_y);
        let position = self.ctx.bfc().place_in(&float, line_top, parent);
        fill.placed.push(LineFloat {
            item,
            piece: site.piece,
            position,
        });
    }

    /// Adds a tried line to the layout; places the floats queued for below
    /// it. Returns the first group of the next line.
    pub(super) fn commit_line(
        &mut self,
        groups: &mut [Group],
        line: TriedLine,
        content_y: f32,
        markers: &mut Vec<PendingMarker<'_>>,
    ) -> usize {
        let TriedLine {
            state,
            open,
            line,
            empty,
            top,
            left,
            right,
            placed,
            queued,
        } = line;
        self.open = open;
        let mut floats = Vec::with_capacity(placed.len() + queued.len());
        for LineFloat {
            item,
            piece,
            position,
        } in placed
        {
            if let Some(Some(slot)) = self.floats.get_mut(item) {
                slot.0.border_rect.x = position.x;
                slot.0.border_rect.y = position.y;
            }
            floats.push(FloatAt { item, piece });
        }
        let line_start = self.fragments.len();
        let pieces = if empty {
            self.finish_empty_line(state, left - self.flow_x)
        } else {
            markers.clear();
            self.y = top - content_y;
            let soft_wrap = self.ends_at_soft_wrap(Self::piece_range(groups, &line));
            self.finish_line(state, left - self.flow_x, right - left, soft_wrap)
        };
        let bottom = content_y + self.y;
        for float in queued {
            self.place_float(float.item, bottom, content_y);
            floats.push(float);
        }
        self.insert_line_floats(line_start, &pieces, floats, false);
        // A `<br>` with `clear` moves the next line below the floats.
        if let Some(clear) = self.line_break_clear(groups, &line)
            && let Some(c) = self.ctx.bfc().exclusions.clearance(clear)
        {
            self.y = self.y.max(c - content_y);
        }
        // The rest of a cut group starts the next line.
        if let Some((cut, before)) = line.tail
            && let Some(index) = line.groups.end.checked_sub(1)
            && let Some(group) = groups.get_mut(index)
        {
            *group = Group {
                pieces: cut.piece..group.pieces.end,
                head: cut.glyph.is_some().then_some(cut),
                width: group.width - before,
                trailing_space: group.trailing_space,
                forced_break_after: group.forced_break_after,
                last_out_of_flow: group.last_out_of_flow.filter(|&p| p >= cut.piece),
                cuttable: group.cuttable,
            };
            return index;
        }
        line.groups.end
    }

    /// Builds a line; `open` are the inline boxes that continue from the
    /// previous line. Returns the line and the boxes that continue on the
    /// next line.
    ///
    /// A line with boxes that have `vertical-align: top` or `bottom` is
    /// built twice: the first pass finds the extent of the rest of the line
    /// and of each aligned subtree, the second places those boxes at their
    /// baseline shifts (CSS 2.2 §10.8.1).
    pub(super) fn build_line(
        &mut self,
        groups: &[Group],
        line: &Line,
        indent: f32,
        markers: &[PendingMarker<'_>],
        open: &[(usize, Arc<ComputedStyle>)],
    ) -> (LineState, Vec<(usize, Arc<ComputedStyle>)>) {
        let (state, continuing) =
            self.build_line_pass(groups, line, indent, markers, open, Vec::new());
        if state.relative.is_empty() {
            return (state, continuing);
        }
        let offsets = state.compute_relative_offsets();
        let (mut state, continuing) = if offsets.iter().any(|o| o.abs() > EPSILON) {
            self.build_line_pass(groups, line, indent, markers, open, offsets)
        } else {
            (state, continuing)
        };
        state.extent = state.line_extent();
        (state, continuing)
    }

    /// One pass of [`Self::build_line`]; `offsets` are the baseline shifts
    /// of the line-relative boxes (empty on the first pass).
    pub(super) fn build_line_pass(
        &mut self,
        groups: &[Group],
        line: &Line,
        indent: f32,
        markers: &[PendingMarker<'_>],
        open: &[(usize, Arc<ComputedStyle>)],
        offsets: Vec<f32>,
    ) -> (LineState, Vec<(usize, Arc<ComputedStyle>)>) {
        self.ctx.layout_units += 1;
        let quirky =
            self.ctx.line_height_quirks && self.container_style.display != Display::ListItem;
        let mut state = LineState::new(self.root_metrics, indent, quirky);
        state.relative_offsets = offsets;
        // Re-open boxes that continue from the previous line.
        for (item, style) in open {
            let (baseline, strut) = state.parent();
            let open = self.open_box(*item, style, state.x, baseline, &strut, true);
            let open = Self::start_relative_box(&mut state, open);
            state.push_open(open, false);
        }

        let piece_range = Self::piece_range(groups, line);
        // A line that ends at a cut has no trailing spaces.
        let trim = match line.tail {
            Some(_) => None,
            None => self.trailing_space_to_remove(piece_range.clone()),
        };
        let head = groups.get(line.groups.start).and_then(|g| g.head);
        let tail = line.tail.map(|(cut, _)| cut);
        let end = piece_range.end;
        for i in piece_range {
            state.piece = i;
            match &self.shaped.pieces[i] {
                Piece::StartBox(item) => self.start_box(&mut state, *item),
                Piece::EndBox { split, .. } => {
                    if let Some(mut open) = state.stack.pop() {
                        if state.quirky && !split && has_quirky_end_edge(&open.style) {
                            state.include_strut(&mut open);
                        }
                        if let Some(index) = open.relative {
                            state.end_relative(index, open.baseline);
                        }
                        if open.has_metrics {
                            state.set_parent_has_metrics();
                        }
                        let fragment = self.close_box(open, &mut state.x, *split);
                        state.push_child(Fragment::Box(fragment));
                    }
                }
                Piece::Text { glyphs, offset, .. } => {
                    let (first, base) = Cut::glyph_in(head, i).unwrap_or((glyphs.start, *offset));
                    let last = Cut::glyph_in(tail, i).map_or(glyphs.end, |(g, _)| g);
                    if first < last {
                        let trim_end = trim.filter(|(piece, _)| *piece == i).map(|(_, end)| end);
                        self.place_text(&mut state, i, (first..last, base), trim_end);
                    }
                }
                Piece::Atomic(item) => self.place_atomic(&mut state, *item),
                Piece::LineBreak(item) => self.place_line_break(&mut state, *item),
                Piece::OutOfFlow(item) => self.place_out_of_flow(&mut state, *item),
            }
        }

        // Close boxes that continue on the next line.
        state.piece = end;
        let mut continuing = Vec::new();
        while let Some(open) = state.stack.pop() {
            if let Some(index) = open.relative {
                state.end_relative(index, open.baseline);
            }
            continuing.insert(0, (open.item, Arc::clone(&open.style)));
            let fragment = self.close_box(open, &mut state.x, true);
            state.push_child(Fragment::Box(fragment));
        }
        for pending in markers {
            self.place_marker(*pending, &mut state);
        }
        (state, continuing)
    }

    /// The `clear` of the `<br>` that ends a line, if any.
    pub(super) fn line_break_clear(
        &self,
        groups: &[Group],
        line: &Line,
    ) -> Option<swb_style::Clear> {
        let last = Self::piece_range(groups, line).last()?;
        match self.shaped.pieces.get(last) {
            Some(Piece::LineBreak(item)) => match self.ifc.items.get(*item) {
                Some(InlineItem::LineBreak(Some(base)))
                    if base.style.clear != swb_style::Clear::None =>
                {
                    Some(base.style.clear)
                }
                _ => None,
            },
            _ => None,
        }
    }

    /// True if the line of the pieces `range` ends at a soft wrap: not at
    /// a forced line break and not at the end of the content.
    pub(super) fn ends_at_soft_wrap(&self, range: std::ops::Range<usize>) -> bool {
        if range.end >= self.shaped.pieces.len() {
            return false;
        }
        let last = self.shaped.pieces.get(range).and_then(|pieces| {
            pieces.iter().rev().find(|p| {
                !matches!(
                    p,
                    Piece::StartBox(_) | Piece::EndBox { .. } | Piece::OutOfFlow(_)
                )
            })
        });
        !matches!(last, Some(Piece::LineBreak(_)))
    }

    pub(super) fn start_box(&mut self, state: &mut LineState, item: usize) {
        let Some(style) = inline_box_style(self.ifc, item).cloned() else {
            return;
        };
        let (baseline, strut) = state.parent();
        let continued = is_continued(self.ifc, item);
        let open = self.open_box(item, &style, state.x, baseline, &strut, continued);
        if !continued {
            state.x += open.edges.start();
        }
        let open = Self::start_relative_box(state, open);
        state.push_open(open, !continued && has_quirky_start_edge(&style));
        state.trailing_space = 0.0;
    }

    /// Starts the aligned subtree of an inline box with `vertical-align:
    /// top` or `bottom`, before its strut counts; other boxes are returned
    /// as they are.
    pub(super) fn start_relative_box(state: &mut LineState, mut open: OpenBox) -> OpenBox {
        if let Some(edge) = LineEdge::of(&open.style.vertical_align) {
            let (index, offset) = state.start_relative(edge);
            open.relative = Some(index);
            open.baseline += offset;
        }
        open
    }

    /// Places glyphs `part.0` of a text piece (all of them, unless the
    /// piece is cut); `part.1` is the advance of the item before them.
    /// `trim_end` is the end of its glyphs without trailing spaces, if they
    /// are removed at the end of the line.
    pub(super) fn place_text(
        &mut self,
        state: &mut LineState,
        piece: usize,
        part: (std::ops::Range<usize>, f64),
        trim_end: Option<usize>,
    ) {
        let (part, base) = part;
        let Some(Piece::Text { run, .. }) = self.shaped.pieces.get(piece) else {
            return;
        };
        let run = &self.shaped.runs[*run];
        let Some(placed) = placed_text(
            &self.shaped.pieces[piece],
            run,
            part.clone(),
            base,
            trim_end,
        ) else {
            return;
        };
        let (baseline, _) = state.parent();
        let style = self.text_style(run);
        // Text whose glyphs are all removed at the end of the line (only
        // collapsible spaces) does not count for the line height (Chromium
        // skips text items of length 0).
        if trim_end != Some(part.start) {
            let strut = strut_for(&style, self.extent_metrics(&style, run));
            state
                .extent
                .include(baseline - strut.above, baseline + strut.below);
            state.include_parent_strut(false);
            state.has_content = true;
        }
        let fragment = text_fragment(
            self.ifc,
            run,
            placed.glyphs,
            placed.text,
            &style,
            Point::new(state.x, baseline),
            placed.width,
        );
        state.x += placed.width;
        state.trailing_space = placed.trailing_space;
        state.push_child(Fragment::Text(fragment));
    }

    /// Places a forced line break; a `<br>` gets a box of width 0 with the
    /// content area of its parent's font (Chromium; with the line height
    /// quirks, height 0 at the baseline if the parent's strut does not
    /// count).
    pub(super) fn place_line_break(&mut self, state: &mut LineState, item: usize) {
        state.include_parent_strut(true);
        let Some(InlineItem::LineBreak(Some(base))) = self.ifc.items.get(item) else {
            return;
        };
        let (baseline, strut) = state.parent();
        let (top, height) = if state.parent_has_strut() {
            (
                baseline - strut.metrics.ascent,
                strut.metrics.ascent + strut.metrics.descent,
            )
        } else {
            (baseline, 0.0)
        };
        let mut fragment = crate::block::finish_fragment(
            base,
            Rect::new(state.x, top, 0.0, height),
            &BoxEdges::default(),
            Vec::new(),
            crate::block::Baselines::default(),
        );
        fragment.is_inline = true;
        state.push_child(Fragment::Box(fragment));
    }

    pub(super) fn place_atomic(&mut self, state: &mut LineState, item: usize) {
        state.trailing_space = 0.0;
        state.has_content = true;
        let Some(atomic) = self.atomics.get(item).and_then(|a| a.as_deref()).cloned() else {
            return;
        };
        let InlineItem::Atomic { inner, .. } = &self.ifc.items[item] else {
            return;
        };
        let style = &inner.base.style;
        let (parent_baseline, parent_strut) = state.parent();
        let own_line_height = if matches!(style.vertical_align, VerticalAlign::LengthPercentage(_))
        {
            strut_metrics(self.ctx, style).line_height
        } else {
            0.0
        };
        let baseline = if let Some(edge) = LineEdge::of(&style.vertical_align) {
            let below = atomic.margin_height - atomic.baseline;
            parent_baseline + state.add_relative_atomic(edge, atomic.baseline, below)
        } else {
            let baseline = parent_baseline
                + baseline_shift(
                    &style.vertical_align,
                    &parent_strut,
                    own_line_height,
                    atomic.baseline,
                    atomic.margin_height,
                );
            let top = baseline - atomic.baseline;
            state.extent.include(top, top + atomic.margin_height);
            baseline
        };
        let top = baseline - atomic.baseline;
        state.set_parent_has_metrics();
        if let Some(mut fragment) = atomic.fragment {
            fragment.border_rect.x += state.x;
            fragment.border_rect.y = top + atomic.margin_top;
            apply_relative_position(&mut fragment, self.cb);
            state.push_child(Fragment::Box(fragment));
        }
        state.x += atomic.margin_width;
    }

    /// Places the placeholder of an absolutely positioned box at the
    /// current position (floats are not placed). Its vertical position
    /// (and for a box that was block-level, its horizontal one) is set
    /// when the line box is known: see [`move_placeholders`].
    pub(super) fn place_out_of_flow(&mut self, state: &mut LineState, item: usize) {
        let Some(InlineItem::AbsolutelyPositioned(ib)) = self.ifc.items.get(item) else {
            return;
        };
        let at = Point::new(state.x, 0.0);
        let inline_level = ib.base.style.original_display.is_inline_level();
        // The static-position rectangle has no width; it gets the height
        // of the line box in `move_placeholders`.
        let extent = crate::geom::Size::default();
        let fragment = placeholder(self.ctx, ib, at, StaticParent::Inline, extent);
        state.push_child(fragment);
        state.placeholders.push(if inline_level {
            StaticRule::Inline
        } else {
            StaticRule::Block {
                after_content: state.has_content,
            }
        });
    }

    /// Aligns the line horizontally in its free range (`left` px from the
    /// content box's left edge, `width` px wide), positions it at `self.y`
    /// and appends its fragments. Outside list markers go to the left of
    /// the free range, as in Chromium. `soft_wrap` is true if the line ends
    /// at a soft wrap opportunity.
    pub(super) fn finish_line(
        &mut self,
        mut state: LineState,
        left: f32,
        width: f32,
        soft_wrap: bool,
    ) -> Vec<usize> {
        state.extent = state.extent.or_zero();
        let line_width = state.x - state.trailing_space;
        let offset = left + self.align_offset(line_width, width);
        self.content_right = self.content_right.max(offset + line_width);
        let line_top = self.y;
        let baseline_y = line_top - state.extent.top;
        for fragment in &mut state.top_level {
            fragment.move_by(offset, baseline_y);
        }
        for fragment in &mut state.outside_markers {
            fragment.move_by(left, baseline_y);
        }
        let line_height = state.extent.bottom - state.extent.top;
        let cb = self.cb;
        // The white space at the end hangs at a soft wrap, and at a forced
        // break or the end of the content only if it does not fit (CSS
        // Text 3 §4.1.3, as Chromium 148).
        let hangs = state.trailing_space > 0.0 && (soft_wrap || state.x > width);
        for fragment in &mut state.top_level {
            set_line_box(fragment, 0.0, line_top, line_height, cb);
            if hangs {
                mark_hanging(fragment, offset + line_width);
            }
        }
        if self.first_baseline.is_none() {
            self.first_baseline = Some(baseline_y);
        }
        self.last_baseline = Some(baseline_y);
        self.line_count += 1;
        if !state.placeholders.is_empty() {
            let line = (line_top, line_top + line_height);
            move_placeholders(&mut state.top_level, &state.placeholders, line);
        }
        let mut pieces = std::mem::take(&mut state.top_level_pieces);
        pieces.resize(pieces.len() + state.outside_markers.len(), usize::MAX);
        self.fragments.append(&mut state.top_level);
        self.fragments.append(&mut state.outside_markers);
        self.y = line_top + line_height;
        pieces
    }

    /// Appends the fragments of an empty line: zero height at the current
    /// position, `left` px from the content box's left edge (Chromium gives
    /// its inline boxes zero-height fragments).
    pub(super) fn finish_empty_line(&mut self, mut state: LineState, left: f32) -> Vec<usize> {
        for fragment in &mut state.top_level {
            collapse_to_line_top(fragment);
            fragment.move_by(left, self.y);
        }
        if !state.placeholders.is_empty() {
            let line = (self.y, self.y);
            move_placeholders(&mut state.top_level, &state.placeholders, line);
        }
        self.fragments.append(&mut state.top_level);
        state.top_level_pieces
    }

    /// The last text piece of a line and the end of its glyphs without the
    /// trailing collapsible spaces, which are removed at the end of a line
    /// (CSS Text 3 §4.1.2,
    /// <https://www.w3.org/TR/css-text-3/#white-space-phase-2>). `None` if
    /// there is nothing to remove.
    pub(super) fn trailing_space_to_remove(
        &self,
        pieces: std::ops::Range<usize>,
    ) -> Option<(usize, usize)> {
        for i in pieces.rev() {
            match &self.shaped.pieces[i] {
                Piece::Text { run, glyphs, .. } => {
                    let run_data = &self.shaped.runs[*run];
                    if !self.text_style(run_data).white_space.collapses_spaces() {
                        return None;
                    }
                    let mut end = glyphs.end;
                    while end > glyphs.start
                        && self.ifc.text[run_data.glyphs[end - 1].cluster..].starts_with(' ')
                    {
                        end -= 1;
                    }
                    return (end < glyphs.end).then_some((i, end));
                }
                Piece::Atomic(_) => return None,
                _ => {}
            }
        }
        None
    }

    /// The font metrics that a text run adds to the line height. Measured
    /// in Chromium 148: the metrics of a fallback font count only with
    /// `line-height: normal`; with any other value the run counts with the
    /// primary font of its style (whose strut is on the line anyway), so a
    /// glyph from a fallback font does not make the line taller.
    pub(super) fn extent_metrics(&mut self, style: &ComputedStyle, run: &Run) -> LineMetrics {
        if style.line_height.resolve(style.font_size).is_none() {
            return run.metrics;
        }
        let primary = fonts::primary_font(self.ctx.fonts, style);
        if primary == run.font {
            return run.metrics;
        }
        fonts::line_metrics(self.ctx.fonts, primary, style.font_size)
    }

    pub(super) fn text_style(&self, run: &Run) -> Arc<ComputedStyle> {
        match &self.ifc.items[run.item] {
            InlineItem::Text { style, .. } => Arc::clone(style),
            _ => Arc::clone(self.container_style),
        }
    }

    pub(super) fn open_box(
        &mut self,
        item: usize,
        style: &Arc<ComputedStyle>,
        start_x: f32,
        parent_baseline: f32,
        parent_strut: &Strut,
        continued: bool,
    ) -> OpenBox {
        let strut = strut_metrics(self.ctx, style);
        let shift = baseline_shift(
            &style.vertical_align,
            parent_strut,
            strut.line_height,
            strut.above,
            strut.above + strut.below,
        );
        let edges = InlineBoxEdges::resolve(style, self.width);
        OpenBox {
            item,
            style: Arc::clone(style),
            start_x: if continued {
                start_x
            } else {
                start_x + edges.margin_left
            },
            baseline: parent_baseline + shift,
            strut,
            edges,
            continued,
            has_metrics: false,
            has_strut: false,
            relative: None,
            children: Vec::new(),
        }
    }

    /// Finishes an inline box fragment. `split` is true if the box
    /// continues on the next line (no end edge on this line).
    pub(super) fn close_box(&self, open: OpenBox, x: &mut f32, split: bool) -> BoxFragment {
        let mut border = open.edges.edges.border;
        let mut padding = open.edges.edges.padding;
        if open.continued {
            border.left = 0.0;
            padding.left = 0.0;
        }
        if split {
            border.right = 0.0;
            padding.right = 0.0;
        } else {
            *x += border.right + padding.right;
        }
        let m = open.strut.metrics;
        let top = open.baseline - m.ascent - border.top - padding.top;
        let height = m.ascent + m.descent + border.vertical() + padding.vertical();
        let rect = Rect::new(open.start_x, top, (*x - open.start_x).max(0.0), height);
        if !split {
            *x += open.edges.margin_right;
        }
        let mut children = open.children;
        for child in &mut children {
            child.move_by(-rect.x, -rect.y);
        }
        let (node, pseudo) = match &self.ifc.items[open.item] {
            InlineItem::StartBox { base, .. } => (base.node, base.pseudo),
            _ => (None, None),
        };
        let mut fragment = BoxFragment {
            node,
            pseudo,
            style: open.style,
            border_rect: rect,
            border,
            padding,
            content: BoxContent::None,
            children: Arc::new(children),
            first_baseline: None,
            last_baseline: None,
            is_inline: true,
            scrollable_overflow: None,
            overflow_base: None,
            in_positioned_inline: false,
            hanging_from: None,
            is_root: false,
            hidden: None,
        };
        apply_relative_position(&mut fragment, self.cb);
        fragment
    }

    /// Places a list marker on the line. An outside marker goes to the left
    /// of its list item's content box; an inside marker is the first
    /// content of the line and moves the rest to the right.
    pub(super) fn place_marker(&mut self, pending: PendingMarker<'_>, state: &mut LineState) {
        let shaped = shape_marker(self.ctx, pending.marker);
        state.extent.include(-shaped.above, shaped.below);
        // With the line height quirks, a marker brings the root strut.
        state.extent.include(-state.root.above, state.root.below);
        if pending.marker.outside {
            let x = pending.x(shaped.width);
            state.outside_markers.extend(shaped.place(x, 0.0));
        } else {
            for fragment in &mut state.top_level {
                fragment.move_by(shaped.width, 0.0);
            }
            state.x += shaped.width;
            let start = state.top_level.len();
            state.top_level.extend(shaped.place(0.0, 0.0));
            let added = state.top_level.len() - start;
            state
                .top_level_pieces
                .extend(std::iter::repeat_n(state.piece, added));
        }
    }

    /// The horizontal offset of a line for `text-align`. `justify` is not
    /// supported and aligns to the start.
    pub(super) fn align_offset(&self, line_width: f32, width: f32) -> f32 {
        let free = width - line_width;
        if free <= 0.0 {
            return 0.0;
        }
        match self.container_style.text_align {
            TextAlign::Center | TextAlign::WebkitCenter => free / 2.0,
            TextAlign::Right | TextAlign::WebkitRight | TextAlign::End => free,
            _ => 0.0,
        }
    }
}
