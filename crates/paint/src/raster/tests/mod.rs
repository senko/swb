use super::*;

#[test]
fn blit_blends_and_clips() {
    let mut pixmap = Pixmap::new(4, 4).unwrap();
    pixmap.fill(tiny_skia::Color::WHITE);
    let data = [255u8, 128, 0, 255];
    let mask = AlphaMask {
        data: &data,
        width: 2,
        height: 2,
    };
    let clip = Rect::new(0.0, 0.0, 4.0, 4.0);
    blit_mask(&mut pixmap.as_mut(), &mask, (3, 3), Rgba::BLACK, clip);
    // Only the top-left mask pixel lands inside the 4x4 pixmap.
    let px = pixmap.pixel(3, 3).unwrap();
    assert_eq!((px.red(), px.alpha()), (0, 255));
    let untouched = pixmap.pixel(2, 2).unwrap();
    assert_eq!(untouched.red(), 255);
}

#[test]
fn blit_far_away_does_not_overflow() {
    let mut pixmap = Pixmap::new(4, 4).unwrap();
    let mask = AlphaMask {
        data: &[255; 4],
        width: 2,
        height: 2,
    };
    let clip = Rect::new(0.0, 0.0, 4.0, 4.0);
    blit_mask(
        &mut pixmap.as_mut(),
        &mask,
        (i64::MAX - 1, 0),
        Rgba::BLACK,
        clip,
    );
    blit_mask(
        &mut pixmap.as_mut(),
        &mask,
        (0, i64::MAX),
        Rgba::BLACK,
        clip,
    );
    assert!(pixmap.data().iter().all(|&b| b == 0));
}

#[test]
fn half_coverage_is_grey() {
    let mut pixmap = Pixmap::new(1, 1).unwrap();
    pixmap.fill(tiny_skia::Color::WHITE);
    let mask = AlphaMask {
        data: &[128],
        width: 1,
        height: 1,
    };
    blit_mask(
        &mut pixmap.as_mut(),
        &mask,
        (0, 0),
        Rgba::BLACK,
        Rect::new(0.0, 0.0, 1.0, 1.0),
    );
    let px = pixmap.pixel(0, 0).unwrap();
    assert!((126..=128).contains(&px.red()));
}

#[test]
fn subpixel_positions_round_to_the_next_pixel() {
    assert_eq!(subpixel_position(10.0), (10.0, 0));
    assert_eq!(subpixel_position(10.3), (10.0, 1));
    assert_eq!(subpixel_position(10.6), (10.0, 2));
    assert_eq!(subpixel_position(10.8), (10.0, 3));
    assert_eq!(subpixel_position(10.9), (11.0, 0));
    assert_eq!(subpixel_position(-0.05), (0.0, 0));
}

struct NoImages;

impl ImageSource for NoImages {
    fn image(&self, _image: &ImageRef) -> Option<&DecodedImage> {
        None
    }

    fn vector_cache(&self) -> Option<&VectorCache> {
        None
    }
}

struct OneImage(DecodedImage, Option<VectorCache>);

impl ImageSource for OneImage {
    fn image(&self, _image: &ImageRef) -> Option<&DecodedImage> {
        Some(&self.0)
    }

    fn vector_cache(&self) -> Option<&VectorCache> {
        self.1.as_ref()
    }
}

const BLUE: Rgba = Rgba::rgb(0, 0, 255);
const RED: Rgba = Rgba::rgb(255, 0, 0);

/// Rasterizes `items` on a white 100x100 target at `scale`.
fn render_with(items: Vec<DisplayItem>, scale: f32, images: &dyn ImageSource) -> Pixmap {
    let size = (100.0 * scale) as u32;
    let mut target = Pixmap::new(size, size).unwrap();
    target.fill(tiny_skia::Color::WHITE);
    let params = RasterParams {
        scroll: Point::default(),
        viewport_scroll: Point::default(),
        scale,
    };
    let mut fonts = FontContext::for_tests();
    rasterize(
        &DisplayList { items },
        &mut target,
        params,
        &mut fonts,
        images,
    );
    target
}

fn render(items: Vec<DisplayItem>) -> Pixmap {
    render_with(items, 1.0, &NoImages)
}

fn rgb(p: &Pixmap, x: u32, y: u32) -> (u8, u8, u8) {
    let c = p.pixel(x, y).unwrap().demultiply();
    (c.red(), c.green(), c.blue())
}

fn border(widths: [f32; 4], colors: [Rgba; 4], style: BorderStyle, radius: f32) -> DisplayItem {
    DisplayItem::Border {
        rect: Rect::new(10.0, 10.0, 80.0, 60.0),
        widths,
        colors,
        styles: [style; 4],
        radii: [(radius, radius); 4],
    }
}

#[test]
fn polygons_fill_and_skip_degenerate_input() {
    let polygon = |points: &[(f32, f32)], color| DisplayItem::Polygon {
        points: points.iter().map(|&(x, y)| Point::new(x, y)).collect(),
        color,
    };
    let triangle = [(10.0, 10.0), (10.0, 90.0), (90.0, 50.0)];
    let p = render(vec![polygon(&triangle, BLUE)]);
    assert_eq!(rgb(&p, 30, 50), (0, 0, 255), "inside");
    assert_eq!(rgb(&p, 80, 20), (255, 255, 255), "outside");
    // At scale 2 the same triangle covers twice the device pixels.
    let p = render_with(vec![polygon(&triangle, BLUE)], 2.0, &NoImages);
    assert_eq!(rgb(&p, 60, 100), (0, 0, 255));
    // Fewer than three points, non-finite points and transparent
    // colors draw nothing (and do not panic).
    let nothing = [
        polygon(&[(10.0, 10.0), (90.0, 90.0)], BLUE),
        polygon(&[(10.0, 10.0), (f32::NAN, 90.0), (90.0, 10.0)], BLUE),
        polygon(&[(10.0, 10.0), (f32::INFINITY, 90.0), (90.0, 10.0)], BLUE),
        polygon(&triangle, Rgba::new(0, 0, 255, 0)),
    ];
    for item in nothing {
        let p = render(vec![item]);
        assert_eq!(rgb(&p, 30, 30), (255, 255, 255));
    }
}

