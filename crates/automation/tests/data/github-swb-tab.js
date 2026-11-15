// Record of the Tab order of github.com/senko/swb (M10 9c). Setup script of
// `swbtools replay github-swb`: it defines `window.__t()`, which returns the
// focused element as a JSON string: its name, its rectangle, `:focus-visible`,
// its focus ring and the scroll position.
//
// Rectangles and the scroll are relative to the header (the diagnosis of
// feature 9, section 3.1): the page's header island shows an error heading
// (14 px, the header wrapper is 86 px instead of 72 px high) when the lazy
// chunks of the search dialog fail, and the stop of the Tab order at which
// that happens differs between runs of both browsers. A box below the header
// is reported as if the heading had no height. Fixed boxes and boxes inside
// the header wrapper are not moved.
window.__t = () => {
  const norm = (s) => (s || '').replace(/\s+/g, ' ').trim();
  const name = (e) =>
    [
      e.localName,
      e.getAttribute('data-testid') || '',
      e.getAttribute('aria-label') || '',
      e.getAttribute('type') || '',
      e.getAttribute('href') || '',
      norm(e.textContent).slice(0, 20),
    ]
      .join('|')
      .slice(0, 110);
  const wrapper = document.querySelector('.header-wrapper');
  const delta = wrapper ? Math.round(wrapper.getBoundingClientRect().height) - 72 : 0;
  const e = document.activeElement;
  const b = e.getBoundingClientRect();
  const cs = getComputedStyle(e);
  const shifted = !(wrapper && wrapper.contains(e)) && cs.position !== 'fixed';
  const y = shifted ? b.y - delta : b.y;
  const scroll = window.scrollY > 0 ? window.scrollY - delta : 0;
  return JSON.stringify({
    active: name(e),
    rect: [b.x, y, b.width, b.height].map(Math.round).join(','),
    focusVisible: e.matches(':focus-visible'),
    outline: cs.outlineStyle + ' ' + cs.outlineWidth + ' ' + cs.outlineColor + ' ' + cs.outlineOffset,
    scroll: Math.round(scroll),
  });
};
1
