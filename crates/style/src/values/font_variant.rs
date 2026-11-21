//! `font-variant-ligatures` and `font-variant-numeric` (CSS Fonts 4 §6.4
//! and §6.7): the computed values and the OpenType features that they
//! turn on or off. The forms and the order of the serialized values were
//! measured in Chromium 148 (`tools/probes/css-videojs-2b.json`, cases
//! `props-7*`).
//!
//! <https://www.w3.org/TR/css-fonts-4/#font-variant-ligatures-prop>,
//! <https://www.w3.org/TR/css-fonts-4/#font-variant-numeric-prop>

use swb_css::{ParseError, Parser};

use crate::parse::ParseResult;

/// An OpenType feature tag and its value.
pub type FeatureSetting = (crate::font_settings::FontTag, i32);

/// The computed `font-variant-ligatures`. A field is `None` if the value
/// does not name the group (`normal`), `Some(true)` for the keyword that
/// turns the ligatures on and `Some(false)` for the `no-` keyword. `none`
/// is the same as all four `no-` keywords (Chromium serializes both as
/// `none`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FontVariantLigatures {
    /// `common-ligatures`.
    pub common: Option<bool>,
    /// `discretionary-ligatures`.
    pub discretionary: Option<bool>,
    /// `historical-ligatures`.
    pub historical: Option<bool>,
    /// `contextual`.
    pub contextual: Option<bool>,
}

impl FontVariantLigatures {
    /// `none`.
    pub const NONE: FontVariantLigatures = FontVariantLigatures {
        common: Some(false),
        discretionary: Some(false),
        historical: Some(false),
        contextual: Some(false),
    };

    /// True for `normal`.
    pub fn is_normal(self) -> bool {
        self == FontVariantLigatures::default()
    }

    /// Parses `normal | none | [<common> || <discretionary> || <historical>
    /// || <contextual>]`: each group at most once, in any order.
    pub(crate) fn parse(p: &mut Parser<'_>) -> ParseResult<FontVariantLigatures> {
        p.try_parse(|p| {
            if p.expect_ident_matching("normal").is_ok() {
                return Ok(FontVariantLigatures::default());
            }
            if p.expect_ident_matching("none").is_ok() {
                return Ok(FontVariantLigatures::NONE);
            }
            let mut value = FontVariantLigatures::default();
            let mut any = false;
            while !p.is_exhausted() {
                let word = p.expect_ident()?.to_ascii_lowercase();
                match value.add_keyword(&word) {
                    Some(Ok(())) => any = true,
                    Some(Err(())) => return Err(ParseError::Invalid),
                    None => return Err(ParseError::Unexpected),
                }
            }
            if any {
                Ok(value)
            } else {
                Err(ParseError::EndOfInput)
            }
        })
    }

    /// Adds one keyword (lower case): `None` if it is not a ligature
    /// keyword, `Some(Err)` if its group is already set.
    pub(crate) fn add_keyword(&mut self, word: &str) -> Option<Result<(), ()>> {
        let (slot, on) = match word {
            "common-ligatures" => (&mut self.common, true),
            "no-common-ligatures" => (&mut self.common, false),
            "discretionary-ligatures" => (&mut self.discretionary, true),
            "no-discretionary-ligatures" => (&mut self.discretionary, false),
            "historical-ligatures" => (&mut self.historical, true),
            "no-historical-ligatures" => (&mut self.historical, false),
            "contextual" => (&mut self.contextual, true),
            "no-contextual" => (&mut self.contextual, false),
            _ => return None,
        };
        Some(if slot.replace(on).is_some() {
            Err(())
        } else {
            Ok(())
        })
    }

