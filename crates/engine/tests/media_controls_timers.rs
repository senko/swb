//! M14 feature 9 (ADR 0047 "Implementation notes (M14 feature 9)"): the
//! timers and the edges of the default controls. In virtual time: the
//! auto-hide (2.5 s after the last move while playing, a fade of 0.7 s), the
//! leave, paused and ended controls that stay, the drags that leave the video
//! or the window, an element that goes during a drag, a click on the
//! controls of an element in error, a drag on a stream that cannot seek, the
//! state changes with JavaScript off, and the cost (no layout; no wake-up
//! when nothing changes). In real time (`set_paints_frames(true)`, the GUI's
//! loop): the time text changes once a second without a layout, and the
//! controls cause no wake-up while hidden. The tests need the feature
//! `media` (`just test-media`).

#![cfg(feature = "media")]
// Test helpers outside `#[test]` functions unwrap too.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::frame_loop::drive;
use common::media_probe::evaluate;
use common::media_site::{FOLDER, MediaSite, RangeMode, VIEW, open_page, real_page};
use common::node;
use swb_engine::{ClockMode, Modifiers, MouseButton, Page, Pixmap};

/// 320 x 240, 25 fps, 10 s, a flat picture, with AAC.
const FLAT: &str = "fixtures/media/mp4-h264-aac-flat-long.mp4";

fn site_with(body: &str) -> MediaSite {
    let mut site = MediaSite::new(
        FOLDER,
        &format!(
            "<!DOCTYPE html><style>body{{margin:0;background:#fff}}video{{display:block}}</style>{body}"
        ),
    );
    site.add_file("flat.mp4", FLAT);
    site.add_file("a.mp4", "fixtures/media/mp4-h264-high-aac.mp4");
    site
}

const VIDEO: &str = "<video id=v src=flat.mp4 controls preload=auto></video>";

/// A page in virtual time (JavaScript `js`) whose media have loaded.
fn page_of(site: MediaSite, js: bool) -> (Page, Arc<MediaSite>) {
    let (mut page, site) = open_page(site, VIEW, ClockMode::Virtual, |page| {
        page.set_javascript(js);
    });
    wait(&mut page, 900);
    (page, site)
}

fn wait(page: &mut Page, ms: u64) {
    page.run_event_loop(Duration::from_millis(ms), Duration::from_secs(60));
}

fn shot(page: &mut Page) -> Pixmap {
    page.screenshot(false).unwrap()
}

/// The red channel of the pixel of the picture at (x, y): the flat picture
/// is 51, the controls' gradient darkens it to 0 at the bottom.
fn red(page: &mut Page, x: u32, y: u32) -> u8 {
    shot(page).pixel(x, y).unwrap().demultiply().red()
}

/// The bottom-left pixel of the video: black under the controls, the
/// picture's 51 when they are hidden.
const GRADIENT: (u32, u32) = (10, 236);

/// The red channel of the flat picture as swb converts it (Chromium: 51).
const FLAT_RED: f32 = 49.0;

fn opacity_of(page: &mut Page) -> f32 {
    1.0 - f32::from(red(page, GRADIENT.0, GRADIENT.1)) / FLAT_RED
}

fn info(page: &mut Page) -> swb_web::MediaInfo {
    let v = node(page, "v");
    page.media_info(v).unwrap()
}

fn click(page: &mut Page, x: f32, y: f32) {
    page.click(x, y);
    wait(page, 1);
}

#[test]
fn the_controls_of_a_playing_video_hide_2_5_s_after_the_last_move_and_fade() {
    let (mut page, _site) = page_of(site_with(VIDEO), true);
    page.note_user_activation();
    click(&mut page, 160.0, 100.0);
    assert!(!info(&mut page).paused);
    wait(&mut page, 1500);
    assert!(
        opacity_of(&mut page) > 0.97,
        "shown while the pointer is in"
    );
    // 2.4 s after the last move: still shown.
    wait(&mut page, 900);
    assert!(opacity_of(&mut page) > 0.97, "shown at 2.4 s");
    // The fade runs 0.7 s from 2.5 s: half way at 2.85 s.
    wait(&mut page, 450);
    let half = opacity_of(&mut page);
    assert!(
        (0.3..0.8).contains(&half),
        "fading at 2.85 s: opacity {half}"
    );
    wait(&mut page, 500);
    assert!(opacity_of(&mut page) < 0.03, "hidden at 3.35 s");
    // A move shows them again at once (a fade in of 0.1 s).
    page.mouse_move(162.0, 102.0);
    wait(&mut page, 150);
    assert!(opacity_of(&mut page) > 0.97, "shown after a move");
}

