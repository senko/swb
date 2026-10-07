//! `font-variant-caps` (CSS Fonts 4 §6.7,
//! <https://www.w3.org/TR/css-fonts-4/#font-variant-caps-prop>), with
//! synthesized small capitals as Chromium draws them (measured in Chromium
//! 148).
//!
//! A font with the OpenType features gets them. Otherwise (the primary
//! font decides) small capitals are synthesized: lowercase letters are
//! uppercased and shaped at 70 % of the font size, rounded to whole pixels
//! (14.08 px gives 10 px, 16.4286 px gives 12 px), in the fonts that
//! `GroupShaper::cased_runs` chooses; the other characters keep the font
//! size. If the primary font has the features, text in a fallback font
//! without them gets synthesized small capitals (not measured: no bundled
//! font has `smcp`). `all-small-caps` gives the other characters the small size
//! too. `petite-caps` use small capitals if the font has no petite
//! capitals. `unicase` and `titling-caps` work only with fonts that have
//! the features (Chromium also synthesizes `unicase`: uppercase letters
//! at the small size, measured; not done).

use std::ops::Range;

use swb_style::FontVariantCaps;
use swb_text::{Feature, FontContext, FontId};
use unicode_segmentation::UnicodeSegmentation;

/// How the text of one font is shaped for its `font-variant-caps`.
#[derive(Default)]
pub(crate) struct CapsPlan {
    /// OpenType features for all the text.
    pub(crate) features: Vec<Feature>,
    pub(crate) synthesis: Option<Synthesis>,
}

/// Synthesized small capitals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Synthesis {
    /// Lowercase letters become small capitals.
    SmallCaps,
    /// All characters get the small size; lowercase letters are uppercased.
    AllSmallCaps,
}

/// The plan for text in `font` with `caps`.
pub(crate) fn plan(fonts: &FontContext, font: FontId, caps: FontVariantCaps) -> CapsPlan {
    use FontVariantCaps as C;
    // The features that the font must all have, features to use instead
    // if it has all of those, and the synthesis without them.
    let (full, fallback, synthesis): (&[&[u8; 4]], &[&[u8; 4]], _) = match caps {
        C::Normal => return CapsPlan::default(),
        C::SmallCaps => (&[b"smcp"], &[], Some(Synthesis::SmallCaps)),
        C::AllSmallCaps => (&[b"smcp", b"c2sc"], &[], Some(Synthesis::AllSmallCaps)),
        C::PetiteCaps => (&[b"pcap"], &[b"smcp"], Some(Synthesis::SmallCaps)),
        C::AllPetiteCaps => (
            &[b"pcap", b"c2pc"],
            &[b"smcp", b"c2sc"],
            Some(Synthesis::AllSmallCaps),
        ),
        C::Unicase => (&[b"unic"], &[], None),
        C::TitlingCaps => (&[b"titl"], &[], None),
    };
    let has_all = |tags: &[&[u8; 4]]| {
        !tags.is_empty() && tags.iter().all(|tag| fonts.has_feature(font, **tag))
    };
    let tags = if has_all(full) {
        full
    } else if has_all(fallback) {
        fallback
    } else {
        return CapsPlan {
            features: Vec::new(),
            synthesis,
        };
    };
    CapsPlan {
        features: tags
            .iter()
            .map(|tag| Feature {
                tag: **tag,
                value: 1,
            })
            .collect(),
        synthesis: None,
    }
}

/// The font size of synthesized small capitals for `size`.
pub(crate) fn small_size(size: f32) -> f32 {
    (size * 0.7).round()
}

/// A part of a text run that is shaped one way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CaseSegment {
    pub(crate) range: Range<usize>,
    /// Shaped at the small size.
    pub(crate) small: bool,
    /// Shaped uppercased.
    pub(crate) uppercase: bool,
}

/// Text with synthesized small capitals (see [`case_text`]).
pub(crate) struct CasedText {
    /// The text with its lowercase letters uppercased.
    pub(crate) text: String,
    /// For each character of `text`, its byte offset and the byte offset
    /// of its source character (see [`source_offset`]).
    pub(crate) map: Vec<(usize, usize)>,
    /// The segments, as byte ranges of `text`.
    pub(crate) segments: Vec<CaseSegment>,
}

