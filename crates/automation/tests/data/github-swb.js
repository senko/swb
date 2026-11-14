// The record script of the GitHub test (`github_swb.rs`): defines
// `window.__s()`, which returns the first view of github.com/senko/swb as a
// JSON string. Chromium (`swbtools replay --record 'window.__s()' --setup`)
// and swb run the same script after the load. Each part has two keys: the
// rectangles (`...Rects`, strings `x,y,width,height` of rounded numbers,
// compared within a tolerance) and the texts (`...Texts`, compared exactly).
// The record has nothing that differs between two runs of Chromium: no React
// `useId` value (`_r_..._` ids), no `anchor-name` style, no time but the
// texts of the `relative-time` elements (the page clock is fixed).
(() => {
  const rounded = (v) => Math.round(v);
  const rect = (e) => {
    const r = e.getBoundingClientRect();
    return [r.x, r.y, r.width, r.height].map(rounded).join(',');
  };
  const norm = (s) => (s || '').replace(/\s+/g, ' ').trim();
  const shown = (e) => {
    const r = e.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
  };
  // The text of an element: the content, the host's shadow text for a
  // `relative-time`, or the placeholder or label of a control.
  const textOf = (e) => {
    let t = norm(e.textContent);
    if (e.localName === 'relative-time') {
      const span = e.shadowRoot && e.shadowRoot.querySelector('span[part=root]');
      t = span ? norm(span.textContent) : '';
    }
    return (
      e.localName +
      ':' +
      (t || e.getAttribute('placeholder') || e.getAttribute('aria-label') || e.getAttribute('alt') || '')
    ).slice(0, 100);
  };
  const part = (o, name, elements) => {
    const list = elements.filter(shown);
    o[name + 'Rects'] = list.map(rect);
    o[name + 'Texts'] = list.map(textOf);
  };
  const all = (selector, root) => Array.from((root || document).querySelectorAll(selector));
  // Elements of `selector` in the box x0..x1 by y0..y1 of the viewport.
  const inside = (selector, x0, y0, x1, y1) =>
    all(selector).filter((e) => {
      const r = e.getBoundingClientRect();
      return r.x >= x0 && r.y >= y0 && r.x < x1 && r.y < y1 && r.width > 0 && r.height > 0;
    });
  const byTestId = (id) => all('[data-testid="' + id + '"]');
  const keyed = (e) => Object.keys(e).some((k) => k.startsWith('__reactFiber$'));

  window.__s = () => {
    const o = {};
    o.url = location.href;
    o.title = document.title;
    o.scroll = [window.scrollX, window.scrollY].join(',');
    o.size = [document.documentElement.scrollWidth, document.documentElement.scrollHeight]
      .map(rounded)
      .join(',');

    // The header: the logo, the six menu buttons (closed), Pricing, the
    // search button, Sign in, Sign up and the button after them.
    const nav = document.querySelector('header nav');
    part(o, 'logo', all('header a[class*=logo]'));
    part(o, 'menus', all('button', nav));
    o.menusExpanded = all('button', nav)
      .filter(shown)
      .map((b) => b.getAttribute('aria-expanded'))
      .join(',');
    part(o, 'pricing', all('header a[class*=navLink]'));
    part(o, 'headerButtons', all('header button[class*=Button-module], header a[class*=Button-module]'));

    // The repository header: breadcrumbs, name, the tabs, the actions
    // (Notifications, Fork, Star).
    part(o, 'breadcrumbs', all('nav[aria-label=Breadcrumbs] a'));
    part(o, 'tabs', all('nav[aria-label=Repository] a'));
    part(o, 'repoName', byTestId('repo-name-link').concat(byTestId('repo-visibility-label')));
    part(o, 'actions', all('[data-testid=repo-header-actions] > li'));
    part(o, 'actionLinks', all('[data-testid=repo-header-actions] a'));

    // The row of the branch selector: its button, the counts, "Go to file"
    // and the Code button.
    part(o, 'branchBar', inside('main a, main button, main input', 0, 255, 960, 305));

    // The latest commit: the box, the author, the message, the hash, the
    // relative time, the check icon and the commit count.
    part(o, 'commitBox', byTestId('latest-commit'));
    part(
      o,
      'commit',
      inside(
        '[data-testid=latest-commit] a, [data-testid=latest-commit] button, ' +
          '[data-testid=latest-commit] img, [data-testid=latest-commit-details] a, ' +
          '[data-testid=latest-commit-details] relative-time, [data-testid=latest-commit-html]',
        0,
        300,
        1280,
        360,
      ).concat(inside('main a', 800, 300, 1280, 360)),
    );
    part(o, 'checks', byTestId('checks-status-badge-icon'));

    // The file list: the rows with the name, the last commit message and the
    // relative time.
    const rows = all('table[aria-labelledby=folders-and-files] tbody tr');
    o.rows = rows.length;
    part(o, 'rows', rows);
    const cell = (selector) => rows.flatMap((r) => all(selector, r)).filter(shown);
    part(o, 'names', cell('a.Link--primary'));
    // The name cell and the box that clips the name (`vertical-align: top` on
    // an inline block: both are 21 px high in Chromium).
    part(o, 'nameCells', cell('.react-directory-filename-cell'));
    part(o, 'nameClips', cell('.react-directory-truncate'));
    part(o, 'messages', cell('a.Link--secondary'));
    part(o, 'times', cell('relative-time'));

    // The sidebar: About, Resources, Releases and Contributors.
    part(o, 'sidebar', inside('main h2, main h3, main a, main strong, main img', 960, 255, 1280, 800));

    // The islands: for each `react-partial` and the `react-app` the elements
    // below it, the elements that carry a React fiber (React's own key
    // prefix on the DOM node: the element was rendered or hydrated by React),
    // the roots, the page's `loaded` class, and the length of the text.
    // The counts of the `react-app` are in their own keys: in one of ten
    // Chromium runs the app had two elements fewer (1,032 and 905 instead of
    // 1,034 and 906; a timing difference of the page), so the test compares
    // them within a tolerance.
    const counts = (e) => {
      const below = all('*', e);
      return [below.length, below.filter(keyed).length];
    };
    const describe = (e) =>
      [
        e.localName,
        e.getAttribute('partial-name') || e.getAttribute('app-name'),
        e.getAttribute('data-ssr'),
        e.classList.contains('loaded'),
        norm(e.textContent).length,
      ].join(' ');
    o.islands = all('react-partial').map((e) => describe(e) + ' ' + counts(e).join(' '));
    o.app = all('react-app').map(describe);
    o.appCounts = all('react-app').map((e) => counts(e).join(' '));
    return JSON.stringify(o);
  };
  return 1;
})()
