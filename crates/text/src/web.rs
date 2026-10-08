//! Web fonts: the faces of a document's `@font-face` rules.
//!
//! The engine gives the faces to [`FontContext::set_web_fonts`] whenever
//! the document's rules change. Families of web faces shadow system
//! families of the same name (CSS Fonts 4 §4.2); generic families do not
//! resolve to web faces (measured in Chromium 148).
//!
//! A face loads only when text needs it (CSS Fonts 4 §5.2 and §4.8.1):
//! itemization requests a face when it reaches the face in the font
//! matching order and the face's `unicode-range` contains the character;
//! the first available font requests the face that covers U+0020 if no
//! other face of its composite font is loaded or requested (measured in
//! Chromium 148, see [`FontContext::take_web_font_requests`]). The engine
//! takes the requests after layout, fetches the sources, and reports the
//! result with [`FontContext::web_font_loaded`] or
//! [`FontContext::web_font_failed`]. Until then text uses the next
//! families (as `font-display: swap` with an infinite swap period).
//!
//! Within a family, CSS Fonts 4 §5.2 matching selects a face by the
//! `font-stretch`, `font-style` and `font-weight` descriptors; all faces
//! with the same descriptors form a composite font, checked per character
//! in reverse rule order (§4.5.1). Faces whose load failed are not part
//! of the family; a family whose faces all failed is missing, and no
//! system font of that name is used instead.
//!
//! Variation axes and synthesis for web faces: see [`web_instance`].

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use crate::FontId;
use crate::context::{Face, FaceState, FontContext, Synthesis, Variations};
use crate::error::TextError;
use crate::face::{FaceDesc, LoadedFace, MetricOverrides, normalize_ranges, ranges_contain};
use crate::matching::{self, Candidate, Desired, FaceStyle};
use crate::query::{FamilyName, FontQuery, FontStyle};
use crate::shape::Feature;

/// The most faces of one web family that font matching considers; later
/// faces are ignored.
pub const MAX_FACES_PER_FAMILY: usize = 1_000;

/// The most faces of one composite font that itemization checks for a
/// character (the last ones in rule order). It bounds the work per
/// character on hostile pages (ADR 0022). Real fonts have about 120
/// `unicode-range` slices per face (Google Fonts CJK families).
pub(crate) const MAX_COMPOSITE_CHECKS: usize = 256;

/// The most web font faces that a document loads (or tries to load);
/// later requests fail at once.
pub const MAX_WEB_FONT_LOADS: usize = 1_000;

/// The `font-style` descriptor of a web font face.
#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub enum WebFontStyle {
    /// `auto`: no constraint.
    #[default]
    Auto,
    /// `normal`.
    Normal,
    /// `italic`.
    Italic,
    /// `oblique` with an inclusive angle range in degrees.
    Oblique(f32, f32),
}

/// A face of an `@font-face` rule, as the font matching needs it.
#[derive(Clone, Debug, PartialEq)]
pub struct WebFontFace {
    /// The family name.
    pub family: String,
    /// The `font-weight` descriptor range, `None` for `auto`.
    pub weight: Option<(f32, f32)>,
    /// The `font-stretch` descriptor range in percent, `None` for `auto`.
    pub stretch: Option<(f32, f32)>,
    /// The `font-style` descriptor.
    pub style: WebFontStyle,
    /// The `unicode-range` descriptor: inclusive code point ranges.
    pub unicode_range: Vec<(u32, u32)>,
    /// The sources, in priority order. The text crate does not read them;
    /// the engine gets them back with [`FontContext::web_font_sources`].
    pub sources: Vec<String>,
    /// The `size-adjust` descriptor as a ratio (CSS Fonts 4 §4.9): it
    /// scales the font size for shaping, metrics and rasterization of the
    /// face. 1 is the initial value.
    pub size_adjust: f32,
    /// The `ascent-override` descriptor as a ratio of the adjusted font
    /// size (measured in Chromium 148), `None` for `normal`.
    pub ascent_override: Option<f32>,
    /// The `descent-override` descriptor, like `ascent_override`.
    pub descent_override: Option<f32>,
    /// The `line-gap-override` descriptor, like `ascent_override`.
    pub line_gap_override: Option<f32>,
    /// The `font-variation-settings` descriptor: (axis tag, value). They
    /// apply after the axes that the matching sets and before the
    /// property of the same name.
    pub variations: Vec<([u8; 4], f32)>,
    /// The `font-feature-settings` descriptor. They come before the
    /// features of the property.
    pub features: Vec<Feature>,
}

