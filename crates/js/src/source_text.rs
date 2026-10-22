//! The kept source text of a script (M7 13e, ADR 0026 section 3): one
//! byte per code unit, with the few wide units of a mostly Latin-1
//! script in a sorted list.

use swb_js_text::{Str16, String16};

/// The text of a script as the functions keep it. Source text is mostly
/// Latin-1: one byte per unit. A script with a few units of 256 or above
/// (a typographic quote in a string) keeps its other units in bytes and
/// those few in a sorted list, instead of two bytes for every unit; a
/// script that is mostly wide keeps two bytes per unit.
#[derive(Debug)]
pub(crate) enum SourceText {
    /// All units in one width.
    Flat(String16),
    /// Units below 256 in `narrow` (a placeholder at the wide ones) and
    /// the wide ones as (offset, unit), sorted by offset.
    Sparse {
        narrow: Box<[u8]>,
        wide: Box<[(u32, u16)]>,
    },
}

impl SourceText {
    /// Keeps a copy of `text` in the smallest form.
    pub(crate) fn new(text: Str16<'_>) -> Self {
        let Str16::Wide(units) = text else {
            return SourceText::Flat(text.to_string16());
        };
        let wide_count = units.iter().filter(|&&u| u >= 256).count();
        // Sparse: 1 byte per unit and 8 per wide unit; flat wide: 2 per unit.
        let sparse = wide_count > 0
            && wide_count.saturating_mul(8) < units.len()
            && u32::try_from(units.len()).is_ok();
        if !sparse {
            return SourceText::Flat(text.to_string16());
        }
        let narrow = units
            .iter()
            .map(|&u| u8::try_from(u).unwrap_or(b'?'))
            .collect();
        let wide = units
            .iter()
            .enumerate()
            .filter(|&(_, &u)| u >= 256)
            .map(|(i, &u)| (i as u32, u))
            .collect();
        SourceText::Sparse { narrow, wide }
    }

    /// The length in code units.
    pub(crate) fn len(&self) -> usize {
        match self {
            SourceText::Flat(text) => text.len(),
            SourceText::Sparse { narrow, .. } => narrow.len(),
        }
    }

    /// The bytes that the text occupies.
    pub(crate) fn byte_size(&self) -> usize {
        match self {
            SourceText::Flat(text) => text.len() * if text.is_latin1() { 1 } else { 2 },
            SourceText::Sparse { narrow, wide } => narrow.len() + wide.len() * 8,
        }
    }

    /// The units `start..end` as an owned text, `None` when out of range.
    pub(crate) fn slice(&self, start: usize, end: usize) -> Option<String16> {
        match self {
            SourceText::Flat(text) => text.as_str16().slice(start, end).map(Str16::to_string16),
            SourceText::Sparse { narrow, wide } => {
                let part = narrow.get(start..end)?;
                let mut units: Vec<u16> = part.iter().map(|&b| u16::from(b)).collect();
                let first = wide.partition_point(|&(o, _)| (o as usize) < start);
                for &(offset, unit) in wide.get(first..)? {
                    let Some(slot) = (offset as usize)
                        .checked_sub(start)
                        .and_then(|i| units.get_mut(i))
                    else {
                        break;
                    };
                    *slot = unit;
                }
                Some(String16::from_units(&units))
            }
        }
    }

    /// Calls `f` with the whole text as one string (for the line index of
    /// an error). Only the sparse form makes a copy.
    pub(crate) fn with_whole<R>(&self, f: impl FnOnce(Str16<'_>) -> R) -> R {
        match self {
            SourceText::Flat(text) => f(text.as_str16()),
            SourceText::Sparse { .. } => {
                f(self.slice(0, self.len()).unwrap_or_default().as_str16())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_text_keeps_one_byte_per_unit_with_few_wide_units() {
        let mut units: Vec<u16> = (0..200u16).map(|i| 32 + i % 90).collect();
        units[10] = 0x2019;
        units[11] = 0xD83D;
        units[199] = 0x2028;
        let text = SourceText::new(Str16::Wide(&units));
        assert!(matches!(text, SourceText::Sparse { .. }));
        assert_eq!(text.len(), 200);
        assert_eq!(text.byte_size(), 200 + 3 * 8);
        let whole = String16::from_units(&units);
        assert!(text.with_whole(|s| s.to_string16() == whole));
        for (start, end) in [(0, 200), (10, 12), (11, 12), (12, 12), (5, 11), (199, 200)] {
            let expected = Str16::Wide(&units)
                .slice(start, end)
                .map(Str16::to_string16);
            assert_eq!(text.slice(start, end), expected, "{start}..{end}");
        }
        assert_eq!(text.slice(0, 201), None);
        assert_eq!(text.slice(5, 4), None);
        // Mostly wide text stays flat; narrow text is one byte per unit.
        let wide: Vec<u16> = (0..100).map(|i| 0x4E00 + i).collect();
        let flat = SourceText::new(Str16::Wide(&wide));
        assert!(matches!(flat, SourceText::Flat(_)));
        assert_eq!(flat.byte_size(), 200);
        let narrow = SourceText::new(Str16::Wide(&[65, 66, 233]));
        assert_eq!(narrow.byte_size(), 3);
    }
}
