# ADR 0043: Blend modes, rounded boxes and resize handles

- Status: accepted (2026-11-19)
- Date: 2026-11-19

## Context

The exit test of the OpenStreetMap target (M12 feature 4) compared swb with
Chromium at the pixel level and found paint and layout differences that the
earlier pages did not show. This ADR records the rules that parts 4c and 4d added.
Every rule comes from black-box measurements in Chromium 148 (probe files
named below).

## Decisions

- **`mix-blend-mode`** (CSS Compositing and Blending 1 §3.2; 17 values,
  `plus-lighter` is the `Plus` operator). The element and its descendants
  are a stacking context and paint into a group (`PushOpacity` with a
  `blend` field, `LayerKind::Blend` in the rasterizer); the group is
  composited over the surface below with tiny-skia's blend mode. The
  property makes a backdrop root (ADR 0033).
- **Blend and transform.** The transform of a box that is not a translation
  is drawn into a transparent layer, so the blend group encloses the
  transform groups of its box (`own-rotate`, `own-scale-in-rounded-clip`).
  **Blend and `backdrop-filter` on one box**: the filtered backdrop is
  drawn first, then the box, then the blend, as with `opacity` below 1
  (`own-backdrop-filter`, `own-backdrop-filter-brightness`); the filtered
  backdrop of an isolating context is outside its layer.
- **SVG elements**: an element of an inline `<svg>` with `mix-blend-mode`
  is a layer that blends (`in-svg`, `in-svg-group`, `in-svg-opacity`), and
  its `<svg>` box counts as a blended box in its stacking context
  (`in-svg-in-z-index`). It uses one of the layers of the SVG budget.
- **A stacking context is an isolated group.** A box that forms a stacking
  context (`forms_stacking_context`) and has a `mix-blend-mode` box in its
  own context paints into a layer too (`blend_contexts` in the display list
  builder: a `PushOpacity` of opacity 1), because the blend sees only what
  the context painted before the box (`parent-opacity-isolates`,
  `parent-filter-isolates`, `parent-z-index-isolates`,
  `parent-transform-isolates` and `parent-backdrop-filter-isolates` of
  `tools/probes/mix-blend-mode.json`). The document of a frame is an
  isolated group too: a frame whose display list has a blend group is drawn
  into a layer (`in-iframe`).
- **A rounded `overflow` clip does not isolate.** swb draws it as a layer
  (ADR 0024), Chromium as a clip. A blend group inside such layers blends
  with the backdrop root's pixels with the clip layers drawn over them, and
  the result replaces the clip layer's pixels under the group
  (`blend_through_clip_layers`; the "(i)" icons of the layer list are such
  boxes). Departure: with `isolation` (not a property in swb) a context
  cannot be isolated on request.
- **Rounded boxes are snapped to device pixels**, as boxes without radii
  were: the border box of a rounded background and border is rounded to
  whole pixels, and only the corners are anti-aliased
  (`border-radius-sides.json`, `frac-top-rounded*`).
- **The join of two border sides** is the line through the outer and the
  inner corner, also beyond the inner corner of a rounded box (CSS
  Backgrounds 3 §5.2: its angle follows the ratio of the two widths), and a
  side with a width and a transparent color keeps its part of the corner
  (Bootstrap's `.spinner-border` is a "C"). A side with a width of 0 gives
  its corner to the other side, as before. `dashed` and `dotted` sides
  stay strokes along the straight side; `double` on a border with unequal
  sides is solid (`border-radius-styles.json`).
- **A box with rounded and square corners paints its sides one by one**
  (each side anti-aliased on the diagonal: a square corner pixel is covered
  0.79 in Chromium, 135 over 222 with the border at 112, and 0.75 in swb,
  140), and **a rounded box with four solid opaque borders keeps its
  background color inside the border** (Chromium shrinks the background so
  that its edge does not bleed): the padding box with the inner radii
  (`border-radius-pagination.json`).
- **The resize handle**: a text area, and a scroll container with
  `overflow` other than `visible` and `clip`, with `resize` other than
  `none` (UA: `textarea { resize: both }`) paints two strokes in the
  corner of its padding box (the bottom left with `direction: rtl`): black
  with alpha 0.6 and a white one with alpha 0.6 a row lower, 7 and 3 pixels
  long, at the pixels that Chromium draws (`textarea-resizer.json`). swb
  does not resize the box.
- **Floats with negative margins**: each float adds its margin box, but at
  least 0, to the line in the min-content and max-content widths
  (`float-negative-margin.json`). A `.form-check` in a flex item with
  `flex: 0 0 auto` had its label wrap.

## Consequences

- Departures that remain: `isolation`; a box with a blend, a transform
  that is not a translation and a `backdrop-filter` draws the backdrop
  inside its blend group, so the blend applies to the filtered backdrop too; a
  `backdrop-filter` in a frame document with a blend group reads the frame's
  layer, not the embedding page; an SVG element past the layer budget of
  its document does not blend; `dashed`, `dotted` and `double`
  borders on rounded boxes; the anti-aliasing of the rounded ring (up to 25
  levels at the curve pixels); the resize handle at a device scale factor
  other than 1 is made of whole CSS pixels; the glyph rasterization of icon
  fonts (the "+" of the zoom button differs by 13 levels).
- A blend group that does not fit into the layer budget draws directly,
  without the blend.
