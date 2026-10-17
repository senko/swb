//! The code point sets of the Unicode properties that regular expressions
//! accept in `\p{...}` (ECMA-262 §22.2.2.9 `CompileToCharSet`): the sets of
//! a [`Property`] as sorted, disjoint, non-adjacent ranges, and the strings
//! of the properties of strings (`v` flag).
//!
//! The data is in `property_data.rs` (generated, UCD 17.0.0) as LEB128
//! numbers: for each range the gap from the previous range and its length
//! minus one; see `tools/swbtools/js_property_sets.py`. Each call decodes
//! the set it needs; the caller keeps the result if it needs it again.
//! `General_Category` groups (`L`, `LC`, `P`, ...) unite their values.
//! `ASCII`, `Any` and `Assigned` have no data and are computed.

use super::property::{self, Property};
use super::property_data as d;

/// The largest code point.
const MAX_CODE_POINT: u32 = 0x0010_FFFF;

/// Reads a LEB128 number; `None` at the end of the data or when it is too
/// long or does not fit in 32 bits.
fn leb128(data: &[u8], pos: &mut usize) -> Option<u32> {
    let mut value = 0u32;
    for shift in (0..35).step_by(7) {
        let byte = *data.get(*pos)?;
        *pos += 1;
        let bits = u32::from(byte & 0x7F);
        // `checked_shl` checks the shift count, not the lost bits: the
        // fifth byte may only have its four low bits set.
        if shift == 28 && bits > 0x0F {
            return None;
        }
        value |= bits << shift;
        if byte < 0x80 {
            return Some(value);
        }
    }
    None
}

/// The item `index` of an offset table: a slice of the data.
fn item(offsets: &[u32], index: usize) -> &'static [u8] {
    let start = offsets.get(index).copied();
    let end = offsets.get(index + 1).copied();
    match (start, end) {
        (Some(s), Some(e)) => d::DATA.get(s as usize..e as usize).unwrap_or(&[]),
        _ => &[],
    }
}

/// Decodes the ranges of a set.
fn decode_ranges(data: &[u8], out: &mut Vec<(u32, u32)>) {
    let mut pos = 0;
    let mut next_free = 0u32;
    while pos < data.len() {
        let (Some(gap), Some(length)) = (leb128(data, &mut pos), leb128(data, &mut pos)) else {
            return;
        };
        let first = next_free.saturating_add(gap);
        let last = first.saturating_add(length).min(MAX_CODE_POINT);
        out.push((first, last));
        next_free = last.saturating_add(1);
    }
}

/// Sorts ranges and merges overlapping and adjacent ones.
fn normalize(mut ranges: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    ranges.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::with_capacity(ranges.len());
    for (first, last) in ranges {
        match merged.last_mut() {
            Some(prev) if first <= prev.1.saturating_add(1) => prev.1 = prev.1.max(last),
            _ => merged.push((first, last)),
        }
    }
    merged.shrink_to_fit();
    merged
}

fn complement(ranges: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut out = Vec::with_capacity(ranges.len() + 1);
    let mut next = 0u32;
    for &(first, last) in ranges {
        if first > next {
            out.push((next, first - 1));
        }
        next = last.saturating_add(1);
    }
    if next <= MAX_CODE_POINT {
        out.push((next, MAX_CODE_POINT));
    }
    out
}

fn general_category(index: u8) -> Vec<(u32, u32)> {
    let mut ranges = Vec::new();
    for &leaf in d::GENERAL_CATEGORY_MEMBERS
        .get(usize::from(index))
        .copied()
        .unwrap_or(&[])
    {
        decode_ranges(
            item(&d::GENERAL_CATEGORY_OFFSETS, usize::from(leaf)),
            &mut ranges,
        );
    }
    normalize(ranges)
}

fn binary(index: u8) -> Vec<(u32, u32)> {
    match property::binary_name(index) {
        Some("ASCII") => vec![(0, 0x7F)],
        Some("Any") => vec![(0, MAX_CODE_POINT)],
        Some("Assigned") => {
            let unassigned =
                property::general_category("Cn").map_or_else(Vec::new, general_category);
            complement(&unassigned)
        }
        _ => {
            let mut ranges = Vec::new();
            decode_ranges(item(&d::BINARY_OFFSETS, usize::from(index)), &mut ranges);
            ranges
        }
    }
}

/// The data indices of a property of strings: `RGI_Emoji` is the union of
/// the others.
fn string_indices(index: u8, count: usize) -> std::ops::Range<usize> {
    if property::string_name(index) == Some("RGI_Emoji") {
        0..count
    } else {
        usize::from(index)..usize::from(index) + 1
    }
}

