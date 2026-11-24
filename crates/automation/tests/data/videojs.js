// The setup script of the exit test of videojs.org (`videojs.rs`, M13 feature
// 6; made by `tools/videojs_records.py`). It runs after the load, in Chromium
// (`swbtools replay --animations on --setup`) and in swb (the test). It
// defines:
//
// - `window.__v()`: the record, a JSON text: the state of the page, the list
//   of the animations, the events of animations and transitions since the
//   last record, and, when a flag is set, the computed values of the
//   animated properties (`window.__values`), the rectangles of the elements
//   that have animations (`window.__targets`, for the choice of pixel
//   points) and the rectangles and texts of the first view
//   (`window.__rects`); `window.__events` gives the events log. The `values`
//   sequence sets the first three; the `events` sequence only the last,
//   because its animations run between its steps;
// - `window.__seek(name, ms)`, `__seekFraction(kind, name, f)`, `__seekAll(f)`
//   and `__finish()`: the sampling of docs/videojs-diagnosis.md section 2
//   (pause the animations, set `currentTime`), relative to each animation's
//   own delay.
//
// Nothing here depends on the browser: no value that differs between two
// runs of Chromium (no time, no React id) goes into the record.
(() => {
  const round = (v, k = 100) => Math.round(v * k) / k;
  const own = (e) => e.getAttribute('aria-label') || e.getAttribute('href') || '';
  // A short description of an element: tag, role, label or link, the start of
  // the class list (no id: Base UI's ids come from React's counter).
  const describe = (e) => {
    if (!e || !e.tagName) return String(e);
    const cls = (e.getAttribute('class') || '').replace(/\s+/g, ' ').slice(0, 40);
    return e.tagName.toLowerCase() + '[' + (e.getAttribute('role') || '') + '|' + own(e) + '|' + cls + ']';
  };

  // The events of animations and transitions, in the order of dispatch. A
  // listener in the capture phase of the document sees every one. The
  // `elapsedTime` of a cancel is the time of the cancel: no value. The
  // marquee's `animationiteration` (every 30 s) is not logged: the group in
  // which it falls depends on the speed of the run (probe rows of
  // `css-animations.json` cover the event).
  window.__log = [];
  for (const type of ['transitionrun', 'transitionstart', 'transitionend', 'transitioncancel',
    'animationstart', 'animationend', 'animationcancel']) {
    document.addEventListener(type, (e) => {
      window.__log.push([
        e.type, e.propertyName || e.animationName, describe(e.target), e.pseudoElement || '',
        e.type.endsWith('cancel') ? null : round(e.elapsedTime, 1000),
      ]);
    }, true);
  }

  // Keyframe keys that are no properties.
  const SKIP = new Set(['offset', 'easing', 'composite', 'computedOffset']);
  const properties = (a) => {
    if (a instanceof CSSTransition) return [a.transitionProperty];
    const keys = a.effect.getKeyframes().flatMap((k) => Object.keys(k).filter((n) => !SKIP.has(n)));
    return [...new Set(keys)];
  };
  const finite = (a) => Number.isFinite(a.effect.getTiming().duration * a.effect.getTiming().iterations);
  const read = (a, name) => {
    const t = a.effect.target;
    const cs = getComputedStyle(t, a.effect.pseudoElement || null);
    return name.startsWith('--') || name.includes('-') ? cs.getPropertyValue(name) : cs[name];
  };
  // The animations in the order of `getAnimations()` is not specified across
  // browsers for equal composite order: sort by a key.
  const sorted = () => document.getAnimations().map((a) => ({
    a,
    key: [a.constructor.name, a.animationName || a.transitionProperty, describe(a.effect.target),
      a.effect.pseudoElement || ''].join('|'),
  })).sort((x, y) => (x.key < y.key ? -1 : x.key > y.key ? 1 : 0));

  const named = (a, prefix) => (a.animationName || a.transitionProperty || '').startsWith(prefix);
  // Pauses the animations whose name starts with `prefix` and sets their
  // time to their delay plus `ms` (the delay of the hero reveal comes from
  // `performance.now()`, so the time is relative to it).
  window.__seek = (prefix, ms) => {
    for (const { a } of sorted()) {
      if (named(a, prefix)) {
        a.pause();
        a.currentTime = a.effect.getTiming().delay + ms;
      }
    }
  };
  // The same at `f` of the duration of each finite animation (`kind` is the
  // class of the animations: CSSAnimation or CSSTransition).
  window.__seekFraction = (kind, prefix, f) => {
    for (const { a } of sorted()) {
      if (a.constructor.name !== kind || !named(a, prefix) || !finite(a)) continue;
      a.pause();
      a.currentTime = a.effect.getTiming().delay + f * a.effect.getTiming().duration;
    }
  };
  window.__seekAll = (f) => window.__seekFraction('CSSTransition', '', f);
  // Finishes the finite animations of a kind and name: they take their end
  // state (a transition then leaves `getAnimations()`). No style read
  // follows: the next style change must start from the end state (probe case
  // `finish-then-style-change` of `css-videojs-6.json`).
  window.__finishKind = (kind, prefix) => {
    let count = 0;
    for (const { a } of sorted()) {
      if (a.constructor.name === kind && named(a, prefix) && finite(a)) {
        a.finish();
        count += 1;
      }
    }
    return count;
  };
  window.__finish = () => window.__finishKind('CSSTransition', '');

  const visible = (e) => {
    const r = e.getBoundingClientRect();
    const s = getComputedStyle(e);
    return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none';
  };
  const box = (r) => [round(r.left), round(r.top), round(r.width), round(r.height)];
  // The rectangles of the first view: per area, for each selector the visible
  // matches (at most 40): [selector, index, left, top, width, height].
  const AREAS = {
    header: [
      'nav', 'nav > a', 'nav > a svg', 'nav .DocSearch-Button', '.DocSearch-Button-Container',
      '.DocSearch-Search-Icon', '.DocSearch-Button-Placeholder', '.DocSearch-Button-Key',
      'nav .hidden.md\\:flex > a', 'nav button[aria-label="Appearance"]', 'nav a[aria-label="Discord"]',
      'nav a[aria-label="GitHub"]', 'nav a svg',
    ],
    hero: [
      'main header', 'h1', 'h1 span', 'main header > div', 'main header p',
      'main header p a',
    ],
    player: [
      '.media-skin', 'video', '.media-poster', '.media-poster-image', '.video-controls-backdrop',
      '.video-controls', '.media-play-button', '.media-mute-button', '.video-time-slider-group',
      '.media-time-value', '.media-time-slider', '.media-slider-track', '.media-slider-buffer',
      '.media-slider-thumb', '.media-time-toggle', '.media-settings-menu-trigger',
      '.media-pip-button', '.media-fullscreen-button', '.video-status-indicators',
    ],
  };
  const rects = () => Object.fromEntries(Object.entries(AREAS).map(([area, selectors]) => [
    area,
    selectors.flatMap((selector) => [...document.querySelectorAll(selector)]
      .filter(visible).slice(0, 40).map((e, i) => [selector, i, ...box(e.getBoundingClientRect())])),
  ]));
  // The line boxes of the title: the rectangles of its text, per text node.
  const lines = (root) => {
    const out = [];
    const walk = (n) => {
      if (n.nodeType === 3) {
        if (!n.textContent.trim()) return;
        const range = document.createRange();
        range.selectNodeContents(n);
        out.push(n.textContent.trim().slice(0, 40), ...[...range.getClientRects()].map((r) => box(r)));
      } else {
        for (const c of n.childNodes) walk(c);
      }
    };
    if (root) walk(root);
    return out;
  };
  const texts = () => ({
    nav: [...document.querySelectorAll('nav a, nav button')].filter(visible)
      .map((e) => e.textContent.replace(/\s+/g, ' ').trim() + '|' + own(e)),
    title: lines(document.querySelector('h1')),
    hero: [...document.querySelectorAll('main header p, main header a')].filter(visible)
      .map((e) => e.textContent.replace(/\s+/g, ' ').trim().slice(0, 80)),
    player: [...document.querySelectorAll('.media-skin time, .media-skin output, .media-skin button')]
      .filter(visible).map((e) => describe(e) + ':' + e.textContent.replace(/\s+/g, ' ').trim()),
  });

  window.__v = () => {
    const all = sorted();
    const animations = all.map(({ a }) => [
      a.constructor.name, a.animationName || a.transitionProperty, describe(a.effect.target),
      a.effect.pseudoElement || '', a.playState === 'paused' ? 'paused' : a.playState,
      a.playState === 'paused' ? round(a.currentTime - a.effect.getTiming().delay, 10) : null,
    ]);
    // Values of the animated properties of the finite animations (a running
    // endless animation has a value that depends on the time).
    const values = all.filter(({ a }) => a.playState === 'paused' || finite(a)).flatMap(({ a }) =>
      properties(a).map((name) => [
        a.animationName || a.transitionProperty, describe(a.effect.target), name, read(a, name)]));
    // The rectangles of the elements that the animations belong to (the
    // pixel points of the tool lie inside them), once per element.
    const seen = new Set();
    const targets = [];
    for (const { a } of all) {
      const e = a.effect.target;
      if (!window.__targets || a.playState !== 'paused' || seen.has(e) || !visible(e)) continue;
      seen.add(e);
      targets.push([describe(e), ...box(e.getBoundingClientRect())]);
    }
    // The elements with an inline transform: the spring of the page at its
    // end (a script sets it), and apart from it the preview image of the time
    // slider (the player's script positions it with Floating UI).
    const inline = [...document.querySelectorAll('[style*="transform"]')];
    const transformed = inline.filter((e) => !e.matches('.media-slider-thumbnail-image'))
      .map((e) => e.style.transform);
    const thumbnail = inline.filter((e) => e.matches('.media-slider-thumbnail-image'))
      .map((e) => e.style.transform);
    const record = {
      scrollY: round(scrollY, 1),
      active: describe(document.activeElement),
      htmlClass: document.documentElement.className,
      expanded: [...document.querySelectorAll('[aria-expanded="true"]')].map(describe),
      open: [...document.querySelectorAll('[data-open], [data-popup-open]')].length,
      transformed,
      thumbnail,
      animate: [...document.querySelectorAll('[data-animate]')].map((e) => describe(e) + '=' + e.getAttribute('data-animate')),
      bars: [...document.querySelectorAll('.data-bar')].map((e) => round(e.getBoundingClientRect().width)),
      heroReady: [...document.querySelectorAll('[data-hero-ready]')].map((e) => e.getAttribute('data-hero-ready')),
      animations,
      values: window.__values ? values : [],
      targets,
      events: window.__events ? window.__log.splice(0) : (window.__log.length = 0, []),
    };
    if (window.__rects) {
      record.rects = rects();
      record.texts = texts();
    }
    return JSON.stringify(record);
  };
})();
