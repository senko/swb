//! Errors of regular expressions. The messages are V8's (measured in
//! Chromium 148, `tools/probes/js-regexp.json`).

use swb_js_text::Str16;

/// Why a pattern is not valid. Each kind has V8's message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SyntaxKind {
    /// A `(` without `)`.
    UnterminatedGroup,
    /// A `)` without `(`.
    UnmatchedParen,
    /// `(?` followed by something that does not start a group.
    InvalidGroup,
    /// A malformed group name.
    InvalidCaptureGroupName,
    /// Two groups with the same name that can both take part in a match.
    DuplicateCaptureGroupName,
    /// A malformed `\u` escape (also `\c` without a letter with `u`).
    InvalidUnicodeEscape,
    /// A modifier letter twice in `(?ims-ims:...)`.
    RepeatedFlag,
    /// `(?-:...)`.
    InvalidFlagGroup,
    /// Two `-` in a modifier group.
    MultipleDashes,
    /// A quantifier without an atom.
    NothingToRepeat,
    /// A quantifier after a lookbehind, or after a lookahead with `u` or `v`.
    InvalidQuantifier,
    /// `{n,m}` with `n > m`.
    QuantifierOutOfOrder,
    /// A `{` that does not complete a quantifier (with `u` or `v`).
    IncompleteQuantifier,
    /// A `{`, `}` or `]` outside a quantifier or class (with `u` or `v`).
    LoneQuantifierBrackets,
    /// A `\` at the end of the pattern.
    EscapeAtEnd,
    /// An escape that the grammar does not allow (with `u` or `v`).
    InvalidEscape,
    /// `\k` without a group name.
    InvalidNamedReference,
    /// `\k<name>` with a name that no group has.
    InvalidNamedCaptureReferenced,
    /// A decimal escape that is not allowed (`\00` with `u`, `\1` in a
    /// class with `u`).
    InvalidDecimalEscape,
    /// An unknown property or value in `\p{...}` outside a class.
    InvalidPropertyName,
    /// An unknown property or value in `\p{...}` in a class.
    InvalidClassPropertyName,
    /// A `[` without `]`.
    UnterminatedCharacterClass,
    /// A range whose start is above its end.
    RangeOutOfOrder,
    /// A range with a class escape at one end (with `u` or `v`).
    InvalidCharacterClass,
    /// A wrong set operation in a class with `v`.
    InvalidClassSetOperation,
    /// A character that a class with `v` does not allow unescaped.
    InvalidClassSetCharacter,
    /// A negated class with `v` that may contain strings.
    NegatedClassMayContainStrings,
    /// More capture groups than the limit.
    TooManyCaptures,
    /// The pattern nests too deep or its program would be too large.
    TooLarge,
}

impl SyntaxKind {
    /// V8's message for the kind.
    pub fn message(self) -> &'static str {
        match self {
            SyntaxKind::UnterminatedGroup => "Unterminated group",
            SyntaxKind::UnmatchedParen => "Unmatched ')'",
            SyntaxKind::InvalidGroup => "Invalid group",
            SyntaxKind::InvalidCaptureGroupName => "Invalid capture group name",
            SyntaxKind::DuplicateCaptureGroupName => "Duplicate capture group name",
            SyntaxKind::InvalidUnicodeEscape => "Invalid Unicode escape",
            SyntaxKind::RepeatedFlag => "Repeated flag in flag group",
            SyntaxKind::InvalidFlagGroup => "Invalid flag group",
            SyntaxKind::MultipleDashes => "Multiple dashes in flag group",
            SyntaxKind::NothingToRepeat => "Nothing to repeat",
            SyntaxKind::InvalidQuantifier => "Invalid quantifier",
            SyntaxKind::QuantifierOutOfOrder => "numbers out of order in {} quantifier",
            SyntaxKind::IncompleteQuantifier => "Incomplete quantifier",
            SyntaxKind::LoneQuantifierBrackets => "Lone quantifier brackets",
            SyntaxKind::EscapeAtEnd => "\\ at end of pattern",
            SyntaxKind::InvalidEscape => "Invalid escape",
            SyntaxKind::InvalidNamedReference => "Invalid named reference",
            SyntaxKind::InvalidNamedCaptureReferenced => "Invalid named capture referenced",
            SyntaxKind::InvalidDecimalEscape => "Invalid decimal escape",
            SyntaxKind::InvalidPropertyName => "Invalid property name",
            SyntaxKind::InvalidClassPropertyName => "Invalid property name in character class",
            SyntaxKind::UnterminatedCharacterClass => "Unterminated character class",
            SyntaxKind::RangeOutOfOrder => "Range out of order in character class",
            SyntaxKind::InvalidCharacterClass => "Invalid character class",
            SyntaxKind::InvalidClassSetOperation => "Invalid set operation in character class",
            SyntaxKind::InvalidClassSetCharacter => "Invalid character in character class",
            SyntaxKind::NegatedClassMayContainStrings => {
                "Negated character class may contain strings"
            }
            SyntaxKind::TooManyCaptures => "Too many captures",
            SyntaxKind::TooLarge => "Regular expression too large",
        }
    }
}

/// An error of a regular expression.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// An unknown flag letter, a letter twice, or both `u` and `v`.
    #[error("Invalid regular expression flags")]
    InvalidFlags,
    /// The pattern is not valid: a `SyntaxError`. The message of the
    /// error that JavaScript sees names the pattern and the flags
    /// ([`Error::message`]).
    #[error("{}", .0.message())]
    Syntax(SyntaxKind),
    /// The backtrack stack of a match needs more memory than the caller
    /// allows.
    #[error("Regular expression backtracking exceeded the memory limit")]
    MemoryLimit,
    /// [`crate::Regex::exec`] ran out of steps.
    #[error("Regular expression match exceeded its step limit")]
    StepLimit,
    /// The caller passed an input of another length than before to a
    /// match that it resumed.
    #[error("The input of a resumed match changed")]
    InputChanged,
}

/// V8's text for a pattern that does not compile: `Invalid regular
/// expression: /PATTERN/FLAGS: REASON`.
pub fn invalid_pattern_message(
    pattern: &str,
    flags: &str,
    reason: impl std::fmt::Display,
) -> String {
    format!("Invalid regular expression: /{pattern}/{flags}: {reason}")
}

impl Error {
    /// The message of the `SyntaxError` that JavaScript sees, as V8 writes
    /// it: for an invalid pattern `Invalid regular expression:
    /// /PATTERN/FLAGS: REASON` (the pattern as it is, not escaped), for
    /// invalid flags of a literal `Invalid regular expression flags`. The
    /// `RegExp` constructor has its own message for invalid flags.
    pub fn message(&self, pattern: Str16<'_>, flags: Str16<'_>) -> String {
        match self {
            Error::Syntax(kind) => invalid_pattern_message(
                &pattern.to_string_lossy(),
                &flags.to_string_lossy(),
                kind.message(),
            ),
            other => other.to_string(),
        }
    }
}
