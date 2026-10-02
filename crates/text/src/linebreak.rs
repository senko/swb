//! Line break opportunities (UAX #14) through `unicode-linebreak`.
//!
//! Spec: <https://www.unicode.org/reports/tr14/>. The crate implements the
//! default algorithm and resolves complex-context (SA) characters, such as
//! Thai, to alphabetic, so it gives no breaks inside Thai words.

use unicode_linebreak::BreakOpportunity;

/// The kind of a line break opportunity.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum BreakKind {
    /// The line must break here (after a hard line break character).
    Mandatory,
    /// The line may break here.
    Allowed,
}

/// Line break opportunities in `text`, as `(index, kind)` where `index` is
/// the byte offset of the character before which the line can break.
///
/// UAX #14 always allows a break at the end of the text. It is reported only
/// if the text ends with a hard line break character (then as
/// `Mandatory`), because the end of one text node is usually not the end of
/// the paragraph.
pub fn line_breaks(text: &str) -> impl Iterator<Item = (usize, BreakKind)> + '_ {
    let ends_with_hard_break = text.chars().next_back().is_some_and(is_hard_break);
    unicode_linebreak::linebreaks(text).filter_map(move |(index, opportunity)| {
        if index == text.len() && !ends_with_hard_break {
            return None;
        }
        let kind = match opportunity {
            BreakOpportunity::Mandatory => BreakKind::Mandatory,
            BreakOpportunity::Allowed => BreakKind::Allowed,
        };
        Some((index, kind))
    })
}

/// Characters of line break classes BK, CR, LF and NL.
fn is_hard_break(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn breaks(text: &str) -> Vec<(usize, BreakKind)> {
        line_breaks(text).collect()
    }

    #[test]
    fn spaces() {
        assert_eq!(
            breaks("Hello big world"),
            vec![(6, BreakKind::Allowed), (10, BreakKind::Allowed)]
        );
    }

    #[test]
    fn hyphens() {
        assert_eq!(breaks("well-known"), vec![(5, BreakKind::Allowed)]);
        // No break before a hyphen or after a leading one.
        assert_eq!(breaks("-5"), vec![]);
    }

    #[test]
    fn cjk_breaks_between_ideographs() {
        // Each ideograph is three bytes in UTF-8.
        assert_eq!(
            breaks("漢字です"),
            vec![
                (3, BreakKind::Allowed),
                (6, BreakKind::Allowed),
                (9, BreakKind::Allowed)
            ]
        );
    }

    #[test]
    fn no_break_before_closing_punctuation() {
        assert_eq!(breaks("文。"), vec![]);
    }

    #[test]
    fn mandatory_breaks() {
        assert_eq!(breaks("a\nb"), vec![(2, BreakKind::Mandatory)]);
        assert_eq!(
            breaks("a\r\nb\n"),
            vec![(3, BreakKind::Mandatory), (5, BreakKind::Mandatory)]
        );
    }

    #[test]
    fn no_break_inside_words_or_at_end() {
        assert_eq!(breaks("word"), vec![]);
        assert_eq!(breaks(""), vec![]);
    }
}
