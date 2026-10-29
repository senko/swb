# Credits and sources of inspiration

This file lists every source that influenced the design or code of swb:
specifications, articles, books, and other projects whose code was read for
ideas. Some parts of swb's code are derived from Chromium, Skia and Servo
code; the entries below say which, and
[THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md) lists them with their
licenses. No other code was copied from these projects.

Format: source, license (for code), what it influenced.

## Specifications

- WHATWG HTML Living Standard — https://html.spec.whatwg.org/ — the IDL
  of `Window`, the members of `Document` and `HTMLDocument`,
  `HTMLElement`, the interfaces of the HTML elements (from
  `HTMLHtmlElement` to `HTMLUnknownElement`, with `HTMLFormControlsCollection`,
  `RadioNodeList`, `HTMLOptionsCollection` and the mixin
  `HTMLHyperlinkElementUtils`), `ErrorEvent`, `PromiseRejectionEvent`,
  `PageTransitionEvent`, `SubmitEvent` and the other HTML events, and the
  event handler mixins, copied into `crates/web/idl/html.idl`
  (BSD-3-Clause for IDL in source code, WHATWG IPR policy 7.1.1); the
  title, body and named-access algorithms, the script type test, the
  reflection rules of content attributes, the form control, select and
  table algorithms, the event handler, focusing, activation and form
  submission algorithms, and the navigation to a `javascript:` URL, of
  `swb-web` and `swb-engine`.
- WHATWG Web IDL Standard — https://webidl.spec.whatwg.org/ — the
  conversions of `swb-web` (§3.2), overload resolution (§3.6), the
  interface objects and members (§3.7), exceptions and `DOMException`
  (§3.14, §4.3), the grammar of `swb-webidl`. Its `DOMException` IDL is
  copied into `crates/web/idl/webidl.idl` (BSD-3-Clause for IDL in source
  code, WHATWG IPR policy 7.1.1; see THIRD_PARTY_NOTICES.md).
- WHATWG DOM Standard — https://dom.spec.whatwg.org/ — the IDL of
  `Event`, `CustomEvent`, `EventTarget`, `window.event`, `Node`,
  `Document` (with `createEvent`), `DocumentType`,
  `DocumentFragment`, `NodeList`, `Element`, `CharacterData`, `Text`,
  `ProcessingInstruction`, `Comment` and the mixins `ParentNode` and
  `NonDocumentTypeChildNode`, copied into `crates/web/idl/dom.idl`
  (BSD-3-Clause, as above); the event listener, dispatch and
  `composedPath` algorithms of `swb-web` (`events/`).
- W3C High Resolution Time — https://w3c.github.io/hr-time/ — the
  typedef `DOMHighResTimeStamp` in `crates/web/idl/hr-time.idl` (W3C
  Software and Document License 2023; see THIRD_PARTY_NOTICES.md).
- W3C UI Events — https://www.w3.org/TR/uievents/ — the IDL blocks of
  the event interfaces in `crates/web/idl/ui-events.idl` (W3C Software
  and Document License 2023; see THIRD_PARTY_NOTICES.md); the order of
  the focus, mouse, wheel and keyboard events in `swb-engine`
  (`page/events.rs`, `page/input.rs`).
- W3C UI Events KeyboardEvent code values and Input Events Level 2 —
  https://www.w3.org/TR/uievents-code/, https://www.w3.org/TR/input-events-2/
  — the `code` names of the US layout (`engine/src/input.rs`) and the
  `inputType` values of `beforeinput` and `input`.
- W3C Pointer Events Level 4, CSSOM View Module and Pointer Lock 2.0 —
  https://www.w3.org/TR/pointerevents/,
  https://www.w3.org/TR/cssom-view-1/, https://www.w3.org/TR/pointerlock-2/
  — the IDL blocks of `PointerEvent`, `MouseEvent` and `WheelEvent` in
  `crates/web/idl/pointer-events.idl` (W3C Software and Document License
  2023; see THIRD_PARTY_NOTICES.md); the conversion between tilt and
  altitude/azimuth angles of `PointerEvent` (Pointer Events Level 4,
  example 1); the order of the pointer events and their compatibility
  mouse events in `swb-engine` (`page/events.rs`).
- W3C CSS Transitions Level 1 and Device Orientation and Motion —
  https://www.w3.org/TR/css-transitions-1/,
  https://www.w3.org/TR/orientation-event/ — the IDL blocks of
  `TransitionEvent`, `DeviceMotionEvent` and `DeviceOrientationEvent` in
  `crates/web/idl/transition-device-events.idl` (W3C Software and
  Document License 2023; see THIRD_PARTY_NOTICES.md).
- W3C Scalable Vector Graphics (SVG) 2 — https://www.w3.org/TR/SVG2/ —
  the IDL blocks of the element interfaces and `SVGAnimatedString` in
  `crates/web/idl/svg.idl` (W3C Software and Document License 2023; see
  THIRD_PARTY_NOTICES.md); the `ownerSVGElement` and `viewportElement`
  algorithms of `swb-web`.
- W3C DOM Parsing and Serialization — https://w3c.github.io/DOM-Parsing/
  — the `DOMParser` interface in `crates/web/idl/dom-parsing.idl` (W3C
  Software and Document License 2023; see THIRD_PARTY_NOTICES.md).
- W3C CSS Object Model (CSSOM) — https://drafts.csswg.org/cssom/ — the
  IDL of `CSSStyleDeclaration` and `ElementCSSInlineStyle` in
  `crates/web/idl/cssom.idl` (W3C Software and Document License 2023; see
  THIRD_PARTY_NOTICES.md); the declaration block algorithms (set, remove,
  serialize a declaration block, the camel-cased attributes) and
  `CSS.escape` of `swb_style::cssom`. CSS Conditional Rules 3 §8.1 —
  https://drafts.csswg.org/css-conditional-3/ — `CSS.supports`. CSS
  Values 4 §10.12 — https://drafts.csswg.org/css-values-4/ — the
  simplification of `calc()` sums in the serialization.
