//! The CSSOM model of style sheets and rules (CSSOM §6.1 "CSS Style
//! Sheets" and §6.4 "CSS Rules", <https://drafts.csswg.org/cssom/>, ADR 0028
//! section 10).
//!
//! A [`StyleSheet`] is the parsed form of a `style` element's text or of a
//! `link` element's response, and at the same time the object behind a
//! script's `CSSStyleSheet`: the engine builds the stylist from its rules,
//! and the bindings read and change them. Both hold it through a
//! [`SheetRef`]; each rule is a [`RuleRef`], so that a rule keeps its
//! identity while rules before it come and go, and a removed rule or sheet
//! keeps its data for the script that holds it (CSSOM: a removed rule keeps
//! its text, its parent pointers become null). A rule points to its parent
//! rule and a top-level rule to its sheet with weak references: the
//! structure owns downwards only.
//!
//! The parser parses the text once; `insertRule` parses only the new rule.
//! A style rule keeps its parsed declaration block for the cascade
//! ([`StyleRule::cascade_block`]), so a rebuild of the stylist after a
//! change parses no declarations of the other rules again. The block of
//! `CSSStyleRule.style` ([`StyleDeclarations`]) is built at its first use.
//!
//! The rules are those of the stylesheet parser: style rules, `@media`,
//! `@supports`, `@import`, `@font-face`, `@keyframes` and the cascade layer
//! rules (`@layer` blocks, which hold rules, and `@layer` statements). Rules
//! that the parser drops (`@page`, `@namespace`, ...) are not in the lists
//! (a departure from Chromium, which has an object for each).

use std::cell::RefCell;
use std::fmt::Write as _;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use swb_css::{
    ComponentValue, CssRule, Declaration, FontFaceRule, ImportLayer, ImportRule, KeyframesRule,
    LayerName, MediaQueryList, SelectorList, SupportsCondition, parse_rule, parse_stylesheet,
    serialize_component_values, serialize_identifier, serialize_string,
};
use swb_dom::NodeId;
use url::Url;

pub use super::memory::{
    Hold, MAX_SCRIPT_STYLE_BYTES, StyleMemory, WriteBudget, estimate_style_bytes,
};
use super::memory::{declarations_bytes, rule_bytes, rules_bytes, selectors_bytes};
use super::{StyleDeclarations, format_number};
use crate::parse::ParserContext;
use crate::properties::DeclarationBlock;

/// A shared style sheet.
pub type SheetRef = Rc<RefCell<StyleSheet>>;

/// A shared rule.
pub type RuleRef = Rc<RefCell<Rule>>;

/// A CSS style sheet (CSSOM §6.1).
#[derive(Debug)]
pub struct StyleSheet {
    /// The top-level rules.
    rules: Vec<RuleRef>,
    /// The media of the sheet (`media`): the `media` attribute of its owner,
    /// or what a script set. The sheet applies only where it matches.
    pub media: MediaQueryList,
    /// The disabled flag (`disabled`): a disabled sheet does not apply.
    pub disabled: bool,
    /// The owner node (`ownerNode`): the `style` or `link` element, or
    /// `None` once the sheet left it.
    pub owner: Option<NodeId>,
    /// The location (`href`) of a `link` element's sheet.
    pub href: Option<String>,
    /// The title (`title`) from the owner's `title` attribute; `None` when
    /// it is missing or empty.
    pub title: Option<String>,
    /// The origin-clean flag: false for a sheet of another origin, whose
    /// rules scripts cannot read or change (`SecurityError`).
    pub origin_clean: bool,
    /// True in a quirks mode document (the unitless length quirk of the
    /// declaration blocks).
    quirks: bool,
    /// True after a script changed the rules (an insertion, a deletion, a
    /// selector or a declaration of a rule).
    modified: bool,
    /// The memory of the document's style data, shared with the rules.
    memory: StyleMemory,
    /// The estimated bytes of the rules of a sheet that a script made,
    /// without the declarations of their style rules (those hold their own);
    /// the page's own sheets hold none.
    hold: Option<Hold>,
}

