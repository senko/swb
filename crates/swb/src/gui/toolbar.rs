//! The browser toolbar (back, forward, reload, address bar) and the status
//! bubble, drawn with the same display list and rasterizer as pages.

use std::sync::Arc;

use swb_engine::TextEdit;
use swb_layout::{Point, PositionedGlyph, Rect};
use swb_paint::{DisplayItem, DisplayList};
use swb_style::{BorderStyle, Rgba};
use swb_text::{
    Direction, FamilyName, FontContext, FontId, FontQuery, FontStyle, GenericFamily, ShapeOptions,
};

/// Height of the toolbar in CSS px.
pub(crate) const TOOLBAR_HEIGHT: f32 = 40.0;
const BUTTON: f32 = 28.0;
const UI_FONT_SIZE: f32 = 14.0;
const FIELD_PADDING: f32 = 10.0;

const BACKGROUND: Rgba = Rgba::rgb(0xEC, 0xEC, 0xEC);
const BORDER: Rgba = Rgba::rgb(0xC8, 0xC8, 0xC8);
const ICON: Rgba = Rgba::rgb(0x30, 0x30, 0x30);
const ICON_DISABLED: Rgba = Rgba::rgb(0xA8, 0xA8, 0xA8);
const SELECTION: Rgba = Rgba::rgb(0xB5, 0xD5, 0xFF);

/// A clickable part of the toolbar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ToolbarHit {
    Back,
    Forward,
    Reload,
    /// The address field; the value is the x position inside the text.
    Address(f32),
}

/// State that the toolbar shows.
pub(crate) struct ToolbarState<'a> {
    pub(crate) can_go_back: bool,
    pub(crate) can_go_forward: bool,
    pub(crate) loading: bool,
    pub(crate) status: Option<&'a str>,
}

/// The toolbar: address field state and geometry.
#[derive(Default)]
pub(crate) struct Toolbar {
    pub(crate) address: TextEdit,
    pub(crate) focused: bool,
}

fn button_rect(index: usize) -> Rect {
    Rect::new(
        6.0 + index as f32 * (BUTTON + 4.0),
        (TOOLBAR_HEIGHT - BUTTON) / 2.0,
        BUTTON,
        BUTTON,
    )
}

fn field_rect(width: f32) -> Rect {
    let x = button_rect(3).x + 4.0;
    Rect::new(x, 5.0, (width - x - 8.0).max(40.0), TOOLBAR_HEIGHT - 10.0)
}

impl Toolbar {
    /// What is at (x, y), in CSS px of the window.
    pub(crate) fn hit(x: f32, y: f32, width: f32) -> Option<ToolbarHit> {
        let p = Point::new(x, y);
        if button_rect(0).contains(p) {
            return Some(ToolbarHit::Back);
        }
        if button_rect(1).contains(p) {
            return Some(ToolbarHit::Forward);
        }
        if button_rect(2).contains(p) {
            return Some(ToolbarHit::Reload);
        }
        field_rect(width)
            .contains(p)
            .then_some(ToolbarHit::Address(Self::text_x(x, width)))
    }

    /// The x position relative to the start of the address text, for a
    /// window x position `x` (CSS px), also outside the field.
    pub(crate) fn text_x(x: f32, width: f32) -> f32 {
        x - field_rect(width).x - FIELD_PADDING
    }

    /// Moves the cursor of the address field to the character nearest to
    /// `x` (relative to the start of the text).
    pub(crate) fn place_cursor(&mut self, fonts: &mut FontContext, x: f32, select: bool) {
        let line = shape_line(fonts, self.address.text(), UI_FONT_SIZE);
        let pos = line
            .boundaries
            .iter()
            .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
            .map_or(0, |b| b.0);
        self.address.move_to(pos, select);
    }

    /// Builds the display list of the toolbar and the status bubble.
    pub(crate) fn display_list(
        &self,
        fonts: &mut FontContext,
        width: f32,
        height: f32,
        state: &ToolbarState<'_>,
    ) -> DisplayList {
        let mut items = Vec::new();
        items.push(rect(Rect::new(0.0, 0.0, width, TOOLBAR_HEIGHT), BACKGROUND));
        items.push(rect(
            Rect::new(0.0, TOOLBAR_HEIGHT - 1.0, width, 1.0),
            BORDER,
        ));

        // While loading, the reload button stops loading.
        let icons = [
            ("\u{2190}", state.can_go_back),
            ("\u{2192}", state.can_go_forward),
            (
                if state.loading {
                    "\u{00D7}"
                } else {
                    "\u{21BB}"
                },
                true,
            ),
        ];
        for (i, (icon, enabled)) in icons.iter().enumerate() {
            let r = button_rect(i);
            let line = shape_line(fonts, icon, 18.0);
            let origin = Point::new(
                r.x + (r.width - line.width) / 2.0,
                r.y + r.height / 2.0 + 6.0,
            );
            line.push_items(
                &mut items,
                origin,
                if *enabled { ICON } else { ICON_DISABLED },
            );
        }

        let field = field_rect(width);
        items.push(DisplayItem::Rect {
            rect: field,
            radii: [(field.height / 2.0, field.height / 2.0); 4],
            color: Rgba::WHITE,
        });
        items.push(DisplayItem::Border {
            rect: field,
            widths: [1.0; 4],
            colors: [if self.focused {
                Rgba::rgb(0x1A, 0x73, 0xE8)
            } else {
                BORDER
            }; 4],
            styles: [BorderStyle::Solid; 4],
            radii: [(field.height / 2.0, field.height / 2.0); 4],
        });
        items.push(DisplayItem::PushClip(Rect::new(
            field.x + FIELD_PADDING / 2.0,
            field.y,
            field.width - FIELD_PADDING,
            field.height,
        )));
        let line = shape_line(fonts, self.address.text(), UI_FONT_SIZE);
        let text_x = field.x + FIELD_PADDING;
        let baseline = field.y + field.height / 2.0 + 5.0;
        if self.focused {
            let sel = self.address.selection();
            if !sel.is_empty() {
                let x0 = line.x_at(sel.start);
                let x1 = line.x_at(sel.end);
                items.push(rect(
                    Rect::new(text_x + x0, field.y + 6.0, x1 - x0, field.height - 12.0),
                    SELECTION,
                ));
            }
        }
        line.push_items(
            &mut items,
            Point::new(text_x, baseline),
            Rgba::rgb(0x20, 0x20, 0x20),
        );
        if self.focused && self.address.selection().is_empty() {
            let x = text_x + line.x_at(self.address.cursor());
            items.push(rect(
                Rect::new(x.round(), field.y + 7.0, 1.0, field.height - 14.0),
                ICON,
            ));
        }
        items.push(DisplayItem::PopClip);

        if let Some(status) = state.status {
            status_bubble(&mut items, fonts, status, width, height);
        }
        DisplayList { items }
    }
}

