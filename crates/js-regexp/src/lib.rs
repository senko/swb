//! Regular expressions for the JavaScript engine (ADR 0026 section 10):
//! pattern parser, compiler and backtracking matcher.
//!
//! - [`check_literal`] checks a regular expression literal for the
//!   front end (§13.2.7.2, `IsValidRegularExpressionLiteral`).
//! - [`Regex::new`] parses and compiles a pattern into a program that is a
//!   plain Rust value.
//! - [`Matcher`] runs a program on an input in slices of steps, so that
//!   the caller can run its time check between them.
//!
//! The parser builds a tree (`ast.rs`) with an explicit stack, the
//! compiler (`compile.rs`) emits instructions (`program.rs`) and the
//! matcher (`exec.rs`) follows ECMA-262 §22.2.2 with a backtrack stack and
//! an undo log. None of them recurses in Rust.

mod ast;
mod charset;
mod compile;
mod error;
mod exec;
mod parser;
mod program;

use swb_js_text::Str16;

pub use error::{Error, SyntaxKind, invalid_pattern_message};
pub use exec::Matcher;

/// The flags of a regular expression (§22.2.6.4: `d`, `g`, `i`, `m`, `s`,
/// `u`, `v`, `y`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Flags(u8);

impl Flags {
    /// `d`: match indices.
    pub const HAS_INDICES: Flags = Flags(1 << 0);
    /// `g`: global.
    pub const GLOBAL: Flags = Flags(1 << 1);
    /// `i`: ignore case.
    pub const IGNORE_CASE: Flags = Flags(1 << 2);
    /// `m`: multiline.
    pub const MULTILINE: Flags = Flags(1 << 3);
    /// `s`: `.` matches line terminators.
    pub const DOT_ALL: Flags = Flags(1 << 4);
    /// `u`: Unicode.
    pub const UNICODE: Flags = Flags(1 << 5);
    /// `v`: Unicode sets.
    pub const UNICODE_SETS: Flags = Flags(1 << 6);
    /// `y`: sticky.
    pub const STICKY: Flags = Flags(1 << 7);

    /// No flags.
    pub const fn empty() -> Flags {
        Flags(0)
    }

    /// Whether all flags of `other` are set.
    pub const fn contains(self, other: Flags) -> bool {
        self.0 & other.0 == other.0
    }

    /// The flag letters in the order of `RegExp.prototype.flags`
    /// (§22.2.6.4), with their flags.
    const LETTERS: [(Flags, char); 8] = [
        (Flags::HAS_INDICES, 'd'),
        (Flags::GLOBAL, 'g'),
        (Flags::IGNORE_CASE, 'i'),
        (Flags::MULTILINE, 'm'),
        (Flags::DOT_ALL, 's'),
        (Flags::UNICODE, 'u'),
        (Flags::UNICODE_SETS, 'v'),
        (Flags::STICKY, 'y'),
    ];

    /// The letters of the flags that are set, in the order of
    /// `RegExp.prototype.flags`.
    pub fn letters(self) -> impl Iterator<Item = char> {
        Self::LETTERS
            .into_iter()
            .filter(move |&(flag, _)| self.contains(flag))
            .map(|(_, letter)| letter)
    }

    /// The flag of a flag letter, or `None` for another code unit.
    fn from_letter(unit: u16) -> Option<Flags> {
        let letter = char::from_u32(u32::from(unit))?;
        Self::LETTERS
            .into_iter()
            .find(|&(_, l)| l == letter)
            .map(|(flag, _)| flag)
    }
}

/// Parses the flags of a regular expression: only the letters of
/// [`Flags`], none twice, and not both `u` and `v` (§22.2.3.3,
/// `RegExpInitialize`; §13.2.7.2, `IsValidRegularExpressionLiteral`).
pub fn parse_flags(text: Str16<'_>) -> Result<Flags, Error> {
    let mut flags = Flags::empty();
    for unit in text.units() {
        let flag = Flags::from_letter(unit).ok_or(Error::InvalidFlags)?;
        if flags.contains(flag) {
            return Err(Error::InvalidFlags);
        }
        flags.0 |= flag.0;
    }
    if flags.contains(Flags::UNICODE) && flags.contains(Flags::UNICODE_SETS) {
        return Err(Error::InvalidFlags);
    }
    Ok(flags)
}