#[test]
fn rounded_border_keeps_the_color_of_each_side() {
    // Only the bottom side has a width; the top color must not be used.
    let p = render(vec![border(
        [0.0, 0.0, 10.0, 0.0],
        [Rgba::BLACK, Rgba::BLACK, BLUE, Rgba::BLACK],
        BorderStyle::Solid,
        4.0,
    )]);
    assert_eq!(rgb(&p, 50, 65), (0, 0, 255));
    assert_eq!(rgb(&p, 50, 30), (255, 255, 255));
    // The rounded corner cuts the bottom-left pixel.
    assert_eq!(rgb(&p, 10, 69), (255, 255, 255));
}

#[test]
fn rounded_border_has_no_gaps_at_the_corners() {
    // The spinner pattern: a circle with one side in another color. The
    // ring at 45° lies inside the inner rectangle's corner, outside the
    // side trapezoids.
    let item = DisplayItem::Border {
        rect: Rect::new(10.0, 10.0, 80.0, 80.0),
        widths: [10.0; 4],
        colors: [BLUE, RED, RED, RED],
        styles: [BorderStyle::Solid; 4],
        radii: [(40.0, 40.0); 4],
    };
    let p = render(vec![item]);
    // A point on the ring's center line at 45° (top-left corner).
    let d = 35.0 * std::f32::consts::FRAC_1_SQRT_2;
    let (x, y) = ((50.0 - d) as u32, (50.0 - d) as u32);
    assert_ne!(
        rgb(&p, x, y),
        (255, 255, 255),
        "corner pixel ({x}, {y}) is painted"
    );
    // The bottom-right corner belongs to the right/bottom sides.
    let (x, y) = ((50.0 + d) as u32, (50.0 + d) as u32);
    assert_eq!(rgb(&p, x, y), (255, 0, 0));
}

#[test]
fn rounded_corner_belongs_to_the_painted_side() {
    // Only the top side has a width: its rounded ends taper into the
    // corners, which must be painted although they lie below the inner
    // corner line.
    let item = DisplayItem::Border {
        rect: Rect::new(10.0, 10.0, 80.0, 60.0),
        widths: [4.0, 0.0, 0.0, 0.0],
        colors: [BLUE; 4],
        styles: [BorderStyle::Solid; 4],
        radii: [(20.0, 20.0); 4],
    };
    let p = render(vec![item]);
    // Near the left end of the taper: the ring at x = 12.5 spans
    // y = 21.3..23.0, below the line from the inner corner (10, 14) to
    // the center. The painted top side owns this area.
    let (r, _, b) = rgb(&p, 12, 22);
    assert!(
        b == 255 && r < 200,
        "taper is painted: {:?}",
        rgb(&p, 12, 22)
    );
    // Further down the left edge nothing is painted.
    assert_eq!(rgb(&p, 11, 50), (255, 255, 255));
}

#[test]
fn uniform_inset_border_is_shaded() {
    let gray = Rgba::rgb(150, 150, 150);
    let p = render(vec![border([10.0; 4], [gray; 4], BorderStyle::Inset, 0.0)]);
    assert_eq!(rgb(&p, 50, 14), (100, 100, 100), "top is darker");
    assert_eq!(rgb(&p, 50, 65), (150, 150, 150), "bottom keeps the color");
}

#[test]
fn double_border_has_two_lines() {
    let p = render(vec![border([9.0; 4], [BLUE; 4], BorderStyle::Double, 0.0)]);
    assert_eq!(rgb(&p, 50, 11), (0, 0, 255), "outer line");
    assert_eq!(rgb(&p, 50, 14), (255, 255, 255), "gap");
    assert_eq!(rgb(&p, 50, 17), (0, 0, 255), "inner line");
}

#[test]
fn adjacent_rects_have_no_seam_at_fractional_scale() {
    let rect = |y: f32| DisplayItem::Rect {
        rect: Rect::new(0.0, y, 50.0, 13.0),
        radii: [(0.0, 0.0); 4],
        color: Rgba::BLACK,
    };
    let p = render_with(vec![rect(0.0), rect(13.0)], 1.25, &NoImages);
    for y in 0..32 {
        assert_eq!(rgb(&p, 10, y), (0, 0, 0), "row {y}");
    }
}

#[test]
fn scaled_image_does_not_bleed_at_its_edges() {
    // 4x4: black, with a white bottom row.
    let mut image = Pixmap::new(4, 4).unwrap();
    image.fill(tiny_skia::Color::BLACK);
    for x in 0..4 {
        let i = (3 * 4 + x) * 4;
        image.data_mut()[i..i + 4].copy_from_slice(&[255; 4]);
    }
    let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
    let item = DisplayItem::Image {
        image: ImageRef::Url("x".into()),
        rect,
        tile: rect,
        clip: rect,
    };
    let p = render_with(
        vec![item],
        1.0,
        &OneImage(DecodedImage::from_pixmap(image), None),
    );
    assert_eq!(rgb(&p, 50, 0), (0, 0, 0));
}

#[test]
fn a_single_image_is_scaled_into_its_pixel_snapped_rectangle() {
    // 4x1: two black pixels, two white. Drawn 20 px wide at x = 10.5,
    // the snapped rectangle is x = 11 (rounded), so the edge between
    // black and white is centered on x = 21, not 20.5.
    let mut image = Pixmap::new(4, 1).unwrap();
    image.fill(tiny_skia::Color::BLACK);
    for x in 2..4 {
        image.data_mut()[x * 4..x * 4 + 4].copy_from_slice(&[255; 4]);
    }
    let tile = Rect::new(10.5, 0.0, 20.0, 10.0);
    let p = render_with(
        vec![image_item(tile, tile)],
        1.0,
        &OneImage(DecodedImage::from_pixmap(image), None),
    );
    let (gray, ..) = rgb(&p, 20, 5);
    assert!((i32::from(gray) - 102).abs() <= 8, "gray {gray}");
}

fn svg(source: &str) -> DecodedImage {
    crate::decode_with_type(source.as_bytes(), Some(crate::SVG_MIME_TYPE)).unwrap()
}

fn image_item(tile: Rect, area: Rect) -> DisplayItem {
    DisplayItem::Image {
        image: ImageRef::Url("x".into()),
        rect: area,
        tile,
        clip: area,
    }
}