#[test]
fn a_repeated_pointer_position_does_not_show_hidden_controls() {
    let (mut page, _site) = page_of(site_with(VIDEO), true);
    page.note_user_activation();
    click(&mut page, 160.0, 100.0);
    wait(&mut page, 3500);
    assert!(opacity_of(&mut page) < 0.03);
    // The same point again (a refresh of the hover after a layout) is no
    // move.
    page.mouse_move(160.0, 100.0);
    wait(&mut page, 300);
    assert!(opacity_of(&mut page) < 0.03, "no move, no controls");
}

#[test]
fn a_leave_fades_the_controls_at_once() {
    let (mut page, _site) = page_of(site_with(VIDEO), true);
    page.note_user_activation();
    click(&mut page, 160.0, 100.0);
    wait(&mut page, 700);
    assert!(opacity_of(&mut page) > 0.97);
    page.mouse_move(395.0, 295.0);
    wait(&mut page, 100);
    let early = opacity_of(&mut page);
    assert!(
        (0.5..0.97).contains(&early),
        "fading 0.1 s after the leave: {early}"
    );
    wait(&mut page, 700);
    assert!(opacity_of(&mut page) < 0.03, "hidden after 0.8 s");
}

#[test]
fn a_pointer_that_leaves_the_window_fades_the_controls_too() {
    let (mut page, _site) = page_of(site_with(VIDEO), true);
    page.note_user_activation();
    click(&mut page, 160.0, 100.0);
    wait(&mut page, 700);
    assert!(opacity_of(&mut page) > 0.97);
    page.mouse_leave();
    wait(&mut page, 100);
    let early = opacity_of(&mut page);
    assert!(
        (0.5..0.97).contains(&early),
        "fading after the leave: {early}"
    );
    wait(&mut page, 700);
    assert!(opacity_of(&mut page) < 0.03, "hidden after 0.8 s");
}

#[test]
fn paused_and_ended_controls_stay_shown() {
    let (mut page, _site) = page_of(site_with(VIDEO), true);
    page.note_user_activation();
    click(&mut page, 160.0, 100.0);
    wait(&mut page, 700);
    click(&mut page, 160.0, 100.0);
    assert!(info(&mut page).paused);
    wait(&mut page, 10_000);
    assert!(opacity_of(&mut page) > 0.97, "paused: shown");
    // Out of the video too.
    page.mouse_move(395.0, 295.0);
    wait(&mut page, 5000);
    assert!(opacity_of(&mut page) > 0.97, "paused, pointer out: shown");
    // To the end: they stay.
    click(&mut page, 160.0, 100.0);
    wait(&mut page, 11_000);
    assert!(info(&mut page).ended);
    assert!(opacity_of(&mut page) > 0.97, "ended: shown");
}

#[test]
fn the_controls_cause_no_wake_up_when_nothing_changes() {
    let (mut page, _site) = page_of(site_with(VIDEO), true);
    page.note_user_activation();
    // Paused with the pointer still, inside the video.
    page.mouse_move(100.0, 100.0);
    wait(&mut page, 3000);
    let mut target = Pixmap::new(800, 600).unwrap();
    page.render(&mut target);
    assert_eq!(page.media_controls_due(), None, "paused, pointer still");
    // Playing with hidden controls.
    click(&mut page, 160.0, 100.0);
    wait(&mut page, 4000);
    page.render(&mut target);
    assert!(opacity_of(&mut page) < 0.03);
    assert_eq!(page.media_controls_due(), None, "hidden controls");
    // Playing and shown: the auto-hide is due.
    page.mouse_move(120.0, 100.0);
    assert!(page.media_controls_due().is_some(), "the auto-hide is due");
}

