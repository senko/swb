//! Media elements: `<video>` and `<audio>`
//! (<https://html.spec.whatwg.org/multipage/media.html>).
//!
//! A media element is a replaced element. swb plays no media and never
//! fetches the media resource (as with `preload="none"`). A video shows
//! its poster image (loaded like an `<img>`); its natural size is the
//! poster's, and without a loaded poster it has no natural size, so the
//! default object size (300×150) applies (HTML §4.8.8; Chromium 148
//! behaves the same, see `tests/layout/video-sizing.html`). The children
//! (`<source>`, `<track>`, fallback content) are not rendered.
//!
//! HTML exposes a user interface "if the `controls` attribute is present,
//! or if scripting is disabled for the media element". So with the
//! scripting flag of the document off, every video has controls, as in
//! Chromium with JavaScript disabled; with it on, only a video with the
//! `controls` attribute has (ADR 0020). Audio without `controls` is
//! `display: none` (user-agent style sheet). The controls approximate
//! Chromium 148's (measured on Linux at device scale 1; the parts that
//! paint draws are described in `paint/src/media.rs`).
//!
//! Video controls, for a content box of `w` × `h` px:
//!
//! - a gradient over the bottom 112 px;
//! - a row of 48 × 48 px buttons centered 48 px above the bottom edge:
//!   play at the left edge, the current time ("0:00", 14 px sans-serif,
//!   on a baseline 5 px below the row's center) after it, and from the
//!   right edge the overflow menu, fullscreen and mute;
//! - a 4 px timeline from 16 px to `w` − 16 px, with its top edge 24 px
//!   and its bottom edge 20 px above the bottom of the video.
//!
//! Chromium leaves out parts that do not fit: play needs `w` ≥ 122, mute
//! 170, fullscreen 198 and the time 246 (the menu always shows). Below a
//! height of 72 px the row is centered 24 px above the bottom and the
//! timeline is hidden; from 24 px to 48 px only the timeline shows; below
//! 24 px nothing but the gradient. Play and the menu are enabled if the
//! element has a media resource (a `src` attribute or a `<source>` child
//! with one); mute and fullscreen stay dimmed, because the media's
//! metadata never loads.
//!
//! Audio controls: a panel with rounded ends behind a row centered
//! vertically: play (2 px from the left edge), "0:00 / 0:00", the
//! timeline, mute (centered 58.5 px from the right edge) and the menu
//! (centered 26 px from the right edge), as measured at the default size
//! (300 × 54 px). The menu is enabled also without a media resource (as
//! measured). The time shows if the timeline after it stays at least
//! 32 px wide, and mute if it does not overlap play (swb's rules; Chromium
//! was not measured at other sizes).

use std::sync::Arc;

use swb_dom::{Document, NodeId, is_html_whitespace, local_name};
use swb_text::{FamilyName, FontContext, FontQuery, GenericFamily, ShapeOptions};

use crate::LayoutContext;
use crate::fonts;
use crate::fragment::{MediaButton, MediaContent, MediaPart, MediaPartKind, MediaText};
use crate::geom::{Point, Rect, Size};

/// The size of the buttons of media controls.
const BUTTON: f32 = 48.0;

/// The height of the band at the bottom of a video where the pointer shows
/// the timeline's thumb (measured: y from 215 px in a 240 px video).
pub const THUMB_BAND: f32 = 25.0;

/// The height of the gradient behind video controls (paint's gradient
/// stops end at the same height).
const GRADIENT_HEIGHT: f32 = 112.0;

/// The font size of the time display.
const TEXT_SIZE: f32 = 14.0;

/// The distance from the center of the button row down to the baseline
/// of the time display.
const BASELINE_BELOW_CENTER: f32 = 5.0;

// The minimum content-box width of a video for its play button, mute
// button, fullscreen button and time display (measured with Chromium 148
// in steps of 2 px).
const MIN_WIDTH_PLAY: f32 = 122.0;
const MIN_WIDTH_MUTE: f32 = 170.0;
const MIN_WIDTH_FULLSCREEN: f32 = 198.0;
const MIN_WIDTH_TIME: f32 = 246.0;

// The minimum content-box height of a video for the full controls (the
// button row and the timeline), for the button row alone, and for the
// timeline alone (measured with Chromium 148).
const MIN_HEIGHT_FULL: f32 = 72.0;
const MIN_HEIGHT_BUTTONS: f32 = 49.0;
const MIN_HEIGHT_TIMELINE: f32 = 24.0;