- W3C CSSOM View Module, Screen Orientation and Geometry Interfaces
  Module Level 1 — https://drafts.csswg.org/cssom-view/,
  https://www.w3.org/TR/screen-orientation/,
  https://drafts.fxtf.org/geometry/ — the IDL in
  `crates/web/idl/cssom-view.idl` and `crates/web/idl/geometry.idl` (W3C
  Software and Document License 2023; see THIRD_PARTY_NOTICES.md); the
  algorithms of `crates/web/src/cssom_view.rs` and
  `crates/engine/src/document/geometry.rs` (the offset parent, the
  client and scroll metrics, scroll into view, the pending scroll event
  targets); `MediaQueryList` and `MediaQueryListEvent` (IDL added
  2026-10-29), the algorithms of `crates/web/src/media_queries.rs`
  (evaluate media queries and report changes) and the serialization of
  media queries in `swb_css`.
- W3C Scalable Vector Graphics (SVG) 1.1 (Second Edition) —
  https://www.w3.org/TR/SVG11/ — the `SVGRect` interface that Chromium
  still returns from `getBBox` and `createSVGRect`; its IDL in
  `crates/web/idl/svg.idl` describes Chromium's members (four `float`
  attributes, measured).
- WHATWG HTML §2.6.1, reflecting content attributes in IDL attributes —
  the getters and setters of `[Reflect]` in `swb-web`. WHATWG HTML, the
  rendered text collection steps and the `innerText` and `outerText`
  setters (`crates/engine/src/document/inner_text.rs`,
  `crates/web/src/html/inner_text.rs`); animation frames and the order
  of the rendering update (`crates/web/src/animation_frames.rs`,
  `crates/engine/src/page/frame.rs`).
- WHATWG URL, Fetch, Encoding standards — https://url.spec.whatwg.org/,
  https://fetch.spec.whatwg.org/, https://encoding.spec.whatwg.org/
- CSS specifications (W3C CSS WG) — https://www.w3.org/Style/CSS/specs.en.html
  - CSS 2.2 (visual formatting model, box model, tables, floats)
  - CSS Syntax Level 3 (tokenizer and parser)
  - Selectors Level 4
  - CSS Cascading and Inheritance Level 4
  - CSS Values and Units Level 4
  - CSS Flexible Box Layout Level 1
  - CSS Grid Layout Level 2
  - CSS Lists 3 and CSS Counter Styles 3 (counters, list item numbers,
    the predefined counter styles in `style/src/counter_style.rs`)
  - CSS Cascade 4/5, Values 4/5, Color 4/5, Variables 1, Display 3,
    Backgrounds 3/4, Fonts 4, Text 3/4, UI 4, Align 3, Images 4, Lists 3,
    Content 3, Overflow 3, Sizing 3, Conditional 3/4 (`selector()`),
    Media Queries 4
  - CSSOM: serialization of identifiers, strings and selectors
    (`css/src/serialize.rs`, `css/src/selector/display.rs`)
  - HTML Living Standard, Rendering section (CC BY 4.0): the user-agent
    stylesheets `crates/style/src/ua.css` and `ua-quirks.css` and the
    presentational hints in `crates/style/src/hints.css` are based on its
    CSS
  - HTML Living Standard, the list of attributes whose values selectors
    match ASCII case-insensitively (`css/src/selector/parse.rs`)
  - Quirks Mode Standard — https://quirks.spec.whatwg.org/
  - HTML Living Standard, interaction and DOM sections: focusable areas and
    sequential focus navigation (`engine/src/focus.rs`), the `innerText`
    algorithm (the copied text, `engine/src/selection.rs`), the HTML
    fragment serialization algorithm (`dom/src/serialize.rs`)
- JSON-RPC 2.0 — https://www.jsonrpc.org/specification — message shape and
  error codes of the automation protocol
- UI Events KeyboardEvent key values — https://www.w3.org/TR/uievents-key/ —
  key names of the engine and the automation protocol
- ECMAScript 2025 Language Specification (ECMA-262, 16th edition) —
  https://tc39.es/ecma262/2025/ — clause 12 (lexical grammar) and Annex
  B.1 (HTML-like comments, legacy octal literals and escapes) —
  `js-syntax/src/lexer/`; §13.2.7.2 and §22.2.6.4 (regular expression
  flags) — `js-regexp`; §6.1.4 and §11.1 (code units and code points) —
  `js-text/src/string16.rs`; clauses 13 to 16 (expressions, statements,
  functions, scripts and their early errors), §9.1 and §10.2.11
  (environments, FunctionDeclarationInstantiation), §10.4.4.7 (mapped
  arguments), §14.7.4.4 (per-iteration bindings), §16.2.1.6.1
  (ParseModule), §19.2.1.1 and §19.2.1.3 (PerformEval,
  EvalDeclarationInstantiation) and Annex B.3.1 to B.3.5 —
  `js-syntax/src/parser/`, `js-syntax/src/scope/`. V8's error messages,
  measured as a black box in Node.js 22 — `js-syntax/src/messages.rs`.
  §6.1.6.1.20 (Number::toString), §7.1 (type conversion), §7.2.13 to
  §7.2.15 (comparison and equality), §10.2 (ordinary function objects),
  §10.4.4.6 (unmapped arguments), clause 13 (operators), §14.7.4.4
  (per-iteration bindings), §16.1.7 (GlobalDeclarationInstantiation) and
  §27.5 (generators) — `js/src/compiler/`, `js/src/vm/`; §14.12
  (`switch`), §14.15 (`try`), §27.5.3 (generator resumption) —
  `js-syntax`, `js/src/compiler/`, `js/src/vm/`; §10.2.4.1
  (%ThrowTypeError%), §20.1 to §20.5 (Object, Function, the error
  constructors), §21.1 (Number), §22.1 (String) and §23.1 (Array) for
  the built-ins in `js/src/builtins/`. V8's run-time error messages,
  measured in Node.js 22 — `js/src/vm/messages.rs`, `js/src/builtins/`.
  The Unicode Character Database 17.0
  (`DerivedCoreProperties.txt`: ID_Start, ID_Continue;
  `DerivedGeneralCategory.txt`: Zs) — `js-text/src/unicode/tables.rs`
  (`swbtools js-unicode-tables`).
- Unicode Standard Annex #14 (line breaking; revisions 51 to 57, for
  Unicode 15.1 to 18.0) and the Unicode Character Database 17.0
  (`LineBreak.txt`, `EastAsianWidth.txt`, `DerivedGeneralCategory.txt`,
  `emoji-data.txt`) — `text/src/linebreak.rs` and its generated
  `linebreak/tables.rs`; CSS Text 3 §5 (`word-break`, `hyphens`); UAX #9
  (bidi)
