//! Hostile patterns and inputs (ADR 0026 section 9): deep nesting, huge
//! counts and alternations, long inputs, catastrophic backtracking and a
//! backtrack stack at its memory limit. None may panic, recurse in Rust
//! or run without bound in one call of the matcher. The tests run on the
//! 2 MiB stack of a test thread, which also shows that nothing recurses
//! with the nesting.

use std::time::{Duration, Instant};

use swb_js_regexp::{Error, Flags, Matcher, Outcome, Regex, SyntaxKind, check_literal};
use swb_js_text::{Str16, String16};

const MEMORY: usize = 256 << 20;

fn s16(text: &str) -> String16 {
    String16::from(text)
}

fn compile(pattern: &str, flags: Flags) -> Result<Regex, Error> {
    Regex::new(s16(pattern).as_str16(), flags)
}

fn first_match(pattern: &str, input: &str) -> Option<(usize, usize)> {
    let regex = compile(pattern, Flags::empty()).expect("a valid pattern");
    regex
        .exec(s16(input).as_str16(), 0, 1 << 32, MEMORY)
        .expect("within the limits")
        .and_then(|m| m.get(0))
}

#[test]
fn deep_nesting_is_an_error() {
    let n = 100_000;
    let cases = [
        ("(".repeat(n) + &")".repeat(n), SyntaxKind::TooManyCaptures),
        ("(?:".repeat(n) + &")".repeat(n), SyntaxKind::TooLarge),
        ("(?=".repeat(n) + &")".repeat(n), SyntaxKind::TooLarge),
        ("(?<=".repeat(n) + &")".repeat(n), SyntaxKind::TooLarge),
    ];
    for (pattern, kind) in cases {
        assert_eq!(
            compile(&pattern, Flags::empty()).err(),
            Some(Error::Syntax(kind))
        );
        let literal = check_literal(s16(&pattern).as_str16(), Str16::Latin1(b""));
        assert_eq!(literal.err(), Some(Error::Syntax(kind)));
    }
    let classes = "[".repeat(n) + &"]".repeat(n);
    assert_eq!(
        compile(&classes, Flags::UNICODE_SETS).err(),
        Some(Error::Syntax(SyntaxKind::TooLarge))
    );
    // Within the limit, deep nesting works.
    let deep = "(?:".repeat(10_000) + "a" + &")".repeat(10_000);
    assert_eq!(first_match(&deep, "xa"), Some((1, 2)));
    let deep = "(".repeat(10_000) + "a" + &")".repeat(10_000) + "*";
    assert_eq!(first_match(&deep, "aa"), Some((0, 2)));
}

#[test]
fn huge_counts() {
    assert_eq!(first_match("a{0,4294967295}", "aaa"), Some((0, 3)));
    assert_eq!(first_match("a{4294967295}", "aaa"), None);
    assert_eq!(first_match("(?:a?){1000000}", "aaa"), Some((0, 3)));
    assert_eq!(first_match("(?:){4294967296}", "a"), Some((0, 0)));
    assert_eq!(first_match("(?:a|b){2,99999999999}", "abab"), Some((0, 4)));
    assert_eq!(first_match("(?:(?:a{2,3}){2,3}){1000}", "aaaa"), None);
}

#[test]
fn a_million_alternatives() {
    let pattern = "a|".repeat(1_000_000) + "b";
    assert_eq!(first_match(&pattern, "xb"), Some((1, 2)));
    let groups = "(a)|".repeat(30_000) + "(b)";
    let regex = compile(&groups, Flags::empty()).expect("valid");
    let m = regex
        .exec(Str16::Latin1(b"b"), 0, 1 << 32, MEMORY)
        .expect("within the limits")
        .expect("a match");
    assert_eq!(m.get(30_001), Some((0, 1)));
}

#[test]
fn ten_million_units() {
    let mut input = String::from("a");
    input.push_str(&"x".repeat(10_000_000));
    assert_eq!(first_match("a.*b", &input), None);
    assert_eq!(first_match("x$", &input), Some((10_000_000, 10_000_001)));
    assert_eq!(first_match("a(x+)", &input), Some((0, 10_000_001)));
    assert_eq!(first_match("q", &input), None);
    let start = Instant::now();
    assert_eq!(first_match("(x)\\1{4999999}", &input), Some((1, 5_000_001)));
    assert!(start.elapsed() < Duration::from_secs(20));
}

