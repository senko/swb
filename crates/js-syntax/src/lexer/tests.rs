//! Tests of the lexer. Expected values and messages come from ECMA-262 and
//! from Node.js 22 (V8) where the specification leaves the message open.

use swb_js_regexp::Flags;
use swb_js_text::{Str16, String16};

use super::*;
use crate::interner::names;
use crate::token::{Legacy, Template, TokenKind as K};

/// Lexes `units` to the end. `goal` chooses the goal symbol from the
/// tokens so far. Returns the tokens (without the end) and the name table.
fn lex_units<U: CodeUnit>(
    units: &[U],
    options: LexerOptions,
    goal: &mut dyn FnMut(&[Token]) -> Goal,
) -> Result<(Vec<Token>, Interner), SyntaxError> {
    let mut lexer = Lexer::new(units, options, Interner::new());
    let mut tokens = Vec::new();
    loop {
        let token = lexer.next_token(goal(&tokens))?;
        if token.kind == K::Eof {
            return Ok((tokens, lexer.into_interner()));
        }
        tokens.push(token);
    }
}

/// Lexes `source` in both widths (the narrow one if the source fits) and
/// checks that they agree.
fn lex_with(
    source: &str,
    options: LexerOptions,
    mut goal: impl FnMut(&[Token]) -> Goal,
) -> Result<(Vec<Token>, Interner), SyntaxError> {
    let wide: Vec<u16> = source.encode_utf16().collect();
    let result = lex_units(&wide, options, &mut goal);
    if wide.iter().all(|&u| u < 256) {
        let narrow: Vec<u8> = wide.iter().map(|&u| u as u8).collect();
        let narrow_result = lex_units(&narrow, options, &mut goal);
        match (&result, &narrow_result) {
            (Ok((a, _)), Ok((b, _))) => assert_eq!(a, b, "{source:?}"),
            (Err(a), Err(b)) => assert_eq!(a, b, "{source:?}"),
            _ => panic!("the widths disagree on {source:?}"),
        }
    }
    result
}

/// The tokens of a source that contains no regular expression literals
/// and no template substitutions.
fn tokens(source: &str) -> Vec<Token> {
    match lex_with(source, LexerOptions::script(), |_| Goal::Div) {
        Ok((tokens, _)) => tokens,
        Err(error) => panic!("{source:?}: {error}"),
    }
}

fn kinds(source: &str) -> Vec<TokenKind> {
    tokens(source).iter().map(|t| t.kind).collect()
}

/// The only token of `source`, with the goal `goal`.
fn single(source: &str, goal: Goal) -> Token {
    match lex_with(source, LexerOptions::script(), |_| goal) {
        Ok((mut tokens, _)) if tokens.len() == 1 => tokens.remove(0),
        Ok((tokens, _)) => panic!("{source:?}: {} tokens: {tokens:?}", tokens.len()),
        Err(error) => panic!("{source:?}: {error}"),
    }
}

/// The error of `source`, with the goal `goal`.
fn error_with(source: &str, goal: Goal) -> SyntaxError {
    match lex_with(source, LexerOptions::script(), |_| goal) {
        Ok((tokens, _)) => panic!("{source:?} lexes: {tokens:?}"),
        Err(error) => error,
    }
}

fn error(source: &str) -> SyntaxError {
    error_with(source, Goal::Div)
}

fn number(source: &str) -> (f64, Legacy) {
    let token = single(source, Goal::Div);
    match token.value {
        TokenValue::Number(value) if token.kind == K::Number => (value, token.legacy),
        _ => panic!("{source:?} is not a number: {token:?}"),
    }
}

fn string_value(source: &str) -> (String16, Legacy) {
    let token = single(source, Goal::Div);
    match token.value {
        TokenValue::String(value) => (value, token.legacy),
        _ => panic!("{source:?} is not a string: {token:?}"),
    }
}

fn units(text: &String16) -> Vec<u16> {
    text.as_str16().units().collect()
}

fn template(token: &Token) -> &Template {
    match &token.value {
        TokenValue::Template(template) => template,
        _ => panic!("not a template: {token:?}"),
    }
}

fn cooked(token: &Token) -> String {
    match &template(token).cooked {
        Ok(text) => text.to_string_lossy(),
        Err(error) => panic!("no cooked value: {error}"),
    }
}

fn raw(token: &Token) -> String {
    template(token).raw.to_string_lossy()
}

/// The goal that a parser would choose in a source of identifiers,
/// punctuators and templates: a template continues at a `}` that closes a
/// substitution.
fn template_goal(tokens: &[Token]) -> Goal {
    let mut depths: Vec<u32> = Vec::new();
    for token in tokens {
        match token.kind {
            K::TemplateHead => depths.push(0),
            K::TemplateTail => {
                depths.pop();
            }
            K::LBrace => {
                if let Some(depth) = depths.last_mut() {
                    *depth += 1;
                }
            }
            K::RBrace => {
                if let Some(depth) = depths.last_mut() {
                    *depth = depth.saturating_sub(1);
                }
            }
            _ => {}
        }
    }
    if depths.last() == Some(&0) {
        Goal::TemplateTail
    } else {
        Goal::Div
    }
}

