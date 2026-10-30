//! CSSOM declaration blocks for `element.style` (CSSOM §6.6 and §6.7,
//! <https://drafts.csswg.org/cssom/#css-declaration-blocks>, ADR 0028
//! section 10): the block of a `style` attribute, its changes by
//! `setProperty`, `removeProperty` and `cssText`, and its serialization as
//! Chromium writes it; and `CSS.supports` and `CSS.escape`
//! (<https://drafts.csswg.org/css-conditional-3/#the-css-namespace>,
//! <https://drafts.csswg.org/cssom/#the-css.escape()-method>).
//!
//! A block holds longhands only, as Chromium's: a shorthand sets each of
//! its longhands, and reading or serializing a shorthand puts it together
//! from them (`shorthand.rs`). Values are kept as Chromium serializes the
//! specified value (`longhand.rs`, `value.rs`), so a read costs nothing;
//! values are checked with swb's own property parsers, so a property or
//! value that swb does not parse is ignored, as in Chromium for what it
//! does not parse. The cascade parses the attribute text that a block
//! writes; the block is the script's view of it.
//!
//! Deliberate departures from Chromium: the longhands that Chromium has
//! and swb does not parse are missing (`length` is 12 + 5 for `border`
//! with the virtual `border-image-*` longhands, Chromium's font sets 19
//! longhands, swb's 9); `grid`, `grid-template`, `mask`, system fonts in
//! `font` and backgrounds of several layers keep the shorthand as written
//! and their longhands read as empty strings (as for `var()`); the value
//! of a custom property has its whitespace runs collapsed; `all` is not
//! supported.

mod longhand;
mod props;
pub mod resolved;
mod shorthand;
mod value;

use std::collections::HashMap;
use std::sync::Arc;

use swb_css::{ComponentValue, Declaration, SupportsCondition, parse_component_values};

use crate::parse::{ParserContext, is_custom_property_name};
use crate::properties::{CssWideKeyword, has_references, is_supported, parse_declaration};
use props::{Property, TABLE};

pub(crate) use value::format_number;

pub use props::{attribute_names, property_of_attribute};

/// The value of a longhand in a block.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    /// A value (serialized).
    Text(String),
    /// A value that a shorthand left out; the text is what Chromium shows
    /// for it (`initial`, or the initial value for some shorthands).
    Implicit(&'static str),
    /// A CSS-wide keyword.
    Wide(&'static str),
    /// A longhand of a shorthand that is kept as written (with `var()`):
    /// the shorthand's index and its text. The longhand reads as `""`.
    Pending(usize, Arc<str>),
}

impl Value {
    /// The text that `getPropertyValue` gives for the longhand.
    fn text(&self) -> &str {
        match self {
            Value::Text(t) => t,
            Value::Implicit(t) | Value::Wide(t) => t,
            Value::Pending(..) => "",
        }
    }
}

/// The property of a declaration.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Name {
    /// A longhand (index into the table).
    Longhand(usize),
    /// A custom property.
    Custom(Arc<str>),
}

#[derive(Clone, Debug, PartialEq)]
struct Entry {
    name: Name,
    value: Value,
    important: bool,
    /// The shorthand that set the value (Chromium serializes
    /// `font-variant` only where it set the longhand).
    via: Option<usize>,
}

/// A CSS declaration block of a `style` attribute (CSSOM §6.6). See the
/// module documentation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleDeclarations {
    entries: Vec<Entry>,
    /// True in a quirks mode document (the unitless length quirk).
    quirks: bool,
}

/// The CSS-wide keyword of a value.
fn wide_keyword(tokens: &[ComponentValue]) -> Option<&'static str> {
    Some(match CssWideKeyword::from_value(tokens)? {
        CssWideKeyword::Initial => "initial",
        CssWideKeyword::Inherit => "inherit",
        CssWideKeyword::Unset => "unset",
        CssWideKeyword::Revert => match value::trimmed(tokens) {
            [ComponentValue::Ident(s)] if s.eq_ignore_ascii_case("revert-layer") => "revert-layer",
            _ => "revert",
        },
    })
}

/// True if a custom property may have `tokens` as its value: no `!`, `;`
/// or closing bracket at the top level, no bad strings or URLs.
fn valid_custom_value(tokens: &[ComponentValue]) -> bool {
    !tokens.iter().any(|t| {
        matches!(
            t,
            ComponentValue::Delim('!')
                | ComponentValue::Semicolon
                | ComponentValue::BadString
                | ComponentValue::BadUrl
                | ComponentValue::CloseParen
                | ComponentValue::CloseSquare
                | ComponentValue::CloseCurly
        )
    })
}