// The timeline: its inset from the left and right edges of a video, its
// thickness, and the distance from the bottom edge of a video up to the
// top edge of the timeline.
const TIMELINE_INSET: f32 = 16.0;
const TIMELINE_HEIGHT: f32 = 4.0;
const TIMELINE_ABOVE_BOTTOM: f32 = 24.0;

/// What box construction knows about a media element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Media {
    /// True for `<audio>`, false for `<video>`.
    pub(crate) audio: bool,
    /// True for a video with a `poster` attribute that is not blank.
    pub(crate) poster: bool,
    /// True if the element has a media resource: a `src` attribute or a
    /// `<source>` child with one (not blank).
    pub(crate) source: bool,
    /// True if the element shows its controls: it has the `controls`
    /// attribute, or scripting is disabled for the document.
    pub(crate) controls: bool,
    /// True if the video shows a frame of its video (the engine knows;
    /// [`crate::ReplacedSizes::shows_video_frame`]).
    pub(crate) frame: bool,
}

impl Media {
    /// The media element `node`, or `None` if it is not one.
    pub(crate) fn of(doc: &Document, node: NodeId) -> Option<Media> {
        let element = doc.element(node)?;
        let audio = element.is_html_named(&local_name!("audio"));
        if !audio && !element.is_html_named(&local_name!("video")) {
            return None;
        }
        let present = |value: Option<&str>| {
            value.is_some_and(|v| !v.trim_matches(is_html_whitespace).is_empty())
        };
        let source = present(element.attr("src"))
            || doc.element_children(node).any(|child| {
                doc.element(child).is_some_and(|c| {
                    c.is_html_named(&local_name!("source")) && present(c.attr("src"))
                })
            });
        Some(Media {
            audio,
            poster: !audio && present(element.attr("poster")),
            source,
            controls: !doc.scripting() || element.has_attr("controls"),
            frame: false,
        })
    }
}

/// The content of the fragment of media element `node` whose content box
/// is `size`.
pub(crate) fn content(
    ctx: &mut LayoutContext<'_>,
    node: NodeId,
    media: Media,
    size: Size,
) -> MediaContent {
    let parts = if media.audio {
        audio_parts(ctx, media, size)
    } else {
        video_parts(ctx, media, size)
    };
    MediaContent {
        node,
        // Chromium paints the dark fill of a video without a poster as part
        // of its controls: a video without controls paints nothing. A frame
        // of the video covers it, and not only where it draws: the
        // letterbox of a frame is transparent (measured, M14 feature 7a).
        default_poster: !media.audio && !media.poster && media.controls && !media.frame,
        audio: media.audio,
        parts,
    }
}

fn video_parts(ctx: &mut LayoutContext<'_>, media: Media, size: Size) -> Vec<MediaPart> {
    if !media.controls {
        return Vec::new();
    }
    let (w, h) = (size.width, size.height);
    let gradient = Rect::new(0.0, h - GRADIENT_HEIGHT, w, GRADIENT_HEIGHT);
    let mut parts = vec![part(MediaPartKind::Panel, gradient, true)];
    let row = if h >= MIN_HEIGHT_FULL {
        Some(h - BUTTON)
    } else if h >= MIN_HEIGHT_BUTTONS {
        Some(h - BUTTON / 2.0)
    } else {
        None
    };
    if let Some(center) = row {
        let button = |kind, x, enabled| {
            part(
                kind,
                Rect::new(x, center - BUTTON / 2.0, BUTTON, BUTTON),
                enabled,
            )
        };
        if w >= MIN_WIDTH_PLAY {
            parts.push(button(MediaPartKind::Play, 0.0, media.source));
        }
        if w >= MIN_WIDTH_TIME {
            parts.push(time(ctx, "0:00", BUTTON, center + BASELINE_BELOW_CENTER));
        }
        let mut right = w - BUTTON;
        parts.push(button(MediaPartKind::Menu, right, media.source));
        if w >= MIN_WIDTH_FULLSCREEN {
            right -= BUTTON;
            parts.push(button(MediaPartKind::Fullscreen, right, false));
        }
        if w >= MIN_WIDTH_MUTE {
            right -= BUTTON;
            parts.push(button(MediaPartKind::Mute, right, false));
        }
    }
    let timeline = h >= MIN_HEIGHT_FULL || (MIN_HEIGHT_TIMELINE..MIN_HEIGHT_BUTTONS).contains(&h);
    if timeline && w > 2.0 * TIMELINE_INSET {
        let rect = Rect::new(
            TIMELINE_INSET,
            h - TIMELINE_ABOVE_BOTTOM,
            w - 2.0 * TIMELINE_INSET,
            TIMELINE_HEIGHT,
        );
        parts.push(part(MediaPartKind::Timeline, rect, false));
    }
    parts
}