/// A CSS rule (CSSOM §6.4).
#[derive(Debug)]
pub struct Rule {
    /// The kind of rule and its data.
    pub kind: RuleKind,
    /// The parent rule (`parentRule`) of a rule inside a grouping rule.
    parent: Weak<RefCell<Rule>>,
    /// The sheet of a top-level rule; empty for nested and removed rules.
    sheet: Weak<RefCell<StyleSheet>>,
    /// The memory of the document's style data, shared with the sheet.
    memory: StyleMemory,
    /// The estimated bytes of a rule that a script inserted, with the rules
    /// inside it but without the declarations of its style rules (those hold
    /// their own, [`StyleRule::decl_hold`]).
    hold: Option<Hold>,
}

/// The kinds of rules and their data.
#[derive(Debug)]
pub enum RuleKind {
    /// A style rule (`CSSStyleRule`).
    Style(StyleRule),
    /// `@media` (`CSSMediaRule`).
    Media {
        /// The media query list.
        media: MediaQueryList,
        /// The rules inside.
        rules: Vec<RuleRef>,
    },
    /// `@supports` (`CSSSupportsRule`).
    Supports {
        /// The condition.
        condition: SupportsCondition,
        /// The condition as written (`conditionText`).
        text: String,
        /// The rules inside.
        rules: Vec<RuleRef>,
    },
    /// `@import` (`CSSImportRule`). swb does not load imported sheets.
    Import(ImportRule),
    /// `@font-face` (`CSSFontFaceRule`).
    FontFace(FontFaceRule),
    /// `@keyframes` (`CSSKeyframesRule`). swb runs no animations.
    Keyframes(KeyframesRule),
    /// `@layer name { ... }` and `@layer { ... }` (`CSSLayerBlockRule`).
    LayerBlock {
        /// The name; `None` for an anonymous layer.
        name: Option<LayerName>,
        /// The rules inside.
        rules: Vec<RuleRef>,
    },
    /// `@layer a, b;` (`CSSLayerStatementRule`).
    LayerStatement {
        /// The names, in source order.
        names: Vec<LayerName>,
    },
}

/// A style rule.
#[derive(Debug)]
pub struct StyleRule {
    /// The selectors.
    selectors: SelectorList,
    /// The declarations, as parsed. Empty and `stale` after a script wrote
    /// the block of `CSSStyleRule.style`: the stylist makes them from that
    /// block at its next use.
    declarations: Vec<Declaration>,
    /// True if the declarations are those of `cssom`, to be made.
    stale: bool,
    /// The block of the cascade with the base URL it was parsed for.
    block: Option<(Arc<Url>, Arc<DeclarationBlock>)>,
    /// The block of `CSSStyleRule.style`, built at its first use.
    cssom: Option<StyleDeclarations>,
    /// The estimated bytes of the declarations of a rule that scripts made
    /// or wrote: as parsed with the cascade's entries, and after a write,
    /// as the block is then (not the sum of the writes).
    decl_hold: Option<Hold>,
    /// The estimated bytes of the selectors that a script set
    /// (`selectorText`), as the list is now (not the sum of the writes).
    selector_hold: Option<Hold>,
}

/// Why a change of a rule list failed. The bindings word the messages
/// (they differ between sheets and grouping rules).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleError {
    /// The index is larger than the list allows (`IndexSizeError`); the
    /// length of the list.
    Index {
        /// The length of the list.
        length: usize,
    },
    /// The text is not exactly one rule that swb keeps (`SyntaxError`).
    Syntax,
    /// The rule may not be at that place (`HierarchyRequestError`):
    /// `@import` after other rules, or a rule before an `@import`.
    Hierarchy,
    /// `@import` inside a grouping rule (`HierarchyRequestError`).
    ImportInGroup,
    /// A grouping rule nested [`MAX_GROUP_DEPTH`] deep gets no more rules
    /// (swb's bound, `HierarchyRequestError`).
    TooDeep,
    /// The style data that scripts made would exceed
    /// [`MAX_SCRIPT_STYLE_BYTES`] (swb's bound, `QuotaExceededError`).
    Memory,
}

/// The most grouping rules around a grouping rule that `insertRule` adds
/// to. The parser keeps at most 64 levels of blocks, so without this bound
/// only scripts (`insertRule` into the innermost rule in a loop) could nest
/// rules deeper, and the serialization and the stylist recurse along the
/// nesting (swb's bound; Chromium has none).
pub const MAX_GROUP_DEPTH: usize = 64;

