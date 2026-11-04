//! Tests of the filter steps against the values of Chromium 148
//! (`tools/probes/filter.json`, three equal runs each).

use swb_style::{Color, DropShadow, FilterFunction as F, Rgba};

use super::*;

const SIZE: usize = 128;

/// A layer of `SIZE` by `SIZE` pixels with an opaque square of `color` at
/// (40, 40), 40 px wide.
fn layer(color: [u8; 4]) -> Vec<u8> {
    let mut data = vec![0_u8; SIZE * SIZE * 4];
    for y in 40..80 {
        for x in 40..80 {
            let i = (y * SIZE + x) * 4;
            data[i..i + 4].copy_from_slice(&color);
        }
    }
    data
}

fn run(data: &mut [u8], list: &[F], scale: f32) {
    let ops = device_ops(list, Rgba::BLACK, scale);
    let mut scratch = vec![0_u8; data.len()];
    apply(data, &mut scratch, SIZE, SIZE, &ops);
}

/// The pixel (x, y) over white, as the page shows it.
fn over_white(data: &[u8], column: usize, row: usize) -> [u8; 3] {
    let at = (row * SIZE + column) * 4;
    let alpha = u16::from(data[at + 3]);
    let channel = |k: usize| (u16::from(data[at + k]) + 255 - alpha) as u8;
    [channel(0), channel(1), channel(2)]
}

fn assert_close(got: &[u8], expected: &[u8], tolerance: i32, what: &str) {
    assert_eq!(got.len(), expected.len(), "{what}");
    for (i, (g, e)) in got.iter().zip(expected).enumerate() {
        assert!(
            (i32::from(*g) - i32::from(*e)).abs() <= tolerance,
            "{what}: index {i}: got {g}, Chromium {e}\n got {got:?}\nwant {expected:?}"
        );
    }
}

/// The values of the first channel along the row y = 60 from x = 24 to 56.
fn row(data: &[u8]) -> Vec<u8> {
    (24..57).map(|x| over_white(data, x, 60)[0]).collect()
}

