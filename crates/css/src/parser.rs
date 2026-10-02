//! CSS parser: stylesheets, rules, declarations and component values.
//!
//! Implements the parsing algorithms of CSS Syntax Level 3:
//! <https://www.w3.org/TR/css-syntax-3/#parsing>. For blocks it follows the
//! current Editor's Draft (<https://drafts.csswg.org/css-syntax/#consume-block-contents>):
//! inside a block, the parser first tries a declaration and falls back to a
//! nested rule. This gives the same error recovery as browsers that support
//! CSS Nesting.
//!
//! The parser builds the stylesheet model directly. It decides how to parse
//! an at-rule's block from the at-rule name, so it never builds component
//! values for the contents of `@media` or `@supports` blocks.
//!
//! Deviations and limitations:
//!
//! - Nested style rules (CSS Nesting) and at-rules nested inside style rules
//!   are parsed for error recovery and then dropped.
//! - Inside `@media`, `@supports` and `@layer` blocks, the parser does not
//!   try declarations first (declarations are not valid there). The result
//!   is the same, because nested rules there stop at `;`.
//! - Blocks nested deeper than [`MAX_NESTING_DEPTH`] are skipped, so that
//!   hostile input cannot overflow the stack.

use log::{debug, warn};

use crate::media::MediaQueryList;
use crate::selector::SelectorList;
use crate::serialize::DisplayValues;
use crate::stylesheet::{
    CssRule, Declaration, FontFaceRule, ImportRule, MediaRule, StyleRule, Stylesheet, SupportsRule,
};
use crate::supports::SupportsCondition;
use crate::tokenizer::{Token, Tokenizer, preprocess};
use crate::values::{BlockKind, ComponentValue, Function, SimpleBlock};

/// The maximum nesting depth of blocks and functions. Deeper blocks are
/// skipped and replaced with empty ones.
pub(crate) const MAX_NESTING_DEPTH: usize = 64;

/// Parses a stylesheet.
///
/// <https://www.w3.org/TR/css-syntax-3/#parse-stylesheet>
pub fn parse_stylesheet(css: &str) -> Stylesheet {
    let input = preprocess(css);
    let mut parser = RuleParser::new(&input);
    Stylesheet {
        rules: parser.consume_stylesheet_contents(),
    }
}

/// Parses the declarations of a `style` attribute (a declaration list
/// without braces). Nested rules are dropped.
///
/// <https://drafts.csswg.org/css-syntax/#parse-block-contents>
pub fn parse_style_attribute(css: &str) -> Vec<Declaration> {
    let input = preprocess(css);
    let mut parser = RuleParser::new(&input);
    let mut declarations = Vec::new();
    parser.consume_block_contents(Context::Style, &mut declarations, &mut Vec::new());
    declarations
}

/// Parses a list of component values, for example a property value.
///
/// <https://www.w3.org/TR/css-syntax-3/#parse-list-of-component-values>
pub fn parse_component_values(css: &str) -> Vec<ComponentValue> {
    let input = preprocess(css);
    let mut parser = RuleParser::new(&input);
    let mut values = Vec::new();
    while let Some(value) = parser.consume_component_value() {
        values.push(value);
    }
    values
}

/// Builds a declaration from its name and raw value: removes `!important`
/// and surrounding whitespace, lowercases the name of a non-custom property
/// and applies the `unicode-range` special case. Returns `None` if the value
/// is not valid for a declaration.
///
/// `raw` is the source text of the value, if known.
///
/// <https://drafts.csswg.org/css-syntax/#consume-declaration>
fn finish_declaration(
    name: Box<str>,
    mut value: Vec<ComponentValue>,
    raw: Option<&str>,
) -> Option<Declaration> {
    let is_custom = name.starts_with("--");
    let mut important = false;
    if let Some(last) = value.iter().rposition(|v| !v.is_whitespace())
        && value[last].is_ident("important")
        && let Some(bang) = value[..last].iter().rposition(|v| !v.is_whitespace())
        && value[bang].is_delim('!')
    {
        value.truncate(bang);
        important = true;
    }
    while value.last().is_some_and(ComponentValue::is_whitespace) {
        value.pop();
    }
    let leading = value.iter().take_while(|v| v.is_whitespace()).count();
    value.drain(..leading);
    if !is_custom {
        // A top-level {}-block is valid only as the entire value.
        let has_curly_block = value
            .iter()
            .any(|v| matches!(v, ComponentValue::Block(b) if b.kind == BlockKind::Curly));
        let non_whitespace = value.iter().filter(|v| !v.is_whitespace()).count();
        if has_curly_block && non_whitespace > 1 {
            return None;
        }
    }
    let name = if is_custom || !name.bytes().any(|b| b.is_ascii_uppercase()) {
        name.into_string()
    } else {
        name.to_ascii_lowercase()
    };
    if name == "unicode-range"
        && let Some(ranges) = raw.and_then(parse_unicode_ranges)
    {
        value = ranges;
    }
    Some(Declaration {
        name,
        value,
        important,
    })
}

