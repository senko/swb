//! Web fonts of a page: the `@font-face` rules of the document as the
//! text crate's face set, and the font files that the page loads
//! (ADR 0022).
//!
//! The text crate decides which faces text needs
//! ([`swb_text::FontContext::take_web_font_requests`]); the page loads
//! their sources in order (`document/loads.rs`). Each URL is fetched once and
//! decoded once, also when several faces use it (a variable font for
//! many `font-weight` rules).

use std::collections::HashMap;
use std::sync::Arc;

use swb_net::{ByteBudget, Reservation, Url};
use swb_style::{FontFace, FontFaceSource, FontFaceStyle};
use swb_text::{Feature, WebFaceId, WebFontFace, WebFontSet, WebFontStyle};

/// The prefix of a `local()` source in the text crate's face set. A
/// resolved URL always has a scheme, so it never starts with this.
const LOCAL_PREFIX: &str = "local(";

/// A source of a face, as the page loads it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Source {
    Url(Url),
    /// `local()`: an installed font, by full name or PostScript name.
    Local(String),
    /// A URL that does not parse.
    Invalid,
}

impl Source {
    /// Parses a source of the text crate's face set (see [`web_font_face`]).
    pub(crate) fn parse(source: &str) -> Source {
        if let Some(name) = source.strip_prefix(LOCAL_PREFIX) {
            return Source::Local(name.strip_suffix(')').unwrap_or(name).to_owned());
        }
        Url::parse(source).map_or(Source::Invalid, Source::Url)
    }
}

/// A face of an `@font-face` rule for the text crate. URL sources are
/// absolute URL strings; `local()` sources are `local(name)`.
pub(crate) fn web_font_face(face: &FontFace) -> WebFontFace {
    WebFontFace {
        family: face.family.to_string(),
        weight: face.weight,
        stretch: face.stretch,
        style: match face.style {
            FontFaceStyle::Auto => WebFontStyle::Auto,
            FontFaceStyle::Normal => WebFontStyle::Normal,
            FontFaceStyle::Italic => WebFontStyle::Italic,
            FontFaceStyle::Oblique(min, max) => WebFontStyle::Oblique(min, max),
        },
        unicode_range: face.unicode_range.clone(),
        size_adjust: face.size_adjust,
        ascent_override: face.ascent_override,
        descent_override: face.descent_override,
        line_gap_override: face.line_gap_override,
        variations: face.variation_settings.to_vec(),
        features: face
            .feature_settings
            .iter()
            .map(|&(tag, value)| Feature::from_setting(tag, value))
            .collect(),
        sources: face
            .sources
            .iter()
            .map(|source| match source {
                FontFaceSource::Url(url) => url.to_string(),
                FontFaceSource::Local(name) => format!("{LOCAL_PREFIX}{name})"),
            })
            .collect(),
    }
}

/// The state of a font file.
#[derive(Debug)]
pub(crate) enum FontFile {
    /// Being fetched; the faces that wait for it, with the index of the
    /// source that refers to the file.
    Loading(Vec<(WebFaceId, usize)>),
    /// Decoded OpenType data.
    Loaded(Arc<[u8]>),
    Failed,
}

/// What a face that loads costs the page's budget for the fonts of frame
/// documents, besides its file: the parsed tables and the shape plans
/// (`LoadedFace`). A document may load 1,000 faces of one small file.
pub(crate) const FACE_COST: usize = 16 * 1024;

/// What a `local()` face costs that budget: the file of the installed font,
/// which the text crate reads (the size is not known before that).
pub(crate) const LOCAL_FACE_COST: usize = 1 << 20;

/// The web font loading state of a page.
#[derive(Debug, Default)]
pub(crate) struct WebFonts {
    /// The font files of the document, by URL.
    pub(crate) files: HashMap<Url, FontFile>,
    /// The media environment of the face set that the font context has;
    /// `None` when the face set must be computed again (a new stylist).
    pub(crate) env: Option<swb_css::MediaEnvironment>,
    /// The faces of a frame document while another document's faces are in
    /// the font context (ADR 0031 section 4). The top-level document's
    /// faces stay in the context, so its text path has no exchange.
    pub(crate) set: WebFontSet,
    /// The page's budget for the fonts of frame documents; `None` for the
    /// top-level document.
    budget: Option<Arc<ByteBudget>>,
    /// What the fonts of the document hold of `budget`; the bytes return
    /// when the document goes.
    held: Vec<Arc<Reservation>>,
}

