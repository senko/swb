//! Regular expressions for the JavaScript engine (ADR 0026 section 10):
//! pattern parser, compiler and backtracking matcher.
//!
//! This is a stub for the spike (roadmap M6 step 3): it checks the flags
//! of a regular expression literal and accepts any pattern. Feature 8a of
//! M7 replaces it with the pattern parser and its early errors.

use swb_js_text::Str16;

/// The flags of a regular expression (§22.2.6.4: `d`, `g`, `i`, `m`, `s`,
/// `u`, `v`, `y`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Flags(u8);

impl Flags {
    /// `d`: match indices.
    pub const HAS_INDICES: Flags = Flags(1 << 0);
    /// `g`: global.
    pub const GLOBAL: Flags = Flags(1 << 1);
    /// `i`: ignore case.
    pub const IGNORE_CASE: Flags = Flags(1 << 2);
    /// `m`: multiline.
    pub const MULTILINE: Flags = Flags(1 << 3);
    /// `s`: `.` matches line terminators.
    pub const DOT_ALL: Flags = Flags(1 << 4);
    /// `u`: Unicode.
    pub const UNICODE: Flags = Flags(1 << 5);
    /// `v`: Unicode sets.
    pub const UNICODE_SETS: Flags = Flags(1 << 6);
    /// `y`: sticky.
    pub const STICKY: Flags = Flags(1 << 7);

    /// No flags.
    pub const fn empty() -> Flags {
        Flags(0)
    }

    /// Whether all flags of `other` are set.
    pub const fn contains(self, other: Flags) -> bool {
        self.0 & other.0 == other.0
    }

    /// The flag of a flag letter, or `None` for another code unit.
    fn from_letter(unit: u16) -> Option<Flags> {
        let flag = match u8::try_from(unit).ok()? {
            b'd' => Flags::HAS_INDICES,
            b'g' => Flags::GLOBAL,
            b'i' => Flags::IGNORE_CASE,
            b'm' => Flags::MULTILINE,
            b's' => Flags::DOT_ALL,
            b'u' => Flags::UNICODE,
            b'v' => Flags::UNICODE_SETS,
            b'y' => Flags::STICKY,
            _ => return None,
        };
        Some(flag)
    }
}

/// An error in a regular expression.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// An unknown flag letter, a letter twice, or both `u` and `v`.
    #[error("Invalid regular expression flags")]
    InvalidFlags,
}

/// Parses the flags of a regular expression: only the letters of
/// [`Flags`], none twice, and not both `u` and `v` (§22.2.3.1,
/// `RegExpInitialize`; §13.2.7.2, `IsValidRegularExpressionLiteral`).
pub fn parse_flags(text: Str16<'_>) -> Result<Flags, Error> {
    let mut flags = Flags::empty();
    for unit in text.units() {
        let flag = Flags::from_letter(unit).ok_or(Error::InvalidFlags)?;
        if flags.contains(flag) {
            return Err(Error::InvalidFlags);
        }
        flags.0 |= flag.0;
    }
    if flags.contains(Flags::UNICODE) && flags.contains(Flags::UNICODE_SETS) {
        return Err(Error::InvalidFlags);
    }
    Ok(flags)
}

/// Checks a regular expression literal (§13.2.7.2,
/// `IsValidRegularExpressionLiteral`) and returns its flags. The stub
/// accepts any pattern.
pub fn check_literal(pattern: Str16<'_>, flags: Str16<'_>) -> Result<Flags, Error> {
    let _ = pattern;
    parse_flags(flags)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(text: &str) -> Result<Flags, Error> {
        parse_flags(Str16::Latin1(text.as_bytes()))
    }

    #[test]
    fn valid_flags() {
        assert_eq!(flags(""), Ok(Flags::empty()));
        let all = flags("dgimsuy").expect("all letters but v are valid together");
        for flag in [
            Flags::HAS_INDICES,
            Flags::GLOBAL,
            Flags::STICKY,
            Flags::UNICODE,
        ] {
            assert!(all.contains(flag));
        }
        assert!(!all.contains(Flags::UNICODE_SETS));
        assert!(flags("v").is_ok_and(|f| f.contains(Flags::UNICODE_SETS)));
    }

    #[test]
    fn invalid_flags() {
        for text in ["gg", "x", "G", "uv", "gimg", " "] {
            assert_eq!(flags(text), Err(Error::InvalidFlags), "{text:?}");
        }
        assert_eq!(
            parse_flags(Str16::Wide(&[0x0067, 0x0131])),
            Err(Error::InvalidFlags)
        );
        assert_eq!(
            parse_flags(Str16::Wide(&[0x0167])),
            Err(Error::InvalidFlags)
        );
    }

    #[test]
    fn any_pattern() {
        let pattern = Str16::Latin1(b"(unclosed[");
        assert!(check_literal(pattern, Str16::Latin1(b"g")).is_ok());
    }
}