/// The type constants of `CSSRule.type`.
pub mod rule_type {
    /// `STYLE_RULE`.
    pub const STYLE: u16 = 1;
    /// `IMPORT_RULE`.
    pub const IMPORT: u16 = 3;
    /// `MEDIA_RULE`.
    pub const MEDIA: u16 = 4;
    /// `FONT_FACE_RULE`.
    pub const FONT_FACE: u16 = 5;
    /// `KEYFRAMES_RULE`.
    pub const KEYFRAMES: u16 = 7;
    /// The type of the rules that have no legacy constant (`CSSLayerBlockRule`
    /// and `CSSLayerStatementRule`; Chromium 148 gives 0).
    pub const NONE: u16 = 0;
    /// `SUPPORTS_RULE`.
    pub const SUPPORTS: u16 = 12;
}

impl StyleSheet {
    /// A sheet of the rules of `css`, without an owner, for a document of
    /// its own memory.
    #[cfg(test)]
    pub(crate) fn parse(css: &str, quirks: bool) -> SheetRef {
        Self::parse_in(css, quirks, &StyleMemory::default(), false)
    }

    /// A sheet of the rules of `css`, without an owner. `script` is true
    /// for the text of a sheet that a script made or changed: the text must
    /// pass the check of [`estimate_style_bytes`] before the parse, the
    /// estimated bytes of the parsed rules count in `memory`, and a text
    /// that does not fit gives a sheet without rules.
    pub fn parse_in(css: &str, quirks: bool, memory: &StyleMemory, script: bool) -> SheetRef {
        let sheet = Rc::new(RefCell::new(StyleSheet {
            rules: Vec::new(),
            media: MediaQueryList::default(),
            disabled: false,
            owner: None,
            href: None,
            title: None,
            origin_clean: true,
            quirks,
            modified: false,
            memory: memory.clone(),
            hold: None,
        }));
        let mut budget = None;
        if script && !memory.fits(estimate_style_bytes(css), "style sheet text") {
            return sheet;
        }
        let parsed = parse_stylesheet(css).rules;
        if script {
            budget = memory.reserve(rules_bytes(&parsed), "style sheet text");
            if budget.is_none() {
                return sheet;
            }
        }
        let rules = convert(
            parsed,
            &Weak::new(),
            &Rc::downgrade(&sheet),
            memory,
            &mut budget,
        );
        let mut this = sheet.borrow_mut();
        this.rules = rules;
        this.hold = budget;
        drop(this);
        sheet
    }

    /// The top-level rules (`cssRules`).
    pub fn rules(&self) -> &[RuleRef] {
        &self.rules
    }

    /// The estimated bytes that the sheet's rules hold in the document's
    /// style memory (0 for a sheet of the page's own text), without the
    /// declarations of its style rules.
    #[cfg(test)]
    pub(crate) fn held_bytes(&self) -> usize {
        self.hold.as_ref().map_or(0, Hold::bytes)
    }

    /// True after a script changed the rules.
    #[cfg(test)]
    pub(crate) fn is_modified(&self) -> bool {
        self.modified
    }

    /// Benchmarks only (`restart_pipeline`): parses `css` again into the
    /// rules of a sheet that no script changed; returns false for a changed
    /// one. A benchmark cannot make the sheet again instead: the sheet may
    /// hold rules that scripts inserted, which the timed runs must keep.
    pub fn reparse(sheet: &SheetRef, css: &str) -> bool {
        if sheet.borrow().modified {
            return false;
        }
        let memory = sheet.borrow().memory.clone();
        let rules = convert(
            parse_stylesheet(css).rules,
            &Weak::new(),
            &Rc::downgrade(sheet),
            &memory,
            &mut None,
        );
        let old = std::mem::replace(&mut sheet.borrow_mut().rules, rules);
        for rule in old {
            rule.borrow_mut().sheet = Weak::new();
        }
        true
    }

