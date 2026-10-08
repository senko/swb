//! Font queries: the CSS properties that select a font, and the family
//! mapping that directory mode uses instead of fontconfig.

/// A CSS generic font family.
///
/// See <https://www.w3.org/TR/css-fonts-4/#generic-font-families>.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum GenericFamily {
    /// `serif`
    Serif,
    /// `sans-serif`
    SansSerif,
    /// `monospace`
    Monospace,
    /// `cursive`
    Cursive,
    /// `fantasy`
    Fantasy,
    /// `system-ui`
    SystemUi,
    /// `math`
    Math,
    /// `emoji`
    Emoji,
}

impl GenericFamily {
    /// All generic families, in declaration order.
    pub const ALL: [GenericFamily; 8] = [
        GenericFamily::Serif,
        GenericFamily::SansSerif,
        GenericFamily::Monospace,
        GenericFamily::Cursive,
        GenericFamily::Fantasy,
        GenericFamily::SystemUi,
        GenericFamily::Math,
        GenericFamily::Emoji,
    ];

    /// The CSS keyword for this family. fontconfig uses the same names as
    /// aliases.
    pub fn css_name(self) -> &'static str {
        match self {
            GenericFamily::Serif => "serif",
            GenericFamily::SansSerif => "sans-serif",
            GenericFamily::Monospace => "monospace",
            GenericFamily::Cursive => "cursive",
            GenericFamily::Fantasy => "fantasy",
            GenericFamily::SystemUi => "system-ui",
            GenericFamily::Math => "math",
            GenericFamily::Emoji => "emoji",
        }
    }
}

/// One entry of a CSS `font-family` list.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum FamilyName<'a> {
    /// A family name such as `"Helvetica Neue"`.
    Named(&'a str),
    /// A generic family keyword.
    Generic(GenericFamily),
}

/// The CSS `font-style` value.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum FontStyle {
    /// `normal`
    #[default]
    Normal,
    /// `italic`
    Italic,
    /// `oblique` (the angle is ignored).
    Oblique,
}

/// The font properties that select a font.
#[derive(Copy, Clone, Debug)]
pub struct FontQuery<'a> {
    /// The `font-family` list, in priority order.
    pub families: &'a [FamilyName<'a>],
    /// The `font-weight` value, 1 to 1000.
    pub weight: f32,
    /// The `font-style` value.
    pub style: FontStyle,
    /// The `font-stretch` value as a percentage (100 is normal).
    pub stretch: f32,
    /// The `font-variation-settings` value: (axis tag, value), sorted by
    /// tag. They are applied after the axis values that `weight`,
    /// `stretch` and `style` select (CSS Fonts 4 §7), to the axes that the
    /// font has, clamped to the axis range.
    pub variations: &'a [([u8; 4], f32)],
    /// The content language (BCP 47, for example `"en-US"`), if known.
    /// It affects system fallback only; shaping takes the language from
    /// [`ShapeOptions::language`](crate::ShapeOptions::language).
    pub language: Option<&'a str>,
}

impl<'a> FontQuery<'a> {
    /// A query for `families` with normal weight, style and stretch.
    pub fn new(families: &'a [FamilyName<'a>]) -> Self {
        FontQuery {
            families,
            weight: 400.0,
            style: FontStyle::Normal,
            stretch: 100.0,
            variations: &[],
            language: None,
        }
    }

    /// The weight, clamped to 1..=1000. NaN becomes 400.
    pub(crate) fn clamped_weight(&self) -> f32 {
        if self.weight.is_nan() {
            400.0
        } else {
            self.weight.clamp(1.0, 1000.0)
        }
    }

    /// The stretch, clamped to 50..=200 percent. NaN becomes 100.
    pub(crate) fn clamped_stretch(&self) -> f32 {
        if self.stretch.is_nan() {
            100.0
        } else {
            self.stretch.clamp(50.0, 200.0)
        }
    }
}

/// Family configuration for
/// [`FontContext::from_directory`](crate::FontContext::from_directory),
/// which does not use fontconfig.
///
/// Family names are compared without regard to ASCII case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenericFamilyMap {
    /// The family for each generic family. Generic families that are not in
    /// this list use `default_family`.
    pub generics: Vec<(GenericFamily, String)>,
    /// The family for unlisted generic families. Queries where no family
    /// in the list exists use the `serif` family instead (Blink's standard
    /// font), and this family only if that is missing.
    pub default_family: String,
    /// Families to try, in order, for characters that the requested families
    /// do not cover. After these, all fonts in the directory are tried in
    /// order of file path.
    pub fallback: Vec<String>,
    /// Substitutes for named families that are not in the directory, as
    /// (requested family, substitute family). This mirrors the
    /// metric-compatible aliases of a fontconfig configuration.
    pub aliases: Vec<(String, String)>,
}

impl GenericFamilyMap {
    /// The mapping for the bundled test fonts in `fixtures/fonts`. It agrees
    /// with `fixtures/fonts/fonts.conf`.
    pub fn bundled() -> Self {
        let sans = "Liberation Sans";
        let serif = "Liberation Serif";
        let mono = "Liberation Mono";
        let aliases = [
            ("Arial", sans),
            ("Arimo", sans),
            ("Helvetica", sans),
            ("Times New Roman", serif),
            ("Tinos", serif),
            ("Times", serif),
            ("Courier New", mono),
            ("Cousine", mono),
            ("Courier", mono),
        ];
        GenericFamilyMap {
            generics: vec![
                (GenericFamily::Serif, serif.to_owned()),
                (GenericFamily::SansSerif, sans.to_owned()),
                (GenericFamily::Monospace, mono.to_owned()),
            ],
            default_family: sans.to_owned(),
            fallback: [sans, serif, mono, "DejaVu Sans"]
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
            aliases: aliases
                .iter()
                .map(|(from, to)| ((*from).to_owned(), (*to).to_owned()))
                .collect(),
        }
    }

    /// The family name for a generic family.
    pub(crate) fn generic(&self, generic: GenericFamily) -> &str {
        self.generics
            .iter()
            .find(|(g, _)| *g == generic)
            .map_or(self.default_family.as_str(), |(_, name)| name.as_str())
    }

    /// The substitute for a named family, if there is one.
    pub(crate) fn alias(&self, name: &str) -> Option<&str> {
        self.aliases
            .iter()
            .find(|(from, _)| from.eq_ignore_ascii_case(name))
            .map(|(_, to)| to.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_map_resolves_generics() {
        let map = GenericFamilyMap::bundled();
        assert_eq!(map.generic(GenericFamily::Serif), "Liberation Serif");
        assert_eq!(map.generic(GenericFamily::Monospace), "Liberation Mono");
        assert_eq!(map.generic(GenericFamily::Cursive), "Liberation Sans");
        assert_eq!(map.alias("arial"), Some("Liberation Sans"));
        assert_eq!(map.alias("Comic Sans MS"), None);
    }

    #[test]
    fn query_clamps_values() {
        let mut query = FontQuery::new(&[]);
        query.weight = f32::NAN;
        query.stretch = 500.0;
        assert_eq!(query.clamped_weight(), 400.0);
        assert_eq!(query.clamped_stretch(), 200.0);
    }
}
