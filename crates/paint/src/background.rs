//! Background image positioning and tiling (CSS Backgrounds 3 §3.6–3.9):
//! <https://www.w3.org/TR/css-backgrounds-3/#background-size>.

use swb_layout::Rect;
use swb_style::{BackgroundRepeatKeyword, BackgroundSize, PositionComponent};

/// The properties of one background layer that affect tiling.
pub(crate) struct Layer<'a> {
    pub(crate) size: &'a BackgroundSize,
    pub(crate) position_x: &'a PositionComponent,
    pub(crate) position_y: &'a PositionComponent,
    pub(crate) repeat: (BackgroundRepeatKeyword, BackgroundRepeatKeyword),
}

/// Computes the first tile and the area the tiles cover. `positioning` is
/// the background positioning area, `clip` the painting area, `natural`
/// the image's natural size.
pub(crate) fn tile(
    layer: &Layer<'_>,
    positioning: Rect,
    clip: Rect,
    natural: (f32, f32),
) -> (Rect, Rect) {
    let (w, h) = tile_size(layer.size, positioning, natural);
    if w <= 0.0 || h <= 0.0 {
        return (Rect::default(), Rect::default());
    }
    let x = positioning.x + offset(layer.position_x, positioning.width - w);
    let y = positioning.y + offset(layer.position_y, positioning.height - h);
    let tile = Rect::new(x, y, w, h);
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

fn tile_size(size: &BackgroundSize, area: Rect, natural: (f32, f32)) -> (f32, f32) {
    let (nw, nh) = natural;
    let ratio = if nh > 0.0 { nw / nh } else { 1.0 };
    match size {
        BackgroundSize::Auto => (nw, nh),
        BackgroundSize::Cover | BackgroundSize::Contain => {
            if nw <= 0.0 || nh <= 0.0 {
                return (area.width, area.height);
            }
            let sx = area.width / nw;
            let sy = area.height / nh;
            let s = if matches!(size, BackgroundSize::Cover) {
                sx.max(sy)
            } else {
                sx.min(sy)
            };
            (nw * s, nh * s)
        }
        BackgroundSize::Explicit(w, h) => {
            let w = w.resolve(area.width);
            let h = h.resolve(area.height);
            match (w, h) {
                (Some(w), Some(h)) => (w, h),
                (Some(w), None) => (w, w / ratio),
                (None, Some(h)) => (h * ratio, h),
                (None, None) => (nw, nh),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_style::LengthPercentage;

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
        let (tile, painted) = tile(&layer, area, area, (20.0, 10.0));
        assert_eq!(tile, Rect::new(40.0, 20.0, 20.0, 10.0));
        assert_eq!(painted, tile);
    }

    #[test]
    fn contain_scales_down() {
        let size = tile_size(
            &BackgroundSize::Contain,
            Rect::new(0.0, 0.0, 100.0, 50.0),
            (200.0, 200.0),
        );
        assert_eq!(size, (50.0, 50.0));
    }
}
