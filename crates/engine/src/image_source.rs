//! Image source selection for `<img>` elements: `srcset` with density
//! (`x`) and width (`w`) descriptors, `sizes`, and the `<source>` elements
//! of a `<picture>` parent.
//!
//! - Selecting an image source:
//!   <https://html.spec.whatwg.org/multipage/images.html#selecting-an-image-source>
//! - The source set of an image (the `<source>` elements of a `<picture>`
//!   parent, then the image's own attributes):
//!   <https://html.spec.whatwg.org/multipage/images.html#update-the-source-set>
//! - Parsing `srcset`:
//!   <https://html.spec.whatwg.org/multipage/images.html#parse-a-srcset-attribute>
//! - The `sizes` attribute: [`swb_css::source_size`].
//!
//! The specification lets the user agent choose among the candidates. swb
//! chooses as Chromium 148 does (measured): the candidates are sorted by
//! pixel density (a `w` descriptor gives width / source size, no
//! descriptor gives 1); of equal densities the first one stays; the first
//! candidate whose density is at least the device pixel ratio wins, or
//! else the densest one. The `src` attribute is a candidate with density 1,
//! unless a candidate has a `w` descriptor. The density of the chosen
//! candidate divides the natural size of the image (density-corrected
//! natural size).
//!
//! A `<source>` before the image takes part if its `srcset` has a valid
//! candidate, its `media` matches, and its `type` (without parameters) is
//! empty or a type that swb decodes. The first such source wins; if it has
//! a `width` or `height` attribute, both of its dimension attributes
//! replace the image's own (the "dimension attribute source").
//!
//! The engine selects the sources of a document when its subresources
//! start to load, and again at the next style computation after a change
//! of the viewport or the scale ("react to changes in the environment",
//! <https://html.spec.whatwg.org/multipage/images.html#img-environment-changes>):
//! an image keeps its current source until the new one has loaded
//! (`crate::resources::Images::reselect`). Deviation: the specification
//! ignores a change while a load is pending; swb replaces the pending
//! selection, so that a change and its reversal end at the right image.
//!
//! Deviations: Chromium also takes a denser candidate if it is already in
//! its memory cache; swb does not. `sizes="auto"` is not supported (see
//! [`swb_css::source_size`]), and neither is the user-agent rule that gives
//! such images `contain: size` with an intrinsic size of 300x150.

use std::collections::HashMap;

use swb_css::{MediaEnvironment, MediaQueryList, source_size};
use swb_dom::{Document, ElementData, NodeId, is_html_whitespace, local_name};
use swb_net::Url;

/// The image chosen for an `<img>` element.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SelectedImage {
    /// The URL to load.
    pub(crate) url: Url,
    /// The pixel density of the chosen candidate. The natural dimensions
    /// of the image are divided by it.
    pub(crate) density: f32,
}

/// The result of image source selection for one `<img>` element.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Selection {
    /// The image to load; `None` if there is no candidate or its URL is
    /// invalid (the image is then broken).
    pub(crate) image: Option<SelectedImage>,
    /// The `<source>` element whose `width` and `height` attributes apply
    /// to the image instead of its own.
    pub(crate) dimension_source: Option<NodeId>,
}

/// Selects the image source of every HTML `<img>` element of `doc`, in
/// tree order. Relative URLs resolve against `base`; `env` gives the
/// viewport and the device pixel ratio.
///
/// The work is linear in the size of the document: the `<source>`
/// children of each `<picture>` are examined at most once, also when the
/// picture has many images.
pub(crate) fn select_images(
    doc: &Document,
    base: &Url,
    env: &MediaEnvironment,
) -> Vec<(NodeId, Selection)> {
    let mut pictures: HashMap<NodeId, PictureScan<'_>> = HashMap::new();
    let mut out = Vec::new();
    for node in doc.descendants(NodeId::DOCUMENT) {
        if !doc.is_html_element(node, &local_name!("img")) {
            continue;
        }
        let from_picture = doc
            .parent(node)
            .filter(|&p| doc.is_html_element(p, &local_name!("picture")))
            .and_then(|picture| {
                pictures
                    .entry(picture)
                    .or_insert_with(|| PictureScan::new(doc, picture))
                    .source_before(doc, node, env)
            });
        let selection = match from_picture {
            Some((source, chosen)) => Selection {
                image: resolve(base, chosen),
                dimension_source: doc
                    .element(source)
                    .is_some_and(|s| s.has_attr("width") || s.has_attr("height"))
                    .then_some(source),
            },
            None => Selection {
                image: own_source(doc, node, env).and_then(|chosen| resolve(base, chosen)),
                dimension_source: None,
            },
        };
        out.push((node, selection));
    }
    out
}