#[test]
fn punctuators() {
    let source = "{ } ( ) [ ] . ... ; , < > <= >= == != === !== + - * / % ** ++ -- << >> >>> \
                  & | ^ ! ~ && || ?? ? ?. : = += -= *= /= %= **= <<= >>= >>>= &= |= ^= &&= ||= \
                  ??= =>";
    assert_eq!(
        kinds(source),
        [
            K::LBrace,
            K::RBrace,
            K::LParen,
            K::RParen,
            K::LBracket,
            K::RBracket,
            K::Dot,
            K::Ellipsis,
            K::Semicolon,
            K::Comma,
            K::Lt,
            K::Gt,
            K::LtEq,
            K::GtEq,
            K::EqEq,
            K::NotEq,
            K::EqEqEq,
            K::NotEqEq,
            K::Plus,
            K::Minus,
            K::Star,
            K::Slash,
            K::Percent,
            K::StarStar,
            K::PlusPlus,
            K::MinusMinus,
            K::Shl,
            K::Shr,
            K::UShr,
            K::Amp,
            K::Pipe,
            K::Caret,
            K::Bang,
            K::Tilde,
            K::AmpAmp,
            K::PipePipe,
            K::QuestionQuestion,
            K::Question,
            K::QuestionDot,
            K::Colon,
            K::Eq,
            K::PlusEq,
            K::MinusEq,
            K::StarEq,
            K::SlashEq,
            K::PercentEq,
            K::StarStarEq,
            K::ShlEq,
            K::ShrEq,
            K::UShrEq,
            K::AmpEq,
            K::PipeEq,
            K::CaretEq,
            K::AmpAmpEq,
            K::PipePipeEq,
            K::QuestionQuestionEq,
            K::Arrow,
        ]
    );
    for token in tokens(source) {
        assert_eq!(token.end - token.start, token.kind.text().len() as u32);
    }
}

#[test]
fn longest_punctuators() {
    assert_eq!(kinds(">>>>="), [K::UShr, K::GtEq]);
    assert_eq!(
        kinds("a--->b"),
        [K::Identifier, K::MinusMinus, K::Minus, K::Gt, K::Identifier]
    );
    assert_eq!(kinds("...."), [K::Ellipsis, K::Dot]);
    assert_eq!(kinds(".."), [K::Dot, K::Dot]);
    assert_eq!(kinds("!==="), [K::NotEqEq, K::Eq]);
    assert_eq!(
        kinds("a?.b"),
        [K::Identifier, K::QuestionDot, K::Identifier]
    );
    // `?.` followed by a digit is `?` and a number.
    assert_eq!(
        kinds("a?.5:1"),
        [K::Identifier, K::Question, K::Number, K::Colon, K::Number]
    );
    assert_eq!(kinds("a ?. [0]")[1], K::QuestionDot);
    assert_eq!(
        kinds("x=>{}"),
        [K::Identifier, K::Arrow, K::LBrace, K::RBrace]
    );
}

