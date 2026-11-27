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

use swb_layout::{
    MediaButton, MediaContent, MediaControlsView, MediaPart, MediaPartKind, Point, Rect,
    VolumeSlider, volume_slider,
};
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

/// The hover circle and the volume pill of video controls (#202124 at 70 %,
/// measured: the pill and the circle composite to the same colors).
const VIDEO_DARK: Rgba = Rgba::new(32, 33, 36, 178);

/// The hover circle and the volume pill of audio controls (black at 5.5 %).
const AUDIO_DARK: Rgba = Rgba::new(0, 0, 0, 14);

/// The played part of the timeline and the volume of audio controls (black
/// at 95.5 %), and the rest of the track (black at 63.5 %).
const AUDIO_PLAYED: Rgba = Rgba::new(0, 0, 0, 243);
const AUDIO_TRACK: Rgba = Rgba::new(0, 0, 0, 162);

/// The volume track of audio controls when muted (black at 20 %), and the
/// played part of a video's volume (white at 68 %).
const AUDIO_TRACK_MUTED: Rgba = Rgba::new(0, 0, 0, 51);
const VIDEO_VOLUME_PLAYED: Rgba = Rgba::new(255, 255, 255, 173);

/// The radii of the hover circles of video and audio controls.
const VIDEO_CIRCLE: f32 = 18.0;
const AUDIO_CIRCLE: f32 = 16.0;

/// The diameter of the thumbs.
const THUMB: f32 = 12.0;

/// The items that paint the controls of a media element whose content box
/// is `content` (document coordinates), in the state that `view` tells (the
/// state before playback without one). They are clipped to the content box.
pub(crate) fn controls(
    media: &MediaContent,
    content: Rect,
    view: Option<&MediaControlsView>,
) -> Vec<DisplayItem> {
    let default = MediaControlsView::default();
    let view = view.unwrap_or(&default);
    let mut items = vec![DisplayItem::PushClip(content)];
    // A hidden control draws nothing (the video's own controls; the audio
    // panel does not fade).
    if view.opacity > 0.0 {
        let mute = media.parts.iter().find(|p| p.kind == MediaPartKind::Mute);
        let look = Look {
            audio: media.audio,
            view,
            fade: if media.audio { 1.0 } else { view.opacity },
            mute_x: mute.map_or(f32::INFINITY, |m| m.rect.x + content.x),
        };
        let slider = mute
            .filter(|_| view.slider)
            .map(|m| volume_slider(media.audio, m.rect));
        for part in &media.parts {
            let rect = part.rect.translate(content.origin());
            let pill = slider.map(|s| s.pill.translate(content.origin()));
            // The pill covers the time text and lies under the mute button.
            if part.kind == MediaPartKind::Mute
                && let Some(slider) = slider
            {
                look.draw_slider(&mut items, &slider, content.origin());
            }
            look.draw_part(&mut items, part, rect, pill);
        }
    }
    if let Some(angle) = view.spinner
        && !media.audio
    {
        items.push(spinner(content, angle));
    }
    items.push(DisplayItem::PopClip);
    items
}

/// What the parts of a control set draw with.
struct Look<'a> {
    audio: bool,
    view: &'a MediaControlsView,
    /// The opacity of the video controls.
    fade: f32,
    /// The left edge of the mute button (document coordinates), or
    /// infinity if there is none: the long time label has to end before it.
    mute_x: f32,
}