impl StyleDeclarations {
    /// The block of a `style` attribute's text.
    pub fn parse(text: &str, quirks: bool) -> Self {
        let mut block = StyleDeclarations {
            entries: Vec::new(),
            quirks,
        };
        block.set_css_text(text);
        block
    }

    /// The parser context of the block's values.
    fn context(&self) -> ParserContext {
        ParserContext::author(None, self.quirks)
    }

    /// Replaces the declarations with those of `text` (the `cssText`
    /// setter, CSSOM §6.7.2). As Chromium: a property declared twice keeps
    /// its last declaration, an `!important` one wins over the others, and
    /// the important declarations come after the others.
    pub fn set_css_text(&mut self, text: &str) {
        let mut parsed: Vec<(Entry, usize)> = Vec::new();
        // The index in `parsed` of each property (a block of many custom
        // properties stays linear).
        let mut index: HashMap<Name, usize> = HashMap::new();
        for (position, declaration) in swb_css::parse_style_attribute(text).into_iter().enumerate()
        {
            let (values, via) = self.expand(&declaration.name, &declaration.value);
            for (name, value) in values {
                let entry = Entry {
                    name,
                    value,
                    important: declaration.important,
                    via,
                };
                match index.get(&entry.name).and_then(|&i| parsed.get_mut(i)) {
                    Some((old, _)) if old.important && !entry.important => {}
                    Some(old) => *old = (entry, position),
                    None => {
                        index.insert(entry.name.clone(), parsed.len());
                        parsed.push((entry, position));
                    }
                }
            }
        }
        let (important, normal): (Vec<_>, Vec<_>) =
            parsed.into_iter().partition(|(e, _)| e.important);
        let mut normal = normal;
        let mut important = important;
        normal.sort_by_key(|(_, p)| *p);
        important.sort_by_key(|(_, p)| *p);
        self.entries = normal
            .into_iter()
            .chain(important)
            .map(|(e, _)| e)
            .collect();
    }

    /// The longhands and values of the declaration `name: tokens` (none if
    /// swb does not parse it), and the shorthand if `name` is one. `name`
    /// is lower case except for custom properties.
    fn expand(&self, name: &str, tokens: &[ComponentValue]) -> (Vec<(Name, Value)>, Option<usize>) {
        let tokens = value::trimmed(tokens);
        if is_custom_property_name(name) {
            if !valid_custom_value(tokens) {
                return (Vec::new(), None);
            }
            let value = wide_keyword(tokens).map_or_else(
                || Value::Text(swb_css::serialize_component_values(tokens)),
                Value::Wide,
            );
            return (vec![(Name::Custom(Arc::from(name)), value)], None);
        }
        let Some(property) = TABLE.lookup(name) else {
            return (Vec::new(), None);
        };
        let cx = self.context();
        if !parse_declaration(name, tokens, &cx, &mut Vec::new()) {
            return (Vec::new(), None);
        }
        let wide = wide_keyword(tokens);
        let references = has_references(tokens);
        match property {
            Property::Longhand(i) => {
                let value = match wide {
                    Some(k) => Value::Wide(k),
                    None if references => Value::Text(swb_css::serialize_component_values(tokens)),
                    None => {
                        let ser = TABLE.longhands.get(i).map_or(props::Ser::Plain, |l| l.ser);
                        Value::Text(longhand::serialize_longhand(ser, tokens))
                    }
                };
                (vec![(Name::Longhand(i), value)], None)
            }
            Property::Shorthand(s) => {
                let Some(def) = TABLE.shorthand(s) else {
                    return (Vec::new(), None);
                };
                let values = match wide {
                    Some(k) => vec![Value::Wide(k); def.longhands.len()],
                    None if references => {
                        let text: Arc<str> = Arc::from(swb_css::serialize_component_values(tokens));
                        vec![Value::Pending(s, text); def.longhands.len()]
                    }
                    None => shorthand::expand(s, def, tokens, self.quirks),
                };
                let values = def
                    .longhands
                    .iter()
                    .zip(values)
                    .filter_map(|(l, v)| Some((Name::Longhand(TABLE.longhand(l)?), v)))
                    .collect();
                (values, Some(s))
            }
        }
    }