#[test]
fn identifiers_and_reserved_words() {
    let (tokens, interner) = lex_with(
        "if iff let yield await $ _x \u{e9}t\u{e9} \u{3c0} x\u{10000} a\u{200c}b",
        LexerOptions::script(),
        |_| Goal::Div,
    )
    .expect("valid identifiers");
    let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        [
            K::If,
            K::Identifier,
            K::Identifier,
            K::Yield,
            K::Await,
            K::Identifier,
            K::Identifier,
            K::Identifier,
            K::Identifier,
            K::Identifier,
            K::Identifier,
        ]
    );
    let names: Vec<String> = tokens
        .iter()
        .filter_map(Token::name)
        .map(|name| {
            interner
                .get(name)
                .map(Str16::to_string_lossy)
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(
        names,
        [
            "iff",
            "let",
            "$",
            "_x",
            "\u{e9}t\u{e9}",
            "\u{3c0}",
            "x\u{10000}",
            "a\u{200c}b"
        ]
    );
    assert_eq!(tokens[2].name(), Some(names::LET));
    assert!(tokens.iter().all(|t| !t.escaped));
    // `x\u{10000}` is three code units.
    assert_eq!(tokens[9].end - tokens[9].start, 3);
}

#[test]
fn identifiers_with_escapes() {
    let (tokens, interner) = lex_with(
        "\\u0069f \\u{69}\\u{66} a\\u0062c \\u{1d4b6} \\u0024 a\\u200c",
        LexerOptions::script(),
        |_| Goal::Div,
    )
    .expect("valid escapes");
    assert!(tokens.iter().all(|t| t.kind == K::Identifier && t.escaped));
    // An escaped reserved word is an identifier with the reserved word's
    // name; the parser rejects it where a reserved word is not allowed.
    assert_eq!(tokens[0].name(), Some(names::IF));
    assert_eq!(tokens[1].name(), Some(names::IF));
    assert_eq!(TokenKind::keyword_from_name(names::IF), Some(K::If));
    let text = |i: usize| {
        let name = tokens[i].name().expect("identifiers have names");
        interner
            .get(name)
            .map(Str16::to_string_lossy)
            .unwrap_or_default()
    };
    assert_eq!(text(2), "abc");
    assert_eq!(text(3), "\u{1d4b6}");
    assert_eq!(text(4), "$");
    assert_eq!(text(5), "a\u{200c}");
}

#[test]
fn identifier_errors() {
    for (source, offset) in [
        ("a\\u0020", 1),
        ("\\u0031a", 0),
        ("\\u200c", 0),
        ("\\x41", 0),
        ("a\\", 1),
        ("\u{2060}", 0),
        ("@", 0),
        ("\u{180e}", 0),
    ] {
        let error = error(source);
        assert_eq!(error.message, INVALID_TOKEN, "{source:?}");
        assert_eq!(error.offset, offset, "{source:?}");
    }
    assert_eq!(error("\\u12").message, "Invalid Unicode escape sequence");
    assert_eq!(error("\\u{}").message, "Invalid Unicode escape sequence");
    assert_eq!(error("\\u{110000}").message, "Undefined Unicode code-point");
}

#[test]
fn private_names() {
    let (tokens, interner) = lex_with("#x #if #\\u0061 #\u{e9}", LexerOptions::script(), |_| {
        Goal::Div
    })
    .expect("valid private names");
    assert!(tokens.iter().all(|t| t.kind == K::PrivateName));
    assert_eq!(tokens[1].name(), Some(names::IF));
    assert!(tokens[2].escaped);
    let a = tokens[2]
        .name()
        .and_then(|n| interner.get(n))
        .map(Str16::to_string_lossy);
    assert_eq!(a.as_deref(), Some("a"));
    assert_eq!(error("#").offset, 0);
    assert_eq!(error("# x").offset, 0);
    assert_eq!(error("#1").offset, 0);
    assert_eq!(error("x #!y").offset, 2);
}

#[test]
fn decimal_numbers() {
    let cases: &[(&str, f64)] = &[
        ("0", 0.0),
        ("42", 42.0),
        ("4.35", 4.35),
        ("5.", 5.0),
        ("5.e3", 5000.0),
        (".5e-7", 5e-8),
        ("0.e1", 0.0),
        ("0.0000001", 1e-7),
        ("1_000.000_1", 1000.0001),
        ("0.0_1", 0.01),
        ("1e1_0", 1e10),
        ("1E+2", 100.0),
        ("1e400", f64::INFINITY),
        ("1e-400", 0.0),
        ("1e99999999999999999999", f64::INFINITY),
        ("9007199254740993", 9_007_199_254_740_992.0),
        ("2.2250738585072011e-308", 2.225_073_858_507_201e-308),
        ("17976931348623158e292", f64::MAX),
        ("4.9e-324", 5e-324),
    ];
    for &(source, expected) in cases {
        assert_eq!(number(source), (expected, Legacy::None), "{source}");
    }
}

#[test]
fn radix_numbers() {
    let cases: &[(&str, f64)] = &[
        ("0x1f", 31.0),
        ("0X1F", 31.0),
        ("0x1_f", 31.0),
        ("0b1_0", 2.0),
        ("0B11", 3.0),
        ("0o17", 15.0),
        ("0O7_7", 63.0),
        ("0x1fffffffffffff1", 144_115_188_075_855_860.0),
        ("0x20000000000001", 9_007_199_254_740_992.0),
        ("0x20000000000003", 9_007_199_254_740_996.0),
        ("0o7777777777777777777777", 73_786_976_294_838_210_000.0),
    ];
    for &(source, expected) in cases {
        assert_eq!(number(source), (expected, Legacy::None), "{source}");
    }
}

#[test]
fn legacy_numbers() {
    // Annex B.1.1: legacy octal and decimal integers with a leading zero
    // are flagged for strict mode.
    let octal = Legacy::OctalInteger;
    let decimal = Legacy::LeadingZeroDecimal;
    let cases: &[(&str, f64, Legacy)] = &[
        ("00", 0.0, octal),
        ("017", 15.0, octal),
        ("0777777777777777777777777", 4.722_366_482_869_645e21, octal),
        ("08", 8.0, decimal),
        ("019", 19.0, decimal),
        ("08.5", 8.5, decimal),
        ("089e1", 890.0, decimal),
        ("09.5e1", 95.0, decimal),
        ("08.1_2", 8.12, decimal),
    ];
    for &(source, expected, legacy) in cases {
        assert_eq!(number(source), (expected, legacy), "{source}");
    }
    // `07.5` is `07` and `.5`; `07.x` is a member access.
    assert_eq!(kinds("07.5"), [K::Number, K::Number]);
    assert_eq!(kinds("07.toString"), [K::Number, K::Dot, K::Identifier]);
    assert_eq!(kinds("5..toString"), [K::Number, K::Dot, K::Identifier]);
}

#[test]
fn bigints() {
    for (source, text) in [
        ("0n", "0"),
        ("123n", "123"),
        ("1_0n", "10"),
        ("0x1F_fn", "0x1Ff"),
        ("0B1n", "0b1"),
        ("0o7n", "0o7"),
    ] {
        let token = single(source, Goal::Div);
        assert_eq!(token.kind, K::BigInt, "{source}");
        assert_eq!(token.value, TokenValue::BigInt(text.into()), "{source}");
    }
}

#[test]
fn number_errors() {
    let separator_after_zero = "Numeric separator can not be used after leading 0.";
    let twice = "Only one underscore is allowed as numeric separator";
    let at_end = "Numeric separators are not allowed at the end of numeric literals";
    for (source, message) in [
        ("0_1", separator_after_zero),
        ("0_n", separator_after_zero),
        ("1__0", twice),
        ("0x1__2", twice),
        ("1_", at_end),
        ("1_.5", at_end),
        ("1_n", at_end),
        ("1e1_", at_end),
        ("1._5", INVALID_TOKEN),
        ("1e_1", INVALID_TOKEN),
        ("0x_1", INVALID_TOKEN),
        ("07_7", INVALID_TOKEN),
        ("08_1", INVALID_TOKEN),
        ("1.5n", INVALID_TOKEN),
        ("1e3n", INVALID_TOKEN),
        ("07n", INVALID_TOKEN),
        ("08n", INVALID_TOKEN),
        ("00n", INVALID_TOKEN),
        ("1n_", INVALID_TOKEN),
        ("3in", INVALID_TOKEN),
        ("3\\u0061", INVALID_TOKEN),
        ("3\u{e9}", INVALID_TOKEN),
        ("0b12", INVALID_TOKEN),
        ("0o8", INVALID_TOKEN),
        ("0x", INVALID_TOKEN),
        ("0b", INVALID_TOKEN),
        ("1e", INVALID_TOKEN),
        ("1e+", INVALID_TOKEN),
        ("08.toString()", INVALID_TOKEN),
        ("07e1", INVALID_TOKEN),
        ("0x1g", INVALID_TOKEN),
    ] {
        assert_eq!(error(source).message, message, "{source}");
    }
    assert_eq!(error("3in").offset, 1);
    assert_eq!(error("0_1").offset, 1);
}

#[test]
fn string_escapes() {
    let cases: &[(&str, &[u16])] = &[
        ("'abc'", &[0x61, 0x62, 0x63]),
        ("\"a'b\"", &[0x61, 0x27, 0x62]),
        ("'\\b\\f\\n\\r\\t\\v'", &[8, 0xC, 0xA, 0xD, 9, 0xB]),
        ("'\\'\\\"\\\\'", &[0x27, 0x22, 0x5C]),
        (
            "'\\x41\\u0042\\u{43}\\u{0000000041}'",
            &[0x41, 0x42, 0x43, 0x41],
        ),
        ("'\\u{1F600}'", &[0xD83D, 0xDE00]),
        ("'\\ud83d\\ude00'", &[0xD83D, 0xDE00]),
        ("'\\u{dc00}'", &[0xDC00]),
        ("'\\c\\q'", &[0x63, 0x71]),
        ("'\\0'", &[0]),
        ("'a\\\nb'", &[0x61, 0x62]),
        ("'a\\\r\nb'", &[0x61, 0x62]),
        ("'a\\\rb'", &[0x61, 0x62]),
        ("'a\\\u{2028}b'", &[0x61, 0x62]),
        ("'a\u{2028}b\u{2029}'", &[0x61, 0x2028, 0x62, 0x2029]),
        ("'\u{e9}\u{3b1}'", &[0xE9, 0x3B1]),
        ("'\\\u{e9}'", &[0xE9]),
        ("''", &[]),
    ];
    for &(source, expected) in cases {
        let (value, legacy) = string_value(source);
        assert_eq!(units(&value), expected, "{source}");
        assert_eq!(legacy, Legacy::None, "{source}");
    }
    assert!(string_value("'abc'").0.is_latin1());
    assert!(!string_value("'\\u03b1'").0.is_latin1());
}

#[test]
fn legacy_string_escapes() {
    // Annex B.1.2: LegacyOctalEscapeSequence and
    // NonOctalDecimalEscapeSequence, flagged for strict mode.
    let octal = Legacy::OctalEscape;
    let eight_nine = Legacy::EightOrNine;
    let cases: &[(&str, &[u16], Legacy)] = &[
        ("'\\07'", &[7], octal),
        ("'\\08'", &[0, 0x38], octal),
        ("'\\8'", &[0x38], eight_nine),
        ("'\\9'", &[0x39], eight_nine),
        ("'\\400'", &[0x20, 0x30], octal),
        ("'\\377'", &[0xFF], octal),
        ("'\\1234'", &[0x53, 0x34], octal),
        ("'\\18'", &[1, 0x38], octal),
        // The first legacy escape decides.
        ("'\\8\\07'", &[0x38, 7], eight_nine),
    ];
    for &(source, expected, kind) in cases {
        let (value, legacy) = string_value(source);
        assert_eq!(units(&value), expected, "{source}");
        assert_eq!(legacy, kind, "{source}");
    }
}

#[test]
fn string_errors() {
    for (source, message, offset) in [
        ("'\\x4'", "Invalid hexadecimal escape sequence", 1),
        ("'\\xg0'", "Invalid hexadecimal escape sequence", 1),
        ("'\\u12'", "Invalid Unicode escape sequence", 1),
        ("'\\u{}'", "Invalid Unicode escape sequence", 1),
        ("'\\u{12'", "Invalid Unicode escape sequence", 1),
        ("'\\u{110000}'", "Undefined Unicode code-point", 1),
        ("'abc", INVALID_TOKEN, 0),
        ("'a\nb'", INVALID_TOKEN, 0),
        ("'a\rb'", INVALID_TOKEN, 0),
        ("'\\", INVALID_TOKEN, 1),
        ("x = \"", INVALID_TOKEN, 4),
    ] {
        let error = error(source);
        assert_eq!(
            (error.message.as_ref(), error.offset),
            (message, offset),
            "{source}"
        );
    }
}

#[test]
fn templates_without_substitutions() {
    let token = single("`abc`", Goal::Div);
    assert_eq!(token.kind, K::NoSubstitutionTemplate);
    assert_eq!((cooked(&token), raw(&token)), ("abc".into(), "abc".into()));
    let token = single("`a\r\nb\rc`", Goal::Div);
    assert_eq!(
        (cooked(&token), raw(&token)),
        ("a\nb\nc".into(), "a\nb\nc".into())
    );
    let token = single("`\\n\\x41\\u{42}`", Goal::Div);
    assert_eq!(
        (cooked(&token), raw(&token)),
        ("\nAB".into(), "\\n\\x41\\u{42}".into())
    );
    let token = single("`a\\\r\nb`", Goal::Div);
    assert_eq!(
        (cooked(&token), raw(&token)),
        ("ab".into(), "a\\\nb".into())
    );
    let token = single("`$ $$ $}`", Goal::Div);
    assert_eq!(cooked(&token), "$ $$ $}");
    let token = single("`\\``", Goal::Div);
    assert_eq!((cooked(&token), raw(&token)), ("`".into(), "\\`".into()));
}

#[test]
fn templates_with_invalid_escapes() {
    // NotEscapeSequence: the cooked value is absent, the raw value stays.
    for (source, raw_value, message) in [
        ("`\\u{`", "\\u{", "Invalid Unicode escape sequence"),
        (
            "`\\01`",
            "\\01",
            "Octal escape sequences are not allowed in template strings.",
        ),
        (
            "`\\1`",
            "\\1",
            "Octal escape sequences are not allowed in template strings.",
        ),
        (
            "`\\8`",
            "\\8",
            "\\8 and \\9 are not allowed in template strings.",
        ),
        ("`\\xg`", "\\xg", "Invalid hexadecimal escape sequence"),
        (
            "`\\u{110000}`",
            "\\u{110000}",
            "Undefined Unicode code-point",
        ),
        ("`a\\u$b`", "a\\u$b", "Invalid Unicode escape sequence"),
    ] {
        let token = single(source, Goal::Div);
        assert_eq!(raw(&token), raw_value, "{source}");
        let error = template(&token)
            .cooked
            .clone()
            .expect_err("no cooked value");
        assert_eq!(error.message, message, "{source}");
    }
    // `\u` followed by `${` starts a substitution.
    let (tokens, _) = lex_with("`\\u${x}`", LexerOptions::script(), template_goal)
        .expect("a template with a substitution");
    assert_eq!(tokens[0].kind, K::TemplateHead);
    assert_eq!(raw(&tokens[0]), "\\u");
    assert!(template(&tokens[0]).cooked.is_err());
}

#[test]
fn templates_with_substitutions() {
    let (tokens, _) = lex_with("`a${b}c${ {d} }e`", LexerOptions::script(), template_goal)
        .expect("a template with substitutions");
    let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        [
            K::TemplateHead,
            K::Identifier,
            K::TemplateMiddle,
            K::LBrace,
            K::Identifier,
            K::RBrace,
            K::TemplateTail,
        ]
    );
    assert_eq!(cooked(&tokens[0]), "a");
    assert_eq!(cooked(&tokens[2]), "c");
    assert_eq!(cooked(&tokens[6]), "e");
    assert_eq!((tokens[2].start, tokens[2].end), (5, 9));
    // `}` is a punctuator unless the goal allows a template tail.
    assert_eq!(
        single("}`", Goal::RegExpOrTemplateTail).kind,
        K::TemplateTail
    );
    assert_eq!(single("}x${", Goal::TemplateTail).kind, K::TemplateMiddle);
    assert_eq!(kinds_with("}`", Goal::RegExp)[0], K::RBrace);
}