    /// Benchmarks only (`restart_pipeline`): drops the declaration blocks
    /// that the cascade parsed, so that the next stylist build parses them
    /// again (for a sheet that [`StyleSheet::reparse`] cannot parse again).
    pub fn clear_cascade_blocks(&self) {
        fn clear(rules: &[RuleRef]) {
            for rule in rules {
                let mut rule = rule.borrow_mut();
                match &mut rule.kind {
                    RuleKind::Style(style) => style.block = None,
                    RuleKind::Media { rules, .. }
                    | RuleKind::Supports { rules, .. }
                    | RuleKind::LayerBlock { rules, .. } => {
                        clear(rules);
                    }
                    _ => {}
                }
            }
        }
        clear(&self.rules);
    }

    /// `insertRule(rule, index)` (CSSOM "insert a CSS rule" on the sheet's
    /// list): parses `text` as one rule and inserts it at `index`. Returns
    /// the index.
    pub fn insert_rule(sheet: &SheetRef, text: &str, index: u32) -> Result<u32, RuleError> {
        let length = sheet.borrow().rules.len();
        let at = usize::try_from(index).unwrap_or(usize::MAX);
        if at > length {
            return Err(RuleError::Index { length });
        }
        let memory = sheet.borrow().memory.clone();
        if !memory.fits(estimate_style_bytes(text), "insertRule") {
            return Err(RuleError::Memory);
        }
        let parsed = parse_rule(text).map_err(|_| RuleError::Syntax)?;
        {
            // The `@import` rules are a prefix of the list (this check keeps
            // it so), so the rules at `at - 1` and `at` decide.
            let this = sheet.borrow();
            let is_import = |r: Option<&RuleRef>| {
                r.is_some_and(|r| matches!(r.borrow().kind, RuleKind::Import(_)))
            };
            let allowed = if matches!(parsed, CssRule::Import(_)) {
                at == 0 || is_import(this.rules.get(at - 1))
            } else {
                !is_import(this.rules.get(at))
            };
            if !allowed {
                return Err(RuleError::Hierarchy);
            }
        }
        let rule = convert_inserted(parsed, &Weak::new(), &Rc::downgrade(sheet), &memory)?;
        let mut this = sheet.borrow_mut();
        this.rules.insert(at, rule);
        this.modified = true;
        Ok(index)
    }

    /// `deleteRule(index)` (CSSOM "remove a CSS rule"): removes the rule at
    /// `index`; it keeps its data, its parent sheet becomes null. Returns
    /// the removed rule.
    pub fn delete_rule(sheet: &SheetRef, index: u32) -> Result<RuleRef, RuleError> {
        let mut this = sheet.borrow_mut();
        let length = this.rules.len();
        let at = usize::try_from(index).unwrap_or(usize::MAX);
        if at >= length {
            return Err(RuleError::Index { length });
        }
        let rule = this.rules.remove(at);
        this.modified = true;
        drop(this);
        rule.borrow_mut().sheet = Weak::new();
        Ok(rule)
    }
}

impl Rule {
    /// The `CSSRule.type` constant of the rule.
    pub fn type_number(&self) -> u16 {
        match self.kind {
            RuleKind::Style(_) => rule_type::STYLE,
            RuleKind::Media { .. } => rule_type::MEDIA,
            RuleKind::Supports { .. } => rule_type::SUPPORTS,
            RuleKind::Import(_) => rule_type::IMPORT,
            RuleKind::FontFace(_) => rule_type::FONT_FACE,
            RuleKind::Keyframes(_) => rule_type::KEYFRAMES,
            RuleKind::LayerBlock { .. } | RuleKind::LayerStatement { .. } => rule_type::NONE,
        }
    }

    /// The parent rule (`parentRule`).
    pub fn parent_rule(&self) -> Option<RuleRef> {
        self.parent.upgrade()
    }

    /// The rules inside a grouping rule (`@media`, `@supports`).
    pub fn child_rules(&self) -> Option<&[RuleRef]> {
        match &self.kind {
            RuleKind::Media { rules, .. }
            | RuleKind::Supports { rules, .. }
            | RuleKind::LayerBlock { rules, .. } => Some(rules),
            _ => None,
        }
    }

