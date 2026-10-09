//! Tokens of the lexical grammar (ECMA-262 clause 12) and the goal
//! symbols with which the parser asks for them.

use swb_js_regexp::Flags;
use swb_js_text::String16;

use crate::SyntaxError;
use crate::interner::{NameId, names};

/// The goal symbol of the lexical grammar (§12): which tokens can start at
/// the current position. Only the parser knows it, from the syntactic
/// context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    /// `InputElementDiv`: `/` and `/=` are punctuators, `}` is a punctuator.
    Div,
    /// `InputElementRegExp`: `/` starts a regular expression literal.
    RegExp,
    /// `InputElementRegExpOrTemplateTail`: `/` starts a regular expression
    /// literal, `}` continues a template.
    RegExpOrTemplateTail,
    /// `InputElementTemplateTail`: `/` is a punctuator, `}` continues a
    /// template.
    TemplateTail,
    /// `InputElementHashbangOrRegExp`: the start of a script or module. A
    /// hashbang comment (`#!`) is allowed at offset 0, `-->` starts a
    /// comment (Annex B.1.1), and `/` starts a regular expression literal.
    HashbangOrRegExp,
}

impl Goal {
    /// Whether `/` starts a regular expression literal.
    pub(crate) fn allows_regexp(self) -> bool {
        matches!(
            self,
            Goal::RegExp | Goal::RegExpOrTemplateTail | Goal::HashbangOrRegExp
        )
    }

    /// Whether `}` continues a template literal.
    pub(crate) fn allows_template_tail(self) -> bool {
        matches!(self, Goal::RegExpOrTemplateTail | Goal::TemplateTail)
    }
}

/// The kind of a token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// The end of the source.
    Eof,
    /// `IdentifierName` that is not a reserved word, or a reserved word
    /// written with an escape ([`Token::escaped`]). Contextual keywords
    /// (`let`, `async`, `of` and others) are identifiers. Value:
    /// [`TokenValue::Name`].
    Identifier,
    /// `PrivateIdentifier` (`#name`). Value: [`TokenValue::Name`] without the
    /// `#`.
    PrivateName,
    /// `NumericLiteral` without the `n` suffix. Value: [`TokenValue::Number`].
    Number,
    /// `NumericLiteral` with the `n` suffix. Value: [`TokenValue::BigInt`].
    BigInt,
    /// `StringLiteral`. Value: [`TokenValue::String`].
    String,
    /// `NoSubstitutionTemplate`: `` `...` ``. Value: [`TokenValue::Template`].
    NoSubstitutionTemplate,
    /// `TemplateHead`: `` `...${ ``. Value: [`TokenValue::Template`].
    TemplateHead,
    /// `TemplateMiddle`: `}...${`. Value: [`TokenValue::Template`].
    TemplateMiddle,
    /// `TemplateTail`: `` }...` ``. Value: [`TokenValue::Template`].
    TemplateTail,
    /// `RegularExpressionLiteral`. Value: [`TokenValue::RegExp`].
    RegExp,

    // ReservedWord (§12.7.2), in the order of the well-known names.
    /// `await`
    Await,
    /// `break`
    Break,
    /// `case`
    Case,
    /// `catch`
    Catch,
    /// `class`
    Class,
    /// `const`
    Const,
    /// `continue`
    Continue,
    /// `debugger`
    Debugger,
    /// `default`
    Default,
    /// `delete`
    Delete,
    /// `do`
    Do,
    /// `else`
    Else,
    /// `enum`
    Enum,
    /// `export`
    Export,
    /// `extends`
    Extends,
    /// `false`
    False,
    /// `finally`
    Finally,
    /// `for`
    For,
    /// `function`
    Function,
    /// `if`
    If,
    /// `import`
    Import,
    /// `in`
    In,
    /// `instanceof`
    Instanceof,
    /// `new`
    New,
    /// `null`
    Null,
    /// `return`
    Return,
    /// `super`
    Super,
    /// `switch`
    Switch,
    /// `this`
    This,
    /// `throw`
    Throw,
    /// `true`
    True,
    /// `try`
    Try,
    /// `typeof`
    Typeof,
    /// `var`
    Var,
    /// `void`
    Void,
    /// `while`
    While,
    /// `with`
    With,
    /// `yield`
    Yield,

    // Punctuator, DivPunctuator, RightBracePunctuator (§12.8).
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `.`
    Dot,
    /// `...`
    Ellipsis,
    /// `;`
    Semicolon,
    /// `,`
    Comma,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    LtEq,
    /// `>=`
    GtEq,
    /// `==`
    EqEq,
    /// `!=`
    NotEq,
    /// `===`
    EqEqEq,
    /// `!==`
    NotEqEq,
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `%`
    Percent,
    /// `**`
    StarStar,
    /// `++`
    PlusPlus,
    /// `--`
    MinusMinus,
    /// `<<`
    Shl,
    /// `>>`
    Shr,
    /// `>>>`
    UShr,
    /// `&`
    Amp,
    /// `|`
    Pipe,
    /// `^`
    Caret,
    /// `!`
    Bang,
    /// `~`
    Tilde,
    /// `&&`
    AmpAmp,
    /// `||`
    PipePipe,
    /// `??`
    QuestionQuestion,
    /// `?`
    Question,
    /// `?.` (`OptionalChainingPunctuator`: not followed by a decimal digit)
    QuestionDot,
    /// `:`
    Colon,
    /// `=`
    Eq,
    /// `+=`
    PlusEq,
    /// `-=`
    MinusEq,
    /// `*=`
    StarEq,
    /// `/=`
    SlashEq,
    /// `%=`
    PercentEq,
    /// `**=`
    StarStarEq,
    /// `<<=`
    ShlEq,
    /// `>>=`
    ShrEq,
    /// `>>>=`
    UShrEq,
    /// `&=`
    AmpEq,
    /// `|=`
    PipeEq,
    /// `^=`
    CaretEq,
    /// `&&=`
    AmpAmpEq,
    /// `||=`
    PipePipeEq,
    /// `??=`
    QuestionQuestionEq,
    /// `=>`
    Arrow,
}

