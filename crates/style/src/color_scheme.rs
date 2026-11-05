//! The `color-scheme` property and the used color scheme of an element (CSS
//! Color Adjustment 1 §2,
//! <https://drafts.csswg.org/css-color-adjust-1/#color-scheme-prop>) and the
//! `color-scheme` meta name (HTML §4.2.5.4,
//! <https://html.spec.whatwg.org/multipage/semantics.html#meta-color-scheme>).
//!
//! swb uses the used color scheme for frames only (ADR 0031 section 4): it
//! decides whether a frame's canvas is transparent, and the color of its
//! text. The rendering of the top-level document does not depend on it.

use std::sync::Arc;

use swb_css::{ColorScheme, ParseError, Parser};

/// `normal | [ light | dark | <custom-ident> ]+ && only?`: `None` for
/// `normal`, else the words in lowercase (custom identifiers as written),
/// in the order of the text, `only` last. `only` stands first or last in the
/// text, not between the words (measured: case
/// `paint-color-scheme-syntax`).
pub(crate) fn parse_color_scheme(p: &mut Parser<'_>) -> Result<Option<Arc<str>>, ParseError> {
    if p.try_parse(|p| p.expect_ident_matching("normal")).is_ok() {
        return Ok(None);
    }
    let mut words: Vec<String> = Vec::new();
    let mut only = false;
    while !p.is_exhausted() {
        let ident = p.expect_ident()?;
        let lower = ident.to_ascii_lowercase();
        match lower.as_str() {
            "only" if !only && (words.is_empty() || p.is_exhausted()) => only = true,
            // Measured: these are not custom identifiers here.
            "only" | "normal" | "initial" | "inherit" | "unset" | "revert" | "revert-layer"
            | "default" => return Err(ParseError::Invalid),
            "light" | "dark" => words.push(lower),
            _ => words.push(ident.to_owned()),
        }
    }
    if words.is_empty() {
        return Err(ParseError::Invalid);
    }
    if only {
        words.push("only".to_owned());
    }
    Ok(Some(Arc::from(words.join(" "))))
}

/// The text of a `color-scheme` value in `element.style`.
pub(crate) fn declared_color_scheme(tokens: &[swb_css::ComponentValue]) -> Option<String> {
    let mut p = Parser::new(tokens);
    let value = p.parse_entirely::<_, ParseError>(parse_color_scheme).ok()?;
    Some(value.as_deref().unwrap_or("normal").to_owned())
}

/// The page's supported color schemes from the `content` of a `color-scheme`
/// meta element: `None` if the text is not a valid value, `Some(None)` for
/// `normal`.
pub fn parse_meta_color_scheme(content: &str) -> Option<Option<Arc<str>>> {
    let values = swb_css::parse_component_values(content);
    let mut p = Parser::new(&values);
    p.parse_entirely::<_, ParseError>(parse_color_scheme).ok()
}

/// The used color scheme of an element whose computed `color-scheme` is
/// `value`, on a page whose supported color schemes are `page` and with the
/// user's preference `preferred`: the preferred one if the list has it, else
/// the first of `light` and `dark` in the list. `normal` takes the page's
/// list; a list without `light` and `dark` is `light`.
pub fn used_color_scheme(
    value: Option<&str>,
    page: Option<&str>,
    preferred: ColorScheme,
) -> ColorScheme {
    let mut first = None;
    for word in value.or(page).unwrap_or_default().split_ascii_whitespace() {
        let scheme = match word {
            "light" => ColorScheme::Light,
            "dark" => ColorScheme::Dark,
            _ => continue,
        };
        if scheme == preferred {
            return scheme;
        }
        first = first.or(Some(scheme));
    }
    first.unwrap_or(ColorScheme::Light)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values() {
        let parse = |text: &str| parse_meta_color_scheme(text).map(|v| v.map(|s| s.to_string()));
        assert_eq!(parse("normal"), Some(None));
        assert_eq!(parse("Dark"), Some(Some("dark".into())));
        assert_eq!(parse("light DARK"), Some(Some("light dark".into())));
        assert_eq!(parse("only dark"), Some(Some("dark only".into())));
        assert_eq!(parse("dark only"), Some(Some("dark only".into())));
        assert_eq!(parse("Foo dark"), Some(Some("Foo dark".into())));
        assert_eq!(parse("none dark"), Some(Some("none dark".into())));
        assert_eq!(parse("only"), None);
        assert_eq!(parse("only dark only"), None);
        assert_eq!(parse("light only dark"), None);
        assert_eq!(
            parse("only light dark"),
            Some(Some("light dark only".into()))
        );
        assert_eq!(
            parse("light dark only"),
            Some(Some("light dark only".into()))
        );
        assert_eq!(parse("normal dark"), None);
        assert_eq!(parse("dark normal"), None);
        assert_eq!(parse("dark, light"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn the_used_scheme() {
        use ColorScheme::{Dark, Light};
        assert_eq!(used_color_scheme(None, None, Light), Light);
        assert_eq!(used_color_scheme(None, None, Dark), Light);
        assert_eq!(used_color_scheme(Some("dark"), None, Light), Dark);
        assert_eq!(used_color_scheme(Some("light dark"), None, Light), Light);
        assert_eq!(used_color_scheme(Some("light dark"), None, Dark), Dark);
        assert_eq!(used_color_scheme(Some("dark light"), None, Light), Light);
        assert_eq!(used_color_scheme(Some("dark only"), None, Light), Dark);
        assert_eq!(used_color_scheme(None, Some("dark"), Light), Dark);
        assert_eq!(used_color_scheme(Some("light"), Some("dark"), Light), Light);
        assert_eq!(used_color_scheme(Some("foo"), Some("dark"), Light), Light);
    }
}
