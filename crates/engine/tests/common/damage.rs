//! The oracle of the damage of frames (ADR 0044 section 18): a pixmap kept
//! between frames and updated with `Page::render_update` (only the damage)
//! must equal a full render of the same frame bit for bit. A difference is a
//! bug of the display list diff, of an input of the render that gives no
//! damage, or of the region raster.

use swb_engine::{FrameDamage, Page, Pixmap};

/// A kept pixmap and the counts of the frames.
#[derive(Default)]
pub(crate) struct DamageOracle {
    kept: Option<Pixmap>,
    /// Frames drawn with damage rectangles.
    pub(crate) region_frames: usize,
    /// Frames rasterized in full.
    pub(crate) full_frames: usize,
    /// Frames without damage.
    pub(crate) empty_frames: usize,
    /// The device pixels of the region frames.
    pub(crate) region_pixels: u64,
    /// The damage of the last frame.
    pub(crate) last: Option<FrameDamage>,
}

impl DamageOracle {
    /// Renders a frame into the kept pixmap (in full when the pixmap is new
    /// or the viewport changed size, as the GUI's composer does), renders
    /// the same frame in full, and compares. Returns the full render.
    pub(crate) fn step(&mut self, page: &mut Page, label: &str) -> Pixmap {
        let (width, height) =
            swb_engine::device_size(page.viewport(), page.scale()).expect("a viewport size");
        let fits = self
            .kept
            .as_ref()
            .is_some_and(|k| k.width() == width && k.height() == height);
        let kept = if fits {
            self.kept.as_mut().expect("the kept pixmap")
        } else {
            let mut pixmap = Pixmap::new(width, height).expect("a viewport pixmap");
            page.render(&mut pixmap);
            self.kept.insert(pixmap)
        };
        let damage = page.render_update(kept);
        if damage.full {
            self.full_frames += 1;
        } else if damage.rects.is_empty() {
            self.empty_frames += 1;
        } else {
            self.region_frames += 1;
            self.region_pixels += damage.pixels();
        }
        let full = page.screenshot(false).expect("a viewport screenshot");
        if let Some((x, y, want, got)) = first_difference(&full, kept) {
            panic!(
                "{label}: pixel ({x}, {y}) is {got:?} after the damage {damage:?}, a full \
                 render gives {want:?}"
            );
        }
        self.last = Some(damage);
        full
    }

    /// A line with the counts.
    pub(crate) fn summary(&self) -> String {
        format!(
            "{} region frames ({} px), {} full, {} without damage",
            self.region_frames, self.region_pixels, self.full_frames, self.empty_frames
        )
    }
}

/// The first pixel where `a` and `b` differ, with both values.
pub(crate) fn first_difference(a: &Pixmap, b: &Pixmap) -> Option<(u32, u32, [u8; 4], [u8; 4])> {
    let width = a.width();
    if a.width() != b.width() || a.height() != b.height() {
        return Some((0, 0, [0; 4], [0; 4]));
    }
    a.data()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.data().as_chunks::<4>().0)
        .enumerate()
        .find(|(_, (p, q))| p != q)
        .map(|(i, (p, q))| {
            let i = i as u32;
            (i % width, i / width, *p, *q)
        })
}
