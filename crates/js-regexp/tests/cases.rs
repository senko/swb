//! The regular expression cases measured in Chromium 148
//! (`tests/data/regexp_cases.json`, made by `tests/data/gen-regexp-cases.js`
//! from `tools/probes/js-regexp.json`): match results of `exec`, the
//! messages of invalid patterns and of invalid literals. Each case runs
//! the parser and the matcher directly.

use serde_json::Value;
use swb_js_regexp::{Error, Flags, Matcher, Outcome, Regex, check_literal, parse_flags};
use swb_js_text::Str16;

const DATA: &str = include_str!("data/regexp_cases.json");

/// Cases where swb differs from Chromium on purpose, with the reason.
const KNOWN: &[(&str, &str)] = &[
    // §22.2.2.9 applies MaybeSimpleCaseFolding to the whole
    // ClassStringDisjunction, so the single characters of `\q{...}` fold
    // with `iv`. Chromium 148 folds only the strings of other lengths.
    (
        "/[\\q{K|ss}]/iv \"\u{212a}\"",
        "single character of \\q{} is folded (spec)",
    ),
    (
        "/[\\q{\u{df}}]/iv \"\u{1e9e}\"",
        "single character of \\q{} is folded (spec)",
    ),
    (
        "/[\\q{a|bc}]/iv \"A\"",
        "single character of \\q{} is folded (spec)",
    ),
    // §22.2.2.9.8 accepts every value of PropertyValueAliases.txt, and
    // `Hrkt` (Katakana_Or_Hiragana) is listed there. Chromium 148 rejects it.
    ("/\\p{sc=Hrkt}/u", "Hrkt is a Script value (spec)"),
    ("/\\p{scx=Hrkt}/u", "Hrkt is a Script value (spec)"),
    (
        "/\\p{sc=Katakana_Or_Hiragana}/u",
        "Katakana_Or_Hiragana is a Script value (spec)",
    ),
    (
        "/\\p{scx=Katakana_Or_Hiragana}/u",
        "Katakana_Or_Hiragana is a Script value (spec)",
    ),
    // `\p{ASCII}` with `iv`: §22.2.2.9 applies MaybeSimpleCaseFolding to the
    // set, so U+017F and U+212A (which fold to `s` and `k`) match it, and
    // CharacterComplement uses AllCharacters (the fixed points of the
    // folding) for `\P{ASCII}`, `[^...]` and `--`. Chromium 148 does not.
    (
        "/\\p{ASCII}/iv \"\u{17f}\"",
        "ASCII with iv: MaybeSimpleCaseFolding (spec)",
    ),
    (
        "/\\P{ASCII}/iv \"\u{17f}\"",
        "ASCII with iv: MaybeSimpleCaseFolding (spec)",
    ),
    (
        "/[^\\p{ASCII}]/iv \"\u{17f}\"",
        "ASCII with iv: MaybeSimpleCaseFolding (spec)",
    ),
    (
        "/[\\p{Lu}--\\p{ASCII}]/iv \"\u{212a}\"",
        "ASCII with iv: MaybeSimpleCaseFolding (spec)",
    ),
    (
        "/[\\p{L}&&\\p{ASCII}]/iv \"\u{17f}\"",
        "ASCII with iv: MaybeSimpleCaseFolding (spec)",
    ),
    (
        "/[\\p{L}&&\\p{ASCII}]/iv \"\u{212a}\"",
        "ASCII with iv: MaybeSimpleCaseFolding (spec)",
    ),
];

/// A string of the data file: a JSON string, or an array of code units
/// for text with lone surrogates.
fn units(value: &Value) -> Vec<u16> {
    match value {
        Value::String(s) => s.encode_utf16().collect(),
        Value::Array(items) => items
            .iter()
            .map(|v| v.as_u64().expect("a code unit") as u16)
            .collect(),
        other => panic!("not a string: {other}"),
    }
}

fn text(units: &[u16]) -> Str16<'_> {
    Str16::Wide(units)
}