/// Audio controls (measured at 300 × 54 px): the play button 2 px from
/// the left edge, the time at 47 px, the menu and mute buttons centered
/// 26 px and 58.5 px from the right edge, and the timeline from 16 px
/// after the time to 90 px from the right edge. The button rects are 48 px
/// boxes around the icons; the pointer hits the 32 px box in their center
/// (`button_hit_area`).
fn audio_parts(ctx: &mut LayoutContext<'_>, media: Media, size: Size) -> Vec<MediaPart> {
    const PLAY_X: f32 = 2.0;
    const TIME_X: f32 = 47.0;
    const MENU_CENTER_FROM_RIGHT: f32 = 26.0;
    // The icon spans x = 235 to 248 at 300 px (re-measured in feature 9;
    // the mute button's box is centered on it).
    const MUTE_CENTER_FROM_RIGHT: f32 = 58.5;
    const TIMELINE_END_FROM_RIGHT: f32 = 90.0;
    const TIMELINE_GAP: f32 = 16.0;
    const MIN_TIMELINE: f32 = 32.0;
    let (w, h) = (size.width, size.height);
    let center = h / 2.0;
    let button = |kind, x, enabled| {
        part(
            kind,
            Rect::new(x, center - BUTTON / 2.0, BUTTON, BUTTON),
            enabled,
        )
    };
    let mut parts = vec![
        part(MediaPartKind::Panel, Rect::new(0.0, 0.0, w, h), true),
        button(MediaPartKind::Play, PLAY_X, media.source),
    ];
    let timeline_end = w - TIMELINE_END_FROM_RIGHT;
    let time = time(ctx, "0:00 / 0:00", TIME_X, center + BASELINE_BELOW_CENTER);
    let mut timeline_start = PLAY_X + BUTTON + TIMELINE_GAP;
    if timeline_end - (time.rect.right() + TIMELINE_GAP) >= MIN_TIMELINE {
        timeline_start = time.rect.right() + TIMELINE_GAP;
        parts.push(time);
    }
    if timeline_end > timeline_start {
        let rect = Rect::new(
            timeline_start,
            center - TIMELINE_HEIGHT / 2.0,
            timeline_end - timeline_start,
            TIMELINE_HEIGHT,
        );
        parts.push(part(MediaPartKind::Timeline, rect, false));
    }
    let mute_x = w - MUTE_CENTER_FROM_RIGHT - BUTTON / 2.0;
    if mute_x >= PLAY_X + BUTTON {
        parts.push(button(MediaPartKind::Mute, mute_x, false));
    }
    // Chromium 148 draws the audio menu enabled also without a source
    // (unlike the video menu).
    parts.push(button(
        MediaPartKind::Menu,
        w - MENU_CENTER_FROM_RIGHT - BUTTON / 2.0,
        true,
    ));
    parts
}

fn part(kind: MediaPartKind, rect: Rect, enabled: bool) -> MediaPart {
    MediaPart {
        kind,
        rect,
        enabled,
    }
}

/// The geometry of the volume slider (M14 feature 9, measured with Chromium
/// 148 at device scale 1), relative to the content box.
///
/// Video: a dark pill 112 × 36 px whose right edge is 4 px left of the mute
/// button's, with a 52 px track 16 px from its left end. Audio: a pill 101 ×
/// 32 px whose right edge is 7.5 px left of the mute button's, the track 17
/// px from its left end. The thumb (12 px) centers on the track's inner 40
/// px: the volume is the thumb center's place from 6 px after the track's
/// start (0) to 6 px before its end (1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeSlider {
    /// The pill.
    pub pill: Rect,
    /// The track (4 px high).
    pub track: Rect,
    /// The area in which the pointer keeps the slider open: the pill and
    /// the mute button.
    pub hover: Rect,
}

const SLIDER_TRACK_WIDTH: f32 = 52.0;
const THUMB_SIZE: f32 = 12.0;

