//! The memory bound of script-made style data (ADR 0028 section 16).
//!
//! The parsed form of CSS is much larger than its text: a declaration of a
//! shorthand becomes one entry per longhand (`all` about 150), and a short
//! token takes a few dozen bytes. So a script that makes rules or sets
//! declarations could take gigabytes with a few megabytes of text. A
//! [`StyleMemory`] counts, for one document, the estimated bytes of the
//! parsed data that scripts made: the rules of `insertRule`, the text of the
//! `style` sheets that scripts made or changed, and the declarations that
//! scripts set through `rule.style`. A change that does not fit is refused.
//!
//! The estimate has two steps. Before the parse, [`estimate_style_bytes`]
//! checks the text (it costs one pass and needs no parse), so that the
//! transient memory of one call stays bounded. After the parse, the charge
//! is the estimate of the data that is kept: [`rule_bytes`] for a parsed
//! rule, and the sum of the costs of the entries for a declaration block
//! that a script writes. The page's own style sheets are not counted: the
//! network and the parser bound them.

use std::cell::Cell;
use std::rc::Rc;

use swb_css::{ComponentValue, CssRule, Declaration, SelectorList};

use super::Entry;
use crate::properties::{DECLARATION_ENTRY_BYTES, declaration_entries};

/// The most estimated bytes of parsed style data that a document's scripts
/// make. A request that does not fit is refused.
pub const MAX_SCRIPT_STYLE_BYTES: usize = 128 << 20;

/// The bytes of a rule object: the shared rule, its entry in the list and in
/// the rule index of the stylist.
const RULE_BYTES: usize = 256;

/// The bytes per selector, and per simple selector or combinator in it.
const SELECTOR_BYTES: usize = 128;
const COMPONENT_BYTES: usize = 96;

/// The bytes of a declaration as parsed (`Declaration` and its name), without
/// its value.
const DECLARATION_BYTES: usize = 96;

/// The bytes of one entry of the cascade block (`DeclarationBlock`) of a
/// rule, with its allocations.
const CASCADE_ENTRY_BYTES: usize = DECLARATION_ENTRY_BYTES + 32;

/// The bytes of one entry of the block of `rule.style` ([`Entry`]), with
/// its allocations.
const CSSOM_ENTRY_BYTES: usize = size_of::<Entry>() + 32;

/// The estimated bytes per `;`, `{` and `,` of a CSS text in the check
/// before the parse (a declaration, a rule, a selector): what the shortest
/// declaration (one longhand, one token) takes in the parsed form. So a text
/// that cannot fit is refused before it is parsed.
pub(crate) const BYTES_PER_ITEM: usize =
    DECLARATION_BYTES + CASCADE_ENTRY_BYTES + CSSOM_ENTRY_BYTES + 3 * size_of::<ComponentValue>();

/// The bytes of one entry of a declaration block that a script wrote: the
/// entry of the block of `rule.style`, and the one that it makes in the
/// cascade block.
pub(crate) const ENTRY_BYTES: usize = CASCADE_ENTRY_BYTES + CSSOM_ENTRY_BYTES;

/// The estimated bytes that the parsed form of the CSS `text` takes, for the
/// check before the parse: the text, and its `;`, `{` and `,` characters.
pub fn estimate_style_bytes(text: &str) -> usize {
    let items = text
        .bytes()
        .filter(|b| matches!(b, b';' | b'{' | b','))
        .count();
    text.len()
        .saturating_mul(2)
        .saturating_add(items.saturating_mul(BYTES_PER_ITEM))
}