/// Checks a regular expression literal (§13.2.7.2,
/// `IsValidRegularExpressionLiteral`): the flags and the early errors of
/// the pattern (§22.2.1.1). Returns the flags. [`Error::message`] gives the
/// message of the `SyntaxError`.
pub fn check_literal(pattern: Str16<'_>, flags: Str16<'_>) -> Result<Flags, Error> {
    let flags = parse_flags(flags)?;
    parser::validate(pattern, flags).map_err(Error::Syntax)?;
    Ok(flags)
}

/// A compiled regular expression. It owns its program and keeps no
/// reference to the pattern text.
#[derive(Clone, Debug)]
pub struct Regex {
    program: program::Program,
    flags: Flags,
    group_names: Box<[Option<Box<[u16]>>]>,
}

impl Regex {
    /// Parses and compiles a pattern (§22.2.3.3, `RegExpInitialize` steps 10
    /// to 16). The errors are [`Error::Syntax`] (a `SyntaxError`, also for
    /// the limits on captures, nesting and program size).
    pub fn new(pattern: Str16<'_>, flags: Flags) -> Result<Regex, Error> {
        let ast = parser::parse(pattern, flags).map_err(Error::Syntax)?;
        let program = compile::compile(&ast, flags)?;
        let group_names = ast
            .group_names
            .into_iter()
            .map(|name| name.map(Vec::into_boxed_slice))
            .collect();
        Ok(Regex {
            program,
            flags,
            group_names,
        })
    }

    /// The flags.
    pub fn flags(&self) -> Flags {
        self.flags
    }

    /// The name of each capture group as UTF-16, in order (index 0 is
    /// group 1); `None` for a group without a name. Duplicate names
    /// (in different alternatives) appear once for each group.
    pub fn group_names(&self) -> &[Option<Box<[u16]>>] {
        &self.group_names
    }

    /// Whether some group has a name (the result then has `groups`).
    pub fn has_named_groups(&self) -> bool {
        self.group_names.iter().any(Option::is_some)
    }

    /// The approximate number of bytes that the compiled program uses on
    /// the heap.
    pub fn heap_bytes(&self) -> usize {
        self.program.heap_bytes()
            + self
                .group_names
                .iter()
                .map(|n| 16 + n.as_ref().map_or(0, |n| n.len() * 2))
                .sum::<usize>()
    }

    /// Runs a match from `start` to its end, with at most `max_steps`
    /// steps (then [`Error::StepLimit`]) and the memory limit. For tests
    /// and tools; the engine uses [`Matcher`] with its time check.
    pub fn exec(
        &self,
        input: Str16<'_>,
        start: usize,
        max_steps: u64,
        memory_limit: usize,
    ) -> Result<Option<Captures>, Error> {
        let mut matcher = Matcher::new(self, start);
        let mut left = max_steps;
        loop {
            let slice = left.min(1 << 20) as u32;
            match matcher.run(self, input, slice, memory_limit)? {
                Outcome::Match(captures) => return Ok(Some(captures)),
                Outcome::NoMatch => return Ok(None),
                Outcome::Pending => {
                    left -= u64::from(slice);
                    if left == 0 {
                        return Err(Error::StepLimit);
                    }
                }
            }
        }
    }
}

/// The capture groups of a match: start and end (code-unit indices) of
/// the whole match (index 0) and of each group, or `None` for a group that
/// did not take part.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Captures {
    spans: Vec<Option<(usize, usize)>>,
}

impl Captures {
    /// The span of group `index` (0 is the whole match).
    pub fn get(&self, index: usize) -> Option<(usize, usize)> {
        self.spans.get(index).copied().flatten()
    }

    /// The number of entries: the group count plus one.
    pub fn len(&self) -> usize {
        self.spans.len()
    }

    /// Whether there are no entries (never: the whole match is entry 0).
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// The spans in order.
    pub fn spans(&self) -> &[Option<(usize, usize)>] {
        &self.spans
    }
}

