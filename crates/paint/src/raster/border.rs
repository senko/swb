//! Borders in the rasterizer: the geometry of a border box in device px, the
//! polygons of its sides, and the drawing of solid, dashed and dotted sides
//! with rounded corners (CSS Backgrounds 3 §5).

use swb_layout::Rect;
use swb_style::{BorderStyle, Rgba};
use tiny_skia::{Path, PathBuilder, Stroke, StrokeDash};

use super::{
    FillRule, Mask, Paint, Pixmap, PixmapPaint, Radii, Rasterizer, Transform, has_radius,
    push_rounded_rect, scale_radii, shade, snap, snap_out, solid_paint,
};

/// The geometry of a border box in device px.
struct BorderGeometry {
    /// The outer edge, snapped to pixels if there are no rounded corners.
    outer: Rect,
    /// Widths: top, right, bottom, left.
    widths: [f32; 4],
    /// Outer corner radii.
    radii: Radii,
    has_radius: bool,
}

impl BorderGeometry {
    fn new(outer: Rect, widths: [f32; 4], radii: Radii) -> Self {
        let has_radius = has_radius(&radii);
        BorderGeometry {
            outer: if has_radius { outer } else { snap(outer) },
            widths,
            radii,
            has_radius,
        }
    }

    /// The edge at `fraction` of the border width from the outer edge (0
    /// is the outer edge, 1 the inner edge), with its corner radii.
    fn edge(&self, fraction: f32) -> (Rect, Radii) {
        let [top, right, bottom, left] = self.widths.map(|w| w * fraction);
        let o = self.outer;
        let rect = Rect::new(
            o.x + left,
            o.y + top,
            (o.width - left - right).max(0.0),
            (o.height - top - bottom).max(0.0),
        );
        if !self.has_radius {
            return (snap(rect), [(0.0, 0.0); 4]);
        }
        let r = self.radii;
        let radii = [
            ((r[0].0 - left).max(0.0), (r[0].1 - top).max(0.0)),
            ((r[1].0 - right).max(0.0), (r[1].1 - top).max(0.0)),
            ((r[2].0 - right).max(0.0), (r[2].1 - bottom).max(0.0)),
            ((r[3].0 - left).max(0.0), (r[3].1 - bottom).max(0.0)),
        ];
        (rect, radii)
    }

    /// The area between the edges at `from` and `to` (see [`Self::edge`]),
    /// filled with the even-odd rule.
    fn ring(&self, from: f32, to: f32) -> Option<Path> {
        let (outer, outer_radii) = self.edge(from);
        let (inner, inner_radii) = self.edge(to);
        let mut pb = PathBuilder::new();
        push_rounded_rect(&mut pb, outer, &outer_radii);
        push_rounded_rect(&mut pb, inner, &inner_radii);
        pb.finish()
    }
}

/// The area of each side (top, right, bottom, left) between the outer and
/// inner border edges, split at the lines through the corners. Without
/// rounded corners the sides are trapezoids. With rounded corners the
/// sides are extended to the center of the box, so that together they
/// cover the whole rounded ring, including the curved corner areas inside
/// the inner rectangle. A corner next to a side that is not painted
/// (`painted[i]` is false) belongs entirely to the painted side, so its
/// tapering end is drawn as in browsers.
fn side_polygons(
    outer: Rect,
    inner: Rect,
    to_center: bool,
    painted: [bool; 4],
) -> [Vec<(f32, f32)>; 4] {
    let outer_corners = [
        (outer.x, outer.y),
        (outer.right(), outer.y),
        (outer.right(), outer.bottom()),
        (outer.x, outer.bottom()),
    ];
    let inner_corners = [
        (inner.x, inner.y),
        (inner.right(), inner.y),
        (inner.right(), inner.bottom()),
        (inner.x, inner.bottom()),
    ];
    if !to_center {
        return [0, 1, 2, 3].map(|side| {
            let (start, end) = (side, (side + 1) % 4);
            vec![
                outer_corners[start],
                outer_corners[end],
                inner_corners[end],
                inner_corners[start],
            ]
        });
    }
    let center = (
        f32::midpoint(inner.x, inner.right()),
        f32::midpoint(inner.y, inner.bottom()),
    );
    // The point where `side` ends at `corner` (`side` or `side + 1`): the
    // inner corner, or, if the other side at that corner is not painted,
    // the point of the outer edge level with the center.
    let corner_end = |side: usize, corner: usize| {
        let other = if corner == side {
            (side + 3) % 4
        } else {
            (side + 1) % 4
        };
        if painted[other] {
            inner_corners[corner]
        } else if side.is_multiple_of(2) {
            // Top and bottom sides extend along the left/right edge.
            (outer_corners[corner].0, center.1)
        } else {
            // Left and right sides extend along the top/bottom edge.
            (center.0, outer_corners[corner].1)
        }
    };
    [0, 1, 2, 3].map(|side| {
        let (start, end) = (side, (side + 1) % 4);
        vec![
            outer_corners[start],
            outer_corners[end],
            corner_end(side, end),
            center,
            corner_end(side, start),
        ]
    })
}