fn kinds_with(source: &str, goal: Goal) -> Vec<TokenKind> {
    let wide: Vec<u16> = source.encode_utf16().collect();
    let mut lexer = Lexer::new(&wide[..], LexerOptions::script(), Interner::new());
    let mut kinds = Vec::new();
    while let Ok(token) = lexer.next_token(goal) {
        if token.kind == K::Eof {
            break;
        }
        kinds.push(token.kind);
    }
    kinds
}

#[test]
fn template_errors() {
    for source in ["`abc", "`a$", "`\\", "}abc"] {
        let goal = if source.starts_with('}') {
            Goal::TemplateTail
        } else {
            Goal::Div
        };
        let error = error_with(source, goal);
        assert_eq!(error.message, "Unexpected end of input", "{source}");
        assert_eq!(error.offset, 0, "{source}");
    }
    // An escape error at the end of the source is the escape's own error
    // (Node.js 22).
    for (source, message) in [
        ("`\\u{12", "Invalid Unicode escape sequence"),
        ("`\\u", "Invalid Unicode escape sequence"),
        ("`\\u{110000", "Undefined Unicode code-point"),
    ] {
        let error = error_with(source, Goal::Div);
        assert_eq!(error.message, message, "{source}");
    }
}

#[test]
fn regular_expressions() {
    let token = single("/ab+c/gi", Goal::RegExp);
    assert_eq!(token.kind, K::RegExp);
    let TokenValue::RegExp { body_end, flags } = token.value else {
        panic!("not a regular expression: {token:?}");
    };
    assert_eq!(body_end, 5);
    assert!(flags.contains(Flags::GLOBAL) && flags.contains(Flags::IGNORE_CASE));
    for source in [
        "/[/]/",
        "/a\\/b/",
        "/[\\]/]/",
        "/=/",
        "/(?:)/v",
        "/\u{e9}\u{3b1}/dgimsy",
    ] {
        assert_eq!(single(source, Goal::RegExp).kind, K::RegExp, "{source}");
        assert_eq!(
            single(source, Goal::HashbangOrRegExp).kind,
            K::RegExp,
            "{source}"
        );
    }
    // A regular expression ends at the first `/` outside a class.
    let wide: Vec<u16> = "/a/ /b/".encode_utf16().collect();
    let mut lexer = Lexer::new(&wide[..], LexerOptions::script(), Interner::new());
    let first = lexer
        .next_token(Goal::RegExp)
        .expect("a regular expression");
    assert_eq!((first.start, first.end), (0, 3));
    // With the division goal, `/` is a punctuator.
    assert_eq!(
        kinds("a / b /= c"),
        [
            K::Identifier,
            K::Slash,
            K::Identifier,
            K::SlashEq,
            K::Identifier
        ]
    );
}