#[test]
fn vector_images_are_sharp_at_any_scale() {
    // A 1x1 checkerboard of four squares: rendered at the device size,
    // the edge between the squares is a sharp pixel edge, also at 3x.
    let image = svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2 2">
        <rect width="1" height="1"/><rect x="1" y="1" width="1" height="1"/></svg>"#,
    );
    let rect = Rect::new(10.0, 10.0, 20.0, 20.0);
    let images = OneImage(image, Some(VectorCache::default()));
    for scale in [1.0, 2.0, 3.0] {
        let p = render_with(vec![image_item(rect, rect)], scale, &images);
        let at = |x: f32, y: f32| rgb(&p, (x * scale) as u32, (y * scale) as u32);
        let edge = 20.0 * scale;
        assert_eq!(rgb(&p, edge as u32 - 1, (15.0 * scale) as u32), (0, 0, 0));
        assert_eq!(rgb(&p, edge as u32, (15.0 * scale) as u32), (255, 255, 255));
        assert_eq!(at(25.0, 25.0), (0, 0, 0));
        assert_eq!(at(5.0, 5.0), (255, 255, 255), "outside the image");
    }
}

#[test]
fn vector_images_tile_at_the_tile_size() {
    // A 10x10 tile: black left half.
    let image = svg(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4">
        <rect width="2" height="4"/></svg>"#,
    );
    let tile = Rect::new(0.0, 0.0, 10.0, 10.0);
    let area = Rect::new(0.0, 0.0, 40.0, 10.0);
    let p = render_with(vec![image_item(tile, area)], 1.0, &OneImage(image, None));
    for x in [0, 4, 10, 14, 30, 34] {
        assert_eq!(rgb(&p, x, 5), (0, 0, 0), "x = {x}");
        assert_eq!(rgb(&p, x + 5, 5), (255, 255, 255), "x = {}", x + 5);
    }
    assert_eq!(rgb(&p, 45, 5), (255, 255, 255), "outside the area");
}

#[test]
fn render_sizes_are_bounded() {
    let size = |w: f32, h: f32| {
        vector_render_size(Rect::new(0.0, 0.0, w, h)).map(|s| (s.width(), s.height()))
    };
    assert_eq!(size(10.4, 0.2), Some((10, 1)));
    assert_eq!(size(0.0, 10.0), None);
    assert_eq!(size(f32::NAN, 10.0), None);
    assert_eq!(size(f32::INFINITY, 10.0), None);
    for huge in [1e9, 1e30] {
        let (w, h) = size(huge, huge).unwrap();
        assert!(w * h <= MAX_RENDER_PIXELS && w == h && w > 1000, "{w}x{h}");
    }
    let (w, h) = size(1e6, 2.0).unwrap();
    assert_eq!((w, h), (MAX_DIMENSION, 1));
}

#[test]
fn opacity_group_is_composited() {
    let rect = Rect::new(20.0, 20.0, 10.0, 10.0);
    let p = render(vec![
        DisplayItem::PushOpacity {
            opacity: 0.5,
            bounds: rect,
            fixed_bounds: None,
            escapes_clips: false,
        },
        DisplayItem::Rect {
            rect,
            radii: [(0.0, 0.0); 4],
            color: RED,
        },
        DisplayItem::PopOpacity,
    ]);
    let (r, g, b) = rgb(&p, 25, 25);
    assert_eq!(r, 255);
    assert!((126..=129).contains(&g) && g == b, "{g} {b}");
    assert_eq!(rgb(&p, 50, 50), (255, 255, 255));
}

#[test]
fn transparent_group_draws_nothing() {
    let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
    let p = render(vec![
        DisplayItem::PushOpacity {
            opacity: 0.0,
            bounds: rect,
            fixed_bounds: None,
            escapes_clips: false,
        },
        DisplayItem::Rect {
            rect,
            radii: [(0.0, 0.0); 4],
            color: RED,
        },
        DisplayItem::PopOpacity,
    ]);
    assert_eq!(rgb(&p, 50, 50), (255, 255, 255));
}

#[test]
fn clip_applies_inside_opacity_layers() {
    let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
    let p = render(vec![
        DisplayItem::PushClip(Rect::new(0.0, 0.0, 50.0, 100.0)),
        DisplayItem::PushOpacity {
            opacity: 0.5,
            bounds: rect,
            fixed_bounds: None,
            escapes_clips: false,
        },
        DisplayItem::Rect {
            rect,
            radii: [(4.0, 4.0); 4],
            color: RED,
        },
        DisplayItem::PopOpacity,
        DisplayItem::PopClip,
    ]);
    assert_ne!(rgb(&p, 25, 50), (255, 255, 255));
    assert_eq!(rgb(&p, 75, 50), (255, 255, 255));
}

#[test]
fn snapping_keeps_thin_rects_visible() {
    assert_eq!(
        snap(Rect::new(0.4, 1.6, 2.2, 0.3)),
        Rect::new(0.0, 2.0, 3.0, 1.0)
    );
    assert_eq!(snap(Rect::new(1.0, 1.0, 0.0, 0.0)).width, 0.0);
}

// ----- Masks -----

/// A 20x20 SVG: black left half.
const LEFT_HALF: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><rect width="10" height="20"/></svg>"#;

fn mask_layer(image: MaskLayerImage) -> MaskLayer {
    MaskLayer {
        image,
        luminance: false,
        composite: swb_style::CompositeOperator::SourceOver,
    }
}

fn image_layer(area: Rect, tile: Rect) -> MaskLayer {
    mask_layer(MaskLayerImage::Image {
        image: ImageRef::Url("x".into()),
        area,
        tile,
    })
}

/// A red square at (20, 20) of 40x40, masked by `layers`.
fn masked_square(layers: Vec<MaskLayer>) -> Vec<DisplayItem> {
    let rect = Rect::new(20.0, 20.0, 40.0, 40.0);
    vec![
        DisplayItem::PushMask {
            bounds: rect,
            layers: Arc::from(layers),
        },
        DisplayItem::Rect {
            rect,
            radii: [(0.0, 0.0); 4],
            color: RED,
        },
        DisplayItem::PopMask,
    ]
}

#[test]
fn mask_multiplies_by_the_image_alpha() {
    let images = OneImage(svg(LEFT_HALF), Some(VectorCache::default()));
    // One 40x40 tile: the left half of the square stays.
    let rect = Rect::new(20.0, 20.0, 40.0, 40.0);
    for scale in [1.0, 2.0] {
        let p = render_with(masked_square(vec![image_layer(rect, rect)]), scale, &images);
        let at = |x: f32, y: f32| rgb(&p, (x * scale) as u32, (y * scale) as u32);
        assert_eq!(at(25.0, 30.0), (255, 0, 0), "scale {scale}");
        assert_eq!(at(39.0, 30.0), (255, 0, 0), "scale {scale}");
        assert_eq!(at(41.0, 30.0), (255, 255, 255), "scale {scale}");
        assert_eq!(at(10.0, 10.0), (255, 255, 255), "scale {scale}");
    }
}