macro_rules! keywords {
    ($($kind:ident = $name:ident,)*) => {
        /// The reserved words, in the order of their name ids.
        const KEYWORDS: &[TokenKind] = &[$(TokenKind::$kind,)*];

        impl TokenKind {
            /// The name of a reserved word, or `None` for another kind.
            pub fn keyword_name(self) -> Option<NameId> {
                match self {
                    $(TokenKind::$kind => Some(names::$name),)*
                    _ => None,
                }
            }
        }
    };
}

keywords! {
    Await = AWAIT,
    Break = BREAK,
    Case = CASE,
    Catch = CATCH,
    Class = CLASS,
    Const = CONST,
    Continue = CONTINUE,
    Debugger = DEBUGGER,
    Default = DEFAULT,
    Delete = DELETE,
    Do = DO,
    Else = ELSE,
    Enum = ENUM,
    Export = EXPORT,
    Extends = EXTENDS,
    False = FALSE,
    Finally = FINALLY,
    For = FOR,
    Function = FUNCTION,
    If = IF,
    Import = IMPORT,
    In = IN,
    Instanceof = INSTANCEOF,
    New = NEW,
    Null = NULL,
    Return = RETURN,
    Super = SUPER,
    Switch = SWITCH,
    This = THIS,
    Throw = THROW,
    True = TRUE,
    Try = TRY,
    Typeof = TYPEOF,
    Var = VAR,
    Void = VOID,
    While = WHILE,
    With = WITH,
    Yield = YIELD,
}

impl TokenKind {
    /// The reserved word whose name is `name`, or `None`. The parser uses
    /// it for identifiers with escapes, which must not be reserved words
    /// (§13.1.1).
    pub fn keyword_from_name(name: NameId) -> Option<TokenKind> {
        if name.index() < names::RESERVED_WORDS {
            KEYWORDS.get(name.index() as usize).copied()
        } else {
            None
        }
    }

    /// Whether this is a reserved word.
    pub fn is_keyword(self) -> bool {
        self.keyword_name().is_some()
    }