/// The single code points of a property of strings.
fn string_code_points(index: u8) -> Vec<(u32, u32)> {
    let count = d::STRING_CODE_POINTS_OFFSETS.len() - 1;
    let mut ranges = Vec::new();
    for i in string_indices(index, count) {
        decode_ranges(item(&d::STRING_CODE_POINTS_OFFSETS, i), &mut ranges);
    }
    normalize(ranges)
}

/// The code points of the set of a property: sorted, disjoint,
/// non-adjacent ranges `(first, last)`. For a property of strings, only the
/// strings of one code point (see [`strings`] for the others).
pub fn ranges(property: Property) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    match property {
        Property::GeneralCategory(i) => return general_category(i),
        Property::Script(i) => decode_ranges(item(&d::SCRIPT_OFFSETS, usize::from(i)), &mut out),
        Property::ScriptExtensions(i) => decode_ranges(
            item(&d::SCRIPT_EXTENSIONS_OFFSETS, usize::from(i)),
            &mut out,
        ),
        Property::Binary(i) => return binary(i),
        Property::Strings(i) => return string_code_points(i),
    }
    out
}

/// The strings of a property of strings that have more than one code
/// point, as code points, sorted. Other properties have none.
pub fn strings(property: Property) -> Vec<Vec<u32>> {
    let Property::Strings(index) = property else {
        return Vec::new();
    };
    let count = d::STRING_SEQUENCES_OFFSETS.len() - 1;
    let mut out = Vec::new();
    for i in string_indices(index, count) {
        let data = item(&d::STRING_SEQUENCES_OFFSETS, i);
        let mut pos = 0;
        while let Some(&length) = data.get(pos) {
            pos += 1;
            let mut sequence = Vec::with_capacity(usize::from(length));
            for _ in 0..length {
                match leb128(data, &mut pos) {
                    Some(cp) => sequence.push(cp),
                    None => break,
                }
            }
            out.push(sequence);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(name: &str, value: Option<&str>) -> Vec<(u32, u32)> {
        ranges(property::lookup(name, value).expect("a property"))
    }

    fn contains(ranges: &[(u32, u32)], cp: u32) -> bool {
        let i = ranges.partition_point(|r| r.1 < cp);
        ranges.get(i).is_some_and(|r| r.0 <= cp)
    }

    fn count(ranges: &[(u32, u32)]) -> u32 {
        ranges.iter().map(|&(a, b)| b - a + 1).sum()
    }

    fn well_formed(ranges: &[(u32, u32)]) -> bool {
        ranges.iter().all(|r| r.0 <= r.1 && r.1 <= MAX_CODE_POINT)
            && ranges.windows(2).all(|w| w[0].1 + 1 < w[1].0)
    }

    #[test]
    fn every_set_is_well_formed() {
        for i in 0..38 {
            assert!(well_formed(&ranges(Property::GeneralCategory(i))), "gc {i}");
        }
        for i in 0..176 {
            assert!(well_formed(&ranges(Property::Script(i))), "sc {i}");
            assert!(
                well_formed(&ranges(Property::ScriptExtensions(i))),
                "scx {i}"
            );
        }
        for i in 0..53 {
            assert!(well_formed(&ranges(Property::Binary(i))), "binary {i}");
        }
        for i in 0..7 {
            assert!(well_formed(&ranges(Property::Strings(i))), "strings {i}");
        }
    }

    #[test]
    fn general_categories() {
        let lu = set("Lu", None);
        assert!(contains(&lu, 0x41) && !contains(&lu, 0x61) && contains(&lu, 0x10400));
        assert!(contains(&set("Ll", None), 0x61));
        // Groups unite their values.
        let letters = set("L", None);
        for cp in [0x41, 0x61, 0x1C5, 0x2B0, 0x5D0] {
            assert!(contains(&letters, cp), "{cp:#x}");
        }
        assert!(!contains(&letters, 0x30));
        let cased = set("LC", None);
        assert!(contains(&cased, 0x1C5) && !contains(&cased, 0x2B0) && !contains(&cased, 0x5D0));
        // The values partition the code points.
        let leaves = [
            "Cc", "Cf", "Cn", "Co", "Cs", "Ll", "Lm", "Lo", "Lt", "Lu", "Mc", "Me", "Mn", "Nd",
            "Nl", "No", "Pc", "Pd", "Pe", "Pf", "Pi", "Po", "Ps", "Sc", "Sk", "Sm", "So", "Zl",
            "Zp", "Zs",
        ];
        let total: u32 = leaves.iter().map(|n| count(&set(n, None))).sum();
        assert_eq!(total, MAX_CODE_POINT + 1);
        assert_eq!(count(&set("Cs", None)), 0x800);
        assert!(contains(&set("Cn", None), 0x378));
        assert!(contains(&set("Zs", None), 0x3000));
        assert_eq!(set("gc", Some("Nd")), set("Nd", None));
    }

    #[test]
    fn scripts_and_extensions() {
        let latin = set("sc", Some("Latn"));
        assert!(contains(&latin, 0x41) && !contains(&latin, 0x3B1));
        assert!(contains(&set("Script", Some("Grek")), 0x3B1));
        // U+0640 ARABIC TATWEEL: Common, extended to several scripts.
        assert!(contains(&set("sc", Some("Zyyy")), 0x640));
        assert!(!contains(&set("scx", Some("Zyyy")), 0x640));
        assert!(contains(&set("scx", Some("Arab")), 0x640));
        assert!(contains(&set("scx", Some("Latn")), 0x41));
    }

    #[test]
    fn binary_properties() {
        assert_eq!(set("ASCII", None), [(0, 0x7F)]);
        assert_eq!(set("Any", None), [(0, MAX_CODE_POINT)]);
        let assigned = set("Assigned", None);
        assert!(contains(&assigned, 0x41) && !contains(&assigned, 0x378));
        assert_eq!(
            count(&assigned) + count(&set("Cn", None)),
            MAX_CODE_POINT + 1
        );
        assert!(contains(&set("Alphabetic", None), 0x41));
        assert!(contains(&set("White_Space", None), 0x2028));
        assert_eq!(set("WSpace", None), set("White_Space", None));
        assert!(contains(&set("Emoji", None), 0x1F600));
        assert!(contains(&set("Bidi_Mirrored", None), 0x28));
        assert!(contains(&set("Changes_When_NFKC_Casefolded", None), 0x41));
        assert_eq!(count(&set("Noncharacter_Code_Point", None)), 66);
        assert_eq!(count(&set("Regional_Indicator", None)), 26);
    }

    #[test]
    fn properties_of_strings() {
        let emoji = property::lookup("RGI_Emoji", None).expect("a property of strings");
        assert!(contains(&ranges(emoji), 0x1F600));
        assert!(strings(emoji).contains(&vec![0x1F1E6, 0x1F1E8]));
        assert!(strings(emoji).contains(&vec![0x23, 0xFE0F, 0x20E3]));
        let flags = property::lookup("RGI_Emoji_Flag_Sequence", None).expect("flags");
        assert_eq!(ranges(flags).len(), 0);
        assert!(strings(flags).iter().all(|s| s.len() == 2));
        assert!(strings(flags).len() > 250);
        let keycaps = property::lookup("Emoji_Keycap_Sequence", None).expect("keycaps");
        assert_eq!(strings(keycaps).len(), 12);
        let zwj = property::lookup("RGI_Emoji_ZWJ_Sequence", None).expect("zwj");
        assert!(strings(zwj).iter().all(|s| s.contains(&0x200D)));
        let basic = property::lookup("Basic_Emoji", None).expect("basic");
        assert!(strings(basic).contains(&vec![0x1F5D3, 0xFE0F]));
        assert_eq!(
            strings(property::lookup("Emoji", None).expect("binary")).len(),
            0
        );
        // RGI_Emoji is the union.
        let total: usize = [
            "Basic_Emoji",
            "Emoji_Keycap_Sequence",
            "RGI_Emoji_Modifier_Sequence",
            "RGI_Emoji_Flag_Sequence",
            "RGI_Emoji_Tag_Sequence",
            "RGI_Emoji_ZWJ_Sequence",
        ]
        .iter()
        .map(|n| strings(property::lookup(n, None).expect("a property")).len())
        .sum();
        assert_eq!(strings(emoji).len(), total);
    }

    #[test]
    fn bad_data_does_not_panic() {
        let mut out = Vec::new();
        decode_ranges(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x80], &mut out);
        decode_ranges(&[0xFF, 0xFF, 0xFF, 0xFF, 0x7F, 0x01], &mut out);
        // A fifth byte with bits above bit 31 is an overflow.
        let mut pos = 0;
        assert_eq!(leb128(&[0xFF, 0xFF, 0xFF, 0xFF, 0x7F], &mut pos), None);
        pos = 0;
        assert_eq!(
            leb128(&[0xFF, 0xFF, 0xFF, 0xFF, 0x0F], &mut pos),
            Some(u32::MAX)
        );
        assert_eq!(item(&d::BINARY_OFFSETS, 1000).len(), 0);
        assert_eq!(ranges(Property::Script(60000)).len(), 0);
    }
}
