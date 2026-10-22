//! Unicode normalization for `String.prototype.normalize` (ECMA-262
//! §22.1.3.15) after UAX #15 (<https://www.unicode.org/reports/tr41/tr15>),
//! and the Hangul syllable algorithm of Unicode Standard §3.12.
//!
//! - Decomposition: every code point maps to its full canonical (NFD, NFC)
//!   or compatibility (NFKD, NFKC) decomposition, which the generated
//!   tables hold fully expanded, so no step recurses. Hangul syllables are
//!   decomposed by the algorithm.
//! - Canonical ordering (UAX #15 §1.2, D109): each maximal run of
//!   non-starters is sorted by `Canonical_Combining_Class` with a stable
//!   sort, `O(n log n)` (`O(n)` for a sorted run), not by insertion.
//! - Canonical composition (UAX #15 §1.3, D117 and following): the
//!   starter is the last starter that was not blocked; each following
//!   character composes with it if no character of the same or a higher
//!   class stands between them and the pair has a primary composite
//!   (`COMPOSE`: pairs not in `Full_Composition_Exclusion`; Hangul by the
//!   algorithm). One pass, one binary search for each character.
//! - Quick check (UAX #15 §9.1, [`is_normalized`]): a text with only
//!   `Yes` values and correct canonical order is normalized, so the caller
//!   returns the same string. Latin-1 text in NFC needs no table.
//!
//! [`Normalizer`] works on slices of a text so that the caller can charge
//! time, check the length and the heap between them.

use super::in_ranges;
use super::norm_tables as t;
use crate::{Str16, String16};

const S_BASE: u32 = 0xAC00;
const L_BASE: u32 = 0x1100;
const V_BASE: u32 = 0x1161;
const T_BASE: u32 = 0x11A7;
const L_COUNT: u32 = 19;
const V_COUNT: u32 = 21;
const T_COUNT: u32 = 28;
const N_COUNT: u32 = V_COUNT * T_COUNT;
const S_COUNT: u32 = L_COUNT * N_COUNT;

/// A normalization form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    /// Canonical decomposition, then canonical composition.
    Nfc,
    /// Canonical decomposition.
    Nfd,
    /// Compatibility decomposition, then canonical composition.
    Nfkc,
    /// Compatibility decomposition.
    Nfkd,
}

impl Form {
    /// The form named by `name` (`"NFC"`, `"NFD"`, `"NFKC"`, `"NFKD"`).
    pub fn from_name(name: Str16<'_>) -> Option<Form> {
        [
            ("NFC", Form::Nfc),
            ("NFD", Form::Nfd),
            ("NFKC", Form::Nfkc),
            ("NFKD", Form::Nfkd),
        ]
        .into_iter()
        .find(|(candidate, _)| name.eq_str(candidate))
        .map(|(_, form)| form)
    }

    fn compatibility(self) -> bool {
        matches!(self, Form::Nfkc | Form::Nfkd)
    }

    fn composing(self) -> bool {
        matches!(self, Form::Nfc | Form::Nfkc)
    }
}