/// The progress through the children of a `<picture>`. Images are visited
/// in tree order, so the scan only moves forward.
struct PictureScan<'a> {
    /// The next child to examine.
    next: Option<NodeId>,
    /// The first `<source>` that matched, and its candidate. It comes
    /// before every image visited after it was found.
    found: Option<(NodeId, Chosen<'a>)>,
}

impl<'a> PictureScan<'a> {
    fn new(doc: &Document, picture: NodeId) -> Self {
        PictureScan {
            next: doc.first_element_child(picture),
            found: None,
        }
    }

    /// The first matching `<source>` before the child `img`, and its
    /// candidate.
    fn source_before(
        &mut self,
        doc: &'a Document,
        img: NodeId,
        env: &MediaEnvironment,
    ) -> Option<(NodeId, Chosen<'a>)> {
        while self.found.is_none() {
            let child = self.next?;
            self.next = doc.next_sibling_element(child);
            if child == img {
                return None;
            }
            self.found = source_candidate(doc, child, env).map(|chosen| (child, chosen));
        }
        self.found
    }
}

/// The candidate of the `<source>` element `node`, if it takes part.
/// The checks are in Chromium's order; the result does not depend on it.
fn source_candidate<'a>(
    doc: &'a Document,
    node: NodeId,
    env: &MediaEnvironment,
) -> Option<Chosen<'a>> {
    if !doc.is_html_element(node, &local_name!("source")) {
        return None;
    }
    let source = doc.element(node)?;
    let srcset = source.attr("srcset").filter(|s| !s.is_empty())?;
    if source.attr("type").is_some_and(|t| !is_supported_type(t)) {
        return None;
    }
    if source
        .attr("media")
        .is_some_and(|m| !MediaQueryList::parse_str(m).matches(env))
    {
        return None;
    }
    choose_from_srcset(source, srcset, None, env)
}

/// The candidate from the image's own `srcset` and `src` attributes.
fn own_source<'a>(doc: &'a Document, img: NodeId, env: &MediaEnvironment) -> Option<Chosen<'a>> {
    let e = doc.element(img)?;
    // URLs are stripped of ASCII white space only, as in Chromium.
    let src = e
        .attr("src")
        .map(|s| s.trim_matches(is_html_whitespace))
        .filter(|s| !s.is_empty());
    match e.attr("srcset") {
        None => src.map(|url| Chosen { url, density: 1.0 }),
        Some(srcset) => choose_from_srcset(e, srcset, src, env),
    }
}

/// The candidate from `srcset` (and `src`) of `element`, with the source
/// size from its `sizes` attribute.
fn choose_from_srcset<'a>(
    element: &ElementData,
    srcset: &'a str,
    src: Option<&'a str>,
    env: &MediaEnvironment,
) -> Option<Chosen<'a>> {
    choose(
        &parse_srcset(srcset),
        src,
        || source_size(element.attr("sizes"), env),
        env.device_pixel_ratio,
    )
}

/// True if the `type` attribute of a `<source>` is empty or names a type
/// that swb decodes. As in Chromium, parameters and surrounding white
/// space are ignored.
fn is_supported_type(value: &str) -> bool {
    let essence = value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim_matches(is_html_whitespace);
    essence.is_empty() || swb_paint::is_supported_image_type(essence)
}

