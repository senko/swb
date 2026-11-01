//! Language tags and locale negotiation (ECMA-402 §6,
//! <https://tc39.es/ecma402/#sec-case-sensitivity-and-case-mapping>, §9.2).
//!
//! The data has two locales, `en-US` and `en-GB` (ADR 0027 section 10). A
//! request for `en` or `en-US` resolves to `en` or `en-US` with the
//! `en-US` data, `en-GB` to `en-GB`; any other locale is not available
//! and the negotiation falls back to the default, `en-US`. Chromium 148
//! has many more locales (`de` gives `de`); that difference is the
//! departure of the ADR.

use super::data::{EN_GB, EN_US};
use super::tables::LocaleData;

/// The locale that is used when a request names no available locale.
pub(crate) const DEFAULT_TAG: &str = "en-US";

/// The result of the negotiation: the data and the tag to report.
#[derive(Clone, Debug)]
pub(crate) struct Resolved {
    /// The tag of `resolvedOptions().locale`.
    pub(crate) tag: String,
    /// The data of the locale.
    pub(crate) data: &'static LocaleData,
    /// The value of the keyword `hc` of the tag (`en-u-hc-h23`).
    pub(crate) hour_cycle: Option<String>,
}

/// The value of the Unicode extension keyword `hc` in a canonical tag. The
/// keywords are read from the `-u-` extension only: the private use part
/// (`-x-`, which ends the extensions) and other extensions are not looked
/// at.
fn hour_cycle_keyword(tag: &str) -> Option<String> {
    let head = tag.split_once("-x-").map_or(tag, |(head, _)| head);
    let extension: Vec<&str> = head
        .split('-')
        .skip_while(|p| *p != "u")
        .skip(1)
        .take_while(|p| p.len() > 1)
        .collect();
    let at = extension.iter().position(|p| *p == "hc")?;
    let value = extension.get(at + 1)?;
    ["h11", "h12", "h23", "h24"]
        .contains(value)
        .then(|| (*value).to_owned())
}

/// A language tag that is not valid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InvalidTag {
    /// Whether the message of the error shows the tag in lower case. In
    /// Chromium 148 it does when the tag is cut short or repeats a variant
    /// or a singleton (`EN-US-1996-1996`, `EN-u`, `EN-`), and not when a
    /// subtag is out of place (`en-GB-oed`, `En_Us`): measured.
    pub(crate) lowercase: bool,
}

const OUT_OF_PLACE: InvalidTag = InvalidTag { lowercase: false };
const CUT_OR_REPEATED: InvalidTag = InvalidTag { lowercase: true };

fn all_alpha(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphabetic())
}