impl Look<'_> {
    /// `color` with the fade applied.
    fn faded(&self, color: Rgba) -> Rgba {
        Rgba::new(
            color.r,
            color.g,
            color.b,
            (f32::from(color.a) * self.fade).round() as u8,
        )
    }

    /// Whether a button part draws as available: layout decides it before
    /// playback; with the metadata the mute button needs an audio track and
    /// the fullscreen button of a video the metadata.
    fn enabled(&self, part: &MediaPart) -> bool {
        match part.kind {
            MediaPartKind::Mute => self.view.metadata && self.view.has_audio,
            MediaPartKind::Fullscreen => self.view.metadata,
            _ => part.enabled,
        }
    }

    fn draw_part(
        &self,
        items: &mut Vec<DisplayItem>,
        part: &MediaPart,
        rect: Rect,
        pill: Option<Rect>,
    ) {
        let enabled = self.enabled(part);
        let color = self.faded(icon_color(self.audio, enabled));
        match &part.kind {
            MediaPartKind::Panel if self.audio => {
                let r = (rect.height / 2.0).min(rect.width / 2.0);
                items.push(DisplayItem::Rect {
                    rect,
                    radii: [(r, r); 4],
                    color: AUDIO_PANEL,
                });
            }
            MediaPartKind::Panel => items.push(gradient(rect, self.fade)),
            MediaPartKind::Play => {
                self.hover_circle(items, MediaButton::Play, enabled, rect);
                if self.view.playing {
                    icon(items, rect, color, &PAUSE);
                } else {
                    icon(items, rect, color, &[&PLAY]);
                }
            }
            MediaPartKind::Time(text) => {
                // With the metadata: the long label if it ends before the
                // mute button (audio controls: always, layout made room).
                let text = match &self.view.time {
                    Some(times) if self.audio || rect.x + times.full_width <= self.mute_x => {
                        &times.full
                    }
                    Some(times) => &times.current,
                    None => text,
                };
                let ink = if self.audio {
                    AUDIO_TEXT
                } else {
                    self.faded(Rgba::WHITE)
                };
                items.push(DisplayItem::Text {
                    origin: Point::new(rect.x, rect.y + text.baseline),
                    font: text.font,
                    size: text.size,
                    glyphs: Arc::clone(&text.glyphs),
                    color: ink,
                });
            }
            MediaPartKind::Timeline => self.timeline(items, rect, pill),
            MediaPartKind::Mute => {
                self.hover_circle(items, MediaButton::Mute, enabled, rect);
                self.mute_icon(items, rect, color);
            }
            MediaPartKind::Fullscreen => {
                self.hover_circle(items, MediaButton::Fullscreen, enabled, rect);
                icon(items, rect, color, &FULLSCREEN);
            }
            MediaPartKind::Menu => {
                self.hover_circle(items, MediaButton::Menu, enabled, rect);
                menu_icon(items, rect, color);
            }
        }
    }

    /// The speaker of the mute button; muted (or at volume 0) it has a slash
    /// instead of the sound waves.
    fn mute_icon(&self, items: &mut Vec<DisplayItem>, rect: Rect, color: Rgba) {
        if self.view.muted || self.view.volume <= 0.0 {
            icon(items, rect, color, &[&SPEAKER, &SLASH]);
        } else {
            let inner = half_ellipse_band((14.0, 12.0), (0.0, 0.0), (2.5, 4.0));
            let outer = half_ellipse_band((14.0, 12.0), (5.0, 6.7), (7.0, 8.8));
            icon(items, rect, color, &[&SPEAKER, &inner, &outer]);
        }
    }

    /// The round background of an enabled button under the pointer.
    fn hover_circle(
        &self,
        items: &mut Vec<DisplayItem>,
        button: MediaButton,
        enabled: bool,
        rect: Rect,
    ) {
        // Under the volume slider's pill the mute button has no circle.
        if self.view.hover != Some(button) || !enabled || self.view.slider {
            return;
        }
        let (r, color) = if self.audio {
            (AUDIO_CIRCLE, AUDIO_DARK)
        } else {
            (VIDEO_CIRCLE, self.faded(VIDEO_DARK))
        };
        let center = Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        items.push(DisplayItem::Rect {
            rect: Rect::new(center.x - r, center.y - r, 2.0 * r, 2.0 * r),
            radii: [(r, r); 4],
            color,
        });
    }

    /// The timeline: the track, the played part and the thumb. With the
    /// volume slider open the timeline of audio controls ends 15 px before
    /// the slider's pill (measured).
    fn timeline(&self, items: &mut Vec<DisplayItem>, bar: Rect, pill: Option<Rect>) {
        let mut bar = bar;
        if let Some(pill) = pill.filter(|_| self.audio) {
            let end = (pill.x - 15.0).max(bar.x);
            bar.width = (end - bar.x).min(bar.width);
        }
        let (track, played, thumb) = if self.audio {
            // Before the metadata the timeline of audio controls is disabled
            // (black at 20 %).
            let track = if self.view.metadata {
                AUDIO_TRACK
            } else {
                AUDIO_TRACK_MUTED
            };
            (track, AUDIO_PLAYED, Rgba::BLACK)
        } else {
            // The bar of a video is white at 68 % once it is enabled (the
            // metadata is known), at 30 % before.
            let alpha = if self.view.metadata {
                VIDEO_VOLUME_PLAYED.a
            } else {
                DIMMED_ALPHA
            };
            (Rgba::new(255, 255, 255, alpha), Rgba::WHITE, Rgba::WHITE)
        };
        // The played part is the fraction of the whole bar (measured: at the
        // end it reaches the bar's end); the thumb's center runs 6 px
        // inside the ends.
        let fraction = self.view.played.clamp(0.0, 1.0);
        let center = bar.x + THUMB / 2.0 + fraction * (bar.width - THUMB).max(0.0);
        let end = bar.x + fraction * bar.width;
        bar_items(items, bar, end, self.faded(played), self.faded(track));
        if self.view.thumb {
            let y = bar.y + bar.height / 2.0;
            items.push(circle(center, y, self.faded(thumb)));
        }
    }

    /// The pill of the volume slider with its track and thumb.
    fn draw_slider(&self, items: &mut Vec<DisplayItem>, slider: &VolumeSlider, origin: Point) {
        let pill = slider.pill.translate(origin);
        let track = slider.track.translate(origin);
        let r = pill.height / 2.0;
        let (dark, unplayed, played, thumb) = if self.audio {
            // Measured: the full track is black at 63 % (the volume is 1),
            // the track of a muted element black at 20 %.
            (AUDIO_DARK, AUDIO_TRACK_MUTED, AUDIO_TRACK, Rgba::BLACK)
        } else {
            (
                self.faded(VIDEO_DARK),
                self.faded(Rgba::new(255, 255, 255, DIMMED_ALPHA)),
                self.faded(VIDEO_VOLUME_PLAYED),
                self.faded(Rgba::WHITE),
            )
        };
        items.push(DisplayItem::Rect {
            rect: pill,
            radii: [(r, r); 4],
            color: dark,
        });
        if self.view.muted {
            // Nothing is played.
            bar_items(items, track, track.x, played, unplayed);
            return;
        }
        let volume = self.view.volume.clamp(0.0, 1.0);
        let center = track.x + THUMB / 2.0 + volume * (track.width - THUMB);
        // At volume 1 the whole track is played (Chromium draws no thumb
        // unless the pointer is over the slider).
        let end = if volume >= 1.0 { track.right() } else { center };
        bar_items(items, track, end, played, unplayed);
        if self.view.slider_thumb {
            items.push(circle(center, track.y + track.height / 2.0, thumb));
        }
    }
}

