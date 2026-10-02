# Target pages

The owner provides these URLs. Each page must be usable in swb: it renders
close to Chromium (with JavaScript disabled), scrolls, links work, and
relevant interactions work.

Status values: `pending` (not started), `in progress`, `done` (reported to
the owner as ready for review), `accepted` (the owner verified it).

| # | URL                                          | Fixture                          | Status  | Notes |
|---|----------------------------------------------|----------------------------------|---------|-------|
| 1 | https://senko.net/                           | `fixtures/pages/senko-net`       | done    | Layout matches Chromium (all 65 element boxes within 0.1 px at 1280×800); the narrow layout (`max-width: 480px`) looks the same in a side-by-side screenshot. Hover underline, cursor, Tab focus rings, text selection and copy work. |
| 2 | https://news.ycombinator.com/                | `fixtures/pages/hacker-news`     | in progress | Readable. Needs table layout, SVG logo, `<center>` alignment (M2). |
| 3 | https://en.wikipedia.org/wiki/Web_browser    | `fixtures/pages/wikipedia-web-browser` | in progress | Readable. Needs grid (sidebar), floats (images), SVG, `mask-image` icons (M3). |

Scores per fixture are in `fixtures/scores.json` (see
[testing.md](testing.md)). `just compare` writes a report per fixture to
`out/compare/<name>/report.html`.