/// The bytes of component values: each one takes its own size, and the
/// text and the values inside it. Short tokens dominate (`1 1 1` is five
/// values for five bytes of text).
pub(crate) fn values_bytes(values: &[ComponentValue]) -> usize {
    values
        .iter()
        .map(|value| {
            size_of::<ComponentValue>()
                + match value {
                    ComponentValue::Ident(s)
                    | ComponentValue::AtKeyword(s)
                    | ComponentValue::String(s)
                    | ComponentValue::Url(s) => s.len(),
                    ComponentValue::Hash { value, .. } => value.len(),
                    ComponentValue::Dimension { unit, .. } => unit.len(),
                    ComponentValue::Function(f) => f.name.len() + values_bytes(&f.arguments),
                    ComponentValue::Block(b) => values_bytes(&b.contents),
                    _ => 0,
                }
        })
        .sum()
}

/// The bytes that a declaration whose name has `name_len` bytes and whose
/// value takes `value_bytes` costs without its entries: the declaration as
/// parsed, the copy that the cascade keeps, and the text of the block of
/// `rule.style`.
pub(crate) fn declaration_bytes_without_entries(name_len: usize, value_bytes: usize) -> usize {
    DECLARATION_BYTES + name_len + value_bytes.saturating_mul(3)
}

/// The bytes of the declaration `declaration` as the parsed rule keeps it,
/// with the entries it adds to the cascade block and to the block of
/// `rule.style` (which does not store `all`).
pub(crate) fn declaration_bytes(declaration: &Declaration) -> usize {
    let entries = declaration_entries(&declaration.name);
    let cssom_entries = if declaration.name == "all" {
        0
    } else {
        entries
    };
    declaration_bytes_without_entries(declaration.name.len(), values_bytes(&declaration.value))
        .saturating_add(entries.saturating_mul(CASCADE_ENTRY_BYTES))
        .saturating_add(cssom_entries.saturating_mul(CSSOM_ENTRY_BYTES))
}

/// The bytes of the declarations of a style rule.
pub(crate) fn declarations_bytes(declarations: &[Declaration]) -> usize {
    declarations.iter().map(declaration_bytes).sum()
}

/// The estimated bytes of the selector list `list` with its entries in the
/// rule index.
pub(crate) fn selectors_bytes(list: &SelectorList) -> usize {
    list.len()
        .saturating_mul(SELECTOR_BYTES)
        .saturating_add(list.component_count().saturating_mul(COMPONENT_BYTES))
}

/// The estimated bytes of the parsed rule `rule` with the rules inside it.
pub(crate) fn rule_bytes(rule: &CssRule) -> usize {
    RULE_BYTES
        + match rule {
            CssRule::Style(style) => {
                selectors_bytes(&style.selectors) + declarations_bytes(&style.declarations)
            }
            CssRule::Media(media) => {
                media.media.to_string().len().saturating_mul(16) + rules_bytes(&media.rules)
            }
            CssRule::Supports(supports) => {
                supports.text.len().saturating_mul(16) + rules_bytes(&supports.rules)
            }
            CssRule::Import(import) => {
                import.url.len() + import.media.to_string().len().saturating_mul(16)
            }
            CssRule::FontFace(face) => declarations_bytes(&face.declarations),
            CssRule::Keyframes(keyframes) => {
                keyframes.name.len()
                    + keyframes
                        .keyframes
                        .iter()
                        .map(|k| {
                            k.keys.len() * 8 + RULE_BYTES + declarations_bytes(&k.declarations)
                        })
                        .sum::<usize>()
            }
        }
}

/// The estimated bytes of parsed rules.
pub(crate) fn rules_bytes(rules: &[CssRule]) -> usize {
    rules.iter().map(rule_bytes).sum()
}

/// The estimated bytes of the parsed style data that scripts made in one
/// document, shared by its sheets and rules ([`MAX_SCRIPT_STYLE_BYTES`]).
#[derive(Clone, Debug)]
pub struct StyleMemory(Rc<MemoryCells>);

#[derive(Debug)]
struct MemoryCells {
    limit: usize,
    used: Cell<usize>,
    warned: Cell<bool>,
}

impl Default for StyleMemory {
    fn default() -> Self {
        Self::with_limit(MAX_SCRIPT_STYLE_BYTES)
    }
}

