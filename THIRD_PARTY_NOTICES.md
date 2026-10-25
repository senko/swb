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
- the WebIDL files of `crates/web/idl/`, copied from WHATWG standards
  (BSD-3-Clause) and three W3C specifications (W3C Software and Document
  License), from which the binding glue in `crates/web/src/generated/` is
  generated;
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

## WHATWG DOM, HTML and Web IDL Standards: IDL (BSD-3-Clause)

`crates/web/idl/dom.idl`, `crates/web/idl/html.idl` and
`crates/web/idl/webidl.idl` contain IDL blocks copied from the DOM Living
Standard (https://dom.spec.whatwg.org/) and the Web IDL Living Standard
(https://webidl.spec.whatwg.org/), both last updated 19 October 2026, and
the HTML Living Standard (https://html.spec.whatwg.org/), copied on 23
and 24 October 2026, cut by swb to the members that it implements (the
headers of the files say what was cut; `html.idl` adds one typedef of
swb's own). The standards are licensed under CC BY 4.0, and "to the extent portions of it are
incorporated into source code, such portions in the source code are
licensed under the BSD 3-Clause License instead" (WHATWG IPR Policy 7.1.1,
https://whatwg.org/ipr-policy). The generated glue in
`crates/web/src/generated/` is derived from these files.

> Copyright © WHATWG (Apple, Google, Mozilla, Microsoft).
>
> Redistribution and use in source and binary forms, with or without
> modification, are permitted provided that the following conditions are
> met:
>
> 1. Redistributions of source code must retain the above copyright
>    notice, this list of conditions and the following disclaimer.
> 2. Redistributions in binary form must reproduce the above copyright
>    notice, this list of conditions and the following disclaimer in the
>    documentation and/or other materials provided with the distribution.
> 3. Neither the name of the copyright holder nor the names of its
>    contributors may be used to endorse or promote products derived from
>    this software without specific prior written permission.
>
> THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS
> IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
> TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
> PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
> HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
> SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED
> TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
> PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
> LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
> NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
> SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

## W3C SVG 2: IDL (W3C Software and Document License)

`crates/web/idl/svg.idl` contains IDL blocks from Scalable Vector Graphics
(SVG) 2 (W3C Candidate Recommendation Snapshot,
https://www.w3.org/TR/SVG2/, Appendix A "IDL Definitions",
https://www.w3.org/TR/SVG2/idl.html, copied on 2026-10-24); swb cut them to
the element interfaces that scripts use and added `SVGClipPathElement` and
`SVGMaskElement` without members (CSS Masking Module Level 1 defines them).
The owner allowed this license for IDL data files (ADR 0003, 2026-10-22).
The license text is in the section of W3C High Resolution Time below.

> This software or document includes material copied from or derived
> from Scalable Vector Graphics (SVG) 2, https://www.w3.org/TR/SVG2/.
> Copyright © 2026 World Wide Web Consortium.
> https://www.w3.org/copyright/software-license-2023/

## W3C DOM Parsing and Serialization: IDL (W3C Software and Document License)

`crates/web/idl/dom-parsing.idl` contains the `DOMParser` interface and
its type enumeration from DOM Parsing and Serialization (W3C Editor's
Draft, https://w3c.github.io/DOM-Parsing/, copied on 2026-10-24); swb
cut the file to this interface and renamed the enumeration. The license
text is in the next section.

> This software or document includes material copied from or derived
> from DOM Parsing and Serialization, https://w3c.github.io/DOM-Parsing/.
> Copyright © World Wide Web Consortium.
> https://www.w3.org/copyright/software-license-2023/

## W3C High Resolution Time: IDL (W3C Software and Document License)

`crates/web/idl/hr-time.idl` contains the typedef `DOMHighResTimeStamp`
from High Resolution Time (W3C Editor's Draft of 1 September 2026,
https://w3c.github.io/hr-time/); swb cut the file to this typedef. The
owner allowed this license for IDL data files (ADR 0003, 2026-10-22).

> This software or document includes material copied from or derived
> from High Resolution Time, https://w3c.github.io/hr-time/. Copyright ©
> 2026 World Wide Web Consortium.
> https://www.w3.org/copyright/software-license-2023/

The license (https://www.w3.org/copyright/software-license-2023/,
retrieved 2026-10-23):

> By obtaining and/or copying this work, you (the licensee) agree that
> you have read, understood, and will comply with the following terms and
> conditions.
>
> Permission to copy, modify, and distribute this work, with or without
> modification, for any purpose and without fee or royalty is hereby
> granted, provided that you include the following on ALL copies of the
> work or portions thereof, including modifications:
>
> - The full text of this NOTICE in a location viewable to users of the
>   redistributed or derivative work.
> - Any pre-existing intellectual property disclaimers, notices, or terms
>   and conditions. If none exist, the W3C software and document short
>   notice should be included.
> - Notice of any changes or modifications, through a copyright statement
>   on the new code or document such as "This software or document
>   includes material copied from or derived from [title and URI of the
>   W3C document]. Copyright © [$year-of-document] World Wide Web
>   Consortium. https://www.w3.org/copyright/software-license-2023/"
>
> Disclaimers
>
> THIS WORK IS PROVIDED "AS IS," AND COPYRIGHT HOLDERS MAKE NO
> REPRESENTATIONS OR WARRANTIES, EXPRESS OR IMPLIED, INCLUDING BUT NOT
> LIMITED TO, WARRANTIES OF MERCHANTABILITY OR FITNESS FOR ANY PARTICULAR
> PURPOSE OR THAT THE USE OF THE SOFTWARE OR DOCUMENT WILL NOT INFRINGE ANY
> THIRD PARTY PATENTS, COPYRIGHTS, TRADEMARKS OR OTHER RIGHTS.
>
> COPYRIGHT HOLDERS WILL NOT BE LIABLE FOR ANY DIRECT, INDIRECT, SPECIAL
> OR CONSEQUENTIAL DAMAGES ARISING OUT OF ANY USE OF THE SOFTWARE OR
> DOCUMENT.
>
> The name and trademarks of copyright holders may NOT be used in
> advertising or publicity pertaining to the work without specific,
> written prior permission. Title to copyright in this work will at all
> times remain with copyright holders.

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

- `ars-technica`: Source Sans 3 (© 2010–2021 Adobe, with Reserved Font
  Name ‘Source’), Faustina (Copyright 2019 The Faustina Project
  Authors), Exo 2 (Copyright 2013 The Exo 2 Project Authors), unchanged
  copies of the WOFF2 subsets that the site serves under these names.
  License: SIL Open Font License 1.1, text in
  `fixtures/LICENSE-OFL-1.1.txt`.
- `bbc`: the site's fonts are replaced with DejaVu Sans and DejaVu Sans
  Bold, converted to WOFF2 (a format change; the glyph data and the
  `name` table with the copyright and license are not changed). License:
  the Bitstream Vera license with the DejaVu changes in the public
  domain, text in `fixtures/fonts/LICENSE-DejaVu.txt`.
- `fixtures/fonts/`: the bundled test fonts, see `fixtures/fonts/README.md`.
- `crates/text/tests/webfonts/`: a subset of DejaVu Sans as TrueType,
  WOFF and WOFF2 (glyphs removed, the `name` table kept), see the
  `README.md` there. License: the Bitstream Vera license with the DejaVu
  changes in the public domain, text in `fixtures/fonts/LICENSE-DejaVu.txt`.
  The directory also holds `swb-variable.ttf`, a variable test font that
  was written for swb (boxes as glyphs, no third-party outlines; CC0 1.0),
  with the script that makes it.

## Unicode Character Database and Unicode CLDR (Unicode License v3)

The tables of the JavaScript engine (`crates/js-text/src/unicode/`) and of
line breaking (`crates/text/src/linebreak/tables.rs`) are generated from
files of the Unicode Character Database 17.0.0
(https://www.unicode.org/Public/17.0.0/ucd/, and, for the emoji sequences
of the sets of `\p{...}`, https://www.unicode.org/Public/17.0.0/emoji/), and
`crates/js-text/tests/data/normalization_sample.txt` is a sample of its
`NormalizationTest.txt`. The English time zone names of `Date`
(`crates/js/src/date/zone_names_data.rs`) are generated from three files of
the Unicode Common Locale Data Repository (CLDR) 48
(https://unicode.org/Public/cldr/48/cldr-common-48.zip: `common/main/en.xml`,
`common/supplemental/metaZones.xml`, `common/bcp47/timezone.xml`); their
headers say "Copyright © 1991-2025 Unicode, Inc. For terms of use, see
http://www.unicode.org/copyright.html" and (`en.xml`)
"SPDX-License-Identifier: Unicode-3.0", the same license as the UCD. The
header of each file of the UCD says:

> © 2025 Unicode®, Inc.
> Unicode and the Unicode Logo are registered trademarks of Unicode, Inc.
> in the U.S. and other countries.
> For terms of use and license, see https://www.unicode.org/terms_of_use.html

The Terms of Use put the data files under the Unicode License v3. Its
text, as published at https://www.unicode.org/license.txt (retrieved
2026-10-16):

> UNICODE LICENSE V3
>
> COPYRIGHT AND PERMISSION NOTICE
>
> Copyright © 1991-2026 Unicode, Inc.
>
> NOTICE TO USER: Carefully read the following legal agreement. BY
> DOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING DATA FILES, AND/OR
> SOFTWARE, YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE
> TERMS AND CONDITIONS OF THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT
> DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE THE DATA FILES OR SOFTWARE.
>
> Permission is hereby granted, free of charge, to any person obtaining a
> copy of data files and any associated documentation (the "Data Files") or
> software and any associated documentation (the "Software") to deal in the
> Data Files or Software without restriction, including without limitation
> the rights to use, copy, modify, merge, publish, distribute, and/or sell
> copies of the Data Files or Software, and to permit persons to whom the
> Data Files or Software are furnished to do so, provided that either (a)
> this copyright and permission notice appear with all copies of the Data
> Files or Software, or (b) this copyright and permission notice appear in
> associated Documentation.
>
> THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY
> KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
> MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
> THIRD PARTY RIGHTS.
>
> IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE
> BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES,
> OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
> WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION,
> ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA
> FILES OR SOFTWARE.
>
> Except as contained in this notice, the name of a copyright holder shall
> not be used in advertising or otherwise to promote the sale, use or other
> dealings in these Data Files or Software without prior written
> authorization of the copyright holder.