fn show(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

fn data() -> Value {
    serde_json::from_str(DATA).expect("the data file is JSON")
}

fn known(key: &str) -> bool {
    KNOWN.iter().any(|(k, _)| *k == key)
}

/// The JSON form of a match, as the probe writes it.
fn result_of(
    regex: &Regex,
    input: &[u16],
    flags: Flags,
    last_index: usize,
) -> Result<Value, Error> {
    let start = if flags.contains(Flags::GLOBAL) || flags.contains(Flags::STICKY) {
        last_index
    } else {
        0
    };
    let found = regex.exec(text(input), start, 50_000_000, 64 << 20)?;
    // The same match in slices of one step.
    let mut matcher = Matcher::new(regex, start);
    let sliced = loop {
        match matcher.run(regex, text(input), 1, 64 << 20)? {
            Outcome::Pending => {}
            Outcome::Match(m) => break Some(m),
            Outcome::NoMatch => break None,
        }
    };
    assert_eq!(sliced, found, "one-step slices");
    // The same match on the one-byte width.
    if let Ok(bytes) = input
        .iter()
        .map(|&u| u8::try_from(u))
        .collect::<Result<Vec<u8>, _>>()
    {
        let narrow = regex.exec(Str16::Latin1(&bytes), start, 50_000_000, 64 << 20)?;
        assert_eq!(narrow, found, "Latin-1 input");
    }
    let Some(m) = found else {
        return Ok(Value::Null);
    };
    let captures: Vec<Value> = m
        .spans()
        .iter()
        .map(|s| match s {
            Some((a, b)) => serde_json::json!([a, b]),
            None => Value::Null,
        })
        .collect();
    Ok(serde_json::json!({ "index": m.get(0).map_or(0, |s| s.0), "captures": captures }))
}

#[test]
fn exec_matches_chromium() {
    let data = data();
    let mut failures = Vec::new();
    let cases = data["exec"].as_array().expect("exec cases");
    for case in cases {
        let pattern = units(&case["pattern"]);
        let input = units(&case["input"]);
        let flag_text = case["flags"].as_str().expect("flags");
        let key = format!("/{}/{} {:?}", show(&pattern), flag_text, show(&input));
        let expected = &case["result"];
        let flags = parse_flags(Str16::Latin1(flag_text.as_bytes())).expect("valid flags");
        let regex = match Regex::new(text(&pattern), flags) {
            Ok(regex) => regex,
            Err(error) => {
                if expected.get("error").is_none() && !known(&key) {
                    failures.push(format!("{key}: compile error {error}"));
                }
                continue;
            }
        };
        if let Some(error) = expected.get("error") {
            failures.push(format!("{key}: compiled, Chromium: {error}"));
            continue;
        }
        let last_index = case["lastIndex"].as_u64().unwrap_or(0) as usize;
        let actual = match result_of(&regex, &input, flags, last_index) {
            Ok(v) => v,
            Err(error) => {
                failures.push(format!("{key}: match error {error}"));
                continue;
            }
        };
        let wanted = if expected.get("match").is_some() {
            Value::Null
        } else {
            serde_json::json!({ "index": expected["index"], "captures": expected["captures"] })
        };
        if actual != wanted && !known(&key) {
            failures.push(format!("{key}: got {actual}, Chromium {wanted}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

#[test]
fn syntax_errors_match_chromium() {
    let data = data();
    let mut failures = Vec::new();
    let cases = data["syntax"].as_array().expect("syntax cases");
    for case in cases {
        let pattern = units(&case["pattern"]);
        let flag_text = case["flags"].as_str().expect("flags");
        let flag_units: Vec<u16> = flag_text.encode_utf16().collect();
        let key = format!("/{}/{}", show(&pattern), flag_text);
        let expected = show(&units(&case["result"]));
        let expected = expected.as_str();
        let actual = match check_literal(text(&pattern), text(&flag_units)) {
            Ok(_) => "ok".to_string(),
            Err(Error::InvalidFlags) => {
                format!("SyntaxError: Invalid flags supplied to RegExp constructor '{flag_text}'")
            }
            Err(error) => format!(
                "SyntaxError: {}",
                error.message(text(&pattern), text(&flag_units))
            ),
        };
        if actual != expected && !known(&key) {
            failures.push(format!("{key}: got {actual:?}, Chromium {expected:?}"));
        }
        // The full parse that builds the tree finds the same errors.
        if let Ok(flags) = parse_flags(text(&flag_units)) {
            let compiled = match Regex::new(text(&pattern), flags) {
                Ok(_) => "ok".to_string(),
                Err(error) => format!(
                    "SyntaxError: {}",
                    error.message(text(&pattern), text(&flag_units))
                ),
            };
            if compiled != actual {
                failures.push(format!("{key}: compiled {compiled:?}, checked {actual:?}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

#[test]
fn literal_errors_match_chromium() {
    let data = data();
    let mut failures = Vec::new();
    let cases = data["literals"].as_array().expect("literal cases");
    for case in cases {
        let source = units(&case["source"]);
        let key = show(&source);
        let expected = show(&units(&case["result"]));
        let expected = expected.as_str();
        // The lexer finds the end of the body and reports a missing `/`
        // and escapes in the flags itself (`crates/js-syntax`).
        if expected.ends_with("missing /") || key.contains("/\\u") || key.ends_with("\\u0069") {
            continue;
        }
        let end = source
            .iter()
            .rposition(|&u| u == u16::from(b'/'))
            .expect("a literal");
        let (body, flags) = (&source[1..end], &source[end + 1..]);
        let actual = match check_literal(text(body), text(flags)) {
            Ok(_) => "ok".to_string(),
            Err(error) => format!("SyntaxError: {}", error.message(text(body), text(flags))),
        };
        if actual != expected && !known(&key) {
            failures.push(format!("{key}: got {actual:?}, Chromium {expected:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}
