//! The groups of the display list builder: the transform groups, and the
//! opacity, mask, clip path, filter and backdrop groups and the stacking
//! context that a box starts and ends (CSS Masking 1 §3 gives the order of
//! the effects).

use std::sync::Arc;

use swb_layout::{BoxContent, BoxFragment, GroupTransform, Point, Rect, has_rounded_corner};
use swb_style::Visibility;

use super::{Builder, DisplayItem, MaskBoxes, MaskLayer, mask, resolve_radii};

/// What [`Builder::open_group`] started for a box.
pub(super) struct Group {
    /// The index of the `PushOpacity` item, if the box has opacity < 1 or
    /// a `mix-blend-mode`.
    pub(super) opacity_item: Option<usize>,
    /// True if the opacity group starts outside the box's transform
    /// groups: [`Builder::box_contents`] ends it.
    pub(super) hoisted: bool,
    /// The index of the `PushMask` item, if the box is masked.
    pub(super) mask_item: Option<usize>,
    /// The item that ends the clip of the `clip-path` of the box (a basic
    /// shape or a geometry box), if it has one.
    pub(super) clip_pop: Option<DisplayItem>,
    /// True if the box started a filter group.
    pub(super) filter: bool,
    /// The number of groups (opacity, mask, clip path, filter) that the box
    /// started: each one is a layer in the rasterizer.
    pub(super) layers: u8,
    /// True if [`Builder::open_group`] already pushed the backdrop item of
    /// the box (before its opacity group).
    pub(super) backdrop_done: bool,
    /// True if the box opened a stacking context.
    pub(super) context: bool,
    /// `contexts.len()` before the box's own context.
    pub(super) outer_depth: usize,
}

