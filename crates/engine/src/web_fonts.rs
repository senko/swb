//! Web fonts of a page: the `@font-face` rules of the document as the
//! text crate's face set, and the font files that the page loads
//! (ADR 0022).
//!
//! The text crate decides which faces text needs
//! ([`swb_text::FontContext::take_web_font_requests`]); the page loads
//! their sources in order (`page/fonts.rs`). Each URL is fetched once and
//! decoded once, also when several faces use it (a variable font for
//! many `font-weight` rules).

use std::collections::HashMap;
use std::sync::Arc;

use swb_net::Url;
use swb_style::{FontFace, FontFaceSource, FontFaceStyle};
use swb_text::{Feature, WebFaceId, WebFontFace, WebFontStyle};

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

/// The web font loading state of a page.
#[derive(Debug, Default)]
pub(crate) struct WebFonts {
    /// The font files of the document, by URL.
    pub(crate) files: HashMap<Url, FontFile>,
    /// The media environment of the face set that the font context has;
    /// `None` when the face set must be computed again (a new stylist).
    pub(crate) env: Option<swb_css::MediaEnvironment>,
}

impl WebFonts {
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
