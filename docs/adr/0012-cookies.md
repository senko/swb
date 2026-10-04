# ADR 0012: Cookies

- Status: accepted (the license question in "Public Suffix List" was
  decided in ADR 0014)
- Date: 2026-10-02
- Updated: 2026-10-03 (ADR 0013: the initiator in history; ADR 0014: the
  license of the Public Suffix List)

## Context

Milestone M2 targets https://news.ycombinator.com/. Its login form posts to
`login`, and the session is a cookie. swb had no cookies: the HTTP client
sent no `Cookie` header and ignored `Set-Cookie`. Form submission needs
`POST` requests with the `Origin` header, and cookies need to know whether
a request is same-site (`SameSite`).

Constraints: content from the network must never cause a panic, memory must
be bounded, tests must not use the network, and dependencies must follow
[ADR 0003](0003-dependency-policy.md).

## Decision

### Specification

The jar follows RFC 6265bis
(https://datatracker.ietf.org/doc/draft-ietf-httpbis-rfc6265bis/): parsing
`Set-Cookie` (§5.6), cookie dates (§5.1.1), the storage model (§5.7) and
retrieval (§5.8). Where the draft leaves a choice to the user agent, swb
does what Chromium does:

- A cookie without `SameSite` (or with an unknown value) is stored as
  `Default` and treated as `Lax` ("Lax by default").
- `SameSite=None` requires `Secure`.
- Sites are schemeful: `http://a.com` and `https://a.com` are different
  sites.
- `http:` URLs to `localhost`, `127.0.0.0/8` and `::1` count as secure:
  they can set and get `Secure` cookies (Chromium since version 89).
  Deviation: Chromium also counts `*.localhost`, because it resolves those
  names to loopback itself. swb uses the system resolver, which can send
  `*.localhost` to a DNS server, so swb does not count them. Plain
  `localhost` comes from `/etc/hosts`.
- Requests to loopback hosts (`localhost`, `*.localhost`, `127.0.0.0/8`,
  `::1`) never use a proxy, as in Chromium (its implicit `<-loopback>`
  bypass rule). Without this, `HTTP_PROXY` would send the `Secure` cookies
  of `http://localhost` to the proxy in cleartext. The HTTP client has a
  second `ureq` agent without a proxy for these hosts.
- Lifetime at most 400 days; name and value at most 4096 bytes together;
  attribute values at most 1024 bytes (longer attributes are ignored).
- Deviation: a cookie whose path or domain is longer than 1024 bytes is
  rejected. The draft limits only attribute values, but the default path
  and a host-only domain come from the URL, which has no length limit. The
  limit bounds the memory of the store.
- Limits from Chromium's `CookieMonster`: 180 cookies per registrable
  domain (above that, the least recently used are evicted down to 150),
  3300 in total (down to 3000). swb evicts only by the last access time.
  Chromium also considers `Secure` and the `Priority` attribute, and its
  global eviction does not remove cookies used in the last 30 days.
- The `Domain` attribute is canonicalized with the URL host parser:
  internationalized names become Punycode, and IPv4 forms are normalized
  (`0x7f.1` becomes `127.0.0.1`). An attribute that is not a valid host
  rejects the cookie. This follows Chromium; the draft rejects a
  non-ASCII attribute instead.
- The `Domain` attribute must have the same registrable domain as the
  request host and domain-match it, as in Chromium's
  `cookie_util::GetCookieDomainWithString`. This rejects public suffixes.
  A host without a registrable domain (an IP address, a public suffix, a
  host under a top-level domain that is not in the list) accepts only
  `Domain=<the host>`, and the cookie is then host-only.
- Third-party cookies (`SameSite=None; Secure` on cross-site requests) are
  allowed, as in Chromium's default settings.
- The prefixes `__Secure-` and `__Host-` are compared case-insensitively.
  A `__Host-` cookie must not have a `Domain` attribute at all, as in
  Chromium; `Domain=<the host>` is not enough, even where it gives a
  host-only cookie.
- A rejected cookie is logged at `warn!` if it is invalid in a response
  from its host, whatever the request: malformed, empty name and value,
  invalid or wrong `Domain`, prefix rules, `SameSite=None` without
  `Secure`. It is logged at `debug!` if the rejection depends on the
  request: cross-site, `Secure` from an insecure URL, a `Secure` cookie
  that it would replace, or a path or domain longer than 1024 bytes (the
  long value comes from the request URL, because longer attribute values
  are ignored). The log shows the cookie name, or `<nameless>`, never the
  value.
- `Cookie` and `Origin` in `Request::headers` are ignored: only the
  fetcher sets them (Fetch's "forbidden request-header" names). A caller's
  value would replace the jar's cookies and would be sent again on every
  redirect hop, also to other sites.

Not implemented: the non-HTTP API (`document.cookie`; `HttpOnly` is stored
but has no effect yet), `Partitioned` (the attribute is ignored, so such
cookies are stored unpartitioned), `Priority`, the `__Http-` prefixes,
Chromium's "Lax+POST" exception (a cookie without `SameSite` younger than
2 minutes is sent with a cross-site top-level `POST`), and the optional
redirect-chain rule for `SameSite` (Chromium has it off by default).

Known difference: header values that are not valid UTF-8 are decoded as
Latin-1 (`swb_net::Headers`). A cookie value with such bytes is sent back
in UTF-8, so the bytes change (`0xE9` becomes `C3 A9`), and the 4096-byte
limit counts the UTF-8 bytes. Chromium keeps the raw bytes. Such values are
rare; storing bytes would need a byte-based header type.

### Public Suffix List

The registrable domain ("eTLD+1") comes from the Public Suffix List
(https://publicsuffix.org/). swb uses the `psl` crate (2.1,
`MIT OR Apache-2.0`, one dependency, `psl-types`, same license; `no_std`,
no `unsafe`). It compiles the list into a match tree, so lookups need no
parsing at startup and no allocation. It is released with each list
update; `cargo update -p psl` refreshes the data. Both sections of the list
(ICANN and private domains such as `github.io`) are used, as in browsers.
Only `crates/net/src/site.rs` uses the crate.

Deviation from the URL spec, as in Chromium ("exclude unknown
registries"): a host under a top-level domain that is not in the list
(`localhost`, `nas.lan`, `a.test`) has no registrable domain, so it is its
own site. The URL spec applies the list's implicit `*` rule instead.

Open question for the owner: the list itself is licensed under MPL-2.0.
The `psl` crate declares `MIT OR Apache-2.0` (so `cargo deny` accepts it),
but its generated code is derived from the MPL-2.0 data. ADR 0003 excludes
MPL-2.0 code and says nothing about data, except fonts. Every browser ships
this list. The alternatives are to embed `public_suffix_list.dat` as a data
file (the same license question, plus a small parser), or to have no list,
which is unsafe: `Domain=co.uk` would be accepted, and `a.co.uk` and
`b.co.uk` would be one site. swb uses `psl` until the owner decides.

Update (2026-10-03): the owner allowed the list as a one-off exception to
ADR 0003, with the license stated clearly in the repository and in
`deny.toml`; see [ADR 0014](0014-public-suffix-list-license-exception.md).

### Where the jar lives

Each `NetworkFetcher` has one `CookieJar`, inside its HTTP client. The
client adds the `Cookie` header to each request and stores the
`Set-Cookie` headers of each response when the headers arrive (before the
body). `fetch_following_redirects` calls the fetcher once per redirect
hop, so every hop sends and stores cookies.

- The binary creates one fetcher per process, and all pages use it (the
  GUI, the headless runner, the automation server). So there is one jar
  per browser session.
- `RecordingFetcher` and `ExtendingFetcher` wrap a `NetworkFetcher`, so a
  recording session has cookies too. It starts with an empty jar, as every
  swb process does. Record fixtures without logging in: a page recorded
  after a login contains session data (on Hacker News, the `auth=` tokens
  of the `logout` and `vote` links), and the fixtures are committed.
- `ReplayFetcher` has no jar. Fixtures store only `content-type` and
  `location` (see `swb_net::fixture`), so a replay never sees `Set-Cookie`.
  Keep it that way: replays then do not depend on cookie state.
- `Fetcher::cookie_jar()` gives access to the jar: `NetworkFetcher`
  returns its jar, the recording fetchers return the jar of the fetcher
  they wrap, other fetchers return none. `Loader::cookie_jar()` and
  `Page::cookie_jar()` pass it on. The automation methods `cookies.get` and
  `cookies.clear` use it.

A wrapping "cookie fetcher" was the alternative. It would work with any
fetcher, but every place that builds a fetcher (the binary, tests) would
have to compose it correctly, and it would have to skip non-HTTP URLs
itself. Cookies are HTTP state, like the connection pool, so they belong to
the HTTP client.

The jar is in memory only; swb starts with no cookies. A file store needs
a format, a location, and expiry on load. A login lasts for the session,
which is enough for M2.

The store is a `Mutex` around a map from site host (registrable domain or
host) to the cookies of that group. Retrieval looks only at the group of
the request host, and the per-domain limit counts one group. As in
Chromium, which also keys cookies by registrable domain, this differs from
the draft in one case: under a private rule of the list, a cookie with
`Domain=amazonaws.com` (set by `www.amazonaws.com`) is not sent to
`bucket.s3.amazonaws.com`, whose registrable domain is itself. Creation
and access order use a logical clock, so cookies set in the same second
keep their order, and tests are deterministic. The public methods use the
system clock; tests call internal variants with an explicit time, so no
test sleeps.

### Same-site context

`Request` has a new field, `initiator: Option<Origin>`: the origin of the
document that started the request (Fetch's request origin). `None` means
that the user started the request. A top-level navigation without an
initiator counts as same-site. A subresource request without an initiator
counts as cross-site: only the user starts requests without an initiator,
and the user starts only navigations, so such a request is a bug in the
caller, and it gets the fewest cookies. The engine sets the initiator:

- Stylesheets and images: the document's origin.
- Links (`Page::follow_link`): the document's origin.
- Addresses typed by the user and `page.navigate` (automation), reload,
  back and forward: `None`.
- Form submissions (another change in M2) must set the form's document
  origin.

Deviation: swb treats reload and history traversal as user-initiated, so
they send all cookies, `SameSite=Strict` included. RFC 6265bis §5.2.1
makes a reload same-site only if the original navigation was same-site,
and Chromium keeps the initiator in the history entry
(`FrameNavigationEntry::initiator_origin`) and uses it again. So after a
cross-site link, a reload in swb sends `Strict` cookies that Chromium does
not send. To fix this, store the initiator in the pending navigation and
in the history entry. This was left out to keep this change out of the
navigation code, which form submission changes at the same time.

Update (ADR 0013): the forms change implements this. The pending
navigation and the history entry keep the initiator, and reload, back
and forward use it again, as RFC 6265bis §5.2.1 requires and as Chromium
does. Form submissions set the origin of the form's document. The
deviation above and the item "the initiator for reload and history
traversal" under "Later work" no longer apply.

`Destination::Document` means a top-level navigation (Fetch's `document`
destination). Frames, when swb supports them, need their own destination,
because cookies treat them as subresources.

swb has no frames, so the "site for cookies" of a subresource is the site
of its initiator (the top-level document), and that of a top-level
navigation is the site of its URL. This gives Chromium's three contexts:

| Context | When | Cookies sent | Cookies set |
|---------|------|--------------|-------------|
| same-site | top-level navigation without an initiator, or the initiator is same-site with the URL | all | all |
| cross-site navigation | top-level navigation from a cross-site document | `None`; `Lax` and `Default` only for `GET` | all |
| cross-site | subresource from a cross-site document, or without an initiator | `None` | only `None` |

An opaque initiator (a `data:` or `file:` document) is cross-site with
everything. Each redirect hop computes its own context.

### POST requests

- `Request::post(url, body, content_type, destination)` sets the method,
  the body and `Content-Type`. `ureq` adds `Content-Length` (also `0` for
  an empty body).
- A request whose method is not `GET` and that has an initiator sends
  `Origin`, as Fetch specifies with the default referrer policy
  (`strict-origin-when-cross-origin`): the serialized origin, or `null`
  for an opaque origin and for a request from an `https:` origin to a URL
  that is not `https:`. Not implemented: the "redirect-tainted origin"
  (`null` after a redirect chain that leaves the initiator's origin and
  then goes to a third origin).
- Redirects (unchanged, tested): 301 and 302 change `POST` to `GET`
  without a body, 303 changes any method to `GET`, 307 and 308 keep the
  method and the body. The body headers (`Content-Type` and others) are
  removed when the method changes.

## Consequences

- Hacker News login can work in live mode. Replays of fixtures never have
  cookies, and fixtures must be recorded logged out.
- Content from the network cannot make the jar panic or grow without
  bound: at most 3300 cookies of at most about 6 KiB each (name and value,
  domain, path), so about 20 MB in the worst case. Network tests use a
  local server; `http://127.0.0.1` counts as secure, so `Secure` and
  `SameSite=None` cookies can be tested locally.
- Tests that need `Domain` cookies must use real public suffixes
  (`example.com`): `.test` is not in the list, so `a.test` has no
  registrable domain.
- Later work: the initiator for reload and history traversal,
  persistence, `document.cookie` (with JavaScript), frames (a "site for
  cookies" field on `Request`), `Partitioned` cookies, the
  redirect-tainted `Origin`, `Sec-Fetch-*` and `Referer` headers.