    /// The computed value as Chromium writes it.
    pub fn as_text(self) -> String {
        if self.is_normal() {
            return "normal".to_owned();
        }
        if self == FontVariantLigatures::NONE {
            return "none".to_owned();
        }
        let words = [
            (self.common, "common-ligatures", "no-common-ligatures"),
            (
                self.discretionary,
                "discretionary-ligatures",
                "no-discretionary-ligatures",
            ),
            (
                self.historical,
                "historical-ligatures",
                "no-historical-ligatures",
            ),
            (self.contextual, "contextual", "no-contextual"),
        ];
        let list: Vec<&str> = words
            .iter()
            .filter_map(|&(v, on, off)| v.map(|v| if v { on } else { off }))
            .collect();
        list.join(" ")
    }

    /// The OpenType features that the value sets (CSS Fonts 4 §6.4:
    /// `none` also turns off the discretionary, historical and contextual
    /// features).
    pub fn features(self) -> Vec<FeatureSetting> {
        let mut out = Vec::new();
        let mut set = |tags: &[&[u8; 4]], on: Option<bool>| {
            if let Some(on) = on {
                out.extend(tags.iter().map(|t| (**t, i32::from(on))));
            }
        };
        set(&[b"liga", b"clig"], self.common);
        set(&[b"dlig"], self.discretionary);
        set(&[b"hlig"], self.historical);
        set(&[b"calt"], self.contextual);
        out
    }
}

/// The computed `font-variant-numeric`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FontVariantNumeric {
    /// `lining-nums` (`Some(false)`) or `oldstyle-nums` (`Some(true)`).
    pub oldstyle: Option<bool>,
    /// `proportional-nums` (`Some(false)`) or `tabular-nums`
    /// (`Some(true)`).
    pub tabular: Option<bool>,
    /// `diagonal-fractions` (`Some(false)`) or `stacked-fractions`
    /// (`Some(true)`).
    pub stacked: Option<bool>,
    /// `ordinal`.
    pub ordinal: bool,
    /// `slashed-zero`.
    pub slashed_zero: bool,
}

impl FontVariantNumeric {
    /// True for `normal`.
    pub fn is_normal(self) -> bool {
        self == FontVariantNumeric::default()
    }

    /// Parses `normal | [<figure> || <spacing> || <fraction> || ordinal ||
    /// slashed-zero]`: each group at most once, in any order.
    pub(crate) fn parse(p: &mut Parser<'_>) -> ParseResult<FontVariantNumeric> {
        p.try_parse(|p| {
            if p.expect_ident_matching("normal").is_ok() {
                return Ok(FontVariantNumeric::default());
            }
            let mut value = FontVariantNumeric::default();
            let mut any = false;
            while !p.is_exhausted() {
                let word = p.expect_ident()?.to_ascii_lowercase();
                match value.add_keyword(&word) {
                    Some(Ok(())) => any = true,
                    Some(Err(())) => return Err(ParseError::Invalid),
                    None => return Err(ParseError::Unexpected),
                }
            }
            if any {
                Ok(value)
            } else {
                Err(ParseError::EndOfInput)
            }
        })
    }

    /// Adds one keyword (lower case): `None` if it is not a numeric
    /// keyword, `Some(Err)` if its group is already set.
    pub(crate) fn add_keyword(&mut self, word: &str) -> Option<Result<(), ()>> {
        let (slot, on) = match word {
            "lining-nums" => (&mut self.oldstyle, false),
            "oldstyle-nums" => (&mut self.oldstyle, true),
            "proportional-nums" => (&mut self.tabular, false),
            "tabular-nums" => (&mut self.tabular, true),
            "diagonal-fractions" => (&mut self.stacked, false),
            "stacked-fractions" => (&mut self.stacked, true),
            "ordinal" | "slashed-zero" => {
                let flag = if word == "ordinal" {
                    &mut self.ordinal
                } else {
                    &mut self.slashed_zero
                };
                return Some(if std::mem::replace(flag, true) {
                    Err(())
                } else {
                    Ok(())
                });
            }
            _ => return None,
        };
        Some(if slot.replace(on).is_some() {
            Err(())
        } else {
            Ok(())
        })
    }