fn all_digit(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn alnum(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// The canonical form of a structurally valid language tag (BCP 47, RFC
/// 5646 §2.1; ECMA-402 `CanonicalizeUnicodeLocaleId`): the language in
/// lower case, a script in title case, a region in upper case, the other
/// subtags in lower case. The aliases and the variant order of the full
/// canonicalization are not applied.
pub(crate) fn canonicalize(tag: &str) -> Result<String, InvalidTag> {
    let parts: Vec<&str> = tag.split('-').collect();
    let language = parts[0];
    let valid_language = all_alpha(language) && matches!(language.len(), 2 | 3 | 5..=8);
    if !valid_language {
        return Err(OUT_OF_PLACE);
    }
    let mut out = vec![language.to_ascii_lowercase()];
    let mut index = 1;
    if let Some(script) = parts.get(index).filter(|s| all_alpha(s) && s.len() == 4) {
        let lower = script.to_ascii_lowercase();
        out.push(format!(
            "{}{}",
            lower[..1].to_ascii_uppercase(),
            &lower[1..]
        ));
        index += 1;
    }
    if let Some(region) = parts
        .get(index)
        .filter(|s| (all_alpha(s) && s.len() == 2) || (all_digit(s) && s.len() == 3))
    {
        out.push(region.to_ascii_uppercase());
        index += 1;
    }
    let mut variants: std::collections::HashSet<String> = std::collections::HashSet::new();
    while let Some(variant) = parts.get(index).filter(|s| {
        alnum(s) && (matches!(s.len(), 5..=8) || (s.len() == 4 && s.as_bytes()[0].is_ascii_digit()))
    }) {
        let lower = variant.to_ascii_lowercase();
        if !variants.insert(lower.clone()) {
            // `unicode_locale_id` allows no variant twice.
            return Err(CUT_OR_REPEATED);
        }
        out.push(lower);
        index += 1;
    }
    // Extensions: a singleton and one or more subtags of 2 to 8 characters.
    let mut singletons: Vec<String> = Vec::new();
    while let Some(singleton) = parts.get(index) {
        if singleton.len() != 1 || !alnum(singleton) {
            break;
        }
        let lower = singleton.to_ascii_lowercase();
        if singletons.contains(&lower) {
            // No singleton may start two extensions.
            return Err(CUT_OR_REPEATED);
        }
        singletons.push(lower.clone());
        out.push(lower);
        index += 1;
        let start = index;
        // The subtags of private use (`x`) may have one character.
        let shortest = if singleton.eq_ignore_ascii_case("x") {
            1
        } else {
            2
        };
        while let Some(subtag) = parts
            .get(index)
            .filter(|s| alnum(s) && (shortest..=8).contains(&s.len()))
        {
            out.push(subtag.to_ascii_lowercase());
            index += 1;
        }
        if index == start {
            return Err(CUT_OR_REPEATED);
        }
    }
    if index != parts.len() {
        // A hyphen at the end cuts the tag short.
        return Err(if parts[index..] == [""] {
            CUT_OR_REPEATED
        } else {
            OUT_OF_PLACE
        });
    }
    Ok(out.join("-"))
}

/// The tag without its extensions (`-u-ca-gregory`).
fn without_extensions(tag: &str) -> &str {
    let mut end = tag.len();
    for (at, _) in tag.match_indices('-') {
        let rest = &tag[at + 1..];
        if rest.len() >= 2 && rest.as_bytes()[1] == b'-' {
            end = at;
            break;
        }
    }
    &tag[..end]
}

/// The data and the reported tag for a canonical tag, if it is available.
fn available(tag: &str) -> Option<Resolved> {
    match without_extensions(tag) {
        "en" => Some(Resolved {
            tag: "en".to_owned(),
            data: &EN_US,
            hour_cycle: hour_cycle_keyword(tag),
        }),
        "en-US" => Some(Resolved {
            tag: "en-US".to_owned(),
            data: &EN_US,
            hour_cycle: hour_cycle_keyword(tag),
        }),
        "en-GB" => Some(Resolved {
            tag: "en-GB".to_owned(),
            data: &EN_GB,
            hour_cycle: hour_cycle_keyword(tag),
        }),
        _ => None,
    }
}

/// The negotiation (`ResolveLocale`, §9.2.7) for a list of canonical tags:
/// the first available one, else the default.
pub(crate) fn resolve(requested: &[String]) -> Resolved {
    requested
        .iter()
        .find_map(|tag| available(tag))
        .unwrap_or_else(|| Resolved {
            tag: DEFAULT_TAG.to_owned(),
            data: &EN_US,
            hour_cycle: None,
        })
}

/// The tags of `requested` that are available (`SupportedLocales`, §9.2.9).
pub(crate) fn supported(requested: &[String]) -> Vec<String> {
    requested
        .iter()
        .filter(|tag| available(tag).is_some())
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_forms() {
        assert_eq!(canonicalize("EN-gb").unwrap(), "en-GB");
        assert_eq!(canonicalize("en-latn-us").unwrap(), "en-Latn-US");
        assert_eq!(
            canonicalize("en-US-u-ca-gregory").unwrap(),
            "en-US-u-ca-gregory"
        );
        assert_eq!(canonicalize("de").unwrap(), "de");
        assert!(canonicalize("i").is_err());
        assert!(canonicalize("en_US").is_err());
        assert!(canonicalize("").is_err());
        assert!(canonicalize("en-").is_err());
        assert!(canonicalize("en-u").is_err());
        assert!(canonicalize("en-US-a").is_err());
    }

    #[test]
    fn repeats_are_invalid() {
        assert!(canonicalize("en-a-bb-a-cc").is_err());
        assert!(canonicalize("en-A-bb-a-cc").is_err());
        assert!(canonicalize("en-1996-1996").is_err());
        assert!(canonicalize("en-fonipa-FONIPA").is_err());
        // Different singletons and variants, and `x` inside private use.
        assert_eq!(canonicalize("en-a-bb-b-cc").unwrap(), "en-a-bb-b-cc");
        assert_eq!(canonicalize("en-1996-fonipa").unwrap(), "en-1996-fonipa");
        assert_eq!(canonicalize("en-x-a-x-b").unwrap(), "en-x-a-x-b");
        assert!(!canonicalize("en-GB-oed").unwrap_err().lowercase);
        assert!(canonicalize("EN-u").unwrap_err().lowercase);
    }

    #[test]
    fn hour_cycle_keyword_is_read_from_the_unicode_extension_only() {
        assert_eq!(hour_cycle_keyword("en-u-hc-h23").as_deref(), Some("h23"));
        assert_eq!(
            hour_cycle_keyword("en-u-ca-gregory-hc-h23").as_deref(),
            Some("h23")
        );
        assert_eq!(hour_cycle_keyword("en-x-u-hc-h23"), None);
        assert_eq!(hour_cycle_keyword("en-US-x-foo-u-hc-h23"), None);
        assert_eq!(
            hour_cycle_keyword("en-u-hc-h11-x-foo").as_deref(),
            Some("h11")
        );
        assert_eq!(hour_cycle_keyword("en-a-bb-hc-h23"), None);
        assert_eq!(hour_cycle_keyword("en-u-ca-gregory-a-hc-h23"), None);
    }

    #[test]
    fn negotiation() {
        let tags = |t: &[&str]| t.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(resolve(&tags(&["en-GB"])).tag, "en-GB");
        assert_eq!(resolve(&tags(&["xx", "en-GB"])).tag, "en-GB");
        assert_eq!(resolve(&tags(&["en"])).tag, "en");
        assert_eq!(resolve(&tags(&["de"])).tag, "en-US");
        assert_eq!(resolve(&tags(&["en-US-u-ca-gregory"])).tag, "en-US");
        assert_eq!(resolve(&[]).tag, "en-US");
        assert_eq!(
            supported(&tags(&["en", "de", "en-GB"])),
            tags(&["en", "en-GB"])
        );
    }
}