/// The slider that opens at the mute button `mute` (relative to the content
/// box) of video or audio controls.
pub fn volume_slider(audio: bool, mute: Rect) -> VolumeSlider {
    let (width, height, gap, inset) = if audio {
        (101.0, 32.0, 7.5, 17.0)
    } else {
        (112.0, 36.0, 4.0, 16.0)
    };
    let center = mute.y + mute.height / 2.0;
    let pill = Rect::new(
        mute.right() - gap - width,
        center - height / 2.0,
        width,
        height,
    );
    let track = Rect::new(
        pill.x + inset,
        center - TIMELINE_HEIGHT / 2.0,
        SLIDER_TRACK_WIDTH,
        TIMELINE_HEIGHT,
    );
    // Open at the pill's left edge less 2 px (measured: 108 opens, 105 does
    // not) and over the button's whole box (for audio, its 32 px hit box).
    let left = pill.x - 2.0;
    let button = button_hit_area(audio, mute);
    let hover = Rect::new(left, button.y, button.right() - left, button.height);
    VolumeSlider { pill, track, hover }
}

/// The side of the box in which an audio button takes the pointer (measured
/// with Chromium 148 in feature 9: hover circles and clicks on the play,
/// mute and menu buttons of a 300 x 54 px panel; the boxes are 32 px, the
/// mute box ends at the menu's start). Video buttons take the pointer in
/// their 48 px box.
const AUDIO_BUTTON_HIT: f32 = 32.0;

/// The box of `button` that takes the pointer.
fn button_hit_area(audio: bool, button: Rect) -> Rect {
    if !audio {
        return button;
    }
    let (cx, cy) = (
        button.x + button.width / 2.0,
        button.y + button.height / 2.0,
    );
    Rect::new(
        cx - AUDIO_BUTTON_HIT / 2.0,
        cy - AUDIO_BUTTON_HIT / 2.0,
        AUDIO_BUTTON_HIT,
        AUDIO_BUTTON_HIT,
    )
}

/// The volume (0 to 1) at the x place `x` of the slider's thumb center.
pub fn volume_fraction(slider: &VolumeSlider, x: f32) -> f64 {
    fraction(
        slider.track.x + THUMB_SIZE / 2.0,
        slider.track.right() - THUMB_SIZE / 2.0,
        x,
    )
}

/// The place (0 to 1) on a timeline `rect` at `x`: the thumb center runs
/// from 6 px after the start to 6 px before the end (measured: a click at
/// x = 100 on the 288 px timeline of a 320 px video seeks to 28.3 % of the
/// duration, 22 px is 0, 298 px is the end).
pub fn timeline_fraction(rect: Rect, x: f32) -> f64 {
    fraction(
        rect.x + THUMB_SIZE / 2.0,
        rect.right() - THUMB_SIZE / 2.0,
        x,
    )
}

fn fraction(from: f32, to: f32, x: f32) -> f64 {
    if to <= from || x.is_nan() {
        return 0.0;
    }
    f64::from(((x - from) / (to - from)).clamp(0.0, 1.0))
}

/// What a point of the controls is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaHit {
    /// The element outside its controls (the picture).
    None,
    /// A button.
    Button(MediaButton),
    /// The timeline (its area is taller than the 4 px bar: a click 5 px
    /// below it counts).
    Timeline,
    /// The volume slider (its track and pill).
    Slider,
}

/// The part of the controls `content` that the point `local` (relative to the
/// content box) is over; the volume slider counts when `slider` is open.
pub fn media_hit(content: &MediaContent, local: Point, slider: bool) -> MediaHit {
    let part = |kind: fn(&MediaPartKind) -> bool| content.parts.iter().find(|p| kind(&p.kind));
    for (kind, button) in [
        (MediaPartKind::Play, MediaButton::Play),
        (MediaPartKind::Mute, MediaButton::Mute),
        (MediaPartKind::Fullscreen, MediaButton::Fullscreen),
        (MediaPartKind::Menu, MediaButton::Menu),
    ] {
        if content
            .parts
            .iter()
            .any(|p| p.kind == kind && button_hit_area(content.audio, p.rect).contains(local))
        {
            return MediaHit::Button(button);
        }
    }
    if slider
        && let Some(mute) = part(|k| matches!(k, MediaPartKind::Mute))
        && volume_slider(content.audio, mute.rect)
            .hover
            .contains(local)
    {
        return MediaHit::Slider;
    }
    if let Some(timeline) = part(|k| matches!(k, MediaPartKind::Timeline))
        && timeline_hit_area(timeline.rect).contains(local)
    {
        return MediaHit::Timeline;
    }
    MediaHit::None
}

