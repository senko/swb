# ADR 0005: Testing strategy

- Status: accepted
- Date: 2026-10-02

## Context

Only an agent works on the code, and the owner verifies results only now and
then. The test suite is the main protection against regressions. It must run
offline and must not need third-party services. The goal is that real pages
render correctly, so tests must cover real pages, not only small cases.

## Decision

Four levels of tests.

### 1. Unit tests

In each crate, next to the code (`#[cfg(test)]` modules). Every parser,
algorithm and data structure has unit tests.

### 2. Layout tests against stored Chromium geometry

Small HTML documents in `tests/layout/` (one feature per file). A tool
(`tools/`, Python + Playwright) loads each document in Chromium and stores
the border-box rectangle of every element in a JSON file next to it. The Rust
test loads the same document in swb and compares rectangles with a tolerance.

Chromium runs only when a developer regenerates the expected files. The test
itself needs only the stored JSON, so it runs offline and fast.

### 3. Page fixtures

Each target page has a fixture in `fixtures/pages/<name>/`:

- `manifest.json`: maps each URL to a response file, status and content type.
- `files/`: the response bodies.
- `reference/`: Chromium output for the fixture: element geometry (JSON) and a
  screenshot of the first viewport.

The `net` crate has a replay mode that answers all requests from a manifest.
A URL that is not in the manifest fails with a `NotInFixture` network error,
which is logged, so missing resources are easy to find. The page keeps its
original URLs, so base URLs and links behave as on the live site. The exact
manifest format is documented in the `swb_net::fixture` module.

Chromium loads the same fixture through Playwright request interception, so
both browsers see the same bytes. Chromium runs with JavaScript disabled,
because swb does not run JavaScript yet. The comparison is then fair.

Fixture capture combines two sources: the requests that Chromium makes, and
the requests that swb makes (swb can request more, for example lazy-loaded
images). Both are recorded into the same manifest.

### 4. Scores and ratchet

A comparison tool computes, per fixture:

- the fraction of elements whose border box matches Chromium within a
  tolerance;
- a pixel-difference score between the two first-viewport screenshots.

The scores are stored in `fixtures/scores.json`. A Rust test computes the
geometry score for each fixture and fails if it is lower than the stored
value. When a change improves a score, the stored value is raised in the same
commit. Scores only go up unless a commit explains why.

### Automation API tests

Integration tests start swb in headless mode with the remote-control server
and drive it through the client library, on fixtures only.

Update (2026-10-04): the automation tests also load `data:` URLs and local
files that they write (form submission), and fill the cookie jar in the
test process. None of them uses the network.

## Consequences

- Fixtures add binary files to the repository. Keep them small: only the
  target pages and their direct resources.
- Element geometry is compared in document order. This requires both browsers
  to build the same DOM. Both use spec-compliant HTML parsers and no
  JavaScript runs, so this holds in practice.
- Fonts and anti-aliasing differ between browsers, so pixel scores never
  reach 100%. Geometry scores are the main metric. Pixel scores catch paint
  errors (missing backgrounds, wrong colours, missing images).
