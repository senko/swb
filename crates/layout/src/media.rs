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
//! or if scripting is disabled for the media element". swb runs no
//! scripts, so every video has controls, as in Chromium with JavaScript
//! disabled; audio without `controls` is `display: none` (user-agent
//! style sheet). The controls approximate Chromium 148's (measured on
//! Linux at device scale 1; the parts that paint draws are described in
//! `paint/src/media.rs`).
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
//! timeline, mute (centered 55.5 px from the right edge) and the menu
//! (centered 26 px from the right edge), as measured at the default size
//! (300 × 54 px). The menu is enabled also without a media resource (as
//! measured). The time shows if the timeline after it stays at least
//! 32 px wide, and mute if it does not overlap play (swb's rules; Chromium
//! was not measured at other sizes).

use std::sync::Arc;

use swb_dom::{Document, NodeId, is_html_whitespace, local_name};
use swb_text::{FamilyName, FontQuery, GenericFamily, ShapeOptions};

use crate::LayoutContext;
use crate::fonts;
use crate::fragment::{MediaContent, MediaPart, MediaPartKind, MediaText};
use crate::geom::{Rect, Size};

/// The size of the buttons of media controls.
const BUTTON: f32 = 48.0;

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
        default_poster: !media.audio && !media.poster,
        audio: media.audio,
        parts,
    }
}

fn video_parts(ctx: &mut LayoutContext<'_>, media: Media, size: Size) -> Vec<MediaPart> {
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
/// 26 px and 55.5 px from the right edge, and the timeline from 16 px
/// after the time to 90 px from the right edge.
fn audio_parts(ctx: &mut LayoutContext<'_>, media: Media, size: Size) -> Vec<MediaPart> {
    const PLAY_X: f32 = 2.0;
    const TIME_X: f32 = 47.0;
    const MENU_CENTER_FROM_RIGHT: f32 = 26.0;
    const MUTE_CENTER_FROM_RIGHT: f32 = 55.5;
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
    use crate::test_support::layout_html;
    use crate::{BoxContent, FragmentRef};

    /// The media content of the first element with this tag in `html`.
    fn media_content(html: &str, tag: &str) -> MediaContent {
        let layout = layout_html(html);
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
        assert_eq!(parts[4], ("mute", Rect::new(220.5, 3.0, 48.0, 48.0), false));
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
}