/// Parses a declaration from component values (for example the contents of
/// `(display: grid)` in `@supports`).
pub(crate) fn parse_declaration_from_values(values: &[ComponentValue]) -> Option<Declaration> {
    let mut iter = values.iter().skip_while(|v| v.is_whitespace());
    let ComponentValue::Ident(name) = iter.next()? else {
        return None;
    };
    let mut iter = iter.skip_while(|v| v.is_whitespace());
    if !matches!(iter.next(), Some(ComponentValue::Colon)) {
        return None;
    }
    finish_declaration(name.clone(), iter.cloned().collect(), None)
}

/// Where a list of rules or declarations appears. It decides which rules
/// are valid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Context {
    /// The top level of a stylesheet.
    TopLevel,
    /// Inside `@media`, `@supports` or `@layer`.
    Grouping,
    /// Inside a style rule (or a `style` attribute).
    Style,
    /// Inside an at-rule that holds descriptors, such as `@font-face`.
    Descriptors,
}

impl Context {
    fn holds_rules(self) -> bool {
        matches!(self, Context::TopLevel | Context::Grouping)
    }
}

/// A token stream with one token of lookahead and the ability to rewind.
struct TokenStream<'a> {
    tokenizer: Tokenizer<'a>,
    peeked: Option<Token>,
    peeked_start: usize,
}

impl<'a> TokenStream<'a> {
    fn new(input: &'a str) -> Self {
        TokenStream {
            tokenizer: Tokenizer::new(input),
            peeked: None,
            peeked_start: 0,
        }
    }

    fn peek(&mut self) -> Option<&Token> {
        if self.peeked.is_none() {
            self.peeked_start = self.tokenizer.position();
            self.peeked = self.tokenizer.next_token();
        }
        self.peeked.as_ref()
    }

    fn next(&mut self) -> Option<Token> {
        match self.peeked.take() {
            Some(token) => Some(token),
            None => self.tokenizer.next_token(),
        }
    }

    /// The byte offset where the next token (or a comment before it) starts.
    fn position(&self) -> usize {
        if self.peeked.is_some() {
            self.peeked_start
        } else {
            self.tokenizer.position()
        }
    }

    /// Rewinds to a position returned by [`TokenStream::position`].
    fn reset(&mut self, position: usize) {
        self.peeked = None;
        self.tokenizer.set_position(position);
    }

    fn source(&self) -> &'a str {
        self.tokenizer.input()
    }
}

/// The rule and declaration parser.
struct RuleParser<'a> {
    input: TokenStream<'a>,
    /// The current nesting depth of blocks and functions.
    depth: usize,
    /// True until the first rule that is not `@charset`, `@import` or an
    /// `@layer` statement. `@import` is valid only before that rule.
    allow_import: bool,
}

impl<'a> RuleParser<'a> {
    fn new(input: &'a str) -> Self {
        RuleParser {
            input: TokenStream::new(input),
            depth: 0,
            allow_import: true,
        }
    }

    /// Consumes the top-level rules up to the end of the input.
    ///
    /// <https://drafts.csswg.org/css-syntax/#consume-stylesheet-contents>
    fn consume_stylesheet_contents(&mut self) -> Vec<CssRule> {
        let context = Context::TopLevel;
        let mut rules = Vec::new();
        loop {
            match self.input.peek() {
                None => return rules,
                Some(Token::Whitespace | Token::Cdo | Token::Cdc) => {
                    self.input.next();
                }
                Some(Token::AtKeyword(_)) => self.consume_at_rule(false, context, &mut rules),
                Some(_) => self.consume_qualified_rule(false, false, context, &mut rules),
            }
        }
    }

