//! Overlay scroll indicators: a thin thumb along the right edge (vertical
//! scrolling) and the bottom edge (horizontal scrolling) of a scrollport
//! that the user can scroll. Its length shows the visible part of the
//! content and its position the scroll offset. The indicators take no
//! layout space and cannot be dragged.
//!
//! Only the GUI draws them (`Scrolling::indicators`): Chromium's headless
//! shell, which makes the reference screenshots, hides scrollbars. Scroll
//! containers get them in the display list (inside their clip, above
//! their content); the viewport gets the same indicator drawn over the
//! page by the engine (ADR 0019).

use swb_layout::{BoxFragment, Point, Rect, ScrollOffsets, Size, scroll_range};
use swb_style::{BorderStyle, Overflow, Rgba};

use crate::display_list::DisplayItem;

/// The thickness of a thumb in CSS px.
const THICKNESS: f32 = 5.0;

/// The distance of a thumb from the edges of the scrollport.
const MARGIN: f32 = 2.0;

/// The shortest thumb.
const MIN_LENGTH: f32 = 20.0;

/// The fill of a thumb: dark, so that it shows on light content.
const FILL: Rgba = Rgba::new(0, 0, 0, 115);

/// The outline of a thumb: light, so that it shows on dark content.
const OUTLINE: Rgba = Rgba::new(255, 255, 255, 150);

/// The indicators of a scrollport (`port`, in the coordinates of the
/// display list) whose content has the size `content` and is scrolled to
/// `offset`. `user_x` and `user_y` say on which axes the user can scroll
/// (not with `overflow: hidden`). Nothing is drawn on an axis without a
/// scroll range or where the port is too small.
pub fn scroll_indicators(
    port: Rect,
    content: Size,
    offset: Point,
    user_x: bool,
    user_y: bool,
) -> Vec<DisplayItem> {
    // The scroll range in whole pixels, as the engine scrolls.
    let max = scroll_range(content, Size::new(port.width, port.height));
    let vertical = user_y && max.y > 0.0;
    let horizontal = user_x && max.x > 0.0;
    // Both thumbs leave the corner free.
    let corner = THICKNESS + MARGIN;
    let mut items = Vec::new();
    if vertical {
        let track = port.height - 2.0 * MARGIN - if horizontal { corner } else { 0.0 };
        if let Some((start, length)) = thumb(track, port.height, content.height, offset.y, max.y) {
            let x = port.right() - MARGIN - THICKNESS;
            push_thumb(
                &mut items,
                Rect::new(x, port.y + MARGIN + start, THICKNESS, length),
            );
        }
    }
    if horizontal {
        let track = port.width - 2.0 * MARGIN - if vertical { corner } else { 0.0 };
        if let Some((start, length)) = thumb(track, port.width, content.width, offset.x, max.x) {
            let y = port.bottom() - MARGIN - THICKNESS;
            push_thumb(
                &mut items,
                Rect::new(port.x + MARGIN + start, y, length, THICKNESS),
            );
        }
    }
    items
}

/// The indicators of scroll container `b` (border box `rect` in the
/// coordinates of the display list). None for other boxes and for
/// pseudo-element boxes, which cannot be scrolled.
pub(crate) fn element_scroll_indicators(
    b: &BoxFragment,
    rect: Rect,
    offsets: &dyn ScrollOffsets,
) -> Vec<DisplayItem> {
    let (Some(overflow), Some(_)) = (b.scrollable_overflow, b.scroll_node()) else {
        return Vec::new();
    };
    scroll_indicators(
        rect.inset(&b.border),
        Size::new(overflow.width, overflow.height),
        b.scroll_offset(offsets),
        b.style.overflow_x != Overflow::Hidden,
        b.style.overflow_y != Overflow::Hidden,
    )
}

/// The start and length of a thumb in a track of `track` px, for a view of
/// `view` px of `content` px scrolled to `offset` (of at most `max`).
/// `None` if the track is too short for a thumb.
fn thumb(track: f32, view: f32, content: f32, offset: f32, max: f32) -> Option<(f32, f32)> {
    // False also for NaN.
    let usable = track >= 2.0 * THICKNESS && content > 0.0 && max > 0.0;
    if !usable {
        return None;
    }
    let length = (track * view / content).clamp(MIN_LENGTH.min(track), track);
    let start = (track - length) * (offset / max).clamp(0.0, 1.0);
    Some((start, length))
}

fn push_thumb(items: &mut Vec<DisplayItem>, rect: Rect) {
    let radius = (THICKNESS / 2.0, THICKNESS / 2.0);
    items.push(DisplayItem::Rect {
        rect,
        radii: [radius; 4],
        color: FILL,
    });
    items.push(DisplayItem::Border {
        rect,
        widths: [1.0; 4],
        colors: [OUTLINE; 4],
        styles: [BorderStyle::Solid; 4],
        radii: [radius; 4],
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rects(items: &[DisplayItem]) -> Vec<Rect> {
        items
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Rect { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn thumb_length_and_position_follow_the_scroll_offset() {
        let port = Rect::new(0.0, 0.0, 100.0, 104.0);
        let content = Size::new(100.0, 400.0);
        let top = rects(&scroll_indicators(
            port,
            content,
            Point::default(),
            true,
            true,
        ));
        // The track is 100 px; the view shows 104 of 400 px.
        assert_eq!(top, vec![Rect::new(93.0, 2.0, 5.0, 26.0)]);
        let bottom = rects(&scroll_indicators(
            port,
            content,
            Point::new(0.0, 296.0),
            true,
            true,
        ));
        assert_eq!(bottom, vec![Rect::new(93.0, 76.0, 5.0, 26.0)]);
    }

    #[test]
    fn no_indicator_without_a_user_scroll_range() {
        let port = Rect::new(0.0, 0.0, 100.0, 100.0);
        let tall = Size::new(100.0, 400.0);
        assert!(scroll_indicators(port, tall, Point::default(), true, false).is_empty());
        assert!(scroll_indicators(port, port_size(port), Point::default(), true, true).is_empty());
        // 0.4 px more than the port: no range in whole pixels.
        let almost = Size::new(100.0, 100.4);
        assert!(scroll_indicators(port, almost, Point::default(), true, true).is_empty());
        // Too small for a thumb.
        let tiny = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(scroll_indicators(tiny, tall, Point::default(), true, true).is_empty());
        // Non-finite sizes draw nothing.
        let nan = Size::new(f32::NAN, f32::NAN);
        assert!(scroll_indicators(port, nan, Point::default(), true, true).is_empty());
    }

    #[test]
    fn two_thumbs_leave_the_corner_free() {
        let port = Rect::new(0.0, 0.0, 100.0, 100.0);
        let items = scroll_indicators(port, Size::new(400.0, 400.0), Point::default(), true, true);
        let r = rects(&items);
        assert_eq!(r.len(), 2);
        assert!(r[0].bottom() <= 100.0 - THICKNESS - MARGIN);
        assert!(r[1].right() <= 100.0 - THICKNESS - MARGIN);
    }

    fn port_size(r: Rect) -> Size {
        Size::new(r.width, r.height)
    }
}