impl Default for WebFontFace {
    /// A face without family or sources, with the initial value of every
    /// descriptor.
    fn default() -> Self {
        WebFontFace {
            family: String::new(),
            weight: None,
            stretch: None,
            style: WebFontStyle::Auto,
            unicode_range: vec![(0, 0x10_FFFF)],
            sources: Vec::new(),
            size_adjust: 1.0,
            ascent_override: None,
            descent_override: None,
            line_gap_override: None,
            variations: Vec::new(),
            features: Vec::new(),
        }
    }
}

/// A web font face, stable across [`FontContext::set_web_fonts`] calls
/// for the same face.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct WebFaceId(u32);

/// The loading state of a web face.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum WebState {
    Unloaded,
    Requested,
    /// Loaded into `FontContext::faces` at this index.
    Loaded(usize),
    Failed,
}

#[derive(Debug)]
struct WebFace {
    id: WebFaceId,
    desc: WebFontFace,
    /// The normalized `unicode-range`.
    range: Arc<[(u32, u32)]>,
    state: WebState,
}

/// A composite font: the faces of a family that the matching selected for
/// one set of font properties, in the order to check them (reverse rule
/// order). Indices into `WebFonts::faces`.
#[derive(Debug)]
struct Composite {
    faces: Vec<usize>,
}

/// The web font state of a [`FontContext`].
#[derive(Debug, Default)]
pub(crate) struct WebFonts {
    /// The current faces, in rule order.
    faces: Vec<WebFace>,
    /// Lowercase family name to the faces of the family, in rule order.
    families: HashMap<String, Arc<[usize]>>,
    next_id: u32,
    /// Faces requested since the last `take_web_font_requests`.
    requests: Vec<WebFaceId>,
    /// Faces that the first available font wants: (face, composite).
    wishes: Vec<(usize, usize)>,
    /// The number of face loads that started in the document. It never
    /// decreases, so failed faces and media-environment changes do not
    /// reset the limit; [`FontContext::reset_web_fonts`] resets it.
    loads: usize,
    /// Composite fonts by family and font properties; cleared whenever a
    /// face changes state.
    composites: Vec<Composite>,
    composite_ids: HashMap<(String, CompositeKey), usize>,
}

/// The font properties that select a composite font.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
struct CompositeKey {
    weight: u32,
    stretch: u32,
    style: FontStyle,
}

/// A font of a query's family list, or a web font face that is not loaded
/// yet (an index into the web faces).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum FamilySlot {
    Font(FontId),
    Pending(usize),
}

impl WebFonts {
    fn index_of(&self, id: WebFaceId) -> Option<usize> {
        self.faces.iter().position(|f| f.id == id)
    }

    /// Drops the composite fonts (and the wishes that refer to them).
    fn clear_composites(&mut self) {
        self.composites.clear();
        self.composite_ids.clear();
        self.wishes.clear();
    }
}