/// The area of a timeline that takes the pointer: its bar and 9 px above
/// and below, 6 px (the thumb's radius) left and right.
pub(crate) fn timeline_hit_area(bar: Rect) -> Rect {
    Rect::new(
        bar.x - THUMB_SIZE / 2.0,
        bar.y - 9.0,
        bar.width + THUMB_SIZE,
        bar.height + 18.0,
    )
}

/// The label of the time display: Chromium's `m:ss` (floor of the seconds;
/// `h:mm:ss` from one hour) of the current time, and for audio and for a
/// video wide enough ("m:ss / m:ss") the duration. `current` and `duration`
/// are seconds; a duration that is not finite or negative has no part
/// (Chromium shows `0:00` and the current time).
pub fn time_label(current: f64, duration: f64, with_duration: bool) -> String {
    let current = format_time(current);
    if with_duration && duration.is_finite() && duration >= 0.0 {
        format!("{current} / {}", format_time(duration))
    } else {
        current
    }
}

/// `m:ss` of `seconds` (floor), or `h:mm:ss` from 3600 s; 0:00 for a value
/// that is negative or not finite.
fn format_time(seconds: f64) -> String {
    let total = if seconds.is_finite() && seconds > 0.0 {
        seconds.floor() as u64
    } else {
        0
    };
    let (hours, minutes, secs) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{}:{secs:02}", total / 60)
    }
}

/// The shaped text of a time display, with the font and the metrics of the
/// controls (the pen is at the left of the part's area, the baseline is
/// [`MediaText::baseline`] below its top). The engine shapes the time once a
/// second without a layout.
pub fn media_time_text(fonts: &mut FontContext, text: &str) -> (MediaText, f32) {
    let families = [FamilyName::Generic(GenericFamily::SansSerif)];
    let font = fonts.select(&FontQuery::new(&families));
    let run = fonts.shape(font, TEXT_SIZE, text, &ShapeOptions::default());
    let metrics = fonts::line_metrics(fonts, font, TEXT_SIZE);
    let text = MediaText {
        font,
        size: TEXT_SIZE,
        glyphs: Arc::from(fonts::positioned_glyphs(&run)),
        baseline: metrics.ascent,
    };
    (text, run.advance)
}