    /// The name of a property as the block knows it: lower case unless
    /// custom.
    fn normalize(property: &str) -> String {
        if is_custom_property_name(property) {
            property.to_owned()
        } else {
            property.to_ascii_lowercase()
        }
    }

    /// `setProperty(property, value, priority)` (CSSOM §6.7.2). Returns
    /// true if the block changed (the attribute must be written).
    pub fn set_property(&mut self, property: &str, value: &str, priority: &str) -> bool {
        let property = Self::normalize(property);
        if !is_custom_property_name(&property) && TABLE.lookup(&property).is_none() {
            return false;
        }
        if value.is_empty() {
            return self.remove_property(&property).1;
        }
        let important = if priority.is_empty() {
            false
        } else if priority.eq_ignore_ascii_case("important") {
            true
        } else {
            return false;
        };
        let tokens = parse_component_values(value);
        let (values, via) = self.expand(&property, &tokens);
        let mut changed = false;
        for (name, value) in values {
            changed |= self.set_declaration(Entry {
                name,
                value,
                important,
                via,
            });
        }
        changed
    }

    /// CSSOM "set a CSS declaration": the value replaces that of the
    /// property in its place, or the declaration goes to the end.
    fn set_declaration(&mut self, new: Entry) -> bool {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.name == new.name) {
            if *entry == new {
                return false;
            }
            *entry = new;
            return true;
        }
        self.entries.push(new);
        true
    }

    /// `removeProperty(property)`: the old value and whether the block
    /// changed. The old value is that of `getPropertyValue` for a longhand
    /// and `""` for a shorthand (Chromium; CSSOM says the shorthand's
    /// value).
    pub fn remove_property(&mut self, property: &str) -> (String, bool) {
        let property = Self::normalize(property);
        let old = match TABLE.lookup(&property) {
            Some(Property::Shorthand(_)) => String::new(),
            _ => self.property_value(&property),
        };
        let names = Self::names_of(&property);
        let before = self.entries.len();
        self.entries.retain(|e| !names.contains(&e.name));
        (old, self.entries.len() != before)
    }

    /// The longhands (or the custom property) of a property name.
    fn names_of(property: &str) -> Vec<Name> {
        if is_custom_property_name(property) {
            return vec![Name::Custom(Arc::from(property))];
        }
        if let Some(i) = TABLE.longhand(property) {
            return vec![Name::Longhand(i)];
        }
        match TABLE.lookup(property) {
            Some(Property::Longhand(i)) => vec![Name::Longhand(i)],
            Some(Property::Shorthand(s)) => TABLE
                .shorthand(s)
                .map(|d| {
                    d.longhands
                        .iter()
                        .filter_map(|l| TABLE.longhand(l).map(Name::Longhand))
                        .collect()
                })
                .unwrap_or_default(),
            None => Vec::new(),
        }
    }

    fn entry(&self, name: &Name) -> Option<&Entry> {
        self.entries.iter().find(|e| &e.name == name)
    }

    /// `getPropertyValue(property)`: a longhand's value; a shorthand put
    /// together from its longhands if they are all set with the same
    /// priority; `""` otherwise.
    pub fn property_value(&self, property: &str) -> String {
        let property = Self::normalize(property);
        if let Some(Property::Shorthand(s)) = TABLE.lookup(&property)
            && TABLE.longhand(&property).is_none()
        {
            return self.shorthand_value(s).map(|(v, _)| v).unwrap_or_default();
        }
        Self::names_of(&property)
            .first()
            .and_then(|n| self.entry(n))
            .map(|e| e.value.text().to_owned())
            .unwrap_or_default()
    }

    /// The value and priority of shorthand `s`, if all its longhands are
    /// set with the same priority.
    fn shorthand_value(&self, s: usize) -> Option<(String, bool)> {
        let def = TABLE.shorthand(s)?;
        let mut values = Vec::with_capacity(def.longhands.len());
        let mut important = None;
        for l in def.longhands {
            let entry = self.entry(&Name::Longhand(TABLE.longhand(l)?))?;
            if *important.get_or_insert(entry.important) != entry.important {
                return None;
            }
            values.push(&entry.value);
        }
        let text = shorthand::value_of(s, def, &values);
        Some((text, important.unwrap_or(false)))
    }

    /// `getPropertyPriority(property)`: `"important"` or `""`.
    pub fn property_priority(&self, property: &str) -> &'static str {
        let property = Self::normalize(property);
        let names = Self::names_of(&property);
        let all_important = !names.is_empty()
            && names
                .iter()
                .all(|n| self.entry(n).is_some_and(|e| e.important));
        if all_important { "important" } else { "" }
    }

    /// The number of declarations (`length`).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True if the block has no declarations.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The property name of declaration `index` (`item()`).
    pub fn item(&self, index: usize) -> Option<String> {
        self.entries.get(index).map(|e| match &e.name {
            Name::Longhand(i) => TABLE.longhand_name(*i).to_owned(),
            Name::Custom(n) => n.to_string(),
        })
    }

    /// `cssText` (CSSOM §6.6.2 "serialize a CSS declaration block"): each
    /// declaration in order, shorthands where all their longhands are set
    /// with the same priority and the shorthand has a value.
    pub fn css_text(&self) -> String {
        let mut out: Vec<String> = Vec::new();
        let mut done = vec![false; self.entries.len()];
        let declaration = |name: &str, value: &str, important: bool| {
            let mut s = format!("{name}: {value}");
            if important {
                s.push_str(" !important");
            }
            s.push(';');
            s
        };
        for (index, entry) in self.entries.iter().enumerate() {
            if done.get(index).copied().unwrap_or(true) {
                continue;
            }
            let longhand = match &entry.name {
                Name::Custom(n) => {
                    out.push(declaration(n, entry.value.text(), entry.important));
                    continue;
                }
                Name::Longhand(i) => *i,
            };
            let mut serialized = false;
            for &s in TABLE.shorthands_of.get(longhand).into_iter().flatten() {
                let Some(def) = TABLE.shorthand(s) else {
                    continue;
                };
                // A shorthand of one longhand of swb's (Chromium's have more)
                // only where it set the value.
                if def.kind == props::Kind::Single && entry.via != Some(s) {
                    continue;
                }
                let positions: Option<Vec<usize>> = def
                    .longhands
                    .iter()
                    .map(|l| {
                        let i = TABLE.longhand(l)?;
                        let p = self
                            .entries
                            .iter()
                            .position(|e| e.name == Name::Longhand(i))?;
                        (!done.get(p).copied().unwrap_or(true)).then_some(p)
                    })
                    .collect();
                let Some(positions) = positions else {
                    continue;
                };
                let Some((value, important)) = self.shorthand_value(s) else {
                    continue;
                };
                if value.is_empty() {
                    continue;
                }
                out.push(declaration(def.name, &value, important));
                for p in positions {
                    if let Some(d) = done.get_mut(p) {
                        *d = true;
                    }
                }
                serialized = true;
                break;
            }
            if !serialized {
                out.push(declaration(
                    TABLE.longhand_name(longhand),
                    entry.value.text(),
                    entry.important,
                ));
                if let Some(d) = done.get_mut(index) {
                    *d = true;
                }
            }
        }
        out.join(" ")
    }
}

