//! The look of media elements (`<video>`, `<audio>`): the default poster
//! and the controls, approximating Chromium 148's (measured on Linux at
//! device scale 1; layout decides which parts show and where, see
//! `layout/src/media.rs`).
//!
//! - A video without a `poster` attribute is filled with #333 (Chromium's
//!   default poster; a poster that fails to load shows the background).
//! - Video controls: a black gradient (opaque at the bottom edge,
//!   transparent 112 px above it; the stops are measured), white icons
//!   and text, a timeline in white at 30 % opacity. Dimmed parts are
//!   white at 30 % opacity.
//! - Audio controls: a #f1f3f4 panel with fully rounded ends, black icons
//!   (30 % opacity when dimmed), the time in black at 87 % opacity, the
//!   timeline in black at 20 % opacity.
//!
//! The icons are drawn in a 20 × 20 px box centered in each button, on a
//! grid of 24 units (the box is 24 units wide): a play triangle, a
//! speaker with two sound waves, four fullscreen corners and three dots,
//! with corners measured from Chromium's rendering.

use std::sync::Arc;

use swb_layout::{MediaContent, MediaPart, MediaPartKind, Point, Rect};
use swb_style::{Color, LengthPercentage, LinearGradient, Rgba};

use crate::display_list::DisplayItem;

/// The fill of a video without a poster.
const DEFAULT_POSTER: Rgba = Rgba::rgb(0x33, 0x33, 0x33);

/// The panel behind audio controls.
const AUDIO_PANEL: Rgba = Rgba::rgb(0xF1, 0xF3, 0xF4);

/// The alpha of dimmed icons (30 %).
const DIMMED_ALPHA: u8 = 77;

/// The text of audio controls (black at 87 %).
const AUDIO_TEXT: Rgba = Rgba::new(0, 0, 0, 222);

/// The timeline of audio controls (black at 20 %).
const AUDIO_TIMELINE: Rgba = Rgba::new(0, 0, 0, 51);

/// The gradient behind video controls: (distance from the bottom edge in
/// px, alpha of black), from the bottom up, measured with Chromium 148.
/// The last stop is at the height of the gradient's area that layout
/// gives (112 px).
const GRADIENT_STOPS: [(f32, f32); 12] = [
    (0.0, 1.0),
    (9.5, 0.988),
    (19.5, 0.949),
    (29.5, 0.859),
    (39.5, 0.741),
    (49.5, 0.596),
    (59.5, 0.447),
    (69.5, 0.302),
    (79.5, 0.176),
    (89.5, 0.086),
    (99.5, 0.027),
    (112.0, 0.0),
];

/// The size of an icon box (centered in its button).
const ICON_SIZE: f32 = 20.0;

/// The items that paint the default poster of a video whose content box
/// is `content` (document coordinates), if it has none.
pub(crate) fn default_poster(media: &MediaContent, content: Rect) -> Option<DisplayItem> {
    media.default_poster.then_some(DisplayItem::Rect {
        rect: content,
        radii: [(0.0, 0.0); 4],
        color: DEFAULT_POSTER,
    })
}

/// The items that paint the controls of a media element whose content box
/// is `content` (document coordinates). They are clipped to the content
/// box.
pub(crate) fn controls(media: &MediaContent, content: Rect) -> Vec<DisplayItem> {
    let mut items = vec![DisplayItem::PushClip(content)];
    for part in &media.parts {
        let rect = part.rect.translate(content.origin());
        draw_part(&mut items, media.audio, part, rect);
    }
    items.push(DisplayItem::PopClip);
    items
}

fn draw_part(items: &mut Vec<DisplayItem>, audio: bool, part: &MediaPart, rect: Rect) {
    let color = icon_color(audio, part.enabled);
    match &part.kind {
        MediaPartKind::Panel if audio => {
            let r = (rect.height / 2.0).min(rect.width / 2.0);
            items.push(DisplayItem::Rect {
                rect,
                radii: [(r, r); 4],
                color: AUDIO_PANEL,
            });
        }
        MediaPartKind::Panel => items.push(gradient(rect)),
        MediaPartKind::Play => icon(items, rect, color, &[&PLAY]),
        MediaPartKind::Time(text) => items.push(DisplayItem::Text {
            origin: Point::new(rect.x, rect.y + text.baseline),
            font: text.font,
            size: text.size,
            glyphs: Arc::clone(&text.glyphs),
            color: if audio { AUDIO_TEXT } else { Rgba::WHITE },
        }),
        MediaPartKind::Timeline => items.push(DisplayItem::Rect {
            rect,
            radii: [(rect.height / 2.0, rect.height / 2.0); 4],
            color: if audio {
                AUDIO_TIMELINE
            } else {
                Rgba::new(255, 255, 255, DIMMED_ALPHA)
            },
        }),
        MediaPartKind::Mute => {
            let inner = half_ellipse_band((14.0, 12.0), (0.0, 0.0), (2.5, 4.0));
            let outer = half_ellipse_band((14.0, 12.0), (5.0, 6.7), (7.0, 8.8));
            icon(items, rect, color, &[&SPEAKER, &inner, &outer]);
        }
        MediaPartKind::Fullscreen => icon(items, rect, color, &FULLSCREEN),
        MediaPartKind::Menu => {
            let origin = icon_origin(rect);
            let scale = ICON_SIZE / 24.0;
            let r = 2.0 * scale;
            for y in [6.0, 12.0, 18.0] {
                items.push(DisplayItem::Rect {
                    rect: Rect::new(
                        origin.x + 12.0 * scale - r,
                        origin.y + y * scale - r,
                        2.0 * r,
                        2.0 * r,
                    ),
                    radii: [(r, r); 4],
                    color,
                });
            }
        }
    }
}

