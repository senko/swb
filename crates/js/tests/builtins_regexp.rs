//! Cases for the `RegExp` built-in and the string methods that take a
//! regular expression (M7 feature 8b): the measured Chromium cases
//! (`tools/probes/js-regexp-builtin.json`), and regular expressions,
//! results and iterators with collections inside callbacks, getters,
//! `exec` and `valueOf`. Each case also runs in the GC stress mode.

mod common;

use common::{check_cases, run};

/// The snippets of `tools/probes/js-regexp-builtin.json` with Chromium
/// 148's results (`data/regexp_builtin_cases.expected`; the script and the
/// expected output are made by `data/gen-regexp-builtin-cases.js` from the
/// output of `just probe`), normally and in the GC stress mode.
#[test]
fn chromium_cases() {
    check_chromium(
        include_str!("data/regexp_builtin_cases.js"),
        include_str!("data/regexp_builtin_cases.expected"),
        &[false, true],
    );
}

/// The Chromium cases with large inputs (long strings, a backtrack stack
/// beyond the limit), without the stress mode.
#[test]
fn chromium_cases_heavy() {
    check_chromium(
        include_str!("data/regexp_builtin_cases_heavy.js"),
        include_str!("data/regexp_builtin_cases_heavy.expected"),
        &[false],
    );
}

fn check_chromium(script: &str, expected: &str, modes: &[bool]) {
    for &stress in modes {
        let actual = run(script, stress);
        let failures: Vec<String> = actual
            .lines()
            .zip(expected.lines())
            .filter(|(actual, expected)| actual != expected)
            .map(|(actual, expected)| format!("expected: {expected}\nactual:   {actual}"))
            .collect();
        assert_eq!(
            actual.lines().count(),
            expected.lines().count(),
            "number of lines (stress: {stress})"
        );
        assert!(
            failures.is_empty(),
            "stress: {stress}\n{}",
            failures.join("\n")
        );
    }
}

/// Regular expressions, results and iterators across collections (the
/// expected output is that of Node.js 22).
#[test]
fn regexps_in_callbacks_and_getters() {
    check_cases(&[(
        include_str!("data/regexp_gc.js"),
        include_str!("data/regexp_gc.expected").trim_end_matches('\n'),
    )]);
}

/// A literal is a new object at each evaluation, and the compiled program
/// is shared: the objects of one literal stay independent.
#[test]
fn literals_make_new_objects() {
    check_cases(&[(
        "var all = []; for (var i = 0; i < 3; i++) all.push(/a/g); \
         all[0].lastIndex = 5; all[1].compile('b'); \
         print(all[0] !== all[1], all[0].lastIndex, all[1].source, all[2].source, all[2].lastIndex);",
        "true 5 b a 0",
    )]);
}

/// `@@split` converts `limit` after it makes the splitter, and calls the
/// splitter's `exec` after that: a `valueOf` of the limit that replaces
/// `RegExp.prototype.exec` is seen (Chromium 148: 4 calls, in both
/// cases).
#[test]
fn split_limit_runs_before_exec() {
    check_cases(&[
        (
            "const re = /b/, log = [], old = RegExp.prototype.exec; \
             re[Symbol.split]('abcb', { valueOf() { \
               RegExp.prototype.exec = function (s) { log.push(1); return old.call(this, s); }; \
               return 10; } }); \
             print(log.length);",
            "4",
        ),
        (
            "const old = RegExp.prototype.exec; let n = 0; \
             const parts = 'abcb'.split(/b/, { valueOf() { \
               RegExp.prototype.exec = function (s) { n++; return old.call(this, s); }; \
               return 10; } }); \
             print(n, JSON.stringify(parts));",
            "4 [\"a\",\"c\",\"\"]",
        ),
    ]);
}

/// Property escapes work in a literal and through the constructor, and
/// `v` allows properties of strings (the Chromium cases have more).
#[test]
fn property_escapes_match() {
    let out = run(
        "print(new RegExp('\\\\p{L}+', 'u').exec('12h\\u00e9llo')[0]); \
         print(/\\p{Lu}/u.test('a'), /\\p{Lu}/iu.test('a')); \
         print(/\\p{RGI_Emoji}/v.test('\\u{1F1ED}\\u{1F1F7}'));",
        false,
    );
    assert_eq!(out, "h\u{e9}llo\nfalse true\ntrue", "{out}");
    let out = run("new RegExp('\\\\p{Foo}', 'u')", false);
    assert!(out.contains("Invalid property name"), "{out}");
}
