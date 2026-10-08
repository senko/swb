//! [`FontContext`]: the font database, font selection and all caches.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use harfrust::{ShapePlan, ShaperInstance};
use skrifa::instance::NormalizedCoord;
use skrifa::{MetadataProvider, Tag};

use crate::error::TextError;
use crate::face::{self, FaceDesc, FamilyDesc, LoadedFace};
use crate::mask_cache::{MASK_CACHE_BUDGET, MaskCache, MaskKey};
use crate::matching::{self, Candidate, Desired};
use crate::metrics::{self, FontMetrics};
use crate::query::{FamilyName, FontQuery, FontStyle, GenericFamily, GenericFamilyMap};
use crate::raster::{self, GlyphMask};
use crate::source::directory::DirectorySource;
use crate::source::{EmptySource, FontSource};
use crate::web::WebFonts;
use crate::{FontId, GlyphId};

/// Language for system fallback when the query has none. Chromium uses the
/// browser locale; `en-US` is the common default.
const DEFAULT_LANGUAGE: &str = "en-us";

/// The synthetic styles applied to a font.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct Synthesis {
    /// The outline is emboldened when rasterized, because the family has no
    /// face that is bold enough. Advances do not change.
    pub bold: bool,
    /// The outline is slanted (horizontal shear of 0.25, about 14 degrees)
    /// when rasterized, because the family has no italic or oblique face.
    pub oblique: bool,
}

/// A description of a font, for debugging and tests.
#[derive(Clone, Debug, PartialEq)]
pub struct FontInfo<'a> {
    /// The family under which the font was found.
    pub family: &'a str,
    /// The font file.
    pub path: &'a Path,
    /// The face index in the file.
    pub index: u32,
    /// The weight of the face, or the `wght` variation value for variable
    /// fonts.
    pub weight: f32,
    /// True if the face itself is italic or oblique.
    pub italic: bool,
    /// The synthetic styles.
    pub synthesis: Synthesis,
}

/// The state of a known face.
pub(crate) enum FaceState {
    Unloaded,
    Loaded(Arc<LoadedFace>),
    Failed,
}

/// A face that the context knows: from a font source, or a loaded web
/// font face.
pub(crate) struct Face {
    pub(crate) desc: FaceDesc,
    pub(crate) state: FaceState,
}

struct Family {
    faces: Vec<usize>,
}

/// The variation axis values of a font, as `f32` bits (user units), for
/// the axes that the font has and that font matching sets.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub(crate) struct Variations {
    pub(crate) wght: Option<u32>,
    pub(crate) wdth: Option<u32>,
    pub(crate) slnt: Option<u32>,
    pub(crate) ital: Option<u32>,
}

impl Variations {
    /// The axis settings as (tag, value) pairs.
    fn settings(&self) -> Vec<(Tag, f32)> {
        [
            (b"wght", self.wght),
            (b"wdth", self.wdth),
            (b"slnt", self.slnt),
            (b"ital", self.ital),
        ]
        .into_iter()
        .filter_map(|(tag, value)| value.map(|v| (Tag::new(tag), f32::from_bits(v))))
        .collect()
    }
}

/// What a [`FontId`] stands for. `face` is `None` for the placeholder font
/// that is used when no fonts exist at all. Caches that key on the
/// [`FontId`] (shape plans, glyph masks) so key on the variation values.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
struct InstanceKey {
    face: Option<usize>,
    synthesis: Synthesis,
    variations: Variations,
}

/// A concrete font: a face with synthesis flags and variations.
pub(crate) struct Instance {
    key: InstanceKey,
    pub(crate) face: Option<Arc<LoadedFace>>,
    /// Normalized variation coordinates (empty for the default instance).
    pub(crate) coords: Vec<NormalizedCoord>,
    pub(crate) variation: Option<ShaperInstance>,
    /// Shape plans for this font, reused across `shape` calls.
    pub(crate) plans: Vec<ShapePlan>,
}

