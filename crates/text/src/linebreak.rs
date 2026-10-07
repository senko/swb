//! Line break opportunities (CSS Text 3 §5,
//! <https://www.w3.org/TR/css-text-3/#line-breaking>; UAX #14 for Unicode
//! 17.0, <https://www.unicode.org/reports/tr14/tr14-55.html>), tailored to
//! give the same results as Chromium 148.
//!
//! This is a clean-room implementation. Sources: UAX #14 (revisions 51 to
//! 57, for Unicode 15.1 to 18.0), the Unicode Character Database 17.0, CSS
//! Text 3, the `unicode-linebreak` crate (the Line_Break data of Unicode
//! 15.0), and black-box measurements of Chromium 148. No browser or ICU
//! source code was read.
//!
//! # Measurements
//!
//! `swbtools linebreaks` (`tools/swbtools/linebreaks.py`, see
//! `docs/testing.md`) renders strings in Chromium in a box of width 0 (the
//! line breaks at every opportunity) and reads the line of each code point.
//! It writes `crates/text/tests/linebreak/`: the opportunity between every
//! pair of U+0020..U+00FF and between samples of every UAX #14 class, in
//! several contexts, for `word-break: normal`, `break-all` and `keep-all`
//! and for `hyphens: none`, and a list of strings (spaces, controls,
//! hyphens, numbers, quotation marks, scripts, emoji, soft hyphens). The
//! test `tests/linebreak.rs` compares this module with every measured
//! position. `swbtools linebreak-tables` makes `linebreak/tables.rs` from
//! the Unicode Character Database 17.0 and from the Latin-1 measurements.
//!
//! # Rules
//!
//! In this order, for the position between the code points `p` and `c`:
//!
//! 1. After LF, and after CR not before LF, the line must break (LB4,
//!    LB5); there is no break before LF or CR (LB6).
//! 2. No break before a space, a tab or U+3000 IDEOGRAPHIC SPACE; a break
//!    after a run of spaces and tabs (also if U+3000 follows the spaces),
//!    before anything else. (Chromium treats tabs as spaces. So the `SP*`
//!    forms of LB8 and LB14 to LB17 and the rules LB13 and LB15d after
//!    spaces never prevent a break.) With `break-all`, U+3000 after a
//!    character of the `break-all` classes (below) breaks like a space.
//! 3. `hyphens: none`: no break after U+00AD; with `break-all`, the breaks
//!    that `break-all` adds remain.
//! 4. If both `p` and `c` are in U+0000..U+00FF, a measured pair table
//!    (`tables::LATIN1_*`: one for `normal` and `keep-all`, one for
//!    `break-all`, and `LATIN1_BREAK_ALL_SHY_NO_HYPHENS`, the row of
//!    U+00AD for `break-all` with `hyphens: none`) decides, without
//!    context. C0 controls, U+000B and U+000C use the entries of U+0080.
//!    The table differs from UAX #14 in
//!    many places, for example: no break after `!`, `/`, `|`, `}` or `]`
//!    before letters and digits; a break between ASCII symbols and `(`; no
//!    break between `$`, `+`, `\`, `%` and other punctuation; a break
//!    before `-` after `-` and `?`; a break after U+0085 that is not
//!    forced. Exceptions, measured with many preceding characters:
//!    - `-` before an ASCII digit: a break only after an ASCII letter or
//!      digit (`978-1`, but not `a -5`, `(-1)` or `а-0`); always with
//!      `break-all`.
//!    - `-` before a character of class AL or AI in U+00A0..U+00FF
//!      (letters, `©`, `×`): step 5 (LB20a, LB21a).
//!    - With `break-all`: no break after U+000B and U+000C; another control
//!      character uses step 5 if it is attached to a base that is not a
//!      control character, and does not break otherwise.
//! 5. Otherwise the rules of UAX #14 for Unicode 17.0 on combining
//!    sequences (LB9), with these tailorings:
//!    - Classes (LB1): AI, SG, XX as AL; SA as CM for marks and AL
//!      otherwise; CJ as ID (`line-break: auto`); U+000B and U+000C as CM.
//!    - BK and NL (U+0085, U+2028, U+2029) allow a break after them; it is
//!      not forced, and there is no break before a following space.
//!    - The rules look back only to the last break opportunity: a hyphen
//!      after an opportunity is word-initial for LB20a (`?-é`, `--é` and,
//!      with `break-all`, `a-é` do not break after the hyphen).
//!    - `break-all` adds breaks (measured): after a letter (class AL but
//!      not XX, or AI, HL, NU, SA; and `+`), and after `/`, before a
//!      letter, OP, PR, BA or HY; after CP before a letter, PR, BA or HY; after CL before
//!      PR, BA or HY; after IS or PO before a letter that is not SA, BA or
//!      HY; after BA before a letter, BA or HY; after EX before BA or HY;
//!      between HY and NU. An SA mark is a letter there (`บ้` breaks before
//!      the tone mark). LB8a (ZWJ) and the attachment of other combining
//!      marks still apply.
//!    - `keep-all`: no break between two typographic letter units
//!      (General_Category L* or N* with their combining marks and VF or VI
//!      spacing marks, but not SA or AP; CSS Text 3 §5.2). Symbols of class
//!      AL, AI or ID still break.
//!
//! # Differences that remain
//!
//! - Chromium breaks Thai and Lao (and probably the other SA scripts) at
//!   word boundaries from a dictionary; swb breaks them only at spaces.
//! - Chromium does not break inside a glyph cluster of the shaped text
//!   (a ligature such as `ff` in a font that has one, a Devanagari conjunct
//!   such as `क्ष` with `break-all`), and with `break-all` it does not break
//!   where the font shapes combining marks across the position (two marks
//!   in a row, a mark before a letter of another script, a Khmer subscript
//!   consonant). That depends on the font; layout skips opportunities
//!   inside glyph clusters (`inline/shaping.rs`), which covers the first
//!   case.
//! - With `break-all`, a Thai mark after an ideograph counts as a letter in
//!   Chromium before BA, HY and other Thai marks.
//! - Emoji modifiers, Brahmi viramas and similar marks without their base:
//!   Chromium's breaks depend on the clusters of the fallback font.
//! - C0 control characters after a space or (with `break-all`) after an
//!   ideograph.
//! - The measured differences are listed in
//!   `crates/text/tests/linebreak/known-differences.txt`.

