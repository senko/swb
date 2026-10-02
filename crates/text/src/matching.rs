//! The CSS font matching algorithm within one family, and the rules for
//! synthetic bold and oblique.
//!
//! Spec: <https://www.w3.org/TR/css-fonts-4/#font-matching-algorithm>,
//! step 4 (font-stretch, then font-style, then font-weight).

use crate::query::FontStyle;

/// The style of a face, as the font declares it.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum FaceStyle {
    Normal,
    Italic,
    Oblique,
}

impl FaceStyle {
    /// True for italic and oblique faces.
    pub(crate) fn is_slanted(self) -> bool {
        self != FaceStyle::Normal
    }
}

/// The properties of one candidate face. Ranges are inclusive; a static face
/// has `min == max`.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Candidate {
    pub(crate) weight: (f32, f32),
    pub(crate) stretch: (f32, f32),
    pub(crate) style: FaceStyle,
}

/// The requested values.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Desired {
    pub(crate) weight: f32,
    pub(crate) stretch: f32,
    pub(crate) style: FontStyle,
}

/// Returns the index of the best candidate, or `None` if there are none.
/// Ties go to the earlier candidate.
pub(crate) fn best_match(candidates: &[Candidate], desired: Desired) -> Option<usize> {
    let mut remaining: Vec<usize> = (0..candidates.len()).collect();
    keep_best(&mut remaining, |i| {
        stretch_key(candidates[i].stretch, desired.stretch)
    });
    keep_best(&mut remaining, |i| {
        (style_rank(candidates[i].style, desired.style), 0.0)
    });
    keep_best(&mut remaining, |i| {
        weight_key(candidates[i].weight, desired.weight)
    });
    remaining.first().copied()
}

/// Keeps only the candidates with the smallest key.
fn keep_best(remaining: &mut Vec<usize>, key: impl Fn(usize) -> (u8, f32)) {
    let Some(best) = remaining
        .iter()
        .map(|&i| key(i))
        .min_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)))
    else {
        return;
    };
    remaining.retain(|&i| key(i) == best);
}

/// Distance from `value` to the range: zero inside, positive outside.
fn below_above(range: (f32, f32), value: f32) -> (f32, f32) {
    // (distance if the range is below value, distance if above)
    (value - range.1, range.0 - value)
}

/// Sort key for font-stretch: narrower first for normal or condensed
/// values, wider first for expanded values.
fn stretch_key(range: (f32, f32), desired: f32) -> (u8, f32) {
    if range.0 <= desired && desired <= range.1 {
        return (0, 0.0);
    }
    let (below, above) = below_above(range, desired);
    let range_is_below = range.1 < desired;
    match (desired <= 100.0, range_is_below) {
        (true, true) => (1, below),
        (true, false) => (2, above),
        (false, false) => (1, above),
        (false, true) => (2, below),
    }
}

/// Sort key for font-weight.
fn weight_key(range: (f32, f32), desired: f32) -> (u8, f32) {
    if range.0 <= desired && desired <= range.1 {
        return (0, 0.0);
    }
    let (below, above) = below_above(range, desired);
    let range_is_below = range.1 < desired;
    if (400.0..=500.0).contains(&desired) {
        // Weights up to 500 in ascending order, then weights below the
        // desired value in descending order, then weights above 500.
        if !range_is_below && range.0 <= 500.0 {
            (1, above)
        } else if range_is_below {
            (2, below)
        } else {
            (3, above)
        }
    } else if desired < 400.0 {
        if range_is_below {
            (1, below)
        } else {
            (2, above)
        }
    } else if range_is_below {
        (2, below)
    } else {
        (1, above)
    }
}

/// Preference order of face styles for a requested style.
fn style_rank(face: FaceStyle, desired: FontStyle) -> u8 {
    let order = match desired {
        FontStyle::Italic => [FaceStyle::Italic, FaceStyle::Oblique, FaceStyle::Normal],
        FontStyle::Oblique => [FaceStyle::Oblique, FaceStyle::Italic, FaceStyle::Normal],
        FontStyle::Normal => [FaceStyle::Normal, FaceStyle::Oblique, FaceStyle::Italic],
    };
    order.iter().position(|s| *s == face).unwrap_or(3) as u8
}

/// Synthetic bold for a face selected through the family list.
///
/// Chromium (Blink `FontCache::CreateFontPlatformData` on Linux) emboldens
/// when the requested weight exceeds the face weight by more than 200.
/// For a regular face (400) this means weights above 600, so `bold` (700)
/// is synthesized but `600` is not.
pub(crate) fn synthesize_bold(desired_weight: f32, face_weight: f32) -> bool {
    desired_weight > face_weight + 200.0
}