impl FontContext {
    /// Replaces the web font faces with `faces` (the document's
    /// `@font-face` rules, in order). A face equal to a current one keeps
    /// its id and its loading state; faces that are no longer present are
    /// unloaded.
    pub fn set_web_fonts(&mut self, faces: Vec<WebFontFace>) {
        let mut old: HashMap<usize, Vec<usize>> = HashMap::new();
        let old_faces = std::mem::take(&mut self.web.faces);
        for (i, face) in old_faces.iter().enumerate() {
            old.entry(desc_hash(&face.desc)).or_default().push(i);
        }
        let mut reused = vec![false; old_faces.len()];
        let mut new_faces = Vec::with_capacity(faces.len());
        for desc in faces {
            let previous = old.get(&desc_hash(&desc)).and_then(|candidates| {
                candidates
                    .iter()
                    .copied()
                    .find(|&i| !reused[i] && old_faces[i].desc == desc)
            });
            let face = if let Some(i) = previous {
                reused[i] = true;
                let old = &old_faces[i];
                WebFace {
                    id: old.id,
                    desc,
                    range: old.range.clone(),
                    state: old.state,
                }
            } else {
                let id = WebFaceId(self.web.next_id);
                self.web.next_id = self.web.next_id.wrapping_add(1);
                let range = Arc::from(normalize_ranges(desc.unicode_range.clone()));
                WebFace {
                    id,
                    desc,
                    range,
                    state: WebState::Unloaded,
                }
            };
            new_faces.push(face);
        }
        for (i, face) in old_faces.iter().enumerate() {
            if !reused[i]
                && let WebState::Loaded(index) = face.state
            {
                self.unload_face(index);
            }
        }
        let mut families: HashMap<String, Vec<usize>> = HashMap::new();
        let mut truncated = false;
        for (i, face) in new_faces.iter().enumerate() {
            let members = families
                .entry(face.desc.family.trim().to_ascii_lowercase())
                .or_default();
            if members.len() < MAX_FACES_PER_FAMILY {
                members.push(i);
            } else {
                truncated = true;
            }
        }
        if truncated {
            log::warn!(
                "a web font family has more than {MAX_FACES_PER_FAMILY} faces; ignoring the rest"
            );
        }
        let kept: Vec<WebFaceId> = new_faces.iter().map(|f| f.id).collect();
        self.web.requests.retain(|id| kept.contains(id));
        self.web.faces = new_faces;
        self.web.families = families
            .into_iter()
            .map(|(name, members)| (name, Arc::from(members)))
            .collect();
        self.web.wishes.clear();
        self.web.clear_composites();
    }

    /// Removes all web font faces and resets the load count: for a new
    /// document.
    pub fn reset_web_fonts(&mut self) {
        self.set_web_fonts(Vec::new());
        self.web.loads = 0;
    }

    /// The faces that text needs and that are not loaded yet, since the
    /// last call. Each face is returned once; it stays requested until
    /// [`FontContext::web_font_loaded`] or [`FontContext::web_font_failed`].
    ///
    /// Besides the faces that itemization requested, this includes the
    /// face of each first available font that covers U+0020, unless
    /// another face of the same composite font is loaded or requested:
    /// measured in Chromium 148, a line box (its strut) loads that face,
    /// but text that needs only other faces of the composite does not.
    pub fn take_web_font_requests(&mut self) -> Vec<WebFaceId> {
        let wishes = std::mem::take(&mut self.web.wishes);
        for (face, composite) in wishes {
            let busy = self.web.composites.get(composite).is_some_and(|c| {
                c.faces.iter().any(|&f| {
                    matches!(
                        self.web.faces.get(f).map(|f| f.state),
                        Some(WebState::Requested | WebState::Loaded(_))
                    )
                })
            });
            if !busy {
                self.request_web_face(face);
            }
        }
        std::mem::take(&mut self.web.requests)
    }

    /// The sources of a web face, if it is still part of the document.
    pub fn web_font_sources(&self, id: WebFaceId) -> Option<&[String]> {
        let index = self.web.index_of(id)?;
        Some(&self.web.faces[index].desc.sources)
    }