#[test]
fn regular_expression_errors() {
    let missing = "Invalid regular expression: missing /";
    let flags = "Invalid regular expression flags";
    for (source, message) in [
        ("/abc", missing),
        ("/a\n/", missing),
        ("/a\u{2028}/", missing),
        ("/a\\\n/", missing),
        ("/[/", missing),
        ("/a\\", missing),
        ("/a/gg", flags),
        ("/a/x", flags),
        ("/a/uv", flags),
        ("/a/\u{e9}", flags),
        // Flags cannot contain escapes; Chromium reports a flag error.
        ("/a/\\u0067", flags),
        ("/a/g\\u0067", flags),
    ] {
        let error = error_with(source, Goal::RegExp);
        assert_eq!(
            (error.message.as_ref(), error.offset),
            (message, 0),
            "{source}"
        );
    }
}

#[test]
fn comments_and_line_terminators() {
    let newlines =
        |source: &str| -> Vec<bool> { tokens(source).iter().map(|t| t.newline_before).collect() };
    assert_eq!(newlines("a // c\nb"), [false, true]);
    assert_eq!(newlines("a /* c */ b"), [false, false]);
    assert_eq!(newlines("a /*\n*/ b"), [false, true]);
    assert_eq!(newlines("a /*\u{2029}*/ b"), [false, true]);
    assert_eq!(newlines("a\u{2028}b"), [false, true]);
    assert_eq!(newlines("a\r\nb\rc"), [false, true, true]);
    assert_eq!(newlines("\na"), [true]);
    assert_eq!(kinds("a /**/ b /***/ c /* * / */"), [K::Identifier; 3]);
    assert_eq!(
        kinds("\u{a0}\u{3000}\u{feff}\u{2003}a\t\u{b}\u{c}"),
        [K::Identifier]
    );
    let error = error("a /* b");
    assert_eq!((error.message.as_ref(), error.offset), (INVALID_TOKEN, 2));
}