- RFC 4647 (language tag matching) — `:lang()`
- CSS Fonts 4 §4, §5 and §7 (`@font-face`, font matching, variation
  resolution) — `style/src/font_face.rs`, `text/src/web.rs`; §4 (the descriptors
  `size-adjust`, `ascent-override`, `descent-override`,
  `line-gap-override` and `src: local()`), §6 (`font-feature-settings`)
  and §7 (`font-variation-settings`) — `style/src/font_settings.rs`,
  `style/src/font_face.rs`, `text/src/web.rs`; WOFF 1.0 and
  WOFF 2.0 (W3C) — the container formats and the limits of
  `text/src/decode.rs` (ADR 0022)
- SVG 2 (coordinate systems; natural dimensions and the `viewBox`
  transform of SVG images) and CSS Images 3 (natural dimensions, the
  default sizing algorithm, the default object size) — `paint/src/svg/`,
  `layout/src/replaced.rs`
- SVG 2 for inline SVG (ADR 0023): §6.6 presentation attributes and the
  user-agent style sheet (`style/src/svg_attributes.rs`, `ua.css`); §8.2
  the `viewBox` transform and §8.9 units (`layout/src/svg/viewport.rs`,
  `draw.rs`); §9 path data and its error handling, §10 the equivalent
  paths of the basic shapes, Appendix B.2.4 and B.2.5 (elliptical arc
  conversion to the center parameterization, out-of-range radii)
  (`layout/src/svg/path.rs`); §13 fill and stroke properties
  (`style/src/properties/svg.rs`). CSS Transforms 1 (`transform-box`,
  the initial `view-box` reference box) and CSS Color 4 (`currentColor`
  as a computed keyword). The cubic Bézier approximation of circular arcs
  (control points at 4/3 · tan(θ/4) of the radius) is standard geometry.
  Part 2: SVG 2 §5.6 (`use`, the instance tree and its inheritance), §6.1
  (`href`), §8.11 (the bounding box: the fill box), §14.3 and CSS Masking 1
  §5 (`clipPath`, `clipPathUnits`, `clip-rule`, `clip-path`), §13.4 (`shape-rendering`),
  §16.4 (`pointer-events`) — `style/src/cascade/use_instances.rs`,
  `layout/src/svg/clip.rs`, `draw.rs`, `paint/src/hit_path.rs`. The extrema
  of quadratic and cubic Béziers (`SvgPath::fill_bounds`) are the roots of
  the derivative, standard calculus.
- HTML Living Standard, forms: form submission, constructing the entry
  list, implicit submission, value sanitization, radio button groups,
  selectedness, labels, constraint validation (`engine/src/forms/`);
  WHATWG URL `application/x-www-form-urlencoded` serializer; Infra
  newline normalization; RFC 7578 (`multipart/form-data`)
- RFC 6265bis (IETF draft, HTTP State Management Mechanism) — cookie
  parsing, storage, retrieval and `SameSite` (`net/src/cookies/`)
- WHATWG Fetch, "append a request `Origin` header" — the `Origin` header
  of `POST` requests

## Books and articles

- "Web Browser Engineering", Pavel Panchekha and Chris Harrelson —
  https://browser.engineering/ — overall structure of a minimal browser
  (layout tree, display list, the order of pipeline stages).
- Donald E. Knuth, The Art of Computer Programming, vol. 3, §6.4
  (multiplicative hashing with the golden ratio) — the name cache of
  `js-syntax/src/interner.rs`.
- The handle hash of `js-text/src/hash.rs`: the multiply-rotate step of
  the FxHash scheme (Firefox, rustc; MIT/Apache-2.0) and the 64-bit
  finalizer of MurmurHash3 (Austin Appleby, public domain), written from
  their published descriptions; no code copied.
- Jones, Hosking, Moss, The Garbage Collection Handbook (mark-and-sweep
  with a work list, weak tables) and the study memos 3 and 4 (shapes,
  dictionary mode, generational indices, the array density idea of Lua
  5.0) — `js/src/heap/`, `js/src/object/`.
- L. Peter Deutsch and Allan M. Schiffman, "Efficient Implementation of
  the Smalltalk-80 System" (POPL 1984: inline caches), Urs Hölzle, Craig
  Chambers and David Ungar, "Optimizing Dynamically-Typed
  Object-Oriented Languages With Polymorphic Inline Caches" (ECOOP 1991)
  and study memo 3.3 — the monomorphic caches of the property sites
  (M7 13a, `js/src/object/cache.rs`, `js/src/vm/inline_cache.rs`). The
  validation of prototype hits by a watch epoch, the global cache and
  the literal shapes are swb's own design; no engine code was read.
- The Lua 5.2 reference manual, §4.7 (continuations of C functions that
  call Lua), and study memo 1.2 ("resumable native functions") — the
  native loops of M7 13c (`js/src/vm/native_loop.rs`): a native that
  calls script functions in a loop returns its state, and the
  interpreter runs each call as a frame of its own loop. No engine code
  was read.
- The study memos 1 and 2 for the compiler and interpreter (register
  machine with temporaries in stack order after "The Implementation of
  Lua 5.0" and Duktape's design notes; one value stack and frame records
  without host recursion after Duktape and MicroQuickJS; deferred calls
  after MicroQuickJS) — `js/src/compiler/`, `js/src/vm/`. Exceptions
  after study memo 1.4: a handler table per code object (the JVM's
  exception table, CPython 3.11) and the `finally` completion stored in
  two registers (Duktape's design, ECMA-262 §6.2.4 and §14.15);
  termination and the time countdown at calls and jumps after memo 1.5
  (QuickJS) and HTML §8.1.4.5. No code was copied; the sessions read
  only the memos.
- Node.js `util.inspect`, measured as a black box — the output format of
  `console.log` (`js/src/builtins/console.rs`).
- The number conversions of `js-text/src/number/` (M7 feature 4), written
  from the literature, no code read or copied: G. L. Steele Jr. and
  J. L. White, "How to Print Floating-Point Numbers Accurately", PLDI 1990
  (exact fixed-format digit generation for `toFixed`, `toExponential`,
  `toPrecision`); R. G. Burger and R. K. Dybvig, "Printing Floating-Point
  Numbers Quickly and Accurately", PLDI 1996 (the free-format shortest
  digit generation of `toString(radix)` in `shortest.rs`, with unequal
  gaps at powers of two and inclusive ends for even mantissas; the first
  digit place is taken from the value instead of the high end of the
  range); W. D. Clinger, "How to Read Floating Point Numbers
  Accurately", PLDI 1990 (the fast path of an exact integer times an exact
  power of ten); D. E. Knuth, TAOCP vol. 2, §4.3.1 (the quotient estimate
  of Algorithm D, for one-digit quotients); W. Kahan, "Further remarks on
  reducing truncation errors", CACM 8(1), 1965 (compensated summation in
  `Math.hypot`, with the arguments scaled by the largest one; the results
  equal V8's in black-box runs); S. Vigna,
  "Further scramblings of Marsaglia's xorshift generators", Journal of
  Computational and Applied Mathematics 315, 2017 (xorshift128+ of
  `Math.random`, `js/src/vm/random.rs`). `toLocaleString` and
  `Math.sumPrecise` follow Chromium 148, measured (`just probe`).
