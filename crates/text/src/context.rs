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
enum FaceState {
    Unloaded,
    Loaded(Arc<LoadedFace>),
    Failed,
}

struct Face {
    desc: FaceDesc,
    state: FaceState,
}

struct Family {
    faces: Vec<usize>,
}

/// What a [`FontId`] stands for. `face` is `None` for the placeholder font
/// that is used when no fonts exist at all.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
struct InstanceKey {
    face: Option<usize>,
    synthesis: Synthesis,
    /// The `wght` variation value as `f32` bits.
    wght: Option<u32>,
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
    faces: Vec<Face>,
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
    /// workspace.
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
    pub fn select(&mut self, query: &FontQuery<'_>) -> FontId {
        for family in query.families {
            if let Some(family) = self.resolve_family(*family)
                && let Some(font) = self.select_in_family(family, query)
            {
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

    /// True if `font` has a glyph for `c`.
    pub fn has_glyph(&self, font: FontId, c: char) -> bool {
        self.instance(font)
            .and_then(|i| i.face.as_ref())
            .is_some_and(|face| face.covers(c))
    }

    /// The anti-aliased coverage mask of a glyph at `size` device pixels,
    /// shifted right by `subpixel` quarter pixels (0 to 3). Returns `None`
    /// for glyphs without an outline (spaces), color glyphs (not supported
    /// yet), glyphs wider or taller than 4096 pixels and invalid arguments.
    ///
    /// Masks of up to 256 × 256 pixels are cached, with at most 64 MiB of
    /// mask data in total. Larger masks are rasterized on every call.
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

    /// Fonts of the families of `query` that exist, in order, without
    /// duplicates.
    pub(crate) fn family_fonts(&mut self, query: &FontQuery<'_>) -> Vec<FontId> {
        let mut fonts = Vec::with_capacity(query.families.len());
        for family in query.families {
            if let Some(family) = self.resolve_family(*family)
                && let Some(font) = self.select_in_family(family, query)
                && !fonts.contains(&font)
            {
                fonts.push(font);
            }
        }
        fonts
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
        self.intern(InstanceKey {
            face: None,
            synthesis: Synthesis::default(),
            wght: None,
        })
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
            wght,
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
                wght,
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
        if let (Some(face), Some(wght)) = (&instance.face, key.wght)
            && let Some(font) = face.font()
        {
            let location = font
                .axes()
                .location([(Tag::new(b"wght"), f32::from_bits(wght))]);
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

/// Font sizes must be finite and positive; others become 0.
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