mod class;
mod tables;

use class::{
    Class, class, is_east_asian, is_final_quote, is_initial_quote, is_letter_unit,
    is_unassigned_pictographic, resolve,
};

/// The kind of a line break opportunity.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum BreakKind {
    /// The line must break here (after a hard line break character).
    Mandatory,
    /// The line may break here.
    Allowed,
}

/// The `word-break` property for break opportunities. `word-break:
/// break-word` is `Normal` here: it changes only what happens to a word
/// that does not fit (see CSS Text 3 §5.2).
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum WordBreak {
    /// UAX #14 as tailored by Chromium.
    #[default]
    Normal,
    /// Also break between letters (`break-all`).
    BreakAll,
    /// No breaks between letters and numbers (`keep-all`).
    KeepAll,
}

/// The rules for the break opportunity before a character.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct BreakRules {
    /// The `word-break` behaviour.
    pub word_break: WordBreak,
    /// False for `hyphens: none`: no break after U+00AD SOFT HYPHEN.
    pub soft_hyphens: bool,
}

impl Default for BreakRules {
    fn default() -> Self {
        BreakRules {
            word_break: WordBreak::Normal,
            soft_hyphens: true,
        }
    }
}

/// Line break opportunities in `text`, as `(index, kind)` where `index` is
/// the byte offset of the character before which the line can break.
/// `rules(index)` gives the rules for the opportunity at `index` (measured
/// in Chromium 148: the style of the text before it decides).
///
/// A line feed or carriage return gives a `Mandatory` break after it; the
/// end of the text is reported only then, because the end of one text is
/// usually not the end of the paragraph. Offset 0 is never reported.
///
/// The time is linear in the length of the text.
pub fn line_breaks(text: &str, rules: impl Fn(usize) -> BreakRules) -> Vec<(usize, BreakKind)> {
    let mut out = Vec::new();
    let mut chars = text.char_indices();
    let Some((_, first)) = chars.next() else {
        return out;
    };
    let mut breaker = Breaker::new(text, first);
    for (i, c) in chars {
        let kind = breaker.decide(i, c, rules(i));
        if let Some(kind) = kind {
            out.push((i, kind));
        }
        breaker.advance(c, kind.is_some());
    }
    if matches!(breaker.prev_char, '\n' | '\r') {
        out.push((text.len(), BreakKind::Mandatory));
    }
    out
}

const SOFT_HYPHEN: char = '\u{AD}';
const IDEOGRAPHIC_SPACE: char = '\u{3000}';
const DOTTED_CIRCLE: char = '\u{25CC}';