    /// The source text of a reserved word or punctuator, or a description
    /// of another kind (for error messages).
    pub fn text(self) -> &'static str {
        if let Some(name) = self.keyword_name() {
            return names::TEXTS
                .get(name.index() as usize)
                .copied()
                .unwrap_or("keyword");
        }
        match self {
            TokenKind::Eof => "end of input",
            TokenKind::Identifier => "identifier",
            TokenKind::PrivateName => "private name",
            TokenKind::Number | TokenKind::BigInt => "number",
            TokenKind::String => "string",
            TokenKind::NoSubstitutionTemplate
            | TokenKind::TemplateHead
            | TokenKind::TemplateMiddle
            | TokenKind::TemplateTail => "template string",
            TokenKind::RegExp => "regular expression",
            TokenKind::LBrace => "{",
            TokenKind::RBrace => "}",
            TokenKind::LParen => "(",
            TokenKind::RParen => ")",
            TokenKind::LBracket => "[",
            TokenKind::RBracket => "]",
            TokenKind::Dot => ".",
            TokenKind::Ellipsis => "...",
            TokenKind::Semicolon => ";",
            TokenKind::Comma => ",",
            TokenKind::Lt => "<",
            TokenKind::Gt => ">",
            TokenKind::LtEq => "<=",
            TokenKind::GtEq => ">=",
            TokenKind::EqEq => "==",
            TokenKind::NotEq => "!=",
            TokenKind::EqEqEq => "===",
            TokenKind::NotEqEq => "!==",
            TokenKind::Plus => "+",
            TokenKind::Minus => "-",
            TokenKind::Star => "*",
            TokenKind::Slash => "/",
            TokenKind::Percent => "%",
            TokenKind::StarStar => "**",
            TokenKind::PlusPlus => "++",
            TokenKind::MinusMinus => "--",
            TokenKind::Shl => "<<",
            TokenKind::Shr => ">>",
            TokenKind::UShr => ">>>",
            TokenKind::Amp => "&",
            TokenKind::Pipe => "|",
            TokenKind::Caret => "^",
            TokenKind::Bang => "!",
            TokenKind::Tilde => "~",
            TokenKind::AmpAmp => "&&",
            TokenKind::PipePipe => "||",
            TokenKind::QuestionQuestion => "??",
            TokenKind::Question => "?",
            TokenKind::QuestionDot => "?.",
            TokenKind::Colon => ":",
            TokenKind::Eq => "=",
            TokenKind::PlusEq => "+=",
            TokenKind::MinusEq => "-=",
            TokenKind::StarEq => "*=",
            TokenKind::SlashEq => "/=",
            TokenKind::PercentEq => "%=",
            TokenKind::StarStarEq => "**=",
            TokenKind::ShlEq => "<<=",
            TokenKind::ShrEq => ">>=",
            TokenKind::UShrEq => ">>>=",
            TokenKind::AmpEq => "&=",
            TokenKind::PipeEq => "|=",
            TokenKind::CaretEq => "^=",
            TokenKind::AmpAmpEq => "&&=",
            TokenKind::PipePipeEq => "||=",
            TokenKind::QuestionQuestionEq => "??=",
            TokenKind::Arrow => "=>",
            _ => "keyword",
        }
    }
}

/// The reserved word spelled by `text` (ASCII), or `None`.
pub(crate) fn keyword_kind(text: &[u8]) -> Option<TokenKind> {
    // All reserved words are 2 to 10 lowercase letters.
    if !(2..=10).contains(&text.len()) || !text[0].is_ascii_lowercase() {
        return None;
    }
    let kind = match text {
        b"await" => TokenKind::Await,
        b"break" => TokenKind::Break,
        b"case" => TokenKind::Case,
        b"catch" => TokenKind::Catch,
        b"class" => TokenKind::Class,
        b"const" => TokenKind::Const,
        b"continue" => TokenKind::Continue,
        b"debugger" => TokenKind::Debugger,
        b"default" => TokenKind::Default,
        b"delete" => TokenKind::Delete,
        b"do" => TokenKind::Do,
        b"else" => TokenKind::Else,
        b"enum" => TokenKind::Enum,
        b"export" => TokenKind::Export,
        b"extends" => TokenKind::Extends,
        b"false" => TokenKind::False,
        b"finally" => TokenKind::Finally,
        b"for" => TokenKind::For,
        b"function" => TokenKind::Function,
        b"if" => TokenKind::If,
        b"import" => TokenKind::Import,
        b"in" => TokenKind::In,
        b"instanceof" => TokenKind::Instanceof,
        b"new" => TokenKind::New,
        b"null" => TokenKind::Null,
        b"return" => TokenKind::Return,
        b"super" => TokenKind::Super,
        b"switch" => TokenKind::Switch,
        b"this" => TokenKind::This,
        b"throw" => TokenKind::Throw,
        b"true" => TokenKind::True,
        b"try" => TokenKind::Try,
        b"typeof" => TokenKind::Typeof,
        b"var" => TokenKind::Var,
        b"void" => TokenKind::Void,
        b"while" => TokenKind::While,
        b"with" => TokenKind::With,
        b"yield" => TokenKind::Yield,
        _ => return None,
    };
    Some(kind)
}