/// The time display `text` with its pen at `x` on the baseline at
/// `baseline`.
fn time(ctx: &mut LayoutContext<'_>, text: &str, x: f32, baseline: f32) -> MediaPart {
    let families = [FamilyName::Generic(GenericFamily::SansSerif)];
    let font = ctx.fonts.select(&FontQuery::new(&families));
    let run = ctx
        .fonts
        .shape(font, TEXT_SIZE, text, &ShapeOptions::default());
    let metrics = fonts::line_metrics(ctx.fonts, font, TEXT_SIZE);
    let text = MediaText {
        font,
        size: TEXT_SIZE,
        glyphs: Arc::from(fonts::positioned_glyphs(&run)),
        baseline: metrics.ascent,
    };
    let rect = Rect::new(
        x,
        baseline - metrics.ascent,
        run.advance,
        metrics.ascent + metrics.descent,
    );
    part(MediaPartKind::Time(text), rect, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{layout_html, layout_html_scripting};
    use crate::{BoxContent, FragmentRef};

    /// The media content of the first element with this tag in `html`.
    fn media_content(html: &str, tag: &str) -> MediaContent {
        media_content_in(&layout_html(html), tag)
    }

    fn media_content_in(layout: &crate::test_support::TestLayout, tag: &str) -> MediaContent {
        let mut found = None;
        layout.tree.walk(|f, _| {
            if let FragmentRef::Box(b) = f
                && found.is_none()
                && b.node
                    .and_then(|n| layout.doc.element(n))
                    .is_some_and(|e| &**e.local_name() == tag)
                && let BoxContent::Media(m) = &b.content
            {
                found = Some(MediaContent::clone(m));
            }
        });
        found.expect("the element has media content")
    }

    fn kinds(content: &MediaContent) -> Vec<(&'static str, Rect, bool)> {
        content
            .parts
            .iter()
            .map(|p| {
                let name = match p.kind {
                    MediaPartKind::Panel => "panel",
                    MediaPartKind::Play => "play",
                    MediaPartKind::Time(_) => "time",
                    MediaPartKind::Timeline => "timeline",
                    MediaPartKind::Mute => "mute",
                    MediaPartKind::Fullscreen => "fullscreen",
                    MediaPartKind::Menu => "menu",
                };
                (name, p.rect, p.enabled)
            })
            .collect()
    }

    fn names(content: &MediaContent) -> Vec<&'static str> {
        kinds(content).into_iter().map(|(n, ..)| n).collect()
    }

    /// The Wikipedia thumbnail: 250 × 159 px with sources (Chromium 148:
    /// play and menu white, mute and fullscreen dimmed).
    #[test]
    fn video_controls_at_250_by_159() {
        let c = media_content(
            "<video width=250 height=159 poster=p.jpg><source src=v.webm></video>",
            "video",
        );
        assert!(!c.default_poster);
        assert!(!c.audio);
        let parts = kinds(&c);
        let button = |x: f32| Rect::new(x, 87.0, 48.0, 48.0);
        assert_eq!(
            parts[0],
            ("panel", Rect::new(0.0, 47.0, 250.0, 112.0), true)
        );
        assert_eq!(parts[1], ("play", button(0.0), true));
        assert_eq!(parts[2].0, "time");
        assert_eq!(parts[3], ("menu", button(202.0), true));
        assert_eq!(parts[4], ("fullscreen", button(154.0), false));
        assert_eq!(parts[5], ("mute", button(106.0), false));
        assert_eq!(
            parts[6],
            ("timeline", Rect::new(16.0, 135.0, 218.0, 4.0), false)
        );
        let MediaPartKind::Time(text) = &c.parts[2].kind else {
            panic!("a time display");
        };
        // The pen at x = 48 on the baseline 43 px above the bottom.
        assert_eq!(c.parts[2].rect.x, 48.0);
        assert_eq!(c.parts[2].rect.y + text.baseline, 116.0);
        assert_eq!(text.glyphs.len(), 4);
        // 14 px Liberation Sans: "0:00" is 27.25 px wide in Chromium.
        assert!((c.parts[2].rect.width - 27.25).abs() < 0.05);
    }

    /// Thresholds measured with Chromium 148 (a height of 159 px).
    #[test]
    fn narrow_videos_leave_out_parts() {
        let at = |w: u32| {
            names(&media_content(
                &format!("<video width={w} height=159></video>"),
                "video",
            ))
        };
        assert_eq!(at(120), ["panel", "menu", "timeline"]);
        assert_eq!(at(122), ["panel", "play", "menu", "timeline"]);
        assert_eq!(at(170), ["panel", "play", "menu", "mute", "timeline"]);
        assert_eq!(
            at(198),
            ["panel", "play", "menu", "fullscreen", "mute", "timeline"]
        );
        assert_eq!(
            at(246),
            [
                "panel",
                "play",
                "time",
                "menu",
                "fullscreen",
                "mute",
                "timeline"
            ]
        );
        assert_eq!(at(30), ["panel", "menu"]);
    }

    #[test]
    fn low_videos_move_or_leave_out_parts() {
        let at = |h: u32| {
            kinds(&media_content(
                &format!("<video width=250 height={h}></video>"),
                "video",
            ))
        };
        // Full controls from 72 px: the row's center 48 px above the
        // bottom.
        let full = at(72);
        assert_eq!(full[1], ("play", Rect::new(0.0, 0.0, 48.0, 48.0), false));
        assert_eq!(full.last().map(|p| p.0), Some("timeline"));
        // Below, the row's center is 24 px above the bottom, without
        // the timeline.
        let row = at(71);
        assert_eq!(row[1], ("play", Rect::new(0.0, 23.0, 48.0, 48.0), false));
        assert!(row.iter().all(|p| p.0 != "timeline"));
        assert_eq!(at(49)[1].0, "play");
        let names = |parts: Vec<(&'static str, Rect, bool)>| -> Vec<&'static str> {
            parts.into_iter().map(|p| p.0).collect()
        };
        assert_eq!(names(at(48)), ["panel", "timeline"]);
        assert_eq!(names(at(24)), ["panel", "timeline"]);
        assert_eq!(names(at(23)), ["panel"]);
    }

    #[test]
    fn default_poster_and_sources() {
        let c = media_content("<video></video>", "video");
        assert!(c.default_poster);
        assert!(!c.parts[1].enabled, "play without a source is dimmed");
        let c = media_content("<video poster=' ' src=v.webm></video>", "video");
        assert!(c.default_poster, "a blank poster counts as none");
        assert!(c.parts[1].enabled);
        let c = media_content("<video><source></video>", "video");
        assert!(!c.parts[1].enabled, "a source without src is no source");
    }

    /// With scripting, a video shows controls only with the `controls`
    /// attribute, and paints no default poster without them.
    #[test]
    fn video_controls_follow_the_scripting_flag() {
        let html = "<video width=300 height=150></video>";
        let off = media_content(html, "video");
        assert!(off.default_poster);
        assert!(off.parts.len() > 1, "controls with scripting off");
        let off_attr = media_content("<video controls></video>", "video");
        assert_eq!(off_attr.parts, off.parts);

        let on = media_content_in(&layout_html_scripting(html), "video");
        assert!(on.parts.is_empty(), "no controls");
        assert!(!on.default_poster, "no dark fill without controls");
        let on_attr = media_content_in(
            &layout_html_scripting("<video width=300 height=150 controls></video>"),
            "video",
        );
        assert_eq!(on_attr.parts, off.parts);
        assert!(on_attr.default_poster);
        let audio = media_content_in(&layout_html_scripting("<audio controls></audio>"), "audio");
        assert!(audio.parts.len() > 1);
    }

    /// Chromium 148's strings (probe `media-controls-sizes.json`, a 1 h 2 min
    /// 5 s file): `m:ss`, and `h:mm:ss` from one hour, for each number.
    #[test]
    fn the_time_label_is_chromiums() {
        let at = |t: f64| time_label(t, 3725.0, true);
        assert_eq!(at(0.0), "0:00 / 1:02:05");
        assert_eq!(at(3700.0), "1:01:40 / 1:02:05");
        assert_eq!(at(610.0), "10:10 / 1:02:05");
        assert_eq!(at(3599.5), "59:59 / 1:02:05");
        assert_eq!(time_label(1.9, 10.0, true), "0:01 / 0:10");
        assert_eq!(time_label(1.9, 10.0, false), "0:01");
        assert_eq!(time_label(9.99, 10.0, true), "0:09 / 0:10");
        // A duration that is not a finite number has no part; a position that
        // is negative or not a number is 0:00.
        assert_eq!(time_label(5.0, f64::INFINITY, true), "0:05");
        assert_eq!(time_label(5.0, f64::NAN, true), "0:05");
        assert_eq!(time_label(f64::NAN, 10.0, true), "0:00 / 0:10");
        assert_eq!(time_label(-3.0, 10.0, true), "0:00 / 0:10");
        assert_eq!(time_label(f64::INFINITY, 10.0, true), "0:00 / 0:10");
        // A huge number does not panic (it saturates).
        assert!(time_label(1e300, 1e300, true).contains(" / "));
    }

    /// The thumb's center maps 6 px inside the bar's ends (measured: a click
    /// at x = 100, 120, 150, 200, 240 on a 320 px video gives 2.83, 3.55,
    /// 4.64, 6.45 and 7.9 s of 10).
    #[test]
    fn the_timeline_maps_the_thumb_center() {
        let bar = Rect::new(16.0, 216.0, 288.0, 4.0);
        let seconds = |x: f32| (timeline_fraction(bar, x) * 10.0 * 100.0).round() / 100.0;
        assert_eq!(seconds(100.0), 2.83);
        assert_eq!(seconds(120.0), 3.55);
        assert_eq!(seconds(150.0), 4.64);
        assert_eq!(seconds(200.0), 6.45);
        assert_eq!(seconds(240.0), 7.90);
        assert_eq!(seconds(0.0), 0.0);
        assert_eq!(seconds(390.0), 10.0);
        assert_eq!(timeline_fraction(bar, f32::NAN), 0.0);
        // A bar narrower than the thumb has no range.
        assert_eq!(timeline_fraction(Rect::new(0.0, 0.0, 10.0, 4.0), 5.0), 0.0);
    }

    /// The volume slider of a 320 px video (measured: 126 and less is 0, 140
    /// is 0.25, 150 is 0.5, 160 is 0.75, 170 and more is 1) and what the
    /// pointer is over.
    #[test]
    fn the_volume_slider_maps_and_hits() {
        let c = media_content("<video width=320 height=240 src=v.webm></video>", "video");
        let mute = c
            .parts
            .iter()
            .find(|p| p.kind == MediaPartKind::Mute)
            .unwrap();
        let s = volume_slider(false, mute.rect);
        assert_eq!(s.pill, Rect::new(108.0, 174.0, 112.0, 36.0));
        assert_eq!(s.track, Rect::new(124.0, 190.0, 52.0, 4.0));
        let v = |x: f32| (volume_fraction(&s, x) * 100.0).round() / 100.0;
        assert_eq!(
            [v(126.0), v(140.0), v(150.0), v(160.0), v(172.0)],
            [0.0, 0.25, 0.5, 0.75, 1.0]
        );
        let hit = |x: f32, y: f32, open: bool| media_hit(&c, Point::new(x, y), open);
        assert_eq!(
            hit(200.0, 192.0, false),
            MediaHit::Button(MediaButton::Mute)
        );
        assert_eq!(hit(150.0, 192.0, false), MediaHit::None);
        assert_eq!(hit(150.0, 192.0, true), MediaHit::Slider);
        assert_eq!(hit(105.0, 192.0, true), MediaHit::None);
        assert_eq!(hit(24.0, 192.0, true), MediaHit::Button(MediaButton::Play));
        assert_eq!(hit(160.0, 225.0, false), MediaHit::Timeline);
        assert_eq!(hit(160.0, 100.0, false), MediaHit::None);
    }

    #[test]
    fn audio_controls_at_the_default_size() {
        let c = media_content("<audio controls src=a.mp3></audio>", "audio");
        assert!(c.audio);
        assert!(!c.default_poster);
        let parts = kinds(&c);
        assert_eq!(parts[0], ("panel", Rect::new(0.0, 0.0, 300.0, 54.0), true));
        assert_eq!(parts[1], ("play", Rect::new(2.0, 3.0, 48.0, 48.0), true));
        assert_eq!(parts[2].0, "time");
        assert_eq!(parts[3].0, "timeline");
        assert_eq!(parts[4], ("mute", Rect::new(217.5, 3.0, 48.0, 48.0), false));
        assert_eq!(parts[5], ("menu", Rect::new(250.0, 3.0, 48.0, 48.0), true));
        let timeline = parts[3].1;
        assert_eq!(timeline.right(), 210.0);
        assert!((timeline.x - 129.0).abs() < 1.5, "{timeline:?}");
        // swb's rules (not measured): at 150 px the time and the timeline
        // do not fit; Chromium also shows play, mute and the menu there.
        let narrow = media_content("<audio controls style='width:150px'></audio>", "audio");
        assert_eq!(names(&narrow), ["panel", "play", "mute", "menu"]);
        let c = media_content("<audio controls></audio>", "audio");
        assert!(!c.parts[1].enabled, "play without a source");
        assert!(c.parts.last().is_some_and(|p| p.enabled), "the menu");
    }

    /// The 32 px hit boxes of the audio buttons (Chromium 148: clicks on
    /// `hit-audio-mute-clicks` and `hit-audio-play-clicks`, hover circles of
    /// `hit-audio-hover` in `tools/probes/media-controls-hit.json`).
    #[test]
    fn audio_buttons_take_the_pointer_in_boxes_of_32_px() {
        let c = media_content("<audio controls src=a.mp3></audio>", "audio");
        let hit = |x: f32, y: f32, open: bool| media_hit(&c, Point::new(x, y), open);
        let (play, mute, menu) = (
            MediaHit::Button(MediaButton::Play),
            MediaHit::Button(MediaButton::Mute),
            MediaHit::Button(MediaButton::Menu),
        );
        // Play: x 10 to 42 and y 11 to 43.
        assert_eq!(hit(9.0, 27.0, false), MediaHit::None);
        assert_eq!([hit(10.0, 27.0, false), hit(41.0, 27.0, false)], [play; 2]);
        assert_eq!(hit(42.0, 27.0, false), MediaHit::None);
        assert_eq!(hit(26.0, 10.0, false), MediaHit::None);
        assert_eq!([hit(26.0, 11.0, false), hit(26.0, 42.0, false)], [play; 2]);
        assert_eq!(hit(26.0, 43.0, false), MediaHit::None);
        // Mute: x 225.5 to 257.5; the menu starts at 258 (no overlap).
        assert_eq!(hit(222.0, 27.0, false), MediaHit::None);
        assert_eq!(
            [hit(226.0, 27.0, false), hit(257.0, 27.0, false)],
            [mute; 2]
        );
        assert_eq!(
            [hit(258.0, 27.0, false), hit(289.0, 27.0, false)],
            [menu; 2]
        );
        assert_eq!(hit(290.0, 27.0, false), MediaHit::None);
        assert_eq!(hit(241.0, 10.0, false), MediaHit::None);
        assert_eq!(hit(241.0, 43.0, false), MediaHit::None);
        // The open slider's area ends at the mute box too.
        assert_eq!(hit(180.0, 27.0, true), MediaHit::Slider);
        assert_eq!(hit(258.0, 27.0, true), menu);
    }
}