- The `BigInt` arithmetic of `js-text/src/bigint/` (M7 10a), written
  from the literature, no code of an engine or of a multi-precision
  library read: D. E. Knuth, The Art of Computer Programming, vol. 2,
  3rd edition, §4.3.1 (Algorithms A, S and M for addition, subtraction
  and schoolbook multiplication; Algorithm D for long division, with its
  normalization, the quotient estimate from the top two limbs, the test
  with the third and the add-back step) and §4.6.3 (left-to-right binary
  exponentiation); N. Möller and T. Granlund, "Improved division by
  invariant integers", IEEE Transactions on Computers 60(2), 2011
  (Algorithm 4, the division of two limbs by one with a precomputed
  reciprocal, used in the single-limb division, in `toString` and in the
  quotient estimate of Algorithm D); H. S. Warren, Jr., "Hacker's
  Delight", 2nd edition, §9-2 (the test case of a multiword division that
  needs the add-back step, scaled to 64-bit limbs). The infinite two's
  complement form of the bitwise operators, `asIntN` and `asUintN` follow
  ECMA-262 §6.1.6.2 and §21.2.2. The reference table comes from Python's
  `int`, used as a black box; Chromium's size limit and messages were
  measured (`just probe` `tools/probes/js-bigint.json`).
- H.-J. Boehm, R. Atkinson and M. Plass, "Ropes: an Alternative to
  Strings", Software: Practice and Experience 25(12), 1995 — the ropes
  of `js/src/string/rope.rs` (M7 feature 5a): concatenation nodes with
  the length, short right parts copied into the leaf at the edge, a
  depth limit with a rebalance that turns the leaves in order into a
  balanced tree and joins short adjacent leaves. The chunk size that
  grows with the length, the two-level edge merge and the in-place
  flatten are swb's own (after study memo 4.2); no code read or copied.
- D. E. Knuth, J. H. Morris and V. R. Pratt, "Fast Pattern Matching in
  Strings", SIAM Journal on Computing 6(2), 1977 — the prefix function
  and the single pass of the search for needles of 16 or more code units
  (`js/src/builtins/string_search.rs`, M7 feature 5b); `lastIndexOf`
  runs it over the mirrored strings. Written from the paper's
  description; the charge of the work to the time countdown is swb's own.
  The URI functions, the padding, `split` and `GetSubstitution` follow
  ECMA-262 §19.2.6, §22.1.3 and §B.2; the messages and the V8
  quirks (`trimLeft` in the receiver message of `trimStart`, the
  `RangeError` texts) were measured in Chromium 148 (`just probe`
  `tools/probes/js-strings.json`).
- JSON (M7 feature 5c, `js/src/builtins/json*.rs`), written from
  ECMA-262 §25.5 and ECMA-404, no engine code read: the TC39 proposal
  "JSON.parse source text access" (https://github.com/tc39/proposal-json-parse-with-source;
  in Chromium 148) for `rawJSON`, `isRawJSON`, the parse
  records and the `context` argument of the reviver. The messages of
  `JSON.parse` and the circular structure error, the limits of
  Chromium's recursion and the quirk of a `space` between 0 and 1 were
  measured in Chromium 148 (`just probe` `tools/probes/js-json.json`).
  The parser reads a decimal number with the fast path of Clinger
  (above) when the mantissa is below 2^53 and the decimal exponent is
  at most 22 in magnitude.
- The sort of `Array.prototype.sort` and `toSorted` (M7 feature 6a,
  `js/src/builtins/array_sort.rs`), written from the textbook
  descriptions, no engine code read: binary insertion (D. E. Knuth, The
  Art of Computer Programming, vol. 3, §5.2.1) for runs of 16, then
  bottom-up mergesort with the test that skips the merge of two runs in
  order and insertion sort for small subarrays (R. Sedgewick and K. Wayne,
  "Algorithms", 4th edition, §2.2). The comparisons of integers by their
  decimal digits and the permutation of indices are swb's own. The
  methods follow ECMA-262 §23.1; the messages, the key order and the
  limits (the depth of `flat`) were measured in Chromium 148 (`just
  probe` `tools/probes/js-arrays.json`).
- The sorts of `%TypedArray%.prototype.sort` and `toSorted` (M7 feature
  11c, `js/src/builtins/typed_array_sort.rs`), written from the textbook
  descriptions, no engine code read: the sort with a comparator is the
  top-down mergesort (R. Sedgewick and K. Wayne, "Algorithms", 4th
  edition, §2.2) whose order of comparator calls was measured in
  Chromium 148 (`tools/probes/js-typed.json`, the groups `m-sort-calls`);
  the sort of one-byte and two-byte types is distribution counting (D. E.
  Knuth, The Art of Computer Programming, vol. 3, §5.2); the keys of the
  float and signed integer types whose unsigned order is the numeric order
  (flip the sign bit, or all bits of a negative float) are a common
  technique; the runs, the bottom-up merge and the charge of the time
  countdown are swb's own. The base64 and hex methods of `Uint8Array`
  follow the proposal text (TC39, "ArrayBuffer base64", `FromBase64`,
  `DecodeBase64Chunk`, `FromHex`) and the alphabets of RFC 4648 §4 and §5;
  no base64 library was read, and V8's messages and the cases where V8
  reads on after a full target were measured in Chromium 148.
