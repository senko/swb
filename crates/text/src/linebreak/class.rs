//! Line break classes (UAX #14 §5, <https://www.unicode.org/reports/tr14/#Properties>)
//! of Unicode 17.0 and the other character properties that the rules use.
//!
//! The Line_Break values come from the `unicode-linebreak` crate (Unicode
//! 15.0) with the ranges that changed in 17.0 (`tables::CLASS_OVERRIDES`).

use unicode_linebreak::BreakClass;

use super::tables;

/// A line break class: the Line_Break property values of UAX #14.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[expect(
    clippy::upper_case_acronyms,
    reason = "the abbreviations of UAX #14, so that the rules read like the specification"
)]
pub(super) enum Class {
    AI,
    AK,
    AL,
    AP,
    AS,
    B2,
    BA,
    BB,
    BK,
    CB,
    CJ,
    CL,
    CM,
    CP,
    CR,
    EB,
    EM,
    EX,
    GL,
    H2,
    H3,
    HH,
    HL,
    HY,
    ID,
    IN,
    IS,
    JL,
    JT,
    JV,
    LF,
    NL,
    NS,
    NU,
    OP,
    PO,
    PR,
    QU,
    RI,
    SA,
    SG,
    SP,
    SY,
    VF,
    VI,
    WJ,
    XX,
    ZW,
    ZWJ,
}

/// The Line_Break value of `c` in Unicode 17.0.
pub(super) fn class(c: char) -> Class {
    let cp = u32::from(c);
    if cp >= tables::FIRST_OVERRIDE {
        let table = &tables::CLASS_OVERRIDES;
        let i = table.partition_point(|&(_, last, _)| last < cp);
        if let Some(&(first, _, class)) = table.get(i)
            && first <= cp
        {
            return class;
        }
    }
    from_crate(unicode_linebreak::break_property(cp))
}

fn from_crate(class: BreakClass) -> Class {
    use BreakClass as B;
    match class {
        B::Mandatory => Class::BK,
        B::CarriageReturn => Class::CR,
        B::LineFeed => Class::LF,
        B::CombiningMark => Class::CM,
        B::NextLine => Class::NL,
        B::Surrogate => Class::SG,
        B::WordJoiner => Class::WJ,
        B::ZeroWidthSpace => Class::ZW,
        B::NonBreakingGlue => Class::GL,
        B::Space => Class::SP,
        B::ZeroWidthJoiner => Class::ZWJ,
        B::BeforeAndAfter => Class::B2,
        B::After => Class::BA,
        B::Before => Class::BB,
        B::Hyphen => Class::HY,
        B::Contingent => Class::CB,
        B::ClosePunctuation => Class::CL,
        B::CloseParenthesis => Class::CP,
        B::Exclamation => Class::EX,
        B::Inseparable => Class::IN,
        B::NonStarter => Class::NS,
        B::OpenPunctuation => Class::OP,
        B::Quotation => Class::QU,
        B::InfixSeparator => Class::IS,
        B::Numeric => Class::NU,
        B::Postfix => Class::PO,
        B::Prefix => Class::PR,
        B::Symbol => Class::SY,
        B::Ambiguous => Class::AI,
        B::Alphabetic => Class::AL,
        B::ConditionalJapaneseStarter => Class::CJ,
        B::EmojiBase => Class::EB,
        B::EmojiModifier => Class::EM,
        B::HangulLvSyllable => Class::H2,
        B::HangulLvtSyllable => Class::H3,
        B::HebrewLetter => Class::HL,
        B::Ideographic => Class::ID,
        B::HangulLJamo => Class::JL,
        B::HangulVJamo => Class::JV,
        B::HangulTJamo => Class::JT,
        B::RegionalIndicator => Class::RI,
        B::ComplexContext => Class::SA,
        B::Unknown => Class::XX,
    }
}

