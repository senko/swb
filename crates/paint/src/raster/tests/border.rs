//! The borders: the colors, corners and joins of the sides, and the
//! shading of the border styles.

use super::*;

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
fn a_transparent_side_keeps_its_part_of_the_corners() {
    // Bootstrap's `.spinner-border`: 32 px, `border: 4px solid #222;
    // border-right-color: transparent; border-radius: 50%`. The join of
    // the top and the transparent right side is the diagonal through the
    // corner, so the C ends at 45° (Chromium's pixels, probe case
    // `spinner` of `tools/probes/border-radius-sides.json`).
    let dark = Rgba::rgb(34, 34, 34);
    let item = DisplayItem::Border {
        rect: Rect::new(10.0, 10.0, 32.0, 32.0),
        widths: [4.0; 4],
        colors: [dark, Rgba::new(0, 0, 0, 0), dark, dark],
        styles: [BorderStyle::Solid; 4],
        radii: [(16.0, 16.0); 4],
    };
    let p = render(vec![item]);
    let at = |x: u32, y: u32| rgb(&p, 10 + x, 10 + y);
    let white = (255, 255, 255);
    for (x, y) in [
        (23, 5),
        (21, 3),
        (3, 15),
        (15, 3),
        (15, 29),
        (23, 27),
        (7, 25),
    ] {
        assert_eq!(at(x, y), (34, 34, 34), "({x}, {y})");
    }
    for (x, y) in [(29, 15), (29, 17), (27, 9), (27, 23), (29, 11), (29, 21)] {
        assert_eq!(at(x, y), white, "({x}, {y})");
    }
}

#[test]
fn the_join_of_unequal_widths_follows_the_ratio_of_the_widths() {
    // 40 px circle, widths 10, 2, 6, 4 and four colors: the join of two
    // sides is the line through the outer and the inner corner, also where
    // the ring lies beyond the inner corner (Chromium's pixels, probe case
    // `unequal-widths-circle`).
    let (red, green) = (Rgba::rgb(204, 0, 0), Rgba::rgb(0, 170, 0));
    let (blue, yellow) = (Rgba::rgb(0, 0, 204), Rgba::rgb(204, 153, 0));
    let item = DisplayItem::Border {
        rect: Rect::new(10.0, 10.0, 40.0, 40.0),
        widths: [10.0, 2.0, 6.0, 4.0],
        colors: [red, green, blue, yellow],
        styles: [BorderStyle::Solid; 4],
        radii: [(20.0, 20.0); 4],
    };
    let p = render(vec![item]);
    let at = |x: u32, y: u32| rgb(&p, 10 + x, 10 + y);
    // (7, 13) is on the top side, right of the join line from (0, 0)
    // through (4, 10); the swb of before painted the left color.
    let (r, g, _) = at(7, 13);
    assert!(r > 200 && g < 80, "{:?}", at(7, 13));
    assert_eq!(at(4, 13), (204, 153, 0));
    assert_eq!(at(4, 31), (204, 153, 0));
    assert_eq!(at(34, 10), (204, 0, 0));
    assert_eq!(at(20, 4), (204, 0, 0));
    assert_eq!(at(37, 16), (0, 170, 0));
    assert_eq!(at(31, 34), (0, 0, 204));
    assert_eq!(at(19, 37), (0, 0, 204));
}

#[test]
fn the_square_corners_of_a_partly_rounded_border_have_a_seam() {
    // A link of a pagination, the last of two: a 1 px border in the color
    // of its background, rounded at the right only, over the border of the
    // first. Chromium paints the sides one by one: the pixel of a square
    // corner is covered 0.79 by the border (probe case
    // `pagination-26-25-16` of `tools/probes/border-radius-pagination.json`:
    // 135 over 222 with the border at 112). swb covers it 0.75 (140): the
    // difference is anti-aliasing.
    let color = Rgba::rgb(112, 146, 255);
    let rect = Rect::new(10.0, 10.0, 25.0, 31.0);
    let item = DisplayItem::Border {
        rect,
        widths: [1.0; 4],
        colors: [color; 4],
        styles: [BorderStyle::Solid; 4],
        radii: [(0.0, 0.0), (6.0, 6.0), (6.0, 6.0), (0.0, 0.0)],
    };
    let under = DisplayItem::Rect {
        rect: Rect::new(0.0, 10.0, 11.0, 1.0),
        radii: [(0.0, 0.0); 4],
        color: Rgba::rgb(222, 226, 230),
    };
    let p = render(vec![under, item]);
    assert_eq!(rgb(&p, 15, 10), (112, 146, 255), "the straight top border");
    let (r, _, _) = rgb(&p, 10, 10);
    assert!((130..=140).contains(&r), "{:?}", rgb(&p, 10, 10));
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