/// The `Canonical_Combining_Class` of `cp`.
pub(crate) fn combining_class(cp: u32) -> u8 {
    if cp < 0x300 {
        return 0;
    }
    t::CCC
        .binary_search_by(|&(first, last, _)| {
            if last < cp {
                std::cmp::Ordering::Less
            } else if first > cp {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .ok()
        .and_then(|index| t::CCC.get(index))
        .map_or(0, |entry| entry.2)
}

fn is_hangul_syllable(cp: u32) -> bool {
    (S_BASE..S_BASE + S_COUNT).contains(&cp)
}

/// The decomposition of `cp` from a pool, if it has one.
fn pooled(index: &[(u32, u16, u8)], data: &'static [u32], cp: u32) -> Option<&'static [u32]> {
    let at = index.binary_search_by_key(&cp, |entry| entry.0).ok()?;
    let &(_, offset, len) = index.get(at)?;
    let start = usize::from(offset);
    data.get(start..start + usize::from(len))
}

/// The full decomposition of `cp` in the form's decomposition type
/// (without Hangul syllables).
fn decomposition(cp: u32, compat: bool) -> Option<&'static [u32]> {
    if cp < 0xA0 {
        return None;
    }
    if compat && let Some(found) = pooled(&t::COMPAT_INDEX, &t::COMPAT_DATA, cp) {
        return Some(found);
    }
    pooled(&t::CANON_INDEX, &t::CANON_DATA, cp)
}

/// The primary composite of `first` and `second`, if there is one.
fn compose(first: u32, second: u32) -> Option<u32> {
    if (L_BASE..L_BASE + L_COUNT).contains(&first) && (V_BASE..V_BASE + V_COUNT).contains(&second) {
        return Some(S_BASE + ((first - L_BASE) * V_COUNT + (second - V_BASE)) * T_COUNT);
    }
    if is_hangul_syllable(first)
        && (first - S_BASE).is_multiple_of(T_COUNT)
        && (T_BASE + 1..T_BASE + T_COUNT).contains(&second)
    {
        return Some(first + (second - T_BASE));
    }
    let at = t::COMPOSE
        .binary_search_by_key(&(first, second), |entry| (entry.0, entry.1))
        .ok()?;
    t::COMPOSE.get(at).map(|entry| entry.2)
}

/// Whether the text is known to be in the form. `false` only means that
/// the text may have to change; normalizing it can give the same text.
/// UAX #15 §9.1 "Detecting Normalization Forms": every character has the
/// quick check value Yes and the classes are in canonical order.
pub fn is_normalized(text: Str16<'_>, form: Form) -> bool {
    let no: &[(u32, u32)] = match form {
        Form::Nfc => &t::NFC_NO,
        Form::Nfd => &t::NFD_NO,
        Form::Nfkc => &t::NFKC_NO,
        Form::Nfkd => &t::NFKD_NO,
    };
    match text {
        // No Latin-1 character is a non-starter or Maybe, and none is No in NFC.
        Str16::Latin1(_) if form == Form::Nfc => true,
        Str16::Latin1(bytes) => bytes
            .iter()
            .all(|&b| b < 0xA0 || !in_ranges(no, u32::from(b))),
        Str16::Wide(_) => {
            let mut last = 0u8;
            for cp in text.code_points() {
                if cp < 0x80 {
                    last = 0;
                    continue;
                }
                let class = combining_class(cp);
                if (class != 0 && last > class)
                    || in_ranges(no, cp)
                    || (form.composing() && in_ranges(&t::NFC_MAYBE, cp))
                {
                    return false;
                }
                last = class;
            }
            true
        }
    }
}

/// The state of a normalization that reads a text in slices.
pub struct Normalizer {
    form: Form,
    /// The non-starters since the last starter, with their classes,
    /// waiting for the canonical ordering.
    run: Vec<(u8, u32)>,
    /// Composition: the starter that later characters may compose with.
    starter: Option<u32>,
    /// Composition: the characters after the starter that did not
    /// compose with it, in canonical order.
    held: Vec<u32>,
    /// Composition: the class of the last held character.
    last_class: u8,
}

impl Normalizer {
    /// A normalization to `form`.
    pub fn new(form: Form) -> Self {
        Normalizer {
            form,
            run: Vec::new(),
            starter: None,
            held: Vec::new(),
            last_class: 0,
        }
    }

    /// The number of code points that are held back and will be written
    /// later (for the caller's limit and memory checks).
    pub fn in_flight(&self) -> usize {
        self.run.len() + self.held.len() + 1
    }

    /// Reads the code points of `text` that start at unit `from` (a code
    /// point boundary) until the unit `limit` is reached or passed, and
    /// appends the output that is ready to `out`. Returns the unit where
    /// it stopped. Each code point of the input gives at most
    /// [`t::MAX_DECOMPOSITION`] code points (the output of earlier slices
    /// that was held back comes on top).
    pub fn feed(
        &mut self,
        text: Str16<'_>,
        from: usize,
        limit: usize,
        out: &mut String16,
    ) -> usize {
        let limit = limit.min(text.len());
        let mut at = from;
        while at < limit {
            let Some((cp, next)) = text.code_point_at(at) else {
                break;
            };
            at = next;
            self.add(cp, out);
        }
        at.max(from)
    }

    /// Writes everything that is held back. Call it once, after the last slice.
    pub fn finish(&mut self, out: &mut String16) {
        self.flush_run(out);
        self.flush_starter(out);
    }

    /// Decomposes one code point and passes the parts on.
    fn add(&mut self, cp: u32, out: &mut String16) {
        if cp < 0xA0 {
            self.add_decomposed(cp, 0, out);
        } else if is_hangul_syllable(cp) {
            let index = cp - S_BASE;
            self.add_decomposed(L_BASE + index / N_COUNT, 0, out);
            self.add_decomposed(V_BASE + (index % N_COUNT) / T_COUNT, 0, out);
            let trail = index % T_COUNT;
            if trail != 0 {
                self.add_decomposed(T_BASE + trail, 0, out);
            }
        } else if let Some(parts) = decomposition(cp, self.form.compatibility()) {
            for &part in parts {
                self.add_decomposed(part, combining_class(part), out);
            }
        } else {
            self.add_decomposed(cp, combining_class(cp), out);
        }
    }

    /// Canonical ordering: a non-starter joins the run, a starter ends it.
    fn add_decomposed(&mut self, cp: u32, class: u8, out: &mut String16) {
        if class != 0 {
            self.run.push((class, cp));
            return;
        }
        self.flush_run(out);
        self.add_starter(cp, out);
    }

    /// Sorts the run by class (stable) and passes it on.
    fn flush_run(&mut self, out: &mut String16) {
        if self.run.is_empty() {
            return;
        }
        self.run.sort_by_key(|entry| entry.0);
        let run = std::mem::take(&mut self.run);
        for &(class, cp) in &run {
            self.add_mark(class, cp, out);
        }
        // Keeps the allocation for the next run.
        self.run = run;
        self.run.clear();
    }

    fn add_starter(&mut self, cp: u32, out: &mut String16) {
        if !self.form.composing() {
            out.push_code_point(cp);
            return;
        }
        if let Some(starter) = self.starter {
            if self.held.is_empty()
                && let Some(composite) = compose(starter, cp)
            {
                self.starter = Some(composite);
                return;
            }
            self.flush_starter(out);
        }
        self.starter = Some(cp);
        self.last_class = 0;
    }

    fn add_mark(&mut self, class: u8, cp: u32, out: &mut String16) {
        if !self.form.composing() {
            out.push_code_point(cp);
            return;
        }
        let Some(starter) = self.starter else {
            out.push_code_point(cp);
            return;
        };
        if (self.held.is_empty() || self.last_class < class)
            && let Some(composite) = compose(starter, cp)
        {
            self.starter = Some(composite);
            return;
        }
        self.held.push(cp);
        self.last_class = class;
    }

    /// Writes the starter and the characters held after it.
    fn flush_starter(&mut self, out: &mut String16) {
        if let Some(starter) = self.starter.take() {
            out.push_code_point(starter);
        }
        for &cp in &self.held {
            out.push_code_point(cp);
        }
        self.held.clear();
        self.last_class = 0;
    }
}

/// The longest decomposition of one code point.
pub const MAX_DECOMPOSITION: usize = t::MAX_DECOMPOSITION;

/// Normalizes a whole text (for tests and small inputs; the engine goes
/// through [`Normalizer`] in slices).
pub fn normalize(text: Str16<'_>, form: Form) -> String16 {
    let mut out = String16::new();
    let mut normalizer = Normalizer::new(form);
    normalizer.feed(text, 0, usize::MAX, &mut out);
    normalizer.finish(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(s: &[u16], form: Form) -> Vec<u16> {
        normalize(Str16::Wide(s), form).as_str16().units().collect()
    }

    fn run(s: &str, form: Form) -> String {
        let units: Vec<u16> = s.encode_utf16().collect();
        String::from_utf16_lossy(&norm(&units, form))
    }

    #[test]
    fn composition_and_decomposition() {
        assert_eq!(run("e\u{301}", Form::Nfc), "\u{E9}");
        assert_eq!(run("\u{E9}", Form::Nfd), "e\u{301}");
        assert_eq!(run("\u{1E9B}\u{323}", Form::Nfc), "\u{1E9B}\u{323}");
        assert_eq!(run("\u{1E9B}\u{323}", Form::Nfd), "\u{17F}\u{323}\u{307}");
        assert_eq!(run("\u{1E9B}\u{323}", Form::Nfkc), "\u{1E69}");
        assert_eq!(run("\u{1E9B}\u{323}", Form::Nfkd), "s\u{323}\u{307}");
        assert_eq!(run("\u{FB01}", Form::Nfkc), "fi");
        assert_eq!(run("\u{FB01}", Form::Nfc), "\u{FB01}");
        // A singleton decomposes and does not compose back.
        assert_eq!(run("\u{212B}", Form::Nfc), "\u{C5}");
        assert_eq!(run("\u{2126}", Form::Nfc), "\u{3A9}");
        // Two marks that compose in two steps.
        assert_eq!(run("a\u{30A}\u{301}", Form::Nfc), "\u{1FB}");
        assert_eq!(run("\u{1FB}", Form::Nfd), "a\u{30A}\u{301}");
    }

    #[test]
    fn canonical_ordering_and_blocking() {
        // 0323 (class 220) sorts before 0307 (230).
        assert_eq!(run("a\u{307}\u{323}", Form::Nfd), "a\u{323}\u{307}");
        // Marks of the same class keep their order.
        assert_eq!(run("a\u{301}\u{300}", Form::Nfd), "a\u{301}\u{300}");
        // Blocked: the same class stands between.
        assert_eq!(run("a\u{300}\u{300}", Form::Nfc), "\u{E0}\u{300}");
        // Not blocked: a lower class between.
        assert_eq!(run("a\u{323}\u{301}", Form::Nfc), "\u{1EA1}\u{301}");
        assert_eq!(run("a\u{301}\u{323}", Form::Nfc), "\u{1EA1}\u{301}");
        // A starter between blocks.
        assert_eq!(
            run("\u{1100}\u{300}\u{1161}", Form::Nfc),
            "\u{1100}\u{300}\u{1161}"
        );
        // U+0344 decomposes into two marks.
        assert_eq!(run("\u{344}", Form::Nfc), "\u{308}\u{301}");
        // Marks at the start have no starter.
        assert_eq!(run("\u{301}\u{300}a", Form::Nfc), "\u{301}\u{300}a");
    }

    #[test]
    fn hangul() {
        assert_eq!(run("\u{AC01}", Form::Nfd), "\u{1100}\u{1161}\u{11A8}");
        assert_eq!(run("\u{1100}\u{1161}\u{11A8}", Form::Nfc), "\u{AC01}");
        assert_eq!(run("\u{1100}\u{1161}", Form::Nfc), "\u{AC00}");
        assert_eq!(run("\u{AC00}\u{11A8}", Form::Nfc), "\u{AC01}");
        // T_BASE itself is not a trailing consonant; LVT does not take another T.
        assert_eq!(run("\u{AC00}\u{11A7}", Form::Nfc), "\u{AC00}\u{11A7}");
        assert_eq!(run("\u{AC01}\u{11A8}", Form::Nfc), "\u{AC01}\u{11A8}");
        assert_eq!(run("\u{D7A3}", Form::Nfd), "\u{1112}\u{1175}\u{11C2}");
        assert_eq!(run("\u{D7A4}", Form::Nfd), "\u{D7A4}");
    }

    #[test]
    fn largest_expansion_and_odd_input() {
        assert_eq!(norm(&[0xFDFA], Form::Nfkd).len(), 18);
        assert_eq!(MAX_DECOMPOSITION, 18);
        // Lone surrogates and unassigned code points are kept.
        assert_eq!(
            norm(&[0xD800, 0x41, 0xDC00], Form::Nfc),
            [0xD800, 0x41, 0xDC00]
        );
        assert_eq!(run("\u{378}\u{10FFFF}", Form::Nfkd), "\u{378}\u{10FFFF}");
        // Supplementary: U+1D15E decomposes to U+1D157 U+1D165 (composition excluded).
        assert_eq!(run("\u{1D15E}", Form::Nfc), "\u{1D157}\u{1D165}");
    }

    #[test]
    fn quick_check() {
        assert!(is_normalized(Str16::Latin1(b"abc \xE9"), Form::Nfc));
        assert!(!is_normalized(Str16::Latin1(b"abc \xE9"), Form::Nfd));
        assert!(is_normalized(Str16::Latin1(b"abc"), Form::Nfkd));
        assert!(!is_normalized(Str16::Latin1(b"\xA0"), Form::Nfkc));
        assert!(is_normalized(Str16::Latin1(b"\xA0"), Form::Nfc));
        assert!(is_normalized(Str16::Wide(&[0x3B1, 0x3B2]), Form::Nfc));
        assert!(!is_normalized(Str16::Wide(&[0x65, 0x301]), Form::Nfc));
        assert!(is_normalized(Str16::Wide(&[0x65, 0x301]), Form::Nfd));
        assert!(!is_normalized(
            Str16::Wide(&[0x65, 0x301, 0x323]),
            Form::Nfd
        ));
        assert!(!is_normalized(Str16::Wide(&[0xAC00]), Form::Nfd));
        assert!(is_normalized(Str16::Wide(&[0xAC00]), Form::Nfc));
    }

    #[test]
    fn a_million_marks_in_reverse_order_are_sorted_in_n_log_n() {
        // Classes 230 and 220 alternate; 1M marks.
        let mut units = vec![0x61];
        for i in 0..1_000_000 {
            units.push(if i % 2 == 0 { 0x301 } else { 0x323 });
        }
        let started = std::time::Instant::now();
        let out = norm(&units, Form::Nfd);
        assert_eq!(out.len(), units.len());
        assert_eq!(out[1], 0x323);
        assert_eq!(out[out.len() - 1], 0x301);
        let out = norm(&units, Form::Nfc);
        assert_eq!(out[0], 0x1EA1);
        assert!(started.elapsed().as_secs() < 5);
    }

    #[test]
    fn tables_are_sorted() {
        for pair in t::CANON_INDEX.windows(2) {
            assert!(pair[0].0 < pair[1].0);
        }
        for pair in t::COMPAT_INDEX.windows(2) {
            assert!(pair[0].0 < pair[1].0);
        }
        for pair in t::COMPOSE.windows(2) {
            assert!((pair[0].0, pair[0].1) < (pair[1].0, pair[1].1));
        }
        for pair in t::CCC.windows(2) {
            assert!(pair[0].1 < pair[1].0);
        }
    }
}