/// The status bubble at the bottom left (the URL of the hovered link).
fn status_bubble(
    items: &mut Vec<DisplayItem>,
    fonts: &mut FontContext,
    status: &str,
    width: f32,
    height: f32,
) {
    // Long URLs (for example `data:` links) are cut; the bubble shows at
    // most 60% of the window width anyway.
    const MAX_CHARS: usize = 300;
    let shown: String = if status.chars().count() > MAX_CHARS {
        status
            .chars()
            .take(MAX_CHARS)
            .chain(std::iter::once('…'))
            .collect()
    } else {
        status.to_owned()
    };
    let line = shape_line(fonts, &shown, 12.0);
    let w = (line.width + 12.0).min(width * 0.6);
    let r = Rect::new(0.0, height - 22.0, w, 22.0);
    items.push(rect(r, Rgba::rgb(0xF4, 0xF4, 0xF4)));
    items.push(DisplayItem::Border {
        rect: r,
        widths: [1.0, 1.0, 0.0, 0.0],
        colors: [BORDER; 4],
        styles: [BorderStyle::Solid; 4],
        radii: [(0.0, 0.0); 4],
    });
    items.push(DisplayItem::PushClip(r));
    line.push_items(
        items,
        Point::new(6.0, height - 7.0),
        Rgba::rgb(0x30, 0x30, 0x30),
    );
    items.push(DisplayItem::PopClip);
}

fn rect(rect: Rect, color: Rgba) -> DisplayItem {
    DisplayItem::Rect {
        rect,
        radii: [(0.0, 0.0); 4],
        color,
    }
}

/// A shaped line of UI text.
struct UiLine {
    runs: Vec<(FontId, f32, Arc<[PositionedGlyph]>)>,
    size: f32,
    width: f32,
    /// (byte offset, x) for every character boundary, including the end.
    boundaries: Vec<(usize, f32)>,
}

impl UiLine {
    fn x_at(&self, offset: usize) -> f32 {
        self.boundaries
            .iter()
            .find(|b| b.0 >= offset)
            .map_or(self.width, |b| b.1)
    }

    fn push_items(&self, items: &mut Vec<DisplayItem>, origin: Point, color: Rgba) {
        for (font, x, glyphs) in &self.runs {
            items.push(DisplayItem::Text {
                origin: Point::new(origin.x + x, origin.y),
                font: *font,
                size: self.size,
                glyphs: Arc::clone(glyphs),
                color,
            });
        }
    }
}

fn shape_line(fonts: &mut FontContext, text: &str, size: f32) -> UiLine {
    let families = [FamilyName::Generic(GenericFamily::SansSerif)];
    let query = FontQuery {
        families: &families,
        weight: 400.0,
        style: FontStyle::Normal,
        stretch: 100.0,
        language: None,
    };
    let options = ShapeOptions {
        direction: Direction::Ltr,
        language: None,
        features: &[],
    };
    let mut runs = Vec::new();
    let mut cluster_x: Vec<(usize, f32)> = Vec::new();
    let mut pen = 0.0;
    for run in fonts.itemize(text, &query) {
        let shaped = fonts.shape(run.font, size, &text[run.range.clone()], &options);
        let start = pen;
        let mut glyphs = Vec::with_capacity(shaped.glyphs.len());
        for g in &shaped.glyphs {
            cluster_x.push((run.range.start + g.cluster as usize, pen));
            glyphs.push(PositionedGlyph {
                id: g.glyph,
                x: pen - start + g.x_offset,
                y: -g.y_offset,
            });
            pen += g.x_advance;
        }
        runs.push((run.font, start, Arc::from(glyphs)));
    }
    cluster_x.sort_by_key(|c| c.0);
    cluster_x.dedup_by_key(|c| c.0);
    // One forward pass: both lists are sorted by byte offset.
    let mut next_cluster = 0;
    let mut x = 0.0;
    let mut boundaries: Vec<(usize, f32)> = text
        .char_indices()
        .map(|(i, _)| {
            while let Some(&(offset, cluster_start)) = cluster_x.get(next_cluster)
                && offset <= i
            {
                x = cluster_start;
                next_cluster += 1;
            }
            (i, x)
        })
        .collect();
    boundaries.push((text.len(), pen));
    UiLine {
        runs,
        size,
        width: pen,
        boundaries,
    }
}