/// A combining character sequence (LB9): a base character and the
/// combining marks attached to it.
#[derive(Copy, Clone, Debug)]
struct Unit {
    /// The class for the rules: resolved (LB1), and AL for a combining mark
    /// without a base (LB10).
    class: Class,
    /// The Line_Break value of the base.
    raw: Class,
    /// The base character.
    base: char,
    /// The last character (LB8a looks for ZWJ).
    last: char,
}

impl Unit {
    fn new(c: char) -> Unit {
        let raw = class(c);
        let class = match resolve(c, raw) {
            Class::CM | Class::ZWJ => Class::AL,
            other => other,
        };
        Unit {
            class,
            raw,
            base: c,
            last: c,
        }
    }

    /// True if the base is East Asian (`$EastAsian`); a mark without a base
    /// is not (LB10: it behaves as U+0041).
    fn east_asian(&self) -> bool {
        !matches!(self.raw, Class::CM | Class::ZWJ) && is_east_asian(self.base)
    }

    /// True for a letter of `break-all`: class AL (but not XX), AI, HL, NU
    /// or SA (marks too), and `+`.
    fn breaks_all_letter(&self) -> bool {
        matches!(
            self.raw,
            Class::AL | Class::AI | Class::HL | Class::NU | Class::SA
        ) || self.base == '+'
    }

    /// True for the characters after which `break-all` adds breaks before
    /// letters or hyphens (see `break_all_adds`; HY adds breaks only before
    /// NU).
    fn breaks_all_after(&self) -> bool {
        self.breaks_all_letter()
            || matches!(
                self.raw,
                Class::BA | Class::CL | Class::CP | Class::EX | Class::IS | Class::PO | Class::SY
            )
    }

    /// True for a typographic letter unit of `keep-all`: General_Category
    /// L* or N*, but not SA or AP (measured).
    fn letter_unit(&self) -> bool {
        !matches!(self.raw, Class::SA | Class::AP | Class::CM | Class::ZWJ)
            && is_letter_unit(self.base)
    }

    /// True for AK, AS and U+25CC DOTTED CIRCLE (LB28a).
    fn aksara(&self) -> bool {
        matches!(self.class, Class::AK | Class::AS) || self.base == DOTTED_CIRCLE
    }
}

/// True if a combining mark attaches to a unit of `class` (LB9).
fn attaches_to(class: Class) -> bool {
    !matches!(
        class,
        Class::BK | Class::CR | Class::LF | Class::NL | Class::SP | Class::ZW
    )
}

/// True if `c` attaches to a preceding unit of class `class` (LB9).
fn attaches(c: char, class: Class) -> bool {
    matches!(resolve(c, self::class(c)), Class::CM | Class::ZWJ) && attaches_to(class)
}

/// `word-break: break-all`: true if the line can break between `prev` and
/// `next` in addition to the opportunities of `normal` (measured).
fn break_all_adds(prev: &Unit, next: &Unit) -> bool {
    use Class::{BA, CL, CP, EX, HY, IS, OP, PO, PR, SA, SY};
    let letter = next.breaks_all_letter();
    let hyphen = matches!(next.raw, BA | HY);
    match prev.raw {
        _ if prev.raw == SY || prev.breaks_all_letter() => {
            letter || hyphen || matches!(next.raw, OP | PR)
        }
        CP => letter || hyphen || next.raw == PR,
        CL => hyphen || next.raw == PR,
        IS | PO => (letter && next.raw != SA) || hyphen,
        BA => letter || hyphen,
        EX => hyphen,
        _ => false,
    }
}

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t')
}

/// The state of LB25 for the units up to the position.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Number {
    /// Not in a number.
    None,
    /// After `NU (SY | IS)*`.
    Digits,
    /// After `NU (SY | IS)* (CL | CP)`.
    Closed,
}

/// The state of the line breaker at a position.
struct Breaker<'a> {
    text: &'a str,
    /// The unit before the position.
    prev: Unit,
    /// The unit before `prev`; None at the start of the text and if the
    /// line can break between it and `prev` (the rules look back only to
    /// the last opportunity).
    before: Option<Unit>,
    /// The code point before the position.
    prev_char: char,
    /// The code point before `prev_char`.
    before_char: Option<char>,
    /// LB25.
    number: Number,
    /// True if `prev` ends a run of an odd number of RI (LB30a).
    ri_odd: bool,
    /// True if the code point before the position is U+3000 after a space
    /// or tab (and other U+3000): the line can break before anything.
    ideographic_after_space: bool,
}

