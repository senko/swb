// Record of the interactions with github.com/senko/swb (M10 9c). Setup script
// of `swbtools replay github-swb`: it defines `window.__i()`, which returns
// the state of the page as a JSON string. The same text runs in swb
// (`github_swb.rs`) and in Chromium.
window.__i = () => {
  const rd = (v) => Math.round(v);
  const rect = (e) => {
    const b = e.getBoundingClientRect();
    return [b.x, b.y, b.width, b.height].map(rd).join(',');
  };
  const norm = (s) => (s || '').replace(/\s+/g, ' ').trim();
  const visible = (e) => {
    const b = e.getBoundingClientRect();
    if (b.width <= 0 || b.height <= 0) return false;
    const cs = getComputedStyle(e);
    return cs.visibility !== 'hidden' && cs.display !== 'none';
  };
  // A name of the element for the record. React's `useId` values (`_r_1_`)
  // are not stable between browsers, so they are left out.
  const name = (e) => {
    if (!e) return 'none';
    return [
      e.localName,
      e.getAttribute('data-testid') || '',
      e.getAttribute('aria-label') || '',
      e.getAttribute('type') || '',
      e.id && !/^_r_|^_R_/i.test(e.id) ? '#' + e.id : '',
      norm(e.textContent).slice(0, 24),
    ].join('|');
  };
  const o = {};
  o.url = location.href;
  o.scroll = [window.scrollX, window.scrollY].join(',');
  const a = document.activeElement;
  o.active = name(a);
  o.activeRect = a ? rect(a) : '';
  o.focusVisible = a ? a.matches(':focus-visible') : false;
  const cs = a ? getComputedStyle(a) : null;
  o.focusOutline = cs ? cs.outlineStyle + ' ' + cs.outlineWidth + ' ' + cs.outlineColor : '';
  o.hasFocus = document.hasFocus();
  o.expanded = Array.from(document.querySelectorAll('[aria-expanded]'))
    .filter((e) => visible(e) && e.getAttribute('aria-expanded') === 'true')
    .map((e) => name(e))
    .join(' ; ');
  o.popovers = Array.from(document.querySelectorAll('[popover]'))
    .filter((e) => e.matches(':popover-open'))
    .map((e) => e.localName + '#' + (/^_r_/i.test(e.id) ? '_r' : e.id) + ' ' + rect(e) + ' ' + norm(e.textContent).slice(0, 60))
    .join(' ; ');
  o.dialogs = Array.from(document.querySelectorAll('dialog[open], [role=dialog], [role=menu], [role=listbox], [role=tooltip], [aria-modal=true]'))
    .filter(visible)
    .map((e) => name(e) + ' ' + e.getAttribute('role') + ' ' + rect(e) + ' ' + norm(e.textContent).slice(0, 80))
    .join(' ; ');
  o.counts = [document.querySelectorAll('*').length, document.body.children.length].join(',');
  o.bodyTail = Array.from(document.body.children)
    .slice(-4)
    .map((e) => e.localName + '.' + String(e.className).slice(0, 24) + (visible(e) ? ' ' + rect(e) : ' hidden'))
    .join(' ; ');
  // The header menus: the visible links of the open menu.
  const header = document.querySelector('header');
  o.menuLinks = header
    ? Array.from(header.querySelectorAll('a')).filter((e) => e.getBoundingClientRect().height > 0 && getComputedStyle(e).visibility !== 'hidden' && e.getBoundingClientRect().y > 60).length
    : -1;
  o.inputs = Array.from(document.querySelectorAll('input, textarea'))
    .filter(visible)
    .map((e) => name(e) + ' ' + rect(e) + ' v=' + e.value)
    .join(' ; ');
  return JSON.stringify(o);
};
1