/// White on video controls, black on audio controls; dimmed parts at
/// 30 % opacity.
fn icon_color(audio: bool, enabled: bool) -> Rgba {
    let alpha = if enabled { 255 } else { DIMMED_ALPHA };
    if audio {
        Rgba::new(0, 0, 0, alpha)
    } else {
        Rgba::new(255, 255, 255, alpha)
    }
}

/// The gradient behind video controls, filling `rect` (its bottom edge is
/// the bottom of the video).
fn gradient(rect: Rect) -> DisplayItem {
    let height = rect.height.max(1.0);
    let stops = GRADIENT_STOPS
        .iter()
        .map(|&(d, alpha)| {
            let a = (alpha * 255.0).round() as u8;
            (
                Color::Rgba(Rgba::new(0, 0, 0, a)),
                Some(LengthPercentage::Percent(d / height)),
            )
        })
        .collect();
    DisplayItem::LinearGradient {
        rect,
        clip: rect,
        gradient: Arc::new(LinearGradient {
            angle_deg: 0.0,
            stops,
            repeating: false,
        }),
        current_color: Rgba::BLACK,
    }
}

/// The top left corner of the icon box of the button `rect`.
fn icon_origin(rect: Rect) -> Point {
    Point::new(
        rect.x + (rect.width - ICON_SIZE) / 2.0,
        rect.y + (rect.height - ICON_SIZE) / 2.0,
    )
}

/// Fills the polygons `shapes` (in icon units: the icon box is 24 units)
/// in the icon box of the button `rect`.
fn icon(items: &mut Vec<DisplayItem>, rect: Rect, color: Rgba, shapes: &[&[(f32, f32)]]) {
    let origin = icon_origin(rect);
    let scale = ICON_SIZE / 24.0;
    for shape in shapes {
        let points: Arc<[Point]> = shape
            .iter()
            .map(|&(x, y)| Point::new(origin.x + x * scale, origin.y + y * scale))
            .collect();
        items.push(DisplayItem::Polygon { points, color });
    }
}

/// The play triangle.
const PLAY: [(f32, f32); 3] = [(8.0, 5.0), (8.0, 19.0), (19.0, 12.0)];

/// The speaker of the mute button: a box and a cone.
const SPEAKER: [(f32, f32); 6] = [
    (3.0, 9.0),
    (3.0, 15.0),
    (7.0, 15.0),
    (12.0, 20.0),
    (12.0, 4.0),
    (7.0, 9.0),
];

/// The four corners of the fullscreen button: L shapes 5 units long and
/// 2 units thick at the corners of the square from 5 to 19.
const FULLSCREEN: [&[(f32, f32)]; 4] = [
    &[
        (5.0, 5.0),
        (10.0, 5.0),
        (10.0, 7.0),
        (7.0, 7.0),
        (7.0, 10.0),
        (5.0, 10.0),
    ],
    &[
        (19.0, 5.0),
        (19.0, 10.0),
        (17.0, 10.0),
        (17.0, 7.0),
        (14.0, 7.0),
        (14.0, 5.0),
    ],
    &[
        (19.0, 19.0),
        (14.0, 19.0),
        (14.0, 17.0),
        (17.0, 17.0),
        (17.0, 14.0),
        (19.0, 14.0),
    ],
    &[
        (5.0, 19.0),
        (5.0, 14.0),
        (7.0, 14.0),
        (7.0, 17.0),
        (10.0, 17.0),
        (10.0, 19.0),
    ],
];

