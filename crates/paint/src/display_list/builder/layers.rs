//! The groups around the layers of a background (ADR 0046): the rounded
//! clip of the painting area (CSS Backgrounds 3 §4.2: each layer is
//! clipped by the rounded clip box) and, inside it, the mask of the text
//! for `background-clip: text` (`text_clip.rs`).
//!
//! The builder makes one [`Piece`] per painted layer (the color and the
//! image layers, bottom first). Consecutive pieces with the same clip
//! groups share them, so that the edge of the rounded shape is anti-aliased
//! once (the color under an image does not show at the curve). A rounded
//! clip group needs a layer of the size of its content, so the content is
//! split at the rows that hold the corners: the part between them is drawn
//! without a group, the parts at the top and bottom each in a group.
//! (A group around a hero image of 1,180 x 570 px took 2.5 ms of 29 ms
//! per frame of videojs.org.)

use std::sync::Arc;

use swb_layout::Rect;
use swb_style::{BackgroundBox, CompositeOperator};

use super::{Builder, DisplayItem, MaskLayer, Radii, rounded_clip};
use crate::mask::MaskLayerImage;

use super::text_clip::TextMask;

/// One painted layer of a background.
pub(super) struct Piece {
    /// The `background-clip` of the layer: `text` selects the mask of the
    /// text, when the box has one.
    pub(super) clip: BackgroundBox,
    /// The rounded painting area and its radii, if the clip box has a
    /// rounded corner: the item is clipped by it.
    pub(super) round: Option<(Rect, Radii)>,
    /// What paints the layer.
    pub(super) item: DisplayItem,
}

impl Builder<'_> {
    /// Paints the pieces of a background in order and empties `pieces` (the
    /// items move into the list). A piece whose clip is
    /// `text` is clipped by `text` (the mask of the box's text; painted as
    /// `border-box` if there is none) and paints nothing if the box has no
    /// text. `area` is the border box and `kinds` the `corner-shape`
    /// kinds of the box.
    pub(super) fn emit_pieces(
        &mut self,
        pieces: &mut Vec<Piece>,
        text: Option<&TextMask>,
        area: Rect,
        kinds: Option<[f32; 4]>,
    ) {
        let masked = |piece: &Piece| text.filter(|_| piece.clip == BackgroundBox::Text);
        while let Some(first) = pieces.first() {
            let round = first.round;
            let first_mask = masked(first);
            let len = pieces
                .iter()
                .take_while(|p| p.round == round && masked(p).is_some() == first_mask.is_some())
                .count();
            self.emit_run(pieces.drain(..len), round, first_mask, area, kinds);
        }
    }

    /// Paints the items of consecutive pieces that share their groups.
    fn emit_run(
        &mut self,
        items: std::vec::Drain<'_, Piece>,
        round: Option<(Rect, Radii)>,
        text: Option<&TextMask>,
        area: Rect,
        kinds: Option<[f32; 4]>,
    ) {
        // The mask covers the ink of the text inside the painting area.
        let extent = match text {
            Some(mask) => match mask.ink.intersection(&area) {
                Some(e) if e.width > 0.0 && e.height > 0.0 => Some(e),
                _ => return,
            },
            None => None,
        };
        let Some((rect, radii)) = round else {
            self.emit_whole(items, text, extent);
            return;
        };
        // The fills snap their rectangles to device pixels; the group does
        // too, or its first and last row would blend at a fractional
        // position (and the bands would not meet the middle).
        let rect = snap_to_pixels(rect, self.scale);
        let group = || {
            kinds
                .and_then(|k| crate::corner_shape::clip(rect, radii, k))
                .or_else(|| rounded_clip(rect, &radii))
        };
        // The rows that hold the corners.
        let top = radii[0].1.max(radii[1].1).ceil();
        let bottom = radii[2].1.max(radii[3].1).ceil();
        if text.is_some() || kinds.is_some() || top + bottom >= rect.height {
            if let Some(group) = group() {
                self.list.push(group);
                self.emit_whole(items, text, extent);
                self.list.push(DisplayItem::PopSvgClip);
            } else {
                self.emit_whole(items, text, extent);
            }
            return;
        }
        let items: Vec<Piece> = items.collect();
        let middle = Rect::new(rect.x, rect.y + top, rect.width, rect.height - top - bottom);
        let bands = [
            Rect::new(rect.x, rect.y, rect.width, top),
            Rect::new(rect.x, rect.bottom() - bottom, rect.width, bottom),
        ];
        self.emit_part(&items, middle);
        for band in bands {
            let Some(group) = group() else {
                self.emit_part(&items, band);
                continue;
            };
            self.list.push(group);
            self.emit_part(&items, band);
            self.list.push(DisplayItem::PopSvgClip);
        }
    }

    /// Paints the items inside the mask group of the text (if `text`).
    fn emit_whole(
        &mut self,
        items: impl Iterator<Item = Piece>,
        text: Option<&TextMask>,
        extent: Option<Rect>,
    ) {
        if let (Some(mask), Some(extent)) = (text, extent) {
            self.list.push(DisplayItem::PushMask {
                bounds: extent,
                layers: Arc::from([MaskLayer {
                    image: MaskLayerImage::Content {
                        items: Arc::clone(&mask.items),
                        area: extent,
                    },
                    luminance: false,
                    composite: CompositeOperator::SourceOver,
                }]),
            });
        }
        self.list.extend(items.map(|piece| piece.item));
        if text.is_some() && extent.is_some() {
            self.list.push(DisplayItem::PopMask);
        }
    }

    /// Paints the items clipped to `only`.
    fn emit_part(&mut self, items: &[Piece], only: Rect) {
        for Piece { item, .. } in items {
            if let Some(clipped) = clipped_to(item, &only) {
                self.list.push(clipped);
            }
        }
    }
}

/// `rect` with its edges rounded to whole device pixels (as the
/// rasterizer does for a fill), at least one pixel wide and high if it is
/// not empty. `scale` is the device pixels per CSS px.
fn snap_to_pixels(rect: Rect, scale: f32) -> Rect {
    if !(scale > 0.0 && scale.is_finite()) {
        return rect;
    }
    let axis = |start: f32, size: f32| {
        let a = (start * scale).round();
        let mut b = ((start + size) * scale).round();
        if size > 0.0 && b <= a {
            b = a + 1.0;
        }
        (a / scale, (b - a).max(0.0) / scale)
    };
    let (x, width) = axis(rect.x, rect.width);
    let (y, height) = axis(rect.y, rect.height);
    Rect::new(x, y, width, height)
}

/// `item` painted only inside `only`: a copy with its area or clip
/// reduced; `None` if nothing is left.
fn clipped_to(item: &DisplayItem, only: &Rect) -> Option<DisplayItem> {
    Some(match item {
        DisplayItem::Rect { rect, radii, color } => DisplayItem::Rect {
            rect: rect.intersection(only)?,
            radii: *radii,
            color: *color,
        },
        DisplayItem::Image {
            image,
            rect,
            tile,
            clip,
        } => DisplayItem::Image {
            image: image.clone(),
            rect: *rect,
            tile: *tile,
            clip: clip.intersection(only)?,
        },
        DisplayItem::LinearGradient {
            rect,
            tile,
            clip,
            gradient,
            current_color,
        } => DisplayItem::LinearGradient {
            rect: *rect,
            tile: *tile,
            clip: clip.intersection(only)?,
            gradient: Arc::clone(gradient),
            current_color: *current_color,
        },
        other => other.clone(),
    })
}
