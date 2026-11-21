//! The areas of a box's background (`BackgroundAreas`: the painting area,
//! the color area and its corner radii) and the positioning and tiling of
//! background images (CSS Backgrounds 3 §2.6–2.9):
//! <https://www.w3.org/TR/css-backgrounds-3/#background-position> to
//! <https://www.w3.org/TR/css-backgrounds-3/#background-size>; and the
//! same sizing and positioning for the image of a replaced element
//! (`object-fit` and `object-position`, CSS Images 3 §5.5–5.6:
//! <https://www.w3.org/TR/css-images-3/#the-object-fit>).

use swb_layout::{BoxFragment, NaturalSize, Point, Rect, has_rounded_corner};
use swb_style::{
    BackgroundBox, BackgroundRepeatKeyword, BackgroundSize, BorderStyle, ComputedStyle, Length,
    ObjectFit, PositionComponent, Rgba,
};

use crate::display_list::Radii;

/// The areas that a background uses, in document coordinates.
pub(crate) struct BackgroundAreas {
    pub(crate) border: Rect,
    pub(crate) padding: Rect,
    /// The content box, for `background-clip: content-box` (a table and
    /// the parts of a table use the padding box).
    pub(crate) content: Rect,
    pub(crate) painting: PaintingArea,
}

/// Where the layers of a background are painted.
#[derive(Clone, Copy)]
pub(crate) enum PaintingArea {
    /// The box's own area that `background-clip` selects.
    Own,
    /// The canvas, for the root background. A gradient fills all of it.
    Canvas(Rect),
    /// A table cell, for the background of a row, row group, column or
    /// column group: the layers are positioned in the part's areas and
    /// painted in the cell (CSS 2.2 §17.5.1).
    Cell(Rect),
}

impl BackgroundAreas {
    pub(crate) fn of(b: &BoxFragment, origin: Point) -> Self {
        BackgroundAreas {
            border: b.border_rect.translate(origin),
            padding: b.padding_rect().translate(origin),
            content: b.content_rect().translate(origin),
            painting: PaintingArea::Own,
        }
    }

    /// Areas whose content box is the padding box: the canvas, a table and
    /// the parts of a table (they have no `background-clip: content-box`).
    pub(crate) fn flat(border: Rect, padding: Rect, painting: PaintingArea) -> Self {
        BackgroundAreas {
            border,
            padding,
            content: padding,
            painting,
        }
    }

    /// The positioning area of `background-origin`. `content-box` is
    /// painted as `padding-box` (not supported yet).
    pub(crate) fn area(&self, which: BackgroundBox) -> Rect {
        match which {
            BackgroundBox::BorderBox | BackgroundBox::Text => self.border,
            _ => self.padding,
        }
    }

    /// The area that `background-clip: which` selects. `text` is the border
    /// box, which the text of the box clips further (ADR 0046).
    fn clip_area(&self, which: BackgroundBox) -> Rect {
        match which {
            BackgroundBox::BorderBox | BackgroundBox::Text => self.border,
            BackgroundBox::PaddingBox => self.padding,
            BackgroundBox::ContentBox => self.content,
        }
    }

    /// The painting area that `clip` selects with its corner radii: the
    /// border radii of the box inside the border and padding (CSS
    /// Backgrounds 3 §4.2, the inner radii), also for a box with no radii.
    pub(crate) fn clip_shape(&self, clip: BackgroundBox, radii: Radii) -> (Rect, Radii) {
        let area = self.clip_area(clip);
        (area, inner_radii(radii, self.border, area))
    }