/// Runs a match in slices of `slice` steps for at most `calls` calls and
/// checks that each call does bounded work.
fn run_in_slices(regex: &Regex, input: Str16<'_>, slice: u32, calls: usize) -> Option<Outcome> {
    let mut matcher = Matcher::new(regex, 0);
    for _ in 0..calls {
        let before = matcher.steps();
        let start = Instant::now();
        let outcome = matcher
            .run(regex, input, slice, MEMORY)
            .expect("within the memory limit");
        let used = matcher.steps() - before;
        assert!(
            used <= u64::from(slice),
            "{used} steps in a call of {slice}"
        );
        assert!(
            start.elapsed() < Duration::from_millis(500),
            "a call took {:?}",
            start.elapsed()
        );
        if outcome != Outcome::Pending {
            return Some(outcome);
        }
    }
    None
}

#[test]
fn catastrophic_backtracking_stops_at_the_callers_limit() {
    let input = s16(&"a".repeat(10_000));
    for pattern in ["(a*)*b", "(a+)+b", "(?:a|aa)+b", "(a|a)*b", "(\\w+\\s?)*!"] {
        let regex = compile(pattern, Flags::empty()).expect("valid");
        // 100 calls of 100,000 steps: the caller's time check would stop
        // the script here.
        assert_eq!(
            run_in_slices(&regex, input.as_str16(), 100_000, 100),
            None,
            "{pattern}"
        );
    }
}

#[test]
fn one_step_slices_give_the_same_results() {
    let cases = [
        ("(z)((a+)?(b+)?(c))*", "zaacbbbcac"),
        ("(?<=(\\d+)(\\d+))$", "1053"),
        ("(a*?)*b\\1", "aab"),
        ("x(.)\\1*y", "xaaaay"),
        ("[\\q{abc|d}x]+", "xabcdx"),
    ];
    for (pattern, input) in cases {
        let flags = if pattern.contains("\\q") {
            Flags::UNICODE_SETS
        } else {
            Flags::empty()
        };
        let regex = compile(pattern, flags).expect("valid");
        let input = s16(input);
        let whole = regex
            .exec(input.as_str16(), 0, 1 << 32, MEMORY)
            .expect("ok");
        let sliced = run_in_slices(&regex, input.as_str16(), 1, 100_000).expect("an end");
        match sliced {
            Outcome::Match(m) => assert_eq!(Some(m), whole, "{pattern}"),
            Outcome::NoMatch => assert_eq!(None, whole, "{pattern}"),
            Outcome::Pending => unreachable!(),
        }
    }
}

#[test]
fn backtrack_stack_at_the_memory_limit() {
    let input = s16(&"x".repeat(5_000_000));
    for pattern in ["(?:x|y)*z", "(x)*z", "(?:x(?=x)|y)+z"] {
        let regex = compile(pattern, Flags::empty()).expect("valid");
        let result = regex.exec(input.as_str16(), 0, 1 << 32, 1 << 20);
        assert_eq!(result, Err(Error::MemoryLimit), "{pattern}");
    }
    let input = s16(&"x".repeat(1_000_000));
    let regex = compile("(?:x|y)*$", Flags::empty()).expect("valid");
    let m = regex
        .exec(input.as_str16(), 0, 1 << 32, MEMORY)
        .expect("fits in 256 MiB")
        .expect("a match");
    assert_eq!(m.get(0), Some((0, 1_000_000)));
}

#[test]
fn resumed_input_must_not_change() {
    let regex = compile("(a*)*b", Flags::empty()).expect("valid");
    let input = s16(&"a".repeat(100));
    let mut matcher = Matcher::new(&regex, 0);
    assert_eq!(
        matcher.run(&regex, input.as_str16(), 10, MEMORY),
        Ok(Outcome::Pending)
    );
    assert_eq!(
        matcher.run(&regex, Str16::Latin1(b"aa"), 10, MEMORY),
        Err(Error::InputChanged)
    );
}

/// The characters `0x20000 + 2 * i`: distinct and never adjacent, so each
/// is a range of its own.
fn spread_chars(n: u32) -> Vec<char> {
    (0..n)
        .map(|i| char::from_u32(0x20000 + 2 * i).expect("a code point"))
        .collect()
}

fn flags_of(letters: &str) -> Flags {
    swb_js_regexp::parse_flags(Str16::Latin1(letters.as_bytes())).expect("valid flags")
}

fn first_match_in(regex: &Regex, input: &str) -> Option<(usize, usize)> {
    regex
        .exec(s16(input).as_str16(), 0, 1 << 32, MEMORY)
        .expect("within the limits")
        .and_then(|m| m.get(0))
}