#[test]
fn a_change_of_the_controls_does_not_lay_the_page_out_again() {
    let (mut page, _site) = page_of(site_with(VIDEO), true);
    page.note_user_activation();
    let mut target = Pixmap::new(800, 600).unwrap();
    // A press on a control moves the focus to the element (a style change,
    // so a layout): the first press does it, and mute twice restores the state.
    click(&mut page, 200.0, 192.0);
    click(&mut page, 200.0, 192.0);
    page.render(&mut target);
    let before = page.flush_counters();
    // Hover, play, the time text and the thumb, the fade, a mute and a
    // volume change: every picture of the controls.
    page.mouse_move(24.0, 192.0);
    page.render(&mut target);
    click(&mut page, 24.0, 192.0);
    for _ in 0..40 {
        wait(&mut page, 100);
        page.render(&mut target);
    }
    page.mouse_move(160.0, 225.0);
    page.render(&mut target);
    click(&mut page, 200.0, 192.0);
    page.render(&mut target);
    let after = page.flush_counters();
    assert_eq!(
        after.layout_runs, before.layout_runs,
        "the controls' pictures run no layout"
    );
    assert!(
        after.display_list_runs > before.display_list_runs,
        "the display list is built again"
    );
}

#[test]
fn a_drag_on_the_timeline_follows_the_pointer_out_of_the_video_and_the_window() {
    let (mut page, _site) = page_of(site_with(VIDEO), true);
    page.note_user_activation();
    page.mouse_move(100.0, 225.0);
    page.mouse_down(100.0, 225.0, MouseButton::Primary, Modifiers::NONE, 1);
    wait(&mut page, 300);
    assert!((info(&mut page).current_time - 2.83).abs() < 0.1);
    // Out of the video, right and below.
    page.mouse_move(700.0, 580.0);
    wait(&mut page, 300);
    assert!(
        (info(&mut page).current_time - 10.0).abs() < 0.01,
        "clamped to the end"
    );
    // Left of the video, outside the window's area.
    page.mouse_move(-50.0, 300.0);
    wait(&mut page, 300);
    assert!(info(&mut page).current_time < 0.01, "clamped to 0");
    page.mouse_up(-50.0, 300.0, MouseButton::Primary);
    wait(&mut page, 300);
    let i = info(&mut page);
    assert!(i.paused && i.current_time < 0.01);
    // A move after the release does not seek.
    page.mouse_move(250.0, 225.0);
    wait(&mut page, 300);
    assert!(info(&mut page).current_time < 0.01);
}

#[test]
fn a_drag_of_10_000_moves_ends_in_the_last_position() {
    let (mut page, _site) = page_of(site_with(VIDEO), true);
    page.note_user_activation();
    page.mouse_move(22.0, 225.0);
    page.mouse_down(22.0, 225.0, MouseButton::Primary, Modifiers::NONE, 1);
    for i in 0..10_000 {
        let x = 22.0 + (i % 276) as f32;
        page.mouse_move(x, 225.0);
    }
    page.mouse_move(160.0, 225.0);
    page.mouse_up(160.0, 225.0, MouseButton::Primary);
    wait(&mut page, 500);
    let i = info(&mut page);
    assert!((i.current_time - 5.0).abs() < 0.05, "{}", i.current_time);
    assert!(i.paused);
}

#[test]
fn an_element_that_goes_during_a_drag_ends_the_drag_without_a_panic() {
    let (mut page, _site) = page_of(site_with(VIDEO), true);
    page.note_user_activation();
    page.mouse_move(100.0, 225.0);
    page.mouse_down(100.0, 225.0, MouseButton::Primary, Modifiers::NONE, 1);
    wait(&mut page, 100);
    evaluate(&mut page, "document.getElementById('v').remove(); 1");
    page.mouse_move(200.0, 225.0);
    page.mouse_up(200.0, 225.0, MouseButton::Primary);
    wait(&mut page, 100);
    // The page goes on: the next press works on a new element.
    evaluate(
        &mut page,
        "document.body.insertAdjacentHTML('beforeend', '<video id=w src=flat.mp4 controls preload=auto></video>'); 1",
    );
    wait(&mut page, 900);
    page.mouse_move(100.0, 225.0);
    page.mouse_down(100.0, 225.0, MouseButton::Primary, Modifiers::NONE, 1);
    page.mouse_up(100.0, 225.0, MouseButton::Primary);
    wait(&mut page, 300);
    let w = node(&page, "w");
    let t = page.media_info(w).unwrap().current_time;
    assert!((t - 2.83).abs() < 0.1, "{t}");
}

