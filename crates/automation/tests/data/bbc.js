// The setup script of the BBC test (`bbc.rs`): defines `window.__s()`, which
// returns the state of the features as a JSON string. Chromium (`swbtools
// replay --setup`) and swb run the same script after the load and after each
// group of steps. The record has nothing that depends on the ads, the
// analytics or the Optimizely experiments, on the time (`ckns_policy_exp`) or
// on the consent frame. The script errors are not in the record: the test
// reads swb's console and compares it with a list.
(() => {
  const rect = (e) => {
    if (!e) return null;
    const r = e.getBoundingClientRect();
    return [r.x, r.y, r.width, r.height].map((v) => Math.round(v)).join(',');
  };
  const visible = (e) => {
    if (!e) return 'none';
    const c = getComputedStyle(e);
    return c.display + '/' + c.visibility;
  };
  const describe = (e) => {
    if (!e) return null;
    const label = e.getAttribute('aria-label') || e.getAttribute('data-testid') || '';
    return e.tagName + '#' + e.id + '[' + label.slice(0, 40) + ']';
  };
  const one = (selector) => document.querySelector(selector);
  const all = (selector) => Array.from(document.querySelectorAll(selector));
  // The cookies of the cookie banner: `ckns_explicit` and `ckns_policy` with
  // their values. The other cookies carry times or random ids.
  const cookies = () =>
    document.cookie
      .split('; ')
      .filter((c) => c.startsWith('ckns_explicit=') || c.startsWith('ckns_policy='))
      .sort()
      .join('; ');
  const isPlaceholder = (img) => (img.getAttribute('src') || '').includes('grey-placeholder');

  // A carousel: the scroller (the element in the section with a horizontal
  // `overflow` of `scroll` or `auto`: the row of cards), its two arrows and
  // its cards.
  const carousel = (grid) => {
    const row = Array.from(grid.querySelectorAll('*')).find((e) =>
      ['scroll', 'auto'].includes(getComputedStyle(e).overflowX),
    );
    if (!row) return { row: 'none' };
    const box = row.getBoundingClientRect();
    const shown = Array.from(row.children).filter((card) => {
      const r = card.getBoundingClientRect();
      return r.right > box.left + 1 && r.left < box.right - 1;
    });
    const first = Array.from(row.children).indexOf(shown[0]);
    let chevrons = null;
    for (let e = grid.parentElement; e && !chevrons; e = e.parentElement) {
      chevrons = e.querySelector('[data-testid=left-arrow]');
    }
    const next = chevrons && chevrons.nextElementSibling;
    return {
      left: Math.round(row.scrollLeft),
      width: row.scrollWidth + '/' + row.clientWidth,
      cards: row.children.length,
      shown: shown.length,
      first,
      previous: chevrons ? chevrons.disabled : null,
      next: next ? next.disabled : null,
    };
  };

  window.__s = () => {
    const o = {};
    o.scrollY = Math.round(scrollY);
    o.active = describe(document.activeElement);
    o.bodyOverflow = getComputedStyle(document.body).overflowY;
    o.cookies = cookies();
    // The links that clicks followed (the recorder of the steps stops the
    // navigation).
    o.clicked = (window.__clicked || []).join(' ');
    // The cookie banner.
    const banner = one('#bbccookies-prompt');
    o.banner = banner ? visible(banner) + ' ' + rect(banner) : 'removed';
    // The hydrated header: the menu button, Register and Sign In.
    const menu = one('button[aria-label$=" menu"]');
    o.menuButton = describe(menu) + ' expanded=' + (menu && menu.getAttribute('aria-expanded'));
    o.headerRects = [menu, one('button[aria-label=Register]'), one('button[aria-label="Sign In"]')]
      .map(rect)
      .join(' ');
    // The navigation panel: its box, the search field and the section News
    // with its sub items.
    o.panel = rect(one('[data-testid=drawer-background]'));
    const search = one('[data-testid=search-input-field]');
    o.search = rect(search) + ' disabled=' + (search && search.disabled);
    const news = one('[data-testid="level0NavContent-/news"]');
    o.news = news
      ? rect(news) + ' items=' + news.querySelectorAll('button, a').length
      : 'collapsed';
    o.newsButton = rect(one('button[data-testid="level0NavButton-/news"]'));
    // The first cards.
    o.cards = all('[data-testid$="-card"]').slice(0, 6).map(rect).join(' ');
    // The carousels: Iowa, Texas and the second Iowa.
    o.carousels = all('[data-testid=iowa-grid], [data-testid=texas-grid]').map((grid) =>
      JSON.stringify(carousel(grid)),
    );
    // The "Save" popover.
    const popovers = all('[data-testid=popoverStyled]');
    o.popovers = popovers.length;
    o.popover = popovers.length
      ? visible(popovers[0]) + ' ' + rect(popovers[0]) + ' ' + popovers[0].textContent
      : 'none';
    // The language dropdown.
    const list = one('[class*=LanguageDropdown-styles__DropdownContentWrapperStyled]');
    o.language = list
      ? visible(list) + ' ' + rect(list) + ' hidden=' + list.getAttribute('aria-hidden')
      : 'none';
    // The images: all, the grey placeholders that the page replaces when
    // the lazy image has loaded, and the loaded photos.
    const images = Array.from(document.images);
    o.images = images.length;
    o.placeholders = images.filter(isPlaceholder).length;
    o.loaded = images.filter((i) => !isPlaceholder(i) && i.complete && i.naturalWidth > 0).length;
    return JSON.stringify(o);
  };
  return 1;
})();
