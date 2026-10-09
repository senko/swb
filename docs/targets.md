# Target pages

The owner provides these URLs. Each page must be usable in swb: it renders
close to Chromium (with JavaScript disabled), scrolls, links work, and
relevant interactions work.

Status values: `pending` (not started), `in progress`, `done` (reported to
the owner as ready for review), `accepted` (the owner verified it).

| # | URL                                          | Fixture                          | Status  | Notes |
|---|----------------------------------------------|----------------------------------|---------|-------|
| 1 | https://senko.net/                           | `fixtures/pages/senko-net`       | accepted | Layout matches Chromium (all 65 element boxes within 0.1 px at 1280×800); the narrow layout (`max-width: 480px`) looks the same in a side-by-side screenshot. Hover underline, cursor, Tab focus rings, text selection and copy work. |
| 2 | https://news.ycombinator.com/                | `fixtures/pages/hacker-news`     | accepted | Layout matches Chromium (all 818 element boxes within 2 px at 1280×800; quirks mode, nested tables); logo and vote arrows (SVG) match. Links, the search form (GET to hn.algolia.com) and back/forward work. Login (POST and the session cookie) is implemented and tested with local servers; it was not tried against the live site. |
| 3 | https://en.wikipedia.org/wiki/Web_browser    | `fixtures/pages/wikipedia-web-browser` | accepted | Layout matches Chromium (geometry 0.9978: all but 9 of 4,052 element boxes within 2 px at 1280×800; pixels 0.9936): grid page layout, floats, positioning and the sticky table of contents, mask icons, the video thumbnail, counters in the references, line breaking. The main menu and other dropdowns open (checkbox, no JavaScript), table of contents links scroll, the search form submits (GET), links and history work. |
| 4 | https://arstechnica.com/                     | `fixtures/pages/ars-technica`    | accepted | M4. `aspect-ratio`, `:host`, `sizes="auto"`, web fonts, inline SVG and its raster cost, rounded overflow clips. Layout matches Chromium (geometry 1.0000: all 1,093 element boxes within 2 px at 1280×800; pixels 0.9992). Links (also in inline SVG) and the page's `:hover` rules are covered by engine tests. The page has no form with JavaScript off. |
| 5 | https://www.bbc.com/                         | `fixtures/pages/bbc`             | accepted | M5. Web fonts and inline SVG (M4); `details` and `summary`; grid buttons; glyph edge precision. Layout matches Chromium (geometry 1.0000: all 2,745 element boxes within 2 px at 1280×800; pixels 0.9987, full page 0.9987). The no-JavaScript menu opens and closes with a click or Enter/Space and matches Chromium when open. Links (cards, SVG logo and section titles, the menu) and the page's `:hover` rules are covered by engine tests. The search field is disabled without JavaScript. |

Scores per fixture are in `fixtures/scores.json` (see
[testing.md](testing.md)). `just compare` writes a report per fixture to
`out/compare/<name>/report.html`.
