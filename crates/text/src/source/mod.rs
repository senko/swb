//! Font sources: where fonts come from and how family names resolve.
//!
//! [`FontContext`](crate::FontContext) asks a source for families and
//! fallback faces, and does CSS matching within a family itself. There are
//! three sources:
//!
//! - [`directory::DirectorySource`]: the fonts of one directory with an
//!   explicit family map. Deterministic; used by tests.
//! - `fontconfig::FontconfigSource`: the system fonts through fontconfig, as
//!   Chromium does it on Linux.
//! - [`EmptySource`]: no fonts, for platforms without a backend.

pub(crate) mod directory;
#[cfg(all(unix, not(any(target_vendor = "apple", target_os = "android"))))]
pub(crate) mod fontconfig;

use crate::face::{FaceDesc, FamilyDesc};
use crate::query::GenericFamily;

/// A provider of font families and fallback faces.
pub(crate) trait FontSource: Send {
    /// Resolves a family name from a CSS `font-family` list. Returns `None`
    /// if the family does not exist, so that the next family in the list is
    /// tried.
    fn named_family(&mut self, name: &str) -> Option<FamilyDesc>;

    /// Resolves a generic family.
    fn generic_family(&mut self, generic: GenericFamily) -> Option<FamilyDesc>;

    /// The family to use when no family of a query exists.
    fn default_family(&mut self) -> Option<FamilyDesc>;

    /// A face with a glyph for `c`, for characters that the requested
    /// families do not cover. `language` is a lowercase BCP 47 tag.
    ///
    /// Each source has a fixed order of fallback candidates. This returns
    /// the first candidate at position `from` or later that has a glyph for
    /// `c`, with its position. If the caller cannot use the face, it calls
    /// again with `from` set to that position plus one.
    fn fallback_face(&mut self, c: char, language: &str, from: usize) -> Option<(usize, FaceDesc)>;
}

/// A source without fonts.
pub(crate) struct EmptySource;

impl FontSource for EmptySource {
    fn named_family(&mut self, _name: &str) -> Option<FamilyDesc> {
        None
    }

    fn generic_family(&mut self, _generic: GenericFamily) -> Option<FamilyDesc> {
        None
    }

    fn default_family(&mut self) -> Option<FamilyDesc> {
        None
    }

    fn fallback_face(
        &mut self,
        _c: char,
        _language: &str,
        _from: usize,
    ) -> Option<(usize, FaceDesc)> {
        None
    }
}