impl Instance {
    pub(crate) fn synthesis(&self) -> Synthesis {
        self.key.synthesis
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
struct SelectionKey {
    family: usize,
    weight: u32,
    style: FontStyle,
    stretch: u32,
}

/// The font database and all font caches: family resolution, selected
/// fonts, fallback results, shape plans and glyph masks.
///
/// Use one context per thread; it is `Send` but not `Sync`.
pub struct FontContext {
    source: Box<dyn FontSource>,
    pub(crate) faces: Vec<Face>,
    face_ids: HashMap<(PathBuf, u32), usize>,
    families: Vec<Family>,
    /// Lowercase family name (as the source returned it) to family index.
    family_ids: HashMap<String, usize>,
    /// Lowercase requested name to family index (`None`: does not exist).
    named: HashMap<String, Option<usize>>,
    generic: HashMap<GenericFamily, Option<usize>>,
    default_family: Option<usize>,
    default_family_resolved: bool,
    selections: HashMap<SelectionKey, FontId>,
    pub(crate) instances: Vec<Instance>,
    instance_ids: HashMap<InstanceKey, FontId>,
    /// System fallback result per language and character.
    fallback: HashMap<String, HashMap<char, Option<usize>>>,
    masks: MaskCache,
    /// Faces for which "color glyphs are not supported" was logged.
    color_warned: HashSet<usize>,
    /// The web font faces of the document.
    pub(crate) web: WebFonts,
}

impl FontContext {
    fn with_source(source: Box<dyn FontSource>) -> Self {
        FontContext {
            source,
            faces: Vec::new(),
            face_ids: HashMap::new(),
            families: Vec::new(),
            family_ids: HashMap::new(),
            named: HashMap::new(),
            generic: HashMap::new(),
            default_family: None,
            default_family_resolved: false,
            selections: HashMap::new(),
            instances: Vec::new(),
            instance_ids: HashMap::new(),
            fallback: HashMap::new(),
            masks: MaskCache::new(MASK_CACHE_BUDGET),
            color_warned: HashSet::new(),
            web: WebFonts::default(),
        }
    }

    /// The system fonts. On Linux and other Unix systems this uses
    /// fontconfig, with Chromium's rules for family names and fallback. If
    /// fontconfig is not available, the context has no fonts: text gets a
    /// placeholder font without glyphs.
    pub fn system() -> Self {
        Self::with_source(system_source())
    }

    /// Only the fonts in `dir` (`.ttf`, `.otf`, `.ttc`, `.otc`; not
    /// recursive), with an explicit family map. Does not use fontconfig, so
    /// the result does not depend on the machine.
    pub fn from_directory(dir: &Path, generics: GenericFamilyMap) -> Result<Self, TextError> {
        let source = DirectorySource::new(dir, generics)?;
        Ok(Self::with_source(Box::new(source)))
    }

    /// The bundled test fonts in `fixtures/fonts` with
    /// [`GenericFamilyMap::bundled`]. For tests in any crate of the
    /// workspace and for `swb --test-fonts`. The directory is found through
    /// the source path at build time; panics if it has no usable fonts.
    pub fn for_tests() -> Self {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/fonts");
        Self::from_directory(&dir, GenericFamilyMap::bundled())
            .expect("the bundled test fonts exist in fixtures/fonts")
    }

    /// Selects the font for `query`: the first family in the list that
    /// exists, then the best face of that family by the CSS font matching
    /// algorithm (<https://www.w3.org/TR/css-fonts-4/#font-matching-algorithm>).
    /// If no family exists, uses the default family. Never fails: without
    /// any fonts it returns a placeholder font that has no glyphs.
    ///
    /// A family of web fonts (`@font-face`, see
    /// [`FontContext::set_web_fonts`]) gives the face of its composite font
    /// that covers U+0020 (CSS Fonts 4 §5.2, "first available font"), if it
    /// is loaded.
    pub fn select(&mut self, query: &FontQuery<'_>) -> FontId {
        for &family in query.families {
            let font = match (family, self.web_family(family)) {
                (FamilyName::Named(name), Some(faces)) => self.web_primary(name, &faces, query),
                _ => self.family_font(family, query),
            };
            if let Some(font) = font {
                return font;
            }
        }
        self.default_font(query)
    }

    /// Font metrics in pixels for a font size of `size` pixels.
    pub fn metrics(&self, font: FontId, size: f32) -> FontMetrics {
        let size = sanitize_size(size);
        match self
            .instance(font)
            .and_then(|i| i.face.as_ref().map(|f| (i, f)))
        {
            Some((instance, face)) => metrics::compute(face, &instance.coords, size),
            None => FontMetrics::placeholder(size),
        }
    }

    /// The synthetic styles of a font.
    pub fn synthesis(&self, font: FontId) -> Synthesis {
        self.instance(font)
            .map(Instance::synthesis)
            .unwrap_or_default()
    }

    /// A description of a font. `None` for the placeholder font.
    pub fn font_info(&self, font: FontId) -> Option<FontInfo<'_>> {
        let instance = self.instance(font)?;
        let face = &self.faces[instance.key.face?];
        let weight = instance
            .key
            .variations
            .wght
            .map_or_else(|| face.desc.weight.map_or(400.0, |w| w.0), f32::from_bits);
        Some(FontInfo {
            family: &face.desc.family,
            path: &face.desc.path,
            index: face.desc.index,
            weight,
            italic: face.desc.style.is_slanted(),
            synthesis: instance.key.synthesis,
        })
    }

    /// True if the `GSUB` table of `font` has the OpenType feature `tag`
    /// (for any script).
    pub fn has_feature(&self, font: FontId, tag: [u8; 4]) -> bool {
        use skrifa::raw::TableProvider;
        let Some(font) = self
            .instance(font)
            .and_then(|i| i.face.as_ref())
            .and_then(|face| face.font())
        else {
            return false;
        };
        font.gsub()
            .ok()
            .and_then(|gsub| gsub.feature_list().ok())
            .is_some_and(|list| {
                list.feature_records()
                    .iter()
                    .any(|r| r.feature_tag() == Tag::new(&tag))
            })
    }

    /// True if `font` has a glyph for `c`.
    pub(crate) fn has_glyph(&self, font: FontId, c: char) -> bool {
        self.instance(font)
            .and_then(|i| i.face.as_ref())
            .is_some_and(|face| face.covers(c))
    }

    /// The anti-aliased coverage mask of a glyph at `size` device pixels,
    /// shifted right by `subpixel` quarter pixels (0 to 3; larger values
    /// count as 3). Returns `None` for glyphs without an outline (spaces),
    /// color glyphs (not supported yet), glyphs wider or taller than 4096
    /// pixels and invalid arguments.
    ///
    /// Masks of up to 65,536 pixels (256 × 256) are cached, with at most
    /// 64 MiB of mask data and 32K masks in total. Larger masks are
    /// rasterized on every call.
    pub fn glyph_mask(
        &mut self,
        font: FontId,
        glyph: GlyphId,
        size: f32,
        subpixel: u8,
    ) -> Option<Arc<GlyphMask>> {
        let size = sanitize_size(size);
        let key = MaskKey {
            font,
            glyph,
            size: size.to_bits(),
            subpixel: subpixel.min(3),
        };
        if let Some(mask) = self.masks.get(&key) {
            return mask.clone();
        }
        let mask = self.rasterize(key).map(Arc::new);
        self.masks.insert(key, mask.clone());
        mask
    }

    fn rasterize(&mut self, key: MaskKey) -> Option<GlyphMask> {
        let instance = self.instances.get(key.font.0 as usize)?;
        let face = instance.face.as_ref()?;
        let synthesis = instance.key.synthesis;
        let result = raster::rasterize(
            face,
            &instance.coords,
            key.glyph,
            f32::from_bits(key.size),
            key.subpixel,
            synthesis,
        );
        if let Ok(mask) = result {
            return mask;
        }
        let face_index = instance.key.face?;
        if self.color_warned.insert(face_index) {
            log::warn!(
                "color glyphs are not supported yet: {}",
                self.faces[face_index].desc.path.display()
            );
        }
        None
    }

    pub(crate) fn instance(&self, font: FontId) -> Option<&Instance> {
        self.instances.get(font.0 as usize)
    }

    /// The font of the system family `family` for `query`, if the family
    /// exists.
    pub(crate) fn family_font(
        &mut self,
        family: FamilyName<'_>,
        query: &FontQuery<'_>,
    ) -> Option<FontId> {
        let family = self.resolve_family(family)?;
        self.select_in_family(family, query)
    }

    /// The font of the default family for `query`, or the placeholder.
    pub(crate) fn default_font(&mut self, query: &FontQuery<'_>) -> FontId {
        if !self.default_family_resolved {
            self.default_family = self.source.default_family().map(|f| self.add_family(f));
            self.default_family_resolved = true;
        }
        if let Some(family) = self.default_family
            && let Some(font) = self.select_in_family(family, query)
        {
            return font;
        }
        self.placeholder_font()
    }

    /// The placeholder font: no glyphs, made-up metrics.
    pub(crate) fn placeholder_font(&mut self) -> FontId {
        self.intern(InstanceKey {
            face: None,
            synthesis: Synthesis::default(),
            variations: Variations::default(),
        })
    }

    /// The font for face `face` with `synthesis` and `variations`.
    pub(crate) fn intern_face(
        &mut self,
        face: usize,
        synthesis: Synthesis,
        variations: Variations,
    ) -> FontId {
        self.intern(InstanceKey {
            face: Some(face),
            synthesis,
            variations,
        })
    }

    /// Adds a face that no font source knows (a web font face) and returns
    /// its index.
    pub(crate) fn push_face(&mut self, face: Face) -> usize {
        self.faces.push(face);
        self.faces.len() - 1
    }

    /// Drops the data of a face that is no longer used (a web font face
    /// of a previous document). Its fonts become placeholder fonts, so
    /// that their [`FontId`]s stay valid.
    pub(crate) fn unload_face(&mut self, face: usize) {
        if let Some(entry) = self.faces.get_mut(face) {
            entry.state = FaceState::Failed;
            entry.desc.data = None;
        }
        for instance in &mut self.instances {
            if instance.key.face == Some(face) {
                instance.face = None;
                instance.coords.clear();
                instance.variation = None;
                instance.plans.clear();
            }
        }
    }

    /// A font for `c` from system fallback, with Chromium's synthesis rules
    /// for fallback fonts. `language` comes from [`fallback_language`].
    pub(crate) fn fallback_font(
        &mut self,
        c: char,
        language: &str,
        query: &FontQuery<'_>,
    ) -> Option<FontId> {
        let cached = self.fallback.get(language).and_then(|m| m.get(&c));
        let face = if let Some(face) = cached {
            *face
        } else {
            let face = self.find_fallback_face(c, language);
            self.fallback
                .entry(language.to_owned())
                .or_default()
                .insert(c, face);
            face
        }?;
        let desc = &self.faces[face].desc;
        let face_style = desc.style;
        let static_weight = desc.weight.map_or(400.0, |w| w.0);
        let (weight, wght) = self.instance_weight(face, query.clamped_weight(), static_weight);
        let synthesis = Synthesis {
            bold: matching::synthesize_bold_fallback(query.clamped_weight(), weight),
            oblique: matching::synthesize_oblique(query.style, face_style),
        };
        Some(self.intern(InstanceKey {
            face: Some(face),
            synthesis,
            variations: Variations {
                wght,
                ..Variations::default()
            },
        }))
    }

    /// Asks the source for fallback faces for `c`, in its order, until one
    /// loads and has a glyph for `c`. The source and the loaded face can
    /// disagree about coverage, and a face file can fail to parse; then the
    /// search continues with the next candidate.
    fn find_fallback_face(&mut self, c: char, language: &str) -> Option<usize> {
        let mut from = 0;
        while let Some((position, desc)) = self.source.fallback_face(c, language, from) {
            let face = self.add_face(desc);
            if self.load(face).is_some_and(|f| f.covers(c)) {
                return Some(face);
            }
            // `max` guarantees progress even if a source ignores `from`.
            from = position.max(from) + 1;
        }
        None
    }

    fn resolve_family(&mut self, family: FamilyName<'_>) -> Option<usize> {
        match family {
            FamilyName::Named(name) => {
                let name = name.trim();
                let key = name.to_ascii_lowercase();
                if let Some(result) = self.named.get(&key) {
                    return *result;
                }
                let result = self.source.named_family(name).map(|f| self.add_family(f));
                self.named.insert(key, result);
                result
            }
            FamilyName::Generic(generic) => {
                if let Some(result) = self.generic.get(&generic) {
                    return *result;
                }
                let result = self
                    .source
                    .generic_family(generic)
                    .map(|f| self.add_family(f));
                self.generic.insert(generic, result);
                result
            }
        }
    }

    fn add_family(&mut self, desc: FamilyDesc) -> usize {
        let key = desc.name.to_ascii_lowercase();
        if let Some(&family) = self.family_ids.get(&key) {
            return family;
        }
        let faces = desc.faces.into_iter().map(|f| self.add_face(f)).collect();
        self.families.push(Family { faces });
        let family = self.families.len() - 1;
        self.family_ids.insert(key, family);
        family
    }

    fn add_face(&mut self, desc: FaceDesc) -> usize {
        let key = (desc.path.clone(), desc.index);
        if let Some(&face) = self.face_ids.get(&key) {
            return face;
        }
        self.faces.push(Face {
            desc,
            state: FaceState::Unloaded,
        });
        let face = self.faces.len() - 1;
        self.face_ids.insert(key, face);
        face
    }

    /// Parses a face on first use. Returns `None` if it cannot be loaded.
    fn load(&mut self, face: usize) -> Option<Arc<LoadedFace>> {
        let entry = &mut self.faces[face];
        match &entry.state {
            FaceState::Loaded(loaded) => return Some(loaded.clone()),
            FaceState::Failed => return None,
            FaceState::Unloaded => {}
        }
        let desc = &entry.desc;
        let result = match &desc.data {
            Some(data) => Ok(data.clone()),
            None => face::read_font_file(&desc.path),
        }
        .and_then(|data| LoadedFace::new(&desc.path, &data, desc.index));
        // The parsed face keeps its own reference to the data.
        entry.desc.data = None;
        match result {
            Ok(loaded) => {
                let loaded = Arc::new(loaded);
                entry.state = FaceState::Loaded(loaded.clone());
                Some(loaded)
            }
            Err(e) => {
                log::warn!("{e}");
                entry.state = FaceState::Failed;
                None
            }
        }
    }

    /// The weight range of a face for matching. Loads the face if the source
    /// did not know the weight. `None` if the face cannot be used.
    fn weight_range(&mut self, face: usize) -> Option<(f32, f32)> {
        if let FaceState::Loaded(loaded) = &self.faces[face].state
            && let Some(axis) = loaded.wght
        {
            return Some((axis.min, axis.max));
        }
        if matches!(self.faces[face].state, FaceState::Failed) {
            return None;
        }
        if let Some(range) = self.faces[face].desc.weight {
            return Some(range);
        }
        let loaded = self.load(face)?;
        Some(
            loaded
                .wght
                .map_or((loaded.weight, loaded.weight), |a| (a.min, a.max)),
        )
    }

    /// Selects a face of `family` by CSS matching and returns its font.
    fn select_in_family(&mut self, family: usize, query: &FontQuery<'_>) -> Option<FontId> {
        let desired = Desired {
            weight: query.clamped_weight(),
            stretch: query.clamped_stretch(),
            style: query.style,
        };
        let key = SelectionKey {
            family,
            weight: desired.weight.to_bits(),
            style: desired.style,
            stretch: desired.stretch.to_bits(),
        };
        if let Some(font) = self.selections.get(&key) {
            return Some(*font);
        }
        // Faces that fail to load drop out and matching runs again.
        loop {
            let faces = self.families[family].faces.clone();
            let mut usable = Vec::with_capacity(faces.len());
            let mut candidates = Vec::with_capacity(faces.len());
            for face in faces {
                if let Some(weight) = self.weight_range(face) {
                    let desc = &self.faces[face].desc;
                    usable.push(face);
                    candidates.push(Candidate {
                        weight,
                        stretch: (desc.stretch, desc.stretch),
                        style: desc.style,
                    });
                }
            }
            let face = usable[matching::best_match(&candidates, desired)?];
            let Some(loaded) = self.load(face) else {
                continue;
            };
            let static_weight = self.faces[face].desc.weight.map_or(loaded.weight, |w| w.0);
            let (weight, wght) = self.instance_weight(face, desired.weight, static_weight);
            let synthesis = Synthesis {
                bold: matching::synthesize_bold(desired.weight, weight),
                oblique: matching::synthesize_oblique(desired.style, self.faces[face].desc.style),
            };
            let font = self.intern(InstanceKey {
                face: Some(face),
                synthesis,
                variations: Variations {
                    wght,
                    ..Variations::default()
                },
            });
            self.selections.insert(key, font);
            return Some(font);
        }
    }

    /// The effective weight of a face for a desired weight, and the `wght`
    /// variation value if the face is variable.
    fn instance_weight(&self, face: usize, desired: f32, static_weight: f32) -> (f32, Option<u32>) {
        match &self.faces[face].state {
            FaceState::Loaded(loaded) => match loaded.wght {
                Some(axis) => {
                    let weight = desired.clamp(axis.min, axis.max);
                    (weight, Some(weight.to_bits()))
                }
                None => (static_weight, None),
            },
            _ => (static_weight, None),
        }
    }

    /// Returns the font for `key`, creating it on first use.
    fn intern(&mut self, key: InstanceKey) -> FontId {
        if let Some(&font) = self.instance_ids.get(&key) {
            return font;
        }
        let face = key.face.and_then(|f| match &self.faces[f].state {
            FaceState::Loaded(loaded) => Some(loaded.clone()),
            _ => None,
        });
        let mut instance = Instance {
            key,
            face,
            coords: Vec::new(),
            variation: None,
            plans: Vec::new(),
        };
        let settings = key.variations.settings();
        if let Some(face) = &instance.face
            && !settings.is_empty()
            && let Some(font) = face.font()
        {
            let location = font.axes().location(settings);
            instance.coords = location.coords().to_vec();
            instance.variation = Some(ShaperInstance::from_coords(
                &font,
                instance.coords.iter().copied(),
            ));
        }
        let font = FontId(self.instances.len() as u32);
        self.instances.push(instance);
        self.instance_ids.insert(key, font);
        font
    }
}

fn system_source() -> Box<dyn FontSource> {
    #[cfg(all(unix, not(any(target_vendor = "apple", target_os = "android"))))]
    {
        if let Some(source) = crate::source::fontconfig::FontconfigSource::new() {
            return Box::new(source);
        }
        log::warn!("fontconfig is not available; no system fonts");
    }
    Box::new(EmptySource)
}

/// Font sizes must be finite and positive; others become 0. Sizes above
/// 16384 px become 16384 px.
pub(crate) fn sanitize_size(size: f32) -> f32 {
    if size.is_finite() && size > 0.0 {
        size.min(16384.0)
    } else {
        0.0
    }
}

/// The language for system fallback: the query's language in lowercase with
/// `-` as separator, as fontconfig expects, or the default.
pub(crate) fn fallback_language(query: &FontQuery<'_>) -> String {
    query.language.map_or_else(
        || DEFAULT_LANGUAGE.to_owned(),
        |language| language.trim().replace('_', "-").to_ascii_lowercase(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<FontContext>();
    }

    #[test]
    fn language_normalization() {
        let mut query = FontQuery::new(&[]);
        assert_eq!(fallback_language(&query), "en-us");
        query.language = Some("en_US");
        assert_eq!(fallback_language(&query), "en-us");
        query.language = Some(" sr-Latn ");
        assert_eq!(fallback_language(&query), "sr-latn");
    }

    #[test]
    fn empty_context_returns_placeholder() {
        let mut ctx = FontContext::with_source(Box::new(EmptySource));
        let families = [FamilyName::Generic(GenericFamily::SansSerif)];
        let font = ctx.select(&FontQuery::new(&families));
        assert!(ctx.font_info(font).is_none());
        let metrics = ctx.metrics(font, 10.0);
        assert!(metrics.ascent > 0.0);
        assert!(ctx.glyph_mask(font, GlyphId(1), 10.0, 0).is_none());
    }
}
