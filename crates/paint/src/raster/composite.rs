//! The composite of a group's layer onto its surface with source-over and
//! opacity 1 (CSS Compositing and Blending 1 §5.1, "source over"), the common
//! way that a layer ends.
//!
//! A pixel with alpha 255 replaces the surface's pixel, and a pixel with
//! alpha 0 leaves it: both are exact for premultiplied source-over. Only
//! the pixels between are blended, by tiny-skia, so the result equals a
//! draw of the whole layer pixel by pixel. A layer that is mostly opaque or
//! empty (the isolating layer of a stacking context with a blended box,
//! the tiles of a map) then costs one pass over its bytes instead of a
//! blend pipeline run over every pixel.

use tiny_skia::{Pixmap, PixmapMut, PixmapPaint, PixmapRef, Transform};

/// The most runs of translucent pixels that [`source_over`] draws one by
/// one, per layer row: a layer with more draws as a whole.
const MAX_RUNS_PER_ROW: usize = 4;

/// A run of translucent pixels in a row of a layer.
#[derive(Clone, Copy, Debug)]
struct Run {
    row: usize,
    first: usize,
    len: usize,
}

/// Draws `src` over `dst` with its top-left pixel at `at`, with source-over
/// and opacity 1, and no mask. Returns false, with `dst` changed only
/// where `src` is opaque (the same pixels that a full draw gives), if the
/// layer lies outside `dst`, has too many translucent runs, or more than a
/// quarter of its pixels are translucent: the caller then draws the whole
/// layer. (Drawn run by run, translucent pixels cost more than in one draw
/// of the layer: a 1280 x 800 layer at alpha 128 took 5.2 ms by runs and
/// 3.4 ms as a whole.)
pub(super) fn source_over(dst: &mut PixmapMut<'_>, at: (i32, i32), src: &Pixmap) -> bool {
    let (sw, sh) = (src.width() as usize, src.height() as usize);
    let dw = dst.width() as usize;
    let (Ok(x), Ok(y)) = (usize::try_from(at.0), usize::try_from(at.1)) else {
        return false;
    };
    if x + sw > dw || y + sh > dst.height() as usize {
        return false;
    }
    let mut runs = Vec::new();
    let max_runs = MAX_RUNS_PER_ROW * sh + 16;
    let max_translucent = sw * sh / 4;
    let mut translucent = 0;
    let src_data = src.data();
    let dst_data = dst.data_mut();
    for row in 0..sh {
        let at_row = ((y + row) * dw + x) * 4;
        let (Some(line), Some(out)) = (
            src_data.get(row * sw * 4..(row + 1) * sw * 4),
            dst_data.get_mut(at_row..at_row + sw * 4),
        ) else {
            return false;
        };
        translucent += copy_opaque_row(line, out, row, &mut runs);
        if runs.len() > max_runs || translucent > max_translucent {
            return false;
        }
    }
    for run in runs {
        draw_run(dst, (x, y), src_data, sw, run);
    }
    true
}

/// Copies the opaque pixels of row `row` of a layer (`line`) over `out`,
/// and appends its runs of translucent pixels to `runs`. Returns the number
/// of translucent pixels in the row.
fn copy_opaque_row(line: &[u8], out: &mut [u8], row: usize, runs: &mut Vec<Run>) -> usize {
    let (pixels, _) = line.as_chunks::<4>();
    let (out_pixels, _) = out.as_chunks_mut::<4>();
    let mut start = None;
    let mut count = 0;
    for (i, (s, d)) in pixels.iter().zip(out_pixels.iter_mut()).enumerate() {
        let partial = match s[3] {
            255 => {
                *d = *s;
                false
            }
            0 => false,
            _ => true,
        };
        count += usize::from(partial);
        match (start, partial) {
            (None, true) => start = Some(i),
            (Some(first), false) => {
                runs.push(Run {
                    row,
                    first,
                    len: i - first,
                });
                start = None;
            }
            _ => {}
        }
    }
    if let Some(first) = start {
        runs.push(Run {
            row,
            first,
            len: pixels.len() - first,
        });
    }
    count
}

/// Draws one run of translucent pixels of a layer whose rows are `width`
/// pixels long (`src`) at its place over `dst` (the layer at `at`).
fn draw_run(dst: &mut PixmapMut<'_>, at: (usize, usize), src: &[u8], width: usize, run: Run) {
    let offset = (run.row * width + run.first) * 4;
    let Some(bytes) = src.get(offset..offset + run.len * 4) else {
        return;
    };
    let Some(part) = PixmapRef::from_bytes(bytes, run.len as u32, 1) else {
        return;
    };
    dst.draw_pixmap(
        (at.0 + run.first) as i32,
        (at.1 + run.row) as i32,
        part,
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
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
        for (seed, translucent) in [(1, 0), (2, 6), (3, 40), (4, 255)] {
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
    fn a_layer_with_many_runs_falls_back_to_a_full_draw() {
        // Every other pixel translucent: more runs than the cap.
        let mut src = noise(40, 8, 7, 0);
        for (i, px) in src.data_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
            if i % 2 == 0 {
                px.copy_from_slice(&[40, 40, 40, 128]);
            }
        }
        let backdrop = noise(40, 8, 9, 0);
        let mut got = backdrop.clone();
        assert!(!source_over(&mut got.as_mut(), (0, 0), &src));
        // The pixels that it changed are those that a full draw sets alike,
        // so the full draw after it gives the same result.
        reference(&mut got, (0, 0), &src);
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
