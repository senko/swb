# ADR 0024: Rounded overflow clips and image reduction

- Status: accepted
- Date: 2026-10-09

## Context

M4 item 9 (Ars Technica) added two paint features:

1. A box with `overflow: hidden` (or another clipping value) and
   `border-radius` clips its content to the rounded padding edge, with
   radius max(0, r - border) (CSS Backgrounds 3 §4.2). Before, swb clipped
   to the padding rectangle.
2. A raster image drawn at a scale below 0.5 is averaged down before the
   bilinear draw. Bilinear sampling of the original skips most source
   pixels, so thin lines drop out (the Ars thumbnails showed a coarse grid).

Review found two cost and robustness problems:

- The reduction ran at every raster for every draw, without a cache or a
  budget. 200 `<img>` of 4000 x 3000 drawn at 50 x 50 took 4.1 s per
  frame (22 ms each); 2,000 at 20 x 20 took 41 s.
- A rounded overflow clip is a layer group. When the layer budget ran
  out, the group drew nothing: 150 nested
  `overflow:hidden;border-radius:12px` divs around a 3,000 px green block
  showed white. With `border-radius:0` they showed green, as Chromium
  does for both.

## Decision

Rounded overflow clip:

- The display list emits `PushSvgClip` / `PopSvgClip` with a
  rounded-rectangle `ClipPath` and `rect_fallback: true`. The rasterizer
  reuses the SVG clip layer (`raster/svg_clip.rs`): the layer counts
  against the layer budget, the coverage against the path work budget
  (the cost model of `paint/src/path_cost/`).
- If the layer does not fit, the group draws directly with a rectangle
  clip (`Layer::rect_clip`). If the coverage does not fit, the layer is
  drawn without the coverage; it is already limited to the clip's bounds.
  In both cases the content shows and the corners are square. A clip
  whose coverage is empty still hides the group. Other SVG clip groups
  keep hiding their content when they are over budget (ADR 0023).

Image reduction (`paint/src/reduce.rs`):

- Each axis whose scale is below 0.5 is halved by 2x2 averaging
  (premultiplied) until the scale is at least 0.5; the draw is bilinear
  from that level.
- The levels belong to the decoded image and are made lazily, one halving
  at a time, keyed by the number of halvings per axis. A repaint (scroll)
  reuses them. An image keeps levels up to the byte size of its own pixels
  (so at most double); levels beyond that are used but not kept.
- One frame (one `rasterize` call) averages at most 64 Mpx of source
  pixels over all images. When a halving does not fit, the draw uses the
  nearest level that exists (at worst the original, as before the
  reduction), and the frame logs one warning ("work budget for reduced
  copies ran out"). A later repaint continues from the kept levels.
- Images over 64 Mpx are not reduced (unchanged).

## Consequences

Measurements (release build, 1280 x 800):

| Page | Before | After |
|------|--------|-------|
| 2,000 `<img width=20 height=20>` of one 4000 x 3000 PNG | 41 s (hostile limit 5 s) | 0.10 s |
| 2 PNGs of 8000 x 8000 at 20 x 20 | n/a | 0.35 s, one warning |
| 150 nested rounded `overflow:hidden`, `--full-page` | white | green, corners square at the budget |
| 2,000 rounded cards 300 x 300, 3 deep | about 0.1 s | unchanged |

- The first frame that draws a large image small costs about 1.4 ns per
  source pixel (22 ms for 4000 x 3000), once per image.
- Memory of an image grows by up to its own size when drawn small.
- A frame that runs out of budget draws some images coarser (aliased)
  until a later repaint; the engine does not schedule that repaint by
  itself.
- The hostile set has the cases `images-many-reduced`,
  `images-reduce-budget`, `rounded-clips-nested` and `rounded-cards`.
- The box filter is a mip level; Chromium's exact filter may differ in
  some edge pixels.
