//! Tests of the gradient stops: interpolation in premultiplied space
//! (CSS Images 3 §3.5.3), with the values of Chromium 148 from
//! `tools/probes/gradients.json` (three equal runs).

use swb_style::{Color, LengthPercentage, LinearGradient};

use super::*;

fn stop(r: u8, g: u8, b: u8, a: u8) -> (Color, Option<LengthPercentage>) {
    (Color::Rgba(Rgba::new(r, g, b, a)), None)
}

/// The gradient across a 100 by 100 box at 90 degrees, shown over `back`:
/// the pixels at x = 0, 16, ..., 96 in the middle row.
fn row(stops: Vec<(Color, Option<LengthPercentage>)>, back: [u8; 3]) -> Vec<[u8; 3]> {
    let gradient = LinearGradient {
        angle_deg: 90.0,
        stops,
        repeating: false,
    };
    let pixmap = gradient_tile(
        &gradient,
        Rgba::BLACK,
        Rect::new(0.0, 0.0, 100.0, 100.0),
        IntSize::from_wh(100, 100).expect("a size"),
    )
    .expect("a pixmap");
    (0..7)
        .map(|k| {
            let px = pixmap.pixel(k * 16, 50).expect("a pixel");
            let behind = 255 - u16::from(px.alpha());
            let over =
                |c: u8, b: u8| (u16::from(c) + (u16::from(b) * behind + 127) / 255).min(255) as u8;
            [
                over(px.red(), back[0]),
                over(px.green(), back[1]),
                over(px.blue(), back[2]),
            ]
        })
        .collect()
}

fn assert_close(got: &[[u8; 3]], expected: &[[u8; 3]], what: &str) {
    for (g, e) in got.iter().zip(expected) {
        for c in 0..3 {
            assert!(
                (i32::from(g[c]) - i32::from(e[c])).abs() <= 3,
                "{what}: got {got:?}, Chromium {expected:?}"
            );
        }
    }
}

#[test]
fn transparent_white_to_a_color_has_no_gray_band() {
    // `linear-gradient(90deg, rgba(255,255,255,0), rgb(16,20,24))`: the
    // dark theme of Wikipedia fades the sticky containers like this.
    let stops = || vec![stop(255, 255, 255, 0), stop(16, 20, 24, 255)];
    assert_close(
        &row(stops(), [255, 255, 255]),
        &[
            [254, 254, 254],
            [215, 216, 217],
            [177, 178, 180],
            [139, 141, 143],
            [101, 103, 106],
            [62, 66, 69],
            [24, 28, 32],
        ],
        "over white",
    );
    assert_close(
        &row(stops(), [16, 32, 48]),
        &[
            [16, 32, 48],
            [16, 30, 44],
            [16, 28, 40],
            [16, 26, 36],
            [16, 24, 32],
            [16, 22, 28],
            [16, 20, 25],
        ],
        "over the dark page",
    );
}

#[test]
fn transparent_to_red_is_red_all_along() {
    assert_close(
        &row(
            vec![stop(0, 0, 0, 0), stop(255, 0, 0, 255)],
            [255, 255, 255],
        ),
        &[
            [255, 254, 254],
            [255, 213, 213],
            [255, 172, 172],
            [255, 131, 131],
            [255, 91, 91],
            [255, 50, 50],
            [255, 9, 9],
        ],
        "transparent to red",
    );
}

#[test]
fn two_semi_transparent_colors() {
    assert_close(
        &row(
            vec![stop(255, 0, 0, 77), stop(0, 0, 255, 204)],
            [255, 255, 255],
        ),
        &[
            [254, 177, 178],
            [221, 157, 190],
            [188, 137, 203],
            [156, 116, 215],
            [123, 96, 227],
            [91, 76, 240],
            [58, 55, 252],
        ],
        "semi-transparent pair",
    );
}

#[test]
fn a_transparent_stop_in_the_middle_has_the_color_of_each_side() {
    // `linear-gradient(90deg, red, transparent, blue)` over the dark page.
    assert_close(
        &row(
            vec![stop(255, 0, 0, 255), stop(0, 0, 0, 0), stop(0, 0, 255, 255)],
            [16, 32, 48],
        ),
        &[
            [252, 0, 0],
            [176, 11, 16],
            [99, 21, 31],
            [23, 31, 47],
            [11, 23, 108],
            [6, 12, 174],
            [1, 2, 240],
        ],
        "red, transparent, blue",
    );
}

#[test]
fn stops_of_one_alpha_are_not_changed() {
    let red = tiny_skia::Color::from_rgba8(255, 0, 0, 255);
    let blue = tiny_skia::Color::from_rgba8(0, 0, 255, 255);
    let stops = vec![(0.0, red), (1.0, blue)];
    assert_eq!(premultiplied_stops(stops.clone()), stops);
    let clear = tiny_skia::Color::from_rgba8(255, 0, 0, 0);
    assert_eq!(
        premultiplied_stops(vec![(0.0, clear), (1.0, clear)]),
        vec![(0.0, clear), (1.0, clear)]
    );
}

#[test]
fn many_stops_stay_within_the_budget() {
    let stops: Vec<(f32, tiny_skia::Color)> = (0..5000)
        .map(|i| {
            let alpha = if i % 2 == 0 { 0 } else { 255 };
            (
                i as f32 / 5000.0,
                tiny_skia::Color::from_rgba8(i as u8, 0, 0, alpha),
            )
        })
        .collect();
    let out = premultiplied_stops(stops);
    // Two stops for each pair, and at most MAX_GRADIENT_STOPS more.
    assert!(
        out.len() <= 2 * (5000 - 1) + MAX_GRADIENT_STOPS,
        "{}",
        out.len()
    );
    assert!(out.windows(2).all(|w| w[0].0 <= w[1].0));
}