/// The right half of the band between two ellipses around `center` with
/// the radii `inner` and `outer` (x, y), as a polygon: a sound wave of the
/// mute button. An inner radius of 0 gives a half ellipse.
fn half_ellipse_band(center: (f32, f32), inner: (f32, f32), outer: (f32, f32)) -> Vec<(f32, f32)> {
    const STEPS: usize = 12;
    let point = |radii: (f32, f32), i: usize| {
        let angle = std::f32::consts::PI * (i as f32 / STEPS as f32 - 0.5);
        (
            center.0 + radii.0 * angle.cos(),
            center.1 + radii.1 * angle.sin(),
        )
    };
    let mut points: Vec<(f32, f32)> = (0..=STEPS).map(|i| point(outer, i)).collect();
    points.extend((0..=STEPS).rev().map(|i| point(inner, i)));
    points
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_dom::NodeId;

    fn content(audio: bool, parts: Vec<MediaPart>) -> MediaContent {
        MediaContent {
            node: NodeId::DOCUMENT,
            default_poster: !audio,
            audio,
            parts,
        }
    }

    fn part(kind: MediaPartKind, rect: Rect, enabled: bool) -> MediaPart {
        MediaPart {
            kind,
            rect,
            enabled,
        }
    }

    #[test]
    fn default_poster_fills_the_content_box() {
        let rect = Rect::new(10.0, 20.0, 250.0, 159.0);
        let video = content(false, Vec::new());
        assert!(matches!(
            default_poster(&video, rect),
            Some(DisplayItem::Rect { rect: r, color, .. }) if r == rect && color == DEFAULT_POSTER
        ));
        assert!(default_poster(&content(true, Vec::new()), rect).is_none());
    }

    /// The play button of the 250×159 video at (10, 20): Chromium draws
    /// the triangle from about (30.7, 125) to (40.2, 131), 12 px high.
    #[test]
    fn play_icon_matches_chromium() {
        let video = content(
            false,
            vec![part(
                MediaPartKind::Play,
                Rect::new(0.0, 87.0, 48.0, 48.0),
                true,
            )],
        );
        let items = controls(&video, Rect::new(10.0, 20.0, 250.0, 159.0));
        assert!(matches!(items.first(), Some(DisplayItem::PushClip(_))));
        assert!(matches!(items.last(), Some(DisplayItem::PopClip)));
        let Some(DisplayItem::Polygon { points, color }) = items.get(1) else {
            panic!("a polygon: {items:?}");
        };
        assert_eq!(*color, Rgba::WHITE);
        let close = |p: Point, x: f32, y: f32| (p.x - x).abs() < 0.2 && (p.y - y).abs() < 0.2;
        assert!(close(points[0], 30.67, 125.17), "{points:?}");
        assert!(close(points[1], 30.67, 136.83), "{points:?}");
        assert!(close(points[2], 39.83, 131.0), "{points:?}");
    }

    #[test]
    fn dimmed_parts_and_audio_colors() {
        assert_eq!(icon_color(false, false), Rgba::new(255, 255, 255, 77));
        assert_eq!(icon_color(true, true), Rgba::BLACK);
        let audio = content(
            true,
            vec![part(
                MediaPartKind::Panel,
                Rect::new(0.0, 0.0, 300.0, 54.0),
                true,
            )],
        );
        let items = controls(&audio, Rect::new(0.0, 0.0, 300.0, 54.0));
        assert!(matches!(
            items[1],
            DisplayItem::Rect { radii, color, .. } if radii == [(27.0, 27.0); 4] && color == AUDIO_PANEL
        ));
    }

    /// The gradient is opaque black at the bottom and transparent 112 px
    /// above it.
    #[test]
    fn gradient_stops_are_in_px_from_the_bottom() {
        let DisplayItem::LinearGradient { gradient, .. } =
            gradient(Rect::new(0.0, 47.0, 250.0, 112.0))
        else {
            panic!("a gradient");
        };
        assert_eq!(gradient.angle_deg, 0.0);
        assert_eq!(
            gradient.stops.first().map(|s| s.0),
            Some(Color::Rgba(Rgba::BLACK))
        );
        assert_eq!(
            gradient.stops.last(),
            Some(&(
                Color::Rgba(Rgba::new(0, 0, 0, 0)),
                Some(LengthPercentage::Percent(1.0))
            ))
        );
    }

    #[test]
    fn sound_waves_are_closed_bands() {
        let band = half_ellipse_band((14.0, 12.0), (5.0, 6.7), (7.0, 8.8));
        assert_eq!(band.len(), 26);
        assert!((band[0].0 - 14.0).abs() < 1e-4 && (band[0].1 - 3.2).abs() < 1e-4);
        assert!((band[6].0 - 21.0).abs() < 1e-4);
    }
}