    /// Consumes the contents of a `{}` block (the `{` is already consumed)
    /// up to the closing `}`, which is not consumed.
    ///
    /// <https://drafts.csswg.org/css-syntax/#consume-block-contents>
    fn consume_block_contents(
        &mut self,
        context: Context,
        declarations: &mut Vec<Declaration>,
        rules: &mut Vec<CssRule>,
    ) {
        loop {
            match self.input.peek() {
                None | Some(Token::CloseCurly) => return,
                Some(Token::Whitespace | Token::Semicolon) => {
                    self.input.next();
                }
                Some(Token::AtKeyword(_)) => self.consume_at_rule(true, context, rules),
                Some(_) => {
                    if !context.holds_rules() {
                        let mark = self.input.position();
                        if let Some(declaration) = self.consume_declaration() {
                            declarations.push(declaration);
                            continue;
                        }
                        self.input.reset(mark);
                    }
                    self.consume_qualified_rule(true, true, context, rules);
                }
            }
        }
    }

    /// <https://drafts.csswg.org/css-syntax/#consume-at-rule>
    fn consume_at_rule(&mut self, nested: bool, context: Context, out: &mut Vec<CssRule>) {
        let Some(Token::AtKeyword(name)) = self.input.next() else {
            return;
        };
        let mut prelude = Vec::new();
        let has_block = loop {
            match self.input.peek() {
                None => break false,
                Some(Token::Semicolon) => {
                    self.input.next();
                    break false;
                }
                Some(Token::CloseCurly) if nested => break false,
                Some(Token::OpenCurly) => {
                    self.input.next();
                    break true;
                }
                Some(_) => {
                    if let Some(value) = self.consume_component_value() {
                        prelude.push(value);
                    }
                }
            }
        };
        let lower = name.to_ascii_lowercase();
        self.handle_at_rule(&lower, &prelude, has_block, context, out);
        if context == Context::TopLevel && closes_import_window(&lower, has_block) {
            self.allow_import = false;
        }
    }

    /// Builds the rule for an at-rule whose prelude is consumed. If
    /// `has_block` is true, the `{` is consumed and this function consumes
    /// the block up to and including the `}`.
    fn handle_at_rule(
        &mut self,
        name: &str,
        prelude: &[ComponentValue],
        has_block: bool,
        context: Context,
        out: &mut Vec<CssRule>,
    ) {
        let holds_rules = context.holds_rules();
        match (name, has_block) {
            ("media", true) if holds_rules => {
                let media = MediaQueryList::parse(prelude);
                let rules = self.consume_rule_block();
                out.push(CssRule::Media(MediaRule { media, rules }));
            }
            ("supports", true) if holds_rules => {
                if let Ok(condition) = SupportsCondition::parse(prelude) {
                    let rules = self.consume_rule_block();
                    out.push(CssRule::Supports(SupportsRule { condition, rules }));
                } else {
                    debug!(
                        "dropped @supports with invalid condition `{}`",
                        DisplayValues(prelude)
                    );
                    self.skip_block(BlockKind::Curly);
                }
            }
            // Approximation: the rules of a layer block join the parent list,
            // and layer order is ignored.
            ("layer", true) if holds_rules => {
                let rules = self.consume_rule_block();
                out.extend(rules);
            }
            ("layer", false) if holds_rules => {}
            ("font-face", true) if holds_rules => {
                let declarations = self.consume_declaration_block(Context::Descriptors);
                out.push(CssRule::FontFace(FontFaceRule { declarations }));
            }
            ("import", false) if context == Context::TopLevel && self.allow_import => {
                match ImportRule::parse(prelude) {
                    Some(rule) => out.push(CssRule::Import(rule)),
                    None => debug!("dropped @import with invalid prelude"),
                }
            }
            ("charset", false) if context == Context::TopLevel => {}
            _ => {
                debug!("dropped @{name} rule");
                if has_block {
                    self.skip_block(BlockKind::Curly);
                }
            }
        }
    }