    /// The area and corner radii that the background color fills. A rounded
    /// box whose four borders are solid and opaque keeps the color out of
    /// the border area (Chromium shrinks the background so that its
    /// anti-aliased edge does not show around the border): the padding box
    /// with the inner radii.
    pub(crate) fn color_area(
        &self,
        style: &ComputedStyle,
        clip: BackgroundBox,
        radii: Radii,
    ) -> (Rect, Radii) {
        let border = self.border;
        let padding = self.padding;
        let opaque = |kind: BorderStyle, color: Rgba| kind == BorderStyle::Solid && color.a == 255;
        let sides = [
            (style.border_top_style, style.border_top_color),
            (style.border_right_style, style.border_right_color),
            (style.border_bottom_style, style.border_bottom_color),
            (style.border_left_style, style.border_left_color),
        ];
        let borders_hide_the_edge = clip == BackgroundBox::BorderBox
            && has_rounded_corner(&radii)
            && padding.x > border.x
            && padding.y > border.y
            && padding.right() < border.right()
            && padding.bottom() < border.bottom()
            && sides
                .iter()
                .all(|&(kind, color)| opaque(kind, color.resolve(style.color)));
        if !borders_hide_the_edge {
            return match self.painting {
                PaintingArea::Own => self.clip_shape(clip, radii),
                _ => (self.painting_area(clip), radii),
            };
        }
        (padding, inner_radii(radii, border, padding))
    }

    pub(crate) fn painting_area(&self, clip: BackgroundBox) -> Rect {
        match self.painting {
            PaintingArea::Own => self.clip_area(clip),
            PaintingArea::Canvas(area) | PaintingArea::Cell(area) => area,
        }
    }
}

/// The corner radii of the box `inner` inside the box `outer` whose corner
/// radii are `radii`: each radius shrinks by the distance of the edges,
/// at least to 0 (CSS Backgrounds 3 §4.2).
pub(crate) fn inner_radii(radii: Radii, outer: Rect, inner: Rect) -> Radii {
    let (top, left) = (inner.y - outer.y, inner.x - outer.x);
    let (right, bottom) = (
        outer.right() - inner.right(),
        outer.bottom() - inner.bottom(),
    );
    [
        ((radii[0].0 - left).max(0.0), (radii[0].1 - top).max(0.0)),
        ((radii[1].0 - right).max(0.0), (radii[1].1 - top).max(0.0)),
        (
            (radii[2].0 - right).max(0.0),
            (radii[2].1 - bottom).max(0.0),
        ),
        ((radii[3].0 - left).max(0.0), (radii[3].1 - bottom).max(0.0)),
    ]
}

/// The properties of one background layer that affect tiling.
pub(crate) struct Layer<'a> {
    pub(crate) size: &'a BackgroundSize,
    pub(crate) position_x: &'a PositionComponent,
    pub(crate) position_y: &'a PositionComponent,
    pub(crate) repeat: (BackgroundRepeatKeyword, BackgroundRepeatKeyword),
}

/// Computes the first tile and the area the tiles cover. `positioning` is
/// the background positioning area, `clip` the painting area, `natural`
/// the image's natural dimensions.
pub(crate) fn tile(
    layer: &Layer<'_>,
    positioning: Rect,
    clip: Rect,
    natural: &NaturalSize,
) -> (Rect, Rect) {
    let (w, h) = tile_size(layer.size, positioning, natural);
    if w <= 0.0 || h <= 0.0 {
        return (Rect::default(), Rect::default());
    }
    let x = positioning.x + offset(layer.position_x, positioning.width - w);
    let y = positioning.y + offset(layer.position_y, positioning.height - h);
    let tile = Rect::new(x, y, w, h);
    // `space` and `round` are painted as `repeat` (not supported yet).
    let repeat_x = !matches!(layer.repeat.0, BackgroundRepeatKeyword::NoRepeat);
    let repeat_y = !matches!(layer.repeat.1, BackgroundRepeatKeyword::NoRepeat);
    let area = Rect::new(
        if repeat_x { clip.x } else { x },
        if repeat_y { clip.y } else { y },
        if repeat_x { clip.width } else { w },
        if repeat_y { clip.height } else { h },
    );
    (tile, area.intersection(&clip).unwrap_or_default())
}

/// The tiling of a gradient layer: the first tile, the area that the tiles
/// cover (within the painting area) and whether the painting area lies
/// within that one tile (no copy is visible).
pub(crate) struct GradientTiling {
    pub(crate) tile: Rect,
    pub(crate) area: Rect,
    pub(crate) single: bool,
}

