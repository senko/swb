//! The names of the Unicode properties that regular expressions accept in
//! `\p{...}` and `\P{...}` (ECMA-262 §22.2.2.9.7 `UnicodeMatchProperty` and
//! §22.2.2.9.8 `UnicodeMatchPropertyValue`,
//! <https://tc39.es/ecma262/#sec-unicodematchproperty-p>).
//!
//! Names match exactly: case, `_` and `-` matter, and there is no `Is`
//! prefix. The tables in `property_names.rs` are generated from
//! `PropertyAliases.txt` and `PropertyValueAliases.txt`; the property
//! lists come from ECMA-262 tables 65 to 67. A [`Property`] names the
//! canonical property and value by index, so that the code point sets can
//! use the same indices.

use super::property_names as t;

/// A property (with a value for the non-binary ones) that a regular
/// expression can test.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Property {
    /// `General_Category` with the value at this index of
    /// [`general_category_name`].
    GeneralCategory(u8),
    /// `Script` with the value at this index of [`script_name`].
    Script(u16),
    /// `Script_Extensions` with the value at this index of [`script_name`].
    ScriptExtensions(u16),
    /// The binary property at this index of [`binary_name`].
    Binary(u8),
    /// The binary property of strings at this index of [`string_name`]
    /// (only with the `v` flag).
    Strings(u8),
}

impl Property {
    /// Whether the property can match strings of more than one code point
    /// (`MayContainStrings`, §22.2.1.8).
    pub fn has_strings(self) -> bool {
        matches!(self, Property::Strings(_))
    }
}

fn find<I: Copy>(table: &[(&str, I)], name: &str) -> Option<I> {
    table
        .binary_search_by(|&(key, _)| key.cmp(name))
        .ok()
        .and_then(|i| table.get(i))
        .map(|&(_, index)| index)
}

/// The `General_Category` value with this name or alias.
pub fn general_category(name: &str) -> Option<u8> {
    find(&t::GENERAL_CATEGORY_NAMES, name)
}

/// The `Script` (and `Script_Extensions`) value with this name or alias.
pub fn script(name: &str) -> Option<u16> {
    find(&t::SCRIPT_NAMES, name)
}

/// The binary property with this name or alias.
pub fn binary(name: &str) -> Option<u8> {
    find(&t::BINARY_PROPERTY_NAMES, name)
}

/// The binary property of strings with this name (they have no aliases).
pub fn string_property(name: &str) -> Option<u8> {
    t::STRING_PROPERTIES
        .iter()
        .position(|&n| n == name)
        .and_then(|i| u8::try_from(i).ok())
}

/// The property of `\p{name=value}` (with `value`) or `\p{name}`: a
/// non-binary property with a value, or, without a value, a
/// `General_Category` value, a binary property or a property of strings.
/// The caller rejects properties of strings without the `v` flag.
pub fn lookup(name: &str, value: Option<&str>) -> Option<Property> {
    match value {
        Some(value) => match name {
            "General_Category" | "gc" => general_category(value).map(Property::GeneralCategory),
            "Script" | "sc" => script(value).map(Property::Script),
            "Script_Extensions" | "scx" => script(value).map(Property::ScriptExtensions),
            _ => None,
        },
        None => general_category(name)
            .map(Property::GeneralCategory)
            .or_else(|| binary(name).map(Property::Binary))
            .or_else(|| string_property(name).map(Property::Strings)),
    }
}

/// The short name of a `General_Category` value.
pub fn general_category_name(index: u8) -> Option<&'static str> {
    t::GENERAL_CATEGORIES.get(usize::from(index)).copied()
}

/// The short name of a `Script` value.
pub fn script_name(index: u16) -> Option<&'static str> {
    t::SCRIPTS.get(usize::from(index)).copied()
}

/// The canonical name of a binary property.
pub fn binary_name(index: u8) -> Option<&'static str> {
    t::BINARY_PROPERTIES.get(usize::from(index)).copied()
}

/// The name of a binary property of strings.
pub fn string_name(index: u8) -> Option<&'static str> {
    t::STRING_PROPERTIES.get(usize::from(index)).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_sorted_for_binary_search() {
        for table in [
            &t::GENERAL_CATEGORY_NAMES[..],
            &t::BINARY_PROPERTY_NAMES[..],
        ] {
            assert!(table.windows(2).all(|w| w[0].0 < w[1].0));
        }
        assert!(t::SCRIPT_NAMES.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn names_and_aliases() {
        let lu = general_category("Lu").expect("Lu is a value");
        assert_eq!(general_category("Uppercase_Letter"), Some(lu));
        assert_eq!(general_category_name(lu), Some("Lu"));
        assert_eq!(general_category("lu"), None);
        let p = general_category("P").expect("P is a value");
        assert_eq!(general_category("punct"), Some(p));
        assert_eq!(general_category("L&"), None);
        assert!(general_category("LC").is_some());
        let latn = script("Latn").expect("Latn is a script");
        assert_eq!(script("Latin"), Some(latn));
        assert_eq!(script_name(latn), Some("Latn"));
        assert_eq!(script("Inherited"), script("Qaai"));
        assert_eq!(script("Zinh"), script("Qaai"));
        assert!(script("Sidt").is_some(), "a script of Unicode 17");
        assert_eq!(binary("space"), binary("White_Space"));
        assert_eq!(binary("WSpace"), binary("White_Space"));
        assert!(binary("ASCII").is_some() && binary("Any").is_some());
        assert_eq!(binary("ascii"), None);
        assert_eq!(binary("IDS_Unary_Operator"), None, "not in table 66");
        assert_eq!(
            binary_name(binary("Alpha").expect("alias")),
            Some("Alphabetic")
        );
    }

    #[test]
    fn lookup_forms() {
        assert_eq!(
            lookup("gc", Some("Lu")),
            general_category("Lu").map(Property::GeneralCategory)
        );
        assert_eq!(lookup("Lu", None), lookup("General_Category", Some("Lu")));
        assert!(matches!(
            lookup("scx", Some("Zyyy")),
            Some(Property::ScriptExtensions(_))
        ));
        assert_eq!(lookup("Script", Some("Foo")), None);
        assert_eq!(lookup("ASCII", Some("Y")), None);
        assert_eq!(lookup("Block", Some("Basic_Latin")), None);
        assert_eq!(lookup("Latin", None), None);
        let emoji = lookup("RGI_Emoji", None).expect("a property of strings");
        assert!(emoji.has_strings());
        assert_eq!(
            lookup("Emoji", None).map(Property::has_strings),
            Some(false)
        );
    }
}