fn resolve(base: &Url, chosen: Chosen<'_>) -> Option<SelectedImage> {
    match base.join(chosen.url) {
        Ok(url) => Some(SelectedImage {
            url,
            density: chosen.density,
        }),
        Err(e) => {
            log::warn!("image URL {:?}: {e}", chosen.url);
            None
        }
    }
}

/// A candidate URL with its pixel density.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Chosen<'a> {
    url: &'a str,
    density: f32,
}

/// Chooses among `candidates` (and `src`, a candidate of density 1 unless
/// a candidate has a `w` descriptor) for the device pixel ratio `dpr`.
/// `source_size` is called only if a candidate has a `w` descriptor.
fn choose<'a>(
    candidates: &[Candidate<'a>],
    src: Option<&'a str>,
    source_size: impl FnOnce() -> f32,
    dpr: f32,
) -> Option<Chosen<'a>> {
    let has_width = candidates
        .iter()
        .any(|c| matches!(c.descriptor, Descriptor::Width(_)));
    let size = if has_width { source_size() } else { 0.0 };
    let mut list: Vec<Chosen<'a>> = candidates
        .iter()
        .map(|c| Chosen {
            url: c.url,
            density: match c.descriptor {
                Descriptor::None => 1.0,
                Descriptor::Density(d) => d,
                // A width is at least 1, so this is never NaN; a source
                // size of 0 gives an infinite density.
                Descriptor::Width(w) => w as f32 / size,
            },
        })
        .collect();
    if let Some(url) = src
        && !has_width
    {
        list.push(Chosen { url, density: 1.0 });
    }
    // A stable sort: of equal densities, the first candidate stays.
    list.sort_by(|a, b| a.density.total_cmp(&b.density));
    list.dedup_by(|later, earlier| later.density == earlier.density);
    list.iter()
        .find(|c| c.density >= dpr)
        .or(list.last())
        .copied()
}

/// The descriptor of an image candidate string.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Descriptor {
    /// No descriptor: density 1.
    None,
    /// A pixel density (`2x`), at least 0.
    Density(f32),
    /// A width in image pixels (`400w`), at least 1.
    Width(u32),
}

/// An image candidate string of a `srcset` attribute.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Candidate<'a> {
    url: &'a str,
    descriptor: Descriptor,
}

/// Parses a `srcset` attribute. Invalid candidates are dropped.
///
/// <https://html.spec.whatwg.org/multipage/images.html#parse-a-srcset-attribute>
fn parse_srcset(input: &str) -> Vec<Candidate<'_>> {
    // All positions where the input is split are at ASCII characters, so
    // slices are always at character boundaries.
    let bytes = input.as_bytes();
    let mut candidates = Vec::new();
    let mut pos = 0;
    loop {
        // Splitting loop.
        while bytes
            .get(pos)
            .is_some_and(|&b| b.is_ascii_whitespace() || b == b',')
        {
            pos += 1;
        }
        if pos >= bytes.len() {
            return candidates;
        }
        let start = pos;
        while bytes.get(pos).is_some_and(|b| !b.is_ascii_whitespace()) {
            pos += 1;
        }
        let mut url = &input[start..pos];
        let mut descriptors = Vec::new();
        if url.ends_with(',') {
            // Not empty: the splitting loop skipped leading commas.
            url = url.trim_end_matches(',');
        } else {
            pos = tokenize_descriptors(input, pos, &mut descriptors);
        }
        if let Some(descriptor) = parse_descriptors(&descriptors) {
            candidates.push(Candidate { url, descriptor });
        }
    }
}