    /// Uses `data` (decoded OpenType data, see
    /// [`decode_web_font`](crate::decode_web_font)) for a requested face.
    /// `label` names the source in errors and debug output (the URL). On
    /// an error the face stays requested, so that the caller can try its
    /// next source; it does nothing if the face is no longer requested.
    pub fn web_font_loaded(
        &mut self,
        id: WebFaceId,
        label: &str,
        data: &Arc<[u8]>,
    ) -> Result<(), TextError> {
        self.load_web_face(id, PathBuf::from(label), data, 0)
    }

    /// Uses the installed font named `name` for a requested face: the
    /// `local()` source of `src`. A font matches if its full name or its
    /// PostScript name equals `name` without regard to ASCII case and
    /// spaces (measured in Chromium 148 with the test fonts: `Liberation
    /// Sans`, `Liberation Sans Bold`, `LiberationSans-Bold` and
    /// `liberationsans` match; `Liberation Sans Regular`, `Arial` and
    /// `Times New Roman` do not, as family names and aliases do not
    /// count). The font loads at once; on an error the face stays
    /// requested, so that the caller can try its next source.
    pub fn web_font_local(&mut self, id: WebFaceId, name: &str) -> Result<(), TextError> {
        let Some(desc) = self.source.local_face(name) else {
            return Err(TextError::NoLocalFont(name.to_owned()));
        };
        let data = match &desc.data {
            Some(data) => data.clone(),
            None => crate::face::read_font_file(&desc.path)?,
        };
        self.load_web_face(id, desc.path, &data, desc.index)
    }

    /// Parses face `font_index` of `data` as the font of a requested web
    /// face.
    fn load_web_face(
        &mut self,
        id: WebFaceId,
        path: PathBuf,
        data: &Arc<[u8]>,
        font_index: u32,
    ) -> Result<(), TextError> {
        let Some(index) = self.web.index_of(id) else {
            return Ok(());
        };
        if self.web.faces[index].state != WebState::Requested {
            return Ok(());
        }
        let mut loaded = LoadedFace::new(&path, data, font_index)?;
        let web = &self.web.faces[index];
        loaded.set_unicode_range(web.range.clone());
        apply_descriptors(&mut loaded, &web.desc);
        let desc = FaceDesc {
            path,
            index: font_index,
            family: web.desc.family.clone(),
            style: face_style(web.desc.style),
            weight: web.desc.weight,
            stretch: web.desc.stretch.map_or(100.0, |s| s.0),
            data: None,
        };
        let face = self.push_face(Face {
            desc,
            state: FaceState::Loaded(Arc::new(loaded)),
        });
        self.web.faces[index].state = WebState::Loaded(face);
        self.web.clear_composites();
        Ok(())
    }

    /// Marks a requested face as failed: none of its sources loaded. It
    /// is no longer part of its family.
    pub fn web_font_failed(&mut self, id: WebFaceId) {
        if let Some(index) = self.web.index_of(id)
            && self.web.faces[index].state == WebState::Requested
        {
            self.web.faces[index].state = WebState::Failed;
            self.web.clear_composites();
        }
    }

    /// Marks all requested faces as failed (loading was stopped).
    pub fn fail_web_font_requests(&mut self) {
        let mut changed = false;
        for face in &mut self.web.faces {
            if face.state == WebState::Requested {
                face.state = WebState::Failed;
                changed = true;
            }
        }
        self.web.requests.clear();
        if changed {
            self.web.clear_composites();
        }
    }

    /// The faces of the web family `family`, if a web family of that name
    /// exists (generic families never resolve to web fonts).
    pub(crate) fn web_family(&self, family: FamilyName<'_>) -> Option<Arc<[usize]>> {
        let FamilyName::Named(name) = family else {
            return None;
        };
        if self.web.families.is_empty() {
            return None;
        }
        self.web
            .families
            .get(&name.trim().to_ascii_lowercase())
            .cloned()
    }

