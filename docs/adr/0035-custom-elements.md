# ADR 0035: Custom elements

- Status: accepted
- Date: 2026-11-08

## Context

M10 targets the front page of github.com/senko/swb with JavaScript on
(roadmap, M10; [github-diagnosis.md](../github-diagnosis.md)). Since feature 3
(ADR 0034) the page's 9 module scripts load their 95 modules and run; they
stop at the first use of `customElements`. Feature 4 adds autonomous custom
elements: the registry (`define`, `get`, `getName`, `whenDefined`,
`upgrade`), `HTMLElement` as a constructor base, synchronous construction,
upgrades, the reactions (`connectedCallback`, `disconnectedCallback`,
`adoptedCallback`, `attributeChangedCallback` with `observedAttributes`),
`[CEReactions]` and `:defined` (also with JavaScript off).

What exists:

- `HTMLElement` and the other element interfaces exist (ADR 0028, feature
  6c). `[HTMLConstructor]` makes the generator write a constructor that
  throws `Illegal constructor` (`crates/webidl/src/emit/iface.rs`). The
  wrapper of an element whose local name is a valid custom element name has
  the prototype `HTMLElement.prototype` (`html::class_of_html`), else
  `HTMLUnknownElement.prototype`. `is_valid_custom_element_name` uses the
  old `PCENChar` production.
- The IDL files have 308 `[CEReactions]` annotations (`html.idl` 261,
  `dom.idl` 43, `cssom.idl` 4); the generator ignores them
  (`crates/webidl/src/model.rs`).
- `:defined` matches every element (`ElementState::DEFINED` is always set in
  `crates/style/src/element.rs`).
- The engine: a native constructor sees `new.target` (`NativeCall`); host
  constructors take the prototype of `new.target` with
  `Runtime::prototype_from_new_target` (GetPrototypeFromConstructor with the
  realm of `new.target`); `super(...)` in a derived class constructs its
  parent with `new.target` and binds `this` to the result, whatever object
  the parent returns (`Insn::SuperCall`); `Reflect.construct` passes
  `new.target`; a derived constructor checks its result (`DerivedReturn`).
  `Runtime::construct`, `is_constructor` and the runtime's
  `[[SetPrototypeOf]]` exist but are `pub(crate)`.
- The DOM: every primitive change goes to the journal and to the document's
  mutation sink (`swb_dom::MutationSink`, ADR 0028 section 3), which the
  mutation observer registry of the agent implements
  (`crates/web/src/mutation_registry.rs`, Rust data only, shared by an
  `Rc`). The glue of every native checks `Runtime::host_signal` after the
  body (`bindings/glue.rs`, `run`).
- The parser: html5ever's tree builder writes into the document through
  `swb_dom::parser::sink::Sink` (`TreeSink`, handle = `NodeId`). The web layer
  drives it one step at a time (`EngineHost::parse_step`, which returns at
  each `script` end tag; `document_load.rs`), runs scripts between the steps,
  and `document.write` feeds written text through `resume_written`
  (`document_write.rs`). The fragment parser parses into a temporary arena
  and copies the nodes into the target arena (`parser/fragment.rs`).

Sources: the HTML Standard (§4.13 "Custom elements": core concepts, the
`CustomElementRegistry` interface, upgrades, reactions, `[CEReactions]`;
§3.2.3 "HTML element constructors"; §13.2.6.1 "create an element for a
token", "insert an element at the adjusted insertion location"; §8.4
"dynamic markup insertion", the throw-on-dynamic-markup-insertion counter),
the DOM Standard (§4.2.3 insert, remove; §4.9 create an element, handle
attribute changes; clone a single node; adopt), Web IDL (callback functions,
`[CEReactions]`). Text of 2026-11-08. Measurements in Chromium 148 (below).
The session that wrote this ADR read no browser or engine source code; it
read swb's own engine code (`crates/js`) for section 2.

### What GitHub needs

Measured in Chromium's replay of `fixtures/js/github-swb` with an init script
that wraps the registry's methods, `createElement`, `innerHTML`, `cloneNode`,
`importNode`, `adoptNode`, `attachInternals`, `attachShadow` and the
callbacks of each definition before `define` reads them (`swbtools replay
github-swb --init-script FILE --steps FILE --record EXPR` with one step
group `{"wait": 2000}`; three runs, equal apart from the times). The init
script and the records were working files of the session and are not kept.

