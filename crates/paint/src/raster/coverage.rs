//! The product of a layer and the coverage of its clip (`raster/svg_clip.rs`),
//! in one pass over the coverage: a pixel whose coverage is 255 keeps its
//! value, a pixel whose coverage is 0 becomes transparent, and only the
//! pixels between are multiplied by tiny-skia (`Pixmap::apply_mask`), all
//! together in one row. The multiplication is the same for each pixel, and
//! 255 and 0 give the pixel and 0 for every premultiplied value (the test
//! checks all of them), so the result equals `apply_mask` over the whole
//! layer. A rounded clip is 255 everywhere but at its edges, so the cost is
//! one pass over its bytes instead of a pipeline run over every pixel.

use tiny_skia::{IntRect, IntSize, Mask, Pixmap};

/// The most partial pixels gathered for one multiplication; more are
/// multiplied in batches of this size (plus at most one row), so the
/// gathered copy stays small for any layer.
const MAX_GATHERED: usize = 1 << 20;

/// Multiplies `pixmap` by the part `within` of `coverage` (one value per
/// pixel, rows of `width` values): the pixmap has the size of `within`, and
/// `within` lies inside the coverage (callers make both from one layer
/// plan; a mismatch is a bug, logged, and leaves the pixmap as it is).
pub(super) fn multiply(pixmap: &mut Pixmap, coverage: &[u8], width: u32, within: IntRect) {
    let (pw, ph) = (pixmap.width() as usize, pixmap.height() as usize);
    let (x0, y0, width) = (within.x() as usize, within.y() as usize, width as usize);
    let fits = within.width() as usize == pw
        && within.height() as usize == ph
        && x0 + pw <= width
        && (y0 + ph) * width <= coverage.len();
    if !fits {
        debug_assert!(fits, "a clip coverage that does not fit its layer");
        log::warn!("clip coverage: the coverage does not fit the layer; the clip is not applied");
        return;
    }
    // The pixels with partial coverage: their place in the pixmap (pixel
    // index), gathered with their coverage.
    let mut places: Vec<usize> = Vec::new();
    let mut values: Vec<u8> = Vec::new();
    for row in 0..ph {
        let from = (y0 + row) * width + x0;
        let data = pixmap.data_mut();
        let (Some(line), Some(out)) = (
            coverage.get(from..from + pw),
            data.get_mut(row * pw * 4..(row + 1) * pw * 4),
        ) else {
            return;
        };
        let (pixels, _) = out.as_chunks_mut::<4>();
        for (column, (&c, px)) in line.iter().zip(pixels.iter_mut()).enumerate() {
            match c {
                255 => {}
                0 => *px = [0; 4],
                _ => {
                    places.push(row * pw + column);
                    values.push(c);
                }
            }
        }
        if places.len() >= MAX_GATHERED {
            apply_gathered(pixmap.data_mut(), &places, std::mem::take(&mut values));
            places.clear();
        }
    }
    apply_gathered(pixmap.data_mut(), &places, values);
}

/// Multiplies the pixels of `data` at `places` (pixel index) by `values`,
/// in one `apply_mask` of a row of them.
fn apply_gathered(data: &mut [u8], places: &[usize], values: Vec<u8>) {
    let Some(size) = IntSize::from_wh(places.len() as u32, 1) else {
        return;
    };
    let (Some(mut partial), Some(mask)) =
        (Pixmap::new(size.width(), 1), Mask::from_vec(values, size))
    else {
        return;
    };
    let (gathered, _) = partial.data_mut().as_chunks_mut::<4>();
    let (pixels, _) = data.as_chunks::<4>();
    for (slot, &place) in gathered.iter_mut().zip(places) {
        if let Some(px) = pixels.get(place) {
            *slot = *px;
        }
    }
    partial.apply_mask(&mask);
    let (gathered, _) = partial.data().as_chunks::<4>();
    let (pixels, _) = data.as_chunks_mut::<4>();
    for (px, &place) in gathered.iter().zip(places) {
        if let Some(out) = pixels.get_mut(place) {
            *out = *px;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every premultiplied pixel value (each channel at most the alpha).
    fn all_pixels() -> Pixmap {
        let mut values = Vec::new();
        for a in 0..=255u8 {
            for c in 0..=a {
                values.push([c, a - c / 2, c / 3, a]);
            }
        }
        let mut pixmap = Pixmap::new(values.len() as u32, 1).unwrap();
        for (px, v) in pixmap
            .data_mut()
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(values)
        {
            *px = v;
        }
        pixmap
    }

    #[test]
    fn coverage_255_keeps_and_0_clears_every_pixel_in_tiny_skia() {
        let pixmap = all_pixels();
        let size = IntSize::from_wh(pixmap.width(), 1).unwrap();
        for (value, keeps) in [(255u8, true), (0, false)] {
            let mut product = pixmap.clone();
            let mask = Mask::from_vec(vec![value; pixmap.width() as usize], size).unwrap();
            product.apply_mask(&mask);
            if keeps {
                assert_eq!(product.data(), pixmap.data());
            } else {
                assert!(product.data().iter().all(|&b| b == 0));
            }
        }
    }

    #[test]
    fn the_product_equals_tiny_skias_over_the_whole_layer() {
        let (width, height) = (61u32, 37u32);
        let mut seed = 0x2545_f491_u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let mut pixmap = Pixmap::new(width, height).unwrap();
        for px in pixmap.data_mut().as_chunks_mut::<4>().0 {
            let a = (next() >> 24) as u8;
            let c = |v: u32| ((v >> 24) * u32::from(a) / 255) as u8;
            *px = [c(next()), c(next()), c(next()), a];
        }
        // Mostly 255 and 0, as a rounded clip, with partial values.
        let coverage: Vec<u8> = (0..width * height)
            .map(|_| match next() % 4 {
                0 => 0,
                1 => (next() >> 24) as u8,
                _ => 255,
            })
            .collect();
        let size = IntSize::from_wh(width, height).unwrap();
        let mut whole = pixmap.clone();
        whole.apply_mask(&Mask::from_vec(coverage.clone(), size).unwrap());
        let all = IntRect::from_xywh(0, 0, width, height).unwrap();
        let mut fast = pixmap.clone();
        multiply(&mut fast, &coverage, width, all);
        assert_eq!(fast.data(), whole.data());
        // A part of the layer with the same part of the coverage.
        let within = IntRect::from_xywh(7, 5, 30, 20).unwrap();
        let mut part = pixmap.clone_rect(within).unwrap();
        multiply(&mut part, &coverage, width, within);
        assert_eq!(part.data(), whole.clone_rect(within).unwrap().data());
    }
}