#[test]
fn a_source_that_changes_during_a_drag_ends_the_drag() {
    let (mut page, _site) = page_of(site_with(VIDEO), true);
    page.note_user_activation();
    page.mouse_move(100.0, 225.0);
    page.mouse_down(100.0, 225.0, MouseButton::Primary, Modifiers::NONE, 1);
    wait(&mut page, 100);
    evaluate(&mut page, "document.getElementById('v').src = 'a.mp4'; 1");
    wait(&mut page, 900);
    page.mouse_move(200.0, 225.0);
    wait(&mut page, 100);
    page.mouse_up(200.0, 225.0, MouseButton::Primary);
    wait(&mut page, 300);
    let i = info(&mut page);
    // The new load starts at 0: the drag's moves did not seek it.
    assert!(i.current_time < 0.01, "{}", i.current_time);
    assert!((i.duration - 2.0).abs() < 0.1);
}

#[test]
fn a_click_on_the_controls_of_an_element_in_error_does_nothing() {
    let (mut page, _site) = page_of(
        site_with("<video id=v width=320 height=240 src=missing.mp4 controls></video>"),
        true,
    );
    page.note_user_activation();
    for (x, y) in [
        (24.0, 192.0),
        (160.0, 225.0),
        (200.0, 192.0),
        (160.0, 100.0),
    ] {
        click(&mut page, x, y);
    }
    let i = info(&mut page);
    assert!(i.paused && !i.muted);
    assert_eq!(i.error.map(|e| e.code), Some(4));
    assert!(i.current_time.abs() < f64::EPSILON);
}

#[test]
fn a_drag_on_a_stream_that_cannot_seek_does_not_seek() {
    let mut site = site_with(VIDEO);
    site.mode = RangeMode::Ignore;
    let (mut page, _site) = page_of(site, true);
    page.note_user_activation();
    let seekable = info(&mut page).seekable;
    page.mouse_move(100.0, 225.0);
    page.mouse_down(100.0, 225.0, MouseButton::Primary, Modifiers::NONE, 1);
    page.mouse_move(200.0, 225.0);
    page.mouse_up(200.0, 225.0, MouseButton::Primary);
    wait(&mut page, 300);
    let i = info(&mut page);
    if seekable.iter().any(|&(a, b)| b > a) {
        // A source that was read whole can seek.
        assert!(i.current_time > 5.0);
    } else {
        assert!(i.current_time < 0.01, "{}", i.current_time);
    }
}

#[test]
fn a_duration_that_is_not_finite_or_a_media_without_metadata_cannot_seek() {
    let (mut page, _site) = page_of(
        site_with("<video id=v width=320 height=240 src=flat.mp4 controls preload=none></video>"),
        true,
    );
    page.note_user_activation();
    // No metadata (`preload=none`): the timeline, the mute button and the
    // picture do nothing.
    for (x, y) in [(160.0, 225.0), (200.0, 192.0), (160.0, 100.0)] {
        click(&mut page, x, y);
    }
    let i = info(&mut page);
    assert!(i.paused && !i.muted && i.current_time.abs() < f64::EPSILON);
    // The play button starts the load and plays.
    click(&mut page, 24.0, 192.0);
    wait(&mut page, 1200);
    assert!(!info(&mut page).paused);
}

#[test]
fn the_actions_work_with_javascript_off() {
    let (mut page, _site) = page_of(site_with(VIDEO), false);
    // Play by the picture and by the button, mute, volume, seek.
    click(&mut page, 160.0, 100.0);
    wait(&mut page, 700);
    assert!(!info(&mut page).paused, "a click on the picture plays");
    click(&mut page, 24.0, 192.0);
    assert!(info(&mut page).paused, "the play button pauses");
    click(&mut page, 200.0, 192.0);
    assert!(info(&mut page).muted, "the mute button mutes");
    click(&mut page, 200.0, 192.0);
    assert!(!info(&mut page).muted);
    page.mouse_move(200.0, 192.0);
    wait(&mut page, 100);
    click(&mut page, 150.0, 192.0);
    assert!((info(&mut page).volume - 0.5).abs() < 0.02);
    click(&mut page, 240.0, 225.0);
    wait(&mut page, 300);
    assert!((info(&mut page).current_time - 7.9).abs() < 0.1);
    // Drag the timeline.
    page.mouse_move(100.0, 225.0);
    page.mouse_down(100.0, 225.0, MouseButton::Primary, Modifiers::NONE, 1);
    page.mouse_move(160.0, 225.0);
    page.mouse_up(160.0, 225.0, MouseButton::Primary);
    wait(&mut page, 300);
    assert!((info(&mut page).current_time - 5.0).abs() < 0.1);
}

