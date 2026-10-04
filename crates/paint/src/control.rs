//! The look of form controls: Chromium's light theme (measured with
//! Chromium 148 on Linux), the select arrow and the text caret.
//!
//! A control with the native look ([`ControlContent::native`]) is drawn
//! instead of its CSS background and border:
//!
//! - text fields, text areas and selects: the background, and a 1 px
//!   border in the border color without its corner pixels;
//! - buttons: the background and a 1 px border with a 3 px radius;
//! - checkboxes: a 13 px box with a 3 px radius, white with a gray border,
//!   or filled with the accent color (2 px radius) and a white check mark;
//! - radio buttons: a circle with a gray border, or with an accent border
//!   and an accent dot.
//!
//! A select always gets its arrow.

use std::sync::Arc;

use swb_layout::{ControlContent, ControlKind, Point, Rect};
use swb_style::{BorderStyle, ComputedStyle, Rgba};

use crate::display_list::DisplayItem;

/// The accent color of checked checkboxes and radio buttons.
const ACCENT: Rgba = Rgba::rgb(0x00, 0x75, 0xFF);

/// The border of checkboxes and radio buttons.
const CHECKBOX_BORDER: Rgba = Rgba::rgb(0x76, 0x76, 0x76);

/// The border and the fill of disabled checkboxes and radio buttons
/// (Chromium's colors at 30 % opacity).
const DISABLED_BORDER: Rgba = Rgba::new(0x76, 0x76, 0x76, 77);
const DISABLED_FILL: Rgba = Rgba::new(0xEF, 0xEF, 0xEF, 77);

/// The corner radius of the border of buttons and checkboxes, and of the
/// fill of a checked checkbox (measured).
const RADIUS: f32 = 3.0;
const CHECKED_RADIUS: f32 = 2.0;

/// The items that draw the native look of a control whose border box is
/// `rect` (document coordinates). Empty for controls without the native
/// look, except for the arrow of a select.
pub(crate) fn native_look(
    control: &ControlContent,
    style: &ComputedStyle,
    rect: Rect,
) -> Vec<DisplayItem> {
    let mut items = Vec::new();
    if control.native {
        match control.kind {
            ControlKind::TextField { .. } | ControlKind::TextArea { .. } | ControlKind::Select => {
                field(&mut items, style, rect);
            }
            ControlKind::Button | ControlKind::ButtonElement => button(&mut items, style, rect),
            ControlKind::Checkbox => checkbox(&mut items, control, rect),
            ControlKind::Radio => radio(&mut items, control, rect),
        }
    }
    if control.kind == ControlKind::Select {
        arrow(&mut items, style, rect);
    }
    items
}

/// The caret of a focused text control whose border box starts at
/// `origin`.
pub(crate) fn caret(
    control: &ControlContent,
    style: &ComputedStyle,
    origin: Point,
) -> Option<DisplayItem> {
    let caret = control.caret?;
    Some(DisplayItem::Rect {
        rect: caret.translate(origin),
        radii: [(0.0, 0.0); 4],
        color: style.color,
    })
}

fn background(style: &ComputedStyle) -> Rgba {
    style.background_color.resolve(style.color)
}

fn border_color(style: &ComputedStyle) -> Rgba {
    style.border_top_color.resolve(style.color)
}

/// A text field: the background, and a 1 px border whose corner pixels
/// stay empty.
fn field(items: &mut Vec<DisplayItem>, style: &ComputedStyle, r: Rect) {
    items.push(rect(r, background(style), 0.0));
    let color = border_color(style);
    let inner_w = (r.width - 2.0).max(0.0);
    let inner_h = (r.height - 2.0).max(0.0);
    for side in [
        Rect::new(r.x + 1.0, r.y, inner_w, 1.0),
        Rect::new(r.x + 1.0, r.bottom() - 1.0, inner_w, 1.0),
        Rect::new(r.x, r.y + 1.0, 1.0, inner_h),
        Rect::new(r.right() - 1.0, r.y + 1.0, 1.0, inner_h),
    ] {
        items.push(rect(side, color, 0.0));
    }
}

/// A button: the background and a 1 px border, both with rounded
/// corners.
fn button(items: &mut Vec<DisplayItem>, style: &ComputedStyle, r: Rect) {
    items.push(rect(r, background(style), RADIUS));
    items.push(ring(r, border_color(style), RADIUS));
}