    /// The composite font of a web family for `query`: CSS Fonts 4 §5.2
    /// over the faces that did not fail, then all faces with the same
    /// descriptors as the selected one, in reverse rule order. `None` if
    /// all faces failed.
    fn web_composite(
        &mut self,
        name: &str,
        faces: &[usize],
        query: &FontQuery<'_>,
    ) -> Option<usize> {
        let desired = Desired {
            weight: query.clamped_weight(),
            stretch: query.clamped_stretch(),
            style: query.style,
        };
        let key = (
            name.trim().to_ascii_lowercase(),
            CompositeKey {
                weight: desired.weight.to_bits(),
                stretch: desired.stretch.to_bits(),
                style: desired.style,
            },
        );
        if let Some(&id) = self.web.composite_ids.get(&key) {
            return Some(id);
        }
        let usable: Vec<usize> = faces
            .iter()
            .copied()
            .filter(|&f| self.web.faces[f].state != WebState::Failed)
            .collect();
        let candidates: Vec<Candidate> = usable
            .iter()
            .map(|&f| candidate(&self.web.faces[f].desc))
            .collect();
        let best = &self.web.faces[usable[matching::best_match(&candidates, desired)?]].desc;
        let mut members: Vec<usize> = usable
            .iter()
            .copied()
            .filter(|&f| same_descriptors(&self.web.faces[f].desc, best))
            .collect();
        members.reverse();
        let id = self.web.composites.len();
        self.web.composites.push(Composite { faces: members });
        self.web.composite_ids.insert(key, id);
        Some(id)
    }

    /// The fonts (and pending faces) of the families of `query`, in
    /// matching order. A web family contributes the faces of its
    /// composite font; a system family its selected font.
    pub(crate) fn family_slots(&mut self, query: &FontQuery<'_>) -> Vec<FamilySlot> {
        let mut slots: Vec<FamilySlot> = Vec::with_capacity(query.families.len());
        for &family in query.families {
            match (family, self.web_family(family)) {
                (FamilyName::Named(name), Some(faces)) => {
                    self.push_web_slots(name, &faces, query, &mut slots);
                }
                _ => {
                    if let Some(font) = self.family_font(family, query) {
                        slots.push(FamilySlot::Font(font));
                    }
                }
            }
        }
        // Keep each font once (a family can be listed twice).
        let mut seen = HashSet::with_capacity(slots.len());
        slots.retain(|slot| seen.insert(*slot));
        slots
    }

    /// Adds the faces of the composite font of a web family to `slots`.
    fn push_web_slots(
        &mut self,
        name: &str,
        faces: &[usize],
        query: &FontQuery<'_>,
        slots: &mut Vec<FamilySlot>,
    ) {
        let Some(composite) = self.web_composite(name, faces, query) else {
            return;
        };
        let members = self.web.composites[composite].faces.clone();
        for face in members.into_iter().take(MAX_COMPOSITE_CHECKS) {
            match self.web.faces[face].state {
                WebState::Loaded(index) => {
                    let font = self.web_instance(face, index, query);
                    slots.push(FamilySlot::Font(font));
                }
                WebState::Unloaded | WebState::Requested => {
                    slots.push(FamilySlot::Pending(face));
                }
                WebState::Failed => {}
            }
        }
    }

    /// The first available font of a web family for `query`: the face of
    /// its composite font that covers U+0020, if loaded; otherwise the
    /// first loaded face of the composite (measured in Chromium 148: the
    /// line height then comes from that face). `None` if the family has
    /// no face for U+0020 or none is loaded; then the face for U+0020 is
    /// wanted (see [`FontContext::take_web_font_requests`]).
    pub(crate) fn web_primary(
        &mut self,
        name: &str,
        faces: &[usize],
        query: &FontQuery<'_>,
    ) -> Option<FontId> {
        let composite = self.web_composite(name, faces, query)?;
        let members = self.web.composites[composite].faces.clone();
        let space = members
            .iter()
            .copied()
            .find(|&f| ranges_contain(&self.web.faces[f].range, 0x20))?;
        let loaded = |f: usize, web: &WebFonts| match web.faces[f].state {
            WebState::Loaded(index) => Some(index),
            _ => None,
        };
        let chosen = std::iter::once(space)
            .chain(members.iter().copied())
            .find_map(|f| loaded(f, &self.web).map(|index| (f, index)));
        if let Some((face, index)) = chosen {
            return Some(self.web_instance(face, index, query));
        }
        if self.web.faces[space].state == WebState::Unloaded
            && !self.web.wishes.contains(&(space, composite))
        {
            self.web.wishes.push((space, composite));
        }
        None
    }