#[test]
fn unavailable_layers_hide_the_group() {
    let rect = Rect::new(20.0, 20.0, 40.0, 40.0);
    // An image that is not loaded, and an empty layer.
    for layer in [image_layer(rect, rect), mask_layer(MaskLayerImage::Empty)] {
        let p = render_with(masked_square(vec![layer]), 1.0, &NoImages);
        assert_eq!(rgb(&p, 40, 40), (255, 255, 255));
    }
    // An opaque layer shows the part inside its area.
    let p = render(masked_square(vec![mask_layer(MaskLayerImage::Opaque(
        Rect::new(20.0, 20.0, 10.0, 40.0),
    ))]));
    assert_eq!(rgb(&p, 25, 40), (255, 0, 0));
    assert_eq!(rgb(&p, 35, 40), (255, 255, 255));
}

#[test]
fn luminance_masks_use_the_colors() {
    let grey = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><rect width="20" height="20" fill="rgb(128,128,128)"/></svg>"#;
    let images = OneImage(svg(grey), None);
    let rect = Rect::new(20.0, 20.0, 40.0, 40.0);
    let mut layer = image_layer(rect, rect);
    layer.luminance = true;
    let p = render_with(masked_square(vec![layer]), 1.0, &images);
    // Red at half coverage over white (Chromium: the same).
    let (r, g, b) = rgb(&p, 40, 40);
    assert_eq!(r, 255);
    assert!((126..=128).contains(&g) && g == b, "{g} {b}");
    // The alpha of the same image is opaque.
    let p = render_with(masked_square(vec![image_layer(rect, rect)]), 1.0, &images);
    assert_eq!(rgb(&p, 40, 40), (255, 0, 0));
}

#[test]
fn layers_composite_from_the_bottom() {
    use swb_style::CompositeOperator as Op;
    let images = OneImage(svg(LEFT_HALF), None);
    let square = Rect::new(20.0, 20.0, 40.0, 40.0);
    // Top: the left half (an image tile of the square); bottom: the
    // top half (opaque).
    let top_half = mask_layer(MaskLayerImage::Opaque(Rect::new(20.0, 20.0, 40.0, 20.0)));
    let quadrants = |op: Op| -> [bool; 4] {
        let mut top = image_layer(square, square);
        top.composite = op;
        let p = render_with(masked_square(vec![top, top_half.clone()]), 1.0, &images);
        // Top left, top right, bottom left, bottom right.
        [(30, 30), (50, 30), (30, 50), (50, 50)].map(|(x, y)| rgb(&p, x, y) == (255, 0, 0))
    };
    assert_eq!(quadrants(Op::SourceOver), [true, true, true, false]);
    assert_eq!(quadrants(Op::SourceIn), [true, false, false, false]);
    assert_eq!(quadrants(Op::SourceOut), [false, false, true, false]);
    assert_eq!(quadrants(Op::Xor), [false, true, true, false]);
    assert_eq!(quadrants(Op::DestinationOut), [false, true, false, false]);
    // The operator of the bottom layer is ignored.
    let mut bottom = top_half.clone();
    bottom.composite = Op::SourceIn;
    let p = render(masked_square(vec![bottom]));
    assert_eq!(rgb(&p, 30, 30), (255, 0, 0));
}

#[test]
fn gradient_masks_tile() {
    use swb_style::{Color, LinearGradient};
    // Opaque on the left, transparent on the right of each tile.
    let gradient = Arc::new(LinearGradient {
        angle_deg: 90.0,
        stops: vec![
            (Color::Rgba(Rgba::BLACK), None),
            (Color::Rgba(Rgba::TRANSPARENT), None),
        ],
        repeating: false,
    });
    let layer = |tile: Rect| {
        mask_layer(MaskLayerImage::Gradient {
            gradient: Arc::clone(&gradient),
            current_color: Rgba::BLACK,
            area: Rect::new(20.0, 20.0, 40.0, 40.0),
            tile,
        })
    };
    // Four 10 px wide tiles.
    let p = render(masked_square(vec![layer(Rect::new(
        20.0, 20.0, 10.0, 40.0,
    ))]));
    for tile in 0..4 {
        let x = 20 + tile * 10;
        let left = rgb(&p, x, 40);
        let right = rgb(&p, x + 9, 40);
        assert!(left.1 < 40, "tile {tile} left {left:?}");
        assert!(right.1 > 200, "tile {tile} right {right:?}");
    }
    // One tile: the gradient spans the square.
    let p = render(masked_square(vec![layer(Rect::new(
        20.0, 20.0, 40.0, 40.0,
    ))]));
    assert!(rgb(&p, 21, 40).1 < 20);
    assert!((100..160).contains(&rgb(&p, 40, 40).1));
    assert!(rgb(&p, 59, 40).1 > 230);
    // Twenty 2 px wide tiles: one tile is rendered and repeated.
    let p = render(masked_square(vec![layer(Rect::new(20.0, 20.0, 2.0, 40.0))]));
    for x in [20, 30, 58] {
        assert!(rgb(&p, x, 40).1 < 128, "x {x} {:?}", rgb(&p, x, 40));
        assert!(
            rgb(&p, x + 1, 40).1 > 128,
            "x {} {:?}",
            x + 1,
            rgb(&p, x + 1, 40)
        );
    }
    // A hard stop in a tile much larger than the visible area stays
    // sharp (each visible tile is drawn as a gradient).
    let hard = Arc::new(LinearGradient {
        angle_deg: 90.0,
        stops: vec![
            (
                Color::Rgba(Rgba::BLACK),
                Some(swb_style::LengthPercentage::Percent(0.5)),
            ),
            (
                Color::Rgba(Rgba::TRANSPARENT),
                Some(swb_style::LengthPercentage::Percent(0.5)),
            ),
        ],
        repeating: false,
    });
    let layer = mask_layer(MaskLayerImage::Gradient {
        gradient: hard,
        current_color: Rgba::BLACK,
        area: Rect::new(-500.0, -500.0, 2000.0, 2000.0),
        tile: Rect::new(20.0, 20.0, 40.0, 500.0),
    });
    let p = render(masked_square(vec![layer]));
    assert_eq!(rgb(&p, 39, 30), (255, 0, 0));
    assert_eq!(rgb(&p, 40, 30), (255, 255, 255));
}