    /// What a write of the declarations of this style rule (`setProperty`
    /// and the property setters) may add to the style data of the document
    /// (without a limit for other rules). A rule that has no charge yet
    /// (a rule of the page's own sheets) counts its block in what the write
    /// adds.
    pub fn write_budget(&mut self, quirks: bool) -> WriteBudget {
        let Rule { kind, memory, .. } = self;
        let RuleKind::Style(style) = kind else {
            return WriteBudget::unlimited();
        };
        let existing = if style.decl_hold.is_some() {
            0
        } else {
            style.declarations_block(quirks).retained_bytes()
        };
        memory.budget(existing)
    }

    /// A script wrote the block of `CSSStyleRule.style` of this rule: the
    /// stylist takes its declarations at its next use, and the rule's charge
    /// for its declarations becomes that of the block as it is now.
    pub fn declarations_written(&mut self) {
        let Rule { kind, memory, .. } = self;
        if let RuleKind::Style(style) = kind {
            style.written(memory);
        }
    }

    /// `cssText` (CSSOM "serialize a CSS rule"), as Chromium writes it.
    pub fn css_text(&mut self, quirks: bool) -> String {
        match &mut self.kind {
            RuleKind::Style(style) => {
                let selectors = style.selectors.cssom_text();
                let block = style.declarations_block(quirks).css_text();
                if block.is_empty() {
                    format!("{selectors} {{ }}")
                } else {
                    format!("{selectors} {{ {block} }}")
                }
            }
            RuleKind::Media { media, rules } => {
                grouping_text("@media", &media.to_string(), rules, quirks)
            }
            RuleKind::Supports { text, rules, .. } => {
                grouping_text("@supports", text, rules, quirks)
            }
            RuleKind::Import(import) => {
                let mut out = String::from("@import url(");
                serialize_string(&import.url, &mut out);
                out.push(')');
                match &import.layer {
                    Some(ImportLayer::Anonymous) => out.push_str(" layer"),
                    Some(ImportLayer::Named(name)) => {
                        let _ = write!(out, " layer({name})");
                    }
                    None => {}
                }
                if let Some(text) = &import.supports_text {
                    let _ = write!(out, " supports({text})");
                }
                if !import.media.is_empty() {
                    let _ = write!(out, " {}", import.media);
                }
                out.push(';');
                out
            }
            RuleKind::LayerBlock { name, rules } => {
                let keyword = name
                    .as_ref()
                    .map_or_else(|| "@layer".to_owned(), |n| format!("@layer {n}"));
                grouping_block(&keyword, rules, quirks)
            }
            RuleKind::LayerStatement { names } => {
                let names: Vec<String> = names.iter().map(ToString::to_string).collect();
                format!("@layer {};", names.join(", "))
            }
            RuleKind::FontFace(face) => {
                let descriptors: Vec<String> = face
                    .declarations
                    .iter()
                    .map(|d| format!("{}: {};", d.name, descriptor_value(&d.value)))
                    .collect();
                if descriptors.is_empty() {
                    "@font-face { }".to_owned()
                } else {
                    format!("@font-face {{ {} }}", descriptors.join(" "))
                }
            }
            RuleKind::Keyframes(keyframes) => keyframes_text(keyframes, quirks),
        }
    }
}

/// The parent sheet (`parentStyleSheet`) of `rule`: the sheet of its
/// top-level ancestor, or `None` if that rule was removed.
pub fn parent_sheet(rule: &RuleRef) -> Option<SheetRef> {
    let mut current = Rc::clone(rule);
    loop {
        let parent = current.borrow().parent.upgrade();
        match parent {
            Some(parent) => current = parent,
            None => return current.borrow().sheet.upgrade(),
        }
    }
}

/// True in a quirks mode document, for the rule's sheet (false when it
/// has none).
pub fn rule_quirks(rule: &RuleRef) -> bool {
    parent_sheet(rule).is_some_and(|s| s.borrow().quirks)
}

