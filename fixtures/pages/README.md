# Page fixtures: licensing

The directories here contain snapshots of third-party web pages and their
resources (HTML, CSS, images, fonts, SVG files), recorded for offline
tests (see [docs/testing.md](../../docs/testing.md)). They are not swb's
work, swb's MIT License does not apply to them, and they are not part of
swb binaries. Each snapshot keeps the license of its source:

| Fixture | Source | License of the content |
|---------|--------|------------------------|
| `senko-net` | https://senko.net/ | Content of the site owner (the project owner). |
| `hacker-news` | https://news.ycombinator.com/ | Content of Y Combinator and of the users who posted it; Hacker News terms of use. |
| `wikipedia-web-browser` | https://en.wikipedia.org/wiki/Web_browser | Text under CC BY-SA 4.0 (Wikipedia contributors); images and other media under their own licenses (see each file's page on Wikimedia Commons); MediaWiki styles and scripts under the licenses of the MediaWiki project. |

The `reference/` files (Chromium's box dumps and screenshots) are derived
from these snapshots.
