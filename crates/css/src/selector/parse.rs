//! Selector parsing.
//!
//! <https://www.w3.org/TR/selectors-4/#grammar>
//!
//! The parser reads component values (the prelude of a style rule or the
//! arguments of a functional pseudo-class). Unknown pseudo-classes and
//! pseudo-elements, including vendor-prefixed ones, make the selector
//! invalid, as in browsers that do not support them.

use super::{
    AttrCase, AttrOperator, AttributeSelector, Combinator, Component, Direction, LocalName, Nth,
    PseudoClass, PseudoElement, RelativeSelector, STATE_PSEUDO_CLASSES, Selector, SelectorList,
};
use crate::cursor::{ParseError, Parser};
use crate::parser::MAX_NESTING_DEPTH;
use crate::selector::AnB;
use crate::values::{BlockKind, ComponentValue, Function, split_on_commas, trim_whitespace};

/// Attributes whose values match ASCII case-insensitively on HTML elements.
/// <https://html.spec.whatwg.org/multipage/semantics-other.html#case-sensitivity-of-selectors>
const HTML_CASE_INSENSITIVE_ATTRIBUTES: &[&str] = &[
    "accept",
    "accept-charset",
    "align",
    "alink",
    "axis",
    "bgcolor",
    "charset",
    "checked",
    "clear",
    "codetype",
    "color",
    "compact",
    "declare",
    "defer",
    "dir",
    "direction",
    "disabled",
    "enctype",
    "face",
    "frame",
    "hreflang",
    "http-equiv",
    "lang",
    "language",
    "link",
    "media",
    "method",
    "multiple",
    "nohref",
    "noresize",
    "noshade",
    "nowrap",
    "readonly",
    "rel",
    "rev",
    "rules",
    "scope",
    "scrolling",
    "selected",
    "shape",
    "target",
    "text",
    "type",
    "valign",
    "valuetype",
    "vlink",
];

/// Parser options that change inside functional pseudo-classes.
#[derive(Clone, Copy, Debug)]
struct Options {
    /// Pseudo-elements are allowed only at the top level.
    allow_pseudo_element: bool,
    /// `:has()` cannot nest.
    inside_has: bool,
    /// The nesting depth of functional pseudo-classes.
    depth: usize,
}

impl Options {
    fn nested(self) -> Result<Options, ParseError> {
        if self.depth >= MAX_NESTING_DEPTH {
            return Err(ParseError::Invalid);
        }
        Ok(Options {
            allow_pseudo_element: false,
            depth: self.depth + 1,
            ..self
        })
    }
}

const TOP_LEVEL: Options = Options {
    allow_pseudo_element: true,
    inside_has: false,
    depth: 0,
};

/// The maximum recursion depth of matching a selector: its combinators plus
/// the depth of nested selector arguments, along the worst path. Selectors
/// above this limit are invalid. Real selectors stay far below it; the limit
/// keeps matching from overflowing the stack (`i+i+i+...` with thousands of
/// compounds).
const MAX_MATCH_DEPTH: usize = 256;

/// Parses a selector list. Fails if any selector is invalid.
pub(crate) fn parse_selector_list(input: &[ComponentValue]) -> Result<SelectorList, ParseError> {
    let list = parse_list(input, TOP_LEVEL)?;
    if list
        .iter()
        .any(|s| match_depth(&s.components) > MAX_MATCH_DEPTH)
    {
        return Err(ParseError::Invalid);
    }
    Ok(list)
}

/// An upper bound for the recursion depth of matching `components`.
fn match_depth(components: &[Component]) -> usize {
    let combinators = components
        .iter()
        .filter(|c| matches!(c, Component::Combinator(_)))
        .count();
    let list_depth = |list: &SelectorList| {
        list.iter()
            .map(|s| match_depth(&s.components))
            .max()
            .unwrap_or(0)
    };
    let nested = components
        .iter()
        .map(|c| match c {
            Component::PseudoClass(
                PseudoClass::Not(list) | PseudoClass::Is(list) | PseudoClass::Where(list),
            ) => list_depth(list),
            Component::PseudoClass(PseudoClass::NthChild(nth) | PseudoClass::NthLastChild(nth)) => {
                nth.of.as_ref().map_or(0, list_depth)
            }
            // The anchor check adds one level.
            Component::PseudoClass(PseudoClass::Has(relative)) => relative
                .iter()
                .map(|r| match_depth(&r.selector.components) + 1)
                .max()
                .unwrap_or(0),
            _ => 0,
        })
        .max()
        .unwrap_or(0);
    combinators + nested + 1
}