#[test]
fn a_click_on_the_picture_that_a_listener_cancels_does_not_play() {
    let (mut page, _site) = page_of(
        site_with(&format!(
            "{VIDEO}<script>v.addEventListener('click', e => e.preventDefault())</script>"
        )),
        true,
    );
    page.note_user_activation();
    click(&mut page, 160.0, 100.0);
    wait(&mut page, 300);
    assert!(info(&mut page).paused, "canceled click: no play");
    // The play button is not the page's: it works.
    click(&mut page, 24.0, 192.0);
    wait(&mut page, 300);
    assert!(!info(&mut page).paused);
}

#[test]
fn a_press_on_a_control_gives_the_page_no_pointer_event() {
    let (mut page, _site) = page_of(
        site_with(&format!(
            "{VIDEO}<script>var log = []; for (const n of ['pointerdown','mousedown','pointerup','mouseup','click']) document.addEventListener(n, () => log.push(n), true)</script>"
        )),
        true,
    );
    page.note_user_activation();
    click(&mut page, 24.0, 192.0);
    click(&mut page, 200.0, 192.0);
    click(&mut page, 160.0, 225.0);
    assert_eq!(evaluate(&mut page, "log.length"), serde_json::json!(0));
    click(&mut page, 160.0, 100.0);
    assert_eq!(
        evaluate(&mut page, "log.join()"),
        serde_json::json!("pointerdown,mousedown,pointerup,mouseup,click")
    );
}

/// The volume slider unmutes a muted element (Chromium 148, `in-slider-muted-*`
/// of the probe file compare the events with scripts). With JavaScript off.
#[test]
fn the_volume_slider_unmutes_a_muted_element_with_javascript_off() {
    let (mut page, _site) = page_of(site_with(VIDEO), false);
    click(&mut page, 200.0, 192.0);
    assert!(info(&mut page).muted, "the mute button mutes");
    // The slider is open (the focus keeps it open): a press at x = 150.
    click(&mut page, 150.0, 192.0);
    let now = info(&mut page);
    assert!(!now.muted, "the slider unmuted it");
    assert!((now.volume - 0.5).abs() < 0.02, "{now:?}");
    // At volume 0 a press at 0 on a muted element does nothing.
    click(&mut page, 200.0, 192.0);
    assert!(info(&mut page).muted);
    click(&mut page, 124.0, 192.0);
    let now = info(&mut page);
    assert!(
        now.muted && now.volume < 0.01,
        "a press at 0 does not unmute: {now:?}"
    );
}

/// A press on a control focuses the media element as a press on the picture
/// does (Chromium 148, `in-focus-*` of the probe file; the cases with scripts
/// are compared there, with the focus events). Here with JavaScript off.
#[test]
fn a_press_on_a_control_moves_the_focus_to_the_element_with_javascript_off() {
    let (mut page, _site) = page_of(
        site_with(&format!(
            "<input id=i style='position:absolute;left:0;top:250px;width:100px'>{VIDEO}"
        )),
        false,
    );
    let (input, video) = (node(&page, "i"), node(&page, "v"));
    for (name, x, y) in [
        ("the play button", 24.0, 192.0),
        ("the timeline", 240.0, 225.0),
        ("the mute button", 200.0, 192.0),
        ("the picture", 160.0, 100.0),
    ] {
        click(&mut page, 50.0, 255.0);
        assert_eq!(page.focused_element(), Some(input), "the input is focused");
        click(&mut page, x, y);
        assert_eq!(page.focused_element(), Some(video), "a press on {name}");
    }
}

// ----- Real time: the GUI's loop -----

#[test]
fn the_time_text_changes_once_a_second_without_a_layout() {
    let mut page = real_page(site_with(
        "<video id=v src=flat.mp4 controls preload=auto muted autoplay></video>",
    ));
    let mut target = Pixmap::new(800, 600).unwrap();
    drive(&mut page, &mut target, Duration::from_millis(1500));
    page.mouse_move(160.0, 100.0);
    let before = page.flush_counters();
    // The video is shown for 2.5 s after the move; the position moves on.
    let started = info(&mut page).current_time;
    let run = drive(&mut page, &mut target, Duration::from_millis(2300));
    let after = page.flush_counters();
    assert!(info(&mut page).current_time - started > 1.5);
    assert_eq!(after.layout_runs, before.layout_runs, "no layout");
    assert!(run.renders >= 2, "the pictures were drawn: {run:?}");
    assert_eq!(run.spins, 0, "{run:?}");
}