impl<'a> Breaker<'a> {
    fn new(text: &'a str, first: char) -> Self {
        let prev = Unit::new(first);
        Breaker {
            text,
            prev,
            before: None,
            prev_char: first,
            before_char: None,
            number: next_number(Number::None, prev.class),
            ri_odd: prev.class == Class::RI,
            ideographic_after_space: false,
        }
    }

    /// Moves the position after `c`; `broke` tells whether the line can
    /// break before `c`.
    fn advance(&mut self, c: char, broke: bool) {
        if !broke && attaches(c, self.prev.class) {
            self.prev.last = c;
        } else {
            let unit = Unit::new(c);
            let number = if broke { Number::None } else { self.number };
            self.number = next_number(number, unit.class);
            self.ri_odd = unit.class == Class::RI && !(self.prev.class == Class::RI && self.ri_odd);
            self.before = (!broke).then_some(self.prev);
            self.prev = unit;
        }
        self.ideographic_after_space =
            c == IDEOGRAPHIC_SPACE && (is_space(self.prev_char) || self.ideographic_after_space);
        self.before_char = Some(self.prev_char);
        self.prev_char = c;
    }

    /// The opportunity before `c` at byte `i`.
    fn decide(&self, i: usize, c: char, rules: BreakRules) -> Option<BreakKind> {
        let p = self.prev_char;
        if p == '\n' || (p == '\r' && c != '\n') {
            return Some(BreakKind::Mandatory);
        }
        let allowed =
            if matches!(p, '\r') || matches!(c, '\n' | '\r' | IDEOGRAPHIC_SPACE) || is_space(c) {
                false
            } else if is_space(p) || self.ideographic_after_space {
                true
            } else if rules.word_break == WordBreak::BreakAll
                && p == IDEOGRAPHIC_SPACE
                && self.before.is_some_and(|u| u.breaks_all_after())
            {
                // With `break-all`, U+3000 after a letter or punctuation breaks
                // like a space.
                true
            } else if !rules.soft_hyphens && p == SOFT_HYPHEN {
                rules.word_break == WordBreak::BreakAll && self.break_all_after_soft_hyphen(c)
            } else if let Some(allowed) = self.latin1_pair(p, c, rules.word_break) {
                allowed
            } else {
                self.unit_rules(i, c, rules.word_break)
            };
        allowed.then_some(BreakKind::Allowed)
    }

    /// `break-all` with `hyphens: none` after U+00AD: only the breaks that
    /// `break-all` adds.
    fn break_all_after_soft_hyphen(&self, c: char) -> bool {
        match latin1_group(c) {
            Some(g) => tables::LATIN1_BREAK_ALL_SHY_NO_HYPHENS & (1 << g) != 0,
            None => break_all_adds(&self.prev, &Unit::new(c)),
        }
    }

    /// The opportunity between two code points of U+0000..U+00FF from the
    /// measured tables; None if the unit rules decide.
    fn latin1_pair(&self, p: char, c: char, word_break: WordBreak) -> Option<bool> {
        let (from, to) = (latin1_group(p)?, latin1_group(c)?);
        if p == '-' {
            if c.is_ascii_digit() {
                return Some(
                    word_break == WordBreak::BreakAll
                        || self.before_char.is_some_and(|b| b.is_ascii_alphanumeric()),
                );
            }
            if u32::from(c) >= 0xA0 && matches!(class(c), Class::AL | Class::AI) {
                return None;
            }
        }
        let row = if word_break == WordBreak::BreakAll {
            if matches!(p, '\u{B}' | '\u{C}') {
                return Some(false);
            }
            if p.is_control() && p != '\u{85}' {
                // A control character after a base uses the unit rules; a
                // control character without a base breaks nowhere.
                return if self.prev.base.is_control() {
                    Some(false)
                } else {
                    None
                };
            }
            tables::LATIN1_BREAK_ALL[from]
        } else {
            tables::LATIN1_NORMAL[from]
        };
        Some(row & (1 << to) != 0)
    }