/// `insertRule(rule, index)` of a grouping rule (CSSOM §6.4.4): parses
/// `text` as one rule and inserts it into the rules of `group` at `index`.
pub fn insert_into_group(group: &RuleRef, text: &str, index: u32) -> Result<u32, RuleError> {
    let length = group.borrow().child_rules().map_or(0, <[RuleRef]>::len);
    let at = usize::try_from(index).unwrap_or(usize::MAX);
    if at > length {
        return Err(RuleError::Index { length });
    }
    let memory = group.borrow().memory.clone();
    if !memory.fits(estimate_style_bytes(text), "insertRule") {
        return Err(RuleError::Memory);
    }
    let parsed = parse_rule(text).map_err(|_| RuleError::Syntax)?;
    if matches!(parsed, CssRule::Import(_)) {
        return Err(RuleError::ImportInGroup);
    }
    let mut depth = 0;
    let mut ancestor = group.borrow().parent_rule();
    while let Some(rule) = ancestor {
        depth += 1;
        if depth >= MAX_GROUP_DEPTH {
            log::warn!("insertRule: grouping rules nested {MAX_GROUP_DEPTH} deep; rule refused");
            return Err(RuleError::TooDeep);
        }
        ancestor = rule.borrow().parent_rule();
    }
    let rule = convert_inserted(parsed, &Rc::downgrade(group), &Weak::new(), &memory)?;
    let mut this = group.borrow_mut();
    if let RuleKind::Media { rules, .. }
    | RuleKind::Supports { rules, .. }
    | RuleKind::LayerBlock { rules, .. } = &mut this.kind
    {
        rules.insert(at, rule);
    }
    drop(this);
    mark_modified(group);
    Ok(index)
}

/// `deleteRule(index)` of a grouping rule: removes the rule at `index`.
pub fn delete_from_group(group: &RuleRef, index: u32) -> Result<RuleRef, RuleError> {
    let mut this = group.borrow_mut();
    let Some(rules) = (match &mut this.kind {
        RuleKind::Media { rules, .. }
        | RuleKind::Supports { rules, .. }
        | RuleKind::LayerBlock { rules, .. } => Some(rules),
        _ => None,
    }) else {
        return Err(RuleError::Index { length: 0 });
    };
    let at = usize::try_from(index).unwrap_or(usize::MAX);
    if at >= rules.len() {
        return Err(RuleError::Index {
            length: rules.len(),
        });
    }
    let rule = rules.remove(at);
    drop(this);
    rule.borrow_mut().parent = Weak::new();
    mark_modified(group);
    Ok(rule)
}

/// `rule.style.cssText = text` for the style rule `rule`: replaces the
/// declarations with those of `text`. The text must pass the check of
/// [`estimate_style_bytes`] before the parse, and the estimated bytes of the
/// new block must fit (the old ones return). Returns false, and changes
/// nothing, if they do not ([`MAX_SCRIPT_STYLE_BYTES`]).
pub fn set_declarations_text(rule: &RuleRef, text: &str, quirks: bool) -> bool {
    let (memory, old) = {
        let guard = rule.borrow();
        let RuleKind::Style(style) = &guard.kind else {
            return false;
        };
        let old = style.decl_hold.as_ref().map_or(0, Hold::bytes);
        (guard.memory.clone(), old)
    };
    // The old declarations go, so their bytes are free for the new ones.
    let what = "rule.style.cssText";
    if !memory.fits(estimate_style_bytes(text).saturating_sub(old), what) {
        return false;
    }
    let block = StyleDeclarations::parse(text, quirks);
    if !memory.fits(block.retained_bytes().saturating_sub(old), what) {
        return false;
    }
    let mut guard = rule.borrow_mut();
    let Rule { kind, memory, .. } = &mut *guard;
    let RuleKind::Style(style) = kind else {
        return false;
    };
    style.cssom = Some(block);
    style.written(memory);
    true
}

/// `rule.selectorText = text` for the style rule `rule`: an invalid list,
/// or one whose estimated bytes do not fit ([`MAX_SCRIPT_STYLE_BYTES`]),
/// changes nothing. Returns true if the selectors changed; false for a rule
/// of another kind.
pub fn set_selector_text(rule: &RuleRef, text: &str) -> bool {
    let mut guard = rule.borrow_mut();
    let Rule { kind, memory, .. } = &mut *guard;
    match kind {
        RuleKind::Style(style) => style.set_selectors(text, memory),
        _ => false,
    }
}

/// Records that a script changed `rule` (in its sheet, if it has one).
pub fn mark_modified(rule: &RuleRef) {
    if let Some(sheet) = parent_sheet(rule) {
        sheet.borrow_mut().modified = true;
    }
}

impl StyleRule {
    /// The selectors (`selectorText` is their CSSOM serialization).
    pub fn selectors(&self) -> &SelectorList {
        &self.selectors
    }

