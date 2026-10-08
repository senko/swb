//! System fonts through fontconfig, with the same family acceptance and
//! character fallback rules as Chromium on Linux.
//!
//! Chromium resolves fonts in Skia's `SkFontConfigInterfaceDirect` and in
//! `ui/gfx/font_fallback_linux.cc`:
//!
//! - A named family: fontconfig matches the family. Skia accepts the match
//!   only if its family is the requested family, the first family of the
//!   pattern after configuration substitution (a strong alias such as a
//!   user's "Arial means Helvetica" rule), or a metric-compatible
//!   replacement from a fixed table (Arial and Liberation Sans, ...).
//!   Otherwise the next CSS family is tried. Blink then retries a few
//!   alternate names (Arial and Helvetica, Times and Times New Roman,
//!   Courier and Courier New).
//! - A generic family: the family that Chrome's default font settings
//!   give it (for example Times New Roman for `serif`), if that family
//!   exists by the rules above; otherwise fontconfig's match, which is
//!   always accepted.
//! - Character fallback: `FcFontSort` for the content language, then the
//!   first face in that order whose character set contains the character.
//!
//! Approximations, because the `fontconfig` crate does not expose
//! everything:
//!
//! - Only the first family name of a match is read directly. Other family
//!   names of the match are checked by listing the requested family and
//!   looking for the matched file.
//! - The pattern does not ask for scalable fonts (`FC_SCALABLE` is a bool and
//!   the crate cannot add bools). Matches that are not TrueType or CFF are
//!   rejected instead.
//! - Character coverage is read from the font's own `cmap` table, not from
//!   fontconfig's character set.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};

use fontconfig::{
    FC_FAMILY, FC_FILE, FC_FONTFORMAT, FC_INDEX, FC_LANG, FC_SLANT, FC_SLANT_ITALIC,
    FC_SLANT_OBLIQUE, FC_SLANT_ROMAN, FC_WEIGHT, FC_WEIGHT_REGULAR, FC_WIDTH, FC_WIDTH_NORMAL,
    FontFormat, Fontconfig, ObjectSet, Pattern, UnicodeCoverage,
};

use crate::face::{self, FaceDesc, FamilyDesc};
use crate::matching::FaceStyle;
use crate::query::GenericFamily;
use crate::source::FontSource;

/// fontconfig property for the font file wrapper (fontconfig 2.14 and
/// later). Skia asks for "SFNT" to prefer TrueType and OpenType files.
const FC_FONT_WRAPPER: &CStr = c"fontwrapper";

/// fontconfig properties for the full name and the PostScript name.
const FC_FULL_NAME: &CStr = c"fullname";
const FC_POSTSCRIPT: &CStr = c"postscriptname";

/// Metric-compatible families. A match from the same class as the
/// requested family is accepted.
///
/// The classes are derived from Skia's `SkFontConfigInterface_direct.cpp`
/// (`GetFontEquivClass`; Copyright Google Inc., BSD-3-Clause); see
/// `THIRD_PARTY_NOTICES.md`.
const METRIC_COMPATIBLE: &[&[&str]] = &[
    &["Arial", "Arimo", "Liberation Sans"],
    &["Times New Roman", "Tinos", "Liberation Serif"],
    &["Courier New", "Cousine", "Liberation Mono"],
    &["Symbol", "Symbol Neu"],
    &[
        "MS PGothic",
        "Noto Sans CJK JP",
        "IPAPGothic",
        "MotoyaG04Gothic",
    ],
    &[
        "MS Gothic",
        "Noto Sans Mono CJK JP",
        "IPAGothic",
        "MotoyaG04GothicMono",
    ],
    &[
        "MS PMincho",
        "Noto Serif CJK JP",
        "IPAPMincho",
        "MotoyaG04Mincho",
    ],
    &[
        "MS Mincho",
        "Noto Serif CJK JP",
        "IPAMincho",
        "MotoyaG04MinchoMono",
    ],
    &["Simsun", "Noto Serif CJK SC", "MSung GB18030", "Song ASC"],
    &[
        "NSimsun",
        "Noto Serif CJK SC",
        "MSung GB18030",
        "N Song ASC",
    ],
    &[
        "Simhei",
        "Noto Sans CJK SC",
        "MYingHeiGB18030",
        "MYingHeiB5HK",
    ],
    &["PMingLiU", "Noto Serif CJK TC", "MSung B5HK"],
    &["MingLiU", "Noto Serif CJK TC", "MSung B5HK"],
    &["PMingLiU_HKSCS", "Noto Serif CJK TC", "MSung B5HK"],
    &["MingLiU_HKSCS", "Noto Serif CJK TC", "MSung B5HK"],
    &["Cambria", "Caladea"],
    &["Calibri", "Carlito"],
];

