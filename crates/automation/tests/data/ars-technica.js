// The setup script of the Ars Technica test (`ars_technica.rs`): defines
// `window.__s()`, which returns the state of the features as a JSON string.
// Chromium (`swbtools replay --setup`) and swb run the same script after the
// load and after each group of steps. Nothing in the record depends on the
// A/B headline variant (`Math.random`), on the ads or on the third-party
// scripts that the fixture lacks. The script errors are not in the record:
// the test reads swb's console and compares it with a list.
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
    return (
      e.tagName +
      '#' +
      e.id +
      '[' +
      (e.getAttribute('aria-label') || e.getAttribute('data-theme') || '').slice(0, 40) +
      ']'
    );
  };
  // The cookies of the features: `theme` with its value, `fides_consent`
  // by name. The third-party scripts that the consent opens up (and the
  // A/B test cookies with their random values) are left out.
  const cookies = () => {
    const names = [];
    for (const part of document.cookie.split('; ')) {
      if (part.startsWith('theme=')) names.push(part);
      else if (part.startsWith('fides_consent=')) names.push('fides_consent');
    }
    return names.sort().join('; ');
  };
  const consent = () => {
    const raw = document.cookie.split('; ').find((c) => c.startsWith('fides_consent='));
    if (!raw) return null;
    try {
      const value = JSON.parse(decodeURIComponent(raw.slice('fides_consent='.length)));
      return Object.keys(value.consent || {})
        .sort()
        .map((k) => k + '=' + value.consent[k])
        .join(',');
    } catch (e) {
      return 'unparsed';
    }
  };
  window.__s = () => {
    const o = {};
    o.html = document.documentElement.className;
    // The story times that Alpine writes with `toLocaleTimeString` and
    // `toLocaleDateString` ("5:59 PM", "10/21/2026"; M9 feature 3, `Intl`).
    o.times = Array.from(document.querySelectorAll('time[x-data="arsTimestamp()"]'))
      .map((e) => e.textContent.trim())
      .join('|');
    o.bg = getComputedStyle(document.body).backgroundColor;
    o.cookies = cookies();
    o.consent = consent();
    // The consent dialog.
    const modal = document.getElementById('fides-modal');
    o.modal = visible(modal);
    o.modalRect = rect(modal && modal.querySelector('.fides-modal-container'));
    o.overlay = Array.from(document.querySelectorAll('.fides-modal-overlay, #fides-overlay'))
      .map((e) => (e.id || e.className.split(' ')[0]) + ':' + visible(e))
      .join(' ');
    o.banner = visible(document.getElementById('fides-banner'));
    o.tabs = Array.from(document.querySelectorAll('#fides-modal [role=tab]'))
      .map((e) => (e.textContent || '').trim().slice(0, 20) + '=' + e.getAttribute('aria-selected'))
      .join(' ');
    o.tabPanels = Array.from(document.querySelectorAll('#fides-modal [role=tabpanel]'))
      .map((e) => (e.id || '?').slice(0, 30) + ':' + visible(e))
      .join(' ');
    const accept = document.getElementById('fides-accept-all-button');
    o.acceptDisabled = accept ? accept.disabled : null;
    o.modalText = modal
      ? (modal.innerText || '').replace(/\s+/g, ' ').trim().slice(0, 200)
      : null;
    // The header menus.
    o.aria = Array.from(document.querySelectorAll('#site-header [aria-expanded]'))
      .map((e) => (e.getAttribute('aria-label') || '').slice(0, 32) + '=' + e.getAttribute('aria-expanded'))
      .join('; ');
    o.panels = Array.from(document.querySelectorAll('#site-header [x-ref=panel]'))
      .map((e, i) => i + ':' + visible(e))
      .join(' ');
    o.panelRects = Array.from(document.querySelectorAll('#site-header [x-ref=panel]'))
      .map((e, i) => (getComputedStyle(e).display === 'none' ? null : i + ':' + rect(e)))
      .filter(Boolean)
      .join(' ');
    // The theme switch.
    o.themeSel = Array.from(document.querySelectorAll('.theme-selection-item'))
      .map((e) => e.dataset.theme + ':' + e.dataset.selected)
      .join(' ');
    o.themeIcons = Array.from(document.querySelectorAll('[class*=theme-selection-active-]'))
      .filter((e) => e.tagName.toLowerCase() === 'svg' && getComputedStyle(e).display !== 'none')
      .map((e) => String(e.getAttribute('class')).match(/theme-selection-active-(\w+)/)[1])
      .join(',');
    // The dialogs (the grid settings and sign in dialogs are moved to body).
    o.dialogs = Array.from(document.querySelectorAll('[role=dialog]'))
      .map((e) => (e.getAttribute('aria-labelledby') ? 'L' : '') + visible(e) + (e.parentNode === document.body ? '@body' : ''))
      .join(' ');
    o.dialogRects = Array.from(document.querySelectorAll('[role=dialog]'))
      .filter((e) => getComputedStyle(e).display !== 'none')
      .map((e) => rect(e))
      .join(' ');
    o.gridInputs = document.querySelectorAll('[role=dialog] input').length;
    o.viewButtons = Array.from(document.querySelectorAll('button[name=view]'))
      .map((e) => e.value + (e.disabled ? ':d' : '') + (e.classList.contains('selected') ? ':s' : ''))
      .join(' ');
    // The sticky header.
    const header = document.getElementById('site-header');
    o.scrollY = Math.round(window.scrollY);
    o.headerTop = header ? header.style.top : null;
    o.headerRect = rect(header);
    // Focus.
    const active = document.activeElement;
    o.active = describe(active);
    o.focusVisible = active ? active.matches(':focus-visible') : null;
    return JSON.stringify(o);
  };
  return 1;
})();
