window.__o = () => {
  const visible = (e) => {
    const r = e.getBoundingClientRect();
    const s = getComputedStyle(e);
    return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none';
  };
  const text = (e, max = 500) => (e ? e.innerText.replace(/\s+/g, ' ').trim().slice(0, max) : null);
  const count = (root, selector) => (root ? root.querySelectorAll(selector).length : 0);
  const titles = (root) => (root
    ? [...root.querySelectorAll('h1, h2, h3')].filter(visible).map((h) => text(h, 80))
    : []);
  // The rectangles of the visible elements that the items of M12 depend on:
  // per area a list of [selector, index among the visible matches, left, top,
  // width, height] in CSS px, rounded to 0.01 (at most 40 matches a selector).
  const areas = {
    header: ['header', 'header a', 'header .btn', 'header img'],
    map: [
      '#map', '.control-button', '.leaflet-control-scale', '.leaflet-control-scale > div',
      '.leaflet-control-attribution', '.leaflet-marker-pane > *', '.leaflet-popup',
    ],
    sidebar: [
      '#sidebar', '#sidebar form', '#sidebar input', '#sidebar select', '#sidebar label',
      '#sidebar button, #sidebar .btn', '#sidebar .btn-close', '#sidebar_content',
      '#sidebar_content h2, #sidebar_content h3, #sidebar_content h4, #sidebar_content h5',
      '#sidebar_content p', '#sidebar_content details', '#sidebar_content summary',
      '#sidebar_content table', '#sidebar_content tr', '#sidebar_content td',
      '#sidebar_content ul', '#sidebar_content li', '#sidebar_content nav',
      '#sidebar_content .pagination > *', '#sidebar_content img', '#sidebar_content dt',
      '#sidebar_content dd',
    ],
    panel: [
      '#map-ui', '#map-ui h2', '#map-ui h4', '#map-ui .btn-close', '.base-layers > div',
      '.base-layers label', '.base-layers .badge', '.layer-info-toggle', '.layer-info-toggle i',
      '.overlay-layers p', '.overlay-layers .form-check', '.overlay-layers label',
      '.overlay-layers input', '.legend-ui table', '.legend-ui tr', '.legend-ui img',
      '.share-link form', '.share-link .form-check', '.share-link label', '.share-link input',
      '#nav-tab', '#nav-tab a', '#nav-tab > span', '#nav-tabContent textarea',
      '#nav-tabContent p', '.share-geo-uri a', '#export-image .row', '#export-image label',
      '#export-image select', '#export-image .col-auto', '#export-image .form-check',
      '#export-image input', '#export-image p',
    ],
  };
  const round = (v) => Math.round(v * 100) / 100;
  const rects = Object.fromEntries(Object.entries(areas).map(([area, selectors]) => [
    area,
    selectors.flatMap((selector) => [...document.querySelectorAll(selector)]
      .filter(visible).slice(0, 40).map((e, i) => {
        const r = e.getBoundingClientRect();
        return [selector, i, round(r.left), round(r.top), round(r.width), round(r.height)];
      })),
  ]));
  const view = location.hash.match(/^#map=(\d+)\/(-?[\d.]+)\/(-?[\d.]+)/);
  const tiles = [...document.querySelectorAll('#map .leaflet-tile-pane img.leaflet-tile')];
  const inViewport = (e) => {
    const r = e.getBoundingClientRect();
    return r.right > 0 && r.bottom > 0 && r.left < innerWidth && r.top < innerHeight;
  };
  const shown = tiles.filter(inViewport);
  const loaded = shown.filter((i) => i.complete && i.naturalWidth > 0);
  const base = document.querySelector('.layers-ui input[name=layer]:checked');
  const overlays = ['notes', 'data', 'gps'].filter((n) => {
    const box = document.querySelector('#label-layers-' + n + ' input');
    return box && box.checked;
  });
  const mapUi = document.querySelector('#map-ui');
  const sidebar = document.querySelector('#sidebar_content');
  const paths = [...document.querySelectorAll('#map .leaflet-overlay-pane path')];
  const markers = [...document.querySelectorAll('#map .leaflet-marker-pane > *')];
  const welcome = document.querySelector('.welcome');
  return JSON.stringify({
    url: location.pathname + location.search,
    hash: location.hash,
    title: document.title,
    view: view ? {zoom: +view[1], lat: +view[2], lon: +view[3]} : null,
    tiles: {
      shown: shown.length,
      loaded: loaded.length,
      srcs: [...new Set(loaded.map((i) => i.src))].sort(),
    },
    base: base ? base.id.replace('map-ui-layer-', '') : null,
    overlays,
    mapUi: {open: !!mapUi && visible(mapUi), titles: titles(mapUi), text: text(mapUi)},
    sidebar: {titles: titles(sidebar), text: text(sidebar, 800)},
    counts: {
      li: count(sidebar, 'li'),
      tr: count(sidebar, 'tr'),
      listGroupItem: count(sidebar, '.list-group-item'),
      links: count(sidebar, 'a'),
      searchResults: count(sidebar, 'li.search_results_entry'),
      routeSteps: count(sidebar, 'tr.turn'),
      queryNearby: count(sidebar, '#query-nearby .query-results-list > li'),
      queryEnclosing: count(sidebar, '#query-isin .query-results-list > li'),
    },
    outlines: {
      paths: paths.length,
      d: paths.map((p) => p.getAttribute('d') || ''),
      strokes: [...new Set(paths.map((p) => p.getAttribute('stroke')))].sort(),
    },
    markers: markers.length,
    share: {
      tab: text(document.querySelector('#nav-tab .active'), 40),
      longInput: (document.querySelector('#long_input') || {}).value || null,
      shortInput: (document.querySelector('#short_input') || {}).value || null,
      embedHtml: (document.querySelector('#embed_html') || {}).value || null,
      marker: !!(document.querySelector('#link_marker') || {}).checked,
    },
    welcome: !!welcome && visible(welcome),
    scale: text(document.querySelector('.leaflet-control-scale')),
    attribution: text(document.querySelector('.leaflet-control-attribution'), 200),
    elements: document.getElementsByTagName('*').length,
    rects,
  });
};