fn parse_list(input: &[ComponentValue], options: Options) -> Result<SelectorList, ParseError> {
    let selectors = split_on_commas(input)
        .map(|part| parse_complex(part, options))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SelectorList { selectors })
}

/// Parses a forgiving selector list: invalid items are dropped.
/// <https://www.w3.org/TR/selectors-4/#forgiving-selector>
fn parse_forgiving_list(input: &[ComponentValue], options: Options) -> SelectorList {
    let selectors = split_on_commas(input)
        .filter(|part| !trim_whitespace(part).is_empty())
        .filter_map(|part| parse_complex(part, options).ok())
        .collect();
    SelectorList { selectors }
}

fn parse_complex(input: &[ComponentValue], options: Options) -> Result<Selector, ParseError> {
    let (components, pseudo_element) = parse_components(input, options)?;
    Ok(Selector::new(components, pseudo_element))
}

/// Parses a relative selector (`:has()` argument), for example `> img`.
fn parse_relative(
    input: &[ComponentValue],
    options: Options,
) -> Result<RelativeSelector, ParseError> {
    let mut p = Parser::new(input);
    let combinator = match p.peek() {
        Some(ComponentValue::Delim('>')) => Combinator::Child,
        Some(ComponentValue::Delim('+')) => Combinator::NextSibling,
        Some(ComponentValue::Delim('~')) => Combinator::SubsequentSibling,
        _ => Combinator::Descendant,
    };
    if combinator != Combinator::Descendant {
        p.next();
    }
    let (components, _) = parse_components(p.remaining(), options)?;
    Ok(RelativeSelector {
        combinator,
        selector: Selector::new(components, None),
    })
}

/// Parses a complex selector into components in right-to-left order.
fn parse_components(
    input: &[ComponentValue],
    options: Options,
) -> Result<(Vec<Component>, Option<PseudoElement>), ParseError> {
    let mut p = Parser::new(input);
    p.skip_whitespace();
    // Left to right first; compounds are reversed at the end.
    let mut left_to_right = Vec::new();
    let mut pseudo_element = None;
    loop {
        let start = left_to_right.len();
        parse_compound(&mut p, &mut left_to_right, &mut pseudo_element, options)?;
        if left_to_right.len() == start {
            if pseudo_element.is_none() {
                return Err(if p.peek_including_whitespace().is_none() {
                    ParseError::EndOfInput
                } else {
                    ParseError::Unexpected
                });
            }
            // A compound with only a pseudo-element, such as `::before`.
            left_to_right.push(Component::Universal);
        }
        let had_whitespace = p.skip_whitespace();
        let combinator = match p.peek_including_whitespace() {
            None => break,
            Some(ComponentValue::Delim('>')) => Combinator::Child,
            Some(ComponentValue::Delim('+')) => Combinator::NextSibling,
            Some(ComponentValue::Delim('~')) => Combinator::SubsequentSibling,
            Some(_) if had_whitespace => Combinator::Descendant,
            Some(_) => return Err(ParseError::Unexpected),
        };
        // A pseudo-element must be in the last compound.
        if pseudo_element.is_some() {
            return Err(ParseError::Invalid);
        }
        if combinator != Combinator::Descendant {
            p.next_including_whitespace();
            p.skip_whitespace();
        }
        left_to_right.push(Component::Combinator(combinator));
    }
    Ok((reverse_compounds(left_to_right), pseudo_element))
}

/// Reverses the order of compounds (not of the components inside them).
fn reverse_compounds(mut left_to_right: Vec<Component>) -> Vec<Component> {
    let mut right_to_left = Vec::with_capacity(left_to_right.len());
    while let Some(i) = left_to_right
        .iter()
        .rposition(|c| matches!(c, Component::Combinator(_)))
    {
        right_to_left.extend(left_to_right.drain(i + 1..));
        right_to_left.extend(left_to_right.pop());
    }
    right_to_left.append(&mut left_to_right);
    right_to_left
}