/// Alternate names that Blink tries when a family is not found
/// (`AlternateFamilyName` in Blink).
const ALTERNATE_NAMES: &[(&str, &str)] = &[
    ("Courier", "Courier New"),
    ("Courier New", "Courier"),
    ("Times", "Times New Roman"),
    ("Times New Roman", "Times"),
    ("Arial", "Helvetica"),
    ("Helvetica", "Arial"),
];

/// fontconfig weights and the CSS weights they correspond to (the weight
/// table of Skia's `skfontstyle_from_fcpattern`).
const WEIGHTS: &[(f32, f32)] = &[
    (0.0, 100.0),
    (40.0, 200.0),
    (50.0, 300.0),
    (55.0, 350.0),
    (75.0, 380.0),
    (80.0, 400.0),
    (100.0, 500.0),
    (180.0, 600.0),
    (200.0, 700.0),
    (205.0, 800.0),
    (210.0, 900.0),
    (215.0, 1000.0),
];

/// Converts a fontconfig weight to a CSS weight by linear interpolation.
fn css_weight(fc_weight: f32) -> f32 {
    let mut previous = WEIGHTS[0];
    for &(fc, css) in WEIGHTS {
        if fc_weight <= fc {
            if fc <= previous.0 {
                return css;
            }
            let t = (fc_weight - previous.0) / (fc - previous.0);
            return previous.1 + t * (css - previous.1);
        }
        previous = (fc, css);
    }
    previous.1
}

fn metric_compatible(a: &str, b: &str) -> bool {
    let class = |name: &str| {
        METRIC_COMPATIBLE
            .iter()
            .position(|class| class.iter().any(|n| n.eq_ignore_ascii_case(name)))
    };
    class(a).is_some_and(|ca| class(b) == Some(ca))
}

/// The system fonts.
pub(crate) struct FontconfigSource {
    fc: Fontconfig,
    /// Fallback candidates per language, in fontconfig's sort order.
    fallback_lists: HashMap<String, Vec<FaceDesc>>,
    /// `cmap` tables of fallback candidates. `None` if unreadable.
    cmaps: HashMap<(PathBuf, u32), Option<Vec<u8>>>,
    /// The faces with their normalized full and PostScript names, for
    /// `local()`. Read on first use.
    local_faces: Option<Vec<(Vec<String>, FaceDesc)>>,
}

impl FontconfigSource {
    /// Initializes fontconfig. Returns `None` if the library is missing or
    /// fails to initialize.
    pub(crate) fn new() -> Option<Self> {
        Some(FontconfigSource {
            fc: Fontconfig::new()?,
            fallback_lists: HashMap::new(),
            cmaps: HashMap::new(),
            local_faces: None,
        })
    }