/// The tiling of a gradient layer: [`tile`] with the size of a gradient
/// (it has no natural size, so `auto` is the positioning area), and
/// whether the painting area `clip` needs more than one tile. The
/// rasterizer repeats the tile over `area`, so the cost does not depend on
/// the number of tiles. `None` for an empty tile.
pub(crate) fn gradient_tiling(
    layer: &Layer<'_>,
    positioning: Rect,
    clip: Rect,
) -> Option<GradientTiling> {
    let (first, area) = tile(layer, positioning, clip, &NaturalSize::default());
    if first.width <= 0.0 || first.height <= 0.0 {
        return None;
    }
    let repeat_x = !matches!(layer.repeat.0, BackgroundRepeatKeyword::NoRepeat);
    let repeat_y = !matches!(layer.repeat.1, BackgroundRepeatKeyword::NoRepeat);
    // The copies of the tile that the clip needs along one axis: from the
    // first tile at or before the start of the clip.
    let count = |first: f32, size: f32, from: f32, length: f32, repeat: bool| -> f32 {
        if !repeat {
            return 1.0;
        }
        let back = ((first - from) / size).ceil().max(0.0);
        ((from + length - (first - back * size)) / size)
            .ceil()
            .max(1.0)
    };
    let nx = count(first.x, first.width, clip.x, clip.width, repeat_x);
    let ny = count(first.y, first.height, clip.y, clip.height, repeat_y);
    Some(GradientTiling {
        tile: first,
        area,
        single: nx <= 1.0 && ny <= 1.0,
    })
}

/// The area of the image of a replaced element whose content box is
/// `content`: the concrete object size from `fit` (with the content box as
/// the default object size) at `position` (x and y).
pub(crate) fn object_rect(
    fit: ObjectFit,
    position: &[PositionComponent; 2],
    content: Rect,
    natural: &NaturalSize,
) -> Rect {
    let sized = |size: BackgroundSize| tile_size(&size, content, natural);
    let (w, h) = match fit {
        ObjectFit::Fill => (content.width, content.height),
        ObjectFit::Contain => sized(BackgroundSize::Contain),
        ObjectFit::Cover => sized(BackgroundSize::Cover),
        ObjectFit::None => sized(BackgroundSize::Auto),
        // The smaller of `none` and `contain`.
        ObjectFit::ScaleDown => {
            let none = sized(BackgroundSize::Auto);
            let contain = sized(BackgroundSize::Contain);
            if none.0 <= contain.0 && none.1 <= contain.1 {
                none
            } else {
                contain
            }
        }
    };
    let [x, y] = position;
    Rect::new(
        content.x + offset(x, content.width - w),
        content.y + offset(y, content.height - h),
        w,
        h,
    )
}

fn offset(p: &PositionComponent, free: f32) -> f32 {
    let v = p.offset.resolve(free);
    if p.from_end { free - v } else { v }
}

/// The size of one tile (CSS Backgrounds 3 §3.9). A dimension that is
/// `auto` comes from the other dimension and the natural ratio, or else
/// from the natural dimension, or else from the positioning area (the
/// default object size of backgrounds, CSS Images 3 §5.2:
/// <https://www.w3.org/TR/css-images-3/#default-sizing>). The ratio and
/// the result are limited to layout lengths, so extreme ratios stay
/// finite.
fn tile_size(size: &BackgroundSize, area: Rect, natural: &NaturalSize) -> (f32, f32) {
    let (w, h) = unclamped_tile_size(size, area, natural);
    (Length::clamp_px(w), Length::clamp_px(h))
}