- The ordered hash table of `Map` and `Set` (M7 feature 6b,
  `js/src/object/table.rs`), written from ECMA-262 §24.1 and §24.2 (the
  list of entries with empty entries for deleted keys) and from the
  textbook descriptions of open addressing with linear probing (D. E.
  Knuth, The Art of Computer Programming, vol. 3, §6.4, with its
  Algorithm R for deletion) and of an ordered hash table as a dense array
  of entries plus an index of positions (a common design; no engine code
  read). The skips over deleted entries use path splitting from
  union-find (R. E. Tarjan and J. van Leeuwen, "Worst-case analysis of
  set union algorithms", Journal of the ACM 31(2), 1984). The compaction
  that corrects the positions of live iterators, the list of cursors
  with its pruning and the epoch of `clear` are swb's own. The hash of numbers and handles is
  the keyed hasher of `js-text`; the hash of strings is the keyed
  `SipHash` of the atom table (J.-P. Aumasson and D. J. Bernstein,
  "SipHash: a fast short-input PRF", 2012, through Rust's standard
  library). The set methods follow ECMA-262 §24.2.4; the messages, the key
  order and the limit of 2^24 entries were measured in Chromium 148
  (`just probe` `tools/probes/js-collections.json`).
- The weak collections (M7 feature 9b, `js/src/object/weak.rs`,
  `js/src/heap/gc.rs`), written from ECMA-262 §9.9 to §9.13, §24.3,
  §24.4, §26.1 and §26.2. The ephemeron marking follows the definition in
  Barry Hayes, "Ephemerons: a new finalization mechanism", OOPSLA 1997 (a
  value is reached only through its reached key); the per-key lists of
  waiting values that make it linear, the two phases and the sweep order
  are swb's own. No engine or collector code was read; the messages and
  the key order were measured in Chromium 148 (`just probe`
  `tools/probes/js-weak.json`), the `console.log` forms in Node.js 22.
- The iterator helpers (M7 feature 6c, `js/src/builtins/iterator*.rs`,
  `js/src/object/iterator_helper.rs`), written from ECMA-262 §27.1 and
  the TC39 iterator sequencing proposal for `Iterator.concat`
  (https://tc39.es/proposal-iterator-sequencing/). The state machines
  that keep the closure variables in the helper object are swb's own;
  the messages and the key order were measured in Chromium 148 (`just
  probe` `tools/probes/js-iterators.json`).
- Howard Hinnant, "chrono-Compatible Low-Level Date Algorithms" —
  https://howardhinnant.github.io/date_algorithms.html — `days_from_civil`
  (`net/src/cookies/date.rs`) and `days_from_civil` and `civil_from_days`
  (M7 feature 12a, `js/src/date/time_value.rs`: the days of the 400-year
  eras, extended to the whole time range of ECMA-262 §21.4.1).
- `Date` (M7 feature 12a, `js/src/date/`, `js/src/builtins/date*.rs`),
  written from ECMA-262 §21.4 and Annex B.2.3, and the `jiff` library
  (Unlicense OR MIT; the offsets of a zone, the system zone, the
  transitions) used through its documentation; the formats, the key
  order and the messages were measured in Chromium 148 in seven time
  zones (`just probe` `tools/probes/js-date.json`), no engine code read.
  ADR 0027.
- `Date.parse` beyond the spec formats (M7 feature 12b,
  `js/src/date/iso_variants.rs`, `written.rs`): written from RFC 5322
  §3.2.2, §3.3 and §4.3 (the date, the obsolete syntax, the zone names,
  comments), RFC 850 §2.1.2 and RFC 1036 §2.1.2 (`DD-Mon-YY`), ISO C
  `asctime` and RFC 9110 §5.6.7 (the three HTTP date formats), RFC 3339
  §5.6 (a space for `T`, lower case `t` and `z`), ISO 8601-1 (decimal
  fractions, offsets) and ECMA-262 §21.4.1.32. Every other rule
  (separators, optional parts, ranges, two-digit years, the year of a
  date without one, a day past the month end, local time or UTC, what
  is NaN) was measured in Chromium 148 (`just probe`,
  `tools/probes/js-date.json`); no engine or browser code was read, and no
  design of any engine's parser was used (ADR 0025; a first version was
  withdrawn for this reason). ADR 0027.
- Zone names (M7 feature 12b, `zone_names.rs`, `zone.rs`,
  `tools/swbtools/js_time_zone_names.py`): ES5.1 §15.9.1.8 (an
  equivalent year for the daylight saving time), UTS #35 part 4 ("Using
  Time Zone Names", "Metazones": the rules for the long names and the
  localized GMT format) and the data of the Unicode CLDR 48 (`en.xml`,
  `metaZones.xml`, `bcp47/timezone.xml`; Unicode License v3,
  `THIRD_PARTY_NOTICES.md`). Chromium's use of the metazone of the
  present time was measured with 484 zones in Chromium 148, and its
  table of 14 equivalent years (one per weekday of January 1 and leap
  year) with seven zones whose rules change from year to year
  (`just probe`, `tools/probes/js-date.json`, groups `names-years`,
  `names-edge`); no source of any engine, browser or ICU was read. ADR
  0027.
- Unicode (M7 feature 5d, `js-text/src/unicode/case.rs`, `normalize.rs`,
  `collate.rs`, `tools/swbtools/js_case_tables.py`, `js_norm_tables.py`),
  written from the Unicode Standard 17.0 and no library read: chapter 3
  (§3.13 Default Case Conversion with the context `Final_Sigma`, Table
  3-17; §3.12 the conjoining Jamo behavior, the Hangul syllable
  algorithms), UAX #15 "Unicode Normalization Forms" (decomposition,
  canonical ordering, canonical composition, the quick check of §9.1),
  UTS #10 (the order of the classes in the default collation table, as a
  description only), and ECMA-262 §22.1.3.12, §22.1.3.15, §22.1.3.28 to
  §22.1.3.31. The data are files of the Unicode Character Database
  17.0.0 (https://www.unicode.org/Public/17.0.0/ucd/: `UnicodeData.txt`,
  `SpecialCasing.txt`, `CaseFolding.txt`, `DerivedCoreProperties.txt`,
  `DerivedNormalizationProps.txt`) in the generated tables, and a sample
  of `NormalizationTest.txt` as test data (Unicode License v3; see
  `THIRD_PARTY_NOTICES.md`). The order of `localeCompare` (four levels
  on the canonical decomposition, the ASCII punctuation order, the order
  of the marks U+0300 to U+036F) is swb's own, measured against Chromium
  148 (`tools/probes/js-unicode.json`).