/// Synthetic bold for a face selected through system fallback.
///
/// Chromium (Blink `FontCache::PlatformFallbackFontForCharacter`)
/// emboldens fallback faces when the requested weight is at least 600 and
/// the face is not bold (weight below 700).
pub(crate) fn synthesize_bold_fallback(desired_weight: f32, face_weight: f32) -> bool {
    desired_weight >= 600.0 && face_weight < 700.0
}

/// Synthetic oblique: the request is italic or oblique and the face is
/// upright.
///
/// Chromium synthesizes only for `italic`; we also do it for `oblique`,
/// which follows the CSS spec.
pub(crate) fn synthesize_oblique(desired: FontStyle, face: FaceStyle) -> bool {
    desired != FontStyle::Normal && !face.is_slanted()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(weight: f32, style: FaceStyle) -> Candidate {
        Candidate {
            weight: (weight, weight),
            stretch: (100.0, 100.0),
            style,
        }
    }

    fn desired(weight: f32, style: FontStyle) -> Desired {
        Desired {
            weight,
            stretch: 100.0,
            style,
        }
    }

    #[test]
    fn exact_match_wins() {
        let faces = [
            face(400.0, FaceStyle::Normal),
            face(700.0, FaceStyle::Normal),
            face(400.0, FaceStyle::Italic),
        ];
        assert_eq!(
            best_match(&faces, desired(700.0, FontStyle::Normal)),
            Some(1)
        );
        assert_eq!(
            best_match(&faces, desired(400.0, FontStyle::Italic)),
            Some(2)
        );
    }

    #[test]
    fn weight_between_400_and_500_prefers_500_then_lighter() {
        let faces = [
            face(300.0, FaceStyle::Normal),
            face(500.0, FaceStyle::Normal),
            face(600.0, FaceStyle::Normal),
        ];
        assert_eq!(
            best_match(&faces, desired(400.0, FontStyle::Normal)),
            Some(1)
        );
        let faces = [
            face(300.0, FaceStyle::Normal),
            face(600.0, FaceStyle::Normal),
        ];
        assert_eq!(
            best_match(&faces, desired(450.0, FontStyle::Normal)),
            Some(0)
        );
    }

    #[test]
    fn light_weight_prefers_lighter_faces() {
        let faces = [
            face(100.0, FaceStyle::Normal),
            face(400.0, FaceStyle::Normal),
        ];
        assert_eq!(
            best_match(&faces, desired(300.0, FontStyle::Normal)),
            Some(0)
        );
    }

    #[test]
    fn heavy_weight_prefers_heavier_faces() {
        let faces = [
            face(400.0, FaceStyle::Normal),
            face(900.0, FaceStyle::Normal),
        ];
        assert_eq!(
            best_match(&faces, desired(600.0, FontStyle::Normal)),
            Some(1)
        );
        let faces = [
            face(400.0, FaceStyle::Normal),
            face(500.0, FaceStyle::Normal),
        ];
        assert_eq!(
            best_match(&faces, desired(700.0, FontStyle::Normal)),
            Some(1)
        );
    }

    #[test]
    fn style_is_matched_before_weight() {
        let faces = [
            face(700.0, FaceStyle::Normal),
            face(400.0, FaceStyle::Italic),
        ];
        assert_eq!(
            best_match(&faces, desired(700.0, FontStyle::Italic)),
            Some(1)
        );
        let faces = [
            face(400.0, FaceStyle::Normal),
            face(400.0, FaceStyle::Oblique),
        ];
        assert_eq!(
            best_match(&faces, desired(400.0, FontStyle::Italic)),
            Some(1)
        );
    }

    #[test]
    fn stretch_is_matched_first() {
        let condensed = Candidate {
            weight: (700.0, 700.0),
            stretch: (75.0, 75.0),
            style: FaceStyle::Normal,
        };
        let faces = [condensed, face(400.0, FaceStyle::Normal)];
        assert_eq!(
            best_match(&faces, desired(700.0, FontStyle::Normal)),
            Some(1)
        );
    }

    #[test]
    fn variable_range_contains_weight() {
        let variable = Candidate {
            weight: (100.0, 900.0),
            stretch: (100.0, 100.0),
            style: FaceStyle::Normal,
        };
        let faces = [face(700.0, FaceStyle::Normal), variable];
        assert_eq!(
            best_match(&faces, desired(650.0, FontStyle::Normal)),
            Some(1)
        );
    }

    #[test]
    fn synthesis_rules() {
        assert!(synthesize_bold(700.0, 400.0));
        assert!(!synthesize_bold(600.0, 400.0));
        assert!(!synthesize_bold(700.0, 700.0));
        assert!(synthesize_bold_fallback(600.0, 400.0));
        assert!(!synthesize_bold_fallback(700.0, 700.0));
        assert!(synthesize_oblique(FontStyle::Italic, FaceStyle::Normal));
        assert!(!synthesize_oblique(FontStyle::Italic, FaceStyle::Oblique));
        assert!(!synthesize_oblique(FontStyle::Normal, FaceStyle::Normal));
    }
}
