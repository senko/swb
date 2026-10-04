# Third-party notices

swb's own source code is licensed under the MIT License (see
[LICENSE](LICENSE)). swb binaries also contain third-party material that is
not under swb's MIT License. The MIT License of swb does not apply to it,
and does not limit the rights that its own license gives you.

This file lists the material that is compiled into swb binaries and is not
under a permissive software license: one component under a copyleft
license (the Public Suffix List) and material under CC BY 4.0 (from the
WHATWG HTML Standard). It does not reproduce the notices of the
permissive dependencies (MIT, Apache-2.0, BSD and others; `cargo deny list`
shows them). A binary distribution must include those notices too, for
example generated with `cargo about`.

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