    /// The rules of UAX #14 with Chromium's tailorings for the position
    /// between the unit `self.prev` and `c`.
    fn unit_rules(&self, i: usize, c: char, word_break: WordBreak) -> bool {
        let prev = self.prev;
        let next = Unit::new(c);
        // LB4, LB5: Chromium allows a break after BK and NL; it is not
        // forced.
        if matches!(prev.class, Class::BK | Class::NL) {
            return true;
        }
        // LB6, LB7.
        if matches!(next.class, Class::BK | Class::NL | Class::ZW) {
            return false;
        }
        // LB8.
        if prev.class == Class::ZW {
            return true;
        }
        // LB8a.
        if prev.last == '\u{200D}' {
            return false;
        }
        // LB9: a combining mark stays with its base. With `break-all`, an
        // SA mark is a letter.
        if attaches(c, prev.class) {
            return word_break == WordBreak::BreakAll
                && next.raw == Class::SA
                && break_all_adds(&prev, &next);
        }
        // LB11, LB12.
        if next.class == Class::WJ || prev.class == Class::WJ || prev.class == Class::GL {
            return false;
        }
        if word_break == WordBreak::BreakAll
            && (break_all_adds(&prev, &next)
                || (prev.class == Class::HY && next.class == Class::NU))
        {
            return true;
        }
        let allowed = self.pair_rules(i, c, prev, next);
        if allowed
            && word_break == WordBreak::KeepAll
            && self.letter_unit_before()
            && next.letter_unit()
        {
            return false;
        }
        allowed
    }

    /// True if `prev` is (or ends) a typographic letter unit: a spacing
    /// mark of class VF or VI belongs to the letter before it.
    fn letter_unit_before(&self) -> bool {
        self.prev.letter_unit()
            || (matches!(self.prev.raw, Class::VF | Class::VI)
                && self.before.is_some_and(|u| u.letter_unit()))
    }

    /// LB12a to LB31.
    #[expect(clippy::too_many_lines, reason = "one block per rule of UAX #14")]
    fn pair_rules(&self, i: usize, c: char, prev: Unit, next: Unit) -> bool {
        use Class::{
            AL, AP, B2, BA, BB, BK, CB, CL, CP, CR, EB, EM, EX, GL, H2, H3, HH, HL, HY, ID, IN, IS,
            JL, JT, JV, LF, NL, NS, NU, OP, PO, PR, QU, RI, SP, SY, VF, VI, WJ, ZW,
        };
        let (a, b) = (prev.class, next.class);
        let before = self.before.map(|u| u.class);
        // LB12a.
        if b == GL && !matches!(a, SP | BA | HY | HH) {
            return false;
        }
        // LB13.
        if matches!(b, CL | CP | EX | SY) {
            return false;
        }
        // LB14 (spaces are handled before).
        if a == OP {
            return false;
        }
        // LB15a.
        if a == QU
            && is_initial_quote(prev.base)
            && before.is_none_or(|x| matches!(x, BK | CR | LF | NL | OP | QU | GL | SP | ZW))
        {
            return false;
        }
        // LB15b.
        if b == QU
            && is_final_quote(c)
            && self.following(i, c)[0].is_none_or(|n| {
                matches!(
                    n.class,
                    SP | GL | WJ | CL | QU | CP | EX | IS | SY | BK | CR | LF | NL | ZW
                )
            })
        {
            return false;
        }
        // LB15d (LB15c is after spaces).
        if b == IS {
            return false;
        }
        // LB16, LB17 (without spaces).
        if (matches!(a, CL | CP) && b == NS) || (a == B2 && b == B2) {
            return false;
        }
        // LB19.
        if (b == QU && !is_initial_quote(c)) || (a == QU && !is_final_quote(prev.base)) {
            return false;
        }
        // LB19a.
        if b == QU
            && (!prev.east_asian() || self.following(i, c)[0].is_none_or(|n| !n.east_asian()))
        {
            return false;
        }
        if a == QU && (!next.east_asian() || self.before.is_none_or(|u| !u.east_asian())) {
            return false;
        }
        // LB20.
        if a == CB || b == CB {
            return true;
        }
        // LB20a: a hyphen at the start of a word (after an opportunity).
        if matches!(a, HY | HH)
            && matches!(b, AL | HL)
            && before.is_none_or(|x| matches!(x, BK | CR | LF | NL | SP | ZW | CB | GL))
        {
            return false;
        }
        // LB21.
        if matches!(b, BA | HH | HY | NS) || a == BB {
            return false;
        }
        // LB21a.
        if matches!(a, HY | HH) && before == Some(HL) && b != HL {
            return false;
        }
        // LB21b.
        if a == SY && b == HL {
            return false;
        }
        // LB22.
        if b == IN {
            return false;
        }
        // LB23.
        if (matches!(a, AL | HL) && b == NU) || (a == NU && matches!(b, AL | HL)) {
            return false;
        }
        // LB23a.
        if (a == PR && matches!(b, ID | EB | EM)) || (matches!(a, ID | EB | EM) && b == PO) {
            return false;
        }
        // LB24.
        if (matches!(a, PR | PO) && matches!(b, AL | HL))
            || (matches!(a, AL | HL) && matches!(b, PR | PO))
        {
            return false;
        }
        // LB25.
        if self.in_number(i, c, a, b) {
            return false;
        }
        // LB26, LB27.
        let hangul = |x| matches!(x, JL | JV | JT | H2 | H3);
        if (a == JL && matches!(b, JL | JV | H2 | H3))
            || (matches!(a, JV | H2) && matches!(b, JV | JT))
            || (matches!(a, JT | H3) && b == JT)
            || (hangul(a) && b == PO)
            || (a == PR && hangul(b))
        {
            return false;
        }
        // LB28.
        if matches!(a, AL | HL) && matches!(b, AL | HL) {
            return false;
        }
        // LB28a.
        if (a == AP && next.aksara())
            || (prev.aksara() && matches!(b, VF | VI))
            || (a == VI
                && self.before.is_some_and(|u| u.aksara())
                && (b == Class::AK || next.base == DOTTED_CIRCLE))
            || (prev.aksara()
                && next.aksara()
                && self.following(i, c)[0].is_some_and(|n| n.class == VF))
        {
            return false;
        }
        // LB29.
        if a == IS && matches!(b, AL | HL) {
            return false;
        }
        // LB30.
        if (matches!(a, AL | HL | NU) && b == OP && !next.east_asian())
            || (a == CP && !prev.east_asian() && matches!(b, AL | HL | NU))
        {
            return false;
        }
        // LB30a.
        if a == RI && b == RI && self.ri_odd {
            return false;
        }
        // LB30b.
        if b == EM && (a == EB || is_unassigned_pictographic(prev.base)) {
            return false;
        }
        // LB31.
        true
    }