/// Parses one compound selector and appends its components to `out`.
fn parse_compound(
    p: &mut Parser<'_>,
    out: &mut Vec<Component>,
    pseudo_element: &mut Option<PseudoElement>,
    options: Options,
) -> Result<(), ParseError> {
    parse_type_selector(p, out)?;
    while let Some(value) = p.peek_including_whitespace() {
        let starts_simple_selector = matches!(
            value,
            ComponentValue::Hash { .. } | ComponentValue::Delim('.') | ComponentValue::Colon
        ) || value.as_block(BlockKind::Square).is_some();
        if !starts_simple_selector {
            break;
        }
        // Nothing may follow a pseudo-element in its compound. (Selectors 4
        // allows some pseudo-classes there; they are rare and not supported.)
        if pseudo_element.is_some() {
            return Err(ParseError::Invalid);
        }
        p.next_including_whitespace();
        match value {
            ComponentValue::Hash { value, is_id: true } => out.push(Component::Id(value.clone())),
            ComponentValue::Hash { is_id: false, .. } => return Err(ParseError::Invalid),
            ComponentValue::Delim('.') => match p.next_including_whitespace() {
                Some(ComponentValue::Ident(name)) => out.push(Component::Class(name.clone())),
                _ => return Err(ParseError::Unexpected),
            },
            ComponentValue::Colon => parse_pseudo(p, out, pseudo_element, options)?,
            ComponentValue::Block(block) => {
                let attribute = parse_attribute(&block.contents)?;
                out.push(Component::Attribute(Box::new(attribute)));
            }
            _ => return Err(ParseError::Unexpected),
        }
    }
    Ok(())
}

/// Parses an optional type selector or universal selector.
fn parse_type_selector(p: &mut Parser<'_>, out: &mut Vec<Component>) -> Result<(), ParseError> {
    match p.peek_including_whitespace() {
        Some(ComponentValue::Ident(name)) => {
            p.next_including_whitespace();
            // `ns|E`: namespace prefixes are never declared.
            if p.peek_including_whitespace()
                .is_some_and(|v| v.is_delim('|'))
            {
                return Err(ParseError::Invalid);
            }
            out.push(Component::LocalName(LocalName::new(name)));
        }
        Some(ComponentValue::Delim('*')) => {
            p.next_including_whitespace();
            if p.peek_including_whitespace()
                .is_some_and(|v| v.is_delim('|'))
            {
                // `*|E` and `*|*`: any namespace.
                p.next_including_whitespace();
                match p.next_including_whitespace() {
                    Some(ComponentValue::Ident(name)) => {
                        out.push(Component::LocalName(LocalName::new(name)));
                    }
                    Some(ComponentValue::Delim('*')) => out.push(Component::Universal),
                    _ => return Err(ParseError::Unexpected),
                }
            } else {
                out.push(Component::Universal);
            }
        }
        // `|E`: elements without a namespace. Not supported.
        Some(ComponentValue::Delim('|')) => return Err(ParseError::Invalid),
        _ => {}
    }
    Ok(())
}

/// Parses a pseudo-class or pseudo-element. The first colon is consumed.
fn parse_pseudo(
    p: &mut Parser<'_>,
    out: &mut Vec<Component>,
    pseudo_element: &mut Option<PseudoElement>,
    options: Options,
) -> Result<(), ParseError> {
    match p.next_including_whitespace() {
        Some(ComponentValue::Colon) => match p.next_including_whitespace() {
            Some(ComponentValue::Ident(name)) => {
                let pe = PseudoElement::from_name(name).ok_or(ParseError::Invalid)?;
                set_pseudo_element(pseudo_element, pe, options)
            }
            _ => Err(ParseError::Invalid),
        },
        Some(ComponentValue::Ident(name)) => {
            if let Some(pe) = PseudoElement::from_name(name).filter(|pe| pe.has_legacy_syntax()) {
                return set_pseudo_element(pseudo_element, pe, options);
            }
            out.push(Component::PseudoClass(pseudo_class(name)?));
            Ok(())
        }
        Some(ComponentValue::Function(f)) => {
            out.push(Component::PseudoClass(functional_pseudo_class(f, options)?));
            Ok(())
        }
        _ => Err(ParseError::Unexpected),
    }
}

fn set_pseudo_element(
    slot: &mut Option<PseudoElement>,
    pe: PseudoElement,
    options: Options,
) -> Result<(), ParseError> {
    if !options.allow_pseudo_element {
        return Err(ParseError::Invalid);
    }
    *slot = Some(pe);
    Ok(())
}