/// The result of a call to [`Matcher::run`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The first match from the start position.
    Match(Captures),
    /// No match from the start position (or, with `y`, at it).
    NoMatch,
    /// The steps ran out; call again with the same input.
    Pending,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(text: &str) -> Result<Flags, Error> {
        parse_flags(Str16::Latin1(text.as_bytes()))
    }

    #[test]
    fn valid_flags() {
        assert_eq!(flags(""), Ok(Flags::empty()));
        let all = flags("dgimsuy").expect("all letters but v are valid together");
        for flag in [
            Flags::HAS_INDICES,
            Flags::GLOBAL,
            Flags::STICKY,
            Flags::UNICODE,
        ] {
            assert!(all.contains(flag));
        }
        assert!(!all.contains(Flags::UNICODE_SETS));
        assert!(flags("v").is_ok_and(|f| f.contains(Flags::UNICODE_SETS)));
    }

    #[test]
    fn letters_follow_the_flags_order() {
        let all = flags("ysmigd").expect("valid flags");
        assert_eq!(all.letters().collect::<String>(), "dgimsy");
        assert_eq!(Flags::empty().letters().count(), 0);
    }

    #[test]
    fn invalid_flags() {
        for text in ["gg", "x", "G", "uv", "gimg", " "] {
            assert_eq!(flags(text), Err(Error::InvalidFlags), "{text:?}");
        }
        assert_eq!(
            parse_flags(Str16::Wide(&[0x0067, 0x0131])),
            Err(Error::InvalidFlags)
        );
        assert_eq!(
            parse_flags(Str16::Wide(&[0x0167])),
            Err(Error::InvalidFlags)
        );
    }

    #[test]
    fn literal_patterns_are_checked() {
        let pattern = Str16::Latin1(b"(unclosed[");
        let error = check_literal(pattern, Str16::Latin1(b"g")).expect_err("unterminated");
        assert_eq!(
            error.message(pattern, Str16::Latin1(b"g")),
            "Invalid regular expression: /(unclosed[/g: Unterminated character class"
        );
        assert!(check_literal(Str16::Latin1(b"a(b)c"), Str16::Latin1(b"gi")).is_ok());
        assert_eq!(
            check_literal(Str16::Latin1(b"a"), Str16::Latin1(b"gg")),
            Err(Error::InvalidFlags)
        );
    }

    /// Runs a matcher in slices of one step until it has a result.
    fn run_to_end(matcher: &mut Matcher, regex: &Regex, input: Str16<'_>) -> Outcome {
        loop {
            match matcher.run(regex, input, 1, 1 << 20).expect("no limit") {
                Outcome::Pending => {}
                outcome => return outcome,
            }
        }
    }

    #[test]
    fn a_reset_matcher_matches_like_a_new_one() {
        let other = Regex::new(Str16::Latin1(b"x(y)(z)(w)|q"), Flags::empty()).expect("valid");
        let regex = Regex::new(Str16::Latin1(b"(a+)(b)?|(?=c)"), Flags::empty()).expect("valid");
        let mut matcher = Matcher::new(&other, 0);
        let first = run_to_end(&mut matcher, &other, Str16::Latin1(b"--xyzw"));
        assert!(matches!(first, Outcome::Match(_)));
        let input = Str16::Latin1(b"-aab-aac");
        for start in 0..=input.len() + 1 {
            matcher.reset(&regex, start);
            let reused = run_to_end(&mut matcher, &regex, input);
            let fresh = run_to_end(&mut Matcher::new(&regex, start), &regex, input);
            assert_eq!(reused, fresh, "start {start}");
        }
    }

    #[test]
    fn compiled_regex_reports_its_groups() {
        let re = Regex::new(Str16::Latin1(b"(?<a>x)|(?<a>y)(z)"), Flags::empty()).expect("valid");
        assert_eq!(re.group_names().len(), 3);
        let names: Vec<Option<&[u16]>> = re.group_names().iter().map(|n| n.as_deref()).collect();
        assert_eq!(names, [Some(&[0x61u16][..]), Some(&[0x61][..]), None]);
        assert!(re.has_named_groups());
        assert!(re.heap_bytes() > 0);
        let m = re
            .exec(Str16::Latin1(b"--yz"), 0, 1000, 1 << 20)
            .expect("no limit")
            .expect("a match");
        assert_eq!(m.spans(), &[Some((2, 4)), None, Some((2, 3)), Some((3, 4))]);
    }
}
