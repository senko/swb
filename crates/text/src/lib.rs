//! Fonts, font fallback, text shaping and glyph rasterization.
//!
//! [`FontContext`] is the entry point. Layout uses it to select fonts
//! ([`FontContext::select`]), split text into runs by font
//! ([`FontContext::itemize`]), shape runs ([`FontContext::shape`]) and get
//! font metrics ([`FontContext::metrics`]). Paint uses it to get glyph
//! coverage masks ([`FontContext::glyph_mask`]). [`line_breaks`] gives the
//! line break opportunities for line layout (UAX #14 as tailored by
//! Chromium).
//!
//! Fonts come from fontconfig ([`FontContext::system`]), with Chromium's
//! rules so that both browsers pick the same fonts, or from one directory
//! ([`FontContext::from_directory`], [`FontContext::for_tests`]) for
//! deterministic tests. See `docs/adr/0006-text-stack.md`.
//!
//! Units: sizes and all returned lengths are pixels. Vertical offsets use y
//! down.

mod context;
mod error;
mod face;
mod itemize;
mod linebreak;
mod mask_cache;
mod matching;
mod metrics;
mod query;
mod raster;
mod shape;
mod source;

pub use context::{FontContext, FontInfo, Synthesis};
pub use error::TextError;
pub use itemize::FontRun;
pub use linebreak::{BreakKind, BreakRules, WordBreak, line_breaks};
pub use metrics::FontMetrics;
pub use query::{FamilyName, FontQuery, FontStyle, GenericFamily, GenericFamilyMap};
pub use raster::GlyphMask;
pub use shape::{Direction, Feature, ShapeOptions, ShapedGlyph, ShapedRun, context_around};

/// A concrete font: one face of a font file with its synthetic styles and
/// variation settings. Valid only for the [`FontContext`] that returned it.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct FontId(u32);

/// A glyph index in a font.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct GlyphId(pub u32);