    /// Sets the selectors from `text` (the `selectorText` setter). An
    /// invalid list changes nothing, and so does a list whose estimated
    /// bytes do not fit in `memory` (ADR 0028 section 16; the text is
    /// checked before the parse). The bytes of the selectors that a script
    /// set before return. Returns true if it changed.
    fn set_selectors(&mut self, text: &str, memory: &StyleMemory) -> bool {
        let old = self.selector_hold.as_ref().map_or(0, Hold::bytes);
        let what = "selectorText";
        if !memory.fits(estimate_style_bytes(text).saturating_sub(old), what) {
            return false;
        }
        let Ok(list) = SelectorList::parse_str(text) else {
            return false;
        };
        let bytes = selectors_bytes(&list);
        if !memory.fits(bytes.saturating_sub(old), what) {
            return false;
        }
        self.selectors = list;
        memory.resize(&mut self.selector_hold, bytes);
        true
    }

    /// The block of `CSSStyleRule.style`, built from the declarations at
    /// the first use.
    pub fn declarations_block(&mut self, quirks: bool) -> &mut StyleDeclarations {
        let declarations = &self.declarations;
        self.cssom
            .get_or_insert_with(|| StyleDeclarations::from_declarations(declarations, quirks))
    }

    /// The block of `CSSStyleRule.style` was written (or replaced): its
    /// declarations become those of the cascade at the next use, and the
    /// rule's charge is that of the block as it is now.
    fn written(&mut self, memory: &StyleMemory) {
        let Some(block) = &self.cssom else {
            return;
        };
        self.declarations = Vec::new();
        self.stale = true;
        self.block = None;
        memory.resize(&mut self.decl_hold, block.retained_bytes());
    }

    /// The declaration block of the cascade for `cx`, parsed at the first
    /// use with this base URL and kept.
    pub(crate) fn cascade_block(&mut self, cx: &ParserContext) -> Arc<DeclarationBlock> {
        if std::mem::take(&mut self.stale)
            && let Some(block) = &self.cssom
        {
            self.declarations = swb_css::parse_style_attribute(&block.css_text());
        }
        if let (Some((url, block)), Some(base)) = (&self.block, &cx.base_url)
            && (Arc::ptr_eq(url, base) || url == base)
        {
            return Arc::clone(block);
        }
        let block = Arc::new(DeclarationBlock::parse(&self.declarations, cx));
        if let Some(base) = &cx.base_url {
            self.block = Some((Arc::clone(base), Arc::clone(&block)));
        }
        block
    }
}

/// Converts a rule that a script inserts: reserves the estimated bytes of
/// the parsed rule (it fails with [`RuleError::Memory`] if they do not
/// fit) and makes the rule hold them.
fn convert_inserted(
    rule: CssRule,
    parent_rule: &Weak<RefCell<Rule>>,
    parent_sheet: &Weak<RefCell<StyleSheet>>,
    memory: &StyleMemory,
) -> Result<RuleRef, RuleError> {
    let mut budget = Some(
        memory
            .reserve(rule_bytes(&rule), "insertRule")
            .ok_or(RuleError::Memory)?,
    );
    let rule = convert_one(rule, parent_rule, parent_sheet, memory, &mut budget);
    rule.borrow_mut().hold = budget;
    Ok(rule)
}

/// Converts parsed rules into shared rules with the given parents. The
/// style rules take the bytes of their declarations from `budget` (the
/// bytes of a script-made sheet or rule), if there is one.
fn convert(
    rules: Vec<CssRule>,
    parent_rule: &Weak<RefCell<Rule>>,
    parent_sheet: &Weak<RefCell<StyleSheet>>,
    memory: &StyleMemory,
    budget: &mut Option<Hold>,
) -> Vec<RuleRef> {
    rules
        .into_iter()
        .map(|r| convert_one(r, parent_rule, parent_sheet, memory, budget))
        .collect()
}

