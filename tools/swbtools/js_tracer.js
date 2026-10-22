// Web API tracer for `swbtools js-survey` (tools/swbtools/js_survey.py).
//
// Runs as a Playwright init script, so it runs before any page script. It
// wraps the members of the Web API interfaces with Proxy objects that count
// the calls, reads and writes, and keeps the results in
// `window.__swb_trace__`. The page must work as without the tracer: the
// tracer never throws, never changes `this` or the arguments, and makes
// `Function.prototype.toString` report the original text.
(() => {
  'use strict';
  try {
    if (window.top !== window || window.__swb_trace__) return;
  } catch (e) {
    return;
  }

  // ---- Originals, captured before any wrapping. ----
  const O = {
    defineProperty: Object.defineProperty,
    getOwnPropertyDescriptor: Object.getOwnPropertyDescriptor,
    getOwnPropertyNames: Object.getOwnPropertyNames,
    create: Object.create,
    getPrototypeOf: Object.getPrototypeOf,
    keys: Object.keys,
    apply: Reflect.apply,
    construct: Reflect.construct,
    Proxy: Proxy,
    WeakMap: WeakMap,
    Set: Set,
    Map: Map,
    toString: Function.prototype.toString,
    objToString: Object.prototype.toString,
    now: performance.now.bind(performance),
    MutationObserver: window.MutationObserver,
    Element: window.Element,
    Node: window.Node,
    DocumentFragment: window.DocumentFragment,
    HTMLScriptElement: window.HTMLScriptElement,
    HTMLIFrameElement: window.HTMLIFrameElement,
    URL: window.URL,
    Request: window.Request,
    addEventListener: EventTarget.prototype.addEventListener,
    querySelectorAll: Element.prototype.querySelectorAll,
    getAttribute: Element.prototype.getAttribute,
    tagName: O_getter(Element.prototype, 'tagName'),
    id: O_getter(Element.prototype, 'id'),
    className: O_getter(Element.prototype, 'className'),
    nodeType: O_getter(Node.prototype, 'nodeType'),
    nodeName: O_getter(Node.prototype, 'nodeName'),
    isConnected: O_getter(Node.prototype, 'isConnected'),
    takeRecords: MutationObserver.prototype.takeRecords,
  };

  function O_getter(owner, name) {
    const d = Object.getOwnPropertyDescriptor(owner, name);
    return d && d.get;
  }

  // ---- State. ----
  const MAX_DISTINCT = 3000;
  const T = {
    version: 1,
    paused: false,
    phase: 'parse',
    phase_times: {},
    counts: { parse: O.create(null), load: O.create(null), after: O.create(null) },
    args: { parse: O.create(null), load: O.create(null), after: O.create(null) },
    sums: { parse: O.create(null), load: O.create(null), after: O.create(null) },
    timers_run: { parse: O.create(null), load: O.create(null), after: O.create(null) },
    inserted_scripts: [],
    iframes: [],
    unwrapped: [],
    wrapped: { interfaces: 0, members: 0, window_members: 0, namespace_members: 0 },
    mutations: {
      parse: newMutationStats(),
      later: newMutationStats(),
    },
    errors: [],
  };
  let busy = false;

  function newMutationStats() {
    return {
      records: 0,
      added_nodes: 0,
      added_elements_total: 0,
      removed_nodes: 0,
      removed_elements_total: 0,
      added_by_tag: O.create(null),
      removed_by_tag: O.create(null),
      attributes_by_name: O.create(null),
      character_data: 0,
    };
  }

  function note(error) {
    if (T.errors.length < 20) T.errors.push(String(error && error.message ? error.message : error));
  }

  function bump(table, key, n) {
    table[key] = (table[key] || 0) + (n === undefined ? 1 : n);
  }

  function count(key, kind) {
    bump(T.counts[T.phase], key + '|' + kind);
  }

  function cut(value) {
    const s = String(value);
    return s.length > 120 ? s.slice(0, 120) : s;
  }

  function hist(name, value) {
    const table = T.args[T.phase];
    let h = table[name];
    if (!h) h = table[name] = O.create(null);
    const key = cut(value);
    if (key in h || O.keys(h).length < MAX_DISTINCT) bump(h, key);
    else bump(h, '(other)');
  }

  function sum(name, n) {
    const table = T.sums[T.phase];
    let s = table[name];
    if (!s) s = table[name] = { count: 0, total: 0 };
    s.count += 1;
    s.total += n;
  }

  // ---- Describing values. ----
  function describe(target) {
    try {
      if (target === window) return 'window';
      if (target === document) return 'document';
      if (target instanceof O.Element) {
        let s = String(O.apply(O.tagName, target, [])).toLowerCase();
        const id = O.apply(O.id, target, []);
        if (id) s += '#' + cut(id).slice(0, 40);
        const cls = O.apply(O.className, target, []);
        if (typeof cls === 'string' && cls.trim()) {
          s += '.' + cls.trim().split(/\s+/).slice(0, 2).map((c) => c.slice(0, 30)).join('.');
        }
        return s;
      }
      return O.apply(O.objToString, target, []).slice(8, -1);
    } catch (e) {
      return '?';
    }
  }

  function hostOf(value) {
    try {
      let text;
      if (value instanceof O.Request) text = value.url;
      else text = String(value);
      return new O.URL(text, document.baseURI).host || '(none)';
    } catch (e) {
      return '(invalid)';
    }
  }

  function delayBucket(delay) {
    const d = Number(delay);
    if (!(d > 0)) return '0';
    if (d <= 16) return '1-16';
    if (d <= 100) return '17-100';
    if (d <= 1000) return '101-1000';
    return '>1000';
  }

  // ---- Scripts and iframes inserted by scripts. ----
  function noteInsertedNode(node, how, target) {
    if (!(node instanceof O.Node)) return;
    const nodeType = O.apply(O.nodeType, node, []);
    if (nodeType !== 1 && nodeType !== 11) return;
    const connected = !!O.apply(O.isConnected, target, []);
    const found = [];
    if (nodeType === 1) found.push(node);
    for (const el of O.apply(O.querySelectorAll, node, ['script, iframe'])) found.push(el);
    for (const el of found) {
      if (el instanceof O.HTMLScriptElement) {
        if (T.inserted_scripts.length < 500) {
          T.inserted_scripts.push({
            phase: T.phase,
            how: how,
            src: cut(el.getAttribute('src') || ''),
            type: cut(el.getAttribute('type') || ''),
            async: el.async,
            defer: el.defer,
            module: (el.getAttribute('type') || '').toLowerCase() === 'module',
            connected: connected,
          });
        }
      } else if (el instanceof O.HTMLIFrameElement) {
        if (T.iframes.length < 500) {
          const src = el.getAttribute('src') || '';
          T.iframes.push({
            phase: T.phase,
            how: how,
            host: src ? hostOf(src) : el.hasAttribute('srcdoc') ? '(srcdoc)' : '(no src)',
            connected: connected,
          });
        }
      }
    }
  }

  function noteWrittenHtml(html, how) {
    const re = /<script\b([^>]*)>/gi;
    let m;
    while ((m = re.exec(html)) !== null && T.inserted_scripts.length < 500) {
      const attrs = m[1];
      const src = /\bsrc\s*=\s*["']?([^"'\s>]+)/i.exec(attrs);
      const type = /\btype\s*=\s*["']?([^"'\s>]+)/i.exec(attrs);
      T.inserted_scripts.push({
        phase: T.phase,
        how: how,
        src: src ? cut(src[1]) : '',
        type: type ? cut(type[1]) : '',
        async: /\basync\b/i.test(attrs),
        defer: /\bdefer\b/i.test(attrs),
        module: !!type && type[1].toLowerCase() === 'module',
        connected: true,
      });
    }
    const frames = /<iframe\b([^>]*)>/gi;
    while ((m = frames.exec(html)) !== null && T.iframes.length < 500) {
      const src = /\bsrc\s*=\s*["']?([^"'\s>]+)/i.exec(m[1]);
      T.iframes.push({
        phase: T.phase,
        how: how,
        host: src ? hostOf(src[1]) : '(no src)',
        connected: true,
      });
    }
  }

  const INSERTION = new O.Set([
    'Node.appendChild',
    'Node.insertBefore',
    'Node.replaceChild',
    'Element.append',
    'Element.prepend',
    'Element.before',
    'Element.after',
    'Element.replaceWith',
    'Element.replaceChildren',
    'Element.insertAdjacentElement',
    'Document.append',
    'Document.prepend',
    'Document.replaceChildren',
    'DocumentFragment.append',
    'DocumentFragment.prepend',
    'DocumentFragment.replaceChildren',
    'CharacterData.before',
    'CharacterData.after',
  ]);
  const SELECTOR_MEMBERS = new O.Set(['querySelector', 'querySelectorAll', 'matches', 'closest']);

  // ---- Argument recording for selected members. ----
  function recordCall(label, member, thisArg, args) {
    switch (label) {
      case 'Document.createElement':
        hist('createElement', String(args[0]).toLowerCase());
        return;
      case 'EventTarget.addEventListener':
        hist('addEventListener', cut(args[0]) + ' @ ' + describe(thisArg));
        return;
      case 'window.setTimeout':
      case 'window.setInterval':
        hist(member, delayBucket(args[1]));
        return;
      case 'HTMLCanvasElement.getContext':
        hist('getContext', args[0]);
        return;
      case 'window.matchMedia':
        hist('matchMedia', args[0]);
        return;
      case 'window.fetch': {
        let method = 'GET';
        if (args[1] && args[1].method) method = String(args[1].method).toUpperCase();
        else if (args[0] instanceof O.Request) method = args[0].method;
        hist('fetch', method + ' ' + hostOf(args[0]));
        return;
      }
      case 'XMLHttpRequest.open':
        hist('XMLHttpRequest.open', String(args[0]).toUpperCase() + ' ' + hostOf(args[1]));
        return;
      case 'Navigator.sendBeacon':
        hist('sendBeacon', hostOf(args[0]));
        return;
      case 'Document.write':
      case 'Document.writeln': {
        let total = 0;
        for (const a of args) {
          const s = String(a);
          total += s.length;
          noteWrittenHtml(s, member);
        }
        sum(label, total);
        return;
      }
      case 'Element.insertAdjacentHTML':
        sum(label, String(args[1]).length);
        return;
      case 'Element.setAttribute':
      case 'Element.setAttributeNS':
        hist('setAttribute', String(args[label.endsWith('NS') ? 1 : 0]).toLowerCase());
        return;
      default:
    }
    if (SELECTOR_MEMBERS.has(member)) {
      hist(member, args[0]);
      return;
    }
    if (INSERTION.has(label)) {
      const insertedHow = member;
      for (const a of args) noteInsertedNode(a, insertedHow, thisArg);
    }
  }

  function recordSet(label, member, thisArg, value) {
    if (label === 'Element.innerHTML' || label === 'Element.outerHTML') {
      sum(label, String(value).length);
      return;
    }
    if (member.length > 2 && member.startsWith('on') && typeof value === 'function') {
      hist('on-handler', member.slice(2) + ' @ ' + describe(thisArg));
    }
  }

  // ---- Proxies. ----
  const originals = new O.WeakMap(); // proxy -> original function
  const proxies = new O.Map(); // original function -> proxy

  function guarded(fn) {
    // `paused` is set by the survey tool around its own calls (scrolling).
    if (busy || T.paused) return;
    busy = true;
    try {
      fn();
    } catch (e) {
      note(e);
    } finally {
      busy = false;
    }
  }

  function makeCallProxy(fn, label, member, kind) {
    const p = new O.Proxy(fn, {
      apply(target, thisArg, args) {
        guarded(() => {
          count(label, kind);
          if (kind === 'call') {
            recordCall(label, member, thisArg, args);
          } else if (kind === 'set') {
            recordSet(label, member, thisArg, args[0]);
          }
        });
        return O.apply(target, thisArg, args);
      },
    });
    originals.set(p, fn);
    return p;
  }

  function wrapCallbackArg(args, index, kind) {
    const cb = args[index];
    if (typeof cb !== 'function') return;
    args[index] = function (...a) {
      bump(T.timers_run[T.phase], kind);
      return O.apply(cb, this, a);
    };
  }

  const TIMER_KINDS = {
    'window.setTimeout': 'timeout',
    'window.setInterval': 'interval',
    'window.requestAnimationFrame': 'animation-frame',
    'window.requestIdleCallback': 'idle',
  };

  function makeTimerProxy(fn, label, member) {
    const kind = TIMER_KINDS[label];
    const p = new O.Proxy(fn, {
      apply(target, thisArg, args) {
        guarded(() => {
          count(label, 'call');
          recordCall(label, member, thisArg, args);
          wrapCallbackArg(args, 0, kind);
        });
        return O.apply(target, thisArg, args);
      },
    });
    originals.set(p, fn);
    return p;
  }

  function wrapMember(owner, name, label, bucket) {
    let d;
    try {
      d = O.getOwnPropertyDescriptor(owner, name);
    } catch (e) {
      return;
    }
    if (!d) return;
    const isAccessor = !!(d.get || d.set);
    const isFunction = typeof d.value === 'function';
    if (!isAccessor && !isFunction) return;
    if (!d.configurable) {
      if (T.unwrapped.indexOf(label) < 0 && T.unwrapped.length < 2000) T.unwrapped.push(label);
      return;
    }
    try {
      let nd;
      if (isAccessor) {
        nd = { enumerable: d.enumerable, configurable: true };
        if (d.get) nd.get = makeCallProxy(d.get, label, name, 'get');
        if (d.set) nd.set = makeCallProxy(d.set, label, name, 'set');
      } else if (label in TIMER_KINDS) {
        nd = { value: makeTimerProxy(d.value, label, name), writable: d.writable,
               enumerable: d.enumerable, configurable: true };
      } else {
        nd = { value: makeCallProxy(d.value, label, name, 'call'), writable: d.writable,
               enumerable: d.enumerable, configurable: true };
      }
      O.defineProperty(owner, name, nd);
      T.wrapped[bucket] += 1;
    } catch (e) {
      note(e);
    }
  }

  function wrapAllMembers(owner, prefix, bucket, skip) {
    let names;
    try {
      names = O.getOwnPropertyNames(owner);
    } catch (e) {
      return;
    }
    for (const name of names) {
      if (skip && skip(name)) continue;
      wrapMember(owner, name, prefix + '.' + name, bucket);
    }
  }

  // ---- Constructors. ----
  function makeConstructProxy(ctor, label) {
    let p = proxies.get(ctor);
    if (p) return p;
    p = new O.Proxy(ctor, {
      construct(target, args, newTarget) {
        guarded(() => count(label, 'construct'));
        return O.construct(target, args, newTarget === p ? target : newTarget);
      },
      apply(target, thisArg, args) {
        guarded(() => count(label, 'call'));
        return O.apply(target, thisArg, args);
      },
    });
    proxies.set(ctor, p);
    originals.set(p, ctor);
    return p;
  }

  // ECMAScript built-ins and other names that are not Web API interfaces.
  // ECMAScript global functions: not Web APIs.
  const ES_GLOBAL_FUNCTIONS = new O.Set([
    'eval', 'parseInt', 'parseFloat', 'isNaN', 'isFinite', 'decodeURI', 'decodeURIComponent',
    'encodeURI', 'encodeURIComponent', 'escape', 'unescape',
  ]);

  const ES_BUILTINS = new O.Set([
    'Object', 'Function', 'Array', 'Number', 'String', 'Boolean', 'Symbol', 'BigInt',
    'Date', 'RegExp', 'Error', 'EvalError', 'RangeError', 'ReferenceError', 'SyntaxError',
    'TypeError', 'URIError', 'AggregateError', 'SuppressedError', 'Promise', 'Map', 'Set',
    'WeakMap', 'WeakSet', 'WeakRef', 'FinalizationRegistry', 'ArrayBuffer',
    'SharedArrayBuffer', 'DataView', 'Int8Array', 'Uint8Array', 'Uint8ClampedArray',
    'Int16Array', 'Uint16Array', 'Int32Array', 'Uint32Array', 'Float16Array',
    'Float32Array', 'Float64Array', 'BigInt64Array', 'BigUint64Array', 'Proxy', 'Reflect',
    'JSON', 'Math', 'Atomics', 'Iterator', 'Intl', 'WebAssembly', 'Temporal',
    'DisposableStack', 'AsyncDisposableStack', 'Infinity', 'NaN',
  ]);

  const done = new O.Map(); // prototype -> true
  function wrapInterfaces() {
    const names = O.getOwnPropertyNames(window);
    for (const name of names) {
      const c = name.charCodeAt(0);
      if (c < 65 || c > 90 || ES_BUILTINS.has(name)) continue;
      let d;
      try {
        d = O.getOwnPropertyDescriptor(window, name);
      } catch (e) {
        continue;
      }
      if (!d || typeof d.value !== 'function') continue;
      const ctor = d.value;
      let proto;
      try {
        proto = ctor.prototype;
      } catch (e) {
        continue;
      }
      if (!proto || typeof proto !== 'object') continue;
      const canonical = proto.constructor === ctor;
      if (!done.has(proto)) {
        done.set(proto, true);
        T.wrapped.interfaces += 1;
        // Aliases (`Image`, `webkitURL`) share the prototype: use the name of
        // the interface that owns it.
        let protoName = name;
        try {
          const own = proto.constructor;
          if (typeof own === 'function' && own.name) protoName = own.name;
        } catch (e) {
          // keep the window name
        }
        wrapAllMembers(proto, protoName, 'members', (n) => n === 'constructor');
        wrapAllMembers(ctor, ctor.name || name, 'members', (n) =>
          n === 'prototype' || n === 'length' || n === 'name');
      }
      if (!d.configurable || !d.writable) {
        if (T.unwrapped.indexOf(name + ' (constructor)') < 0) T.unwrapped.push(name + ' (constructor)');
        continue;
      }
      try {
        const p = makeConstructProxy(ctor, name);
        O.defineProperty(window, name, {
          value: p, writable: d.writable, enumerable: d.enumerable, configurable: true,
        });
        if (canonical) {
          const cd = O.getOwnPropertyDescriptor(proto, 'constructor');
          if (cd && cd.configurable) {
            O.defineProperty(proto, 'constructor', {
              value: p, writable: cd.writable, enumerable: cd.enumerable, configurable: true,
            });
          }
        }
      } catch (e) {
        note(e);
      }
    }
  }

  function wrapWindowMembers() {
    const names = O.getOwnPropertyNames(window);
    for (const name of names) {
      if (name === '__swb_trace__' || ES_GLOBAL_FUNCTIONS.has(name)) continue;
      let d;
      try {
        d = O.getOwnPropertyDescriptor(window, name);
      } catch (e) {
        continue;
      }
      if (!d) continue;
      // Interface objects were handled by wrapInterfaces.
      const c = name.charCodeAt(0);
      if (c >= 65 && c <= 90 && typeof d.value === 'function' && d.value.prototype) continue;
      wrapMember(window, name, 'window.' + name, 'window_members');
    }
  }

  // ---- Function.prototype.toString shows the original text. ----
  function patchToString() {
    const patched = new O.Proxy(O.toString, {
      apply(target, thisArg, args) {
        const original = originals.get(thisArg);
        return O.apply(target, original || thisArg, args);
      },
    });
    originals.set(patched, O.toString);
    O.defineProperty(Function.prototype, 'toString', {
      value: patched, writable: true, enumerable: false, configurable: true,
    });
  }

  // ---- Mutation observer. ----
  function countAdded(stats, nodes, bag, totalField) {
    for (const node of nodes) {
      if (O.apply(O.nodeType, node, []) !== 1) continue;
      bump(bag, String(O.apply(O.nodeName, node, [])).toLowerCase());
      stats[totalField] += 1 + O.apply(O.querySelectorAll, node, ['*']).length;
    }
  }

  function startObserver() {
    const mo = new O.MutationObserver((records) => handleRecords(records));
    mo.observe(document, {
      subtree: true, attributes: true, childList: true, characterData: true,
    });
    T.observer = mo;
  }

  function handleRecords(records) {
    const was = busy;
    busy = true;
    try {
      const stats = T.phase === 'parse' ? T.mutations.parse : T.mutations.later;
      for (const r of records) {
        stats.records += 1;
        if (r.type === 'childList') {
          stats.added_nodes += r.addedNodes.length;
          stats.removed_nodes += r.removedNodes.length;
          countAdded(stats, r.addedNodes, stats.added_by_tag, 'added_elements_total');
          countAdded(stats, r.removedNodes, stats.removed_by_tag, 'removed_elements_total');
        } else if (r.type === 'attributes') {
          bump(stats.attributes_by_name, r.attributeName);
        } else if (r.type === 'characterData') {
          stats.character_data += 1;
        }
      }
    } catch (e) {
      note(e);
    } finally {
      busy = was;
    }
  }

  // ---- Phases. ----
  function setPhase(phase) {
    T.phase = phase;
    T.phase_times[phase] = O.now();
  }

  T.phase_times.parse = 0;
  O.apply(O.addEventListener, document, ['DOMContentLoaded', () => {
    handleRecords(O.apply(O.takeRecords, T.observer, []));
    setPhase('load');
  }, true]);
  O.apply(O.addEventListener, window, ['load', () => {
    handleRecords(O.apply(O.takeRecords, T.observer, []));
    setPhase('after');
  }, true]);

  // ---- Installation. ----
  O.defineProperty(window, '__swb_trace__', {
    value: T, writable: false, enumerable: false, configurable: false,
  });
  O.defineProperty(T, 'flush', {
    value: () => {
      handleRecords(O.apply(O.takeRecords, T.observer, []));
      return JSON.parse(JSON.stringify(T, (k, v) => (k === 'observer' ? undefined : v)));
    },
    enumerable: false,
  });
  try {
    startObserver();
    wrapInterfaces();
    wrapWindowMembers();
    for (const ns of ['console', 'CSS']) {
      const obj = window[ns];
      if (obj && typeof obj === 'object') wrapAllMembers(obj, ns, 'namespace_members');
    }
    patchToString();
  } catch (e) {
    note(e);
  }
})();