#[test]
fn big_classes_with_v_parse_in_linear_time() {
    let chars = spread_chars(100_000);
    let list: String = chars.iter().collect();
    let alternatives = chars
        .iter()
        .map(char::to_string)
        .collect::<Vec<_>>()
        .join("|");
    let nested = "[".to_string() + &"[x]".repeat(50_000) + "]";
    let cases = [
        (format!("[{list}]"), Flags::UNICODE_SETS, "\u{20000}"),
        (
            format!("[\\q{{{alternatives}}}]"),
            Flags::UNICODE_SETS,
            "\u{20000}",
        ),
        (nested, flags_of("iv"), "x"),
    ];
    for (pattern, flags, matching) in cases {
        let start = Instant::now();
        let regex = compile(&pattern, flags).expect("valid");
        // It took 11 to 35 s when each element copied the whole set.
        let elapsed = start.elapsed();
        assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
        let found = first_match_in(&regex, &format!("-{matching}"));
        assert_eq!(found, Some((1, 1 + matching.encode_utf16().count())));
    }
}

#[test]
fn duplicate_names_use_memory_linear_in_the_pattern() {
    let n = 20_000;
    let pattern = "(?<a>x)|".repeat(n) + &"\\k<a>".repeat(n);
    let start = Instant::now();
    let regex = compile(&pattern, Flags::empty()).expect("valid");
    // 1.1 s and 3.1 GB when each reference copied the list of groups.
    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
    assert!(regex.heap_bytes() < 8 << 20, "{}", regex.heap_bytes());
    assert_eq!(first_match_in(&regex, "x"), Some((0, 1)));
}

#[test]
fn repeated_escapes_and_classes_with_i_share_their_sets() {
    let start = Instant::now();
    let flags = flags_of("iu");
    let regex = compile(&"\\W".repeat(50_000), flags).expect("valid");
    // 5.4 s and 515 MB with a closure and a set for each occurrence.
    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
    assert!(regex.heap_bytes() < 4 << 20, "{}", regex.heap_bytes());
    let start = Instant::now();
    let regex =
        compile(&"[\u{100}-\u{1000}a-z]".repeat(100_000), Flags::IGNORE_CASE).expect("valid");
    // 5.6 s before.
    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
    assert!(regex.heap_bytes() < 16 << 20, "{}", regex.heap_bytes());
    let found = first_match_in(&regex, &"A".repeat(100_000));
    assert_eq!(found, Some((0, 100_000)));
}

#[test]
fn popping_choice_points_costs_steps() {
    // 30,000 lookahead frames that a failure pops one after the other
    // without resuming anywhere.
    let pattern = "(?=".repeat(30_000) + "a" + &")".repeat(30_000);
    let regex = compile(&pattern, Flags::STICKY).expect("valid");
    let input = s16("b");
    let run = |slice: u32| {
        let mut matcher = Matcher::new(&regex, 0);
        let mut calls = 0u64;
        loop {
            calls += 1;
            match matcher.run(&regex, input.as_str16(), slice, MEMORY) {
                Ok(Outcome::Pending) => {}
                Ok(Outcome::NoMatch) => return (matcher.steps(), calls),
                other => panic!("{other:?}"),
            }
        }
    };
    let (whole, _) = run(u32::MAX);
    let (sliced, calls) = run(100);
    assert_eq!(sliced, whole);
    // Every call does at most 100 steps, the popping included.
    assert!(calls >= whole / 100, "{calls} calls for {whole} steps");
}

#[test]
fn slots_and_undo_log_count_against_the_memory_limit() {
    // 20,000 groups: 60,000 slots of 12 bytes each.
    let regex = compile(&"(a)".repeat(20_000), Flags::empty()).expect("valid");
    let input = s16("a");
    let mut matcher = Matcher::new(&regex, 0);
    assert_eq!(
        matcher.run(&regex, input.as_str16(), 1000, 100_000),
        Err(Error::MemoryLimit)
    );
    // The undo log stops before it grows past the limit.
    let regex = compile("(?:(a)|b)*c", Flags::empty()).expect("valid");
    let input = s16(&"a".repeat(1_000_000));
    let limit = 1 << 20;
    let mut matcher = Matcher::new(&regex, 0);
    let result = matcher.run(&regex, input.as_str16(), u32::MAX, limit);
    assert_eq!(result, Err(Error::MemoryLimit));
    assert!(matcher.heap_bytes() <= limit, "{}", matcher.heap_bytes());
}