/// Appends a closed polygon to a path.
fn push_polygon(pb: &mut PathBuilder, points: &[(f32, f32)]) {
    let Some((first, rest)) = points.split_first() else {
        return;
    };
    pb.move_to(first.0, first.1);
    for p in rest {
        pb.line_to(p.0, p.1);
    }
    pb.close();
}

impl Rasterizer<'_> {
    pub(super) fn border(
        &mut self,
        rect: Rect,
        widths: [f32; 4],
        colors: [Rgba; 4],
        styles: [BorderStyle; 4],
        radii: &Radii,
    ) {
        let s = self.params.scale;
        let geometry = BorderGeometry::new(
            self.to_device(rect),
            widths.map(|w| w.max(0.0) * s),
            scale_radii(radii, s),
        );
        let has_width = |i: usize| widths[i] > 0.0 && !styles[i].has_no_width();
        let first = (0..4).find(|&i| has_width(i));
        let Some(first) = first else {
            return;
        };
        let uniform = (0..4)
            .filter(|&i| has_width(i))
            .all(|i| colors[i] == colors[first] && styles[i] == styles[first]);
        if uniform && matches!(styles[first], BorderStyle::Solid | BorderStyle::Double) {
            self.uniform_border(&geometry, colors[first], styles[first]);
        } else {
            self.border_sides(&geometry, colors, styles);
        }
    }

    /// A border whose sides have one color and a solid or double style:
    /// one ring, or two rings for `double`.
    fn uniform_border(&mut self, geometry: &BorderGeometry, color: Rgba, style: BorderStyle) {
        if color.is_transparent() {
            return;
        }
        // Like browsers, draw `double` as solid if the lines would be
        // thinner than one pixel.
        let thinnest = geometry
            .widths
            .iter()
            .copied()
            .filter(|w| *w > 0.0)
            .fold(f32::INFINITY, f32::min);
        let bands: &[(f32, f32)] = if style == BorderStyle::Double && thinnest >= 3.0 {
            &[(0.0, 1.0 / 3.0), (2.0 / 3.0, 1.0)]
        } else {
            &[(0.0, 1.0)]
        };
        let paint = solid_paint(color, true);
        for &(from, to) in bands {
            let Some(path) = geometry.ring(from, to) else {
                continue;
            };
            self.draw(geometry.outer, |p, t, m| {
                p.fill_path(&path, &paint, FillRule::EvenOdd, t, m);
            });
        }
    }

    /// A border with different sides: each side is painted separately, with
    /// the 3D shading of `inset`, `outset`, `groove` and `ridge`. `double`
    /// is painted as solid here.
    fn border_sides(
        &mut self,
        geometry: &BorderGeometry,
        colors: [Rgba; 4],
        styles: [BorderStyle; 4],
    ) {
        // Whole sides, the outer halves of grooves and ridges, and their
        // inner halves.
        let mut whole = [None; 4];
        let mut outer_half = [None; 4];
        let mut inner_half = [None; 4];
        for side in 0..4 {
            let (color, style) = (colors[side], styles[side]);
            if geometry.widths[side] <= 0.0 || color.is_transparent() || style.has_no_width() {
                continue;
            }
            match style {
                BorderStyle::Dashed | BorderStyle::Dotted => {
                    self.dashed_side(geometry, side, color, style);
                }
                BorderStyle::Groove => {
                    outer_half[side] = Some(shade(color, BorderStyle::Inset, side));
                    inner_half[side] = Some(shade(color, BorderStyle::Outset, side));
                }
                BorderStyle::Ridge => {
                    outer_half[side] = Some(shade(color, BorderStyle::Outset, side));
                    inner_half[side] = Some(shade(color, BorderStyle::Inset, side));
                }
                _ => whole[side] = Some(shade(color, style, side)),
            }
        }
        self.fill_sides(geometry, (0.0, 1.0), whole);
        self.fill_sides(geometry, (0.0, 0.5), outer_half);
        self.fill_sides(geometry, (0.5, 1.0), inner_half);
    }

    /// Fills the band `from..to` (fractions of the border width, 0 is the
    /// outer edge) of the given sides (see [`side_polygons`]). With rounded
    /// corners, the sides are drawn into a temporary layer and cut to the
    /// rounded ring.
    fn fill_sides(
        &mut self,
        geometry: &BorderGeometry,
        band: (f32, f32),
        colors: [Option<Rgba>; 4],
    ) {
        if colors.iter().all(Option::is_none) {
            return;
        }
        let (outer, _) = geometry.edge(band.0);
        let (inner, _) = geometry.edge(band.1);
        let painted = colors.map(|c| c.is_some());
        let polygons = side_polygons(outer, inner, geometry.has_radius, painted);
        // Sides of the same color are filled as one path, so that there is
        // no anti-aliasing seam on the diagonal between them.
        let mut by_color: Vec<(Rgba, PathBuilder)> = Vec::new();
        for (polygon, color) in polygons.iter().zip(colors) {
            let Some(color) = color else {
                continue;
            };
            let index = by_color
                .iter()
                .position(|(c, _)| *c == color)
                .unwrap_or_else(|| {
                    by_color.push((color, PathBuilder::new()));
                    by_color.len() - 1
                });
            push_polygon(&mut by_color[index].1, polygon);
        }
        let sides: Vec<(Path, Paint<'static>)> = by_color
            .into_iter()
            .filter_map(|(color, pb)| Some((pb.finish()?, solid_paint(color, true))))
            .collect();
        if !geometry.has_radius {
            for (path, paint) in &sides {
                self.draw(outer, |p, t, m| {
                    p.fill_path(path, paint, FillRule::Winding, t, m);
                });
            }
            return;
        }
        let Some(ring) = geometry.ring(band.0, band.1) else {
            return;
        };
        let Some(area) = self
            .visible_area()
            .and_then(|v| snap_out(outer).intersection(&v))
        else {
            return;
        };
        let (Some(mut layer), Some(mut ring_mask)) = (
            Pixmap::new(area.width as u32, area.height as u32),
            Mask::new(area.width as u32, area.height as u32),
        ) else {
            return;
        };
        let to_layer = Transform::from_translate(-area.x, -area.y);
        for (path, paint) in &sides {
            layer.fill_path(path, paint, FillRule::Winding, to_layer, None);
        }
        ring_mask.fill_path(&ring, FillRule::EvenOdd, true, to_layer);
        layer.apply_mask(&ring_mask);
        self.draw(area, |p, t, m| {
            p.draw_pixmap(
                area.x as i32,
                area.y as i32,
                layer.as_ref(),
                &PixmapPaint::default(),
                t,
                m,
            );
        });
    }

    /// A dashed or dotted side: a stroke along the middle of the side.
    /// Rounded corners are not followed.
    fn dashed_side(
        &mut self,
        geometry: &BorderGeometry,
        side: usize,
        color: Rgba,
        style: BorderStyle,
    ) {
        let rect = geometry.outer;
        let width = geometry.widths[side];
        let half = width / 2.0;
        let (x0, y0, x1, y1) = match side {
            0 => (rect.x, rect.y + half, rect.right(), rect.y + half),
            1 => (
                rect.right() - half,
                rect.y,
                rect.right() - half,
                rect.bottom(),
            ),
            2 => (
                rect.right(),
                rect.bottom() - half,
                rect.x,
                rect.bottom() - half,
            ),
            _ => (rect.x + half, rect.bottom(), rect.x + half, rect.y),
        };
        let mut pb = PathBuilder::new();
        pb.move_to(x0, y0);
        pb.line_to(x1, y1);
        let Some(path) = pb.finish() else {
            return;
        };
        let dash = if style == BorderStyle::Dotted {
            StrokeDash::new(vec![width, width], 0.0)
        } else {
            StrokeDash::new(vec![width * 3.0, width * 3.0], 0.0)
        };
        let stroke = Stroke {
            width,
            dash,
            ..Stroke::default()
        };
        let paint = solid_paint(color, true);
        self.draw(rect, |p, t, m| p.stroke_path(&path, &paint, &stroke, t, m));
    }
}
