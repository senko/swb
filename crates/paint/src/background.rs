//! Background image positioning and tiling (CSS Backgrounds 3 §2.6–2.9):
//! <https://www.w3.org/TR/css-backgrounds-3/#background-position> to
//! <https://www.w3.org/TR/css-backgrounds-3/#background-size>.

use swb_layout::{NaturalSize, Rect};
use swb_style::{BackgroundRepeatKeyword, BackgroundSize, Length, PositionComponent};

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