/// `CSS.supports(property, value)`: true if swb parses `value` for the
/// property (a custom property takes any value). The name is not case
/// sensitive except for custom properties; a value with `!important` is
/// not supported.
pub fn supports_property(property: &str, value: &str) -> bool {
    let tokens = parse_component_values(value);
    let tokens = value::trimmed(&tokens);
    if is_custom_property_name(property) {
        return valid_custom_value(tokens);
    }
    let name = property.to_ascii_lowercase();
    if TABLE.lookup(&name).is_none() || tokens.is_empty() {
        return false;
    }
    is_supported(&Declaration {
        name,
        value: tokens.to_vec(),
        important: false,
    })
}

/// `CSS.supports(conditionText)`: the text as a `<supports-condition>`, or
/// else wrapped in parentheses (a bare declaration). A declaration may be
/// `!important` (Chromium, measured; the two-argument form takes no
/// priority).
pub fn supports_condition(text: &str) -> bool {
    let evaluate = |condition: &SupportsCondition| {
        condition.evaluate(&|d: &Declaration| {
            supports_property(&d.name, &swb_css::serialize_component_values(&d.value))
        })
    };
    if let Ok(condition) = SupportsCondition::parse_str(text)
        && evaluate(&condition)
    {
        return true;
    }
    SupportsCondition::parse_str(&format!("({text})")).is_ok_and(|c| evaluate(&c))
}

/// `CSS.escape(ident)` (CSSOM §2.1 "serialize an identifier").
pub fn escape_identifier(ident: &str) -> String {
    let mut out = String::new();
    swb_css::serialize_identifier(ident, &mut out);
    out
}

#[cfg(test)]
mod tests;