/// The descriptor tokenizer of the `srcset` parser: appends the
/// descriptors that start at `pos` to `out`. Returns the position after
/// them (after the comma that ends the candidate, if any).
fn tokenize_descriptors<'a>(input: &'a str, mut pos: usize, out: &mut Vec<&'a str>) -> usize {
    #[derive(PartialEq)]
    enum State {
        InDescriptor,
        InParens,
        AfterDescriptor,
    }
    let bytes = input.as_bytes();
    while bytes.get(pos).is_some_and(u8::is_ascii_whitespace) {
        pos += 1;
    }
    let mut start = pos;
    let mut state = State::InDescriptor;
    loop {
        let c = bytes.get(pos).copied();
        match state {
            State::InDescriptor => match c {
                Some(b) if b.is_ascii_whitespace() => {
                    if pos > start {
                        out.push(&input[start..pos]);
                    }
                    state = State::AfterDescriptor;
                }
                Some(b',') => {
                    if pos > start {
                        out.push(&input[start..pos]);
                    }
                    return pos + 1;
                }
                Some(b'(') => state = State::InParens,
                Some(_) => {}
                None => {
                    if pos > start {
                        out.push(&input[start..pos]);
                    }
                    return pos;
                }
            },
            State::InParens => match c {
                Some(b')') => state = State::InDescriptor,
                Some(_) => {}
                None => {
                    out.push(&input[start..pos]);
                    return pos;
                }
            },
            State::AfterDescriptor => match c {
                Some(b) if b.is_ascii_whitespace() => {}
                Some(_) => {
                    // Process this character again in the descriptor state.
                    state = State::InDescriptor;
                    start = pos;
                    continue;
                }
                None => return pos,
            },
        }
        pos += 1;
    }
}

/// The descriptor parser of the `srcset` parser. `None` if the candidate
/// is invalid.
fn parse_descriptors(descriptors: &[&str]) -> Option<Descriptor> {
    let mut width = None;
    let mut density = None;
    let mut height = None;
    for d in descriptors {
        if let Some(digits) = d.strip_suffix('w')
            && is_valid_non_negative_integer(digits)
        {
            if width.is_some() || density.is_some() {
                return None;
            }
            width = Some(parse_positive_integer(digits)?);
        } else if let Some(number) = d.strip_suffix('x')
            && is_valid_float(number)
        {
            if width.is_some() || density.is_some() || height.is_some() {
                return None;
            }
            let value: f32 = number.parse().ok()?;
            // Huge values are invalid, as in Chromium.
            if !(value >= 0.0 && value.is_finite()) {
                return None;
            }
            // `-0x` is 0x: a negative zero would give negative sizes.
            density = Some(value.abs());
        } else if let Some(digits) = d.strip_suffix('h')
            && is_valid_non_negative_integer(digits)
        {
            // `h` is reserved for future use; it is checked and ignored.
            if height.is_some() || density.is_some() {
                return None;
            }
            height = Some(parse_positive_integer(digits)?);
        } else {
            return None;
        }
    }
    if height.is_some() && width.is_none() {
        return None;
    }
    Some(match (width, density) {
        (Some(w), _) => Descriptor::Width(w),
        (None, Some(x)) => Descriptor::Density(x),
        (None, None) => Descriptor::None,
    })
}

/// <https://html.spec.whatwg.org/multipage/common-microsyntaxes.html#valid-non-negative-integer>
fn is_valid_non_negative_integer(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// The value of a valid non-negative integer, if it is at least 1. Values
/// that do not fit in 32 bits are invalid, as in Chromium.
fn parse_positive_integer(digits: &str) -> Option<u32> {
    digits.parse().ok().filter(|&v| v > 0)
}

/// <https://html.spec.whatwg.org/multipage/common-microsyntaxes.html#valid-floating-point-number>:
/// an optional `-`, digits with an optional fraction or only a fraction,
/// and an optional exponent.
fn is_valid_float(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = usize::from(b.first() == Some(&b'-'));
    let digits = |i: &mut usize| {
        let start = *i;
        while b.get(*i).is_some_and(u8::is_ascii_digit) {
            *i += 1;
        }
        *i > start
    };
    let integer = digits(&mut i);
    if b.get(i) == Some(&b'.') {
        i += 1;
        if !digits(&mut i) {
            return false;
        }
    } else if !integer {
        return false;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        if !digits(&mut i) {
            return false;
        }
    }
    i == b.len()
}

#[cfg(test)]
mod tests;