fn pseudo_class(name: &str) -> Result<PseudoClass, ParseError> {
    let lower = name.to_ascii_lowercase();
    let pc = match lower.as_str() {
        "link" => PseudoClass::Link,
        "visited" => PseudoClass::Visited,
        "any-link" => PseudoClass::AnyLink,
        "root" => PseudoClass::Root,
        "empty" => PseudoClass::Empty,
        "scope" => PseudoClass::Scope,
        "host" => PseudoClass::Host(None),
        "first-child" => PseudoClass::FirstChild,
        "last-child" => PseudoClass::LastChild,
        "only-child" => PseudoClass::OnlyChild,
        "first-of-type" => PseudoClass::FirstOfType,
        "last-of-type" => PseudoClass::LastOfType,
        "only-of-type" => PseudoClass::OnlyOfType,
        _ => {
            let (_, state) = STATE_PSEUDO_CLASSES
                .iter()
                .find(|(n, _)| *n == lower)
                .ok_or(ParseError::Invalid)?;
            PseudoClass::State(*state)
        }
    };
    Ok(pc)
}

fn functional_pseudo_class(f: &Function, options: Options) -> Result<PseudoClass, ParseError> {
    let nested = options.nested()?;
    let args = &f.arguments[..];
    let pc = match f.name.to_ascii_lowercase().as_str() {
        "not" => PseudoClass::Not(parse_list(args, nested)?),
        "is" => PseudoClass::Is(parse_forgiving_list(args, nested)),
        "where" => PseudoClass::Where(parse_forgiving_list(args, nested)),
        "has" => {
            if options.inside_has {
                return Err(ParseError::Invalid);
            }
            let nested = Options {
                inside_has: true,
                ..nested
            };
            let selectors = split_on_commas(args)
                .map(|part| parse_relative(part, nested))
                .collect::<Result<Vec<_>, _>>()?;
            PseudoClass::Has(selectors.into_boxed_slice())
        }
        "nth-child" => PseudoClass::NthChild(Box::new(parse_nth(args, nested)?)),
        "nth-last-child" => PseudoClass::NthLastChild(Box::new(parse_nth(args, nested)?)),
        "nth-of-type" => PseudoClass::NthOfType(parse_anb_only(args)?),
        "nth-last-of-type" => PseudoClass::NthLastOfType(parse_anb_only(args)?),
        "lang" => PseudoClass::Lang(parse_lang(args)?),
        "dir" => {
            let mut p = Parser::new(args);
            let dir = p.parse_entirely(Parser::expect_ident)?;
            if dir.eq_ignore_ascii_case("ltr") {
                PseudoClass::Dir(Direction::Ltr)
            } else if dir.eq_ignore_ascii_case("rtl") {
                PseudoClass::Dir(Direction::Rtl)
            } else {
                return Err(ParseError::Invalid);
            }
        }
        "host" => PseudoClass::Host(Some(parse_compound_argument(args, nested)?)),
        "host-context" => PseudoClass::HostContext(parse_compound_argument(args, nested)?),
        _ => return Err(ParseError::Invalid),
    };
    Ok(pc)
}

/// Parses the argument of `:host()` and `:host-context()`: one compound
/// selector, without combinators or pseudo-elements.
fn parse_compound_argument(
    args: &[ComponentValue],
    options: Options,
) -> Result<Box<Selector>, ParseError> {
    let selector = parse_complex(args, options)?;
    if selector
        .components
        .iter()
        .any(|c| matches!(c, Component::Combinator(_)))
    {
        return Err(ParseError::Invalid);
    }
    Ok(Box::new(selector))
}

/// Parses `An+B [of S]?`.
fn parse_nth(args: &[ComponentValue], options: Options) -> Result<Nth, ParseError> {
    let mut p = Parser::new(args);
    let anb = AnB::parse(&mut p)?;
    if p.is_exhausted() {
        return Ok(Nth { anb, of: None });
    }
    p.expect_ident_matching("of")?;
    let of = parse_list(p.remaining(), options)?;
    Ok(Nth { anb, of: Some(of) })
}

fn parse_anb_only(args: &[ComponentValue]) -> Result<AnB, ParseError> {
    Parser::new(args).parse_entirely(AnB::parse)
}

/// Parses the `:lang()` argument: a list of identifiers or strings.
fn parse_lang(args: &[ComponentValue]) -> Result<Box<[Box<str>]>, ParseError> {
    let ranges = Parser::new(args).parse_comma_separated(Parser::expect_ident_or_string)?;
    Ok(ranges.into_iter().map(Box::from).collect())
}