/// A rasterizer for `target` at scale 1 with the given budgets.
fn rasterizer<'a>(
    target: &'a mut Pixmap,
    vectors: &'a mut FrameBudget,
    max_layer_pixels: u64,
    max_mask_pixels: u64,
) -> Rasterizer<'a> {
    let target_size = (target.width(), target.height());
    Rasterizer {
        target: target.data_mut(),
        target_size,
        offset_y: 0.0,
        params: RasterParams {
            scroll: Point::default(),
            viewport_scroll: Point::default(),
            scale: 1.0,
        },
        layers: Vec::new(),
        budget: Budget {
            max_layer_pixels,
            max_mask_pixels,
            max_transform_pixels: MAX_TRANSFORM_PIXELS,
            max_path_work: path::MAX_PATH_WORK,
            ..Budget::default()
        },
        clips: Vec::new(),
        mask: None,
        vectors,
        translation: Point::default(),
        visible: Rect::new(0.0, 0.0, target_size.0 as f32, target_size.1 as f32),
        viewport_scroll: Point::default(),
        fixed_origin: Point::default(),
        viewport_clip: None,
        depth: 0,
        frames: FrameNesting::default(),
        sticky: StickyCache::default(),
    }
}

/// A gradient mask layer of opaque-to-transparent tiles in `area`.
fn gradient_layer(area: Rect, tile: Rect) -> MaskLayer {
    use swb_style::{Color, LinearGradient};
    mask_layer(MaskLayerImage::Gradient {
        gradient: Arc::new(LinearGradient {
            angle_deg: 90.0,
            stops: vec![
                (Color::Rgba(Rgba::BLACK), None),
                (Color::Rgba(Rgba::TRANSPARENT), None),
            ],
            repeating: false,
        }),
        current_color: Rgba::BLACK,
        area,
        tile,
    })
}

#[test]
fn rendered_gradient_tiles_are_part_of_the_admitted_work() {
    let rect = Rect::new(20.0, 20.0, 40.0, 40.0);
    // Twenty 2 px wide tiles: one tile is rendered and repeated.
    let layers = vec![gradient_layer(rect, Rect::new(20.0, 20.0, 2.0, 40.0))];
    let mut target = Pixmap::new(100, 100).unwrap();
    let mut vectors = FrameBudget::new();
    let r = rasterizer(
        &mut target,
        &mut vectors,
        MAX_GROUP_LAYER_PIXELS,
        MAX_MASK_PIXELS,
    );
    let work = r.mask_work(rect, &layers);
    assert!(work > 1600 + 40 * ROW_COST_PIXELS, "{work}");
    let masked = masked_square(layers);
    let (p, skipped) = render_with_budgets(&masked, MAX_GROUP_LAYER_PIXELS, work, 0);
    assert!(!skipped);
    let (r, g, _) = rgb(&p, 20, 40);
    assert!(r == 255 && g < 128, "drawn: {:?}", rgb(&p, 20, 40));
    let (p, skipped) = render_with_budgets(&masked, MAX_GROUP_LAYER_PIXELS, work - 1, 0);
    assert!(skipped);
    assert_eq!(rgb(&p, 20, 40), (255, 255, 255));
}

#[test]
fn hostile_tile_plans_do_not_overflow() {
    let mut target = Pixmap::new(100, 100).unwrap();
    let mut vectors = FrameBudget::new();
    let r = rasterizer(
        &mut target,
        &mut vectors,
        MAX_GROUP_LAYER_PIXELS,
        MAX_MASK_PIXELS,
    );
    let visible = Rect::new(0.0, 0.0, 100.0, 1.0);
    let cases = [
        Rect::new(400.0, -33_000_000.0, 1e-7, 1.0),
        Rect::new(400.0, -33_000_000.0, 1.0, 1e-7),
        Rect::new(-3e7, -3e7, 1e-30, 1e-30),
        Rect::new(f32::NAN, 0.0, 1.0, 1.0),
        Rect::new(0.0, 0.0, f32::INFINITY, 1.0),
    ];
    for tile in cases {
        let plan = r.tile_plan(Rect::new(0.0, 0.0, 100.0, 1.0), tile, visible);
        let _ = plan.work();
        if let TilePlan::Copies { columns, rows, .. } = plan {
            let count = (columns.1 - columns.0) * (rows.1 - rows.0);
            assert!((1..=MAX_GRADIENT_TILES).contains(&count), "{tile:?}");
        }
    }
}

#[test]
fn strips_round_like_one_pass() {
    // Rectangles with edges at many fractional device positions, at a
    // fractional scale: strips of 7 rows give the same pixels.
    let items: Vec<DisplayItem> = (0..60)
        .map(|i| DisplayItem::Rect {
            rect: Rect::new(
                1.0 + i as f32 * 0.37,
                0.5 + i as f32 * 1.13,
                5.0 + i as f32 * 0.21,
                0.4 + (i % 7) as f32 * 0.3,
            ),
            radii: [(0.0, 0.0); 4],
            color: if i % 2 == 0 { RED } else { BLUE },
        })
        .collect();
    let list = DisplayList { items };
    let render = |strip_rows: u32| {
        let mut target = Pixmap::new(90, 210).unwrap();
        target.fill(tiny_skia::Color::WHITE);
        let params = RasterParams {
            scroll: Point::new(0.0, 0.3),
            viewport_scroll: Point::new(0.0, 0.3),
            scale: 2.6,
        };
        let mut fonts = FontContext::for_tests();
        rasterize_in_strips(
            &list,
            &mut target,
            params,
            strip_rows,
            &mut fonts,
            &NoImages,
        );
        target
    };
    let whole = render(210);
    for rows in [7, 13, 100] {
        assert!(whole.data() == render(rows).data(), "strips of {rows} rows");
    }
}