    /// <https://drafts.csswg.org/css-syntax/#consume-qualified-rule>
    fn consume_qualified_rule(
        &mut self,
        stop_at_semicolon: bool,
        nested: bool,
        context: Context,
        out: &mut Vec<CssRule>,
    ) {
        let mut prelude = Vec::new();
        loop {
            match self.input.peek() {
                None => {
                    debug!("dropped rule: end of input in prelude");
                    return;
                }
                Some(Token::Semicolon) if stop_at_semicolon => return,
                Some(Token::CloseCurly) if nested => return,
                Some(Token::OpenCurly) => {
                    if starts_like_custom_property(&prelude) {
                        if nested {
                            self.consume_bad_declaration_remnants();
                        } else {
                            self.input.next();
                            self.skip_block(BlockKind::Curly);
                        }
                        return;
                    }
                    self.input.next();
                    break;
                }
                Some(_) => {
                    if let Some(value) = self.consume_component_value() {
                        prelude.push(value);
                    }
                }
            }
        }
        if !context.holds_rules() {
            debug!("dropped nested rule `{}`", DisplayValues(&prelude));
            self.skip_block(BlockKind::Curly);
            return;
        }
        if let Ok(selectors) = SelectorList::parse(&prelude) {
            let declarations = self.consume_declaration_block(Context::Style);
            out.push(CssRule::Style(StyleRule {
                selectors,
                declarations,
            }));
            if context == Context::TopLevel {
                self.allow_import = false;
            }
        } else {
            debug!(
                "dropped rule with invalid selector `{}`",
                DisplayValues(&prelude)
            );
            self.skip_block(BlockKind::Curly);
        }
    }

    /// Consumes a `{}` block of rules. The `{` is already consumed.
    fn consume_rule_block(&mut self) -> Vec<CssRule> {
        let mut rules = Vec::new();
        if !self.enter_block(BlockKind::Curly) {
            return rules;
        }
        self.consume_block_contents(Context::Grouping, &mut Vec::new(), &mut rules);
        self.leave_block();
        rules
    }

    /// Consumes a `{}` block of declarations. The `{` is already consumed.
    fn consume_declaration_block(&mut self, context: Context) -> Vec<Declaration> {
        let mut declarations = Vec::new();
        if !self.enter_block(BlockKind::Curly) {
            return declarations;
        }
        self.consume_block_contents(context, &mut declarations, &mut Vec::new());
        self.leave_block();
        declarations
    }

    /// Increments the nesting depth for a block or function of `kind` whose
    /// opening token is consumed. If it is too deep, skips it (up to and
    /// including the closing token) and returns false.
    fn enter_block(&mut self, kind: BlockKind) -> bool {
        if self.depth >= MAX_NESTING_DEPTH {
            warn!("CSS blocks nested too deeply; block dropped");
            self.skip_block(kind);
            return false;
        }
        self.depth += 1;
        true
    }

    /// Consumes the closing `}` of a `{}` block started with
    /// [`RuleParser::enter_block`].
    fn leave_block(&mut self) {
        self.depth -= 1;
        // The `}`, or nothing at the end of the input.
        self.input.next();
    }

    /// Tries to consume a declaration. Returns `None` if the input does not
    /// start with a valid declaration; the caller must then rewind.
    ///
    /// <https://drafts.csswg.org/css-syntax/#consume-declaration>
    fn consume_declaration(&mut self) -> Option<Declaration> {
        let Some(Token::Ident(_)) = self.input.peek() else {
            return None;
        };
        let Some(Token::Ident(name)) = self.input.next() else {
            return None;
        };
        self.skip_whitespace();
        if !matches!(self.input.peek(), Some(Token::Colon)) {
            return None;
        }
        self.input.next();
        self.skip_whitespace();
        let is_custom = name.starts_with("--");
        let value_start = self.input.position();
        let mut value = Vec::new();
        let mut seen_non_block = false;
        loop {
            match self.input.peek() {
                None | Some(Token::Semicolon | Token::CloseCurly) => break,
                // A {}-block after other values makes the declaration
                // invalid. Fail early: this is the common case of a nested
                // rule such as `a:hover { ... }`, and consuming the rest
                // would make the parser quadratic.
                Some(Token::OpenCurly) if !is_custom && seen_non_block => return None,
                Some(Token::OpenCurly | Token::Whitespace) => {}
                Some(_) => seen_non_block = true,
            }
            if let Some(v) = self.consume_component_value() {
                value.push(v);
            }
        }
        let value_end = self.input.position();
        let raw = self.input.source().get(value_start..value_end);
        finish_declaration(name, value, raw)
    }

    /// <https://drafts.csswg.org/css-syntax/#consume-the-remnants-of-a-bad-declaration>
    fn consume_bad_declaration_remnants(&mut self) {
        loop {
            match self.input.peek() {
                None | Some(Token::CloseCurly) => return,
                Some(Token::Semicolon) => {
                    self.input.next();
                    return;
                }
                Some(_) => self.skip_component_value(),
            }
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.input.peek(), Some(Token::Whitespace)) {
            self.input.next();
        }
    }

