//! The resize handle (ADR 0043) of a text area, and of a scroll container
//! with `resize`: two diagonal strokes in the corner of the padding box.
//! It is part of the page, so screenshots show it (the overlay scroll
//! indicators of `scroll_indicator.rs` are drawn by the GUI only).

use swb_layout::{BoxContent, BoxFragment, ControlKind, Rect};
use swb_style::{Direction, Overflow, Resize, Rgba, Visibility};

use crate::display_list::DisplayItem;

/// The dark stroke of a resize handle.
const HANDLE_DARK: Rgba = Rgba::new(0, 0, 0, 153);

/// The light stroke of a resize handle.
const HANDLE_LIGHT: Rgba = Rgba::new(255, 255, 255, 153);

/// The resize handle of box `b` (border box `rect` in the coordinates of
/// the display list), the two diagonal strokes in the corner of its padding
/// box: the bottom right, or the bottom left with `direction: rtl`. A text
/// area has one, and a scroll container with `overflow` other than
/// `visible` and `clip`, if `resize` is not `none` (a form control other
/// than a text area has none). The strokes are 1 px wide: a dark one (black
/// with alpha 0.6) and a light one below it (white with alpha 0.6), at the
/// positions that Chromium 148 draws (`tools/probes/textarea-resizer.json`).
/// swb does not resize the box.
pub(crate) fn resize_handle(b: &BoxFragment, rect: Rect) -> Vec<DisplayItem> {
    let text_area = matches!(
        &b.content,
        BoxContent::Control(c) if matches!(c.kind, ControlKind::TextArea { .. })
    );
    let other_control = matches!(&b.content, BoxContent::Control(_)) && !text_area;
    let scrolls = b.scroll_node().is_some_and(|_| {
        !matches!(b.style.overflow_x, Overflow::Visible | Overflow::Clip)
            || !matches!(b.style.overflow_y, Overflow::Visible | Overflow::Clip)
    });
    if b.style.resize == Resize::None
        || b.style.visibility != Visibility::Visible
        || other_control
        || !(text_area || scrolls)
    {
        return Vec::new();
    }
    let padding = rect.inset(&b.border);
    let right = padding.right().round();
    let left = padding.x.round();
    let bottom = padding.bottom().round();
    let rtl = b.style.direction == Direction::Rtl;
    // The column of the stroke pixel `k` pixels from the corner edge.
    let column = |k: f32| if rtl { left + 1.0 + k } else { right - 2.0 - k };
    let mut items = Vec::new();
    let mut pixel = |x: f32, y: f32, color: Rgba| {
        items.push(DisplayItem::Rect {
            rect: Rect::new(x, y, 1.0, 1.0),
            radii: [(0.0, 0.0); 4],
            color,
        });
    };
    // The long stroke is 7 px, the short one 3 px; the light stroke is
    // the dark one a row lower, without the row below the padding edge
    // minus 1.
    for (first_row, length) in [(8.0, 7), (4.0, 3)] {
        for k in 0..length {
            let (x, y) = (column(k as f32), bottom - first_row + k as f32);
            pixel(x, y, HANDLE_DARK);
            if k + 1 < length {
                pixel(x, y + 1.0, HANDLE_LIGHT);
            }
        }
    }
    items
}