    /// A pattern for `family` with regular style, as Skia builds it.
    fn family_pattern(&self, family: &CStr) -> Option<Pattern<'_>> {
        let mut pattern = Pattern::new(&self.fc).ok()?;
        pattern.add_string(FC_FAMILY, family).ok()?;
        pattern.add_integer(FC_WEIGHT, FC_WEIGHT_REGULAR).ok()?;
        pattern.add_integer(FC_SLANT, FC_SLANT_ROMAN).ok()?;
        pattern.add_integer(FC_WIDTH, FC_WIDTH_NORMAL).ok()?;
        // Older fontconfig versions do not know this property; then it has
        // no effect.
        let _ = pattern.add_string(FC_FONT_WRAPPER, c"SFNT");
        Some(pattern)
    }

    /// Resolves `name` to the family name to list, following Skia's
    /// acceptance rules. `accept_any` is true for generic families.
    fn resolve(&self, name: &str, accept_any: bool) -> Option<String> {
        let cname = CString::new(name).ok()?;
        let post_config = {
            let mut pattern = self.family_pattern(&cname)?;
            pattern.config_substitute().ok()?;
            pattern.default_substitute();
            pattern.get_string(FC_FAMILY).unwrap_or_default().to_owned()
        };
        let mut pattern = self.family_pattern(&cname)?;
        let matched = pattern.font_match().ok()?;
        let face = face_from_pattern(&matched)?;
        let match_family = matched.get_string(FC_FAMILY).ok()?.to_owned();
        if accept_any
            || match_family.eq_ignore_ascii_case(name)
            || match_family.eq_ignore_ascii_case(&post_config)
            || metric_compatible(name, &match_family)
        {
            return Some(match_family);
        }
        // The match can have more than one family name; only the first one
        // is readable. Look for the matched file under the other names.
        [name, post_config.as_str()]
            .into_iter()
            .filter(|n| !n.is_empty())
            .find(|n| {
                self.list_family(n).is_some_and(|family| {
                    family
                        .faces
                        .iter()
                        .any(|f| f.path == face.path && f.index == face.index)
                })
            })
            .map(str::to_owned)
    }

    /// All usable faces that have `name` as one of their family names.
    fn list_family(&self, name: &str) -> Option<FamilyDesc> {
        let cname = CString::new(name).ok()?;
        let mut pattern = Pattern::new(&self.fc).ok()?;
        pattern.add_string(FC_FAMILY, &cname).ok()?;
        let mut objects = ObjectSet::new(&self.fc).ok()?;
        for object in [
            FC_FAMILY,
            FC_FILE,
            FC_INDEX,
            FC_WEIGHT,
            FC_SLANT,
            FC_WIDTH,
            FC_FONTFORMAT,
        ] {
            objects.add(object).ok()?;
        }
        let set = fontconfig::list_fonts(&pattern, Some(&objects)).ok()?;
        let mut faces: Vec<FaceDesc> = set
            .iter()
            .filter_map(|p| face_from_pattern(&p))
            .map(|mut face| {
                name.clone_into(&mut face.family);
                face
            })
            .collect();
        faces.sort_by(|a, b| a.path.cmp(&b.path).then(a.index.cmp(&b.index)));
        faces.dedup_by(|a, b| a.path == b.path && a.index == b.index);
        if faces.is_empty() {
            return None;
        }
        Some(FamilyDesc {
            name: name.to_owned(),
            faces,
        })
    }

    /// All usable faces with their normalized full and PostScript names,
    /// from the fontconfig font list, in its order.
    fn list_local_faces(&self) -> Vec<(Vec<String>, FaceDesc)> {
        let Ok(pattern) = Pattern::new(&self.fc) else {
            return Vec::new();
        };
        let Ok(mut objects) = ObjectSet::new(&self.fc) else {
            return Vec::new();
        };
        for object in [
            FC_FAMILY,
            FC_FILE,
            FC_INDEX,
            FC_WEIGHT,
            FC_SLANT,
            FC_WIDTH,
            FC_FONTFORMAT,
            FC_FULL_NAME,
            FC_POSTSCRIPT,
        ] {
            if objects.add(object).is_err() {
                return Vec::new();
            }
        }
        let Ok(set) = fontconfig::list_fonts(&pattern, Some(&objects)) else {
            return Vec::new();
        };
        set.iter()
            .filter_map(|p| {
                let face = face_from_pattern(&p)?;
                let names: Vec<String> = [FC_FULL_NAME, FC_POSTSCRIPT]
                    .into_iter()
                    .filter_map(|name| p.get_string(name).ok())
                    .map(face::normalize_local_name)
                    .filter(|name| !name.is_empty())
                    .collect();
                Some((names, face))
            })
            .collect()
    }

    /// Fallback candidates for a language: all usable fonts in the order of
    /// `FcFontSort` (Chromium's `CachedFontSet::CreateForLocale`).
    fn fallback_list(&self, language: &str) -> Vec<FaceDesc> {
        let Ok(mut pattern) = Pattern::new(&self.fc) else {
            return Vec::new();
        };
        if let Ok(lang) = CString::new(language)
            && !language.is_empty()
        {
            let _ = pattern.add_string(FC_LANG, &lang);
        }
        let Ok(set) = pattern.sort_fonts(UnicodeCoverage::NoTrim) else {
            return Vec::new();
        };
        let mut faces: Vec<FaceDesc> = Vec::new();
        for face in set.iter().filter_map(|p| face_from_pattern(&p)) {
            if !faces
                .iter()
                .any(|f| f.path == face.path && f.index == face.index)
            {
                faces.push(face);
            }
        }
        faces
    }
}

/// The family that Chrome's default font settings on Linux give a generic
/// family (`chrome/app/resources/locale_settings_linux.grd`). Checked with
/// Chromium 148: serif is Times New Roman, sans-serif Arial, cursive Comic
/// Sans MS, fantasy Impact; monospace and system-ui come from fontconfig.
fn chrome_default_family(generic: GenericFamily) -> Option<&'static str> {
    match generic {
        GenericFamily::Serif => Some("Times New Roman"),
        GenericFamily::SansSerif => Some("Arial"),
        GenericFamily::Cursive => Some("Comic Sans MS"),
        GenericFamily::Fantasy => Some("Impact"),
        _ => None,
    }
}