impl WebFonts {
    /// The state of a new document under `budget`.
    pub(crate) fn with_budget(budget: Option<Arc<ByteBudget>>) -> Self {
        WebFonts {
            budget,
            ..WebFonts::default()
        }
    }

    /// Charges `bytes` to the page's budget for the fonts of frame
    /// documents. False, with one warning per budget and top-level document,
    /// if they do not fit: the font (or face) then fails.
    pub(crate) fn admit(&mut self, bytes: usize) -> bool {
        let Some(budget) = &self.budget else {
            return true;
        };
        if let Some(reservation) = Reservation::new(budget, bytes) {
            self.held.push(reservation);
            return true;
        }
        if budget.warn_once() {
            log::warn!(
                "the web fonts of frame documents hold {} of {} bytes, the limit; later fonts of frames fail",
                budget.used(),
                budget.limit()
            );
        }
        false
    }

    /// Forgets files whose fetch was cancelled (by `stop` or a
    /// navigation): their faces fail.
    pub(crate) fn forget_loading(&mut self) -> Vec<WebFaceId> {
        let mut waiting = Vec::new();
        self.files.retain(|_, file| match file {
            FontFile::Loading(faces) => {
                waiting.extend(faces.iter().map(|(id, _)| *id));
                false
            }
            _ => true,
        });
        waiting
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fonts_of_frame_documents_share_a_budget() {
        let budget = ByteBudget::new(100);
        let mut first = WebFonts::with_budget(Some(Arc::clone(&budget)));
        let mut second = WebFonts::with_budget(Some(Arc::clone(&budget)));
        assert!(first.admit(60));
        assert!(!second.admit(41));
        assert!(second.admit(40));
        assert_eq!(budget.used(), 100);
        // The bytes return with the document.
        drop(first);
        assert_eq!(budget.used(), 40);
        assert!(second.admit(60));
        // The top-level document has no budget.
        assert!(WebFonts::default().admit(1 << 40));
    }

    #[test]
    fn local_source_strips_one_closing_parenthesis() {
        assert_eq!(Source::parse("local(A)"), Source::Local("A".into()));
        assert_eq!(Source::parse("local(A))"), Source::Local("A)".into()));
        assert_eq!(Source::parse("local(A"), Source::Local("A".into()));
    }

    #[test]
    fn sources_round_trip() {
        let face = FontFace {
            family: "F".into(),
            sources: vec![
                FontFaceSource::Local("Liberation Sans".into()),
                FontFaceSource::Url("https://example.com/f.woff2".into()),
                FontFaceSource::Url("not a url".into()),
            ],
            weight: Some((400.0, 700.0)),
            stretch: None,
            style: FontFaceStyle::Oblique(10.0, 20.0),
            unicode_range: vec![(0, 0x7F)],
            display: swb_style::FontDisplay::Swap,
            size_adjust: 0.9375,
            ascent_override: Some(0.98),
            descent_override: None,
            line_gap_override: Some(0.0),
            variation_settings: Arc::from([(*b"wght", 660.0)]),
            feature_settings: Arc::from([(*b"liga", 0), (*b"salt", -1)]),
        };
        let web = web_font_face(&face);
        assert_eq!(web.family, "F");
        assert_eq!(web.weight, Some((400.0, 700.0)));
        assert_eq!(web.style, WebFontStyle::Oblique(10.0, 20.0));
        assert_eq!(web.size_adjust, 0.9375);
        assert_eq!(web.ascent_override, Some(0.98));
        assert_eq!(web.descent_override, None);
        assert_eq!(web.line_gap_override, Some(0.0));
        assert_eq!(web.variations, vec![(*b"wght", 660.0)]);
        assert_eq!(
            web.features,
            vec![
                Feature {
                    tag: *b"liga",
                    value: 0
                },
                Feature {
                    tag: *b"salt",
                    value: u32::MAX
                }
            ]
        );
        let sources: Vec<Source> = web.sources.iter().map(|s| Source::parse(s)).collect();
        assert_eq!(
            sources,
            vec![
                Source::Local("Liberation Sans".into()),
                Source::Url(Url::parse("https://example.com/f.woff2").unwrap()),
                Source::Invalid,
            ]
        );
    }
}