    /// Requests a pending face if its `unicode-range` contains `c`.
    pub(crate) fn request_web_face_for(&mut self, face: usize, c: char) {
        if self
            .web
            .faces
            .get(face)
            .is_some_and(|f| ranges_contain(&f.range, u32::from(c)))
        {
            self.request_web_face(face);
        }
    }

    fn request_web_face(&mut self, face: usize) {
        let Some(web) = self.web.faces.get_mut(face) else {
            return;
        };
        if web.state != WebState::Unloaded {
            return;
        }
        if self.web.loads >= MAX_WEB_FONT_LOADS {
            if self.web.loads == MAX_WEB_FONT_LOADS {
                log::warn!("more than {MAX_WEB_FONT_LOADS} web font faces; not loading more");
                self.web.loads += 1;
            }
            web.state = WebState::Failed;
            self.web.clear_composites();
            return;
        }
        web.state = WebState::Requested;
        self.web.loads += 1;
        self.web.requests.push(web.id);
    }

    /// The font for a loaded web face (`face` in the web faces, `index` in
    /// the context's faces) and the properties of `query`.
    ///
    /// Variation axes (CSS Fonts 4 §7.2): `wght` from `font-weight`, `wdth`
    /// from `font-stretch`, clamped to the descriptor range (not for
    /// `auto`), then to the axis; `ital` 1 for `italic` if the descriptor
    /// allows italic, else `slnt` from an oblique angle; never both.
    /// Measured in Chromium 148: a face with `font-weight: 400` on a
    /// variable font renders with `wght` 400 at every weight, also when
    /// the default instance is lighter.
    ///
    /// Synthesis, measured in Chromium 148: bold if the weight is at least
    /// 600, the descriptor weight (unless `auto`) is below 600 and the
    /// weight of the font (the `wght` value, else OS/2 `usWeightClass`) is
    /// below 600; oblique if the style is italic or oblique and neither
    /// the descriptor nor the font data is italic or oblique and no slant
    /// axis is set.
    fn web_instance(&mut self, face: usize, index: usize, query: &FontQuery<'_>) -> FontId {
        let desc = &self.web.faces[face].desc;
        let FaceState::Loaded(loaded) = &self.faces[index].state else {
            return self.placeholder_font();
        };
        let (mut variations, synthesis) = web_instance(desc, loaded, query);
        variations.custom = self.intern_variation_set(query.variations);
        self.intern_face(index, synthesis, variations)
    }
}