/// A list with a rotated group (with an opacity group and a mask group
/// inside) and a fixed group, for strips.
fn transformed_and_fixed_groups() -> DisplayList {
    use swb_layout::GroupTransform;
    let (sin, cos) = 25f32.to_radians().sin_cos();
    let (cx, cy) = (45.0, 60.0);
    let rotate = Matrix::new(
        cos,
        sin,
        -sin,
        cos,
        cx - cos * cx + sin * cy,
        cy - sin * cx - cos * cy,
    );
    let rect = |x, y, w, h, color| DisplayItem::Rect {
        rect: Rect::new(x, y, w, h),
        radii: [(0.0, 0.0); 4],
        color,
    };
    let mut items = vec![
        DisplayItem::PushTransform {
            transform: GroupTransform::Matrix(rotate),
            bounds: Rect::new(20.0, 20.0, 50.0, 100.0),
            hit_bounds: Rect::default(),
            fixed_bounds: None,
        },
        rect(30.0, 40.0, 30.0, 80.0, BLUE),
        DisplayItem::PushOpacity {
            opacity: 0.5,
            bounds: Rect::new(25.0, 90.0, 40.0, 20.0),
            fixed_bounds: None,
            escapes_clips: false,
        },
        rect(25.0, 90.0, 40.0, 20.0, RED),
        DisplayItem::PopOpacity,
    ];
    let opaque = mask_layer(MaskLayerImage::Opaque(Rect::new(20.0, 20.0, 20.0, 40.0)));
    items.extend(masked_square(vec![opaque]));
    items.extend([
        DisplayItem::PopTransform,
        DisplayItem::PushViewportClip,
        DisplayItem::PushTransform {
            transform: GroupTransform::Fixed,
            bounds: Rect::new(10.0, 70.0, 20.0, 40.0),
            hit_bounds: Rect::default(),
            fixed_bounds: None,
        },
        rect(10.0, 70.0, 20.0, 40.0, Rgba::rgb(0, 128, 0)),
        DisplayItem::PopTransform,
        DisplayItem::PopClip,
    ]);
    DisplayList { items }
}

/// Strips give the pixels of one pass (within rounding) for transform
/// layers, and fixed groups are at the same place in every strip.
#[test]
fn transform_and_fixed_groups_in_strips() {
    let list = transformed_and_fixed_groups();
    let render = |strip_rows: u32| {
        let mut target = Pixmap::new(90, 210).unwrap();
        target.fill(tiny_skia::Color::WHITE);
        let params = RasterParams {
            scroll: Point::new(0.0, 10.0),
            viewport_scroll: Point::new(0.0, 10.0),
            scale: 1.5,
        };
        let mut fonts = FontContext::for_tests();
        rasterize_in_strips(
            &list,
            &mut target,
            params,
            strip_rows,
            &mut fonts,
            &NoImages,
        );
        target
    };
    let whole = render(210);
    // The fixed group moves with the scroll offset in every strip.
    let green = (0, 128, 0);
    assert_eq!(rgb(&whole, 30, 106), green);
    assert_eq!(rgb(&whole, 30, 164), green);
    assert_ne!(rgb(&whole, 30, 103), green);
    assert_ne!(rgb(&whole, 30, 166), green);
    for rows in [7, 13, 100] {
        let strips = render(rows);
        let diff: Vec<u8> = whole
            .data()
            .iter()
            .zip(strips.data())
            .map(|(a, b)| a.abs_diff(*b))
            .collect();
        let max = diff.iter().copied().max().unwrap_or(0);
        let count = diff.chunks(4).filter(|p| p.iter().any(|&d| d > 0)).count();
        // Transform layers have their pixels at the same positions in
        // every strip; rounding differs in a few pixels.
        assert!(max <= 16 && count <= 10, "strips of {rows} rows");
        if rows == 100 {
            assert_eq!(count, 0, "strips of {rows} rows");
        }
    }
}

/// Runs `items` on a white 100x100 target with the given budgets of
/// group layers and transform layers, and returns the target and the
/// rasterizer's budget afterwards.
fn run_with_budgets(
    items: &[DisplayItem],
    max_layer_pixels: u64,
    max_transform_pixels: u64,
) -> (Pixmap, Budget) {
    let mut target = Pixmap::new(100, 100).unwrap();
    target.fill(tiny_skia::Color::WHITE);
    let mut vectors = FrameBudget::new();
    let budget = {
        let mut r = rasterizer(&mut target, &mut vectors, max_layer_pixels, MAX_MASK_PIXELS);
        r.budget.max_transform_pixels = max_transform_pixels;
        let ends = transform_ends(items);
        let mut fonts = FontContext::for_tests();
        r.run(items, 0..items.len(), &ends, &mut fonts, &NoImages);
        assert!(r.layers.is_empty());
        r.budget
    };
    (target, budget)
}

/// A 40x40 red square at (30, 30) rotated by 45 degrees about its
/// center, with an opacity group and a mask group inside.
fn rotated_square() -> Vec<DisplayItem> {
    use swb_layout::GroupTransform;
    let (sin, cos) = 45f32.to_radians().sin_cos();
    let rotate = Matrix::new(
        cos,
        sin,
        -sin,
        cos,
        50.0 - cos * 50.0 + sin * 50.0,
        50.0 - sin * 50.0 - cos * 50.0,
    );
    let rect = Rect::new(30.0, 30.0, 40.0, 40.0);
    let mut items = vec![
        DisplayItem::PushTransform {
            transform: GroupTransform::Matrix(rotate),
            bounds: rect,
            hit_bounds: Rect::default(),
            fixed_bounds: None,
        },
        DisplayItem::PushOpacity {
            opacity: 1.0,
            bounds: rect,
            fixed_bounds: None,
            escapes_clips: false,
        },
    ];
    items.extend(masked_square(vec![mask_layer(MaskLayerImage::Opaque(
        Rect::new(0.0, 0.0, 100.0, 100.0),
    ))]));
    items.extend([
        DisplayItem::Rect {
            rect,
            radii: [(0.0, 0.0); 4],
            color: RED,
        },
        DisplayItem::PopOpacity,
        DisplayItem::PopTransform,
    ]);
    items
}

#[test]
fn transform_layers_use_the_budgets_of_the_strip() {
    let items = rotated_square();
    // The layer, the opacity layer and the mask layers are released;
    // the work is the layer's pixels and the device pixels it covers.
    let (p, budget) = run_with_budgets(&items, MAX_GROUP_LAYER_PIXELS, MAX_TRANSFORM_PIXELS);
    assert_eq!(rgb(&p, 50, 50), (255, 0, 0));
    assert_eq!(budget.layer_pixels, 0);
    assert!(
        (3200..10_000).contains(&budget.transform_pixels),
        "{}",
        budget.transform_pixels
    );
    assert!(!budget.skipped.transforms && !budget.skipped.layers);
    let work = budget.transform_pixels;
    // Not enough work budget: not drawn.
    let (p, budget) = run_with_budgets(&items, MAX_GROUP_LAYER_PIXELS, work - 1);
    assert_eq!(rgb(&p, 50, 50), (255, 255, 255));
    assert_eq!(budget.transform_pixels, 0);
    assert!(budget.skipped.transforms);
    // Not enough layer memory for the transform layer: not drawn.
    let (p, budget) = run_with_budgets(&items, 100, MAX_TRANSFORM_PIXELS);
    assert_eq!(rgb(&p, 50, 50), (255, 255, 255));
    assert_eq!(budget.layer_pixels, 0);
    assert!(budget.skipped.layers);
}

