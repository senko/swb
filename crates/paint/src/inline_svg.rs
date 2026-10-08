//! Paints the content of an inline `<svg>` element (ADR 0023) into the
//! display list: path items with their transforms from user units to the
//! list's coordinates, and opacity groups.
//!
//! The content is clipped to the content box when the element's
//! `overflow` clips (the user-agent style sheet sets `overflow: hidden`).
//! Chromium clips at the content box, not the padding box (measured,
//! `tools/probes/inline-svg.json`, case `paint-viewbox-mapping`).

use swb_layout::svg::{SvgContent, SvgDrawItem};
use swb_layout::{Matrix, Rect, Size};
use swb_style::ComputedStyle;

use crate::display_list::DisplayItem;
use crate::rope::ItemRope;

/// Appends the items of `svg` drawn into the content box `content` (list
/// coordinates) of an element with style `style`.
pub(crate) fn paint(list: &mut ItemRope, svg: &SvgContent, style: &ComputedStyle, content: Rect) {
    let mut items = Vec::new();
    svg.draw(Size::new(content.width, content.height), &mut items);
    if items.is_empty() {
        return;
    }
    let clip = style.overflow_x.clips() || style.overflow_y.clips();
    if clip {
        list.push(DisplayItem::PushClip(content));
    }
    let origin = Matrix::translate(content.x, content.y);
    for item in items {
        list.push(match item {
            SvgDrawItem::PushOpacity(opacity) => DisplayItem::PushOpacity {
                opacity: opacity.max(0.0),
                bounds: Rect::default(),
                fixed_bounds: None,
                escapes_clips: false,
            },
            SvgDrawItem::PopOpacity => DisplayItem::PopOpacity,
            SvgDrawItem::Fill {
                path,
                transform,
                color,
                rule,
            } => DisplayItem::FillPath {
                path,
                transform: origin.multiply(&transform),
                color,
                rule,
            },
            SvgDrawItem::Stroke {
                path,
                transform,
                color,
                stroke,
            } => DisplayItem::StrokePath {
                path,
                transform: origin.multiply(&transform),
                color,
                stroke,
            },
        });
    }
    if clip {
        list.push(DisplayItem::PopClip);
    }
}