| Item | Chromium replay |
|---|---|
| `define` calls | 75: 74 succeed; one is a second `toggle-switch`, `NotSupportedError`, caught by catalyst's define helper (it tests `instanceof DOMException` and the name `NotSupportedError`) |
| When | 70 while `readyState` is `interactive` (the module scripts after the parse, before `DOMContentLoaded`); 5 after `load` (lazy chunks: `select-panel-experimental`, the second `toggle-switch`, `cookie-consent-link`, `ghcc-consent`, `react-partial`) |
| Options and statics | no `extends`; no `formAssociated`; no `disabledFeatures`; `observedAttributes` on 24 definitions, 74 names (`relative-time` 21, `sudo-credential-options` 9) |
| Prototype callbacks (of the 74 definitions) | `connectedCallback` 73, `disconnectedCallback` 51, `attributeChangedCallback` 50, `adoptedCallback` 0, `connectedMoveCallback` 0, form callbacks 0; `live-region` has none |
| Other API calls during the load | `get` 26,051; `upgrade` 8 (catalyst's `connectedCallback` upgrades its own element: no-ops); `whenDefined` 0 (4 call sites in later code); `getName` 0; `attachInternals` 0 |
| The 8 tags on the page | `tool-tip` 1, `turbo-frame` 1, `react-app` 1: upgraded at their `define` (interactive); `react-partial` 4, `cookie-consent-link` 2, `ghcc-consent` 1: upgraded at their `define` after `load`; `relative-time` 23: created by `document.createElement` after the define (25 synchronous constructions, 23 inserted); `clipboard-copy` 2: in template contents, cloned with `content.cloneNode(true)` and inserted (upgrade at the insertion) |
| Callbacks during the load | `relative-time` connected 23, attributeChanged 69; `tool-tip` connected 1, attributeChanged 4 (its attributes at the upgrade); `ghcc-consent` 1 and 3; `react-partial` connected 4; `react-app` 1; `turbo-frame` 1; `cookie-consent-link` 2; `clipboard-copy` 2. No disconnected or adopted callback |
| `attachShadow` | `relative-time` 25 (one per construction), `tool-tip` 1 (feature 5) |
| `HTMLElement` | Turbo replaces `window.HTMLElement` with `function () { return Reflect.construct(BuiltIn, [], this.constructor) }` when `customElements` exists (the "native shim"); classes defined after it extend the shim |
| `:defined` in the CSS | 2 rules: `themed-picture:defined` and `query-builder:not(:defined) .QueryBuilder-Input`; neither element is on the page, so the JS-off rendering does not change |
| Lazy definitions | element-registry (catalyst's lazy define) inserts `X:not(:defined){animation: ...}` rules into an adopted style sheet only with a feature flag that the page has off; Chromium takes the fallback (a `MutationObserver` and `requestAnimationFrame` that call `customElements.get` and `querySelector` for each of about 200 lazy tags: the 26,051 `get` calls) |

Consequences for the plan: on the first load the parser creates no element
whose name is defined (the definitions come from module scripts, which run
after the parse). GitHub needs upgrades at `define`, synchronous
construction in `createElement`, the reactions of DOM operations (insert,
attribute changes) and `[CEReactions]`; it does not need synchronous
construction in the parser. The plan puts the parser last (sub-feature 4c).

Other targets: BBC calls `customElements.whenDefined('green-video')` and its
video player checks `customElements`, `HTMLElement.prototype.attachShadow`
and `ResizeObserver`; Wikipedia's Vue template compiler calls
`customElements.get`. No other fixture has `:defined` in its CSS.

### Measurements

New probe file `tools/probes/web-custom-elements.json` (37 cases, 115
queries; the logging header of `web-module-graph.json` with `onerror`'s error
object, and a helper `mk()` that defines a class which logs its constructor
and callbacks). Every case has a control query. Chromium 148, three runs,
byte-identical. In the logs, `ctor:x-a#1:A:P:C` is a constructor with `A`
attributes, a parent (`P`) and connected (`C`) at the time; `attr:E:N:OLD:NEW`
an `attributeChangedCallback`.

| Question | Case | Chromium |
|---|---|---|
| Members | `api-surface` | `CustomElementRegistry.prototype`: `constructor, define, get, getName, initialize, upgrade, whenDefined`; `new CustomElementRegistry()` works (scoped registries); `document.customElementRegistry`, `Element.prototype.customElementRegistry`, `attachInternals`, `ElementInternals`, `moveBefore`, `CustomStateSet` exist. `window.customElements`: getter, no setter, enumerable, configurable. Lengths: `define` 2, `get`, `getName`, `whenDefined`, `upgrade` 1 |
| `HTMLElement` as a function and with `new` | `api-html-element-constructor` | `new HTMLElement()`, `Reflect.construct(HTMLElement, [], function(){})`, `new (class extends HTMLElement {})()` without a definition: `TypeError: Failed to construct 'HTMLElement': Illegal constructor`; `HTMLElement()`: `... Please use the 'new' operator, this DOM object constructor cannot be called as a function.`; a defined `class extends HTMLDivElement` without `extends`: `Failed to construct 'HTMLDivElement': Illegal constructor: autonomous custom elements must extend HTMLElement`; `define('x-btn', HTMLButtonElement)` succeeds, then `createElement('x-btn')` reports `Failed to construct 'HTMLButtonElement': Illegal constructor` and returns an `HTMLUnknownElement` |
| `define` errors | `define-errors` | name: `SyntaxError: Failed to execute 'define' on 'CustomElementRegistry': "foo" is not a valid custom element name` (also `Foo-bar`, `-foo`, `font-face`, `a-B`, `1-a`, `x-a b`; `x-é`, `a-`, `x-a!` are valid); not callable: `TypeError: ...: parameter 2 is not of type 'Function'.`; callable, not a constructor (arrow function, `Math.max`): `...: constructor argument is not a constructor`; `NotSupportedError: ...: the name "x-e1" has already been used with this registry`; `...: this constructor has already been used with this registry`; `TypeError: ...: constructor prototype is not an object`; `...: "connectedCallback" is not a function`; `observedAttributes = 5`: `...: The provided value cannot be converted to a sequence.`; a getter that throws: its error, nothing defined; `define` inside a getter that `define` reads: `NotSupportedError: ...: an element definition is already being processed`; `extends: 'x-y'`: `..."x-y" is a valid custom element name`; `extends: 'bogus'`: `..."bogus" is an HTMLUnknownElement`; `extends: 'p'` works (customized built-ins); a bound class: defined (spec: `TypeError`), `createElement` then fails with `Illegal constructor`; one argument: `2 arguments required, but only 1 present.` |
| Order of reads in `define` | `define-read-order` | `prototype`, then on the prototype `connectedCallback, disconnectedCallback, connectedMoveCallback, adoptedCallback, attributeChangedCallback`, then `observedAttributes` (only with an `attributeChangedCallback`), `disabledFeatures`, `formAssociated`, and with `formAssociated` true the four form callbacks and Chromium's own `toolFillCallback` |
| `get`, `getName` | `get-and-getname` | `get` of an undefined name, `X-G`, `div`: `undefined`; `getName(C)`: the name; of another function: `null`; `getName(5)`: `parameter 1 is not of type 'Function'.` |
| `whenDefined` | `when-defined` | the same promise for an undefined name; after `define` a new promise per call; it resolves with the constructor during `define`, after the upgrades: `ctor attr conn after-define`, then the microtasks in order `mt-before wd1 mt-after wd3`; an invalid name rejects with `SyntaxError: Failed to execute 'whenDefined' on 'CustomElementRegistry': "bad" is not a valid custom element name` |
| Upgrades at `define` | `upgrade-at-define` | the elements of the document in tree order, each with its constructor, `attributeChangedCallback` for each observed attribute in attribute order, then `connectedCallback`, before `define` returns; a detached element and template contents are not upgraded; `customElements.upgrade(detached)` upgrades it (no connected callback); `upgrade` of an element in template contents does nothing; `importNode` of the template contents upgrades the copy at once |
| Attributes at an upgrade | `upgrade-attributes` | one callback per observed attribute, in attribute order, also a namespaced one with the observed local name (`attr:x-a#1:a:null:ns:urn:x`) |
| A constructor that fails at an upgrade | `upgrade-constructor-failures` | the error is reported (`onerror`), the element is not `:defined`, keeps the prototype that `super()` gave it, gets no callbacks and is never upgraded again (insertion, `upgrade()`); other elements go on. No `super()`: `ReferenceError: Must call super constructor in derived class before accessing 'this' or returning from derived constructor`; another object returned: `TypeError: custom element constructors must call super() first and must not return a different object`; `super()` twice: `TypeError: Failed to construct 'HTMLElement': This instance is already constructed` |
| Upgrade inside a constructor | `upgrade-reentrant` | the HTML example: `ctor:a ctor:b conn:b conn:a`, each constructor once |
| `upgrade(root)` | `upgrade-root` | tree order over two definitions; `upgrade(5)`: `parameter 1 is not of type 'Node'.` |
| `createElement` | `create-element-sync` | the constructor runs inside the call (no attributes, no parent); `createElement('X-A')` too; `createElementNS(HTML, 'x-a')` too; `createElementNS(HTML, 'X-A')`: `HTMLUnknownElement`, `:defined`; SVG namespace: no construction, `:defined`; `new C()`: local name `x-a`; in a `createHTMLDocument` document: no construction, not `:defined` |
| A constructor that fails in `createElement` | `create-element-failures` | `createElement` does not throw: it reports the error and returns a new `HTMLUnknownElement` that is not `:defined`. Messages: `NotSupportedError: Failed to execute 'createElement' on 'Document': The result must not have attributes` (`children`, `a parent`, `must have the same localName`); `TypeError: ...: The result must implement HTMLElement interface` (`return {}`); the constructor's own error. `new C()` with the same constructor returns the element with its attribute |
| Parser, definition before the markup | `parser-sync` | `ctor:x-a:0:0:0` (no attributes, not inserted), then one `attributeChangedCallback` per attribute, `connectedCallback`, and a microtask queued in `connectedCallback` runs before the next element's constructor |
| `document.write` | `parser-sync-document-write` | the constructor runs inside `write`; its microtask runs after the script |
| Dynamic markup in a parser-created constructor | `parser-sync-dynamic-markup` | `InvalidStateError: Failed to execute 'write' on 'Document': Custom Element constructor should not use write().` (also `open()`, `close()`); in a constructor of an upgrade, `write` works |
| A constructor that fails in the parser | `parser-sync-failures` | the parser inserts a new `HTMLUnknownElement` with the token's attributes and later children; the reports have the document URL and 0:0 and no "Failed to execute" prefix (`NotSupportedError: The result must not have attributes`, `... a parent`, `... the same localName`); an element that the constructor inserted elsewhere stays there; a constructor that makes another instance before `super()` gives a custom element (both constructors run) |
| Fragment parsing | `fragment-parsing` | `innerHTML`, `insertAdjacentHTML`, `outerHTML` upgrade the new elements inside the call, also when the context element is detached (no connected callback then); `outerHTML`: the new element's upgrade and connected callback come before the old element's `disconnectedCallback`; `template.innerHTML` and `DOMParser`: no upgrade; `adoptNode` of a `DOMParser` element and an insertion: upgrade at the insertion |
| `cloneNode`, `importNode` | `clone-and-import` | `cloneNode(true)` of a custom element upgrades the copies inside the call (detached); `template.content.cloneNode(true)`: no upgrade until the insertion; `importNode`: upgrade inside the call |
| Subtrees | `reactions-subtree` | connected and disconnected callbacks in tree order, root first; `replaceChildren()` and `textContent = ''` disconnect in tree order |
| Grouping in the element queue | `reactions-queue-order`, `reactions-append-several` | `body.append(y, x)` with `x` connected: `disc:x conn:x conn:y` (all reactions of an element run when the element is first reached); `replaceChildren(z, y)`: `disc:z conn:z disc:y conn:y disc:x` |
| Nested reactions | `reactions-nested`, `constructor-defines-another` | a callback's own mutations run their reactions inside the callback (`conn:x conn:y conn-end:x`); a constructor that defines another element upgrades that element's instances at once |
| Attribute operations | `reactions-attributes` | a callback for `setAttribute` also with the same value (`a:1:1`), none for an unobserved name or a missing attribute, `toggleAttribute` (`null` to `""`), the namespaced attributes, `id`, `className`, `classList`, `dataset`, `style` (`color: red;`), `Attr.value`, `setAttributeNode`, `setAttribute('A')` in an HTML document (`a`) |
| Adoption | `reactions-adopted` | `adoptNode` of a connected subtree: per element `disc` then `adopt(old, new)`; an insertion into another document: `disc adopt conn` per element; a document of `createHTMLDocument` counts as connected for the callbacks |
| A callback that throws | `reactions-callback-throws` | reported (`onerror`), the next callbacks run, the element stays `:defined` |
| Depth | `reactions-depth-limit`, `reactions-recursion-count`, `reactions-reinsert-loop` | at most 43 nested reaction invocations: the 44th nested `connectedCallback`, `attributeChangedCallback` or constructor does not run and is never run later, no error is reported, a `createElement` at that depth returns a failed `HTMLUnknownElement`; independent of the JavaScript stack (100 extra frames per level: still 43). A callback that re-inserts its element in a loop runs 43 times |
| 10,000 upgrades at one `define` | `upgrade-many` | all upgraded inside the call |
| `:defined` | `defined-pseudo-class`, `defined-js-off` | not defined: a valid custom element name in the HTML namespace before its upgrade, also with JavaScript off; defined: `div`, `foo`, `font-face`, `annotation-xml`, an SVG element named `x-a`; after `define` the style follows |
| Patterns of the engine | `shim-reflect-construct`, `derived-classes` | Turbo's shim works (upgrade, `createElement`, `new`, `instanceof` of the shim and the built-in); a subclass of a defined class: the callbacks and `observedAttributes` are inherited, `Reflect.construct(A, [], B)` makes a `B` element |
| GitHub's order | `define-in-module-before-dcl` | a module script that defines upgrades the parsed elements during its run, before `DOMContentLoaded` |

## Decision

### 1. Element state and `:defined`

Each element gets the custom element state of the DOM Standard (`undefined`,
`failed`, `uncustomized`, `precustomized`, `custom`) and its definition, in
`swb_dom::ElementData`: a `u8` state and a `u32` definition number (0 = none;
the number indexes the agent's definition table, section 3). Both fit in the
padding after `script_already_started`, so `ElementData` keeps its size.

- **Creation.** Every path that creates an element (the parser's sink, the
  fragment parser's copy, `create_node_in`, clones) gives it `undefined` if it
  is in the HTML namespace and its local name is a valid custom element name,
  else `uncustomized`. The check moves to `swb_dom` and follows the current
  definition (a valid element local name that starts with an ASCII lower
  alpha, contains `-` and no ASCII upper alpha, and is not one of the eight
  reserved names): Chromium accepts `x-a!` (measured), which the `PCENChar`
  production rejects. The test for `-` first keeps the parser's cost to one
  byte search per element name (names are atoms; the result can be cached
  per atom).
- **`:defined`** matches an element whose state is `uncustomized` or
  `custom` (HTML "defined"). `interaction_state` in `swb_style` sets
  `ElementState::DEFINED` from the state. With JavaScript off, every element
  with a custom element name stays `undefined`, as in Chromium (measured,
  `defined-js-off`).
- **Invalidation.** A change of the state of a connected element appends a
  journal entry `Change::State { element }` that the mutation sink does not
  see (no mutation record). The engine drains it as an attribute change
  that every selector may depend on (the element, its subtree, and the
  siblings after it if the sheets have sibling combinators), so
  `x-a:not(:defined) .child` updates. A detached element needs no entry.

JS-off effect: `:defined` and `:not(:defined)` start to differ for custom
element names. No fixture uses them on an element that it has (GitHub's two
rules name elements that the page does not have), so no JS-off score
changes; the sub-feature checks it with `just snapshot`.

### 2. The engine

A class that extends `HTMLElement` needs nothing new from the interpreter.
Checked in swb's code and measured (`derived-classes`,
`shim-reflect-construct`, the constructor failure cases):

- `super()` in a derived constructor calls `[[Construct]]` of the parent
  with the frame's `new.target`. For a native parent (an interface object)
  the native gets `new.target` and its own function object as the callee
  (the "active function object" of HTML's steps). Whatever object it
  returns becomes `this` (`Insn::SuperCall`); a second `super()` constructs
  again and then throws the `ReferenceError` of a bound `this`, as ECMA-262
  says (HTML's "already constructed" check throws first; measured).
- The prototype: `Runtime::prototype_from_new_target` is
  GetPrototypeFromConstructor with the realm of `new.target`; HTML's
  [HTMLConstructor] steps 6 and 7 need exactly that.
- `Reflect.construct(HTMLElement, [], C)` (Turbo's shim) is a deferred
  `[[Construct]]` with an explicit `new.target`; a plain function as the
  parent (the shim) gets an ordinary `this` and returns another object,
  which the derived class then takes (return override, §10.2.2).
- A derived class of a derived class passes `new.target` down the chain.
  Static members (`observedAttributes`) are inherited through the
  constructor's prototype chain; `Get` finds them.
- A constructor that returns a non-object primitive: the engine's
  `Derived constructors may only return object or undefined` (V8's text).

What the host needs and does not have (all in `crates/js`, small):

1. `Runtime::construct(callee, args) -> VmResult<Value>` public (it exists as
   `pub(crate)`): the upgrade and "create an element" call `Construct(C)`.
2. `Runtime::is_constructor(value)` public: `define` step 1.
3. `Runtime::set_prototype_of(object, proto) -> VmResult<bool>` public: the
   ordinary `[[SetPrototypeOf]]` with the cycle check and the charge, for
   the wrapper of an element at an upgrade (HTML [HTMLConstructor] step
   "Perform ? element.[[SetPrototypeOf]](prototype)"). Wrappers are host
   objects of classes without hooks; an object without own properties moves
   to the root shape of the new prototype (no dictionary mode).
4. A second host flag (`Runtime::host_flags() -> u8` with bit 0 the existing
   "several documents" flag and bit 1 "custom element definitions exist"),
   one load in the fast path of `[CEReactions]` (section 6).

Rejected: a hook in the interpreter for `super()` into `HTMLElement`
(HTML's steps run in the native constructor; the engine needs no knowledge
of custom elements); an engine-side construction stack (it is host state,
keyed by definitions that the engine does not know).

### 3. The registry and the definitions

- **Definitions** belong to the agent (one runtime, ADR 0031 section 5): an
  element keeps its definition when it moves to another document of the
  runtime. `WebState` gets a table of definitions; the number in
  `ElementData` indexes it. A definition has the name, the local name, the
  constructor, the callbacks (`connected`, `disconnected`, `adopted`,
  `attributeChanged`, `connectedMove`; the form callbacks are read but not
  kept, section 9), the observed attributes, `formAssociated`, the disable
  flags, the construction stack (section 4) and the realm of its registry.
  The JavaScript values (constructor, callbacks) are in `WebState`, traced
  by `HostState::trace`; the Rust part (local name, observed attribute set,
  a bit mask of the callbacks that exist) is also in the agent's reaction
  state (section 5), which the document sinks share by `Rc`, so that the
  sink can decide without the runtime whether a change enqueues a reaction.
  Definitions are never removed: a registry lives as long as its window,
  and the budget of section 10 bounds them.
- **Registries** belong to a window: `DocState` gets the registry of its
  `Window`: the `CustomElementRegistry` object (made at the first read of
  `window.customElements`; `[SameObject]`), a map name → definition, a map
  constructor → definition (`Gc<Object>` hashes by slot and generation; the
  definition keeps the constructor alive), the flag "element definition is
  running", and the when-defined map name → promise (traced). Each document
  also gets the set of defined local names (`swb_dom::Document`, for the
  parser's sink and the creation paths, which have no runtime).
- **Which registry.** HTML's newest text gives each element a registry; for
  the global registry this is the node document's window's registry.
  swb looks up the registry of the element's node document: a document with
  a `Window` has one; `createHTMLDocument`, `DOMParser` and template contents
  documents have none (measured: no upgrade there). Scoped registries are
  out (section 9).
- **`define`** follows HTML's steps and Chromium's messages and order of
  reads (table above): `IsConstructor`, the name, the existing name and
  constructor, `extends` (section 9), the running flag, then `prototype`,
  the five callbacks (`connectedMoveCallback` included), `observedAttributes`
  (only with an `attributeChangedCallback`; `sequence<DOMString>` through the
  iterator, as the generated conversions do), `disabledFeatures`,
  `formAssociated` and, if it is true, the four form callbacks. A callback
  that is not `undefined` must be callable (`"connectedCallback" is not a
  function`). Then the definition is appended, the agent's flag is set (on
  the first definition), the local name joins the document's set, the
  upgrade candidates of the document are enqueued (section 4), and the
  when-defined promise resolves with the constructor. `define` has
  `[CEReactions]`, so the upgrades run before it returns (measured).
- **`get`, `getName`, `whenDefined`, `upgrade`** follow HTML. `whenDefined`
  rejects an invalid name with the measured `SyntaxError`; it returns the
  stored promise for an undefined name and a new resolved promise for a
  defined one. `upgrade(root)` tries to upgrade the inclusive descendants of
  `root` in tree order (shadow trees join with feature 5).
- **IDL.** `CustomElementRegistry` with `define`, `get`, `getName`,
  `whenDefined`, `upgrade`; `callback CustomElementConstructor = HTMLElement
  ()`; `dictionary ElementDefinitionOptions { DOMString extends; }`;
  `partial interface Window { readonly attribute CustomElementRegistry
  customElements; }`. No constructor, no `initialize`, no
  `customElementRegistry` attributes (section 9).

### 4. Constructors, "create an element" and upgrades

- **[HTMLConstructor]** (HTML §3.2.3) for every element interface: the
  generator writes a constructor that calls one hand-written function with
  the interface's class. It runs HTML's steps: `new.target` equal to the
  callee, or no definition with `new.target` as its constructor in the
  current realm's registry: `Illegal constructor`; an autonomous definition
  whose callee is not `HTMLElement`: `Illegal constructor: autonomous custom
  elements must extend HTMLElement` (measured). Then the prototype from
  `new.target`, and:
  - an empty construction stack: a new element of the current window's
    document with the definition's local name, state `custom`, the
    definition, and a wrapper with that prototype (`wrap_node_with_proto`);
  - otherwise the element at the top of the stack: an "already constructed"
    marker there throws `Failed to construct 'HTMLElement': This instance is
    already constructed`; else the wrapper gets the prototype (made with it
    if the element has no wrapper yet, else `set_prototype_of`), the entry
    becomes the marker, and the element is returned.
- **The construction stack** of a definition is host data: a `Vec` of
  `(DocKey, NodeId)` entries or the marker. Its depth is bounded by the
  nesting limit (section 5).
- **"Create an element"** (DOM §4.9) is one function in `crates/web`, used by
  `createElement`, `createElementNS`, the parser (section 8), clones and the
  fragment parser's copy:
  - definition found and the synchronous flag set (`createElement`,
    `createElementNS`, the parser outside fragments): `Construct(C)`, then
    the checks of the result (implements `HTMLElement`, no attributes, no
    children, no parent, the same node document, the same local name);
    a failure is reported (not thrown) and the result is a new
    `HTMLUnknownElement` in state `failed`. Messages as measured, with the
    prefix `Failed to execute 'createElement' on 'Document': ` in
    `createElement` and none in the parser. The report uses the definition's
    realm (`errors::report`; a value thrown by the constructor is reported as
    it is);
  - definition found, flag not set (clones, `importNode`, the fragment
    parser): a new element in state `undefined` and an upgrade reaction;
  - no definition: the state of section 1.
- **Upgrade** (HTML §4.13.5): the state check (`undefined` or
  `uncustomized`), the definition, state `failed`, an
  `attributeChangedCallback` reaction for each observed attribute in
  attribute order, a `connectedCallback` reaction if the element is
  connected, push the element on the construction stack, state
  `precustomized`, `Construct(C)`, `SameValue` with the element or the
  measured `TypeError`, pop the stack; on an error the definition goes, the
  element's reaction queue is emptied, and the caller reports the error (the
  state stays `failed` or `precustomized`, the prototype stays what `super()`
  set: measured); else state `custom`. The journal entry of section 1
  follows each state change of a connected element.
- **Upgrade candidates at `define`**: the elements of the window's document in
  tree order with the HTML namespace and the local name (shadow trees with
  feature 5). The walk costs the size of the document per `define`; GitHub
  has 74 definitions and 1,732 elements. A cheap filter avoids most walks:
  each arena keeps the set of custom element names that it ever created
  (`swb_dom::Document`, filled at element creation); a name outside the set
  needs no walk (70 of GitHub's 74 names). The walk charges the time
  countdown per node (as the mutation registry's work does).

### 5. Reactions

The agent has one reaction state (`crates/web/src/custom_elements/reactions.rs`),
shared by `Rc<RefCell<...>>` with the document sinks (as the mutation
registry is), Rust data only:

- **The element reaction queues**: a map from the element (`Place`: document
  key and `NodeId`, as the mutation registry names nodes) to a `VecDeque` of
  reactions. A reaction is `Upgrade(definition)` or `Callback(kind, args)`;
  the arguments are Rust values (the attribute's local name, namespace, old
  and new value; the old and new document keys). The callback functions are
  looked up in the definition when the reaction runs (definitions do not
  change, so this equals HTML's stored callback).
- **The reactions stack** as a flat `Vec<Place>` of queued elements and a
  `Vec<usize>` of marks: pushing an element queue records the length,
  popping takes the suffix. The current element queue is the suffix after
  the last mark. A nested scope always pops before its parent continues, so
  the suffix is exactly HTML's top queue. Push and pop with nothing queued
  cost a `Vec` push and a comparison.
- **The backup element queue** (`Vec<Place>` and the "processing" flag): an
  enqueue with an empty stack goes there and queues one microtask
  (`queueMicrotask`'s job kind) that invokes it. Mutations outside
  `[CEReactions]`: the engine's own changes (form state, user editing) and
  the parser's insertions between pauses (section 8 handles the parser's
  own).
- **Invoke** (HTML "invoke custom element reactions"): for each element of
  the queue, take its reactions one at a time until its queue is empty
  (reactions that an upgrade adds run in the same loop: `ctor attr conn`).
  An upgrade reaction runs section 4's upgrade and reports its error; a
  callback reaction calls the function with the element's wrapper as
  `this` and the arguments (`adoptedCallback` gets the two documents'
  objects) and reports an error (`errors::report`, the callback's realm).
  After each call `event_loop::after_listener` runs HTML "clean up after
  running script": a microtask checkpoint when no script is on the stack
  (measured: the parser's case runs the microtask of a `connectedCallback`
  before the next constructor). A termination stops the invocation and goes
  up.
- **Depth limit (Chromium's, measured)**: a counter of nested invocations
  (callbacks, upgrades, and the synchronous constructions of section 4).
  At 43, a further callback or upgrade reaction is dropped without a
  report, and a synchronous construction gives a failed
  `HTMLUnknownElement` without a report. This matches Chromium in the three
  measured shapes and bounds the Rust stack: each level is one re-entry of
  the interpreter (ADR 0026 section 9), so 43 levels stay far from the
  recursion budget.
- **Element queue entries for elements that move** to another arena (frames,
  ADR 0031 "transplant"): the transplant re-keys the element's reaction
  queue and its entries in the stack and backup queue, as it re-points
  wrappers.

### 6. `[CEReactions]` in the generated bindings

The generator honors `[CEReactions]` on operations, attribute setters, and
the setters and deleters of legacy platform objects (`DOMStringMap`). It
wraps only the call of the hand-written member, after the argument
conversions (Web IDL runs the conversions before the method steps, so a
`toString` that defines an element cannot see a half-pushed queue):

```rust
let r = b::ce::scope(rt, |rt| <H as ElementImpl>::set_attribute(&mut b::Cx::new(rt, call, CLASS), this, a0, a1));
```

`b::ce::scope` is `#[inline(always)]`: if bit 1 of `Runtime::host_flags` is
clear (no definition in the agent), it calls the body and returns; else a
cold function pushes a mark, calls the body, pops, invokes the queue if it
is not empty, and returns the body's result (an exception of the body is
returned after the reactions ran, as HTML says). Hand-written natives that
mutate the DOM outside the generated glue (`document.write` through the
parser, the `[SwbCustom]` members, the engine's paths into the bindings)
call the same function.

Why the fast path is safe: without a definition no element is custom and no
upgrade can be enqueued, and a body that runs no script cannot make a
definition; the bodies that run script (`define` itself, synchronous
construction) are entered with the flag already set, or set it themselves
before they enqueue (`define`). A reaction enqueued with an empty stack
still runs, from the backup queue.

Rejected: a push and pop in `glue::run` for every native (most natives do
not mutate the DOM; this costs every call); reading the definition count
from `WebState` (a type check of the host data per call, ADR 0031 feature
12a).

### 7. Which DOM operations enqueue reactions

The DOM's change functions already report each primitive change to the
document's sink (ADR 0028 section 3). The sink of a document becomes a small
dispatcher: the mutation observer registry and the reaction state, each
called when it is active. The reaction state acts only while the agent has
a definition, and reads the document after the change:

| Operation | Reaction | Where |
|---|---|---|
| Insertion (`appendChild`, `insertBefore`, `append`, `replaceChildren`, `textContent`, `innerHTML`'s insertion, the parser's insertions) | for each inclusive descendant of each added node in tree order, if it is connected: `connectedCallback` if `custom`, else try to upgrade (a definition in its node document's registry) | sink, `Change::Children` with added nodes |
| Removal | if the parent was connected: `disconnectedCallback` for each `custom` inclusive descendant of each removed node in tree order | sink, `Change::Children` with removed nodes (handled before the added ones of the same change: HTML removes first) |
| Attribute set, change, removal, replacement (`setAttribute*`, `removeAttribute*`, `toggleAttribute`, `Attr.value`, `setAttributeNode*`, `className`, `classList`, `dataset`, `style`, reflected IDL attributes, the parser's `add_attrs_if_missing`) | `attributeChangedCallback(localName, old, new, namespace)` for a `custom` element when the local name is observed; also for an unchanged value (measured) | sink, `Change::Attribute` (the new value is read from the document) |
| Adoption (`adoptNode`, an insertion from another document of the arena, a transplant to another arena) | `adoptedCallback(oldDocument, newDocument)` for each `custom` inclusive descendant, after the removal's callbacks | a new report `MutationSink::adopted(doc, node, old_document)` from `swb_dom::adopt` and the transplant |
| `createElement`, `createElementNS` | synchronous construction | "create an element", section 4 |
| `cloneNode`, `importNode` | an upgrade reaction for each copy whose name is defined in the copy's document | clone code in `crates/web` after `swb_dom::clone_node` returns the copies |
| `innerHTML`, `outerHTML`, `insertAdjacentHTML`, `DOMParser` | the fragment parser's copy into the target arena: an upgrade reaction per element with a defined name in tree order, before the insertion (measured order of `outerHTML`); none when the target document has no registry | `crates/web/src/dom/markup.rs` after `parse_fragment` |
| `define`, `upgrade()` | upgrade reactions | section 3 |
| The main parser, `document.write` | synchronous construction, then the insertion | section 8 |
| `moveBefore` | (swb has no `moveBefore`; `connectedMoveCallback` is read at `define` and never called) | |

Cost: the walk of an inserted subtree visits each node once and tests a
byte of the element; it runs only while the agent has definitions, and
only for subtrees that the arena's custom name set (section 4) says may
hold a custom element name. React builds a subtree detached and inserts it
once, so the walks are linear in the inserted nodes. The sink counts its
work for the time limit (as `Registry::take_overdue`).

### 8. The parser

**Today.** The web layer calls `EngineHost::parse_step`, which runs
html5ever until a `script` end tag (`ParseStep::Script`) or the end, then
processes the journal; scripts run between steps; `document.write` runs
`resume_written` on the written text inside the script's call. The tree
builder calls the sink's `create_element` and, in the same token, the
insertion (`append`, `append_before_sibling`, `append_based_on_parent_node`);
the sink cannot run script (it holds the document, and the runtime is
borrowed by the caller).

**Synchronous construction** (HTML "create an element for a token" with
"will execute script"):

1. The sink's `create_element`, for an HTML element whose local name is in
   the document's set of defined names (section 3) and a parser that is not
   the fragment parser, creates a placeholder element P with the name and no
   attributes, keeps the token's attributes and marks P pending; the
   following insertion call for P is recorded, not done.
2. A token sink that wraps the tree builder (the parser's
   `Tokenizer<Wrapper<TreeBuilder<...>>>`) forwards each token and, when the
   sink has a pending element, ends the token with
   `TokenSinkResult::Script(P)`, so the tokenizer returns to `resume` after
   that token. `resume` returns `ParseStep::CustomElement(P)` (and
   `resume_written` `WrittenStep::CustomElement(P)`). (The wrapper must keep
   the tokenizer's state as it is after the token; a test with a custom
   element before `<textarea>`, `<script>`, `<plaintext>` and in foreign
   content checks this, since swb does not read html5ever's source.)
3. The web layer runs HTML's steps: increment the document's
   throw-on-dynamic-markup-insertion counter; a microtask checkpoint if no
   script is on the stack (a parser task; not inside `document.write`);
   push an element queue; "create an element" with the synchronous flag
   (section 4), which gives a new element R or the failed fallback; append
   the token's attributes to R through the attribute funnel (so
   `attributeChangedCallback` reactions and mutation records follow);
   pop and invoke the queue; decrement the counter.
4. `HtmlParser::finish_custom_element(&mut doc, P, R)`: the sink maps P to R
   (an alias table; every sink method resolves its handles through it, with
   a fast path while the table is empty) and performs the recorded
   insertion with R, inside a pushed element queue whose reactions
   (`connectedCallback`) the web layer invokes; then the parse continues.

The alias keeps html5ever's handle (P stays in its stack of open elements)
while the element in the tree is R, so every measured case of
`parser-sync-failures` comes out as in Chromium: the failed fallback is a
new element, an element that the constructor inserted elsewhere stays
there, and a constructor that makes another instance before `super()`
works. P stays an empty detached element in the arena (one per synchronous
construction).

**Reactions of other parser changes.** A parser insertion can also enqueue
reactions for elements that are already custom (the adoption agency moving a
custom element). HTML pushes an element queue around each insertion. The
wrapper of step 2 also ends a token after which the agent's reaction state
has queued elements (one `Cell<bool>` read per token, set by the sink), and
the web layer invokes them at that pause, so they run before the next token
as in HTML. While no definition exists the cell is never set.

**The counter.** `DocState` gets the throw-on-dynamic-markup-insertion
counter; `document.write`, `writeln`, `open` and `close` throw
`InvalidStateError` with Chromium's text (`Failed to execute 'write' on
'Document': Custom Element constructor should not use write().`) while it is
above 0. A constructor of an upgrade or of `createElement` does not raise it
(measured: `write` works there).

**Fragment parsing** (`innerHTML` and the others) does not construct
synchronously: its parser has no runtime (a temporary arena), and HTML
creates the elements with an upgrade reaction. The copy into the target
arena enqueues the upgrades (section 7).

Rejected: running the constructor before the tree builder sees the token
(the wrapper would need the insertion mode to know whether the token
creates an HTML element: `<select>`, foreign content, ignored tokens); an
upgrade instead of a construction (the constructor would see the
attributes and the parent; measured: it sees neither); swapping the arena
slots of P and R (every side table keyed by `NodeId` would have to move).

### 9. Scope, departures and the API around it

- **Customized built-in elements** (`extends`, `is`): out of scope. Chromium
  supports them (measured: `extends: 'p'` works). swb runs `define`'s
  checks of `extends` with Chromium's messages for an invalid value, and
  for a valid one throws `NotSupportedError` with the text `Customized
  built-in elements are not supported` (open question 1). `is` in
  `createElement` options and the `is` attribute stay ignored.
- **Scoped registries** (`new CustomElementRegistry()`, `initialize`,
  `customElementRegistry`, the `customElementRegistry` option of
  `createElement` and `attachShadow`): out of scope; the constructor is
  absent (`Illegal constructor`), so pages that test for them take their
  fallback. GitHub uses none.
- **`ElementInternals` and `attachInternals`, `formAssociated`,
  `disabledFeatures`, `CustomStateSet` (`:state()`)**: GitHub uses none of
  them (measured: 0 calls, no static). `define` reads `disabledFeatures` and
  `formAssociated` and the form callbacks in Chromium's order and keeps the
  flags, but swb has no `attachInternals`, so feature detection answers
  "absent", and a form-associated custom element is not a form control
  (backlog: "form-associated custom elements and `ElementInternals`"). The
  disable-shadow flag is checked at an upgrade with feature 5's
  `attachShadow`.
- **`connectedMoveCallback`**: read at `define`, never called (swb has no
  `moveBefore`).
- **A bound function as the constructor**: Chromium defines it (measured);
  swb follows HTML (`constructor prototype is not an object`). Both fail
  to construct it later.
- **Chromium's `toolFillCallback`** read: not done.
- **Wrappers**: no change for elements without definitions; an upgraded
  element whose wrapper had own properties goes to dictionary mode at
  `set_prototype_of` (rare: expandos before the upgrade).

### 10. Budgets

| Resource | Limit | Result |
|---|---|---|
| Nested reaction invocations and synchronous constructions | 43 (Chromium, measured) | the reaction is dropped, the construction gives a failed `HTMLUnknownElement`; no report (as Chromium) |
| Definitions per agent | 65,536 (Chromium: none) | `define` throws `NotSupportedError` (`swb: too many custom element definitions`), one warning |
| Observed attribute names, all definitions | 1,048,576 names or 16 MiB of name text | as above |
| Pending when-defined promises per registry | 65,536 names | a new promise that is not stored (it never resolves), one warning |
| Queued reactions (element queue entries and reactions) | 4,194,304 entries or 128 MiB of argument text per agent (with the frames' runtimes, as the mutation records' budget) | later reactions are dropped, one warning |
| Construction stack | bounded by the nesting limit | |
| Work of `define`'s walk and the sink's walks | the task's time limit (5 s), charged per node | termination |
| Synchronous constructions in one parse step | the task's time limit | termination; the parse goes on without scripts after three (ADR 0028 section 7) |

Hostile cases (`tools/swbtools/hostile_cases/custom_elements.py`, all with
JavaScript on): `ce-connected-recursion` (each `connectedCallback` appends
a new instance: stops at depth 43), `ce-upgrade-100000` (100,000 elements
upgraded at one `define`), `ce-reinsert-loop` (a callback re-inserts its
element: 43 runs), `ce-observed-attributes-100000` (one definition with
100,000 observed names, then 100,000 attribute writes), `ce-define-chain`
(each constructor defines the next name, 10,000 names),
`ce-define-100000-names` (the definition budget), `ce-when-defined-1000000`,
`ce-attribute-flood` (one `innerHTML` of 10,000 custom elements with 100
observed attributes of 10 KB: the reaction byte budget),
`ce-disconnect-100000` (`textContent = ''` over 100,000 connected custom
elements), `ce-parser-100000` (100,000 defined elements in markup after
the definition: 100,000 pauses), `ce-write-in-constructor` (a constructor
that calls `document.write` and `open` in a loop: the counter).

### 11. Performance

The cost when no page defines an element must stay near zero (Ars
Technica, BBC, Craigslist with JavaScript on; their scripts mutate the DOM
often). The added work per `[CEReactions]` call is one load and one branch;
per primitive DOM change none (the sink is installed only while an observer
or a definition exists); per created element the custom name test of
section 1.

Measured as in ADR 0031 feature 12a (`docs/performance.md`, "WindowProxy and
the split of the binding state"), base against each sub-feature, release
build:

- Bindings under callgrind (`swb --headless --js`, 20,000 iterations against
  an empty page, instructions per iteration): `e.setAttribute('a', i)`,
  `e.id = i`, `t.appendChild(n)` with `n.remove()`, `e.textContent = 'x'`,
  `e.innerHTML = '<b></b>'`, `document.createElement('div')`,
  `e.classList.add('c')`. Budget: +1 % without a definition. With one
  definition of a name that the page does not use: +5 % (the marks and the
  sink's walk), reported, not a gate.
- The pages: Ars Technica, BBC, Craigslist posting with `--bench 3` under
  callgrind, total instructions. Budget: +0.3 %.
- GitHub: the load time and the callback counts go to feature 8's budget
  (1.56 s).

### 12. Tests

- Probe cases: `tools/probes/web-custom-elements.json` runs in Rust against
  Chromium's values, as the module cases do (`probe_net`, a generated
  `crates/engine/tests/probe_net/custom_elements.rs`, one table of Chromium
  values; departures as rows with reasons). Most cases need no network, so
  they can also run in `crates/web/tests` with the test document.
- Unit tests: the name check (Chromium's valid and invalid names), the
  reaction stack with marks, the grouping of the element queue, the budget
  counters, the parser's alias table and recorded insertions
  (`crates/dom/src/parser/tests.rs`: the tree after each pause equals the
  tree without definitions), the tokenizer state after a pause.
- WPT-like tests written for swb (the WPT `custom-elements` directory is
  not in the repository; the cases above follow its topics): the reaction
  order of each row of section 7's table, upgrades at `define` and at
  insertion, constructor failures, `whenDefined` timing.
- Integration: a test with the local `github-swb` fixture (skipped when it
  is absent) that checks that 74 definitions succeed, the 10 upgrades
  happen, and the callback counts of the table "What GitHub needs" match
  (with JavaScript on; the shadow roots of `relative-time` and `tool-tip`
  come with feature 5, so the test counts calls, not shadow content).
- JS off: `just snapshot` before and after sub-feature 4a (no change
  expected: section 1).

### The plan of sub-features

Three sessions. Each ends with a review and a commit (M10 rules). GitHub's
elements are upgraded and their callbacks run after 4b; 4c completes the
scope (GitHub's first load does not need it). Order (orchestrator's
decision, Consequences): 4a, 4b, then features 5 to 8, then 4c.

**4a. Definitions, constructors and upgrades** (`implementer-hard`).
Scope: sections 1 to 6 and 10 without the mutation enqueue points: the
element state and `:defined` (JS on and off) with the state journal entry;
the engine API (section 2); the registry and definitions with all of
`define`'s checks, reads and messages, `get`, `getName`, `whenDefined`,
`upgrade(root)`; [HTMLConstructor] for all element interfaces and the
`HTMLElement` constructor with the construction stack; "create an element"
with the synchronous flag for `createElement` and `createElementNS`
(failure fallback, messages); the upgrade algorithm; the reaction state
(element queues with marks, backup queue and its microtask, invoke with
reports and the microtask checkpoint, the depth limit, the budgets);
`[CEReactions]` in the generator with the fast path; the upgrade
candidates at `define` with the arena's name set. The reactions that an
upgrade enqueues itself (attributes, connected) run in 4a.
Files: `crates/js/src/vm/call.rs`, `vm/convert.rs`, `vm/internal.rs`,
`host/mod.rs` (public API, flags); `crates/dom/src/tree.rs` (state,
definition, name set), `journal.rs` (`Change::State`), `mutation.rs`
(creation states); `crates/style/src/element.rs`; `crates/webidl/src/
model.rs`, `emit/iface.rs` (`[CEReactions]`, [HTMLConstructor]);
`crates/web/idl/html.idl` (the registry, `Window.customElements`); new
`crates/web/src/custom_elements/` (`mod.rs` registry and `define`,
`constructor.rs`, `create.rs`, `upgrade.rs`, `reactions.rs`, `names.rs`);
`state.rs` (tables, trace), `dom/document.rs` (createElement),
`html/classes.rs` (the name check moves to `swb_dom`); the generated
bindings (`just bindings`); `crates/engine/src/document/changes.rs` (the
state entry's invalidation).
Tests: the probe cases `api-*`, `define-*`, `get-and-getname`,
`when-defined`, `upgrade-*` (without the `importNode` row),
`create-element-*`, `constructor-defines-another`, `defined-*`,
`shim-reflect-construct`, `derived-classes`, `define-in-module-before-dcl`,
`reactions-depth-limit` (the constructor and attribute rows);
unit tests; hostile cases `ce-upgrade-100000`, `ce-define-chain`,
`ce-define-100000-names`, `ce-observed-attributes-100000`,
`ce-when-defined-1000000`; the binding rows of section 11.
Exit: the cases match Chromium (departures listed); GitHub's 74 definitions
succeed and its 3 upgrades at `interactive` and 7 after `load` happen (by
`--eval` counts); the targets keep their scores with JavaScript on and off
(BBC and Wikipedia now see `customElements`); JS-off snapshot unchanged;
`just check`.
Risk: high (new machinery in four crates; the generator change touches
every `[CEReactions]` member; the fast path must not cost the targets).

**4b. Reactions of DOM operations** (`implementer`).
Scope: section 7: the sink dispatcher and the reaction state's sink part
(insertion with try-upgrade, removal, attributes through every funnel,
adoption with the new sink report, also across arenas), the reactions of
clones, `importNode` and the fragment parser's copy, the grouping order,
the re-keying at a transplant, the sink's work count, the subtree filter;
the performance measurement of section 11.
Files: `crates/dom/src/journal.rs` (`MutationSink::adopted`), `mutation.rs`,
`algorithms.rs` (clone), `transplant.rs`; `crates/web/src/
mutation_registry.rs` (the dispatcher), `custom_elements/reactions.rs`,
`dom/markup.rs`, `dom/mod.rs` (clone), `dom/document.rs` (`importNode`,
`adoptNode`), `state/moved.rs`; `docs/performance.md`.
Tests: the probe cases `reactions-*`, `fragment-parsing`,
`clone-and-import`, the `importNode` row of `upgrade-at-define`; hostile
cases `ce-connected-recursion`, `ce-reinsert-loop`, `ce-attribute-flood`,
`ce-disconnect-100000`; the GitHub fixture test (callback counts).
Exit: the cases match Chromium; GitHub's callback counts equal the replay's
(connected 35, attributeChanged 76); the
budgets of section 11 hold; the targets keep their scores; `just check`.
Risk: medium (many entry points; the order of the element queue; adoption
across arenas).

**4c. Synchronous construction in the parser** (`implementer-hard`).
Scope: section 8: the placeholder, the recorded insertion, the token sink
wrapper and `ParseStep::CustomElement` / `WrittenStep::CustomElement`, the
alias table, the steps of "create an element for a token" in the web layer
(counter, microtask checkpoint, element queue, attributes), the pause for
reactions of other parser insertions, the throw-on-dynamic-markup-insertion
counter in `document.write`, `open` and `close`.
Files: `crates/dom/src/parser.rs`, `parser/sink.rs`, `parser/write.rs`,
new `parser/token_sink.rs`; `crates/web/src/document_load.rs`,
`document_write.rs`, `custom_elements/create.rs`; `crates/engine/src/
document/mod.rs` (`parse_step`, `parse_written_step`), `crates/web/src/
host.rs` (`EngineHost` methods for the finish step).
Tests: the probe cases `parser-sync*`; parser unit tests (trees with and
without pauses equal; tokenizer state after `<textarea>`, `<script>`,
`<plaintext>`, `<svg>`, tables with foster parenting, `<select>`); hostile
cases `ce-parser-100000`, `ce-write-in-constructor`; the JS-off and JS-on
targets (the parser's fast path: no definition, no pause).
Exit: the cases match Chromium; no target changes; the parse time of the
fixtures with JavaScript on within 1 % (the alias fast path and the
per-token cell read); `just check`.
Risk: medium-high (html5ever's tokenizer state after an early return;
recorded insertions in foster parenting and the adoption agency).

## Open questions

The orchestrator decided them on 2026-11-08 (see Consequences): 1, 2, 3, 4,
6 and 7 as recommended; 5 with a change of order.

1. **`define` with a valid `extends`.** Throw `NotSupportedError`
   ("Customized built-in elements are not supported", as Safari does) or
   accept the definition and never create such elements? Recommendation:
   throw. Pages that use customized built-ins feature-test with `define` in
   a `try` and load a polyfill; an accepted definition that never upgrades
   breaks them silently. GitHub uses none.
2. **Chromium's depth limit of 43.** Copy it (drops without a report) or
   report a `RangeError` at a swb limit? Recommendation: copy it; it is
   measured in three shapes, it is the bound that keeps hostile recursion
   cheap, and pages that hit it behave as in Chromium.
3. **Scoped registries.** Chromium 148 has them; GitHub does not use them.
   Recommendation: out of M10, backlog item; `CustomElementRegistry` has no
   constructor in swb.
4. **`ElementInternals` and form-associated custom elements.**
   Recommendation: backlog; read the flags at `define` in Chromium's order
   (section 9), no `attachInternals`.
5. **Three sessions or two.** 4b could join 4a (one large session) or 4c
   could wait until a target needs it. Recommendation: three sessions in
   this order; 4c stays in M10 because the scope names synchronous
   construction in the parser, and it is independent of features 5 to 9.
6. **A bound function as the constructor.** Recommendation: follow HTML
   (`TypeError`), a listed departure; Chromium's acceptance has no use
   (the element can never be constructed).
7. **Budgets of section 10** (65,536 definitions, 16 MiB of observed names,
   65,536 pending `whenDefined` names, 128 MiB of queued reactions).
   Recommendation: accept; Chromium has none, and the values are far above
   GitHub's (74 definitions, 74 observed names).

## Consequences

- `customElements`, `CustomElementRegistry` and a working `HTMLElement`
  constructor appear for every page with JavaScript on. Pages that tested
  for them take other paths: BBC's video player code and Wikipedia's Vue
  compiler (the targets' scores are the check).
- `:defined` stops matching elements with custom element names before their
  upgrade, also with JavaScript off (as in Chromium; no fixture changes).
- The generator honors `[CEReactions]` and [HTMLConstructor]; ADR 0028
  section 5's "`[CEReactions]` is ignored (no custom elements)" no longer
  holds.
- `ElementData` carries the custom element state and definition; the DOM
  gets a state journal entry and an adoption report to its sink.
- The parser can pause after any token (custom elements, reactions), not
  only at `script` end tags.
- The engine's public API grows by `construct`, `is_constructor`,
  `set_prototype_of` and a second host flag; the interpreter does not
  change.
- Departures from Chromium: customized built-ins, scoped registries,
  `ElementInternals`, form-associated custom elements, `moveBefore`, the
  bound-function case, the budgets of section 10.
- Backlog: customized built-in elements; scoped custom element registries;
  `ElementInternals`, form-associated custom elements and `:state()`;
  `moveBefore` and `connectedMoveCallback`.
- Orchestrator decisions on the open questions (2026-11-08):
  - A valid `extends` throws `NotSupportedError` ("Customized built-in
    elements are not supported").
  - The nesting limit of 43 copies Chromium's (re-measured by the
    orchestrator: 43 constructors, 43 attribute callbacks, 43 re-insertions,
    also with 100 extra stack frames per level, while plain recursion
    reaches 200).
  - Scoped registries, `ElementInternals` and form-associated custom
    elements go to the backlog.
  - A bound function as the constructor follows HTML (`TypeError`), a
    listed departure.
  - The budgets of section 10 are accepted.
  - Three sub-features, in the order 4a, 4b, then features 5 to 8, then 4c.
    GitHub's first load does not need synchronous construction in the
    parser, and 4c is independent of features 5 to 8, so the page's
    progress comes first. Until 4c, an element that the parser creates
    with a defined name is upgraded at its insertion from the backup
    element queue (section 5). Its constructor then sees the attributes
    and the parent, where Chromium's sees neither (measured,
    `parser-sync`); 4c removes this difference. 4c moves earlier if
    feature 8 needs it.