    /// <https://www.w3.org/TR/css-syntax-3/#consume-component-value>
    fn consume_component_value(&mut self) -> Option<ComponentValue> {
        let token = self.input.next()?;
        Some(match token {
            Token::OpenCurly => ComponentValue::Block(self.consume_simple_block(BlockKind::Curly)),
            Token::OpenSquare => {
                ComponentValue::Block(self.consume_simple_block(BlockKind::Square))
            }
            Token::OpenParen => ComponentValue::Block(self.consume_simple_block(BlockKind::Paren)),
            Token::Function(name) => ComponentValue::Function(Function {
                name,
                arguments: self.consume_nested_values(BlockKind::Paren),
            }),
            other => ComponentValue::from_preserved_token(other)?,
        })
    }

    /// <https://www.w3.org/TR/css-syntax-3/#consume-simple-block>
    fn consume_simple_block(&mut self, kind: BlockKind) -> SimpleBlock {
        SimpleBlock {
            kind,
            contents: self.consume_nested_values(kind),
        }
    }

    /// Consumes component values up to the closing token of a block or
    /// function of `kind`. The opening token is already consumed.
    fn consume_nested_values(&mut self, kind: BlockKind) -> Vec<ComponentValue> {
        let mut values = Vec::new();
        if !self.enter_block(kind) {
            return values;
        }
        loop {
            match self.input.peek() {
                None => break,
                Some(token) if closes(token, kind) => {
                    self.input.next();
                    break;
                }
                Some(_) => {
                    if let Some(value) = self.consume_component_value() {
                        values.push(value);
                    }
                }
            }
        }
        self.depth -= 1;
        values
    }

    /// Skips one component value without building it.
    fn skip_component_value(&mut self) {
        match self.input.next() {
            Some(Token::OpenCurly) => self.skip_block(BlockKind::Curly),
            Some(Token::OpenSquare) => self.skip_block(BlockKind::Square),
            Some(Token::OpenParen | Token::Function(_)) => self.skip_block(BlockKind::Paren),
            _ => {}
        }
    }

    /// Skips the rest of a block of `kind` whose opening token is consumed,
    /// including the closing token. Uses a heap stack, so any nesting depth
    /// is safe.
    fn skip_block(&mut self, kind: BlockKind) {
        let mut stack = vec![kind];
        while let Some(token) = self.input.next() {
            let opened = match token {
                Token::OpenCurly => Some(BlockKind::Curly),
                Token::OpenSquare => Some(BlockKind::Square),
                Token::OpenParen | Token::Function(_) => Some(BlockKind::Paren),
                _ => None,
            };
            if let Some(opened) = opened {
                stack.push(opened);
                continue;
            }
            if let Some(&top) = stack.last()
                && closes(&token, top)
            {
                stack.pop();
                if stack.is_empty() {
                    return;
                }
            }
        }
    }
}

/// True if a valid top-level at-rule with this (lowercase) name ends the
/// part of the stylesheet where `@import` is allowed. Only `@charset`,
/// `@import` and `@layer` statements may come before `@import`; unknown
/// (invalid) at-rules are ignored.
/// <https://www.w3.org/TR/css-cascade-5/#at-import>
fn closes_import_window(name: &str, has_block: bool) -> bool {
    match name {
        "charset" | "import" => false,
        "layer" => has_block,
        _ => KNOWN_AT_RULES.contains(&name),
    }
}

/// At-rules that browsers support, whether or not swb keeps them.
const KNOWN_AT_RULES: &[&str] = &[
    "container",
    "counter-style",
    "font-face",
    "font-feature-values",
    "font-palette-values",
    "keyframes",
    "-webkit-keyframes",
    "media",
    "namespace",
    "page",
    "position-try",
    "property",
    "scope",
    "starting-style",
    "supports",
    "view-transition",
];

fn closes(token: &Token, kind: BlockKind) -> bool {
    matches!(
        (token, kind),
        (Token::CloseCurly, BlockKind::Curly)
            | (Token::CloseSquare, BlockKind::Square)
            | (Token::CloseParen, BlockKind::Paren)
    )
}

/// True if the first two non-whitespace values are a custom property name
/// and a colon (`--foo:`). Such a prelude is a declaration, not a rule.
fn starts_like_custom_property(prelude: &[ComponentValue]) -> bool {
    let mut iter = prelude.iter().filter(|v| !v.is_whitespace());
    matches!(
        (iter.next(), iter.next()),
        (Some(ComponentValue::Ident(name)), Some(ComponentValue::Colon)) if name.starts_with("--")
    )
}

