# ADR 0014: License exception for the Public Suffix List (MPL-2.0)

- Status: accepted
- Date: 2026-10-03

## Context

Cookies (ADR 0012) need the registrable domain of a host: to reject a
`Domain` attribute that names a public suffix (`Domain=co.uk`), and to
decide whether two URLs are the same site (`SameSite`). The only maintained
public source of this information is the Public Suffix List
(https://publicsuffix.org/). All major browsers and many HTTP clients with
cookie support ship it. Without the list, `a.co.uk` could set cookies for
every `.co.uk` site.

The list is licensed under the Mozilla Public License 2.0. ADR 0003 does
not allow copyleft licenses, weak copyleft (MPL-2.0) included, so that the
dependency tree stays fully permissive and the license situation stays
simple.

swb gets the list through the `psl` crate. The crate declares
`MIT/Apache-2.0`. Almost all of the crate is a table (`src/list.rs`)
generated from the list; this table is what goes into the binary. The
crate's other copy of the list (`data/rules.txt`) has all comments
removed, the MPL-2.0 license header included. So the crate metadata does
not show the license of the data, and `cargo deny` accepted the crate
without a warning. If we relied on that metadata, the MPL-2.0 license
would be hidden ("license washing").

MPL-2.0 is a file-level copyleft. A larger work under another license can
include MPL-2.0 material unchanged (§3.3). The material stays under
MPL-2.0. Whoever distributes it in executable form must make its source
code form available and tell the recipients how to get it (§3.2(a)), and
must not limit the recipients' rights to the source code form (§3.2(b)).

Alternatives:

- No list: unsafe (see above).
- Embed `public_suffix_list.dat` in this repository and parse it: the same
  license, and more code to maintain.
- A list under a permissive license: none exists.

## Decision

- The owner allows the Public Suffix List as a **one-off exception** to
  ADR 0003. It is not a precedent and not a change of the policy. The
  default for every other dependency or data under MPL-2.0 or another
  copyleft license stays "no". Do not cite this ADR as a reason for
  another exception; only a new decision by the owner can make one.
- Scope: the list data. Today it comes in through the `psl` crate (any
  version, so that list updates come in with `cargo update -p psl`). The
  exception covers MPL-2.0 in `psl` as a whole: if a future version of the
  crate adds MPL-2.0 code that is not the list, that needs a new decision.
- swb does not modify the list. A modified copy would also be MPL-2.0.
- swb states the license in these places:
  - `deny.toml`, licenses: a `clarify` entry declares the real license of
    `psl`, `(MIT OR Apache-2.0) AND MPL-2.0`, and an `exceptions` entry
    allows MPL-2.0 for `psl` only. `cargo deny` fails for any other crate
    that declares MPL-2.0, and for `psl` if the exception is removed
    (checked).
  - `deny.toml`, bans: only `swb-net` may depend on `psl`, and the
    `publicsuffix` crate (a second copy of the list) is banned (checked:
    another wrapper fails the check).
  - `THIRD_PARTY_NOTICES.md`: the MPL-2.0 notice, that the MIT License of
    swb does not apply to the list, where the source code form is (with
    history, for the version in a given build), and that swb binaries
    contain it.
  - `README.md` (License), the `psl` entry in the workspace `Cargo.toml`,
    the module documentation of `crates/net/src/site.rs`,
    `docs/credits.md`, update notes in ADR 0003 and ADR 0012, and the
    owner decisions in `docs/ground-rules.md`.
- Whoever distributes swb binaries must include `THIRD_PARTY_NOTICES.md`
  (MPL-2.0 §3.2). The source code form stays available upstream; the
  distributor stays responsible for that.

## Consequences

- swb's own source code stays MIT. Binaries contain one MPL-2.0
  component, which `THIRD_PARTY_NOTICES.md` names.
- `cargo deny` checks only the licenses that crates declare. The `psl`
  crate shows that data can have another license than its crate: when you
  add a crate that embeds data (lists, tables, fonts, images), check the
  license of the data too.
- If the list comes in another way (another crate, or the file embedded
  directly), the exception covers only the list data in the new form, not
  other MPL-2.0 code of a new crate. Update `deny.toml`,
  `THIRD_PARTY_NOTICES.md` and this ADR in the same commit.
- If the license of the list changes, update the same files.