impl StyleMemory {
    /// A memory with its own limit in estimated bytes (tests).
    pub fn with_limit(limit: usize) -> Self {
        StyleMemory(Rc::new(MemoryCells {
            limit,
            used: Cell::new(0),
            warned: Cell::new(false),
        }))
    }

    /// The bytes held now.
    pub fn used(&self) -> usize {
        self.0.used.get()
    }

    /// The bytes that fit now.
    pub fn free(&self) -> usize {
        self.0.limit.saturating_sub(self.0.used.get())
    }

    /// Logs the first refusal of a document (one warning per document).
    fn refused(&self, what: &str) {
        if !self.0.warned.replace(true) {
            log::warn!(
                "{what}: the style data that scripts made ({} of {} estimated bytes) \
                 reaches the limit; refused",
                self.0.used.get(),
                self.0.limit
            );
        }
    }

    /// True if `bytes` fit; a refusal logs one warning per document. Used
    /// for the check of the text before a parse.
    pub fn fits(&self, bytes: usize, what: &str) -> bool {
        let fits = bytes <= self.free();
        if !fits {
            self.refused(what);
        }
        fits
    }

    /// Takes `bytes` for `what` if they fit; the bytes return when the
    /// [`Hold`] drops. A refusal logs one warning per document.
    pub fn reserve(&self, bytes: usize, what: &str) -> Option<Hold> {
        if !self.fits(bytes, what) {
            return None;
        }
        self.0.used.set(self.0.used.get() + bytes);
        Some(Hold {
            memory: self.clone(),
            bytes,
        })
    }

    /// Makes `hold` hold exactly `bytes`: the data it stands for changed
    /// (a script wrote the declarations of a rule), and the new size
    /// replaces the old one. The caller has checked that the growth fits.
    pub(crate) fn resize(&self, hold: &mut Option<Hold>, bytes: usize) {
        if let Some(old) = hold.take() {
            drop(old);
        }
        self.0.used.set(self.0.used.get().saturating_add(bytes));
        *hold = Some(Hold {
            memory: self.clone(),
            bytes,
        });
    }

    /// The bytes that a write of declarations may add: what fits, less the
    /// `existing` bytes that the rule has not been charged for yet.
    pub(crate) fn budget(&self, existing: usize) -> WriteBudget {
        WriteBudget {
            memory: Some(self.clone()),
            free: self.free().saturating_sub(existing),
        }
    }
}

/// Bytes taken from a [`StyleMemory`]; they return when it drops.
#[derive(Debug)]
pub struct Hold {
    memory: StyleMemory,
    bytes: usize,
}

impl Hold {
    /// The bytes held.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Splits `bytes` (at most all that is left) off into a hold of its own.
    pub(crate) fn split(&mut self, bytes: usize) -> Hold {
        let part = bytes.min(self.bytes);
        self.bytes -= part;
        Hold {
            memory: self.memory.clone(),
            bytes: part,
        }
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        let used = &self.memory.0.used;
        used.set(used.get().saturating_sub(self.bytes));
    }
}

/// What a write of the declarations of a rule may add to the style data of
/// the document (ADR 0028 section 16). The block checks the estimated bytes
/// of the entries it would add before it changes.
#[derive(Debug)]
pub struct WriteBudget {
    memory: Option<StyleMemory>,
    free: usize,
}

impl WriteBudget {
    /// A budget without a limit (the `style` attribute of an element).
    pub fn unlimited() -> Self {
        WriteBudget {
            memory: None,
            free: usize::MAX,
        }
    }

    /// True if a write that adds at most `bytes` may go on; a refusal logs
    /// one warning per document.
    pub fn allows(&self, bytes: usize) -> bool {
        let Some(memory) = &self.memory else {
            return true;
        };
        if bytes <= self.free {
            return true;
        }
        memory.refused("rule.style");
        false
    }
}

#[cfg(test)]
mod tests;