/// Parses the contents of an attribute selector (`[...]`).
fn parse_attribute(contents: &[ComponentValue]) -> Result<AttributeSelector, ParseError> {
    let mut p = Parser::new(contents);
    p.skip_whitespace();
    let name = match p.next_including_whitespace() {
        // `[*|a]`: any namespace (approximated as no namespace).
        Some(ComponentValue::Delim('*')) => {
            expect_adjacent_delim(&mut p, '|')?;
            expect_adjacent_ident(&mut p)?
        }
        // `[|a]`: no namespace.
        Some(ComponentValue::Delim('|')) => expect_adjacent_ident(&mut p)?,
        Some(ComponentValue::Ident(name)) => {
            // `ns|a` is a namespace prefix (never declared); `a|=b` is an
            // operator.
            let mut look = p;
            if look
                .next_including_whitespace()
                .is_some_and(|v| v.is_delim('|'))
                && matches!(
                    look.peek_including_whitespace(),
                    Some(ComponentValue::Ident(_))
                )
            {
                return Err(ParseError::Invalid);
            }
            &**name
        }
        _ => return Err(ParseError::Unexpected),
    };
    let name = LocalName::new(name);
    let html_case_insensitive = HTML_CASE_INSENSITIVE_ATTRIBUTES.contains(&name.lower());
    p.skip_whitespace();
    if p.is_exhausted() {
        return Ok(AttributeSelector {
            name,
            operation: None,
            case: AttrCase::Default,
            html_case_insensitive,
        });
    }
    let operator = match p.next_including_whitespace() {
        Some(ComponentValue::Delim('=')) => AttrOperator::Equals,
        Some(ComponentValue::Delim(c)) => {
            let operator = match c {
                '~' => AttrOperator::Includes,
                '|' => AttrOperator::DashMatch,
                '^' => AttrOperator::Prefix,
                '$' => AttrOperator::Suffix,
                '*' => AttrOperator::Substring,
                _ => return Err(ParseError::Unexpected),
            };
            expect_adjacent_delim(&mut p, '=')?;
            operator
        }
        _ => return Err(ParseError::Unexpected),
    };
    let value = p.expect_ident_or_string()?;
    let case = match p.next() {
        None => AttrCase::Default,
        Some(v) if v.is_ident("i") => AttrCase::Insensitive,
        Some(v) if v.is_ident("s") => AttrCase::Sensitive,
        Some(_) => return Err(ParseError::Unexpected),
    };
    p.expect_exhausted()?;
    Ok(AttributeSelector {
        name,
        operation: Some((operator, value.into())),
        case,
        html_case_insensitive,
    })
}

/// Consumes the delim `c` with no whitespace before it.
fn expect_adjacent_delim(p: &mut Parser<'_>, c: char) -> Result<(), ParseError> {
    match p.next_including_whitespace() {
        Some(v) if v.is_delim(c) => Ok(()),
        _ => Err(ParseError::Unexpected),
    }
}