/// A stop position in px.
#[allow(clippy::unnecessary_wraps)] // The type of the position of a stop.
fn px(v: f32) -> Option<LengthPercentage> {
    Some(LengthPercentage::Px(v))
}

/// The same as `row`, with a repeating flag.
fn positioned_row(
    stops: Vec<(Color, Option<LengthPercentage>)>,
    repeating: bool,
    back: [u8; 3],
) -> Vec<[u8; 3]> {
    let gradient = LinearGradient {
        angle_deg: 90.0,
        stops,
        repeating,
    };
    let pixmap = gradient_tile(
        &gradient,
        Rgba::BLACK,
        Rect::new(0.0, 0.0, 100.0, 100.0),
        IntSize::from_wh(100, 100).expect("a size"),
    )
    .expect("a pixmap");
    (0..7)
        .map(|k| {
            let p = pixmap.pixel(k * 16, 50).expect("a pixel");
            let behind = 255 - u16::from(p.alpha());
            let over =
                |c: u8, b: u8| (u16::from(c) + (u16::from(b) * behind + 127) / 255).min(255) as u8;
            [
                over(p.red(), back[0]),
                over(p.green(), back[1]),
                over(p.blue(), back[2]),
            ]
        })
        .collect()
}

#[test]
fn a_repeating_gradient_repeats_the_part_between_its_first_and_last_stop() {
    // `repeating-linear-gradient(90deg, rgba(255,255,255,0) 0px, rgb(0,0,0)
    // 20px)` over white, x = 0, 16, ..., 96.
    let stops = vec![
        (Color::Rgba(Rgba::new(255, 255, 255, 0)), px(0.0)),
        (Color::Rgba(Rgba::BLACK), px(20.0)),
    ];
    assert_close(
        &positioned_row(stops, true, [255, 255, 255]),
        &[
            [249, 249, 249],
            [45, 45, 45],
            [96, 96, 96],
            [147, 147, 147],
            [198, 198, 198],
            [249, 249, 249],
            [45, 45, 45],
        ],
        "repeating, white",
    );
}

#[test]
fn a_repeating_gradient_of_semi_transparent_colors() {
    // `repeating-linear-gradient(90deg, rgba(255,0,0,.5) 0px, rgba(0,0,255,.9)
    // 25px, rgba(0,255,0,0) 50px)` over white.
    let stops = vec![
        (Color::Rgba(Rgba::new(255, 0, 0, 128)), px(0.0)),
        (Color::Rgba(Rgba::new(0, 0, 255, 230)), px(25.0)),
        (Color::Rgba(Rgba::new(0, 255, 0, 0)), px(50.0)),
    ];
    assert_close(
        &positioned_row(stops, true, [255, 255, 255]),
        &[
            [250, 125, 129],
            [103, 60, 211],
            [94, 94, 255],
            [241, 241, 255],
            [121, 68, 201],
            [76, 76, 255],
            [223, 223, 255],
        ],
        "repeating, semi-transparent",
    );
}

#[test]
fn a_transparent_stop_before_a_color_and_a_hard_start() {
    // `linear-gradient(90deg, rgba(255,0,0,0) 30%, rgba(255,0,0,0) 30%,
    // rgb(0,128,0) 70%)` over white: the red at the start is not seen.
    let stops = vec![
        (Color::Rgba(Rgba::new(255, 0, 0, 0)), px(30.0)),
        (Color::Rgba(Rgba::new(255, 0, 0, 0)), px(30.0)),
        (Color::Rgba(Rgba::rgb(0, 128, 0)), px(70.0)),
    ];
    assert_close(
        &positioned_row(stops, false, [255, 255, 255]),
        &[
            [255, 255, 255],
            [255, 255, 255],
            [239, 247, 239],
            [137, 196, 137],
            [35, 145, 35],
            [0, 128, 0],
            [0, 128, 0],
        ],
        "transparent red, then green",
    );
}

#[test]
fn stop_positions_follow_the_fixup_of_css_images() {
    let gradient = |stops| LinearGradient {
        angle_deg: 90.0,
        stops,
        repeating: false,
    };
    let c = Color::Rgba(Rgba::BLACK);
    // No positions: spread over the line.
    assert_eq!(
        stop_positions(&gradient(vec![(c, None), (c, None), (c, None)]), 100.0),
        vec![0.0, 0.5, 1.0]
    );
    // A run between two positions is spread between them.
    let p = stop_positions(
        &gradient(vec![
            (c, None),
            (c, None),
            (c, px(50.0)),
            (c, None),
            (c, None),
        ]),
        100.0,
    );
    assert_eq!(p, vec![0.0, 0.25, 0.5, 0.75, 1.0]);
    // A position before an earlier one moves up to it.
    let p = stop_positions(
        &gradient(vec![(c, px(60.0)), (c, px(20.0)), (c, None)]),
        100.0,
    );
    assert_eq!(p, vec![0.6, 0.6, 1.0]);
    // Positions outside the line stay.
    let p = stop_positions(&gradient(vec![(c, px(-50.0)), (c, px(150.0))]), 100.0);
    assert_eq!(p, vec![-0.5, 1.5]);
}
