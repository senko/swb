//! A benchmark of the regular expression matcher (M7 feature 8a): typical
//! patterns of the target scripts and patterns that backtrack much, over
//! long inputs. Each case finds all matches like a global `exec` loop.
//! `bench.js` runs the same cases in Node.js (`node --jitless
//! crates/js-regexp/examples/bench.js`, V8's regular expression
//! interpreter). Run with
//! `cargo run --release -p swb-js-regexp --example bench`.

use std::time::Instant;

use swb_js_regexp::{Flags, Matcher, Outcome, Regex, parse_flags};
use swb_js_text::{Str16, String16};

/// The cases: name, pattern, flags, input, repetitions. Keep in step with
/// `bench.js`.
fn cases() -> Vec<(&'static str, &'static str, &'static str, String, u32)> {
    let words = "The quick brown fox jumps over the lazy dog, again and again. ".repeat(2_000);
    let query = "a=1&bb=22&ccc=333&dddd=4444&".repeat(2_000);
    let html = "<div class=\"story\"><a href=\"/news/123\">Headline</a></div>\n".repeat(2_000);
    let spaces = format!(
        "{}{}{}",
        " ".repeat(1_000),
        "x".repeat(100_000),
        " ".repeat(1_000)
    );
    let emoji = "Text \u{1F600} and \u{1F44D}\u{1F3FD} and \u{1F1ED}\u{1F1F7} and \
                 \u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467} 1\u{FE0F}\u{20E3} end. "
        .repeat(2_000);
    vec![
        ("trim", "^\\s+|\\s+$", "g", spaces, 20),
        ("query", "([^&=]+)=([^&]*)", "g", query, 20),
        ("word", "\\b(?:fox|dog)\\b", "gi", words.clone(), 20),
        ("class", "[A-Z]", "g", words.clone(), 20),
        (
            "tag",
            "<a href=\"([^\"]*)\">(.*?)</a>",
            "g",
            html.clone(),
            20,
        ),
        ("literal", "Headline", "g", html, 20),
        (
            "alternation",
            "(?:quick|lazy|again)\\s(\\w+)",
            "g",
            words.clone(),
            20,
        ),
        ("dot-star", "o.*z", "g", words, 20),
        ("nested", "(a+)+b", "", "a".repeat(22), 1),
        (
            "redos",
            "^(\\w+\\s?)*$",
            "",
            format!("{}!", "aaaa ".repeat(4)),
            50,
        ),
        (
            "backref",
            "(\\w+)\\s\\1",
            "g",
            "hello hello world world ".repeat(5_000),
            20,
        ),
        (
            "lookbehind",
            "(?<=\\$)\\d+",
            "g",
            "cost $12, then $345, and 678 ".repeat(5_000),
            20,
        ),
        ("emoji", "\\p{RGI_Emoji}", "gv", emoji.clone(), 5),
        ("emoji-iv", "\\p{RGI_Emoji}", "giv", emoji, 5),
    ]
}

fn main() {
    println!(
        "{:<12} {:>10} {:>10} {:>12}",
        "case", "matches", "ms", "steps"
    );
    for (name, pattern, flags, input, runs) in cases() {
        let flags = parse_flags(Str16::Latin1(flags.as_bytes())).unwrap_or(Flags::empty());
        let regex = match Regex::new(String16::from(pattern).as_str16(), flags) {
            Ok(regex) => regex,
            Err(error) => {
                println!("{name:<12} error: {error}");
                continue;
            }
        };
        let input = String16::from(input.as_str());
        let global = flags.contains(Flags::GLOBAL);
        let start = Instant::now();
        let (mut matches, mut steps) = (0, 0);
        for _ in 0..runs {
            (matches, steps) = (0, 0);
            let mut at = 0;
            while at <= input.len() {
                let mut matcher = Matcher::new(&regex, at);
                let outcome = loop {
                    match matcher.run(&regex, input.as_str16(), 1 << 20, 1 << 30) {
                        Ok(Outcome::Pending) => {}
                        Ok(outcome) => break outcome,
                        Err(_) => break Outcome::NoMatch,
                    }
                };
                steps += matcher.steps();
                let Outcome::Match(m) = outcome else {
                    break;
                };
                matches += 1;
                let (s, e) = m.get(0).unwrap_or((at, at));
                if !global {
                    break;
                }
                at = if e == s { e + 1 } else { e };
            }
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0 / f64::from(runs);
        println!("{name:<12} {matches:>10} {ms:>10.3} {steps:>12}");
    }
}