/// The variations and synthesis of a web face for `query`: see
/// [`FontContext::web_instance`].
fn web_instance(
    desc: &WebFontFace,
    loaded: &LoadedFace,
    query: &FontQuery<'_>,
) -> (Variations, Synthesis) {
    let weight = query.clamped_weight();
    let clamp = |value: f32, range: Option<(f32, f32)>| {
        range.map_or(value, |(min, max)| value.clamp(min, max))
    };
    let wght = loaded
        .wght
        .map(|axis| clamp(weight, desc.weight).clamp(axis.min, axis.max));
    let wdth = loaded
        .wdth
        .map(|axis| clamp(query.clamped_stretch(), desc.stretch).clamp(axis.min, axis.max));
    let slanted = query.style != FontStyle::Normal;
    let mut ital = None;
    let mut slnt = None;
    if slanted {
        match (desc.style, query.style) {
            (WebFontStyle::Auto | WebFontStyle::Italic, FontStyle::Italic)
                if loaded.ital.is_some() =>
            {
                ital = loaded.ital.map(|axis| 1f32.clamp(axis.min, axis.max));
            }
            (WebFontStyle::Auto | WebFontStyle::Oblique(..), _) => {
                // The slant axis goes the other way: positive angles lean
                // right, negative `slnt` values lean right.
                let angle = clamp(OBLIQUE_ANGLE, oblique_range(desc.style));
                slnt = loaded.slnt.map(|axis| (-angle).clamp(axis.min, axis.max));
            }
            _ => {}
        }
    }
    let instance_weight = wght.unwrap_or(loaded.weight);
    let synthesis = Synthesis {
        bold: weight >= 600.0
            && desc.weight.is_none_or(|(_, max)| max < 600.0)
            && instance_weight < 600.0,
        oblique: slanted
            && matches!(desc.style, WebFontStyle::Auto | WebFontStyle::Normal)
            && !loaded.slanted
            && ital.is_none_or(|v| v == 0.0)
            && slnt.is_none_or(|v| v == 0.0),
    };
    let variations = Variations {
        wght: wght.map(f32::to_bits),
        wdth: wdth.map(f32::to_bits),
        slnt: slnt.map(f32::to_bits),
        ital: ital.map(f32::to_bits),
        custom: 0,
    };
    (variations, synthesis)
}

/// Gives a loaded face the metric, variation and feature descriptors of
/// its `@font-face` rule. Non-finite values are ignored.
fn apply_descriptors(loaded: &mut LoadedFace, desc: &WebFontFace) {
    let ratio = |value: f32| value.is_finite().then_some(value.max(0.0));
    loaded.size_adjust = ratio(desc.size_adjust).unwrap_or(1.0);
    loaded.metric_overrides = MetricOverrides {
        ascent: desc.ascent_override.and_then(ratio),
        descent: desc.descent_override.and_then(ratio),
        line_gap: desc.line_gap_override.and_then(ratio),
    };
    loaded.variation_settings = desc
        .variations
        .iter()
        .copied()
        .filter(|(_, value)| value.is_finite())
        .collect();
    loaded.features = desc.features.iter().copied().collect();
}

/// The angle of `font-style: italic` and `oblique` for a `slnt` axis:
/// the default oblique angle (CSS Fonts 4 §2.4). swb does not keep the
/// angle of `oblique <angle>` yet.
const OBLIQUE_ANGLE: f32 = 14.0;

fn oblique_range(style: WebFontStyle) -> Option<(f32, f32)> {
    match style {
        WebFontStyle::Oblique(min, max) => Some((min, max)),
        _ => None,
    }
}

/// The matching candidate of a face: `auto` descriptors match as
/// `normal` (CSS Fonts 4 §4.4).
fn candidate(desc: &WebFontFace) -> Candidate {
    Candidate {
        weight: desc.weight.unwrap_or((400.0, 400.0)),
        stretch: desc.stretch.unwrap_or((100.0, 100.0)),
        style: face_style(desc.style),
    }
}

fn face_style(style: WebFontStyle) -> FaceStyle {
    match style {
        WebFontStyle::Auto | WebFontStyle::Normal => FaceStyle::Normal,
        WebFontStyle::Italic => FaceStyle::Italic,
        WebFontStyle::Oblique(..) => FaceStyle::Oblique,
    }
}

/// True if two faces have the same `font-weight`, `font-stretch` and
/// `font-style` descriptors: they belong to the same composite font.
fn same_descriptors(a: &WebFontFace, b: &WebFontFace) -> bool {
    a.weight == b.weight && a.stretch == b.stretch && a.style == b.style
}

/// A hash of the descriptors of a face, to find equal faces quickly.
fn desc_hash(desc: &WebFontFace) -> usize {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    desc.family.hash(&mut h);
    desc.sources.hash(&mut h);
    desc.unicode_range.hash(&mut h);
    h.finish() as usize
}