/// Consumes an identifier with no whitespace before it.
fn expect_adjacent_ident<'a>(p: &mut Parser<'a>) -> Result<&'a str, ParseError> {
    match p.next_including_whitespace() {
        Some(ComponentValue::Ident(name)) => Ok(name),
        _ => Err(ParseError::Unexpected),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selector::Specificity;

    fn parse(css: &str) -> Result<SelectorList, ParseError> {
        SelectorList::parse_str(css)
    }

    /// Parses and serializes; the result must parse to the same list.
    fn normalize(css: &str) -> String {
        let list = parse(css).unwrap_or_else(|e| panic!("{css:?} should parse: {e}"));
        let text = list.to_string();
        let again = parse(&text).unwrap_or_else(|e| panic!("{text:?} should parse: {e}"));
        assert_eq!(list, again, "{css:?} -> {text:?}");
        text
    }

    #[test]
    fn valid_selectors() {
        let cases: &[(&str, &str)] = &[
            ("div", "div"),
            ("DIV", "DIV"),
            ("*", "*"),
            (".a", ".a"),
            ("#b", "#b"),
            ("div.a#b.c", "div.a#b.c"),
            ("*.a", "*.a"),
            ("[href]", "[href]"),
            ("[ href ]", "[href]"),
            ("[type=text]", "[type=\"text\"]"),
            ("[type='a b']", "[type=\"a b\"]"),
            (
                "[a~=b][c|=d][e^=f][g$=h][i*=j]",
                "[a~=\"b\"][c|=\"d\"][e^=\"f\"][g$=\"h\"][i*=\"j\"]",
            ),
            ("[a=b i]", "[a=\"b\" i]"),
            ("[a=b S]", "[a=\"b\" s]"),
            ("[a = \"b\"  i ]", "[a=\"b\" i]"),
            ("[|a]", "[a]"),
            ("[*|a=b]", "[a=\"b\"]"),
            ("*|div", "div"),
            ("*|*", "*"),
            ("a b", "a b"),
            ("a > b", "a > b"),
            ("a>b", "a > b"),
            ("a+b ~ c", "a + b ~ c"),
            ("  a   b  ", "a b"),
            ("a, b", "a, b"),
            ("a:hover", "a:hover"),
            ("a:HOVER", "a:hover"),
            ("a::before", "a::before"),
            ("a:before", "a::before"),
            ("::after", "*::after"),
            ("p::first-line", "p::first-line"),
            ("p:first-letter", "p::first-letter"),
            ("input::placeholder", "input::placeholder"),
            ("::selection", "*::selection"),
            ("li:nth-child(2n+1)", "li:nth-child(2n+1)"),
            ("li:nth-child(odd)", "li:nth-child(2n+1)"),
            ("li:nth-child(-n+3)", "li:nth-child(-n+3)"),
            ("li:nth-child(5)", "li:nth-child(5)"),
            (
                "li:nth-last-child(2n of .a, .b)",
                "li:nth-last-child(2n of .a, .b)",
            ),
            ("p:nth-of-type(even)", "p:nth-of-type(2n)"),
            ("p:nth-last-of-type(n)", "p:nth-last-of-type(n)"),
            ("a:not(.b)", "a:not(.b)"),
            ("a:not(.b, .c d)", "a:not(.b, .c d)"),
            ("a:is(.b, :hover)", "a:is(.b, :hover)"),
            ("a:where(.b)", "a:where(.b)"),
            ("a:is()", "a:is()"),
            ("a:is(.b, ::before, :-moz-x, .c)", "a:is(.b, .c)"),
            ("a:has(> img)", "a:has(> img)"),
            ("a:has(img, + b, ~ c d)", "a:has(img, + b, ~ c d)"),
            ("a:lang(en)", "a:lang(\"en\")"),
            ("a:lang(en, 'fr-CA')", "a:lang(\"en\", \"fr-CA\")"),
            ("a:dir(rtl)", "a:dir(rtl)"),
            (":root", ":root"),
            (":scope > a", ":scope > a"),
            ("a:link:visited:any-link", "a:link:visited:any-link"),
            (
                "input:checked:disabled:enabled",
                "input:checked:disabled:enabled",
            ),
            (
                ":indeterminate:required:optional",
                ":indeterminate:required:optional",
            ),
            (
                ":read-only:read-write:placeholder-shown",
                ":read-only:read-write:placeholder-shown",
            ),
            (
                ":default:defined:target:focus",
                ":default:defined:target:focus",
            ),
            (
                ":focus-visible:focus-within:active",
                ":focus-visible:focus-within:active",
            ),
            (
                ":empty:first-child:last-child:only-child",
                ":empty:first-child:last-child:only-child",
            ),
            (
                ":first-of-type:last-of-type:only-of-type",
                ":first-of-type:last-of-type:only-of-type",
            ),
            (".\\31 23", ".\\31 23"),
            ("#\\31 23", "#\\31 23"),
            ("a\\:b", "a\\:b"),
            ("a:not(:is(:where(:not(b))))", "a:not(:is(:where(:not(b))))"),
            // The end of the input closes open blocks.
            ("[a", "[a]"),
            // `:has()` inside a forgiving list inside `:has()` is dropped.
            (":has(:is(:has(a), b))", ":has(:is(b))"),
        ];
        for (css, expected) in cases {
            assert_eq!(normalize(css), *expected, "{css}");
        }
    }

    /// `:host`, `:host()` and `:host-context()` parse, as in Chromium 148
    /// (`tools/probes/host-sizes-auto.json`); they never match.
    #[test]
    fn shadow_host_selectors() {
        for (css, expected) in [
            (":host", ":host"),
            (":HOST, html", ":host, html"),
            (":host(.x)", ":host(.x)"),
            (":host( div.x:hover )", ":host(div.x:hover)"),
            (":host-context(.x)", ":host-context(.x)"),
            (":host > p", ":host > p"),
            (":host.x, :host:not(.y)", ":host.x, :host:not(.y)"),
            ("p:host::before", "p:host::before"),
            (":is(:host, .a)", ":is(:host, .a)"),
            (":not(:host(*))", ":not(:host(*))"),
        ] {
            assert_eq!(normalize(css), expected, "{css}");
        }
    }

    #[test]
    fn invalid_selectors() {
        let cases = [
            "",
            " ",
            ",",
            "a,",
            ",a",
            "a,,b",
            "#123",
            ".",
            ". a",
            ".1",
            "a > ",
            "> a",
            "a >> b",
            "a + + b",
            "a:",
            "a: hover",
            "a:hovr",
            "a::",
            "a::bfore",
            "::-webkit-scrollbar",
            ":-moz-focusring",
            "::-moz-selection",
            ":-webkit-any(a)",
            "a::before b",
            "a::before.c",
            "a::before:hover",
            "a::before::after",
            ":not(::before)",
            ":not()",
            ":not(a,)",
            ":has(:has(a))",
            ":has()",
            ":has(::before)",
            ":nth-child()",
            ":nth-child(x)",
            ":nth-child(2n of)",
            ":nth-child(2n of ::before)",
            ":nth-child(2n with .a)",
            ":nth-of-type(2n of .a)",
            ":lang()",
            ":lang(1)",
            ":dir(up)",
            ":unknown(a)",
            "ns|a",
            "|a",
            "*|",
            "[ns|a]",
            "[]",
            "[a=]",
            "[a==b]",
            "[a=b c]",
            "[a=b i i]",
            "[a=1]",
            "[a~b]",
            "a!",
            "a{}",
            "a b c >",
            "a::slotted(b)",
            ":host()",
            ":host(.a .b)",
            ":host(.a > .b)",
            ":host(.a, .b)",
            ":host(> p)",
            ":host(::before)",
            ":host-context()",
            ":host-context(.a .b)",
            ":host-context(.a, .b)",
            ":host-foo",
        ];
        for css in cases {
            assert!(parse(css).is_err(), "{css:?} should be invalid");
        }
    }

    #[test]
    fn match_depth_limit() {
        let long = |n: usize| vec!["i"; n].join("+");
        assert!(parse(&long(200)).is_ok());
        assert!(parse(&long(300)).is_err());
        // Nesting counts too.
        let nested = format!(":is({}) {}", long(150), vec!["a"; 150].join(" "));
        assert!(parse(&nested).is_err());
        let nested = format!(":is({}) {}", long(100), vec!["a"; 100].join(" "));
        assert!(parse(&nested).is_ok());
    }

    #[test]
    fn nesting_limit() {
        let deep = format!("{}a{}", ":is(".repeat(200), ")".repeat(200));
        // Too deep: the innermost levels are dropped, but parsing terminates.
        assert!(parse(&deep).is_ok());
        let deep_not = format!("{}a{}", ":not(".repeat(200), ")".repeat(200));
        assert!(parse(&deep_not).is_err());
    }

    #[test]
    fn specificity() {
        let cases: &[(&str, (u32, u32, u32))] = &[
            ("*", (0, 0, 0)),
            ("li", (0, 0, 1)),
            ("ul li", (0, 0, 2)),
            ("ul ol+li", (0, 0, 3)),
            ("h1 + *[rel=up]", (0, 1, 1)),
            ("ul ol li.red", (0, 1, 3)),
            ("li.red.level", (0, 2, 1)),
            ("#x34y", (1, 0, 0)),
            ("#s12:not(foo)", (1, 0, 1)),
            (".foo :is(.bar, #baz)", (1, 1, 0)),
            ("a:where(#x, .y) b", (0, 0, 2)),
            ("a:not(#x, .y)", (1, 0, 1)),
            ("a:has(> #x, .y)", (1, 0, 1)),
            ("a::before", (0, 0, 2)),
            ("a:before", (0, 0, 2)),
            ("::selection", (0, 0, 1)),
            ("a:hover", (0, 1, 1)),
            ("li:nth-child(2n+1)", (0, 1, 1)),
            ("li:nth-child(2n+1 of .a, #b)", (1, 1, 1)),
            ("li:nth-last-child(2n of p)", (0, 1, 2)),
            ("p:nth-of-type(2)", (0, 1, 1)),
            ("[a][b=c]", (0, 2, 0)),
            (":is()", (0, 0, 0)),
            (":lang(en)", (0, 1, 0)),
            (":root", (0, 1, 0)),
            (":host", (0, 1, 0)),
            (":host(#a.b)", (1, 2, 0)),
            (":host-context(p)", (0, 1, 1)),
        ];
        for (css, (a, b, c)) in cases {
            let list = parse(css).unwrap_or_else(|e| panic!("{css}: {e}"));
            assert_eq!(
                list.selectors()[0].specificity(),
                Specificity::new(*a, *b, *c),
                "{css}"
            );
        }
    }

    #[test]
    fn pseudo_element_and_bucket_key() {
        use crate::selector::BucketKey;
        let key = |css: &str| {
            let list = parse(css).expect("valid");
            let key = format!("{:?}", list.selectors()[0].bucket_key());
            key
        };
        assert_eq!(key("div#a.b"), format!("{:?}", BucketKey::Id("a")));
        assert_eq!(key("div.b.c"), format!("{:?}", BucketKey::Class("b")));
        assert_eq!(
            key("x .y DIV:hover"),
            format!(
                "{:?}",
                BucketKey::LocalName {
                    name: "DIV",
                    lower_name: "div"
                }
            )
        );
        assert_eq!(key(":is(.a)"), format!("{:?}", BucketKey::Universal));
        assert_eq!(key(".a > *"), format!("{:?}", BucketKey::Universal));
        assert_eq!(key("a.b::before"), format!("{:?}", BucketKey::Class("b")));
        let list = parse("a::marker, b").expect("valid");
        assert_eq!(
            list.selectors()[0].pseudo_element(),
            Some(PseudoElement::Marker)
        );
        assert_eq!(list.selectors()[1].pseudo_element(), None);
    }

    #[test]
    fn ancestor_keys() {
        use crate::selector::BucketKey;
        let keys = |css: &str| {
            let list = parse(css).expect("valid");
            format!("{:?}", list.selectors()[0].ancestor_keys())
        };
        let local = |name, lower_name| BucketKey::LocalName { name, lower_name };
        assert_eq!(keys(".a"), "[]");
        assert_eq!(
            keys("DIV#x.y > p.z .s"),
            format!(
                "{:?}",
                [
                    local("DIV", "div"),
                    BucketKey::Id("x"),
                    BucketKey::Class("y"),
                    local("p", "p"),
                    BucketKey::Class("z"),
                ]
            )
        );
        // Siblings are not ancestors; the ancestor of a sibling is.
        assert_eq!(keys(".s + .t"), "[]");
        assert_eq!(keys(".a .s ~ .t"), format!("{:?}", [BucketKey::Class("a")]));
        // Selector arguments, attributes and `*` give no keys.
        assert_eq!(keys(":is(.a) [b] * .c"), "[]");
        assert_eq!(
            keys(".a:not(.b):hover .c"),
            format!("{:?}", [BucketKey::Class("a")])
        );
    }

    #[test]
    fn state_dependencies() {
        use crate::selector::ElementState;
        let deps = |css: &str| parse(css).expect("valid").selectors()[0].state_dependencies();
        assert_eq!(deps("a.b > c"), ElementState::empty());
        assert_eq!(deps("a:hover"), ElementState::HOVER);
        assert_eq!(deps("a:link"), ElementState::VISITED);
        assert_eq!(
            deps(":is(:focus) :not(:checked):has(:active)"),
            ElementState::FOCUS | ElementState::CHECKED | ElementState::ACTIVE
        );
        assert_eq!(deps(":nth-child(2 of :disabled)"), ElementState::DISABLED);
    }

    #[test]
    fn components_are_right_to_left() {
        let list = parse("a.x > b c").expect("valid");
        let components = &list.selectors()[0].components;
        assert!(matches!(&components[0], Component::LocalName(n) if &*n.name == "c"));
        assert_eq!(components[1], Component::Combinator(Combinator::Descendant));
        assert!(matches!(&components[2], Component::LocalName(n) if &*n.name == "b"));
        assert_eq!(components[3], Component::Combinator(Combinator::Child));
        assert!(matches!(&components[4], Component::LocalName(n) if &*n.name == "a"));
        assert!(matches!(&components[5], Component::Class(c) if &**c == "x"));
    }
}
