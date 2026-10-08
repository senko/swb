# Third-party notices

swb's own source code is licensed under the MIT License (see
[LICENSE](LICENSE)). Some parts of swb's source code are derived from other
projects, and swb binaries contain third-party material. These keep their
own licenses. The MIT License of swb does not apply to them, and does not
limit the rights that their own licenses give you.

This file lists:

- material that is compiled into swb binaries and is not under a
  permissive software license: material under a copyleft license (the
  Public Suffix List and one file derived from Servo, both MPL-2.0)
  and material under CC BY 4.0 (from the WHATWG HTML Standard);
- parts of swb's source code that are derived from the source code of
  other projects: from Chromium and Skia (BSD-3-Clause; copies of swb's
  source code and binaries must include their license texts below) and
  from Servo (MPL-2.0);
- third-party content in the test fixtures (not in binaries).

It does not reproduce the notices of the permissive dependencies (MIT,
Apache-2.0, BSD and others; `cargo deny list` shows them). A binary
distribution must include those notices too, for example generated with
`cargo about`.

## Public Suffix List (MPL-2.0)

swb binaries contain the Public Suffix List, maintained by the Public
Suffix List project (https://publicsuffix.org/). The list is licensed under
the Mozilla Public License 2.0:

> This Source Code Form is subject to the terms of the Mozilla Public
> License, v. 2.0. If a copy of the MPL was not distributed with this
> file, You can obtain one at https://mozilla.org/MPL/2.0/.

- How it gets into the binary: the `psl` crate (https://crates.io/crates/psl)
  contains a table (`src/list.rs`) generated from the list. The crate's
  metadata says "MIT/Apache-2.0"; that applies to its hand-written code.
  The generated table is the list in another form, and swb treats it as
  MPL-2.0.
- The swb source repository does not contain the list. Cargo downloads the
  `psl` crate at build time; its version is in `Cargo.lock` of the swb
  source revision that the binary was built from.
- Source code form: the list is at
  https://github.com/publicsuffix/list (file `public_suffix_list.dat`,
  with its full history), and the current version at
  https://publicsuffix.org/list/public_suffix_list.dat. A binary contains
  the version that its `psl` crate was generated from. The crate also
  contains `data/rules.txt`, a copy of the rules without the comments of
  the list (so without the license header and without the section
  markers).
- swb does not modify the list. Your rights to the list come from
  MPL-2.0 (MPL-2.0 §3.2(b), §3.3).
- swb uses the list to find registrable domains for cookies
  (`crates/net/src/site.rs`).
- Why swb makes this exception: [ADR 0014](docs/adr/0014-public-suffix-list-license-exception.md).

## WHATWG HTML Standard (CC BY 4.0)

swb binaries contain three stylesheets that are based on the CSS rules in
section 15 "Rendering" of the HTML Living Standard
(https://html.spec.whatwg.org/multipage/rendering.html), with changes (the
headers of the files list them):

- `crates/style/src/ua.css` (the user-agent stylesheet)
- `crates/style/src/ua-quirks.css` (quirks mode rules)
- `crates/style/src/hints.css` (presentational hints)

Copyright WHATWG (Apple, Google, Mozilla, Microsoft). Licensed under the
Creative Commons Attribution 4.0 International License:
https://creativecommons.org/licenses/by/4.0/. The changes are swb's.

## Chromium (BSD-3-Clause)

Parts of the following items of swb's source code are derived from the
source code of Chromium (https://chromium.googlesource.com/chromium/src),
Copyright The Chromium Authors, licensed under the BSD-3-Clause license
below. Each item has a comment that names its Chromium source. The project
owner decided on 2026-10-07 to keep this code with this attribution; a
clean-room rewrite may follow.

- Table layout: `crates/layout/src/table/columns.rs`, `distribute.rs`,
  `rows.rs`, `cells.rs`, and parts of `layout.rs`. From
  `third_party/blink/renderer/core/layout/table/`:
  `table_layout_utils.cc`, `table_layout_algorithm_types.cc`,
  `table_layout_algorithm_types.h`, `table_layout_algorithm.cc`.
- Grid track sizing: `crates/layout/src/grid/sizing.rs`, and
  `auto_repetitions` in `crates/layout/src/grid/mod.rs`. From
  `third_party/blink/renderer/core/layout/grid/`:
  `grid_track_sizing_algorithm.cc`, `grid_track_sizing_algorithm.h`,
  `grid_layout_utils.cc`.
- Sticky offsets: `StickyConstraints` (its offsets and offset ranges) and
  `sticky_offset` in `crates/layout/src/positioned.rs`. From
  `third_party/blink/renderer/core/page/scrolling/sticky_position_scrolling_constraints.cc`.
- Boxes that establish a block formatting context next to floats:
  `try_opportunity` in `crates/layout/src/block.rs`. From
  `third_party/blink/renderer/core/layout/block_layout_algorithm.cc`
  (`HandleNewFormattingContext`).
- Content sizes of block children with floats: `blocks_content_sizes` in
  `crates/layout/src/intrinsic.rs`. From
  `third_party/blink/renderer/core/layout/block_layout_algorithm.cc`
  (`ComputeMinMaxSizes`).
- The check mark of checked checkboxes in `crates/paint/src/control.rs`.
  From `ui/native_theme/native_theme_base.cc` (`PaintCheckbox`).

Chromium's license
(https://chromium.googlesource.com/chromium/src/+/main/LICENSE):

```text
Copyright 2015 The Chromium Authors

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

   * Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.
   * Redistributions in binary form must reproduce the above
copyright notice, this list of conditions and the following disclaimer
in the documentation and/or other materials provided with the
distribution.
   * Neither the name of Google LLC nor the names of its
contributors may be used to endorse or promote products derived from
this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

## Skia (BSD-3-Clause)

The following data in swb's source code is derived from the source code
of Skia (https://skia.org/), Copyright Google Inc., licensed under the
BSD-3-Clause license below. Each item has a comment that names its Skia
source.

- The classes of metric-compatible font families (`METRIC_COMPATIBLE` in
  `crates/text/src/source/fontconfig.rs`). From
  `SkFontConfigInterface_direct.cpp` (`GetFontEquivClass`).
- The stroke widths of synthetic bold (`fake_bold_scale` in
  `crates/text/src/raster.rs`). From `SkTextFormatParams.h`.

Skia's license (https://skia.googlesource.com/skia/+/main/LICENSE):

```text
Copyright (c) 2011 Google Inc. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

  * Redistributions of source code must retain the above copyright
    notice, this list of conditions and the following disclaimer.

  * Redistributions in binary form must reproduce the above copyright
    notice, this list of conditions and the following disclaimer in
    the documentation and/or other materials provided with the
    distribution.

  * Neither the name of the copyright holder nor the names of its
    contributors may be used to endorse or promote products derived
    from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

## Servo (MPL-2.0)

One file of swb's source code is derived from Servo
(https://github.com/servo/servo, The Servo Project Developers), which is
licensed under the Mozilla Public License 2.0:

- `crates/layout/src/collapsed_margin.rs`: the type `CollapsedMargin`
  with its methods `new`, `adjoin` and `solve`, and the type
  `BlockMargins`. They are derived from `CollapsedMargin` and
  `CollapsedBlockMargins` in Servo's
  `components/layout/fragment_tree/fragment.rs`.

This file is under the MPL-2.0, not under swb's MIT License. The MPL-2.0
applies per file; the code is kept in its own file so that it covers
only this file. The rest of swb is not derived from Servo and stays under
swb's MIT License. The file carries the notice of the MPL-2.0:

> This Source Code Form is subject to the terms of the Mozilla Public
> License, v. 2.0. If a copy of the MPL was not distributed with this
> file, You can obtain one at https://mozilla.org/MPL/2.0/.

- Source code form: the file is in the swb source repository
  (https://github.com/senko/swb). A binary contains it in compiled form;
  its source code form is `crates/layout/src/collapsed_margin.rs` of the
  swb source revision that the binary was built from. Your rights to
  this file come from the MPL-2.0.
- The project owner decided on 2026-10-07 to keep this code for now,
  with this notice; a rewrite may follow.

## Test fixtures

`fixtures/pages/` holds copies of the target pages and their resources
(HTML, CSS, SVG, fonts, images), for offline tests only. The content
belongs to the owners of those sites. swb binaries do not contain it.

Before a fixture is committed, `swbtools substitute`
(docs/testing.md) replaces every raster image with a generated
placeholder and every font without a free license with DejaVu Sans, so
the repository does not publish photos or commercial fonts.

Fonts in the fixtures:

- `bbc`: the site's fonts are replaced with DejaVu Sans and DejaVu Sans
  Bold, converted to WOFF2 (a format change; the glyph data and the
  `name` table with the copyright and license are not changed). License:
  the Bitstream Vera license with the DejaVu changes in the public
  domain, text in `fixtures/fonts/LICENSE-DejaVu.txt`.
- `fixtures/fonts/`: the bundled test fonts, see `fixtures/fonts/README.md`.
