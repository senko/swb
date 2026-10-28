# Architecture

This document describes the current structure of swb. Update it when the
structure changes. Decisions and their reasons are in [adr/](adr/).

## Pipeline

```
URL ──► net ──► bytes ──► dom (html5ever) ──► Document
                                                │
          stylesheets (net) ──► css ──► style ◄─┘
                                          │  computed styles
                                          ▼
                              layout (uses text) ──► fragment tree
                                                        │
                                                        ▼
                                  paint ──► display list ──► pixels
```

`engine` drives this pipeline for one page. It also handles input (pointer,
keyboard, focus, text selection), scrolling, navigation and history.
`engine` has no windowing code, so it runs the same way in headless mode,
in the GUI and under the automation server.

Design records per stage: text in [ADR 0006](adr/0006-text-stack.md)
and web fonts in [ADR 0022](adr/0022-web-fonts.md),
style in [ADR 0007](adr/0007-style-system.md), interaction in
[ADR 0009](adr/0009-interaction.md), automation in
[ADR 0008](adr/0008-automation-protocol.md) and
[automation.md](automation.md).

## Crates

All crates are in `crates/`. The package name is `swb-<dir>`.

| Crate        | Responsibility                                                                 | Depends on                    |
|--------------|--------------------------------------------------------------------------------|-------------------------------|
| `net`        | Fetch resources: `http`, `https`, `file`, `data`, `about`. Cookie jar (RFC 6265bis). Replay from fixtures. | — |
| `dom`        | Arena-based DOM tree. HTML parsing (html5ever tree builder into our tree; the resumable parser `HtmlParser` in `parser.rs` with the source positions of inline scripts in `parser/position.rs`, the tree sink in `parser/sink.rs`, fragment parsing and the parse of a whole document into a new document of the arena (`DOMParser`) in `parser/fragment.rs`), the change functions (`mutation.rs`) and the other DOM algorithms of scripts (`algorithms.rs`: clone, node equality, document position, `normalize`, the reset of form owners), the change journal and the mutation sink (`journal.rs`), the id index (`id_index.rs`), character encoding detection, the HTML serialization of `outerHTML` and `innerHTML` (`serialize.rs`, iterative), the tree dump. Scripting is enabled only for nodes of the document of `NodeId::DOCUMENT` (`scripting_for`), and each other document of the arena has its own quirks mode. | — |
| `css`        | CSS syntax: tokenizer, rule and declaration parser, serializer, selector parser and matcher, media queries, `@supports` conditions, the `sizes` attribute. | — |
| `style`      | Property definitions, value parsing, cascade, inheritance, computed values, CSS counters and list item numbers, `@font-face` descriptors, `font-variation-settings` and `font-feature-settings`. `query.rs` is the selector queries of scripts (`Matcher`: `matches`, `closest`, the first or all matches below a node, with a bound on the work of one query). `cssom/` is the declaration block of `element.style` (feature 7a): `StyleDeclarations` (longhands only, values kept as Chromium serializes them, checked by swb's property parsers), the property table with Chromium's names, logical longhands and the IDL attribute names (`props.rs`), the serialization of values (`value.rs`, `longhand.rs`) and of shorthands (`shorthand.rs`), `CSS.supports` and `CSS.escape`. `cssom/resolved/` is `getComputedStyle` (feature 7a2): the computed values serialized as Chromium resolves them, the values that layout resolves from a `UsedBox`, and the shorthands. `compute_element_styles` is the cascade of one element and its ancestors, `compute_pseudo_element_style` that of a pseudo-element also without a box (ADR 0028 section 9). | `css`, `dom`                  |
| `text`       | Font discovery and matching, web font faces (`@font-face`) and their decoding (WOFF, WOFF2), fallback, shaping, glyph outlines and masks. | —                             |
| `layout`     | Box tree construction, layout algorithms, fragment tree, the geometry of inline SVG (ADR 0023). | `dom`, `style`, `text`        |
| `paint`      | Display list, rasterization, image decoding (raster formats; SVG images with resvg, ADR 0011), inline SVG drawing commands, hit testing of SVG shapes, group bounds, image reduction, and the cost model of path rasterization (`path_cost`). | `dom`, `layout`, `style`, `text` |
| `engine`     | Page lifecycle (`page/`: navigation, history, loading, input, the script runtime of the document in `script.rs`) and the state of one document (`document/`: the DOM and its parser, the journal drain, subresources, style and layout pipeline, queries; `DocumentState` implements `web`'s `EngineHost`); focus, selection, form controls, hit testing. | all of the above, `web` |
| `automation` | Remote-control protocol: WebSocket server, methods, headless runner, Rust client. | `engine`, `net`, `dom`       |
| `js-text`    | JavaScript text: code units of both widths (`Str16`, `String16`), code points, the Unicode data (`unicode/`; generated tables from the UCD 17.0.0: the character classes of the lexer; case mapping with the full mappings and `Final_Sigma` (`case.rs`); normalization after UAX #15 with Hangul by the algorithm, a stable sort of the non-starters and a one-pass composition, in slices (`normalize.rs`); the four-level order of `localeCompare` on the canonical decomposition (`collate.rs`); the names and aliases of the Unicode properties of regular expressions (`property.rs`) and their code point sets, General_Category, Script, Script_Extensions, the binary properties and the emoji strings, as delta-encoded ranges that are decoded on use (`property_sets.rs`, data in `property_data.rs`), and the Canonicalize rule of regular expressions without `u`), numbers and their text forms (`number/`: `StringToNumber`, `parseFloat` and `parseInt` in one bounded pass, decimal digits to Rust's float parser, `parseInt` exact in all radixes; `Number::toString` in all radixes, in radixes other than 10 the shortest digits that read back, the closest of them (Burger and Dybvig's free format, `number/shortest.rs`); exact `toFixed`, `toExponential` and `toPrecision` on multi-precision integers; the en-US `toLocaleString`; the exact sum of `Math.sumPrecise` and `Math.f16round`; the binary16 bits of `getFloat16` and `setFloat16`, `number/binary16.rs`), the integers of `BigInt` (`bigint/`: sign and magnitude in 64-bit limbs, schoolbook multiplication, Knuth's Algorithm D and the single-limb division with a precomputed reciprocal, powers, shifts and the bitwise operators on the two's complement form, `asIntN` and `asUintN`, the exact conversions and comparisons with doubles, `StringToBigInt` and the literals, `toString` in all radixes; a size limit of 2^18 bits, fallible allocation and a cost per operation for the engine's time countdown), a keyed hasher for small integer keys, and the engine's shared recursion budget (`RecursionBudget`). ADR 0026. | — |
| `js-regexp`  | Regular expressions (ADR 0026 section 10). The parser (`parser/`) reads a pattern in the grammar of the `u` and `v` flags or in the web grammar of Annex B.1.2, with all early errors and V8's messages, and builds a tree (`ast.rs`) bottom-up with an explicit stack; the flags of modifiers and the case closure of `i` are resolved into the leaves (character sets of `charset.rs`). The compiler (`compile.rs`) emits a backtracking program of enum instructions (`program.rs`) with counter registers for counted quantifiers and backward instructions for lookbehinds. The matcher (`exec.rs`) is a loop with a backtrack stack and an undo log over both string widths; it stops after a given number of steps and continues on the next call (`Matcher`), within a memory limit that the caller passes; `Matcher::reset` reuses its vectors for the next match. `check_literal` checks literals for `js-syntax` without building the tree. The sets of `\p{...}` and `\P{...}` come from `js-text` and are cached per property in the parser. | `js-text` |
| `js-syntax`  | JavaScript front end: lexer (on demand, by goal symbol), name interner, parser with the early errors, the AST in an arena per script, and the scope analysis (registers, cells, globals by name, captures, a separate body scope for functions with parameter expressions). Cover grammars convert the tree (`parser/patterns.rs`): a literal before `=` becomes an assignment pattern, the expression before `=>` the arrow's parameters; the names, scopes and functions of a cover expression are logged per open scope, so an arrow moves each item once. A class has two scopes: the class scope holds the inner binding of the name (the heritage is evaluated in it), the class body scope holds the private names as bindings named `#x`, which are indices into one implicit binding `%private` (the class's private environment) and take no registers; the parser checks private names when the outermost class ends. The field initializers of a class belong to two synthetic functions (instance; static, with the static blocks as var scopes), so `this`, `super` and captures follow the function rules. `super` and `super(...)` resolve to implicit bindings of the nearest non-arrow function (home object, active function), like `this`. Async arrows come from the call cover `async(...)`; the context of each function says whether `await` is an identifier, an expression or reserved. `parse_module` parses the Module goal and returns the module records of ParseModule (`module.rs`): top-level declarations are registers or cells of the module function (exported ones are cells), imports are cells that the module function receives as captures. `parse_eval` parses eval code with the early errors of its call site; names of direct eval code that do not resolve inside it resolve against the caller. `parse_dynamic_function` parses the source of a `Function` constructor call (`(function anonymous(P\n) {\nbody\n})`, as V8 builds it) and checks with positions that the parameters end at their text's end and that the script is the one function (V8's messages); the function's name `anonymous` is not bound. `parse_event_handler` parses the source of an event handler in the same way, as direct eval code, so that its free names resolve outside (the engine looks them up in the handler's object environments). Annex B.3.2 block functions get their var binding in the analysis, after the parse. `with` bodies and the var scopes of sloppy functions with a direct `eval` are dynamic environments: an implicit `%env` binding holds the run-time record, and each reference in their reach records how many records to check before its static binding. A function with a direct `eval` gets one reference per name of the enclosing functions that its eval code can see (`ScopeTree::eval_outer_references`, resolved from the scope around the function), so it captures every binding that eval code can reach. The name table keeps the well-known names in one table per process. | `js-text`, `js-regexp` |
| `js`         | JavaScript engine: heap with a mark-and-sweep collector (`heap/`; `heap/gc.rs` also marks the ephemerons of `WeakMap` and sweeps the weak objects, `heap/weak_roots.rs` the kept set of `WeakRef` targets and the queue of finalization cleanup jobs), values, strings: flat text or ropes, atoms and property keys (`string.rs`; `string/rope.rs` the concatenation rules, the in-place flatten and rebalance, the leaf walk for comparisons and copies), objects with shapes (`object/`; `object/cache.rs` the inline cache entries of the property sites, their checks and the walks that fill them, with the watch epoch of the prototypes and global objects; `object/table.rs` the table of `Map` and `Set`, `object/collection.rs` the heap operations on it (lookup, insert, remove, iterators, native loops); `object/element_access.rs` the element access of the natives: the rule of ordinary elements, the searches for the next stored index along the prototype chain, the dense array state that allows a shortcut, and the dense builder that makes arrays without the general property definition; `object/dense.rs` the dense vector with a start offset, so that `shift` moves nothing), the bytecode compiler (`compiler/`, `bytecode/`: `mod.rs` the instruction set and the code objects, `verify.rs` the listing and the verifier, `eval_info.rs` the scope descriptions of functions with a direct `eval`; `compiler/expr.rs` expressions, `compiler/assign.rs` assignment and update, `compiler/call.rs` calls, `new`, tagged templates and optional chains, `compiler/function.rs` the function compiler and its registers, scopes and emission, `compiler/statement.rs` statements; `source_text.rs` the source text that the functions of a script share, one byte per unit with a list for the few wide units, and the script name and line index (`Source`); `compiler/support.rs` rejects the constructs that it does not compile yet, module code included, before any function compiles; `compiler/pattern.rs` destructuring, `compiler/params.rs` parameter lists that are not simple, `compiler/iterate.rs` `for`-`of`, `for await`, spread and the loops of `yield*` in generators and async generators, `compiler/class.rs` classes, private names, `super` and the default constructors; `compiler/dynamic.rs` the references in the reach of `with` bodies and of eval vars, and the `with` statement; `compiler/eval.rs` the scope descriptions of functions that call `eval` and the access of eval code to its caller's cells; `compiler/positions.rs` and `debug_names.rs` give Chromium's source positions and the names of anonymous functions in stack traces), the interpreter with the miss paths of its inline caches (`vm/inline_cache.rs`) and the global cache (`vm/global.rs`), the native loops: natives that call script functions in a loop return their state, and its calls run as frames of the interpreter loop whose returns run the next step (`vm/native_loop.rs`; the array, typed-array and sort callbacks, `replace` with a function), realms, conversions, the internal methods and iterator operations for natives and for compiled code (`vm/`; `vm/internal.rs`, `vm/iter.rs`), the keys of `for`-`in` (`vm/for_in.rs`), Proxy objects: the slots (`object/proxy.rs`), the 13 internal methods with their invariant checks and the generic forms of the internal methods that dispatch to them, which the heap's walks hand over to when they meet a proxy (`vm/proxy.rs`), and `Proxy` with `Proxy.revocable` (`builtins/proxy.rs`), the dynamic environment records of `with` and of eval vars (`vm/env.rs`), `eval`: direct and indirect `PerformEval` and `EvalDeclarationInstantiation`, eval code run as a frame (`vm/eval.rs`), generator objects: the frame moved in and out at `yield`, the resume modes, the steps of `yield*` (`vm/generator.rs`), async functions on the same machinery: the result promise, `await` as an internal reaction that resumes the frame from a job, the end of the body that settles the promise (`vm/async_function.rs`), async generators: the generator object with its queue of requests, `AsyncGeneratorYield`, the await of a return request's value, the completion and the drain of the queue (`vm/async_generator.rs`), async iteration in compiled code: `GetIterator` with kind async, the steps and the awaited close of `for await`, the steps of an async `yield*`, and the Async-from-Sync Iterator (`vm/async_iteration.rs`), promises: the promise objects with their reaction pairs, whose handlers are functions or internal continuations, the resolving functions, `PerformPromiseThen`, the capabilities and `SpeciesConstructor` (`vm/promise.rs`), the job queue with the drain and the rejection tracker (`vm/jobs.rs`; built-in functions with internal slots carry the state of the resolving and element functions), the `Function`, `GeneratorFunction`, `AsyncFunction` and `AsyncGeneratorFunction` constructors (`CreateDynamicFunction`: the source text, its parse and compile charged like eval code, the function object with the prototype of `new.target`; `vm/create_function.rs`), mapped arguments objects, whose mapped elements hold the cells of their parameters, and the template of each realm that the arguments objects after the first are made from in one step (`object/arguments.rs`), the global declarations with the checks of Annex B.3.2.2 (`vm/global.rs`), the run-time parts of classes and `super` properties (`vm/class.rs`), private names: the private environment of each class evaluation, `PrivateGet`, `PrivateSet`, the brands of private methods and `#x in o` (`vm/private.rs`), with the private elements of objects as entries of their shapes under private symbols (`object/private.rs`), the run-time parts of literals: accessors, object spread and object rest, function names of computed keys and template objects (`vm/literal.rs`), rest parameters, spread calls and V8's destructuring messages (`vm/destructure.rs`), array patterns that step a plain array with the built-in iteration without an Array Iterator, with the cache of that check (`vm/array_pattern.rs`), the well-known symbols and the symbol registry of the runtime (`vm/symbols.rs`), the `stack` of error objects in Chromium's format (`vm/stack.rs`, `vm/stack_text.rs`), the built-ins (`builtins/`, one module per object or group: `object.rs`, `object_proto.rs`, `object_iter.rs`, `descriptor.rs`, `integrity.rs`, `reflect.rs`, `function.rs`, `generator.rs` (`GeneratorFunction`, its prototype and `%GeneratorPrototype%`), `async_function.rs` (`AsyncFunction` and its prototype), `async_generator.rs` (`AsyncGeneratorFunction`, its prototype, `%AsyncGeneratorPrototype%`, `%AsyncIteratorPrototype%`, `%AsyncFromSyncIteratorPrototype%`), `promise.rs` (`Promise`, its statics and `Promise.prototype`) and `promise_combinators.rs` (`all`, `allSettled`, `any`, `race` and their element functions), `symbol.rs`, `boolean.rs`, `number.rs`, `math.rs`, `iterator.rs`, `global.rs` and others; the iterator helpers are `iterator.rs` (the `Iterator` constructor and the accessors of `Iterator.prototype`), `iterator_helper.rs` (`map`, `filter`, `take`, `drop`, `flatMap` as native state machines with the states of a generator, `%IteratorHelperPrototype%`; the variables of the closure are in `object/iterator_helper.rs`), `iterator_consume.rs` (`reduce`, `toArray`, `forEach`, `some`, `every`, `find`) and `iterator_from.rs` (`Iterator.from`, `Iterator.concat`, `GetIteratorFlattenable`, `%WrapForValidIteratorPrototype%`); `Array` is `array.rs` (the constructor, `from`, `of`, the key order of `Array.prototype`), `array_from_async.rs` (`Array.fromAsync` as a state machine whose awaits are internal reactions), `array_access.rs` (the element loops: `HasProperty`, `Get`, `Set` and `DeletePropertyOrThrow` on an index, the loops that visit only the stored indices, the moves of `shift`, `unshift`, `splice` and `copyWithin`), `array_iteration.rs` (the callback methods), `array_search.rs` (`at`, the searches, `join`), `array_mutate.rs` (the methods that change the receiver, with the shortcuts in the dense vector), `array_copy.rs` (`concat`, `slice`, `flat`, the copying methods) and `array_sort.rs` (`sort`, `toSorted`, a stable merge sort of an index permutation), and `array_species.rs`; `Map` and `Set` are `map.rs` (the constructor, `groupBy`, the methods, `getOrInsert` and `getOrInsertComputed`), `set.rs`, `set_methods.rs` (the ES2025 set methods with `GetSetRecord`) and `collection.rs` (what both share: the key preparation, the receiver checks, `forEach`, the constructors' loop, the iterator objects), over the ordered hash table of `object/table.rs` (entries in insertion order, an open-addressing index with a keyed hash, compaction that corrects the positions of iterators and native loops, the list of unfinished iterators); `WeakMap` and `WeakSet` are `weak.rs` and `WeakRef` and `FinalizationRegistry` are `weak_ref.rs`, with their data (the weak tables over the same hash table, the cells of a registry) in `object/weak.rs`; the strings are `string.rs` (statics, character access, `concat`, `repeat` and the padding as ropes, slices, trimming, the `Builder` that copies with the length and heap checks), `string_search.rs` (the searches: a scan for the first unit for short needles, Knuth, Morris and Pratt for long ones, charged by the unit), `string_replace.rs` (`replace`, `replaceAll`, `split` and `GetSubstitution`, which the `RegExp` methods call), `string_match.rs` (`match`, `matchAll` and `search`, which dispatch to the symbol methods), `string_iter.rs`, `string_annexb.rs`, `string_unicode.rs` (case mapping, `normalize` and `localeCompare` in slices that check the length and heap limits and charge the time) and `uri.rs`; `RegExp` is `regexp.rs` (the constructor with `RegExpCreate` and `RegExpInitialize`, the accessors of `RegExp.prototype`, `exec`, `test`, `toString`, `compile`, `RegExp.escape`, the evaluation of literals: the program is compiled at the first evaluation and kept in the literal's constant), `regexp_exec.rs` (`RegExpBuiltinExec`, `RegExpExec`, the result arrays, and the driver that runs the matcher of `js-regexp` in slices with the time check, a safepoint and the memory bound between them), `regexp_symbol.rs` (`@@match`, `@@matchAll` with the RegExp String Iterator, `@@search`, `@@split`), `regexp_replace.rs` (`@@replace`), `regexp_legacy.rs` (the static properties `RegExp.$1` and the others, as V8 has them) and `regexp_fast.rs` (the shapes that prove that the steps of the specification cannot be observed, for the shortcuts), with the data of the objects and of the realm in `object/regexp.rs`; `JSON` is `json.rs` (`parse` and `rawJSON`), `json_parse.rs` (the text parser: a loop over a vector of open containers, V8's `SyntaxError` messages, the parse records of the source text access), `json_revive.rs` (the reviver walk: a loop over a vector of levels) and `json_stringify.rs` (the serializer: a loop over a vector of levels, one flat output buffer, the cycle check, V8's message for a circular structure); `BigInt` is `bigint.rs` (the function, `asIntN`, `asUintN` and the prototype methods), with the operators on heap values, `ToNumeric`, `ToBigInt`, `ToBigInt64`, `ToBigUint64`, `StringToBigInt` and the comparisons with Numbers and strings, all charged by the cost of `js-text`, in `vm/bigint.rs`, and the literals, their key texts and the size check before the compile in `compiler/bigint.rs`; `ArrayBuffer` is `array_buffer.rs` (the constructor with `maxByteLength`, `slice`, `resize`, `transfer`, `transferToFixedLength`, `isView`, `@@species`, the element conversions that the views share, and `DetachArrayBuffer` for the host) and `DataView` is `data_view.rs` (the constructor, the accessors, `get` and `set` of the eleven element types), with the bytes outside the arenas in the heap accounting, the view records, the element types and their byte encodings, and the bounds checks of views on resizable buffers in `object/buffer.rs`; the typed arrays are `typed_array.rs` (`%TypedArray%` with `from`, `of` and `@@species`, and the 12 constructors), `typed_array_create.rs` (the forms of the constructors, `TypedArraySpeciesCreate`, `TypedArrayCreateFromConstructor`, `ValidateTypedArray`), `typed_array_proto.rs` (the accessors, `set`, `subarray`, `slice`, the iterators, and the key order of all methods), `typed_array_method.rs` (the receiver checks with V8's messages that the methods share), `typed_array_callback.rs` (`forEach`, `every`, `some`, the `find` family, `map`, `filter`, `reduce`, `reduceRight`), `typed_array_search.rs` (`at`, `includes`, `indexOf`, `lastIndexOf`, `join`, `toLocaleString`: the searches compare the bits of the elements in chunks), `typed_array_change.rs` (`fill`, `copyWithin`, `reverse`, `toReversed`, `with`) and `typed_array_sort.rs` (`sort`, `toSorted`: a counting sort and a chunked merge of keys without a comparator, a top-down merge sort of a permutation with one), `atomics.rs` (`Atomics` on integer typed arrays over `ArrayBuffer`s) and `typed_array_base64.rs` (`Uint8Array.fromBase64`, `fromHex`, `toBase64`, `toHex`, `setFromBase64`, `setFromHex`), with the slots, `CanonicalNumericIndexString` and the parts of the exotic internal methods that run no script code in `object/typed_array.rs` (`object/ops.rs` dispatches to it) and the bulk access to the bytes of the elements in `object/typed_window.rs` and the conversions of `TypedArraySetElement` and the `BigInt` elements in `vm/typed_array.rs`; `Date` is `date.rs` (the constructor, `Date.now`, `Date.parse`, `Date.UTC`, the receiver checks and the conversions between UTC and local time), `date_get.rs`, `date_set.rs` (the setters in the ES2025 order of the conversions) and `date_string.rs` (`toString` and the other text forms, `toJSON`, `@@toPrimitive`, the en-US `toLocale*String`), over `date/` (ADR 0027): `time_value.rs` (the calendar of §21.4.1 in integers, exact over the whole range, and `MakeDay`, `MakeTime`, `MakeDate`, `TimeClip`), `zone.rs` (`TimeZone` over the `jiff` library: the offset of a UTC time and the UTC time of a local time with the rules for skipped and repeated times, the zone of the system found at the first use, a POSIX rule or an IANA name, 400-year eras for the years beyond `jiff`'s range, the zone name of `toString` with Chromium's equivalent-year rule for the saving time), `format.rs` (the text forms), `parse.rs` (the date time string format and the printed forms), `iso_variants.rs` and `written.rs` (the other forms of `Date.parse`: ISO 8601 variants, RFC 5322, RFC 850, asctime, English and numeric dates), `zone_names.rs` (the long English zone names and the GMT format) over the generated `zone_names_data.rs` (CLDR 48, `swbtools js-time-zone-names`) and the host hooks for the clock and the zone (`Runtime::set_clock`, `Runtime::set_time_zone`; the shell options `--now` and `--time-zone`); the generator of `Math.random` is `vm/random.rs`, the time charges of number conversions `vm/number.rs`) and the embedding API (`runtime.rs`; the host runs the jobs with `run_jobs`, which ends with `ClearKeptObjects`, takes the unhandled rejections, and runs the finalization cleanup jobs with `run_finalization_cleanup`; `host/` has the parts for the Web API bindings, ADR 0028 section 6: the registry of host classes with their parents and the brand check (`host/mod.rs`), the interface objects and prototypes per realm, the member definitions and the host-defined global object (`host/interface.rs`), the hooks for the internal methods of host objects of classes with hooks, which the generic internal methods call as they call the traps of a proxy, with the messages of refusals (`host/hooks.rs`), immutable prototype objects marked by the host (`Runtime::set_immutable_prototype`, `vm/internal.rs`), event handler functions compiled with a scope chain of object environments (`host/handler.rs`), the ECMAScript operations that the Web IDL conversions use (`host/ops.rs`), the host state that the collector traces as a root, and the termination of a task by a native function; the slot of a host object, its class and its host data (a word or traced generic data), is `object/host.rs`). The `swb-js` binary (`main.rs`, `shell.rs`) has a shell, the `test262` runner (`test262/`), `bench-compile` (`bench_compile.rs`) and `--dump-ast`. | `js-text`, `js-syntax`, `js-regexp` |
| `web`        | The Web API bindings (ADR 0028 sections 1, 4, 5). Feature 4b: `bindings/` is the support code of the generated glue: the binding context `Cx`, the `Host` trait that the engine side implements (node wrappers, content attributes for `[Reflect]`, the glue state), the conversions of Web IDL §3.2 in both directions (`convert.rs`, `to_js.rs`), exceptions with Chromium's messages and their context prefix (`exception.rs`), `DOMException` (`dom_exception.rs`), the overload selection (`overload.rs`), `[Reflect]` (`reflect.rs`) and the shared steps of members, `[LegacyUnforgeable]` templates, `[Replaceable]`, `[PutForwards]` and callbacks (`glue.rs`). Feature 4c1: the internal methods of legacy platform objects (indexed and named properties, Web IDL §3.9) as host hooks, with Chromium's messages of refused writes (`legacy.rs`), and iterable declarations: the `%Array.prototype%` functions of value iterators, the default iterator objects and iterator prototypes of pair iterators (`iterable.rs`). `generated/` is the glue that `swb-webidl` writes from `idl/` (checked in; one module per interface with the trait of its hand-written behaviour, `types.rs`, `mod.rs` with the class numbers and the registration). Feature 4c2: the runtime of a document (`runtime.rs`: `ScriptRuntime` creates a `js::Runtime` with the generated classes, `WindowProperties` and the `Window` global, runs classic scripts as tasks with the time limit and a microtask checkpoint, evaluates expressions into JSON with a serializer compiled before any page script, and keeps the console buffer), the trait `EngineHost` through which the engine lends its document state (`host.rs`), the binding state with the wrapper vector and the side table of `[SameObject]` objects (`state.rs`), the DOM interfaces (`dom/`; feature 6a: `Node` and the shared helpers in `mod.rs`, the tree changes and the `ParentNode` and `ChildNode` methods with Chromium's exceptions in `tree_ops.rs` (the members of the mixins come from the macros of `macros.rs`), `Element` in `element.rs`, the attribute algorithms, `Attr` and `NamedNodeMap` in `attrs.rs`, `classList` in `token_list.rs`, `dataset` in `string_map.rs`, `CharacterData`, `Text`, `Comment`, `DocumentType` and `ProcessingInstruction` in `character_data.rs`, `Document` with HTML's `title`, `body` and `head`, the node factories and `getElementById` in `document.rs`, the live `NodeList` and `HTMLCollection` with their caches keyed by the DOM generation in `lists.rs`), `Window` and the named properties object (`window.rs`). Feature 5a: the event core (`events/`): `Event` with its state and flags, `CustomEvent`, `ErrorEvent`, `PromiseRejectionEvent`, `PageTransitionEvent` and `document.createEvent` (`mod.rs`), the listener lists by event type with an index by callback and capture (`listeners.rs`), `EventTarget` with the lists of nodes and of the window in the binding state and those of constructed targets in their generic data (`target.rs`), and the dispatch algorithm with `composedPath`, `window.event`, listener exceptions reported, and the entry for events that the engine fires (`dispatch.rs`; `ScriptRuntime::dispatch_event`). Feature 5b: the event loop (`event_loop.rs`: the task queue by due time, the microtask checkpoint after tasks and after the listeners of the engine's events, the notification of rejected promises, finalization cleanups), the timers, `queueMicrotask` and `reportError` (`timers.rs`), HTML "report an exception" with `error` and `unhandledrejection` events at the window (`errors.rs`), the page clock with real and virtual time and the document's time origin (`clock.rs`), `performance` (`window.rs`), and in `runtime.rs` the task runner with the stop rules, the budget of long tasks and the evaluation of promises that settle later. Feature 5c: script elements (`scripts.rs`: their state, "prepare", "mark as ready", "execute", the steps after insertions, `HTMLScriptElement`, the JavaScript MIME types) and the parse and life of the document (`document_load.rs`: the parser driven through `EngineHost::parse_step` with its waits for scripts and style sheets, parser tasks, "the end" with the deferred scripts, `DOMContentLoaded`, `load` and `pageshow`, the unload), `readyState`, `currentScript`, `createElement` and `appendChild`. Feature 6b: the selector queries (`dom/queries.rs`: `querySelector`, `querySelectorAll`, `matches`, `closest` with `swb_style::Matcher` and the parsed selectors cached by text; the states come from `EngineHost::element_states`), the live lists of `getElementsBy*`, the document collections (`forms`, `scripts`, ...) and the static list of `querySelectorAll` in `dom/lists.rs`, markup and `insertAdjacent*` in `dom/markup.rs` (the serializer and the fragment parser are in `dom`), and `DOMImplementation`, `DOMParser` and `template.content` in `dom/implementation.rs`; `window[name]` with several elements gives a live collection. Feature 6c: the HTML element interfaces (`html/`: the interface of an element by tag name, `classes.rs`; `HTMLElement` and the elements with members of their own, `element.rs`; the URL members of `a` and `area`, `hyperlink.rs`, with the `url` crate; `relList`, `sizes`, `sandbox` and their supported tokens, `token_lists.rs`; form owners, `owner.rs`; `input`, `form_controls.rs`; `form`, `select`, `option`, `label`, `fieldset`, `output`, `textarea` and the collections of a form and a select, `forms.rs`; tables, `tables.rs`), the element lists in `dom/element_lists.rs` (`table.rows`, `select.options`, `form.elements`, ...), and the reflected attributes, which come from `idl/html.idl`; the value, the checkedness and the selectedness of controls are the engine's form state, reached through `EngineHost`. Feature 6c2: the factory functions `Image` and `Option` (`html/factories.rs`; the generator writes `[LegacyFactoryFunction]` and `js` has `Runtime::define_constructor`), and the SVG element interfaces (`svg/mod.rs`: the interface of an SVG element by local name, `className` as `SVGAnimatedString`, `ownerSVGElement`, `viewportElement`; the members come from `idl/svg.idl`, `dataset` and `tabIndex` from the code of `HTMLElement`). Feature 6d1: the event interfaces (`events/ui.rs`: `UIEvent`, `FocusEvent`, `InputEvent`, `KeyboardEvent`, `CompositionEvent`, `TextEvent` and the state of the UI family; `events/mouse.rs`: `MouseEvent`, `WheelEvent`, `PointerEvent`, `DragEvent`; `events/misc.rs`: `SubmitEvent`, `MessageEvent` and the other events that `createEvent` names), the event handlers (`events/handlers.rs`: the maps of the `on*` IDL attributes and content attributes, the compile at first need, the processing algorithm; the generator writes their glue itself), focus (`html/focus.rs`) and activation (`html/activation.rs`: the activation behaviors in the dispatch, `click()`, `requestSubmit()`, `submit()`, `reset()`); the engine's focus, link and form state is reached through `EngineHost`. Feature 6d2: the events of the user's input (`events/user.rs`: `UserEvent` describes a mouse, pointer, wheel, keyboard or input event that the engine fires, and `dispatch_trusted` makes the object with Chromium's values; a `wheel` event is cancelable only if a listener on its path is not passive), the tasks that the engine's input needs (`ScriptRuntime::user_focus`, `window_focus`, `commit_change`, `simulated_click`, `implicit_submit`), the `summary` toggle and the `javascript:` URLs of links and form actions in the activation behaviors. Feature 7a: `cssom.rs`, `element.style` (`CSSStyleDeclaration` over the `swb_style::cssom` block of the `style` attribute, one block per element kept with the attribute text it stands for; the property names as named properties, `[SwbPropertyNames]`) and the `CSS` namespace (written by hand); feature 7a2: `getComputedStyle`, the same interface with a computed target that asks `EngineHost::resolved_value` at each read. Feature 7b: `cssom_view.rs`, CSSOM View (geometry and scroll members of `Element`, `HTMLElement`, `Document` and `Window`, `Screen`, `ScreenOrientation`) and `DOMRect`, `DOMRectReadOnly`, `DOMRectList` (`idl/cssom-view.idl`, `idl/geometry.idl`), through the geometry methods of `EngineHost`; `offsetX` and `layerX` of the mouse events that scripts dispatch (`events/mouse.rs`). | `js`, `dom`, `css`, `style` |
| `webidl`      | A development tool, not a dependency of the browser: the parser of the WebIDL subset of ADR 0028 section 5 (`lexer.rs`, `parser.rs`, `ast.rs`; errors with file and line), the checked model of a set of files (`model.rs`: partials and mixins merged, typedefs resolved, extended attributes checked, classes ordered parents first; `legacy.rs`: the special operations of each interface with the inherited ones, and the checks of special operations and `iterable`) and the generator of the glue (`emit/`; `emit/legacy.rs` the hooks of legacy platform objects and the natives of pair iterators). The binary (`just bindings`) writes the sets of `binding_sets`; a test checks that the checked-in output is fresh. | — |
| `swb`        | The binary: CLI, window, browser UI, clipboard, headless runner, benchmark.    | `engine`, `automation`, `net`, `paint`; `dom`, `layout`, `style`, `text` for the toolbar and debugging dumps |

Rules:

- Lower crates must not depend on higher crates. `css` and `text` do not know
  about the DOM. `css` matches selectors through a trait that `style`
  implements for DOM elements.
- Only `swb` depends on windowing and clipboard libraries.
- The `js-*` crates and `js` depend on no other swb crate (ADR 0026); the
  bindings to the DOM are in `web`, whose glue `webidl` generates (ADR
  0028).
- Only `net` does network I/O to load pages. `automation` listens on a
  local socket for the remote-control protocol.
- Lengths and `calc()` exist twice, on purpose: `css` (`media.rs`)
  resolves media query and `sizes` lengths when it parses them, with the
  media environment; `style` (`values/length.rs`, `parse/length.rs`)
  keeps property lengths and `calc()` expressions until computed values.
  Each has its own unit list; keep them in step.
- HTML attribute microsyntaxes (integers, non-negative integers, floats)
  are in `dom` (`microsyntax.rs`), for all crates.

## Key data structures

- **DOM** (`dom`): nodes are stored in a `Vec` (the arena, type
  `Document`) and addressed by `NodeId` (a `u32` index; `Option<NodeId>`
  takes four bytes). Nodes link to parent, siblings and children by
  `NodeId`, and to their node document. Nodes are never freed. This makes
  the tree easy to traverse without reference counting. It also gives a
  stable handle for the automation API and for JavaScript. The parser
  limits the tree depth to 512 (as Blink does), because later stages
  recurse over the tree. ADR 0028 section 3 adds what scripts need:
  - *More documents in one arena:* document and fragment nodes besides
    `NodeId::DOCUMENT` (`create_document`; template contents belong to an
    inert template contents owner document). Rendering uses only the tree
    of `NodeId::DOCUMENT`.
  - *Change functions* (`mutation.rs`): the only code that changes a tree,
    an attribute or text data after a node is created. They follow the
    DOM's "insert", "remove", "replace", "replace all", "adopt", "change
    an attribute" and "replace data", check its preconditions (`DomError`),
    and report each primitive change: the generation grows, the journal
    gets an entry, and the installed mutation sink is called.
  - *Journal* (`journal.rs`): the changes since the engine last drained it
    (`take_journal`), in the form of DOM mutation records (parent, added
    and removed nodes with their siblings; attribute name and old value;
    the text node). It is incomplete (the consumer treats everything as
    changed) after an overflow (1,048,576 entries) or while recording is
    off; `parse_html*` parses without recording (recording costs 2 to 3 %
    of the parse time), `HtmlParser` records.
  - *Mutation sink* (`MutationSink`): a trait object that the owner of the
    document installs (`set_mutation_sink`; mutation observers later). It
    gets each change at the moment it happens, with the document in its new
    state, also the parser's changes.
  - *Id index* (`id_index.rs`): `id` values of connected elements, built at
    the first lookup and then kept up to date by the change functions;
    `get_element_by_id` picks the first in tree order among the elements
    of one id.
  - *Resumable parser* (`HtmlParser`): drives html5ever's tokenizer over a
    `BufferQueue`; each `resume` parses until a `script` end tag or the end
    of the input. The caller keeps the document and lends it to each step.
    With the scripting flag, it also gives the line and column where the
    script's text starts (`script_position`), for error positions.
    `parse_html*` run it to the end. `parse_fragment` parses with a context
    element into a temporary arena and copies the nodes into a new
    fragment.
  - *Limits* (`Limits`, ADR 0028 section 16): at most 1,048,576 nodes and
    256 MiB of text, names and attributes per arena. The change functions
    return `DomError::NodeLimit` or `ByteLimit`; the parser drops the rest
    of its input with a warning.
- **Computed style** (`style`): one `Arc<ComputedStyle>` per element. Elements
  that share a style share the `Arc`. `StyleMap` also holds each list
  item's ordinal value; after the cascade, `counters.rs` replaces the
  counters in pseudo-element `content` with text (ADR 0007).
- **Box tree** (`layout`): built from the DOM and computed styles. It contains
  anonymous boxes (for example, anonymous block wrappers around inline
  content).
- **Fragment tree** (`layout`): the output of layout. It contains positioned
  boxes, line boxes and glyph runs in CSS pixels. It is immutable. The engine
  rebuilds it when the input changes.
- **Display list** (`paint`): a flat list of drawing commands (rectangles,
  borders, glyph runs, images, linear gradients, polylines, polygons,
  the filled and stroked paths of inline SVG and their hit areas, clips,
  opacity, mask, SVG clip path and transform groups with their bounds) in paint order. A mask group carries its mask layers: images or gradients
  positioned like background layers
  (`paint/src/mask.rs`, ADR 0018). The builder collects items in chunks
  (`paint/src/rope.rs`), so moving the items of positioned boxes copies
  each item a bounded number of times, and computes the bounds of all
  groups in one pass at the end (`paint/src/group_bounds.rs`). Form
  controls with the native look are drawn by `paint/src/control.rs`. The
  rasterizer consumes it. The rasterizer can be replaced without changes
  to layout. The list also
  contains hit regions, so hit testing finds what is painted on top.
- **Text fragments** (`layout`): a glyph run on one line, with caret
  stops: the offset in the DOM text node and the x position of each glyph
  cluster boundary. White-space processing records a source map from the
  processed text to the node's data (`layout/src/source_map.rs`).
- **Selection** (`engine`): a range between two positions in text nodes
  (node and byte offset). It refers to the DOM, so it survives a new
  layout. The tree order of nodes for comparisons is derived data of the
  document state (built when a selection needs it, again after a DOM
  change).
- **Element states** (`engine` → `style`): hovered, active, focused and
  target elements. A change restyles only if a selector depends on that
  state; the layout stays if no style changed (ADR 0009).
- **Scroll offsets** (`engine`, ADR 0019): one per scroll container,
  keyed by `NodeId`, in whole CSS px, clamped after each layout. Layout
  stores each scroll container's scrollable overflow in
  `BoxFragment::scrollable_overflow` (`layout/src/scroll.rs`);
  `swb_layout::ScrollState` applies the offsets in tree walks, paint
  and hit testing. A scroll rebuilds only the display list.
- **Session history** (`engine`): one entry per committed document or
  fragment, with the scroll position to restore. A navigation is pending
  until its document arrives; starting a navigation cancels all running
  requests (`Loader::cancel_all`), and only a committed navigation adds a
  history entry. Entries record their document number, the initiator of
  the request and, for `POST` results, the body.
- **Form controls** (`engine/src/forms/`, ADR 0013): per-document `Forms`
  keyed by `NodeId` (value as `TextEdit`, checkedness, select options,
  form owner, a dirty flag for what the user changed). Layout gets them
  through `LayoutInput::controls`; style gets `:checked`,
  `:placeholder-shown`, `:valid` and `:invalid` through
  `ElementStates::controls`. The disabled state of all elements comes
  from one tree walk (`DisabledElements`, linear time). After a DOM change
  the document state rebuilds `Forms` for the new generation
  (`Forms::rebuild`): owners, disabled states, options and defaults are
  computed again, a dirty control keeps its state.
- **Natural sizes** (`layout`): `NaturalSize` has an optional width,
  height and aspect ratio (SVG images, videos without a loaded poster and
  audio can lack any of them). Replaced elements and backgrounds use the
  CSS default sizing rules; the default object size is 300×150. The
  natural size of an `<img>` is divided by the pixel density of its
  chosen `srcset` candidate.
- **SVG images** (`paint/src/svg/`, ADR 0011): usvg and resvg behind
  limits that are checked before conversion: source size, XML entities,
  XML nodes and depth, style sheets (`css.rs`), the size of the render
  tree with copies, reference depth and cycles (`expansion.rs`). An
  estimate of the rendering time and memory (`cost.rs`, with the cost of
  dense paths and of spans narrower than a pixel from `path_cost/`, see
  below) sets the resolution. SVG images load only
  `data:` URLs. They are rendered at the device pixel size of each tile
  and cached per document (`VectorCache` in the engine's `Images`,
  128 MiB). One frame renders new renderings up to a work budget; after
  that, images use the closest cached rendering or are not drawn until a
  later repaint. Counting the spans of a decoded SVG image draws on a
  budget per document (`CountBudget` in `Images`, 2 billion units, ADR
  0023 part 3); past it, images use the bound without counting.
- **Rounded overflow clips and image reduction** (ADR 0024): a box with
  `overflow` clipping and `border-radius` clips to the rounded padding
  edge. The display list emits it as an SVG clip group (`PushSvgClip` with
  a rounded-rectangle `ClipPath`, `rect_fallback` set), which the
  rasterizer draws on a layer and multiplies by the coverage
  (`paint/src/raster/svg_clip.rs`). The layer and the coverage count
  against the layer budget and the path work budget. When they do not fit,
  the group draws its content clipped to the rectangle (square corners)
  instead of hiding it. A raster image drawn at a scale below 0.5 is drawn
  from an averaged mip level (`paint/src/reduce.rs`): 2x2 averaging per
  axis until the scale is at least 0.5, then bilinear. The levels are kept
  with the decoded image (at most its own size) and made lazily. One frame
  averages at most 64 Mpx of source pixels; past that, images use the
  nearest level that exists, and a later repaint continues.
- **Inline SVG** (ADR 0023): an `svg` element in HTML is a replaced box
  (`style::is_replaced_element`); its natural size comes from its
  `width`, `height` and `viewBox` attributes, which are also
  presentation attributes for `width` and `height`. Style computes the
  fill and stroke properties for all SVG elements and maps their
  presentation attributes as author-level hints
  (`style/src/svg_attributes.rs`). When the box tree is built,
  `layout/src/svg/` walks the descendants once into an `SvgContent`
  (groups and shapes with their computed styles; `path`, `polyline` and
  `polygon` data parsed into user units with `svgtypes`; `use` as groups
  around a copy, whose styles inherit from the `use` element:
  `style/src/cascade/use_instances.rs`; `clip-path` references to
  `clipPath` elements, `layout/src/svg/clip.rs`). The descendants get no
  boxes of their own: `FragmentTree::element_boxes_scrolled` adds the
  bounding boxes of `g`, shapes and `use` that `SvgContent::element_boxes`
  computes. `BoxContent::Svg` carries the content to paint, which asks it
  for its drawing commands for the content box size (percentages and the
  `viewBox` transform depend on it: clip regions, `FillPath` and
  `StrokePath` items with a matrix from user units and the hit areas of
  shapes, `paint/src/inline_svg.rs`). A clip path that is not a clip
  rectangle becomes a `PushSvgClip` group, which the rasterizer multiplies
  by the coverage of the clip shapes (`paint/src/raster/svg_clip.rs`).
  `DisplayList::hit_test` finds shapes by their geometry
  (`paint/src/hit_path.rs`, within a work budget per hit test), so an SVG
  `a` works as a link. The rasterizer draws paths with tiny-skia within a
  work budget per strip (`paint/src/raster/path.rs`; the cost of a path
  includes the rows that its edges cross and the pairs of edges whose
  boxes overlap, `paint/src/path_cost/edges.rs`, and for anti-aliased
  fills the spans narrower than a pixel, `paint/src/path_cost/spans.rs`,
  ADR 0023 part 3; the weights are in `paint/src/path_cost/mod.rs` and
  the estimate for SVG images uses the same model). Limits per
  document: 50,000 shapes and 50,000 groups, 1,000,000 path segments,
  groups 64 deep, 256 opacity layers and 256 clip layers, 20,000
  elements in the copies of `use` elements.
- **Web fonts** (ADR 0022): the stylist keeps the `@font-face` rules
  (`swb_style::FontFace`) with their `@media` chain. The engine gives the
  applicable faces to the `FontContext` (`set_web_fonts`), which owns the
  face set: families, composite fonts by `unicode-range`, the load state
  of each face and the requests that layout makes
  (`text/src/web.rs`). After layout the engine takes the requests and
  loads the sources (`engine/src/document/loads.rs`; each URL once,
  `engine/src/web_fonts.rs`); `text/src/decode.rs` decodes WOFF and
  WOFF2 (`wuff`) within size limits. A font that arrives invalidates the
  layout. A font is a face with synthesis flags and variation axis
  values (`FontId`); caches key on it. The axis values are the ones that
  matching sets, then the `@font-face` descriptor, then
  `font-variation-settings` (the text crate interns the list). A
  `local()` source asks the font source for a face by full name or
  PostScript name and loads at once. `size-adjust` and the metric
  overrides are properties of the loaded face: the text crate scales the
  size for shaping, metrics and glyph masks, so layout is unchanged.
  `font-feature-settings` go to the shaper (`ShapeOptions::features`).
- **Cookie jar** (`net`): one `CookieJar` per `NetworkFetcher` (so one per
  swb process), inside the HTTP client. It adds `Cookie` to every HTTP
  request and stores `Set-Cookie` on every redirect hop. Cookies are
  grouped by registrable domain (Public Suffix List, `net/src/site.rs`).
  Each `Request` carries its `initiator` (the origin of the document that
  started it); with `Destination::Document` (a top-level navigation) this
  gives the `SameSite` context. In memory only; fixture replay has no
  cookies (ADR 0012).

## Units

Layout uses `f32` CSS pixels. Paint multiplies by the device pixel ratio
(HiDPI scale factor) only at rasterization.

## Threads

- The engine runs on one thread.
- `net` runs fetches on a pool of worker threads. Results return to the engine
  through a channel. The engine processes them when it is polled. The
  workers share the cookie jar (a mutex).
- The GUI shell runs the window event loop on the main thread and calls the
  engine. Network completions wake the event loop. After each batch of
  window events it calls `Page::pump` (the page's event loop: network
  completions and the due tasks of the document's scripts) and waits with
  `ControlFlow::WaitUntil` until the next task is due, or for input; it
  never busy-waits.
- The automation server accepts connections on its own thread and runs one
  thread per connection. Each request goes to the page thread through a
  channel and wakes it (the GUI through its event loop proxy, the headless
  runner through its wake-up channel); the response comes back through a
  reply channel. `page.waitForLoad`, `page.waitForIdle` and `page.evaluate`
  of a promise stay pending on the page thread until their condition holds
  or their timeout expires. The headless server calls `Page::pump` in its
  loop and sleeps until the next task, a request or a network completion.
- The headless runner and the headless automation server run the page on a
  thread that swb creates with a 16 MiB stack (`swb/src/stack.rs`). The GUI
  runs the page on the main thread and reads the stack limit of the process
  at start: below 8 MiB it keeps JavaScript off (ADR 0028 section 2).

## Page and document state

`Page` (`engine/src/page/`) is one browsing context: the session history,
the pending navigation, the loader (shared through an `Rc`), the font
context, the viewport and the state of the pointer and the press. The
current document is a boxed `DocumentState` (`engine/src/document/`, ADR
0028 section 2): the DOM and its parser, the stylesheets and the stylist,
the styles, the fragments and the display list, the images and web fonts
with their requests, the form controls, the scroll offsets, the selection
and the element states. Style, layout, display list, raster and the
queries (geometry, scroll state, hit testing, rendered text, selector
queries) are methods of `DocumentState`; those that lay out take the font
context as an argument. The state is `'static` and borrows nothing from the
page, so a script runtime can hold the box for a task
(`Page::take_document_state`, `return_document_state`). The
load state of the page is derived: `LoadingResources` while the document
state has requests or a parse in steps.

Scripts (`page/script.rs`, ADR 0028 section 2): a document that commits
with JavaScript on gets a `swb_web::ScriptRuntime`, which the page drops
with the document. For each task the page lends the boxed state to the
runtime as a `Box<dyn EngineHost>` and takes it back (a downcast), which
drains the journal of the task. Feature 5c (ADR 0028 section 8): the
runtime drives the parse of its document (`EngineHost::parse_step`, one
step per `script` end tag), runs inline scripts at their place, waits for
parser-blocking scripts and for the style sheets that block scripts, and
continues in parser tasks of its queue; then "the end": deferred scripts,
`DOMContentLoaded`, the scripts that run as soon as possible. The page
fetches the scripts of script elements (`document/scripts.rs`: at most 64
in flight, the rest queued; status and MIME checks; decoding) and hands
each result to the runtime (`Page::deliver_scripts`), tells it when style
sheets arrive, and fires `load` and `pageshow` when the runtime is ready
and nothing delays the load event: no request runs, also after a layout,
which finds background images and fonts (`Page::check_load_event`). A
document with a runtime is loaded when its `load` event fired
(`Page::is_fully_loaded`, `page.waitForLoad`); requests that scripts
start later do not count. Leaving a document fires `pagehide` and
`unload`. Feature 5a adds
`Page::dispatch_event`, which fires a trusted DOM event at a node or the
window as a task (feature 6d2: the engine's input events use it). Feature 5b adds the event loop
(ADR 0028 section 7): the runtime keeps a task queue (timers, parser tasks,
the notification of rejected promises, finalization cleanups) ordered by due
time in page time; `Page::pump` runs the due tasks for at most 16 ms and
returns when the next is due; `run_event_loop` (`--settle-ms`),
`wait_until_idle` (`page.waitForIdle`) and the wait of an evaluation for
its promise block and run the loop themselves; `wait_until_loaded` runs
due tasks while the page loads. The page owns a `PageClock` (real or
virtual time, an optional fixed start from `--now`); in virtual time only
the blocking loops move it, to the next task, when nothing runs and no
request is pending. A runtime that reached the heap limit is dropped after
its task; the DOM stays, and the parse goes on to the end without scripts. A
page may run scripts only if its configuration says that the thread that
runs it has a big enough stack (`PageConfig::scripts_allowed`); otherwise
`set_javascript` keeps JavaScript off.

Parsing: with JavaScript off, `parse_html*` parses to the end without a
journal, and the document state walks the new document once. With
JavaScript on, the page parses with `HtmlParser` in steps (one per
`script` end tag; the script runtime drives them, or, without a runtime,
the page runs them to the end) and the document state drains the journal
after each step.

The journal drain (`DocumentState::process_changes`, `document/changes.rs`)
runs after each parser step, after each DOM change through the change API
(`Page::change_dom`, `DocumentState::change_dom`, the return of a lent
state) and before each flush. It reconciles the elements that the changes
touched, in tree order: a `style` or `link` gets the sheet that its
attributes ask for (a moved one keeps its sheet; a removed one loses it),
an `img` or a `video` its image or poster, so loads start at insertion; a
`base` or `title` change rebuilds the base URL or the title. Any change
marks the styles dirty, if it is in the document (feature 7a: changes in
detached subtrees and template contents do not); a sheet change or a new
base URL marks the stylist dirty. A drain with an unchanged DOM generation
does nothing, and style and layout run only when a frame or a query needs
them (full restyle and full layout in M8). A query of a script that needs
one element's style while the styles are outdated
(`DocumentState::query_element_style`: the focusability check of
`focus()`) computes that element and its ancestors only
(`swb_style::compute_element_styles`) and keeps them until the next
change. `getComputedStyle` (feature 7a2, `document/resolved.rs`) reads
the resolved value of a property through `EngineHost::resolved_value`:
a style value needs that style query, a value that layout resolves a
layout flush in the task. For that flush the page lends its font context
with the state (`take_document_state` moves it into
`DocumentState::lent_fonts`, `return_document_state` back), and the used
boxes of all elements are indexed in one walk of the fragment tree after
each layout. The geometry of scripts (feature 7b, `document/geometry.rs`:
`getBoundingClientRect`, `offset*`, `client*`, `scroll*`, the scrolls of
scripts, `elementFromPoint`) uses the same flush and two indexes: the
boxes of the layout (per layout serial) and the client rectangles as
painted (per layout serial and scroll state). The scrolls of the viewport
and of scroll containers are recorded in `DocumentState::scrolled`, in
their order, and the page's next frame fires `scroll` at each target.
`DocumentState::flushes` counts the style and layout runs of the
document and their time, and apart the runs that script queries forced
(`query_element_style`, `query_layout`); `--bench` and `page.info` show
them. An incomplete journal (the parse without scripts,
an overflow) reconciles every element in one walk.

Caches: the stylist keeps each sheet parsed (`SheetSlot::parsed`), so a
rebuild of the stylist after a change builds only the rule index again
(the benchmark's `restart_pipeline` drops the parses too). Derived data
records the DOM generation it was built for (`document/derived.rs`,
`Derived<T>`): the base URL and the title (kept current by the drain; a
drain that did not touch them confirms them), the form controls (rebuilt
at the next flush or Page-level drain) and the tree order (rebuilt when
the selection needs it).

## The JavaScript setting

`Page` holds the setting (`set_javascript`, off by default; command line
`--js`, automation `page.setJavaScript`, GUI toggle). A new document copies
it when it is created: the parser gets it as the scripting flag
(`<noscript>` is raw text) and the `Document` keeps it (`Document::scripting`,
set at parse time, not changed afterwards). Later stages read the flag from
the document, not from a global, so two pages in one process can differ: the
media environment (`scripting`), the cascade (`noscript` is `display: none`),
the serializer, layout (video controls, `<canvas>` as a replaced element) and
the `canvas` size hint. No script runs yet. See ADR 0028 section 15.

## Layout details

- The box tree (`layout/src/box_tree.rs`) is rebuilt for every layout.
  Inline content is a flat list of items (text, inline box start/end,
  atomic inlines, line breaks); white space is collapsed when the inline
  formatting context is finished, because spaces collapse across inline
  box boundaries. Generated text (`content`, list markers) is limited to
  1 MiB per box tree (`MAX_GENERATED_TEXT`, ADR 0007).
- `details` (HTML §15.5.5): style computation gives every `details` a
  `::details-content` pseudo-element style (`PseudoKind::DetailsContent`)
  and, without a `summary` child, a default summary style
  (`DetailsSummary`: a list item with the text "Details"). Box
  construction puts all children except the first `summary` into the
  `::details-content` box. While `details` is closed, its style has
  `contents_hidden` (Chromium's `content-visibility: hidden` on that box):
  the box is a block formatting context and the containing block of
  absolutely positioned boxes, has no height, intrinsic size or baseline,
  and does not add to scrollable overflow. After layout, `hide_contents`
  moves its children to `BoxFragment::hidden`: paint, hit testing, the
  selection and scrolling see only `children`, while the box dump
  (`FragmentTree::element_boxes`) also walks `hidden`. The engine toggles
  the `open` attribute on a click on the summary or Enter/Space on it
  (`page/forms.rs`, through `Page::change_dom`, so the journal drain
  restyles), and skips elements in closed contents in
  the focus order (`Document::in_closed_details_content`).
- Flex layout (`flex.rs`) follows CSS Flexbox 1 §9: lines, flexible
  lengths, cross sizes, `align-content`, first baselines and automatic
  minimum sizes. Item layouts are cached in `LayoutContext::layouts`.
  `align.rs` resolves `auto` self-alignment and the edge an item aligns
  to, for flex, grid and positioned layout.
- `replaced.rs` sizes replaced elements (CSS 2.2 §10.3.2, §10.6.2, with
  the min/max rules of §10.4); `intrinsic.rs` computes min-content and
  max-content widths for shrink-to-fit boxes, flex base sizes, grid
  tracks and tables. Margin collapsing (CSS 2.2 §8.3.1) uses the
  `CollapsedMargin` type in `collapsed_margin.rs` (derived from Servo,
  MPL-2.0, ADR 0021).
- `aspect.rs`: the `aspect-ratio` of blocks, flex containers and grid
  containers (CSS Sizing 4 §5; replaced elements stay in `replaced.rs`).
  An `auto` axis takes its size from the other axis through the ratio,
  which applies to the content box or, with `box-sizing: border-box`, to
  the border box. Min and max sizes transfer with the table of CSS 2.2
  §10.4. A box whose height came from the ratio grows to its content
  unless it is a scroll container or has an explicit `min-height`; the
  ratio-derived height is definite for percentage heights of children.
  Block, shrink-to-fit, flex, grid and absolutely positioned layout each
  call into it where they resolve a width or a height; `intrinsic.rs` uses
  the width from a definite height as the content size.
- Inline layout shapes the text of an inline formatting context once
  (cached per layout pass), splits it into pieces at soft wrap
  opportunities, groups pieces into unbreakable groups, fills lines
  greedily, and then builds each line: vertical alignment, line box height,
  fragments. Font metrics and leading are rounded as Blink does, so line
  heights match Chromium. Text is shaped across inline element
  boundaries when the font does not change; shaping breaks at box edges
  as measured in Chromium. Break opportunities come from
  `text/src/linebreak.rs` (UAX #14 with tailorings measured in Chromium,
  tables generated from the measurements) and from
  `layout/src/inline/breaks.rs` at element boundaries (`white-space` at
  box ends, isolating boxes). A group that does not fit on a line by
  itself is cut at grapheme boundaries when `overflow-wrap` or
  `word-break` allow it (`inline/overflow.rs`). Text item widths are
  rounded up to 1/64 px; small capitals are synthesized in
  `inline/caps.rs`.
- Depth limits: the HTML parser nests at most 512 elements deep (deeper
  elements are appended to the ancestor at the limit, as in Blink); box
  construction nests at most 256 boxes deep (deeper content keeps its
  text but not its boxes). Together they bound the recursion of all later
  stages.
- Lengths are clamped to ±33,554,431 px (the range of Blink's
  `LayoutUnit`), so hostile values cannot produce infinite geometry.
- Flex item, grid item and table cell layouts are cached per layout pass
  by box and constraints (`LayoutContext::layouts`), so nested flex
  containers, grids and tables do not cost exponential time. Fragment
  children are shared (`Arc`), so a cached subtree is stored once and
  copying a fragment is cheap.
- Grid containers (`layout/src/grid/`, ADR 0017): the explicit grid of
  each axis as segments of repeated track lists (`template.rs`),
  placement and auto-placement with per-row interval occupancy
  (`placement.rs`), tracks grouped into ranges and sets as in Chromium
  (`tracks.rs`), the track sizing algorithm on sets (`sizing.rs`), and
  the grid sizing algorithm, item contributions, alignment and baselines
  (`mod.rs`). Placements and intrinsic widths are cached per layout pass
  (`LayoutContext::grids`). Lines are clamped to ±10,000.
- The overflow of the root (or of the body, if the root's is `visible`)
  applies to the viewport (CSS Overflow 3 §3.3); the source element then
  has a used overflow of `visible`.
- List markers wait for the first line box of the list item, also inside
  nested blocks and independent formatting contexts.
- Tables (`layout/src/table/`, ADR 0010) follow Chromium's LayoutNG
  algorithm: box construction with the anonymous table fixup
  (`build.rs`), the grid (`grid.rs`), column constraints and the grid's
  intrinsic widths (`columns.rs`), width distribution (`distribute.rs`),
  row heights (`rows.rs`, `cells.rs`), placement and fragments
  (`layout.rs`, `fragments.rs`), collapsing borders (`collapsed.rs`).
  The `<table>` box contains its captions; row groups, rows, cells,
  columns and column groups have fragments. `BoxContent` tells paint how
  to paint table parts: the table's background and border fill the grid;
  cells paint the backgrounds of their columns, row groups and rows; the
  table paints collapsed borders after its cells.
- `text-align: -webkit-left/center/right` aligns block-level children
  without `auto` margins (`<center>`).
- Quirks mode: the line height quirks and the table cell quirks are in
  layout (`LayoutContext::quirks`, `line_height_quirks`).
- Media elements (`layout/src/media.rs`, ADR 0020): `<video>` and
  `<audio>` are replaced boxes; layout chooses the visible parts of
  their controls (Chromium's size thresholds) and shapes the time text;
  `BoxContent::Media` carries them to paint (`paint/src/media.rs`).
  Posters load like `<img>` sources; media resources are never fetched.
- Form controls are atomic boxes with generated content
  (`layout/src/control.rs`); their sizes follow Chromium. A `<button>`
  with `display: flex` or `display: grid` lays out its DOM content as an
  ordinary flex or grid container (`grid::layout_contents`); its flow
  content is centered vertically.
- `<br>` elements and inline boxes around block-level children get boxes
  for the box dump; the latter are `BoxContent::GeometryOnly`.
- Absolutely positioned boxes (`layout/src/positioned.rs`, ADR 0016)
  get a placeholder fragment at their static position (block, inline,
  flex and grid layout). After layout, one walk lays each box out in its
  containing block (the nearest positioned or transformed ancestor; a
  positioned inline box from its first to its last fragment) and
  replaces the placeholder, so fragments stay relative to their parents
  and in tree order. The walk then recomputes the scrollable overflow of
  scroll containers that contain placed boxes. Sticky offsets and
  transforms do not change layout: paint applies them (`Ancestry`,
  `GroupTransform`), and `FragmentTree::element_boxes_scrolled` gives
  the painted boxes.
- Floats (`layout/src/floats.rs`, ADR 0015): each block formatting
  context has an exclusion space; `LayoutContext` keeps a stack of BFCs.
  Block layout knows the BFC position of its boxes; boxes whose top
  margin can still collapse wait in a chain of frames, and the floats in
  them wait for its position. Lines and BFC roots take layout
  opportunities produced one at a time; all float work of a layout pass
  shares a work budget.
- Paint order: each stacking context (root; positioned boxes with an
  integer z-index; fixed, sticky and transformed boxes; opacity < 1;
  masked boxes) paints its normal-flow content in three phases
  (backgrounds and borders of in-flow blocks, floats, inline content;
  CSS 2.2 Appendix E), then its positioned and transformed descendants
  in z-index order and, for equal values, tree order (a pre-order index
  of the fragment tree; negative z-index before the content). Floats,
  atomic inline-level boxes and flex and grid items paint as a unit.
  Relative and absolute boxes with `z-index: auto` paint in that order
  but form no stacking context. A
  positioned box repeats the clips between its stacking context and
  itself that belong to its containing block chain; a box fixed to the
  viewport replaces all clips (`PushViewportClip`) except `clip`
  rectangles.
- Transform groups (`PushTransform`/`PopTransform`) wrap a box's opacity
  and mask groups. Fixed and sticky boxes are translation groups that
  the rasterizer and hit testing resolve at the viewport scroll offset,
  so a viewport scroll does not rebuild the display list. Translation
  groups move their items exactly; other transforms draw into a bounded
  layer (anti-aliased edges, per-strip budgets, at most 8 deep).
- The content of scroll containers is translated by their offsets in
  the display list; `paint/src/scroll_indicator.rs` draws the GUI's
  overlay scroll indicators (not in headless screenshots).
- The display list contains the selection highlight (a rectangle behind
  the selected glyphs, then the glyphs again in the selection color) and
  outlines; `outline-style: auto` is Chromium's two-ring focus ring.
- Rasterization works in device pixels. Rectangles without rounded corners
  and clips are snapped to whole pixels, and a single image is drawn into
  its pixel-snapped rectangle (tiled images are not snapped); an opacity or mask group draws
  into a layer that covers only its visible bounds, and a mask group's
  layer is multiplied by the mask when the group ends. The rasterizer
  works in strips: a window is one strip; a full-page screenshot is split
  into equal strips of at most the viewport height or 16 Mpx, drawn in
  place. Per strip, the layers of open groups share a memory budget
  (max(64 Mpx, 4 × strip pixels); beyond it, opacity groups draw directly
  and mask groups draw nothing), and mask groups share a work budget: a
  group starts only if all its work fits (layer pixels, a cost per row,
  gradient tiles). SVG paths share a work budget per strip; a path
  that does not fit is not drawn. The SVG rendering budget is shared by
  all strips.
  Full-page screenshots keep the layout of the viewport and draw at
  scroll offset 0, as Chromium does. The root
  element's opacity and mask also apply to the canvas background.

## Engine modules

- `page/mod.rs`: page state, navigation, history, links, accessors, the
  DOM change API (`change_dom`) and the lending of the document state.
- `page/loading.rs`: network completions, documents (HTML, text, image,
  error pages), the parse to the end or in steps.
- `page/pipeline.rs`: rendering and screenshots, the viewport and its
  limits.
- `page/scroll.rs`: scrolling by the user (wheel, keys) and the scroll
  calls of the front ends, with the hover state after a scroll.
- `page/script.rs`: the script runtime of the document: its creation at
  the commit and the start of the parse, the delivery of fetched scripts,
  the `load` event and the unload, the lending of the state for a task,
  `evaluate` and the console messages, and the requests that scripts made
  during the task (feature 6d1: a navigation from `a.click()` or
  `form.submit()`, a scroll into view after `focus()`), which the page
  carries out when the task ends.
- `document/mod.rs`: `DocumentState`, its construction and the parse in
  steps, element states and the selection.
- `document/script_actions.rs`: what scripts ask for focus, links and
  forms (feature 6d1): the focus (set at once; the restyle for `:focus`
  waits for the next flush), whether an element can be focused (the
  engine's rule and the styles of the last flush), the invalid controls of
  a form, the reset of its controls, and the stored navigation and scroll
  requests (`ScriptRequests`).
- `document/scripts.rs`: the fetches of the scripts of script elements
  (the limit in flight, the checks of the response, decoding) and the
  style sheets that block scripts.
- `document/changes.rs`: the journal drain: resource discovery,
  sheets of removed elements, invalidation, title and base URL.
- `document/derived.rs`: `Derived<T>` (data keyed by the DOM generation),
  the base URL and the title.
- `document/loads.rs`, `web_fonts.rs`: stylesheet, image and font
  requests and their completions; image source selection (also
  `sizes="auto"` after each layout); web fonts: the face set of the
  document, loading the faces that layout requested (`url()` through the
  loader, `local()` from the installed fonts), font files by URL.
- `document/pipeline.rs`: style, layout, display list and raster with
  per-stage timings; restyles after state changes.
- `document/queries.rs`: element boxes, scroll state and scrolls that
  scripts can ask for, scrolling to a fragment, hit testing, rendered
  text and the selection.
- `scrollers.rs`: scroll offsets per element, scroll ranges, scroll
  chains, alignment for scroll into view.
- `page/input.rs`: mouse and key events, element states, cursor, focus
  navigation, selection by mouse and keyboard.
- `page/events.rs` (feature 6d2): the user's input as DOM events when the
  document has a script runtime. Each input is a task of the user
  interaction source that the embedder's call runs at once, ahead of the
  tasks of the event loop (ADR 0028 "Feature 6d2 notes"): the page
  hit-tests, fires the events (`pointerover`, `mousemove`, `pointerdown`,
  `click`, `wheel`, `keydown`, `beforeinput`, ...) through
  `Page::dispatch_event` with `swb_web::UserEvent`, and runs the default
  action only if no listener canceled the event. The activation behavior
  of a click (links, checkboxes, radio buttons, buttons, labels,
  summaries, `javascript:` URLs) runs in the dispatch of `click` in `web`,
  so the engine's own action does not run twice. The same file keeps the
  frame that fires `scroll` once per frame for the document and for each
  scroll container that scrolled. With JavaScript off none of it runs.
- `page/forms.rs`: editing, activation and submission of form controls;
  `forms/` (state, view for layout, select, value sanitization, the
  entry list and submission, encodings); `edit.rs` (`TextEdit`, the
  editing model, also used by the GUI address bar).
- `focus.rs`: focusable elements and the sequential focus order.
- `image_source.rs`: image source selection for `<img>` (`srcset`,
  `sizes`, `<picture>` and `<source>`, the source of the dimension
  attributes); `resources.rs` keeps each image's URL and pixel density
  (and selects sources again after viewport or scale changes) and the
  author stylesheets of a document (`Sheets`).
- `selection.rs`: text positions, the position at a point, words and
  blocks, the highlight for paint, and the selected text (`innerText`
  rules).
- `input.rs` (the input event types `Key`, `Modifiers`, `MouseButton`, and
  the attributes that keyboard events show for a key),
  `hit_test.rs`, `history.rs`, `resources.rs`, `boxes.rs`.

## Testing

See [testing.md](testing.md) and [ADR 0005](adr/0005-testing-strategy.md).
Python tools for fixtures and Chromium comparisons are in `tools/`.