fn unclamped_tile_size(size: &BackgroundSize, area: Rect, natural: &NaturalSize) -> (f32, f32) {
    let max = Length::MAX_PX;
    let ratio = natural
        .ratio
        .filter(|r| r.is_finite() && *r > 0.0)
        .map(|r| r.max(1.0 / max).min(max));
    let fit = |cover: bool| -> (f32, f32) {
        let Some(r) = ratio else {
            return (area.width, area.height);
        };
        // Full width, unless the height then is too large (contain) or too
        // small (cover).
        let height = area.width / r;
        if (height > area.height) == cover {
            (area.width, height)
        } else {
            (area.height * r, area.height)
        }
    };
    let (w, h) = match size {
        BackgroundSize::Cover => return fit(true),
        BackgroundSize::Contain => return fit(false),
        BackgroundSize::Auto => (None, None),
        BackgroundSize::Explicit(w, h) => (w.resolve(area.width), h.resolve(area.height)),
    };
    let width_for = |h: f32| ratio.map_or_else(|| natural.width.unwrap_or(area.width), |r| h * r);
    let height_for =
        |w: f32| ratio.map_or_else(|| natural.height.unwrap_or(area.height), |r| w / r);
    match (w, h) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, height_for(w)),
        (None, Some(h)) => (width_for(h), h),
        (None, None) => match (natural.width, natural.height) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, height_for(w)),
            (None, Some(h)) => (width_for(h), h),
            (None, None) => fit(false),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_style::{LengthPercentage, LengthPercentageOrAuto};

    /// A box of 100 x 100 with a border of 10 and padding of 12.
    fn areas() -> BackgroundAreas {
        BackgroundAreas {
            border: Rect::new(0.0, 0.0, 100.0, 100.0),
            padding: Rect::new(10.0, 10.0, 80.0, 80.0),
            content: Rect::new(22.0, 22.0, 56.0, 56.0),
            painting: PaintingArea::Own,
        }
    }

    /// The clip boxes have the border radii shrunk by the border and the
    /// padding (CSS Backgrounds 3 §4.2); `text` is the border box.
    #[test]
    fn clip_boxes_have_inner_radii() {
        let radii = [(40.0, 40.0), (40.0, 20.0), (5.0, 5.0), (0.0, 0.0)];
        let a = areas();
        assert_eq!(
            a.clip_shape(BackgroundBox::BorderBox, radii),
            (a.border, radii)
        );
        assert_eq!(a.clip_shape(BackgroundBox::Text, radii), (a.border, radii));
        let (rect, padding) = a.clip_shape(BackgroundBox::PaddingBox, radii);
        assert_eq!(rect, a.padding);
        assert_eq!(
            padding,
            [(30.0, 30.0), (30.0, 10.0), (0.0, 0.0), (0.0, 0.0)]
        );
        let (rect, content) = a.clip_shape(BackgroundBox::ContentBox, radii);
        assert_eq!(rect, a.content);
        assert_eq!(content, [(18.0, 18.0), (18.0, 0.0), (0.0, 0.0), (0.0, 0.0)]);
        // The origin of `content-box` is still the padding box.
        assert_eq!(a.area(BackgroundBox::ContentBox), a.padding);
        assert_eq!(a.painting_area(BackgroundBox::ContentBox), a.content);
    }

    #[test]
    fn gradient_tiling_counts_copies_without_listing_them() {
        let zero = PositionComponent {
            offset: LengthPercentage::Percent(0.0),
            from_end: false,
        };
        let area = Rect::new(0.0, 0.0, 100.0, 100.0);
        let size = BackgroundSize::Explicit(px(40.0), px(40.0));
        let repeat = (
            BackgroundRepeatKeyword::Repeat,
            BackgroundRepeatKeyword::Repeat,
        );
        let layer = Layer {
            size: &size,
            position_x: &zero,
            position_y: &zero,
            repeat,
        };
        let t = gradient_tiling(&layer, area, area).expect("a tile");
        assert_eq!(t.tile, Rect::new(0.0, 0.0, 40.0, 40.0));
        assert_eq!(t.area, area);
        assert!(!t.single);
        // One tile covers the area: no copy shows.
        let big = BackgroundSize::Explicit(px(100.0), px(100.0));
        let layer = Layer {
            size: &big,
            position_x: &zero,
            position_y: &zero,
            repeat,
        };
        assert!(gradient_tiling(&layer, area, area).is_some_and(|t| t.single));
        // A tile of 1 px in a huge area is one tiling, not 10^10 items.
        let tiny = BackgroundSize::Explicit(px(1.0), px(1.0));
        let layer = Layer {
            size: &tiny,
            position_x: &zero,
            position_y: &zero,
            repeat,
        };
        let huge = Rect::new(0.0, 0.0, 100_000.0, 100_000.0);
        assert!(gradient_tiling(&layer, huge, huge).is_some_and(|t| !t.single && t.area == huge));
        // No repeat in y: the area is one row of tiles.
        let layer = Layer {
            size: &size,
            position_x: &zero,
            position_y: &zero,
            repeat: (
                BackgroundRepeatKeyword::Repeat,
                BackgroundRepeatKeyword::NoRepeat,
            ),
        };
        let t = gradient_tiling(&layer, area, area).expect("a tile");
        assert_eq!(t.area, Rect::new(0.0, 0.0, 100.0, 40.0));
    }

    #[test]
    fn centered_no_repeat() {
        let center = PositionComponent {
            offset: LengthPercentage::Percent(0.5),
            from_end: false,
        };
        let layer = Layer {
            size: &BackgroundSize::Auto,
            position_x: &center,
            position_y: &center,
            repeat: (
                BackgroundRepeatKeyword::NoRepeat,
                BackgroundRepeatKeyword::NoRepeat,
            ),
        };
        let area = Rect::new(0.0, 0.0, 100.0, 50.0);
        let (tile, painted) = tile(&layer, area, area, &NaturalSize::fixed(20.0, 10.0));
        assert_eq!(tile, Rect::new(40.0, 20.0, 20.0, 10.0));
        assert_eq!(painted, tile);
    }

    #[test]
    fn contain_and_cover_keep_the_ratio() {
        let area = Rect::new(0.0, 0.0, 100.0, 50.0);
        let square = NaturalSize::fixed(200.0, 200.0);
        assert_eq!(
            tile_size(&BackgroundSize::Contain, area, &square),
            (50.0, 50.0)
        );
        assert_eq!(
            tile_size(&BackgroundSize::Cover, area, &square),
            (100.0, 100.0)
        );
    }

    fn px(v: f32) -> LengthPercentageOrAuto {
        LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Px(v))
    }

    fn percent(v: f32) -> LengthPercentageOrAuto {
        LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Percent(v))
    }

    /// Tile sizes of SVG images that lack natural dimensions, in a 200x100
    /// area, measured with Chromium 148.
    #[test]
    fn missing_dimensions_follow_chromium() {
        use LengthPercentageOrAuto::Auto;
        let area = Rect::new(0.0, 0.0, 200.0, 100.0);
        let only_ratio = NaturalSize {
            width: None,
            height: None,
            ratio: Some(2.0),
        };
        let only_width = NaturalSize {
            width: Some(60.0),
            height: None,
            ratio: None,
        };
        let only_height = NaturalSize {
            width: None,
            height: Some(30.0),
            ratio: None,
        };
        let nothing = NaturalSize::default();
        let width_and_ratio = NaturalSize {
            width: Some(60.0),
            height: None,
            ratio: Some(2.0),
        };
        let sizes = [
            BackgroundSize::Auto,
            BackgroundSize::Explicit(px(10.0), Auto),
            BackgroundSize::Explicit(Auto, px(10.0)),
            BackgroundSize::Contain,
            BackgroundSize::Explicit(percent(0.5), Auto),
            BackgroundSize::Explicit(Auto, percent(0.5)),
        ];
        let expected = [
            (
                only_ratio,
                [
                    (200, 100),
                    (10, 5),
                    (20, 10),
                    (200, 100),
                    (100, 50),
                    (100, 50),
                ],
            ),
            (
                only_width,
                [
                    (60, 100),
                    (10, 100),
                    (60, 10),
                    (200, 100),
                    (100, 100),
                    (60, 50),
                ],
            ),
            (
                only_height,
                [
                    (200, 30),
                    (10, 30),
                    (200, 10),
                    (200, 100),
                    (100, 30),
                    (200, 50),
                ],
            ),
            (
                nothing,
                [
                    (200, 100),
                    (10, 100),
                    (200, 10),
                    (200, 100),
                    (100, 100),
                    (200, 50),
                ],
            ),
            (
                width_and_ratio,
                [
                    (60, 30),
                    (10, 5),
                    (20, 10),
                    (200, 100),
                    (100, 50),
                    (100, 50),
                ],
            ),
        ];
        for (natural, tiles) in expected {
            for (size, (w, h)) in sizes.iter().zip(tiles) {
                assert_eq!(
                    tile_size(size, area, &natural),
                    (w as f32, h as f32),
                    "{natural:?} {size:?}"
                );
            }
        }
    }

    /// A 100×50 and a 400×100 image in a 200×120 content box at (10, 20),
    /// measured with Chromium 148 (`<img>` and video posters).
    #[test]
    fn object_fit_and_position_follow_chromium() {
        let content = Rect::new(10.0, 20.0, 200.0, 120.0);
        let small = NaturalSize::fixed(100.0, 50.0);
        let wide = NaturalSize::fixed(400.0, 100.0);
        let center = [PositionComponent::CENTER, PositionComponent::CENTER];
        let at = |fit, natural: &NaturalSize, position: &[PositionComponent; 2]| {
            let r = object_rect(fit, position, content, natural);
            (r.x - 10.0, r.y - 20.0, r.width, r.height)
        };
        assert_eq!(
            at(ObjectFit::Fill, &small, &center),
            (0.0, 0.0, 200.0, 120.0)
        );
        assert_eq!(
            at(ObjectFit::Contain, &small, &center),
            (0.0, 10.0, 200.0, 100.0)
        );
        assert_eq!(
            at(ObjectFit::Cover, &small, &center),
            (-20.0, 0.0, 240.0, 120.0)
        );
        assert_eq!(
            at(ObjectFit::None, &small, &center),
            (50.0, 35.0, 100.0, 50.0)
        );
        assert_eq!(
            at(ObjectFit::ScaleDown, &small, &center),
            (50.0, 35.0, 100.0, 50.0)
        );
        assert_eq!(
            at(ObjectFit::ScaleDown, &wide, &center),
            (0.0, 35.0, 200.0, 50.0)
        );
        let percent = |v: f32| PositionComponent {
            offset: LengthPercentage::Percent(v),
            from_end: false,
        };
        assert_eq!(
            at(ObjectFit::None, &small, &[percent(0.25), percent(0.75)]),
            (25.0, 52.5, 100.0, 50.0)
        );
        let right_10px = PositionComponent {
            offset: LengthPercentage::Px(10.0),
            from_end: true,
        };
        assert_eq!(
            at(ObjectFit::None, &small, &[right_10px, percent(0.0)]),
            (90.0, 0.0, 100.0, 50.0)
        );
        // Without natural dimensions, `none` uses the content box.
        assert_eq!(
            at(ObjectFit::None, &NaturalSize::default(), &center),
            (0.0, 0.0, 200.0, 120.0)
        );
    }

    #[test]
    fn extreme_ratios_give_finite_tiles() {
        let area = Rect::new(0.0, 0.0, 200.0, 100.0);
        for ratio in [1e-37, 1e37, f32::MIN_POSITIVE] {
            let natural = NaturalSize {
                width: Some(100.0),
                height: None,
                ratio: Some(ratio),
            };
            for size in [
                BackgroundSize::Auto,
                BackgroundSize::Contain,
                BackgroundSize::Cover,
                BackgroundSize::Explicit(LengthPercentageOrAuto::Auto, px(100.0)),
            ] {
                let (w, h) = tile_size(&size, area, &natural);
                assert!(w.is_finite() && h.is_finite(), "{ratio} {size:?}: {w} {h}");
            }
        }
    }
}