/// The value of a template part.
#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    /// The template value (TV, §12.9.6.1). An invalid escape sequence
    /// makes it absent (`undefined` in a tagged template); the error is
    /// the one that the parser reports for a template without a tag.
    pub cooked: Result<String16, SyntaxError>,
    /// The template raw value (TRV), with CR LF and CR normalized to LF.
    pub raw: String16,
}

/// The value of a token.
#[derive(Clone, Debug, PartialEq)]
pub enum TokenValue {
    /// No value (reserved words, punctuators, the end).
    None,
    /// An identifier or private name: the `StringValue`, with escapes
    /// decoded.
    Name(NameId),
    /// A number.
    Number(f64),
    /// The text of a `BigInt` literal: the digits with the radix prefix
    /// (`0x`, `0o`, `0b`), without separators and without the `n`.
    BigInt(Box<str>),
    /// The string value (SV) of a string literal.
    String(String16),
    /// A template part.
    Template(Box<Template>),
    /// A regular expression literal: the body is the source range
    /// `start + 1..body_end`, the flags follow the closing `/`.
    RegExp {
        /// The offset of the closing `/`.
        body_end: u32,
        /// The checked flags.
        flags: Flags,
    },
}

/// A token with its source range.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    /// The kind.
    pub kind: TokenKind,
    /// The offset of the first code unit.
    pub start: u32,
    /// The offset after the last code unit.
    pub end: u32,
    /// Whether a line terminator (or a comment that contains one) comes
    /// between the previous token and this one; for automatic semicolon
    /// insertion (§12.10) and the `[no LineTerminator here]` rules.
    pub newline_before: bool,
    /// An identifier or private name that contains a Unicode escape.
    pub escaped: bool,
    /// The Annex B form of a numeric or string literal that is a syntax
    /// error in strict mode code, if any.
    pub legacy: Legacy,
    /// The value.
    pub value: TokenValue,
}

/// An Annex B form of a numeric or string literal that strict mode code
/// does not allow (§12.9.3.1, §12.9.4.1). Chromium gives each kind its own
/// error message.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Legacy {
    /// No legacy form.
    #[default]
    None,
    /// A legacy octal integer literal: `017`.
    OctalInteger,
    /// A decimal literal with a leading zero: `08`, `019.5`.
    LeadingZeroDecimal,
    /// A string with a legacy octal escape sequence: `'\07'`, `'\08'`.
    /// If a string has several legacy escapes, the first one decides.
    OctalEscape,
    /// A string with `\8` or `\9`.
    EightOrNine,
}

impl Token {
    /// The name of an identifier or private name.
    pub fn name(&self) -> Option<NameId> {
        match self.value {
            TokenValue::Name(name) => Some(name),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keywords_match_their_names() {
        assert_eq!(KEYWORDS.len() as u32, names::RESERVED_WORDS);
        for (index, &kind) in KEYWORDS.iter().enumerate() {
            let name = kind.keyword_name().expect("every entry is a keyword");
            assert_eq!(name.index() as usize, index);
            assert_eq!(TokenKind::keyword_from_name(name), Some(kind));
            assert_eq!(keyword_kind(kind.text().as_bytes()), Some(kind));
        }
        assert_eq!(TokenKind::keyword_from_name(names::LET), None);
        assert_eq!(keyword_kind(b"let"), None);
        assert_eq!(keyword_kind(b"If"), None);
        assert_eq!(TokenKind::Arrow.text(), "=>");
        assert!(!TokenKind::Identifier.is_keyword());
    }
}