/// A bar with round ends: the part up to `end` in `played`, the rest in
/// `unplayed` (the two do not overlap, so translucent colors do not blend
/// twice).
fn bar_items(items: &mut Vec<DisplayItem>, bar: Rect, end: f32, played: Rgba, unplayed: Rgba) {
    let r = (bar.height / 2.0, bar.height / 2.0);
    let square = (0.0, 0.0);
    let end = end.clamp(bar.x, bar.right());
    if end > bar.x {
        let whole = end >= bar.right();
        items.push(DisplayItem::Rect {
            rect: Rect::new(bar.x, bar.y, end - bar.x, bar.height),
            radii: if whole {
                [r; 4]
            } else {
                [r, square, square, r]
            },
            color: played,
        });
    }
    if end < bar.right() {
        items.push(DisplayItem::Rect {
            rect: Rect::new(end, bar.y, bar.right() - end, bar.height),
            radii: if end > bar.x {
                [square, r, r, square]
            } else {
                [r; 4]
            },
            color: unplayed,
        });
    }
}

/// The three dots of the overflow menu button.
fn menu_icon(items: &mut Vec<DisplayItem>, rect: Rect, color: Rgba) {
    let origin = icon_origin(rect);
    let scale = ICON_SIZE / 24.0;
    // Radius 1.67 px (the look before playback; Chromium's are about 1.5:
    // a departure that keeps the Wikipedia thumbnail unchanged).
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

/// A filled circle of the thumbs' size.
fn circle(x: f32, y: f32, color: Rgba) -> DisplayItem {
    let r = THUMB / 2.0;
    DisplayItem::Rect {
        rect: Rect::new(x - r, y - r, THUMB, THUMB),
        radii: [(r, r); 4],
        color,
    }
}

/// The loading arc of a video: 270 degrees of a ring of about 55 px across,
/// a little above the middle, black, turned by `angle` radians.
fn spinner(content: Rect, angle: f32) -> DisplayItem {
    const OUTER: f32 = 31.0;
    const INNER: f32 = 27.5;
    const SPAN: f32 = std::f32::consts::PI * 1.5;
    const STEPS: usize = 24;
    let center = Point::new(
        content.x + content.width / 2.0,
        content.y + content.height / 2.0 - 12.5,
    );
    let at = |radius: f32, i: usize| {
        let a = angle + SPAN * (i as f32 / STEPS as f32);
        Point::new(center.x + radius * a.cos(), center.y + radius * a.sin())
    };
    let mut points: Vec<Point> = (0..=STEPS).map(|i| at(OUTER, i)).collect();
    points.extend((0..=STEPS).rev().map(|i| at(INNER, i)));
    DisplayItem::Polygon {
        points: points.into(),
        color: Rgba::new(0, 0, 0, 230),
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
/// the bottom of the video); `fade` scales its opacity.
fn gradient(rect: Rect, fade: f32) -> DisplayItem {
    let height = rect.height.max(1.0);
    let stops = GRADIENT_STOPS
        .iter()
        .map(|&(d, alpha)| {
            let a = (alpha * fade * 255.0).round() as u8;
            (
                Color::Rgba(Rgba::new(0, 0, 0, a)),
                Some(LengthPercentage::Percent(d / height)),
            )
        })
        .collect();
    DisplayItem::LinearGradient {
        rect,
        tile: rect,
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

/// The pause icon: two bars (measured: x 19 to 22.4 and 25.6 to 29 on a
/// button centered at x = 24, a 20 px icon box).
const PAUSE: [&[(f32, f32)]; 2] = [
    &[(6.0, 5.0), (10.0, 5.0), (10.0, 19.0), (6.0, 19.0)],
    &[(14.0, 5.0), (18.0, 5.0), (18.0, 19.0), (14.0, 19.0)],
];

/// The slash of the muted icon, from the top left to the bottom right.
const SLASH: [(f32, f32); 4] = [(4.3, 3.0), (3.0, 4.3), (19.7, 21.0), (21.0, 19.7)];

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
        let items = controls(&video, Rect::new(10.0, 20.0, 250.0, 159.0), None);
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
        let items = controls(&audio, Rect::new(0.0, 0.0, 300.0, 54.0), None);
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
            gradient(Rect::new(0.0, 47.0, 250.0, 112.0), 1.0)
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