    /// The computed value as Chromium writes it: figure, spacing,
    /// fraction, `ordinal`, `slashed-zero`.
    pub fn as_text(self) -> String {
        if self.is_normal() {
            return "normal".to_owned();
        }
        let mut words = Vec::new();
        let mut pick = |v: Option<bool>, off: &'static str, on: &'static str| {
            if let Some(v) = v {
                words.push(if v { on } else { off });
            }
        };
        pick(self.oldstyle, "lining-nums", "oldstyle-nums");
        pick(self.tabular, "proportional-nums", "tabular-nums");
        pick(self.stacked, "diagonal-fractions", "stacked-fractions");
        if self.ordinal {
            words.push("ordinal");
        }
        if self.slashed_zero {
            words.push("slashed-zero");
        }
        words.join(" ")
    }

    /// The OpenType features that the value turns on.
    pub fn features(self) -> Vec<FeatureSetting> {
        let mut out = Vec::new();
        if let Some(oldstyle) = self.oldstyle {
            out.push((if oldstyle { *b"onum" } else { *b"lnum" }, 1));
        }
        if let Some(tabular) = self.tabular {
            out.push((if tabular { *b"tnum" } else { *b"pnum" }, 1));
        }
        if let Some(stacked) = self.stacked {
            out.push((if stacked { *b"afrc" } else { *b"frac" }, 1));
        }
        if self.ordinal {
            out.push((*b"ordn", 1));
        }
        if self.slashed_zero {
            out.push((*b"zero", 1));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swb_css::parse_component_values;

    fn ligatures(css: &str) -> Option<String> {
        let values = parse_component_values(css);
        let mut p = Parser::new(&values);
        p.parse_entirely(FontVariantLigatures::parse)
            .ok()
            .map(FontVariantLigatures::as_text)
    }

    fn numeric(css: &str) -> Option<String> {
        let values = parse_component_values(css);
        let mut p = Parser::new(&values);
        p.parse_entirely(FontVariantNumeric::parse)
            .ok()
            .map(FontVariantNumeric::as_text)
    }

    #[test]
    fn ligatures_serialize_in_the_order_of_chromium() {
        assert_eq!(ligatures("normal").unwrap(), "normal");
        assert_eq!(ligatures("none").unwrap(), "none");
        assert_eq!(
            ligatures("contextual common-ligatures").unwrap(),
            "common-ligatures contextual"
        );
        assert_eq!(
            ligatures("historical-ligatures no-contextual common-ligatures").unwrap(),
            "common-ligatures historical-ligatures no-contextual"
        );
        assert_eq!(
            ligatures(
                "no-contextual no-historical-ligatures no-discretionary-ligatures \
                 no-common-ligatures"
            )
            .unwrap(),
            "none"
        );
        assert_eq!(ligatures("common-ligatures no-common-ligatures"), None);
        assert_eq!(ligatures("none common-ligatures"), None);
        assert_eq!(ligatures("common-ligatures common-ligatures"), None);
        assert_eq!(ligatures("foo"), None);
    }

    #[test]
    fn numeric_serializes_in_the_order_of_chromium() {
        assert_eq!(
            numeric("ordinal slashed-zero stacked-fractions oldstyle-nums tabular-nums").unwrap(),
            "oldstyle-nums tabular-nums stacked-fractions ordinal slashed-zero"
        );
        assert_eq!(numeric("lining-nums oldstyle-nums"), None);
        assert_eq!(numeric("ordinal ordinal"), None);
        assert_eq!(numeric("none"), None);
        assert_eq!(numeric("normal").unwrap(), "normal");
    }

    #[test]
    fn features_follow_the_values() {
        let v = FontVariantNumeric {
            tabular: Some(true),
            slashed_zero: true,
            ..Default::default()
        };
        assert_eq!(v.features(), vec![(*b"tnum", 1), (*b"zero", 1)]);
        let l = FontVariantLigatures::NONE;
        assert_eq!(l.features().len(), 5);
        assert!(l.features().iter().all(|f| f.1 == 0));
    }
}
