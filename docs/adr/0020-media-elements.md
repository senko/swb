# ADR 0020: Media elements

- Status: accepted
- Date: 2026-10-07

## Context

Target 3 (Wikipedia "Web browser") has a video thumbnail:
`<video poster="//thumb.wikimedia.org/...jpg" controls preload="none"
width="250" height="159">` with five `<source>` children, in a
`figure` that is a table with a caption. swb laid `<video>` out as a
non-replaced inline box: the figure showed only its caption, the
`<source>` elements had boxes that Chromium does not have, and the page
below the figure was about 76 px too high.

The specifications are HTML §4.8.8–4.8.11 (media elements,
<https://html.spec.whatwg.org/multipage/media.html>) and the rendering
section (<https://html.spec.whatwg.org/multipage/rendering.html#video>).
Chromium 148 is the reference where they leave room. swb runs no
scripts, and the Chromium references run with JavaScript disabled.

## Decision

### Scope

- `<video>` and `<audio>` are replaced elements. Their children
  (`<source>`, `<track>`, fallback content) are not rendered.
- swb plays no media and never fetches the media resource, also when
  `preload` asks for it. A video shows its poster image.
- `<canvas>` stays a non-replaced element: with scripting disabled it
  represents its fallback content (HTML), as in Chromium with JavaScript
  disabled.

### Sizes

- The natural size of a video is its poster's, when the poster is
  loaded. Otherwise a video, and audio, have no natural size, so the
  default object size (300×150) applies without an aspect ratio: Chromium
  gives `<video width=100>` a size of 100×150, not 100×50.
- `width` and `height` are presentational hints (as before), and on
  `<video>` they also map to `aspect-ratio: auto w / h` when both are
  lengths (HTML "map to the aspect-ratio property (using dimension
  rules)"; Chromium's computed value is `auto 250 / 159`). This needed
  `aspect-ratio` to keep `auto`: the computed value is now
  `AspectRatio { auto, ratio }`, and `auto && <ratio>` prefers the
  natural ratio of a replaced element (CSS Sizing 4 §7.1). So the
  Wikipedia video, which has `height: auto` from the site's style sheet,
  is 250×159 with or without its poster.
- `<audio controls>` is 300×54 from the user-agent style sheet;
  `<audio>` without `controls` is `display: none` there.
- Everything else (min/max sizes, flex and grid items, absolutely
  positioned boxes, baselines) is the replaced-element code of `<img>`.
  The layout tests `video-sizing`, `video-aspect-ratio` and
  `media-children` check the sizes against Chromium.

### Poster

The engine loads the poster like an `<img>` source (same URL resolution,
`Destination::Image`, the same rule for `file:` URLs); raster and SVG
posters work. Paint places it with `object-fit` (`contain` from the
user-agent style sheet) and `object-position`, clipped to the content
box. `object-position` is a new longhand; `<img>` now uses both
properties too (they were parsed but ignored before). Placement uses the
default sizing algorithm of backgrounds (`background.rs`), which
matches Chromium for all five `object-fit` values (unit test
`object_fit_and_position_follow_chromium`).

### Controls

HTML exposes a user interface "if the `controls` attribute is present,
or if scripting is disabled for the media element". So every video in
swb has controls, as in Chromium with JavaScript disabled.

Chromium's controls are a shadow tree with style sheets and SVG icons.
swb does not build that tree; it draws an approximation of what Chromium
shows before playback, measured at device scale 1 from screenshots of
videos with plain posters (no Chromium code was read):

- a black gradient over the bottom 112 px of the video (alpha stops
  every 10 px);
- 48×48 px buttons centered 48 px above the bottom edge: play at the
  left, the current time ("0:00", 14 px sans-serif) after it, and from
  the right edge the overflow menu, fullscreen and mute;
- a 4 px timeline in white at 30 % from 16 px to the width − 16 px,
  with its bottom edge 20 px above the bottom of the video;
- icons in a 20×20 px box centered in their button, white, or white at
  30 % when the control is not available: play and the menu are
  available if the element has a media resource (`src` or a `<source>`
  with `src`); mute and fullscreen never are, because the metadata never
  loads;
- parts are left out at the widths and heights where Chromium leaves
  them out (play below 122 px, mute below 170, fullscreen below 198, the
  time below 246; below 72 px of height the row is centered 24 px above
  the bottom and the timeline disappears; from 24 to 48 px only the
  timeline shows; below 24 px only the gradient);
- a video without a `poster` attribute is filled with #333 (Chromium's
  default poster); a poster that fails to load shows the background.

Audio controls are a #f1f3f4 panel with round ends: play, "0:00 /
0:00", the timeline, mute and the menu, positioned as measured at the
default size. Unlike the video menu, Chromium draws the audio menu as
available also without a media resource. At other sizes swb uses its
own rules (the time shows if 32 px of timeline remain after it).

Layout decides which parts show and where (`layout/src/media.rs`,
`BoxContent::Media`) and shapes the time text, because it has the fonts;
paint draws the parts (`paint/src/media.rs`). Paint gained one display
item, `Polygon` (a filled polygon), for the icons. In a screenshot of a
250×159 video, the controls differ from Chromium's only in glyph
anti-aliasing.

### Alternatives

- Draw no controls and document the difference: the Wikipedia thumbnail
  is in the first viewport, and its controls cover about 15,000 pixels
  that the pixel score compares.
- Build the controls as generated boxes (like form controls): more
  layout code for a fixed arrangement of icons, and the box tree would
  contain boxes that have no element.

## Consequences

- The Wikipedia figure has Chromium's size; the five `<source>` boxes
  are gone (`extra 5` → 0).
- Not done: playback, the overlay play button and the loading spinner
  that Chromium shows in some states, the video's first frame when
  `preload` would load it (swb shows the poster or the default poster),
  focus and hover states of the controls, interaction (a click does
  nothing), right-to-left controls, controls at device scales other than
  1 (the geometry scales; Chromium may change the layout), the
  `aspect-ratio` hint on `<img>` and image buttons.