/// Converts one parsed rule (and the rules inside it).
fn convert_one(
    rule: CssRule,
    parent_rule: &Weak<RefCell<Rule>>,
    parent_sheet: &Weak<RefCell<StyleSheet>>,
    memory: &StyleMemory,
    budget: &mut Option<Hold>,
) -> RuleRef {
    let (kind, children) = match rule {
        CssRule::Style(style) => {
            let decl_hold = budget
                .as_mut()
                .map(|b| b.split(declarations_bytes(&style.declarations)));
            (
                RuleKind::Style(StyleRule {
                    selectors: style.selectors,
                    declarations: style.declarations,
                    stale: false,
                    block: None,
                    cssom: None,
                    decl_hold,
                    selector_hold: None,
                }),
                None,
            )
        }
        CssRule::Media(media) => (
            RuleKind::Media {
                media: media.media,
                rules: Vec::new(),
            },
            Some(media.rules),
        ),
        CssRule::Supports(supports) => (
            RuleKind::Supports {
                condition: supports.condition,
                text: supports.text,
                rules: Vec::new(),
            },
            Some(supports.rules),
        ),
        CssRule::Import(import) => (RuleKind::Import(import), None),
        CssRule::FontFace(face) => (RuleKind::FontFace(face), None),
        CssRule::Keyframes(keyframes) => (RuleKind::Keyframes(keyframes), None),
        CssRule::LayerBlock(layer) => (
            RuleKind::LayerBlock {
                name: layer.name,
                rules: Vec::new(),
            },
            Some(layer.rules),
        ),
        CssRule::LayerStatement(layer) => (RuleKind::LayerStatement { names: layer.names }, None),
    };
    let rule = Rc::new(RefCell::new(Rule {
        kind,
        parent: parent_rule.clone(),
        sheet: parent_sheet.clone(),
        memory: memory.clone(),
        hold: None,
    }));
    if let Some(children) = children {
        let converted = convert(
            children,
            &Rc::downgrade(&rule),
            &Weak::new(),
            memory,
            budget,
        );
        if let RuleKind::Media { rules, .. }
        | RuleKind::Supports { rules, .. }
        | RuleKind::LayerBlock { rules, .. } = &mut rule.borrow_mut().kind
        {
            *rules = converted;
        }
    }
    rule
}

/// The text of a grouping rule: Chromium writes each rule on its own line,
/// indented by two spaces (the lines of a nested rule keep their own
/// indentation), and the closing brace on a line of its own.
fn grouping_text(keyword: &str, condition: &str, rules: &[RuleRef], quirks: bool) -> String {
    grouping_block(&format!("{keyword} {condition}"), rules, quirks)
}

/// [`grouping_text`] with the whole prelude given.
fn grouping_block(prelude: &str, rules: &[RuleRef], quirks: bool) -> String {
    let mut out = format!("{prelude} {{\n");
    for rule in rules {
        let _ = writeln!(out, "  {}", rule.borrow_mut().css_text(quirks));
    }
    out.push('}');
    out
}

/// The text of an `@keyframes` rule, as Chromium writes it: a space after
/// the opening brace, each keyframe on its own line with its offsets in
/// percent.
fn keyframes_text(rule: &KeyframesRule, quirks: bool) -> String {
    let mut out = String::from("@keyframes ");
    serialize_identifier(&rule.name, &mut out);
    out.push_str(" { \n");
    for keyframe in &rule.keyframes {
        let keys: Vec<String> = keyframe
            .keys
            .iter()
            .map(|k| format!("{}%", format_number(f64::from(*k))))
            .collect();
        let block = StyleDeclarations::from_declarations(&keyframe.declarations, quirks).css_text();
        if block.is_empty() {
            let _ = writeln!(out, "  {} {{ }}", keys.join(", "));
        } else {
            let _ = writeln!(out, "  {} {{ {block} }}", keys.join(", "));
        }
    }
    out.push('}');
    out
}

/// The value of an `@font-face` descriptor: its component values, with
/// an unquoted `url()` written with a quoted string (Chromium:
/// `url("x.woff2")`).
fn descriptor_value(values: &[ComponentValue]) -> String {
    let mut out = String::new();
    for value in values {
        match value {
            ComponentValue::Url(url) => {
                out.push_str("url(");
                serialize_string(url, &mut out);
                out.push(')');
            }
            other => out.push_str(&serialize_component_values(std::slice::from_ref(other))),
        }
    }
    out
}

#[cfg(test)]
mod tests;
