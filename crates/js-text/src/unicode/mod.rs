//! Unicode character classes of the JavaScript lexical grammar (ECMA-262
//! clause 12): white space, line terminators and identifier characters.
//!
//! The tables in `tables.rs` are generated from the Unicode Character
//! Database by `swbtools js-unicode-tables`. ASCII has fast paths that a
//! test compares with the tables.

mod tables;

/// U+200C ZERO WIDTH NON-JOINER.
const ZWNJ: u32 = 0x200C;
/// U+200D ZERO WIDTH JOINER.
const ZWJ: u32 = 0x200D;
/// U+FEFF ZERO WIDTH NO-BREAK SPACE.
const ZWNBSP: u32 = 0xFEFF;

fn in_ranges(table: &[(u32, u32)], cp: u32) -> bool {
    table
        .binary_search_by(|&(first, last)| {
            if last < cp {
                std::cmp::Ordering::Less
            } else if first > cp {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// Whether a code point has the Unicode property `ID_Start`.
pub(crate) fn is_id_start(cp: u32) -> bool {
    if cp < 0x80 {
        return (cp as u8).is_ascii_alphabetic();
    }
    in_ranges(&tables::ID_START, cp)
}

/// Whether a code point has the Unicode property `ID_Continue`.
pub(crate) fn is_id_continue(cp: u32) -> bool {
    if cp < 0x80 {
        return (cp as u8).is_ascii_alphanumeric() || cp == u32::from(b'_');
    }
    in_ranges(&tables::ID_CONTINUE, cp)
}

/// `IdentifierStartChar` (§12.7): `ID_Start`, `$` or `_`.
pub fn is_identifier_start(cp: u32) -> bool {
    cp == u32::from(b'$') || cp == u32::from(b'_') || is_id_start(cp)
}

/// `IdentifierPartChar` (§12.7): `ID_Continue`, `$`, ZWNJ or ZWJ.
pub fn is_identifier_part(cp: u32) -> bool {
    cp == u32::from(b'$') || cp == ZWNJ || cp == ZWJ || is_id_continue(cp)
}

/// Whether `cp` can be in an identifier at this position: an
/// `IdentifierStartChar` if `first`, else an `IdentifierPartChar`.
pub fn is_identifier_char(cp: u32, first: bool) -> bool {
    if first {
        is_identifier_start(cp)
    } else {
        is_identifier_part(cp)
    }
}

/// `WhiteSpace` (§12.2): TAB, VT, FF, ZWNBSP and `General_Category` Zs.
pub fn is_whitespace(cp: u32) -> bool {
    match cp {
        0x09 | 0x0B | 0x0C | 0x20 | 0xA0 | ZWNBSP => true,
        0..0x80 => false,
        _ => in_ranges(&tables::SPACE_SEPARATORS, cp),
    }
}

/// `LineTerminator` (§12.3): LF, CR, U+2028 and U+2029.
pub fn is_line_terminator(cp: u32) -> bool {
    matches!(cp, 0x0A | 0x0D | 0x2028 | 0x2029)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_fast_paths_agree_with_the_tables() {
        for cp in 0..0x80 {
            assert_eq!(is_id_start(cp), in_ranges(&tables::ID_START, cp), "{cp:#x}");
            assert_eq!(
                is_id_continue(cp),
                in_ranges(&tables::ID_CONTINUE, cp),
                "{cp:#x}"
            );
            let space = in_ranges(&tables::SPACE_SEPARATORS, cp) || matches!(cp, 9 | 0xB | 0xC);
            assert_eq!(is_whitespace(cp), space, "{cp:#x}");
        }
    }

    #[test]
    fn tables_are_sorted_and_disjoint() {
        for table in [
            &tables::ID_START[..],
            &tables::ID_CONTINUE,
            &tables::SPACE_SEPARATORS,
        ] {
            for pair in table.windows(2) {
                assert!(pair[0].0 <= pair[0].1 && pair[0].1 < pair[1].0, "{pair:?}");
            }
        }
    }

    #[test]
    fn identifier_characters() {
        assert!(is_identifier_start(u32::from('$')));
        assert!(is_identifier_start(u32::from('_')));
        assert!(is_identifier_start(0xAA)); // feminine ordinal indicator, Lo
        assert!(is_identifier_start(0x2118)); // script P, Other_ID_Start
        assert!(is_identifier_start(0x1_0000)); // Linear B syllable B008 A
        assert!(!is_identifier_start(u32::from('1')));
        assert!(!is_identifier_start(0x200C));
        assert!(is_identifier_part(0x200C));
        assert!(is_identifier_part(0x200D));
        assert!(is_identifier_part(u32::from('7')));
        assert!(is_identifier_part(0xB7)); // middle dot, Other_ID_Continue
        assert!(!is_identifier_start(0xB7));
        assert!(!is_identifier_part(0x2E2F)); // vertical tilde: Pattern_Syntax
        assert!(!is_identifier_part(0xD800));
    }

    #[test]
    fn white_space_and_line_terminators() {
        for cp in [
            0x09, 0x0B, 0x0C, 0x20, 0xA0, 0x1680, 0x2000, 0x200A, 0x202F, 0x205F, 0x3000,
        ] {
            assert!(is_whitespace(cp), "{cp:#x}");
        }
        assert!(is_whitespace(0xFEFF));
        // U+180E MONGOLIAN VOWEL SEPARATOR is no longer Zs; U+200B is Cf.
        assert!(!is_whitespace(0x180E));
        assert!(!is_whitespace(0x200B));
        assert!(!is_whitespace(0x0A));
        assert!(!is_whitespace(0x2028));
        assert!(is_line_terminator(0x2028));
        assert!(is_line_terminator(0x0D));
        assert!(!is_line_terminator(0x85));
    }
}