impl Builder<'_> {
    /// Starts the transform groups `groups` of a box (see
    /// [`Ancestry::group_transforms`]). Returns the indices of the
    /// `PushTransform` items.
    pub(super) fn open_transforms(&mut self, groups: &[GroupTransform]) -> std::ops::Range<usize> {
        let start = self.list.len();
        self.list
            .extend(groups.iter().map(|transform| DisplayItem::PushTransform {
                transform: transform.clone(),
                bounds: Rect::default(),
                hit_bounds: Rect::default(),
                fixed_bounds: None,
            }));
        start..self.list.len()
    }

    /// Ends the transform groups that [`Builder::open_transforms`]
    /// started. Their bounds are computed when the list is finished
    /// (`group_bounds.rs`).
    pub(super) fn close_transforms(&mut self, transforms: std::ops::Range<usize>) {
        self.list
            .extend(transforms.map(|_| DisplayItem::PopTransform));
    }

    /// Starts the opacity group, the mask group, the clip of a basic shape
    /// and the stacking context of a box, as needed. `origin` is the
    /// parent's border-box origin. The effects apply in the order of CSS
    /// Masking 1 §3: the clip first, then the mask, then the opacity.
    /// `early` is the opacity group that [`Builder::box_contents`] started
    /// outside the box's transform (see [`blends_outside_transform`]).
    pub(super) fn open_group(
        &mut self,
        b: &BoxFragment,
        origin: Point,
        stacking_context: bool,
        early: Option<usize>,
    ) -> Group {
        let opacity = b.style.opacity;
        let outer_depth = self.contexts.len();
        // The opacity of a box with a `backdrop-filter` applies to the
        // filtered backdrop and to the box separately (measured in Chromium
        // 148: the backdrop is drawn with the opacity, then the box's group
        // with it), so the backdrop item comes before the opacity group.
        // With a mask, a clip path or a filter on the box, they apply to
        // both, and the opacity group holds both (departure).
        //
        // The same holds for a `mix-blend-mode` and for the layer that
        // isolates a stacking context with blended boxes (measured in
        // Chromium 148: the filtered backdrop, then the box, then the
        // blend; a parent's filtered backdrop is outside the isolated
        // group). The group of a blend that sits outside the transform
        // (`early`) holds the backdrop item (departure).
        let isolated = self.blend_contexts.contains(&std::ptr::from_ref(b));
        let grouped = opacity < 1.0 || b.style.has_blend_mode() || isolated;
        let backdrop_done = grouped
            && early.is_none()
            && opacity > 0.0
            && !b.style.has_mask()
            && !b.style.clip_path.is_shape()
            && !b.style.has_filter()
            && self.backdrop_item(b, origin, (0, opacity), outer_depth == 0);
        // A `mix-blend-mode` makes the same group, which blends when it
        // ends (CSS Compositing and Blending 1 §3.2).
        let opacity_item = early.or_else(|| grouped.then(|| self.push_opacity(b)));
        // The layers are computed when the group ends.
        let mask_item = b.style.has_mask().then(|| {
            self.list.push(DisplayItem::PushMask {
                bounds: Rect::default(),
                layers: Arc::from([]),
            });
            self.list.len() - 1
        });
        let clip = crate::clip_path::box_clip(b, origin);
        let clip_pop = clip.map(|clip| {
            self.list.push(clip.push);
            clip.pop
        });
        // The filter applies first, inside the clip (Filter Effects 1 §5).
        // A `url()` and functions that change nothing paint no group.
        let filtered = b.style.has_filter();
        let filter = filtered && self.push_filter(b);
        if (opacity_item.is_some() || mask_item.is_some() || clip_pop.is_some() || filtered)
            && outer_depth == 0
        {
            let canvas = std::mem::take(&mut self.root_canvas);
            self.list.append(canvas);
        }
        let context = stacking_context
            || opacity_item.is_some()
            || mask_item.is_some()
            || clip_pop.is_some()
            || filtered
            || b.style.has_backdrop_filter()
            || b.style.will_change_makes_stacking_context()
            || b.style.has_layout_containment()
            || b.style.has_paint_containment()
            || outer_depth == 0;
        if context {
            self.contexts.push(Vec::new());
        }
        let layers = u8::from(opacity_item.is_some())
            + u8::from(mask_item.is_some())
            + u8::from(matches!(clip_pop, Some(DisplayItem::PopSvgClip)))
            + u8::from(filter);
        Group {
            opacity_item,
            hoisted: early.is_some(),
            mask_item,
            clip_pop,
            filter,
            layers,
            backdrop_done,
            context,
            outer_depth,
        }
    }

    /// Draws the filtered backdrop of a box (its `backdrop-filter`) and
    /// starts the layer of a backdrop root with `backdrop-filter`
    /// descendants that has none. Call it after [`Builder::open_group`],
    /// before the box paints. Returns true if it started a layer, which
    /// [`Builder::close_backdrop`] ends.
    ///
    /// The root layer starts after the box's own backdrop item: the
    /// backdrop of a descendant holds what the box painted before it, not
    /// the filtered backdrop of the box (measured in Chromium 148).
    pub(super) fn open_backdrop(&mut self, b: &BoxFragment, origin: Point, group: &Group) -> bool {
        let style = &b.style;
        if !group.backdrop_done {
            self.backdrop_item(b, origin, (group.layers, 1.0), group.outer_depth == 0);
        }
        let root = self.backdrop_roots.contains(&std::ptr::from_ref(b))
            && (group.layers == 0 || style.has_backdrop_filter());
        if root {
            self.list.push(DisplayItem::opacity_group(
                1.0,
                swb_style::MixBlendMode::Normal,
            ));
        }
        root
    }

    /// Pushes the backdrop item of a box with a `backdrop-filter` that
    /// paints: `layers` groups of the box are open, and `alpha` is the
    /// opacity that the filtered backdrop is drawn with. Returns true if
    /// it pushed the item. The root element has none: nothing is painted
    /// before it but the canvas.
    pub(super) fn backdrop_item(
        &mut self,
        b: &BoxFragment,
        origin: Point,
        (layers, alpha): (u8, f32),
        is_root: bool,
    ) -> bool {
        let style = &b.style;
        let Some(list) = style.backdrop_filter.list() else {
            return false;
        };
        if is_root
            || !crate::filter::paints(list)
            || style.visibility != Visibility::Visible
            || b.content == BoxContent::GeometryOnly
        {
            return false;
        }
        let rect = b.border_rect.translate(origin);
        if !(rect.width > 0.0 && rect.height > 0.0) {
            return false;
        }
        let radii = resolve_radii(style, rect);
        let rounded = has_rounded_corner(&radii);
        self.list.push(DisplayItem::BackdropFilter {
            filter: Arc::clone(list),
            current_color: style.color,
            rect,
            radii: rounded.then_some(radii),
            alpha,
            layers,
        });
        true
    }

    /// Ends the layer that [`Builder::open_backdrop`] started.
    pub(super) fn close_backdrop(&mut self, started: bool) {
        if started {
            self.list.push(DisplayItem::PopOpacity);
        }
    }

    /// Starts the filter group of a box if its `filter` paints. Returns
    /// true if it did. The bounds are computed when the list is finished.
    pub(super) fn push_filter(&mut self, b: &BoxFragment) -> bool {
        let Some(list) = b.style.filter.list() else {
            return false;
        };
        if !crate::filter::paints(list) {
            return false;
        }
        self.list.push(DisplayItem::PushFilter {
            filter: Arc::clone(list),
            current_color: b.style.color,
            bounds: Rect::default(),
            fixed_bounds: None,
            escapes_clips: false,
        });
        true
    }

    /// Ends the mask and opacity groups of a box and records the areas
    /// they draw. `origin` is the parent's border-box origin; `positioned`
    /// the area of the positioned descendants, if a `no-clip` mask needs
    /// it.
    pub(super) fn close_group(
        &mut self,
        group: &Group,
        b: &BoxFragment,
        origin: Point,
        positioned: Option<Rect>,
    ) {
        if group.filter {
            self.list.push(DisplayItem::PopFilter);
        }
        if let Some(pop) = &group.clip_pop {
            self.list.push(pop.clone());
        }
        if let Some(at) = group.mask_item {
            // With `no-clip`, the area of the descendants with their own
            // layer in Chromium: positioned ones, and nested groups (with
            // the positioned descendants inside them). Each `no-clip` mask
            // reads the items of its content once.
            let layered = mask::needs_positioned_area(&b.style)
                .then(|| {
                    positioned
                        .into_iter()
                        .chain(mask::group_area(self.list.iter_from(at + 1)))
                        .reduce(|a, b| a.union(&b))
                })
                .flatten();
            let boxes = MaskBoxes::of(b, origin, layered);
            let layers = mask::layers(&b.style, &boxes, self.images);
            // The bounds are the content inside this extent (computed when
            // the list is finished, `group_bounds.rs`).
            let extent = layers
                .iter()
                .filter_map(MaskLayer::extent)
                .reduce(|a, b| a.union(&b))
                .unwrap_or_default();
            if let Some(DisplayItem::PushMask {
                bounds: bounds_slot,
                layers: layers_slot,
            }) = self.list.get_mut(at)
            {
                *bounds_slot = extent;
                *layers_slot = Arc::from(layers);
            }
            self.list.push(DisplayItem::PopMask);
        }
        if group.opacity_item.is_some() && !group.hoisted {
            self.list.push(DisplayItem::PopOpacity);
        }
    }

    /// Starts the opacity group of a box (with its `mix-blend-mode`) and
    /// returns the index of its item.
    pub(super) fn push_opacity(&mut self, b: &BoxFragment) -> usize {
        self.list.push(DisplayItem::opacity_group(
            b.style.opacity.max(0.0),
            b.style.mix_blend_mode,
        ));
        self.list.len() - 1
    }
}