/// The class of `c` (whose Line_Break value is `class`) for the rules (LB1),
/// as Chromium resolves it: AI, SG and XX as AL (no East Asian context),
/// SA as CM for marks and AL otherwise (no dictionary), CJ as ID
/// (`line-break: auto`), and U+000B and U+000C as CM (Chromium does not
/// break around them).
pub(super) fn resolve(c: char, class: Class) -> Class {
    match class {
        Class::SA if in_ranges(&tables::SA_MARKS, c) => Class::CM,
        Class::AI | Class::SG | Class::XX | Class::SA => Class::AL,
        Class::CJ => Class::ID,
        Class::BK if matches!(c, '\u{B}' | '\u{C}') => Class::CM,
        other => other,
    }
}

/// True if `c` has East_Asian_Width F, W or H (`$EastAsian`).
pub(super) fn is_east_asian(c: char) -> bool {
    u32::from(c) >= 0x1100 && in_ranges(&tables::EAST_ASIAN, c)
}

/// True if `c` is a quotation mark (QU) with General_Category Pi.
pub(super) fn is_initial_quote(c: char) -> bool {
    in_ranges(&tables::INITIAL_QUOTES, c)
}

/// True if `c` is a quotation mark (QU) with General_Category Pf.
pub(super) fn is_final_quote(c: char) -> bool {
    in_ranges(&tables::FINAL_QUOTES, c)
}

/// True if `c` is Extended_Pictographic and unassigned (LB30b).
pub(super) fn is_unassigned_pictographic(c: char) -> bool {
    u32::from(c) >= 0x1F000 && in_ranges(&tables::UNASSIGNED_PICTOGRAPHIC, c)
}

/// True if `c` is a typographic letter unit for `word-break: keep-all`:
/// General_Category L* or N*.
pub(super) fn is_letter_unit(c: char) -> bool {
    in_ranges(&tables::LETTER_UNITS, c)
}

/// True if `c` is in one of the sorted, disjoint `ranges`.
fn in_ranges(ranges: &[(u32, u32)], c: char) -> bool {
    let cp = u32::from(c);
    let i = ranges.partition_point(|&(_, last)| last < cp);
    ranges.get(i).is_some_and(|&(first, _)| first <= cp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_of_unicode_17() {
        assert_eq!(class('a'), Class::AL);
        assert_eq!(class('\u{2010}'), Class::HH);
        assert_eq!(class('\u{2013}'), Class::HH);
        assert_eq!(class('\u{AD}'), Class::BA);
        assert_eq!(class('\u{1B05}'), Class::AK);
        assert_eq!(class('\u{1B44}'), Class::VI);
        assert_eq!(class('\u{1BF2}'), Class::VF);
        assert_eq!(class('\u{11003}'), Class::AP);
        assert_eq!(class('\u{1B50}'), Class::AS);
        assert_eq!(class('\u{34F}'), Class::CM);
        assert_eq!(class('\u{10FFFF}'), Class::XX);
    }

    #[test]
    fn resolution() {
        assert_eq!(resolve('\u{E01}', class('\u{E01}')), Class::AL);
        assert_eq!(resolve('\u{E31}', class('\u{E31}')), Class::CM);
        assert_eq!(resolve('\u{3041}', class('\u{3041}')), Class::ID);
        assert_eq!(resolve('\u{B}', class('\u{B}')), Class::CM);
        assert_eq!(resolve('\u{2028}', class('\u{2028}')), Class::BK);
    }

    #[test]
    fn properties() {
        assert!(is_east_asian('\u{4E00}'));
        assert!(is_east_asian('\u{FF08}'));
        assert!(!is_east_asian('('));
        assert!(is_initial_quote('\u{201C}') && is_final_quote('\u{201D}'));
        assert!(is_initial_quote('\u{AB}') && is_final_quote('\u{BB}'));
        assert!(!is_initial_quote('"') && !is_final_quote('"'));
        assert!(is_letter_unit('a') && is_letter_unit('\u{4E00}') && is_letter_unit('\u{2460}'));
        assert!(!is_letter_unit('\u{25CC}') && !is_letter_unit('\u{1F600}'));
    }
}