- Unicode property sets of regular expressions (M7 feature 8c,
  `js-text/src/unicode/property_sets.rs`, `tools/swbtools/js_property_sets.py`),
  written from ECMA-262 §22.2.2.9 (`CompileToCharSet`), UAX #44 (the
  format of the data files) and UTS #51 (the emoji sequence files). The
  data are files of the Unicode Character Database 17.0.0
  (https://www.unicode.org/Public/17.0.0/ucd/: `DerivedGeneralCategory.txt`,
  `Scripts.txt`, `ScriptExtensions.txt`, `PropList.txt`,
  `DerivedCoreProperties.txt`, `DerivedNormalizationProps.txt`,
  `DerivedBinaryProperties.txt`, `emoji/emoji-data.txt`) and of the emoji
  data 17.0.0 (https://www.unicode.org/Public/17.0.0/emoji/:
  `emoji-sequences.txt`, `emoji-zwj-sequences.txt`) in the generated
  table (Unicode License v3; see `THIRD_PARTY_NOTICES.md`). No library
  was read; test262's generated property tests and Chromium 148 probes
  checked the result.

## Projects

- test262 (BSD-3-Clause, Ecma International), commit 2e0a567
  (2026-10-08) — the conformance tests of the JavaScript engine. A
  separate checkout in `out/test262`, not copied into the repository;
  the runner in `js/src/test262/` follows the frontmatter rules of its
  `INTERPRETING.md` and loads its harness files `assert.js` and
  `sta.js`.

