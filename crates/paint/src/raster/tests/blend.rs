//! Blend groups: the modes against Chromium, a blend group in an isolated
//! group, the blend group of one image that draws without a layer
//! (`blend_image_group`) and a layer of opacity 1 that composites by rows
//! (`composite::source_over`). The last two are compared with the general
//! path, pixel by pixel.

use super::*;
use crate::raster::blend::single_image_group;
use swb_style::MixBlendMode as M;

/// The largest difference per channel between the fast and the general
/// path: the layer rounds the image to 8 bits before it blends. The
/// non-separable modes amplify it (as in `blend_modes_match_chromium`).
fn tolerance(mode: M) -> u8 {
    if matches!(mode, M::Hue | M::Saturation | M::Color | M::Luminosity) {
        6
    } else {
        2
    }
}

const ALL_MODES: [M; 17] = [
    M::Normal,
    M::Multiply,
    M::Screen,
    M::Overlay,
    M::Darken,
    M::Lighten,
    M::ColorDodge,
    M::ColorBurn,
    M::HardLight,
    M::SoftLight,
    M::Difference,
    M::Exclusion,
    M::Hue,
    M::Saturation,
    M::Color,
    M::Luminosity,
    M::PlusLighter,
];

/// An 8x8 image whose colors vary. With `translucent`, its alpha varies
/// from 0 to 255 (premultiplied).
fn picture(translucent: bool) -> DecodedImage {
    let mut pixmap = Pixmap::new(8, 8).unwrap();
    for y in 0..8u32 {
        for x in 0..8u32 {
            let alpha = if translucent {
                (x * 36 + 3).min(255)
            } else {
                255
            };
            let channel = |v: u32| (v * alpha / 255) as u8;
            let at = ((y * 8 + x) * 4) as usize;
            pixmap.data_mut()[at..at + 4].copy_from_slice(&[
                channel(30 + x * 25),
                channel(220 - y * 20),
                channel(60 + (x + y) * 10),
                alpha as u8,
            ]);
        }
    }
    DecodedImage::from_pixmap(pixmap)
}

fn stripe(y: f32, color: Rgba) -> DisplayItem {
    DisplayItem::Rect {
        rect: Rect::new(0.0, y, 100.0, 34.0),
        radii: [(0.0, 0.0); 4],
        color,
    }
}

/// How the blend group sits in the list.
#[derive(Clone, Copy)]
struct Setup {
    mode: M,
    opacity: f32,
    clip: bool,
    transform: Option<Matrix>,
}

/// The list of a blend group of one image over stripes of backdrop. With
/// `general`, the group holds an invisible rectangle too, which takes it
/// to the general path (a layer).
fn scene(setup: Setup, tile: Rect, general: bool) -> Vec<DisplayItem> {
    let Setup {
        mode,
        opacity,
        clip,
        transform,
    } = setup;
    let mut items = vec![
        stripe(0.0, Rgba::rgb(233, 236, 239)),
        stripe(33.0, Rgba::rgb(200, 60, 40)),
        stripe(66.0, Rgba::rgb(60, 120, 200)),
    ];
    if clip {
        items.push(DisplayItem::PushClip(Rect::new(20.0, 15.0, 41.0, 50.0)));
    }
    if let Some(m) = transform {
        items.push(DisplayItem::PushTransform {
            transform: GroupTransform::Matrix(m),
            bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
            hit_bounds: Rect::default(),
            fixed_bounds: None,
        });
    }
    items.push(DisplayItem::PushOpacity {
        opacity,
        blend: mode,
        bounds: tile,
        fixed_bounds: None,
        escapes_clips: false,
    });
    if general {
        items.push(DisplayItem::Rect {
            rect: tile,
            radii: [(0.0, 0.0); 4],
            color: Rgba::TRANSPARENT,
        });
    }
    items.push(image_item(tile, tile));
    items.push(DisplayItem::PopOpacity);
    if transform.is_some() {
        items.push(DisplayItem::PopTransform);
    }
    if clip {
        items.push(DisplayItem::PopClip);
    }
    items
}