#[test]
fn transform_layers_nest_up_to_a_depth() {
    use swb_layout::GroupTransform;
    let rotate = GroupTransform::Matrix(Matrix::new(0.0, 1.0, -1.0, 0.0, 100.0, 0.0));
    let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
    let nested = |depth: usize| {
        let mut items: Vec<DisplayItem> = (0..depth)
            .map(|_| DisplayItem::PushTransform {
                transform: rotate.clone(),
                bounds: rect,
                hit_bounds: Rect::default(),
                fixed_bounds: None,
            })
            .collect();
        items.push(DisplayItem::Rect {
            rect,
            radii: [(0.0, 0.0); 4],
            color: RED,
        });
        items.extend((0..depth).map(|_| DisplayItem::PopTransform));
        items
    };
    let (p, budget) = run_with_budgets(
        &nested(MAX_TRANSFORM_DEPTH),
        MAX_GROUP_LAYER_PIXELS,
        MAX_TRANSFORM_PIXELS,
    );
    assert_eq!(rgb(&p, 50, 50), (255, 0, 0));
    assert!(!budget.skipped.transforms);
    assert_eq!(budget.layer_pixels, 0);
    let (p, budget) = run_with_budgets(
        &nested(MAX_TRANSFORM_DEPTH + 1),
        MAX_GROUP_LAYER_PIXELS,
        MAX_TRANSFORM_PIXELS,
    );
    assert_eq!(rgb(&p, 50, 50), (255, 255, 255));
    assert!(budget.skipped.transforms);
    assert_eq!(budget.layer_pixels, 0);
}

#[test]
fn transform_layer_geometry_is_bounded() {
    let area = Rect::new(0.0, 0.0, 100.0, 50.0);
    assert_eq!(layer_scale(area, f32::NAN), None);
    assert_eq!(layer_scale(area, 0.0), None);
    assert_eq!(layer_scale(area, f32::INFINITY), None);
    // Sides and pixels are limited.
    let k = layer_scale(Rect::new(0.0, 0.0, 1e6, 1.0), 1.0).unwrap();
    assert!(1e6 * k <= MAX_TRANSFORM_LAYER_SIDE * 1.001);
    let k = layer_scale(Rect::new(0.0, 0.0, 1e4, 1e4), 1.0).unwrap();
    assert!(1e8 * k * k <= MAX_TRANSFORM_LAYER_PIXELS * 1.001);
    assert_eq!(
        layer_size(Rect::new(0.0, 0.0, f32::NAN, 1.0), 1.0),
        Some((1, 1))
    );
    assert_eq!(layer_size(Rect::new(0.0, 0.0, 1e9, 1.0), 1.0), None);
    // The grid starts at the corner of the bounds; two pixels of
    // margin, inside the bounds.
    let bounds = Rect::new(10.0, 10.0, 100.0, 100.0);
    let grid = grid_area(Rect::new(30.3, 40.7, 10.0, 10.0), bounds, 2.0);
    assert_eq!((grid.x, grid.y), (29.0, 39.5));
    assert!((grid.right() - 41.3).abs() < 1e-4 && (grid.bottom() - 51.7).abs() < 1e-4);
    assert_eq!(grid_area(bounds, bounds, 2.0), bounds);
    let nan = grid_area(Rect::new(f32::NAN, 0.0, 1.0, 1.0), bounds, 2.0);
    assert!(nan.width >= 0.0 || nan.width.is_nan());
}

#[test]
fn strips_have_equal_heights() {
    assert_eq!(equal_strip_rows(30_000, 20_972), 15_000);
    assert_eq!(equal_strip_rows(30_001, 20_972), 15_001);
    assert_eq!(equal_strip_rows(100, 100), 100);
    assert_eq!(equal_strip_rows(100, 1000), 100);
    assert_eq!(equal_strip_rows(100, 0), 1);
    assert_eq!(equal_strip_rows(0, 5), 1);
    assert_eq!(equal_strip_rows(10, 3), 3);
}

#[test]
fn layer_pixels_are_bounded() {
    let mut target = Pixmap::new(1, 1).unwrap();
    let mut vectors = FrameBudget::new();
    let mut r = rasterizer(&mut target, &mut vectors, 100, MAX_MASK_PIXELS);
    r.budget.layer_pixels = 90;
    let small = r.layer_pixmap(2, 5).unwrap();
    assert_eq!(r.budget.layer_pixels, 100);
    assert!(r.layer_pixmap(1, 1).is_none());
    assert!(r.budget.skipped.layers);
    r.release(&small);
    assert!(r.layer_pixmap(1, 1).is_some());
}

/// Rasterizes `items` on a white 100x100 target with the given
/// budgets, after `mask_work` pixels of mask work.
fn render_with_budgets(
    items: &[DisplayItem],
    max_layer_pixels: u64,
    max_mask_pixels: u64,
    mask_work: u64,
) -> (Pixmap, bool) {
    let mut target = Pixmap::new(100, 100).unwrap();
    target.fill(tiny_skia::Color::WHITE);
    let mut vectors = FrameBudget::new();
    let mut r = rasterizer(&mut target, &mut vectors, max_layer_pixels, max_mask_pixels);
    r.budget.mask_pixels = mask_work;
    let mut fonts = FontContext::for_tests();
    for item in items {
        r.item(item, &mut fonts, &NoImages);
    }
    assert!(r.layers.is_empty());
    assert_eq!(r.budget.layer_pixels, 0, "all layers are released");
    let masks_skipped = r.budget.skipped.masks;
    (target, masks_skipped)
}

fn render_with_layer_budget(items: &[DisplayItem], max_layer_pixels: u64) -> Pixmap {
    render_with_budgets(items, max_layer_pixels, MAX_MASK_PIXELS, 0).0
}