/// The red and green channels of the time text and the timeline of the
/// audio controls (a 300 x 54 panel at the top left).
fn audio_row(target: &Pixmap) -> Vec<u8> {
    let mut row = Vec::new();
    for y in 18..38 {
        for x in 44..215 {
            let pixel = target.pixel(x, y).unwrap().demultiply();
            row.extend([pixel.red(), pixel.green()]);
        }
    }
    row
}

#[test]
fn a_playing_audio_repaints_its_time_and_timeline_without_a_layout() {
    let mut site = site_with("<audio id=v src=a.mp3 controls preload=auto muted></audio>");
    site.add_file("a.mp3", "fixtures/media/mp3-long.mp3");
    let mut page = real_page(site);
    let mut target = Pixmap::new(800, 600).unwrap();
    drive(&mut page, &mut target, Duration::from_millis(1000));
    // The play button of the panel.
    page.click(26.0, 27.0);
    drive(&mut page, &mut target, Duration::from_millis(1500));
    let before = page.flush_counters();
    let first = audio_row(&target);
    let started = info(&mut page).current_time;
    let run = drive(&mut page, &mut target, Duration::from_millis(2300));
    let after = page.flush_counters();
    let now = info(&mut page);
    assert!(now.current_time - started > 1.5, "{started} {now:?}");
    assert_eq!(after.layout_runs, before.layout_runs, "no layout");
    assert!(run.renders >= 2, "the pictures were drawn: {run:?}");
    assert_ne!(
        audio_row(&target),
        first,
        "the time text and the played part changed"
    );
    assert_eq!(run.spins, 0, "{run:?}");
}

/// The pixels of the picture's ring area (above the controls' gradient)
/// that are almost black: the loading arc on the default poster (#333).
fn dark_pixels(target: &Pixmap) -> usize {
    let mut count = 0;
    for y in 76..120 {
        for x in 128..192 {
            if target.pixel(x, y).unwrap().demultiply().red() < 20 {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn the_loading_arc_shows_between_0_1_and_1_1_s_on_a_page_that_paints_frames() {
    let mut page = real_page(site_with(
        "<video id=v width=320 height=240 src=flat.mp4 controls preload=none></video>",
    ));
    let mut target = Pixmap::new(800, 600).unwrap();
    let begin = Instant::now();
    let mut peak = 0;
    let mut late = 0;
    while begin.elapsed() < Duration::from_millis(1800) {
        page.process_network();
        page.pump();
        if page.frame_outdated() {
            page.render_update(&mut target);
        }
        let dark = dark_pixels(&target);
        if begin.elapsed() > Duration::from_millis(1500) {
            late = late.max(dark);
        } else {
            peak = peak.max(dark);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(peak > 100, "the arc shows: {peak} dark pixels");
    assert_eq!(late, 0, "and goes after 1.1 s");
    // Headless pictures have none.
    let (mut headless, _site) = page_of(
        site_with("<video id=v width=320 height=240 src=flat.mp4 controls preload=none></video>"),
        true,
    );
    let shot = headless.screenshot(false).unwrap();
    assert_eq!(dark_pixels(&shot), 0, "no arc without frames");
}

#[test]
fn hidden_controls_make_a_playing_video_wake_the_loop_no_more_than_the_video_does() {
    let with = "<video id=v src=flat.mp4 controls preload=auto muted autoplay></video>";
    let without = "<video id=v src=flat.mp4 preload=auto muted autoplay></video>";
    let mut renders = Vec::new();
    for body in [with, without] {
        let mut page = real_page(site_with(body));
        let mut target = Pixmap::new(800, 600).unwrap();
        // Past the start and the auto-hide (the pointer never moved in).
        drive(&mut page, &mut target, Duration::from_millis(4200));
        let run = drive(&mut page, &mut target, Duration::from_millis(2000));
        assert_eq!(run.spins, 0, "{run:?}");
        renders.push(run.renders);
    }
    assert!(
        renders[0] <= renders[1] + 1,
        "controls: {} renders, none: {}",
        renders[0],
        renders[1]
    );
}
