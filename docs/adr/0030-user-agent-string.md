# ADR 0030: User-agent string

- Status: accepted
- Date: 2026-10-31
- Replaces: the user-agent item of ADR 0025 ("unchanged") and of the
  ground rules of 2026-10-09.

## Context

Until now swb sent `Mozilla/5.0 (X11; Linux x86_64) swb/0.1.0`, in the
`User-Agent` header and as `navigator.userAgent`. The ground rules kept
this string, with a condition: "If browser sniffing causes real problems,
decide again."

M8 feature 11d found such a problem on a target page. Leaflet (the map of
the Craigslist posting) halves the wheel delta when the user agent string
does not contain "chrome". One wheel notch zoomed one level in swb and two
levels in Chromium, and the integration test had to send 240 where
Chromium got 120. Other pages also pick code paths by the string: some
show a basic page or an "unsupported browser" notice to browsers that they
do not know. swb measures its results against Chromium, so the code path
that pages take in Chromium is the one that swb is measured on.

## Decision

The owner decided on 2026-10-31: swb sends Chrome's user-agent string with
swb's own token at the end.

```
Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.0.0 Safari/537.36 swb/0.1.0
```

- The form is Chrome's reduced string: the major version only, the other
  parts zero. Other Chromium-based browsers also add a token at the end
  (Edge `Edg/...`, Opera `OPR/...`). Scripts that look for "Chrome" see
  Chrome, and a server log still shows swb.
- The major version is that of the Chromium that makes the reference
  measurements (148). Change it when the reference Chromium changes.
- One constant (`swb_net::USER_AGENT`) is the value of the header,
  `navigator.userAgent` and `navigator.appVersion` (without `Mozilla/`).
- Only the string changes. `navigator.vendor` stays empty, and swb adds no
  `navigator.userAgentData`, no `window.chrome` and no `Sec-CH-UA` request
  headers. Each of these claims more. Change them only when a target page
  needs it.

Rejected: only adding `Chrome/148` to the old string. Scripts that detect
browsers expect the full form with `AppleWebKit` and `Safari`, and a
partial string can make them detect the wrong browser.

## Consequences

- Leaflet zooms two levels per wheel notch, as in Chromium. The Craigslist
  test sends the wheel delta of Chromium's records (120).
- `compare --js` is unchanged on the three targets (Craigslist 0.9804 /
  0.9976, Hacker News 1.0000 / 0.9986, Wikipedia 0.9991 / 0.9967; geometry
  / pixels). The fixtures are replays, so their responses do not depend on
  the header; only the scripts see the new string.
- Pages send swb the code that they write for Chrome. Where that code uses
  an API that swb does not have, a page can fail where its general code
  worked. Report such pages as backlog items of the missing API.
- Risk: bot-protection services can compare the browser that the string
  claims with the TLS and HTTP fingerprint of the connection and with the
  `Sec-CH-UA` headers. swb's do not look like Chrome's, so such services
  can show swb more challenge pages than before. The size of this effect is
  not known. Tests cannot measure it, because they do not use the network.