#[test]
fn mask_groups_start_if_their_work_fits() {
    let rect = Rect::new(20.0, 20.0, 40.0, 40.0);
    let opaque = || mask_layer(MaskLayerImage::Opaque(rect));
    // Three layers of 40x40 px: (1600 + 40 rows * 16) * 3 = 6720 px of
    // work.
    let masked = masked_square(vec![opaque(), opaque(), opaque()]);
    let (p, skipped) = render_with_budgets(&masked, MAX_GROUP_LAYER_PIXELS, 6720, 0);
    assert_eq!(rgb(&p, 40, 40), (255, 0, 0));
    assert!(!skipped);
    // It does not fit after other work: no layer, nothing drawn.
    let (p, skipped) = render_with_budgets(&masked, MAX_GROUP_LAYER_PIXELS, 6720, 1);
    assert_eq!(rgb(&p, 40, 40), (255, 255, 255));
    assert!(skipped);
    // A second group (20x20 px: 400 + 20 * 16 = 720) after the first.
    let small = Rect::new(70.0, 70.0, 20.0, 20.0);
    let mut both = masked.clone();
    both.extend([
        DisplayItem::PushMask {
            bounds: small,
            layers: Arc::from([mask_layer(MaskLayerImage::Opaque(small))]),
        },
        DisplayItem::Rect {
            rect: small,
            radii: [(0.0, 0.0); 4],
            color: RED,
        },
        DisplayItem::PopMask,
    ]);
    let (p, skipped) = render_with_budgets(&both, MAX_GROUP_LAYER_PIXELS, 7439, 0);
    assert_eq!(rgb(&p, 40, 40), (255, 0, 0));
    assert_eq!(rgb(&p, 80, 80), (255, 255, 255));
    assert!(skipped);
    let (p, _) = render_with_budgets(&both, MAX_GROUP_LAYER_PIXELS, 7440, 0);
    assert_eq!(rgb(&p, 80, 80), (255, 0, 0));
}

#[test]
fn strips_render_like_one_pass() {
    // A rounded rectangle, an opacity group and a masked group across
    // the strip boundaries.
    let images = OneImage(svg(LEFT_HALF), Some(VectorCache::default()));
    let rect = Rect::new(10.0, 15.0, 70.0, 70.0);
    let items = vec![
        DisplayItem::Rect {
            rect: Rect::new(5.0, 5.0, 90.0, 90.0),
            radii: [(9.0, 9.0); 4],
            color: BLUE,
        },
        DisplayItem::PushOpacity {
            opacity: 0.5,
            bounds: rect,
            fixed_bounds: None,
            escapes_clips: false,
        },
        DisplayItem::PushMask {
            bounds: rect,
            layers: Arc::from([image_layer(rect, Rect::new(10.0, 15.0, 20.0, 20.0))]),
        },
        DisplayItem::Rect {
            rect,
            radii: [(0.0, 0.0); 4],
            color: RED,
        },
        DisplayItem::PopMask,
        DisplayItem::PopOpacity,
    ];
    let render = |strip_rows: u32| {
        let params = RasterParams {
            scroll: Point::default(),
            viewport_scroll: Point::default(),
            scale: 2.0,
        };
        let mut target = Pixmap::new(200, 200).unwrap();
        target.fill(tiny_skia::Color::WHITE);
        let mut fonts = FontContext::for_tests();
        let list = DisplayList {
            items: items.clone(),
        };
        rasterize_in_strips(&list, &mut target, params, strip_rows, &mut fonts, &images);
        target
    };
    let whole = render(200);
    // Exact where no anti-aliased curve crosses a strip boundary (the
    // rounded corners are rows 10..28 and 172..190).
    for rows in [64, 199] {
        assert!(whole.data() == render(rows).data(), "strips of {rows} rows");
    }
    // Otherwise tiny-skia clips the path edges to the strip, which
    // changes the coverage of single anti-aliased edge pixels of the
    // corners. Everything else (the opacity and mask groups, rows
    // 30..170) is the same.
    for rows in [1, 7] {
        let strips = render(rows);
        let differing: Vec<usize> = whole
            .pixels()
            .iter()
            .zip(strips.pixels())
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, _)| i / 200)
            .collect();
        assert!(
            differing.iter().all(|row| !(30..170).contains(row)),
            "strips of {rows}"
        );
        assert!(
            differing.len() < 200,
            "strips of {rows}: {}",
            differing.len()
        );
    }
}

#[test]
fn groups_beyond_the_layer_budget() {
    let rect = Rect::new(20.0, 20.0, 40.0, 40.0);
    let square = DisplayItem::Rect {
        rect,
        radii: [(0.0, 0.0); 4],
        color: RED,
    };
    let opacity = vec![
        DisplayItem::PushOpacity {
            opacity: 0.5,
            bounds: rect,
            fixed_bounds: None,
            escapes_clips: false,
        },
        square.clone(),
        DisplayItem::PopOpacity,
    ];
    // Within the budget: composited with the opacity.
    let p = render_with_layer_budget(&opacity, 10_000);
    assert_ne!(rgb(&p, 40, 40), (255, 0, 0));
    // Beyond it: drawn directly, without the opacity.
    let p = render_with_layer_budget(&opacity, 100);
    assert_eq!(rgb(&p, 40, 40), (255, 0, 0));
    // A mask group needs its layer and a temporary one; beyond the
    // budget it draws nothing.
    let opaque = mask_layer(MaskLayerImage::Opaque(rect));
    let mut masked = masked_square(vec![opaque]);
    let p = render_with_layer_budget(&masked, 2 * 1600);
    assert_eq!(rgb(&p, 40, 40), (255, 0, 0));
    let p = render_with_layer_budget(&masked, 2 * 1600 - 1);
    assert_eq!(rgb(&p, 40, 40), (255, 255, 255));
    // A masked group inside an opacity group: three layers.
    masked.insert(
        0,
        DisplayItem::PushOpacity {
            opacity: 0.5,
            bounds: rect,
            fixed_bounds: None,
            escapes_clips: false,
        },
    );
    masked.push(DisplayItem::PopOpacity);
    let p = render_with_layer_budget(&masked, 3 * 1600);
    let (r, g, b) = rgb(&p, 40, 40);
    assert!(
        r == 255 && (120..=135).contains(&g) && g == b,
        "{r} {g} {b}"
    );
    let p = render_with_layer_budget(&masked, 3 * 1600 - 1);
    assert_eq!(rgb(&p, 40, 40), (255, 255, 255));
}

#[test]
fn opaque_layers_are_opaque_for_luminance() {
    let rect = Rect::new(20.0, 20.0, 40.0, 40.0);
    let mut layer = mask_layer(MaskLayerImage::Opaque(rect));
    layer.luminance = true;
    let p = render(masked_square(vec![layer]));
    assert_eq!(rgb(&p, 40, 40), (255, 0, 0));
}

mod backdrop;
mod frame;
mod gradient;
mod svg;