#[test]
fn blur_profiles_match_chromium() {
    // Chromium: a 40 px black square on white, `blur(R)`, the row through
    // its middle, x = -16 to 16 from the left edge.
    let cases: [(f32, [u8; 33]); 4] = [
        (
            1.0,
            [
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 234, 170, 85,
                21, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
        ),
        (
            2.0,
            [
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 252, 242, 223, 191, 150,
                105, 64, 32, 13, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
        ),
        (
            5.0,
            [
                255, 255, 255, 255, 255, 254, 252, 248, 243, 235, 226, 213, 197, 179, 159, 138,
                117, 96, 76, 58, 42, 29, 20, 12, 7, 3, 1, 0, 0, 0, 0, 0, 0,
            ],
        ),
        (
            10.0,
            [
                241, 239, 235, 231, 226, 220, 214, 207, 200, 192, 183, 174, 165, 156, 146, 137,
                126, 116, 107, 98, 88, 79, 71, 63, 55, 48, 43, 37, 32, 28, 24, 21, 18,
            ],
        ),
    ];
    for (sigma, expected) in cases {
        let mut data = layer([0, 0, 0, 255]);
        run(&mut data, &[F::Blur(sigma)], 1.0);
        assert_close(&row(&data), &expected, 1, &format!("blur({sigma}px)"));
    }
}

#[test]
fn small_blurs_are_the_identity_as_in_chromium() {
    // `d` is 1 below a standard deviation of 0.8.
    for sigma in [0.3, 0.6] {
        let mut data = layer([0, 0, 0, 255]);
        let before = data.clone();
        run(&mut data, &[F::Blur(sigma)], 1.0);
        assert_eq!(data, before);
    }
    assert_eq!(box_size(0.9), 2);
    assert_eq!(box_size(1.2), 2);
    assert_eq!(box_size(1.6), 3);
    assert_eq!(box_size(5.0), 9);
}

#[test]
fn blur_scales_with_the_device() {
    // `blur(2px)` at a scale of 2 is a blur of 4 device px.
    let mut a = layer([0, 0, 0, 255]);
    run(&mut a, &[F::Blur(2.0)], 2.0);
    let mut b = layer([0, 0, 0, 255]);
    run(&mut b, &[F::Blur(4.0)], 1.0);
    assert_eq!(a, b);
}

#[test]
fn color_functions_match_chromium() {
    // Chromium: (input, filter, output) on opaque pixels.
    let cases: [([u8; 3], Vec<F>, [u8; 3]); 14] = [
        ([200, 100, 50], vec![F::Brightness(0.5)], [100, 50, 25]),
        ([200, 100, 50], vec![F::Brightness(1.5)], [255, 150, 75]),
        ([200, 100, 50], vec![F::Contrast(0.5)], [164, 114, 89]),
        ([120, 200, 60], vec![F::Contrast(2.0)], [113, 255, 0]),
        ([200, 100, 50], vec![F::Grayscale(1.0)], [118, 118, 118]),
        ([255, 0, 0], vec![F::Grayscale(1.0)], [54, 54, 54]),
        ([200, 100, 50], vec![F::HueRotate(90.0)], [50, 146, 35]),
        ([200, 100, 50], vec![F::Invert(1.0)], [55, 155, 205]),
        ([200, 100, 50], vec![F::Invert(0.5)], [128, 128, 128]),
        ([0, 128, 255], vec![F::Saturate(2.0)], [0, 146, 255]),
        ([200, 100, 50], vec![F::Sepia(1.0)], [165, 147, 114]),
        (
            [200, 100, 50],
            vec![F::Invert(1.0), F::HueRotate(180.0)],
            [220, 120, 70],
        ),
        (
            [200, 100, 50],
            vec![F::Invert(1.0), F::HueRotate(180.0), F::Brightness(0.8)],
            [176, 96, 56],
        ),
        (
            [120, 200, 60],
            vec![F::Brightness(0.0), F::Invert(1.0)],
            [255, 255, 255],
        ),
    ];
    for (input, list, expected) in cases {
        let mut data = vec![input[0], input[1], input[2], 255];
        let ops = device_ops(&list, Rgba::BLACK, 1.0);
        let mut scratch = vec![0_u8; 4];
        apply(&mut data, &mut scratch, 1, 1, &ops);
        assert_close(&data[..3], &expected, 1, &format!("{list:?} on {input:?}"));
    }
}

#[test]
fn color_functions_keep_the_alpha_and_work_on_the_color() {
    // Chromium: `rgba(255, 0, 0, .5)` with `invert(1) opacity(.5)` over
    // white is 191 in red; the color is inverted without the alpha.
    let mut data = vec![128, 0, 0, 128];
    let list = [F::Invert(1.0)];
    let ops = device_ops(&list, Rgba::BLACK, 1.0);
    apply(&mut data, &mut [0; 4], 1, 1, &ops);
    assert_eq!(data[3], 128);
    // Inverted red is cyan: green and blue are the alpha.
    assert_eq!(&data[..3], &[0, 128, 128]);
    let list = [F::Invert(1.0), F::Opacity(0.5)];
    let mut data = vec![128, 0, 0, 128];
    let ops = device_ops(&list, Rgba::BLACK, 1.0);
    apply(&mut data, &mut [0; 4], 1, 1, &ops);
    assert_eq!(data[3], 64);
    // Transparent pixels stay transparent.
    let mut data = vec![0, 0, 0, 0];
    let ops = device_ops(&[F::Invert(1.0)], Rgba::BLACK, 1.0);
    apply(&mut data, &mut [0; 4], 1, 1, &ops);
    assert_eq!(data, [0, 0, 0, 0]);
}

#[test]
fn drop_shadow_matches_chromium() {
    // Chromium: black square, `drop-shadow(10px 10px 5px rgb(0, 0, 255))`,
    // the row through the middle of the square, x = 34 to 62 in steps of 2
    // from the left edge (the square ends at x = 40).
    let shadow = F::DropShadow(DropShadow {
        color: Color::Rgba(Rgba::rgb(0, 0, 255)),
        x: 10.0,
        y: 10.0,
        blur: 5.0,
    });
    let mut data = layer([0, 0, 0, 255]);
    run(&mut data, &[shadow], 1.0);
    let expected_red: [u8; 15] = [
        0, 0, 0, 4, 13, 30, 59, 97, 139, 179, 213, 235, 248, 254, 255,
    ];
    let got: Vec<u8> = (34..64)
        .step_by(2)
        .map(|x| over_white(&data, 40 + x, 60)[0])
        .collect();
    assert_close(&got, &expected_red, 1, "drop-shadow red channel");
    let blue: Vec<u8> = (34..64)
        .step_by(2)
        .map(|x| over_white(&data, 40 + x, 60)[2])
        .collect();
    assert_eq!(&blue[3..], &[255; 12]);
}

#[test]
fn drop_shadow_offsets_are_rounded_down() {
    // Chromium: 2.5px moves the shadow 2 device px, -0.4px moves it 1 px to
    // the left, and the shadow of a blur of 0 is sharp.
    let shadow = |x: f32, y: f32| {
        F::DropShadow(DropShadow {
            color: Color::Rgba(Rgba::rgb(255, 0, 0)),
            x,
            y,
            blur: 0.0,
        })
    };
    let mut data = layer([0, 0, 0, 255]);
    run(&mut data, &[shadow(2.5, 0.0)], 1.0);
    assert_eq!(over_white(&data, 80, 60), [255, 0, 0]);
    assert_eq!(over_white(&data, 81, 60), [255, 0, 0]);
    assert_eq!(over_white(&data, 82, 60), [255, 255, 255]);
    let mut data = layer([0, 0, 0, 255]);
    run(&mut data, &[shadow(-0.4, 0.0)], 1.0);
    assert_eq!(over_white(&data, 39, 60), [255, 0, 0]);
    assert_eq!(over_white(&data, 38, 60), [255, 255, 255]);
}

#[test]
fn drop_shadow_keeps_the_alpha_of_the_color() {
    // Chromium: a shadow with `rgba(0, 0, 255, .5)` over white is (127,
    // 127, 255) where the square is opaque.
    let shadow = F::DropShadow(DropShadow {
        color: Color::Rgba(Rgba::new(0, 0, 255, 128)),
        x: 20.0,
        y: 0.0,
        blur: 0.0,
    });
    let mut data = layer([0, 0, 0, 255]);
    run(&mut data, &[shadow], 1.0);
    let [r, g, b] = over_white(&data, 90, 60);
    assert!(
        (i32::from(r) - 127).abs() <= 1 && r == g && b == 255,
        "{r} {g} {b}"
    );
}

#[test]
fn the_current_color_is_the_default_of_a_shadow() {
    let shadow = F::DropShadow(DropShadow {
        color: Color::CurrentColor,
        x: 20.0,
        y: 0.0,
        blur: 0.0,
    });
    let ops = device_ops(&[shadow], Rgba::rgb(0, 100, 0), 1.0);
    let [Op::Shadow(s)] = ops.as_slice() else {
        panic!("{ops:?}");
    };
    assert_eq!(s.color[3], 1.0);
    assert!((s.color[1] - 100.0 / 255.0).abs() < 1e-6);
}

#[test]
fn identities_and_urls_make_no_steps() {
    let list = [
        F::Blur(0.0),
        F::Brightness(1.0),
        F::Contrast(1.0),
        F::Grayscale(0.0),
        F::HueRotate(0.0),
        F::HueRotate(360.0),
        F::Invert(0.0),
        F::Opacity(1.0),
        F::Saturate(1.0),
        F::Sepia(0.0),
        F::Url("#a".into()),
    ];
    assert!(!paints(&list));
    assert_eq!(device_ops(&list, Rgba::BLACK, 1.0).len(), 0);
    assert!(paints(&[F::Url("#a".into()), F::Invert(1.0)]));
}

#[test]
fn consecutive_color_functions_make_one_pass() {
    let ops = device_ops(
        &[
            F::Invert(1.0),
            F::HueRotate(180.0),
            F::Blur(3.0),
            F::Brightness(0.5),
        ],
        Rgba::BLACK,
        1.0,
    );
    assert_eq!(ops.len(), 3);
    assert!(matches!(&ops[0], Op::Colors(steps) if steps.len() == 2));
    assert!(matches!(ops[1], Op::Blur(_)));
}

#[test]
fn extents_of_the_steps() {
    let blur = device_ops(&[F::Blur(5.0)], Rgba::BLACK, 1.0);
    // d = 9: each of the three boxes reaches 4 px.
    let m = forward_growth(&blur);
    assert!(m.left >= 12 && m.left <= 14, "{m:?}");
    assert_eq!(m, backward_reach(&blur));
    assert_eq!(forward_growth(&[]), Margins::default());
    let shadow = F::DropShadow(DropShadow {
        color: Color::Rgba(Rgba::BLACK),
        x: 10.0,
        y: -5.0,
        blur: 0.0,
    });
    let ops = device_ops(&[shadow], Rgba::BLACK, 1.0);
    let grow = forward_growth(&ops);
    assert_eq!(
        (grow.left, grow.right, grow.top, grow.bottom),
        (0, 10, 5, 0)
    );
    let reach = backward_reach(&ops);
    assert_eq!(
        (reach.left, reach.right, reach.top, reach.bottom),
        (10, 0, 0, 5)
    );
}

#[test]
fn a_huge_blur_is_limited() {
    let ops = device_ops(&[F::Blur(1e9)], Rgba::BLACK, 1.0);
    let [Op::Blur(sigma)] = ops.as_slice() else {
        panic!("{ops:?}");
    };
    assert_eq!(*sigma, MAX_SIGMA);
    let m = forward_growth(&ops);
    assert!(m.left < 1000, "{m:?}");
}

#[test]
fn work_counts_the_passes() {
    let ops = device_ops(&[F::Invert(1.0), F::Blur(3.0)], Rgba::BLACK, 1.0);
    assert_eq!(work(&ops, 100), (1 + 6) * 100);
    assert_eq!(work(&[], 100), 0);
}

/// One row of opaque gray pixels (premultiplied RGBA) with the values.
fn gray_row(values: &[u8]) -> Vec<u8> {
    values.iter().flat_map(|&v| [v, v, v, 255]).collect()
}

fn blur_mirrored(data: &mut [u8], width: usize, height: usize, sigma: f32) {
    apply_mirrored(data, width, height, &[Op::Blur(sigma)]);
}

#[test]
fn mirrored_blur_matches_chromium_at_the_edges() {
    // Chromium 148 (`tools/probes/backdrop-filter.json`, `bf-edge-stripe-*`):
    // the backdrop of a box 100 px wide whose first 4 px are black and the
    // rest white, `blur(3px)`: the row from the left edge. The image is
    // mirrored at its edges once, before the three box passes (an even
    // window is not symmetric, so mirroring before every pass differs by
    // 2 to 8 levels here).
    let mut values = vec![0_u8; 4];
    values.extend(vec![255_u8; 96]);
    let want = [56, 67, 87, 113, 143, 172, 198, 220, 235, 245, 251, 254];
    let mut data = gray_row(&values);
    blur_mirrored(&mut data, 100, 1, 3.0);
    let got: Vec<u8> = data.chunks(4).take(12).map(|px| px[0]).collect();
    assert_close(&got, &want, 1, "left edge");
    // The same picture at the right edge gives the same values inward.
    values.reverse();
    let mut data = gray_row(&values);
    blur_mirrored(&mut data, 100, 1, 3.0);
    let got: Vec<u8> = data.chunks(4).rev().take(12).map(|px| px[0]).collect();
    assert_close(&got, &want, 1, "right edge");
}

#[test]
fn mirrored_blur_keeps_a_flat_image_and_the_alpha() {
    // No fade at the edges: a flat image stays flat, a half transparent one
    // too (the transparent-edge blur of `filter` would fade the border).
    let mut data: Vec<u8> = std::iter::repeat_n([100, 50, 0, 128], 20 * 10)
        .flatten()
        .collect();
    blur_mirrored(&mut data, 20, 10, 4.0);
    for px in data.chunks(4) {
        assert_close(px, &[100, 50, 0, 128], 1, "flat");
    }
}

#[test]
fn mirrored_blur_works_with_a_radius_larger_than_the_image() {
    // The extension repeats the mirrored image: 3 px wide, a blur that
    // reaches 20 px. The result is between the extremes and symmetric.
    let mut data = gray_row(&[0, 255, 0]);
    blur_mirrored(&mut data, 3, 1, 5.0);
    let got: Vec<u8> = data.chunks(4).map(|px| px[0]).collect();
    assert!(got.iter().all(|&v| (60..200).contains(&v)), "{got:?}");
    assert_eq!(got[0], got[2], "{got:?}");
}

#[test]
fn mirrored_blur_ignores_an_empty_or_short_buffer() {
    let mut data = vec![0_u8; 8];
    blur_mirrored(&mut data, 0, 0, 3.0);
    blur_mirrored(&mut data, 5, 5, 3.0);
    assert_eq!(data, vec![0_u8; 8]);
}
