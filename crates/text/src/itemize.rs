//! Itemization: splitting text into runs that one font can render.
//!
//! The unit of font assignment is the extended grapheme cluster (UAX #29),
//! so combining marks and variation selectors stay with their base
//! character. A font covers a cluster if it has glyphs for all its
//! characters as written, or for all characters of its NFC or NFD form: the
//! shaper composes and decomposes characters to use the glyphs that a font
//! has (`HarfBuzz` normalization), and Blink keeps a font when shaping finds
//! glyphs. For each cluster the fonts are tried in this order:
//!
//! 1. the fonts of the query's families that cover the whole cluster;
//! 2. system fallback fonts (for each character the first family font
//!    lacks) that cover the whole cluster;
//! 3. the query's fonts, then the system fallback font, that cover the base
//!    character;
//! 4. the first font of the query (the text renders with `.notdef`).
//!
//! Whitespace, control characters and default-ignorable characters stay in
//! the current run if its font covers them.

use std::ops::Range;

use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

use crate::FontId;
use crate::context::{FontContext, fallback_language};
use crate::query::FontQuery;

/// A range of text with the font to render it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontRun {
    /// Byte range in the itemized text.
    pub range: Range<usize>,
    /// The font for this range.
    pub font: FontId,
}

impl FontContext {
    /// Splits `text` into runs, each with a font that has glyphs for its
    /// characters. The runs cover the whole text in order. Empty text gives
    /// no runs.
    pub fn itemize(&mut self, text: &str, query: &FontQuery<'_>) -> Vec<FontRun> {
        if text.is_empty() {
            return Vec::new();
        }
        let fonts = self.family_fonts(query);
        let primary = match fonts.first() {
            Some(font) => *font,
            None => self.default_font(query),
        };
        // Common case: ASCII text in a font that covers it.
        if text.is_ascii()
            && self
                .instance(primary)
                .and_then(|i| i.face.as_ref())
                .is_some_and(|face| face.covers_ascii(text))
        {
            return vec![FontRun {
                range: 0..text.len(),
                font: primary,
            }];
        }
        let fonts = if fonts.is_empty() {
            vec![primary]
        } else {
            fonts
        };
        let language = fallback_language(query);

        let mut runs: Vec<FontRun> = Vec::new();
        for (start, cluster) in text.grapheme_indices(true) {
            let previous = runs.last().map(|run| run.font);
            let font = match previous {
                Some(font) if is_neutral(cluster) && self.covers_cluster(font, cluster) => font,
                _ => self.font_for_cluster(cluster, &fonts, &language, query),
            };
            let range = start..start + cluster.len();
            match runs.last_mut() {
                Some(run) if run.font == font => run.range.end = range.end,
                _ => runs.push(FontRun { range, font }),
            }
        }
        runs
    }

    fn font_for_cluster(
        &mut self,
        cluster: &str,
        fonts: &[FontId],
        language: &str,
        query: &FontQuery<'_>,
    ) -> FontId {
        let primary = fonts[0];
        if let Some(font) = fonts.iter().find(|f| self.covers_cluster(**f, cluster)) {
            return *font;
        }
        let Some(base) = cluster.chars().find(|c| !is_ignorable(*c)) else {
            return primary;
        };
        // System fallback, asked for each character that the first font
        // lacks (the base first).
        let mut base_fallback = None;
        // Each fallback font is checked against the cluster once, so long
        // clusters (many combining marks) cost linear time.
        let mut checked: Vec<FontId> = Vec::new();
        for c in cluster.chars().filter(|c| !is_ignorable(*c)) {
            if self.has_glyph(primary, c) {
                continue;
            }
            if let Some(font) = self.fallback_font(c, language, query) {
                if !checked.contains(&font) {
                    if self.covers_cluster(font, cluster) {
                        return font;
                    }
                    checked.push(font);
                }
                if c == base {
                    base_fallback = Some(font);
                }
            }
        }
        if let Some(font) = fonts.iter().find(|f| self.has_glyph(**f, base)) {
            return *font;
        }
        base_fallback.unwrap_or(primary)
    }

    /// True if `font` has glyphs for all characters of `cluster` that need
    /// one, as written or after normalization.
    fn covers_cluster(&self, font: FontId, cluster: &str) -> bool {
        cluster_covered(cluster, |c| self.has_glyph(font, c))
    }
}

/// True if `has_glyph` holds for all characters of `cluster` that need a
/// glyph, either as written or in the NFC or NFD form of the cluster.
///
/// Examples: `e` + U+0301 is covered by a font with `é` but without the
/// combining acute accent (NFC); `≉` by a font with `≈` and U+0338 (NFD).
fn cluster_covered(cluster: &str, has_glyph: impl Fn(char) -> bool) -> bool {
    // Normalization is skipped for unusually long clusters (hundreds of
    // combining marks), which only occur in hostile or broken text.
    const MAX_NORMALIZED_LEN: usize = 64;
    let covers = |c: char| is_ignorable(c) || has_glyph(c);
    cluster.chars().all(covers)
        || (!cluster.is_ascii()
            && cluster.len() <= MAX_NORMALIZED_LEN
            && (cluster.nfc().all(covers) || cluster.nfd().all(covers)))
}

/// True for clusters that can join any run: whitespace and characters that
/// are not rendered.
fn is_neutral(cluster: &str) -> bool {
    cluster
        .chars()
        .all(|c| c.is_whitespace() || is_ignorable(c))
}

/// Control characters and Unicode default-ignorable code points. They do
/// not need a glyph: shaping hides them, or layout handles them.
fn is_ignorable(c: char) -> bool {
    c.is_control()
        || matches!(
            c as u32,
            0x00AD
                | 0x034F
                | 0x061C
                | 0x115F..=0x1160
                | 0x17B4..=0x17B5
                | 0x180B..=0x180F
                | 0x200B..=0x200F
                | 0x202A..=0x202E
                | 0x2060..=0x206F
                | 0x3164
                | 0xFE00..=0xFE0F
                | 0xFEFF
                | 0xFFA0
                | 0xFFF0..=0xFFF8
                | 0x1BCA0..=0x1BCA3
                | 0x1D173..=0x1D17A
                | 0xE0000..=0xE0FFF
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignorable_characters() {
        assert!(is_ignorable('\n'));
        assert!(is_ignorable('\u{200D}'));
        assert!(is_ignorable('\u{FE0F}'));
        assert!(!is_ignorable('a'));
        assert!(!is_ignorable(' '));
    }

    #[test]
    fn coverage_after_normalization() {
        // A font with `é` but without the combining acute accent.
        let precomposed = |c: char| matches!(c, 'e' | 'é');
        assert!(cluster_covered("e\u{0301}", precomposed));
        assert!(cluster_covered("é", precomposed));
        // A font with `≈` and U+0338 but without `≉`.
        let decomposed = |c: char| matches!(c, '≈' | '\u{0338}');
        assert!(cluster_covered("≉", decomposed));
        // Neither form is complete.
        let base_only = |c: char| c == 'e';
        assert!(!cluster_covered("e\u{0301}", base_only));
        assert!(!cluster_covered("é", base_only));
        // Ignorable characters need no glyph in any form.
        assert!(cluster_covered("e\u{200D}", base_only));
    }

    #[test]
    fn neutral_clusters() {
        assert!(is_neutral(" "));
        assert!(is_neutral("\u{00A0}"));
        assert!(is_neutral("\r\n"));
        assert!(!is_neutral("a"));
    }
}