#[test]
fn html_like_comments() {
    assert_eq!(kinds("a <!-- b\nc"), [K::Identifier, K::Identifier]);
    assert_eq!(kinds("a\n--> b\nc"), [K::Identifier, K::Identifier]);
    assert_eq!(
        kinds("a\n /* x */ --> b\nc"),
        [K::Identifier, K::Identifier]
    );
    assert_eq!(kinds("a /*\n*/ --> b\nc"), [K::Identifier, K::Identifier]);
    // Not at the start of a line: `--` and `>`.
    assert_eq!(
        kinds("a --> b"),
        [K::Identifier, K::MinusMinus, K::Gt, K::Identifier]
    );
    // At the start of the input (InputElementHashbangOrRegExp).
    let (tokens, _) = lex_with("--> x\ny", LexerOptions::script(), |tokens| {
        if tokens.is_empty() {
            Goal::HashbangOrRegExp
        } else {
            Goal::Div
        }
    })
    .expect("a comment and an identifier");
    assert_eq!(tokens.len(), 1);
    // After white space and a comment at the start of the input, too.
    assert_eq!(kinds(" /* c */ --> x\ny"), [K::Identifier]);
    // Not after a token on the same line, whatever the goal.
    let (tokens, _) = lex_with("x --> y", LexerOptions::script(), |_| {
        Goal::HashbangOrRegExp
    })
    .expect("punctuators");
    assert_eq!(tokens.len(), 4);
    // Modules have no HTML-like comments.
    let (tokens, _) =
        lex_with("a <!-- b", LexerOptions::module(), |_| Goal::Div).expect("punctuators");
    let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        [K::Identifier, K::Lt, K::Bang, K::MinusMinus, K::Identifier]
    );
    let (tokens, _) =
        lex_with("a\n--> b", LexerOptions::module(), |_| Goal::Div).expect("punctuators");
    assert_eq!(tokens.len(), 4);
}

