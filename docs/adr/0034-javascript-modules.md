# ADR 0034: JavaScript modules

- Status: accepted
- Date: 2026-11-07

## Context

M10 targets the front page of github.com/senko/swb with JavaScript on
(roadmap, M10; [github-diagnosis.md](../github-diagnosis.md)). The page has
9 module scripts, an import map and 111 `modulepreload` links. Its 133
JavaScript files (5.37 MB) are modules: the static graph of the 9 roots has
95 modules and 158 edges (regex count of the fixture), at most 2 levels
deep, with no cycle; 16 imports use bare specifiers that the import map
resolves (`react`, `react-dom`, `scheduler` and others); the other modules
are loaded with `import()` by the bundler's chunk loader (`wp-runtime`,
which also reads `import.meta.url`) and by React Router (`react-core`). No
file uses import attributes, `export *`, or a JSON or CSS module. The
largest module exports 45 names. Today swb refuses 8 of the 9 module
scripts (`not supported yet (module syntax)`), so none of the page's
scripts run.

What exists (ADR 0026, ADR 0028 section 6 item 7 and section 8 "Module
scripts"):

- The front end parses modules completely. `ParseModule` gives a
  `ModuleRecord` (`crates/js-syntax/src/module.rs`): the requests with
  their import attributes, the import entries, the local, indirect and
  star export entries, and `[[HasTLA]]`. The scope analysis makes every
  exported local binding a cell, gives an import binding no storage in the
  frame (`Storage::Import`) and lets the module function read the cell of
  import entry *i* through a capture (`CaptureSource::Import(i)`).
  `export default` of an expression binds `*default*`.
- `Runtime::eval_module_at` compiles a module without requests as a
  function (`CodeKind::Normal`, strict) and runs it in a frame without a
  function object. The compiler refuses `import`, `export`, `import()`,
  `import.meta` and top-level `await` (`compiler/support.rs`).
- Generators and async functions suspend their frame into a generator
  object (`vm/generator.rs`, `vm/async_function.rs`); `await` is a promise
  reaction that resumes the frame in a job.
- The web layer has a module map per document (one entry per URL with the
  fragment), the lists of HTML "prepare the script element" for module
  scripts, inline modules (`Task::ModuleReady`), the module fetch
  (`EngineHost::fetch_script` with `ScriptFetchMode::Module`: CORS mode
  `cors`, credentials `same-origin` or `include`, the JavaScript MIME type
  check, UTF-8 decoding) and the `load` and `error` events
  (`crates/web/src/scripts/modules.rs`).
- test262: the runner skips every test with the flag `module`
  (`crates/js/src/test262/run.rs`); the parse mode parses them.

Sources for this ADR: ECMA-262 2025 §16.2 (modules), §10.4.6 (module
namespace exotic objects), §13.3.10 (`import()`), §13.3.12 (`import.meta`);
the HTML Standard (§8.1.5 "module scripts", "fetch the descendants of and
link a module script", "HostLoadImportedModule", "resolve a module
specifier", "import maps", "merge existing and new import maps", "run a
module script", the link type `modulepreload`); study memos 2 ("Script
versus module top level"), 3.4 (module namespaces) and 5.3 ("Modules");
measurements in Chromium 148 (below). The session that wrote this ADR read
no engine or browser source code.

### Measurements

New probe file `tools/probes/web-module-graph.json` (25 cases, the logging
header of `web-module-scripts.json` plus `load` and `error` of `link`
elements and `unhandledrejection`). Chromium 148, four runs (one before a
change of one case, then three with the final file); the three final runs
are equal apart from the order of two `link` events in `modulepreload-bare`.
Values from the earlier file `web-module-scripts.json` (M9 feature 7) are
marked "M9". `L` is the log of the case: `name:readyState[:currentScript]`.

| Question | Case | Chromium |
|---|---|---|
| A deferred graph with a slow dependency, then a deferred module with a dependency, then a `defer` classic script | `graph-order-defer` | `slow m1 f2 fast d dcl load`: document order; each graph runs when it and the graphs before it are complete |
| An async graph (slow dependency) against a deferred graph | `graph-async-vs-defer` | `b-dep b d dcl a-dep a load`: the async graph runs when it is complete, after `DOMContentLoaded` here; the window `load` waits for it |
| An async graph that completes first | `graph-async-fast` | `a-dep a b-dep b dcl load`: it runs before the deferred graph and before `DOMContentLoaded` |
| A dependency shared by three graphs; `./sub/../shared.js`; `./shared.js#f` | `graph-shared` | `shared` runs once for the three spellings of one URL; the URL with `#f` is a second module and runs again; one request for `shared.js` (Chromium's memory cache answers the `#f` URL) |
| A descendant fails to fetch (404, two levels deep; `text/plain`) | `descendant-fetch-error` | `error` at the root element; no `window.onerror`; the root does not run |
| A descendant has a syntax error (external and inline root) | `descendant-parse-error` | `onerror` with the `SyntaxError` at the position in the descendant (`bad.js:2:9`), once per root; then `load` at an external root |
| A descendant throws; two roots import it | `descendant-eval-error` | the descendant runs once; its error reaches `onerror` once per root (the same error again); `load` at each root |
| A missing export; conflicting star exports | `link-error-external`, M9 `import-bad-name` | `SyntaxError: The requested module './dep.js' does not provide an export named 'zz'` and `... './star.js' contains conflicting star exports for name 's'`, at the importing module, line and column of the imported name (`x.js:1:9`); `load` at the root |
| A bare specifier without an import map | `resolve-error` | `TypeError: Failed to resolve module specifier "bare-x". Relative references must start with either "/", "./", or "../".` at the document URL, 0:0; `load` at an external root |
| A dependency with top-level `await` on a timer | `tla-dependency` | `a-before m2 d dcl load a-after m1`: neither the next deferred scripts, nor `DOMContentLoaded`, nor `load` wait for the awaited part |
| Top-level `await` rejects (root; dependency) | `tla-reject` | `onerror` with the error, at the time of the rejection (after `load` for the later one); no `unhandledrejection`; `load` at the root fired before |
| Three siblings, two with `await` | `tla-siblings` | `a1 b c1 a2 c2 m1` |
| A cycle with a hoisted function and a `let` | `cycle` | `b:fa:ReferenceError: Cannot access 'a' before initialization`, `a:B:fb`, `m1:A` |
| Live binding, assignment to an import, namespace keys | `live-binding` | `c1`, `c2:2`, `TypeError: Assignment to constant variable.`, keys `c,inc`, `Module` |
| Namespace object from `import()` | `import-call-namespace` | `[object Module]`; keys `a,b,default`; not extensible; prototype `null`; `{writable: true, enumerable: true, configurable: false}`; `TypeError: Cannot assign to property 'b' of [object Module]`; `TypeError: Cannot delete property 'b' of [object Module]` |
| Import map with a prefix and a scope; `import.meta.resolve` | `importmap-basic` | all resolve as HTML says; `resolve` gives the mapped URL |
| An import map after an external module script and after an inline one | `importmap-after-module` | the external module (fetch still in flight) resolves its bare import with the later map; the inline module failed at once (`TypeError` at the document URL 0:0); a module after the map works |
| Two import maps with one conflicting key | `importmap-merge` | the first map wins for `x`; the second adds `y` |
| Invalid JSON; a non-string address; `src` | `importmap-errors` | `onerror`: `SyntaxError: Failed to parse import map: invalid JSON` at the document URL 0:0, during the parse; the bad address is ignored (no error event); `<script type=importmap src>` fires `error` at the element |
| An import map inserted by a script | `importmap-inserted` | applies |
| `modulepreload` of a module with descendants, of a 404 and of `text/plain` | `modulepreload` | fetches the URL only (no descendant); does not run it; `load` or `error` at the `link`, after the window `load` (it does not delay it) |
| `import()` of a preloaded module later | `modulepreload-then-import` | no second request for the preloaded module; its descendant is fetched then |
| `modulepreload` with `href` of a bare specifier | `modulepreload-bare` | `href` is a URL, not a specifier: `x` is requested (404, `error`) |
| `import()` base URL: external classic script of another origin with and without `crossorigin`, inline classic script, event handler attribute | `import-call-base` | with `crossorigin`: the script's URL; inline and handler: the document's base URL; without `crossorigin`: rejected at once with `TypeError` (before the script's `load`) |
| `import()` failures | `import-call-errors` | bare: `TypeError: Failed to resolve module specifier 'bare'`; syntax error: the `SyntaxError`; 404: `TypeError: Failed to fetch dynamically imported module: URL`; a throwing module: its error, the same for each call |
| `import()` from a classic script; `import.meta` | M9 `import-call-classic`, `import-call`, `import-meta` | the import runs after `load` (its fetch does not delay `load`); `import.meta` keys `url,resolve`, `url` of an inline module is the document URL |

The probe tool serves several files per case (`resources`), but not in
subdirectories: `_serve_case` writes each resource directly into the case
directory. The cases use a second origin (`$OTHER`) and scopes keyed by a
file URL instead. Proposed extension (not built): create the parent
directories of a resource name with a `/`.

**Compile cost.** The front end compiles modules as it compiles scripts.
The fixture's 133 files with the module syntax removed by a script (import
and export declarations deleted, `import.meta` replaced) compile with
`swb-js bench-compile` (release build of afb8b8d, idle machine, load 1.5,
best of three): 130 files of 4.38 MB in 0.214 s, 22,829 functions, 34 MB
of code objects (7.8 bytes per source byte). Three files did not compile
because the removal broke them. The same files wrapped in a strict `async
function` (top-level declarations become function bindings, as in a module)
compile in 0.211 s, so the register limits of a function are no problem
for these modules. Estimate for all 5.37 MB: 0.26 s of parse and compile,
42 MB of code in the heap. This is 17 % of the load budget (1.56 s).

## Decision

### Part 3a: the engine

#### 1. Module records and the host interface

A Source Text Module Record is a heap object (`Gc<Generic>` data, traced)
in a new module `crates/js/src/module/` (`mod.rs` records and API,
`load.rs`, `link.rs`, `evaluate.rs`, `namespace.rs`, `dynamic.rs`). The
public handle is `Module` (a copyable wrapper of the heap handle). A record
holds the fields of Module Records, Cyclic Module Records and Source Text
Module Records (§16.2.1.4 to §16.2.1.7) that swb needs: the realm, the
code object, the requests and entries of the front end's `ModuleRecord`,
`[[LoadedModules]]` (one slot per request), the status (`new`,
`unlinked`, `linking`, `linked`, `evaluating`, `evaluating-async`,
`evaluated`), the DFS indices, `[[EvaluationError]]`, the async fields
(`[[CycleRoot]]`, `[[HasTLA]]`, `[[AsyncEvaluation]]` with its order,
`[[TopLevelCapability]]`, `[[AsyncParentModules]]`,
`[[PendingAsyncDependencies]]`), the environment (below), the namespace
object and the `import.meta` object (both made on first use), and the
host-defined value (a `u32` that the host chooses).

API (names are proposals; the implementer may adjust them):

- `Runtime::parse_module(realm, source, name, start, host_defined) ->
  Result<Module, ScriptError>`: ParseModule and the compile of the whole
  module. A syntax error is `ScriptError::Compile`, as for scripts.
- `Runtime::module_requests(module)`: the specifiers and attributes, in
  source order.
- `Runtime::set_module_hooks(ModuleHooks { load_imported_module,
  import_meta_properties })`: function pointers, as `set_lazy_hooks`.
  `load_imported_module(rt, LoadRequest) -> LoadAnswer` is
  HostLoadImportedModule (§16.2.1.10). The request names the referrer
  (`Referrer::Module(Module)`, `Referrer::Script(host_defined)` or
  `Referrer::Realm(realm)`), the specifier, the attributes, whether it is
  dynamic, and a `u32` token. The answer is `Now(Result<Module, Value>)`
  (the module map has the result) or `Later` (a fetch started). A hook runs
  no script.
- `Runtime::finish_loading_imported_module(token, Result<Module, Value>)`:
  FinishLoadingImportedModule (§16.2.1.11) for an answer that came later.
- `Runtime::load_requested_modules(module) -> Value`: LoadRequestedModules
  (§16.2.1.6.1.1); the result is a promise. The host attaches its reaction
  with the existing `Runtime::react_to_promise`.
- `Runtime::link(module) -> Result<(), Value>`: Link (§16.2.1.6.1.2); the
  error is the `SyntaxError` object.
- `Runtime::evaluate(module) -> Result<Value, ScriptError>`: Evaluate
  (§16.2.1.6.1.3); the result is the promise. Without top-level `await` in
  the graph it is settled when the call returns
  (`Runtime::promise_settlement`). A termination is `ScriptError::Terminated`.
- `Runtime::module_namespace(module)`: GetModuleNamespace.

The engine keeps the state of each graph load (GraphLoadingState) and the
pending tokens in a table of the runtime, which is a root, so the host
holds only numbers. A token that the host never answers (the document went
away) stays until the runtime ends; the module map limit of section 11
bounds them.

Errors come back as values: a load error is the value that the host gave
(a `TypeError` or the `SyntaxError` of a descendant), a link error is a
`SyntaxError` whose location (for `Runtime::error_location`) is the
imported name in the importing module (Chromium: `x.js:1:9`, the column of
`zz` in `import {zz} from`). The front end adds the offset of the name to
each import and export entry for this. The messages are Chromium's (table
above).

Reasons: the host answers HostLoadImportedModule after a network fetch, so
the engine must keep the load state between calls (memo 5.3). One
implementation of LoadRequestedModules serves the script elements and
`import()`. `LoadAnswer::Now` avoids a call of `finish_loading` from inside
the hook (HTML's module map answers a known URL at once). Heap records die
with their last reference, so they do not block realm collection (section
5).

Rejected: a synchronous loader that returns source text (QuickJS's design;
the network is asynchronous); a graph walk in the host that only tells the
engine the edges (`import()` needs the engine's walk anyway, and the
spec's walk handles cycles and errors in one place); records in a vector
of the runtime with indices (roots for the runtime's life, no way to free
the records of a removed frame document).

#### 2. Bindings, environments and the module function

A module is compiled as one function of a new code kind `Module`. Its
top-level declarations stay registers or cells of that function, so the
register limits of ADR 0028 section 6 item 7 stay (about 64,500 declared
bindings; GitHub's modules compile, measured above).

- **Exported cells.** The record has an environment: one cell per local
  export binding (in the order the compiler gives them). The linker makes
  a cell when it first needs it (ResolveExport gives the module and the
  binding; InitializeEnvironment of an importer; a namespace object). It
  holds `Empty` (the TDZ marker). At scope entry the module code does not
  make new cells for these bindings: a new instruction `ModuleCell { dst,
  index }` puts the environment's cell into the binding's register. Code
  after that is unchanged: `LoadCell`, `StoreCell`, closures that capture
  the register.
- **Import bindings** are captures of the module function
  (`CaptureSource::Import(i)`): the cell of the exporter's binding, which
  the linker resolves through indirect exports and star exports. An import
  and the exported binding are the same cell, so the binding is live (memo
  2). Loads check the TDZ (the cell can be `Empty` in a cycle: Chromium's
  `ReferenceError: Cannot access 'a' before initialization`). An assignment
  to an import is the `TypeError` of a `const` (`Assignment to constant
  variable.`, measured). A namespace import (`import * as ns`) gets a new
  cell that holds the namespace object. `export * as ns from "m"` and a
  re-export of a namespace import resolve to one namespace cell per
  module, made on first use.
- **`export default`.** `export default function` and `export default
  class` are declarations with the name `default` (the function's `name`
  is `"default"`). `export default expression` stores into the `*default*`
  cell when the statement runs (anonymous functions and classes get the
  name `"default"` through NamedEvaluation).
- **InitializeEnvironment** (§16.2.1.7.3.1) runs during Link. It resolves
  the import entries, makes the module's closure (a function object that
  no script sees, with the import cells as captures), and calls it. The
  prologue of the module function puts the environment cells in place,
  sets `var` bindings to `undefined`, `let`, `const` and `class` bindings
  to `Empty`, and creates the hoisted function declarations. Then an
  initial suspension (the generator's `InitialYield`) moves the frame into
  a generator object of the record. Evaluate resumes it.
- **Namespace objects** (§10.4.6) are a new object kind
  `ModuleNamespace` with the exotic internal methods in the runtime's
  generic paths (as for proxies and `arguments`): a sorted list of export
  names with their cells, `[[Prototype]]` null, not extensible,
  `@@toStringTag` `"Module"`. A read of an `Empty` cell throws the
  `ReferenceError`. The inline caches treat them as uncacheable. Messages
  are Chromium's (`Cannot assign to property 'b' of [object Module]`,
  `Cannot delete property 'b' of [object Module]`).

Reasons: the scope analysis already gives the cell layout that live
bindings need; the generator machinery already moves a frame into an
object and back; hoisted functions of a module in a cycle must exist
before any module of the cycle runs (the `cycle` case: `b` calls `fa`
before `a` runs), which the suspension after the prologue gives.

Rejected: a module environment object read by name (new instructions on
every access, slower than cells, and the closures of the module would
need it too); copies of the values at link time (not live); two functions
per module, one for the declarations and one for the body (every
top-level binding would have to be a cell shared between them); host
hooks for namespace objects (they are language semantics, needed in
`swb-js` and test262 without a browser).

#### 3. Top-level `await`

A module with `[[HasTLA]]` runs its body as the body of an async
function. Evaluate makes the capability and resumes the module's generator
object; the instruction after the initial suspension takes the role of
`AsyncStart` with that capability, `await` uses the existing reactions and
jobs, the end of the body resolves the capability, and a handler over the
body rejects it (as `AsyncReject`). A module without `[[HasTLA]]` runs to
its end in the resume; its completion is the return or the throw.

The async evaluation follows §16.2.1.6.1.3 and its helpers exactly:
InnerModuleEvaluation with `[[PendingAsyncDependencies]]`,
`[[AsyncParentModules]]`, the order counter of `[[AsyncEvaluation]]`,
ExecuteAsyncModule, GatherAvailableAncestors (sorted by the order),
AsyncModuleExecutionFulfilled and AsyncModuleExecutionRejected. The
reactions on the capability are internal continuations (the kind that
`await` uses). The `tla-siblings` case (`a1 b c1 a2 c2 m1`) and the
test262 directory `language/module-code/top-level-await` check the order.

Rejected: running the job queue inside Evaluate until the module settles
(it changes the order of jobs against other tasks, and an `await` on a
timer would block the task); a scheduler of its own for module jobs.

#### 4. `import()`, `import.meta`, import attributes

- **`import()`** (§13.3.10): a new instruction evaluates the specifier and
  the options, makes the promise, and starts a load with the referrer of
  the running code. Each code object records its Script or Module record
  (a module's code objects point to the module record; a classic script's
  code objects point to a small Script record heap object with the
  host-defined value). Code without one (an event handler attribute,
  `setTimeout` with a string compiled without a script) gives
  `Referrer::Realm`. When the load answers, ContinueDynamicImport loads the
  rest of the graph, links, evaluates and resolves the promise with the
  namespace object, or rejects it. `Runtime::eval_named_at` and
  `compile_event_handler` get the host-defined value.
- **`import.meta`** (§13.3.12): an instruction that gives the record's
  meta object; the first use makes an object with a null prototype and
  calls the hook `import_meta_properties`, which defines `url` and
  `resolve` (HTML). Measured: keys `url,resolve`.
- **Import attributes.** The engine supports the key `type`
  (HostGetSupportedImportAttributes); the parser already turns the `with`
  clause into sorted attributes and checks the keys. A request with `type`
  fails to load in the host (`TypeError`, warning): swb has no JSON and no
  CSS modules. GitHub uses none (0 attribute clauses in its 133 files);
  JSON and CSS modules go to the backlog.

#### 5. Garbage collection

The roots of module data:

- The host's module map holds the records of its document (HTML: for the
  document's life). The web state traces them in its `HostState::trace`.
- Records reference each other through `[[LoadedModules]]`; code objects
  reference their record; namespace objects reference the cells and the
  record; the pending loads are in the runtime's table (a root); the
  async reactions hold their records as continuation data.
- A record references its code object, its closure or suspended generator
  object, its environment cells, the namespace and meta objects, the
  evaluation error, the capability and its async parents.

When a document goes, the host drops its map. Its records then die with
the last script object that reaches them. This does not change the
realm-collection backlog item (roadmap, "Backlog from M9"): a record has
the index of its realm, and realm collection must count records as
objects of the realm, so a namespace object that another same-origin
document keeps also keeps the realm. Stress-mode tests (ADR 0026 section
8) cover the load, the link with cell creation, and the suspended module
frames.

#### 6. Limits and robustness

- Every graph algorithm is iterative with explicit stacks: InnerModuleLoading,
  InnerModuleLinking, InnerModuleEvaluation (Tarjan's DFS indices on a
  `Vec` of frames), GatherAvailableAncestors, ResolveExport (its
  `resolveSet` as a hash set) and GetExportedNames (`exportStarSet`). A
  chain of 10,000 modules uses no Rust stack per module; each module body
  runs from the evaluation loop, not nested in the previous one.
- ResolveExport is memoized per (module, export name); ECMA-262 requires
  that it gives the same answer each time, so the cache is safe. With the
  cache, the link work is linear in the number of import and export
  entries plus the star-export edges per name. Namespace creation sorts the
  names once.
- The link and evaluation loops charge the time countdown per step, so a
  hostile graph (a mesh of `export *`, 100,000 export names) ends at the
  task deadline with a termination, never a hang.
- Heap accounting: records charge their vectors (entries, loaded modules,
  environment cells, async parents); namespace objects their export list;
  the source text and the code objects are charged as for scripts. A
  termination during Evaluate marks the modules on the DFS stack as
  evaluated with an internal "terminated" error: they never run again and
  report nothing more.

#### 7. Tests

- Unit tests per file of `crates/js/src/module/`; integration tests
  `crates/js/tests/module_graph.rs` with an in-memory loader (cycles,
  live bindings, TDZ, star exports and ambiguity, namespaces, default
  exports, errors and their locations, TLA order, `import()`,
  `import.meta`), also in the GC stress mode; `crates/js/tests/modules.rs`
  keeps its cases.
- `swb-js --module FILE`: runs a file as a module, with a loader for
  relative specifiers from the file's directory (no paths outside it).
- test262: the runner loads a test with the flag `module` as a module and
  its imports from the test's directory (`_FIXTURE.js` files); script tests
  of `import()` use the same loader. Requests leave the test directory
  only through `../` within the test262 tree. The runner answers the load
  requests and runs the jobs until nothing is pending; negative tests with
  phase `resolution` pass when the load or the link fails with the type.
  New features in `crates/js/test262/subset.txt`: `top-level-await`,
  `dynamic-import`, `import.meta`, `export-star-as-namespace-from-module`,
  `arbitrary-module-namespace-names`, `import-attributes`. Out of scope:
  `import-defer`, `source-phase-imports`, `json-modules`, `import-text`,
  `import-bytes`. New directories: `test/language/module-code` (with its
  subdirectories), `test/language/import`, `test/language/export`,
  `test/language/expressions/dynamic-import`,
  `test/language/expressions/import.meta`.
- In-scope counts at the pinned commit (count of the checkout in
  `out/test262`, with the subset's feature and harness rules): static
  module tests 350 (`module-code` 343, `import` 4, `export` 3), top-level
  `await` 250 (`module-code`), `import()` 631 (`dynamic-import` 619,
  `module-code` 12), `import.meta` 22. Ratchet targets (to be replaced by
  the measured counts in `scores.json` at each sub-feature): static 330,
  top-level `await` 230, `import()` 590, `import.meta` 21.

### Part 3b: the host

#### 8. The module graph fetch

- **Module map.** The map of `Scripts::modules` stays one entry per URL
  with the fragment (HTML: the key is the URL and the module type
  `javascript`), for the document's life. An entry is: fetching (with the
  waiting loads and elements), fetched text that is not parsed yet, a
  module record, a parse error, or failed (null). The fetch layer shares
  one request between URLs that differ only in the fragment (Chromium's
  memory cache does the same; this removes a departure of M9).
- **Parse on first use.** A fetched text is parsed when a graph first needs
  it, not when it arrives. Only `modulepreload` fetches text that may never
  be used (29 of GitHub's 111 preloads are not in the static graph); the
  difference is not observable apart from time. HTML parses at the fetch;
  this is a deliberate departure for the budget.
- **HostLoadImportedModule** (the hook): resolve the specifier against the
  referrer's base URL with the import maps (section 9); a resolution error
  is a `TypeError` with Chromium's message (static: `Failed to resolve
  module specifier "x". Relative references must start with either "/",
  "./", or "../".`; dynamic: `Failed to resolve module specifier 'x'`). A
  map entry with a record answers `Now`; a fetching entry adds the token
  to its waiters; otherwise the hook starts a fetch and answers `Later`.
- **The fetch** reuses M9's: `EngineHost::fetch_module(id, url, options)`
  (a module fetch without a script element) and
  `ScriptRuntime::module_fetched(id, result)`. A descendant uses the
  root's options (HTML "descendant script fetch options": the credentials
  mode of the root's `crossorigin`). Mode `cors`, the JavaScript MIME type
  check, UTF-8 decoding, the queue of 64 script fetches and the 256 MiB of
  held text apply. The module's base URL is the response URL.
- **Graph completion and readiness** (HTML "fetch the descendants of and
  link"): the root's record calls `load_requested_modules`; when the
  promise settles, the host links in the same reaction; then the element
  is ready, with one of the results: the record (linked), an error to
  rethrow (a descendant's parse error, a resolution error, a link error),
  or null (a fetch failed). An element is ready as the single fetch of M9
  was ready, and the lists, `Task::ModuleReady` for inline modules without
  requests and the order rules of ADR 0028 section 8 stay.
- **Run** (HTML "run a module script"): with an error to rethrow, report it
  (no evaluation); else `evaluate` and, if the promise is rejected now or
  later, report the error. The report is the one of ADR 0028 ("Errors":
  `window.onerror`, the console in Chromium's form). Then `load` at an
  external element (also after an error); an inline element fires nothing;
  a null result fires `error` and reports nothing to `onerror`.

Which errors reach `window.onerror` (measured above):

| Failure | `onerror` | Element event | Notes |
|---|---|---|---|
| Fetch error of the root or a descendant (network, status, CORS, MIME) | no | `error` | console message as M9 |
| Syntax error of the root or a descendant | yes, at the bad module's position | `load` (external) | once per root that contains it |
| Resolution error | yes, `TypeError` at the document URL 0:0 | `load` (external) | |
| Link error (missing export, ambiguous star export) | yes, `SyntaxError` at the imported name | `load` (external) | |
| Evaluation error | yes, the thrown value | `load` (external) | the module runs once; the same error for each root |
| Rejection of top-level `await` | yes, when it rejects | `load` already fired | no `unhandledrejection` |
| `import()` failure | no (the promise rejects) | none | `unhandledrejection` if nobody handles it |
| Invalid import map JSON | yes, `SyntaxError` at the document URL 0:0 | none | |
| `<script type=importmap src>` | no | `error` | |

The M9 departure "an element after the first one reports no error if the
module threw or failed to parse" goes away: the record keeps its
evaluation error, and the host keeps the parse error in the map.

#### 9. Import maps, `modulepreload`, `import()` in the page

- **Import maps** (HTML "import maps"): `<script type=importmap>` inline
  only (`src` fires `error`); parsed with `serde_json`; `imports` and
  `scopes` with the normalization rules of HTML (specifier keys, URL-like
  keys, trailing `/` prefixes, invalid addresses ignored with a warning);
  invalid JSON reported as above. Several maps merge as HTML "merge existing
  and new import maps" says: a key that an earlier map has stays (measured
  `importmap-merge`), and a new rule that would change an already resolved
  specifier is ignored. A map applies to every resolution after it is
  registered, also of graphs whose fetch is in flight (measured
  `importmap-after-module`); an inline module resolves its requests when it
  is prepared. Maps inserted by scripts apply. Resolution looks up the
  prefixes of the specifier that end in `/` in a hash map, so it costs
  O(segments) and not O(entries). `import.meta.resolve` uses the same
  function.
- **`modulepreload`** (`link rel=modulepreload`, JavaScript on only): one
  module fetch of the `href` URL (not a specifier) into the module map,
  with the `link`'s `crossorigin`; no descendants, no run (measured). `load`
  or `error` at the `link` in a task after the fetch; it does not delay
  the window `load` event. A later graph uses the entry without a request.
- **`import()` in the page**: the referrer's base URL is the module's URL,
  the classic script's URL (external) or the document's base URL (inline
  script, event handler, a string timer). An `import()` from a
  cross-origin classic script without `crossorigin` is rejected at once
  with a `TypeError`, as in Chromium (measure the message), with no
  request. HTML would fetch with credentials `include` (the CORS check
  then fails for `*`). The orchestrator chose Chromium's behaviour: it is
  simpler and sends no credentialed request from a script with muted
  errors; no target depends on it. The fetches of `import()` do not delay
  `load`.

#### 10. Order in the event loop

- A module fetch completes in a networking task (ADR 0028 section 7, step
  1 of `Page::pump`). In that task the host parses the module if a graph
  waits for it, answers the waiting loads, and, when a graph's promise
  settles, links it in the microtask checkpoint of the task and marks the
  element ready. A parser-inserted module without `async` waits in the
  list of scripts that run after the parse; an async one runs in a task of
  its own, as M9's async scripts (measured `graph-async-fast`: before
  `DOMContentLoaded` when it is ready first).
- `DOMContentLoaded` waits for the deferred module graphs (their load and
  the start of their evaluation), not for top-level `await`.
- The window `load` event waits for the graphs of async and inserted
  module scripts, not for `modulepreload`, not for `import()`, and not for
  top-level `await` (measured).
- The 9 module scripts of GitHub are parser-inserted modules without
  `async` (`defer` has no effect): they run in document order after the
  parse, each after its graph is complete.

#### 11. Budgets

| Resource | Limit | Result |
|---|---|---|
| Module map entries per document (also failed ones) | 65,536 (Chromium: none) | the load fails as a fetch error (`error` event, or a `TypeError` for `import()`), one warning per document |
| Module fetches in flight | the 64 script fetches of ADR 0028 section 16 | queued |
| Fetched module text not yet parsed | the 256 MiB of held script text | the fetch fails as a network error |
| Parsed source and code objects | the JavaScript heap (256 MiB) | termination; scripts stop |
| Import map entries of a document (after the merge, `imports` and all scopes) | 65,536 | later entries are ignored, one warning |
| Pending load tokens of a runtime | bounded by the map entries and the fetch queue | |
| Link and evaluation work | the task's time limit (5 s) through the countdown | termination |

Time: GitHub's modules cost about 0.26 s of parse and compile (measured
above, 17 % of the 1.56 s budget), spread over the fetch tasks; the
records and the link are linear in 95 modules and 158 edges. Code objects
take about 42 MB of the heap. The evaluation time of the module bodies and
of the page's later work is measured in feature 8.

### The plan of sub-features

Five sessions, in this order. Each ends with a review and a commit (M10
rules). The engine parts touch only `js-syntax` and `js`, so they cannot
break a target until 3b-1 switches the web layer.

**3a-1. Static module graphs** (`implementer-hard`).
Scope: module records and the API of section 1 (without `import()`);
LoadRequestedModules with the hook; Link with ResolveExport (indirect,
star, ambiguity, memoized), GetExportedNames, InitializeEnvironment;
synchronous Evaluate with cycles and cached errors; the code kind
`Module`, `ModuleCell`, import captures, the initial suspension;
`export default`; namespace objects; link error messages and locations
(entry offsets in `js-syntax`); `swb-js --module`; the test262 runner's
module loader and the static directories. `Runtime::eval_module_at` runs
a module without requests through the new path (so a module with only
`export` runs, as in Chromium's `export-const` case); modules with
requests, `import()`, `import.meta` or top-level `await` stay refused in
the web layer.
Files: `crates/js-syntax/src/module.rs`, `parser/modules.rs`;
`crates/js/src/module/` (new), `compiler/function.rs`, `compiler/support.rs`,
`compiler/statement.rs`, `bytecode/mod.rs`, `bytecode/verify.rs`,
`vm/interp.rs`, `vm/generator.rs`, `object/` (namespace kind), `heap/gc.rs`,
`runtime.rs`, `main.rs`, `test262/run.rs`, `crates/js/test262/subset.txt`,
`scores.json`; tests `crates/js/tests/module_graph.rs`.
Tests: unit and integration tests, stress mode, test262 static
directories, the probe values of `cycle` and `live-binding` as engine
tests. Exit: test262 static module tests at least 330 of 350; all other
scores unchanged; `just check`.
Risk: high (the most new code: about 3,000 lines). The suspension of a
module frame between link and evaluation is new for the generator
machinery.

**3a-2. Top-level `await`** (`implementer-hard`).
Scope: section 3: the async module body, the capability, the async
evaluation algorithms, rejections, `[[CycleRoot]]` and error propagation
to async parents; termination during async evaluation.
Files: `crates/js/src/module/evaluate.rs`, `vm/async_function.rs`,
`vm/promise.rs`, `compiler/function.rs`; test262 subset.
Tests: `tla-siblings` and `tla-dependency` orders as engine tests; test262
`language/module-code/top-level-await`. Exit: at least 230 of 250 TLA
tests; `just check`. Risk: medium-high (order of jobs against the
specification; cycles with async modules).

**3a-3. `import()` and `import.meta`** (`implementer-hard`).
Scope: section 4: the instructions, Script records of classic scripts and
the referrer of code objects, the `host_defined` parameter of
`eval_named_at` and `compile_event_handler`, ContinueDynamicImport, the
import attribute checks and their rejections, the meta object and its
hook; the runner for `dynamic-import` and `import.meta` tests.
Files: `crates/js/src/module/dynamic.rs`, `compiler/expr.rs`,
`compiler/support.rs` (the last refusals go), `bytecode/mod.rs`,
`runtime.rs`, `host/`, `test262/`. Tests: test262 `dynamic-import` and
`import.meta`; engine tests for referrers (module, script, eval code,
`Function`, no script). Exit: at least 590 of 631 `import()` tests and 21
of 22 `import.meta` tests; `just check`. Risk: medium.

**3b-1. The module graph in the page** (`implementer-hard`).
Scope: sections 8, 10 and 11 without import maps and `modulepreload`:
the module map with records, `fetch_module` and `module_fetched`, the
hook with URL resolution (relative and absolute URLs only), descendant
fetch options, parse on first use, readiness, run and error reporting as
in the table, TLA rejections, the `import()` and `import.meta.url` hooks
(base URLs), the fetch sharing for fragments, the budgets; the web layer
stops using `eval_module_at`.
Files: `crates/web/src/scripts.rs`, `scripts/modules.rs` (split:
`scripts/module_map.rs`, `scripts/module_hooks.rs`), `document_load.rs`,
`state.rs` (trace), `crates/engine/src/document/scripts.rs`,
`document/mod.rs`, `resources.rs`.
Tests: the cases of `tools/probes/web-module-graph.json` without
`importmap-*` and `modulepreload*`, and the import cases of
`web-module-scripts.json` (their `MODULE_DEPARTURES` rows go), run against
`Sim` in `crates/engine/tests/probe_net.rs` (a generated
`probe_net/module_graph.rs`, as `module-cases` does); hostile cases
`module-chain-10000-js-on`, `module-cycle-ring-10000-js-on`,
`module-star-mesh-js-on`, `module-exports-100000-js-on`,
`module-import-call-urls-100000-js-on`, `module-tla-never-settles-js-on`.
Exit: the probe cases match Chromium (departures listed with reasons);
BBC's module script runs as before; all targets keep their scores with
JavaScript on and off; `just check`. Risk: medium (the order of tasks and
the readiness rules; many error paths).

**3b-2. Import maps, `modulepreload`, GitHub** (`implementer`).
Scope: section 9: import maps (parse, register, merge, scopes, errors,
`import.meta.resolve`), `modulepreload`, the `import()` base URLs of
classic scripts and handlers in the page. Then the GitHub check: the 9
module scripts load their graphs and run in document order.
Files: `crates/web/src/scripts/import_map.rs` (new), `scripts.rs`,
`crates/engine/src/document/` (`link` processing), `resources.rs`.
Tests: the `importmap-*`, `modulepreload*` and `import-call-*` cases;
unit tests of the import map parser and the resolution against HTML's
examples; hostile case `importmap-entries-100000-js-on`; an automation
test with the local `github-swb` fixture (skipped when it is absent, as
the other fixture tests) that checks that the 9 module scripts ran in
Chromium's order and that no module failed to load, link or parse.
Exit: the milestone exit of feature 3: the 9 module scripts of GitHub run
in Chromium's order (the errors after that, for example the missing
`customElements`, belong to later features); BBC's module scripts keep
working; the other targets keep their scores with JavaScript on and off.
Risk: low to medium (the import map rules are long but mechanical).

## Consequences

- swb runs module graphs. GitHub's scripts start to run, and the next
  errors of the page (custom elements, shadow DOM) become visible.
- The engine gets a new code kind, three instructions (`ModuleCell`,
  `import()`, `import.meta`), a new object kind and the module records;
  the compiler's refusals of module syntax go away in 3a-1 and 3a-3.
- Each code object knows its Script or Module record. This adds one field
  to code objects and a small Script record per classic script.
- Module records are heap objects that the host's module map keeps; the
  realm-collection backlog item must count them as objects of their realm.
- Departures from HTML: a module's text is parsed on first use, not when it
  arrives; `import()` from a cross-origin classic script without
  `crossorigin` is rejected at once, as in Chromium. Departures from
  Chromium: the limits of section 11.
- Orchestrator decisions on the questions of the proposal (2026-11-07):
  five sub-features instead of two parts; JSON and CSS modules to the
  backlog; parse on first use and the two limits of 65,536 entries
  accepted; `import()` from a cross-origin classic script without
  `crossorigin` as in Chromium (section 9).
- Backlog: JSON and CSS modules (import attributes with `type`); the
  probe tool's resources in subdirectories.
- The test262 scores file gets five new directories, about 1,250 tests.

## 3a-1 implementation notes

Facts of the implementation (M10 sub-feature 3a-1) that differ from the
decision above or that the later sub-features need.

**API** (`crates/js/src/module/`, re-exported by `swb_js`):

- `Runtime::link(module) -> Result<(), ScriptError>`, not `Result<(),
  Value>`: Link runs the prologues of the module functions, so it can end
  with a termination. A link error is `ScriptError::Uncaught`; the
  `SyntaxError` object is `Runtime::last_value()`, and
  `Runtime::error_location` gives the imported name in the importing
  module. The modules of a failed link are `unlinked` again.
- `load_requested_modules` and `finish_loading_imported_module` return
  `VmResult`; `module_namespace` returns `Result<Value, ScriptError>`.
  Further: `module_host_defined`, `module_realm`. `Module::trace` reports
  a record to the host's tracer; the host keeps its records alive.
- `ModuleHooks` has only `load_imported_module`; `import_meta_properties`
  comes with 3a-3. The graph load has no `[[HostDefined]]` of its own: the
  hook sees the referrer (`Referrer::Module`), whose host-defined number
  the host chose in `parse_module`. `LoadRequest` has the specifier and
  the attributes as Rust strings (lone surrogates become U+FFFD). Two
  specifiers that differ only in lone surrogates reach the host as one
  string; the record keeps its loaded modules by request index, so the
  engine does not mix them. 3b-1 keys its module map by the resolved URL;
  it must measure which URL Chromium requests for such a specifier.
- The promise of `Runtime::evaluate` is marked as handled: the host
  reports its rejection (the rejection tracker does not). A graph whose
  evaluation was ended by a termination gives `ScriptError::Terminated`
  once; a later Evaluate of a module of it gives a promise that never
  settles.
- An import attribute with a key other than `type` fails the load with
  `SyntaxError: Invalid attribute key "k".` (Chromium 148's text,
  measured; the key error comes before the value error of `type`, also
  for `export ... from ... with {k2: 'v'}`). The host sees only
  requests with `type`. The shell's loader fails a `type` value other than
  "json" and "css" with `TypeError: "javascript" is not a valid module
  type.` (measured) and "json"/"css" with `Unsupported module type`
  (JSON and CSS modules are in the backlog); the web layer must do the
  same in 3b.
- `Runtime::eval_module_at` uses the record path (parse, link, evaluate)
  for a module without requests; a module with requests is refused with
  "not supported yet (module syntax)" at its first `import` or `export
  ... from`. `import()`, `import.meta` and top-level `await` are still
  refused by the compiler (`compiler/support.rs`).

**Engine:**

- The module function has `CodeKind::Module` (strict, no `prototype`).
  Its closure has the cells of the import entries that its code uses
  (the capture list of the scope analysis, `CaptureSource::Import`), then
  the environment cells; `Insn::ModuleCell { dst, index }` puts capture
  `index` (at or after the import captures; the verifier checks that) into
  the binding's register. The environment is the bindings of the local
  export entries, each once (`compiler::module_environment`, shared by the
  compiler and the record). The prologue stores `undefined` into exported
  `var` cells; lexical cells stay `Empty` from the linker.
- The suspended body is a generator object without prototype
  (`GeneratorKind::Sync`); Evaluate resumes it with
  `Runtime::run_suspended` from the evaluation loop. 3a-2 needs a kind
  that settles the capability (an `AsyncStart` after the initial
  suspension) and the status `evaluating-async` with the async fields,
  which the record does not have yet (`top_level_promise` is
  `[[TopLevelCapability]]`'s promise).
- Namespace objects are `ObjectKind::ModuleNamespace` next to
  `HostExotic`; `Runtime::host_hooks` gives the engine's hooks
  `NAMESPACE_HOOKS` for them, so every generic path that handles host
  objects with hooks (and the inline caches, which never store them)
  handles namespaces. `[[HasProperty]]` is a special case in `has_generic`
  (it must not read a binding in its dead zone). A namespace shows as
  `[object Module]` in messages (`TypeError: [object Module] is not
  extensible` for `Object.setPrototypeOf(ns, {})`, measured). The
  namespace cell of a module exists before its object; a work list makes
  the objects, so `export * as` cycles need no recursion.
- Export names are heap atoms, compared by handle. ResolveExport keeps the
  specification's shared resolve set; a result is memoized per (module,
  name) when no circularity cut in its computation hit an entry of the set
  from before the call (such a result equals the answer for an empty set).
- A link error object keeps one captured frame, the imported name, for
  `Runtime::error_location` (the host reports the error at `x.js:1:9`, as
  Chromium reports it to `window.onerror`). Its `stack` is the first line
  only, `SyntaxError: message`, with no `at` line (measured). The shell
  prints the `at` line itself from the location.
- Messages that Chromium 148 gives (all measured):
  - A pure write to an import binding (`late = 1`, a destructuring
    target, a `for` head, `eval('late = 1')`, in a closure) throws
    `TypeError: Assignment to constant variable.` and does no dead-zone
    check; a form that reads first (`late++`, `late += 1`, `late ??= 1`,
    `typeof late`) throws the `ReferenceError` in the dead zone. After
    initialization `??=` and `||=` do not assign.
  - A namespace binding `v` in its dead zone: `[[Get]]` (`ns.v`, `Reflect.get`,
    a Proxy without traps, `Object.create(ns).v`) throws `ReferenceError:
    Failed to read the 'v' property from 'Module': Cannot access 'v'
    before initialization`; `[[GetOwnProperty]]` and `[[DefineOwnProperty]]`
    (`getOwnPropertyDescriptor`, `hasOwn`, `values`, `entries`, spread,
    `assign`, `defineProperty`, `freeze`, `isFrozen`, and `keys`,
    `for-in` and `JSON.stringify` of a Proxy around ns) throw
    `ReferenceError: v is not defined`; the key enumeration of the
    namespace object itself (`Object.keys(ns)`, `for-in` over ns or an
    object with ns on its chain, `JSON.stringify(ns)`) throws `ReferenceError:
    Cannot access 'v' before initialization`.
    `Runtime::exotic_own_property_listed` and `own_enumerable_listed`
    give the third text.
  - `Object.defineProperty(ns, key, desc)` for a string or index key that
    the module does not export: `TypeError: Cannot redefine property: q`;
    a symbol key keeps `Cannot define property Symbol(s), object is not
    extensible`.
  - Not changed, differ from Chromium (backlog): the messages of circular
    re-exports (`Detected cycle while resolving name 'b' in './c2.js'`).
  - The measurements are the cases `msg-*` of
    `tools/probes/web-module-graph.json`; 3b-1 runs them in the page.
- Direct `eval` in module code sees the imports: the scope analysis
  captures every import of a module function with a direct `eval`, and the
  eval references of inner functions include imports (`js-syntax`
  `scope/analysis.rs`, `scope/dynamic.rs`).
- A termination during a graph load leaves that load pending (its
  promise never settles); during Link the modules on the stack return to
  `unlinked`.

**Tests and scores.** test262: the static module tests pass 350 of 350
(also with `--stress`); `subset.txt` has the features
`export-star-as-namespace-from-module`, `arbitrary-module-namespace-names`
and `import-attributes` and the groups `test/language/module-code` (with
`ambiguous-export-bindings`, `import-attributes`, `namespace`),
`test/language/import` and `test/language/export`; module tests that were
in other groups (eval code, classes, `Proxy` and others) raised those
counts. The robustness tests of `module_graph.rs` (chain and ring of
10,000 modules, a chain of 10,000 re-exports, a mesh of 200 modules with
40,000 star edges, 100,000 export names) each take well under a second
in the test profile. In the web layer, a module with `export` and no
request now runs; four rows of `MODULE_DEPARTURES` went (`export-const`,
`export-default`, `export-list`, `import-external`).

## 3a-2 implementation notes

Facts of the implementation (M10 sub-feature 3a-2, top-level `await`)
that differ from the decision above or that 3a-3 and 3b-1 need. The
algorithms follow ECMA-262 2025 §16.2.1.6.1.3 and its helpers, which name
the field `[[AsyncEvaluationOrder]]` (unset, an integer, or done) where
section 1 says `[[AsyncEvaluation]]` with its order.

**API of a pending evaluation:**

- `Runtime::evaluate(module)` returns `[[TopLevelCapability]]`'s promise
  of the module's cycle root. Without top-level `await` in the graph it is
  settled when the call returns, as in 3a-1. With it, the promise can be
  pending: it settles in a job of `Runtime::run_jobs`, when the last async
  body of the graph ends (fulfilled with `undefined`) or when a body of
  the graph rejects (rejected with that value). The promise counts as
  handled, so the rejection tracker never reports it.
- The host learns that a promise settled with `Runtime::react_to_promise`
  (a continuation in a job, after the module's own reactions), or reads it
  with `Runtime::promise_settlement` after its microtask checkpoint. 3b-1
  reports a rejection as HTML "run a module script" says (upon rejection
  of the evaluation promise), which fits the measured `tla-reject` case:
  the error reaches `window.onerror` at the time of the rejection.
- Evaluate of a module that waits (`evaluating-async`) gives the promise
  of its cycle root, a new one if the root has none; it settles when the
  root's async evaluation ends. Evaluate of a module evaluated with an
  error gives a promise rejected now with the same value (also an error of
  an async body).
- `Runtime::eval_module_at` (the page path, `run_module_without_requests`)
  still refuses a module with top-level `await` with "not supported yet
  (top-level await)" at offset 0, now in `module/mod.rs` instead of the
  compiler. 3b-1 removes it with the switch to the records. A module that
  also uses `import()` or `import.meta` gets the refusal of that construct
  (the compile runs before this check).
- `swb-js --module` runs the jobs after Evaluate and reports a rejection of
  the module's promise as an uncaught error (`Uncaught RangeError: late`
  and the location of the error), exit code 1. A promise that never
  settles ends the run without a report (exit code 0). An error in the
  synchronous part of Evaluate ends the run before the jobs, as an
  uncaught error of a script does: async bodies that started do not go
  on. The page runs them in its microtask checkpoint after the script
  (3b-1).

**Engine:**

- A module function with top-level `await` (`[[HasTLA]]`) keeps the code
  kind `Module`; its code has the handler of an async function over the
  body after `InitialYield` (`AsyncReject`). The verifier allows `Await`,
  `Resume` and the instructions of `for await` in `Module` code
  (`CodeKind::can_await`).
- `ExecuteAsyncModule` makes a new promise (the capability; resolving
  functions that no script reaches; the record keeps it in
  `async_capability` until the body has ended), gives the module's
  suspended generator object the kind `GeneratorKind::AsyncModule(module)`
  (the promise is not in the kind: a second handle would make every
  generator object 8 bytes bigger) and attaches an internal continuation
  to the promise (data: the generator object, whose kind names the
  module); then it resumes the body. That kind behaves as an async
  function's: `await` delivers the promise to the caller, the end of the
  body resolves it, `AsyncReject` rejects it. The continuation runs
  `AsyncModuleExecutionFulfilled` or `AsyncModuleExecutionRejected` in its
  job. No instruction after `InitialYield` does the work of `AsyncStart`:
  the kind is set before the resume, so the body starts directly. While a
  body waits, the record keeps its frame alive (record, capability,
  reaction, generator object), also when the awaited promise can never
  settle; an async function's frame in that case is garbage.
- The record has the status `evaluating-async`, `async_order`
  (`AsyncOrder::Unset`, `Order(n)`, `Done`), `pending_async` and
  `async_parents` (traced, charged with `Heap::generic_resized`). The
  counter of `IncrementModuleAsyncEvaluationCount` is a `u64` of the
  runtime's module table; it is never reset.
- `GatherAvailableAncestors` is a loop over a work stack with a hash set
  for "execList contains"; the set of gathered modules does not depend on
  the order of the walk, and the list is sorted by the order before it
  runs. `AsyncModuleExecutionRejected` is a loop over an explicit stack of
  (module, next parent): the top-level promises are rejected after those of
  the parents, the order of the specification's recursion (tested). Both
  and the run of the gathered modules charge the time countdown per step.
- A module whose status is `evaluated` with an error and whose
  `[[CycleRoot]]` is empty (it left the DFS stack with an error) stands
  for itself where the specification reads its cycle root. The guards
  for the assertions of `GatherAvailableAncestors` (a parent that is not
  `evaluating-async` or has no pending dependency) skip the parent instead
  of failing.
- Stack traces: a module body that waits for an async function shows as
  `at async x.js:2:5` below it (Chromium 148, probe case `msg-tla-stack`);
  after the module's own `await`, its frame is the only one
  (`msg-tla-dependency-stack`). The async frames end at the module (no
  script waits for a module).

**Termination.** A termination (or an engine error) ends the evaluation
of the modules it reaches; these are `evaluated` with the internal
"terminated" error: they never run again and report nothing more, and
every promise that waits for them stays pending. Which modules:

- in the synchronous part of Evaluate (also in the first run of an async
  body): the modules on the DFS stack (as in 3a-1). Async modules that
  left the stack before go on in later jobs; their parents on the stack
  never run;
- in a body resumed by a job (after an `await`): its module
  (`Runtime::module_body_ended`, from the `await` reaction); its parents
  wait for ever;
- in a module that `AsyncModuleExecutionFulfilled` runs from a job (a
  synchronous ancestor, or the first run of an async one): that module and
  the gathered modules that did not run yet. A termination in the
  rejection walk (the time limit, at a step of the walk) leaves the walk
  undone: the modules that it reached are `evaluated` with the error, but
  a top-level promise that it had not rejected yet stays pending; the
  modules that it did not reach keep waiting.

A later Evaluate of a graph with such a module gives a promise that never
settles; a module that waits for one never runs. No record is left in a
state where a later step runs a body twice or settles a promise wrongly
(engine tests in `module/evaluate.rs` and `crates/js/tests/module_graph/
tla.rs`).

**Tests and scores.** test262: `test/language/module-code/top-level-await`
passes 240 of 251 (also with `--stress`): 8 tests are skipped for the
feature `dynamic-import` and 3 use `import()` (`fulfillment-order.js`,
`rejection-order.js`, `module-graphs-does-not-hang.js`; they come with
3a-3). The feature `top-level-await` raised `expressions/class` and
`statements/class` by 4 each and `expressions/object` by 1 (`await` in
computed keys at the top level). The robustness tests (chains of 10,000
modules that await, 10,000 synchronous modules above one async module,
10,000 async dependencies, 10,000 async parents of a module that rejects,
and a chain of them, a ring of 10,000 with one async module, a body that
never settles) each take well under a second in the test profile.