/// Parses the source text of a `unicode-range` value into
/// [`ComponentValue::UnicodeRange`] values separated by commas. Returns
/// `None` if any range is invalid.
///
/// <https://drafts.csswg.org/css-syntax/#urange-syntax>
fn parse_unicode_ranges(raw: &str) -> Option<Vec<ComponentValue>> {
    let raw = strip_comments(raw);
    let mut out = Vec::new();
    for (i, part) in raw.split(',').enumerate() {
        if i > 0 {
            out.push(ComponentValue::Comma);
        }
        let (start, end) = parse_urange(part.trim_matches([' ', '\t', '\n']))?;
        out.push(ComponentValue::UnicodeRange { start, end });
    }
    Some(out)
}

/// Removes `/* ... */` comments.
fn strip_comments(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains("/*") {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => rest = "",
        }
    }
    out.push_str(rest);
    std::borrow::Cow::Owned(out)
}

fn parse_urange(s: &str) -> Option<(u32, u32)> {
    let rest = s.strip_prefix(['u', 'U'])?.strip_prefix('+')?;
    let (first, second) = match rest.split_once('-') {
        Some((a, b)) => (a, Some(b)),
        None => (rest, None),
    };
    let hex_len = first.bytes().take_while(u8::is_ascii_hexdigit).count();
    let wildcards = first.len() - hex_len;
    if first.is_empty() || first.len() > 6 || !first.bytes().skip(hex_len).all(|b| b == b'?') {
        return None;
    }
    let base = if hex_len == 0 {
        0
    } else {
        u32::from_str_radix(&first[..hex_len], 16).ok()?
    };
    let (start, end) = if wildcards > 0 {
        if second.is_some() {
            return None;
        }
        let shift = 4 * wildcards as u32;
        (base << shift, (base << shift) | ((1 << shift) - 1))
    } else {
        let end = match second {
            Some(b) => {
                if b.is_empty() || b.len() > 6 || !b.bytes().all(|c| c.is_ascii_hexdigit()) {
                    return None;
                }
                u32::from_str_radix(b, 16).ok()?
            }
            None => base,
        };
        (base, end)
    };
    (end <= 0x10_FFFF && start <= end).then_some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serialize::serialize_component_values;

    fn style_rules(css: &str) -> Vec<StyleRule> {
        parse_stylesheet(css)
            .rules
            .into_iter()
            .filter_map(|r| match r {
                CssRule::Style(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    fn decl_summary(decls: &[Declaration]) -> Vec<(String, String, bool)> {
        decls
            .iter()
            .map(|d| {
                (
                    d.name.clone(),
                    serialize_component_values(&d.value),
                    d.important,
                )
            })
            .collect()
    }

    fn s(name: &str, value: &str, important: bool) -> (String, String, bool) {
        (name.to_owned(), value.to_owned(), important)
    }

    #[test]
    fn simple_stylesheet() {
        let rules = style_rules("a { color: red; margin : 0 auto } b{x:y}");
        assert_eq!(rules.len(), 2);
        assert_eq!(
            decl_summary(&rules[0].declarations),
            vec![s("color", "red", false), s("margin", "0 auto", false)]
        );
        assert_eq!(
            decl_summary(&rules[1].declarations),
            vec![s("x", "y", false)]
        );
    }

    #[test]
    fn important_variants() {
        let decls = parse_style_attribute(
            "a: 1 !important; b: 2 ! important; c: 3!IMPORTANT ; d: 4 !important x; e: !important; f: 5 !imp",
        );
        assert_eq!(
            decl_summary(&decls),
            vec![
                s("a", "1", true),
                s("b", "2", true),
                s("c", "3", true),
                s("d", "4 !important x", false),
                s("e", "", true),
                s("f", "5 !imp", false),
            ]
        );
    }

    #[test]
    fn declaration_names() {
        let decls = parse_style_attribute("COLOR: Red; --My-Var: { a: b }; --empty:;");
        assert_eq!(
            decl_summary(&decls),
            vec![
                s("color", "Red", false),
                s("--My-Var", "{ a: b }", false),
                s("--empty", "", false),
            ]
        );
    }

    #[test]
    fn missing_semicolons_and_garbage() {
        // A missing semicolon makes one bad declaration that swallows the
        // next one; parsing continues after the next semicolon.
        let decls = parse_style_attribute("color: red background: blue; margin: 0");
        assert_eq!(
            decl_summary(&decls),
            vec![
                s("color", "red background: blue", false),
                s("margin", "0", false)
            ]
        );
        let decls = parse_style_attribute("color red; 12px: x; : y; margin: 0;;; padding: 1px");
        assert_eq!(
            decl_summary(&decls),
            vec![s("margin", "0", false), s("padding", "1px", false)]
        );
    }

    #[test]
    fn garbage_before_rules() {
        let rules = style_rules("garbage; a { color: red } } b { color: blue } c { x: y }");
        // `garbage; a` is one invalid prelude; `} b` is another.
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selectors.to_string(), "c");
        let rules = style_rules("<!-- a { x: y } -->");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn unclosed_blocks_at_eof() {
        let rules = style_rules("a { color: red; b: rgb(1, 2");
        assert_eq!(rules.len(), 1);
        assert_eq!(
            decl_summary(&rules[0].declarations),
            vec![s("color", "red", false), s("b", "rgb(1, 2)", false)]
        );
        let sheet = parse_stylesheet("@media screen { a { color: red");
        assert_eq!(sheet.rules.len(), 1);
        let CssRule::Media(media) = &sheet.rules[0] else {
            panic!("expected @media");
        };
        assert_eq!(media.rules.len(), 1);
        // A prelude without a block is dropped.
        assert_eq!(parse_stylesheet("a, b").rules.len(), 0);
    }

    #[test]
    fn nested_rules_are_dropped() {
        let rules = style_rules(
            "a { color: red; &:hover { color: blue } .b { x: y } margin: 0; div:hover { z: 1 } padding: 1px; @media screen { q: r } top: 0 }",
        );
        assert_eq!(rules.len(), 1);
        assert_eq!(
            decl_summary(&rules[0].declarations),
            vec![
                s("color", "red", false),
                s("margin", "0", false),
                s("padding", "1px", false),
                s("top", "0", false),
            ]
        );
    }

    #[test]
    fn at_rules() {
        let sheet = parse_stylesheet(
            "@charset \"utf-8\"; @import url(a.css) screen; @import 'b.css' layer(x) supports(display: grid); \
             @namespace svg url(http://www.w3.org/2000/svg); @keyframes spin { from { x: y } to { x: z } } \
             @font-face { font-family: X; src: url(x.woff2) format('woff2'); unicode-range: U+0-7F, U+4??; } \
             @page :first { margin: 1in } @unknown foo; @layer base, components; \
             @layer base { a { x: y } } @supports (display: grid) and (not (display: inline-grid)) { b { x: y } } \
             @import url(late.css); c { x: y }",
        );
        let kinds: Vec<&str> = sheet
            .rules
            .iter()
            .map(|r| match r {
                CssRule::Style(_) => "style",
                CssRule::Media(_) => "media",
                CssRule::Import(_) => "import",
                CssRule::FontFace(_) => "font-face",
                CssRule::Supports(_) => "supports",
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                "import",
                "import",
                "font-face",
                "style",
                "supports",
                "style"
            ]
        );
        let CssRule::Import(import) = &sheet.rules[0] else {
            panic!("expected @import");
        };
        assert_eq!(import.url, "a.css");
        assert!(!import.media.is_empty());
        let CssRule::Import(import) = &sheet.rules[1] else {
            panic!("expected @import");
        };
        assert_eq!(import.url, "b.css");
        assert!(import.media.is_empty());
        let CssRule::FontFace(font_face) = &sheet.rules[2] else {
            panic!("expected @font-face");
        };
        let range = &font_face.declarations[2];
        assert_eq!(range.name, "unicode-range");
        assert_eq!(
            range.value,
            vec![
                ComponentValue::UnicodeRange {
                    start: 0,
                    end: 0x7F
                },
                ComponentValue::Comma,
                ComponentValue::UnicodeRange {
                    start: 0x400,
                    end: 0x4FF
                },
            ]
        );
    }

    #[test]
    fn import_after_rule_is_dropped() {
        let sheet = parse_stylesheet("a { x: y } @import url(late.css);");
        assert_eq!(sheet.rules.len(), 1);
        // Dropped but valid rules close the @import window too.
        let sheet = parse_stylesheet("@keyframes k {} @import url(late.css);");
        assert_eq!(sheet.rules.len(), 0);
        let sheet = parse_stylesheet("@namespace x url(y); @import url(late.css);");
        assert_eq!(sheet.rules.len(), 0);
        let sheet = parse_stylesheet("@layer a, b; @import url(ok.css);");
        assert_eq!(sheet.rules.len(), 1);
        // Invalid rules do not close the @import window.
        let sheet = parse_stylesheet("@foo; !! { } @import 'x.css';");
        assert!(matches!(&sheet.rules[..], [CssRule::Import(_)]));
    }

    #[test]
    fn nested_media() {
        let sheet = parse_stylesheet(
            "@media screen { @media (min-width: 100px) { a { x: y } } b { x: y } @import 'x'; }",
        );
        let CssRule::Media(outer) = &sheet.rules[0] else {
            panic!("expected @media");
        };
        assert_eq!(outer.rules.len(), 2);
        let CssRule::Media(inner) = &outer.rules[0] else {
            panic!("expected nested @media");
        };
        assert_eq!(inner.rules.len(), 1);
    }

    #[test]
    fn grouping_rule_recovery() {
        // Inside a grouping rule, a qualified rule stops at `;`.
        let sheet = parse_stylesheet("@media screen { color: red; a { x: y } junk; b { x: y } }");
        let CssRule::Media(media) = &sheet.rules[0] else {
            panic!("expected @media");
        };
        assert_eq!(media.rules.len(), 2);
    }

    #[test]
    fn at_rule_without_block_in_block() {
        let sheet = parse_stylesheet("@media screen { @foo bar } a { x: y }");
        assert_eq!(sheet.rules.len(), 2);
    }

    #[test]
    fn invalid_selector_drops_rule() {
        let rules =
            style_rules("a, ::-webkit-scrollbar { x: y } b { x: y } c:-moz-focusring { x: y }");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].selectors.to_string(), "b");
    }

    #[test]
    fn custom_property_like_prelude_at_top_level() {
        let rules = style_rules("--foo: { a: b } c { x: y }");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn deep_nesting_does_not_overflow() {
        let deep = "(".repeat(100_000);
        let values = parse_component_values(&deep);
        assert_eq!(values.len(), 1);
        let deep_rules = format!("{} a {{ x: y }}", "@media screen {".repeat(10_000));
        let _ = parse_stylesheet(&deep_rules);
        let deep_decl = format!("a {{ b: {} }} c {{ x: y }}", "[".repeat(50_000));
        let _ = parse_stylesheet(&deep_decl);
        let deep_curly = format!("a {{ b: {}", "{".repeat(50_000));
        let _ = parse_stylesheet(&deep_curly);
    }

    #[test]
    fn component_values() {
        let values = parse_component_values("a(b [c] {d}) ) }");
        assert_eq!(values.len(), 5);
        assert!(values[0].is_function("a"));
        assert_eq!(values[2], ComponentValue::CloseParen);
        assert_eq!(values[4], ComponentValue::CloseCurly);
    }

    #[test]
    fn unicode_range_parsing() {
        assert_eq!(parse_urange("U+26"), Some((0x26, 0x26)));
        assert_eq!(parse_urange("u+0-7F"), Some((0, 0x7F)));
        assert_eq!(parse_urange("U+4??"), Some((0x400, 0x4FF)));
        assert_eq!(parse_urange("U+??????"), None);
        assert_eq!(parse_urange("U+10FFFF"), Some((0x10_FFFF, 0x10_FFFF)));
        assert_eq!(parse_urange("U+110000"), None);
        assert_eq!(parse_urange("U+7F-0"), None);
        assert_eq!(parse_urange("U+"), None);
        assert_eq!(parse_urange("U+1?-2"), None);
        assert_eq!(parse_urange("U+1234567"), None);
        assert_eq!(parse_urange("V+12"), None);
        let decls = parse_style_attribute("unicode-range: U+0-7F /* latin */, /**/U+100");
        assert_eq!(
            decls[0].value,
            vec![
                ComponentValue::UnicodeRange {
                    start: 0,
                    end: 0x7F
                },
                ComponentValue::Comma,
                ComponentValue::UnicodeRange {
                    start: 0x100,
                    end: 0x100
                },
            ]
        );
        assert_eq!(strip_comments("a/* b */c/* d"), "ac");
        // An invalid range keeps the tokens.
        let decls = parse_style_attribute("unicode-range: U+X");
        assert!(matches!(decls[0].value[0], ComponentValue::Ident(_)));
    }

    #[test]
    fn crlf_in_stylesheet() {
        let rules = style_rules("a {\r\n  color: red;\r\n}\r\nb { x: y }");
        assert_eq!(rules.len(), 2);
    }
}