#[test]
fn hashbang() {
    let goal = |tokens: &[Token]| {
        if tokens.is_empty() {
            Goal::HashbangOrRegExp
        } else {
            Goal::Div
        }
    };
    let (tokens, _) = lex_with("#!/usr/bin/env node\nx", LexerOptions::script(), goal)
        .expect("a hashbang comment");
    assert_eq!(tokens.len(), 1);
    assert!(tokens[0].newline_before);
    assert!(lex_with("#!x", LexerOptions::module(), goal).is_ok_and(|(t, _)| t.is_empty()));
    // Only at the start of the source, and only with that goal.
    assert_eq!(error_with("#!x", Goal::RegExp).offset, 0);
    assert_eq!(error_with(" #!x", Goal::HashbangOrRegExp).offset, 1);
}

#[test]
fn end_of_input_repeats() {
    let wide: Vec<u16> = "a".encode_utf16().collect();
    let mut lexer = Lexer::new(&wide[..], LexerOptions::script(), Interner::new());
    assert_eq!(
        lexer.next_token(Goal::Div).map(|t| t.kind),
        Ok(K::Identifier)
    );
    for _ in 0..3 {
        let token = lexer.next_token(Goal::Div).expect("the end");
        assert_eq!((token.kind, token.start, token.end), (K::Eof, 1, 1));
    }
}

#[test]
fn rescan_with_another_goal() {
    let source = b"x\n/a/g";
    let mut lexer = Lexer::new(&source[..], LexerOptions::script(), Interner::new());
    assert_eq!(
        lexer.next_token(Goal::Div).map(|t| t.kind),
        Ok(K::Identifier)
    );
    let after_x = lexer.checkpoint();
    let slash = lexer.next_token(Goal::Div).expect("a slash");
    assert_eq!((slash.kind, slash.newline_before), (K::Slash, true));
    let regexp = lexer
        .rescan(&slash, Goal::RegExp)
        .expect("a regular expression");
    assert_eq!((regexp.kind, regexp.start, regexp.end), (K::RegExp, 2, 6));
    assert!(regexp.newline_before);
    assert_eq!(lexer.next_token(Goal::Div).map(|t| t.kind), Ok(K::Eof));
    lexer.restore(after_x);
    let again = lexer
        .next_token(Goal::RegExp)
        .expect("a regular expression");
    assert_eq!(again, regexp);
    // An offset past the end restores to the end.
    lexer.restore(Checkpoint {
        offset: 100,
        newline_before: false,
    });
    assert_eq!(lexer.next_token(Goal::Div).map(|t| t.kind), Ok(K::Eof));
    assert!(lexer.text(2, 5).is_some_and(|text| text.eq_str("/a/")));
}

#[test]
fn unpaired_surrogates() {
    // In strings, templates, comments and regular expressions they are
    // ordinary characters; elsewhere they are errors.
    let source: Vec<u16> = [
        &[0x27, 0xD800, 0x27][..],         // '\uD800'
        &[0x20, 0x60, 0xDC00, 0x60],       // `\uDC00`
        &[0x20, 0x2F, 0x2F, 0xD800, 0x0A], // // comment
        &[0x2F, 0x2A, 0xDBFF, 0x2A, 0x2F], /* comment */
    ]
    .concat();
    let (tokens, _) = lex_units(&source, LexerOptions::script(), &mut |_| Goal::Div)
        .expect("surrogates in literals and comments");
    assert_eq!(tokens.len(), 2);
    assert_eq!(
        tokens[0].value,
        TokenValue::String(String16::from_units(&[0xD800]))
    );
    assert_eq!(template(&tokens[1]).raw, String16::from_units(&[0xDC00]));
    let regexp: [u16; 3] = [0x2F, 0xD800, 0x2F];
    let (tokens, _) = lex_units(&regexp, LexerOptions::script(), &mut |_| Goal::RegExp)
        .expect("a surrogate in a regular expression");
    assert_eq!(tokens[0].kind, K::RegExp);
    for bad in [
        &[0xD800u16][..],
        &[0x61, 0xDC00],
        &[0x31, 0xD800],
        &[0xDBFF, 0x61],
    ] {
        let result = lex_units(bad, LexerOptions::script(), &mut |_| Goal::Div);
        assert!(result.is_err(), "{bad:x?}");
    }
    // A pair is one code point: U+1D4B6 is ID_Start.
    let pair: [u16; 2] = [0xD835, 0xDCB6];
    let (tokens, interner) =
        lex_units(&pair, LexerOptions::script(), &mut |_| Goal::Div).expect("an identifier");
    let name = tokens[0].name().and_then(|n| interner.get(n));
    assert_eq!(name, Some(Str16::Wide(&pair)));
}

/// A source with every kind of token, for the truncation test.
const ALL_KINDS: &str = "#!hashbang\n<!-- c\n--> c\nvar \\u0061 = 0x1F + 0b1_0 + 0o7 + 017 + 08.5 \
    + 1_000.5e-3 + 12n + .5; let s = 'a\\x41\\u{1F600}\\07\\\n' + \"\\u0062\"; \
    /* block\n */ t = `x${ y }z${ {a: `q`} }w`; r = /[/\\]]+/giu.test(s); #p in o; \
    a?.b ?? c ??= d ** e >>>= f; \u{3c0} \u{e9} \u{1d4b6}";