impl FontSource for FontconfigSource {
    fn local_face(&mut self, name: &str) -> Option<FaceDesc> {
        let name = face::normalize_local_name(name);
        if self.local_faces.is_none() {
            self.local_faces = Some(self.list_local_faces());
        }
        self.local_faces
            .iter()
            .flatten()
            .find(|(names, _)| names.contains(&name))
            .map(|(_, face)| face.clone())
    }

    fn named_family(&mut self, name: &str) -> Option<FamilyDesc> {
        let resolved = self.resolve(name, false).or_else(|| {
            ALTERNATE_NAMES
                .iter()
                .find(|(from, _)| from.eq_ignore_ascii_case(name))
                .and_then(|(_, to)| self.resolve(to, false))
        })?;
        self.list_family(&resolved)
    }

    fn generic_family(&mut self, generic: GenericFamily) -> Option<FamilyDesc> {
        // Chrome's default font settings name a family for some generic
        // families; fontconfig decides only if that family is missing.
        if let Some(name) = chrome_default_family(generic)
            && let Some(family) = self.named_family(name)
        {
            return Some(family);
        }
        let resolved = self.resolve(generic.css_name(), true)?;
        self.list_family(&resolved)
    }

    fn default_family(&mut self) -> Option<FamilyDesc> {
        // When no family of a query exists, Blink uses its standard font,
        // which is the serif font. Its last resort is fontconfig's "Sans".
        self.generic_family(GenericFamily::Serif).or_else(|| {
            let resolved = self.resolve("sans", true)?;
            self.list_family(&resolved)
        })
    }

    /// Candidate order: the `FcFontSort` order for `language`.
    fn fallback_face(&mut self, c: char, language: &str, from: usize) -> Option<(usize, FaceDesc)> {
        if !self.fallback_lists.contains_key(language) {
            let list = self.fallback_list(language);
            self.fallback_lists.insert(language.to_owned(), list);
        }
        let list = self.fallback_lists.get(language)?;
        let cmaps = &mut self.cmaps;
        list.iter()
            .enumerate()
            .skip(from)
            .find(|(_, face)| {
                cmaps
                    .entry((face.path.clone(), face.index))
                    .or_insert_with(|| face::read_cmap_table(&face.path, face.index))
                    .as_deref()
                    .is_some_and(|table| face::cmap_covers(table, c))
            })
            .map(|(position, face)| (position, face.clone()))
    }
}

/// Converts a fontconfig pattern into a face description. Returns `None`
/// for formats other than TrueType and CFF, and for named instances of
/// variable fonts (the variable face itself covers them).
fn face_from_pattern(pattern: &Pattern<'_>) -> Option<FaceDesc> {
    if !matches!(pattern.format(), Ok(FontFormat::TrueType | FontFormat::CFF)) {
        return None;
    }
    let path = Path::new(pattern.filename().ok()?).to_owned();
    let index = u32::try_from(pattern.face_index().unwrap_or(0)).ok()?;
    if index >> 16 != 0 {
        return None;
    }
    let style = match pattern.slant() {
        Ok(FC_SLANT_ITALIC) => FaceStyle::Italic,
        Ok(FC_SLANT_OBLIQUE) => FaceStyle::Oblique,
        _ => FaceStyle::Normal,
    };
    Some(FaceDesc {
        path,
        index,
        family: pattern.get_string(FC_FAMILY).unwrap_or_default().to_owned(),
        style,
        // Variable fonts store a range, which `weight()` cannot read.
        weight: pattern.weight().ok().map(|w| {
            let weight = css_weight(w as f32);
            (weight, weight)
        }),
        stretch: pattern.width().map_or(100.0, |w| w as f32),
        data: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weight_conversion() {
        assert_eq!(css_weight(80.0), 400.0);
        assert_eq!(css_weight(200.0), 700.0);
        assert_eq!(css_weight(0.0), 100.0);
        assert_eq!(css_weight(300.0), 1000.0);
        assert_eq!(css_weight(90.0), 450.0);
    }

    #[test]
    fn metric_compatibility() {
        assert!(metric_compatible("Arial", "Liberation Sans"));
        assert!(metric_compatible("arial", "ARIMO"));
        assert!(!metric_compatible("Arial", "Liberation Serif"));
        assert!(!metric_compatible("Helvetica", "Liberation Sans"));
    }
}
