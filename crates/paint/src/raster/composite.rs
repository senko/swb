//! The composite of a group's layer onto its surface with source-over and
//! opacity 1 (CSS Compositing and Blending 1 §5.1, "source over"), the common
//! way that a layer ends.
//!
//! A pixel with alpha 255 replaces the surface's pixel, and a pixel with
//! alpha 0 leaves it: both are exact for premultiplied source-over. Only
//! the pixels between are blended, by tiny-skia: gathered into one row with
//! the surface's pixels under them, blended in one draw, and put back (the
//! blend is the same for each pixel, so the result equals a draw of the
//! whole layer). A layer that is mostly opaque or empty (the isolating
//! layer of a stacking context with a blended box, the tiles of a map, a
//! clip's layer of text over a box) then costs one pass over its bytes
//! instead of a blend pipeline run over every pixel.

use tiny_skia::{Pixmap, PixmapMut, PixmapPaint, Transform};

/// Draws `src` over `dst` with its top-left pixel at `at`, with source-over
/// and opacity 1, and no mask. Returns false, with `dst` changed only
/// where `src` is opaque (the same pixels that a full draw gives), if the
/// layer lies outside `dst`, or more than half of its pixels (or more than
/// 1 Mi pixels) are translucent: the caller then draws the whole layer
/// (one blend of every pixel costs less then than gathering them).
pub(super) fn source_over(dst: &mut PixmapMut<'_>, at: (i32, i32), src: &Pixmap) -> bool {
    let (sw, sh) = (src.width() as usize, src.height() as usize);
    let dw = dst.width() as usize;
    let (Ok(x), Ok(y)) = (usize::try_from(at.0), usize::try_from(at.1)) else {
        return false;
    };
    if x + sw > dw || y + sh > dst.height() as usize {
        return false;
    }
    // At most half of the pixels, and at most 1 Mi of them, so the gathered
    // copy stays small for any layer.
    let max_translucent = (sw * sh / 2).min(1 << 20);
    // The translucent pixels: their index in `src` and in `dst`.
    let mut places: Vec<(usize, usize)> = Vec::new();
    let src_pixels = src.data().as_chunks::<4>().0;
    let dst_data = dst.data_mut();
    for row in 0..sh {
        let at_row = (y + row) * dw + x;
        let (Some(line), Some(out)) = (
            src_pixels.get(row * sw..(row + 1) * sw),
            dst_data.get_mut(at_row * 4..(at_row + sw) * 4),
        ) else {
            return false;
        };
        copy_opaque_row(line, out.as_chunks_mut::<4>().0, |column| {
            places.push((row * sw + column, at_row + column));
        });
        if places.len() > max_translucent {
            return false;
        }
    }
    blend_gathered(src_pixels, dst_data, &places);
    true
}

/// Copies the opaque pixels of a row of a layer (`line`) over `out`, and
/// calls `translucent` with the column of each translucent pixel.
fn copy_opaque_row(line: &[[u8; 4]], out: &mut [[u8; 4]], mut translucent: impl FnMut(usize)) {
    for (column, (s, d)) in line.iter().zip(out.iter_mut()).enumerate() {
        match s[3] {
            255 => *d = *s,
            0 => {}
            _ => translucent(column),
        }
    }
}

/// Blends the pixels of `src` at `places` (index in `src`, index in `dst`)
/// over those of `dst`, in one draw of a row of them.
fn blend_gathered(src: &[[u8; 4]], dst: &mut [u8], places: &[(usize, usize)]) {
    let Ok(count) = u32::try_from(places.len()) else {
        return;
    };
    let (Some(mut over), Some(mut under)) = (Pixmap::new(count, 1), Pixmap::new(count, 1)) else {
        return;
    };
    let dst = dst.as_chunks_mut::<4>().0;
    for ((o, u), &(from, to)) in over
        .data_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(under.data_mut().as_chunks_mut::<4>().0.iter_mut())
        .zip(places)
    {
        if let (Some(s), Some(d)) = (src.get(from), dst.get(to)) {
            *o = *s;
            *u = *d;
        }
    }
    under.draw_pixmap(
        0,
        0,
        over.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
    for (u, &(_, to)) in under.data().as_chunks::<4>().0.iter().zip(places) {
        if let Some(d) = dst.get_mut(to) {
            *d = *u;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic pixmap whose pixels are opaque, empty or translucent
    /// by `translucent_per_256` out of 256 (premultiplied).
    fn noise(width: u32, height: u32, seed: u32, translucent_per_256: u32) -> Pixmap {
        let mut pixmap = Pixmap::new(width, height).unwrap();
        let mut state = seed;
        let mut next = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            state >> 8
        };
        for px in pixmap.data_mut().as_chunks_mut::<4>().0.iter_mut() {
            let kind = next() % 256;
            let alpha = if kind < translucent_per_256 {
                1 + next() % 254
            } else if kind % 2 == 0 {
                255
            } else {
                0
            };
            for c in &mut px[..3] {
                *c = (next() % 256 * alpha / 255) as u8;
            }
            px[3] = alpha as u8;
        }
        pixmap
    }

    fn reference(dst: &mut Pixmap, at: (i32, i32), src: &Pixmap) {
        dst.draw_pixmap(
            at.0,
            at.1,
            src.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }

    #[test]
    fn rows_equal_a_full_draw() {
        for (seed, translucent) in [(1, 0), (2, 6), (3, 40), (4, 120), (5, 255)] {
            let src = noise(37, 23, seed, translucent);
            let backdrop = noise(60, 50, seed + 100, 0);
            let mut want = backdrop.clone();
            reference(&mut want, (9, 11), &src);
            let mut got = backdrop;
            let mut view = got.as_mut();
            if !source_over(&mut view, (9, 11), &src) {
                reference(&mut got, (9, 11), &src);
            }
            assert_eq!(got.data(), want.data(), "seed {seed}");
        }
    }

    #[test]
    fn a_layer_with_many_runs_is_blended_in_one_draw() {
        // Every other pixel translucent: text over a transparent layer.
        let mut src = noise(40, 8, 7, 0);
        for (i, px) in src.data_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
            if i % 2 == 0 {
                px.copy_from_slice(&[40, 40, 40, 128]);
            }
        }
        let backdrop = noise(40, 8, 9, 0);
        let mut got = backdrop.clone();
        assert!(source_over(&mut got.as_mut(), (0, 0), &src));
        let mut want = backdrop;
        reference(&mut want, (0, 0), &src);
        assert_eq!(got.data(), want.data());
    }

    #[test]
    fn a_mostly_translucent_layer_falls_back_to_a_full_draw() {
        // One run per row (under the run cap), but every pixel translucent.
        let mut src = Pixmap::new(40, 8).unwrap();
        src.fill(tiny_skia::Color::from_rgba8(80, 120, 160, 128));
        let backdrop = noise(40, 8, 11, 0);
        let mut got = backdrop.clone();
        assert!(!source_over(&mut got.as_mut(), (0, 0), &src));
        reference(&mut got, (0, 0), &src);
        let mut want = backdrop;
        reference(&mut want, (0, 0), &src);
        assert_eq!(got.data(), want.data());
    }

    #[test]
    fn a_layer_outside_the_surface_is_left_to_the_caller() {
        let src = noise(10, 10, 5, 0);
        let mut dst = Pixmap::new(12, 12).unwrap();
        assert!(!source_over(&mut dst.as_mut(), (5, 0), &src));
        assert!(!source_over(&mut dst.as_mut(), (-1, 0), &src));
        assert!(dst.data().iter().all(|&b| b == 0));
    }
}