fn checkbox(items: &mut Vec<DisplayItem>, control: &ControlContent, r: Rect) {
    match (control.checked, control.disabled) {
        (false, disabled) => {
            let (fill, border) = if disabled {
                (DISABLED_FILL, DISABLED_BORDER)
            } else {
                (Rgba::WHITE, CHECKBOX_BORDER)
            };
            items.push(rect(r, fill, RADIUS));
            items.push(ring(r, border, RADIUS));
        }
        (true, disabled) => {
            let (fill, mark) = if disabled {
                (DISABLED_BORDER, Rgba::new(255, 255, 255, 153))
            } else {
                (ACCENT, Rgba::WHITE)
            };
            items.push(rect(r, fill, CHECKED_RADIUS));
            // Chromium's check mark (`NativeThemeBase::PaintCheckbox`).
            let at = |fx: f32, fy: f32| Point::new(r.x + r.width * fx, r.y + r.height * fy);
            items.push(DisplayItem::Polyline {
                points: Arc::from([at(0.2, 0.5), at(0.4, 0.7), at(0.8, 0.2)]),
                width: r.height * 0.16,
                color: mark,
            });
        }
    }
}

fn radio(items: &mut Vec<DisplayItem>, control: &ControlContent, r: Rect) {
    let radius = r.width.min(r.height) / 2.0;
    let border = match (control.checked, control.disabled) {
        (_, true) => DISABLED_BORDER,
        (true, false) => ACCENT,
        (false, false) => CHECKBOX_BORDER,
    };
    let fill = if control.disabled {
        DISABLED_FILL
    } else {
        Rgba::WHITE
    };
    items.push(rect(r, fill, radius));
    items.push(ring(r, border, radius));
    if control.checked {
        let dot = radius * 0.6;
        let center = Point::new(r.x + r.width / 2.0, r.y + r.height / 2.0);
        let dot_rect = Rect::new(center.x - dot, center.y - dot, dot * 2.0, dot * 2.0);
        items.push(rect(dot_rect, border, dot));
    }
}

/// The arrow of a select: a chevron centered 9 px from the right edge,
/// scaled with the font size (13.33 px is Chromium's default).
fn arrow(items: &mut Vec<DisplayItem>, style: &ComputedStyle, r: Rect) {
    let k = (style.font_size / 13.333_333).clamp(0.5, 4.0);
    let cx = r.right() - 9.0 * k;
    let cy = r.y + r.height / 2.0;
    items.push(DisplayItem::Polyline {
        points: Arc::from([
            Point::new(cx - 4.0 * k, cy - 2.5 * k),
            Point::new(cx, cy + 2.0 * k),
            Point::new(cx + 4.0 * k, cy - 2.5 * k),
        ]),
        width: 1.5 * k,
        color: style.color,
    });
}

fn rect(r: Rect, color: Rgba, radius: f32) -> DisplayItem {
    DisplayItem::Rect {
        rect: r,
        radii: [(radius, radius); 4],
        color,
    }
}

/// A 1 px border inside `r`.
fn ring(r: Rect, color: Rgba, radius: f32) -> DisplayItem {
    DisplayItem::Border {
        rect: r,
        widths: [1.0; 4],
        colors: [color; 4],
        styles: [BorderStyle::Solid; 4],
        radii: [(radius, radius); 4],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn control(kind: ControlKind, checked: bool) -> ControlContent {
        ControlContent {
            kind,
            checked,
            disabled: false,
            native: true,
            caret: None,
            scroll: Point::default(),
        }
    }

    #[test]
    fn checked_checkbox_has_a_check_mark() {
        let style = ComputedStyle::initial();
        let r = Rect::new(0.0, 0.0, 13.0, 13.0);
        let items = native_look(&control(ControlKind::Checkbox, true), &style, r);
        assert!(matches!(items[0], DisplayItem::Rect { color, .. } if color == ACCENT));
        assert!(matches!(items[1], DisplayItem::Polyline { color, .. } if color == Rgba::WHITE));
    }

    #[test]
    fn styled_select_keeps_its_arrow() {
        let style = ComputedStyle::initial();
        let r = Rect::new(0.0, 0.0, 30.0, 19.0);
        let mut select = control(ControlKind::Select, false);
        select.native = false;
        let items = native_look(&select, &style, r);
        assert_eq!(items.len(), 1);
        assert!(matches!(items[0], DisplayItem::Polyline { .. }));
    }
}