/// The goal for [`ALL_KINDS`]: the template rule plus regular expressions
/// after `=`.
fn all_kinds_goal(tokens: &[Token]) -> Goal {
    if tokens.is_empty() {
        return Goal::HashbangOrRegExp;
    }
    match template_goal(tokens) {
        Goal::TemplateTail => Goal::TemplateTail,
        _ if tokens.last().is_some_and(|t| t.kind == K::Eq) => Goal::RegExp,
        _ => Goal::Div,
    }
}

#[test]
fn every_prefix_and_suffix_lexes_without_panic() {
    let (tokens, _) = lex_with(ALL_KINDS, LexerOptions::script(), all_kinds_goal)
        .expect("the full source is valid");
    assert_eq!(tokens.len(), 66);
    let wide: Vec<u16> = ALL_KINDS.encode_utf16().collect();
    for goal in [
        Goal::Div,
        Goal::RegExp,
        Goal::TemplateTail,
        Goal::HashbangOrRegExp,
    ] {
        for cut in 0..wide.len() {
            for part in [&wide[..cut], &wide[cut..]] {
                let mut lexer = Lexer::new(part, LexerOptions::script(), Interner::new());
                for _ in 0..=part.len() {
                    match lexer.next_token(goal) {
                        Ok(token) if token.kind != K::Eof => {
                            assert!(token.start < token.end && token.end as usize <= part.len());
                        }
                        _ => break,
                    }
                }
            }
        }
    }
}

// --- Hostile inputs ---

const TEN_MB: usize = 10 * 1024 * 1024;

#[test]
fn hostile_ten_megabyte_line() {
    let source = "a+".repeat(TEN_MB / 2);
    let mut lexer = Lexer::new(source.as_bytes(), LexerOptions::script(), Interner::new());
    let mut count = 0;
    while lexer.next_token(Goal::Div).is_ok_and(|t| t.kind != K::Eof) {
        count += 1;
    }
    assert_eq!(count, TEN_MB);
}

#[test]
fn hostile_ten_megabyte_tokens() {
    for (prefix, body, suffix, kind) in [
        ("'", "x", "'", K::String),
        ("`", "x", "`", K::NoSubstitutionTemplate),
        ("/", "x", "/", K::RegExp),
        ("", "x", "", K::Identifier),
        ("", "9", "", K::Number),
        ("0x", "f", "", K::Number),
    ] {
        let source = format!("{prefix}{}{suffix}", body.repeat(TEN_MB));
        let token = Lexer::new(source.as_bytes(), LexerOptions::script(), Interner::new())
            .next_token(Goal::RegExp)
            .expect("one long token");
        assert_eq!((token.kind, token.end as usize), (kind, source.len()));
    }
    let digits = "9".repeat(TEN_MB);
    let token = Lexer::new(digits.as_bytes(), LexerOptions::script(), Interner::new())
        .next_token(Goal::Div)
        .expect("a long number");
    assert_eq!(token.value, TokenValue::Number(f64::INFINITY));
}

#[test]
fn hostile_nested_template_substitutions() {
    const DEPTH: usize = 100_000;
    let source = format!("{}x{}", "`${".repeat(DEPTH), "}`".repeat(DEPTH));
    let mut lexer = Lexer::new(source.as_bytes(), LexerOptions::script(), Interner::new());
    // The parser's part: after `x`, each `}` continues a template.
    let (mut heads, mut tails, mut closing) = (0, 0, false);
    loop {
        let goal = if closing {
            Goal::TemplateTail
        } else {
            Goal::Div
        };
        let token = lexer.next_token(goal).expect("valid nesting");
        match token.kind {
            K::TemplateHead => heads += 1,
            K::TemplateTail => tails += 1,
            K::Identifier => closing = true,
            K::Eof => break,
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!((heads, tails), (DEPTH, DEPTH));
}

#[test]
fn hostile_unterminated_comments() {
    let source = "/* ".repeat(TEN_MB / 3);
    let error = Lexer::new(source.as_bytes(), LexerOptions::script(), Interner::new())
        .next_token(Goal::RegExp)
        .expect_err("an unterminated comment");
    assert_eq!(error.offset, 0);
    // `/*/` ends a comment: a run of `/*` is comments and `*` punctuators.
    let source = "/*".repeat(TEN_MB / 2);
    let mut lexer = Lexer::new(source.as_bytes(), LexerOptions::script(), Interner::new());
    let mut stars = 0;
    while lexer.next_token(Goal::Div).is_ok_and(|t| t.kind == K::Star) {
        stars += 1;
    }
    assert!(stars > TEN_MB / 8, "{stars}");
}

#[test]
fn hostile_invalid_surrogates() {
    // Every unpaired surrogate position in a wide source.
    let mut units: Vec<u16> = Vec::new();
    for i in 0..2000u16 {
        units.push(0xD800 + (i % 0x800));
        units.push(0x61);
    }
    let result = lex_units(&units, LexerOptions::script(), &mut |_| Goal::Div);
    assert_eq!(result.map(|(t, _)| t.len()).map_err(|e| e.offset), Err(0));
    let mut in_string = vec![0x27];
    in_string.extend(&units);
    in_string.push(0x27);
    let (tokens, _) = lex_units(&in_string, LexerOptions::script(), &mut |_| Goal::Div)
        .expect("surrogates in a string");
    assert_eq!(tokens.len(), 1);
}