/// `text` for synthesized small capitals: its lowercase letters
/// uppercased, and its segments.
pub(crate) fn case_text(text: &str, synthesis: Synthesis) -> CasedText {
    let mut out = CasedText {
        text: String::with_capacity(text.len()),
        map: Vec::with_capacity(text.len()),
        segments: Vec::new(),
    };
    for segment in segments(text, synthesis) {
        let start = out.text.len();
        let part = text.get(segment.range.clone()).unwrap_or("");
        for (i, c) in part.char_indices() {
            let source = segment.range.start + i;
            if segment.uppercase {
                for u in c.to_uppercase() {
                    out.map.push((out.text.len(), source));
                    out.text.push(u);
                }
            } else {
                out.map.push((out.text.len(), source));
                out.text.push(c);
            }
        }
        out.segments.push(CaseSegment {
            range: start..out.text.len(),
            ..segment
        });
    }
    out
}

/// Splits `text` into segments by case: the grapheme clusters that change
/// when uppercased, and the others.
fn segments(text: &str, synthesis: Synthesis) -> Vec<CaseSegment> {
    let mut out: Vec<CaseSegment> = Vec::new();
    // By grapheme cluster, so that combining marks stay with their base.
    for (i, cluster) in text.grapheme_indices(true) {
        let lower = cluster.chars().next().is_some_and(changes_when_uppercased);
        let small = lower || synthesis == Synthesis::AllSmallCaps;
        let end = i + cluster.len();
        match out.last_mut() {
            Some(last) if last.uppercase == lower => last.range.end = end,
            _ => out.push(CaseSegment {
                range: i..end,
                small,
                uppercase: lower,
            }),
        }
    }
    out
}

fn changes_when_uppercased(c: char) -> bool {
    let mut upper = c.to_uppercase();
    upper.next() != Some(c) || upper.next().is_some()
}

/// The offset in the source text of byte offset `offset` of a case-mapped
/// text with `map` (see [`CasedText::map`]); `offset` itself if `map` is
/// empty.
pub(crate) fn source_offset(map: &[(usize, usize)], offset: usize) -> usize {
    let i = map.partition_point(|&(o, _)| o <= offset);
    i.checked_sub(1)
        .and_then(|i| map.get(i))
        .map_or(offset, |&(_, source)| source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_caps_segments() {
        let segments = segments("Ab1c", Synthesis::SmallCaps);
        let parts: Vec<(Range<usize>, bool)> = segments
            .iter()
            .map(|s| (s.range.clone(), s.small))
            .collect();
        assert_eq!(
            parts,
            vec![(0..1, false), (1..2, true), (2..3, false), (3..4, true)]
        );
        let all = segments_of("Ab", Synthesis::AllSmallCaps);
        assert_eq!(all, vec![(0..1, true, false), (1..2, true, true)]);
        // A combining mark stays with its base.
        let marks = segments_of("e\u{301}X", Synthesis::SmallCaps);
        assert_eq!(marks, vec![(0..3, true, true), (3..4, false, false)]);
    }

    fn segments_of(text: &str, synthesis: Synthesis) -> Vec<(Range<usize>, bool, bool)> {
        segments(text, synthesis)
            .into_iter()
            .map(|s| (s.range, s.small, s.uppercase))
            .collect()
    }

    #[test]
    fn case_text_keeps_source_offsets() {
        let cased = case_text("aßÉ1", Synthesis::SmallCaps);
        assert_eq!(cased.text, "ASSÉ1");
        let map = &cased.map;
        assert_eq!(source_offset(map, 0), 0);
        assert_eq!(source_offset(map, 1), 1);
        assert_eq!(source_offset(map, 2), 1);
        assert_eq!(source_offset(map, 3), 3);
        assert_eq!(source_offset(map, 5), 5);
        let ranges: Vec<(Range<usize>, bool)> = cased
            .segments
            .iter()
            .map(|s| (s.range.clone(), s.small))
            .collect();
        assert_eq!(ranges, vec![(0..3, true), (3..6, false)]);
        assert_eq!(source_offset(&[], 7), 7);
    }

    #[test]
    fn small_size_rounds_to_whole_pixels() {
        assert_eq!(small_size(14.08), 10.0);
        assert_eq!(small_size(16.0), 11.0);
    }
}