    /// LB25: true if the position is inside a number.
    fn in_number(&self, i: usize, c: char, before: Class, after: Class) -> bool {
        use Class::{HY, IS, NU, OP, PO, PR};
        match (before, after) {
            (_, PO | PR) if self.number != Number::None => true,
            (PO | PR | HY | IS, NU) => true,
            (_, NU) if self.number == Number::Digits => true,
            (PO | PR, OP) => match self.following(i, c) {
                [Some(first), second] => {
                    first.class == NU
                        || (first.class == IS && second.is_some_and(|u| u.class == NU))
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// The next two units after the unit that starts with `c` at byte `i`.
    /// Called only for the rules that look ahead, at the start of a unit, so
    /// each code point is read a bounded number of times.
    fn following(&self, i: usize, c: char) -> [Option<Unit>; 2] {
        let mut out = [None, None];
        let mut current = Unit::new(c).class;
        let mut count = 0;
        for ch in self.text.get(i + c.len_utf8()..).unwrap_or("").chars() {
            if count == out.len() {
                break;
            }
            if attaches(ch, current) {
                continue;
            }
            let unit = Unit::new(ch);
            current = unit.class;
            out[count] = Some(unit);
            count += 1;
        }
        out
    }
}

/// The LB25 state after a unit of class `class`, from the state before.
fn next_number(state: Number, class: Class) -> Number {
    match class {
        Class::NU => Number::Digits,
        Class::SY | Class::IS if state == Number::Digits => Number::Digits,
        Class::CL | Class::CP if state == Number::Digits => Number::Closed,
        _ => Number::None,
    }
}

/// The group of `c` in the Latin-1 pair tables: U+0020..U+00FF, and C0
/// controls (which break like U+0080).
fn latin1_group(c: char) -> Option<usize> {
    let cp = u32::from(c);
    let index = match cp {
        0x20..=0xFF => cp - 0x20,
        0..0x20 => 0x80 - 0x20,
        _ => return None,
    };
    tables::LATIN1_GROUPS
        .get(index as usize)
        .map(|&g| usize::from(g))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn breaks(text: &str) -> Vec<(usize, BreakKind)> {
        line_breaks(text, |_| BreakRules::default())
    }

    fn allowed(text: &str) -> Vec<usize> {
        breaks(text).into_iter().map(|(i, _)| i).collect()
    }

    fn with(text: &str, word_break: WordBreak) -> Vec<usize> {
        let rules = BreakRules {
            word_break,
            soft_hyphens: true,
        };
        line_breaks(text, |_| rules)
            .into_iter()
            .map(|(i, _)| i)
            .collect()
    }

    #[test]
    fn spaces() {
        assert_eq!(
            breaks("Hello big world"),
            vec![(6, BreakKind::Allowed), (10, BreakKind::Allowed)]
        );
        // After a run of spaces, whatever follows (UAX #14 forbids the
        // break before `)`, `!` and `;`; measured in Chromium 148: the
        // break is allowed).
        assert_eq!(allowed("a  )"), vec![3]);
        assert_eq!(allowed("( a"), vec![2]);
        assert_eq!(allowed("a ;b"), vec![2]);
        // No break before a space.
        assert_eq!(allowed("a\u{A0} b"), vec![4]);
        // Tabs are spaces; U+3000 after a space does not break.
        assert_eq!(allowed("a\t)"), vec![2]);
        assert_eq!(allowed("a \u{3000}b"), vec![5]);
        // After spaces and U+3000 the line can break before anything, also
        // before a zero-width space; U+3000 alone does not allow that.
        assert_eq!(allowed("a \u{3000}\u{200B}b"), vec![5, 8]);
        assert_eq!(allowed("a\u{3000}\u{200B}b"), vec![7]);
    }

    #[test]
    fn hyphens() {
        assert_eq!(allowed("well-known"), vec![5]);
        // No break before a hyphen or after a leading one.
        assert_eq!(allowed("-5"), vec![]);
        assert_eq!(allowed("a -5"), vec![2]);
        // A hyphen before a digit breaks after a letter or digit only.
        assert_eq!(allowed("978-1-4503-6438-6"), vec![4, 6, 11, 16]);
        assert_eq!(allowed("ABCD-1234"), vec![5]);
        assert_eq!(allowed("(-1)"), vec![]);
        assert_eq!(allowed("a--b"), vec![2, 3]);
        assert_eq!(allowed("a-$"), vec![]);
        // Only after an ASCII letter or digit.
        assert_eq!(allowed("\u{430}-0"), vec![]);
    }

    #[test]
    fn urls_break_only_after_hyphens_and_question_marks() {
        assert_eq!(allowed("https://en.wikipedia.org/wiki/Web_browser"), vec![]);
        assert_eq!(allowed("example.com/a-b?q=1"), vec![14, 16]);
        assert_eq!(allowed("10.1145/3240431.3240443"), vec![]);
    }

    #[test]
    fn ascii_punctuation() {
        // Before an opening bracket after punctuation, not after a letter.
        assert_eq!(allowed("a.(b"), vec![2]);
        assert_eq!(allowed("\"(b"), vec![1]);
        assert_eq!(allowed("a(b"), vec![]);
        // No break after `]`, `}`, `!` or `|` (UAX #14 has one).
        assert_eq!(allowed("a]b}c!d|e"), vec![]);
        assert_eq!(allowed("?\"x"), vec![]);
        assert_eq!(allowed("a?b"), vec![2]);
    }

    #[test]
    fn latin1_pairs_use_uax14() {
        // U+00A0 NO-BREAK SPACE glues; U+00AD SOFT HYPHEN breaks after.
        assert_eq!(allowed("a\u{A0}b"), vec![]);
        assert_eq!(allowed("a\u{AD}b"), vec![3]);
        assert_eq!(allowed("é(b"), vec![]);
        // A Latin-1 character after a hyphen: UAX #14 with context.
        assert_eq!(allowed("x-é"), vec![2]);
        // The hyphen is word-initial after an opportunity (LB20a).
        assert_eq!(allowed("x?-é"), vec![2]);
        assert_eq!(allowed("x(-é"), vec![3]);
    }

    #[test]
    fn newer_unicode_rules() {
        // LB20a: no break after a word-initial hyphen.
        assert_eq!(allowed("-é"), vec![]);
        assert_eq!(allowed("a \u{2010}b"), vec![2]);
        assert_eq!(allowed("a\u{2010}b"), vec![4]);
        // LB30: a letter before a fullwidth opening parenthesis.
        assert_eq!(allowed("a\u{FF08}b"), vec![1]);
        // LB21a: no break after a hyphen between Hebrew and other letters.
        assert_eq!(allowed("\u{5D0}-\u{430}"), vec![]);
        // LB28a: Balinese orthographic syllables.
        assert_eq!(allowed("\u{1B05}\u{1B44}\u{1B13}\u{1B05}"), vec![9]);
    }

    #[test]
    fn soft_hyphens_can_be_disabled() {
        let rules = BreakRules {
            word_break: WordBreak::Normal,
            soft_hyphens: false,
        };
        assert_eq!(line_breaks("a\u{AD}b", |_| rules).len(), 0);
        assert_eq!(line_breaks("\u{3042}\u{AD}b", |_| rules).len(), 0);
        // With `break-all`, the breaks between letters remain.
        let rules = BreakRules {
            word_break: WordBreak::BreakAll,
            soft_hyphens: false,
        };
        let found: Vec<usize> = line_breaks("x\u{AD}a x\u{AD}%", |_| rules)
            .into_iter()
            .map(|(i, _)| i)
            .collect();
        assert_eq!(found, vec![1, 3, 5, 6]);
    }

    #[test]
    fn other_characters_use_uax14() {
        // En dash (HH): after, not before; em dash (B2): both sides.
        assert_eq!(allowed("64\u{2013}71"), vec![5]);
        assert_eq!(allowed("a\u{2014}b"), vec![1, 4]);
        // Quotation marks stay with their words.
        assert_eq!(allowed("\u{201C}a\u{201D}."), vec![]);
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
        // Small kana (CJ) break like ideographs.
        assert_eq!(allowed("\u{3042}\u{3041}"), vec![3]);
    }

    #[test]
    fn no_break_before_closing_punctuation() {
        assert_eq!(breaks("文。"), vec![]);
    }

    #[test]
    fn other_hard_line_breaks() {
        // As measured in Chromium 148: a break after them, not forced.
        assert_eq!(allowed("a\u{85}b"), vec![3]);
        assert_eq!(allowed("a\u{2028}b\u{2029}c"), vec![4, 8]);
        assert_eq!(allowed("a\u{2028} b"), vec![5]);
        // No break around U+000B and U+000C.
        assert_eq!(allowed("a\u{0B}b\u{0C}c"), vec![]);
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
        // Thai has no dictionary: breaks only at spaces.
        assert_eq!(allowed("\u{E01}\u{E32}\u{E23} \u{E1A}"), vec![10]);
    }

    #[test]
    fn break_all() {
        assert_eq!(with("abc", WordBreak::BreakAll), vec![1, 2]);
        // Before a hyphen and BA characters too, but not around HH (an en
        // dash).
        assert_eq!(with("a-b", WordBreak::BreakAll), vec![1, 2]);
        assert_eq!(with("a|b", WordBreak::BreakAll), vec![1, 2]);
        assert_eq!(with("a\u{2013}b", WordBreak::BreakAll), vec![4]);
        assert_eq!(with("1\u{2013}-2", WordBreak::BreakAll), vec![5]);
        // Not before closing punctuation; combining marks stay.
        assert_eq!(with("a)", WordBreak::BreakAll), vec![]);
        assert_eq!(with("e\u{301}x", WordBreak::BreakAll), vec![3]);
        assert_eq!(with("a+b", WordBreak::BreakAll), vec![1, 2]);
        // Thai tone marks are letters.
        assert_eq!(with("\u{E1A}\u{E49}", WordBreak::BreakAll), vec![3]);
        // A word-initial hyphen (after the break that break-all adds).
        assert_eq!(with("a-\u{E9}", WordBreak::BreakAll), vec![1]);
    }

    #[test]
    fn keep_all() {
        assert_eq!(with("漢字です", WordBreak::KeepAll), vec![]);
        assert_eq!(with("漢字 です", WordBreak::KeepAll), vec![7]);
        // Punctuation still breaks as usual.
        assert_eq!(with("字、字", WordBreak::KeepAll), vec![6]);
        // Symbols are not letters.
        assert_eq!(with("\u{4E00}\u{1F600}", WordBreak::KeepAll), vec![3]);
    }

    #[test]
    fn rules_apply_per_position() {
        let rules = |i: usize| BreakRules {
            word_break: if i < 3 {
                WordBreak::BreakAll
            } else {
                WordBreak::Normal
            },
            soft_hyphens: true,
        };
        let found: Vec<usize> = line_breaks("abcdef", rules)
            .into_iter()
            .map(|(i, _)| i)
            .collect();
        assert_eq!(found, vec![1, 2]);
    }

    #[test]
    fn hostile_input_is_linear() {
        // Long runs that the look-ahead and look-behind rules see.
        let marks = format!("\u{201D}{}x", "\u{301}".repeat(200_000));
        assert_eq!(breaks(&marks).len(), 0);
        let spaces = format!("a{}b", " ".repeat(1_000_000));
        assert_eq!(allowed(&spaces), vec![1_000_001]);
        let mixed =
            "$(1) \u{201C}a\u{201D} \u{1F1E6}\u{1F1E6}\u{1F1E6} \u{2014}\u{2014} ".repeat(20_000);
        // Five breaks in each repetition; none after the last space.
        assert_eq!(breaks(&mixed).len(), 20_000 * 5 - 1);
    }
}