- `wuff` (MIT, Nico Burns; a port of Google's woff2 decoder, MIT) — a
  dependency for WOFF and WOFF2 decoding; its source was read for the
  robustness audit of ADR 0022. No code copied.

- Chromium / Blink (BSD-3-Clause, parts LGPL) — font selection and
  fallback rules, so that swb picks the same fonts as Chromium on Linux
  (`text` crate, ADR 0006). Files read: Blink `font_cache_skia.cc`,
  `font_cache_linux.cc`, `alternate_font_family.h`,
  `font_platform_data.cc`; Chromium `ui/gfx/font_fallback_linux.cc`,
  `ui/gfx/linux/fontconfig_util.cc`. Also Blink's rounding of font metrics
  on Linux (`SimpleFontData`) and the `vertical-align: sub/super` offsets
  (`layout` crate). No code copied.
- Chromium / Blink LayoutNG table layout (BSD-3-Clause): in
  `third_party/blink/renderer/core/layout/table/`:
  `table_layout_utils.cc/.h`, `table_layout_algorithm.cc`,
  `table_layout_algorithm_types.cc/.h`,
  `table_section_layout_algorithm.cc`, `table_row_layout_algorithm.cc`,
  `table_borders.cc/.h`, `table_node.cc`, `layout_table_cell.cc/.h`,
  `layout_table_column_visitor.h`; in `core/layout/`: `length_utils.cc`
  (table width, auto margins, replaced contributions),
  `block_layout_algorithm.cc` (the `-webkit-` text-align offset, baseline
  propagation), `block_layout_algorithm_utils.cc`, `layout_utils.cc`,
  `block_node.cc`, `inline/inline_box_state.cc/.h`,
  `inline/inline_layout_algorithm.cc`, `inline/logical_line_builder.cc`,
  `inline/inline_item.cc`, `inline/inline_node.cc/.h`,
  `inline/line_breaker.cc` (line height quirks); `core/paint/
  table_painters.cc` (collapsed borders). Read for the algorithms
  (`layout/src/table/`, ADR 0010). Parts of `layout/src/table/columns.rs`,
  `distribute.rs`, `rows.rs`, `cells.rs` and `layout.rs` are derived from
  `table_layout_utils.cc`, `table_layout_algorithm_types.cc/.h` and
  `table_layout_algorithm.cc`; see `THIRD_PARTY_NOTICES.md`.
- Chromium / Blink LayoutNG grid layout (BSD-3-Clause): in
  `third_party/blink/renderer/core/layout/grid/`:
  `grid_layout_algorithm.cc`, `grid_track_sizing_algorithm.cc`,
  `grid_track_collection.cc`, `grid_placement.cc`,
  `grid_layout_utils.cc`, `grid_item.cc`, `grid_baseline_accumulator.h`;
  `core/css/properties/css_parsing_utils.cc`. Read for the ranges and
  sets of tracks, the distribution of extra space, the automatic
  repetitions, the minimum contribution, item alignment, the
  auto-placement cursors, grid baselines, grid value parsing and limits
  (`kGridMaxTracks`), and `TableNode::AllowColumnPercentages`
  (`layout/src/grid/`, `style/src/parse/grid.rs`, ADR 0017). Parts of
  `layout/src/grid/sizing.rs` and of `auto_repetitions` in
  `layout/src/grid/mod.rs` are derived from
  `grid_track_sizing_algorithm.cc/.h` and `grid_layout_utils.cc`; see
  `THIRD_PARTY_NOTICES.md`.
- Chromium / Blink, style: `html.css` and `quirks.css` (user-agent
  defaults, compared with the HTML spec; LGPL-2), `font_size_functions.cc`
  (font size keyword tables; LGPL-2), `font_builder.cc`
  (`CheckForGenericFamilyChange`, the monospace size rule),
  `style_builder_converter.cc` (font size absoluteness), and LayoutNG's
  `CalculateLeadingSpace` (half-leading rounding). Chrome's default font
  settings (`locale_settings_linux.grd`), checked against Chromium 148.
  Until 2026-10-07, the font size keyword tables
  (`style/src/values/keywords.rs`) and the form control, ruby and
  frameset rules of `style/src/ua.css` restated data from `html.css` and
  `font_size_functions.cc`. Data derived from LGPL files is not allowed
  (ADR 0003), so they now use the HTML Standard and values measured in
  Chromium 148 (`swbtools measure`). No code copied.
- Chromium 148, measured with Playwright (behavior, no code read): the
  focus ring of `outline-style: auto` (two rings, offsets, radii), the
  selection colors, `a:any-link:focus-visible { outline-offset: 1px }` in
  `html.css`, the 4 px drag threshold, 40 px arrow-key scrolling and the
  87.5% page step; the scrollable overflow of scroll containers
  (`scrollWidth`, `scrollHeight`), scroll chaining of the wheel and keys,
  scroll into view of nested containers, whole-pixel scroll offsets;
  responsive image selection (candidate order and choice for the device
  pixel ratio, `w` densities, `sizes` with `auto` and negative `calc()`,
  `<source type>` matching, a `<source>` with both a `media` that does
  not match and an unsupported `type`, the dimensions of a selected
  `<source>`, density-corrected sizes); with `swbtools measure`
  (docs/testing.md): the font families whose text fields use the width
  of `0`, the font size keyword tables, the computed styles of form
  controls, ruby and frameset elements, and the `<source>` checks; line
  break opportunities for `word-break: normal | break-all | keep-all`
  and `hyphens: none` (`text/src/linebreak.rs`, written from these
  measurements and UAX #14 only; `swbtools linebreaks`); web fonts:
  supported `format()` and `tech()` values, descriptor validity, which
  faces load for text and line boxes, composite font order, variation
  axis values and the rules for synthetic bold and oblique
  (`tools/probes/web-fonts.json`, ADR 0022); web fonts, part 2: which
  names `local()` matches (full name and PostScript name, ignoring case
  and spaces), how the metric descriptors scale and override the line
  box (overrides are ratios of the adjusted size), the order of the axis
  values (matching, `@font-face` descriptor, property), the serialization
  and the validity of the two settings properties, which of them the
  `font` shorthand resets, and that synthetic bold ignores
  `font-variation-settings` (`tools/probes/web-fonts-2.json`, ADR 0022);
  inline SVG: the sizes of the outer `<svg>` from its `width`, `height`
  and `viewBox` (presentation attributes, natural size, invalid values),
  its baseline and flex behaviour, the content-box clip, presentation
  attribute precedence, `currentColor` inheritance, `url()` paints
  without fallback, fill, stroke, dash, opacity and transform results by
  their ink; part 2: the boxes of `g`, shapes and `use` (fill bounding
  box, degenerate shapes, empty groups, `display: none`), which clip
  references and shapes count, the clip units, transforms and fill rule,
  cycles and invalid references, which pixels the hit test finds, the
  link cursor of SVG `a`, and the values of `shape-rendering` that turn
  anti-aliasing off (`tools/probes/inline-svg.json`, ADR 0023).
  Blink's design names (`ScrollableOverflowCalculator`,
  `ScrollManager::LogicalScroll`, `ScrollRectToVisible`) from
  recollection.
- Chromium / Blink, forms (`layout_text_control.cc` and `LayoutMenuList`
  and its successors are LGPL-2; `ui/native_theme/` is BSD-3-Clause),
  from recollection and confirmed by measurements with Chromium 148: text
  field widths (`layout_text_control.cc`: `GetAvgCharWidth`,
  `HasValidAvgCharWidth`, `PreferredContentLogicalWidth`), select option
  widths and optgroup indentation (`LayoutMenuList::UpdateOptionsWidth`),
  the check mark of `NativeThemeBase::PaintCheckbox`, label activation
  (`HTMLLabelElement::DefaultEventHandler`) and line breaks in pasted
  text (`TextFieldInputType`). The list of families whose text fields use
  the width of `0` (`layout/src/control.rs`) comes from a measurement
  (`swbtools measure text-field-families`, 2026-10-07), not from
  `layout_text_control.cc`. The check mark in `paint/src/control.rs` is
  derived from `ui/native_theme/native_theme_base.cc`; see
  `THIRD_PARTY_NOTICES.md`. No other code copied.
- Chromium (BSD-3-Clause), cookies: `net/cookies/cookie_monster.cc`
  (limits of 180/150 cookies per domain and 3300/3000 in total, LRU
  eviction), `net/cookies/cookie_util.cc` (`GetCookieDomainWithString`:
  the `Domain` attribute rule; `ComputeSameSiteContext`: the same-site
  contexts), `registry_controlled_domains` (unknown registries have no
  registrable domain). No code copied.
- usvg, resvg, svgtypes, simplecss, roxmltree, kurbo (Apache-2.0 OR MIT,
  MIT) — their source was read to mirror their behaviour in the SVG
  limits (`paint/src/svg/`): usvg's size resolution, viewBox transform,
  id and href resolution, caching and recursion; simplecss's selector
  matching; roxmltree's entity limits; kurbo's arc subdivision count.
  No code copied.
- Chromium / Blink (BSD-3-Clause), text layout and shaping (M3), read at
  `main` before the owner's rule of 2026-10-07 (ADR 0021):
  `core/layout/inline/line_breaker.cc/.h` (break rules at element
  boundaries, the `overflow-wrap` retry, the fit tolerance),
  `core/layout/inline/inline_node.cc` (where shaping stops at inline box
  edges, the shaping context),
  `platform/fonts/shaping/shaping_line_breaker.cc` and `shape_result.cc`
  (snapped widths); the synthesis of small capitals and the font size
  rules from recollection. Each rule was then measured in Chromium 148,
  and the code cites the measurements (`layout/src/inline/breaks.rs`,
  `overflow.rs`, `caps.rs`, `shaping.rs`; `text/src/shape.rs`). A first
  line breaker, written after reading Blink's `text_break_iterator.cc`
  (LGPL) and `character_property_data_generator.cc`, was discarded before
  it was committed; `text/src/linebreak.rs` was written in a clean room
  from UAX #14 and measurements (ADR 0021).
- Chromium / Blink LayoutNG floats (BSD-3-Clause), from recollection and
  confirmed by measurements with Chromium 148: the exclusion space and
  layout opportunities, `BlockLayoutAlgorithm::HandleFloat`,
  `NextBorderEdge`, `HandleNewFormattingContext` (margins next to
  floats, `abort_if_cleared`), `HasClearancePastAdjoiningFloats` and the
  forced BFC block offset, `InlineLayoutAlgorithm::Layout`,
  `LineBreaker::HandleFloat`, `ShouldWrapLine`,
  `IsEqualToAvailableFloatInlineSize`, `ComputeMinMaxSizes` with floats
  (`layout/src/floats.rs`, ADR 0015). swb's exclusion space is its own
  model (two step functions in a sorted list of segments), not Chromium's
  shelves. `try_opportunity` in `layout/src/block.rs` is derived from
  `HandleNewFormattingContext`, and `blocks_content_sizes` in
  `layout/src/intrinsic.rs` from `ComputeMinMaxSizes`
  (`core/layout/block_layout_algorithm.cc`); see
  `THIRD_PARTY_NOTICES.md`.
- Chromium / Blink (BSD-3-Clause), counters:
  `core/css/counters_attachment_context.cc/.h` and
  `core/html/list_item_ordinal.cc/.h`, read for the counter scope,
  `list-item` counter and ordinal value rules (`style/src/counters.rs`);
  the rules were measured in Chromium 148 (the unit tests are generated
  from its output). No code copied.
- Chromium / Blink (BSD-3-Clause), positioning, from recollection and
  confirmed by measurements with Chromium 148: static positions in
  inline content (`InlineLayoutAlgorithm::PlaceOutOfFlowObjects`,
  `IsOriginalDisplayInlineType`), inline containing blocks, sticky
  offsets with shifting sticky ancestors
  (`StickyPositionScrollingConstraints`), `clip` on fixed descendants
  (`CssClipFixedPosition`) (`layout/src/positioned.rs`, ADR 0016). The
  sticky offsets in `layout/src/positioned.rs` are derived from
  `core/page/scrolling/sticky_position_scrolling_constraints.cc`; see
  `THIRD_PARTY_NOTICES.md`.
- Servo (layout 2020, MPL-2.0) — the idea of keeping a fragment at the
  static position of a hoisted absolutely positioned box; swb replaces
  it with the laid-out fragment. No code copied for it.
- Servo (MPL-2.0) — `layout/src/collapsed_margin.rs` (`CollapsedMargin`,
  `BlockMargins`) is derived from Servo's `CollapsedMargin` and
  `CollapsedBlockMargins` (`components/layout/fragment_tree/fragment.rs`).
  This file is under the MPL-2.0; see `THIRD_PARTY_NOTICES.md`.
- Servo and Blink — the idea of an ancestor Bloom filter for selector
  matching (`SelectorFilter`), and right-to-left selector matching with
  limited backtracking, which all browser engines use. No code read for
  swb's implementation.
- Björn Ottosson, "A perceptual color space for image processing" — the
  OKLab matrices (`style` crate color conversion), and the sample
  conversion code in CSS Color 4.
- Skia (BSD-3-Clause) — metric selection and synthetic bold/oblique
  parameters (`text` crate). Files read: `SkFontConfigInterface_direct.cpp`,
  `SkFontHost_FreeType.cpp`, `SkScalerContext.cpp`, `SkTextFormatParams.h`.
  The metric-compatible family classes (`METRIC_COMPATIBLE` in
  `text/src/source/fontconfig.rs`) are derived from `GetFontEquivClass`
  in `SkFontConfigInterface_direct.cpp`, and the synthetic bold stroke
  widths (`fake_bold_scale` in `text/src/raster.rs`) from
  `SkTextFormatParams.h`; see `THIRD_PARTY_NOTICES.md`.

## JavaScript engine study (ADR 0025)

`analyst` sessions read these engines on 2026-10-09 and wrote the study
memos in `docs/js-study/`. The memos describe ideas in words and contain
no code. The "Sources" section of each memo lists the files that were
read and the literature. Sessions that design, implement or review swb's
JavaScript engine read only the memos, never the engine source.

- QuickJS (MIT, Fabrice Bellard, Charlie Gordon), commit 535a7c2
  (2026-09-29), including `doc/quickjs.texi` — memos 1 to 5.
- QuickJS-ng (MIT, Fabrice Bellard, Charlie Gordon, Ben Noordhuis, Saúl
  Ibarra Corretgé), commit c359cac (2026-10-09) — memos 1 to 5.
- MicroQuickJS (MIT, Fabrice Bellard, Charlie Gordon), commit 6d4d7eb
  (2026-09-26) — memos 1 to 5.
- MuJS (ISC, Artifex Software), commit aab59f2 (2026-10-06),
  https://codeberg.org/ccxvii/mujs — memos 1 to 5.
- Duktape (MIT, Duktape authors), commit 3afa016 (2026-09-05, 3.0
  development), source and the design notes in `doc/` — memos 1 to 5.

Literature used by the memos (details in each memo): Ierusalimschy, de
Figueiredo, Celes, "The Implementation of Lua 5.0"; Nystrom, Crafting
Interpreters; Shi, Gregg, Beatty, Ertl, "Virtual Machine Showdown"; the
Self papers on maps and polymorphic inline caches (Chambers, Ungar, Lee;
Hölzle, Chambers, Ungar); Brunthaler, "Inline Caching Meets Quickening";
Jones, Hosking, Moss, The Garbage Collection Handbook; Bacon and Rajan
(trial deletion); Hayes (ephemerons); Jonkers (sliding compaction);
Boehm, Atkinson, Plass (ropes); Steele and White, Burger and Dybvig, Gay,
Clinger, Loitsch, Adams (Ryū), Lemire (number conversion); Knuth TAOCP
vol. 2; Brent and Zimmermann, Modern Computer Arithmetic; Russ Cox's
regular expression articles; Davis, Servant, Lee (selective
memoization); RFC 9636 and the IANA time zone database documentation;
the V8 blog, the SpiderMonkey documentation and the WebKit blog
(articles and documentation, not source).

## Bundled data

- Liberation fonts 2.1.5 (Sans, Serif, Mono; Regular, Bold, Italic,
  BoldItalic) — SIL Open Font License 1.1 — `fixtures/fonts/`, test fonts.
- DejaVu Sans 2.37 (Regular, Bold) — Bitstream Vera license, DejaVu changes
  in the public domain — `fixtures/fonts/`, test fonts.

See `fixtures/fonts/README.md` for file details.

## Data in dependencies

- Public Suffix List (Public Suffix List project, Mozilla; **MPL-2.0**) —
  compiled into a table in the `psl` crate (the crate's hand-written code
  is `MIT OR Apache-2.0`; the table is the list, under MPL-2.0) —
  registrable domains for cookies and sites (`net/src/site.rs`). A
  one-off exception to the dependency policy, see ADR 0014 and
  `THIRD_PARTY_NOTICES.md`.