fn max_difference(a: &Pixmap, b: &Pixmap) -> u8 {
    a.data()
        .iter()
        .zip(b.data())
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

/// Checks that the fast path and the general path agree for `setup`.
fn assert_same(setup: Setup, tile: Rect, translucent: bool, scale: f32) {
    let images = OneImage(picture(translucent), None);
    let fast = render_with(scene(setup, tile, false), scale, &images);
    let general = render_with(scene(setup, tile, true), scale, &images);
    let difference = max_difference(&fast, &general);
    assert!(
        difference <= tolerance(setup.mode),
        "{:?}, opacity {}, clip {}, transform {}, translucent {translucent}, scale {scale}: \
         difference {difference}",
        setup.mode,
        setup.opacity,
        setup.clip,
        setup.transform.is_some(),
    );
}

fn plain(mode: M) -> Setup {
    Setup {
        mode,
        opacity: 1.0,
        clip: false,
        transform: None,
    }
}

#[test]
fn a_blend_group_of_one_image_matches_its_layer_in_each_mode() {
    let tile = Rect::new(10.0, 12.0, 40.0, 32.0);
    for mode in ALL_MODES {
        for translucent in [false, true] {
            assert_same(plain(mode), tile, translucent, 1.0);
        }
    }
}

#[test]
fn a_blend_group_of_one_image_matches_its_layer_at_fractional_positions() {
    let tile = Rect::new(10.4, 12.6, 40.3, 30.7);
    for mode in [M::Multiply, M::Difference, M::Luminosity, M::PlusLighter] {
        for scale in [1.0, 1.5] {
            assert_same(plain(mode), tile, false, scale);
            assert_same(plain(mode), tile, true, scale);
        }
    }
}

#[test]
fn a_blend_group_of_one_image_matches_its_layer_with_opacity() {
    let tile = Rect::new(10.0, 12.0, 40.0, 32.0);
    for mode in [M::Multiply, M::Screen, M::PlusLighter, M::Hue] {
        for opacity in [0.5, 0.2, 0.99] {
            let setup = Setup {
                opacity,
                ..plain(mode)
            };
            assert_same(setup, tile, false, 1.0);
            assert_same(setup, tile, true, 1.0);
        }
    }
}

#[test]
fn a_blend_group_of_one_image_matches_its_layer_in_a_clip() {
    // The clip cuts the image in the middle.
    let tile = Rect::new(10.4, 12.6, 60.3, 40.7);
    for mode in [M::Multiply, M::PlusLighter, M::Exclusion] {
        let setup = Setup {
            clip: true,
            opacity: 0.7,
            ..plain(mode)
        };
        assert_same(setup, tile, false, 1.0);
        assert_same(setup, tile, true, 2.0);
    }
}

#[test]
fn a_blend_group_of_one_image_matches_its_layer_in_a_transform() {
    let tile = Rect::new(10.0, 12.0, 40.0, 32.0);
    let transforms = [
        Matrix::translate(7.25, 3.5),
        Matrix::new(1.3, 0.0, 0.0, 1.3, 2.0, 3.0),
    ];
    for m in transforms {
        for mode in [M::Multiply, M::PlusLighter, M::Overlay] {
            let setup = Setup {
                transform: Some(m),
                ..plain(mode)
            };
            assert_same(setup, tile, false, 1.0);
            assert_same(setup, tile, true, 1.0);
        }
    }
}

#[test]
fn a_blend_group_of_one_image_in_an_isolated_group_sees_only_that_group() {
    // The isolating layer (opacity 1, no blend) holds the group: the image
    // keeps its colors, and the layer composites by rows.
    let tile = Rect::new(10.0, 12.0, 40.0, 32.0);
    for mode in [M::Difference, M::PlusLighter] {
        let isolate = |general| {
            let mut items = scene(plain(mode), tile, general);
            items.insert(
                3,
                DisplayItem::PushOpacity {
                    opacity: 1.0,
                    blend: M::Normal,
                    bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
                    fixed_bounds: None,
                    escapes_clips: false,
                },
            );
            items.push(DisplayItem::PopOpacity);
            items
        };
        let images = OneImage(picture(true), None);
        let fast = render_with(isolate(false), 1.0, &images);
        let general = render_with(isolate(true), 1.0, &images);
        assert!(
            max_difference(&fast, &general) <= tolerance(mode),
            "{mode:?}"
        );
        // Outside the image, nothing changed from the backdrop.
        assert_eq!(rgb(&fast, 90, 10), (233, 236, 239));
    }
}

#[test]
fn a_blend_group_of_one_image_matches_chromium() {
    // The cases of `blend_modes_match_chromium`, with an image for the box.
    let mut solid = Pixmap::new(4, 4).unwrap();
    solid.fill(tiny_skia::Color::from_rgba8(120, 200, 90, 255));
    let images = OneImage(DecodedImage::from_pixmap(solid), None);
    let area = Rect::new(0.0, 0.0, 60.0, 90.0);
    for (mode, expected) in chromium_blend_cases() {
        let mut items = scene(plain(mode), area, false);
        // The stripes of that test are 30 px high.
        items.splice(
            0..3,
            [
                (0.0, (233, 236, 239)),
                (30.0, (200, 60, 40)),
                (60.0, (60, 120, 200)),
            ]
            .map(|(y, (r, g, b))| DisplayItem::Rect {
                rect: Rect::new(0.0, y, 60.0, 30.0),
                radii: [(0.0, 0.0); 4],
                color: Rgba::rgb(r, g, b),
            }),
        );
        let p = render_with(items, 1.0, &images);
        for (i, want) in expected.into_iter().enumerate() {
            let got = rgb(&p, 30, 15 + 30 * i as u32);
            let tolerance = if matches!(mode, M::Hue | M::Saturation | M::Color | M::Luminosity) {
                6
            } else {
                2
            };
            let close = |a: u8, b: u8| a.abs_diff(b) <= tolerance;
            assert!(
                close(got.0, want.0) && close(got.1, want.1) && close(got.2, want.2),
                "{mode:?} stripe {i}: {got:?}, Chromium {want:?}"
            );
        }
    }
}

#[test]
fn only_a_group_of_one_image_takes_the_direct_path() {
    let image = || image_item(Rect::new(0.0, 0.0, 4.0, 4.0), Rect::new(0.0, 0.0, 4.0, 4.0));
    let hit = || DisplayItem::HitRegion {
        rect: Rect::new(0.0, 0.0, 4.0, 4.0),
        node: NodeId::from_index(1).unwrap(),
    };
    let rect = || DisplayItem::Rect {
        rect: Rect::new(0.0, 0.0, 4.0, 4.0),
        radii: [(0.0, 0.0); 4],
        color: RED,
    };
    let push = || DisplayItem::PushOpacity {
        opacity: 1.0,
        blend: M::Multiply,
        bounds: Rect::default(),
        fixed_bounds: None,
        escapes_clips: false,
    };
    let pop = DisplayItem::PopOpacity;
    let clip = DisplayItem::PushClip(Rect::default());
    let range = |items: &[DisplayItem]| single_image_group(items, 0..items.len());
    assert_eq!(range(&[push(), image(), pop.clone()]), Some((1, 2)));
    assert_eq!(range(&[push(), hit(), image(), pop.clone()]), Some((2, 3)));
    // An empty clip pair after the image draws nothing.
    let after = [
        push(),
        image(),
        clip.clone(),
        DisplayItem::PopClip,
        pop.clone(),
    ];
    assert_eq!(range(&after), Some((1, 4)));
    // A clip before the image, two images, another item, no image, and a
    // group that does not end all take the general path.
    assert_eq!(
        range(&[push(), clip, image(), DisplayItem::PopClip, pop.clone()]),
        None
    );
    assert_eq!(range(&[push(), image(), image(), pop.clone()]), None);
    assert_eq!(range(&[push(), image(), rect(), pop.clone()]), None);
    assert_eq!(range(&[push(), pop.clone()]), None);
    assert_eq!(range(&[push(), image()]), None);
}

/// The mode and the colors at the middle of each stripe (see
/// [`chromium_blend_cases`]).
type BlendCase = (MixBlendMode, [(u8, u8, u8); 3]);

/// The blend modes against Chromium 148 (probe cases `box-*` of
/// `tools/probes/mix-blend-mode.json`): a box (120, 200, 90) over three
/// stripes of backdrop (233, 236, 239), (200, 60, 40) and (60, 120, 200),
/// each 30 px high; the colors at the middle of each stripe.
fn chromium_blend_cases() -> [BlendCase; 17] {
    use MixBlendMode as M;
    #[rustfmt::skip]
    let cases = [
        (M::Normal, [(120, 200, 90), (120, 200, 90), (120, 200, 90)]),
        (M::Multiply, [(110, 185, 85), (94, 47, 15), (29, 94, 71)]),
        (M::Screen, [(243, 251, 245), (226, 213, 116), (152, 226, 219)]),
        (M::Overlay, [(231, 246, 234), (196, 94, 29), (57, 188, 184)]),
        (M::Darken, [(120, 200, 90), (120, 60, 40), (60, 120, 90)]),
        (M::Lighten, [(233, 236, 239), (200, 200, 90), (120, 200, 200)]),
        (M::ColorDodge, [(255, 255, 255), (255, 255, 62), (113, 255, 255)]),
        (M::ColorBurn, [(208, 231, 210), (138, 6, 0), (0, 83, 99)]),
        (M::HardLight, [(219, 246, 169), (188, 171, 29), (57, 196, 141)]),
        (M::SoftLight, [(232, 241, 235), (197, 96, 30), (57, 151, 187)]),
        (M::Difference, [(113, 36, 149), (80, 140, 50), (60, 80, 110)]),
        (M::Exclusion, [(133, 66, 159), (132, 166, 100), (122, 132, 148)]),
        (M::Hue, [(233, 237, 231), (41, 149, 0), (55, 157, 17)]),
        (M::Saturation, [(222, 239, 255), (169, 72, 59), (71, 118, 181)]),
        (M::Color, [(212, 255, 195), (56, 136, 26), (67, 147, 37)]),
        (M::Luminosity, [(161, 164, 167), (255, 128, 110), (113, 173, 253)]),
        (M::PlusLighter, [(255, 255, 255), (255, 255, 130), (180, 255, 255)]),
    ];
    cases
}

#[test]
fn blend_modes_match_chromium() {
    use MixBlendMode as M;
    let cases = chromium_blend_cases();
    let stripe = |y: f32, (r, g, b): (u8, u8, u8)| DisplayItem::Rect {
        rect: Rect::new(0.0, y, 60.0, 30.0),
        radii: [(0.0, 0.0); 4],
        color: Rgba::rgb(r, g, b),
    };
    for (mode, expected) in cases {
        let area = Rect::new(0.0, 0.0, 60.0, 90.0);
        let p = render(vec![
            stripe(0.0, (233, 236, 239)),
            stripe(30.0, (200, 60, 40)),
            stripe(60.0, (60, 120, 200)),
            DisplayItem::PushOpacity {
                opacity: 1.0,
                blend: mode,
                bounds: area,
                fixed_bounds: None,
                escapes_clips: false,
            },
            DisplayItem::Rect {
                rect: area,
                radii: [(0.0, 0.0); 4],
                color: Rgba::rgb(120, 200, 90),
            },
            DisplayItem::PopOpacity,
        ]);
        for (i, want) in expected.into_iter().enumerate() {
            let got = rgb(&p, 30, 15 + 30 * i as u32);
            // The non-separable modes differ by a few levels in rounding.
            let tolerance = if matches!(mode, M::Hue | M::Saturation | M::Color | M::Luminosity) {
                6
            } else {
                2
            };
            let close = |a: u8, b: u8| a.abs_diff(b) <= tolerance;
            assert!(
                close(got.0, want.0) && close(got.1, want.1) && close(got.2, want.2),
                "{mode:?} stripe {i}: {got:?}, Chromium {want:?}"
            );
        }
    }
}

#[test]
fn a_blend_group_in_an_isolated_group_sees_only_that_group() {
    // A `difference` box in a group of opacity 0.99 over a grey backdrop:
    // the group has nothing behind the box, so the box keeps its color.
    use MixBlendMode as M;
    let area = Rect::new(0.0, 0.0, 60.0, 30.0);
    let group = |opacity, blend| DisplayItem::PushOpacity {
        opacity,
        blend,
        bounds: area,
        fixed_bounds: None,
        escapes_clips: false,
    };
    let rect = |color| DisplayItem::Rect {
        rect: area,
        radii: [(0.0, 0.0); 4],
        color,
    };
    let p = render(vec![
        rect(Rgba::rgb(233, 236, 239)),
        group(0.99, M::Normal),
        group(1.0, M::Difference),
        rect(Rgba::rgb(120, 200, 90)),
        DisplayItem::PopOpacity,
        DisplayItem::PopOpacity,
    ]);
    let (r, g, b) = rgb(&p, 30, 15);
    assert!(r.abs_diff(121) <= 2 && g.abs_diff(200) <= 2 && b.abs_diff(91) <= 2);
}
